// Lit meshes: ships, stations, rocks, gates, and procedural planets/stars.

struct Globals {
    view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time (s), y: exposure
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) shade: f32,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    // x: surface mode, y: emissive, z: seed, w: unused
    @location(8) params: vec4<f32>,
    // xyz: direction to the star (world), w: intensity
    @location(9) light: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) obj: vec3<f32>,
    // Per-instance values: flat, so huge distances can't perturb them.
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) light: vec4<f32>,
    @location(5) shade: f32,
    @location(6) view_dir: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = model * vec4<f32>(v.pos, 1.0);
    var o: VOut;
    o.clip = g.view_proj * world;
    o.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    o.obj = v.pos;
    o.color = v.color;
    o.params = v.params;
    o.light = v.light;
    o.shade = v.shade;
    o.view_dir = normalize(-world.xyz);
    return o;
}

// Integer lattice hash: stable at any coordinate, no visible patterns.
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
        // Rotate (orthonormal) and double, so octaves don't line up.
        q = OCTAVE * q * 2.02;
        amp = amp * 0.5;
    }
    return sum;
}

// Surface modes (must match render.rs).
const PLAIN: u32 = 0u;
const STAR: u32 = 1u;
const ROCKY: u32 = 2u;
const GAS: u32 = 3u;
const OCEAN: u32 = 4u;
const ICE: u32 = 5u;
const LAVA: u32 = 6u;
const DESERT: u32 = 7u;
const BARREN: u32 = 8u;

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = u32(round(i.params.x));
    // Spheres are wound consistently: only their front faces count. (Kit
    // meshes stay two-sided.)
    if (mode != PLAIN && !front) { discard; }
    var n = normalize(i.normal);
    // Two-sided lighting for meshes seen from "inside".
    if (dot(n, i.view_dir) < 0.0 && mode == PLAIN) { n = -n; }
    // Per-body offset into noise space (seed is an integer < 10000).
    let s = i.params.z;
    let p = i.obj + vec3<f32>(s % 97.0, (s / 97.0) % 89.0, s * 0.37 % 83.0);
    let mu = max(dot(n, i.view_dir), 0.0);

    if (mode == STAR) {
        let limb = 0.55 + 0.45 * pow(mu, 0.5);
        let granules = 0.9 + 0.1 * fbm(p * 12.0 + vec3<f32>(g.misc.x * 0.02));
        return vec4<f32>(i.color.rgb * i.params.y * limb * granules, 1.0);
    }

    var base = i.color.rgb * i.shade;
    var emissive = vec3<f32>(0.0);
    var atmosphere = vec3<f32>(0.0);
    if (mode == GAS) {
        let bands = sin(i.obj.y * 14.0 + fbm(p * 2.5) * 4.0);
        base = mix(base * 0.7, base * 1.15, 0.5 + 0.5 * bands);
        atmosphere = base * 0.6;
    } else if (mode == OCEAN) {
        let land = fbm(p * 2.2);
        let sea = mix(vec3<f32>(0.02, 0.06, 0.16), vec3<f32>(0.04, 0.12, 0.26), smoothstep(0.35, 0.56, land));
        let ground = mix(vec3<f32>(0.16, 0.26, 0.1), vec3<f32>(0.36, 0.32, 0.22), fbm(p * 6.0));
        base = select(sea, ground, land > 0.56);
        let ice = smoothstep(0.82, 0.9, abs(i.obj.y) + fbm(p * 5.0) * 0.1);
        base = mix(base, vec3<f32>(0.85, 0.9, 0.95), ice);
        let clouds = smoothstep(0.55, 0.75, fbm(p * 3.0 + vec3<f32>(g.misc.x * 0.001)));
        base = mix(base, vec3<f32>(0.85), clouds * 0.45);
        atmosphere = vec3<f32>(0.3, 0.5, 1.0);
    } else if (mode == ROCKY || mode == DESERT || mode == BARREN) {
        let t = fbm(p * 3.0);
        base = base * (0.65 + 0.7 * t);
        if (mode == BARREN) {
            // Crater-ish speckle.
            base = base * (0.8 + 0.4 * smoothstep(0.4, 0.8, noise(p * 9.0)));
        }
        if (mode == ROCKY) { atmosphere = vec3<f32>(0.25, 0.3, 0.4) * 0.5; }
        if (mode == DESERT) { atmosphere = vec3<f32>(0.6, 0.4, 0.2) * 0.5; }
    } else if (mode == ICE) {
        base = mix(base, vec3<f32>(0.95, 0.97, 1.0), fbm(p * 4.0) * 0.6);
    } else if (mode == LAVA) {
        let cracks = smoothstep(0.48, 0.52, fbm(p * 4.0));
        base = vec3<f32>(0.08, 0.06, 0.05);
        emissive = vec3<f32>(1.6, 0.45, 0.08) * (1.0 - cracks);
    }

    let ndl = dot(n, i.light.xyz);
    let lit = max(ndl, 0.0) * i.light.w;
    // Starlight, desaturated a little so orange suns don't tint everything.
    let light_color = mix(vec3<f32>(1.0), g.star_color.rgb, 0.4);
    var ambient = 0.03;
    var fill = 0.0;
    if (mode == PLAIN) {
        // Built things get some fill so they read in shadow.
        ambient = 0.06;
        fill = 0.12 * mu;
    }
    var color = base * (lit * light_color + vec3<f32>(ambient + fill)) + emissive;
    // Thin atmosphere glow at the limb, on the day side.
    let rim = pow(1.0 - mu, 3.0) * smoothstep(-0.2, 0.3, ndl);
    color = color + atmosphere * rim * i.light.w;
    // Small emissive parts (windows, lamps) for built things.
    color = color + i.color.rgb * i.params.y;
    return vec4<f32>(color, 1.0);
}
