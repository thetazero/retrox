//! Combat (DESIGN §7.2): guns, shields and hulls, and the raiders who lurk
//! in asteroid fields.
//!
//! **Guns.** The trigger fires every gun fitted to a weapon slot (mining
//! lasers have their own gauge, and stand down while hostiles are near;
//! see [`crate::mining`]). Autocannons throw shells at [`SHELL_SPEED`] on top of the ship's own velocity; lasers hit
//! along their line at once. Fixed guns swing up to [`GIMBAL`] to meet the
//! target's lead, so a pilot who puts the nose on the lead pip hits.
//!
//! **Budgets** (DESIGN §7.4). Everything that runs draws power and makes
//! heat: engines in proportion to how hard they burn, shields while they
//! recharge, guns while they fire. The reactor feeds the rest first and the
//! guns last, so a fit that can't fire, shield and burn at once fires
//! slower while it does all three. Heat the radiators can't shed builds up
//! in the sections; at the top the guns lock out until the ship has shed
//! half of it.
//!
//! **Damage.** Shields take hits first and come back [`SHIELD_DELAY`]
//! seconds after the last one. Past them, a hit lands on the section it
//! struck, by where along the hull it hit (DESIGN §7.4). A section that
//! runs out of structure is wrecked and its modules die with it: lose the
//! stern and you lose the engines. A small ship is one section, so that is
//! the end of it; a framed ship's further hits there go to the frame, and
//! the ship is lost when the frame gives out or every section is gone.
//!
//! **Raiders.** Pirate bands lurk at asteroid fields, preying on miners,
//! away from faction capitals and more often where no one has settled.
//! Each field has a band or not for each [`BAND_DAYS`] stretch of the
//! calendar, decided from the field and the date (so a restored checkpoint
//! meets the same band). They wait until a ship comes, then close in.
//! Raiders fly the same arcade flight model the player does, with stock
//! fits, under the same budgets. Ships killed stay dead until the band
//! regroups. Until raiders are agents of the sim's bubble (DESIGN §5.2),
//! bands are kept here, like the mining laser's work on a field.
//!
//! Positions are relative to the fight's *anchor*: the field (or, outside
//! one, wherever the player's ship is framed). Metres, y up.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use glam::{DQuat, DVec3};
use retrox_gen::Rng;
use retrox_sim::economy::Good;
use retrox_sim::ship::{Function, Loadout, ShipStats};
use retrox_sim::{FieldId, GalaxyState, SystemId};
use serde::{Deserialize, Serialize};

use crate::world::{STRAFE_SPEED, assist, handling, look_rotation, steer};

/// Autocannon shells leave the muzzle this fast, m/s, on top of the
/// ship's velocity.
pub const SHELL_SPEED: f64 = 1600.0;
/// How far fixed guns swing to meet a target's lead, radians.
pub const GIMBAL: f64 = 0.07;
/// Hostiles this close keep the ship from fast travel, jumps and saving.
pub const HOSTILE_RANGE: f64 = 20e3;
/// Seconds after the last hit before shields start to come back.
pub const SHIELD_DELAY: f64 = 3.0;
/// Locked-out guns come back once heat falls to this share of capacity.
const UNLOCK_HEAT: f64 = 0.5;
/// Galaxy days a field's band lasts before another takes its place.
pub const BAND_DAYS: f64 = 20.0;
/// How long a wreck drifts before it's lost from view, seconds.
const WRECK_LIFE: f64 = 180.0;
/// Raiders give up the chase once the player is this far from their field.
const LEASH: f64 = 16e3;
/// Raiders spawn this far from the ship that comes to their field.
const AMBUSH_RANGE: f64 = 7000.0;
/// Credits per point of structure repaired.
pub const REPAIR_PER_POINT: f64 = 3.0;

/// Who fired, or who was hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Player,
    Raider(u32),
}

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
    fn hull_shape(&self) -> (f64, f64) {
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
        self.firing = firing;
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
        if self.locked {
            self.firing = false;
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
    fn shot_speed(&self) -> Option<f64> {
        match self.guns.first() {
            Some(g) if g.laser => None,
            Some(_) => Some(SHELL_SPEED),
            None => Some(SHELL_SPEED),
        }
    }
}

/// Where and how a ship moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Craft {
    pub pos: DVec3,
    pub vel: DVec3,
    pub rot: DQuat,
    pub spin: DVec3,
}

impl Craft {
    pub fn forward(&self) -> DVec3 {
        self.rot * DVec3::NEG_Z
    }
}

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

/// A pirate ship.
#[derive(Clone, Debug)]
pub struct Raider {
    pub id: u32,
    pub vessel: Vessel,
    pub craft: Craft,
    /// How hard its engines burn, 0..1.
    pub burn: f64,
    pub tactic: Tactic,
    timer: f64,
    /// 0..1: aim, nerve and reactions.
    skill: f64,
    rng: Rng,
    /// Evasive strafe and lift, and how long until it changes.
    jink: DVec3,
    jink_t: f64,
    /// Seconds left in the current burst of fire (negative: pause).
    burst: f64,
}

/// A shell in flight.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub pos: DVec3,
    pub vel: DVec3,
    pub life: f64,
    pub damage: f64,
    pub from: Side,
}

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

/// Which raiders of a field's band are dead, for one stretch of the
/// calendar. Saved with the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub epoch: u64,
    /// Bit `i` set: the band's ship `i` is dead.
    pub killed: u32,
}

/// The bands the player has fought, by field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bands(#[serde(with = "crate::save::pairs")] pub HashMap<FieldId, Band>);

/// Running counts of events (see [`Combat::tally`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub fired: u64,
    pub lasers: u64,
    pub enemy_fired: u64,
    pub shield_hits: u64,
    pub hull_hits: u64,
    pub blasts: u64,
}

/// The player's side, as the fight needs it this frame.
#[derive(Clone, Copy, Debug)]
pub struct Pilot {
    pub craft: Craft,
    pub burn: f64,
    pub trigger: bool,
}

/// What a frame of fighting did, for the world to apply.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub messages: Vec<String>,
    /// Loose cargo thrown from wrecks: where, how it drifts, what, tonnes.
    pub salvage: Vec<(DVec3, DVec3, Good, f64, u64)>,
    pub player_destroyed: bool,
}

/// Everything fighting near the player.
#[derive(Clone, Debug)]
pub struct Combat {
    /// The field whose band is out, if any.
    pub field: Option<FieldId>,
    pub raiders: Vec<Raider>,
    pub shots: Vec<Shot>,
    pub pulses: Vec<Pulse>,
    pub flashes: Vec<Flash>,
    pub blasts: Vec<Blast>,
    pub debris: Vec<Debris>,
    pub wrecks: Vec<Wreck>,
    /// The player's ship.
    pub player: Vessel,
    /// The raider the guns track, by id.
    pub target: Option<u32>,
    /// Seconds since the player's ship was destroyed, if it was.
    pub lost: Option<f64>,
    pub bands: Bands,
    /// Seconds since the player's guns last hit something, and since the
    /// player's shield and hull were last hit (for the HUD).
    pub since_scored: f64,
    pub since_shield_hit: f64,
    pub since_hull_hit: f64,
    /// Which way the last hit on the player came from (ship space).
    pub hit_from: DVec3,
    /// Running counts of what happened, for the sound to follow: the
    /// player's shells and pulses, everyone else's, hits on the player's
    /// shield and hull, and ships blown up.
    pub tally: Tally,
    /// Raiders of the field's band, by index, and the epoch they're from.
    band_epoch: u64,
    rng: Rng,
    /// Laser hits waiting to land this frame: who, how hard, where, which
    /// way.
    pending: Vec<(Side, f64, DVec3, DVec3)>,
    said: Vec<String>,
    dropped: Vec<(DVec3, DVec3, Good, f64, u64)>,
    /// Nothing has happened since [`Combat::new`]: the ship is as a
    /// loaded game has it.
    fresh: bool,
}

impl Combat {
    pub fn new(fit: Loadout, damage: Damage) -> Self {
        Self {
            field: None,
            raiders: Vec::new(),
            shots: Vec::new(),
            pulses: Vec::new(),
            flashes: Vec::new(),
            blasts: Vec::new(),
            debris: Vec::new(),
            wrecks: Vec::new(),
            player: Vessel::new(fit, damage),
            target: None,
            lost: None,
            bands: Bands::default(),
            since_scored: 10.0,
            since_shield_hit: 10.0,
            since_hull_hit: 10.0,
            hit_from: DVec3::NEG_Z,
            tally: Tally::default(),
            band_epoch: 0,
            rng: Rng::new(0x636f_6d62),
            pending: Vec::new(),
            said: Vec::new(),
            dropped: Vec::new(),
            fresh: true,
        }
    }

    /// The ship at rest in a dock or a city: cool, shields up, guns ready,
    /// nothing in flight. That is just how a loaded checkpoint has it, so
    /// the game after a checkpoint plays the same whether it was restored
    /// or not (DESIGN §7.7). The bands fought are remembered.
    pub fn rest(&mut self, fit: &Loadout, damage: &Damage) {
        if self.fresh && self.player.fit == *fit && self.player.damage == *damage {
            return;
        }
        let mut c = Combat::new(fit.clone(), damage.clone());
        c.bands = std::mem::take(&mut self.bands);
        // The sound follows these counts; they only go up.
        c.tally = self.tally;
        *self = c;
    }

    /// Raiders within `range` of `pos`.
    pub fn hostiles_within(&self, pos: DVec3, range: f64) -> usize {
        self.raiders
            .iter()
            .filter(|r| r.tactic != Tactic::Flee && (r.craft.pos - pos).length() < range)
            .count()
    }

    pub fn raider(&self, id: u32) -> Option<&Raider> {
        self.raiders.iter().find(|r| r.id == id)
    }

    /// Where to aim to hit raider `id` with the player's guns, if it's
    /// there.
    pub fn lead(&self, shooter: &Craft, id: u32) -> Option<DVec3> {
        let r = self.raider(id)?;
        Some(lead_point(
            shooter.pos,
            shooter.vel,
            r.craft.pos,
            r.craft.vel,
            self.player.shot_speed(),
        ))
    }

    /// Land a hit on raider `id` from outside the fight (for staging
    /// scenes and tests).
    pub fn hit_raider(&mut self, id: u32, damage: f64, pilot: &Pilot) {
        if let Some(r) = self.raider(id) {
            let point = r.craft.pos;
            self.land(Side::Raider(id), damage, point, DVec3::NEG_Z, pilot);
        }
    }

    /// Target the next raider, nearest first from the current one.
    pub fn cycle_target(&mut self, from: DVec3) {
        let mut ids: Vec<(f64, u32)> = self
            .raiders
            .iter()
            .map(|r| ((r.craft.pos - from).length(), r.id))
            .collect();
        ids.sort_by(|a, b| a.0.total_cmp(&b.0));
        let next = match self
            .target
            .and_then(|t| ids.iter().position(|(_, id)| *id == t))
        {
            Some(i) => ids.get(i + 1).or(ids.first()),
            None => ids.first(),
        };
        self.target = next.map(|(_, id)| *id);
    }

    /// The fight is somewhere else now: shells, flashes and the band go.
    /// Wrecks and the bands' records stay.
    pub fn leave(&mut self) {
        self.field = None;
        self.raiders.clear();
        self.shots.clear();
        self.pulses.clear();
        self.flashes.clear();
        self.blasts.clear();
        self.debris.clear();
        self.wrecks.clear();
        self.target = None;
    }

    /// Bring out the band of `field`, if it has one and any of it is
    /// alive, waiting for a ship at `player` (field-relative).
    #[allow(clippy::too_many_arguments)]
    pub fn arrive(
        &mut self,
        galaxy: &GalaxyState,
        system: SystemId,
        field: FieldId,
        day: f64,
        player: &Craft,
    ) -> Option<String> {
        self.leave();
        self.field = Some(field);
        let epoch = (day / BAND_DAYS) as u64;
        self.band_epoch = epoch;
        let band = band(galaxy, system, field, epoch);
        let record = self.bands.0.get(&field).copied().unwrap_or_default();
        let killed = if record.epoch == epoch {
            record.killed
        } else {
            0
        };
        let mut rng = Rng::new((field.index() as u64) ^ epoch.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        // The fight's own dice (shot scatter, wreckage) start over too, from
        // the band and who of it is left: what came before (guns fired at
        // nothing, an earlier fight) isn't saved, and a restored checkpoint
        // has to meet the same fight (DESIGN §7.7).
        self.rng = Rng::new(rng.next_u64() ^ u64::from(killed).wrapping_mul(0xd6e8_feb8_6659_fd93));
        // They come from deeper in the field, toward the ship.
        let inward = (-player.pos).normalize_or(DVec3::X);
        let side = inward.cross(DVec3::Y).normalize_or(DVec3::Z);
        let mut out = 0;
        for (i, hull) in band.into_iter().enumerate() {
            if killed & (1 << i) != 0 {
                continue;
            }
            let Some(fit) = retrox_content::ships::stock(hull) else {
                continue;
            };
            let spread = (i as f64 - 1.0) * 350.0;
            let pos = player.pos
                + inward * (AMBUSH_RANGE + rng.range(-800.0, 800.0))
                + side * spread
                + DVec3::Y * rng.range(-300.0, 300.0);
            let to_player = (player.pos - pos).normalize_or(DVec3::Z);
            self.raiders.push(Raider {
                id: i as u32,
                vessel: Vessel::new(fit, Damage::default()),
                craft: Craft {
                    pos,
                    vel: to_player * 150.0,
                    rot: look_rotation(to_player),
                    spin: DVec3::ZERO,
                },
                burn: 0.0,
                tactic: Tactic::Attack,
                timer: 0.0,
                skill: rng.range(0.45, 0.9),
                rng: Rng::new(rng.next_u64()),
                jink: DVec3::ZERO,
                jink_t: 0.0,
                burst: rng.range(-1.0, 0.5),
            });
            out += 1;
        }
        (out > 0).then(|| {
            let ships = if out == 1 { "ship" } else { "ships" };
            format!("Raiders! {out} {ships} closing")
        })
    }

    /// One frame of the fight.
    pub fn update(&mut self, dt: f64, pilot: Pilot, solids: &[(DVec3, f64)]) -> Outcome {
        let mut out = Outcome::default();
        self.fresh = false;
        self.since_scored += dt;
        self.since_shield_hit += dt;
        self.since_hull_hit += dt;
        self.age_effects(dt);
        let alive = self.lost.is_none();
        if let Some(t) = &mut self.lost {
            *t += dt;
        }

        // The player's guns.
        let fired = self.player.systems(dt, pilot.burn, pilot.trigger && alive);
        if alive && !fired.is_empty() {
            let lead = self.target.and_then(|t| self.lead(&pilot.craft, t));
            for i in fired {
                let gun = self.player.guns[i].clone();
                let muzzle = pilot.craft.pos + pilot.craft.rot * gun.muzzle;
                let dir = aim(pilot.craft.forward(), muzzle, lead, GIMBAL);
                let dir = scatter(&mut self.rng, dir, 0.002);
                self.fire(Side::Player, &gun, muzzle, pilot.craft.vel, dir, &pilot);
            }
        }

        // The raiders.
        let target = alive.then_some(pilot.craft);
        for i in 0..self.raiders.len() {
            let fired = self.raiders[i].think(dt, target, solids);
            for (gun, muzzle, dir) in fired {
                let vel = self.raiders[i].craft.vel;
                let id = self.raiders[i].id;
                self.fire(Side::Raider(id), &gun, muzzle, vel, dir, &pilot);
            }
        }
        // Escaped: a fleeing raider far enough off is gone.
        self.raiders.retain(|r| {
            !(r.tactic == Tactic::Flee && (r.craft.pos - pilot.craft.pos).length() > 14e3)
        });

        // Shells.
        let mut shots = std::mem::take(&mut self.shots);
        shots.retain_mut(|s| {
            let from = s.pos;
            s.pos += s.vel * dt;
            s.life -= dt;
            let hit = self.strike(s.from, from, s.pos, dt, &pilot);
            if let Some((side, point, _)) = hit {
                self.land(side, s.damage, point, s.vel, &pilot);
                return false;
            }
            s.life > 0.0
        });
        self.shots.extend(shots);
        // Lasers already landed in `fire`; apply what they did now.
        let pending = std::mem::take(&mut self.pending);
        for (side, damage, point, dir) in pending {
            self.land(side, damage, point, dir, &pilot);
        }
        if self.target.is_none_or(|t| self.raider(t).is_none()) {
            self.target = None;
            if alive {
                self.cycle_target(pilot.craft.pos);
            }
        }
        out.player_destroyed = self.lost == Some(0.0);
        out.messages.append(&mut self.said);
        out.salvage.append(&mut self.dropped);
        out
    }

    fn age_effects(&mut self, dt: f64) {
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

    /// Fire one gun.
    #[allow(clippy::too_many_arguments)]
    fn fire(
        &mut self,
        side: Side,
        gun: &Gun,
        muzzle: DVec3,
        vel: DVec3,
        dir: DVec3,
        pilot: &Pilot,
    ) {
        match (side, gun.laser) {
            (Side::Player, true) => self.tally.lasers += 1,
            (Side::Player, false) => self.tally.fired += 1,
            _ => self.tally.enemy_fired += 1,
        }
        self.flashes.push(Flash {
            pos: muzzle,
            vel,
            kind: FlashKind::Muzzle,
            size: if gun.laser { 0.45 } else { 0.6 },
            age: 0.0,
            life: 0.06,
        });
        if gun.laser {
            let end = muzzle + dir * gun.range;
            let hit = self.strike(side, muzzle, end, 0.0, pilot);
            let to = hit.map_or(end, |h| h.1);
            self.pulses.push(Pulse {
                from: muzzle,
                to,
                age: 0.0,
                side,
            });
            if let Some((target, point, _)) = hit {
                self.pending.push((target, gun.damage, point, dir));
            }
        } else {
            self.shots.push(Shot {
                pos: muzzle,
                vel: vel + dir * SHELL_SPEED,
                life: gun.range / SHELL_SPEED,
                damage: gun.damage,
                from: side,
            });
        }
    }

    /// The first ship on the other side from `side` that the segment
    /// `a`→`b` (swept over `dt`) passes into: who, where, and how far
    /// along.
    fn strike(
        &self,
        side: Side,
        a: DVec3,
        b: DVec3,
        dt: f64,
        pilot: &Pilot,
    ) -> Option<(Side, DVec3, f64)> {
        let mut best: Option<(Side, DVec3, f64)> = None;
        let mut test = |who: Side, v: &Vessel, c: &Craft| {
            let (across, along) = if v.shield > 0.0 {
                v.bubble()
            } else {
                v.hull_shape()
            };
            // In the target's frame, which moved during the frame too.
            let inv = c.rot.inverse();
            let a_l = inv * (a - (c.pos - c.vel * dt));
            let b_l = inv * (b - c.pos);
            if let Some(t) = segment_ellipsoid(a_l, b_l, DVec3::new(across, across, along))
                && best.is_none_or(|(_, _, bt)| t < bt)
            {
                best = Some((who, a + (b - a) * t, t));
            }
        };
        match side {
            Side::Player => {
                for r in &self.raiders {
                    test(Side::Raider(r.id), &r.vessel, &r.craft);
                }
            }
            Side::Raider(_) => {
                if self.lost.is_none() {
                    test(Side::Player, &self.player, &pilot.craft);
                }
            }
        }
        best
    }

    /// A shot lands on `side` at `point` (anchor-relative), travelling
    /// along `dir`.
    fn land(&mut self, side: Side, damage: f64, point: DVec3, dir: DVec3, pilot: &Pilot) {
        let (vessel, craft) = match side {
            Side::Player => {
                if self.lost.is_some() {
                    return;
                }
                (&mut self.player, pilot.craft)
            }
            Side::Raider(id) => match self.raiders.iter_mut().find(|r| r.id == id) {
                Some(r) => {
                    // Being shot rattles them.
                    r.jink_t = r.jink_t.min(0.2);
                    (&mut r.vessel, r.craft)
                }
                None => return,
            },
        };
        let local = craft.rot.inverse() * (point - craft.pos);
        let hit = vessel.hit(damage, local);
        let length = vessel.stats.length_m;
        let fit = vessel.fit.clone();
        match side {
            Side::Player => {
                self.hit_from = (craft.rot.inverse() * -dir).normalize_or(DVec3::Z);
                if hit == Hit::Shield {
                    self.since_shield_hit = 0.0;
                    self.tally.shield_hits += 1;
                } else {
                    self.since_hull_hit = 0.0;
                    self.tally.hull_hits += 1;
                }
            }
            Side::Raider(_) => self.since_scored = 0.0,
        }
        match hit {
            Hit::Shield => {
                let (across, along) = vessel_bubble(&self.player, &self.raiders, side);
                let dir_l = (local / DVec3::new(across, across, along)).normalize_or(DVec3::Z);
                self.flashes.push(Flash {
                    pos: point,
                    vel: craft.vel,
                    kind: FlashKind::Shield {
                        ship: side,
                        dir: dir_l,
                    },
                    size: 1.0,
                    age: 0.0,
                    life: 0.45,
                });
            }
            Hit::Hull | Hit::Wrecked(_) => {
                self.flashes.push(Flash {
                    pos: point,
                    vel: craft.vel,
                    kind: FlashKind::Spark,
                    size: 0.6 + damage.sqrt() * 0.08,
                    age: 0.0,
                    life: 0.18,
                });
                let n = if matches!(hit, Hit::Wrecked(_)) {
                    10
                } else {
                    3
                };
                self.embers(point, craft.vel, n, 60.0, 0.5);
                if let Hit::Wrecked(part) = hit {
                    self.blasts.push(Blast {
                        pos: point,
                        vel: craft.vel,
                        radius: length * 0.35,
                        age: 0.0,
                        life: 1.0,
                        seed: self.rng.range(0.0, 100.0),
                        ring: false,
                    });
                    let name = match side {
                        Side::Player => self.player.parts[part].name,
                        Side::Raider(id) => self
                            .raider(id)
                            .map_or("section", |r| r.vessel.parts[part].name),
                    };
                    let whose = if side == Side::Player {
                        "Our"
                    } else {
                        "Raider's"
                    };
                    self.said
                        .push(format!("{whose} {} is wrecked", name.to_lowercase()));
                }
            }
            Hit::Destroyed => {
                self.explode(&fit, craft, length);
                match side {
                    Side::Player => {
                        self.lost = Some(0.0);
                        self.said.push("Ship destroyed".to_owned());
                    }
                    Side::Raider(id) => {
                        self.raiders.retain(|r| r.id != id);
                        if let Some(field) = self.field {
                            let band = self.bands.0.entry(field).or_default();
                            if band.epoch != self.band_epoch {
                                *band = Band {
                                    epoch: self.band_epoch,
                                    killed: 0,
                                };
                            }
                            band.killed |= 1 << id;
                        }
                        let left = self.raiders.len();
                        self.said.push(if left == 0 {
                            "Raider destroyed · the band is broken".to_owned()
                        } else {
                            format!("Raider destroyed · {left} left")
                        });
                        // What it was carrying spills out.
                        let tonnes = (length / 4.0).clamp(1.0, 8.0) * self.rng.range(0.5, 1.2);
                        for (good, share) in [(Good::Components, 0.6), (Good::Metal, 0.4)] {
                            let seed = self.rng.next_u64();
                            let kick = random_dir(&mut self.rng) * 6.0;
                            self.dropped.push((
                                craft.pos + kick,
                                craft.vel * 0.3 + kick,
                                good,
                                tonnes * share,
                                seed,
                            ));
                        }
                    }
                }
            }
        }
    }

    fn embers(&mut self, at: DVec3, vel: DVec3, n: usize, speed: f64, life: f64) {
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

    /// A ship blows up: a fireball and its secondaries, a flash, plates
    /// and embers flung out, and the wreck tumbling on.
    /// The player's ship goes up, whatever its hull has left (for
    /// screenshots).
    pub fn lose_player(&mut self, craft: Craft) {
        let fit = self.player.fit.clone();
        let length = self.player.stats.length_m;
        self.explode(&fit, craft, length);
        self.lost = Some(0.0);
    }

    fn explode(&mut self, fit: &Loadout, craft: Craft, length: f64) {
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

/// How long a laser pulse shows, seconds.
pub const PULSE_LIFE: f64 = 0.09;

fn vessel_bubble(player: &Vessel, raiders: &[Raider], side: Side) -> (f64, f64) {
    match side {
        Side::Player => player.bubble(),
        Side::Raider(id) => raiders
            .iter()
            .find(|r| r.id == id)
            .map_or((5.0, 10.0), |r| r.vessel.bubble()),
    }
}

impl Raider {
    /// One frame of flying and fighting toward `target` (the player, if
    /// alive). Returns the guns it fires: the gun, its muzzle, and which
    /// way.
    fn think(
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
                (-c.pos, 120.0)
            }
            (Tactic::Return, Some(t)) => {
                if t.pos.length() < LEASH * 0.8 {
                    self.tactic = Tactic::Attack;
                }
                if c.pos.length() < 3000.0 {
                    (t.pos - c.pos, 60.0)
                } else {
                    (-c.pos, 350.0)
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
                if t.pos.length() > LEASH {
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

/// Which hulls a field's band flies this epoch (empty: no band).
pub fn band(
    galaxy: &GalaxyState,
    system: SystemId,
    field: FieldId,
    epoch: u64,
) -> Vec<&'static str> {
    let chance = band_chance(galaxy, system);
    let mut rng = Rng::new(
        (field.index() as u64).rotate_left(17) ^ epoch.wrapping_mul(0x2545_f491_4f6c_dd1d),
    );
    if !rng.chance(chance) {
        return Vec::new();
    }
    let frontier = chance > 0.6;
    let n = if frontier {
        2 + rng.below(2)
    } else {
        1 + rng.below(2)
    };
    (0..n)
        .map(|i| {
            let roll = rng.f64();
            if frontier && i == 0 && roll < 0.35 {
                "gunship"
            } else if roll < 0.65 {
                "interceptor"
            } else {
                "heavy-fighter"
            }
        })
        .collect()
}

/// How likely a field in this system is to have raiders: never where a
/// faction keeps its capital, sometimes where people live, mostly where
/// no one does.
pub fn band_chance(galaxy: &GalaxyState, system: SystemId) -> f64 {
    let capital = galaxy.factions.values().any(|f| {
        galaxy
            .settlements
            .get(f.capital)
            .is_some_and(|s| s.system == system)
    });
    if capital {
        0.0
    } else if galaxy.settlements_in(system).next().is_some() {
        0.45
    } else {
        0.85
    }
}

/// Where to aim from `pos` (moving at `vel`) to hit something at `tpos`
/// moving at `tvel` with shots at `speed` (on top of the shooter's
/// velocity; `None` for lasers).
pub fn lead_point(pos: DVec3, vel: DVec3, tpos: DVec3, tvel: DVec3, speed: Option<f64>) -> DVec3 {
    let Some(speed) = speed else {
        return tpos;
    };
    let rel = tvel - vel;
    let mut t = (tpos - pos).length() / speed;
    for _ in 0..4 {
        t = (tpos + rel * t - pos).length() / speed;
    }
    tpos + rel * t
}

/// The direction to fire: along `forward`, or swung toward `lead` if it's
/// within `gimbal` radians of it.
fn aim(forward: DVec3, muzzle: DVec3, lead: Option<DVec3>, gimbal: f64) -> DVec3 {
    match lead {
        Some(l) => {
            let to = (l - muzzle).normalize_or(forward);
            if forward.angle_between(to) < gimbal {
                to
            } else {
                forward
            }
        }
        None => forward,
    }
}

fn scatter(rng: &mut Rng, dir: DVec3, sigma: f64) -> DVec3 {
    let a = dir.any_orthonormal_vector();
    let b = dir.cross(a);
    let (r, t) = (rng.f64().max(1e-9), rng.f64());
    // Box–Muller: a normal spread around the line.
    let g = (-2.0 * r.ln()).sqrt() * sigma;
    let th = t * TAU;
    (dir + a * g * th.cos() + b * g * th.sin()).normalize()
}

fn random_dir(rng: &mut Rng) -> DVec3 {
    let z = rng.range(-1.0, 1.0);
    let a = rng.range(0.0, TAU);
    let r = (1.0 - z * z).sqrt();
    DVec3::new(r * a.cos(), z, r * a.sin())
}

/// Stick input that turns a ship with rotation `rot` toward `dir`:
/// pitch and yaw at the target, rolling to bring it overhead when it's
/// well off the nose (ships pitch faster than they yaw).
pub fn stick_toward(rot: DQuat, dir: DVec3) -> DVec3 {
    let l = rot.inverse() * dir.normalize_or(DVec3::NEG_Z);
    let pitch = l.y.atan2(-l.z);
    let yaw = l.x.atan2(-l.z);
    let off = (-l.z).clamp(-1.0, 1.0).acos();
    let roll = if off > 0.25 && (l.x * l.x + l.y * l.y) > 1e-6 {
        let r = l.x.atan2(l.y);
        // Never roll more than a right angle: past that, pitch down.
        let r = if r.abs() > PI / 2.0 {
            r - PI * r.signum()
        } else {
            r
        };
        (r * 1.2).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    DVec3::new(
        (pitch * 3.0).clamp(-1.0, 1.0),
        (yaw * 3.0).clamp(-1.0, 1.0),
        roll,
    )
}

/// Where along `a`→`b` (0..1) a segment first enters an ellipsoid at the
/// origin with semi-axes `axes`.
pub fn segment_ellipsoid(a: DVec3, b: DVec3, axes: DVec3) -> Option<f64> {
    let (a, b) = (a / axes, b / axes);
    let d = b - a;
    let dd = d.length_squared();
    if a.length_squared() <= 1.0 {
        return Some(0.0);
    }
    if dd < 1e-12 {
        return None;
    }
    let bq = a.dot(d);
    let c = a.length_squared() - 1.0;
    let disc = bq * bq - dd * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-bq - disc.sqrt()) / dd;
    (0.0..=1.0).contains(&t).then_some(t)
}

#[cfg(test)]
mod tests;
