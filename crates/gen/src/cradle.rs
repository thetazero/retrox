//! The cradle (DESIGN §4.2, issue #15): history starts from one homeworld
//! at the dawn of FTL, shared by a few rival nations. Everything else in
//! the galaxy starts empty; colonization fills it.
//!
//! The cradle is the best habitable world in a well-connected system away
//! from the rim. Each founding nation holds one of its regions, with a
//! population large enough to grow, industry enough to build colony ships,
//! and a few hauling companies.

use retrox_sim::economy::LocalEconomy;
use retrox_sim::{
    EconomyModel, Faction, GalaxyState, GalaxyTime, HistoryKind, RegionId, Settlement,
    SettlementId, Site, SystemId,
};

use crate::GalaxyConfig;
use crate::factions::{faction_name, hsl};
use crate::rng::Rng;

/// Population of each founding nation at the start.
const NATION_POPULATION: (f64, f64) = (60.0, 120.0);

/// The cradle system and the world the nations share.
fn pick(
    state: &GalaxyState,
    config: &GalaxyConfig,
    nations: usize,
) -> Option<(SystemId, Vec<RegionId>)> {
    state
        .systems
        .iter()
        .filter(|(_, s)| {
            s.lanes.len() >= 3 && s.position.distance(Default::default()) < 0.7 * config.radius_ly
        })
        .flat_map(|(id, s)| s.bodies.iter().map(move |&b| (id, b)))
        .filter(|&(_, b)| state.bodies[b].regions.len() >= nations)
        .max_by(|&(sa, a), &(sb, b)| {
            let score = |b: retrox_sim::BodyId| {
                let body = &state.bodies[b];
                body.habitability + 0.05 * body.regions.len() as f64
            };
            score(a).total_cmp(&score(b)).then(sb.cmp(&sa))
        })
        .map(|(system, body)| {
            // The nations take the most habitable regions.
            let mut regions = state.bodies[body].regions.clone();
            regions.sort_by(|&a, &b| {
                state.regions[b]
                    .habitability
                    .total_cmp(&state.regions[a].habitability)
                    .then(a.cmp(&b))
            });
            regions.truncate(nations);
            (system, regions)
        })
}

/// Found the cradle's nations. Returns their capitals, or nothing if no
/// world in the galaxy can hold them.
pub fn found(rng: &mut Rng, state: &mut GalaxyState, config: &GalaxyConfig) -> Vec<SettlementId> {
    let (lo, hi) = config.cradle_nations;
    let nations = lo + rng.below(hi - lo + 1);
    let Some((system, regions)) = pick(state, config, nations) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut capitals = Vec::new();
    for (i, region) in regions.into_iter().enumerate() {
        let economy = if rng.chance(0.5) {
            EconomyModel::Planned
        } else {
            EconomyModel::Market
        };
        let region_name = state.regions[region].name.clone();
        let mut name = faction_name(rng, &region_name, economy, false);
        while names.contains(&name) {
            name = faction_name(rng, &region_name, economy, true);
        }
        names.push(name.clone());
        let faction = state.factions.insert(Faction {
            name,
            economy,
            capital: SettlementId::default(),
            color: hsl(
                (i as f64 * 137.508 + rng.range(0.0, 60.0)) % 360.0,
                0.65,
                0.6,
            ),
            founded: GalaxyTime::ZERO,
            treasury: 0.0,
            population: 0.0,
        });
        state
            .history
            .record(GalaxyTime::ZERO, HistoryKind::FactionFounded { faction });
        let capital = state.add_settlement(Settlement {
            system,
            site: Site::Region(region),
            owner: faction,
            name: region_name,
            founded: GalaxyTime::ZERO,
            economy: LocalEconomy {
                population: rng.range(NATION_POPULATION.0, NATION_POPULATION.1),
                ..LocalEconomy::default()
            },
        });
        state.factions[faction].capital = capital;
        state.history.record(
            GalaxyTime::ZERO,
            HistoryKind::SettlementFounded {
                settlement: capital,
                faction,
            },
        );
        capitals.push(capital);
    }
    crate::factions::refresh_populations(state);
    capitals
}
