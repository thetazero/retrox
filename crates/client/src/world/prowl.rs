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
use retrox_sim::flight::{self, Goal, Kinematics};
use retrox_sim::pirates::{self, Aim, GangId};
use retrox_sim::{FactionId, FieldId};

use super::{Mode, Place, World};
use crate::combat::LEASH;
use crate::standing;
use crate::traffic::{to_client, to_sim};

/// A stalking gang this close to a ship in fast travel pulls it out.
pub const INTERDICT_M: f64 = 4e6;
/// At 1:1, a stalking gang's ships come out this close.
pub const ENGAGE_M: f64 = 30e3;
/// Going after a gang, the ship comes to rest this far short of it.
const GANG_STANDOFF_M: f64 = 10e3;
/// Speed a ship is left with after an interdiction, m/s.
const INTERDICTED_SPEED: f64 = 250.0;

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
                    // A gang at home in the field we're in comes out there
                    // (see `World::fight`); one out flying spots us in its
                    // field as anywhere else (#171).
                    let field = self.galaxy.pirates.gangs[id].field;
                    // In the field by where we are, as `update_field` will
                    // find later this frame: on the frame we arrive,
                    // `field_rocks` doesn't have it yet.
                    let centre = self.position(Place::Field(field));
                    let in_field = (centre - self.ship_position()).length()
                        < self.place_radius(Place::Field(field)) + 50e3;
                    if self.mode == Mode::Normal && in_field && self.gang_home(id, field) {
                        continue;
                    }
                    // Nor does a gang at home fly out to meet a ship bound
                    // for its field: it waits, and the ship arrives among
                    // it. Flying out, it matched the ship's speed and was
                    // left millions of km off when the ship stopped (#171).
                    let bound_here = matches!(
                        self.mode,
                        Mode::FastTravel(super::FastTravel {
                            leg: super::Leg::To(Place::Field(f)),
                            ..
                        }) if f == field
                    );
                    if bound_here && prowl.aim == Aim::Lurk {
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
                            "Contact: the {name} are moving to intercept · {}",
                            crate::hud::distance(d)
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
    ///
    /// The gang flies the shared autopilot (DESIGN §7.2.2), coming to the
    /// ship and matching its velocity: a frame of fast travel is hours of
    /// galaxy time, and a stalk that only closed, never braking, flew past
    /// by hundreds of thousands of km and was left far off and receding
    /// when the ship stopped (issue #171).
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
        let mut closest = (to_client(ship.pos) - from).length();
        if elapsed > 0.0 {
            // The ship's track over the frame, straight from `from` to `to`.
            let prey = |t: f64| from + vel * t;
            let mut last = (0.0, to_client(ship.pos));
            let arrived = flight::fly(&mut ship, accel, elapsed, |t, k| {
                // How close the two came over the step just flown, both
                // moving straight: at warp a step can be thousands of km.
                let (t0, p0) = last;
                let p = to_client(k.pos);
                closest = closest.min(segment_to_origin(prey(t0) - p0, prey(t) - p));
                last = (t, p);
                Goal {
                    pos: to_sim(prey(t)),
                    vel: to_sim(vel),
                }
            });
            if arrived.is_some() {
                // On the ship: it rides along for the rest of the frame.
                ship = Kinematics {
                    pos: to_sim(to),
                    vel: to_sim(vel),
                };
                closest = 0.0;
            }
        }
        let pos = to_client(ship.pos);
        if let Some(p) = self.galaxy.pirates.prowls.get_mut(&id) {
            p.ship = ship;
        }
        match self.mode {
            Mode::FastTravel(_) if closest < INTERDICT_M => self.interdict(id, pos),
            Mode::Normal if closest < ENGAGE_M => {
                // Into the field we're in, if we're in one: a fight
                // brought out with no field there was ended the next frame,
                // the gang sent home resting as if it had fought (#171).
                let anchor = self.combat_origin();
                let craft = self.player_craft();
                let field = self.combat.field;
                self.engage(id, field, pos - anchor, craft.pos);
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
        // The card says it; the comms line keeps "Raiders!".
        self.interdicted = Some((0.0, name));
    }

    /// Where gang `id` is at galaxy day `t`, and how it's moving, as far
    /// as anyone can predict it from here: straight on from where it is.
    pub(super) fn gang_at_day(&self, id: GangId, t: f64) -> Option<(DVec3, DVec3)> {
        let c = self.traffic.pirate(id)?;
        let ahead = (t - self.t_days()) * super::SECONDS_PER_DAY;
        Some((c.pos + c.vel * ahead, c.vel))
    }

    /// Where going after gang `id` comes to rest at galaxy day `t`:
    /// [`GANG_STANDOFF_M`] short of its ships, on our side, matching them.
    pub(super) fn gang_goal(&self, id: GangId, ship: DVec3, t: f64) -> Goal {
        let Some((pos, vel)) = self.gang_at_day(id, t) else {
            // Lost it (it's in a fight, or broken up): stop where we are.
            return Goal {
                pos: to_sim(ship),
                vel: to_sim(self.ship.vel + self.velocity_at(self.ship.frame, t)),
            };
        };
        let back = (ship - pos).normalize_or(DVec3::Z);
        Goal {
            pos: to_sim(pos + back * GANG_STANDOFF_M),
            vel: to_sim(vel),
        }
    }

    /// Go after the gang `id` ([I] with its red diamond in the sights, or
    /// in its lair): fast travel to it, or, within [`ENGAGE_M`], bring its
    /// ships out at once. A gang not resting comes for us on the way and
    /// pulls us out of fast travel; one lying low after a fight is caught
    /// where it is (DESIGN §7.2.2, #171).
    pub(super) fn go_after_gang(&mut self, id: GangId) {
        let Some(c) = self.traffic.pirate(id) else {
            return;
        };
        let name = c.name.clone();
        if (c.pos - self.ship_position()).length() < ENGAGE_M {
            self.catch_gang(id);
            return;
        }
        self.say(format!("Going after the {name}"));
        self.start_fast_travel(super::Leg::Gang(id));
    }

    /// We've come up on gang `id`: its ships come out.
    pub(super) fn catch_gang(&mut self, id: GangId) {
        if self.combat.gang.is_some() {
            return;
        }
        let Some(at) = self.traffic.pirate(id).map(|c| c.pos) else {
            self.say("The gang is gone");
            return;
        };
        let anchor = self.combat_origin();
        let craft = self.player_craft();
        let field = self.combat.field;
        self.engage(id, field, at - anchor, craft.pos);
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
        self.combat
            .raiders
            .retain(|r| r.crew != crate::combat::Crew::Pirate);
        pirates::fought(&mut self.galaxy, id, &killed, Some(at));
    }

    /// Raider `ship` of the gang that's out was destroyed: the bounty is
    /// paid, and the factions who wanted it dead think better of us.
    pub(super) fn raider_killed(&mut self, ship: u32) {
        let Some(id) = self.combat.gang else {
            return;
        };
        let local = self.galaxy.largest_faction_in(self.system);
        // The warrant's faction is glad of it even when its treasury
        // can't pay (§7.2.2: +3 for a ship with its warrant on it).
        let warrant = (self.galaxy.pirates.gangs.get(id))
            .and_then(|g| g.bounty)
            .map(|b| b.faction);
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
        }
        if let Some(faction) = warrant
            && let Some(rank) = self.player.standing.change(faction, standing::BOUNTY_KILL)
        {
            said.push(crate::standing::news(&self.faction_name(faction), rank));
        }
        if let Some(f) = local.filter(|&f| warrant != Some(f))
            && let Some(rank) = self.player.standing.change(f, standing::PIRATE_KILL)
        {
            said.push(crate::standing::news(&self.faction_name(f), rank));
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
        if self.combat.gang.is_none() {
            return false;
        }
        let me = self.combat_position();
        // Only the gang's ships count: traders caught up in it don't.
        self.combat
            .raiders
            .iter()
            .filter(|r| r.crew == crate::combat::Crew::Pirate)
            .all(|r| (r.home - me).length() > LEASH * 2.0)
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

impl World {
    /// Fly at the nearest gang's lair from a third of an AU out until it
    /// pulls the ship out of fast travel (for screenshots). Returns whether
    /// it did.
    pub fn stage_interdiction(&mut self) -> bool {
        if !self.start_by_raiders() {
            return false;
        }
        let Place::Field(field) = self.ship.frame else {
            return false;
        };
        let centre = self.position(Place::Field(field));
        self.combat.leave();
        self.field_rocks = None;
        self.mode = Mode::Normal;
        self.ship.frame = Place::Star;
        for side in [1.0, -1.0] {
            self.ship.rel = centre + DVec3::new(side * 0.3 * 1.496e11, 0.006e11, 0.0);
            if self.current_well().is_none() {
                break;
            }
        }
        self.ship.vel = DVec3::ZERO;
        self.target = Some(Place::Field(field));
        self.update(1.0 / 60.0, super::Controls::default());
        self.act(super::Action::FastTravel);
        for _ in 0..60 * 20 {
            if !matches!(self.mode, Mode::FastTravel(_)) {
                break;
            }
            self.update(1.0 / 60.0, super::Controls::default());
        }
        // A moment later, as the band slams in.
        for _ in 0..20 {
            self.update(1.0 / 60.0, super::Controls::default());
        }
        self.interdicted.is_some()
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
