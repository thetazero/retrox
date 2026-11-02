//! The skimming HUD (issue #68): how deep in the gas the ship is, how fast
//! the scoop takes it in, how hot it's running and what the hold is worth.

use egui::{Align2, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use retrox_sim::economy::Good;

use super::{
    ALARM, AMBER, CREAM, CREAM_DIM, HudInput, Panel, RULE, VFD, VFD_DIM, glow_label, keycap, lamp,
    mono, panel, segment_bar, tag,
};
use crate::skim::{self, BAND_M};
use crate::world::Place;

/// Gas's colour on the HUD: the pale amber of a giant's cloud tops.
pub const GAS: Color32 = Color32::from_rgb(255, 206, 150);

/// The altimeter runs from the cloud deck to this height, metres.
const GAUGE_TOP_M: f64 = 160_000.0;
const WIDTH: f32 = 276.0;

/// Whether the skimming panel is up: the ship is near a giant's clouds.
pub fn showing(h: &HudInput) -> bool {
    h.world.skim.pass.is_some()
}

pub fn draw(ctx: &egui::Context, h: &HudInput) {
    let w = h.world;
    let Some(pass) = w.skim.pass else {
        return;
    };
    let stats = w.stats();
    let giant = w.name(Place::Body(pass.giant));
    panel(
        ctx,
        Panel {
            id: "skimming",
            title: "Skimming",
            anchor: Align2::LEFT_CENTER,
            offset: [16.0, 40.0],
            width: 300.0,
            collapsible: Some(true),
        },
        |ui| {
            ui.label(
                RichText::new(format!(
                    "{} · GAS {:.0}%",
                    giant.to_uppercase(),
                    pass.richness * 100.0
                ))
                .font(mono(12.0))
                .color(CREAM),
            );
            ui.add_space(2.0);
            altimeter(ui, pass.altitude_m);
            ui.add_space(4.0);

            // Readouts: height, speed through the gas, intake.
            ui.horizontal(|ui| {
                let alt = pass.altitude_m.max(0.0) / 1000.0;
                readout(ui, "ALT", &format!("{alt:.0} km"), VFD);
                readout(ui, "SPD", &format!("{:.0} m/s", pass.vel.length()), VFD);
                let rate = w.skim.rate;
                readout(
                    ui,
                    "IN",
                    &format!("{rate:.2} t/s"),
                    if rate > 0.0 { GAS } else { VFD_DIM },
                );
            });
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("SCOOP").color(CREAM_DIM));
                lamp(ui, "OPEN", w.skim.open, AMBER);
                lamp(ui, "VENTING", w.skim.locked, ALARM);
                lamp(ui, "IN BAND", skim::density(pass.altitude_m) > 0.0, VFD);
            });
            let heat = w.skim.heat as f32;
            let color = if w.skim.locked || heat > 0.8 {
                ALARM
            } else {
                VFD
            };
            segment_bar(ui, heat, WIDTH, color);
            ui.add_space(4.0);

            let cargo = w.player.cargo_t();
            ui.horizontal(|ui| {
                ui.label(RichText::new("HOLD").color(CREAM_DIM));
                ui.label(
                    RichText::new(format!("{cargo:.1} / {:.0} t", stats.cargo_t)).color(
                        if cargo >= stats.cargo_t - 0.01 {
                            ALARM
                        } else {
                            VFD
                        },
                    ),
                );
            });
            segment_bar(ui, (cargo / stats.cargo_t.max(1e-9)) as f32, WIDTH, AMBER);
            ui.add_space(4.0);

            let t = w.mining.haul[Good::Gas];
            if t >= 0.05 {
                ui.horizontal(|ui| {
                    tag(ui, "gas", GAS);
                    ui.label(RichText::new(format!("{t:.1} t skimmed")).color(CREAM));
                });
                super::mining::buyers(ui, w, Good::Gas, w.player.cargo[Good::Gas].max(t));
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                keycap(ui, "LMB");
                keycap(ui, "R");
                ui.label(RichText::new("open the scoop").color(CREAM));
            });
            let hint = if skim::density(pass.altitude_m) <= 0.0 {
                "dive below 120 km into the band"
            } else if w.skim.locked {
                "climb or slow down while it vents"
            } else {
                "deeper fills faster; faster runs hotter"
            };
            ui.label(RichText::new(hint).color(CREAM_DIM));
        },
    );
}

/// A small labelled VFD readout.
fn readout(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.label(RichText::new(label).font(mono(11.0)).color(CREAM_DIM));
    glow_label(ui, value, mono(14.0), color);
    ui.add_space(6.0);
}

/// The altimeter: a strip from the cloud deck (left) up past the band's
/// top, shaded by how dense the gas is, with the ship's height as an amber
/// needle.
fn altimeter(ui: &mut egui::Ui, altitude_m: f64) {
    let height = 22.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, height + 14.0), Sense::hover());
    let painter = ui.painter();
    let strip = Rect::from_min_size(rect.min, Vec2::new(WIDTH, height));
    painter.rect_filled(strip, 0.0, VFD_DIM.gamma_multiply(0.5));
    let x_at = |m: f64| strip.left() + (m / GAUGE_TOP_M).clamp(0.0, 1.0) as f32 * WIDTH;
    // Density as columns of cloud colour, thick at the deck.
    let n = 56;
    for i in 0..n {
        let m0 = GAUGE_TOP_M * i as f64 / n as f64;
        let rho = skim::density(m0) as f32;
        if rho <= 0.0 {
            continue;
        }
        let r = Rect::from_min_max(
            Pos2::new(x_at(m0), strip.top() + 1.0),
            Pos2::new(
                x_at(GAUGE_TOP_M * (i + 1) as f64 / n as f64) - 1.0,
                strip.bottom() - 1.0,
            ),
        );
        painter.rect_filled(r, 0.0, GAS.gamma_multiply(0.15 + 0.75 * rho));
    }
    // The band's top, and kilometre ticks.
    let band = x_at(BAND_M);
    painter.line_segment(
        [
            Pos2::new(band, strip.top() - 2.0),
            Pos2::new(band, strip.bottom() + 2.0),
        ],
        Stroke::new(1.5, VFD),
    );
    for (m, label) in [
        (0.0, "DECK"),
        (40_000.0, "40"),
        (80_000.0, "80"),
        (BAND_M, "120 KM"),
    ] {
        let x = x_at(m);
        painter.line_segment(
            [
                Pos2::new(x, strip.bottom()),
                Pos2::new(x, strip.bottom() + 3.0),
            ],
            Stroke::new(1.0, RULE),
        );
        let align = if m == 0.0 {
            Align2::LEFT_TOP
        } else {
            Align2::CENTER_TOP
        };
        painter.text(
            Pos2::new(x, strip.bottom() + 3.0),
            align,
            label,
            mono(10.0),
            CREAM_DIM,
        );
    }
    // The needle.
    let x = x_at(altitude_m.max(0.0));
    let color = if altitude_m > GAUGE_TOP_M {
        CREAM_DIM
    } else {
        AMBER
    };
    painter.line_segment(
        [
            Pos2::new(x, strip.top() - 3.0),
            Pos2::new(x, strip.bottom() + 3.0),
        ],
        Stroke::new(2.0, color),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x - 4.0, strip.top() - 6.0),
            Pos2::new(x + 4.0, strip.top() - 6.0),
            Pos2::new(x, strip.top() - 1.0),
        ],
        color,
        Stroke::NONE,
    ));
}
