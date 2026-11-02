//! Asteroid fields: the dense, minable clusters inside a belt. A belt is a
//! whole ring; its fields are where the rocks actually are. Individual
//! rocks aren't stored: they're generated from the field's seed when
//! someone is close enough to see them.

use serde::{Deserialize, Serialize};

use crate::bodies::{BodyId, Orbit};
use crate::{Id, Resources};

pub type FieldId = Id<AsteroidField>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsteroidField {
    pub belt: BodyId,
    pub name: String,
    /// Where the field's centre is: an orbit inside its belt.
    pub orbit: Orbit,
    pub radius_km: f64,
    /// What it's made of, each 0..1 richness.
    pub resources: Resources,
    /// Seed for the individual rocks.
    pub seed: u64,
}

impl AsteroidField {
    /// Mostly ice rather than rock.
    pub fn is_icy(&self) -> bool {
        self.resources.ice > self.resources.ore
    }
}
