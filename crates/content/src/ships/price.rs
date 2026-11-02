//! What ships and their parts cost, in credits.
//!
//! Everything is priced by the tonne, from the economy's price for finished
//! hull ([`HULL_PRICE_PER_T`]): frames and sections at that rate, modules
//! at a multiple of it that reflects how much work goes into each kind (a
//! reactor or a jump drive is worth far more per tonne than a cargo rack).
//! A fitted ship costs the sum of its parts.
//!
//! Parts sell back at [`RESALE`] of their price.

use retrox_sim::economy::HULL_PRICE_PER_T;
use retrox_sim::ship::{Hull, Loadout, Module, Section, SlotKind};

/// Share of the price a used part or ship sells back for.
pub const RESALE: f64 = 0.6;

/// Price per tonne of a module of this kind, as a multiple of hull.
fn kind_factor(kind: SlotKind) -> f64 {
    match kind {
        SlotKind::Cargo => 0.5,
        SlotKind::Radiator => 1.0,
        SlotKind::Engine | SlotKind::Rcs => 2.0,
        SlotKind::Power => 2.5,
        SlotKind::Weapon | SlotKind::Shield | SlotKind::Radar => 3.0,
        SlotKind::JumpDrive => 4.0,
    }
}

pub fn module_price(module: &Module) -> f64 {
    (module.mass_t * HULL_PRICE_PER_T * kind_factor(module.kind())).round()
}

pub fn section_price(section: &Section) -> f64 {
    (section.mass_t * HULL_PRICE_PER_T).round()
}

/// The bare frame. Small hulls have no frame of their own: their built-in
/// section is the hull.
pub fn frame_price(hull: &Hull) -> f64 {
    (hull.frame_mass_t * HULL_PRICE_PER_T).round()
}

/// A fitted ship: frame, every section and every module in it.
pub fn loadout_price(fit: &Loadout) -> f64 {
    let mut total = frame_price(fit.hull());
    for (_, section) in fit.mounts() {
        let Some(section) = section else { continue };
        total += section_price(section.section);
        total += section
            .slots()
            .filter_map(|(_, m)| m)
            .map(module_price)
            .sum::<f64>();
    }
    total
}

/// What something bought for `price` sells back for.
pub fn resale(price: f64) -> f64 {
    (price * RESALE).round()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ships::{HULLS, stock};
    use retrox_sim::ship::Size;

    #[test]
    fn bigger_ships_cost_more() {
        let price = |size| {
            HULLS
                .iter()
                .filter(|h| h.size == size)
                .map(|h| loadout_price(&stock(h.id).unwrap()))
                .fold(f64::INFINITY, f64::min)
        };
        let most = |size| {
            HULLS
                .iter()
                .filter(|h| h.size == size)
                .map(|h| loadout_price(&stock(h.id).unwrap()))
                .fold(0.0, f64::max)
        };
        assert!(most(Size::Small) < price(Size::Medium));
        assert!(most(Size::Medium) < price(Size::Large));
    }

    #[test]
    fn a_ship_costs_its_parts() {
        let fit = stock("hauler").unwrap();
        let parts: f64 = fit
            .mounts()
            .filter_map(|(_, s)| s)
            .map(|s| {
                section_price(s.section)
                    + s.slots()
                        .filter_map(|(_, m)| m)
                        .map(module_price)
                        .sum::<f64>()
            })
            .sum();
        assert_eq!(loadout_price(&fit), frame_price(fit.hull()) + parts);
    }
}
