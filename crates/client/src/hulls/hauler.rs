//! The hauler: the light trader's big working cousin, built like a freight
//! train. A long, light box truss in worn mustard runs the length of the
//! ship, and each section rides on it as its own wagon on a flat deck,
//! with hazard-striped buffer beams, buffers, drawbars, hoses and a railed
//! gangway across every coupling. A tall locomotive cab up front, wagons
//! behind (flatbeds with strapped-down containers, a tank car for the
//! reactor, a bolt-on turret wagon, a gantry for the shield), and an engine
//! wagon or caboose at the tail.

use std::f32::consts::TAU;

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::*;
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, icosphere, lathe, loft, strut,
    torus,
};

/// Top of the wagon decks.
const DECK: f32 = -1.9;
/// Half the width of a wagon deck.
const HALF_W: f32 = 2.2;
/// The spine truss: top, bottom and half-width.
const TRUSS_TOP: f32 = -2.35;
const TRUSS_BOTTOM: f32 = -3.75;
const TRUSS_W: f32 = 1.0;
/// How far each wagon stops short of its section's ends, leaving room for
/// the coupling.
const INSET: f32 = 1.1;
/// The hauler's fleet number.
const NUMBER: &str = "27";

/// How big a module is drawn on this hull.
fn km(module: &Module) -> f32 {
    scale(module.size) * 1.8
}

pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut m = Mesh::default();
    let mut nozzles = Vec::new();
    let sections = placed(fit);
    if sections.is_empty() {
        return crate::meshes::generated(fit);
    }
    let n = sections.len();

    // The truss runs from under the cab to the tail wagon.
    let z0 = sections[0].front + 4.0;
    let z1 = sections[n - 1].back - 3.0;
    truss(&mut m, z0, z1);

    let mut cargo = 0;
    for (i, p) in sections.iter().enumerate() {
        let wagon_no = format!("{NUMBER}-{}", i + 1);
        match p.section.id {
            "command-bow-m" => cab(&mut m, p),
            "gun-bow-m" => shunter(&mut m, p),
            "gun-deck-m" => turret_wagon(&mut m, p, &wagon_no),
            "shield-ring-m" => gantry_wagon(&mut m, p, &wagon_no),
            "cargo-bay-m" => flatbed(&mut m, p, &wagon_no, &mut cargo),
            "reactor-section-m" => tank_car(&mut m, p, &wagon_no),
            "sprint-stern-m" => engine_wagon(&mut m, p, &mut nozzles),
            "cruise-stern-m" => caboose(&mut m, p, &mut nozzles),
            _ => match p.section.position {
                Position::Bow => cab(&mut m, p),
                Position::Stern => caboose(&mut m, p, &mut nozzles),
                _ => flatbed(&mut m, p, &wagon_no, &mut cargo),
            },
        }
        if i + 1 < n {
            coupling(&mut m, p.back - INSET, p.back + INSET);
        }
    }
    let tail = sections[n - 1].back;
    soot(&mut m, tail - 6.0, tail + 3.0, 0.4);
    (m, ShipShape { nozzles })
}

// --- Small helpers ----------------------------------------------------------

fn bx(m: &mut Mesh, centre: Vec3, size: Vec3, shade: f32, paint: Paint) {
    m.paint(&cube(), boxed(centre, size), shade, paint);
}

fn rod(m: &mut Mesh, a: Vec3, b: Vec3, r: f32, paint: Paint) {
    m.paint(&cylinder(8), strut(a, b, r), 1.0, paint);
}

/// A square-section beam from `a` to `b`.
fn beam(m: &mut Mesh, a: Vec3, b: Vec3, t: f32, shade: f32, paint: Paint) {
    let d = b - a;
    let rot = Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y));
    m.paint(
        &cube(),
        Mat4::from_scale_rotation_translation(Vec3::new(t, d.length(), t), rot, (a + b) / 2.0),
        shade,
        paint,
    );
}

fn lamp(m: &mut Mesh, p: Vec3, r: f32) {
    m.paint(&icosphere(1), at(p, r), 1.0, Paint::Lamp);
}

fn beacon(m: &mut Mesh, p: Vec3, r: f32) {
    m.paint(&icosphere(1), at(p, r), 1.0, Paint::Beacon);
}

/// `text` stencilled on both flat flanks at `x = ±half`, centred on `zmid`.
fn flank_number(m: &mut Mesh, text: &str, h: f32, paint: Paint, half: f32, y: f32, zmid: f32) {
    let width = text.len() as f32 * 0.85 * h - 0.3 * h;
    for side in [-1.0f32, 1.0] {
        let origin = Vec3::new(side * half, y, zmid + side * width / 2.0);
        let surface = flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
        stencil(m, text, h, paint, &surface);
    }
}

/// A row of handrail: posts every metre or so and a rail along the top
/// and middle.
fn handrail(m: &mut Mesh, a: Vec3, b: Vec3, h: f32) {
    let posts = ((b - a).length() / 1.2).ceil().max(1.0) as i32;
    for i in 0..=posts {
        let p = a.lerp(b, i as f32 / posts as f32);
        rod(m, p, p + Vec3::Y * h, 0.04, Paint::Metal);
    }
    rod(m, a + Vec3::Y * h, b + Vec3::Y * h, 0.05, Paint::Red);
    rod(
        m,
        a + Vec3::Y * h * 0.5,
        b + Vec3::Y * h * 0.5,
        0.035,
        Paint::Red,
    );
}

/// A ladder from `a` up to `b`, standing off the surface.
fn ladder(m: &mut Mesh, a: Vec3, b: Vec3, across: Vec3) {
    let w = across * 0.3;
    rod(m, a - w, b - w, 0.035, Paint::Metal);
    rod(m, a + w, b + w, 0.035, Paint::Metal);
    let rungs = ((b - a).length() / 0.35) as i32;
    for i in 1..rungs {
        let p = a.lerp(b, i as f32 / rungs as f32);
        rod(m, p - w, p + w, 0.025, Paint::Metal);
    }
}

/// A block of four little thrusters, for the RCS.
fn rcs_quad(m: &mut Mesh, p: Vec3, out: Vec3) {
    bx(m, p, Vec3::splat(0.45), 0.9, Paint::Metal);
    for d in [out, Vec3::Y, Vec3::NEG_Y, Vec3::Z] {
        let base = p + d * 0.22;
        m.paint(
            &lathe(&[(0.0, 0.06), (0.22, 0.13)], 8, 0.0, TAU),
            Mat4::from_rotation_translation(Quat::from_rotation_arc(Vec3::Z, d), base),
            0.8,
            Paint::Metal,
        );
    }
}

/// A bolt-on turret: a slewing ring, a squat olive housing with a sloped
/// face and bolted cheek plates, and its gun. `up` is +1 on top, -1
/// slung underneath.
fn turret(m: &mut Mesh, base: Vec3, up: f32, k: f32, laser: bool, muzzle_z: f32) {
    let ring_top = base + Vec3::Y * up * 0.22 * k;
    rod(m, base, ring_top, 0.6 * k, Paint::Metal);
    let c = ring_top + Vec3::Y * up * 0.3 * k;
    let housing = [
        (
            -0.75 * k,
            chamfer_rect(0.9 * k, 0.3 * k, 0.1 * k, -0.05 * k),
        ),
        (-0.35 * k, chamfer_rect(1.15 * k, 0.6 * k, 0.15 * k, 0.0)),
        (0.6 * k, chamfer_rect(1.15 * k, 0.6 * k, 0.15 * k, 0.0)),
        (0.75 * k, chamfer_rect(0.95 * k, 0.45 * k, 0.12 * k, 0.0)),
    ];
    m.add(
        &loft(&housing, |_, _| Paint::Olive, Paint::Olive),
        Mat4::from_translation(c),
        1.0,
    );
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            c + Vec3::new(side * 0.6 * k, 0.0, 0.1 * k),
            Vec3::new(0.06 * k, 0.45 * k, 0.8 * k),
            0.85,
            Paint::Olive,
        );
    }
    // A sighting periscope on the roof.
    bx(
        m,
        c + Vec3::new(0.3 * k, up * 0.35 * k, 0.2 * k),
        Vec3::new(0.14, 0.2, 0.2) * k,
        1.0,
        Paint::Metal,
    );
    let breech = Vec3::new(c.x, c.y, c.z - 0.6 * k);
    gun(m, breech, muzzle_z, laser, k);
    // Red muzzle bands, like the light trader's.
    let off = if laser { 0.0 } else { 0.09 * k };
    for x in [-off, off] {
        rod(
            m,
            Vec3::new(c.x + x, c.y, muzzle_z + 0.15 * k),
            Vec3::new(c.x + x, c.y, muzzle_z + 0.32 * k),
            0.08 * k,
            Paint::Red,
        );
        if laser {
            break;
        }
    }
}

// --- The frame ------------------------------------------------------------

/// The spine: a light box truss in mustard, with cross frames every couple
/// of metres and braced on every face.
fn truss(m: &mut Mesh, z0: f32, z1: f32) {
    let (t, b, w) = (TRUSS_TOP, TRUSS_BOTTOM, TRUSS_W);
    for (x, y) in [(-w, t), (w, t), (-w, b), (w, b)] {
        beam(
            m,
            Vec3::new(x, y, z0),
            Vec3::new(x, y, z1),
            0.24,
            1.0,
            Paint::Ochre,
        );
    }
    let bays = ((z1 - z0) / 2.0).round().max(1.0) as i32;
    let z = |i: i32| z0 + (z1 - z0) * i as f32 / bays as f32;
    for i in 0..=bays {
        let zi = z(i);
        let c = |x: f32, y: f32| Vec3::new(x, y, zi);
        beam(m, c(-w, t), c(w, t), 0.16, 0.9, Paint::Ochre);
        beam(m, c(-w, b), c(w, b), 0.16, 0.9, Paint::Ochre);
        beam(m, c(-w, b), c(-w, t), 0.16, 0.9, Paint::Ochre);
        beam(m, c(w, b), c(w, t), 0.16, 0.9, Paint::Ochre);
        if i == bays {
            break;
        }
        let zn = z(i + 1);
        let flip = if i % 2 == 0 { 1.0 } else { -1.0 };
        // Diagonals down both sides and across the belly, zig-zagging.
        for x in [-w, w] {
            let (ya, yb) = if flip > 0.0 { (b, t) } else { (t, b) };
            rod(
                m,
                Vec3::new(x, ya, zi),
                Vec3::new(x, yb, zn),
                0.07,
                Paint::Metal,
            );
        }
        rod(
            m,
            Vec3::new(-w * flip, b, zi),
            Vec3::new(w * flip, b, zn),
            0.07,
            Paint::Metal,
        );
    }
    // Conduits along the inside.
    for x in [-0.4f32, 0.4] {
        rod(
            m,
            Vec3::new(x, b + 0.35, z0),
            Vec3::new(x, b + 0.35, z1),
            0.1,
            if x < 0.0 { Paint::Red } else { Paint::Metal },
        );
    }
}

/// A wagon's flat deck from `z0` to `z1`: deck plate, sole bars, buffer
/// beams with buffers on the coupled ends, saddle clamps onto the truss,
/// and its number on the sole bars.
fn wagon(m: &mut Mesh, z0: f32, z1: f32, front: bool, back: bool, number: Option<&str>) {
    let (mid, len) = ((z0 + z1) / 2.0, z1 - z0);
    bx(
        m,
        Vec3::new(0.0, DECK - 0.12, mid),
        Vec3::new(HALF_W * 2.0, 0.24, len),
        0.85,
        Paint::Ochre,
    );
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * (HALF_W - 0.1), DECK - 0.42, mid),
            Vec3::new(0.3, 0.5, len - 0.2),
            1.0,
            Paint::Ochre,
        );
        // Tie-down rings along the deck edge.
        let rings = (len / 1.6) as i32;
        for i in 0..rings {
            let z = z0 + len * (i as f32 + 0.5) / rings as f32;
            bx(
                m,
                Vec3::new(side * (HALF_W + 0.04), DECK - 0.2, z),
                Vec3::new(0.08, 0.16, 0.25),
                1.0,
                Paint::Metal,
            );
        }
    }
    // Saddle clamps onto the truss.
    for z in [z0 + 1.0, z1 - 1.0] {
        bx(
            m,
            Vec3::new(0.0, TRUSS_TOP + 0.05, z),
            Vec3::new(TRUSS_W * 2.0 + 0.5, 0.5, 0.5),
            0.8,
            Paint::Metal,
        );
    }
    for (end, coupled, dir) in [(z0, front, -1.0f32), (z1, back, 1.0)] {
        bx(
            m,
            Vec3::new(0.0, DECK - 0.35, end),
            Vec3::new(HALF_W * 2.0 + 0.2, 0.62, 0.3),
            1.0,
            Paint::Hazard,
        );
        if !coupled {
            continue;
        }
        for x in [-1.45f32, 1.45] {
            let a = Vec3::new(x, DECK - 0.35, end);
            let b = a + Vec3::Z * dir * 0.45;
            rod(m, a, b, 0.12, Paint::Metal);
            rod(m, b, b + Vec3::Z * dir * 0.07, 0.3, Paint::Metal);
        }
    }
    if let Some(text) = number {
        flank_number(m, text, 0.32, Paint::Metal, HALF_W + 0.06, DECK - 0.6, mid);
    }
}

/// The coupling between two wagons, from the back of one (`za`) to the
/// front of the next (`zb`): drawbar and knuckle, sagging hoses, and a
/// railed gangway at deck height.
fn coupling(m: &mut Mesh, za: f32, zb: f32) {
    let mid = (za + zb) / 2.0;
    let y = DECK - 0.35;
    rod(
        m,
        Vec3::new(0.0, y, za),
        Vec3::new(0.0, y, zb),
        0.13,
        Paint::Metal,
    );
    bx(
        m,
        Vec3::new(0.0, y, mid),
        Vec3::new(0.45, 0.4, 0.55),
        1.0,
        Paint::Red,
    );
    for (x, paint) in [(-0.75f32, Paint::Red), (0.75, Paint::Metal)] {
        let low = Vec3::new(x, y - 0.55, mid);
        rod(m, Vec3::new(x, y - 0.1, za), low, 0.06, paint);
        rod(m, low, Vec3::new(x, y - 0.1, zb), 0.06, paint);
    }
    // The gangway plate and its rails.
    bx(
        m,
        Vec3::new(0.0, DECK - 0.06, mid),
        Vec3::new(1.4, 0.08, zb - za + 0.3),
        0.75,
        Paint::Metal,
    );
    for x in [-0.72f32, 0.72] {
        handrail(
            m,
            Vec3::new(x, DECK, za + 0.1),
            Vec3::new(x, DECK, zb - 0.1),
            1.0,
        );
    }
}

// --- Bows -----------------------------------------------------------------

/// The command bow: a locomotive's cab. A short, rounded hood with a
/// hazard-striped nose, headlamps and a plough, and behind it a tall boxy
/// cab with a big raked windscreen, a sun visor, roof lamps and a horn.
fn cab(m: &mut Mesh, p: &Placed) {
    let (f, b) = (p.front, p.back - INSET);
    let id = Mat4::IDENTITY;
    wagon(m, f + 2.0, b, false, true, None);

    // The short hood.
    let hood = [
        (f + 0.3, chamfer_rect(3.2, 2.4, 0.7, -0.7)),
        (f + 1.0, chamfer_rect(3.8, 3.0, 0.6, -0.4)),
        (f + 4.2, chamfer_rect(3.8, 3.0, 0.6, -0.4)),
    ];
    let hood_paint = |s: usize, e: usize| match (s, e) {
        (0, _) => Paint::Hazard,
        (_, 4) => Paint::Metal,
        _ => Paint::Ochre,
    };
    m.add(&loft(&hood, hood_paint, Paint::Hazard), id, 1.0);
    // Louvres down the hood sides and a filler cap on top.
    for side in [-1.0f32, 1.0] {
        for i in 0..4 {
            bx(
                m,
                Vec3::new(side * 1.91, -0.1, f + 1.6 + i as f32 * 0.55),
                Vec3::new(0.05, 1.2, 0.2),
                1.0,
                Paint::Metal,
            );
        }
        // Headlamps either side of the nose, a red marker below.
        m.paint(
            &cylinder(12),
            strut(
                Vec3::new(side * 0.95, 0.15, f + 0.35),
                Vec3::new(side * 0.95, 0.15, f + 0.2),
                0.28,
            ),
            1.0,
            Paint::Lamp,
        );
        beacon(m, Vec3::new(side * 1.3, -1.3, f + 0.45), 0.13);
    }
    // The plough under the nose.
    let plough = [
        (f - 0.2, chamfer_rect(3.6, 0.4, 0.1, -2.1)),
        (f + 1.4, chamfer_rect(4.2, 0.9, 0.2, -2.0)),
    ];
    m.add(&loft(&plough, |_, _| Paint::Hazard, Paint::Metal), id, 0.9);

    // The cab.
    let (c0, c1) = (f + 4.0, f + 5.3);
    let roof = 4.8;
    let cab = [
        (c0, chamfer_rect(4.4, 4.7, 0.5, 0.45)),
        (c1, chamfer_rect(4.4, 6.7, 0.6, 1.45)),
        (b, chamfer_rect(4.4, 6.7, 0.6, 1.45)),
    ];
    let cab_paint = |s: usize, e: usize| match (s, e) {
        (0, 0 | 1 | 7) => Paint::Glass,
        (_, 4) => Paint::Metal,
        _ => Paint::Ochre,
    };
    m.add(&loft(&cab, cab_paint, Paint::Ochre), id, 1.0);
    // Mullions over the windscreen and a sun visor above it.
    for x in [-1.75f32, -0.6, 0.6, 1.75] {
        rod(
            m,
            Vec3::new(x, 2.82, c0 - 0.02),
            Vec3::new(x, roof - 0.03, c1 - 0.02),
            0.05,
            Paint::Metal,
        );
    }
    bx(
        m,
        Vec3::new(0.0, roof, c1 - 0.25),
        Vec3::new(4.0, 0.1, 0.7),
        0.9,
        Paint::Ochre,
    );
    // A cream band with a red pinstripe round the cab and the hood.
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * 2.21, 0.6, (c1 + b) / 2.0 - 0.6),
            Vec3::new(0.04, 0.55, b - c1 + 1.1),
            1.0,
            Paint::Hull,
        );
        bx(
            m,
            Vec3::new(side * 2.22, 0.2, (c1 + b) / 2.0 - 0.6),
            Vec3::new(0.04, 0.12, b - c1 + 1.1),
            1.0,
            Paint::Red,
        );
        bx(
            m,
            Vec3::new(side * 1.91, 0.6, f + 2.7),
            Vec3::new(0.04, 0.5, 3.0),
            1.0,
            Paint::Hull,
        );
        // Side windows, a door with grab rails, and steps up to it.
        for (z, w) in [(c1 + 0.9, 1.3), (c1 + 2.6, 1.3)] {
            bx(
                m,
                Vec3::new(side * 2.21, 3.4, z),
                Vec3::new(0.05, 1.1, w),
                1.0,
                Paint::Glass,
            );
        }
        let door = b - 1.3;
        bx(
            m,
            Vec3::new(side * 2.21, 1.3, door),
            Vec3::new(0.05, 2.8, 1.0),
            0.8,
            Paint::Ochre,
        );
        bx(
            m,
            Vec3::new(side * 2.23, 2.2, door),
            Vec3::new(0.05, 0.5, 0.6),
            1.0,
            Paint::Glass,
        );
        for dz in [-0.65f32, 0.65] {
            rod(
                m,
                Vec3::new(side * 2.35, 0.0, door + dz),
                Vec3::new(side * 2.35, 2.4, door + dz),
                0.04,
                Paint::Metal,
            );
        }
        ladder(
            m,
            Vec3::new(side * 2.35, DECK - 0.6, door),
            Vec3::new(side * 2.35, -0.1, door),
            Vec3::Z,
        );
    }
    flank_number(m, NUMBER, 1.3, Paint::Red, 2.23, -1.3, (c1 + b) / 2.0 - 0.4);
    // Roof: a red cap, a bar of lamps over the windscreen, a horn, beacons,
    // a hatch.
    bx(
        m,
        Vec3::new(0.0, roof + 0.02, (c1 + b) / 2.0),
        Vec3::new(3.4, 0.1, b - c1),
        1.0,
        Paint::Red,
    );
    bx(
        m,
        Vec3::new(0.0, roof + 0.15, c1 + 0.2),
        Vec3::new(3.0, 0.18, 0.2),
        1.0,
        Paint::Metal,
    );
    for x in [-1.2f32, -0.4, 0.4, 1.2] {
        bx(
            m,
            Vec3::new(x, roof + 0.35, c1 + 0.12),
            Vec3::new(0.45, 0.3, 0.12),
            1.0,
            Paint::Lamp,
        );
    }
    for x in [-0.9f32, -0.6] {
        m.paint(
            &lathe(&[(0.0, 0.06), (0.5, 0.1), (0.62, 0.2)], 10, 0.0, TAU),
            Mat4::from_translation(Vec3::new(x, roof + 0.25, c1 + 1.4)),
            1.0,
            Paint::Metal,
        );
    }
    bx(
        m,
        Vec3::new(0.0, roof + 0.05, b - 1.0),
        Vec3::new(1.2, 0.15, 1.2),
        0.85,
        Paint::Ochre,
    );
    for side in [-1.0f32, 1.0] {
        beacon(m, Vec3::new(side * 2.0, roof + 0.15, b - 0.2), 0.14);
    }

    for (slot, module, _) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon => {
                // A chin turret slung under the hood.
                turret(
                    m,
                    Vec3::new(0.0, -1.9, f + 2.4),
                    -1.0,
                    k,
                    is_laser(module),
                    f - 1.2 * k,
                );
            }
            SlotKind::Radar => {
                rod(
                    m,
                    Vec3::new(0.0, roof, f + 7.0),
                    Vec3::new(0.0, roof + 1.2, f + 7.0),
                    0.12,
                    Paint::Metal,
                );
                radar(m, Vec3::new(0.0, roof + 1.2, f + 7.0), k);
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, roof + 0.05, b - 2.6), k),
            SlotKind::Rcs => {
                for side in [-1.0f32, 1.0] {
                    rcs_quad(m, Vec3::new(side * 2.0, 0.8, f + 1.4), Vec3::X * side);
                    rcs_quad(
                        m,
                        Vec3::new(side * 2.45, roof - 0.4, b - 0.4),
                        Vec3::X * side,
                    );
                }
            }
            _ => {}
        }
    }
}

/// The gun bow: an armoured shunter. A long, low hood with a wedge plough
/// for a prow, chevroned in hazard stripes, two guns in bolted-on sponsons
/// on its cheeks, and a squat cab at the back with a slit windscreen
/// behind an armour grille.
fn shunter(m: &mut Mesh, p: &Placed) {
    let (f, b) = (p.front, p.back - INSET);
    let id = Mat4::IDENTITY;
    wagon(m, f + 3.0, b, false, true, None);

    let hood = [
        (f + 0.1, chamfer_rect(1.0, 1.6, 0.3, -1.2)),
        (f + 2.6, chamfer_rect(4.0, 3.4, 0.6, -0.3)),
        (f + 7.2, chamfer_rect(4.0, 3.4, 0.6, -0.3)),
    ];
    let hood_paint = |s: usize, e: usize| match (s, e) {
        (0, 0 | 1 | 7) => Paint::Ochre,
        (0, _) => Paint::Hazard,
        (_, 4) => Paint::Metal,
        _ => Paint::Ochre,
    };
    m.add(&loft(&hood, hood_paint, Paint::Metal), id, 1.0);
    // A spine of armour down the hood and rows of bolts.
    bx(
        m,
        Vec3::new(0.0, 1.45, f + 5.1),
        Vec3::new(0.7, 0.12, 4.2),
        0.85,
        Paint::Olive,
    );
    for side in [-1.0f32, 1.0] {
        for i in 0..6 {
            bx(
                m,
                Vec3::new(side * 2.02, 0.9, f + 3.4 + i as f32 * 0.7),
                Vec3::splat(0.1),
                1.0,
                Paint::Metal,
            );
        }
        // Lamps in the plough's shoulders.
        m.paint(
            &cylinder(10),
            strut(
                Vec3::new(side * 1.5, 0.5, f + 2.6),
                Vec3::new(side * 1.6, 0.5, f + 2.35),
                0.2,
            ),
            1.0,
            Paint::Lamp,
        );
    }

    // The squat cab.
    let (c0, c1) = (f + 7.0, f + 7.6);
    let cab = [
        (c0, chamfer_rect(3.8, 3.6, 0.5, 0.1)),
        (c1, chamfer_rect(3.8, 4.6, 0.6, 0.6)),
        (b, chamfer_rect(3.8, 4.6, 0.6, 0.6)),
    ];
    let cab_paint = |s: usize, e: usize| match (s, e) {
        (0, 0) => Paint::Glass,
        (_, 4) => Paint::Metal,
        _ => Paint::Ochre,
    };
    m.add(&loft(&cab, cab_paint, Paint::Ochre), id, 1.0);
    for i in 0..3 {
        let t = (i as f32 + 0.5) / 3.0;
        bx(
            m,
            Vec3::new(0.0, 1.9 + t * 1.0, c0 + t * 0.6 - 0.04),
            Vec3::new(2.6, 0.08, 0.1),
            1.0,
            Paint::Metal,
        );
    }
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * 1.91, 2.1, (c1 + b) / 2.0),
            Vec3::new(0.05, 0.3, b - c1 - 1.0),
            1.0,
            Paint::Glass,
        );
        bx(
            m,
            Vec3::new(side * 1.92, 0.9, (c1 + b) / 2.0),
            Vec3::new(0.04, 0.4, b - c1),
            1.0,
            Paint::Hull,
        );
        lamp(m, Vec3::new(side * 1.5, 3.0, b - 0.3), 0.2);
        beacon(m, Vec3::new(side * 1.7, 3.0, c1 + 0.2), 0.13);
    }
    flank_number(m, NUMBER, 1.0, Paint::Red, 2.03, -1.5, f + 5.0);

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon => {
                // A sponson bolted to the hood's cheek on an outrigger.
                let side = s.x.signum();
                let c = Vec3::new(side * 2.75, -0.7, f + 5.0);
                bx(m, c, Vec3::new(1.3, 1.3, 3.0), 1.0, Paint::Olive);
                bx(
                    m,
                    c + Vec3::new(0.0, 0.0, -1.55),
                    Vec3::new(1.0, 1.0, 0.15),
                    0.8,
                    Paint::Olive,
                );
                for dz in [-1.0f32, 1.0] {
                    beam(
                        m,
                        c + Vec3::new(-side * 0.6, 0.3, dz),
                        Vec3::new(side * 1.9, 0.3, c.z + dz),
                        0.25,
                        0.8,
                        Paint::Metal,
                    );
                }
                for dy in [-0.45f32, 0.45] {
                    for i in 0..4 {
                        bx(
                            m,
                            c + Vec3::new(side * 0.66, dy, -1.2 + i as f32 * 0.8),
                            Vec3::splat(0.1),
                            1.0,
                            Paint::Metal,
                        );
                    }
                }
                gun(
                    m,
                    c + Vec3::new(0.0, 0.0, -1.5),
                    f - 0.9 * k,
                    is_laser(module),
                    k,
                );
                bx(
                    m,
                    c + Vec3::new(0.0, 0.0, -1.7),
                    Vec3::new(0.6, 0.6, 0.3),
                    1.0,
                    Paint::Red,
                );
            }
            SlotKind::Radar => {
                rod(
                    m,
                    Vec3::new(0.0, 1.3, f + 3.6),
                    Vec3::new(0.0, 2.1, f + 3.6),
                    0.1,
                    Paint::Metal,
                );
                radar(m, Vec3::new(0.0, 2.1, f + 3.6), k);
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, 2.9, b - 1.6), k),
            SlotKind::Rcs => {
                for side in [-1.0f32, 1.0] {
                    rcs_quad(m, Vec3::new(side * 2.1, 0.4, f + 3.4), Vec3::X * side);
                    rcs_quad(m, Vec3::new(side * 2.1, 2.7, b - 0.4), Vec3::X * side);
                }
            }
            _ => {}
        }
    }
}

// --- Mid wagons -------------------------------------------------------------

/// The cargo bay: a flatbed wagon with stakes down its sides and its
/// containers strapped down to the deck. An empty slot shows the bare
/// deck and its twist-locks.
fn flatbed(m: &mut Mesh, p: &Placed, number: &str, cargo: &mut usize) {
    let (z0, z1) = (p.front + INSET, p.back - INSET);
    wagon(m, z0, z1, true, true, Some(number));
    for side in [-1.0f32, 1.0] {
        let stakes = ((p.len() - 2.0 * INSET) / 2.2).round() as i32;
        for i in 0..=stakes {
            let z = z0 + 0.2 + (z1 - z0 - 0.4) * i as f32 / stakes as f32;
            beam(
                m,
                Vec3::new(side * (HALF_W - 0.05), DECK, z),
                Vec3::new(side * (HALF_W - 0.05), DECK + 1.1, z),
                0.18,
                0.8,
                Paint::Metal,
            );
        }
    }
    beacon(m, Vec3::new(HALF_W - 0.05, DECK + 1.2, z1 - 0.2), 0.12);
    beacon(m, Vec3::new(-HALF_W + 0.05, DECK + 1.2, z0 + 0.2), 0.12);

    let slots: Vec<_> = p
        .section
        .slots
        .iter()
        .map(|s| p.centre - s.position_m[2] as f32)
        .collect();
    let mut filled = vec![false; slots.len()];
    for (slot, module, s) in p.modules() {
        if slot.kind != SlotKind::Cargo {
            continue;
        }
        if let Some(i) = slots.iter().position(|&z| (z - s.z).abs() < 0.01) {
            filled[i] = true;
        }
        let k = scale(module.size) / 1.4;
        let size = Vec3::new(3.7, 3.3, 4.3) * Vec3::new(k.min(1.0), k, 1.0);
        let paint = [Paint::Red, Paint::Hull, Paint::Sky, Paint::Hull][*cargo % 4];
        *cargo += 1;
        let c = Vec3::new(0.0, DECK + size.y / 2.0, s.z);
        container(m, c, size, paint);
        // Ratchet straps over the top and down the sides.
        for dz in [-1.2f32, 1.2] {
            bx(
                m,
                c + Vec3::new(0.0, size.y / 2.0 + 0.03, dz),
                Vec3::new(size.x + 0.1, 0.05, 0.2),
                1.0,
                Paint::Ochre,
            );
            for side in [-1.0f32, 1.0] {
                bx(
                    m,
                    c + Vec3::new(side * (size.x / 2.0 + 0.06), 0.0, dz),
                    Vec3::new(0.05, size.y, 0.2),
                    1.0,
                    Paint::Ochre,
                );
            }
        }
    }
    for (i, z) in slots.iter().enumerate() {
        if filled[i] {
            continue;
        }
        for (x, dz) in [(-1.7f32, -2.0f32), (1.7, -2.0), (-1.7, 2.0), (1.7, 2.0)] {
            bx(
                m,
                Vec3::new(x, DECK + 0.1, z + dz),
                Vec3::new(0.3, 0.2, 0.3),
                1.0,
                Paint::Red,
            );
        }
        // Chains lying slack across the empty bed.
        rod(
            m,
            Vec3::new(-HALF_W, DECK + 0.05, z - 0.8),
            Vec3::new(HALF_W, DECK + 0.05, z + 0.6),
            0.05,
            Paint::Metal,
        );
    }
}

/// The reactor section: a tank car. Each power plant is a cream pressure
/// tank with domed ends and hazard bands, cradled on saddles, with a
/// manway and a railed catwalk on top; radiators fold out sideways like
/// wings. An empty power slot leaves bare saddles and pipework.
fn tank_car(m: &mut Mesh, p: &Placed, number: &str) {
    let (z0, z1) = (p.front + INSET, p.back - INSET);
    wagon(m, z0, z1, true, true, Some(number));
    let r = 1.85;
    let cy = DECK + r + 0.25;
    // A manifold pipe running the length under where the tanks sit.
    for x in [-1.0f32, 1.0] {
        rod(
            m,
            Vec3::new(x, DECK + 0.2, z0 + 0.3),
            Vec3::new(x, DECK + 0.2, z1 - 0.3),
            0.12,
            Paint::Metal,
        );
    }
    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Power => {
                let half = 1.85;
                let profile: Vec<(f32, f32)> = [
                    (-half, 0.0),
                    (-half + 0.08, r * 0.55),
                    (-half + 0.35, r * 0.9),
                    (-half + 0.75, r),
                    (half - 0.75, r),
                    (half - 0.35, r * 0.9),
                    (half - 0.08, r * 0.55),
                    (half, 0.0),
                ]
                .into();
                let c = Vec3::new(0.0, cy, s.z);
                m.paint(
                    &lathe(&profile, 20, 0.0, TAU),
                    Mat4::from_translation(c),
                    1.0,
                    Paint::Hull,
                );
                // Hazard bands near the ends, a red belly band in the middle.
                for dz in [-0.95f32, 0.95] {
                    m.paint(
                        &lathe(&[(-0.22, r + 0.04), (0.22, r + 0.04)], 20, 0.0, TAU),
                        Mat4::from_translation(c + Vec3::Z * dz),
                        1.0,
                        Paint::Hazard,
                    );
                }
                m.paint(
                    &lathe(&[(-0.08, r + 0.03), (0.08, r + 0.03)], 20, 0.0, TAU),
                    Mat4::from_translation(c),
                    1.0,
                    Paint::Red,
                );
                // Saddles.
                for dz in [-1.1f32, 1.1] {
                    bx(
                        m,
                        Vec3::new(0.0, DECK + 0.45, s.z + dz),
                        Vec3::new(3.0, 0.9, 0.35),
                        0.8,
                        Paint::Metal,
                    );
                }
                // The manway dome on top, and a tag on the tank.
                m.paint(
                    &cylinder(12),
                    strut(c + Vec3::Y * (r - 0.1), c + Vec3::Y * (r + 0.35), 0.38),
                    1.0,
                    Paint::Metal,
                );
                m.paint(
                    &icosphere(1),
                    at(c + Vec3::new(0.0, r + 0.38, 0.0), 0.2),
                    1.0,
                    Paint::Red,
                );
            }
            SlotKind::Radiator => {
                let side = s.x.signum();
                bx(
                    m,
                    Vec3::new(side * (HALF_W + 0.2), DECK + 0.5, s.z),
                    Vec3::new(0.4, 0.6, 1.0),
                    0.9,
                    Paint::Metal,
                );
                radiator(
                    m,
                    Vec3::new(side * (HALF_W + 0.3), DECK + 0.5, s.z),
                    Vec3::new(side, 0.12, 0.0),
                    2.0 * k,
                    2.3 * k,
                );
            }
            _ => {}
        }
    }
    // A catwalk along the top with its rails, and a ladder up the side.
    let top = cy + r + 0.05;
    bx(
        m,
        Vec3::new(0.0, top, p.centre),
        Vec3::new(0.9, 0.08, z1 - z0 - 1.6),
        0.75,
        Paint::Metal,
    );
    for x in [-0.45f32, 0.45] {
        handrail(
            m,
            Vec3::new(x, top, z0 + 0.8),
            Vec3::new(x, top, z1 - 0.8),
            0.8,
        );
    }
    ladder(
        m,
        Vec3::new(HALF_W - 0.25, DECK, z1 - 0.4),
        Vec3::new(0.5, top, z1 - 0.4),
        Vec3::Z,
    );
}

/// The gun deck: a turret wagon. A low armoured housing on a flat deck
/// with olive plates bolted on, guns in bolt-on turrets cantilevered out
/// on platforms either side, one on a pedestal on the roof and one slung
/// under the truss; ammunition lockers and a radiator at the back.
fn turret_wagon(m: &mut Mesh, p: &Placed, number: &str) {
    let (z0, z1) = (p.front + INSET, p.back - INSET);
    wagon(m, z0, z1, true, true, Some(number));
    let id = Mat4::IDENTITY;
    let roof = 0.9;
    let house = [
        (z0 + 0.6, chamfer_rect(2.4, 2.0, 0.5, DECK + 1.0)),
        (
            z0 + 1.6,
            chamfer_rect(3.2, DECK.abs() + roof, 0.6, (DECK + roof) / 2.0),
        ),
        (
            z1 - 1.0,
            chamfer_rect(3.2, DECK.abs() + roof, 0.6, (DECK + roof) / 2.0),
        ),
        (z1 - 0.4, chamfer_rect(2.6, 2.2, 0.5, DECK + 1.1)),
    ];
    let house_paint = |_, e: usize| if e == 4 { Paint::Metal } else { Paint::Ochre };
    m.add(&loft(&house, house_paint, Paint::Ochre), id, 1.0);
    // Olive plates bolted over the housing's flanks.
    for side in [-1.0f32, 1.0] {
        for (i, z) in [z0 + 2.6, z0 + 4.8, z0 + 7.0, z0 + 9.2]
            .into_iter()
            .enumerate()
        {
            if z > z1 - 1.6 {
                continue;
            }
            bx(
                m,
                Vec3::new(side * 1.64, -0.6, z),
                Vec3::new(0.1, 1.9, 2.0),
                if i % 2 == 0 { 0.92 } else { 1.05 },
                Paint::Olive,
            );
            for dy in [-1.4f32, 0.2] {
                bx(
                    m,
                    Vec3::new(side * 1.71, dy, z),
                    Vec3::new(0.08, 0.1, 1.7),
                    1.0,
                    Paint::Metal,
                );
            }
        }
        // Ammunition lockers on the deck edge.
        for i in 0..3 {
            bx(
                m,
                Vec3::new(side * 1.9, DECK + 0.3, z1 - 1.0 - i as f32 * 0.8),
                Vec3::new(0.45, 0.6, 0.65),
                1.0,
                Paint::Olive,
            );
        }
    }
    bx(
        m,
        Vec3::new(0.0, roof + 0.05, p.centre),
        Vec3::new(3.25, 0.1, 0.3),
        1.0,
        Paint::Hazard,
    );

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon if s.y.abs() < 0.5 => {
                // A side turret on a cantilevered platform with a railing.
                let side = s.x.signum();
                let base = Vec3::new(s.x + side * 0.4, -0.7, s.z);
                bx(
                    m,
                    Vec3::new(side * (1.6 + (base.x.abs() - 0.4) / 2.0), -0.8, s.z),
                    Vec3::new(base.x.abs() + 0.6 - 1.6, 0.2, 3.0),
                    0.9,
                    Paint::Ochre,
                );
                bx(
                    m,
                    Vec3::new(base.x + side * 1.0, -0.8, s.z),
                    Vec3::new(0.15, 0.25, 3.0),
                    1.0,
                    Paint::Hazard,
                );
                for dz in [-1.2f32, 1.2] {
                    beam(
                        m,
                        Vec3::new(side * 1.6, DECK, s.z + dz),
                        Vec3::new(base.x, -0.9, s.z + dz),
                        0.18,
                        0.8,
                        Paint::Metal,
                    );
                }
                turret(m, base, 1.0, k, is_laser(module), s.z - 2.2 * k);
            }
            SlotKind::Weapon if s.y > 0.0 => {
                let base = Vec3::new(0.0, s.y - 0.6, s.z);
                m.paint(
                    &cylinder(12),
                    strut(Vec3::new(0.0, roof, s.z), base, 0.5),
                    0.9,
                    Paint::Metal,
                );
                turret(m, base, 1.0, k, is_laser(module), s.z - 2.4 * k);
            }
            SlotKind::Weapon => {
                let base = Vec3::new(0.0, TRUSS_BOTTOM - 0.1, s.z);
                turret(m, base, -1.0, k, is_laser(module), s.z - 2.4 * k);
            }
            SlotKind::Radiator => {
                radiator(
                    m,
                    Vec3::new(0.0, roof, s.z.min(z1 - 1.8)),
                    Vec3::Y,
                    2.2 * k / 1.4,
                    2.4 * k / 1.4,
                );
            }
            _ => {}
        }
    }
}

/// The shield ring: a gantry wagon. Two octagonal girder hoops stand round
/// the whole frame, truss and all, braced together, with the emitters on
/// top and underneath; the generator sits on the deck between them.
fn gantry_wagon(m: &mut Mesh, p: &Placed, number: &str) {
    let (z0, z1) = (p.front + INSET, p.back - INSET);
    wagon(m, z0, z1, true, true, Some(number));
    let r = 4.0;
    let cy = -0.5;
    let corner = |i: usize, z: f32| {
        let a = (i as f32 + 0.5) / 8.0 * TAU;
        Vec3::new(r * a.cos(), cy + r * a.sin(), z)
    };
    let hoops = [p.centre - 2.2, p.centre + 1.0];
    for &z in &hoops {
        for i in 0..8 {
            // Hazard stripes on the lower faces, mustard elsewhere.
            let paint = if (4..=6).contains(&i) {
                Paint::Hazard
            } else {
                Paint::Ochre
            };
            beam(m, corner(i, z), corner(i + 1, z), 0.45, 1.0, paint);
        }
        // Feet down to the deck edge.
        for side in [-1.0f32, 1.0] {
            beam(
                m,
                Vec3::new(side * HALF_W, DECK, z),
                Vec3::new(side * r * 0.92, cy, z),
                0.3,
                0.85,
                Paint::Metal,
            );
        }
    }
    for i in 0..8 {
        beam(
            m,
            corner(i, hoops[0]),
            corner(i, hoops[1]),
            0.2,
            0.85,
            Paint::Ochre,
        );
        rod(
            m,
            corner(i, hoops[0]),
            corner(i + 1, hoops[1]),
            0.06,
            Paint::Metal,
        );
    }
    // Lamps on the hoop's shoulders.
    for i in [0usize, 3] {
        lamp(m, corner(i, hoops[1]) + Vec3::Z * 0.3, 0.2);
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Shield => {
                let up = s.y.signum();
                let tip = Vec3::new(
                    0.0,
                    cy + up * (r * 0.93 + 0.25),
                    (hoops[0] + hoops[1]) / 2.0,
                );
                bx(m, tip, Vec3::new(1.6, 0.4, 3.6), 0.9, Paint::Metal);
                shield(m, tip + Vec3::Y * up * 0.2, k);
            }
            SlotKind::Power => {
                // The generator: a ribbed drum on a skid between the hoops.
                let c = Vec3::new(0.0, DECK + 1.3, (hoops[0] + hoops[1]) / 2.0);
                bx(
                    m,
                    Vec3::new(0.0, DECK + 0.15, c.z),
                    Vec3::new(2.8, 0.3, 3.4),
                    0.8,
                    Paint::Metal,
                );
                m.paint(
                    &cylinder(16),
                    strut(c - Vec3::Z * 1.5, c + Vec3::Z * 1.5, 1.05),
                    1.0,
                    Paint::Red,
                );
                for i in 0..6 {
                    let z = c.z - 1.25 + i as f32 * 0.5;
                    m.paint(
                        &torus(0.1, 16, 4),
                        Mat4::from_rotation_translation(
                            Quat::from_rotation_x(TAU / 4.0),
                            Vec3::new(0.0, c.y, z),
                        ) * Mat4::from_scale(Vec3::splat(1.06)),
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            SlotKind::Radiator => {
                radiator(
                    m,
                    Vec3::new(0.0, DECK, s.z.min(z1 - 1.4)),
                    Vec3::Y,
                    2.3 * k / 1.4,
                    1.6 * k / 1.4,
                );
            }
            _ => {}
        }
    }
}

// --- Sterns -----------------------------------------------------------------

/// The sprint stern: an engine wagon. An open girder cage on a flat deck
/// with two engines in it, their chambers wrapped in gold blanket and
/// their bells hung out the back bare; radiators stand up off the cage and
/// hang down below the truss.
fn engine_wagon(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (z0, z1) = (p.front + INSET, p.back - 1.4);
    wagon(m, z0, z1, true, false, None);
    let (cw, top) = (3.3f32, 2.0f32);
    // The cage.
    for x in [-cw, cw] {
        for y in [DECK, top] {
            beam(
                m,
                Vec3::new(x, y, z0 + 0.3),
                Vec3::new(x, y, z1),
                0.3,
                1.0,
                Paint::Ochre,
            );
        }
        for z in [z0 + 0.3, p.centre, z1] {
            beam(
                m,
                Vec3::new(x, DECK, z),
                Vec3::new(x, top, z),
                0.3,
                1.0,
                Paint::Ochre,
            );
        }
        rod(
            m,
            Vec3::new(x, DECK, z0 + 0.3),
            Vec3::new(x, top, p.centre),
            0.08,
            Paint::Metal,
        );
        rod(
            m,
            Vec3::new(x, top, p.centre),
            Vec3::new(x, DECK, z1),
            0.08,
            Paint::Metal,
        );
    }
    for z in [z0 + 0.3, p.centre, z1] {
        beam(
            m,
            Vec3::new(-cw, top, z),
            Vec3::new(cw, top, z),
            0.3,
            1.0,
            if z == z1 { Paint::Hazard } else { Paint::Ochre },
        );
    }
    // The deck is wider than a wagon's here: outriggers to the cage.
    bx(
        m,
        Vec3::new(0.0, DECK - 0.1, (z0 + z1) / 2.0),
        Vec3::new(cw * 2.0, 0.2, z1 - z0),
        0.8,
        Paint::Ochre,
    );
    // The thrust frame at the back of the cage.
    bx(
        m,
        Vec3::new(0.0, 0.0, z1 + 0.2),
        Vec3::new(cw * 2.0 - 0.6, 3.6, 0.4),
        0.75,
        Paint::Metal,
    );
    for side in [-1.0f32, 1.0] {
        beacon(m, Vec3::new(side * cw, top + 0.2, z1), 0.15);
        beacon(m, Vec3::new(side * cw, DECK - 0.3, z1), 0.15);
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Engine => {
                let c = Vec3::new(s.x, 0.0, 0.0);
                let profile: Vec<(f32, f32)> = vec![
                    (z0 + 1.0, 0.0),
                    (z0 + 1.0, 0.7),
                    (z0 + 1.6, 1.15),
                    (z1 - 0.8, 1.15),
                    (z1, 0.6),
                    (z1 + 0.4, 0.5),
                ];
                m.paint(
                    &lathe(&profile, 16, 0.0, TAU),
                    Mat4::from_translation(c),
                    1.0,
                    Paint::Foil,
                );
                for z in [z0 + 2.2, p.centre, z1 - 1.4] {
                    m.paint(
                        &lathe(&[(-0.12, 1.2), (0.12, 1.2)], 16, 0.0, TAU),
                        Mat4::from_translation(Vec3::new(c.x, 0.0, z)),
                        1.0,
                        Paint::Metal,
                    );
                }
                // Feed lines over the chamber.
                rod(
                    m,
                    Vec3::new(c.x * 0.6, 1.2, z0 + 1.6),
                    Vec3::new(c.x * 0.6, 0.8, z1 - 0.6),
                    0.12,
                    Paint::Metal,
                );
                nozzles.push(bell(m, Vec3::new(s.x, 0.0, z1 + 0.4), k, 0.55));
            }
            SlotKind::Power => {
                let c = Vec3::new(0.0, DECK + 0.7, s.z);
                bx(m, c, Vec3::new(1.4, 1.4, 2.0), 0.95, Paint::Hazard);
                bx(
                    m,
                    c + Vec3::Y * 0.75,
                    Vec3::new(1.0, 0.1, 1.6),
                    1.0,
                    Paint::Metal,
                );
            }
            SlotKind::Radiator => {
                let up = s.y.signum();
                let root = if up > 0.0 {
                    Vec3::new(0.0, top + 0.1, s.z)
                } else {
                    Vec3::new(0.0, TRUSS_BOTTOM, s.z)
                };
                radiator(m, root, Vec3::Y * up, 2.4 * k / 1.4, 3.0 * k / 1.4);
            }
            SlotKind::JumpDrive => {
                let c = Vec3::new(0.0, top + 0.3, z0 + 1.6);
                for i in 0..3 {
                    m.paint(
                        &torus(0.18, 16, 6),
                        Mat4::from_rotation_translation(
                            Quat::from_rotation_x(TAU / 4.0),
                            c + Vec3::Z * (i as f32 * 0.3),
                        ) * Mat4::from_scale(Vec3::splat(0.5)),
                        1.0,
                        if i == 1 { Paint::Red } else { Paint::Foil },
                    );
                }
            }
            _ => {}
        }
    }
}

/// The cruise stern: the caboose. A boxy van with a lookout cupola on its
/// roof, glazed all round, a railed back porch, and one big engine hung
/// off the back in a thrust ring; radiators stand out from its flanks.
fn caboose(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (z0, z1) = (p.front + INSET, p.back - 1.6);
    wagon(m, z0, z1, true, false, None);
    let id = Mat4::IDENTITY;
    let (v0, v1) = (z0 + 0.4, z1 - 1.6);
    let top = 2.1;
    let h = top - DECK;
    let van = [
        (v0, chamfer_rect(4.0, h, 0.4, (top + DECK) / 2.0)),
        (v1, chamfer_rect(4.0, h, 0.4, (top + DECK) / 2.0)),
    ];
    m.add(
        &loft(
            &van,
            |_, e| if e == 4 { Paint::Metal } else { Paint::Ochre },
            Paint::Ochre,
        ),
        id,
        1.0,
    );
    // The roof overhang, a red waist band, end windows and side windows.
    bx(
        m,
        Vec3::new(0.0, top + 0.05, (v0 + v1) / 2.0),
        Vec3::new(4.3, 0.12, v1 - v0 + 0.5),
        0.85,
        Paint::Red,
    );
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * 2.01, 0.1, (v0 + v1) / 2.0),
            Vec3::new(0.04, 0.4, v1 - v0),
            1.0,
            Paint::Hull,
        );
        for z in [v0 + 1.3, v1 - 1.3] {
            bx(
                m,
                Vec3::new(side * 2.01, 1.0, z),
                Vec3::new(0.05, 0.75, 0.9),
                1.0,
                Paint::Glass,
            );
        }
        // Louvres for the power plant.
        for i in 0..4 {
            bx(
                m,
                Vec3::new(side * 2.02, 1.0, (v0 + v1) / 2.0 - 0.75 + i as f32 * 0.5),
                Vec3::new(0.05, 0.8, 0.18),
                1.0,
                Paint::Metal,
            );
        }
        // Grab rails at the porch.
        rod(
            m,
            Vec3::new(side * 2.05, DECK, v1 - 0.1),
            Vec3::new(side * 2.05, top - 0.2, v1 - 0.1),
            0.05,
            Paint::Metal,
        );
    }
    flank_number(m, NUMBER, 1.0, Paint::Red, 2.03, -1.3, (v0 + v1) / 2.0);
    // The lookout cupola, glazed all round.
    let cz = (v0 + v1) / 2.0 - 0.4;
    let cupola = [
        (cz - 1.1, chamfer_rect(2.6, 1.2, 0.25, top + 0.65)),
        (cz + 1.1, chamfer_rect(2.6, 1.2, 0.25, top + 0.65)),
    ];
    m.add(
        &loft(
            &cupola,
            |_, e| if e == 0 { Paint::Ochre } else { Paint::Glass },
            Paint::Glass,
        ),
        id,
        1.0,
    );
    bx(
        m,
        Vec3::new(0.0, top + 1.3, cz),
        Vec3::new(2.9, 0.12, 2.6),
        0.9,
        Paint::Ochre,
    );
    beacon(m, Vec3::new(0.0, top + 1.5, cz), 0.2);
    // The back porch: a railed platform behind the van.
    bx(
        m,
        Vec3::new(0.0, DECK - 0.05, (v1 + z1) / 2.0),
        Vec3::new(4.2, 0.12, z1 - v1),
        0.8,
        Paint::Metal,
    );
    for side in [-1.0f32, 1.0] {
        handrail(
            m,
            Vec3::new(side * 2.0, DECK, v1 + 0.1),
            Vec3::new(side * 2.0, DECK, z1),
            1.0,
        );
        lamp(m, Vec3::new(side * 1.6, top - 0.3, v1 + 0.08), 0.18);
        beacon(m, Vec3::new(side * 2.0, DECK - 0.3, z1 + 0.2), 0.16);
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Engine => {
                // Struts from the van's back wall out to a thrust ring.
                let ring = Vec3::new(s.x, 0.0, z1 + 0.6);
                for i in 0..6 {
                    let a = i as f32 / 6.0 * TAU + 0.3;
                    let on_ring = ring + Vec3::new(a.cos(), a.sin(), 0.0) * 1.0;
                    let on_wall = Vec3::new(a.cos() * 1.5, a.sin() * 1.4, v1);
                    rod(m, on_wall, on_ring, 0.1, Paint::Metal);
                }
                m.paint(
                    &torus(0.2, 20, 6),
                    Mat4::from_rotation_translation(Quat::from_rotation_x(TAU / 4.0), ring),
                    1.0,
                    Paint::Hazard,
                );
                m.paint(
                    &lathe(&[(v1, 0.6), (z1 + 0.6, 0.75)], 16, 0.0, TAU),
                    Mat4::from_translation(Vec3::new(s.x, 0.0, 0.0)),
                    0.8,
                    Paint::Metal,
                );
                nozzles.push(bell(m, ring, k * 1.2, 0.75));
            }
            SlotKind::Power => {
                // A generator housing on the roof with an exhaust stack.
                let c = Vec3::new(0.0, top + 0.5, v0 + 0.9);
                bx(m, c, Vec3::new(2.4, 0.9, 1.4), 0.9, Paint::Metal);
                rod(
                    m,
                    c + Vec3::new(0.7, 0.4, 0.0),
                    c + Vec3::new(0.7, 1.4, 0.0),
                    0.18,
                    Paint::Metal,
                );
            }
            SlotKind::Radiator => {
                let side = s.x.signum();
                radiator(
                    m,
                    Vec3::new(side * 2.0, 0.9, s.z.min(v1 - 1.5)),
                    Vec3::new(side, 0.35, 0.0),
                    2.0 * k / 1.4,
                    2.6 * k / 1.4,
                );
            }
            SlotKind::JumpDrive => {
                let c = Vec3::new(0.0, DECK + 0.6, (v1 + z1) / 2.0);
                m.paint(
                    &torus(0.25, 16, 6),
                    Mat4::from_translation(c) * Mat4::from_scale(Vec3::splat(0.55)),
                    1.0,
                    Paint::Foil,
                );
            }
            _ => {}
        }
    }
}
