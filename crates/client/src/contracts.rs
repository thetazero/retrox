//! The player's freight contracts: taking one off a settlement's dispatch
//! board, delivering it, and what happens when it's late or abandoned.
//!
//! The board itself, and why each contract is on it, is the sim's
//! ([`retrox_sim::freight`]). Here the money and goods move between the
//! player and the two settlements' books, as described there:
//! - accept: the goods leave the origin's market, the consignee buys them
//!   and escrows the fee, and the player leaves a deposit of their value;
//! - deliver: the goods reach the consignee's market, the deposit comes
//!   back with the fee, less a late penalty;
//! - fail: the consignee keeps the deposit and takes back the fee; the
//!   goods are the player's to sell.

use retrox_sim::freight::{self, Offer, Pending};
use retrox_sim::{GalaxyState, GalaxyTime, SettlementId};

use crate::hangar::Player;

/// A contract the player has taken.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Contract {
    pub offer: Offer,
    pub accepted: GalaxyTime,
}

impl Contract {
    pub fn fails_on(&self) -> GalaxyTime {
        freight::fails_on(self.offer.deadline)
    }
}

/// What the player already carries, for the board.
pub fn pending(player: &Player) -> Vec<Pending> {
    player
        .contracts
        .iter()
        .map(|c| Pending {
            to: c.offer.to,
            good: c.offer.good,
            amount: c.offer.amount,
        })
        .collect()
}

/// Why the player can't take an offer right now, if they can't.
pub fn refusal(player: &Player, capacity_t: f64, offer: &Offer) -> Option<String> {
    let free = capacity_t - player.cargo_t();
    if offer.amount > free + 1e-9 {
        Some(format!("needs {:.0} t free in the hold", offer.amount))
    } else if offer.value > player.credits {
        Some(format!(
            "needs a deposit of {}",
            crate::hud::credits(offer.value)
        ))
    } else {
        None
    }
}

/// Take a contract from the board of the settlement it loads at.
pub fn accept(
    player: &mut Player,
    capacity_t: f64,
    galaxy: &mut GalaxyState,
    offer: &Offer,
) -> Result<String, String> {
    if let Some(why) = refusal(player, capacity_t, offer) {
        return Err(format!("Can't take it: {why}"));
    }
    let (Some(from), Some(to)) = (
        galaxy.settlements.get(offer.from),
        galaxy.settlements.get(offer.to),
    ) else {
        return Err("That contract is gone".into());
    };
    if from.economy.market.stock[offer.good] + 1e-9 < offer.amount {
        return Err("The goods are no longer here".into());
    }
    if to.economy.treasury < offer.value + offer.fee {
        return Err(format!("{} can no longer pay for it", to.name));
    }
    let to_name = to.name.clone();
    let from = &mut galaxy.settlements[offer.from].economy;
    from.market.stock[offer.good] -= offer.amount;
    from.treasury += offer.value;
    // The consignee pays for the goods and escrows the fee; the deposit
    // covers the goods.
    galaxy.settlements[offer.to].economy.treasury -= offer.fee;
    player.credits -= offer.value;
    player.contracts.push(Contract {
        offer: offer.clone(),
        accepted: galaxy.time,
    });
    Ok(format!(
        "Contract: {:.0} t of {} to {to_name} by {}",
        offer.amount,
        offer.good.name(),
        offer.deadline
    ))
}

/// Deliver every contract bound for `here`. Returns what to tell the player.
pub fn deliver(player: &mut Player, galaxy: &mut GalaxyState, here: SettlementId) -> Vec<String> {
    let today = galaxy.time;
    let mut said = Vec::new();
    let mut i = 0;
    while i < player.contracts.len() {
        if player.contracts[i].offer.to != here {
            i += 1;
            continue;
        }
        let c = player.contracts.remove(i);
        let o = &c.offer;
        let owner = galaxy.settlements[here].owner;
        player.standing.change(owner, crate::standing::DELIVERED);
        let pay = freight::pay_on(o.fee, o.deadline, today);
        let economy = &mut galaxy.settlements[here].economy;
        economy.market.stock[o.good] += o.amount;
        // The consignee hands back the deposit and pays the fee out of
        // escrow; what's withheld for lateness stays with it.
        economy.treasury += o.fee - pay - o.value;
        player.credits += o.value + pay;
        let late = today.days().saturating_sub(o.deadline.days());
        // The deposit comes back too: say so, so the purse's jump adds up.
        let deposit = crate::hud::credits(o.value);
        said.push(if late == 0 {
            format!(
                "Delivered {:.0} t of {}: paid {}, and the {deposit} deposit back",
                o.amount,
                o.good.name(),
                crate::hud::credits(pay)
            )
        } else {
            format!(
                "Delivered {late} d late: paid {} of {}, and the {deposit} deposit back",
                crate::hud::credits(pay),
                crate::hud::credits(o.fee)
            )
        });
    }
    said
}

/// Give up contract `index`: the consignee keeps the deposit and its fee;
/// the goods stay in the hold as the player's own.
pub fn abandon(player: &mut Player, galaxy: &mut GalaxyState, index: usize) -> Option<String> {
    if index >= player.contracts.len() {
        return None;
    }
    let c = player.contracts.remove(index);
    let o = &c.offer;
    if let Some(to) = galaxy.settlements.get_mut(o.to) {
        to.economy.treasury += o.fee;
        // The faction that posted it remembers (DESIGN §7.2.2).
        player.standing.change(to.owner, crate::standing::FAILED);
    }
    player.cargo[o.good] += o.amount;
    Some(format!(
        "Contract failed: forfeit the {} deposit, the {:.0} t of {} are yours",
        crate::hud::credits(o.value),
        o.amount,
        o.good.name()
    ))
}

/// Fail every contract past its grace period, and any whose consignee is
/// gone. Returns what to tell the player.
pub fn expire(player: &mut Player, galaxy: &mut GalaxyState) -> Vec<String> {
    let today = galaxy.time;
    let mut said = Vec::new();
    let mut i = 0;
    while i < player.contracts.len() {
        let c = &player.contracts[i];
        if today >= c.fails_on() || galaxy.settlements.get(c.offer.to).is_none() {
            said.extend(abandon(player, galaxy, i));
        } else {
            i += 1;
        }
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use retrox_sim::economy::Good;

    /// Two settlements in one galaxy: the first with food to spare, the
    /// second short of it.
    fn setup() -> (GalaxyState, Offer) {
        let mut galaxy = retrox_gen::generate(1, &retrox_gen::GalaxyConfig::default());
        let ids: Vec<SettlementId> = galaxy.settlements.keys().take(2).collect();
        let (from, to) = (ids[0], ids[1]);
        galaxy.settlements[from].economy.market.stock[Good::Food] = 1.0e4;
        galaxy.settlements[from].economy.treasury = 1.0e5;
        galaxy.settlements[to].economy.treasury = 1.0e5;
        let offer = Offer {
            from,
            to,
            good: Good::Food,
            amount: 10.0,
            jumps: 2,
            trip_days: 12.0,
            deadline: GalaxyTime::from_days(galaxy.time.days() + 20),
            value: 150.0,
            fee: 2000.0,
            shortage: 0.6,
            urgent: true,
        };
        (galaxy, offer)
    }

    fn money(galaxy: &GalaxyState, player: &Player, offer: &Offer) -> f64 {
        let escrow: f64 = player.contracts.iter().map(|c| c.offer.fee).sum();
        galaxy.settlements[offer.from].economy.treasury
            + galaxy.settlements[offer.to].economy.treasury
            + player.credits
            + escrow
    }

    #[test]
    fn deliver_on_time_pays_the_fee_and_returns_the_deposit() {
        let (mut galaxy, offer) = setup();
        let mut player = Player::new();
        let (credits, total) = (player.credits, money(&galaxy, &player, &offer));
        let stock_to = galaxy.settlements[offer.to].economy.market.stock[Good::Food];
        accept(&mut player, 12.0, &mut galaxy, &offer).unwrap();
        assert_eq!(player.cargo_t(), 10.0);
        assert_eq!(player.credits, credits - offer.value);
        assert!((money(&galaxy, &player, &offer) - total).abs() < 1e-6);
        // Contract cargo can't be sold off at a market.
        assert_eq!(player.cargo[Good::Food], 0.0);
        // Nothing happens at the wrong place.
        assert!(deliver(&mut player, &mut galaxy, offer.from).is_empty());
        let said = deliver(&mut player, &mut galaxy, offer.to);
        assert_eq!(said.len(), 1);
        assert!(player.contracts.is_empty());
        assert_eq!(player.credits, credits + offer.fee);
        assert_eq!(
            galaxy.settlements[offer.to].economy.market.stock[Good::Food],
            stock_to + offer.amount
        );
        assert!((money(&galaxy, &player, &offer) - total).abs() < 1e-6);
    }

    #[test]
    fn late_delivery_pays_less_and_failure_forfeits_the_deposit() {
        let (mut galaxy, offer) = setup();
        let mut player = Player::new();
        let (credits, total) = (player.credits, money(&galaxy, &player, &offer));
        accept(&mut player, 12.0, &mut galaxy, &offer).unwrap();
        // Three days late: 30% off.
        galaxy.time = GalaxyTime::from_days(offer.deadline.days() + 3);
        assert!(expire(&mut player, &mut galaxy).is_empty());
        deliver(&mut player, &mut galaxy, offer.to);
        assert!((player.credits - (credits + offer.fee * 0.7)).abs() < 1e-6);
        assert!((money(&galaxy, &player, &offer) - total).abs() < 1e-6);

        let (mut galaxy, offer) = setup();
        let mut player = Player::new();
        let total = money(&galaxy, &player, &offer);
        accept(&mut player, 12.0, &mut galaxy, &offer).unwrap();
        galaxy.time = freight::fails_on(offer.deadline);
        assert_eq!(expire(&mut player, &mut galaxy).len(), 1);
        assert!(player.contracts.is_empty());
        assert_eq!(player.credits, credits - offer.value);
        // The goods are the player's now, and nobody gained or lost money.
        assert_eq!(player.cargo[Good::Food], offer.amount);
        assert!((money(&galaxy, &player, &offer) - total).abs() < 1e-6);
    }

    #[test]
    fn needs_room_and_a_deposit() {
        let (mut galaxy, offer) = setup();
        let mut player = Player::new();
        assert!(accept(&mut player, 5.0, &mut galaxy, &offer).is_err());
        player.credits = 10.0;
        assert!(accept(&mut player, 12.0, &mut galaxy, &offer).is_err());
        assert!(player.contracts.is_empty());
    }
}
