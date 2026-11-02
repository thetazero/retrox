//! `retrox-client` — the game. Launch to the title screen; start a new
//! galaxy or carry on from a save. In play: fly your ship around a
//! procedurally generated star system, fast travel between planets, dock,
//! land, refit or buy ships in the hangar, and jump along hyperlanes while
//! the galaxy simulates around you. Esc pauses.
//!
//! Development flags skip the title and drop straight into a fresh game:
//! `--seed N`, `--station`, `--undocked`, `--map`, `--course <jumps>`,
//! `--ship <hull>`,
//! `--ship-view <deg>`, `--screenshot <path.ppm>`. `--menu <page>` opens a
//! menu page instead (`title`, `new`, `load`, `settings`, or `pause` or `quit`
//! over a fresh game), for screenshots. `--keys` plays a key script
//! (see [`Flags::keys`]).

mod audio;
mod contracts;
mod contracts_ui;
mod hangar;
mod hangar_ui;
mod hud;
mod hulls;
mod menu;
mod meshes;
mod mining;
mod render;
mod save;
mod settings;
mod station_ui;
mod title;
mod trade;
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
use render::{Instance, MeshId, Renderer, SkyVertex, Surface};
use save::{GameId, Kind};
use settings::Settings;
use world::{Action, Controls, Mode, Place, Screen, World};

/// Command-line flags, for development.
#[derive(Default)]
struct Flags {
    seed: Option<u64>,
    /// Start in space rather than docked (handy for testing).
    undocked: bool,
    /// Start docked at a station, in the first system that has one.
    station: bool,
    /// Start in an asteroid field, facing a rock, prospected.
    field: bool,
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
    /// Press these keys, one a frame (`--keys down,enter,wait60,esc`), as
    /// if typed; `waitN` waits N frames (and any wait lasts until a game
    /// being started is ready). Quits when done, unless `--screenshot`
    /// does first. For checking menu flows end to end without a person.
    keys: Vec<String>,
    /// No sound. Screenshot and scripted runs are silent unless `--sound`.
    mute: bool,
}

impl Flags {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| args.iter().skip_while(|a| *a != flag).nth(1).cloned();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        Self {
            seed: value("--seed").and_then(|s| s.parse().ok()),
            undocked: has("--undocked"),
            station: has("--station"),
            field: has("--field"),
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
            keys: value("--keys")
                .map(|k| k.split(',').map(|s| s.trim().to_lowercase()).collect())
                .unwrap_or_default(),
            mute: has("--mute")
                || (!has("--sound") && (has("--screenshot") || has("--keys"))),
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
                    || self.station
                    || self.field
                    || self.contracts
                    || self.map
                    || self.course.is_some()
                    || self.ship.is_some()
                    || self.ship_view.is_some()
                    || self.screenshot.is_some()
            }
        }
    }

    /// A fresh world set up as the flags ask.
    fn world(&self) -> World {
        let mut world = World::new(self.seed.unwrap_or(1));
        if self.station {
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
        if self.field && world.start_at_field() {
            world.act(Action::Scan);
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
    rocks: Vec<MeshId>,
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
    /// A short note in the corner: "checkpoint saved".
    toast: Option<(String, bool, Instant)>,
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
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("retrox")
                        .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 960.0))
                        .with_fullscreen(
                            settings.fullscreen.then_some(Fullscreen::Borderless(None)),
                        ),
                )
                .expect("create window"),
        );
        let mut renderer = Renderer::new(window.clone());
        let meshes = Meshes {
            sphere: renderer.add_mesh(&meshes::icosphere(5)),
            ship: MeshId::default(),
            ship_shape: meshes::ShipShape::default(),
            ship_cache: std::collections::HashMap::new(),
            hangar_bay: renderer.add_mesh(&meshes::hangar_bay()),
            rocks: (0..mining::ROCK_MESHES)
                .map(|i| renderer.add_mesh(&meshes::rock(mining::rock_mesh_seed(i))))
                .collect(),
            stations: Vec::new(),
            system_generation: u64::MAX,
        };

        let egui_ctx = egui::Context::default();
        hud::style(&egui_ctx);
        egui_ctx.set_zoom_factor(settings.ui_scale);
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
            audio: audio::Audio::new(&settings, self.flags.mute),
            quit: false,
            script: self.flags.keys.iter().cloned().collect(),
            script_wait: 0,
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
        } else {
            r.back_to_title();
        }
        let page = match self.flags.menu.as_deref() {
            Some("new") => Some(Page::NewGame),
            Some("load") => Some(Page::Load),
            Some("settings") => Some(Page::Settings),
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
            WindowEvent::Resized(size) => r.renderer.resize(size.width, size.height),
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
            && self
                .game
                .as_ref()
                .is_some_and(|g| g.world.mode == Mode::Normal && !g.world.map_open)
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
        if let Some(p) = &self.pending {
            match p.rx.try_recv() {
                Ok(Ok(started)) => {
                    self.pending = None;
                    self.start_game(started);
                }
                Ok(Err(why)) => {
                    self.pending = None;
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
                self.egui_ctx.set_zoom_factor(self.settings.ui_scale);
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
        let keys_pitch = axis(KeyCode::ArrowDown, KeyCode::ArrowUp);
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
                KeyCode::KeyX => {
                    world.ship.throttle = 0.0;
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
        } else {
            let (controls, actions, pause) = self.controls(dt, keys);
            frame_controls = Some(controls);
            let game = self.game.as_mut().expect("playing");
            for a in actions {
                game.world.act(a);
            }
            game.world.update(dt, controls);
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

        let raw_input = self.egui_state.take_egui_input(&self.renderer.window);
        let mut ui_actions = Vec::new();
        let output = self.egui_ctx.run_ui(raw_input, |ui| {
            let ctx = ui.ctx();
            if let Some(p) = &self.pending {
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
                } else {
                    ui_actions = hud::draw(
                        ctx,
                        hud::HudInput {
                            world: &game.world,
                            view_proj,
                            camera: cam_pos,
                            stick: self.stick,
                            captured: self.captured,
                        },
                    );
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
            ui: Some((&ctx, output)),
        });
    }

    /// Press the next scripted key, if it's time.
    fn run_script(&mut self) {
        if self.pending.is_some() || !self.writers.is_empty() {
            return;
        }
        if self.script_wait > 0 {
            self.script_wait -= 1;
            return;
        }
        let Some(step) = self.script.pop_front() else {
            return;
        };
        let key = match step.as_str() {
            "up" => KeyCode::ArrowUp,
            "down" => KeyCode::ArrowDown,
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Escape,
            "tab" => KeyCode::Tab,
            other => {
                if let Some(n) = other.strip_prefix("wait") {
                    self.script_wait = n.parse().unwrap_or(30);
                } else if let Some(c) = other.chars().next().filter(|_| other.len() == 1) {
                    let code = match c.to_ascii_lowercase() {
                        'f' => Some(KeyCode::KeyF),
                        'l' => Some(KeyCode::KeyL),
                        'j' => Some(KeyCode::KeyJ),
                        't' => Some(KeyCode::KeyT),
                        'm' => Some(KeyCode::KeyM),
                        _ => None,
                    };
                    self.just_pressed.extend(code);
                }
                // The step after a key waits a frame for it to land.
                self.script_wait = self.script_wait.max(2);
                return;
            }
        };
        self.just_pressed.push(key);
        self.script_wait = 2;
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
                let world = &self.game.as_ref().expect("playing").world;
                let view = Mat4::from_quat(cam_rot.as_quat().inverse());
                (
                    proj * view,
                    scene(world, &self.meshes, cam_pos),
                    cam_pos,
                    star,
                )
            }
        }
    }
}

/// Everything visible, positioned relative to the camera.
fn scene(world: &World, meshes: &Meshes, cam: DVec3) -> Vec<(MeshId, Instance)> {
    let t = world.clock_s;
    let mut out = Vec::new();
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };

    // The star, never smaller than a small disc so it reads from anywhere.
    let star_r = world.star_radius_m().max(cam.length() * 0.0035);
    let star_rgb = if world.star() == StarClass::BlackHole {
        [0.02, 0.0, 0.03]
    } else {
        star_color(world.star())
    };
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
            6.0,
            0.0,
            [0.0, 1.0, 0.0, 1.0],
        ),
    ));

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

    let hidden = world.cockpit || !matches!(world.mode, Mode::Normal | Mode::FastTravel(_));
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
            .with_detail(0.5),
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
        out.push((
            meshes.rocks[mining::rock_mesh(rock.seed)],
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(rock.radius as f32),
                    rock.rot.as_quat(),
                    rel(pos),
                ),
                color,
                Surface::Plain,
                // Glowing from the laser's work.
                (rock.damage as f32).powi(2) * 0.25,
                0.0,
                lit,
            ),
        ));
        // Veins, once read, on rocks near enough to matter.
        if rock.scanned && (pos - cam).length() < mining::SCAN_RANGE + rock.radius {
            let (glow, strength) = if rock.lode.volatile {
                ([1.0, 0.22, 0.12], 2.0 + 3.0 * pulse)
            } else {
                (ore_glow(rock.lode.good), 2.5)
            };
            let size = (rock.radius * 0.16).max(0.7) as f32;
            for v in rock.vein_points() {
                out.push((
                    meshes.sphere,
                    Instance::new(
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(size),
                            Quat::IDENTITY,
                            rel(center + v),
                        ),
                        glow,
                        Surface::Star,
                        strength,
                        0.0,
                        [0.0, 1.0, 0.0, 1.0],
                    ),
                ));
            }
        }
    }
    if let Some(state) = world.mining.field(field) {
        for n in &state.nuggets {
            let size = (0.5 + 1.1 * n.tonnes.cbrt()) as f32;
            let spin = Quat::from_rotation_y((t * 0.7 + n.seed as f64 % 6.0) as f32)
                * Quat::from_rotation_x((t * 0.4) as f32);
            out.push((
                meshes.rocks[mining::rock_mesh(n.seed)],
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
                Surface::Star,
                5.0 * (1.0 - k).powi(2),
                0.0,
                [0.0, 1.0, 0.0, 1.0],
            ),
        ));
    }
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
