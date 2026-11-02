//! Every movement of money and market goods goes through here.
//!
//! An [`Account`] is anything that holds credits: a settlement's treasury,
//! a company, a faction, a colony ship's purse. Each kind has a floor it can't go below:
//! settlements, companies and colony ships can't go into debt; a faction can, down to a
//! debt limit set by its population, since a faction's negative balance is
//! money it has issued (see [`crate::fiscal`]).
//!
//! [`pay`] moves at most what the payer can spend; [`buy`] and [`sell`]
//! move goods and money together, limited by the stock on offer and what
//! the buyer can afford. Each returns what actually moved, so callers never
//! have to repeat the checks, and there's one place to add taxes, records
//! or other hooks on transactions later.
//!
//! In debug builds, [`check`] verifies after every tick that money is
//! conserved and nothing is below its floor.

use crate::colonize::ColonyShipId;
use crate::economy::Good;
use crate::haul::CompanyId;
use crate::{FactionId, GalaxyState, SettlementId};

/// How deep in debt a faction may go, per unit of its population.
pub const DEBT_PER_POPULATION: f64 = 5_000.0;

/// The most a faction of `population` may owe.
pub fn debt_limit(population: f64) -> f64 {
    population * DEBT_PER_POPULATION
}

/// Something that holds credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Account {
    Settlement(SettlementId),
    Company(CompanyId),
    Faction(FactionId),
    /// A colony ship's purse, carried to its new settlement.
    Colony(ColonyShipId),
}

/// What a market is willing to sell of a good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Supply {
    /// Everything in stock.
    Stock,
    /// Only what's above its own target.
    Surplus,
}

/// An account's balance, or `None` if it no longer exists.
pub fn balance(state: &GalaxyState, account: Account) -> Option<f64> {
    match account {
        Account::Settlement(id) => state.settlements.get(id).map(|s| s.economy.treasury),
        Account::Company(id) => state.companies.get(id).map(|c| c.credits),
        Account::Faction(id) => state.factions.get(id).map(|f| f.treasury),
        Account::Colony(id) => state.colony_ships.get(id).map(|c| c.credits),
    }
}

fn balance_mut(state: &mut GalaxyState, account: Account) -> Option<&mut f64> {
    match account {
        Account::Settlement(id) => state
            .settlements
            .get_mut(id)
            .map(|s| &mut s.economy.treasury),
        Account::Company(id) => state.companies.get_mut(id).map(|c| &mut c.credits),
        Account::Faction(id) => state.factions.get_mut(id).map(|f| &mut f.treasury),
        Account::Colony(id) => state.colony_ships.get_mut(id).map(|c| &mut c.credits),
    }
}

/// The lowest balance an account may reach.
pub fn floor(state: &GalaxyState, account: Account) -> f64 {
    match account {
        Account::Settlement(_) | Account::Company(_) | Account::Colony(_) => 0.0,
        Account::Faction(id) => -debt_limit(state.factions.get(id).map_or(0.0, |f| f.population)),
    }
}

/// How much an account can pay right now.
pub fn spendable(state: &GalaxyState, account: Account) -> f64 {
    balance(state, account).map_or(0.0, |b| (b - floor(state, account)).max(0.0))
}

/// Move up to `credits` from one account to another; returns how much
/// moved. Nothing moves if either account is gone.
pub fn pay(state: &mut GalaxyState, from: Account, to: Account, credits: f64) -> f64 {
    if from == to || balance(state, to).is_none() {
        return 0.0;
    }
    let amount = credits.min(spendable(state, from)).max(0.0);
    if amount <= 0.0 {
        return 0.0;
    }
    *balance_mut(state, from).expect("spendable was positive") -= amount;
    *balance_mut(state, to).expect("checked above") += amount;
    amount
}

/// What `market` can offer of `good` under `supply`.
pub fn available(state: &GalaxyState, market: SettlementId, good: Good, supply: Supply) -> f64 {
    let m = &state.settlements[market].economy.market;
    match supply {
        Supply::Stock => m.stock[good].max(0.0),
        Supply::Surplus => m.surplus(good),
    }
}

/// `buyer` buys up to `wanted` of `good` from `market` at `price` a unit,
/// spending at most `spend_share` of what it can spend. The goods leave the
/// market; the caller puts them wherever they go. Returns the amount
/// bought.
#[allow(clippy::too_many_arguments)]
pub fn buy(
    state: &mut GalaxyState,
    buyer: Account,
    market: SettlementId,
    good: Good,
    wanted: f64,
    price: f64,
    supply: Supply,
    spend_share: f64,
) -> f64 {
    let mut amount = wanted.min(available(state, market, good, supply));
    if price > 0.0 && buyer != Account::Settlement(market) {
        amount = amount.min(spendable(state, buyer) * spend_share / price);
    }
    if amount <= 0.0 {
        return 0.0;
    }
    state.settlements[market].economy.market.stock[good] -= amount;
    if price > 0.0 {
        let paid = pay(state, buyer, Account::Settlement(market), amount * price);
        debug_assert!((paid - amount * price).abs() <= 1e-6 * paid.max(1.0));
    }
    amount
}

/// `market` buys up to `offered` of `good` from `seller` at `price` a unit,
/// spending at most `spend_share` of what it can spend. The goods arrive
/// in the market; the caller takes them from wherever they were. Returns
/// the amount sold.
pub fn sell(
    state: &mut GalaxyState,
    seller: Account,
    market: SettlementId,
    good: Good,
    offered: f64,
    price: f64,
    spend_share: f64,
) -> f64 {
    let mut amount = offered;
    if price > 0.0 {
        amount = amount.min(spendable(state, Account::Settlement(market)) * spend_share / price);
    }
    if amount <= 0.0 {
        return 0.0;
    }
    state.settlements[market].economy.market.stock[good] += amount;
    if price > 0.0 {
        pay(state, Account::Settlement(market), seller, amount * price);
    }
    amount
}

/// Use up to `wanted` of `good` from a market's own stock, with no money
/// changing hands (construction materials, say). Returns the amount used.
pub fn consume(state: &mut GalaxyState, market: SettlementId, good: Good, wanted: f64) -> f64 {
    let stock = &mut state.settlements[market].economy.market.stock[good];
    let amount = wanted.min(stock.max(0.0)).max(0.0);
    *stock -= amount;
    amount
}

/// Goods handed to a market for nothing: cargo nobody would buy.
pub fn donate(state: &mut GalaxyState, market: SettlementId, good: Good, amount: f64) {
    state.settlements[market].economy.market.stock[good] += amount.max(0.0);
}

/// All money anywhere: settlements, companies, factions and colony ships.
pub fn total_money(state: &GalaxyState) -> f64 {
    let settlements: f64 = state.settlements.values().map(|s| s.economy.treasury).sum();
    let companies: f64 = state.companies.values().map(|c| c.credits).sum();
    let factions: f64 = state.factions.values().map(|f| f.treasury).sum();
    let colonies: f64 = state.colony_ships.values().map(|c| c.credits).sum();
    settlements + companies + factions + colonies
}

/// Panic if money wasn't conserved since `before`, or any settlement,
/// company, colony ship or stock is below zero. Called by the tick in debug builds.
pub fn check(state: &GalaxyState, before: f64) {
    const EPS: f64 = 1e-6;
    let after = total_money(state);
    assert!(
        (after - before).abs() <= 1e-7 * before.abs().max(1.0),
        "money not conserved: {before} -> {after}"
    );
    for s in state.settlements.values() {
        assert!(
            s.economy.treasury >= -EPS,
            "{} treasury {}",
            s.name,
            s.economy.treasury
        );
        for good in Good::ALL {
            assert!(
                s.economy.market.stock[good] >= -EPS,
                "{} {} stock {}",
                s.name,
                good.name(),
                s.economy.market.stock[good]
            );
        }
    }
    for c in state.companies.values() {
        assert!(c.credits >= -EPS, "{} credits {}", c.name, c.credits);
    }
    for c in state.colony_ships.values() {
        assert!(
            c.credits >= -EPS,
            "colony ship to {} credits {}",
            c.name,
            c.credits
        );
    }
    // Faction floors aren't re-checked here: a faction's debt limit shrinks
    // when its population does, so a balance that was within the limit
    // when it was spent can sit below it later. `pay` enforces the limit
    // whenever money moves.
}
