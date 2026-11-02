//! Stability and secession (DESIGN §5.5, issue #18): how new factions are
//! born.
//!
//! Every settlement has a stability between 0 and 1. At its monthly review
//! it moves part of the way toward a target that falls with lane distance
//! from its faction's capital, with the size of its faction
//! (overextension) and with hunger, and drops sharply in a famine. Once a year each faction's unstable systems, grouped with the
//! unstable systems near them, may break away together as a new faction.
//! The breakaway takes as capital its most populous settlement, keeps only
//! the systems nearer that new capital than the old one, remembers its
//! parent, inherits its economic model (usually) and a colour near its
//! parent's, and is named along the lineage. This loop is what keeps
//! history from settling into one empire; see DESIGN §5.5.1 for the
//! tuning.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::settlement::{REVIEW_DAYS, review_slots};
use crate::{
    EconomyModel, Faction, FactionId, GalaxyState, HistoryKind, Rng, SettlementId, SystemId, names,
};

/// Stability closes the gap to its target over about this many days:
/// each monthly review moves it `REVIEW_DAYS / STABILITY_DAYS` of the way.
const STABILITY_DAYS: f64 = 720.0;
/// Stability lost per `DISTANCE_SCALE_LY` of lanes from the capital.
pub const DISTANCE_WEIGHT: f64 = 0.5;
pub const DISTANCE_SCALE_LY: f64 = 60.0;
/// Stability lost everywhere in a faction per e-fold of its people past
/// `OVEREXTENSION_PEOPLE`: big realms are harder to hold together.
pub const OVEREXTENSION_WEIGHT: f64 = 0.2;
pub const OVEREXTENSION_PEOPLE: f64 = 2000.0;
/// A breakaway needs at least this share of its parent's people, so a
/// great power isn't nibbled to dust by its outposts.
pub const SECESSION_SHARE: f64 = 0.02;
/// Stability lost at total hunger, and on top of that during a famine.
const HUNGER_WEIGHT: f64 = 0.4;
const FAMINE_SHOCK: f64 = 0.3;
/// A faction's systems below this mean stability are unstable.
pub const UNSTABLE: f64 = 0.35;
/// Unstable systems this close by lane count as one region.
const REGION_LY: f64 = 60.0;
/// Yearly chance an unstable region breaks away.
const SECESSION_CHANCE: f64 = 0.2;
/// A breakaway needs this many people to stand on its own; scattered
/// outposts stay dependent however unhappy they are.
pub const SECESSION_POPULATION: f64 = 20.0;
/// Stability of settlements in a newly independent faction: the first
/// years of freedom are hopeful.
const INDEPENDENCE_STABILITY: f64 = 0.8;
/// Chance a breakaway switches between planned and market economies.
const ECONOMY_FLIP: f64 = 0.3;
/// Chance a breakaway names itself after its parent rather than its own
/// capital, when that name is free.
const PARENT_NAME: f64 = 0.5;
/// At most this many factions share a stem, so a family's names stay
/// telling apart.
const STEM_FAMILY: usize = 3;
/// Day of the year when secessions happen.
const SECESSION_DAY: u64 = 180;
/// Day in the review cycle when settlements reconsider their loyalty;
/// apart from construction (0) and colonists (15).
const REVIEW_OFFSET: u64 = 22;

/// Lane distance from a capital's system to every system, in ly, indexed
/// by system; infinite where no lane leads. The lanes never change, so
/// these are remembered per capital system. Not saved.
#[derive(Default)]
pub struct Distances(HashMap<SystemId, Arc<[f32]>>);

impl Clone for Distances {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for Distances {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Distances")
    }
}

/// Lane distances from `origin` to every system.
fn distances_from(state: &mut GalaxyState, origin: SystemId) -> Arc<[f32]> {
    if let Some(d) = state.capital_distances.0.get(&origin) {
        return d.clone();
    }
    let mut dense = vec![f32::INFINITY; state.systems.len()];
    crate::routes::search(
        state,
        origin,
        |lane| lane.length_ly,
        |step| {
            dense[step.system.index()] = step.cost as f32;
            crate::routes::Visit::Expand
        },
    );
    let dense: Arc<[f32]> = dense.into();
    state.capital_distances.0.insert(origin, dense.clone());
    dense
}

/// Lane distance from a faction's capital to `system`, if it has one.
fn from_capital(state: &mut GalaxyState, faction: FactionId, system: SystemId) -> Option<f64> {
    let capital = state.factions.get(faction)?.capital;
    let origin = state.settlements.get(capital)?.system;
    Some(f64::from(distances_from(state, origin)[system.index()]))
}

/// Where a settlement's stability is heading, `distance_ly` from its
/// capital (or nowhere near it).
pub fn target(state: &GalaxyState, id: SettlementId, distance_ly: Option<f64>) -> f64 {
    let s = &state.settlements[id];
    if state.factions.get(s.owner).is_some_and(|f| f.capital == id) {
        return 1.0;
    }
    let distance = distance_ly.unwrap_or(f64::INFINITY) / DISTANCE_SCALE_LY;
    let famine = if s.economy.famine { FAMINE_SHOCK } else { 0.0 };
    let people = state.factions.get(s.owner).map_or(0.0, |f| f.population);
    let overextension = OVEREXTENSION_WEIGHT * (people / OVEREXTENSION_PEOPLE).max(1.0).ln();
    (1.0 - DISTANCE_WEIGHT * distance - overextension - HUNGER_WEIGHT * s.economy.hunger - famine)
        .clamp(0.0, 1.0)
}

/// Planning phase, politics part: the settlements reviewing today move
/// toward their target stability; once a year, unstable regions may break
/// away.
pub fn update(state: &mut GalaxyState) {
    let day = state.time.days();
    let rate = REVIEW_DAYS as f64 / STABILITY_DAYS;
    for i in review_slots(day, REVIEW_OFFSET, state.settlements.slot_count()) {
        let Some(id) = state.settlements.key_at(i) else {
            continue;
        };
        crate::decisions::time(crate::decisions::Decision::Politics, || {
            let s = &state.settlements[id];
            let distance = from_capital(state, s.owner, s.system);
            let target = target(state, id, distance);
            let s = &mut state.settlements[id];
            s.stability += (target - s.stability) * rate;
        });
    }
    if state.time.day_of_year() == SECESSION_DAY {
        secessions(state);
    }
}

/// A faction's systems by the mean stability of its settlements there,
/// weighted by population. The capital's system never breaks away.
fn unstable_systems(state: &GalaxyState, faction: FactionId) -> Vec<(SystemId, f64)> {
    let capital_system = state
        .settlements
        .get(state.factions[faction].capital)
        .map(|c| c.system);
    let mut by_system: HashMap<SystemId, (f64, f64, f64)> = HashMap::new();
    for s in state.settlements.values() {
        if s.owner == faction && Some(s.system) != capital_system {
            let weight = s.economy.population.max(1e-3);
            let e = by_system.entry(s.system).or_default();
            e.0 += s.stability * weight;
            e.1 += weight;
            e.2 += s.economy.population;
        }
    }
    let mut out: Vec<(SystemId, f64)> = by_system
        .into_iter()
        .filter(|(_, (st, w, _))| st / w < UNSTABLE)
        .map(|(sys, (_, _, pop))| (sys, pop))
        .collect();
    out.sort_by_key(|&(sys, _)| sys);
    out
}

/// Unstable systems grouped into regions: systems within [`REGION_LY`]
/// of lanes of each other are one region.
fn regions(state: &GalaxyState, unstable: &[(SystemId, f64)]) -> Vec<Vec<SystemId>> {
    let members: HashSet<SystemId> = unstable.iter().map(|&(s, _)| s).collect();
    let mut seen: HashSet<SystemId> = HashSet::new();
    let mut out = Vec::new();
    for &(start, _) in unstable {
        if !seen.insert(start) {
            continue;
        }
        let mut region = Vec::new();
        let mut queue = VecDeque::from([start]);
        while let Some(system) = queue.pop_front() {
            region.push(system);
            for &(near, _, _) in crate::haul::reach(state, system, REGION_LY).iter() {
                if members.contains(&near) && seen.insert(near) {
                    queue.push_back(near);
                }
            }
        }
        region.sort();
        out.push(region);
    }
    out
}

/// Unstable regions with enough people may break away; normally run once
/// a year by [`update`].
pub fn secessions(state: &mut GalaxyState) {
    let year = state.time.year();
    let factions: Vec<FactionId> = state.factions.keys().collect();
    for faction in factions {
        let unstable = unstable_systems(state, faction);
        if unstable.is_empty() {
            continue;
        }
        let population: HashMap<SystemId, f64> = unstable.iter().copied().collect();
        let needed = needed(state, faction);
        for region in regions(state, &unstable) {
            let people: f64 = region.iter().map(|s| population[s]).sum();
            if people < needed {
                continue;
            }
            let key = region[0].index() as u64;
            if Rng::stream(state.seed, "secession", year, key).chance(SECESSION_CHANCE) {
                secede(state, faction, &region);
            }
        }
    }
}

/// People a region of `faction` needs to break away.
fn needed(state: &GalaxyState, faction: FactionId) -> f64 {
    let share = SECESSION_SHARE * state.factions[faction].population;
    share.max(SECESSION_POPULATION)
}

/// Settlements of `faction` in `systems`.
fn held(state: &GalaxyState, faction: FactionId, systems: &[SystemId]) -> Vec<SettlementId> {
    let mut out: Vec<SettlementId> = systems
        .iter()
        .flat_map(|&sys| state.settlements_in(sys))
        .filter(|(_, s)| s.owner == faction)
        .map(|(id, _)| id)
        .collect();
    out.sort();
    out
}

/// A region breaks away from `parent` as a new faction, if what is left
/// of it after keeping only the systems nearer its new capital than the
/// old one still has the people to stand alone.
fn secede(state: &mut GalaxyState, parent: FactionId, region: &[SystemId]) {
    let now = state.time;
    let settlements = held(state, parent, region);
    let Some(&capital) = settlements.iter().max_by(|&&a, &&b| {
        let pa = state.settlements[a].economy.population;
        let pb = state.settlements[b].economy.population;
        pa.total_cmp(&pb).then(b.cmp(&a))
    }) else {
        return;
    };
    let new_capital = distances_from(state, state.settlements[capital].system);
    let Some(old_capital) = state
        .settlements
        .get(state.factions[parent].capital)
        .map(|c| c.system)
    else {
        return;
    };
    let old_capital = distances_from(state, old_capital);
    let leaving: Vec<SettlementId> = settlements
        .into_iter()
        .filter(|&id| {
            let i = state.settlements[id].system.index();
            new_capital[i] < old_capital[i]
        })
        .collect();
    let people: f64 = leaving
        .iter()
        .map(|&id| state.settlements[id].economy.population)
        .sum();
    if people < needed(state, parent) {
        return;
    }

    let mut rng = Rng::stream(
        state.seed,
        "secession-founding",
        now.days(),
        state.settlements[capital].system.index() as u64,
    );
    let old = &state.factions[parent];
    let economy = if rng.chance(ECONOMY_FLIP) {
        match old.economy {
            EconomyModel::Planned => EconomyModel::Market,
            EconomyModel::Market => EconomyModel::Planned,
        }
    } else {
        old.economy
    };
    let color = drift(&mut rng, old.color);
    let parent_stem = if old.stem.is_empty() {
        names::stem(&old.name).to_owned()
    } else {
        old.stem.clone()
    };
    let capital_stem = names::stem(&state.settlements[capital].name).to_owned();
    let (name, stem) = breakaway_name(state, &mut rng, &parent_stem, &capital_stem, economy);
    let leader = names::person(&mut rng);

    let child = state.factions.insert(Faction {
        name,
        economy,
        capital,
        color,
        founded: now,
        treasury: 0.0,
        population: 0.0,
        welfare: Default::default(),
        parent: Some(parent),
        stem,
    });
    for &id in &leaving {
        let s = &mut state.settlements[id];
        s.owner = child;
        s.stability = INDEPENDENCE_STABILITY;
    }
    crate::demography::refresh_faction_populations(state);
    state
        .history
        .record(now, HistoryKind::FactionFounded { faction: child });
    state.history.record(
        now,
        HistoryKind::Secession {
            parent,
            child,
            settlements: leaving,
            leader,
        },
    );
}

/// A name for a breakaway, not already taken, with its stem: built on its
/// parent's stem (the Free Ports of Kessa broke from the Union of Kessa)
/// or on its own capital's, or failing both on a new word.
fn breakaway_name(
    state: &GalaxyState,
    rng: &mut Rng,
    parent_stem: &str,
    capital_stem: &str,
    economy: EconomyModel,
) -> (String, String) {
    let taken: HashSet<&str> = state.factions.values().map(|f| f.name.as_str()).collect();
    let carrying = |stem: &str| state.factions.values().filter(|f| f.stem == stem).count();
    let mut stems = Vec::with_capacity(2);
    if rng.chance(PARENT_NAME) && carrying(parent_stem) < STEM_FAMILY {
        stems.push(parent_stem);
    }
    stems.push(capital_stem);
    for stem in stems {
        for _ in 0..4 {
            let name = names::faction_name(rng, stem, economy);
            if !taken.contains(name.as_str()) {
                return (name, stem.to_owned());
            }
        }
    }
    loop {
        let stem = names::word(rng);
        let name = names::faction_name(rng, &stem, economy);
        if !taken.contains(name.as_str()) {
            return (name, stem);
        }
    }
}

/// A colour near the parent's, so families read as families on a map.
fn drift(rng: &mut Rng, [r, g, b]: [u8; 3]) -> [u8; 3] {
    let mut shift = |c: u8| (f64::from(c) + rng.range(-40.0, 40.0)).clamp(40.0, 235.0) as u8;
    [shift(r), shift(g), shift(b)]
}

/// A faction's family line: itself, its parent, and so on back to a
/// founding nation.
pub fn lineage(state: &GalaxyState, faction: FactionId) -> Vec<FactionId> {
    let mut line = vec![faction];
    let mut current = faction;
    while let Some(parent) = state.factions.get(current).and_then(|f| f.parent) {
        if line.contains(&parent) {
            break;
        }
        line.push(parent);
        current = parent;
    }
    line
}

/// How fragmented the galaxy is, for checking that history neither
/// converges on one empire nor crumbles to dust (DESIGN §5.5.1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Balance {
    /// Factions that hold at least one settlement.
    pub living: usize,
    /// Living factions holding at least [`MAJOR_SHARE`] of all people.
    pub major: usize,
    /// The largest faction's share of all people.
    pub largest_share: f64,
    /// Effective number of factions: one over the sum of squared
    /// population shares (an inverse Herfindahl index). Four equal powers
    /// score 4; one empire with crumbs scores near 1.
    pub effective: f64,
}

/// A faction with this share of the galaxy's people counts as a power.
pub const MAJOR_SHARE: f64 = 0.05;

pub fn balance(state: &GalaxyState) -> Balance {
    let mut people: HashMap<FactionId, f64> = HashMap::new();
    for s in state.settlements.values() {
        *people.entry(s.owner).or_default() += s.economy.population;
    }
    let total: f64 = people.values().sum();
    if total <= 0.0 {
        return Balance::default();
    }
    let shares = people.values().map(|p| p / total);
    Balance {
        living: people.len(),
        major: shares.clone().filter(|&s| s >= MAJOR_SHARE).count(),
        largest_share: shares.clone().fold(0.0, f64::max),
        effective: 1.0 / shares.map(|s| s * s).sum::<f64>(),
    }
}
