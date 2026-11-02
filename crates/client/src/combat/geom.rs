//! Aiming and hit-testing geometry.

use std::f64::consts::{PI, TAU};

use glam::{DQuat, DVec3};
use retrox_gen::Rng;

/// Where to aim from `pos` (moving at `vel`) to hit something at `tpos`
/// moving at `tvel` with shots at `speed` (on top of the shooter's
/// velocity; `None` for lasers).
pub fn lead_point(pos: DVec3, vel: DVec3, tpos: DVec3, tvel: DVec3, speed: Option<f64>) -> DVec3 {
    let Some(speed) = speed else {
        return tpos;
    };
    let rel = tvel - vel;
    let mut t = (tpos - pos).length() / speed;
    for _ in 0..4 {
        t = (tpos + rel * t - pos).length() / speed;
    }
    tpos + rel * t
}

/// The direction to fire: along `forward`, or swung toward `lead` if it's
/// within `gimbal` radians of it.
pub(super) fn aim(forward: DVec3, muzzle: DVec3, lead: Option<DVec3>, gimbal: f64) -> DVec3 {
    match lead {
        Some(l) => {
            let to = (l - muzzle).normalize_or(forward);
            if forward.angle_between(to) < gimbal {
                to
            } else {
                forward
            }
        }
        None => forward,
    }
}

pub(super) fn scatter(rng: &mut Rng, dir: DVec3, sigma: f64) -> DVec3 {
    let a = dir.any_orthonormal_vector();
    let b = dir.cross(a);
    let (r, t) = (rng.f64().max(1e-9), rng.f64());
    // Box–Muller: a normal spread around the line.
    let g = (-2.0 * r.ln()).sqrt() * sigma;
    let th = t * TAU;
    (dir + a * g * th.cos() + b * g * th.sin()).normalize()
}

pub(crate) fn random_dir(rng: &mut Rng) -> DVec3 {
    let z = rng.range(-1.0, 1.0);
    let a = rng.range(0.0, TAU);
    let r = (1.0 - z * z).sqrt();
    DVec3::new(r * a.cos(), z, r * a.sin())
}

/// Stick input that turns a ship with rotation `rot` toward `dir`:
/// pitch and yaw at the target, rolling to bring it overhead when it's
/// well off the nose (ships pitch faster than they yaw).
pub fn stick_toward(rot: DQuat, dir: DVec3) -> DVec3 {
    let l = rot.inverse() * dir.normalize_or(DVec3::NEG_Z);
    let pitch = l.y.atan2(-l.z);
    let yaw = l.x.atan2(-l.z);
    let off = (-l.z).clamp(-1.0, 1.0).acos();
    let roll = if off > 0.25 && (l.x * l.x + l.y * l.y) > 1e-6 {
        let r = l.x.atan2(l.y);
        // Never roll more than a right angle: past that, pitch down.
        let r = if r.abs() > PI / 2.0 {
            r - PI * r.signum()
        } else {
            r
        };
        (r * 1.2).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    DVec3::new(
        (pitch * 3.0).clamp(-1.0, 1.0),
        (yaw * 3.0).clamp(-1.0, 1.0),
        roll,
    )
}

/// Where along `a`→`b` (0..1) a segment first enters an ellipsoid at the
/// origin with semi-axes `axes`.
pub fn segment_ellipsoid(a: DVec3, b: DVec3, axes: DVec3) -> Option<f64> {
    let (a, b) = (a / axes, b / axes);
    let d = b - a;
    let dd = d.length_squared();
    if a.length_squared() <= 1.0 {
        return Some(0.0);
    }
    if dd < 1e-12 {
        return None;
    }
    let bq = a.dot(d);
    let c = a.length_squared() - 1.0;
    let disc = bq * bq - dd * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-bq - disc.sqrt()) / dd;
    (0.0..=1.0).contains(&t).then_some(t)
}
