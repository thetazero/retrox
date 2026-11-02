//! The fight around the ship (see [`crate::combat`]): raider bands come
//! out when the ship enters their field, and the ship's own guns, shields
//! and hull run every frame. Damage is repaired, at a price, on docking or
//! landing.

use glam::DVec3;

use super::{Mode, Place, World, rock_offset};
use crate::combat::{Combat, Craft, HOSTILE_RANGE, Pilot, REPAIR_PER_POINT};

impl World {
    /// Where the fight is anchored: the field whose band is out, else
    /// wherever the ship is framed.
    pub fn combat_anchor(&self) -> Place {
        match self.combat.field {
            Some(f) => Place::Field(f),
            None => self.ship.frame,
        }
    }

    /// The anchor's position in the system frame.
    pub fn combat_origin(&self) -> DVec3 {
        self.position(self.combat_anchor())
    }

    /// The ship, relative to the anchor.
    pub fn combat_position(&self) -> DVec3 {
        self.ship_position() - self.combat_origin()
    }

    /// Hostile ships within [`HOSTILE_RANGE`]: while there are any, no fast
    /// travel, no jumps, no saving.
    pub fn hostiles_near(&self) -> usize {
        if self.mode != Mode::Normal {
            return 0;
        }
        self.combat
            .hostiles_within(self.combat_position(), HOSTILE_RANGE)
    }

    /// Whether a raider band is out at `field` now, as far as anyone
    /// here knows: one has gathered there this stretch of the calendar,
    /// and the player hasn't broken it.
    pub fn raiders_reported(&self, field: retrox_sim::FieldId) -> bool {
        let epoch = (self.t_days() / crate::combat::BAND_DAYS) as u64;
        let band = crate::combat::band(&self.galaxy, self.system, field, epoch);
        let killed = match self.combat.bands.0.get(&field) {
            Some(b) if b.epoch == epoch => b.killed,
            _ => 0,
        };
        (0..band.len()).any(|i| killed & (1 << i) == 0)
    }

    /// The ship as the fight sees it.
    pub fn player_craft(&self) -> Craft {
        let anchor = self.combat_anchor();
        let vel = if anchor == self.ship.frame {
            self.ship.vel
        } else {
            let t = self.t_days();
            self.ship.vel + self.velocity_at(self.ship.frame, t) - self.velocity_at(anchor, t)
        };
        Craft {
            pos: self.combat_position(),
            vel,
            rot: self.ship.rot,
            spin: self.ship.spin,
        }
    }

    pub(super) fn fight(&mut self, dt: f64, trigger: bool) {
        // Refits change the guns, shields and sections.
        if self.combat.player.fit != self.player.ship.fit {
            let lost = self.combat.lost;
            let bands = std::mem::take(&mut self.combat.bands);
            self.combat = Combat::new(
                self.player.ship.fit.clone(),
                self.player.ship.damage.clone(),
            );
            self.combat.bands = bands;
            self.combat.lost = lost;
        }

        // A field's band comes out when we arrive, and stays behind when
        // we go.
        let field = match self.mode {
            Mode::Normal => self.field_rocks.as_ref().map(|f| f.0),
            _ => None,
        };
        if field != self.combat.field {
            match field {
                Some(f) => {
                    // Fight in the field's frame, as the band does.
                    self.enter_frame(Place::Field(f));
                    let craft = Craft {
                        pos: self.ship.rel,
                        vel: self.ship.vel,
                        rot: self.ship.rot,
                        spin: self.ship.spin,
                    };
                    let day = self.t_days();
                    let alert = self
                        .combat
                        .arrive(&self.galaxy, self.system, f, day, &craft);
                    if let Some(text) = alert {
                        self.say(text);
                    }
                }
                None => self.combat.leave(),
            }
        }

        if self.mode != Mode::Normal {
            // Docked, landed or in transit: the ship cools and its shields
            // come back.
            self.combat.player.systems(dt, 0.0, false);
            return;
        }
        let pilot = Pilot {
            craft: self.player_craft(),
            burn: self.burn,
            trigger,
        };
        let solids = self.combat_solids(pilot.craft.pos);
        let out = self.combat.update(dt, pilot, &solids);
        self.player.ship.damage = self.combat.player.damage.clone();
        if let Some(f) = self.combat.field {
            for (pos, vel, good, tonnes, seed) in out.salvage {
                self.mining.drop_loose(f, pos, vel, good, tonnes, seed);
            }
        }
        if let Some(text) = out.messages.into_iter().last() {
            self.say(text);
        }
    }

    /// Rocks near the fight, anchor-relative, for raiders to steer round.
    fn combat_solids(&self, near: DVec3) -> Vec<(DVec3, f64)> {
        let Some((field, rocks)) = &self.field_rocks else {
            return Vec::new();
        };
        if self.combat.field != Some(*field) || self.combat.raiders.is_empty() {
            return Vec::new();
        }
        let close = |p: DVec3| (p - near).length() < 20e3;
        let mut out: Vec<(DVec3, f64)> = rocks
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.mining.is_broken(*field, *i))
            .map(|(_, r)| (rock_offset(r), r.radius_m))
            .filter(|(p, _)| close(*p))
            .collect();
        out.extend(self.mining.frag_solids(*field).filter(|(p, _)| close(*p)));
        out
    }

    /// Go to the nearest asteroid field with a raider band and park there,
    /// for the band to find (for testing). Returns whether there was one.
    pub fn start_by_raiders(&mut self) -> bool {
        let here = self.galaxy.systems[self.system].position;
        let mut systems: Vec<_> = self
            .galaxy
            .systems
            .iter()
            .map(|(id, s)| (id, here.distance(s.position)))
            .collect();
        systems.sort_by(|a, b| a.1.total_cmp(&b.1));
        let epoch = (self.t_days() / crate::combat::BAND_DAYS) as u64;
        let found = systems.into_iter().find_map(|(id, _)| {
            let fields = super::SystemView::new(&self.galaxy, id).fields;
            fields
                .into_iter()
                .find(|&f| !crate::combat::band(&self.galaxy, id, f, epoch).is_empty())
                .map(|f| (id, f))
        });
        let Some((system, field)) = found else {
            return false;
        };
        if system != self.system {
            self.arrive_jump(system);
        }
        self.park_in(field)
    }

    /// Bring a band in close, in front of the ship and already battered,
    /// so a fight is under way at once (for screenshots).
    pub fn stage_brawl(&mut self) -> bool {
        if !self.start_by_raiders() {
            return false;
        }
        self.update(0.02, super::Controls::default());
        // Look away from the rock we parked by, out into open space.
        self.ship.rot = super::look_rotation(self.ship.rel);
        self.cam_rot = self.ship.rot;
        let me = self.player_craft();
        let n = self.combat.raiders.len();
        for (i, r) in self.combat.raiders.iter_mut().enumerate() {
            let side = me.rot * DVec3::X * ((i as f64 - (n as f64 - 1.0) / 2.0) * 90.0);
            r.craft.pos = me.pos + me.forward() * (380.0 + 160.0 * i as f64) + side;
            r.craft.vel = me.forward() * -40.0;
            r.craft.rot = super::look_rotation(me.pos - r.craft.pos);
            r.vessel.shield = 0.0;
            let structure = r.vessel.parts[0].structure;
            r.vessel.hit(structure * 0.85, DVec3::ZERO);
        }
        // The first is already done for: it blows up as we watch.
        if let Some(id) = self.combat.raiders.first().map(|r| r.id) {
            let pilot = Pilot {
                craft: me,
                burn: 0.0,
                trigger: false,
            };
            self.combat.hit_raider(id, 1e6, &pilot);
        }
        self.message = None;
        true
    }

    /// Docked or landed: the yard repairs the ship, as far as the player
    /// can pay. The money goes to the settlement's treasury.
    pub(super) fn repair_docked(&mut self) {
        let Some(here) = self.docked_at() else {
            return;
        };
        let points = self.player.ship.damage.total();
        if points <= 0.0 {
            return;
        }
        let cost = points * REPAIR_PER_POINT;
        let paid = cost.min(self.player.credits.max(0.0));
        if paid <= 0.0 {
            return;
        }
        let share = paid / cost;
        self.player.credits -= paid;
        if let Some(s) = self.galaxy.settlements.get_mut(here) {
            s.economy.treasury += paid;
        }
        let d = &mut self.player.ship.damage;
        for s in &mut d.sections {
            *s *= 1.0 - share;
        }
        d.frame *= 1.0 - share;
        if share >= 1.0 {
            *d = Default::default();
        }
        self.combat = {
            let bands = std::mem::take(&mut self.combat.bands);
            let mut c = Combat::new(
                self.player.ship.fit.clone(),
                self.player.ship.damage.clone(),
            );
            c.bands = bands;
            c
        };
        let what = if share >= 1.0 {
            "Hull repaired"
        } else {
            "Hull partly repaired: out of money"
        };
        self.say(format!("{what} · {}", crate::hud::credits(paid)));
    }
}
