//! Settlements: the places people build and live in. A settlement belongs
//! to exactly one faction and carries what used to be a whole system's
//! economy: population, facilities and its own market.

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::bodies::{BodyId, Orbit, RegionId};
use crate::economy::LocalEconomy;
use crate::{FactionId, GalaxyTime, SystemId};

new_key_type! {
    /// Settlements can be founded and destroyed, so they use generational keys.
    pub struct SettlementId;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settlement {
    pub system: SystemId,
    pub site: Site,
    /// Every settlement has an owner; an independent settlement is a
    /// faction of its own.
    pub owner: FactionId,
    pub name: String,
    pub founded: GalaxyTime,
    pub economy: LocalEconomy,
    /// How loyal the settlement is to its faction, 0..1; see
    /// [`crate::politics`]. Low for long enough, it may break away.
    pub stability: f64,
}

/// Where a settlement is.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Site {
    /// Part of a divided, habitable world.
    Region(RegionId),
    /// A whole undivided body: a moon base, a camp in a belt.
    Surface(BodyId),
    /// A station in space, orbiting a body or the star.
    Space(Orbit),
}
