//! The blockade runner: the gunship's layout stripped to a skeleton for
//! running dark. A triangular keel truss runs the length of the ship,
//! with pipework along it and everything else bolted on where it's
//! needed: an armoured capsule for the crew, plated drums round the
//! reactors, and oversized engine bells hung on open frames. Matte
//! gunmetal and black, low-visibility markings, small red running lights.
//!
//! Every section it can mount has its own runner rendition, drawn round
//! whichever modules are fitted, so empty slots show bare structure.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{Loadout, Module, Position, SlotKind};

use super::kit::*;
use super::{Placed, at, is_laser, placed, scale};
use crate::meshes::{
    Mesh, Paint, ShipShape, boxed, chamfer_rect, cube, cylinder, icosphere, lathe, loft, strut,
    torus,
};

/// Circumradius of the keel truss's triangular section.
const KEEL_R: f32 = 1.25;
/// Kit modules are sized for small ships; the runner's are this much bigger.
const KIT: f32 = 1.35;
/// The low-visibility hull number.
const NUMBER: &str = "31";

/// The ship under construction, with the meshes it stamps out.
struct Runner {
    m: Mesh,
    nozzles: Vec<(Vec3, f32)>,
    cyl: Mesh,
    cube: Mesh,
    ball: Mesh,
}

impl Runner {
    fn rod(&mut self, a: Vec3, b: Vec3, r: f32, paint: Paint) {
        self.rod_s(a, b, r, 1.0, paint);
    }

    fn rod_s(&mut self, a: Vec3, b: Vec3, r: f32, shade: f32, paint: Paint) {
        self.m.paint(&self.cyl, strut(a, b, r), shade, paint);
    }

    fn block(&mut self, c: Vec3, size: Vec3, shade: f32, paint: Paint) {
        self.m.paint(&self.cube, boxed(c, size), shade, paint);
    }

    fn ball(&mut self, c: Vec3, r: f32, paint: Paint) {
        self.m.paint(&self.ball, at(c, r), 1.0, paint);
    }

    /// A small red running light.
    fn running_light(&mut self, c: Vec3) {
        self.m.paint(&self.ball, at(c, 0.09), 1.0, Paint::Beacon);
    }

    /// A ring round the Z axis at `z`.
    fn hoop(&mut self, z: f32, radius: f32, thick: f32, y: f32, paint: Paint) {
        self.m.paint(
            &torus(thick / radius, 40, 6),
            Mat4::from_translation(Vec3::new(0.0, y, z))
                * Mat4::from_rotation_x(FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(radius)),
            1.0,
            paint,
        );
    }

    /// The keel truss from `z0` to `z1`: three longerons, triangular
    /// battens, zig-zag diagonals down each face and pipework inside.
    fn keel(&mut self, z0: f32, z1: f32) {
        let bays = ((z1 - z0) / 2.4).ceil().max(1.0) as usize;
        let corners = keel_corners();
        for c in corners {
            self.rod(c + Vec3::Z * z0, c + Vec3::Z * z1, 0.085, Paint::Gunmetal);
        }
        for i in 0..=bays {
            let z = z0 + (z1 - z0) * i as f32 / bays as f32;
            for k in 0..3 {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                self.rod_s(a + Vec3::Z * z, b + Vec3::Z * z, 0.05, 0.9, Paint::Metal);
                if i < bays {
                    let zn = z0 + (z1 - z0) * (i + 1) as f32 / bays as f32;
                    let (p, q) = if (i + k) % 2 == 0 { (a, b) } else { (b, a) };
                    self.rod_s(p + Vec3::Z * z, q + Vec3::Z * zn, 0.035, 0.8, Paint::Metal);
                }
            }
        }
        // Pipework: a fat coolant line, a thinner fuel line in dull red,
        // and a cable bundle, clamped at every batten.
        self.rod_s(
            Vec3::new(0.38, -0.2, z0),
            Vec3::new(0.38, -0.2, z1),
            0.09,
            1.4,
            Paint::Metal,
        );
        self.rod_s(
            Vec3::new(-0.4, -0.25, z0),
            Vec3::new(-0.4, -0.25, z1),
            0.055,
            0.45,
            Paint::Red,
        );
        self.rod_s(
            Vec3::new(0.0, 0.45, z0),
            Vec3::new(0.0, 0.45, z1),
            0.07,
            0.7,
            Paint::Olive,
        );
        for i in 0..=bays {
            let z = z0 + (z1 - z0) * i as f32 / bays as f32;
            self.block(
                Vec3::new(0.0, -0.2, z),
                Vec3::new(1.15, 0.12, 0.14),
                0.8,
                Paint::Metal,
            );
        }
    }

    /// Struts joining the keel at `z_keel` to points round a ring, for
    /// carrying something wider than the truss.
    fn spider(&mut self, z_keel: f32, ends: &[Vec3], r: f32) {
        let corners = keel_corners();
        for &e in ends {
            let near = corners
                .iter()
                .copied()
                .min_by(|a, b| {
                    a.truncate()
                        .distance(e.truncate())
                        .total_cmp(&b.truncate().distance(e.truncate()))
                })
                .unwrap();
            self.rod(near + Vec3::Z * z_keel, e, r, Paint::Gunmetal);
        }
    }

    /// A plated reactor drum round the keel at `z`, `len` long.
    fn reactor_drum(&mut self, z: f32, radius: f32, len: f32) {
        let (h, r) = (len / 2.0, radius);
        let profile = [
            (-h, 0.0),
            (-h, r * 0.62),
            (-h + 0.25, r),
            (h - 0.25, r),
            (h, r * 0.62),
            (h, 0.0),
        ];
        self.m.paint(
            &lathe(&profile, 10, 0.0, TAU).flat(),
            Mat4::from_translation(Vec3::Z * z),
            1.0,
            Paint::Gunmetal,
        );
        // Bolted armour bands and a dull red warning plate.
        for dz in [-h * 0.45, h * 0.45] {
            let band = [(z + dz - 0.12, r + 0.06), (z + dz + 0.12, r + 0.06)];
            self.m.paint(
                &lathe(&band, 10, 0.0, TAU).flat(),
                Mat4::IDENTITY,
                1.0,
                Paint::Metal,
            );
        }
        self.block(
            Vec3::new(0.0, r + 0.03, z),
            Vec3::new(0.5, 0.06, 0.5),
            0.5,
            Paint::Red,
        );
    }

    /// Open saddle clamps on the keel where a reactor would go.
    fn empty_saddle(&mut self, z: f32) {
        for dz in [-0.8f32, 0.8] {
            for side in [-1.0f32, 1.0] {
                self.block(
                    Vec3::new(side * 1.25, -0.3, z + dz),
                    Vec3::new(0.25, 0.7, 0.18),
                    1.0,
                    Paint::Metal,
                );
            }
        }
    }

    /// The jump drive: three field coils round the keel, gold-wrapped.
    fn jump_coils(&mut self, z: f32, k: f32) {
        for i in 0..3 {
            let zz = z + (i as f32 - 1.0) * 0.55 * k;
            self.hoop(zz, 1.55 * k / 1.4, 0.16, 0.0, Paint::Foil);
            self.hoop(zz, 1.55 * k / 1.4, 0.19, 0.0, Paint::Metal);
        }
    }

    /// An RCS quad: four small thruster blocks round `c`.
    fn rcs(&mut self, c: Vec3, spread: f32) {
        for a in [0.0f32, 1.0, 2.0, 3.0] {
            let ang = a * FRAC_PI_2 + FRAC_PI_2 / 2.0;
            let d = Vec3::new(ang.cos(), ang.sin(), 0.0);
            let p = c + d * spread;
            self.block(p, Vec3::splat(0.32), 1.0, Paint::Metal);
            self.rod(p, p + d * 0.28, 0.08, Paint::Metal);
        }
    }

    /// A gun on an open ball mount at `p`, muzzle forward at `muzzle_z`.
    fn ball_gun(&mut self, p: Vec3, muzzle_z: f32, module: &Module, k: f32) {
        self.m
            .paint(&self.ball, at(p, 0.36 * k), 1.0, Paint::Gunmetal);
        gun(&mut self.m, p, muzzle_z, is_laser(module), k * KIT);
    }

    /// A radiator panel standing from `root` along `out`, sized for `k`.
    /// Blackened, not white: the runner's radiators are painted out, a
    /// dark plate with coolant ribs and a frame of tube.
    fn radiator(&mut self, root: Vec3, out: Vec3, k: f32, reach: f32, len: f32) {
        let (h, l) = (reach * k, len * k);
        let out = out.normalize();
        let rot = Quat::from_rotation_arc(Vec3::Y, out);
        let mid = root + out * h / 2.0;
        let panel = |r: &mut Runner, size: Vec3, at_: Vec3, shade: f32, paint: Paint| {
            r.m.paint(
                &r.cube,
                Mat4::from_scale_rotation_translation(size, rot, at_),
                shade,
                paint,
            );
        };
        panel(self, Vec3::new(0.04, h, l), mid, 0.55, Paint::Gunmetal);
        let ribs = (l / 0.45) as i32;
        for i in 0..ribs {
            let z = -l / 2.0 + l * (i as f32 + 0.5) / ribs as f32;
            panel(
                self,
                Vec3::new(0.09, h * 0.94, 0.05),
                mid + Vec3::Z * z,
                1.0,
                Paint::Metal,
            );
        }
        // Tube frame: the header pipes along root and tip, the ends.
        let side = rot * Vec3::Z;
        for t in [0.0f32, 1.0] {
            let c = root + out * h * t;
            self.rod(c - side * l / 2.0, c + side * l / 2.0, 0.06, Paint::Metal);
        }
        for e in [-0.5f32, 0.5] {
            let c = root + side * l * e;
            self.rod(c, c + out * h, 0.045, Paint::Metal);
        }
    }

    /// A low-visibility number stencilled on a flat panel.
    fn number(&mut self, origin: Vec3, right: Vec3, up: Vec3, h: f32) {
        self.number_on(h, &flat(origin, right, up));
    }

    /// The number in a grey just lighter than the hull: readable up close,
    /// gone at a distance.
    fn number_on(&mut self, h: f32, surface: Surface) {
        let mut text = Mesh::default();
        stencil(&mut text, NUMBER, h, Paint::Gunmetal, surface);
        self.m.add(&text, Mat4::IDENTITY, 1.7);
    }
}

/// The keel truss's three longerons in cross-section (top, then the two
/// bottom corners).
fn keel_corners() -> [Vec3; 3] {
    [90.0f32, 210.0, 330.0].map(|d| {
        let a = d.to_radians();
        Vec3::new(KEEL_R * a.cos(), KEEL_R * a.sin(), 0.0)
    })
}

pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    let mut r = Runner {
        m: Mesh::default(),
        nozzles: Vec::new(),
        cyl: cylinder(8),
        cube: cube(),
        ball: icosphere(1),
    };
    let sections = placed(fit);
    for p in &sections {
        match p.section.id {
            "command-bow-m" => command_bow(&mut r, p),
            "gun-bow-m" => gun_bow(&mut r, p),
            "gun-deck-m" => gun_deck(&mut r, p),
            "shield-ring-m" => shield_ring(&mut r, p),
            "cargo-bay-m" => cargo_bay(&mut r, p),
            "reactor-section-m" => reactor_section(&mut r, p),
            "sprint-stern-m" => sprint_stern(&mut r, p),
            "cruise-stern-m" => cruise_stern(&mut r, p),
            _ => match p.section.position {
                Position::Bow => command_bow(&mut r, p),
                Position::Stern => cruise_stern(&mut r, p),
                _ => {
                    r.keel(p.front, p.back);
                    loose_modules(&mut r, p);
                }
            },
        }
    }
    if let Some(last) = sections.last() {
        soot(&mut r.m, last.back - 7.0, last.back + 2.0, 0.35);
    }
    // Matte, dark, not much to see: knock back the enamels.
    for v in &mut r.m.vertices {
        let p = v.paint;
        if [Paint::Gunmetal, Paint::Olive]
            .iter()
            .any(|&q| q as u32 as f32 == p)
        {
            v.shade *= 0.55;
        } else if p == Paint::Hull as u32 as f32 {
            v.shade *= 0.45;
        }
    }
    (r.m, ShipShape { nozzles: r.nozzles })
}

/// Modules on a section the runner has no rendition for, hung off the keel.
fn loose_modules(r: &mut Runner, p: &Placed) {
    for (slot, module, at_) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => r.ball_gun(at_, at_.z - 2.0 * k, module, k),
            SlotKind::Engine => {
                let n = bell(&mut r.m, at_, k * 2.0, 0.6 * k);
                r.nozzles.push(n);
            }
            SlotKind::Shield => shield(&mut r.m, at_, k * KIT),
            SlotKind::Radar => radar(&mut r.m, at_, k * KIT),
            SlotKind::Radiator => r.radiator(at_, Vec3::Y, k, 2.0, 2.0),
            SlotKind::Cargo => pod_cluster(r, at_.z, k, 0),
            SlotKind::Power => r.reactor_drum(at_.z, 1.6, 2.0),
            _ => {}
        }
    }
}

// --- Bows -----------------------------------------------------------------

/// The capsule's radius `t` metres back from its nose.
fn capsule_r(t: f32) -> f32 {
    const PROFILE: [(f32, f32); 7] = [
        (0.0, 0.0),
        (0.25, 0.55),
        (0.7, 1.05),
        (1.4, 1.42),
        (2.2, 1.58),
        (4.3, 1.58),
        (5.2, 0.95),
    ];
    let t = t.clamp(0.0, 5.2);
    for w in PROFILE.windows(2) {
        let ((t0, r0), (t1, r1)) = (w[0], w[1]);
        if t <= t1 {
            return r0 + (r1 - r0) * (t - t0) / (t1 - t0);
        }
    }
    0.95
}

/// Command bow: a small armoured crew capsule out at the end of the keel,
/// a slit windscreen in its brow, a whip aerial on its nose.
fn command_bow(r: &mut Runner, p: &Placed) {
    let f = p.front;
    let y0 = 0.15;
    let lift = Mat4::from_translation(Vec3::new(0.0, y0, f));
    let mut profile: Vec<(f32, f32)> = (0..=26)
        .map(|i| {
            let t = 5.2 * i as f32 / 26.0;
            (t, capsule_r(t))
        })
        .collect();
    profile.push((5.2, 0.0));
    r.m.paint(
        &lathe(&profile, 12, 0.0, TAU).flat(),
        lift,
        1.0,
        Paint::Gunmetal,
    );
    // The slit windscreen and its visor bars.
    let up = FRAC_PI_2;
    let slit = [(0.95, capsule_r(0.95) + 0.03), (1.3, capsule_r(1.3) + 0.03)];
    r.m.paint(
        &lathe(&slit, 12, up - 0.95, up + 0.95),
        lift,
        1.0,
        Paint::Glass,
    );
    for a in [-0.5f32, 0.0, 0.5] {
        let ang = up + a;
        let rr = capsule_r(1.12) + 0.07;
        r.block(
            Vec3::new(rr * ang.cos(), y0 + rr * ang.sin(), f + 1.12),
            Vec3::new(0.07, 0.07, 0.5),
            1.0,
            Paint::Metal,
        );
    }
    // Armour bands, side vision ports, the number.
    for t in [2.25f32, 4.25] {
        let band = [
            (t - 0.12, capsule_r(t) + 0.06),
            (t + 0.12, capsule_r(t) + 0.06),
        ];
        r.m.paint(&lathe(&band, 12, 0.0, TAU).flat(), lift, 1.0, Paint::Metal);
    }
    for side in [-1.0f32, 1.0] {
        r.block(
            Vec3::new(side * 1.5, y0 + 0.45, f + 1.9),
            Vec3::new(0.06, 0.12, 0.5),
            1.0,
            Paint::Glass,
        );
        r.running_light(Vec3::new(side * 1.6, y0, f + 3.3));
        let surface = flank(side, f + 3.9 - side * 0.0, -0.35, |_| 1.6);
        let origin_shift = if side > 0.0 { 0.0 } else { -0.95 };
        let lifted = move |u: f32, v: f32| {
            let (pos, right, upv) = surface(u + origin_shift, v);
            (pos + Vec3::Y * y0, right, upv)
        };
        r.number_on(0.6, &lifted);
    }
    // Nose: a docking probe and a whip aerial.
    r.rod(
        Vec3::new(0.0, y0, f + 0.1),
        Vec3::new(0.0, y0, f - 0.9),
        0.06,
        Paint::Metal,
    );
    r.ball(Vec3::new(0.0, y0, f - 0.9), 0.1, Paint::Metal);
    r.rod(
        Vec3::new(0.6, y0 + 1.1, f + 2.0),
        Vec3::new(0.9, y0 + 1.6, f - 1.5),
        0.02,
        Paint::Metal,
    );
    // The adapter cone of struts back to the keel, then the keel.
    let kz = f + 6.6;
    let ends: Vec<Vec3> = (0..6)
        .map(|i| {
            let a = i as f32 / 6.0 * TAU + 0.3;
            Vec3::new(0.85 * a.cos(), y0 + 0.85 * a.sin(), f + 5.1)
        })
        .collect();
    r.spider(kz, &ends, 0.07);
    r.keel(kz, p.back);

    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => {
                // A ball turret under the chin.
                let ball = Vec3::new(s.x, y0 - 1.25, f + 2.0);
                r.ball_gun(ball, f - 0.6 - 0.8 * k, module, k);
            }
            SlotKind::Radar => {
                let base = Vec3::new(s.x, y0 + capsule_r(3.0) - 0.05, f + 3.2);
                radar(&mut r.m, base, k * KIT);
            }
            SlotKind::Shield => {
                let z = s.z.max(kz + 0.6);
                r.rod(
                    Vec3::new(0.0, KEEL_R, z),
                    Vec3::new(0.0, KEEL_R + 0.3, z),
                    0.12,
                    Paint::Metal,
                );
                shield(&mut r.m, Vec3::new(0.0, KEEL_R + 0.35, z), k * KIT);
            }
            SlotKind::Rcs => r.rcs(Vec3::new(0.0, y0, f + 4.6), 1.3),
            _ => {}
        }
    }
}

/// Gun bow: no capsule, just a low armoured sled for the crew between two
/// gun booms thrust forward like a tuning fork.
fn gun_bow(r: &mut Runner, p: &Placed) {
    let f = p.front;
    let sled = [
        (f + 1.2, chamfer_rect(0.9, 0.4, 0.14, 0.05)),
        (f + 2.3, chamfer_rect(1.95, 1.15, 0.36, 0.16)),
        (f + 2.8, chamfer_rect(2.2, 1.35, 0.4, 0.2)),
        (f + 6.4, chamfer_rect(2.4, 1.5, 0.45, 0.25)),
        (f + 7.6, chamfer_rect(1.9, 1.1, 0.35, 0.2)),
    ];
    let slit = |s: usize, e: usize| match (s, e) {
        (1, 0) => Paint::Glass,
        _ => Paint::Gunmetal,
    };
    r.m.add(&loft(&sled, slit, Paint::Metal), Mat4::IDENTITY, 1.0);
    // Vision slits down the sides.
    for side in [-1.0f32, 1.0] {
        r.block(
            Vec3::new(side * 1.13, 0.6, f + 3.6),
            Vec3::new(0.04, 0.09, 0.9),
            1.0,
            Paint::Glass,
        );
        r.running_light(Vec3::new(side * 1.2, 0.0, f + 7.0));
    }
    // An armour lid over the cabin, and the number on it.
    r.block(
        Vec3::new(0.0, 1.0, f + 4.8),
        Vec3::new(1.4, 0.12, 2.6),
        0.85,
        Paint::Gunmetal,
    );
    r.number(Vec3::new(-0.45, 1.07, f + 5.8), Vec3::X, Vec3::NEG_Z, 0.65);
    r.keel(f + 6.0, p.back);

    // The crossbeam the booms hang from, under the keel.
    let (bx, by, bz) = (2.5f32, -1.0f32, f + 8.6);
    for dz in [-0.45f32, 0.45] {
        r.rod(
            Vec3::new(-bx, by, bz + dz),
            Vec3::new(bx, by, bz + dz),
            0.08,
            Paint::Gunmetal,
        );
    }
    for i in 0..6 {
        let x0 = -bx + 2.0 * bx * i as f32 / 6.0;
        let x1 = -bx + 2.0 * bx * (i + 1) as f32 / 6.0;
        let (a, b) = if i % 2 == 0 {
            (-0.45, 0.45)
        } else {
            (0.45, -0.45)
        };
        r.rod_s(
            Vec3::new(x0, by, bz + a),
            Vec3::new(x1, by, bz + b),
            0.04,
            0.8,
            Paint::Metal,
        );
    }
    let corners = keel_corners();
    for side in [-1.0f32, 1.0] {
        let x = side * bx;
        // The boom: a tube with lightening collars, braced back to the keel.
        let (tip, root) = (Vec3::new(x, by, f + 1.6), Vec3::new(x, by, bz + 0.6));
        r.rod(tip, root, 0.14, Paint::Gunmetal);
        for t in [0.3f32, 0.55, 0.8] {
            let c = tip.lerp(root, t);
            r.rod(c - Vec3::Z * 0.06, c + Vec3::Z * 0.06, 0.2, Paint::Metal);
        }
        let corner = corners[if side > 0.0 { 2 } else { 1 }];
        r.rod(
            Vec3::new(x, by, f + 4.0),
            corner + Vec3::Z * (f + 7.5),
            0.06,
            Paint::Gunmetal,
        );
        r.rod(
            Vec3::new(x, by, f + 4.0),
            Vec3::new(side * 1.1, 0.4, f + 5.0),
            0.05,
            Paint::Metal,
        );
        r.running_light(Vec3::new(x + side * 0.25, by + 0.2, bz));
    }

    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon => {
                // A gun housing on the boom's tip.
                let c = Vec3::new(s.x, by, f + 1.9);
                r.block(c, Vec3::new(0.62, 0.66, 1.5), 1.0, Paint::Gunmetal);
                r.block(
                    c + Vec3::new(0.0, 0.36, 0.0),
                    Vec3::new(0.4, 0.08, 1.2),
                    1.0,
                    Paint::Metal,
                );
                gun(
                    &mut r.m,
                    Vec3::new(s.x, by, f + 1.15),
                    f - 1.2 - 1.2 * k,
                    is_laser(module),
                    k * KIT,
                );
            }
            SlotKind::Radar => {
                radar(&mut r.m, Vec3::new(0.0, 1.06, f + 4.2), k * KIT);
            }
            SlotKind::Shield => {
                let z = s.z.max(f + 7.0);
                shield(&mut r.m, Vec3::new(0.0, KEEL_R + 0.2, z), k * KIT);
            }
            SlotKind::Rcs => {
                for side in [-1.0f32, 1.0] {
                    r.rcs(Vec3::new(side * bx, by, f + 6.0), 0.3);
                }
            }
            _ => {}
        }
    }
}

// --- Mid sections ---------------------------------------------------------

/// Gun deck: a spider frame off the keel carrying open gun mounts out to
/// the sides, pylon turrets above and below, a radiator pair aft.
fn gun_deck(r: &mut Runner, p: &Placed) {
    r.keel(p.front, p.back);
    let corners = keel_corners();
    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Weapon if s.x.abs() > 0.5 => {
                let side = s.x.signum();
                let mount = Vec3::new(s.x, s.y, s.z);
                let corner = corners[if side > 0.0 { 2 } else { 1 }];
                for dz in [-0.9f32, 0.9] {
                    r.rod(corner + Vec3::Z * (s.z + dz), mount, 0.07, Paint::Gunmetal);
                    r.rod(corners[0] + Vec3::Z * (s.z + dz), mount, 0.05, Paint::Metal);
                }
                // A trunnion ring round the ball.
                r.m.paint(
                    &torus(0.12, 20, 6),
                    Mat4::from_translation(mount)
                        * Mat4::from_rotation_x(FRAC_PI_2)
                        * Mat4::from_scale(Vec3::splat(0.55 * k)),
                    1.0,
                    Paint::Metal,
                );
                r.ball_gun(mount, s.z - 2.4 * k, module, k);
                if is_laser(module) {
                    // A capacitor bank for the laser.
                    let c = Vec3::new(side * 1.7, -0.6, s.z + 1.6);
                    r.rod(c - Vec3::Z * 0.8, c + Vec3::Z * 0.8, 0.3, Paint::Metal);
                    r.block(
                        c + Vec3::new(0.0, 0.31, 0.0),
                        Vec3::new(0.2, 0.04, 1.0),
                        0.6,
                        Paint::Red,
                    );
                } else {
                    // Ammunition cans clamped beside the keel.
                    for (i, dz) in [0.9f32, 1.9].into_iter().enumerate() {
                        r.block(
                            Vec3::new(side * 1.55, -0.7, s.z + dz),
                            Vec3::new(0.55, 0.6, 0.8),
                            0.9 + 0.1 * i as f32,
                            Paint::Olive,
                        );
                    }
                }
            }
            SlotKind::Weapon => {
                // A pylon turret above or below the keel.
                let dir = s.y.signum();
                let root = Vec3::new(0.0, if dir > 0.0 { KEEL_R } else { -0.62 }, s.z);
                let top = Vec3::new(0.0, s.y * 0.85, s.z);
                r.rod(root, top, 0.12, Paint::Gunmetal);
                r.rod(root + Vec3::Z * 1.2, top, 0.05, Paint::Metal);
                r.block(top, Vec3::new(0.7, 0.35, 0.9), 1.0, Paint::Gunmetal);
                gun(
                    &mut r.m,
                    top + Vec3::Y * dir * 0.1,
                    s.z - 1.8 * k,
                    is_laser(module),
                    k * KIT,
                );
            }
            SlotKind::Radiator => {
                r.radiator(Vec3::new(0.0, KEEL_R, s.z), Vec3::Y, k, 1.9, 2.2);
                r.radiator(Vec3::new(0.0, -0.6, s.z), Vec3::NEG_Y, k, 1.5, 2.2);
            }
            _ => {}
        }
    }
    // Empty gun positions still show their outrigger stubs.
    for (slot, module) in p.fitted.slots() {
        if module.is_none() && slot.kind == SlotKind::Weapon && slot.position_m[0].abs() > 0.5 {
            let x = slot.position_m[0] as f32;
            let z = p.centre - slot.position_m[2] as f32;
            let corner = corners[if x > 0.0 { 2 } else { 1 }];
            r.rod(
                corner + Vec3::Z * z,
                Vec3::new(x * 0.7, 0.0, z),
                0.07,
                Paint::Gunmetal,
            );
            r.ball(Vec3::new(x * 0.7, 0.0, z), 0.15, Paint::Metal);
        }
    }
}

/// Shield ring: a thin hoop on six spokes, rigged back to the keel with
/// tension wires, the emitters riding on it top and bottom.
fn shield_ring(r: &mut Runner, p: &Placed) {
    r.keel(p.front, p.back);
    let (z, rr) = (p.centre - 0.5, 3.3f32);
    r.hoop(z, rr, 0.09, 0.0, Paint::Gunmetal);
    r.hoop(z + 0.25, rr, 0.05, 0.0, Paint::Metal);
    for i in 0..6 {
        let a = (i as f32 / 6.0) * TAU + FRAC_PI_2;
        let d = Vec3::new(a.cos(), a.sin(), 0.0);
        r.rod(
            d * 0.7 + Vec3::Z * z,
            d * rr + Vec3::Z * z,
            0.07,
            Paint::Gunmetal,
        );
        for dz in [-3.0f32, 3.0] {
            r.rod_s(
                d * rr + Vec3::Z * z,
                d * 0.9 + Vec3::Z * (z + dz),
                0.016,
                1.5,
                Paint::Metal,
            );
        }
        if i % 2 == 1 {
            r.running_light(d * (rr + 0.12) + Vec3::Z * z);
        }
    }
    for (slot, module, s) in p.modules() {
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Shield => {
                let c = Vec3::new(0.0, s.y.signum() * (rr + 0.15), z);
                r.block(c * 0.97, Vec3::new(0.7, 0.2, 0.7), 1.0, Paint::Metal);
                shield(&mut r.m, c, k * KIT);
            }
            SlotKind::Power => {
                // A small armoured ball reactor caged in the keel.
                r.m.paint(
                    &icosphere(1).flat(),
                    at(Vec3::new(0.0, 0.0, s.z + 1.6), 0.95),
                    1.0,
                    Paint::Gunmetal,
                );
                r.hoop(s.z + 1.6, 0.97, 0.08, 0.0, Paint::Foil);
            }
            SlotKind::Radiator => {
                for side in [-1.0f32, 1.0] {
                    r.radiator(
                        Vec3::new(side * 0.9, -0.3, s.z + 0.5),
                        Vec3::new(side, 0.0, 0.0),
                        k,
                        1.6,
                        1.8,
                    );
                }
            }
            _ => {}
        }
    }
}

/// A cluster of four slim pods clamped in the cage at `z`.
fn pod_cluster(r: &mut Runner, z: f32, k: f32, n: usize) {
    let len = 4.4 * (k / 1.4).min(1.0);
    let (h, pr) = (len / 2.0, 0.6);
    let profile = [
        (-h, 0.0),
        (-h + 0.1, 0.35),
        (-h + 0.3, 0.55),
        (-h + 0.6, pr),
        (h - 0.6, pr),
        (h - 0.3, 0.55),
        (h - 0.1, 0.35),
        (h, 0.0),
    ];
    let pod = lathe(&profile, 10, 0.0, TAU).flat();
    let paints = [
        (Paint::Gunmetal, 1.0),
        (Paint::Olive, 0.8),
        (Paint::Gunmetal, 0.7),
        (Paint::Gunmetal, 1.3),
    ];
    for (i, (x, y)) in [
        (-0.82f32, 0.75f32),
        (0.82, 0.75),
        (-0.82, -0.75),
        (0.82, -0.75),
    ]
    .into_iter()
    .enumerate()
    {
        let c = Vec3::new(x, y, z);
        let (paint, shade) = paints[(i + n * 3) % 4];
        r.m.paint(&pod, Mat4::from_translation(c), shade, paint);
        for dz in [-h * 0.5, h * 0.5] {
            let band = [(z + dz - 0.1, pr + 0.04), (z + dz + 0.1, pr + 0.04)];
            r.m.paint(
                &lathe(&band, 10, 0.0, TAU).flat(),
                Mat4::from_translation(Vec3::new(x, y, 0.0)),
                1.0,
                Paint::Metal,
            );
            // A clamp arm out to the cage's corner.
            let corner = Vec3::new(x.signum() * 1.85, y.signum() * 1.7, z + dz);
            let rim = c + Vec3::new(x.signum(), y.signum(), 0.0).normalize() * pr + Vec3::Z * dz;
            r.rod(rim, corner, 0.06, Paint::Metal);
        }
    }
}

/// Cargo bay: an open lattice cage, slim pods clamped inside it in fours.
fn cargo_bay(r: &mut Runner, p: &Placed) {
    let (cx, cy) = (1.85f32, 1.7f32);
    let corners = [
        Vec3::new(-cx, -cy, 0.0),
        Vec3::new(cx, -cy, 0.0),
        Vec3::new(cx, cy, 0.0),
        Vec3::new(-cx, cy, 0.0),
    ];
    let half = p.len() / 2.0;
    let stations = [
        p.front + 0.15,
        p.centre - half * 0.31,
        p.centre + half * 0.31,
        p.back - 0.15,
    ];
    for c in corners {
        r.rod(
            c + Vec3::Z * p.front,
            c + Vec3::Z * p.back,
            0.1,
            Paint::Gunmetal,
        );
    }
    for (i, &z) in stations.iter().enumerate() {
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            r.rod(a + Vec3::Z * z, b + Vec3::Z * z, 0.075, Paint::Gunmetal);
            if let Some(&zn) = stations.get(i + 1) {
                // X-bracing on every face.
                r.rod_s(a + Vec3::Z * z, b + Vec3::Z * zn, 0.032, 0.8, Paint::Metal);
                r.rod_s(b + Vec3::Z * z, a + Vec3::Z * zn, 0.032, 0.8, Paint::Metal);
            }
        }
    }
    // The end frames take the keel's loads into the cage.
    let kc = keel_corners();
    for (z, kz) in [(stations[0], p.front), (stations[3], p.back)] {
        for (i, c) in kc.iter().enumerate() {
            let target = corners[[2, 0, 1][i]];
            r.rod(
                *c + Vec3::Z * kz,
                target + Vec3::Z * z,
                0.06,
                Paint::Gunmetal,
            );
        }
        r.rod(
            kc[0] + Vec3::Z * kz,
            corners[3] + Vec3::Z * z,
            0.06,
            Paint::Gunmetal,
        );
    }
    // A thin spine pipe down the middle, between the pods.
    r.rod_s(
        Vec3::new(0.0, 0.0, p.front),
        Vec3::new(0.0, 0.0, p.back),
        0.16,
        1.3,
        Paint::Metal,
    );
    for c in [corners[2], corners[3]] {
        r.running_light(c + Vec3::new(c.x.signum() * 0.1, 0.12, p.front + 0.2));
    }
    r.number(
        Vec3::new(cx + 0.11, -0.4, stations[2] - 0.3),
        Vec3::NEG_Z,
        Vec3::Y,
        0.5,
    );

    let mut n = 0;
    for (slot, module) in p.fitted.slots() {
        let z = p.centre - slot.position_m[2] as f32;
        match module {
            Some(module) if slot.kind == SlotKind::Cargo => {
                pod_cluster(r, z, scale(module.size), n);
                n += 1;
            }
            None if slot.kind == SlotKind::Cargo => {
                // Bare clamp saddles.
                for c in corners {
                    for dz in [-1.1f32, 1.1] {
                        r.block(
                            c * 0.88 + Vec3::Z * (z + dz),
                            Vec3::new(0.3, 0.3, 0.2),
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

/// Reactor section: the one place the runner is properly plated. Armoured
/// drums round the keel, radiator wings out on spars.
fn reactor_section(r: &mut Runner, p: &Placed) {
    r.keel(p.front, p.back);
    for (slot, module) in p.fitted.slots() {
        let z = p.centre - slot.position_m[2] as f32;
        match (slot.kind, module) {
            (SlotKind::Power, Some(m)) => {
                let k = scale(m.size) / 1.4;
                r.reactor_drum(z, 1.95 * k.max(0.75), 3.3);
            }
            (SlotKind::Power, None) => r.empty_saddle(z),
            _ => {}
        }
    }
    // Pipes from the drums out along the spars.
    for (slot, module, s) in p.modules() {
        if slot.kind != SlotKind::Radiator {
            continue;
        }
        let k = scale(module.size);
        let side = s.x.signum();
        let root = Vec3::new(side * 2.3, 0.0, s.z);
        let corner = keel_corners()[if side > 0.0 { 2 } else { 1 }];
        for dz in [-1.6f32, 1.6] {
            r.rod(
                corner + Vec3::Z * (s.z + dz),
                root + Vec3::Z * dz * 0.6,
                0.07,
                Paint::Gunmetal,
            );
        }
        r.rod(
            Vec3::new(side * 1.0, 0.6, s.z),
            root + Vec3::Y * 0.3,
            0.06,
            Paint::Metal,
        );
        r.radiator(root, Vec3::new(side, 0.0, 0.0), k, 2.6, 2.3);
        r.running_light(root + Vec3::new(side * 2.6 * k + 0.1, 0.1, 0.0));
    }
}

// --- Sterns ---------------------------------------------------------------

/// Sprint stern: twin oversized bells on an open thrust frame, radiator
/// vanes above and below like tail fins.
fn sprint_stern(r: &mut Runner, p: &Placed) {
    let plate = p.back - 2.3;
    r.keel(p.front, plate - 2.0);
    // The thrust frame: a box of tubes from the keel's end out to a
    // crossbeam frame behind.
    let (fx, fy) = (3.0f32, 1.25f32);
    let frame = [
        Vec3::new(-fx, -fy, plate),
        Vec3::new(fx, -fy, plate),
        Vec3::new(fx, fy, plate),
        Vec3::new(-fx, fy, plate),
    ];
    for k in 0..4 {
        r.rod(frame[k], frame[(k + 1) % 4], 0.1, Paint::Gunmetal);
    }
    r.rod(frame[0], frame[2], 0.05, Paint::Metal);
    r.rod(frame[1], frame[3], 0.05, Paint::Metal);
    let kc = keel_corners();
    let kz = plate - 2.0;
    for c in frame {
        for (i, k) in kc.iter().enumerate() {
            let hit = match i {
                0 => c.y > 0.0,
                1 => c.x < 0.0,
                _ => c.x > 0.0,
            };
            if hit {
                r.rod(*k + Vec3::Z * kz, c, 0.08, Paint::Gunmetal);
            }
        }
    }

    for (slot, module) in p.fitted.slots() {
        let [x, y, z] = slot.position_m.map(|v| v as f32);
        let s = Vec3::new(x, y, p.centre - z);
        let Some(module) = module else {
            if slot.kind == SlotKind::Engine {
                // A bare gimbal ring with nothing in it.
                r.hoop(plate + 0.4, 0.55, 0.06, 0.0, Paint::Metal);
            }
            continue;
        };
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(s.x, s.y, plate + 0.6);
                engine_mount(r, throat, &frame, k);
                let n = bell(&mut r.m, throat, 2.0 * k, 0.62 * k);
                r.nozzles.push(n);
            }
            SlotKind::Power => r.reactor_drum(s.z - 1.6, 1.5, 2.2),
            SlotKind::Radiator => {
                let dir = s.y.signum();
                let root = Vec3::new(0.0, if dir > 0.0 { KEEL_R } else { -0.62 }, s.z);
                r.radiator(root, Vec3::new(0.0, dir, 0.25), k, 1.9, 2.6);
            }
            SlotKind::JumpDrive => r.jump_coils(s.z + 0.6, k),
            _ => {}
        }
    }
    for side in [-1.0f32, 1.0] {
        r.running_light(Vec3::new(side * (fx + 0.12), 0.0, plate));
    }
}

/// An engine's gimbal ring at `throat`, hung from the corners of `frame`
/// by four struts, with a turbopump beside it.
fn engine_mount(r: &mut Runner, throat: Vec3, frame: &[Vec3], k: f32) {
    let ring = 0.62 * k * 0.95;
    r.m.paint(
        &torus(0.14, 24, 6),
        Mat4::from_translation(throat)
            * Mat4::from_rotation_x(FRAC_PI_2)
            * Mat4::from_scale(Vec3::splat(ring)),
        1.0,
        Paint::Gunmetal,
    );
    let mut near: Vec<Vec3> = frame.to_vec();
    near.sort_by(|a, b| a.distance(throat).total_cmp(&b.distance(throat)));
    for c in near.iter().take(2) {
        r.rod(
            *c,
            throat + (*c - throat).with_z(0.0).normalize() * ring,
            0.06,
            Paint::Metal,
        );
    }
    for a in [0.4f32, 2.7, 3.9, 5.5] {
        let d = Vec3::new(a.cos(), a.sin(), 0.0);
        r.rod(
            throat + d * ring,
            Vec3::new(throat.x * 0.5, 0.0, throat.z - 2.0) + d * 0.5,
            0.045,
            Paint::Metal,
        );
    }
    let pump = throat + Vec3::new(throat.x.signum() * 0.0, 0.9 * k / 1.4, -0.9);
    r.rod(
        pump - Vec3::Z * 0.5,
        pump + Vec3::Z * 0.5,
        0.25,
        Paint::Metal,
    );
    r.rod(
        pump + Vec3::Z * 0.4,
        throat + Vec3::Y * ring,
        0.07,
        Paint::Metal,
    );
}

/// Cruise stern: one big bell in a cone of struts, spherical propellant
/// tanks slung either side of the keel, radiator wings.
fn cruise_stern(r: &mut Runner, p: &Placed) {
    let kz = p.back - 3.6;
    r.keel(p.front, kz);
    // The tanks, on short struts, with fill lines.
    let tz = p.centre - 1.0;
    for side in [-1.0f32, 1.0] {
        let c = Vec3::new(side * 2.05, -0.1, tz);
        r.m.paint(&icosphere(2), at(c, 1.15), 1.0, Paint::Gunmetal);
        r.m.paint(
            &torus(0.06, 28, 6),
            Mat4::from_translation(c) * Mat4::from_scale(Vec3::splat(1.17)),
            1.0,
            Paint::Metal,
        );
        let corner = keel_corners()[if side > 0.0 { 2 } else { 1 }];
        for dz in [-0.8f32, 0.8] {
            r.rod(
                corner + Vec3::Z * (tz + dz),
                c + Vec3::Z * dz * 0.8,
                0.07,
                Paint::Gunmetal,
            );
        }
        r.rod(
            c + Vec3::new(-side * 0.3, -0.9, 0.6),
            Vec3::new(side * 0.4, -0.25, kz + 1.5),
            0.06,
            Paint::Metal,
        );
    }

    for (slot, module) in p.fitted.slots() {
        let [x, y, z] = slot.position_m.map(|v| v as f32);
        let s = Vec3::new(x, y, p.centre - z);
        let Some(module) = module else { continue };
        let k = scale(module.size);
        match slot.kind {
            SlotKind::Engine => {
                let throat = Vec3::new(s.x, s.y, p.back - 1.4);
                let ring = 0.85 * k * 0.9;
                r.m.paint(
                    &torus(0.12, 28, 6),
                    Mat4::from_translation(throat)
                        * Mat4::from_rotation_x(FRAC_PI_2)
                        * Mat4::from_scale(Vec3::splat(ring)),
                    1.0,
                    Paint::Gunmetal,
                );
                // The thrust cone: six struts from the keel's end.
                for i in 0..6 {
                    let a = i as f32 / 6.0 * TAU + FRAC_PI_2;
                    let d = Vec3::new(a.cos(), a.sin(), 0.0);
                    let kc = keel_corners()[i / 2];
                    r.rod(kc + Vec3::Z * kz, throat + d * ring, 0.065, Paint::Gunmetal);
                }
                r.reactor_drum(kz + 1.1, 0.9 * k / 1.4, 1.2);
                let n = bell(&mut r.m, throat, 2.4 * k, 0.85 * k);
                r.nozzles.push(n);
            }
            SlotKind::Power => r.reactor_drum(s.z - 3.0, 1.45, 2.0),
            SlotKind::Radiator => {
                let side = s.x.signum();
                let root = Vec3::new(side * 3.1, -0.1, s.z + 0.3);
                r.rod(
                    Vec3::new(side * 1.0, -0.6, s.z - 1.0),
                    root + Vec3::Z * -1.0,
                    0.07,
                    Paint::Gunmetal,
                );
                r.radiator(root, Vec3::new(side, -0.15, 0.0), k, 2.2, 2.6);
                r.running_light(root + Vec3::new(side * 3.1 * k / 1.4, -0.3, 0.0));
            }
            SlotKind::JumpDrive => r.jump_coils(s.z - 0.2, k),
            _ => {}
        }
    }
}
