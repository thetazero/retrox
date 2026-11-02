//! New-pilot help on the HUD (issue #79): the first-steps notes, the strip
//! of keys in the corner, and the controls card on H.

use egui::{Align2, Color32, Margin, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::{
    AMBER, CREAM, CREAM_DIM, INK, Panel, RULE, VFD, VFD_DIM, brackets, keycap, mono, panel,
    section, title_band,
};
use crate::guide::{self, KeyRow, Seg};
use crate::world::{Mode, World};

const NOTES_W: f32 = 380.0;

/// Text with `[K]` keys drawn as keycaps, wrapping at the width given.
pub(crate) fn rich_hint(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 3.0);
        for seg in guide::segments(text) {
            match seg {
                Seg::Key(k) => {
                    ui.add_space(1.0);
                    keycap(ui, k);
                    ui.add_space(1.0);
                }
                // Word by word, so the line can wrap between them.
                Seg::Text(t) => {
                    for (i, word) in t.split(' ').enumerate() {
                        if i > 0 {
                            ui.add_space(5.0);
                        }
                        if !word.is_empty() {
                            ui.label(RichText::new(word).font(mono(13.0)).color(color));
                        }
                    }
                }
            }
        }
    });
}

/// The first-steps notes: a lamp for each first, and a hint for what to
/// do next. Once every lamp is lit the lamps go and the hint stays, on
/// its own (issue #127); the setting turns it all off.
pub fn notes(ctx: &egui::Context, w: &World) {
    let progress = &w.player.progress;
    let complete = progress.complete();
    let hint = guide::hint(w);
    if complete && hint.is_none() {
        return;
    }
    let docked = w.docked_at().is_some();
    if !(docked || matches!(w.mode, Mode::Normal | Mode::FastTravel(_))) {
        return;
    }
    // Beside the station menu when docked; above the key strip in flight.
    let (anchor, offset) = if docked {
        (Align2::RIGHT_BOTTOM, [-20.0, -46.0])
    } else {
        (Align2::LEFT_BOTTOM, [20.0, -46.0])
    };
    let steps = progress.steps();
    let next = steps.iter().position(|s| !s.1);
    let done = steps.iter().filter(|s| s.1).count();
    let title = if complete {
        "Pilot's notes".to_owned()
    } else {
        format!("First steps · {done}/{}", steps.len())
    };
    panel(
        ctx,
        Panel {
            id: "first-steps",
            title: &title,
            anchor,
            offset,
            width: NOTES_W,
            collapsible: Some(true),
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            // Two columns of lamps, until they're all lit.
            let half = steps.len().div_ceil(2);
            if !complete {
                ui.horizontal_top(|ui| {
                    for column in [&steps[..half], &steps[half..]] {
                        ui.vertical(|ui| {
                            ui.set_width((NOTES_W - 24.0) / 2.0 - 4.0);
                            for (title, lit) in column {
                                let is_next = next.is_some_and(|n| steps[n].0 == *title);
                                step_row(ui, title, *lit, is_next);
                            }
                        });
                    }
                });
            }
            if let Some(text) = hint {
                if !complete {
                    ui.add_space(8.0);
                }
                let frame = egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(30, 22, 10, 160))
                    .inner_margin(Margin {
                        left: 12,
                        right: 8,
                        top: 6,
                        bottom: 7,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        rich_hint(ui, &text, CREAM);
                    });
                let r = frame.response.rect;
                ui.painter().rect_filled(
                    Rect::from_min_size(r.min, Vec2::new(3.0, r.height())),
                    0.0,
                    AMBER,
                );
            }
        },
    );
}

/// A square lamp and the step's name: teal and ticked when done, amber
/// when it's the next one.
fn step_row(ui: &mut egui::Ui, title: &str, lit: bool, next: bool) {
    let height = 20.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    let lamp = Rect::from_center_size(
        Pos2::new(rect.left() + 7.0, rect.center().y),
        Vec2::splat(11.0),
    );
    if lit {
        painter.rect_filled(lamp.expand(2.0), 0.0, VFD.gamma_multiply(0.2));
        painter.rect_filled(lamp, 0.0, VFD);
        // A tick, in ink.
        painter.line(
            vec![
                lamp.left_center() + Vec2::new(2.0, 0.0),
                lamp.center_bottom() + Vec2::new(-1.0, -2.5),
                lamp.right_top() + Vec2::new(-2.0, 2.0),
            ],
            Stroke::new(1.8, INK),
        );
    } else if next {
        painter.rect_stroke(lamp, 0.0, Stroke::new(1.5, AMBER), egui::StrokeKind::Inside);
        painter.rect_filled(lamp.shrink(3.5), 0.0, AMBER);
    } else {
        painter.rect_filled(lamp, 0.0, VFD_DIM);
    }
    let color = match (lit, next) {
        (true, _) => VFD,
        (false, true) => AMBER,
        _ => CREAM_DIM,
    };
    painter.text(
        Pos2::new(lamp.right() + 9.0, rect.center().y),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        mono(13.0),
        color,
    );
}

/// Keys along the bottom-left corner: how to get help and the menu, and
/// how to take the mouse when it's free.
pub fn key_strip(ctx: &egui::Context, w: &World, captured: bool) {
    let flying = w.mode == Mode::Normal && !w.map_open;
    let mut keys: Vec<(&str, &str)> = vec![("H", "controls"), ("ESC", "menu")];
    if flying && !captured {
        keys.push(("TAB", "fly with the mouse"));
    }
    egui::Area::new(egui::Id::unique("key-strip"))
        .anchor(Align2::LEFT_BOTTOM, [14.0, -10.0])
        .interactable(false)
        .show(ctx, |ui| {
            // A dark plate: the strip sits over the planet you launch from
            // (issue #124).
            egui::Frame::new()
                .fill(crate::widgets::BACKING)
                .inner_margin(egui::Margin {
                    left: 6,
                    right: 0,
                    top: 4,
                    bottom: 4,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (key, what) in keys {
                            keycap(ui, key);
                            ui.label(
                                RichText::new(what.to_uppercase())
                                    .font(mono(12.0))
                                    .color(CREAM_DIM),
                            );
                            ui.add_space(10.0);
                        }
                    })
                });
        });
}

/// The controls card: every key, in three columns, over the view.
pub fn controls_card(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let shade = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique("controls-shade"),
    ));
    shade.rect_filled(screen, 0.0, INK.gamma_multiply(0.45));
    let col_w = 330.0;
    let width = col_w * 3.0 + 2.0 * 28.0 + 32.0;
    egui::Area::new(egui::Id::unique("controls-card"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .interactable(false)
        .show(ctx, |ui| {
            // Opaque: the card sits over other panels.
            let frame = egui::Frame::new()
                .fill(Color32::from_rgb(11, 9, 16))
                .stroke(Stroke::new(1.0, RULE))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // Room for the title band, painted once the card's
                    // width is known.
                    ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 16,
                            right: 16,
                            top: 14,
                            bottom: 12,
                        })
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 28.0;
                                for (title, rows) in [
                                    ("Flight", guide::FLIGHT_KEYS),
                                    ("Travel", guide::TRAVEL_KEYS),
                                    ("Work & view", guide::WORK_KEYS),
                                ] {
                                    ui.vertical(|ui| {
                                        ui.set_width(col_w);
                                        key_table(ui, title, rows);
                                    });
                                }
                            });
                            ui.add_space(12.0);
                            ui.painter().line_segment(
                                [
                                    ui.cursor().left_top(),
                                    ui.cursor().left_top() + Vec2::new(width - 32.0, 0.0),
                                ],
                                Stroke::new(1.0, RULE),
                            );
                            ui.add_space(8.0);
                            rich_hint(
                                ui,
                                "Click a name in NAVIGATION to target it. Jump with the JUMP \
                                 buttons in HYPERLANES, clear of gravity wells.  [H] closes this \
                                 card; the HANDBOOK in the [ESC] menu explains how to start.",
                                CREAM_DIM,
                            );
                        });
                });
            let card = frame.response.rect;
            title_band(
                ui.painter(),
                Rect::from_min_size(card.min, Vec2::new(card.width(), 36.0)),
                "Controls",
                None,
                false,
            );
            brackets(ui.painter(), card.expand(4.0), 14.0, CREAM_DIM);
        });
}

/// A titled table of keys and what they do.
pub(crate) fn key_table(ui: &mut egui::Ui, title: &str, rows: &[KeyRow]) {
    section(ui, title);
    ui.add_space(6.0);
    for (keys, what) in rows {
        ui.horizontal(|ui| {
            ui.set_min_height(23.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            // Keys in a fixed-width gutter so the descriptions line up.
            let gutter = 100.0;
            let start = ui.cursor().left();
            for k in *keys {
                keycap(ui, k);
            }
            let used = ui.cursor().left() - start;
            ui.add_space((gutter - used).max(6.0));
            ui.label(RichText::new(*what).font(mono(13.0)).color(CREAM));
        });
    }
}
