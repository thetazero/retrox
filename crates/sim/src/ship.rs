//! Ship fitting: hulls built from sections, the slots in each section, the
//! modules that go in them, and the stats a fitted ship ends up with.
//!
//! See `docs/DESIGN.md` §7.4.
//!
//! - A [`Hull`] is a frame with [`Mount`]s. Small ships have a single
//!   fixed [`Section`]. Medium and large ships have bow, mid and stern
//!   mounts, and any section of the right [`Position`] and exactly the
//!   right [`Size`] can go in one — a gun deck or a cargo bay amidships, a
//!   sprint stern or an efficient one aft.
//! - Each section carries [`Slot`]s. A slot takes one [`Module`] of its
//!   [`SlotKind`] at its size or smaller.
//!
//! Two budgets tie a fit together:
//!
//! - **Power:** reactors produce it; weapons, shields, radar, engines and
//!   RCS thrusters draw it while active.
//! - **Heat:** every active module makes heat; sections shed some on their
//!   own and radiators shed more. Surplus heat fills the sections' heat
//!   capacity, and when that's full the ship is overheating.
//!
//! A fit is allowed to be over budget — an interceptor that can't run
//! everything at once is a choice the pilot manages in flight, so
//! [`ShipStats`] reports both budgets rather than rejecting the fit.
//!
//! Turning comes from RCS (reaction control system) thrusters: their
//! torque against the ship's moment of inertia, which grows with mass and
//! with the square of its length. Heavy or long ships turn slowly.
//!
//! Units: tonnes, kilonewtons (so kN / t = m/s²), kilonewton-metres,
//! megawatts, megajoules, metres, kilometres.

use std::fmt;

/// Slot, module, section and hull size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
    Small,
    Medium,
    Large,
}

/// What a ship is built for. Descriptive: nothing stops a hauler from
/// carrying guns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShipRole {
    Combat,
    Trade,
    Speed,
}

/// Where along the ship a section sits. Small ships are one `Whole`
/// section; bigger ships stack bow, mids and stern, nose to tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Position {
    Whole,
    Bow,
    Mid,
    Stern,
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
    Cargo,
    /// The hyperdrive, for jumping along hyperlanes.
    JumpDrive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub size: Size,
    /// Mount point relative to the section's origin, in metres: +x right,
    /// +y up, +z forward. Used to place the module's visual and, for
    /// weapons, where shots leave the ship.
    pub position_m: [f64; 3],
}

/// One piece of a ship: a structural unit with its own slots, and its own
/// damage location.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub id: &'static str,
    pub name: &'static str,
    pub position: Position,
    pub size: Size,
    pub mass_t: f64,
    /// Nose to tail. Sections stack, so a ship's length is their sum.
    pub length_m: f64,
    pub structure: f64,
    /// Heat the section can soak before the ship overheats.
    pub heat_capacity_mj: f64,
    /// Heat the bare section sheds without any radiators.
    pub base_dissipation_mw: f64,
    pub slots: &'static [Slot],
}

/// A place on a hull's frame that holds a section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mount {
    /// Built in; can't be swapped. Small ships are one of these.
    Fixed(&'static Section),
    /// Takes any section of this position and exactly this size.
    Open { position: Position, size: Size },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    pub id: &'static str,
    pub name: &'static str,
    pub role: ShipRole,
    pub size: Size,
    /// The frame tying the sections together; zero for small ships, whose
    /// single section is the whole ship.
    pub frame_mass_t: f64,
    pub frame_structure: f64,
    /// Mounts from nose to tail.
    pub mounts: &'static [Mount],
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
    Cargo {
        capacity_t: f64,
    },
    /// A hyperdrive. Its power and heat are drawn only while jumping, so
    /// they don't count toward the combat or cruise loads.
    JumpDrive {
        /// Light-years per day along a hyperlane, at or below rated mass.
        speed_ly_per_day: f64,
        /// Ships heavier than this jump proportionally slower.
        rated_mass_t: f64,
        /// Which hyperlane tiers the drive can use (DESIGN §4.1).
        tier: u8,
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
            Self::Cargo { .. } => SlotKind::Cargo,
            Self::JumpDrive { .. } => SlotKind::JumpDrive,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FitError {
    NoSuchMount { mount: usize, mounts: usize },
    FixedMount { mount: usize },
    EmptyMount { mount: usize },
    WrongPosition { mount: Position, section: Position },
    SectionSize { mount: Size, section: Size },
    NoSuchSlot { slot: usize, slots: usize },
    WrongKind { slot: SlotKind, module: SlotKind },
    TooBig { slot: Size, module: Size },
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchMount { mount, mounts } => {
                write!(f, "mount {mount} doesn't exist (hull has {mounts})")
            }
            Self::FixedMount { mount } => write!(f, "mount {mount}'s section is built in"),
            Self::EmptyMount { mount } => write!(f, "mount {mount} has no section"),
            Self::WrongPosition { mount, section } => {
                write!(f, "a {section:?} section can't go in a {mount:?} mount")
            }
            Self::SectionSize { mount, section } => {
                write!(f, "a {section:?} section doesn't fit a {mount:?} mount")
            }
            Self::NoSuchSlot { slot, slots } => {
                write!(f, "slot {slot} doesn't exist (section has {slots})")
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

/// A section in place on a hull, and what's fitted in its slots.
#[derive(Clone, Debug, PartialEq)]
pub struct FittedSection {
    pub section: &'static Section,
    modules: Vec<Option<&'static Module>>,
}

impl FittedSection {
    fn new(section: &'static Section) -> Self {
        Self {
            section,
            modules: vec![None; section.slots.len()],
        }
    }

    /// Each slot alongside whatever is fitted in it.
    pub fn slots(&self) -> impl Iterator<Item = (&'static Slot, Option<&'static Module>)> + '_ {
        self.section.slots.iter().zip(self.modules.iter().copied())
    }
}

/// A hull, the section in each mount, and the module in each slot.
///
/// Refitting — modules or sections — is only allowed while docked
/// (`docs/DESIGN.md` §7.4). Ships don't exist in the sim state yet, so
/// callers enforce that until they do.
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout {
    hull: &'static Hull,
    sections: Vec<Option<FittedSection>>,
}

impl Loadout {
    /// A hull with its fixed sections in place, open mounts empty and
    /// every slot empty.
    pub fn new(hull: &'static Hull) -> Self {
        let sections = hull
            .mounts
            .iter()
            .map(|mount| match mount {
                Mount::Fixed(section) => Some(FittedSection::new(section)),
                Mount::Open { .. } => None,
            })
            .collect();
        Self { hull, sections }
    }

    pub fn hull(&self) -> &'static Hull {
        self.hull
    }

    /// Each mount alongside its section, if it has one.
    pub fn mounts(&self) -> impl Iterator<Item = (&'static Mount, Option<&FittedSection>)> + '_ {
        self.hull
            .mounts
            .iter()
            .zip(self.sections.iter().map(Option::as_ref))
    }

    fn mount(&self, mount: usize) -> Result<&'static Mount, FitError> {
        self.hull.mounts.get(mount).ok_or(FitError::NoSuchMount {
            mount,
            mounts: self.hull.mounts.len(),
        })
    }

    /// Whether `section` could go in `mount`, without placing it.
    pub fn check_section(&self, mount: usize, section: &Section) -> Result<(), FitError> {
        match *self.mount(mount)? {
            Mount::Fixed(_) => Err(FitError::FixedMount { mount }),
            Mount::Open { position, size } => {
                if section.position != position {
                    Err(FitError::WrongPosition {
                        mount: position,
                        section: section.position,
                    })
                } else if section.size != size {
                    Err(FitError::SectionSize {
                        mount: size,
                        section: section.size,
                    })
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Put `section` in `mount`. Whatever section was there comes out with
    /// its modules; those modules are returned.
    pub fn set_section(
        &mut self,
        mount: usize,
        section: &'static Section,
    ) -> Result<Vec<&'static Module>, FitError> {
        self.check_section(mount, section)?;
        let old = self.sections[mount].replace(FittedSection::new(section));
        Ok(old.into_iter().flat_map(|s| s.modules).flatten().collect())
    }

    /// Whether `module` could go in `slot` of the section in `mount`,
    /// without fitting it.
    pub fn check(&self, mount: usize, slot: usize, module: &Module) -> Result<(), FitError> {
        self.mount(mount)?;
        let section = self.sections[mount]
            .as_ref()
            .ok_or(FitError::EmptyMount { mount })?
            .section;
        let s = section.slots.get(slot).ok_or(FitError::NoSuchSlot {
            slot,
            slots: section.slots.len(),
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

    /// Fit `module` into `slot` of the section in `mount`, returning
    /// whatever was there before.
    pub fn fit(
        &mut self,
        mount: usize,
        slot: usize,
        module: &'static Module,
    ) -> Result<Option<&'static Module>, FitError> {
        self.check(mount, slot, module)?;
        let section = self.sections[mount].as_mut().expect("checked above");
        Ok(section.modules[slot].replace(module))
    }

    /// Empty `slot` of the section in `mount`, returning whatever was there.
    pub fn remove(&mut self, mount: usize, slot: usize) -> Option<&'static Module> {
        let section = self.sections.get_mut(mount)?.as_mut()?;
        section.modules.get_mut(slot)?.take()
    }

    pub fn stats(&self) -> ShipStats {
        let mut stats = ShipStats {
            mass_t: self.hull.frame_mass_t,
            structure: self.hull.frame_structure,
            ..ShipStats::default()
        };
        for fitted in self.sections.iter().flatten() {
            let section = fitted.section;
            stats.mass_t += section.mass_t;
            stats.length_m += section.length_m;
            stats.structure += section.structure;
            stats.heat_capacity_mj += section.heat_capacity_mj;
            stats.heat_dissipation_mw += section.base_dissipation_mw;
            for module in fitted.modules.iter().flatten() {
                stats.add(module);
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

/// What a [`Loadout`] adds up to. Mass is empty; see [`ShipStats::laden`].
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
    pub cargo_t: f64,
    /// The jump drive's rated speed, light-years per day; 0 without one.
    pub jump_speed_ly_per_day: f64,
    pub jump_rated_mass_t: f64,
    /// Highest hyperlane tier the ship can use; 0 without a drive.
    pub drive_tier: u8,
}

impl ShipStats {
    fn add(&mut self, module: &Module) {
        self.mass_t += module.mass_t;
        if let Function::JumpDrive {
            speed_ly_per_day,
            rated_mass_t,
            tier,
        } = module.function
        {
            // Drawn only while jumping; see `Function::JumpDrive`.
            if speed_ly_per_day > self.jump_speed_ly_per_day {
                self.jump_speed_ly_per_day = speed_ly_per_day;
                self.jump_rated_mass_t = rated_mass_t;
            }
            self.drive_tier = self.drive_tier.max(tier);
            return;
        }
        self.combat.power_mw += module.power_mw;
        self.combat.heat_mw += module.heat_mw;
        if module.kind() != SlotKind::Weapon {
            self.cruise.power_mw += module.power_mw;
            self.cruise.heat_mw += module.heat_mw;
        }
        match module.function {
            Function::Weapon {
                damage,
                fire_rate_hz,
                range_km,
            } => {
                self.dps += damage * fire_rate_hz;
                self.weapon_range_km = self.weapon_range_km.max(range_km);
            }
            Function::Engine { thrust_kn } => self.thrust_kn += thrust_kn,
            Function::Rcs { torque_knm } => self.rcs_torque_knm += torque_knm,
            Function::Power { output_mw } => self.power_supply_mw += output_mw,
            Function::Shield {
                capacity,
                regen_per_s,
            } => {
                self.shield += capacity;
                self.shield_regen_per_s += regen_per_s;
            }
            Function::Radar { range_km } => {
                self.radar_range_km = self.radar_range_km.max(range_km);
            }
            Function::Radiator { dissipation_mw } => self.heat_dissipation_mw += dissipation_mw,
            Function::Cargo { capacity_t } => self.cargo_t += capacity_t,
            Function::JumpDrive { .. } => unreachable!("handled above"),
        }
    }

    /// Jump speed along a hyperlane at this mass, light-years per day:
    /// the drive's rated speed, slowed in proportion when the ship is
    /// heavier than the drive is rated for. Use [`ShipStats::laden`] for a
    /// full hold.
    pub fn jump_speed(&self) -> f64 {
        if self.mass_t <= 0.0 {
            return 0.0;
        }
        self.jump_speed_ly_per_day * (self.jump_rated_mass_t / self.mass_t).min(1.0)
    }

    /// The same ship with a full hold.
    pub fn laden(&self) -> Self {
        Self {
            mass_t: self.mass_t + self.cargo_t,
            ..*self
        }
    }

    /// Acceleration at full thrust, in m/s².
    pub fn acceleration(&self) -> f64 {
        if self.mass_t > 0.0 {
            self.thrust_kn / self.mass_t
        } else {
            0.0
        }
    }

    /// Angular acceleration at full RCS, in rad/s². The ship is treated
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

    /// Seconds from cold until the ship's heat capacity fills under
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

    const fn slot(kind: SlotKind, size: Size) -> Slot {
        Slot {
            kind,
            size,
            position_m: [0.0; 3],
        }
    }

    const fn section(
        id: &'static str,
        position: Position,
        size: Size,
        slots: &'static [Slot],
    ) -> Section {
        Section {
            id,
            name: id,
            position,
            size,
            mass_t: 8.0,
            length_m: 6.0,
            structure: 100.0,
            heat_capacity_mj: 30.0,
            base_dissipation_mw: 1.0,
            slots,
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
    const HOLD: Module = module("hold", Size::Medium, Function::Cargo { capacity_t: 50.0 });

    const BODY: Section = section(
        "body",
        Position::Whole,
        Size::Small,
        &[
            slot(SlotKind::Weapon, Size::Medium),
            slot(SlotKind::Weapon, Size::Small),
            slot(SlotKind::Engine, Size::Small),
        ],
    );
    const SMALL: Hull = Hull {
        id: "small",
        name: "small",
        role: ShipRole::Combat,
        size: Size::Small,
        frame_mass_t: 0.0,
        frame_structure: 0.0,
        mounts: &[Mount::Fixed(&BODY)],
    };

    const GUN_DECK: Section = section(
        "gun-deck",
        Position::Mid,
        Size::Medium,
        &[slot(SlotKind::Weapon, Size::Medium)],
    );
    const CARGO_BAY: Section = section(
        "cargo-bay",
        Position::Mid,
        Size::Medium,
        &[slot(SlotKind::Cargo, Size::Medium)],
    );
    const SMALL_BAY: Section = section(
        "small-bay",
        Position::Mid,
        Size::Small,
        &[slot(SlotKind::Cargo, Size::Small)],
    );
    const STERN: Section = section(
        "stern",
        Position::Stern,
        Size::Medium,
        &[slot(SlotKind::Engine, Size::Medium)],
    );
    const MEDIUM: Hull = Hull {
        id: "medium",
        name: "medium",
        role: ShipRole::Trade,
        size: Size::Medium,
        frame_mass_t: 5.0,
        frame_structure: 50.0,
        mounts: &[
            Mount::Open {
                position: Position::Mid,
                size: Size::Medium,
            },
            Mount::Open {
                position: Position::Stern,
                size: Size::Medium,
            },
        ],
    };

    #[test]
    fn small_ships_come_with_their_section() {
        let fit = Loadout::new(&SMALL);
        assert!(fit.mounts().all(|(_, s)| s.is_some()));
        assert_eq!(
            Loadout::new(&SMALL).set_section(0, &BODY),
            Err(FitError::FixedMount { mount: 0 })
        );
    }

    #[test]
    fn smaller_modules_fit_bigger_slots() {
        let mut fit = Loadout::new(&SMALL);
        assert_eq!(fit.fit(0, 0, &GUN), Ok(None));
        assert_eq!(fit.fit(0, 0, &BIG_GUN), Ok(Some(&GUN)));
    }

    #[test]
    fn rejects_oversized_and_wrong_kind_and_missing_slots() {
        let mut fit = Loadout::new(&SMALL);
        assert_eq!(
            fit.fit(0, 1, &BIG_GUN),
            Err(FitError::TooBig {
                slot: Size::Small,
                module: Size::Medium
            })
        );
        assert_eq!(
            fit.fit(0, 2, &GUN),
            Err(FitError::WrongKind {
                slot: SlotKind::Engine,
                module: SlotKind::Weapon
            })
        );
        assert_eq!(
            fit.fit(0, 3, &GUN),
            Err(FitError::NoSuchSlot { slot: 3, slots: 3 })
        );
        assert_eq!(
            fit.fit(1, 0, &GUN),
            Err(FitError::NoSuchMount {
                mount: 1,
                mounts: 1
            })
        );
    }

    #[test]
    fn sections_need_matching_position_and_exact_size() {
        let mut fit = Loadout::new(&MEDIUM);
        assert_eq!(fit.fit(0, 0, &GUN), Err(FitError::EmptyMount { mount: 0 }));
        assert_eq!(
            fit.set_section(0, &STERN),
            Err(FitError::WrongPosition {
                mount: Position::Mid,
                section: Position::Stern
            })
        );
        assert_eq!(
            fit.set_section(0, &SMALL_BAY),
            Err(FitError::SectionSize {
                mount: Size::Medium,
                section: Size::Small
            })
        );
        assert_eq!(fit.set_section(0, &GUN_DECK), Ok(vec![]));
        assert_eq!(fit.set_section(1, &STERN), Ok(vec![]));
    }

    #[test]
    fn swapping_a_section_unfits_its_modules() {
        let mut fit = Loadout::new(&MEDIUM);
        fit.set_section(0, &GUN_DECK).unwrap();
        fit.fit(0, 0, &BIG_GUN).unwrap();
        assert_eq!(fit.set_section(0, &CARGO_BAY), Ok(vec![&BIG_GUN]));
        fit.fit(0, 0, &HOLD).unwrap();
        let (_, mid) = fit.mounts().next().unwrap();
        assert_eq!(mid.unwrap().slots().next().unwrap().1, Some(&HOLD));
    }

    #[test]
    fn stats_add_up() {
        let mut fit = Loadout::new(&SMALL);
        fit.fit(0, 0, &GUN).unwrap();
        fit.fit(0, 1, &GUN).unwrap();
        fit.fit(0, 2, &ENGINE).unwrap();
        let stats = fit.stats();
        assert_eq!(stats.mass_t, 11.0);
        assert_eq!(stats.length_m, 6.0);
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

        assert_eq!(fit.remove(0, 2), Some(&ENGINE));
        assert_eq!(fit.stats().thrust_kn, 0.0);
    }

    #[test]
    fn sections_and_frame_add_up() {
        let mut fit = Loadout::new(&MEDIUM);
        fit.set_section(0, &CARGO_BAY).unwrap();
        fit.set_section(1, &STERN).unwrap();
        fit.fit(0, 0, &HOLD).unwrap();
        let stats = fit.stats();
        assert_eq!(stats.mass_t, 5.0 + 8.0 + 8.0 + 1.0);
        assert_eq!(stats.length_m, 12.0);
        assert_eq!(stats.structure, 250.0);
        assert_eq!(stats.heat_capacity_mj, 60.0);
        assert_eq!(stats.cargo_t, 50.0);
        assert_eq!(stats.laden().mass_t, 72.0);
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
