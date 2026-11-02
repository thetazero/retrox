//! What a fight looks like: pulses, flashes, fireballs, debris and wrecks.

use std::f64::consts::TAU;

use glam::{DQuat, DVec3};
use retrox_sim::ship::Loadout;

use super::geom::random_dir;
use super::{Combat, Craft, Side};

/// How long a wreck drifts before it's lost from view, seconds.
const WRECK_LIFE: f64 = 180.0;

/// How long a laser pulse shows, seconds.
pub const PULSE_LIFE: f64 = 0.09;

/// A laser pulse, drawn for a moment.
#[derive(Clone, Copy, Debug)]
pub struct Pulse {
    pub from: DVec3,
    pub to: DVec3,
    pub age: f64,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlashKind {
    Muzzle,
    /// A shell or pulse striking bare hull.
    Spark,
    /// A hit on a shield: the bubble lights round `dir` (ship space).
    Shield {
        ship: Side,
        dir: DVec3,
    },
}

/// A short-lived point of light.
#[derive(Clone, Copy, Debug)]
pub struct Flash {
    pub pos: DVec3,
    pub vel: DVec3,
    pub kind: FlashKind,
    pub size: f64,
    pub age: f64,
    pub life: f64,
}

/// An explosion's fireball.
#[derive(Clone, Copy, Debug)]
pub struct Blast {
    pub pos: DVec3,
    pub vel: DVec3,
    pub radius: f64,
    /// Starts negative for a delayed secondary.
    pub age: f64,
    pub life: f64,
    pub seed: f64,
    /// Throws a shockwave ring (the main blast only).
    pub ring: bool,
}

/// A piece of a ship flung off: a plate of hull, or a glowing ember.
#[derive(Clone, Copy, Debug)]
pub struct Debris {
    pub pos: DVec3,
    pub vel: DVec3,
    pub axis: DVec3,
    pub rate: f64,
    pub size: DVec3,
    pub age: f64,
    pub life: f64,
    pub ember: bool,
}

impl Debris {
    pub fn rot(&self) -> DQuat {
        DQuat::from_axis_angle(self.axis, self.rate * self.age % TAU)
    }
}

/// What's left of a destroyed ship, tumbling.
#[derive(Clone, Debug)]
pub struct Wreck {
    pub fit: Loadout,
    pub pos: DVec3,
    pub vel: DVec3,
    pub rot0: DQuat,
    pub axis: DVec3,
    pub rate: f64,
    pub age: f64,
}

impl Wreck {
    pub fn rot(&self) -> DQuat {
        DQuat::from_axis_angle(self.axis, self.rate * self.age % TAU) * self.rot0
    }
}

impl Combat {
    pub(super) fn age_effects(&mut self, dt: f64) {
        for p in &mut self.pulses {
            p.age += dt;
        }
        self.pulses.retain(|p| p.age < PULSE_LIFE);
        for f in &mut self.flashes {
            f.age += dt;
            f.pos += f.vel * dt;
        }
        self.flashes.retain(|f| f.age < f.life);
        for b in &mut self.blasts {
            b.age += dt;
            b.pos += b.vel * dt;
        }
        self.blasts.retain(|b| b.age < b.life);
        for d in &mut self.debris {
            d.age += dt;
            d.pos += d.vel * dt;
        }
        self.debris.retain(|d| d.age < d.life);
        for w in &mut self.wrecks {
            w.age += dt;
            w.pos += w.vel * dt;
        }
        self.wrecks.retain(|w| w.age < WRECK_LIFE);
    }

    pub(super) fn embers(&mut self, at: DVec3, vel: DVec3, n: usize, speed: f64, life: f64) {
        for _ in 0..n {
            let d = random_dir(&mut self.rng);
            self.debris.push(Debris {
                pos: at,
                vel: vel + d * speed * self.rng.range(0.3, 1.0),
                axis: DVec3::Y,
                rate: 0.0,
                size: DVec3::splat(self.rng.range(0.12, 0.3)),
                age: 0.0,
                life: life * self.rng.range(0.6, 1.4),
                ember: true,
            });
        }
    }

    /// The player's ship goes up, whatever its hull has left (for
    /// screenshots).
    pub fn lose_player(&mut self, craft: Craft) {
        let fit = self.player.fit.clone();
        let length = self.player.stats.length_m;
        self.explode(&fit, craft, length);
        self.lost = Some(0.0);
    }

    /// A ship blows up: a fireball and its secondaries, a flash, plates
    /// and embers flung out, and the wreck tumbling on.
    pub(super) fn explode(&mut self, fit: &Loadout, craft: Craft, length: f64) {
        self.tally.blasts += 1;
        let r = (length * 1.6).max(14.0);
        let seed = self.rng.range(0.0, 100.0);
        self.blasts.push(Blast {
            pos: craft.pos,
            vel: craft.vel,
            radius: r * 1.1,
            age: 0.0,
            life: 1.8,
            seed,
            ring: true,
        });
        for k in 0..3 {
            let off = random_dir(&mut self.rng) * r * 0.4;
            self.blasts.push(Blast {
                pos: craft.pos + off,
                vel: craft.vel + off * 0.5,
                radius: r * self.rng.range(0.4, 0.7),
                age: -0.15 - 0.2 * k as f64,
                life: 1.3,
                seed: seed + 10.0 + k as f64,
                ring: false,
            });
        }
        self.flashes.push(Flash {
            pos: craft.pos,
            vel: craft.vel,
            kind: FlashKind::Spark,
            size: r * 0.9,
            age: 0.0,
            life: 0.3,
        });
        for _ in 0..14 {
            let d = random_dir(&mut self.rng);
            let s = r * 0.08;
            self.debris.push(Debris {
                pos: craft.pos + d * r * 0.2,
                vel: craft.vel + d * self.rng.range(15.0, 70.0),
                axis: random_dir(&mut self.rng),
                rate: self.rng.range(1.0, 5.0),
                size: DVec3::new(
                    s * self.rng.range(0.5, 2.0),
                    s * self.rng.range(0.1, 0.3),
                    s * self.rng.range(0.6, 2.5),
                ),
                age: 0.0,
                life: self.rng.range(25.0, 50.0),
                ember: false,
            });
        }
        self.embers(craft.pos, craft.vel, 40, 120.0, 2.0);
        self.wrecks.push(Wreck {
            fit: fit.clone(),
            pos: craft.pos,
            vel: craft.vel * 0.4 + random_dir(&mut self.rng) * 5.0,
            rot0: craft.rot,
            axis: random_dir(&mut self.rng),
            rate: self.rng.range(0.1, 0.35),
            age: 0.0,
        });
    }
}
