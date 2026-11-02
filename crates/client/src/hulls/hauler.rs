//! The hauler. Not modelled yet: generated from its fit.

use retrox_sim::ship::Loadout;

use crate::meshes::{Mesh, ShipShape};

pub(super) fn build(fit: &Loadout) -> (Mesh, ShipShape) {
    crate::meshes::generated(fit)
}
