//! `retrox bench`: how much the sim costs, per actor and as a whole
//! (DESIGN §5.1.1, issue #55).
//!
//! Two parts:
//!
//! - **Actors:** the static galaxy for a year, after a month to settle.
//!   Each phase's time is divided by what it works on, so the numbers
//!   compare directly with the per-actor aims: settlement phases per
//!   settlement-day, hauling phases per fleet-day.
//! - **Bubble:** the same galaxy with the bubble on its busiest system for
//!   60 days: what a day costs while ships there fly for real, in ms of
//!   wall time and autopilot steps per flight-day (issue #115).
//! - **Prehistory:** N years from the cradle, in seconds per simulated year
//!   (overall, and every 25 years as the galaxy grows), with per-phase
//!   totals.
//!
//! Results print as a table, then as one `BENCH {...}` JSON line for
//! tracking over time.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use retrox_gen::{GalaxyConfig, generate, generate_cradle};
use retrox_sim::decisions::{self, Decision};
use retrox_sim::{GalaxyState, PIPELINE, tick_profiled};

/// Phases whose work scales with settlements, and with fleets.
const SETTLEMENT_PHASES: &[&str] = &["production", "planning", "prices"];
/// A settlement's daily books (DESIGN §5.1.1, aim: tens of ns); the rest
/// of its phases are monthly reviews, spread over the days.
const BOOKS_PHASES: &[&str] = &["production", "prices"];
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

/// Each kind of decision: how many, ns each, and the whole of it.
fn print_decisions(totals: &[(Duration, u64)]) {
    for (d, &(took, n)) in Decision::ALL.iter().zip(totals) {
        println!(
            "    {:<13} {:>9} made, {:>7.0} ns each, {:>7.1} ms in all",
            d.name(),
            n,
            ns(took, n as f64),
            took.as_secs_f64() * 1000.0
        );
    }
}

fn ns(d: Duration, per: f64) -> f64 {
    d.as_nanos() as f64 / per.max(1.0)
}

struct Bubble {
    flights: f64,
    ms_per_day: f64,
    steps_per_flight_day: f64,
}

/// Centre the bubble on the busiest system (the most-populous capital's,
/// where the player starts) and time 60 days of it.
fn bubble(state: &mut GalaxyState) -> Bubble {
    let centre = state
        .factions
        .values()
        .filter_map(|f| state.settlements.get(f.capital))
        .max_by(|a, b| a.economy.population.total_cmp(&b.economy.population))
        .map(|s| s.system);
    retrox_sim::bubble::set_centre(state, centre);
    let days = 60;
    let mut flight_days = 0;
    let steps = retrox_sim::flight::steps_flown();
    let start = Instant::now();
    for _ in 0..days {
        flight_days += state.bubble.flights.len();
        retrox_sim::tick(state);
    }
    let wall = start.elapsed();
    let steps = retrox_sim::flight::steps_flown() - steps;
    retrox_sim::bubble::set_centre(state, None);
    Bubble {
        flights: flight_days as f64 / days as f64,
        ms_per_day: wall.as_secs_f64() * 1000.0 / days as f64,
        steps_per_flight_day: steps as f64 / flight_days.max(1) as f64,
    }
}

pub fn run_bench(seed: u64, years: u64) {
    // Actors, on the static galaxy.
    let mut state = generate(seed, &GalaxyConfig::default());
    run(&mut state, 30, |_, _| {});
    decisions::set_timing(true);
    decisions::take();
    let actors = run(&mut state, retrox_sim::time::DAYS_PER_YEAR, |_, _| {});
    let actor_decisions = decisions::take();
    decisions::set_timing(false);
    let per_settlement = ns(sum(&actors, SETTLEMENT_PHASES), actors.settlement_days);
    let books = ns(sum(&actors, BOOKS_PHASES), actors.settlement_days);
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
    println!("  settlement books (production + prices): {books:.0} ns per settlement-day");
    println!("  decisions (aim ~100 ns each):");
    print_decisions(&actor_decisions);

    // The bubble, on the same galaxy.
    let bubble = bubble(&mut state);
    println!(
        "bubble (busiest system, 60 days): {:.0} flights on average; {:.2} ms per day, {:.0} autopilot steps per flight-day",
        bubble.flights, bubble.ms_per_day, bubble.steps_per_flight_day
    );

    // Prehistory, from the cradle.
    let mut state = generate_cradle(seed, &GalaxyConfig::default());
    decisions::set_timing(true);
    decisions::take();
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
    let history_decisions = decisions::take();
    decisions::set_timing(false);
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
    println!("  decisions:");
    print_decisions(&history_decisions);

    let phases: Vec<String> = PIPELINE
        .iter()
        .map(|p| {
            let d = history.phases.get(p.name).copied().unwrap_or_default();
            format!("\"{}\":{:.3}", p.name, d.as_secs_f64())
        })
        .collect();
    let decision_ns: Vec<String> = Decision::ALL
        .iter()
        .zip(actor_decisions)
        .map(|(d, (took, n))| format!("\"{}\":{:.0}", d.name(), ns(took, n as f64)))
        .collect();
    println!(
        "BENCH {{\"seed\":{seed},\"ns_per_decision\":{{{}}},\"ns_per_settlement_day\":{per_settlement:.0},\"ns_books_per_settlement_day\":{books:.0},\"ns_per_fleet_day\":{per_fleet:.0},\"static_ms_per_day\":{per_day_ms:.3},\"bubble_ms_per_day\":{:.3},\"bubble_steps_per_flight_day\":{:.0},\"prehistory_years\":{years},\"prehistory_seconds\":{seconds:.2},\"prehistory_settlements\":{},\"prehistory_phases_s\":{{{}}}}}",
        decision_ns.join(","),
        bubble.ms_per_day,
        bubble.steps_per_flight_day,
        state.settlements.len(),
        phases.join(",")
    );
}
