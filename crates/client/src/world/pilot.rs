//! Flying the ship inside a system: the arcade flight model, collisions,
//! and fast travel under the sim's autopilot.

use glam::{DQuat, DVec2, DVec3};
use retrox_sim::flight::{self, Goal, Kinematics};
use retrox_sim::ship::ShipStats;

use super::{
    Controls, FastTravel, Leg, Mode, Place, REFERENCE, SECONDS_PER_DAY, SettlementId, World,
};

/// Top of the fine-control range, m/s (throttle [`DETENT`]).
pub const COMBAT_SPEED: f64 = 500.0;
/// The throttle's detent between the fine-control range and cruise: a held
/// [W] stops here, and a fresh press goes on past it (#166).
pub const DETENT: f64 = 0.5;
/// Top speed at full throttle, m/s.
pub const CRUISE_SPEED: f64 = 150_000.0;
const BOOST: f64 = 2.0;
/// A ship's acceleration on its own thrust in fine control, m/s², times
/// its thrust multiplier (see [`handling`]).
pub const ACCEL: f64 = 90.0;
pub const STRAFE_SPEED: f64 = 80.0;
/// Pitch, yaw, roll rates at full stick, rad/s.
const TURN_RATES: DVec3 = DVec3::new(1.3, 0.9, 2.2);
/// The ship's main drive under autopilot, m/s²: about a day per AU.
const DRIVE_ACCEL: f64 = ACCEL;
/// How hard the attitude autopilot turns per radian off target, 1/s.
const ATTITUDE_GAIN: f64 = 2.5;
/// Up to this speed, twice the detent (boosted fine control), flight
/// assist pushes with the hull's own thrust and no more, so local speeds
/// build and bleed off at a rate a pilot can judge (#210). Past it, for
/// cruise, it pushes harder the faster it's going: [`CRUISE_PUSH`] more
/// m/s² per m/s, so cruise speeds come and go in seconds.
const CRUISE_KNEE: f64 = COMBAT_SPEED * BOOST;
/// See [`CRUISE_KNEE`], 1/s.
const CRUISE_PUSH: f64 = 0.5;
/// After letting go of a push of [W] that reached the detent, a fresh
/// press goes on into cruise until the ship has slowed below this, m/s:
/// a couple of seconds, time to read the hint that says so and act on it.
const CRUISE_READY_SPEED: f64 = COMBAT_SPEED * 0.6;

/// How hard flight assist pushes, m/s², for a ship whose own thrust gives
/// `base` m/s², asked to go (or already going) `speed` m/s: see
/// [`CRUISE_KNEE`].
fn assist_accel(base: f64, speed: f64) -> f64 {
    base + CRUISE_PUSH * (speed - CRUISE_KNEE).max(0.0)
}

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
    // A turn eases in, and settles faster than it builds, so letting go
    // of the stick stops the nose close to where it was let go (#210).
    let ease = |want: f64, now: f64| {
        let rate = if want.abs() < now.abs() || want * now < 0.0 {
            10.0
        } else {
            6.0
        };
        now + (want - now) * (1.0 - (-rate * dt).exp())
    };
    *spin = DVec3::new(
        ease(target.x, spin.x),
        ease(target.y, spin.y),
        ease(target.z, spin.z),
    );
    let s = *spin;
    // Local axes: x = pitch, y = yaw (negated: + yaw turns right), z = roll.
    let delta = DQuat::from_scaled_axis(DVec3::new(s.x, -s.y, -s.z) * dt);
    *rot = (*rot * delta).normalize();
}

/// The stick the attitude autopilot holds this frame to bring the nose of
/// a ship at `rot` round to `face`, as a pilot would: rates in proportion
/// to how far off it is, never more than full stick. Fed through
/// [`steer`], the ship turns at its own hull's rates.
pub fn attitude_stick(rot: DQuat, face: DVec3, turn: f64) -> DVec3 {
    let mut err = rot.inverse() * look_rotation(face);
    if err.w < 0.0 {
        err = -err;
    }
    // In ship axes; `steer` turns by (pitch, -yaw, -roll).
    let e = err.to_scaled_axis();
    let rates = DVec3::new(e.x, -e.y, -e.z) * ATTITUDE_GAIN;
    (rates / (TURN_RATES * turn)).clamp(DVec3::splat(-1.0), DVec3::splat(1.0))
}

/// How far the pilot's aim may lead the nose, radians (about 26°): far
/// enough that the ship turns at its full rate toward it, near enough
/// that the aim stays in view (DESIGN §7.1.5).
pub const AIM_LEAD: f64 = 0.45;
/// How hard the ship turns toward the aim per radian off, 1/s: the nose
/// closes on it smoothly and settles there without overshooting.
const AIM_GAIN: f64 = 3.0;

/// The aim moved by `by` (radians, right and up as a ship at `rot` sees
/// them), and kept within [`AIM_LEAD`] of its nose.
pub fn move_aim(aim: DVec3, rot: DQuat, by: DVec2) -> DVec3 {
    let up = rot * DVec3::Y;
    let right = rot * DVec3::X;
    let aim = DQuat::from_axis_angle(up, -by.x) * DQuat::from_axis_angle(right, by.y) * aim;
    let nose = rot * DVec3::NEG_Z;
    let off = nose.angle_between(aim);
    if off <= AIM_LEAD {
        return aim.normalize();
    }
    let axis = nose.cross(aim);
    // Straight behind the nose, any way round will do.
    let axis = axis.try_normalize().unwrap_or(up);
    DQuat::from_axis_angle(axis, AIM_LEAD) * nose
}

/// The pitch and yaw stick that turns a ship at `rot` toward `aim` (a
/// direction): rates in proportion to how far off the nose is, along the
/// straight way there, never more than full stick. Fed through [`steer`]
/// it eases in and settles on the aim. Roll is left to the pilot.
pub fn aim_stick(rot: DQuat, aim: DVec3, turn: f64) -> DVec2 {
    // Ship axes: x right, y up, -z forward.
    let local = rot.inverse() * aim.normalize_or(rot * DVec3::NEG_Z);
    let side = DVec2::new(local.y, local.x);
    let off = (-local.z).clamp(-1.0, 1.0).acos();
    let Some(dir) = side.try_normalize() else {
        // On the nose, or dead behind it (go over the top).
        return if local.z > 0.0 { DVec2::new(1.0, 0.0) } else { DVec2::ZERO };
    };
    let rates = dir * off * AIM_GAIN;
    let stick = rates / (DVec2::new(TURN_RATES.x, TURN_RATES.y) * turn);
    // Scaled as one, so a diagonal aim is reached along a straight line.
    stick / stick.abs().max_element().max(1.0)
}

/// Flight assist for one frame: thrust the velocity toward `local` (the
/// velocity asked for, in ship axes), as hard as the ship can (`thrust`,
/// see [`handling`]). Returns how hard the engines burned, 0..1.
pub fn assist(vel: &mut DVec3, rot: DQuat, local: DVec3, boost: bool, thrust: f64, dt: f64) -> f64 {
    let boost = if boost { BOOST } else { 1.0 };
    let desired = rot * (local * DVec3::new(1.0, 1.0, boost));
    let accel = assist_accel(ACCEL * boost * thrust, desired.length().max(vel.length()));
    let step = (desired - *vel).clamp_length_max(accel * dt);
    *vel += step;
    if dt > 0.0 {
        (step.length() / (accel * dt)).min(1.0)
    } else {
        0.0
    }
}

/// How far a ship flying at `speed` with flight assist on goes before it
/// stops once the throttle is cut, m, given its `thrust` (see [`handling`]):
/// what [`assist`] does with nothing asked for. Up to [`CRUISE_KNEE`] it
/// brakes at its own thrust, `v² / 2a`; past it the brakes grow with speed
/// (see [`assist_accel`]).
pub fn stopping_distance(speed: f64, thrust: f64) -> f64 {
    let a = ACCEL * thrust;
    let k = CRUISE_KNEE;
    let c = CRUISE_PUSH;
    if speed <= k {
        speed * speed / (2.0 * a)
    } else {
        // ∫ v dv / (a + c (v - k)) from k up to `speed`.
        let w = speed - k;
        k * k / (2.0 * a) + w / c + (k - a / c) / c * ((a + c * w) / a).ln()
    }
}

/// Throttle per second that [W] or [S] moves it by in fine control, for a
/// ship whose own thrust gives `accel` m/s²: the speed asked for builds as
/// fast as the ship can follow it.
fn fine_throttle_rate(accel: f64) -> f64 {
    accel * DETENT / COMBAT_SPEED
}

/// The fast-travel drive a ship of `stats` has, m/s²: what the player's
/// own autopilot and a pirate's stalk both fly on.
pub fn drive_accel_of(stats: &ShipStats) -> f64 {
    DRIVE_ACCEL * handling(stats).0
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
        match leg {
            Leg::Intercept(fleet) => return self.intercept_goal(fleet, ship, t),
            Leg::Gang(gang) => return self.gang_goal(gang, ship, t),
            _ => {}
        }
        let place = leg.frame();
        let centre = self.position_at(place, t);
        if let Some(rock) = self.stop_rock(leg, t) {
            let side = (ship - rock).normalize_or(DVec3::Z);
            let standoff = self.field_stop.map_or(0.0, |s| s.2);
            return Goal {
                pos: (rock + side * standoff).to_array(),
                vel: self.velocity_at(place, t).to_array(),
            };
        }
        let centre_vel = self.velocity_at(place, t);
        let up = (ship - centre).normalize_or(DVec3::Z);
        // The side of the place the goal is on, and how fast that side
        // turns, in 1/s.
        let (side, turn, distance) = match leg {
            Leg::To(_) | Leg::Intercept(_) | Leg::Gang(_) => {
                (up, DVec3::ZERO, self.arrival_distance(place))
            }
            Leg::Climb { well, toward } => {
                let distance = self.well_radius(well) * 1.05;
                let to_next = toward.map(|next| {
                    let r = self.position_at(next, t) - centre;
                    (r, self.velocity_at(next, t) - centre_vel)
                });
                match to_next {
                    Some((r, v)) if r.length() > 0.0 => {
                        // The side facing the next place swings round as the
                        // well orbits: the goal moves with it, and must say
                        // so, or the ship chases it and never matches speed
                        // closely enough to arrive (issue #109).
                        let side = r / r.length();
                        (side, (v - side * side.dot(v)) / r.length(), distance)
                    }
                    _ => (up, DVec3::ZERO, distance),
                }
            }
        };
        Goal {
            pos: (centre + side * distance).to_array(),
            vel: (centre_vel + turn * distance).to_array(),
        }
    }

    /// Which way the ship should be looking when `leg` ends: at the place
    /// it travelled to, or, out of a well, the way it's headed next.
    pub(super) fn arrival_facing(&self, leg: Leg, ship: DVec3, t: f64) -> DVec3 {
        if let Some(rock) = self.stop_rock(leg, t) {
            return rock - ship;
        }
        match leg {
            Leg::To(place) => self.position_at(place, t) - ship,
            Leg::Intercept(fleet) => self.position_at(Place::Convoy(fleet), t) - ship,
            Leg::Gang(gang) => self
                .gang_at_day(gang, t)
                .map_or(DVec3::NEG_Z, |(p, _)| p - ship),
            Leg::Climb { .. } => {
                let goal = self.fast_travel_goal(leg, ship, t);
                DVec3::from_array(goal.pos) - self.position_at(leg.frame(), t)
            }
        }
    }

    /// Where the rock a trip into a field stops by is at day `t`, if this
    /// leg goes to one.
    fn stop_rock(&self, leg: Leg, t: f64) -> Option<DVec3> {
        let Leg::To(Place::Field(field)) = leg else {
            return None;
        };
        let (f, offset, _) = self.field_stop?;
        (f == field).then(|| self.position_at(leg.frame(), t) + offset)
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
        self.field_stop = match leg {
            Leg::To(Place::Field(field)) => {
                let side = (pos - self.position(Place::Field(field))).normalize_or(DVec3::Z);
                self.field_stop(field, side)
                    .map(|(offset, standoff)| (field, offset, standoff))
            }
            _ => None,
        };
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
        self.turn_over = None;
        self.mode = Mode::FastTravel(FastTravel {
            leg,
            ship,
            depart_s: self.clock_s,
            warp: estimate / real,
            elapsed: 0.0,
            burn: 0.0,
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
        // flipped round to brake. Turning over to face the place comes
        // after, at the ship's own rates ([`World::arrive_fast_travel`]).
        let goal = self.fast_travel_goal(leg, pos, self.t_days());
        let thrust = DVec3::from_array(flight::autopilot(&trip.ship, &goal, accel, 1.0));
        if thrust.length() > accel * 0.1 {
            let k = 1.0 - (-4.0 * dt).exp();
            self.ship.rot = self.ship.rot.slerp(look_rotation(thrust), k).normalize();
        }
        trip.burn = drive_burn(thrust, self.ship.rot, accel);
        let anchor = self.combat_anchor();
        self.ship.frame = Place::Star;
        self.ship.rel = pos;
        self.ship.vel = vel;
        self.rebase_fight(anchor);
        match arrived {
            Some(_) => self.arrive_fast_travel(leg),
            None => self.mode = Mode::FastTravel(trip),
        }
    }

    /// Switch the ship into `frame`, keeping where it is and how it moves.
    /// A fight anchored to the ship's frame moves over with it.
    pub(super) fn enter_frame(&mut self, frame: Place) {
        let t = self.t_days();
        let pos = self.ship_position();
        let vel = self.ship.vel + self.velocity_at(self.ship.frame, t);
        let anchor = self.combat_anchor();
        self.ship.frame = frame;
        self.ship.rel = pos - self.position(frame);
        self.ship.vel = vel - self.velocity_at(frame, t);
        self.rebase_fight(anchor);
    }

    /// The fight was anchored at `was`: if its anchor has changed, carry
    /// everything in it over to the new one, so nothing in the scene jumps.
    /// `was` must still have a position (a lost convoy's last sighting
    /// counts).
    pub(super) fn rebase_fight(&mut self, was: Place) {
        let now = self.combat_anchor();
        if now == was {
            return;
        }
        let t = self.t_days();
        self.combat.rebase(
            self.position(was) - self.position(now),
            self.velocity_at(was, t) - self.velocity_at(now, t),
        );
    }

    /// Stop the autopilot mid-trip: the ship carries on from wherever it
    /// is, as fast as it's going.
    pub(super) fn drop_out(&mut self, _trip: FastTravel) {
        self.mode = Mode::Normal;
        let speed = self.ship.vel.length() / 1000.0;
        self.say(format!("Dropped out of fast travel at {speed:.0} km/s"));
    }

    pub(super) fn arrive_fast_travel(&mut self, leg: Leg) {
        // The ship comes to rest tail-first, braking; its attitude
        // autopilot then turns it round to face the place (DESIGN §7.1.2).
        self.turn_over = Some(leg);
        // At rest relative to the target, within a few m/s: carry on from
        // exactly there.
        self.enter_frame(leg.frame());
        self.mode = Mode::Normal;
        if let Leg::Gang(gang) = leg {
            self.catch_gang(gang);
            return;
        }
        let name = self.name(leg.frame());
        self.say(match leg {
            Leg::To(_) => format!("Arrived at {name}"),
            Leg::Climb { .. } => format!("Clear of the gravity well of {name}"),
            Leg::Intercept(fleet) => {
                let company = self
                    .galaxy
                    .companies
                    .get(fleet.0)
                    .map_or("the convoy", |c| c.name.as_str());
                format!("Alongside {company}")
            }
            // Caught up: see `World::catch_gang`.
            Leg::Gang(_) => return,
        });
    }

    pub(super) fn steer(&mut self, dt: f64, mut c: Controls) {
        let turn = self.handling().1;
        let aiming = c.aim != DVec2::ZERO;
        if self.ship.rot.dot(self.aim_rot).abs() < 1.0 - 1e-12 {
            self.aim = None;
        }
        if let Some(leg) = self.turn_over {
            let face = self.arrival_facing(leg, self.ship_position(), self.t_days());
            let off = (self.ship.rot * DVec3::NEG_Z).angle_between(face);
            if c.stick != DVec3::ZERO
                || aiming
                || (off < 0.2_f64.to_radians() && self.ship.spin.length() < 0.02)
            {
                // The pilot has the stick, or we're there.
                self.turn_over = None;
            } else {
                c.stick = attitude_stick(self.ship.rot, face, turn);
                self.aim = None;
            }
        }
        if self.turn_over.is_none() {
            if c.stick.x != 0.0 || c.stick.y != 0.0 {
                // Turned by rates (scripted flight): no aim to hold.
                self.aim = None;
            } else {
                // The pilot's aim, held where it was left (DESIGN §7.1.5).
                let rot = self.ship.rot;
                let aim = self.aim.unwrap_or(rot * DVec3::NEG_Z);
                let aim = move_aim(aim, rot, c.aim);
                self.aim = Some(aim);
                let s = aim_stick(rot, aim, turn);
                c.stick.x = s.x;
                c.stick.y = s.y;
            }
        }
        steer(&mut self.ship.rot, &mut self.ship.spin, c.stick, turn, dt);
        self.aim_rot = self.ship.rot;
        self.ship.throttle = self.next_throttle(c.throttle_delta, dt);
    }

    /// The throttle after a frame of [W]/[S] (DESIGN §7.1).
    ///
    /// In fine control (up to the [`DETENT`]) the throttle is held, not
    /// set: [W] asks for more speed from however fast the ship is going,
    /// as fast as its own thrust can build it, and letting go brings the
    /// ship to rest; [S] likewise backs it astern. A held [W] stops at the
    /// detent (#166). Let go there, a fresh press goes on into cruise for
    /// as long as the ship is still faster than [`CRUISE_READY_SPEED`]
    /// (the flight panel says so for just that long), where the throttle stays where it's
    /// set: [W] opens it a quarter as fast (full cruise is [W] held five
    /// seconds), [S] closes it, and once it's back down through the detent
    /// it's held again.
    fn next_throttle(&mut self, delta: f64, dt: f64) -> f64 {
        let before = self.ship.throttle;
        let was_held = self.throttle_held;
        self.throttle_held = delta.signum() * f64::from(u8::from(delta != 0.0));
        if self.ship.vel.length() < CRUISE_READY_SPEED || delta < 0.0 {
            self.cruise_ready = false;
        }
        if before > DETENT {
            self.at_detent = false;
            let rate = if delta > 0.0 { 0.1 } else { 0.4 };
            return (before + delta * dt * rate).clamp(-0.3, 1.0);
        }
        if delta > 0.0 && self.cruise_ready && was_held <= 0.0 {
            self.cruise_ready = false;
            return (DETENT + delta * dt * 0.1).min(1.0);
        }
        if delta == 0.0 {
            if was_held > 0.0 && self.at_detent {
                self.cruise_ready = true;
            }
            self.at_detent = false;
            // Let go: the ship comes to rest. Nothing held before (the
            // autopilot or a start left the throttle set): it stays.
            return if was_held != 0.0 { 0.0 } else { before };
        }
        // From however fast the ship is going along its nose, so [W] never
        // brakes a ship that's coasting faster than the throttle says.
        let rate = fine_throttle_rate(ACCEL * self.handling().0);
        let along = self.ship.vel.dot(self.ship.rot * DVec3::NEG_Z);
        let here = (along / COMBAT_SPEED * DETENT).clamp(-0.3, DETENT);
        if delta > 0.0 {
            let next = before.max(here) + delta * dt * rate;
            if next >= DETENT {
                self.at_detent = true;
                return DETENT;
            }
            next
        } else {
            self.at_detent = false;
            (before.min(here) + delta * dt * rate).max(-0.3)
        }
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
        let mut thrust = self.handling().0;
        // Let go in time, flight assist settles the ship beside what it's
        // flying at rather than wherever full brakes would leave it.
        if local == DVec3::ZERO
            && let Some(a) = self.approach()
            && let Some(brake) = a.settle_brake
        {
            thrust = thrust.min(brake / ACCEL);
        }
        self.burn = if self.ship.flight_assist {
            assist(&mut self.ship.vel, self.ship.rot, local, boost, thrust, dt)
        } else {
            let boost = if boost { BOOST } else { 1.0 };
            let desired = (local * DVec3::new(1.0, 1.0, boost)).length();
            let accel = assist_accel(ACCEL * boost * thrust, desired.max(self.ship.vel.length()));
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
            // Rocks stop the ship at the surface you see, not at a sphere
            // of their nominal radius (#155): the sphere kept it a hundred
            // metres and more off a squashed quarry's face, out of the
            // tractor's reach of the ore it shed, and let bulges poke
            // through the ship.
            let center = self.position(Place::Field(*field));
            let near = pos - center;
            for rock in self.mining.solids(*field, rocks, self.clock_s) {
                let margin = if rock.base { 8.0 } else { 4.0 };
                if (near - rock.pos).length() > rock.bound() + margin {
                    continue;
                }
                let radius = rock.surface_toward(near) + margin;
                solids.push((center + rock.pos, radius));
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

/// How hard a ship's main engines burn, 0..1, pushing `thrust` m/s² with
/// its nose at `rot` and `accel` m/s² to give: only the share along the
/// nose counts, so the engines go dark while it turns over to brake.
pub fn drive_burn(thrust: DVec3, rot: DQuat, accel: f64) -> f32 {
    let along = thrust.dot(rot * DVec3::NEG_Z);
    (along / accel.max(1e-9)).clamp(0.0, 1.0) as f32
}

/// How far off a rock may be to be what the pilot is flying at, m.
const APPROACH_ROCK_RANGE: f64 = 6_000.0;
/// How far off a station may be, m.
const APPROACH_STATION_RANGE: f64 = 40_000.0;
/// How far off the line the ship is going along (or its nose) something
/// may be, past its own edge, and still be what it's flying at, rad.
const APPROACH_CONE: f64 = 0.07;

/// What the pilot is flying at, and how the approach to it is going
/// (DESIGN §7.1): the rock or station nearest the line the ship is moving
/// along, or, at rest, the one its nose is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Approach {
    /// A station, if it's one; else a rock.
    pub station: Option<SettlementId>,
    /// Its centre, in the system frame.
    pub pos: DVec3,
    /// From its centre out to the surface facing the ship, m.
    pub radius: f64,
    /// From the ship to that surface, m.
    pub gap: f64,
    /// How fast the ship closes on it, m/s; negative when drawing away.
    pub closing: f64,
    /// Closing on it: how far short of its surface the ship comes to rest
    /// if the throttle is let go now, m, negative when that's past it.
    pub stops_short: Option<f64>,
    /// How far off its surface "beside it" is, m: where flight assist
    /// settles a ship let go in time (see [`settle_window`]). A ship's
    /// length off a rock; in docking reach of a station.
    pub standoff: f64,
    /// Let go now, full brakes would stop the ship past the standoff but
    /// within the settle window: the gentler braking, m/s², that stops it
    /// at the standoff instead.
    pub settle_brake: Option<f64>,
}

/// How much further than the standoff full brakes may stop the ship, m,
/// for flight assist to settle it at the standoff when the throttle is let
/// go, closing at `closing` m/s: about 0.8 s of reaction, as the stop
/// point closes on what it's flying at at twice the ship's speed while
/// [W] is held (#210). Let go sooner and it stops short; later, past.
pub fn settle_window(closing: f64) -> f64 {
    (closing * 1.6).max(60.0)
}

impl World {
    /// What the pilot is flying at, if anything: see [`Approach`].
    /// Whether the pilot's aim is on a rock or station in reach (or just
    /// off its edge): there the arrow keys slow to their fine rate, so a
    /// held key doesn't carry the aim past a small rock before the pilot
    /// can let go (DESIGN §7.1.5).
    pub fn aim_on_something(&self) -> bool {
        let Some(aim) = self.aim else {
            return false;
        };
        if self.mode != Mode::Normal {
            return false;
        }
        let me = self.ship_position();
        let on = |pos: DVec3, radius: f64| {
            let to = pos - me;
            let d = to.length();
            d > radius && aim.angle_between(to) < (radius / d).asin() + 0.25_f64.to_radians()
        };
        if let Some((field, rocks)) = &self.field_rocks {
            let centre = self.position(Place::Field(*field));
            let f = &self.galaxy.fields[*field];
            for r in self.mining.live_rocks(*field, f, rocks, self.clock_s) {
                let pos = centre + r.pos;
                if (pos - me).length() - r.radius <= APPROACH_ROCK_RANGE && on(pos, r.radius) {
                    return true;
                }
            }
        }
        self.view.stations.iter().any(|&s| {
            let pos = self.position(Place::Station(s));
            let radius = self.station_size_m(s) * 0.35;
            (pos - me).length() - radius <= APPROACH_STATION_RANGE && on(pos, radius)
        })
    }

    pub fn approach(&self) -> Option<Approach> {
        if self.mode != Mode::Normal || self.combat.lost.is_some() {
            return None;
        }
        let me = self.ship_position();
        let t = self.t_days();
        let my_vel = self.ship.vel + self.velocity_at(self.ship.frame, t);
        let nose = self.ship.rot * DVec3::NEG_Z;
        let thrust = self.handling().0;
        let length = self.stats().length_m.max(12.0);
        // (pos, vel, nominal radius, surface toward the ship, standoff,
        // station)
        #[allow(clippy::type_complexity)]
        let mut near: Vec<(DVec3, DVec3, f64, f64, f64, Option<SettlementId>)> = Vec::new();
        if let Some((field, rocks)) = &self.field_rocks {
            let place = Place::Field(*field);
            let centre = self.position(place);
            let field_vel = self.velocity_at(place, t);
            let f = &self.galaxy.fields[*field];
            for r in self.mining.live_rocks(*field, f, rocks, self.clock_s) {
                let pos = centre + r.pos;
                if (pos - me).length() - r.radius > APPROACH_ROCK_RANGE {
                    continue;
                }
                let surface = crate::mining::surface_along(r.seed, r.rot, r.radius, me - pos);
                near.push((pos, field_vel + r.vel, r.radius, surface, length, None));
            }
        }
        for &s in &self.view.stations {
            let place = Place::Station(s);
            let pos = self.position(place);
            let size = self.station_size_m(s);
            let radius = size * 0.35;
            if (pos - me).length() - radius > APPROACH_STATION_RANGE {
                continue;
            }
            // Where fast travel stops, in reach of [F] (DESIGN §7.1.2).
            let standoff = size * 1.2 + 200.0 - radius;
            near.push((
                pos,
                self.velocity_at(place, t),
                radius,
                radius,
                standoff,
                Some(s),
            ));
        }
        let mut best: Option<(f64, f64, Approach)> = None;
        for (pos, vel, radius, surface, standoff, station) in near {
            let rel = my_vel - vel;
            let to = pos - me;
            let d = to.length();
            if d < 1e-6 {
                continue;
            }
            let speed = rel.length();
            let dir = if speed > 3.0 { rel / speed } else { nose };
            let along = to.dot(dir);
            if along <= 0.0 {
                continue;
            }
            let off = (to - dir * along).length();
            let miss = ((off - radius).max(0.0)).atan2(along);
            if miss > APPROACH_CONE {
                continue;
            }
            let closing = rel.dot(to / d);
            let gap = d - surface;
            // Full brakes along the line it's moving on: only the share
            // toward the thing counts.
            let full =
                (closing > 0.5).then(|| gap - stopping_distance(speed, thrust) * closing / speed);
            let settles =
                full.is_some_and(|f| f > standoff && f <= standoff + settle_window(closing));
            let a = Approach {
                station,
                pos,
                radius: surface,
                gap,
                closing,
                stops_short: if settles { Some(standoff) } else { full },
                standoff,
                // Stopping `gap - standoff` further on, along the line.
                settle_brake: settles.then(|| speed * closing / (2.0 * (gap - standoff).max(1.0))),
            };
            // Dead on the line first, nearest first; then the least off it.
            if best.is_none_or(|(m, g, _)| (miss, gap) < (m, g)) {
                best = Some((miss, gap, a));
            }
        }
        best.map(|(_, _, a)| a)
    }
}
