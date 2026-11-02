// Background stars: the actual other systems of the galaxy, at infinity,
// each carrying the light it really sends us (computed on the CPU).
//
// A point of light reaches the sensor spread by the optics, so each star is
// drawn as a small quad holding its point-spread function: a sharp core plus
// a tight halo. Most stars fit under those peaks and stay below the bloom
// threshold, so their glow is all here and fades smoothly at the frame's
// edge instead of vanishing with the centre pixel.
//
// A near neighbour sends far more light than that (an O star ten light-years
// off, thousands of times more). The halo never widens past a few pixels:
// a wide halo reads as a smudge on the lens, not a star. Like film, the
// light past that point is compressed (logarithmically), and what's left
// goes to the core, which then burns over the bloom threshold and glows in
// the bloom pass, and to four thin diffraction spikes that grow longer, not
// brighter. A bright star is a hard white point with a glare and rays.

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
const HALO_MIN_SIGMA: f32 = 2.0;
// The widest the halo gets, in pixels (it is drawn out to 3 sigma).
const HALO_MAX_SIGMA: f32 = 2.5;
// Spikes: a gaussian across (this sigma, in pixels), fading along with this
// many pixels of length per e-fold of extra light; they reach 3 lengths.
const SPIKE_SIGMA: f32 = 0.6;
const SPIKE_GROWTH: f32 = 3.5;
// Half the extra light goes to the spikes, half to the core.
const SPIKE_SHARE: f32 = 0.5;
// The spikes lie on the diagonals, apart from the HUD's square-on ticks.
const SPIKE_ANGLE: f32 = 0.7853982;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    // Offset from the star's centre, in pixels.
    @location(0) px: vec2<f32>,
    @location(1) tint: vec3<f32>,
    // x: core flux, y: halo flux, z: halo sigma (pixels)
    @location(2) psf: vec3<f32>,
    // x: spike peak (per pixel), y: spike length (pixels), z: quad radius
    @location(3) spike: vec3<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) v: u32,
    @location(0) dir: vec3<f32>,
    @location(1) color: vec3<f32>,
) -> VOut {
    let raw = max(dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)), 1e-8);
    // What the core and the widest halo hold under their peaks.
    let core_cap = CORE_PEAK * TAU * CORE_SIGMA * CORE_SIGMA;
    let knee = core_cap + HALO_PEAK * TAU * HALO_MAX_SIGMA * HALO_MAX_SIGMA;
    // Past the knee the light is compressed, like film's shoulder.
    let stops = log(max(raw / knee, 1.0));
    let flux = min(raw, knee * (1.0 + stops));
    // The core takes as much as fits under its peak; the rest is halo,
    // widened until it too stays under its peak, up to the cap.
    var core = min(flux, core_cap);
    let halo = min(flux - core, knee - core_cap);
    let sigma = clamp(sqrt(halo / (TAU * HALO_PEAK)), HALO_MIN_SIGMA, HALO_MAX_SIGMA);
    let extra = flux - core - halo;
    core = core + extra * (1.0 - SPIKE_SHARE);
    // Four spikes, each an exponential fade of 1/e length `len` along and a
    // gaussian across: together they hold `extra * SPIKE_SHARE`.
    let len = SPIKE_GROWTH * stops;
    let spike_peak = select(
        0.0,
        extra * SPIKE_SHARE / (4.0 * len * sqrt(TAU) * SPIKE_SIGMA),
        len > 0.01,
    );
    let radius = max(select(3.0 * CORE_SIGMA, 3.0 * sigma, halo > 0.0), 3.0 * len);

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
    o.tint = color / raw;
    o.psf = vec3<f32>(core, halo, sigma);
    o.spike = vec3<f32>(spike_peak, len, radius);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let r2 = dot(i.px, i.px);
    let cs2 = CORE_SIGMA * CORE_SIGMA;
    let hs2 = i.psf.z * i.psf.z;
    var e = i.psf.x * exp(-r2 / (2.0 * cs2)) / (TAU * cs2)
        + i.psf.y * exp(-r2 / (2.0 * hs2)) / (TAU * hs2);
    if i.spike.x > 0.0 {
        let cs = cos(SPIKE_ANGLE);
        let sn = sin(SPIKE_ANGLE);
        // Along and across each spike: |x| and |y| in the spikes' frame.
        let q = abs(vec2<f32>(cs * i.px.x + sn * i.px.y, -sn * i.px.x + cs * i.px.y));
        let ss2 = 2.0 * SPIKE_SIGMA * SPIKE_SIGMA;
        let rays = exp(-q.x / i.spike.y - q.y * q.y / ss2)
            + exp(-q.y / i.spike.y - q.x * q.x / ss2);
        // Taper to nothing at the quad's edge, so no ray ends in a cut.
        let edge = 1.0 - smoothstep(0.6, 1.0, max(q.x, q.y) / i.spike.z);
        e = e + i.spike.x * rays * edge;
    }
    return vec4<f32>(i.tint * e, 1.0);
}
