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
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) light: vec4<f32>,
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

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.3183099 + vec3<f32>(0.1, 0.2, 0.3));
    q = q * 17.0;
    return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x),
            mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y),
        mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x),
            mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y),
        u.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 5; i = i + 1) {
        sum = sum + amp * noise(q);
        q = q * 2.03;
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
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let mode = u32(i.params.x);
    var n = normalize(i.normal);
    // Two-sided lighting for meshes seen from "inside".
    if (dot(n, i.view_dir) < 0.0 && mode == PLAIN) { n = -n; }
    let seed = vec3<f32>(i.params.z * 0.001, i.params.z * 0.0007, i.params.z * 0.0013);
    let p = i.obj + fract(seed) * 50.0;
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
        let sea = vec3<f32>(0.05, 0.14, 0.32);
        let ground = mix(vec3<f32>(0.22, 0.32, 0.14), vec3<f32>(0.42, 0.36, 0.24), fbm(p * 7.0));
        base = select(sea, ground, land > 0.52);
        let ice = smoothstep(0.82, 0.9, abs(i.obj.y) + fbm(p * 5.0) * 0.1);
        base = mix(base, vec3<f32>(0.85, 0.9, 0.95), ice);
        let clouds = smoothstep(0.55, 0.75, fbm(p * 3.0 + vec3<f32>(g.misc.x * 0.001)));
        base = mix(base, vec3<f32>(0.9), clouds * 0.7);
        atmosphere = vec3<f32>(0.3, 0.5, 1.0);
    } else if (mode == ROCKY || mode == DESERT || mode == BARREN) {
        let t = fbm(p * 3.0);
        base = base * (0.65 + 0.7 * t);
        if (mode == BARREN) {
            // Crater-ish speckle.
            base = base * (0.8 + 0.4 * smoothstep(0.4, 0.8, noise(p * 30.0)));
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
    let ambient = 0.015;
    var color = base * (lit * g.star_color.rgb + vec3<f32>(ambient)) + emissive;
    // Thin atmosphere glow at the limb, on the day side.
    let rim = pow(1.0 - mu, 3.0) * smoothstep(-0.2, 0.3, ndl);
    color = color + atmosphere * rim * i.light.w;
    // Small emissive parts (windows, lamps) for built things.
    color = color + i.color.rgb * i.params.y;
    return vec4<f32>(color, 1.0);
}
