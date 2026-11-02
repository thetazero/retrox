//! Everything that goes in a slot.
//!
//! Within a size there are trade-offs rather than upgrades: the laser
//! out-damages and out-ranges the autocannon but runs hotter and hungrier;
//! a sprint drive out-pushes a cruise drive but cooks the ship doing it.

use retrox_sim::ship::{Function, Module, Size};

pub static MODULES: &[Module] = &[
    LIGHT_AUTOCANNON,
    LIGHT_LASER,
    AUTOCANNON,
    LASER,
    HEAVY_CANNON,
    LIGHT_DRIVE,
    INTERCEPTOR_DRIVE,
    SPRINT_DRIVE,
    CRUISE_DRIVE,
    HEAVY_SPRINT_DRIVE,
    HEAVY_CRUISE_DRIVE,
    LIGHT_RCS,
    RCS,
    HEAVY_RCS,
    SMALL_REACTOR,
    REACTOR,
    LARGE_REACTOR,
    LIGHT_DEFLECTOR,
    DEFLECTOR,
    HEAVY_DEFLECTOR,
    SHORT_RANGE_RADAR,
    LONG_RANGE_RADAR,
    FIN_RADIATOR,
    RADIATOR_PANEL,
    RADIATOR_ARRAY,
    CARGO_RACK,
    CARGO_POD,
    CARGO_HOLD,
];

const fn module(
    id: &'static str,
    name: &'static str,
    size: Size,
    mass_t: f64,
    power_mw: f64,
    heat_mw: f64,
    function: Function,
) -> Module {
    Module {
        id,
        name,
        size,
        mass_t,
        power_mw,
        heat_mw,
        function,
    }
}

const fn weapon(damage: f64, fire_rate_hz: f64, range_km: f64) -> Function {
    Function::Weapon {
        damage,
        fire_rate_hz,
        range_km,
    }
}

const fn engine(thrust_kn: f64) -> Function {
    Function::Engine { thrust_kn }
}

const fn rcs(torque_knm: f64) -> Function {
    Function::Rcs { torque_knm }
}

const fn power(output_mw: f64) -> Function {
    Function::Power { output_mw }
}

const fn shield(capacity: f64, regen_per_s: f64) -> Function {
    Function::Shield {
        capacity,
        regen_per_s,
    }
}

const fn radar(range_km: f64) -> Function {
    Function::Radar { range_km }
}

const fn radiator(dissipation_mw: f64) -> Function {
    Function::Radiator { dissipation_mw }
}

const fn cargo(capacity_t: f64) -> Function {
    Function::Cargo { capacity_t }
}

use Size::{Large as L, Medium as M, Small as S};

// Weapons. Autocannons are cheap and cool; lasers hit harder and further
// but draw and heat more.
#[rustfmt::skip]
pub const LIGHT_AUTOCANNON: Module = module("light-autocannon", "Light autocannon", S, 1.2, 2.0, 3.0, weapon(12.0, 8.0, 2.5));
#[rustfmt::skip]
pub const LIGHT_LASER: Module = module("light-laser", "Light laser", S, 0.9, 4.0, 6.0, weapon(60.0, 2.0, 4.0));
#[rustfmt::skip]
pub const AUTOCANNON: Module = module("autocannon", "Autocannon", M, 5.0, 5.0, 8.0, weapon(40.0, 6.0, 3.5));
#[rustfmt::skip]
pub const LASER: Module = module("laser", "Laser", M, 4.0, 10.0, 15.0, weapon(150.0, 2.0, 6.0));
#[rustfmt::skip]
pub const HEAVY_CANNON: Module = module("heavy-cannon", "Heavy cannon", L, 20.0, 15.0, 25.0, weapon(500.0, 1.5, 8.0));

// Engines. The interceptor drive is a compact high-output unit that fits
// a medium slot on a small ship; sprint drives push hard and run hot,
// cruise drives trade thrust for efficiency.
#[rustfmt::skip]
pub const LIGHT_DRIVE: Module = module("light-drive", "Light drive", S, 2.0, 1.5, 4.0, engine(400.0));
#[rustfmt::skip]
pub const INTERCEPTOR_DRIVE: Module = module("interceptor-drive", "Interceptor drive", M, 4.0, 3.0, 8.0, engine(900.0));
#[rustfmt::skip]
pub const SPRINT_DRIVE: Module = module("sprint-drive", "Sprint drive", M, 12.0, 8.0, 28.0, engine(3200.0));
#[rustfmt::skip]
pub const CRUISE_DRIVE: Module = module("cruise-drive", "Cruise drive", M, 12.0, 4.0, 15.0, engine(1800.0));
#[rustfmt::skip]
pub const HEAVY_SPRINT_DRIVE: Module = module("heavy-sprint-drive", "Heavy sprint drive", L, 60.0, 40.0, 150.0, engine(18000.0));
#[rustfmt::skip]
pub const HEAVY_CRUISE_DRIVE: Module = module("heavy-cruise-drive", "Heavy cruise drive", L, 60.0, 20.0, 80.0, engine(10000.0));

// Attitude thrusters.
#[rustfmt::skip]
pub const LIGHT_RCS: Module = module("light-rcs", "Light RCS", S, 0.8, 1.0, 1.0, rcs(1300.0));
#[rustfmt::skip]
pub const RCS: Module = module("rcs", "RCS", M, 4.0, 3.0, 3.0, rcs(30000.0));
#[rustfmt::skip]
pub const HEAVY_RCS: Module = module("heavy-rcs", "Heavy RCS", L, 20.0, 12.0, 12.0, rcs(400000.0));

// Power.
#[rustfmt::skip]
pub const SMALL_REACTOR: Module = module("small-reactor", "Small reactor", S, 3.0, 0.0, 4.0, power(10.0));
#[rustfmt::skip]
pub const REACTOR: Module = module("reactor", "Reactor", M, 18.0, 0.0, 15.0, power(40.0));
#[rustfmt::skip]
pub const LARGE_REACTOR: Module = module("large-reactor", "Large reactor", L, 100.0, 0.0, 70.0, power(200.0));

// Shields.
#[rustfmt::skip]
pub const LIGHT_DEFLECTOR: Module = module("light-deflector", "Light deflector", S, 1.5, 3.0, 2.0, shield(200.0, 15.0));
#[rustfmt::skip]
pub const DEFLECTOR: Module = module("deflector", "Deflector", M, 8.0, 10.0, 6.0, shield(1000.0, 60.0));
#[rustfmt::skip]
pub const HEAVY_DEFLECTOR: Module = module("heavy-deflector", "Heavy deflector", L, 40.0, 40.0, 25.0, shield(6000.0, 300.0));

// Sensors.
#[rustfmt::skip]
pub const SHORT_RANGE_RADAR: Module = module("short-range-radar", "Short-range radar", S, 0.5, 1.0, 0.5, radar(15.0));
#[rustfmt::skip]
pub const LONG_RANGE_RADAR: Module = module("long-range-radar", "Long-range radar", M, 3.0, 4.0, 2.0, radar(40.0));

// Radiators.
#[rustfmt::skip]
pub const FIN_RADIATOR: Module = module("fin-radiator", "Fin radiator", S, 0.6, 0.0, 0.0, radiator(7.0));
#[rustfmt::skip]
pub const RADIATOR_PANEL: Module = module("radiator-panel", "Radiator panel", M, 4.0, 0.0, 0.0, radiator(35.0));
#[rustfmt::skip]
pub const RADIATOR_ARRAY: Module = module("radiator-array", "Radiator array", L, 20.0, 0.0, 0.0, radiator(180.0));

// Cargo.
#[rustfmt::skip]
pub const CARGO_RACK: Module = module("cargo-rack", "Cargo rack", S, 0.5, 0.0, 0.0, cargo(6.0));
#[rustfmt::skip]
pub const CARGO_POD: Module = module("cargo-pod", "Cargo pod", M, 4.0, 0.0, 0.0, cargo(60.0));
#[rustfmt::skip]
pub const CARGO_HOLD: Module = module("cargo-hold", "Cargo hold", L, 30.0, 0.0, 0.0, cargo(500.0));
