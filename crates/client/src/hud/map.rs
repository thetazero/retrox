//! The galaxy map, built as an electromechanical star-chart instrument
//! (DESIGN §8): the galaxy is seen through a round chart glass set in an
//! enamelled console, with a bearing ring that turns as the view orbits,
//! elevation and range scales with pointers, and drum counters for the
//! chart drive. Drag to orbit, right-drag (or shift-drag) to pan, scroll to
//! change range, click a system to plot a course to it, double-click to
//! slew the chart there. The next hop of the course can be jumped from here.
//!
//! Galaxy coordinates are light-years with the disc in x/y and z up; the
//! disc is only a few light-years thick. The chart plots on the plane level
//! with its focus: range rings and bearing spokes round the focus, and
//! lines from the systems that matter down (or up) to it to show their
//! height. Those systems are drawn at full strength and the rest of the
//! galaxy faint behind them (issue #192), and a search box finds a
//! system by name.

use std::f32::consts::{PI, TAU};

use egui::epaint::{Mesh, TextShape};
use egui::{Align2, Color32, PointerButton, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec3;
use retrox_sim::{GalaxyPos, StarClass, SystemId};

use super::*;

/// Where the chart is pointed. Kept between frames in egui's memory.
#[derive(Clone, Copy)]
struct View {
    /// The point the camera orbits and looks at (galaxy ly).
    focus: DVec3,
    /// Where the chart drive is slewing the focus to, if anywhere.
    goal: Option<DVec3>,
    /// The range it is opening out to as it slews, if any.
    goal_dist: Option<f64>,
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
/// Field of view across the chart glass's diameter.
const FOV: f64 = 0.9;
const NEAR: f64 = 0.05;

/// Console enamel, and its highlight and shadow for bevels and seams.
const BEZEL: Color32 = Color32::from_rgb(27, 24, 29);
const BEZEL_HI: Color32 = Color32::from_rgb(52, 46, 50);
const BEZEL_LO: Color32 = Color32::from_rgb(10, 9, 12);
/// The machined bearing ring.
const RING: Color32 = Color32::from_rgb(36, 32, 36);
/// The chart glass: a faint phosphor teal at the centre, near black at the
/// rim.
const GLASS_MID: Color32 = Color32::from_rgb(13, 24, 26);
const GLASS_RIM: Color32 = Color32::from_rgb(5, 6, 9);
/// Range rings and bearing spokes on the plane of the focus.
const PLOT: Color32 = Color32::from_rgba_premultiplied(13, 28, 25, 30);

/// Where the instrument's parts sit on screen.
struct Layout {
    /// The middle of the chart glass.
    centre: Pos2,
    /// The glass's radius.
    r: f32,
}

impl Layout {
    /// Readouts on the left, the course panel on the right, and the chart
    /// glass as large as fits between them, with room for its scales.
    fn new(screen: Rect) -> Self {
        let area = Rect::from_min_max(
            Pos2::new(screen.left() + 236.0, screen.top() + 64.0),
            Pos2::new(screen.right() - 392.0, screen.bottom() - 52.0),
        );
        let r = (area.width() / 2.0 - 104.0)
            .min(area.height() / 2.0 - 46.0)
            .max(140.0);
        Self {
            centre: area.center(),
            r,
        }
    }

    fn ring_in(&self) -> f32 {
        self.r + 5.0
    }

    fn ring_out(&self) -> f32 {
        self.r + 36.0
    }

    /// Radius of the elevation and range scales outside the ring.
    fn scale(&self) -> f32 {
        self.r + 58.0
    }

    /// The point at `theta` radians clockwise from the top, `radius` out.
    fn at(&self, theta: f32, radius: f32) -> Pos2 {
        self.centre + dir(theta) * radius
    }
}

/// Unit vector `theta` radians clockwise from straight up.
fn dir(theta: f32) -> Vec2 {
    Vec2::new(theta.sin(), -theta.cos())
}

/// A perspective camera, built fresh from the view each frame, projecting
/// onto the chart glass.
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
    fn new(view: &View, layout: &Layout) -> Self {
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
            focal: f64::from(layout.r) / (FOV / 2.0).tan(),
            centre: layout.centre,
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

    /// The bearing (radians, galactic frame, anticlockwise from +x) the
    /// camera is looking along, across the galactic plane.
    fn heading(&self) -> f64 {
        self.forward.y.atan2(self.forward.x)
    }
}

fn galaxy_point(p: GalaxyPos) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

/// Bearing in whole degrees, 0..360.
fn degrees(rad: f64) -> f64 {
    rad.to_degrees().rem_euclid(360.0)
}

pub(super) fn draw(ctx: &egui::Context, w: &World, actions: &mut Vec<Action>) {
    let screen = ctx.content_rect();
    let layout = Layout::new(screen);
    let id = egui::Id::unique("galaxy_map_view");
    let here = galaxy_point(w.galaxy.systems[w.system].position);
    let route = w.course.and_then(|c| w.route(c)).unwrap_or_default();
    let neighbours: Vec<SystemId> = w.lanes().iter().map(|l| l.to).collect();
    let contracts = contract_systems(w);
    let mut view = ctx
        .data(|d| d.get_temp::<View>(id))
        .filter(|v| v.home == w.system)
        .unwrap_or_else(|| View {
            focus: here,
            goal: None,
            goal_dist: None,
            // Look inward from the rim, toward the core.
            yaw: here.y.atan2(here.x),
            pitch: 0.6,
            dist: opening_range(w, here, &neighbours, &contracts),
            home: w.system,
        });

    // The chart drive slews the focus toward its goal, easing in.
    if let Some(goal) = view.goal {
        let dt = f64::from(ctx.input(|i| i.stable_dt)).min(0.1);
        view.focus += (goal - view.focus) * (1.0 - (-dt * 5.0).exp());
        if let Some(d) = view.goal_dist {
            view.dist += (d - view.dist) * (1.0 - (-dt * 5.0).exp());
        }
        if (goal - view.focus).length() < view.dist * 1e-3 {
            view.focus = goal;
            view.goal = None;
            view.dist = view.goal_dist.take().unwrap_or(view.dist);
        }
        ctx.request_repaint();
    }

    let next_hop = route.get(1).copied();
    let mut hovered: Option<SystemId> = None;
    // The search box's text, and the systems it finds.
    let query_id = egui::Id::unique("galaxy_map_query");
    let mut query: String = ctx.data(|d| d.get_temp(query_id)).unwrap_or_default();
    let found = search(w, &query, FOUND_MARKED);
    // The shipyards panel's systems, marked on the glass; and the one
    // whose row the pointer was on last frame.
    let found_yards = w.nearest(crate::world::Service::Shipyard, YARDS_LISTED);
    let yard_systems: Vec<SystemId> = found_yards.iter().map(|y| y.system).collect();
    let pointed_id = egui::Id::unique("galaxy_map_yard_pointed");
    let pointed = ctx
        .data(|d| d.get_temp::<Option<SystemId>>(pointed_id))
        .flatten();

    egui::Area::new(egui::Id::unique("galaxy_map"))
        .order(egui::Order::Background)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(screen.size(), Sense::click_and_drag());
            let canvas = response.rect;
            painter.rect_filled(canvas, 0.0, BEZEL);

            let shift = ui.input(|i| i.modifiers.shift);
            let delta = response.drag_delta();
            let panning = response.dragged_by(PointerButton::Secondary)
                || response.dragged_by(PointerButton::Middle)
                || (shift && response.dragged_by(PointerButton::Primary));
            if panning {
                // Slide the focus along the galactic plane.
                let cam = Camera::new(&view, &layout);
                let ahead = DVec3::new(cam.forward.x, cam.forward.y, 0.0).normalize_or(cam.up);
                let per_point = view.dist / cam.focal;
                view.focus +=
                    (-cam.right * f64::from(delta.x) + ahead * f64::from(delta.y)) * per_point;
                view.goal = None;
                view.goal_dist = None;
            } else if response.dragged_by(PointerButton::Primary) {
                view.yaw -= f64::from(delta.x) * 0.006;
                view.pitch = (view.pitch + f64::from(delta.y) * 0.006).clamp(-MAX_PITCH, MAX_PITCH);
            }
            if response.hover_pos().is_some() {
                let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                let factor = f64::from(pinch * (scroll * 0.002).exp());
                if (factor - 1.0).abs() > f64::EPSILON {
                    view.dist = (view.dist / factor).clamp(MIN_DIST, MAX_DIST);
                    view.goal_dist = None;
                }
            }

            let cam = Camera::new(&view, &layout);
            let (centre, r) = (layout.centre, layout.r);

            // The glass, then the chart on it, clipped to the glass's square
            // (the bezel covers the corners).
            ring_mesh(&painter, centre, 0.0, r * 0.55, GLASS_MID, GLASS_MID);
            ring_mesh(&painter, centre, r * 0.55, r, GLASS_MID, GLASS_RIM);
            let glass =
                painter.with_clip_rect(Rect::from_center_size(centre, Vec2::splat(r * 2.0)));
            let ring_step = plane_plot(&glass, &cam, &view);
            let marks = Marks {
                route: &route,
                neighbours: &neighbours,
                contracts: &contracts,
                yards: &yard_systems,
                found: &found,
                pointed,
            };
            hovered = chart(
                &glass,
                &cam,
                &view,
                &layout,
                w,
                &marks,
                response.hover_pos(),
            );
            crosshair(&glass, centre, r);
            // Darken the glass toward its rim, and a faint reflection.
            for k in 0..10 {
                let t = k as f32 / 10.0;
                glass.circle_stroke(
                    centre,
                    r - 1.0 - t * 18.0,
                    Stroke::new(2.0, Color32::from_black_alpha((70.0 * (1.0 - t)) as u8)),
                );
            }
            arc(
                &glass,
                centre,
                r * 0.9,
                -1.25,
                -0.35,
                Stroke::new(10.0, Color32::from_white_alpha(4)),
            );
            arc(
                &glass,
                centre,
                r * 0.9,
                -1.1,
                -0.5,
                Stroke::new(4.0, Color32::from_white_alpha(5)),
            );

            // The bezel, over the glass's corners, and the instrument round it.
            ring_mesh(&painter, centre, r, r * 1.45, BEZEL, BEZEL);
            bearing_ring(&painter, &layout, cam.heading());
            elevation_scale(&painter, &layout, view.pitch);
            range_scale(&painter, &layout, view.dist);
            painter.text(
                layout.at(PI, layout.ring_out() + 10.0),
                Align2::CENTER_TOP,
                format!("PLANE OF FOCUS · RINGS {} LY", ly(ring_step)),
                mono(11.0),
                CREAM_DIM,
            );
            readouts(
                &painter,
                canvas,
                &view,
                (view.focus - here).length() < view.dist * 1e-2,
            );
            legend(&painter, canvas);
            title_band(
                &painter,
                Rect::from_min_size(canvas.min + Vec2::new(18.0, 14.0), Vec2::new(300.0, 30.0)),
                "Star chart",
                None,
                false,
            );

            if let Some(h) = hovered {
                let pos = galaxy_point(w.galaxy.systems[h].position);
                if response.double_clicked() {
                    view.goal = Some(pos);
                } else if response.clicked() {
                    actions.push(Action::PlotCourse(Some(h)));
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });

    sidebar(ctx, w, hovered, &route, next_hop, &mut view, actions);
    let yard_pointed = yards_panel(ctx, w, &found_yards, actions);
    let find_pointed = find_panel(ctx, w, &mut query, &contracts, &mut view, actions);
    ctx.data_mut(|d| {
        d.insert_temp(id, view);
        d.insert_temp(pointed_id, yard_pointed.or(find_pointed));
        d.insert_temp(query_id, query);
    });
}

/// The systems that matter to what the player is doing, drawn at full
/// strength on the chart; everything else fades into the background
/// (issue #192).
struct Marks<'a> {
    /// The plotted course, from here to its destination.
    route: &'a [SystemId],
    /// One jump away: what's in reach.
    neighbours: &'a [SystemId],
    /// Where the player's contracts are bound.
    contracts: &'a [ContractStop],
    /// The shipyards in their panel.
    yards: &'a [SystemId],
    /// What the search box finds.
    found: &'a [SystemId],
    /// The system of the panel row under the pointer.
    pointed: Option<SystemId>,
}

impl Marks<'_> {
    fn key(&self, w: &World, s: SystemId) -> bool {
        s == w.system
            || Some(s) == w.course
            || self.pointed == Some(s)
            || self.route.contains(&s)
            || self.neighbours.contains(&s)
            || self.contracts.iter().any(|c| c.system == s)
            || self.yards.contains(&s)
            || self.found.contains(&s)
    }
}

/// How strongly the systems and lanes in the background are drawn.
const BACKGROUND: f32 = 0.34;

/// Lanes, routes, height lines, systems and their markers and labels, on
/// the glass. Returns the system under the cursor, if any.
fn chart(
    painter: &egui::Painter,
    cam: &Camera,
    view: &View,
    layout: &Layout,
    w: &World,
    marks: &Marks,
    cursor: Option<Pos2>,
) -> Option<SystemId> {
    let pos = |s: SystemId| galaxy_point(w.galaxy.systems[s].position);
    let visible = Rect::from_center_size(layout.centre, Vec2::splat(layout.r * 2.0)).expand(40.0);
    let on_glass = |p: Pos2, margin: f32| p.distance(layout.centre) < layout.r - margin;
    let cursor = cursor.filter(|&c| on_glass(c, 0.0));
    let key = |s: SystemId| marks.key(w, s);

    // Systems on screen, far to near, and the one under the cursor.
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
    let radius_at = |depth: f64| ((cam.focal * 0.4 / depth) as f32).clamp(1.0, 6.0);
    let mut best = f32::INFINITY;
    let mut hovered = None;
    let mut screen_of: std::collections::HashMap<SystemId, (Pos2, f32)> =
        std::collections::HashMap::with_capacity(dots.len());
    for &(depth, p, sid) in &dots {
        let r = radius_at(depth);
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

    // Lanes: a faint web in the background; the lanes out of here (what a
    // jump can take) in cream; the course in amber.
    let on_route = |a: SystemId, b: SystemId| {
        marks
            .route
            .windows(2)
            .any(|h| (h[0] == a && h[1] == b) || (h[0] == b && h[1] == a))
    };
    let mut near_lanes = Vec::new();
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
        } else if lane.a == w.system || lane.b == w.system {
            near_lanes.push(seg);
        } else {
            painter.line_segment(seg, Stroke::new(1.0, RED.gamma_multiply(0.14)));
        }
    }
    for seg in near_lanes {
        painter.line_segment(seg, Stroke::new(1.5, CREAM_DIM));
    }

    // The way to each contract's destination, when it isn't the course,
    // and to the system under the cursor: dashed, so it reads as a way
    // there rather than a course set.
    let dashed = |path: &[SystemId], stroke: Stroke| {
        for hop in path.windows(2) {
            if let Some(seg) = cam.segment(pos(hop[0]), pos(hop[1])) {
                painter.extend(egui::Shape::dashed_line(&seg, stroke, 6.0, 4.0));
            }
        }
    };
    let mut shown: Vec<SystemId> = Vec::new();
    for c in marks.contracts {
        if Some(c.system) == w.course || shown.contains(&c.system) {
            continue;
        }
        shown.push(c.system);
        if let Some(path) = w.route(c.system) {
            dashed(&path, Stroke::new(1.5, AMBER.gamma_multiply(0.6)));
        }
    }
    for seg in route_lanes {
        painter.line_segment(seg, Stroke::new(5.0, AMBER.gamma_multiply(0.18)));
        painter.line_segment(seg, Stroke::new(2.0, AMBER));
    }
    if let Some(h) = hovered.filter(|&h| h != w.system && Some(h) != w.course)
        && let Some(path) = w.route(h)
    {
        dashed(&path, Stroke::new(1.5, CREAM));
    }

    // Height lines to the chart's plane, for the systems that matter.
    for (sid, system) in w.galaxy.systems.iter() {
        if !(key(sid) || hovered == Some(sid)) {
            continue;
        }
        let p = galaxy_point(system.position);
        let foot = DVec3::new(p.x, p.y, view.focus.z);
        let Some(seg) = cam.segment(p, foot) else {
            continue;
        };
        let color = CREAM_DIM.gamma_multiply(0.6);
        painter.line_segment(seg, Stroke::new(1.0, color));
        painter.circle_filled(seg[1], 1.2, color);
    }

    // Systems, far to near, coloured by their largest faction and sized and
    // dimmed by distance: the ones that matter as lit points with a halo,
    // the rest small and faint.
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
        if key(sid) || hovered == Some(sid) {
            painter.circle_filled(p, r * 2.4, color.gamma_multiply(0.10 * fade));
            painter.circle_filled(p, r, color.gamma_multiply(fade));
            if r > 3.0 {
                painter.circle_filled(p, r * 0.4, CREAM.gamma_multiply(0.7 * fade));
            }
        } else {
            painter.circle_filled(p, r * 0.75, color.gamma_multiply(BACKGROUND * fade));
        }
    }

    // Markers: you, the course's destination, contracts, shipyards, the
    // hovered system.
    if let Some(&(me, r)) = screen_of.get(&w.system) {
        painter.circle_stroke(me, r + 5.0, Stroke::new(2.0, VFD));
        painter.circle_stroke(me, r + 9.0, Stroke::new(1.0, VFD.gamma_multiply(0.5)));
    }
    if let Some(&(p, r)) = w.course.and_then(|d| screen_of.get(&d)) {
        brackets(
            painter,
            Rect::from_center_size(p, Vec2::splat(r * 2.0 + 16.0)),
            5.0,
            AMBER,
        );
    }
    // A contract's destination: an amber diamond, as cargo is tagged amber.
    for c in marks.contracts {
        if let Some(&(p, r)) = screen_of.get(&c.system) {
            let d = r + 9.0;
            painter.add(egui::Shape::closed_line(
                vec![
                    p + Vec2::new(0.0, -d),
                    p + Vec2::new(d, 0.0),
                    p + Vec2::new(0.0, d),
                    p + Vec2::new(-d, 0.0),
                ],
                Stroke::new(1.5, AMBER),
            ));
        }
    }
    // Shipyards in the panel: a cream square round each (issue #161),
    // bright for the one whose row the pointer is on.
    for &y in marks.yards {
        if let Some(&(p, r)) = screen_of.get(&y) {
            let (side, ink) = if marks.pointed == Some(y) {
                (r * 2.0 + 14.0, CREAM)
            } else {
                (r * 2.0 + 9.0, CREAM.gamma_multiply(0.75))
            };
            painter.rect_stroke(
                Rect::from_center_size(p, Vec2::splat(side)),
                0.0,
                Stroke::new(1.2, ink),
                egui::StrokeKind::Middle,
            );
        }
    }
    for &s in marks.found.iter().chain(&marks.pointed) {
        if let Some(&(p, r)) = screen_of.get(&s) {
            painter.circle_stroke(p, r + 4.0, Stroke::new(1.0, CREAM.gamma_multiply(0.7)));
        }
    }
    if let Some(&(p, r)) = hovered.and_then(|h| screen_of.get(&h)) {
        painter.circle_stroke(p, r + 4.0, Stroke::new(1.5, CREAM));
    }

    // Labels, most important first, skipping any that would overlap.
    let mut wanted: Vec<(SystemId, Color32, f32, Option<&str>)> = vec![(w.system, VFD, 14.0, None)];
    wanted.extend(hovered.map(|h| (h, CREAM, 13.0, None)));
    wanted.extend(marks.pointed.map(|p| (p, CREAM, 13.0, None)));
    wanted.extend(w.course.map(|c| (c, AMBER, 13.0, None)));
    wanted.extend(
        marks
            .contracts
            .iter()
            .map(|c| (c.system, AMBER, 12.0, Some("CONTRACT"))),
    );
    wanted.extend(marks.found.iter().map(|&s| (s, CREAM, 12.0, None)));
    wanted.extend(
        marks
            .route
            .iter()
            .map(|&s| (s, AMBER.gamma_multiply(0.8), 11.0, None)),
    );
    wanted.extend(marks.neighbours.iter().map(|&s| (s, CREAM, 11.0, None)));
    wanted.extend(marks.yards.iter().map(|&s| (s, CREAM, 11.0, None)));
    // Then a few of the background's names, nearest the middle of the
    // glass, faint, so the chart can still be read for where things are.
    let mut background: Vec<(f32, SystemId)> = dots
        .iter()
        .filter(|&&(depth, p, sid)| depth < view.dist * 2.0 && !key(sid) && on_glass(p, 12.0))
        .map(|&(_, p, sid)| (p.distance(layout.centre), sid))
        .collect();
    background.sort_by(|a, b| a.0.total_cmp(&b.0));
    wanted.extend(
        background
            .iter()
            .take(BACKGROUND_NAMES)
            .map(|&(_, sid)| (sid, CREAM_DIM.gamma_multiply(0.5), 10.0, None)),
    );
    let mut taken: Vec<Rect> = Vec::new();
    let mut done: Vec<SystemId> = Vec::new();
    for (sid, color, size, tag) in wanted {
        if done.contains(&sid) {
            continue;
        }
        let Some(&(p, r)) = screen_of.get(&sid) else {
            continue;
        };
        if !on_glass(p, 12.0) {
            continue;
        }
        let galley =
            painter.layout_no_wrap(w.galaxy.systems[sid].name.to_uppercase(), mono(size), color);
        // A contract's destination keeps its tag under any other mark.
        let tag = tag
            .or_else(|| {
                marks
                    .contracts
                    .iter()
                    .any(|c| c.system == sid)
                    .then_some("CONTRACT")
            })
            .map(|t| painter.layout_no_wrap(t.to_owned(), mono(9.0), color));
        let tag_w = tag.as_ref().map_or(0.0, |t| t.size().x + 14.0);
        let size = galley.size() + Vec2::new(tag_w, 0.0);
        let mut rect = Rect::from_min_size(p + Vec2::new(r + 6.0, -size.y / 2.0), size).expand(1.0);
        // Near the rim, write it on the dot's other side, whole.
        if !on_glass(rect.right_center(), 4.0) {
            rect = rect.translate(Vec2::new(-(rect.width() + 2.0 * r + 10.0), 0.0));
        }
        if taken.iter().any(|t| t.intersects(rect)) {
            continue;
        }
        taken.push(rect);
        done.push(sid);
        let name_w = galley.size().x;
        painter.galley(rect.min + Vec2::splat(1.0), galley, color);
        if let Some(tag) = tag {
            let at = Pos2::new(rect.min.x + 1.0 + name_w + 8.0, p.y - tag.size().y / 2.0);
            let boxed = Rect::from_min_size(at, tag.size()).expand2(Vec2::new(3.0, 1.0));
            painter.rect_stroke(
                boxed,
                0.0,
                Stroke::new(1.0, color),
                egui::StrokeKind::Middle,
            );
            painter.galley(at, tag, color);
        }
    }
    hovered
}

/// Faint names of background systems written on the glass.
const BACKGROUND_NAMES: usize = 14;

/// A system a contract is bound for, and the settlement there.
#[derive(Clone, Copy)]
struct ContractStop {
    system: SystemId,
    to: retrox_sim::SettlementId,
}

/// Where the player's contracts are bound, in the order they were taken.
fn contract_systems(w: &World) -> Vec<ContractStop> {
    w.player
        .contracts
        .iter()
        .map(|c| ContractStop {
            system: w.galaxy.settlements[c.offer.to].system,
            to: c.offer.to,
        })
        .collect()
}

/// The chart's range on opening: enough to take in the systems in reach,
/// the course and the contracts' destinations, and no more, so they fill
/// the glass rather than sitting in a crowd.
fn opening_range(
    w: &World,
    here: DVec3,
    neighbours: &[SystemId],
    contracts: &[ContractStop],
) -> f64 {
    let pos = |s: SystemId| galaxy_point(w.galaxy.systems[s].position);
    let reach = neighbours
        .iter()
        .copied()
        .chain(contracts.iter().map(|c| c.system))
        .chain(w.course)
        .map(|s| (pos(s) - here).length())
        .fold(0.0, f64::max);
    (reach * 3.2).clamp(20.0, 400.0)
}

/// Systems marked on the chart while the search box has text.
const FOUND_MARKED: usize = 24;
/// Systems listed under the search box.
const FOUND_LISTED: usize = 6;

/// Systems whose name holds `query`, ignoring case and accents: names
/// starting with it first, then a word in them starting with it, then the
/// rest; nearest first within each.
fn search(w: &World, query: &str, max: usize) -> Vec<SystemId> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let here = w.galaxy.systems[w.system].position;
    let mut hits: Vec<(u8, f64, SystemId)> = w
        .galaxy
        .systems
        .iter()
        .filter_map(|(sid, s)| {
            let name = fold(&s.name);
            let rank = if name.starts_with(&q) {
                0
            } else if name.split([' ', '-']).any(|word| word.starts_with(&q)) {
                1
            } else if name.contains(&q) {
                2
            } else {
                return None;
            };
            Some((rank, here.distance(s.position), sid))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    hits.into_iter().take(max).map(|(.., s)| s).collect()
}

/// Lowercase, with accents dropped, for matching names as typed.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'è' | 'é' | 'ê' | 'ë' | 'ē' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ý' | 'ÿ' => 'y',
            c => c,
        })
        .collect()
}

/// Range rings and bearing spokes round the focus, on the plane level with
/// it (parallel to the galactic plane),
/// every 1/2/5 × 10ⁿ ly, whichever suits the range; plus a marker at the
/// core. Returns the ring spacing.
fn plane_plot(painter: &egui::Painter, cam: &Camera, view: &View) -> f64 {
    let step = f64::from(nice_length((view.dist / 4.0) as f32));
    let rings = ((view.dist * 1.6 / step).ceil() as usize).clamp(2, 12);
    let centre = view.focus;
    let reach = step * rings as f64;
    let stroke = Stroke::new(1.0, PLOT);
    let n = 96;
    for k in 1..=rings {
        let radius = step * k as f64;
        let on_ring = |i: usize| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            centre + DVec3::new(a.cos(), a.sin(), 0.0) * radius
        };
        let heavy = k % 5 == 0;
        for i in 0..n {
            if let Some(seg) = cam.segment(on_ring(i), on_ring(i + 1)) {
                painter.line_segment(
                    seg,
                    if heavy {
                        Stroke::new(1.5, PLOT.gamma_multiply(1.6))
                    } else {
                        stroke
                    },
                );
            }
        }
        // Range, written where the ring crosses the camera's right.
        let side = DVec3::new(cam.right.x, cam.right.y, 0.0).normalize_or(DVec3::X);
        if k <= 4
            && let Some((p, _)) = cam.project(centre + side * radius)
        {
            painter.text(
                p + Vec2::new(3.0, -2.0),
                Align2::LEFT_BOTTOM,
                format!("{} LY", ly(radius)),
                mono(10.0),
                VFD.gamma_multiply(0.35),
            );
        }
    }
    for b in (0..360).step_by(30) {
        let a = f64::from(b).to_radians();
        let tip = centre + DVec3::new(a.cos(), a.sin(), 0.0) * reach;
        if let Some(seg) = cam.segment(centre, tip) {
            painter.line_segment(seg, stroke);
        }
    }
    if let Some((core, _)) = cam.project(DVec3::ZERO) {
        painter.circle_stroke(core, 6.0, Stroke::new(1.0, CREAM_DIM));
        painter.circle_stroke(core, 10.0, Stroke::new(1.0, CREAM_DIM.gamma_multiply(0.4)));
        painter.text(
            core + Vec2::new(14.0, 0.0),
            Align2::LEFT_CENTER,
            "GALACTIC CORE",
            mono(10.0),
            CREAM_DIM,
        );
    }
    step
}

/// A fine cross at the chart's focus, broken in the middle, with ticks
/// running out to the rim.
fn crosshair(painter: &egui::Painter, c: Pos2, r: f32) {
    let stroke = Stroke::new(1.0, CREAM.gamma_multiply(0.22));
    for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
        painter.line_segment(
            [c + d * 8.0, c + d * 22.0],
            Stroke::new(1.0, CREAM.gamma_multiply(0.5)),
        );
        painter.line_segment([c + d * 30.0, c + d * (r - 4.0)], stroke);
        let n = 20;
        for k in 1..n {
            let at = c + d * (r * k as f32 / n as f32);
            let side = d.rot90() * if k % 5 == 0 { 5.0 } else { 2.5 };
            painter.line_segment([at - side, at + side], stroke);
        }
    }
}

/// The bearing ring round the glass: a machined band graduated in degrees,
/// turned so the fixed index at the top reads the camera's heading.
fn bearing_ring(painter: &egui::Painter, l: &Layout, heading: f64) {
    let (r0, r1) = (l.ring_in(), l.ring_out());
    ring_mesh(painter, l.centre, r0, r1, RING, RING);
    // Turned grooves.
    for (radius, color) in [
        (r0 + 13.0, BEZEL_LO),
        (r0 + 14.0, BEZEL_HI),
        (r1 - 1.0, BEZEL_HI),
    ] {
        painter.circle_stroke(l.centre, radius, Stroke::new(1.0, color));
    }
    // A dark lip where the glass sits, catching a line of light.
    painter.circle_stroke(l.centre, l.r + 2.0, Stroke::new(4.0, BEZEL_LO));
    painter.circle_stroke(
        l.centre,
        l.r + 4.5,
        Stroke::new(1.0, CREAM.gamma_multiply(0.25)),
    );

    let up = degrees(heading);
    for b in (0..360).step_by(5) {
        let theta = ((up - f64::from(b)) as f32).to_radians();
        let len = match b {
            b if b % 30 == 0 => 11.0,
            b if b % 10 == 0 => 8.0,
            _ => 5.0,
        };
        let color = if b % 30 == 0 { CREAM } else { CREAM_DIM };
        painter.line_segment(
            [l.at(theta, r1 - 2.0), l.at(theta, r1 - 2.0 - len)],
            Stroke::new(if b % 30 == 0 { 1.5 } else { 1.0 }, color),
        );
        if b % 30 == 0 {
            rotated_text(
                painter,
                l.at(theta, r0 + 7.0),
                theta,
                &format!("{b:03}"),
                mono(11.0),
                CREAM,
            );
        }
    }
    // The fixed index: a red pointer above the ring and a lubber line.
    let tip = l.at(0.0, r1 - 1.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip + Vec2::new(8.0, -13.0),
            tip + Vec2::new(-8.0, -13.0),
        ],
        RED,
        Stroke::new(1.0, RED_DARK),
    ));
    painter.line_segment(
        [l.at(0.0, l.r + 1.0), l.at(0.0, r0 + 3.0)],
        Stroke::new(2.0, RED),
    );
    painter.text(
        tip + Vec2::new(14.0, -8.0),
        Align2::LEFT_CENTER,
        format!("HDG {:03.0}°", up.floor()),
        mono(12.0),
        CREAM_DIM,
    );
}

/// The elevation scale, on the left: +90° (looking straight down on the
/// disc) at the top, edge-on in the middle.
fn elevation_scale(painter: &egui::Painter, l: &Layout, pitch: f64) {
    let theta = |deg: f32| (270.0 + deg * 0.62).to_radians();
    let radius = l.scale();
    arc(
        painter,
        l.centre,
        radius,
        theta(-90.0),
        theta(90.0),
        Stroke::new(1.5, CREAM_DIM),
    );
    for d in (-90..=90).step_by(5) {
        let t = theta(d as f32);
        let major = d % 45 == 0;
        let len = if major {
            10.0
        } else if d % 15 == 0 {
            7.0
        } else {
            4.0
        };
        painter.line_segment(
            [l.at(t, radius), l.at(t, radius + len)],
            Stroke::new(if major { 1.5 } else { 1.0 }, CREAM_DIM),
        );
        if major {
            painter.text(
                l.at(t, radius + 14.0),
                Align2::RIGHT_CENTER,
                format!("{d:+}"),
                mono(11.0),
                CREAM,
            );
        }
    }
    pointer(painter, l, theta(pitch.to_degrees() as f32), radius);
    rotated_text(
        painter,
        l.at(theta(0.0), radius + 46.0),
        -PI / 2.0,
        "ELEVATION °",
        display(16.0),
        CREAM_DIM,
    );
}

/// The range scale, on the right, logarithmic: near at the bottom, far at
/// the top.
fn range_scale(painter: &egui::Painter, l: &Layout, dist: f64) {
    let span = (MAX_DIST / MIN_DIST).ln();
    let theta = |d: f64| {
        let t = ((d / MIN_DIST).ln() / span) as f32;
        (90.0 + 56.0 - t * 112.0).to_radians()
    };
    let radius = l.scale();
    arc(
        painter,
        l.centre,
        radius,
        theta(MAX_DIST),
        theta(MIN_DIST),
        Stroke::new(1.5, CREAM_DIM),
    );
    let mut decade = 1.0;
    while decade <= MAX_DIST {
        for m in 1..10 {
            let d = decade * f64::from(m);
            if !(MIN_DIST..=MAX_DIST).contains(&d) {
                continue;
            }
            let t = theta(d);
            let major = matches!(m, 1 | 2 | 5);
            painter.line_segment(
                [
                    l.at(t, radius),
                    l.at(t, radius + if major { 10.0 } else { 4.0 }),
                ],
                Stroke::new(if major { 1.5 } else { 1.0 }, CREAM_DIM),
            );
            if major {
                painter.text(
                    l.at(t, radius + 14.0),
                    Align2::LEFT_CENTER,
                    ly(d),
                    mono(11.0),
                    CREAM,
                );
            }
        }
        decade *= 10.0;
    }
    pointer(painter, l, theta(dist), radius);
    rotated_text(
        painter,
        l.at(PI / 2.0, radius + 52.0),
        PI / 2.0,
        "RANGE LY",
        display(16.0),
        CREAM_DIM,
    );
}

/// An amber pointer riding inside a scale at `radius`, pointing out at it.
fn pointer(painter: &egui::Painter, l: &Layout, theta: f32, radius: f32) {
    let tip = l.at(theta, radius - 1.0);
    let back = l.at(theta, radius - 15.0);
    let side = dir(theta).rot90() * 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + side, back - side],
        AMBER,
        Stroke::NONE,
    ));
    painter.circle_filled(tip, 6.0, AMBER.gamma_multiply(0.12));
}

/// The chart drive's readouts, on a plate on the left of the console: drum
/// counters for range, heading, elevation and focus, and lamps for the
/// drive slewing and for the chart being centred on the ship.
fn readouts(painter: &egui::Painter, canvas: Rect, view: &View, on_ship: bool) {
    let plate = Rect::from_min_size(canvas.min + Vec2::new(18.0, 64.0), Vec2::new(196.0, 416.0));
    painter.rect_filled(plate, 0.0, PANEL);
    painter.rect_stroke(plate, 0.0, Stroke::new(1.0, RULE), egui::StrokeKind::Inside);
    for p in [
        plate.left_top() + Vec2::new(9.0, 9.0),
        plate.right_top() + Vec2::new(-9.0, 9.0),
        plate.left_bottom() + Vec2::new(9.0, -9.0),
        plate.right_bottom() + Vec2::new(-9.0, -9.0),
    ] {
        screw(painter, p);
    }
    let x = plate.left() + 22.0;
    let mut y = plate.top() + 20.0;
    let heading = degrees(view.yaw + std::f64::consts::PI);
    let rows: [(&str, f64, usize, bool, &str); 3] = [
        ("RANGE", view.dist, 4, false, "LY"),
        ("HEADING", heading, 3, false, "°"),
        ("ELEVATION", view.pitch.to_degrees(), 2, true, "°"),
    ];
    heading_rule(
        painter,
        Pos2::new(x, y),
        plate.right() - 22.0,
        "Chart drive",
    );
    y += 30.0;
    for (label, value, digits, signed, unit) in rows {
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_TOP,
            label,
            mono(11.0),
            CREAM_DIM,
        );
        let r = drum(painter, Pos2::new(x, y + 16.0), value, digits, signed);
        painter.text(
            Pos2::new(r.right() + 8.0, r.center().y),
            Align2::LEFT_CENTER,
            unit,
            mono(13.0),
            CREAM_DIM,
        );
        y += 56.0;
    }
    y += 6.0;
    heading_rule(painter, Pos2::new(x, y), plate.right() - 22.0, "Focus · ly");
    y += 30.0;
    for (axis, value) in [
        ("X", view.focus.x),
        ("Y", view.focus.y),
        ("Z", view.focus.z),
    ] {
        painter.text(
            Pos2::new(x, y + 12.0),
            Align2::LEFT_CENTER,
            axis,
            display(20.0),
            CREAM_DIM,
        );
        drum(painter, Pos2::new(x + 24.0, y), value, 4, true);
        y += 36.0;
    }
    y += 8.0;
    let lx = crate::widgets::lamp_at(
        painter,
        Pos2::new(x, y),
        "SLEW",
        view.goal.is_some(),
        AMBER,
        None,
    )
    .right()
        + 8.0;
    crate::widgets::lamp_at(painter, Pos2::new(lx, y), "ON SHIP", on_ship, VFD, None);
}

/// A sub-heading painted straight onto a panel, with a rule to `right`.
fn heading_rule(painter: &egui::Painter, at: Pos2, right: f32, title: &str) {
    let r = painter.text(
        at,
        Align2::LEFT_TOP,
        title.to_uppercase(),
        display(20.0),
        CREAM_DIM,
    );
    painter.line_segment(
        [
            Pos2::new(r.right() + 8.0, r.center().y),
            Pos2::new(right, r.center().y),
        ],
        Stroke::new(1.0, RULE),
    );
}

/// A mechanical drum counter at `at` showing `value`: cream digits on black
/// drums, the low digit rolling continuously and each higher one turning
/// over as the one below it passes nine. Returns the counter's rect.
pub(crate) fn drum(
    painter: &egui::Painter,
    at: Pos2,
    value: f64,
    digits: usize,
    signed: bool,
) -> Rect {
    let size = Vec2::new(17.0, 24.0);
    let windows = digits + usize::from(signed);
    let whole = Rect::from_min_size(at, Vec2::new(size.x * windows as f32, size.y));
    painter.rect_filled(whole.expand(3.0), 2.0, BEZEL_LO);
    painter.rect_stroke(
        whole.expand(3.0),
        2.0,
        Stroke::new(1.0, BEZEL_HI),
        egui::StrokeKind::Outside,
    );
    let window = |i: usize| Rect::from_min_size(at + Vec2::new(size.x * i as f32, 0.0), size);
    let face = |painter: &egui::Painter, rect: Rect| {
        painter.rect_filled(rect.shrink2(Vec2::new(1.0, 0.0)), 0.0, INK);
    };
    let shade = |painter: &egui::Painter, rect: Rect| {
        // The drums curve away top and bottom.
        for (h, a) in [(3.0, 150), (6.0, 70), (9.0, 30)] {
            painter.rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(rect.width(), h)),
                0.0,
                Color32::from_black_alpha(a),
            );
            painter.rect_filled(
                Rect::from_min_size(
                    rect.left_bottom() - Vec2::new(0.0, h),
                    Vec2::new(rect.width(), h),
                ),
                0.0,
                Color32::from_black_alpha(a),
            );
        }
    };
    let font = mono(19.0);
    if signed {
        let rect = window(0);
        face(painter, rect);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            if value < -0.5 { "−" } else { "+" },
            font.clone(),
            CREAM_DIM,
        );
        shade(painter, rect);
    }
    let top = 10f64.powi(digits as i32);
    let v = value.abs().min(top - 1e-6);
    for place in 0..digits {
        let rect = window(windows - 1 - place);
        face(painter, rect);
        let unit = 10f64.powi(place as i32);
        let digit = ((v / unit).floor() % 10.0) as u32;
        // Each drum sits on its detent and only turns over in the last
        // stretch before the next whole number, carrying the ones above.
        let carry = ((v.fract() - 0.9) / 0.1).clamp(0.0, 1.0);
        let roll = if place == 0 || (v % unit) >= unit - 1.0 {
            carry
        } else {
            0.0
        } as f32;
        let clip = painter.with_clip_rect(rect);
        for (d, offset) in [(digit, -roll), ((digit + 1) % 10, 1.0 - roll)] {
            clip.text(
                rect.center() + Vec2::new(0.0, offset * size.y),
                Align2::CENTER_CENTER,
                d.to_string(),
                font.clone(),
                CREAM,
            );
        }
        shade(painter, rect);
    }
    whole
}

/// The controls, as keycaps along the bottom of the console.
fn legend(painter: &egui::Painter, canvas: Rect) {
    let mut x = canvas.left() + 24.0;
    let y = canvas.bottom() - 30.0;
    for (key, what) in [
        ("DRAG", "orbit"),
        ("R-DRAG", "pan"),
        ("WHEEL", "range"),
        ("CLICK", "plot course"),
        ("DBL-CLICK", "slew to"),
        ("/", "find system"),
        ("M", "close"),
    ] {
        x = crate::widgets::legend_item(painter, Pos2::new(x, y + 9.0), key, what);
    }
}

/// A slotted screw head.
fn screw(painter: &egui::Painter, p: Pos2) {
    painter.circle_filled(p, 4.5, BEZEL_HI);
    painter.circle_stroke(p, 4.5, Stroke::new(1.0, BEZEL_LO));
    let d = Vec2::angled(0.7) * 3.5;
    painter.line_segment([p - d, p + d], Stroke::new(1.5, BEZEL_LO));
}

/// A flat-shaded ring from `r0` to `r1`, its colour running from `inner` to
/// `outer` (a disc when `r0` is zero).
fn ring_mesh(painter: &egui::Painter, c: Pos2, r0: f32, r1: f32, inner: Color32, outer: Color32) {
    let n = 128;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        let d = dir(i as f32 / n as f32 * TAU);
        mesh.colored_vertex(c + d * r0, inner);
        mesh.colored_vertex(c + d * r1, outer);
    }
    for i in 0..n {
        let k = 2 * i as u32;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(mesh);
}

/// An arc from `t0` to `t1` (radians clockwise from the top).
fn arc(painter: &egui::Painter, c: Pos2, r: f32, t0: f32, t1: f32, stroke: Stroke) {
    let n = ((t1 - t0).abs() * r / 4.0).ceil().max(2.0) as usize;
    let points = (0..=n)
        .map(|i| c + dir(t0 + (t1 - t0) * i as f32 / n as f32) * r)
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

/// Text centred on `at`, turned `angle` radians clockwise.
fn rotated_text(
    painter: &egui::Painter,
    at: Pos2,
    angle: f32,
    text: &str,
    font: egui::FontId,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let pos = at - galley.size() / 2.0;
    painter.add(
        TextShape::new(pos, galley, color).with_angle_and_anchor(angle, Align2::CENTER_CENTER),
    );
}

/// A distance in light-years, without trailing zeros.
fn ly(d: f64) -> String {
    if d >= 1.0 {
        format!("{d:.0}")
    } else {
        format!("{d}")
    }
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
                    ui.label(RichText::new("none · click a system on the chart").color(CREAM_DIM));
                }
                Some(dest) => {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {:.0} d in transit",
                            w.galaxy.systems[dest].name,
                            jumps_text(route.len().saturating_sub(1) as u32),
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
            // The jump interlock: course plotted, and clear of any well.
            ui.horizontal(|ui| {
                lamp(ui, "COURSE SET", w.course.is_some(), AMBER);
                lamp(ui, "WELL CLEAR", open_space, VFD);
                lamp(ui, "JUMP READY", next_hop.is_some() && open_space, RED_HOT);
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if let Some(next) = next_hop {
                    let text = format!(
                        "JUMP · {}",
                        crate::widgets::truncate(&w.galaxy.systems[next].name, 14)
                    );
                    if crate::widgets::push(
                        ui,
                        &text.to_uppercase(),
                        display(20.0),
                        crate::widgets::Look::Primary,
                        open_space,
                        Vec2::new(0.0, 30.0),
                    )
                    .clicked()
                    {
                        actions.push(Action::Jump(next));
                    }
                }
                let plain = crate::widgets::Look::Plain;
                if w.course.is_some()
                    && crate::widgets::push(
                        ui,
                        "CLEAR",
                        mono(14.0),
                        plain,
                        true,
                        Vec2::new(0.0, 30.0),
                    )
                    .clicked()
                {
                    actions.push(Action::PlotCourse(None));
                }
                if crate::widgets::push(ui, "CENTRE", mono(14.0), plain, true, Vec2::new(0.0, 30.0))
                    .clicked()
                {
                    view.goal = Some(galaxy_point(w.galaxy.systems[w.system].position));
                }
            });
            if next_hop.is_some() && !open_space {
                let how = match w.mode {
                    Mode::Normal => {
                        "Jumps need open space: close the chart [M], then [J] climbs out of the gravity well."
                    }
                    _ => "Jumps need open space: launch, then [J] climbs out of the gravity well.",
                };
                crate::hud::guide::rich_hint(ui, how, AMBER);
            }
        },
    );
}

/// The search box, under the chart drive's readouts (issue #192): type a
/// system's name and the matches light up on the chart and list here;
/// a click on one (or Enter for the first) plots a course there and slews
/// the chart to it. With the box empty, the contracts' destinations are
/// listed instead, the places the player most often looks for. Returns the
/// system of the row under the pointer, to mark on the glass.
fn find_panel(
    ctx: &egui::Context,
    w: &World,
    query: &mut String,
    contracts: &[ContractStop],
    view: &mut View,
    actions: &mut Vec<Action>,
) -> Option<SystemId> {
    let mut pointed = None;
    let mut chosen = None;
    panel(
        ctx,
        Panel {
            id: "galaxy_map_find",
            title: "Find system",
            anchor: Align2::LEFT_TOP,
            offset: [18.0, 496.0],
            width: 196.0,
            collapsible: None,
        },
        |ui| {
            let edit_id = egui::Id::unique("galaxy_map_find_edit");
            let typing = ui.memory(|m| m.has_focus(edit_id));
            // [/] puts the cursor in the box from anywhere on the chart.
            if !typing && ui.input(|i| i.key_pressed(egui::Key::Slash)) {
                ui.memory_mut(|m| m.request_focus(edit_id));
            }
            let edit = egui::TextEdit::singleline(query)
                .id(edit_id)
                .font(mono(15.0))
                .text_color(CREAM)
                .hint_text(RichText::new("type a name").color(CREAM_DIM.gamma_multiply(0.6)))
                .desired_width(f32::INFINITY)
                .margin(Vec2::new(8.0, 5.0));
            let response = ui.add(edit);
            // The [/] that opened the box isn't part of the name.
            if query.contains('/') {
                query.retain(|c| c != '/');
            }
            let (enter, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if response.lost_focus() && escape {
                query.clear();
            }
            let hits = search(w, query, FOUND_LISTED);
            if response.lost_focus() && enter {
                chosen = hits.first().copied();
            }
            ui.add_space(4.0);
            if query.trim().is_empty() {
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                crate::widgets::legend_item(
                    ui.painter(),
                    Pos2::new(row.left(), row.center().y),
                    "/",
                    "or click to type",
                );
                if contracts.is_empty() {
                    return;
                }
                ui.add_space(4.0);
                section(ui, "Contracts");
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut listed: Vec<SystemId> = Vec::new();
                for c in contracts {
                    if listed.contains(&c.system) {
                        continue;
                    }
                    listed.push(c.system);
                    let to = &w.galaxy.settlements[c.to].name;
                    let (hot, clicked) = find_row(ui, w, c.system, Some(to), AMBER);
                    if hot {
                        pointed = Some(c.system);
                    }
                    if clicked {
                        chosen = Some(c.system);
                    }
                }
                return;
            }
            if hits.is_empty() {
                ui.label(RichText::new("no system by that name").color(CREAM_DIM));
                return;
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            for s in hits {
                let (hot, clicked) = find_row(ui, w, s, None, CREAM);
                if hot {
                    pointed = Some(s);
                }
                if clicked {
                    chosen = Some(s);
                }
            }
        },
    );
    if let Some(s) = chosen {
        let there = galaxy_point(w.galaxy.systems[s].position);
        let here = galaxy_point(w.galaxy.systems[w.system].position);
        view.goal = Some(there);
        // Open the range out enough to keep the ship on the glass too, so
        // the way there shows.
        let wanted = ((there - here).length() * 2.6).clamp(MIN_DIST, MAX_DIST);
        view.goal_dist = (wanted > view.dist).then_some(wanted);
        if s != w.system {
            actions.push(Action::PlotCourse(Some(s)));
        }
    }
    pointed
}

/// A system in the search panel: its name, how many jumps, and under it
/// the settlement a contract is bound for there or who holds it. Returns
/// whether the pointer is on it and whether it was clicked.
fn find_row(
    ui: &mut egui::Ui,
    w: &World,
    s: SystemId,
    contract_to: Option<&str>,
    ink: Color32,
) -> (bool, bool) {
    let (rect, response, look) =
        crate::widgets::list_row(ui, 36.0, w.course == Some(s), true, AMBER);
    let painter = ui.painter();
    let jumps = if s == w.system {
        "HERE".to_owned()
    } else {
        match w.route(s) {
            Some(r) => jumps_text(r.len() as u32 - 1).to_uppercase(),
            None => "NO ROUTE".to_owned(),
        }
    };
    let range = painter.layout_no_wrap(
        jumps,
        mono(11.0),
        if look.hot {
            CREAM
        } else {
            VFD.gamma_multiply(0.85)
        },
    );
    let x = rect.left() + 10.0;
    let top = rect.top() + 11.0;
    let room = rect.right() - 6.0 - range.size().x - 8.0 - x;
    let name = &w.galaxy.systems[s].name;
    painter.text(
        Pos2::new(x, top),
        Align2::LEFT_CENTER,
        crate::widgets::fit(painter, name, mono(13.0), room),
        mono(13.0),
        if look.hot { CREAM } else { ink },
    );
    painter.galley(
        Pos2::new(
            rect.right() - 6.0 - range.size().x,
            top - range.size().y / 2.0,
        ),
        range,
        Color32::PLACEHOLDER,
    );
    let line = match contract_to {
        Some(to) => format!("contract · {to}"),
        None => match w.galaxy.largest_faction_in(s) {
            Some(f) => w.galaxy.factions[f].name.clone(),
            None => "uninhabited".to_owned(),
        },
    };
    painter.text(
        Pos2::new(x, rect.top() + 27.0),
        Align2::LEFT_CENTER,
        crate::widgets::fit(painter, &line, mono(11.0), rect.right() - 6.0 - x),
        mono(11.0),
        CREAM_DIM,
    );
    (response.hovered(), response.clicked())
}

/// Shipyards listed in the course panel (issue #161).
const YARDS_LISTED: usize = 4;

/// The nearest places to buy a ship, in a panel of their own in the
/// console's free corner (so it holds still while the course panel above
/// changes with the pointer), as instrument rows: a click plots a course
/// there, or, in this system, targets it and closes the chart. Returns the
/// system of the row under the pointer, to mark on the glass.
fn yards_panel(
    ctx: &egui::Context,
    w: &World,
    found: &[crate::world::Nearby],
    actions: &mut Vec<Action>,
) -> Option<SystemId> {
    let mut pointed = None;
    panel(
        ctx,
        Panel {
            id: "galaxy_map_yards",
            title: "Nearest shipyards",
            anchor: Align2::RIGHT_BOTTOM,
            offset: [-16.0, -16.0],
            width: 360.0,
            collapsible: None,
        },
        |ui| {
            ui.label(
                RichText::new("ships sold and refitted · click one to plot a course")
                    .size(11.0)
                    .color(CREAM_DIM),
            );
            ui.add_space(4.0);
            if found.is_empty() {
                ui.label(RichText::new("none within 12 jumps").color(CREAM_DIM));
                return;
            }
            pointed = yard_rows(ui, w, found, actions);
        },
    );
    pointed
}

fn yard_rows(
    ui: &mut egui::Ui,
    w: &World,
    found: &[crate::world::Nearby],
    actions: &mut Vec<Action>,
) -> Option<SystemId> {
    let mut pointed = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    for &yard in found {
        let st = &w.galaxy.settlements[yard.at];
        let here = yard.system == w.system;
        let selected = if here {
            w.nav_selected().is_some_and(|(place, city)| {
                city == Some(yard.at) || place == Place::Station(yard.at)
            })
        } else {
            w.course == Some(yard.system)
        };
        let (rect, response, look) = crate::widgets::list_row(ui, 36.0, selected, true, AMBER);
        if response.hovered() {
            pointed = Some(yard.system);
        }
        if response.clicked() {
            if here {
                actions.push(match st.site {
                    retrox_sim::Site::Space(_) => Action::Target(Place::Station(yard.at)),
                    _ => Action::TargetCity(yard.at),
                });
                actions.push(Action::ToggleMap);
            } else {
                actions.push(Action::PlotCourse(Some(yard.system)));
            }
        }
        let painter = ui.painter();
        let ink = if look.selected && !look.hot {
            AMBER
        } else {
            CREAM_DIM
        };
        let glyph = Pos2::new(rect.left() + 16.0, rect.top() + 12.0);
        match st.site {
            retrox_sim::Site::Space(_) => {
                place_glyph(painter, w, Place::Station(yard.at), glyph, ink)
            }
            _ => city_glyph(painter, glyph, ink),
        }
        let range = painter.layout_no_wrap(
            jumps_text(yard.jumps).to_uppercase(),
            mono(13.0),
            if look.hot {
                CREAM
            } else if look.selected {
                AMBER.gamma_multiply(0.85)
            } else {
                VFD.gamma_multiply(0.85)
            },
        );
        let name_x = rect.left() + 32.0;
        let room = rect.right() - 6.0 - range.size().x - 10.0 - name_x;
        painter.text(
            Pos2::new(name_x, glyph.y),
            Align2::LEFT_CENTER,
            crate::widgets::fit(painter, &st.name, mono(14.0), room),
            mono(14.0),
            look.text(AMBER),
        );
        let lot = crate::hangar::lot()
            .iter()
            .filter(|o| crate::hangar::can_build(&st.economy, o))
            .count();
        let lot = match lot {
            0 => "nothing on the lot".to_owned(),
            n => format!("{} on the lot", count(n, "ship")),
        };
        let line = if here {
            lot
        } else {
            format!(
                "{} · about {:.0} d · {lot}",
                w.galaxy.systems[yard.system].name, yard.days
            )
        };
        painter.text(
            Pos2::new(name_x, rect.top() + 27.0),
            Align2::LEFT_CENTER,
            crate::widgets::fit(painter, &line, mono(11.0), rect.right() - 6.0 - name_x),
            mono(11.0),
            CREAM_DIM,
        );
        painter.galley(
            Pos2::new(
                rect.right() - 6.0 - range.size().x,
                glyph.y - range.size().y / 2.0,
            ),
            range,
            Color32::PLACEHOLDER,
        );
    }
    pointed
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
        "{} star · {} · {}",
        star_name(system.star),
        count(system.lanes.len(), "lane"),
        count(system.settlements.len(), "settlement")
    );
    if population > 0.0 {
        facts += &format!(" · pop {population:.1}k");
    }
    ui.label(RichText::new(facts).color(CREAM_DIM));
    let yards = w.count_in(s, crate::world::Service::Shipyard);
    if yards > 0 {
        ui.label(RichText::new(count(yards, "shipyard")).color(CREAM));
    }
    let here = w.galaxy.systems[w.system].position;
    let away = if s == w.system {
        "you are here".to_owned()
    } else {
        match w.route(s) {
            Some(route) => format!(
                "{:.0} ly away · {} · {:.0} d",
                here.distance(system.position),
                jumps_text(route.len() as u32 - 1),
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

/// The 1/2/5 × 10ⁿ length nearest to (and not above) `ly`.
fn nice_length(ly: f32) -> f32 {
    let mag = 10f32.powf(ly.max(1e-3).log10().floor());
    [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|&v| v <= ly)
        .unwrap_or(mag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_a_system_by_part_of_its_name() {
        let w = World::new(1);
        let (sid, system) = w
            .galaxy
            .systems
            .iter()
            .find(|(_, s)| s.name.len() > 6)
            .expect("a system");
        let name = system.name.clone();
        // Typed in lower case, the start of the name finds it first among
        // the names that start the same way.
        let typed: String = name.to_lowercase().chars().take(5).collect();
        let found = search(&w, &typed, 50);
        assert!(found.contains(&sid), "{typed} should find {name}");
        let first = &w.galaxy.systems[found[0]].name;
        assert!(
            fold(first).starts_with(&typed),
            "{first} doesn't start {typed}"
        );
        // The whole name in capitals, too.
        assert!(search(&w, &name.to_uppercase(), 5).contains(&sid));
        assert!(search(&w, "   ", 5).is_empty());
        assert!(search(&w, "zzzzqqq", 5).is_empty());
    }

    #[test]
    fn accents_and_case_fold_away() {
        assert_eq!(fold("Siöprend Gate"), "sioprend gate");
    }

    #[test]
    fn the_chart_opens_wide_enough_for_what_matters() {
        let w = World::new(1);
        let here = galaxy_point(w.galaxy.systems[w.system].position);
        let neighbours: Vec<SystemId> = w.lanes().iter().map(|l| l.to).collect();
        let range = opening_range(&w, here, &neighbours, &[]);
        for &n in &neighbours {
            let away = (galaxy_point(w.galaxy.systems[n].position) - here).length();
            assert!(range > away * 2.0, "{away} ly away, range {range}");
        }
    }
}
