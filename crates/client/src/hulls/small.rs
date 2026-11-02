//! The small hulls, each modelled whole.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{Loadout, SlotKind};

use super::kit::*;
use super::{at, fitted, is_laser, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, fin, icosphere, lathe, loft, strut,
};

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

/// The courier, everyone's first ship, hand-built rather than generated:
/// a cream enamel rocket of the old optimistic school, nose toward -Z, with
/// a red nose and cheatline, a wraparound windscreen, lit portholes, three
/// swept radiator fins, a gold-foil service collar and a clustered engine.
/// Scaled so it is the hull's length from nose to bell exit. Whatever sits
/// in its chin mount (a mining laser, stock) hangs under the nose.
pub(super) fn courier(loadout: &Loadout) -> (Mesh, ShipShape) {
    let length = loadout.stats().length_m as f32;
    const SEGS: u32 = 36;
    let (cyl, ball) = (cylinder(16), icosphere(2));
    let mut m = Mesh::default();
    let id = Mat4::IDENTITY;

    // Fuselage: red nose cap, cream body.
    let mut nose = profile(&starter_radius, -12.0, -9.6, 8, 0.0);
    nose.insert(0, (-12.0, 0.0));
    m.paint(&lathe(&nose, SEGS, 0.0, TAU), id, 1.0, Paint::Red);
    m.add(
        &lathe(
            &profile(&starter_radius, -9.6, 4.0, 40, 0.0),
            SEGS,
            0.0,
            TAU,
        ),
        id,
        1.0,
    );
    // Panel seams: thin dark rings.
    for z in [-9.6, -5.0, 2.6] {
        m.paint(
            &lathe(
                &profile(&starter_radius, z - 0.04, z + 0.04, 1, 0.012),
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
            &profile(&starter_radius, -8.85, -7.75, 6, 0.015),
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
                &profile(&starter_radius, -8.75, -7.85, 6, 0.03),
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
                &profile(&starter_radius, -9.6, 3.4, 30, 0.01),
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
                &profile(&starter_radius, -9.0, 3.4, 30, 0.01),
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
    for (slot, module, p) in fitted(loadout) {
        if slot.kind == SlotKind::Weapon {
            // A pod on a short pylon under the nose.
            let pod = Vec3::new(p.x, p.y - 0.1, p.z + 0.5);
            out.paint(
                &cylinder(10),
                strut(pod + Vec3::Y * 0.45, pod, 0.08),
                1.0,
                Paint::Metal,
            );
            out.paint(&icosphere(2), at(pod, 0.24), 1.0, Paint::Red);
            let k = scale(module.size);
            gun(&mut out, pod, p.z - 0.9 * k, is_laser(module), k);
        }
    }
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
pub(super) fn light_trader(fit: &Loadout) -> (Mesh, ShipShape) {
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
                gun(&mut m, turret, p.z - 1.3 * k, is_laser(module), k);
            }
            SlotKind::Engine => {
                m.paint(
                    &cube,
                    boxed(Vec3::new(p.x, p.y, 7.75), Vec3::new(1.0, 1.0, 0.5)),
                    0.8,
                    Paint::Metal,
                );
                nozzles.push(bell(&mut m, Vec3::new(p.x, p.y, 8.0), k, 0.32));
            }
            SlotKind::Shield => shield(&mut m, Vec3::new(p.x, 1.42, p.z), k),
            SlotKind::Radar => radar(&mut m, Vec3::new(p.x, 1.55, p.z), k),
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

/// The interceptor: a nose-heavy military dart. Gunmetal over a pale belly,
/// a red nose and red tips, a bubble canopy, cheek intakes, forward-swept
/// wings carrying gun pods, canted radiator fins for a tail and one big
/// engine.
pub(super) fn interceptor(fit: &Loadout) -> (Mesh, ShipShape) {
    const SEGS: u32 = 28;
    let id = Mat4::IDENTITY;
    let mut m = Mesh::default();
    let up = TAU / 4.0;

    // Fuselage: a pale belly under gunmetal, a red nose, and a thin red
    // line where the two meet.
    let mut body = profile(&interceptor_radius, -4.6, 4.3, 30, 0.0);
    body.extend([(4.3, 0.65), (4.3, 0.0)]);
    m.add(&lathe(&body, SEGS, 0.0, TAU), id, 0.8);
    m.paint(
        &lathe(
            &profile(&interceptor_radius, -6.0, -4.6, 10, 0.0),
            SEGS,
            0.0,
            TAU,
        ),
        id,
        1.0,
        Paint::Red,
    );
    m.paint(
        &lathe(
            &profile(&interceptor_radius, -4.6, 4.3, 30, 0.01),
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
                &profile(&interceptor_radius, -4.6, 4.3, 30, 0.015),
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
            &profile(&interceptor_radius, -4.4, -3.4, 4, 0.02),
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
                gun(
                    &mut m,
                    Vec3::new(p.x, y, -1.1),
                    p.z - 0.3 * k,
                    is_laser(module),
                    k,
                );
            }
            SlotKind::Engine => {
                m.paint(
                    &lathe(&[(4.2, 0.64), (4.42, 0.6)], SEGS, 0.0, TAU),
                    id,
                    1.0,
                    Paint::Metal,
                );
                nozzles.push(bell(&mut m, Vec3::new(p.x, p.y, 4.35), k, 0.34));
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
            SlotKind::Shield => shield(&mut m, Vec3::new(p.x, 0.9, 0.3), k),
            SlotKind::Radar => {
                // A dark radome on the nose and a pitot probe.
                m.paint(
                    &lathe(
                        &profile(&interceptor_radius, -6.0, -5.2, 6, 0.012),
                        SEGS,
                        0.0,
                        TAU,
                    ),
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

// --- Heavy fighter --------------------------------------------------------

/// The heavy fighter: a flying tank. A squat armoured wedge in olive drab
/// over a gunmetal belly, bolted-on armour plates down its flanks, a low
/// cupola with a slit windscreen, stub wings that are really gun pylons
/// (two guns stacked on each), a chin cannon, and an armoured engine skirt.
/// Red only on the muzzles and the number.
pub(super) fn heavy_fighter(fit: &Loadout) -> (Mesh, ShipShape) {
    let (cube, cyl) = (cube(), cylinder(10));
    let id = Mat4::IDENTITY;
    let mut m = Mesh::default();

    // The hull: a blunt wedge widening into a slab-sided box.
    let hull = [
        (-6.6, chamfer_rect(1.6, 0.35, 0.12, -0.35)),
        (-4.4, chamfer_rect(3.3, 1.2, 0.45, -0.15)),
        (-1.0, chamfer_rect(4.0, 1.6, 0.6, 0.0)),
        (3.2, chamfer_rect(4.0, 1.6, 0.6, 0.0)),
        (5.2, chamfer_rect(2.8, 1.4, 0.5, 0.0)),
    ];
    let two_tone = |_, e: usize| {
        if (3..=5).contains(&e) {
            Paint::Gunmetal
        } else {
            Paint::Olive
        }
    };
    m.add(&loft(&hull, two_tone, Paint::Gunmetal), id, 1.0);

    // Bolted armour plates down each flank, with rivet rows.
    for side in [-1.0f32, 1.0] {
        for (k, (z0, z1)) in [(-0.8f32, 0.5f32), (0.6, 1.9), (2.0, 3.1)]
            .into_iter()
            .enumerate()
        {
            let shade = if k % 2 == 0 { 0.92 } else { 1.06 };
            m.paint(
                &cube,
                boxed(
                    Vec3::new(side * 2.04, 0.0, (z0 + z1) / 2.0),
                    Vec3::new(0.08, 0.36, z1 - z0),
                ),
                shade,
                Paint::Olive,
            );
            for y in [-0.13f32, 0.13] {
                for i in 0..4 {
                    let z = z0 + 0.12 + (z1 - z0 - 0.24) * i as f32 / 3.0;
                    m.paint(
                        &cube,
                        boxed(Vec3::new(side * 2.09, y, z), Vec3::splat(0.05)),
                        1.0,
                        Paint::Metal,
                    );
                }
            }
        }
        // Her number on the aft plate.
        let surface = flat(
            Vec3::new(side * 2.09, -0.15, if side > 0.0 { 2.9 } else { 2.2 }),
            Vec3::new(0.0, 0.0, -side),
            Vec3::Y,
        );
        stencil(&mut m, "63", 0.3, Paint::Red, &surface);
    }

    // The cupola: low and armoured, a slit windscreen across its face and
    // vision slits down its sides.
    let cupola = [
        (-3.9, chamfer_rect(0.9, 0.25, 0.1, 0.62)),
        (-3.3, chamfer_rect(1.4, 0.6, 0.2, 0.85)),
        (-1.0, chamfer_rect(1.4, 0.6, 0.2, 0.92)),
        (0.2, chamfer_rect(1.0, 0.3, 0.12, 0.8)),
    ];
    let slit = |s: usize, e: usize| match (s, e) {
        (0, 0 | 1 | 7) => Paint::Glass,
        _ => Paint::Olive,
    };
    m.add(&loft(&cupola, slit, Paint::Olive), id, 1.02);
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cube,
            boxed(
                Vec3::new(side * 0.71, 0.97, -2.6),
                Vec3::new(0.03, 0.08, 0.9),
            ),
            1.0,
            Paint::Glass,
        );
    }

    // Stub wings, thick and straight: pylons more than wings.
    let wing = |side: f32| {
        Mat4::from_translation(Vec3::new(side * 1.85, -0.25, 0.0))
            * Mat4::from_rotation_z(-side * FRAC_PI_2)
    };
    for side in [-1.0f32, 1.0] {
        m.paint(
            &fin((-4.4, 0.4), (-4.2, -1.6), 1.7, 0.3, (0.0, 1.0)),
            wing(side),
            1.0,
            Paint::Olive,
        );
        m.paint(
            &icosphere(1),
            wing(side) * at(Vec3::new(0.0, 1.75, -2.9), 0.1),
            1.0,
            Paint::Beacon,
        );
    }

    // The armoured engine skirt.
    let skirt = [
        (5.2, chamfer_rect(2.8, 1.4, 0.5, 0.0)),
        (6.3, chamfer_rect(3.1, 1.7, 0.6, 0.0)),
    ];
    m.add(&loft(&skirt, |_, _| Paint::Gunmetal, Paint::Metal), id, 0.9);

    let mut nozzles = Vec::new();
    let muzzle = |m: &mut Mesh, p: Vec3, k: f32| {
        m.paint(
            &cyl,
            strut(p + Vec3::Z * 0.12 * k, p, 0.09 * k),
            1.0,
            Paint::Red,
        );
    };
    for (slot, module, p) in fitted(fit) {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon if p.x.abs() < 0.5 => {
                // A chin cannon in a ball mount.
                let ball = Vec3::new(0.0, -0.8, -4.1);
                m.paint(&icosphere(2), at(ball, 0.42), 1.0, Paint::Gunmetal);
                let tip = -6.9;
                gun(&mut m, ball, tip, is_laser(module), k);
                for x in [-0.09f32, 0.09] {
                    muzzle(&mut m, Vec3::new(x * k, ball.y, tip), k);
                }
            }
            SlotKind::Weapon => {
                // A pod under the wing with two guns stacked in it.
                let pod = Vec3::new(p.x * 1.3, -0.5, -2.6);
                m.paint(
                    &cube,
                    boxed(pod, Vec3::new(0.42, 0.62, 1.7)),
                    1.0,
                    Paint::Gunmetal,
                );
                for dy in [0.15f32, -0.15] {
                    let breech = pod + Vec3::new(0.0, dy, -0.8);
                    gun(&mut m, breech, -5.1, is_laser(module), k * 0.8);
                    muzzle(&mut m, Vec3::new(pod.x + 0.072, breech.y, -5.1), 0.8);
                }
            }
            SlotKind::Engine => {
                nozzles.push(bell(&mut m, Vec3::new(p.x, p.y, 6.0), k, 0.46));
            }
            SlotKind::Shield => shield(&mut m, Vec3::new(0.0, 0.82, 1.0), k),
            SlotKind::Radar => radar(&mut m, Vec3::new(0.0, 1.22, -1.7), k * 0.8),
            SlotKind::Radiator if p.x.abs() < 0.5 => {
                radiator(&mut m, Vec3::new(0.0, 0.8, p.z), Vec3::Y, 1.1 * k, 1.8 * k);
            }
            SlotKind::Radiator => {
                let side = p.x.signum();
                radiator(
                    &mut m,
                    Vec3::new(side * 1.5, 0.75, p.z),
                    Vec3::new(side, 1.1, 0.0),
                    1.0 * k,
                    1.8 * k,
                );
            }
            _ => {}
        }
    }
    soot(&mut m, 3.0, 6.5, 0.4);
    (m, ShipShape { nozzles })
}

// --- Racer ----------------------------------------------------------------

/// The racer: an engine with a seat. A huge engine in cream with a sky-blue
/// racing stripe, a tiny cockpit pod perched on its nose under a bubble,
/// and long outrigger fins carrying roundels and the racing number.
pub(super) fn racer(fit: &Loadout) -> (Mesh, ShipShape) {
    const SEGS: u32 = 32;
    let id = Mat4::IDENTITY;
    let mut m = Mesh::default();
    let up = TAU / 4.0;

    // The engine: everything else hangs off it.
    let engine = [
        (-1.7, 0.55),
        (-1.4, 0.95),
        (-0.8, 1.12),
        (2.4, 1.18),
        (3.3, 1.02),
        (3.3, 1.02),
        (3.5, 0.95),
    ];
    m.add(&lathe(&engine, SEGS, 0.0, TAU), id, 1.05);
    // An intake lip, and the racing stripe over the top.
    m.paint(
        &lathe(&[(-1.75, 0.5), (-1.62, 0.62), (-1.55, 0.5)], SEGS, 0.0, TAU),
        id,
        1.0,
        Paint::Metal,
    );
    let stripe = |inflate: f32| -> Vec<(f32, f32)> {
        engine[1..5]
            .iter()
            .map(|&(z, r)| (z, r + inflate))
            .collect()
    };
    m.paint(
        &lathe(&stripe(0.01), 6, up - 0.32, up + 0.32),
        id,
        1.0,
        Paint::Sky,
    );
    for edge in [up - 0.42, up + 0.38] {
        m.paint(
            &lathe(&stripe(0.012), 1, edge, edge + 0.04),
            id,
            1.0,
            Paint::Red,
        );
    }
    // A red band round the tail.
    m.paint(
        &lathe(&[(2.7, 1.17), (3.0, 1.12)], SEGS, 0.0, TAU),
        id,
        1.0,
        Paint::Red,
    );

    // The cockpit pod on the nose: a sky-blue teardrop with a bubble.
    let pod: Vec<(f32, f32)> = (0..=14)
        .map(|i| {
            let t = i as f32 / 14.0;
            let z = -4.7 + 3.4 * t;
            (z, 0.58 * (t * std::f32::consts::PI).sin().powf(0.6))
        })
        .collect();
    let perch = Mat4::from_translation(Vec3::Y * 0.3);
    m.paint(&lathe(&pod, 24, 0.0, TAU), perch, 1.0, Paint::Sky);
    m.paint(
        &icosphere(3),
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.36, 0.34, 0.8),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.72, -3.0),
        ),
        1.0,
        Paint::Glass,
    );
    // Struts tying the pod to the engine.
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cylinder(8),
            strut(
                Vec3::new(side * 0.3, 0.1, -2.0),
                Vec3::new(side * 0.7, 0.0, -1.0),
                0.05,
            ),
            1.0,
            Paint::Metal,
        );
    }

    let mut nozzles = Vec::new();
    for (slot, module, p) in fitted(fit) {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Engine => {
                nozzles.push(bell(&mut m, Vec3::new(0.0, 0.0, 3.45), k * 1.1, 0.72));
            }
            SlotKind::Radar => {
                // A long pitot boom off the pod's nose.
                m.paint(
                    &cylinder(6),
                    strut(Vec3::new(0.0, 0.3, -4.6), Vec3::new(0.0, 0.3, -5.9), 0.025),
                    1.0,
                    Paint::Red,
                );
            }
            SlotKind::Radiator => {
                // A long outrigger fin, a tip pod, and a roundel on top.
                let side = p.x.signum();
                let t = Mat4::from_translation(Vec3::new(side * 1.05, 0.0, 0.0))
                    * Mat4::from_rotation_z(-side * FRAC_PI_2);
                let span = 2.6 * k;
                m.add(
                    &fin((0.4, 3.2), (2.0, 3.3), span, 0.07, (0.0, 1.0)),
                    t,
                    1.05,
                );
                let tip = Vec3::new(side * (1.05 + span), 0.0, 2.6);
                let cigar = [(-1.0, 0.0), (-0.7, 0.13), (0.6, 0.13), (1.0, 0.0)];
                m.paint(
                    &lathe(&cigar, 12, 0.0, TAU),
                    Mat4::from_translation(tip),
                    1.0,
                    Paint::Red,
                );
                let disc = |r: f32, y: f32, paint: Paint, m: &mut Mesh| {
                    m.paint(
                        &cylinder(24),
                        Mat4::from_scale_rotation_translation(
                            Vec3::new(r, 0.012, r),
                            Quat::IDENTITY,
                            Vec3::new(side * (1.05 + span * 0.55), y, 2.55),
                        ),
                        1.0,
                        paint,
                    );
                };
                disc(0.5, 0.035, Paint::Red, &mut m);
                disc(0.42, 0.047, Paint::Hull, &mut m);
                let x0 = side * (1.05 + span * 0.55) - 0.17;
                let surface = flat(Vec3::new(x0, 0.06, 2.75), Vec3::X, Vec3::NEG_Z);
                stencil(&mut m, "5", 0.4, Paint::Red, &surface);
            }
            _ => {}
        }
    }
    (m, ShipShape { nozzles })
}
