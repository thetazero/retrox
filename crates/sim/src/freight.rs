//! Freight contracts for the player (issue #69): the dispatch board of a
//! settlement.
//!
//! A contract is a real need, not a dice roll. It pairs a good this
//! settlement can spare with a nearby settlement that holds too little of
//! it, counting what traders already have on the way. A settlement spares
//! stock down to [`RELEASE_TO`] of its target, and only for a consignee
//! below that, so goods always flow from fuller markets to emptier ones. The
//! short settlement — the **consignee** — buys the goods here at this
//! market's price and pays the captain a fee to bring them over. So every
//! contract on the board is one the galaxy needs carried, and taking one
//! moves the goods from where they pile up to where they're missing.
//!
//! The fee is carriage (DESIGN §7.8): the hauler's keep for the trip, a
//! rate per tonne-day plus the crew's wage. How short the consignee is
//! decides whether it posts at all, and orders the board, not the pay.
//! The price gap between the two markets, and a planned faction's
//! delivery premium, belong to whoever buys the goods and
//! carries them at their own risk: a trader (or an NPC hauler), not a
//! captain paid to move someone else's cargo.
//!
//! Money moves (in the client, which owns the player's purse):
//! - **Accept:** the goods leave this market. The consignee pays this
//!   settlement for them, puts the fee in escrow, and takes a deposit of
//!   the goods' value from the captain.
//! - **Deliver:** the goods enter the consignee's market; the captain gets
//!   the deposit back and the fee, less [`LATE_PENALTY_PER_DAY`] for each
//!   day late. What's withheld goes back to the consignee.
//! - **Fail** (abandoned, or [`GRACE_DAYS`] past the deadline): the
//!   consignee keeps the deposit, which paid for the goods, and takes the
//!   fee back. The captain keeps the goods.
//!
//! The board is a pure function of the galaxy state, so the same state
//! shows the same board (DESIGN §5.1).

use serde::{Deserialize, Serialize};

use crate::economy::Good;
use crate::routes::{self, Visit};
use crate::settlement::{SettlementId, Site};
use crate::{EconomyModel, GalaxyState, GalaxyTime, SystemId};

/// The board lists settlements at most this many jumps away.
pub const MAX_JUMPS: u32 = 5;
/// At most this many contracts on a board.
pub const BOARD_SIZE: usize = 8;
/// At most this many contracts to the same settlement.
const PER_DESTINATION: usize = 2;
/// A settlement posts for a good only once it is this far below target
/// (as a share of target), counting what's already on the way.
const MIN_SHORTAGE: f64 = 0.25;
/// The settlement whose board it is releases stock down to this share of
/// its target, so it never ends up shorter than the consignee started.
const RELEASE_TO: f64 = 1.0 - MIN_SHORTAGE;
/// Freight, in credits per tonne per day of the trip. It is what the galaxy's own
/// haulers spend keeping a tonne of hold in service (DESIGN §7.8): the
/// galaxy's haulers move goods for that, so a consignee won't pay a captain
/// much more.
const FREIGHT_RATE: f64 = crate::haul::UPKEEP_PER_CARGO_T_DAY;
/// The crew's wage, in credits per day of the trip, likewise: a hauler
/// crew's upkeep. The player's ship burns no fuel yet, so none is paid.
const WAGE: f64 = crate::haul::UPKEEP_PER_SHIP_DAY;
/// The smallest lot worth posting, as a share of the hold.
const MIN_LOT_SHARE: f64 = 0.25;
/// Days allowed for each system on the route (flying to the jump point,
/// or to the destination), on top of the jumps themselves.
const SYSTEM_DAYS: f64 = 3.0;
/// The deadline allows this much more than the expected trip.
const DEADLINE_SLACK: f64 = 1.5;
/// Share of the fee lost for each day late.
pub const LATE_PENALTY_PER_DAY: f64 = 0.1;
/// Days after the deadline when a contract fails.
pub const GRACE_DAYS: u64 = 10;
/// The consignee spends no more than this share of its treasury on one
/// contract: goods plus fee.
const MAX_SPEND_SHARE: f64 = 0.2;

/// A contract on the board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    /// Where the goods are loaded: the settlement whose board this is.
    pub from: SettlementId,
    /// The consignee.
    pub to: SettlementId,
    pub good: Good,
    /// Tonnes.
    pub amount: f64,
    /// Jumps along the quickest route.
    pub jumps: u32,
    /// Expected days for the trip, jumps plus flying in each system.
    pub trip_days: f64,
    pub deadline: GalaxyTime,
    /// What the consignee pays for the goods here, and the captain's
    /// deposit.
    pub value: f64,
    /// What the consignee pays the captain on time.
    pub fee: f64,
    /// How far below target the consignee is, after what's on the way:
    /// 0 on target, 1 empty.
    pub shortage: f64,
    /// The consignee is going without it right now (unmet demand).
    pub urgent: bool,
}

/// Something the player is already carrying, so the board doesn't ask
/// for it twice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pending {
    pub to: SettlementId,
    pub good: Good,
    pub amount: f64,
    /// Taken off this board today. The dispatcher posts no more of that
    /// good to that consignee until tomorrow's board (issue #126), so a
    /// lot just taken doesn't seem to stay listed. If the consignee is
    /// still short then, the next lot goes up as a second one.
    pub taken_today: bool,
}

/// The board at `here`: contracts sized to a hold of `lot_t` tonnes,
/// best pay per day first. `lane_days` is how many days the player's ship
/// takes to cross a lane of a given length in light-years. `pending` is
/// what the player already carries under contract.
pub fn board(
    state: &GalaxyState,
    here: SettlementId,
    lot_t: f64,
    lane_days: impl Fn(f64) -> f64,
    pending: &[Pending],
) -> Vec<Offer> {
    let Some(origin) = state.settlements.get(here) else {
        return Vec::new();
    };
    if lot_t < 1.0 {
        return Vec::new();
    }
    let market = &origin.economy.market;
    let model = model(state, here);
    let today = state.time;

    let mut offers: Vec<(f64, Offer)> = Vec::new();
    for (system, jumps, days) in routes(state, origin.system, lane_days) {
        for (to, dest) in state.settlements_in(system) {
            // Only places the player can dock or land at.
            if to == here || matches!(dest.site, Site::Surface(_)) {
                continue;
            }
            let dest_market = &dest.economy.market;
            for good in Good::ALL {
                let spare = market.stock[good] - RELEASE_TO * market.target(good);
                let target = dest_market.target(good);
                let coming = state.traffic.inbound(to, good)
                    + pending
                        .iter()
                        .filter(|p| p.to == to && p.good == good)
                        .map(|p| p.amount)
                        .sum::<f64>();
                let need = target - dest_market.stock[good] - coming;
                let shortage = (need / target).clamp(0.0, 1.0);
                if shortage < MIN_SHORTAGE {
                    continue;
                }
                if pending
                    .iter()
                    .any(|p| p.taken_today && p.to == to && p.good == good)
                {
                    continue;
                }
                let min_lot = (lot_t * MIN_LOT_SHARE).max(1.0);
                let mut amount = spare.min(need).min(lot_t).floor();
                if amount < min_lot {
                    continue;
                }
                let buy = |n: f64| market.price_at(good, market.stock[good] - n / 2.0, model);
                let trip_days = days + SYSTEM_DAYS * f64::from(jumps + 1);
                let price = |n: f64| {
                    let value = buy(n) * n;
                    let fee = (n * FREIGHT_RATE + WAGE) * trip_days;
                    (value, fee)
                };
                // The consignee only commits what it can afford.
                let budget = dest.economy.treasury.max(0.0) * MAX_SPEND_SHARE;
                let (mut value, mut fee) = price(amount);
                while amount >= min_lot && value + fee > budget {
                    amount = (amount * 0.7).floor();
                    (value, fee) = price(amount);
                }
                if amount < min_lot {
                    continue;
                }
                let allowed = (trip_days * DEADLINE_SLACK).ceil() as u64 + 1;
                let offer = Offer {
                    from: here,
                    to,
                    good,
                    amount,
                    jumps,
                    trip_days,
                    deadline: GalaxyTime::from_days(today.days() + allowed),
                    value,
                    fee,
                    shortage,
                    urgent: dest_market.shortfall[good] > 0.01,
                };
                offers.push((fee / trip_days.max(1.0), offer));
            }
        }
    }

    // Best pay per day first; ties broken by ids so the order is stable.
    offers.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.to.cmp(&b.1.to))
            .then_with(|| a.1.good.cmp(&b.1.good))
    });
    let mut board: Vec<Offer> = Vec::new();
    for (_, offer) in offers {
        if board.len() >= BOARD_SIZE {
            break;
        }
        // A destination gets a few contracts at most, so the board has
        // range.
        let to_there = board.iter().filter(|o| o.to == offer.to).count();
        if to_there < PER_DESTINATION {
            board.push(offer);
        }
    }
    board
}

/// What the captain is paid for delivering on `day`, given the deadline:
/// the fee less the late penalty. Never negative.
pub fn pay_on(fee: f64, deadline: GalaxyTime, day: GalaxyTime) -> f64 {
    let late = day.days().saturating_sub(deadline.days()) as f64;
    fee * (1.0 - late * LATE_PENALTY_PER_DAY).max(0.0)
}

/// The day a contract with this deadline fails.
pub fn fails_on(deadline: GalaxyTime) -> GalaxyTime {
    GalaxyTime::from_days(deadline.days() + GRACE_DAYS)
}

/// Every system within [`MAX_JUMPS`] of `origin`, with the jumps and days
/// of the quickest route there, nearest first.
fn routes(
    state: &GalaxyState,
    origin: SystemId,
    lane_days: impl Fn(f64) -> f64,
) -> Vec<(SystemId, u32, f64)> {
    let mut out = Vec::new();
    routes::search(
        state,
        origin,
        |lane| lane_days(lane.length_ly),
        |step| {
            out.push((step.system, step.jumps, step.cost));
            if step.jumps >= MAX_JUMPS {
                Visit::Leaf
            } else {
                Visit::Expand
            }
        },
    );
    out
}

fn model(state: &GalaxyState, id: SettlementId) -> EconomyModel {
    model_of(state, state.settlements[id].owner)
}

fn model_of(state: &GalaxyState, owner: crate::FactionId) -> EconomyModel {
    state.factions[owner].economy
}
