//! Settlements: the places people build and live in. A settlement belongs
//! to exactly one faction and carries what used to be a whole system's
//! economy: population, facilities and its own market.

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::bodies::{BodyId, Orbit, RegionId};
use crate::economy::LocalEconomy;
use crate::{FactionId, GalaxyState, GalaxyTime, SystemId};

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

/// Days between a settlement's monthly reviews (DESIGN §5.1.1).
pub const REVIEW_DAYS: u64 = 30;

/// Slots of the settlements whose monthly review falls on `day`, `offset`
/// days into their cycle: slot `day - offset`, then every
/// [`REVIEW_DAYS`]th slot after, up to `slots` (the pool's
/// [`slot_count`](crate::pool::Pool::slot_count)). A stride, not a search:
/// a day only touches the settlements it reviews, and walking slots rather
/// than keys lets the caller change the state as it goes. Each settlement
/// keeps its review day for life; one founded in a reused slot takes that
/// slot's day.
pub fn review_slots(day: u64, offset: u64, slots: usize) -> impl Iterator<Item = usize> {
    let first = (day + REVIEW_DAYS - offset % REVIEW_DAYS) % REVIEW_DAYS;
    (first as usize..slots).step_by(REVIEW_DAYS as usize)
}

/// The settlements reviewing on `day` at `offset`; see [`review_slots`].
/// For callers that only read.
pub fn reviewing(
    settlements: &crate::pool::Pool<SettlementId, Settlement>,
    day: u64,
    offset: u64,
) -> impl Iterator<Item = SettlementId> + '_ {
    review_slots(day, offset, settlements.slot_count()).filter_map(|i| settlements.key_at(i))
}

/// Every settlement's daily books (DESIGN §5.1.1), in one walk over the
/// settlements: it produces and consumes, its population changes by what
/// it ate, its project takes today's materials, and it pays its taxes and
/// takes its welfare. Then companies pay their taxes and factions close
/// their books. Arithmetic and ledger transfers only: nothing is searched
/// for and nothing allocated.
///
/// One walk, on one thread: money moves through shared faction accounts,
/// which fixes the order, and splitting off the part each settlement does
/// alone (production, population) to run on threads costs a second walk
/// over every settlement, which on one core costs more than it saves.
pub fn books(state: &mut GalaxyState) {
    for faction in state.factions.values_mut() {
        faction.welfare.counted = 0.0;
    }
    for i in 0..state.settlements.slot_count() {
        let Some(id) = state.settlements.key_at(i) else {
            continue;
        };
        let GalaxyState {
            settlements,
            factions,
            regions,
            bodies,
            history,
            time,
            ..
        } = &mut *state;
        let settlement = &mut settlements[id];
        let economy = &mut settlement.economy;
        economy.produce();
        let capacity = crate::demography::site_capacity(settlement.site, regions, bodies);
        if crate::demography::live(economy, capacity) {
            history.record(*time, crate::HistoryKind::Famine { settlement: id });
        }
        if let Some(faction) = factions.get_mut(settlement.owner) {
            faction.welfare.counted += economy.population;
        }
        if economy.project.is_some() && economy.population > 0.0 {
            crate::construction::progress(state, id);
        }
        crate::fiscal::settle(state, id);
    }
    crate::fiscal::close_books(state);
}
