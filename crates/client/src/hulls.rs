//! Hand-built ships. Most hulls are generated from their fit
//! ([`meshes::ship`]); the ones here are modelled by hand, each with its
//! own silhouette, and draw their fitted modules — guns, cargo, radiators,
//! radar, shield emitters, engines — from a shared kit at the slots'
//! positions, so a refit shows.
//!
//! Model space is metres, nose toward -Z; a slot's `position_m` (+z
//! forward) lands at `(x, y, -z)`.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{Loadout, Module, Size, Slot, SlotKind};

use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, fin, icosphere, lathe, loft,
    strut, torus,
};

/// The hand-built model for `fit`'s hull, if it has one.
pub fn authored(fit: &Loadout) -> Option<(Mesh, ShipShape)> {
    match fit.hull().id {
        "courier" => Some(courier(fit.stats().length_m as f32)),
        "light-trader" => Some(light_trader(fit)),
        "interceptor" => Some(interceptor(fit)),
        _ => None,
    }
}

/// Every slot with its fitted module, and where it sits in model space.
fn fitted(fit: &Loadout) -> Vec<(&'static Slot, &'static Module, Vec3)> {
    fit.mounts()
        .filter_map(|(_, s)| s)
        .flat_map(|s| s.slots())
        .filter_map(|(slot, module)| {
            let [x, y, z] = slot.position_m.map(|v| v as f32);
            module.map(|m| (slot, m, Vec3::new(x, y, -z)))
        })
        .collect()
}

/// How much bigger a module of this size is drawn than a small one.
fn scale(size: Size) -> f32 {
    match size {
        Size::Small => 1.0,
        Size::Medium => 1.4,
        Size::Large => 2.0,
    }
}

fn at(p: Vec3, s: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::splat(s), Quat::IDENTITY, p)
}

// --- The module kit -------------------------------------------------------

/// A gun from `breech` forward to `muzzle_z`: twin barrels with muzzle
/// brakes for cannon, one finned barrel with a glowing lens for lasers.
fn gun(m: &mut Mesh, breech: Vec3, muzzle_z: f32, module: &Module) {
    let k = scale(module.size);
    let cyl = cylinder(10);
    let tip = |off: Vec3| Vec3::new(breech.x + off.x, breech.y + off.y, muzzle_z);
    if module.id.contains("laser") {
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
fn container(m: &mut Mesh, centre: Vec3, size: Vec3, paint: Paint) {
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
type Surface<'a> = &'a dyn Fn(f32, f32) -> (Vec3, Vec3, Vec3);

/// A flat surface from `origin` (the text's bottom left).
fn flat(origin: Vec3, right: Vec3, up: Vec3) -> impl Fn(f32, f32) -> (Vec3, Vec3, Vec3) {
    move |u, v| (origin + right * u + up * v, right, up)
}

/// The flank of a body of revolution about Z, on the `side` (±1) of x:
/// text reading nose-ward from `z0`, its foot at angle `a0` above
/// horizontal, wrapped round the curve. `radius(z)` is the surface.
fn flank(
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
fn stencil(m: &mut Mesh, text: &str, h: f32, paint: Paint, surface: Surface) {
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
fn soot(m: &mut Mesh, z0: f32, z1: f32, amount: f32) {
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
fn flapped(
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
fn radiator(m: &mut Mesh, root: Vec3, out: Vec3, height: f32, len: f32) {
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
fn radar(m: &mut Mesh, base: Vec3, module: &Module) {
    let k = scale(module.size);
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
fn shield(m: &mut Mesh, centre: Vec3, module: &Module) {
    let k = scale(module.size);
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
fn bell(m: &mut Mesh, throat: Vec3, module: &Module, r0: f32) -> (Vec3, f32) {
    let k = scale(module.size);
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

/// Where the courier's main engine bell ends, in model units (+Z is aft).
const COURIER_NOZZLE_Z: f32 = 12.2;
/// The courier model's length, nose probe to bell exit, in model units.
const COURIER_MODEL_LEN: f32 = 25.1;

/// Radius of the starter's fuselage at `z` (nose at -12, toward -Z).
fn starter_radius(z: f32) -> f32 {
    if z < -5.0 {
        // A blunt ogive nose.
        let t = ((z + 12.0) / 7.0).clamp(0.0, 1.0);
        0.3 + 1.9 * (1.0 - (1.0 - t).powi(2)).powf(0.75)
    } else {
        // A long, very slightly tapering cabin.
        let t = ((z + 5.0) / 9.0).clamp(0.0, 1.0);
        2.2 - 0.18 * t * t
    }
}

fn starter_profile(z0: f32, z1: f32, steps: u32, inflate: f32) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let z = z0 + (z1 - z0) * i as f32 / steps as f32;
            (z, starter_radius(z) + inflate)
        })
        .collect()
}

/// The courier, everyone's first ship, hand-built rather than generated:
/// a cream enamel rocket of the old optimistic school, nose toward -Z, with
/// a red nose and cheatline, a wraparound windscreen, lit portholes, three
/// swept radiator fins, a gold-foil service collar and a clustered engine.
/// Scaled so it is `length` metres from nose to bell exit.
fn courier(length: f32) -> (Mesh, ShipShape) {
    const SEGS: u32 = 36;
    let (cyl, ball) = (cylinder(16), icosphere(2));
    let mut m = Mesh::default();
    let id = Mat4::IDENTITY;

    // Fuselage: red nose cap, cream body.
    let mut nose = starter_profile(-12.0, -9.6, 8, 0.0);
    nose.insert(0, (-12.0, 0.0));
    m.paint(&lathe(&nose, SEGS, 0.0, TAU), id, 1.0, Paint::Red);
    m.add(
        &lathe(&starter_profile(-9.6, 4.0, 40, 0.0), SEGS, 0.0, TAU),
        id,
        1.0,
    );
    // Panel seams: thin dark rings.
    for z in [-9.6, -5.0, 2.6] {
        m.paint(
            &lathe(
                &starter_profile(z - 0.04, z + 0.04, 1, 0.012),
                SEGS,
                0.0,
                TAU,
            ),
            id,
            1.0,
            Paint::Metal,
        );
    }

    // Wraparound windscreen across the top of the nose: a row of panes
    // set into a dark frame.
    let up = TAU / 4.0;
    m.paint(
        &lathe(
            &starter_profile(-8.85, -7.75, 6, 0.015),
            SEGS,
            up - 0.98,
            up + 0.98,
        ),
        id,
        1.0,
        Paint::Metal,
    );
    for k in 0..5 {
        let a = up + (k as f32 - 2.0) * 0.38;
        m.paint(
            &lathe(
                &starter_profile(-8.75, -7.85, 6, 0.03),
                4,
                a - 0.16,
                a + 0.16,
            ),
            id,
            1.0,
            Paint::Glass,
        );
    }

    // Red cheatline down each flank, with a thin cream pinstripe below.
    for side in [0.0, TAU / 2.0] {
        m.paint(
            &lathe(
                &starter_profile(-9.6, 3.4, 30, 0.01),
                3,
                side - 0.11,
                side + 0.07,
            ),
            id,
            1.0,
            Paint::Red,
        );
        m.paint(
            &lathe(
                &starter_profile(-9.0, 3.4, 30, 0.01),
                2,
                side - 0.17,
                side - 0.14,
            ),
            id,
            1.0,
            Paint::Red,
        );
    }

    // Round portholes above the cheatline, glowing warm.
    for side in [-1.0f32, 1.0] {
        for k in 0..5 {
            let z = -3.6 + k as f32 * 1.35;
            let a: f32 = 0.38;
            let r = starter_radius(z);
            let n = Vec3::new(side * a.cos(), a.sin(), 0.0);
            let rot = Quat::from_rotation_arc(Vec3::Y, n);
            m.paint(
                &cyl,
                Mat4::from_translation(Vec3::new(0.0, 0.0, z))
                    * Mat4::from_scale_rotation_translation(
                        Vec3::new(0.36, 0.06, 0.36),
                        rot,
                        n * (r - 0.02),
                    ),
                1.0,
                Paint::Metal,
            );
            m.paint(
                &cyl,
                Mat4::from_translation(Vec3::new(0.0, 0.0, z))
                    * Mat4::from_scale_rotation_translation(
                        Vec3::new(0.27, 0.09, 0.27),
                        rot,
                        n * (r - 0.02),
                    ),
                1.0,
                Paint::Glass,
            );
        }
    }

    // Docking probe on the nose.
    m.paint(
        &cyl,
        strut(Vec3::new(0.0, 0.0, -11.9), Vec3::new(0.0, 0.0, -12.9), 0.12),
        1.0,
        Paint::Metal,
    );
    m.paint(
        &ball,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(0.2),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.0, -12.95),
        ),
        1.0,
        Paint::Metal,
    );

    // Service collar in gold thermal blanket, between two dark rings.
    let collar = [(3.6, 2.08), (3.8, 2.32), (6.4, 2.32), (6.6, 2.1)];
    m.paint(&lathe(&collar, SEGS, 0.0, TAU), id, 1.0, Paint::Foil);
    for z in [3.7f32, 6.5] {
        m.paint(
            &lathe(&[(z - 0.1, 2.36), (z + 0.1, 2.36)], SEGS, 0.0, TAU),
            id,
            1.0,
            Paint::Metal,
        );
    }
    // Thrust structure tapering to the engine, in dark metal.
    let tail = [
        (6.6, 2.1),
        (6.6, 2.1),
        (7.6, 1.9),
        (8.8, 1.25),
        (8.8, 1.25),
        (8.8, 0.0),
    ];
    m.paint(&lathe(&tail, SEGS, 0.0, TAU), id, 0.9, Paint::Metal);

    // Main bell, flaring, with a cream throat ring; four verniers around it.
    let bell: Vec<(f32, f32)> = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            (8.6 + (COURIER_NOZZLE_Z - 8.6) * t, 0.6 + 1.0 * t.powf(1.6))
        })
        .collect();
    m.paint(&lathe(&bell, SEGS, 0.0, TAU), id, 1.0, Paint::Metal);
    m.add(&lathe(&[(8.7, 0.78), (9.2, 0.78)], SEGS, 0.0, TAU), id, 0.9);
    let vernier: Vec<(f32, f32)> = bell
        .iter()
        .map(|&(z, r)| (z * 0.4 + 4.9, r * 0.32))
        .collect();
    let mut verniers = Vec::new();
    for k in 0..4 {
        let a = (k as f32 + 0.5) * TAU / 4.0;
        let at = Vec3::new(a.cos(), a.sin(), 0.0) * 1.45;
        verniers.push(at);
        m.paint(
            &lathe(&vernier, 16, 0.0, TAU),
            Mat4::from_translation(at),
            1.0,
            Paint::Metal,
        );
    }

    // Three swept radiator fins: dorsal, and two ventral at ±120°, red-tipped.
    for k in 0..3 {
        let a = k as f32 * TAU / 3.0;
        let rot = Mat4::from_rotation_z(a);
        let root_z = 0.4;
        let at = Mat4::from_translation(Vec3::new(0.0, starter_radius(root_z) - 0.25, 0.0));
        let (root, tip, span, thick) = ((root_z, 7.2), (6.0, 8.8), 3.4, 0.22);
        m.add(&fin(root, tip, span, thick, (0.0, 0.7)), rot * at, 1.0);
        m.paint(
            &fin(root, tip, span, thick, (0.7, 1.0)),
            rot * at,
            1.0,
            Paint::Red,
        );
        // A beacon on each tip.
        m.paint(
            &ball,
            rot * at
                * Mat4::from_scale_rotation_translation(
                    Vec3::splat(0.16),
                    Quat::IDENTITY,
                    Vec3::new(0.0, span + 0.05, tip.0 + 0.5),
                ),
            1.0,
            Paint::Beacon,
        );
    }

    // A whip antenna behind the windscreen.
    m.paint(
        &cyl,
        strut(Vec3::new(0.0, 2.1, -5.6), Vec3::new(0.0, 3.4, -4.4), 0.04),
        1.0,
        Paint::Metal,
    );
    m.paint(
        &ball,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(0.09),
            Quat::IDENTITY,
            Vec3::new(0.0, 3.4, -4.4),
        ),
        1.0,
        Paint::Red,
    );

    // Her number low on each flank, under the cheatline.
    for side in [-1.0f32, 1.0] {
        let z = if side > 0.0 { -0.6 } else { -3.0 };
        let surface = flank(side, z, -0.75, starter_radius);
        stencil(&mut m, "01", 1.1, Paint::Red, &surface);
    }
    soot(&mut m, 3.0, 12.0, 0.35);

    // Centre it and scale it to the hull's length.
    let k = length / COURIER_MODEL_LEN;
    let fit = Mat4::from_scale(Vec3::splat(k))
        * Mat4::from_translation(Vec3::Z * (COURIER_MODEL_LEN / 2.0 - COURIER_NOZZLE_Z));
    let mut out = Mesh::default();
    out.add(&m, fit, 1.0);
    let mut nozzles = vec![(fit.transform_point3(Vec3::Z * COURIER_NOZZLE_Z), 1.6 * k)];
    for &at in &verniers {
        nozzles.push((fit.transform_point3(at + Vec3::Z * 9.78), 0.5 * k));
    }
    (out, ShipShape { nozzles })
}

// --- Light trader ---------------------------------------------------------

/// The light trader: a working truck, not a rocket. A glazed cab in
/// mustard work enamel up front, an open box-girder cargo frame with its
/// containers hung in it, and a square engine block with hazard stripes,
/// radiators standing up off its shoulders. Empty cargo slots leave the
/// frame bare.
fn light_trader(fit: &Loadout) -> (Mesh, ShipShape) {
    let (cube, cyl) = (cube(), cylinder(12));
    let id = Mat4::IDENTITY;
    let mut m = Mesh::default();

    // The cab: a raked, wraparound windscreen, a recessed ring at the back.
    let cab = [
        (-8.3, chamfer_rect(2.3, 1.5, 0.45, 0.15)),
        (-7.5, chamfer_rect(3.0, 2.3, 0.55, 0.25)),
        (-4.8, chamfer_rect(3.2, 2.6, 0.6, 0.3)),
        (-4.4, chamfer_rect(3.0, 2.4, 0.55, 0.3)),
    ];
    let glazed = |s: usize, e: usize| match (s, e) {
        (0, 0 | 1 | 7) => Paint::Glass,
        (2, _) => Paint::Metal,
        _ => Paint::Ochre,
    };
    m.add(&loft(&cab, glazed, Paint::Ochre), id, 1.0);
    // Window mullions, a sun visor, side windows and a grab rail.
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cyl,
            strut(
                Vec3::new(side * 0.7, 0.9, -8.3),
                Vec3::new(side * 0.95, 1.4, -7.5),
                0.045,
            ),
            1.0,
            Paint::Metal,
        );
        for z in [-6.9f32, -5.75] {
            let x = 1.5 + 0.1 * (z + 7.5) / 2.7 + 0.02;
            m.paint(
                &cube,
                boxed(Vec3::new(side * x, 0.85, z), Vec3::new(0.04, 0.55, 0.95)),
                1.0,
                Paint::Glass,
            );
        }
        m.paint(
            &cyl,
            strut(
                Vec3::new(side * 1.68, 0.2, -7.2),
                Vec3::new(side * 1.7, 0.2, -5.0),
                0.03,
            ),
            1.0,
            Paint::Metal,
        );
        // Headlamps on the nose plate.
        m.paint(
            &cyl,
            strut(
                Vec3::new(side * 0.75, -0.3, -8.25),
                Vec3::new(side * 0.75, -0.3, -8.36),
                0.17,
            ),
            1.0,
            Paint::Lamp,
        );
        // Floodlights on the cab roof, looking back over the cargo.
        m.paint(
            &cube,
            boxed(
                Vec3::new(side * 1.0, 1.66, -5.0),
                Vec3::new(0.3, 0.16, 0.12),
            ),
            1.0,
            Paint::Lamp,
        );
    }
    m.paint(
        &cube,
        boxed(Vec3::new(0.0, 1.45, -7.6), Vec3::new(2.1, 0.06, 0.4)),
        0.9,
        Paint::Ochre,
    );
    m.paint(
        &cube,
        boxed(Vec3::new(0.0, 0.25, -8.32), Vec3::new(2.05, 0.22, 0.04)),
        1.0,
        Paint::Red,
    );

    // The spine carrying everything, and the cargo frame around it.
    let (fz0, fz1) = (-4.4f32, 4.2f32);
    let spine = [
        (fz0, chamfer_rect(1.0, 0.6, 0.15, 1.1)),
        (fz1, chamfer_rect(1.0, 0.6, 0.15, 1.1)),
    ];
    m.add(&loft(&spine, |_, _| Paint::Ochre, Paint::Ochre), id, 0.95);
    let (fx, top, bottom) = (1.95f32, 0.75f32, -1.65f32);
    let beam = |m: &mut Mesh, a: Vec3, b: Vec3, paint: Paint| {
        let (c, d) = ((a + b) / 2.0, (b - a).abs());
        m.paint(&cube, boxed(c, d.max(Vec3::splat(0.14))), 1.0, paint);
    };
    for side in [-1.0f32, 1.0] {
        let x = side * fx;
        beam(
            &mut m,
            Vec3::new(x, top, fz0),
            Vec3::new(x, top, fz1),
            Paint::Metal,
        );
        beam(
            &mut m,
            Vec3::new(x, bottom, fz0),
            Vec3::new(x, bottom, fz1),
            Paint::Hazard,
        );
    }
    let rings = [fz0 + 0.1, -1.0, 2.7, fz1 - 0.1];
    for (i, &z) in rings.iter().enumerate() {
        beam(
            &mut m,
            Vec3::new(-fx, top, z),
            Vec3::new(fx, top, z),
            Paint::Metal,
        );
        beam(
            &mut m,
            Vec3::new(-fx, bottom, z),
            Vec3::new(fx, bottom, z),
            Paint::Metal,
        );
        for side in [-1.0f32, 1.0] {
            let x = side * fx;
            beam(
                &mut m,
                Vec3::new(x, bottom, z),
                Vec3::new(x, top, z),
                Paint::Metal,
            );
            // Diagonal bracing down each side.
            if let Some(&next) = rings.get(i + 1) {
                m.paint(
                    &cyl,
                    strut(Vec3::new(x, bottom, z), Vec3::new(x, top, next), 0.05),
                    1.0,
                    Paint::Metal,
                );
            }
        }
    }
    // A keel between the containers, and a docking clamp on the belly.
    beam(
        &mut m,
        Vec3::new(0.0, -0.6, fz0),
        Vec3::new(0.0, -0.6, fz1),
        Paint::Metal,
    );
    m.paint(
        &cube,
        boxed(Vec3::new(0.0, -1.8, 0.8), Vec3::new(0.7, 0.18, 1.1)),
        1.0,
        Paint::Red,
    );
    for side in [-1.0f32, 1.0] {
        // Beacons on the rear corners of the frame.
        m.paint(
            &icosphere(1),
            at(Vec3::new(side * fx, top + 0.14, fz1 - 0.1), 0.11),
            1.0,
            Paint::Beacon,
        );
    }

    // The engine block, hazard-striped round its back edge.
    let block = [
        (fz1, chamfer_rect(3.2, 2.3, 0.5, 0.0)),
        (6.8, chamfer_rect(3.2, 2.3, 0.5, 0.0)),
        (7.0, chamfer_rect(3.4, 2.5, 0.55, 0.0)),
        (7.5, chamfer_rect(3.4, 2.5, 0.55, 0.0)),
    ];
    let banded = |s: usize, _| [Paint::Ochre, Paint::Metal, Paint::Hazard][s];
    m.add(&loft(&block, banded, Paint::Metal), id, 1.0);
    // Louvred vents down its flanks.
    for side in [-1.0f32, 1.0] {
        for k in 0..3 {
            m.paint(
                &cube,
                boxed(
                    Vec3::new(side * 1.61, -0.1, 5.75 + k as f32 * 0.38),
                    Vec3::new(0.04, 0.9, 0.16),
                ),
                1.0,
                Paint::Metal,
            );
        }
    }

    // Her number on the block's flanks and across its top, for whoever
    // follows.
    for side in [-1.0f32, 1.0] {
        let origin = Vec3::new(side * 1.6, -0.32, 4.5 + if side > 0.0 { 1.0 } else { 0.0 });
        let surface = flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
        stencil(&mut m, "14", 0.62, Paint::Metal, &surface);
    }
    let top = flat(Vec3::new(-0.6, 1.15, 5.95), Vec3::X, Vec3::NEG_Z);
    stencil(&mut m, "14", 0.9, Paint::Metal, &top);

    let mut nozzles = Vec::new();
    let mut cargo = 0;
    for (slot, module, p) in fitted(fit) {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => {
                // A chin turret under the cab.
                let turret = Vec3::new(p.x, -1.0, p.z + 0.2);
                m.paint(&icosphere(2), at(turret, 0.32), 1.0, Paint::Metal);
                gun(&mut m, turret, p.z - 1.3 * k, module);
            }
            SlotKind::Engine => {
                m.paint(
                    &cube,
                    boxed(Vec3::new(p.x, p.y, 7.75), Vec3::new(1.0, 1.0, 0.5)),
                    0.8,
                    Paint::Metal,
                );
                nozzles.push(bell(&mut m, Vec3::new(p.x, p.y, 8.0), module, 0.32));
            }
            SlotKind::Shield => shield(&mut m, Vec3::new(p.x, 1.42, p.z), module),
            SlotKind::Radar => radar(&mut m, Vec3::new(p.x, 1.55, p.z), module),
            SlotKind::Radiator => {
                let side = p.x.signum();
                radiator(
                    &mut m,
                    Vec3::new(side * 1.2, 1.15, p.z),
                    Vec3::new(side * 0.35, 1.0, 0.0),
                    1.5 * k,
                    2.2 * k,
                );
            }
            SlotKind::Cargo => {
                let paint = [Paint::Red, Paint::Hull, Paint::Hull, Paint::Red][cargo % 4];
                cargo += 1;
                container(
                    &mut m,
                    Vec3::new(p.x * 1.0, -0.65, p.z),
                    Vec3::new(1.65, 1.55, 2.6),
                    paint,
                );
            }
            _ => {}
        }
    }
    soot(&mut m, 5.0, 9.0, 0.45);
    (m, ShipShape { nozzles })
}

// --- Interceptor ----------------------------------------------------------

/// Radius of the interceptor's fuselage at `z`.
fn interceptor_radius(z: f32) -> f32 {
    if z < -2.0 {
        let t = ((z + 6.0) / 4.0).clamp(0.0, 1.0);
        0.9 * (1.0 - (1.0 - t).powi(2)).powf(0.8)
    } else {
        let t = ((z - 3.0) / 1.3).clamp(0.0, 1.0);
        0.9 - 0.25 * t
    }
}

fn interceptor_profile(z0: f32, z1: f32, steps: u32, inflate: f32) -> Vec<(f32, f32)> {
    (0..=steps)
        .map(|i| {
            let z = z0 + (z1 - z0) * i as f32 / steps as f32;
            (z, interceptor_radius(z) + inflate)
        })
        .collect()
}

/// The interceptor: a nose-heavy military dart. Gunmetal over a pale belly,
/// a red nose and red tips, a bubble canopy, cheek intakes, forward-swept
/// wings carrying gun pods, canted radiator fins for a tail and one big
/// engine.
fn interceptor(fit: &Loadout) -> (Mesh, ShipShape) {
    const SEGS: u32 = 28;
    let id = Mat4::IDENTITY;
    let mut m = Mesh::default();
    let up = TAU / 4.0;

    // Fuselage: a pale belly under gunmetal, a red nose, and a thin red
    // line where the two meet.
    let mut body = interceptor_profile(-4.6, 4.3, 30, 0.0);
    body.extend([(4.3, 0.65), (4.3, 0.0)]);
    m.add(&lathe(&body, SEGS, 0.0, TAU), id, 0.8);
    m.paint(
        &lathe(&interceptor_profile(-6.0, -4.6, 10, 0.0), SEGS, 0.0, TAU),
        id,
        1.0,
        Paint::Red,
    );
    m.paint(
        &lathe(
            &interceptor_profile(-4.6, 4.3, 30, 0.01),
            16,
            up - 1.3,
            up + 1.3,
        ),
        id,
        1.0,
        Paint::Gunmetal,
    );
    for edge in [up - 1.3, up + 1.3] {
        m.paint(
            &lathe(
                &interceptor_profile(-4.6, 4.3, 30, 0.015),
                1,
                edge - 0.05,
                edge + 0.05,
            ),
            id,
            1.0,
            Paint::Red,
        );
    }
    // Her number on each flank behind the canopy.
    for side in [-1.0f32, 1.0] {
        let z = if side > 0.0 { -0.1 } else { -0.95 };
        let surface = flank(side, z, 0.42, interceptor_radius);
        stencil(&mut m, "27", 0.36, Paint::Hull, &surface);
    }
    // An anti-glare panel ahead of the canopy.
    m.paint(
        &lathe(
            &interceptor_profile(-4.4, -3.4, 4, 0.02),
            6,
            up - 0.35,
            up + 0.35,
        ),
        id,
        1.0,
        Paint::Metal,
    );

    // Bubble canopy with two frame bows.
    let (cz, cy) = (-2.4f32, 0.62f32);
    m.paint(
        &icosphere(3),
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.5, 0.5, 1.4),
            Quat::IDENTITY,
            Vec3::new(0.0, cy, cz),
        ),
        1.0,
        Paint::Glass,
    );
    for z in [-2.4f32, -1.55] {
        let r = 0.5 * (1.0 - ((z - cz) / 1.4).powi(2)).sqrt() + 0.02;
        m.paint(
            &lathe(&[(z - 0.04, r), (z + 0.04, r)], 12, 0.0, TAU / 2.0),
            Mat4::from_translation(Vec3::Y * cy),
            1.0,
            Paint::Metal,
        );
    }

    // Cheek intakes.
    for side in [-1.0f32, 1.0] {
        let intake = [
            (-1.6, chamfer_rect(0.4, 0.55, 0.12, -0.15)),
            (1.4, chamfer_rect(0.3, 0.4, 0.1, -0.1)),
        ];
        m.add(
            &loft(
                &intake,
                |_, e| {
                    if e < 3 || e == 7 {
                        Paint::Gunmetal
                    } else {
                        Paint::Hull
                    }
                },
                Paint::Metal,
            ),
            Mat4::from_translation(Vec3::X * side * 0.82),
            0.95,
        );
    }

    // Forward-swept wings with red tips and beacons.
    let wing = |side: f32| {
        Mat4::from_translation(Vec3::new(side * 0.55, -0.2, 0.0))
            * Mat4::from_rotation_z(-side * (FRAC_PI_2 + 0.08))
    };
    let (root, tip, span) = ((-0.2, 2.6), (-2.3, -1.1), 3.0);
    for side in [-1.0f32, 1.0] {
        flapped(
            &mut m,
            wing(side),
            root,
            tip,
            span,
            0.16,
            Paint::Gunmetal,
            Paint::Red,
            0.8,
        );
        m.paint(
            &icosphere(1),
            wing(side) * at(Vec3::new(0.0, span + 0.06, -1.7), 0.1),
            1.0,
            Paint::Beacon,
        );
    }
    // A small ventral fin.
    m.add(
        &fin((2.3, 4.0), (3.3, 4.3), 0.75, 0.08, (0.0, 1.0)),
        Mat4::from_translation(Vec3::Y * -0.7) * Mat4::from_rotation_z(TAU / 2.0),
        1.0,
    );

    let mut nozzles = Vec::new();
    for (slot, module, p) in fitted(fit) {
        let k = scale(module.size);
        let side = if p.x < 0.0 { -1.0 } else { 1.0 };
        match slot.kind {
            SlotKind::Weapon => {
                // A gun pod slung under the wing on a pylon.
                let y = -0.55;
                let pod: Vec<(f32, f32)> = vec![
                    (-1.2, 0.0),
                    (-1.1, 0.12),
                    (-0.8, 0.19),
                    (0.8, 0.19),
                    (1.2, 0.08),
                    (1.25, 0.0),
                ];
                m.paint(
                    &lathe(&pod, 12, 0.0, TAU),
                    Mat4::from_translation(Vec3::new(p.x, y, 0.0)),
                    1.0,
                    Paint::Metal,
                );
                m.add(
                    &cube(),
                    boxed(Vec3::new(p.x, y + 0.2, 0.0), Vec3::new(0.06, 0.25, 1.2)),
                    0.9,
                );
                gun(&mut m, Vec3::new(p.x, y, -1.1), p.z - 0.3 * k, module);
            }
            SlotKind::Engine => {
                m.paint(
                    &lathe(&[(4.2, 0.64), (4.42, 0.6)], SEGS, 0.0, TAU),
                    id,
                    1.0,
                    Paint::Metal,
                );
                nozzles.push(bell(&mut m, Vec3::new(p.x, p.y, 4.35), module, 0.34));
            }
            SlotKind::Radiator => {
                // Canted fins make the tail.
                let t = Mat4::from_translation(Vec3::new(side * 0.42, 0.55, 0.0))
                    * Mat4::from_rotation_z(-side * 0.55);
                let (root, tip, span) = ((1.6, 4.1), (3.3, 4.7), 1.6 * k);
                flapped(
                    &mut m,
                    t,
                    root,
                    tip,
                    span,
                    0.12,
                    Paint::Gunmetal,
                    Paint::Red,
                    0.75,
                );
            }
            SlotKind::Shield => shield(&mut m, Vec3::new(p.x, 0.9, 0.3), module),
            SlotKind::Radar => {
                // A dark radome on the nose and a pitot probe.
                m.paint(
                    &lathe(&interceptor_profile(-6.0, -5.2, 6, 0.012), SEGS, 0.0, TAU),
                    id,
                    1.0,
                    Paint::Metal,
                );
                m.paint(
                    &cylinder(6),
                    strut(
                        Vec3::new(0.0, 0.0, -5.9),
                        Vec3::new(0.0, 0.0, -6.9),
                        0.03 * k,
                    ),
                    1.0,
                    Paint::Metal,
                );
            }
            _ => {}
        }
    }
    soot(&mut m, 2.5, 5.0, 0.4);
    (m, ShipShape { nozzles })
}
