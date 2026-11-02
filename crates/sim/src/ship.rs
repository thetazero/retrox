//! Ship fitting: hulls with typed slots, the modules that go in them, and
//! the stats a fitted ship ends up with.
//!
//! See `docs/DESIGN.md` §7.4. A small ship is a fixed [`Hull`] with a list
//! of [`Slot`]s. Each slot takes one [`Module`] of the same [`SlotKind`] and
//! the same or a smaller [`Size`]. Two budgets tie a fit together:
//!
//! - **Power:** reactors produce it; weapons, shields, radar, engines and
//!   RCS thrusters draw it while active.
//! - **Heat:** every active module makes heat; the hull sheds some on its
//!   own and radiators shed more. Surplus heat fills the hull's heat
//!   capacity, and when that's full the ship is overheating.
//!
//! A fit is allowed to be over budget — an interceptor that can't run
//! everything at once is a choice the pilot manages in flight, so
//! [`ShipStats`] reports both budgets rather than rejecting the fit.
//!
//! Turning comes from RCS (reaction control system) thrusters: their
//! torque against the hull's moment of inertia, which grows with mass and
//! with the square of the hull's length. Heavy or long ships turn slowly.
//!
//! Units: tonnes, kilonewtons (so kN / t = m/s²), kilonewton-metres,
//! megawatts, megajoules, metres, kilometres.

use std::fmt;

/// Slot and module size. A slot accepts modules of its own size or smaller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
    Small,
    Medium,
    Large,
}

/// What a slot is for. A module only fits a slot of its own kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotKind {
    Weapon,
    Engine,
    /// Attitude thrusters, for turning.
    Rcs,
    Power,
    Shield,
    Radar,
    Radiator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub size: Size,
    /// Mount point relative to the hull origin, in metres: +x right,
    /// +y up, +z forward. Used to place the module's visual and, for
    /// weapons, where shots leave the ship.
    pub position_m: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    pub id: &'static str,
    pub name: &'static str,
    pub size: Size,
    pub mass_t: f64,
    /// Nose to tail. Sets the moment of inertia, so long hulls turn slower.
    pub length_m: f64,
    pub structure: f64,
    /// Heat the hull can soak before it overheats.
    pub heat_capacity_mj: f64,
    /// Heat the bare hull sheds without any radiators.
    pub base_dissipation_mw: f64,
    pub slots: &'static [Slot],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub id: &'static str,
    pub name: &'static str,
    pub size: Size,
    pub mass_t: f64,
    /// Power drawn while active.
    pub power_mw: f64,
    /// Heat produced while active.
    pub heat_mw: f64,
    pub function: Function,
}

impl Module {
    pub fn kind(&self) -> SlotKind {
        self.function.kind()
    }
}

/// What a module does, with the numbers specific to that job.
#[derive(Clone, Debug, PartialEq)]
pub enum Function {
    Weapon {
        damage: f64,
        fire_rate_hz: f64,
        range_km: f64,
    },
    Engine {
        thrust_kn: f64,
    },
    Rcs {
        torque_knm: f64,
    },
    Power {
        output_mw: f64,
    },
    Shield {
        capacity: f64,
        regen_per_s: f64,
    },
    Radar {
        range_km: f64,
    },
    Radiator {
        dissipation_mw: f64,
    },
}

impl Function {
    pub fn kind(&self) -> SlotKind {
        match self {
            Self::Weapon { .. } => SlotKind::Weapon,
            Self::Engine { .. } => SlotKind::Engine,
            Self::Rcs { .. } => SlotKind::Rcs,
            Self::Power { .. } => SlotKind::Power,
            Self::Shield { .. } => SlotKind::Shield,
            Self::Radar { .. } => SlotKind::Radar,
            Self::Radiator { .. } => SlotKind::Radiator,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FitError {
    NoSuchSlot { slot: usize, slots: usize },
    WrongKind { slot: SlotKind, module: SlotKind },
    TooBig { slot: Size, module: Size },
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchSlot { slot, slots } => {
                write!(f, "slot {slot} doesn't exist (hull has {slots} slots)")
            }
            Self::WrongKind { slot, module } => {
                write!(f, "a {module:?} module can't go in a {slot:?} slot")
            }
            Self::TooBig { slot, module } => {
                write!(f, "a {module:?} module is too big for a {slot:?} slot")
            }
        }
    }
}

impl std::error::Error for FitError {}

/// A hull and what's fitted in each of its slots.
///
/// Refitting is only allowed while docked (`docs/DESIGN.md` §7.4). Ships
/// don't exist in the sim state yet, so whoever calls [`Loadout::fit`]
/// enforces that until they do.
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout<'a> {
    hull: &'a Hull,
    modules: Vec<Option<&'a Module>>,
}

impl<'a> Loadout<'a> {
    /// A hull with every slot empty.
    pub fn new(hull: &'a Hull) -> Self {
        Self {
            hull,
            modules: vec![None; hull.slots.len()],
        }
    }

    pub fn hull(&self) -> &'a Hull {
        self.hull
    }

    /// Each slot alongside whatever is fitted in it.
    pub fn slots(&self) -> impl Iterator<Item = (&'a Slot, Option<&'a Module>)> + '_ {
        self.hull.slots.iter().zip(self.modules.iter().copied())
    }

    /// Whether `module` could go in `slot`, without fitting it.
    pub fn check(&self, slot: usize, module: &Module) -> Result<(), FitError> {
        let s = self.hull.slots.get(slot).ok_or(FitError::NoSuchSlot {
            slot,
            slots: self.hull.slots.len(),
        })?;
        if module.kind() != s.kind {
            return Err(FitError::WrongKind {
                slot: s.kind,
                module: module.kind(),
            });
        }
        if module.size > s.size {
            return Err(FitError::TooBig {
                slot: s.size,
                module: module.size,
            });
        }
        Ok(())
    }

    /// Fit `module` into `slot`, returning whatever was there before.
    pub fn fit(&mut self, slot: usize, module: &'a Module) -> Result<Option<&'a Module>, FitError> {
        self.check(slot, module)?;
        Ok(self.modules[slot].replace(module))
    }

    /// Empty `slot`, returning whatever was there.
    pub fn remove(&mut self, slot: usize) -> Option<&'a Module> {
        self.modules.get_mut(slot)?.take()
    }

    pub fn stats(&self) -> ShipStats {
        let hull = self.hull;
        let mut stats = ShipStats {
            mass_t: hull.mass_t,
            length_m: hull.length_m,
            structure: hull.structure,
            heat_capacity_mj: hull.heat_capacity_mj,
            heat_dissipation_mw: hull.base_dissipation_mw,
            ..ShipStats::default()
        };
        for module in self.modules.iter().flatten() {
            stats.mass_t += module.mass_t;
            stats.combat.power_mw += module.power_mw;
            stats.combat.heat_mw += module.heat_mw;
            if module.kind() != SlotKind::Weapon {
                stats.cruise.power_mw += module.power_mw;
                stats.cruise.heat_mw += module.heat_mw;
            }
            match module.function {
                Function::Weapon {
                    damage,
                    fire_rate_hz,
                    range_km,
                } => {
                    stats.dps += damage * fire_rate_hz;
                    stats.weapon_range_km = stats.weapon_range_km.max(range_km);
                }
                Function::Engine { thrust_kn } => stats.thrust_kn += thrust_kn,
                Function::Rcs { torque_knm } => stats.rcs_torque_knm += torque_knm,
                Function::Power { output_mw } => stats.power_supply_mw += output_mw,
                Function::Shield {
                    capacity,
                    regen_per_s,
                } => {
                    stats.shield += capacity;
                    stats.shield_regen_per_s += regen_per_s;
                }
                Function::Radar { range_km } => {
                    stats.radar_range_km = stats.radar_range_km.max(range_km);
                }
                Function::Radiator { dissipation_mw } => {
                    stats.heat_dissipation_mw += dissipation_mw;
                }
            }
        }
        stats
    }
}

/// Power draw and heat output for one way of flying the ship.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub power_mw: f64,
    pub heat_mw: f64,
}

/// What a [`Loadout`] adds up to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShipStats {
    pub mass_t: f64,
    pub length_m: f64,
    pub structure: f64,
    pub thrust_kn: f64,
    pub rcs_torque_knm: f64,
    pub power_supply_mw: f64,
    pub heat_capacity_mj: f64,
    pub heat_dissipation_mw: f64,
    /// Everything running, guns firing.
    pub combat: Load,
    /// Everything running except the weapons.
    pub cruise: Load,
    pub dps: f64,
    pub weapon_range_km: f64,
    pub shield: f64,
    pub shield_regen_per_s: f64,
    pub radar_range_km: f64,
}

impl ShipStats {
    /// Acceleration at full thrust, in m/s².
    pub fn acceleration(&self) -> f64 {
        if self.mass_t > 0.0 {
            self.thrust_kn / self.mass_t
        } else {
            0.0
        }
    }

    /// Angular acceleration at full RCS, in rad/s². The hull is treated
    /// as a uniform rod about its middle: I = m L² / 12.
    pub fn angular_acceleration(&self) -> f64 {
        let inertia = self.mass_t * self.length_m * self.length_m / 12.0;
        if inertia > 0.0 {
            self.rcs_torque_knm / inertia
        } else {
            0.0
        }
    }

    /// Seconds to turn through `degrees` from rest to rest: full RCS one
    /// way to spin up, then the other way to stop. `None` with no RCS.
    pub fn time_to_turn(&self, degrees: f64) -> Option<f64> {
        let alpha = self.angular_acceleration();
        (alpha > 0.0).then(|| 2.0 * (degrees.to_radians() / alpha).sqrt())
    }

    /// Power left over under `load`; negative means the reactor can't keep
    /// up and something has to be throttled.
    pub fn power_margin(&self, load: Load) -> f64 {
        self.power_supply_mw - load.power_mw
    }

    /// Seconds from cold until the hull's heat capacity fills under
    /// `load`, or `None` if the radiators keep up indefinitely.
    pub fn time_to_overheat(&self, load: Load) -> Option<f64> {
        let surplus = load.heat_mw - self.heat_dissipation_mw;
        (surplus > 0.0).then(|| self.heat_capacity_mj / surplus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn module(id: &'static str, size: Size, function: Function) -> Module {
        Module {
            id,
            name: id,
            size,
            mass_t: 1.0,
            power_mw: 1.0,
            heat_mw: 2.0,
            function,
        }
    }

    const GUN: Module = module(
        "gun",
        Size::Small,
        Function::Weapon {
            damage: 10.0,
            fire_rate_hz: 2.0,
            range_km: 3.0,
        },
    );
    const BIG_GUN: Module = module(
        "big-gun",
        Size::Medium,
        Function::Weapon {
            damage: 40.0,
            fire_rate_hz: 1.0,
            range_km: 5.0,
        },
    );
    const ENGINE: Module = module("engine", Size::Small, Function::Engine { thrust_kn: 100.0 });

    const HULL: Hull = Hull {
        id: "test",
        name: "test",
        size: Size::Small,
        mass_t: 8.0,
        length_m: 6.0,
        structure: 100.0,
        heat_capacity_mj: 30.0,
        base_dissipation_mw: 1.0,
        slots: &[
            Slot {
                kind: SlotKind::Weapon,
                size: Size::Medium,
                position_m: [0.0; 3],
            },
            Slot {
                kind: SlotKind::Weapon,
                size: Size::Small,
                position_m: [0.0; 3],
            },
            Slot {
                kind: SlotKind::Engine,
                size: Size::Small,
                position_m: [0.0; 3],
            },
        ],
    };

    #[test]
    fn smaller_modules_fit_bigger_slots() {
        let mut fit = Loadout::new(&HULL);
        assert_eq!(fit.fit(0, &GUN), Ok(None));
        assert_eq!(fit.fit(0, &BIG_GUN), Ok(Some(&GUN)));
    }

    #[test]
    fn rejects_oversized_and_wrong_kind_and_missing_slots() {
        let mut fit = Loadout::new(&HULL);
        assert_eq!(
            fit.fit(1, &BIG_GUN),
            Err(FitError::TooBig {
                slot: Size::Small,
                module: Size::Medium
            })
        );
        assert_eq!(
            fit.fit(2, &GUN),
            Err(FitError::WrongKind {
                slot: SlotKind::Engine,
                module: SlotKind::Weapon
            })
        );
        assert_eq!(
            fit.fit(3, &GUN),
            Err(FitError::NoSuchSlot { slot: 3, slots: 3 })
        );
        assert!(fit.slots().all(|(_, m)| m.is_none()));
    }

    #[test]
    fn stats_add_up() {
        let mut fit = Loadout::new(&HULL);
        fit.fit(0, &GUN).unwrap();
        fit.fit(1, &GUN).unwrap();
        fit.fit(2, &ENGINE).unwrap();
        let stats = fit.stats();
        assert_eq!(stats.mass_t, 11.0);
        assert_eq!(stats.dps, 40.0);
        assert_eq!(stats.acceleration(), 100.0 / 11.0);
        assert_eq!(
            stats.combat,
            Load {
                power_mw: 3.0,
                heat_mw: 6.0
            }
        );
        assert_eq!(
            stats.cruise,
            Load {
                power_mw: 1.0,
                heat_mw: 2.0
            }
        );
        assert_eq!(stats.power_margin(stats.combat), -3.0);
        // 6 MW in, 1 MW out: 30 MJ fills in 6 s.
        assert_eq!(stats.time_to_overheat(stats.combat), Some(6.0));
        assert_eq!(stats.time_to_overheat(Load::default()), None);

        // No RCS fitted: the ship can't turn.
        assert_eq!(stats.time_to_turn(180.0), None);

        assert_eq!(fit.remove(2), Some(&ENGINE));
        assert_eq!(fit.stats().thrust_kn, 0.0);
    }

    #[test]
    fn turning_slows_with_mass_and_length() {
        let stats = ShipStats {
            mass_t: 12.0,
            length_m: 6.0,
            rcs_torque_knm: 36.0,
            ..ShipStats::default()
        };
        // I = 12 * 36 / 12 = 36, so α = 1 rad/s²; π rad takes 2√π s.
        assert_eq!(stats.angular_acceleration(), 1.0);
        let flip = stats.time_to_turn(180.0).unwrap();
        assert!((flip - 2.0 * std::f64::consts::PI.sqrt()).abs() < 1e-9);

        let longer = ShipStats {
            length_m: 12.0,
            ..stats
        };
        assert_eq!(longer.angular_acceleration(), 0.25);
    }
}
