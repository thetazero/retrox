//! Render a galaxy as an SVG, top-down.

use std::fmt::Write;

use std::collections::HashMap;

use retrox_sim::{FactionId, GalaxyState};

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

    // Spread faction hues around the colour wheel by the golden angle.
    let colors: HashMap<FactionId, String> = state
        .factions
        .keys()
        .enumerate()
        .map(|(i, id)| (id, hsl((i as f64 * 137.508) % 360.0, 0.65, 0.6)))
        .collect();
    for s in systems {
        let (color, r) = match s.owner {
            Some(owner) => (colors[&owner].as_str(), 2.6),
            None => ("#5c606b", 1.8),
        };
        let _ = writeln!(
            out,
            r#"<circle cx="{:.1}" cy="{:.1}" r="{r}" fill="{color}"/>"#,
            px(s.position.x),
            px(s.position.y)
        );
    }
    out.push_str("</g>\n<g font-family=\"monospace\" font-size=\"18\" fill=\"#f1efe6\" text-anchor=\"middle\">\n");
    for faction in state.factions.values() {
        let p = state.systems[faction.capital].position;
        let (x, y) = (px(p.x), px(p.y));
        let _ = writeln!(
            out,
            r##"<circle cx="{x:.1}" cy="{y:.1}" r="6" fill="none" stroke="#f1efe6" stroke-width="2"/><text x="{x:.1}" y="{:.1}">{}</text>"##,
            y - 12.0,
            faction.name
        );
    }
    out.push_str("</g>\n</svg>\n");
    out
}

fn hsl(h: f64, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f64| ((v + m) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}
