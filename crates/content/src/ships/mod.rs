//! Ships: modules, sections, hulls and stock fits (see `docs/DESIGN.md`
//! §7.4 and `retrox_sim::ship`).
//!
//! Five small ships (single built-in section), three medium and three
//! large (bow, mid and stern sections on a frame), across combat, trade
//! and speed roles. No capital ships yet.

mod hulls;
mod modules;
pub mod price;
mod sections;
mod stock;

pub use hulls::*;
pub use modules::*;
pub use sections::*;
pub use stock::{STOCK, StockFit, stock};

use retrox_sim::ship::{Hull, Module, Mount, Section};

pub fn hull(id: &str) -> Option<&'static Hull> {
    HULLS.iter().find(|h| h.id == id)
}

pub fn module(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id == id)
}

/// A swappable section by id.
pub fn section(id: &str) -> Option<&'static Section> {
    SECTIONS.iter().find(|s| s.id == id)
}

/// Every section: the swappable ones and those built into small hulls.
pub fn all_sections() -> impl Iterator<Item = &'static Section> {
    let fixed = HULLS.iter().flat_map(|h| h.mounts).filter_map(|m| match m {
        Mount::Fixed(s) => Some(*s),
        Mount::Open { .. } => None,
    });
    SECTIONS.iter().chain(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use retrox_sim::ship::{Loadout, ShipRole, ShipStats, Size};
    use std::collections::HashSet;

    fn stock_stats() -> Vec<(&'static Hull, ShipStats)> {
        HULLS
            .iter()
            .map(|h| (h, stock(h.id).unwrap().stats()))
            .collect()
    }

    #[test]
    fn ids_are_unique_and_resolve() {
        let modules: HashSet<_> = MODULES.iter().map(|m| m.id).collect();
        assert_eq!(modules.len(), MODULES.len());
        let sections: HashSet<_> = all_sections().map(|s| s.id).collect();
        assert_eq!(sections.len(), all_sections().count());
        let hulls: HashSet<_> = HULLS.iter().map(|h| h.id).collect();
        assert_eq!(hulls.len(), HULLS.len());
        for m in MODULES {
            assert_eq!(module(m.id), Some(m));
        }
        for s in SECTIONS {
            assert_eq!(section(s.id), Some(s));
        }
        for h in HULLS {
            assert_eq!(hull(h.id), Some(h));
        }
    }

    #[test]
    fn every_module_fits_some_section() {
        for m in MODULES {
            let fits = all_sections()
                .flat_map(|s| s.slots)
                .any(|slot| slot.kind == m.kind() && slot.size >= m.size);
            assert!(fits, "{} doesn't fit any section", m.id);
        }
    }

    #[test]
    fn every_swappable_section_fits_some_hull() {
        for s in SECTIONS {
            let fits = HULLS.iter().any(|h| {
                let fit = Loadout::new(h);
                (0..h.mounts.len()).any(|i| fit.check_section(i, s).is_ok())
            });
            assert!(fits, "{} doesn't fit any hull", s.id);
        }
    }

    #[test]
    fn every_hull_has_a_complete_stock_fit() {
        for h in HULLS {
            let fit = stock(h.id).unwrap_or_else(|| panic!("{} has no stock fit", h.id));
            for (_, section) in fit.mounts() {
                let section = section.unwrap_or_else(|| panic!("{} has an empty mount", h.id));
                assert!(
                    section.slots().all(|(_, m)| m.is_some()),
                    "{} has an empty slot in {}",
                    h.id,
                    section.section.id
                );
            }
        }
    }

    #[test]
    fn stock_ships_cruise_within_budget() {
        for (h, s) in stock_stats() {
            assert!(
                s.power_margin(s.cruise) >= 0.0,
                "{} is short of power cruising",
                h.id
            );
            assert_eq!(
                s.time_to_overheat(s.cruise),
                None,
                "{} overheats cruising",
                h.id
            );
            assert!(s.time_to_turn(180.0).is_some(), "{} can't turn", h.id);
            assert!(s.acceleration() > 0.0, "{} can't thrust", h.id);
            assert!(s.drive_tier >= 1, "{} has no jump drive", h.id);
            assert!(s.laden().jump_speed() > 0.5, "{} jumps too slowly", h.id);
        }
    }

    #[test]
    fn roles_lead_their_size() {
        let stats = stock_stats();
        for size in [Size::Small, Size::Medium, Size::Large] {
            let of_size: Vec<_> = stats.iter().filter(|(h, _)| h.size == size).collect();
            let best = |key: fn(&ShipStats) -> f64| {
                of_size
                    .iter()
                    .max_by(|a, b| key(&a.1).total_cmp(&key(&b.1)))
                    .unwrap()
                    .0
            };
            assert_eq!(best(|s| s.dps).role, ShipRole::Combat, "{size:?} dps");
            assert_eq!(best(|s| s.cargo_t).role, ShipRole::Trade, "{size:?} cargo");
            assert_eq!(
                best(ShipStats::acceleration).role,
                ShipRole::Speed,
                "{size:?} acceleration"
            );
        }
    }

    #[test]
    fn bigger_ships_turn_slower() {
        let stats = stock_stats();
        let slowest = |size| {
            stats
                .iter()
                .filter(|(h, _)| h.size == size)
                .map(|(_, s)| s.time_to_turn(180.0).unwrap())
                .fold(0.0, f64::max)
        };
        let fastest = |size| {
            stats
                .iter()
                .filter(|(h, _)| h.size == size)
                .map(|(_, s)| s.time_to_turn(180.0).unwrap())
                .fold(f64::INFINITY, f64::min)
        };
        assert!(slowest(Size::Small) < fastest(Size::Large));
        assert!(fastest(Size::Small) < fastest(Size::Medium));
        assert!(fastest(Size::Medium) < fastest(Size::Large));
    }

    #[test]
    fn stock_interceptor_is_short_in_combat() {
        let s = stock("interceptor").unwrap().stats();
        assert!(s.power_margin(s.combat) < 0.0);
        let secs = s.time_to_overheat(s.combat).unwrap();
        assert!((10.0..120.0).contains(&secs), "overheats in {secs}s");
        let flip = s.time_to_turn(180.0).unwrap();
        assert!((1.0..2.0).contains(&flip), "flips in {flip}s");
    }

    #[test]
    fn lasers_trade_heat_for_damage() {
        let mut fit = stock("interceptor").unwrap();
        let cannons = fit.stats();
        fit.fit(0, 0, &LIGHT_LASER).unwrap();
        fit.fit(0, 1, &LIGHT_LASER).unwrap();
        let lasers = fit.stats();
        assert!(lasers.dps > cannons.dps);
        assert!(lasers.weapon_range_km > cannons.weapon_range_km);
        assert!(
            lasers.time_to_overheat(lasers.combat).unwrap()
                < cannons.time_to_overheat(cannons.combat).unwrap()
        );
    }

    #[test]
    fn a_hauler_can_be_refitted_as_a_gunboat() {
        let mut fit = stock("hauler").unwrap();
        let before = fit.stats();
        let removed = fit.set_section(1, &GUN_DECK_M).unwrap();
        assert_eq!(removed, vec![&CARGO_POD; 3]);
        for (slot, m) in [
            &AUTOCANNON,
            &AUTOCANNON,
            &LIGHT_AUTOCANNON,
            &LIGHT_AUTOCANNON,
            &RADIATOR_PANEL,
        ]
        .into_iter()
        .enumerate()
        {
            fit.fit(1, slot, m).unwrap();
        }
        let after = fit.stats();
        assert!(after.dps > before.dps);
        assert!(after.cargo_t < before.cargo_t);
    }
}
