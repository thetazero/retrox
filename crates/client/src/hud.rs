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

mod combat;
pub(crate) mod guide;
mod map;
mod mining;
pub(crate) mod skim;

pub(crate) use map::drum;

use crate::world::{AU, Action, COMBAT_SPEED, Leg, Mode, Place, SECONDS_PER_DAY, World};

/// Enamel cream: body text and rules.
pub(crate) const CREAM: Color32 = Color32::from_rgb(234, 222, 196);
pub(crate) const CREAM_DIM: Color32 = Color32::from_rgb(160, 146, 122);
pub(crate) const RULE: Color32 = Color32::from_rgb(110, 98, 84);
/// Signal red: title bands and primary buttons.
pub(crate) const RED: Color32 = Color32::from_rgb(196, 44, 34);
pub(crate) const RED_DARK: Color32 = Color32::from_rgb(118, 26, 22);
pub(crate) const RED_HOT: Color32 = Color32::from_rgb(232, 70, 52);
/// Warnings and alarms.
pub(crate) const ALARM: Color32 = Color32::from_rgb(255, 88, 64);
/// Amber: targets and selection.
pub(crate) const AMBER: Color32 = Color32::from_rgb(255, 178, 76);
/// Vacuum-fluorescent teal: live instrument readouts.
pub(crate) const VFD: Color32 = Color32::from_rgb(110, 240, 214);
pub(crate) const VFD_DIM: Color32 = Color32::from_rgb(22, 58, 56);
/// HUD panels. Blending happens in linear light on the sRGB surface, so
/// a bright scene shows through far more than the alpha suggests: keep it
/// near opaque.
pub(crate) const PANEL: Color32 = Color32::from_rgba_premultiplied(11, 9, 16, 248);
/// Menu pages (handbook, settings, load): opaque, as they're read at length
/// (issue #124).
pub(crate) const PAGE_FILL: Color32 = Color32::from_rgb(11, 9, 16);
pub(crate) const INK: Color32 = Color32::from_rgb(20, 16, 24);

const DISPLAY: &str = "display";

pub(crate) fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

pub(crate) fn mono(size: f32) -> FontId {
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
    w.inactive.weak_bg_fill = crate::widgets::FACE;
    w.inactive.bg_fill = crate::widgets::FACE;
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
    /// The controls card is up (H).
    pub controls_card: bool,
    /// Show the first-steps notes (a setting).
    pub notes: bool,
}

pub fn draw(ctx: &egui::Context, h: HudInput) -> Vec<Action> {
    let mut actions = Vec::new();
    let w = h.world;
    let screen = ctx.content_rect();

    if w.map_open && w.map_available() {
        map::draw(ctx, w, &mut actions);
        return actions;
    }

    // Markers for every place; labels placed nearest-first (target always
    // first) and skipped where they'd overlap one already drawn.
    if matches!(w.mode, Mode::Normal | Mode::FastTravel(_)) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        // (targeted, distance, where, text, colour)
        let mut labels: Vec<(bool, f64, Pos2, String, Color32)> = Vec::new();
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
                format!("{}  {}", w.short_name(place).to_uppercase(), distance(dist)),
                CREAM_DIM,
            ));
        }
        // Traffic: the bubble's fleets, as small squares in the VFD's
        // colour, labelled once they're near enough to matter.
        let me = w.ship_position();
        for c in &w.traffic.contacts {
            // A fleet in the fight's scene is marked ship by ship there.
            if w.combat.has_convoy(c.key) {
                continue;
            }
            let rel = (c.pos - h.camera).as_vec3();
            let Some(p) = project(h.view_proj, rel, screen) else {
                continue;
            };
            let dist = (c.pos - me).length();
            let r = 3.0;
            painter.rect_stroke(
                Rect::from_center_size(p, Vec2::splat(2.0 * r)),
                0.0,
                Stroke::new(1.0, VFD),
                egui::StrokeKind::Middle,
            );
            if dist < TRAFFIC_LABEL_M {
                labels.push((false, dist, p, contact_label(c, dist), VFD));
            }
        }
        // Pirates: red diamonds, a doubled one for a gang after us. A gang
        // coming for us is labelled at any range.
        for c in &w.traffic.pirates {
            let rel = (c.pos - h.camera).as_vec3();
            let Some(p) = project(h.view_proj, rel, screen) else {
                continue;
            };
            let dist = (c.pos - me).length();
            let hunting = c.aim == retrox_sim::pirates::Aim::Stalk;
            let color = if hunting { ALARM } else { RED_HOT };
            diamond(&painter, p, 4.5, color);
            if hunting {
                diamond(&painter, p, 8.0, color.gamma_multiply(0.7));
            }
            if dist < TRAFFIC_LABEL_M || hunting {
                labels.push((false, dist, p, pirate_label(c, dist), color));
            }
        }
        labels.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        let mut taken: Vec<Rect> = Vec::new();
        for (targeted, _, p, text, color) in labels {
            let (font, color) = if targeted {
                (mono(13.0), AMBER)
            } else {
                (mono(11.0), color)
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
            crate::widgets::shadowed_galley(&painter, rect.min + Vec2::splat(2.0), galley);
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

    // Over a gas giant the scoop's panel takes the mining panel's place.
    if skim::showing(&h) {
        skim::draw(ctx, &h);
    } else {
        mining::draw(ctx, &h);
    }
    combat::draw(ctx, &h);

    // Top-left: where and when.
    egui::Area::new(egui::Id::unique("where"))
        .anchor(Align2::LEFT_TOP, [18.0, 14.0])
        .show(ctx, |ui| {
            // A faint backing keeps it legible over bright planets.
            let frame = egui::Frame::new()
                .fill(crate::widgets::READOUT_FILL)
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
                    ui.horizontal(|ui| {
                        glow_label(ui, &w.date().to_string(), mono(20.0), VFD);
                        ui.add_space(10.0);
                        glow_label(ui, &credits(w.player.credits), mono(20.0), AMBER);
                    });
                });
            let r = frame.response.rect;
            ui.painter().rect_filled(
                Rect::from_min_size(r.min, Vec2::new(6.0, r.height())),
                0.0,
                RED,
            );
        });

    if matches!(w.mode, Mode::Normal | Mode::FastTravel(_)) {
        crate::contracts_ui::flight_readout(ctx, w);
    }

    // Bottom: flight instruments.
    if w.mode == Mode::Normal && w.combat.lost.is_none() {
        egui::Area::new(egui::Id::unique("flight"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -18.0])
            .show(ctx, |ui| flight_panel(ui, w));
    }

    // Prompts.
    let prompts = w.prompts();
    let mut lines: Vec<(Option<&str>, String)> = Vec::new();
    let hostiles = w.hostiles_near();
    if hostiles > 0 && w.combat.lost.is_none() {
        let plural = if hostiles == 1 { "" } else { "s" };
        lines.push((
            Some("Y"),
            format!("{hostiles} hostile{plural} near · next target · no fast travel or jumps"),
        ));
    }
    if w.mode == Mode::Normal && w.combat.lost.is_none() {
        match (prompts.fast_travel, prompts.climb) {
            (Some(t), _) => lines.push((Some("J"), format!("fast travel to {}", w.name(t)))),
            (None, Some(well)) => lines.push((
                Some("J"),
                format!("climb out of the gravity well of {}", w.name(well)),
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
            // More than one city down there (issue #116): say how to pick.
            let others = w.cities_on(body).count() - 1;
            if others > 0 {
                let plural = if others == 1 { "y" } else { "ies" };
                lines.push((
                    Some("T"),
                    format!("{others} more cit{plural} here · pick one in NAVIGATION"),
                ));
            }
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
        banner(ctx, screen, text);
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
                lanes_list(ui, w, open_space, &mut actions);
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
                    .show(ui, |ui| nav_list(ui, w, &mut actions));
            },
        );
    }

    if let Some((t, gang)) = &w.interdicted {
        interdicted(ctx, screen, *t as f32, gang);
    }

    match w.mode {
        Mode::Docked(s) => crate::station_ui::draw(ctx, w, s, &mut actions),
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
                crate::station_ui::draw(ctx, w, city, &mut actions);
            }
        }
        Mode::Jumping { to, timer } => {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("jump"),
            ));
            let t = 1.0 - timer / crate::world::JUMP_DURATION;
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
            let (title, detail) = match trip.leg {
                Leg::To(place) => {
                    let au = w.distance_to(place) / AU;
                    ("Fast travel", format!("{} · {au:.2} AU", w.name(place)))
                }
                Leg::Intercept(fleet) => {
                    let company = w
                        .galaxy
                        .companies
                        .get(fleet.0)
                        .map_or("the convoy".to_owned(), |c| c.name.clone());
                    let d = w
                        .traffic
                        .contacts
                        .iter()
                        .find(|c| c.key == fleet)
                        .map_or(0.0, |c| (c.pos - w.ship_position()).length());
                    ("Intercept", format!("{company} · {}", distance(d)))
                }
                Leg::Climb { well, .. } => {
                    let out = (w.well_radius(well) - w.distance_to(well)).max(0.0) / 1000.0;
                    (
                        "Climbing out",
                        format!("gravity well of {} · {out:.0} km to the edge", w.name(well)),
                    )
                }
            };
            title_card(
                &painter,
                c + Vec2::new(0.0, -screen.height() * 0.3),
                title,
                &format!("{detail} · {km_s:.0} km/s · {days:.1} d · J drop out"),
                CREAM,
            );
        }
        _ => {}
    }

    // Help for new pilots.
    if h.notes {
        guide::notes(ctx, w);
    }
    guide::key_strip(ctx, w, h.captured);
    if h.controls_card {
        guide::controls_card(ctx);
    }

    actions
}

/// The red message banner at the top of the view, sized to its text and
/// wrapped only at spaces past [`BANNER_MAX_W`] (issue #120). Painted
/// straight onto a layer rather than in an `Area`, so its width can't
/// carry over from an earlier, longer or shorter, message.
fn banner(ctx: &egui::Context, screen: Rect, text: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("message-banner"),
    ));
    let margin = Vec2::new(18.0, 4.0);
    let wrap = BANNER_MAX_W.min(screen.width() - 2.0 * (32.0 + margin.x));
    let mut job = egui::text::LayoutJob::simple(text.to_uppercase(), display(26.0), CREAM, wrap);
    job.halign = Align2::CENTER_TOP.x();
    let galley = painter.layout_job(job);
    let top = Pos2::new(screen.center().x, screen.top() + 72.0);
    let size = galley.size() + 2.0 * margin;
    let plate = Rect::from_min_size(top - Vec2::new(size.x / 2.0, 0.0), size);
    painter.rect_filled(plate, 0.0, RED.gamma_multiply(0.94));
    painter.rect_filled(
        Rect::from_min_size(plate.left_bottom() - Vec2::new(0.0, 2.0), Vec2::new(plate.width(), 2.0)),
        0.0,
        RED_DARK,
    );
    painter.galley(top + Vec2::new(0.0, margin.y), galley, CREAM);
}

/// The widest the message banner grows before it wraps.
const BANNER_MAX_W: f32 = 760.0;

/// The system's places as a list of instrument rows: a glyph for what each
/// is (moons hang off their planet, and cities off the body they're on),
/// its name, and the range in VFD teal.
fn nav_list(ui: &mut egui::Ui, w: &World, actions: &mut Vec<Action>) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let rows = w.nav_entries();
    let depths: Vec<usize> = rows.iter().map(|&r| w.nav_depth(r)).collect();
    let selected_row = w.nav_selected();
    let bound: Vec<SettlementId> = w.player.contracts.iter().map(|c| c.offer.to).collect();
    let mut heading: Option<&str> = None;
    for (i, &(place, city)) in rows.iter().enumerate() {
        let depth = depths[i];
        // Stations round a body hang off it; only those round the star
        // get a heading of their own.
        let group = match place {
            _ if depth > 0 => heading,
            Place::Star | Place::Body(_) => None,
            Place::Field(_) => Some("Fields"),
            Place::Station(_) => Some("Stations"),
            Place::Convoy(_) => Some("Traffic"),
        };
        if group.is_some() && group != heading {
            ui.add_space(8.0);
            section(ui, group.unwrap_or_default());
            ui.add_space(2.0);
            heading = group;
        }
        let selected = selected_row == Some((place, city));
        let (rect, response, look) = crate::widgets::list_row(ui, 25.0, selected, true, AMBER);
        if response.clicked() {
            actions.push(match city {
                Some(c) => Action::TargetCity(c),
                None => Action::Target(place),
            });
        }
        let painter = ui.painter();
        let column = |d: usize| rect.left() + 16.0 + 16.0 * d as f32;
        let glyph_x = column(depth);
        let c = Pos2::new(glyph_x, rect.center().y);
        let ink = if look.selected && !look.hot {
            AMBER
        } else {
            CREAM_DIM
        };
        // The tree's brackets: an elbow from the row's parent, and the
        // parent's (and grandparent's) line running on through to a later
        // sibling.
        let stroke = Stroke::new(1.0, RULE);
        let sibling_follows = |d: usize| {
            depths[i + 1..]
                .iter()
                .take_while(|&&e| e >= d)
                .any(|&e| e == d)
        };
        for d in 1..=depth {
            let x = column(d - 1);
            let follows = sibling_follows(d);
            if d == depth {
                let bottom = if follows { rect.bottom() } else { c.y };
                painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, bottom)], stroke);
                painter.line_segment([Pos2::new(x, c.y), Pos2::new(c.x - 6.0, c.y)], stroke);
            } else if follows {
                painter.line_segment(
                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                    stroke,
                );
            }
        }
        match city {
            Some(_) => city_glyph(painter, c, ink),
            None => place_glyph(painter, w, place, c, ink),
        }
        let label = match (place, city) {
            (_, Some(c)) => w.galaxy.settlements[c].name.clone(),
            // A station under its body: the bracket says whose it is.
            (Place::Station(s), None) => {
                let name = w.name(place);
                w.station_body(s)
                    .and_then(|b| {
                        name.strip_prefix(w.galaxy.bodies[b].name.as_str())
                            .map(str::trim_start)
                            .filter(|r| !r.is_empty())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| w.short_name(place))
            }
            _ => w.short_name(place),
        };
        let tag = match (place, city) {
            // Pirates are known where they lair.
            (Place::Field(f), _) if w.gang_reported(f) => Some(("RAIDERS", ALARM)),
            // Where the hold is bound for (issue #116).
            (_, Some(s)) | (Place::Station(s), None) if bound.contains(&s) => {
                Some(("CARGO", AMBER))
            }
            _ => None,
        };
        let tag = tag.map(|(text, colour)| painter.layout_no_wrap(text.into(), mono(11.0), colour));
        // A city is as far as its planet: the range is on the planet's row.
        let range = city.is_none().then(|| {
            let colour = if look.hot {
                CREAM
            } else if look.selected {
                AMBER.gamma_multiply(0.85)
            } else {
                VFD.gamma_multiply(0.85)
            };
            painter.layout_no_wrap(distance(w.distance_to(place)), mono(13.0), colour)
        });
        // The name gets what's left between the glyph and the range.
        let name_x = glyph_x + 14.0;
        let room = rect.right()
            - 6.0
            - range.as_ref().map_or(0.0, |g| g.size().x + 10.0)
            - tag.as_ref().map_or(0.0, |g| g.size().x + 8.0)
            - name_x;
        let name = painter.text(
            Pos2::new(name_x, c.y),
            Align2::LEFT_CENTER,
            crate::widgets::fit(painter, &label, mono(14.0), room),
            mono(14.0),
            look.text(AMBER),
        );
        if let Some(tag) = tag {
            painter.galley(
                Pos2::new(name.right() + 8.0, c.y - tag.size().y / 2.0),
                tag,
                Color32::PLACEHOLDER,
            );
        }
        if let Some(range) = range {
            painter.galley(
                Pos2::new(rect.right() - 6.0 - range.size().x, c.y - range.size().y / 2.0),
                range,
                Color32::PLACEHOLDER,
            );
        }
    }
}

/// A city on the chart: a little skyline of three blocks.
fn city_glyph(painter: &egui::Painter, c: Pos2, ink: Color32) {
    for (dx, h) in [(-3.5, 5.0), (0.0, 8.0), (3.5, 6.0)] {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(c.x + dx - 1.3, c.y + 4.0 - h),
                Pos2::new(c.x + dx + 1.3, c.y + 4.0),
            ),
            0.0,
            ink,
        );
    }
}

/// A small chart symbol for a place.
fn place_glyph(painter: &egui::Painter, w: &World, place: Place, c: Pos2, ink: Color32) {
    let stroke = Stroke::new(1.2, ink);
    match place {
        Place::Star => {
            painter.circle_filled(c, 4.0, AMBER);
            for k in 0..8 {
                let d = Vec2::angled(k as f32 * std::f32::consts::FRAC_PI_4);
                painter.line_segment([c + d * 5.5, c + d * 7.5], Stroke::new(1.0, AMBER));
            }
        }
        Place::Body(b) => match w.galaxy.bodies[b].kind {
            BodyKind::Moon => {
                painter.circle_filled(c, 2.5, ink);
            }
            BodyKind::GasGiant => {
                painter.circle_stroke(c, 5.0, stroke);
                painter.line_segment([c + Vec2::new(-8.5, 2.5), c + Vec2::new(8.5, -2.5)], stroke);
            }
            BodyKind::Belt => {
                for k in 0..5 {
                    let a = std::f32::consts::PI * (0.15 + 0.175 * k as f32);
                    painter.circle_filled(c + Vec2::angled(a) * Vec2::new(7.0, -4.0), 1.2, ink);
                }
            }
            BodyKind::Planet(_) => {
                painter.circle_stroke(c, 5.0, stroke);
                painter.circle_filled(c + Vec2::new(-1.5, -1.5), 1.6, ink);
            }
        },
        Place::Field(_) => {
            for d in [
                Vec2::new(-4.0, 2.5),
                Vec2::new(0.0, -3.0),
                Vec2::new(4.0, 2.0),
            ] {
                painter.circle_filled(c + d, 1.6, ink);
            }
        }
        Place::Convoy(_) => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(6.0)),
                0.0,
                Stroke::new(1.2, VFD),
                egui::StrokeKind::Middle,
            );
        }
        Place::Station(_) => {
            let r = 5.0;
            painter.add(egui::Shape::closed_line(
                vec![
                    c + Vec2::new(0.0, -r),
                    c + Vec2::new(r, 0.0),
                    c + Vec2::new(0.0, r),
                    c + Vec2::new(-r, 0.0),
                ],
                Stroke::new(1.2, VFD),
            ));
        }
    }
}

/// The lanes out of this system: name, length and days, and a jump key
/// (dark while the gravity well holds the ship).
fn lanes_list(ui: &mut egui::Ui, w: &World, open_space: bool, actions: &mut Vec<Action>) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let next_hop = w.next_hop();
    if !open_space {
        ui.horizontal(|ui| {
            lamp(ui, "IN WELL", true, AMBER);
            ui.label(RichText::new("leave the gravity well to jump").color(AMBER));
        });
        ui.add_space(6.0);
    }
    for lane in w.lanes() {
        let on_course = next_hop == Some(lane.to);
        let (rect, response, look) = crate::widgets::list_row(ui, 30.0, on_course, true, AMBER);
        if response.clicked() {
            actions.push(Action::PlotCourse(Some(lane.to)));
        }
        let painter = ui.painter();
        let y = rect.center().y;
        let mut x = rect.left() + 12.0;
        if on_course {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - 3.0, y - 5.0),
                    Pos2::new(x + 4.0, y),
                    Pos2::new(x - 3.0, y + 5.0),
                ],
                AMBER,
                Stroke::NONE,
            ));
        }
        x += 10.0;
        // The jump key sits inside the row, clear of the panel's edge
        // (issue #125), and the rest shares out what's left.
        let key = Rect::from_min_max(
            Pos2::new(rect.right() - 6.0 - JUMP_KEY_W, rect.top() + 4.0),
            Pos2::new(rect.right() - 6.0, rect.bottom() - 4.0),
        );
        let leg = painter.layout_no_wrap(
            format!("{:.1} LY · {:.0} D", lane.length_ly, lane.days),
            mono(12.0),
            if on_course {
                AMBER
            } else {
                VFD.gamma_multiply(0.85)
            },
        );
        let leg_x = key.left() - 10.0 - leg.size().x;
        painter.galley(Pos2::new(leg_x, y - leg.size().y / 2.0), leg, VFD);
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            crate::widgets::fit(painter, &lane.name, mono(14.0), leg_x - 10.0 - x),
            mono(14.0),
            look.text(AMBER),
        );
        let look = if on_course {
            crate::widgets::Look::Primary
        } else {
            crate::widgets::Look::Plain
        };
        if crate::widgets::push_in(
            ui,
            key,
            egui::Id::new(("jump", lane.to)),
            "JUMP",
            display(18.0),
            look,
            open_space,
        )
        .clicked()
        {
            actions.push(Action::Jump(lane.to));
        }
    }
    if let Some(dest) = w.course {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "COURSE · {}",
                w.galaxy.systems[dest].name.to_uppercase()
            ))
            .color(AMBER),
        );
    }
    ui.add_space(8.0);
    // `horizontal`, not a centred layout: that would stretch to the
    // panel's height from the frame before and keep a tall empty band
    // under a short list (issue #125).
    ui.horizontal(|ui| {
        keycap(ui, "M");
        if crate::widgets::push(
            ui,
            "GALAXY MAP",
            display(18.0),
            crate::widgets::Look::Plain,
            true,
            Vec2::new(0.0, 24.0),
        )
        .clicked()
        {
            actions.push(Action::ToggleMap);
        }
    });
}

/// The width of a lane's JUMP key.
const JUMP_KEY_W: f32 = 64.0;

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
            combat::gauges(ui, w, width);
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

pub(crate) fn economy_name(model: EconomyModel) -> &'static str {
    match model {
        EconomyModel::Planned => "planned",
        EconomyModel::Market => "market",
    }
}

/// A HUD panel: a red title band (click to fold, if `collapsible`) over a
/// dark body, framed by cream rules with corner brackets.
pub(crate) struct Panel<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub anchor: Align2,
    pub offset: [f32; 2],
    pub width: f32,
    /// `Some(open by default)` to let the title band fold the panel.
    pub collapsible: Option<bool>,
}

pub(crate) fn panel(ctx: &egui::Context, p: Panel, add: impl FnOnce(&mut egui::Ui)) {
    panel_filled(ctx, p, PANEL, add);
}

/// A [`panel`] on a body of `fill`: menu pages use [`PAGE_FILL`].
pub(crate) fn panel_filled(
    ctx: &egui::Context,
    p: Panel,
    fill: Color32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let id = egui::Id::unique(p.id);
    let mut open = p
        .collapsible
        .is_none_or(|default| ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(default));
    egui::Area::new(id)
        .anchor(p.anchor, p.offset)
        .show(ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(fill)
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
pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
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
pub(crate) fn brackets(painter: &egui::Painter, rect: Rect, len: f32, color: Color32) {
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
pub(crate) fn glow_label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) {
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
pub(crate) fn lamp(ui: &mut egui::Ui, text: &str, lit: bool, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), mono(13.0), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(12.0, 6.0), Sense::hover());
    crate::widgets::lamp_at(ui.painter(), rect.min, text, lit, color, None);
}

/// A small coloured tag, outlined.
pub(crate) fn tag(ui: &mut egui::Ui, text: &str, color: Color32) {
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
pub(crate) fn keycap(ui: &mut egui::Ui, key: &str) {
    let galley = ui.painter().layout_no_wrap(key.to_owned(), mono(13.0), INK);
    let (rect, _) =
        ui.allocate_exact_size(crate::widgets::keycap_size(galley.size()), Sense::hover());
    crate::widgets::paint_keycap(ui.painter(), rect, galley);
}

/// A segmented VFD bar for a value in -1..=1 (negative lights from the left
/// too, in the reverse colour), with quarter ticks underneath.
pub(crate) fn segment_bar(ui: &mut egui::Ui, value: f32, width: f32, color: Color32) {
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
/// The moment a gang pulls the ship out of fast travel: the view tears
/// with interference, the edges flash red, and a red band slams across with
/// INTERDICTED and who did it, then all of it fades.
fn interdicted(ctx: &egui::Context, screen: Rect, t: f32, gang: &str) {
    const SHOW: f32 = 3.2;
    if t > SHOW {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("interdicted"),
    ));
    let fade = (1.0 - ((t - SHOW + 0.8) / 0.8).clamp(0.0, 1.0)).min((t / 0.08).min(1.0));
    // Interference: torn scan bands, thick at first, thinning out.
    let tear = (1.0 - t / 1.2).clamp(0.0, 1.0);
    if tear > 0.0 {
        for i in 0..26 {
            let h = ((i as f32 * 12.9898 + (t * 37.0).floor() * 78.233).sin() * 43758.545).fract();
            let y = screen.top() + h.abs() * screen.height();
            let thick = 1.0 + 5.0 * tear * ((i * 7 % 5) as f32 / 4.0);
            let shift = (h - 0.5) * 60.0 * tear;
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(screen.left() + shift, y),
                    Vec2::new(screen.width(), thick),
                ),
                0.0,
                ALARM.gamma_multiply(0.12 * tear),
            );
        }
    }
    // A red flash round the edges.
    let flash = (1.0 - t / 0.6).clamp(0.0, 1.0);
    for k in 0..6 {
        painter.rect_stroke(
            screen.shrink(k as f32 * 14.0),
            0.0,
            Stroke::new(
                14.0,
                RED.gamma_multiply(0.22 * flash * (1.0 - k as f32 / 6.0)),
            ),
            egui::StrokeKind::Inside,
        );
    }
    // The band slams in from the left, then holds.
    let slam = (t / 0.18).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - slam).powi(3);
    let y = screen.top() + screen.height() * 0.24;
    let off = screen.width() * (1.0 - ease);
    let band = Rect::from_min_max(
        Pos2::new(screen.left() - off, y - 46.0),
        Pos2::new(screen.right() - off * 0.2, y + 46.0),
    );
    painter.rect_filled(band, 0.0, RED.gamma_multiply(0.92 * fade));
    painter.rect_filled(
        Rect::from_min_size(band.left_bottom(), Vec2::new(band.width(), 4.0)),
        0.0,
        RED_DARK.gamma_multiply(fade),
    );
    // Hazard slashes at the band's right end, as on the panel tabs.
    for k in 0..5 {
        let x = band.right() - 40.0 - k as f32 * 18.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x, band.bottom()),
                Pos2::new(x + 9.0, band.bottom()),
                Pos2::new(x + 25.0, band.top()),
                Pos2::new(x + 16.0, band.top()),
            ],
            RED_DARK.gamma_multiply(fade),
            Stroke::NONE,
        ));
    }
    let c = Pos2::new(screen.center().x - off, y);
    painter.text(
        c + Vec2::new(0.0, 8.0),
        Align2::CENTER_BOTTOM,
        "INTERDICTED",
        display(72.0),
        CREAM.gamma_multiply(fade),
    );
    painter.text(
        c + Vec2::new(0.0, 12.0),
        Align2::CENTER_TOP,
        format!("pulled out of fast travel by the {gang}").to_uppercase(),
        mono(18.0),
        CREAM.gamma_multiply(0.85 * fade),
    );
}

pub(crate) fn title_card(
    painter: &egui::Painter,
    centre: Pos2,
    title: &str,
    subtitle: &str,
    color: Color32,
) {
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
    project_any(view_proj, rel, screen).filter(|p| screen.contains(*p))
}

/// Where a point in front of the camera lands, on screen or off it.
fn project_any(view_proj: Mat4, rel: glam::Vec3, screen: egui::Rect) -> Option<Pos2> {
    let clip = view_proj * Vec4::new(rel.x, rel.y, rel.z, 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Pos2::new(
        screen.left() + (ndc.x + 1.0) * 0.5 * screen.width(),
        screen.top() + (1.0 - ndc.y) * 0.5 * screen.height(),
    ))
}

/// An arrow at the screen edge pointing toward an off-screen target.
fn edge_arrow(painter: &egui::Painter, view_proj: Mat4, rel: glam::Vec3, screen: egui::Rect) {
    edge_arrow_color(painter, view_proj, rel, screen, AMBER);
}

/// An arrow at the screen's edge pointing toward something off screen.
fn edge_arrow_color(
    painter: &egui::Painter,
    view_proj: Mat4,
    rel: glam::Vec3,
    screen: egui::Rect,
    color: Color32,
) {
    let dir = edge_arrow_dir(view_proj, rel);
    let c = screen.center();
    let r = screen.height().min(screen.width()) * 0.42;
    let tip = c + dir * r;
    let side = Vec2::new(-dir.y, dir.x) * 7.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, tip - dir * 14.0 + side, tip - dir * 14.0 - side],
        color,
        Stroke::NONE,
    ));
}

/// Which way, on screen (egui: +y down), to turn toward `rel`.
///
/// Clip-space x/y before the divide by w are the view-space right/up
/// offsets scaled by the lens, so they point toward the target whether
/// it's in front of the camera or behind it. Dividing by w (or flipping
/// for w < 0) is what turns a behind-the-camera arrow around, so don't.
/// A target dead astern has no preferred side; the arrow then points down.
fn edge_arrow_dir(view_proj: Mat4, rel: glam::Vec3) -> Vec2 {
    let clip = view_proj * Vec4::new(rel.x, rel.y, rel.z, 1.0);
    let dir = Vec2::new(clip.x, -clip.y);
    if dir.length_sq() > 1e-12 * clip.w.abs().max(1.0).powi(2) {
        dir.normalized()
    } else {
        Vec2::new(0.0, 1.0)
    }
}

/// Credits with thin-space thousands separators.
/// Fleets nearer than this get a label.
const TRAFFIC_LABEL_M: f64 = 2.0e9;

/// "KESSA HAULERS · 3× LIGHT TRADER · 120 T FOOD  42 KM"
fn diamond(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let pts = vec![
        c + Vec2::new(0.0, -r),
        c + Vec2::new(r, 0.0),
        c + Vec2::new(0.0, r),
        c + Vec2::new(-r, 0.0),
    ];
    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.2, color)));
}

fn pirate_label(c: &crate::traffic::PirateContact, dist: f64) -> String {
    let n = c.fits.len();
    let ships = if n == 1 {
        "1 SHIP".to_owned()
    } else {
        format!("{n} SHIPS")
    };
    let wanted = c
        .bounty
        .map(|b| format!(" · WANTED {}", credits(b).to_uppercase()))
        .unwrap_or_default();
    format!(
        "{} · {ships}{wanted}  {}",
        c.name.to_uppercase(),
        distance(dist)
    )
}

fn contact_label(c: &crate::traffic::Contact, dist: f64) -> String {
    let class = c.fit.hull().name.to_uppercase();
    let ships = if c.ships > 1 {
        format!("{}× {class}", c.ships)
    } else {
        class
    };
    let hold = match (&c.work, c.cargo) {
        // A harvester on its run: what it's doing rather than its hold.
        (Some(w), _) if w.at_source => {
            let doing = match w.good {
                Good::Gas => "SKIMMING GAS",
                _ => "CUTTING ICE",
            };
            let days = if w.days_left == 1 { "DAY" } else { "DAYS" };
            format!("{doing} · {} {days} LEFT", w.days_left)
        }
        (Some(w), _) => format!("BOUND FOR {}", w.source.to_uppercase()),
        (None, Some((good, t))) => format!("{:.0} T {}", t, good.name().to_uppercase()),
        (None, None) => "EMPTY".into(),
    };
    format!(
        "{} · {ships} · {hold}  {}",
        c.company.to_uppercase(),
        distance(dist)
    )
}

/// `n` of `thing`, with the plural `s` when it needs one.
pub fn count(n: usize, thing: &str) -> String {
    if n == 1 {
        format!("1 {thing}")
    } else {
        format!("{n} {thing}s")
    }
}

/// A count of hyperlane jumps, in words.
pub fn jumps_text(jumps: u32) -> String {
    match jumps {
        0 => "this system".into(),
        1 => "1 jump".into(),
        n => format!("{n} jumps"),
    }
}

pub fn credits(cr: f64) -> String {
    let n = cr.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{2009}');
        }
        out.push(c);
    }
    format!("{}{out} cr", if n < 0 { "-" } else { "" })
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

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// The game's lens, looking along `forward` with +y up.
    fn camera(forward: Vec3) -> Mat4 {
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            70f32.to_radians(),
            16.0 / 9.0,
            0.5,
        );
        let view = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, forward, Vec3::Y);
        proj * view
    }

    fn angle_deg(a: Vec2, b: Vec2) -> f32 {
        a.normalized()
            .dot(b.normalized())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    #[test]
    fn behind_and_below_points_down() {
        // The playtest case: 20 km off, 69° below the nose and behind.
        let vp = camera(Vec3::NEG_Z);
        let a = 111f32.to_radians();
        let rel = Vec3::new(0.0, -a.sin(), -a.cos()) * 20_000.0;
        assert!(rel.z > 0.0, "target is behind the camera");
        let d = edge_arrow_dir(vp, rel);
        assert!(angle_deg(d, Vec2::new(0.0, 1.0)) < 1.0, "{d:?}");
    }

    #[test]
    fn behind_points_the_short_way_round_on_every_side() {
        let vp = camera(Vec3::NEG_Z);
        // (view-space offset, screen direction): right, left, up, down.
        for (side, want) in [
            (Vec3::X, Vec2::new(1.0, 0.0)),
            (Vec3::NEG_X, Vec2::new(-1.0, 0.0)),
            (Vec3::Y, Vec2::new(0.0, -1.0)),
            (Vec3::NEG_Y, Vec2::new(0.0, 1.0)),
        ] {
            let rel = side * 3.0 + Vec3::Z * 10.0;
            let d = edge_arrow_dir(vp, rel);
            assert!(angle_deg(d, want) < 1.0, "{side:?} -> {d:?}");
        }
    }

    #[test]
    fn off_screen_in_front_points_at_it() {
        let vp = camera(Vec3::NEG_Z);
        // Well outside the 70° cone, ahead and up-left.
        let d = edge_arrow_dir(vp, Vec3::new(-10.0, 10.0, -1.0));
        assert!(d.x < 0.0 && d.y < 0.0, "{d:?}");
    }

    #[test]
    fn turned_camera_still_points_the_right_way() {
        // Facing +x: a target behind (-x) and above should point up.
        let vp = camera(Vec3::X);
        let d = edge_arrow_dir(vp, Vec3::new(-50.0, 20.0, 0.0));
        assert!(angle_deg(d, Vec2::new(0.0, -1.0)) < 1.0, "{d:?}");
    }

    #[test]
    fn dead_astern_still_gives_an_arrow() {
        let vp = camera(Vec3::NEG_Z);
        let d = edge_arrow_dir(vp, Vec3::Z * 1000.0);
        assert!(d.x.is_finite() && d.y.is_finite());
        assert!((d.length() - 1.0).abs() < 1e-4);
    }
}
