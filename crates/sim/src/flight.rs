//! Flight inside a system: one model and one autopilot for every ship.
//!
//! A ship is a point with a position and a velocity, in metres and m/s, in
//! its star's frame. It moves only by thrusting, up to its acceleration.
//! The autopilot burns toward a goal and brakes on a curve that brings it
//! to rest there, matching the goal's own velocity (planets and stations
//! orbit). [`fly`] steps that physics over any stretch of time, so sped-up
//! time is the same flight in bigger slices, never a shortcut: wherever a
//! trip is interrupted, the ship is somewhere real, moving at a real
//! speed. That's what lets anything in the system find and intercept it.
//!
//! The frame's axes don't matter here; the sim and the client each use
//! their own.

use serde::{Deserialize, Serialize};

pub type Vec3 = [f64; 3];

/// Where a ship is and how it's moving, in its star's frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kinematics {
    /// Metres.
    pub pos: Vec3,
    /// Metres per second.
    pub vel: Vec3,
}

/// Where the autopilot is going: a point, moving at `vel`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub pos: Vec3,
    pub vel: Vec3,
}

/// Arrived: this close to the goal...
pub const ARRIVE_DISTANCE_M: f64 = 500.0;
/// ...and moving this slowly relative to it.
pub const ARRIVE_SPEED: f64 = 5.0;
/// Brake on a curve that only needs this share of full thrust, keeping the
/// rest for steering onto a moving goal.
const BRAKE_SHARE: f64 = 0.8;
/// Each step is at most this share of the time left to the goal...
const STEP_SHARE: f64 = 0.1;
/// ...and between these bounds, in seconds.
const MIN_STEP_S: f64 = 0.05;
const MAX_STEP_S: f64 = 1800.0;

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn length(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

pub fn arrived(ship: &Kinematics, goal: &Goal) -> bool {
    length(sub(goal.pos, ship.pos)) <= ARRIVE_DISTANCE_M
        && length(sub(ship.vel, goal.vel)) <= ARRIVE_SPEED
}

/// The thrust, in m/s², that the autopilot asks for over the next `dt`
/// seconds: close on the goal no faster than the ship can still brake from.
pub fn autopilot(ship: &Kinematics, goal: &Goal, accel: f64, dt: f64) -> Vec3 {
    let to_goal = sub(goal.pos, ship.pos);
    let d = length(to_goal);
    let closing = if d > 0.0 {
        let speed = (2.0 * accel * BRAKE_SHARE * d).sqrt().min(d / dt);
        scale(to_goal, speed / d)
    } else {
        [0.0; 3]
    };
    let want = add(goal.vel, closing);
    let thrust = scale(sub(want, ship.vel), 1.0 / dt);
    let a = length(thrust);
    if a > accel {
        scale(thrust, accel / a)
    } else {
        thrust
    }
}

/// How long to step: a small share of the time left to the goal.
fn step_size(ship: &Kinematics, goal: &Goal, accel: f64) -> f64 {
    let d = length(sub(goal.pos, ship.pos));
    let speed = length(sub(ship.vel, goal.vel));
    // From rest, a brake-curve trip takes about 2·sqrt(d/a); at speed, d/v.
    let left = d / speed.max((accel * d).sqrt()).max(1e-9);
    (left * STEP_SHARE).clamp(MIN_STEP_S, MAX_STEP_S)
}

/// Advance `ship` by one step of `dt` seconds under `thrust` m/s².
pub fn step(ship: &mut Kinematics, thrust: Vec3, dt: f64) {
    ship.vel = add(ship.vel, scale(thrust, dt));
    ship.pos = add(ship.pos, scale(ship.vel, dt));
}

/// Fly `ship` under autopilot for up to `seconds`. `goal(t, ship)` says
/// where the goal is `t` seconds from now, given where the ship is then. Returns how long it took to arrive,
/// or `None` if it's still flying.
pub fn fly(
    ship: &mut Kinematics,
    accel: f64,
    seconds: f64,
    mut goal: impl FnMut(f64, &Kinematics) -> Goal,
) -> Option<f64> {
    let mut t = 0.0;
    loop {
        let g = goal(t, ship);
        if arrived(ship, &g) {
            return Some(t);
        }
        if t >= seconds {
            return None;
        }
        let dt = step_size(ship, &g, accel).min(seconds - t);
        step(ship, autopilot(ship, &g, accel, dt), dt);
        t += dt;
    }
}

/// Seconds a brake-curve trip of `metres` takes from rest to rest, at
/// `accel` m/s². An estimate for planning; [`fly`] is the real thing.
pub fn trip_seconds(metres: f64, accel: f64) -> f64 {
    if accel <= 0.0 {
        return f64::INFINITY;
    }
    2.0 * (metres / (accel * BRAKE_SHARE)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AU: f64 = 1.496e11;

    #[test]
    fn flies_to_a_still_goal_at_about_the_planned_time() {
        let mut ship = Kinematics::default();
        let goal = Goal {
            pos: [AU, 0.0, 0.0],
            vel: [0.0; 3],
        };
        let t = fly(&mut ship, 10.0, 1e9, |_, _| goal).expect("arrives");
        let plan = trip_seconds(AU, 10.0);
        assert!(
            (t / plan - 1.0).abs() < 0.1,
            "took {t:.0} s, planned {plan:.0} s"
        );
        assert!(arrived(&ship, &goal));
    }

    #[test]
    fn catches_an_orbiting_goal() {
        // A planet on a 1 AU circular orbit, about 30 km/s.
        let omega = 2.0 * std::f64::consts::PI / (365.25 * 86_400.0);
        let orbit = |t: f64| Goal {
            pos: [AU * (omega * t).cos(), AU * (omega * t).sin(), 0.0],
            vel: [
                -AU * omega * (omega * t).sin(),
                AU * omega * (omega * t).cos(),
                0.0,
            ],
        };
        let mut ship = Kinematics {
            pos: [-AU, 0.5 * AU, 0.0],
            vel: [0.0; 3],
        };
        let t = fly(&mut ship, 20.0, 30.0 * 86_400.0, |t, _| orbit(t)).expect("arrives");
        assert!(arrived(&ship, &orbit(t)));
    }

    #[test]
    fn slices_of_time_fly_the_same_trip() {
        let goal = |_, _: &Kinematics| Goal {
            pos: [0.2 * AU, 0.1 * AU, 0.0],
            vel: [0.0; 3],
        };
        let mut whole = Kinematics::default();
        let t = fly(&mut whole, 10.0, 1e9, goal).expect("arrives");
        let mut sliced = Kinematics::default();
        let mut elapsed = 0.0;
        loop {
            match fly(&mut sliced, 10.0, 3_600.0, goal) {
                Some(dt) => break elapsed += dt,
                None => elapsed += 3_600.0,
            }
        }
        assert!((elapsed / t - 1.0).abs() < 0.02, "{elapsed:.0} vs {t:.0}");
    }
}
