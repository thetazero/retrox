//! `retrox-client` — the game. For now: fly your ship around a
//! procedurally generated star system, fast travel between planets, dock,
//! land, refit or buy ships in the hangar, and jump along hyperlanes while
//! the galaxy simulates around you.

mod contracts;
mod contracts_ui;
mod hangar;
mod hangar_ui;
mod hud;
mod hulls;
mod meshes;
mod mining;
mod render;
mod station_ui;
mod trade;
mod traffic;
mod world;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use glam::{DQuat, DVec2, DVec3, Mat4, Quat, Vec3};
use retrox_sim::StarClass;
use retrox_sim::{BodyKind, PlanetClass, SettlementId};
use slotmap::Key;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use meshes::StationKind;
use render::{Instance, MeshId, Renderer, SkyVertex, Surface};
use world::{Action, Controls, Mode, Place, Screen, World};

fn main() {
    let seed = std::env::args()
        .skip_while(|a| a != "--seed")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let undocked = std::env::args().any(|a| a == "--undocked");
    let station = std::env::args().any(|a| a == "--station");
    // `--field`: start in an asteroid field, facing a rock, prospected.
    let field = std::env::args().any(|a| a == "--field");
    // `--traffic`: start in space alongside a trader fleet.
    let traffic = std::env::args().any(|a| a == "--traffic");
    // `--fire`: hold the mining laser's trigger down (for screenshots).
    let fire = std::env::args().any(|a| a == "--fire");
    // `--map`: start with the galaxy map open.
    let map = std::env::args().any(|a| a == "--map");
    // `--ship <hull>`: start in that stock ship instead (for testing).
    let ship = std::env::args().skip_while(|a| a != "--ship").nth(1);
    // `--screenshot <path.ppm>`: save the 90th frame and quit (for checking
    // visual changes without a person at the window).
    let screenshot = std::env::args()
        .skip_while(|a| a != "--screenshot")
        .nth(1)
        .map(std::path::PathBuf::from);
    // `--ship-view <yaw degrees>`: look at your ship from that angle, close
    // up (0 = from behind, 90 = from the right). For inspecting the model.
    let ship_view = std::env::args()
        .skip_while(|a| a != "--ship-view")
        .nth(1)
        .and_then(|s| s.parse::<f64>().ok());
    // `--contracts`: start on the dispatch board; `--contract`: also take
    // the first contract on it (for checking the contract screens).
    let take_contract = std::env::args().any(|a| a == "--contract");
    let contracts = take_contract || std::env::args().any(|a| a == "--contracts");
    let mut app = App {
        seed,
        contracts,
        take_contract,
        undocked,
        station,
        field,
        traffic,
        fire,
        map,
        ship,
        screenshot,
        ship_view,
        running: None,
    };
    event_loop.run_app(&mut app).expect("run");
}

struct App {
    seed: u64,
    /// Start on the dispatch board.
    contracts: bool,
    /// Start with the first contract on the board taken.
    take_contract: bool,
    /// Start in space rather than docked (handy for testing).
    undocked: bool,
    /// Start docked at a station, in the first system that has one.
    station: bool,
    /// Start in an asteroid field.
    field: bool,
    /// Start in space alongside a trader fleet.
    traffic: bool,
    /// Hold the mining laser's trigger down.
    fire: bool,
    /// Start with the galaxy map open.
    map: bool,
    ship: Option<String>,
    screenshot: Option<std::path::PathBuf>,
    ship_view: Option<f64>,
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
    system_generation: u64,
}

struct Running {
    renderer: Renderer,
    world: World,
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("retrox")
                        .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 960.0)),
                )
                .expect("create window"),
        );
        let mut renderer = Renderer::new(window.clone());
        let mut world = World::new(self.seed);
        if self.station {
            world.start_at_station();
        }
        if let Some(fit) = self.ship.as_deref().and_then(retrox_content::ships::stock) {
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
        if self.traffic {
            world.start_by_traffic();
        }

        world.map_open = self.map;

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
        renderer.set_sky(&sky(&world));

        let egui_ctx = egui::Context::default();
        hud::style(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        self.running = Some(Running {
            renderer,
            world,
            meshes,
            egui_ctx,
            egui_state,
            keys: HashSet::new(),
            just_pressed: Vec::new(),
            trigger: false,
            fire: self.fire,
            stick: DVec2::ZERO,
            captured: false,
            last_frame: Instant::now(),
            start: Instant::now(),
            frames: 0,
            ship_view: self.ship_view,
        });
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
                ElementState::Pressed
                    if !consumed && r.world.mode == Mode::Normal && !r.world.map_open =>
                {
                    r.set_captured(true);
                }
                ElementState::Released => r.trigger = false,
                ElementState::Pressed => {}
            },
            WindowEvent::Focused(false) => r.set_captured(false),
            WindowEvent::RedrawRequested => {
                const SCREENSHOT_FRAME: u32 = 90;
                r.frames += 1;
                if r.frames == SCREENSHOT_FRAME {
                    r.renderer.capture = self.screenshot.clone();
                }
                r.frame();
                if r.frames == SCREENSHOT_FRAME && self.screenshot.is_some() {
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
            r.stick += DVec2::new(delta.0, delta.1) * 0.004;
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

    fn controls(&mut self, dt: f64) -> (Controls, Vec<Action>) {
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
        for key in std::mem::take(&mut self.just_pressed) {
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
                    self.world.ship.throttle = 0.0;
                    None
                }
                KeyCode::Tab => {
                    self.set_captured(!self.captured);
                    None
                }
                KeyCode::Escape if self.world.map_open => Some(Action::ToggleMap),
                KeyCode::Escape => {
                    self.set_captured(false);
                    // Back out of a docked screen to the menu.
                    (self.world.docked_at().is_some() && self.world.screen != Screen::Hub)
                        .then_some(Action::Open(Screen::Hub))
                }
                _ => None,
            };
            actions.extend(action);
        }
        (controls, actions)
    }

    /// The ship to show in the hangar bay, if a bay screen is open: yours
    /// in the hangar and trading, the one being looked at in the shipyard.
    fn bay_fit(&self) -> Option<retrox_sim::ship::Loadout> {
        self.world.docked_at()?;
        match self.world.screen {
            // Trading happens in the bay too, with your ship being loaded.
            Screen::Hangar | Screen::Trade => Some(self.world.player.ship.fit.clone()),
            Screen::Shipyard => Some(
                self.world
                    .inspect
                    .and_then(|h| retrox_content::ships::stock(h.id))
                    .unwrap_or_else(|| self.world.player.ship.fit.clone()),
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

        let (controls, actions) = self.controls(dt);
        for a in actions {
            self.world.act(a);
        }
        self.world.update(dt, controls);
        if (self.world.mode != Mode::Normal || self.world.map_open) && self.captured {
            self.set_captured(false);
        }

        let fit = self.world.player.ship.fit.clone();
        (self.meshes.ship, self.meshes.ship_shape) = self.ship_mesh(&fit);
        if self.meshes.system_generation != self.world.system_generation {
            self.meshes.stations = self
                .world
                .view
                .stations
                .iter()
                .map(|&s| {
                    let mesh = meshes::station(station_kind(&self.world, s), s.data().as_ffi());
                    self.renderer.add_mesh(&mesh)
                })
                .collect();
            self.meshes.system_generation = self.world.system_generation;
            self.renderer.set_sky(&sky(&self.world));
        }

        let (mut cam_pos, mut cam_rot) = self.world.camera();
        if let Some(yaw) = self.ship_view {
            let rot = self.world.ship.rot;
            let dir =
                DQuat::from_rotation_y(yaw.to_radians()) * DVec3::new(0.0, 0.4, 1.0).normalize();
            let offset = rot * dir * 38.0;
            cam_pos = self.world.ship_position() + offset;
            cam_rot = rot * world::look_rotation(-dir);
        }
        let (w, h) = self.renderer.size();
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            70f32.to_radians(),
            w as f32 / h.max(1) as f32,
            0.5,
        );
        let (view_proj, instances) = match self.bay_fit() {
            // In the hangar or shipyard: a ship on the bay floor.
            Some(fit) => {
                let (mesh, _) = self.ship_mesh(&fit);
                let time = self.start.elapsed().as_secs_f32();
                let (view, instances) = bay_scene(&self.meshes, mesh, &fit, time);
                (proj * view, instances)
            }
            None => {
                let view = Mat4::from_quat(cam_rot.as_quat().inverse());
                let fits: Vec<_> = self
                    .world
                    .traffic
                    .contacts
                    .iter()
                    .map(|c| c.fit.clone())
                    .collect();
                let traffic: Vec<_> = fits.iter().map(|fit| self.ship_mesh(fit)).collect();
                (
                    proj * view,
                    scene(&self.world, &self.meshes, &traffic, cam_pos),
                )
            }
        };

        let raw_input = self.egui_state.take_egui_input(&self.renderer.window);
        let mut ui_actions = Vec::new();
        let output = self.egui_ctx.run_ui(raw_input, |ui| {
            ui_actions = hud::draw(
                ui.ctx(),
                hud::HudInput {
                    world: &self.world,
                    view_proj,
                    camera: cam_pos,
                    stick: self.stick,
                    captured: self.captured,
                },
            );
        });
        self.egui_state
            .handle_platform_output(&self.renderer.window, output.platform_output.clone());
        for a in ui_actions {
            self.world.act(a);
        }

        let star = star_color(self.world.star());
        let ctx = self.egui_ctx.clone();
        self.renderer.render(render::Frame {
            view_proj,
            star_color: star,
            time: self.start.elapsed().as_secs_f32(),
            exposure: 1.0,
            instances: &instances,
            ui: Some((&ctx, output)),
        });
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

    traffic_scene(world, meshes, traffic, cam, &mut out);

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
        if (c.pos - cam).length() > TRAFFIC_DRAW_M {
            continue;
        }
        let burn = (c.thrust.length() / c.accel.max(1e-9)).clamp(0.0, 1.0) as f32;
        let heading = if burn > 0.02 { c.thrust } else { c.vel };
        let rot = world::look_rotation(heading);
        let seed = c.key.0.data().as_ffi() ^ u64::from(c.key.1);
        let length = c.fit.stats().length_m;
        for offset in traffic::formation(c.ships, length, seed) {
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
