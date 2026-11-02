//! Faction money (issue #33, step 3): taxes, welfare and delivery
//! contracts.
//!
//! One credit circulates. Trade only moves it; factions are where it is
//! created and removed. A faction taxes its settlements' treasuries above
//! a reserve and its companies' credits above their working capital, pays
//! welfare to its settlements by population (more to those below their
//! reserve), and — if its economy is planned — pays captains a premium for goods
//! delivered to its short settlements, since its fixed official prices
//! give them no other reason to. A faction's balance can go negative: it
//! has then issued that much money, up to the debt limit [`crate::ledger`] sets.

use slotmap::SecondaryMap;

use crate::economy::Good;
use crate::ledger::{self, Account};
use crate::{EconomyModel, FactionId, GalaxyState, SettlementId};

/// Settlements keep this many days of their own demand, at base prices,
/// untaxed.
const RESERVE_DAYS: f64 = 30.0;
/// Daily tax on a settlement's treasury above its reserve.
const PLANNED_TAX: f64 = 0.01;
const MARKET_TAX: f64 = 0.003;
/// Daily tax on a company's credits above its working capital, paid to
/// the faction of its home settlement.
const COMPANY_TAX: f64 = 0.01;
/// A company keeps enough to fill its holds this many times over, at
/// base prices, untaxed.
const WORKING_CAPITAL_HOLDS: f64 = 2.0;
/// Rough value of a tonne of cargo at base prices, for working capital.
const CARGO_VALUE_PER_T: f64 = 20.0;
/// A faction without a positive balance prints money only for settlements
/// below their reserve: this share of how far below they are, each day,
/// and never more than this share of its remaining debt room.
const RELIEF_SHARE: f64 = 0.02;
const WELFARE_DEBT_SHARE: f64 = 0.002;
/// Share of a faction's positive balance paid out as welfare each day.
const WELFARE_SHARE: f64 = 0.05;
/// Delivery premium, as a share of the base price, at a completely empty
/// market; it falls to zero as stock reaches target.
const CONTRACT_PREMIUM: f64 = 0.8;
/// What a settlement wants to hold in reserve: `RESERVE_DAYS` of its
/// demand at base prices.
fn reserve(state: &GalaxyState, id: SettlementId) -> f64 {
    let market = &state.settlements[id].economy.market;
    RESERVE_DAYS
        * Good::ALL
            .iter()
            .map(|&g| market.demand[g] * g.base_price())
            .sum::<f64>()
}

/// A faction in debt reins in what it spends by choice (contract
/// premiums, construction wages, printed relief) as its debt grows,
/// down to nothing at this share of its debt limit.
const DEBT_COMFORT: f64 = 0.25;

/// How freely a faction spends by choice: fully with money in hand,
/// falling to nothing as its debt reaches [`DEBT_COMFORT`] of its limit.
/// This is what keeps a planned economy's debt from growing for ever.
pub fn thrift(state: &GalaxyState, id: FactionId) -> f64 {
    let Some(f) = state.factions.get(id) else {
        return 0.0;
    };
    if f.treasury >= 0.0 {
        return 1.0;
    }
    let comfort = f.population * ledger::DEBT_PER_POPULATION * DEBT_COMFORT;
    if comfort <= 0.0 {
        return 0.0;
    }
    (1.0 + f.treasury / comfort).clamp(0.0, 1.0)
}

/// How much of the full delivery premium each faction is posting this
/// tick: planned factions only, reined in by [`thrift`].
pub fn open_contracts(state: &GalaxyState) -> SecondaryMap<FactionId, f64> {
    state
        .factions
        .iter()
        .map(|(id, f)| {
            let share = match f.economy {
                EconomyModel::Planned => thrift(state, id),
                EconomyModel::Market => 0.0,
            };
            (id, share)
        })
        .collect()
}

/// The most a contract can ever pay per unit of `good`.
pub fn max_premium(good: Good) -> f64 {
    good.base_price() * CONTRACT_PREMIUM
}

/// The premium per unit a faction pays a captain for delivering `good` to
/// `settlement`, on top of the price, if the market held `stock` of it:
/// only from factions in `open` (see [`open_contracts`]), and only below
/// target.
pub fn contract_premium(
    state: &GalaxyState,
    open: &SecondaryMap<FactionId, f64>,
    settlement: SettlementId,
    good: Good,
    stock: f64,
) -> f64 {
    let s = &state.settlements[settlement];
    let share = open.get(s.owner).copied().unwrap_or(0.0);
    if share <= 0.0 {
        return 0.0;
    }
    let shortage = (1.0 - stock / s.economy.market.target(good)).clamp(0.0, 1.0);
    good.base_price() * CONTRACT_PREMIUM * shortage * share
}

/// Planning phase, money part: taxes, then welfare.
pub fn taxes_and_welfare(state: &mut GalaxyState) {
    let ids: Vec<SettlementId> = state.settlements.keys().collect();
    let reserves: Vec<f64> = ids.iter().map(|&id| reserve(state, id)).collect();

    for (&id, &reserve) in ids.iter().zip(&reserves) {
        let owner = state.settlements[id].owner;
        let rate = match state.factions[owner].economy {
            EconomyModel::Planned => PLANNED_TAX,
            EconomyModel::Market => MARKET_TAX,
        };
        let tax = rate * (state.settlements[id].economy.treasury - reserve).max(0.0);
        ledger::pay(state, Account::Settlement(id), Account::Faction(owner), tax);
    }

    let keys: Vec<_> = state.companies.keys().collect();
    for key in keys {
        let company = &state.companies[key];
        let Some(home) = state.settlements.get(company.home) else {
            continue;
        };
        let owner = home.owner;
        let capacity: f64 = company
            .fleets
            .iter()
            .map(|f| state.ship_classes[f.class].cargo_t * f64::from(f.ships))
            .sum();
        let working = capacity * CARGO_VALUE_PER_T * WORKING_CAPITAL_HOLDS;
        let tax = COMPANY_TAX * (company.credits - working).max(0.0);
        ledger::pay(state, Account::Company(key), Account::Faction(owner), tax);
    }

    // From a positive balance, welfare goes to every settlement by
    // population, twice as much per head to those below their reserve. A
    // faction without one prints only to relieve settlements below their
    // reserve, by how far below they are, within its debt limit: printing
    // follows need, so debt doesn't grow while settlements are doing fine.
    let weight = |state: &GalaxyState, id: SettlementId, reserve: f64, printing: bool| {
        let s = &state.settlements[id];
        let gap = (reserve - s.economy.treasury).max(0.0);
        if printing {
            gap * RELIEF_SHARE
        } else if gap > 0.0 {
            2.0 * s.economy.population
        } else {
            s.economy.population
        }
    };
    let prints: SecondaryMap<FactionId, bool> = state
        .factions
        .iter()
        .map(|(id, f)| (id, f.treasury <= 0.0))
        .collect();
    let printing = |_: &GalaxyState, owner: FactionId| prints.get(owner).copied().unwrap_or(false);
    let mut totals: SecondaryMap<FactionId, f64> = SecondaryMap::new();
    for (&id, &reserve) in ids.iter().zip(&reserves) {
        let owner = state.settlements[id].owner;
        let w = weight(state, id, reserve, printing(state, owner));
        *totals.entry(owner).unwrap().or_default() += w;
    }
    let budgets: SecondaryMap<FactionId, f64> = state
        .factions
        .iter()
        .map(|(id, f)| {
            let budget = if !prints[id] {
                f.treasury * WELFARE_SHARE
            } else {
                let need = totals.get(id).copied().unwrap_or(0.0) * thrift(state, id);
                need.min(ledger::spendable(state, Account::Faction(id)) * WELFARE_DEBT_SHARE)
            };
            (id, budget)
        })
        .collect();
    for (&id, &reserve) in ids.iter().zip(&reserves) {
        let owner = state.settlements[id].owner;
        let (Some(&budget), Some(&total)) = (budgets.get(owner), totals.get(owner)) else {
            continue;
        };
        if total <= 0.0 {
            continue;
        }
        let pay = budget * weight(state, id, reserve, printing(state, owner)) / total;
        ledger::pay(state, Account::Faction(owner), Account::Settlement(id), pay);
    }
}
