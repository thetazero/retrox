//! The dispatch board's rules (DESIGN §7.5.1), on a generated galaxy.

use std::collections::HashMap;

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::freight::{self, BOARD_SIZE, MAX_JUMPS, Pending};
use retrox_sim::{GalaxyState, GalaxyTime, Site, tick};

fn galaxy() -> GalaxyState {
    let mut state = generate(
        5,
        &GalaxyConfig {
            systems: 800,
            radius_ly: 110.0,
            factions: 6,
            ..GalaxyConfig::default()
        },
    );
    // Markets drift off their generated targets, as in the client.
    for _ in 0..30 {
        tick(&mut state);
    }
    state
}

fn lane_days(ly: f64) -> f64 {
    (ly * 0.6).clamp(2.0, 14.0).round()
}

const LOT: f64 = 40.0;

#[test]
fn boards_follow_the_rules() {
    let state = galaxy();
    let mut offers = 0;
    for (here, s) in state.settlements.iter() {
        if matches!(s.site, Site::Surface(_)) {
            continue;
        }
        let board = freight::board(&state, here, LOT, lane_days, &[]);
        assert!(board.len() <= BOARD_SIZE);
        // The same state draws the same board.
        assert_eq!(board, freight::board(&state, here, LOT, lane_days, &[]));
        let mut per_destination: HashMap<_, usize> = HashMap::new();
        for o in &board {
            *per_destination.entry(o.to).or_default() += 1;
            assert_eq!(o.from, here);
            assert_ne!(o.to, here);
            assert!(o.jumps <= MAX_JUMPS);
            assert!(o.amount >= 1.0 && o.amount <= LOT, "{}", o.amount);
            assert!(o.shortage >= 0.25, "consignee only {} short", o.shortage);
            assert!(o.deadline > state.time);
            assert!(o.fee > 0.0 && o.value > 0.0);
            assert!(!matches!(state.settlements[o.to].site, Site::Surface(_)));
        }
        assert!(per_destination.values().all(|&n| n <= 2));
        // Best pay per day first, to the cent; the shorter run first
        // among equals.
        let rate = |o: &freight::Offer| (o.fee / o.trip_days.max(1.0) * 100.0).round();
        assert!(board.windows(2).all(|w| rate(&w[0]) > rate(&w[1])
            || (rate(&w[0]) == rate(&w[1]) && w[0].jumps <= w[1].jumps)));
        offers += board.len();
    }
    assert!(offers > 0, "some board somewhere has work");
}

#[test]
fn what_the_player_carries_is_not_asked_for_twice() {
    let state = galaxy();
    let (here, board) = state
        .settlements
        .keys()
        .map(|s| (s, freight::board(&state, s, LOT, lane_days, &[])))
        .find(|(_, b)| !b.is_empty())
        .expect("some board has work");
    let first = &board[0];
    // Carrying a big enough lot there already covers the shortage.
    let pending = [Pending {
        to: first.to,
        good: first.good,
        amount: 1e9,
    }];
    let again = freight::board(&state, here, LOT, lane_days, &pending);
    assert!(
        !again
            .iter()
            .any(|o| o.to == first.to && o.good == first.good)
    );
}

#[test]
fn late_delivery_pays_less_and_then_fails() {
    let deadline = GalaxyTime::from_days(100);
    let fee = 1000.0;
    assert_eq!(
        freight::pay_on(fee, deadline, GalaxyTime::from_days(90)),
        fee
    );
    assert_eq!(freight::pay_on(fee, deadline, deadline), fee);
    let late = freight::pay_on(fee, deadline, GalaxyTime::from_days(103));
    assert!((late - fee * (1.0 - 3.0 * freight::LATE_PENALTY_PER_DAY)).abs() < 1e-9);
    assert_eq!(
        freight::pay_on(fee, deadline, GalaxyTime::from_days(150)),
        0.0
    );
    assert_eq!(
        freight::fails_on(deadline),
        GalaxyTime::from_days(100 + freight::GRACE_DAYS)
    );
}
