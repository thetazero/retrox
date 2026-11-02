// Tonemap the HDR scene and grade it like period film stock with a
// sovietwave lean: bloom with red halation, warm highlights,
// a touch of lens fringing, vignette, grain and faint scanlines.

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time, y: exposure, z: width, w: height
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var hdr: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var bloom: texture_2d<f32>;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return o;
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // Lens fringing: red and blue pulled apart toward the edges.
    let off = (i.uv - 0.5) * 0.0018;
    var c = vec3<f32>(
        textureSample(hdr, samp, i.uv + off).r,
        textureSample(hdr, samp, i.uv).g,
        textureSample(hdr, samp, i.uv - off).b,
    ) * g.misc.y;
    // Bloom, with film halation: an extra red-orange fringe on bright things.
    let b = textureSample(bloom, samp, i.uv).rgb;
    c = c + b * 0.35 + vec3<f32>(dot(b, vec3<f32>(0.33))) * vec3<f32>(0.22, 0.05, 0.03);

    c = aces(c);
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    // A little extra saturation in the mids.
    c = max(mix(vec3<f32>(lum), c, 1.12), vec3<f32>(0.0));
    // Warm peach highlights. (No lift in the shadows: empty space stays
    // black, and the only light in the sky is real starlight.)
    c = c * mix(vec3<f32>(0.97, 0.98, 1.03), vec3<f32>(1.05, 0.99, 0.9), smoothstep(0.15, 0.85, lum));
    // Vignette.
    let d = distance(i.uv, vec2<f32>(0.5));
    c = c * (1.0 - smoothstep(0.4, 0.95, d) * 0.6);
    // Grain (strongest in the mids, like film) and faint scanlines.
    let px = i.uv * g.misc.zw;
    let grain = hash(px + fract(g.misc.x) * 113.0) - 0.5;
    c = c + grain * (0.008 + 0.025 * lum * (1.0 - lum));
    c = c * (0.985 + 0.015 * sin(px.y * 3.14159));
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
