use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::economy::{self, Good};
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
    // Consumer settlements that run out of money, and planned settlements
    // whose fixed prices give captains no reason to deliver, go short until
    // contracts and faction spending land (#33, step 3). Until then, only
    // guard against collapse.
    let summary = economy::summarize(&state);
    let unmet = summary.shortfall[Good::Food] / summary.demand[Good::Food];
    assert!(unmet < 0.6, "{:.0}% of food demand unmet", unmet * 100.0);
}

#[test]
fn trade_conserves_money() {
    let mut state = small();
    for _ in 0..30 {
        tick(&mut state);
    }
    let before = economy::summarize(&state).money;
    for _ in 0..30 {
        tick(&mut state);
    }
    let after = economy::summarize(&state).money;
    assert!(
        (after - before).abs() <= 1e-9 * before,
        "{before} -> {after}"
    );
}
