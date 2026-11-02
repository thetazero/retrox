//! Saved games (DESIGN §9 "Saves", §7.7 "Death").
//!
//! **One game, two files.** Every game has its own folder under
//! `<data dir>/saves/<game id>/`, holding:
//! - `save.json`, the game's one long save, written when the player saves
//!   from the pause menu; and
//! - `checkpoint.json`, the autosave, rewritten every time the player
//!   docks at a station or lands at a city.
//!
//! **A save is a snapshot of the whole game**: the galaxy sim's state
//! exactly as it stands (RNG streams, calendar queue and free lists
//! included, so the same future plays out after loading — §5.1) and the
//! player's: date, ship and fit, cargo, credits, ships in storage, and
//! whether they're docked, landed or in space and where.
//!
//! **File layout.** A small [`Header`] as a line of JSON (read alone to
//! list saves without parsing a whole galaxy), then, gzipped, the player's
//! state and the galaxy as a line of JSON each. A save from an incompatible version fails to load with a
//! message saying so, never a crash.

use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use retrox_sim::{BodyId, GalaxyState, SettlementId, SystemId};
use serde::{Deserialize, Serialize};

use crate::hangar::Player;
use crate::mining::Mining;
use crate::world::{Mode, Place, Ship, World};

/// Bump whenever a change to the client's or the sim's saved state would
/// make old saves load wrongly. Older formats are refused with a message.
pub const FORMAT: u32 = 1;

/// Which of a game's two files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// The game's one long save, written from the pause menu.
    Save,
    /// The autosave, written on docking and landing.
    Checkpoint,
}

impl Kind {
    pub fn file_name(self) -> &'static str {
        match self {
            Kind::Save => "save.json",
            Kind::Checkpoint => "checkpoint.json",
        }
    }
}

/// Which game is being played: the folder its saves go in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameId {
    /// The folder name: when the game was started and its seed.
    pub id: String,
    pub seed: u64,
    /// Unix seconds when the game was started.
    pub started: u64,
}

impl GameId {
    /// A new game, started now.
    pub fn new(seed: u64) -> Self {
        let started = now_unix();
        Self {
            id: format!("{started}-{seed}"),
            seed,
            started,
        }
    }
}

/// What the load screen shows about a save without reading the galaxy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    /// The client version that wrote it.
    pub version: String,
    pub game: GameId,
    pub kind: Kind,
    /// Unix seconds.
    pub saved: u64,
    /// Real seconds played in this game.
    pub played_s: f64,
    /// Galaxy date, as the HUD shows it.
    pub date: String,
    pub system: String,
    /// Where the player is: "docked at X", "landed at Y", "in space near Z".
    pub place: String,
    pub credits: f64,
    pub ship: String,
}

/// Where the player is, as saved. Transit (fast travel, a jump, the
/// landing animation) isn't saveable: checkpoints are written on arrival,
/// and the pause menu says why it can't save until then.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum SavedMode {
    InSpace,
    Docked(SettlementId),
    Landed { city: SettlementId, body: BodyId },
}

/// The player's side of a save.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlayerState {
    system: SystemId,
    /// Galaxy seconds, exactly (floats round-trip; see the workspace's
    /// `float_roundtrip`).
    clock_s: f64,
    ship: Ship,
    mode: SavedMode,
    target: Option<Place>,
    course: Option<SystemId>,
    cockpit: bool,
    player: Player,
    /// What the mining laser has done to every field, and the haul.
    #[serde(default)]
    mining: Mining,
    played_s: f64,
}

/// A loaded game.
pub struct Loaded {
    pub header: Header,
    pub world: World,
    pub played_s: f64,
}

/// Whether the game can be saved right now, or why not.
pub fn can_save(world: &World) -> Result<(), &'static str> {
    saved_mode(world).map(|_| ())
}

fn saved_mode(world: &World) -> Result<SavedMode, &'static str> {
    match world.mode {
        Mode::Normal => Ok(SavedMode::InSpace),
        Mode::Docked(s) => Ok(SavedMode::Docked(s)),
        Mode::Landed { city, body, timer } if timer <= 0.0 => Ok(SavedMode::Landed { city, body }),
        Mode::Landed { .. } => Err("can't save while landing"),
        Mode::FastTravel(_) => Err("can't save during fast travel"),
        Mode::Jumping { .. } => Err("can't save in a hyperlane jump"),
    }
}

/// Where the player is, in words.
pub fn place_text(world: &World) -> String {
    match world.mode {
        Mode::Docked(s) => format!("docked at {}", world.name(Place::Station(s))),
        Mode::Landed { city, .. } => format!(
            "landed at {}",
            world
                .galaxy
                .settlements
                .get(city)
                .map_or("(gone)", |s| s.name.as_str())
        ),
        _ => {
            let near = world
                .nav_places()
                .into_iter()
                .min_by(|a, b| world.distance_to(*a).total_cmp(&world.distance_to(*b)));
            match near {
                Some(p) => format!("in space near {}", world.name(p)),
                None => "in space".to_owned(),
            }
        }
    }
}

/// A game serialized but not yet compressed or written: taken on the main
/// thread (it needs the world), finished on a worker.
pub struct Snapshot {
    pub header: Header,
    /// The player's state and the galaxy, one JSON line each.
    body: String,
}

/// Serialize a game.
pub fn snapshot(
    world: &World,
    game: &GameId,
    kind: Kind,
    played_s: f64,
) -> Result<Snapshot, String> {
    let mode = saved_mode(world).map_err(str::to_owned)?;
    let header = Header {
        format: FORMAT,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        game: game.clone(),
        kind,
        saved: now_unix(),
        played_s,
        date: world.date().to_string(),
        system: world.galaxy.systems[world.system].name.clone(),
        place: place_text(world),
        credits: world.player.credits,
        ship: world.player.ship.name.clone(),
    };
    let state = PlayerState {
        system: world.system,
        clock_s: world.clock_s,
        ship: world.ship.clone(),
        mode,
        target: world.target,
        course: world.course,
        cockpit: world.cockpit,
        player: world.player.clone(),
        mining: world.mining.clone(),
        played_s,
    };
    let err = |e: serde_json::Error| format!("couldn't write the save: {e}");
    let mut body = serde_json::to_string(&state).map_err(err)?;
    body.push('\n');
    body.push_str(&serde_json::to_string(&world.galaxy).map_err(err)?);
    Ok(Snapshot { header, body })
}

impl Snapshot {
    /// The file's bytes: the header as a line of plain JSON, then the
    /// rest gzipped (a galaxy is tens of megabytes of JSON).
    pub fn encode(&self) -> Vec<u8> {
        use flate2::write::GzEncoder;
        let mut out = serde_json::to_vec(&self.header).expect("headers serialize");
        out.push(b'\n');
        let mut gz = GzEncoder::new(out, flate2::Compression::fast());
        gz.write_all(self.body.as_bytes())
            .expect("writing to memory");
        gz.finish().expect("writing to memory")
    }
}

/// Read a game back from a save file's bytes.
pub fn decode(bytes: &[u8]) -> Result<Loaded, String> {
    use std::io::Read as _;
    let split = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    let header = parse_header(std::str::from_utf8(&bytes[..split]).unwrap_or(""))?;
    let mut body = String::new();
    flate2::read::GzDecoder::new(bytes.get(split + 1..).unwrap_or(&[]))
        .read_to_string(&mut body)
        .map_err(|e| format!("This save is damaged and couldn't be read ({e})."))?;
    let mut lines = body.splitn(2, '\n');
    let unreadable = |e: serde_json::Error| {
        format!(
            "This save couldn't be read. It was made by retrox {} and may not match this \
             version ({e}).",
            header.version
        )
    };
    let state: PlayerState =
        serde_json::from_str(lines.next().unwrap_or("")).map_err(unreadable)?;
    let galaxy: GalaxyState =
        serde_json::from_str(lines.next().unwrap_or("")).map_err(unreadable)?;
    if galaxy.systems.get(state.system).is_none() {
        return Err("This save is damaged: its system isn't in its galaxy.".to_owned());
    }
    let mode = match state.mode {
        SavedMode::InSpace => Mode::Normal,
        SavedMode::Docked(s) => Mode::Docked(s),
        SavedMode::Landed { city, body } => Mode::Landed {
            city,
            body,
            timer: 0.0,
        },
    };
    let mut world = World::from_parts(
        galaxy,
        state.system,
        state.clock_s,
        state.ship,
        mode,
        state.player,
    );
    world.target = state.target;
    world.course = state.course;
    world.cockpit = state.cockpit;
    world.mining = state.mining;
    Ok(Loaded {
        header,
        world,
        played_s: state.played_s,
    })
}

fn parse_header(line: &str) -> Result<Header, String> {
    #[derive(Deserialize)]
    struct Format {
        format: u32,
        #[serde(default)]
        version: String,
    }
    let format: Format = serde_json::from_str(line)
        .map_err(|_| "This file isn't a retrox save, or it's damaged.".to_owned())?;
    if format.format != FORMAT {
        let age = if format.format < FORMAT {
            "an older"
        } else {
            "a newer"
        };
        return Err(format!(
            "This save was made by {age} version of retrox ({}) and can't be loaded by this one.",
            format.version
        ));
    }
    serde_json::from_str(line).map_err(|e| format!("This save's header is damaged ({e})."))
}

/// Where saves live: `$RETROX_DATA`, else `$XDG_DATA_HOME/retrox`, else
/// `~/.local/share/retrox`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RETROX_DATA") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("retrox")
}

pub fn saves_dir(data: &Path) -> PathBuf {
    data.join("saves")
}

/// Write a game's save or checkpoint, replacing the old one only once the
/// new one is fully written.
pub fn write(data: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let kind = snapshot.header.kind;
    let dir = saves_dir(data).join(&snapshot.header.game.id);
    let io = |e: std::io::Error| format!("couldn't write the save to {}: {e}", dir.display());
    std::fs::create_dir_all(&dir).map_err(io)?;
    let path = dir.join(kind.file_name());
    let tmp = dir.join(format!("{}.tmp", kind.file_name()));
    let mut file = std::fs::File::create(&tmp).map_err(io)?;
    file.write_all(&snapshot.encode()).map_err(io)?;
    file.sync_all().map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)?;
    Ok(())
}

/// Load a save file.
pub fn read(path: &Path) -> Result<Loaded, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
    decode(&bytes)
}

/// A save file on disk, and its header if it could be read.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub header: Result<Header, String>,
}

impl Entry {
    /// Unix seconds it was saved, or 0 if unknown.
    pub fn saved(&self) -> u64 {
        self.header.as_ref().map_or(0, |h| h.saved)
    }
}

/// Every save and checkpoint, newest first.
pub fn list(data: &Path) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Ok(games) = std::fs::read_dir(saves_dir(data)) else {
        return entries;
    };
    for game in games.flatten() {
        for kind in [Kind::Save, Kind::Checkpoint] {
            let path = game.path().join(kind.file_name());
            if !path.is_file() {
                continue;
            }
            let header = read_header(&path);
            entries.push(Entry { path, header });
        }
    }
    entries.sort_by(|a, b| b.saved().cmp(&a.saved()).then(a.path.cmp(&b.path)));
    entries
}

fn read_header(path: &Path) -> Result<Header, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("couldn't open: {e}"))?;
    let mut line = String::new();
    BufReader::new(file)
        .read_line(&mut line)
        .map_err(|e| format!("couldn't read: {e}"))?;
    parse_header(line.trim_end())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(unix: u64) -> String {
    let s = now_unix().saturating_sub(unix);
    match s {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86_400 => format!("{} h ago", s / 3600),
        _ => match s / 86_400 {
            1 => "yesterday".to_owned(),
            d => format!("{d} days ago"),
        },
    }
}

/// "2 h 05 min" of play.
pub fn played(seconds: f64) -> String {
    let m = (seconds / 60.0) as u64;
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02} min", m / 60, m % 60)
    }
}

/// A map saved as a list of pairs (JSON map keys must be strings), sorted
/// by key so the same state always saves the same.
pub mod pairs {
    use std::collections::HashMap;
    use std::hash::Hash;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K, V, S>(map: &HashMap<K, V>, s: S) -> Result<S::Ok, S::Error>
    where
        K: Serialize + Ord,
        V: Serialize,
        S: Serializer,
    {
        let mut pairs: Vec<(&K, &V)> = map.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        s.collect_seq(pairs)
    }

    pub fn deserialize<'de, K, V, D>(d: D) -> Result<HashMap<K, V>, D::Error>
    where
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}

/// A ship's fit as the ids of its hull, sections and modules, so saves
/// don't depend on the order of anything in `retrox-content`.
pub mod loadout {
    use retrox_sim::ship::{Loadout, Mount};
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Fit {
        hull: String,
        /// Per mount: its section and the module in each of its slots.
        mounts: Vec<Option<Section>>,
    }

    #[derive(Serialize, Deserialize)]
    struct Section {
        section: String,
        modules: Vec<Option<String>>,
    }

    pub fn serialize<S: Serializer>(fit: &Loadout, s: S) -> Result<S::Ok, S::Error> {
        Fit {
            hull: fit.hull().id.to_owned(),
            mounts: fit
                .mounts()
                .map(|(_, fitted)| {
                    fitted.map(|f| Section {
                        section: f.section.id.to_owned(),
                        modules: f.slots().map(|(_, m)| m.map(|m| m.id.to_owned())).collect(),
                    })
                })
                .collect(),
        }
        .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Loadout, D::Error> {
        let saved = Fit::deserialize(d)?;
        let hull = retrox_content::ships::hull(&saved.hull)
            .ok_or_else(|| D::Error::custom(format!("unknown hull '{}'", saved.hull)))?;
        let mut fit = Loadout::new(hull);
        for (i, section) in saved.mounts.iter().enumerate() {
            let Some(section) = section else { continue };
            match hull.mounts.get(i) {
                Some(Mount::Fixed(fixed)) if fixed.id == section.section => {}
                Some(Mount::Open { .. }) => {
                    let s = retrox_content::ships::section(&section.section).ok_or_else(|| {
                        D::Error::custom(format!("unknown section '{}'", section.section))
                    })?;
                    fit.set_section(i, s).map_err(D::Error::custom)?;
                }
                _ => {
                    return Err(D::Error::custom(format!(
                        "section '{}' doesn't fit mount {i} of the {}",
                        section.section, hull.name
                    )));
                }
            }
            for (j, module) in section.modules.iter().enumerate() {
                let Some(module) = module else { continue };
                let m = retrox_content::ships::module(module)
                    .ok_or_else(|| D::Error::custom(format!("unknown module '{module}'")))?;
                fit.fit(i, j, m).map_err(D::Error::custom)?;
            }
        }
        Ok(fit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hangar::{OwnedShip, StoredShip};
    use crate::world::{Action, Controls};
    use glam::{DQuat, DVec3};
    use retrox_sim::economy::Good;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("retrox-save-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Everything a save must bring back, compared field by field.
    fn assert_same(a: &World, b: &World) {
        assert_eq!(a.system, b.system);
        assert_eq!(a.clock_s.to_bits(), b.clock_s.to_bits());
        assert_eq!(a.date(), b.date());
        assert_eq!(a.ship, b.ship);
        assert_eq!(a.mode, b.mode);
        assert_eq!(a.target, b.target);
        assert_eq!(a.course, b.course);
        assert_eq!(a.player, b.player);
        assert_eq!(
            serde_json::to_string(&a.galaxy).unwrap(),
            serde_json::to_string(&b.galaxy).unwrap()
        );
    }

    /// A game in progress with something in every part of the player's
    /// state: cargo, a refitted ship, a ship in storage, money spent.
    fn game_in_progress() -> World {
        let mut world = World::new(3);
        // A hauler, with room for a contract's freight and some ore.
        world.player.ship.fit = retrox_content::ships::stock("hauler").unwrap();
        // A contract off the capital's board, before moving on.
        world.act(Action::AcceptContract(0));
        assert_eq!(world.player.contracts.len(), 1, "took a contract");
        world.start_at_station();
        world.act(Action::Wait(5));
        let here = world.docked_at().expect("docked");
        world.galaxy.settlements[here].economy.market.stock[Good::Ore] += 50.0;
        world.act(Action::Trade(Good::Ore, 4.0));
        assert!(world.player.cargo[Good::Ore] > 0.0, "bought some ore");
        let mut spare = retrox_content::ships::stock("hauler").unwrap();
        spare.remove(1, 0);
        world.player.stored.push(StoredShip {
            ship: OwnedShip {
                name: "Spare".into(),
                fit: spare,
            },
            at: here,
        });
        world.player.ship.name = "Zvezda".into();
        world.player.credits -= 1234.5;
        world
    }

    #[test]
    fn a_docked_game_roundtrips() {
        let world = game_in_progress();
        let game = GameId::new(3);
        let snap = snapshot(&world, &game, Kind::Save, 61.0).unwrap();
        let header = snap.header.clone();
        assert!(header.place.starts_with("docked at "));
        let loaded = decode(&snap.encode()).unwrap();
        assert_eq!(loaded.played_s, 61.0);
        assert_eq!(loaded.header, header);
        assert_same(&world, &loaded.world);
    }

    #[test]
    fn a_game_in_space_roundtrips() {
        let mut world = game_in_progress();
        world.act(Action::Undock);
        for _ in 0..30 {
            world.update(
                1.0 / 30.0,
                Controls {
                    stick: DVec3::new(0.3, -0.2, 0.1),
                    throttle_delta: 1.0,
                    ..Controls::default()
                },
            );
        }
        world.ship.rot = (world.ship.rot * DQuat::from_rotation_z(0.7)).normalize();
        world.act(Action::CycleTarget);
        let snap = snapshot(&world, &GameId::new(3), Kind::Save, 0.0).unwrap();
        assert!(snap.header.place.starts_with("in space"));
        let loaded = decode(&snap.encode()).unwrap().world;
        assert_eq!(loaded.mode, Mode::Normal);
        assert_same(&world, &loaded);
    }

    /// DESIGN §7.7: loading a checkpoint replays the same future unless the
    /// player acts differently.
    #[test]
    fn a_loaded_game_replays_the_same_future() {
        let mut world = game_in_progress();
        let snap = snapshot(&world, &GameId::new(3), Kind::Checkpoint, 0.0).unwrap();
        let mut replay = decode(&snap.encode()).unwrap().world;
        for w in [&mut world, &mut replay] {
            w.act(Action::Wait(45));
            w.act(Action::Trade(Good::Ore, -2.0));
            w.act(Action::Wait(30));
        }
        assert_same(&world, &replay);
    }

    #[test]
    fn mining_survives_a_save() {
        let mut world = World::new(3);
        assert!(world.start_at_field(), "seed 3 has a field to start in");
        world.act(Action::Scan);
        for _ in 0..600 {
            world.update(
                1.0 / 30.0,
                Controls {
                    fire: true,
                    ..Controls::default()
                },
            );
        }
        let snap = snapshot(&world, &GameId::new(3), Kind::Save, 0.0).unwrap();
        let loaded = decode(&snap.encode()).unwrap().world;
        let field = world.field_rocks.as_ref().expect("in a field").0;
        assert_eq!(loaded.field_rocks.as_ref().map(|f| f.0), Some(field));
        let json = |w: &World| serde_json::to_string(&w.mining).unwrap();
        assert!(
            world.mining.field(field).is_some(),
            "the laser did something"
        );
        assert_eq!(json(&world), json(&loaded));
        assert_eq!(world.mining.haul, loaded.mining.haul);
    }

    #[test]
    fn transit_is_not_saveable() {
        let mut world = World::new(3);
        world.start_at_station();
        world.mode = Mode::Jumping {
            to: world.system,
            timer: 1.0,
        };
        assert!(can_save(&world).is_err());
        assert!(snapshot(&world, &GameId::new(3), Kind::Save, 0.0).is_err());
    }

    #[test]
    fn files_list_newest_first_and_load() {
        let data = scratch("files");
        let world = game_in_progress();
        let game = GameId::new(3);
        let checkpoint = snapshot(&world, &game, Kind::Checkpoint, 1.0).unwrap();
        write(&data, &checkpoint).unwrap();
        let save = snapshot(&world, &game, Kind::Save, 2.0).unwrap();
        write(&data, &save).unwrap();
        let header = save.header;
        let entries = list(&data);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.header.is_ok()));
        let latest = entries.iter().find(|e| e.header.is_ok()).unwrap();
        let loaded = read(&latest.path).unwrap();
        assert_eq!(loaded.header.game, header.game);
        assert_same(&world, &loaded.world);
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn other_versions_fail_with_a_message() {
        let world = World::new(3);
        let mut snap = snapshot(&world, &GameId::new(3), Kind::Save, 0.0).unwrap();
        snap.header.format = FORMAT + 1;
        let err = decode(&snap.encode()).err().expect("refused");
        assert!(err.contains("newer version"), "{err}");

        // The sim's state changed shape under the same format number.
        snap.header.format = FORMAT;
        let state = snap.body.lines().next().unwrap().to_owned();
        snap.body = format!("{state}\n{{\"seed\":3}}");
        let err = decode(&snap.encode()).err().expect("refused");
        assert!(err.contains("couldn't be read"), "{err}");

        let mut bytes = serde_json::to_vec(&snap.header).unwrap();
        bytes.extend_from_slice(b"\nnot gzip");
        let err = decode(&bytes).err().expect("refused");
        assert!(err.contains("damaged"), "{err}");

        let err = decode(b"garbage").err().expect("refused");
        assert!(err.contains("isn't a retrox save"), "{err}");
    }

    #[test]
    fn unreadable_files_are_listed_not_loaded() {
        let data = scratch("unreadable");
        let dir = saves_dir(&data).join("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("save.json"), "{\"format\":0}\n").unwrap();
        let entries = list(&data);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].header.is_err());
        assert!(entries.iter().all(|e| e.header.is_err()));
        std::fs::remove_dir_all(&data).unwrap();
    }
}
