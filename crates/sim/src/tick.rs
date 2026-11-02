//! The tick pipeline: one galaxy day, as an explicit list of phases.
//!
//! Read [`PIPELINE`] top to bottom to know exactly what a day does, and in
//! what order. Phases parallelize internally (rayon), never across each other.

use std::time::{Duration, Instant};

use crate::GalaxyState;
use crate::phases;

/// One step of a galaxy day.
pub struct Phase {
    pub name: &'static str,
    pub run: fn(&mut GalaxyState),
}

/// Everything that happens in a galaxy day, in order (§5.1).
pub const PIPELINE: &[Phase] = &[
    Phase {
        name: "production",
        run: phases::production,
    },
    Phase {
        name: "planning",
        run: phases::planning,
    },
    Phase {
        name: "trade",
        run: phases::trade,
    },
    Phase {
        name: "movement",
        run: phases::movement,
    },
    Phase {
        name: "conflict",
        run: phases::conflict,
    },
    Phase {
        name: "events",
        run: phases::events,
    },
    Phase {
        name: "news",
        run: phases::news,
    },
];

/// Advance the galaxy by one day.
pub fn tick(state: &mut GalaxyState) {
    #[cfg(debug_assertions)]
    let money = crate::ledger::total_money(state);
    for phase in PIPELINE {
        (phase.run)(state);
    }
    #[cfg(debug_assertions)]
    crate::ledger::check(state, money);
    state.time.advance(1);
}

/// Like [`tick`], reporting how long each phase took.
pub fn tick_profiled(state: &mut GalaxyState, mut report: impl FnMut(&'static str, Duration)) {
    #[cfg(debug_assertions)]
    let money = crate::ledger::total_money(state);
    for phase in PIPELINE {
        let start = Instant::now();
        (phase.run)(state);
        report(phase.name, start.elapsed());
    }
    #[cfg(debug_assertions)]
    crate::ledger::check(state, money);
    state.time.advance(1);
}
