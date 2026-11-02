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

#[test]
fn shipyards_grow_the_fleet() {
    let mut state = small();
    let start = economy::summarize(&state).ships;
    assert!(
        state.settlements.values().any(|s| s
            .economy
            .facilities
            .iter()
            .any(|f| f.kind == FacilityKind::Shipyard)),
        "no shipyards"
    );
    for _ in 0..360 * 2 {
        tick(&mut state);
    }
    let end = economy::summarize(&state).ships;
    assert!(end > start, "fleet didn't grow: {start} -> {end}");
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
