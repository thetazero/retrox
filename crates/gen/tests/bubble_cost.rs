//! What a busy bubble's day costs (issue #115), counted in autopilot steps
//! so the guard doesn't depend on the machine. Its own test binary, with
//! one test, because the step counter is shared by the whole process.

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{GalaxyState, bubble, flight, tick};

fn small() -> GalaxyState {
    generate(
        9,
        &GalaxyConfig {
            systems: 600,
            radius_ly: 100.0,
            factions: 5,
            ..GalaxyConfig::default()
        },
    )
}

/// A flight flies a few hundred steps a day: fine ones near its goal,
/// half-hour ones in cruise. A ship that can't settle on its goal steps at
/// the smallest step all day, over a million; harvesters closing on a gas
/// giant did, and a busy bubble's day took most of a second.
#[test]
fn a_busy_bubble_day_flies_few_steps() {
    let mut state = small();
    for _ in 0..60 {
        tick(&mut state);
    }
    // The busiest system: the most-populous capital's.
    let centre = state
        .factions
        .values()
        .filter_map(|f| state.settlements.get(f.capital))
        .max_by(|a, b| a.economy.population.total_cmp(&b.economy.population))
        .expect("a capital")
        .system;
    bubble::set_centre(&mut state, Some(centre));
    let mut worst = 0.0_f64;
    let mut flight_days = 0;
    let before = flight::steps_flown();
    for _ in 0..60 {
        let flights = state.bubble.flights.len().max(1);
        let start = flight::steps_flown();
        tick(&mut state);
        let per_flight = (flight::steps_flown() - start) as f64 / flights as f64;
        worst = worst.max(per_flight);
        flight_days += flights;
    }
    let mean = (flight::steps_flown() - before) as f64 / flight_days as f64;
    eprintln!("autopilot steps per flight-day: mean {mean:.0}, worst day {worst:.0}");
    assert!(
        flight_days > 300,
        "only {flight_days} flight-days: not busy"
    );
    assert!(mean < 1_000.0, "{mean:.0} steps per flight-day");
    assert!(worst < 3_000.0, "a day of {worst:.0} steps per flight");
}
