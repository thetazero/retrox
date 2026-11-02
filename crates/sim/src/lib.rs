//! The galaxy simulation: plain data, an explicit tick pipeline, no engine.
//!
//! See `docs/DESIGN.md` §5. State lives in typed arenas inside
//! [`GalaxyState`]; [`tick`] advances it by one galaxy day by running each
//! phase of [`PIPELINE`] in order.

pub mod arena;
pub mod history;
pub mod phases;
pub mod state;
pub mod tick;
pub mod time;

pub use arena::{Arena, Id};
pub use history::{History, HistoryEvent, HistoryKind};
pub use state::{
    EconomyModel, Faction, FactionId, GalaxyPos, GalaxyState, Hyperlane, LaneId, StarClass,
    StarSystem, SystemId,
};
pub use tick::{PIPELINE, Phase, tick, tick_profiled};
pub use time::GalaxyTime;
