//! Mining: the first thing a new pilot does (DESIGN §7.3), and an activity
//! rather than a timer.
//!
//! **The loop.** Fly into an asteroid field. Ping the prospector (G) and
//! the rocks around you light up: what each is made of, how rich it is,
//! and the glowing *veins* on its surface. Hold the mining laser (left
//! mouse or R) on a rock until it fractures. Big rocks split into smaller
//! ones; small ones crumble; everything sheds ore chunks, which the
//! tractor pulls into the hold when you fly close. Full hold: fly to a
//! station and sell.
//!
//! **The skill.** Veins are weak points. The laser cuts four times as fast
//! on a vein and runs cooler, and a rock fractured through a vein yields
//! half as much again. Rocks spin, so veins turn away: chase them. The
//! laser heats up as it fires; let it hit the top and it locks out while
//! it vents. Radiators cool it faster, reactors cut harder, cargo racks
//! carry more home, so refits matter. Some rocks are *volatile*: rich,
//! but they burst when fractured, scattering their ore and shoving the
//! ship away.
//!
//! **Big rocks are quarries.** Rocks larger than [`QUARRY_RADIUS`] don't
//! break; each fracture calves a boulder off where the laser hit, until
//! the rock is worked out.
//!
//! **The field remembers.** Broken rocks stay broken, calved ones stay
//! worked, and fragments stay where they drifted, for [`REGROW_DAYS`] of
//! galaxy time. (Nobody is out there between visits; it stands in for
//! the belt's slow churn.)
//!
//! Positions here are relative to the field's centre, in the client's
//! axes (metres, y up), like [`crate::world::rock_offset`].

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;
use std::sync::LazyLock;

use glam::{DQuat, DVec3, Vec3};
use retrox_gen::{Rng, Rock};
use retrox_sim::economy::{Good, GoodMap};
use retrox_sim::ship::ShipStats;
use retrox_sim::{AsteroidField, FieldId};

use crate::world::{SECONDS_PER_DAY, rock_offset};

/// How far the rock under the crosshair is read out, metres.
pub const READOUT_RANGE: f64 = 2000.0;
/// How far a prospector ping reaches, metres.
pub const SCAN_RANGE: f64 = 3000.0;
const SCAN_COOLDOWN: f64 = 3.0;
/// How far the tractor reaches for loose ore, metres.
pub const TRACTOR_RANGE: f64 = 120.0;
/// Rocks bigger than this are quarried: fractures calve boulders off them.
pub const QUARRY_RADIUS: f64 = 60.0;
/// Rock under this radius crumbles straight into ore.
const MIN_ROCK: f64 = 5.0;
/// Galaxy days until a worked rock is whole again.
pub const REGROW_DAYS: f64 = 120.0;
/// Seconds before loose ore drifts off for good.
const NUGGET_LIFETIME: f64 = 900.0;
/// Cutting is this many times faster on a vein.
pub const VEIN_CUT: f64 = 4.0;
/// Fracturing through a vein yields this much more.
pub const VEIN_YIELD: f64 = 1.5;
/// Laser heat per second while firing, before cooling (1 = locked out),
/// per MW the lasers make.
const HEAT_PER_MW: f64 = 0.04;
/// Heat while firing on a vein, relative to bare rock.
const VEIN_HEAT: f64 = 0.6;
/// A locked laser comes back once it has cooled to this.
const UNLOCK_HEAT: f64 = 0.2;

/// The rock meshes, by index; see [`rock_mesh`].
pub const ROCK_MESHES: usize = 8;

/// Which of the [`ROCK_MESHES`] a rock with this seed is drawn with.
pub fn rock_mesh(seed: u64) -> usize {
    (seed % ROCK_MESHES as u64) as usize
}

/// The seed that rock mesh `i` is built from.
pub fn rock_mesh_seed(i: usize) -> u64 {
    i as u64 * 7919 + 13
}

/// The lumpy shape of a rock mesh, so the laser hits the surface you see
/// and veins sit on it.
#[derive(Clone, Debug)]
pub struct RockShape {
    bumps: Vec<(Vec3, f32)>,
    pub squash: Vec3,
}

impl RockShape {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        // A few random "dents and bulges" plus overall squash.
        let bumps = (0..7)
            .map(|_| {
                let dir = Vec3::new(
                    rng.range(-1.0, 1.0) as f32,
                    rng.range(-1.0, 1.0) as f32,
                    rng.range(-1.0, 1.0) as f32,
                )
                .normalize_or(Vec3::Y);
                (dir, rng.range(-0.35, 0.3) as f32)
            })
            .collect();
        let squash = Vec3::new(
            rng.range(0.7, 1.2) as f32,
            rng.range(0.5, 0.9) as f32,
            rng.range(0.7, 1.3) as f32,
        );
        Self { bumps, squash }
    }

    /// Bulge along a unit direction, before squashing.
    pub fn bulge(&self, dir: Vec3) -> f32 {
        let mut r = 1.0;
        for (b, amount) in &self.bumps {
            r += amount * dir.dot(*b).max(0.0).powi(3);
        }
        r
    }

    /// The surface point along a unit direction, in unit rock space.
    pub fn surface(&self, dir: Vec3) -> Vec3 {
        dir * self.bulge(dir) * self.squash
    }

    /// Whether a point in unit rock space is inside the rock.
    fn inside(&self, p: Vec3) -> bool {
        let u = p / self.squash;
        let len = u.length();
        len < 1e-6 || len < self.bulge(u / len)
    }

    /// Where a ray (unit rock space) first enters the rock, as a distance
    /// along it, if it does.
    fn hit(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        // Nothing reaches past this: bulges ≤ 1.3, squash ≤ 1.3.
        const BOUND: f32 = 1.75;
        let b = origin.dot(dir);
        let c = origin.length_squared() - BOUND * BOUND;
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let (t0, t1) = (-b - disc.sqrt(), -b + disc.sqrt());
        if t1 < 0.0 {
            return None;
        }
        const STEP: f32 = 0.03;
        let mut prev = t0.max(0.0);
        if self.inside(origin + dir * prev) {
            return Some(prev);
        }
        let mut t = prev;
        while t < t1 {
            t += STEP;
            if self.inside(origin + dir * t) {
                // Narrow it down.
                let (mut lo, mut hi) = (prev, t);
                for _ in 0..8 {
                    let mid = (lo + hi) / 2.0;
                    if self.inside(origin + dir * mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(hi);
            }
            prev = t;
        }
        None
    }
}

static SHAPES: LazyLock<Vec<RockShape>> = LazyLock::new(|| {
    (0..ROCK_MESHES)
        .map(|i| RockShape::new(rock_mesh_seed(i)))
        .collect()
});

pub fn shape(seed: u64) -> &'static RockShape {
    &SHAPES[rock_mesh(seed)]
}

/// What's in a rock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lode {
    pub good: Good,
    /// 0..1; how much ore a tonne of rock gives.
    pub grade: f64,
    /// Unit directions (rock space, before squash) of the veins.
    veins: [Vec3; 4],
    vein_count: usize,
    /// Bursts when it fractures.
    pub volatile: bool,
}

impl Lode {
    /// A rock's lode, from the field it's in and its seed.
    pub fn new(field: &AsteroidField, seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0x6d69_6e65);
        let res = field.resources;
        let ice_share = res.ice / (res.ore + res.ice).max(1e-9);
        let good = if rng.chance(ice_share) {
            Good::Ice
        } else {
            Good::Ore
        };
        let richness = if good == Good::Ice { res.ice } else { res.ore };
        // Mostly middling, now and then a rich one.
        let grade = (0.15 + 0.6 * richness * rng.f64() + 0.35 * rng.f64().powi(4)).min(1.0);
        let volatile = rng.chance(0.07);
        let grade = if volatile {
            (grade * 1.5).min(1.0)
        } else {
            grade
        };
        Self::with(good, grade, volatile, &mut rng)
    }

    fn with(good: Good, grade: f64, volatile: bool, rng: &mut Rng) -> Self {
        let vein_count = 1 + (grade * 3.0 + rng.f64()) as usize;
        let mut veins = [Vec3::Y; 4];
        for v in &mut veins {
            *v = Vec3::new(
                rng.range(-1.0, 1.0) as f32,
                rng.range(-1.0, 1.0) as f32,
                rng.range(-1.0, 1.0) as f32,
            )
            .normalize_or(Vec3::X);
        }
        Self {
            good,
            grade,
            veins,
            vein_count: vein_count.min(4),
            volatile,
        }
    }

    pub fn veins(&self) -> &[Vec3] {
        &self.veins[..self.vein_count]
    }

    /// A letter for the grade, A best.
    pub fn grade_letter(&self) -> char {
        match self.grade {
            g if g >= 0.75 => 'A',
            g if g >= 0.5 => 'B',
            g if g >= 0.3 => 'C',
            _ => 'D',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RockId {
    /// One of the field's own rocks, by index.
    Base(usize),
    /// A fragment broken off one.
    Frag(u32),
}

/// A piece of a broken rock, drifting.
#[derive(Clone, Debug)]
pub struct Frag {
    pub id: u32,
    pub pos: DVec3,
    pub vel: DVec3,
    pub radius: f64,
    pub seed: u64,
    spin_axis: DVec3,
    spin_rate: f64,
    pub lode: Lode,
    /// Galaxy day it broke off.
    born_day: f64,
}

/// Loose ore, waiting for the tractor.
#[derive(Clone, Debug)]
pub struct Nugget {
    pub pos: DVec3,
    pub vel: DVec3,
    pub tonnes: f64,
    pub good: Good,
    pub seed: u64,
    age: f64,
}

/// A flash of light, for fractures.
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub pos: DVec3,
    pub radius: f64,
    pub good: Good,
    pub volatile: bool,
    pub age: f64,
    pub life: f64,
}

/// What we've done to one field.
#[derive(Clone, Debug, Default)]
pub struct FieldState {
    /// Base rocks broken up, and the galaxy day it happened.
    broken: HashMap<usize, f64>,
    /// Boulders calved off each quarried base rock.
    calved: HashMap<usize, u32>,
    /// Laser work done toward each rock's next fracture.
    damage: HashMap<RockId, f64>,
    /// Rocks the prospector (or the laser) has read.
    scanned: HashSet<RockId>,
    /// Galaxy day each base rock in `scanned`, `calved` or `damage` was
    /// last touched, so it all regrows together.
    touched: HashMap<usize, f64>,
    pub frags: Vec<Frag>,
    pub nuggets: Vec<Nugget>,
    next_id: u32,
}

/// A rock that's there to be mined, wherever it came from.
#[derive(Clone, Copy, Debug)]
pub struct LiveRock {
    pub id: RockId,
    pub pos: DVec3,
    pub vel: DVec3,
    pub radius: f64,
    pub rot: DQuat,
    pub seed: u64,
    pub lode: Lode,
    pub scanned: bool,
    /// 0..1 toward the next fracture.
    pub damage: f64,
    /// A quarry with nothing left to calve.
    pub worked_out: bool,
}

impl LiveRock {
    /// World-space (field-relative) centres of the veins, while they last.
    pub fn vein_points(&self) -> impl Iterator<Item = DVec3> + '_ {
        let veins = if self.worked_out {
            &[][..]
        } else {
            self.lode.veins()
        };
        let shape = shape(self.seed);
        veins
            .iter()
            .map(move |&v| self.pos + self.rot * (shape.surface(v).as_dvec3() * self.radius))
    }

    /// Tonnes of ore its next fracture gives, before any vein bonus.
    pub fn yield_t(&self) -> f64 {
        fracture_yield(self.radius, &self.lode)
    }
}

/// Laser work to fracture a rock of this radius.
pub fn integrity(radius: f64) -> f64 {
    0.3 * radius.min(QUARRY_RADIUS).powf(1.5)
}

/// Tonnes of ore a fracture sheds, before any vein bonus. Quarries shed a
/// boulder's worth.
pub fn fracture_yield(radius: f64, lode: &Lode) -> f64 {
    let r = if radius > QUARRY_RADIUS {
        QUARRY_RADIUS / 3.0
    } else {
        radius
    };
    lode.grade * 0.06 * r.powf(1.5)
}

/// How many boulders a quarry gives before it's worked out.
fn quarry_capacity(radius: f64) -> u32 {
    (radius / 15.0) as u32
}

/// Rotation of a spinning rock at a moment of the galaxy clock. The
/// renderer draws with this too.
pub fn spin(axis: DVec3, rate: f64, clock_s: f64) -> DQuat {
    DQuat::from_axis_angle(axis.normalize_or(DVec3::Y), rate * clock_s % TAU)
}

/// Where the laser is pointing.
#[derive(Clone, Copy, Debug)]
pub struct Aim {
    pub rock: LiveRock,
    /// The surface point hit, field-relative.
    pub point: DVec3,
    pub distance: f64,
    pub on_vein: bool,
}

/// The ship, as mining sees it: field-relative.
#[derive(Clone, Copy, Debug)]
pub struct Miner {
    /// The laser's muzzle.
    pub nose: DVec3,
    pub forward: DVec3,
    /// The ship's middle, where the tractor pulls to.
    pub centre: DVec3,
    /// Pickup reach, metres.
    pub reach: f64,
    /// Free space in the hold, tonnes.
    pub free_t: f64,
    pub firing: bool,
}

/// What a frame of mining did, for the world to apply.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub collected: Vec<(Good, f64)>,
    /// A shove on the ship, m/s.
    pub kick: DVec3,
    pub messages: Vec<String>,
}

/// Mining laser cutting rate: laser work per second.
pub fn laser_power(stats: &ShipStats) -> f64 {
    stats.mining_cut
}

/// Laser heat per second while firing on bare rock.
fn laser_heating(stats: &ShipStats) -> f64 {
    stats.mining_heat_mw * HEAT_PER_MW
}

/// Laser cooling per second: radiators shed its heat.
pub fn laser_cooling(stats: &ShipStats) -> f64 {
    0.11 * (stats.heat_dissipation_mw / 16.0).sqrt().clamp(0.5, 3.0)
}

/// The player's mining gear and everything it has done.
#[derive(Clone, Debug, Default)]
pub struct Mining {
    fields: HashMap<FieldId, FieldState>,
    /// 0..1; locked out at 1 until it cools to [`UNLOCK_HEAT`].
    pub heat: f64,
    pub locked: bool,
    /// The laser fired this frame.
    pub firing: bool,
    pub aim: Option<Aim>,
    scan_cooldown: f64,
    /// Seconds since the last prospector ping, for the HUD's ripple.
    pub since_scan: f64,
    pub bursts: Vec<Burst>,
    /// Tonnes mined since last docking.
    pub haul: GoodMap<f64>,
    hold_full_warned: bool,
}

impl Mining {
    pub fn field(&self, id: FieldId) -> Option<&FieldState> {
        self.fields.get(&id)
    }

    /// Every rock left in a field, as of the galaxy clock.
    pub fn live_rocks<'a>(
        &'a self,
        id: FieldId,
        field: &'a AsteroidField,
        rocks: &'a [Rock],
        clock_s: f64,
    ) -> impl Iterator<Item = LiveRock> + 'a {
        let state = self.fields.get(&id);
        let base = rocks.iter().enumerate().filter_map(move |(i, rock)| {
            let rid = RockId::Base(i);
            if state.is_some_and(|s| s.broken.contains_key(&i)) {
                return None;
            }
            let calved = state.and_then(|s| s.calved.get(&i)).copied().unwrap_or(0);
            Some(LiveRock {
                id: rid,
                pos: rock_offset(rock),
                vel: DVec3::ZERO,
                radius: rock.radius_m,
                rot: spin(DVec3::from_array(rock.spin_axis), rock.spin_rate, clock_s),
                seed: rock.seed,
                lode: Lode::new(field, rock.seed),
                scanned: state.is_some_and(|s| s.scanned.contains(&rid)),
                damage: state
                    .and_then(|s| s.damage.get(&rid))
                    .copied()
                    .unwrap_or(0.0)
                    / integrity(rock.radius_m),
                worked_out: rock.radius_m > QUARRY_RADIUS
                    && calved >= quarry_capacity(rock.radius_m),
            })
        });
        let frags = state.into_iter().flat_map(move |s| {
            s.frags.iter().map(move |f| {
                let rid = RockId::Frag(f.id);
                LiveRock {
                    id: rid,
                    pos: f.pos,
                    vel: f.vel,
                    radius: f.radius,
                    rot: spin(f.spin_axis, f.spin_rate, clock_s),
                    seed: f.seed,
                    lode: f.lode,
                    scanned: s.scanned.contains(&rid),
                    damage: s.damage.get(&rid).copied().unwrap_or(0.0) / integrity(f.radius),
                    worked_out: false,
                }
            })
        });
        base.chain(frags)
    }

    /// The first rock along a ray, within `range`.
    #[allow(clippy::too_many_arguments)]
    fn cast(
        &self,
        id: FieldId,
        field: &AsteroidField,
        rocks: &[Rock],
        clock_s: f64,
        origin: DVec3,
        dir: DVec3,
        range: f64,
    ) -> Option<Aim> {
        let mut best: Option<Aim> = None;
        for rock in self.live_rocks(id, field, rocks, clock_s) {
            let to = rock.pos - origin;
            let along = to.dot(dir);
            let bound = rock.radius * 1.75;
            if along < -bound
                || along - bound > range
                || (to - dir * along).length_squared() > bound * bound
            {
                continue;
            }
            let inv = rock.rot.inverse();
            let o = (inv * (origin - rock.pos) / rock.radius).as_vec3();
            let d = (inv * dir).as_vec3();
            let Some(t) = shape(rock.seed).hit(o, d) else {
                continue;
            };
            let distance = f64::from(t) * rock.radius;
            if distance > range || best.as_ref().is_some_and(|b| b.distance <= distance) {
                continue;
            }
            let point = origin + dir * distance;
            let vein_r = (rock.radius * 0.35).max(1.5);
            let on_vein = rock
                .vein_points()
                .any(|v| (v - point).length_squared() < vein_r * vein_r);
            best = Some(Aim {
                rock,
                point,
                distance,
                on_vein,
            });
        }
        best
    }

    /// Prospector ping: read every rock within [`SCAN_RANGE`]. Returns how
    /// many rocks it read, or `None` while it recharges.
    pub fn scan(
        &mut self,
        id: FieldId,
        field: &AsteroidField,
        rocks: &[Rock],
        clock_s: f64,
        from: DVec3,
    ) -> Option<usize> {
        if self.scan_cooldown > 0.0 {
            return None;
        }
        let found: Vec<RockId> = self
            .live_rocks(id, field, rocks, clock_s)
            .filter(|r| (r.pos - from).length() < SCAN_RANGE + r.radius)
            .map(|r| r.id)
            .collect();
        let day = clock_s / SECONDS_PER_DAY;
        let state = self.fields.entry(id).or_default();
        for &rid in &found {
            state.scanned.insert(rid);
            if let RockId::Base(i) = rid {
                state.touched.insert(i, day);
            }
        }
        self.scan_cooldown = SCAN_COOLDOWN;
        self.since_scan = 0.0;
        Some(found.len())
    }

    /// No field here: the laser can still fire (at nothing), cool down,
    /// and effects fade. Returns a message if it overheats.
    pub fn idle(&mut self, dt: f64, stats: &ShipStats, fire: bool) -> Option<String> {
        let can_fire = fire && !self.locked && stats.mining_cut > 0.0;
        self.firing = can_fire;
        self.aim = None;
        self.tick_gear(dt, stats, can_fire, false);
        (can_fire && self.heat >= 1.0).then(|| {
            self.locked = true;
            "Laser overheated: venting".to_owned()
        })
    }

    fn tick_gear(&mut self, dt: f64, stats: &ShipStats, firing: bool, on_vein: bool) {
        self.scan_cooldown = (self.scan_cooldown - dt).max(0.0);
        self.since_scan += dt;
        for b in &mut self.bursts {
            b.age += dt;
        }
        self.bursts.retain(|b| b.age < b.life);
        let cool = laser_cooling(stats);
        if firing {
            let heat = laser_heating(stats) * if on_vein { VEIN_HEAT } else { 1.0 };
            self.heat += (heat - cool * 0.35) * dt;
        } else {
            self.heat -= cool * dt;
        }
        self.heat = self.heat.clamp(0.0, 1.0);
        if self.locked && self.heat <= UNLOCK_HEAT {
            self.locked = false;
        }
    }

    /// One frame of mining in a field.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f64,
        id: FieldId,
        field: &AsteroidField,
        rocks: &[Rock],
        clock_s: f64,
        stats: &ShipStats,
        ship: Miner,
    ) -> Outcome {
        let mut out = Outcome::default();
        let day = clock_s / SECONDS_PER_DAY;
        self.regrow(id, day);

        self.aim = self.cast(
            id,
            field,
            rocks,
            clock_s,
            ship.nose,
            ship.forward,
            READOUT_RANGE,
        );
        let can_fire = ship.firing && !self.locked && stats.mining_cut > 0.0;
        let cutting = self
            .aim
            .filter(|a| can_fire && a.distance <= stats.mining_range_m && !a.rock.worked_out);
        self.firing = can_fire;
        self.tick_gear(dt, stats, can_fire, cutting.is_some_and(|a| a.on_vein));
        if can_fire && self.heat >= 1.0 {
            self.locked = true;
            out.messages.push("Laser overheated: venting".into());
        }

        if let Some(aim) = cutting {
            let rate = laser_power(stats) * if aim.on_vein { VEIN_CUT } else { 1.0 };
            let state = self.fields.entry(id).or_default();
            state.scanned.insert(aim.rock.id);
            if let RockId::Base(i) = aim.rock.id {
                state.touched.insert(i, day);
            }
            let work = state.damage.entry(aim.rock.id).or_insert(0.0);
            *work += rate * dt;
            if *work >= integrity(aim.rock.radius) {
                state.damage.remove(&aim.rock.id);
                let ship_pos = ship.centre;
                self.fracture(id, aim, day, ship_pos, &mut out);
            }
        }

        self.drift(id, dt, ship, &mut out);
        out
    }

    /// Bring back rocks worked long enough ago.
    fn regrow(&mut self, id: FieldId, day: f64) {
        let Some(state) = self.fields.get_mut(&id) else {
            return;
        };
        let fresh = |d: &f64| day - *d < REGROW_DAYS;
        state.broken.retain(|_, d| fresh(d));
        let stale: Vec<usize> = state
            .touched
            .iter()
            .filter(|(i, d)| !fresh(d) && !state.broken.contains_key(i))
            .map(|(&i, _)| i)
            .collect();
        for i in stale {
            state.touched.remove(&i);
            state.calved.remove(&i);
            state.damage.remove(&RockId::Base(i));
            state.scanned.remove(&RockId::Base(i));
        }
        let gone: Vec<u32> = state
            .frags
            .iter()
            .filter(|f| !fresh(&f.born_day))
            .map(|f| f.id)
            .collect();
        state.frags.retain(|f| fresh(&f.born_day));
        for f in gone {
            state.damage.remove(&RockId::Frag(f));
            state.scanned.remove(&RockId::Frag(f));
        }
    }

    /// Break a rock along where the laser hit it.
    fn fracture(&mut self, id: FieldId, aim: Aim, day: f64, ship: DVec3, out: &mut Outcome) {
        let rock = aim.rock;
        let state = self.fields.entry(id).or_default();
        state.next_id += 1;
        let mut rng = Rng::new(rock.seed ^ (u64::from(state.next_id) << 20) ^ 0x6672_6163);
        let lode = rock.lode;
        let mut yield_t = fracture_yield(rock.radius, &lode);
        if aim.on_vein {
            yield_t *= VEIN_YIELD;
        }
        let quarry = rock.radius > QUARRY_RADIUS;
        let normal = (aim.point - rock.pos).normalize_or(DVec3::Y);

        let mut children: Vec<(DVec3, DVec3, f64)> = Vec::new();
        // Where the ore sheds from.
        let source = if quarry {
            // Calve a boulder off where the laser hit.
            let RockId::Base(i) = rock.id else {
                unreachable!("only base rocks are big enough to quarry")
            };
            *state.calved.entry(i).or_insert(0) += 1;
            state.touched.insert(i, day);
            let r = rng.range(8.0, 16.0);
            let pos = aim.point + normal * (r * 1.1);
            children.push((pos, normal * rng.range(1.0, 3.0), r));
            aim.point + normal * 2.0
        } else {
            match rock.id {
                RockId::Base(i) => {
                    state.broken.insert(i, day);
                    state.touched.insert(i, day);
                }
                RockId::Frag(f) => state.frags.retain(|x| x.id != f),
            }
            state.scanned.remove(&rock.id);
            let n = 2 + rng.below(3);
            for _ in 0..n {
                let r = rock.radius * rng.range(0.4, 0.6);
                let dir = random_dir(&mut rng);
                let pos = rock.pos + dir * rock.radius * 0.5;
                let speed = if lode.volatile {
                    rng.range(4.0, 9.0)
                } else {
                    rng.range(1.0, 4.0)
                };
                children.push((pos, rock.vel + dir * speed, r));
            }
            rock.pos
        };

        for (pos, vel, r) in children {
            if r < MIN_ROCK {
                // Too small to stand: it crumbles, its ore with the rest.
                yield_t += fracture_yield(r, &lode) * 0.6;
                continue;
            }
            state.next_id += 1;
            let mut child_rng = rng.fork(u64::from(state.next_id));
            let grade = (lode.grade * child_rng.range(0.85, 1.15)).clamp(0.05, 1.0);
            let axis = random_dir(&mut child_rng);
            let spin_rate = child_rng.range(-0.15, 0.15) / r.sqrt();
            state.frags.push(Frag {
                id: state.next_id,
                pos,
                vel,
                radius: r,
                seed: child_rng.next_u64(),
                spin_axis: axis,
                spin_rate,
                lode: Lode::with(lode.good, grade, false, &mut child_rng),
                born_day: day,
            });
            // A boulder from a quarry comes away already read.
            if quarry {
                state.scanned.insert(RockId::Frag(state.next_id));
            }
        }

        // Shed the ore.
        let pieces = (yield_t / 0.5).ceil().max(1.0) as usize;
        for _ in 0..pieces {
            let dir = if quarry {
                (normal + random_dir(&mut rng) * 0.8).normalize_or(normal)
            } else {
                random_dir(&mut rng)
            };
            let speed = if lode.volatile {
                rng.range(10.0, 25.0)
            } else {
                rng.range(2.0, 7.0)
            };
            let start = if quarry { 0.0 } else { rock.radius * 0.6 };
            state.nuggets.push(Nugget {
                pos: source + dir * start * rng.f64(),
                vel: rock.vel + dir * speed,
                tonnes: yield_t / pieces as f64,
                good: lode.good,
                seed: rng.next_u64(),
                age: 0.0,
            });
        }

        let size = if quarry { 20.0 } else { rock.radius };
        self.bursts.push(Burst {
            pos: source,
            radius: size * if lode.volatile { 2.5 } else { 1.2 },
            good: lode.good,
            volatile: lode.volatile,
            age: 0.0,
            life: if lode.volatile { 1.4 } else { 0.8 },
        });
        if lode.volatile {
            let away = ship - rock.pos;
            let reach = rock.radius * 6.0 + 60.0;
            let d = away.length();
            if d < reach {
                out.kick += away.normalize_or(DVec3::Y) * 45.0 * (1.0 - d / reach);
            }
            out.messages.push("Volatile pocket burst!".into());
        }
        let what = lode.good.name();
        out.messages.push(if quarry {
            format!("Calved a boulder · {yield_t:.1} t {what}")
        } else if aim.on_vein {
            format!("Fractured through a vein · {yield_t:.1} t {what}")
        } else {
            format!("Fractured · {yield_t:.1} t {what}")
        });
    }

    /// Fragments and loose ore drift; the tractor pulls ore in.
    fn drift(&mut self, id: FieldId, dt: f64, ship: Miner, out: &mut Outcome) {
        let Some(state) = self.fields.get_mut(&id) else {
            return;
        };
        for f in &mut state.frags {
            f.pos += f.vel * dt;
            // The field's loose rubble settles rather than flying off for
            // good.
            f.vel *= (-0.08 * dt).exp();
        }
        let mut free = ship.free_t;
        let mut blocked = false;
        for n in &mut state.nuggets {
            n.age += dt;
            let to = ship.centre - n.pos;
            let d = to.length();
            if d < TRACTOR_RANGE && free > 1e-6 {
                // Pull in, faster as it closes, matching the ship.
                let want = to / d.max(1e-6) * (25.0 + (TRACTOR_RANGE - d) * 0.6);
                n.vel += (want - n.vel) * (1.0 - (-4.0 * dt).exp());
            } else {
                if d < TRACTOR_RANGE {
                    blocked = true;
                }
                n.vel *= (-0.4 * dt).exp();
            }
            n.pos += n.vel * dt;
            if (ship.centre - n.pos).length() < ship.reach && free > 1e-6 {
                let take = n.tonnes.min(free);
                free -= take;
                n.tonnes -= take;
                out.collected.push((n.good, take));
                self.haul[n.good] += take;
            }
        }
        state
            .nuggets
            .retain(|n| n.tonnes > 1e-6 && n.age < NUGGET_LIFETIME);
        if blocked && !self.hold_full_warned {
            self.hold_full_warned = true;
            out.messages
                .push("Hold full: dock and sell, or dump cargo at a station".into());
        } else if free > 1e-6 {
            self.hold_full_warned = false;
        }
    }

    /// Leaving a field: loose ore is lost to the dark.
    pub fn leave(&mut self, id: FieldId) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.nuggets.clear();
        }
    }

    /// Positions and radii of the fragments, for collisions.
    pub fn frag_solids(&self, id: FieldId) -> impl Iterator<Item = (DVec3, f64)> + '_ {
        self.fields
            .get(&id)
            .into_iter()
            .flat_map(|s| s.frags.iter().map(|f| (f.pos, f.radius)))
    }

    /// Whether a base rock has been broken up.
    pub fn is_broken(&self, id: FieldId, index: usize) -> bool {
        self.fields
            .get(&id)
            .is_some_and(|s| s.broken.contains_key(&index))
    }
}

fn random_dir(rng: &mut Rng) -> DVec3 {
    loop {
        let v = DVec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let l = v.length_squared();
        if l > 1e-4 && l <= 1.0 {
            return v / l.sqrt();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retrox_sim::{Id, Orbit, Orbiting, Resources};

    fn field() -> AsteroidField {
        AsteroidField {
            belt: Id::from_index(0),
            name: "Test field".into(),
            orbit: Orbit::circular(Orbiting::Star, 1.0, 0.0),
            radius_km: 20.0,
            resources: Resources { ore: 0.8, ice: 0.1 },
            seed: 7,
        }
    }

    fn rock(radius_m: f64) -> Rock {
        Rock {
            offset: [0.0; 3],
            radius_m,
            seed: 3,
            spin_axis: [0.0, 1.0, 0.0],
            spin_rate: 0.0,
        }
    }

    fn fid() -> FieldId {
        FieldId::from_index(0)
    }

    fn courier() -> ShipStats {
        retrox_content::ships::stock("courier").unwrap().stats()
    }

    /// The ship `distance` metres off the rock along +z, nose on it.
    fn miner(distance: f64, firing: bool) -> Miner {
        let centre = DVec3::Z * distance;
        Miner {
            nose: centre - DVec3::Z * 6.0,
            forward: -DVec3::Z,
            centre,
            reach: 10.0,
            free_t: 12.0,
            firing,
        }
    }

    fn run(
        m: &mut Mining,
        rocks: &[Rock],
        ship: Miner,
        seconds: f64,
        clock: &mut f64,
    ) -> Vec<Outcome> {
        let (f, stats, dt) = (field(), courier(), 1.0 / 30.0);
        (0..(seconds / dt) as usize)
            .map(|_| {
                *clock += dt;
                m.update(dt, fid(), &f, rocks, *clock, &stats, ship)
            })
            .collect()
    }

    #[test]
    fn the_laser_hits_the_surface_you_see() {
        let m = Mining::default();
        let rocks = [rock(20.0)];
        let aim = m
            .cast(
                fid(),
                &field(),
                &rocks,
                0.0,
                DVec3::Z * 200.0,
                -DVec3::Z,
                600.0,
            )
            .expect("straight at it");
        let shape = shape(3);
        let surface = shape.surface(Vec3::Z).z as f64 * 20.0;
        assert!((aim.point.z - surface).abs() < 0.5, "{aim:?} vs {surface}");
        // And misses when pointed away.
        assert!(
            m.cast(
                fid(),
                &field(),
                &rocks,
                0.0,
                DVec3::Z * 200.0,
                DVec3::Z,
                600.0
            )
            .is_none()
        );
    }

    #[test]
    fn firing_fractures_a_rock_into_ore_and_fragments() {
        let mut m = Mining::default();
        let rocks = [rock(20.0)];
        let mut clock = 0.0;
        // Bursts: it will overheat, lock, and come back.
        let outcomes = run(&mut m, &rocks, miner(150.0, true), 60.0, &mut clock);
        assert!(
            m.is_broken(fid(), 0),
            "20 m rock still whole after a minute"
        );
        assert!(
            outcomes
                .iter()
                .flat_map(|o| &o.messages)
                .any(|s| s.contains("overheated")),
            "a minute of firing should overheat the courier's laser"
        );
        let state = m.field(fid()).unwrap();
        assert!(!state.frags.is_empty() || !state.nuggets.is_empty());
    }

    #[test]
    fn veins_cut_faster_than_bare_rock() {
        let rocks = [rock(20.0)];
        let f = field();
        let live = Mining::default()
            .live_rocks(fid(), &f, &rocks, 0.0)
            .next()
            .unwrap();
        let vein = live.vein_points().next().expect("every rock has a vein");
        // Aim straight down onto the vein, from outside the rock.
        let from = vein + (vein - live.pos).normalize() * 100.0;
        let dir = (vein - from).normalize();
        let ship = |firing| Miner {
            nose: from,
            forward: dir,
            centre: from,
            reach: 10.0,
            free_t: 12.0,
            firing,
        };
        let mut m = Mining::default();
        let stats = courier();
        m.update(0.1, fid(), &f, &rocks, 0.0, &stats, ship(true));
        let aim = m.aim.expect("aimed at the rock");
        assert!(aim.on_vein, "{aim:?}");
        let on_vein = m.field(fid()).unwrap().damage[&RockId::Base(0)];
        assert!((on_vein - laser_power(&stats) * VEIN_CUT * 0.1).abs() < 1e-9);
    }

    #[test]
    fn the_tractor_fills_the_hold_and_no_more() {
        let mut m = Mining::default();
        let rocks = [rock(30.0)];
        let mut clock = 0.0;
        let mut collected = 0.0;
        // Less than any 30 m rock sheds (grade ≥ 0.15 gives 1.5 t).
        let mut ship = miner(150.0, true);
        ship.free_t = 1.0;
        for _ in 0..120 * 30 {
            for o in run(&mut m, &rocks, ship, 1.0 / 30.0 + 1e-9, &mut clock) {
                for (_, t) in o.collected {
                    collected += t;
                    ship.free_t -= t;
                }
            }
            // Sit in the middle of where the rock was to sweep up ore.
            if m.is_broken(fid(), 0) {
                ship.firing = false;
                ship.centre = DVec3::ZERO;
            }
        }
        assert!(m.is_broken(fid(), 0));
        assert!((collected - 1.0).abs() < 1e-6, "collected {collected} t");
        assert!((m.haul[Good::Ore] + m.haul[Good::Ice] - 1.0).abs() < 1e-6);
        let left: f64 = m
            .field(fid())
            .unwrap()
            .nuggets
            .iter()
            .map(|n| n.tonnes)
            .sum();
        assert!(left > 0.4, "the rest is still floating there: {left} t");
    }

    #[test]
    fn quarries_calve_until_worked_out() {
        let mut m = Mining::default();
        let rocks = [rock(100.0)];
        let mut clock = 0.0;
        let f = field();
        let mut boulders = 0;
        for _ in 0..200 {
            run(&mut m, &rocks, miner(300.0, true), 10.0, &mut clock);
            // Haul the boulders away so the laser keeps working the quarry.
            let state = m.fields.get_mut(&fid()).unwrap();
            boulders += state.frags.len();
            state.frags.clear();
            if m.live_rocks(fid(), &f, &rocks, clock)
                .next()
                .unwrap()
                .worked_out
            {
                break;
            }
        }
        assert!(!m.is_broken(fid(), 0), "a quarry never breaks");
        assert!(
            m.live_rocks(fid(), &f, &rocks, clock)
                .next()
                .unwrap()
                .worked_out
        );
        assert_eq!(boulders as u32, quarry_capacity(100.0));
    }

    #[test]
    fn broken_rocks_stay_broken_then_regrow() {
        let mut m = Mining::default();
        let rocks = [rock(10.0)];
        let mut clock = 0.0;
        run(&mut m, &rocks, miner(100.0, true), 20.0, &mut clock);
        assert!(m.is_broken(fid(), 0));
        let mut later = clock + 10.0 * SECONDS_PER_DAY;
        run(&mut m, &rocks, miner(100.0, false), 0.1, &mut later);
        assert!(m.is_broken(fid(), 0), "still broken ten days on");
        let mut much_later = clock + (REGROW_DAYS + 1.0) * SECONDS_PER_DAY;
        run(&mut m, &rocks, miner(100.0, false), 0.1, &mut much_later);
        assert!(!m.is_broken(fid(), 0), "whole again");
        assert!(m.field(fid()).unwrap().frags.is_empty());
    }

    #[test]
    fn scanning_reads_rocks_in_range_and_recharges() {
        let mut m = Mining::default();
        let mut far = rock(10.0);
        far.offset = [0.0, 10_000.0, 0.0];
        let rocks = [rock(10.0), far];
        let f = field();
        assert_eq!(m.scan(fid(), &f, &rocks, 0.0, DVec3::ZERO), Some(1));
        assert_eq!(m.scan(fid(), &f, &rocks, 0.0, DVec3::ZERO), None);
        let read: Vec<bool> = m
            .live_rocks(fid(), &f, &rocks, 0.0)
            .map(|r| r.scanned)
            .collect();
        assert_eq!(read, [true, false]);
    }
}
