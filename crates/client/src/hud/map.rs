//! The galaxy map: every system and hyperlane in 3D, seen from a camera
//! orbiting a focus point. Drag to orbit, right-drag (or shift-drag) to
//! pan, scroll to zoom, click a system to plot a course to it,
//! double-click to fly the camera there. The next hop of the course can be
//! jumped from here.
//!
//! Galaxy coordinates are light-years with the disc in x/y and z up; the
//! disc is only a few light-years thick, so lines drop from systems near
//! the focus to the galactic plane to show their height.

use egui::{Align2, Color32, PointerButton, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec3;
use retrox_sim::{GalaxyPos, StarClass, SystemId};

use super::*;

/// Where the map camera is. Kept between frames in egui's memory.
#[derive(Clone, Copy)]
struct View {
    /// The point the camera orbits and looks at (galaxy ly).
    focus: DVec3,
    /// Heading around the galactic pole, radians.
    yaw: f64,
    /// Elevation above the galactic plane, radians.
    pitch: f64,
    /// Distance from the camera to the focus, ly.
    dist: f64,
    /// The system the ship was in when the view was set up; when the ship
    /// moves on, the map re-centres on its new home.
    home: SystemId,
}

const MIN_DIST: f64 = 4.0;
const MAX_DIST: f64 = 1200.0;
const MAX_PITCH: f64 = 1.55;
const FOV_Y: f64 = 0.9;
const NEAR: f64 = 0.05;

/// A perspective camera, built fresh from the view each frame.
struct Camera {
    eye: DVec3,
    forward: DVec3,
    right: DVec3,
    up: DVec3,
    /// Screen points per unit of (lateral offset / depth).
    focal: f64,
    centre: Pos2,
}

impl Camera {
    fn new(view: &View, canvas: Rect) -> Self {
        let (sy, cy) = view.yaw.sin_cos();
        let (sp, cp) = view.pitch.sin_cos();
        let back = DVec3::new(cp * cy, cp * sy, sp);
        let eye = view.focus + back * view.dist;
        let forward = -back;
        let right = forward.cross(DVec3::Z).normalize_or(DVec3::X);
        let up = right.cross(forward);
        Self {
            eye,
            forward,
            right,
            up,
            focal: f64::from(canvas.height()) / 2.0 / (FOV_Y / 2.0).tan(),
            centre: canvas.center(),
        }
    }

    /// Screen position and depth (ly along the view axis), if in front.
    fn project(&self, p: DVec3) -> Option<(Pos2, f64)> {
        let d = p - self.eye;
        let depth = d.dot(self.forward);
        if depth < NEAR {
            return None;
        }
        let k = self.focal / depth;
        Some((
            self.centre + Vec2::new((d.dot(self.right) * k) as f32, (-d.dot(self.up) * k) as f32),
            depth,
        ))
    }

    /// A segment's ends, clipped to in front of the camera.
    fn segment(&self, a: DVec3, b: DVec3) -> Option<[Pos2; 2]> {
        let (da, db) = (
            (a - self.eye).dot(self.forward),
            (b - self.eye).dot(self.forward),
        );
        if da < NEAR && db < NEAR {
            return None;
        }
        let clip = |p: DVec3, dp: f64, q: DVec3, dq: f64| {
            if dp >= NEAR {
                p
            } else {
                p + (q - p) * ((NEAR - dp) / (dq - dp))
            }
        };
        let (a2, b2) = (clip(a, da, b, db), clip(b, db, a, da));
        Some([self.project(a2)?.0, self.project(b2)?.0])
    }
}

fn galaxy_point(p: GalaxyPos) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

pub(super) fn draw(ctx: &egui::Context, w: &World, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let id = egui::Id::unique("galaxy_map_view");
    let here = galaxy_point(w.galaxy.systems[w.system].position);
    let mut view = ctx
        .data(|d| d.get_temp::<View>(id))
        .filter(|v| v.home == w.system)
        .unwrap_or(View {
            focus: here,
            // Look inward from the rim, toward the core.
            yaw: here.y.atan2(here.x),
            pitch: 0.6,
            dist: 70.0,
            home: w.system,
        });

    let route = w.course.and_then(|c| w.route(c)).unwrap_or_default();
    let next_hop = route.get(1).copied();
    let neighbours: Vec<SystemId> = w.lanes().iter().map(|l| l.to).collect();
    let mut hovered: Option<SystemId> = None;

    egui::Area::new(egui::Id::unique("galaxy_map"))
        .order(egui::Order::Background)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(screen.size(), Sense::click_and_drag());
            let canvas = response.rect;
            painter.rect_filled(canvas, 0.0, Color32::from_rgb(9, 8, 14));

            let shift = ui.input(|i| i.modifiers.shift);
            let delta = response.drag_delta();
            let panning = response.dragged_by(PointerButton::Secondary)
                || response.dragged_by(PointerButton::Middle)
                || (shift && response.dragged_by(PointerButton::Primary));
            if panning {
                // Slide the focus along the galactic plane.
                let cam = Camera::new(&view, canvas);
                let ahead = DVec3::new(cam.forward.x, cam.forward.y, 0.0).normalize_or(cam.up);
                let per_point = view.dist / cam.focal;
                view.focus +=
                    (-cam.right * f64::from(delta.x) + ahead * f64::from(delta.y)) * per_point;
            } else if response.dragged_by(PointerButton::Primary) {
                view.yaw -= f64::from(delta.x) * 0.006;
                view.pitch = (view.pitch + f64::from(delta.y) * 0.006).clamp(-MAX_PITCH, MAX_PITCH);
            }
            if response.hover_pos().is_some() {
                let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                let factor = f64::from(pinch * (scroll * 0.002).exp());
                if (factor - 1.0).abs() > f64::EPSILON {
                    view.dist = (view.dist / factor).clamp(MIN_DIST, MAX_DIST);
                }
            }

            let cam = Camera::new(&view, canvas);
            let pos = |s: SystemId| galaxy_point(w.galaxy.systems[s].position);
            let visible = canvas.expand(40.0);

            grid(&painter, &cam, &view, canvas);

            // Lanes: faint in general, brighter out of here, amber on the route.
            let on_route = |a: SystemId, b: SystemId| {
                route
                    .windows(2)
                    .any(|h| (h[0] == a && h[1] == b) || (h[0] == b && h[1] == a))
            };
            let mut route_lanes = Vec::new();
            for lane in w.galaxy.lanes.as_slice() {
                let Some(seg) = cam.segment(pos(lane.a), pos(lane.b)) else {
                    continue;
                };
                if !visible.intersects(Rect::from_two_pos(seg[0], seg[1])) {
                    continue;
                }
                if on_route(lane.a, lane.b) {
                    route_lanes.push(seg);
                    continue;
                }
                let stroke = if lane.a == w.system || lane.b == w.system {
                    Stroke::new(1.5, CREAM_DIM)
                } else {
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(196, 44, 34, 80))
                };
                painter.line_segment(seg, stroke);
            }
            for seg in route_lanes {
                painter.line_segment(seg, Stroke::new(2.5, AMBER));
            }

            // Height lines: down to the galactic plane for the systems that
            // matter and for everything near the focus.
            let important = |s: SystemId| {
                s == w.system
                    || Some(s) == w.course
                    || route.contains(&s)
                    || neighbours.contains(&s)
            };
            let near_radius = view.dist * 0.6;
            for (sid, system) in w.galaxy.systems.iter() {
                let p = galaxy_point(system.position);
                let near = (p - view.focus).length() < near_radius;
                if !(near || important(sid)) {
                    continue;
                }
                let foot = DVec3::new(p.x, p.y, 0.0);
                let Some(seg) = cam.segment(p, foot) else {
                    continue;
                };
                let color = if important(sid) {
                    CREAM_DIM.gamma_multiply(0.6)
                } else {
                    RULE.gamma_multiply(0.5)
                };
                painter.line_segment(seg, Stroke::new(1.0, color));
                painter.circle_filled(seg[1], 1.2, color);
            }

            // Systems, far to near, coloured by their largest faction and
            // sized and dimmed by distance.
            let mut dots: Vec<(f64, Pos2, SystemId)> = w
                .galaxy
                .systems
                .iter()
                .filter_map(|(sid, s)| {
                    let (p, depth) = cam.project(galaxy_point(s.position))?;
                    visible.contains(p).then_some((depth, p, sid))
                })
                .collect();
            dots.sort_by(|a, b| b.0.total_cmp(&a.0));
            let radius_at = |depth: f64| ((cam.focal * 0.45 / depth) as f32).clamp(1.0, 7.0);
            let cursor = response.hover_pos();
            let mut best = f32::INFINITY;
            let mut screen_of: std::collections::HashMap<SystemId, (Pos2, f32)> =
                std::collections::HashMap::with_capacity(dots.len());
            for &(depth, p, sid) in &dots {
                let r = radius_at(depth);
                let color = match w.galaxy.largest_faction_in(sid) {
                    Some(f) => {
                        let [r, g, b] = w.galaxy.factions[f].color;
                        Color32::from_rgb(r, g, b)
                    }
                    None => Color32::from_gray(110),
                };
                let fade = (1.6 - depth / (view.dist * 2.5)).clamp(0.35, 1.0) as f32;
                painter.circle_filled(p, r, color.gamma_multiply(fade));
                screen_of.insert(sid, (p, r));
                if let Some(c) = cursor {
                    let d = c.distance(p);
                    // Nearer systems win ties, since they're drawn on top.
                    if d < 10.0_f32.max(r + 4.0) && d <= best {
                        best = d;
                        hovered = Some(sid);
                    }
                }
            }

            // Markers: you, the course's destination, the hovered system.
            if let Some(&(me, r)) = screen_of.get(&w.system) {
                painter.circle_stroke(me, r + 5.0, Stroke::new(2.0, VFD));
                painter.circle_stroke(me, r + 9.0, Stroke::new(1.0, VFD.gamma_multiply(0.5)));
            }
            if let Some(&(p, r)) = w.course.and_then(|d| screen_of.get(&d)) {
                brackets(
                    &painter,
                    Rect::from_center_size(p, Vec2::splat(r * 2.0 + 16.0)),
                    5.0,
                    AMBER,
                );
            }
            if let Some(&(p, r)) = hovered.and_then(|h| screen_of.get(&h)) {
                painter.circle_stroke(p, r + 4.0, Stroke::new(1.5, CREAM));
            }

            // Labels, most important first, skipping any that would overlap.
            let mut wanted: Vec<(SystemId, Color32, f32)> = vec![(w.system, VFD, 14.0)];
            wanted.extend(hovered.map(|h| (h, CREAM, 13.0)));
            wanted.extend(w.course.map(|c| (c, AMBER, 13.0)));
            wanted.extend(route.iter().map(|&s| (s, AMBER.gamma_multiply(0.8), 11.0)));
            wanted.extend(neighbours.iter().map(|&s| (s, CREAM, 11.0)));
            // Then whatever's near the focus, nearest the camera first.
            wanted.extend(
                dots.iter()
                    .rev()
                    .filter(|(depth, ..)| *depth < view.dist * 1.3)
                    .map(|&(.., sid)| (sid, CREAM_DIM.gamma_multiply(0.8), 10.0)),
            );
            let mut taken: Vec<Rect> = Vec::new();
            let mut done: Vec<SystemId> = Vec::new();
            for (sid, color, size) in wanted {
                if done.contains(&sid) || taken.len() > 120 {
                    continue;
                }
                let Some(&(p, r)) = screen_of.get(&sid) else {
                    continue;
                };
                if !canvas.contains(p) {
                    continue;
                }
                let galley = painter.layout_no_wrap(
                    w.galaxy.systems[sid].name.to_uppercase(),
                    mono(size),
                    color,
                );
                let rect = Rect::from_min_size(
                    p + Vec2::new(r + 6.0, -galley.size().y / 2.0),
                    galley.size(),
                )
                .expand(1.0);
                if taken.iter().any(|t| t.intersects(rect)) {
                    continue;
                }
                taken.push(rect);
                done.push(sid);
                painter.galley(rect.min + Vec2::splat(1.0), galley, color);
            }

            if let Some(h) = hovered {
                if response.double_clicked() {
                    view.focus = pos(h);
                } else if response.clicked() {
                    actions.push(Action::PlotCourse(Some(h)));
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            painter.text(
                Pos2::new(canvas.left() + 24.0, canvas.bottom() - 14.0),
                Align2::LEFT_BOTTOM,
                "drag to orbit · right-drag or shift-drag to pan · scroll to zoom · \
                 click a system to plot a course · double-click to look at it · M or ESC to close",
                mono(11.0),
                CREAM_DIM,
            );

            // Header, top left.
            let band =
                Rect::from_min_size(canvas.min + Vec2::new(18.0, 14.0), Vec2::new(300.0, 30.0));
            title_band(&painter, band, "Galaxy map", None, false);
        });

    sidebar(ctx, w, hovered, &route, next_hop, &mut view, actions);
    ctx.data_mut(|d| d.insert_temp(id, view));
}

fn sidebar(
    ctx: &egui::Context,
    w: &World,
    hovered: Option<SystemId>,
    route: &[SystemId],
    next_hop: Option<SystemId>,
    view: &mut View,
    actions: &mut Vec<Action>,
) {
    panel(
        ctx,
        Panel {
            id: "galaxy_map_info",
            title: "Course",
            anchor: Align2::RIGHT_TOP,
            offset: [-16.0, 16.0],
            width: 360.0,
            collapsible: None,
        },
        |ui| {
            let shown = hovered.or(w.course).unwrap_or(w.system);
            system_info(ui, w, shown);
            ui.add_space(8.0);
            section(ui, "Plotted course");
            match w.course {
                None => {
                    ui.label(RichText::new("none · click a system on the map").color(CREAM_DIM));
                }
                Some(dest) => {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} jumps · {:.0} d in transit",
                            w.galaxy.systems[dest].name,
                            route.len().saturating_sub(1),
                            w.route_days(route)
                        ))
                        .color(AMBER),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for (i, &s) in route.iter().enumerate().skip(1) {
                                let marker = if i == 1 { "▶" } else { " " };
                                ui.label(
                                    RichText::new(format!(
                                        "{marker} {i:>2}. {}",
                                        w.galaxy.systems[s].name
                                    ))
                                    .color(if i == 1 {
                                        CREAM
                                    } else {
                                        CREAM_DIM
                                    }),
                                );
                            }
                        });
                }
            }
            ui.add_space(8.0);
            let open_space = w.mode == Mode::Normal && w.current_well().is_none();
            ui.horizontal(|ui| {
                if let Some(next) = next_hop {
                    let jump = egui::Button::new(
                        RichText::new(format!(
                            "JUMP · {}",
                            truncate(&w.galaxy.systems[next].name, 14)
                        ))
                        .font(display(20.0))
                        .color(CREAM),
                    )
                    .fill(RED)
                    .min_size(Vec2::new(0.0, 28.0));
                    if ui.add_enabled(open_space, jump).clicked() {
                        actions.push(Action::Jump(next));
                    }
                }
                if w.course.is_some()
                    && ui
                        .add(egui::Button::new("CLEAR").min_size(Vec2::new(0.0, 28.0)))
                        .clicked()
                {
                    actions.push(Action::PlotCourse(None));
                }
                if ui
                    .add(egui::Button::new("CENTRE").min_size(Vec2::new(0.0, 28.0)))
                    .clicked()
                {
                    let p = w.galaxy.systems[w.system].position;
                    view.focus = galaxy_point(p);
                }
            });
            if next_hop.is_some() && !open_space {
                ui.label(
                    RichText::new(match w.mode {
                        Mode::Normal => "leave the gravity well to jump",
                        _ => "launch and leave the gravity well to jump",
                    })
                    .color(AMBER),
                );
            }
        },
    );
}

fn system_info(ui: &mut egui::Ui, w: &World, s: SystemId) {
    let system = &w.galaxy.systems[s];
    ui.label(
        RichText::new(system.name.to_uppercase())
            .font(display(32.0))
            .color(if s == w.system { VFD } else { CREAM }),
    );
    let owner = match w.galaxy.largest_faction_in(s) {
        Some(f) => w.galaxy.factions[f].name.clone(),
        None => "uninhabited".into(),
    };
    ui.label(RichText::new(owner).color(CREAM_DIM));
    let population: f64 = w
        .galaxy
        .settlements_in(s)
        .map(|(_, st)| st.economy.population)
        .sum();
    let mut facts = format!(
        "{} star · {} lanes · {} settlements",
        star_name(system.star),
        system.lanes.len(),
        system.settlements.len()
    );
    if population > 0.0 {
        facts += &format!(" · pop {population:.1}k");
    }
    ui.label(RichText::new(facts).color(CREAM_DIM));
    let here = w.galaxy.systems[w.system].position;
    let away = if s == w.system {
        "you are here".to_owned()
    } else {
        match w.route(s) {
            Some(route) => format!(
                "{:.0} ly away · {} jumps · {:.0} d",
                here.distance(system.position),
                route.len() - 1,
                w.route_days(&route)
            ),
            None => format!(
                "{:.0} ly away · no hyperlane route",
                here.distance(system.position)
            ),
        }
    };
    glow_label(ui, &away, mono(14.0), VFD);
}

fn star_name(class: StarClass) -> &'static str {
    use StarClass::*;
    match class {
        O => "O",
        B => "B",
        A => "A",
        F => "F",
        G => "G",
        K => "K",
        M => "M",
        WhiteDwarf => "white dwarf",
        Neutron => "neutron",
        BlackHole => "black hole",
    }
}

/// A square grid on the galactic plane around the focus, with lines every
/// 1/2/5 × 10ⁿ ly, whichever suits the zoom; plus a marker at the core.
fn grid(painter: &egui::Painter, cam: &Camera, view: &View, canvas: Rect) {
    let step = f64::from(nice_length((view.dist / 8.0) as f32));
    let reach = (view.dist * 1.5 / step).ceil() * step;
    let color = Color32::from_rgba_unmultiplied(110, 98, 84, 40);
    let (cx, cy) = (
        (view.focus.x / step).round() * step,
        (view.focus.y / step).round() * step,
    );
    let mut k = -reach;
    while k <= reach {
        for (a, b) in [
            (
                DVec3::new(cx + k, cy - reach, 0.0),
                DVec3::new(cx + k, cy + reach, 0.0),
            ),
            (
                DVec3::new(cx - reach, cy + k, 0.0),
                DVec3::new(cx + reach, cy + k, 0.0),
            ),
        ] {
            if let Some(seg) = cam.segment(a, b) {
                painter.line_segment(seg, Stroke::new(1.0, color));
            }
        }
        k += step;
    }
    painter.text(
        Pos2::new(canvas.left() + 24.0, canvas.bottom() - 32.0),
        Align2::LEFT_BOTTOM,
        format!("GRID {step} LY · CAMERA {:.0} LY OUT", view.dist),
        mono(11.0),
        CREAM_DIM,
    );
    if let Some((core, _)) = cam.project(DVec3::ZERO) {
        painter.circle_stroke(core, 6.0, Stroke::new(1.0, CREAM_DIM));
        painter.text(
            core + Vec2::new(10.0, 0.0),
            Align2::LEFT_CENTER,
            "GALACTIC CORE",
            mono(10.0),
            CREAM_DIM,
        );
    }
}

/// The 1/2/5 × 10ⁿ length nearest to (and not above) `ly`.
fn nice_length(ly: f32) -> f32 {
    let mag = 10f32.powf(ly.max(1e-3).log10().floor());
    [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|&v| v <= ly)
        .unwrap_or(mag)
}
