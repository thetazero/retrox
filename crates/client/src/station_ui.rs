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
use crate::world::{Action, Screen, Service, World};

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
        Screen::Bounties => crate::bounty_ui::bounty_screen(ctx, w, here, actions),
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
            let rank = w.player.standing.rank(settlement.owner);
            ui.label(
                RichText::new(format!("they count you {}", rank.name().to_lowercase()))
                    .color(crate::bounty_ui::rank_color(rank)),
            );
            ui.add_space(14.0);
            ui.spacing_mut().item_spacing.y = 8.0;

            let shortages = Good::ALL
                .iter()
                .filter(|&&g| economy.market.shortfall[g] > 0.01)
                .count();
            let best_run = w.trade_runs(1).first().map(|r| r.profit());
            let trade_note = match (best_run, shortages) {
                (Some(p), _) => format!("buy here, sell there · best run +{}", credits(p)),
                (None, 0) => "buy and sell goods".to_owned(),
                (None, n) => format!("{} here", crate::hud::count(n, "shortage")),
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
                (b, 0) => format!("{} to haul", crate::hud::count(b, "contract")),
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
            let warrants = crate::bounty_ui::wanted(w).len();
            let bounty_note = match warrants {
                0 => "no warrants posted nearby".to_owned(),
                n => format!("{} wanted nearby", crate::hud::count(n, "gang")),
            };
            if menu_button(ui, "Bounties", &bounty_note, true) {
                actions.push(Action::Open(Screen::Bounties));
            }
            let stored_here = w.player.stored.iter().filter(|s| s.at == here).count();
            let hangar_note = match (services.outfitter, stored_here) {
                (true, 0) => "refit your ship".to_owned(),
                (true, n) => format!("refit · {n} stored here"),
                (false, 0) => format!("no outfitter · {}", nearest_note(w, Service::Refit)),
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
                format!("none here · {}", nearest_note(w, Service::Shipyard))
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
        (true, false) => (crate::widgets::CARD_FILL, CREAM, CREAM_DIM),
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
                    &format!("{} / {capacity:.0} t", crate::hud::tonnage(carried)),
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
            let planned =
                w.galaxy.factions[settlement.owner].economy == retrox_sim::EconomyModel::Planned;
            let note = if planned {
                "official prices · only the surplus over the plan is for sale · the faction \
                 adds a premium for goods it's short of"
            } else {
                "prices rise as you buy and fall as you sell · selling pays 90% of the market price"
            };
            ui.label(RichText::new(note).size(11.0).color(CREAM_DIM));
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
    runs_panel(ctx, w, here, actions);
}

/// Beside the market (issue #160): what to buy here and where to take
/// it, from the prices a pilot hears (this system and one jump out), and
/// where the hold's own goods sell best.
fn runs_panel(ctx: &egui::Context, w: &World, here: SettlementId, actions: &mut Vec<Action>) {
    let runs = w.trade_runs(4);
    panel(
        ctx,
        Panel {
            id: "trade-runs",
            title: "Trade runs",
            anchor: Align2::LEFT_TOP,
            offset: [760.0, 150.0],
            width: 470.0,
            collapsible: None,
        },
        |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let held: Vec<Good> = Good::ALL
                .into_iter()
                .filter(|&g| w.player.cargo[g] > trade::DUST_T)
                .collect();
            if !held.is_empty() {
                section(ui, "Your cargo sells at");
                for &good in &held {
                    cargo_row(ui, w, here, good, actions);
                }
                ui.add_space(6.0);
            }
            section(ui, "Buy here, sell there");
            ui.label(
                RichText::new(
                    "today's prices, heard from this system and one jump out · \
                     for as much as your hold and purse take",
                )
                .size(11.0)
                .color(CREAM_DIM),
            );
            for run in &runs {
                run_card(ui, w, run, actions);
            }
            if runs.is_empty() {
                no_runs(ui, w, here, actions);
            }
        },
    );
}

/// Where a port is, for a line under its name: its system unless it's
/// this one, and how far.
fn port_line(w: &World, to: SettlementId, jumps: u32) -> String {
    let s = &w.galaxy.settlements[to];
    let economy = economy_name(w.galaxy.factions[s.owner].economy);
    if jumps == 0 {
        format!("this system · {economy} economy")
    } else {
        format!(
            "{} · {} · {economy} economy",
            w.galaxy.systems[s.system].name,
            crate::hud::jumps_text(jumps)
        )
    }
}

/// One run: the good and what it clears, where it goes, both ends' sums,
/// and a key that buys the lot and sets the course.
fn run_card(ui: &mut egui::Ui, w: &World, run: &crate::world::Run, actions: &mut Vec<Action>) {
    let dest = &w.galaxy.settlements[run.to];
    crate::contracts_ui::card(ui, VFD, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(run.good.name().to_uppercase())
                            .font(display(26.0))
                            .color(CREAM),
                    );
                    ui.label(RichText::new("▶").color(CREAM_DIM));
                    ui.label(
                        RichText::new(dest.name.to_uppercase())
                            .font(display(20.0))
                            .color(CREAM),
                    );
                });
                ui.label(
                    RichText::new(port_line(w, run.to, run.jumps))
                        .size(11.0)
                        .color(CREAM_DIM),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        RichText::new(format!("{:.0} t for {}", run.tonnes, credits(run.cost)))
                            .font(mono(13.0))
                            .color(AMBER),
                    );
                    ui.label(RichText::new("· fetches").size(12.0).color(CREAM_DIM));
                    ui.label(
                        RichText::new(credits(run.value))
                            .font(mono(13.0))
                            .color(VFD),
                    );
                });
                let premium = w.premium(run.to, run.good, run.tonnes);
                if premium >= 1.0 {
                    ui.label(
                        RichText::new(format!(
                            "{} of it the faction's premium: they're short",
                            credits(premium)
                        ))
                        .size(11.0)
                        .color(CREAM_DIM),
                    );
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    glow_label(ui, &format!("+{}", credits(run.profit())), mono(20.0), VFD);
                    if push(
                        ui,
                        "BUY & GO",
                        mono(13.0),
                        Look::Tint(AMBER),
                        true,
                        Vec2::new(96.0, 28.0),
                    )
                    .on_hover_text("buy the lot and set course for where it sells")
                    .clicked()
                    {
                        actions.push(Action::TakeRun(*run));
                    }
                });
            });
        });
    });
}

/// A good in the hold: where it fetches most within one jump, and a key
/// to head there (or word that it's here).
fn cargo_row(
    ui: &mut egui::Ui,
    w: &World,
    here: SettlementId,
    good: Good,
    actions: &mut Vec<Action>,
) {
    let have = w.player.cargo[good];
    let best = w.buyers(good, have, 1).into_iter().next();
    ui.horizontal(|ui| {
        ui.set_min_height(30.0);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.label(
            RichText::new(format!("{} {}", crate::hud::tonnes(have), good.name()))
                .font(mono(14.0))
                .color(CREAM),
        );
        let Some(b) = best else {
            ui.label(
                RichText::new("· nobody in reach can pay for it")
                    .size(12.0)
                    .color(CREAM_DIM),
            );
            return;
        };
        let name = if b.at == here {
            "here".to_owned()
        } else {
            w.galaxy.settlements[b.at].name.clone()
        };
        ui.label(RichText::new(format!("· {name}")).size(12.0).color(CREAM));
        if b.at != here {
            ui.label(
                RichText::new(crate::hud::jumps_text(b.jumps))
                    .size(11.0)
                    .color(CREAM_DIM),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if b.at == here {
                ui.label(RichText::new("SELL ◀").font(mono(12.0)).color(VFD));
            } else if w.player.sell_at == Some(b.at) {
                crate::hud::tag(ui, "COURSE SET", AMBER);
            } else if push(
                ui,
                "GO",
                mono(13.0),
                Look::Tint(AMBER),
                true,
                Vec2::new(44.0, 26.0),
            )
            .on_hover_text("set course for where it sells best")
            .clicked()
            {
                actions.push(Action::Head(b.at));
            }
            glow_label(ui, &credits(b.per_t * have), mono(15.0), VFD);
        });
    });
}

/// Why there's no run from here, and where to look instead.
fn no_runs(ui: &mut egui::Ui, w: &World, here: SettlementId, actions: &mut Vec<Action>) {
    ui.add_space(6.0);
    let capacity = w.stats().cargo_t;
    let free = capacity - w.player.cargo_t();
    let text = if free < 1.0 {
        "The hold is full. Sell or deliver what you carry to buy more."
    } else {
        "Nothing on sale here fetches enough more within one jump to be worth the trip."
    };
    ui.label(RichText::new(text).color(CREAM));
    let planned = w.galaxy.factions[w.galaxy.settlements[here].owner].economy
        == retrox_sim::EconomyModel::Planned;
    if free < 1.0 {
        return;
    }
    if !planned {
        ui.label(
            RichText::new("Markets move daily: check back, or try another port.")
                .size(11.0)
                .color(CREAM_DIM),
        );
        return;
    }
    ui.label(
        RichText::new(
            "Planned economies hold one price everywhere, so the money is in market space, \
             where prices float, and across its border.",
        )
        .size(12.0)
        .color(CREAM_DIM),
    );
    ui.label(
        RichText::new("Meanwhile CONTRACTS and mining pay anywhere.")
            .size(12.0)
            .color(CREAM_DIM),
    );
    let Some((port, jumps)) = w.nearest_market_space() else {
        return;
    };
    let system = w.galaxy.settlements[port].system;
    ui.add_space(4.0);
    crate::contracts_ui::card(ui, AMBER, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    RichText::new("NEAREST MARKET SPACE")
                        .font(mono(11.0))
                        .color(AMBER),
                );
                ui.label(
                    RichText::new(w.galaxy.settlements[port].name.to_uppercase())
                        .font(display(22.0))
                        .color(CREAM),
                );
                ui.label(
                    RichText::new(port_line(w, port, jumps))
                        .size(11.0)
                        .color(CREAM_DIM),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let set = w.course == Some(system);
                if set {
                    crate::hud::tag(ui, "COURSE SET", AMBER);
                } else if push(
                    ui,
                    "PLOT COURSE",
                    mono(13.0),
                    Look::Tint(AMBER),
                    true,
                    Vec2::new(110.0, 28.0),
                )
                .clicked()
                {
                    actions.push(Action::PlotCourse(Some(system)));
                }
            });
        });
    });
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
    let planned = model == retrox_sim::EconomyModel::Planned;
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();
    let x0 = head.left() + 10.0;
    for (i, (x, text, align)) in [
        (x0, "GOOD", Align2::LEFT_CENTER),
        (
            head.left() + COL_STOCK,
            if planned { "FOR SALE" } else { "STOCK" },
            Align2::RIGHT_CENTER,
        ),
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
        // A planned market sells only its surplus over the plan.
        let stock = trade::for_sale(market, model, good);
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
        let premium = w.premium(here, good, 1.0);
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
        if premium >= 0.5 {
            // What the faction adds for a good it's short of.
            painter.text(
                Pos2::new(row.left() + COL_SELL, y - 6.0),
                Align2::RIGHT_CENTER,
                format!("{sell:.2}"),
                mono(15.0),
                VFD,
            );
            painter.text(
                Pos2::new(row.left() + COL_SELL, y + 10.0),
                Align2::RIGHT_CENTER,
                format!("+{premium:.0} PREMIUM"),
                mono(10.0),
                VFD,
            );
        } else {
            cell(COL_SELL, format!("{sell:.2}"), VFD);
        }
        cell(
            COL_HOLD,
            crate::hud::tonnes(have),
            if have > trade::DUST_T { CREAM } else { RULE },
        );
        let max = trade::max_buy(&w.player, capacity, economy, model, good);
        let buys = [("+1", 1.0), ("+10", 10.0), ("MAX", max)].map(|(l, n)| (l, n.min(max)));
        // Sell keys take what's left when it's less than their step, so a
        // mined fraction sells too (issue #175).
        let sells = [("−1", 1.0), ("−10", 10.0), ("ALL", have)].map(|(l, n)| (l, n.min(have)));
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
        let keys = sells.map(|(l, n)| (l, n > trade::DUST_T));
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

/// "nearest: Kirov Yards, 2 jumps" (issue #161), or where to look.
fn nearest_note(w: &World, service: Service) -> String {
    match w.nearest(service, 1).first() {
        Some(n) => format!(
            "nearest {}, {}",
            w.galaxy.settlements[n.at].name,
            crate::hud::jumps_text(n.jumps)
        ),
        None => "none within 12 jumps".to_owned(),
    }
}
