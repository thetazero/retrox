//! The bulk freighter: the biggest, plainest ship in the yards. An endless
//! oxide-red box spine stacked with containers, a cream bridge block at
//! the bow like an ore carrier's, a squat engine house aft. Hazard
//! striping, stencilled hull numbers, work lamps and floodlights down the
//! spine, beacons on every corner. Monumental only by length and
//! repetition.
//!
//! Every section it can mount has its own freighter rendition, drawn round
//! whichever modules are fitted, so empty slots show bare structure.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat4, Quat, Vec2, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::*;
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, icosphere, lathe, loft, strut,
    torus,
};

/// Kit modules are sized for small ships; the freighter's are this much
/// bigger.
const KIT: f32 = 3.0;
/// The spine's half width and half height.
const SPINE: Vec2 = Vec2::new(2.6, 2.1);
/// The plated hull's half width and half height (bows, gun deck).
const HULL: Vec2 = Vec2::new(7.5, 4.25);
/// Her number, stencilled big.
const NUMBER: &str = "4471";
/// Oxide red: the signal red, weathered down.
const OXIDE: f32 = 0.45;

struct Freighter {
    m: Mesh,
    nozzles: Vec<(Vec3, f32)>,
    cyl: Mesh,
    cube: Mesh,
    ball: Mesh,
    /// Which section this is, for varying the repeats.
    index: usize,
}

impl Freighter {
    fn block(&mut self, c: Vec3, size: Vec3, shade: f32, paint: Paint) {
        self.m.paint(&self.cube, boxed(c, size), shade, paint);
    }

    fn rod(&mut self, a: Vec3, b: Vec3, r: f32, shade: f32, paint: Paint) {
        self.m.paint(&self.cyl, strut(a, b, r), shade, paint);
    }

    /// A square beam between two points (axis-aligned ones only).
    fn beam(&mut self, a: Vec3, b: Vec3, t: f32, shade: f32, paint: Paint) {
        let size = (b - a).abs().max(Vec3::splat(t));
        self.block((a + b) / 2.0, size, shade, paint);
    }

    fn beacon(&mut self, c: Vec3) {
        self.m.paint(&self.ball, at(c, 0.35), 1.0, Paint::Beacon);
    }

    /// A work lamp on a short bracket, shining from `c`.
    fn lamp(&mut self, c: Vec3, out: Vec3) {
        self.block(c - out * 0.3, Vec3::splat(0.35), 1.0, Paint::Metal);
        self.block(c, Vec3::new(0.7, 0.45, 0.7), 1.0, Paint::Lamp);
    }

    /// A ring round the Z axis at `z`.
    fn hoop(&mut self, z: f32, radius: f32, thick: f32, paint: Paint) {
        self.m.paint(
            &torus(thick / radius, 48, 6),
            Mat4::from_translation(Vec3::Z * z)
                * Mat4::from_rotation_x(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(radius)),
            1.0,
            paint,
        );
    }

    /// The box spine from `z0` to `z1`: oxide red, framed every few
    /// metres, a hazard band at its forward joint, a catwalk rail on top.
    fn spine(&mut self, z0: f32, z1: f32) {
        let (w, h) = (SPINE.x, SPINE.y);
        self.block(
            Vec3::Z * (z0 + z1) / 2.0,
            Vec3::new(w * 2.0, h * 2.0, z1 - z0),
            OXIDE,
            Paint::Red,
        );
        let n = ((z1 - z0) / 4.0).round().max(1.0) as usize;
        for i in 0..=n {
            let z = z0 + (z1 - z0) * i as f32 / n as f32;
            self.block(
                Vec3::Z * z,
                Vec3::new(w * 2.0 + 0.3, h * 2.0 + 0.3, 0.35),
                1.0,
                Paint::Metal,
            );
        }
        self.block(
            Vec3::Z * (z0 + 0.6),
            Vec3::new(w * 2.0 + 0.2, h * 2.0 + 0.2, 1.0),
            1.0,
            Paint::Hazard,
        );
        for side in [-1.0f32, 1.0] {
            self.rod(
                Vec3::new(side * (w - 0.3), h + 1.0, z0),
                Vec3::new(side * (w - 0.3), h + 1.0, z1),
                0.06,
                1.0,
                Paint::Ochre,
            );
        }
    }

    /// A slab of plated hull: a chamfered box lofted through `sections`,
    /// oxide red sides and bottom, a dark deck.
    fn hull(&mut self, sections: &[(f32, Vec<Vec2>)]) {
        let paint = |_: usize, e: usize| match e {
            0 => Paint::Metal,
            _ => Paint::Red,
        };
        self.m
            .add(&loft(sections, paint, Paint::Red), Mat4::IDENTITY, OXIDE);
        // Patched plating: replacement plates in a different red here and
        // there along the full-width run.
        let full: Vec<f32> = sections
            .iter()
            .filter(|(_, pts)| pts.iter().any(|q| q.x >= HULL.x - 0.01))
            .map(|(z, _)| *z)
            .collect();
        let (Some(&a), Some(&b)) = (full.first(), full.last()) else {
            return;
        };
        let n = ((b - a) / 3.0) as usize;
        for i in 0..n {
            for (j, side) in [-1.0f32, 1.0].into_iter().enumerate() {
                let h = hash(self.index, i, j);
                if !h.is_multiple_of(3) {
                    continue;
                }
                let z = a + 1.5 + i as f32 * 3.0;
                let y = -2.4 + (h >> 8) as f32 % 4.0;
                let shade = [0.32, 0.55, 0.38][(h >> 3) as usize % 3];
                self.block(
                    Vec3::new(side * (HULL.x + 0.04), y, z),
                    Vec3::new(0.06, 1.6 + (h >> 5) as f32 % 2.0, 2.6),
                    shade,
                    Paint::Red,
                );
            }
        }
    }

    /// Plate seams and frames on a plated hull: thin dark ribs round it.
    fn frames(&mut self, z0: f32, z1: f32, half: Vec2, step: f32) {
        let n = ((z1 - z0) / step).round().max(1.0) as usize;
        for i in 1..n {
            let z = z0 + (z1 - z0) * i as f32 / n as f32;
            for side in [-1.0f32, 1.0] {
                self.block(
                    Vec3::new(side * (half.x + 0.05), 0.0, z),
                    Vec3::new(0.2, half.y * 1.6, 0.3),
                    1.0,
                    Paint::Metal,
                );
            }
        }
    }

    /// Her number on both flanks at `z` (the start of the text), on a
    /// surface `x` out from the centreline.
    fn number(&mut self, x: f32, y: f32, z: f32, h: f32) {
        let w = NUMBER.len() as f32 * 0.85 * h;
        for side in [-1.0f32, 1.0] {
            let z0 = if side > 0.0 { z + w } else { z };
            let surface = flat(
                Vec3::new(side * x, y, z0),
                Vec3::new(0.0, 0.0, -side),
                Vec3::Y,
            );
            let mut text = Mesh::default();
            stencil(&mut text, NUMBER, h, Paint::Hull, &surface);
            self.m.add(&text, Mat4::IDENTITY, 0.95);
        }
    }

    /// A gun in a turret at `c`, its muzzle forward at `muzzle_z`.
    fn turret(&mut self, c: Vec3, up: f32, muzzle_z: f32, module: &Module, k: f32) {
        let s = k / 2.0;
        self.block(c, Vec3::new(3.4, 1.6, 3.8) * s, 0.9, Paint::Ochre);
        self.block(
            c + Vec3::new(0.0, up * 0.85 * s, 0.4 * s),
            Vec3::new(2.6, 0.3, 2.6) * s,
            1.0,
            Paint::Metal,
        );
        self.block(
            c + Vec3::new(0.0, 0.0, -1.95 * s),
            Vec3::new(2.2, 1.2, 0.2) * s,
            1.0,
            Paint::Hazard,
        );
        gun(
            &mut self.m,
            c + Vec3::Z * -1.5 * s,
            muzzle_z,
            is_laser(module),
            k * KIT * 0.9,
        );
    }

    /// The freighter's own radiator: a big grimy-white plate standing from
    /// `root` along `out`, chunky coolant headers, a mustard frame, a
    /// hazard strip at the root.
    fn radiator(&mut self, root: Vec3, out: Vec3, reach: f32, len: f32) {
        let out = out.normalize();
        let rot = Quat::from_rotation_arc(Vec3::Y, out);
        let along = rot * Vec3::Z;
        let piece = |f: &mut Freighter, size: Vec3, c: Vec3, shade: f32, p: Paint| {
            f.m.paint(
                &f.cube,
                Mat4::from_scale_rotation_translation(size, rot, c),
                shade,
                p,
            );
        };
        let mid = root + out * reach / 2.0;
        piece(self, Vec3::new(0.15, reach, len), mid, 0.85, Paint::Hull);
        let ribs = (len / 1.4).round() as i32;
        for i in 0..ribs {
            let z = -len / 2.0 + len * (i as f32 + 0.5) / ribs as f32;
            piece(
                self,
                Vec3::new(0.35, reach * 0.94, 0.22),
                mid + along * z,
                1.0,
                Paint::Metal,
            );
        }
        for e in [-0.5f32, 0.5] {
            piece(
                self,
                Vec3::new(0.5, reach, 0.5),
                mid + along * len * e,
                1.0,
                Paint::Ochre,
            );
        }
        piece(
            self,
            Vec3::new(0.5, 0.6, len),
            root + out * (reach - 0.3),
            1.0,
            Paint::Ochre,
        );
        piece(
            self,
            Vec3::new(0.6, 1.2, len),
            root + out * 0.6,
            1.0,
            Paint::Hazard,
        );
    }
}

pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut f = Freighter {
        m: Mesh::default(),
        nozzles: Vec::new(),
        cyl: cylinder(12),
        cube: cube(),
        ball: icosphere(1),
        index: 0,
    };
    let sections = placed(fit);
    for (i, p) in sections.iter().enumerate() {
        f.index = i;
        match p.section.id {
            "command-bow-l" => command_bow(&mut f, p),
            "gun-bow-l" => gun_bow(&mut f, p),
            "gun-deck-l" => gun_deck(&mut f, p),
            "shield-ring-l" => shield_ring(&mut f, p),
            "cargo-bay-l" => cargo_bay(&mut f, p),
            "reactor-section-l" => reactor_section(&mut f, p),
            "sprint-stern-l" => sprint_stern(&mut f, p),
            "cruise-stern-l" => cruise_stern(&mut f, p),
            _ => match p.section.position {
                Position::Bow => command_bow(&mut f, p),
                Position::Stern => cruise_stern(&mut f, p),
                _ => cargo_bay(&mut f, p),
            },
        }
    }
    if let Some(last) = sections.last() {
        soot(&mut f.m, last.back - 22.0, last.back + 4.0, 0.5);
    }
    (f.m, ShipShape { nozzles: f.nozzles })
}

/// A cheap, stable hash for varying repeats.
fn hash(a: usize, b: usize, c: usize) -> u32 {
    let mut h = (a as u32).wrapping_mul(0x9E37_79B1) ^ (b as u32).wrapping_mul(0x85EB_CA77);
    h ^= (c as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

// --- Bows -----------------------------------------------------------------

/// Command bow: a blunt raked barge bow, a tall cream bridge block on its
/// after deck with wings out over the sides, a mast on top.
fn command_bow(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    let (w, h) = (HULL.x * 2.0, HULL.y * 2.0);
    let deck = HULL.y;
    // The scow bow: the bottom rakes up to a short, blunt front face.
    f.hull(&[
        (z0, chamfer_rect(w - 2.0, 3.6, 0.8, deck - 1.8)),
        (z0 + 3.0, chamfer_rect(w - 0.8, 6.0, 1.2, deck - 3.0)),
        (z0 + 8.0, chamfer_rect(w, h, 1.5, 0.0)),
        (z1, chamfer_rect(w, h, 1.5, 0.0)),
    ]);
    f.frames(z0 + 8.0, z1, HULL, 4.2);
    // A rubbing strake and a hazard-striped bumper across the face.
    f.block(
        Vec3::new(0.0, deck - 1.8, z0 - 0.35),
        Vec3::new(w - 2.4, 1.6, 0.7),
        1.0,
        Paint::Hazard,
    );
    for side in [-1.0f32, 1.0] {
        f.block(
            Vec3::new(side * (HULL.x + 0.15), deck - 1.2, (z0 + z1) / 2.0 + 2.0),
            Vec3::new(0.4, 0.5, z1 - z0 - 6.0),
            1.0,
            Paint::Ochre,
        );
        // Anchor-like docking clamps on the bow corners.
        f.block(
            Vec3::new(side * (HULL.x - 1.4), deck - 3.0, z0 + 2.2),
            Vec3::new(1.4, 1.4, 1.4),
            1.0,
            Paint::Metal,
        );
        f.beacon(Vec3::new(side * (HULL.x - 1.2), deck + 0.4, z0 + 0.4));
        f.lamp(Vec3::new(side * 2.5, deck - 0.8, z0 - 0.2), Vec3::NEG_Z);
    }
    f.number(HULL.x + 0.05, -1.4, z0 + 9.0, 3.2);

    // The bridge block: storeys of lit windows, stepping in as they rise.
    let (bz0, bz1) = (z1 - 11.0, z1 - 2.5);
    let storey = 2.8;
    for i in 0..4 {
        let y = deck + storey * (i as f32 + 0.5);
        let inset = i as f32 * 0.35;
        let width = 10.0 - inset * 2.0;
        let len = bz1 - bz0 - inset;
        let cz = (bz0 + bz1) / 2.0 + inset / 2.0;
        f.block(
            Vec3::new(0.0, y, cz),
            Vec3::new(width, storey, len),
            1.0,
            Paint::Windows,
        );
        // A deck plate overhanging each storey.
        f.block(
            Vec3::new(0.0, y + storey / 2.0, cz),
            Vec3::new(width + 0.6, 0.22, len + 0.6),
            1.0,
            Paint::Hull,
        );
    }
    // The wheelhouse on top, wider, glazed all along its front, with
    // bridge wings out over the sides.
    let wy = deck + storey * 4.0 + 1.4;
    let wz = bz0 + 1.6;
    f.block(
        Vec3::new(0.0, wy, wz + 2.0),
        Vec3::new(12.0, 2.8, 6.0),
        1.0,
        Paint::Hull,
    );
    f.block(
        Vec3::new(0.0, wy + 0.2, wz - 1.02),
        Vec3::new(11.6, 1.3, 0.1),
        1.0,
        Paint::Glass,
    );
    for side in [-1.0f32, 1.0] {
        f.block(
            Vec3::new(side * 9.0, wy + 0.3, wz + 0.4),
            Vec3::new(6.0, 2.0, 2.8),
            1.0,
            Paint::Hull,
        );
        f.block(
            Vec3::new(side * 9.0, wy + 0.5, wz - 1.02),
            Vec3::new(5.6, 1.0, 0.1),
            1.0,
            Paint::Glass,
        );
        f.block(
            Vec3::new(side * 6.06, wy + 0.2, wz + 2.0),
            Vec3::new(0.1, 1.2, 4.6),
            1.0,
            Paint::Glass,
        );
        f.beacon(Vec3::new(side * 12.0, wy + 1.0, wz + 0.4));
        // Floodlights on the bridge's back, looking aft down the spine.
        f.lamp(Vec3::new(side * 3.5, wy + 1.6, wz + 5.1), Vec3::Z);
    }
    f.block(
        Vec3::new(0.0, wy + 1.5, wz + 2.0),
        Vec3::new(12.6, 0.3, 6.6),
        0.8,
        Paint::Red,
    );
    // The mast, and a vent stack behind the bridge.
    let top = Vec3::new(0.0, wy + 1.6, wz + 3.5);
    f.rod(top, top + Vec3::Y * 7.0, 0.3, 1.0, Paint::Metal);
    f.beam(
        top + Vec3::new(-3.0, 5.0, 0.0),
        top + Vec3::new(3.0, 5.0, 0.0),
        0.25,
        1.0,
        Paint::Metal,
    );
    f.beacon(top + Vec3::Y * 7.3);
    let stack = Vec3::new(0.0, deck, bz1 + 1.4);
    f.rod(stack, stack + Vec3::Y * 9.0, 1.3, OXIDE, Paint::Red);
    f.rod(
        stack + Vec3::Y * 8.0,
        stack + Vec3::Y * 9.1,
        1.35,
        1.0,
        Paint::Metal,
    );
    f.rod(
        stack + Vec3::Y * 6.0,
        stack + Vec3::Y * 7.0,
        1.35,
        1.0,
        Paint::Ochre,
    );

    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => {
                // A chin turret under the rake.
                let c = Vec3::new(s.x, deck - 4.6, z0 + 5.5);
                f.turret(c, -1.0, z0 - 3.0 - 2.0 * k, module, k);
            }
            SlotKind::Radar => radar(&mut f.m, top + Vec3::new(0.0, 0.0, -2.0), k * KIT),
            SlotKind::Shield => {
                // On a plinth on the foredeck.
                let c = Vec3::new(0.0, deck + 0.8, z0 + 9.0);
                f.block(c, Vec3::new(4.0, 1.6, 4.0), 1.0, Paint::Ochre);
                shield(&mut f.m, c + Vec3::Y * 0.9, k * KIT);
            }
            SlotKind::Rcs => {
                for side in [-1.0f32, 1.0] {
                    for y in [deck - 0.6, -deck + 0.6] {
                        let c = Vec3::new(side * (HULL.x + 0.5), y, z1 - 5.0);
                        f.block(c, Vec3::new(1.0, 1.4, 2.2), 1.0, Paint::Metal);
                        f.rod(
                            c,
                            c + Vec3::new(side * 0.9, 0.0, 0.0),
                            0.35,
                            1.0,
                            Paint::Metal,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Gun bow: a towboat's square pushing bow with hazard-striped push
/// knees, a low armoured pilot house, and gun sponsons on the flanks.
fn gun_bow(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    let (w, h) = (HULL.x * 2.0, HULL.y * 2.0);
    let deck = HULL.y;
    f.hull(&[
        (z0, chamfer_rect(w - 1.0, h - 1.0, 2.8, 0.0)),
        (z0 + 2.5, chamfer_rect(w, h, 1.5, 0.0)),
        (z1, chamfer_rect(w, h, 1.5, 0.0)),
    ]);
    f.frames(z0 + 2.5, z1, HULL, 3.6);
    // Push knees: tall posts on the face, a fender beam across them.
    for side in [-1.0f32, 1.0] {
        f.block(
            Vec3::new(side * 3.6, 0.6, z0 - 0.7),
            Vec3::new(1.4, h + 1.6, 1.4),
            1.0,
            Paint::Hazard,
        );
        f.block(
            Vec3::new(side * 3.6, deck + 1.6, z0 - 0.7),
            Vec3::new(0.9, 0.4, 0.9),
            1.0,
            Paint::Beacon,
        );
    }
    f.rod(
        Vec3::new(-HULL.x + 1.0, -deck + 1.5, z0 - 0.8),
        Vec3::new(HULL.x - 1.0, -deck + 1.5, z0 - 0.8),
        0.6,
        1.0,
        Paint::Metal,
    );
    // The pilot house: low, armoured, slit windows.
    let pz = z0 + 7.0;
    let house = [
        (pz, chamfer_rect(7.0, 1.8, 0.6, deck + 1.0)),
        (pz + 1.6, chamfer_rect(9.0, 3.6, 0.9, deck + 1.8)),
        (pz + 8.0, chamfer_rect(9.0, 3.6, 0.9, deck + 1.8)),
        (pz + 9.0, chamfer_rect(8.0, 3.0, 0.8, deck + 1.5)),
    ];
    let slits = |s: usize, e: usize| match (s, e) {
        (0, 0 | 1 | 7) => Paint::Glass,
        _ => Paint::Ochre,
    };
    f.m.add(&loft(&house, slits, Paint::Ochre), Mat4::IDENTITY, 0.85);
    f.block(
        Vec3::new(0.0, deck + 3.75, pz + 5.0),
        Vec3::new(7.0, 0.3, 5.0),
        1.0,
        Paint::Metal,
    );
    f.number(HULL.x + 0.05, -2.0, z0 + 13.0, 2.6);
    for side in [-1.0f32, 1.0] {
        f.lamp(Vec3::new(side * 4.6, deck + 3.4, pz + 8.6), Vec3::Z);
        f.beacon(Vec3::new(side * (HULL.x - 0.4), deck + 0.4, z1 - 1.0));
    }

    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => {
                // A sponson casemate bolted on the flank.
                let side = s.x.signum();
                let c = Vec3::new(side * (HULL.x + 1.4), s.y, z0 + 6.0);
                f.block(c, Vec3::new(3.0, 4.0, 7.0), 0.9, Paint::Ochre);
                f.block(
                    c + Vec3::new(0.0, -2.1, 0.0),
                    Vec3::new(3.2, 0.4, 7.2),
                    1.0,
                    Paint::Hazard,
                );
                gun(
                    &mut f.m,
                    c + Vec3::new(0.0, 0.3, -3.2),
                    z0 - 2.0 - 1.6 * k,
                    is_laser(module),
                    k * KIT,
                );
            }
            SlotKind::Radar => radar(&mut f.m, Vec3::new(0.0, deck + 3.9, pz + 6.5), k * KIT),
            SlotKind::Shield => {
                let c = Vec3::new(0.0, deck + 0.6, z1 - 5.0);
                f.block(c, Vec3::new(3.6, 1.2, 3.6), 1.0, Paint::Ochre);
                shield(&mut f.m, c + Vec3::Y * 0.7, k * KIT);
            }
            SlotKind::Rcs => {
                for side in [-1.0f32, 1.0] {
                    let c = Vec3::new(side * (HULL.x + 0.5), deck - 1.0, z1 - 4.0);
                    f.block(c, Vec3::new(1.0, 1.4, 2.2), 1.0, Paint::Metal);
                }
            }
            _ => {}
        }
    }
}

// --- Mid sections ---------------------------------------------------------

/// Gun deck: an armed merchant's section. Plated hull, casemates bolted
/// to the flanks, turrets on pedestals above and below, radiators aft.
fn gun_deck(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    let (w, h) = (HULL.x * 2.0, HULL.y * 2.0);
    f.hull(&[
        (z0, chamfer_rect(w, h, 1.5, 0.0)),
        (z1, chamfer_rect(w, h, 1.5, 0.0)),
    ]);
    f.frames(z0, z1, HULL, 3.75);
    f.block(
        Vec3::Z * (z0 + 0.5),
        Vec3::new(w + 0.3, h + 0.3, 1.0),
        1.0,
        Paint::Hazard,
    );
    // An ammunition hoist house on deck.
    f.block(
        Vec3::new(0.0, HULL.y + 1.2, p.centre + 2.0),
        Vec3::new(5.0, 2.4, 6.0),
        0.85,
        Paint::Ochre,
    );
    for side in [-1.0f32, 1.0] {
        f.lamp(
            Vec3::new(side * (HULL.x - 0.6), HULL.y + 0.5, z1 - 2.0),
            Vec3::Y,
        );
    }
    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon if s.x.abs() > 1.0 => {
                let side = s.x.signum();
                let c = Vec3::new(side * (HULL.x + 2.0), 0.0, s.z);
                f.turret(c, side, s.z - 4.0 - 1.5 * k, module, k);
                f.block(
                    Vec3::new(side * (HULL.x + 0.6), 0.0, s.z),
                    Vec3::new(1.2, 2.6, 5.0),
                    1.0,
                    Paint::Metal,
                );
            }
            SlotKind::Weapon => {
                let dir = s.y.signum();
                let base = Vec3::new(0.0, dir * HULL.y, s.z);
                let c = Vec3::new(0.0, dir * (HULL.y + 2.4), s.z);
                f.rod(base, c, 1.2, 1.0, Paint::Metal);
                f.turret(c, dir, s.z - 3.5 - 1.5 * k, module, k);
            }
            SlotKind::Radiator => {
                let k = k / 2.0;
                f.radiator(Vec3::new(0.0, HULL.y, s.z), Vec3::Y, 9.0 * k, 8.0 * k);
                f.radiator(Vec3::new(0.0, -HULL.y, s.z), Vec3::NEG_Y, 7.0 * k, 8.0 * k);
            }
            _ => {}
        }
    }
}

/// Shield ring: a heavy octagonal collar clamped round the spine, flanged
/// and bolted, the emitters on plinths top and bottom.
fn shield_ring(f: &mut Freighter, p: &Placed) {
    f.spine(p.front, p.back);
    let z = p.centre;
    let (ri, ro) = (3.0f32, 9.6f32);
    let collar = [
        (z - 3.8, ri),
        (z - 3.8, ro - 0.7),
        (z - 3.1, ro),
        (z + 3.1, ro),
        (z + 3.8, ro - 0.7),
        (z + 3.8, ri),
        (z - 3.8, ri),
    ];
    let a0 = TAU / 16.0;
    f.m.paint(
        &lathe(&collar, 8, a0, a0 + TAU).flat(),
        Mat4::IDENTITY,
        OXIDE,
        Paint::Red,
    );
    // Flanges, a hazard band, bolt heads round the flanges.
    for (dz, paint, r) in [
        (-3.4f32, Paint::Metal, ro + 0.35),
        (3.4, Paint::Metal, ro + 0.35),
        (0.0, Paint::Hazard, ro + 0.1),
    ] {
        let wdt = if paint == Paint::Hazard { 1.6 } else { 0.5 };
        let band = [(z + dz - wdt / 2.0, r), (z + dz + wdt / 2.0, r)];
        f.m.paint(
            &lathe(&band, 8, a0, a0 + TAU).flat(),
            Mat4::IDENTITY,
            1.0,
            paint,
        );
    }
    for i in 0..32 {
        let a = i as f32 / 32.0 * TAU;
        for dz in [-3.4f32, 3.4] {
            f.block(
                Vec3::new(
                    a.cos() * (ro + 0.25),
                    a.sin() * (ro + 0.25),
                    z + dz - dz.signum() * 0.45,
                ),
                Vec3::splat(0.4),
                1.0,
                Paint::Metal,
            );
        }
    }
    for side in [-1.0f32, 1.0] {
        f.beacon(Vec3::new(side * (ro + 0.6), 0.0, z));
        f.lamp(Vec3::new(side * 4.0, ro - 0.2, z + 3.9), Vec3::Z);
    }
    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Shield => {
                let dir = s.y.signum();
                let c = Vec3::new(0.0, dir * (ro + 0.8), z);
                f.block(c, Vec3::new(4.4, 1.6, 4.4), 1.0, Paint::Ochre);
                shield(&mut f.m, c + Vec3::Y * dir * 0.9, k * KIT);
            }
            SlotKind::Power => {
                // Transformer cabinets on the collar's flanks.
                for side in [-1.0f32, 1.0] {
                    let c = Vec3::new(side * (ro + 1.0), -3.4, z);
                    f.block(c, Vec3::new(2.0, 3.0, 5.0), 0.85, Paint::Ochre);
                    for i in 0..5 {
                        f.block(
                            c + Vec3::new(side * 1.05, 0.0, -2.0 + i as f32),
                            Vec3::new(0.2, 2.6, 0.25),
                            1.0,
                            Paint::Metal,
                        );
                    }
                }
            }
            SlotKind::Radiator => {
                let k = k / 2.0;
                for side in [-1.0f32, 1.0] {
                    f.radiator(
                        Vec3::new(side * SPINE.x, 0.0, s.z.min(p.back - 5.0 * k)),
                        Vec3::new(side, 0.0, 0.0),
                        11.0 * k,
                        8.0 * k,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Container colours: worn liveries from every line in the trade.
const LIVERIES: [(Paint, f32); 9] = [
    (Paint::Red, 0.75),
    (Paint::Ochre, 1.0),
    (Paint::Hull, 0.8),
    (Paint::Olive, 1.0),
    (Paint::Sky, 0.6),
    (Paint::Gunmetal, 1.1),
    (Paint::Red, 0.5),
    (Paint::Ochre, 0.7),
    (Paint::Hull, 0.6),
];

/// Cargo bay: open-frame cell guides round the spine, containers stacked
/// five high in them, a gantry crane riding the top, floodlights.
fn cargo_bay(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    // Cell grid: six columns, five tiers, the spine in the middle two.
    let (cw, ch) = (2.65f32, 2.7f32);
    let cols: Vec<f32> = (0..6).map(|i| (i as f32 - 2.5) * cw).collect();
    let tiers: Vec<f32> = (0..5).map(|j| (j as f32 - 2.0) * ch).collect();
    let (hx, hy) = (3.0 * cw + 0.3, 2.5 * ch + 0.3);
    // The spine through the middle, wider here to fill its cells.
    f.block(
        Vec3::Z * p.centre,
        Vec3::new(2.0 * cw, ch, z1 - z0),
        OXIDE,
        Paint::Red,
    );
    f.block(
        Vec3::Z * (z0 + 0.6),
        Vec3::new(2.0 * cw + 0.2, ch + 0.2, 1.0),
        1.0,
        Paint::Hazard,
    );

    // Portal frames at each end and between the slots.
    let slots: Vec<f32> = p
        .fitted
        .slots()
        .filter(|(s, _)| s.kind == SlotKind::Cargo)
        .map(|(s, _)| p.centre - s.position_m[2] as f32)
        .collect();
    let mut portals = vec![z0 + 0.5, z1 - 0.5];
    for w in slots.windows(2) {
        portals.push((w[0] + w[1]) / 2.0);
    }
    for &z in &portals {
        for side in [-1.0f32, 1.0] {
            f.beam(
                Vec3::new(side * hx, -hy, z),
                Vec3::new(side * hx, hy, z),
                0.7,
                1.0,
                Paint::Ochre,
            );
        }
        for y in [-hy, hy] {
            f.beam(
                Vec3::new(-hx, y, z),
                Vec3::new(hx, y, z),
                0.7,
                1.0,
                Paint::Ochre,
            );
        }
        // Struts from the frame's corners in to the spine.
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            f.rod(
                Vec3::new(sx * hx, sy * hy, z),
                Vec3::new(sx * cw, sy * ch / 2.0, z),
                0.22,
                1.0,
                Paint::Metal,
            );
        }
    }
    // Longitudinal rails along the corners, hazard-striped below.
    for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let paint = if sy < 0.0 {
            Paint::Hazard
        } else {
            Paint::Ochre
        };
        f.beam(
            Vec3::new(sx * hx, sy * hy, z0 + 0.5),
            Vec3::new(sx * hx, sy * hy, z1 - 0.5),
            0.5,
            1.0,
            paint,
        );
    }
    // Work lamps along the top rails, beacons at the corners.
    for (i, &z) in portals.iter().enumerate() {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        f.lamp(Vec3::new(side * hx, hy + 0.6, z), Vec3::Y);
    }
    f.beacon(Vec3::new(-hx, hy + 0.5, z0 + 0.5));
    f.beacon(Vec3::new(hx, -hy - 0.5, z1 - 0.5));

    // The stacks.
    let bay = f.index;
    let mut box_ = Mesh::default();
    for (n, (slot, module)) in p.fitted.slots().enumerate() {
        if slot.kind != SlotKind::Cargo {
            continue;
        }
        let z = p.centre - slot.position_m[2] as f32;
        let Some(module) = module else {
            // Empty cell guides: just the corner posts of the cells.
            for &x in &[cols[0], cols[5]] {
                for &y in &[tiers[0], tiers[4]] {
                    f.beam(
                        Vec3::new(x * 1.08, y * 1.08, z - 4.0),
                        Vec3::new(x * 1.08, y * 1.08, z + 4.0),
                        0.3,
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            continue;
        };
        let fill = match module.size {
            retrox_sim::ship::Size::Large => 1.0,
            retrox_sim::ship::Size::Medium => 0.45,
            retrox_sim::ship::Size::Small => 0.15,
        };
        for (i, &x) in cols.iter().enumerate() {
            for (j, &y) in tiers.iter().enumerate() {
                if (2..=3).contains(&i) && j == 2 {
                    continue; // the spine
                }
                let hsh = hash(bay, n, i * 7 + j);
                // Partial loads fill from the spine out; the top tier has
                // gaps.
                let reach = ((x.abs() / cw) + (y.abs() / ch)) / 5.0;
                if reach > fill || (j == 4 && hsh.is_multiple_of(7)) {
                    continue;
                }
                let (paint, shade) = LIVERIES[(hsh >> 4) as usize % LIVERIES.len()];
                box_.vertices.clear();
                box_.indices.clear();
                container(
                    &mut box_,
                    Vec3::new(x, y, z),
                    Vec3::new(cw - 0.2, ch - 0.2, 9.6),
                    paint,
                );
                f.m.add(
                    &box_,
                    Mat4::IDENTITY,
                    shade * (0.9 + 0.2 * ((hsh >> 9) % 8) as f32 / 8.0),
                );
            }
        }
    }

    // A gantry crane riding the top rails, parked somewhere different in
    // each bay.
    let cz = z0 + 5.0 + (hash(bay, 99, 0) % 25) as f32;
    let top = hy + 4.2;
    for side in [-1.0f32, 1.0] {
        for dz in [-1.6f32, 1.6] {
            f.beam(
                Vec3::new(side * hx, hy, cz + dz),
                Vec3::new(side * hx, top, cz + dz),
                0.6,
                1.0,
                Paint::Hazard,
            );
        }
        f.beacon(Vec3::new(side * hx, top + 0.9, cz));
    }
    for dz in [-1.6f32, 1.6] {
        f.beam(
            Vec3::new(-hx - 0.3, top, cz + dz),
            Vec3::new(hx + 0.3, top, cz + dz),
            0.8,
            1.0,
            Paint::Ochre,
        );
    }
    f.block(
        Vec3::new(hx * 0.4, top - 0.9, cz),
        Vec3::new(2.4, 1.8, 2.6),
        1.0,
        Paint::Ochre,
    );
    f.block(
        Vec3::new(hx * 0.4, top - 0.8, cz - 1.32),
        Vec3::new(1.8, 0.7, 0.05),
        1.0,
        Paint::Glass,
    );
    f.lamp(Vec3::new(-hx * 0.5, top - 0.7, cz), Vec3::NEG_Y);
}

/// Reactor section: squat drums round the spine and a radiator array on
/// each side, three big panels stacked on a lattice mast.
fn reactor_section(f: &mut Freighter, p: &Placed) {
    f.spine(p.front, p.back);
    for (slot, module) in p.fitted.slots() {
        let z = p.centre - slot.position_m[2] as f32;
        match (slot.kind, module) {
            (SlotKind::Power, Some(m)) => {
                let k = scale(m.size) / 2.0;
                let (r, h) = (7.2 * k.max(0.6), 3.4);
                let drum = [
                    (z - h, 0.0),
                    (z - h, r - 0.6),
                    (z - h + 0.6, r),
                    (z + h - 0.6, r),
                    (z + h, r - 0.6),
                    (z + h, 0.0),
                ];
                f.m.paint(
                    &lathe(&drum, 16, 0.0, TAU).flat(),
                    Mat4::IDENTITY,
                    OXIDE * 0.9,
                    Paint::Red,
                );
                for dz in [-2.0f32, 0.0, 2.0] {
                    let paint = if dz == 0.0 {
                        Paint::Hazard
                    } else {
                        Paint::Metal
                    };
                    let band = [(z + dz - 0.35, r + 0.12), (z + dz + 0.35, r + 0.12)];
                    f.m.paint(
                        &lathe(&band, 16, 0.0, TAU).flat(),
                        Mat4::IDENTITY,
                        1.0,
                        paint,
                    );
                }
                // A hatch and a ladder of rungs up its face.
                f.block(
                    Vec3::new(0.0, r + 0.15, z),
                    Vec3::new(2.0, 0.3, 2.0),
                    1.0,
                    Paint::Ochre,
                );
                f.beacon(Vec3::new(0.0, r + 0.5, z + h - 0.5));
            }
            (SlotKind::Power, None) => {
                // Empty cradles.
                for dz in [-2.4f32, 2.4] {
                    f.block(
                        Vec3::new(0.0, -SPINE.y - 1.2, z + dz),
                        Vec3::new(9.0, 1.0, 0.8),
                        1.0,
                        Paint::Ochre,
                    );
                }
            }
            _ => {}
        }
    }
    for (slot, module, s) in p.modules() {
        if slot.kind != SlotKind::Radiator {
            continue;
        }
        let k = scale(module.size) / 2.0;
        let side = s.x.signum();
        let x0 = side * (SPINE.x + 4.0);
        // The lattice mast the panels hang from.
        let (my, mz) = (8.5 * k, s.z);
        for dz in [-1.0f32, 1.0] {
            f.beam(
                Vec3::new(x0, -my, mz + dz),
                Vec3::new(x0, my, mz + dz),
                0.5,
                1.0,
                Paint::Ochre,
            );
            f.rod(
                Vec3::new(side * SPINE.x, 0.0, mz + dz * 3.0),
                Vec3::new(x0, 0.0, mz + dz),
                0.35,
                1.0,
                Paint::Metal,
            );
        }
        f.beacon(Vec3::new(x0, my + 0.5, mz));
        for y in [-my * 0.8, 0.0, my * 0.8] {
            f.radiator(
                Vec3::new(x0, y, mz),
                Vec3::new(side, 0.0, 0.0),
                15.0 * k,
                8.0 * k,
            );
        }
    }
}

// --- Sterns ---------------------------------------------------------------

/// Sprint stern: a slab-sided mustard engine house, her number big on its
/// flanks, two bells out of its back, radiator fins above and below.
fn sprint_stern(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    let ez1 = z1 - 3.5;
    let house = [
        (z0, chamfer_rect(HULL.x * 2.0, HULL.y * 2.0, 1.5, 0.0)),
        (z0 + 3.0, chamfer_rect(17.0, 11.0, 2.0, 0.0)),
        (ez1, chamfer_rect(17.0, 11.0, 2.0, 0.0)),
        (ez1 + 1.0, chamfer_rect(16.0, 10.0, 2.0, 0.0)),
    ];
    let paint = |s: usize, _e: usize| match s {
        2 => Paint::Hazard,
        _ => Paint::Ochre,
    };
    f.m.add(&loft(&house, paint, Paint::Metal), Mat4::IDENTITY, 0.8);
    // Louvred vents and frames down the flanks.
    for side in [-1.0f32, 1.0] {
        for i in 0..8 {
            f.block(
                Vec3::new(side * 8.55, -2.8, z0 + 5.0 + i as f32 * 0.9),
                Vec3::new(0.2, 3.0, 0.4),
                1.0,
                Paint::Metal,
            );
        }
        f.block(
            Vec3::new(side * 8.55, -4.6, (z0 + ez1) / 2.0 + 1.5),
            Vec3::new(0.15, 0.6, ez1 - z0 - 3.0),
            OXIDE,
            Paint::Red,
        );
        f.beacon(Vec3::new(side * 8.5, 5.5, ez1 + 0.5));
        f.lamp(Vec3::new(side * 6.5, 5.6, z0 + 3.4), Vec3::NEG_Z);
    }
    f.number(8.55, 0.4, z0 + 8.0, 3.6);

    for (slot, module) in p.fitted.slots() {
        let [x, y, z] = slot.position_m.map(|v| v as f32);
        let s = Vec3::new(x, y, p.centre - z);
        let Some(module) = module else {
            if slot.kind == SlotKind::Engine {
                f.block(
                    Vec3::new(s.x, s.y, ez1 + 1.2),
                    Vec3::new(4.0, 4.0, 0.6),
                    1.0,
                    Paint::Metal,
                );
            }
            continue;
        };
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(s.x, s.y, ez1 + 1.0);
                f.rod(
                    throat - Vec3::Z * 0.2,
                    throat + Vec3::Z * 1.0,
                    1.9 * k / 2.0,
                    1.0,
                    Paint::Metal,
                );
                let n = bell(&mut f.m, throat + Vec3::Z * 0.8, k * 5.0, 1.05 * k);
                f.nozzles.push(n);
            }
            SlotKind::Radiator => {
                let dir = s.y.signum();
                f.radiator(
                    Vec3::new(0.0, dir * 5.5, s.z),
                    Vec3::new(0.0, dir, 0.0),
                    8.0 * k / 2.0,
                    10.0 * k / 2.0,
                );
            }
            SlotKind::Power => {
                // A pair of vent stacks on the house's roof.
                for side in [-1.0f32, 1.0] {
                    let b = Vec3::new(side * 3.0, 5.5, s.z);
                    f.rod(b, b + Vec3::Y * 3.5, 0.9, OXIDE, Paint::Red);
                    f.rod(
                        b + Vec3::Y * 3.0,
                        b + Vec3::Y * 3.6,
                        0.95,
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            SlotKind::JumpDrive => {
                // The jump coil drum, slung under the house.
                let c = Vec3::new(0.0, -6.3, s.z);
                f.rod(
                    c - Vec3::Z * 4.0,
                    c + Vec3::Z * 4.0,
                    1.5 * k / 2.0,
                    1.0,
                    Paint::Foil,
                );
                for dz in [-3.0f32, -1.0, 1.0, 3.0] {
                    f.rod(
                        c + Vec3::Z * (dz - 0.2),
                        c + Vec3::Z * (dz + 0.2),
                        1.65 * k / 2.0,
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Cruise stern: four fat propellant tanks clustered round the spine, a
/// thrust cone behind them and one big bell; radiator arrays out the sides.
fn cruise_stern(f: &mut Freighter, p: &Placed) {
    let (z0, z1) = (p.front, p.back);
    let tz1 = z1 - 7.0;
    f.spine(z0, tz1 + 1.0);
    let tank_r = 3.0;
    let profile: Vec<(f32, f32)> = {
        let len = tz1 - z0 - 1.0;
        let mut v = vec![(0.0, 0.0)];
        for i in 1..=4 {
            let a = i as f32 / 4.0 * FRAC_PI_2;
            v.push((tank_r * (1.0 - a.cos()), tank_r * a.sin()));
        }
        for i in 0..4 {
            let a = i as f32 / 4.0 * FRAC_PI_2;
            v.push((len - tank_r + tank_r * a.sin(), tank_r * a.cos()));
        }
        v.push((len, 0.0));
        v
    };
    let tank = lathe(&profile, 16, 0.0, TAU);
    for (i, (sx, sy)) in [(-1.0f32, 1.0f32), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
        .into_iter()
        .enumerate()
    {
        let c = Vec3::new(sx * 3.4, sy * 3.4, z0 + 0.5);
        let (paint, shade) = if i % 2 == 0 {
            (Paint::Ochre, 0.9)
        } else {
            (Paint::Red, OXIDE)
        };
        f.m.paint(&tank, Mat4::from_translation(c), shade, paint);
        for t in [0.3f32, 0.7] {
            let z = c.z + (tz1 - z0 - 1.0) * t;
            f.m.paint(
                &lathe(
                    &[(z - 0.4, tank_r + 0.1), (z + 0.4, tank_r + 0.1)],
                    16,
                    0.0,
                    TAU,
                ),
                Mat4::from_translation(c.with_z(0.0)),
                1.0,
                if t < 0.5 { Paint::Hazard } else { Paint::Metal },
            );
        }
    }
    // The thrust cone.
    let cone = [
        (tz1 - 0.5, 0.0),
        (tz1 - 0.5, 7.4),
        (tz1 + 1.0, 7.4),
        (z1 - 2.0, 3.2),
        (z1 - 2.0, 0.0),
    ];
    f.m.paint(
        &lathe(&cone, 16, 0.0, TAU).flat(),
        Mat4::IDENTITY,
        OXIDE,
        Paint::Red,
    );
    f.hoop(tz1 + 0.2, 7.5, 0.35, Paint::Hazard);
    for side in [-1.0f32, 1.0] {
        f.beacon(Vec3::new(side * 7.6, 0.0, tz1 + 0.6));
        f.lamp(Vec3::new(0.0, side * 6.8, z0 + 0.3), Vec3::NEG_Z);
    }

    for (slot, module) in p.fitted.slots() {
        let [x, y, z] = slot.position_m.map(|v| v as f32);
        let s = Vec3::new(x, y, p.centre - z);
        let Some(module) = module else { continue };
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Engine => {
                let n = bell(&mut f.m, Vec3::new(s.x, s.y, z1 - 2.2), k * 5.5, 1.5 * k);
                f.nozzles.push(n);
            }
            SlotKind::Radiator => {
                let side = s.x.signum();
                f.radiator(
                    Vec3::new(side * 6.0, 0.0, s.z),
                    Vec3::new(side, 0.0, 0.0),
                    13.0 * k / 2.0,
                    9.0 * k / 2.0,
                );
            }
            SlotKind::Power => {
                f.rod(
                    Vec3::new(0.0, 0.0, s.z - 2.5),
                    Vec3::new(0.0, 0.0, s.z + 2.5),
                    3.0 * k / 2.0,
                    1.0,
                    Paint::Metal,
                );
            }
            SlotKind::JumpDrive => {
                // Field coils round the whole tank cluster, on standoffs.
                for dz in [-1.6f32, 0.0, 1.6] {
                    f.hoop(s.z + dz - 4.0, 8.5, 0.5, Paint::Foil);
                }
                for i in 0..8 {
                    let a = (i as f32 + 0.5) / 8.0 * TAU;
                    let d = Vec3::new(a.cos(), a.sin(), 0.0);
                    f.beam(
                        d * 7.2 + Vec3::Z * (s.z - 6.0),
                        d * 7.2 + Vec3::Z * (s.z - 2.0),
                        0.5,
                        1.0,
                        Paint::Metal,
                    );
                    f.rod(
                        d * 6.0 + Vec3::Z * (s.z - 4.0),
                        d * 8.0 + Vec3::Z * (s.z - 4.0),
                        0.3,
                        1.0,
                        Paint::Metal,
                    );
                }
            }
            _ => {}
        }
    }
}
