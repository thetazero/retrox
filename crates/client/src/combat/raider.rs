//! The other ships in a fight, and how they fly and fight.

use glam::DVec3;
use retrox_gen::Rng;
use retrox_sim::FactionId;
use retrox_sim::bubble::FleetKey;

use super::geom::{aim, lead_point, scatter, stick_toward};
use super::{Craft, GIMBAL, Gun, LEASH, Vessel};
use crate::world::{STRAFE_SPEED, assist, handling, steer};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tactic {
    /// Closing on the target.
    Attack,
    /// Overshot: flying out along a direction before coming round again.
    Extend(DVec3),
    /// Badly hurt: running.
    Flee,
    /// The target left: heading back to the field.
    Return,
}

/// Whose a ship in the fight is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Crew {
    /// One of the gang's.
    Pirate,
    /// One of a trader fleet's, flying for `faction`.
    Trader { fleet: FleetKey, faction: FactionId },
}

/// Ids from here up are traders' ships; below, the gang's.
pub const TRADER_IDS: u32 = 1000;

/// A ship in the fight other than the player's: a pirate, or a trader
/// caught up in it.
#[derive(Clone, Debug)]
pub struct Raider {
    pub id: u32,
    pub vessel: Vessel,
    pub craft: Craft,
    /// How hard its engines burn, 0..1.
    pub burn: f64,
    pub tactic: Tactic,
    pub(super) timer: f64,
    /// 0..1: aim, nerve and reactions.
    pub(super) skill: f64,
    pub(super) rng: Rng,
    /// Evasive strafe and lift, and how long until it changes.
    pub(super) jink: DVec3,
    pub(super) jink_t: f64,
    /// Seconds left in the current burst of fire (negative: pause).
    pub(super) burst: f64,
    /// What it guards and comes back to, anchor-relative.
    pub home: DVec3,
    pub crew: Crew,
    /// Fighting the player. A peaceful ship holds its place in its convoy.
    pub hostile: bool,
    /// A peaceful trader's place in its convoy: where, and how fast.
    pub slot: Option<(DVec3, DVec3)>,
}

impl Raider {
    /// A hostile ship at `craft`, attacking, guarding `home`. Its nerve is
    /// drawn from `skill` (a range) and its own dice from `rng`.
    pub(super) fn new(
        id: u32,
        vessel: Vessel,
        craft: Craft,
        crew: Crew,
        home: DVec3,
        skill: (f64, f64),
        rng: &mut Rng,
    ) -> Self {
        let skill = rng.range(skill.0, skill.1);
        let own = Rng::new(rng.next_u64());
        Self {
            id,
            vessel,
            craft,
            burn: 0.0,
            tactic: Tactic::Attack,
            timer: 0.0,
            skill,
            rng: own,
            jink: DVec3::ZERO,
            jink_t: 0.0,
            burst: rng.range(-1.0, 0.5),
            home,
            crew,
            hostile: true,
            slot: None,
        }
    }

    pub fn trader_fleet(&self) -> Option<FleetKey> {
        match self.crew {
            Crew::Trader { fleet, .. } => Some(fleet),
            Crew::Pirate => None,
        }
    }

    /// Being shot rattles it: it jinks again soon.
    pub(super) fn rattle(&mut self) {
        self.jink_t = self.jink_t.min(0.2);
    }

    /// One frame of flying and fighting toward `target` (the player, if
    /// alive). Returns the guns it fires: the gun, its muzzle, and which
    /// way.
    pub(super) fn think(
        &mut self,
        dt: f64,
        target: Option<Craft>,
        solids: &[(DVec3, f64)],
    ) -> Vec<(Gun, DVec3, DVec3)> {
        let c = self.craft;
        let range = self.vessel.gun_range().max(1500.0);
        let mut fire = false;
        let mut aim_at = None;
        // (direction to fly, speed, strafe/lift)
        let (dir, speed) = match (self.tactic, target) {
            (_, None) => {
                self.tactic = Tactic::Return;
                (self.home - c.pos, 120.0)
            }
            (Tactic::Return, Some(_)) if self.crew != Crew::Pirate => {
                // A trader that has lost the player keeps running.
                self.tactic = Tactic::Flee;
                (c.vel, 600.0)
            }
            (Tactic::Return, Some(t)) => {
                if (t.pos - self.home).length() < LEASH * 0.8 {
                    self.tactic = Tactic::Attack;
                }
                if (c.pos - self.home).length() < 3000.0 {
                    (t.pos - c.pos, 60.0)
                } else {
                    (self.home - c.pos, 350.0)
                }
            }
            (Tactic::Flee, Some(t)) => (c.pos - t.pos, 600.0),
            (Tactic::Extend(d), Some(_)) => {
                self.timer -= dt;
                if self.timer <= 0.0 {
                    self.tactic = Tactic::Attack;
                }
                (d, 480.0)
            }
            (Tactic::Attack, Some(t)) => {
                let to = t.pos - c.pos;
                let dist = to.length();
                if (t.pos - self.home).length() > LEASH {
                    self.tactic = Tactic::Return;
                } else if self.vessel.hull_left() < 0.3 && self.rng.chance(dt * (1.2 - self.skill))
                {
                    self.tactic = Tactic::Flee;
                } else if dist < 260.0 + self.vessel.half_length() * 4.0 {
                    // Overshot: break away to one side and come round.
                    let side = to.cross(DVec3::Y).normalize_or(DVec3::X)
                        * if self.rng.chance(0.5) { 1.0 } else { -1.0 };
                    let out = (-to.normalize_or(DVec3::Z) * 0.4
                        + side
                        + DVec3::Y * self.rng.range(-0.5, 0.5))
                    .normalize_or(DVec3::X);
                    self.tactic = Tactic::Extend(out);
                    self.timer = self.rng.range(2.0, 3.5);
                }
                let lead = lead_point(c.pos, c.vel, t.pos, t.vel, self.vessel.shot_speed());
                aim_at = Some(lead);
                let along = t.vel.dot(to.normalize_or(DVec3::Z));
                let speed = if dist > 2500.0 {
                    480.0
                } else {
                    (along + 60.0 + dist * 0.05).clamp(140.0, 450.0)
                };
                let off = c
                    .forward()
                    .angle_between((lead - c.pos).normalize_or(DVec3::Z));
                let cone = 0.035 + (1.0 - self.skill) * 0.04;
                fire = dist < range * 0.92 && off < cone;
                (lead - c.pos, speed)
            }
        };

        // Rocks ahead: steer round them.
        let mut dir = dir.normalize_or(c.forward());
        let look = c.vel.length() * 1.5 + 150.0;
        for &(p, r) in solids {
            let to = p - c.pos;
            let along = to.dot(c.forward());
            if along < 0.0 || along > look + r {
                continue;
            }
            let off = to - c.forward() * along;
            let clear = r + self.vessel.half_length() + 30.0;
            if off.length() < clear {
                dir = (dir - off.normalize_or(DVec3::Y) * 1.5).normalize_or(dir);
            }
        }

        // Jink when someone is on our tail.
        self.jink_t -= dt;
        if self.jink_t <= 0.0 {
            self.jink_t = self.rng.range(0.5, 1.3);
            let threatened = target.is_some_and(|t| {
                let from = c.pos - t.pos;
                from.length() < 2500.0
                    && t.forward().angle_between(from.normalize_or(DVec3::Z)) < 0.35
            });
            self.jink = if threatened {
                DVec3::new(
                    if self.rng.chance(0.5) { 1.0 } else { -1.0 },
                    self.rng.range(-1.0, 1.0),
                    0.0,
                )
            } else {
                DVec3::ZERO
            };
        }

        let stick = stick_toward(c.rot, dir);
        let (thrust, turn) = handling(&self.vessel.stats);
        steer(&mut self.craft.rot, &mut self.craft.spin, stick, turn, dt);
        let local = DVec3::new(
            self.jink.x * STRAFE_SPEED,
            self.jink.y * STRAFE_SPEED,
            -speed,
        );
        self.burn = assist(
            &mut self.craft.vel,
            self.craft.rot,
            local,
            false,
            thrust,
            dt,
        );
        if self.vessel.stats.thrust_kn <= 0.0 {
            // No engines left: drifting.
            self.burn = 0.0;
        }
        self.craft.pos += self.craft.vel * dt;
        for &(p, r) in solids {
            let d = self.craft.pos - p;
            let clear = r + self.vessel.half_length();
            let len = d.length();
            if len < clear && len > 0.0 {
                let n = d / len;
                self.craft.pos += n * (clear - len);
                let into = self.craft.vel.dot(n);
                if into < 0.0 {
                    self.craft.vel -= n * into * 1.3;
                }
            }
        }

        // Bursts: fire for a moment, pause, fire again.
        self.burst -= dt;
        if self.burst < -self.rng.range(0.6, 1.4) {
            self.burst = self.rng.range(0.8, 1.6);
        }
        let trigger = fire && self.burst > 0.0;
        let fired = self.vessel.systems(dt, self.burn, trigger);
        let c = self.craft;
        let spread = 0.004 + (1.0 - self.skill) * 0.016;
        fired
            .into_iter()
            .map(|i| {
                let gun = self.vessel.guns[i].clone();
                let muzzle = c.pos + c.rot * gun.muzzle;
                let dir = aim(c.forward(), muzzle, aim_at, GIMBAL);
                let dir = scatter(&mut self.rng, dir, spread);
                (gun, muzzle, dir)
            })
            .collect()
    }
}
