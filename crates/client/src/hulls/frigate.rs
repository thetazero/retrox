//! The frigate: a warship off a 1960s poster, heroic rather than menacing.
//! One continuous armoured hull in deep gunmetal, slab-sided and
//! chamfered, with strong horizontal lines (an armour belt down each flank
//! with a cream line along its top) crossed by strong vertical ones
//! (buttresses, the stepped superstructure, the bridge tower). Red for the
//! prow, the identification number, roof caps and muzzles; work lamps and
//! beacons on every corner.

use std::f32::consts::TAU;

use glam::{Mat4, Quat, Vec2, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::*;
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, icosphere, lathe, loft, strut,
};

/// The hull's cross-section: width, height (centred on y = 0).
const W: f32 = 13.0;
const H: f32 = 10.0;
/// The armour belt's centre height and depth on each flank.
const BELT_Y: f32 = -1.6;
const BELT_H: f32 = 1.8;
/// The frigate's pennant number.
const NUMBER: &str = "412";

/// How big a module is drawn on this hull.
fn km(module: &Module) -> f32 {
    scale(module.size) * 2.5
}

pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut m = Mesh::default();
    let mut nozzles = Vec::new();
    let sections = placed(fit);
    if sections.is_empty() {
        return crate::meshes::generated(fit);
    }
    let n = sections.len();
    let mut cargo = 0;
    for (i, p) in sections.iter().enumerate() {
        match p.section.id {
            "gun-bow-l" => ram_prow(&mut m, p),
            "command-bow-l" => tower_bow(&mut m, p),
            "gun-deck-l" => battle_deck(&mut m, p),
            "shield-ring-l" => collar(&mut m, p),
            "cargo-bay-l" => hangar(&mut m, p, &mut cargo),
            "reactor-section-l" => drum(&mut m, p),
            "sprint-stern-l" => sprint_stern(&mut m, p, &mut nozzles),
            "cruise-stern-l" => cruise_stern(&mut m, p, &mut nozzles),
            _ => match p.section.position {
                Position::Bow => tower_bow(&mut m, p),
                Position::Stern => cruise_stern(&mut m, p, &mut nozzles),
                _ => battle_deck(&mut m, p),
            },
        }
        if i + 1 < n {
            seam(&mut m, p.back);
        }
    }
    // The gunmetal goes on a shade deeper than the stock enamel.
    let gunmetal = Paint::Gunmetal as u32 as f32;
    for v in &mut m.vertices {
        if v.paint == gunmetal {
            v.shade *= 0.6;
        }
    }
    let tail = sections[n - 1].back;
    soot(&mut m, tail - 12.0, tail + 4.0, 0.45);
    (m, ShipShape { nozzles })
}

// --- Small helpers ----------------------------------------------------------

fn bx(m: &mut Mesh, centre: Vec3, size: Vec3, shade: f32, paint: Paint) {
    m.paint(&cube(), boxed(centre, size), shade, paint);
}

fn rod(m: &mut Mesh, a: Vec3, b: Vec3, r: f32, paint: Paint) {
    m.paint(&cylinder(12), strut(a, b, r), 1.0, paint);
}

/// The frigate's slab section: a rectangle with heavy chamfers.
fn prof(w: f32, h: f32, y: f32) -> Vec<Vec2> {
    chamfer_rect(w, h, w.min(h) * 0.2, y)
}

/// A run of hull through `sections`, gunmetal over a dark belly.
fn slab(m: &mut Mesh, sections: &[(f32, Vec<Vec2>)], x: f32) {
    let paint = |_, e: usize| {
        if e == 4 {
            Paint::Metal
        } else {
            Paint::Gunmetal
        }
    };
    m.add(
        &loft(sections, paint, Paint::Gunmetal),
        Mat4::from_translation(Vec3::X * x),
        1.0,
    );
}

/// The standard hull from `z0` to `z1`.
fn hull(m: &mut Mesh, z0: f32, z1: f32) {
    slab(m, &[(z0, prof(W, H, 0.0)), (z1, prof(W, H, 0.0))], 0.0);
}

/// The armour belt down both flanks, a cream line along its top.
fn belt(m: &mut Mesh, z0: f32, z1: f32, half: f32) {
    let (mid, len) = ((z0 + z1) / 2.0, z1 - z0);
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * (half + 0.3), BELT_Y, mid),
            Vec3::new(0.6, BELT_H, len),
            0.85,
            Paint::Gunmetal,
        );
        bx(
            m,
            Vec3::new(side * (half + 0.33), BELT_Y + BELT_H / 2.0 + 0.12, mid),
            Vec3::new(0.62, 0.24, len),
            1.0,
            Paint::Hull,
        );
    }
}

/// Vertical buttresses on both flanks, every `spacing` metres, above and
/// below the belt.
fn buttresses(m: &mut Mesh, z0: f32, z1: f32, spacing: f32, half: f32) {
    let count = ((z1 - z0) / spacing).floor().max(1.0) as i32;
    for i in 0..count {
        let z = z0 + (z1 - z0) * (i as f32 + 0.5) / count as f32;
        for side in [-1.0f32, 1.0] {
            let x = side * (half + 0.2);
            bx(
                m,
                Vec3::new(x, (BELT_Y + BELT_H / 2.0 + 3.3) / 2.0 + 0.2, z),
                Vec3::new(0.4, 3.3 - BELT_Y - BELT_H / 2.0, 0.7),
                0.9,
                Paint::Gunmetal,
            );
            bx(
                m,
                Vec3::new(x, (BELT_Y - BELT_H / 2.0 - 3.3) / 2.0, z),
                Vec3::new(0.4, 3.3 + BELT_Y - BELT_H / 2.0, 0.7),
                0.9,
                Paint::Gunmetal,
            );
        }
    }
}

/// The joint between two sections: a heavy dark flange round the hull.
fn seam(m: &mut Mesh, z: f32) {
    slab(
        m,
        &[
            (z - 0.35, prof(W + 0.5, H + 0.5, 0.0)),
            (z + 0.35, prof(W + 0.5, H + 0.5, 0.0)),
        ],
        0.0,
    );
    // Picked out in red along the top.
    bx(
        m,
        Vec3::new(0.0, H / 2.0 + 0.27, z),
        Vec3::new(W * 0.55, 0.06, 0.72),
        1.0,
        Paint::Red,
    );
}

/// The pennant number, big and red, on both flanks.
fn pennant(m: &mut Mesh, h: f32, half: f32, y: f32, zmid: f32) {
    let width = NUMBER.len() as f32 * 0.85 * h - 0.3 * h;
    for side in [-1.0f32, 1.0] {
        let origin = Vec3::new(side * half, y, zmid + side * width / 2.0);
        let surface = flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
        stencil(m, NUMBER, h, Paint::Red, &surface);
    }
}

/// A searchlight looking along `dir`.
fn searchlight(m: &mut Mesh, p: Vec3, dir: Vec3, r: f32) {
    let dir = dir.normalize();
    rod(m, p - dir * r * 1.2, p, r * 1.15, Paint::Metal);
    rod(m, p, p + dir * 0.08, r, Paint::Lamp);
}

fn beacon(m: &mut Mesh, p: Vec3, r: f32) {
    m.paint(&icosphere(1), at(p, r), 1.0, Paint::Beacon);
}

/// Heavy barrels from `breech` forward to `muzzle_z`: two (three on the
/// largest mounts) with blast sleeves and red muzzles, or one fat laser
/// projector with cooling rings and a lens.
fn barrels(m: &mut Mesh, breech: Vec3, muzzle_z: f32, laser: bool, k: f32) {
    let to = |x: f32| Vec3::new(breech.x + x, breech.y, muzzle_z);
    if laser {
        rod(m, breech, to(0.0), 0.1 * k, Paint::Metal);
        let len = breech.z - muzzle_z;
        for f in [0.2f32, 0.35, 0.5, 0.65] {
            let z = breech.z - len * f;
            rod(
                m,
                Vec3::new(breech.x, breech.y, z),
                Vec3::new(breech.x, breech.y, z - 0.08 * k),
                0.2 * k,
                Paint::Metal,
            );
        }
        rod(
            m,
            to(0.0) + Vec3::Z * 0.25 * k,
            to(0.0),
            0.14 * k,
            Paint::Red,
        );
        m.paint(&icosphere(1), at(to(0.0), 0.12 * k), 1.0, Paint::Glass);
        return;
    }
    let count = if k >= 4.5 { 3 } else { 2 };
    let gap = 0.3 * k;
    for i in 0..count {
        let x = (i as f32 - (count - 1) as f32 / 2.0) * gap;
        let root = breech + Vec3::X * x;
        rod(m, root, to(x), 0.06 * k, Paint::Metal);
        rod(m, root, root - Vec3::Z * 0.45 * k, 0.1 * k, Paint::Gunmetal);
        rod(m, to(x) + Vec3::Z * 0.3 * k, to(x), 0.085 * k, Paint::Red);
    }
}

/// A heavy turret on its barbette: an angular house with a sloped glacis,
/// a flat roof and an overhanging bustle, a rangefinder across its back,
/// a red band. `up` is +1 on top of the ship, -1 slung under it.
fn turret(m: &mut Mesh, base: Vec3, up: f32, k: f32, laser: bool, muzzle_z: f32) {
    let ring = base + Vec3::Y * up * 0.25 * k;
    rod(m, base - Vec3::Y * up * 0.3, ring, 0.85 * k, Paint::Metal);
    let c = ring + Vec3::Y * up * 0.32 * k;
    let house = [
        (
            -1.05 * k,
            chamfer_rect(1.3 * k, 0.3 * k, 0.08 * k, -0.12 * k * up),
        ),
        (-0.55 * k, chamfer_rect(1.7 * k, 0.64 * k, 0.12 * k, 0.0)),
        (0.8 * k, chamfer_rect(1.7 * k, 0.64 * k, 0.12 * k, 0.0)),
        (
            1.05 * k,
            chamfer_rect(1.4 * k, 0.5 * k, 0.1 * k, 0.04 * k * up),
        ),
    ];
    m.add(
        &loft(&house, |_, _| Paint::Gunmetal, Paint::Gunmetal),
        Mat4::from_translation(c),
        1.0,
    );
    // A red band round the house, the rangefinder arms out of its back.
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            c + Vec3::new(side * 0.86 * k, 0.0, 0.15 * k),
            Vec3::new(0.03 * k, 0.16 * k, 1.1 * k),
            1.0,
            Paint::Red,
        );
        bx(
            m,
            c + Vec3::new(side * 1.05 * k, up * 0.18 * k, 0.6 * k),
            Vec3::new(0.4 * k, 0.12 * k, 0.12 * k),
            1.0,
            Paint::Metal,
        );
    }
    // Gun ports: a mantlet across the glacis.
    bx(
        m,
        c + Vec3::new(0.0, 0.0, -0.6 * k),
        Vec3::new(1.2 * k, 0.4 * k, 0.15 * k),
        0.8,
        Paint::Metal,
    );
    barrels(m, Vec3::new(c.x, c.y, c.z - 0.65 * k), muzzle_z, laser, k);
}

/// Thruster blocks for the RCS at the corners of a bow.
fn rcs(m: &mut Mesh, z: f32, half_w: f32, half_h: f32) {
    for sx in [-1.0f32, 1.0] {
        for sy in [-1.0f32, 1.0] {
            let p = Vec3::new(sx * half_w, sy * half_h, z);
            bx(m, p, Vec3::new(1.0, 1.0, 1.4), 0.8, Paint::Metal);
            for d in [Vec3::X * sx, Vec3::Y * sy] {
                m.paint(
                    &lathe(&[(0.0, 0.15), (0.45, 0.32)], 10, 0.0, TAU),
                    Mat4::from_rotation_translation(
                        Quat::from_rotation_arc(Vec3::Z, d),
                        p + d * 0.5,
                    ),
                    0.8,
                    Paint::Metal,
                );
            }
        }
    }
}

// --- Bows -------------------------------------------------------------------

/// The gun bow: a massive armoured ram. The hull draws down to a red
/// stem with a knife-edged ram under it, two heavy casemates are built
/// into its cheeks, and a low armoured conning position with a slit
/// windscreen sits on top.
fn ram_prow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let sections = [
        (f + 0.6, prof(3.0, 4.2, -1.6)),
        (f + 2.5, prof(6.0, 6.4, -1.3)),
        (f + 9.0, prof(11.0, 9.0, -0.5)),
        (f + 14.0, prof(W, H, 0.0)),
        (b, prof(W, H, 0.0)),
    ];
    let paint = |s: usize, e: usize| match (s, e) {
        (0, _) => Paint::Red,
        (_, 4) => Paint::Metal,
        _ => Paint::Gunmetal,
    };
    m.add(&loft(&sections, paint, Paint::Red), Mat4::IDENTITY, 1.0);
    // The ram: a blade under the stem.
    let ram = [
        (f - 3.0, chamfer_rect(0.5, 1.0, 0.15, -3.7)),
        (f + 1.0, chamfer_rect(1.6, 2.6, 0.4, -3.6)),
        (f + 9.0, chamfer_rect(3.0, 2.0, 0.6, -4.9)),
    ];
    let ram_paint = |s: usize, _| if s == 0 { Paint::Red } else { Paint::Gunmetal };
    m.add(&loft(&ram, ram_paint, Paint::Red), Mat4::IDENTITY, 0.95);
    // A vertical stem rib up the bow.
    bx(
        m,
        Vec3::new(0.0, 1.0, f + 1.6),
        Vec3::new(0.5, 3.4, 2.4),
        1.0,
        Paint::Red,
    );
    belt(m, f + 12.0, b, W / 2.0);
    buttresses(m, b - 4.0, b, 3.0, W / 2.0);
    pennant(m, 3.5, W / 2.0 + 0.02, -0.4, f + 19.5);

    // The casemates: always there; a fitted gun shows in its port.
    let case = |x: f32| {
        [
            (f + 4.0, chamfer_rect(2.6, 2.4, 0.5, 0.0)),
            (f + 6.0, chamfer_rect(3.4, 3.8, 0.7, 0.0)),
            (f + 14.0, chamfer_rect(3.4, 3.8, 0.7, 0.0)),
            (f + 15.5, chamfer_rect(2.6, 3.0, 0.5, 0.0)),
        ]
        .map(|(z, pts)| (z, pts.into_iter().map(|p| p + Vec2::new(x, -2.2)).collect()))
    };
    for side in [-1.0f32, 1.0] {
        let x = side * 5.0;
        slab(m, &case(x), 0.0);
        // Ribs across its top.
        for i in 0..4 {
            bx(
                m,
                Vec3::new(x, -0.25, f + 7.0 + i as f32 * 2.0),
                Vec3::new(3.0, 0.2, 0.5),
                0.85,
                Paint::Gunmetal,
            );
        }
        searchlight(
            m,
            Vec3::new(side * 3.6, 1.2, f + 6.5),
            Vec3::new(side * 0.2, 0.0, -1.0),
            0.45,
        );
    }

    // The conning position: low, armoured, a slit windscreen.
    let conn = [
        (f + 13.0, prof(5.0, 1.0, H / 2.0 + 0.3)),
        (f + 15.0, prof(6.5, 2.6, H / 2.0 + 1.1)),
        (b - 2.5, prof(6.5, 2.6, H / 2.0 + 1.1)),
        (b - 1.5, prof(5.5, 1.6, H / 2.0 + 0.6)),
    ];
    slab(m, &conn, 0.0);
    bx(
        m,
        Vec3::new(0.0, H / 2.0 + 1.25, f + 14.95),
        Vec3::new(5.4, 0.35, 0.12),
        1.0,
        Paint::Glass,
    );
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * 3.26, H / 2.0 + 1.35, f + 17.5),
            Vec3::new(0.1, 0.3, 3.0),
            1.0,
            Paint::Glass,
        );
        beacon(m, Vec3::new(side * 3.0, H / 2.0 + 2.5, b - 2.6), 0.3);
    }
    bx(
        m,
        Vec3::new(0.0, H / 2.0 + 2.45, c + 3.0),
        Vec3::new(5.4, 0.15, 6.0),
        1.0,
        Paint::Red,
    );

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon => {
                let x = s.x.signum() * 5.0;
                barrels(
                    m,
                    Vec3::new(x, -2.2, f + 4.2),
                    f - 0.5 * k,
                    is_laser(module),
                    k,
                );
                bx(
                    m,
                    Vec3::new(x, -2.2, f + 4.0),
                    Vec3::new(1.8, 1.0, 0.3),
                    0.8,
                    Paint::Metal,
                );
            }
            SlotKind::Radar => {
                let base = Vec3::new(0.0, 3.6, f + 9.5);
                for side in [-1.0f32, 1.0] {
                    rod(
                        m,
                        base + Vec3::new(side * 1.2, 0.0, 0.8),
                        base + Vec3::Y * 2.4,
                        0.12,
                        Paint::Metal,
                    );
                }
                rod(
                    m,
                    base - Vec3::Z * 0.8,
                    base + Vec3::Y * 2.4,
                    0.12,
                    Paint::Metal,
                );
                radar(m, base + Vec3::Y * 2.4, k);
            }
            SlotKind::Shield => shield(m, Vec3::new(0.0, H / 2.0 + 2.5, c - 0.5), k),
            SlotKind::Rcs => rcs(m, f + 12.0, W / 2.0 - 0.6, H / 2.0 - 0.6),
            _ => {}
        }
    }
}

/// The command bow: a stepped tower bridge. A chisel bow with a cream
/// cap, then three tiers stepping up to a tall tower with a band of lit
/// bridge windows, bridge wings, a red roof and searchlights.
fn tower_bow(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let sections = [
        (f + 0.3, prof(6.0, 4.6, -0.8)),
        (f + 2.0, prof(9.5, 7.5, -0.5)),
        (f + 7.0, prof(W, H, 0.0)),
        (b, prof(W, H, 0.0)),
    ];
    let paint = |s: usize, e: usize| match (s, e) {
        (0, _) => Paint::Hull,
        (_, 4) => Paint::Metal,
        _ => Paint::Gunmetal,
    };
    m.add(&loft(&sections, paint, Paint::Hull), Mat4::IDENTITY, 1.0);
    // A red ring behind the cap.
    slab(
        m,
        &[
            (f + 2.0, prof(9.7, 7.7, -0.5)),
            (f + 2.6, prof(10.3, 8.1, -0.45)),
        ],
        0.0,
    );
    m.paint(
        &loft(
            &[
                (f + 2.62, prof(10.4, 8.2, -0.45)),
                (f + 3.4, prof(11.0, 8.6, -0.4)),
            ],
            |_, _| Paint::Red,
            Paint::Red,
        ),
        Mat4::IDENTITY,
        1.0,
        Paint::Red,
    );
    belt(m, f + 7.0, b, W / 2.0);
    buttresses(m, f + 18.0, b, 3.0, W / 2.0);
    pennant(m, 3.5, W / 2.0 + 0.02, -0.4, f + 12.0);

    // The tiers, each set back from the one below.
    let top = H / 2.0;
    let tier = |m: &mut Mesh, z0: f32, z1: f32, w: f32, y0: f32, h: f32| {
        slab(
            m,
            &[
                (z0, prof(w - 1.2, h * 0.4, y0 + h * 0.2)),
                (z0 + 1.2, prof(w, h, y0 + h / 2.0)),
                (z1, prof(w, h, y0 + h / 2.0)),
            ],
            0.0,
        );
    };
    tier(m, c - 6.5, b - 0.8, 11.0, top, 2.4);
    tier(m, c - 4.0, b - 2.2, 9.0, top + 2.4, 2.4);
    let (t0, th) = (top + 4.8, 8.5);
    // The tower: sheer sides, a band of windows near the top.
    let (z0, z1) = (c - 2.0, c + 5.5);
    slab(
        m,
        &[
            (z0, prof(7.0, th, t0 + th / 2.0)),
            (z1, prof(7.0, th, t0 + th / 2.0)),
        ],
        0.0,
    );
    m.paint(
        &loft(
            &[
                (z0 - 0.2, prof(7.4, 2.2, t0 + th - 1.4)),
                (z1 + 0.2, prof(7.4, 2.2, t0 + th - 1.4)),
            ],
            |_, _| Paint::Windows,
            Paint::Windows,
        ),
        Mat4::IDENTITY,
        1.0,
        Paint::Windows,
    );
    // Vertical fins up the tower's face and sides: its strong lines.
    for x in [-2.2f32, -0.75, 0.75, 2.2] {
        bx(
            m,
            Vec3::new(x, t0 + (th - 2.6) / 2.0, z0 - 0.1),
            Vec3::new(0.35, th - 2.6, 0.3),
            0.9,
            Paint::Gunmetal,
        );
    }
    for side in [-1.0f32, 1.0] {
        for z in [z0 + 1.5, z0 + 3.75, z0 + 6.0] {
            bx(
                m,
                Vec3::new(side * 3.6, t0 + (th - 2.6) / 2.0, z),
                Vec3::new(0.3, th - 2.6, 0.35),
                0.9,
                Paint::Gunmetal,
            );
        }
        // Bridge wings.
        bx(
            m,
            Vec3::new(side * 4.7, t0 + th - 2.6, z0 + 0.9),
            Vec3::new(2.4, 0.25, 1.6),
            1.0,
            Paint::Gunmetal,
        );
        bx(
            m,
            Vec3::new(side * 5.8, t0 + th - 1.6, z0 + 0.9),
            Vec3::new(0.35, 1.0, 1.6),
            1.0,
            Paint::Windows,
        );
        searchlight(
            m,
            Vec3::new(side * 2.7, t0 + th + 0.25, z0 + 0.3),
            Vec3::new(side * 0.15, -0.1, -1.0),
            0.4,
        );
        searchlight(
            m,
            Vec3::new(side * 4.8, top + 2.6, c - 5.0),
            Vec3::new(side * 0.3, 0.0, -1.0),
            0.4,
        );
    }
    // A red roof and the mast.
    bx(
        m,
        Vec3::new(0.0, t0 + th, (z0 + z1) / 2.0),
        Vec3::new(7.4, 0.5, z1 - z0 + 0.9),
        1.0,
        Paint::Red,
    );
    let mast = Vec3::new(0.0, t0 + th + 0.25, z1 - 1.5);
    rod(m, mast, mast + Vec3::Y * 5.0, 0.15, Paint::Metal);
    rod(
        m,
        mast + Vec3::new(-2.0, 3.2, 0.0),
        mast + Vec3::new(2.0, 3.2, 0.0),
        0.08,
        Paint::Metal,
    );
    beacon(m, mast + Vec3::Y * 5.1, 0.3);

    for (slot, module, _) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon => {
                turret(
                    m,
                    Vec3::new(0.0, -H / 2.0 + 0.2, f + 8.5),
                    -1.0,
                    k,
                    is_laser(module),
                    f - 0.5 * k,
                );
            }
            SlotKind::Radar => radar(m, Vec3::new(0.0, t0 + th + 0.25, z0 + 2.2), k),
            SlotKind::Shield => shield(m, Vec3::new(0.0, top + 2.2, b - 2.6), k),
            SlotKind::Rcs => rcs(m, f + 7.0, W / 2.0 - 0.6, H / 2.0 - 0.6),
            _ => {}
        }
    }
}

// --- Mid sections -----------------------------------------------------------

/// The gun deck: a turret-lined battle deck. Round barbettes stand out of
/// the flanks for the heavy turrets, another rises from the deck and one
/// hangs under the keel; a row of light twin mounts lines each deck edge
/// and a fire-control director stands amidships.
fn battle_deck(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    hull(m, f, b);
    belt(m, f, b, W / 2.0);
    buttresses(m, c + 0.5, b - 1.0, 3.5, W / 2.0);
    let top = H / 2.0;
    // Deck plating lines and a red deck-edge stripe.
    for side in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(side * (W / 2.0 - 2.6), top + 0.03, c),
            Vec3::new(0.3, 0.06, b - f - 1.0),
            1.0,
            Paint::Red,
        );
        // A row of light twin mounts down each deck edge.
        for i in 0..4 {
            let z = c + 1.5 + i as f32 * 3.2;
            let base = Vec3::new(side * (W / 2.0 - 1.3), top, z);
            rod(m, base, base + Vec3::Y * 0.6, 0.75, Paint::Gunmetal);
            bx(
                m,
                base + Vec3::new(0.0, 0.9, 0.0),
                Vec3::new(1.0, 0.6, 1.2),
                1.0,
                Paint::Gunmetal,
            );
            for dx in [-0.2f32, 0.2] {
                rod(
                    m,
                    base + Vec3::new(dx, 0.95, -0.5),
                    base + Vec3::new(dx, 1.2, -2.6),
                    0.08,
                    Paint::Metal,
                );
            }
        }
    }
    // The director: a stepped block with a rangefinder bar and lamps.
    let d = c + 6.5;
    slab(
        m,
        &[
            (d - 2.0, prof(4.0, 0.8, top + 0.4)),
            (d - 1.2, prof(5.0, 2.0, top + 1.0)),
            (d + 2.5, prof(5.0, 2.0, top + 1.0)),
        ],
        0.0,
    );
    slab(
        m,
        &[
            (d - 0.8, prof(2.6, 1.4, top + 2.7)),
            (d + 1.6, prof(2.6, 1.4, top + 2.7)),
        ],
        0.0,
    );
    bx(
        m,
        Vec3::new(0.0, top + 3.0, d + 0.4),
        Vec3::new(6.5, 0.35, 0.5),
        1.0,
        Paint::Metal,
    );
    bx(
        m,
        Vec3::new(0.0, top + 2.8, d - 0.82),
        Vec3::new(2.2, 0.25, 0.05),
        1.0,
        Paint::Glass,
    );
    for side in [-1.0f32, 1.0] {
        searchlight(
            m,
            Vec3::new(side * 3.3, top + 3.0, d + 0.4),
            Vec3::new(side * 0.3, 0.0, -1.0),
            0.35,
        );
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Weapon if s.y.abs() < 0.5 => {
                // A round barbette built out of the flank.
                let side = s.x.signum();
                let bz = Vec3::new(side * (W / 2.0 + 1.6), 0.0, s.z);
                m.paint(
                    &cylinder(16),
                    strut(bz - Vec3::Y * 3.0, bz + Vec3::Y * 0.9, 2.9),
                    0.95,
                    Paint::Gunmetal,
                );
                m.paint(
                    &lathe(&[(0.0, 2.9), (1.4, 1.2), (1.4, 0.0)], 16, 0.0, TAU),
                    Mat4::from_rotation_translation(
                        Quat::from_rotation_arc(Vec3::Z, Vec3::NEG_Y),
                        bz - Vec3::Y * 3.0,
                    ),
                    0.85,
                    Paint::Gunmetal,
                );
                turret(
                    m,
                    bz + Vec3::Y * 0.9,
                    1.0,
                    k,
                    is_laser(module),
                    s.z - 2.2 * k,
                );
            }
            SlotKind::Weapon => {
                let up = s.y.signum();
                let base = Vec3::new(0.0, up * top, s.z);
                rod(m, base, base + Vec3::Y * up * 0.9, 2.3, Paint::Gunmetal);
                turret(
                    m,
                    base + Vec3::Y * up * 0.9,
                    up,
                    k,
                    is_laser(module),
                    s.z - 2.6 * k,
                );
            }
            SlotKind::Radiator => {
                radiator(m, Vec3::new(0.0, top, s.z), Vec3::Y, 1.0 * k, 1.2 * k);
            }
            _ => {}
        }
    }
}

/// The shield ring: a great ribbed collar standing out round the hull,
/// emitters on its crown and keel, its front face studded with lit ports
/// when the generator is fitted.
fn collar(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    hull(m, f, b);
    belt(m, f, c - 7.0, W / 2.0);
    belt(m, c + 5.0, b, W / 2.0);
    let r = 10.0;
    let (z0, z1) = (c - 6.0, c + 3.5);
    let a0 = TAU / 16.0;
    let powered = p.modules().any(|(slot, _, _)| slot.kind == SlotKind::Power);
    let ring = |m: &mut Mesh, profile: &[(f32, f32)], paint: Paint| {
        m.paint(
            &lathe(profile, 8, a0, a0 + TAU).flat(),
            Mat4::IDENTITY,
            1.0,
            paint,
        );
    };
    ring(
        m,
        &[(z0 - 1.5, 6.6), (z0, r)],
        if powered {
            Paint::Windows
        } else {
            Paint::Gunmetal
        },
    );
    ring(m, &[(z0, r), (z1, r)], Paint::Gunmetal);
    ring(m, &[(z1, r), (z1 + 1.8, 6.6)], Paint::Gunmetal);
    ring(m, &[(c - 1.8, r + 0.05), (c - 0.6, r + 0.05)], Paint::Red);
    // Ribs all round it, standing proud.
    for i in 0..16 {
        let a = i as f32 / 16.0 * TAU;
        let rz = Mat4::from_rotation_z(a);
        m.paint(
            &cube(),
            rz * boxed(
                Vec3::new(r * 0.97 + 0.55, 0.0, (z0 + z1) / 2.0),
                Vec3::new(1.1, 0.55, z1 - z0 + 1.0),
            ),
            0.85,
            Paint::Gunmetal,
        );
    }
    // Lamps on the collar's shoulders, looking forward.
    for side in [-1.0f32, 1.0] {
        let a = TAU / 8.0;
        searchlight(
            m,
            Vec3::new(side * r * a.cos() * 0.98, r * a.sin() * 0.98, z0 - 0.4),
            Vec3::new(side * 0.2, 0.2, -1.0),
            0.45,
        );
        beacon(m, Vec3::new(side * (r * 0.93 + 1.2), 0.0, z1), 0.35);
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Shield => {
                let up = s.y.signum();
                let tip = Vec3::new(0.0, up * (r * 0.93 + 0.9), (z0 + z1) / 2.0);
                bx(m, tip, Vec3::new(3.2, 0.8, 6.0), 0.9, Paint::Metal);
                shield(m, tip + Vec3::Y * up * 0.4, k);
            }
            SlotKind::Radiator => {
                radiator(
                    m,
                    Vec3::new(0.0, H / 2.0, s.z.max(z1 + 3.5)),
                    Vec3::Y,
                    0.9 * k,
                    1.0 * k,
                );
            }
            _ => {}
        }
    }
}

/// The cargo bay: an armoured supply hangar. Thick buttressed walls with
/// great numbered doors stand either side of an open bay, crane gantries
/// span it, and its containers sit in it under work lamps. An empty bay
/// shows its hazard-marked floor.
fn hangar(m: &mut Mesh, p: &Placed, cargo: &mut usize) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let floor = 0.0;
    // The lower hull.
    slab(
        m,
        &[
            (f, prof(W, H / 2.0 + 0.4, -H / 4.0 + 0.2)),
            (b, prof(W, H / 2.0 + 0.4, -H / 4.0 + 0.2)),
        ],
        0.0,
    );
    belt(m, f, b, W / 2.0);
    // The walls.
    let wall_top = H / 2.0 + 1.6;
    let wall = |x: f32| {
        [
            (
                f + 0.5,
                chamfer_rect(1.6, wall_top + 0.2, 0.4, wall_top / 2.0 - 0.1),
            ),
            (
                b - 0.5,
                chamfer_rect(1.6, wall_top + 0.2, 0.4, wall_top / 2.0 - 0.1),
            ),
        ]
        .map(|(z, pts)| {
            (
                z,
                pts.into_iter().map(|p| p + Vec2::X * x).collect::<Vec<_>>(),
            )
        })
    };
    for side in [-1.0f32, 1.0] {
        slab(m, &wall(side * (W / 2.0 - 0.8)), 0.0);
        // Buttresses between the doors.
        for i in 0..4 {
            let z = f + 1.5 + (b - f - 3.0) * i as f32 / 3.0;
            bx(
                m,
                Vec3::new(side * (W / 2.0 + 0.2), wall_top / 2.0, z),
                Vec3::new(0.6, wall_top, 1.2),
                0.9,
                Paint::Gunmetal,
            );
            bx(
                m,
                Vec3::new(side * (W / 2.0 + 0.2), wall_top - 0.15, z),
                Vec3::new(0.65, 0.3, 1.25),
                1.0,
                Paint::Red,
            );
        }
        // Lamps along the wall tops, looking down into the bay.
        for i in 0..3 {
            let z = c - 11.0 + i as f32 * 11.0;
            searchlight(
                m,
                Vec3::new(side * (W / 2.0 - 1.4), wall_top + 0.2, z + 4.0),
                Vec3::new(-side, -0.6, 0.0),
                0.3,
            );
        }
    }
    // End bulkheads with a red band.
    for (z, dir) in [(f + 0.6, 1.0f32), (b - 0.6, -1.0)] {
        bx(
            m,
            Vec3::new(0.0, wall_top / 2.0, z),
            Vec3::new(W - 3.2, wall_top, 1.2),
            0.95,
            Paint::Gunmetal,
        );
        bx(
            m,
            Vec3::new(0.0, wall_top - 0.8, z - dir * 0.62),
            Vec3::new(W - 3.2, 0.5, 0.05),
            1.0,
            Paint::Red,
        );
    }
    // Crane gantries spanning the bay, each with a trolley and a hook.
    for z in [c - 5.5, c + 5.5] {
        bx(
            m,
            Vec3::new(0.0, wall_top + 0.45, z),
            Vec3::new(W + 0.4, 0.9, 1.0),
            1.0,
            Paint::Hazard,
        );
        let trolley = Vec3::new(1.4, wall_top - 0.2, z);
        bx(m, trolley, Vec3::new(1.4, 0.6, 1.4), 1.0, Paint::Metal);
        rod(m, trolley, trolley - Vec3::Y * 2.0, 0.05, Paint::Metal);
        beacon(m, Vec3::new(W / 2.0 + 0.1, wall_top + 1.0, z), 0.3);
        beacon(m, Vec3::new(-W / 2.0 - 0.1, wall_top + 1.0, z), 0.3);
    }

    // Each slot is a bay with its own door in each wall, numbered.
    let slots: Vec<f32> = p
        .section
        .slots
        .iter()
        .map(|s| c - s.position_m[2] as f32)
        .collect();
    for (i, &z) in slots.iter().enumerate() {
        for side in [-1.0f32, 1.0] {
            bx(
                m,
                Vec3::new(side * (W / 2.0 + 0.02), 2.6, z),
                Vec3::new(0.08, 4.4, 7.0),
                0.8,
                Paint::Gunmetal,
            );
            bx(
                m,
                Vec3::new(side * (W / 2.0 + 0.04), 0.3, z),
                Vec3::new(0.1, 0.35, 7.0),
                1.0,
                Paint::Hazard,
            );
            let h = 2.4;
            let text = format!("{}", i + 1);
            let origin = Vec3::new(side * (W / 2.0 + 0.07), 1.6, z + side * 0.66);
            let surface = flat(origin, Vec3::new(0.0, 0.0, -side), Vec3::Y);
            stencil(m, &text, h, Paint::Hull, &surface);
        }
    }
    let mut filled = vec![false; slots.len()];
    for (slot, module, s) in p.modules() {
        if slot.kind != SlotKind::Cargo {
            continue;
        }
        if let Some(i) = slots.iter().position(|&z| (z - s.z).abs() < 0.01) {
            filled[i] = true;
        }
        let k = scale(module.size) / 2.0;
        let size = Vec3::new(8.4, 6.0 * k.max(0.6), 9.4);
        let paint = [Paint::Olive, Paint::Red, Paint::Hull][*cargo % 3];
        *cargo += 1;
        container(m, Vec3::new(0.0, floor + size.y / 2.0, s.z), size, paint);
    }
    for (i, &z) in slots.iter().enumerate() {
        if filled[i] {
            continue;
        }
        bx(
            m,
            Vec3::new(0.0, floor + 0.03, z),
            Vec3::new(7.0, 0.06, 1.2),
            1.0,
            Paint::Hazard,
        );
        for (dx, dz) in [(-3.6f32, -4.0f32), (3.6, -4.0), (-3.6, 4.0), (3.6, 4.0)] {
            bx(
                m,
                Vec3::new(dx, floor + 0.2, z + dz),
                Vec3::new(0.6, 0.4, 0.6),
                1.0,
                Paint::Metal,
            );
        }
    }
}

/// The reactor section: a fortified drum. A great faceted drum, much
/// fatter than the hull, ribbed like a fortress tower, hazard-banded at
/// its ends; each power plant glows through a band of vents, and the
/// radiators spread out from its flanks like wings.
fn drum(m: &mut Mesh, p: &Placed) {
    let (f, b, c) = (p.front, p.back, p.centre);
    hull(m, f, b);
    belt(m, f, c - 8.5, W / 2.0);
    belt(m, c + 8.5, b, W / 2.0);
    let r = 8.8;
    let a0 = TAU / 24.0;
    let ring = |m: &mut Mesh, profile: &[(f32, f32)], paint: Paint| {
        m.paint(
            &lathe(profile, 12, a0, a0 + TAU).flat(),
            Mat4::IDENTITY,
            1.0,
            paint,
        );
    };
    ring(
        m,
        &[(c - 8.6, 7.0), (c - 7.6, r), (c + 7.6, r), (c + 8.6, 7.0)],
        Paint::Gunmetal,
    );
    for z in [c - 7.0, c + 6.5] {
        ring(m, &[(z, r + 0.04), (z + 0.5, r + 0.04)], Paint::Hazard);
    }
    ring(m, &[(c - 0.4, r + 0.04), (c + 0.4, r + 0.04)], Paint::Red);
    // Ribs along every edge.
    for i in 0..12 {
        let a = i as f32 / 12.0 * TAU;
        m.paint(
            &cube(),
            Mat4::from_rotation_z(a)
                * boxed(Vec3::new(r * 0.99 + 0.3, 0.0, c), Vec3::new(0.7, 0.8, 15.0)),
            0.85,
            Paint::Gunmetal,
        );
    }
    // A crown of lamps and beacons.
    for side in [-1.0f32, 1.0] {
        searchlight(
            m,
            Vec3::new(side * 2.4, r + 0.3, c - 7.8),
            Vec3::new(side * 0.2, 0.3, -1.0),
            0.4,
        );
        beacon(m, Vec3::new(side * 2.4, r + 0.5, c + 7.5), 0.3);
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Power => {
                ring(
                    m,
                    &[(s.z - 0.9, r + 0.02), (s.z + 0.9, r + 0.02)],
                    Paint::Glass,
                );
                for dz in [-0.45f32, 0.0, 0.45] {
                    ring(
                        m,
                        &[(s.z + dz - 0.08, r + 0.07), (s.z + dz + 0.08, r + 0.07)],
                        Paint::Metal,
                    );
                }
            }
            SlotKind::Radiator => {
                let side = s.x.signum();
                bx(
                    m,
                    Vec3::new(side * (r + 0.8), 0.0, c),
                    Vec3::new(1.6, 1.6, 6.0),
                    0.9,
                    Paint::Metal,
                );
                radiator(
                    m,
                    Vec3::new(side * (r + 1.4), 0.0, c),
                    Vec3::new(side, 0.0, 0.0),
                    1.6 * k,
                    2.4 * k,
                );
            }
            _ => {}
        }
    }
}

// --- Sterns -----------------------------------------------------------------

/// The sprint stern: twin huge bells side by side, set deep in a heavy
/// armoured frame that runs round them like a picture frame, red-banded
/// at its rim; radiators stand up off the top and hang off the keel.
fn sprint_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let (fz0, fz1) = (c + 4.5, b - 1.0);
    slab(
        m,
        &[
            (f, prof(W, H, 0.0)),
            (fz0, prof(W, H, 0.0)),
            (fz0 + 2.5, prof(W - 2.0, H - 2.0, 0.0)),
        ],
        0.0,
    );
    belt(m, f, fz0, W / 2.0);
    buttresses(m, f + 1.0, fz0, 3.5, W / 2.0);
    // The frame: slabs above, below and either side of the bells.
    let (ow, oh, t) = (19.0f32, 11.6f32, 2.0f32);
    let fm = (fz0 + fz1) / 2.0;
    let fl = fz1 - fz0;
    for sy in [-1.0f32, 1.0] {
        slab(
            m,
            &[
                (fz0, prof(ow - 2.0, t, sy * (oh / 2.0 - t / 2.0))),
                (fz0 + 1.5, prof(ow, t, sy * (oh / 2.0 - t / 2.0))),
                (fz1, prof(ow, t, sy * (oh / 2.0 - t / 2.0))),
            ],
            0.0,
        );
    }
    for sx in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(sx * (ow / 2.0 - t / 2.0), 0.0, fm + 0.75),
            Vec3::new(t, oh - 2.0 * t + 0.2, fl - 1.5),
            1.0,
            Paint::Gunmetal,
        );
        // Wedges back to the hull.
        m.paint(
            &cube(),
            Mat4::from_scale_rotation_translation(
                Vec3::new(1.0, oh - 2.0 * t, 4.5),
                Quat::from_rotation_y(sx * 0.5),
                Vec3::new(sx * (W / 2.0 + 1.0), 0.0, fz0 + 0.5),
            ),
            0.9,
            Paint::Gunmetal,
        );
    }
    // The rim: a red band round the back of the frame.
    for sy in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(0.0, sy * (oh / 2.0 + 0.02), fz1 - 0.6),
            Vec3::new(ow - 0.6, 0.06, 0.9),
            1.0,
            Paint::Red,
        );
    }
    for sx in [-1.0f32, 1.0] {
        bx(
            m,
            Vec3::new(sx * (ow / 2.0 + 0.02), 0.0, fz1 - 0.6),
            Vec3::new(0.06, oh - 1.0, 0.9),
            1.0,
            Paint::Red,
        );
        for sy in [-1.0f32, 1.0] {
            beacon(m, Vec3::new(sx * ow / 2.0, sy * oh / 2.0, fz1), 0.35);
        }
        searchlight(
            m,
            Vec3::new(sx * (ow / 2.0 - 1.0), oh / 2.0 + 0.4, fz0 + 2.0),
            Vec3::new(sx * 0.3, 0.2, -1.0),
            0.4,
        );
    }
    // The thrust plate the bells hang from.
    bx(
        m,
        Vec3::new(0.0, 0.0, fz0 + 2.6),
        Vec3::new(ow - 2.0 * t, oh - 2.0 * t, 0.4),
        0.6,
        Paint::Metal,
    );

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(s.x, 0.0, fz0 + 4.0);
                rod(m, throat - Vec3::Z * 1.4, throat, 1.5, Paint::Metal);
                m.paint(
                    &lathe(&[(-1.4, 1.9), (-1.0, 2.1), (-0.6, 1.9)], 16, 0.0, TAU),
                    Mat4::from_translation(throat),
                    1.0,
                    Paint::Gunmetal,
                );
                nozzles.push(bell(m, throat, k, 1.65));
            }
            SlotKind::Radiator => {
                let up = s.y.signum();
                radiator(
                    m,
                    Vec3::new(0.0, up * H / 2.0, s.z.min(fz0 - 1.0)),
                    Vec3::Y * up,
                    1.0 * k,
                    1.2 * k,
                );
            }
            SlotKind::Power => {
                // An armoured vent house on the deck.
                let v = Vec3::new(0.0, H / 2.0 + 0.7, s.z - 3.0);
                bx(m, v, Vec3::new(5.0, 1.4, 3.0), 0.95, Paint::Gunmetal);
                for i in 0..5 {
                    bx(
                        m,
                        v + Vec3::new(-2.0 + i as f32, 0.72, 0.0),
                        Vec3::new(0.5, 0.06, 2.6),
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            _ => {}
        }
    }
}

/// The cruise stern: the hull steps down in two great terraces to a single
/// huge bell set in a thick octagonal armoured collar with a red rim;
/// radiators stand out from the lower terrace's flanks.
fn cruise_stern(m: &mut Mesh, p: &Placed, nozzles: &mut Vec<(Vec3, f32)>) {
    let (f, b, c) = (p.front, p.back, p.centre);
    let (s1, s2, s3) = (c + 1.0, c + 5.0, c + 7.5);
    let a = prof(W, H, 0.0);
    let bb = prof(W - 3.0, H - 2.4, -0.2);
    let cc = prof(W - 5.5, H - 4.0, -0.2);
    slab(
        m,
        &[
            (f, a.clone()),
            (s1, a),
            (s1, bb.clone()),
            (s2, bb),
            (s2, cc.clone()),
            (s3, cc),
        ],
        0.0,
    );
    belt(m, f, s1, W / 2.0);
    belt(m, s1, s2, W / 2.0 - 1.5);
    buttresses(m, f + 1.0, s1, 3.5, W / 2.0);
    // Red stripes along the terrace edges.
    for (z, w, y) in [(s1, W - 3.0, H / 2.0 - 1.2), (s2, W - 5.5, H / 2.0 - 2.2)] {
        bx(
            m,
            Vec3::new(0.0, y + 0.02, z + 0.3),
            Vec3::new(w * 0.6, 0.06, 0.6),
            1.0,
            Paint::Red,
        );
    }
    for sx in [-1.0f32, 1.0] {
        searchlight(
            m,
            Vec3::new(sx * (W / 2.0 - 1.0), H / 2.0 + 0.3, s1 - 0.6),
            Vec3::new(sx * 0.3, 0.3, 1.0),
            0.4,
        );
        beacon(
            m,
            Vec3::new(sx * (W / 2.0 - 1.5), H / 2.0 - 1.0, s2 - 0.3),
            0.3,
        );
    }

    for (slot, module, s) in p.modules() {
        let k = km(module);
        match slot.kind {
            SlotKind::Engine => {
                // The collar, thick and octagonal.
                let a0 = TAU / 16.0;
                let collar = [
                    (s3 - 0.5, 3.4),
                    (s3, 6.6),
                    (b - 0.5, 6.6),
                    (b, 6.1),
                    (b, 5.5),
                    (s3 + 0.6, 3.6),
                ];
                m.paint(
                    &lathe(&collar, 8, a0, a0 + TAU).flat(),
                    Mat4::from_translation(Vec3::new(s.x, -0.2, 0.0)),
                    1.0,
                    Paint::Gunmetal,
                );
                m.paint(
                    &lathe(&[(b - 1.6, 6.65), (b - 0.7, 6.65)], 8, a0, a0 + TAU).flat(),
                    Mat4::from_translation(Vec3::new(s.x, -0.2, 0.0)),
                    1.0,
                    Paint::Red,
                );
                let throat = Vec3::new(s.x, -0.2, s3 + 0.4);
                nozzles.push(bell(m, throat, k * 1.3, 2.9));
            }
            SlotKind::Radiator => {
                let side = s.x.signum();
                radiator(
                    m,
                    Vec3::new(side * (W / 2.0 - 1.5), 0.5, s.z.min(s2 - 2.5)),
                    Vec3::new(side, 0.3, 0.0),
                    1.3 * k,
                    0.9 * k,
                );
            }
            SlotKind::Power => {
                let v = Vec3::new(0.0, H / 2.0 + 0.7, s.z - 4.0);
                bx(m, v, Vec3::new(5.0, 1.4, 3.0), 0.95, Paint::Gunmetal);
                for i in 0..5 {
                    bx(
                        m,
                        v + Vec3::new(-2.0 + i as f32, 0.72, 0.0),
                        Vec3::new(0.5, 0.06, 2.6),
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            _ => {}
        }
    }
}
