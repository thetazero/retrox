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

#[derive(Default)]
pub struct Traffic {
    system: Option<SystemId>,
    tracks: HashMap<FleetKey, Track>,
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
            if track.done {
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
            let thrust = flight::autopilot(&track.ship, &g, accel, 1.0);
            contacts.push(Contact {
                key,
                fit,
                ships: fleet.ships,
                company: company.name.clone(),
                cargo: fleet.cargo.map(|c| (c.good, c.amount)),
                pos: to_client(track.ship.pos),
                vel: to_client(track.ship.vel),
                thrust: to_client(thrust),
                accel,
            });
        }
        self.tracks.retain(|k, _| live.contains(k));
        self.contacts = contacts;
    }
}

/// Sim frame (system plane x/y) to client frame (system plane x/z, y up).
fn to_client(v: [f64; 3]) -> DVec3 {
    DVec3::new(v[0], v[2], v[1])
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
