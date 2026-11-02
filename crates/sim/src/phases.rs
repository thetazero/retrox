//! The phases of a galaxy day, in pipeline order. Empty phases are filled
//! in by later milestones.

use crate::GalaxyState;

/// Every settlement's daily books (DESIGN §5.1.1): facilities consume
/// inputs and produce outputs, populations eat then grow or shrink,
/// projects take their materials, and money moves as taxes and welfare.
/// Arithmetic only.
pub fn production(state: &mut GalaxyState) {
    crate::economy::production(state);
    crate::construction::progress_all(state);
    crate::fiscal::taxes_and_welfare(state);
}

/// Decisions: the settlements whose monthly review falls today choose what
/// to build and whether to send colonists. Plan quotas, fleet orders and
/// diplomacy later.
pub fn planning(state: &mut GalaxyState) {
    crate::construction::review(state);
    crate::colonize::launch(state);
}

/// Docked fleets sell their cargo and pick their next trip.
pub fn trade(state: &mut GalaxyState) {
    crate::haul::trade(state);
}

/// Fleets arrive where they were going.
pub fn movement(state: &mut GalaxyState) {
    crate::colonize::arrive(state);
}

/// Battles are resolved abstractly.
pub fn conflict(_state: &mut GalaxyState) {}

/// Emergent events and storylet triggers.
pub fn events(_state: &mut GalaxyState) {}

/// Factions report (their version of) what happened.
pub fn news(_state: &mut GalaxyState) {}

/// Every market prices every good from its stock, once at the end of the
/// day (DESIGN §5.1.1).
pub fn prices(state: &mut GalaxyState) {
    crate::economy::update_all_prices(state);
}
