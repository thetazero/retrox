//! Render a galaxy as an SVG, top-down.

use std::fmt::Write;

use retrox_sim::GalaxyState;

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

    for (id, s) in state.systems.iter() {
        // Nobody owns a system; colour it by its largest faction.
        let (color, r) = match state.largest_faction_in(id) {
            Some(owner) => {
                let [r, g, b] = state.factions[owner].color;
                (format!("#{r:02x}{g:02x}{b:02x}"), 2.6)
            }
            None => ("#5c606b".to_owned(), 1.8),
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
        // Label the powers, not every independent outpost.
        let capital = &state.settlements[faction.capital];
        if crate::holdings(state, capital.owner) < 2 {
            continue;
        }
        let p = state.systems[capital.system].position;
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
