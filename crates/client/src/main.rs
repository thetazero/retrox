//! `retrox-client` — the game. For now: fly a placeholder ship around a
//! procedurally generated star system, supercruise between planets, dock,
//! land, and jump along hyperlanes while the galaxy simulates around you.

mod hud;
mod meshes;
mod render;
mod world;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use glam::{DQuat, DVec2, DVec3, Mat4, Quat, Vec3};
use retrox_sim::StarClass;
use retrox_sim::layout::{BodyKind, Place, StationKind};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use render::{Instance, MeshId, Renderer, SkyVertex, Surface};
use world::{Action, Controls, Mode, World};

fn main() {
    let seed = std::env::args()
        .skip_while(|a| a != "--seed")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let undocked = std::env::args().any(|a| a == "--undocked");
    let mut app = App {
        seed,
        undocked,
        running: None,
    };
    event_loop.run_app(&mut app).expect("run");
}

struct App {
    seed: u64,
    /// Start in space rather than docked (handy for testing).
    undocked: bool,
    running: Option<Running>,
}

struct Meshes {
    sphere: MeshId,
    ship: MeshId,
    gate: MeshId,
    rocks: Vec<MeshId>,
    stations: Vec<MeshId>,
    layout_generation: u64,
}

struct Running {
    renderer: Renderer,
    world: World,
    meshes: Meshes,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    keys: HashSet<KeyCode>,
    just_pressed: Vec<KeyCode>,
    stick: DVec2,
    captured: bool,
    last_frame: Instant,
    start: Instant,
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
        if self.undocked {
            world.act(Action::Undock);
            world.ship.throttle = 0.6;
            // Look back at the station.
            world.ship.rot *= DQuat::from_rotation_y(2.6);
            world.cam_rot = world.ship.rot;
        }

        let meshes = Meshes {
            sphere: renderer.add_mesh(&meshes::icosphere(5)),
            ship: renderer.add_mesh(&meshes::placeholder_ship()),
            gate: renderer.add_mesh(&meshes::gate()),
            rocks: (0..8)
                .map(|i| renderer.add_mesh(&meshes::rock(i * 7919 + 13)))
                .collect(),
            stations: Vec::new(),
            layout_generation: u64::MAX,
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
            stick: DVec2::ZERO,
            captured: false,
            last_frame: Instant::now(),
            start: Instant::now(),
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
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !consumed
                && !r.captured
                && matches!(r.world.mode, Mode::Normal | Mode::Supercruise) =>
            {
                r.set_captured(true);
            }
            WindowEvent::Focused(false) => r.set_captured(false),
            WindowEvent::RedrawRequested => r.frame(),
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
        };

        let mut actions = Vec::new();
        for key in std::mem::take(&mut self.just_pressed) {
            let action = match key {
                KeyCode::KeyJ => Some(Action::ToggleSupercruise),
                KeyCode::KeyF => Some(Action::Dock),
                KeyCode::KeyL => Some(Action::Land),
                KeyCode::KeyH => Some(Action::Jump),
                KeyCode::KeyT => Some(Action::CycleTarget),
                KeyCode::KeyV => Some(Action::ToggleCamera),
                KeyCode::KeyZ => Some(Action::ToggleFlightAssist),
                KeyCode::KeyX => {
                    self.world.ship.throttle = 0.0;
                    None
                }
                KeyCode::Tab => {
                    self.set_captured(!self.captured);
                    None
                }
                KeyCode::Escape => {
                    self.set_captured(false);
                    None
                }
                _ => None,
            };
            actions.extend(action);
        }
        (controls, actions)
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
        if !matches!(self.world.mode, Mode::Normal | Mode::Supercruise) && self.captured {
            self.set_captured(false);
        }

        if self.meshes.layout_generation != self.world.layout_generation {
            self.meshes.stations = self
                .world
                .layout
                .stations
                .iter()
                .map(|s| self.renderer.add_mesh(&meshes::station(s.kind, s.seed)))
                .collect();
            self.meshes.layout_generation = self.world.layout_generation;
            self.renderer.set_sky(&sky(&self.world));
        }

        let (cam_pos, cam_rot) = self.world.camera();
        let (w, h) = self.renderer.size();
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            70f32.to_radians(),
            w as f32 / h.max(1) as f32,
            0.5,
        );
        let view = Mat4::from_quat(cam_rot.as_quat().inverse());
        let view_proj = proj * view;
        let instances = scene(&self.world, &self.meshes, cam_pos);

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

        let star = self.world.layout.star.color;
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
fn scene(world: &World, meshes: &Meshes, cam: DVec3) -> Vec<(MeshId, Instance)> {
    let t = world.clock_s;
    let layout = &world.layout;
    let mut out = Vec::new();
    let rel = |p: DVec3| (p - cam).as_vec3();
    let light = |p: DVec3| {
        let dir = (-p).normalize_or(DVec3::Y).as_vec3();
        [dir.x, dir.y, dir.z, 1.3]
    };

    // The star, never smaller than a small disc so it reads from anywhere.
    let star = &layout.star;
    let star_dist = cam.length();
    let star_r = star.radius_m.max(star_dist * 0.0035);
    let star_color = if star.class == StarClass::BlackHole {
        [0.02, 0.0, 0.03]
    } else {
        star.color
    };
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale_rotation_translation(
                Vec3::splat(star_r as f32),
                Quat::IDENTITY,
                rel(DVec3::ZERO),
            ),
            star_color,
            Surface::Star,
            6.0,
            0.0,
            [0.0, 1.0, 0.0, 1.0],
        ),
    ));

    for (i, body) in layout.bodies.iter().enumerate() {
        let pos = layout.position(Place::Body(i), t);
        let (surface, color) = match body.kind {
            BodyKind::Lava => (Surface::Lava, [0.3, 0.2, 0.15]),
            BodyKind::Rocky => (Surface::Rocky, [0.45, 0.42, 0.38]),
            BodyKind::Desert => (Surface::Desert, [0.72, 0.52, 0.32]),
            BodyKind::Ocean => (Surface::Ocean, [0.2, 0.35, 0.5]),
            BodyKind::Ice => (Surface::Ice, [0.65, 0.75, 0.82]),
            BodyKind::GasGiant => (Surface::Gas, gas_color(body.seed)),
            BodyKind::Barren => (Surface::Barren, [0.4, 0.38, 0.36]),
        };
        let spin = Quat::from_rotation_y(
            (t / 86_400.0 * std::f64::consts::TAU % std::f64::consts::TAU) as f32,
        );
        out.push((
            meshes.sphere,
            Instance::new(
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(body.radius_m as f32),
                    spin,
                    rel(pos),
                ),
                color,
                surface,
                0.0,
                (body.seed % 10_000) as f32,
                light(pos),
            ),
        ));
    }

    for (i, station) in layout.stations.iter().enumerate() {
        let pos = layout.position(Place::Station(i), t);
        let spin = if station.kind == StationKind::Habitat {
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
                    Vec3::splat(station.size_m as f32 * 0.5),
                    Quat::from_rotation_y(spin) * Quat::from_rotation_x(0.3),
                    rel(pos),
                ),
                [0.72, 0.7, 0.64],
                Surface::Plain,
                0.0,
                0.0,
                light(pos),
            ),
        ));
    }

    for gate in &layout.gates {
        let rot = world::look_rotation(-gate.position).as_quat();
        out.push((
            meshes.gate,
            Instance::new(
                Mat4::from_scale_rotation_translation(Vec3::splat(1000.0), rot, rel(gate.position)),
                [0.75, 0.55, 0.35],
                Surface::Plain,
                0.05,
                0.0,
                light(gate.position),
            ),
        ));
    }

    if let Some((field, rocks)) = &world.field_rocks {
        let center = layout.position(Place::Field(*field), t);
        let icy = layout.fields[*field].kind == retrox_sim::layout::BeltKind::Icy;
        let color = if icy {
            [0.6, 0.66, 0.72]
        } else {
            [0.42, 0.37, 0.33]
        };
        for rock in rocks {
            let pos = center + rock.offset;
            let spin =
                DQuat::from_axis_angle(rock.spin_axis, rock.spin_rate * t % std::f64::consts::TAU)
                    .as_quat();
            out.push((
                meshes.rocks[(rock.seed % meshes.rocks.len() as u64) as usize],
                Instance::new(
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(rock.radius_m as f32),
                        spin,
                        rel(pos),
                    ),
                    color,
                    Surface::Plain,
                    0.0,
                    0.0,
                    light(pos),
                ),
            ));
        }
    }

    let hidden = world.cockpit || !matches!(world.mode, Mode::Normal | Mode::Supercruise);
    if !hidden {
        let pos = world.ship_position();
        out.push((
            meshes.ship,
            Instance::new(
                Mat4::from_rotation_translation(world.ship.rot.as_quat(), rel(pos)),
                [0.78, 0.74, 0.66],
                Surface::Plain,
                0.0,
                0.0,
                light(pos),
            ),
        ));
    }
    out
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

/// The rest of the galaxy as background stars, in their true directions.
fn sky(world: &World) -> Vec<SkyVertex> {
    let here = world.galaxy.systems[world.system].position;
    world
        .galaxy
        .systems
        .iter()
        .filter(|(id, _)| *id != world.system)
        .map(|(_, s)| {
            let d = DVec3::new(
                s.position.x - here.x,
                s.position.z - here.z,
                s.position.y - here.y,
            );
            let ly = d.length();
            let brightness = (120.0 / ly).clamp(0.15, 6.0) as f32;
            let c = match s.star {
                StarClass::O | StarClass::B => [0.7, 0.8, 1.0],
                StarClass::A | StarClass::F | StarClass::WhiteDwarf => [1.0, 1.0, 1.0],
                StarClass::G => [1.0, 0.92, 0.75],
                StarClass::K => [1.0, 0.78, 0.55],
                StarClass::M => [1.0, 0.6, 0.45],
                StarClass::Neutron => [0.6, 0.9, 1.0],
                StarClass::BlackHole => [0.3, 0.2, 0.4],
            };
            SkyVertex {
                dir: (d / ly).as_vec3().into(),
                color: [c[0] * brightness, c[1] * brightness, c[2] * brightness],
            }
        })
        .collect()
}
