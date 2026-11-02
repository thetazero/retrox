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

use serde::{Deserialize, Serialize};
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
    let comfort = ledger::debt_limit(f.population) * DEBT_COMFORT;
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

/// A faction's welfare round. Each day's books pay out what the day before
/// set aside, shared by the claims staked the day before, and stake
/// today's claims for tomorrow: one pass over settlements, with the sums
/// kept here. Saved with the faction, so a loaded game pays the same.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Welfare {
    /// Whether the faction had no positive balance, and so prints money
    /// for relief only.
    pub printing: bool,
    /// What it pays out today.
    pub budget: f64,
    /// Its settlements' claims, which share the budget.
    pub claims: f64,
    /// Claims staked today, if it pays from a positive balance (by
    /// population) and if it prints (by how far below their reserve they
    /// are). Which applies is only known once every tax is in.
    pub staked_paying: f64,
    pub staked_printing: f64,
    /// Its settlements' population, added up as the books go; it becomes
    /// [`Faction::population`](crate::Faction::population) at the end, so
    /// the debt limit holds still meanwhile.
    pub counted: f64,
}

/// What a settlement holding `treasury` claims of its faction's welfare:
/// by population (twice over below its reserve), or only by how far below
/// its reserve it is if the faction is printing.
fn claim(population: f64, treasury: f64, reserve: f64, printing: bool) -> f64 {
    let gap = (reserve - treasury).max(0.0);
    if printing {
        gap * RELIEF_SHARE
    } else if gap > 0.0 {
        2.0 * population
    } else {
        population
    }
}

/// Daily books, a settlement's money: it pays its tax, takes its share of
/// welfare, and stakes its claim for tomorrow.
pub(crate) fn settle(state: &mut GalaxyState, id: SettlementId) {
    let owner = state.settlements[id].owner;
    let Some(faction) = state.factions.get(owner) else {
        return;
    };
    let rate = match faction.economy {
        EconomyModel::Planned => PLANNED_TAX,
        EconomyModel::Market => MARKET_TAX,
    };
    let welfare = faction.welfare;
    let reserve = reserve(state, id);
    let economy = &state.settlements[id].economy;
    let population = economy.population;
    // The tax is always within what the settlement holds, so its treasury
    // after tax is known before any money moves.
    let tax = rate * (economy.treasury - reserve).max(0.0);
    let treasury = economy.treasury - tax;
    let benefit = if welfare.claims > 0.0 {
        welfare.budget * claim(population, treasury, reserve, welfare.printing) / welfare.claims
    } else {
        0.0
    };
    // Tax and welfare settle as one transfer, whichever way it runs: the
    // faction can pay the difference exactly when it could pay welfare
    // out of its balance plus the tax.
    let (settlement, faction) = (Account::Settlement(id), Account::Faction(owner));
    if benefit >= tax {
        ledger::pay(state, faction, settlement, benefit - tax);
    } else {
        ledger::pay(state, settlement, faction, tax - benefit);
    }
    let w = &mut state.factions[owner].welfare;
    w.staked_paying += claim(population, treasury, reserve, false);
    w.staked_printing += claim(population, treasury, reserve, true);
}

/// Daily books, after every settlement: companies pay their taxes, then
/// each faction counts its people and, with every tax in, sets aside
/// tomorrow's welfare.
pub(crate) fn close_books(state: &mut GalaxyState) {
    for i in 0..state.companies.slot_count() {
        let Some(key) = state.companies.key_at(i) else {
            continue;
        };
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

    for i in 0..state.factions.slot_count() {
        let Some(id) = state.factions.key_at(i) else {
            continue;
        };
        let f = &mut state.factions[id];
        f.population = f.welfare.counted;
        let printing = f.treasury <= 0.0;
        let (claims, budget) = if !printing {
            (f.welfare.staked_paying, f.treasury * WELFARE_SHARE)
        } else {
            let claims = f.welfare.staked_printing;
            let need = claims * thrift(state, id);
            let room = ledger::spendable(state, Account::Faction(id)) * WELFARE_DEBT_SHARE;
            (claims, need.min(room))
        };
        state.factions[id].welfare = Welfare {
            printing,
            budget,
            claims,
            ..Welfare::default()
        };
    }
}
