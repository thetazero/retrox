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

/// Rock labels are drawn this close, nearest first, up to [`MAX_LABELS`].
const LABEL_RANGE: f64 = 1500.0;
const MAX_LABELS: usize = 40;

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

    // Read rocks, labelled with their grade.
    if let Some((field, rocks)) = &w.field_rocks {
        let centre = w.position(Place::Field(*field));
        let f = &w.galaxy.fields[*field];
        let mut near: Vec<(f64, LiveRock)> = w
            .mining
            .live_rocks(*field, f, rocks, w.clock_s)
            .filter(|r| r.scanned)
            .map(|r| ((centre + r.pos - ship).length() - r.radius, r))
            .filter(|(d, _)| *d < LABEL_RANGE)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, rock) in near.into_iter().take(MAX_LABELS) {
            let Some(p) = to_screen(centre + rock.pos) else {
                continue;
            };
            let color = if rock.lode.volatile {
                ALARM
            } else if rock.worked_out {
                CREAM_DIM
            } else {
                good_color(rock.lode.good)
            };
            let text = if rock.worked_out {
                "×".to_owned()
            } else if rock.lode.volatile {
                format!("{}!", rock.lode.grade_letter())
            } else {
                rock.lode.grade_letter().to_string()
            };
            painter.text(p, Align2::CENTER_CENTER, text, mono(12.0), color);
        }

        // Loose ore: small diamonds, so nothing is lost in the dark.
        if let Some(state) = w.mining.field(*field) {
            for n in &state.nuggets {
                let pos = centre + n.pos;
                if (pos - ship).length() > 1500.0 {
                    continue;
                }
                if let Some(p) = to_screen(pos) {
                    let r = 3.0;
                    painter.add(egui::Shape::closed_line(
                        vec![
                            p + Vec2::new(0.0, -r),
                            p + Vec2::new(r, 0.0),
                            p + Vec2::new(0.0, r),
                            p + Vec2::new(-r, 0.0),
                        ],
                        Stroke::new(1.0, good_color(n.good)),
                    ));
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
            let size = 26.0;
            brackets(
                &painter,
                Rect::from_center_size(p, Vec2::splat(size * 2.0)),
                7.0,
                CREAM,
            );
            let mut y = p.y - size;
            let x = p.x + size + 10.0;
            let line = |y: f32, text: String, color: Color32| {
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, text, mono(12.0), color);
            };
            let color = if rock.scanned {
                good_color(rock.lode.good)
            } else {
                CREAM
            };
            line(y, describe(&rock), color);
            y += 15.0;
            if rock.scanned {
                if rock.worked_out {
                    line(y, "WORKED OUT".into(), CREAM_DIM);
                } else {
                    line(
                        y,
                        format!(
                            "≈ {:.1} T PER FRACTURE · VEINS ×{:.1}",
                            rock.yield_t(),
                            mining::VEIN_YIELD
                        ),
                        CREAM_DIM,
                    );
                }
                y += 15.0;
                if rock.lode.volatile {
                    line(y, "VOLATILE · KEEP CLEAR".into(), ALARM);
                    y += 15.0;
                }
            } else {
                line(y, "G TO PROSPECT".into(), CREAM_DIM);
                y += 15.0;
            }
            if aim.distance > stats.mining_range_m {
                line(
                    y,
                    format!("OUT OF LASER RANGE · {}", distance(aim.distance)),
                    CREAM_DIM,
                );
            } else if rock.damage > 0.0 {
                // Work done toward the fracture.
                let rect = Rect::from_min_size(Pos2::new(x, y + 2.0), Vec2::new(120.0, 5.0));
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
