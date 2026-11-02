//! The pilot's handbook: a menu page (title screen and pause menu) that
//! explains how to get started, every control, and the ways to make a
//! living. Chapters are tabs across the top of the page.

use egui::{Align2, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::guide;
use crate::hud::guide::{key_table, rich_hint};
use crate::hud::{CREAM, CREAM_DIM, RED, RED_DARK, RULE, display, mono, section};

pub const CHAPTERS: [&str; 3] = ["First steps", "Controls", "Making a living"];

/// The page's contents, below its title band.
pub fn draw(ui: &mut egui::Ui, chapter: &mut usize, max_h: f32) {
    tabs(ui, chapter);
    ui.add_space(12.0);
    egui::ScrollArea::vertical()
        .id_salt(("handbook", *chapter))
        .max_height(max_h)
        .min_scrolled_height(max_h)
        .auto_shrink([false, true])
        .show(ui, |ui| match *chapter {
            0 => first_steps(ui),
            1 => controls(ui),
            _ => living(ui),
        });
}

/// Chapter tabs: the open one in red, the others outlined.
fn tabs(ui: &mut egui::Ui, chapter: &mut usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (i, name) in CHAPTERS.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(name.to_uppercase(), display(22.0), CREAM);
            let size = Vec2::new(galley.size().x + 28.0, 32.0);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let open = *chapter == i;
            let hot = response.hovered();
            let painter = ui.painter();
            if open {
                painter.rect_filled(rect, 0.0, RED);
            } else if hot {
                painter.rect_filled(rect, 0.0, RED_DARK);
            }
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, if open || hot { CREAM } else { RULE }),
                egui::StrokeKind::Inside,
            );
            // A numbered corner, like a manual's section mark.
            painter.text(
                rect.left_top() + Vec2::new(5.0, 3.0),
                Align2::LEFT_TOP,
                format!("{}", i + 1),
                mono(9.0),
                if open { CREAM } else { CREAM_DIM },
            );
            painter.galley(
                rect.center() - galley.size() / 2.0 + Vec2::new(4.0, 1.0),
                galley,
                if open || hot { CREAM } else { CREAM_DIM },
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *chapter = i;
            }
        }
    });
}

/// A numbered step: a big red numeral, a title, and text with keys.
fn step(ui: &mut egui::Ui, n: usize, title: &str, text: &str) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(52.0, 54.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            rect.left_top() + Vec2::new(0.0, -4.0),
            Align2::LEFT_TOP,
            format!("{n}"),
            display(58.0),
            RED,
        );
        // A slanted rule under the numeral.
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(rect.left(), rect.bottom() - 4.0),
                Pos2::new(rect.left() + 34.0, rect.bottom() - 4.0),
                Pos2::new(rect.left() + 30.0, rect.bottom()),
                Pos2::new(rect.left(), rect.bottom()),
            ],
            RED,
            Stroke::NONE,
        ));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(title.to_uppercase())
                    .font(display(26.0))
                    .color(CREAM),
            );
            rich_hint(ui, text, CREAM_DIM);
        });
    });
    ui.add_space(10.0);
}

fn first_steps(ui: &mut egui::Ui) {
    rich_hint(
        ui,
        "You start landed at your faction's capital with 25 000 cr and a small ship. The galaxy \
         runs whether or not you take part; here's one way into it.",
        CREAM,
    );
    ui.add_space(12.0);
    step(
        ui,
        1,
        "Take a contract",
        "Open CONTRACTS on the city's menu. Each job pays to carry goods to a settlement that's \
         short of them. ACCEPT one: the goods load and you leave a deposit. Then PLOT COURSE.",
    );
    step(
        ui,
        2,
        "Launch",
        "Press LAUNCH. Click the view or press [TAB] to steer with the mouse; [W] and [S] set \
         the throttle, [X] cuts it.",
    );
    step(
        ui,
        3,
        "Leave the gravity well",
        "Near a planet the flight panel reads GRAVITY WELL. Jumps only work outside it, and \
         fast travel only within it: [J] climbs you out to the edge, then [J] again goes on.",
    );
    step(
        ui,
        4,
        "Jump",
        "The HYPERLANES panel marks the next lane of your course with ▶. Press JUMP. Days pass \
         in a jump, and the galaxy moves on.",
    );
    step(
        ui,
        5,
        "Arrive and get paid",
        "Pick the station or city in NAVIGATION (or [T]), [J] to fast travel there, then [F] to \
         dock or [L] to land. Contracts pay on arrival: your deposit back, plus the fee.",
    );
    section(ui, "Other ways in");
    ui.add_space(4.0);
    rich_hint(
        ui,
        "Mine: fly into an asteroid field (∴ in NAVIGATION), [G] to prospect, hold [LMB] on the \
         veins, and sell the ore. Trade: buy what's in surplus, sell where it's short.",
        CREAM_DIM,
    );
    ui.add_space(6.0);
    rich_hint(
        ui,
        "In flight, the FIRST STEPS notes light up as you go and say what to do next. [H] shows \
         every control.",
        CREAM_DIM,
    );
}

fn controls(ui: &mut egui::Ui) {
    let col_w = (ui.available_width() - 24.0) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        ui.vertical(|ui| {
            ui.set_width(col_w);
            key_table(ui, "Flight", guide::FLIGHT_KEYS);
        });
        ui.vertical(|ui| {
            ui.set_width(col_w);
            key_table(ui, "Travel", guide::TRAVEL_KEYS);
            ui.add_space(10.0);
            key_table(ui, "Work & view", guide::WORK_KEYS);
        });
    });
    ui.add_space(10.0);
    section(ui, "Galaxy map");
    ui.add_space(4.0);
    rich_hint(
        ui,
        "Drag to turn the chart, right-drag to pan, scroll for range. Click a system to plot a \
         course to it; double-click to centre on it. [M] or [ESC] closes the map.",
        CREAM_DIM,
    );
    ui.add_space(6.0);
    rich_hint(
        ui,
        "Mouse sensitivity and inverted pitch are in SETTINGS.",
        CREAM_DIM,
    );
}

/// A short paragraph under a heading.
fn topic(ui: &mut egui::Ui, title: &str, text: &str) {
    section(ui, title);
    ui.add_space(4.0);
    rich_hint(ui, text, CREAM_DIM);
    ui.add_space(12.0);
}

fn living(ui: &mut egui::Ui) {
    topic(
        ui,
        "Contracts",
        "Settlements short of something post jobs on their dispatch board to fetch it. You leave \
         the cargo's value as a deposit and get it back with the fee on delivery. Late \
         deliveries lose 10% of the fee a day; ten days late, the deposit is lost.",
    );
    topic(
        ui,
        "Trade",
        "Every market's prices follow its stock. Buy where a good is in SURPLUS, sell where \
         there's a SHORTAGE. Prices move as you buy and sell, so big loads earn less per tonne.",
    );
    topic(
        ui,
        "Mining",
        "In an asteroid field, [G] reads the rocks: what they hold, how rich, and the glowing \
         veins. Hold the laser ([LMB] or [R]) on a vein: it cuts faster and yields more. Watch \
         the heat; fly close to scoop the ore, and sell it at a station.",
    );
    topic(
        ui,
        "Combat",
        "Pirate gangs hole up in asteroid fields away from faction capitals; NAVIGATION marks a \
         field where one lairs. They close in when you arrive, and they hunt: a gang that \
         spots you in fast travel can pull you out of it. [LMB] or [R] fires every gun \
         you carry; put the boresight on the red lead pip and the guns swing onto it. [Y] picks \
         the next hostile. Shields soak hits first and come back once you stop taking them; \
         heat and power are shared, so burning hard and firing hot will lock your guns. Wrecks \
         spill salvage for the tractor. While raiders are near you can't fast travel, jump or \
         save: fight, or run. Docking repairs the hull, for a price. Lose the ship and you go \
         back to the last checkpoint, written every time you dock or land: the galaxy is as it \
         was then and the same things happen again, unless you do something different.",
    );
    topic(
        ui,
        "Bounties",
        "The BOUNTIES office lists warrants on gangs within six jumps: who posted them, what \
         each ship is worth, and where the gang lairs. PLOT COURSE there and fly into its field. \
         The faction pays the moment a ship goes down, and thinks better of you for it.",
    );
    topic(
        ui,
        "Standing and piracy",
        "Every faction keeps count of you. Hunting pirates raises your standing; failing its \
         freight lowers it. [I] intercepts the trader in your sights and brings you alongside. \
         Open fire and its whole faction turns on you: hostile factions refuse you clearance to \
         dock or land and their armed ships shoot on sight. What a destroyed trader carried, \
         the tractor takes. Grudges fade in a season or two.",
    );
    topic(
        ui,
        "Ships",
        "The HANGAR refits your ship (cargo racks, radiators, reactors, drives); the SHIPYARD \
         sells new hulls. Ships you leave behind wait in storage where you left them.",
    );
    topic(
        ui,
        "Time",
        "Time passes as you travel and wait: a jump takes days, fast travel hours or days. \
         Markets, factions and wars move on meanwhile. WAIT on a station's menu passes days.",
    );
    // Draw a closing rule so the end of the chapter reads as an end.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(40.0, 4.0)),
        0.0,
        RED,
    );
}
