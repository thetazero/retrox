//! The complete simulation state. A save is (mostly) this struct, serialized.

use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::economy::SystemEconomy;
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
    pub factions: SlotMap<FactionId, Faction>,
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
    pub owner: Option<FactionId>,
    /// Natural resources, each 0..1 richness.
    pub resources: Resources,
    pub economy: SystemEconomy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    pub ore: f64,
    pub ice: f64,
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
    pub capital: SystemId,
    pub founded: GalaxyTime,
}

/// How a faction runs its economy (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomyModel {
    /// Quotas and multi-year plans; stable official prices, shortages and gluts.
    Planned,
    /// Floating prices; booms and crashes.
    Market,
}
