//! Trading goods with a settlement's market, into and out of the ship's
//! hold.
//!
//! Prices follow the market's own pricing ([`Market::price_at`]): a
//! purchase is priced at the stock level halfway through it, so buying a
//! lot pushes the price up as you go (in a market economy; planned ones
//! keep a fixed price). Selling pays [`SELL_SHARE`] of that, so buying and
//! selling in the same place loses money. Goods are counted in tonnes.
//! Money moves with the settlement's treasury.

use retrox_sim::EconomyModel;
use retrox_sim::economy::{Good, LocalEconomy, Market};

use crate::hangar::Player;

/// Share of the market price a seller gets.
pub const SELL_SHARE: f64 = 0.9;

/// What buying `amount` costs, in total.
pub fn buy_cost(market: &Market, model: EconomyModel, good: Good, amount: f64) -> f64 {
    market.price_at(good, market.stock[good] - amount / 2.0, model) * amount
}

/// What selling `amount` pays, in total.
pub fn sell_value(market: &Market, model: EconomyModel, good: Good, amount: f64) -> f64 {
    market.price_at(good, market.stock[good] + amount / 2.0, model) * amount * SELL_SHARE
}

/// The most whole tonnes the player can buy: limited by the market's
/// stock, the free hold and their credits.
pub fn max_buy(
    player: &Player,
    capacity_t: f64,
    economy: &LocalEconomy,
    model: EconomyModel,
    good: Good,
) -> f64 {
    let market = &economy.market;
    let mut n = market.stock[good]
        .min(capacity_t - player.cargo_t())
        .max(0.0)
        .floor();
    // Prices rise as you buy, so step down until it's affordable.
    while n > 0.0 && buy_cost(market, model, good, n) > player.credits {
        n = (n * 0.9).floor().min(n - 1.0);
    }
    n
}

/// Buy (positive `amount`) or sell (negative) whole tonnes of a good.
pub fn trade(
    player: &mut Player,
    capacity_t: f64,
    economy: &mut LocalEconomy,
    model: EconomyModel,
    good: Good,
    amount: f64,
) -> Result<String, String> {
    let amount = amount.round();
    let market = &economy.market;
    if amount > 0.0 {
        if amount > market.stock[good].floor() {
            return Err(format!(
                "The market only has {:.0} t of {}",
                market.stock[good].floor(),
                good.name()
            ));
        }
        let free = capacity_t - player.cargo_t();
        if amount > free + 1e-9 {
            return Err(format!("The hold only has room for {free:.0} t"));
        }
        let cost = buy_cost(market, model, good, amount);
        if cost > player.credits {
            return Err(format!(
                "Not enough credits: that costs {}",
                crate::hud::credits(cost)
            ));
        }
        player.credits -= cost;
        player.cargo[good] += amount;
        economy.market.stock[good] -= amount;
        economy.treasury += cost;
        Ok(format!(
            "Bought {amount:.0} t of {} for {}",
            good.name(),
            crate::hud::credits(cost)
        ))
    } else if amount < 0.0 {
        let amount = -amount;
        if amount > player.cargo[good] + 1e-9 {
            return Err(format!(
                "You only carry {:.0} t of {}",
                player.cargo[good],
                good.name()
            ));
        }
        let value = sell_value(market, model, good, amount);
        if value > economy.treasury {
            return Err("The settlement can't afford to buy that".into());
        }
        player.credits += value;
        player.cargo[good] -= amount;
        economy.market.stock[good] += amount;
        economy.treasury -= value;
        Ok(format!(
            "Sold {amount:.0} t of {} for {}",
            good.name(),
            crate::hud::credits(value)
        ))
    } else {
        Err("Nothing to trade".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market() -> LocalEconomy {
        let mut economy = LocalEconomy {
            treasury: 1.0e6,
            population: 100.0,
            ..Default::default()
        };
        economy.refresh_demand();
        economy.market.stock[Good::Food] = 500.0;
        economy.update_prices(EconomyModel::Market);
        economy
    }

    #[test]
    fn buying_fills_the_hold_and_pays_the_treasury() {
        let (mut p, mut eco) = (Player::new(), market());
        let cost = buy_cost(&eco.market, EconomyModel::Market, Good::Food, 10.0);
        trade(
            &mut p,
            12.0,
            &mut eco,
            EconomyModel::Market,
            Good::Food,
            10.0,
        )
        .unwrap();
        assert_eq!(p.cargo[Good::Food], 10.0);
        assert_eq!(eco.market.stock[Good::Food], 490.0);
        assert!((p.credits - (crate::hangar::STARTING_CREDITS - cost)).abs() < 1e-6);
        // The hold is full at 12 t.
        assert!(
            trade(
                &mut p,
                12.0,
                &mut eco,
                EconomyModel::Market,
                Good::Food,
                5.0
            )
            .is_err()
        );
        assert_eq!(
            max_buy(&p, 12.0, &eco, EconomyModel::Market, Good::Food),
            2.0
        );
    }

    #[test]
    fn selling_back_loses_money() {
        let (mut p, mut eco) = (Player::new(), market());
        let start = p.credits;
        trade(
            &mut p,
            12.0,
            &mut eco,
            EconomyModel::Market,
            Good::Food,
            10.0,
        )
        .unwrap();
        trade(
            &mut p,
            12.0,
            &mut eco,
            EconomyModel::Market,
            Good::Food,
            -10.0,
        )
        .unwrap();
        assert_eq!(p.cargo_t(), 0.0);
        assert!(p.credits < start);
        assert!(
            trade(
                &mut p,
                12.0,
                &mut eco,
                EconomyModel::Market,
                Good::Food,
                -1.0
            )
            .is_err()
        );
    }

    #[test]
    fn buying_more_costs_more_per_tonne() {
        let eco = market();
        let one = buy_cost(&eco.market, EconomyModel::Market, Good::Food, 1.0);
        let many = buy_cost(&eco.market, EconomyModel::Market, Good::Food, 200.0) / 200.0;
        assert!(many > one);
    }
}
