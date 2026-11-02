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
//! 2. *Fast travel*: the sim's [`flight`] autopilot flies the ship to a
//!    nav target while time runs faster, only from outside every gravity
//!    well. Same physics as every other ship, so the trip takes as long as
//!    the flight does. You arrive at the edge of the target's well (or near
//!    a station or field); press J again to drop out anywhere.
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
use crate::mining::{self, Mining};
use retrox_gen::{GalaxyConfig, Rock, generate, rocks};
use retrox_sim::economy::Good;
use retrox_sim::flight::{self, Goal, Kinematics};
use retrox_sim::{
    BodyId, BodyKind, FieldId, GalaxyState, GalaxyTime, SettlementId, Site, StarClass, SystemId,
    tick,
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
/// The ship's main drive under autopilot, m/s²: about a day per AU.
const DRIVE_ACCEL: f64 = ACCEL;
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
    /// The ship in the star's frame, flown by the autopilot.
    pub ship: Kinematics,
    /// Galaxy clock at departure.
    pub depart_s: f64,
    /// Galaxy seconds per real second.
    pub warp: f64,
    /// Real seconds since departure.
    pub elapsed: f32,
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
    /// The mining laser's trigger is held.
    pub fire: bool,
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
    /// Open or close the galaxy map.
    ToggleMap,
    /// Plot a course along the hyperlanes to a system, or clear it.
    PlotCourse(Option<SystemId>),
    /// A job in the hangar of the settlement we're docked or landed at.
    Hangar(Job),
    /// Go to one of the docked screens.
    Open(Screen),
    /// Put a stock ship on the shipyard floor to look at.
    Inspect(&'static retrox_sim::ship::Hull),
    /// Buy (positive) or sell (negative) tonnes of a good.
    Trade(Good, f64),
    /// Ping the prospector: read the rocks around the ship.
    Scan,
}

/// The screens of a station or city, while docked or landed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    /// The menu: where you are, and where to go.
    #[default]
    Hub,
    Trade,
    /// Your ship in the bay: refits and stored ships.
    Hangar,
    /// Ships for sale, on the bay floor.
    Shipyard,
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
    /// The mining laser and what it has done to every field.
    pub mining: Mining,
    /// The mining laser's beam this frame, as offsets from the ship's
    /// position: muzzle to where it stops.
    pub beam: Option<(DVec3, DVec3)>,
    pub message: Option<(String, f32)>,
    /// The player's money and ships.
    pub player: Player,
    /// Which docked screen is showing.
    pub screen: Screen,
    /// The stock ship being looked at in the shipyard.
    pub inspect: Option<&'static retrox_sim::ship::Hull>,
    /// Bumped whenever the system changes, so the client re-uploads meshes.
    pub system_generation: u64,
    /// The galaxy map is open.
    pub map_open: bool,
    /// The system the plotted course leads to, if any.
    pub course: Option<SystemId>,
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
            mining: Mining::default(),
            beam: None,
            message: None,
            player: Player::new(),
            screen: Screen::Hub,
            inspect: None,
            system_generation: 0,
            map_open: false,
            course: None,
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
        self.position_at(place, self.t_days())
    }

    /// Position of a place at galaxy day `t`.
    pub fn position_at(&self, place: Place, t: f64) -> DVec3 {
        match place {
            Place::Star => DVec3::ZERO,
            Place::Body(b) => to_metres(self.galaxy.body_position(b, t)),
            Place::Field(f) => to_metres(self.galaxy.orbit_position(
                self.system,
                &self.galaxy.fields[f].orbit,
                t,
            )),
            Place::Station(s) => match self.galaxy.settlements.get(s).map(|s| s.site) {
                Some(Site::Space(orbit)) => {
                    to_metres(self.galaxy.orbit_position(self.system, &orbit, t))
                }
                _ => DVec3::ZERO,
            },
        }
    }

    /// Velocity of a place at galaxy day `t`, m/s.
    pub fn velocity_at(&self, place: Place, t: f64) -> DVec3 {
        const H_DAYS: f64 = 1e-3;
        (self.position_at(place, t + H_DAYS) - self.position_at(place, t - H_DAYS))
            / (2.0 * H_DAYS * SECONDS_PER_DAY)
    }

    /// Where the autopilot brings the ship to rest for `place` at galaxy
    /// day `t`: on the arrival sphere, on the ship's side of it.
    fn fast_travel_goal(&self, place: Place, ship: DVec3, t: f64) -> Goal {
        let centre = self.position_at(place, t);
        let side = (ship - centre).normalize_or(DVec3::Z);
        let pos = centre + side * self.arrival_distance(place);
        Goal {
            pos: pos.to_array(),
            vel: self.velocity_at(place, t).to_array(),
        }
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

    /// The quickest way along the hyperlanes from here to `to` (by days in
    /// transit): every system on the way, starting here and ending at `to`.
    pub fn route(&self, to: SystemId) -> Option<Vec<SystemId>> {
        use std::cmp::Reverse;
        use std::collections::{BinaryHeap, HashMap};
        let from = self.system;
        let mut best: HashMap<SystemId, (f64, SystemId)> = HashMap::from([(from, (0.0, from))]);
        let mut heap = BinaryHeap::from([Reverse((0.0f64.to_bits(), from))]);
        while let Some(Reverse((bits, system))) = heap.pop() {
            let days = f64::from_bits(bits);
            if best.get(&system).is_some_and(|&(b, _)| b < days) {
                continue;
            }
            if system == to {
                let mut path = vec![to];
                while let Some(&last) = path.last()
                    && last != from
                {
                    path.push(best[&last].1);
                }
                path.reverse();
                return Some(path);
            }
            for &lane in &self.galaxy.systems[system].lanes {
                let lane = &self.galaxy.lanes[lane];
                let next = lane.other(system);
                let d = days + jump_days(lane.length_ly);
                if best.get(&next).is_none_or(|&(b, _)| d < b) {
                    best.insert(next, (d, system));
                    heap.push(Reverse((d.to_bits(), next)));
                }
            }
        }
        None
    }

    /// The galaxy map can be opened: flying normally, docked, or landed.
    pub fn map_available(&self) -> bool {
        match self.mode {
            Mode::Normal | Mode::Docked(_) => true,
            Mode::Landed { timer, .. } => timer <= 0.0,
            _ => false,
        }
    }

    /// The next system on the plotted course.
    pub fn next_hop(&self) -> Option<SystemId> {
        self.route(self.course?)?.get(1).copied()
    }

    /// Total days in transit along a route (hops only, not flying to the
    /// edge of each system's well).
    pub fn route_days(&self, route: &[SystemId]) -> f64 {
        route
            .windows(2)
            .map(|hop| {
                self.galaxy.systems[hop[0]]
                    .lanes
                    .iter()
                    .map(|&l| &self.galaxy.lanes[l])
                    .find(|l| l.other(hop[0]) == hop[1])
                    .map_or(0.0, |l| jump_days(l.length_ly))
            })
            .sum()
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
            Action::ToggleMap => self.map_open = !self.map_open && self.map_available(),
            Action::PlotCourse(to) => {
                self.course = to.filter(|&t| t != self.system && self.route(t).is_some());
            }
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
                (Mode::FastTravel(trip), _, _) => self.drop_out(trip),
                _ => {}
            },
            Action::Scan => self.scan(),
            Action::Dock => {
                if let Some(s) = prompts.dock {
                    self.mining.haul = Default::default();
                    self.mode = Mode::Docked(s);
                    self.screen = Screen::Hub;
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
                    self.screen = Screen::Hub;
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
                    self.map_open = false;
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
            Action::Open(screen) => {
                if self.docked_at().is_some() {
                    self.screen = screen;
                }
            }
            Action::Inspect(hull) => self.inspect = Some(hull),
            Action::Trade(good, amount) => {
                let Some(here) = self.docked_at() else {
                    return;
                };
                let capacity = self.stats().cargo_t;
                let settlement = &mut self.galaxy.settlements[here];
                let model = self.galaxy.factions[settlement.owner].economy;
                let result = crate::trade::trade(
                    &mut self.player,
                    capacity,
                    &mut settlement.economy,
                    model,
                    good,
                    amount,
                );
                match result {
                    Ok(text) | Err(text) => self.say(text),
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

    /// The fast-travel drive's acceleration for this ship.
    fn drive_accel(&self) -> f64 {
        DRIVE_ACCEL * self.handling().0
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
        let t = self.t_days();
        let pos = self.ship_position();
        let ship = Kinematics {
            pos: pos.to_array(),
            vel: (self.ship.vel + self.velocity_at(self.ship.frame, t)).to_array(),
        };
        // Pick how fast time runs so the flight plays out in a few seconds.
        // Time it by flying it once ahead: a ship already moving fast (say,
        // just dropped out of another trip) must brake and come back, which
        // can take far longer than the distance alone suggests.
        let distance = (self.position(target) - pos).length();
        let accel = self.drive_accel();
        let mut rehearsal = ship;
        let estimate = flight::fly(&mut rehearsal, accel, 1e10, |s, k| {
            self.fast_travel_goal(target, DVec3::from_array(k.pos), t + s / SECONDS_PER_DAY)
        })
        .unwrap_or_else(|| flight::trip_seconds(distance, accel));
        let real = (2.0 + (distance / 1e7).max(1.0).log10() * 1.2).min(7.0);
        self.ship.spin = DVec3::ZERO;
        self.ship.throttle = 0.0;
        self.mode = Mode::FastTravel(FastTravel {
            target,
            ship,
            depart_s: self.clock_s,
            warp: estimate / real,
            elapsed: 0.0,
        });
    }

    /// One frame of fast travel: fly `dt` real seconds of sped-up flight.
    fn fast_travel(&mut self, mut trip: FastTravel, dt: f64) {
        trip.elapsed += dt as f32;
        let t0 = self.t_days();
        let target = trip.target;
        let accel = self.drive_accel();
        let arrived = flight::fly(&mut trip.ship, accel, dt * trip.warp, |t, ship| {
            let pos = DVec3::from_array(ship.pos);
            self.fast_travel_goal(target, pos, t0 + t / SECONDS_PER_DAY)
        });
        // Galaxy time passes by exactly as long as the ship flew.
        self.advance_clock(arrived.unwrap_or(dt * trip.warp));
        let pos = DVec3::from_array(trip.ship.pos);
        let vel = DVec3::from_array(trip.ship.vel);
        // Face the way the drive is pushing: out along the route, then
        // flipped round to brake.
        let goal = self.fast_travel_goal(target, pos, self.t_days());
        let thrust = DVec3::from_array(flight::autopilot(&trip.ship, &goal, accel, 1.0));
        if thrust.length() > accel * 0.1 {
            let k = 1.0 - (-4.0 * dt).exp();
            self.ship.rot = self.ship.rot.slerp(look_rotation(thrust), k).normalize();
        }
        self.ship.frame = Place::Star;
        self.ship.rel = pos;
        self.ship.vel = vel;
        match arrived {
            Some(_) => self.arrive_fast_travel(target),
            None => self.mode = Mode::FastTravel(trip),
        }
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
            Mode::FastTravel(trip) => self.fast_travel(trip, dt),
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
        self.mine(dt, controls.fire);
        self.update_camera(dt);
    }

    /// Switch the ship into `frame`, keeping where it is and how it moves.
    fn enter_frame(&mut self, frame: Place) {
        let t = self.t_days();
        let pos = self.ship_position();
        let vel = self.ship.vel + self.velocity_at(self.ship.frame, t);
        self.ship.frame = frame;
        self.ship.rel = pos - self.position(frame);
        self.ship.vel = vel - self.velocity_at(frame, t);
    }

    /// Stop the autopilot mid-trip: the ship carries on from wherever it
    /// is, as fast as it's going.
    fn drop_out(&mut self, _trip: FastTravel) {
        self.mode = Mode::Normal;
        let speed = self.ship.vel.length() / 1000.0;
        self.say(format!("Dropped out of fast travel at {speed:.0} km/s"));
    }

    fn arrive_fast_travel(&mut self, target: Place) {
        // At rest relative to the target, within a few m/s: carry on from
        // exactly there.
        self.enter_frame(target);
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
            for (i, rock) in rocks.iter().enumerate() {
                if !self.mining.is_broken(*field, i) {
                    solids.push((center + rock_offset(rock), rock.radius_m + 8.0));
                }
            }
            for (pos, radius) in self.mining.frag_solids(*field) {
                solids.push((center + pos, radius + 4.0));
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
            (None, Some((left, _))) => {
                self.mining.leave(*left);
                self.field_rocks = None;
            }
            (None, None) => {}
        }
    }

    /// Where the mining laser's muzzle is, in ship space (metres, nose
    /// toward -Z): the first fitted mining laser's slot, else the nose.
    pub fn laser_muzzle(&self) -> DVec3 {
        use retrox_sim::ship::Function;
        self.player
            .ship
            .fit
            .mounts()
            .filter_map(|(_, s)| s)
            .flat_map(|s| s.slots())
            .find_map(|(slot, module)| {
                matches!(module?.function, Function::MiningLaser { .. }).then(|| {
                    let [x, y, z] = slot.position_m;
                    DVec3::new(x, y, -z)
                })
            })
            .unwrap_or(DVec3::new(0.0, 0.0, -self.stats().length_m / 2.0))
    }

    /// One frame of the mining laser and the tractor.
    fn mine(&mut self, dt: f64, fire: bool) {
        let stats = self.stats();
        self.beam = None;
        let Some((field, rocks)) = &self.field_rocks else {
            if let Some(text) = self
                .mining
                .idle(dt, &stats, fire && self.mode == Mode::Normal)
            {
                self.say(text);
            }
            if self.mining.firing {
                let muzzle = self.ship.rot * self.laser_muzzle();
                let forward = self.ship.rot * DVec3::NEG_Z;
                self.beam = Some((muzzle, muzzle + forward * stats.mining_range_m));
            }
            return;
        };
        let field = *field;
        let centre = self.position(Place::Field(field));
        let ship = self.ship_position() - centre;
        let muzzle = self.ship.rot * self.laser_muzzle();
        let forward = self.ship.rot * DVec3::NEG_Z;
        let miner = mining::Miner {
            nose: ship + muzzle,
            forward,
            centre: ship,
            reach: stats.length_m * 0.6 + 6.0,
            free_t: (stats.cargo_t - self.player.cargo_t()).max(0.0),
            firing: fire && self.mode == Mode::Normal,
        };
        let out = self.mining.update(
            dt,
            field,
            &self.galaxy.fields[field],
            rocks,
            self.clock_s,
            &stats,
            miner,
        );
        for (good, t) in out.collected {
            self.player.cargo[good] += t;
        }
        self.ship.vel += out.kick;
        if let Some(text) = out.messages.into_iter().last() {
            self.say(text);
        }
        if self.mining.firing {
            let end = match self.mining.aim {
                Some(aim) if aim.distance <= stats.mining_range_m => aim.point - ship,
                _ => muzzle + forward * stats.mining_range_m,
            };
            self.beam = Some((muzzle, end));
        }
    }

    fn scan(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        let Some((field, rocks)) = &self.field_rocks else {
            self.say("Nothing to prospect: fly into an asteroid field");
            return;
        };
        let centre = self.position(Place::Field(*field));
        let from = self.ship_position() - centre;
        let found = self.mining.scan(
            *field,
            &self.galaxy.fields[*field],
            rocks,
            self.clock_s,
            from,
        );
        if let Some(n) = found {
            self.say(format!("Prospector: {n} rocks read"));
        }
    }

    /// The best price a tonne of `good` sells for in this system, and
    /// where.
    pub fn best_local_price(&self, good: Good) -> Option<(f64, SettlementId)> {
        self.view
            .stations
            .iter()
            .copied()
            .chain(self.view.cities.iter().map(|&(c, _)| c))
            .filter_map(|s| {
                let settlement = self.galaxy.settlements.get(s)?;
                let model = self.galaxy.factions.get(settlement.owner)?.economy;
                let price = crate::trade::sell_value(&settlement.economy.market, model, good, 1.0);
                Some((price, s))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Park in the first asteroid field of this system (or the nearest
    /// system with one), facing a mid-sized rock a few hundred metres off
    /// (for testing). Returns whether it found one.
    pub fn start_at_field(&mut self) -> bool {
        let here = SystemView::new(&self.galaxy, self.system);
        if here.fields.is_empty() {
            let found = self.galaxy.systems.iter().find_map(|(id, _)| {
                (!SystemView::new(&self.galaxy, id).fields.is_empty()).then_some(id)
            });
            let Some(system) = found else {
                return false;
            };
            self.arrive_jump(system);
        }
        let field = self.view.fields[0];
        let rocks = rocks(&self.galaxy.fields[field]);
        let Some(rock) = rocks
            .iter()
            .filter(|r| (12.0..40.0).contains(&r.radius_m))
            .min_by(|a, b| rock_offset(a).length().total_cmp(&rock_offset(b).length()))
        else {
            return false;
        };
        let at = rock_offset(rock);
        let back = (at.normalize_or(DVec3::Z) + DVec3::new(0.0, 0.3, 0.0)).normalize();
        self.mode = Mode::Normal;
        self.ship.frame = Place::Field(field);
        self.ship.rel = at + back * (rock.radius_m * 3.0 + 150.0);
        self.ship.vel = DVec3::ZERO;
        self.ship.throttle = 0.0;
        self.ship.rot = look_rotation(at - self.ship.rel);
        self.cam_rot = self.ship.rot;
        self.target = Some(Place::Field(field));
        self.message = None;
        self.update_field();
        true
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
        if let Some((left, _)) = self.field_rocks.take() {
            self.mining.leave(left);
        }
        self.target = None;
        if self.course == Some(to) {
            self.course = None;
        }
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
            run(&mut world, 30.0);
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
    fn trading_from_the_station_menu() {
        let mut world = World::new(1);
        assert_eq!(world.screen, Screen::Hub);
        world.act(Action::Open(Screen::Trade));
        assert_eq!(world.screen, Screen::Trade);
        let credits = world.player.credits;
        world.act(Action::Trade(Good::Food, 5.0));
        assert_eq!(world.player.cargo[Good::Food], 5.0);
        assert!(world.player.credits < credits);
        // More than the hold takes is refused.
        world.act(Action::Trade(Good::Food, 1000.0));
        assert_eq!(world.player.cargo[Good::Food], 5.0);
        world.act(Action::Open(Screen::Hub));
        world.act(Action::Launch);
        // Screens only open while docked or landed.
        world.act(Action::Open(Screen::Trade));
        assert_eq!(world.mode, Mode::Normal);
        assert_eq!(world.docked_at(), None);
    }

    #[test]
    fn handling_follows_the_fit() {
        let mut world = World::new(1);
        assert_eq!(world.handling(), (1.0, 1.0));
        world.player.ship.fit = retrox_content::ships::stock("bulk-freighter").unwrap();
        let (thrust, turn) = world.handling();
        assert!(thrust < 1.0 && turn < 1.0, "{thrust} {turn}");
    }

    #[test]
    fn mine_a_rock_and_sell_the_ore() {
        let mut world = World::new(1);
        assert!(
            world.stats().mining_cut > 0.0,
            "the courier comes with a laser"
        );
        assert!(world.start_at_field());
        world.act(Action::Scan);
        let (field, _) = world.field_rocks.clone().expect("in a field");
        let mine = Controls {
            fire: true,
            ..Default::default()
        };
        let dt = 1.0 / 60.0;
        // Fire until something fractures, letting the laser vent.
        let mut frames = 0;
        while world
            .mining
            .field(field)
            .is_none_or(|s| s.nuggets.is_empty())
        {
            frames += 1;
            assert!(frames < 60 * 180, "nothing fractured in three minutes");
            world.update(dt, mine);
        }
        // Sweep up: park by each loose chunk in turn and let the tractor
        // pull it in.
        for _ in 0..200 {
            let Some(next) = world
                .mining
                .field(field)
                .and_then(|s| s.nuggets.first())
                .map(|n| n.pos)
            else {
                break;
            };
            world.ship.frame = Place::Field(field);
            world.ship.rel = next + DVec3::Y * 20.0;
            world.ship.vel = DVec3::ZERO;
            run(&mut world, 3.0);
        }
        let mined = world.player.cargo[Good::Ore] + world.player.cargo[Good::Ice];
        assert!(mined > 0.1, "hold has {mined} t");
        assert!(world.player.cargo_t() <= world.stats().cargo_t + 1e-9);

        // Sell it at a station.
        world.start_at_station();
        let credits = world.player.credits;
        for good in [Good::Ore, Good::Ice] {
            let t = world.player.cargo[good].floor();
            if t >= 1.0 {
                world.act(Action::Trade(good, -t));
            }
        }
        if mined >= 1.0 {
            assert!(world.player.credits > credits);
        }
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
    fn fast_travel_is_a_real_flight() {
        let mut world = World::new(1);
        world.act(Action::Launch);
        for target in world.nav_places().into_iter().filter(|p| *p != Place::Star) {
            open_space(&mut world);
            world.target = Some(target);
            world.act(Action::FastTravel);
            let name = world.name(target);
            let dt = 1.0 / 60.0;
            for frame in 0.. {
                assert!(frame < 60 * 60, "{name}: still flying after a minute");
                let (pos, vel, clock) = (world.ship_position(), world.ship.vel, world.clock_s);
                world.update(dt, Controls::default());
                // Moved only as far as its speed allows in the galaxy time
                // that passed: no teleporting.
                let moved = (world.ship_position() - pos).length();
                let flown = world.clock_s - clock;
                let reach = vel.length().max(world.ship.vel.length()) * flown;
                if matches!(world.mode, Mode::FastTravel(_)) {
                    assert!(
                        moved <= reach * 1.01 + 1.0,
                        "{name}: jumped {moved:.3e} m in a {flown:.0} s step"
                    );
                } else {
                    break;
                }
            }
            // At rest next to the target, carrying on in its frame.
            assert_eq!(world.ship.frame, target);
            assert!(
                world.ship.vel.length() <= flight::ARRIVE_SPEED + 1.0,
                "{name}"
            );
        }
    }

    #[test]
    fn dropping_out_keeps_position_and_speed() {
        let mut world = World::new(1);
        world.act(Action::Launch);
        open_space(&mut world);
        let target = world.view.bodies[0];
        world.target = Some(Place::Body(target));
        world.act(Action::FastTravel);
        run(&mut world, 1.0);
        let (pos, vel) = (world.ship_position(), world.ship.vel);
        assert!(
            vel.length() > COMBAT_SPEED,
            "should be flying fast mid-trip"
        );
        world.act(Action::FastTravel);
        assert_eq!(world.mode, Mode::Normal);
        assert_eq!((world.ship_position(), world.ship.vel), (pos, vel));
    }

    #[test]
    fn fast_travel_from_a_fast_drop_out_still_takes_seconds() {
        // Dropping out of a trip leaves the ship near its target at
        // thousands of km/s. Going again must still play out in seconds,
        // braking included, not crawl at the warp a short hop would get.
        let mut world = World::new(1);
        world.act(Action::Launch);
        let field = world
            .nav_places()
            .into_iter()
            .find(|p| matches!(p, Place::Field(_)))
            .expect("a field");
        world.ship.frame = field;
        world.ship.rel = DVec3::X * 1e6;
        world.ship.vel = DVec3::new(0.7, 0.7, 0.0) * 1e6;
        world.mode = Mode::Normal;
        world.target = Some(field);
        world.act(Action::FastTravel);
        assert!(matches!(world.mode, Mode::FastTravel(_)));
        run(&mut world, 10.0);
        assert_eq!(world.mode, Mode::Normal, "still flying after 10 s");
        assert_eq!(world.ship.frame, field);
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

    #[test]
    fn a_plotted_course_is_followed_hop_by_hop() {
        let mut world = World::new(1);
        // Somewhere a few jumps away.
        let far = world
            .galaxy
            .systems
            .iter()
            .map(|(s, _)| s)
            .filter_map(|s| world.route(s).map(|r| (s, r)))
            .find(|(_, r)| r.len() >= 4)
            .map(|(s, _)| s)
            .expect("a system several jumps away");
        world.act(Action::PlotCourse(Some(far)));
        assert_eq!(world.course, Some(far));
        let route = world.route(far).unwrap();
        assert_eq!(route.first(), Some(&world.system));
        assert_eq!(route.last(), Some(&far));
        assert!(world.route_days(&route) > 0.0);

        world.act(Action::Launch);
        world.act(Action::ToggleMap);
        assert!(world.map_open);
        for &expected in &route[1..] {
            open_space(&mut world);
            let hop = world.next_hop().expect("course has a next hop");
            assert_eq!(hop, expected);
            world.act(Action::Jump(hop));
            assert!(!world.map_open, "jumping closes the map");
            run(&mut world, 5.0);
            assert_eq!(world.system, hop);
        }
        assert_eq!(world.course, None, "arriving clears the course");
        world.act(Action::PlotCourse(Some(world.system)));
        assert_eq!(world.course, None, "no course to where you are");
    }
}
