//! What each kind of monthly decision costs (DESIGN §5.1.1, issue #58).
//!
//! Decisions aim at ~100 ns each, which only holds if they pick from data
//! prepared for them rather than searching. `retrox bench` switches the
//! timer on to check; off, a decision pays one relaxed load.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The kinds of decision, as `retrox bench` reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// A settlement's construction review.
    Construction,
    /// A settlement's colonization review.
    Colonization,
    /// A settlement's review of whether to found a trading company.
    Founding,
    /// A trader choosing its route for the month.
    Route,
    /// A harvester choosing its run for the month.
    Harvest,
    /// A settlement's review of whether a pirate gang forms there.
    Pirates,
    /// A settlement's review of its loyalty to its faction.
    Politics,
    /// Refreshing one system's trade lists.
    TradeLists,
}

impl Decision {
    pub const ALL: [Decision; 8] = [
        Decision::Construction,
        Decision::Colonization,
        Decision::Founding,
        Decision::Route,
        Decision::Harvest,
        Decision::Pirates,
        Decision::Politics,
        Decision::TradeLists,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Decision::Construction => "construction",
            Decision::Colonization => "colonization",
            Decision::Founding => "founding",
            Decision::Route => "route",
            Decision::Harvest => "harvest",
            Decision::Pirates => "pirates",
            Decision::Politics => "politics",
            Decision::TradeLists => "trade lists",
        }
    }
}

static ON: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TOTALS: RefCell<[(Duration, u64); Decision::ALL.len()]> =
        const { RefCell::new([(Duration::ZERO, 0); Decision::ALL.len()]) };
}

/// Start or stop timing decisions on this thread.
pub fn set_timing(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Time spent on each kind of decision on this thread, and how many were
/// made, since the last call; the totals start again from zero.
pub fn take() -> [(Duration, u64); Decision::ALL.len()] {
    TOTALS.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// Run one decision, timing it if timing is on.
#[inline]
pub(crate) fn time<R>(kind: Decision, f: impl FnOnce() -> R) -> R {
    if !ON.load(Ordering::Relaxed) {
        return f();
    }
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    TOTALS.with(|t| {
        let entry = &mut t.borrow_mut()[kind as usize];
        entry.0 += took;
        entry.1 += 1;
    });
    out
}
