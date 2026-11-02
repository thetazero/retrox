//! `retrox-viewer` — a developer tool that runs the galaxy simulation live
//! on a pannable, zoomable map.
//!
//! Drag to pan, scroll to zoom, click a system to inspect it.

use std::time::Instant;

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::economy::{Good, summarize};
use retrox_sim::{EconomyModel, GalaxyState, Settlement, Site, SystemId, tick};

fn main() -> eframe::Result {
    let seed = std::env::args()
        .skip_while(|a| a != "--seed")
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("retrox viewer")
            .with_inner_size([1500.0, 950.0]),
        ..Default::default()
    };
    eframe::run_native(
        "retrox viewer",
        options,
        Box::new(move |_cc| Ok(Box::new(Viewer::new(seed)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Faction,
    Population,
    Price,
    Shortage,
}

struct Viewer {
    state: GalaxyState,
    seed: u64,
    running: bool,
    days_per_frame: u32,
    last_tick_ms: f64,
    mode: ColorMode,
    good: Good,
    selected: Option<SystemId>,
    /// Galaxy light-years → screen points.
    zoom: f32,
    /// Screen offset of the galactic centre from the canvas centre.
    pan: Vec2,
}

impl Viewer {
    fn new(seed: u64) -> Self {
        Self {
            state: generate(seed, &GalaxyConfig::default()),
            seed,
            running: false,
            days_per_frame: 1,
            last_tick_ms: 0.0,
            mode: ColorMode::Faction,
            good: Good::Food,
            selected: None,
            zoom: 1.6,
            pan: Vec2::ZERO,
        }
    }

    fn to_screen(&self, canvas: Rect, x: f64, y: f64) -> Pos2 {
        canvas.center() + self.pan + Vec2::new(x as f32, y as f32) * self.zoom
    }

    /// A system's most populous settlement, whose market stands in for the
    /// system on the map.
    fn main_settlement(&self, id: SystemId) -> Option<&Settlement> {
        self.state
            .settlements_in(id)
            .map(|(_, s)| s)
            .reduce(|a, b| {
                if b.economy.population > a.economy.population {
                    b
                } else {
                    a
                }
            })
    }

    fn population(&self, id: SystemId) -> f64 {
        self.state
            .settlements_in(id)
            .map(|(_, s)| s.economy.population)
            .sum()
    }

    fn system_color(&self, id: SystemId) -> Color32 {
        if self.mode == ColorMode::Faction {
            // Nobody owns a system; colour it by its largest faction.
            return match self.state.largest_faction_in(id) {
                Some(f) => {
                    let [r, g, b] = self.state.factions[f].color;
                    Color32::from_rgb(r, g, b)
                }
                None => Color32::from_gray(90),
            };
        }
        if self.mode == ColorMode::Population {
            let p = self.population(id);
            return if p <= 0.0 {
                Color32::from_gray(50)
            } else {
                ramp(((p.ln() + 1.0) / 6.5) as f32)
            };
        }
        let Some(main) = self.main_settlement(id) else {
            return Color32::from_gray(50);
        };
        let market = &main.economy.market;
        match self.mode {
            ColorMode::Faction | ColorMode::Population => unreachable!(),
            ColorMode::Price => {
                // log2(price / base): -2 (cheap, blue) .. +2 (dear, red).
                let rel = (market.price[self.good] / self.good.base_price()).log2();
                diverging((rel / 2.0) as f32)
            }
            ColorMode::Shortage => {
                let demand = market.demand[self.good];
                if demand <= 0.0 {
                    Color32::from_gray(50)
                } else {
                    let unmet = market.shortfall[self.good] / demand;
                    if unmet <= 1e-6 {
                        Color32::from_rgb(70, 110, 90)
                    } else {
                        ramp(0.5 + unmet as f32 * 0.5)
                    }
                }
            }
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("Galaxy {}", self.state.time));
        ui.label(format!("seed {}", self.seed));
        ui.horizontal(|ui| {
            if ui
                .button(if self.running { "⏸ Pause" } else { "▶ Run" })
                .clicked()
            {
                self.running = !self.running;
            }
            if ui.button("Step day").clicked() {
                self.step(1);
            }
            if ui.button("Step month").clicked() {
                self.step(30);
            }
        });
        ui.add(egui::Slider::new(&mut self.days_per_frame, 1..=30).text("days / frame"));
        ui.label(format!("last frame: {:.1} ms of sim", self.last_tick_ms));

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Colour by");
            egui::ComboBox::from_id_salt("mode")
                .selected_text(match self.mode {
                    ColorMode::Faction => "faction",
                    ColorMode::Population => "population",
                    ColorMode::Price => "price",
                    ColorMode::Shortage => "shortage",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.mode, ColorMode::Faction, "faction");
                    ui.selectable_value(&mut self.mode, ColorMode::Population, "population");
                    ui.selectable_value(&mut self.mode, ColorMode::Price, "price");
                    ui.selectable_value(&mut self.mode, ColorMode::Shortage, "shortage");
                });
        });
        if matches!(self.mode, ColorMode::Price | ColorMode::Shortage) {
            ui.horizontal(|ui| {
                ui.label("Good");
                egui::ComboBox::from_id_salt("good")
                    .selected_text(self.good.name())
                    .show_ui(ui, |ui| {
                        for good in Good::ALL {
                            ui.selectable_value(&mut self.good, good, good.name());
                        }
                    });
            });
        }

        ui.separator();
        ui.strong("Galaxy economy");
        let e = summarize(&self.state);
        ui.label(format!("population {:.0}", e.population));
        egui::Grid::new("galaxy_econ").striped(true).show(ui, |ui| {
            for h in ["good", "stock", "short/d", "price", "#short"] {
                ui.strong(h);
            }
            ui.end_row();
            for good in Good::ALL {
                ui.label(good.name());
                ui.label(format!("{:.0}", e.stock[good]));
                ui.label(format!("{:.0}", e.shortfall[good]));
                ui.label(format!("{:.2}", e.mean_price[good]));
                ui.label(format!("{}", e.markets_short[good]));
                ui.end_row();
            }
        });

        if let Some(id) = self.selected {
            ui.separator();
            self.system_panel(ui, id);
        }
    }

    fn system_panel(&self, ui: &mut egui::Ui, id: SystemId) {
        let s = &self.state.systems[id];
        ui.strong(&s.name);
        ui.label(format!(
            "{:?} star · {} lanes · {} bodies",
            s.star,
            s.lanes.len(),
            s.bodies.len()
        ));
        for &b in &s.bodies {
            let body = &self.state.bodies[b];
            ui.label(format!(
                "  {} — {:?}, {:.2} AU, hab {:.2}, ore {:.2}, ice {:.2}{}",
                body.name,
                body.kind,
                body.orbit.semi_major_au,
                body.habitability,
                body.resources.ore,
                body.resources.ice,
                if body.regions.is_empty() {
                    String::new()
                } else {
                    format!(", {} regions", body.regions.len())
                }
            ));
        }
        if s.settlements.is_empty() {
            ui.label("no settlements");
        }
        for (sid, settlement) in self.state.settlements_in(id) {
            ui.separator();
            let faction = &self.state.factions[settlement.owner];
            let model = match faction.economy {
                EconomyModel::Planned => "planned",
                EconomyModel::Market => "market",
            };
            let capital = if faction.capital == sid {
                " · capital"
            } else {
                ""
            };
            let site = match settlement.site {
                Site::Region(r) => {
                    let region = &self.state.regions[r];
                    format!("region of {}", self.state.bodies[region.body].name)
                }
                Site::Surface(b) => format!("on {}", self.state.bodies[b].name),
                Site::Space(_) => "station".to_owned(),
            };
            ui.strong(format!("{} ({site})", settlement.name));
            ui.label(format!("{} ({model}){capital}", faction.name));
            self.settlement_panel(ui, settlement);
        }
    }

    fn settlement_panel(&self, ui: &mut egui::Ui, s: &Settlement) {
        ui.label(format!("population {:.1}", s.economy.population));
        for f in &s.economy.facilities {
            ui.label(format!("  {:?} ×{:.1}", f.kind, f.scale));
        }
        let m = &s.economy.market;
        egui::Grid::new(("settlement_market", &s.name))
            .striped(true)
            .show(ui, |ui| {
                for h in ["good", "stock", "target", "price", "short/d"] {
                    ui.strong(h);
                }
                ui.end_row();
                for good in Good::ALL {
                    ui.label(good.name());
                    ui.label(format!("{:.0}", m.stock[good]));
                    ui.label(format!("{:.0}", m.target(good)));
                    ui.label(format!("{:.2}", m.price[good]));
                    ui.label(format!("{:.1}", m.shortfall[good]));
                    ui.end_row();
                }
            });
    }

    fn step(&mut self, days: u32) {
        let start = Instant::now();
        for _ in 0..days {
            tick(&mut self.state);
        }
        self.last_tick_ms = start.elapsed().as_secs_f64() * 1000.0;
    }

    fn map(&mut self, ui: &mut egui::Ui) {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = response.rect;
        painter.rect_filled(canvas, 0.0, Color32::from_rgb(11, 13, 18));

        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.002).exp();
            if (factor - 1.0).abs() > f32::EPSILON
                && let Some(cursor) = response.hover_pos()
            {
                // Zoom around the cursor.
                let before = cursor - canvas.center() - self.pan;
                self.zoom = (self.zoom * factor).clamp(0.3, 60.0);
                self.pan = cursor - canvas.center() - before * factor;
            }
        }

        let lane_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 85, 61, 70));
        for lane in self.state.lanes.as_slice() {
            let (a, b) = (
                self.state.systems[lane.a].position,
                self.state.systems[lane.b].position,
            );
            let (pa, pb) = (
                self.to_screen(canvas, a.x, a.y),
                self.to_screen(canvas, b.x, b.y),
            );
            if canvas.contains(pa) || canvas.contains(pb) {
                painter.line_segment([pa, pb], lane_stroke);
            }
        }

        let radius = (self.zoom * 0.9).clamp(1.5, 7.0);
        let mut hovered: Option<(SystemId, f32)> = None;
        let cursor = response.hover_pos();
        for (id, s) in self.state.systems.iter() {
            let p = self.to_screen(canvas, s.position.x, s.position.y);
            if !canvas.contains(p) {
                continue;
            }
            painter.circle_filled(p, radius, self.system_color(id));
            if let Some(c) = cursor {
                let d = c.distance(p);
                if d < 10.0 && hovered.is_none_or(|(_, best)| d < best) {
                    hovered = Some((id, d));
                }
            }
        }

        let mut holdings: std::collections::HashMap<retrox_sim::FactionId, usize> =
            std::collections::HashMap::new();
        for s in self.state.settlements.values() {
            *holdings.entry(s.owner).or_default() += 1;
        }
        for (fid, faction) in &self.state.factions {
            // Ring the powers' capitals, not every independent outpost.
            if holdings.get(&fid).copied().unwrap_or(0) < 2 {
                continue;
            }
            let capital = &self.state.settlements[faction.capital];
            let c = self.state.systems[capital.system].position;
            let p = self.to_screen(canvas, c.x, c.y);
            painter.circle_stroke(p, radius + 3.0, Stroke::new(1.5, Color32::from_gray(230)));
            if self.zoom > 2.5 {
                painter.text(
                    p - Vec2::new(0.0, radius + 6.0),
                    egui::Align2::CENTER_BOTTOM,
                    &faction.name,
                    egui::FontId::monospace(12.0),
                    Color32::from_gray(230),
                );
            }
        }

        if let Some(id) = self.selected {
            let s = self.state.systems[id].position;
            let p = self.to_screen(canvas, s.x, s.y);
            painter.circle_stroke(p, radius + 6.0, Stroke::new(2.0, Color32::YELLOW));
        }

        if let Some((id, _)) = hovered {
            let s = &self.state.systems[id];
            let price = self.main_settlement(id).map_or(String::from("-"), |m| {
                format!("{:.2}", m.economy.market.price[self.good])
            });
            response.clone().on_hover_text(format!(
                "{}\npop {:.1}\n{} {price}",
                s.name,
                self.population(id),
                self.good.name(),
            ));
            if response.clicked() {
                self.selected = Some(id);
            }
        }
    }
}

impl eframe::App for Viewer {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.running {
            self.step(self.days_per_frame);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::right("controls")
            .min_size(340.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.map(ui));
    }
}

/// Dark → amber → white, for "more is more" values in 0..1.
fn ramp(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32, t: f32| (a + (b - a) * t) as u8;
    if t < 0.5 {
        let u = t * 2.0;
        Color32::from_rgb(
            lerp(60.0, 230.0, u),
            lerp(40.0, 140.0, u),
            lerp(70.0, 40.0, u),
        )
    } else {
        let u = (t - 0.5) * 2.0;
        Color32::from_rgb(
            lerp(230.0, 255.0, u),
            lerp(140.0, 245.0, u),
            lerp(40.0, 220.0, u),
        )
    }
}

/// Blue (negative) ← grey (zero) → red (positive), for values in -1..1.
fn diverging(t: f32) -> Color32 {
    let t = t.clamp(-1.0, 1.0);
    let lerp = |a: f32, b: f32, t: f32| (a + (b - a) * t) as u8;
    if t < 0.0 {
        let u = -t;
        Color32::from_rgb(
            lerp(150.0, 60.0, u),
            lerp(150.0, 130.0, u),
            lerp(150.0, 240.0, u),
        )
    } else {
        Color32::from_rgb(
            lerp(150.0, 240.0, t),
            lerp(150.0, 70.0, t),
            lerp(150.0, 50.0, t),
        )
    }
}
