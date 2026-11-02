//! Generate the inside of a star system on demand.
//!
//! Deterministic from the galaxy seed and the system's coarse state: the
//! star class, resources and facilities decide what's there, so a system
//! with ore-mining facilities has rocky asteroid fields with mining
//! stations in them, a populous one has habitats, and so on.

use std::f64::consts::{PI, TAU};

use glam::DVec3;
use retrox_sim::economy::FacilityKind;
use retrox_sim::layout::*;
use retrox_sim::{GalaxyState, StarClass, SystemId};

use crate::names;
use crate::rng::Rng;

const ROMAN: [&str; 12] = [
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
];

pub fn system_layout(state: &GalaxyState, id: SystemId) -> SystemLayout {
    let system = &state.systems[id];
    let mut rng = Rng::derive(state.seed, "system-layout").fork(id.index() as u64);
    let star = star(system.star);
    let mut layout = SystemLayout {
        system: id,
        star,
        bodies: Vec::new(),
        belts: Vec::new(),
        fields: Vec::new(),
        stations: Vec::new(),
        gates: Vec::new(),
    };

    planets(&mut rng, &mut layout, &system.name);
    belts(&mut rng, &mut layout, &system.name, system.resources);
    stations(&mut rng, &mut layout, state, id);
    landing_points(&mut rng, &mut layout, system.economy.population);
    gates(&mut layout, state, id);
    layout
}

fn star(class: StarClass) -> Star {
    use StarClass::*;
    // (solar masses, solar radii, colour)
    let (mass, radius, color) = match class {
        O => (30.0, 10.0, [0.62, 0.72, 1.0]),
        B => (8.0, 4.0, [0.72, 0.8, 1.0]),
        A => (2.0, 1.7, [0.9, 0.92, 1.0]),
        F => (1.3, 1.3, [1.0, 0.97, 0.9]),
        G => (1.0, 1.0, [1.0, 0.92, 0.75]),
        K => (0.7, 0.8, [1.0, 0.76, 0.5]),
        M => (0.3, 0.4, [1.0, 0.55, 0.35]),
        WhiteDwarf => (0.6, 0.012, [0.88, 0.94, 1.0]),
        Neutron => (1.4, 1.5e-5, [0.7, 0.9, 1.0]),
        BlackHole => (10.0, 4.3e-5, [0.05, 0.02, 0.08]),
    };
    let luminosity = match class {
        WhiteDwarf => 0.01,
        Neutron | BlackHole => 0.001,
        _ => f64::powf(mass, 3.5),
    };
    Star {
        class,
        mass_kg: mass * SOLAR_MASS,
        radius_m: radius * SOLAR_RADIUS,
        luminosity,
        color,
    }
}

fn planets(rng: &mut Rng, layout: &mut SystemLayout, system_name: &str) {
    let star = &layout.star;
    let remnant = matches!(
        star.class,
        StarClass::WhiteDwarf | StarClass::Neutron | StarClass::BlackHole
    );
    let count = if remnant {
        rng.below(4)
    } else {
        2 + rng.below(8)
    };
    let sqrt_l = star.luminosity.sqrt().max(0.05);
    let frost_line = 2.7 * sqrt_l * AU;
    let star_mass = star.mass_kg;
    let mut a = rng.range(0.2, 0.5) * sqrt_l * AU;
    a = a.max(star.radius_m * 8.0);

    for i in 0..count {
        let kind = if a < 0.3 * frost_line {
            BodyKind::Lava
        } else if a < frost_line {
            // Habitable-ish zone around sqrt(L) AU.
            let habitable = (a / (sqrt_l * AU) - 1.0).abs() < 0.35;
            *rng.pick(if habitable {
                &[
                    BodyKind::Ocean,
                    BodyKind::Ocean,
                    BodyKind::Rocky,
                    BodyKind::Desert,
                ][..]
            } else {
                &[BodyKind::Rocky, BodyKind::Desert, BodyKind::Barren][..]
            })
        } else if rng.chance(0.6) {
            BodyKind::GasGiant
        } else {
            BodyKind::Ice
        };
        let radius_m = match kind {
            BodyKind::GasGiant => rng.range(2.2e7, 7.5e7),
            BodyKind::Ice => rng.range(2.0e6, 1.4e7),
            BodyKind::Barren => rng.range(1.0e6, 3.5e6),
            _ => rng.range(2.5e6, 9.0e6),
        };
        let density = if kind == BodyKind::GasGiant {
            1300.0
        } else {
            5000.0
        };
        let mass_kg = density * 4.0 / 3.0 * PI * radius_m.powi(3);
        let planet = layout.bodies.len();
        layout.bodies.push(Body {
            name: format!("{system_name} {}", ROMAN[i.min(ROMAN.len() - 1)]),
            kind,
            radius_m,
            mass_kg,
            orbit: Orbit {
                parent: Parent::Star,
                radius_m: a,
                period_s: kepler_period(a, star_mass),
                phase: rng.range(0.0, TAU),
                inclination: rng.range(-0.04, 0.04),
            },
            seed: rng.next_u64(),
            landing_points: Vec::new(),
        });

        let moons = match kind {
            BodyKind::GasGiant => 1 + rng.below(5),
            BodyKind::Ocean | BodyKind::Rocky | BodyKind::Ice => rng.below(3),
            _ => usize::from(rng.chance(0.3)),
        };
        let mut moon_a = radius_m * rng.range(3.0, 6.0);
        for m in 0..moons {
            let moon_kind = if a > frost_line && rng.chance(0.6) {
                BodyKind::Ice
            } else {
                BodyKind::Barren
            };
            let moon_radius = rng.range(2.0e5, (radius_m * 0.3).clamp(2.5e5, 2.6e6));
            layout.bodies.push(Body {
                name: format!(
                    "{} {}",
                    layout.bodies[planet].name,
                    (b'a' + m as u8) as char
                ),
                kind: moon_kind,
                radius_m: moon_radius,
                mass_kg: 3000.0 * 4.0 / 3.0 * PI * moon_radius.powi(3),
                orbit: Orbit {
                    parent: Parent::Body(planet),
                    radius_m: moon_a,
                    period_s: kepler_period(moon_a, mass_kg),
                    phase: rng.range(0.0, TAU),
                    inclination: rng.range(-0.1, 0.1),
                },
                seed: rng.next_u64(),
                landing_points: Vec::new(),
            });
            moon_a *= rng.range(1.5, 2.4);
        }

        a *= rng.range(1.45, 2.1);
    }
}

fn belts(rng: &mut Rng, layout: &mut SystemLayout, system_name: &str, res: retrox_sim::Resources) {
    let star_mass = layout.star.mass_kg;
    let planets: Vec<(usize, f64)> = layout
        .bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| b.orbit.parent == Parent::Star)
        .map(|(i, b)| (i, b.orbit.radius_m))
        .collect();
    let outermost = planets.last().map_or(2.0 * AU, |p| p.1);

    // Rocky belt in a gap between planets, if there's ore.
    if res.ore > 0.15 || rng.chance(0.3) {
        let (inner, outer) = if planets.len() >= 2 {
            let k = 1 + rng.below(planets.len() - 1);
            let (a0, a1) = (planets[k - 1].1, planets[k].1);
            (a0 + (a1 - a0) * 0.3, a0 + (a1 - a0) * 0.7)
        } else {
            (outermost * 1.3, outermost * 1.8)
        };
        add_belt(
            rng,
            layout,
            format!("{system_name} Belt"),
            BeltKind::Rocky,
            Parent::Star,
            inner,
            outer,
            star_mass,
            res.ore,
        );
    }
    // Icy belt beyond the planets, if there's ice.
    if res.ice > 0.15 || rng.chance(0.3) {
        let inner = outermost * rng.range(1.3, 1.6);
        add_belt(
            rng,
            layout,
            format!("{system_name} Outer Belt"),
            BeltKind::Icy,
            Parent::Star,
            inner,
            inner * 1.5,
            star_mass,
            res.ice,
        );
    }
    // Rings around some gas giants.
    for (i, _) in planets {
        let body = &layout.bodies[i];
        if body.kind == BodyKind::GasGiant && rng.chance(0.4) {
            let (r, mass, name) = (
                body.radius_m,
                body.mass_kg,
                format!("Rings of {}", body.name),
            );
            add_belt(
                rng,
                layout,
                name,
                BeltKind::Icy,
                Parent::Body(i),
                r * 1.4,
                r * 2.3,
                mass,
                res.ice.max(0.3),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_belt(
    rng: &mut Rng,
    layout: &mut SystemLayout,
    name: String,
    kind: BeltKind,
    parent: Parent,
    inner_m: f64,
    outer_m: f64,
    central_mass: f64,
    richness: f64,
) {
    let belt = layout.belts.len();
    let fields = 2 + rng.below(4);
    for f in 0..fields {
        let radius = rng.range(inner_m, outer_m);
        layout.fields.push(AsteroidField {
            name: format!("{name} field {}", (b'A' + f as u8) as char),
            kind,
            belt,
            orbit: Orbit {
                parent,
                radius_m: radius,
                period_s: kepler_period(radius, central_mass),
                phase: rng.range(0.0, TAU),
                inclination: rng.range(-0.02, 0.02),
            },
            radius_m: rng.range(15e3, 40e3),
            seed: rng.next_u64(),
            richness: (richness * rng.range(0.6, 1.4)).clamp(0.05, 1.0),
        });
    }
    layout.belts.push(Belt {
        name,
        kind,
        parent,
        inner_m,
        outer_m,
    });
}

fn stations(rng: &mut Rng, layout: &mut SystemLayout, state: &GalaxyState, id: SystemId) {
    let system = &state.systems[id];
    let economy = &system.economy;

    // The "home" planet: the most habitable large body.
    let home = layout
        .bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.is_moon())
        .max_by_key(|(_, b)| match b.kind {
            BodyKind::Ocean => 5,
            BodyKind::Rocky => 4,
            BodyKind::Desert => 3,
            BodyKind::Ice | BodyKind::GasGiant => 2,
            BodyKind::Barren => 1,
            BodyKind::Lava => 0,
        })
        .map(|(i, _)| i);

    let orbiting = |rng: &mut Rng, layout: &SystemLayout, parent: Parent| -> Orbit {
        let (radius, mass) = match parent {
            Parent::Body(i) => {
                let b = &layout.bodies[i];
                (b.radius_m * rng.range(1.3, 2.5), b.mass_kg)
            }
            Parent::Field(i) => (layout.fields[i].radius_m * rng.range(0.6, 1.1), 0.0),
            Parent::Star => (layout.star.radius_m * 40.0, layout.star.mass_kg),
        };
        Orbit {
            parent,
            radius_m: radius,
            period_s: if mass > 0.0 {
                kepler_period(radius, mass)
            } else {
                0.0
            },
            phase: rng.range(0.0, TAU),
            inclination: rng.range(-0.3, 0.3),
        }
    };
    let home_parent = home.map_or(Parent::Star, Parent::Body);

    // Habitats: more for bigger populations.
    let habitats = match economy.population {
        p if p >= 100.0 => 3,
        p if p >= 20.0 => 2,
        p if p >= 3.0 => 1,
        _ => 0,
    };
    for h in 0..habitats {
        let orbit = orbiting(rng, layout, home_parent);
        let anchor = layout_anchor_name(layout, home_parent, &system.name);
        layout.stations.push(Station {
            name: format!("{} {}", anchor, ["Habitat", "Station", "Concourse"][h]),
            kind: StationKind::Habitat,
            orbit,
            size_m: rng.range(1500.0, 4000.0),
            seed: rng.next_u64(),
        });
    }
    if habitats == 0 && economy.population > 0.0 {
        let orbit = orbiting(rng, layout, home_parent);
        layout.stations.push(Station {
            name: format!("{} Outpost", names::word(rng)),
            kind: StationKind::Outpost,
            orbit,
            size_m: rng.range(300.0, 700.0),
            seed: rng.next_u64(),
        });
    }

    // One station per facility, placed where its work is.
    let fields_of = |kind: BeltKind| -> Vec<usize> {
        layout
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.kind == kind)
            .map(|(i, _)| i)
            .collect()
    };
    let (rocky, icy) = (fields_of(BeltKind::Rocky), fields_of(BeltKind::Icy));
    for facility in &economy.facilities {
        let parent = match facility.kind {
            FacilityKind::Mine => rocky.first().map(|_| Parent::Field(*rng.pick(&rocky))),
            FacilityKind::IceHarvester => icy.first().map(|_| Parent::Field(*rng.pick(&icy))),
            _ => None,
        }
        .unwrap_or(home_parent);
        let orbit = orbiting(rng, layout, parent);
        let label = match facility.kind {
            FacilityKind::Mine => "Mining Station",
            FacilityKind::IceHarvester => "Ice Works",
            FacilityKind::Refinery => "Refinery",
            FacilityKind::Hydroponics => "Hydroponics",
            FacilityKind::FuelPlant => "Fuel Depot",
            FacilityKind::Factory => "Works",
        };
        let anchor = layout_anchor_name(layout, parent, &system.name);
        layout.stations.push(Station {
            name: format!("{anchor} {label}"),
            kind: StationKind::Facility(facility.kind),
            orbit,
            size_m: (400.0 + facility.scale.sqrt() * 120.0).min(3000.0),
            seed: rng.next_u64(),
        });
    }
}

fn layout_anchor_name(layout: &SystemLayout, parent: Parent, system_name: &str) -> String {
    match parent {
        Parent::Star => system_name.to_owned(),
        Parent::Body(i) => layout.bodies[i].name.clone(),
        // "Steros Belt field B" → "Steros B"
        Parent::Field(i) => {
            let f = &layout.fields[i];
            let letter = f.name.chars().last().unwrap_or('A');
            format!("{system_name} {letter}")
        }
    }
}

fn landing_points(rng: &mut Rng, layout: &mut SystemLayout, population: f64) {
    for body in &mut layout.bodies {
        let count = match body.kind {
            BodyKind::GasGiant | BodyKind::Lava => 0,
            BodyKind::Ocean | BodyKind::Rocky | BodyKind::Desert if population > 3.0 => {
                1 + rng.below(3)
            }
            _ => usize::from(rng.chance(0.25)),
        };
        for _ in 0..count {
            body.landing_points.push(LandingPoint {
                name: format!(
                    "{} {}",
                    names::word(rng),
                    *rng.pick(&["Port", "Field", "Base", "Landing", "Yards"])
                ),
                latitude: rng.range(-1.1, 1.1),
                longitude: rng.range(-PI, PI),
            });
        }
    }
}

fn gates(layout: &mut SystemLayout, state: &GalaxyState, id: SystemId) {
    let here = state.systems[id].position;
    let distance = (layout.extent_m() * 1.25).max(8.0 * AU);
    for to in state.neighbours(id) {
        let there = state.systems[to].position;
        // Galaxy x/y map onto the system's x/z plane.
        let dir = DVec3::new(there.x - here.x, 0.0, there.y - here.y).normalize_or(DVec3::X);
        layout.gates.push(Gate {
            to,
            name: format!("Gate to {}", state.systems[to].name),
            position: dir * distance,
        });
    }
}

/// The individual rocks in an asteroid field, relative to its centre.
pub fn rocks(field: &AsteroidField) -> Vec<Rock> {
    let mut rng = Rng::new(field.seed);
    let count = (150.0 + 450.0 * field.richness) as usize;
    (0..count)
        .map(|_| {
            // Denser toward the centre; flattened like the belt.
            let r = field.radius_m * rng.f64().powf(0.7);
            let theta = rng.range(0.0, TAU);
            let phi = rng.range(-1.0, 1.0f64).asin() * 0.5;
            let offset = DVec3::new(
                theta.cos() * phi.cos(),
                phi.sin() * 0.6,
                theta.sin() * phi.cos(),
            ) * r;
            // Mostly small, a few huge (power law).
            let radius_m = 8.0 * (1.0 - rng.f64()).powf(-0.9).min(120.0);
            let axis = DVec3::new(
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
                rng.range(-1.0, 1.0),
            )
            .normalize_or(DVec3::Y);
            Rock {
                offset,
                radius_m,
                seed: rng.next_u64(),
                spin_axis: axis,
                spin_rate: rng.range(-0.05, 0.05) / radius_m.sqrt(),
            }
        })
        .collect()
}
