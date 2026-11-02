//! Procedural meshes: spheres, rocks, ships generated from their fits,
//! stations built from a kit of modules, and gates.
//!
//! The look is "orbital hardware": cylinders, spheres, trusses and panels,
//! flat-shaded so the facets read as built things.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec2, Vec3};
use retrox_gen::Rng;
use retrox_sim::economy::FacilityKind;

/// What a station looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StationKind {
    /// Where people live: a habitat ring.
    Habitat,
    /// Shaped by its main industry.
    Facility(FacilityKind),
    /// A small frontier stop.
    Outpost,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Per-vertex tint multiplier (lets one mesh have panels of different shades).
    pub shade: f32,
    /// A [`Paint`] finish, as a float (vertex attributes are floats).
    pub paint: f32,
}

/// Surface finishes for built things; must match `mesh.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Paint {
    /// The instance colour (enamelled hull plating).
    Hull = 0,
    /// Signal red livery.
    Red = 1,
    /// Dark glazing with a warm lit interior.
    Glass = 2,
    /// Solar cells: deep blue with a fine grid.
    Solar = 3,
    /// Gold thermal blanket.
    Foil = 4,
    /// Hull plating with rows of lit windows.
    Windows = 5,
    /// Bare dark metal (nozzles, machinery).
    Metal = 6,
    /// A blinking red beacon.
    Beacon = 7,
    /// A steady work lamp.
    Lamp = 8,
    /// Mustard-ochre work enamel.
    Ochre = 9,
    /// Black-and-yellow hazard stripes.
    Hazard = 10,
    /// Deep teal-grey military enamel.
    Gunmetal = 11,
    /// Olive drab.
    Olive = 12,
    /// Sky-blue racing enamel.
    Sky = 13,
}

#[derive(Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Append `other`, transformed.
    pub fn add(&mut self, other: &Mesh, transform: Mat4, shade: f32) {
        self.add_painted(other, transform, shade, None);
    }

    /// Append `other`, transformed, with every vertex given `paint`.
    pub fn paint(&mut self, other: &Mesh, transform: Mat4, shade: f32, paint: Paint) {
        self.add_painted(other, transform, shade, Some(paint));
    }

    fn add_painted(&mut self, other: &Mesh, transform: Mat4, shade: f32, paint: Option<Paint>) {
        let base = self.vertices.len() as u32;
        let normal_m = transform.inverse().transpose();
        for v in &other.vertices {
            let pos = transform.transform_point3(Vec3::from(v.pos));
            let normal = normal_m
                .transform_vector3(Vec3::from(v.normal))
                .normalize_or_zero();
            self.vertices.push(Vertex {
                pos: pos.into(),
                normal: normal.into(),
                shade: v.shade * shade,
                paint: paint.map_or(v.paint, |p| p as u32 as f32),
            });
        }
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Duplicate vertices per triangle with face normals, for a faceted look.
    pub fn flat(&self) -> Mesh {
        let mut out = Mesh::default();
        for tri in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| self.vertices[i as usize]);
            let (pa, pb, pc) = (Vec3::from(a.pos), Vec3::from(b.pos), Vec3::from(c.pos));
            let n = (pb - pa).cross(pc - pa).normalize_or_zero();
            for v in [a, b, c] {
                out.indices.push(out.vertices.len() as u32);
                out.vertices.push(Vertex {
                    normal: n.into(),
                    ..v
                });
            }
        }
        out
    }
}

/// A unit icosphere (radius 1), smooth normals.
pub fn icosphere(subdivisions: u32) -> Mesh {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut pos: Vec<Vec3> = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ]
    .iter()
    .map(|p| Vec3::from(*p).normalize())
    .collect();
    let mut tris: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut midpoints: HashMap<(u32, u32), u32> = HashMap::new();
        let mut mid = |a: u32, b: u32, pos: &mut Vec<Vec3>| -> u32 {
            let key = (a.min(b), a.max(b));
            *midpoints.entry(key).or_insert_with(|| {
                pos.push(((pos[a as usize] + pos[b as usize]) / 2.0).normalize());
                pos.len() as u32 - 1
            })
        };
        let mut next = Vec::with_capacity(tris.len() * 4);
        for [a, b, c] in tris {
            let ab = mid(a, b, &mut pos);
            let bc = mid(b, c, &mut pos);
            let ca = mid(c, a, &mut pos);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
    }
    Mesh {
        vertices: pos
            .iter()
            .map(|p| Vertex {
                pos: (*p).into(),
                normal: (*p).into(),
                shade: 1.0,
                paint: 0.0,
            })
            .collect(),
        indices: tris.into_iter().flatten().collect(),
    }
}

/// A lumpy, faceted rock of roughly unit radius.
pub fn rock(seed: u64) -> Mesh {
    let mut rng = Rng::new(seed);
    let mut sphere = icosphere(2);
    // A few random "dents and bulges" plus overall squash.
    let bumps: Vec<(Vec3, f32)> = (0..7)
        .map(|_| {
            let dir = Vec3::new(
                rng.range(-1.0, 1.0) as f32,
                rng.range(-1.0, 1.0) as f32,
                rng.range(-1.0, 1.0) as f32,
            )
            .normalize_or(Vec3::Y);
            (dir, rng.range(-0.35, 0.3) as f32)
        })
        .collect();
    let squash = Vec3::new(
        rng.range(0.7, 1.2) as f32,
        rng.range(0.5, 0.9) as f32,
        rng.range(0.7, 1.3) as f32,
    );
    for v in &mut sphere.vertices {
        let p = Vec3::from(v.pos);
        let mut r = 1.0;
        for (dir, amount) in &bumps {
            r += amount * p.dot(*dir).max(0.0).powi(3);
        }
        v.pos = (p * r * squash).into();
    }
    sphere.flat()
}

/// A cylinder along +Y from y=0 to y=1, radius 1, capped.
pub fn cylinder(segments: u32) -> Mesh {
    let mut m = Mesh::default();
    let ring = |y: f32| -> Vec<Vec3> {
        (0..segments)
            .map(|i| {
                let a = i as f32 / segments as f32 * TAU;
                Vec3::new(a.cos(), y, a.sin())
            })
            .collect()
    };
    let (bottom, top) = (ring(0.0), ring(1.0));
    for i in 0..segments as usize {
        let j = (i + 1) % segments as usize;
        quad(&mut m, bottom[i], bottom[j], top[j], top[i]);
        tri(&mut m, Vec3::new(0.0, 1.0, 0.0), top[i], top[j]);
        tri(&mut m, Vec3::ZERO, bottom[j], bottom[i]);
    }
    m
}

/// A unit cube centred on the origin.
pub fn cube() -> Mesh {
    let mut m = Mesh::default();
    let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * 0.5;
    let faces = [
        [
            c(-1., -1., 1.),
            c(1., -1., 1.),
            c(1., 1., 1.),
            c(-1., 1., 1.),
        ],
        [
            c(1., -1., -1.),
            c(-1., -1., -1.),
            c(-1., 1., -1.),
            c(1., 1., -1.),
        ],
        [
            c(1., -1., 1.),
            c(1., -1., -1.),
            c(1., 1., -1.),
            c(1., 1., 1.),
        ],
        [
            c(-1., -1., -1.),
            c(-1., -1., 1.),
            c(-1., 1., 1.),
            c(-1., 1., -1.),
        ],
        [
            c(-1., 1., 1.),
            c(1., 1., 1.),
            c(1., 1., -1.),
            c(-1., 1., -1.),
        ],
        [
            c(-1., -1., -1.),
            c(1., -1., -1.),
            c(1., -1., 1.),
            c(-1., -1., 1.),
        ],
    ];
    for [a, b, cc, d] in faces {
        quad(&mut m, a, b, cc, d);
    }
    m
}

/// A torus in the XZ plane: major radius 1, minor radius `minor`.
pub fn torus(minor: f32, segments: u32, sides: u32) -> Mesh {
    let mut m = Mesh::default();
    let point = |i: u32, j: u32| -> Vec3 {
        let (u, v) = (
            i as f32 / segments as f32 * TAU,
            j as f32 / sides as f32 * TAU,
        );
        let r = 1.0 + minor * v.cos();
        Vec3::new(r * u.cos(), minor * v.sin(), r * u.sin())
    };
    for i in 0..segments {
        for j in 0..sides {
            quad(
                &mut m,
                point(i, j),
                point(i + 1, j),
                point(i + 1, j + 1),
                point(i, j + 1),
            );
        }
    }
    m
}

pub(crate) fn tri(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3) {
    let n = (b - a).cross(c - a).normalize_or_zero();
    for p in [a, b, c] {
        m.indices.push(m.vertices.len() as u32);
        m.vertices.push(Vertex {
            pos: p.into(),
            normal: n.into(),
            shade: 1.0,
            paint: 0.0,
        });
    }
}

pub(crate) fn quad(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    tri(m, a, b, c);
    tri(m, a, c, d);
}

/// Transform for a cylinder from `from` to `to` with the given radius.
pub(crate) fn strut(from: Vec3, to: Vec3, radius: f32) -> Mat4 {
    let dir = to - from;
    let rot = Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y));
    Mat4::from_scale_rotation_translation(Vec3::new(radius, dir.length(), radius), rot, from)
}

pub(crate) fn boxed(center: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center)
}

/// A surface of revolution about the Z axis through `profile` points
/// `(z, r)`, swept from angle `a0` to `a1` (radians, 0 = +X, TAU/4 = +Y).
/// Smooth around and along the profile; repeat a point for a hard crease.
pub fn lathe(profile: &[(f32, f32)], segments: u32, a0: f32, a1: f32) -> Mesh {
    let n = profile.len();
    // Normal of each profile segment in the (z, r) plane, as (nz, nr).
    let seg_normal = |i: usize| -> Option<(f32, f32)> {
        let (z0, r0) = profile[i];
        let (z1, r1) = profile[i + 1];
        let (dz, dr) = (z1 - z0, r1 - r0);
        let len = (dz * dz + dr * dr).sqrt();
        (len > 1e-6).then(|| (-dr / len, dz / len))
    };
    // Per profile point, the normal on each side of it (they differ at creases).
    let mut m = Mesh::default();
    let ring = |a: f32| (a.cos(), a.sin());
    for i in 0..n - 1 {
        let Some(own) = seg_normal(i) else { continue };
        let blend = |other: Option<(f32, f32)>| {
            other.map_or(own, |o| {
                let (z, r) = (own.0 + o.0, own.1 + o.1);
                let l = (z * z + r * r).sqrt().max(1e-6);
                (z / l, r / l)
            })
        };
        let n0 = blend(if i > 0 { seg_normal(i - 1) } else { None });
        let n1 = blend(if i + 2 < n { seg_normal(i + 1) } else { None });
        for j in 0..segments {
            let (ca, sa) = ring(a0 + (a1 - a0) * j as f32 / segments as f32);
            let (cb, sb) = ring(a0 + (a1 - a0) * (j + 1) as f32 / segments as f32);
            let v = |(z, r): (f32, f32), (c, s): (f32, f32), (nz, nr): (f32, f32)| Vertex {
                pos: [r * c, r * s, z],
                normal: [nr * c, nr * s, nz],
                shade: 1.0,
                paint: 0.0,
            };
            let base = m.vertices.len() as u32;
            m.vertices.extend([
                v(profile[i], (ca, sa), n0),
                v(profile[i], (cb, sb), n0),
                v(profile[i + 1], (cb, sb), n1),
                v(profile[i + 1], (ca, sa), n1),
            ]);
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    m
}

/// A fin with a diamond cross-section, in its own frame: the root lies
/// along Z at y = 0, the span runs up +Y, thickness is along X. `root` and
/// `tip` are (leading edge z, trailing edge z); `y` is the span range to
/// build, as fractions of `span` (so a fin can be painted in bands).
pub(crate) fn fin(root: (f32, f32), tip: (f32, f32), span: f32, thick: f32, y: (f32, f32)) -> Mesh {
    let section = |f: f32| -> [Vec3; 4] {
        let le = root.0 + (tip.0 - root.0) * f;
        let te = root.1 + (tip.1 - root.1) * f;
        let t = thick * (1.0 - 0.6 * f);
        let yy = span * f;
        [
            Vec3::new(0.0, yy, le),
            Vec3::new(t, yy, le + (te - le) * 0.35),
            Vec3::new(0.0, yy, te),
            Vec3::new(-t, yy, le + (te - le) * 0.35),
        ]
    };
    let (a, b) = (section(y.0), section(y.1));
    let mut m = Mesh::default();
    for k in 0..4 {
        let l = (k + 1) % 4;
        quad(&mut m, a[k], a[l], b[l], b[k]);
    }
    for s in [a, b] {
        quad(&mut m, s[0], s[1], s[2], s[3]);
    }
    m
}

/// A closed solid swept through cross-sections along Z, flat-shaded.
/// Every section has the same number of points (wound counter-clockwise
/// seen from -Z). `paint(segment, edge)` paints each side face; the end
/// caps get `caps`.
pub(crate) fn loft(
    sections: &[(f32, Vec<Vec2>)],
    paint: impl Fn(usize, usize) -> Paint,
    caps: Paint,
) -> Mesh {
    let mut m = Mesh::default();
    let at = |(z, pts): &(f32, Vec<Vec2>), k: usize| {
        let p = pts[k % pts.len()];
        Vec3::new(p.x, p.y, *z)
    };
    let painted = |m: &mut Mesh, from: usize, p: Paint| {
        for v in &mut m.vertices[from..] {
            v.paint = p as u32 as f32;
        }
    };
    for (s, pair) in sections.windows(2).enumerate() {
        let n = pair[0].1.len();
        for e in 0..n {
            let from = m.vertices.len();
            quad(
                &mut m,
                at(&pair[0], e),
                at(&pair[1], e),
                at(&pair[1], e + 1),
                at(&pair[0], e + 1),
            );
            painted(&mut m, from, paint(s, e));
        }
    }
    for (end, flip) in [(&sections[0], false), (&sections[sections.len() - 1], true)] {
        let n = end.1.len();
        let centre = (0..n).map(|k| at(end, k)).sum::<Vec3>() / n as f32;
        let from = m.vertices.len();
        for k in 0..n {
            let (a, b) = (at(end, k), at(end, k + 1));
            if flip {
                tri(&mut m, centre, a, b);
            } else {
                tri(&mut m, centre, b, a);
            }
        }
        painted(&mut m, from, caps);
    }
    m
}

/// A `w` × `h` rectangle with its corners cut by `c`, centred at height
/// `y`: eight points, starting at the left end of the top edge, so edge 0
/// is the top, 2 the right side, 4 the bottom and 6 the left side.
pub(crate) fn chamfer_rect(w: f32, h: f32, c: f32, y: f32) -> Vec<Vec2> {
    let (x, t) = (w / 2.0, h / 2.0);
    [
        (-x + c, t),
        (x - c, t),
        (x, t - c),
        (x, -t + c),
        (x - c, -t),
        (-x + c, -t),
        (-x, -t + c),
        (-x, t - c),
    ]
    .into_iter()
    .map(|(px, py)| Vec2::new(px, py + y))
    .collect()
}

/// A station from the module kit, sized to roughly unit radius (scale by
/// the station's size at draw time). Shape depends on kind and seed.
pub fn station(kind: StationKind, seed: u64) -> Mesh {
    let mut rng = Rng::new(seed);
    let (cyl, cube, ball) = (cylinder(14), cube(), icosphere(1).flat());
    let mut m = Mesh::default();
    let r = |rng: &mut Rng, lo: f32, hi: f32| rng.range(lo as f64, hi as f64) as f32;

    // Spine: a stack of pressurised modules along Y.
    let modules = 2 + rng.below(4);
    let mut y = -0.6;
    for _ in 0..modules {
        let len = r(&mut rng, 0.15, 0.32);
        let rad = r(&mut rng, 0.06, 0.12);
        let paint = if rng.below(2) == 0 {
            Paint::Windows
        } else {
            Paint::Hull
        };
        m.paint(
            &cyl,
            strut(Vec3::new(0.0, y, 0.0), Vec3::new(0.0, y + len, 0.0), rad),
            1.0,
            paint,
        );
        // A red band at the top of each module.
        m.paint(
            &cyl,
            strut(
                Vec3::new(0.0, y + len * 0.82, 0.0),
                Vec3::new(0.0, y + len * 0.92, 0.0),
                rad * 1.04,
            ),
            1.0,
            Paint::Red,
        );
        // Docking node between modules.
        m.add(
            &ball,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(rad * 1.1),
                Quat::IDENTITY,
                Vec3::new(0.0, y + len, 0.0),
            ),
            0.85,
        );
        y += len + 0.02;
    }

    match kind {
        StationKind::Habitat => {
            // A rotating habitat ring on spokes.
            let ring_r = r(&mut rng, 0.55, 0.8);
            m.paint(
                &torus(0.08, 48, 10),
                Mat4::from_scale(Vec3::splat(ring_r)),
                0.95,
                Paint::Windows,
            );
            for i in 0..4 {
                let a = i as f32 / 4.0 * TAU;
                let end = Vec3::new(a.cos(), 0.0, a.sin()) * ring_r;
                m.paint(&cyl, strut(Vec3::ZERO, end, 0.025), 1.0, Paint::Metal);
            }
        }
        StationKind::Facility(FacilityKind::Mine | FacilityKind::IceHarvester) => {
            // Gantries and ore hoppers.
            for i in 0..3 {
                let a = i as f32 / 3.0 * TAU;
                let out = Vec3::new(a.cos(), r(&mut rng, -0.4, 0.4), a.sin()) * 0.7;
                m.paint(&cyl, strut(Vec3::ZERO, out, 0.03), 1.0, Paint::Metal);
                m.add(&cube, boxed(out, Vec3::splat(0.22)), 0.75);
            }
        }
        StationKind::Facility(FacilityKind::Refinery | FacilityKind::FuelPlant) => {
            // Spherical tanks around the spine.
            for i in 0..4 {
                let a = i as f32 / 4.0 * TAU + 0.4;
                let p = Vec3::new(a.cos() * 0.35, r(&mut rng, -0.4, 0.3), a.sin() * 0.35);
                m.paint(
                    &ball,
                    Mat4::from_scale_rotation_translation(Vec3::splat(0.17), Quat::IDENTITY, p),
                    1.0,
                    Paint::Foil,
                );
            }
        }
        StationKind::Facility(FacilityKind::Hydroponics) => {
            // Long greenhouse arms.
            for side in [-1.0f32, 1.0] {
                m.paint(
                    &cyl,
                    strut(Vec3::ZERO, Vec3::new(side * 0.9, 0.0, 0.0), 0.07),
                    1.0,
                    Paint::Windows,
                );
            }
        }
        StationKind::Facility(FacilityKind::Factory | FacilityKind::Shipyard) => {
            // Blocky works.
            for _ in 0..3 {
                let p = Vec3::new(
                    r(&mut rng, -0.35, 0.35),
                    r(&mut rng, -0.5, 0.4),
                    r(&mut rng, -0.35, 0.35),
                );
                m.paint(
                    &cube,
                    boxed(
                        p,
                        Vec3::new(
                            r(&mut rng, 0.2, 0.4),
                            r(&mut rng, 0.15, 0.3),
                            r(&mut rng, 0.2, 0.4),
                        ),
                    ),
                    0.8,
                    Paint::Windows,
                );
            }
        }
        StationKind::Outpost => {}
    }

    // Solar wings on a truss.
    let wing_y = r(&mut rng, -0.3, 0.3);
    m.paint(
        &cyl,
        strut(
            Vec3::new(-0.95, wing_y, 0.0),
            Vec3::new(0.95, wing_y, 0.0),
            0.02,
        ),
        1.0,
        Paint::Metal,
    );
    for side in [-1.0f32, 1.0] {
        m.paint(
            &cube,
            boxed(
                Vec3::new(side * 0.7, wing_y, 0.0),
                Vec3::new(0.45, 0.01, 0.22),
            ),
            1.0,
            Paint::Solar,
        );
        m.paint(
            &ball,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.012),
                Quat::IDENTITY,
                Vec3::new(side * 0.95, wing_y, 0.0),
            ),
            1.0,
            Paint::Beacon,
        );
    }
    m
}

/// The shape of a generated ship, for placing effects and the camera.
#[derive(Clone, Debug, Default)]
pub struct ShipShape {
    /// Engine nozzles: (position, radius), with the exhaust along +Z.
    pub nozzles: Vec<(Vec3, f32)>,
}

/// A ship's mesh: its hull's hand-built model (see [`crate::hulls`]), or
/// one generated from the fit.
pub fn ship(fit: &retrox_sim::ship::Loadout) -> (Mesh, ShipShape) {
    crate::hulls::authored(fit).unwrap_or_else(|| generated(fit))
}

/// A ship generated from its fit: each section a segment of the spine,
/// nose (-Z) to tail, sized by its mass and length, with its modules where
/// its slots are — engine bells, gun barrels, radiator fins, cargo pods,
/// shield rings, radar masts.
pub fn generated(fit: &retrox_sim::ship::Loadout) -> (Mesh, ShipShape) {
    use retrox_sim::ship::{Position, Size, SlotKind};
    let (cyl, cube, ball) = (cylinder(14), cube(), icosphere(1).flat());
    let ring = torus(0.06, 32, 6);
    let module_m = |size: Size| -> f32 {
        match size {
            Size::Small => 0.8,
            Size::Medium => 2.0,
            Size::Large => 5.0,
        }
    };
    let sections: Vec<_> = fit.mounts().filter_map(|(_, s)| s).collect();
    let length: f32 = sections.iter().map(|s| s.section.length_m as f32).sum();
    let mut m = Mesh::default();
    let mut shape = ShipShape {
        nozzles: Vec::new(),
    };
    let mut front = -length / 2.0;
    for (i, fitted) in sections.iter().enumerate() {
        let section = fitted.section;
        let len = section.length_m as f32;
        let centre = front + len / 2.0;
        let radius = ((section.mass_t / section.length_m) as f32 * 4.0)
            .sqrt()
            .clamp(1.0, 14.0);
        let kinds: Vec<SlotKind> = section.slots.iter().map(|s| s.kind).collect();
        let count = |k| kinds.iter().filter(|&&x| x == k).count();
        // The body: a box for cargo sections, a cylinder otherwise.
        if count(SlotKind::Cargo) * 2 > kinds.len() {
            m.add(
                &cube,
                boxed(
                    Vec3::new(0.0, 0.0, centre),
                    Vec3::new(radius * 1.7, radius * 1.7, len * 0.96),
                ),
                0.95,
            );
        } else {
            m.add(
                &cyl,
                strut(
                    Vec3::new(0.0, 0.0, front + len * 0.02),
                    Vec3::new(0.0, 0.0, front + len * 0.98),
                    radius,
                ),
                1.0,
            );
        }
        // A painted band on each section; red on fighting sections.
        let band = if count(SlotKind::Weapon) > 0 {
            Paint::Red
        } else if count(SlotKind::Power) > 1 {
            Paint::Metal
        } else if i % 2 == 0 {
            Paint::Red
        } else {
            Paint::Foil
        };
        m.paint(
            &cyl,
            strut(
                Vec3::new(0.0, 0.0, centre - len * 0.08),
                Vec3::new(0.0, 0.0, centre + len * 0.08),
                radius * 1.03,
            ),
            1.0,
            band,
        );
        // The flight deck up front.
        if i == 0 && matches!(section.position, Position::Bow | Position::Whole) {
            m.add(
                &ball,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(radius, radius, radius * 1.2),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, front + radius * 0.6),
                ),
                0.95,
            );
            m.paint(
                &cube,
                boxed(
                    Vec3::new(0.0, radius * 0.45, front + radius * 0.1),
                    Vec3::new(radius * 1.1, radius * 0.3, radius * 0.5),
                ),
                1.0,
                Paint::Glass,
            );
        }
        for (slot, module) in fitted.slots() {
            let Some(module) = module else { continue };
            let size = module_m(module.size);
            let [x, y, z] = slot.position_m.map(|v| v as f32);
            let at = Vec3::new(x, y, centre - z);
            let side = if x < 0.0 { -1.0 } else { 1.0 };
            match slot.kind {
                SlotKind::Engine => {
                    let tail = front + len;
                    let r = (size * 0.6).min(radius * 0.9);
                    let nozzle = Vec3::new(x, y, tail);
                    m.paint(
                        &cyl,
                        strut(nozzle, nozzle + Vec3::Z * size * 1.3, r),
                        1.0,
                        Paint::Metal,
                    );
                    shape.nozzles.push((nozzle + Vec3::Z * size * 1.3, r));
                }
                SlotKind::Weapon => {
                    let mount = Vec3::new(x, y.max(radius * 0.8), at.z);
                    m.paint(
                        &cube,
                        boxed(mount, Vec3::splat(size * 0.7)),
                        1.0,
                        Paint::Red,
                    );
                    m.paint(
                        &cyl,
                        strut(mount, mount - Vec3::Z * size * 2.2, size * 0.12),
                        1.0,
                        Paint::Metal,
                    );
                }
                SlotKind::Radiator => {
                    let out = radius + size * 0.8;
                    m.add(
                        &cube,
                        boxed(
                            Vec3::new(side * out, 0.0, at.z),
                            Vec3::new(0.12 * size, size * 1.6, size * 3.0),
                        ),
                        1.1,
                    );
                }
                SlotKind::Cargo => {
                    let out = Vec3::new(x.signum() * (x.abs().max(radius * 0.7)), y, at.z);
                    m.paint(&cube, boxed(out, Vec3::splat(size * 1.1)), 1.0, Paint::Foil);
                }
                SlotKind::Shield => {
                    m.paint(
                        &ring,
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(radius * 1.12),
                            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                            Vec3::new(0.0, 0.0, at.z),
                        ),
                        1.0,
                        Paint::Red,
                    );
                }
                SlotKind::Radar => {
                    let base = Vec3::new(x, radius * 0.9, at.z);
                    let top = base + Vec3::Y * size * 1.2;
                    m.paint(&cyl, strut(base, top, size * 0.08), 1.0, Paint::Metal);
                    m.add(
                        &ball,
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(size * 0.35),
                            Quat::IDENTITY,
                            top,
                        ),
                        1.0,
                    );
                }
                SlotKind::JumpDrive => {
                    m.paint(
                        &ring,
                        Mat4::from_scale_rotation_translation(
                            Vec3::splat(radius * 1.05),
                            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                            Vec3::new(0.0, 0.0, at.z),
                        ),
                        1.0,
                        Paint::Windows,
                    );
                }
                SlotKind::Rcs => {
                    for s in [-1.0f32, 1.0] {
                        m.paint(
                            &cube,
                            boxed(Vec3::new(s * radius, 0.0, at.z), Vec3::splat(size * 0.35)),
                            1.0,
                            Paint::Metal,
                        );
                    }
                }
                SlotKind::Power => {}
            }
        }
        front += len;
    }
    // Beacons at the widest point.
    let span = m
        .vertices
        .iter()
        .map(|v| v.pos[0].abs())
        .fold(1.0, f32::max);
    for s in [-1.0f32, 1.0] {
        m.paint(
            &ball,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.25 + length * 0.004),
                Quat::IDENTITY,
                Vec3::new(s * span, 0.0, 0.0),
            ),
            1.0,
            Paint::Beacon,
        );
    }
    (m, shape)
}

/// A hangar bay, in units of the ship it holds (the ship is about one unit
/// long, centred at the origin): a long hall along Z with a deck below,
/// steel ribs, ceiling lamps, a landing ring and control-room windows.
pub fn hangar_bay() -> Mesh {
    let (cube, cyl) = (cube(), cylinder(24));
    let ring = torus(0.02, 64, 6);
    let mut m = Mesh::default();
    let (half_w, half_l, floor, ceiling) = (1.5, 1.9, -0.3, 0.75);
    let height = ceiling - floor;
    // Deck, walls, ceiling and the far ends.
    m.paint(
        &cube,
        boxed(
            Vec3::new(0.0, floor - 0.01, 0.0),
            Vec3::new(half_w * 2.0, 0.02, half_l * 2.0),
        ),
        0.5,
        Paint::Metal,
    );
    for side in [-1.0f32, 1.0] {
        m.add(
            &cube,
            boxed(
                Vec3::new(side * half_w, floor + height / 2.0, 0.0),
                Vec3::new(0.02, height, half_l * 2.0),
            ),
            0.9,
        );
        m.add(
            &cube,
            boxed(
                Vec3::new(0.0, floor + height / 2.0, side * half_l),
                Vec3::new(half_w * 2.0, height, 0.02),
            ),
            0.8,
        );
        // A strip of lit control-room windows along each wall.
        m.paint(
            &cube,
            boxed(
                Vec3::new(side * (half_w - 0.015), floor + height * 0.62, 0.0),
                Vec3::new(0.01, height * 0.08, half_l * 1.6),
            ),
            1.0,
            Paint::Windows,
        );
        // Red hazard bands along the deck edges.
        m.paint(
            &cube,
            boxed(
                Vec3::new(side * (half_w - 0.08), floor, 0.0),
                Vec3::new(0.05, 0.005, half_l * 2.0),
            ),
            1.0,
            Paint::Red,
        );
        // A big red panel on each end wall.
        m.paint(
            &cube,
            boxed(
                Vec3::new(
                    side * half_w * 0.45,
                    floor + height * 0.5,
                    side * (half_l - 0.015),
                ),
                Vec3::new(half_w * 0.35, height * 0.75, 0.01),
            ),
            1.0,
            Paint::Red,
        );
    }
    m.add(
        &cube,
        boxed(
            Vec3::new(0.0, ceiling, 0.0),
            Vec3::new(half_w * 2.0, 0.02, half_l * 2.0),
        ),
        0.7,
    );
    // Steel ribs: up each wall and across the ceiling.
    let ribs = 11;
    for i in 0..ribs {
        let z = -half_l + (i as f32 + 0.5) * (half_l * 2.0 / ribs as f32);
        for side in [-1.0f32, 1.0] {
            m.paint(
                &cube,
                boxed(
                    Vec3::new(side * (half_w - 0.03), floor + height / 2.0, z),
                    Vec3::new(0.05, height, 0.06),
                ),
                1.0,
                Paint::Metal,
            );
        }
        m.paint(
            &cube,
            boxed(
                Vec3::new(0.0, ceiling - 0.03, z),
                Vec3::new(half_w * 2.0, 0.05, 0.06),
            ),
            1.0,
            Paint::Metal,
        );
        // Lamps between the ribs.
        if i + 1 < ribs {
            let lz = z + half_l / ribs as f32;
            for x in [-0.7f32, 0.0, 0.7] {
                m.paint(
                    &cube,
                    boxed(
                        Vec3::new(x, ceiling - 0.02, lz),
                        Vec3::new(0.18, 0.01, 0.04),
                    ),
                    1.0,
                    Paint::Lamp,
                );
            }
        }
    }
    // The landing ring on the deck, and a cradle under the ship.
    m.paint(
        &ring,
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.65, 0.65, 0.65),
            Quat::IDENTITY,
            Vec3::new(0.0, floor + 0.005, 0.0),
        ),
        1.0,
        Paint::Red,
    );
    for z in [-0.25f32, 0.25] {
        m.paint(
            &cyl,
            strut(
                Vec3::new(0.0, floor, z),
                Vec3::new(0.0, floor + 0.18, z),
                0.03,
            ),
            1.0,
            Paint::Metal,
        );
    }
    m
}
