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

mod field;
mod pilot;
mod routing;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use retrox_sim::ship::ShipStats;
use serde::{Deserialize, Serialize};

use crate::contracts;
use crate::hangar::{self, Job, Player};
use crate::mining::Mining;
use retrox_gen::{GalaxyConfig, Rock, generate, rocks};
use retrox_sim::economy::Good;
use retrox_sim::flight::Kinematics;
use retrox_sim::freight::{self, Offer};
use retrox_sim::{
    BodyId, BodyKind, FieldId, GalaxyState, GalaxyTime, SettlementId, Site, StarClass, SystemId,
    tick,
};

pub const AU: f64 = 1.495_978_707e11;
pub const SECONDS_PER_DAY: f64 = 86_400.0;
const SOLAR_RADIUS_M: f64 = 6.957e8;

/// Gravity wells extend this many radii from the centre of a star or body.
pub const WELL_RADII: f64 = 4.0;
pub use pilot::{COMBAT_SPEED, look_rotation};

/// Real seconds the hyperlane sequence plays for.
pub const JUMP_DURATION: f32 = 4.0;
const LANDING_DURATION: f32 = 3.0;
/// How long a frame may spend ticking the sim through a wait, so a long
/// wait plays out over several frames instead of freezing one.
const WAIT_BUDGET: Duration = Duration::from_millis(12);
/// Days the galaxy runs before the player starts (see [`World::new`]).
const WARM_UP_DAYS: f64 = 30.0;
/// The ship the flight model is tuned for: the starting ship, stock.
static REFERENCE: LazyLock<ShipStats> = LazyLock::new(|| {
    retrox_content::ships::stock(hangar::STARTING_HULL)
        .expect("starting hull exists")
        .stats()
});

/// Something in the current system you can navigate to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Wait docked or landed for up to this many days. The days pass over
    /// the next frames (see [`World::waiting`]).
    Wait(u64),
    /// Stop a wait early.
    StopWaiting,
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
    /// Take a contract off the dispatch board (by index into
    /// [`World::board`]).
    AcceptContract(usize),
    /// Give up one of the player's contracts (by index).
    AbandonContract(usize),
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
    /// The dispatch board: freight contracts.
    Contracts,
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
    /// The dispatch board where we're docked or landed.
    pub board: Vec<Offer>,
    /// What the board was drawn up for: settlement, day, and the player's
    /// hold and contracts then.
    board_key: Option<(SettlementId, u64, u64, usize)>,
    /// A wait in progress, if any.
    pub waiting: Option<Waiting>,
    /// The galaxy clock when the current hyperlane jump began: the jump's
    /// days pass over its sequence, not all in its last frame.
    jump_start_s: Option<f64>,
}

/// A wait in progress (DESIGN §3): docked or landed only, a day at a time,
/// stopping on the day a contract falls due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// Days asked for.
    pub days: u64,
    /// Days passed so far.
    pub waited: u64,
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
        let clock_s = galaxy.time.days() as f64 * SECONDS_PER_DAY;
        let ship = Ship {
            frame: Place::Body(body),
            rel: DVec3::ZERO,
            vel: DVec3::ZERO,
            rot: DQuat::IDENTITY,
            spin: DVec3::ZERO,
            throttle: 0.0,
            flight_assist: true,
        };
        let mode = Mode::Landed {
            city,
            body,
            timer: 0.0,
        };
        let mut world = Self::from_parts(galaxy, system, clock_s, ship, mode, Player::new());
        // Generation leaves every market exactly on target. Let the galaxy
        // run for a while first, so the player arrives in a going concern:
        // traders under way, some markets short, others with goods to spare.
        world.advance_clock(WARM_UP_DAYS * SECONDS_PER_DAY);
        world.refresh_board();
        world
    }

    /// A world from its saved parts; everything else (the system's index,
    /// the camera, asteroid rocks) is rebuilt from them.
    pub fn from_parts(
        galaxy: GalaxyState,
        system: SystemId,
        clock_s: f64,
        ship: Ship,
        mode: Mode,
        player: Player,
    ) -> Self {
        let view = SystemView::new(&galaxy, system);
        let cam_rot = ship.rot;
        let mut world = Self {
            galaxy,
            system,
            view,
            clock_s,
            ship,
            mode,
            target: None,
            cockpit: false,
            cam_rot,
            field_rocks: None,
            mining: Mining::default(),
            beam: None,
            message: None,
            player,
            screen: Screen::Hub,
            inspect: None,
            system_generation: 0,
            map_open: false,
            course: None,
            board: Vec::new(),
            board_key: None,
            waiting: None,
            jump_start_s: None,
        };
        world.update_field();
        world.refresh_board();
        world
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
                    self.jump_start_s = Some(self.clock_s);
                    self.mode = Mode::Jumping {
                        to,
                        timer: JUMP_DURATION,
                    };
                }
            }
            Action::Wait(days) => {
                if days > 0 && self.waiting.is_none() && self.docked_at().is_some() {
                    self.waiting = Some(Waiting { days, waited: 0 });
                }
            }
            Action::StopWaiting => {
                if let Some(w) = self.waiting.take() {
                    self.end_wait(w, None);
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
            Action::AcceptContract(i) => {
                let Some(offer) = self.board.get(i).cloned() else {
                    return;
                };
                if self.docked_at() != Some(offer.from) {
                    return;
                }
                let capacity = self.stats().cargo_t;
                match contracts::accept(&mut self.player, capacity, &mut self.galaxy, &offer) {
                    Ok(text) => {
                        self.board.remove(i);
                        self.say(text);
                    }
                    Err(text) => self.say(text),
                }
            }
            Action::AbandonContract(i) => {
                if let Some(text) = contracts::abandon(&mut self.player, &mut self.galaxy, i) {
                    self.say(text);
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

    /// Pass as many days of a wait as fit in this frame's budget.
    fn wait_some(&mut self) {
        let started = Instant::now();
        while let Some(mut w) = self.waiting {
            if self.docked_at().is_none() {
                // Waiting is docked-only (DESIGN §3): leaving ends it.
                self.waiting = None;
                self.end_wait(w, None);
                return;
            }
            self.advance_clock(SECONDS_PER_DAY);
            w.waited += 1;
            let today = self.galaxy.time;
            let due = self
                .player
                .contracts
                .iter()
                .find(|c| c.offer.deadline == today || c.fails_on() == today)
                .map(|c| self.galaxy.settlements[c.offer.to].name.clone());
            if due.is_some() || w.waited >= w.days {
                self.waiting = None;
                self.end_wait(w, due);
                return;
            }
            self.waiting = Some(w);
            if started.elapsed() >= WAIT_BUDGET {
                return;
            }
        }
    }

    /// Say how a wait ended, and settle contracts it made due.
    fn end_wait(&mut self, w: Waiting, due: Option<String>) {
        let waited = w.waited;
        let plural = if waited == 1 { "" } else { "s" };
        match due {
            Some(to) if waited < w.days => self.say(format!(
                "Stopped waiting after {waited} day{plural}: a contract to {to} is due"
            )),
            _ => self.say(format!("Waited {waited} day{plural}")),
        }
        self.settle_contracts();
    }

    /// Deliver contracts bound for where we're docked or landed, and fail
    /// the ones past their grace period.
    fn settle_contracts(&mut self) {
        if self.player.contracts.is_empty() {
            return;
        }
        let mut said = contracts::expire(&mut self.player, &mut self.galaxy);
        if let Some(here) = self.docked_at() {
            said.extend(contracts::deliver(&mut self.player, &mut self.galaxy, here));
        }
        if !said.is_empty() {
            self.say(said.join(" · "));
        }
    }

    /// Draw up the dispatch board again if anything it depends on changed:
    /// where we are, the day, the hold, or the player's contracts.
    fn refresh_board(&mut self) {
        let Some(here) = self.docked_at() else {
            self.board_key = None;
            self.board.clear();
            return;
        };
        let lot = self.stats().cargo_t;
        let key = (
            here,
            self.galaxy.time.days(),
            lot.to_bits(),
            self.player.contracts.len(),
        );
        if self.board_key == Some(key) {
            return;
        }
        self.board_key = Some(key);
        let pending = contracts::pending(&self.player);
        self.board = freight::board(&self.galaxy, here, lot, |ly| self.jump_days(ly), &pending);
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
                    self.jump_progress(to, 1.0 - f64::from(timer / JUMP_DURATION));
                }
            }
        }
        self.wait_some();

        self.settle_contracts();
        self.refresh_board();
        self.update_field();
        self.mine(dt, controls.fire);
        self.update_camera(dt);
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
