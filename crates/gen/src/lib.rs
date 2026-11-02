//! Procedural generation: galaxy, star systems, hyperlanes, factions.
//!
//! Generation is deterministic: the same seed and config always produce the
//! same starting galaxy (see `docs/DESIGN.md` §4). It uses its own small
//! PRNG ([`Rng`]) rather than `rand`, so seeds stay stable across
//! dependency upgrades.

mod economy;
mod factions;
mod grid;
mod lanes;
mod names;
mod rng;
mod stars;

pub use rng::Rng;

use retrox_sim::{GalaxyState, StarSystem};

/// Knobs for galaxy generation.
#[derive(Clone, Debug)]
pub struct GalaxyConfig {
    /// Number of star systems.
    pub systems: usize,
    /// Radius of the galactic disc, in light-years.
    pub radius_ly: f64,
    /// Thickness of the disc at its centre, in light-years.
    pub thickness_ly: f64,
    /// Number of spiral arms.
    pub arms: u32,
    /// No two systems are closer than this.
    pub min_spacing_ly: f64,
    /// Lanes longer than this are only created as bridges between
    /// otherwise disconnected regions.
    pub max_lane_ly: f64,
    /// Chance to keep each "extra" lane beyond the sparse backbone. Higher
    /// means a denser, more forgiving lane network with fewer chokepoints.
    pub extra_lane_chance: f64,
    /// Number of factions at the start of history.
    pub factions: usize,
    /// Share of systems claimed by some faction at the start; the rest is
    /// unclaimed frontier.
    pub claimed_fraction: f64,
}

impl Default for GalaxyConfig {
    fn default() -> Self {
        Self {
            systems: 5000,
            radius_ly: 260.0,
            thickness_ly: 8.0,
            arms: 4,
            min_spacing_ly: 2.5,
            max_lane_ly: 18.0,
            extra_lane_chance: 0.12,
            factions: 20,
            claimed_fraction: 0.6,
        }
    }
}

/// Generate a starting galaxy from a seed.
pub fn generate(seed: u64, config: &GalaxyConfig) -> GalaxyState {
    let mut state = GalaxyState::empty(seed);

    let positions = stars::place(&mut Rng::derive(seed, "positions"), config);
    let names = names::generate_unique(&mut Rng::derive(seed, "names"), positions.len());
    let mut star_rng = Rng::derive(seed, "stars");
    state.systems = retrox_sim::Arena::with_capacity(positions.len());
    for (position, name) in positions.into_iter().zip(names) {
        state.systems.push(StarSystem {
            name,
            position,
            star: stars::star_class(&mut star_rng),
            lanes: Vec::new(),
            owner: None,
            resources: Default::default(),
            economy: Default::default(),
        });
    }

    for (a, b) in lanes::build(&mut Rng::derive(seed, "lanes"), &state, config) {
        state.connect(a, b);
    }

    factions::seed(&mut Rng::derive(seed, "factions"), &mut state, config);
    economy::seed(&mut Rng::derive(seed, "economy"), &mut state);

    state
}
