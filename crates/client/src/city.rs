//! Landed at a city (#178, DESIGN §8.8): the ship on its pad at the
//! city's port, the skyline beyond it, under its own world's sky and on
//! its own ground. Everything here is fixed per city (from its seed and
//! its world), so a city looks the same each time you land.
//!
//! The scene is in metres in the pad's frame: y up, the ground at 0, the
//! pad's centre at the origin. Like the flight scene, everything is drawn
//! relative to the camera.

use std::f32::consts::{PI, TAU};

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::meshes::{self, Mesh, Paint, boxed, strut};
use crate::render::{Instance, MeshId, Surface};

/// What the ground is made of; must match `ground` in mesh.wgsl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Grass = 1,
    Sand = 2,
    Snow = 3,
    Regolith = 4,
    /// Grass down to a beach and the sea.
    Shore = 5,
}

/// A world's air, as the sky and the haze show it.
#[derive(Clone, Copy, Debug)]
pub struct Air {
    pub zenith: Vec3,
    pub horizon: Vec3,
    /// How thick the haze is, per metre of sight.
    pub per_m: f32,
    /// Cloud cover, 0..1.
    pub clouds: f32,
}

/// A body hanging in the sky: a moon's planet.
#[derive(Clone, Copy, Debug)]
pub struct SkyBody {
    pub surface: Surface,
    pub color: [f32; 3],
    pub seed: f32,
    pub sea_level: f32,
    /// Its angular radius, in radians.
    pub radius: f32,
}

/// Everything about where a city stands that the scene shows.
#[derive(Clone, Debug)]
pub struct Site {
    pub terrain: Terrain,
    /// `None` on an airless world: a black sky with the stars in it.
    pub air: Option<Air>,
    /// Toward the sun, and its angular radius (radians).
    pub sun: Vec3,
    pub sun_radius: f32,
    /// The sunlight's colour (the star's, warmed by a low sun) and
    /// strength.
    pub light: [f32; 3],
    pub light_w: f32,
    /// The star's own colour, for its disc on an airless world.
    pub star: [f32; 3],
    pub seed: u64,
    /// The settlement's population (thousands): how big the city is.
    pub population: f64,
    /// Which way the city lies from the pad, radians round y (0 = +x,
    /// increasing toward +z).
    pub bearing: f32,
    /// How the ship is parked: radians round y.
    pub ship_yaw: f32,
    /// Toward the sea (x, z) and how far the quay is, on a shore.
    pub sea: Option<(Vec2, f32)>,
    pub parent: Option<SkyBody>,
}

/// The kind of world a city stands on, for its ground and air.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum World {
    Terran,
    Ocean,
    Desert,
    Ice,
    /// Barren planets, moons, anything without air.
    Airless,
}

fn mix3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A small deterministic random stream.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// 0..1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
}

fn dir(angle: f32) -> Vec3 {
    Vec3::new(angle.cos(), 0.0, angle.sin())
}

impl Site {
    /// Works out a city's place: `seed` is the city's own, `star` its
    /// star's colour, `sun_radius` the star's angular radius from there,
    /// `pad` the radius of the pad (a shore's quay is just past it).
    pub fn new(
        pad: f32,
        world: World,
        seed: u64,
        population: f64,
        star: [f32; 3],
        sun_radius: f32,
        parent: Option<SkyBody>,
    ) -> Self {
        let mut rng = Rng::new(seed);
        // The city lies along +x and the camera stands across the pad
        // from it, so the port is laid out once round that. The ship is
        // parked at the city's own angle, always seen three-quarters on.
        let bearing = 0.0;
        let camera = bearing + PI;
        let ship_yaw = [0.75f32, PI - 0.75, -0.75, 0.75 - PI][(rng.next() % 4) as usize]
            + rng.range(-0.2, 0.2);
        // The hour is the city's own: from a low sun to early afternoon,
        // from the camera's side, so the ship and the city's faces are lit.
        let elevation = rng.range(5.0, 50.0).to_radians();
        let side = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
        let azimuth = camera + side * rng.range(0.5, 1.5);
        let sun = Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.sin(),
            elevation.cos() * azimuth.sin(),
        );
        let low = 1.0 - smoothstep(0.08, 0.42, elevation);
        let star_v = Vec3::from(star);
        let tint = mix3(Vec3::ONE, star_v / star_v.max_element().max(1e-3), 0.3);

        let terrain = match world {
            World::Terran => Terrain::Grass,
            World::Ocean => Terrain::Shore,
            World::Desert => Terrain::Sand,
            World::Ice => Terrain::Snow,
            World::Airless => Terrain::Regolith,
        };
        let base = match world {
            World::Terran => Some(Air {
                zenith: Vec3::new(0.09, 0.22, 0.52),
                horizon: Vec3::new(0.5, 0.62, 0.75),
                per_m: 1.0 / 7_000.0,
                clouds: 0.45,
            }),
            World::Ocean => Some(Air {
                zenith: Vec3::new(0.07, 0.21, 0.5),
                horizon: Vec3::new(0.52, 0.67, 0.78),
                per_m: 1.0 / 6_000.0,
                clouds: 0.6,
            }),
            World::Desert => Some(Air {
                zenith: Vec3::new(0.22, 0.24, 0.42),
                horizon: Vec3::new(0.82, 0.56, 0.36),
                per_m: 1.0 / 4_500.0,
                clouds: 0.12,
            }),
            World::Ice => Some(Air {
                zenith: Vec3::new(0.035, 0.08, 0.27),
                horizon: Vec3::new(0.6, 0.72, 0.83),
                per_m: 1.0 / 16_000.0,
                clouds: 0.25,
            }),
            World::Airless => None,
        };
        let air = base.map(|a| {
            // A low sun reddens the horizon and darkens the zenith.
            let horizon = mix3(a.horizon, Vec3::new(0.95, 0.5, 0.3), low * 0.55) * tint;
            let zenith = a.zenith * (1.0 - 0.45 * low) * tint;
            Air {
                zenith,
                horizon,
                per_m: a.per_m * rng.range(0.7, 1.3),
                clouds: (a.clouds + rng.range(-0.2, 0.2)).clamp(0.0, 1.0),
            }
        });
        let light = star_v * mix3(Vec3::ONE, Vec3::new(1.0, 0.68, 0.48), low * 0.7);
        // Air takes some of a low sun's strength; with none, it's harsh.
        let light_w = if air.is_some() {
            1.25 * (1.0 - 0.4 * low)
        } else {
            1.35
        };
        let sea = (terrain == Terrain::Shore).then(|| {
            // The pad stands on the waterfront, the sea off to the right
            // (+z), where no panel covers the view; from a pilot's height
            // a sea any further off would be a sliver on the horizon. The
            // coast runs nearly along the view, so the city keeps the left
            // and the middle.
            let d = dir(bearing + rng.range(1.4, 1.52));
            (Vec2::new(d.x, d.z), pad * 3.3 + rng.range(15.0, 45.0))
        });
        Self {
            terrain,
            air,
            sun,
            sun_radius: sun_radius.clamp(0.0045, 0.08),
            light: light.into(),
            light_w,
            star,
            seed,
            population,
            bearing,
            ship_yaw,
            sea,
            parent,
        }
    }

    fn light(&self) -> [f32; 4] {
        [self.sun.x, self.sun.y, self.sun.z, self.light_w]
    }

    /// The air as instances carry it (`aux`): its colour and thickness.
    fn aux(&self) -> [f32; 4] {
        match self.air {
            Some(a) => [a.horizon.x, a.horizon.y, a.horizon.z, a.per_m],
            None => [0.0; 4],
        }
    }
}

/// The meshes of one city's scene.
#[derive(Clone, Copy, Debug)]
pub struct CityMeshes {
    /// The skyline, in metres.
    pub skyline: MeshId,
    /// The hills round the horizon, in metres.
    pub hills: MeshId,
}

/// Radius of the pad for a ship this long.
pub fn pad_radius(ship_length: f32) -> f32 {
    ship_length.max(16.0) * 0.8
}

/// How high the chocks the ship stands on are, in pad radii.
const CHOCK: f32 = 0.05;

/// How far the ground and sky go.
const GROUND_M: f32 = 45_000.0;
const DOME_M: f32 = 60_000.0;

/// The view and what to draw: `ship` on the pad, `time` seconds in.
#[allow(clippy::too_many_arguments)]
pub fn scene(
    site: &Site,
    city: CityMeshes,
    sphere: MeshId,
    disc: MeshId,
    port: MeshId,
    chocks: MeshId,
    ship: MeshId,
    ship_length: f32,
    ship_low: f32,
    time: f32,
) -> (Mat4, Vec<(MeshId, Instance)>) {
    let size = ship_length.max(16.0);
    let pad = pad_radius(ship_length);
    // Standing on the chocks.
    let ship_at = Vec3::new(0.0, CHOCK * pad - ship_low, 0.0);
    // The camera stands across the pad from the city, swaying slowly
    // either side, so the ship is always seen against the skyline.
    let sway = (time * 0.035).sin() * 0.35;
    let camera = site.bearing + PI + sway;
    let eye = dir(camera) * size * 1.45 + Vec3::Y * size * 0.42;
    let forward = (ship_at - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    // Aim a little to the ship's left so it stands clear of the panels
    // on the left of the screen, and a little up, for the sky.
    let target = ship_at - right * 0.3 * size + Vec3::Y * 0.12 * size;
    let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, target - eye, Vec3::Y);
    let at = |p: Vec3| Mat4::from_translation(p - eye);
    let light = site.light();
    let aux = site.aux();
    let mut out = Vec::new();

    match site.air {
        Some(air) => {
            let mut sky = Instance::new(
                Mat4::from_scale(Vec3::splat(DOME_M)),
                air.zenith.into(),
                Surface::SkyDome,
                site.sun_radius,
                (site.seed % 997) as f32,
                light,
            )
            .with_detail(air.clouds);
            sky.aux = [air.horizon.x, air.horizon.y, air.horizon.z, 1.0];
            out.push((sphere, sky));
        }
        None => {
            // No air: the sun is a disc in a black sky, among the stars.
            let far = DOME_M * 0.9;
            out.push((
                sphere,
                Instance::new(
                    at(site.sun * far + eye)
                        * Mat4::from_scale(Vec3::splat(far * site.sun_radius.tan())),
                    site.star,
                    Surface::Star,
                    5.0,
                    0.0,
                    [0.0, 1.0, 0.0, 1.0],
                ),
            ));
        }
    }
    if let Some(body) = site.parent {
        // A moon's planet, big in the sky, away from the sun.
        let mut rng = Rng::new(site.seed ^ 0x51ab);
        let azimuth = site.bearing + rng.range(-0.5, 0.5);
        let elevation = rng.range(0.35, 0.6);
        let d = Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.sin(),
            elevation.cos() * azimuth.sin(),
        );
        let far = DOME_M * 0.8;
        out.push((
            sphere,
            Instance::new(
                at(d * far + eye)
                    * Mat4::from_quat(Quat::from_rotation_x(0.4))
                    * Mat4::from_scale(Vec3::splat(far * body.radius.tan())),
                body.color,
                body.surface,
                0.0,
                body.seed,
                light,
            )
            .with_detail(body.sea_level),
        ));
    }

    let mut ground = Instance::new(
        at(Vec3::ZERO) * Mat4::from_scale(Vec3::splat(GROUND_M)),
        [0.0; 3],
        Surface::Ground,
        0.0,
        (site.seed % 997) as f32,
        light,
    )
    .with_detail(site.terrain as u32 as f32)
    .with_aux(aux);
    if let Some((sea, coast)) = site.sea {
        ground.color = [sea.x, sea.y, coast, 0.0];
    }
    ground.color[3] = pad;
    out.push((disc, ground));
    let mut hills = Instance::new(
        at(Vec3::ZERO),
        [0.0; 3],
        Surface::Ground,
        0.0,
        (site.seed % 997) as f32,
        light,
    )
    .with_detail(site.terrain as u32 as f32)
    .with_aux(aux);
    if let Some((sea, coast)) = site.sea {
        hills.color = [sea.x, sea.y, coast, 0.0];
    }
    out.push((city.hills, hills));

    // Concrete, warm under a yellow sun.
    out.push((
        city.skyline,
        Instance::new(
            at(Vec3::ZERO),
            [0.55, 0.52, 0.46],
            Surface::Plain,
            0.0,
            1.0,
            light,
        )
        .with_detail(0.12)
        .with_aux(aux),
    ));
    out.push((
        port,
        Instance::new(
            at(Vec3::ZERO) * Mat4::from_scale(Vec3::splat(pad)),
            [0.66, 0.62, 0.54],
            Surface::Plain,
            0.0,
            2.0,
            light,
        )
        .with_detail(pad / 4.0)
        .with_aux(aux),
    ));
    let parked = Mat4::from_rotation_y(site.ship_yaw);
    out.push((
        chocks,
        Instance::new(
            at(Vec3::ZERO) * parked * Mat4::from_scale(Vec3::splat(pad)),
            [0.66, 0.62, 0.54],
            Surface::Plain,
            0.0,
            3.0,
            light,
        )
        .with_aux(aux),
    ));
    out.push((
        ship,
        Instance::new(
            at(ship_at) * parked,
            [0.8, 0.75, 0.64],
            Surface::Plain,
            0.0,
            0.0,
            light,
        )
        .with_detail(0.5)
        .with_aux(aux),
    ));
    (view, out)
}

/// A flat unit disc in the XZ plane: the ground.
pub fn disc() -> Mesh {
    let mut m = Mesh::default();
    let n = 96;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32 * TAU, (i + 1) as f32 / n as f32 * TAU);
        meshes::tri(
            &mut m,
            Vec3::ZERO,
            Vec3::new(b.cos(), 0.0, b.sin()),
            Vec3::new(a.cos(), 0.0, a.sin()),
        );
    }
    m
}

/// The port round the pad, in units of the pad's radius: floodlight
/// masts at the apron's corners, a service gantry, the control tower,
/// fuel tanks and a shed, laid out round the camera standing at -x.
pub fn port() -> Mesh {
    let (cube, cyl, ball) = (meshes::cube(), meshes::cylinder(16), meshes::icosphere(2));
    let mut m = Mesh::default();
    // Floodlights on tall masts, their lamps turned to the pad.
    for (x, z) in [
        (2.95f32, 2.95f32),
        (-2.95, 2.95),
        (2.95, -2.95),
        (-2.95, -2.95),
    ] {
        let base = Vec3::new(x, 0.0, z);
        let top = base + Vec3::Y * 1.7;
        m.paint(&cyl, strut(base, top, 0.025), 1.0, Paint::Metal);
        let toward = Quat::from_rotation_y(-(z.atan2(x)) + PI / 2.0);
        m.paint(
            &cube,
            Mat4::from_scale_rotation_translation(Vec3::new(0.32, 0.06, 0.05), toward, top),
            1.0,
            Paint::Metal,
        );
        m.paint(
            &cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(0.28, 0.04, 0.02),
                toward,
                top - Vec3::new(x, 0.0, z).normalize() * 0.03,
            ),
            1.0,
            Paint::Lamp,
        );
        m.paint(
            &cube,
            boxed(top + Vec3::Y * 0.06, Vec3::splat(0.04)),
            1.0,
            Paint::Beacon,
        );
    }
    // The service gantry beside the pad: a red lattice tower with an arm.
    let g = Vec3::new(0.25, 0.0, -1.3);
    let (w, h) = (0.09f32, 0.75f32);
    let legs = [(-w, -w), (w, -w), (w, w), (-w, w)].map(|(x, z)| g + Vec3::new(x, 0.0, z));
    for &l in &legs {
        m.paint(&cyl, strut(l, l + Vec3::Y * h, 0.012), 1.0, Paint::Red);
    }
    let bays = 6;
    for k in 0..bays {
        let (y0, y1) = (h * k as f32 / bays as f32, h * (k + 1) as f32 / bays as f32);
        for i in 0..4 {
            let (a, b) = (legs[i], legs[(i + 1) % 4]);
            m.paint(
                &cyl,
                strut(a + Vec3::Y * y0, b + Vec3::Y * y1, 0.006),
                1.0,
                Paint::Metal,
            );
            m.paint(
                &cyl,
                strut(a + Vec3::Y * y1, b + Vec3::Y * y1, 0.006),
                1.0,
                Paint::Metal,
            );
        }
    }
    // Its arm reaches over to the ship's flank.
    m.paint(
        &cube,
        boxed(
            g + Vec3::new(0.0, h * 0.45, 0.3),
            Vec3::new(0.05, 0.035, 0.6),
        ),
        1.0,
        Paint::Red,
    );
    m.paint(
        &cyl,
        strut(
            g + Vec3::new(0.0, h * 0.45, 0.58),
            g + Vec3::new(0.0, h * 0.3, 0.58),
            0.012,
        ),
        1.0,
        Paint::Metal,
    );
    m.paint(
        &cube,
        boxed(g + Vec3::Y * (h + 0.03), Vec3::splat(0.05)),
        1.0,
        Paint::Beacon,
    );
    // The control tower: a concrete shaft, a glazed cab, a red roof.
    let t = Vec3::new(4.2, 0.0, -2.4);
    m.add(&cyl, strut(t, t + Vec3::Y * 1.9, 0.13), 1.0);
    m.paint(
        &cyl,
        strut(t + Vec3::Y * 1.9, t + Vec3::Y * 2.12, 0.3),
        1.0,
        Paint::Glass,
    );
    m.paint(
        &cyl,
        strut(t + Vec3::Y * 2.12, t + Vec3::Y * 2.18, 0.34),
        1.0,
        Paint::Red,
    );
    m.add(
        &cyl,
        strut(t + Vec3::Y * 1.84, t + Vec3::Y * 1.9, 0.34),
        0.9,
    );
    m.paint(
        &cyl,
        strut(t + Vec3::Y * 2.18, t + Vec3::Y * 2.6, 0.01),
        1.0,
        Paint::Metal,
    );
    m.paint(
        &cube,
        boxed(t + Vec3::Y * 2.62, Vec3::splat(0.04)),
        1.0,
        Paint::Beacon,
    );
    // Fuel spheres on legs, in work mustard.
    for z in [-3.3f32, -4.05] {
        let c = Vec3::new(2.6, 0.5, z);
        m.paint(
            &ball,
            Mat4::from_scale_rotation_translation(Vec3::splat(0.3), Quat::IDENTITY, c),
            1.0,
            Paint::Ochre,
        );
        for a in 0..4 {
            let d = dir(a as f32 * TAU / 4.0 + 0.4) * 0.22;
            m.paint(
                &cyl,
                strut(c + d - Vec3::Y * 0.5, c + d, 0.02),
                1.0,
                Paint::Metal,
            );
        }
    }
    m.paint(
        &cube,
        boxed(Vec3::new(2.6, 0.5, -3.675), Vec3::new(0.04, 0.04, 0.5)),
        1.0,
        Paint::Metal,
    );
    // A low shed with a red door.
    let s = Vec3::new(-0.4, 0.0, -2.2);
    m.add(
        &cube,
        boxed(s + Vec3::Y * 0.17, Vec3::new(0.7, 0.34, 0.5)),
        0.85,
    );
    m.paint(
        &cube,
        boxed(
            s + Vec3::new(-0.351, 0.14, 0.0),
            Vec3::new(0.01, 0.26, 0.32),
        ),
        1.0,
        Paint::Red,
    );
    m.paint(
        &cube,
        boxed(s + Vec3::new(0.0, 0.34, 0.0), Vec3::new(0.74, 0.03, 0.54)),
        0.7,
        Paint::Metal,
    );
    m
}

/// The chocks the ship stands on, fore and aft, in pad radii.
pub fn chocks() -> Mesh {
    let cube = meshes::cube();
    let mut m = Mesh::default();
    for z in [-0.3f32, 0.3] {
        m.paint(
            &cube,
            boxed(Vec3::new(0.0, CHOCK / 2.0, z), Vec3::new(0.14, CHOCK, 0.06)),
            1.0,
            Paint::Hazard,
        );
    }
    m
}

/// A building's footprint, to keep them apart.
struct Lot {
    at: Vec2,
    r: f32,
}

/// The city's skyline, in metres: blocks of flats, towers, domes and
/// works spread over the arc the city stands in, a broadcast tower at its
/// heart, more and taller the bigger the city.
pub fn skyline(site: &Site, pad: f32) -> Mesh {
    let (cube, cyl, ball) = (meshes::cube(), meshes::cylinder(20), meshes::icosphere(2));
    let ring = meshes::torus(0.06, 40, 6);
    let mut rng = Rng::new(site.seed ^ 0xc17e);
    let mut m = Mesh::default();
    let size = (1.0 + site.population.max(0.0)).ln() as f32;
    let count = (28.0 + 14.0 * size).clamp(28.0, 140.0) as usize;
    let r0 = (pad * 4.6).max(170.0);
    let depth = 420.0 + 9.0 * count as f32;
    let spread = 1.25f32;
    let airless = site.air.is_none() || site.terrain == Terrain::Snow;
    let mut lots: Vec<Lot> = Vec::new();
    // Whether a lot `r` across at `p` would be in the sea or on the quay.
    let in_sea = |p: Vec2, r: f32| match site.sea {
        Some((sea, coast)) => p.dot(sea) > coast - r - 15.0,
        None => false,
    };

    // The broadcast tower at the city's heart.
    let heart = dir(site.bearing) * (r0 + depth * 0.45);
    let heart2 = Vec2::new(heart.x, heart.z);
    if !in_sea(heart2, 30.0) {
        let h = (170.0 + 30.0 * size).min(360.0);
        let shaft = meshes::lathe(
            &[(0.0, 11.0), (h * 0.3, 6.0), (h, 3.0), (h, 0.0)],
            16,
            0.0,
            TAU,
        );
        let up = Mat4::from_translation(heart) * Mat4::from_rotation_x(-PI / 2.0);
        m.add(&shaft, up, 1.0);
        let pod = heart + Vec3::Y * h * 0.66;
        m.paint(
            &cyl,
            strut(pod, pod + Vec3::Y * 9.0, 16.0),
            1.0,
            Paint::Windows,
        );
        m.paint(&cyl, strut(pod - Vec3::Y * 3.0, pod, 19.0), 1.0, Paint::Red);
        m.add(
            &cyl,
            strut(pod + Vec3::Y * 9.0, pod + Vec3::Y * 11.0, 13.0),
            0.9,
        );
        m.paint(
            &cyl,
            strut(heart + Vec3::Y * h, heart + Vec3::Y * (h + 45.0), 1.2),
            1.0,
            Paint::Metal,
        );
        for k in 0..3 {
            let y = h + 10.0 + k as f32 * 15.0;
            m.paint(
                &cube,
                boxed(heart + Vec3::Y * y, Vec3::splat(2.5)),
                1.0,
                Paint::Beacon,
            );
        }
        lots.push(Lot {
            at: heart2,
            r: 30.0,
        });
    }

    let mut placed = 0;
    let mut tries = 0;
    while placed < count && tries < count * 12 {
        tries += 1;
        // More buildings toward the middle of the arc, and the tallest
        // round the heart.
        let off = (rng.unit() + rng.unit() - 1.0) * spread;
        let reach = rng.unit().powf(0.8);
        let d = r0 + depth * reach;
        let p = dir(site.bearing + off) * d;
        let at = Vec2::new(p.x, p.z);
        let core = (1.0 - (off / spread).abs()) * (1.0 - (reach - 0.45).abs());
        let kind = rng.unit();
        let r = 22.0 + 30.0 * rng.unit();
        if in_sea(at, r) || lots.iter().any(|l| l.at.distance(at) < l.r + r) {
            continue;
        }
        lots.push(Lot { at, r });
        placed += 1;
        // Faces the port, give or take.
        let face = Quat::from_rotation_y(-(site.bearing + off) + rng.range(-0.3, 0.3));
        let shade = rng.range(0.75, 1.1);
        let place = |c: Vec3, s: Vec3| Mat4::from_scale_rotation_translation(s, face, p + face * c);

        if airless && kind < 0.22 {
            // A habitat under a glass dome on a concrete ring.
            let rd = rng.range(22.0, 48.0);
            m.paint(
                &ball,
                Mat4::from_scale_rotation_translation(Vec3::new(rd, rd * 0.62, rd), face, p),
                1.0,
                Paint::Glass,
            );
            m.add(
                &ring,
                Mat4::from_scale_rotation_translation(Vec3::splat(rd), face, p),
                shade,
            );
            continue;
        }
        if kind < 0.42 {
            // A slab of flats, with a red panel up one end.
            let (w, dd) = (rng.range(45.0, 95.0), rng.range(12.0, 16.0));
            let h = rng.range(22.0, 40.0) + 25.0 * core;
            m.paint(
                &cube,
                place(Vec3::Y * h / 2.0, Vec3::new(w, h, dd)),
                shade,
                Paint::Windows,
            );
            m.add(
                &cube,
                place(Vec3::Y * (h + 0.8), Vec3::new(w + 1.5, 1.6, dd + 1.5)),
                shade * 0.85,
            );
            // Some have a panel up one end, red or sky blue, like a
            // mosaic on a prefab block.
            let panel = rng.unit();
            if panel < 0.6 {
                let end = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                m.paint(
                    &cube,
                    place(
                        Vec3::new(end * (w / 2.0 + 0.3), h * 0.5, 0.0),
                        Vec3::new(0.8, h * 0.86, dd * 0.6),
                    ),
                    1.0,
                    if panel < 0.35 { Paint::Red } else { Paint::Sky },
                );
            }
        } else if kind < 0.66 {
            // A tower, stepped back near the top, some with a mast.
            let s = rng.range(18.0, 28.0);
            let h = rng.range(55.0, 90.0) + 90.0 * core;
            m.paint(
                &cube,
                place(Vec3::Y * h * 0.4, Vec3::new(s, h * 0.8, s)),
                shade,
                Paint::Windows,
            );
            m.paint(
                &cube,
                place(Vec3::Y * h * 0.9, Vec3::new(s * 0.75, h * 0.2, s * 0.75)),
                shade,
                Paint::Windows,
            );
            m.add(
                &cube,
                place(Vec3::Y * h * 0.8, Vec3::new(s + 1.0, 1.4, s + 1.0)),
                shade * 0.85,
            );
            m.add(
                &cube,
                place(Vec3::Y * h, Vec3::new(s * 0.75 + 1.0, 1.4, s * 0.75 + 1.0)),
                shade * 0.85,
            );
            if rng.unit() < 0.4 {
                let top = p + Vec3::Y * h;
                m.paint(
                    &cyl,
                    strut(top, top + Vec3::Y * 30.0, 0.6),
                    1.0,
                    Paint::Metal,
                );
                m.paint(
                    &cube,
                    boxed(top + Vec3::Y * 31.0, Vec3::splat(1.6)),
                    1.0,
                    Paint::Beacon,
                );
            }
        } else if kind < 0.8 {
            // A round tower with a red crown.
            let rr = rng.range(10.0, 16.0);
            let h = rng.range(45.0, 70.0) + 60.0 * core;
            m.paint(&cyl, strut(p, p + Vec3::Y * h, rr), shade, Paint::Windows);
            m.add(
                &cyl,
                strut(p + Vec3::Y * h, p + Vec3::Y * (h + 1.5), rr + 1.0),
                shade * 0.85,
            );
            m.paint(
                &ring,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(rr + 1.0, 30.0, rr + 1.0),
                    Quat::IDENTITY,
                    p + Vec3::Y * (h - 4.0),
                ),
                1.0,
                Paint::Red,
            );
        } else {
            // Works: a long low hall and a striped chimney.
            let (w, dd, h) = (
                rng.range(40.0, 75.0),
                rng.range(25.0, 40.0),
                rng.range(10.0, 18.0),
            );
            m.paint(
                &cube,
                place(Vec3::Y * h / 2.0, Vec3::new(w, h, dd)),
                shade,
                Paint::Ochre,
            );
            for k in 0..3 {
                let x = (k as f32 - 1.0) * w / 3.0;
                m.add(
                    &cube,
                    place(
                        Vec3::new(x, h + 2.0, 0.0),
                        Vec3::new(w / 3.0 - 1.0, 4.0, dd * 0.5),
                    ),
                    shade * 0.8,
                );
            }
            let c = p + face * Vec3::new(w * 0.35, 0.0, dd * 0.7);
            let ch = rng.range(40.0, 70.0);
            m.add(&cyl, strut(c, c + Vec3::Y * ch * 0.75, 3.0), 0.95);
            m.paint(
                &cyl,
                strut(c + Vec3::Y * ch * 0.75, c + Vec3::Y * ch, 2.8),
                1.0,
                Paint::Red,
            );
        }
    }
    m
}

/// A smooth periodic noise round a circle: `cells` values, cosine-
/// interpolated.
fn ring_noise(rng: &mut Rng, cells: usize) -> Vec<f32> {
    (0..cells).map(|_| rng.unit()).collect()
}

fn sample(values: &[f32], t: f32) -> f32 {
    let n = values.len();
    let x = t.rem_euclid(1.0) * n as f32;
    let i = x.floor() as usize % n;
    let f = x.fract();
    let f = (1.0 - (f * PI).cos()) * 0.5;
    values[i] * (1.0 - f) + values[(i + 1) % n] * f
}

/// The hills round the horizon, in metres: a near ring and a far one,
/// shaped by the world (rolling grassland, desert mesas, ice peaks, a
/// moon's crater walls). On a shore, none stand in the sea.
pub fn hills(site: &Site) -> Mesh {
    let mut rng = Rng::new(site.seed ^ 0x4111);
    let mut m = Mesh::default();
    // (radius, lowest crest, highest crest, slope: run per rise)
    let rings: [(f32, f32, f32, f32); 2] = match site.terrain {
        Terrain::Grass | Terrain::Shore => {
            [(4_000.0, 60.0, 380.0, 5.0), (11_000.0, 350.0, 1_200.0, 3.5)]
        }
        Terrain::Sand => [(3_500.0, 0.0, 300.0, 1.0), (9_000.0, 200.0, 800.0, 1.2)],
        Terrain::Snow => [
            (7_000.0, 150.0, 600.0, 2.0),
            (21_000.0, 900.0, 2_800.0, 1.4),
        ],
        Terrain::Regolith => [(2_600.0, 30.0, 160.0, 3.0), (6_500.0, 250.0, 900.0, 2.2)],
    };
    let segments = 360;
    for (radius, lo, hi, slope) in rings {
        let coarse = ring_noise(&mut rng, 24);
        let mid = ring_noise(&mut rng, 70);
        let fine = ring_noise(&mut rng, 200);
        let height = |t: f32| -> f32 {
            let n = sample(&coarse, t) * 0.6 + sample(&mid, t) * 0.3 + sample(&fine, t) * 0.1;
            let shaped = match site.terrain {
                // Flat-topped mesas with sheer sides.
                Terrain::Sand => smoothstep(0.42, 0.48, n) * (0.8 + 0.2 * sample(&fine, t)),
                // Sharp peaks.
                Terrain::Snow => (n * n * 1.6).min(1.0) + 0.15 * sample(&fine, t * 3.0),
                _ => n,
            };
            let h = lo + (hi - lo) * shaped;
            match site.sea {
                Some((sea, _)) => {
                    let d = dir(t * TAU);
                    h * smoothstep(0.35, -0.1, Vec2::new(d.x, d.z).dot(sea))
                }
                None => h,
            }
        };
        for i in 0..segments {
            let (t0, t1) = (i as f32 / segments as f32, (i + 1) as f32 / segments as f32);
            let (h0, h1) = (height(t0), height(t1));
            if h0 < 1.0 && h1 < 1.0 {
                continue;
            }
            let (d0, d1) = (dir(t0 * TAU), dir(t1 * TAU));
            let rows = |d: Vec3, h: f32| -> [Vec3; 3] {
                [
                    d * (radius - h * slope),
                    d * radius + Vec3::Y * h,
                    d * (radius + h * slope * 1.5) + Vec3::Y * h * 0.3,
                ]
            };
            let (a, b) = (rows(d0, h0), rows(d1, h1));
            for k in 0..2 {
                meshes::quad(&mut m, a[k], a[k + 1], b[k + 1], b[k]);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(world: World, seed: u64) -> Site {
        Site::new(16.0, world, seed, 200.0, [1.0, 0.92, 0.75], 0.005, None)
    }

    #[test]
    fn a_city_looks_the_same_each_visit() {
        let (a, b) = (site(World::Terran, 7), site(World::Terran, 7));
        assert_eq!(a.sun, b.sun);
        assert_eq!(a.ship_yaw, b.ship_yaw);
        assert_eq!(
            skyline(&a, 16.0).vertices.len(),
            skyline(&b, 16.0).vertices.len()
        );
        assert_ne!(
            site(World::Terran, 8).sun,
            a.sun,
            "each city has its own hour"
        );
    }

    #[test]
    fn the_sun_is_up_and_on_the_cameras_side() {
        for seed in 0..200 {
            let s = site(World::Desert, seed);
            assert!(s.sun.y > 0.08, "seed {seed}: sun too low");
            // The camera stands toward -x.
            assert!(s.sun.x < 0.0, "seed {seed}: sun behind the city");
        }
    }

    #[test]
    fn a_waterfront_city_builds_nothing_in_the_sea() {
        for seed in 0..40 {
            let s = site(World::Ocean, seed);
            let (sea, coast) = s.sea.expect("an ocean world has a shore");
            let m = skyline(&s, 16.0);
            assert!(!m.vertices.is_empty());
            for v in &m.vertices {
                let p = Vec2::new(v.pos[0], v.pos[2]);
                assert!(
                    p.dot(sea) < coast,
                    "seed {seed}: building in the sea at {p}"
                );
            }
        }
    }
}
