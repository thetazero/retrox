//! The player's world: the galaxy sim, the current system, the galaxy
//! clock, and the ship.
//!
//! The sim is the source of truth for what's in a system: bodies and their
//! orbits, asteroid fields, and settlements. Space settlements are stations
//! you dock at; settlements on a planet's regions are cities you land at.
//!
//! **Three ways to move:**
//! 1. *Normal flight*, in real time. The throttle's lower half is fine
//!    control up to [`COMBAT_SPEED`]; the upper half ramps up to
//!    [`CRUISE_SPEED`] for crossing gravity wells. One flight model.
//! 2. *Fast travel*: automatic, to a nav target, only from outside every
//!    gravity well. You arrive at the edge of the target's well (or near a
//!    station or field); galaxy time passes by distance.
//! 3. *Hyperlane travel*: to a neighbouring system, also only from outside
//!    gravity wells. Days pass.
//!
//! **Units.** The sim uses AU in a frame with the system plane as x/y and
//! days. The client uses metres with y up (system plane x/z) and seconds.
//!
//! **Frames.** The ship's position is stored relative to a *frame*: the
//! place it arrived at or launched from. The frame moves with that place,
//! so nearby things hold still while you fly.

use std::sync::LazyLock;

use glam::{DQuat, DVec3};
use retrox_sim::ship::ShipStats;

use crate::hangar::{self, Job, Player};
use retrox_gen::{GalaxyConfig, Rock, generate, rocks};
use retrox_sim::{
    BodyId, BodyKind, FieldId, GalaxyState, GalaxyTime, Orbit, SettlementId, Site, StarClass,
    SystemId, tick,
};

pub const AU: f64 = 1.495_978_707e11;
pub const SECONDS_PER_DAY: f64 = 86_400.0;
const SOLAR_RADIUS_M: f64 = 6.957e8;

/// Top of the fine-control range, m/s (throttle 0.5).
pub const COMBAT_SPEED: f64 = 500.0;
/// Top speed at full throttle, m/s.
pub const CRUISE_SPEED: f64 = 150_000.0;
const BOOST: f64 = 2.0;
const ACCEL: f64 = 90.0;
const STRAFE_SPEED: f64 = 80.0;
/// Pitch, yaw, roll rates at full stick, rad/s.
const TURN_RATES: DVec3 = DVec3::new(1.3, 0.9, 2.2);
/// Gravity wells extend this many radii from the centre of a star or body.
pub const WELL_RADII: f64 = 4.0;
/// Galaxy time per metre of fast travel: about 1.7 days per AU.
const FAST_TRAVEL_SECONDS_PER_METRE: f64 = 1.0e-6;
const JUMP_DURATION: f32 = 4.0;
const LANDING_DURATION: f32 = 3.0;
/// The ship the flight model is tuned for: the starting ship, stock.
static REFERENCE: LazyLock<ShipStats> = LazyLock::new(|| {
    retrox_content::ships::stock(hangar::STARTING_HULL)
        .expect("starting hull exists")
        .stats()
});

/// Something in the current system you can navigate to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    Star,
    Body(BodyId),
    Field(FieldId),
    /// A settlement in space.
    Station(SettlementId),
}

/// The current system's contents, indexed once on arrival.
#[derive(Clone, Debug, Default)]
pub struct SystemView {
    /// Planets, gas giants and moons (belts are only drawn as their fields).
    pub bodies: Vec<BodyId>,
    pub fields: Vec<FieldId>,
    pub stations: Vec<SettlementId>,
    /// Settlements on planet regions, with the body they're on.
    pub cities: Vec<(SettlementId, BodyId)>,
}

impl SystemView {
    fn new(galaxy: &GalaxyState, system: SystemId) -> Self {
        let bodies = galaxy.systems[system]
            .bodies
            .iter()
            .copied()
            .filter(|&b| galaxy.bodies[b].kind != BodyKind::Belt)
            .collect();
        let fields = galaxy
            .fields
            .iter()
            .filter(|(_, f)| galaxy.bodies[f.belt].system == system)
            .map(|(id, _)| id)
            .collect();
        let mut stations = Vec::new();
        let mut cities = Vec::new();
        for (id, s) in galaxy.settlements_in(system) {
            match s.site {
                Site::Space(_) => stations.push(id),
                Site::Region(r) => cities.push((id, galaxy.regions[r].body)),
                // Surface settlements on undivided bodies: neither docked nor
                // landed at, for now.
                Site::Surface(_) => {}
            }
        }
        Self {
            bodies,
            fields,
            stations,
            cities,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Normal,
    FastTravel(FastTravel),
    Docked(SettlementId),
    /// Landing animation (timer > 0), then the city screen.
    Landed {
        city: SettlementId,
        body: BodyId,
        timer: f32,
    },
    Jumping {
        to: SystemId,
        timer: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FastTravel {
    pub target: Place,
    /// Start and end, in the star's frame.
    pub from: DVec3,
    pub to: DVec3,
    /// Galaxy clock at departure, and the trip's galaxy duration.
    pub depart_s: f64,
    pub travel_s: f64,
    /// Real seconds: elapsed and total.
    pub elapsed: f32,
    pub duration: f32,
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
    /// -0.3..1. See [`throttle_speed`].
    pub throttle: f64,
    pub flight_assist: bool,
}

/// Forward speed for a throttle setting: linear up to [`COMBAT_SPEED`] at
/// 0.5, then exponential up to [`CRUISE_SPEED`] at 1.0.
pub fn throttle_speed(throttle: f64) -> f64 {
    if throttle <= 0.5 {
        throttle / 0.5 * COMBAT_SPEED
    } else {
        let t = (throttle - 0.5) / 0.5;
        COMBAT_SPEED * (CRUISE_SPEED / COMBAT_SPEED).powf(t)
    }
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    FastTravel,
    Dock,
    Undock,
    Land,
    Launch,
    Jump(SystemId),
    CycleTarget,
    Target(Place),
    ToggleCamera,
    ToggleFlightAssist,
    Wait(u64),
    /// A job in the hangar of the settlement we're docked or landed at.
    Hangar(Job),
}

/// What the player could do right now (shown as prompts).
#[derive(Clone, Debug, Default)]
pub struct Prompts {
    pub dock: Option<SettlementId>,
    /// A city to land at, and its body.
    pub land: Option<(SettlementId, BodyId)>,
    /// The gravity well the ship is in, if any.
    pub well: Option<Place>,
    pub fast_travel: Option<Place>,
}

/// A hyperlane out of the current system.
#[derive(Clone, Debug)]
pub struct LaneInfo {
    pub to: SystemId,
    pub name: String,
    pub length_ly: f64,
    pub days: f64,
}

pub struct World {
    pub galaxy: GalaxyState,
    pub system: SystemId,
    pub view: SystemView,
    /// Galaxy seconds since the start of history (finer than the sim's days).
    pub clock_s: f64,
    pub ship: Ship,
    pub mode: Mode,
    pub target: Option<Place>,
    pub cockpit: bool,
    pub cam_rot: DQuat,
    /// Rocks of the asteroid field the ship is in, if any.
    pub field_rocks: Option<(FieldId, Vec<Rock>)>,
    pub message: Option<(String, f32)>,
    /// The player's money and ships.
    pub player: Player,
    /// Bumped whenever the system changes, so the client re-uploads meshes.
    pub system_generation: u64,
}

impl World {
    pub fn new(seed: u64) -> Self {
        let galaxy = generate(seed, &GalaxyConfig::default());
        // Start landed at the biggest faction capital that's a city.
        let (city, body) = galaxy
            .factions
            .values()
            .filter_map(|f| {
                let s = galaxy.settlements.get(f.capital)?;
                match s.site {
                    Site::Region(r) => {
                        Some((f.capital, galaxy.regions[r].body, s.economy.population))
                    }
                    _ => None,
                }
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(s, b, _)| (s, b))
            .expect("some faction capital is a city");
        let system = galaxy.settlements[city].system;
        let view = SystemView::new(&galaxy, system);
        let clock_s = galaxy.time.days() as f64 * SECONDS_PER_DAY;
        Self {
            galaxy,
            system,
            view,
            clock_s,
            ship: Ship {
                frame: Place::Body(body),
                rel: DVec3::ZERO,
                vel: DVec3::ZERO,
                rot: DQuat::IDENTITY,
                spin: DVec3::ZERO,
                throttle: 0.0,
                flight_assist: true,
            },
            mode: Mode::Landed {
                city,
                body,
                timer: 0.0,
            },
            target: None,
            cockpit: false,
            cam_rot: DQuat::IDENTITY,
            field_rocks: None,
            message: None,
            player: Player::new(),
            system_generation: 0,
        }
    }

    fn t_days(&self) -> f64 {
        self.clock_s / SECONDS_PER_DAY
    }

    pub fn star(&self) -> StarClass {
        self.galaxy.systems[self.system].star
    }

    pub fn star_radius_m(&self) -> f64 {
        star_radius_suns(self.star()) * SOLAR_RADIUS_M
    }

    /// Position of a place in the system frame (metres, y up).
    pub fn position(&self, place: Place) -> DVec3 {
        let t = self.t_days();
        match place {
            Place::Star => DVec3::ZERO,
            Place::Body(b) => to_metres(self.galaxy.body_position(b, t)),
            Place::Field(f) => to_metres(self.galaxy.orbit_position(
                self.system,
                &self.galaxy.fields[f].orbit,
                t,
            )),
            Place::Station(s) => match self.galaxy.settlements.get(s).map(|s| s.site) {
                Some(Site::Space(orbit)) => self.orbit_m(&orbit),
                _ => DVec3::ZERO,
            },
        }
    }

    fn orbit_m(&self, orbit: &Orbit) -> DVec3 {
        to_metres(
            self.galaxy
                .orbit_position(self.system, orbit, self.t_days()),
        )
    }

    pub fn name(&self, place: Place) -> String {
        match place {
            Place::Star => format!("{} (star)", self.galaxy.systems[self.system].name),
            Place::Body(b) => self.galaxy.bodies[b].name.clone(),
            Place::Field(f) => self.galaxy.fields[f].name.clone(),
            Place::Station(s) => self
                .galaxy
                .settlements
                .get(s)
                .map_or("(gone)".into(), |s| s.name.clone()),
        }
    }

    pub fn ship_position(&self) -> DVec3 {
        self.position(self.ship.frame) + self.ship.rel
    }

    pub fn date(&self) -> GalaxyTime {
        GalaxyTime::from_days(self.t_days() as u64)
    }

    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), 4.0));
    }

    /// Advance the galaxy clock, ticking the sim for every day crossed.
    pub fn advance_clock(&mut self, seconds: f64) {
        self.clock_s += seconds;
        let target_day = self.t_days() as u64;
        while self.galaxy.time.days() < target_day {
            tick(&mut self.galaxy);
        }
    }

    /// Places worth navigating to, in display order.
    pub fn nav_places(&self) -> Vec<Place> {
        std::iter::once(Place::Star)
            .chain(self.view.bodies.iter().map(|&b| Place::Body(b)))
            .chain(self.view.fields.iter().map(|&f| Place::Field(f)))
            .chain(self.view.stations.iter().map(|&s| Place::Station(s)))
            .collect()
    }

    /// Station size in metres, from its population.
    pub fn station_size_m(&self, s: SettlementId) -> f64 {
        let pop = self
            .galaxy
            .settlements
            .get(s)
            .map_or(0.0, |s| s.economy.population);
        (400.0 + pop.sqrt() * 250.0).min(4000.0)
    }

    /// Radius-ish size of a place, for arrival distances and collision.
    pub fn place_radius(&self, place: Place) -> f64 {
        match place {
            Place::Star => self.star_radius_m(),
            Place::Body(b) => self.galaxy.bodies[b].radius_km * 1000.0,
            Place::Field(f) => self.galaxy.fields[f].radius_km * 1000.0,
            Place::Station(s) => self.station_size_m(s) * 0.5,
        }
    }

    /// Gravity well radius; zero for things without one.
    pub fn well_radius(&self, place: Place) -> f64 {
        match place {
            Place::Star | Place::Body(_) => self.place_radius(place) * WELL_RADII,
            _ => 0.0,
        }
    }

    /// Where fast travel to `place` stops, measured from its centre.
    pub fn arrival_distance(&self, place: Place) -> f64 {
        match place {
            Place::Star | Place::Body(_) => self.well_radius(place) * 1.05,
            Place::Field(_) => self.place_radius(place) + 10e3,
            Place::Station(_) => 20e3,
        }
    }

    /// The innermost gravity well the ship is in, if any.
    pub fn current_well(&self) -> Option<Place> {
        let pos = self.ship_position();
        self.nav_places()
            .into_iter()
            .filter(|p| matches!(p, Place::Star | Place::Body(_)))
            .filter(|p| (self.position(*p) - pos).length() < self.well_radius(*p))
            .min_by(|a, b| self.well_radius(*a).total_cmp(&self.well_radius(*b)))
    }

    pub fn distance_to(&self, place: Place) -> f64 {
        (self.position(place) - self.ship_position()).length()
    }

    /// Days to cross a lane, by the jump drive's speed with the ship's mass,
    /// relative to the starting ship.
    pub fn jump_days(&self, length_ly: f64) -> f64 {
        let speed = self.stats().jump_speed();
        let slower = if speed > 0.0 {
            (REFERENCE.jump_speed() / speed).clamp(0.5, 3.0)
        } else {
            1.0
        };
        (jump_days(length_ly) * slower).round().max(1.0)
    }

    pub fn lanes(&self) -> Vec<LaneInfo> {
        self.galaxy.systems[self.system]
            .lanes
            .iter()
            .map(|&l| {
                let lane = &self.galaxy.lanes[l];
                let to = lane.other(self.system);
                LaneInfo {
                    to,
                    name: self.galaxy.systems[to].name.clone(),
                    length_ly: lane.length_ly,
                    days: self.jump_days(lane.length_ly),
                }
            })
            .collect()
    }

    pub fn prompts(&self) -> Prompts {
        let mut p = Prompts::default();
        if self.mode != Mode::Normal {
            return p;
        }
        let speed = self.ship.vel.length();
        for &s in &self.view.stations {
            let reach = self.station_size_m(s) * 1.5 + 400.0;
            if self.distance_to(Place::Station(s)) < reach && speed < 150.0 {
                p.dock = Some(s);
            }
        }
        for &(city, body) in &self.view.cities {
            if self.distance_to(Place::Body(body)) < self.place_radius(Place::Body(body)) * 2.0 {
                p.land = Some((city, body));
            }
        }
        p.well = self.current_well();
        if p.well.is_none() {
            p.fast_travel = self
                .target
                .filter(|&t| self.distance_to(t) > self.arrival_distance(t) * 1.2);
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
                let places = self.nav_places();
                let next = match self
                    .target
                    .and_then(|t| places.iter().position(|p| *p == t))
                {
                    Some(i) => places[(i + 1) % places.len()],
                    None => places[0],
                };
                self.target = Some(next);
            }
            Action::FastTravel => match (self.mode, prompts.fast_travel, prompts.well) {
                (Mode::Normal, Some(target), _) => self.start_fast_travel(target),
                (Mode::Normal, None, Some(well)) => {
                    let name = self.name(well);
                    self.say(format!(
                        "Can't fast travel inside the gravity well of {name}"
                    ));
                }
                (Mode::Normal, None, None) => self.say("Select a target to fast travel"),
                _ => {}
            },
            Action::Dock => {
                if let Some(s) = prompts.dock {
                    self.mode = Mode::Docked(s);
                    self.ship.frame = Place::Station(s);
                    self.ship.rel = DVec3::ZERO;
                    self.ship.vel = DVec3::ZERO;
                    self.say(format!("Docked at {}", self.name(Place::Station(s))));
                }
            }
            Action::Undock => {
                if let Mode::Docked(s) = self.mode {
                    if !self.can_fly() {
                        return;
                    }
                    let size = self.station_size_m(s);
                    self.ship.frame = Place::Station(s);
                    self.ship.rel = self.ship.rot * DVec3::new(0.0, 0.0, -(size * 0.9 + 150.0));
                    self.ship.vel = self.ship.rot * DVec3::new(0.0, 0.0, -60.0);
                    self.ship.throttle = 0.1;
                    self.mode = Mode::Normal;
                }
            }
            Action::Land => {
                if let Some((city, body)) = prompts.land {
                    self.mode = Mode::Landed {
                        city,
                        body,
                        timer: LANDING_DURATION,
                    };
                }
            }
            Action::Launch => {
                if let Mode::Landed { body, .. } = self.mode {
                    if !self.can_fly() {
                        return;
                    }
                    let r = self.place_radius(Place::Body(body));
                    self.ship.frame = Place::Body(body);
                    self.ship.rel = self.ship.rot * DVec3::new(0.0, 0.0, r * 1.25);
                    self.ship.vel = DVec3::ZERO;
                    self.ship.throttle = 0.0;
                    self.mode = Mode::Normal;
                    self.advance_clock(3600.0);
                }
            }
            Action::Jump(to) => {
                if self.mode != Mode::Normal {
                    return;
                }
                if let Some(well) = prompts.well {
                    let name = self.name(well);
                    self.say(format!("Can't jump inside the gravity well of {name}"));
                } else if self.stats().drive_tier == 0 {
                    self.say("Can't jump: no jump drive fitted");
                } else if self.lanes().iter().any(|l| l.to == to) {
                    self.mode = Mode::Jumping {
                        to,
                        timer: JUMP_DURATION,
                    };
                }
            }
            Action::Wait(days) => {
                if matches!(self.mode, Mode::Docked(_) | Mode::Landed { .. }) {
                    self.advance_clock(days as f64 * SECONDS_PER_DAY);
                    self.say(format!(
                        "Waited {days} day{}",
                        if days == 1 { "" } else { "s" }
                    ));
                }
            }
            Action::Hangar(job) => {
                let Some(here) = self.docked_at() else {
                    return;
                };
                let economy = &mut self.galaxy.settlements[here].economy;
                match hangar::run(&mut self.player, here, economy, job) {
                    Ok(done) => {
                        self.advance_clock(done.seconds);
                        self.say(done.message);
                    }
                    Err(why) => self.say(why),
                }
            }
        }
    }

    /// The settlement we're docked at, or landed at (once down).
    pub fn docked_at(&self) -> Option<SettlementId> {
        match self.mode {
            Mode::Docked(s) => Some(s),
            Mode::Landed { city, timer, .. } if timer <= 0.0 => Some(city),
            _ => None,
        }
    }

    /// How far the chase camera sits, relative to a ship ~20 m long.
    fn camera_scale(&self) -> f64 {
        (self.stats().length_m / 20.0).clamp(0.8, 6.0)
    }

    /// The stats of the ship being flown.
    pub fn stats(&self) -> ShipStats {
        self.player.ship.fit.stats()
    }

    /// Whether the ship can leave: it needs thrust and a way to turn.
    fn can_fly(&mut self) -> bool {
        let stats = self.stats();
        if stats.acceleration() <= 0.0 {
            self.say("Can't leave: no engine fitted");
        } else if stats.angular_acceleration() <= 0.0 {
            self.say("Can't leave: no RCS fitted");
        } else {
            return true;
        }
        false
    }

    /// How the ship handles next to the starting ship: thrust and turning
    /// multipliers, within limits so every ship stays flyable.
    pub fn handling(&self) -> (f64, f64) {
        let (s, r) = (self.stats(), &*REFERENCE);
        let thrust = (s.acceleration() / r.acceleration()).clamp(0.3, 2.5);
        let turn = (s.angular_acceleration() / r.angular_acceleration())
            .sqrt()
            .clamp(0.3, 1.8);
        (thrust, turn)
    }

    fn start_fast_travel(&mut self, target: Place) {
        let from = self.ship_position();
        let distance = (self.position(target) - from).length();
        let travel_s = distance * FAST_TRAVEL_SECONDS_PER_METRE;
        // Aim for where the target will be on arrival, approaching from our side.
        let now = self.clock_s;
        self.clock_s += travel_s;
        let arrive_at = self.position(target);
        self.clock_s = now;
        let approach = (from - arrive_at).normalize_or(DVec3::Z);
        let to = arrive_at + approach * self.arrival_distance(target);
        let duration = (2.0 + (distance / 1e7).max(1.0).log10() * 1.2).min(7.0) as f32;
        self.ship.rot = look_rotation(to - from);
        self.ship.vel = DVec3::ZERO;
        self.ship.spin = DVec3::ZERO;
        self.mode = Mode::FastTravel(FastTravel {
            target,
            from,
            to,
            depart_s: self.clock_s,
            travel_s,
            elapsed: 0.0,
            duration,
        });
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
                self.fly(dt, controls);
                self.advance_clock(dt);
            }
            Mode::FastTravel(mut trip) => {
                trip.elapsed += dt as f32;
                let k = (trip.elapsed / trip.duration).min(1.0) as f64;
                // Ease in and out.
                let s = k * k * (3.0 - 2.0 * k);
                let clock = trip.depart_s + trip.travel_s * k;
                self.advance_clock(clock - self.clock_s);
                self.ship.frame = Place::Star;
                self.ship.rel = trip.from.lerp(trip.to, s);
                if k >= 1.0 {
                    self.arrive_fast_travel(trip.target);
                } else {
                    self.mode = Mode::FastTravel(trip);
                }
            }
            Mode::Docked(_) => self.advance_clock(dt),
            Mode::Landed { city, body, timer } => {
                self.mode = Mode::Landed {
                    city,
                    body,
                    timer: (timer - dt as f32).max(0.0),
                };
                self.advance_clock(dt);
            }
            Mode::Jumping { to, timer } => {
                let timer = timer - dt as f32;
                if timer <= 0.0 {
                    self.arrive_jump(to);
                } else {
                    self.mode = Mode::Jumping { to, timer };
                }
            }
        }

        self.update_field();
        self.update_camera(dt);
    }

    fn arrive_fast_travel(&mut self, target: Place) {
        let pos = self.ship_position();
        self.ship.frame = target;
        self.ship.rel = pos - self.position(target);
        self.ship.rot = look_rotation(-self.ship.rel);
        self.cam_rot = self.ship.rot;
        self.ship.vel = DVec3::ZERO;
        self.ship.throttle = 0.0;
        self.mode = Mode::Normal;
        let name = self.name(target);
        self.say(format!("Arrived at {name}"));
    }

    fn steer(&mut self, dt: f64, c: Controls) {
        let target = c.stick * TURN_RATES * self.handling().1;
        let k = 1.0 - (-6.0 * dt).exp();
        self.ship.spin += (target - self.ship.spin) * k;
        let s = self.ship.spin;
        // Local axes: x = pitch, y = yaw (negated: + yaw turns right), z = roll.
        let delta = DQuat::from_scaled_axis(DVec3::new(s.x, -s.y, -s.z) * dt);
        self.ship.rot = (self.ship.rot * delta).normalize();
        self.ship.throttle = (self.ship.throttle + c.throttle_delta * dt * 0.4).clamp(-0.3, 1.0);
    }

    fn fly(&mut self, dt: f64, c: Controls) {
        // Boost only helps in the fine-control range.
        let boost = if c.boost && self.ship.throttle <= 0.5 {
            BOOST
        } else {
            1.0
        };
        let forward = if self.ship.throttle >= 0.0 {
            throttle_speed(self.ship.throttle)
        } else {
            self.ship.throttle / 0.5 * COMBAT_SPEED
        };
        let local = DVec3::new(
            c.strafe * STRAFE_SPEED,
            c.lift * STRAFE_SPEED,
            -forward * boost,
        );
        let desired = self.ship.rot * local;
        // Acceleration scales with the speed being asked for, so cruise
        // speeds are reached in a few seconds without making fine control
        // twitchy.
        let accel = (ACCEL * boost * self.handling().0)
            .max(desired.length().max(self.ship.vel.length()) * 0.5);
        if self.ship.flight_assist {
            let step = (desired - self.ship.vel).clamp_length_max(accel * dt);
            self.ship.vel += step;
        } else {
            let thrust = self.ship.rot * DVec3::new(c.strafe, c.lift, -c.throttle_delta.max(0.0));
            self.ship.vel += thrust * accel * dt;
        }
        self.ship.rel += self.ship.vel * dt;
        self.collide();
    }

    /// Push the ship out of the star, bodies, stations and rocks.
    fn collide(&mut self) {
        let pos = self.ship_position();
        let mut solids: Vec<(DVec3, f64)> = vec![(DVec3::ZERO, self.star_radius_m() * 1.02)];
        for &b in &self.view.bodies {
            solids.push((
                self.position(Place::Body(b)),
                self.place_radius(Place::Body(b)) + 30.0,
            ));
        }
        for &s in &self.view.stations {
            solids.push((
                self.position(Place::Station(s)),
                self.station_size_m(s) * 0.35,
            ));
        }
        if let Some((field, rocks)) = &self.field_rocks {
            let center = self.position(Place::Field(*field));
            for rock in rocks {
                solids.push((center + rock_offset(rock), rock.radius_m + 8.0));
            }
        }
        for (center, radius) in solids {
            let d = pos - center;
            let len = d.length();
            if len < radius && len > 0.0 {
                let n = d / len;
                self.ship.rel += n * (radius - len);
                let into = self.ship.vel.dot(n);
                if into < 0.0 {
                    self.ship.vel -= n * into * 1.3;
                }
            }
        }
    }

    fn update_field(&mut self) {
        let pos = self.ship_position();
        let inside = self.view.fields.iter().copied().find(|&f| {
            (self.position(Place::Field(f)) - pos).length()
                < self.place_radius(Place::Field(f)) + 50e3
        });
        match (inside, &self.field_rocks) {
            (Some(f), Some((current, _))) if f == *current => {}
            (Some(f), _) => self.field_rocks = Some((f, rocks(&self.galaxy.fields[f]))),
            (None, Some(_)) => self.field_rocks = None,
            (None, None) => {}
        }
    }

    /// Move to the first system with a station and dock there (for
    /// testing). Does nothing if there are no stations anywhere.
    pub fn start_at_station(&mut self) {
        let found = self.galaxy.systems.iter().find_map(|(id, _)| {
            let view = SystemView::new(&self.galaxy, id);
            view.stations.first().map(|&s| (id, s))
        });
        if let Some((system, station)) = found {
            self.arrive_jump(system);
            self.message = None;
            self.mode = Mode::Docked(station);
        }
    }

    fn arrive_jump(&mut self, to: SystemId) {
        let from = self.system;
        let days = self
            .lanes()
            .iter()
            .find(|l| l.to == to)
            .map_or(3.0, |l| l.days);
        self.advance_clock(days * SECONDS_PER_DAY);

        self.system = to;
        self.view = SystemView::new(&self.galaxy, to);
        self.system_generation += 1;
        self.field_rocks = None;
        self.target = None;
        // Arrive on the side facing the system we came from, outside the
        // star's well, facing the star.
        let entry = galaxy_direction(&self.galaxy, to, from);
        let distance = (self.well_radius(Place::Star) * 3.0).max(0.3 * AU);
        self.ship.frame = Place::Star;
        self.ship.rel = entry * distance;
        self.ship.rot = look_rotation(-entry);
        self.cam_rot = self.ship.rot;
        self.ship.vel = DVec3::ZERO;
        self.ship.throttle = 0.0;
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
        // A slow orbit on the sunlit side of the object.
        let orbit_view = |center: DVec3, radius: f64| {
            let sunward = (-center).normalize_or(DVec3::Z);
            let swing = (self.clock_s * 0.02).sin() * 0.7;
            let dir = DQuat::from_rotation_y(swing) * (sunward + DVec3::new(0.0, 0.35, 0.0));
            let offset = dir.normalize() * radius;
            (center + offset, look_rotation(-offset))
        };
        match self.mode {
            Mode::Docked(s) => orbit_view(
                self.position(Place::Station(s)),
                self.station_size_m(s) * 1.6,
            ),
            Mode::Landed { body, .. } => {
                let b = Place::Body(body);
                orbit_view(self.position(b), self.place_radius(b) * 3.0)
            }
            _ if self.cockpit => (
                ship + self.ship.rot * DVec3::new(0.0, 1.2, -self.stats().length_m * 0.5 + 1.0),
                self.ship.rot,
            ),
            _ => (
                ship + self.cam_rot * DVec3::new(0.0, 6.0, 28.0) * self.camera_scale(),
                self.cam_rot,
            ),
        }
    }
}

/// Sim AU (system plane x/y) to client metres (system plane x/z, y up).
pub fn to_metres(p: [f64; 3]) -> DVec3 {
    DVec3::new(p[0], p[2], p[1]) * AU
}

pub fn rock_offset(rock: &Rock) -> DVec3 {
    DVec3::new(rock.offset[0], rock.offset[2], rock.offset[1])
}

/// Direction from one system toward another on the galaxy map, mapped into
/// a system's frame.
pub fn galaxy_direction(galaxy: &GalaxyState, here: SystemId, there: SystemId) -> DVec3 {
    let (a, b) = (
        galaxy.systems[here].position,
        galaxy.systems[there].position,
    );
    DVec3::new(b.x - a.x, b.z - a.z, b.y - a.y).normalize_or(DVec3::Z)
}

/// Star radius in solar radii, by class (the sim doesn't model it).
pub fn star_radius_suns(class: StarClass) -> f64 {
    use StarClass::*;
    match class {
        O => 10.0,
        B => 4.0,
        A => 1.7,
        F => 1.3,
        G => 1.0,
        K => 0.8,
        M => 0.4,
        WhiteDwarf => 0.012,
        Neutron => 1.5e-5,
        BlackHole => 4.3e-5,
    }
}

/// Galaxy days a hyperlane jump takes (DESIGN §3: 2–14 days by length).
pub fn jump_days(length_ly: f64) -> f64 {
    (length_ly * 0.6).clamp(2.0, 14.0).round()
}

/// Rotation that points -Z along `dir`, keeping +Y roughly up.
pub fn look_rotation(dir: DVec3) -> DQuat {
    let f = dir.normalize_or(DVec3::NEG_Z);
    let r = f.cross(DVec3::Y).normalize_or(DVec3::X);
    let u = r.cross(f);
    DQuat::from_mat3(&glam::DMat3::from_cols(r, u, -f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(world: &mut World, seconds: f64) {
        let dt = 1.0 / 60.0;
        for _ in 0..(seconds / dt) as usize {
            world.update(dt, Controls::default());
        }
    }

    /// Somewhere outside every gravity well.
    fn open_space(world: &mut World) {
        world.ship.frame = Place::Star;
        world.ship.rel = DVec3::new(0.0, 5.0 * AU, 0.0);
        world.mode = Mode::Normal;
        assert_eq!(world.current_well(), None);
    }

    #[test]
    fn throttle_curve() {
        assert_eq!(throttle_speed(0.0), 0.0);
        assert!((throttle_speed(0.5) - COMBAT_SPEED).abs() < 1e-9);
        assert!((throttle_speed(1.0) - CRUISE_SPEED).abs() < 1e-6);
    }

    #[test]
    fn starts_landed_at_a_capital_city() {
        let world = World::new(1);
        let Mode::Landed { city, .. } = world.mode else {
            panic!("should start landed");
        };
        assert!(world.galaxy.factions.values().any(|f| f.capital == city));
    }

    #[test]
    fn can_leave_a_planets_well_after_launch() {
        let mut world = World::new(1);
        world.act(Action::Launch);
        let well = world
            .current_well()
            .expect("launch puts us in orbit, inside the well");
        let away = world.ship_position() - world.position(well);
        world.ship.rot = look_rotation(away);
        world.ship.throttle = 1.0;
        run(&mut world, 180.0);
        assert_eq!(
            world.current_well(),
            None,
            "still in a well after 3 minutes"
        );
    }

    #[test]
    fn fast_travel_reaches_every_place_and_cannot_start_in_a_well() {
        let mut world = World::new(1);
        world.act(Action::Launch);
        world.target = Some(Place::Star);
        world.act(Action::FastTravel);
        assert_eq!(
            world.mode,
            Mode::Normal,
            "fast travel must not start inside a well"
        );

        for target in world.nav_places().into_iter().filter(|p| *p != Place::Star) {
            open_space(&mut world);
            world.target = Some(target);
            let before = world.clock_s;
            world.act(Action::FastTravel);
            assert!(matches!(world.mode, Mode::FastTravel(_)), "{target:?}");
            run(&mut world, 10.0);
            assert_eq!(world.mode, Mode::Normal, "{target:?}");
            let (d, expected) = (world.distance_to(target), world.arrival_distance(target));
            assert!(
                (d - expected).abs() < expected * 0.05 + 1.0,
                "{}: arrived {d:.3e} m away, expected {expected:.3e}",
                world.name(target)
            );
            assert!(world.clock_s > before, "galaxy time must pass");
        }
    }

    #[test]
    fn buy_and_refit_a_ship_at_the_starting_yard() {
        let mut world = World::new(1);
        let here = world.docked_at().expect("starts landed");
        assert!(hangar::services(&world.galaxy.settlements[here].economy).shipyard);
        // The yard builds hull over time; the cheapest ship is soon in stock.
        let cheapest = hangar::lot().remove(0);
        for _ in 0..60 {
            if hangar::can_build(&world.galaxy.settlements[here].economy, &cheapest) {
                break;
            }
            world.act(Action::Wait(1));
        }
        let credits = world.player.credits;
        world.act(Action::Hangar(Job::BuyShip(cheapest.hull)));
        assert_eq!(world.player.ship.fit.hull().id, cheapest.hull.id);
        assert_eq!(world.player.credits, credits - cheapest.price);
        assert_eq!(world.player.stored.len(), 1);

        // Strip the engine: the ship can't leave until one is fitted again.
        let (mount, slot) = engine_slot(&world);
        let engine = hangar::module_in(&world.player.ship.fit, mount, slot).unwrap();
        world.act(Action::Hangar(Job::Strip { mount, slot }));
        world.act(Action::Launch);
        assert!(matches!(world.mode, Mode::Landed { .. }));
        world.act(Action::Hangar(Job::Fit {
            mount,
            slot,
            module: engine,
        }));
        world.act(Action::Launch);
        assert_eq!(world.mode, Mode::Normal);
    }

    #[test]
    fn handling_follows_the_fit() {
        let mut world = World::new(1);
        assert_eq!(world.handling(), (1.0, 1.0));
        world.player.ship.fit = retrox_content::ships::stock("bulk-freighter").unwrap();
        let (thrust, turn) = world.handling();
        assert!(thrust < 1.0 && turn < 1.0, "{thrust} {turn}");
    }

    fn engine_slot(world: &World) -> (usize, usize) {
        let fit = &world.player.ship.fit;
        fit.mounts()
            .enumerate()
            .find_map(|(m, (_, section))| {
                let s = section?.slots().position(|(slot, module)| {
                    slot.kind == retrox_sim::ship::SlotKind::Engine && module.is_some()
                })?;
                Some((m, s))
            })
            .expect("an engine")
    }

    #[test]
    fn jumping_needs_open_space_and_takes_days() {
        let mut world = World::new(1);
        world.act(Action::Launch);
        let lane = world.lanes()[0].clone();
        world.act(Action::Jump(lane.to));
        assert_eq!(world.mode, Mode::Normal, "can't jump inside a well");

        open_space(&mut world);
        let before = world.date();
        world.act(Action::Jump(lane.to));
        run(&mut world, 5.0);
        assert_eq!(world.system, lane.to);
        assert_eq!(world.current_well(), None, "arrive outside the star's well");
        assert!(world.date().days() >= before.days() + lane.days as u64);
    }
}
