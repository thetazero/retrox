//! What a star system is made of: natural bodies (planets, moons, belts)
//! and the regions of habitable worlds. Bodies and regions are generated
//! once and never removed; what people build on or around them is a
//! [`Settlement`](crate::Settlement).
//!
//! Positions inside a system are in AU, in a frame centred on the primary
//! star, and are a pure function of time: nothing about orbits is ticked
//! or saved beyond the orbital elements.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::{Id, Resources, SystemId};

pub type BodyId = Id<Body>;
pub type RegionId = Id<Region>;

/// Earth masses per solar mass.
pub const EARTH_MASSES_PER_SUN: f64 = 332_946.0;
/// Days per year of an orbit at 1 AU around one solar mass. Physical time,
/// unrelated to the 360-day galaxy calendar.
const DAYS_PER_KEPLER_YEAR: f64 = 365.25;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    pub system: SystemId,
    pub name: String,
    pub kind: BodyKind,
    pub orbit: Orbit,
    /// Earth masses. For a belt, its total mass.
    pub mass: f64,
    /// Mean radius in km. For a belt, its half-width.
    pub radius_km: f64,
    /// Natural resources, each 0..1 richness. For a divided world, the
    /// mean over its regions.
    pub resources: Resources,
    /// 0..1: how well the body supports population. For a divided world,
    /// the mean over its regions.
    pub habitability: f64,
    /// The regions of a habitable world; empty for undivided bodies.
    pub regions: Vec<RegionId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyKind {
    Planet(PlanetClass),
    GasGiant,
    Moon,
    /// An asteroid belt: a ring at its orbit's semi-major axis.
    Belt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlanetClass {
    Barren,
    Desert,
    Terran,
    Ocean,
    Ice,
}

/// One part of a habitable world, with its own resources and climate.
/// Rival settlements can share a world by holding different regions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub body: BodyId,
    pub name: String,
    pub resources: Resources,
    pub habitability: f64,
}

/// What an orbit goes around.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orbiting {
    Star,
    Body(BodyId),
}

/// Keplerian orbital elements. Angles are in radians; the reference plane
/// is the system's plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Orbit {
    pub parent: Orbiting,
    pub semi_major_au: f64,
    pub eccentricity: f64,
    pub inclination: f64,
    pub ascending_node: f64,
    pub arg_periapsis: f64,
    /// Mean anomaly at day 0.
    pub mean_anomaly_epoch: f64,
}

impl Orbit {
    /// A circular orbit in the reference plane, starting at angle `phase`.
    pub const fn circular(parent: Orbiting, semi_major_au: f64, phase: f64) -> Self {
        Self {
            parent,
            semi_major_au,
            eccentricity: 0.0,
            inclination: 0.0,
            ascending_node: 0.0,
            arg_periapsis: 0.0,
            mean_anomaly_epoch: phase,
        }
    }

    /// Orbital period in days, around a parent of `parent_mass_suns`.
    pub fn period_days(&self, parent_mass_suns: f64) -> f64 {
        DAYS_PER_KEPLER_YEAR * (self.semi_major_au.powi(3) / parent_mass_suns.max(1e-12)).sqrt()
    }

    /// Position relative to the parent at time `t_days`, in AU.
    pub fn position_at(&self, t_days: f64, parent_mass_suns: f64) -> [f64; 3] {
        self.state_at(t_days, parent_mass_suns).0
    }

    /// Position (AU) and velocity (AU per day) relative to the parent at
    /// time `t_days`, from one solution of Kepler's equation.
    pub fn state_at(&self, t_days: f64, parent_mass_suns: f64) -> ([f64; 3], [f64; 3]) {
        let e = self.eccentricity;
        let period = self.period_days(parent_mass_suns);
        let mean = self.mean_anomaly_epoch + TAU * t_days / period;
        let mean = mean.rem_euclid(TAU);
        // Kepler's equation, E - e sin E = M, by Newton's method.
        let mut ecc = if e < 0.8 { mean } else { std::f64::consts::PI };
        for _ in 0..12 {
            let step = (ecc - e * ecc.sin() - mean) / (1.0 - e * ecc.cos());
            ecc -= step;
            if step.abs() < 1e-12 {
                break;
            }
        }
        let a = self.semi_major_au;
        let (se, ce) = ecc.sin_cos();
        let b = a * (1.0 - e * e).sqrt();
        let (x, y) = (a * (ce - e), b * se);
        // dE/dt from M = E - e sin E, with dM/dt = 2π / period.
        let de = TAU / period / (1.0 - e * ce);
        let (vx, vy) = (-a * se * de, b * ce * de);

        let (sw, cw) = self.arg_periapsis.sin_cos();
        let (si, ci) = self.inclination.sin_cos();
        let (sn, cn) = self.ascending_node.sin_cos();
        let rotate = |x: f64, y: f64| {
            let (xw, yw) = (x * cw - y * sw, x * sw + y * cw);
            [xw * cn - yw * ci * sn, xw * sn + yw * ci * cn, yw * si]
        };
        (rotate(x, y), rotate(vx, vy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_velocity_matches_finite_difference() {
        let orbit = Orbit {
            parent: Orbiting::Star,
            semi_major_au: 1.7,
            eccentricity: 0.3,
            inclination: 0.2,
            ascending_node: 1.1,
            arg_periapsis: 2.3,
            mean_anomaly_epoch: 0.4,
        };
        for t in [0.0, 37.0, 400.0] {
            let (_, v) = orbit.state_at(t, 1.2);
            let h = 1e-4;
            let (a, b) = (orbit.position_at(t - h, 1.2), orbit.position_at(t + h, 1.2));
            for i in 0..3 {
                let fd = (b[i] - a[i]) / (2.0 * h);
                assert!((v[i] - fd).abs() < 1e-6, "t {t} axis {i}: {} vs {fd}", v[i]);
            }
        }
    }
}
