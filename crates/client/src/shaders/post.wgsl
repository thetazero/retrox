// Tonemap the HDR scene and give it a filmic, slightly worn look:
// exposure, vignette, film grain, faint scanlines.

struct Globals {
    view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time, y: exposure, z: width, w: height
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var hdr: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

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
    var c = textureSample(hdr, samp, i.uv).rgb * g.misc.y;
    // Slight warm film-stock grade: lift shadows toward teal, warm highlights.
    c = aces(c);
    c = mix(c, c * vec3<f32>(1.04, 1.0, 0.92), smoothstep(vec3<f32>(0.3), vec3<f32>(1.0), c));
    c = c + vec3<f32>(0.004, 0.008, 0.01) * (1.0 - c);
    // Vignette.
    let d = distance(i.uv, vec2<f32>(0.5));
    c = c * (1.0 - smoothstep(0.45, 0.95, d) * 0.55);
    // Grain and faint scanlines.
    let px = i.uv * g.misc.zw;
    let grain = hash(px + fract(g.misc.x) * 113.0) - 0.5;
    c = c + grain * 0.012;
    c = c * (0.993 + 0.007 * sin(px.y * 3.14159));
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
