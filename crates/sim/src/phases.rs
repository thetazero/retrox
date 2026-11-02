//! The phases of a galaxy day, in pipeline order. Empty phases are filled
//! in by later milestones.

use crate::GalaxyState;

/// Facilities consume inputs and produce outputs into stockpiles.
pub fn production(state: &mut GalaxyState) {
    crate::economy::production(state);
}

/// Faction AI: plan quotas, fleet orders, diplomacy.
pub fn planning(_state: &mut GalaxyState) {}

/// Goods flow between markets along the hyperlane network.
pub fn trade(state: &mut GalaxyState) {
    crate::economy::trade(state);
}

/// Fleets and coarse traffic move along lanes.
pub fn movement(_state: &mut GalaxyState) {}

/// Battles are resolved abstractly.
pub fn conflict(_state: &mut GalaxyState) {}

/// Emergent events and storylet triggers.
pub fn events(_state: &mut GalaxyState) {}

/// Factions report (their version of) what happened.
pub fn news(_state: &mut GalaxyState) {}
