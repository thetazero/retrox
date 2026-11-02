//! A health check on the economy after a long run (issue #72): is fuel
//! where people are, are factions drowning in debt, are fleets sized to
//! the freight there is?

use retrox_sim::economy::{FacilityKind, Good};
use retrox_sim::haul::Location;
use retrox_sim::ledger::DEBT_PER_POPULATION;
use retrox_sim::{EconomyModel, GalaxyState};

/// A market counts as short of a good when more than this share of its
/// demand for it went unmet today.
const SHORT_SHARE: f64 = 0.1;
/// A project counts as stalled after this many days.
const STALLED_DAYS: u64 = 360;

pub(crate) fn print(state: &GalaxyState) {
    let markets = state.settlements.len().max(1);
    println!("health:");

    // Shortages: markets missing more than a tenth of their demand.
    let mut line = Vec::new();
    for good in Good::ALL {
        let short = state
            .settlements
            .values()
            .filter(|s| {
                let m = &s.economy.market;
                m.demand[good] > 0.0 && m.shortfall[good] > SHORT_SHARE * m.demand[good]
            })
            .count();
        line.push(format!(
            "{} {:.0}%",
            good.name(),
            100.0 * short as f64 / markets as f64
        ));
    }
    println!("  markets >10% short: {}  (of {markets})", line.join(", "));

    // Money: who holds it, and how deep factions are in debt.
    let settlements: f64 = state.settlements.values().map(|s| s.economy.treasury).sum();
    let companies: f64 = state.companies.values().map(|c| c.credits).sum();
    println!("  money: settlements {settlements:.0}, companies {companies:.0}");
    for model in [EconomyModel::Planned, EconomyModel::Market] {
        let factions: Vec<_> = state
            .factions
            .values()
            .filter(|f| f.economy == model)
            .collect();
        let balance: f64 = factions.iter().map(|f| f.treasury).sum();
        let in_debt = factions.iter().filter(|f| f.treasury < 0.0).count();
        let limit: f64 = factions
            .iter()
            .map(|f| f.population * DEBT_PER_POPULATION)
            .sum();
        let debt: f64 = factions.iter().map(|f| (-f.treasury).max(0.0)).sum();
        println!(
            "  {model:?} factions: {} ({in_debt} in debt), balance {balance:.0}, debt {:.1}% of limit",
            factions.len(),
            100.0 * debt / limit.max(1.0)
        );
    }

    // Fleets: idle ships, empty legs, and how full the laden ones are.
    let (mut ships, mut idle, mut laden, mut empty, mut docked_cargo) = (0u32, 0u32, 0u32, 0, 0);
    let (mut carried, mut capacity) = (0.0, 0.0);
    for company in state.companies.values() {
        for fleet in &company.fleets {
            let hold = state.ship_classes[fleet.class].cargo_t * f64::from(fleet.ships);
            ships += fleet.ships;
            match (fleet.at, fleet.cargo) {
                (Location::Docked(_), Some(_)) if fleet.idle => docked_cargo += 1,
                (Location::Docked(_), _) => {}
                (_, Some(cargo)) => {
                    laden += fleet.ships;
                    carried += cargo.amount;
                    capacity += hold;
                }
                (_, None) => empty += fleet.ships,
            }
            if fleet.idle {
                idle += fleet.ships;
            }
        }
    }
    println!(
        "  ships: {ships} in {} companies; {:.0}% idle, {laden} laden ({:.0}% full), {empty} empty legs; {docked_cargo} idle fleets holding cargo",
        state.companies.len(),
        100.0 * f64::from(idle) / f64::from(ships.max(1)),
        100.0 * carried / capacity.max(1.0),
    );

    // Shipyards and hull, construction.
    let yards: Vec<_> = state
        .settlements
        .values()
        .flat_map(|s| s.economy.facilities.iter())
        .filter(|f| f.kind == FacilityKind::Shipyard)
        .collect();
    let planned_yards = state
        .settlements
        .values()
        .filter(|s| state.factions[s.owner].economy == EconomyModel::Planned)
        .filter(|s| {
            s.economy
                .facilities
                .iter()
                .any(|f| f.kind == FacilityKind::Shipyard)
        })
        .count();
    let hull: f64 = state.settlements.values().map(|s| s.economy.hull_t).sum();
    println!(
        "  shipyards: {} ({planned_yards} planned), scale {:.0}, {:.0}% utilized; {hull:.0} t of hull unsold",
        yards.len(),
        yards.iter().map(|f| f.scale).sum::<f64>(),
        100.0 * yards.iter().map(|f| f.utilization * f.scale).sum::<f64>()
            / yards.iter().map(|f| f.scale).sum::<f64>().max(1e-9),
    );
    let today = state.time.days();
    let projects: Vec<_> = state
        .settlements
        .values()
        .filter_map(|s| s.economy.project.as_ref())
        .collect();
    let stalled = projects
        .iter()
        .filter(|p| today.saturating_sub(p.started.days()) > STALLED_DAYS)
        .count();
    println!(
        "  projects: {} under way, {stalled} older than {STALLED_DAYS} days",
        projects.len()
    );
}
