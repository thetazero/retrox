//! Traffic: the bubble's trader fleets flying in the current system
//! (DESIGN §5.2), as the client sees them.
//!
//! The sim flies each fleet a galaxy day at a time. Between ticks the
//! client carries on the same flight with the same autopilot toward the
//! same goal, in small slices, so a fleet moves smoothly at 1:1 and under
//! fast travel alike. Each day the sim's state takes over again.

use std::collections::HashMap;

use glam::DVec3;
use retrox_sim::bubble::{self, FleetKey};
use retrox_sim::economy::Good;
use retrox_sim::flight::{self, Kinematics};
use retrox_sim::pirates::{self, Aim, GangId};
use retrox_sim::ship::Loadout;
use retrox_sim::{GalaxyState, SystemId};

use crate::world::SECONDS_PER_DAY;

/// A fleet in view: where it is, which way it's burning, and whose it is.
#[derive(Clone, Debug)]
pub struct Contact {
    pub key: FleetKey,
    /// The class's stock fit, for its model.
    pub fit: Loadout,
    pub ships: u32,
    pub company: String,
    pub cargo: Option<(Good, f64)>,
    /// System frame, metres, y up.
    pub pos: DVec3,
    pub vel: DVec3,
    /// Thrust the autopilot is asking for, m/s², and the most it has.
    pub thrust: DVec3,
    pub accel: f64,
    /// A harvester's run, if it's on one (issue #68).
    pub work: Option<Work>,
}

/// What a harvester is doing, as the HUD and the renderer show it.
#[derive(Clone, Debug)]
pub struct Work {
    pub good: Good,
    pub source: String,
    /// Holding station at the source, working it.
    pub at_source: bool,
    pub days_left: u32,
}

/// A fleet's flight as last predicted: the galaxy day the sim's state was
/// from, and the client's time it has been flown to.
#[derive(Clone, Copy, Debug)]
struct Track {
    day: u64,
    t_s: f64,
    ship: Kinematics,
    /// Reached its goal before the sim's next tick (a port, or the jump
    /// point it leaves by); not drawn until the sim moves it on.
    done: bool,
}

/// A pirate gang in view (DESIGN §7.2.2), flying as one.
#[derive(Clone, Debug)]
pub struct PirateContact {
    pub gang: GangId,
    pub name: String,
    /// Each ship's stock fit, in the gang's order.
    pub fits: Vec<Loadout>,
    /// A fighter's bounty, if a faction has posted one.
    pub bounty: Option<f64>,
    pub aim: Aim,
    /// System frame, metres, y up.
    pub pos: DVec3,
    pub vel: DVec3,
    pub thrust: DVec3,
    pub accel: f64,
}

#[derive(Default)]
pub struct Traffic {
    system: Option<SystemId>,
    tracks: HashMap<FleetKey, Track>,
    /// Gangs' flights as last predicted, and what they were doing then.
    pirate_tracks: HashMap<GangId, (Aim, Track)>,
    pub pirates: Vec<PirateContact>,
    /// The galaxy clock the contacts are for.
    pub at_s: f64,
    /// Stock fits by hull id, built once.
    fits: HashMap<String, Loadout>,
    pub contacts: Vec<Contact>,
}

impl Traffic {
    /// Bring every fleet in `system` up to galaxy time `clock_s`.
    pub fn update(&mut self, galaxy: &GalaxyState, system: SystemId, clock_s: f64) {
        if self.system != Some(system) {
            self.tracks.clear();
            self.system = Some(system);
        }
        let day = galaxy.time.days();
        let day_start = day as f64 * SECONDS_PER_DAY;
        let mut contacts = Vec::new();
        let mut live = Vec::new();
        for (key, f, sim_ship) in bubble::flights_in(galaxy, system) {
            let Some(company) = galaxy.companies.get(key.0) else {
                continue;
            };
            let Some(fleet) = company.fleets.iter().find(|fl| fl.id == key.1) else {
                continue;
            };
            let class = &galaxy.ship_classes[fleet.class];
            let accel = class.accel_laden;
            let track = self
                .tracks
                .entry(key)
                .and_modify(|t| {
                    if t.day != day {
                        *t = Track {
                            day,
                            t_s: day_start,
                            ship: sim_ship,
                            done: false,
                        };
                    }
                })
                .or_insert(Track {
                    day,
                    t_s: day_start,
                    ship: sim_ship,
                    done: false,
                });
            live.push(key);
            let goal = |t_s: f64| bubble::goal(galaxy, f, system, t_s / SECONDS_PER_DAY);
            if !track.done && clock_s > track.t_s {
                let t0 = track.t_s;
                let arrived =
                    flight::fly(&mut track.ship, accel, clock_s - t0, |t, _| goal(t0 + t));
                track.t_s = clock_s;
                track.done = arrived.is_some();
            }
            // A harvester at its source holds station there: still drawn.
            let working = f.working().copied();
            if track.done && working.is_none() {
                continue;
            }
            if !self.fits.contains_key(&class.hull) {
                let Some(fit) = retrox_content::ships::stock(&class.hull) else {
                    continue;
                };
                self.fits.insert(class.hull.clone(), fit);
            }
            let fit = self.fits[&class.hull].clone();
            let g = goal(track.t_s);
            let at_source = working.is_some()
                && (track.done || flight::length(flight::sub(g.pos, track.ship.pos)) < 2_000.0);
            // Holding station: wherever the source is now.
            let g = if at_source { goal(clock_s) } else { g };
            let (ship, thrust) = if at_source {
                (
                    Kinematics {
                        pos: g.pos,
                        vel: g.vel,
                    },
                    [0.0; 3],
                )
            } else {
                (track.ship, flight::autopilot(&track.ship, &g, accel, 1.0))
            };
            let work = working.map(|w| Work {
                good: retrox_sim::harvest::yields(galaxy, w.source).map_or(Good::Ice, |y| y.0),
                source: retrox_sim::harvest::name(galaxy, w.source).to_owned(),
                at_source,
                days_left: w.days_left,
            });
            contacts.push(Contact {
                key,
                fit,
                ships: fleet.ships,
                company: company.name.clone(),
                cargo: fleet.cargo.map(|c| (c.good, c.amount)),
                pos: to_client(ship.pos),
                vel: to_client(ship.vel),
                thrust: to_client(thrust),
                accel,
                work,
            });
        }
        self.tracks.retain(|k, _| live.contains(k));
        self.contacts = contacts;
        self.at_s = clock_s;
        self.update_pirates(galaxy, system, clock_s);
    }

    /// The gang `id` in view, if it is.
    pub fn pirate(&self, id: GangId) -> Option<&PirateContact> {
        self.pirates.iter().find(|p| p.gang == id)
    }

    /// Bring every gang flying in `system` up to `clock_s`: from the sim's
    /// state each day, like the traders; a stalk is the client's own and
    /// is drawn where it is.
    fn update_pirates(&mut self, galaxy: &GalaxyState, system: SystemId, clock_s: f64) {
        let day = galaxy.time.days();
        let day_start = day as f64 * SECONDS_PER_DAY;
        let mut out = Vec::new();
        let mut live = Vec::new();
        for (&id, prowl) in &galaxy.pirates.prowls {
            let Some(gang) = galaxy.pirates.gangs.get(id) else {
                continue;
            };
            if gang.lair != system || prowl.aim == Aim::Engaged {
                continue;
            }
            live.push(id);
            let accel = pirates::accel(galaxy, gang);
            let fresh = Track {
                day,
                t_s: day_start,
                ship: prowl.ship,
                done: false,
            };
            let entry = self.pirate_tracks.entry(id).or_insert((prowl.aim, fresh));
            if entry.0 != prowl.aim || entry.1.day != day {
                *entry = (prowl.aim, fresh);
            }
            let track = &mut entry.1;
            let prey = match prowl.aim {
                Aim::Hunt(key) => pirates::prey_ship(galaxy, key, system),
                _ => None,
            };
            let goal = |t_s: f64| {
                pirates::prowl_goal(
                    galaxy,
                    gang,
                    prowl,
                    prey,
                    t_s - day_start,
                    t_s / SECONDS_PER_DAY,
                )
            };
            if prowl.aim == Aim::Stalk {
                track.ship = prowl.ship;
                track.t_s = clock_s;
            } else if !track.done && clock_s > track.t_s {
                let t0 = track.t_s;
                if goal(t0).is_some() {
                    let arrived = flight::fly(&mut track.ship, accel, clock_s - t0, |t, _| {
                        goal(t0 + t).expect("checked")
                    });
                    // Home or caught up: hold there until the sim moves on.
                    track.done = arrived.is_some() && prowl.aim != Aim::Lurk;
                }
                track.t_s = clock_s;
            }
            if prowl.aim == Aim::Lurk
                && let Some(g) = goal(clock_s)
            {
                track.ship = Kinematics {
                    pos: g.pos,
                    vel: g.vel,
                };
            }
            let thrust = match goal(track.t_s) {
                Some(g) => flight::autopilot(&track.ship, &g, accel, 1.0),
                None => track.ship.vel,
            };
            let fits = gang
                .ships
                .iter()
                .filter_map(|h| retrox_content::ships::stock(h))
                .collect();
            out.push(PirateContact {
                gang: id,
                name: gang.name.clone(),
                fits,
                bounty: gang.bounty.map(|b| b.per_ship),
                aim: prowl.aim,
                pos: to_client(track.ship.pos),
                vel: to_client(track.ship.vel),
                thrust: to_client(thrust),
                accel,
            });
        }
        self.pirate_tracks.retain(|k, _| live.contains(k));
        self.pirates = out;
    }
}

/// Sim frame (system plane x/y) to client frame (system plane x/z, y up).
pub fn to_client(v: [f64; 3]) -> DVec3 {
    DVec3::new(v[0], v[2], v[1])
}

/// Client frame back to the sim's.
pub fn to_sim(v: DVec3) -> [f64; 3] {
    [v.x, v.z, v.y]
}

/// Where each ship of an `n`-ship fleet flies, relative to the fleet's
/// position, for ships `length_m` long: a loose echelon behind and to the
/// sides of the lead, in the fleet's own frame (nose toward -Z).
pub fn formation(n: u32, length_m: f64, seed: u64) -> Vec<DVec3> {
    let gap = length_m.max(10.0) * 3.0;
    (0..n)
        .map(|i| {
            if i == 0 {
                return DVec3::ZERO;
            }
            let rank = i.div_ceil(2) as f64;
            let side = if i % 2 == 1 { 1.0 } else { -1.0 };
            // A little unevenness, the same every time for a fleet.
            let h = seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(i) * 0x51);
            let jitter = ((h >> 20) % 1000) as f64 / 1000.0 - 0.5;
            DVec3::new(side * rank * gap, jitter * gap * 0.6, rank * gap * 1.2)
        })
        .collect()
}
