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

/// Which factions are posting delivery contracts this tick: planned
/// factions within their debt limit.
pub fn open_contracts(state: &GalaxyState) -> SecondaryMap<FactionId, bool> {
    state
        .factions
        .iter()
        .map(|(id, f)| {
            let solvent = ledger::spendable(state, Account::Faction(id)) > 0.0;
            (id, f.economy == EconomyModel::Planned && solvent)
        })
        .collect()
}

/// The premium per unit a faction pays a captain for delivering `good` to
/// `settlement`, on top of the price, if the market held `stock` of it:
/// only from factions in `open` (see [`open_contracts`]), and only below
/// target.
pub fn contract_premium(
    state: &GalaxyState,
    open: &SecondaryMap<FactionId, bool>,
    settlement: SettlementId,
    good: Good,
    stock: f64,
) -> f64 {
    let s = &state.settlements[settlement];
    if !open.get(s.owner).copied().unwrap_or(false) {
        return 0.0;
    }
    let shortage = (1.0 - stock / s.economy.market.target(good)).clamp(0.0, 1.0);
    good.base_price() * CONTRACT_PREMIUM * shortage
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

    // Welfare goes to every settlement by population, twice as much per
    // head to those below their reserve.
    let weight = |state: &GalaxyState, id: SettlementId, reserve: f64| {
        let s = &state.settlements[id];
        s.economy.population
            * if s.economy.treasury < reserve {
                2.0
            } else {
                1.0
            }
    };
    let mut totals: SecondaryMap<FactionId, f64> = SecondaryMap::new();
    for (&id, &reserve) in ids.iter().zip(&reserves) {
        let owner = state.settlements[id].owner;
        *totals.entry(owner).unwrap().or_default() += weight(state, id, reserve);
    }
    let budgets: SecondaryMap<FactionId, f64> = state
        .factions
        .iter()
        .filter(|(_, f)| f.treasury > 0.0)
        .map(|(id, f)| (id, f.treasury * WELFARE_SHARE))
        .collect();
    for (&id, &reserve) in ids.iter().zip(&reserves) {
        let owner = state.settlements[id].owner;
        let (Some(&budget), Some(&total)) = (budgets.get(owner), totals.get(owner)) else {
            continue;
        };
        if total <= 0.0 {
            continue;
        }
        let pay = budget * weight(state, id, reserve) / total;
        ledger::pay(state, Account::Faction(owner), Account::Settlement(id), pay);
    }
}
