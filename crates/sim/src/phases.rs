//! The phases of a galaxy day, in pipeline order. Empty phases are filled
//! in by later milestones.

use crate::GalaxyState;

/// Facilities consume inputs and produce outputs into stockpiles.
pub fn production(state: &mut GalaxyState) {
    crate::economy::production(state);
}

/// Faction AI: plan quotas, fleet orders, diplomacy.
pub fn planning(_state: &mut GalaxyState) {}

/// Docked fleets sell their cargo and pick their next trip.
pub fn trade(state: &mut GalaxyState) {
    crate::haul::trade(state);
}

/// Fleets arrive where they were going.
pub fn movement(state: &mut GalaxyState) {
    crate::haul::movement(state);
}

/// Battles are resolved abstractly.
pub fn conflict(_state: &mut GalaxyState) {}

/// Emergent events and storylet triggers.
pub fn events(_state: &mut GalaxyState) {}

/// Factions report (their version of) what happened.
pub fn news(_state: &mut GalaxyState) {}
