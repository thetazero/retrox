//! The asteroid field around the ship: its rocks, the mining laser and
//! the prospector.

use glam::DVec3;
use retrox_gen::rocks;

use super::{Mode, Place, World};
use crate::mining;

impl World {
    pub(super) fn update_field(&mut self) {
        let pos = self.ship_position();
        let inside = self.view.fields.iter().copied().find(|&f| {
            (self.position(Place::Field(f)) - pos).length()
                < self.place_radius(Place::Field(f)) + 50e3
        });
        match (inside, &self.field_rocks) {
            (Some(f), Some((current, _))) if f == *current => {}
            (Some(f), _) => self.field_rocks = Some((f, rocks(&self.galaxy.fields[f]))),
            (None, Some((left, _))) => {
                self.mining.leave(*left);
                self.field_rocks = None;
            }
            (None, None) => {}
        }
    }

    /// Where the mining laser's muzzle is, in ship space (metres, nose
    /// toward -Z): the first fitted mining laser's slot, else the nose.
    pub fn laser_muzzle(&self) -> DVec3 {
        use retrox_sim::ship::Function;
        self.player
            .ship
            .fit
            .mounts()
            .filter_map(|(_, s)| s)
            .flat_map(|s| s.slots())
            .find_map(|(slot, module)| {
                matches!(module?.function, Function::MiningLaser { .. }).then(|| {
                    let [x, y, z] = slot.position_m;
                    DVec3::new(x, y, -z)
                })
            })
            .unwrap_or(DVec3::new(0.0, 0.0, -self.stats().length_m / 2.0))
    }

    /// One frame of the mining laser and the tractor.
    pub(super) fn mine(&mut self, dt: f64, fire: bool) {
        let stats = self.stats();
        self.beam = None;
        let Some((field, rocks)) = &self.field_rocks else {
            if let Some(text) = self
                .mining
                .idle(dt, &stats, fire && self.mode == Mode::Normal)
            {
                self.say(text);
            }
            if self.mining.firing {
                let muzzle = self.ship.rot * self.laser_muzzle();
                let forward = self.ship.rot * DVec3::NEG_Z;
                self.beam = Some((muzzle, muzzle + forward * stats.mining_range_m));
            }
            return;
        };
        let field = *field;
        let centre = self.position(Place::Field(field));
        let ship = self.ship_position() - centre;
        let muzzle = self.ship.rot * self.laser_muzzle();
        let forward = self.ship.rot * DVec3::NEG_Z;
        let miner = mining::Miner {
            nose: ship + muzzle,
            forward,
            centre: ship,
            reach: stats.length_m * 0.6 + 6.0,
            free_t: (stats.cargo_t - self.player.cargo_t()).max(0.0),
            firing: fire && self.mode == Mode::Normal,
        };
        let out = self.mining.update(
            dt,
            field,
            &self.galaxy.fields[field],
            rocks,
            self.clock_s,
            &stats,
            miner,
        );
        for (good, t) in out.collected {
            self.player.cargo[good] += t;
            // Ice taken strains the field as NPC ice crews' does (#68).
            if good == retrox_sim::economy::Good::Ice {
                retrox_sim::harvest::strain(
                    &mut self.galaxy,
                    retrox_sim::harvest::Source::Field(field),
                    t,
                );
            }
        }
        self.ship.vel += out.kick;
        if let Some(text) = out.messages.into_iter().last() {
            self.say(text);
        }
        if self.mining.firing {
            let end = match self.mining.aim {
                Some(aim) if aim.distance <= stats.mining_range_m => aim.point - ship,
                _ => muzzle + forward * stats.mining_range_m,
            };
            self.beam = Some((muzzle, end));
        }
    }

    pub(super) fn scan(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        let Some((field, rocks)) = &self.field_rocks else {
            self.say("Nothing to prospect: fly into an asteroid field");
            return;
        };
        let centre = self.position(Place::Field(*field));
        let from = self.ship_position() - centre;
        let found = self.mining.scan(
            *field,
            &self.galaxy.fields[*field],
            rocks,
            self.clock_s,
            from,
        );
        if let Some(n) = found {
            self.say(format!("Prospector: {n} rocks read"));
        }
    }
}
