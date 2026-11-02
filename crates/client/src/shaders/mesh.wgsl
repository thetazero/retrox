// Lit meshes: ships, stations, rocks, gates, and procedural planets/stars.

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time (s), y: exposure, z: width, w: height
    misc: vec4<f32>,
    // Where the pilot's own hull opens round the combat target: x, y in
    // pixels, z: radius in pixels, w: strength (render.rs reveal_window).
    reveal: vec4<f32>,
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
    // Rock: packed vein points. Dust: the camera from the field's centre.
    @location(11) aux: vec4<f32>,
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
    // Camera-relative world position.
    @location(8) wpos: vec3<f32>,
    @location(9) @interpolate(flat) aux: vec4<f32>,
    // Metres per object unit (the model's scale).
    @location(10) @interpolate(flat) scale: f32,
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
    o.wpos = world.xyz;
    o.aux = v.aux;
    o.scale = length(v.m0.xyz);
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
const ROCK: u32 = 9u;
// Glows (fs_glow, additive).
const SHIELD: u32 = 20u;
const FIRE: u32 = 21u;
const RING: u32 = 22u;
const FLASH: u32 = 23u;
const DUST: u32 = 25u;
const CORONA: u32 = 24u;

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

// fbm with each octave faded out as it nears the pixel size: `w` is the
// pixel's footprint in units of `p`.
fn fbm_aa(p: vec3<f32>, w: f32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    var f = 1.0;
    for (var i = 0; i < 5; i = i + 1) {
        let keep = 1.0 - smoothstep(0.2, 0.5, f * w);
        // A faded octave leaves its average behind.
        sum = sum + amp * mix(0.5, noise(q), keep);
        q = OCTAVE * q * 2.02;
        f = f * 2.02;
        amp = amp * 0.5;
    }
    return sum;
}

// Value noise with its gradient (x: value 0..1, yzw: d/dp).
fn noised(p: vec3<f32>) -> vec4<f32> {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let du = 6.0 * f * (1.0 - f);
    let a = hash3(i);
    let b = hash3(i + vec3<i32>(1, 0, 0));
    let c = hash3(i + vec3<i32>(0, 1, 0));
    let d = hash3(i + vec3<i32>(1, 1, 0));
    let e = hash3(i + vec3<i32>(0, 0, 1));
    let f1 = hash3(i + vec3<i32>(1, 0, 1));
    let g1 = hash3(i + vec3<i32>(0, 1, 1));
    let h = hash3(i + vec3<i32>(1, 1, 1));
    let k1 = b - a;
    let k2 = c - a;
    let k3 = e - a;
    let k4 = a - b - c + d;
    let k5 = a - c - e + g1;
    let k6 = a - b - e + f1;
    let k7 = -a + b + c - d + e - f1 - g1 + h;
    let v = a + k1 * u.x + k2 * u.y + k3 * u.z + k4 * u.x * u.y + k5 * u.y * u.z
        + k6 * u.z * u.x + k7 * u.x * u.y * u.z;
    let grad = du * vec3<f32>(
        k1 + k4 * u.y + k6 * u.z + k7 * u.y * u.z,
        k2 + k5 * u.z + k4 * u.x + k7 * u.z * u.x,
        k3 + k6 * u.x + k5 * u.y + k7 * u.x * u.y,
    );
    return vec4<f32>(v, grad);
}

// How thick a belt is, as a fraction of its radius (as the rocks lie,
// fields.rs: within about 0.4 of the radius above and below).
const BELT_THICKNESS: f32 = 0.35;

// Sunlight scattered toward the camera by the belt's dust, looking along
// `d`, per unit of dust depth: a faint grey veil everywhere and a glow
// round the sun, where fine dust scatters forward.
fn dust_light(d: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let c = max(dot(d, sun), 0.0);
    let phase = 0.008 + 0.03 * pow(c, 3.0) + 0.22 * pow(c, 24.0);
    let light = mix(vec3<f32>(1.0), g.star_color.rgb, 0.35);
    return light * vec3<f32>(0.92, 0.9, 0.88) * phase;
}

// The surface of bare rock, in metres from an object-aligned origin:
// self-similar lumps from the rock's own size down to the pixel, and
// craters at several scales, each faded out before it aliases (`wm`: the
// pixel's footprint in metres). Returns the height (x, metres), its
// gradient (yzw), and in `marks` how much of the point lies in a crater's
// floor (x) and on a fresh rim (y).
struct Relief {
    h: vec4<f32>,
    marks: vec2<f32>,
};

fn relief(p: vec3<f32>, size: f32, wm: f32, seed: f32) -> Relief {
    var out: Relief;
    var h = vec4<f32>(0.0);
    // Lumps and grit: amplitude in step with wavelength, so a quarry and
    // a boulder are equally rough at their own scale, and a quarry seen
    // close still has metre grit and pebble texture.
    var m = mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    var lambda = size * 0.6;
    let off = vec3<f32>(seed % 17.0, seed % 13.0, seed % 11.0) * 3.7;
    for (var k = 0; k < 13; k = k + 1) {
        let keep = 1.0 - smoothstep(0.35, 0.7, wm / lambda);
        if (keep <= 0.0) { break; }
        let f = 1.0 / lambda;
        let n = noised(m * p * f + off);
        let amp = lambda * 0.12 * keep;
        h = h + vec4<f32>(amp * (n.x - 0.5), amp * f * (transpose(m) * n.yzw));
        m = OCTAVE * m;
        lambda = lambda / 2.02;
    }
    // Craters: a few big bowls, many small ones, each in its own cell so
    // only that cell is looked at. Bowls with a raised rim.
    var floor_ = 0.0;
    var rim = 0.0;
    var cell = size * 0.9;
    for (var k = 0; k < 6; k = k + 1) {
        if (cell < wm * 6.0) { break; }
        let q = p / cell + off * (1.0 + f32(k));
        let c = floor(q);
        let ci = vec3<i32>(c);
        let r0 = hash3(ci + vec3<i32>(k * 31, 7, 3));
        if (r0 < 0.42) {
            let jit = vec3<f32>(hash3(ci + vec3<i32>(1, k, 5)), hash3(ci + vec3<i32>(9, 2, k)), hash3(ci + vec3<i32>(k, 4, 8)));
            let centre = c + 0.3 + 0.4 * jit;
            let rc = 0.1 + 0.18 * hash3(ci + vec3<i32>(3, 3, k * 5));
            let v = q - centre;
            let dist = length(v);
            let t = dist / rc;
            if (t < 1.8 && dist > 1e-5) {
                // Some fresh and deep, most worn shallow by later hits.
                let depth = rc * cell * (0.06 + 0.3 * pow(hash3(ci + vec3<i32>(5, k, 1)), 2.0));
                // Profile in t and its slope.
                var prof = 0.0;
                var slope = 0.0;
                if (t < 1.0) {
                    prof = depth * (t * t - 1.0);
                    slope = depth * 2.0 * t;
                    floor_ = max(floor_, 1.0 - t * t);
                }
                let e = (t - 1.0) / 0.22;
                let bump = exp(-e * e);
                prof = prof + depth * 0.35 * bump;
                slope = slope + depth * 0.35 * bump * (-2.0 * e / 0.22);
                // Fresh craters throw pale ejecta round their rims.
                if (r0 < 0.12) {
                    rim = max(rim, bump * 0.8 + exp(-max(t - 1.0, 0.0) * 2.0) * 0.4 * step(1.0, t));
                }
                // d/dp = slope/rc * v/dist / cell
                h = h + vec4<f32>(prof, slope / (rc * cell) * v / dist);
            }
        }
        cell = cell * 0.36;
    }
    out.h = h;
    out.marks = vec2<f32>(floor_, rim);
    return out;
}

// How thick a belt's dust is, per metre of sight (mining.rs BELT_DUST).
const BELT_DUST: f32 = 5.0e-5;

// A point on a rock packed by render.rs pack_rock_point: in the mesh's own
// space, or a w of 0 if there's none.
fn rock_point(a: f32) -> vec4<f32> {
    if (a < 0.5) { return vec4<f32>(0.0); }
    let u = u32(round(a)) - 1u;
    let q = vec3<f32>(f32(u & 255u), f32((u >> 8u) & 255u), f32((u >> 16u) & 255u)) / 255.0;
    return vec4<f32>((q * 2.0 - 1.0) * 1.75, 1.0);
}

// Bare rock: dark, mottled stone, cratered and gritty, lit hard by the sun
// with only a faint fill, so a rock with the sun behind it is a dark shape
// with a lit edge, not a pale slab; read veins run through it as seams of
// glowing ore; and far off, the belt's dust veils it.
fn rock(i: VOut, n0: vec3<f32>, dox: vec3<f32>, doy: vec3<f32>, dwx: vec3<f32>, dwy: vec3<f32>) -> vec3<f32> {
    let s = i.params.z;
    let size = i.scale;
    // Object-aligned metres, and the pixel's footprint in metres.
    let pm = i.obj * size;
    let wm = max(max(length(dwx), length(dwy)), 1e-4);
    let rel = relief(pm, size, wm, s);
    let h = rel.h;
    // Bump: the height's gradient carried to the screen through the
    // position derivatives (surface gradient, Mikkelsen 2010).
    let dhx = dot(h.yzw, dox) * size;
    let dhy = dot(h.yzw, doy) * size;
    let r1 = cross(dwy, n0);
    let r2 = cross(n0, dwx);
    let det = dot(dwx, r1);
    var n = n0;
    if (abs(det) > 1e-12) {
        let grad = sign(det) * (dhx * r1 + dhy * r2);
        n = normalize(abs(det) * n0 - grad);
    }
    let mu = max(dot(n, i.view_dir), 0.0);

    // Albedo. Asteroids are dark: a fifth of the light or less comes
    // back. Each rock leans its own way between umber, slate and rust, so
    // a field isn't one grey; mottled in broad patches, some banded where
    // the parent body was layered, darker in hollows and crater floors,
    // pale round fresh craters, with flecks of mineral close up.
    let lean = fract(s * 0.6180339);
    let umber = vec3<f32>(1.08, 0.92, 0.76);
    let slate = vec3<f32>(0.88, 0.93, 1.02);
    let rust = vec3<f32>(1.14, 0.86, 0.68);
    let hue = select(mix(slate, rust, (lean - 0.5) * 2.0), mix(umber, slate, lean * 2.0), lean < 0.5);
    let po = i.obj * 1.6 + vec3<f32>(s % 13.0, s % 7.0, s % 5.0);
    let wo = max(length(dox), length(doy)) * 1.6;
    let mottle = fbm_aa(po, wo);
    var base = i.color.rgb * hue * 0.22 * (0.25 + 1.4 * mottle);
    let layered = step(0.55, fract(s * 0.3819));
    if (layered > 0.0) {
        let axis = normalize(vec3<f32>(fract(s * 0.13) - 0.5, 0.6, fract(s * 0.71) - 0.5));
        let band = sin(dot(i.obj, axis) * 22.0 + fbm_aa(po * 2.0, wo * 2.0) * 6.0);
        let fade = 1.0 - smoothstep(0.1, 0.25, wo * 8.0);
        base = base * (1.0 + 0.22 * band * fade);
    }
    // Patches of lighter and darker stone at every scale down to the
    // pixel, so a quarry close up is as varied as a boulder far off.
    var blotch = 0.0;
    var bl = size * 0.25;
    var bw = 0.5;
    for (var k = 0; k < 6; k = k + 1) {
        let keep = 1.0 - smoothstep(0.3, 0.7, wm / bl);
        if (keep <= 0.0) { break; }
        blotch = blotch + bw * keep * (noise(pm / bl + vec3<f32>(f32(k) * 5.1 + s % 9.0)) - 0.5);
        bl = bl / 2.3;
        bw = bw * 0.75;
    }
    base = base * (1.0 + 1.1 * blotch);
    let hollow = clamp(h.x / (size * 0.06), -1.0, 1.0);
    base = base * (0.8 + 0.35 * hollow) * (1.0 - 0.3 * rel.marks.x) + vec3<f32>(0.05, 0.048, 0.045) * rel.marks.y;
    let fp = pm / 0.35;
    let fleck = step(0.92, noise(fp)) * (1.0 - smoothstep(0.1, 0.3, wm / 0.35));
    base = mix(base, vec3<f32>(0.5, 0.48, 0.44), fleck * 0.4);

    let light_color = mix(vec3<f32>(1.0), g.star_color.rgb, 0.4);
    let ndl = dot(n, i.light.xyz);
    // Hollows sit in their own shadow a little.
    let lit = max(ndl, 0.0) * i.light.w * (0.75 + 0.25 * clamp(hollow + 0.5, 0.0, 1.0));
    // Fill: the sky's cool ambient, mostly greyed (bare rock shouldn't go
    // lavender in the shade), and a little light from behind the camera
    // (the ship's own floods, say) when the sun is behind the rock, so its
    // dark side still shows its facets and pits.
    let behind = clamp(-dot(i.view_dir, i.light.xyz), 0.0, 1.0);
    let flood = pow(mu, 1.5) * (0.06 + 0.2 * behind);
    let sky = ambient_light(n);
    let fill = mix(sky, vec3<f32>(dot(sky, vec3<f32>(0.33))), 0.85) * 2.0
        + vec3<f32>(0.75, 0.74, 0.76) * flood;
    // Sun behind the rock: its limb lights up, brightest on the side
    // facing the sun, so it stands off the black.
    let limb = pow(1.0 - mu, 2.5) * behind * smoothstep(-0.6, 0.2, ndl);
    let rim = pow(1.0 - mu, 4.0) * vec3<f32>(0.02, 0.03, 0.04) * behind;
    var color = base * (lit * light_color * 0.95 + fill) + limb * light_color * 0.2 + rim;

    // Heat from the laser's work glows out of the cracks first, then
    // spreads, dull red to orange; the rock stays rock.
    var heat = clamp(i.params.y * 4.0, 0.0, 1.0);
    // Round where the laser cuts, a few tens of metres across, if it is.
    let cut = rock_point(i.color.a);
    if (cut.w > 0.0) {
        let reach = max(0.7, 40.0 / size);
        heat = heat * (1.0 - smoothstep(0.3, 1.0, distance(i.obj, cut.xyz) / reach));
    }
    let crack = 1.0 - smoothstep(0.0, 0.03, abs(fbm_aa(po * 1.4 + vec3<f32>(9.0), wo * 1.4) - 0.5));
    let glow = crack * smoothstep(0.0, 0.6, heat) + smoothstep(0.6, 1.0, heat) * 0.07;
    let hot = mix(vec3<f32>(0.7, 0.12, 0.03), vec3<f32>(1.0, 0.45, 0.12), heat);
    color = color + hot * glow * 1.1;

    // Veins: seams of ore running through a ragged patch of the rock,
    // glowing out of it, not lamps stuck on it. The patch is as big as
    // the laser's mark for "on a vein" (mining.rs: a third of the radius,
    // at least 1.5 m).
    let kind = u32(round(i.params.w));
    if (kind > 0u) {
        let vr = max(0.35, 1.5 / size);
        var near = 0.0;
        for (var k = 0; k < 4; k = k + 1) {
            let v = rock_point(i.aux[k]);
            if (v.w == 0.0) { continue; }
            let d = distance(i.obj, v.xyz) / vr;
            let warp = (noise(i.obj / vr * 2.2 + vec3<f32>(f32(k) * 7.3 + s % 5.0)) - 0.5) * 0.7;
            near = max(near, 1.0 - smoothstep(0.45, 1.0, d + warp));
        }
        if (near > 0.0) {
            var ore = vec3<f32>(1.0, 0.55, 0.16);
            var strength = 1.0;
            if (kind == 2u) {
                ore = vec3<f32>(0.3, 0.7, 1.0);
                strength = 0.75;
            }
            if (kind == 3u) {
                ore = vec3<f32>(1.0, 0.24, 0.1);
                strength = 0.7 + 0.6 * (0.5 + 0.5 * sin(g.misc.x * 4.0));
            }
            // Seams: the ridges of two noises, a main lode and finer
            // stringers off it, stretched along one grain.
            let grain = normalize(vec3<f32>(fract(s * 0.29) - 0.5, fract(s * 0.53) - 0.5, 0.4));
            // Squeezed across the grain, so the seams run along it in
            // rough parallels rather than curling into rings.
            let q = i.obj / vr;
            let wobble = (noise(q * 1.3 + vec3<f32>(7.0)) - 0.5) * 0.8;
            let sp = (q + grain * (dot(q, grain) * 4.0 + wobble)) * 1.3 + vec3<f32>(s % 3.0);
            let px = max(length(dox), length(doy)) / vr * 1.3 * 5.0;
            let width = 0.045;
            let w1 = max(width, px * 0.8);
            let w2 = max(width * 0.6, px * 0.8);
            let seam1 = (1.0 - smoothstep(0.0, w1, abs(noise(sp) - 0.5))) * (width / w1);
            let seam2 = (1.0 - smoothstep(0.0, w2, abs(noise(sp * 2.6 + vec3<f32>(4.0)) - 0.5))) * (width * 0.6 / w2);
            let seam = max(seam1, seam2 * 0.7) * near;
            // The rock round a seam is stained with it.
            color = mix(color, color * (0.6 + ore * 0.8), near * 0.6);
            color = color + ore * strength * (seam * 1.8 + near * near * 0.12);
        }
    }

    // The belt's dust between the camera and the rock.
    let veil = exp(-length(i.wpos) * BELT_DUST);
    color = color * veil + dust_light(-i.view_dir, i.light.xyz) * (1.0 - veil);
    return color;
}

// Additive glows over the solid scene. light.w is how far through its
// life the glow is (0..1); params.y its strength.
@fragment
fn fs_glow(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = u32(round(i.params.x));
    // Spheres show their near side only; rings both.
    if (!front && mode != RING && mode != DUST) { discard; }
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
    if (mode == FLASH) {
        // A rock breaking: a hot white core that cools to the ore's colour
        // and thins to nothing at the edge, so it lights what's round it
        // without hiding it.
        let dust = smoothstep(0.3, 0.75, fbm(i.obj * 2.5 + vec3<f32>(i.params.z, 0.0, t * 0.5)));
        // The core is a spark, small and gone in a moment; the rest is
        // dust, ragged, in the ore's colour (#182: no glaring blobs).
        let core = pow(mu, 18.0) * (1.0 - smoothstep(0.0, 0.25, k));
        let puff = pow(mu, 1.2) * dust * 0.3;
        let hot = mix(vec3<f32>(1.0, 0.85, 0.65), i.color.rgb, smoothstep(0.0, 0.2, k));
        return vec4<f32>(hot * (core * 1.4 + puff) * fade * i.params.y, 1.0);
    }
    if (mode == DUST) {
        // The belt's dust along this line of sight: as much as lies
        // between the camera and the edge of the field (a flattened
        // ellipsoid), lit by the sun.
        let d = -i.view_dir;
        let r = i.aux.w;
        let squash = vec3<f32>(1.0, 1.0 / BELT_THICKNESS, 1.0) / r;
        let o = i.aux.xyz * squash;
        let dd = d * squash;
        let qa = dot(dd, dd);
        let qb = dot(o, dd);
        let qc = dot(o, o) - 1.0;
        let disc = qb * qb - qa * qc;
        if (disc <= 0.0) { discard; }
        let t0 = max((-qb - sqrt(disc)) / qa, 0.0);
        let t1 = max((-qb + sqrt(disc)) / qa, 0.0);
        let depth = 1.0 - exp(-(t1 - t0) * i.params.w);
        return vec4<f32>(dust_light(d, i.light.xyz) * depth * i.params.y, 1.0);
    }
    if (mode == CORONA) {
        // A star's corona, on a shell params.w times the star's radius:
        // light falling off from the limb, combed into faint streamers.
        let b = sqrt(max(1.0 - mu * mu, 0.0)) * i.params.w;
        if (b < 1.0) { discard; }
        let h = b - 1.0;
        let around = normalize(i.obj - i.view_dir * dot(i.obj, i.view_dir) + vec3<f32>(1e-5));
        let streamers = 0.65 + 0.35 * fbm(around * 2.5 + vec3<f32>(i.params.z * 0.01));
        let glow = (exp(-h * 9.0) * 0.8 + exp(-h * 2.2) * 0.2 * streamers) * streamers;
        let edge = 1.0 - smoothstep(0.7, 1.0, b / i.params.w);
        return vec4<f32>(i.color.rgb * glow * edge * i.params.y, 1.0);
    }
    // RING: lit at the rim.
    let rim = pow(1.0 - mu, 2.0);
    return vec4<f32>(i.color.rgb * rim * fade * i.params.y, 1.0);
}

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let mode = u32(round(i.params.x));
    let fw = fwidth(i.obj * max(i.params.w, 0.0001));
    // For bump-mapped rock: how object and world positions change across
    // the pixel (taken here, in uniform control flow).
    let dox = dpdx(i.obj);
    let doy = dpdy(i.obj);
    let dwx = dpdx(i.wpos);
    let dwy = dpdy(i.wpos);
    // The pilot's own hull (seed < 0) is cut away round the target behind
    // it, like a cutaway on a technical drawing, so what you're shooting
    // at is never hidden (#123). The cut's edge is inked in amber, the
    // target's colour, so the hole reads as deliberate.
    if (i.params.z < 0.0 && g.reveal.w > 0.0) {
        let d = distance(i.clip.xy, g.reveal.xy);
        let r = g.reveal.z;
        if (d < r) { discard; }
        let ink = max(1.5, g.misc.w / 600.0);
        if (d < r + ink) {
            return vec4<f32>(vec3<f32>(1.0, 0.55, 0.12) * 0.9 * g.reveal.w, 1.0);
        }
    }
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
    if (mode == ROCK) {
        return vec4<f32>(rock(i, n, dox, doy, dwx, dwy), 1.0);
    }

    // Per-body offset into noise space (seed is an integer < 10000).
    let s = i.params.z;
    let p = i.obj + vec3<f32>(s % 97.0, (s / 97.0) % 89.0, s * 0.37 % 83.0);

    if (mode == STAR) {
        // Limb darkening, redder toward the edge; granulation and a few
        // faculae, faded out where the disc is too small to show them.
        let limb = 0.35 + 0.65 * pow(mu, 0.6);
        let fine = 1.0 - smoothstep(0.05, 0.3, max(fw.x, max(fw.y, fw.z)) * 12.0);
        let cells = fbm(p * 18.0 + vec3<f32>(g.misc.x * 0.03));
        let granules = 1.0 + (cells - 0.5) * 0.35 * fine;
        let spots = smoothstep(0.7, 0.78, fbm(p * 3.0 + vec3<f32>(17.0))) * fine;
        let warm = mix(vec3<f32>(1.0, 0.45, 0.2), vec3<f32>(1.0), smoothstep(0.0, 0.8, mu));
        let disc = i.color.rgb * warm * limb * granules * (1.0 - 0.55 * spots);
        return vec4<f32>(disc * i.params.y, 1.0);
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
