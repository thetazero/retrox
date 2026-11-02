//! What's in each star system: planets, gas giants, moons and belts on
//! Keplerian orbits, and the regions of habitable worlds.
//!
//! Orbits are spaced roughly geometrically outward. What forms where
//! depends on the habitable zone and frost line, both set by the star's
//! luminosity. Habitable planets are split into regions, each with its own
//! resources and climate.

use std::f64::consts::TAU;

use retrox_sim::{
    Body, BodyKind, GalaxyState, Orbit, Orbiting, PlanetClass, Region, Resources, StarClass,
    SystemId,
};

use crate::names;
use crate::rng::Rng;

/// Kilometres per AU.
pub const AU_KM: f64 = 1.496e8;
/// Mean Earth radius, km.
const EARTH_RADIUS_KM: f64 = 6371.0;
/// Planets at least this habitable are split into regions.
const REGION_THRESHOLD: f64 = 0.4;

pub fn generate(rng: &mut Rng, state: &mut GalaxyState) {
    for i in 0..state.systems.len() {
        let mut r = rng.fork(i as u64);
        system(&mut r, state, SystemId::from_index(i as u32));
    }
}

/// Slot kinds before sizes and resources are rolled.
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Planet(PlanetClass),
    GasGiant,
    Belt,
}

fn system(r: &mut Rng, state: &mut GalaxyState, system: SystemId) {
    let star = state.systems[system].star;
    let system_name = state.systems[system].name.clone();
    let hz = star.habitable_zone_au();
    let frost = star.frost_line_au();

    let count = match star {
        StarClass::BlackHole | StarClass::Neutron => r.below(4),
        StarClass::WhiteDwarf => r.below(5),
        _ => 2 + r.below(9),
    };

    let mut a = r.range(0.2, 0.5) * star.mass_suns().max(0.3).sqrt();
    let mut belt_at_frost = false;
    let (mut planet_n, mut belt_n) = (0, 0);
    for _ in 0..count {
        let slot = pick_slot(r, a, hz, frost, &mut belt_at_frost);
        let orbit = Orbit {
            parent: Orbiting::Star,
            semi_major_au: a,
            eccentricity: r.f64().powi(3) * 0.15,
            inclination: r.range(0.0, 0.06),
            ascending_node: r.range(0.0, TAU),
            arg_periapsis: r.range(0.0, TAU),
            mean_anomaly_epoch: r.range(0.0, TAU),
        };
        let name = match slot {
            Slot::Belt => {
                belt_n += 1;
                match belt_n {
                    1 => format!("{system_name} Belt"),
                    2 => format!("{system_name} Outer Belt"),
                    n => format!("{system_name} Belt {}", roman(n)),
                }
            }
            _ => {
                planet_n += 1;
                format!("{system_name} {}", roman(planet_n))
            }
        };
        let beyond_frost = a > frost;
        let body = make_body(r, state, system, star, slot, orbit, name, hz, beyond_frost);
        moons(r, state, system, star, body, hz, beyond_frost);
        a *= r.range(1.4, 2.0);
    }
}

fn pick_slot(r: &mut Rng, a: f64, hz: f64, frost: f64, belt_at_frost: &mut bool) -> Slot {
    use PlanetClass::*;
    if !*belt_at_frost && a > 0.9 * frost {
        *belt_at_frost = true;
        if r.chance(0.35) {
            return Slot::Belt;
        }
    }
    if a > frost {
        return [Slot::GasGiant, Slot::Planet(Ice), Slot::Belt][r.weighted(&[0.6, 0.3, 0.1])];
    }
    if r.chance(0.12) {
        return Slot::Belt;
    }
    let classes: (&[PlanetClass], &[f64]) = if a < 0.6 * hz {
        (&[Barren, Desert], &[0.6, 0.4])
    } else if a < 1.6 * hz {
        (&[Terran, Ocean, Desert, Barren], &[0.12, 0.1, 0.38, 0.4])
    } else {
        (&[Barren, Desert, Ice], &[0.5, 0.2, 0.3])
    };
    Slot::Planet(classes.0[r.weighted(classes.1)])
}

/// How much a star's light and radiation allow life, 0..1.
fn star_factor(star: StarClass) -> f64 {
    use StarClass::*;
    match star {
        O => 0.3,
        B => 0.5,
        A => 0.8,
        F | G | K => 1.0,
        M => 0.7,
        WhiteDwarf => 0.4,
        Neutron => 0.1,
        BlackHole => 0.0,
    }
}

/// 1.0 at the centre of the habitable zone, falling off in log distance.
fn zone_factor(a: f64, hz: f64) -> f64 {
    if hz < 1e-3 {
        return 0.0;
    }
    let x = (a / hz).ln();
    (-(x * x) / (2.0 * 0.25 * 0.25)).exp()
}

fn ore_bias(star: StarClass) -> f64 {
    use StarClass::*;
    match star {
        O | B | Neutron | WhiteDwarf | BlackHole => 1.6,
        _ => 1.0,
    }
}

fn ice_bias(star: StarClass) -> f64 {
    use StarClass::*;
    match star {
        K | M => 1.5,
        O | B => 0.5,
        _ => 1.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn make_body(
    r: &mut Rng,
    state: &mut GalaxyState,
    system: SystemId,
    star: StarClass,
    slot: Slot,
    orbit: Orbit,
    name: String,
    hz: f64,
    beyond_frost: bool,
) -> retrox_sim::BodyId {
    use PlanetClass::*;
    let (ob, ib) = (ore_bias(star), ice_bias(star));
    let mut roll = |power: f64, bias: f64| (r.f64().powf(power) * bias).min(1.0);
    let (kind, resources, class_hab) = match slot {
        Slot::Belt => {
            let resources = Resources {
                ore: roll(0.5, ob),
                ice: roll(if beyond_frost { 0.7 } else { 2.0 }, ib),
            };
            (BodyKind::Belt, resources, 0.0)
        }
        Slot::GasGiant => (BodyKind::GasGiant, Resources::default(), 0.0),
        Slot::Planet(class) => {
            let resources = match class {
                Barren => Resources {
                    ore: roll(1.5, ob),
                    ice: roll(3.0, ib),
                },
                Desert => Resources {
                    ore: roll(1.5, ob),
                    ice: roll(4.0, ib),
                },
                Terran => Resources {
                    ore: roll(2.0, ob),
                    ice: roll(2.0, ib),
                },
                Ocean => Resources {
                    ore: roll(2.5, ob),
                    ice: roll(0.8, ib),
                },
                Ice => Resources {
                    ore: roll(3.0, ob),
                    ice: roll(0.6, ib),
                },
            };
            let class_hab = match class {
                Terran => 1.0,
                Ocean => 0.8,
                Desert => 0.45,
                Barren => 0.12,
                Ice => 0.15,
            };
            (BodyKind::Planet(class), resources, class_hab)
        }
    };
    let (mass, radius_km) = match slot {
        Slot::Belt => (
            r.range(0.001, 0.01),
            orbit.semi_major_au * r.range(0.05, 0.15) * AU_KM,
        ),
        Slot::GasGiant => {
            let re = r.range(4.0, 12.0);
            (r.range(15.0, 400.0), re * EARTH_RADIUS_KM)
        }
        Slot::Planet(class) => {
            let (lo, hi) = match class {
                Barren => (0.3, 0.8),
                Desert => (0.5, 1.1),
                Terran => (0.8, 1.4),
                Ocean => (0.9, 1.6),
                Ice => (0.4, 1.2),
            };
            let re = r.range(lo, hi);
            (re.powi(3) * r.range(0.8, 1.1), re * EARTH_RADIUS_KM)
        }
    };
    let habitability =
        class_hab * star_factor(star) * (0.03 + 0.97 * zone_factor(orbit.semi_major_au, hz));
    let body = state.add_body(Body {
        system,
        name,
        kind,
        orbit,
        mass,
        radius_km,
        resources,
        habitability,
        regions: Vec::new(),
    });
    if matches!(kind, BodyKind::Planet(_)) && habitability >= REGION_THRESHOLD {
        regions(r, state, body);
    }
    body
}

/// Split a habitable world into 4–8 regions with their own resources and
/// climate, then set the world's values to the mean over its regions.
fn regions(r: &mut Rng, state: &mut GalaxyState, body: retrox_sim::BodyId) {
    let (resources, habitability, radius_km) = {
        let b = &state.bodies[body];
        (b.resources, b.habitability, b.radius_km)
    };
    let count =
        ((radius_km / EARTH_RADIUS_KM * 4.0 + r.range(0.0, 2.0)).round() as usize).clamp(4, 8);
    let mut sum = (Resources::default(), 0.0);
    for _ in 0..count {
        let region = Region {
            body,
            name: names::word(r),
            resources: Resources {
                ore: (resources.ore * r.range(0.3, 1.7)).min(1.0),
                ice: (resources.ice * r.range(0.3, 1.7)).min(1.0),
            },
            habitability: (habitability * r.range(0.5, 1.3)).min(1.0),
        };
        sum.0.ore += region.resources.ore;
        sum.0.ice += region.resources.ice;
        sum.1 += region.habitability;
        state.add_region(region);
    }
    let n = count as f64;
    let b = &mut state.bodies[body];
    b.resources = Resources {
        ore: sum.0.ore / n,
        ice: sum.0.ice / n,
    };
    b.habitability = sum.1 / n;
}

fn moons(
    r: &mut Rng,
    state: &mut GalaxyState,
    system: SystemId,
    star: StarClass,
    parent: retrox_sim::BodyId,
    hz: f64,
    beyond_frost: bool,
) {
    let (kind, parent_radius, parent_a, parent_name) = {
        let p = &state.bodies[parent];
        (p.kind, p.radius_km, p.orbit.semi_major_au, p.name.clone())
    };
    let count = match kind {
        BodyKind::GasGiant => 1 + r.weighted(&[0.25, 0.3, 0.2, 0.15, 0.1]),
        BodyKind::Planet(_) => r.weighted(&[0.5, 0.35, 0.15]),
        _ => 0,
    };
    let mut a_km = parent_radius * r.range(10.0, 30.0);
    for k in 0..count {
        let radius_km = r.range(300.0, 3000.0);
        let orbit = Orbit {
            parent: Orbiting::Body(parent),
            semi_major_au: a_km / AU_KM,
            eccentricity: r.f64().powi(3) * 0.05,
            inclination: r.range(0.0, 0.1),
            ascending_node: r.range(0.0, TAU),
            arg_periapsis: r.range(0.0, TAU),
            mean_anomaly_epoch: r.range(0.0, TAU),
        };
        let resources = if beyond_frost {
            Resources {
                ore: (r.f64().powf(2.5) * ore_bias(star)).min(1.0),
                ice: (r.f64().powf(0.8) * ice_bias(star)).min(1.0),
            }
        } else {
            Resources {
                ore: (r.f64().powf(2.2) * ore_bias(star)).min(1.0),
                ice: (r.f64().powi(4) * ice_bias(star)).min(1.0),
            }
        };
        let habitability = 0.08 * star_factor(star) * zone_factor(parent_a, hz);
        let letter = (b'a' + k as u8) as char;
        state.add_body(Body {
            system,
            name: format!("{parent_name} {letter}"),
            kind: BodyKind::Moon,
            orbit,
            // Rocky density, in Earth masses.
            mass: (radius_km / EARTH_RADIUS_KM).powi(3) * 0.6,
            radius_km,
            resources,
            habitability,
            regions: Vec::new(),
        });
        a_km *= r.range(1.4, 2.2);
    }
}

fn roman(mut n: usize) -> String {
    const NUMERALS: [(usize, &str); 9] = [
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::roman;

    #[test]
    fn roman_numerals() {
        let got: Vec<String> = [1, 4, 9, 12, 14].map(roman).to_vec();
        assert_eq!(got, ["I", "IV", "IX", "XII", "XIV"]);
    }
}
