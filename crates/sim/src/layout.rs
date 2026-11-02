//! The inside of a star system: star, planets, moons, belts, asteroid
//! fields, stations and hyperlane gates, at real scale.
//!
//! Layouts are *derived* data: `retrox-gen` builds one deterministically
//! from the galaxy seed and the system's coarse state whenever it's needed
//! (the player's system, the LOD bubble), so they aren't stored in saves.
//!
//! All positions are `f64` metres in the system's own frame, with the star
//! at the origin and the orbital plane being x/z (y up). Times are galaxy
//! seconds since the start of history.

use glam::DVec3;

use crate::SystemId;
use crate::economy::FacilityKind;
use crate::state::StarClass;

pub const AU: f64 = 1.495_978_707e11;
pub const SOLAR_MASS: f64 = 1.989e30;
pub const SOLAR_RADIUS: f64 = 6.957e8;
pub const G: f64 = 6.674e-11;
pub const SECONDS_PER_DAY: f64 = 86_400.0;

#[derive(Clone, Debug)]
pub struct SystemLayout {
    pub system: SystemId,
    pub star: Star,
    pub bodies: Vec<Body>,
    pub belts: Vec<Belt>,
    pub fields: Vec<AsteroidField>,
    pub stations: Vec<Station>,
    pub gates: Vec<Gate>,
}

#[derive(Clone, Debug)]
pub struct Star {
    pub class: StarClass,
    pub mass_kg: f64,
    pub radius_m: f64,
    /// Relative to the Sun.
    pub luminosity: f64,
    /// Linear RGB, roughly 0..1.
    pub color: [f32; 3],
}

/// What an orbit goes around.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    Star,
    Body(usize),
    Field(usize),
}

/// A circular orbit, tilted by `inclination` about the x axis.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    pub parent: Parent,
    pub radius_m: f64,
    /// Zero or infinite means stationary relative to the parent.
    pub period_s: f64,
    /// Angle at t = 0, radians.
    pub phase: f64,
    pub inclination: f64,
}

impl Orbit {
    /// Offset from the parent at galaxy time `t_s`.
    pub fn offset(&self, t_s: f64) -> DVec3 {
        let angle = if self.period_s.is_finite() && self.period_s > 0.0 {
            self.phase + std::f64::consts::TAU * (t_s / self.period_s).fract()
        } else {
            self.phase
        };
        let flat = DVec3::new(angle.cos(), 0.0, angle.sin()) * self.radius_m;
        let (s, c) = self.inclination.sin_cos();
        DVec3::new(flat.x, flat.z * s, flat.z * c)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyKind {
    Lava,
    Rocky,
    Desert,
    Ocean,
    Ice,
    GasGiant,
    /// Airless moons and dwarf planets.
    Barren,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub name: String,
    pub kind: BodyKind,
    pub radius_m: f64,
    pub mass_kg: f64,
    pub orbit: Orbit,
    /// Seed for surface detail.
    pub seed: u64,
    pub landing_points: Vec<LandingPoint>,
}

impl Body {
    pub fn is_moon(&self) -> bool {
        matches!(self.orbit.parent, Parent::Body(_))
    }
}

/// A predefined place to land: landing plays an animation and opens the
/// location's UI (§7.6).
#[derive(Clone, Debug)]
pub struct LandingPoint {
    pub name: String,
    /// Radians.
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BeltKind {
    Rocky,
    Icy,
}

/// A belt or planetary ring. Only its asteroid fields have actual rocks.
#[derive(Clone, Debug)]
pub struct Belt {
    pub name: String,
    pub kind: BeltKind,
    pub parent: Parent,
    pub inner_m: f64,
    pub outer_m: f64,
}

/// A dense, minable cluster of asteroids inside a belt.
#[derive(Clone, Debug)]
pub struct AsteroidField {
    pub name: String,
    pub kind: BeltKind,
    pub belt: usize,
    pub orbit: Orbit,
    pub radius_m: f64,
    /// Seed for the individual rocks.
    pub seed: u64,
    /// 0..1, how much is worth mining here.
    pub richness: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StationKind {
    /// Where people live; most services.
    Habitat,
    /// Houses one economic facility.
    Facility(FacilityKind),
    /// A small frontier stop.
    Outpost,
}

#[derive(Clone, Debug)]
pub struct Station {
    pub name: String,
    pub kind: StationKind,
    pub orbit: Orbit,
    /// Rough size for rendering and docking, metres.
    pub size_m: f64,
    pub seed: u64,
}

/// A hyperlane exit. Gates sit far out, in the direction of the system they
/// lead to on the galaxy map.
#[derive(Clone, Debug)]
pub struct Gate {
    pub to: SystemId,
    pub name: String,
    pub position: DVec3,
}

/// One rock in an asteroid field, relative to the field's centre.
#[derive(Clone, Copy, Debug)]
pub struct Rock {
    pub offset: DVec3,
    pub radius_m: f64,
    pub seed: u64,
    /// Rotation axis (unit) and speed, radians per second.
    pub spin_axis: DVec3,
    pub spin_rate: f64,
}

/// Anything with a position in a system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Star,
    Body(usize),
    Field(usize),
    Station(usize),
    Gate(usize),
}

impl SystemLayout {
    pub fn parent_position(&self, parent: Parent, t_s: f64) -> DVec3 {
        match parent {
            Parent::Star => DVec3::ZERO,
            Parent::Body(i) => self.position(Place::Body(i), t_s),
            Parent::Field(i) => self.position(Place::Field(i), t_s),
        }
    }

    pub fn position(&self, place: Place, t_s: f64) -> DVec3 {
        match place {
            Place::Star => DVec3::ZERO,
            Place::Body(i) => {
                let o = &self.bodies[i].orbit;
                self.parent_position(o.parent, t_s) + o.offset(t_s)
            }
            Place::Field(i) => {
                let o = &self.fields[i].orbit;
                self.parent_position(o.parent, t_s) + o.offset(t_s)
            }
            Place::Station(i) => {
                let o = &self.stations[i].orbit;
                self.parent_position(o.parent, t_s) + o.offset(t_s)
            }
            Place::Gate(i) => self.gates[i].position,
        }
    }

    pub fn name(&self, place: Place) -> &str {
        match place {
            Place::Star => "star",
            Place::Body(i) => &self.bodies[i].name,
            Place::Field(i) => &self.fields[i].name,
            Place::Station(i) => &self.stations[i].name,
            Place::Gate(i) => &self.gates[i].name,
        }
    }

    /// Every named place, for navigation lists.
    pub fn places(&self) -> impl Iterator<Item = Place> + '_ {
        std::iter::once(Place::Star)
            .chain((0..self.bodies.len()).map(Place::Body))
            .chain((0..self.fields.len()).map(Place::Field))
            .chain((0..self.stations.len()).map(Place::Station))
            .chain((0..self.gates.len()).map(Place::Gate))
    }

    /// Radius of the outermost thing in the system (excluding gates).
    pub fn extent_m(&self) -> f64 {
        let bodies = self
            .bodies
            .iter()
            .filter(|b| b.orbit.parent == Parent::Star);
        let belts = self.belts.iter().filter(|b| b.parent == Parent::Star);
        bodies
            .map(|b| b.orbit.radius_m)
            .chain(belts.map(|b| b.outer_m))
            .fold(self.star.radius_m * 10.0, f64::max)
    }
}

/// Orbital period of a small body at `radius_m` around a mass.
pub fn kepler_period(radius_m: f64, central_mass_kg: f64) -> f64 {
    std::f64::consts::TAU * (radius_m.powi(3) / (G * central_mass_kg)).sqrt()
}
