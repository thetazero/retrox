//! Render a galaxy as an SVG, top-down.

use std::fmt::Write;

use retrox_sim::{GalaxyState, StarClass};

pub fn svg(state: &GalaxyState) -> String {
    let systems = state.systems.as_slice();
    let extent = systems
        .iter()
        .map(|s| s.position.x.abs().max(s.position.y.abs()))
        .fold(1.0, f64::max)
        * 1.05;
    let size = 2000.0;
    let scale = size / (2.0 * extent);
    let px = |x: f64| (x + extent) * scale;

    let mut out = String::new();
    let _ = writeln!(
        out,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">
<rect width="100%" height="100%" fill="#0b0d12"/>
<g stroke="#c8553d" stroke-opacity="0.45" stroke-width="1.2">"##
    );
    for lane in state.lanes.as_slice() {
        let (a, b) = (
            state.systems[lane.a].position,
            state.systems[lane.b].position,
        );
        let _ = writeln!(
            out,
            r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"#,
            px(a.x),
            px(a.y),
            px(b.x),
            px(b.y)
        );
    }
    out.push_str("</g>\n<g>\n");
    for s in systems {
        let (color, r) = match s.star {
            StarClass::O | StarClass::B => ("#9fc4ff", 3.2),
            StarClass::A | StarClass::F => ("#f1efe6", 2.6),
            StarClass::G => ("#f4d37a", 2.4),
            StarClass::K => ("#f0a35e", 2.2),
            StarClass::M => ("#e0715a", 1.8),
            StarClass::WhiteDwarf => ("#ffffff", 1.6),
            StarClass::Neutron => ("#7df0e8", 2.0),
            StarClass::BlackHole => ("#b07cff", 3.0),
        };
        let _ = writeln!(
            out,
            r#"<circle cx="{:.1}" cy="{:.1}" r="{r}" fill="{color}"/>"#,
            px(s.position.x),
            px(s.position.y)
        );
    }
    out.push_str("</g>\n</svg>\n");
    out
}
