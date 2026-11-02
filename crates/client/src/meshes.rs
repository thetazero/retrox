//! Procedural meshes: spheres, rocks, the placeholder ship, stations built
//! from a kit of modules, and gates.
//!
//! The look is "orbital hardware": cylinders, spheres, trusses and panels,
//! flat-shaded so the facets read as built things.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
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
}

#[derive(Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Append `other`, transformed.
    pub fn add(&mut self, other: &Mesh, transform: Mat4, shade: f32) {
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

fn tri(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3) {
    let n = (b - a).cross(c - a).normalize_or_zero();
    for p in [a, b, c] {
        m.indices.push(m.vertices.len() as u32);
        m.vertices.push(Vertex {
            pos: p.into(),
            normal: n.into(),
            shade: 1.0,
        });
    }
}

fn quad(m: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
    tri(m, a, b, c);
    tri(m, a, c, d);
}

/// Transform for a cylinder from `from` to `to` with the given radius.
fn strut(from: Vec3, to: Vec3, radius: f32) -> Mat4 {
    let dir = to - from;
    let rot = Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y));
    Mat4::from_scale_rotation_translation(Vec3::new(radius, dir.length(), radius), rot, from)
}

fn boxed(center: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center)
}

/// The placeholder ship: a stubby utility craft, ~24 m long, nose toward -Z.
pub fn placeholder_ship() -> Mesh {
    let (cyl, cube, ball) = (cylinder(10), cube(), icosphere(1).flat());
    let mut m = Mesh::default();
    // Pressurised hull: a cylinder along Z.
    m.add(
        &cyl,
        strut(Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, -6.0), 2.2),
        1.0,
    );
    // Cockpit ball with a dark "window" band.
    m.add(
        &ball,
        Mat4::from_scale_rotation_translation(
            Vec3::splat(2.4),
            Quat::IDENTITY,
            Vec3::new(0.0, 0.3, -7.0),
        ),
        0.95,
    );
    m.add(
        &cube,
        boxed(Vec3::new(0.0, 0.9, -8.6), Vec3::new(2.6, 0.7, 1.2)),
        0.25,
    );
    // Service module and engine bell.
    m.add(
        &cube,
        boxed(Vec3::new(0.0, 0.0, 9.5), Vec3::new(3.6, 3.6, 3.0)),
        0.8,
    );
    m.add(
        &cyl,
        strut(Vec3::new(0.0, 0.0, 11.0), Vec3::new(0.0, 0.0, 13.0), 1.6),
        0.45,
    );
    // Side pods on struts.
    for side in [-1.0f32, 1.0] {
        m.add(
            &cyl,
            strut(
                Vec3::new(side * 1.5, 0.0, 3.0),
                Vec3::new(side * 4.8, -0.5, 3.0),
                0.25,
            ),
            0.6,
        );
        m.add(
            &cyl,
            strut(
                Vec3::new(side * 5.0, -0.5, 7.5),
                Vec3::new(side * 5.0, -0.5, -1.0),
                0.9,
            ),
            0.85,
        );
        // Radiator fin.
        m.add(
            &cube,
            boxed(Vec3::new(side * 5.0, 1.4, 4.0), Vec3::new(0.15, 2.6, 5.0)),
            1.1,
        );
    }
    m
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
        m.add(
            &cyl,
            strut(Vec3::new(0.0, y, 0.0), Vec3::new(0.0, y + len, 0.0), rad),
            1.0,
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
            m.add(
                &torus(0.08, 40, 8),
                Mat4::from_scale(Vec3::splat(ring_r)),
                0.95,
            );
            for i in 0..4 {
                let a = i as f32 / 4.0 * TAU;
                let end = Vec3::new(a.cos(), 0.0, a.sin()) * ring_r;
                m.add(&cyl, strut(Vec3::ZERO, end, 0.025), 0.7);
            }
        }
        StationKind::Facility(FacilityKind::Mine | FacilityKind::IceHarvester) => {
            // Gantries and ore hoppers.
            for i in 0..3 {
                let a = i as f32 / 3.0 * TAU;
                let out = Vec3::new(a.cos(), r(&mut rng, -0.4, 0.4), a.sin()) * 0.7;
                m.add(&cyl, strut(Vec3::ZERO, out, 0.03), 0.6);
                m.add(&cube, boxed(out, Vec3::splat(0.22)), 0.75);
            }
        }
        StationKind::Facility(FacilityKind::Refinery | FacilityKind::FuelPlant) => {
            // Spherical tanks around the spine.
            for i in 0..4 {
                let a = i as f32 / 4.0 * TAU + 0.4;
                let p = Vec3::new(a.cos() * 0.35, r(&mut rng, -0.4, 0.3), a.sin() * 0.35);
                m.add(
                    &ball,
                    Mat4::from_scale_rotation_translation(Vec3::splat(0.17), Quat::IDENTITY, p),
                    1.05,
                );
            }
        }
        StationKind::Facility(FacilityKind::Hydroponics) => {
            // Long greenhouse arms.
            for side in [-1.0f32, 1.0] {
                m.add(
                    &cyl,
                    strut(Vec3::ZERO, Vec3::new(side * 0.9, 0.0, 0.0), 0.07),
                    1.15,
                );
            }
        }
        StationKind::Facility(FacilityKind::Factory) => {
            // Blocky works.
            for _ in 0..3 {
                let p = Vec3::new(
                    r(&mut rng, -0.35, 0.35),
                    r(&mut rng, -0.5, 0.4),
                    r(&mut rng, -0.35, 0.35),
                );
                m.add(
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
                );
            }
        }
        StationKind::Outpost => {}
    }

    // Solar wings on a truss.
    let wing_y = r(&mut rng, -0.3, 0.3);
    m.add(
        &cyl,
        strut(
            Vec3::new(-0.95, wing_y, 0.0),
            Vec3::new(0.95, wing_y, 0.0),
            0.02,
        ),
        0.6,
    );
    for side in [-1.0f32, 1.0] {
        m.add(
            &cube,
            boxed(
                Vec3::new(side * 0.7, wing_y, 0.0),
                Vec3::new(0.45, 0.01, 0.22),
            ),
            0.35,
        );
    }
    m
}
