//! Shortest routes along the hyperlanes.
//!
//! One search serves every caller: traders measuring how far a market is
//! ([`crate::haul`]), dispatch boards looking for consignees
//! ([`crate::freight`]), and the player plotting a course. Each passes its
//! own cost per lane (light years, or days for a given drive) and decides,
//! system by system, how far to go.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::{GalaxyState, Hyperlane, SystemId};

/// A system reached by [`search`]: the cheapest way there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub system: SystemId,
    /// Total cost of the route here.
    pub cost: f64,
    /// Lanes crossed on the way.
    pub jumps: u32,
}

/// What [`search`] does after reaching a system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visit {
    /// Carry on along its lanes.
    Expand,
    /// Don't go on from here (other routes still go on).
    Leaf,
    /// Stop the whole search.
    Stop,
}

/// Visit every system reachable from `origin`, cheapest first, each once
/// with its cheapest route. `lane_cost` prices crossing a lane; `visit`
/// says whether to go on. Ties go to fewer jumps, then the lower id, so
/// the order is deterministic (DESIGN §5.1).
///
/// Returns each reached system's predecessor on its route (the origin
/// is its own), for [`path`].
pub fn search(
    state: &GalaxyState,
    origin: SystemId,
    lane_cost: impl Fn(&Hyperlane) -> f64,
    mut visit: impl FnMut(Step) -> Visit,
) -> HashMap<SystemId, SystemId> {
    // Best (cost, jumps, previous) found so far for each system.
    let mut best: HashMap<SystemId, (f64, u32, SystemId)> =
        HashMap::from([(origin, (0.0, 0, origin))]);
    let mut heap = BinaryHeap::from([Reverse((0.0f64.to_bits(), 0u32, origin))]);
    while let Some(Reverse((bits, jumps, system))) = heap.pop() {
        let cost = f64::from_bits(bits);
        if best
            .get(&system)
            .is_some_and(|&(b, j, _)| (b, j) < (cost, jumps))
        {
            continue;
        }
        match visit(Step {
            system,
            cost,
            jumps,
        }) {
            Visit::Expand => {}
            Visit::Leaf => continue,
            Visit::Stop => break,
        }
        for &lane in &state.systems[system].lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            let c = cost + lane_cost(lane);
            if best
                .get(&next)
                .is_none_or(|&(b, j, _)| (c, jumps + 1) < (b, j))
            {
                best.insert(next, (c, jumps + 1, system));
                heap.push(Reverse((c.to_bits(), jumps + 1, next)));
            }
        }
    }
    best.into_iter()
        .map(|(s, (_, _, prev))| (s, prev))
        .collect()
}

/// The cheapest route from `from` to `to` by `lane_cost`: every system on
/// the way, starting with `from` and ending with `to`.
pub fn path(
    state: &GalaxyState,
    from: SystemId,
    to: SystemId,
    lane_cost: impl Fn(&Hyperlane) -> f64,
) -> Option<Vec<SystemId>> {
    let mut found = false;
    let prev = search(state, from, lane_cost, |step| {
        found = step.system == to;
        if found { Visit::Stop } else { Visit::Expand }
    });
    if !found {
        return None;
    }
    let mut route = vec![to];
    while let Some(&last) = route.last()
        && last != from
    {
        route.push(prev[&last]);
    }
    route.reverse();
    Some(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GalaxyPos, StarClass, StarSystem};

    /// A line of systems 0–1–2–3 with lanes of 1 ly, plus a 5 ly shortcut
    /// 0–3.
    fn line() -> (GalaxyState, Vec<SystemId>) {
        let mut state = GalaxyState::default();
        let ids: Vec<SystemId> = (0..4)
            .map(|i| {
                state.systems.push(StarSystem {
                    name: format!("S{i}"),
                    position: GalaxyPos {
                        x: f64::from(i),
                        y: 0.0,
                        z: 0.0,
                    },
                    star: StarClass::G,
                    lanes: Vec::new(),
                    bodies: Vec::new(),
                    settlements: Vec::new(),
                })
            })
            .collect();
        for w in ids.windows(2) {
            state.connect(w[0], w[1]);
        }
        let shortcut = state.connect(ids[0], ids[3]);
        state.lanes[shortcut].length_ly = 5.0;
        (state, ids)
    }

    #[test]
    fn cheapest_first_and_by_the_callers_cost() {
        let (state, ids) = line();
        let mut seen = Vec::new();
        search(
            &state,
            ids[0],
            |l| l.length_ly,
            |s| {
                seen.push((s.system, s.cost, s.jumps));
                Visit::Expand
            },
        );
        assert_eq!(
            seen,
            vec![
                (ids[0], 0.0, 0),
                (ids[1], 1.0, 1),
                (ids[2], 2.0, 2),
                (ids[3], 3.0, 3)
            ]
        );
        // Priced per jump instead, the shortcut wins.
        let route = path(&state, ids[0], ids[3], |_| 1.0).unwrap();
        assert_eq!(route, vec![ids[0], ids[3]]);
        let route = path(&state, ids[0], ids[3], |l| l.length_ly).unwrap();
        assert_eq!(route, ids);
    }

    #[test]
    fn leaves_and_stops() {
        let (state, ids) = line();
        let mut seen = Vec::new();
        search(
            &state,
            ids[0],
            |l| l.length_ly,
            |s| {
                seen.push(s.system);
                if s.jumps >= 1 {
                    Visit::Leaf
                } else {
                    Visit::Expand
                }
            },
        );
        // One jump out: 1 directly, and 3 by the shortcut.
        assert_eq!(seen, vec![ids[0], ids[1], ids[3]]);
        let mut n = 0;
        search(
            &state,
            ids[0],
            |l| l.length_ly,
            |_| {
                n += 1;
                Visit::Stop
            },
        );
        assert_eq!(n, 1);
    }
}
