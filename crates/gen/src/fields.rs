//! Asteroid fields inside belts, and the rocks inside a field.

use std::f64::consts::TAU;

use retrox_sim::{AsteroidField, BodyKind, GalaxyState, Orbit, Resources};

use crate::bodies::AU_KM;
use crate::rng::Rng;

/// Give every belt a few fields spread around its ring and across its width.
pub fn generate(rng: &mut Rng, state: &mut GalaxyState) {
    let belts: Vec<_> = state
        .bodies
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Belt)
        .map(|(id, _)| id)
        .collect();
    for belt in belts {
        let mut r = rng.fork(belt.index() as u64);
        let (orbit, half_width_km, res, name) = {
            let b = &state.bodies[belt];
            (b.orbit, b.radius_km, b.resources, b.name.clone())
        };
        let count = 2 + r.below(4);
        for i in 0..count {
            let offset_au = r.range(-1.0, 1.0) * half_width_km / AU_KM;
            let vary = |r: &mut Rng, v: f64| (v * r.range(0.6, 1.4)).clamp(0.02, 1.0);
            state.fields.push(AsteroidField {
                belt,
                name: format!("{name} field {}", (b'A' + i as u8) as char),
                orbit: Orbit {
                    semi_major_au: (orbit.semi_major_au + offset_au).max(1e-6),
                    mean_anomaly_epoch: r.range(0.0, TAU),
                    ..orbit
                },
                radius_km: r.range(15.0, 40.0),
                resources: Resources {
                    ore: vary(&mut r, res.ore),
                    ice: vary(&mut r, res.ice),
                },
                seed: r.next_u64(),
            });
        }
    }
}

/// One rock in a field, relative to the field's centre, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rock {
    pub offset: [f64; 3],
    pub radius_m: f64,
    pub seed: u64,
    /// Unit rotation axis and spin rate (rad/s).
    pub spin_axis: [f64; 3],
    pub spin_rate: f64,
}

/// The rocks of a field, generated from its seed. Denser toward the centre,
/// flattened like the belt, mostly small with a few huge ones.
pub fn rocks(field: &AsteroidField) -> Vec<Rock> {
    let mut rng = Rng::new(field.seed);
    let richness = field.resources.ore.max(field.resources.ice);
    let count = (150.0 + 450.0 * richness) as usize;
    let radius = field.radius_km * 1000.0;
    (0..count)
        .map(|_| {
            let r = radius * rng.f64().powf(0.7);
            let theta = rng.range(0.0, TAU);
            let phi = rng.range(-1.0, 1.0f64).asin() * 0.5;
            let offset = [
                theta.cos() * phi.cos() * r,
                theta.sin() * phi.cos() * r,
                phi.sin() * 0.6 * r,
            ];
            let radius_m = 8.0 * (1.0 - rng.f64()).powf(-0.9).min(120.0);
            let axis = [
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            ];
            let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2])
                .sqrt()
                .max(1e-9);
            Rock {
                offset,
                radius_m,
                seed: rng.next_u64(),
                spin_axis: [axis[0] / len, axis[1] / len, axis[2] / len],
                spin_rate: rng.range(-0.05, 0.05) / radius_m.sqrt(),
            }
        })
        .collect()
}
