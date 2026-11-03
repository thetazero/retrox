//! Mining: the first thing a new pilot does (DESIGN §7.3), and an activity
//! rather than a timer.
//!
//! **The loop.** Fly into an asteroid field. Ping the prospector (G) and
//! the rocks around you light up: what each is made of, how rich it is,
//! and the glowing *veins* on its surface. Work the mining laser (left
//! mouse or R) on a rock, feathering it, until it fractures. Big rocks
//! split into smaller ones; small ones crumble; everything sheds ore
//! chunks, which the tractor pulls into the hold when you fly close.
//! Full hold: fly to a station and sell.
//!
//! **The skill.** Veins are weak points. The laser cuts four times as fast
//! on a vein and runs cooler, and a rock worked mostly on a vein breaks
//! through it for half as much again. Rocks spin, so veins turn away:
//! chase them. And the laser builds *stress* in the rock (#212): each rock
//! has a [`Temper`], a band of stress where it cracks clean, so the pilot
//! feathers the trigger to hold the needle there. Below the band the laser
//! only warms the rock; past it, in the red, it cracks faster but scars the
//! vein, and at the end of the gauge the vein shatters (a botch: poor rock
//! left). Gas pockets warn, then kick the needle. A clean break through a
//! vein that has a core frees it: the jackpot. Pushed into the red the
//! laser runs hot, and at the top of its heat it scorches the hull rather
//! than locking out. Radiators cool it faster, reactors cut harder, cargo
//! racks carry more home, so refits matter. Some rocks are *volatile*:
//! rich and touchy, they burst when fractured, scattering their ore and
//! shoving the ship away.
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

use std::collections::{BTreeSet, HashMap};
use std::f64::consts::TAU;
use std::sync::LazyLock;

use glam::{DQuat, DVec3, Vec3};
use retrox_gen::{Rng, Rock};
use retrox_sim::economy::{Good, GoodMap};
use retrox_sim::ship::ShipStats;
use retrox_sim::{AsteroidField, FieldId};
use serde::{Deserialize, Serialize};

use crate::combat::random_dir;
use crate::world::{SECONDS_PER_DAY, rock_offset};

/// How far the rock under the crosshair is read out, metres.
pub const READOUT_RANGE: f64 = 2000.0;
/// How thick a belt's dust is, per metre of sight: a rock 14 km off is
/// half veiled by it. Must match mesh.wgsl's `BELT_DUST`.
pub const BELT_DUST: f64 = 5.0e-5;
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
/// How far above rock loose ore comes to rest, metres.
const NUGGET_CLEARANCE: f64 = 2.0;
/// Seconds before loose ore drifts off for good.
const NUGGET_LIFETIME: f64 = 900.0;
/// Cutting is this many times faster on a vein.
pub const VEIN_CUT: f64 = 4.0;
/// Fracturing through a vein yields this much more.
pub const VEIN_YIELD: f64 = 1.5;
/// Laser heat per second while firing, before cooling (1 = at the top),
/// per MW the lasers make.
const HEAT_PER_MW: f64 = 0.04;
/// Heat while firing on a vein, relative to bare rock.
const VEIN_HEAT: f64 = 0.6;
/// Heat by how hard the laser is pushing the rock (#212), relative to its
/// rating: below the rock's clean band, in it, and in the red past it.
/// Worked in the band it runs cool enough to keep at; pushed into the red
/// it heats fast and soon scorches the hull.
const COLD_HEAT: f64 = 0.3;
const BAND_HEAT: f64 = 0.5;
const RED_HEAT: f64 = 1.6;
/// A laser run at the top of its heat scorches the hull round it, this
/// much structure a second (#212): heat is a risk, never a lockout.
pub const SCORCH_PER_S: f64 = 6.0;
/// Scorching stops once the laser has cooled to this.
const SCORCH_CLEAR: f64 = 0.9;

/// Stress a rock under the laser sheds per second off it (#212).
const STRESS_FALL: f64 = 0.24;
/// How fast the laser cracks a rock below its clean band, relative to in it:
/// it only warms the rock.
const COLD_CUT: f64 = 0.15;
/// ...and in the red above it, faster, at the vein's cost.
const RED_CUT: f64 = 2.2;
/// A pocket's warning, seconds: time to let off before it goes.
pub const POCKET_WARNING: f64 = 0.7;
/// A pocket that goes with the laser off kicks this share of its stress.
const POCKET_OFF: f64 = 0.25;
/// Scar from the vein at or under which a break is clean.
pub const CLEAN_RUIN: f64 = 0.15;
/// A botched rock gives this share of its ore, as fines.
pub const BOTCH_YIELD: f64 = 0.3;
/// A vein's core, freed by a clean break through it: this many times the
/// rock's ordinary yield on top.
pub const CORE_YIELD: f64 = 2.0;
/// Hull a volatile rock's botched burst takes, structure.
const BOTCH_BLAST: f64 = 40.0;

/// The rock meshes, by index; see [`rock_mesh`]. Each is its own broken
/// shape, so a field isn't copies of one rock at different sizes.
pub const ROCK_MESHES: usize = 20;

/// Which of the [`ROCK_MESHES`] a rock with this seed is drawn with.
pub fn rock_mesh(seed: u64) -> usize {
    (seed % ROCK_MESHES as u64) as usize
}

/// The seed that rock mesh `i` is built from.
pub fn rock_mesh_seed(i: usize) -> u64 {
    i as u64 * 7919 + 13
}

/// The shape of a rock: how far its surface is from its centre in each
/// direction, so the laser hits the surface you see and veins sit on it.
/// An asteroid is a broken thing (DESIGN §8.7): a lumpy body, stretched,
/// with flat fracture faces where pieces came off, craters and knobs, all
/// one function of direction so a rock is solid exactly where it's drawn.
#[derive(Clone, Debug)]
pub struct RockShape {
    /// Broad lumps and hollows: direction, height, sharpness (a power).
    lumps: Vec<(Vec3, f32, i32)>,
    /// Fracture faces: the plane's normal and its distance from the centre.
    cuts: Vec<(Vec3, f32)>,
    /// Craters: centre direction, angular radius (radians), depth.
    craters: Vec<(Vec3, f32, f32)>,
    /// Knobs and chips all over, faces included.
    chips: Vec<(Vec3, f32, i32)>,
    pub squash: Vec3,
}

fn unit(rng: &mut Rng) -> Vec3 {
    Vec3::new(
        rng.range(-1.0, 1.0) as f32,
        rng.range(-1.0, 1.0) as f32,
        rng.range(-1.0, 1.0) as f32,
    )
    .normalize_or(Vec3::Y)
}

impl RockShape {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let lumps = (0..12)
            .map(|_| {
                let sharp = [1, 2, 3, 5, 8][rng.below(5)];
                (unit(&mut rng), rng.range(-0.4, 0.38) as f32, sharp)
            })
            .collect();
        // Two to five fracture faces: what makes a rock read as a broken
        // piece rather than a pebble. One is usually big.
        let n_cuts = 3 + rng.below(4);
        let cuts = (0..n_cuts)
            .map(|k| {
                let depth = if k == 0 {
                    rng.range(0.5, 0.66)
                } else {
                    rng.range(0.64, 0.88)
                };
                (unit(&mut rng), depth as f32)
            })
            .collect();
        let craters = (0..2 + rng.below(4))
            .map(|_| {
                let size = rng.range(0.18, 0.55) as f32;
                (unit(&mut rng), size, size * rng.range(0.12, 0.25) as f32)
            })
            .collect();
        let chips = (0..36)
            .map(|_| {
                let sharp = 30 + rng.below(150) as i32;
                (unit(&mut rng), rng.range(-0.06, 0.07) as f32, sharp)
            })
            .collect();
        // Stretched one way or another: some long, some flat, few round.
        let long = rng.range(1.0, 1.3) as f32;
        let mid = rng.range(0.7, 1.0) as f32;
        let short = rng.range(0.5, 0.78) as f32;
        let squash = match rng.below(3) {
            0 => Vec3::new(long, short, mid),
            1 => Vec3::new(mid, short, long),
            _ => Vec3::new(short, mid, long),
        };
        let mut shape = Self {
            lumps,
            cuts,
            craters,
            chips,
            squash,
        };
        // As big as its size says: a rock whose faces cut deep is scaled
        // back up, so it reads as big as the readout's "13 M" across. Its
        // farthest point stays inside [`ROCK_BOUND`].
        let (mut sum, mut far) = (0.0f32, 0.0f32);
        let samples = 400;
        for i in 0..samples {
            // A Fibonacci sphere: evenly spread directions.
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / samples as f32;
            let r = (1.0 - y * y).sqrt();
            let a = i as f32 * 2.399_963;
            let d = Vec3::new(r * a.cos(), y, r * a.sin());
            let len = shape.surface(d).length();
            sum += len;
            far = far.max(len);
        }
        let scale = (0.88 / (sum / samples as f32)).min(1.68 / far);
        shape.squash *= scale;
        shape
    }

    /// Bulge along a unit direction, before squashing.
    pub fn bulge(&self, dir: Vec3) -> f32 {
        let mut r = 1.0;
        for &(b, amount, sharp) in &self.lumps {
            r += amount * dir.dot(b).max(0.0).powi(sharp);
        }
        // Fracture faces: the surface along `dir` is no further out than
        // the plane, softened a touch so edges catch the light.
        for &(n, h) in &self.cuts {
            let k = dir.dot(n);
            if k > 1e-3 {
                r = smooth_min(r, h / k, 0.04);
            }
        }
        let mut dent = 0.0;
        for &(c, size, depth) in &self.craters {
            // Angle from the centre, as a fraction of the crater's radius.
            let a = dir.dot(c).clamp(-1.0, 1.0).acos() / size;
            if a < 1.6 {
                dent += if a < 1.0 { -depth * (1.0 - a * a) } else { 0.0 }
                    + depth * 0.3 * (-((a - 1.0) / 0.22).powi(2)).exp();
            }
        }
        let mut chip = 0.0;
        for &(b, amount, sharp) in &self.chips {
            chip += amount * dir.dot(b).max(0.0).powi(sharp);
        }
        // Nothing reaches past [`BOUND`] (squash ≤ 1.3).
        ((r + dent) * (1.0 + chip)).clamp(0.4, 1.32)
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
        const BOUND: f32 = ROCK_BOUND as f32;
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

/// Nothing of a rock reaches past this many times its radius: bulges are
/// at most 1.32, squash at most 1.3.
pub const ROCK_BOUND: f64 = 1.75;

/// A smooth minimum: `min(a, b)`, rounded over a width `k`.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

/// How a rock takes the laser (#212): where on its stress it cracks clean,
/// how fast stress builds, how often gas pockets kick it, and whether its
/// vein has a core to free. Every rock differs, so each is a new read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temper {
    /// The clean band: stress from `lo` to `hi` cracks the rock well.
    pub lo: f64,
    pub hi: f64,
    /// Stress per second the laser builds.
    pub rise: f64,
    /// Pockets per second of cutting, and the stress each kicks.
    pub pockets: f64,
    pub kick: f64,
    /// A clean break through a vein frees its core: the jackpot.
    pub core: bool,
}

impl Temper {
    /// A rock's temper, from its seed and what's in it. Ice is soft and
    /// forgiving; rich rock is touchier (a narrower band); volatile rock
    /// is touchiest and full of pockets.
    pub fn of(seed: u64, lode: &Lode) -> Self {
        let mut rng = Rng::new(seed ^ 0x7465_6d70);
        let mut width = if lode.good == Good::Ice { 0.30 } else { 0.22 };
        width *= 1.15 - 0.5 * lode.grade;
        let mut pockets = rng.range(0.08, 0.4);
        let mut rise = rng.range(0.16, 0.26);
        let mut core = 0.12 + 0.3 * lode.grade;
        if lode.volatile {
            width *= 0.65;
            pockets += 0.3;
            rise *= 1.25;
            core += 0.15;
        }
        let hi = rng.range(0.62, 0.9);
        Self {
            lo: hi - width,
            hi,
            rise,
            pockets,
            kick: rng.range(0.2, 0.34),
            core: rng.chance(core),
        }
    }
}

/// How a rock came apart (#212).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Break {
    /// In the clean band, the vein whole or nearly.
    Clean,
    /// Cracked with the vein scarred by time in the red: less ore.
    Rough,
    /// Stress topped out, or the vein was ruined: it shattered to fines.
    Botched,
    /// Clean through a vein that had a core: the core came out whole.
    Jackpot,
}

/// The rock under the laser right now (#212): its stress, how scarred its
/// vein is, and any pocket about to go. Of the moment, not saved.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    pub rock: RockId,
    pub temper: Temper,
    /// 0..1; at 1 the rock shatters.
    pub stress: f64,
    /// 0..1; time in the red scars the vein, and at 1 it's ruined.
    pub ruin: f64,
    /// A pocket about to go: seconds left, and the stress it kicks.
    pub pocket: Option<(f64, f64)>,
    /// Laser work done in this cut, and how much of it on a vein: a rock
    /// worked mostly on its vein breaks through it.
    pub work: f64,
    pub vein_work: f64,
}

impl Cut {
    /// Cracking speed for this stress, relative to the bottom of the band.
    pub fn bite(&self) -> f64 {
        let t = &self.temper;
        if self.stress < t.lo {
            COLD_CUT
        } else if self.stress <= t.hi {
            1.0 + 0.6 * (self.stress - t.lo) / (t.hi - t.lo).max(1e-6)
        } else {
            RED_CUT
        }
    }

    /// Whether stress is in the red, past the clean band.
    pub fn red(&self) -> bool {
        self.stress > self.temper.hi
    }

    /// Whether the laser has kept to the vein for most of this cut.
    pub fn on_vein(&self) -> bool {
        self.vein_work >= 0.5 * self.work
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RockId {
    /// One of the field's own rocks, by index.
    Base(usize),
    /// A fragment broken off one.
    Frag(u32),
}

/// A piece of a broken rock, drifting.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub how: Break,
    pub age: f64,
    pub life: f64,
}

/// What we've done to one field. Saved with the game.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FieldState {
    /// Base rocks broken up, and the galaxy day it happened.
    #[serde(with = "crate::save::pairs")]
    broken: HashMap<usize, f64>,
    /// Boulders calved off each quarried base rock.
    #[serde(with = "crate::save::pairs")]
    calved: HashMap<usize, u32>,
    /// Laser work done toward each rock's next fracture.
    #[serde(with = "crate::save::pairs")]
    damage: HashMap<RockId, f64>,
    /// Rocks the prospector (or the laser) has read.
    scanned: BTreeSet<RockId>,
    /// Rocks whose vein a botch shattered (#212): poor rock now.
    #[serde(default)]
    ruined: BTreeSet<RockId>,
    /// Galaxy day each base rock in `scanned`, `calved` or `damage` was
    /// last touched, so it all regrows together.
    #[serde(with = "crate::save::pairs")]
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
    /// A botch shattered its vein (#212): no veins, poor yield.
    pub ruined: bool,
}

impl LiveRock {
    /// World-space (field-relative) centres of the veins, while they last.
    pub fn vein_points(&self) -> impl Iterator<Item = DVec3> + '_ {
        self.vein_marks()
            .map(move |v| self.pos + self.rot * (v.as_dvec3() * self.radius))
    }

    /// The centres of the veins, while they last, on the rock's mesh (its
    /// own axes, unit radius): where they're drawn.
    pub fn vein_marks(&self) -> impl Iterator<Item = Vec3> + '_ {
        let veins = if self.worked_out || self.ruined {
            &[][..]
        } else {
            self.lode.veins()
        };
        let shape = shape(self.seed);
        veins.iter().map(move |&v| shape.surface(v))
    }

    /// Tonnes of ore its next fracture gives, before any vein bonus.
    pub fn yield_t(&self) -> f64 {
        fracture_yield(self.radius, &self.lode)
    }

    pub fn temper(&self) -> Temper {
        Temper::of(self.seed, &self.lode)
    }
}

/// A rock as something to bump into: where it is, how it's turned, and
/// its lumpy shape, so things stop at the surface you see rather than at
/// a sphere of its nominal radius (#155). A squashed quarry can sit a
/// third of that radius in, and a bulge can stand well past it.
#[derive(Clone, Copy, Debug)]
pub struct Solid {
    pub pos: DVec3,
    pub radius: f64,
    pub rot: DQuat,
    pub seed: u64,
    /// One of the field's own rocks rather than a broken-off piece.
    pub base: bool,
}

impl Solid {
    /// How far from the centre the surface is, toward `p`, metres. The
    /// rock is star-shaped about its centre, so a point nearer than this
    /// along the same line is inside it.
    pub fn surface_toward(&self, p: DVec3) -> f64 {
        surface_along(self.seed, self.rot, self.radius, p - self.pos)
    }

    /// Nothing of the rock reaches past this, metres.
    pub fn bound(&self) -> f64 {
        self.radius * ROCK_BOUND
    }

    /// If `p` is within `margin` of the surface or inside, how to move it
    /// out: the outward direction (from the centre) and how far.
    pub fn push_out(&self, p: DVec3, margin: f64) -> Option<(DVec3, f64)> {
        let to = p - self.pos;
        let d2 = to.length_squared();
        let bound = self.bound() + margin;
        if d2 > bound * bound {
            return None;
        }
        let d = d2.sqrt();
        let n = if d > 1e-6 { to / d } else { DVec3::Y };
        let want = self.surface_toward(p) + margin;
        (d < want).then_some((n, want - d))
    }
}

/// Distance from a rock's centre to its surface along `dir` (world axes,
/// any length), metres.
pub fn surface_along(seed: u64, rot: DQuat, radius: f64, dir: DVec3) -> f64 {
    let shape = shape(seed);
    let q = (rot.inverse() * dir.normalize_or(DVec3::Y)).as_vec3();
    // The point t·q is inside while |t·q / squash| < bulge of that
    // direction, so the surface is at t = bulge / |q / squash|.
    let u = q / shape.squash;
    let len = u.length().max(1e-6);
    f64::from(shape.bulge(u / len) / len) * radius
}

/// Laser work to fracture a rock of this radius.
pub fn integrity(radius: f64) -> f64 {
    // A quarry calves a boulder for the work of a 30 m rock: it sheds a
    // 20 m rock's ore and leaves the boulder to break (#212).
    let r = if radius > QUARRY_RADIUS {
        QUARRY_CALVE_R
    } else {
        radius
    };
    INTEGRITY * r.powf(1.5)
}

/// The size of rock a quarry's calving is as much work as, metres.
const QUARRY_CALVE_R: f64 = 30.0;

/// Laser work per m^1.5 of rock to fracture it.
const INTEGRITY: f64 = 0.6;

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
    /// Structure the hull takes: a scorching laser, a botched volatile.
    pub hurt: f64,
    /// A rock broke this frame, and how.
    pub broke: Option<Break>,
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

/// The player's mining gear and everything it has done. What it has done
/// to each field, and the haul, are saved with the game; what's only of
/// the moment (aim, flashes, the trigger, the rock's stress) isn't.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mining {
    #[serde(with = "crate::save::pairs")]
    fields: HashMap<FieldId, FieldState>,
    /// 0..1; at 1 the laser scorches the hull round it until it cools.
    pub heat: f64,
    /// At the top of its heat and burning the hull (#212).
    #[serde(skip)]
    pub scorching: bool,
    /// The trigger is held.
    #[serde(skip)]
    pub held: bool,
    /// The laser is cutting a rock this frame. It only fires at rock in
    /// its reach: held on nothing, it stays cold (#212).
    #[serde(skip)]
    pub firing: bool,
    #[serde(skip)]
    pub aim: Option<Aim>,
    /// The rock last under the laser, while its stress lasts.
    #[serde(skip)]
    pub cut: Option<Cut>,
    #[serde(skip, default = "pocket_rng")]
    rng: Rng,
    scan_cooldown: f64,
    /// Seconds since the last prospector ping, for the HUD's ripple.
    pub since_scan: f64,
    #[serde(skip)]
    pub bursts: Vec<Burst>,
    /// Tonnes mined since last docking.
    pub haul: GoodMap<f64>,
    hold_full_warned: bool,
}

fn pocket_rng() -> Rng {
    Rng::new(0x706f_636b)
}

impl Default for Mining {
    fn default() -> Self {
        Self {
            fields: HashMap::new(),
            heat: 0.0,
            scorching: false,
            held: false,
            firing: false,
            aim: None,
            cut: None,
            rng: pocket_rng(),
            scan_cooldown: 0.0,
            // Never pinged: no ripple on the HUD until the first ping.
            since_scan: 1e9,
            bursts: Vec::new(),
            haul: GoodMap::default(),
            hold_full_warned: false,
        }
    }
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
                ruined: state.is_some_and(|s| s.ruined.contains(&rid)),
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
                    ruined: s.ruined.contains(&rid),
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
            let bound = rock.radius * ROCK_BOUND;
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

    /// No field here: nothing for the laser to cut, so it stays cold
    /// while it cools, and effects fade.
    pub fn idle(&mut self, dt: f64, stats: &ShipStats, fire: bool) {
        self.held = fire && stats.mining_cut > 0.0;
        self.firing = false;
        self.aim = None;
        self.cut = None;
        let mut out = Outcome::default();
        self.tick_gear(dt, stats, false, false, &mut out);
    }

    /// The laser's heat, and the rock's stress, for a frame.
    fn tick_gear(
        &mut self,
        dt: f64,
        stats: &ShipStats,
        firing: bool,
        on_vein: bool,
        out: &mut Outcome,
    ) {
        let push = match &self.cut {
            Some(c) if c.red() => RED_HEAT,
            Some(c) if c.stress >= c.temper.lo => BAND_HEAT,
            _ => COLD_HEAT,
        };
        self.scan_cooldown = (self.scan_cooldown - dt).max(0.0);
        self.since_scan += dt;
        for b in &mut self.bursts {
            b.age += dt;
        }
        self.bursts.retain(|b| b.age < b.life);
        let cool = laser_cooling(stats);
        if firing {
            let heat = laser_heating(stats) * push * if on_vein { VEIN_HEAT } else { 1.0 };
            self.heat += (heat - cool * 0.35) * dt;
        } else {
            self.heat -= cool * dt;
        }
        self.heat = self.heat.clamp(0.0, 1.0);
        if firing && self.heat >= 1.0 {
            if !self.scorching {
                out.messages
                    .push("Laser overheating: it's scorching the hull. Let off".into());
            }
            self.scorching = true;
        } else if self.heat <= SCORCH_CLEAR || !firing {
            self.scorching = false;
        }
        if self.scorching {
            out.hurt += SCORCH_PER_S * dt;
        }
        // Off the rock its stress bleeds away.
        if !firing && let Some(cut) = &mut self.cut {
            cut.stress = (cut.stress - STRESS_FALL * dt).max(0.0);
            if let Some((left, kick)) = &mut cut.pocket {
                *left -= dt;
                if *left <= 0.0 {
                    cut.stress += *kick * POCKET_OFF;
                    cut.pocket = None;
                }
            }
        }
    }

    /// The rock under the laser takes a frame of it: its stress rises,
    /// pockets warn and go, and the red scars the vein. Returns how fast
    /// it cracks (relative to the bottom of its clean band), and whether
    /// its vein just shattered.
    fn stress(&mut self, dt: f64, rock: &LiveRock) -> (f64, bool) {
        let cut = match &mut self.cut {
            Some(c) if c.rock == rock.id => c,
            slot => slot.insert(Cut {
                rock: rock.id,
                temper: rock.temper(),
                stress: 0.0,
                ruin: 0.0,
                pocket: None,
                work: 0.0,
                vein_work: 0.0,
            }),
        };
        let t = cut.temper;
        cut.stress += t.rise * dt;
        match &mut cut.pocket {
            Some((left, kick)) => {
                *left -= dt;
                if *left <= 0.0 {
                    cut.stress += *kick;
                    cut.pocket = None;
                }
            }
            None => {
                if self.rng.chance(t.pockets * dt) {
                    let kick = t.kick * self.rng.range(0.7, 1.3);
                    cut.pocket = Some((POCKET_WARNING, kick));
                }
            }
        }
        if rock.ruined {
            // Nothing left to spoil: it only cracks.
            cut.stress = cut.stress.min(1.0);
            cut.ruin = 1.0;
            return (cut.bite(), false);
        }
        if cut.red() {
            let over = (cut.stress - t.hi) / (1.0 - t.hi).max(1e-6);
            cut.ruin += (0.3 + 0.9 * over) * dt;
        }
        let shattered = cut.stress >= 1.0 || cut.ruin >= 1.0;
        if shattered {
            // The vein's gone; the rock's stress goes with the blast.
            cut.stress = 0.0;
            cut.ruin = 1.0;
            cut.pocket = None;
        }
        (cut.bite(), shattered)
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
        self.held = ship.firing && stats.mining_cut > 0.0;
        let cutting = self
            .aim
            .filter(|a| self.held && a.distance <= stats.mining_range_m && !a.rock.worked_out);
        self.firing = cutting.is_some();
        self.tick_gear(
            dt,
            stats,
            self.firing,
            cutting.is_some_and(|a| a.on_vein),
            &mut out,
        );

        if let Some(aim) = cutting {
            let (bite, shattered) = self.stress(dt, &aim.rock);
            if shattered {
                self.shatter(id, aim, ship.centre, &mut out);
            }
            let rate = laser_power(stats) * if aim.on_vein { VEIN_CUT } else { 1.0 };
            let state = self.fields.entry(id).or_default();
            state.scanned.insert(aim.rock.id);
            if let RockId::Base(i) = aim.rock.id {
                state.touched.insert(i, day);
            }
            let done = rate * bite * dt;
            if let Some(cut) = &mut self.cut {
                cut.work += done;
                if aim.on_vein {
                    cut.vein_work += done;
                }
            }
            let state = self.fields.entry(id).or_default();
            let work = state.damage.entry(aim.rock.id).or_insert(0.0);
            *work += done;
            if *work >= integrity(aim.rock.radius) {
                state.damage.remove(&aim.rock.id);
                let ruined = aim.rock.ruined || state.ruined.remove(&aim.rock.id);
                let cut = self.cut.take().expect("a rock under the laser has a cut");
                let how = if ruined {
                    Break::Botched
                } else if cut.ruin > CLEAN_RUIN {
                    Break::Rough
                } else if cut.on_vein() && cut.temper.core {
                    Break::Jackpot
                } else {
                    Break::Clean
                };
                let ship_pos = ship.centre;
                self.fracture(id, aim, &cut, how, day, ship_pos, &mut out);
            }
        }

        let solids: Vec<Solid> = if self.fields.get(&id).is_some_and(|s| !s.nuggets.is_empty()) {
            self.solids(id, rocks, clock_s).collect()
        } else {
            Vec::new()
        };
        self.drift(id, dt, ship, &solids, &mut out);
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
            state.ruined.remove(&RockId::Base(i));
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
            state.ruined.remove(&RockId::Frag(f));
        }
    }

    /// A botch (#212): the rock's stress topped out, or the red ruined its
    /// vein. The vein shatters to a spray of fines, leaving poor rock that
    /// still has to be cut through; a volatile one blows in the ship's
    /// face.
    fn shatter(&mut self, id: FieldId, aim: Aim, ship: DVec3, out: &mut Outcome) {
        let rock = aim.rock;
        let state = self.fields.entry(id).or_default();
        state.ruined.insert(rock.id);
        state.next_id += 1;
        let mut rng = Rng::new(rock.seed ^ (u64::from(state.next_id) << 20) ^ 0x626f_7463);
        let lode = rock.lode;
        let fines = fracture_yield(rock.radius, &lode) * 0.1;
        let normal = (aim.point - rock.pos).normalize_or(DVec3::Y);
        let pieces = (fines / 0.25).ceil().max(1.0) as usize;
        for _ in 0..pieces {
            let dir = (normal + random_dir(&mut rng)).normalize_or(normal);
            state.nuggets.push(Nugget {
                pos: aim.point + normal * 3.0,
                vel: rock.vel + dir * rng.range(8.0, 18.0),
                tonnes: fines / pieces as f64,
                good: lode.good,
                seed: rng.next_u64(),
                age: 0.0,
            });
        }
        self.bursts.push(Burst {
            pos: aim.point,
            radius: rock.radius.min(30.0) * if lode.volatile { 2.0 } else { 1.0 },
            good: lode.good,
            volatile: lode.volatile,
            how: Break::Botched,
            age: 0.0,
            life: 1.0,
        });
        if lode.volatile {
            let away = ship - aim.point;
            let reach = rock.radius * 4.0 + 80.0;
            let d = away.length();
            if d < reach {
                let k = 1.0 - d / reach;
                out.kick += away.normalize_or(DVec3::Y) * 40.0 * k;
                out.hurt += BOTCH_BLAST * k;
            }
            out.messages
                .push("BOTCHED: the volatile pocket blew, and the vein with it".into());
        } else {
            out.messages
                .push("BOTCHED: the vein shattered. What's left is poor rock".into());
        }
        out.broke = Some(Break::Botched);
    }

    /// Break a rock along where the laser hit it, `how` it came apart.
    #[allow(clippy::too_many_arguments)]
    fn fracture(
        &mut self,
        id: FieldId,
        aim: Aim,
        cut: &Cut,
        how: Break,
        day: f64,
        ship: DVec3,
        out: &mut Outcome,
    ) {
        let rock = aim.rock;
        let state = self.fields.entry(id).or_default();
        state.next_id += 1;
        let mut rng = Rng::new(rock.seed ^ (u64::from(state.next_id) << 20) ^ 0x6672_6163);
        let lode = rock.lode;
        let base_t = fracture_yield(rock.radius, &lode);
        let mut yield_t = match how {
            Break::Botched => base_t * BOTCH_YIELD,
            // A scarred vein gives less, down to half.
            Break::Rough => base_t * (1.0 - 0.5 * cut.ruin.min(1.0)),
            Break::Clean | Break::Jackpot => base_t,
        };
        let through_vein = cut.on_vein() && how != Break::Botched;
        if through_vein {
            yield_t *= VEIN_YIELD;
        }
        let botched = how == Break::Botched;
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
            // Off the surface you see, not off a sphere of the quarry's
            // nominal radius: a squashed quarry's face can be far inside
            // that, and its ore must come out where the ship can reach.
            let surface = surface_along(rock.seed, rock.rot, rock.radius, normal);
            let r = rng.range(8.0, 16.0);
            let pos = rock.pos + normal * (surface + r * 1.1);
            children.push((pos, normal * rng.range(1.0, 3.0), r));
            rock.pos + normal * (surface + 3.0)
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
                let speed = if lode.volatile || botched {
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

        // Shed the ore: a botch flings fines everywhere, fast; a clean
        // break lets chunks drift off.
        let piece_t = if botched { 0.25 } else { 0.5 };
        let pieces = (yield_t / piece_t).ceil().max(1.0) as usize;
        for _ in 0..pieces {
            let dir = if quarry {
                (normal + random_dir(&mut rng) * 0.8).normalize_or(normal)
            } else {
                random_dir(&mut rng)
            };
            let speed = if lode.volatile {
                rng.range(10.0, 25.0)
            } else if botched {
                rng.range(8.0, 18.0)
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
        // The core comes out whole, one big chunk, toward the ship.
        let core_t = if how == Break::Jackpot {
            base_t * CORE_YIELD
        } else {
            0.0
        };
        if core_t > 0.0 {
            let dir = (ship - source).normalize_or(normal);
            state.nuggets.push(Nugget {
                pos: source + dir * rock.radius.min(20.0) * 0.5,
                vel: rock.vel + dir * 3.0,
                tonnes: core_t,
                good: lode.good,
                seed: rng.next_u64(),
                age: 0.0,
            });
        }

        let size = if quarry { 14.0 } else { rock.radius };
        let burst = match how {
            _ if lode.volatile => 2.5,
            Break::Jackpot => 2.0,
            Break::Botched => 1.8,
            _ => 1.2,
        };
        self.bursts.push(Burst {
            pos: source,
            radius: size * burst,
            good: lode.good,
            volatile: lode.volatile,
            how,
            age: 0.0,
            life: if lode.volatile || how == Break::Jackpot {
                1.4
            } else {
                0.8
            },
        });
        if lode.volatile {
            let away = ship - rock.pos;
            let reach = rock.radius * 6.0 + 60.0;
            let d = away.length();
            if d < reach {
                let k = 1.0 - d / reach;
                out.kick += away.normalize_or(DVec3::Y) * 45.0 * k;
            }
        }
        let what = lode.good.name();
        let total = yield_t + core_t;
        let off = if quarry { "Calved a boulder" } else { "Broke" };
        out.messages.push(match how {
            Break::Jackpot => {
                format!("JACKPOT: the vein's core came out whole · {total:.1} t {what}")
            }
            Break::Clean if through_vein => {
                format!("{off} clean through a vein · {total:.1} t {what}")
            }
            Break::Clean => format!("{off} clean · {total:.1} t {what}"),
            Break::Rough => format!("{off} rough: the red scarred the vein · {total:.1} t {what}"),
            Break::Botched => format!("{off}, poor rock · {total:.1} t {what}"),
        });
        out.broke = Some(how);
    }

    /// Fragments and loose ore drift; the tractor pulls ore in.
    fn drift(&mut self, id: FieldId, dt: f64, ship: Miner, solids: &[Solid], out: &mut Outcome) {
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
            let pulled = d < TRACTOR_RANGE && free > 1e-6;
            if pulled {
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
            if !pulled {
                // Loose ore rests on rock, never in it, so the tractor
                // can always get at it from outside (#155). (Ore being
                // pulled in goes straight to the hold, through anything.)
                for s in solids {
                    if let Some((out_dir, depth)) = s.push_out(n.pos, NUGGET_CLEARANCE) {
                        n.pos += out_dir * depth;
                        let into = n.vel.dot(out_dir);
                        if into < 0.0 {
                            n.vel -= out_dir * into;
                        }
                    }
                }
            }
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

    /// Set something loose in a field for the tractor to pick up: cargo
    /// spilled from a wreck, say.
    pub fn drop_loose(
        &mut self,
        id: FieldId,
        pos: DVec3,
        vel: DVec3,
        good: Good,
        tonnes: f64,
        seed: u64,
    ) {
        self.fields.entry(id).or_default().nuggets.push(Nugget {
            pos,
            vel,
            tonnes,
            good,
            seed,
            age: 0.0,
        });
    }

    /// Leaving a field: loose ore is lost to the dark.
    pub fn leave(&mut self, id: FieldId) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.nuggets.clear();
        }
    }

    /// Every rock standing in a field, as of the galaxy clock, with its
    /// true shape: for the ship and loose ore to bump into.
    pub fn solids<'a>(
        &'a self,
        id: FieldId,
        rocks: &'a [Rock],
        clock_s: f64,
    ) -> impl Iterator<Item = Solid> + 'a {
        let state = self.fields.get(&id);
        let base = rocks.iter().enumerate().filter_map(move |(i, rock)| {
            if state.is_some_and(|s| s.broken.contains_key(&i)) {
                return None;
            }
            Some(Solid {
                pos: rock_offset(rock),
                radius: rock.radius_m,
                rot: spin(DVec3::from_array(rock.spin_axis), rock.spin_rate, clock_s),
                seed: rock.seed,
                base: true,
            })
        });
        let frags = state.into_iter().flat_map(move |s| {
            s.frags.iter().map(move |f| Solid {
                pos: f.pos,
                radius: f.radius,
                rot: spin(f.spin_axis, f.spin_rate, clock_s),
                seed: f.seed,
                base: false,
            })
        });
        base.chain(frags)
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
            resources: Resources {
                ore: 0.8,
                ice: 0.1,
                gas: 0.0,
            },
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
    fn holding_the_trigger_botches_then_grinds_through() {
        // #212: a new player who just holds the laser on a rock tops its
        // stress out in seconds and shatters the vein; the poor rock left
        // still breaks, for little.
        let mut m = Mining::default();
        let rocks = [rock(20.0)];
        let mut clock = 0.0;
        let outcomes = run(&mut m, &rocks, miner(150.0, true), 60.0, &mut clock);
        let broke: Vec<Break> = outcomes.iter().filter_map(|o| o.broke).collect();
        assert_eq!(broke.first(), Some(&Break::Botched), "{broke:?}");
        assert!(
            m.is_broken(fid(), 0),
            "20 m rock still whole after a minute"
        );
        let state = m.field(fid()).unwrap();
        assert!(!state.frags.is_empty() || !state.nuggets.is_empty());
    }

    /// Tonnes a fracture banner says it gave: the number before " t ".
    fn banner_t(msg: &str) -> Option<f64> {
        let at = msg.find(" t ")?;
        msg[..at].rsplit(' ').next()?.parse().ok()
    }

    /// How a pilot works the trigger.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Hand {
        /// Holds it down.
        Hold,
        /// Feathers on the needle: lets off near the band's top, but
        /// doesn't heed pockets.
        Needle,
        /// Feathers and lets off for pockets too.
        Practised,
    }

    fn trigger(m: &Mining, hand: Hand) -> bool {
        let Some(c) = m.cut else { return true };
        match hand {
            Hand::Hold => true,
            Hand::Needle => c.stress < c.temper.hi - 0.06,
            Hand::Practised => c.pocket.is_none() && c.stress < c.temper.hi - 0.06,
        }
    }

    /// One rock, worked on a vein by `hand` until it breaks or `limit`
    /// seconds pass: how it broke, how long it took, tonnes shed.
    fn work_rock(radius: f64, seed: u64, hand: Hand, limit: f64) -> (Vec<Break>, f64, f64) {
        work_rock_at(radius, seed, hand, limit, true)
    }

    /// The same, on a vein or off them all.
    fn work_rock_at(
        radius: f64,
        seed: u64,
        hand: Hand,
        limit: f64,
        vein: bool,
    ) -> (Vec<Break>, f64, f64) {
        let mut rocks = [rock(radius)];
        rocks[0].seed = seed;
        let f = field();
        let stats = courier();
        let mut m = Mining {
            rng: Rng::new(seed),
            ..Default::default()
        };
        let live = m.live_rocks(fid(), &f, &rocks, 0.0).next().unwrap();
        let spot = if vein {
            live.vein_points().next().expect("every rock has a vein")
        } else {
            // The surface point farthest from every vein.
            (0..200)
                .map(|i| {
                    let d = random_dir(&mut Rng::new(i));
                    live.pos + d * surface_along(live.seed, live.rot, live.radius, d)
                })
                .max_by(|a, b| {
                    let gap = |p: &DVec3| {
                        live.vein_points()
                            .map(|v| (v - *p).length())
                            .fold(f64::INFINITY, f64::min)
                    };
                    gap(a).total_cmp(&gap(b))
                })
                .unwrap()
        };
        let from = spot + (spot - live.pos).normalize() * 100.0;
        let dir = (spot - from).normalize();
        let ship = |firing| Miner {
            nose: from,
            forward: dir,
            centre: from,
            reach: 10.0,
            free_t: 1000.0,
            firing,
        };
        let (dt, mut t, mut breaks, mut shed) = (1.0 / 30.0, 0.0, Vec::new(), 0.0);
        while t < limit && !m.is_broken(fid(), 0) {
            t += dt;
            let fire = trigger(&m, hand);
            let o = m.update(dt, fid(), &f, &rocks, t, &stats, ship(fire));
            if let Some(b) = o.broke {
                breaks.push(b);
            }
            shed += o.messages.iter().filter_map(|s| banner_t(s)).sum::<f64>();
        }
        (breaks, t, shed)
    }

    #[test]
    fn a_rock_is_a_minute_of_feathering_or_less() {
        // #212, DESIGN §7.3: each rock a small job lasting a minute or
        // less, for a pilot reading its needle.
        for radius in [20.0, 40.0, 60.0] {
            let mut times = Vec::new();
            for seed in 0..20 {
                let (breaks, t, _) = work_rock(radius, seed, Hand::Practised, 120.0);
                assert!(!breaks.is_empty(), "{radius} m seed {seed}: unbroken");
                times.push(t);
            }
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            let worst = times.iter().copied().fold(0.0, f64::max);
            println!("{radius} m: mean {mean:.1} s, worst {worst:.1} s");
            assert!(mean <= 45.0, "{radius} m: mean {mean:.1} s");
            assert!(worst <= 75.0, "{radius} m: worst {worst:.1} s");
        }
    }

    #[test]
    fn practice_beats_holding_and_pays_jackpots() {
        // #212: a practised hand mines better than a new one, and the
        // same rocks can go clean, botched or jackpot by what the pilot
        // did.
        let mut tally = HashMap::new();
        for hand in [Hand::Hold, Hand::Needle, Hand::Practised] {
            let (mut tonnes, mut secs) = (0.0, 0.0);
            let mut kinds: HashMap<String, usize> = HashMap::new();
            for seed in 0..40 {
                let (breaks, t, shed) = work_rock(30.0, seed, hand, 180.0);
                tonnes += shed;
                secs += t;
                if let Some(last) = breaks.last() {
                    let botched = breaks.contains(&Break::Botched);
                    let key = if botched {
                        "Botched".into()
                    } else {
                        format!("{last:?}")
                    };
                    *kinds.entry(key).or_default() += 1;
                }
            }
            println!("{hand:?}: {:.2} t/min, {kinds:?}", tonnes / secs * 60.0);
            tally.insert(format!("{hand:?}"), (tonnes / secs, kinds));
        }
        let rate = |h: &str| tally[h].0;
        assert!(rate("Practised") > rate("Needle") && rate("Needle") > rate("Hold"));
        assert!(rate("Practised") > 1.5 * rate("Hold"));
        let practised = &tally["Practised"].1;
        assert!(
            practised.get("Jackpot").copied().unwrap_or(0) > 0,
            "{practised:?}"
        );
        // Off the vein the same hand cracks slower and never frees a core.
        let (mut tonnes, mut secs) = (0.0, 0.0);
        for seed in 0..40 {
            let (breaks, t, shed) = work_rock_at(30.0, seed, Hand::Practised, 180.0, false);
            assert!(!breaks.contains(&Break::Jackpot), "seed {seed}: {breaks:?}");
            tonnes += shed;
            secs += t;
        }
        println!("Practised off the vein: {:.2} t/min", tonnes / secs * 60.0);
        assert!(tonnes / secs < 0.6 * rate("Practised"));
        assert!(tally["Hold"].1.get("Botched").copied().unwrap_or(0) >= 35);
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
        // Cold rock, below its band: the laser only warms it.
        let want = laser_power(&stats) * VEIN_CUT * COLD_CUT * 0.1;
        assert!((on_vein - want).abs() < 1e-9, "{on_vein} vs {want}");
    }

    #[test]
    fn the_tractor_fills_the_hold_and_no_more() {
        let mut m = Mining::default();
        let rocks = [rock(30.0)];
        let mut clock = 0.0;
        let mut collected = 0.0;
        // Less than any 30 m rock sheds (grade ≥ 0.15 gives 1.5 t; a
        // botch's poor rock a third of that, its fines a tenth).
        let mut ship = miner(150.0, true);
        ship.free_t = 0.4;
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
        assert!((collected - 0.4).abs() < 1e-6, "collected {collected} t");
        assert!((m.haul[Good::Ore] + m.haul[Good::Ice] - 0.4).abs() < 1e-6);
        let left: f64 = m
            .field(fid())
            .unwrap()
            .nuggets
            .iter()
            .map(|n| n.tonnes)
            .sum();
        assert!(left > 0.05, "the rest is still floating there: {left} t");
    }

    #[test]
    fn quarries_calve_until_worked_out() {
        let mut m = Mining::default();
        let rocks = [rock(100.0)];
        let mut clock = 0.0;
        let f = field();
        let mut boulders = 0;
        let (stats, dt) = (courier(), 1.0 / 30.0);
        for _ in 0..30 * 2000 {
            clock += dt;
            let ship = miner(300.0, trigger(&m, Hand::Practised));
            m.update(dt, fid(), &f, &rocks, clock, &stats, ship);
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

    /// #155: a calved quarry's ore lands where a ship parked against the
    /// rock, as close as it can get, pulls it all into the hold; and none
    /// of it rests inside rock. Every rock shape, from every side.
    #[test]
    fn a_quarry_s_ore_is_in_reach_of_a_ship_against_it() {
        let f = field();
        let dirs = [
            DVec3::X,
            DVec3::NEG_X,
            DVec3::Y,
            DVec3::NEG_Y,
            DVec3::Z,
            DVec3::NEG_Z,
        ];
        for mesh in 0..ROCK_MESHES as u64 {
            for dir in dirs {
                let mut rocks = [rock(300.0)];
                rocks[0].seed = mesh;
                let mut m = Mining::default();
                let solid = m.solids(fid(), &rocks, 0.0).next().unwrap();
                // Where the ship's collision stops it (pilot.rs).
                let centre = dir * (solid.surface_toward(dir) + 8.0);
                let mut ship = Miner {
                    nose: centre - dir * 6.0,
                    forward: -dir,
                    centre,
                    reach: 10.0,
                    free_t: 100.0,
                    firing: true,
                };
                let mut clock = 0.0;
                let mut shed = 0.0;
                let mut collected = 0.0;
                for _ in 0..30 * 900 {
                    clock += 1.0 / 30.0;
                    if shed == 0.0 {
                        ship.firing = trigger(&m, Hand::Practised);
                    }
                    let o = m.update(1.0 / 30.0, fid(), &f, &rocks, clock, &courier(), ship);
                    // The calving banner, clean, rough or jackpot (a
                    // botch's banner gives no tonnes: its fines are few).
                    for msg in &o.messages {
                        if let Some(t) = banner_t(msg) {
                            shed = t;
                            ship.firing = false;
                        }
                    }
                    collected += o.collected.iter().map(|(_, t)| t).sum::<f64>();
                    if shed > 0.0 {
                        let state = m.field(fid()).unwrap();
                        let solids: Vec<Solid> = m.solids(fid(), &rocks, clock).collect();
                        // Ore the tractor has hold of comes straight in,
                        // through anything; the rest must lie on rock.
                        let loose = state
                            .nuggets
                            .iter()
                            .filter(|n| (n.pos - ship.centre).length() >= TRACTOR_RANGE);
                        for n in loose {
                            for s in &solids {
                                assert!(
                                    (n.pos - s.pos).length() >= s.surface_toward(n.pos),
                                    "mesh {mesh} from {dir}: ore inside rock at {:?}",
                                    n.pos
                                );
                            }
                        }
                        if state.nuggets.is_empty() {
                            break;
                        }
                    }
                }
                assert!(shed > 0.0, "mesh {mesh} from {dir}: never calved");
                // The banner rounds to a tenth of a tonne.
                assert!(
                    (collected - shed).abs() < 0.06,
                    "mesh {mesh} from {dir}: shed {shed} t, hold got {collected} t"
                );
            }
        }
    }

    #[test]
    fn rock_surfaces_are_where_the_laser_finds_them() {
        // The collision surface and the laser's agree, for every shape.
        let m = Mining::default();
        for mesh in 0..ROCK_MESHES as u64 {
            let mut rocks = [rock(40.0)];
            rocks[0].seed = mesh;
            let solid = m.solids(fid(), &rocks, 0.0).next().unwrap();
            for dir in [
                DVec3::X,
                DVec3::NEG_Y,
                DVec3::new(0.3, -0.5, 0.8).normalize(),
            ] {
                let aim = m
                    .cast(fid(), &field(), &rocks, 0.0, dir * 300.0, -dir, 600.0)
                    .unwrap();
                let laser = aim.point.length();
                let surface = solid.surface_toward(dir);
                assert!(
                    (laser - surface).abs() < 0.5,
                    "mesh {mesh}: {laser} vs {surface}"
                );
            }
        }
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
