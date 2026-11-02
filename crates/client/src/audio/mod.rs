//! Sound and music (DESIGN §8 Audio).
//!
//! Everything is synthesized: a generative analog-synth score
//! ([`score`]), live loops for the engines, laser, drives and ambience,
//! and one-shot cues rendered at startup ([`sfx`]), mixed on the audio
//! thread by [`mixer`] and played through kira. The game side is
//! [`Audio`]: it watches the world each frame and turns what changed into
//! cues and loop levels, so game code doesn't have to call it.
//!
//! No audio device (CI, a headless box) means silence, never an error.

mod dsp;
mod mixer;
mod score;
mod sfx;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use glam::DVec3;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend};

use crate::settings::Settings;
use crate::world::{Controls, Mode, Place, Screen, World};
pub use mixer::Param;
use mixer::{Play, Shared, Synth, SynthData};
pub use score::Mood;
pub use sfx::Cue;

pub struct Audio {
    shared: Arc<Shared>,
    tx: Sender<Play>,
    /// The cue library rendering, and the receiver waiting for the synth.
    starting: Option<(JoinHandle<sfx::Library>, mpsc::Receiver<Play>)>,
    manager: Option<AudioManager>,
    /// `--mute`, or no device.
    muted: bool,
    watch: Option<Snapshot>,
    rng: dsp::Rng,
    /// Seconds to the next distant station clank, or hull creak in transit.
    ambient_in: f32,
    /// Seconds since the last cue of each rate-limited kind.
    since_comms: f32,
    since_collect: f32,
}

/// What the director needs to know about the frame.
pub struct View<'a> {
    pub world: Option<&'a World>,
    pub paused: bool,
    pub controls: Option<&'a Controls>,
}

impl Audio {
    /// Start rendering the cues in the background; sound starts when
    /// they're ready (a fraction of a second).
    pub fn new(settings: &Settings, mute: bool) -> Self {
        let shared = Arc::new(Shared::new());
        let (tx, rx) = mpsc::channel();
        let starting = (!mute).then(|| (std::thread::spawn(sfx::Library::render), rx));
        let audio = Self {
            shared,
            tx,
            starting,
            manager: None,
            muted: mute,
            watch: None,
            rng: dsp::Rng::new(0xA0D10),
            ambient_in: 8.0,
            since_comms: 10.0,
            since_collect: 10.0,
        };
        audio.set_volumes(settings);
        audio
    }

    pub fn set_volumes(&self, s: &Settings) {
        // Perceptual: the slider is closer to loudness than amplitude.
        let curve = |v: f32| v.clamp(0.0, 1.0).powf(1.8);
        self.shared.set(Param::Master, curve(s.master_volume));
        self.shared.set(Param::Music, curve(s.music_volume));
        self.shared.set(Param::Effects, curve(s.effects_volume));
    }

    fn poll_start(&mut self) {
        let ready = self
            .starting
            .as_ref()
            .is_some_and(|(handle, _)| handle.is_finished());
        if !ready {
            return;
        }
        let (handle, rx) = self.starting.take().expect("checked");
        let Ok(lib) = handle.join() else {
            self.muted = true;
            return;
        };
        let synth = Synth::new(Arc::new(lib), self.shared.clone(), rx);
        match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(mut manager) => {
                if manager.play(SynthData(synth)).is_ok() {
                    self.manager = Some(manager);
                } else {
                    self.muted = true;
                }
            }
            Err(e) => {
                eprintln!("no sound: {e}");
                self.muted = true;
            }
        }
    }

    pub fn play(&self, cue: Cue) {
        self.play_with(cue, 1.0, 1.0, 0.0);
    }

    pub fn play_with(&self, cue: Cue, gain: f32, pitch: f32, pan: f32) {
        if !self.muted {
            // Fails only once the synth is gone, at exit.
            let _ = self.tx.send(Play {
                cue,
                gain,
                pitch,
                pan,
            });
        }
    }

    /// Once a frame: start up if ready, and follow the game.
    pub fn update(&mut self, view: View, dt: f32) {
        self.poll_start();
        self.since_comms += dt;
        self.since_collect += dt;
        let set = |p, v: f32| self.shared.set(p, v);

        let Some(world) = view.world else {
            self.watch = None;
            set(Param::Mood, Mood::Title.index() as f32);
            for p in LOOPS {
                set(p, 0.0);
            }
            return;
        };
        let now = Snapshot::of(world);
        let mood = match world.mode {
            Mode::Docked(_) | Mode::Landed { .. } => Mood::Station,
            Mode::FastTravel(_) | Mode::Jumping { .. } => Mood::Transit,
            Mode::Normal => Mood::Space,
        };
        set(Param::Mood, mood.index() as f32);

        if view.paused {
            for p in LOOPS {
                set(p, 0.0);
            }
            // Keep watching so nothing fires on resume.
            self.watch = Some(now);
            return;
        }

        // Loops.
        let flying = matches!(world.mode, Mode::Normal);
        let travelling = matches!(world.mode, Mode::FastTravel(_));
        set(Param::Flight, if flying { 1.0 } else if travelling { 0.5 } else { 0.0 });
        let accel = self.watch.as_ref().map_or(0.0, |w| {
            let dv = (now.vel - w.vel).length() / f64::from(dt.max(1e-3));
            (dv / (now.vel.length() * 0.5).max(90.0)).clamp(0.0, 1.0) as f32
        });
        let throttle = world.ship.throttle.abs() as f32;
        set(
            Param::Thrust,
            if travelling { 0.8 } else { accel.max(0.3 * throttle) },
        );
        let c = view.controls.copied().unwrap_or_default();
        set(
            Param::Boost,
            if flying && c.boost && world.ship.throttle <= 0.5 { 1.0 } else { 0.0 },
        );
        let rcs = c.stick.abs().max_element().max(c.strafe.abs()).max(c.lift.abs());
        set(Param::Rcs, if flying { rcs as f32 } else { 0.0 });
        set(Param::Cockpit, if world.cockpit { 1.0 } else { 0.0 });
        let mining = &world.mining;
        set(Param::Laser, if mining.firing { 1.0 } else { 0.0 });
        let range = world.stats().mining_range_m;
        let contact = mining.firing && mining.aim.is_some_and(|a| a.distance <= range);
        set(Param::Contact, if contact { 1.0 } else { 0.0 });
        set(Param::Heat, mining.heat as f32);
        match world.mode {
            Mode::FastTravel(trip) => {
                set(Param::Warp, 1.0);
                set(Param::WarpRate, (trip.warp.max(1.0).log10() / 5.0).clamp(0.0, 1.0) as f32);
            }
            _ => set(Param::Warp, 0.0),
        }
        set(
            Param::Jump,
            match world.mode {
                Mode::Jumping { timer, .. } => (1.0 - timer / crate::world::JUMP_DURATION).clamp(0.0, 1.0),
                _ => 0.0,
            },
        );
        let docked = matches!(world.mode, Mode::Docked(_));
        let city = matches!(world.mode, Mode::Landed { timer, .. } if timer <= 0.0);
        set(Param::Station, if docked { 1.0 } else { 0.0 });
        set(Param::City, if city { 1.0 } else { 0.0 });

        // Ambient incidents: a station's clanks, a hull creaking in transit.
        self.ambient_in -= dt;
        if self.ambient_in <= 0.0 {
            if docked {
                let gain = self.rng.range(0.4, 1.0);
                let pan = self.rng.range(-0.8, 0.8);
                self.play_with(Cue::Clank, gain, self.rng.range(0.85, 1.1), pan);
                self.ambient_in = self.rng.range(5.0, 13.0);
            } else if travelling {
                self.play_with(Cue::HullCreak, self.rng.range(0.4, 0.7), self.rng.range(0.9, 1.1), 0.0);
                self.ambient_in = self.rng.range(7.0, 16.0);
            } else {
                self.ambient_in = 3.0;
            }
        }

        if let Some(was) = self.watch.take() {
            self.changes(&was, &now, world);
        }
        self.watch = Some(now);
    }

    /// Cues for what changed since last frame.
    fn changes(&mut self, was: &Snapshot, now: &Snapshot, world: &World) {
        use ModeKind as K;
        match (was.mode, now.mode) {
            (a, b) if a == b => {}
            (K::Landing, K::Landed) => {}
            (_, K::Docked) => self.play(Cue::DockClamp),
            (K::Docked, _) => self.play(Cue::Undock),
            (_, K::Landing) => self.play(Cue::Land),
            (K::Landing | K::Landed, _) => self.play(Cue::Launch),
            (_, K::Travel) => self.play(Cue::TravelEngage),
            (K::Travel, _) => self.play(Cue::TravelDrop),
            (_, K::Jumping) => self.play(Cue::JumpEngage),
            (K::Jumping, _) => self.play(Cue::JumpArrive),
            _ => {}
        }

        if now.map_open != was.map_open {
            self.play(if now.map_open { Cue::MapOpen } else { Cue::MapClose });
        }
        if now.course != was.course && now.course.is_some() {
            self.play(Cue::Relay);
            self.play_with(Cue::Relay, 0.7, 1.1, 0.2);
        }
        if now.cockpit != was.cockpit {
            self.play(Cue::Switch);
        }
        if now.flight_assist != was.flight_assist {
            self.play_with(Cue::Switch, 1.0, if now.flight_assist { 1.0 } else { 0.85 }, 0.0);
        }
        if now.target != was.target && now.target.is_some() {
            self.play(Cue::Tick);
        }
        if now.message.is_some() && now.message != was.message && self.since_comms > 0.4 {
            self.since_comms = 0.0;
            self.play(Cue::Comms);
        }

        // Collisions: a thud and the hull complaining, by impact speed.
        if world.bump > 2.0 {
            let k = ((world.bump - 2.0) / 40.0).clamp(0.0, 1.0) as f32;
            self.play_with(Cue::Bump, 0.35 + 0.65 * k, 1.0 - 0.15 * k, 0.0);
            if k > 0.3 {
                self.play_with(Cue::HullCreak, k, 1.0, 0.0);
            }
        }

        // Mining.
        if now.since_scan < was.since_scan {
            self.play(Cue::Scan);
        }
        if now.locked && !was.locked {
            self.play(Cue::Overheat);
        } else if was.locked && !now.locked {
            self.play(Cue::Cooled);
        }
        for b in &world.mining.bursts {
            let key = burst_key(b.pos);
            if !was.bursts.contains(&key) {
                if b.volatile {
                    self.play(Cue::Blast);
                } else {
                    let big = (b.radius / 40.0).clamp(0.0, 1.0) as f32;
                    self.play_with(Cue::Fracture, 0.6 + 0.4 * big, 1.15 - 0.3 * big, 0.0);
                }
            }
        }
        let docked = matches!(now.mode, K::Docked | K::Landed);
        if !docked && now.cargo > was.cargo + 1e-6 && self.since_collect > 0.09 {
            self.since_collect = 0.0;
            let pitch = self.rng.range(0.9, 1.15);
            self.play_with(Cue::Collect, 1.0, pitch, 0.0);
        }

        // The station: contracts, deliveries, trades.
        if now.contracts > was.contracts {
            self.play(Cue::Stamp);
        } else if now.contracts < was.contracts && now.credits > was.credits {
            self.play(Cue::Delivered);
        } else if now.contracts < was.contracts {
            self.play(Cue::Back);
        } else if (now.credits - was.credits).abs() > 1e-6 && docked {
            self.play(Cue::Trade);
        }
        if now.ship != was.ship || now.stored != was.stored {
            self.play(Cue::Refit);
        }
        if now.screen != was.screen && docked {
            self.play_with(Cue::Relay, 0.6, 0.9, 0.0);
        }
    }

    /// A widget clicked in any interface.
    pub fn ui_click(&self) {
        self.play(Cue::Switch);
    }

    /// The menu cursor moved with the keys.
    pub fn ui_nav(&self) {
        self.play(Cue::Tick);
    }

    /// A save or checkpoint written (or not).
    pub fn saved(&self, ok: bool) {
        self.play(if ok { Cue::Teletype } else { Cue::Overheat });
    }
}

/// Every loop, for silencing.
const LOOPS: [Param; 13] = [
    Param::Flight,
    Param::Thrust,
    Param::Boost,
    Param::Rcs,
    Param::Cockpit,
    Param::Laser,
    Param::Contact,
    Param::Heat,
    Param::Warp,
    Param::WarpRate,
    Param::Jump,
    Param::Station,
    Param::City,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModeKind {
    Normal,
    Travel,
    Docked,
    Landing,
    Landed,
    Jumping,
}

/// The parts of the world that make a sound when they change.
struct Snapshot {
    mode: ModeKind,
    vel: DVec3,
    map_open: bool,
    course: Option<retrox_sim::SystemId>,
    cockpit: bool,
    flight_assist: bool,
    target: Option<Place>,
    screen: Screen,
    message: Option<String>,
    since_scan: f64,
    locked: bool,
    bursts: HashSet<[u64; 3]>,
    cargo: f64,
    credits: f64,
    contracts: usize,
    /// The ship's hull and fit, by name: changes on a refit or a purchase.
    ship: String,
    stored: usize,
}

fn burst_key(p: DVec3) -> [u64; 3] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
}

impl Snapshot {
    fn of(world: &World) -> Self {
        let mode = match world.mode {
            Mode::Normal => ModeKind::Normal,
            Mode::FastTravel(_) => ModeKind::Travel,
            Mode::Docked(_) => ModeKind::Docked,
            Mode::Landed { timer, .. } if timer > 0.0 => ModeKind::Landing,
            Mode::Landed { .. } => ModeKind::Landed,
            Mode::Jumping { .. } => ModeKind::Jumping,
        };
        let player = &world.player;
        Self {
            mode,
            vel: world.ship.vel,
            map_open: world.map_open,
            course: world.course,
            cockpit: world.cockpit,
            flight_assist: world.ship.flight_assist,
            target: world.target,
            screen: world.screen,
            message: world.message.as_ref().map(|(m, _)| m.clone()),
            since_scan: world.mining.since_scan,
            locked: world.mining.locked,
            bursts: world.mining.bursts.iter().map(|b| burst_key(b.pos)).collect(),
            cargo: player.cargo_t(),
            credits: player.credits,
            contracts: player.contracts.len(),
            ship: format!("{} {}", player.ship.name, crate::fit_key(&player.ship.fit)),
            stored: player.stored.len(),
        }
    }
}
