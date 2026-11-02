//! `retrox bench`: how much the sim costs, per actor and as a whole
//! (DESIGN §5.1.1, issue #55).
//!
//! Two parts:
//!
//! - **Actors:** the static galaxy for a year, after a month to settle.
//!   Each phase's time is divided by what it works on, so the numbers
//!   compare directly with the per-actor aims: settlement phases per
//!   settlement-day, hauling phases per fleet-day.
//! - **Prehistory:** N years from the cradle, in seconds per simulated year
//!   (overall, and every 25 years as the galaxy grows), with per-phase
//!   totals.
//!
//! Results print as a table, then as one `BENCH {...}` JSON line for
//! tracking over time.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use retrox_gen::{GalaxyConfig, generate, generate_cradle};
use retrox_sim::{GalaxyState, PIPELINE, tick_profiled};

/// Phases whose work scales with settlements, and with fleets.
const SETTLEMENT_PHASES: &[&str] = &["production", "planning"];
const FLEET_PHASES: &[&str] = &["trade", "movement"];

struct Totals {
    phases: BTreeMap<&'static str, Duration>,
    settlement_days: f64,
    fleet_days: f64,
    days: u64,
    wall: Duration,
}

fn fleets(state: &GalaxyState) -> usize {
    state
        .companies
        .values()
        .map(|c| c.fleets.len())
        .sum::<usize>()
        + state.colony_ships.len()
}

/// Run `days` ticks, timing every phase and counting actor-days.
fn run(
    state: &mut GalaxyState,
    days: u64,
    mut every_year: impl FnMut(&GalaxyState, Duration),
) -> Totals {
    let mut totals = Totals {
        phases: BTreeMap::new(),
        settlement_days: 0.0,
        fleet_days: 0.0,
        days,
        wall: Duration::ZERO,
    };
    let start = Instant::now();
    let mut year_start = Instant::now();
    for day in 0..days {
        totals.settlement_days += state.settlements.len() as f64;
        totals.fleet_days += fleets(state) as f64;
        tick_profiled(state, |name, elapsed| {
            *totals.phases.entry(name).or_default() += elapsed;
        });
        if (day + 1) % retrox_sim::time::DAYS_PER_YEAR == 0 {
            every_year(state, year_start.elapsed());
            year_start = Instant::now();
        }
    }
    totals.wall = start.elapsed();
    totals
}

fn sum(totals: &Totals, names: &[&str]) -> Duration {
    names
        .iter()
        .map(|n| totals.phases.get(n).copied().unwrap_or_default())
        .sum()
}

fn ns(d: Duration, per: f64) -> f64 {
    d.as_nanos() as f64 / per.max(1.0)
}

pub fn run_bench(seed: u64, years: u64) {
    // Actors, on the static galaxy.
    let mut state = generate(seed, &GalaxyConfig::default());
    run(&mut state, 30, |_, _| {});
    let actors = run(&mut state, retrox_sim::time::DAYS_PER_YEAR, |_, _| {});
    let per_settlement = ns(sum(&actors, SETTLEMENT_PHASES), actors.settlement_days);
    let per_fleet = ns(sum(&actors, FLEET_PHASES), actors.fleet_days);
    let per_day_ms = actors.wall.as_secs_f64() * 1000.0 / actors.days as f64;
    println!("actors (static galaxy, seed {seed}, 1 year):");
    println!(
        "  {:.0} settlements, {:.0} fleets on average; {per_day_ms:.2} ms per day",
        actors.settlement_days / actors.days as f64,
        actors.fleet_days / actors.days as f64
    );
    for phase in PIPELINE {
        let d = actors.phases.get(phase.name).copied().unwrap_or_default();
        let (per, unit) = if SETTLEMENT_PHASES.contains(&phase.name) {
            (ns(d, actors.settlement_days), "settlement-day")
        } else if FLEET_PHASES.contains(&phase.name) {
            (ns(d, actors.fleet_days), "fleet-day")
        } else {
            (ns(d, actors.days as f64), "day")
        };
        println!("  {:<12} {:>10.0} ns per {unit}", phase.name, per);
    }

    // Prehistory, from the cradle.
    let mut state = generate_cradle(seed, &GalaxyConfig::default());
    let history = run(
        &mut state,
        years * retrox_sim::time::DAYS_PER_YEAR,
        |state, took| {
            let year = state.time.year() + 1;
            if year % 25 == 0 {
                println!(
                    "  year {year:>4}: {:>6} settlements, {:>6} fleets, {:.2} s/year",
                    state.settlements.len(),
                    fleets(state),
                    took.as_secs_f64()
                );
            }
        },
    );
    let seconds = history.wall.as_secs_f64();
    println!(
        "prehistory (seed {seed}, {years} years): {seconds:.1} s, {:.2} s per simulated year",
        seconds / years.max(1) as f64
    );
    for phase in PIPELINE {
        let d = history.phases.get(phase.name).copied().unwrap_or_default();
        println!("  {:<12} {:>8.2} s", phase.name, d.as_secs_f64());
    }
    println!(
        "  ended with {} settlements, {} fleets",
        state.settlements.len(),
        fleets(&state)
    );

    let phases: Vec<String> = PIPELINE
        .iter()
        .map(|p| {
            let d = history.phases.get(p.name).copied().unwrap_or_default();
            format!("\"{}\":{:.3}", p.name, d.as_secs_f64())
        })
        .collect();
    println!(
        "BENCH {{\"seed\":{seed},\"ns_per_settlement_day\":{per_settlement:.0},\"ns_per_fleet_day\":{per_fleet:.0},\"static_ms_per_day\":{per_day_ms:.3},\"prehistory_years\":{years},\"prehistory_seconds\":{seconds:.2},\"prehistory_settlements\":{},\"prehistory_phases_s\":{{{}}}}}",
        state.settlements.len(),
        phases.join(",")
    );
}
