//! The player's standing with each faction (DESIGN §7.2.2): a number from
//! −100 to +100 that what the player does moves, read as a rank.
//!
//! - **Up:** destroying pirates a faction has a bounty on, or pirates in
//!   its space; delivering its freight.
//! - **Down:** shooting its ships, and far more for destroying them;
//!   failing its freight contracts.
//! - **Hostile** (at [`HOSTILE`] or below): its stations and cities refuse
//!   to dock or land the player, and its armed ships fire on sight.
//! - Standing drifts back toward zero over time, faster from below: a
//!   faction forgets a grudge in a season or two, and a favour in a year.

use std::collections::HashMap;

use retrox_sim::FactionId;
use serde::{Deserialize, Serialize};

/// At or below this, a faction is hostile.
pub const HOSTILE: f64 = -25.0;
/// Below this, wary.
const WARY: f64 = -5.0;
/// At or above this, trusted; at or above [`HONOURED`], honoured.
const TRUSTED: f64 = 15.0;
const HONOURED: f64 = 50.0;
/// Points a day a grudge fades by, and a favour.
const FORGIVE_PER_DAY: f64 = 0.25;
const FORGET_PER_DAY: f64 = 0.05;

/// Standing for destroying a ship with a faction's bounty on it.
pub const BOUNTY_KILL: f64 = 3.0;
/// For destroying a pirate in a faction's space.
pub const PIRATE_KILL: f64 = 1.0;
/// For opening fire on a faction's peaceful ship, for each further hit,
/// and for destroying one.
pub const ATTACKED: f64 = -20.0;
pub const SHOT_SHIP: f64 = -6.0;
pub const KILLED_SHIP: f64 = -30.0;
/// For delivering a faction's freight, and failing it.
pub const DELIVERED: f64 = 1.0;
pub const FAILED: f64 = -5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Hostile,
    Wary,
    Neutral,
    Trusted,
    Honoured,
}

impl Rank {
    pub fn of(standing: f64) -> Self {
        if standing <= HOSTILE {
            Rank::Hostile
        } else if standing < WARY {
            Rank::Wary
        } else if standing < TRUSTED {
            Rank::Neutral
        } else if standing < HONOURED {
            Rank::Trusted
        } else {
            Rank::Honoured
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rank::Hostile => "Hostile",
            Rank::Wary => "Wary",
            Rank::Neutral => "Neutral",
            Rank::Trusted => "Trusted",
            Rank::Honoured => "Honoured",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Standing {
    #[serde(with = "crate::save::pairs")]
    by: HashMap<FactionId, f64>,
    /// The galaxy day standing last drifted.
    day: u64,
}

impl Standing {
    pub fn get(&self, faction: FactionId) -> f64 {
        self.by.get(&faction).copied().unwrap_or(0.0)
    }

    pub fn rank(&self, faction: FactionId) -> Rank {
        Rank::of(self.get(faction))
    }

    pub fn hostile(&self, faction: FactionId) -> bool {
        self.rank(faction) == Rank::Hostile
    }

    /// Move standing with `faction` by `delta`. Returns the new rank if it
    /// changed.
    pub fn change(&mut self, faction: FactionId, delta: f64) -> Option<Rank> {
        let before = self.rank(faction);
        let s = self.by.entry(faction).or_insert(0.0);
        *s = (*s + delta).clamp(-100.0, 100.0);
        let after = self.rank(faction);
        (after != before).then_some(after)
    }

    /// Let standing drift toward zero up to galaxy day `today`.
    pub fn drift(&mut self, today: u64) {
        if self.day == 0 || today < self.day {
            self.day = today;
            return;
        }
        let days = (today - self.day) as f64;
        self.day = today;
        if days <= 0.0 {
            return;
        }
        for s in self.by.values_mut() {
            *s = if *s < 0.0 {
                (*s + FORGIVE_PER_DAY * days).min(0.0)
            } else {
                (*s - FORGET_PER_DAY * days).max(0.0)
            };
        }
        self.by.retain(|_, s| s.abs() > 1e-9);
    }

    /// Every faction the player has a standing with, most liked first.
    pub fn all(&self) -> Vec<(FactionId, f64)> {
        let mut v: Vec<_> = self.by.iter().map(|(&f, &s)| (f, s)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }
}

/// What the HUD says when standing crosses into a new rank.
pub fn news(faction: &str, rank: Rank) -> String {
    format!("The {faction} now count us {}", rank.name().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn faction(n: u64) -> FactionId {
        FactionId::from(KeyData::from_ffi(n | (1 << 32)))
    }

    #[test]
    fn a_few_shots_make_a_faction_hostile_and_time_forgives() {
        let mut s = Standing::default();
        let f = faction(1);
        s.drift(100);
        assert_eq!(s.rank(f), Rank::Neutral);
        let mut became = None;
        for _ in 0..5 {
            if let Some(r) = s.change(f, SHOT_SHIP) {
                became = Some(r);
            }
        }
        assert_eq!(became, Some(Rank::Hostile));
        assert!(s.hostile(f));
        s.drift(160);
        assert!(!s.hostile(f), "two months on: {}", s.get(f));
        s.drift(400);
        assert_eq!(s.get(f), 0.0);
    }

    #[test]
    fn bounty_hunting_earns_trust() {
        let mut s = Standing::default();
        let f = faction(2);
        for _ in 0..6 {
            s.change(f, BOUNTY_KILL);
        }
        assert_eq!(s.rank(f), Rank::Trusted);
    }
}
