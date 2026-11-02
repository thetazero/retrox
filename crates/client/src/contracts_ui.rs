//! The dispatch board while docked or landed, and the contracts readout in
//! flight. Same panel kit as the trade screen: red tabs, display type for
//! names, teal VFD for stock, time and what you're paid, amber for what
//! you pay or are short of (DESIGN §8.4).

use egui::{Align2, Color32, Margin, Rect, RichText, Stroke, Vec2};
use retrox_sim::SettlementId;
use retrox_sim::freight::Offer;

use crate::contracts::{self, Contract};
use crate::hud::jumps_text;
use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, Panel, RED, RULE, VFD, credits, display, glow_label, mono,
    panel, section, segment_bar, tag,
};
use crate::station_ui::screen_header;
use crate::widgets::{CARD_FILL, READOUT_FILL};
use crate::world::{Action, World};

pub fn contracts_screen(
    ctx: &egui::Context,
    w: &World,
    here: SettlementId,
    actions: &mut Vec<Action>,
) {
    let Some(settlement) = w.galaxy.settlements.get(here) else {
        return;
    };
    let capacity = w.stats().cargo_t;
    let carried = w.player.cargo_t();
    let screen = ctx.content_rect();
    panel(
        ctx,
        Panel {
            id: "contracts",
            title: &format!("Dispatch · {}", settlement.name),
            anchor: Align2::LEFT_TOP,
            offset: [24.0, 150.0],
            width: 780.0,
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
                let under = w.player.contract_t();
                if under > 0.0 {
                    ui.label(RichText::new(format!("· {under:.0} t under contract")).color(AMBER));
                }
            });
            let fill = if capacity > 0.0 {
                (carried / capacity) as f32
            } else {
                0.0
            };
            segment_bar(ui, fill, ui.available_width(), VFD);
            ui.add_space(6.0);

            // Tall enough for every card, up to the bottom of the screen.
            let cards = w.board.len() + w.player.contracts.len();
            let wanted = cards as f32 * 112.0 + 160.0;
            let room = screen.height() - 330.0;
            egui::ScrollArea::vertical()
                .max_height(room)
                .min_scrolled_height(wanted.min(room))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if !w.player.contracts.is_empty() {
                        section(ui, "Your contracts");
                        for (i, c) in w.player.contracts.iter().enumerate() {
                            active_card(ui, w, i, c, actions);
                        }
                        ui.add_space(8.0);
                    }
                    section(ui, "Dispatch board");
                    ui.label(
                        RichText::new(
                            "short settlements nearby buy these goods here and pay you to carry them · \
                             you leave the cargo's value as a deposit, refunded on delivery · \
                             10% of the fee off per day late; 10 days late, the deposit is forfeit",
                        )
                        .size(11.0)
                        .color(CREAM_DIM),
                    );
                    let free = capacity - carried;
                    if !w.board.is_empty() && w.board.iter().all(|o| o.amount > free + 1e-9) {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!(
                                "the hold has {free:.0} t free · deliver or sell cargo to take more"
                            ))
                            .color(AMBER),
                        );
                    }
                    ui.add_space(2.0);
                    if w.board.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "Nothing to haul from here today: no nearby settlement is short of \
                                 anything this market has to spare.",
                            )
                            .color(CREAM_DIM),
                        );
                        ui.label(
                            RichText::new("Markets move daily; check back, or try another port.")
                                .size(11.0)
                                .color(CREAM_DIM),
                        );
                    }
                    for (i, offer) in w.board.iter().enumerate() {
                        offer_card(ui, w, capacity, i, offer, actions);
                    }
                });
        },
    );
}

/// A card's frame: a dark plate with a coloured tab down the left edge.
pub(crate) fn card(ui: &mut egui::Ui, tab: Color32, add: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::new()
        .fill(CARD_FILL)
        .stroke(Stroke::new(1.0, RULE))
        .inner_margin(Margin {
            left: 18,
            right: 10,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    let r = frame.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.min, Vec2::new(6.0, r.height())),
        0.0,
        tab,
    );
}

/// Where a settlement is: its system and faction.
fn whereabouts(w: &World, to: SettlementId) -> String {
    let s = &w.galaxy.settlements[to];
    let system = &w.galaxy.systems[s.system].name;
    let faction = &w.galaxy.factions[s.owner].name;
    format!("{system} · {faction}")
}

/// The left half of a card: destination, where it is, what's carried, and
/// why it's wanted.
fn cargo_lines(ui: &mut egui::Ui, w: &World, o: &Offer, route: &str) {
    let dest = &w.galaxy.settlements[o.to];
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    ui.horizontal(|ui| {
        ui.label(RichText::new("▶").color(CREAM_DIM));
        ui.label(
            RichText::new(dest.name.to_uppercase())
                .font(display(28.0))
                .color(CREAM),
        );
        if o.urgent {
            ui.add_space(4.0);
            tag(ui, "GOING WITHOUT", ALARM);
        }
    });
    ui.label(
        RichText::new(format!("{} · {route}", whereabouts(w, o.to)))
            .size(12.0)
            .color(CREAM_DIM),
    );
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        glow_label(
            ui,
            &format!("{:.0} t {}", o.amount, o.good.name()),
            mono(16.0),
            VFD,
        );
        ui.add_space(6.0);
        // How much of what they need they have (after what's on the way).
        ui.label(RichText::new("STOCK").size(11.0).color(CREAM_DIM));
        let have = (1.0 - o.shortage) as f32;
        segment_bar(ui, have, 90.0, if o.urgent { ALARM } else { AMBER });
        ui.label(
            RichText::new(format!("{:.0}% of need", have * 100.0))
                .size(11.0)
                .color(CREAM_DIM),
        );
    });
}

fn offer_card(
    ui: &mut egui::Ui,
    w: &World,
    capacity: f64,
    i: usize,
    o: &Offer,
    actions: &mut Vec<Action>,
) {
    let refusal = contracts::refusal(&w.player, capacity, o);
    let days_left = o.deadline.days().saturating_sub(w.date().days());
    card(ui, if o.urgent { RED } else { RULE }, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(400.0);
                let route = format!("{} · about {:.0} d", jumps_text(o.jumps), o.trip_days);
                cargo_lines(ui, w, o, &route);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = crate::widgets::push(
                    ui,
                    "ACCEPT",
                    display(22.0),
                    crate::widgets::Look::Primary,
                    refusal.is_none(),
                    Vec2::new(96.0, 40.0),
                );
                if response.clicked() {
                    actions.push(Action::AcceptContract(i));
                }
                if let Some(why) = &refusal {
                    response.on_hover_text(why);
                }
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        glow_label(ui, &format!("+{}", credits(o.fee)), mono(24.0), VFD);
                        ui.label(
                            RichText::new(format!("due {} · {days_left} d", o.deadline))
                                .size(12.0)
                                .color(CREAM_DIM),
                        );
                        let deposit = format!("deposit {}", credits(o.value));
                        let short = o.value > w.player.credits;
                        ui.label(RichText::new(deposit).size(11.0).color(if short {
                            ALARM
                        } else {
                            CREAM_DIM
                        }));
                    });
                });
            });
        });
    });
}

/// Colour for time left on a contract: teal with time to spare, amber
/// close to the deadline, alarm red once late.
fn due_color(days_left: i64) -> Color32 {
    if days_left < 0 {
        ALARM
    } else if days_left <= 3 {
        AMBER
    } else {
        VFD
    }
}

/// Days until the deadline (negative once late), and how to say it.
fn due_text(w: &World, c: &Contract) -> (i64, String) {
    let today = w.date().days() as i64;
    let left = c.offer.deadline.days() as i64 - today;
    let text = match left {
        l if l < 0 => format!("{} D LATE", -l),
        0 => "DUE TODAY".into(),
        l => format!("{l} D LEFT"),
    };
    (left, text)
}

/// Jumps from the current system to a contract's destination.
fn jumps_from_here(w: &World, to: SettlementId) -> Option<u32> {
    let system = w.galaxy.settlements[to].system;
    w.route(system).map(|r| r.len().saturating_sub(1) as u32)
}

fn active_card(ui: &mut egui::Ui, w: &World, i: usize, c: &Contract, actions: &mut Vec<Action>) {
    let o = &c.offer;
    let (left, due) = due_text(w, c);
    let pay = retrox_sim::freight::pay_on(o.fee, o.deadline, w.date());
    let confirm_id = egui::Id::unique(("abandon", i, o.to, o.good));
    let confirming = ui.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false);
    card(ui, due_color(left), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(400.0);
                let route = match jumps_from_here(w, o.to) {
                    Some(n) => format!("{} from here", jumps_text(n)),
                    None => "no route from here".into(),
                };
                cargo_lines(ui, w, o, &route);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    glow_label(ui, &due, mono(22.0), due_color(left));
                    ui.label(
                        RichText::new(format!("pays {} · due {}", credits(pay), o.deadline))
                            .size(12.0)
                            .color(VFD),
                    );
                    ui.label(
                        RichText::new(format!("deposit {} held", credits(o.value)))
                            .size(11.0)
                            .color(CREAM_DIM),
                    );
                });
            });
        });
        ui.add_space(4.0);
        let row = Vec2::new(ui.available_width(), 24.0);
        let right = egui::Layout::right_to_left(egui::Align::Center);
        ui.allocate_ui_with_layout(row, right, |ui| {
            let (label, look) = if confirming {
                (
                    format!("FORFEIT {} DEPOSIT?", credits(o.value)),
                    crate::widgets::Look::Tint(ALARM),
                )
            } else {
                ("ABANDON".to_owned(), crate::widgets::Look::Plain)
            };
            let abandon =
                crate::widgets::push(ui, &label, mono(13.0), look, true, Vec2::new(130.0, 26.0));
            if abandon.clicked() {
                if confirming {
                    actions.push(Action::AbandonContract(i));
                    ui.data_mut(|d| d.remove::<bool>(confirm_id));
                } else {
                    ui.data_mut(|d| d.insert_temp(confirm_id, true));
                }
            } else if confirming && !abandon.hovered() && ui.input(|i| i.pointer.any_click()) {
                ui.data_mut(|d| d.remove::<bool>(confirm_id));
            }
            let system = w.galaxy.settlements[o.to].system;
            let plotted = w.course == Some(system);
            if crate::widgets::push(
                ui,
                if plotted { "COURSE SET" } else { "PLOT COURSE" },
                mono(13.0),
                crate::widgets::Look::Tint(AMBER),
                !plotted && system != w.system,
                Vec2::new(130.0, 26.0),
            )
            .clicked()
            {
                actions.push(Action::PlotCourse(Some(system)));
            }
        });
    });
}

/// In flight: the contracts being carried, under the where-and-when
/// readout. Nothing when there are none.
pub fn flight_readout(ctx: &egui::Context, w: &World) {
    if w.player.contracts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::unique("contracts-readout"))
        .anchor(Align2::LEFT_TOP, [18.0, 126.0])
        .show(ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(READOUT_FILL)
                .inner_margin(Margin {
                    left: 18,
                    right: 14,
                    top: 4,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(
                        RichText::new("CONTRACTS")
                            .font(display(20.0))
                            .color(CREAM_DIM),
                    );
                    for c in &w.player.contracts {
                        let o = &c.offer;
                        let (left, due) = due_text(w, c);
                        let dest = &w.galaxy.settlements[o.to];
                        let course = w.course == Some(dest.system);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{:.0} t {} ▶ {}",
                                    o.amount,
                                    o.good.name(),
                                    dest.name
                                ))
                                .color(CREAM),
                            );
                            ui.add_space(6.0);
                            glow_label(ui, &due, mono(14.0), due_color(left));
                        });
                        let place = if dest.system == w.system {
                            "in this system".to_owned()
                        } else {
                            let system = &w.galaxy.systems[dest.system].name;
                            match jumps_from_here(w, o.to) {
                                Some(n) => format!("{system} · {}", jumps_text(n)),
                                None => system.clone(),
                            }
                        };
                        let note = if course { " · course set" } else { "" };
                        ui.label(
                            RichText::new(format!("{place}{note}"))
                                .size(11.0)
                                .color(if course { AMBER } else { CREAM_DIM }),
                        );
                    }
                });
            let r = frame.response.rect;
            ui.painter().rect_filled(
                Rect::from_min_size(r.min, Vec2::new(6.0, r.height())),
                0.0,
                AMBER,
            );
        });
}
