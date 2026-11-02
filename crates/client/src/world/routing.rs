//! Between systems: hyperlanes, routes along them, and the jump itself.

use glam::DVec3;
use retrox_sim::{GalaxyState, SystemId};

use super::{AU, Mode, Place, REFERENCE, SECONDS_PER_DAY, SystemView, World, look_rotation};

/// A hyperlane out of the current system.
#[derive(Clone, Debug)]
pub struct LaneInfo {
    pub to: SystemId,
    pub name: String,
    pub length_ly: f64,
    pub days: f64,
}

/// Direction from one system toward another on the galaxy map, mapped into
/// a system's frame.
pub fn galaxy_direction(galaxy: &GalaxyState, here: SystemId, there: SystemId) -> DVec3 {
    let (a, b) = (
        galaxy.systems[here].position,
        galaxy.systems[there].position,
    );
    DVec3::new(b.x - a.x, b.z - a.z, b.y - a.y).normalize_or(DVec3::Z)
}

/// Galaxy days a hyperlane jump takes (DESIGN §3: 2–14 days by length).
pub fn jump_days(length_ly: f64) -> f64 {
    (length_ly * 0.6).clamp(2.0, 14.0).round()
}

impl World {
    /// Days to cross a lane, by the jump drive's speed with the ship's mass,
    /// relative to the starting ship.
    pub fn jump_days(&self, length_ly: f64) -> f64 {
        let speed = self.stats().jump_speed();
        let slower = if speed > 0.0 {
            (REFERENCE.jump_speed() / speed).clamp(0.5, 3.0)
        } else {
            1.0
        };
        (jump_days(length_ly) * slower).round().max(1.0)
    }

    /// Days the jump to `to` takes (3 if it isn't a neighbour: only
    /// test starts arrive that way).
    fn lane_days(&self, to: SystemId) -> f64 {
        self.lanes()
            .iter()
            .find(|l| l.to == to)
            .map_or(3.0, |l| l.days)
    }

    /// Mid-jump: let the galaxy clock catch up to `progress` (0..1) of the
    /// jump's days, so they tick over the sequence, not in one frame.
    pub(super) fn jump_progress(&mut self, to: SystemId, progress: f64) {
        let Some(start) = self.jump_start_s else {
            return;
        };
        let goal = start + self.lane_days(to) * SECONDS_PER_DAY * progress.clamp(0.0, 1.0);
        if goal > self.clock_s {
            self.advance_clock(goal - self.clock_s);
        }
    }

    pub fn lanes(&self) -> Vec<LaneInfo> {
        self.galaxy.systems[self.system]
            .lanes
            .iter()
            .map(|&l| {
                let lane = &self.galaxy.lanes[l];
                let to = lane.other(self.system);
                LaneInfo {
                    to,
                    name: self.galaxy.systems[to].name.clone(),
                    length_ly: lane.length_ly,
                    days: self.jump_days(lane.length_ly),
                }
            })
            .collect()
    }

    /// The quickest way along the hyperlanes from here to `to` (by days in
    /// transit): every system on the way, starting here and ending at `to`.
    pub fn route(&self, to: SystemId) -> Option<Vec<SystemId>> {
        use std::cmp::Reverse;
        use std::collections::{BinaryHeap, HashMap};
        let from = self.system;
        let mut best: HashMap<SystemId, (f64, SystemId)> = HashMap::from([(from, (0.0, from))]);
        let mut heap = BinaryHeap::from([Reverse((0.0f64.to_bits(), from))]);
        while let Some(Reverse((bits, system))) = heap.pop() {
            let days = f64::from_bits(bits);
            if best.get(&system).is_some_and(|&(b, _)| b < days) {
                continue;
            }
            if system == to {
                let mut path = vec![to];
                while let Some(&last) = path.last()
                    && last != from
                {
                    path.push(best[&last].1);
                }
                path.reverse();
                return Some(path);
            }
            for &lane in &self.galaxy.systems[system].lanes {
                let lane = &self.galaxy.lanes[lane];
                let next = lane.other(system);
                let d = days + jump_days(lane.length_ly);
                if best.get(&next).is_none_or(|&(b, _)| d < b) {
                    best.insert(next, (d, system));
                    heap.push(Reverse((d.to_bits(), next)));
                }
            }
        }
        None
    }

    /// The galaxy map can be opened: flying normally, docked, or landed.
    pub fn map_available(&self) -> bool {
        match self.mode {
            Mode::Normal | Mode::Docked(_) => true,
            Mode::Landed { timer, .. } => timer <= 0.0,
            _ => false,
        }
    }

    /// The next system on the plotted course.
    pub fn next_hop(&self) -> Option<SystemId> {
        self.route(self.course?)?.get(1).copied()
    }

    /// Total days in transit along a route (hops only, not flying to the
    /// edge of each system's well).
    pub fn route_days(&self, route: &[SystemId]) -> f64 {
        route
            .windows(2)
            .map(|hop| {
                self.galaxy.systems[hop[0]]
                    .lanes
                    .iter()
                    .map(|&l| &self.galaxy.lanes[l])
                    .find(|l| l.other(hop[0]) == hop[1])
                    .map_or(0.0, |l| jump_days(l.length_ly))
            })
            .sum()
    }

    pub(super) fn arrive_jump(&mut self, to: SystemId) {
        let from = self.system;
        let days = self.lane_days(to);
        // Whatever of the jump's days the sequence hasn't passed yet.
        let start = self.jump_start_s.take().unwrap_or(self.clock_s);
        self.advance_clock((start + days * SECONDS_PER_DAY - self.clock_s).max(0.0));

        self.system = to;
        self.view = SystemView::new(&self.galaxy, to);
        self.system_generation += 1;
        if let Some((left, _)) = self.field_rocks.take() {
            self.mining.leave(left);
        }
        self.target = None;
        if self.course == Some(to) {
            self.course = None;
        }
        // Arrive on the side facing the system we came from, outside the
        // star's well, facing the star.
        let entry = galaxy_direction(&self.galaxy, to, from);
        let distance = (self.well_radius(Place::Star) * 3.0).max(0.3 * AU);
        self.ship.frame = Place::Star;
        self.ship.rel = entry * distance;
        self.ship.rot = look_rotation(-entry);
        self.cam_rot = self.ship.rot;
        self.ship.vel = DVec3::ZERO;
        self.ship.throttle = 0.0;
        self.mode = Mode::Normal;
        let name = self.galaxy.systems[to].name.clone();
        self.say(format!("Arrived at {name} after {days} days in transit"));
    }
}
