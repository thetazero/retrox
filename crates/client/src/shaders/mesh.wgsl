// Lit meshes: ships, stations, rocks, gates, and procedural planets/stars.

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time (s), y: exposure
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) shade: f32,
    @location(10) paint: f32,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    // x: surface mode, y: emissive, z: seed, w: detail scale (built
    // things: cells per object unit) or sea level (ocean worlds)
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
    @location(7) paint: f32,
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
    o.paint = v.paint;
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
// Glows (fs_glow, additive).
const SHIELD: u32 = 20u;
const FIRE: u32 = 21u;
const RING: u32 = 22u;

// Paints for built things (must match meshes.rs).
const HULL: u32 = 0u;
const RED: u32 = 1u;
const GLASS: u32 = 2u;
const SOLAR: u32 = 3u;
const FOIL: u32 = 4u;
const WINDOWS: u32 = 5u;
const METAL: u32 = 6u;
const BEACON: u32 = 7u;
const LAMP: u32 = 8u;
const OCHRE: u32 = 9u;
const HAZARD: u32 = 10u;
const GUNMETAL: u32 = 11u;
const OLIVE: u32 = 12u;
const SKY: u32 = 13u;

// Sovietwave ambient: a cool teal sky above, a dusky violet below.
fn ambient_light(n: vec3<f32>) -> vec3<f32> {
    return mix(vec3<f32>(0.05, 0.025, 0.075), vec3<f32>(0.03, 0.065, 0.085), n.y * 0.5 + 0.5);
}

// Faint panel seams on a grid in object space, faded out before they alias.
fn seams(p: vec3<f32>, w: vec3<f32>) -> f32 {
    let q = abs(fract(p) - 0.5);
    let fade = 1.0 - smoothstep(0.08, 0.25, max(w.x, max(w.y, w.z)));
    let edge = max(max(1.0 - smoothstep(0.0, w.x * 1.5 + 0.01, 0.5 - q.x),
                       1.0 - smoothstep(0.0, w.y * 1.5 + 0.01, 0.5 - q.y)),
                   1.0 - smoothstep(0.0, w.z * 1.5 + 0.01, 0.5 - q.z));
    return edge * fade;
}

// fw: fwidth of the detail-scaled object position, taken in uniform control flow.
fn built(i: VOut, n: vec3<f32>, mu: f32, fw: vec3<f32>) -> vec3<f32> {
    let paint = u32(round(i.paint));
    let scale = i.params.w;
    let p = i.obj * max(scale, 0.0001);
    var base = i.color.rgb * i.shade;
    var spec = 0.06;
    var gloss = 12.0;
    var emissive = vec3<f32>(0.0);

    if (paint == RED) {
        base = vec3<f32>(0.6, 0.075, 0.05) * i.shade;
        spec = 0.12;
    } else if (paint == GLASS) {
        base = vec3<f32>(0.015, 0.02, 0.03);
        spec = 0.9;
        gloss = 80.0;
        emissive = vec3<f32>(1.0, 0.5, 0.18) * 0.35;
    } else if (paint == SOLAR) {
        let cell = abs(fract(p * 3.0) - 0.5);
        let grid = step(0.44, max(cell.x, cell.z));
        base = mix(vec3<f32>(0.03, 0.045, 0.14), vec3<f32>(0.32, 0.3, 0.28), grid);
        spec = 0.8;
        gloss = 40.0;
    } else if (paint == FOIL) {
        let crinkle = noise(p * 6.0) * 0.6 + noise(p * 17.0) * 0.4;
        base = vec3<f32>(0.78, 0.5, 0.14) * (0.6 + 0.7 * crinkle);
        spec = 0.9 * crinkle + 0.2;
        gloss = 20.0;
    } else if (paint == WINDOWS) {
        // Rows of windows, some lit.
        let q = p * vec3<f32>(2.0, 3.0, 2.0);
        let c = vec3<i32>(floor(q));
        let f = abs(fract(q) - 0.5);
        let pane = step(max(f.x, f.z), 0.32) * step(f.y, 0.22);
        let lit = step(0.45, hash3(c));
        let warm = mix(vec3<f32>(1.0, 0.62, 0.3), vec3<f32>(0.6, 0.9, 1.0), step(0.93, hash3(c + vec3<i32>(7, 3, 1))));
        let far = smoothstep(0.15, 0.5, length(fw * vec3<f32>(2.0, 3.0, 2.0)));
        emissive = warm * mix(pane * lit, 0.25, far) * 1.6;
        base = base * (1.0 - 0.7 * pane);
    } else if (paint == METAL) {
        base = vec3<f32>(0.09, 0.085, 0.09) * i.shade;
        spec = 0.5;
        gloss = 30.0;
    } else if (paint == BEACON) {
        let on = step(0.8, fract(g.misc.x * 0.6 + i.params.z * 0.13));
        base = vec3<f32>(0.3, 0.02, 0.02);
        emissive = vec3<f32>(1.0, 0.08, 0.04) * (0.3 + 12.0 * on);
    } else if (paint == LAMP) {
        base = vec3<f32>(0.9, 0.85, 0.75);
        emissive = vec3<f32>(1.0, 0.86, 0.62) * 5.0;
    } else if (paint == OCHRE) {
        // Worn tractor mustard.
        base = vec3<f32>(0.44, 0.32, 0.12) * i.shade;
        spec = 0.08;
    } else if (paint == HAZARD) {
        // Diagonal stripes, a fixed width in object units.
        let stripe = step(0.5, fract((i.obj.x + i.obj.y + i.obj.z) * 1.6));
        base = mix(vec3<f32>(0.035, 0.03, 0.03), vec3<f32>(0.75, 0.55, 0.08), stripe) * i.shade;
        spec = 0.1;
    } else if (paint == GUNMETAL) {
        // Deep teal-grey military enamel.
        base = vec3<f32>(0.15, 0.19, 0.2) * i.shade;
        spec = 0.18;
        gloss = 24.0;
    } else if (paint == OLIVE) {
        base = vec3<f32>(0.22, 0.24, 0.13) * i.shade;
        spec = 0.08;
    } else if (paint == SKY) {
        base = vec3<f32>(0.25, 0.52, 0.78) * i.shade;
        spec = 0.3;
        gloss = 40.0;
    }
    let enamel = paint == HULL || paint == RED || paint == OCHRE || paint == GUNMETAL
        || paint == OLIVE || paint == SKY;
    if (scale > 0.0 && enamel) {
        base = base * (1.0 - 0.3 * seams(p, fw));
        // Wear, faded out before it aliases: grime streaked along the
        // ship's length, faded and chipped paint.
        let near = 1.0 - smoothstep(0.15, 0.6, max(fw.x, max(fw.y, fw.z)));
        let streak = fbm(p * vec3<f32>(2.5, 2.5, 0.35) + vec3<f32>(11.0));
        let grime = smoothstep(0.38, 0.72, streak) * near;
        let blotch = fbm(p * 0.8 + vec3<f32>(5.0));
        let chips = step(0.86, noise(p * 14.0)) * smoothstep(0.55, 0.7, blotch) * near;
        let faded = mix(base, vec3<f32>(dot(base, vec3<f32>(0.33))), 0.4 * blotch) * (0.92 + 0.16 * blotch);
        base = mix(faded * (1.0 - 0.45 * grime), vec3<f32>(0.12, 0.11, 0.1), chips * 0.7);
        spec = spec * (1.0 - 0.5 * grime);
    }

    let light_color = mix(vec3<f32>(1.0), g.star_color.rgb, 0.4);
    let ndl = dot(n, i.light.xyz);
    // Slightly wrapped key light: softer terminators on hard facets.
    let lit = clamp((ndl + 0.1) / 1.1, 0.0, 1.0) * i.light.w;
    let h = normalize(i.light.xyz + i.view_dir);
    let highlight = pow(max(dot(n, h), 0.0), gloss) * spec * step(0.0, ndl) * i.light.w;
    // Cool rim so silhouettes read against the sky.
    let rim = pow(1.0 - mu, 4.0) * vec3<f32>(0.1, 0.16, 0.22);
    // Battle damage: soot blooming over the paint in patches, and on a
    // burnt-out wreck, embers glowing in the cracks.
    let scorch = i.color.a;
    if (scorch > 0.0) {
        let q = i.obj * 0.45 + vec3<f32>(i.params.z * 3.1);
        let soot_mask = fbm(q);
        let burnt = smoothstep(1.0 - scorch * 1.1, 1.05 - scorch * 0.9, soot_mask);
        base = mix(base, vec3<f32>(0.025, 0.02, 0.018), burnt * 0.92);
        spec = spec * (1.0 - burnt);
        let crack = 1.0 - smoothstep(0.0, 0.035, abs(fbm(q * 2.7 + vec3<f32>(5.0)) - 0.5));
        let hot = smoothstep(0.85, 1.0, scorch);
        let flicker = 0.75 + 0.25 * sin(g.misc.x * 7.0 + soot_mask * 30.0);
        emissive = emissive + vec3<f32>(1.0, 0.32, 0.06) * crack * burnt * hot * flicker * 3.0;
    }

    var color = base * (lit * light_color + ambient_light(n) * 2.0 + vec3<f32>(0.04 * mu))
        + highlight * light_color + rim + emissive;
    color = color + i.color.rgb * i.params.y;
    return color;
}

// Additive glows over the solid scene. light.w is how far through its
// life the glow is (0..1); params.y its strength.
@fragment
fn fs_glow(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = u32(round(i.params.x));
    // Spheres show their near side only; rings both.
    if (!front && mode != RING) { discard; }
    let n = normalize(i.normal);
    let mu = clamp(dot(n, i.view_dir), 0.0, 1.0);
    let k = clamp(i.light.w, 0.0, 1.0);
    let fade = (1.0 - k) * (1.0 - k);
    let t = g.misc.x;

    if (mode == SHIELD) {
        // A field of fine horizontal scan bands, faint at the limb and
        // flaring round where the shot struck.
        let d = normalize(i.obj);
        let at = max(dot(d, normalize(i.light.xyz)), 0.0);
        let spot = pow(at, 14.0) * 2.4 + pow(at, 4.0) * 0.35;
        let rim = pow(1.0 - mu, 4.0) * 0.18;
        let bands = 0.55 + 0.45 * sin(i.obj.y * 26.0 - t * 18.0);
        let ripple = 0.6 + 0.4 * sin(acos(clamp(dot(d, normalize(i.light.xyz)), -1.0, 1.0)) * 18.0 - k * 30.0);
        let glow = (rim * bands + spot * ripple) * fade * i.params.y;
        return vec4<f32>(i.color.rgb * glow, 1.0);
    }
    if (mode == FIRE) {
        // Churning flame: white-hot at the start and in the core, cooling
        // through orange to a dull red as it spreads.
        let p = i.obj * 2.2 + vec3<f32>(i.params.z * 7.0, i.params.z, t * 0.4);
        let churn = fbm(p + vec3<f32>(fbm(p * 1.7) * 1.5));
        let core = pow(mu, 0.8);
        let heat = clamp((churn * 1.3 - 0.2) * core * (1.15 - k) * 1.4, 0.0, 1.5);
        let cool = vec3<f32>(0.55, 0.08, 0.02);
        let warm = vec3<f32>(1.0, 0.42, 0.08);
        let white = vec3<f32>(1.0, 0.9, 0.7);
        var c = mix(cool, warm, smoothstep(0.15, 0.6, heat));
        c = mix(c, white, smoothstep(0.75, 1.2, heat) * (1.0 - k));
        let edge = smoothstep(0.05, 0.35, mu);
        return vec4<f32>(c * heat * edge * fade * i.params.y, 1.0);
    }
    // RING: lit at the rim.
    let rim = pow(1.0 - mu, 2.0);
    return vec4<f32>(i.color.rgb * rim * fade * i.params.y, 1.0);
}

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = u32(round(i.params.x));
    let fw = fwidth(i.obj * max(i.params.w, 0.0001));
    // Spheres are wound consistently: only their front faces count. (Kit
    // meshes stay two-sided.)
    if (mode != PLAIN && !front) { discard; }
    var n = normalize(i.normal);
    // Two-sided lighting for meshes seen from "inside".
    if (dot(n, i.view_dir) < 0.0 && mode == PLAIN) { n = -n; }
    let mu = max(dot(n, i.view_dir), 0.0);

    if (mode == PLAIN) {
        return vec4<f32>(built(i, n, mu, fw), 1.0);
    }

    // Per-body offset into noise space (seed is an integer < 10000).
    let s = i.params.z;
    let p = i.obj + vec3<f32>(s % 97.0, (s / 97.0) % 89.0, s * 0.37 % 83.0);

    if (mode == STAR) {
        let limb = 0.55 + 0.45 * pow(mu, 0.5);
        let granules = 0.9 + 0.1 * fbm(p * 12.0 + vec3<f32>(g.misc.x * 0.02));
        return vec4<f32>(i.color.rgb * i.params.y * limb * granules, 1.0);
    }

    var base = i.color.rgb * i.shade;
    var emissive = vec3<f32>(0.0);
    var atmosphere = vec3<f32>(0.0);
    var water = 0.0;
    if (mode == GAS) {
        let warp = fbm(p * 2.5) * 4.0;
        let bands = sin(i.obj.y * 14.0 + warp) + 0.4 * sin(i.obj.y * 37.0 + warp * 1.7);
        base = mix(base * 0.62, base * 1.18, 0.5 + 0.35 * bands);
        // A great storm, for some.
        let storm = smoothstep(0.82, 0.95, fbm(p * 1.3 + vec3<f32>(3.0)));
        base = mix(base, base * vec3<f32>(1.25, 0.8, 0.65), storm * 0.6);
        atmosphere = base * 0.6;
    } else if (mode == OCEAN) {
        // Sea level from params.w: higher means more ocean.
        let sea_level = select(0.56, i.params.w, i.params.w > 0.0);
        let land = fbm(p * 2.2) + fbm(p * 9.0) * 0.12;
        let depth = smoothstep(sea_level - 0.25, sea_level, land);
        let sea = mix(vec3<f32>(0.01, 0.035, 0.11), vec3<f32>(0.03, 0.12, 0.22), depth);
        let lowland = mix(vec3<f32>(0.11, 0.2, 0.07), vec3<f32>(0.3, 0.26, 0.14), fbm(p * 6.0));
        let highland = vec3<f32>(0.36, 0.32, 0.27);
        let ground = mix(lowland, highland, smoothstep(sea_level + 0.08, sea_level + 0.25, land));
        let is_land = step(sea_level, land);
        base = mix(sea, ground, is_land);
        water = 1.0 - is_land;
        let ice = smoothstep(0.8, 0.88, abs(i.obj.y) + fbm(p * 5.0) * 0.12);
        base = mix(base, vec3<f32>(0.85, 0.9, 0.95), ice);
        water = water * (1.0 - ice);
        let clouds = smoothstep(0.5, 0.75, fbm(p * 3.0 + vec3<f32>(g.misc.x * 0.001)));
        base = mix(base, vec3<f32>(0.9), clouds * 0.6);
        water = water * (1.0 - clouds);
        atmosphere = vec3<f32>(0.3, 0.55, 1.0);
    } else if (mode == ROCKY || mode == DESERT || mode == BARREN) {
        let t = fbm(p * 3.0);
        base = base * (0.6 + 0.8 * t);
        if (mode == BARREN) {
            // Crater-ish speckle.
            base = base * (0.75 + 0.5 * smoothstep(0.4, 0.8, noise(p * 9.0)));
        }
        if (mode == DESERT) {
            let dunes = sin(i.obj.y * 40.0 + fbm(p * 4.0) * 8.0);
            base = base * (0.92 + 0.08 * dunes);
            atmosphere = vec3<f32>(0.75, 0.45, 0.22) * 0.5;
        }
        if (mode == ROCKY) { atmosphere = vec3<f32>(0.25, 0.3, 0.4) * 0.5; }
    } else if (mode == ICE) {
        base = mix(base, vec3<f32>(0.8, 0.84, 0.88), fbm(p * 4.0) * 0.6) * 0.8;
        let cracks = 1.0 - smoothstep(0.0, 0.03, abs(fbm(p * 5.0) - 0.5));
        base = mix(base, vec3<f32>(0.35, 0.55, 0.7), cracks * 0.5);
        atmosphere = vec3<f32>(0.5, 0.7, 0.9) * 0.2;
    } else if (mode == LAVA) {
        let cracks = smoothstep(0.48, 0.52, fbm(p * 4.0));
        base = vec3<f32>(0.08, 0.06, 0.05);
        emissive = vec3<f32>(1.6, 0.45, 0.08) * (1.0 - cracks);
    }

    let ndl = dot(n, i.light.xyz);
    let lit = max(ndl, 0.0) * i.light.w;
    // Starlight, desaturated a little so orange suns don't tint everything.
    let light_color = mix(vec3<f32>(1.0), g.star_color.rgb, 0.4);
    var color = base * (lit * light_color + vec3<f32>(0.012)) + emissive;
    // Sun glint on open water.
    let h = normalize(i.light.xyz + i.view_dir);
    color = color + water * pow(max(dot(n, h), 0.0), 60.0) * 0.6 * lit * light_color;
    // Atmosphere: a glow at the limb on the day side, reddening toward the
    // terminator.
    let rim = pow(1.0 - mu, 2.5) * smoothstep(-0.25, 0.35, ndl);
    let dusk = vec3<f32>(1.0, 0.42, 0.22) * length(atmosphere);
    let air = mix(dusk, atmosphere, smoothstep(0.0, 0.45, ndl));
    color = color + air * rim * i.light.w * 0.8;
    return vec4<f32>(color, 1.0);
}
