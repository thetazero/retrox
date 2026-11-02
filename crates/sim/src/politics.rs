//! Politics (DESIGN §5.5) isn't built yet. Its monthly decisions will
//! weigh how far each settlement is from its capital: holding territory
//! costs more the further out it is. That is a lane search, so it isn't
//! made at decision time (§5.1.1): each settlement stores its distance to
//! its capital, worked out when it is founded and again only when its
//! faction's capital or its own owner changes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::routes::{self, Visit};
use crate::{FactionId, GalaxyState, SettlementId, SystemId};

/// How far a settlement is from its capital, along the lanes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToCapital {
    pub ly: f64,
    pub jumps: u32,
}

/// Lane distance from `capital` to each of `targets`, in one search that
/// stops once it has reached them all. Systems out of reach of the lanes
/// are left out.
fn search(
    state: &GalaxyState,
    capital: SystemId,
    targets: &[SystemId],
) -> HashMap<SystemId, ToCapital> {
    let mut found = HashMap::new();
    let mut left = targets.len();
    routes::search(
        state,
        capital,
        |lane| lane.length_ly,
        |step| {
            if targets.contains(&step.system) {
                found.insert(
                    step.system,
                    ToCapital {
                        ly: step.cost,
                        jumps: step.jumps,
                    },
                );
                left -= 1;
                if left == 0 {
                    return Visit::Stop;
                }
            }
            Visit::Expand
        },
    );
    found
}

/// The capital system of `faction`, if it has a capital.
fn capital_system(state: &GalaxyState, faction: FactionId) -> Option<SystemId> {
    let capital = state.factions.get(faction)?.capital;
    Some(state.settlements.get(capital)?.system)
}

/// Work out the distance to its capital of every settlement `faction`
/// owns: one search.
fn refresh_faction(state: &mut GalaxyState, faction: FactionId, members: &[SettlementId]) {
    let Some(capital) = capital_system(state, faction) else {
        for &id in members {
            state.settlements[id].to_capital = None;
        }
        return;
    };
    let mut targets: Vec<SystemId> = members
        .iter()
        .map(|&id| state.settlements[id].system)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let found = search(state, capital, &targets);
    for &id in members {
        let system = state.settlements[id].system;
        state.settlements[id].to_capital = found.get(&system).copied();
    }
}

/// Every settlement's distance to its capital, worked out afresh: one
/// search per faction. For a newly made galaxy, and for saves made before
/// distances were stored.
pub fn refresh_all(state: &mut GalaxyState) {
    let mut members: HashMap<FactionId, Vec<SettlementId>> = HashMap::new();
    for (id, s) in state.settlements.iter() {
        members.entry(s.owner).or_default().push(id);
    }
    let mut factions: Vec<FactionId> = members.keys().copied().collect();
    factions.sort_unstable();
    for faction in factions {
        refresh_faction(state, faction, &members[&faction]);
    }
}

/// Make `capital` its faction's capital, and work out again how far each
/// of the faction's settlements is from it.
pub fn set_capital(state: &mut GalaxyState, faction: FactionId, capital: SettlementId) {
    state.factions[faction].capital = capital;
    let members: Vec<SettlementId> = state
        .settlements
        .iter()
        .filter(|(_, s)| s.owner == faction)
        .map(|(id, _)| id)
        .collect();
    refresh_faction(state, faction, &members);
}

/// Hand `settlement` to `owner`, and work out how far it is from its new
/// capital.
pub fn set_owner(state: &mut GalaxyState, settlement: SettlementId, owner: FactionId) {
    state.settlements[settlement].owner = owner;
    founded(state, settlement);
}

/// A settlement has just been founded (or changed hands): how far is its
/// capital?
pub(crate) fn founded(state: &mut GalaxyState, settlement: SettlementId) {
    let s = &state.settlements[settlement];
    let to_capital = capital_system(state, s.owner)
        .and_then(|capital| search(state, capital, &[s.system]).get(&s.system).copied());
    state.settlements[settlement].to_capital = to_capital;
}
