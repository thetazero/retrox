//! The mining HUD: where the laser bites, what the rocks are, how hot the
//! laser is and what the hold is worth.

use egui::{Align, Align2, Color32, Layout, Pos2, Rect, RichText, Stroke, Vec2};
use retrox_sim::economy::Good;

use super::{
    ALARM, AMBER, CREAM, CREAM_DIM, HudInput, Panel, VFD, brackets, credits, distance, jumps_text,
    keycap, lamp, mono, panel, project, segment_bar, tag,
};
use crate::mining::{self, LiveRock};
use crate::world::{Mode, Place, World};

/// Rocks are tagged this close, nearest first, up to [`MAX_LABELS`].
const LABEL_RANGE: f64 = 1500.0;
const MAX_LABELS: usize = 14;

fn good_color(good: Good) -> Color32 {
    match good {
        Good::Ice => Color32::from_rgb(120, 210, 255),
        _ => AMBER,
    }
}

/// One line about a rock: what it is, if it's been read.
fn describe(rock: &LiveRock) -> String {
    if !rock.scanned {
        return format!("UNREAD ROCK · {:.0} M", rock.radius);
    }
    let kind = if rock.radius > mining::QUARRY_RADIUS {
        " · QUARRY"
    } else {
        ""
    };
    format!(
        "{} · GRADE {} · {:.0} M{kind}",
        rock.lode.good.name().to_uppercase(),
        rock.lode.grade_letter(),
        rock.radius
    )
}

/// Where `tonnes` of `good` sell best nearby (issue #122): up to three
/// markets in this system or a jump out, each with what the lot fetches
/// there, best first.
pub(super) fn buyers(ui: &mut egui::Ui, w: &World, good: Good, tonnes: f64) {
    for b in w.buyers(good, tonnes, 3) {
        ui.horizontal(|ui| {
            let name = &w.galaxy.settlements[b.at].name;
            let name = match name.char_indices().nth(18) {
                Some((i, _)) => format!("{}…", &name[..i]),
                None => name.clone(),
            };
            ui.label(RichText::new(name).size(12.0).color(CREAM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(credits(b.per_t * tonnes))
                        .font(mono(13.0))
                        .color(VFD),
                );
                ui.label(
                    RichText::new(format!("{} · {:.1}/t", jumps_text(b.jumps), b.per_t))
                        .size(11.0)
                        .color(CREAM_DIM),
                );
            });
        });
    }
}

/// How big a ball of `radius` at `pos` looks on screen, in pixels, given
/// where its centre lands (`p`).
fn on_screen_radius(h: &HudInput, pos: glam::DVec3, radius: f64, p: Pos2, screen: Rect) -> f32 {
    let rel = (pos - h.camera).as_vec3();
    let across = rel
        .cross(glam::Vec3::Y)
        .try_normalize()
        .unwrap_or(glam::Vec3::X);
    super::project_any(h.view_proj, rel + across * radius as f32, screen)
        .map_or(0.0, |e| (e - p).length())
}

/// Draws over the view while flying: markers in the world and the mining
/// panel. Only when there's a laser fitted or a field to mine.
pub fn draw(ctx: &egui::Context, h: &HudInput) {
    let w = h.world;
    let stats = w.stats();
    if w.mode != Mode::Normal || (w.field_rocks.is_none() && stats.mining_cut <= 0.0) {
        return;
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());
    let ship = w.ship_position();
    let to_screen = |p: glam::DVec3| project(h.view_proj, (p - h.camera).as_vec3(), screen);

    // Read rocks get a tag beside them (#156): what's in it, bright when
    // the laser reaches it, dim when it's a flight away.
    if let Some((field, rocks)) = &w.field_rocks {
        let centre = w.position(Place::Field(*field));
        let f = &w.galaxy.fields[*field];
        let aimed = w.mining.aim.map(|a| a.rock.id);
        let mut near: Vec<(f64, LiveRock)> = w
            .mining
            .live_rocks(*field, f, rocks, w.clock_s)
            .filter(|r| r.scanned && !r.worked_out && Some(r.id) != aimed)
            .map(|r| ((centre + r.pos - ship).length() - r.radius, r))
            .filter(|(d, _)| *d < LABEL_RANGE)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut taken: Vec<Rect> = Vec::new();
        for (gap, rock) in near.into_iter().take(MAX_LABELS) {
            let pos = centre + rock.pos;
            let Some(p) = to_screen(pos) else {
                continue;
            };
            let in_reach = gap <= stats.mining_range_m;
            let color = if rock.lode.volatile {
                ALARM
            } else {
                good_color(rock.lode.good)
            };
            let color = if in_reach {
                color
            } else {
                color.gamma_multiply(0.55)
            };
            let mut text = format!(
                "{} {}",
                rock.lode.good.name().to_uppercase(),
                rock.lode.grade_letter()
            );
            if rock.lode.volatile {
                text.push_str(" · VOLATILE");
            }
            // From the rock's upper right edge, as it looks on screen.
            let r = on_screen_radius(h, pos, rock.radius, p, screen).clamp(3.0, 120.0);
            let edge = p + Vec2::new(0.7, -0.7) * r;
            let elbow = edge + Vec2::new(7.0, -7.0);
            let galley = painter.layout_no_wrap(text, mono(11.0), color);
            let at = elbow + Vec2::new(6.0, -galley.size().y / 2.0);
            let rect = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(4.0, 1.0));
            if !screen.contains_rect(rect) || taken.iter().any(|t| t.intersects(rect)) {
                continue;
            }
            taken.push(rect);
            let ink = Stroke::new(1.0, color);
            painter.line_segment([edge, elbow], ink);
            painter.line_segment([elbow, elbow + Vec2::new(4.0, 0.0)], ink);
            crate::widgets::plated_galley(&painter, at, galley);
        }

        // Loose ore: a filled diamond while the tractor has it, hollow
        // when it's out of reach, dim when the hold is full. The nearest
        // piece out of reach says how far, so none is lost in the dark.
        if let Some(state) = w.mining.field(*field) {
            let full = w.player.cargo_t() >= stats.cargo_t - 0.01;
            let mut nearest: Option<(f64, Pos2, Good)> = None;
            for n in &state.nuggets {
                let pos = centre + n.pos;
                let d = (pos - ship).length();
                if d > LABEL_RANGE {
                    continue;
                }
                let Some(p) = to_screen(pos) else {
                    continue;
                };
                let pulled = d < mining::TRACTOR_RANGE && !full;
                let color = if full { CREAM_DIM } else { good_color(n.good) };
                let r = 3.5;
                let points = vec![
                    p + Vec2::new(0.0, -r),
                    p + Vec2::new(r, 0.0),
                    p + Vec2::new(0.0, r),
                    p + Vec2::new(-r, 0.0),
                ];
                if pulled {
                    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
                } else {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(1.0, color)));
                }
                if !pulled && nearest.is_none_or(|(best, ..)| d < best) {
                    nearest = Some((d, p, n.good));
                }
            }
            if let Some((d, p, good)) = nearest {
                let (text, color) = if full {
                    ("HOLD FULL".to_owned(), CREAM_DIM)
                } else {
                    (
                        format!("LOOSE {} · {}", good.name().to_uppercase(), distance(d)),
                        good_color(good),
                    )
                };
                let galley = painter.layout_no_wrap(text, mono(11.0), color);
                let at = p + Vec2::new(10.0, -galley.size().y / 2.0);
                let rect = Rect::from_min_size(at, galley.size()).expand2(Vec2::new(4.0, 1.0));
                if !taken.iter().any(|t| t.intersects(rect)) {
                    crate::widgets::plated_galley(&painter, at, galley);
                }
            }
        }
    }

    // The laser's reticle, where it bites (the chase camera sits above the
    // ship, so this isn't the screen centre).
    if stats.mining_cut > 0.0 {
        let forward = w.ship.rot * glam::DVec3::NEG_Z;
        let muzzle = ship + w.ship.rot * w.laser_muzzle();
        let reach = match w.mining.aim {
            Some(a) if a.distance <= stats.mining_range_m => a.distance,
            _ => stats.mining_range_m,
        };
        if let Some(p) = to_screen(muzzle + forward * reach) {
            let in_reach = w
                .mining
                .aim
                .is_some_and(|a| a.distance <= stats.mining_range_m);
            let color = match w.mining.aim {
                _ if w.mining.locked => ALARM,
                Some(a) if in_reach && a.on_vein => Color32::from_rgb(255, 222, 120),
                _ if in_reach => AMBER,
                _ => CREAM_DIM,
            };
            let s = 7.0;
            for (a, b) in [
                (Vec2::new(-s, -s), Vec2::new(-s * 0.4, -s * 0.4)),
                (Vec2::new(s, -s), Vec2::new(s * 0.4, -s * 0.4)),
                (Vec2::new(-s, s), Vec2::new(-s * 0.4, s * 0.4)),
                (Vec2::new(s, s), Vec2::new(s * 0.4, s * 0.4)),
            ] {
                painter.line_segment([p + a, p + b], Stroke::new(1.5, color));
            }
            if w.mining.aim.is_some_and(|a| a.on_vein) {
                painter.circle_stroke(p, 11.0, Stroke::new(1.0, color));
            }
        }
    }

    // A prospector ping: a ring sweeping out from the ship.
    if w.mining.since_scan < 1.2 {
        let k = (w.mining.since_scan / 1.2) as f32;
        if let Some(p) = to_screen(ship) {
            painter.circle_stroke(
                p,
                20.0 + k * screen.width() * 0.6,
                Stroke::new(2.0 * (1.0 - k) + 0.5, VFD.gamma_multiply(1.0 - k)),
            );
        }
    }

    // The rock under the reticle (not mid-fight: the target's box is
    // what matters then).
    if let Some(aim) = w.mining.aim.filter(|_| w.hostiles_near() == 0) {
        let rock = aim.rock;
        let pos = match &w.field_rocks {
            Some((field, _)) => w.position(Place::Field(*field)) + rock.pos,
            None => ship,
        };
        if let Some(p) = to_screen(pos) {
            // The brackets close round the rock as it looks on screen, and
            // the readout stands beside them, not over the rock (issue
            // #124): it's read while aiming at the rock.
            let on_screen = on_screen_radius(h, pos, rock.radius, p, screen);
            let size = (on_screen + 8.0).clamp(26.0, screen.height() * 0.3);
            brackets(
                &painter,
                Rect::from_center_size(p, Vec2::splat(size * 2.0)),
                7.0,
                CREAM,
            );
            let mut lines: Vec<(String, Color32)> = Vec::new();
            let color = if rock.scanned {
                good_color(rock.lode.good)
            } else {
                CREAM
            };
            lines.push((describe(&rock), color));
            if rock.scanned {
                if rock.worked_out {
                    lines.push(("WORKED OUT".into(), CREAM_DIM));
                } else {
                    lines.push((
                        format!(
                            "ABOUT {:.1} T PER FRACTURE · VEINS ×{:.1}",
                            rock.yield_t(),
                            mining::VEIN_YIELD
                        ),
                        CREAM,
                    ));
                }
                if rock.lode.volatile {
                    lines.push(("VOLATILE · KEEP CLEAR".into(), ALARM));
                }
            } else {
                lines.push(("G TO PROSPECT".into(), CREAM));
            }
            let out_of_range = aim.distance > stats.mining_range_m;
            if out_of_range {
                lines.push((
                    format!("OUT OF LASER RANGE · {}", distance(aim.distance)),
                    CREAM_DIM,
                ));
            }
            let bar = !out_of_range && rock.damage > 0.0;
            let galleys: Vec<_> = lines
                .into_iter()
                .map(|(text, color)| painter.layout_no_wrap(text, mono(12.0), color))
                .collect();
            let line_h = 15.0;
            let pad = Vec2::new(8.0, 5.0);
            let inner = Vec2::new(
                galleys.iter().map(|g| g.size().x).fold(120.0, f32::max),
                galleys.len() as f32 * line_h + if bar { 9.0 } else { 0.0 },
            );
            let plate_size = inner + 2.0 * pad;
            // Right of the rock, or left of it if that runs off the screen.
            let right = p.x + size + 12.0;
            let x = if right + plate_size.x <= screen.right() - 16.0 {
                right
            } else {
                p.x - size - 12.0 - plate_size.x
            };
            let y = (p.y - plate_size.y / 2.0)
                .clamp(screen.top() + 16.0, screen.bottom() - 16.0 - plate_size.y);
            let plate = Rect::from_min_size(Pos2::new(x, y), plate_size);
            painter.rect_filled(plate, 0.0, crate::widgets::BACKING);
            painter.rect_filled(
                Rect::from_min_size(plate.min, Vec2::new(2.0, plate.height())),
                0.0,
                color,
            );
            let mut at = plate.min + pad;
            for g in galleys {
                painter.galley(at, g, CREAM);
                at.y += line_h;
            }
            if bar {
                // Work done toward the fracture.
                let rect = Rect::from_min_size(at + Vec2::new(0.0, 3.0), Vec2::new(120.0, 5.0));
                painter.rect_filled(rect, 0.0, super::VFD_DIM);
                painter.rect_filled(
                    Rect::from_min_size(
                        rect.min,
                        Vec2::new(120.0 * rock.damage.min(1.0) as f32, 5.0),
                    ),
                    0.0,
                    AMBER,
                );
            }
        }
    }

    // The panel.
    panel(
        ctx,
        Panel {
            id: "mining",
            title: "Mining",
            anchor: Align2::LEFT_CENTER,
            offset: [16.0, 40.0],
            width: 300.0,
            collapsible: Some(true),
        },
        |ui| {
            if stats.mining_cut <= 0.0 {
                ui.label(RichText::new("no mining laser fitted").color(AMBER));
                ui.label(RichText::new("fit one to a weapon slot in a hangar").color(CREAM_DIM));
                return;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("LASER").color(CREAM_DIM));
                lamp(ui, "FIRING", w.mining.firing, AMBER);
                lamp(ui, "VENTING", w.mining.locked, ALARM);
                lamp(
                    ui,
                    "VEIN",
                    w.mining.firing && w.mining.aim.is_some_and(|a| a.on_vein),
                    Color32::from_rgb(255, 222, 120),
                );
            });
            let heat = w.mining.heat as f32;
            let color = if w.mining.locked || heat > 0.8 {
                ALARM
            } else {
                VFD
            };
            segment_bar(ui, heat, 276.0, color);
            ui.add_space(4.0);

            let cargo = w.player.cargo_t();
            ui.horizontal(|ui| {
                ui.label(RichText::new("HOLD").color(CREAM_DIM));
                ui.label(
                    RichText::new(format!("{cargo:.1} / {:.0} t", stats.cargo_t)).color(
                        if cargo >= stats.cargo_t - 0.01 {
                            ALARM
                        } else {
                            VFD
                        },
                    ),
                );
            });
            segment_bar(ui, (cargo / stats.cargo_t.max(1e-9)) as f32, 276.0, AMBER);
            ui.add_space(4.0);

            // This run's haul, and where the hold's load of it sells.
            for good in [Good::Ore, Good::Ice] {
                let t = w.mining.haul[good];
                if t < 0.05 {
                    continue;
                }
                ui.horizontal(|ui| {
                    tag(ui, good.name(), good_color(good));
                    ui.label(RichText::new(format!("{t:.1} t mined")).color(CREAM));
                });
                buyers(ui, w, good, w.player.cargo[good].max(t));
                ui.add_space(2.0);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                keycap(ui, "G");
                ui.label(RichText::new("prospect").color(CREAM));
                keycap(ui, "LMB");
                keycap(ui, "R");
                ui.label(RichText::new("laser").color(CREAM));
            });
            if w.field_rocks.is_none() {
                ui.label(RichText::new("fly into an asteroid field to mine").color(CREAM_DIM));
            }
        },
    );
}
