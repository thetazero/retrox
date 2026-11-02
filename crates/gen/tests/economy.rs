use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::economy::{self, FacilityKind, Good};
use retrox_sim::haul;
use retrox_sim::{GalaxyState, tick};

fn small() -> GalaxyState {
    generate(
        5,
        &GalaxyConfig {
            systems: 800,
            radius_ly: 110.0,
            factions: 6,
            ..GalaxyConfig::default()
        },
    )
}

#[test]
fn hauling_conserves_goods() {
    let mut state = small();
    for _ in 0..30 {
        tick(&mut state);
    }
    // Harvesters bring new goods in from space (see tests/harvest.rs);
    // trade itself only moves them.
    let harvesters: Vec<_> = state
        .companies
        .iter()
        .filter(|(_, c)| c.fleets.iter().any(|f| f.harvest.is_some()))
        .map(|(k, _)| k)
        .collect();
    for k in harvesters {
        state.companies.remove(k);
    }
    let before = haul::total_goods(&state);
    haul::trade(&mut state);
    let after = haul::total_goods(&state);
    for good in Good::ALL {
        // Ships burn fuel; everything else only moves.
        if good == Good::Fuel {
            assert!(after[good] <= before[good] + 1e-6);
            continue;
        }
        let (b, a) = (before[good], after[good]);
        assert!((a - b).abs() <= 1e-9 * b.max(1.0), "{good:?}: {b} -> {a}");
    }
}

#[test]
fn ships_keep_trading() {
    let mut state = small();
    for _ in 0..360 {
        tick(&mut state);
    }
    let e = economy::summarize(&state);
    assert!(e.ships > 0);
    assert!(
        e.ships_travelling * 2 > e.ships,
        "only {} of {} ships are travelling after a year",
        e.ships_travelling,
        e.ships
    );
}

#[test]
fn economy_stays_healthy() {
    let mut state = small();
    for _ in 0..360 * 5 {
        tick(&mut state);
    }
    for s in state.settlements.values() {
        for good in Good::ALL {
            let m = &s.economy.market;
            assert!(
                m.stock[good].is_finite() && m.stock[good] >= -1e-9,
                "{good:?} stock"
            );
            assert!(
                m.price[good].is_finite() && m.price[good] > 0.0,
                "{good:?} price"
            );
        }
    }
    for s in state.settlements.values() {
        assert!(
            s.economy.treasury.is_finite() && s.economy.treasury >= -1e-6,
            "{} treasury {}",
            s.name,
            s.economy.treasury
        );
    }
    // Market settlements that run out of money still go short, so this is a
    // loose bound for now.
    let summary = economy::summarize(&state);
    let unmet = summary.shortfall[Good::Food] / summary.demand[Good::Food];
    assert!(unmet < 0.35, "{:.0}% of food demand unmet", unmet * 100.0);
}

#[test]
fn money_is_conserved() {
    // Trade, taxes, welfare and contracts only move money between
    // settlements, companies and factions.
    let mut state = small();
    let total = |state: &GalaxyState| {
        let e = economy::summarize(state);
        e.money + e.faction_balance
    };
    for _ in 0..30 {
        tick(&mut state);
    }
    let before = total(&state);
    for _ in 0..60 {
        tick(&mut state);
    }
    let after = total(&state);
    assert!(
        (after - before).abs() <= 1e-6 * before.abs(),
        "{before} -> {after}"
    );
}

/// Fleets are sized to the freight there is (#72): generation starts with
/// more ships than freight, and the idle ones go for scrap, while
/// shipyards keep building for new companies where there is freight.
#[test]
fn fleets_follow_freight() {
    let mut state = small();
    assert!(
        state.settlements.values().any(|s| s
            .economy
            .facilities
            .iter()
            .any(|f| f.kind == FacilityKind::Shipyard)),
        "no shipyards"
    );
    let start: std::collections::HashSet<_> = state.companies.keys().collect();
    for _ in 0..360 * 3 {
        tick(&mut state);
    }
    let founded = state
        .companies
        .keys()
        .filter(|k| !start.contains(k))
        .count();
    assert!(founded > 0, "no new companies in three years");
    let (mut ships, mut idle) = (0, 0);
    for fleet in state.companies.values().flat_map(|c| c.fleets.iter()) {
        ships += fleet.ships;
        if fleet.idle || fleet.slack >= haul::SLACK_REVIEWS {
            idle += fleet.ships;
        }
    }
    assert!(ships > 0, "no ships left");
    assert!(
        idle * 5 < ships,
        "{idle} of {ships} ships have nothing to carry"
    );
}

#[test]
fn populations_change_and_stay_sane() {
    let mut state = small();
    let start: Vec<f64> = state
        .settlements
        .values()
        .map(|s| s.economy.population)
        .collect();
    for _ in 0..360 * 2 {
        tick(&mut state);
    }
    let end: Vec<f64> = state
        .settlements
        .values()
        .map(|s| s.economy.population)
        .collect();
    assert!(end.iter().all(|p| p.is_finite() && *p >= 0.0));
    let changed = start
        .iter()
        .zip(&end)
        .filter(|(a, b)| (*a - *b).abs() > 1e-3 * a.max(1.0))
        .count();
    assert!(
        changed * 2 > start.len(),
        "only {changed} populations changed"
    );
}

#[test]
fn construction_builds_without_running_away() {
    let mut state = small();
    let scale = |state: &GalaxyState| -> f64 {
        state
            .settlements
            .values()
            .flat_map(|s| s.economy.facilities.iter())
            .map(|f| f.scale)
            .sum()
    };
    let start = scale(&state);
    let mut projects = 0;
    for _ in 0..360 * 3 {
        tick(&mut state);
        projects = projects.max(
            state
                .settlements
                .values()
                .filter(|s| s.economy.project.is_some())
                .count(),
        );
    }
    let end = scale(&state);
    assert!(projects > 0, "nothing was ever built");
    assert!(end > start, "industry shrank: {start} -> {end}");
    assert!(end < 3.0 * start, "industry ran away: {start} -> {end}");
}

#[test]
fn crowded_settlements_send_colonists() {
    let mut state = small();
    let start = state.settlements.len();
    let mut launched = false;
    for _ in 0..360 * 3 {
        tick(&mut state);
        launched |= !state.colony_ships.is_empty();
    }
    assert!(launched, "no colony ship ever launched");
    assert!(
        state.settlements.len() > start,
        "no new settlements: {start} -> {}",
        state.settlements.len()
    );
    // New settlements belong to real factions and sit in their system.
    for (id, s) in &state.settlements {
        assert!(state.factions.contains_key(s.owner));
        assert!(state.systems[s.system].settlements.contains(&id));
    }
}

#[test]
fn stalled_projects_are_cut_short() {
    use retrox_sim::economy::{GoodMap, Project};
    let mut state = small();
    let id = state
        .settlements
        .iter()
        .find(|(_, s)| s.economy.population > 1.0)
        .map(|(id, _)| id)
        .expect("a populated settlement");
    // More material than the galaxy holds: it can never finish.
    let mut needs = GoodMap::splat(0.0);
    needs[Good::Metal] = 2e9;
    needs[Good::Components] = 3e8;
    let economy = &mut state.settlements[id].economy;
    economy.project = Some(Project {
        kind: FacilityKind::Refinery,
        scale: 1e8,
        needs,
        started: state.time,
    });
    economy.refresh_demand();
    for _ in 0..200 {
        tick(&mut state);
    }
    let project = &state.settlements[id].economy.project;
    assert!(
        project.as_ref().is_none_or(|p| p.scale < 1e8),
        "the impossible project is still there"
    );
}

#[test]
fn planned_settlements_build_shipyards_for_orders() {
    use retrox_sim::EconomyModel;
    let mut state = small();
    let id = state
        .settlements
        .iter()
        .max_by(|a, b| a.1.economy.population.total_cmp(&b.1.economy.population))
        .map(|(id, _)| id)
        .expect("a settlement");
    let owner = state.settlements[id].owner;
    state.factions[owner].economy = EconomyModel::Planned;
    let economy = &mut state.settlements[id].economy;
    economy.population = economy.population.max(100.0);
    economy
        .facilities
        .retain(|f| f.kind != FacilityKind::Shipyard);
    if !economy
        .facilities
        .iter()
        .any(|f| f.kind == FacilityKind::Factory)
    {
        economy
            .facilities
            .push(economy::Facility::new(FacilityKind::Factory, 1.0));
    }
    for f in &mut economy.facilities {
        f.utilization = 1.0;
    }
    economy.refresh_demand();
    // Plenty of everything; only hull is short.
    for good in Good::ALL {
        economy.market.stock[good] = 2.0 * economy.market.target(good);
    }
    economy.hull_t = 0.0;
    economy.hull_wanted = 500.0;
    let project = retrox_sim::construction::choose(&state, id).expect("a project");
    assert_eq!(project.kind, FacilityKind::Shipyard);
}
