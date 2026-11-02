//! Flying the ship inside a system: the arcade flight model, collisions,
//! and fast travel under the sim's autopilot.

use glam::{DQuat, DVec3};
use retrox_sim::flight::{self, Goal, Kinematics};
use retrox_sim::ship::ShipStats;

use super::{
    Controls, FastTravel, Leg, Mode, REFERENCE, SECONDS_PER_DAY, World, rock_offset,
};

/// Top of the fine-control range, m/s (throttle 0.5).
pub const COMBAT_SPEED: f64 = 500.0;
/// Top speed at full throttle, m/s.
pub const CRUISE_SPEED: f64 = 150_000.0;
const BOOST: f64 = 2.0;
const ACCEL: f64 = 90.0;
pub const STRAFE_SPEED: f64 = 80.0;
/// Pitch, yaw, roll rates at full stick, rad/s.
const TURN_RATES: DVec3 = DVec3::new(1.3, 0.9, 2.2);
/// The ship's main drive under autopilot, m/s²: about a day per AU.
const DRIVE_ACCEL: f64 = ACCEL;

/// Forward speed for a throttle setting: linear up to [`COMBAT_SPEED`] at
/// 0.5, then exponential up to [`CRUISE_SPEED`] at 1.0.
pub fn throttle_speed(throttle: f64) -> f64 {
    if throttle <= 0.5 {
        throttle / 0.5 * COMBAT_SPEED
    } else {
        let t = (throttle - 0.5) / 0.5;
        COMBAT_SPEED * (CRUISE_SPEED / COMBAT_SPEED).powf(t)
    }
}
/// Rotation that points -Z along `dir`, keeping +Y roughly up.
pub fn look_rotation(dir: DVec3) -> DQuat {
    let f = dir.normalize_or(DVec3::NEG_Z);
    let r = f.cross(DVec3::Y).normalize_or(DVec3::X);
    let u = r.cross(f);
    DQuat::from_mat3(&glam::DMat3::from_cols(r, u, -f))
}

/// Turn a ship for one frame of stick input: its spin eases toward the
/// stick's rates (scaled by `turn`, see [`handling`]), and it rotates by
/// its spin. The player and every ship flown near them turn this way.
pub fn steer(rot: &mut DQuat, spin: &mut DVec3, stick: DVec3, turn: f64, dt: f64) {
    let target = stick * TURN_RATES * turn;
    let k = 1.0 - (-6.0 * dt).exp();
    *spin += (target - *spin) * k;
    let s = *spin;
    // Local axes: x = pitch, y = yaw (negated: + yaw turns right), z = roll.
    let delta = DQuat::from_scaled_axis(DVec3::new(s.x, -s.y, -s.z) * dt);
    *rot = (*rot * delta).normalize();
}

/// Flight assist for one frame: thrust the velocity toward `local` (the
/// velocity asked for, in ship axes), as hard as the ship can (`thrust`,
/// see [`handling`]). Returns how hard the engines burned, 0..1.
pub fn assist(vel: &mut DVec3, rot: DQuat, local: DVec3, boost: bool, thrust: f64, dt: f64) -> f64 {
    let boost = if boost { BOOST } else { 1.0 };
    let desired = rot * (local * DVec3::new(1.0, 1.0, boost));
    // Acceleration scales with the speed being asked for, so cruise
    // speeds are reached in a few seconds without making fine control
    // twitchy.
    let accel = (ACCEL * boost * thrust).max(desired.length().max(vel.length()) * 0.5);
    let step = (desired - *vel).clamp_length_max(accel * dt);
    *vel += step;
    if dt > 0.0 {
        (step.length() / (accel * dt)).min(1.0)
    } else {
        0.0
    }
}

/// How a ship handles next to the starting ship: thrust and turning
/// multipliers, within limits so every ship stays flyable.
pub fn handling(s: &ShipStats) -> (f64, f64) {
    let r = &*REFERENCE;
    let thrust = (s.acceleration() / r.acceleration()).clamp(0.3, 2.5);
    let turn = (s.angular_acceleration() / r.angular_acceleration())
        .sqrt()
        .clamp(0.3, 1.8);
    (thrust, turn)
}

impl World {
    /// Where the autopilot brings the ship to rest for `leg` at galaxy
    /// day `t`. Going to a place: on its arrival sphere, on the ship's side
    /// of it. Climbing: just past the well's edge, facing where the ship is
    /// headed next (or straight up from where it is).
    pub(super) fn fast_travel_goal(&self, leg: Leg, ship: DVec3, t: f64) -> Goal {
        let place = leg.frame();
        let centre = self.position_at(place, t);
        let up = (ship - centre).normalize_or(DVec3::Z);
        let (side, distance) = match leg {
            Leg::To(_) => (up, self.arrival_distance(place)),
            Leg::Climb { well, toward } => {
                let side = toward.map_or(up, |next| {
                    (self.position_at(next, t) - centre).normalize_or(up)
                });
                (side, self.well_radius(well) * 1.05)
            }
        };
        let pos = centre + side * distance;
        Goal {
            pos: pos.to_array(),
            vel: self.velocity_at(place, t).to_array(),
        }
    }

    /// The fast-travel drive's acceleration for this ship.
    pub(super) fn drive_accel(&self) -> f64 {
        DRIVE_ACCEL * self.handling().0
    }

    /// How the ship handles: see [`handling`]. Sections wrecked in a fight
    /// take their modules with them.
    pub fn handling(&self) -> (f64, f64) {
        if self.combat.player.fit == self.player.ship.fit {
            handling(&self.combat.player.stats)
        } else {
            // Refitted since the last frame of flight.
            handling(&self.stats())
        }
    }

    pub(super) fn start_fast_travel(&mut self, leg: Leg) {
        let t = self.t_days();
        let pos = self.ship_position();
        let ship = Kinematics {
            pos: pos.to_array(),
            vel: (self.ship.vel + self.velocity_at(self.ship.frame, t)).to_array(),
        };
        // Pick how fast time runs so the flight plays out in a few seconds.
        // Time it by flying it once ahead: a ship already moving fast (say,
        // just dropped out of another trip) must brake and come back, which
        // can take far longer than the distance alone suggests.
        let goal = self.fast_travel_goal(leg, pos, t);
        let distance = (DVec3::from_array(goal.pos) - pos).length();
        let accel = self.drive_accel();
        let mut rehearsal = ship;
        let estimate = flight::fly(&mut rehearsal, accel, 1e10, |s, k| {
            self.fast_travel_goal(leg, DVec3::from_array(k.pos), t + s / SECONDS_PER_DAY)
        })
        .unwrap_or_else(|| flight::trip_seconds(distance, accel));
        let real = (2.0 + (distance / 1e7).max(1.0).log10() * 1.2).min(7.0);
        self.ship.spin = DVec3::ZERO;
        self.ship.throttle = 0.0;
        self.mode = Mode::FastTravel(FastTravel {
            leg,
            ship,
            depart_s: self.clock_s,
            warp: estimate / real,
            elapsed: 0.0,
        });
    }

    /// One frame of fast travel: fly `dt` real seconds of sped-up flight.
    pub(super) fn fast_travel(&mut self, mut trip: FastTravel, dt: f64) {
        trip.elapsed += dt as f32;
        let t0 = self.t_days();
        let leg = trip.leg;
        let accel = self.drive_accel();
        let arrived = flight::fly(&mut trip.ship, accel, dt * trip.warp, |t, ship| {
            let pos = DVec3::from_array(ship.pos);
            self.fast_travel_goal(leg, pos, t0 + t / SECONDS_PER_DAY)
        });
        // Galaxy time passes by exactly as long as the ship flew.
        self.advance_clock(arrived.unwrap_or(dt * trip.warp));
        let pos = DVec3::from_array(trip.ship.pos);
        let vel = DVec3::from_array(trip.ship.vel);
        // Face the way the drive is pushing: out along the route, then
        // flipped round to brake.
        let goal = self.fast_travel_goal(leg, pos, self.t_days());
        let thrust = DVec3::from_array(flight::autopilot(&trip.ship, &goal, accel, 1.0));
        if thrust.length() > accel * 0.1 {
            let k = 1.0 - (-4.0 * dt).exp();
            self.ship.rot = self.ship.rot.slerp(look_rotation(thrust), k).normalize();
        }
        self.ship.frame = Place::Star;
        self.ship.rel = pos;
        self.ship.vel = vel;
        match arrived {
            Some(_) => self.arrive_fast_travel(leg),
            None => self.mode = Mode::FastTravel(trip),
        }
    }

    /// Switch the ship into `frame`, keeping where it is and how it moves.
    pub(super) fn enter_frame(&mut self, frame: Place) {
        let t = self.t_days();
        let pos = self.ship_position();
        let vel = self.ship.vel + self.velocity_at(self.ship.frame, t);
        self.ship.frame = frame;
        self.ship.rel = pos - self.position(frame);
        self.ship.vel = vel - self.velocity_at(frame, t);
    }

    /// Stop the autopilot mid-trip: the ship carries on from wherever it
    /// is, as fast as it's going.
    pub(super) fn drop_out(&mut self, _trip: FastTravel) {
        self.mode = Mode::Normal;
        let speed = self.ship.vel.length() / 1000.0;
        self.say(format!("Dropped out of fast travel at {speed:.0} km/s"));
    }

    pub(super) fn arrive_fast_travel(&mut self, leg: Leg) {
        // At rest relative to the target, within a few m/s: carry on from
        // exactly there.
        self.enter_frame(leg.frame());
        self.mode = Mode::Normal;
        let name = self.name(leg.frame());
        self.say(match leg {
            Leg::To(_) => format!("Arrived at {name}"),
            Leg::Climb { .. } => format!("Clear of the gravity well of {name}"),
        });
    }

    pub(super) fn steer(&mut self, dt: f64, c: Controls) {
        let turn = self.handling().1;
        steer(&mut self.ship.rot, &mut self.ship.spin, c.stick, turn, dt);
        self.ship.throttle = (self.ship.throttle + c.throttle_delta * dt * 0.4).clamp(-0.3, 1.0);
    }

    pub(super) fn fly(&mut self, dt: f64, c: Controls) {
        let forward = if self.ship.throttle >= 0.0 {
            throttle_speed(self.ship.throttle)
        } else {
            self.ship.throttle / 0.5 * COMBAT_SPEED
        };
        // Boost only helps in the fine-control range.
        let boost = c.boost && self.ship.throttle <= 0.5;
        let local = DVec3::new(c.strafe * STRAFE_SPEED, c.lift * STRAFE_SPEED, -forward);
        let thrust = self.handling().0;
        self.burn = if self.ship.flight_assist {
            assist(&mut self.ship.vel, self.ship.rot, local, boost, thrust, dt)
        } else {
            let boost = if boost { BOOST } else { 1.0 };
            let desired = (local * DVec3::new(1.0, 1.0, boost)).length();
            let accel = (ACCEL * boost * thrust).max(desired.max(self.ship.vel.length()) * 0.5);
            let push = DVec3::new(c.strafe, c.lift, -c.throttle_delta.max(0.0));
            self.ship.vel += self.ship.rot * push * accel * dt;
            push.length().min(1.0)
        };
        self.ship.rel += self.ship.vel * dt;
        self.collide();
    }

    /// Push the ship out of the star, bodies, stations and rocks.
    pub(super) fn collide(&mut self) {
        let pos = self.ship_position();
        let mut solids: Vec<(DVec3, f64)> = vec![(DVec3::ZERO, self.star_radius_m() * 1.02)];
        for &b in &self.view.bodies {
            solids.push((
                self.position(Place::Body(b)),
                self.place_radius(Place::Body(b)) + 30.0,
            ));
        }
        for &s in &self.view.stations {
            solids.push((
                self.position(Place::Station(s)),
                self.station_size_m(s) * 0.35,
            ));
        }
        if let Some((field, rocks)) = &self.field_rocks {
            let center = self.position(Place::Field(*field));
            for (i, rock) in rocks.iter().enumerate() {
                if !self.mining.is_broken(*field, i) {
                    solids.push((center + rock_offset(rock), rock.radius_m + 8.0));
                }
            }
            for (pos, radius) in self.mining.frag_solids(*field) {
                solids.push((center + pos, radius + 4.0));
            }
        }
        for (center, radius) in solids {
            let d = pos - center;
            let len = d.length();
            if len < radius && len > 0.0 {
                let n = d / len;
                self.ship.rel += n * (radius - len);
                let into = self.ship.vel.dot(n);
                if into < 0.0 {
                    self.bump = self.bump.max(-into);
                    self.ship.vel -= n * into * 1.3;
                }
            }
        }
    }
}
