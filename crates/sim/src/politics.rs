//! Stability and secession (DESIGN §5.5, issue #18): how new factions are
//! born.
//!
//! Every settlement has a stability between 0 and 1 that drifts toward a
//! target over about a year. The target falls with lane distance from the
//! faction's capital and with hunger. Once a year, a faction's unstable
//! settlements, grouped by lane connectivity, may break away together as a
//! new faction. The new faction remembers its parent, inherits its economic
//! model (usually), and takes a name from its largest settlement. This loop
//! is what keeps history from settling into one empire.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    EconomyModel, Faction, FactionId, GalaxyState, HistoryKind, Rng, SettlementId, SystemId, names,
};

/// Days for stability to move most of the way to its target.
const STABILITY_DAYS: f64 = 360.0;
/// Lost stability at `DISTANCE_SCALE_LY` or more from the capital.
const DISTANCE_WEIGHT: f64 = 0.6;
const DISTANCE_SCALE_LY: f64 = 120.0;
/// Lost stability at total hunger.
const HUNGER_WEIGHT: f64 = 0.5;
/// Settlements below this are unstable.
const UNSTABLE: f64 = 0.35;
/// Yearly chance an unstable group breaks away.
const SECESSION_CHANCE: f64 = 0.25;
/// A group needs this many people between them to stand on its own; tiny
/// outposts stay dependent however unhappy they are.
const SECESSION_POPULATION: f64 = 30.0;
/// Stability of settlements in a newly independent faction.
const INDEPENDENCE_STABILITY: f64 = 0.8;
/// Chance a breakaway switches between planned and market economies.
const ECONOMY_FLIP: f64 = 0.3;
/// Day of the year when secessions happen.
const SECESSION_DAY: u64 = 180;
/// Effectively unbounded range, for distances from a capital.
const ANYWHERE_LY: f64 = 1e9;

/// Lane distance from each faction's capital system to every system it
/// can reach.
fn distances(state: &GalaxyState) -> HashMap<FactionId, HashMap<SystemId, f64>> {
    state
        .factions
        .iter()
        .filter_map(|(id, f)| {
            let capital = state.settlements.get(f.capital)?;
            let reached = crate::haul::reach(state, capital.system, ANYWHERE_LY);
            Some((id, reached.iter().copied().collect()))
        })
        .collect()
}

/// Where a settlement's stability is heading.
fn target(state: &GalaxyState, id: SettlementId, distance_ly: Option<f64>) -> f64 {
    let s = &state.settlements[id];
    if state.factions.get(s.owner).is_some_and(|f| f.capital == id) {
        return 1.0;
    }
    let distance = distance_ly.map_or(1.0, |ly| (ly / DISTANCE_SCALE_LY).min(1.0));
    (1.0 - DISTANCE_WEIGHT * distance - HUNGER_WEIGHT * s.economy.hunger).clamp(0.0, 1.0)
}

/// Planning phase, politics part: stability drifts monthly, each settlement
/// on its own day; secessions happen once a year.
pub fn update(state: &mut GalaxyState) {
    let day = state.time.days();
    let reviewing: Vec<SettlementId> = state
        .settlements
        .keys()
        .filter(|&id| slotmap::Key::data(&id).as_ffi().wrapping_add(7) % 30 == day % 30)
        .collect();
    if !reviewing.is_empty() {
        let distances = distances(state);
        for id in reviewing {
            let s = &state.settlements[id];
            let distance = distances
                .get(&s.owner)
                .and_then(|d| d.get(&s.system))
                .copied();
            let target = target(state, id, distance);
            let s = &mut state.settlements[id];
            s.stability += (target - s.stability) * (30.0 / STABILITY_DAYS);
        }
    }
    if state.time.day_of_year() == SECESSION_DAY {
        secessions(state);
    }
}

/// Unstable settlements of one faction, grouped by lane connectivity
/// between their systems. The capital never takes part.
fn unstable_groups(state: &GalaxyState, faction: FactionId) -> Vec<Vec<SettlementId>> {
    let capital = state.factions[faction].capital;
    let mut by_system: HashMap<SystemId, Vec<SettlementId>> = HashMap::new();
    for (id, s) in &state.settlements {
        if s.owner == faction && id != capital && s.stability < UNSTABLE {
            by_system.entry(s.system).or_default().push(id);
        }
    }
    let mut systems: Vec<SystemId> = by_system.keys().copied().collect();
    systems.sort();
    let mut seen: HashSet<SystemId> = HashSet::new();
    let mut groups = Vec::new();
    for start in systems {
        if !seen.insert(start) {
            continue;
        }
        let mut group = Vec::new();
        let mut queue = VecDeque::from([start]);
        while let Some(system) = queue.pop_front() {
            group.extend(by_system[&system].iter().copied());
            for next in state.neighbours(system) {
                if by_system.contains_key(&next) && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        groups.push(group);
    }
    groups
}

/// Unstable groups with enough people may break away; normally run once a
/// year by [`update`].
pub fn secessions(state: &mut GalaxyState) {
    let year = state.time.year();
    let factions: Vec<FactionId> = state.factions.keys().collect();
    for faction in factions {
        for group in unstable_groups(state, faction) {
            let population: f64 = group
                .iter()
                .map(|&id| state.settlements[id].economy.population)
                .sum();
            if population < SECESSION_POPULATION {
                continue;
            }
            let key = slotmap::Key::data(&group[0]).as_ffi();
            if Rng::stream(state.seed, "secession", year, key).chance(SECESSION_CHANCE) {
                secede(state, faction, &group);
            }
        }
    }
}

/// A group of settlements breaks away from `parent` as a new faction.
fn secede(state: &mut GalaxyState, parent: FactionId, group: &[SettlementId]) {
    let now = state.time;
    let capital = *group
        .iter()
        .max_by(|&&a, &&b| {
            let pa = state.settlements[a].economy.population;
            let pb = state.settlements[b].economy.population;
            pa.total_cmp(&pb).then(b.cmp(&a))
        })
        .expect("groups aren't empty");
    let mut rng = Rng::stream(
        state.seed,
        "secession-founding",
        now.days(),
        slotmap::Key::data(&capital).as_ffi(),
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
    let base = state.settlements[capital]
        .name
        .split(' ')
        .next()
        .unwrap_or("Free")
        .to_owned();
    let name = unique_name(state, &mut rng, &base, economy);
    let leader = format!("{} {}", names::word(&mut rng), names::word(&mut rng));

    let child = state.factions.insert(Faction {
        name,
        economy,
        capital,
        color,
        founded: now,
        treasury: 0.0,
        population: 0.0,
        parent: Some(parent),
    });
    for &id in group {
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
            leader,
        },
    );
}

/// A faction name from its largest settlement, in the style of its
/// economy, not already taken.
fn unique_name(state: &GalaxyState, rng: &mut Rng, base: &str, economy: EconomyModel) -> String {
    let templates: &[&str] = match economy {
        EconomyModel::Planned => &[
            "{} Commune",
            "People's Republic of {}",
            "{} Cooperative",
            "{} Collective",
        ],
        EconomyModel::Market => &["Free Ports of {}", "{} Republic", "{} League", "{} Compact"],
    };
    let taken: HashSet<&str> = state.factions.values().map(|f| f.name.as_str()).collect();
    for template in templates {
        let name = template.replace("{}", base);
        if !taken.contains(name.as_str()) {
            return name;
        }
    }
    loop {
        let name = rng.pick(templates).replace("{}", &names::word(rng));
        if !taken.contains(name.as_str()) {
            return name;
        }
    }
}

/// A colour near the parent's, so families read as families on a map.
fn drift(rng: &mut Rng, [r, g, b]: [u8; 3]) -> [u8; 3] {
    let mut shift = |c: u8| (f64::from(c) + rng.range(-50.0, 50.0)).clamp(30.0, 240.0) as u8;
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
