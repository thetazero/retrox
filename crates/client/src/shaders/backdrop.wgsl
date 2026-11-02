// The sky behind everything: the galaxy's disc as a glowing band (brighter
// and warmer toward the core, split by dust lanes), faint coloured nebulae
// and a dust of unresolved stars. Real neighbouring systems are drawn on top
// as points (sky.wgsl).

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time, y: exposure, z: width, w: height
    misc: vec4<f32>,
    // xyz: direction to the galactic core, w: how far out we are (0..1)
    sky: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.ndc = uv * 2.0 - 1.0;
    o.clip = vec4<f32>(o.ndc, 0.0, 1.0);
    return o;
}

fn hash3(c: vec3<i32>) -> f32 {
    var h = bitcast<u32>(c.x) * 73856093u ^ bitcast<u32>(c.y) * 19349663u ^ bitcast<u32>(c.z) * 83492791u;
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967295.0);
}

fn noise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(mix(hash3(i), hash3(i + vec3<i32>(1, 0, 0)), u.x),
            mix(hash3(i + vec3<i32>(0, 1, 0)), hash3(i + vec3<i32>(1, 1, 0)), u.x), u.y),
        mix(mix(hash3(i + vec3<i32>(0, 0, 1)), hash3(i + vec3<i32>(1, 0, 1)), u.x),
            mix(hash3(i + vec3<i32>(0, 1, 1)), hash3(i + vec3<i32>(1, 1, 1)), u.x), u.y),
        u.z);
}

const OCTAVE = mat3x3<f32>(
    vec3<f32>(0.00, 0.80, 0.60),
    vec3<f32>(-0.80, 0.36, -0.48),
    vec3<f32>(-0.60, -0.48, 0.64),
);

fn fbm(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 5; i = i + 1) {
        sum = sum + amp * noise(q);
        q = OCTAVE * q * 2.02;
        amp = amp * 0.5;
    }
    return sum;
}

// Unresolved stars: at most one per cell of a lattice on the sphere.
fn star_dust(dir: vec3<f32>, scale: f32) -> f32 {
    let q = dir * scale;
    let c = vec3<i32>(floor(q));
    let h = hash3(c);
    let centre = vec3<f32>(c) + 0.25 + 0.5 * vec3<f32>(h, hash3(c + vec3<i32>(11, 5, 3)), hash3(c + vec3<i32>(2, 17, 9)));
    let d = length(q - centre);
    let size = fwidth(q.x) + fwidth(q.y) + fwidth(q.z);
    // Only one cell in ~40 holds a star bright enough to see.
    let bright = max(h - 0.975, 0.0) * 40.0;
    return (1.0 - smoothstep(0.0, size * 1.2 + 1e-4, d)) * bright * bright;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(i.ndc, 0.5, 1.0);
    let dir = normalize(far.xyz / far.w);
    let core = normalize(g.sky.xyz);
    let out = g.sky.w;

    // Galactic latitude is the world's Y (the disc lies in XZ).
    let lat = dir.y;
    let flat = normalize(vec2<f32>(dir.x, dir.z) + vec2<f32>(1e-5, 0.0));
    let toward = dot(flat, normalize(core.xz + vec2<f32>(1e-5, 0.0)));
    // (Clamped: pow of a rounding-error negative is NaN.)
    let coreness = pow(clamp(0.5 + 0.5 * toward, 0.0, 1.0), mix(2.0, 5.0, out));
    let width = mix(0.06, 0.2, coreness);
    let band = exp(-(lat * lat) / (width * width));

    let clouds = fbm(dir * 5.0);
    let detail = fbm(dir * 16.0 + vec3<f32>(4.0));
    let lane_w = width * 0.25;
    let lane = smoothstep(0.42, 0.68, fbm(dir * 9.0 + vec3<f32>(1.0, 7.0, 2.0)))
        * exp(-((lat - 0.01) * (lat - 0.01)) / (lane_w * lane_w));
    let density = band * (0.3 + 0.7 * clouds) * (0.75 + 0.5 * detail) * (1.0 - 0.85 * lane);

    // Cool violet arms, warm core.
    let arm = vec3<f32>(0.32, 0.36, 0.75);
    let warm = vec3<f32>(1.0, 0.68, 0.42);
    var c = mix(arm, warm, clamp(coreness * 1.3, 0.0, 1.0)) * density * (0.012 + 0.05 * coreness);
    // The bulge, a soft glow around the core itself.
    let bulge = pow(max(dot(dir, core), 0.0), 18.0) * (1.0 - 0.7 * lane);
    c = c + warm * bulge * 0.05;

    // Faint nebulae: magenta and teal veils, mostly near the disc.
    let veil = exp(-(lat * lat) / 0.12);
    let neb_a = pow(fbm(dir * 2.2 + vec3<f32>(9.0, 1.0, 4.0)), 3.0);
    let neb_b = pow(fbm(dir * 2.8 + vec3<f32>(2.0, 8.0, 5.0)), 3.0);
    c = c + (vec3<f32>(0.55, 0.12, 0.38) * neb_a + vec3<f32>(0.08, 0.32, 0.38) * neb_b) * 0.035 * (0.35 + veil);

    // Unresolved stars, denser in the band.
    let dust = star_dust(dir, 380.0) + star_dust(dir.zxy, 620.0) * band;
    c = c + vec3<f32>(0.9, 0.92, 1.0) * dust * (0.08 + 0.4 * band);

    // A deep indigo floor rather than pure black.
    c = c + vec3<f32>(0.0012, 0.0010, 0.0028);
    return vec4<f32>(c, 1.0);
}
