//! Where to buy a ship or refit one (issue #161): the nearest settlements
//! with a shipyard or an outfitter, along the hyperlanes from here. Read off
//! the galaxy, like a pilot's almanac: which settlements build ships is
//! public knowledge, not something the player has to discover by docking.

use retrox_sim::routes::{self, Visit};
use retrox_sim::{SettlementId, SystemId};

use super::{World, routing::jump_days};
use crate::hangar;

/// What the player wants done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    /// Buy or sell a ship (a shipyard, which also refits).
    Shipyard,
    /// Refit: fit modules and swap sections (any outfitter).
    Refit,
}

/// A settlement that offers a [`Service`], and how far it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearby {
    pub at: SettlementId,
    pub system: SystemId,
    pub jumps: u32,
    /// Days in transit along the hyperlanes (0 in this system).
    pub days: f64,
}

/// The furthest out the search looks.
const MAX_JUMPS: u32 = 12;

impl World {
    /// Whether settlement `s` has `service`. Every settlement can be
    /// docked or landed at (issue #169).
    pub fn offers(&self, s: SettlementId, service: Service) -> bool {
        self.galaxy.settlements.get(s).is_some_and(|st| {
            let offers = hangar::services(&st.economy);
            match service {
                Service::Shipyard => offers.shipyard,
                Service::Refit => offers.outfitter,
            }
        })
    }

    /// Up to `n` settlements offering `service`, quickest to reach first,
    /// this system's own first of all. Settlements whose faction counts the
    /// player hostile are left out: they won't clear the ship to dock.
    pub fn nearest(&self, service: Service, n: usize) -> Vec<Nearby> {
        let mut out = Vec::new();
        routes::search(
            &self.galaxy,
            self.system,
            |lane| jump_days(lane.length_ly),
            |step| {
                let mut here: Vec<Nearby> = self
                    .galaxy
                    .settlements_in(step.system)
                    .filter(|&(id, st)| {
                        self.offers(id, service) && !self.player.standing.hostile(st.owner)
                    })
                    .map(|(at, _)| Nearby {
                        at,
                        system: step.system,
                        jumps: step.jumps,
                        days: step.cost,
                    })
                    .collect();
                // Biggest yard first within a system: the likeliest to
                // have ships on the lot.
                here.sort_by(|a, b| {
                    let size = |s: SettlementId| self.galaxy.settlements[s].economy.population;
                    size(b.at).total_cmp(&size(a.at))
                });
                out.extend(here);
                if out.len() >= n {
                    Visit::Stop
                } else if step.jumps >= MAX_JUMPS {
                    Visit::Leaf
                } else {
                    Visit::Expand
                }
            },
        );
        out.truncate(n);
        out
    }

    /// How many settlements in `system` offer `service`.
    pub fn count_in(&self, system: SystemId, service: Service) -> usize {
        self.galaxy
            .settlements_in(system)
            .filter(|&(id, _)| self.offers(id, service))
            .count()
    }
}
