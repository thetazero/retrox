//! The hangar and shipyard screens, around the ship standing in the bay
//! (drawn by `main`): refit the ship you're flying and manage the ships you
//! keep in storage, or look over and buy ships off the lot. Hovering any
//! refit option previews its stats against the current fit.

use egui::{Align2, Color32, RichText, Stroke, Vec2};
use retrox_content::ships::{MODULES, SECTIONS, price};
use retrox_sim::SettlementId;
use retrox_sim::ship::{Loadout, Mount, ShipStats, Size, SlotKind};

use crate::hangar::{self, Job, Services};
use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, Panel, RED, RED_DARK, VFD, credits, display, panel, section,
};
use crate::station_ui::screen_header;
use crate::world::{Action, World};

/// What's picked on the loadout: a slot to fill, or a mount to re-section.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    Slot { mount: usize, slot: usize },
    Mount(usize),
}

const SIDE_WIDTH: f32 = 440.0;

/// Your ship in the bay: the loadout to refit on the left, its stats and
/// your stored ships on the right.
pub fn hangar_screen(
    ctx: &egui::Context,
    w: &World,
    here: SettlementId,
    actions: &mut Vec<Action>,
) {
    let Some(settlement) = w.galaxy.settlements.get(here) else {
        return;
    };
    let services = hangar::services(&settlement.economy);
    let pick_id = egui::Id::unique("hangar-pick");
    let mut pick: Option<Pick> = ctx.data(|d| d.get_temp(pick_id)).flatten();
    let mut hovered: Option<Job> = None;
    let screen = ctx.content_rect();

    panel(
        ctx,
        Panel {
            id: "hangar-loadout",
            title: "Hangar · refit",
            anchor: Align2::LEFT_TOP,
            offset: [24.0, 150.0],
            width: SIDE_WIDTH,
            collapsible: None,
        },
        |ui| {
            screen_header(ui, w, actions);
            egui::ScrollArea::vertical()
                .max_height(screen.height() - 300.0)
                .min_scrolled_height(screen.height() - 300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    loadout(ui, w, services, &mut pick, &mut hovered, actions)
                });
        },
    );

    panel(
        ctx,
        Panel {
            id: "hangar-ship",
            title: "Your ship",
            anchor: Align2::RIGHT_TOP,
            offset: [-24.0, 24.0],
            width: 400.0,
            collapsible: None,
        },
        |ui| {
            let player = &w.player;
            ship_heading(ui, &player.ship.name, &player.ship.fit);
            let preview = hovered.and_then(|j| hangar::preview(player, j));
            stats_block(ui, &player.ship.fit, preview.as_ref());
            ui.add_space(8.0);
            fleet(ui, w, here, services, actions);
        },
    );
    ctx.data_mut(|d| d.insert_temp(pick_id, pick));
}

/// Ships for sale: the lot on the left (click one to put it on the bay
/// floor), and the chosen ship against yours on the right.
pub fn shipyard_screen(
    ctx: &egui::Context,
    w: &World,
    here: SettlementId,
    actions: &mut Vec<Action>,
) {
    let Some(settlement) = w.galaxy.settlements.get(here) else {
        return;
    };
    let economy = &settlement.economy;
    let offers = hangar::lot();
    panel(
        ctx,
        Panel {
            id: "shipyard-lot",
            title: "Shipyard · the lot",
            anchor: Align2::LEFT_TOP,
            offset: [24.0, 150.0],
            width: SIDE_WIDTH,
            collapsible: None,
        },
        |ui| {
            screen_header(ui, w, actions);
            ui.label(
                RichText::new(format!(
                    "yard stock {:.0} t of hull · builds up to {:.0} t a day",
                    economy.hull_t,
                    hangar::yard_output_t_per_day(economy)
                ))
                .color(CREAM_DIM),
            );
            ui.add_space(4.0);
            egui::Grid::new("lot")
                .num_columns(3)
                .spacing([12.0, 3.0])
                .striped(true)
                .show(ui, |ui| {
                    for head in ["SHIP", "ROLE", "PRICE"] {
                        ui.label(RichText::new(head).size(11.0).color(CREAM_DIM));
                    }
                    ui.end_row();
                    for offer in &offers {
                        let built = hangar::can_build(economy, offer);
                        let chosen = w.inspect.is_some_and(|h| h.id == offer.hull.id);
                        let name = RichText::new(offer.hull.name).color(if built {
                            CREAM
                        } else {
                            CREAM_DIM
                        });
                        if ui.selectable_label(chosen, name).clicked() {
                            actions.push(Action::Inspect(offer.hull));
                        }
                        ui.label(
                            RichText::new(format!(
                                "{:?} {}",
                                offer.hull.role,
                                size_letter(offer.hull.size)
                            ))
                            .color(CREAM_DIM),
                        );
                        let color = if offer.price > w.player.credits {
                            ALARM
                        } else if built {
                            VFD
                        } else {
                            CREAM_DIM
                        };
                        ui.label(RichText::new(credits(offer.price)).color(color));
                        ui.end_row();
                    }
                });
        },
    );

    panel(
        ctx,
        Panel {
            id: "shipyard-ship",
            title: "On the floor",
            anchor: Align2::RIGHT_TOP,
            offset: [-24.0, 24.0],
            width: 400.0,
            collapsible: None,
        },
        |ui| {
            let Some(offer) = w
                .inspect
                .and_then(|h| offers.iter().find(|o| o.hull.id == h.id))
            else {
                ui.label(RichText::new("Pick a ship on the lot to look it over.").color(CREAM_DIM));
                ui.add_space(6.0);
                ship_heading(ui, &w.player.ship.name, &w.player.ship.fit);
                stats_block(ui, &w.player.ship.fit, None);
                return;
            };
            let job = Job::BuyShip(offer.hull);
            let fit = hangar::preview(&w.player, job);
            ship_heading(
                ui,
                offer.hull.name,
                fit.as_ref().unwrap_or(&w.player.ship.fit),
            );
            ui.label(
                RichText::new("compared with your ship")
                    .size(11.0)
                    .color(CREAM_DIM),
            );
            stats_block(ui, &w.player.ship.fit, fit.as_ref());
            ui.add_space(10.0);
            let built = hangar::can_build(economy, offer);
            let affordable = offer.price <= w.player.credits;
            ui.horizontal(|ui| {
                let buy = egui::Button::new(
                    RichText::new(format!("BUY · {}", credits(offer.price)))
                        .font(display(24.0))
                        .color(CREAM),
                )
                .fill(RED)
                .min_size(Vec2::new(ui.available_width(), 38.0));
                if ui.add_enabled(built && affordable, buy).clicked() {
                    actions.push(Action::Hangar(job));
                }
            });
            let why = if !built {
                format!(
                    "the yard needs {:.0} t of hull for one · it has {:.0} t",
                    offer.mass_t, economy.hull_t
                )
            } else if !affordable {
                format!("you're {} short", credits(offer.price - w.player.credits))
            } else {
                "takes a day · your current ship stays here in storage".to_owned()
            };
            ui.label(RichText::new(why).size(11.0).color(if built && affordable {
                CREAM_DIM
            } else {
                AMBER
            }));
        },
    );
}

fn loadout(
    ui: &mut egui::Ui,
    w: &World,
    services: Services,
    pick: &mut Option<Pick>,
    hover_now: &mut Option<Job>,
    actions: &mut Vec<Action>,
) {
    let fit = &w.player.ship.fit;
    if !services.outfitter {
        ui.label(RichText::new("No outfitter here: refits unavailable").color(AMBER));
    } else {
        ui.label(
            RichText::new("click a slot to change what's in it")
                .size(11.0)
                .color(CREAM_DIM),
        );
    }
    for (m, (mount, fitted)) in fit.mounts().enumerate() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let name = fitted.map_or("(empty)", |f| f.section.name);
            let where_ = match mount {
                Mount::Fixed(_) => "HULL".to_owned(),
                Mount::Open { position, size } => {
                    format!("{position:?} {}", size_letter(*size)).to_uppercase()
                }
            };
            ui.label(RichText::new(where_).size(11.0).color(CREAM_DIM));
            ui.label(
                RichText::new(name.to_uppercase())
                    .font(display(19.0))
                    .color(CREAM),
            );
            if matches!(mount, Mount::Open { .. }) && services.outfitter {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let picked = *pick == Some(Pick::Mount(m));
                    if ui.selectable_label(picked, "SWAP SECTION").clicked() {
                        *pick = (!picked).then_some(Pick::Mount(m));
                    }
                });
            }
        });
        if *pick == Some(Pick::Mount(m)) {
            section_choices(ui, w, m, hover_now, actions);
        }
        let Some(fitted) = fitted else { continue };
        for (s, (slot, module)) in fitted.slots().enumerate() {
            let picked = *pick == Some(Pick::Slot { mount: m, slot: s });
            let label = format!(
                "  {} {}  {}",
                kind_code(slot.kind),
                size_letter(slot.size),
                module.map_or("— empty —", |m| m.name)
            );
            let text = if module.is_some() {
                RichText::new(label)
            } else {
                RichText::new(label).color(CREAM_DIM)
            };
            let clicked = ui
                .add_enabled(services.outfitter, egui::Button::selectable(picked, text))
                .clicked();
            if clicked {
                *pick = (!picked).then_some(Pick::Slot { mount: m, slot: s });
            }
            if picked {
                module_choices(ui, w, m, s, hover_now, actions);
            }
        }
    }
}

/// Your ships: the one you fly, and those in storage here and elsewhere.
fn fleet(
    ui: &mut egui::Ui,
    w: &World,
    here: SettlementId,
    services: Services,
    actions: &mut Vec<Action>,
) {
    let player = &w.player;
    section(ui, "Stored ships");
    if player.stored.is_empty() {
        ui.label(RichText::new("none · ships you replace are kept here").color(CREAM_DIM));
        return;
    }
    egui::Grid::new("stored")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for (i, stored) in player.stored.iter().enumerate() {
                ui.label(RichText::new(&stored.ship.name).color(CREAM));
                let at_here = stored.at == here;
                let place = if at_here {
                    "here".to_owned()
                } else {
                    w.galaxy
                        .settlements
                        .get(stored.at)
                        .map_or("lost".to_owned(), |s| s.name.clone())
                };
                ui.label(RichText::new(place).color(if at_here { VFD } else { CREAM_DIM }));
                ui.horizontal(|ui| {
                    if !at_here {
                        return;
                    }
                    if ui.button("BOARD").clicked() {
                        actions.push(Action::Hangar(Job::Board(i)));
                    }
                    if services.shipyard {
                        let refund = -hangar::quote(player, Job::SellShip(i)).unwrap_or(0.0);
                        if ui.button(format!("SELL {}", credits(refund))).clicked() {
                            actions.push(Action::Hangar(Job::SellShip(i)));
                        }
                    }
                });
                ui.end_row();
            }
        });
}

/// Modules that fit a slot, with what each costs after selling the old one.
fn module_choices(
    ui: &mut egui::Ui,
    w: &World,
    mount: usize,
    slot: usize,
    hover_now: &mut Option<Job>,
    actions: &mut Vec<Action>,
) {
    let player = &w.player;
    let current = hangar::module_in(&player.ship.fit, mount, slot);
    choices_frame(ui, |ui| {
        if let Some(old) = current {
            let job = Job::Strip { mount, slot };
            let refund = hangar::quote(player, job).unwrap_or(0.0);
            choice_row(
                ui,
                w,
                &format!("Remove {}", old.name),
                "",
                refund,
                job,
                hover_now,
                actions,
            );
        }
        let mut options: Vec<_> = MODULES
            .iter()
            .filter(|m| Some(*m) != current && player.ship.fit.check(mount, slot, m).is_ok())
            .collect();
        options.sort_by(|a, b| a.size.cmp(&b.size).then(a.name.cmp(b.name)));
        if options.is_empty() && current.is_none() {
            ui.label(RichText::new("nothing on sale fits").color(CREAM_DIM));
        }
        for module in options {
            let job = Job::Fit {
                mount,
                slot,
                module,
            };
            let Some(cost) = hangar::quote(player, job) else {
                continue;
            };
            let detail = format!("{} · {:.1} t", size_letter(module.size), module.mass_t);
            choice_row(ui, w, module.name, &detail, cost, job, hover_now, actions);
        }
    });
}

/// Sections that fit an open mount.
fn section_choices(
    ui: &mut egui::Ui,
    w: &World,
    mount: usize,
    hover_now: &mut Option<Job>,
    actions: &mut Vec<Action>,
) {
    let player = &w.player;
    let current = player
        .ship
        .fit
        .mounts()
        .nth(mount)
        .and_then(|(_, s)| s)
        .map(|s| s.section.id);
    choices_frame(ui, |ui| {
        for section in SECTIONS {
            if Some(section.id) == current {
                continue;
            }
            let job = Job::SetSection { mount, section };
            let Some(cost) = hangar::quote(player, job) else {
                continue;
            };
            let detail = format!("{} slots · {:.0} t", section.slots.len(), section.mass_t);
            choice_row(ui, w, section.name, &detail, cost, job, hover_now, actions);
        }
    });
    ui.label(
        RichText::new("  swapping a section sells the modules in it")
            .size(11.0)
            .color(CREAM_DIM),
    );
}

/// One option in a choice list: name, detail, cost and a button.
#[allow(clippy::too_many_arguments)]
fn choice_row(
    ui: &mut egui::Ui,
    w: &World,
    name: &str,
    detail: &str,
    cost: f64,
    job: Job,
    hover_now: &mut Option<Job>,
    actions: &mut Vec<Action>,
) {
    let affordable = cost <= w.player.credits;
    ui.horizontal(|ui| {
        let row = ui.label(RichText::new(name).color(CREAM));
        ui.label(RichText::new(detail).size(11.0).color(CREAM_DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let verb = match job {
                Job::Strip { .. } => "SELL",
                _ => "FIT",
            };
            let amount = if cost < 0.0 {
                format!("+{}", credits(-cost))
            } else {
                credits(cost)
            };
            let button = ui.add_enabled(affordable, egui::Button::new(verb));
            let color = if !affordable {
                ALARM
            } else if cost < 0.0 {
                VFD
            } else {
                AMBER
            };
            ui.label(RichText::new(amount).color(color));
            if button.hovered() || row.hovered() {
                *hover_now = Some(job);
            }
            if button.clicked() {
                actions.push(Action::Hangar(job));
            }
        });
    });
}

fn choices_frame(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(30, 10, 10, 120))
        .stroke(Stroke::new(1.0, RED_DARK))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn ship_heading(ui: &mut egui::Ui, name: &str, fit: &Loadout) {
    ui.label(
        RichText::new(name.to_uppercase())
            .font(display(30.0))
            .color(CREAM),
    );
    let hull = fit.hull();
    ui.label(
        RichText::new(format!(
            "{} · {:?} · {}",
            hull.name,
            hull.role,
            match hull.size {
                Size::Small => "small",
                Size::Medium => "medium",
                Size::Large => "large",
            }
        ))
        .color(CREAM_DIM),
    );
    ui.label(
        RichText::new(format!(
            "worth {}",
            credits(price::resale(price::loadout_price(fit)))
        ))
        .size(11.0)
        .color(CREAM_DIM),
    );
    ui.add_space(4.0);
}

/// One line of the stats table.
struct Stat {
    label: &'static str,
    value: fn(&ShipStats) -> Option<f64>,
    unit: &'static str,
    decimals: usize,
    higher_is_better: bool,
}

const STATS: &[Stat] = &[
    Stat {
        label: "MASS",
        value: |s| Some(s.mass_t),
        unit: "t",
        decimals: 1,
        higher_is_better: false,
    },
    Stat {
        label: "ACCEL",
        value: |s| Some(s.acceleration()),
        unit: "m/s²",
        decimals: 1,
        higher_is_better: true,
    },
    Stat {
        label: "TURN 180°",
        value: |s| s.time_to_turn(180.0),
        unit: "s",
        decimals: 1,
        higher_is_better: false,
    },
    Stat {
        label: "POWER SPARE",
        value: |s| Some(s.power_margin(s.cruise)),
        unit: "MW",
        decimals: 1,
        higher_is_better: true,
    },
    Stat {
        label: "IN COMBAT",
        value: |s| Some(s.power_margin(s.combat)),
        unit: "MW",
        decimals: 1,
        higher_is_better: true,
    },
    Stat {
        label: "OVERHEAT",
        value: |s| s.time_to_overheat(s.combat),
        unit: "s",
        decimals: 0,
        higher_is_better: true,
    },
    Stat {
        label: "DAMAGE",
        value: |s| Some(s.dps),
        unit: "/s",
        decimals: 0,
        higher_is_better: true,
    },
    Stat {
        label: "SHIELD",
        value: |s| Some(s.shield),
        unit: "",
        decimals: 0,
        higher_is_better: true,
    },
    Stat {
        label: "CARGO",
        value: |s| Some(s.cargo_t),
        unit: "t",
        decimals: 0,
        higher_is_better: true,
    },
    Stat {
        label: "JUMP",
        value: |s| Some(s.jump_speed()),
        unit: "ly/d",
        decimals: 2,
        higher_is_better: true,
    },
];

/// The ship's stats in two columns; with `preview`, what a change would
/// make them, with the difference.
fn stats_block(ui: &mut egui::Ui, now: &Loadout, preview: Option<&Loadout>) {
    let a = now.stats();
    let b = preview.map(Loadout::stats);
    egui::Grid::new("stats")
        .num_columns(4)
        .spacing([14.0, 2.0])
        .min_col_width(70.0)
        .show(ui, |ui| {
            for pair in STATS.chunks(2) {
                for stat in pair {
                    stat_cell(ui, stat, &a, b.as_ref());
                }
                ui.end_row();
            }
        });
}

fn stat_cell(ui: &mut egui::Ui, stat: &Stat, now: &ShipStats, preview: Option<&ShipStats>) {
    ui.label(RichText::new(stat.label).size(11.0).color(CREAM_DIM));
    let show = |v: Option<f64>| match v {
        Some(v) => format!("{v:.*} {}", stat.decimals, stat.unit),
        // Overheat with no surplus heat, or turning with no RCS.
        None if stat.higher_is_better => "stable".to_owned(),
        None => "—".to_owned(),
    };
    let current = (stat.value)(now);
    ui.horizontal(|ui| {
        match preview.map(|p| (stat.value)(p)) {
            Some(next) if next != current => {
                let better = match (current, next) {
                    (Some(c), Some(n)) => (n > c) == stat.higher_is_better,
                    // None is "stable" (good) for overheating, "can't" for turning.
                    (Some(_), None) => stat.higher_is_better,
                    (None, Some(_)) => !stat.higher_is_better,
                    (None, None) => true,
                };
                let color = if better { VFD } else { ALARM };
                ui.label(RichText::new(show(next)).color(color));
                ui.label(
                    RichText::new(if better { "▲" } else { "▼" })
                        .size(10.0)
                        .color(color),
                );
            }
            _ => {
                let warn = stat.label.starts_with("POWER") && current.is_some_and(|v| v < 0.0);
                ui.label(RichText::new(show(current)).color(if warn { ALARM } else { VFD }));
            }
        }
    });
}

fn kind_code(kind: SlotKind) -> &'static str {
    match kind {
        SlotKind::Weapon => "WPN",
        SlotKind::Engine => "ENG",
        SlotKind::Rcs => "RCS",
        SlotKind::Power => "PWR",
        SlotKind::Shield => "SHD",
        SlotKind::Radar => "RDR",
        SlotKind::Radiator => "RAD",
        SlotKind::Cargo => "CGO",
        SlotKind::JumpDrive => "JMP",
    }
}

fn size_letter(size: Size) -> &'static str {
    match size {
        Size::Small => "S",
        Size::Medium => "M",
        Size::Large => "L",
    }
}
