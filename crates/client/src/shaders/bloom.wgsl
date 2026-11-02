// Bloom at quarter resolution: a soft bright-pass downsample, then
// separable gaussian blurs (two widths, for a tight glow and a wide one).

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time, y: exposure, z: width, w: height
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var src: texture_2d<f32>;
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

@fragment
fn fs_down(i: VOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    var c = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 2) {
        for (var x = -1; x <= 1; x = x + 2) {
            c = c + min(textureSample(src, samp, i.uv + vec2<f32>(f32(x), f32(y)) * t).rgb, vec3<f32>(40.0));
        }
    }
    c = max(c * 0.25 * g.misc.y, vec3<f32>(0.0));
    // Soft threshold: only what's brighter than the hull paint blooms.
    let l = max(c.r, max(c.g, c.b));
    let knee = smoothstep(0.9, 2.5, l);
    return vec4<f32>(c * knee, 1.0);
}

fn blur(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    let t = dir / vec2<f32>(textureDimensions(src));
    var c = textureSample(src, samp, uv).rgb * 0.2270270270;
    c = c + textureSample(src, samp, uv + t * 1.3846153846).rgb * 0.3162162162;
    c = c + textureSample(src, samp, uv - t * 1.3846153846).rgb * 0.3162162162;
    c = c + textureSample(src, samp, uv + t * 3.2307692308).rgb * 0.0702702703;
    c = c + textureSample(src, samp, uv - t * 3.2307692308).rgb * 0.0702702703;
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_blur_h1(i: VOut) -> @location(0) vec4<f32> { return blur(i.uv, vec2<f32>(1.0, 0.0)); }
@fragment
fn fs_blur_v1(i: VOut) -> @location(0) vec4<f32> { return blur(i.uv, vec2<f32>(0.0, 1.0)); }
// The wide glow: a dense gaussian (sigma 4 texels), so a single bright
// pixel, like a star, spreads into a round halo rather than the grid of
// dots a sparse wide kernel leaves.
fn blur_wide(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    let t = dir / vec2<f32>(textureDimensions(src));
    var c = vec3<f32>(0.0);
    var total = 0.0;
    for (var k = -12; k <= 12; k = k + 1) {
        let x = f32(k);
        let w = exp(-x * x / 32.0);
        c = c + textureSample(src, samp, uv + t * x).rgb * w;
        total = total + w;
    }
    return vec4<f32>(c / total, 1.0);
}

@fragment
fn fs_blur_h2(i: VOut) -> @location(0) vec4<f32> { return blur_wide(i.uv, vec2<f32>(1.0, 0.0)); }
@fragment
fn fs_blur_v2(i: VOut) -> @location(0) vec4<f32> { return blur_wide(i.uv, vec2<f32>(0.0, 1.0)); }
