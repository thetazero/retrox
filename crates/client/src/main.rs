//! `retrox-client` — the game. Launch to the title screen; start a new
//! galaxy or carry on from a save. In play: fly your ship around a
//! procedurally generated star system, fast travel between planets, dock,
//! land, refit or buy ships in the hangar, and jump along hyperlanes while
//! the galaxy simulates around you. Esc pauses.
//!
//! Development flags skip the title and drop straight into a fresh game:
//! `--seed N`, `--station`, `--undocked`, `--cities`, `--map`, `--course <jumps>`,
//! `--ship <hull>`, `--screen <trade|hangar|shipyard|contracts|bounties>`,
//! `--combat` (by a raider band), `--brawl` (in a fight), `--behind-hull`
//! (in a fight, the target hidden by your own hull), `--lost <s>`
//! (the ship lost that long ago; with `--keys`, it has a checkpoint at a
//! station to go back to), `--rewind` (rewinding to a checkpoint),
//! `--arrival` (back at the checkpoint), `--harvesters <gas|ice>` (by a
//! harvester at work), `--skim` (in a giant's clouds), `--cities` (over a
//! planet with two cities), `--interdict` (flying past a gang's lair until
//! it interdicts), `--convoy` (alongside a trader fleet; with
//! `--hostile-convoy`, its faction hostile),
//! `--ship-view <deg>`, `--screenshot <path.ppm>`. `--menu <page>` opens a
//! menu page instead (`title`, `new`, `load`, `settings`, `handbook[:chapter]`, or `pause` or `quit`
//! over a fresh game), for screenshots. `--keys` plays a key script
//! (see [`Flags::keys`]), and `--remote <file>` plays turn by turn from
//! steps appended to a file (see [`Running::run_script`]).

mod audio;
mod bounty_ui;
mod combat;
mod contracts;
mod contracts_ui;
mod guide;
mod handbook;
mod hangar;
mod hangar_ui;
mod hud;
mod hulls;
mod lost;
mod menu;
mod meshes;
mod mining;
mod render;
mod save;
mod settings;
mod skim;
mod standing;
mod station_ui;
mod title;
mod trade;
mod traffic;
mod widgets;
mod world;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use glam::{DQuat, DVec2, DVec3, Mat4, Quat, Vec3};
use retrox_sim::StarClass;
use retrox_sim::{BodyKind, PlanetClass, SettlementId};
use slotmap::Key;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use menu::{Menu, MenuAction, Page};
use meshes::StationKind;
use render::{Instance, MeshId, Renderer, SkyVertex, Surface, VeinKind};
use save::{GameId, Kind};
use settings::Settings;
use world::{Action, Controls, Mode, Place, Screen, World};

/// Command-line flags, for development.
#[derive(Default)]
struct Flags {
    seed: Option<u64>,
    /// Start in space rather than docked (handy for testing).
    undocked: bool,
    /// In orbit of a planet with two cities, cargo bound for one (#116).
    cities: bool,
    /// Start docked at a station, in the first system that has one.
    station: bool,
    /// Start in an asteroid field, facing a rock, prospected.
    field: bool,
    /// Start in the field with the biggest sun in its sky, the sun behind
    /// the rock (#156).
    sunward: bool,
    /// `--quarry [back]`: start by a quarry, front- or backlit.
    quarry: Option<bool>,
    /// The window's size in logical pixels (`--size 1280x800`).
    size: Option<(f64, f64)>,
    /// Start in space alongside a trader fleet.
    traffic: bool,
    /// Fly past a pirate gang's lair until it pulls the ship out of fast
    /// travel.
    interdict: bool,
    /// Alongside a trader fleet in its frame (and its faction hostile).
    convoy: bool,
    /// With `convoy`: its faction counts the player hostile.
    hostile_convoy: bool,
    /// Start skimming a gas giant (`--skim`).
    skim: bool,
    /// Start by a harvester at work: `gas` or `ice` (`--harvesters`).
    harvesters: Option<String>,
    /// Start in the nearest asteroid field with raiders, for them to find.
    combat: bool,
    /// With `combat`: the band close in front, battered, fighting at once.
    brawl: bool,
    /// With `brawl`: the target behind the ship's own hull (#123), and
    /// held there until a `--screenshot` is taken.
    behind_hull: bool,
    /// With `brawl`: the ship blown up this many seconds ago.
    lost: Option<f64>,
    /// Show the rewind to a checkpoint, or the arrival back at one (for
    /// screenshots).
    rewind: bool,
    arrival: bool,
    /// Hold the mining laser's trigger down (for screenshots).
    fire: bool,
    /// Start on the dispatch board.
    contracts: bool,
    /// Start with the first contract on the board taken.
    take_contract: bool,
    /// Start with the galaxy map open.
    map: bool,
    /// Plot a course to the nearest system at least this many jumps away
    /// (for checking the map's course display).
    course: Option<usize>,
    /// Start in this stock ship instead.
    ship: Option<String>,
    /// With `ship`: put these sections (comma-separated ids) in its mounts,
    /// in order, each filled with the biggest modules that fit.
    sections: Option<String>,
    /// Save the 90th frame here and quit (for checking visual changes
    /// without a person at the window).
    screenshot: Option<PathBuf>,
    /// Look at your ship from this angle, close up (0 = from behind, 90 =
    /// from the right). For inspecting the model.
    ship_view: Option<f64>,
    /// Open this menu page.
    menu: Option<String>,
    /// Docked: open this station screen (`trade`, `hangar`, `shipyard`,
    /// `contracts`). Implies `--station`.
    screen: Option<String>,
    /// Press these keys, one a frame (`--keys down,enter,wait60,esc`), as
    /// if typed; `waitN` waits N frames (and any wait lasts until a game
    /// being started is ready). Quits when done, unless `--screenshot`
    /// does first. For checking menu flows end to end without a person.
    keys: Vec<String>,
    /// No sound. Screenshot and scripted runs are silent unless `--sound`.
    mute: bool,
    /// `--remote PATH`: play turn by turn, for playtesting without a person
    /// at the window. Steps appended to the file run as if in `--keys`
    /// (one per line or comma-separated), and the world stands still
    /// between them. See [`Running::run_script`] for the steps.
    remote: Option<PathBuf>,
}

impl Flags {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().skip_while(|a| *a != flag).nth(1).cloned();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        Self {
            seed: value("--seed").and_then(|s| s.parse().ok()),
            undocked: has("--undocked"),
            cities: has("--cities"),
            station: has("--station"),
            field: has("--field") || has("--sunward") || has("--quarry"),
            sunward: has("--sunward"),
            quarry: has("--quarry").then(|| value("--quarry").as_deref() == Some("back")),
            size: value("--size").and_then(|s| {
                let (w, h) = s.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            }),
            traffic: has("--traffic"),
            interdict: has("--interdict"),
            convoy: has("--convoy") || has("--hostile-convoy"),
            hostile_convoy: has("--hostile-convoy"),
            skim: has("--skim"),
            harvesters: value("--harvesters").or_else(|| has("--harvesters").then(|| "gas".into())),
            combat: has("--combat") || has("--brawl") || has("--lost") || has("--rewind"),
            brawl: has("--brawl") || has("--lost") || has("--rewind") || has("--behind-hull"),
            behind_hull: has("--behind-hull"),
            lost: value("--lost")
                .and_then(|s| s.parse().ok())
                .or(has("--rewind").then_some(3.0)),
            rewind: has("--rewind"),
            arrival: has("--arrival"),
            fire: has("--fire"),
            contracts: has("--contracts") || has("--contract"),
            take_contract: has("--contract"),
            map: has("--map"),
            course: value("--course").and_then(|s| s.parse().ok()),
            ship: value("--ship"),
            sections: value("--sections"),
            screenshot: value("--screenshot").map(PathBuf::from),
            ship_view: value("--ship-view").and_then(|s| s.parse().ok()),
            menu: value("--menu"),
            screen: value("--screen"),
            keys: value("--keys")
                .map(|k| k.split(',').map(|s| s.trim().to_lowercase()).collect())
                .unwrap_or_default(),
            mute: has("--mute")
                || (!has("--sound") && (has("--screenshot") || has("--keys") || has("--remote"))),
            remote: value("--remote").map(PathBuf::from),
        }
    }

    /// Whether to skip the title and start a fresh game straight away.
    fn straight_in(&self) -> bool {
        match self.menu.as_deref() {
            Some("pause" | "quit") => true,
            Some(_) => false,
            None => {
                self.seed.is_some()
                    || self.undocked
                    || self.cities
                    || self.station
                    || self.field
                    || self.traffic
                    || self.interdict
                    || self.convoy
                    || self.harvesters.is_some()
                    || self.skim
                    || self.combat
                    || self.arrival
                    || self.contracts
                    || self.map
                    || self.course.is_some()
                    || self.ship.is_some()
                    || self.ship_view.is_some()
                    || self.screenshot.is_some()
                    || self.screen.is_some()
            }
        }
    }

    /// A fresh world set up as the flags ask.
    fn world(&self) -> World {
        let mut world = World::new(self.seed.unwrap_or(1));
        if self.station || self.screen.is_some() || self.arrival {
            world.start_at_station();
        }
        if let Some(mut fit) = self.ship.as_deref().and_then(retrox_content::ships::stock) {
            if let Some(ids) = &self.sections {
                refit_sections(&mut fit, ids);
            }
            world.player.ship.fit = fit;
        }
        if self.contracts {
            world.act(Action::Open(Screen::Contracts));
        }
        let screen = match self.screen.as_deref() {
            Some("trade") => Some(Screen::Trade),
            Some("hangar") => Some(Screen::Hangar),
            Some("shipyard") => Some(Screen::Shipyard),
            Some("contracts") => Some(Screen::Contracts),
            Some("bounties") => Some(Screen::Bounties),
            Some(other) => {
                eprintln!("--screen: no screen {other}");
                None
            }
            None => None,
        };
        if let Some(screen) = screen {
            world.act(Action::Open(screen));
        }
        if self.take_contract {
            world.act(Action::AcceptContract(0));
        }
        if self.undocked {
            // Out by a station if there's one here, else in orbit.
            if let (Mode::Landed { .. }, Some(&s)) = (world.mode, world.view.stations.first()) {
                world.mode = Mode::Docked(s);
            }
            if let Mode::Docked(s) = world.mode {
                world.act(Action::Undock);
                world.target = Some(Place::Station(s));
            } else {
                world.act(Action::Launch);
            }
            world.ship.throttle = 0.3;
            // Look back at the station.
            world.ship.rot *= DQuat::from_rotation_y(2.6);
            world.cam_rot = world.ship.rot;
        }
        if self.cities && world.start_by_cities().is_none() {
            eprintln!("--cities: no planet with two cities anywhere");
        }
        let parked = if let Some(backlit) = self.quarry {
            world.start_at_quarry(backlit)
        } else if self.sunward {
            world.start_sunward()
        } else {
            self.field && world.start_at_field()
        };
        if parked {
            world.act(Action::Scan);
        }
        if self.traffic {
            world.start_by_traffic();
        }
        if self.interdict {
            world.stage_interdiction();
        }
        if self.convoy {
            world.stage_convoy(self.hostile_convoy);
        }
        if self.skim && !world.start_skimming() {
            eprintln!("--skim: no gas giant anywhere");
        }
        if let Some(good) = &self.harvesters {
            use retrox_sim::economy::Good;
            let good = if good == "ice" { Good::Ice } else { Good::Gas };
            if !world.start_by_harvesters(good) {
                eprintln!("--harvesters: nobody harvesting {} nearby", good.name());
            }
        }
        if self.brawl {
            world.stage_brawl();
            if self.behind_hull {
                world.stage_behind_hull();
            }
            if let Some(t) = self.lost {
                world.stage_loss(t);
            }
        } else if self.combat {
            world.start_by_raiders();
        }
        world.map_open = self.map;
        if let Some(jumps) = self.course {
            let here = world.galaxy.systems[world.system].position;
            let mut systems: Vec<_> = world
                .galaxy
                .systems
                .iter()
                .map(|(id, s)| (id, s.position))
                .collect();
            systems.sort_by(|a, b| here.distance(a.1).total_cmp(&here.distance(b.1)));
            let dest = systems
                .into_iter()
                .find(|&(id, _)| world.route(id).is_some_and(|r| r.len() > jumps));
            if let Some((dest, _)) = dest {
                world.act(Action::PlotCourse(Some(dest)));
            }
        }
        world
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        flags: Flags::parse(),
        running: None,
    };
    event_loop.run_app(&mut app).expect("run");
    if let Some(r) = app.running.as_mut() {
        r.finish_writes();
    }
    // Leave without tearing the renderer down: dropping it segfaults in
    // the graphics driver on some systems, and there's nothing left to
    // clean up but memory. Saves are already on disk (above).
    std::process::exit(0);
}

struct App {
    flags: Flags,
    running: Option<Running>,
}

struct Meshes {
    sphere: MeshId,
    /// The player's ship, generated from its fit.
    ship: MeshId,
    ship_shape: meshes::ShipShape,
    /// Every ship mesh built so far, by [`fit_key`]: refits and the
    /// shipyard's ships come back without rebuilding.
    ship_cache: std::collections::HashMap<String, (MeshId, meshes::ShipShape)>,
    hangar_bay: MeshId,
    /// Each rock shape at each of [`meshes::ROCK_DETAIL`].
    rocks: Vec<[MeshId; 3]>,
    /// A unit cube: hull plates flung off in explosions.
    plate: MeshId,
    /// A thin unit ring: shockwaves.
    ring: MeshId,
    /// One mesh per station in the current system, aligned with `view.stations`.
    stations: Vec<MeshId>,
    /// The world's `system_generation` the stations and sky were built
    /// for; `u64::MAX` to rebuild.
    system_generation: u64,
}

/// The game being played.
struct Game {
    world: World,
    id: GameId,
    /// Real seconds played, over every session.
    played_s: f64,
    /// Where we were docked or landed last frame: arriving somewhere new
    /// writes a checkpoint.
    docked: Option<SettlementId>,
    /// Unix seconds of this game's last save and checkpoint, if any.
    last_save: Option<u64>,
    last_checkpoint: Option<u64>,
}

/// A game starting: generated or loaded on a worker thread.
struct Pending {
    title: String,
    subtitle: String,
    rx: mpsc::Receiver<Result<Started, String>>,
}

struct Started {
    world: World,
    id: GameId,
    played_s: f64,
    /// The save it was loaded from, if it was.
    from: Option<save::Header>,
}

/// A save written on a worker thread.
struct Written {
    header: save::Header,
    result: Result<(), String>,
}

struct Running {
    renderer: Renderer,
    meshes: Meshes,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    keys: HashSet<KeyCode>,
    just_pressed: Vec<KeyCode>,
    /// The left mouse button is held (while flying with the mouse).
    trigger: bool,
    /// `--fire`: the trigger is held down for good.
    fire: bool,
    stick: DVec2,
    captured: bool,
    /// The controls card (H) is up.
    controls_card: bool,
    last_frame: Instant,
    start: Instant,
    frames: u32,
    ship_view: Option<f64>,
    settings: Settings,
    settings_path: PathBuf,
    /// Where saves go.
    data: PathBuf,
    /// Don't write saves (screenshot runs).
    read_only: bool,
    game: Option<Game>,
    /// The pause menu is open (always, in effect, on the title screen).
    paused: bool,
    menu: Menu,
    pending: Option<Pending>,
    /// Every save on disk, newest first; refreshed after each write.
    saves: Vec<save::Entry>,
    written_tx: mpsc::Sender<Written>,
    written_rx: mpsc::Receiver<Written>,
    writers: Vec<std::thread::JoinHandle<()>>,
    /// Scripted keys still to press (see [`Flags::keys`]), and frames to
    /// wait before the next.
    script: std::collections::VecDeque<String>,
    script_wait: u32,
    /// `--remote`: the step file and how far into it we've read.
    remote: Option<(PathBuf, usize)>,
    /// Scripted keys held down, with the frames left to hold them.
    holds: Vec<(KeyCode, u32)>,
    /// Scripted pointer events for the UI, one batch a frame.
    pointer: std::collections::VecDeque<Vec<egui::Event>>,
    /// A short note in the corner: "checkpoint saved".
    toast: Option<(String, bool, Instant)>,
    /// The ship was lost and the game is rewinding to its checkpoint
    /// (loading behind it), then the black lifting off the checkpoint.
    rewind: Option<lost::Rewind>,
    arrival: Option<lost::Arrival>,
    audio: audio::Audio,
    quit: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let settings_path = settings::path();
        let settings = Settings::load(&settings_path);
        let (width, height) = self.flags.size.unwrap_or((1600.0, 960.0));
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("retrox")
                        .with_inner_size(winit::dpi::LogicalSize::new(width, height))
                        .with_fullscreen(
                            settings.fullscreen.then_some(Fullscreen::Borderless(None)),
                        ),
                )
                .expect("create window"),
        );
        if self.flags.size.is_none() && !settings.fullscreen {
            fit_to_monitor(&window);
        }
        let mut renderer = Renderer::new(window.clone());
        let meshes = Meshes {
            sphere: renderer.add_mesh(&meshes::icosphere(5)),
            ship: MeshId::default(),
            ship_shape: meshes::ShipShape::default(),
            ship_cache: std::collections::HashMap::new(),
            hangar_bay: renderer.add_mesh(&meshes::hangar_bay()),
            rocks: (0..mining::ROCK_MESHES)
                .map(|i| {
                    meshes::ROCK_DETAIL
                        .map(|d| renderer.add_mesh(&meshes::rock(mining::rock_mesh_seed(i), d)))
                })
                .collect(),
            stations: Vec::new(),
            system_generation: u64::MAX,
            plate: renderer.add_mesh(&meshes::cube()),
            ring: renderer.add_mesh(&meshes::torus(0.035, 64, 6)),
        };

        let egui_ctx = egui::Context::default();
        hud::style(&egui_ctx);
        egui_ctx.set_zoom_factor(hud_zoom(settings.ui_scale, &window));
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        let data = save::data_dir();
        let (written_tx, written_rx) = mpsc::channel();
        let audio = audio::Audio::new(&settings, self.flags.mute);
        let mut r = Running {
            renderer,
            meshes,
            egui_ctx,
            egui_state,
            keys: HashSet::new(),
            just_pressed: Vec::new(),
            trigger: false,
            fire: self.flags.fire,
            stick: DVec2::ZERO,
            captured: false,
            controls_card: false,
            last_frame: Instant::now(),
            start: Instant::now(),
            frames: 0,
            ship_view: self.flags.ship_view,
            settings,
            settings_path,
            saves: save::list(&data),
            data,
            read_only: self.flags.screenshot.is_some() && self.flags.keys.is_empty(),
            game: None,
            paused: false,
            menu: Menu::default(),
            pending: None,
            written_tx,
            written_rx,
            writers: Vec::new(),
            toast: None,
            rewind: None,
            arrival: None,
            audio,
            quit: false,
            script: self.flags.keys.iter().cloned().collect(),
            script_wait: 0,
            remote: self.flags.remote.clone().map(|p| (p, 0)),
            holds: Vec::new(),
            pointer: std::collections::VecDeque::new(),
        };
        if self.flags.straight_in() {
            let world = self.flags.world();
            let seed = world.galaxy.seed;
            r.start_game(Started {
                world,
                id: GameId::new(seed),
                played_s: 0.0,
                from: None,
            });
            if self.flags.lost.is_some() && !r.read_only {
                // Something to go back to: this game docked at a station.
                let mut earlier = World::new(seed);
                earlier.start_at_station();
                let id = &r.game.as_ref().expect("started").id;
                if let Ok(snap) = save::snapshot(&earlier, id, Kind::Checkpoint, 0.0) {
                    let _ = save::write(&r.data, &snap);
                    r.saves = save::list(&r.data);
                }
            }
            let place = r.game.as_ref().map(|g| save::place_text(&g.world));
            let game = r.game.as_ref().expect("started");
            if self.flags.rewind {
                r.rewind = Some(lost::Rewind {
                    started: Instant::now(),
                    from_s: game.world.clock_s,
                    to_s: game.world.clock_s - 2.6 * world::SECONDS_PER_DAY,
                    place: "docked at a station".to_owned(),
                    date: game.world.date().to_string(),
                });
            }
            if self.flags.arrival {
                r.arrival = Some(lost::Arrival {
                    started: Instant::now(),
                    place: place.unwrap_or_default(),
                    date: game.world.date().to_string(),
                });
            }
        } else {
            r.back_to_title();
        }
        let page = match self.flags.menu.as_deref() {
            Some("new") => Some(Page::NewGame),
            Some("load") => Some(Page::Load),
            Some("settings") => Some(Page::Settings),
            Some(m) if m.starts_with("handbook") => {
                // `handbook:N` opens chapter N.
                let chapter = m.split(':').nth(1).and_then(|n| n.parse().ok());
                r.menu.set_chapter(chapter.unwrap_or(0));
                Some(Page::Handbook)
            }
            Some("pause") => {
                r.paused = true;
                None
            }
            Some("quit") => {
                r.paused = true;
                Some(Page::ConfirmQuit)
            }
            _ => None,
        };
        if let Some(page) = page {
            r.menu.open(page);
        }
        self.running = Some(r);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = self.running.as_mut() else {
            return;
        };
        let consumed = r
            .egui_state
            .on_window_event(&r.renderer.window, &event)
            .consumed;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                r.renderer.resize(size.width, size.height);
                r.egui_ctx
                    .set_zoom_factor(hud_zoom(r.settings.ui_scale, &r.renderer.window));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            if !event.repeat {
                                r.just_pressed.push(code);
                            }
                            r.keys.insert(code);
                        }
                        ElementState::Released => {
                            r.keys.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                // Flying with the mouse, it's the mining laser's trigger.
                ElementState::Pressed if r.captured => r.trigger = true,
                ElementState::Pressed if !consumed && r.flying() => r.set_captured(true),
                ElementState::Released => r.trigger = false,
                ElementState::Pressed => {}
            },
            WindowEvent::Focused(false) => r.set_captured(false),
            WindowEvent::RedrawRequested => {
                const SCREENSHOT_FRAME: u32 = 90;
                r.frames += 1;
                // With a key script, the screenshot is of where it ends.
                let shoot = if self.flags.keys.is_empty() {
                    r.frames == SCREENSHOT_FRAME
                } else {
                    r.script.is_empty()
                        && r.script_wait == 0
                        && r.pending.is_none()
                        && r.writers.is_empty()
                };
                if shoot && r.renderer.capture.is_none() {
                    r.renderer.capture = self.flags.screenshot.clone();
                }
                // Held behind the hull until the shot, or it flies off.
                if self.flags.behind_hull
                    && self.flags.screenshot.is_some()
                    && let Some(g) = &mut r.game
                {
                    g.world.stage_behind_hull();
                }
                r.frame();
                let shot_done = if self.flags.keys.is_empty() {
                    r.frames == SCREENSHOT_FRAME && self.flags.screenshot.is_some()
                } else {
                    shoot
                };
                if r.quit || shot_done {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        let Some(r) = self.running.as_mut() else {
            return;
        };
        if let DeviceEvent::MouseMotion { delta } = event
            && r.captured
        {
            let k = 0.004 * r.settings.mouse_sensitivity;
            let pitch = if r.settings.invert_pitch {
                -delta.1
            } else {
                delta.1
            };
            r.stick += DVec2::new(delta.0, pitch) * k;
            r.stick = r.stick.clamp_length_max(1.0);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(r) = &self.running {
            r.renderer.window.request_redraw();
        }
    }
}

impl Running {
    /// In free flight, unpaused: where clicking the view takes the mouse.
    fn flying(&self) -> bool {
        !self.paused
            && self.game.as_ref().is_some_and(|g| {
                g.world.mode == Mode::Normal && !g.world.map_open && g.world.combat.lost.is_none()
            })
    }

    fn set_captured(&mut self, captured: bool) {
        let window = &self.renderer.window;
        if captured {
            let grabbed = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            if grabbed.is_err() {
                return;
            }
            window.set_cursor_visible(false);
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.captured = captured;
        self.trigger = false;
        self.stick = DVec2::ZERO;
    }

    /// Back to the title screen, with its backdrop.
    fn back_to_title(&mut self) {
        self.game = None;
        self.paused = false;
        self.menu = Menu::default();
        self.set_captured(false);
        self.renderer.set_sky(&title::sky());
        self.meshes.system_generation = u64::MAX;
        self.saves = save::list(&self.data);
    }

    fn start_game(&mut self, started: Started) {
        // A loaded game has its checkpoint already; a new one gets its
        // first on the first frame, where it starts landed.
        let docked = started.from.as_ref().and(started.world.docked_at());
        let (last_save, last_checkpoint) = match &started.from {
            Some(h) => self.last_writes(&h.game.id),
            None => (None, None),
        };
        self.game = Some(Game {
            world: started.world,
            id: started.id,
            played_s: started.played_s,
            docked,
            last_save,
            last_checkpoint,
        });
        self.paused = false;
        self.menu = Menu::default();
        self.meshes.system_generation = u64::MAX;
        self.set_captured(false);
    }

    /// When a game was last saved and checkpointed, from the saves on disk.
    fn last_writes(&self, game: &str) -> (Option<u64>, Option<u64>) {
        let mut out = (None, None);
        for h in self.saves.iter().filter_map(|e| e.header.as_ref().ok()) {
            if h.game.id == game {
                match h.kind {
                    Kind::Save => out.0 = out.0.max(Some(h.saved)),
                    Kind::Checkpoint => out.1 = out.1.max(Some(h.saved)),
                }
            }
        }
        out
    }

    /// Generate or load a game on a worker thread.
    fn begin(
        &mut self,
        title: &str,
        subtitle: String,
        work: impl FnOnce() -> Result<Started, String> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.pending = Some(Pending {
            title: title.to_owned(),
            subtitle,
            rx,
        });
    }

    /// Serialize the game now and write it on a worker thread.
    fn write(&mut self, kind: Kind) {
        let Some(game) = &self.game else { return };
        if self.read_only {
            return;
        }
        match save::snapshot(&game.world, &game.id, kind, game.played_s) {
            Ok(snapshot) => {
                let data = self.data.clone();
                let tx = self.written_tx.clone();
                self.writers.push(std::thread::spawn(move || {
                    let result = save::write(&data, &snapshot);
                    let _ = tx.send(Written {
                        header: snapshot.header,
                        result,
                    });
                }));
            }
            Err(why) => self.toast(why, false),
        }
    }

    /// Wait for saves still being written (on quitting).
    fn finish_writes(&mut self) {
        for w in self.writers.drain(..) {
            let _ = w.join();
        }
    }

    fn toast(&mut self, text: impl Into<String>, ok: bool) {
        self.toast = Some((text.into(), ok, Instant::now()));
        self.audio.saved(ok);
    }

    /// Pick up finished background work: a game ready to play, saves
    /// written.
    fn poll_background(&mut self) {
        // A rewind runs its course however fast the checkpoint loads.
        let rewinding = self
            .rewind
            .as_ref()
            .is_some_and(|r| r.elapsed() < lost::REWIND_S);
        if let Some(p) = &self.pending
            && !rewinding
        {
            match p.rx.try_recv() {
                Ok(Ok(started)) => {
                    self.pending = None;
                    self.start_game(started);
                    if let Some(r) = self.rewind.take() {
                        self.arrival = Some(lost::Arrival {
                            started: Instant::now(),
                            place: r.place,
                            date: r.date,
                        });
                    }
                }
                Ok(Err(why)) => {
                    self.pending = None;
                    if self.rewind.take().is_some() {
                        // Show why over the pause menu.
                        self.paused = true;
                        self.menu = Menu::default();
                    }
                    self.menu.error = Some(why);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.menu.error = Some("Starting the game failed unexpectedly.".to_owned());
                }
            }
        }
        while let Ok(done) = self.written_rx.try_recv() {
            match done.result {
                Ok(()) => {
                    if let Some(game) = self.game.as_mut().filter(|g| g.id == done.header.game) {
                        match done.header.kind {
                            Kind::Save => game.last_save = Some(done.header.saved),
                            Kind::Checkpoint => game.last_checkpoint = Some(done.header.saved),
                        }
                    }
                    let note = match done.header.kind {
                        Kind::Save => "Game saved",
                        Kind::Checkpoint => "Checkpoint saved",
                    };
                    self.toast(note, true);
                }
                Err(why) => self.toast(why, false),
            }
            self.saves = save::list(&self.data);
        }
        self.writers.retain(|w| !w.is_finished());
    }

    fn apply(&mut self, action: MenuAction) {
        match action {
            MenuAction::Resume => self.paused = false,
            MenuAction::Pause => {
                self.paused = true;
                self.menu = Menu::default();
                self.set_captured(false);
            }
            MenuAction::NewGame(seed) => {
                self.begin("Generating galaxy", format!("seed {seed}"), move || {
                    Ok(Started {
                        world: World::new(seed),
                        id: GameId::new(seed),
                        played_s: 0.0,
                        from: None,
                    })
                });
            }
            MenuAction::Load(path) => {
                let subtitle = self
                    .saves
                    .iter()
                    .find(|e| e.path == path)
                    .and_then(|e| e.header.as_ref().ok())
                    .map_or_else(String::new, |h| format!("{} · {}", h.system, h.date));
                self.begin("Loading", subtitle, move || {
                    let loaded = save::read(&path)?;
                    Ok(Started {
                        world: loaded.world,
                        id: loaded.header.game.clone(),
                        played_s: loaded.played_s,
                        from: Some(loaded.header),
                    })
                });
            }
            MenuAction::Save => self.write(Kind::Save),
            MenuAction::QuitToTitle { save } => {
                if save {
                    self.write(Kind::Save);
                }
                self.back_to_title();
            }
            MenuAction::Quit => self.quit = true,
            MenuAction::SettingsChanged => {
                let window = &self.renderer.window;
                let fullscreen = window.fullscreen().is_some();
                if fullscreen != self.settings.fullscreen {
                    window.set_fullscreen(
                        self.settings
                            .fullscreen
                            .then_some(Fullscreen::Borderless(None)),
                    );
                }
                self.egui_ctx
                    .set_zoom_factor(hud_zoom(self.settings.ui_scale, window));
                if let Err(why) = self.settings.save(&self.settings_path) {
                    self.toast(why, false);
                }
            }
        }
    }

    /// Flight controls and one-shot actions from the keyboard; `true` in
    /// the last place if Esc asks for the pause menu.
    fn controls(&mut self, dt: f64, keys: Vec<KeyCode>) -> (Controls, Vec<Action>, bool) {
        let down = |k| self.keys.contains(&k);
        let axis = |pos, neg| f64::from(u8::from(down(pos))) - f64::from(u8::from(down(neg)));

        // The virtual stick springs back toward centre.
        self.stick *= (-2.2 * dt).exp();
        // Like a stick: ↓ pulls the nose up, unless pitch is inverted, which
        // flips the keys as well as the mouse (issue #128).
        let (nose_up, nose_down) = if self.settings.invert_pitch {
            (KeyCode::ArrowUp, KeyCode::ArrowDown)
        } else {
            (KeyCode::ArrowDown, KeyCode::ArrowUp)
        };
        let keys_pitch = axis(nose_up, nose_down);
        let keys_yaw = axis(KeyCode::ArrowRight, KeyCode::ArrowLeft);
        let controls = Controls {
            stick: DVec3::new(
                (-self.stick.y + keys_pitch).clamp(-1.0, 1.0),
                (self.stick.x + keys_yaw).clamp(-1.0, 1.0),
                axis(KeyCode::KeyE, KeyCode::KeyQ),
            ),
            throttle_delta: axis(KeyCode::KeyW, KeyCode::KeyS),
            strafe: axis(KeyCode::KeyD, KeyCode::KeyA),
            lift: axis(KeyCode::Space, KeyCode::ControlLeft),
            boost: down(KeyCode::ShiftLeft),
            fire: (self.trigger && self.captured) || down(KeyCode::KeyR) || self.fire,
        };

        let mut actions = Vec::new();
        let mut pause = false;
        let Some(game) = &mut self.game else {
            return (controls, actions, false);
        };
        let world = &mut game.world;
        let mut capture = None;
        for key in keys {
            let action = match key {
                KeyCode::KeyJ => Some(Action::FastTravel),
                KeyCode::KeyF => Some(Action::Dock),
                KeyCode::KeyL => Some(Action::Land),
                KeyCode::KeyT => Some(Action::CycleTarget),
                KeyCode::KeyV => Some(Action::ToggleCamera),
                KeyCode::KeyZ => Some(Action::ToggleFlightAssist),
                KeyCode::KeyM => Some(Action::ToggleMap),
                KeyCode::KeyG => Some(Action::Scan),
                KeyCode::KeyY => Some(Action::CycleHostile),
                KeyCode::KeyI => Some(Action::Intercept),
                KeyCode::KeyX => {
                    world.ship.throttle = 0.0;
                    None
                }
                KeyCode::KeyH | KeyCode::F1 => {
                    self.controls_card = !self.controls_card;
                    None
                }
                KeyCode::Tab => {
                    capture = Some(!capture.unwrap_or(self.captured));
                    None
                }
                KeyCode::Escape if world.map_open => Some(Action::ToggleMap),
                // Back out of a docked screen to the menu.
                KeyCode::Escape if world.docked_at().is_some() && world.screen != Screen::Hub => {
                    Some(Action::Open(Screen::Hub))
                }
                KeyCode::Escape => {
                    pause = true;
                    None
                }
                _ => None,
            };
            actions.extend(action);
        }
        if let Some(c) = capture {
            self.set_captured(c);
        }
        (controls, actions, pause)
    }

    /// The ship to show in the hangar bay, if a bay screen is open: yours
    /// in the hangar and trading, the one being looked at in the shipyard.
    fn bay_fit(world: &World) -> Option<retrox_sim::ship::Loadout> {
        world.docked_at()?;
        match world.screen {
            // Trading happens in the bay too, with your ship being loaded.
            Screen::Hangar | Screen::Trade => Some(world.player.ship.fit.clone()),
            Screen::Shipyard => Some(
                world
                    .inspect
                    .and_then(|h| retrox_content::ships::stock(h.id))
                    .unwrap_or_else(|| world.player.ship.fit.clone()),
            ),
            _ => None,
        }
    }

    /// A fit's mesh, built on first use.
    fn ship_mesh(&mut self, fit: &retrox_sim::ship::Loadout) -> (MeshId, meshes::ShipShape) {
        let key = fit_key(fit);
        if let Some(found) = self.meshes.ship_cache.get(&key) {
            return found.clone();
        }
        let (mesh, shape) = meshes::ship(fit);
        let built = (self.renderer.add_mesh(&mesh), shape);
        self.meshes.ship_cache.insert(key, built.clone());
        built
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        self.poll_background();
        self.run_script();
        let idle = self.remote_idle();

        let keys = std::mem::take(&mut self.just_pressed);
        let mut menu_actions = Vec::new();
        let mut nav = Vec::new();
        let mut frame_controls = None;
        if self.pending.is_some() {
            // Starting a game: nothing to do but wait.
        } else if self.game.is_none() || self.paused {
            for key in &keys {
                match key {
                    KeyCode::Escape => {
                        self.audio.play(audio::Cue::Back);
                        menu_actions.extend(self.menu.back(self.game.is_some()));
                    }
                    KeyCode::ArrowUp => nav.push(menu::Nav::Up),
                    KeyCode::ArrowDown => nav.push(menu::Nav::Down),
                    KeyCode::Enter | KeyCode::NumpadEnter => nav.push(menu::Nav::Enter),
                    _ => {}
                }
            }
            for n in &nav {
                match n {
                    menu::Nav::Enter => self.audio.ui_click(),
                    _ => self.audio.ui_nav(),
                }
            }
        } else if self
            .game
            .as_ref()
            .is_some_and(|g| g.world.combat.lost.is_some_and(|t| t > lost::READY_AT))
            && keys
                .iter()
                .any(|k| matches!(k, KeyCode::Enter | KeyCode::NumpadEnter))
        {
            self.restore_checkpoint();
        } else {
            let (controls, actions, pause) = self.controls(dt, keys);
            frame_controls = Some(controls);
            let game = self.game.as_mut().expect("playing");
            for a in actions {
                game.world.act(a);
            }
            let was_lost = game.world.combat.lost;
            // Played by remote, the world waits for the next step.
            let dt = if idle { 0.0 } else { dt };
            game.world.update(dt, controls);
            // The card slams in over the wreck.
            if let (Some(before), Some(now)) = (was_lost, game.world.combat.lost)
                && before < lost::CARD_AT
                && now >= lost::CARD_AT
            {
                self.audio.play(audio::Cue::Lost);
            }
            game.played_s += dt;
            if pause {
                menu_actions.push(MenuAction::Pause);
            }
        }
        for a in menu_actions.drain(..) {
            self.apply(a);
        }
        if !self.flying() && self.captured {
            self.set_captured(false);
        }
        self.checkpoint();

        let (w, h) = self.renderer.size();
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            70f32.to_radians(),
            w as f32 / h.max(1) as f32,
            0.5,
        );
        let time = self.start.elapsed().as_secs_f32();
        let (view_proj, instances, cam_pos, star) = if self.game.is_some() {
            self.game_scene(proj)
        } else {
            let courier =
                retrox_content::ships::stock(hangar::STARTING_HULL).expect("starting hull exists");
            let (ship, _) = self.ship_mesh(&courier);
            let (view, instances) = title::scene(&self.meshes, ship, time);
            (proj * view, instances, DVec3::ZERO, title::STAR)
        };

        let mut raw_input = self.egui_state.take_egui_input(&self.renderer.window);
        if let Some(events) = self.pointer.pop_front() {
            raw_input.events.extend(events);
        }
        let mut ui_actions = Vec::new();
        let mut lost_choice = None;
        let output = self.egui_ctx.run_ui(raw_input, |ui| {
            let ctx = ui.ctx();
            if let Some(r) = &self.rewind {
                lost::rewind(ctx, r);
            } else if let Some(p) = &self.pending {
                menu::loading(ctx, &p.title, &p.subtitle, time);
            } else if let Some(game) = &self.game {
                if self.paused {
                    menu_actions = self.menu.draw(
                        ctx,
                        menu::MenuInput {
                            game: Some(menu::GameInfo {
                                world: &game.world,
                                id: &game.id,
                                played_s: game.played_s,
                                last_save: game.last_save,
                                last_checkpoint: game.last_checkpoint,
                            }),
                            saves: &self.saves,
                            settings: &mut self.settings,
                            time,
                            nav: &nav,
                        },
                    );
                } else if game.world.combat.lost.is_none() {
                    // (The instruments die with the ship.)
                    ui_actions = hud::draw(
                        ctx,
                        hud::HudInput {
                            world: &game.world,
                            view_proj,
                            camera: cam_pos,
                            stick: self.stick,
                            captured: self.captured,
                            controls_card: self.controls_card,
                            notes: self.settings.first_steps,
                            invert_pitch: self.settings.invert_pitch,
                        },
                    );
                }
                if !self.paused
                    && let Some(t) = game.world.combat.lost
                {
                    let checkpoint =
                        Self::newest_checkpoint(&self.saves, &game.id.id).map(|(h, _)| {
                            lost::Checkpoint {
                                place: h.place.clone(),
                                date: h.date.clone(),
                                clock_s: h.clock_s,
                            }
                        });
                    let info = lost::LostInfo::new(&game.world, checkpoint);
                    lost_choice = lost::card(ctx, &info, t);
                }
            } else {
                menu_actions = self.menu.draw(
                    ctx,
                    menu::MenuInput {
                        game: None,
                        saves: &self.saves,
                        settings: &mut self.settings,
                        time,
                        nav: &nav,
                    },
                );
            }
            if let Some((text, ok, at)) = &self.toast {
                menu::toast(ctx, text, *ok, at.elapsed().as_secs_f32());
            }
            if let Some(a) = &self.arrival {
                lost::arrival(ctx, a);
            }
        });
        // Interface clicks: egui's own widgets report them; the menus'
        // hand-drawn ones show the pointing hand.
        let pointer_click = self.egui_ctx.input(|i| i.pointer.primary_clicked());
        let out = &output.platform_output;
        if out
            .events
            .iter()
            .any(|e| matches!(e, egui::output::OutputEvent::Clicked(_)))
            || (pointer_click && out.cursor_icon == egui::CursorIcon::PointingHand)
        {
            self.audio.ui_click();
        }
        self.egui_state
            .handle_platform_output(&self.renderer.window, output.platform_output.clone());
        if let Some(game) = &mut self.game {
            for a in ui_actions {
                game.world.act(a);
            }
        }
        for a in menu_actions {
            self.apply(a);
        }
        match lost_choice {
            Some(lost::Choice::Restore) => self.restore_checkpoint(),
            Some(lost::Choice::Menu) => self.apply(MenuAction::Pause),
            None => {}
        }
        if self
            .arrival
            .as_ref()
            .is_some_and(|a| a.started.elapsed().as_secs_f32() > lost::ARRIVE_S)
        {
            self.arrival = None;
        }
        self.audio.set_volumes(&self.settings);
        self.audio.update(
            audio::View {
                world: self.game.as_ref().map(|g| &g.world),
                paused: self.paused || self.pending.is_some(),
                controls: frame_controls.as_ref(),
            },
            dt as f32,
        );
        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.2.elapsed().as_secs_f32() > menu::TOAST_SECONDS)
        {
            self.toast = None;
        }

        let ctx = self.egui_ctx.clone();
        self.renderer.render(render::Frame {
            view_proj,
            star_color: star,
            time,
            exposure: 1.0,
            instances: &instances,
            reveal: (self.game.as_ref()).and_then(|g| hull_reveal(&g.world, cam_pos)),
            ui: Some((&ctx, output)),
        });
    }

    /// Played by `--remote` with nothing left to do: the world stands
    /// still until the next step arrives.
    fn remote_idle(&self) -> bool {
        self.remote.is_some()
            && self.script.is_empty()
            && self.script_wait == 0
            && self.holds.is_empty()
            && self.pointer.is_empty()
    }

    /// Steps appended to the `--remote` file since we last looked.
    fn read_remote(&mut self) {
        let Some((path, read)) = &mut self.remote else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&*path) else {
            return;
        };
        // Only whole lines, so a step being written isn't cut in half.
        let Some(end) = text.rfind('\n').map(|i| i + 1) else {
            return;
        };
        if end <= *read {
            return;
        }
        let new = text[*read..end].to_owned();
        *read = end;
        for step in new.split(['\n', ',']) {
            let step = step.trim().to_lowercase();
            if !step.is_empty() {
                self.script.push_back(step);
            }
        }
    }

    /// Press the next scripted key, if it's time. Steps:
    /// - a key's name (`f`, `enter`, `esc`, `up`, `f1`, `space`, `1`…)
    ///   presses it once;
    /// - `waitN` waits N frames (and any wait lasts until a game being
    ///   started is ready);
    /// - `hold:KEY:N` holds a key down for N frames (`hold:w:60`, or
    ///   `hold:r:30` for the trigger), without waiting for it;
    /// - `click:X:Y` clicks the UI at that pixel of the window (as in a
    ///   screenshot), `move:X:Y` just points there;
    /// - `scroll:X:Y:DY` turns the mouse wheel there;
    /// - `shot:PATH` saves this frame as a PPM;
    /// - `info:PATH` writes where things stand, as text.
    fn run_script(&mut self) {
        self.holds.retain_mut(|(key, left)| {
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.keys.remove(key);
            }
            *left > 0
        });
        if self.pending.is_some() || !self.writers.is_empty() {
            return;
        }
        if self.script_wait > 0 {
            self.script_wait -= 1;
            return;
        }
        if self.script.is_empty() {
            self.read_remote();
        }
        let Some(step) = self.script.pop_front() else {
            return;
        };
        let parts: Vec<&str> = step.split(':').collect();
        let points = |x: &str, y: &str| {
            let ppp = self.egui_ctx.pixels_per_point();
            Some(egui::pos2(
                x.parse::<f32>().ok()? / ppp,
                y.parse::<f32>().ok()? / ppp,
            ))
        };
        match parts.as_slice() {
            [wait] if wait.starts_with("wait") => {
                self.script_wait = wait[4..].parse().unwrap_or(30);
                return;
            }
            ["hold", key, n] => {
                if let (Some(code), Ok(n)) = (key_code(key), n.parse()) {
                    self.just_pressed.push(code);
                    self.keys.insert(code);
                    self.holds.push((code, n));
                }
            }
            ["click", x, y] => {
                if let Some(pos) = points(x, y) {
                    let button = |pressed| egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    };
                    self.pointer
                        .push_back(vec![egui::Event::PointerMoved(pos), button(true)]);
                    self.pointer.push_back(vec![button(false)]);
                }
            }
            ["move", x, y] => {
                if let Some(pos) = points(x, y) {
                    self.pointer.push_back(vec![egui::Event::PointerMoved(pos)]);
                }
            }
            ["shot", path @ ..] => self.renderer.capture = Some(PathBuf::from(path.join(":"))),
            ["scroll", x, y, dy] => {
                if let (Some(pos), Ok(dy)) = (points(x, y), dy.parse::<f32>()) {
                    self.pointer.push_back(vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, dy),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
            }
            ["info", path @ ..] => {
                if let Some(game) = &self.game {
                    let _ = std::fs::write(path.join(":"), remote_info(&game.world));
                }
            }
            [key] => self.just_pressed.extend(key_code(key)),
            _ => {}
        }
        // The step after a key waits a frame for it to land.
        self.script_wait = 2;
    }

    /// A game's newest checkpoint on disk, if it has one.
    fn newest_checkpoint<'a>(
        saves: &'a [save::Entry],
        game: &str,
    ) -> Option<(&'a save::Header, &'a save::Entry)> {
        saves
            .iter()
            .filter_map(|e| Some((e.header.as_ref().ok()?, e)))
            .filter(|(h, _)| h.game.id == game && h.kind == Kind::Checkpoint)
            .max_by_key(|(h, _)| h.saved)
    }

    /// The ship was destroyed: rewind to the game's last checkpoint
    /// (DESIGN §7.7), or go to the title if it has none.
    fn restore_checkpoint(&mut self) {
        if self.pending.is_some() {
            return;
        }
        let Some(game) = &self.game else { return };
        self.saves = save::list(&self.data);
        let found = Self::newest_checkpoint(&self.saves, &game.id.id).map(|(h, e)| {
            let date_s = h.clock_s.unwrap_or(game.world.clock_s);
            let rewind = lost::Rewind {
                started: Instant::now(),
                from_s: game.world.clock_s,
                to_s: date_s,
                place: h.place.clone(),
                date: h.date.clone(),
            };
            (e.path.clone(), rewind)
        });
        match found {
            Some((path, rewind)) => {
                self.audio.play(audio::Cue::Rewind);
                self.rewind = Some(rewind);
                self.apply(MenuAction::Load(path));
            }
            None => {
                self.back_to_title();
                self.toast("No checkpoint to go back to", false);
            }
        }
    }

    /// Write a checkpoint on arriving at a station or city (DESIGN §7.7).
    fn checkpoint(&mut self) {
        let Some(game) = &mut self.game else { return };
        let here = game.world.docked_at();
        if here == game.docked {
            return;
        }
        game.docked = here;
        if here.is_some() && save::can_save(&game.world).is_ok() {
            self.write(Kind::Checkpoint);
        }
    }

    /// The game's view: flight scene or hangar bay.
    fn game_scene(&mut self, proj: Mat4) -> (Mat4, Vec<(MeshId, Instance)>, DVec3, [f32; 3]) {
        let world = &self.game.as_ref().expect("playing").world;
        let fit = world.player.ship.fit.clone();
        let bay = Self::bay_fit(world);
        (self.meshes.ship, self.meshes.ship_shape) = self.ship_mesh(&fit);
        let world = &self.game.as_ref().expect("playing").world;
        if self.meshes.system_generation != world.system_generation {
            self.meshes.stations = world
                .view
                .stations
                .iter()
                .map(|&s| {
                    let mesh = meshes::station(station_kind(world, s), s.data().as_ffi());
                    self.renderer.add_mesh(&mesh)
                })
                .collect();
            self.meshes.system_generation = world.system_generation;
            self.renderer.set_sky(&sky(world));
        }

        let (mut cam_pos, mut cam_rot) = world.camera();
        if let Some(yaw) = self.ship_view {
            let rot = world.ship.rot;
            let dir =
                DQuat::from_rotation_y(yaw.to_radians()) * DVec3::new(0.0, 0.4, 1.0).normalize();
            let offset = rot * dir * (world.stats().length_m * 1.5).max(24.0);
            cam_pos = world.ship_position() + offset;
            cam_rot = rot * world::look_rotation(-dir);
        }
        let star = star_color(world.star());
        match bay {
            // In the hangar or shipyard: a ship on the bay floor.
            Some(fit) => {
                let (mesh, _) = self.ship_mesh(&fit);
                let time = self.start.elapsed().as_secs_f32();
                let (view, instances) = bay_scene(&self.meshes, mesh, &fit, time);
                (proj * view, instances, cam_pos, star)
            }
            None => {
                let fits: Vec<_> = (self.game.as_ref().expect("playing").world)
                    .traffic
                    .contacts
                    .iter()
                    .map(|c| c.fit.clone())
                    .collect();
                let traffic: Vec<_> = fits.iter().map(|fit| self.ship_mesh(fit)).collect();
                let fits: Vec<_> = (self.game.as_ref().expect("playing").world)
                    .traffic
                    .pirates
                    .iter()
                    .flat_map(|c| c.fits.iter().cloned())
                    .collect();
                let pirates: Vec<_> = fits.iter().map(|fit| self.ship_mesh(fit)).collect();
                // Raiders, then wrecks.
                let combat = &self.game.as_ref().expect("playing").world.combat;
                let fits: Vec<_> = (combat.raiders.iter().map(|r| r.vessel.fit.clone()))
                    .chain(combat.wrecks.iter().map(|w| w.fit.clone()))
                    .collect();
                let fighting: Vec<_> = fits.iter().map(|fit| self.ship_mesh(fit)).collect();
                let world = &self.game.as_ref().expect("playing").world;
                let view = Mat4::from_quat(cam_rot.as_quat().inverse());
                let mut instances = scene(world, &self.meshes, &traffic, cam_pos);
                pirate_scene(world, &self.meshes, &pirates, cam_pos, &mut instances);
                combat_scene(world, &self.meshes, &fighting, cam_pos, &mut instances);
                (proj * view, instances, cam_pos, star)
            }
        }
    }
}

/// The UI's zoom: the player's UI scale, shrunk in a window smaller than
/// the 1600 × 960 the HUD is laid out for (#156), so its corner panels
/// keep their room and never pile onto each other.
fn hud_zoom(ui_scale: f32, window: &Window) -> f32 {
    let size = window.inner_size().to_logical::<f32>(window.scale_factor());
    if size.width <= 0.0 || size.height <= 0.0 {
        return ui_scale;
    }
    let fit = (size.width / 1600.0)
        .min(size.height / 960.0)
        .clamp(0.6, 1.0);
    ui_scale * fit
}

/// Shrinks a new window that came out bigger than its monitor, keeping its
/// shape (#156): a window past the screen's edges loses the HUD's corner
/// panels with them. Works in physical pixels, since a compositor's
/// fractional scale isn't known until the window exists.
fn fit_to_monitor(window: &Window) {
    let Some(monitor) = window.current_monitor() else {
        return;
    };
    let room = monitor.size();
    let size = window.outer_size();
    if room.width == 0 || room.height == 0 || size.width == 0 || size.height == 0 {
        return;
    }
    // Room for the desktop's own bars round the window.
    let fit = (room.width as f64 * 0.95 / size.width as f64)
        .min(room.height as f64 * 0.9 / size.height as f64);
    if fit < 1.0 {
        let inner = window.inner_size();
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(
            (inner.width as f64 * fit) as u32,
            (inner.height as f64 * fit) as u32,
        ));
    }
}

/// Everything visible, positioned relative to the camera.
fn scene(
    world: &World,
    meshes: &Meshes,
    traffic: &[(MeshId, meshes::ShipShape)],
    cam: DVec3,
) -> Vec<(MeshId, Instance)> {
    let t = world.clock_s;
    let mut out = Vec::new();
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };

    // The star, never smaller than a small disc so it reads from anywhere.
    let star_r = world.star_radius_m().max(cam.length() * 0.0035);
    let black_hole = world.star() == StarClass::BlackHole;
    let star_rgb = if black_hole {
        [0.02, 0.0, 0.03]
    } else {
        star_color(world.star())
    };
    // A far star is a point that has to burn through the bloom; a near
    // one is a disc whose surface should show, so it's exposed down as it
    // grows (#156), like an eye adjusting.
    let across = (star_r / cam.length().max(1.0)) as f32;
    let brightness = 6.0 - 4.4 * smoothstep(0.004, 0.04, across);
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale_rotation_translation(
                Vec3::splat(star_r as f32),
                Quat::IDENTITY,
                rel(DVec3::ZERO),
            ),
            star_rgb,
            Surface::Star,
            brightness,
            0.0,
            [0.0, 1.0, 0.0, 1.0],
        ),
    ));
    if !black_hole {
        const CORONA: f32 = 3.0;
        out.push((
            meshes.sphere,
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(star_r as f32 * CORONA),
                    Quat::IDENTITY,
                    rel(DVec3::ZERO),
                ),
                star_rgb,
                Surface::Corona,
                brightness * 0.45,
                world.system.index() as f32 % 1000.0,
                [0.0, 1.0, 0.0, 1.0],
            )
            .with_detail(CORONA),
        ));
    }

    for &b in &world.view.bodies {
        let body = &world.galaxy.bodies[b];
        let pos = world.position(Place::Body(b));
        let seed = b.index() as u64 * 7919 % 10_000;
        let (surface, color) = match body.kind {
            BodyKind::Planet(PlanetClass::Barren) => (Surface::Barren, [0.42, 0.39, 0.36]),
            BodyKind::Planet(PlanetClass::Desert) => (Surface::Desert, [0.72, 0.52, 0.32]),
            BodyKind::Planet(PlanetClass::Terran) => (Surface::Ocean, [0.36, 0.42, 0.3]),
            BodyKind::Planet(PlanetClass::Ocean) => (Surface::Ocean, [0.2, 0.35, 0.5]),
            BodyKind::Planet(PlanetClass::Ice) => (Surface::Ice, [0.65, 0.75, 0.82]),
            BodyKind::GasGiant => (Surface::Gas, gas_color(seed)),
            BodyKind::Moon => (Surface::Barren, [0.4, 0.38, 0.36]),
            BodyKind::Belt => continue,
        };
        // How much of an ocean world is sea.
        let sea_level = match body.kind {
            BodyKind::Planet(PlanetClass::Terran) => 0.52,
            BodyKind::Planet(PlanetClass::Ocean) => 0.6,
            _ => 0.0,
        };
        let spin = Quat::from_rotation_y(
            (t / 86_400.0 * std::f64::consts::TAU % std::f64::consts::TAU) as f32,
        );
        out.push((
            meshes.sphere,
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat((body.radius_km * 1000.0) as f32),
                    spin,
                    rel(pos),
                ),
                color,
                surface,
                0.0,
                seed as f32,
                light(pos),
            )
            .with_detail(sea_level),
        ));
    }

    for (i, &s) in world.view.stations.iter().enumerate() {
        let pos = world.position(Place::Station(s));
        let spin = if station_kind(world, s) == StationKind::Habitat {
            (t * 0.05 % std::f64::consts::TAU) as f32
        } else {
            0.0
        };
        let Some(&mesh) = meshes.stations.get(i) else {
            continue;
        };
        out.push((
            mesh,
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(world.station_size_m(s) as f32 * 0.5),
                    Quat::from_rotation_y(spin) * Quat::from_rotation_x(0.3),
                    rel(pos),
                ),
                [0.74, 0.71, 0.63],
                Surface::Plain,
                0.0,
                i as f32,
                light(pos),
            )
            .with_detail(24.0),
        ));
    }

    if let Some((field, rocks)) = &world.field_rocks {
        mining_scene(world, meshes, *field, rocks, cam, &mut out);
    }

    traffic_scene(world, meshes, traffic, cam, &mut out);

    let hidden = world.cockpit
        || world.combat.lost.is_some()
        || !matches!(world.mode, Mode::Normal | Mode::FastTravel(_));
    if !hidden {
        let pos = world.ship_position();
        let rot = world.ship.rot.as_quat();
        out.push((
            meshes.ship,
            Instance::new(
                Mat4::from_rotation_translation(rot, rel(pos)),
                [0.8, 0.75, 0.64],
                Surface::Plain,
                0.0,
                0.0,
                light(pos),
            )
            .with_detail(0.5)
            .with_scorch(battle_scorch(world.combat.player.hull_left()))
            .own_hull(),
        ));
        // Engine plumes, growing with the throttle.
        let thrust = world.ship.throttle.max(0.0) as f32;
        for &(nozzle, r) in meshes.ship_shape.nozzles.iter().filter(|_| thrust > 0.01) {
            let len = r * (1.0 + 3.0 * thrust);
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(r * 0.8, r * 0.8, len),
                        rot,
                        rel(pos) + rot * (nozzle + Vec3::Z * len * 0.6),
                    ),
                    [0.35, 0.7, 1.0],
                    Surface::Star,
                    1.2 + 2.5 * thrust,
                    0.0,
                    [0.0, 1.0, 0.0, 1.0],
                ),
            ));
        }
        // The gas scoop open in a giant's clouds: the same plume NPC
        // skimmers trail.
        if world.skim.open {
            // Scaled down: the chase camera sits right behind it.
            let length = world.stats().length_m as f32 * 0.3;
            harvest_glow(
                meshes,
                retrox_sim::economy::Good::Gas,
                pos,
                rot,
                length,
                7,
                world.clock_s,
                cam,
                &mut out,
            );
        }
        // The mining laser.
        if let Some((from, to)) = world.beam {
            let (a, b) = (pos + from, pos + to);
            let len = (b - a).length();
            let flicker = 0.8 + 0.2 * ((t * 53.0).sin() as f32);
            let on_vein = world.mining.aim.is_some_and(|a| a.on_vein);
            let color = if on_vein {
                [1.0, 0.85, 0.35]
            } else {
                [1.0, 0.32, 0.18]
            };
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(0.22, 0.22, len as f32 / 2.0),
                        world::look_rotation(b - a).as_quat(),
                        rel((a + b) / 2.0),
                    ),
                    color,
                    Surface::Star,
                    4.0 * flicker,
                    0.0,
                    [0.0, 1.0, 0.0, 1.0],
                ),
            ));
            // Where it bites: a hot spot, bigger on a vein.
            if world.mining.aim.is_some() {
                let r = if on_vein { 2.2 } else { 1.3 } * flicker;
                out.push((
                    meshes.sphere,
                    Instance::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(r),
                            Quat::IDENTITY,
                            rel(b),
                        ),
                        color,
                        Surface::Star,
                        6.0,
                        0.0,
                        [0.0, 1.0, 0.0, 1.0],
                    ),
                ));
            }
        }
    }
    out
}

/// Ships further than this aren't drawn: they'd be less than a pixel.
const TRAFFIC_DRAW_M: f64 = 300e3;

/// The bubble's fleets in this system: each ship of a fleet in formation,
/// nose along its thrust (so a braking ship flies tail first), with its
/// engines lit by how hard it's burning.
fn traffic_scene(
    world: &World,
    meshes: &Meshes,
    traffic: &[(MeshId, meshes::ShipShape)],
    cam: DVec3,
    out: &mut Vec<(MeshId, Instance)>,
) {
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };
    for (c, (mesh, shape)) in world.traffic.contacts.iter().zip(traffic) {
        // A fleet in the fight's scene is drawn as its ships there.
        if (c.pos - cam).length() > TRAFFIC_DRAW_M || world.combat.has_convoy(c.key) {
            continue;
        }
        let burn = (c.thrust.length() / c.accel.max(1e-9)).clamp(0.0, 1.0) as f32;
        let heading = if burn > 0.02 { c.thrust } else { c.vel };
        let rot = world::look_rotation(heading);
        let seed = c.key.0.data().as_ffi() ^ u64::from(c.key.1);
        let length = c.fit.stats().length_m;
        let working = c.work.as_ref().filter(|w| w.at_source);
        for (i, offset) in traffic::formation(c.ships, length, seed)
            .into_iter()
            .enumerate()
        {
            let pos = c.pos + rot * offset;
            let q = rot.as_quat();
            if let Some(w) = working {
                harvest_glow(
                    meshes,
                    w.good,
                    pos,
                    q,
                    length as f32,
                    seed ^ i as u64,
                    world.clock_s,
                    cam,
                    out,
                );
            }
            out.push((
                *mesh,
                Instance::new(
                    Mat4::from_rotation_translation(q, rel(pos)),
                    [0.8, 0.75, 0.64],
                    Surface::Plain,
                    0.0,
                    0.0,
                    light(pos),
                )
                .with_detail(0.5),
            ));
            for &(nozzle, r) in shape.nozzles.iter().filter(|_| burn > 0.02) {
                let len = r * (1.0 + 3.0 * burn);
                out.push((
                    meshes.sphere,
                    Instance::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(r * 0.8, r * 0.8, len),
                            q,
                            rel(pos) + q * (nozzle + Vec3::Z * len * 0.6),
                        ),
                        [0.35, 0.7, 1.0],
                        Surface::Star,
                        1.2 + 2.5 * burn,
                        0.0,
                        [0.0, 1.0, 0.0, 1.0],
                    ),
                ));
            }
        }
    }
}

/// Pirate gangs flying in this system: each gang's ships in formation,
/// drawn like the traders.
fn pirate_scene(
    world: &World,
    meshes: &Meshes,
    ships: &[(MeshId, meshes::ShipShape)],
    cam: DVec3,
    out: &mut Vec<(MeshId, Instance)>,
) {
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };
    let mut next = ships.iter();
    for c in &world.traffic.pirates {
        let these: Vec<_> = next.by_ref().take(c.fits.len()).collect();
        if (c.pos - cam).length() > TRAFFIC_DRAW_M {
            continue;
        }
        let burn = (c.thrust.length() / c.accel.max(1e-9)).clamp(0.0, 1.0) as f32;
        let heading = if burn > 0.02 { c.thrust } else { c.vel };
        let rot = world::look_rotation(heading.normalize_or(DVec3::NEG_Z));
        let seed = c.gang.data().as_ffi();
        let length = c.fits.first().map_or(20.0, |f| f.stats().length_m);
        let offsets = traffic::formation(c.fits.len() as u32, length, seed);
        for ((mesh, shape), offset) in these.into_iter().zip(offsets) {
            let pos = c.pos + rot * offset;
            let q = rot.as_quat();
            out.push((
                *mesh,
                Instance::new(
                    Mat4::from_rotation_translation(q, rel(pos)),
                    [0.8, 0.75, 0.64],
                    Surface::Plain,
                    0.0,
                    0.0,
                    light(pos),
                )
                .with_detail(0.5),
            ));
            for &(nozzle, r) in shape.nozzles.iter().filter(|_| burn > 0.02) {
                let len = r * (1.0 + 3.0 * burn);
                out.push((
                    meshes.sphere,
                    Instance::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(r * 0.8, r * 0.8, len),
                            q,
                            rel(pos) + q * (nozzle + Vec3::Z * len * 0.6),
                        ),
                        [1.0, 0.45, 0.25],
                        Surface::Star,
                        1.2 + 2.5 * burn,
                        0.0,
                        [0.0, 1.0, 0.0, 1.0],
                    ),
                ));
            }
        }
    }
}

/// A harvester at work (issue #68), drawn round one ship at `pos` facing
/// `q`: at an ice source, its laser cutting a boulder off its bow; at a gas
/// giant, the glowing wake it ploughs through the cloud tops.
#[allow(clippy::too_many_arguments)]
fn harvest_glow(
    meshes: &Meshes,
    good: retrox_sim::economy::Good,
    pos: DVec3,
    q: Quat,
    length: f32,
    seed: u64,
    clock_s: f64,
    cam: DVec3,
    out: &mut Vec<(MeshId, Instance)>,
) {
    let rel = |p: DVec3| (p - cam).as_vec3();
    let phase = (seed % 1000) as f64 * 0.37;
    let flicker = 0.8 + 0.2 * ((clock_s * 53.0 + phase).sin() as f32);
    let unlit = [0.0, 1.0, 0.0, 1.0];
    let length = length.max(10.0);
    match good {
        retrox_sim::economy::Good::Gas => {
            // Ploughing the cloud tops: a thin bright contrail streaming
            // astern (it reads against the bright cloud), and churning
            // plasma where the hull meets the gas.
            let trail = length * 7.0;
            let breathe = 0.85 + 0.15 * ((clock_s * 1.7 + phase).sin() as f32);
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(length * 0.07, length * 0.05, trail / 2.0),
                        q,
                        rel(pos) + q * (Vec3::Z * (trail / 2.0 + length * 0.2)),
                    ),
                    [1.0, 0.95, 0.86],
                    Surface::Star,
                    1.6 * breathe,
                    0.0,
                    unlit,
                ),
            ));
            let plume = length * 1.6;
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(length * 0.2, length * 0.16, plume / 2.0),
                        q,
                        rel(pos) + q * (Vec3::Z * (plume / 2.0 + length * 0.1)),
                    ),
                    [1.0, 0.6, 0.3],
                    Surface::Fire,
                    5.0 * breathe,
                    (seed % 977) as f32,
                    [0.0, 1.0, 0.0, 0.25],
                ),
            ));
        }
        _ => {
            // The laser out ahead to a boulder of ice it's cutting up.
            let reach = length * 2.0;
            let rock_r = length * 0.3;
            let bow = q * (Vec3::NEG_Z * length * 0.5);
            let hit = q * (Vec3::NEG_Z * (length * 0.5 + reach));
            let mesh = meshes.rocks[mining::rock_mesh(seed)][1];
            let spin = Quat::from_rotation_y((clock_s * 0.05 + phase) as f32)
                * Quat::from_rotation_x(phase as f32);
            out.push((
                mesh,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(rock_r),
                        spin,
                        rel(pos) + hit + q * (Vec3::NEG_Z * rock_r * 0.8),
                    ),
                    [0.58, 0.66, 0.74],
                    Surface::Plain,
                    0.05,
                    0.0,
                    [0.0, 1.0, 0.0, 1.3],
                ),
            ));
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(0.25, 0.25, reach / 2.0),
                        q,
                        rel(pos) + (bow + hit) / 2.0,
                    ),
                    [1.0, 0.32, 0.18],
                    Surface::Star,
                    4.0 * flicker,
                    0.0,
                    unlit,
                ),
            ));
            out.push((
                meshes.sphere,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(1.4 * flicker),
                        Quat::IDENTITY,
                        rel(pos) + hit,
                    ),
                    [1.0, 0.32, 0.18],
                    Surface::Star,
                    6.0,
                    0.0,
                    unlit,
                ),
            ));
        }
    }
}

/// How scorched a ship looks with this much hull left.
/// The combat target, where the chase camera has the pilot's own hull
/// between it and the eye: camera relative, with its radius in metres.
/// The renderer opens the hull round it (DESIGN §7.2.1, #123).
fn hull_reveal(world: &World, cam: DVec3) -> Option<(Vec3, f32)> {
    let c = &world.combat;
    if world.cockpit || c.lost.is_some() || world.mode != Mode::Normal {
        return None;
    }
    let r = c.raider(c.target?)?;
    let rel = (world.combat_origin() + r.craft.pos - cam).as_vec3();
    Some((rel, (r.vessel.stats.length_m * 0.5) as f32))
}

fn battle_scorch(hull_left: f64) -> f32 {
    (((1.0 - hull_left) * 0.8) as f32).clamp(0.0, 0.8)
}

/// Tracer and pulse colours: the player's guns warm cream, everyone
/// else's red.
fn gun_color(side: combat::Side, laser: bool) -> [f32; 3] {
    match (side, laser) {
        (combat::Side::Player, false) => [1.0, 0.8, 0.45],
        (combat::Side::Player, true) => [0.5, 1.0, 0.88],
        (_, false) => [1.0, 0.3, 0.14],
        (_, true) => [1.0, 0.18, 0.32],
    }
}

/// The fight: raiders and wrecks, shells and pulses, shields lighting up,
/// sparks, fireballs, shockwaves and flying plates. `ships` holds the
/// raiders' meshes, then the wrecks'.
fn combat_scene(
    world: &World,
    meshes: &Meshes,
    ships: &[(MeshId, meshes::ShipShape)],
    cam: DVec3,
    out: &mut Vec<(MeshId, Instance)>,
) {
    let c = &world.combat;
    let origin = world.combat_origin();
    let rel = |p: DVec3| (origin + p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-(origin + p)).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };
    let glow = |m: Mat4, color: [f32; 3], surface: Surface, strength: f32, light: [f32; 4]| {
        Instance::new(m, color, surface, strength, 0.0, light)
    };
    let unlit = [0.0, 1.0, 0.0, 1.0];

    let (raider_meshes, wreck_meshes) = ships.split_at(c.raiders.len().min(ships.len()));
    for (r, (mesh, shape)) in c.raiders.iter().zip(raider_meshes) {
        let q = r.craft.rot.as_quat();
        let pos = r.craft.pos;
        out.push((
            *mesh,
            Instance::new(
                Mat4::from_rotation_translation(q, rel(pos)),
                [0.8, 0.75, 0.64],
                Surface::Plain,
                0.0,
                r.id as f32,
                light(pos),
            )
            .with_detail(0.5)
            .with_scorch(battle_scorch(r.vessel.hull_left())),
        ));
        let burn = r.burn.max(0.15) as f32;
        for &(nozzle, radius) in &shape.nozzles {
            let len = radius * (1.0 + 3.0 * burn);
            out.push((
                meshes.sphere,
                glow(
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(radius * 0.8, radius * 0.8, len),
                        q,
                        rel(pos) + q * (nozzle + Vec3::Z * len * 0.6),
                    ),
                    [1.0, 0.45, 0.25],
                    Surface::Star,
                    1.2 + 2.5 * burn,
                    unlit,
                ),
            ));
        }
    }
    for (w, (mesh, _)) in c.wrecks.iter().zip(wreck_meshes) {
        // Embers die down over the first minute.
        let scorch = 1.0 - (w.age / 60.0).min(1.0) as f32 * 0.14;
        out.push((
            *mesh,
            Instance::new(
                Mat4::from_rotation_translation(w.rot().as_quat(), rel(w.pos)),
                [0.8, 0.75, 0.64],
                Surface::Plain,
                0.0,
                7.0,
                light(w.pos),
            )
            .with_detail(0.5)
            .with_scorch(scorch),
        ));
    }

    // Shells: streaks along their flight.
    for s in &c.shots {
        // One about to pass through the camera would fill the screen.
        let near = (origin + s.pos - cam).length();
        if near < 25.0 {
            continue;
        }
        let len = (s.vel.length() * 0.012).clamp(6.0, 30.0) as f32;
        out.push((
            meshes.sphere,
            glow(
                Mat4::from_scale_rotation_translation(
                    Vec3::new(0.24, 0.24, len),
                    world::look_rotation(s.vel).as_quat(),
                    rel(s.pos),
                ),
                gun_color(s.from, false),
                Surface::Star,
                5.0,
                unlit,
            ),
        ));
    }
    for p in &c.pulses {
        let len = (p.to - p.from).length();
        let k = (p.age / combat::PULSE_LIFE) as f32;
        out.push((
            meshes.sphere,
            glow(
                Mat4::from_scale_rotation_translation(
                    Vec3::new(0.3, 0.3, len as f32 / 2.0),
                    world::look_rotation(p.to - p.from).as_quat(),
                    rel((p.from + p.to) / 2.0),
                ),
                gun_color(p.side, true),
                Surface::Star,
                7.0 * (1.0 - k),
                unlit,
            ),
        ));
    }

    for f in &c.flashes {
        let k = (f.age / f.life) as f32;
        match f.kind {
            combat::FlashKind::Muzzle => out.push((
                meshes.sphere,
                glow(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(f.size as f32 * (1.0 - 0.5 * k)),
                        Quat::IDENTITY,
                        rel(f.pos),
                    ),
                    [1.0, 0.82, 0.55],
                    Surface::Star,
                    4.0 * (1.0 - k),
                    unlit,
                ),
            )),
            combat::FlashKind::Spark => out.push((
                meshes.sphere,
                glow(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(f.size as f32 * (0.4 + 0.6 * k.sqrt())),
                        Quat::IDENTITY,
                        rel(f.pos),
                    ),
                    [1.0, 0.7, 0.4],
                    Surface::Star,
                    8.0 * (1.0 - k).powi(2),
                    unlit,
                ),
            )),
            combat::FlashKind::Shield { ship, dir } => {
                let (vessel, craft) = match ship {
                    combat::Side::Player if !world.cockpit => (&c.player, world.player_craft()),
                    combat::Side::Raider(id) => match c.raider(id) {
                        Some(r) => (&r.vessel, r.craft),
                        None => continue,
                    },
                    combat::Side::Player => continue,
                };
                let (across, along) = vessel.bubble();
                out.push((
                    meshes.sphere,
                    glow(
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(across as f32, across as f32, along as f32),
                            craft.rot.as_quat(),
                            rel(craft.pos),
                        ),
                        [0.3, 0.85, 1.0],
                        Surface::Shield,
                        if ship == combat::Side::Player {
                            0.7
                        } else {
                            1.6
                        },
                        [dir.x as f32, dir.y as f32, dir.z as f32, k],
                    ),
                ));
            }
        }
    }

    for b in c.blasts.iter().filter(|b| b.age >= 0.0) {
        let k = (b.age / b.life) as f32;
        let grow = 1.0 - (1.0 - k).powi(3);
        let r = b.radius as f32 * (0.3 + 0.8 * grow);
        out.push((
            meshes.sphere,
            Instance::new(
                Mat4::from_scale_rotation_translation(Vec3::splat(r), Quat::IDENTITY, rel(b.pos)),
                [1.0, 0.5, 0.2],
                Surface::Fire,
                3.0,
                b.seed as f32,
                [0.0, 1.0, 0.0, k],
            ),
        ));
        // A blinding core, for the first moment.
        if k < 0.12 {
            out.push((
                meshes.sphere,
                glow(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(b.radius as f32 * 0.45),
                        Quat::IDENTITY,
                        rel(b.pos),
                    ),
                    [1.0, 0.92, 0.78],
                    Surface::Star,
                    30.0 * (1.0 - k / 0.12),
                    unlit,
                ),
            ));
        }
        // The shockwave: a ring racing out, tilted by the blast's seed.
        if !b.ring {
            continue;
        }
        let tilt =
            Quat::from_rotation_x(b.seed as f32 * 1.7) * Quat::from_rotation_z(b.seed as f32 * 0.9);
        out.push((
            meshes.ring,
            glow(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(b.radius as f32 * (0.4 + 2.6 * k.sqrt())),
                    tilt,
                    rel(b.pos),
                ),
                [1.0, 0.62, 0.32],
                Surface::Ring,
                2.4,
                [0.0, 1.0, 0.0, k],
            ),
        ));
    }

    for d in &c.debris {
        let k = (d.age / d.life) as f32;
        if d.ember {
            out.push((
                meshes.sphere,
                glow(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(d.size.x as f32 * (1.0 - k * 0.6)),
                        Quat::IDENTITY,
                        rel(d.pos),
                    ),
                    [1.0, 0.48, 0.14],
                    Surface::Star,
                    8.0 * (1.0 - k),
                    unlit,
                ),
            ));
        } else {
            out.push((
                meshes.plate,
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        d.size.as_vec3(),
                        d.rot().as_quat(),
                        rel(d.pos),
                    ),
                    [0.55, 0.52, 0.46],
                    Surface::Plain,
                    0.0,
                    3.0,
                    light(d.pos),
                )
                .with_scorch(0.7 + 0.3 * (1.0 - (d.age / 8.0).min(1.0) as f32)),
            ));
        }
    }
}

/// Glow colour of a good's veins and loose chunks.
fn ore_glow(good: retrox_sim::economy::Good) -> [f32; 3] {
    match good {
        retrox_sim::economy::Good::Ice => [0.45, 0.85, 1.0],
        _ => [1.0, 0.6, 0.18],
    }
}

/// The rocks of the field the ship is in, as mined so far: rocks and
/// fragments, the veins of rocks that have been read, loose ore and the
/// flashes of fractures.
fn mining_scene(
    world: &World,
    meshes: &Meshes,
    field: retrox_sim::FieldId,
    rocks: &[retrox_gen::Rock],
    cam: DVec3,
    out: &mut Vec<(MeshId, Instance)>,
) {
    let t = world.clock_s;
    let center = world.position(Place::Field(field));
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };
    let lit = light(center);
    let pulse = (0.6 + 0.4 * (t * 4.0).sin()) as f32;
    let f = &world.galaxy.fields[field];
    let field_r = f.radius_km * 1000.0;
    let dust = mining::BELT_DUST as f32;
    // The belt's dust round the camera, sunlit.
    let from_centre = cam - center;
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale(Vec3::splat(1.0e6)),
            [1.0; 3],
            Surface::Dust,
            1.0,
            0.0,
            lit,
        )
        .with_detail(dust)
        .with_aux([
            from_centre.x as f32,
            from_centre.y as f32,
            from_centre.z as f32,
            field_r as f32,
        ]),
    ));
    let icy = f.resources.ice > f.resources.ore;
    rubble_scene(meshes, from_centre, field_r, f.seed, icy, lit, t, out);
    for rock in world.mining.live_rocks(field, f, rocks, t) {
        let pos = center + rock.pos;
        let mut color = match rock.lode.good {
            retrox_sim::economy::Good::Ice => [0.6, 0.66, 0.72],
            _ => [0.42, 0.37, 0.33],
        };
        if rock.scanned && rock.lode.volatile {
            color = [0.5, 0.3, 0.26];
        }
        if rock.worked_out {
            color = color.map(|c| c * 0.6);
        }
        let distance = (pos - cam).length();
        // Veins, once read, on rocks near enough to matter: seams of ore
        // in the rock (mesh.wgsl).
        let veins: Vec<Vec3> = if rock.scanned && distance < mining::SCAN_RANGE + rock.radius {
            rock.vein_marks().collect()
        } else {
            Vec::new()
        };
        let kind = match (veins.is_empty(), rock.lode.volatile, rock.lode.good) {
            (true, _, _) => VeinKind::None,
            (false, true, _) => VeinKind::Volatile,
            (false, false, retrox_sim::economy::Good::Ice) => VeinKind::Ice,
            _ => VeinKind::Ore,
        };
        out.push((
            rock_lod(meshes, rock.seed, rock.radius, distance),
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(rock.radius as f32),
                    rock.rot.as_quat(),
                    rel(pos),
                ),
                color,
                Surface::Rock,
                // Glowing from the laser's work.
                (rock.damage as f32).powi(2) * 0.25,
                (rock.seed % 10_000) as f32,
                lit,
            )
            .with_veins(&veins, kind, dust),
        ));
    }
    if let Some(state) = world.mining.field(field) {
        for n in &state.nuggets {
            let size = (0.5 + 1.1 * n.tonnes.cbrt()) as f32;
            let spin = Quat::from_rotation_y((t * 0.7 + n.seed as f64 % 6.0) as f32)
                * Quat::from_rotation_x((t * 0.4) as f32);
            out.push((
                meshes.rocks[mining::rock_mesh(n.seed)][0],
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(size),
                        spin,
                        rel(center + n.pos),
                    ),
                    ore_glow(n.good),
                    Surface::Plain,
                    0.6 + 0.5 * pulse,
                    0.0,
                    lit,
                ),
            ));
        }
    }
    for b in &world.mining.bursts {
        let k = (b.age / b.life) as f32;
        let glow = if b.volatile {
            [1.0, 0.45, 0.2]
        } else {
            ore_glow(b.good)
        };
        out.push((
            meshes.sphere,
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(b.radius as f32 * (0.25 + 0.75 * k.sqrt())),
                    Quat::IDENTITY,
                    rel(center + b.pos),
                ),
                glow,
                Surface::Flash,
                if b.volatile { 3.0 } else { 1.6 },
                0.0,
                [0.0, 1.0, 0.0, k],
            ),
        ));
    }
}

/// Which detail of a rock's mesh to draw, by how big it looks: a speck, a
/// rock across the field, or one close enough that its outline shows.
fn rock_lod(meshes: &Meshes, seed: u64, radius: f64, distance: f64) -> MeshId {
    let looks = radius / distance.max(1.0);
    let lod = if looks > 0.05 {
        2
    } else if looks > 0.006 {
        1
    } else {
        0
    };
    meshes.rocks[mining::rock_mesh(seed)][lod]
}

/// Gravel and boulders between the rocks, too small to work (the
/// smallest rock worth mining is 8 m across), which make a belt look like
/// one: dense and deep, thinning toward its edge as the rocks do. Fixed
/// in the field, so they slide past as the ship flies. Boulders far off
/// are specks that shrink away before the ship gets near, and gravel
/// shrinks away from right round the ship, so nothing that can't be mined
/// looks as if it could be (DESIGN §8.7).
#[allow(clippy::too_many_arguments)]
fn rubble_scene(
    meshes: &Meshes,
    from_centre: DVec3,
    field_r: f64,
    field_seed: u64,
    icy: bool,
    lit: [f32; 4],
    t: f64,
    out: &mut Vec<(MeshId, Instance)>,
) {
    // Cell size, reach, sizes (radius, metres), share of cells with one,
    // how near it shows, and a salt.
    let layers = [
        (110.0, 1400.0, (0.4f64, 2.2f64), 0.45, 25.0, 1u64),
        (480.0, 7000.0, (2.5, 5.5), 0.6, 1600.0, 2u64),
    ];
    let color = if icy {
        [0.6, 0.66, 0.72]
    } else {
        [0.42, 0.37, 0.33]
    };
    for (cell, reach, (small, big), share, near, layer) in layers {
        let lo = ((from_centre - DVec3::splat(reach)) / cell).floor();
        let hi = ((from_centre + DVec3::splat(reach)) / cell).floor();
        for x in lo.x as i64..=hi.x as i64 {
            for y in lo.y as i64..=hi.y as i64 {
                for z in lo.z as i64..=hi.z as i64 {
                    let c = DVec3::new(x as f64, y as f64, z as f64) * cell;
                    // As the rocks lie: flattened, thinning out.
                    let rr = DVec3::new(c.x, c.y / 0.35, c.z).length() / field_r;
                    if rr > 1.0 {
                        continue;
                    }
                    let h = cell_hash(x, y, z, layer ^ field_seed);
                    let thick = share * f64::from(1.0 - smoothstep(0.75, 1.0, rr as f32));
                    if unit_of(h) > thick {
                        continue;
                    }
                    let h2 = cell_hash(x, y, z, layer.wrapping_mul(977) ^ field_seed);
                    let jitter = DVec3::new(unit_of(h2), unit_of(h2 >> 21), unit_of(h2 >> 42));
                    let p = c + jitter * cell;
                    let d = (p - from_centre).length();
                    if d > reach {
                        continue;
                    }
                    let fade = smoothstep(reach as f32, reach as f32 * 0.75, d as f32)
                        * smoothstep(near as f32 * 0.6, near as f32, d as f32);
                    if fade <= 0.0 {
                        continue;
                    }
                    let size =
                        small * (big / small).powf(unit_of(h >> 11).powi(2)) * f64::from(fade);
                    let axis = Vec3::new(
                        unit_of(h >> 7) as f32 - 0.5,
                        unit_of(h >> 19) as f32 - 0.5,
                        unit_of(h >> 31) as f32 - 0.5,
                    )
                    .normalize_or(Vec3::Y);
                    let turn =
                        (unit_of(h >> 43) * 6.3 + t * 0.2 / size.sqrt()) % std::f64::consts::TAU;
                    let seed = h % 10_000;
                    out.push((
                        rock_lod(meshes, seed, size, d),
                        Instance::new(
                            Mat4::from_scale_rotation_translation(
                                Vec3::splat(size as f32),
                                Quat::from_axis_angle(axis, turn as f32),
                                (p - from_centre).as_vec3(),
                            ),
                            color,
                            Surface::Rock,
                            0.0,
                            seed as f32,
                            lit,
                        )
                        .with_veins(
                            &[],
                            VeinKind::None,
                            mining::BELT_DUST as f32,
                        ),
                    ));
                }
            }
        }
    }
}

/// A well-mixed hash of a lattice cell.
fn cell_hash(x: i64, y: i64, z: i64, salt: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ salt.wrapping_mul(0x27d4_eb2f_1656_67c5);
    h ^= h >> 31;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 32)
}

/// 21 bits of a hash as 0..1.
fn unit_of(h: u64) -> f64 {
    (h & 0x1f_ffff) as f64 / 0x1f_ffff as f64
}

/// Hermite step from 0 at `a` to 1 at `b`.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A ship on the hangar bay floor, the camera slowly circling it. The bay
/// is sized to the ship. Returns the view matrix and what to draw.
fn bay_scene(
    meshes: &Meshes,
    ship: MeshId,
    fit: &retrox_sim::ship::Loadout,
    time: f32,
) -> (Mat4, Vec<(MeshId, Instance)>) {
    let size = (fit.stats().length_m as f32).max(16.0);
    let angle = time * 0.05 + 0.9;
    let eye = Vec3::new(angle.sin() * 1.05, 0.16, angle.cos() * 1.05) * size;
    // Aim a little to the ship's left so it stands clear of the panels on
    // the left of the screen.
    let right = (-eye).normalize().cross(Vec3::Y).normalize();
    let view = glam::camera::rh::view::look_at_mat4(eye, -right * 0.22 * size, Vec3::Y);
    // Overhead work lights.
    let light = {
        let d = Vec3::new(0.35, 0.8, 0.5).normalize();
        [d.x, d.y, d.z, 1.1]
    };
    // Unlike the flight scene this keeps the camera's translation in the
    // view: the bay is small enough that precision doesn't matter, and its
    // walls hide the sky.
    let instances = vec![
        (
            meshes.hangar_bay,
            Instance::new(
                Mat4::from_scale(Vec3::splat(size)),
                [0.16, 0.24, 0.23],
                Surface::Plain,
                0.0,
                0.0,
                light,
            )
            .with_detail(5.0),
        ),
        (
            ship,
            Instance::new(
                Mat4::IDENTITY,
                [0.8, 0.75, 0.64],
                Surface::Plain,
                0.0,
                0.0,
                light,
            )
            .with_detail(0.5),
        ),
    ];
    (view, instances)
}

/// Identifies a fit's parts, to notice when the ship needs rebuilding.
fn fit_key(fit: &retrox_sim::ship::Loadout) -> String {
    let mut key = fit.hull().id.to_owned();
    for (_, section) in fit.mounts() {
        let Some(section) = section else {
            key.push_str("|-");
            continue;
        };
        key.push('|');
        key.push_str(section.section.id);
        for (_, module) in section.slots() {
            key.push(',');
            key.push_str(module.map_or("", |m| m.id));
        }
    }
    key
}

/// What a station looks like: a habitat ring for populous ones, otherwise
/// shaped by its biggest industry.
fn station_kind(world: &World, s: SettlementId) -> StationKind {
    let Some(settlement) = world.galaxy.settlements.get(s) else {
        return StationKind::Outpost;
    };
    let economy = &settlement.economy;
    if economy.population >= 3.0 {
        return StationKind::Habitat;
    }
    economy
        .facilities
        .iter()
        .max_by(|a, b| a.scale.total_cmp(&b.scale))
        .map_or(StationKind::Outpost, |f| StationKind::Facility(f.kind))
}

/// Linear RGB for a star's light and disc.
fn star_color(class: StarClass) -> [f32; 3] {
    use StarClass::*;
    match class {
        O => [0.62, 0.72, 1.0],
        B => [0.72, 0.8, 1.0],
        A => [0.9, 0.92, 1.0],
        F => [1.0, 0.97, 0.9],
        G => [1.0, 0.92, 0.75],
        K => [1.0, 0.76, 0.5],
        M => [1.0, 0.55, 0.35],
        WhiteDwarf => [0.88, 0.94, 1.0],
        Neutron => [0.7, 0.9, 1.0],
        BlackHole => [0.05, 0.02, 0.08],
    }
}

fn gas_color(seed: u64) -> [f32; 3] {
    const COLORS: [[f32; 3]; 5] = [
        [0.75, 0.6, 0.42],
        [0.55, 0.62, 0.72],
        [0.7, 0.5, 0.45],
        [0.6, 0.66, 0.5],
        [0.78, 0.72, 0.6],
    ];
    COLORS[(seed % COLORS.len() as u64) as usize]
}

/// How bright a point of starlight is drawn per unit of flux reaching us
/// (solar luminosities per square light-year): the camera's sensitivity.
/// Set so that a star at the naked-eye limit (magnitude ~6.5, the Sun seen
/// from ~75 ly) is just visible and a magnitude-0 star blooms.
const STARLIGHT_EXPOSURE: f64 = 150.0;

/// The sky: every other system in the generated galaxy, in its true
/// direction from here, carrying exactly the light its star sends us:
/// luminosity over distance squared, in the star's own colour. Nothing else
/// is drawn behind it; the galaxy's band is just these stars adding up.
fn sky(world: &World) -> Vec<SkyVertex> {
    let here = world.galaxy.systems[world.system].position;
    world
        .galaxy
        .systems
        .iter()
        .filter(|(id, _)| *id != world.system)
        .filter_map(|(_, s)| {
            let d = DVec3::new(
                s.position.x - here.x,
                s.position.z - here.z,
                s.position.y - here.y,
            );
            let ly = d.length();
            let flux = s.star.luminosity_suns() / (ly * ly);
            if flux <= 0.0 {
                return None; // black holes send us nothing
            }
            let brightness = (flux * STARLIGHT_EXPOSURE) as f32;
            let c = star_color(s.star);
            // Colour only: the flux carries the brightness.
            let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            Some(SkyVertex {
                dir: (d / ly).as_vec3().into(),
                color: c.map(|x| x / lum * brightness),
            })
        })
        .collect()
}

/// Put the sections named in `ids` (comma-separated) into `fit`'s mounts in
/// order, each slot filled with the biggest module of its kind that fits.
fn refit_sections(fit: &mut retrox_sim::ship::Loadout, ids: &str) {
    use retrox_content::ships::{MODULES, section};
    for (mount, id) in ids.split(',').enumerate() {
        let Some(s) = section(id.trim()) else {
            eprintln!("--sections: no section {id}");
            continue;
        };
        if let Err(e) = fit.set_section(mount, s) {
            eprintln!("--sections: {id} won't go in mount {mount}: {e:?}");
            continue;
        }
        for (i, slot) in s.slots.iter().enumerate() {
            let best = MODULES
                .iter()
                .filter(|m| m.kind() == slot.kind && m.size <= slot.size)
                .max_by_key(|m| m.size);
            if let Some(m) = best {
                let _ = fit.fit(mount, i, m);
            }
        }
    }
}

/// A key by the name a key script uses for it (see [`Running::run_script`]).
fn key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    const FS: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return match c {
            'a'..='z' => Some(LETTERS[c as usize - 'a' as usize]),
            '0'..='9' => Some(DIGITS[c as usize - '0' as usize]),
            _ => None,
        };
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()) {
        return FS.get(n.checked_sub(1)?).copied();
    }
    Some(match name {
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "enter" => Enter,
        "esc" => Escape,
        "tab" => Tab,
        "space" => Space,
        "shift" => ShiftLeft,
        "ctrl" => ControlLeft,
        "backspace" => Backspace,
        _ => return None,
    })
}

/// Where things stand, for a `--remote` player who can't see numbers in a
/// screenshot: the target's bearing from the nose, closing speed and so on.
fn remote_info(w: &World) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let ship = w.ship_position();
    let _ = writeln!(out, "system {}", w.galaxy.systems[w.system].name);
    let _ = writeln!(out, "mode {:?}", w.mode);
    let _ = writeln!(out, "credits {:.0}", w.player.credits);
    let _ = writeln!(out, "cargo_t {:.1}", w.player.cargo_t());
    let _ = writeln!(out, "speed {:.0}", w.ship.vel.length());
    let _ = writeln!(out, "hostiles_near {}", w.hostiles_near());
    let _ = writeln!(out, "lost {:?}", w.combat.lost);
    if let Some(t) = w.target {
        let to = w.position(t) - ship;
        let local = w.ship.rot.inverse() * to.normalize_or_zero();
        // Ship space: x right, y up, -z forward.
        let yaw = local.x.atan2(-local.z).to_degrees();
        let pitch = local.y.asin().to_degrees();
        let closing = w.ship.vel.dot(to.normalize_or_zero());
        let _ = writeln!(out, "target {}", w.name(t));
        let _ = writeln!(out, "distance_m {:.0}", to.length());
        let _ = writeln!(
            out,
            "bearing yaw {yaw:.1} pitch {pitch:.1} (deg; +yaw right, +pitch up)"
        );
        let _ = writeln!(out, "closing_mps {closing:.0}");
    }
    let bearing = |to: glam::DVec3| {
        let local = w.ship.rot.inverse() * to.normalize_or_zero();
        (
            local.x.atan2(-local.z).to_degrees(),
            local.y.asin().to_degrees(),
        )
    };
    let me = w.combat_position();
    let _ = writeln!(
        out,
        "player shield {:.0} hull_damage {:.0} heat {:.1} locked {}",
        w.combat.player.shield,
        w.combat.player.damage.total(),
        w.combat.player.heat,
        w.combat.player.locked
    );
    for r in &w.combat.raiders {
        let to = r.craft.pos - me;
        let (yaw, pitch) = bearing(to);
        // Where the lead pip sits, the bearing to put the nose on.
        let (lead_yaw, lead_pitch) = w
            .combat
            .lead(&w.player_craft(), r.id)
            .map_or((yaw, pitch), |lead| bearing(lead - me));
        let _ = writeln!(
            out,
            "raider {} dist {:.0} yaw {yaw:.1} pitch {pitch:.1} lead_yaw {lead_yaw:.1} lead_pitch {lead_pitch:.1} shield {:.0} damage {:.0} targeted {}",
            r.id,
            to.length(),
            r.vessel.shield,
            r.vessel.damage.total(),
            w.combat.target == Some(r.id)
        );
    }
    for (id, p) in &w.galaxy.pirates.prowls {
        let Some(gang) = w.galaxy.pirates.gangs.get(*id) else {
            continue;
        };
        if gang.lair != w.system {
            continue;
        }
        let at = w.traffic.pirate(*id).map(|c| (c.pos - ship).length());
        let _ = writeln!(
            out,
            "gang {} aim {:?} since {:?} rest_until {:?} dist {:?}",
            gang.name, p.aim, p.since, p.rest_until, at
        );
    }
    if let Some((field, rocks)) = &w.field_rocks {
        let ship = w.ship.rel;
        let mut near: Vec<_> = w
            .mining
            .live_rocks(*field, &w.galaxy.fields[*field], rocks, w.clock_s)
            .filter(|r| !r.worked_out)
            .map(|r| ((r.pos - ship).length(), r))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (d, r) in near.iter().take(6) {
            let (yaw, pitch) = bearing(r.pos - ship);
            let _ = writeln!(
                out,
                "rock {:?} dist {d:.0} r {:.0} scanned {} grade {:.2} veins {} damage {:.2} yaw {yaw:.1} pitch {pitch:.1}",
                r.id,
                r.radius,
                r.scanned,
                r.lode.grade,
                r.lode.veins().len(),
                r.damage
            );
        }
        // The nearest quarry, which may be well past the six nearest rocks.
        if let Some((d, r)) = near.iter().find(|(_, r)| r.radius > mining::QUARRY_RADIUS) {
            let (yaw, pitch) = bearing(r.pos - ship);
            let _ = writeln!(
                out,
                "quarry {:?} dist {d:.0} r {:.0} yaw {yaw:.1} pitch {pitch:.1}",
                r.id, r.radius
            );
        }
        if let Some(state) = w.mining.field(*field) {
            let _ = writeln!(out, "nuggets {}", state.nuggets.len());
        }
    }
    out
}
