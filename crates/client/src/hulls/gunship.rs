//! The gunship: a heavy armoured spine with slab sides, riveted plates
//! and frame hoops between its sections, in gunmetal and olive drab with
//! signal red only where it means something (muzzles, insulators, the
//! number). Every section it can mount has its own rendition here, built
//! round the same octagonal spine so any bow, mid and stern mate.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Mat4, Quat, Vec2, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::{bell, container, flat, gun, radar, radiator, shield, soot, stencil};
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, icosphere, lathe, loft, strut,
    torus,
};

/// The spine's cross-section where sections meet: width, height, chamfer.
const W: f32 = 5.0;
const H: f32 = 4.0;
const C: f32 = 0.9;
/// How much bigger the module kit is drawn on this hull than on a small
/// ship.
const GK: f32 = 2.0;
/// Her number.
const NUMBER: &str = "41";

const ID: Mat4 = Mat4::IDENTITY;

fn spine(z: f32) -> (f32, Vec<Vec2>) {
    (z, chamfer_rect(W, H, C, 0.0))
}

/// Gunmetal all over, olive drab on the flat flanks.
fn livery(_: usize, e: usize) -> Paint {
    if e == 2 || e == 6 {
        Paint::Olive
    } else {
        Paint::Gunmetal
    }
}

fn bx(m: &mut Mesh, c: Vec3, s: Vec3, paint: Paint) {
    m.paint(&cube(), boxed(c, s), 1.0, paint);
}

/// A row of `n` rivet heads from `a` to `b`.
fn rivets(m: &mut Mesh, a: Vec3, b: Vec3, n: usize) {
    let cube = cube();
    for i in 0..n {
        let t = if n > 1 {
            i as f32 / (n - 1) as f32
        } else {
            0.5
        };
        m.paint(
            &cube,
            boxed(a.lerp(b, t), Vec3::splat(0.1)),
            1.0,
            Paint::Metal,
        );
    }
}

/// An armour plate bolted on facing `out` (±X or ±Y): a slab with rivet
/// rows along its long edges.
fn plate(m: &mut Mesh, c: Vec3, size: Vec3, out: Vec3, paint: Paint, shade: f32) {
    m.paint(&cube(), boxed(c, size), shade, paint);
    let n = ((size.z / 0.55) as usize).max(2);
    let (z0, z1) = (c.z - size.z / 2.0 + 0.2, c.z + size.z / 2.0 - 0.2);
    if out.x.abs() > 0.5 {
        let x = c.x + out.x.signum() * (size.x / 2.0 + 0.02);
        for y in [c.y - size.y / 2.0 + 0.16, c.y + size.y / 2.0 - 0.16] {
            rivets(m, Vec3::new(x, y, z0), Vec3::new(x, y, z1), n);
        }
    } else {
        let y = c.y + out.y.signum() * (size.y / 2.0 + 0.02);
        for x in [c.x - size.x / 2.0 + 0.16, c.x + size.x / 2.0 - 0.16] {
            rivets(m, Vec3::new(x, y, z0), Vec3::new(x, y, z1), n);
        }
    }
}

/// Bolted slabs down both flanks of the spine between `z0` and `z1`,
/// in `n` plates.
fn flank_slabs(m: &mut Mesh, z0: f32, z1: f32, n: usize) {
    let len = (z1 - z0) / n as f32;
    for side in [-1.0f32, 1.0] {
        for i in 0..n {
            let z = z0 + len * (i as f32 + 0.5);
            let shade = if i % 2 == 0 { 0.94 } else { 1.05 };
            plate(
                m,
                Vec3::new(side * (W / 2.0 + 0.14), -0.05, z),
                Vec3::new(0.28, 2.5, len - 0.12),
                Vec3::X * side,
                Paint::Olive,
                shade,
            );
        }
    }
}

/// Riveted armour plates along the top of the spine, in `n` pieces.
fn deck_plates(m: &mut Mesh, z0: f32, z1: f32, n: usize) {
    let len = (z1 - z0) / n as f32;
    for i in 0..n {
        let z = z0 + len * (i as f32 + 0.5);
        plate(
            m,
            Vec3::new(0.0, H / 2.0 + 0.1, z),
            Vec3::new(W - 2.0 * C - 0.1, 0.2, len - 0.15),
            Vec3::Y,
            Paint::Gunmetal,
            if i % 2 == 0 { 1.08 } else { 0.95 },
        );
    }
}

/// A raised frame hoop round the spine at `z`.
fn hoop(m: &mut Mesh, z: f32) {
    let band = [
        (z - 0.3, chamfer_rect(W + 0.5, H + 0.4, C + 0.15, 0.0)),
        (z + 0.3, chamfer_rect(W + 0.5, H + 0.4, C + 0.15, 0.0)),
    ];
    m.add(&loft(&band, |_, _| Paint::Gunmetal, Paint::Metal), ID, 0.82);
}

/// The kit scale for a module on this hull.
fn k(module: &Module) -> f32 {
    scale(module.size) * GK
}

/// Red caps on a gun's muzzles.
fn muzzles(m: &mut Mesh, x: f32, y: f32, z: f32, laser: bool, k: f32) {
    let cyl = cylinder(10);
    let xs: &[f32] = if laser { &[0.0] } else { &[-0.09, 0.09] };
    for dx in xs {
        let p = Vec3::new(x + dx * k, y, z);
        m.paint(
            &cyl,
            strut(p + Vec3::Z * 0.14 * k, p, 0.085 * k),
            1.0,
            Paint::Red,
        );
    }
}

/// A turret on its ring at `base`, standing up `up` (±1): a faceted,
/// sloped-front armoured box carrying the gun, built upright and turned
/// over for the belly.
fn turret(m: &mut Mesh, base: Vec3, up: f32, module: &Module) {
    let k = k(module);
    let s = 0.55 * k;
    let laser = is_laser(module);
    let mut t = Mesh::default();
    let body = [
        (-1.3 * s, chamfer_rect(1.5 * s, 0.5 * s, 0.15 * s, 0.45 * s)),
        (-0.8 * s, chamfer_rect(2.1 * s, 0.95 * s, 0.3 * s, 0.55 * s)),
        (0.9 * s, chamfer_rect(2.2 * s, 1.0 * s, 0.3 * s, 0.55 * s)),
        (1.4 * s, chamfer_rect(1.8 * s, 0.8 * s, 0.25 * s, 0.5 * s)),
    ];
    let paint = |_, e: usize| {
        if e == 2 || e == 6 {
            Paint::Olive
        } else {
            Paint::Gunmetal
        }
    };
    t.add(&loft(&body, paint, Paint::Gunmetal), ID, 1.05);
    // A cupola for the gunner, a red stripe round the back.
    t.paint(
        &cylinder(10),
        strut(
            Vec3::new(0.45 * s, 0.9 * s, 0.4 * s),
            Vec3::new(0.45 * s, 1.25 * s, 0.4 * s),
            0.26 * s,
        ),
        1.0,
        Paint::Gunmetal,
    );
    t.paint(
        &cube(),
        boxed(
            Vec3::new(0.45 * s, 1.1 * s, 0.13 * s),
            Vec3::new(0.3 * s, 0.07 * s, 0.03 * s),
        ),
        1.0,
        Paint::Glass,
    );
    t.paint(
        &cube(),
        boxed(
            Vec3::new(0.0, 0.55 * s, 1.18 * s),
            Vec3::new(2.05 * s, 0.22 * s, 0.4 * s),
        ),
        1.0,
        Paint::Red,
    );
    // The mantlet and gun.
    let breech = Vec3::new(0.0, 0.5 * s, -1.2 * s);
    t.paint(
        &cube(),
        boxed(breech, Vec3::new(0.9 * s, 0.45 * s, 0.3 * s)),
        0.9,
        Paint::Metal,
    );
    let muzzle = breech.z - 2.6 * k;
    gun(&mut t, breech, muzzle, laser, k);
    if !laser {
        muzzles(&mut t, 0.0, breech.y, muzzle, laser, k);
    }
    let turn = if up < 0.0 {
        Mat4::from_rotation_z(PI)
    } else {
        ID
    };
    m.add(&t, Mat4::from_translation(base) * turn, 1.0);
}

/// A turret ring and its barbette: always there, turret or not.
fn ring(m: &mut Mesh, base: Vec3, up: f32, r: f32) {
    m.paint(&torus(0.08, 28, 6), at(base, r), 1.0, Paint::Metal);
    m.paint(
        &cylinder(20),
        strut(base - Vec3::Y * up * 0.05, base, r * 0.93),
        0.7,
        Paint::Metal,
    );
}

/// Attitude thrusters: a block on each flank with three nozzles.
fn rcs(m: &mut Mesh, z: f32, y: f32, x: f32) {
    for side in [-1.0f32, 1.0] {
        let c = Vec3::new(side * x, y, z);
        bx(m, c, Vec3::new(0.5, 0.6, 0.8), Paint::Gunmetal);
        let cyl = cylinder(8);
        for (d, o) in [
            (Vec3::X * side, Vec3::X * side * 0.25),
            (Vec3::Y, Vec3::Y * 0.3),
            (Vec3::NEG_Y, Vec3::NEG_Y * 0.3),
        ] {
            m.paint(
                &cyl,
                strut(c + o, c + o + d * 0.22, 0.12),
                1.0,
                Paint::Metal,
            );
        }
    }
}

/// Her number on both flanks at `z`, reading nose-ward.
fn number(m: &mut Mesh, x: f32, y: f32, z: f32, h: f32) {
    let len = NUMBER.len() as f32 * 0.85 * h;
    for side in [-1.0f32, 1.0] {
        let origin = Vec3::new(
            side * x,
            y,
            z + if side > 0.0 { len / 2.0 } else { -len / 2.0 },
        );
        let surface = flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
        stencil(m, NUMBER, h, Paint::Red, &surface);
    }
}

/// Narrow vision slits across a sloped armoured face from `a` (bottom
/// centre) to `b` (top centre), `t` of the way up, `w` wide in `n` panes.
fn slits(m: &mut Mesh, a: Vec3, b: Vec3, t: f32, w: f32, n: usize) {
    let d = (b - a).normalize();
    let tilt = Quat::from_rotation_x(d.z.atan2(d.y));
    let normal = Vec3::new(0.0, -d.z, d.y);
    let normal = if normal.z > 0.0 { -normal } else { normal };
    let c = a.lerp(b, t) + normal * 0.04;
    let pane = w / n as f32;
    for i in 0..n {
        let x = -w / 2.0 + pane * (i as f32 + 0.5);
        m.paint(
            &cube(),
            Mat4::from_scale_rotation_translation(
                Vec3::new(pane - 0.12, 0.14, 0.06),
                tilt,
                c + Vec3::X * x,
            ),
            1.0,
            Paint::Glass,
        );
    }
}

// --- Bows -----------------------------------------------------------------

/// An armoured bridge on a blunt prow: a tall riveted block with a steep
/// glacis, vision slits under a heavy brow, a chin gun in a ball mount.
fn command_bow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let body = [
        (f, chamfer_rect(2.6, 1.3, 0.4, -0.7)),
        (f + 0.5, chamfer_rect(3.2, 2.0, 0.55, -0.55)),
        (f + 4.0, chamfer_rect(4.6, 3.4, 0.8, -0.2)),
        (f + 7.0, chamfer_rect(W, H, C, 0.0)),
        spine(b),
    ];
    m.add(&loft(&body, livery, Paint::Gunmetal), ID, 1.0);
    // A ram plate and red chevron on the prow, searchlights either side.
    bx(
        m,
        Vec3::new(0.0, -0.7, f - 0.08),
        Vec3::new(2.2, 0.9, 0.16),
        Paint::Metal,
    );
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cube(),
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.9, 0.3, 0.06),
                Quat::from_rotation_y(side * 0.33),
                Vec3::new(side * 0.9, 0.0, f + 1.6 + 0.0),
            ) * Mat4::from_translation(Vec3::ZERO),
            1.0,
            Paint::Red,
        );
        m.paint(
            &cylinder(12),
            strut(
                Vec3::new(side * 1.05, -0.35, f + 0.3),
                Vec3::new(side * 1.05, -0.35, f + 0.18),
                0.22,
            ),
            1.0,
            Paint::Lamp,
        );
    }

    // The bridge: a riveted armoured block with a steep glacis.
    let bridge = [
        (f + 4.3, chamfer_rect(2.6, 0.6, 0.2, 1.6)),
        (f + 5.4, chamfer_rect(3.6, 2.6, 0.45, 2.7)),
        (b - 1.6, chamfer_rect(3.6, 2.6, 0.45, 2.7)),
        (b - 0.5, chamfer_rect(3.0, 1.8, 0.4, 2.4)),
    ];
    m.add(&loft(&bridge, livery, Paint::Gunmetal), ID, 1.04);
    slits(
        m,
        Vec3::new(0.0, 1.9, f + 4.3),
        Vec3::new(0.0, 3.55, f + 5.4),
        0.62,
        2.6,
        4,
    );
    // The brow over the slits.
    bx(
        m,
        Vec3::new(0.0, 3.95, f + 5.3),
        Vec3::new(3.75, 0.22, 0.9),
        Paint::Gunmetal,
    );
    for side in [-1.0f32, 1.0] {
        for i in 0..3 {
            bx(
                m,
                Vec3::new(side * 1.82, 3.3, f + 6.0 + i as f32 * 1.0),
                Vec3::new(0.05, 0.13, 0.7),
                Paint::Glass,
            );
        }
        rivets(
            m,
            Vec3::new(side * 1.83, 2.75, f + 5.6),
            Vec3::new(side * 1.83, 2.75, b - 1.8),
            8,
        );
        // Armour skirts down the cheeks.
        plate(
            m,
            Vec3::new(side * (W / 2.0 + 0.14), -0.05, (f + 7.1 + b) / 2.0),
            Vec3::new(0.28, 2.5, b - f - 7.3),
            Vec3::X * side,
            Paint::Olive,
            1.0,
        );
    }
    number(m, W / 2.0 + 0.3, -0.75, f + 9.0, 0.95);
    // A beacon on the bridge roof, aerials.
    m.paint(
        &icosphere(1),
        at(Vec3::new(0.0, 4.1, b - 0.9), 0.14),
        1.0,
        Paint::Beacon,
    );
    m.paint(
        &cylinder(6),
        strut(
            Vec3::new(1.2, 4.0, b - 1.2),
            Vec3::new(1.2, 5.6, b - 1.6),
            0.03,
        ),
        1.0,
        Paint::Metal,
    );

    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Weapon => {
                // A chin gun in a ball mount.
                let ball = Vec3::new(0.0, -1.55, f + 1.4);
                m.paint(&icosphere(2), at(ball, 0.55), 1.0, Paint::Gunmetal);
                let tip = f - 2.2;
                gun(m, ball, tip, is_laser(module), kk * 0.8);
                if !is_laser(module) {
                    muzzles(m, 0.0, ball.y, tip, false, kk * 0.8);
                }
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, 4.0, c - 0.2), kk),
            SlotKind::Radar => radar(m, Vec3::new(0.0, 4.0, b - 2.6), kk * 0.9),
            SlotKind::Rcs => rcs(m, pos.z, 0.6, 2.25),
            _ => {}
        }
    }
}

/// Twin casemates: a broad armoured beak with a gun casemate in each
/// cheek, a low slit cupola behind.
fn gun_bow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let body = [
        (f + 0.6, chamfer_rect(3.4, 1.4, 0.45, -0.35)),
        (f + 1.0, chamfer_rect(4.2, 2.2, 0.65, -0.25)),
        (f + 4.5, chamfer_rect(6.4, 3.8, 0.9, -0.1)),
        (b - 2.5, chamfer_rect(6.4, 3.8, 0.9, -0.1)),
        spine(b),
    ];
    m.add(&loft(&body, livery, Paint::Gunmetal), ID, 1.0);
    // A red chevron on the beak's deck.
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cube(),
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.7, 0.06, 0.32),
                Quat::from_rotation_y(-side * 0.5),
                Vec3::new(side * 0.72, 1.2, f + 2.6),
            ),
            1.0,
            Paint::Red,
        );
    }

    // The casemates: armoured boxes in the lower cheeks.
    for side in [-1.0f32, 1.0] {
        let x = side * 3.1;
        let y = -1.0;
        let cm = [
            (f, chamfer_rect(1.9, 1.7, 0.45, y)),
            (f + 0.6, chamfer_rect(2.4, 2.2, 0.55, y)),
            (f + 6.5, chamfer_rect(2.4, 2.2, 0.55, y)),
            (f + 7.8, chamfer_rect(1.4, 1.4, 0.4, y)),
        ];
        m.add(
            &loft(&cm, livery, Paint::Gunmetal),
            Mat4::from_translation(Vec3::X * x),
            0.96,
        );
        plate(
            m,
            Vec3::new(x + side * 1.26, y, f + 3.6),
            Vec3::new(0.16, 1.7, 5.4),
            Vec3::X * side,
            Paint::Olive,
            1.05,
        );
        // Lamps under the casemates.
        m.paint(
            &cube(),
            boxed(Vec3::new(x, y - 1.13, f + 1.0), Vec3::new(0.6, 0.08, 0.3)),
            1.0,
            Paint::Lamp,
        );
    }

    // The cupola: low, slit-eyed.
    let cupola = [
        (f + 4.6, chamfer_rect(2.0, 0.6, 0.2, 1.6)),
        (f + 5.8, chamfer_rect(2.9, 1.7, 0.35, 2.05)),
        (b - 1.4, chamfer_rect(2.9, 1.7, 0.35, 2.05)),
        (b - 0.5, chamfer_rect(2.4, 1.2, 0.3, 1.9)),
    ];
    m.add(&loft(&cupola, livery, Paint::Gunmetal), ID, 1.04);
    slits(
        m,
        Vec3::new(0.0, 1.9, f + 4.6),
        Vec3::new(0.0, 2.9, f + 5.8),
        0.6,
        2.0,
        3,
    );
    for side in [-1.0f32, 1.0] {
        plate(
            m,
            Vec3::new(side * 3.33, -0.1, b - 1.3),
            Vec3::new(0.2, 2.6, 2.0),
            Vec3::X * side,
            Paint::Olive,
            0.95,
        );
    }
    number(m, 3.36, -0.7, b - 1.3, 0.8);

    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Weapon => {
                // A rounded mantlet in the casemate's face, the gun through it.
                let x = pos.x.signum() * 3.1;
                let mantlet = Vec3::new(x, -1.0, f);
                m.paint(
                    &icosphere(2),
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(0.85, 0.85, 0.5),
                        Quat::IDENTITY,
                        mantlet,
                    ),
                    1.0,
                    Paint::Gunmetal,
                );
                let tip = f - 3.4 * kk / 2.8;
                gun(m, mantlet, tip, is_laser(module), kk);
                if !is_laser(module) {
                    muzzles(m, x, mantlet.y, tip, false, kk);
                }
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, 2.9, c + 1.0), kk),
            SlotKind::Radar => radar(m, Vec3::new(0.0, 1.35, f + 2.6), kk),
            SlotKind::Rcs => rcs(m, b - 3.6, 1.0, 3.3),
            _ => {}
        }
    }
    // Empty casemates are shuttered.
    let armed: Vec<f32> = p
        .modules()
        .filter(|(s, _, _)| s.kind == SlotKind::Weapon)
        .map(|(_, _, pos)| pos.x.signum())
        .collect();
    for side in [-1.0f32, 1.0] {
        if !armed.contains(&side) {
            let x = side * 3.1;
            bx(
                m,
                Vec3::new(x, -1.0, f - 0.05),
                Vec3::new(1.5, 1.3, 0.12),
                Paint::Metal,
            );
            bx(
                m,
                Vec3::new(x, -1.0, f - 0.12),
                Vec3::new(1.3, 0.2, 0.06),
                Paint::Red,
            );
        }
    }
}

// --- Mid sections ---------------------------------------------------------

/// Turret rings on the spine and on two flank sponsons; turrets only where
/// guns are fitted. A radiator fin aft.
fn gun_deck(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    m.add(
        &loft(&[spine(f), spine(b)], livery, Paint::Gunmetal),
        ID,
        1.0,
    );
    hoop(m, f + 0.3);
    hoop(m, b - 0.3);
    // Sponsons for the flank turrets.
    let zs = c - 3.0;
    for side in [-1.0f32, 1.0] {
        let sponson = [
            (zs - 3.0, chamfer_rect(0.6, 0.6, 0.1, -0.55)),
            (zs - 1.8, chamfer_rect(2.0, 1.1, 0.25, -0.55)),
            (zs + 1.8, chamfer_rect(2.0, 1.1, 0.25, -0.55)),
            (zs + 3.0, chamfer_rect(0.6, 0.6, 0.1, -0.55)),
        ];
        m.add(
            &loft(&sponson, livery, Paint::Gunmetal),
            Mat4::from_translation(Vec3::X * side * 3.5),
            0.95,
        );
        ring(m, Vec3::new(side * 3.7, 0.0, zs), 1.0, 1.2);
        // Hazard edging along the sponson lip.
        bx(
            m,
            Vec3::new(side * 4.52, -0.1, zs),
            Vec3::new(0.06, 0.18, 3.4),
            Paint::Hazard,
        );
    }
    flank_slabs(m, zs + 2.2, b - 0.7, 2);
    // Barbettes top and bottom.
    for up in [-1.0f32, 1.0] {
        let y0 = up * H / 2.0;
        let y1 = up * (H / 2.0 + 0.7);
        m.paint(
            &cylinder(16),
            strut(
                Vec3::new(0.0, y0 - up * 0.2, c),
                Vec3::new(0.0, y1, c),
                1.55,
            ),
            1.0,
            Paint::Gunmetal,
        );
        ring(m, Vec3::new(0.0, y1, c), up, 1.35);
    }
    let mut radiator_fitted = false;
    for (slot, module, pos) in p.modules() {
        match slot.kind {
            SlotKind::Weapon if pos.x.abs() > 0.5 => {
                turret(m, Vec3::new(pos.x.signum() * 3.7, 0.0, zs), 1.0, module);
            }
            SlotKind::Weapon => {
                let up = pos.y.signum();
                turret(m, Vec3::new(0.0, up * (H / 2.0 + 0.7), c), up, module);
            }
            SlotKind::Radiator => {
                radiator_fitted = true;
                let kk = k(module);
                radiator(
                    m,
                    Vec3::new(0.0, H / 2.0, b - 3.2),
                    Vec3::Y,
                    1.0 * kk,
                    1.3 * kk,
                );
            }
            _ => {}
        }
    }
    if !radiator_fitted {
        bx(
            m,
            Vec3::new(0.0, H / 2.0 + 0.1, b - 3.2),
            Vec3::new(0.4, 0.25, 3.0),
            Paint::Metal,
        );
    }
    deck_plates(m, f + 0.8, c - 1.8, 2);
    deck_plates(m, c + 1.8, b - 0.8, 2);
}

/// A big emitter ring round the spine, like a power-station transformer:
/// a heavy collar studded with red ribbed insulators, the emitters on
/// tall bushings top and bottom.
fn shield_ring(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    m.add(
        &loft(&[spine(f), spine(b)], livery, Paint::Gunmetal),
        ID,
        1.0,
    );
    hoop(m, f + 0.3);
    hoop(m, b - 0.3);
    deck_plates(m, f + 0.7, c - 1.8, 1);
    deck_plates(m, c + 1.8, b - 0.7, 1);
    let r = 5.4;
    let collar = [
        (c - 1.7, 2.4),
        (c - 1.7, 3.2),
        (c - 1.7, 3.2),
        (c - 1.25, r),
        (c - 1.25, r),
        (c + 1.25, r),
        (c + 1.25, r),
        (c + 1.7, 3.2),
        (c + 1.7, 3.2),
        (c + 1.7, 2.4),
    ];
    m.paint(&lathe(&collar, 32, 0.0, TAU), ID, 1.0, Paint::Gunmetal);
    // Bolted ring flanges on its faces, a band of dark metal round the rim.
    for z in [c - 1.72, c + 1.72] {
        m.paint(
            &torus(0.05, 32, 4),
            Mat4::from_translation(Vec3::new(0.0, 0.0, z))
                * Mat4::from_rotation_x(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(3.3)),
            1.0,
            Paint::Metal,
        );
    }
    let rim = [(c - 0.5, r + 0.04), (c + 0.5, r + 0.04)];
    m.paint(&lathe(&rim, 32, 0.0, TAU), ID, 0.8, Paint::Metal);
    // Insulators all round, but where the emitters stand.
    let cyl = cylinder(14);
    let insulator = |m: &mut Mesh, a: f32, discs: usize, z: f32| {
        let d = Vec3::new(a.cos(), a.sin(), 0.0);
        let root = d * (r - 0.1) + Vec3::Z * z;
        let len = 0.3 * discs as f32 + 0.3;
        m.paint(&cyl, strut(root, root + d * len, 0.13), 1.0, Paint::Metal);
        for i in 0..discs {
            let q = root + d * (0.25 + 0.3 * i as f32);
            m.paint(&cyl, strut(q, q + d * 0.11, 0.42), 1.0, Paint::Red);
        }
        m.paint(&icosphere(1), at(root + d * len, 0.2), 1.0, Paint::Metal);
    };
    for i in 0..12 {
        let a = i as f32 / 12.0 * TAU;
        if (a - FRAC_PI_2).abs() < 0.1 || (a - 3.0 * FRAC_PI_2).abs() < 0.1 {
            continue;
        }
        for z in [c - 0.65, c + 0.65] {
            insulator(m, a, 4, z);
        }
    }
    // Bus bars from the collar aft along the spine's shoulders.
    for side in [-1.0f32, 1.0] {
        for y in [-1.3f32, 1.3] {
            m.paint(
                &cyl,
                strut(
                    Vec3::new(side * 2.55, y, c + 1.7),
                    Vec3::new(side * 2.55, y, b - 0.6),
                    0.16,
                ),
                1.0,
                Paint::Metal,
            );
        }
    }
    let mut emitters = [false, false];
    let mut radiator_fitted = false;
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Shield => {
                let up = pos.y.signum();
                emitters[(up > 0.0) as usize] = true;
                insulator(m, up * FRAC_PI_2, 7, c);
                shield(m, Vec3::new(0.0, up * (r + 2.6), c), kk * 1.2);
            }
            SlotKind::Radiator => {
                radiator_fitted = true;
                radiator(
                    m,
                    Vec3::new(0.0, H / 2.0, b - 2.0),
                    Vec3::Y,
                    0.9 * kk,
                    1.1 * kk,
                );
            }
            _ => {}
        }
    }
    for (i, up) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !emitters[i] {
            // A blanked-off flange where an emitter would stand.
            m.paint(
                &cyl,
                strut(
                    Vec3::new(0.0, up * (r - 0.1), c),
                    Vec3::new(0.0, up * (r + 0.35), c),
                    0.7,
                ),
                1.0,
                Paint::Metal,
            );
        }
    }
    if !radiator_fitted {
        bx(
            m,
            Vec3::new(0.0, H / 2.0 + 0.1, b - 2.0),
            Vec3::new(0.4, 0.25, 2.2),
            Paint::Metal,
        );
    }
}

/// An armoured container hangar: slab walls, clamshell doors standing
/// open, the containers in their cradles inside.
fn cargo_bay(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let (hw, floor, top) = (3.8f32, -2.8f32, 0.5f32);
    let box_rect = || chamfer_rect(2.0 * hw, top - floor, 0.7, (top + floor) / 2.0);
    let front = [spine(f), (f + 0.8, box_rect()), (f + 1.3, box_rect())];
    let back = [(b - 1.3, box_rect()), (b - 0.8, box_rect()), spine(b)];
    for end in [&front[..], &back[..]] {
        m.add(&loft(end, livery, Paint::Metal), ID, 1.0);
    }
    let (z0, z1) = (f + 1.3, b - 1.3);
    let len = z1 - z0;
    // Floor and walls.
    bx(
        m,
        Vec3::new(0.0, floor + 0.25, c),
        Vec3::new(2.0 * hw - 0.4, 0.5, len),
        Paint::Metal,
    );
    m.paint(
        &cube(),
        boxed(
            Vec3::new(0.0, floor + 0.1, c),
            Vec3::new(2.0 * hw - 1.0, 0.2, len + 0.02),
        ),
        1.0,
        Paint::Gunmetal,
    );
    for side in [-1.0f32, 1.0] {
        plate(
            m,
            Vec3::new(side * (hw - 0.2), (top + floor) / 2.0, c),
            Vec3::new(0.4, top - floor - 0.3, len),
            Vec3::X * side,
            Paint::Olive,
            1.0,
        );
        // Stiffening ribs on the walls outside.
        for i in 0..=4 {
            let z = z0 + len * i as f32 / 4.0;
            bx(
                m,
                Vec3::new(side * hw, (top + floor) / 2.0, z),
                Vec3::new(0.2, top - floor - 0.2, 0.3),
                Paint::Gunmetal,
            );
        }
        // The clamshell door, swung up and out.
        let hinge = Vec3::new(side * hw, top, 0.0);
        let d = Vec2::new(side * 0.8, 0.6);
        let frame = Mat4::from_translation(hinge) * Mat4::from_rotation_z(d.y.atan2(d.x));
        let dw = hw;
        m.paint(
            &cube(),
            frame * boxed(Vec3::new(dw / 2.0, 0.0, c), Vec3::new(dw, 0.25, len - 0.2)),
            1.0,
            Paint::Olive,
        );
        let out = -side * 0.18;
        for i in 0..=3 {
            let z = z0 + 0.3 + (len - 0.6) * i as f32 / 3.0;
            m.paint(
                &cube(),
                frame * boxed(Vec3::new(dw / 2.0, out, z), Vec3::new(dw, 0.14, 0.25)),
                1.0,
                Paint::Gunmetal,
            );
        }
        m.paint(
            &cube(),
            frame
                * boxed(
                    Vec3::new(dw - 0.15, out, c),
                    Vec3::new(0.3, 0.12, len - 0.2),
                ),
            1.0,
            Paint::Red,
        );
        m.paint(
            &cylinder(10),
            strut(
                Vec3::new(side * hw, top, z0),
                Vec3::new(side * hw, top, z1),
                0.2,
            ),
            1.0,
            Paint::Metal,
        );
        // Work lamps on the wall tops, looking in.
        for z in [z0 + 0.6, z1 - 0.6] {
            bx(
                m,
                Vec3::new(side * (hw - 0.45), top + 0.1, z),
                Vec3::new(0.35, 0.18, 0.25),
                Paint::Lamp,
            );
        }
    }
    // A gantry across the hold.
    for z in [z0 + 0.2, z1 - 0.2] {
        bx(
            m,
            Vec3::new(0.0, top + 0.05, z),
            Vec3::new(2.0 * hw, 0.3, 0.3),
            Paint::Hazard,
        );
    }
    number(m, hw + 0.15, -1.6, b - 2.6, 0.9);
    // Cradles, and the containers in them.
    let paints = [Paint::Olive, Paint::Ochre, Paint::Red, Paint::Hull];
    let pitch = 5.0;
    for i in -1..=1 {
        let z = c + i as f32 * pitch;
        for x in [-1.6f32, 1.6] {
            bx(
                m,
                Vec3::new(x, floor + 0.65, z),
                Vec3::new(0.35, 0.3, 4.4),
                Paint::Metal,
            );
        }
        for dz in [-1.8f32, 1.8] {
            bx(
                m,
                Vec3::new(0.0, floor + 0.6, z + dz),
                Vec3::new(5.0, 0.22, 0.3),
                Paint::Gunmetal,
            );
        }
    }
    for (n, (slot, _, pos)) in p.modules().enumerate() {
        if slot.kind == SlotKind::Cargo {
            container(
                m,
                Vec3::new(0.0, floor + 0.8 + 1.7, pos.z),
                Vec3::new(5.2, 3.4, 4.3),
                paints[n % paints.len()],
            );
        }
    }
}

/// A bulbous armoured containment vessel: one swollen sphere under
/// riveted gores, belted with a hazard band, a crown of radiator fins on
/// its crest and the slot radiators spread either side.
fn reactor_section(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let collar = |z0: f32, z1: f32| [spine(z0), spine(z1)];
    m.add(&loft(&collar(f, f + 0.9), livery, Paint::Metal), ID, 1.0);
    m.add(&loft(&collar(b - 0.9, b), livery, Paint::Metal), ID, 1.0);
    hoop(m, f + 0.3);
    hoop(m, b - 0.3);
    let half = p.len() / 2.0 - 0.6;
    let radius = |z: f32| -> f32 {
        let t = ((z - c) / half).clamp(-1.0, 1.0);
        1.6 + 3.3 * (1.0 - t * t).max(0.0).powf(0.6)
    };
    let profile = |z0: f32, z1: f32, inflate: f32| -> Vec<(f32, f32)> {
        (0..=24)
            .map(|i| {
                let z = z0 + (z1 - z0) * i as f32 / 24.0;
                (z, radius(z) + inflate)
            })
            .collect()
    };
    m.paint(
        &lathe(&profile(c - half, c + half, 0.0), 32, 0.0, TAU),
        ID,
        1.0,
        Paint::Metal,
    );
    // Armour gores fore and aft of the belt, gaps between them.
    let gores = 12;
    for (z0, z1) in [(c - half + 0.4, c - 0.7), (c + 0.7, c + half - 0.4)] {
        for i in 0..gores {
            let a = i as f32 / gores as f32 * TAU;
            m.paint(
                &lathe(
                    &profile(z0, z1, 0.08),
                    4,
                    a + 0.025,
                    a + TAU / gores as f32 - 0.025,
                ),
                ID,
                if i % 2 == 0 { 1.0 } else { 0.9 },
                Paint::Gunmetal,
            );
        }
    }
    // The hazard belt round the equator, dark rims either side.
    m.paint(
        &lathe(&profile(c - 0.55, c + 0.55, 0.14), 40, 0.0, TAU),
        ID,
        1.0,
        Paint::Hazard,
    );
    for z in [c - 0.62, c + 0.62] {
        m.paint(
            &lathe(&profile(z - 0.08, z + 0.08, 0.2), 40, 0.0, TAU),
            ID,
            1.0,
            Paint::Metal,
        );
    }
    // Pipework along the underside.
    for side in [-1.0f32, 1.0] {
        let a = -FRAC_PI_2 + side * 0.7;
        let d = Vec3::new(a.cos(), a.sin(), 0.0);
        let pts: Vec<Vec3> = (0..=10)
            .map(|i| {
                let z = f + 0.7 + (b - f - 1.4) * i as f32 / 10.0;
                d * (radius(z).max(2.4) + 0.35) + Vec3::Z * z
            })
            .collect();
        for w in pts.windows(2) {
            m.paint(&cylinder(8), strut(w[0], w[1], 0.15), 1.0, Paint::Metal);
        }
    }
    // The crown: a ring of radiator fins round the crest.
    let crest = Vec3::new(0.0, radius(c) - 0.3, c);
    m.paint(
        &cylinder(20),
        strut(crest, crest + Vec3::Y * 0.6, 1.9),
        1.0,
        Paint::Gunmetal,
    );
    m.paint(
        &torus(0.08, 28, 6),
        at(crest + Vec3::Y * 0.6, 1.9),
        1.0,
        Paint::Red,
    );
    let fins = 10;
    for i in 0..fins {
        let a = (i as f32 + 0.5) / fins as f32 * TAU;
        let rot = Quat::from_rotation_y(-a);
        let d = rot * Vec3::X;
        let base = crest + Vec3::Y * 0.6 + d * 1.35;
        let piece = |m: &mut Mesh, off: Vec3, size: Vec3, paint: Paint| {
            m.paint(
                &cube(),
                Mat4::from_scale_rotation_translation(size, rot, base + rot * off),
                1.0,
                paint,
            );
        };
        piece(
            m,
            Vec3::new(0.3, 0.9, 0.0),
            Vec3::new(1.5, 1.8, 0.07),
            Paint::Hull,
        );
        for j in 0..4 {
            piece(
                m,
                Vec3::new(-0.3 + j as f32 * 0.4, 0.9, 0.0),
                Vec3::new(0.05, 1.7, 0.11),
                Paint::Metal,
            );
        }
        piece(
            m,
            Vec3::new(0.3, 1.82, 0.0),
            Vec3::new(1.5, 0.1, 0.12),
            Paint::Red,
        );
    }
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Radiator => {
                let side = pos.x.signum();
                radiator(
                    m,
                    Vec3::new(side * 4.3, -0.4, c),
                    Vec3::new(side, -0.2, 0.0),
                    1.7 * kk,
                    1.6 * kk,
                );
            }
            SlotKind::Power => {
                // Lit sight-glasses on a live chamber.
                for a in [0.35f32, PI - 0.35] {
                    let d = Vec3::new(a.cos(), a.sin(), 0.0);
                    let z = c + (pos.z - c).signum() * 2.6;
                    let q = d * (radius(z) + 0.06) + Vec3::Z * z;
                    m.paint(&cylinder(10), strut(q, q + d * 0.1, 0.16), 1.0, Paint::Lamp);
                    m.paint(
                        &torus(0.3, 12, 4),
                        Mat4::from_translation(q + d * 0.08)
                            * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, d))
                            * Mat4::from_scale(Vec3::splat(0.26)),
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            _ => {}
        }
    }
}

// --- Sterns ---------------------------------------------------------------

/// Two big bells in an armoured engine block, shrouded by skirts, a
/// radiator above and below.
fn sprint_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let block = [
        spine(f),
        (f + 3.0, chamfer_rect(7.4, 4.4, 1.1, 0.0)),
        (b - 2.0, chamfer_rect(7.4, 4.4, 1.1, 0.0)),
        (b - 1.6, chamfer_rect(7.0, 4.0, 1.0, 0.0)),
        (b - 1.2, chamfer_rect(7.0, 4.0, 1.0, 0.0)),
    ];
    let banded = |s: usize, e: usize| if s == 3 { Paint::Red } else { livery(s, e) };
    m.add(&loft(&block, banded, Paint::Metal), ID, 1.0);
    hoop(m, f + 0.3);
    for side in [-1.0f32, 1.0] {
        plate(
            m,
            Vec3::new(side * 3.85, 0.0, (f + 3.4 + b + 0.8) / 2.0),
            Vec3::new(0.3, 3.2, b + 0.8 - f - 3.4),
            Vec3::X * side,
            Paint::Olive,
            1.0,
        );
        bx(
            m,
            Vec3::new(side * 3.85, 0.0, b + 0.65),
            Vec3::new(0.34, 3.2, 0.3),
            Paint::Hazard,
        );
        // Vents along the block's shoulders.
        for i in 0..5 {
            bx(
                m,
                Vec3::new(side * 2.4, 2.22, f + 4.0 + i as f32 * 0.55),
                Vec3::new(1.4, 0.06, 0.22),
                Paint::Metal,
            );
        }
        m.paint(
            &icosphere(1),
            at(Vec3::new(side * 3.85, 1.75, b + 0.85), 0.15),
            1.0,
            Paint::Beacon,
        );
    }
    let mut radiators = [false, false];
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(pos.x, 0.0, b - 1.2);
                m.paint(
                    &cylinder(18),
                    strut(throat - Vec3::Z * 0.1, throat + Vec3::Z * 0.4, 1.2),
                    1.0,
                    Paint::Metal,
                );
                nozzles.push(bell(m, throat + Vec3::Z * 0.3, 2.6 * kk / 2.8, 1.0));
            }
            SlotKind::Radiator => {
                let up = pos.y.signum();
                radiators[(up > 0.0) as usize] = true;
                radiator(
                    m,
                    Vec3::new(0.0, up * 2.15, c + 1.5),
                    Vec3::Y * up,
                    1.1 * kk,
                    1.7 * kk,
                );
            }
            _ => {}
        }
    }
    for (i, up) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !radiators[i] {
            bx(
                m,
                Vec3::new(0.0, up * 2.3, c + 1.5),
                Vec3::new(0.4, 0.25, 3.5),
                Paint::Metal,
            );
        }
    }
    soot(m, f + 2.0, b + 2.5, 0.35);
}

/// One big bell in a tapering block, flanked by a radiator array: stacks
/// of panels on truss frames either side.
fn cruise_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let block = [
        spine(f),
        (f + 2.0, chamfer_rect(5.6, 4.6, 1.2, 0.0)),
        (b - 3.4, chamfer_rect(5.6, 4.6, 1.2, 0.0)),
        (b - 1.6, chamfer_rect(3.6, 3.4, 1.0, 0.0)),
        (b - 1.3, chamfer_rect(3.6, 3.4, 1.0, 0.0)),
    ];
    let banded = |s: usize, e: usize| if s == 3 { Paint::Red } else { livery(s, e) };
    m.add(&loft(&block, banded, Paint::Metal), ID, 1.0);
    hoop(m, f + 0.3);
    flank_slabs(m, f + 2.2, b - 3.6, 2);
    let za = c + 2.0;
    let (zf, zb) = (za - 3.0, za + 3.0);
    let cyl = cylinder(8);
    let mut fitted = [false, false];
    for (slot, module, pos) in p.modules() {
        let kk = k(module);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(0.0, 0.0, b - 1.3);
                nozzles.push(bell(m, throat, 3.2 * kk / 2.8, 1.35));
            }
            SlotKind::Radiator => {
                let side = pos.x.signum();
                fitted[(side > 0.0) as usize] = true;
                let reach = 1.6 * kk;
                // A truss frame, three panels racked in it.
                for y in [-1.8f32, 0.0, 1.8] {
                    radiator(m, Vec3::new(side * 2.85, y, za), Vec3::X * side, reach, 5.6);
                }
                let xo = side * (2.85 + reach + 0.1);
                for z in [zf, zb] {
                    m.paint(
                        &cyl,
                        strut(Vec3::new(xo, -2.2, z), Vec3::new(xo, 2.2, z), 0.12),
                        1.0,
                        Paint::Metal,
                    );
                }
                m.paint(
                    &cyl,
                    strut(Vec3::new(xo, -2.2, zf), Vec3::new(xo, 2.2, zb), 0.08),
                    1.0,
                    Paint::Metal,
                );
                m.paint(
                    &icosphere(1),
                    at(Vec3::new(xo, 2.3, zb), 0.15),
                    1.0,
                    Paint::Beacon,
                );
            }
            _ => {}
        }
    }
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        if !fitted[i] {
            for y in [-1.8f32, 0.0, 1.8] {
                bx(
                    m,
                    Vec3::new(side * 3.0, y, za),
                    Vec3::new(0.4, 0.2, 5.0),
                    Paint::Metal,
                );
            }
        }
    }
    soot(m, f + 2.0, b + 3.0, 0.35);
}

/// A plain armoured length of spine, for a section with no rendition of
/// its own: its guns in turrets, its radiators as fins.
fn plain(m: &mut Mesh, p: &Placed) {
    let (f, b) = (p.front, p.back);
    m.add(
        &loft(&[spine(f), spine(b)], livery, Paint::Gunmetal),
        ID,
        1.0,
    );
    hoop(m, f + 0.3);
    hoop(m, b - 0.3);
    flank_slabs(m, f + 0.8, b - 0.8, 2);
    for (slot, module, pos) in p.modules() {
        match slot.kind {
            SlotKind::Weapon => turret(m, Vec3::new(0.0, H / 2.0, pos.z), 1.0, module),
            SlotKind::Radiator => radiator(m, Vec3::new(0.0, H / 2.0, pos.z), Vec3::Y, 2.0, 3.0),
            _ => {}
        }
    }
}

/// The gunship, built section by section from its fit.
pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut m = Mesh::default();
    let mut nozzles = Vec::new();
    for p in placed(fit) {
        match (p.section.id, p.section.position) {
            ("command-bow-m", _) => command_bow(&mut m, &p),
            ("gun-bow-m", _) => gun_bow(&mut m, &p),
            ("gun-deck-m", _) => gun_deck(&mut m, &p),
            ("shield-ring-m", _) => shield_ring(&mut m, &p),
            ("cargo-bay-m", _) => cargo_bay(&mut m, &p),
            ("reactor-section-m", _) => reactor_section(&mut m, &p),
            ("sprint-stern-m", _) => sprint_stern(&mut m, &p, &mut nozzles),
            ("cruise-stern-m", _) => cruise_stern(&mut m, &p, &mut nozzles),
            (_, Position::Bow) => command_bow(&mut m, &p),
            (_, Position::Stern) => cruise_stern(&mut m, &p, &mut nozzles),
            _ => plain(&mut m, &p),
        }
    }
    (m, ShipShape { nozzles })
}
