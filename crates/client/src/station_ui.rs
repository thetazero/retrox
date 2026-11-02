//! The screens of a station or city while docked or landed: the menu
//! (hub), the trade screen, and — in `hangar_ui` — the hangar and the
//! shipyard. Each screen has a way back to the menu (and Esc).

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use retrox_sim::SettlementId;
use retrox_sim::economy::Good;

use crate::hangar;
use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, Panel, RED, RED_DARK, RULE, VFD, credits, display,
    economy_name, glow_label, mono, panel, section, segment_bar,
};
use crate::trade;
use crate::widgets::{Look, push};
use crate::world::{Action, Screen, World};

pub fn draw(ctx: &egui::Context, w: &World, here: SettlementId, actions: &mut Vec<Action>) {
    if w.galaxy.settlements.get(here).is_none() {
        return;
    }
    match w.screen {
        Screen::Hub => hub(ctx, w, here, actions),
        Screen::Trade => trade_screen(ctx, w, here, actions),
        Screen::Hangar => crate::hangar_ui::hangar_screen(ctx, w, here, actions),
        Screen::Shipyard => crate::hangar_ui::shipyard_screen(ctx, w, here, actions),
        Screen::Contracts => crate::contracts_ui::contracts_screen(ctx, w, here, actions),
    }
}

/// Station or surface?
fn place_kind(w: &World) -> (&'static str, Action, &'static str) {
    match w.mode {
        crate::world::Mode::Docked(_) => ("Station", Action::Undock, "UNDOCK"),
        _ => ("Surface", Action::Launch, "LAUNCH"),
    }
}

/// The menu: where you are, and big buttons to everything here.
fn hub(ctx: &egui::Context, w: &World, here: SettlementId, actions: &mut Vec<Action>) {
    let settlement = &w.galaxy.settlements[here];
    let economy = &settlement.economy;
    let services = hangar::services(economy);
    let (kind, leave, leave_label) = place_kind(w);
    panel(
        ctx,
        Panel {
            id: "hub",
            title: kind,
            anchor: Align2::LEFT_CENTER,
            offset: [32.0, 0.0],
            width: 460.0,
            collapsible: None,
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(settlement.name.to_uppercase())
                    .font(display(46.0))
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
                RichText::new(format!("population {:.1}k", economy.population)).color(CREAM_DIM),
            );
            ui.add_space(14.0);
            ui.spacing_mut().item_spacing.y = 8.0;

            let shortages = Good::ALL
                .iter()
                .filter(|&&g| economy.market.shortfall[g] > 0.01)
                .count();
            let trade_note = match shortages {
                0 => "buy and sell goods".to_owned(),
                1 => "1 shortage here".to_owned(),
                n => format!("{n} shortages here"),
            };
            if menu_button(ui, "Trade", &trade_note, true) {
                actions.push(Action::Open(Screen::Trade));
            }
            let bound_here = w
                .player
                .contracts
                .iter()
                .filter(|c| c.offer.to == here)
                .count();
            let contracts_note = match (w.board.len(), w.player.contracts.len()) {
                (0, 0) => "nothing to haul today".to_owned(),
                (0, n) => format!("{n} under way · nothing new today"),
                (b, 0) => format!("{b} contracts to haul"),
                (b, n) => format!("{b} to haul · {n} under way"),
            };
            let contracts_note = if bound_here > 0 {
                format!("{contracts_note} · delivered here")
            } else {
                contracts_note
            };
            if menu_button(ui, "Contracts", &contracts_note, true) {
                actions.push(Action::Open(Screen::Contracts));
            }
            let stored_here = w.player.stored.iter().filter(|s| s.at == here).count();
            let hangar_note = match (services.outfitter, stored_here) {
                (true, 0) => "refit your ship".to_owned(),
                (true, n) => format!("refit · {n} stored here"),
                (false, 0) => "no outfitter · storage only".to_owned(),
                (false, n) => format!("no outfitter · {n} stored here"),
            };
            if menu_button(ui, "Hangar", &hangar_note, true) {
                actions.push(Action::Open(Screen::Hangar));
            }
            let yard_note = if services.shipyard {
                let in_stock = hangar::lot()
                    .iter()
                    .filter(|o| hangar::can_build(economy, o))
                    .count();
                match in_stock {
                    0 => "nothing built yet · the yard is working".to_owned(),
                    n => format!("{n} ships on the lot"),
                }
            } else {
                "no shipyard here".to_owned()
            };
            if menu_button(ui, "Shipyard", &yard_note, services.shipyard) {
                actions.push(Action::Open(Screen::Shipyard));
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if push(
                    ui,
                    leave_label,
                    display(24.0),
                    Look::Primary,
                    true,
                    Vec2::new(130.0, 36.0),
                )
                .clicked()
                {
                    actions.push(leave);
                }
                ui.add_space(10.0);
                if let Some(wait) = w.waiting {
                    let text = format!("WAITING · DAY {} OF {}", wait.waited, wait.days);
                    ui.label(RichText::new(text).color(CREAM));
                    if push(
                        ui,
                        "STOP",
                        mono(14.0),
                        Look::Tint(AMBER),
                        true,
                        Vec2::new(60.0, 36.0),
                    )
                    .clicked()
                    {
                        actions.push(Action::StopWaiting);
                    }
                } else {
                    ui.label(RichText::new("WAIT").color(CREAM_DIM));
                    for (days, text) in [(1, "1 D"), (7, "7 D"), (30, "30 D")] {
                        if push(
                            ui,
                            text,
                            mono(14.0),
                            Look::Plain,
                            true,
                            Vec2::new(48.0, 36.0),
                        )
                        .clicked()
                        {
                            actions.push(Action::Wait(days));
                        }
                    }
                }
            });
        },
    );
}

/// A big menu entry: a title in display type, a note under it, an arrow.
pub(crate) fn menu_button(ui: &mut egui::Ui, title: &str, note: &str, enabled: bool) -> bool {
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 58.0), sense);
    let painter = ui.painter();
    let hot = enabled && response.hovered();
    let (fill, text, sub) = match (enabled, hot) {
        (false, _) => (Color32::TRANSPARENT, RULE, RULE),
        (true, true) => (RED_DARK, CREAM, CREAM),
        (true, false) => (
            Color32::from_rgba_premultiplied(24, 18, 28, 200),
            CREAM,
            CREAM_DIM,
        ),
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, if hot { CREAM } else { RULE }),
        egui::StrokeKind::Inside,
    );
    // A red tab down the left edge.
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(6.0, rect.height())),
        0.0,
        if enabled { RED } else { RULE },
    );
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        title.to_uppercase(),
        display(32.0),
        text,
    );
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.bottom() - 6.0),
        Align2::LEFT_BOTTOM,
        note,
        mono(12.0),
        sub,
    );
    if enabled {
        let c = Pos2::new(rect.right() - 22.0, rect.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + Vec2::new(-6.0, -9.0),
                c + Vec2::new(6.0, 0.0),
                c + Vec2::new(-6.0, 9.0),
            ],
            if hot { CREAM } else { CREAM_DIM },
            Stroke::NONE,
        ));
    }
    response.clicked()
}

/// The row at the top of every sub-screen: back to the menu, and credits.
pub(crate) fn screen_header(ui: &mut egui::Ui, w: &World, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if push(
            ui,
            "◀ BACK",
            display(20.0),
            Look::Plain,
            true,
            Vec2::new(0.0, 30.0),
        )
        .on_hover_text("back to the menu (Esc)")
        .clicked()
        {
            actions.push(Action::Open(Screen::Hub));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            glow_label(ui, &credits(w.player.credits), mono(20.0), AMBER);
        });
    });
    ui.add_space(4.0);
}

fn trade_screen(ctx: &egui::Context, w: &World, here: SettlementId, actions: &mut Vec<Action>) {
    let settlement = &w.galaxy.settlements[here];
    let economy = &settlement.economy;
    let capacity = w.stats().cargo_t;
    let carried = w.player.cargo_t();
    panel(
        ctx,
        Panel {
            id: "trade",
            title: &format!("Trade · {}", settlement.name),
            anchor: Align2::LEFT_TOP,
            offset: [24.0, 150.0],
            width: 720.0,
            collapsible: None,
        },
        |ui| {
            screen_header(ui, w, actions);
            ui.horizontal(|ui| {
                ui.label(RichText::new("HOLD").color(CREAM_DIM));
                glow_label(
                    ui,
                    &format!("{carried:.0} / {capacity:.0} t"),
                    mono(18.0),
                    VFD,
                );
                ui.label(RichText::new(format!("· {}", w.player.ship.name)).color(CREAM_DIM));
            });
            let fill = if capacity > 0.0 {
                (carried / capacity) as f32
            } else {
                0.0
            };
            segment_bar(ui, fill, ui.available_width(), VFD);
            ui.add_space(8.0);
            section(ui, "Market");
            ui.label(
                RichText::new(
                    "prices rise as you buy and fall as you sell · selling pays 90% of the market price",
                )
                .size(11.0)
                .color(CREAM_DIM),
            );
            ui.add_space(4.0);
            market_table(ui, w, here, capacity, actions);
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "the settlement's treasury: {}",
                    credits(economy.treasury)
                ))
                .size(11.0)
                .color(CREAM_DIM),
            );
        },
    );
}

/// Column positions across the market table, from its left edge.
const COL_STOCK: f32 = 150.0;
const COL_BUY: f32 = 240.0;
const COL_SELL: f32 = 310.0;
const COL_HOLD: f32 = 380.0;
const COL_BANKS: f32 = 400.0;
const BANK_W: f32 = 132.0;

/// The market as rows of goods: name and stock (with a gauge of how full
/// the market is), prices, what you carry, then a bank of buy keys in amber
/// and sell keys in teal.
fn market_table(
    ui: &mut egui::Ui,
    w: &World,
    here: SettlementId,
    capacity: f64,
    actions: &mut Vec<Action>,
) {
    let economy = &w.galaxy.settlements[here].economy;
    let market = &economy.market;
    let model = w.galaxy.factions[w.galaxy.settlements[here].owner].economy;
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();
    let x0 = head.left() + 10.0;
    for (i, (x, text, align)) in [
        (x0, "GOOD", Align2::LEFT_CENTER),
        (head.left() + COL_STOCK, "STOCK", Align2::RIGHT_CENTER),
        (head.left() + COL_BUY, "BUY", Align2::RIGHT_CENTER),
        (head.left() + COL_SELL, "SELL", Align2::RIGHT_CENTER),
        (head.left() + COL_HOLD, "HOLD", Align2::RIGHT_CENTER),
        (
            head.left() + COL_BANKS + BANK_W / 2.0,
            "BUY",
            Align2::CENTER_CENTER,
        ),
        (
            head.left() + COL_BANKS + BANK_W * 1.5 + 10.0,
            "SELL",
            Align2::CENTER_CENTER,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let color = match i {
            5 => AMBER,
            6 => VFD,
            _ => CREAM_DIM,
        };
        painter.text(
            Pos2::new(x, head.center().y),
            align,
            text,
            mono(11.0),
            color,
        );
    }
    painter.line_segment(
        [head.left_bottom(), head.right_bottom()],
        Stroke::new(1.0, RULE),
    );
    ui.spacing_mut().item_spacing.y = 0.0;
    for (i, good) in Good::ALL.into_iter().enumerate() {
        let stock = market.stock[good];
        let have = w.player.cargo[good];
        let (row, _) = ui.allocate_exact_size(Vec2::new(width, 38.0), Sense::hover());
        let painter = ui.painter();
        if i % 2 == 1 {
            painter.rect_filled(row, 0.0, crate::widgets::CARD_FILL.gamma_multiply(0.6));
        }
        painter.line_segment(
            [row.left_bottom(), row.right_bottom()],
            Stroke::new(1.0, RULE.gamma_multiply(0.45)),
        );
        let y = row.center().y;
        let shortage = market.shortfall[good] > 0.01;
        let surplus = !shortage && market.fill(good) > 2.5;
        // A tab down the left edge flags shortage or surplus.
        if shortage || surplus {
            painter.rect_filled(
                Rect::from_min_size(row.min, Vec2::new(3.0, row.height())),
                0.0,
                if shortage { ALARM } else { VFD },
            );
        }
        let note = if shortage {
            Some(("SHORTAGE", ALARM))
        } else if surplus {
            Some(("SURPLUS", VFD))
        } else {
            None
        };
        painter.text(
            Pos2::new(
                row.left() + 10.0,
                if note.is_some() { y - 6.0 } else { y + 1.0 },
            ),
            Align2::LEFT_CENTER,
            good.name().to_uppercase(),
            display(20.0),
            CREAM,
        );
        if let Some((text, color)) = note {
            painter.text(
                Pos2::new(row.left() + 10.0, y + 10.0),
                Align2::LEFT_CENTER,
                text,
                mono(10.0),
                color,
            );
        }
        let cell = |x: f32, text: String, color: Color32| {
            painter.text(
                Pos2::new(row.left() + x, y),
                Align2::RIGHT_CENTER,
                text,
                mono(15.0),
                color,
            );
        };
        cell(COL_STOCK, format!("{stock:.0} t"), VFD);
        let buy = trade::buy_cost(market, model, good, 1.0);
        let sell = trade::sell_value(market, model, good, 1.0);
        cell(COL_BUY, format!("{buy:.2}"), AMBER);
        cell(COL_SELL, format!("{sell:.2}"), VFD);
        cell(
            COL_HOLD,
            format!("{have:.0} t"),
            if have > 0.0 { CREAM } else { RULE },
        );
        let max = trade::max_buy(&w.player, capacity, economy, model, good);
        let buys = [("+1", 1.0), ("+10", 10.0), ("MAX", max)].map(|(l, n)| (l, n.min(max)));
        let sells =
            [("−1", 1.0), ("−10", 10.0), ("ALL", have)].map(|(l, n)| (l, n.min(have).floor()));
        let bank_rect = |x: f32| {
            Rect::from_min_size(
                Pos2::new(row.left() + x, row.top() + 7.0),
                Vec2::new(BANK_W, row.height() - 14.0),
            )
        };
        let id = egui::Id::unique(("trade-bank", i));
        let keys = buys.map(|(l, n)| (l, n >= 1.0));
        if let Some(k) =
            crate::widgets::bank(ui, bank_rect(COL_BANKS), id.with("buy"), &keys, AMBER)
        {
            actions.push(Action::Trade(good, buys[k].1));
        }
        let keys = sells.map(|(l, n)| (l, n >= 1.0));
        if let Some(k) = crate::widgets::bank(
            ui,
            bank_rect(COL_BANKS + BANK_W + 10.0),
            id.with("sell"),
            &keys,
            VFD,
        ) {
            actions.push(Action::Trade(good, -sells[k].1));
        }
    }
}
