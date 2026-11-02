//! Hand-built ships. Most hulls are generated from their fit
//! ([`meshes::ship`]); the ones here are modelled by hand, each with its
//! own silhouette, and draw their fitted modules — guns, cargo, radiators,
//! radar, shield emitters, engines — from a shared kit at the slots'
//! positions, so a refit shows.
//!
//! Model space is metres, nose toward -Z; a slot's `position_m` (+z
//! forward) lands at `(x, y, -z)`.

mod blockade_runner;
mod bulk_freighter;
mod express_freighter;
mod frigate;
mod gunship;
mod hauler;
mod kit;
mod small;

use glam::{Mat4, Quat, Vec3};
use retrox_sim::ship::{FittedSection, Loadout, Module, Section, Size, Slot};

use crate::meshes::{Mesh, ShipShape};

/// The hand-built model for `fit`'s hull, if it has one.
pub fn authored(fit: &Loadout) -> Option<(Mesh, ShipShape)> {
    match fit.hull().id {
        "courier" => Some(small::courier(fit.stats().length_m as f32)),
        "light-trader" => Some(small::light_trader(fit)),
        "interceptor" => Some(small::interceptor(fit)),
        "heavy-fighter" => Some(small::heavy_fighter(fit)),
        "racer" => Some(small::racer(fit)),
        "gunship" => Some(gunship::build(fit)),
        "blockade-runner" => Some(blockade_runner::build(fit)),
        "hauler" => Some(hauler::build(fit)),
        "frigate" => Some(frigate::build(fit)),
        "express-freighter" => Some(express_freighter::build(fit)),
        "bulk-freighter" => Some(bulk_freighter::build(fit)),
        _ => None,
    }
}

/// Every slot with its fitted module, and where it sits in model space.
fn fitted(fit: &Loadout) -> Vec<(&'static Slot, &'static Module, Vec3)> {
    fit.mounts()
        .filter_map(|(_, s)| s)
        .flat_map(|s| s.slots())
        .filter_map(|(slot, module)| {
            let [x, y, z] = slot.position_m.map(|v| v as f32);
            module.map(|m| (slot, m, Vec3::new(x, y, -z)))
        })
        .collect()
}

/// How much bigger a module of this size is drawn than a small one.
fn scale(size: Size) -> f32 {
    match size {
        Size::Small => 1.0,
        Size::Medium => 1.4,
        Size::Large => 2.0,
    }
}

fn at(p: Vec3, s: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(Vec3::splat(s), Quat::IDENTITY, p)
}

/// Lasers and cannon look different.
fn is_laser(module: &Module) -> bool {
    module.id.contains("laser")
}

/// A section of a framed (medium or large) hull, placed along the ship:
/// sections run nose (-Z) to tail, end to end, the whole ship centred on
/// the origin.
pub(super) struct Placed<'a> {
    pub section: &'static Section,
    pub fitted: &'a FittedSection,
    /// Where it starts (toward the nose) and ends, in model Z.
    pub front: f32,
    pub back: f32,
    pub centre: f32,
}

impl Placed<'_> {
    pub fn len(&self) -> f32 {
        self.back - self.front
    }

    /// This section's fitted modules and where their slots are, in model
    /// space.
    pub fn modules(&self) -> impl Iterator<Item = (&'static Slot, &'static Module, Vec3)> + '_ {
        self.fitted.slots().filter_map(move |(slot, module)| {
            let [x, y, z] = slot.position_m.map(|v| v as f32);
            module.map(|m| (slot, m, Vec3::new(x, y, self.centre - z)))
        })
    }
}

/// Every fitted section, placed.
pub(super) fn placed(fit: &Loadout) -> Vec<Placed<'_>> {
    let sections: Vec<&FittedSection> = fit.mounts().filter_map(|(_, s)| s).collect();
    let total: f32 = sections.iter().map(|s| s.section.length_m as f32).sum();
    let mut front = -total / 2.0;
    sections
        .into_iter()
        .map(|fitted| {
            let len = fitted.section.length_m as f32;
            let p = Placed {
                section: fitted.section,
                fitted,
                front,
                back: front + len,
                centre: front + len / 2.0,
            };
            front += len;
            p
        })
        .collect()
}
