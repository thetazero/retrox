//! Colony ships (DESIGN §5.7, issue #27): crowded settlements send people
//! out along the lanes to found new settlements.
//!
//! Once a month a settlement past half its carrying capacity, with the goods
//! to build and stock a ship, may launch one. The ship carries a few
//! thousand people, food and money to a free site within reach: an
//! unsettled region of a habitable world, a habitable undivided body, or
//! a rich one for a mining outpost. Its arrival is an event on a day set at
//! launch, like a trader's (DESIGN §5.1.1); on arrival the colonists found
//! a settlement of their faction.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::bodies::{BodyId, BodyKind, RegionId};
use crate::economy::{Facility, FacilityKind, Good, GoodMap, LocalEconomy};
use crate::ledger::{self, Account};
use crate::settlement::{Settlement, SettlementId, Site};
use crate::{FactionId, GalaxyState, GalaxyTime, HistoryKind, Rng, SystemId};

new_key_type! {
    pub struct ColonyShipId;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColonyShip {
    pub owner: FactionId,
    pub from: SettlementId,
    pub system: SystemId,
    pub site: Site,
    pub name: String,
    pub population: f64,
    pub cargo: GoodMap<f64>,
    pub credits: f64,
    pub arrive: GalaxyTime,
}

/// Emigration starts at this share of capacity and peaks at `CROWDED`.
const UNEASY: f64 = 0.5;
const CROWDED: f64 = 0.85;
/// Monthly chance a fully crowded settlement launches a ship; it ramps up
/// from zero at `UNEASY`.
const LAUNCH_CHANCE: f64 = 0.3;
/// A settlement needs at least this many people to send some away.
const MIN_POPULATION: f64 = 10.0;
/// People aboard one ship.
const COLONISTS: f64 = 3.0;
/// What building one ship takes from the market.
const SHIP_METAL: f64 = 60.0;
const SHIP_COMPONENTS: f64 = 10.0;
/// Days of food and fuel the colonists take with them.
const PROVISION_DAYS: f64 = 120.0;
/// Days of their demand, at base prices, the colonists take in credits.
const ENDOWMENT_DAYS: f64 = 60.0;
/// Colony ships go no further than this, and travel at this speed.
const MAX_DAYS: f64 = 60.0;
const JUMP_LY_PER_DAY: f64 = 1.3;
/// Rough in-system time at each end, for choosing a destination.
const IN_SYSTEM_DAYS: f64 = 4.0;
/// Undivided bodies at least this habitable can take a colony.
const SETTLEABLE: f64 = 0.03;
/// Bodies at least this rich in a resource can take a mining outpost.
const RICH: f64 = 0.5;
/// Starting facility scale per colonist.
const START_SCALE: f64 = 1.0;

/// What makes a site unique for claiming: a region or a whole body.
/// Stations never clash.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Claim {
    Region(RegionId),
    Body(BodyId),
}

fn claim(site: Site) -> Option<Claim> {
    match site {
        Site::Region(r) => Some(Claim::Region(r)),
        Site::Surface(b) => Some(Claim::Body(b)),
        Site::Space(_) => None,
    }
}

fn is_taken(taken: &HashSet<Claim>, site: Site) -> bool {
    claim(site).is_some_and(|c| taken.contains(&c))
}

/// A free site worth settling, with a score.
fn candidates(
    state: &GalaxyState,
    taken: &HashSet<Claim>,
    system: SystemId,
) -> Vec<(Site, String, f64)> {
    let mut out = Vec::new();
    for &b in &state.systems[system].bodies {
        let body = &state.bodies[b];
        if body.regions.is_empty() {
            let rich = body.resources.ore.max(body.resources.ice);
            let site = Site::Surface(b);
            let settleable = !matches!(body.kind, BodyKind::GasGiant)
                && (body.habitability >= SETTLEABLE || rich >= RICH);
            if settleable && !is_taken(taken, site) {
                out.push((site, body.name.clone(), body.habitability + 0.3 * rich));
            }
        } else {
            for &r in &body.regions {
                let site = Site::Region(r);
                if !is_taken(taken, site) {
                    let region = &state.regions[r];
                    out.push((site, region.name.clone(), 2.0 * region.habitability));
                }
            }
        }
    }
    out
}

/// Sites already settled or claimed by a ship on its way.
fn taken_sites(state: &GalaxyState) -> HashSet<Claim> {
    state
        .settlements
        .values()
        .map(|s| s.site)
        .chain(state.colony_ships.values().map(|c| c.site))
        .filter_map(claim)
        .collect()
}

/// Planning phase: crowded settlements may launch colony ships.
pub fn launch(state: &mut GalaxyState) {
    let day = state.time.days();
    let ids: Vec<SettlementId> = state.settlements.keys().collect();
    let mut taken: Option<HashSet<Claim>> = None;
    for id in ids {
        let key = slotmap::Key::data(&id).as_ffi();
        if (key.wrapping_add(15)) % 30 != day % 30 {
            continue;
        }
        let settlement = &state.settlements[id];
        let economy = &settlement.economy;
        let capacity = crate::demography::capacity(state, id);
        let crowding = ((economy.population / capacity.max(1e-6) - UNEASY) / (CROWDED - UNEASY))
            .clamp(0.0, 1.0);
        if economy.population < MIN_POPULATION || crowding <= 0.0 {
            continue;
        }
        if !Rng::stream(state.seed, "colony-launch", day, key).chance(LAUNCH_CHANCE * crowding) {
            continue;
        }
        let market = &economy.market;
        let food = crate::economy::per_capita(Good::Food) * COLONISTS * PROVISION_DAYS;
        let fuel = crate::economy::per_capita(Good::Fuel) * COLONISTS * PROVISION_DAYS;
        let enough = market.stock[Good::Metal] >= SHIP_METAL
            && market.stock[Good::Components] >= SHIP_COMPONENTS
            && market.stock[Good::Food] >= food
            && market.stock[Good::Fuel] >= fuel;
        if !enough {
            continue;
        }

        let taken = taken.get_or_insert_with(|| taken_sites(state));
        let origin = settlement.system;
        let best = crate::haul::reach(
            state,
            origin,
            (MAX_DAYS - 2.0 * IN_SYSTEM_DAYS) * JUMP_LY_PER_DAY,
        )
        .iter()
        .flat_map(|&(system, ly, _)| {
            let days = 2.0 * IN_SYSTEM_DAYS + ly / JUMP_LY_PER_DAY;
            candidates(state, taken, system)
                .into_iter()
                .map(move |(site, name, score)| {
                    (system, site, name, score / (1.0 + days / 20.0), days)
                })
        })
        .max_by(|a, b| a.3.total_cmp(&b.3));
        let Some((system, site, name, _, days)) = best else {
            continue;
        };
        taken.extend(claim(site));

        let demand_value: f64 = Good::ALL
            .iter()
            .map(|&g| crate::economy::per_capita(g) * COLONISTS * g.base_price())
            .sum();
        let endowment = demand_value * ENDOWMENT_DAYS;
        let owner = settlement.owner;
        // The colonists' own settlement builds and stocks the ship.
        ledger::consume(state, id, Good::Metal, SHIP_METAL);
        ledger::consume(state, id, Good::Components, SHIP_COMPONENTS);
        let mut cargo = GoodMap::splat(0.0);
        cargo[Good::Food] = ledger::consume(state, id, Good::Food, food);
        cargo[Good::Fuel] = ledger::consume(state, id, Good::Fuel, fuel);
        let economy = &mut state.settlements[id].economy;
        economy.population -= COLONISTS;
        economy.refresh_demand();
        let ship = state.colony_ships.insert(ColonyShip {
            owner,
            from: id,
            system,
            site,
            name,
            population: COLONISTS,
            cargo,
            credits: 0.0,
            arrive: GalaxyTime::from_days(day + days.ceil().max(1.0) as u64),
        });
        // Up to half of what the settlement can spend goes with them.
        let stake = endowment.min(ledger::spendable(state, Account::Settlement(id)) * 0.5);
        ledger::pay(state, Account::Settlement(id), Account::Colony(ship), stake);
    }
}

/// Movement phase: colony ships that arrive today found their
/// settlement.
pub fn arrive(state: &mut GalaxyState) {
    let now = state.time;
    let arrived: Vec<ColonyShipId> = state
        .colony_ships
        .iter()
        .filter(|(_, c)| c.arrive <= now)
        .map(|(k, _)| k)
        .collect();
    if arrived.is_empty() {
        return;
    }
    let mut settled: HashSet<Claim> = state
        .settlements
        .values()
        .filter_map(|s| claim(s.site))
        .collect();
    for key in arrived {
        let ship = state.colony_ships[key].clone();
        // Someone may have got there first; the colonists make do with a
        // station in orbit.
        let site = if is_taken(&settled, ship.site) {
            station_near(state, ship.site)
        } else {
            ship.site
        };
        settled.extend(claim(site));
        let owner = if state.factions.contains_key(ship.owner) {
            Some(ship.owner)
        } else {
            state.settlements.get(ship.from).map(|s| s.owner)
        };
        let Some(owner) = owner else {
            // Nobody left to belong to: the colonists scatter, and their
            // money goes to any faction.
            let any = state.factions.keys().next();
            if let Some(f) = any {
                ledger::pay(
                    state,
                    Account::Colony(key),
                    Account::Faction(f),
                    ship.credits,
                );
            }
            state.colony_ships.remove(key);
            continue;
        };
        let mut economy = LocalEconomy {
            population: ship.population,
            ..LocalEconomy::default()
        };
        economy.market.stock = ship.cargo;
        economy.facilities = starting_facilities(state, site, ship.population);
        economy.refresh_demand();
        economy.update_prices(state.factions[owner].economy);
        let settlement = state.add_settlement(Settlement {
            system: ship.system,
            site,
            owner,
            name: ship.name,
            founded: now,
            economy,
        });
        ledger::pay(
            state,
            Account::Colony(key),
            Account::Settlement(settlement),
            ship.credits,
        );
        state.colony_ships.remove(key);
        state.history.record(
            now,
            HistoryKind::SettlementFounded {
                settlement,
                faction: owner,
            },
        );
    }
}

/// A station orbiting whatever `site` is on.
fn station_near(state: &GalaxyState, site: Site) -> Site {
    let body: Option<BodyId> = match site {
        Site::Region(r) => Some(region_body(state, r)),
        Site::Surface(b) => Some(b),
        Site::Space(orbit) => return Site::Space(orbit),
    };
    let body = body.expect("set above");
    let radius_au = state.bodies[body].radius_km * 4.0 / 1.496e8;
    Site::Space(crate::Orbit::circular(
        crate::Orbiting::Body(body),
        radius_au,
        0.0,
    ))
}

fn region_body(state: &GalaxyState, r: RegionId) -> BodyId {
    state.regions[r].body
}

/// Hydroponics to feed themselves, and a mine or ice works if the site is
/// rich.
fn starting_facilities(state: &GalaxyState, site: Site, population: f64) -> Vec<Facility> {
    let resources = match site {
        Site::Region(r) => state.regions[r].resources,
        Site::Surface(b) => state.bodies[b].resources,
        Site::Space(_) => Default::default(),
    };
    let scale = START_SCALE * population;
    let mut out = vec![Facility::new(FacilityKind::Hydroponics, scale)];
    if resources.ore >= RICH {
        out.push(Facility::new(FacilityKind::Mine, scale));
    }
    if resources.ice >= RICH {
        out.push(Facility::new(FacilityKind::IceHarvester, scale));
    }
    out
}
