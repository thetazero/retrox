//! The galaxy simulation: plain data, an explicit tick pipeline, no engine.
//!
//! See `docs/DESIGN.md` §5. State lives in typed arenas inside
//! [`GalaxyState`]; [`tick`] advances it by one galaxy day by running each
//! phase of [`PIPELINE`] in order.

pub mod arena;
pub mod bodies;
pub mod colonize;
pub mod construction;
pub mod demography;
pub mod economy;
pub mod fields;
pub mod fiscal;
pub mod haul;
pub mod history;
pub mod phases;
pub mod rng;
pub mod settlement;
pub mod ship;
pub mod state;
pub mod tick;
pub mod time;

pub use arena::{Arena, Id};
pub use bodies::{Body, BodyId, BodyKind, Orbit, Orbiting, PlanetClass, Region, RegionId};
pub use fields::{AsteroidField, FieldId};
pub use history::{History, HistoryEvent, HistoryKind};
pub use rng::Rng;
pub use settlement::{Settlement, SettlementId, Site};
pub use state::{
    EconomyModel, Faction, FactionId, GalaxyPos, GalaxyState, Hyperlane, LaneId, Resources,
    StarClass, StarSystem, SystemId,
};
pub use tick::{PIPELINE, Phase, tick, tick_profiled};
pub use time::GalaxyTime;
