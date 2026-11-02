// Background stars: the actual other systems of the galaxy, at infinity,
// each carrying the light it really sends us (computed on the CPU).
//
// A point of light reaches the sensor spread by the optics, so each star is
// drawn as a small quad holding its point-spread function: a sharp core plus
// a wider halo, together summing to the star's flux. Peaks are kept below
// the bloom threshold, so a star's glow is all here and fades smoothly at
// the frame's edge instead of vanishing with its centre pixel.

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    star_color: vec4<f32>,
    // x: time, y: exposure, z: width, w: height
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

const TAU: f32 = 6.2831853;
// Peak brightness (per pixel) of the core and of the halo.
const CORE_PEAK: f32 = 0.8;
const HALO_PEAK: f32 = 0.25;
const CORE_SIGMA: f32 = 0.7;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    // Offset from the star's centre, in pixels.
    @location(0) px: vec2<f32>,
    @location(1) tint: vec3<f32>,
    // x: core flux, y: halo flux, z: halo sigma (pixels)
    @location(2) psf: vec3<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) v: u32,
    @location(0) dir: vec3<f32>,
    @location(1) color: vec3<f32>,
) -> VOut {
    let flux = max(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)), 1e-8);
    // The core takes as much as fits under its peak; the rest is halo,
    // widened until it too stays under its peak.
    let core = min(flux, CORE_PEAK * TAU * CORE_SIGMA * CORE_SIGMA);
    let halo = flux - core;
    let sigma = sqrt(max(halo / (TAU * HALO_PEAK), 4.0));
    let radius = select(3.0 * CORE_SIGMA, 3.0 * sigma, halo > 0.0);

    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let px = corners[v] * radius;

    var o: VOut;
    // w = 0: a direction, at infinite distance (depth 0 in reversed Z).
    let c = g.view_proj * vec4<f32>(dir, 0.0);
    // Offset in pixels, scaled by w so it survives the perspective divide.
    o.clip = vec4<f32>(c.xy + px * 2.0 / g.misc.zw * c.w, c.z, c.w);
    o.px = px;
    o.tint = color / flux;
    o.psf = vec3<f32>(core, halo, sigma);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let r2 = dot(i.px, i.px);
    let cs2 = CORE_SIGMA * CORE_SIGMA;
    let hs2 = i.psf.z * i.psf.z;
    let e = i.psf.x * exp(-r2 / (2.0 * cs2)) / (TAU * cs2)
        + i.psf.y * exp(-r2 / (2.0 * hs2)) / (TAU * hs2);
    return vec4<f32>(i.tint * e, 1.0);
}
