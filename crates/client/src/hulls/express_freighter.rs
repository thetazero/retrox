//! The express freighter: a long, streamlined body of revolution in
//! cream enamel, like an express train or an airliner of the jet age. A
//! red cheatline runs nose to tail under a long row of lit windows; every
//! fairing is turned round its axis; radiators are swept like wings and
//! fins with red tips; lamps, beacons and her registration stencilled on
//! the nose. Every section it can mount has its own rendition here, all
//! meeting at the same fuselage radius.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::{bell, container, flank, gun, profile, radar, shield, soot, stencil};
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, cube, cylinder, fin, icosphere, lathe, quad, strut, torus,
};

/// The fuselage radius where sections meet.
const R: f32 = 4.6;
/// How much bigger the module kit is drawn on this hull than on a small
/// ship.
const EK: f32 = 2.6;
/// Her registration.
const REG: &str = "06-71";
/// Angles (above the horizontal, on the right flank) of the window row and
/// the cheatline under it.
const WIN_A: f32 = 0.16;
const LINE: (f32, f32) = (-0.07, 0.07);
const SEGS: u32 = 48;

const ID: Mat4 = Mat4::IDENTITY;

fn k(module: &Module) -> f32 {
    scale(module.size) * EK
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A strip on a body of revolution about `axis` (a point on the Z axis
/// line, offset in X/Y): from `z0` to `z1`, between angles `lo(z)` and
/// `hi(z)` on the right flank, mirrored onto the left, raised `inflate`.
#[allow(clippy::too_many_arguments)]
fn band(
    m: &mut Mesh,
    radius: &dyn Fn(f32) -> f32,
    z0: f32,
    z1: f32,
    lo: &dyn Fn(f32) -> f32,
    hi: &dyn Fn(f32) -> f32,
    inflate: f32,
    paint: Paint,
    axis: Vec3,
) {
    let steps = (((z1 - z0) / 0.8).ceil() as usize).max(2);
    let across = 3;
    let mut t = Mesh::default();
    for side in [1.0f32, -1.0] {
        let p = |z: f32, f: f32| {
            let a = lo(z) + (hi(z) - lo(z)) * f;
            let r = radius(z) + inflate;
            axis + Vec3::new(side * r * a.cos(), r * a.sin(), z)
        };
        for i in 0..steps {
            let (za, zb) = (
                z0 + (z1 - z0) * i as f32 / steps as f32,
                z0 + (z1 - z0) * (i + 1) as f32 / steps as f32,
            );
            for j in 0..across {
                let (fa, fb) = (j as f32 / across as f32, (j + 1) as f32 / across as f32);
                let (a, b, c, d) = (p(za, fa), p(zb, fa), p(zb, fb), p(za, fb));
                let n = (b - a).cross(d - a);
                let out = (a - axis) * Vec3::new(1.0, 1.0, 0.0);
                if n.dot(out) >= 0.0 {
                    quad(&mut t, a, b, c, d);
                } else {
                    quad(&mut t, a, d, c, b);
                }
            }
        }
    }
    m.paint(&t, ID, 1.0, paint);
}

/// A window pane on a body of revolution of radius `r` about `axis`, at
/// `z` and angle `a` (right flank), mirrored: `w` along, `h` up.
#[allow(clippy::too_many_arguments)]
fn panes(m: &mut Mesh, axis: Vec3, r: f32, z: f32, a: f32, w: f32, h: f32, paint: Paint) {
    let cube = cube();
    for side in [1.0f32, -1.0] {
        let n = Vec3::new(side * a.cos(), a.sin(), 0.0);
        let up = Vec3::new(-side * a.sin(), a.cos(), 0.0);
        let to_world = Mat4::from_cols(
            (Vec3::Z * w).extend(0.0),
            (up * h).extend(0.0),
            (n * 0.1).extend(0.0),
            (axis + n * (r + 0.01) + Vec3::Z * z).extend(1.0),
        );
        m.paint(&cube, to_world, 1.0, paint);
    }
}

/// The cream skin of a stretch of fuselage.
fn skin(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z0: f32, z1: f32) {
    let n = (((z1 - z0) / 0.7).ceil() as usize).max(2);
    m.add(
        &lathe(&profile(radius, z0, z1, n, 0.0), SEGS, 0.0, TAU),
        ID,
        1.0,
    );
}

/// The red cheatline down both flanks, with a pinstripe under it.
fn cheatline(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z0: f32, z1: f32) {
    band(
        m,
        radius,
        z0,
        z1,
        &|_| LINE.0,
        &|_| LINE.1,
        0.02,
        Paint::Red,
        Vec3::ZERO,
    );
    band(
        m,
        radius,
        z0,
        z1,
        &|_| LINE.0 - 0.06,
        &|_| LINE.0 - 0.04,
        0.02,
        Paint::Red,
        Vec3::ZERO,
    );
}

/// The window row down both flanks: a dark frame strip and lit panes.
fn windows(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z0: f32, z1: f32) {
    band(
        m,
        radius,
        z0,
        z1,
        &|_| WIN_A - 0.07,
        &|_| WIN_A + 0.07,
        0.015,
        Paint::Metal,
        Vec3::ZERO,
    );
    let pitch = 1.1;
    let n = ((z1 - z0) / pitch) as usize;
    let start = z0 + (z1 - z0 - (n as f32 - 1.0) * pitch) / 2.0;
    for i in 0..n {
        let z = start + i as f32 * pitch;
        panes(m, Vec3::ZERO, radius(z), z, WIN_A, 0.62, 0.5, Paint::Glass);
    }
}

/// A swept surface: a radiator laid out like a wing or a fin, cream with
/// dark coolant ribs across it and a red tip. `frame` puts its root at
/// the origin, span up +Y.
fn swept(m: &mut Mesh, frame: Mat4, root: (f32, f32), tip: (f32, f32), span: f32, thick: f32) {
    let chord = |(le, te): (f32, f32), a: f32, b: f32| (le + (te - le) * a, le + (te - le) * b);
    let piece = |m: &mut Mesh, a: f32, b: f32, y: (f32, f32), t: f32, p: Paint| {
        m.paint(
            &fin(chord(root, a, b), chord(tip, a, b), span, t, y),
            frame,
            1.0,
            p,
        );
    };
    piece(m, 0.0, 1.0, (0.0, 0.88), thick, Paint::Hull);
    piece(m, 0.0, 1.0, (0.88, 1.0), thick, Paint::Red);
    let ribs = 9;
    for i in 1..ribs {
        let a = i as f32 / ribs as f32;
        piece(
            m,
            a - 0.01,
            a + 0.01,
            (0.06, 0.86),
            thick * 1.12,
            Paint::Metal,
        );
    }
    // A dark root fillet.
    piece(m, 0.0, 1.0, (0.0, 0.05), thick * 1.25, Paint::Metal);
}

/// A frame for a swept surface rooted at `root`, its span along `out`.
fn surface_frame(root: Vec3, out: Vec3) -> Mat4 {
    Mat4::from_rotation_translation(Quat::from_rotation_arc(Vec3::Y, out.normalize()), root)
}

/// A turret: a smooth dome on a red ring, a sighting window, twin
/// barrels; built upright and stood on `axis`.
fn dome_turret(m: &mut Mesh, base: Vec3, axis: Vec3, module: &Module) {
    let kk = k(module);
    let s = 0.5 * kk;
    let mut t = Mesh::default();
    t.paint(
        &torus(0.1, 32, 6),
        at(Vec3::ZERO, 1.25 * s),
        1.0,
        Paint::Red,
    );
    t.add(
        &icosphere(3),
        Mat4::from_scale(Vec3::new(1.2 * s, 0.85 * s, 1.2 * s)),
        1.05,
    );
    t.paint(
        &cube(),
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.9 * s, 0.16 * s, 0.4 * s),
            Quat::from_rotation_x(0.5),
            Vec3::new(0.0, 0.6 * s, -0.75 * s),
        ),
        1.0,
        Paint::Glass,
    );
    let breech = Vec3::new(0.0, 0.3 * s, -0.9 * s);
    gun(&mut t, breech, breech.z - 2.4 * kk, is_laser(module), kk);
    let turn = Quat::from_rotation_arc(Vec3::Y, axis.normalize());
    m.add(&t, Mat4::from_rotation_translation(turn, base), 1.0);
}

/// A teardrop fairing, `len` long and `r` round, its nose at `z0`.
fn teardrop(z0: f32, len: f32, r: f32) -> Vec<(f32, f32)> {
    (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let shape = if t < 0.35 {
                (1.0 - ((0.35 - t) / 0.35).powi(2)).sqrt()
            } else {
                1.0 - smooth((t - 0.35) / 0.65) * 0.92
            };
            (z0 + len * t, r * shape)
        })
        .collect()
}

/// Navigation lights: red beacons top and bottom at `z`, white lamps on the
/// flanks.
fn beacons(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z: f32) {
    let r = radius(z);
    for y in [r + 0.1, -r - 0.1] {
        m.paint(
            &icosphere(1),
            at(Vec3::new(0.0, y, z), 0.3),
            1.0,
            Paint::Beacon,
        );
    }
}

/// Her registration on both flanks, reading nose-ward from `z0`.
fn registration(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z0: f32, a0: f32, h: f32) {
    let len = REG.len() as f32 * 0.85 * h;
    for side in [-1.0f32, 1.0] {
        let z = if side > 0.0 { z0 } else { z0 - len };
        let surface = flank(side, z, a0, radius);
        stencil(m, REG, h, Paint::Metal, &surface);
    }
}

// --- Bows -----------------------------------------------------------------

/// A rounded streamlined nose, the bridge in a wraparound band of windows
/// across it, the cheatline sweeping down to a point under its chin, a
/// headlamp in the tip.
fn command_bow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let nose = 15.0;
    let radius = move |z: f32| {
        let t = ((z - f) / nose).clamp(0.0, 1.0);
        R * (1.0 - (1.0 - t).powf(2.2)).powf(0.5)
    };
    skin(m, &radius, f + 0.02, b);
    // The tip: a lamp in a dark ring.
    m.paint(
        &icosphere(2),
        at(Vec3::new(0.0, 0.0, f + 0.35), 0.5),
        1.0,
        Paint::Lamp,
    );
    m.paint(
        &torus(0.18, 24, 6),
        Mat4::from_translation(Vec3::Z * (f + 0.5))
            * Mat4::from_rotation_x(FRAC_PI_2)
            * Mat4::from_scale(Vec3::splat(0.65)),
        1.0,
        Paint::Metal,
    );
    // The bridge band: a dark frame, panes across the top of the nose.
    let (w0, w1) = (f + 4.6, f + 6.2);
    band(
        m,
        &radius,
        w0 - 0.2,
        w1 + 0.2,
        &|_| 0.42,
        &|_| PI / 2.0,
        0.03,
        Paint::Metal,
        Vec3::ZERO,
    );
    let n = 5;
    let span = PI / 2.0 - 0.48;
    for i in 0..n {
        let a0 = 0.48 + span * i as f32 / n as f32 + 0.03;
        let a1 = 0.48 + span * (i + 1) as f32 / n as f32 - 0.03;
        band(
            m,
            &radius,
            w0,
            w1,
            &move |_| a0,
            &move |_| a1,
            0.06,
            Paint::Glass,
            Vec3::ZERO,
        );
    }
    // The cheatline sweeps down and forward to a point under the chin.
    let sweep_from = f + 13.0;
    let mid = move |z: f32| -0.85 * (1.0 - smooth((z - f - 2.0) / (sweep_from - f - 2.0)));
    let half = move |z: f32| 0.07 * smooth((z - f - 1.5) / 4.0);
    band(
        m,
        &radius,
        f + 1.6,
        sweep_from,
        &move |z| mid(z) - half(z),
        &move |z| mid(z) + half(z),
        0.02,
        Paint::Red,
        Vec3::ZERO,
    );
    cheatline(m, &radius, sweep_from, b);
    windows(m, &radius, f + 11.0, b - 0.6);
    registration(m, &radius, f + 15.5, LINE.0 - 0.3, 1.1);
    // Docking lamps either side of the bridge, a beacon top and bottom.
    for side in [-1.0f32, 1.0] {
        let z = f + 8.0;
        let a: f32 = 0.42;
        let r = radius(z);
        let q = Vec3::new(side * r * a.cos(), r * a.sin(), z);
        m.paint(&icosphere(1), at(q, 0.25), 1.0, Paint::Lamp);
    }
    beacons(m, &radius, b - 2.0);

    for (slot, module, _) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Weapon => {
                // A chin turret in a teardrop blister.
                let y = -radius(f + 7.0) + 0.5;
                m.add(
                    &lathe(&teardrop(f + 4.5, 6.0, 1.3), 20, 0.0, TAU),
                    Mat4::from_translation(Vec3::Y * y),
                    1.0,
                );
                let ball = Vec3::new(0.0, y - 0.5, f + 5.2);
                m.paint(&icosphere(2), at(ball, 0.7), 1.0, Paint::Metal);
                gun(m, ball, f - 1.0, is_laser(module), kk);
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, R - 0.05, c - 1.0), kk),
            SlotKind::Radar => radar(m, Vec3::new(0.0, R - 0.1, c + 6.0), kk),
            SlotKind::Rcs => rcs(m, &radius, c - 3.0),
            _ => {}
        }
    }
}

/// Attitude thruster grilles on both flanks at `z`.
fn rcs(m: &mut Mesh, radius: &dyn Fn(f32) -> f32, z: f32) {
    for a in [-0.55f32, 0.55] {
        panes(m, Vec3::ZERO, radius(z), z, a, 1.0, 0.6, Paint::Metal);
    }
}

/// A glazed navigator's nose, a stepped cockpit canopy above it, and two
/// gun pods slung on stub wings like engine nacelles.
fn gun_bow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let nose = 13.0;
    let radius = move |z: f32| {
        let t = ((z - f) / nose).clamp(0.0, 1.0);
        0.7 + (R - 0.7) * (1.0 - (1.0 - t).powf(2.0)).powf(0.6)
    };
    // The glazed tip: a dome of glass in a cream frame.
    let tip: Vec<(f32, f32)> = (0..=6)
        .map(|i| {
            let a = i as f32 / 6.0 * FRAC_PI_2;
            (f - 0.7 * a.cos() + 0.0, 0.7 * a.sin())
        })
        .collect();
    m.paint(&lathe(&tip, 24, 0.0, TAU), ID, 1.0, Paint::Glass);
    let glazed = f + 3.6;
    m.paint(
        &lathe(&profile(&radius, f, glazed, 6, 0.0), 24, 0.0, TAU),
        ID,
        1.0,
        Paint::Glass,
    );
    // Frames over the glazing: hoops and ribs.
    for z in [f, f + 1.2, f + 2.4, glazed] {
        m.add(
            &lathe(&profile(&radius, z - 0.08, z + 0.08, 1, 0.04), 24, 0.0, TAU),
            ID,
            1.0,
        );
    }
    for i in 0..8 {
        let a = i as f32 / 8.0 * TAU;
        m.add(
            &lathe(&profile(&radius, f, glazed, 6, 0.04), 2, a - 0.05, a + 0.05),
            ID,
            1.0,
        );
    }
    skin(m, &radius, glazed, b);
    // The canopy: a faired hump over the nose, its windscreen stepped up.
    let hump = teardrop(f + 5.0, 14.0, 2.2);
    let hump_at =
        Mat4::from_translation(Vec3::Y * (R - 1.5)) * Mat4::from_scale(Vec3::new(1.0, 0.9, 1.0));
    m.add(&lathe(&hump, 24, 0.0, TAU), hump_at, 1.0);
    let screen: Vec<(f32, f32)> = hump[1..6].iter().map(|&(z, r)| (z, r + 0.04)).collect();
    m.paint(
        &lathe(&screen, 12, 0.35, PI - 0.35),
        hump_at,
        1.0,
        Paint::Glass,
    );
    for a in [0.95f32, 1.57, 2.19] {
        let rib: Vec<(f32, f32)> = hump[1..6].iter().map(|&(z, r)| (z, r + 0.07)).collect();
        m.add(&lathe(&rib, 1, a - 0.03, a + 0.03), hump_at, 1.0);
    }
    cheatline(m, &radius, f + 7.0, b);
    windows(m, &radius, f + 13.0, b - 0.6);
    registration(m, &radius, f + 13.0, LINE.0 - 0.3, 1.0);
    beacons(m, &radius, b - 2.0);

    // Stub wings and the pods on them.
    let (px, py) = (6.6f32, -2.4f32);
    let (pz0, plen) = (f + 3.5, 14.0);
    for side in [-1.0f32, 1.0] {
        let wing_root = Vec3::new(side * 3.6, py + 0.2, 0.0);
        swept(
            m,
            surface_frame(wing_root, Vec3::new(side, -0.05, 0.0)),
            (pz0 + 4.0, pz0 + 11.0),
            (pz0 + 5.0, pz0 + 10.0),
            3.2,
            0.35,
        );
        let pod = Mat4::from_translation(Vec3::new(side * px, py, 0.0));
        let body = teardrop(pz0, plen, 1.4);
        m.add(&lathe(&body, 24, 0.0, TAU), pod, 1.0);
        // A red ring round its nose.
        let ring: Vec<(f32, f32)> = body[2..4].iter().map(|&(z, r)| (z, r + 0.03)).collect();
        m.paint(&lathe(&ring, 24, 0.0, TAU), pod, 1.0, Paint::Red);
    }
    let armed: Vec<f32> = p
        .modules()
        .filter(|(s, _, _)| s.kind == SlotKind::Weapon)
        .map(|(_, _, pos)| pos.x.signum())
        .collect();
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Weapon => {
                let mouth = Vec3::new(pos.x.signum() * px, py, pz0 + 0.3);
                m.paint(
                    &cylinder(16),
                    strut(mouth + Vec3::Z * 0.4, mouth, 0.75),
                    1.0,
                    Paint::Metal,
                );
                gun(m, mouth, pz0 - 4.5, is_laser(module), kk * 0.9);
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, R - 0.05, c + 4.0), kk),
            SlotKind::Radar => radar(m, Vec3::new(0.0, R - 0.1, c + 8.5), kk),
            SlotKind::Rcs => rcs(m, &radius, c + 1.0),
            _ => {}
        }
    }
    for side in [-1.0f32, 1.0] {
        if !armed.contains(&side) {
            // A blank cone where the gun would be.
            let cone = [(pz0 - 1.0, 0.0), (pz0 - 0.4, 0.5), (pz0 + 0.3, 0.9)];
            m.paint(
                &lathe(&cone, 16, 0.0, TAU),
                Mat4::from_translation(Vec3::new(side * px, py, 0.0)),
                1.0,
                Paint::Red,
            );
        }
    }
}

// --- Mid sections ---------------------------------------------------------

fn barrel(_: f32) -> f32 {
    R
}

/// The straight fuselage with its turrets: domes in streamlined blisters
/// on either flank, on a faired hump above and below, a V of radiator
/// fins aft.
fn gun_deck(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    skin(m, &barrel, f, b);
    cheatline(m, &barrel, f, b);
    windows(m, &barrel, f + 0.6, c - 11.5);
    windows(m, &barrel, c - 0.5, b - 0.6);
    // Blisters on the flanks.
    let zs = c - 6.0;
    for side in [-1.0f32, 1.0] {
        m.add(
            &lathe(&teardrop(zs - 5.0, 12.0, 2.0), 24, 0.0, TAU),
            Mat4::from_translation(Vec3::new(side * (R - 0.4), 0.0, 0.0)),
            1.0,
        );
        m.paint(
            &torus(0.1, 28, 6),
            Mat4::from_translation(Vec3::new(side * (R + 1.6), 0.0, zs))
                * Mat4::from_rotation_z(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(1.3)),
            1.0,
            Paint::Metal,
        );
    }
    // Humps above and below for the centreline turrets.
    for up in [-1.0f32, 1.0] {
        m.add(
            &lathe(&teardrop(c - 5.0, 12.0, 2.2), 24, 0.0, TAU),
            Mat4::from_translation(Vec3::new(0.0, up * (R - 1.4), 0.0))
                * Mat4::from_scale(Vec3::new(1.2, 1.0, 1.0)),
            1.0,
        );
    }
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Weapon if pos.x.abs() > 0.5 => {
                let side = pos.x.signum();
                dome_turret(
                    m,
                    Vec3::new(side * (R + 1.6), 0.0, zs),
                    Vec3::X * side,
                    module,
                );
            }
            SlotKind::Weapon => {
                let up = pos.y.signum();
                dome_turret(
                    m,
                    Vec3::new(0.0, up * (R + 0.75), c - 1.5),
                    Vec3::Y * up,
                    module,
                );
            }
            SlotKind::Radiator => {
                for side in [-1.0f32, 1.0] {
                    let a = FRAC_PI_2 - side * 0.6;
                    let out = Vec3::new(a.cos(), a.sin(), 0.0);
                    swept(
                        m,
                        surface_frame(out * (R - 0.2), out),
                        (b - 10.0, b - 2.5),
                        (b - 5.5, b - 1.8),
                        1.25 * kk,
                        0.3,
                    );
                }
            }
            _ => {}
        }
    }
    beacons(m, &barrel, b - 1.0);
}

/// A smooth faired collar swelling out of the fuselage, the emitters
/// standing on its crest above and below, a bright band round its waist.
fn shield_ring(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let half = 7.5;
    let swell = 2.0;
    let radius = move |z: f32| {
        let t = ((z - c) / half).clamp(-1.0, 1.0);
        R + swell * (0.5 + 0.5 * (t * PI).cos()).powf(0.8)
    };
    skin(m, &radius, f, b);
    cheatline(m, &radius, f, b);
    windows(m, &radius, f + 0.6, c - half + 0.5);
    windows(m, &radius, c + half - 0.5, b - 0.6);
    // A dark band round the crest, red pinstripes either side.
    m.paint(
        &lathe(&profile(&radius, c - 1.0, c + 1.0, 4, 0.03), SEGS, 0.0, TAU),
        ID,
        1.0,
        Paint::Metal,
    );
    for z in [c - 2.6, c + 2.6] {
        m.paint(
            &lathe(
                &profile(&radius, z - 0.18, z + 0.18, 2, 0.03),
                SEGS,
                0.0,
                TAU,
            ),
            ID,
            1.0,
            Paint::Red,
        );
    }
    // Lamps round the crest, lighting the hull.
    for i in 0..8 {
        let a = (i as f32 + 0.5) / 8.0 * TAU;
        let q = Vec3::new(a.cos(), a.sin(), 0.0) * (radius(c) + 0.05) + Vec3::Z * (c + 1.2);
        m.paint(&icosphere(1), at(q, 0.2), 1.0, Paint::Lamp);
    }
    let mut emitters = [false, false];
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Shield => {
                let up = pos.y.signum();
                emitters[(up > 0.0) as usize] = true;
                let base = Vec3::new(0.0, up * (R + swell - 0.1), c);
                m.paint(
                    &cylinder(20),
                    strut(base, base + Vec3::Y * up * 0.6, 1.0),
                    1.0,
                    Paint::Metal,
                );
                shield(m, base + Vec3::Y * up * 0.6, kk);
            }
            SlotKind::Radiator => {
                swept(
                    m,
                    surface_frame(Vec3::new(0.0, R - 0.2, 0.0), Vec3::Y),
                    (b - 6.5, b - 0.8),
                    (b - 2.5, b - 0.4),
                    1.1 * kk,
                    0.3,
                );
            }
            _ => {}
        }
    }
    for (i, up) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !emitters[i] {
            let base = Vec3::new(0.0, up * (R + swell - 0.2), c);
            m.paint(
                &cylinder(20),
                strut(base, base + Vec3::Y * up * 0.3, 1.3),
                1.0,
                Paint::Red,
            );
        }
    }
}

/// A faired container carrier: streamlined hoops carry a crew corridor
/// along the top, a keel along the bottom, and the containers between
/// them; the corridor has a long band of windows.
fn cargo_bay(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    // Bulkhead fairings at each end.
    let front = [(f, R), (f + 1.4, R), (f + 2.0, R - 0.9), (f + 2.0, 0.0)];
    let back = [(b - 2.0, 0.0), (b - 2.0, R - 0.9), (b - 1.4, R), (b, R)];
    for end in [&front[..], &back[..]] {
        m.add(&lathe(end, SEGS, 0.0, TAU), ID, 1.0);
    }
    cheatline(m, &barrel, f, f + 1.4);
    cheatline(m, &barrel, b - 1.4, b);
    // Hoops between the containers.
    let pitch = 11.0;
    for z in [c - pitch / 2.0, c + pitch / 2.0] {
        let hoop = [
            (z - 0.9, R - 0.5),
            (z - 0.7, R + 0.1),
            (z - 0.3, R + 0.3),
            (z + 0.3, R + 0.3),
            (z + 0.7, R + 0.1),
            (z + 0.9, R - 0.5),
        ];
        m.add(&lathe(&hoop, SEGS, 0.0, TAU), ID, 1.0);
        m.paint(
            &lathe(
                &[(z - 0.32, R + 0.31), (z + 0.32, R + 0.31)],
                SEGS,
                0.0,
                TAU,
            ),
            ID,
            1.0,
            Paint::Red,
        );
        // The hoop's web, inside.
        m.paint(
            &torus(0.12, SEGS, 4),
            Mat4::from_translation(Vec3::Z * z)
                * Mat4::from_rotation_x(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(R - 0.4)),
            0.8,
            Paint::Metal,
        );
    }
    // The corridor: a tube along the top with its window band.
    let (cy, cr) = (3.4f32, 1.5f32);
    let corridor = Vec3::Y * cy;
    let tube = |_: f32| cr;
    m.add(
        &lathe(&[(f + 1.5, cr), (b - 1.5, cr)], 24, 0.0, TAU),
        Mat4::from_translation(corridor),
        1.0,
    );
    band(
        m,
        &tube,
        f + 1.9,
        b - 1.9,
        &|_| -0.25,
        &|_| 0.25,
        0.01,
        Paint::Metal,
        corridor,
    );
    let n = ((b - f - 4.0) / 0.9) as usize;
    for i in 0..n {
        let z = f + 2.3 + i as f32 * 0.9;
        panes(m, corridor, cr, z, 0.0, 0.55, 0.42, Paint::Glass);
    }
    band(
        m,
        &tube,
        f + 1.9,
        b - 1.9,
        &|_| -0.42,
        &|_| -0.32,
        0.01,
        Paint::Red,
        corridor,
    );
    // Floodlights under the corridor, looking down on the cargo.
    for i in -1..=1 {
        let z = c + i as f32 * pitch + 4.0;
        m.paint(
            &cube(),
            boxed(
                corridor + Vec3::new(0.0, -cr - 0.05, z),
                Vec3::new(0.8, 0.12, 0.4),
            ),
            1.0,
            Paint::Lamp,
        );
    }
    // The keel.
    let keel = Vec3::Y * -3.6;
    m.paint(
        &lathe(&[(f + 1.5, 1.1), (b - 1.5, 1.1)], 16, 0.0, TAU),
        Mat4::from_translation(keel),
        1.0,
        Paint::Metal,
    );
    // Cradle arms for each container, from the keel.
    let cyl = cylinder(8);
    for i in -1..=1 {
        let z = c + i as f32 * pitch;
        for dz in [-3.6f32, 3.6] {
            for side in [-1.0f32, 1.0] {
                let a = keel + Vec3::new(0.0, 0.6, z + dz);
                let elbow = Vec3::new(side * 3.6, -2.6, z + dz);
                let top = Vec3::new(side * 3.6, 1.0, z + dz);
                m.paint(&cyl, strut(a, elbow, 0.2), 1.0, Paint::Metal);
                m.paint(&cyl, strut(elbow, top, 0.2), 1.0, Paint::Metal);
                m.paint(
                    &cube(),
                    boxed(top, Vec3::new(0.5, 0.4, 0.5)),
                    1.0,
                    Paint::Red,
                );
            }
        }
    }
    let paints = [Paint::Hull, Paint::Red, Paint::Hull];
    for (n, (slot, _, pos)) in p.modules().enumerate() {
        if slot.kind == SlotKind::Cargo {
            container(
                m,
                Vec3::new(0.0, -0.4, pos.z),
                Vec3::new(6.6, 4.4, 9.4),
                paints[n % paints.len()],
            );
        }
    }
    m.paint(
        &icosphere(1),
        at(corridor + Vec3::new(0.0, cr + 0.15, b - 3.0), 0.3),
        1.0,
        Paint::Beacon,
    );
}

/// The wing section: the radiators spread either side like an
/// airliner's swept wings over a wing-root fairing, the reactor in a
/// polished pod beneath, its vents lit when it is fitted.
fn reactor_section(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    skin(m, &barrel, f, b);
    cheatline(m, &barrel, f, b);
    windows(m, &barrel, f + 0.6, b - 0.6);
    // The wing-root fairing and the reactor pod under it.
    let root = Mat4::from_translation(Vec3::new(0.0, -2.2, 0.0))
        * Mat4::from_scale(Vec3::new(1.35, 0.9, 1.0));
    m.add(
        &lathe(&teardrop(f + 1.0, b - f - 1.5, 3.4), 32, 0.0, TAU),
        root,
        1.0,
    );
    let pod = Vec3::new(0.0, -5.2, 0.0);
    let pod_body = teardrop(c - 7.0, 15.0, 1.9);
    m.paint(
        &lathe(&pod_body, 24, 0.0, TAU),
        Mat4::from_translation(pod),
        1.0,
        Paint::Metal,
    );
    let mut wings = [false, false];
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Radiator => {
                let side = pos.x.signum();
                wings[(side > 0.0) as usize] = true;
                let dihedral: f32 = 0.08;
                let out = Vec3::new(side * dihedral.cos(), dihedral.sin(), 0.0);
                let span = 2.6 * kk;
                swept(
                    m,
                    surface_frame(Vec3::new(side * (R - 0.5), -2.0, 0.0), out),
                    (c - 7.0, c + 5.0),
                    (c + 3.0, c + 7.5),
                    span,
                    0.6,
                );
                // A light on each wingtip.
                let tip = Vec3::new(side * (R - 0.5), -2.0, 0.0) + out * span + Vec3::Z * (c + 5.0);
                m.paint(
                    &icosphere(1),
                    at(tip, 0.3),
                    1.0,
                    if side > 0.0 {
                        Paint::Lamp
                    } else {
                        Paint::Beacon
                    },
                );
            }
            SlotKind::Power => {
                // Lit vents round the pod.
                for i in 0..6 {
                    let a = i as f32 / 6.0 * TAU;
                    let d = Vec3::new(a.cos(), a.sin(), 0.0);
                    let z = c + (pos.z - c).signum() * 3.2;
                    m.paint(
                        &cube(),
                        Mat4::from_rotation_translation(
                            Quat::from_rotation_arc(Vec3::Y, d),
                            pod + d * 1.93 + Vec3::Z * z,
                        ) * Mat4::from_scale(Vec3::new(0.35, 0.08, 1.2)),
                        1.0,
                        Paint::Lamp,
                    );
                }
            }
            _ => {}
        }
    }
    // Red rings on the pod, where the power units sit.
    for z in [c - 4.6, c - 1.8, c + 1.8, c + 4.6] {
        let r = pod_body.iter().find(|q| q.0 >= z).map_or(1.0, |q| q.1);
        m.paint(
            &lathe(&[(z - 0.2, r + 0.05), (z + 0.2, r + 0.05)], 24, 0.0, TAU),
            Mat4::from_translation(pod),
            1.0,
            Paint::Red,
        );
    }
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !wings[i] {
            swept(
                m,
                surface_frame(Vec3::new(side * (R - 0.5), -2.0, 0.0), Vec3::X * side),
                (c - 5.0, c + 3.0),
                (c - 3.0, c + 2.0),
                2.0,
                0.6,
            );
        }
    }
}

// --- Sterns ---------------------------------------------------------------

/// A boat tail with two engines in nacelles on its shoulders, a tall swept
/// fin above and a ventral fin below: the radiators.
fn sprint_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let radius = move |z: f32| R - (R - 1.4) * smooth((z - f - 4.0) / (b - f - 4.0)).powf(0.9);
    skin(m, &radius, f, b);
    let cap = [(b, radius(b)), (b + 0.6, 1.0), (b + 0.9, 0.0)];
    m.paint(&lathe(&cap, 24, 0.0, TAU), ID, 1.0, Paint::Metal);
    m.paint(
        &icosphere(1),
        at(Vec3::Z * (b + 0.9), 0.3),
        1.0,
        Paint::Lamp,
    );
    cheatline(m, &radius, f, b - 1.5);
    windows(m, &radius, f + 0.6, f + 5.0);
    // Nacelles on short pylons.
    let (nz0, nlen, ny) = (c - 4.0, 13.5, 1.2);
    let nx = |side: f32| side * (radius(c + 2.0) + 2.6);
    let mut fitted = [false, false];
    for (slot, _, pos) in p.modules() {
        if slot.kind == SlotKind::Engine {
            fitted[(pos.x > 0.0) as usize] = true;
        }
    }
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let x = nx(side);
        let at_n = Mat4::from_translation(Vec3::new(x, ny, 0.0));
        let body: Vec<(f32, f32)> = (0..=12)
            .map(|j| {
                let t = j as f32 / 12.0;
                let r =
                    2.1 * (1.0 - 0.25 * smooth((t - 0.6) / 0.4)) * (0.82 + 0.18 * smooth(t / 0.2));
                (nz0 + nlen * t, r)
            })
            .collect();
        m.add(&lathe(&body, 28, 0.0, TAU), at_n, 1.0);
        // Intake lip, a red ring.
        m.paint(
            &torus(0.2, 28, 8),
            at_n * Mat4::from_translation(Vec3::Z * nz0)
                * Mat4::from_rotation_x(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(1.72)),
            1.0,
            Paint::Metal,
        );
        // Inside it, the compressor face and a red spinner.
        m.paint(
            &lathe(&[(nz0 + 0.6, 0.0), (nz0 + 0.6, 1.8)], 24, 0.0, TAU),
            at_n,
            0.5,
            Paint::Metal,
        );
        m.paint(
            &lathe(
                &[(nz0 - 0.2, 0.0), (nz0 + 0.1, 0.35), (nz0 + 0.6, 0.6)],
                16,
                0.0,
                TAU,
            ),
            at_n,
            1.0,
            Paint::Red,
        );
        m.paint(
            &lathe(
                &body[3..5]
                    .iter()
                    .map(|&(z, r)| (z, r + 0.03))
                    .collect::<Vec<_>>(),
                28,
                0.0,
                TAU,
            ),
            at_n,
            1.0,
            Paint::Red,
        );
        // The pylon.
        swept(
            m,
            surface_frame(
                Vec3::new(side * (radius(c) - 0.4), ny - 0.3, 0.0),
                Vec3::new(side, 0.12, 0.0),
            ),
            (nz0 + 3.0, nz0 + 10.0),
            (nz0 + 3.5, nz0 + 9.0),
            (x.abs() - radius(c)).max(0.5) + 0.3,
            0.4,
        );
        let end = nz0 + nlen;
        if !fitted[i] {
            let cone = [(end, 1.6), (end + 1.4, 0.8), (end + 2.0, 0.0)];
            m.add(&lathe(&cone, 20, 0.0, TAU), at_n, 0.9);
        }
    }
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(nx(pos.x.signum()), ny, nz0 + nlen - 0.2);
                nozzles.push(bell(m, throat, 0.8 * kk, 1.45));
            }
            SlotKind::Radiator => {
                let up = pos.y.signum();
                let zr = c + 4.0;
                let span = if up > 0.0 { 1.8 * kk } else { 1.0 * kk };
                swept(
                    m,
                    surface_frame(Vec3::new(0.0, up * (radius(zr) - 0.3), 0.0), Vec3::Y * up),
                    (zr - 8.0, zr + 4.0),
                    (zr + 1.0, zr + 6.0),
                    span,
                    0.45,
                );
                if up > 0.0 {
                    m.paint(
                        &icosphere(1),
                        at(Vec3::new(0.0, radius(zr) - 0.3 + span + 0.2, zr + 4.0), 0.3),
                        1.0,
                        Paint::Beacon,
                    );
                }
            }
            _ => {}
        }
    }
    soot(m, c + 2.0, b + 4.0, 0.3);
}

/// A tapering tail ending in one big engine in a cowled nacelle,
/// radiators swept back either side like tailplanes, and a tall fixed fin
/// carrying her registration.
fn cruise_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let cowl = b - 3.5;
    let radius = move |z: f32| R - (R - 2.7) * smooth((z - f - 3.0) / (cowl - f - 3.0));
    skin(m, &radius, f, cowl);
    cheatline(m, &radius, f, cowl - 1.0);
    windows(m, &radius, f + 0.6, f + 6.0);
    // The cowl: a red ring, then a dark lip.
    m.paint(
        &lathe(&[(cowl - 2.4, 2.75), (cowl - 1.6, 2.75)], 32, 0.0, TAU),
        ID,
        1.0,
        Paint::Red,
    );
    let lip = [
        (cowl, 2.7),
        (cowl + 0.6, 2.75),
        (cowl + 0.9, 2.5),
        (cowl + 0.9, 1.8),
    ];
    m.paint(&lathe(&lip, 32, 0.0, TAU), ID, 1.0, Paint::Metal);
    // The fixed fin.
    let fz = c + 1.0;
    let fin_frame = surface_frame(Vec3::new(0.0, radius(fz) - 0.3, 0.0), Vec3::Y);
    let fin_root = (fz - 3.0, fz + 7.0);
    let fin_tip = (fz + 4.0, fz + 7.5);
    m.add(
        &fin(fin_root, fin_tip, 7.0, 0.5, (0.0, 0.85)),
        fin_frame,
        1.0,
    );
    m.paint(
        &fin(fin_root, fin_tip, 7.0, 0.5, (0.85, 1.0)),
        fin_frame,
        1.0,
        Paint::Red,
    );
    m.paint(
        &icosphere(1),
        at(Vec3::new(0.0, radius(fz) + 6.8, fz + 7.6), 0.3),
        1.0,
        Paint::Beacon,
    );
    for side in [-1.0f32, 1.0] {
        let origin = Vec3::new(
            side * 0.35,
            radius(fz) + 2.2,
            fz + if side > 0.0 { 6.3 } else { 1.7 },
        );
        let surface = super::kit::flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
        stencil(m, REG, 1.0, Paint::Metal, &surface);
    }
    let mut planes = [false, false];
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Engine => {
                nozzles.push(bell(m, Vec3::new(0.0, 0.0, cowl + 0.7), 0.9 * kk, 1.75));
            }
            SlotKind::Radiator => {
                let side = pos.x.signum();
                planes[(side > 0.0) as usize] = true;
                let zr = c + 4.0;
                let out = Vec3::new(side, 0.1, 0.0);
                swept(
                    m,
                    surface_frame(Vec3::new(side * (radius(zr) - 0.3), 0.0, 0.0), out),
                    (zr - 5.0, zr + 4.0),
                    (zr + 1.5, zr + 5.0),
                    1.7 * kk,
                    0.4,
                );
            }
            _ => {}
        }
    }
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !planes[i] {
            let zr = c + 4.0;
            swept(
                m,
                surface_frame(
                    Vec3::new(side * (radius(zr) - 0.3), 0.0, 0.0),
                    Vec3::X * side,
                ),
                (zr - 2.0, zr + 3.0),
                (zr, zr + 2.5),
                1.5,
                0.4,
            );
        }
    }
    soot(m, c, b + 4.0, 0.3);
}

/// A plain stretch of fuselage, for a section with no rendition of its
/// own.
fn plain(m: &mut Mesh, p: &Placed) {
    let (f, b) = (p.front, p.back);
    skin(m, &barrel, f, b);
    cheatline(m, &barrel, f, b);
    windows(m, &barrel, f + 0.6, b - 0.6);
    for (slot, module, pos) in p.modules() {
        if slot.kind == SlotKind::Weapon {
            dome_turret(m, Vec3::new(0.0, R, pos.z), Vec3::Y, module);
        }
    }
}

/// The express freighter, built section by section from its fit.
pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut m = Mesh::default();
    let mut nozzles = Vec::new();
    for p in placed(fit) {
        match (p.section.id, p.section.position) {
            ("command-bow-l", _) => command_bow(&mut m, &p),
            ("gun-bow-l", _) => gun_bow(&mut m, &p),
            ("gun-deck-l", _) => gun_deck(&mut m, &p),
            ("shield-ring-l", _) => shield_ring(&mut m, &p),
            ("cargo-bay-l", _) => cargo_bay(&mut m, &p),
            ("reactor-section-l", _) => reactor_section(&mut m, &p),
            ("sprint-stern-l", _) => sprint_stern(&mut m, &p, &mut nozzles),
            ("cruise-stern-l", _) => cruise_stern(&mut m, &p, &mut nozzles),
            (_, Position::Bow) => command_bow(&mut m, &p),
            (_, Position::Stern) => cruise_stern(&mut m, &p, &mut nozzles),
            _ => plain(&mut m, &p),
        }
    }
    (m, ShipShape { nozzles })
}
