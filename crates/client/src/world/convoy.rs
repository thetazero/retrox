//! Faction ships near the player (DESIGN §7.2.2): the bubble's trader
//! fleets become ships in the fight's scene when the player comes close,
//! so they can be shot, and so a faction that counts the player hostile can
//! shoot back.
//!
//! - **Intercept.** [I] fast travels to the trader fleet nearest the
//!   crosshair and comes to rest alongside it, matching its course: the
//!   start of the pirate's trade (interdicting on the lanes).
//! - **Close by,** within [`CONVOY_M`], each of a fleet's ships is a vessel
//!   holding its place in the formation, peaceful unless its faction counts
//!   the player hostile.
//! - **Shot,** the whole fleet turns: armed ships fight, the rest run. Every
//!   hit costs standing with its faction and destroying a ship costs far
//!   more; a few hits make the faction hostile (see [`crate::standing`]).
//! - **Destroyed,** a ship is gone from the sim's fleet with its share of the
//!   cargo, and the tractor takes what it can of that share into the hold.

use std::collections::HashMap;

use glam::{DQuat, DVec3};
use retrox_sim::bubble::FleetKey;
use retrox_sim::{FactionId, haul};

use super::{Leg, Mode, Place, World};
use crate::combat::{Crew, TRADER_IDS};
use crate::standing;
use crate::traffic::{Contact, formation};

/// A trader fleet's ships join the scene this close.
pub const CONVOY_M: f64 = 12e3;
/// And leave it, if still peaceful, this far off.
const CONVOY_DROP_M: f64 = 25e3;
/// Seconds between complaints from one fleet: a burst of fire is one
/// offence, not twenty.
const COMPLAINT_S: f64 = 1.5;
/// Fast travel to a fleet stops this far off its quarter.
const ALONGSIDE_M: f64 = 2500.0;

impl World {
    /// The faction a trader fleet flies for.
    pub fn fleet_faction(&self, fleet: FleetKey) -> Option<FactionId> {
        let company = self.galaxy.companies.get(fleet.0)?;
        Some(self.galaxy.settlements.get(company.home)?.owner)
    }

    /// Where each ship of contact `c` flies: anchor-relative position,
    /// velocity and heading.
    fn slots(&self, c: &Contact) -> Vec<(DVec3, DVec3, DQuat)> {
        let anchor = self.combat_anchor();
        let t = self.t_days();
        let origin = self.position(anchor);
        let frame_vel = self.velocity_at(anchor, t);
        let burn = (c.thrust.length() / c.accel.max(1e-9)).clamp(0.0, 1.0);
        let heading = if burn > 0.02 { c.thrust } else { c.vel };
        let rot = super::look_rotation(heading.normalize_or(DVec3::NEG_Z));
        let seed = slotmap::Key::data(&c.key.0).as_ffi() ^ u64::from(c.key.1);
        let length = c.fit.stats().length_m;
        formation(c.ships, length, seed)
            .into_iter()
            .map(|o| (c.pos + rot * o - origin, c.vel - frame_vel, rot))
            .collect()
    }

    /// Bring trader fleets near the ship into the scene, keep peaceful
    /// ones in formation, and let the far ones fly on.
    pub(super) fn convoys(&mut self) {
        let flying = self.mode == Mode::Normal && self.combat.lost.is_none();
        let me = self.ship_position();
        let contacts = self.traffic.contacts.clone();
        for fleet in self.combat.convoys() {
            let near = contacts
                .iter()
                .find(|c| c.key == fleet)
                .is_some_and(|c| (c.pos - me).length() < CONVOY_DROP_M);
            if !flying || !near {
                self.combat.drop_convoy(fleet);
            }
        }
        if !flying {
            return;
        }
        for c in &contacts {
            if (c.pos - me).length() > CONVOY_DROP_M {
                continue;
            }
            let slots = self.slots(c);
            if self.combat.has_convoy(c.key) {
                self.combat.set_slots(c.key, &slots);
            } else if (c.pos - me).length() < CONVOY_M {
                let Some(faction) = self.fleet_faction(c.key) else {
                    continue;
                };
                let hostile = self.player.standing.hostile(faction);
                self.combat
                    .join_convoy(c.key, faction, &c.fit, &slots, hostile);
                if hostile {
                    let name = self.faction_name(faction);
                    self.say(format!(
                        "{} ({}) count us hostile: weapons free",
                        c.company, name
                    ));
                }
            }
        }
    }

    /// The player hit raider `id`: if it's a trader, its faction counts it.
    /// Opening fire on a peaceful ship is an attack on its faction.
    pub(super) fn ship_hit(
        &mut self,
        id: u32,
        peaceful: bool,
        complaints: &mut HashMap<FleetKey, f64>,
    ) {
        if id < TRADER_IDS {
            return;
        }
        let Some(Crew::Trader { fleet, faction }) = self.combat.raider(id).map(|r| r.crew) else {
            return;
        };
        let now = self.clock_s;
        if !peaceful
            && complaints
                .get(&fleet)
                .is_some_and(|&t| now - t < COMPLAINT_S)
        {
            return;
        }
        complaints.insert(fleet, now);
        let name = self.faction_name(faction);
        let cost = if peaceful {
            standing::ATTACKED
        } else {
            standing::SHOT_SHIP
        };
        if let Some(rank) = self.player.standing.change(faction, cost) {
            self.say(super::prowl::rank_news(&name, rank));
        }
    }

    /// The player destroyed trader ship `id` of `fleet`: the sim loses it,
    /// its faction remembers, and the tractor takes what it can of the
    /// cargo.
    pub(super) fn trader_killed(&mut self, fleet: FleetKey, faction: FactionId) {
        let lost = haul::ship_destroyed(&mut self.galaxy, fleet.0, fleet.1);
        let mut said = vec!["Trader destroyed".to_owned()];
        if let Some(cargo) = lost {
            let free = (self.stats().cargo_t - self.player.cargo_t()).max(0.0);
            let took = cargo.amount.min(free) * 0.8;
            if took > 0.05 {
                self.player.cargo[cargo.good] += took;
                said.push(format!(
                    "tractor took {took:.0} t of {}",
                    cargo.good.name().to_lowercase()
                ));
            }
        }
        let name = self.faction_name(faction);
        if let Some(rank) = self.player.standing.change(faction, standing::KILLED_SHIP) {
            said.push(super::prowl::rank_news(&name, rank));
        }
        self.say(said.join(" · "));
    }

    /// The ship as saved: a convoy's frame isn't (it's rebuilt from the
    /// traffic, which a load doesn't have yet), so it's saved in the
    /// star's.
    pub fn saved_ship(&self) -> super::Ship {
        let mut ship = self.ship.clone();
        if let Place::Convoy(_) = ship.frame {
            let t = self.t_days();
            ship.rel = self.ship_position() - self.position(Place::Star);
            ship.vel = self.ship.vel + self.velocity_at(ship.frame, t);
            ship.frame = Place::Star;
        }
        ship
    }

    /// Flying in a convoy's frame: remember where it is, and if it's gone
    /// (docked, jumped, destroyed) carry on in the star's frame from where
    /// it was last seen.
    pub(super) fn keep_convoy_frame(&mut self) {
        let Place::Convoy(fleet) = self.ship.frame else {
            self.convoy_seen = None;
            return;
        };
        match self.traffic.contacts.iter().find(|c| c.key == fleet) {
            Some(c) => self.convoy_seen = Some((fleet, c.pos, c.vel, self.traffic.at_s)),
            None => {
                self.enter_frame(Place::Star);
                self.convoy_seen = None;
            }
        }
    }

    /// Fast travel to the trader fleet nearest the crosshair (or, if none
    /// is near it, nearest the ship).
    pub(super) fn intercept(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        if self.hostiles_near() > 0 {
            self.say("Can't intercept: hostiles within 20 km");
            return;
        }
        if let Some(well) = self.current_well() {
            let name = self.name(well);
            self.say(format!("Can't intercept inside the gravity well of {name}"));
            return;
        }
        let me = self.ship_position();
        let ahead = self.cam_rot * DVec3::NEG_Z;
        let pick = |c: &&Contact| {
            let to = c.pos - me;
            ahead.angle_between(to.normalize_or(ahead))
        };
        let contacts = &self.traffic.contacts;
        let aimed = contacts
            .iter()
            .filter(|c| pick(c) < 0.3)
            .min_by(|a, b| pick(a).total_cmp(&pick(b)));
        let nearest = || {
            contacts
                .iter()
                .min_by(|a, b| (a.pos - me).length().total_cmp(&(b.pos - me).length()))
        };
        let Some(c) = aimed.or_else(nearest).cloned() else {
            self.say("No traffic to intercept");
            return;
        };
        if (c.pos - me).length() < CONVOY_M {
            self.say(format!("Already alongside {}", c.company));
            return;
        }
        self.say(format!("Intercepting {}", c.company));
        self.start_fast_travel(Leg::Intercept(c.key));
    }

    /// Where an intercept comes to rest at galaxy day `t`: off the fleet's
    /// quarter, matching it, as far as anyone can predict it from here.
    pub(super) fn intercept_goal(
        &self,
        fleet: FleetKey,
        ship: DVec3,
        t: f64,
    ) -> retrox_sim::flight::Goal {
        let Some(c) = self.traffic.contacts.iter().find(|c| c.key == fleet) else {
            // Lost it: stop where we are.
            return retrox_sim::flight::Goal {
                pos: ship.to_array(),
                vel: (self.ship.vel + self.velocity_at(self.ship.frame, t)).to_array(),
            };
        };
        let ahead = (t - self.t_days()) * super::SECONDS_PER_DAY;
        let pos = c.pos + c.vel * ahead;
        let back = -c.vel.normalize_or(DVec3::X);
        let side = back.cross(DVec3::Y).normalize_or(DVec3::Z);
        retrox_sim::flight::Goal {
            pos: (pos + (back + side * 0.5).normalize() * ALONGSIDE_M).to_array(),
            vel: c.vel.to_array(),
        }
    }
}
