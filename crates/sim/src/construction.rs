//! Construction (DESIGN §5.7, issue #17): settlements build the facilities
//! they need, and let idle ones decay.
//!
//! Once a month each settlement without a project may start one. Market
//! settlements build whatever earns the best margin at local prices and
//! pays for itself within a couple of years; planned settlements build
//! whatever produces the good they're shortest of. Either way a project
//! needs metal and components from the local market, which raises demand
//! there so haulers bring them in, and takes at least a month. Planned
//! factions pay construction wages to the settlement, which is where their
//! tax money goes back into circulation.

use crate::economy::{
    Facility, FacilityKind, Good, GoodMap, HULL_PRICE_PER_T, HULL_T_PER_RUN, Project,
};
use crate::ledger::{self, Account};
use crate::{EconomyModel, GalaxyState, Resources, SettlementId};

/// Shortest time a project can take, in days; its materials are wanted
/// over this long.
pub const BUILD_DAYS: f64 = 30.0;
/// A project still short of materials after this many days is cut
/// short at whatever share of it has been built.
const PATIENCE_DAYS: u64 = 180;
/// Materials per unit of scale added.
const METAL_PER_SCALE: f64 = 20.0;
const COMPONENTS_PER_SCALE: f64 = 3.0;
/// A market project must pay back its materials within this many days.
const PAYBACK_DAYS: f64 = 720.0;
/// Planned settlements build for goods below this fill.
const PLAN_FILL: f64 = 0.8;
/// Credits a planned faction pays in wages per unit of material used.
const WAGE_PER_UNIT: f64 = 15.0;
/// Facilities running below this utilization lose this share of their
/// scale each month, and close below `MIN_SCALE`.
const IDLE_UTILIZATION: f64 = 0.2;
const DECAY_PER_MONTH: f64 = 0.05;
const MIN_SCALE: f64 = 0.1;
/// Resources richer than this can be mined.
const MINEABLE: f64 = 0.25;
/// Shipyards need this much population.
const SHIPYARD_POPULATION: f64 = 30.0;
/// Only expand a kind whose existing facilities run at least this hard.
const BUSY: f64 = 0.8;
/// Only build what has its inputs: each input at least this full.
const INPUT_FILL: f64 = 0.5;
/// Market settlements build only for goods below this fill: no gluts.
const MARKET_FILL: f64 = 1.0;
/// A settlement's total scale of one kind stays under this much per unit
/// of population, plus `SCALE_BASE`.
const SCALE_PER_POPULATION: f64 = 3.0;
const SCALE_BASE: f64 = 5.0;

/// The richest of each resource among a system's bodies.
fn reachable_resources(state: &GalaxyState, id: SettlementId) -> Resources {
    let system = state.settlements[id].system;
    state.systems[system]
        .bodies
        .iter()
        .fold(Resources::default(), |acc, &b| {
            let r = state.bodies[b].resources;
            Resources {
                ore: acc.ore.max(r.ore),
                ice: acc.ice.max(r.ice),
                gas: acc.gas.max(r.gas),
            }
        })
}

/// Facility kinds this settlement could sensibly build now: the resources
/// are there, the inputs are on hand, existing facilities of the kind are
/// busy, and it isn't over its cap.
fn buildable(state: &GalaxyState, id: SettlementId) -> Vec<FacilityKind> {
    let resources = reachable_resources(state, id);
    let economy = &state.settlements[id].economy;
    let market = &economy.market;
    FacilityKind::ALL
        .into_iter()
        .filter(|&kind| {
            let existing: Vec<&Facility> = economy
                .facilities
                .iter()
                .filter(|f| f.kind == kind)
                .collect();
            let scale: f64 = existing.iter().map(|f| f.scale).sum();
            existing.iter().all(|f| f.utilization >= BUSY)
                && scale < SCALE_PER_POPULATION * economy.population + SCALE_BASE
                && kind
                    .recipe()
                    .inputs
                    .iter()
                    .all(|&(g, _)| market.fill(g) >= INPUT_FILL)
        })
        .filter(|&kind| match kind {
            FacilityKind::Mine => resources.ore > MINEABLE,
            FacilityKind::IceHarvester => resources.ice > MINEABLE,
            // Gas plants sit by a giant their harvesters can skim.
            FacilityKind::GasPlant => resources.gas > MINEABLE,
            // Only where captains want more hull than there is.
            FacilityKind::Shipyard => {
                economy.hull_wanted > economy.hull_t
                    && economy.population > SHIPYARD_POPULATION
                    && economy
                        .facilities
                        .iter()
                        .any(|f| f.kind == FacilityKind::Factory)
            }
            _ => true,
        })
        .collect()
}

/// Daily margin per unit of scale at local prices.
fn margin(state: &GalaxyState, id: SettlementId, kind: FacilityKind) -> f64 {
    let market = &state.settlements[id].economy.market;
    let recipe = kind.recipe();
    let out: f64 = recipe
        .outputs
        .iter()
        .map(|&(g, a)| a * market.price[g])
        .sum();
    let out = if kind == FacilityKind::Shipyard {
        HULL_T_PER_RUN * HULL_PRICE_PER_T
    } else {
        out
    };
    let cost: f64 = recipe
        .inputs
        .iter()
        .map(|&(g, a)| a * market.price[g])
        .sum();
    out - cost
}

fn materials(scale: f64) -> GoodMap<f64> {
    let mut needs = GoodMap::splat(0.0);
    needs[Good::Metal] = METAL_PER_SCALE * scale;
    needs[Good::Components] = COMPONENTS_PER_SCALE * scale;
    needs
}

/// How much to add: a fifth of what's there, at least a little.
fn increment(state: &GalaxyState, id: SettlementId, kind: FacilityKind) -> f64 {
    let economy = &state.settlements[id].economy;
    let existing: f64 = economy
        .facilities
        .iter()
        .filter(|f| f.kind == kind)
        .map(|f| f.scale)
        .sum();
    (0.2 * existing).max(0.02 * economy.population).max(1.0)
}

/// What this settlement should build next, if anything.
pub fn choose(state: &GalaxyState, id: SettlementId) -> Option<Project> {
    let settlement = &state.settlements[id];
    let market = &settlement.economy.market;
    let kinds = buildable(state, id);
    let kind = match state.factions[settlement.owner].economy {
        EconomyModel::Market => kinds
            .into_iter()
            .filter_map(|kind| {
                let good = kind.recipe().outputs.first().map(|o| o.0);
                if good.is_some_and(|g| market.fill(g) >= MARKET_FILL) {
                    return None;
                }
                let m = margin(state, id, kind);
                let scale = increment(state, id, kind);
                let cost: f64 = Good::ALL
                    .iter()
                    .map(|&g| materials(scale)[g] * market.price[g])
                    .sum();
                (m > 0.0 && cost / (m * scale) < PAYBACK_DAYS).then_some((kind, m))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(kind, _)| kind)?,
        EconomyModel::Planned => {
            // The plan: build for the good we're shortest of. Hull counts
            // as a good: how much of what captains want is on hand.
            let economy = &settlement.economy;
            kinds
                .into_iter()
                .filter_map(|kind| {
                    let fill = match kind.recipe().outputs.first() {
                        Some(&(good, _)) => market.fill(good),
                        None => economy.hull_t / economy.hull_wanted.max(1e-9),
                    };
                    (fill < PLAN_FILL).then_some((kind, fill))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(kind, _)| kind)?
        }
    };
    let scale = increment(state, id, kind);
    Some(Project {
        kind,
        scale,
        needs: materials(scale),
        started: state.time,
    })
}

/// Planning phase, construction part: start projects, make progress on
/// them, and let idle facilities decay. Each settlement reviews once a
/// month, on its own day.
pub fn build(state: &mut GalaxyState) {
    let day = state.time.days();
    let ids: Vec<SettlementId> = state.settlements.keys().collect();
    for id in ids {
        if state.settlements[id].economy.population <= 0.0 {
            continue;
        }
        let review = (slotmap::Key::data(&id).as_ffi() % 30) == day % 30;
        if review {
            decay(state, id);
            state.settlements[id].economy.hull_wanted *= 0.5;
            if state.settlements[id].economy.project.is_none() {
                let project = choose(state, id);
                let economy = &mut state.settlements[id].economy;
                economy.project = project;
                economy.refresh_demand();
            }
        }
        progress(state, id);
    }
}

/// Use today's share of materials from the local market; finish when
/// nothing is left to use.
fn progress(state: &mut GalaxyState, id: SettlementId) {
    let owner = state.settlements[id].owner;
    let planned = state.factions[owner].economy == EconomyModel::Planned;
    let Some(project) = state.settlements[id].economy.project.clone() else {
        return;
    };
    let total = materials(project.scale);
    let mut needs = project.needs;
    let mut used = 0.0;
    for good in Good::ALL {
        let today = ledger::consume(state, id, good, (total[good] / BUILD_DAYS).min(needs[good]));
        needs[good] -= today;
        used += today;
    }
    let done = Good::ALL.iter().all(|&g| needs[g] <= 1e-9);
    if let Some(p) = state.settlements[id].economy.project.as_mut() {
        p.needs = needs;
    }
    if planned && used > 0.0 {
        let wages = used * WAGE_PER_UNIT * crate::fiscal::thrift(state, owner);
        ledger::pay(
            state,
            Account::Faction(owner),
            Account::Settlement(id),
            wages,
        );
    }
    let late = state.time.days().saturating_sub(project.started.days()) > PATIENCE_DAYS;
    let economy = &mut state.settlements[id].economy;
    if done || late {
        // A project that can't get the rest of its materials is cut short:
        // the settlement keeps the part it has built, and can plan again.
        let project = economy.project.take().expect("checked above");
        let share = 1.0
            - Good::ALL.iter().map(|&g| project.needs[g]).sum::<f64>()
                / Good::ALL.iter().map(|&g| total[g]).sum::<f64>().max(1e-9);
        let scale = project.scale * share.clamp(0.0, 1.0);
        if scale >= MIN_SCALE {
            match economy
                .facilities
                .iter_mut()
                .find(|f| f.kind == project.kind)
            {
                Some(f) => f.scale += scale,
                None => economy.facilities.push(Facility::new(project.kind, scale)),
            }
        }
        economy.refresh_demand();
    }
}

/// Long-idle facilities shrink, and close when there's nothing left.
fn decay(state: &mut GalaxyState, id: SettlementId) {
    let economy = &mut state.settlements[id].economy;
    for f in &mut economy.facilities {
        if f.utilization < IDLE_UTILIZATION {
            f.scale *= 1.0 - DECAY_PER_MONTH;
        }
    }
    let before = economy.facilities.len();
    economy.facilities.retain(|f| f.scale >= MIN_SCALE);
    if economy.facilities.len() != before {
        economy.refresh_demand();
    }
}
