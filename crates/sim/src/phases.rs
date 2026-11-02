//! The phases of a galaxy day, in pipeline order. Each is filled in by a
//! later milestone; for now they are deliberately empty.

use crate::GalaxyState;

/// Facilities consume inputs and produce outputs into stockpiles.
pub fn production(_state: &mut GalaxyState) {}

/// Faction AI: plan quotas, fleet orders, diplomacy.
pub fn planning(_state: &mut GalaxyState) {}

/// Goods flow between markets along the hyperlane network.
pub fn trade(_state: &mut GalaxyState) {}

/// Fleets and coarse traffic move along lanes.
pub fn movement(_state: &mut GalaxyState) {}

/// Battles are resolved abstractly.
pub fn conflict(_state: &mut GalaxyState) {}

/// Emergent events and storylet triggers.
pub fn events(_state: &mut GalaxyState) {}

/// Factions report (their version of) what happened.
pub fn news(_state: &mut GalaxyState) {}
