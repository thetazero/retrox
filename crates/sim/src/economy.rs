//! The coarse economy (§5.4): goods, recipes, facilities, markets.
//!
//! Every settlement has a [`LocalEconomy`]: a population that consumes, a
//! set of facilities that turn inputs into outputs, and a market holding
//! stock. Prices come from stock relative to a target. Factions with a
//! [`EconomyModel::Planned`] economy hold prices at their official base
//! value and move goods by plan instead of by price.
//!
//! Goods only move between settlements in ships; see [`crate::haul`].

use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};

use crate::{EconomyModel, GalaxyState};

/// Tradeable goods. A small starting set; the full 15–30 goods will come
/// from content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Good {
    Ore,
    Ice,
    Metal,
    Food,
    Fuel,
    Components,
    /// Skimmed from gas giants and refined into fuel (issue #68).
    Gas,
}

impl Good {
    pub const COUNT: usize = 7;
    pub const ALL: [Good; Self::COUNT] = [
        Good::Ore,
        Good::Ice,
        Good::Metal,
        Good::Food,
        Good::Fuel,
        Good::Components,
        Good::Gas,
    ];

    /// The reference price, in credits per unit. Planned economies sell at
    /// this price; markets float around it.
    pub const fn base_price(self) -> f64 {
        match self {
            Good::Ore => 10.0,
            Good::Ice => 6.0,
            Good::Metal => 30.0,
            Good::Food => 15.0,
            Good::Fuel => 20.0,
            Good::Components => 80.0,
            Good::Gas => 10.0,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Good::Ore => "ore",
            Good::Ice => "ice",
            Good::Metal => "metal",
            Good::Food => "food",
            Good::Fuel => "fuel",
            Good::Components => "components",
            Good::Gas => "gas",
        }
    }
}

/// One value per [`Good`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GoodMap<T>(pub [T; Good::COUNT]);

impl<T: Copy> GoodMap<T> {
    pub const fn splat(value: T) -> Self {
        Self([value; Good::COUNT])
    }

    pub fn from_fn(f: impl FnMut(Good) -> T) -> Self {
        Self(Good::ALL.map(f))
    }
}

impl<T> Index<Good> for GoodMap<T> {
    type Output = T;

    fn index(&self, good: Good) -> &T {
        &self.0[good as usize]
    }
}

impl<T> IndexMut<Good> for GoodMap<T> {
    fn index_mut(&mut self, good: Good) -> &mut T {
        &mut self.0[good as usize]
    }
}

/// What a facility does: per unit of scale per day, consume `inputs` and
/// produce `outputs`.
pub struct Recipe {
    pub inputs: &'static [(Good, f64)],
    pub outputs: &'static [(Good, f64)],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FacilityKind {
    Mine,
    IceHarvester,
    Refinery,
    Hydroponics,
    FuelPlant,
    Factory,
    /// Builds ship hulls: its output goes to [`LocalEconomy::hull_t`], not
    /// the market.
    Shipyard,
    /// Refines skimmed gas into fuel: cheaper to run than cracking ice in a
    /// [`FacilityKind::FuelPlant`], but its gas comes from harvesters.
    GasPlant,
}

impl FacilityKind {
    pub const ALL: [FacilityKind; 8] = [
        FacilityKind::Mine,
        FacilityKind::IceHarvester,
        FacilityKind::Refinery,
        FacilityKind::Hydroponics,
        FacilityKind::FuelPlant,
        FacilityKind::Factory,
        FacilityKind::Shipyard,
        FacilityKind::GasPlant,
    ];

    pub const fn recipe(self) -> &'static Recipe {
        use Good::*;
        match self {
            FacilityKind::Mine => &Recipe {
                inputs: &[],
                outputs: &[(Ore, 1.0)],
            },
            FacilityKind::IceHarvester => &Recipe {
                inputs: &[],
                outputs: &[(Ice, 1.0)],
            },
            FacilityKind::Refinery => &Recipe {
                inputs: &[(Ore, 2.0), (Fuel, 0.2)],
                outputs: &[(Metal, 1.0)],
            },
            FacilityKind::Hydroponics => &Recipe {
                inputs: &[(Ice, 0.5)],
                outputs: &[(Food, 1.0)],
            },
            FacilityKind::FuelPlant => &Recipe {
                inputs: &[(Ice, 1.0)],
                outputs: &[(Fuel, 1.0)],
            },
            FacilityKind::Factory => &Recipe {
                inputs: &[(Metal, 1.0), (Fuel, 0.5)],
                outputs: &[(Components, 1.0)],
            },
            FacilityKind::GasPlant => &Recipe {
                inputs: &[(Gas, 1.0)],
                outputs: &[(Fuel, 1.4)],
            },
            // One tonne of hull per run; see `HULL_T_PER_RUN`.
            FacilityKind::Shipyard => &Recipe {
                inputs: &[(Metal, 1.0), (Components, 0.3), (Fuel, 0.1)],
                outputs: &[],
            },
        }
    }
}

/// A recipe laid out for the daily books: per-good rows instead of lists,
/// so a facility's day is a few passes over seven numbers.
struct DenseRecipe {
    inputs: [f64; Good::COUNT],
    outputs: [f64; Good::COUNT],
    /// Bit `g` set if good `g` is an input, or an output.
    input_mask: u8,
    output_mask: u8,
}

const fn dense(kind: FacilityKind) -> DenseRecipe {
    let recipe = kind.recipe();
    let mut out = DenseRecipe {
        inputs: [0.0; Good::COUNT],
        outputs: [0.0; Good::COUNT],
        input_mask: 0,
        output_mask: 0,
    };
    let mut i = 0;
    while i < recipe.inputs.len() {
        let (good, amount) = recipe.inputs[i];
        out.inputs[good as usize] = amount;
        out.input_mask |= 1 << good as u8;
        i += 1;
    }
    let mut i = 0;
    while i < recipe.outputs.len() {
        let (good, amount) = recipe.outputs[i];
        out.outputs[good as usize] = amount;
        out.output_mask |= 1 << good as u8;
        i += 1;
    }
    out
}

/// Every kind's [`DenseRecipe`], indexed by kind.
static DENSE: [DenseRecipe; FacilityKind::ALL.len()] = {
    let mut i = 0;
    let mut out = [const {
        DenseRecipe {
            inputs: [0.0; Good::COUNT],
            outputs: [0.0; Good::COUNT],
            input_mask: 0,
            output_mask: 0,
        }
    }; FacilityKind::ALL.len()];
    while i < FacilityKind::ALL.len() {
        out[FacilityKind::ALL[i] as usize] = dense(FacilityKind::ALL[i]);
        i += 1;
    }
    out
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Facility {
    pub kind: FacilityKind,
    /// Units of the recipe run per day at full capacity.
    pub scale: f64,
    /// How much of capacity it has been running at, averaged over about a
    /// season. Long-idle facilities decay.
    pub utilization: f64,
}

impl Facility {
    pub const fn new(kind: FacilityKind, scale: f64) -> Self {
        Self {
            kind,
            scale,
            utilization: 1.0,
        }
    }
}

/// Days over which a facility's utilization is averaged.
const UTILIZATION_DAYS: f64 = 90.0;

/// Daily consumption per unit of population.
pub const PER_CAPITA_DEMAND: [(Good, f64); 3] = [
    (Good::Food, 1.0),
    (Good::Fuel, 0.3),
    (Good::Components, 0.05),
];

/// Tonnes of hull a shipyard builds per run of its recipe.
pub const HULL_T_PER_RUN: f64 = 1.0;
/// Shipyards keep this many days of their own output in stock, plus
/// whatever hull captains have asked for and couldn't get
/// ([`LocalEconomy::hull_wanted`]): beyond that, nobody is buying.
pub const HULL_STORAGE_DAYS: f64 = 30.0;
/// What a tonne of finished hull sells for, in credits: about twice what
/// goes into it (a tonne of metal, 0.3 t of components, 0.1 t of fuel, 56
/// cr at base prices), the same mark-up a factory puts on components. It
/// prices every ship and part (DESIGN §7.8).
pub const HULL_PRICE_PER_T: f64 = 120.0;

/// A facility under construction: what it will be, and the materials it
/// still needs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub kind: FacilityKind,
    /// Scale added when finished.
    pub scale: f64,
    pub needs: GoodMap<f64>,
    /// When it was started. A project that can't get its materials is
    /// eventually cut short; see [`crate::construction`].
    #[serde(default)]
    pub started: crate::GalaxyTime,
}

/// Daily consumption of `good` per unit of population.
pub fn per_capita(good: Good) -> f64 {
    PER_CAPITA_DEMAND
        .iter()
        .find(|(g, _)| *g == good)
        .map_or(0.0, |&(_, amount)| amount)
}

/// Markets aim to hold this many days of their own demand.
pub const TARGET_DAYS: f64 = 30.0;
/// Facilities stop producing a good once stock reaches this multiple of target.
pub const STORAGE_MULTIPLE: f64 = 4.0;
/// Every market keeps at least this much target stock of everything, so
/// pure producers still price their output sensibly.
pub const MIN_TARGET: f64 = 20.0;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Market {
    pub stock: GoodMap<f64>,
    pub price: GoodMap<f64>,
    /// Daily demand at full rate: population plus facility inputs.
    pub demand: GoodMap<f64>,
    /// How much demand went unmet in the last production phase.
    pub shortfall: GoodMap<f64>,
}

impl Market {
    pub fn target(&self, good: Good) -> f64 {
        (self.demand[good] * TARGET_DAYS).max(MIN_TARGET)
    }

    /// Stock relative to target: 1.0 is exactly on target.
    pub fn fill(&self, good: Good) -> f64 {
        self.stock[good] / self.target(good)
    }

    /// What this market would charge for `good` if it held `stock` of it.
    /// Planned economies always charge the official base price.
    pub fn price_at(&self, good: Good, stock: f64, model: EconomyModel) -> f64 {
        match model {
            EconomyModel::Planned => good.base_price(),
            EconomyModel::Market => {
                let scarcity = self.target(good) / (stock.max(0.0) + 1.0);
                good.base_price() * price_curve(scarcity)
            }
        }
    }

    /// Stock above target: what the market is willing to sell.
    pub fn surplus(&self, good: Good) -> f64 {
        (self.stock[good] - self.target(good)).max(0.0)
    }
}

/// How far a market price strays from base with scarcity (target stock
/// over stock held): `scarcity^0.6`, held between a quarter and five times
/// base.
///
/// Every market prices every good every day, so this is read from tables
/// instead of calling `powf` (DESIGN §5.1.1): the power of two in
/// `scarcity` from one, the rest from the other, interpolated. Within
/// 1e-5 of `powf`, and the tables are written out rather than computed, so
/// the result is the same on every platform (§5.1).
pub fn price_curve(scarcity: f64) -> f64 {
    // scarcity^0.6 is 0.25 and 5 at these.
    const LOW: f64 = 0.09921256574801246;
    const HIGH: f64 = 14.620088691064332;
    const MANTISSA_BITS: u32 = 52;
    const STEPS_BITS: u32 = 6;
    // `max` first, so NaN reads as plenty.
    let x = scarcity.max(LOW).min(HIGH);
    let bits = x.to_bits();
    // x is 2^e · m with m in [1, 2), and e in -4..=3 between the clamps.
    let e = ((bits >> MANTISSA_BITS) & 0x7ff) as usize + 4 - 1023;
    let fraction = bits & ((1 << MANTISSA_BITS) - 1);
    let step = (fraction >> (MANTISSA_BITS - STEPS_BITS)) as usize;
    let within = (fraction & ((1 << (MANTISSA_BITS - STEPS_BITS)) - 1)) as f64
        / (1u64 << (MANTISSA_BITS - STEPS_BITS)) as f64;
    let m = POW_MANTISSA[step] + (POW_MANTISSA[step + 1] - POW_MANTISSA[step]) * within;
    (POW_EXP[e] * m).clamp(0.25, 5.0)
}

/// 2^(0.6 e) for e in -4..=3.
#[rustfmt::skip]
const POW_EXP: [f64; 8] = [
    0.18946457081379978, 0.2871745887492588, 0.43527528164806206, 0.6597539553864471,
    1.0, 1.515716566510398, 2.2973967099940698, 3.482202253184496,
];

/// m^0.6 for m = 1, 1 + 1/64, … 2.
#[rustfmt::skip]
const POW_MANTISSA: [f64; 65] = [
    1.0, 1.0093459147663688, 1.018634490106604, 1.0278669387264774,
    1.0370444300164117, 1.0461680922111765, 1.055239014412241, 1.06425824848335,
    1.0732268108289416, 1.0821456840641672, 1.0910158185845167, 1.0998381340423509,
    1.1086135207370322, 1.1173428409247734, 1.1260269300538224, 1.1346665979301414,
    1.143262629818316, 1.151815787482061, 1.1603268101683355, 1.1687964155387764,
    1.1772253005518665, 1.1856141422989992, 1.193963598797363, 1.2022743097423483,
    1.2105468972219857, 1.2187819663957407, 1.22698010613982, 1.2351418896609991,
    1.2432678750808317, 1.251358605991976, 1.259414611988256, 1.2674364091699617,
    1.2754245006257907, 1.2833793768927504, 1.291301516395233, 1.2991913858644224,
    1.3070494407390916, 1.314876125548802, 1.3226718742804395, 1.3304371107289703,
    1.338172248833238, 1.3458776929975842, 1.3535538384000132, 1.3612010712875904,
    1.368819769259713, 1.3764103015398634, 1.38397302923641, 1.3915083055929955,
    1.399016476229016, 1.406497879370671, 1.4139528460730293, 1.421381700433541,
    1.4287847597973933, 1.4361623349550887, 1.4435147303326075, 1.4508422441744886,
    1.4581451687201539, 1.465423790373777, 1.4726783898679854, 1.4799092424216662,
    1.4871166178921387, 1.4943007809219329, 1.5014619910804108, 1.50860050300045,
    1.515716566510398,
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalEconomy {
    /// Abstract population units (roughly: thousands of people).
    pub population: f64,
    pub facilities: Vec<Facility>,
    pub market: Market,
    /// Share of food demand going unmet, averaged over about a month; see
    /// [`crate::demography`].
    pub hunger: f64,
    /// Whether the settlement is in famine.
    pub famine: bool,
    /// The facility being built here, if any.
    pub project: Option<Project>,
    /// Finished ship hull ready for sale, in tonnes. Built by shipyards.
    pub hull_t: f64,
    /// Hull captains wanted to buy here lately and couldn't, in tonnes:
    /// the order book shipyards build to. Halves every month.
    #[serde(default)]
    pub hull_wanted: f64,
    /// The settlement's money, in credits. Trade only moves money; nothing
    /// creates or destroys it yet.
    pub treasury: f64,
}

impl LocalEconomy {
    /// Recompute full-rate demand from population and facilities. Call after
    /// changing either.
    pub fn refresh_demand(&mut self) {
        const N: usize = Good::COUNT;
        let mut demand = [0.0; N];
        for (good, per_capita) in PER_CAPITA_DEMAND {
            demand[good as usize] += per_capita * self.population;
        }
        for facility in &self.facilities {
            let inputs = &DENSE[facility.kind as usize].inputs;
            for g in 0..N {
                demand[g] += inputs[g] * facility.scale;
            }
        }
        // A project wants its materials over the next month or so.
        if let Some(project) = &self.project {
            for g in 0..N {
                demand[g] += project.needs.0[g] * (1.0 / crate::construction::BUILD_DAYS);
            }
        }
        self.market.demand = GoodMap(demand);
    }

    /// One day of production and consumption. People take what they need
    /// first; facilities then share what's left of each input in proportion
    /// to what they want of it, so no facility comes first because of where
    /// it sits in the list. Runs over [`DenseRecipe`] rows: every step is
    /// the same few multiplies for every good.
    pub(crate) fn produce(&mut self) {
        const N: usize = Good::COUNT;
        let market = &mut self.market;
        let stock = &mut market.stock.0;
        let mut shortfall = [0.0; N];

        for (good, per_capita) in PER_CAPITA_DEMAND {
            let g = good as usize;
            let want = per_capita * self.population;
            let got = want.min(stock[g].max(0.0));
            stock[g] -= got;
            shortfall[g] += want - got;
        }

        // Which facilities can run at all today: full storage, or a yard
        // with all the hull it can hold, stops them.
        let mut full = 0u8;
        for g in 0..N {
            let target = (market.demand.0[g] * TARGET_DAYS).max(MIN_TARGET);
            if stock[g] >= target * STORAGE_MULTIPLE {
                full |= 1 << g;
            }
        }
        let (hull_t, hull_wanted) = (self.hull_t, self.hull_wanted);
        let open = |facility: &Facility| {
            DENSE[facility.kind as usize].output_mask & full == 0
                && (facility.kind != FacilityKind::Shipyard
                    || hull_t
                        < hull_wanted + facility.scale * HULL_T_PER_RUN * HULL_STORAGE_DAYS)
        };

        // Facilities by kind are few (one each, normally), so which are
        // running fits in a bit set; the tail past 64, if ever, just runs
        // the check again.
        let mut running = 0u64;
        let mut wanted = [0.0; N];
        for (i, facility) in self.facilities.iter().enumerate() {
            if !open(facility) {
                continue;
            }
            running |= 1u64.checked_shl(i as u32).unwrap_or(0);
            let inputs = &DENSE[facility.kind as usize].inputs;
            for g in 0..N {
                wanted[g] += inputs[g] * facility.scale;
            }
        }
        let mut share = [1.0; N];
        for g in 0..N {
            if wanted[g] > 0.0 {
                share[g] = (stock[g].max(0.0) / wanted[g]).min(1.0);
            }
        }

        let mut used = [0.0; N];
        let mut made = [0.0; N];
        for (i, facility) in self.facilities.iter_mut().enumerate() {
            let recipe = &DENSE[facility.kind as usize];
            // A facility stopped because nobody needs its output wants
            // nothing today: that isn't a shortage of its inputs.
            let on = if i < 64 {
                running & (1 << i) != 0
            } else {
                open(facility)
            };
            let rate = if on {
                let mut rate: f64 = 1.0;
                for (g, &s) in share.iter().enumerate() {
                    let s = if recipe.input_mask & (1 << g) != 0 { s } else { 1.0 };
                    rate = if s < rate { s } else { rate };
                }
                rate
            } else {
                0.0
            };
            facility.utilization += (rate - facility.utilization) * (1.0 / UTILIZATION_DAYS);
            let missing = if on { 1.0 - rate } else { 0.0 };
            let (take, short, give) = (
                facility.scale * rate,
                facility.scale * missing,
                facility.scale * rate,
            );
            for g in 0..N {
                used[g] += recipe.inputs[g] * take;
                shortfall[g] += recipe.inputs[g] * short;
                made[g] += recipe.outputs[g] * give;
            }
            if facility.kind == FacilityKind::Shipyard {
                self.hull_t += facility.scale * HULL_T_PER_RUN * rate;
            }
        }
        // Shares keep what facilities use within stock; today's output
        // reaches the market at the end of the day.
        for g in 0..N {
            stock[g] = (stock[g] - used[g]).max(0.0) + made[g];
        }
        market.shortfall.0 = shortfall;
    }

    pub fn update_prices(&mut self, model: EconomyModel) {
        let market = &mut self.market;
        for good in Good::ALL {
            market.price[good] = market.price_at(good, market.stock[good], model);
        }
    }
}

/// Recompute every settlement's prices from its stock, once at the end of
/// the day (DESIGN §5.1.1).
pub fn update_all_prices(state: &mut GalaxyState) {
    let factions = &state.factions;
    for settlement in state.settlements.values_mut() {
        let model = factions[settlement.owner].economy;
        settlement.economy.update_prices(model);
    }
}

/// Galaxy-wide totals, for tools and tests.
#[derive(Clone, Debug, Default)]
pub struct EconomySummary {
    pub population: f64,
    /// Credits held by all settlements, companies and colony ships.
    pub money: f64,
    /// Sum of faction balances; negative means money factions have issued.
    pub faction_balance: f64,
    pub companies: usize,
    pub ships: u32,
    pub ships_travelling: u32,
    /// Goods aboard ships, by good.
    pub cargo: GoodMap<f64>,
    pub stock: GoodMap<f64>,
    pub demand: GoodMap<f64>,
    pub shortfall: GoodMap<f64>,
    /// Mean price across markets with any demand or stock of the good.
    pub mean_price: GoodMap<f64>,
    /// Markets where some of this good's demand went unmet.
    pub markets_short: GoodMap<usize>,
}

pub fn summarize(state: &GalaxyState) -> EconomySummary {
    let mut out = EconomySummary::default();
    let mut price_n = GoodMap::splat(0usize);
    for settlement in state.settlements.values() {
        let e = &settlement.economy;
        out.population += e.population;
        out.money += e.treasury;
        for good in Good::ALL {
            let m = &e.market;
            out.stock[good] += m.stock[good];
            out.demand[good] += m.demand[good];
            out.shortfall[good] += m.shortfall[good];
            if m.shortfall[good] > 1e-6 {
                out.markets_short[good] += 1;
            }
            if m.demand[good] > 0.0 || m.stock[good] > 0.0 {
                out.mean_price[good] += m.price[good];
                price_n[good] += 1;
            }
        }
    }
    for good in Good::ALL {
        if price_n[good] > 0 {
            out.mean_price[good] /= price_n[good] as f64;
        }
    }
    out.faction_balance = state.factions.values().map(|f| f.treasury).sum();
    out.money += state.colony_ships.values().map(|c| c.credits).sum::<f64>();
    out.companies = state.companies.len();
    for company in state.companies.values() {
        out.money += company.credits;
        for fleet in &company.fleets {
            out.ships += fleet.ships;
            if !matches!(fleet.at, crate::haul::Location::Docked(_)) {
                out.ships_travelling += fleet.ships;
            }
            if let Some(cargo) = fleet.cargo {
                out.cargo[cargo.good] += cargo.amount;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn economy(facilities: Vec<Facility>) -> LocalEconomy {
        let mut e = LocalEconomy {
            population: 10.0,
            facilities,
            ..LocalEconomy::default()
        };
        e.market.stock[Good::Metal] = 10.0;
        e.market.stock[Good::Fuel] = 10.0;
        e.market.stock[Good::Components] = 100.0;
        e.refresh_demand();
        e
    }

    #[test]
    fn facility_order_doesnt_decide_who_gets_inputs() {
        let yard = Facility::new(FacilityKind::Shipyard, 10.0);
        let factory = Facility::new(FacilityKind::Factory, 10.0);
        let mut a = economy(vec![yard, factory]);
        let mut b = economy(vec![factory, yard]);
        a.produce();
        b.produce();
        assert_eq!(a.market.stock, b.market.stock);
        assert_eq!(a.hull_t, b.hull_t);
        // Both wanted 10 metal of the 10 there: each got half.
        assert!((a.hull_t - 5.0).abs() < 1e-9, "hull {}", a.hull_t);
        let made = a.market.stock[Good::Components] - (100.0 - 0.5 - 1.5);
        assert!((made - 5.0).abs() < 1e-9, "components made {made}");
    }

    #[test]
    fn people_eat_before_industry() {
        let mut e = economy(vec![Facility::new(FacilityKind::Factory, 20.0)]);
        e.market.stock[Good::Fuel] = 3.0;
        // Room in its store, so the factory wants to run.
        e.market.stock[Good::Components] = 10.0;
        e.produce();
        // The 3 fuel there is what 10 people need: the factory gets none.
        let short = e.market.shortfall[Good::Fuel];
        assert!((short - 10.0).abs() < 1e-9, "fuel short {short}");
        assert_eq!(e.market.stock[Good::Metal], 10.0);
    }
    #[test]
    fn price_curve_follows_powf() {
        let mut x: f64 = 0.01;
        while x < 40.0 {
            let want = x.powf(0.6).clamp(0.25, 5.0);
            let got = price_curve(x);
            assert!((got - want).abs() <= 1e-5 * want, "{x}: {got} vs {want}");
            x *= 1.0137;
        }
        assert_eq!(price_curve(0.0), 0.25);
        assert!((price_curve(f64::INFINITY) - 5.0).abs() < 1e-5);
        assert!((price_curve(f64::NAN) - 0.25).abs() < 1e-5);
        assert_eq!(price_curve(1.0), 1.0);
    }
}
