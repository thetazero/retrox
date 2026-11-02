//! The complete simulation state. A save is (mostly) this struct, serialized.

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::pool::Pool;

use crate::bodies::{Body, BodyId, EARTH_MASSES_PER_SUN, Orbit, Orbiting, Region, RegionId};
use crate::haul::{Company, CompanyId, ShipClass};
use crate::settlement::{Settlement, SettlementId, Site};
use crate::{Arena, GalaxyTime, History, Id};

pub type SystemId = Id<StarSystem>;
pub type LaneId = Id<Hyperlane>;

new_key_type! {
    /// Factions can die, so they use generational keys rather than an arena.
    pub struct FactionId;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GalaxyState {
    pub seed: u64,
    pub time: GalaxyTime,
    pub systems: Arena<StarSystem>,
    pub lanes: Arena<Hyperlane>,
    pub bodies: Arena<Body>,
    pub regions: Arena<Region>,
    /// Asteroid fields inside belts.
    #[serde(default)]
    pub fields: Arena<crate::AsteroidField>,
    pub settlements: Pool<SettlementId, Settlement>,
    pub factions: Pool<FactionId, Faction>,
    /// The kinds of trade ship in service.
    pub ship_classes: Arena<ShipClass>,
    pub companies: Pool<CompanyId, Company>,
    /// Traders' upcoming events; see [`crate::haul`].
    pub calendar: crate::haul::Calendar,
    /// Cargo on its way to each market.
    pub traffic: crate::haul::Traffic,
    /// Each system's best buyers and sellers, rebuilt in place every trade
    /// phase; see [`crate::haul`].
    #[serde(skip)]
    pub(crate) board: crate::haul::Board,
    /// Remembered lane distances; see [`crate::haul::ReachCache`].
    #[serde(skip)]
    pub reach_cache: crate::haul::ReachCache,
    /// Recent takings from each harvest source; see [`crate::harvest`].
    #[serde(default)]
    pub strain: crate::harvest::Strain,
    /// Where the harvest sources are; see [`crate::harvest::SourceIndex`].
    #[serde(skip)]
    pub source_index: crate::harvest::SourceIndex,
    /// The agent tier around the player; see [`crate::bubble`].
    #[serde(default)]
    pub bubble: crate::bubble::Bubble,
    /// Pirate gangs and their prowls; see [`crate::pirates`].
    #[serde(default)]
    pub pirates: crate::pirates::Pirates,
    pub colony_ships: Pool<crate::colonize::ColonyShipId, crate::colonize::ColonyShip>,
    /// Which sites are taken and where colonists could go; see
    /// [`crate::colonize::Sites`].
    #[serde(skip)]
    pub(crate) sites: crate::colonize::Sites,
    pub history: History,
}

impl GalaxyState {
    /// An empty galaxy. Normally you'd get one from `retrox-gen` instead.
    pub fn empty(seed: u64) -> Self {
        let mut state = Self {
            seed,
            ..Self::default()
        };
        state
            .history
            .record(GalaxyTime::ZERO, crate::HistoryKind::Genesis { seed });
        state
    }

    /// Add a hyperlane between two systems, keeping both systems' lane
    /// lists in sync.
    pub fn connect(&mut self, a: SystemId, b: SystemId) -> LaneId {
        assert_ne!(a, b, "a hyperlane must join two different systems");
        let length_ly = self.systems[a].position.distance(self.systems[b].position);
        let lane = self.lanes.push(Hyperlane { a, b, length_ly });
        self.systems[a].lanes.push(lane);
        self.systems[b].lanes.push(lane);
        lane
    }

    /// Systems one hyperlane jump away from `system`.
    pub fn neighbours(&self, system: SystemId) -> impl Iterator<Item = SystemId> + '_ {
        self.systems[system]
            .lanes
            .iter()
            .map(move |&lane| self.lanes[lane].other(system))
    }

    /// Add a body to its system, keeping the system's body list in sync.
    pub fn add_body(&mut self, body: Body) -> BodyId {
        let system = body.system;
        let id = self.bodies.push(body);
        self.systems[system].bodies.push(id);
        id
    }

    /// Add a region to its body, keeping the body's region list in sync.
    pub fn add_region(&mut self, region: Region) -> RegionId {
        let body = region.body;
        let id = self.regions.push(region);
        self.bodies[body].regions.push(id);
        id
    }

    /// Found a settlement, keeping its system's settlement list in sync.
    pub fn add_settlement(&mut self, settlement: Settlement) -> SettlementId {
        let system = settlement.system;
        self.sites.settle(settlement.site);
        let id = self.settlements.insert(settlement);
        self.systems[system].settlements.push(id);
        id
    }

    /// Whether a settlement is a highport: a station orbiting a body that
    /// has a city on it (#112). A city and the port above it are one
    /// settled place, for anything that counts places (pirate gangs).
    pub fn is_port(&self, settlement: &Settlement) -> bool {
        self.settlements_in(settlement.system)
            .any(|(_, s)| self.port_over(settlement, s))
    }

    /// Whether `port` is a station orbiting the body that `city` is on.
    pub fn port_over(&self, port: &Settlement, city: &Settlement) -> bool {
        match (port.site, city.site) {
            (crate::Site::Space(orbit), crate::Site::Region(r)) => {
                orbit.parent == crate::Orbiting::Body(self.regions[r].body)
            }
            _ => false,
        }
    }

    /// Settled places: settlements, not counting highports.
    pub fn places(&self) -> usize {
        self.settlements
            .values()
            .filter(|s| !self.is_port(s))
            .count()
    }

    /// Settled places in a system: its settlements, not counting highports.
    pub fn places_in(&self, system: SystemId) -> usize {
        self.settlements_in(system)
            .filter(|(_, s)| !self.is_port(s))
            .count()
    }

    /// The settlements in a system, in founding order.
    pub fn settlements_in(
        &self,
        system: SystemId,
    ) -> impl Iterator<Item = (SettlementId, &Settlement)> + '_ {
        self.systems[system]
            .settlements
            .iter()
            .map(move |&id| (id, &self.settlements[id]))
    }

    /// The faction with the most population in a system, if any. Nobody
    /// owns a system; this is for display and summaries only.
    pub fn largest_faction_in(&self, system: SystemId) -> Option<FactionId> {
        let mut totals: Vec<(FactionId, f64)> = Vec::new();
        for (_, s) in self.settlements_in(system) {
            match totals.iter_mut().find(|(f, _)| *f == s.owner) {
                Some((_, p)) => *p += s.economy.population,
                None => totals.push((s.owner, s.economy.population)),
            }
        }
        // Ties go to the faction that settled first.
        totals
            .into_iter()
            .reduce(|best, x| if x.1 > best.1 { x } else { best })
            .map(|(f, _)| f)
    }

    /// Mass of whatever `orbit` goes around, in solar masses.
    pub fn parent_mass_suns(&self, system: SystemId, orbit: &Orbit) -> f64 {
        match orbit.parent {
            Orbiting::Star => self.systems[system].star.mass_suns(),
            Orbiting::Body(b) => self.bodies[b].mass / EARTH_MASSES_PER_SUN,
        }
    }

    /// Where a body is at time `t_days`, in AU from its system's star.
    pub fn body_position(&self, body: BodyId, t_days: f64) -> [f64; 3] {
        let b = &self.bodies[body];
        self.orbit_position(b.system, &b.orbit, t_days)
    }

    /// Where something on `orbit` is at time `t_days`, in AU from the star.
    /// Position (AU from the star) and velocity (AU per day) of something
    /// on `orbit` at time `t_days`, including its parents' motion.
    pub fn orbit_state(
        &self,
        system: SystemId,
        orbit: &Orbit,
        t_days: f64,
    ) -> ([f64; 3], [f64; 3]) {
        let (p, v) = orbit.state_at(t_days, self.parent_mass_suns(system, orbit));
        match orbit.parent {
            Orbiting::Star => (p, v),
            Orbiting::Body(parent) => {
                let b = &self.bodies[parent];
                let (pp, pv) = self.orbit_state(b.system, &b.orbit, t_days);
                (
                    [pp[0] + p[0], pp[1] + p[1], pp[2] + p[2]],
                    [pv[0] + v[0], pv[1] + v[1], pv[2] + v[2]],
                )
            }
        }
    }

    pub fn orbit_position(&self, system: SystemId, orbit: &Orbit, t_days: f64) -> [f64; 3] {
        let local = orbit.position_at(t_days, self.parent_mass_suns(system, orbit));
        match orbit.parent {
            Orbiting::Star => local,
            Orbiting::Body(parent) => {
                let p = self.body_position(parent, t_days);
                [p[0] + local[0], p[1] + local[1], p[2] + local[2]]
            }
        }
    }

    /// The body a settlement is on or around, if any.
    pub fn settlement_body(&self, settlement: SettlementId) -> Option<BodyId> {
        match self.settlements[settlement].site {
            Site::Region(r) => Some(self.regions[r].body),
            Site::Surface(b) => Some(b),
            Site::Space(orbit) => match orbit.parent {
                Orbiting::Body(b) => Some(b),
                Orbiting::Star => None,
            },
        }
    }
}

/// A position in the galaxy, in light-years from the galactic centre.
/// Galaxy-scale only; positions inside a system use a per-system frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GalaxyPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl GalaxyPos {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn distance(self, other: Self) -> f64 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StarSystem {
    pub name: String,
    pub position: GalaxyPos,
    pub star: StarClass,
    /// Hyperlanes touching this system. Maintained by [`GalaxyState::connect`].
    pub lanes: Vec<LaneId>,
    /// Planets, moons and belts, inner to outer. Maintained by
    /// [`GalaxyState::add_body`].
    pub bodies: Vec<BodyId>,
    /// Settlements here, in founding order. Maintained by
    /// [`GalaxyState::add_settlement`]. Nobody owns a system: factions own
    /// the settlements in it.
    pub settlements: Vec<SettlementId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    pub ore: f64,
    pub ice: f64,
    /// Skimmable gas: only gas giants have any (issue #68).
    #[serde(default)]
    pub gas: f64,
}

/// The primary star's spectral class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StarClass {
    O,
    B,
    A,
    F,
    G,
    K,
    M,
    WhiteDwarf,
    Neutron,
    BlackHole,
}

impl StarClass {
    /// Typical mass in solar masses.
    pub const fn mass_suns(self) -> f64 {
        use StarClass::*;
        match self {
            O => 20.0,
            B => 6.0,
            A => 2.0,
            F => 1.3,
            G => 1.0,
            K => 0.7,
            M => 0.3,
            WhiteDwarf => 0.6,
            Neutron => 1.4,
            BlackHole => 10.0,
        }
    }

    /// Typical radius in solar radii.
    pub const fn radius_suns(self) -> f64 {
        use StarClass::*;
        match self {
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

    /// Typical luminosity in solar luminosities.
    pub const fn luminosity_suns(self) -> f64 {
        use StarClass::*;
        match self {
            O => 50_000.0,
            B => 1_000.0,
            A => 20.0,
            F => 2.5,
            G => 1.0,
            K => 0.3,
            M => 0.03,
            WhiteDwarf => 0.002,
            Neutron => 0.0001,
            BlackHole => 0.0,
        }
    }

    /// Centre of the habitable zone, in AU.
    pub fn habitable_zone_au(self) -> f64 {
        self.luminosity_suns().sqrt()
    }

    /// Where water ice survives, in AU.
    pub fn frost_line_au(self) -> f64 {
        (2.7 * self.luminosity_suns().sqrt()).max(0.05)
    }
}

/// An undirected hyperlane between two systems.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hyperlane {
    pub a: SystemId,
    pub b: SystemId,
    pub length_ly: f64,
}

impl Hyperlane {
    /// The system at the other end of this lane from `from`.
    pub fn other(&self, from: SystemId) -> SystemId {
        if from == self.a { self.b } else { self.a }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Faction {
    pub name: String,
    pub economy: EconomyModel,
    pub capital: SettlementId,
    /// Map and branding colour, sRGB.
    pub color: [u8; 3],
    pub founded: GalaxyTime,
    /// The faction's balance, in credits. Taxes raise it and spending
    /// lowers it; below zero, the faction has issued that much money, down
    /// to a limit set by its population (see [`crate::ledger`]).
    pub treasury: f64,
    /// Total population of its settlements, refreshed daily.
    pub population: f64,
    /// What it pays out in welfare tomorrow, and to whom; see
    /// [`crate::fiscal::Welfare`].
    #[serde(default)]
    pub welfare: crate::fiscal::Welfare,
}

/// How a faction runs its economy (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomyModel {
    /// Quotas and multi-year plans; stable official prices, shortages and gluts.
    Planned,
    /// Floating prices; booms and crashes.
    Market,
}
