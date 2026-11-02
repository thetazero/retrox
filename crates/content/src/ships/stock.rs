//! Stock fits: each hull as it comes off the lot.

use retrox_sim::ship::{FitError, Loadout, Module, Section};

use super::hulls::*;
use super::modules::*;
use super::sections::*;

/// A complete fit: a section for each mount and a module for every slot.
pub struct StockFit {
    pub hull: &'static retrox_sim::ship::Hull,
    /// The section for each mount, nose to tail, paired with the module for
    /// each of that section's slots. `None` keeps a fixed mount's own
    /// section.
    pub sections: &'static [(Option<&'static Section>, &'static [&'static Module])],
}

impl StockFit {
    pub fn build(&self) -> Result<Loadout, FitError> {
        let mut fit = Loadout::new(self.hull);
        for (mount, &(section, modules)) in self.sections.iter().enumerate() {
            if let Some(section) = section {
                fit.set_section(mount, section)?;
            }
            for (slot, module) in modules.iter().enumerate() {
                fit.fit(mount, slot, module)?;
            }
        }
        Ok(fit)
    }
}

/// The stock fit of the hull with this id, built.
pub fn stock(hull_id: &str) -> Option<Loadout> {
    let fit = STOCK.iter().find(|s| s.hull.id == hull_id)?;
    Some(fit.build().expect("stock fits are valid"))
}

pub static STOCK: &[StockFit] = &[
    StockFit {
        hull: &INTERCEPTOR,
        sections: &[(
            None,
            &[
                &LIGHT_AUTOCANNON,
                &LIGHT_AUTOCANNON,
                &INTERCEPTOR_DRIVE,
                &LIGHT_RCS,
                &SMALL_REACTOR,
                &LIGHT_DEFLECTOR,
                &SHORT_RANGE_RADAR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
            ],
        )],
    },
    StockFit {
        hull: &HEAVY_FIGHTER,
        sections: &[(
            None,
            &[
                &AUTOCANNON,
                &LIGHT_AUTOCANNON,
                &LIGHT_AUTOCANNON,
                &INTERCEPTOR_DRIVE,
                &LIGHT_RCS,
                &SMALL_REACTOR,
                &SMALL_REACTOR,
                &LIGHT_DEFLECTOR,
                &SHORT_RANGE_RADAR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
            ],
        )],
    },
    StockFit {
        hull: &RACER,
        sections: &[(
            None,
            &[
                &INTERCEPTOR_DRIVE,
                &LIGHT_RCS,
                &SMALL_REACTOR,
                &SHORT_RANGE_RADAR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
            ],
        )],
    },
    StockFit {
        hull: &COURIER,
        sections: &[(
            None,
            &[
                &INTERCEPTOR_DRIVE,
                &LIGHT_RCS,
                &SMALL_REACTOR,
                &LIGHT_DEFLECTOR,
                &SHORT_RANGE_RADAR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
                &CARGO_RACK,
                &CARGO_RACK,
            ],
        )],
    },
    StockFit {
        hull: &LIGHT_TRADER,
        sections: &[(
            None,
            &[
                &LIGHT_AUTOCANNON,
                &LIGHT_DRIVE,
                &LIGHT_DRIVE,
                &LIGHT_RCS,
                &SMALL_REACTOR,
                &LIGHT_DEFLECTOR,
                &SHORT_RANGE_RADAR,
                &FIN_RADIATOR,
                &FIN_RADIATOR,
                &CARGO_RACK,
                &CARGO_RACK,
                &CARGO_RACK,
                &CARGO_RACK,
            ],
        )],
    },
    StockFit {
        hull: &GUNSHIP,
        sections: &[
            (
                Some(&GUN_BOW_M),
                &[
                    &AUTOCANNON,
                    &AUTOCANNON,
                    &RCS,
                    &DEFLECTOR,
                    &SHORT_RANGE_RADAR,
                ],
            ),
            (
                Some(&GUN_DECK_M),
                &[
                    &AUTOCANNON,
                    &AUTOCANNON,
                    &LIGHT_AUTOCANNON,
                    &LIGHT_AUTOCANNON,
                    &RADIATOR_PANEL,
                ],
            ),
            (
                Some(&SPRINT_STERN_M),
                &[
                    &SPRINT_DRIVE,
                    &SPRINT_DRIVE,
                    &REACTOR,
                    &RADIATOR_PANEL,
                    &RADIATOR_PANEL,
                ],
            ),
        ],
    },
    StockFit {
        hull: &BLOCKADE_RUNNER,
        sections: &[
            (
                Some(&COMMAND_BOW_M),
                &[&LIGHT_AUTOCANNON, &RCS, &DEFLECTOR, &LONG_RANGE_RADAR],
            ),
            (Some(&CARGO_BAY_M), &[&CARGO_POD, &CARGO_POD, &CARGO_POD]),
            (
                Some(&SPRINT_STERN_M),
                &[
                    &SPRINT_DRIVE,
                    &SPRINT_DRIVE,
                    &REACTOR,
                    &RADIATOR_PANEL,
                    &RADIATOR_PANEL,
                ],
            ),
        ],
    },
    StockFit {
        hull: &HAULER,
        sections: &[
            (
                Some(&COMMAND_BOW_M),
                &[&LIGHT_AUTOCANNON, &RCS, &DEFLECTOR, &LONG_RANGE_RADAR],
            ),
            (Some(&CARGO_BAY_M), &[&CARGO_POD, &CARGO_POD, &CARGO_POD]),
            (Some(&CARGO_BAY_M), &[&CARGO_POD, &CARGO_POD, &CARGO_POD]),
            (
                Some(&CRUISE_STERN_M),
                &[&CRUISE_DRIVE, &REACTOR, &RADIATOR_PANEL, &RADIATOR_PANEL],
            ),
        ],
    },
    StockFit {
        hull: &FRIGATE,
        sections: &[
            (
                Some(&GUN_BOW_L),
                &[
                    &HEAVY_CANNON,
                    &HEAVY_CANNON,
                    &HEAVY_RCS,
                    &HEAVY_DEFLECTOR,
                    &LONG_RANGE_RADAR,
                ],
            ),
            (
                Some(&GUN_DECK_L),
                &[
                    &HEAVY_CANNON,
                    &HEAVY_CANNON,
                    &AUTOCANNON,
                    &AUTOCANNON,
                    &RADIATOR_ARRAY,
                ],
            ),
            (
                Some(&SHIELD_RING_L),
                &[
                    &HEAVY_DEFLECTOR,
                    &HEAVY_DEFLECTOR,
                    &LARGE_REACTOR,
                    &RADIATOR_ARRAY,
                ],
            ),
            (
                Some(&SPRINT_STERN_L),
                &[
                    &HEAVY_SPRINT_DRIVE,
                    &HEAVY_SPRINT_DRIVE,
                    &LARGE_REACTOR,
                    &RADIATOR_ARRAY,
                    &RADIATOR_ARRAY,
                ],
            ),
        ],
    },
    StockFit {
        hull: &EXPRESS_FREIGHTER,
        sections: &[
            (
                Some(&COMMAND_BOW_L),
                &[&AUTOCANNON, &HEAVY_RCS, &HEAVY_DEFLECTOR, &LONG_RANGE_RADAR],
            ),
            (Some(&CARGO_BAY_L), &[&CARGO_HOLD, &CARGO_HOLD, &CARGO_HOLD]),
            (
                Some(&SPRINT_STERN_L),
                &[
                    &HEAVY_SPRINT_DRIVE,
                    &HEAVY_SPRINT_DRIVE,
                    &LARGE_REACTOR,
                    &RADIATOR_ARRAY,
                    &RADIATOR_ARRAY,
                ],
            ),
        ],
    },
    StockFit {
        hull: &BULK_FREIGHTER,
        sections: &[
            (
                Some(&COMMAND_BOW_L),
                &[&AUTOCANNON, &HEAVY_RCS, &HEAVY_DEFLECTOR, &LONG_RANGE_RADAR],
            ),
            (Some(&CARGO_BAY_L), &[&CARGO_HOLD, &CARGO_HOLD, &CARGO_HOLD]),
            (Some(&CARGO_BAY_L), &[&CARGO_HOLD, &CARGO_HOLD, &CARGO_HOLD]),
            (Some(&CARGO_BAY_L), &[&CARGO_HOLD, &CARGO_HOLD, &CARGO_HOLD]),
            (
                Some(&CRUISE_STERN_L),
                &[
                    &HEAVY_CRUISE_DRIVE,
                    &LARGE_REACTOR,
                    &RADIATOR_ARRAY,
                    &RADIATOR_ARRAY,
                ],
            ),
        ],
    },
];
