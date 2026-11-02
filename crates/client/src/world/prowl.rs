//! Pirates near the player (DESIGN §7.2.2): the sim's gangs flying in this
//! system, as the client meets them.
//!
//! - **Spotted.** A gang that isn't resting after a fight, within
//!   [`pirates::HUNT_M`] of the player in space, goes after them. The client
//!   flies a stalk, since only it knows where the player is between ticks:
//!   full burn at the lead point, the same flight model as everyone's
//!   fast travel.
//! - **Interdicted.** A stalking gang that comes within [`INTERDICT_M`] of a
//!   ship in fast travel pulls it out: the ship is dragged down to a few
//!   hundred m/s and the gang's ships come in from where it was. At 1:1 the
//!   gang closes in for real and its ships come out at [`ENGAGE_M`].
//! - **At home.** A ship that comes to a gang's field while it's there
//!   meets the whole gang.
//! - **After.** Ships destroyed are paid for by the faction with a bounty on
//!   them and written back to the sim when the fight ends; the rest fly
//!   home and keep away for a few days.

use glam::DVec3;
use retrox_sim::flight::Kinematics;
use retrox_sim::pirates::{self, Aim, GangId};
use retrox_sim::{FactionId, FieldId};

use super::{Mode, Place, World};
use crate::combat::LEASH;
use crate::standing::{self, Rank};
use crate::traffic::{to_client, to_sim};

/// A stalking gang this close to a ship in fast travel pulls it out.
pub const INTERDICT_M: f64 = 4e6;
/// At 1:1, a stalking gang's ships come out this close.
pub const ENGAGE_M: f64 = 30e3;
/// Speed a ship is left with after an interdiction, m/s.
const INTERDICTED_SPEED: f64 = 250.0;
/// Slices a frame of stalking is flown in.
const STALK_SLICES: usize = 24;

impl World {
    /// Where the player is, for the sim's gangs: in space in this system,
    /// or nowhere they can reach.
    fn tell_pirates(&mut self) {
        let flying =
            matches!(self.mode, Mode::Normal | Mode::FastTravel(_)) && self.combat.lost.is_none();
        self.galaxy.pirates.player = flying.then(|| {
            let t = self.t_days();
            let vel = self.ship.vel + self.velocity_at(self.ship.frame, t);
            (
                self.system,
                Kinematics {
                    pos: to_sim(self.ship_position()),
                    vel: to_sim(vel),
                },
            )
        });
    }

    /// One frame of pirates near the player: spot, stalk, interdict,
    /// engage. `from` is where the ship was at the start of the frame and
    /// `clock0` the galaxy clock then.
    pub(super) fn prowl(&mut self, from: DVec3, clock0: f64) {
        self.tell_pirates();
        self.report_robberies();
        let Some((_, me)) = self.galaxy.pirates.player else {
            return;
        };
        let today = self.galaxy.time;
        let elapsed = (self.clock_s - clock0).max(0.0);
        let here: Vec<GangId> = self
            .galaxy
            .pirates
            .prowls
            .keys()
            .copied()
            .filter(|g| {
                self.galaxy
                    .pirates
                    .gangs
                    .get(*g)
                    .is_some_and(|gang| gang.lair == self.system)
            })
            .collect();
        for id in here {
            let Some(&prowl) = self.galaxy.pirates.prowls.get(&id) else {
                continue;
            };
            match prowl.aim {
                Aim::Engaged => {}
                Aim::Lurk | Aim::Return | Aim::Hunt(_) => {
                    if today < prowl.rest_until || self.combat.gang.is_some() {
                        continue;
                    }
                    // Coming to their own field, we meet them there.
                    let field = self.galaxy.pirates.gangs[id].field;
                    if self.field_rocks.as_ref().is_some_and(|(f, _)| *f == field) {
                        continue;
                    }
                    let Some(at) = self.traffic.pirate(id) else {
                        continue;
                    };
                    let d = (at.pos - to_client(me.pos)).length();
                    if d < pirates::HUNT_M {
                        pirates::stalk(&mut self.galaxy, id);
                        if let Some(p) = self.galaxy.pirates.prowls.get_mut(&id) {
                            p.ship = Kinematics {
                                pos: to_sim(at.pos),
                                vel: to_sim(at.vel),
                            };
                        }
                        let name = self.galaxy.pirates.gangs[id].name.clone();
                        self.say(format!(
                            "Contact: the {name} are moving to intercept · {:.2} AU",
                            d / 1.496e11
                        ));
                    }
                }
                Aim::Stalk => self.stalk(id, from, elapsed),
            }
        }
    }

    /// Fly a stalking gang through this frame's `elapsed` galaxy seconds,
    /// toward the ship that went from `from` to where it is now; pull it
    /// out of fast travel, or bring the gang's ships out, if they get close.
    fn stalk(&mut self, id: GangId, from: DVec3, elapsed: f64) {
        if self.combat.gang.is_some() {
            return;
        }
        let Some(gang) = self.galaxy.pirates.gangs.get(id) else {
            return;
        };
        let accel = self.pirate_drive(gang);
        let to = self.ship_position();
        let vel = (to - from) / elapsed.max(1e-9);
        let mut ship = self.galaxy.pirates.prowls[&id].ship;
        let mut pos = to_client(ship.pos);
        let mut v = to_client(ship.vel);
        let mut closest = (pos - from).length();
        if elapsed > 0.0 {
            let dt = elapsed / STALK_SLICES as f64;
            for k in 0..STALK_SLICES {
                let u0 = k as f64 / STALK_SLICES as f64;
                let u = (k + 1) as f64 / STALK_SLICES as f64;
                let target = from + (to - from) * u;
                // Close along the line of sight while matching the ship's
                // velocity across it, so the line doesn't swing: no
                // braking, since meeting it is the point.
                let rel = target - pos;
                let los = rel.normalize_or(DVec3::X);
                let want = vel + los * (2.0 * accel * rel.length()).sqrt();
                let dir = (want - v).normalize_or(los);
                let p0 = pos;
                v += dir * accel * dt;
                pos += v * dt;
                // How close the two came over the slice, both moving
                // straight: at warp a slice is thousands of km.
                let r0 = from + (to - from) * u0 - p0;
                let r1 = target - pos;
                closest = closest.min(segment_to_origin(r0, r1));
            }
        }
        ship.pos = to_sim(pos);
        ship.vel = to_sim(v);
        if let Some(p) = self.galaxy.pirates.prowls.get_mut(&id) {
            p.ship = ship;
        }
        match self.mode {
            Mode::FastTravel(_) if closest < INTERDICT_M => self.interdict(id, pos),
            Mode::Normal if closest < ENGAGE_M => {
                let anchor = self.combat_origin();
                let craft = self.player_craft();
                self.engage(id, None, pos - anchor, craft.pos);
            }
            _ => {}
        }
    }

    /// The gang `id`, at `at` (system frame), pulls the ship out of fast
    /// travel and comes in for the kill.
    fn interdict(&mut self, id: GangId, at: DVec3) {
        let pos = self.ship_position();
        self.mode = Mode::Normal;
        self.ship.frame = Place::Star;
        self.ship.rel = pos - self.position(Place::Star);
        let dir = (self.ship.vel).normalize_or(self.ship.rot * DVec3::NEG_Z);
        self.ship.vel = dir * INTERDICTED_SPEED;
        self.ship.throttle = 0.0;
        self.ship.spin = DVec3::ZERO;
        let name = self.galaxy.pirates.gangs[id].name.clone();
        let anchor = self.combat_origin();
        let craft = self.player_craft();
        let from = if (at - pos).length() > 1.0 {
            pos + (at - pos).normalize() * 10e3
        } else {
            pos + self.ship.rot * DVec3::NEG_Z * 10e3
        };
        self.engage(id, None, from - anchor, craft.pos);
        self.say(format!(
            "INTERDICTED · the {name} pulled us out of fast travel"
        ));
        self.interdicted = Some(0.0);
    }

    /// The fast-travel drive a gang's slowest ship has.
    fn pirate_drive(&self, gang: &pirates::Gang) -> f64 {
        gang.ships
            .iter()
            .filter_map(|h| retrox_content::ships::stock(h))
            .map(|fit| super::pilot::drive_accel_of(&fit.stats()))
            .fold(f64::INFINITY, f64::min)
            .min(1e3)
    }

    /// Bring `gang`'s ships out: at `field` if it's there, coming in from
    /// `from` and guarding `home` (anchor-relative).
    pub(super) fn engage(&mut self, id: GangId, field: Option<FieldId>, from: DVec3, home: DVec3) {
        let Some(gang) = self.galaxy.pirates.gangs.get(id) else {
            return;
        };
        let hulls: Vec<(u32, String)> = gang
            .ships
            .iter()
            .enumerate()
            .map(|(i, h)| (i as u32, h.clone()))
            .collect();
        let name = gang.name.clone();
        let seed = slotmap::Key::data(&id).as_ffi()
            ^ self.galaxy.time.days().wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (hulls.len() as u64) << 56;
        self.combat.field = field;
        let craft = self.player_craft();
        let alert = self
            .combat
            .bring_out(id, &name, &hulls, seed, field, &craft, from, home);
        pirates::engage(&mut self.galaxy, id);
        if let Some(text) = alert {
            self.say(text);
        }
    }

    /// The fight with the gang that's out is over, one way or another:
    /// tell the sim who died, and send the rest home.
    pub(super) fn end_fight(&mut self) {
        let Some(id) = self.combat.gang.take() else {
            return;
        };
        let killed: Vec<usize> = self.combat.killed.drain(..).map(|k| k as usize).collect();
        let at = Kinematics {
            pos: to_sim(self.ship_position()),
            vel: to_sim(self.ship.vel + self.velocity_at(self.ship.frame, self.t_days())),
        };
        self.combat.raiders.clear();
        pirates::fought(&mut self.galaxy, id, &killed, Some(at));
    }

    /// Raider `ship` of the gang that's out was destroyed: the bounty is
    /// paid, and the factions who wanted it dead think better of us.
    pub(super) fn raider_killed(&mut self, ship: u32) {
        let Some(id) = self.combat.gang else {
            return;
        };
        let local = self.galaxy.largest_faction_in(self.system);
        let paid = pirates::claim_bounty(&mut self.galaxy, id, ship as usize);
        let mut said = Vec::new();
        if let Some((faction, amount)) = paid {
            self.player.credits += amount;
            self.player.progress.earned = true;
            let name = self.faction_name(faction);
            said.push(format!(
                "Bounty paid · {} from the {name}",
                crate::hud::credits(amount)
            ));
            if let Some(rank) = self.player.standing.change(faction, standing::BOUNTY_KILL) {
                said.push(rank_news(&name, rank));
            }
        }
        if let Some(f) = local.filter(|&f| paid.is_none_or(|(p, _)| p != f))
            && let Some(rank) = self.player.standing.change(f, standing::PIRATE_KILL)
        {
            said.push(rank_news(&self.faction_name(f), rank));
        }
        if !said.is_empty() {
            self.say(said.join(" · "));
        }
    }

    pub fn faction_name(&self, f: FactionId) -> String {
        self.galaxy
            .factions
            .get(f)
            .map_or_else(|| "a faction gone".to_owned(), |f| f.name.clone())
    }

    /// Mayday calls from traders robbed in this system.
    fn report_robberies(&mut self) {
        let robberies = std::mem::take(&mut self.galaxy.pirates.robberies);
        let system = self.system;
        for r in robberies.into_iter().filter(|r| r.system == system) {
            let company = self
                .galaxy
                .companies
                .get(r.company)
                .map_or("a trader", |c| c.name.as_str())
                .to_owned();
            let gang = self
                .galaxy
                .pirates
                .gangs
                .get(r.gang)
                .map_or("pirates", |g| g.name.as_str())
                .to_owned();
            let what = match r.good {
                Some(g) if r.tonnes > 0.0 => {
                    format!(", {:.0} t of {} taken", r.tonnes, g.name().to_lowercase())
                }
                _ => String::new(),
            };
            self.say(format!(
                "Mayday from {company}: boarded by the {gang}{what}"
            ));
        }
    }

    /// The fight is far behind: the gang has gone home.
    pub(super) fn fight_left_behind(&self) -> bool {
        self.combat.gang.is_some()
            && (self.combat.raiders.is_empty()
                || self
                    .combat
                    .raiders
                    .iter()
                    .all(|r| (r.home - self.combat_position()).length() > LEASH * 2.0))
    }

    /// A loaded game has no fight going: gangs that were in one, or after
    /// the player, head home.
    pub(super) fn settle_prowls(&mut self) {
        let today = self.galaxy.time;
        for p in self.galaxy.pirates.prowls.values_mut() {
            if matches!(p.aim, Aim::Engaged | Aim::Stalk) {
                p.aim = Aim::Return;
                p.since = today;
            }
        }
    }

    /// Whether `settlement` lets us dock or land; says why not if it
    /// doesn't.
    pub(super) fn cleared(&mut self, settlement: retrox_sim::SettlementId) -> bool {
        let Some(owner) = self.galaxy.settlements.get(settlement).map(|s| s.owner) else {
            return true;
        };
        if self.welcome(owner) {
            return true;
        }
        let name = self.faction_name(owner);
        self.say(format!("Clearance refused: the {name} count us hostile"));
        false
    }

    /// Whether we may dock or land at settlements of `faction`.
    pub fn welcome(&self, faction: FactionId) -> bool {
        !self.player.standing.hostile(faction)
    }
}

/// The least distance from the origin to the segment `a`–`b`.
fn segment_to_origin(a: DVec3, b: DVec3) -> f64 {
    let d = b - a;
    let len2 = d.length_squared();
    let t = if len2 > 0.0 {
        (-a.dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t).length()
}

/// What the HUD says when standing crosses into a new rank.
pub fn rank_news(faction: &str, rank: Rank) -> String {
    match rank {
        Rank::Hostile => format!("The {faction} now count us hostile"),
        r => format!("The {faction} now count us {}", r.name().to_lowercase()),
    }
}
