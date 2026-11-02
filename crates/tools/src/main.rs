//! `retrox` — headless tools for running and inspecting the simulation.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::{GalaxyTime, PIPELINE, tick_profiled};

mod map;
mod ship;

const USAGE: &str = "\
usage:
  retrox simulate [--seed N] [--years N]   run the sim headless, print timings
  retrox map [--seed N] > galaxy.svg       render the generated galaxy as SVG
  retrox stats [--seed N]                  print generation statistics
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
    let (seed, years) = match parse_flags(&args[1..]) {
        Ok(flags) => flags,
        Err(err) => {
            eprintln!("{err}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match command.as_str() {
        "simulate" => simulate(seed, years),
        "map" => {
            let state = generate(seed, &GalaxyConfig::default());
            print!("{}", map::svg(&state));
        }
        "stats" => stats(seed),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn parse_flags(args: &[String]) -> Result<(u64, u64), String> {
    let (mut seed, mut years) = (0, 10);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let value: u64 = value
            .parse()
            .map_err(|_| format!("{flag} expects a number, got {value:?}"))?;
        match flag.as_str() {
            "--seed" => seed = value,
            "--years" => years = value,
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    Ok((seed, years))
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
    println!("  factions:");
    for faction in state.factions.values() {
        let systems = state
            .systems
            .as_slice()
            .iter()
            .filter(|s| {
                s.owner
                    .is_some_and(|o| state.factions[o].capital == faction.capital)
            })
            .count();
        println!(
            "    {:<32} {:?}  {systems} systems",
            faction.name, faction.economy
        );
    }
}

fn simulate(seed: u64, years: u64) {
    let mut state = generate(seed, &GalaxyConfig::default());
    let end = GalaxyTime::from_years(years);
    let mut per_phase: BTreeMap<&'static str, Duration> = BTreeMap::new();

    let start = Instant::now();
    while state.time < end {
        tick_profiled(&mut state, |name, elapsed| {
            *per_phase.entry(name).or_default() += elapsed;
        });
    }
    let total = start.elapsed();

    let days = end.days().max(1) as u32;
    println!(
        "simulated {years} years ({} days) of seed {seed} in {total:.2?} ({:.2?}/day)",
        end.days(),
        total / days
    );
    for phase in PIPELINE {
        println!("  {:<12} {:.2?}", phase.name, per_phase[phase.name]);
    }
    println!("history: {} events", state.history.events().len());
}
