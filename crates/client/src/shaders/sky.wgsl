// Background stars: the actual neighbouring systems of the galaxy, drawn
// as points at infinity.

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
    @location(0) color: vec3<f32>,
};

@vertex
fn vs(@location(0) dir: vec3<f32>, @location(1) color: vec3<f32>) -> VOut {
    var o: VOut;
    // w = 0: a direction, at infinite distance (depth 0 in reversed Z).
    o.clip = g.view_proj * vec4<f32>(dir, 0.0);
    o.color = color;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
