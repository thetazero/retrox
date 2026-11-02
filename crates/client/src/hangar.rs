//! The hangar: the player's ships and money, and what they can do with
//! them while docked.
//!
//! - **Shipyard** (settlements with a shipyard): buy a stock ship off the
//!   lot, if the yard has built enough hull for it; sell ships back. The
//!   ship you were flying stays in storage here.
//! - **Outfitting** (industrial or large settlements): fit, swap and strip
//!   modules, and swap the sections of medium and large ships. Swapping a
//!   section sells the modules that were in it.
//! - **Storage** (anywhere): board any ship stored at this settlement.
//!
//! Money changes hands with the settlement's treasury, so the player's
//! spending enters the economy like anyone else's. Parts and ships sell
//! back at [`price::RESALE`] of their price, and only as far as the
//! treasury can pay. Every job takes galaxy time (DESIGN §3).

use retrox_content::ships::{price, stock};
use retrox_sim::SettlementId;
use retrox_sim::economy::{FacilityKind, HULL_T_PER_RUN};
use retrox_sim::economy::{GoodMap, LocalEconomy};
use retrox_sim::ship::{Hull, Loadout, Module, Mount, Section, Size};
use serde::{Deserialize, Serialize};

const HOUR: f64 = 3600.0;
/// Buying a ship off the lot.
const BUY_SHIP_SECONDS: f64 = 24.0 * HOUR;
/// Settlements this populous (thousands) have an outfitter even without
/// industry.
const OUTFITTER_POPULATION: f64 = 50.0;
/// A new pilot owns the courier and a small purse: enough for a first
/// contract's deposit or a hold of cargo, not for another ship
/// (DESIGN §7.8).
pub const STARTING_CREDITS: f64 = 2_000.0;
pub const STARTING_HULL: &str = "courier";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnedShip {
    pub name: String,
    /// Saved as the ids of its parts (see [`crate::save::loadout`]).
    #[serde(with = "crate::save::loadout")]
    pub fit: Loadout,
    /// Hull damage from fighting, until repaired (see [`crate::combat`]).
    #[serde(default)]
    pub damage: crate::combat::Damage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredShip {
    pub ship: OwnedShip,
    pub at: SettlementId,
}

/// The player's money and ships.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub credits: f64,
    /// The ship being flown.
    pub ship: OwnedShip,
    pub stored: Vec<StoredShip>,
    /// Goods in the hold of the ship being flown, tonnes.
    pub cargo: GoodMap<f64>,
    /// Freight contracts taken and not yet delivered. Their goods ride in
    /// the hold too, but aren't the player's to sell.
    pub contracts: Vec<crate::contracts::Contract>,
    /// Ships bought so far, for naming.
    bought: u32,
    /// What the player has done, for the first-steps notes.
    #[serde(default)]
    pub progress: crate::guide::Progress,
    /// Standing with each faction (DESIGN §7.2.2).
    #[serde(default)]
    pub standing: crate::standing::Standing,
    /// Where the player is taking their own goods to sell (issue #160):
    /// set by a trade run or a GO on the trade screen, cleared when the
    /// hold's own goods are sold.
    #[serde(default)]
    pub sell_at: Option<retrox_sim::SettlementId>,
}

impl Player {
    pub fn new() -> Self {
        let fit = stock(STARTING_HULL).expect("starting hull exists");
        Self {
            credits: STARTING_CREDITS,
            ship: OwnedShip {
                name: format!("{} No. 1", fit.hull().name),
                fit,
                damage: Default::default(),
            },
            stored: Vec::new(),
            cargo: GoodMap::default(),
            contracts: Vec::new(),
            bought: 1,
            standing: Default::default(),
            progress: Default::default(),
            sell_at: None,
        }
    }

    /// Tonnes in the hold, the player's own goods and contract cargo.
    pub fn cargo_t(&self) -> f64 {
        self.cargo.0.iter().sum::<f64>() + self.contract_t()
    }

    /// Tonnes in the hold under contract.
    pub fn contract_t(&self) -> f64 {
        self.contracts.iter().map(|c| c.offer.amount).sum()
    }
}

/// Something to do in the hangar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Job {
    /// Buy the stock fit of a hull; the current ship goes into storage.
    BuyShip(&'static Hull),
    /// Sell a stored ship (by index into [`Player::stored`]).
    SellShip(usize),
    /// Fly a stored ship instead; the current one is stored in its place.
    Board(usize),
    /// Fit a module, selling whatever was in the slot.
    Fit {
        mount: usize,
        slot: usize,
        module: &'static Module,
    },
    /// Sell the module in a slot.
    Strip { mount: usize, slot: usize },
    /// Put a section in an open mount, selling the old one and its modules.
    SetSection {
        mount: usize,
        section: &'static Section,
    },
}

/// What a settlement's hangar offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Services {
    pub shipyard: bool,
    pub outfitter: bool,
}

pub fn services(economy: &LocalEconomy) -> Services {
    let has = |kind| economy.facilities.iter().any(|f| f.kind == kind);
    let shipyard = has(FacilityKind::Shipyard);
    Services {
        shipyard,
        outfitter: shipyard
            || has(FacilityKind::Factory)
            || economy.population >= OUTFITTER_POPULATION,
    }
}

/// A ship on the lot.
pub struct Offer {
    pub hull: &'static Hull,
    pub price: f64,
    /// Empty mass, which the yard must have built as hull.
    pub mass_t: f64,
}

/// Every stock ship, cheapest first. Whether a yard can sell one depends on
/// its hull stock ([`can_build`]).
pub fn lot() -> Vec<Offer> {
    let mut offers: Vec<Offer> = retrox_content::ships::HULLS
        .iter()
        .map(|hull| {
            let fit = stock(hull.id).expect("every hull has a stock fit");
            Offer {
                hull,
                price: price::loadout_price(&fit),
                mass_t: fit.stats().mass_t,
            }
        })
        .collect();
    offers.sort_by(|a, b| a.price.total_cmp(&b.price));
    offers
}

/// Tonnes of hull this settlement's shipyards can build a day, at full
/// capacity. (Haulers buy from the same stock.)
pub fn yard_output_t_per_day(economy: &LocalEconomy) -> f64 {
    economy
        .facilities
        .iter()
        .filter(|f| f.kind == FacilityKind::Shipyard)
        .map(|f| f.scale * HULL_T_PER_RUN)
        // Not `sum`: an empty float sum is -0.0, shown as "-0".
        .fold(0.0, |a, b| a + b)
}

pub fn can_build(economy: &LocalEconomy, offer: &Offer) -> bool {
    economy.hull_t >= offer.mass_t
}

/// Time to fit or remove a module of this size.
pub fn module_seconds(size: Size) -> f64 {
    match size {
        Size::Small => HOUR,
        Size::Medium => 3.0 * HOUR,
        Size::Large => 8.0 * HOUR,
    }
}

/// Time to swap a section of this size.
pub fn section_seconds(size: Size) -> f64 {
    match size {
        Size::Small => 6.0 * HOUR,
        Size::Medium => 12.0 * HOUR,
        Size::Large => 24.0 * HOUR,
    }
}

/// What a job will cost (negative: pays out) before it's done, for showing
/// on buttons. `None` if it can't be done at all.
pub fn quote(player: &Player, job: Job) -> Option<f64> {
    let fit = &player.ship.fit;
    match job {
        Job::BuyShip(hull) => Some(price::loadout_price(&stock(hull.id)?)),
        Job::SellShip(i) => Some(-price::resale(price::loadout_price(
            &player.stored.get(i)?.ship.fit,
        ))),
        Job::Board(i) => player.stored.get(i).map(|_| 0.0),
        Job::Fit {
            mount,
            slot,
            module,
        } => {
            fit.check(mount, slot, module).ok()?;
            let old =
                module_in(fit, mount, slot).map_or(0.0, |m| price::resale(price::module_price(m)));
            Some(price::module_price(module) - old)
        }
        Job::Strip { mount, slot } => {
            module_in(fit, mount, slot).map(|m| -price::resale(price::module_price(m)))
        }
        Job::SetSection { mount, section } => {
            fit.check_section(mount, section).ok()?;
            Some(price::section_price(section) - section_refund(fit, mount))
        }
    }
}

/// The ship as it would be after a job, for comparing stats. `None` if the
/// job doesn't change the ship being flown or can't be done.
pub fn preview(player: &Player, job: Job) -> Option<Loadout> {
    let mut fit = player.ship.fit.clone();
    match job {
        Job::BuyShip(hull) => return stock(hull.id),
        Job::Board(i) => return Some(player.stored.get(i)?.ship.fit.clone()),
        Job::SellShip(_) => return None,
        Job::Fit {
            mount,
            slot,
            module,
        } => {
            fit.fit(mount, slot, module).ok()?;
        }
        Job::Strip { mount, slot } => {
            fit.remove(mount, slot)?;
        }
        Job::SetSection { mount, section } => {
            fit.set_section(mount, section).ok()?;
        }
    }
    Some(fit)
}

pub fn module_in(fit: &Loadout, mount: usize, slot: usize) -> Option<&'static Module> {
    let (_, section) = fit.mounts().nth(mount)?;
    section?.slots().nth(slot)?.1
}

/// What the section in a mount, and everything in it, sells back for.
fn section_refund(fit: &Loadout, mount: usize) -> f64 {
    let Some((Mount::Open { .. }, Some(section))) = fit.mounts().nth(mount) else {
        return 0.0;
    };
    let modules: f64 = section
        .slots()
        .filter_map(|(_, m)| m)
        .map(price::module_price)
        .sum();
    price::resale(price::section_price(section.section) + modules)
}

/// A finished job: what to tell the player and how long it took.
#[derive(Debug, PartialEq)]
pub struct Done {
    pub message: String,
    pub seconds: f64,
}

/// Do a job at settlement `here`, paying or being paid by its treasury.
pub fn run(
    player: &mut Player,
    here: SettlementId,
    economy: &mut LocalEconomy,
    job: Job,
) -> Result<Done, String> {
    let offers = services(economy);
    let need_yard = matches!(job, Job::BuyShip(_) | Job::SellShip(_));
    let need_outfitter = matches!(
        job,
        Job::Fit { .. } | Job::Strip { .. } | Job::SetSection { .. }
    );
    if need_yard && !offers.shipyard {
        return Err("No shipyard here".into());
    }
    if need_outfitter && !offers.outfitter {
        return Err("No outfitter here".into());
    }
    let cost = quote(player, job).ok_or("That doesn't fit")?;
    if cost > player.credits {
        return Err(format!(
            "Not enough credits: that costs {}",
            crate::hud::credits(cost)
        ));
    }
    if -cost > economy.treasury {
        return Err("The settlement can't afford to buy that".into());
    }
    // The cargo stays aboard the ship being flown, so its hold must still
    // take it.
    if let Some(next) = preview(player, job) {
        let room = next.stats().cargo_t;
        if room + 1e-9 < player.cargo_t() {
            return Err(format!(
                "Sell some cargo first: the hold would only take {room:.0} t"
            ));
        }
    }

    let done = match job {
        Job::BuyShip(hull) => {
            let fit = stock(hull.id).ok_or("No such ship")?;
            let mass = fit.stats().mass_t;
            if economy.hull_t < mass {
                return Err(format!(
                    "The yard has {:.0} t of hull built; a {} needs {mass:.0} t",
                    economy.hull_t, hull.name
                ));
            }
            economy.hull_t -= mass;
            player.bought += 1;
            let ship = OwnedShip {
                name: format!("{} No. {}", hull.name, player.bought),
                fit,
                damage: Default::default(),
            };
            let old = std::mem::replace(&mut player.ship, ship);
            player.stored.push(StoredShip {
                ship: old,
                at: here,
            });
            Done {
                message: format!("Bought the {}", player.ship.name),
                seconds: BUY_SHIP_SECONDS,
            }
        }
        Job::SellShip(i) => {
            let stored = player.stored.get(i).ok_or("No such ship")?;
            if stored.at != here {
                return Err("That ship is stored somewhere else".into());
            }
            let stored = player.stored.remove(i);
            // The yard takes the hull back to sell on.
            economy.hull_t += stored.ship.fit.stats().mass_t;
            Done {
                message: format!("Sold the {}", stored.ship.name),
                seconds: HOUR,
            }
        }
        Job::Board(i) => {
            if player.stored.get(i).ok_or("No such ship")?.at != here {
                return Err("That ship is stored somewhere else".into());
            }
            std::mem::swap(&mut player.ship, &mut player.stored[i].ship);
            Done {
                message: format!("Now flying the {}", player.ship.name),
                seconds: HOUR,
            }
        }
        Job::Fit {
            mount,
            slot,
            module,
        } => {
            let old = player
                .ship
                .fit
                .fit(mount, slot, module)
                .map_err(|e| e.to_string())?;
            let message = match old {
                Some(old) => format!("Swapped the {} for a {}", old.name, module.name),
                None => format!("Fitted a {}", module.name),
            };
            Done {
                message,
                seconds: module_seconds(module.size),
            }
        }
        Job::Strip { mount, slot } => {
            let old = player
                .ship
                .fit
                .remove(mount, slot)
                .ok_or("Nothing to remove")?;
            Done {
                message: format!("Sold the {}", old.name),
                seconds: module_seconds(old.size),
            }
        }
        Job::SetSection { mount, section } => {
            let sold = player
                .ship
                .fit
                .set_section(mount, section)
                .map_err(|e| e.to_string())?;
            let message = if sold.is_empty() {
                format!("Fitted a {}", section.name)
            } else {
                format!(
                    "Fitted a {} and sold {}",
                    section.name,
                    crate::hud::count(sold.len(), "module")
                )
            };
            Done {
                message,
                seconds: section_seconds(section.size),
            }
        }
    };
    player.credits -= cost;
    economy.treasury += cost;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use retrox_content::ships::{CARGO_BAY_M, GUN_DECK_M, MODULES, hull};
    use retrox_sim::economy::Facility;
    use slotmap::SlotMap;

    fn yard() -> LocalEconomy {
        LocalEconomy {
            population: 100.0,
            facilities: vec![Facility {
                kind: FacilityKind::Shipyard,
                scale: 1.0,
                utilization: 1.0,
            }],
            hull_t: 1000.0,
            treasury: 1.0e6,
            ..Default::default()
        }
    }

    /// A pilot with money to spend: the starting purse doesn't buy ships.
    fn rich() -> Player {
        Player {
            credits: 100_000.0,
            ..Player::new()
        }
    }

    fn here() -> SettlementId {
        SlotMap::<SettlementId, ()>::with_key().insert(())
    }

    #[test]
    fn buying_a_ship_stores_the_old_one_and_pays_the_yard() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        let hauler = hull("hauler").unwrap();
        let price = quote(&p, Job::BuyShip(hauler)).unwrap();
        let (credits, treasury, hull_t) = (p.credits, eco.treasury, eco.hull_t);
        run(&mut p, at, &mut eco, Job::BuyShip(hauler)).unwrap();
        assert_eq!(p.ship.fit.hull().id, "hauler");
        assert_eq!(p.stored.len(), 1);
        assert_eq!(p.stored[0].ship.fit.hull().id, STARTING_HULL);
        assert_eq!(p.credits, credits - price);
        assert_eq!(eco.treasury, treasury + price);
        assert!(eco.hull_t < hull_t);
    }

    #[test]
    fn cannot_buy_without_hull_stock_money_or_a_yard() {
        let (mut p, at) = (Player::new(), here());
        let hauler = hull("hauler").unwrap();
        let mut empty = LocalEconomy {
            hull_t: 0.0,
            ..yard()
        };
        assert!(run(&mut p, at, &mut empty, Job::BuyShip(hauler)).is_err());
        let mut no_yard = LocalEconomy {
            facilities: vec![],
            ..yard()
        };
        assert!(run(&mut p, at, &mut no_yard, Job::BuyShip(hauler)).is_err());
        let frigate = hull("frigate").unwrap();
        let mut rich_yard = LocalEconomy {
            hull_t: 1.0e5,
            ..yard()
        };
        let err = run(&mut p, at, &mut rich_yard, Job::BuyShip(frigate)).unwrap_err();
        assert!(err.contains("credits"), "{err}");
        assert_eq!(p.ship.fit.hull().id, STARTING_HULL);
        assert_eq!(p.credits, STARTING_CREDITS);
    }

    #[test]
    fn boarding_and_selling_stored_ships() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        run(&mut p, at, &mut eco, Job::BuyShip(hull("racer").unwrap())).unwrap();
        run(&mut p, at, &mut eco, Job::Board(0)).unwrap();
        assert_eq!(p.ship.fit.hull().id, STARTING_HULL);
        assert_eq!(p.stored[0].ship.fit.hull().id, "racer");
        let credits = p.credits;
        let refund = -quote(&p, Job::SellShip(0)).unwrap();
        run(&mut p, at, &mut eco, Job::SellShip(0)).unwrap();
        assert!(p.stored.is_empty());
        assert_eq!(p.credits, credits + refund);
        // Ships stored elsewhere can't be boarded here.
        run(&mut p, at, &mut eco, Job::BuyShip(hull("racer").unwrap())).unwrap();
        let elsewhere = {
            let mut keys = SlotMap::<SettlementId, ()>::with_key();
            keys.insert(());
            keys.insert(())
        };
        assert!(run(&mut p, elsewhere, &mut eco, Job::Board(0)).is_err());
    }

    #[test]
    fn swapping_a_module_sells_the_old_one() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        // A filled slot, and a different module that also fits it.
        let (mount, slot, replacement) = (0..p.ship.fit.hull().mounts.len())
            .flat_map(|m| (0..16).map(move |s| (m, s)))
            .find_map(|(m, s)| {
                let old = module_in(&p.ship.fit, m, s)?;
                let new = MODULES
                    .iter()
                    .find(|n| n.id != old.id && p.ship.fit.check(m, s, n).is_ok())?;
                Some((m, s, new))
            })
            .expect("some slot has an alternative module");
        let old = module_in(&p.ship.fit, mount, slot).unwrap();
        let credits = p.credits;
        let job = Job::Fit {
            mount,
            slot,
            module: replacement,
        };
        let done = run(&mut p, at, &mut eco, job).unwrap();
        assert_eq!(module_in(&p.ship.fit, mount, slot), Some(replacement));
        let expected = price::module_price(replacement) - price::resale(price::module_price(old));
        assert_eq!(p.credits, credits - expected);
        assert!(done.seconds > 0.0);

        run(&mut p, at, &mut eco, Job::Strip { mount, slot }).unwrap();
        assert_eq!(module_in(&p.ship.fit, mount, slot), None);
    }

    #[test]
    fn swapping_a_section_sells_its_modules() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        run(&mut p, at, &mut eco, Job::BuyShip(hull("hauler").unwrap())).unwrap();
        let mount = (0..p.ship.fit.hull().mounts.len())
            .find(|&m| p.ship.fit.check_section(m, &GUN_DECK_M).is_ok())
            .expect("the hauler has a medium mid mount");
        let refund = section_refund(&p.ship.fit, mount);
        assert!(refund > 0.0);
        let credits = p.credits;
        run(
            &mut p,
            at,
            &mut eco,
            Job::SetSection {
                mount,
                section: &GUN_DECK_M,
            },
        )
        .unwrap();
        assert_eq!(
            p.credits,
            credits - (price::section_price(&GUN_DECK_M) - refund)
        );
        let (_, section) = p.ship.fit.mounts().nth(mount).unwrap();
        assert!(section.unwrap().slots().all(|(_, m)| m.is_none()));
        // Wrong position or size is refused.
        let bow = (0..p.ship.fit.hull().mounts.len())
            .find(|&m| p.ship.fit.check_section(m, &CARGO_BAY_M).is_err())
            .unwrap();
        assert!(
            quote(
                &p,
                Job::SetSection {
                    mount: bow,
                    section: &CARGO_BAY_M
                }
            )
            .is_none()
        );
    }

    #[test]
    fn cargo_must_fit_the_new_hold() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        p.cargo[retrox_sim::economy::Good::Ore] = 10.0;
        // A racer's hold is smaller than the courier's 12 t.
        let racer = hull("racer").unwrap();
        assert!(stock("racer").unwrap().stats().cargo_t < 10.0);
        assert!(run(&mut p, at, &mut eco, Job::BuyShip(racer)).is_err());
        p.cargo[retrox_sim::economy::Good::Ore] = 0.0;
        run(&mut p, at, &mut eco, Job::BuyShip(racer)).unwrap();
    }

    #[test]
    fn the_settlement_must_afford_what_it_buys_back() {
        let (mut p, at, mut eco) = (rich(), here(), yard());
        run(&mut p, at, &mut eco, Job::BuyShip(hull("racer").unwrap())).unwrap();
        eco.treasury = 0.0;
        assert!(run(&mut p, at, &mut eco, Job::SellShip(0)).is_err());
        assert_eq!(p.stored.len(), 1);
    }
}
