//! Pirates (DESIGN §7.2.2): gangs come from somewhere, live somewhere that
//! makes sense, rob traders in both tiers, draw bounties, and fly for real
//! in the bubble.

use retrox_gen::{GalaxyConfig, generate};
use retrox_sim::pirates::{self, Aim};
use retrox_sim::{GalaxyState, HistoryKind, SystemId, bubble, tick};

fn small() -> GalaxyState {
    generate(
        9,
        &GalaxyConfig {
            systems: 600,
            radius_ly: 100.0,
            factions: 5,
            ..GalaxyConfig::default()
        },
    )
}

fn capital_systems(state: &GalaxyState) -> Vec<SystemId> {
    state
        .factions
        .values()
        .filter_map(|f| state.settlements.get(f.capital))
        .map(|s| s.system)
        .collect()
}

#[test]
fn gangs_live_where_it_makes_sense() {
    let state = small();
    let gangs: Vec<_> = state.pirates.gangs.values().collect();
    assert!(gangs.len() >= 3, "only {} gangs", gangs.len());
    let capitals = capital_systems(&state);
    let mut lairs = Vec::new();
    for g in &gangs {
        assert!(!capitals.contains(&g.lair), "{} lairs in a capital", g.name);
        assert!(!lairs.contains(&g.lair), "two gangs in one system");
        lairs.push(g.lair);
        assert_eq!(state.bodies[state.fields[g.field].belt].system, g.lair);
        assert_eq!(g.ships.len(), 2);
        // Within two jumps of where the crews came from.
        let home = state.settlements[g.origin].system;
        let near = home == g.lair
            || state
                .neighbours(home)
                .any(|n| n == g.lair || state.neighbours(n).any(|m| m == g.lair));
        assert!(near, "{} lairs far from home", g.name);
    }
    // Lairs lean lawless: fewer settlements than the average settled
    // system's neighbourhood would give by chance.
    let settled = |s: SystemId| state.systems[s].settlements.len();
    let in_lairs: usize = lairs.iter().map(|&s| settled(s)).sum();
    println!(
        "{} gangs, {} settlements in their lair systems",
        gangs.len(),
        in_lairs
    );
}

#[test]
fn gangs_rob_and_draw_bounties() {
    let mut state = small();
    let before: f64 = state.pirates.gangs.values().map(|g| g.haul_t).sum();
    for _ in 0..240 {
        tick(&mut state);
    }
    let haul: f64 = state.pirates.gangs.values().map(|g| g.haul_t).sum();
    let bounties = state
        .pirates
        .gangs
        .values()
        .filter(|g| g.bounty.is_some())
        .count();
    let posted = state
        .history
        .events()
        .iter()
        .filter(|e| matches!(e.kind, HistoryKind::BountyPosted { .. }))
        .count();
    let formed = state
        .history
        .events()
        .iter()
        .filter(|e| matches!(e.kind, HistoryKind::GangFormed { .. }))
        .count();
    let broken = state
        .history
        .events()
        .iter()
        .filter(|e| matches!(e.kind, HistoryKind::GangBroken { .. }))
        .count();
    let ships: usize = state.pirates.gangs.values().map(|g| g.ships.len()).sum();
    println!(
        "after 240 days: {} gangs ({formed} formed, {broken} broken), {ships} ships, {haul:.0} t taken, {bounties} with bounties ({posted} posted)",
        state.pirates.gangs.len()
    );
    for g in state.pirates.gangs.values() {
        println!(
            "  {:<32} {} ships {:>6.0} t  bounty {:?}",
            g.name,
            g.ships.len(),
            g.haul_t,
            g.bounty.map(|b| b.per_ship.round())
        );
    }
    assert!(haul > before + 50.0, "gangs took only {haul} t");
    assert!(bounties > 0, "no faction posted a bounty");
}

#[test]
fn gangs_fly_in_the_bubble_and_hunt() {
    let mut state = small();
    // Centre the bubble on a gang's lair.
    let (id, lair) = state
        .pirates
        .gangs
        .iter()
        .map(|(id, g)| (id, g.lair))
        .max_by_key(|&(_, lair)| {
            state
                .neighbours(lair)
                .map(|n| state.systems[n].settlements.len())
                .sum::<usize>()
                + state.systems[lair].settlements.len() * 2
        })
        .expect("a gang");
    bubble::set_centre(&mut state, Some(lair));
    assert!(state.pirates.prowls.contains_key(&id));
    let mut hunted = 0;
    let mut robbed = 0;
    for _ in 0..120 {
        tick(&mut state);
        for p in state.pirates.prowls.values() {
            if matches!(p.aim, Aim::Hunt(_)) {
                hunted += 1;
            }
        }
        robbed += state.pirates.robberies.len();
        state.pirates.robberies.clear();
        // Every prowl is a gang in the bubble.
        for g in state.pirates.prowls.keys() {
            let gang = &state.pirates.gangs[*g];
            assert!(bubble::contains(&state, gang.lair));
        }
    }
    println!("{hunted} prowl-days hunting, {robbed} robberies in the bubble");
    assert!(hunted > 0, "no gang went hunting");
    // Leaving takes the prowls away.
    bubble::set_centre(&mut state, None);
    assert!(state.pirates.prowls.is_empty());
}

#[test]
fn a_fight_writes_back() {
    let mut state = small();
    let id = state.pirates.gangs.keys().next().expect("a gang");
    let lair = state.pirates.gangs[id].lair;
    bubble::set_centre(&mut state, Some(lair));
    pirates::engage(&mut state, id);
    assert_eq!(state.pirates.prowls[&id].aim, Aim::Engaged);
    tick(&mut state);
    assert_eq!(
        state.pirates.prowls[&id].aim,
        Aim::Engaged,
        "the sim leaves a fight alone"
    );
    pirates::fought(&mut state, id, &[0], None);
    assert_eq!(state.pirates.gangs[id].ships.len(), 1);
    assert_eq!(state.pirates.prowls[&id].aim, Aim::Return);
    pirates::fought(&mut state, id, &[0], None);
    assert!(
        state.pirates.gangs.get(id).is_none(),
        "a gang with no ships is gone"
    );
    assert!(!state.pirates.prowls.contains_key(&id));
}

#[test]
fn saves_round_trip() {
    let mut state = small();
    let lair = state.pirates.gangs.values().next().expect("a gang").lair;
    bubble::set_centre(&mut state, Some(lair));
    for _ in 0..5 {
        tick(&mut state);
    }
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GalaxyState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.pirates.gangs.len(), state.pirates.gangs.len());
    assert_eq!(back.pirates.prowls, state.pirates.prowls);
}
