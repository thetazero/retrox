//! Hulls: the frames ships are built on.
//!
//! Small hulls are a single built-in section. Medium and large hulls are a
//! frame with open bow, mid and stern mounts; what makes them differ
//! before anything is fitted is the frame itself — how many mid mounts,
//! and how heavy and tough the frame is.

use retrox_sim::ship::{Hull, Mount, Position, ShipRole, Size};

use super::sections::*;

pub static HULLS: &[Hull] = &[
    INTERCEPTOR,
    HEAVY_FIGHTER,
    RACER,
    COURIER,
    LIGHT_TRADER,
    GUNSHIP,
    BLOCKADE_RUNNER,
    HAULER,
    FRIGATE,
    EXPRESS_FREIGHTER,
    BULK_FREIGHTER,
];

const fn open(position: Position, size: Size) -> Mount {
    Mount::Open { position, size }
}

const BOW_M: Mount = open(Position::Bow, Size::Medium);
const MID_M: Mount = open(Position::Mid, Size::Medium);
const STERN_M: Mount = open(Position::Stern, Size::Medium);
const BOW_L: Mount = open(Position::Bow, Size::Large);
const MID_L: Mount = open(Position::Mid, Size::Large);
const STERN_L: Mount = open(Position::Stern, Size::Large);

const fn small(
    id: &'static str,
    name: &'static str,
    role: ShipRole,
    mounts: &'static [Mount],
) -> Hull {
    Hull {
        id,
        name,
        role,
        size: Size::Small,
        frame_mass_t: 0.0,
        frame_structure: 0.0,
        mounts,
    }
}

// --- Small ----------------------------------------------------------------

pub const INTERCEPTOR: Hull = small(
    "interceptor",
    "Interceptor",
    ShipRole::Combat,
    &[Mount::Fixed(&INTERCEPTOR_BODY)],
);

pub const HEAVY_FIGHTER: Hull = small(
    "heavy-fighter",
    "Heavy fighter",
    ShipRole::Combat,
    &[Mount::Fixed(&HEAVY_FIGHTER_BODY)],
);

pub const RACER: Hull = small(
    "racer",
    "Racer",
    ShipRole::Speed,
    &[Mount::Fixed(&RACER_BODY)],
);

pub const COURIER: Hull = small(
    "courier",
    "Courier",
    ShipRole::Speed,
    &[Mount::Fixed(&COURIER_BODY)],
);

pub const LIGHT_TRADER: Hull = small(
    "light-trader",
    "Light trader",
    ShipRole::Trade,
    &[Mount::Fixed(&LIGHT_TRADER_BODY)],
);

// --- Medium ---------------------------------------------------------------

/// Heavy, armoured frame; one mid mount.
pub const GUNSHIP: Hull = Hull {
    id: "gunship",
    name: "Gunship",
    role: ShipRole::Combat,
    size: Size::Medium,
    frame_mass_t: 30.0,
    frame_structure: 1500.0,
    mounts: &[BOW_M, MID_M, STERN_M],
};

/// The same layout as the gunship on a stripped-down frame: lighter and
/// quicker, but it won't take a beating.
pub const BLOCKADE_RUNNER: Hull = Hull {
    id: "blockade-runner",
    name: "Blockade runner",
    role: ShipRole::Speed,
    size: Size::Medium,
    frame_mass_t: 12.0,
    frame_structure: 400.0,
    mounts: &[BOW_M, MID_M, STERN_M],
};

/// Two mid mounts on a long, light frame.
pub const HAULER: Hull = Hull {
    id: "hauler",
    name: "Hauler",
    role: ShipRole::Trade,
    size: Size::Medium,
    frame_mass_t: 25.0,
    frame_structure: 800.0,
    mounts: &[BOW_M, MID_M, MID_M, STERN_M],
};

// --- Large ----------------------------------------------------------------

/// Two mid mounts on a heavy, armoured frame.
pub const FRIGATE: Hull = Hull {
    id: "frigate",
    name: "Frigate",
    role: ShipRole::Combat,
    size: Size::Large,
    frame_mass_t: 200.0,
    frame_structure: 10000.0,
    mounts: &[BOW_L, MID_L, MID_L, STERN_L],
};

/// One mid mount on a light frame: a fast large ship.
pub const EXPRESS_FREIGHTER: Hull = Hull {
    id: "express-freighter",
    name: "Express freighter",
    role: ShipRole::Speed,
    size: Size::Large,
    frame_mass_t: 80.0,
    frame_structure: 2500.0,
    mounts: &[BOW_L, MID_L, STERN_L],
};

/// Three mid mounts: the longest, slowest hull.
pub const BULK_FREIGHTER: Hull = Hull {
    id: "bulk-freighter",
    name: "Bulk freighter",
    role: ShipRole::Trade,
    size: Size::Large,
    frame_mass_t: 150.0,
    frame_structure: 5000.0,
    mounts: &[BOW_L, MID_L, MID_L, MID_L, STERN_L],
};
