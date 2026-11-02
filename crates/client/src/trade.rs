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

/// What a market will sell a private trader of `good` (issue #160): all
/// its stock in a market economy; in a planned one only its surplus over
/// the plan's target, as NPC haulers buy (`Market::surplus`). Planned
/// factions pay a delivery premium for goods short of target, so selling
/// below target would let a trader buy a premium's worth back and sell it
/// again.
pub fn for_sale(market: &Market, model: EconomyModel, good: Good) -> f64 {
    match model {
        EconomyModel::Market => market.stock[good],
        EconomyModel::Planned => market.surplus(good),
    }
}

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
    let mut n = for_sale(market, model, good)
        .min(capacity_t - player.cargo_t())
        .max(0.0)
        .floor();
    // Prices rise as you buy, so step down until it's affordable.
    while n > 0.0 && buy_cost(market, model, good, n) > player.credits {
        n = (n * 0.9).floor().min(n - 1.0);
    }
    n
}

/// Below this a hold's share of a good is float dust, not cargo.
pub const DUST_T: f64 = 1e-6;

/// Buy (positive `amount`) or sell (negative) a good. Purchases are whole
/// tonnes; a sale may take a mined fraction, so selling all of a good
/// empties the hold of it (issue #175).
pub fn trade(
    player: &mut Player,
    capacity_t: f64,
    economy: &mut LocalEconomy,
    model: EconomyModel,
    good: Good,
    amount: f64,
) -> Result<String, String> {
    let amount = if amount > 0.0 { amount.round() } else { amount };
    let market = &economy.market;
    if amount > 0.0 {
        let available = for_sale(market, model, good).floor();
        if amount > available {
            return Err(match model {
                EconomyModel::Planned => format!(
                    "The plan releases only {available:.0} t of {}: the rest is spoken for",
                    good.name()
                ),
                EconomyModel::Market => {
                    format!("The market only has {available:.0} t of {}", good.name())
                }
            });
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
    } else if amount < -DUST_T {
        let have = player.cargo[good];
        // Selling all of it, give or take float dust, sells exactly that.
        let amount = if (-amount - have).abs() < DUST_T {
            have
        } else {
            -amount
        };
        if amount > have + DUST_T {
            return Err(format!(
                "You only carry {} of {}",
                crate::hud::tonnes(have),
                good.name()
            ));
        }
        let value = sell_value(market, model, good, amount);
        if value > economy.treasury {
            return Err("The settlement can't afford to buy that".into());
        }
        player.credits += value;
        player.cargo[good] -= amount;
        if player.cargo[good] < DUST_T {
            player.cargo[good] = 0.0;
        }
        economy.market.stock[good] += amount;
        economy.treasury -= value;
        Ok(format!(
            "Sold {} of {} for {}",
            crate::hud::tonnes(amount),
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

    /// Issue #175: a mined lot isn't whole tonnes, and selling all of it
    /// leaves nothing behind.
    #[test]
    fn selling_all_of_a_mined_lot_empties_the_hold() {
        let (mut p, mut eco) = (Player::new(), market());
        // Mining adds a little at a time.
        for _ in 0..21 {
            p.cargo[Good::Food] += 0.1;
        }
        let have = p.cargo[Good::Food];
        let start = p.credits;
        let text = trade(
            &mut p,
            12.0,
            &mut eco,
            EconomyModel::Market,
            Good::Food,
            -have,
        )
        .unwrap();
        assert!(text.starts_with("Sold 2.1 t of food"), "{text}");
        assert_eq!(p.cargo_t(), 0.0);
        assert!(p.credits > start);
        // A remnant under a tonne sells on its own.
        p.cargo[Good::Food] = 0.1;
        trade(
            &mut p,
            12.0,
            &mut eco,
            EconomyModel::Market,
            Good::Food,
            -0.1,
        )
        .unwrap();
        assert_eq!(p.cargo_t(), 0.0);
    }

    #[test]
    fn tonnes_read_like_the_mining_hold() {
        assert_eq!(crate::hud::tonnes(2.0), "2 t");
        assert_eq!(crate::hud::tonnes(2.1000000001), "2.1 t");
        assert_eq!(crate::hud::tonnes(0.1), "0.1 t");
        assert_eq!(crate::hud::tonnes(11.98), "12 t");
    }

    #[test]
    fn buying_more_costs_more_per_tonne() {
        let eco = market();
        let one = buy_cost(&eco.market, EconomyModel::Market, Good::Food, 1.0);
        let many = buy_cost(&eco.market, EconomyModel::Market, Good::Food, 200.0) / 200.0;
        assert!(many > one);
    }

    /// A planned market sells only its surplus over target (issue #160),
    /// so a trader can't buy back below target what its faction pays a
    /// premium to have delivered.
    #[test]
    fn a_planned_market_sells_only_its_surplus() {
        let mut eco = market();
        let target = eco.market.target(Good::Food);
        eco.market.stock[Good::Food] = target + 5.0;
        let p = Player::new();
        let n = max_buy(&p, 100.0, &eco, EconomyModel::Planned, Good::Food);
        assert_eq!(n, 5.0);
        let mut p = Player::new();
        assert!(
            trade(
                &mut p,
                100.0,
                &mut eco,
                EconomyModel::Planned,
                Good::Food,
                6.0
            )
            .is_err()
        );
        eco.market.stock[Good::Food] = target - 1.0;
        assert_eq!(
            max_buy(&p, 100.0, &eco, EconomyModel::Planned, Good::Food),
            0.0
        );
        // A market economy sells all it has.
        assert!(max_buy(&p, 100.0, &eco, EconomyModel::Market, Good::Food) >= 12.0);
    }
}
