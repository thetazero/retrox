//! The title screen's backdrop: a starting ship holding station off the
//! dark side of a world at dawn, under a made-up sky. Nothing here comes
//! from a galaxy — none exists until the player starts or loads one.

use glam::{Mat4, Quat, Vec3};

use crate::Meshes;
use crate::render::{Instance, MeshId, SkyVertex, Surface};

/// The star's light: a warm G.
pub const STAR: [f32; 3] = [1.0, 0.9, 0.74];
/// Direction to the galactic core, and how far out we are.
pub const CORE: Vec3 = Vec3::new(-0.62, 0.2, -0.76);
pub const CORE_OUT: f32 = 0.55;

/// Where the light comes from: behind the planet, up and to the left, so
/// the world is a crescent with the dawn on its upper-left limb.
fn sun_dir() -> Vec3 {
    Vec3::new(-0.85, 0.42, -0.3).normalize()
}

/// The view and what to draw, `time` seconds after launch.
pub fn scene(meshes: &Meshes, ship: MeshId, time: f32) -> (Mat4, Vec<(MeshId, Instance)>) {
    // A slow drift, so the screen is alive without anything moving fast.
    let drift = Quat::from_rotation_y((time * 0.013).sin() * 0.035)
        * Quat::from_rotation_x((time * 0.009).sin() * 0.02);
    let view = Mat4::from_quat(drift.inverse());
    let sun = sun_dir();
    let light = [sun.x, sun.y, sun.z, 1.35];
    let mut out = Vec::new();

    // The world: big, low and to the right.
    let planet_r = 6.4e6;
    let planet = Vec3::new(0.55, -0.62, -1.0).normalize() * 1.25e7;
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale_rotation_translation(
                Vec3::splat(planet_r),
                Quat::from_rotation_y(time * 0.004 + 2.1) * Quat::from_rotation_x(0.35),
                planet,
            ),
            [0.36, 0.42, 0.3],
            Surface::Ocean,
            0.0,
            4127.0,
            light,
        )
        .with_detail(0.52),
    ));

    // Its moon, small and far, catching the light.
    let moon = Vec3::new(0.62, 0.3, -1.0).normalize() * 3.0e8;
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale_rotation_translation(Vec3::splat(1.7e6), Quat::IDENTITY, moon),
            [0.45, 0.42, 0.39],
            Surface::Barren,
            0.0,
            811.0,
            light,
        ),
    ));

    // The star, off to the left, out of view: drawn for the bloom at the
    // screen's edge.
    let star = sun * 1.5e11;
    out.push((
        meshes.sphere,
        Instance::new(
            Mat4::from_scale_rotation_translation(Vec3::splat(7.0e8), Quat::IDENTITY, star),
            STAR,
            Surface::Star,
            6.0,
            0.0,
            [0.0, 1.0, 0.0, 1.0],
        ),
    ));

    // The ship, holding station in the foreground, turning slowly.
    let ship_pos = Vec3::new(20.0, 11.0, -66.0);
    let ship_rot = Quat::from_rotation_y(0.9 + time * 0.03)
        * Quat::from_rotation_x(-0.12)
        * Quat::from_rotation_z(0.18);
    out.push((
        ship,
        Instance::new(
            Mat4::from_rotation_translation(ship_rot, ship_pos),
            [0.8, 0.75, 0.64],
            Surface::Plain,
            0.0,
            0.0,
            light,
        )
        .with_detail(0.5),
    ));
    (view, out)
}

/// A made-up sky: a band of faint stars along the galactic plane, densest
/// toward the core, and a scatter of brighter ones everywhere.
pub fn sky() -> Vec<SkyVertex> {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut rand = move || {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    };
    let core = CORE.normalize();
    // The galactic plane holds the core direction and is tilted a little.
    let normal = core
        .cross(Vec3::new(0.0, 1.0, 0.25))
        .cross(core)
        .normalize();
    let mut stars = Vec::with_capacity(5000);
    for i in 0..5000 {
        let in_band = i % 3 != 0;
        let dir = loop {
            let v = Vec3::new(rand() * 2.0 - 1.0, rand() * 2.0 - 1.0, rand() * 2.0 - 1.0);
            if v.length_squared() <= 1.0 && v.length_squared() > 1e-4 {
                let v = v.normalize();
                if !in_band {
                    break v;
                }
                // Squash toward the plane.
                let h = v.dot(normal);
                let squashed = (v - normal * h * 0.88).normalize();
                break squashed;
            }
        };
        let toward_core = dir.dot(core).max(0.0);
        let brightness = if in_band {
            0.15 + rand().powi(3) * 0.9 + toward_core * 0.25
        } else {
            0.2 + rand().powi(6) * 4.0
        };
        let tint = rand();
        let c = if tint < 0.15 {
            [0.7, 0.8, 1.0]
        } else if tint < 0.6 {
            [1.0, 1.0, 1.0]
        } else if tint < 0.85 {
            [1.0, 0.9, 0.72]
        } else {
            [1.0, 0.68, 0.5]
        };
        stars.push(SkyVertex {
            dir: dir.into(),
            color: [c[0] * brightness, c[1] * brightness, c[2] * brightness],
        });
    }
    stars
}
