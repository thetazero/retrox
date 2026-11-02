//! The module kit: guns, containers, radiators, radar, shield emitters and
//! engine bells, plus markings, soot and flying surfaces. Sizes are in
//! multiples `k` of a small module on a small ship.

use std::f32::consts::TAU;

use glam::{Mat4, Quat, Vec3};

use super::at;
use crate::meshes::{Mesh, Paint, boxed, cube, cylinder, fin, icosphere, lathe, strut, torus};

/// A gun from `breech` forward to `muzzle_z`: twin barrels with muzzle
/// brakes for cannon, one finned barrel with a glowing lens for lasers.
pub(super) fn gun(m: &mut Mesh, breech: Vec3, muzzle_z: f32, laser: bool, k: f32) {
    let cyl = cylinder(10);
    let tip = |off: Vec3| Vec3::new(breech.x + off.x, breech.y + off.y, muzzle_z);
    if laser {
        m.paint(
            &cyl,
            strut(breech, tip(Vec3::ZERO), 0.07 * k),
            1.0,
            Paint::Metal,
        );
        let len = breech.z - muzzle_z;
        for f in [0.25f32, 0.4, 0.55] {
            let z = breech.z - len * f;
            m.paint(
                &cyl,
                strut(
                    Vec3::new(breech.x, breech.y, z),
                    Vec3::new(breech.x, breech.y, z - 0.06),
                    0.15 * k,
                ),
                1.0,
                Paint::Metal,
            );
        }
        m.paint(
            &icosphere(1),
            at(tip(Vec3::ZERO), 0.1 * k),
            1.0,
            Paint::Glass,
        );
    } else {
        for side in [-1.0f32, 1.0] {
            let off = Vec3::X * side * 0.09 * k;
            m.paint(
                &cyl,
                strut(breech + off, tip(off), 0.05 * k),
                1.0,
                Paint::Metal,
            );
            m.paint(
                &cyl,
                strut(tip(off) + Vec3::Z * 0.25 * k, tip(off), 0.075 * k),
                0.8,
                Paint::Metal,
            );
        }
    }
}

/// A cargo container hung at `centre`: ribbed sides, a painted band.
pub(super) fn container(m: &mut Mesh, centre: Vec3, size: Vec3, paint: Paint) {
    let cube = cube();
    m.paint(&cube, boxed(centre, size), 1.0, paint);
    let ribs = 5;
    for i in 0..ribs {
        let z = centre.z - size.z / 2.0 + size.z * (i as f32 + 0.5) / ribs as f32;
        m.paint(
            &cube,
            boxed(
                Vec3::new(centre.x, centre.y, z),
                Vec3::new(size.x + 0.06, size.y * 0.94, 0.08),
            ),
            0.82,
            paint,
        );
    }
    // Locking bars on the end doors.
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cube,
            boxed(
                centre + Vec3::new(side * size.x * 0.2, 0.0, -size.z / 2.0 - 0.02),
                Vec3::new(0.05, size.y * 0.85, 0.05),
            ),
            1.0,
            Paint::Metal,
        );
    }
}

/// Where markings go: layout position (along the text, up it) to a point
/// on the surface and the text's right and up directions there (the
/// outward normal is right × up).
pub(super) type Surface<'a> = &'a dyn Fn(f32, f32) -> (Vec3, Vec3, Vec3);

/// A flat surface from `origin` (the text's bottom left).
pub(super) fn flat(origin: Vec3, right: Vec3, up: Vec3) -> impl Fn(f32, f32) -> (Vec3, Vec3, Vec3) {
    move |u, v| (origin + right * u + up * v, right, up)
}

/// The flank of a body of revolution about Z, on the `side` (±1) of x:
/// text reading nose-ward from `z0`, its foot at angle `a0` above
/// horizontal, wrapped round the curve. `radius(z)` is the surface.
pub(super) fn flank(
    side: f32,
    z0: f32,
    a0: f32,
    radius: impl Fn(f32) -> f32,
) -> impl Fn(f32, f32) -> (Vec3, Vec3, Vec3) {
    move |u, v| {
        let z = z0 - side * u;
        let r = radius(z);
        let a = a0 + v / r;
        (
            Vec3::new(side * r * a.cos(), r * a.sin(), z),
            Vec3::new(0.0, 0.0, -side),
            Vec3::new(-side * a.sin(), a.cos(), 0.0),
        )
    }
}

/// Stencilled markings in seven-segment figures, like a VFD readout:
/// `text` of digits, '-' and spaces, `h` tall, laid out on `surface` and
/// raised just off it.
pub(super) fn stencil(m: &mut Mesh, text: &str, h: f32, paint: Paint, surface: Surface) {
    // Segments a–g (top, top right, bottom right, bottom, bottom left,
    // top left, middle) lit for each figure.
    const FIGURES: [u8; 10] = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    let (w, t) = (0.55 * h, 0.14 * h);
    let (long_h, long_v) = (w - t * 1.3, h / 2.0 - t * 1.3);
    // (centre u, centre v, size u, size v) for each segment.
    let segments = [
        (w / 2.0, h - t / 2.0, long_h, t),
        (w - t / 2.0, h * 0.75, t, long_v),
        (w - t / 2.0, h * 0.25, t, long_v),
        (w / 2.0, t / 2.0, long_h, t),
        (t / 2.0, h * 0.25, t, long_v),
        (t / 2.0, h * 0.75, t, long_v),
        (w / 2.0, h / 2.0, long_h, t),
    ];
    let cube = cube();
    let mut u0 = 0.0;
    for ch in text.chars() {
        let lit = match ch {
            '0'..='9' => FIGURES[ch as usize - '0' as usize],
            '-' => 0b1000000,
            _ => 0,
        };
        for (k, &(cu, cv, su, sv)) in segments.iter().enumerate() {
            if lit & (1 << k) == 0 {
                continue;
            }
            let (at, right, up) = surface(u0 + cu, cv);
            let (right, up) = (right.normalize(), up.normalize());
            let normal = right.cross(up).normalize();
            let centre = at + normal * 0.6 * t;
            let to_world = Mat4::from_cols(
                (right * su).extend(0.0),
                (up * sv).extend(0.0),
                (normal * t).extend(0.0),
                centre.extend(1.0),
            );
            m.paint(&cube, to_world, 1.0, paint);
        }
        u0 += w + 0.3 * h;
    }
}

/// Darken everything (but lamps and glass) toward the engines, from
/// nothing at `z0` to `amount` at `z1`.
pub(super) fn soot(m: &mut Mesh, z0: f32, z1: f32, amount: f32) {
    let lit = [Paint::Glass, Paint::Beacon, Paint::Lamp].map(|p| p as u32 as f32);
    for v in &mut m.vertices {
        if lit.contains(&v.paint) {
            continue;
        }
        let t = ((v.pos[2] - z0) / (z1 - z0)).clamp(0.0, 1.0);
        v.shade *= 1.0 - amount * t * t * (3.0 - 2.0 * t);
    }
}

/// A flying surface (see [`fin`]) with a split control surface along its
/// trailing edge, and its outer span from `tip_from` painted `tip_paint`.
#[allow(clippy::too_many_arguments)]
pub(super) fn flapped(
    m: &mut Mesh,
    to_world: Mat4,
    root: (f32, f32),
    tip: (f32, f32),
    span: f32,
    thick: f32,
    paint: Paint,
    tip_paint: Paint,
    tip_from: f32,
) {
    let chord = |(le, te): (f32, f32), a: f32, b: f32| (le + (te - le) * a, le + (te - le) * b);
    let piece = |m: &mut Mesh, a: f32, b: f32, y: (f32, f32), t: f32, shade: f32, p: Paint| {
        m.paint(
            &fin(chord(root, a, b), chord(tip, a, b), span, t, y),
            to_world,
            shade,
            p,
        );
    };
    piece(m, 0.0, 0.78, (0.0, tip_from), thick, 1.0, paint);
    piece(m, 0.0, 0.78, (tip_from, 1.0), thick, 1.0, tip_paint);
    piece(m, 0.78, 1.0, (0.0, 0.12), thick * 0.45, 1.0, paint);
    piece(m, 0.8, 1.0, (0.13, 0.84), thick * 0.4, 0.8, paint);
    piece(m, 0.78, 1.0, (0.85, 1.0), thick * 0.45, 1.0, tip_paint);
}

/// A radiator panel standing out from `root` along `out`, `len` long in Z:
/// a white plate with dark coolant ribs.
pub(super) fn radiator(m: &mut Mesh, root: Vec3, out: Vec3, height: f32, len: f32) {
    let cube = cube();
    let rot = Quat::from_rotation_arc(Vec3::Y, out.normalize());
    let panel = |m: &mut Mesh, size: Vec3, offset_z: f32, shade: f32, paint: Paint| {
        m.paint(
            &cube,
            Mat4::from_scale_rotation_translation(
                size,
                rot,
                root + out.normalize() * height / 2.0 + Vec3::Z * offset_z,
            ),
            shade,
            paint,
        );
    };
    panel(m, Vec3::new(0.05, height, len), 0.0, 1.1, Paint::Hull);
    let ribs = (len / 0.3) as i32;
    for i in 0..ribs {
        let z = -len / 2.0 + len * (i as f32 + 0.5) / ribs as f32;
        panel(
            m,
            Vec3::new(0.08, height * 0.92, 0.03),
            z,
            1.0,
            Paint::Metal,
        );
    }
    // A dark frame round the ends.
    for end in [-1.0f32, 1.0] {
        panel(
            m,
            Vec3::new(0.1, height, 0.07),
            end * len / 2.0,
            1.0,
            Paint::Metal,
        );
    }
    // A red edge along the top.
    m.paint(
        &cube,
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.1, 0.12, len),
            rot,
            root + out.normalize() * (height - 0.06),
        ),
        1.0,
        Paint::Red,
    );
}

/// A radar on a mast from `base`: a dish tilted up and forward, a red-tipped
/// feed. Long-range sets get a bigger dish on a taller mast.
pub(super) fn radar(m: &mut Mesh, base: Vec3, k: f32) {
    let top = base + Vec3::Y * 0.55 * k;
    m.paint(&cylinder(8), strut(base, top, 0.05 * k), 1.0, Paint::Metal);
    let dish: Vec<(f32, f32)> = (0..=6)
        .map(|i| {
            let r = 0.4 * k * i as f32 / 6.0;
            (r * r * 0.9 / k, r)
        })
        .collect();
    let facing = Quat::from_rotation_arc(Vec3::NEG_Z, Vec3::new(0.0, 0.7, -1.0).normalize());
    let to_world = Mat4::from_rotation_translation(facing, top);
    m.add(&lathe(&dish, 16, 0.0, TAU), to_world, 1.1);
    let feed = to_world.transform_point3(Vec3::new(0.0, 0.0, -0.35 * k));
    m.paint(&cylinder(6), strut(top, feed, 0.02 * k), 1.0, Paint::Metal);
    m.paint(&icosphere(1), at(feed, 0.05 * k), 1.0, Paint::Red);
}

/// A deflector emitter: a low dome in a red ring.
pub(super) fn shield(m: &mut Mesh, centre: Vec3, k: f32) {
    m.paint(
        &icosphere(2),
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.32, 0.18, 0.32) * k,
            Quat::IDENTITY,
            centre,
        ),
        1.0,
        Paint::Metal,
    );
    m.paint(&torus(0.15, 24, 6), at(centre, 0.34 * k), 1.0, Paint::Red);
}

/// An engine bell from `throat` aft, flaring; returns the nozzle (exit, radius).
pub(super) fn bell(m: &mut Mesh, throat: Vec3, k: f32, r0: f32) -> (Vec3, f32) {
    let (len, r1) = (1.0 * k, r0 * 1.7);
    let profile: Vec<(f32, f32)> = (0..=8)
        .map(|i| {
            let t = i as f32 / 8.0;
            (len * t, r0 + (r1 - r0) * t.powf(1.6))
        })
        .collect();
    m.paint(
        &lathe(&profile, 20, 0.0, TAU),
        Mat4::from_translation(throat),
        1.0,
        Paint::Metal,
    );
    (throat + Vec3::Z * len, r1)
}
