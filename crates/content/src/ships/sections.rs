//! Sections: the pieces ships are built from.
//!
//! Small ships are a single built-in section each. Medium and large ships
//! take any bow, mid and stern section of their size, so the same frame can
//! be fitted as a gunboat or a freighter. Slot positions are relative to
//! the section's origin at its centre.

use retrox_sim::ship::{Position, Section, Size, Slot, SlotKind};

use Position::{Bow, Mid, Stern, Whole};
use Size::{Large as L, Medium as M, Small as S};
use SlotKind::{Cargo, Engine, JumpDrive, Power, Radar, Radiator, Rcs, Shield, Weapon};

/// Every swappable section (the ones that go in open mounts).
pub static SECTIONS: &[Section] = &[
    COMMAND_BOW_M,
    GUN_BOW_M,
    GUN_DECK_M,
    SHIELD_RING_M,
    CARGO_BAY_M,
    REACTOR_SECTION_M,
    SPRINT_STERN_M,
    CRUISE_STERN_M,
    COMMAND_BOW_L,
    GUN_BOW_L,
    GUN_DECK_L,
    SHIELD_RING_L,
    CARGO_BAY_L,
    REACTOR_SECTION_L,
    SPRINT_STERN_L,
    CRUISE_STERN_L,
];

const fn slot(kind: SlotKind, size: Size, position_m: [f64; 3]) -> Slot {
    Slot {
        kind,
        size,
        position_m,
    }
}

// --- Small ships: one built-in section each -------------------------------

/// Fast and fragile, with an engine oversized for its frame.
pub const INTERCEPTOR_BODY: Section = Section {
    id: "interceptor-body",
    name: "Interceptor body",
    position: Whole,
    size: S,
    mass_t: 14.0,
    length_m: 10.0,
    structure: 300.0,
    heat_capacity_mj: 120.0,
    base_dissipation_mw: 2.0,
    slots: &[
        slot(Weapon, S, [-1.6, -0.3, 3.0]),
        slot(Weapon, S, [1.6, -0.3, 3.0]),
        slot(Engine, M, [0.0, 0.0, -4.5]),
        slot(Rcs, S, [0.0, 0.0, 0.0]),
        slot(Power, S, [0.0, 0.0, -1.5]),
        slot(Shield, S, [0.0, 0.4, 0.0]),
        slot(Radar, S, [0.0, 0.0, 5.6]),
        slot(Radiator, S, [-1.2, 0.0, -2.5]),
        slot(Radiator, S, [1.2, 0.0, -2.5]),
        slot(JumpDrive, S, [0.0, 0.0, 0.0]),
    ],
};

/// Armoured and heavily armed for its size; slower to accelerate and turn.
pub const HEAVY_FIGHTER_BODY: Section = Section {
    id: "heavy-fighter-body",
    name: "Heavy fighter body",
    position: Whole,
    size: S,
    mass_t: 24.0,
    length_m: 13.0,
    structure: 650.0,
    heat_capacity_mj: 180.0,
    base_dissipation_mw: 3.0,
    slots: &[
        slot(Weapon, M, [0.0, -0.8, 5.0]),
        slot(Weapon, S, [-2.0, -0.3, 4.0]),
        slot(Weapon, S, [2.0, -0.3, 4.0]),
        slot(Engine, M, [0.0, 0.0, -6.0]),
        slot(Rcs, S, [0.0, 0.0, 0.0]),
        slot(Power, S, [-0.8, 0.0, -2.5]),
        slot(Power, S, [0.8, 0.0, -2.5]),
        slot(Shield, S, [0.0, 0.6, 0.0]),
        slot(Radar, S, [0.0, 0.8, 3.5]),
        slot(Radiator, S, [-1.6, 0.0, -3.5]),
        slot(Radiator, S, [1.6, 0.0, -3.5]),
        slot(Radiator, S, [0.0, 1.2, -3.5]),
        slot(JumpDrive, S, [0.0, 0.0, 0.0]),
    ],
};

/// An engine with a seat. No guns, no shield, no hold.
pub const RACER_BODY: Section = Section {
    id: "racer-body",
    name: "Racer body",
    position: Whole,
    size: S,
    mass_t: 8.0,
    length_m: 9.0,
    structure: 150.0,
    heat_capacity_mj: 100.0,
    base_dissipation_mw: 2.0,
    slots: &[
        slot(Engine, M, [0.0, 0.0, -3.5]),
        slot(Rcs, S, [0.0, 0.0, 0.5]),
        slot(Power, S, [0.0, 0.0, -1.0]),
        slot(Radar, S, [0.0, 0.4, 3.0]),
        slot(Radiator, S, [-1.0, 0.0, -2.5]),
        slot(Radiator, S, [1.0, 0.0, -2.5]),
        slot(JumpDrive, S, [0.0, 0.0, 0.0]),
    ],
};

/// Small hold, big engine: high-value cargo, delivered fast. A chin mount
/// for a mining laser and a belly gun to defend it.
pub const COURIER_BODY: Section = Section {
    id: "courier-body",
    name: "Courier body",
    position: Whole,
    size: S,
    mass_t: 12.0,
    length_m: 12.0,
    structure: 250.0,
    heat_capacity_mj: 120.0,
    base_dissipation_mw: 2.0,
    slots: &[
        // A chin mount under the nose, for a mining laser.
        slot(Weapon, S, [0.0, -0.75, 3.6]),
        slot(Engine, M, [0.0, 0.0, -5.0]),
        slot(Rcs, S, [0.0, 0.0, 0.5]),
        slot(Power, S, [0.0, 0.0, -2.0]),
        slot(Shield, S, [0.0, 0.5, 0.0]),
        slot(Radar, S, [0.0, 0.6, 4.0]),
        slot(Radiator, S, [-1.0, 0.0, -3.5]),
        slot(Radiator, S, [1.0, 0.0, -3.5]),
        slot(Cargo, S, [-0.9, -0.4, 1.0]),
        slot(Cargo, S, [0.9, -0.4, 1.0]),
        slot(JumpDrive, S, [0.0, 0.0, 0.0]),
        // A belly gun behind the chin mount, so a new pilot can defend
        // themselves (added last, so older saves keep their slots).
        slot(Weapon, S, [0.0, -1.25, 1.0]),
    ],
};

/// A box with engines: the cheapest way to start trading.
pub const LIGHT_TRADER_BODY: Section = Section {
    id: "light-trader-body",
    name: "Light trader body",
    position: Whole,
    size: S,
    mass_t: 20.0,
    length_m: 16.0,
    structure: 350.0,
    heat_capacity_mj: 150.0,
    base_dissipation_mw: 3.0,
    slots: &[
        slot(Weapon, S, [0.0, -0.5, 7.0]),
        slot(Engine, S, [-1.0, 0.0, -7.5]),
        slot(Engine, S, [1.0, 0.0, -7.5]),
        slot(Rcs, S, [0.0, 0.0, 0.0]),
        slot(Power, S, [0.0, 0.0, -5.0]),
        slot(Shield, S, [0.0, 1.0, 2.0]),
        slot(Radar, S, [0.0, 1.2, 6.0]),
        slot(Radiator, S, [-1.2, 1.0, -5.0]),
        slot(Radiator, S, [1.2, 1.0, -5.0]),
        slot(Cargo, S, [-1.0, -0.6, 3.0]),
        slot(Cargo, S, [1.0, -0.6, 3.0]),
        slot(Cargo, S, [-1.0, -0.6, -1.0]),
        slot(Cargo, S, [1.0, -0.6, -1.0]),
        slot(JumpDrive, S, [0.0, 0.0, 0.0]),
    ],
};

// --- Medium sections ------------------------------------------------------

/// Bridge, long-range radar and a shield; one gun for show.
pub const COMMAND_BOW_M: Section = Section {
    id: "command-bow-m",
    name: "Command bow",
    position: Bow,
    size: M,
    mass_t: 40.0,
    length_m: 12.0,
    structure: 600.0,
    heat_capacity_mj: 300.0,
    base_dissipation_mw: 5.0,
    slots: &[
        slot(Weapon, S, [0.0, -1.5, 5.5]),
        slot(Rcs, M, [0.0, 0.0, 2.0]),
        slot(Shield, M, [0.0, 2.0, 0.0]),
        slot(Radar, M, [0.0, 2.5, 4.0]),
    ],
};

/// Two forward guns in place of the bridge's sensor mast.
pub const GUN_BOW_M: Section = Section {
    id: "gun-bow-m",
    name: "Gun bow",
    position: Bow,
    size: M,
    mass_t: 45.0,
    length_m: 12.0,
    structure: 750.0,
    heat_capacity_mj: 300.0,
    base_dissipation_mw: 5.0,
    slots: &[
        slot(Weapon, M, [-2.5, -1.0, 5.0]),
        slot(Weapon, M, [2.5, -1.0, 5.0]),
        slot(Rcs, M, [0.0, 0.0, 2.0]),
        slot(Shield, M, [0.0, 2.0, 0.0]),
        slot(Radar, S, [0.0, 2.2, 4.0]),
    ],
};

pub const GUN_DECK_M: Section = Section {
    id: "gun-deck-m",
    name: "Gun deck",
    position: Mid,
    size: M,
    mass_t: 50.0,
    length_m: 14.0,
    structure: 800.0,
    heat_capacity_mj: 350.0,
    base_dissipation_mw: 6.0,
    slots: &[
        slot(Weapon, M, [-3.0, 0.0, 3.0]),
        slot(Weapon, M, [3.0, 0.0, 3.0]),
        slot(Weapon, S, [0.0, 3.0, 0.0]),
        slot(Weapon, S, [0.0, -3.0, 0.0]),
        slot(Radiator, M, [0.0, 0.0, -4.0]),
    ],
};

pub const SHIELD_RING_M: Section = Section {
    id: "shield-ring-m",
    name: "Shield ring",
    position: Mid,
    size: M,
    mass_t: 45.0,
    length_m: 10.0,
    structure: 700.0,
    heat_capacity_mj: 300.0,
    base_dissipation_mw: 5.0,
    slots: &[
        slot(Shield, M, [0.0, 3.0, 0.0]),
        slot(Shield, M, [0.0, -3.0, 0.0]),
        slot(Power, M, [0.0, 0.0, 0.0]),
        slot(Radiator, M, [0.0, 0.0, -3.0]),
    ],
};

pub const CARGO_BAY_M: Section = Section {
    id: "cargo-bay-m",
    name: "Cargo bay",
    position: Mid,
    size: M,
    mass_t: 30.0,
    length_m: 16.0,
    structure: 500.0,
    heat_capacity_mj: 200.0,
    base_dissipation_mw: 4.0,
    slots: &[
        slot(Cargo, M, [0.0, 0.0, 5.0]),
        slot(Cargo, M, [0.0, 0.0, 0.0]),
        slot(Cargo, M, [0.0, 0.0, -5.0]),
    ],
};

pub const REACTOR_SECTION_M: Section = Section {
    id: "reactor-section-m",
    name: "Reactor section",
    position: Mid,
    size: M,
    mass_t: 45.0,
    length_m: 10.0,
    structure: 600.0,
    heat_capacity_mj: 500.0,
    base_dissipation_mw: 6.0,
    slots: &[
        slot(Power, M, [0.0, 0.0, 2.0]),
        slot(Power, M, [0.0, 0.0, -2.0]),
        slot(Radiator, M, [-4.0, 0.0, 0.0]),
        slot(Radiator, M, [4.0, 0.0, 0.0]),
    ],
};

/// Twin engines: more thrust than a cruise stern, and more heat to shed.
pub const SPRINT_STERN_M: Section = Section {
    id: "sprint-stern-m",
    name: "Sprint stern",
    position: Stern,
    size: M,
    mass_t: 50.0,
    length_m: 12.0,
    structure: 600.0,
    heat_capacity_mj: 400.0,
    base_dissipation_mw: 6.0,
    slots: &[
        slot(Engine, M, [-2.0, 0.0, -5.0]),
        slot(Engine, M, [2.0, 0.0, -5.0]),
        slot(Power, M, [0.0, 0.0, 1.0]),
        slot(Radiator, M, [0.0, 3.5, -2.0]),
        slot(Radiator, M, [0.0, -3.5, -2.0]),
        slot(JumpDrive, M, [0.0, 0.0, 0.0]),
    ],
};

/// One engine, plenty of radiator: steady all-day running.
pub const CRUISE_STERN_M: Section = Section {
    id: "cruise-stern-m",
    name: "Cruise stern",
    position: Stern,
    size: M,
    mass_t: 45.0,
    length_m: 12.0,
    structure: 600.0,
    heat_capacity_mj: 400.0,
    base_dissipation_mw: 6.0,
    slots: &[
        slot(Engine, M, [0.0, 0.0, -5.0]),
        slot(Power, M, [0.0, 0.0, 1.0]),
        slot(Radiator, M, [-3.5, 0.0, -2.0]),
        slot(Radiator, M, [3.5, 0.0, -2.0]),
        slot(JumpDrive, M, [0.0, 0.0, 0.0]),
    ],
};

// --- Large sections -------------------------------------------------------

pub const COMMAND_BOW_L: Section = Section {
    id: "command-bow-l",
    name: "Command bow",
    position: Bow,
    size: L,
    mass_t: 300.0,
    length_m: 25.0,
    structure: 4000.0,
    heat_capacity_mj: 2000.0,
    base_dissipation_mw: 30.0,
    slots: &[
        slot(Weapon, M, [0.0, -3.0, 11.0]),
        slot(Rcs, L, [0.0, 0.0, 4.0]),
        slot(Shield, L, [0.0, 4.0, 0.0]),
        slot(Radar, M, [0.0, 5.0, 8.0]),
    ],
};

pub const GUN_BOW_L: Section = Section {
    id: "gun-bow-l",
    name: "Gun bow",
    position: Bow,
    size: L,
    mass_t: 350.0,
    length_m: 25.0,
    structure: 5000.0,
    heat_capacity_mj: 2000.0,
    base_dissipation_mw: 30.0,
    slots: &[
        slot(Weapon, L, [-5.0, -2.0, 10.0]),
        slot(Weapon, L, [5.0, -2.0, 10.0]),
        slot(Rcs, L, [0.0, 0.0, 4.0]),
        slot(Shield, L, [0.0, 4.0, 0.0]),
        slot(Radar, M, [0.0, 4.5, 8.0]),
    ],
};

pub const GUN_DECK_L: Section = Section {
    id: "gun-deck-l",
    name: "Gun deck",
    position: Mid,
    size: L,
    mass_t: 400.0,
    length_m: 30.0,
    structure: 5500.0,
    heat_capacity_mj: 2500.0,
    base_dissipation_mw: 35.0,
    slots: &[
        slot(Weapon, L, [-6.0, 0.0, 6.0]),
        slot(Weapon, L, [6.0, 0.0, 6.0]),
        slot(Weapon, M, [0.0, 6.0, 0.0]),
        slot(Weapon, M, [0.0, -6.0, 0.0]),
        slot(Radiator, L, [0.0, 0.0, -10.0]),
    ],
};

pub const SHIELD_RING_L: Section = Section {
    id: "shield-ring-l",
    name: "Shield ring",
    position: Mid,
    size: L,
    mass_t: 350.0,
    length_m: 20.0,
    structure: 4500.0,
    heat_capacity_mj: 2000.0,
    base_dissipation_mw: 30.0,
    slots: &[
        slot(Shield, L, [0.0, 6.0, 0.0]),
        slot(Shield, L, [0.0, -6.0, 0.0]),
        slot(Power, L, [0.0, 0.0, 0.0]),
        slot(Radiator, L, [0.0, 0.0, -7.0]),
    ],
};

pub const CARGO_BAY_L: Section = Section {
    id: "cargo-bay-l",
    name: "Cargo bay",
    position: Mid,
    size: L,
    mass_t: 250.0,
    length_m: 35.0,
    structure: 3000.0,
    heat_capacity_mj: 1500.0,
    base_dissipation_mw: 25.0,
    slots: &[
        slot(Cargo, L, [0.0, 0.0, 11.0]),
        slot(Cargo, L, [0.0, 0.0, 0.0]),
        slot(Cargo, L, [0.0, 0.0, -11.0]),
    ],
};

pub const REACTOR_SECTION_L: Section = Section {
    id: "reactor-section-l",
    name: "Reactor section",
    position: Mid,
    size: L,
    mass_t: 350.0,
    length_m: 20.0,
    structure: 4000.0,
    heat_capacity_mj: 3500.0,
    base_dissipation_mw: 35.0,
    slots: &[
        slot(Power, L, [0.0, 0.0, 4.0]),
        slot(Power, L, [0.0, 0.0, -4.0]),
        slot(Radiator, L, [-8.0, 0.0, 0.0]),
        slot(Radiator, L, [8.0, 0.0, 0.0]),
    ],
};

pub const SPRINT_STERN_L: Section = Section {
    id: "sprint-stern-l",
    name: "Sprint stern",
    position: Stern,
    size: L,
    mass_t: 400.0,
    length_m: 25.0,
    structure: 4000.0,
    heat_capacity_mj: 2500.0,
    base_dissipation_mw: 35.0,
    slots: &[
        slot(Engine, L, [-4.0, 0.0, -10.0]),
        slot(Engine, L, [4.0, 0.0, -10.0]),
        slot(Power, L, [0.0, 0.0, 2.0]),
        slot(Radiator, L, [0.0, 7.0, -4.0]),
        slot(Radiator, L, [0.0, -7.0, -4.0]),
        slot(JumpDrive, L, [0.0, 0.0, 0.0]),
    ],
};

pub const CRUISE_STERN_L: Section = Section {
    id: "cruise-stern-l",
    name: "Cruise stern",
    position: Stern,
    size: L,
    mass_t: 350.0,
    length_m: 25.0,
    structure: 4000.0,
    heat_capacity_mj: 2500.0,
    base_dissipation_mw: 35.0,
    slots: &[
        slot(Engine, L, [0.0, 0.0, -10.0]),
        slot(Power, L, [0.0, 0.0, 2.0]),
        slot(Radiator, L, [-7.0, 0.0, -4.0]),
        slot(Radiator, L, [7.0, 0.0, -4.0]),
        slot(JumpDrive, L, [0.0, 0.0, 0.0]),
    ],
};
