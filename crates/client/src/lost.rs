//! Losing the ship (DESIGN §7.7): the card over the wreck, the rewind to
//! the last checkpoint with the galaxy's date counter rolling back, and the
//! black lifting off the dock where the checkpoint was written.
//!
//! The look (DESIGN §8): a red constructivist band slammed across the
//! view, a panel of readouts (alarm red for the loss, teal for the
//! checkpoint, amber for the time it costs), then a reel-to-reel transport
//! rewinding over film running backwards through the gate.

use std::f32::consts::TAU;
use std::time::Instant;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use retrox_sim::time::DAYS_PER_YEAR;

use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, INK, PANEL, RED, RED_DARK, RULE, VFD, display, drum, mono,
};
use crate::widgets::{self, Look};
use crate::world::{SECONDS_PER_DAY, World};

/// Seconds after the blast before the card slams in.
pub const CARD_AT: f64 = 1.6;
/// ...and before Enter restores, so a held key doesn't skip the moment.
pub const READY_AT: f64 = 2.1;
/// The rewind runs at least this long, real seconds, however fast the
/// checkpoint loads.
pub const REWIND_S: f32 = 2.8;
/// How long the arrival takes: black lifting, then the note fading.
pub const ARRIVE_S: f32 = 3.4;

/// The checkpoint the card offers.
pub struct Checkpoint {
    pub place: String,
    pub date: String,
    pub clock_s: Option<f64>,
}

/// What the card says.
pub struct LostInfo {
    pub ship: String,
    pub hull: &'static str,
    pub place: String,
    pub date: String,
    pub by_raiders: bool,
    pub clock_s: f64,
    pub checkpoint: Option<Checkpoint>,
}

impl LostInfo {
    pub fn new(world: &World, checkpoint: Option<Checkpoint>) -> Self {
        let place = crate::save::place_text(world);
        Self {
            ship: world.player.ship.name.clone(),
            hull: world.player.ship.fit.hull().name,
            place: place.strip_prefix("in space ").unwrap_or(&place).to_owned(),
            date: world.date().to_string(),
            by_raiders: !world.combat.raiders.is_empty(),
            clock_s: world.clock_s,
            checkpoint,
        }
    }
}

pub enum Choice {
    /// Back to the checkpoint (or the title, without one).
    Restore,
    Menu,
}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A cheap hash to [0, 1): film grain and scratches.
fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 13;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// "3 d 04 h", "11 h", "40 min": how much the restore takes back.
pub fn earlier(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let days = (s / SECONDS_PER_DAY).floor();
    let hours = ((s - days * SECONDS_PER_DAY) / 3600.0).floor();
    if days >= 1.0 {
        format!("{days:.0} d {hours:02.0} h earlier")
    } else if hours >= 1.0 {
        format!("{hours:.0} h earlier")
    } else {
        format!("{:.0} min earlier", (s / 60.0).max(1.0))
    }
}

/// The card over the wreck, `t` seconds after the ship was lost.
pub fn card(ctx: &egui::Context, info: &LostInfo, t: f64) -> Option<Choice> {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::unique("ship-lost"),
    ));
    // The view darkens and the colour drains out of it, edges first.
    let dark = ease_in_out(((t - 0.6) / 2.0) as f32);
    vignette(&painter, screen, dark);
    if t < CARD_AT {
        return None;
    }
    let k = ease_out(((t - CARD_AT) / 0.32) as f32);
    let fade = ease_out(((t - CARD_AT - 0.25) / 0.5) as f32);

    // The band: slammed in from the left, slanting up to the right.
    let mid = screen.center().y - 120.0;
    let h = 118.0;
    let skew = 34.0;
    let dx = -(1.0 - k) * screen.width() * 1.2;
    let band = [
        Pos2::new(screen.left() - 60.0 + dx, mid - h / 2.0 + skew),
        Pos2::new(screen.right() + 60.0 + dx, mid - h / 2.0 - skew),
        Pos2::new(screen.right() + 60.0 + dx, mid + h / 2.0 - skew),
        Pos2::new(screen.left() - 60.0 + dx, mid + h / 2.0 + skew),
    ];
    // A dark shadow band under it, offset, and a cream rule: the
    // constructivist layering of the title screen.
    let shadow: Vec<Pos2> = band.iter().map(|p| *p + Vec2::new(14.0, 16.0)).collect();
    painter.add(Shape::convex_polygon(shadow, RED_DARK, Stroke::NONE));
    painter.add(Shape::convex_polygon(band.to_vec(), RED, Stroke::NONE));
    let rule_y = |x: f32, off: f32| {
        mid + h / 2.0 + off - skew * 2.0 * (x - screen.left()) / screen.width() + skew
    };
    painter.line_segment(
        [
            Pos2::new(screen.left() + dx, rule_y(screen.left(), 8.0)),
            Pos2::new(screen.right() + dx, rule_y(screen.right(), 8.0)),
        ],
        Stroke::new(2.0, CREAM.gamma_multiply(0.85)),
    );
    let c = Pos2::new(screen.center().x + dx, mid);
    let title = painter.text(
        c + Vec2::new(0.0, 6.0),
        Align2::CENTER_CENTER,
        "SHIP LOST",
        display(112.0),
        CREAM,
    );
    // The ship's plate, riveted to the band beside the title. The name
    // alone when it already says what the hull is ("Courier no. 1").
    let name = widgets::truncate(&info.ship.to_uppercase(), 26);
    let plate_text = if name.contains(&info.hull.to_uppercase()) {
        name
    } else {
        format!("{} · {name}", info.hull.to_uppercase())
    };
    let g = painter.layout_no_wrap(plate_text, mono(15.0), INK);
    let x = title.right() + 22.0;
    // The band's centre line where the plate sits, then a little below it.
    let y = mid - 2.0 * skew * (x - screen.center().x - dx) / (screen.width() + 120.0) + 26.0;
    let plate = Rect::from_min_size(Pos2::new(x, y - 13.0), g.size() + Vec2::new(18.0, 8.0));
    painter.rect_filled(plate.translate(Vec2::new(3.0, 3.0)), 0.0, RED_DARK);
    painter.rect_filled(plate, 0.0, CREAM);
    for corner in [
        plate.left_top(),
        plate.right_top(),
        plate.left_bottom(),
        plate.right_bottom(),
    ] {
        let inset = Vec2::new(
            if corner.x < plate.center().x {
                4.0
            } else {
                -4.0
            },
            if corner.y < plate.center().y {
                4.0
            } else {
                -4.0
            },
        );
        painter.circle_filled(corner + inset, 1.3, CREAM_DIM);
    }
    painter.galley(plate.center() - g.size() / 2.0, g, INK);

    // The readouts.
    let a = fade;
    let w = 660.0;
    let panel = Rect::from_min_size(
        Pos2::new(
            screen.center().x - w / 2.0,
            mid + h / 2.0 + skew + 34.0 + (1.0 - a) * 16.0,
        ),
        Vec2::new(w, 176.0),
    );
    // The readouts above, a footer line under them.
    let footer = panel.top() + 132.0;
    painter.rect_filled(panel, 0.0, PANEL.gamma_multiply(a));
    painter.rect_stroke(
        panel,
        0.0,
        Stroke::new(1.0, RULE.gamma_multiply(a)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(panel.left_top(), Vec2::new(4.0, panel.height())),
        0.0,
        ALARM.gamma_multiply(a),
    );
    let col = |i: f32| panel.left() + 28.0 + i * w / 2.0;
    painter.line_segment(
        [
            Pos2::new(panel.center().x, panel.top() + 18.0),
            Pos2::new(panel.center().x, footer - 18.0),
        ],
        Stroke::new(1.0, RULE.gamma_multiply(a)),
    );
    painter.line_segment(
        [
            Pos2::new(panel.left() + 18.0, footer),
            Pos2::new(panel.right() - 18.0, footer),
        ],
        Stroke::new(1.0, RULE.gamma_multiply(0.6 * a)),
    );
    let heading = |x: f32, text: &str, lamp: Color32| {
        let y = panel.top() + 22.0;
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(x + 4.0, y), Vec2::splat(8.0)),
            0.0,
            lamp.gamma_multiply(a),
        );
        painter.text(
            Pos2::new(x + 16.0, y),
            Align2::LEFT_CENTER,
            text,
            mono(12.0),
            CREAM_DIM.gamma_multiply(a),
        );
    };
    let lines =
        |x: f32, date: &str, date_color: Color32, place: &str, note: &str, note_color: Color32| {
            painter.text(
                Pos2::new(x, panel.top() + 38.0),
                Align2::LEFT_TOP,
                date,
                mono(28.0),
                date_color.gamma_multiply(a),
            );
            painter.text(
                Pos2::new(x, panel.top() + 76.0),
                Align2::LEFT_TOP,
                widgets::truncate(&place.to_uppercase(), 34),
                mono(14.0),
                CREAM.gamma_multiply(a),
            );
            painter.text(
                Pos2::new(x, panel.top() + 98.0),
                Align2::LEFT_TOP,
                note.to_uppercase(),
                mono(13.0),
                note_color.gamma_multiply(a),
            );
        };
    heading(col(0.0), "DESTROYED", ALARM);
    lines(
        col(0.0),
        &info.date,
        ALARM,
        &info.place,
        if info.by_raiders { "by raiders" } else { "" },
        CREAM_DIM,
    );
    heading(col(1.0), "LAST CHECKPOINT", VFD);
    match &info.checkpoint {
        Some(cp) => {
            let ago = cp
                .clock_s
                .map_or_else(String::new, |s| earlier(info.clock_s - s));
            lines(col(1.0), &cp.date, VFD, &cp.place, &ago, AMBER);
        }
        None => lines(
            col(1.0),
            "NONE",
            RULE,
            "nothing to go back to",
            "",
            CREAM_DIM,
        ),
    }
    let note = if info.checkpoint.is_some() {
        "The galaxy goes back to how it was then. What you learned, you keep."
    } else {
        "This game has no checkpoint: dock or land to write one."
    };
    painter.text(
        Pos2::new(panel.center().x, (footer + panel.bottom()) / 2.0),
        Align2::CENTER_CENTER,
        note,
        mono(14.0),
        CREAM_DIM.gamma_multiply(a),
    );

    // The keys.
    if t < READY_AT - 0.2 {
        return None;
    }
    let keys_a = ease_out(((t - READY_AT + 0.2) / 0.3) as f32);
    // A dark plate under the keys, so they read over the fireball.
    let plate = Rect::from_center_size(
        Pos2::new(screen.center().x, panel.bottom() + 62.0),
        Vec2::new(440.0, 92.0),
    );
    painter.rect_filled(
        plate,
        0.0,
        Color32::from_black_alpha((170.0 * keys_a) as u8),
    );
    let mut choice = None;
    egui::Area::new(egui::Id::unique("ship-lost-keys"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(Pos2::new(screen.center().x, panel.bottom() + 30.0))
        .show(ctx, |ui| {
            ui.set_opacity(keys_a);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let restore = if info.checkpoint.is_some() {
                    "RESTORE CHECKPOINT"
                } else {
                    "TITLE SCREEN"
                };
                if widgets::push(
                    ui,
                    restore,
                    mono(17.0),
                    Look::Primary,
                    true,
                    Vec2::new(250.0, 40.0),
                )
                .clicked()
                {
                    choice = Some(Choice::Restore);
                }
                if widgets::push(
                    ui,
                    "MENU",
                    mono(17.0),
                    Look::Plain,
                    true,
                    Vec2::new(120.0, 40.0),
                )
                .clicked()
                {
                    choice = Some(Choice::Menu);
                }
            });
            ui.add_space(12.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(384.0, 20.0), egui::Sense::hover());
            let p = ui.painter();
            let x = widgets::legend_item(
                p,
                Pos2::new(r.left() + 70.0, r.center().y),
                "ENTER",
                if info.checkpoint.is_some() {
                    "restore"
                } else {
                    "title"
                },
            );
            widgets::legend_item(p, Pos2::new(x, r.center().y), "ESC", "menu");
        });
    choice
}

/// The view's colour drained toward the edges: black from the rim in, a
/// wash of soot over the whole.
fn vignette(painter: &egui::Painter, screen: Rect, k: f32) {
    if k <= 0.0 {
        return;
    }
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha((110.0 * k) as u8));
    let depth = screen.height() * 0.32;
    let outer = Color32::from_black_alpha((200.0 * k) as u8);
    let o = [
        screen.left_top(),
        screen.right_top(),
        screen.right_bottom(),
        screen.left_bottom(),
    ];
    let i = [
        screen.left_top() + Vec2::new(depth, depth * 0.7),
        screen.right_top() + Vec2::new(-depth, depth * 0.7),
        screen.right_bottom() + Vec2::new(-depth, -depth * 0.7),
        screen.left_bottom() + Vec2::new(depth, -depth * 0.7),
    ];
    let mut mesh = egui::Mesh::default();
    for k in 0..4 {
        let n = (k + 1) % 4;
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(o[k], outer);
        mesh.colored_vertex(o[n], outer);
        mesh.colored_vertex(i[n], Color32::TRANSPARENT);
        mesh.colored_vertex(i[k], Color32::TRANSPARENT);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(Shape::mesh(mesh));
}

/// The rewind to a checkpoint, under way.
pub struct Rewind {
    pub started: Instant,
    /// Galaxy seconds when the ship was lost, and at the checkpoint.
    pub from_s: f64,
    pub to_s: f64,
    pub place: String,
    pub date: String,
}

impl Rewind {
    pub fn elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }
}

/// The share of the way back the counter has rolled.
fn progress(e: f32) -> f32 {
    ease_in_out((e - 0.25) / (REWIND_S - 0.6))
}

/// The film running backwards through the gate, and the tape transport.
pub fn rewind(ctx: &egui::Context, r: &Rewind) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("rewind"),
    ));
    let e = r.elapsed();
    let frame = (e * 24.0) as u32;
    let p = progress(e);
    // Over the frozen view: dark, flickering, going to black.
    let flicker = 0.06 * hash(frame, 7);
    let cover = (0.72 + 0.28 * ease_in_out(e / REWIND_S) - flicker).clamp(0.0, 1.0);
    painter.rect_filled(
        screen,
        0.0,
        Color32::from_black_alpha((255.0 * cover) as u8),
    );
    // The projector's lamp through the running film: a warm, unsteady wash
    // the frame lines and scratches show against.
    let lamp_k = (0.05 + 0.03 * hash(frame, 11)) * (1.0 - ease_in_out((e - REWIND_S + 0.5) / 0.5));
    painter.rect_filled(
        screen,
        0.0,
        Color32::from_rgb(255, 214, 160).gamma_multiply(lamp_k),
    );

    // Film strip: frame lines and sprocket holes rolling down the screen
    // (backwards), streaked with scratches and grain.
    let pitch = screen.height() * 0.62;
    let travel = film_travel(e) * pitch * 9.0;
    let line_off = travel % pitch;
    for k in -1..3 {
        let y = screen.top() + line_off + k as f32 * pitch;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(screen.left(), y), Vec2::new(screen.width(), 18.0)),
            0.0,
            Color32::from_black_alpha(230),
        );
        painter.line_segment(
            [
                Pos2::new(screen.left(), y + 18.0),
                Pos2::new(screen.right(), y + 18.0),
            ],
            Stroke::new(1.0, CREAM_DIM.gamma_multiply(0.12)),
        );
    }
    let gutter = 46.0;
    for side in [screen.left(), screen.right() - gutter] {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(side, screen.top()),
                Vec2::new(gutter, screen.height()),
            ),
            0.0,
            Color32::from_black_alpha(200),
        );
        let hole = 36.0;
        let off = travel % hole;
        let mut y = screen.top() - hole + off;
        while y < screen.bottom() {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(side + 15.0, y), Vec2::new(16.0, 20.0)),
                3.0,
                CREAM_DIM.gamma_multiply(0.16),
            );
            y += hole;
        }
    }
    for i in 0..3 {
        if hash(frame, 100 + i) < 0.55 {
            let x = screen.left() + gutter + hash(frame, 200 + i) * (screen.width() - 2.0 * gutter);
            painter.line_segment(
                [
                    Pos2::new(x, screen.top()),
                    Pos2::new(x + 6.0 * (hash(frame, 300 + i) - 0.5), screen.bottom()),
                ],
                Stroke::new(
                    1.0,
                    CREAM.gamma_multiply(0.07 + 0.08 * hash(frame, 400 + i)),
                ),
            );
        }
    }
    for i in 0..220 {
        let x = screen.left() + hash(frame, 1000 + i) * screen.width();
        let y = screen.top() + hash(frame, 5000 + i) * screen.height();
        let a = 0.05 + 0.12 * hash(frame, 9000 + i);
        painter.circle_filled(
            Pos2::new(x, y),
            0.6 + hash(i, frame) * 0.9,
            CREAM.gamma_multiply(a),
        );
    }

    // The transport: a panel with two reels and the date counter.
    let size = Vec2::new(640.0, 260.0);
    let panel = Rect::from_center_size(screen.center(), size);
    painter.rect_filled(
        panel.translate(Vec2::new(10.0, 12.0)),
        0.0,
        Color32::from_black_alpha(160),
    );
    painter.rect_filled(panel, 0.0, Color32::from_rgb(26, 21, 30));
    painter.rect_stroke(panel, 0.0, Stroke::new(1.0, RULE), StrokeKind::Inside);
    let head = Rect::from_min_size(panel.min, Vec2::new(size.x, 40.0));
    painter.rect_filled(head, 0.0, RED);
    painter.text(
        head.left_center() + Vec2::new(18.0, 2.0),
        Align2::LEFT_CENTER,
        "REWIND",
        display(34.0),
        CREAM,
    );
    painter.text(
        head.right_center() - Vec2::new(18.0, 0.0),
        Align2::RIGHT_CENTER,
        "TO THE LAST CHECKPOINT",
        mono(13.0),
        CREAM,
    );

    // Reels: the left takes the tape up as it rewinds.
    let reel_y = panel.top() + 140.0;
    let reel_r = 64.0;
    let left = Pos2::new(panel.left() + 96.0, reel_y);
    let right = Pos2::new(panel.right() - 96.0, reel_y);
    let spin = -reel_angle(e);
    let pack_l = 26.0 + 32.0 * p;
    let pack_r = 26.0 + 32.0 * (1.0 - p);
    // Tape between them, under two guide rollers.
    let guide_l = Pos2::new(panel.center().x - 120.0, panel.bottom() - 26.0);
    let guide_r = Pos2::new(panel.center().x + 120.0, panel.bottom() - 26.0);
    let tape = Color32::from_rgb(70, 50, 38);
    painter.line_segment(
        [left + Vec2::new(-pack_l * 0.2, pack_l), guide_l],
        Stroke::new(2.0, tape),
    );
    painter.line_segment([guide_l, guide_r], Stroke::new(2.0, tape));
    painter.line_segment(
        [guide_r, right + Vec2::new(pack_r * 0.2, pack_r)],
        Stroke::new(2.0, tape),
    );
    for g in [guide_l, guide_r] {
        painter.circle_filled(g, 5.0, CREAM_DIM);
    }
    // The head between the rollers.
    painter.rect_filled(
        Rect::from_center_size(
            Pos2::new(panel.center().x, guide_l.y - 6.0),
            Vec2::new(30.0, 12.0),
        ),
        2.0,
        RULE,
    );
    for (c, pack, phase) in [(left, pack_l, 0.0), (right, pack_r, 0.6)] {
        reel(&painter, c, reel_r, pack, spin * (58.0 / pack) + phase);
    }

    // The counter: the galaxy's date and hour rolling back.
    let clock = r.from_s + (r.to_s - r.from_s) * f64::from(p);
    let days = clock / SECONDS_PER_DAY;
    let year = (days / DAYS_PER_YEAR as f64).floor();
    let doy = days - year * DAYS_PER_YEAR as f64;
    let hours = days.fract() * 24.0;
    let cy = panel.top() + 72.0;
    let digit = 17.0;
    let width = digit * 8.0 + 46.0;
    let mut x = panel.center().x - width / 2.0;
    painter.text(
        Pos2::new(x - 6.0, cy + 12.0),
        Align2::RIGHT_CENTER,
        "Y",
        mono(16.0),
        CREAM_DIM,
    );
    x = drum(&painter, Pos2::new(x, cy), year, 3, false).right() + 8.0;
    painter.text(
        Pos2::new(x + 3.0, cy + 12.0),
        Align2::CENTER_CENTER,
        ".",
        mono(19.0),
        CREAM_DIM,
    );
    x = drum(&painter, Pos2::new(x + 10.0, cy), doy, 3, false).right() + 16.0;
    x = drum(&painter, Pos2::new(x, cy), hours, 2, false).right() + 6.0;
    painter.text(
        Pos2::new(x, cy + 12.0),
        Align2::LEFT_CENTER,
        "H",
        mono(16.0),
        CREAM_DIM,
    );

    // Where it's going, and the transport's lamps.
    painter.text(
        Pos2::new(panel.center().x, cy + 40.0),
        Align2::CENTER_TOP,
        widgets::truncate(&r.place.to_uppercase(), 36),
        mono(14.0),
        CREAM,
    );
    painter.text(
        Pos2::new(panel.center().x, cy + 60.0),
        Align2::CENTER_TOP,
        &r.date,
        mono(14.0),
        VFD,
    );
    let rewinding = e < REWIND_S - 0.15;
    let blink = (e * 3.0).fract() < 0.6;
    widgets::lamp_at(
        &painter,
        Pos2::new(panel.center().x - 52.0, cy + 96.0),
        "◀◀",
        rewinding && blink,
        AMBER,
        Some(40.0),
    );
    widgets::lamp_at(
        &painter,
        Pos2::new(panel.center().x + 12.0, cy + 96.0),
        "▶",
        !rewinding,
        VFD,
        Some(40.0),
    );
}

/// How far the film has run, in frames of `pitch`: the integral of its
/// speed, so it starts and stops smoothly.
fn film_travel(e: f32) -> f32 {
    // Up to speed over `a`, running until `b`, braked to a stop at `c`.
    let (a, b, c) = (0.4, REWIND_S - 0.9, REWIND_S - 0.2);
    let e = e.clamp(0.0, c);
    if e < a {
        e * e / (2.0 * a)
    } else if e < b {
        a / 2.0 + (e - a)
    } else {
        let x = e - b;
        a / 2.0 + (b - a) + x - x * x / (2.0 * (c - b))
    }
}

/// The reels' turn, radians: fast in the middle of the rewind.
fn reel_angle(e: f32) -> f32 {
    progress(e) * 9.0 * TAU + e * 0.5
}

fn reel(painter: &egui::Painter, c: Pos2, r: f32, pack: f32, angle: f32) {
    painter.circle_filled(c, r, Color32::from_rgb(14, 11, 16));
    painter.circle_filled(c, pack, Color32::from_rgb(58, 41, 32));
    painter.circle_stroke(c, pack, Stroke::new(1.0, Color32::from_rgb(90, 66, 50)));
    // The flange: a ring with three windows that show the tape pack turn.
    painter.circle_stroke(c, r, Stroke::new(2.0, CREAM_DIM));
    for i in 0..3 {
        let a = angle + i as f32 * TAU / 3.0;
        let dir = Vec2::new(a.cos(), a.sin());
        let w = Vec2::new(-dir.y, dir.x);
        let pts = vec![
            c + dir * 20.0 + w * 7.0,
            c + dir * (r - 8.0) + w * 20.0,
            c + dir * (r - 8.0) - w * 20.0,
            c + dir * 20.0 - w * 7.0,
        ];
        painter.add(Shape::convex_polygon(
            pts,
            Color32::from_black_alpha(90),
            Stroke::new(1.0, CREAM_DIM.gamma_multiply(0.5)),
        ));
    }
    painter.circle_filled(c, 15.0, CREAM_DIM);
    painter.circle_filled(c, 6.0, INK);
    for i in 0..3 {
        let a = angle + (i as f32 + 0.5) * TAU / 3.0;
        painter.line_segment(
            [
                c + Vec2::new(a.cos(), a.sin()) * 6.0,
                c + Vec2::new(a.cos(), a.sin()) * 13.0,
            ],
            Stroke::new(2.0, INK),
        );
    }
}

/// Back at the checkpoint: the black lifts off the dock.
pub struct Arrival {
    pub started: Instant,
    pub place: String,
    pub date: String,
}

pub fn arrival(ctx: &egui::Context, a: &Arrival) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("arrival"),
    ));
    let e = a.started.elapsed().as_secs_f32();
    let black = 1.0 - ease_in_out((e - 0.15) / 1.3);
    if black > 0.0 {
        painter.rect_filled(
            screen,
            0.0,
            Color32::from_black_alpha((255.0 * black) as u8),
        );
    }
    let alpha = ease_out(e / 0.3) * (1.0 - ease_in_out((e - ARRIVE_S + 0.9) / 0.9));
    if alpha <= 0.0 {
        return;
    }
    let c = Pos2::new(screen.center().x, screen.top() + screen.height() * 0.24);
    let title = painter.text(
        c,
        Align2::CENTER_BOTTOM,
        "CHECKPOINT RESTORED",
        display(54.0),
        CREAM.gamma_multiply(alpha),
    );
    let w = (title.width() + 40.0) * ease_out(e / 0.6);
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(c.x, title.bottom() + 4.0), Vec2::new(w, 4.0)),
        0.0,
        RED.gamma_multiply(alpha),
    );
    painter.text(
        Pos2::new(c.x, title.bottom() + 16.0),
        Align2::CENTER_TOP,
        format!("{} · {}", a.place.to_uppercase(), a.date),
        mono(18.0),
        VFD.gamma_multiply(alpha),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_taken_back_reads_plainly() {
        assert_eq!(
            earlier(3.0 * SECONDS_PER_DAY + 4.5 * 3600.0),
            "3 d 04 h earlier"
        );
        assert_eq!(earlier(11.2 * 3600.0), "11 h earlier");
        assert_eq!(earlier(30.0), "1 min earlier");
    }

    #[test]
    fn the_counter_rolls_all_the_way_back_and_the_film_stops() {
        assert_eq!(progress(0.0), 0.0);
        assert_eq!(progress(REWIND_S), 1.0);
        assert!(film_travel(REWIND_S) == film_travel(REWIND_S + 5.0));
    }
}
