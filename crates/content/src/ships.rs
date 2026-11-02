//! Hulls and modules (see `docs/DESIGN.md` §7.4).
//!
//! The interceptor is the first hull: fast and fragile, with an engine
//! oversized for its frame. Its stock fit runs everything but the guns
//! within its power and heat budgets; opening fire puts it over on power
//! and slowly cooks it, so the pilot has to pick what to run.

use retrox_sim::ship::{Function, Hull, Loadout, Module, Size, Slot, SlotKind};

pub static HULLS: &[Hull] = &[INTERCEPTOR];

pub static MODULES: &[Module] = &[
    LIGHT_AUTOCANNON,
    LIGHT_LASER,
    INTERCEPTOR_DRIVE,
    LIGHT_DRIVE,
    SMALL_REACTOR,
    LIGHT_DEFLECTOR,
    SHORT_RANGE_RADAR,
    FIN_RADIATOR,
];

pub fn hull(id: &str) -> Option<&'static Hull> {
    HULLS.iter().find(|h| h.id == id)
}

pub fn module(id: &str) -> Option<&'static Module> {
    MODULES.iter().find(|m| m.id == id)
}

const fn slot(kind: SlotKind, size: Size, position_m: [f64; 3]) -> Slot {
    Slot {
        kind,
        size,
        position_m,
    }
}

pub const INTERCEPTOR: Hull = Hull {
    id: "interceptor",
    name: "Interceptor",
    size: Size::Small,
    mass_t: 14.0,
    structure: 300.0,
    heat_capacity_mj: 120.0,
    base_dissipation_mw: 2.0,
    slots: &[
        slot(SlotKind::Weapon, Size::Small, [-1.6, -0.3, 3.0]),
        slot(SlotKind::Weapon, Size::Small, [1.6, -0.3, 3.0]),
        slot(SlotKind::Engine, Size::Medium, [0.0, 0.0, -4.5]),
        slot(SlotKind::Power, Size::Small, [0.0, 0.0, -1.5]),
        slot(SlotKind::Shield, Size::Small, [0.0, 0.4, 0.0]),
        slot(SlotKind::Radar, Size::Small, [0.0, 0.6, 2.5]),
        slot(SlotKind::Radiator, Size::Small, [-1.2, 0.0, -2.5]),
        slot(SlotKind::Radiator, Size::Small, [1.2, 0.0, -2.5]),
    ],
};

/// The interceptor as it comes off the lot.
pub fn stock_interceptor() -> Loadout<'static> {
    let mut fit = Loadout::new(&INTERCEPTOR);
    let modules = [
        &LIGHT_AUTOCANNON,
        &LIGHT_AUTOCANNON,
        &INTERCEPTOR_DRIVE,
        &SMALL_REACTOR,
        &LIGHT_DEFLECTOR,
        &SHORT_RANGE_RADAR,
        &FIN_RADIATOR,
        &FIN_RADIATOR,
    ];
    for (slot, module) in modules.into_iter().enumerate() {
        fit.fit(slot, module)
            .expect("stock interceptor fit is valid");
    }
    fit
}

/// Cheap, cool-running, short-ranged.
pub const LIGHT_AUTOCANNON: Module = Module {
    id: "light-autocannon",
    name: "Light autocannon",
    size: Size::Small,
    mass_t: 1.2,
    power_mw: 2.0,
    heat_mw: 3.0,
    function: Function::Weapon {
        damage: 12.0,
        fire_rate_hz: 8.0,
        range_km: 2.5,
    },
};

/// Hits harder and reaches further, but runs hot and hungry.
pub const LIGHT_LASER: Module = Module {
    id: "light-laser",
    name: "Light laser",
    size: Size::Small,
    mass_t: 0.9,
    power_mw: 4.0,
    heat_mw: 6.0,
    function: Function::Weapon {
        damage: 60.0,
        fire_rate_hz: 2.0,
        range_km: 4.0,
    },
};

pub const INTERCEPTOR_DRIVE: Module = Module {
    id: "interceptor-drive",
    name: "Interceptor drive",
    size: Size::Medium,
    mass_t: 4.0,
    power_mw: 3.0,
    heat_mw: 8.0,
    function: Function::Engine { thrust_kn: 900.0 },
};

pub const LIGHT_DRIVE: Module = Module {
    id: "light-drive",
    name: "Light drive",
    size: Size::Small,
    mass_t: 2.0,
    power_mw: 1.5,
    heat_mw: 4.0,
    function: Function::Engine { thrust_kn: 400.0 },
};

pub const SMALL_REACTOR: Module = Module {
    id: "small-reactor",
    name: "Small reactor",
    size: Size::Small,
    mass_t: 3.0,
    power_mw: 0.0,
    heat_mw: 4.0,
    function: Function::Power { output_mw: 10.0 },
};

pub const LIGHT_DEFLECTOR: Module = Module {
    id: "light-deflector",
    name: "Light deflector",
    size: Size::Small,
    mass_t: 1.5,
    power_mw: 3.0,
    heat_mw: 2.0,
    function: Function::Shield {
        capacity: 200.0,
        regen_per_s: 15.0,
    },
};

pub const SHORT_RANGE_RADAR: Module = Module {
    id: "short-range-radar",
    name: "Short-range radar",
    size: Size::Small,
    mass_t: 0.5,
    power_mw: 1.0,
    heat_mw: 0.5,
    function: Function::Radar { range_km: 15.0 },
};

pub const FIN_RADIATOR: Module = Module {
    id: "fin-radiator",
    name: "Fin radiator",
    size: Size::Small,
    mass_t: 0.6,
    power_mw: 0.0,
    heat_mw: 0.0,
    function: Function::Radiator {
        dissipation_mw: 7.0,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_resolve() {
        let ids: HashSet<_> = MODULES.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), MODULES.len());
        for m in MODULES {
            assert_eq!(module(m.id), Some(m));
        }
        assert_eq!(hull("interceptor"), Some(&INTERCEPTOR));
    }

    #[test]
    fn every_module_fits_some_hull() {
        for m in MODULES {
            let fits = HULLS.iter().any(|h| {
                let fit = Loadout::new(h);
                (0..h.slots.len()).any(|i| fit.check(i, m).is_ok())
            });
            assert!(fits, "{} doesn't fit any hull", m.id);
        }
    }

    #[test]
    fn stock_interceptor_cruises_within_budget() {
        let stats = stock_interceptor().stats();
        assert!(stats.power_margin(stats.cruise) >= 0.0);
        assert_eq!(stats.time_to_overheat(stats.cruise), None);
    }

    #[test]
    fn stock_interceptor_is_short_in_combat() {
        let stats = stock_interceptor().stats();
        assert!(stats.power_margin(stats.combat) < 0.0);
        let secs = stats.time_to_overheat(stats.combat).unwrap();
        assert!((10.0..120.0).contains(&secs), "overheats in {secs}s");
    }

    #[test]
    fn lasers_trade_heat_for_damage() {
        let mut fit = stock_interceptor();
        let cannons = fit.stats();
        fit.fit(0, &LIGHT_LASER).unwrap();
        fit.fit(1, &LIGHT_LASER).unwrap();
        let lasers = fit.stats();
        assert!(lasers.dps > cannons.dps);
        assert!(lasers.weapon_range_km > cannons.weapon_range_km);
        assert!(
            lasers.time_to_overheat(lasers.combat).unwrap()
                < cannons.time_to_overheat(cannons.combat).unwrap()
        );
    }
}
