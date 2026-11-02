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

use std::collections::HashMap;

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

/// Who holds a claimed site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holder {
    Settlement,
    Ship,
}

/// The best free site in reach, as a launch chooses it.
#[derive(Clone, Copy)]
struct Candidate {
    system: SystemId,
    site: Site,
    /// How much colonists want it, discounted by the trip.
    score: f64,
    days: f64,
}

/// Where colonists can go, prepared for the monthly reviews (DESIGN
/// §5.1.1): which sites are taken, and each system's settleable sites,
/// best first, with how many at the front are taken.
///
/// Kept up to date as sites are taken: a system's list moves past its
/// first free site once that is taken, and goes back to its start if a
/// site comes free again (colonists who scatter). A launch compares the
/// first free site of each system in reach, nearest first, which is
/// what a search of every body in reach would find.
///
/// Everything here follows from the state, so it isn't saved: a loaded
/// game builds it again on its first launch and chooses the same.
#[derive(Default)]
pub struct Sites {
    built: bool,
    taken: HashMap<Claim, Holder>,
    /// By system index, made the first time a launch looks there.
    systems: Vec<Option<SystemSites>>,
}

/// One system's settleable sites, best first. Sites and their scores
/// don't change; only which are taken does.
#[derive(Clone)]
struct SystemSites {
    sites: Vec<(Site, f64)>,
    /// Every site before this one is taken.
    first_free: usize,
}

impl Clone for Sites {
    /// Clones build their own when they need it.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for Sites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sites")
    }
}

impl Sites {
    fn build(&mut self, state: &GalaxyState) {
        if self.built {
            return;
        }
        self.taken.clear();
        self.systems.clear();
        let ships = state.colony_ships.values().map(|c| (c.site, Holder::Ship));
        let settled = state
            .settlements
            .values()
            .map(|s| (s.site, Holder::Settlement));
        // Settlements last: a settlement holds its site even if a ship is
        // still on its way there.
        for (site, holder) in ships.chain(settled) {
            if let Some(c) = claim(site) {
                self.taken.insert(c, holder);
            }
        }
        self.built = true;
    }

    fn settled_at(&self, site: Site) -> bool {
        claim(site).is_some_and(|c| self.taken.get(&c) == Some(&Holder::Settlement))
    }

    /// A settlement now stands on `site`.
    pub(crate) fn settle(&mut self, site: Site) {
        if self.built
            && let Some(c) = claim(site)
        {
            self.taken.insert(c, Holder::Settlement);
        }
    }

    fn launch_to(&mut self, site: Site) {
        if let Some(c) = claim(site) {
            self.taken.entry(c).or_insert(Holder::Ship);
        }
    }

    /// Colonists bound for `site`, in `system`, gave up: it's free again,
    /// unless someone settled it meanwhile.
    fn release(&mut self, system: SystemId, site: Site) {
        if self.built
            && let Some(c) = claim(site)
            && self.taken.get(&c) == Some(&Holder::Ship)
        {
            self.taken.remove(&c);
            if let Some(Some(here)) = self.systems.get_mut(system.index()) {
                here.first_free = 0;
            }
        }
    }

    /// `system`'s best free site and its score.
    fn first_free(&mut self, state: &GalaxyState, system: SystemId) -> Option<(Site, f64)> {
        let i = system.index();
        if self.systems.len() <= i {
            self.systems.resize(i + 1, None);
        }
        let here = self.systems[i].get_or_insert_with(|| SystemSites {
            sites: ranked(state, system),
            first_free: 0,
        });
        let taken = &self.taken;
        while let Some(&(site, _)) = here.sites.get(here.first_free)
            && claim(site).is_some_and(|c| taken.contains_key(&c))
        {
            here.first_free += 1;
        }
        // Past the first free one, some may be taken too: only the first
        // is ever read, and it is checked above.
        here.sites.get(here.first_free).copied()
    }

    /// The best free site in reach of `origin`. Equal scores go to the
    /// one found later, as they always have.
    fn best(&mut self, state: &GalaxyState, origin: SystemId) -> Option<Candidate> {
        self.build(state);
        let mut best: Option<Candidate> = None;
        for &(system, ly, _) in reach(state, origin).iter() {
            let Some((site, score)) = self.first_free(state, system) else {
                continue;
            };
            let days = 2.0 * IN_SYSTEM_DAYS + ly / JUMP_LY_PER_DAY;
            let score = score / (1.0 + days / 20.0);
            if best.is_none_or(|b| score >= b.score) {
                best = Some(Candidate {
                    system,
                    site,
                    score,
                    days,
                });
            }
        }
        best
    }
}

/// Systems a colony ship can get to from `origin`.
fn reach(state: &GalaxyState, origin: SystemId) -> crate::haul::Reached {
    crate::haul::reach(
        state,
        origin,
        (MAX_DAYS - 2.0 * IN_SYSTEM_DAYS) * JUMP_LY_PER_DAY,
    )
}

/// Every settleable site in `system`, best first; equal scores put the
/// later-found first.
fn ranked(state: &GalaxyState, system: SystemId) -> Vec<(Site, f64)> {
    let mut sites = Vec::new();
    candidates(state, &HashMap::new(), system, |site, score| {
        sites.push((site, score));
    });
    let mut indexed: Vec<(usize, (Site, f64))> = sites.into_iter().enumerate().collect();
    indexed.sort_by(|(i, a), (j, b)| b.1.total_cmp(&a.1).then(j.cmp(i)));
    indexed.into_iter().map(|(_, s)| s).collect()
}

/// The best free site in reach of `origin`'s colonists: the system and
/// the site. Taken from the prepared lists ([`Sites`]).
pub fn best_free_site(state: &mut GalaxyState, origin: SystemId) -> Option<(SystemId, Site)> {
    let mut sites = std::mem::take(&mut state.sites);
    let best = sites.best(state, origin);
    state.sites = sites;
    best.map(|c| (c.system, c.site))
}

/// The same as [`best_free_site`], found by searching every body in
/// reach: what the lists stand in for, kept to check them against.
pub fn search_free_site(state: &GalaxyState, origin: SystemId) -> Option<(SystemId, Site)> {
    let mut sites = Sites::default();
    sites.build(state);
    let mut best: Option<(SystemId, Site, f64)> = None;
    for &(system, ly, _) in reach(state, origin).iter() {
        let days = 2.0 * IN_SYSTEM_DAYS + ly / JUMP_LY_PER_DAY;
        candidates(state, &sites.taken, system, |site, score| {
            let score = score / (1.0 + days / 20.0);
            if best.is_none_or(|b| score >= b.2) {
                best = Some((system, site, score));
            }
        });
    }
    best.map(|(system, site, _)| (system, site))
}

fn site_name(state: &GalaxyState, site: Site) -> String {
    match site {
        Site::Region(r) => state.regions[r].name.clone(),
        Site::Surface(b) => state.bodies[b].name.clone(),
        Site::Space(_) => String::new(),
    }
}

/// Each free site in `system` worth settling, with a score.
fn candidates(
    state: &GalaxyState,
    taken: &HashMap<Claim, Holder>,
    system: SystemId,
    mut found: impl FnMut(Site, f64),
) {
    let is_taken = |site| claim(site).is_some_and(|c| taken.contains_key(&c));
    for &b in &state.systems[system].bodies {
        let body = &state.bodies[b];
        if body.regions.is_empty() {
            let rich = body.resources.ore.max(body.resources.ice);
            let site = Site::Surface(b);
            let settleable = !matches!(body.kind, BodyKind::GasGiant)
                && (body.habitability >= SETTLEABLE || rich >= RICH);
            if settleable && !is_taken(site) {
                found(site, body.habitability + 0.3 * rich);
            }
        } else {
            for &r in &body.regions {
                let site = Site::Region(r);
                if !is_taken(site) {
                    found(site, 2.0 * state.regions[r].habitability);
                }
            }
        }
    }
}

/// Monthly reviews, colonization part: crowded settlements may launch
/// colony ships. Looking for a site is a search, so it only happens here,
/// for the few settlements reviewing today that can afford a ship.
pub fn launch(state: &mut GalaxyState) {
    let day = state.time.days();
    // Half a month off the construction review, to spread the work.
    for i in crate::settlement::review_slots(day, 15, state.settlements.slot_count()) {
        let Some(id) = state.settlements.key_at(i) else {
            continue;
        };
        crate::decisions::time(crate::decisions::Decision::Colonization, || {
            launch_one(state, id, day);
        });
    }
}

/// One settlement's monthly review of whether to send colonists.
fn launch_one(state: &mut GalaxyState, id: SettlementId, day: u64) {
    let key = slotmap::Key::data(&id).as_ffi();
    let settlement = &state.settlements[id];
    let economy = &settlement.economy;
    let capacity = crate::demography::capacity(state, id);
    let crowding =
        ((economy.population / capacity.max(1e-6) - UNEASY) / (CROWDED - UNEASY)).clamp(0.0, 1.0);
    if economy.population < MIN_POPULATION || crowding <= 0.0 {
        return;
    }
    if !Rng::stream(state.seed, "colony-launch", day, key).chance(LAUNCH_CHANCE * crowding) {
        return;
    }
    let market = &economy.market;
    let food = crate::economy::per_capita(Good::Food) * COLONISTS * PROVISION_DAYS;
    let fuel = crate::economy::per_capita(Good::Fuel) * COLONISTS * PROVISION_DAYS;
    let enough = market.stock[Good::Metal] >= SHIP_METAL
        && market.stock[Good::Components] >= SHIP_COMPONENTS
        && market.stock[Good::Food] >= food
        && market.stock[Good::Fuel] >= fuel;
    if !enough {
        return;
    }

    let origin = settlement.system;
    let owner = settlement.owner;
    let mut sites = std::mem::take(&mut state.sites);
    let best = sites.best(state, origin);
    if let Some(c) = best {
        sites.launch_to(c.site);
    }
    state.sites = sites;
    let Some(Candidate {
        system, site, days, ..
    }) = best
    else {
        return;
    };
    let name = site_name(state, site);

    let demand_value: f64 = Good::ALL
        .iter()
        .map(|&g| crate::economy::per_capita(g) * COLONISTS * g.base_price())
        .sum();
    let endowment = demand_value * ENDOWMENT_DAYS;
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
    let mut sites = std::mem::take(&mut state.sites);
    sites.build(state);
    state.sites = sites;
    for key in arrived {
        let ship = state.colony_ships[key].clone();
        // Someone may have got there first; the colonists make do with a
        // station in orbit.
        let site = if state.sites.settled_at(ship.site) {
            station_near(state, ship.site)
        } else {
            ship.site
        };
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
            state.sites.release(ship.system, ship.site);
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
            to_capital: None,
        });
        crate::politics::founded(state, settlement);
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

/// A station orbiting whatever `site` is on, at 3 radii: inside the
/// planet's well (4 radii) with room for its arrival point (#112).
fn station_near(state: &GalaxyState, site: Site) -> Site {
    let body: Option<BodyId> = match site {
        Site::Region(r) => Some(region_body(state, r)),
        Site::Surface(b) => Some(b),
        Site::Space(orbit) => return Site::Space(orbit),
    };
    let body = body.expect("set above");
    let radius_au = state.bodies[body].radius_km * 3.0 / 1.496e8;
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
