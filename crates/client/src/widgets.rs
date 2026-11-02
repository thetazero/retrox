//! Instrument-panel controls shared by every screen, so lists, buttons and
//! key legends look the same everywhere (DESIGN §8: painted panels, lamps,
//! keycaps, VFD readouts). Each is painted by hand rather than left to
//! egui's defaults.

use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

use crate::hud::{CREAM, CREAM_DIM, INK, RED, RED_DARK, RED_HOT, RULE, mono};

/// The dark plate behind cards and list rows.
pub(crate) const CARD_FILL: Color32 = Color32::from_rgba_premultiplied(24, 18, 28, 200);
/// A plain button's face.
const FACE: Color32 = Color32::from_rgb(30, 24, 34);

/// How a list row is lit, for the caller to colour what it paints on it.
#[derive(Clone, Copy)]
pub(crate) struct RowLook {
    pub hot: bool,
    pub selected: bool,
    pub enabled: bool,
}

impl RowLook {
    /// Main text: amber when selected, cream otherwise, rule when disabled.
    pub fn text(self, selected: Color32) -> Color32 {
        if !self.enabled {
            RULE
        } else if self.selected {
            selected
        } else {
            CREAM
        }
    }
}

/// A clickable row in a list: a hairline under it, a red wash and tab when
/// hovered, an amber wash and tab when selected. The caller paints the
/// contents into the returned rect.
pub(crate) fn list_row(
    ui: &mut egui::Ui,
    height: f32,
    selected: bool,
    enabled: bool,
    accent: Color32,
) -> (Rect, Response, RowLook) {
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    let hot = enabled && response.hovered();
    let painter = ui.painter();
    let tab = Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
    if hot {
        painter.rect_filled(rect, 0.0, RED_DARK.gamma_multiply(0.85));
        painter.rect_filled(tab, 0.0, if selected { accent } else { CREAM });
    } else if selected {
        painter.rect_filled(rect, 0.0, accent.gamma_multiply(0.12));
        painter.rect_filled(tab, 0.0, accent);
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, RULE.gamma_multiply(0.45)),
    );
    let look = RowLook {
        hot,
        selected,
        enabled,
    };
    (
        rect,
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        },
        look,
    )
}

/// What a push button looks like.
#[derive(Clone, Copy)]
pub(crate) enum Look {
    /// Signal red: the one thing to do here.
    Primary,
    /// A dark enamel key.
    Plain,
    /// Outlined in a colour (teal to sell, amber to buy...).
    Tint(Color32),
}

/// A hardware push button: a face, a darker lip along the bottom, and its
/// legend; it sinks a pixel while pressed.
pub(crate) fn push(
    ui: &mut egui::Ui,
    text: &str,
    font: FontId,
    look: Look,
    enabled: bool,
    min: Vec2,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let size = (galley.size() + Vec2::new(22.0, 10.0)).max(min);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    paint_push(ui.painter(), rect, &galley, look, enabled, &response);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn paint_push(
    painter: &egui::Painter,
    rect: Rect,
    galley: &std::sync::Arc<egui::Galley>,
    look: Look,
    enabled: bool,
    response: &Response,
) {
    let hot = enabled && response.hovered();
    let down = enabled && response.is_pointer_button_down_on();
    let (fill, stroke, text, lip) = if !enabled {
        (
            Color32::TRANSPARENT,
            RULE.gamma_multiply(0.6),
            RULE,
            Color32::TRANSPARENT,
        )
    } else {
        match look {
            Look::Primary => (
                if down {
                    RED_DARK
                } else if hot {
                    RED_HOT
                } else {
                    RED
                },
                if hot { CREAM } else { RED },
                CREAM,
                RED_DARK,
            ),
            Look::Plain => (
                if hot { RED_DARK } else { FACE },
                if hot { CREAM } else { RULE },
                CREAM,
                INK,
            ),
            Look::Tint(c) => (
                c.gamma_multiply(if hot { 0.28 } else { 0.07 }),
                c.gamma_multiply(if hot { 0.9 } else { 0.5 }),
                c,
                c.gamma_multiply(0.25),
            ),
        }
    };
    let face = if down {
        rect.translate(Vec2::new(0.0, 1.0))
    } else {
        rect
    };
    painter.rect_filled(face, 0.0, fill);
    painter.rect_stroke(face, 0.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    if !down && lip != Color32::TRANSPARENT {
        painter.line_segment(
            [
                rect.left_bottom() + Vec2::new(1.0, -1.0),
                rect.right_bottom() + Vec2::new(-1.0, -1.0),
            ],
            Stroke::new(2.0, lip),
        );
    }
    painter.galley_with_override_text_color(
        face.center() - galley.size() / 2.0 + Vec2::new(0.0, 1.0),
        galley.clone(),
        text,
    );
}

/// A bank of joined keys across `rect`, like a row of switches on a panel,
/// in one colour. Returns the index of the key clicked.
pub(crate) fn bank(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    keys: &[(&str, bool)],
    color: Color32,
) -> Option<usize> {
    let n = keys.len().max(1) as f32;
    let w = rect.width() / n;
    let mut clicked = None;
    let any_enabled = keys.iter().any(|k| k.1);
    let frame = if any_enabled {
        color.gamma_multiply(0.5)
    } else {
        RULE.gamma_multiply(0.6)
    };
    for (i, &(label, enabled)) in keys.iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(rect.left() + i as f32 * w, rect.top()),
            Vec2::new(w, rect.height()),
        );
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(r, id.with(i), sense);
        let hot = enabled && response.hovered();
        let painter = ui.painter();
        if hot {
            painter.rect_filled(r, 0.0, color.gamma_multiply(0.3));
        } else if enabled {
            painter.rect_filled(r, 0.0, color.gamma_multiply(0.06));
        }
        if i > 0 {
            painter.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, frame));
        }
        let text = if !enabled {
            RULE
        } else if hot {
            CREAM
        } else {
            color
        };
        painter.text(r.center(), Align2::CENTER_CENTER, label, mono(13.0), text);
        if response.clicked() {
            clicked = Some(i);
        }
        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    }
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0, frame), StrokeKind::Inside);
    clicked
}

/// Draws one keycap with its label after it, starting at `at` (the keycap's
/// left edge, vertically centred), and returns where the next should start.
pub(crate) fn legend_item(painter: &egui::Painter, at: Pos2, key: &str, what: &str) -> f32 {
    let galley = painter.layout_no_wrap(key.to_owned(), mono(12.0), INK);
    let size = Vec2::new((galley.size().x + 10.0).max(20.0), galley.size().y + 4.0);
    let cap = Rect::from_min_size(Pos2::new(at.x, at.y - size.y / 2.0), size);
    painter.rect_filled(cap, 2.0, CREAM);
    painter.line_segment(
        [cap.left_bottom(), cap.right_bottom()],
        Stroke::new(2.0, CREAM_DIM),
    );
    painter.galley(cap.center() - galley.size() / 2.0, galley, INK);
    let r = painter.text(
        Pos2::new(cap.right() + 6.0, cap.center().y),
        Align2::LEFT_CENTER,
        what.to_uppercase(),
        mono(12.0),
        CREAM_DIM,
    );
    r.right() + 18.0
}
