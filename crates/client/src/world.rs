//! The player's world: the galaxy sim, the current system's layout, the
//! galaxy clock, and the ship.
//!
//! **Frames.** Real-scale orbits move fast (a station in low orbit does
//! ~7 km/s around its planet), so the ship's position is stored relative to
//! a *frame*: the place it's flying near. The frame moves with that place,
//! so nearby things hold still while you fly. Supercruise uses the star's
//! frame; dropping out picks the nearest place as the new frame.
//!
//! **Time** (DESIGN §3). The galaxy clock runs 1:1 in normal flight and at
//! one hour per second in supercruise; jumps take days; docked waiting is
//! explicit. The sim ticks whenever the clock crosses a day.

use glam::{DQuat, DVec3};
use retrox_gen::{GalaxyConfig, generate, rocks, system_layout};
use retrox_sim::layout::{Place, Rock, SECONDS_PER_DAY, StationKind, SystemLayout};
use retrox_sim::{GalaxyState, GalaxyTime, SystemId, tick};

/// Normal-flight top speed, m/s.
pub const MAX_SPEED: f64 = 320.0;
const BOOST: f64 = 2.2;
const ACCEL: f64 = 90.0;
const STRAFE_SPEED: f64 = 80.0;
/// Pitch, yaw, roll rates at full stick, rad/s.
const TURN_RATES: DVec3 = DVec3::new(1.3, 0.9, 2.2);
/// Galaxy seconds per real second in supercruise.
const SUPERCRUISE_TIME_RATE: f64 = 3600.0;
const JUMP_DURATION: f32 = 4.0;
const LANDING_DURATION: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Normal,
    Supercruise,
    Docked(usize),
    /// Landing animation, then the location screen.
    Landed {
        body: usize,
        point: usize,
        timer: f32,
    },
    Jumping {
        gate: usize,
        timer: f32,
    },
}

#[derive(Clone, Debug)]
pub struct Ship {
    pub frame: Place,
    /// Position relative to the frame, metres.
    pub rel: DVec3,
    /// Velocity relative to the frame, m/s.
    pub vel: DVec3,
    pub rot: DQuat,
    /// Local angular velocity (pitch, yaw, roll), rad/s.
    pub spin: DVec3,
    /// -0.3..1, fraction of top speed (normal) or of supercruise limit.
    pub throttle: f64,
    pub flight_assist: bool,
    /// Current supercruise speed, m/s.
    pub cruise_speed: f64,
}

/// Player input for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// Virtual stick: pitch (+ nose up), yaw (+ right), roll (+ right), each -1..1.
    pub stick: DVec3,
    pub throttle_delta: f64,
    pub strafe: f64,
    pub lift: f64,
    pub boost: bool,
}

/// One-shot actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleSupercruise,
    Dock,
    Undock,
    Land,
    Launch,
    Jump,
    CycleTarget,
    Target(Place),
    ToggleCamera,
    ToggleFlightAssist,
    Wait(u64),
}

/// What the player could do right now (shown as prompts).
#[derive(Clone, Debug, Default)]
pub struct Prompts {
    pub dock: Option<usize>,
    pub land: Option<(usize, usize)>,
    pub jump: Option<usize>,
    pub drop: bool,
}

pub struct World {
    pub galaxy: GalaxyState,
    pub system: SystemId,
    pub layout: SystemLayout,
    /// Galaxy seconds since the start of history (finer than the sim's days).
    pub clock_s: f64,
    pub ship: Ship,
    pub mode: Mode,
    pub target: Option<Place>,
    pub cockpit: bool,
    pub cam_rot: DQuat,
    /// Rocks of the asteroid field the ship is in, if any.
    pub field_rocks: Option<(usize, Vec<Rock>)>,
    pub message: Option<(String, f32)>,
    /// Bumped whenever the layout changes, so the client re-uploads meshes.
    pub layout_generation: u64,
}

impl World {
    pub fn new(seed: u64) -> Self {
        let galaxy = generate(seed, &GalaxyConfig::default());
        // Start docked at a habitat in the most populous faction capital.
        let system = galaxy
            .factions
            .values()
            .map(|f| f.capital)
            .max_by(|a, b| {
                let pa = galaxy.systems[*a].economy.population;
                let pb = galaxy.systems[*b].economy.population;
                pa.total_cmp(&pb)
            })
            .unwrap_or(SystemId::from_index(0));
        let layout = system_layout(&galaxy, system);
        let station = layout
            .stations
            .iter()
            .position(|s| s.kind == StationKind::Habitat)
            .unwrap_or(0);
        let clock_s = galaxy.time.days() as f64 * SECONDS_PER_DAY;
        let mut world = Self {
            galaxy,
            system,
            layout,
            clock_s,
            ship: Ship {
                frame: Place::Station(station),
                rel: DVec3::ZERO,
                vel: DVec3::ZERO,
                rot: DQuat::IDENTITY,
                spin: DVec3::ZERO,
                throttle: 0.0,
                flight_assist: true,
                cruise_speed: 0.0,
            },
            mode: Mode::Docked(station),
            target: None,
            cockpit: false,
            cam_rot: DQuat::IDENTITY,
            field_rocks: None,
            message: None,
            layout_generation: 0,
        };
        if world.layout.stations.is_empty() {
            world.mode = Mode::Normal;
            world.ship.frame = Place::Star;
            world.ship.rel = DVec3::new(0.0, 0.0, world.layout.extent_m() * 0.5);
        }
        world
    }

    pub fn ship_position(&self) -> DVec3 {
        self.layout.position(self.ship.frame, self.clock_s) + self.ship.rel
    }

    pub fn date(&self) -> GalaxyTime {
        GalaxyTime::from_days((self.clock_s / SECONDS_PER_DAY) as u64)
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), 4.0));
    }

    /// Advance the galaxy clock, ticking the sim for every day crossed.
    pub fn advance_clock(&mut self, seconds: f64) {
        self.clock_s += seconds;
        let target_day = (self.clock_s / SECONDS_PER_DAY) as u64;
        while self.galaxy.time.days() < target_day {
            tick(&mut self.galaxy);
        }
    }

    /// Radius-ish size of a place, for drop distances and collision.
    pub fn place_radius(&self, place: Place) -> f64 {
        match place {
            Place::Star => self.layout.star.radius_m,
            Place::Body(i) => self.layout.bodies[i].radius_m,
            Place::Field(i) => self.layout.fields[i].radius_m,
            Place::Station(i) => self.layout.stations[i].size_m * 0.5,
            Place::Gate(_) => 1000.0,
        }
    }

    /// Distance at which supercruise should hand over to normal flight.
    fn drop_distance(&self, place: Place) -> f64 {
        match place {
            Place::Star => self.layout.star.radius_m * 4.0,
            Place::Body(i) => self.layout.bodies[i].radius_m * 1.6,
            Place::Field(i) => self.layout.fields[i].radius_m + 15e3,
            Place::Station(_) => 25e3,
            Place::Gate(_) => 30e3,
        }
    }

    pub fn distance_to(&self, place: Place) -> f64 {
        (self.layout.position(place, self.clock_s) - self.ship_position()).length()
    }

    fn nearest_place(&self) -> Place {
        let pos = self.ship_position();
        self.layout
            .places()
            .min_by(|a, b| {
                let da =
                    (self.layout.position(*a, self.clock_s) - pos).length() - self.place_radius(*a);
                let db =
                    (self.layout.position(*b, self.clock_s) - pos).length() - self.place_radius(*b);
                da.total_cmp(&db)
            })
            .unwrap_or(Place::Star)
    }

    /// Move the ship into a new frame without moving it in space.
    fn reframe(&mut self, frame: Place) {
        let pos = self.ship_position();
        self.ship.rel = pos - self.layout.position(frame, self.clock_s);
        self.ship.frame = frame;
    }

    pub fn prompts(&self) -> Prompts {
        let mut p = Prompts::default();
        match self.mode {
            Mode::Normal => {
                let speed = self.ship.vel.length();
                for (i, s) in self.layout.stations.iter().enumerate() {
                    if self.distance_to(Place::Station(i)) < s.size_m * 1.5 + 400.0 && speed < 150.0
                    {
                        p.dock = Some(i);
                    }
                }
                for (i, b) in self.layout.bodies.iter().enumerate() {
                    if !b.landing_points.is_empty()
                        && self.distance_to(Place::Body(i)) < b.radius_m * 2.0
                    {
                        p.land = Some((i, 0));
                    }
                }
                for i in 0..self.layout.gates.len() {
                    if self.distance_to(Place::Gate(i)) < 40e3 {
                        p.jump = Some(i);
                    }
                }
            }
            Mode::Supercruise => {
                if let Some(t) = self.target {
                    p.drop = self.distance_to(t) < self.drop_distance(t) * 1.5;
                }
            }
            _ => {}
        }
        p
    }

    pub fn act(&mut self, action: Action) {
        let prompts = self.prompts();
        match action {
            Action::ToggleCamera => self.cockpit = !self.cockpit,
            Action::ToggleFlightAssist => {
                self.ship.flight_assist = !self.ship.flight_assist;
                let state = if self.ship.flight_assist { "on" } else { "off" };
                self.say(format!("Flight assist {state}"));
            }
            Action::Target(place) => self.target = Some(place),
            Action::CycleTarget => {
                let places: Vec<Place> = self.layout.places().collect();
                let next = match self
                    .target
                    .and_then(|t| places.iter().position(|p| *p == t))
                {
                    Some(i) => places[(i + 1) % places.len()],
                    None => places[0],
                };
                self.target = Some(next);
            }
            Action::ToggleSupercruise => match self.mode {
                Mode::Normal => {
                    self.reframe(Place::Star);
                    self.ship.cruise_speed = self.ship.vel.length().max(1000.0);
                    self.ship.vel = DVec3::ZERO;
                    self.mode = Mode::Supercruise;
                    self.say("Supercruise engaged");
                }
                Mode::Supercruise => self.drop_out(),
                _ => {}
            },
            Action::Dock => {
                if let Some(i) = prompts.dock {
                    self.mode = Mode::Docked(i);
                    self.ship.frame = Place::Station(i);
                    self.ship.rel = DVec3::ZERO;
                    self.ship.vel = DVec3::ZERO;
                    self.say(format!("Docked at {}", self.layout.stations[i].name));
                }
            }
            Action::Undock => {
                if let Mode::Docked(i) = self.mode {
                    let size = self.layout.stations[i].size_m;
                    self.ship.frame = Place::Station(i);
                    self.ship.rel = self.ship.rot * DVec3::new(0.0, 0.0, -(size * 0.9 + 150.0));
                    self.ship.vel = self.ship.rot * DVec3::new(0.0, 0.0, -60.0);
                    self.ship.throttle = 0.2;
                    self.mode = Mode::Normal;
                }
            }
            Action::Land => {
                if let Some((body, point)) = prompts.land {
                    self.mode = Mode::Landed {
                        body,
                        point,
                        timer: LANDING_DURATION,
                    };
                }
            }
            Action::Launch => {
                if let Mode::Landed { body, .. } = self.mode {
                    let r = self.layout.bodies[body].radius_m;
                    self.ship.frame = Place::Body(body);
                    self.ship.rel = self.ship.rot * DVec3::new(0.0, 0.0, r * 1.25);
                    self.ship.vel = DVec3::ZERO;
                    self.mode = Mode::Normal;
                    self.advance_clock(3600.0);
                }
            }
            Action::Jump => {
                if let Some(gate) = prompts.jump {
                    self.mode = Mode::Jumping {
                        gate,
                        timer: JUMP_DURATION,
                    };
                }
            }
            Action::Wait(days) => {
                if matches!(self.mode, Mode::Docked(_)) {
                    self.advance_clock(days as f64 * SECONDS_PER_DAY);
                    self.say(format!(
                        "Waited {days} day{}",
                        if days == 1 { "" } else { "s" }
                    ));
                }
            }
        }
    }

    fn drop_out(&mut self) {
        let frame = self.nearest_place();
        self.reframe(frame);
        self.ship.vel = self.ship.rot * DVec3::new(0.0, 0.0, -MAX_SPEED * 0.5);
        self.ship.throttle = 0.5;
        self.mode = Mode::Normal;
        let name = self.layout.name(frame).to_owned();
        self.say(format!("Dropped out near {name}"));
    }

    pub fn update(&mut self, dt: f64, controls: Controls) {
        if let Some((_, t)) = &mut self.message {
            *t -= dt as f32;
            if *t <= 0.0 {
                self.message = None;
            }
        }

        match self.mode {
            Mode::Normal => {
                self.steer(dt, controls);
                self.fly_normal(dt, controls);
                self.advance_clock(dt);
            }
            Mode::Supercruise => {
                self.steer(dt, controls);
                self.fly_supercruise(dt, controls);
                self.advance_clock(dt * SUPERCRUISE_TIME_RATE);
            }
            Mode::Docked(_) => self.advance_clock(dt),
            Mode::Landed { body, point, timer } => {
                self.mode = Mode::Landed {
                    body,
                    point,
                    timer: (timer - dt as f32).max(0.0),
                };
                self.advance_clock(dt);
            }
            Mode::Jumping { gate, timer } => {
                let timer = timer - dt as f32;
                if timer <= 0.0 {
                    self.arrive(gate);
                } else {
                    self.mode = Mode::Jumping { gate, timer };
                }
            }
        }

        self.update_field();
        self.update_camera(dt);
    }

    fn steer(&mut self, dt: f64, c: Controls) {
        let target = c.stick * TURN_RATES;
        let k = 1.0 - (-6.0 * dt).exp();
        self.ship.spin += (target - self.ship.spin) * k;
        let s = self.ship.spin;
        // Local axes: x = pitch, y = yaw (negated: + yaw turns right), z = roll.
        let delta = DQuat::from_scaled_axis(DVec3::new(s.x, -s.y, -s.z) * dt);
        self.ship.rot = (self.ship.rot * delta).normalize();
        self.ship.throttle = (self.ship.throttle + c.throttle_delta * dt * 0.8).clamp(-0.3, 1.0);
    }

    fn fly_normal(&mut self, dt: f64, c: Controls) {
        let boost = if c.boost { BOOST } else { 1.0 };
        let local = DVec3::new(
            c.strafe * STRAFE_SPEED,
            c.lift * STRAFE_SPEED,
            -self.ship.throttle * MAX_SPEED * boost,
        );
        let accel = ACCEL * boost;
        if self.ship.flight_assist {
            let desired = self.ship.rot * local;
            let diff = desired - self.ship.vel;
            let step = diff.clamp_length_max(accel * dt);
            self.ship.vel += step;
        } else {
            let thrust = self.ship.rot
                * DVec3::new(
                    c.strafe,
                    c.lift,
                    -self.ship.throttle.signum()
                        * c.throttle_delta.abs().max(self.ship.throttle.abs()),
                );
            self.ship.vel += thrust * accel * dt;
        }
        self.ship.rel += self.ship.vel * dt;
        self.collide();
    }

    fn fly_supercruise(&mut self, dt: f64, _c: Controls) {
        let pos = self.ship_position();
        // Slow down near anything massive: the limit scales with the
        // distance to the nearest surface.
        let nearest = self
            .layout
            .places()
            .filter(|p| matches!(p, Place::Star | Place::Body(_)))
            .map(|p| (self.layout.position(p, self.clock_s) - pos).length() - self.place_radius(p))
            .fold(f64::INFINITY, f64::min)
            .max(1.0);
        let limit = (nearest * 0.35).clamp(30e3, 3e10);
        let target = self.ship.throttle.max(0.05) * limit;
        let k = 1.0 - (-1.8 * dt).exp();
        self.ship.cruise_speed += (target - self.ship.cruise_speed) * k;
        self.ship.cruise_speed = self.ship.cruise_speed.min(limit * 1.2);
        self.ship.rel += self.ship.rot * DVec3::new(0.0, 0.0, -self.ship.cruise_speed * dt);
        // Hitting a planet in supercruise drops you out.
        for i in 0..self.layout.bodies.len() {
            if self.distance_to(Place::Body(i)) < self.layout.bodies[i].radius_m * 1.05 {
                self.drop_out();
                self.say("Emergency drop: too close to a planet");
                return;
            }
        }
    }

    /// Push the ship out of planets, stations and rocks.
    fn collide(&mut self) {
        let pos = self.ship_position();
        let push = |center: DVec3, radius: f64, ship: &mut Ship| {
            let d = pos - center;
            let len = d.length();
            if len < radius && len > 0.0 {
                let n = d / len;
                ship.rel += n * (radius - len);
                let into = ship.vel.dot(n);
                if into < 0.0 {
                    ship.vel -= n * into * 1.3;
                }
            }
        };
        for (i, b) in self.layout.bodies.iter().enumerate() {
            push(
                self.layout.position(Place::Body(i), self.clock_s),
                b.radius_m + 30.0,
                &mut self.ship,
            );
        }
        push(
            DVec3::ZERO,
            self.layout.star.radius_m * 1.02,
            &mut self.ship,
        );
        for (i, s) in self.layout.stations.iter().enumerate() {
            push(
                self.layout.position(Place::Station(i), self.clock_s),
                s.size_m * 0.35,
                &mut self.ship,
            );
        }
        if let Some((field, rocks)) = &self.field_rocks {
            let center = self.layout.position(Place::Field(*field), self.clock_s);
            for rock in rocks {
                push(center + rock.offset, rock.radius_m + 8.0, &mut self.ship);
            }
        }
    }

    fn update_field(&mut self) {
        let pos = self.ship_position();
        let inside = self.layout.fields.iter().enumerate().find(|(i, f)| {
            (self.layout.position(Place::Field(*i), self.clock_s) - pos).length()
                < f.radius_m + 50e3
        });
        match (inside, &self.field_rocks) {
            (Some((i, _)), Some((current, _))) if i == *current => {}
            (Some((i, field)), _) => self.field_rocks = Some((i, rocks(field))),
            (None, Some(_)) => self.field_rocks = None,
            (None, None) => {}
        }
    }

    fn arrive(&mut self, gate: usize) {
        let from = self.system;
        let to = self.layout.gates[gate].to;
        let lane = self.galaxy.systems[from]
            .lanes
            .iter()
            .map(|&l| &self.galaxy.lanes[l])
            .find(|l| l.other(from) == to);
        let days = lane
            .map_or(3.0, |l| (l.length_ly * 0.6).clamp(2.0, 14.0))
            .round();
        self.advance_clock(days * SECONDS_PER_DAY);

        self.system = to;
        self.layout = system_layout(&self.galaxy, to);
        self.layout_generation += 1;
        self.field_rocks = None;
        self.target = None;
        let back = self
            .layout
            .gates
            .iter()
            .position(|g| g.to == from)
            .unwrap_or(0);
        // Arrive just in front of the return gate, facing the star.
        let gate_pos = self.layout.gates[back].position;
        let inward = (-gate_pos).normalize_or(DVec3::Z);
        self.ship.frame = Place::Gate(back);
        self.ship.rel = inward * 3000.0;
        self.ship.rot = DQuat::from_rotation_arc(DVec3::NEG_Z, inward);
        self.cam_rot = self.ship.rot;
        self.ship.vel = inward * 150.0;
        self.ship.throttle = 0.4;
        self.mode = Mode::Normal;
        let name = self.galaxy.systems[to].name.clone();
        self.say(format!("Arrived at {name} after {days} days in transit"));
    }

    fn update_camera(&mut self, dt: f64) {
        let k = 1.0 - (-9.0 * dt).exp();
        self.cam_rot = self.cam_rot.slerp(self.ship.rot, k).normalize();
    }

    /// Camera position and orientation in the system frame.
    pub fn camera(&self) -> (DVec3, DQuat) {
        let ship = self.ship_position();
        match self.mode {
            Mode::Docked(i) => {
                // An exterior view of the station.
                let size = self.layout.stations[i].size_m;
                let station = self.layout.position(Place::Station(i), self.clock_s);
                let t = self.clock_s * 0.02;
                let offset = DVec3::new(t.cos(), 0.35, t.sin()) * size * 1.6;
                let rot = look_rotation(-offset);
                (station + offset, rot)
            }
            _ if self.cockpit => (
                ship + self.ship.rot * DVec3::new(0.0, 1.2, -8.5),
                self.ship.rot,
            ),
            _ => (
                ship + self.cam_rot * DVec3::new(0.0, 6.0, 28.0),
                self.cam_rot,
            ),
        }
    }
}

/// Rotation that points -Z along `dir`, keeping +Y roughly up.
pub fn look_rotation(dir: DVec3) -> DQuat {
    let f = dir.normalize_or(DVec3::NEG_Z);
    let r = f.cross(DVec3::Y).normalize_or(DVec3::X);
    let u = r.cross(f);
    DQuat::from_mat3(&glam::DMat3::from_cols(r, u, -f))
}
