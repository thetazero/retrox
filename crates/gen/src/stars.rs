//! Where stars go, and what kind they are.

use std::f64::consts::TAU;

use retrox_sim::{GalaxyPos, StarClass};

use crate::GalaxyConfig;
use crate::grid::Grid;
use crate::rng::Rng;

/// Place `config.systems` stars in a spiral disc: dense core, distinct arms,
/// sparse outskirts, with a minimum spacing between any two stars.
pub fn place(rng: &mut Rng, config: &GalaxyConfig) -> Vec<GalaxyPos> {
    let r_max = config.radius_ly;
    let mut grid = Grid::new(r_max + config.thickness_ly, config.min_spacing_ly.max(1.0));
    let mut out: Vec<GalaxyPos> = Vec::with_capacity(config.systems);
    let max_attempts = config.systems * 400;

    for _ in 0..max_attempts {
        if out.len() == config.systems {
            break;
        }
        // Uniform over the disc's area, then thin by density.
        let r = r_max * rng.f64().sqrt();
        let theta = rng.range(0.0, TAU);
        if !rng.chance(density(r / r_max, theta, config.arms)) {
            continue;
        }
        // Thicker near the core, thin at the rim.
        let spread = config.thickness_ly * (1.0 - 0.75 * r / r_max);
        let z = (rng.f64() + rng.f64() + rng.f64() - 1.5) * spread;
        let pos = GalaxyPos::new(r * theta.cos(), r * theta.sin(), z);

        let too_close = grid
            .near(pos, config.min_spacing_ly)
            .any(|i| out[i as usize].distance(pos) < config.min_spacing_ly);
        if too_close {
            continue;
        }
        grid.insert(out.len() as u32, pos);
        out.push(pos);
    }

    assert_eq!(
        out.len(),
        config.systems,
        "couldn't fit {} systems; increase radius_ly or decrease min_spacing_ly",
        config.systems
    );
    out
}

/// Acceptance probability at normalized radius `t` (0 = centre, 1 = rim)
/// and angle `theta`.
fn density(t: f64, theta: f64, arms: u32) -> f64 {
    // A bright core that falls off toward the rim.
    let core = (-(t / 0.12).powi(2)).exp();
    let falloff = (1.0 - t).max(0.0).powf(0.6);
    if arms == 0 {
        return (core + falloff * 0.5).min(1.0);
    }
    // Logarithmic spiral arms: angle of the arm centreline grows with ln(r).
    let winding = 2.2;
    let arm_spacing = TAU / f64::from(arms);
    let arm_angle = winding * (t.max(0.01)).ln();
    let offset = (theta - arm_angle).rem_euclid(arm_spacing);
    let to_arm = offset.min(arm_spacing - offset) / (arm_spacing / 2.0); // 0 on arm, 1 between
    let arm = (-(to_arm / 0.28).powi(2)).exp();
    let inter_arm = 0.04;
    (core + falloff * (inter_arm + (1.0 - inter_arm) * arm)).min(1.0)
}

pub fn star_class(rng: &mut Rng) -> StarClass {
    use StarClass::*;
    // Skewed toward interesting stars rather than real-galaxy frequencies.
    const CLASSES: [StarClass; 10] = [O, B, A, F, G, K, M, WhiteDwarf, Neutron, BlackHole];
    const WEIGHTS: [f64; 10] = [0.5, 2.0, 5.0, 10.0, 15.0, 20.0, 38.0, 6.0, 2.5, 1.0];
    CLASSES[rng.weighted(&WEIGHTS)]
}
