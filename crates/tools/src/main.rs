//! `retrox` — headless tools for running and inspecting the simulation.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{GalaxyTime, PIPELINE, tick_profiled};

mod bench;
mod health;
mod map;
mod ship;

const USAGE: &str = "\
usage:
  retrox simulate [--seed N] [--years N] [--csv PATH]
                                           run the sim headless, print timings;
                                           --csv writes monthly economy totals
  retrox map [--seed N] > galaxy.svg       render the generated galaxy as SVG
  retrox stats [--seed N]                  print generation statistics
  retrox prehistory [--seed N] [--years N] grow the galaxy from the cradle,
                                           printing it every 10 years
  retrox bench [--seed N] [--years N]      time the sim per actor, and N years
                                           of prehistory
  retrox ship [HULL]                       list stock ships, or show one in detail";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if command == "ship" {
        return ship::print(args.get(1).map(String::as_str));
    }
    let Flags { seed, years, csv } = match parse_flags(&args[1..]) {
        Ok(flags) => flags,
        Err(err) => {
            eprintln!("{err}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match command.as_str() {
        "simulate" => {
            if let Err(err) = simulate(seed, years, csv.as_deref()) {
                eprintln!("{err}");
                return ExitCode::FAILURE;
            }
        }
        "map" => {
            let state = generate(seed, &GalaxyConfig::default());
            print!("{}", map::svg(&state));
        }
        "stats" => stats(seed),
        "prehistory" => prehistory(seed, years),
        "bench" => bench::run_bench(seed, years),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

struct Flags {
    seed: u64,
    years: u64,
    csv: Option<String>,
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut flags = Flags {
        seed: 0,
        years: 10,
        csv: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let number = || {
            value
                .parse::<u64>()
                .map_err(|_| format!("{flag} expects a number, got {value:?}"))
        };
        match flag.as_str() {
            "--seed" => flags.seed = number()?,
            "--years" => flags.years = number()?,
            "--csv" => flags.csv = Some(value.clone()),
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    Ok(flags)
}

fn stats(seed: u64) {
    let start = Instant::now();
    let state = generate(seed, &GalaxyConfig::default());
    let elapsed = start.elapsed();

    let lanes = state.lanes.as_slice();
    let mut lengths: Vec<f64> = lanes.iter().map(|l| l.length_ly).collect();
    lengths.sort_by(f64::total_cmp);
    let pct = |p: f64| lengths[((lengths.len() - 1) as f64 * p) as usize];

    let mut degrees = [0usize; 8];
    for (_, s) in state.systems.iter() {
        degrees[s.lanes.len().min(7)] += 1;
    }

    println!("seed {seed}: generated in {elapsed:.2?}");
    println!(
        "  {} systems, {} lanes, mean degree {:.2}",
        state.systems.len(),
        lanes.len(),
        2.0 * lanes.len() as f64 / state.systems.len() as f64
    );
    println!(
        "  lane length ly: p10 {:.1}  median {:.1}  p90 {:.1}  max {:.1}",
        pct(0.1),
        pct(0.5),
        pct(0.9),
        pct(1.0)
    );
    let degree_line: Vec<String> = degrees
        .iter()
        .enumerate()
        .map(|(d, n)| format!("{d}{}:{n}", if d == 7 { "+" } else { "" }))
        .collect();
    println!("  degree histogram: {}", degree_line.join("  "));
    let sample: Vec<&str> = state
        .systems
        .as_slice()
        .iter()
        .take(12)
        .map(|s| s.name.as_str())
        .collect();
    println!("  names: {}", sample.join(", "));
    print_bodies(&state);
    println!("  factions:");
    let mut independents = 0;
    for (id, faction) in &state.factions {
        let settlements = holdings(&state, id);
        if settlements < 2 {
            independents += 1;
            continue;
        }
        let population: f64 = state
            .settlements
            .values()
            .filter(|s| s.owner == id)
            .map(|s| s.economy.population)
            .sum();
        println!(
            "    {:<32} {:?}  {settlements} settlements, population {population:.0}",
            faction.name, faction.economy
        );
    }
    println!("    + {independents} independent one-settlement factions");
}

fn print_bodies(state: &retrox_sim::GalaxyState) {
    use retrox_sim::BodyKind;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for body in state.bodies.as_slice() {
        let kind = match body.kind {
            BodyKind::Planet(class) => format!("{class:?} planet"),
            other => format!("{other:?}"),
        };
        *kinds.entry(kind).or_default() += 1;
    }
    let kinds: Vec<String> = kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
    println!("  {} bodies: {}", state.bodies.len(), kinds.join(", "));
    let divided = state
        .bodies
        .as_slice()
        .iter()
        .filter(|b| !b.regions.is_empty())
        .count();
    let systems_with_world = state
        .systems
        .as_slice()
        .iter()
        .filter(|s| {
            s.bodies
                .iter()
                .any(|&b| !state.bodies[b].regions.is_empty())
        })
        .count();
    println!(
        "  {} habitable worlds divided into {} regions, in {} systems ({:.0}%); {} settlements",
        divided,
        state.regions.len(),
        systems_with_world,
        100.0 * systems_with_world as f64 / state.systems.len() as f64,
        state.settlements.len()
    );
}

/// How many settlements a faction holds.
pub(crate) fn holdings(state: &retrox_sim::GalaxyState, faction: retrox_sim::FactionId) -> usize {
    state
        .settlements
        .values()
        .filter(|s| s.owner == faction)
        .count()
}

fn simulate(seed: u64, years: u64, csv: Option<&str>) -> std::io::Result<()> {
    use retrox_sim::economy::{Good, summarize};
    use std::io::Write;

    let mut csv = match csv {
        Some(path) => {
            let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
            writeln!(
                file,
                "day,good,stock,demand,shortfall,mean_price,markets_short"
            )?;
            Some(file)
        }
        None => None,
    };
    let mut state = generate(seed, &GalaxyConfig::default());
    let end = GalaxyTime::from_years(years);
    let mut per_phase: BTreeMap<&'static str, Duration> = BTreeMap::new();

    let start = Instant::now();
    while state.time < end {
        tick_profiled(&mut state, |name, elapsed| {
            *per_phase.entry(name).or_default() += elapsed;
        });
        if let Some(csv) = csv.as_mut()
            && state.time.days().is_multiple_of(30)
        {
            let e = summarize(&state);
            for good in Good::ALL {
                writeln!(
                    csv,
                    "{},{},{:.1},{:.1},{:.1},{:.3},{}",
                    state.time.days(),
                    good.name(),
                    e.stock[good],
                    e.demand[good],
                    e.shortfall[good],
                    e.mean_price[good],
                    e.markets_short[good]
                )?;
            }
        }
    }
    if let Some(mut csv) = csv {
        csv.flush()?;
    }
    let total = start.elapsed();

    let days = end.days().max(1) as u32;
    println!(
        "simulated {years} years ({} days) of seed {seed} in {total:.2?} ({:.2?}/day)",
        end.days(),
        total / days
    );
    for phase in PIPELINE {
        println!(
            "  {:<12} {:.2?}",
            phase.name,
            per_phase.get(phase.name).copied().unwrap_or_default()
        );
    }
    println!("history: {} events", state.history.events().len());
    print_economy(&state);
    health::print(&state);
    Ok(())
}

fn print_economy(state: &retrox_sim::GalaxyState) {
    use retrox_sim::economy::{Good, summarize};
    let e = summarize(state);
    println!(
        "economy: population {:.0}, money {:.0} credits, faction balances {:.0}",
        e.population, e.money, e.faction_balance
    );
    println!(
        "  hauling: {} companies, {} ships ({} travelling)",
        e.companies, e.ships, e.ships_travelling
    );
    println!(
        "  {} settlements, {} colony ships under way",
        state.settlements.len(),
        state.colony_ships.len()
    );
    println!(
        "  {:<11} {:>12} {:>10} {:>10} {:>9} {:>8}",
        "good", "stock", "demand/d", "short/d", "price", "#short"
    );
    for good in Good::ALL {
        println!(
            "  {:<11} {:>12.0} {:>10.0} {:>10.1} {:>9.2} {:>8}",
            good.name(),
            e.stock[good],
            e.demand[good],
            e.shortfall[good],
            e.mean_price[good],
            e.markets_short[good]
        );
    }
}

fn prehistory(seed: u64, years: u64) {
    use std::collections::HashSet;

    let start = Instant::now();
    let mut state = retrox_gen::generate_cradle(seed, &GalaxyConfig::default());
    let Some(first) = state.settlements.values().next() else {
        println!("seed {seed}: no world can hold a cradle");
        return;
    };
    println!(
        "seed {seed}: cradle in {}, {} nations",
        state.systems[first.system].name,
        state.factions.len()
    );
    println!(
        "  {:>5} {:>9} {:>11} {:>8} {:>8} {:>10} {:>9}",
        "year", "factions", "settlements", "systems", "colonies", "population", "ships"
    );
    let end = GalaxyTime::from_years(years);
    loop {
        if state.time.day_of_year() == 0 && state.time.year().is_multiple_of(10) {
            let systems: HashSet<_> = state.settlements.values().map(|s| s.system).collect();
            let e = retrox_sim::economy::summarize(&state);
            println!(
                "  {:>5} {:>9} {:>11} {:>8} {:>8} {:>10.0} {:>9}",
                state.time.year(),
                state.factions.len(),
                state.settlements.len(),
                systems.len(),
                state.colony_ships.len(),
                e.population,
                e.ships
            );
        }
        if state.time >= end {
            break;
        }
        retrox_sim::tick(&mut state);
    }
    println!("ran in {:.1?}", start.elapsed());
}
