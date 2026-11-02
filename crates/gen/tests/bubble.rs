//! The bubble (DESIGN §5.2): traders near the player fly for real, and
//! fold back into coarse trips when it moves or goes away, losing nothing.

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::bubble::{self, Stage};
use retrox_sim::economy::Good;
use retrox_sim::haul::{self, EventKind, Location};
use retrox_sim::{GalaxyState, SystemId, ledger, tick};

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

/// The busiest system: the most-populous faction capital's, where the
/// client starts the player.
fn busiest(state: &GalaxyState) -> SystemId {
    let capital = state
        .factions
        .values()
        .filter_map(|f| state.settlements.get(f.capital))
        .max_by(|a, b| a.economy.population.total_cmp(&b.economy.population))
        .expect("a capital");
    capital.system
}

fn warm(days: u32) -> GalaxyState {
    let mut state = small();
    for _ in 0..days {
        tick(&mut state);
    }
    state
}

/// Every fleet's location agrees with the bubble's flights, and every
/// flight is inside the bubble.
fn consistent(state: &GalaxyState) {
    let mut flying = 0;
    for (key, company) in &state.companies {
        for fleet in &company.fleets {
            let flight = state.bubble.flights.get(&(key, fleet.id));
            match fleet.at {
                Location::Flying { to } => {
                    let flight = flight.expect("a flying fleet has a flight");
                    assert_eq!(flight.to, to);
                    assert!(bubble::contains(state, flight.stage.system()));
                    flying += 1;
                }
                _ => assert!(flight.is_none(), "{} has a stray flight", company.name),
            }
        }
    }
    assert_eq!(flying, state.bubble.flights.len());
}

/// Every coarse trip has an arrival event on its day.
fn coarse_trips_scheduled(state: &GalaxyState) {
    for (key, company) in &state.companies {
        for fleet in &company.fleets {
            if let Location::Travelling { arrive, .. } = fleet.at {
                assert!(
                    arrive >= state.time,
                    "{}: arrival in the past",
                    company.name
                );
                let scheduled = state.calendar.events().any(|e| {
                    e.company == key && e.fleet == fleet.id && e.kind == EventKind::Arrive
                });
                assert!(
                    scheduled,
                    "{} fleet {} has no arrival",
                    company.name, fleet.id
                );
            }
        }
    }
}

fn totals(state: &GalaxyState) -> (f64, Vec<f64>, u32) {
    let goods = haul::total_goods(state);
    let ships = state
        .companies
        .values()
        .flat_map(|c| &c.fleets)
        .map(|f| f.ships)
        .sum();
    (
        ledger::total_money(state),
        Good::ALL.into_iter().map(|g| goods[g]).collect(),
        ships,
    )
}

#[test]
fn materializing_and_collapsing_conserve_everything() {
    let mut state = warm(60);
    let centre = busiest(&state);
    let before = totals(&state);
    bubble::set_centre(&mut state, Some(centre));
    assert!(
        !state.bubble.flights.is_empty(),
        "nothing materialized around the busiest system"
    );
    consistent(&state);
    assert_eq!(totals(&state), before);
    bubble::set_centre(&mut state, None);
    assert!(state.bubble.flights.is_empty());
    consistent(&state);
    coarse_trips_scheduled(&state);
    assert_eq!(totals(&state), before);
}

#[test]
fn ships_fly_trade_and_leave_the_bubble() {
    let mut state = warm(60);
    let centre = busiest(&state);
    bubble::set_centre(&mut state, Some(centre));
    let docked_before = |s: &GalaxyState| -> Vec<_> {
        s.companies
            .iter()
            .flat_map(|(k, c)| c.fleets.iter().map(move |f| ((k, f.id), f.at)))
            .collect()
    };
    let mut docked = 0;
    let mut jumped = 0;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 {
        let flights: Vec<_> = state.bubble.flights.keys().copied().collect();
        seen.extend(flights.iter().copied());
        let before = docked_before(&state);
        tick(&mut state);
        consistent(&state);
        for (key, at) in before {
            if flights.contains(&key) {
                match state.bubble.flights.get(&key) {
                    None if matches!(at, Location::Flying { .. }) => {
                        if let Some(c) = state.companies.get(key.0)
                            && let Some(f) = c.fleets.iter().find(|f| f.id == key.1)
                        {
                            match f.at {
                                Location::Travelling { .. } => jumped += 1,
                                _ => docked += 1,
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for flight in state.bubble.flights.values() {
            if let Stage::Flying { system, .. } = flight.stage {
                assert!(bubble::contains(&state, system));
            }
        }
    }
    assert!(
        seen.len() > 5,
        "only {} fleets flew in the bubble",
        seen.len()
    );
    assert!(docked > 0, "no flight reached port in 60 days");
    assert!(jumped > 0, "no flight left the bubble in 60 days");
    coarse_trips_scheduled(&state);
}

#[test]
fn moving_the_bubble_keeps_the_books() {
    let mut state = warm(40);
    let centre = busiest(&state);
    bubble::set_centre(&mut state, Some(centre));
    let far = state
        .systems
        .iter()
        .map(|(id, _)| id)
        .find(|&s| !state.bubble.systems.contains(&s) && !state.systems[s].settlements.is_empty())
        .expect("a settled system outside the bubble");
    for day in 0..120 {
        if day % 10 == 0 {
            let to = if day % 20 == 0 { far } else { centre };
            let before = totals(&state);
            bubble::set_centre(&mut state, Some(to));
            assert_eq!(totals(&state), before);
            consistent(&state);
        }
        tick(&mut state);
    }
    bubble::set_centre(&mut state, None);
    consistent(&state);
    coarse_trips_scheduled(&state);
}

#[test]
fn the_bubble_is_deterministic_and_saves() {
    let run = |save_at: Option<u32>| {
        let mut state = warm(30);
        let centre = busiest(&state);
        bubble::set_centre(&mut state, Some(centre));
        for day in 0..60 {
            if Some(day) == save_at {
                let saved = serde_json::to_string(&state).unwrap();
                state = serde_json::from_str(&saved).unwrap();
            }
            tick(&mut state);
        }
        serde_json::to_string(&state).unwrap()
    };
    let plain = run(None);
    assert_eq!(plain, run(None));
    assert_eq!(plain, run(Some(25)));
}

/// Coarse trip times are what flying the trip for real takes: on average,
/// across in-system hops and trips through the lanes.
#[test]
fn coarse_trip_times_match_real_flights() {
    let mut state = warm(60);
    // The bubble visits each faction's capital in turn.
    let mut capitals: Vec<_> = state
        .factions
        .values()
        .filter_map(|f| state.settlements.get(f.capital))
        .map(|s| (s.economy.population, s.system))
        .collect();
    capitals.sort_by(|a, b| b.0.total_cmp(&a.0));
    let capitals: Vec<SystemId> = capitals.into_iter().map(|c| c.1).take(6).collect();
    // Flights launched from a port in the bubble: their estimate, and the
    // day they set out.
    let mut watching: std::collections::BTreeMap<_, (u64, f64, bool)> = Default::default();
    let (mut local, mut through) = ((0.0, 0.0, 0), (0.0, 0.0, 0));
    for day in 0..360 {
        if day % 60 == 0 {
            let centre = capitals[day / 60 % capitals.len()];
            bubble::set_centre(&mut state, Some(centre));
        }
        let today = state.time.days();
        tick(&mut state);
        for (&key, flight) in &state.bubble.flights {
            if flight.depart.days() == today && !watching.contains_key(&key) {
                let class = state.companies[key.0]
                    .fleets
                    .iter()
                    .find(|f| f.id == key.1)
                    .expect("fleet")
                    .class;
                let Some(estimate) = haul::trip_days(&state, class, flight.from, flight.to) else {
                    continue;
                };
                let lanes = !flight.path.is_empty();
                watching.insert(key, (today, estimate, lanes));
            }
        }
        let done: Vec<_> = watching
            .iter()
            .filter_map(|(&key, &(day, estimate, lanes))| {
                let fleet = state
                    .companies
                    .get(key.0)?
                    .fleets
                    .iter()
                    .find(|f| f.id == key.1)?;
                let flight = state.bubble.flights.get(&key);
                let arrived = match (fleet.at, flight) {
                    // Docked and sent on again: a new trip.
                    (Location::Flying { .. }, Some(f)) => f.depart.days() > day,
                    (Location::Docked(_), _) => true,
                    _ => return Some((key, None)),
                };
                arrived.then(|| (key, Some((state.time.days() - day, estimate, lanes))))
            })
            .collect();
        for (key, trip) in done {
            watching.remove(&key);
            if let Some((days, estimate, lanes)) = trip {
                let sum = if lanes { &mut through } else { &mut local };
                sum.0 += days as f64;
                sum.1 += estimate;
                sum.2 += 1;
            }
        }
    }
    // Before #72, trips through the lanes took about 1.4 times their
    // estimate. In-system trips are rare here; check them when there are
    // enough.
    assert!(
        through.2 >= 20,
        "only {} trips through the lanes",
        through.2
    );
    for (what, (actual, estimate, n)) in [("in-system", local), ("through lanes", through)] {
        let ratio = actual / estimate;
        eprintln!("{what}: {n} trips, flown/estimated {ratio:.2}");
        if n < 5 {
            continue;
        }
        assert!(
            (0.8..1.25).contains(&ratio),
            "{what} trips take {ratio:.2} times their estimate"
        );
    }
}
