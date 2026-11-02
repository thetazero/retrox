//! The combat HUD: hostile markers, the target's box and lead pip, the
//! guns' boresight, hits taken and dealt, and the ship's shield, hull and
//! heat gauges in the flight panel.

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec3;

use super::{
    ALARM, AMBER, CREAM_DIM, HudInput, INK, VFD, VFD_DIM, brackets, distance, mono, project,
};
use crate::combat::Tactic;
use crate::world::{Mode, World};

/// A hostile's marker: a red diamond.
fn diamond(painter: &egui::Painter, p: Pos2, r: f32, color: Color32) {
    painter.add(egui::Shape::closed_line(
        vec![
            p + Vec2::new(0.0, -r),
            p + Vec2::new(r, 0.0),
            p + Vec2::new(0.0, r),
            p + Vec2::new(-r, 0.0),
        ],
        Stroke::new(1.5, color),
    ));
}

/// A thin two-tone bar: `value` of it lit.
fn bar(painter: &egui::Painter, rect: Rect, value: f32, color: Color32) {
    painter.rect_filled(rect, 0.0, VFD_DIM);
    painter.rect_filled(
        Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * value.clamp(0.0, 1.0), rect.height()),
        ),
        0.0,
        color,
    );
}

fn hull_color(left: f64) -> Color32 {
    if left < 0.3 { ALARM } else { AMBER }
}

/// A glow along the screen's edges, strongest at the edge: hits taken.
fn edge_glow(painter: &egui::Painter, screen: Rect, color: Color32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    let depth = screen.height() * 0.07;
    let outer = color.gamma_multiply(strength.min(1.0));
    let inner = Color32::TRANSPARENT;
    let o = [
        screen.left_top(),
        screen.right_top(),
        screen.right_bottom(),
        screen.left_bottom(),
    ];
    let i = [
        screen.left_top() + Vec2::new(depth, depth),
        screen.right_top() + Vec2::new(-depth, depth),
        screen.right_bottom() + Vec2::new(-depth, -depth),
        screen.left_bottom() + Vec2::new(depth, -depth),
    ];
    let mut mesh = egui::Mesh::default();
    for k in 0..4 {
        let n = (k + 1) % 4;
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(o[k], outer);
        mesh.colored_vertex(o[n], outer);
        mesh.colored_vertex(i[n], inner);
        mesh.colored_vertex(i[k], inner);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Markers and effects over the view while flying.
pub fn draw(ctx: &egui::Context, h: &HudInput) {
    let w = h.world;
    if w.mode != Mode::Normal {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let c = &w.combat;

    if c.lost.is_some() {
        // The ship is gone: the card over the wreck is drawn with the
        // restore (see `crate::lost`).
        return;
    }

    let origin = w.combat_origin();
    let me = w.player_craft();
    let to_screen = |p: DVec3| project(h.view_proj, (origin + p - h.camera).as_vec3(), screen);

    // Every raider: a red diamond; the target in brackets, with what it is,
    // how far, its shield and hull, and where to aim.
    for r in &c.raiders {
        let targeted = c.target == Some(r.id);
        let rel = (origin + r.craft.pos - h.camera).as_vec3();
        let Some(p) = to_screen(r.craft.pos) else {
            if targeted {
                super::edge_arrow_color(&painter, h.view_proj, rel, screen, ALARM);
            }
            continue;
        };
        let dist = (r.craft.pos - me.pos).length();
        if !targeted {
            diamond(&painter, p, 5.0, ALARM);
            if dist < 8000.0 {
                painter.text(
                    p + Vec2::new(10.0, 0.0),
                    Align2::LEFT_CENTER,
                    distance(dist),
                    mono(11.0),
                    ALARM.gamma_multiply(0.8),
                );
            }
            continue;
        }
        let size = 30.0;
        let rect = Rect::from_center_size(p, Vec2::splat(size));
        brackets(&painter, rect, 7.0, ALARM);
        let x = rect.right() + 10.0;
        let mut y = rect.top() - 4.0;
        let status = match r.tactic {
            Tactic::Flee => " · RUNNING",
            Tactic::Return => " · BREAKING OFF",
            _ => "",
        };
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            format!(
                "RAIDER · {}{status}",
                r.vessel.fit.hull().name.to_uppercase()
            ),
            mono(12.0),
            ALARM,
        );
        y += 15.0;
        let closing = -(r.craft.vel - me.vel).dot((r.craft.pos - me.pos).normalize_or(DVec3::Z));
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            format!("{}  {:+.0} M/S", distance(dist), closing),
            mono(12.0),
            CREAM_DIM,
        );
        y += 17.0;
        let v = &r.vessel;
        if v.shield_max() > 0.0 {
            bar(
                &painter,
                Rect::from_min_size(Pos2::new(x, y), Vec2::new(90.0, 4.0)),
                (v.shield / v.shield_max()) as f32,
                VFD,
            );
            y += 7.0;
        }
        bar(
            &painter,
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(90.0, 4.0)),
            v.hull_left() as f32,
            hull_color(v.hull_left()),
        );

        // The lead pip: put the boresight on it.
        if let Some(lead) = c.lead(&me, r.id)
            && let Some(q) = to_screen(lead)
            && (q - p).length() > 2.0
        {
            painter.line_segment([p, q], Stroke::new(1.0, ALARM.gamma_multiply(0.35)));
            painter.circle_stroke(q, 6.0, Stroke::new(1.5, ALARM));
            painter.circle_filled(q, 1.5, ALARM);
        }
    }

    // The guns' boresight: where they point, at the target's range.
    if !c.player.guns.is_empty() {
        let range = c.target.and_then(|t| c.raider(t)).map_or(1500.0, |r| {
            (r.craft.pos - me.pos).length().clamp(200.0, 6000.0)
        });
        if let Some(p) = to_screen(me.pos + me.forward() * range) {
            let color = if c.player.locked {
                ALARM
            } else {
                Color32::from_rgb(255, 222, 160)
            };
            painter.circle_stroke(p, 9.0, Stroke::new(1.0, color));
            for d in [Vec2::X, -Vec2::X, Vec2::Y] {
                painter.line_segment([p + d * 9.0, p + d * 15.0], Stroke::new(1.0, color));
            }
            // A hit: a cross flashes on the boresight.
            if c.since_scored < 0.18 {
                let s = 11.0;
                let k = 1.0 - (c.since_scored / 0.18) as f32;
                let hit = Color32::from_rgb(255, 240, 210).gamma_multiply(k);
                for (a, b) in [
                    (Vec2::new(-s, -s), Vec2::new(-5.0, -5.0)),
                    (Vec2::new(s, -s), Vec2::new(5.0, -5.0)),
                    (Vec2::new(-s, s), Vec2::new(-5.0, 5.0)),
                    (Vec2::new(s, s), Vec2::new(5.0, 5.0)),
                ] {
                    painter.line_segment([p + a, p + b], Stroke::new(2.0, hit));
                }
            }
        }
    }

    // Hits taken: teal flicker on the shield, red on the hull.
    let shield = (1.0 - c.since_shield_hit / 0.3).max(0.0) as f32;
    edge_glow(&painter, screen, VFD.gamma_multiply(0.15), shield);
    let hull = (1.0 - c.since_hull_hit / 0.5).max(0.0) as f32;
    edge_glow(&painter, screen, ALARM.gamma_multiply(0.4), hull);
}

/// The ship's defences in the flight panel: shield, hull and heat, and
/// lamps when the guns are starved of power or venting. Nothing for a ship
/// with no guns or shields that's never been hit.
pub fn gauges(ui: &mut egui::Ui, w: &World, width: f32) {
    let v = &w.combat.player;
    if v.guns.is_empty() && v.shield_max() <= 0.0 && v.damage.total() <= 0.0 {
        return;
    }
    ui.add_space(4.0);
    let col = (width - 24.0) / 3.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::hover());
    let painter = ui.painter();
    let gauge = |i: usize, label: &str, value: f64, text: String, color: Color32| {
        let x = rect.left() + i as f32 * (col + 12.0);
        painter.text(
            Pos2::new(x, rect.top()),
            Align2::LEFT_TOP,
            label,
            mono(11.0),
            CREAM_DIM,
        );
        painter.text(
            Pos2::new(x + col, rect.top()),
            Align2::RIGHT_TOP,
            text,
            mono(11.0),
            color,
        );
        let bar_rect = Rect::from_min_size(Pos2::new(x, rect.top() + 16.0), Vec2::new(col, 8.0));
        // Segmented, like the throttle.
        let n = 16;
        let gap = 2.0;
        let seg = (col - gap * (n - 1) as f32) / n as f32;
        let lit = (value.clamp(0.0, 1.0) * n as f64).round() as usize;
        for s in 0..n {
            let r = Rect::from_min_size(
                Pos2::new(bar_rect.left() + s as f32 * (seg + gap), bar_rect.top()),
                Vec2::new(seg, bar_rect.height()),
            );
            painter.rect_filled(r, 0.0, if s < lit { color } else { VFD_DIM });
        }
    };
    let shield_max = v.shield_max();
    if shield_max > 0.0 {
        gauge(
            0,
            "SHIELD",
            v.shield / shield_max,
            format!("{:.0}", v.shield),
            VFD,
        );
    } else {
        gauge(0, "SHIELD", 0.0, "NONE".into(), CREAM_DIM);
    }
    let left = v.hull_left();
    gauge(
        1,
        "HULL",
        left,
        format!("{:.0}%", left * 100.0),
        hull_color(left),
    );
    let heat = v.heat_share();
    gauge(
        2,
        "HEAT",
        heat,
        format!("{:.0}%", heat * 100.0),
        if v.locked || heat > 0.8 { ALARM } else { AMBER },
    );
    // What's wrong, as lamps under the gauges.
    let wrecked: Vec<&str> = (0..v.parts.len())
        .filter(|&i| v.wrecked(i))
        .map(|i| v.parts[i].name)
        .collect();
    let starved = v.firing && v.gun_power < 0.98;
    if v.locked || starved || !wrecked.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if v.locked {
                super::lamp(ui, "GUNS VENTING", true, ALARM);
            }
            if starved {
                super::lamp(ui, "LOW POWER", true, AMBER);
            }
            for name in wrecked {
                ui.label(
                    RichText::new(format!("{} WRECKED", name.to_uppercase()))
                        .color(ALARM)
                        .background_color(INK),
                );
            }
        });
    }
}
