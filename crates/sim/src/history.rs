//! The ground-truth record of what happened in the galaxy.
//!
//! This is what *actually* happened. The player never reads it directly;
//! factions' news (§5.6) will be generated from it, with bias.

use serde::{Deserialize, Serialize};

use crate::GalaxyTime;
use crate::settlement::SettlementId;
use crate::state::FactionId;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct History {
    events: Vec<HistoryEvent>,
}

impl History {
    pub fn record(&mut self, when: GalaxyTime, kind: HistoryKind) {
        self.events.push(HistoryEvent { when, kind });
    }

    pub fn events(&self) -> &[HistoryEvent] {
        &self.events
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub when: GalaxyTime,
    pub kind: HistoryKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HistoryKind {
    /// The galaxy was generated.
    Genesis { seed: u64 },
    /// A faction came into existence.
    FactionFounded { faction: FactionId },
    /// A settlement was founded.
    SettlementFounded {
        settlement: SettlementId,
        faction: FactionId,
    },
    /// A settlement started going badly hungry.
    Famine { settlement: SettlementId },
}
