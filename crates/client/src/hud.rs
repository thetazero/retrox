//! The heads-up display and station/landing screens, drawn with egui in a
//! constructivist / instrument-panel style: red title bands set in condensed
//! display type, cream rules, and glowing teal VFD readouts.

use std::sync::Arc;

use egui::{
    Align2, Color32, FontFamily, FontId, Margin, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};
use glam::{DVec3, Mat4, Vec4};
use retrox_sim::economy::Good;
use retrox_sim::{BodyKind, EconomyModel, SettlementId};

use crate::world::{AU, Action, COMBAT_SPEED, Mode, Place, SECONDS_PER_DAY, World};

/// Enamel cream: body text and rules.
const CREAM: Color32 = Color32::from_rgb(234, 222, 196);
const CREAM_DIM: Color32 = Color32::from_rgb(160, 146, 122);
const RULE: Color32 = Color32::from_rgb(110, 98, 84);
/// Signal red: title bands and primary buttons.
const RED: Color32 = Color32::from_rgb(196, 44, 34);
const RED_DARK: Color32 = Color32::from_rgb(118, 26, 22);
const RED_HOT: Color32 = Color32::from_rgb(232, 70, 52);
/// Warnings and alarms.
const ALARM: Color32 = Color32::from_rgb(255, 88, 64);
/// Amber: targets and selection.
const AMBER: Color32 = Color32::from_rgb(255, 178, 76);
/// Vacuum-fluorescent teal: live instrument readouts.
const VFD: Color32 = Color32::from_rgb(110, 240, 214);
const VFD_DIM: Color32 = Color32::from_rgb(22, 58, 56);
const PANEL: Color32 = Color32::from_rgba_premultiplied(12, 10, 18, 222);
const INK: Color32 = Color32::from_rgb(20, 16, 24);

const DISPLAY: &str = "display";

fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "share-tech-mono".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/ShareTechMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "bebas-neue".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/BebasNeue-Regular.ttf"
        ))),
    );
    // Keep egui's own fonts behind ours as fallbacks for symbols.
    let defaults = fonts.families[&FontFamily::Monospace].clone();
    for family in [FontFamily::Monospace, FontFamily::Proportional] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "share-tech-mono".into());
    }
    let mut display_list = vec!["bebas-neue".to_owned(), "share-tech-mono".to_owned()];
    display_list.extend(defaults);
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), display_list);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    for (text_style, font) in style.text_styles.iter_mut() {
        font.family = FontFamily::Monospace;
        font.size = match text_style {
            egui::TextStyle::Heading => 24.0,
            egui::TextStyle::Small => 11.0,
            _ => 14.0,
        };
    }
    style.spacing.button_padding = Vec2::new(10.0, 4.0);
    style.spacing.item_spacing = Vec2::new(8.0, 4.0);
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(CREAM);
    v.window_fill = PANEL;
    v.panel_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, RULE);
    v.window_corner_radius = 0.into();
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.faint_bg_color = Color32::from_rgba_premultiplied(10, 9, 12, 10);
    v.extreme_bg_color = INK;
    v.selection.bg_fill = RED_DARK;
    v.selection.stroke = Stroke::new(1.0, CREAM);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, RULE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, CREAM);
    w.inactive.weak_bg_fill = Color32::from_rgb(30, 24, 34);
    w.inactive.bg_fill = Color32::from_rgb(30, 24, 34);
    w.inactive.bg_stroke = Stroke::new(1.0, RULE);
    w.inactive.fg_stroke = Stroke::new(1.0, CREAM);
    w.hovered.weak_bg_fill = RED_DARK;
    w.hovered.bg_fill = RED_DARK;
    w.hovered.bg_stroke = Stroke::new(1.0, CREAM);
    w.hovered.fg_stroke = Stroke::new(1.0, CREAM);
    w.active.weak_bg_fill = RED;
    w.active.bg_fill = RED;
    w.active.bg_stroke = Stroke::new(1.0, CREAM);
    w.active.fg_stroke = Stroke::new(1.0, CREAM);
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = 0.into();
        state.expansion = 0.0;
    }
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
            if targeted {
                brackets(
                    &painter,
                    Rect::from_center_size(p, Vec2::splat(22.0)),
                    6.0,
                    AMBER,
                );
                painter.circle_filled(p, 1.5, AMBER);
            } else {
                let r = 3.5;
                painter.add(egui::Shape::closed_line(
                    vec![
                        p + Vec2::new(0.0, -r),
                        p + Vec2::new(r, 0.0),
                        p + Vec2::new(0.0, r),
                        p + Vec2::new(-r, 0.0),
                    ],
                    Stroke::new(1.0, CREAM_DIM),
                ));
            }
            labels.push((
                targeted,
                dist,
                p,
                format!("{}  {}", w.name(place).to_uppercase(), distance(dist)),
            ));
        }
        labels.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        let mut taken: Vec<Rect> = Vec::new();
        for (targeted, _, p, text) in labels {
            let (font, color) = if targeted {
                (mono(13.0), AMBER)
            } else {
                (mono(11.0), CREAM_DIM)
            };
            let galley = painter.layout_no_wrap(text, font, color);
            let rect =
                Rect::from_min_size(p + Vec2::new(16.0, -galley.size().y / 2.0), galley.size())
                    .expand(2.0);
            if taken.iter().any(|t| t.intersects(rect)) {
                continue;
            }
            taken.push(rect);
            if targeted {
                // A leader line out to the label.
                painter.line_segment(
                    [p + Vec2::new(11.0, 0.0), rect.left_center()],
                    Stroke::new(1.0, AMBER.gamma_multiply(0.6)),
                );
            }
            painter.galley(rect.min + Vec2::splat(2.0), galley, color);
        }

        // Crosshair and stick position.
        let c = screen.center();
        let ring = CREAM_DIM.gamma_multiply(0.7);
        painter.circle_stroke(c, 14.0, Stroke::new(1.0, ring));
        for dir in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
            painter.line_segment([c + dir * 18.0, c + dir * 26.0], Stroke::new(1.0, ring));
        }
        let s = Vec2::new(h.stick.x as f32, h.stick.y as f32) * 60.0;
        painter.circle_filled(c + s, 2.5, AMBER);
    }

    // Top-left: where and when.
    egui::Area::new(egui::Id::unique("where"))
        .anchor(Align2::LEFT_TOP, [18.0, 14.0])
        .show(ctx, |ui| {
            // A faint backing keeps it legible over bright planets.
            let frame = egui::Frame::new()
                .fill(Color32::from_rgba_premultiplied(9, 7, 14, 205))
                .inner_margin(Margin {
                    left: 18,
                    right: 14,
                    top: 4,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let system = &w.galaxy.systems[w.system];
                    ui.label(
                        RichText::new(system.name.to_uppercase())
                            .font(display(40.0))
                            .color(CREAM),
                    );
                    let owner = match w.galaxy.largest_faction_in(w.system) {
                        Some(f) => {
                            let faction = &w.galaxy.factions[f];
                            format!(
                                "{} · {} economy",
                                faction.name,
                                economy_name(faction.economy)
                            )
                        }
                        None => "uninhabited".into(),
                    };
                    ui.label(RichText::new(owner).color(CREAM_DIM));
                    ui.add_space(4.0);
                    glow_label(ui, &w.date().to_string(), mono(20.0), VFD);
                });
            let r = frame.response.rect;
            ui.painter().rect_filled(
                Rect::from_min_size(r.min, Vec2::new(6.0, r.height())),
                0.0,
                RED,
            );
        });

    // Bottom: flight instruments.
    if w.mode == Mode::Normal {
        egui::Area::new(egui::Id::unique("flight"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -18.0])
            .show(ctx, |ui| flight_panel(ui, w));
    }

    // Prompts.
    let prompts = w.prompts();
    let mut lines: Vec<(Option<&str>, String)> = Vec::new();
    if w.mode == Mode::Normal {
        match (prompts.fast_travel, prompts.well, w.target) {
            (Some(t), _, _) => lines.push((Some("J"), format!("fast travel to {}", w.name(t)))),
            (None, Some(well), Some(_)) => lines.push((
                None,
                format!(
                    "fast travel and jumps: leave the gravity well of {} first",
                    w.name(well)
                ),
            )),
            _ => {}
        }
        if let Some(s) = prompts.dock {
            lines.push((Some("F"), format!("dock at {}", w.name(Place::Station(s)))));
        }
        if let Some((city, body)) = prompts.land {
            lines.push((
                Some("L"),
                format!(
                    "land at {}, {}",
                    w.galaxy.settlements[city].name,
                    w.name(Place::Body(body))
                ),
            ));
        }
    }
    if !lines.is_empty() {
        egui::Area::new(egui::Id::unique("prompts"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -190.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| prompt_lines(ui, lines));
            });
    }

    if let Some((text, _)) = &w.message {
        egui::Area::new(egui::Id::unique("message"))
            .anchor(Align2::CENTER_TOP, [0.0, 72.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(RED.gamma_multiply(0.9))
                    .inner_margin(Margin::symmetric(18, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(text.to_uppercase())
                                .font(display(26.0))
                                .color(CREAM),
                        );
                    });
            });
    }

    // Navigation lists.
    if w.mode == Mode::Normal {
        let open_space = prompts.well.is_none();
        panel(
            ctx,
            Panel {
                id: "hyperlanes",
                title: "Hyperlanes",
                anchor: Align2::RIGHT_BOTTOM,
                offset: [-16.0, -16.0],
                width: 400.0,
                collapsible: Some(false),
            },
            |ui| {
                if !open_space {
                    ui.label(RichText::new("leave the gravity well to jump").color(AMBER));
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
                                .color(CREAM_DIM),
                            );
                            if ui
                                .add_enabled(open_space, egui::Button::new("JUMP"))
                                .clicked()
                            {
                                actions.push(Action::Jump(lane.to));
                            }
                            ui.end_row();
                        }
                    });
            },
        );
        panel(
            ctx,
            Panel {
                id: "navigation",
                title: "Navigation",
                anchor: Align2::RIGHT_TOP,
                offset: [-16.0, 16.0],
                width: 380.0,
                collapsible: Some(true),
            },
            |ui| {
                egui::ScrollArea::vertical()
                    .max_height(screen.height() * 0.55)
                    .show(ui, |ui| {
                        egui::Grid::new("nav")
                            .num_columns(2)
                            .spacing([12.0, 1.0])
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
                                                    .color(CREAM_DIM),
                                            );
                                        },
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            },
        );
    }

    match w.mode {
        Mode::Docked(s) => settlement_screen(ctx, w, s, "Station", Action::Undock, &mut actions),
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
                title_card(
                    &painter,
                    screen.center(),
                    "Descent",
                    &w.galaxy.settlements[city].name,
                    CREAM,
                );
            } else {
                settlement_screen(ctx, w, city, "Surface", Action::Launch, &mut actions);
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
                Color32::from_rgba_unmultiplied(238, 228, 250, alpha),
            );
            title_card(
                &painter,
                screen.center(),
                "Hyperlane",
                &w.galaxy.systems[to].name,
                INK,
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
            for i in 0..72 {
                let a = i as f32 * 2.399;
                let f = (i as f32 * 0.37 + phase).fract();
                let r = f * screen.width() * 0.6;
                let dir = Vec2::new(a.cos(), a.sin());
                let color = if i % 3 == 0 { VFD } else { CREAM_DIM };
                painter.line_segment(
                    [c + dir * r, c + dir * (r + 40.0 + r * 0.25)],
                    Stroke::new(1.0 + f, color.gamma_multiply(f)),
                );
            }
            let days = (w.clock_s - trip.depart_s) / SECONDS_PER_DAY;
            let km_s = w.ship.vel.length() / 1000.0;
            let au = w.distance_to(trip.target) / AU;
            title_card(
                &painter,
                c + Vec2::new(0.0, -screen.height() * 0.3),
                "Fast travel",
                &format!(
                    "{} · {au:.2} AU · {km_s:.0} km/s · {days:.1} d · J drop out",
                    w.name(trip.target)
                ),
                CREAM,
            );
        }
        _ => {}
    }

    // Controls help.
    egui::Area::new(egui::Id::unique("help"))
        .anchor(Align2::LEFT_BOTTOM, [18.0, -14.0])
        .show(ctx, |ui| {
            let text = if h.captured {
                "mouse steer · W/S throttle · X stop · A/D strafe · SPACE/CTRL lift · Q/E roll · SHIFT boost\n\
                 J fast travel · T next target · V camera · Z flight assist · TAB free mouse"
            } else {
                "TAB or click the view to fly with the mouse · arrow keys also steer"
            };
            ui.label(RichText::new(text).size(11.0).color(CREAM_DIM));
        });

    actions
}

/// Key prompts, one per line; a line without a key is a warning.
fn prompt_lines(ui: &mut egui::Ui, lines: Vec<(Option<&str>, String)>) {
    for (key, text) in lines {
        ui.horizontal(|ui| match key {
            Some(k) => {
                keycap(ui, k);
                ui.label(RichText::new(text).color(CREAM));
            }
            None => {
                ui.label(RichText::new(text).color(AMBER));
            }
        });
    }
}

/// Speed, throttle, flight mode and what we're near or aimed at.
fn flight_panel(ui: &mut egui::Ui, w: &World) {
    let width = 440.0;
    let frame = egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, RULE))
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_width(width);
            let speed = w.ship.vel.length();
            let cruise = speed > COMBAT_SPEED * 1.05;
            ui.horizontal(|ui| {
                lamp(ui, if cruise { "CRUISE" } else { "NORMAL" }, cruise, AMBER);
                ui.add_space(6.0);
                glow_label(ui, &speed_text(speed), mono(30.0), VFD);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    lamp(ui, "FA OFF", !w.ship.flight_assist, ALARM);
                });
            });
            ui.add_space(4.0);
            let throttle = w.ship.throttle as f32;
            segment_bar(
                ui,
                throttle,
                width,
                if throttle < 0.0 { AMBER } else { VFD },
            );
            ui.add_space(6.0);
            if let Some(well) = w.current_well() {
                ui.label(
                    RichText::new(format!("GRAVITY WELL · {}", w.name(well).to_uppercase()))
                        .color(CREAM_DIM),
                );
            }
            if let Some(t) = w.target {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TARGET").color(CREAM_DIM));
                    ui.label(RichText::new(w.name(t).to_uppercase()).color(AMBER));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(distance(w.distance_to(t))).color(AMBER));
                    });
                });
            }
        });
    // A red rule along the top edge.
    let r = frame.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.min, Vec2::new(r.width(), 3.0)),
        0.0,
        RED,
    );
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
    panel(
        ctx,
        Panel {
            id: "settlement",
            title,
            anchor: Align2::LEFT_CENTER,
            offset: [24.0, 0.0],
            width: 440.0,
            collapsible: None,
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(settlement.name.to_uppercase())
                    .font(display(38.0))
                    .color(CREAM),
            );
            let faction = &w.galaxy.factions[settlement.owner];
            ui.label(
                RichText::new(format!(
                    "{} · {} economy",
                    faction.name,
                    economy_name(faction.economy)
                ))
                .color(CREAM_DIM),
            );
            ui.label(
                RichText::new(format!("population {:.1}k", settlement.economy.population))
                    .color(CREAM_DIM),
            );
            ui.add_space(10.0);
            section(ui, "Market");
            ui.spacing_mut().item_spacing.y = 4.0;
            egui::Grid::new("market")
                .striped(true)
                .num_columns(4)
                .spacing([18.0, 5.0])
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for head in ["GOOD", "STOCK", "PRICE", ""] {
                        ui.label(RichText::new(head).size(11.0).color(CREAM_DIM));
                    }
                    ui.end_row();
                    for good in Good::ALL {
                        ui.label(good.name());
                        ui.label(RichText::new(format!("{:.0}", market.stock[good])).color(VFD));
                        ui.label(RichText::new(format!("{:.2} cr", market.price[good])).color(VFD));
                        let fill = market.fill(good);
                        if market.shortfall[good] > 0.01 {
                            tag(ui, "SHORTAGE", ALARM);
                        } else if fill > 2.5 {
                            tag(ui, "SURPLUS", VFD);
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let label = if leave == Action::Launch {
                    "LAUNCH"
                } else {
                    "UNDOCK"
                };
                let primary =
                    egui::Button::new(RichText::new(label).font(display(22.0)).color(CREAM))
                        .fill(RED)
                        .min_size(Vec2::new(110.0, 30.0));
                if ui.add(primary).clicked() {
                    actions.push(leave);
                }
                ui.add_space(8.0);
                ui.label(RichText::new("WAIT").color(CREAM_DIM));
                for (days, text) in [(1, "1 D"), (7, "7 D"), (30, "30 D")] {
                    let button = egui::Button::new(text).min_size(Vec2::new(0.0, 30.0));
                    if ui.add(button).clicked() {
                        actions.push(Action::Wait(days));
                    }
                }
            });
        },
    );
}

fn economy_name(model: EconomyModel) -> &'static str {
    match model {
        EconomyModel::Planned => "planned",
        EconomyModel::Market => "market",
    }
}

/// A HUD panel: a red title band (click to fold, if `collapsible`) over a
/// dark body, framed by cream rules with corner brackets.
struct Panel<'a> {
    id: &'a str,
    title: &'a str,
    anchor: Align2,
    offset: [f32; 2],
    width: f32,
    /// `Some(open by default)` to let the title band fold the panel.
    collapsible: Option<bool>,
}

fn panel(ctx: &egui::Context, p: Panel, add: impl FnOnce(&mut egui::Ui)) {
    let id = egui::Id::unique(p.id);
    let mut open = p
        .collapsible
        .is_none_or(|default| ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(default));
    egui::Area::new(id)
        .anchor(p.anchor, p.offset)
        .show(ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(PANEL)
                .stroke(Stroke::new(1.0, RULE))
                .show(ui, |ui| {
                    ui.set_width(p.width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let sense = if p.collapsible.is_some() {
                        Sense::click()
                    } else {
                        Sense::hover()
                    };
                    let (band, response) = ui.allocate_exact_size(Vec2::new(p.width, 30.0), sense);
                    let fold = p.collapsible.map(|_| open);
                    title_band(
                        ui.painter(),
                        band,
                        p.title,
                        fold,
                        response.hovered() && fold.is_some(),
                    );
                    if response.clicked() {
                        open = !open;
                        ctx.data_mut(|d| d.insert_temp(id, open));
                    }
                    if open {
                        egui::Frame::new()
                            .inner_margin(Margin {
                                left: 12,
                                right: 12,
                                top: 8,
                                bottom: 10,
                            })
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.set_width(p.width - 24.0);
                                add(ui);
                            });
                    }
                });
            brackets(
                ui.painter(),
                frame.response.rect.expand(3.0),
                10.0,
                CREAM_DIM,
            );
        });
}

/// A red band with the title in display type and constructivist slashes.
fn title_band(painter: &egui::Painter, rect: Rect, title: &str, fold: Option<bool>, hot: bool) {
    painter.rect_filled(rect, 0.0, if hot { RED_HOT } else { RED });
    // Diagonal slashes at the right end.
    for k in 0..4 {
        let x = rect.right() - 16.0 - k as f32 * 11.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 8.0, rect.bottom()),
                Pos2::new(x - 2.0, rect.bottom()),
                Pos2::new(x + 6.0, rect.top()),
                Pos2::new(x, rect.top()),
            ],
            RED_DARK,
            Stroke::NONE,
        ));
    }
    let mut x = rect.left() + 10.0;
    if let Some(open) = fold {
        let c = Pos2::new(x + 5.0, rect.center().y);
        let points = if open {
            vec![
                c + Vec2::new(-5.0, -3.0),
                c + Vec2::new(5.0, -3.0),
                c + Vec2::new(0.0, 4.0),
            ]
        } else {
            vec![
                c + Vec2::new(-3.0, -5.0),
                c + Vec2::new(4.0, 0.0),
                c + Vec2::new(-3.0, 5.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(points, CREAM, Stroke::NONE));
        x += 18.0;
    }
    painter.text(
        Pos2::new(x, rect.center().y + 2.0),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        display(25.0),
        CREAM,
    );
}

/// A sub-heading with a rule running to the right edge.
fn section(ui: &mut egui::Ui, title: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(title.to_uppercase(), display(20.0), CREAM_DIM);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), galley.size().y),
        Sense::hover(),
    );
    let text_w = galley.size().x;
    ui.painter().galley(rect.min, galley, CREAM_DIM);
    let y = rect.center().y;
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + text_w + 8.0, y),
            Pos2::new(rect.right(), y),
        ],
        Stroke::new(1.0, RULE),
    );
}

/// Corner brackets around `rect`.
fn brackets(painter: &egui::Painter, rect: Rect, len: f32, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    for (corner, dx, dy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.left_bottom(), 1.0, -1.0),
        (rect.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + Vec2::new(dx * len, 0.0)], stroke);
        painter.line_segment([corner, corner + Vec2::new(0.0, dy * len)], stroke);
    }
}

/// Text with a soft glow, like a vacuum-fluorescent display.
fn glow_label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    let glow = color.gamma_multiply(0.12);
    for (dx, dy) in [
        (-1.5, 0.0),
        (1.5, 0.0),
        (0.0, -1.5),
        (0.0, 1.5),
        (-1.0, -1.0),
        (1.0, 1.0),
    ] {
        ui.painter().galley_with_override_text_color(
            rect.min + Vec2::new(dx, dy),
            galley.clone(),
            glow,
        );
    }
    ui.painter().galley(rect.min, galley, color);
}

/// An indicator lamp: lit (filled) or dark (outlined).
fn lamp(ui: &mut egui::Ui, text: &str, lit: bool, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), mono(13.0), color);
    let size = galley.size() + Vec2::new(12.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    if lit {
        painter.rect_filled(rect.expand(2.0), 0.0, color.gamma_multiply(0.2));
        painter.rect_filled(rect, 0.0, color);
    } else {
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, color.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }
    let text_color = if lit { INK } else { color.gamma_multiply(0.45) };
    painter.galley_with_override_text_color(
        rect.center() - galley.size() / 2.0,
        galley,
        text_color,
    );
}

/// A small coloured tag, outlined.
fn tag(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), mono(11.0), color);
    let size = galley.size() + Vec2::new(8.0, 2.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// A key, drawn as a cream keycap.
fn keycap(ui: &mut egui::Ui, key: &str) {
    let galley = ui.painter().layout_no_wrap(key.to_owned(), mono(13.0), INK);
    let size = Vec2::new((galley.size().x + 10.0).max(20.0), galley.size().y + 4.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 2.0, CREAM);
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(2.0, CREAM_DIM),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, INK);
}

/// A segmented VFD bar for a value in -1..=1 (negative lights from the left
/// too, in the reverse colour), with quarter ticks underneath.
fn segment_bar(ui: &mut egui::Ui, value: f32, width: f32, color: Color32) {
    let height = 12.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height + 6.0), Sense::hover());
    let painter = ui.painter();
    let n = 40;
    let gap = 2.0;
    let seg = (width - gap * (n - 1) as f32) / n as f32;
    let lit = (value.abs() * n as f32).round() as usize;
    for i in 0..n {
        let x = rect.left() + i as f32 * (seg + gap);
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(seg, height));
        if i < lit {
            painter.rect_filled(r.expand(1.0), 0.0, color.gamma_multiply(0.18));
            painter.rect_filled(r, 0.0, color);
        } else {
            painter.rect_filled(r, 0.0, VFD_DIM);
        }
    }
    for q in 0..=4 {
        let x = rect.left() + width * q as f32 / 4.0;
        let x = x.clamp(rect.left() + 0.5, rect.right() - 0.5);
        painter.line_segment(
            [
                Pos2::new(x, rect.top() + height + 1.0),
                Pos2::new(x, rect.bottom()),
            ],
            Stroke::new(1.0, RULE),
        );
    }
}

/// A big centred caption with a red rule and a subtitle, for transitions.
fn title_card(painter: &egui::Painter, centre: Pos2, title: &str, subtitle: &str, color: Color32) {
    let r = painter.text(
        centre,
        Align2::CENTER_BOTTOM,
        title.to_uppercase(),
        display(56.0),
        color,
    );
    painter.rect_filled(
        Rect::from_center_size(
            Pos2::new(centre.x, r.bottom() + 4.0),
            Vec2::new(r.width() + 40.0, 4.0),
        ),
        0.0,
        RED,
    );
    painter.text(
        Pos2::new(centre.x, r.bottom() + 14.0),
        Align2::CENTER_TOP,
        subtitle.to_uppercase(),
        mono(20.0),
        color,
    );
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
        AMBER,
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
