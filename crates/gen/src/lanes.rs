//! The hyperlane network.
//!
//! 1. Backbone: the *relative neighbourhood graph* of nearby systems. It's
//!    sparse (average degree ~2.5), connected wherever stars are close, and
//!    naturally full of chokepoints.
//! 2. Extras: some edges of the denser *Gabriel graph* are added back at
//!    random, so the core isn't a pure maze.
//! 3. Bridges: any regions still disconnected (sparse outskirts, gaps between
//!    arms) are joined by their shortest possible lane, however long.

use rayon::prelude::*;
use retrox_sim::{GalaxyPos, GalaxyState, SystemId};

use crate::GalaxyConfig;
use crate::grid::Grid;
use crate::rng::Rng;

pub fn build(
    rng: &mut Rng,
    state: &GalaxyState,
    config: &GalaxyConfig,
) -> Vec<(SystemId, SystemId)> {
    let positions: Vec<GalaxyPos> = state
        .systems
        .as_slice()
        .iter()
        .map(|s| s.position)
        .collect();
    let n = positions.len();
    let mut grid = Grid::new(config.radius_ly + config.thickness_ly, config.max_lane_ly);
    for (i, &p) in positions.iter().enumerate() {
        grid.insert(i as u32, p);
    }

    // For every pair (a < b) within max_lane_ly, classify it. Parallel over
    // `a`; each worker's output is in a fixed order, so the result is
    // deterministic.
    let classified: Vec<Vec<(u32, u32, Edge)>> = (0..n as u32)
        .into_par_iter()
        .map(|a| {
            let pa = positions[a as usize];
            let near: Vec<u32> = grid
                .near(pa, config.max_lane_ly)
                .filter(|&j| j != a && positions[j as usize].distance(pa) <= config.max_lane_ly)
                .collect();
            let mut edges = Vec::new();
            for &b in near.iter().filter(|&&b| b > a) {
                let pb = positions[b as usize];
                let edge = classify(pa, pb, near.iter().map(|&c| positions[c as usize]));
                if edge != Edge::None {
                    edges.push((a, b, edge));
                }
            }
            edges.sort_by_key(|&(_, b, _)| b);
            edges
        })
        .collect();

    let mut lanes = Vec::new();
    let mut sets = UnionFind::new(n);
    for (a, b, edge) in classified.into_iter().flatten() {
        let keep = match edge {
            Edge::Backbone => true,
            Edge::Extra => rng.chance(config.extra_lane_chance),
            Edge::None => false,
        };
        if keep {
            sets.union(a as usize, b as usize);
            lanes.push((a, b));
        }
    }

    lanes.extend(bridges(&positions, &mut sets));
    lanes
        .into_iter()
        .map(|(a, b)| (SystemId::from_index(a), SystemId::from_index(b)))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// In the relative neighbourhood graph.
    Backbone,
    /// In the Gabriel graph but not the RNG.
    Extra,
    None,
}

/// `others` must include every point within |ab| of `a` (it may include `a`
/// and `b` themselves; they're skipped).
fn classify(a: GalaxyPos, b: GalaxyPos, others: impl Iterator<Item = GalaxyPos>) -> Edge {
    let ab = a.distance(b);
    let mut edge = Edge::Backbone;
    for c in others {
        if c == a || c == b {
            continue;
        }
        let (ac, bc) = (a.distance(c), b.distance(c));
        if ac * ac + bc * bc < ab * ab {
            return Edge::None; // c inside the circle with diameter ab
        }
        if ac.max(bc) < ab {
            edge = Edge::Extra; // c inside the lune
        }
    }
    edge
}

/// Join disconnected components, smallest first, each to its nearest
/// system in any other component.
fn bridges(positions: &[GalaxyPos], sets: &mut UnionFind) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    loop {
        let mut members: std::collections::BTreeMap<usize, Vec<u32>> = Default::default();
        for i in 0..positions.len() {
            members.entry(sets.find(i)).or_default().push(i as u32);
        }
        if members.len() <= 1 {
            return out;
        }
        let smallest = members.values().min_by_key(|m| m.len()).unwrap().clone();
        let root = sets.find(smallest[0] as usize);

        let (a, b) = smallest
            .par_iter()
            .map(|&a| {
                let pa = positions[a as usize];
                let (b, d) = (0..positions.len() as u32)
                    .filter(|&b| sets.find_readonly(b as usize) != root)
                    .map(|b| (b, positions[b as usize].distance(pa)))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                    .unwrap();
                (a, b, d)
            })
            .min_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)))
            .map(|(a, b, _)| (a.min(b), a.max(b)))
            .unwrap();
        sets.union(a as usize, b as usize);
        out.push((a, b));
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    /// `find` without path compression, for shared (parallel) reads.
    fn find_readonly(&self, mut x: usize) -> usize {
        while self.parent[x] != x {
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra.max(rb)] = ra.min(rb);
        }
    }
}
