//! Gas skimming (issue #68, DESIGN §7.3.1): the player's way to harvest a
//! gas giant, the same sources NPC skimmers work.
//!
//! **The loop.** Fast travel to a gas giant and you stop just above its
//! cloud deck. Dive into the skimming band, hold the trigger to open the
//! scoop, and the hold fills with gas as you fly. Sell it where gas plants
//! want it.
//!
//! **The skill.** The air thickens as you go down: [`density`] grows by a
//! factor of *e* every [`SCALE_HEIGHT_M`]. The scoop takes in gas as
//! density × speed, but friction heats it as density × speed², so deep and
//! slow beats high and fast, and deep and fast cooks the scoop in seconds.
//! Down near the deck the gas is turbulent and shoves the ship about; touch
//! the deck and you bounce off it. Radiators shed the heat, as they do the
//! mining laser's; let it reach the top and the scoop shuts while it vents.

use glam::DVec3;
use retrox_sim::BodyId;
use retrox_sim::ship::ShipStats;

/// Density falls by a factor of e every this many metres up.
pub const SCALE_HEIGHT_M: f64 = 20_000.0;
/// The top of the skimming band, metres above the cloud deck.
pub const BAND_M: f64 = 120_000.0;
/// Fast travel to a gas giant stops this far above its cloud deck.
pub const ARRIVAL_ALTITUDE_M: f64 = 160_000.0;
/// Tonnes a second the scoop takes in, per unit of density, richness,
/// metre a second and square root of hold tonnes.
const INTAKE: f64 = 4.2e-4;
/// Scoop heat a second at full density and [`HEAT_SPEED`], open.
const HEAT: f64 = 0.06;
const HEAT_SPEED: f64 = 300.0;
/// Shut, the scoop still warms this much from the hull's friction.
const HEAT_SHUT: f64 = 0.3;
/// A vented scoop opens again once it has cooled to this.
const UNLOCK_HEAT: f64 = 0.2;
/// Drag: the share of speed through the gas lost a second at full density.
const DRAG: f64 = 0.03;
/// Turbulent shoves at full density and [`HEAT_SPEED`], m/s².
const TURBULENCE: f64 = 12.0;

/// How dense the gas is at `altitude_m` above the cloud deck: 1 at the
/// deck, nothing above the band.
pub fn density(altitude_m: f64) -> f64 {
    if altitude_m > BAND_M {
        0.0
    } else {
        (-altitude_m.max(0.0) / SCALE_HEIGHT_M).exp()
    }
}

/// The ship near a gas giant, as skimming sees it.
#[derive(Clone, Copy, Debug)]
pub struct Pass {
    pub giant: BodyId,
    /// Metres above the cloud deck.
    pub altitude_m: f64,
    /// Velocity through the gas (the giant's frame), m/s.
    pub vel: DVec3,
    /// Up, away from the giant's centre.
    pub up: DVec3,
    /// 0..1: how much gas there is to take.
    pub richness: f64,
    /// Free space in the hold, tonnes.
    pub free_t: f64,
}

/// What a frame of skimming did, for the world to apply.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// Tonnes of gas scooped.
    pub gas_t: f64,
    /// Change to the ship's velocity: drag and turbulence, m/s.
    pub kick: DVec3,
    pub message: Option<String>,
}

/// The scoop: its heat and what it's doing this frame. Only the moment;
/// nothing here is saved (the haul is, with mining's).
#[derive(Clone, Debug, Default)]
pub struct Skim {
    /// 0..1; shut at 1 until it cools to [`UNLOCK_HEAT`].
    pub heat: f64,
    pub locked: bool,
    /// The scoop is open and taking in gas.
    pub open: bool,
    /// Tonnes a second coming in.
    pub rate: f64,
    /// The pass this frame, if the ship is near a giant's cloud deck.
    pub pass: Option<Pass>,
    /// Seconds into the run, for the turbulence's phase.
    t: f64,
    told_band: bool,
    told_full: bool,
}

/// Scoop cooling a second: radiators shed its heat, as they do a laser's.
fn cooling(stats: &ShipStats) -> f64 {
    crate::mining::laser_cooling(stats)
}

impl Skim {
    /// One frame. `pass` is the ship near a giant (or `None`), `trigger`
    /// whether the pilot holds the scoop open.
    pub fn update(
        &mut self,
        dt: f64,
        stats: &ShipStats,
        pass: Option<Pass>,
        trigger: bool,
    ) -> Outcome {
        let mut out = Outcome::default();
        self.t += dt;
        self.pass = pass;
        let cool = cooling(stats);
        let Some(p) = pass else {
            self.open = false;
            self.rate = 0.0;
            self.heat = (self.heat - cool * dt).max(0.0);
            self.locked &= self.heat > UNLOCK_HEAT;
            self.told_band = false;
            return out;
        };
        let rho = density(p.altitude_m);
        let speed = p.vel.length();
        if rho > 0.0 && !self.told_band {
            self.told_band = true;
            out.message = Some("In the skimming band: hold the trigger to scoop gas".into());
        }
        self.open = trigger && !self.locked && rho > 0.0 && p.free_t > 0.0;
        if trigger && p.free_t <= 0.0 && !self.told_full {
            self.told_full = true;
            out.message = Some("Hold full: sell the gas where gas plants want it".into());
        }
        if p.free_t > 0.0 {
            self.told_full = false;
        }
        let friction = HEAT * rho * (speed / HEAT_SPEED).powi(2);
        let heating = if self.open {
            friction
        } else {
            friction * HEAT_SHUT
        };
        self.heat = (self.heat + (heating - cool) * dt).clamp(0.0, 1.0);
        if self.heat >= 1.0 && !self.locked {
            self.locked = true;
            self.open = false;
            out.message = Some("Scoop overheated: shut to vent. Climb or slow down".into());
        }
        if self.locked && self.heat <= UNLOCK_HEAT {
            self.locked = false;
        }
        self.rate = if self.open {
            INTAKE * rho * p.richness * speed * stats.cargo_t.max(1.0).sqrt()
        } else {
            0.0
        };
        out.gas_t = (self.rate * dt).min(p.free_t);
        // The gas drags and, low down, shoves.
        out.kick = -p.vel * (DRAG * rho * dt).min(1.0);
        let shove = TURBULENCE * rho * (speed / HEAT_SPEED).min(3.0);
        let wobble =
            (self.t * 1.3).sin() + 0.6 * (self.t * 3.7 + 1.0).sin() + 0.3 * (self.t * 9.1).sin();
        out.kick += p.up * shove * wobble * dt;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> ShipStats {
        retrox_content::ships::stock(crate::hangar::STARTING_HULL)
            .expect("starting hull")
            .stats()
    }

    fn pass(altitude_m: f64, speed: f64) -> Pass {
        Pass {
            giant: BodyId::from_index(0),
            altitude_m,
            vel: DVec3::X * speed,
            up: DVec3::Y,
            richness: 0.7,
            free_t: 100.0,
        }
    }

    #[test]
    fn deeper_fills_faster() {
        let stats = stats();
        let take = |alt| {
            let mut s = Skim::default();
            s.update(1.0, &stats, Some(pass(alt, 300.0)), true).gas_t
        };
        assert!(take(5_000.0) > take(40_000.0) * 2.0);
        assert_eq!(take(BAND_M + 1.0), 0.0);
    }

    #[test]
    fn deep_and_fast_overheats_and_shuts_the_scoop() {
        let stats = stats();
        let mut s = Skim::default();
        let mut shut_at = None;
        for i in 0..600 {
            s.update(0.1, &stats, Some(pass(0.0, 900.0)), true);
            if s.locked {
                shut_at = Some(i);
                break;
            }
        }
        let i = shut_at.expect("never overheated");
        assert!(i < 200, "took {} s", f64::from(i) / 10.0);
        assert!(!s.open);
        // Climbing out, it cools and opens again.
        for _ in 0..600 {
            s.update(0.1, &stats, None, true);
        }
        assert!(!s.locked);
    }

    #[test]
    fn a_steady_line_keeps_the_scoop_open() {
        let stats = stats();
        let mut s = Skim::default();
        for _ in 0..1200 {
            s.update(0.1, &stats, Some(pass(15_000.0, 300.0)), true);
        }
        assert!(s.open && !s.locked, "heat {}", s.heat);
    }

    #[test]
    fn a_full_hold_takes_no_more() {
        let stats = stats();
        let mut s = Skim::default();
        let mut p = pass(0.0, 300.0);
        p.free_t = 0.05;
        let out = s.update(1.0, &stats, Some(p), true);
        assert!(out.gas_t <= 0.05 + 1e-12);
    }
}
