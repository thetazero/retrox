//! A ship as a fight sees it: its sections, guns, gauges and damage.

use glam::DVec3;
use retrox_sim::ship::{Function, Loadout, ShipStats};
use serde::{Deserialize, Serialize};

use super::{SHELL_SPEED, SHIELD_DELAY};

/// Locked-out guns come back once heat falls to this share of capacity.
const UNLOCK_HEAT: f64 = 0.5;

/// Hull damage, per fitted section (nose to tail) and to the frame. The
/// player's is saved with their ship; shields and heat recover on their
/// own and aren't.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Damage {
    pub sections: Vec<f64>,
    pub frame: f64,
}

impl Damage {
    /// Structure points to repair.
    pub fn total(&self) -> f64 {
        self.sections.iter().sum::<f64>() + self.frame
    }
}

/// A fitted section, as a place hits land.
#[derive(Clone, Debug)]
pub struct Part {
    pub mount: usize,
    pub name: &'static str,
    /// Where it starts (toward the nose, -Z) and ends, in ship space.
    pub front: f64,
    pub back: f64,
    pub structure: f64,
}

/// A gun, where it sits and what it does.
#[derive(Clone, Debug)]
pub struct Gun {
    /// The muzzle, in ship space (nose toward -Z).
    pub muzzle: DVec3,
    pub damage: f64,
    pub rate: f64,
    pub range: f64,
    pub power_mw: f64,
    pub heat_mw: f64,
    pub laser: bool,
    cooldown: f64,
}

/// Power (MW) and heat (MW) of a group of running modules.
#[derive(Clone, Copy, Debug, Default)]
struct Draw {
    power: f64,
    heat: f64,
}

impl Draw {
    fn add(&mut self, power: f64, heat: f64) {
        self.power += power;
        self.heat += heat;
    }
}

/// What a hit did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Shield,
    Hull,
    /// A section was wrecked (by index into [`Vessel::parts`]).
    Wrecked(usize),
    Destroyed,
}

/// A ship as a fight sees it: its fit, what's left of it, and its gauges.
#[derive(Clone, Debug)]
pub struct Vessel {
    pub fit: Loadout,
    /// What still works: the fit without the modules of wrecked sections.
    pub stats: ShipStats,
    pub parts: Vec<Part>,
    pub frame_max: f64,
    pub damage: Damage,
    pub shield: f64,
    pub since_hit: f64,
    pub guns: Vec<Gun>,
    /// Heat in the sections, MJ, against `stats.heat_capacity_mj`.
    pub heat: f64,
    /// The guns are locked out, venting heat.
    pub locked: bool,
    /// Share of the guns' power they're getting, 0..1.
    pub gun_power: f64,
    /// The guns fired this frame.
    pub firing: bool,
    base: Draw,
    engines: Draw,
    shields: Draw,
}

impl Vessel {
    pub fn new(fit: Loadout, damage: Damage) -> Self {
        let mut parts = Vec::new();
        let total: f64 = fit
            .mounts()
            .filter_map(|(_, s)| s)
            .map(|s| s.section.length_m)
            .sum();
        let mut front = -total / 2.0;
        for (mount, (_, section)) in fit.mounts().enumerate() {
            let Some(s) = section else { continue };
            let len = s.section.length_m;
            parts.push(Part {
                mount,
                name: s.section.name,
                front,
                back: front + len,
                structure: s.section.structure,
            });
            front += len;
        }
        let mut damage = damage;
        damage.sections.resize(parts.len(), 0.0);
        let frame_max = fit.hull().frame_structure;
        let mut v = Self {
            stats: fit.stats(),
            fit,
            parts,
            frame_max,
            damage,
            shield: 0.0,
            since_hit: SHIELD_DELAY,
            guns: Vec::new(),
            heat: 0.0,
            locked: false,
            gun_power: 1.0,
            firing: false,
            base: Draw::default(),
            engines: Draw::default(),
            shields: Draw::default(),
        };
        v.refit();
        v.shield = v.stats.shield;
        v
    }

    pub fn wrecked(&self, part: usize) -> bool {
        let p = &self.parts[part];
        self.damage.sections[part] >= p.structure
    }

    /// Rebuild what works from the fit and the damage.
    fn refit(&mut self) {
        let mut live = self.fit.clone();
        for (i, part) in self.parts.iter().enumerate() {
            if self.damage.sections[i] >= part.structure {
                let slots = live
                    .mounts()
                    .nth(part.mount)
                    .and_then(|(_, s)| s)
                    .map_or(0, |s| s.slots().count());
                for slot in 0..slots {
                    live.remove(part.mount, slot);
                }
            }
        }
        self.stats = live.stats();
        self.shield = self.shield.min(self.stats.shield);
        let old: Vec<f64> = self.guns.iter().map(|g| g.cooldown).collect();
        self.guns.clear();
        self.base = Draw::default();
        self.engines = Draw::default();
        self.shields = Draw::default();
        let sections: Vec<_> = live.mounts().filter_map(|(_, s)| s).collect();
        for (part, section) in self.parts.iter().zip(sections) {
            let centre = (part.front + part.back) / 2.0;
            for (slot, module) in section.slots() {
                let Some(m) = module else { continue };
                let [x, y, z] = slot.position_m;
                match m.function {
                    Function::Weapon {
                        damage,
                        fire_rate_hz,
                        range_km,
                    } => self.guns.push(Gun {
                        muzzle: DVec3::new(x, y, centre - z - 0.8),
                        damage,
                        rate: fire_rate_hz,
                        range: range_km * 1000.0,
                        power_mw: m.power_mw,
                        heat_mw: m.heat_mw,
                        laser: m.id.contains("laser"),
                        cooldown: 0.0,
                    }),
                    Function::Engine { .. } => self.engines.add(m.power_mw, m.heat_mw),
                    Function::Shield { .. } => self.shields.add(m.power_mw, m.heat_mw),
                    // Mining lasers keep their own gauge; jump drives only
                    // draw in the lanes.
                    Function::MiningLaser { .. } | Function::JumpDrive { .. } => {}
                    _ => self.base.add(m.power_mw, m.heat_mw),
                }
            }
        }
        for (g, c) in self.guns.iter_mut().zip(old) {
            g.cooldown = c;
        }
    }

    /// Half the ship's length, metres.
    pub fn half_length(&self) -> f64 {
        (self.stats.length_m / 2.0).max(3.0)
    }

    /// The shield bubble's semi-axes (across, along), ship space.
    pub fn bubble(&self) -> (f64, f64) {
        let along = self.half_length() * 1.15 + 1.5;
        (along * 0.5, along)
    }

    /// The hull's semi-axes, roughly: a slim ellipsoid along the ship.
    pub(super) fn hull_shape(&self) -> (f64, f64) {
        let along = self.half_length();
        ((along * 0.32).max(1.6), along)
    }

    pub fn shield_max(&self) -> f64 {
        self.stats.shield
    }

    /// Structure left, 0..1, the way a pilot reads it: the section in the
    /// worst state, or the frame if that's worse.
    pub fn hull_left(&self) -> f64 {
        let worst = self
            .parts
            .iter()
            .zip(&self.damage.sections)
            .map(|(p, d)| 1.0 - d / p.structure.max(1.0))
            .fold(1.0, f64::min);
        let frame = if self.frame_max > 0.0 {
            1.0 - self.damage.frame / self.frame_max
        } else {
            1.0
        };
        worst.min(frame).max(0.0)
    }

    pub fn destroyed(&self) -> bool {
        if self.frame_max > 0.0 {
            self.damage.frame >= self.frame_max || (0..self.parts.len()).all(|i| self.wrecked(i))
        } else {
            (0..self.parts.len()).any(|i| self.wrecked(i))
        }
    }

    /// Take `amount` of damage at `local` (ship space).
    pub fn hit(&mut self, amount: f64, local: DVec3) -> Hit {
        self.since_hit = 0.0;
        let mut amount = amount;
        if self.shield > 0.0 {
            let absorbed = self.shield.min(amount);
            self.shield -= absorbed;
            amount -= absorbed;
            if amount <= 1e-9 {
                return Hit::Shield;
            }
        }
        if self.parts.is_empty() {
            return Hit::Hull;
        }
        let part = self
            .parts
            .iter()
            .position(|p| local.z < p.back)
            .unwrap_or(self.parts.len() - 1);
        let mut result = Hit::Hull;
        if self.wrecked(part) {
            self.damage.frame += amount;
        } else {
            let structure = self.parts[part].structure;
            self.damage.sections[part] += amount;
            let over = self.damage.sections[part] - structure;
            if over >= 0.0 {
                if self.frame_max > 0.0 {
                    self.damage.frame += over;
                }
                self.damage.sections[part] = structure;
                self.refit();
                result = Hit::Wrecked(part);
            }
        }
        if self.destroyed() {
            Hit::Destroyed
        } else {
            result
        }
    }

    /// Run the ship's systems for a frame: the reactor feeds base systems,
    /// then the engines (at `burn`, 0..1), then the shields, then the guns;
    /// heat builds and sheds. Returns which guns fire this frame.
    pub fn systems(&mut self, dt: f64, burn: f64, trigger: bool) -> Vec<usize> {
        self.since_hit += dt;
        let supply = self.stats.power_supply_mw;
        let mut left = supply - self.base.power - self.engines.power * burn;
        let charging = self.shield < self.stats.shield && self.since_hit >= SHIELD_DELAY;
        let mut shield_load = 0.0;
        if charging {
            let share = if self.shields.power > 0.0 {
                (left / self.shields.power).clamp(0.0, 1.0)
            } else {
                1.0
            };
            self.shield =
                (self.shield + self.stats.shield_regen_per_s * share * dt).min(self.stats.shield);
            left -= self.shields.power * share;
            shield_load = share;
        }
        let gun_draw: f64 = self.guns.iter().map(|g| g.power_mw).sum();
        self.gun_power = if gun_draw > 0.0 {
            (left / gun_draw).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let firing = trigger && !self.locked && !self.guns.is_empty() && self.gun_power > 0.05;
        let gun_heat: f64 = self.guns.iter().map(|g| g.heat_mw).sum::<f64>() * self.gun_power;
        let made = self.base.heat
            + self.engines.heat * burn
            + self.shields.heat * shield_load
            + if firing { gun_heat } else { 0.0 };
        let capacity = self.stats.heat_capacity_mj.max(1.0);
        self.heat = (self.heat + (made - self.stats.heat_dissipation_mw) * dt).clamp(0.0, capacity);
        if firing && self.heat >= capacity {
            self.locked = true;
        }
        if self.locked && self.heat <= capacity * UNLOCK_HEAT {
            self.locked = false;
        }
        // The volley that reaches capacity locks the guns before it fires.
        let firing = firing && !self.locked;
        self.firing = firing;
        let mut fired = Vec::new();
        for (i, g) in self.guns.iter_mut().enumerate() {
            g.cooldown = (g.cooldown - dt).max(-1.0);
            if !firing {
                g.cooldown = g.cooldown.max(0.0);
                continue;
            }
            if g.cooldown <= 0.0 {
                fired.push(i);
                g.cooldown += 1.0 / (g.rate * self.gun_power.max(0.05));
            }
        }
        fired
    }

    /// Heat as a share of capacity, 0..1.
    pub fn heat_share(&self) -> f64 {
        self.heat / self.stats.heat_capacity_mj.max(1.0)
    }

    /// The longest reach of its guns, metres.
    pub fn gun_range(&self) -> f64 {
        self.guns.iter().map(|g| g.range).fold(0.0, f64::max)
    }

    /// The speed its first gun's shells fly at (lasers: instant).
    pub(super) fn shot_speed(&self) -> Option<f64> {
        match self.guns.first() {
            Some(g) if g.laser => None,
            Some(_) => Some(SHELL_SPEED),
            None => Some(SHELL_SPEED),
        }
    }
}
