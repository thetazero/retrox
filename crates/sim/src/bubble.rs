//! The agent tier's travel (DESIGN §5.2): ships near the player fly real
//! trips under the [`crate::flight`] autopilot, out to a jump point, along
//! the lanes and in again.
//!
//! Nothing in the coarse tier uses this; it's here for the bubble (#59),
//! where traders become individual fleets.

use serde::{Deserialize, Serialize};

use crate::flight::{self, Goal, Kinematics, Vec3};
use crate::settlement::{SettlementId, Site};
use crate::{GalaxyState, GalaxyTime, SystemId};

const METRES_PER_AU: f64 = 1.496e11;
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Jump points sit this far from a star, toward the system on the far end.
pub const JUMP_POINT_AU: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stage {
    /// Under autopilot in `system`: to the destination if it's here,
    /// otherwise to the jump point toward it.
    Flying { system: SystemId, ship: Kinematics },
    /// In the hyperlanes from `from`, coming out at the destination
    /// system's jump point facing it.
    Jumping { from: SystemId, arrive: GalaxyTime },
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
    let (pos, vel) = match site {
        Site::Region(r) => {
            let b = &state.bodies[state.regions[r].body];
            state.orbit_state(system, &b.orbit, t_days)
        }
        Site::Surface(b) => state.orbit_state(system, &state.bodies[b].orbit, t_days),
        Site::Space(orbit) => state.orbit_state(system, &orbit, t_days),
    };
    Goal {
        pos: flight::scale(pos, METRES_PER_AU),
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

/// Where a trip stands after a day of it.
pub enum Leg {
    Underway(Stage),
    Arrived,
}

/// One day of a trip toward `goal` (a function of time in days) in
/// `dest_system`: fly under autopilot, wait out the jump, or come out of
/// the lanes. Shared by every kind of ship.
pub fn step_trip(
    state: &GalaxyState,
    stage: Stage,
    dest_system: SystemId,
    jump_days: f64,
    accel: f64,
    goal: impl Fn(f64) -> Goal,
) -> Leg {
    let now = state.time;
    match stage {
        Stage::Jumping { arrive, .. } if arrive > now => Leg::Underway(stage),
        // Out of the lanes at rest, on the side facing where we came from;
        // the autopilot takes it from here tomorrow.
        Stage::Jumping { from, .. } => Leg::Underway(Stage::Flying {
            system: dest_system,
            ship: Kinematics {
                pos: jump_point(state, dest_system, from),
                vel: [0.0; 3],
            },
        }),
        Stage::Flying { system, mut ship } => {
            let t0 = now.days() as f64;
            let arrived = if system == dest_system {
                flight::fly(&mut ship, accel, SECONDS_PER_DAY, |t, _| {
                    goal(t0 + t / SECONDS_PER_DAY)
                })
            } else {
                let point = Goal {
                    pos: jump_point(state, system, dest_system),
                    vel: [0.0; 3],
                };
                flight::fly(&mut ship, accel, SECONDS_PER_DAY, |_, _| point)
            };
            match arrived {
                None => Leg::Underway(Stage::Flying { system, ship }),
                Some(_) if system == dest_system => Leg::Arrived,
                Some(_) => Leg::Underway(Stage::Jumping {
                    from: system,
                    arrive: GalaxyTime::from_days(now.days() + jump_days.ceil().max(1.0) as u64),
                }),
            }
        }
    }
}
