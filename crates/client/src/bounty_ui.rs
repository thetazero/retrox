//! The bounty office while docked or landed (DESIGN §7.2.2): warrants out
//! on pirate gangs nearby, and how the factions here count the player.
//! Same kit as the dispatch board: cards with a coloured tab, display type
//! for names, teal for what's paid, amber for the course.

use egui::{Align2, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use retrox_sim::pirates::{self, GangId};
use retrox_sim::{FactionId, SettlementId};

use crate::contracts_ui::card;
use crate::hud::{
    ALARM, AMBER, CREAM, CREAM_DIM, Panel, RED, RULE, VFD, count, credits, display, glow_label,
    jumps_text, mono, panel, section, tag,
};
use crate::standing::{HOSTILE, Rank};
use crate::station_ui::screen_header;
use crate::widgets::{Look, push};
use crate::world::{Action, World};

/// Warrants are posted for gangs this many jumps away or nearer.
pub const WANTED_JUMPS: usize = 6;

/// The gangs with warrants on them near here, nearest first.
pub fn wanted(w: &World) -> Vec<(GangId, usize)> {
    pirates::wanted_near(&w.galaxy, w.system, WANTED_JUMPS)
}

/// A rank's colour (DESIGN §8.4): alarm red for hostile, a dimmer red for
/// wary, dim cream for neutral, teal for trusted and up.
pub fn rank_color(rank: Rank) -> Color32 {
    match rank {
        Rank::Hostile => ALARM,
        Rank::Wary => ALARM.gamma_multiply(0.7),
        Rank::Neutral => CREAM_DIM,
        Rank::Trusted | Rank::Honoured => VFD,
    }
}

pub fn bounty_screen(
    ctx: &egui::Context,
    w: &World,
    here: SettlementId,
    actions: &mut Vec<Action>,
) {
    let Some(settlement) = w.galaxy.settlements.get(here) else {
        return;
    };
    let screen = ctx.content_rect();
    let warrants = wanted(w);
    panel(
        ctx,
        Panel {
            id: "bounties",
            title: &format!("Bounty office · {}", settlement.name),
            anchor: Align2::LEFT_TOP,
            offset: [24.0, 150.0],
            width: 780.0,
            collapsible: None,
        },
        |ui| {
            screen_header(ui, w, actions);
            let wanted_h = warrants.len() as f32 * 104.0 + 120.0;
            let room = screen.height() - 300.0;
            egui::ScrollArea::vertical()
                .max_height(room)
                .min_scrolled_height((wanted_h + 200.0).min(room))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    section(ui, "Standing");
                    standing_rows(ui, w, settlement.owner);
                    ui.add_space(8.0);
                    section(ui, "Wanted");
                    ui.label(
                        RichText::new(
                            format!(
                                "warrants on pirate gangs within {} · the faction that posted \
                                 a warrant pays it the moment a ship of the gang is destroyed · \
                                 gunships pay double",
                                count(WANTED_JUMPS, "jump")
                            ),
                        )
                        .size(11.0)
                        .color(CREAM_DIM),
                    );
                    if warrants.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "No warrants posted near here: the lanes are quiet, or nobody \
                                 has been robbed enough to pay for it yet.",
                            )
                            .color(CREAM_DIM),
                        );
                    }
                    for (gang, jumps) in &warrants {
                        warrant_card(ui, w, *gang, *jumps, actions);
                    }
                });
        },
    );
}

/// One line per faction the player has standing with, the local one first
/// (even at nothing).
fn standing_rows(ui: &mut egui::Ui, w: &World, local: FactionId) {
    let mut rows: Vec<FactionId> = vec![local];
    for (f, _) in w.player.standing.all() {
        if f != local && w.galaxy.factions.contains_key(f) {
            rows.push(f);
        }
    }
    for f in rows {
        let s = w.player.standing.get(f);
        let rank = Rank::of(s);
        let row = ui.horizontal(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.allocate_ui(Vec2::new(300.0, 26.0), |ui| {
                ui.set_width(300.0);
                ui.label(
                    RichText::new(w.faction_name(f).to_uppercase())
                        .font(display(22.0))
                        .color(CREAM),
                );
            });
            gauge(ui, s, 260.0);
            ui.add_space(8.0);
            tag(ui, &rank.name().to_uppercase(), rank_color(rank));
            ui.label(
                RichText::new(format!("{s:+.0}"))
                    .font(mono(13.0))
                    .color(rank_color(rank)),
            );
        });
        // A hairline under each, like every other list of rows.
        let r = row.response.rect;
        ui.painter().line_segment(
            [r.left_bottom(), r.right_bottom()],
            Stroke::new(1.0, RULE.gamma_multiply(0.45)),
        );
    }
    ui.label(
        RichText::new(
            "destroying pirates raises standing; shooting a faction's ships lowers it fast · \
             hostile factions refuse clearance to dock or land, and their armed ships fire \
             on sight · grudges fade in a season or two",
        )
        .size(11.0)
        .color(CREAM_DIM),
    );
}

/// A standing gauge from −100 to +100: a hairline scale with a centre
/// mark, the hostile line in red, and a needle.
fn gauge(ui: &mut egui::Ui, standing: f64, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), egui::Sense::hover());
    let p = ui.painter_at(rect.expand(2.0));
    let x = |v: f64| rect.left() + ((v + 100.0) / 200.0) as f32 * rect.width();
    let mid = rect.center().y;
    // The hostile zone, then the scale.
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), mid - 3.0),
            Pos2::new(x(HOSTILE), mid + 3.0),
        ),
        0.0,
        RED.gamma_multiply(0.35),
    );
    p.line_segment(
        [Pos2::new(rect.left(), mid), Pos2::new(rect.right(), mid)],
        Stroke::new(1.0, RULE),
    );
    for v in [-100.0, -50.0, 0.0, 50.0, 100.0] {
        let h = if v == 0.0 { 7.0 } else { 4.0 };
        p.line_segment(
            [Pos2::new(x(v), mid - h), Pos2::new(x(v), mid + h)],
            Stroke::new(1.0, CREAM_DIM),
        );
    }
    let rank = Rank::of(standing);
    let n = x(standing.clamp(-100.0, 100.0));
    let color = rank_color(rank);
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(n, mid + 1.0),
            Pos2::new(n - 5.0, rect.top()),
            Pos2::new(n + 5.0, rect.top()),
        ],
        color,
        Stroke::NONE,
    ));
    p.line_segment(
        [Pos2::new(n, mid - 2.0), Pos2::new(n, rect.bottom())],
        Stroke::new(2.0, color),
    );
}

fn warrant_card(ui: &mut egui::Ui, w: &World, id: GangId, jumps: usize, actions: &mut Vec<Action>) {
    let Some(g) = w.galaxy.pirates.gangs.get(id) else {
        return;
    };
    let Some(b) = g.bounty else {
        return;
    };
    let total: f64 = (0..g.ships.len())
        .map(|i| pirates::ship_bounty(&w.galaxy, g, i))
        .sum();
    // Edged red when it runs a gunship: the warrant pays double for it.
    let heavy = (0..g.ships.len()).any(|i| pirates::ship_bounty(&w.galaxy, g, i) > b.per_ship);
    card(ui, if heavy { RED } else { RULE }, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(430.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("▶").color(ALARM));
                    ui.label(
                        RichText::new(g.name.to_uppercase())
                            .font(display(26.0))
                            .color(CREAM),
                    );
                });
                let system = &w.galaxy.systems[g.lair].name;
                let field = &w.galaxy.fields[g.field].name;
                let where_ = jumps_text(jumps as u32);
                ui.label(
                    RichText::new(format!("lair: {field} · {system} · {where_}"))
                        .size(12.0)
                        .color(CREAM_DIM),
                );
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    glow_label(ui, &fleet_text(&g.ships), mono(14.0), ALARM);
                    ui.add_space(6.0);
                    let record = if g.haul_t < 0.5 {
                        "no raids yet".to_owned()
                    } else {
                        format!("{:.0} t taken", g.haul_t)
                    };
                    ui.label(
                        RichText::new(format!(
                            "posted by the {} · {record}",
                            w.faction_name(b.faction),
                        ))
                        .size(11.0)
                        .color(CREAM_DIM),
                    );
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let plotted = w.course == Some(g.lair);
                let here = g.lair == w.system;
                let label = if here {
                    "IN THIS SYSTEM"
                } else if plotted {
                    "COURSE SET"
                } else {
                    "PLOT COURSE"
                };
                if push(
                    ui,
                    label,
                    mono(13.0),
                    Look::Tint(AMBER),
                    !plotted && !here,
                    Vec2::new(130.0, 30.0),
                )
                .clicked()
                {
                    actions.push(Action::PlotCourse(Some(g.lair)));
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    glow_label(ui, &format!("+{}", credits(b.per_ship)), mono(22.0), VFD);
                    ui.label(RichText::new("per fighter").size(11.0).color(CREAM_DIM));
                    ui.label(
                        RichText::new(format!("{} the gang", credits(total)))
                            .size(12.0)
                            .color(VFD),
                    );
                });
            });
        });
    });
}

/// "2× INTERCEPTOR · GUNSHIP".
fn fleet_text(ships: &[String]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for h in ships {
        let name = retrox_content::ships::stock(h)
            .map_or_else(|| h.to_uppercase(), |f| f.hull().name.to_uppercase());
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, k)) => *k += 1,
            None => counts.push((name, 1)),
        }
    }
    counts
        .into_iter()
        .map(|(n, k)| if k > 1 { format!("{k}× {n}") } else { n })
        .collect::<Vec<_>>()
        .join(" · ")
}
