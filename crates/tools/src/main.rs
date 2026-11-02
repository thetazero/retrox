//! `retrox` — headless tools for running and inspecting the simulation.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use retrox_sim::{GalaxyState, GalaxyTime, PIPELINE, tick_profiled};

const USAGE: &str = "usage: retrox simulate [--seed N] [--years N]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("simulate") => match parse_simulate(&args[1..]) {
            Ok((seed, years)) => {
                simulate(seed, years);
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{err}\n{USAGE}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn parse_simulate(args: &[String]) -> Result<(u64, u64), String> {
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

fn simulate(seed: u64, years: u64) {
    let mut state = GalaxyState::empty(seed);
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
