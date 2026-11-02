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
//!    nav target while time runs faster. Same physics as every other ship,
//!    so the trip takes as long as the flight does. You arrive at the edge
//!    of the target's well, low over a planet with a city on it, or near a
//!    station or field; press J again to drop out anywhere. Inside a
//!    gravity well the autopilot only flies within it: down to what's in
//!    it, or up and out to its edge (a *climb*), where the ship stops
//!    (DESIGN §7.1.1).
//! 3. *Hyperlane travel*: to a neighbouring system, also only from outside
//!    gravity wells. Days pass.
//!
//! **Units.** The sim uses AU in a frame with the system plane as x/y and
//! days. The client uses metres with y up (system plane x/z) and seconds.
//!
//! **Frames.** The ship's position is stored relative to a *frame*: the
//! place it arrived at or launched from. The frame moves with that place,
//! so nearby things hold still while you fly.

#[cfg(test)]
mod careers;
mod convoy;
mod field;
mod fight;
mod pilot;
mod prowl;
mod routing;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use retrox_sim::ship::ShipStats;
use serde::{Deserialize, Serialize};

use crate::combat::Combat;
use crate::contracts;
use crate::hangar::{self, Job, Player};
use crate::mining::Mining;
use crate::traffic::Traffic;
use retrox_gen::{GalaxyConfig, Rock, generate, rocks};
use retrox_sim::bubble;
use retrox_sim::economy::Good;
use retrox_sim::flight::Kinematics;
use retrox_sim::freight::{self, Offer};
use retrox_sim::{
    BodyId, BodyKind, FieldId, GalaxyState, GalaxyTime, SettlementId, Site, StarClass, SystemId,
    tick,
};

pub const AU: f64 = retrox_sim::bubble::METRES_PER_AU;
pub const SECONDS_PER_DAY: f64 = 86_400.0;
const SOLAR_RADIUS_M: f64 = 6.957e8;

/// Gravity wells extend this many radii from the centre of a star or body.
pub const WELL_RADII: f64 = 4.0;
/// A city can be landed at from within this many radii of its planet.
pub const LANDING_RADII: f64 = 2.0;
/// Fast travel to a planet with a city on it stops this many radii out:
/// low enough to land straight away.
pub const APPROACH_RADII: f64 = 1.6;
pub use pilot::{COMBAT_SPEED, STRAFE_SPEED, assist, handling, look_rotation, steer};

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
    /// A trader fleet flying in the system: a frame that moves with it,
    /// for flying alongside after an intercept (DESIGN §7.2.2).
    Convoy(retrox_sim::bubble::FleetKey),
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

/// Where a fast-travel leg goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leg {
    /// To a place, stopping at its [`World::arrival_distance`].
    To(Place),
    /// Up and out of a gravity well, stopping just past its edge: on the
    /// side facing `toward` if that's somewhere outside, else straight up.
    Climb { well: Place, toward: Option<Place> },
    /// Alongside a trader fleet, matching its course (DESIGN §7.2.2).
    Intercept(retrox_sim::bubble::FleetKey),
}

impl Leg {
    /// The place the ship comes to rest by, and carries on in the frame of.
    pub fn frame(self) -> Place {
        match self {
            Leg::To(place) | Leg::Climb { well: place, .. } => place,
            Leg::Intercept(fleet) => Place::Convoy(fleet),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FastTravel {
    pub leg: Leg,
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
    /// Fast travel alongside the trader fleet nearest the crosshair.
    Intercept,
    /// Point the guns at the next hostile.
    CycleHostile,
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
    /// Warrants on pirates nearby, and standing with the factions.
    Bounties,
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
    /// The well [J] would climb out of, when there's nowhere in it to
    /// fast travel to.
    pub climb: Option<Place>,
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
    /// The gas scoop, near a gas giant (issue #68).
    pub skim: crate::skim::Skim,
    /// The mining laser's beam this frame, as offsets from the ship's
    /// position: muzzle to where it stops.
    pub beam: Option<(DVec3, DVec3)>,
    pub message: Option<(String, f32)>,
    /// How hard the ship hit something this frame, m/s into the surface.
    pub bump: f64,
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
    /// The bubble's fleets flying in this system.
    pub traffic: Traffic,
    /// A wait in progress, if any.
    pub waiting: Option<Waiting>,
    /// The galaxy clock when the current hyperlane jump began: the jump's
    /// days pass over its sequence, not all in its last frame.
    jump_start_s: Option<f64>,
    /// Guns, shields and hulls, and whoever is fighting near the ship.
    pub combat: Combat,
    /// How hard the ship's engines burned this frame, 0..1.
    pub burn: f64,
    /// Seconds since a gang pulled us out of fast travel, and which,
    /// while that's news.
    pub interdicted: Option<(f64, String)>,
    /// The convoy the ship flies alongside, as last seen: where, how fast,
    /// and the galaxy clock then.
    convoy_seen: Option<(retrox_sim::bubble::FleetKey, DVec3, DVec3, f64)>,
    /// When each trader fleet last took offence at our shots.
    complaints: std::collections::HashMap<retrox_sim::bubble::FleetKey, f64>,
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
        // No bubble until the player arrives.
        bubble::set_centre(&mut world.galaxy, None);
        // Generation leaves every market exactly on target. Let the galaxy
        // run for a while first, so the player arrives in a going concern:
        // traders under way, some markets short, others with goods to spare.
        world.advance_clock(WARM_UP_DAYS * SECONDS_PER_DAY);
        bubble::set_centre(&mut world.galaxy, Some(system));
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
            skim: crate::skim::Skim::default(),
            beam: None,
            message: None,
            bump: 0.0,
            screen: Screen::Hub,
            inspect: None,
            system_generation: 0,
            map_open: false,
            course: None,
            board: Vec::new(),
            board_key: None,
            waiting: None,
            jump_start_s: None,
            traffic: Traffic::default(),
            combat: Combat::new(player.ship.fit.clone(), player.ship.damage.clone()),
            burn: 0.0,
            interdicted: None,
            complaints: Default::default(),
            convoy_seen: None,
            player,
        };
        // Traders near the player are ships (DESIGN §5.2).
        bubble::set_centre(&mut world.galaxy, Some(system));
        world.settle_prowls();
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
        self.star().radius_suns() * SOLAR_RADIUS_M
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
            Place::Convoy(fleet) => {
                // Where the traffic last had it, carried on straight.
                let seen = self
                    .traffic
                    .contacts
                    .iter()
                    .find(|c| c.key == fleet)
                    .map(|c| (c.pos, c.vel, self.traffic.at_s))
                    .or(self
                        .convoy_seen
                        .filter(|s| s.0 == fleet)
                        .map(|s| (s.1, s.2, s.3)));
                match seen {
                    Some((pos, vel, at_s)) => pos + vel * (t * SECONDS_PER_DAY - at_s),
                    None => DVec3::ZERO,
                }
            }
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
            Place::Convoy(fleet) => self
                .galaxy
                .companies
                .get(fleet.0)
                .map_or("a convoy".into(), |c| format!("{} convoy", c.name)),
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
            Place::Convoy(_) => 200.0,
        }
    }

    /// Gravity well radius; zero for things without one.
    pub fn well_radius(&self, place: Place) -> f64 {
        match place {
            Place::Star | Place::Body(_) => self.place_radius(place) * WELL_RADII,
            _ => 0.0,
        }
    }

    /// Whether a body has a city to land at.
    pub fn has_city(&self, body: BodyId) -> bool {
        self.view.cities.iter().any(|&(_, b)| b == body)
    }

    /// Where fast travel to `place` stops, measured from its centre.
    pub fn arrival_distance(&self, place: Place) -> f64 {
        match place {
            // Gas giants: just above the cloud deck, ready to skim.
            Place::Body(b) if self.galaxy.bodies[b].kind == BodyKind::GasGiant => {
                self.place_radius(place) + crate::skim::ARRIVAL_ALTITUDE_M
            }
            Place::Body(b) if self.has_city(b) => self.place_radius(place) * APPROACH_RADII,
            Place::Star | Place::Body(_) => self.well_radius(place) * 1.05,
            Place::Field(_) => self.place_radius(place) + 10e3,
            Place::Station(_) => 20e3,
            Place::Convoy(_) => 2.5e3,
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

    /// Whether fast travel to `place` would stop inside `well`: the
    /// autopilot may fly there without climbing out first.
    pub fn arrives_within(&self, place: Place, well: Place) -> bool {
        let from_centre = (self.position(place) - self.position(well)).length();
        from_centre + self.arrival_distance(place) < self.well_radius(well)
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
            let reach = self.place_radius(Place::Body(body)) * LANDING_RADII;
            if self.distance_to(Place::Body(body)) < reach {
                p.land = Some((city, body));
            }
        }
        p.well = self.current_well();
        if self.hostiles_near() == 0 {
            // Inside a well the autopilot only flies within it (DESIGN
            // §7.1.1): down to what's in it, or else up and out.
            let well = p.well;
            p.fast_travel = self.target.filter(|&t| {
                self.distance_to(t) > self.arrival_distance(t) * 1.2
                    && well.is_none_or(|well| self.arrives_within(t, well))
            });
            if p.fast_travel.is_none() {
                p.climb = well;
            }
        }
        p
    }

    pub fn act(&mut self, action: Action) {
        if self.combat.lost.is_some() {
            // Nothing to fly any more.
            return;
        }
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
            Action::FastTravel => match (self.mode, prompts.fast_travel, prompts.climb) {
                (Mode::Normal, _, _) if self.hostiles_near() > 0 => {
                    self.say("Can't fast travel: hostiles within 20 km");
                }
                (Mode::Normal, Some(target), _) => self.start_fast_travel(Leg::To(target)),
                (Mode::Normal, None, Some(well)) => {
                    let toward = self.target.filter(|&t| t != well);
                    self.start_fast_travel(Leg::Climb { well, toward });
                }
                (Mode::Normal, None, None) => self.say("Select a target to fast travel"),
                (Mode::FastTravel(trip), _, _) => self.drop_out(trip),
                _ => {}
            },
            Action::Scan => self.scan(),
            Action::Intercept => self.intercept(),
            Action::CycleHostile => {
                let me = self.combat_position();
                self.combat.cycle_target(me);
                if self.combat.raiders.is_empty() {
                    self.say("No hostiles in range");
                }
            }
            Action::Dock => {
                if let Some(s) = prompts.dock.filter(|&s| self.cleared(s)) {
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
                if let Some((city, body)) = prompts.land.filter(|&(c, _)| self.cleared(c)) {
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
                if self.hostiles_near() > 0 {
                    self.say("Can't jump: hostiles within 20 km");
                } else if let Some(well) = prompts.well {
                    let name = self.name(well);
                    self.say(format!("Can't jump inside the gravity well of {name}"));
                } else if self.stats().drive_tier == 0 {
                    self.say("Can't jump: no jump drive fitted");
                } else if self.lanes().iter().any(|l| l.to == to) {
                    self.map_open = false;
                    self.jump_start_s = Some(self.clock_s);
                    // Days in the lanes: no bubble until we come out the
                    // other side.
                    bubble::set_centre(&mut self.galaxy, None);
                    self.mode = Mode::Jumping {
                        to,
                        timer: JUMP_DURATION,
                    };
                }
            }
            Action::Wait(days) => {
                if days > 0 && self.waiting.is_none() && self.docked_at().is_some() {
                    // Time compresses: the bubble folds back into the coarse
                    // tier while we wait, and comes back after (DESIGN §5.2).
                    bubble::set_centre(&mut self.galaxy, None);
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
                let before = self.player.credits;
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
                self.player.progress.earned |= self.player.credits > before;
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

    /// Wait `days` and run the wait to its end at once (for tests).
    #[cfg(test)]
    pub fn wait_days(&mut self, days: u64) {
        self.act(Action::Wait(days));
        while self.waiting.is_some() {
            self.wait_some();
        }
    }

    /// Say how a wait ended, and settle contracts it made due.
    fn end_wait(&mut self, w: Waiting, due: Option<String>) {
        bubble::set_centre(&mut self.galaxy, Some(self.system));
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
        let before = self.player.credits;
        let mut said = contracts::expire(&mut self.player, &mut self.galaxy);
        if let Some(here) = self.docked_at() {
            said.extend(contracts::deliver(&mut self.player, &mut self.galaxy, here));
        }
        self.player.progress.earned |= self.player.credits > before;
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
        self.bump = 0.0;
        let from = self.ship_position();
        let clock0 = self.clock_s;
        if let Some((t, _)) = &mut self.interdicted {
            *t += dt;
            if *t > 6.0 {
                self.interdicted = None;
            }
        }
        if let Some((_, t)) = &mut self.message {
            *t -= dt as f32;
            if *t <= 0.0 {
                self.message = None;
            }
        }

        match self.mode {
            Mode::Normal if self.combat.lost.is_some() => {
                // What's left drifts on.
                self.ship.rel += self.ship.vel * dt;
                self.burn = 0.0;
                self.advance_clock(dt);
            }
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

        self.traffic.update(&self.galaxy, self.system, self.clock_s);
        self.keep_convoy_frame();
        self.prowl(from, clock0);
        self.player.standing.drift(self.galaxy.time.days());
        self.settle_contracts();
        self.refresh_board();
        self.update_field();
        let trigger = controls.fire && self.mode == Mode::Normal && self.combat.lost.is_none();
        // Mid-fight the trigger is the guns' alone.
        let fighting = self.hostiles_near() > 0 && !self.combat.player.guns.is_empty();
        // In a giant's skimming band the trigger opens the scoop instead.
        let pass = self.skim_pass();
        let skimming = pass.is_some_and(|p| crate::skim::density(p.altitude_m) > 0.0);
        self.mine(dt, trigger && !fighting && !skimming);
        self.skim(dt, pass, trigger && !fighting);
        self.convoys();
        self.fight(dt, trigger);
        self.repair_docked();
        self.update_camera(dt);
        let mut progress = std::mem::take(&mut self.player.progress);
        progress.observe(self);
        self.player.progress = progress;
    }

    /// The ship's pass over a gas giant, if it's within twice the arrival
    /// height of one's cloud deck and flying.
    pub fn skim_pass(&self) -> Option<crate::skim::Pass> {
        if self.mode != Mode::Normal || self.combat.lost.is_some() {
            return None;
        }
        let pos = self.ship_position();
        let t = self.t_days();
        self.view
            .bodies
            .iter()
            .copied()
            .filter(|&b| self.galaxy.bodies[b].kind == BodyKind::GasGiant)
            .find_map(|b| {
                let centre = self.position(Place::Body(b));
                let d = pos - centre;
                let altitude_m = d.length() - self.place_radius(Place::Body(b));
                (altitude_m < 2.0 * crate::skim::ARRIVAL_ALTITUDE_M).then(|| {
                    let vel = self.ship.vel + self.velocity_at(self.ship.frame, t)
                        - self.velocity_at(Place::Body(b), t);
                    let stats = self.stats();
                    crate::skim::Pass {
                        giant: b,
                        altitude_m,
                        vel,
                        up: d.normalize_or(DVec3::Y),
                        richness: self.galaxy.bodies[b].resources.gas,
                        free_t: (stats.cargo_t - self.player.cargo_t()).max(0.0),
                    }
                })
            })
    }

    /// One frame of the gas scoop: gas into the hold, the source strained by
    /// it as NPC skimmers strain it, and the gas's drag and shoves.
    fn skim(&mut self, dt: f64, pass: Option<crate::skim::Pass>, trigger: bool) {
        let stats = self.stats();
        let out = self.skim.update(dt, &stats, pass, trigger);
        if out.gas_t > 0.0 {
            self.player.cargo[Good::Gas] += out.gas_t;
            self.mining.haul[Good::Gas] += out.gas_t;
            if let Some(p) = pass {
                retrox_sim::harvest::strain(
                    &mut self.galaxy,
                    retrox_sim::harvest::Source::Body(p.giant),
                    out.gas_t,
                );
            }
        }
        self.ship.vel += out.kick;
        if let Some(text) = out.message {
            self.say(text);
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

    /// Skim the richest gas giant in this system (or the first system with
    /// one): 15 km over its sunlit cloud deck, flying level at 300 m/s
    /// (for testing). Returns whether it found one.
    pub fn start_skimming(&mut self) -> bool {
        let giants = |w: &Self, id: SystemId| -> Option<BodyId> {
            w.galaxy.systems[id]
                .bodies
                .iter()
                .copied()
                .filter(|&b| w.galaxy.bodies[b].kind == BodyKind::GasGiant)
                .max_by(|&a, &b| {
                    let g = |x: BodyId| w.galaxy.bodies[x].resources.gas;
                    g(a).total_cmp(&g(b))
                })
        };
        if giants(self, self.system).is_none() {
            let Some(system) = self
                .galaxy
                .systems
                .iter()
                .find_map(|(id, _)| giants(self, id).map(|_| id))
            else {
                return false;
            };
            self.arrive_jump(system);
        }
        let Some(giant) = giants(self, self.system) else {
            return false;
        };
        let place = Place::Body(giant);
        let centre = self.position(place);
        // Over the dayside, a little off the line to the star.
        let up = (-centre).normalize_or(DVec3::X);
        let up = (up + up.cross(DVec3::Y) * 0.6 + DVec3::Y * 0.25).normalize();
        let along = up.cross(DVec3::Y).normalize_or(DVec3::Z);
        self.mode = Mode::Normal;
        self.ship.frame = place;
        self.ship.rel = up * (self.place_radius(place) + 15_000.0);
        self.ship.vel = along * 300.0;
        // Level: wings parallel to the cloud deck.
        let right = along.cross(up).normalize();
        self.ship.rot = DQuat::from_mat3(&glam::DMat3::from_cols(right, up, -along));
        self.ship.throttle = 0.3;
        self.ship.flight_assist = true;
        self.cam_rot = self.ship.rot;
        self.message = None;
        true
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
        self.park_in(field)
    }

    /// Park in `field` (of this system) facing a mid-sized rock a few
    /// hundred metres off. Returns whether there was one.
    pub fn park_in(&mut self, field: FieldId) -> bool {
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

    /// Start in space by a harvester at work on `good` (for screenshots of
    /// issue #68), waiting a few days for one if need be. Returns false if
    /// none turned up.
    pub fn start_by_harvesters(&mut self, good: Good) -> bool {
        let at_work = |w: &Self| {
            w.galaxy.bubble.flights.values().find_map(|f| {
                let work = f.working()?;
                let bubble::Stage::Flying { system, ship } = f.stage else {
                    return None;
                };
                let (g, _) = retrox_sim::harvest::yields(&w.galaxy, work.source)?;
                let goal = retrox_sim::harvest::work_goal(
                    &w.galaxy,
                    work.source,
                    work.slot,
                    w.galaxy.time.days() as f64,
                );
                let there = retrox_sim::flight::length(retrox_sim::flight::sub(goal.pos, ship.pos));
                (g == good && there < 2_000.0).then_some(system)
            })
        };
        let mut found = at_work(self);
        for _ in 0..40 {
            if found.is_some() {
                break;
            }
            tick(&mut self.galaxy);
            self.clock_s = self.galaxy.time.days() as f64 * SECONDS_PER_DAY;
            found = at_work(self);
        }
        let Some(system) = found else {
            return false;
        };
        if system != self.system {
            self.arrive_jump(system);
        }
        self.traffic.update(&self.galaxy, self.system, self.clock_s);
        let Some(c) = self
            .traffic
            .contacts
            .iter()
            .filter(|c| {
                c.work
                    .as_ref()
                    .is_some_and(|w| w.at_source && w.good == good)
            })
            .max_by_key(|c| c.ships)
            .cloned()
        else {
            return false;
        };
        let length = c.fit.stats().length_m.max(10.0);
        let along = c.vel.normalize_or(DVec3::Z);
        let across = along.cross(DVec3::Y).normalize_or(DVec3::X);
        // Off its quarter and above, looking past it at its work.
        let at = c.pos
            + (across * 1.5 + DVec3::Y * 0.55 - along * 0.8)
                * (length * (1.6 + 0.6 * f64::from(c.ships)));
        self.mode = Mode::Normal;
        self.ship.frame = Place::Star;
        self.ship.rel = at;
        self.ship.vel = c.vel;
        self.ship.throttle = 0.0;
        self.ship.flight_assist = false;
        self.ship.rot = look_rotation(c.pos - at);
        self.cam_rot = self.ship.rot;
        self.message = None;
        true
    }

    /// Start in space alongside a trader fleet, drifting with it (for
    /// testing the bubble's traffic). Returns false if none is flying.
    pub fn start_by_traffic(&mut self) -> bool {
        for _ in 0..4 {
            self.traffic.update(&self.galaxy, self.system, self.clock_s);
            if !self.traffic.contacts.is_empty() {
                break;
            }
            let Some(system) = self
                .galaxy
                .bubble
                .flights
                .values()
                .find_map(|f| match f.stage {
                    bubble::Stage::Flying { system, .. } => Some(system),
                    bubble::Stage::Jumping { .. } => None,
                })
            else {
                return false;
            };
            self.arrive_jump(system);
        }
        let Some(c) = self
            .traffic
            .contacts
            .iter()
            .max_by_key(|c| c.ships)
            .cloned()
        else {
            return false;
        };
        // Off its flank and a little above, looking across at it, moving
        // with it, flight assist off so we keep pace.
        let length = c.fit.stats().length_m.max(10.0);
        let across = c.vel.cross(DVec3::Y).normalize_or(DVec3::X);
        let at = c.pos
            + (across * 2.2 + DVec3::Y * 0.5 - c.vel.normalize_or(DVec3::Z) * 0.6)
                * (length * (3.0 + f64::from(c.ships)));
        self.mode = Mode::Normal;
        self.ship.frame = Place::Star;
        self.ship.rel = at;
        self.ship.vel = c.vel;
        self.ship.throttle = 0.0;
        self.ship.flight_assist = false;
        self.ship.rot = look_rotation(c.pos - at);
        self.cam_rot = self.ship.rot;
        self.message = None;
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
            // The ship lost: from outside, drawing slowly back and round from
            // what's left of it (DESIGN §7.7).
            _ if self.combat.lost.is_some() => {
                let t = self.combat.lost.unwrap_or(0.0);
                let ease = 1.0 - (-t / 3.0).exp();
                let rot = self.cam_rot * DQuat::from_rotation_y(-0.35 * ease + 0.02 * t);
                let offset = DVec3::new(0.0, 6.0 + 10.0 * ease, 28.0 + 50.0 * ease);
                let offset = rot * offset * self.camera_scale();
                // At the wreck, keeping the chase camera's roll.
                let f = (-offset).normalize();
                let r = f.cross(rot * DVec3::Y).normalize_or(rot * DVec3::X);
                let look = DQuat::from_mat3(&glam::DMat3::from_cols(r, r.cross(f), -f));
                (ship + offset, look)
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
