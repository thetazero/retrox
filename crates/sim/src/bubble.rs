//! The bubble: the agent tier around the player (DESIGN §5.2).
//!
//! Within [`RADIUS_JUMPS`] jumps of the player's system, trader fleets stop
//! being scheduled events and become ships: each one flies its trip for
//! real under the [`crate::flight`] autopilot, out to a jump point, through
//! the lanes one jump at a time, and in again, a day of flight per tick.
//! The client draws them from here.
//!
//! - **Materialize.** A coarse fleet whose trip passes through the bubble
//!   becomes a [`Flight`] when it gets there: placed where its trip puts it
//!   that day, in a burn out, in the lanes, or braking in. Fleets departing
//!   from inside the bubble fly from the start.
//! - **Collapse.** A flight that jumps out of the bubble, or that the
//!   bubble leaves behind (the player jumps away, or waits and time
//!   compresses), goes back to being a coarse trip with what's left of it
//!   as its arrival event.
//! - **Conservation.** Cargo, ships and money stay on the fleet record in
//!   [`crate::haul`] the whole time; the bubble only says where the fleet
//!   is. Lane losses roll once per trip, on leaving the last lane, in
//!   both tiers.
//! - **Behaviour.** Inside the bubble a fleet plans a new route at every
//!   port it reaches instead of waiting for its monthly review, so traders
//!   near the player react to the markets the player is changing.
//!
//! There is no bubble until the client puts one somewhere with
//! [`set_centre`]; prehistory never does.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};

use serde::{Deserialize, Serialize};

use crate::flight::{self, Goal, Kinematics, Vec3};
use crate::haul::{self, CompanyId, Location, ShipClass};
use crate::settlement::{SettlementId, Site};
use crate::{GalaxyState, GalaxyTime, SystemId};

const METRES_PER_AU: f64 = 1.496e11;
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Jump points sit this far from a star, toward the system on the far end.
pub const JUMP_POINT_AU: f64 = 0.3;
/// A planet's port sits this many radii above its centre, over the pole.
const PORT_ALTITUDE_RADII: f64 = 1.5;
/// How far the bubble reaches from the player's system, in jumps.
pub const RADIUS_JUMPS: usize = 3;

/// A fleet, by company and the fleet's id within it.
pub type FleetKey = (CompanyId, u32);

/// The agent tier: where it is, and the fleets flying in it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bubble {
    /// The player's system, or `None` while there is no bubble.
    pub centre: Option<SystemId>,
    /// Every system within [`RADIUS_JUMPS`] of the centre.
    pub systems: BTreeSet<SystemId>,
    /// Fleets flying for real. Their [`haul::Fleet`] says
    /// [`Location::Flying`] while they're here.
    #[serde(with = "pairs")]
    pub flights: BTreeMap<FleetKey, Flight>,
    /// Lane distance from every system near enough to matter to the
    /// nearest bubble system: a cheap test for whether a coarse trip
    /// could pass through. Rebuilt from `systems` when empty.
    #[serde(skip)]
    near: HashMap<SystemId, f64>,
}

// JSON maps need string keys, so flights save as a list of pairs.
mod pairs {
    use super::{BTreeMap, FleetKey, Flight};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<FleetKey, Flight>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(map.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<FleetKey, Flight>, D::Error> {
        Ok(Vec::<(FleetKey, Flight)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}

/// A fleet's trip, flown for real.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flight {
    pub from: SettlementId,
    pub to: SettlementId,
    /// When the trip began, in either tier.
    pub depart: GalaxyTime,
    pub stage: Stage,
    /// Systems still to enter, in order, ending in `to`'s. Empty once the
    /// fleet is in its destination's system.
    pub path: Vec<SystemId>,
    /// The trip's lane losses are still to roll.
    pub jumps: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    /// Under autopilot in `system`: to the destination if it's here,
    /// otherwise to the jump point toward the next system on the path.
    Flying { system: SystemId, ship: Kinematics },
    /// In the lane from `from` to `to`, coming out at `to`'s jump point
    /// toward `from` on day `arrive`.
    Jumping {
        from: SystemId,
        to: SystemId,
        arrive: GalaxyTime,
    },
}

impl Stage {
    /// The system the fleet is in, or jumping into.
    pub fn system(&self) -> SystemId {
        match *self {
            Stage::Flying { system, .. } => system,
            Stage::Jumping { to, .. } => to,
        }
    }
}

/// Where a settlement is at time `t_days`, in AU from its star.
pub fn settlement_position(state: &GalaxyState, id: SettlementId, t_days: f64) -> [f64; 3] {
    let s = &state.settlements[id];
    site_position(state, s.system, s.site, t_days)
}

/// Where a site in `system` is at time `t_days`, in AU from its star.
pub fn site_position(state: &GalaxyState, system: SystemId, site: Site, t_days: f64) -> [f64; 3] {
    match site {
        Site::Region(r) => state.body_position(state.regions[r].body, t_days),
        Site::Surface(b) => state.body_position(b, t_days),
        Site::Space(orbit) => state.orbit_position(system, &orbit, t_days),
    }
}

/// A settlement's position and velocity at `t_days`, in metres from its
/// star: where a ship docking there has to be, and how fast.
pub fn settlement_goal(state: &GalaxyState, id: SettlementId, t_days: f64) -> Goal {
    let s = &state.settlements[id];
    site_goal(state, s.system, s.site, t_days)
}

/// A site's position and velocity at `t_days`, in metres from its star.
pub fn site_goal(state: &GalaxyState, system: SystemId, site: Site, t_days: f64) -> Goal {
    let body = match site {
        Site::Region(r) => Some(state.regions[r].body),
        Site::Surface(b) => Some(b),
        Site::Space(_) => None,
    };
    let (pos, vel) = match (site, body) {
        (Site::Space(orbit), _) => state.orbit_state(system, &orbit, t_days),
        (_, Some(b)) => state.orbit_state(system, &state.bodies[b].orbit, t_days),
        _ => unreachable!("a site on a body has its body"),
    };
    let mut pos = flight::scale(pos, METRES_PER_AU);
    // A planet's port is a holding point above its north pole, clear of
    // the planet: ships come and go from there, not from its centre.
    if let Some(b) = body {
        pos[2] += state.bodies[b].radius_km * 1000.0 * PORT_ALTITUDE_RADII;
    }
    Goal {
        pos,
        vel: flight::scale(vel, METRES_PER_AU / SECONDS_PER_DAY),
    }
}

/// The jump point in `here` toward `there`, in metres from the star.
pub fn jump_point(state: &GalaxyState, here: SystemId, there: SystemId) -> Vec3 {
    let (a, b) = (state.systems[here].position, state.systems[there].position);
    let d = [b.x - a.x, b.y - a.y, b.z - a.z];
    let len = norm(d);
    let dir = if len > 0.0 {
        flight::scale(d, 1.0 / len)
    } else {
        [1.0, 0.0, 0.0]
    };
    flight::scale(dir, JUMP_POINT_AU * METRES_PER_AU)
}

fn norm(p: [f64; 3]) -> f64 {
    (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()
}

/// Days to fly `au` from rest to rest under autopilot at `accel` m/s²: a
/// planning estimate of what [`flight::fly`] will take.
pub fn cruise_days(au: f64, accel: f64) -> f64 {
    flight::trip_seconds(au * METRES_PER_AU, accel) / SECONDS_PER_DAY
}

/// Whether `system` is in the bubble.
pub fn contains(state: &GalaxyState, system: SystemId) -> bool {
    state.bubble.systems.contains(&system)
}

/// The fleets flying in `system` right now (not those in a lane into it).
pub fn flights_in(
    state: &GalaxyState,
    system: SystemId,
) -> impl Iterator<Item = (FleetKey, &Flight, Kinematics)> + '_ {
    state
        .bubble
        .flights
        .iter()
        .filter_map(move |(&key, f)| match f.stage {
            Stage::Flying { system: s, ship } if s == system => Some((key, f, ship)),
            _ => None,
        })
}

/// Where a flight flying in `system` is headed at day `t_days`: its
/// destination if it's there, or the jump point toward the next system on
/// its path. What [`step`] flies toward, for anyone predicting a flight
/// between ticks.
pub fn goal(state: &GalaxyState, flight: &Flight, system: SystemId, t_days: f64) -> Goal {
    match flight.path.first() {
        Some(&next) => Goal {
            pos: jump_point(state, system, next),
            vel: [0.0; 3],
        },
        None => settlement_goal(state, flight.to, t_days),
    }
}

/// Put the bubble around `centre`, or take it away with `None`. Flights the
/// bubble no longer covers collapse into coarse trips; coarse trips that
/// now pass through it materialize, today or on the day they get there.
pub fn set_centre(state: &mut GalaxyState, centre: Option<SystemId>) {
    if state.bubble.centre == centre {
        return;
    }
    let systems = centre.map_or_else(BTreeSet::new, |c| within_jumps(state, c, RADIUS_JUMPS));
    state.bubble.centre = centre;
    state.bubble.systems = systems;
    state.bubble.near.clear();
    let leaving: Vec<FleetKey> = state
        .bubble
        .flights
        .iter()
        .filter(|(_, f)| !state.bubble.systems.contains(&f.stage.system()))
        .map(|(&k, _)| k)
        .collect();
    for key in leaving {
        collapse(state, key);
    }
    if centre.is_none() {
        return;
    }
    let travelling: Vec<FleetKey> = state
        .companies
        .iter()
        .flat_map(|(k, c)| {
            c.fleets
                .iter()
                .filter(|f| matches!(f.at, Location::Travelling { .. }))
                .map(move |f| (k, f.id))
        })
        .collect();
    for key in travelling {
        watch(state, key);
    }
}

/// Systems within `jumps` lane jumps of `centre`.
fn within_jumps(state: &GalaxyState, centre: SystemId, jumps: usize) -> BTreeSet<SystemId> {
    let mut seen = BTreeSet::from([centre]);
    let mut frontier = vec![centre];
    for _ in 0..jumps {
        let mut next = Vec::new();
        for s in frontier {
            for n in state.neighbours(s) {
                if seen.insert(n) {
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
    seen
}

/// Fill in [`Bubble::near`] if it's empty: lane distance to the nearest
/// bubble system, out to the longest trip any class can make.
fn index(state: &mut GalaxyState) {
    if !state.bubble.near.is_empty() || state.bubble.systems.is_empty() {
        return;
    }
    let fastest = state
        .ship_classes
        .iter()
        .map(|(_, c)| c.jump_laden)
        .fold(0.0, f64::max);
    let max_ly = haul::MAX_TRIP_DAYS * fastest;
    let mut best: HashMap<SystemId, f64> = HashMap::new();
    let mut heap = BinaryHeap::new();
    for &s in &state.bubble.systems {
        best.insert(s, 0.0);
        heap.push(Reverse((0.0f64.to_bits(), s)));
    }
    while let Some(Reverse((bits, system))) = heap.pop() {
        let ly = f64::from_bits(bits);
        if best.get(&system).is_some_and(|&b| b < ly) {
            continue;
        }
        for &lane in &state.systems[system].lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            let d = ly + lane.length_ly;
            if d <= max_ly && best.get(&next).is_none_or(|&b| d < b) {
                best.insert(next, d);
                heap.push(Reverse((d.to_bits(), next)));
            }
        }
    }
    state.bubble.near = best;
}

/// The shortest lane path from `from` to `to`, both ends included, no
/// longer than `max_ly`.
pub fn lane_path(
    state: &GalaxyState,
    from: SystemId,
    to: SystemId,
    max_ly: f64,
) -> Option<Vec<SystemId>> {
    let mut best: HashMap<SystemId, (f64, SystemId)> = HashMap::from([(from, (0.0, from))]);
    let mut heap = BinaryHeap::from([Reverse((0.0f64.to_bits(), from))]);
    while let Some(Reverse((bits, system))) = heap.pop() {
        let ly = f64::from_bits(bits);
        if best.get(&system).is_some_and(|&(b, _)| b < ly) {
            continue;
        }
        if system == to {
            let mut path = vec![to];
            let mut at = to;
            while at != from {
                at = best[&at].1;
                path.push(at);
            }
            path.reverse();
            return Some(path);
        }
        for &lane in &state.systems[system].lanes {
            let lane = &state.lanes[lane];
            let next = lane.other(system);
            let d = ly + lane.length_ly;
            if d <= max_ly && best.get(&next).is_none_or(|&(b, _)| d < b) {
                best.insert(next, (d, system));
                heap.push(Reverse((d.to_bits(), next)));
            }
        }
    }
    None
}

/// The length of the shortest lane between two neighbouring systems.
fn lane_ly(state: &GalaxyState, a: SystemId, b: SystemId) -> f64 {
    state.systems[a]
        .lanes
        .iter()
        .map(|&l| &state.lanes[l])
        .filter(|l| l.other(a) == b)
        .map(|l| l.length_ly)
        .fold(f64::INFINITY, f64::min)
}

/// A trip broken into the stretches a coarse trip-time estimate is made
/// of: the burn out, each lane, the burn in, and handling.
struct Plan {
    /// Every system on the way, origin first.
    systems: Vec<SystemId>,
    out_days: f64,
    lane_days: Vec<f64>,
    in_days: f64,
}

impl Plan {
    fn total(&self) -> f64 {
        self.out_days + self.lane_days.iter().sum::<f64>() + self.in_days + haul::HANDLING_DAYS
    }
}

fn plan(
    state: &GalaxyState,
    class: &ShipClass,
    from: SettlementId,
    to: SettlementId,
    max_days: f64,
) -> Option<Plan> {
    let (a, b) = (state.settlements.get(from)?, state.settlements.get(to)?);
    let cruise = |au: f64| cruise_days(au, class.accel_laden);
    let (ra, rb) = (haul::radius_au(state, from), haul::radius_au(state, to));
    if a.system == b.system {
        return Some(Plan {
            systems: vec![a.system],
            out_days: cruise((ra - rb).abs().max(1e-3)),
            lane_days: Vec::new(),
            in_days: 0.0,
        });
    }
    let systems = lane_path(state, a.system, b.system, max_days * class.jump_laden)?;
    let lane_days = systems
        .windows(2)
        .map(|w| lane_ly(state, w[0], w[1]) / class.jump_laden)
        .collect();
    Some(Plan {
        systems,
        out_days: cruise((ra - JUMP_POINT_AU).abs()),
        lane_days,
        in_days: cruise((rb - JUMP_POINT_AU).abs()),
    })
}

/// Where a trip of `plan` stands `elapsed` days after it began: the stage
/// and the systems still ahead, and whether it is past its last lane.
fn place(
    state: &GalaxyState,
    plan: &Plan,
    from: SettlementId,
    to: SettlementId,
    elapsed: f64,
) -> (Stage, Vec<SystemId>, bool) {
    let today = state.time;
    let now = today.days() as f64;
    let sys = &plan.systems;
    let origin = sys[0];
    if sys.len() == 1 {
        let a = settlement_goal(state, from, now).pos;
        let b = settlement_goal(state, to, now).pos;
        let ship = burn(a, b, plan.out_days, elapsed);
        return (
            Stage::Flying {
                system: origin,
                ship,
            },
            Vec::new(),
            false,
        );
    }
    if elapsed < plan.out_days {
        let a = settlement_goal(state, from, now).pos;
        let b = jump_point(state, origin, sys[1]);
        let ship = burn(a, b, plan.out_days, elapsed);
        return (
            Stage::Flying {
                system: origin,
                ship,
            },
            sys[1..].to_vec(),
            false,
        );
    }
    let mut t = plan.out_days;
    for (i, &days) in plan.lane_days.iter().enumerate() {
        if elapsed < t + days {
            let left = (t + days - elapsed).ceil().max(1.0) as u64;
            return (
                Stage::Jumping {
                    from: sys[i],
                    to: sys[i + 1],
                    arrive: GalaxyTime::from_days(today.days() + left),
                },
                sys[i + 2..].to_vec(),
                false,
            );
        }
        t += days;
    }
    let dest = sys[sys.len() - 1];
    let a = jump_point(state, dest, sys[sys.len() - 2]);
    let b = settlement_goal(state, to, now).pos;
    let ship = burn(a, b, plan.in_days, (elapsed - t).min(plan.in_days));
    (Stage::Flying { system: dest, ship }, Vec::new(), true)
}

/// Where a ship burning from rest at `a` to rest at `b` over `days` is
/// after `elapsed` of them: accelerating for the first half, braking for
/// the second.
fn burn(a: Vec3, b: Vec3, days: f64, elapsed: f64) -> Kinematics {
    let u = if days > 0.0 {
        (elapsed / days).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let (s, v) = if u < 0.5 {
        (2.0 * u * u, 4.0 * u)
    } else {
        (1.0 - 2.0 * (1.0 - u) * (1.0 - u), 4.0 * (1.0 - u))
    };
    let d = flight::sub(b, a);
    let seconds = (days * SECONDS_PER_DAY).max(1.0);
    Kinematics {
        pos: flight::add(a, flight::scale(d, s)),
        vel: flight::scale(d, v / seconds),
    }
}

/// How far into a coarse trip we are, in the plan's days: the calendar
/// trip is stretched or squeezed to match the plan.
fn elapsed(state: &GalaxyState, plan: &Plan, depart: GalaxyTime, arrive: GalaxyTime) -> f64 {
    let span = arrive.days().saturating_sub(depart.days()).max(1) as f64;
    let gone = state.time.days().saturating_sub(depart.days()) as f64;
    gone / span * plan.total()
}

/// A coarse trip has begun, or the bubble has moved: if the trip passes
/// through the bubble, materialize it now if it's there already, or
/// schedule it to enter on the day it gets there.
pub(crate) fn watch(state: &mut GalaxyState, key: FleetKey) {
    if state.bubble.systems.is_empty() {
        return;
    }
    index(state);
    let Some(n) = haul::fleet_index(state, key.0, key.1) else {
        return;
    };
    let fleet = &state.companies[key.0].fleets[n];
    let Location::Travelling {
        from,
        depart,
        to,
        arrive,
        ..
    } = fleet.at
    else {
        return;
    };
    let (Some(a), Some(b)) = (state.settlements.get(from), state.settlements.get(to)) else {
        return;
    };
    let (sa, sb) = (a.system, b.system);
    let class = &state.ship_classes[fleet.class];
    let span = arrive.days().saturating_sub(depart.days()) as f64;
    // Cheap test first: a trip can only pass through the bubble if its
    // ends are no further from it, together, than the trip is long.
    let near = &state.bubble.near;
    let (Some(&da), Some(&db)) = (near.get(&sa), near.get(&sb)) else {
        return;
    };
    if da + db > (span + 1.0) * class.jump_laden {
        return;
    }
    let Some(plan) = plan(state, class, from, to, span + 1.0) else {
        return;
    };
    // The trip reaches the bubble when it sets out from a bubble system,
    // or enters the lane into one.
    let mut t = 0.0;
    let mut enter = None;
    if contains(state, sa) {
        enter = Some(0.0);
    } else {
        t += plan.out_days;
        for (i, &days) in plan.lane_days.iter().enumerate() {
            if contains(state, plan.systems[i + 1]) {
                enter = Some(t);
                break;
            }
            t += days;
        }
    }
    let Some(enter) = enter else {
        return;
    };
    let scale = span.max(1.0) / plan.total();
    let day = depart.days() + (enter * scale).floor() as u64;
    let today = state.time.days();
    if day <= today {
        materialize(state, key);
    } else if day < arrive.days() {
        state.calendar.schedule(
            state.time,
            GalaxyTime::from_days(day),
            haul::Event {
                company: key.0,
                fleet: key.1,
                kind: haul::EventKind::Enter,
            },
        );
    }
}

/// A coarse fleet's trip has reached the bubble, or so it was scheduled to
/// when it set out: if it's in the bubble today, it starts flying.
pub(crate) fn enter(state: &mut GalaxyState, key: FleetKey) {
    if state.bubble.systems.is_empty() {
        return;
    }
    materialize(state, key);
}

/// Turn a coarse trip into a flight, placed where its trip puts it today,
/// if that's inside the bubble.
fn materialize(state: &mut GalaxyState, key: FleetKey) {
    let Some(n) = haul::fleet_index(state, key.0, key.1) else {
        return;
    };
    let fleet = &state.companies[key.0].fleets[n];
    let Location::Travelling {
        from,
        depart,
        to,
        arrive,
        jumps,
    } = fleet.at
    else {
        return;
    };
    let class = &state.ship_classes[fleet.class];
    let span = arrive.days().saturating_sub(depart.days()) as f64;
    let Some(plan) = plan(state, class, from, to, span + 1.0) else {
        return;
    };
    let gone = elapsed(state, &plan, depart, arrive);
    let (stage, path, past_lanes) = place(state, &plan, from, to, gone);
    if !contains(state, stage.system()) {
        return;
    }
    state.companies[key.0].fleets[n].at = Location::Flying { to };
    state.bubble.flights.insert(
        key,
        Flight {
            from,
            to,
            depart,
            stage,
            path,
            jumps,
        },
    );
    // Past its last lane already: the losses it would have taken coming
    // out of it are taken now.
    if jumps && past_lanes {
        lane_exit(state, key);
    }
}

/// A fleet in the bubble sets out from `here` for `to`. Returns false if
/// there's no lane path (the caller sends it coarse instead).
pub(crate) fn launch(
    state: &mut GalaxyState,
    key: FleetKey,
    here: SettlementId,
    to: SettlementId,
    jumps: bool,
) -> bool {
    let Some(n) = haul::fleet_index(state, key.0, key.1) else {
        return false;
    };
    let class = &state.ship_classes[state.companies[key.0].fleets[n].class];
    let (a, b) = (state.settlements[here].system, state.settlements[to].system);
    let path = if a == b {
        vec![a]
    } else {
        match lane_path(state, a, b, haul::MAX_TRIP_DAYS * 2.0 * class.jump_laden) {
            Some(p) => p,
            None => return false,
        }
    };
    let start = settlement_goal(state, here, state.time.days() as f64);
    state.companies[key.0].fleets[n].at = Location::Flying { to };
    state.bubble.flights.insert(
        key,
        Flight {
            from: here,
            to,
            depart: state.time,
            stage: Stage::Flying {
                system: a,
                ship: Kinematics {
                    pos: start.pos,
                    vel: start.vel,
                },
            },
            path: path[1..].to_vec(),
            jumps: jumps && a != b,
        },
    );
    true
}

/// Days a flight still needs, estimated the way coarse trips are.
fn days_left(state: &GalaxyState, flight: &Flight, class: &ShipClass) -> f64 {
    let cruise = |m: f64| flight::trip_seconds(m, class.accel_laden) / SECONDS_PER_DAY;
    let today = state.time.days();
    let (here, mut days) = match flight.stage {
        Stage::Flying { system, ship } => {
            let target = match flight.path.first() {
                Some(&next) => jump_point(state, system, next),
                None => settlement_goal(state, flight.to, today as f64).pos,
            };
            (
                system,
                cruise(flight::length(flight::sub(target, ship.pos))),
            )
        }
        Stage::Jumping { to, arrive, .. } => (to, arrive.days().saturating_sub(today) as f64),
    };
    let mut prev = here;
    for &next in &flight.path {
        days += lane_ly(state, prev, next) / class.jump_laden;
        prev = next;
    }
    let entered_dest = matches!(flight.stage, Stage::Flying { .. }) && flight.path.is_empty();
    if !entered_dest {
        let rb = haul::radius_au(state, flight.to);
        days += cruise_days((rb - JUMP_POINT_AU).abs(), class.accel_laden);
    }
    days + haul::HANDLING_DAYS
}

/// A flight leaves the agent tier: what's left of its trip becomes a
/// coarse arrival event.
fn collapse(state: &mut GalaxyState, key: FleetKey) {
    let Some(flight) = state.bubble.flights.remove(&key) else {
        return;
    };
    let Some(n) = haul::fleet_index(state, key.0, key.1) else {
        return;
    };
    if !state.settlements.contains_key(flight.to) {
        haul::lose_fleet(state, key.0, n, flight.to);
        return;
    }
    let class = &state.ship_classes[state.companies[key.0].fleets[n].class];
    let left = days_left(state, &flight, class).ceil().max(1.0) as u64;
    let today = state.time;
    let arrive = GalaxyTime::from_days(today.days() + left);
    state.companies[key.0].fleets[n].at = Location::Travelling {
        from: flight.from,
        depart: flight.depart,
        to: flight.to,
        arrive,
        jumps: flight.jumps,
    };
    state.calendar.schedule(
        today,
        arrive,
        haul::Event {
            company: key.0,
            fleet: key.1,
            kind: haul::EventKind::Arrive,
        },
    );
}

/// Coming out of the trip's last lane: roll its losses.
fn lane_exit(state: &mut GalaxyState, key: FleetKey) {
    let Some(flight) = state.bubble.flights.get_mut(&key) else {
        return;
    };
    flight.jumps = false;
    let to = flight.to;
    if let Some(n) = haul::fleet_index(state, key.0, key.1) {
        haul::roll_losses(state, key.0, n, to);
    }
}

/// A day of flight for every fleet in the bubble: burn, wait out a lane,
/// come out of one, or dock and trade. Runs in the trade phase, after the
/// day's coarse events.
pub(crate) fn step(state: &mut GalaxyState, contracts: &haul::Contracts, board: &haul::Board) {
    let today = state.time;
    let t0 = today.days() as f64;
    let keys: Vec<FleetKey> = state.bubble.flights.keys().copied().collect();
    for key in keys {
        let Some(n) = haul::fleet_index(state, key.0, key.1) else {
            // The fleet is gone; so is its flight.
            state.bubble.flights.remove(&key);
            continue;
        };
        let Some(flight) = state.bubble.flights.get(&key) else {
            continue;
        };
        let to = flight.to;
        if !state.settlements.contains_key(to) {
            state.bubble.flights.remove(&key);
            haul::lose_fleet(state, key.0, n, to);
            continue;
        }
        let class = &state.ship_classes[state.companies[key.0].fleets[n].class];
        let accel = class.accel_laden;
        let jump_laden = class.jump_laden;
        match flight.stage {
            Stage::Jumping { arrive, .. } if arrive > today => {}
            // Out of the lane at rest, facing where it came from; the
            // autopilot takes over tomorrow.
            Stage::Jumping {
                from, to: system, ..
            } => {
                let last = flight.path.is_empty();
                let jumps = flight.jumps;
                let ship = Kinematics {
                    pos: jump_point(state, system, from),
                    vel: [0.0; 3],
                };
                state.bubble.flights.get_mut(&key).expect("flight").stage =
                    Stage::Flying { system, ship };
                if last && jumps {
                    lane_exit(state, key);
                }
            }
            Stage::Flying { system, mut ship } => {
                let next = flight.path.first().copied();
                let arrived = flight::fly(&mut ship, accel, SECONDS_PER_DAY, |t, _| {
                    goal(state, flight, system, t0 + t / SECONDS_PER_DAY)
                });
                match (arrived, next) {
                    (None, _) => {
                        state.bubble.flights.get_mut(&key).expect("flight").stage =
                            Stage::Flying { system, ship };
                    }
                    (Some(_), None) => {
                        state.bubble.flights.remove(&key);
                        haul::dock(state, contracts, board, key.0, n, to, true);
                        // Straight out again: today is flown, so the next
                        // trip starts from where the port is tomorrow.
                        if let Some(f) = state.bubble.flights.get(&key) {
                            let start = settlement_goal(state, f.from, t0 + 1.0);
                            if let Some(f) = state.bubble.flights.get_mut(&key) {
                                f.stage = Stage::Flying {
                                    system: f.stage.system(),
                                    ship: Kinematics {
                                        pos: start.pos,
                                        vel: start.vel,
                                    },
                                };
                            }
                        }
                    }
                    (Some(_), Some(next)) => {
                        let days = (lane_ly(state, system, next) / jump_laden).ceil().max(1.0);
                        let flight = state.bubble.flights.get_mut(&key).expect("flight");
                        flight.path.remove(0);
                        flight.stage = Stage::Jumping {
                            from: system,
                            to: next,
                            arrive: GalaxyTime::from_days(today.days() + days as u64),
                        };
                        if !contains(state, next) {
                            collapse(state, key);
                        }
                    }
                }
            }
        }
    }
}
