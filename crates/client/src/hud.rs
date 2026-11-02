//! The heads-up display and station/landing screens (egui for now; a
//! custom diegetic UI comes with the art pass).

use egui::{Align2, Color32, FontId, Pos2, RichText, Stroke, Vec2};
use glam::{DVec3, Mat4, Vec4};
use retrox_sim::economy::Good;
use retrox_sim::{BodyKind, EconomyModel, SettlementId};

use crate::world::{AU, Action, COMBAT_SPEED, Mode, Place, SECONDS_PER_DAY, World};

pub const AMBER: Color32 = Color32::from_rgb(255, 176, 72);
const AMBER_DIM: Color32 = Color32::from_rgb(170, 110, 45);
const CYAN: Color32 = Color32::from_rgb(120, 220, 210);
const PANEL: Color32 = Color32::from_rgba_premultiplied(8, 7, 5, 200);

pub fn style(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    for font in style.text_styles.values_mut() {
        font.family = egui::FontFamily::Monospace;
    }
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(AMBER);
    v.window_fill = PANEL;
    v.panel_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, AMBER_DIM);
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 28, 12);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 28, 12);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(80, 52, 18);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(120, 76, 24);
    v.selection.bg_fill = Color32::from_rgb(110, 70, 20);
    v.window_corner_radius = 0.into();
    ctx.set_global_style(style);
}

pub struct HudInput<'a> {
    pub world: &'a World,
    pub view_proj: Mat4,
    pub camera: DVec3,
    pub stick: glam::DVec2,
    pub captured: bool,
}

pub fn draw(ctx: &egui::Context, h: HudInput) -> Vec<Action> {
    let mut actions = Vec::new();
    let w = h.world;
    let screen = ctx.content_rect();

    // Markers for every place; labels placed nearest-first (target always
    // first) and skipped where they'd overlap one already drawn.
    if matches!(w.mode, Mode::Normal | Mode::FastTravel(_)) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let mut labels: Vec<(bool, f64, Pos2, String)> = Vec::new();
        for place in w.nav_places() {
            let pos = w.position(place);
            let rel = (pos - h.camera).as_vec3();
            let targeted = w.target == Some(place);
            let dist = w.distance_to(place);
            let Some(p) = project(h.view_proj, rel, screen) else {
                if targeted {
                    edge_arrow(&painter, h.view_proj, rel, screen);
                }
                continue;
            };
            let color = if targeted { CYAN } else { AMBER_DIM };
            let r = if targeted { 9.0 } else { 4.0 };
            painter.add(egui::Shape::closed_line(
                vec![
                    p + Vec2::new(0.0, -r),
                    p + Vec2::new(r, 0.0),
                    p + Vec2::new(0.0, r),
                    p + Vec2::new(-r, 0.0),
                ],
                Stroke::new(1.2, color),
            ));
            labels.push((
                targeted,
                dist,
                p,
                format!("{}  {}", w.name(place), distance(dist)),
            ));
        }
        labels.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        let mut taken: Vec<egui::Rect> = Vec::new();
        for (targeted, _, p, text) in labels {
            let (font, color) = if targeted {
                (FontId::monospace(13.0), CYAN)
            } else {
                (FontId::monospace(11.0), AMBER_DIM)
            };
            let galley = painter.layout_no_wrap(text, font, color);
            let rect = egui::Rect::from_min_size(
                p + Vec2::new(12.0, -galley.size().y / 2.0),
                galley.size(),
            )
            .expand(2.0);
            if taken.iter().any(|t| t.intersects(rect)) {
                continue;
            }
            taken.push(rect);
            painter.galley(rect.min + Vec2::splat(2.0), galley, color);
        }

        // Crosshair and stick position.
        let c = screen.center();
        painter.circle_stroke(c, 14.0, Stroke::new(1.0, AMBER_DIM));
        let s = Vec2::new(h.stick.x as f32, h.stick.y as f32) * 60.0;
        painter.circle_filled(c + s, 2.5, AMBER);
    }

    // Top-left: where and when.
    egui::Area::new(egui::Id::unique("where"))
        .anchor(Align2::LEFT_TOP, [16.0, 12.0])
        .show(ctx, |ui| {
            let system = &w.galaxy.systems[w.system];
            ui.label(RichText::new(w.date().to_string()).size(20.0));
            ui.label(RichText::new(&system.name).size(16.0));
            let owner = match w.galaxy.largest_faction_in(w.system) {
                Some(f) => {
                    let faction = &w.galaxy.factions[f];
                    let model = match faction.economy {
                        EconomyModel::Planned => "planned",
                        EconomyModel::Market => "market",
                    };
                    format!("{} · {model}", faction.name)
                }
                None => "uninhabited".into(),
            };
            ui.label(RichText::new(owner).color(AMBER_DIM));
        });

    // Bottom: flight readout.
    if w.mode == Mode::Normal {
        egui::Area::new(egui::Id::unique("flight"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -18.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_width(380.0);
                        let speed = w.ship.vel.length();
                        let mode = if speed > COMBAT_SPEED * 1.05 {
                            "CRUISE"
                        } else {
                            "NORMAL"
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(mode).strong());
                            ui.label(RichText::new(speed_text(speed)).size(18.0));
                            if !w.ship.flight_assist {
                                ui.label(
                                    RichText::new("FA OFF").color(Color32::from_rgb(255, 90, 60)),
                                );
                            }
                        });
                        ui.add(
                            egui::ProgressBar::new(w.ship.throttle.max(0.0) as f32)
                                .desired_height(8.0)
                                .fill(if w.ship.throttle < 0.0 { CYAN } else { AMBER }),
                        );
                        if let Some(well) = w.current_well() {
                            ui.label(
                                RichText::new(format!("GRAVITY WELL · {}", w.name(well)))
                                    .color(AMBER_DIM),
                            );
                        }
                        if let Some(t) = w.target {
                            ui.label(format!(
                                "TARGET {}  {}",
                                w.name(t),
                                distance(w.distance_to(t))
                            ));
                        }
                    });
            });
    }

    // Prompts.
    let prompts = w.prompts();
    let mut lines: Vec<String> = Vec::new();
    if w.mode == Mode::Normal {
        match (prompts.fast_travel, prompts.well, w.target) {
            (Some(t), _, _) => lines.push(format!("[J] fast travel to {}", w.name(t))),
            (None, Some(well), Some(_)) => lines.push(format!(
                "fast travel and jumps: leave the gravity well of {} first",
                w.name(well)
            )),
            _ => {}
        }
        if let Some(s) = prompts.dock {
            lines.push(format!("[F] dock at {}", w.name(Place::Station(s))));
        }
        if let Some((city, body)) = prompts.land {
            lines.push(format!(
                "[L] land at {}, {}",
                w.galaxy.settlements[city].name,
                w.name(Place::Body(body))
            ));
        }
    }
    if !lines.is_empty() {
        egui::Area::new(egui::Id::unique("prompts"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -120.0])
            .show(ctx, |ui| {
                for l in lines {
                    ui.label(RichText::new(l).color(CYAN));
                }
            });
    }

    if let Some((text, _)) = &w.message {
        egui::Area::new(egui::Id::unique("message"))
            .anchor(Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(text).size(18.0));
            });
    }

    // Navigation list.
    if w.mode == Mode::Normal {
        egui::Window::new("HYPERLANES")
            .anchor(Align2::RIGHT_BOTTOM, [-12.0, -12.0])
            .default_width(400.0)
            .default_open(false)
            .resizable(false)
            .show(ctx, |ui| {
                let open_space = prompts.well.is_none();
                if !open_space {
                    ui.label(RichText::new("leave the gravity well to jump").color(AMBER_DIM));
                }
                egui::Grid::new("lanes")
                    .num_columns(3)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for lane in w.lanes() {
                            ui.label(&lane.name);
                            ui.label(
                                RichText::new(format!(
                                    "{:.1} ly · {:.0} d",
                                    lane.length_ly, lane.days
                                ))
                                .color(AMBER_DIM),
                            );
                            if ui
                                .add_enabled(open_space, egui::Button::new("Jump"))
                                .clicked()
                            {
                                actions.push(Action::Jump(lane.to));
                            }
                            ui.end_row();
                        }
                    });
            });
        egui::Window::new("NAVIGATION")
            .anchor(Align2::RIGHT_TOP, [-12.0, 12.0])
            .default_width(400.0)
            .resizable(false)
            .collapsible(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(screen.height() * 0.6)
                    .show(ui, |ui| {
                        egui::Grid::new("nav")
                            .num_columns(2)
                            .spacing([12.0, 2.0])
                            .show(ui, |ui| {
                                for place in w.nav_places() {
                                    let indent = match place {
                                        Place::Body(b)
                                            if w.galaxy.bodies[b].kind == BodyKind::Moon =>
                                        {
                                            "  "
                                        }
                                        Place::Station(_) => "◇ ",
                                        Place::Field(_) => "∴ ",
                                        _ => "",
                                    };
                                    let label = format!("{indent}{}", truncate(&w.name(place), 30));
                                    if ui
                                        .selectable_label(w.target == Some(place), label)
                                        .clicked()
                                    {
                                        actions.push(Action::Target(place));
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(distance(w.distance_to(place)))
                                                    .color(AMBER_DIM),
                                            );
                                        },
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
    }

    match w.mode {
        Mode::Docked(s) => settlement_screen(ctx, w, s, "STATION", Action::Undock, &mut actions),
        Mode::Landed { city, timer, .. } => {
            if timer > 0.0 {
                // Descent: fade to black and back.
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("fade"),
                ));
                let fade = (timer / 3.0).clamp(0.0, 1.0);
                let alpha = (255.0 * (1.0 - (fade - 0.5).abs() * 2.0).clamp(0.0, 1.0)) as u8;
                painter.rect_filled(screen, 0.0, Color32::from_black_alpha(alpha.max(120)));
                painter.text(
                    screen.center(),
                    Align2::CENTER_CENTER,
                    "DESCENT",
                    FontId::monospace(28.0),
                    AMBER,
                );
            } else {
                settlement_screen(ctx, w, city, "SURFACE", Action::Launch, &mut actions);
            }
        }
        Mode::Jumping { to, timer } => {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("jump"),
            ));
            let t = 1.0 - timer / 4.0;
            let alpha = (255.0 * (t * 1.4).min(1.0)) as u8;
            painter.rect_filled(
                screen,
                0.0,
                Color32::from_rgba_unmultiplied(220, 235, 255, alpha),
            );
            painter.text(
                screen.center(),
                Align2::CENTER_CENTER,
                format!("HYPERLANE — {}", w.galaxy.systems[to].name),
                FontId::monospace(26.0),
                Color32::from_rgb(30, 30, 40),
            );
        }
        Mode::FastTravel(trip) => {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("fast"),
            ));
            // Streaks rushing out from the centre.
            let c = screen.center();
            let phase = trip.elapsed * 3.0;
            for i in 0..64 {
                let a = i as f32 * 2.399;
                let r = ((i as f32 * 0.37 + phase).fract()) * screen.width() * 0.6;
                let dir = Vec2::new(a.cos(), a.sin());
                painter.line_segment(
                    [c + dir * r, c + dir * (r + 40.0 + r * 0.2)],
                    Stroke::new(1.0, AMBER_DIM),
                );
            }
            let days = (w.clock_s - trip.depart_s) / SECONDS_PER_DAY;
            let km_s = w.ship.vel.length() / 1000.0;
            let au = w.distance_to(trip.target) / AU;
            painter.text(
                c + Vec2::new(0.0, -screen.height() * 0.3),
                Align2::CENTER_CENTER,
                format!(
                    "FAST TRAVEL — {} · {au:.2} AU · {km_s:.0} km/s · {days:.1} d · J drop out",
                    w.name(trip.target)
                ),
                FontId::monospace(22.0),
                AMBER,
            );
        }
        _ => {}
    }

    // Controls help.
    egui::Area::new(egui::Id::unique("help"))
        .anchor(Align2::LEFT_BOTTOM, [16.0, -12.0])
        .show(ctx, |ui| {
            let text = if h.captured {
                "mouse steer · W/S throttle · X stop · A/D strafe · SPACE/CTRL lift · Q/E roll · SHIFT boost\n\
                 J fast travel · T next target · V camera · Z flight assist · TAB free mouse"
            } else {
                "TAB or click the view to fly with the mouse · arrow keys also steer"
            };
            ui.label(RichText::new(text).size(11.0).color(AMBER_DIM));
        });

    actions
}

/// A settlement's screen: its owner, population and market, plus waiting
/// and leaving. Used both docked at a station and landed at a city.
fn settlement_screen(
    ctx: &egui::Context,
    w: &World,
    id: SettlementId,
    title: &str,
    leave: Action,
    actions: &mut Vec<Action>,
) {
    let Some(settlement) = w.galaxy.settlements.get(id) else {
        return;
    };
    let market = &settlement.economy.market;
    egui::Window::new(title)
        .anchor(Align2::LEFT_CENTER, [24.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.heading(&settlement.name);
            let faction = &w.galaxy.factions[settlement.owner];
            let model = match faction.economy {
                EconomyModel::Planned => "planned",
                EconomyModel::Market => "market",
            };
            ui.label(RichText::new(format!("{} · {model}", faction.name)).color(AMBER_DIM));
            ui.label(
                RichText::new(format!("population {:.1}k", settlement.economy.population))
                    .color(AMBER_DIM),
            );
            ui.separator();
            ui.label("MARKET");
            egui::Grid::new("market").striped(true).show(ui, |ui| {
                for head in ["good", "stock", "price", "status"] {
                    ui.label(RichText::new(head).color(AMBER_DIM));
                }
                ui.end_row();
                for good in Good::ALL {
                    ui.label(good.name());
                    ui.label(format!("{:.0}", market.stock[good]));
                    ui.label(format!("{:.2} cr", market.price[good]));
                    let fill = market.fill(good);
                    let status = if market.shortfall[good] > 0.01 {
                        RichText::new("SHORTAGE").color(Color32::from_rgb(255, 90, 60))
                    } else if fill > 2.5 {
                        RichText::new("surplus").color(CYAN)
                    } else {
                        RichText::new("")
                    };
                    ui.label(status);
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let label = if leave == Action::Launch {
                    "Launch"
                } else {
                    "Undock"
                };
                if ui.button(label).clicked() {
                    actions.push(leave);
                }
                if ui.button("Wait 1 day").clicked() {
                    actions.push(Action::Wait(1));
                }
                if ui.button("Wait 7 days").clicked() {
                    actions.push(Action::Wait(7));
                }
                if ui.button("Wait 30 days").clicked() {
                    actions.push(Action::Wait(30));
                }
            });
        });
}

fn project(view_proj: Mat4, rel: glam::Vec3, screen: egui::Rect) -> Option<Pos2> {
    let clip = view_proj * Vec4::new(rel.x, rel.y, rel.z, 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.x.abs() > 1.0 || ndc.y.abs() > 1.0 {
        return None;
    }
    Some(Pos2::new(
        screen.left() + (ndc.x + 1.0) * 0.5 * screen.width(),
        screen.top() + (1.0 - ndc.y) * 0.5 * screen.height(),
    ))
}

/// An arrow at the screen edge pointing toward an off-screen target.
fn edge_arrow(painter: &egui::Painter, view_proj: Mat4, rel: glam::Vec3, screen: egui::Rect) {
    let clip = view_proj * Vec4::new(rel.x, rel.y, rel.z, 1.0);
    let mut dir = Vec2::new(clip.x, -clip.y);
    if clip.w < 0.0 {
        dir = -dir;
    }
    let dir = dir.normalized();
    let c = screen.center();
    let r = screen.height().min(screen.width()) * 0.42;
    let tip = c + dir * r;
    let side = Vec2::new(-dir.y, dir.x) * 7.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, tip - dir * 14.0 + side, tip - dir * 14.0 - side],
        CYAN,
        Stroke::NONE,
    ));
}

pub fn distance(m: f64) -> String {
    if m < 1e3 {
        format!("{m:.0} m")
    } else if m < 1e6 {
        format!("{:.1} km", m / 1e3)
    } else if m < 0.01 * AU {
        format!("{:.0} Mm", m / 1e6)
    } else {
        format!("{:.2} AU", m / AU)
    }
}

fn speed_text(v: f64) -> String {
    const C: f64 = 299_792_458.0;
    if v < 1e3 {
        format!("{v:.0} m/s")
    } else if v < 0.1 * C {
        format!("{:.1} km/s", v / 1e3)
    } else {
        format!("{:.2} c", v / C)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        s.chars().take(n - 1).chain(['…']).collect()
    }
}
