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
//! **Raiders.** The ships of a pirate gang from the sim (DESIGN §7.2.2,
//! [`retrox_sim::pirates`]), brought out when the player meets the gang:
//! at its lair field, or wherever it intercepts them. Raiders fly the same
//! arcade flight model the player does, with stock fits, under the same
//! budgets, and guard a *home* point (the field, or where the fight
//! began). Who dies is reported back to the world, which pays the bounty
//! and tells the sim.
//!
//! Positions are relative to the fight's *anchor*: the field (or, outside
//! one, wherever the player's ship is framed). Metres, y up.

mod effects;
mod geom;
mod raider;
mod vessel;

pub use effects::{Blast, Debris, Flash, FlashKind, PULSE_LIFE, Pulse, Wreck};
pub(crate) use geom::random_dir;
pub use geom::{lead_point, segment_ellipsoid};
pub use raider::{Crew, Raider, TRADER_IDS, Tactic};
pub use vessel::{Damage, Gun, Hit, Vessel};

use glam::{DQuat, DVec3};
use retrox_gen::Rng;
use retrox_sim::FactionId;
use retrox_sim::FieldId;
use retrox_sim::bubble::FleetKey;
use retrox_sim::economy::Good;
use retrox_sim::pirates::GangId;
use retrox_sim::ship::Loadout;

use crate::world::look_rotation;
use geom::{aim, scatter};

/// Autocannon shells leave the muzzle this fast, m/s, on top of the
/// ship's velocity.
pub const SHELL_SPEED: f64 = 1600.0;
/// How far fixed guns swing to meet a target's lead, radians.
pub const GIMBAL: f64 = 0.07;
/// Hostiles this close keep the ship from fast travel, jumps and saving.
pub const HOSTILE_RANGE: f64 = 20e3;
/// Seconds after the last hit before shields start to come back.
pub const SHIELD_DELAY: f64 = 3.0;
/// Raiders give up the chase once the player is this far from their home.
pub const LEASH: f64 = 16e3;
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

/// A shell in flight.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub pos: DVec3,
    pub vel: DVec3,
    pub life: f64,
    pub damage: f64,
    pub from: Side,
}

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
    /// Ships destroyed this frame, by id and whose; ships the player hit,
    /// and whether they were peaceful until then.
    pub kills: Vec<(u32, Crew)>,
    pub hits: Vec<(u32, bool)>,
}

/// Everything fighting near the player.
#[derive(Clone, Debug)]
pub struct Combat {
    /// The field the fight is at, if any.
    pub field: Option<FieldId>,
    /// The gang whose ships are out, if any.
    pub gang: Option<GangId>,
    /// Raiders of the gang destroyed in this fight, by id (the ship's
    /// place in the gang).
    pub killed: Vec<u32>,
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
    /// Seconds since the player's guns last hit something, and since the
    /// player's shield and hull were last hit (for the HUD).
    pub since_scored: f64,
    pub since_shield_hit: f64,
    pub since_hull_hit: f64,
    /// Running counts of what happened, for the sound to follow: the
    /// player's shells and pulses, everyone else's, hits on the player's
    /// shield and hull, and ships blown up.
    pub tally: Tally,
    rng: Rng,
    kills: Vec<(u32, Crew)>,
    hits: Vec<(u32, bool)>,
    next_trader: u32,
    /// Laser hits waiting to land this frame: who, how hard, where.
    pending: Vec<(Side, f64, DVec3)>,
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
            gang: None,
            killed: Vec::new(),
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
            since_scored: 10.0,
            since_shield_hit: 10.0,
            since_hull_hit: 10.0,
            tally: Tally::default(),
            rng: Rng::new(0x636f_6d62),
            kills: Vec::new(),
            hits: Vec::new(),
            next_trader: TRADER_IDS,
            pending: Vec::new(),
            said: Vec::new(),
            dropped: Vec::new(),
            fresh: true,
        }
    }

    /// The ship at rest in a dock or a city: cool, shields up, guns ready,
    /// nothing in flight. That is just how a loaded checkpoint has it, so
    /// the game after a checkpoint plays the same whether it was restored
    /// or not (DESIGN §7.7).
    pub fn rest(&mut self, fit: &Loadout, damage: &Damage) {
        if self.fresh && self.player.fit == *fit && self.player.damage == *damage {
            return;
        }
        self.rebuild(fit.clone(), damage.clone());
    }

    /// A fresh fight for a changed ship (a refit, a repair, a rest). The
    /// tally stays: the sound follows those counts, so they only go up.
    pub fn rebuild(&mut self, fit: Loadout, damage: Damage) {
        let mut c = Combat::new(fit, damage);
        c.tally = self.tally;
        *self = c;
    }

    /// Hostile ships within `range` of `pos`.
    pub fn hostiles_within(&self, pos: DVec3, range: f64) -> usize {
        self.raiders
            .iter()
            .filter(|r| {
                r.hostile && r.tactic != Tactic::Flee && (r.craft.pos - pos).length() < range
            })
            .count()
    }

    /// Whether a trader fleet's ships are in the fight.
    pub fn has_convoy(&self, fleet: FleetKey) -> bool {
        self.raiders.iter().any(|r| r.trader_fleet() == Some(fleet))
    }

    /// A trader fleet's ships join the scene, at `slots` (anchor-relative
    /// position, velocity and heading), hostile or not.
    pub fn join_convoy(
        &mut self,
        fleet: FleetKey,
        faction: FactionId,
        fit: &Loadout,
        slots: &[(DVec3, DVec3, DQuat)],
        hostile: bool,
    ) {
        let mut rng = Rng::new(
            slotmap::Key::data(&fleet.0).as_ffi() ^ u64::from(fleet.1).wrapping_mul(0x9e37),
        );
        for &(pos, vel, rot) in slots {
            let id = self.next_trader;
            self.next_trader += 1;
            let vessel = Vessel::new(fit.clone(), Damage::default());
            let armed = !vessel.guns.is_empty();
            let craft = Craft {
                pos,
                vel,
                rot,
                spin: DVec3::ZERO,
            };
            let crew = Crew::Trader { fleet, faction };
            let mut r = Raider::new(id, vessel, craft, crew, pos, (0.25, 0.55), &mut rng);
            if hostile && !armed {
                r.tactic = Tactic::Flee;
            }
            r.hostile = hostile;
            r.slot = (!hostile).then_some((pos, vel));
            self.raiders.push(r);
        }
    }

    /// Where a peaceful convoy's ships should be now, in fleet order.
    pub fn set_slots(&mut self, fleet: FleetKey, slots: &[(DVec3, DVec3, DQuat)]) {
        let ships = self
            .raiders
            .iter_mut()
            .filter(|r| r.trader_fleet() == Some(fleet));
        for (r, &(pos, vel, _)) in ships.zip(slots) {
            if !r.hostile {
                r.slot = Some((pos, vel));
            }
        }
    }

    /// A convoy the player has left flies on out of the scene: its
    /// peaceful ships at once, its hostile ones once the player at `me`
    /// is well past their leash (as for a gang), or all of them when
    /// `all` (the player isn't flying).
    pub fn drop_convoy(&mut self, fleet: FleetKey, me: DVec3, all: bool) {
        self.raiders.retain(|r| {
            r.trader_fleet() != Some(fleet)
                || (!all && r.hostile && (r.home - me).length() <= LEASH * 2.0)
        });
    }

    /// The trader fleets in the scene.
    pub fn convoys(&self) -> Vec<FleetKey> {
        let mut v: Vec<FleetKey> = self
            .raiders
            .iter()
            .filter_map(|r| r.trader_fleet())
            .collect();
        v.dedup();
        v
    }

    /// A trader fleet turns on the player: the armed fight, the rest run.
    fn provoke(&mut self, fleet: FleetKey) {
        for r in self
            .raiders
            .iter_mut()
            .filter(|r| r.trader_fleet() == Some(fleet) && !r.hostile)
        {
            r.hostile = true;
            r.slot = None;
            r.home = r.craft.pos;
            r.tactic = if r.vessel.guns.is_empty() {
                Tactic::Flee
            } else {
                Tactic::Attack
            };
        }
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
            self.land(Side::Raider(id), damage, point, pilot);
        }
    }

    /// Target the next raider, nearest first from the current one.
    pub fn cycle_target(&mut self, from: DVec3) {
        // Hostiles first, nearest first; peaceful ships after them.
        let mut ids: Vec<(bool, f64, u32)> = self
            .raiders
            .iter()
            .map(|r| (!r.hostile, (r.craft.pos - from).length(), r.id))
            .collect();
        ids.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let ids: Vec<(f64, u32)> = ids.into_iter().map(|(_, d, id)| (d, id)).collect();
        let next = match self
            .target
            .and_then(|t| ids.iter().position(|(_, id)| *id == t))
        {
            Some(i) => ids.get(i + 1).or(ids.first()),
            None => ids.first(),
        };
        self.target = next.map(|(_, id)| *id);
    }

    /// The fight's anchor moved: `dpos` and `dvel` are the old anchor's
    /// position and velocity less the new one's. Carry everything in the
    /// scene over, so nothing in it moves in the system frame.
    pub fn rebase(&mut self, dpos: DVec3, dvel: DVec3) {
        for r in &mut self.raiders {
            r.craft.pos += dpos;
            r.craft.vel += dvel;
            r.home += dpos;
            if let Some((pos, vel)) = &mut r.slot {
                *pos += dpos;
                *vel += dvel;
            }
        }
        for s in &mut self.shots {
            s.pos += dpos;
            s.vel += dvel;
        }
        for p in &mut self.pulses {
            p.from += dpos;
            p.to += dpos;
        }
        for f in &mut self.flashes {
            f.pos += dpos;
            f.vel += dvel;
        }
        for b in &mut self.blasts {
            b.pos += dpos;
            b.vel += dvel;
        }
        for d in &mut self.debris {
            d.pos += dpos;
            d.vel += dvel;
        }
        for w in &mut self.wrecks {
            w.pos += dpos;
            w.vel += dvel;
        }
        for (_, _, point) in &mut self.pending {
            *point += dpos;
        }
        for (pos, vel, ..) in &mut self.dropped {
            *pos += dpos;
            *vel += dvel;
        }
    }

    /// The fight is somewhere else now: shells, flashes, wrecks and the
    /// gang go (their positions are relative to the anchor). The world
    /// reports the fight to the sim first.
    pub fn leave(&mut self) {
        self.field = None;
        self.gang = None;
        self.killed.clear();
        self.raiders.clear();
        self.shots.clear();
        self.pulses.clear();
        self.flashes.clear();
        self.blasts.clear();
        self.debris.clear();
        self.wrecks.clear();
        self.target = None;
    }

    /// Bring out `gang`'s ships (`hulls`, by their place in the gang)
    /// near a ship at `player`: they come in from `from` and guard `home`
    /// (both anchor-relative). `seed` decides the fight's dice, so a
    /// restored checkpoint meets the same fight (DESIGN §7.7).
    #[allow(clippy::too_many_arguments)]
    pub fn bring_out(
        &mut self,
        gang: GangId,
        name: &str,
        hulls: &[(u32, String)],
        seed: u64,
        field: Option<FieldId>,
        player: &Craft,
        from: DVec3,
        home: DVec3,
    ) -> Option<String> {
        self.leave();
        self.field = field;
        self.gang = Some(gang);
        let mut rng = Rng::new(seed);
        // The fight's own dice (shot scatter, wreckage) start over too, from
        // the gang and who of it is left: what came before (guns fired at
        // nothing, an earlier fight) isn't saved.
        self.rng = Rng::new(rng.next_u64());
        // They come from where the gang was, toward the ship.
        let inward = (from - player.pos).normalize_or(DVec3::X);
        let side = inward.cross(DVec3::Y).normalize_or(DVec3::Z);
        let mut out = 0;
        for (k, (id, hull)) in hulls.iter().enumerate() {
            let Some(fit) = retrox_content::ships::stock(hull) else {
                continue;
            };
            let spread = (k as f64 - 1.0) * 350.0;
            let pos = player.pos
                + inward * (AMBUSH_RANGE + rng.range(-800.0, 800.0))
                + side * spread
                + DVec3::Y * rng.range(-300.0, 300.0);
            let to_player = (player.pos - pos).normalize_or(DVec3::Z);
            let craft = Craft {
                pos,
                vel: player.vel + to_player * 150.0,
                rot: look_rotation(to_player),
                spin: DVec3::ZERO,
            };
            let vessel = Vessel::new(fit, Damage::default());
            self.raiders.push(Raider::new(
                *id,
                vessel,
                craft,
                Crew::Pirate,
                home,
                (0.45, 0.9),
                &mut rng,
            ));
            out += 1;
        }
        (out > 0).then(|| {
            let ships = if out == 1 { "ship" } else { "ships" };
            format!("Raiders! The {name}: {out} {ships} closing")
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
            if let (false, Some((pos, vel))) = (self.raiders[i].hostile, self.raiders[i].slot) {
                // Holding its place in its convoy.
                let r = &mut self.raiders[i];
                r.craft.pos = pos;
                r.craft.vel = vel;
                r.burn = 0.15;
                let k = 1.0 - (-2.0 * dt).exp();
                if vel.length() > 1.0 {
                    r.craft.rot = r.craft.rot.slerp(look_rotation(vel), k).normalize();
                }
                r.vessel.systems(dt, 0.1, false);
                continue;
            }
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
                self.land(side, s.damage, point, &pilot);
                return false;
            }
            s.life > 0.0
        });
        self.shots.extend(shots);
        // Lasers already landed in `fire`; apply what they did now.
        let pending = std::mem::take(&mut self.pending);
        for (side, damage, point) in pending {
            self.land(side, damage, point, &pilot);
        }
        if self.target.is_none_or(|t| self.raider(t).is_none()) {
            self.target = None;
            if alive && self.raiders.iter().any(|r| r.hostile) {
                self.cycle_target(pilot.craft.pos);
            }
        }
        out.player_destroyed = self.lost == Some(0.0);
        out.messages.append(&mut self.said);
        out.salvage.append(&mut self.dropped);
        out.kills.append(&mut self.kills);
        out.hits.append(&mut self.hits);
        out
    }
}

impl Combat {
    /// Fire one gun.
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
                self.pending.push((target, gun.damage, point));
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
    fn land(&mut self, side: Side, damage: f64, point: DVec3, pilot: &Pilot) {
        let (vessel, craft) = match side {
            Side::Player => {
                if self.lost.is_some() {
                    return;
                }
                (&mut self.player, pilot.craft)
            }
            Side::Raider(id) => {
                // Only the player's guns hit anyone else; a trader they
                // hit turns on them, with all its fleet.
                let peaceful = self.raider(id).is_some_and(|r| !r.hostile);
                self.hits.push((id, peaceful));
                if let Some(fleet) = self.raider(id).and_then(|r| r.trader_fleet()) {
                    self.provoke(fleet);
                }
                match self.raiders.iter_mut().find(|r| r.id == id) {
                    Some(r) => {
                        r.rattle();
                        (&mut r.vessel, r.craft)
                    }
                    None => return,
                }
            }
        };
        let local = craft.rot.inverse() * (point - craft.pos);
        let hit = vessel.hit(damage, local);
        let length = vessel.stats.length_m;
        let fit = vessel.fit.clone();
        match side {
            Side::Player => {
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
                    let whose = match side {
                        Side::Player => "Our",
                        Side::Raider(id) if id >= TRADER_IDS => "Trader's",
                        Side::Raider(_) => "Raider's",
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
                        let crew = self.raider(id).map_or(Crew::Pirate, |r| r.crew);
                        self.raiders.retain(|r| r.id != id);
                        self.kills.push((id, crew));
                        if id >= TRADER_IDS {
                            // The world says what it cost; the wreck still
                            // spills what it was carrying below.
                        } else {
                            self.killed.push(id);
                        }
                        let left = self
                            .raiders
                            .iter()
                            .filter(|r| r.crew == Crew::Pirate)
                            .count();
                        if id < TRADER_IDS {
                            self.said.push(if left == 0 {
                                "Raider destroyed · the gang is broken".to_owned()
                            } else {
                                format!("Raider destroyed · {left} left")
                            });
                        }
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
}

fn vessel_bubble(player: &Vessel, raiders: &[Raider], side: Side) -> (f64, f64) {
    match side {
        Side::Player => player.bubble(),
        Side::Raider(id) => raiders
            .iter()
            .find(|r| r.id == id)
            .map_or((5.0, 10.0), |r| r.vessel.bubble()),
    }
}

#[cfg(test)]
mod tests;
