//! Pirates (DESIGN §7.2.2): gangs that come from somewhere, hole up in an
//! asteroid field where pickings are good and the law is thin, rob the
//! traders who pass, and draw bounties from the factions they rob.
//!
//! - **Where they come from.** Each settlement looks at itself once a month,
//!   on its own day. Hunger and want (food and fuel going short) turn some
//!   crews to piracy; even a settlement doing well loses one now and then.
//!   A new gang takes two light fighters to a lair within two jumps.
//! - **Where they live.** A lair is an asteroid field in a system with no
//!   faction capital, chosen by prey (settlements within a jump, whose
//!   traffic passes by) over law (settlements in the lair system itself),
//!   with chokepoints (systems of one or two lanes) preferred. One gang per
//!   system.
//! - **Raids.** In the coarse tier, a trader trip that starts or ends in a
//!   lair system, or a system next to one, may be robbed on arrival: the
//!   gang takes up to [`RAID_T_PER_SHIP`] tonnes per ship, and now and then a
//!   ship. In the bubble (§5.2) gangs are ships: they lurk at their field,
//!   fly out under the shared autopilot to intercept a laden trader or the
//!   player, and fly home again (see [`Prowl`]).
//! - **Growth and decline.** Every [`GROW_T`] tonnes taken buys a gang
//!   another ship, up to [`MAX_SHIPS`]; a gang that goes
//!   [`STARVE_DAYS`] without a raid loses one, and a gang with none left is
//!   gone.
//! - **Bounties.** At its monthly review a gang that has robbed a faction's
//!   traders draws a bounty from the faction it has robbed most, priced by
//!   how much it has taken, if the faction can afford it for the whole gang.
//!   It is paid from the faction's treasury for each of the gang's ships
//!   destroyed (see [`claim_bounty`]).
//!
//! Fights with the player happen in the client, which tells the sim how
//! they ended ([`engage`], [`fought`]).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::bubble::{self, FleetKey, Stage};
use crate::economy::Good;
use crate::flight::{self, Goal, Kinematics};
use crate::haul::{self, CompanyId};
use crate::ledger::{self, Account};
use crate::{FactionId, FieldId, GalaxyState, GalaxyTime, Rng, SettlementId, SystemId};

new_key_type! {
    /// Gangs form and break up, so they use generational keys.
    pub struct GangId;
}

const METRES_PER_AU: f64 = 1.496e11;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Most ships a gang runs.
pub const MAX_SHIPS: usize = 5;
/// Tonnes stolen that buy a gang another ship.
pub const GROW_T: f64 = 80.0;
/// Most a raid takes, per raiding ship, in tonnes.
pub const RAID_T_PER_SHIP: f64 = 25.0;
/// A gang this long without a raid loses a ship at its review.
pub const STARVE_DAYS: u64 = 150;
/// How far from their position a gang in the bubble spots prey.
pub const HUNT_M: f64 = 0.4 * METRES_PER_AU;
/// A hunt that hasn't caught its prey in this many days is given up.
const HUNT_DAYS: u64 = 3;
/// A gang just out of a fight with the player keeps away from them for
/// this many days.
pub const REST_DAYS: u64 = 3;
/// A bounty per ship starts here...
const BOUNTY_BASE: f64 = 2_500.0;
/// ...rises this much per tonne the gang has taken...
const BOUNTY_PER_T: f64 = 15.0;
/// ...and stops here.
const BOUNTY_MAX: f64 = 9_000.0;
/// A gunship is worth this many times a fighter's bounty.
const GUNSHIP_BOUNTY: f64 = 2.0;
/// Chance a month that a settlement doing fine loses a crew to piracy...
const BASE_CHANCE: f64 = 0.004;
/// ...plus this much per unit of hardship (the share of its food and fuel
/// demand going unmet).
const HARDSHIP_CHANCE: f64 = 0.25;
/// At most one gang per this many settlements, plus a few.
const SETTLEMENTS_PER_GANG: usize = 6;

/// A hull pirates fly, registered by generation from the ship content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaiderHull {
    pub id: String,
    /// Main-drive acceleration of its stock fit, m/s².
    pub accel: f64,
    /// A heavy hull (a gunship), only for gangs of three or more.
    pub heavy: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gang {
    pub name: String,
    pub lair: SystemId,
    /// The field they hide in.
    pub field: FieldId,
    /// Where the crews came from.
    pub origin: SettlementId,
    pub founded: GalaxyTime,
    /// Hull ids of its ships, in the order they fly.
    pub ships: Vec<String>,
    /// Tonnes taken, all told, and since the last new ship.
    pub haul_t: f64,
    pub grown_t: f64,
    pub last_raid: GalaxyTime,
    /// Tonnes taken from each faction's traders.
    pub victims: Vec<(FactionId, f64)>,
    pub bounty: Option<Bounty>,
    /// Ships the player has destroyed, all told.
    #[serde(default)]
    pub lost: u32,
}

/// A faction's price on a gang's ships.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounty {
    pub faction: FactionId,
    /// Paid for each fighter destroyed; gunships pay more (see
    /// [`ship_bounty`]).
    pub per_ship: f64,
    pub posted: GalaxyTime,
}

/// A gang flying in the bubble.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prowl {
    /// In the lair system's frame, as every flight is.
    pub ship: Kinematics,
    pub aim: Aim,
    /// The day the current aim began.
    pub since: GalaxyTime,
    /// It keeps away from the player until this day.
    pub rest_until: GalaxyTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aim {
    /// Holding at the lair field.
    Lurk,
    /// Flying out to intercept a trader fleet.
    Hunt(FleetKey),
    /// After the player. The client flies a stalk, since only it knows
    /// where the player is between ticks.
    Stalk,
    /// In a fight with the player; the client has it.
    Engaged,
    /// Flying home to the field.
    Return,
}

/// A robbery in the bubble, for the client to report if it's in view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Robbery {
    pub gang: GangId,
    pub system: SystemId,
    pub company: CompanyId,
    pub good: Option<Good>,
    pub tonnes: f64,
    pub ships_lost: u32,
}

/// The pirates' side of the galaxy state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Pirates {
    pub gangs: crate::pool::Pool<GangId, Gang>,
    pub hulls: Vec<RaiderHull>,
    /// Gangs flying in the bubble, by gang.
    #[serde(with = "prowl_pairs")]
    pub prowls: BTreeMap<GangId, Prowl>,
    /// Where the player is flying, if they're in space in a bubble system:
    /// the client keeps it up to date. In the system's frame, as flights.
    pub player: Option<(SystemId, Kinematics)>,
    /// Robberies in the bubble since the client last looked.
    pub robberies: Vec<Robbery>,
    /// Which gang lairs in or next to each system, and whether it's the
    /// lair itself. Rebuilt when empty.
    #[serde(skip)]
    near: HashMap<SystemId, (GangId, bool)>,
}

mod prowl_pairs {
    use super::{BTreeMap, GangId, Prowl};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(map: &BTreeMap<GangId, Prowl>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(map.iter())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<GangId, Prowl>, D::Error> {
        Ok(Vec::<(GangId, Prowl)>::deserialize(d)?.into_iter().collect())
    }
}

fn gang_bits(id: GangId) -> u64 {
    slotmap::Key::data(&id).as_ffi()
}

fn settlement_bits(id: SettlementId) -> u64 {
    slotmap::Key::data(&id).as_ffi()
}

/// The fields in a system.
pub fn fields_in(state: &GalaxyState, system: SystemId) -> Vec<FieldId> {
    state
        .fields
        .iter()
        .filter(|(_, f)| state.bodies[f.belt].system == system)
        .map(|(id, _)| id)
        .collect()
}

/// The gang that lairs in `system`, if any.
pub fn gang_in(state: &GalaxyState, system: SystemId) -> Option<GangId> {
    state
        .pirates
        .gangs
        .iter()
        .find(|(_, g)| g.lair == system)
        .map(|(id, _)| id)
}

/// The gang whose hideout is `field`, if any.
pub fn gang_at(state: &GalaxyState, field: FieldId) -> Option<GangId> {
    state
        .pirates
        .gangs
        .iter()
        .find(|(_, g)| g.field == field)
        .map(|(id, _)| id)
}

/// The slowest of a gang's ships sets its pace, m/s².
pub fn accel(state: &GalaxyState, gang: &Gang) -> f64 {
    gang.ships
        .iter()
        .filter_map(|h| state.pirates.hulls.iter().find(|r| &r.id == h))
        .map(|r| r.accel)
        .fold(f64::INFINITY, f64::min)
        .min(200.0)
}

/// What one of a gang's ships is worth to its bounty's faction.
pub fn ship_bounty(state: &GalaxyState, gang: &Gang, ship: usize) -> f64 {
    let Some(b) = gang.bounty else {
        return 0.0;
    };
    let heavy = gang
        .ships
        .get(ship)
        .and_then(|h| state.pirates.hulls.iter().find(|r| &r.id == h))
        .is_some_and(|r| r.heavy);
    if heavy {
        b.per_ship * GUNSHIP_BOUNTY
    } else {
        b.per_ship
    }
}

/// Whether a faction keeps its capital in `system`.
fn capital_system(state: &GalaxyState, system: SystemId) -> bool {
    state.factions.values().any(|f| {
        state
            .settlements
            .get(f.capital)
            .is_some_and(|s| s.system == system)
    })
}

/// Choose a lair within two jumps of `origin`'s system, or `None` if
/// nowhere will do.
fn choose_lair(state: &GalaxyState, rng: &mut Rng, origin: SystemId) -> Option<(SystemId, FieldId)> {
    let mut seen = vec![origin];
    let mut frontier = vec![origin];
    for _ in 0..2 {
        let mut next = Vec::new();
        for s in frontier {
            for n in state.neighbours(s) {
                if !seen.contains(&n) {
                    seen.push(n);
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
    let mut options: Vec<(SystemId, FieldId, f64)> = Vec::new();
    for &s in &seen {
        if capital_system(state, s) || gang_in(state, s).is_some() {
            continue;
        }
        let fields = fields_in(state, s);
        if fields.is_empty() {
            continue;
        }
        let law = state.systems[s].settlements.len() as f64;
        let prey: f64 = state
            .neighbours(s)
            .map(|n| state.systems[n].settlements.len() as f64)
            .sum::<f64>()
            + law
            + 0.5;
        let choke = if state.systems[s].lanes.len() <= 2 {
            1.6
        } else {
            1.0
        };
        let score = prey * choke / (1.0 + 0.7 * law);
        let field = fields[rng.below(fields.len())];
        options.push((s, field, score));
    }
    let total: f64 = options.iter().map(|o| o.2).sum();
    if total <= 0.0 {
        return None;
    }
    let mut pick = rng.f64() * total;
    for (s, f, w) in &options {
        if pick < *w {
            return Some((*s, *f));
        }
        pick -= w;
    }
    options.last().map(|o| (o.0, o.1))
}

const NAMES: &[&str] = &[
    "{} Wreckers",
    "Free Crews of {}",
    "{} Black Brigade",
    "Lantern Gang of {}",
    "Ash Rats of {}",
    "{} Night Shift",
    "{} Salvage Cooperative",
    "Jackals of {}",
    "{} Red Lamps",
    "Last Shift of {}",
];

/// Found a gang from `origin`'s crews, if a lair can be found.
pub fn found(state: &mut GalaxyState, origin: SettlementId, rng: &mut Rng) -> Option<GangId> {
    let light: Vec<String> = state
        .pirates
        .hulls
        .iter()
        .filter(|h| !h.heavy)
        .map(|h| h.id.clone())
        .collect();
    if light.is_empty() {
        return None;
    }
    let home = state.settlements.get(origin)?.system;
    let (lair, field) = choose_lair(state, rng, home)?;
    let name = NAMES[rng.below(NAMES.len())].replace("{}", &state.systems[lair].name);
    let ships = (0..2).map(|_| light[rng.below(light.len())].clone()).collect();
    let today = state.time;
    let id = state.pirates.gangs.insert(Gang {
        name,
        lair,
        field,
        origin,
        founded: today,
        ships,
        haul_t: 0.0,
        grown_t: 0.0,
        last_raid: today,
        victims: Vec::new(),
        bounty: None,
        lost: 0,
    });
    state.pirates.near.clear();
    state.history.record(
        today,
        crate::HistoryKind::GangFormed {
            system: lair,
            origin,
        },
    );
    sync(state);
    Some(id)
}

/// The starting galaxy's gangs: as if some settlements' crews had already
/// gone over. Prehistory will grow them instead.
pub fn seed(state: &mut GalaxyState, share: f64) {
    let origins: Vec<SettlementId> = state.settlements.keys().collect();
    for origin in origins {
        let mut rng = Rng::stream(state.seed, "gang-seed", 0, settlement_bits(origin));
        if rng.chance(share) {
            found(state, origin, &mut rng);
        }
    }
}

/// How much of a settlement's food and fuel demand goes unmet, 0..1.
fn hardship(state: &GalaxyState, id: SettlementId) -> f64 {
    let m = &state.settlements[id].economy.market;
    [Good::Food, Good::Fuel]
        .into_iter()
        .filter(|&g| m.demand[g] > 1e-9)
        .map(|g| (m.shortfall[g] / m.demand[g]).clamp(0.0, 1.0))
        .fold(0.0, f64::max)
}

/// The monthly reviews: settlements whose crews may turn pirate, and gangs
/// that grow, starve, or draw a bounty. Runs in the planning phase.
pub fn review(state: &mut GalaxyState) {
    let day = state.time.days();
    let cap = state.settlements.len() / SETTLEMENTS_PER_GANG + 3;
    if !state.pirates.hulls.is_empty() && state.pirates.gangs.len() < cap {
        let due: Vec<SettlementId> = state
            .settlements
            .keys()
            .filter(|&id| (settlement_bits(id).wrapping_add(7)) % 30 == day % 30)
            .collect();
        for id in due {
            let p = BASE_CHANCE + HARDSHIP_CHANCE * hardship(state, id);
            let mut rng = Rng::stream(state.seed, "gang-found", day, settlement_bits(id));
            if rng.chance(p) && state.pirates.gangs.len() < cap {
                found(state, id, &mut rng);
            }
        }
    }
    let gangs: Vec<GangId> = state
        .pirates
        .gangs
        .keys()
        .filter(|&g| gang_bits(g) % 30 == day % 30)
        .collect();
    for g in gangs {
        review_gang(state, g);
    }
}

fn review_gang(state: &mut GalaxyState, id: GangId) {
    let today = state.time;
    let mut rng = Rng::stream(state.seed, "gang-review", today.days(), gang_bits(id));
    let hulls = state.pirates.hulls.clone();
    let Some(gang) = state.pirates.gangs.get_mut(id) else {
        return;
    };
    // Growth: plunder buys ships.
    while gang.grown_t >= GROW_T && gang.ships.len() < MAX_SHIPS {
        gang.grown_t -= GROW_T;
        let heavy_ok = gang.ships.len() >= 3 && !gang.ships.iter().any(|h| is_heavy(&hulls, h));
        let pool: Vec<&RaiderHull> = hulls.iter().filter(|h| !h.heavy || heavy_ok).collect();
        if pool.is_empty() {
            break;
        }
        // A gang big enough for a gunship gets one first.
        let pick = pool
            .iter()
            .find(|h| h.heavy)
            .copied()
            .unwrap_or_else(|| pool[rng.below(pool.len())]);
        gang.ships.push(pick.id.clone());
    }
    if gang.ships.len() >= MAX_SHIPS {
        gang.grown_t = gang.grown_t.min(GROW_T);
    }
    // Decline: crews drift away from a gang with nothing to take.
    if today.days().saturating_sub(gang.last_raid.days()) > STARVE_DAYS {
        gang.ships.pop();
        gang.last_raid = GalaxyTime::from_days(today.days().saturating_sub(STARVE_DAYS / 2));
    }
    if gang.ships.is_empty() {
        disband(state, id);
        return;
    }
    // Bounty: the faction it has robbed most prices its ships.
    let victim = gang
        .victims
        .iter()
        .filter(|(f, _)| state.factions.contains_key(*f))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|v| v.0);
    let per_ship = (BOUNTY_BASE + BOUNTY_PER_T * gang.haul_t).min(BOUNTY_MAX);
    let ships = gang.ships.len() as f64;
    let current = gang.bounty.filter(|b| state.factions.contains_key(b.faction));
    let bounty = match (current, victim) {
        (Some(b), _) => Some(Bounty {
            per_ship: b.per_ship.max(per_ship),
            ..b
        }),
        (None, Some(f)) => {
            let afford = ledger::spendable(state, Account::Faction(f));
            (afford >= per_ship * ships * 2.0).then_some(Bounty {
                faction: f,
                per_ship,
                posted: today,
            })
        }
        (None, None) => None,
    };
    let posted = current.is_none() && bounty.is_some();
    let lair = state.pirates.gangs[id].lair;
    state.pirates.gangs[id].bounty = bounty;
    if let (true, Some(b)) = (posted, bounty) {
        state.history.record(
            today,
            crate::HistoryKind::BountyPosted {
                faction: b.faction,
                system: lair,
            },
        );
    }
}

fn is_heavy(hulls: &[RaiderHull], id: &str) -> bool {
    hulls.iter().any(|h| h.id == id && h.heavy)
}

/// A gang with no ships left is gone.
fn disband(state: &mut GalaxyState, id: GangId) {
    if let Some(g) = state.pirates.gangs.remove(id) {
        state
            .history
            .record(state.time, crate::HistoryKind::GangBroken { system: g.lair });
    }
    state.pirates.prowls.remove(&id);
    state.pirates.near.clear();
}

fn index(state: &mut GalaxyState) {
    if !state.pirates.near.is_empty() || state.pirates.gangs.is_empty() {
        return;
    }
    let mut near = HashMap::new();
    for (id, g) in state.pirates.gangs.iter() {
        for n in state.neighbours(g.lair) {
            near.entry(n).or_insert((id, false));
        }
    }
    for (id, g) in state.pirates.gangs.iter() {
        near.insert(g.lair, (id, true));
    }
    state.pirates.near = near;
}

/// A coarse trip from `from` arrives today: if it passed a gang, it may
/// have been robbed. Returns false if the fleet is gone.
pub(crate) fn coarse_raid(
    state: &mut GalaxyState,
    key: CompanyId,
    n: usize,
    from: SettlementId,
    to: SettlementId,
) -> bool {
    if state.pirates.gangs.is_empty() {
        return true;
    }
    index(state);
    let ends = [from, to].map(|s| state.settlements.get(s).map(|s| s.system));
    let mut found = None;
    for system in ends.into_iter().flatten() {
        if let Some(&(g, own)) = state.pirates.near.get(&system) {
            if found.is_none_or(|(_, o): (GangId, bool)| own && !o) {
                found = Some((g, own));
            }
        }
    }
    let Some((gang, own)) = found else {
        return true;
    };
    let Some(g) = state.pirates.gangs.get(gang) else {
        return true;
    };
    if state.pirates.prowls.contains_key(&gang) {
        // In the bubble the gang hunts for real instead.
        return true;
    }
    let ships = g.ships.len() as f64;
    let p = (if own { 0.05 } else { 0.02 } * ships).min(0.4);
    let fleet = &state.companies[key].fleets[n];
    let entity = slotmap::Key::data(&key).as_ffi() ^ (u64::from(fleet.id) << 40);
    let mut rng = Rng::stream(state.seed, "raid", state.time.days(), entity);
    if !rng.chance(p) {
        return true;
    }
    rob(state, gang, key, n, to, &mut rng).1
}

/// `gang` robs fleet `n` of `key`, bound for `to`: takes cargo, and now and
/// then a ship. Returns the robbery and whether any of the fleet is left.
fn rob(
    state: &mut GalaxyState,
    gang: GangId,
    key: CompanyId,
    n: usize,
    to: SettlementId,
    rng: &mut Rng,
) -> (Robbery, bool) {
    let ships = state.pirates.gangs[gang].ships.len() as f64;
    let mut robbery = Robbery {
        gang,
        system: state.pirates.gangs[gang].lair,
        company: key,
        good: None,
        tonnes: 0.0,
        ships_lost: 0,
    };
    let fleet = &mut state.companies[key].fleets[n];
    if let Some(cargo) = fleet.cargo.as_mut() {
        let take = cargo.amount.min(RAID_T_PER_SHIP * ships);
        cargo.amount -= take;
        robbery.good = Some(cargo.good);
        robbery.tonnes = take;
        let good = cargo.good;
        if cargo.amount <= 1e-9 {
            fleet.cargo = None;
        }
        state.traffic.add(to, good, -take);
    }
    let mut alive = true;
    if rng.chance(0.2) {
        robbery.ships_lost = 1;
        let fleet = &mut state.companies[key].fleets[n];
        if fleet.ships > 1 {
            let share = f64::from(fleet.ships - 1) / f64::from(fleet.ships);
            fleet.ships -= 1;
            if let Some(c) = fleet.cargo.as_mut() {
                let gone = c.amount * (1.0 - share);
                c.amount -= gone;
                let good = c.good;
                state.traffic.add(to, good, -gone);
            }
        } else {
            haul::lose_fleet(state, key, n, to);
            alive = false;
        }
    }
    let faction = state
        .companies
        .get(key)
        .and_then(|c| state.settlements.get(c.home))
        .map(|s| s.owner);
    let today = state.time;
    let g = &mut state.pirates.gangs[gang];
    g.haul_t += robbery.tonnes;
    g.grown_t += robbery.tonnes;
    g.last_raid = today;
    if let Some(f) = faction {
        // Even an empty hold counts: they were attacked.
        let w = robbery.tonnes.max(5.0);
        match g.victims.iter_mut().find(|(v, _)| *v == f) {
            Some((_, t)) => *t += w,
            None => g.victims.push((f, w)),
        }
    }
    (robbery, alive)
}

/// Where a gang's field is at `t_days`, and how it moves: home, for its
/// ships.
pub fn field_goal(state: &GalaxyState, field: FieldId, t_days: f64) -> Goal {
    let f = &state.fields[field];
    let system = state.bodies[f.belt].system;
    let (p, v) = state.orbit_state(system, &f.orbit, t_days);
    Goal {
        pos: flight::scale(p, METRES_PER_AU),
        vel: flight::scale(v, METRES_PER_AU / SECONDS_PER_DAY),
    }
}

/// Where a prowl is headed at `t_days`, `t` seconds into a stretch that
/// began with its prey at `prey` (a hunt), as the sim and the client both
/// fly it. `None` for a stalk or a fight, which the client flies.
pub fn prowl_goal(
    state: &GalaxyState,
    gang: &Gang,
    prowl: &Prowl,
    prey: Option<Kinematics>,
    t: f64,
    t_days: f64,
) -> Option<Goal> {
    match prowl.aim {
        Aim::Lurk | Aim::Return => Some(field_goal(state, gang.field, t_days)),
        Aim::Hunt(_) => {
            let p = prey?;
            Some(Goal {
                pos: flight::add(p.pos, flight::scale(p.vel, t)),
                vel: p.vel,
            })
        }
        Aim::Stalk | Aim::Engaged => None,
    }
}

/// Where a trader fleet flying in `system` is now, if it is.
pub fn prey_ship(state: &GalaxyState, key: FleetKey, system: SystemId) -> Option<Kinematics> {
    match state.bubble.flights.get(&key)?.stage {
        Stage::Flying { system: s, ship } if s == system => Some(ship),
        _ => None,
    }
}

/// Bring the bubble's prowls in line with its systems and the gangs: a
/// gang whose lair is in the bubble flies, starting at home.
pub fn sync(state: &mut GalaxyState) {
    let t = state.time.days() as f64;
    let wanted: Vec<(GangId, FieldId)> = state
        .pirates
        .gangs
        .iter()
        .filter(|(_, g)| bubble::contains(state, g.lair))
        .map(|(id, g)| (id, g.field))
        .collect();
    state
        .pirates
        .prowls
        .retain(|id, _| wanted.iter().any(|(w, _)| w == id));
    for (id, field) in wanted {
        if state.pirates.prowls.contains_key(&id) {
            continue;
        }
        let home = field_goal(state, field, t);
        state.pirates.prowls.insert(
            id,
            Prowl {
                ship: Kinematics {
                    pos: home.pos,
                    vel: home.vel,
                },
                aim: Aim::Lurk,
                since: state.time,
                rest_until: GalaxyTime::ZERO,
            },
        );
    }
    if state.pirates.player.is_some_and(|(s, _)| !bubble::contains(state, s)) {
        state.pirates.player = None;
    }
}

/// A day for every gang in the bubble: lurk, pick prey, chase it, rob it,
/// fly home. Runs in the trade phase, after the traders have flown.
pub(crate) fn step(state: &mut GalaxyState) {
    sync(state);
    let today = state.time;
    let t0 = today.days() as f64;
    let ids: Vec<GangId> = state.pirates.prowls.keys().copied().collect();
    for id in ids {
        let (Some(gang), Some(&prowl)) = (state.pirates.gangs.get(id), state.pirates.prowls.get(&id))
        else {
            continue;
        };
        let lair = gang.lair;
        let accel = accel(state, gang);
        let mut next = prowl;
        match prowl.aim {
            Aim::Engaged => continue,
            Aim::Stalk => {
                // The client flies it; give up once the player has gone or
                // got away.
                let here = state.pirates.player.is_some_and(|(s, _)| s == lair);
                if !here || today.days() > prowl.since.days() + HUNT_DAYS {
                    next.aim = Aim::Return;
                    next.since = today;
                }
            }
            Aim::Lurk => {
                let home = field_goal(state, gang.field, t0 + 1.0);
                next.ship = Kinematics {
                    pos: home.pos,
                    vel: home.vel,
                };
                if let Some(prey) = pick_prey(state, lair, &prowl.ship) {
                    next.aim = Aim::Hunt(prey);
                    next.since = today;
                }
            }
            Aim::Hunt(key) => {
                let Some(prey) = prey_ship(state, key, lair) else {
                    next.aim = Aim::Return;
                    next.since = today;
                    state.pirates.prowls.insert(id, next);
                    continue;
                };
                let gang = &state.pirates.gangs[id];
                let caught = flight::fly(&mut next.ship, accel, SECONDS_PER_DAY, |t, _| {
                    prowl_goal(state, gang, &prowl, Some(prey), t, t0 + t / SECONDS_PER_DAY)
                        .expect("a hunt has a goal")
                });
                let close = flight::length(flight::sub(next.ship.pos, prey.pos)) < 2e7;
                if caught.is_some() || close {
                    if let Some(n) = haul::fleet_index(state, key.0, key.1) {
                        if let Some(&haul::Location::Flying { to }) =
                            state.companies.get(key.0).map(|c| &c.fleets[n].at)
                        {
                            let entity = gang_bits(id) ^ (u64::from(key.1) << 40);
                            let mut rng = Rng::stream(state.seed, "raid", today.days(), entity);
                            let (robbery, alive) = rob(state, id, key.0, n, to, &mut rng);
                            if !alive {
                                state.bubble.flights.remove(&key);
                            }
                            state.pirates.robberies.push(robbery);
                        }
                    }
                    next.aim = Aim::Return;
                    next.since = today;
                } else if today.days() > prowl.since.days() + HUNT_DAYS {
                    next.aim = Aim::Return;
                    next.since = today;
                }
            }
            Aim::Return => {
                let gang = &state.pirates.gangs[id];
                let home = flight::fly(&mut next.ship, accel, SECONDS_PER_DAY, |t, _| {
                    field_goal(state, gang.field, t0 + t / SECONDS_PER_DAY)
                });
                if home.is_some() {
                    next.aim = Aim::Lurk;
                    next.since = today;
                }
            }
        }
        state.pirates.prowls.insert(id, next);
    }
    // Nobody reads robberies outside the client; keep the list short.
    let n = state.pirates.robberies.len();
    if n > 32 {
        state.pirates.robberies.drain(..n - 32);
    }
}

/// The nearest laden trader flying in `system` within [`HUNT_M`] of `from`.
fn pick_prey(state: &GalaxyState, system: SystemId, from: &Kinematics) -> Option<FleetKey> {
    bubble::flights_in(state, system)
        .filter(|(key, _, _)| {
            haul::fleet_index(state, key.0, key.1)
                .is_some_and(|n| state.companies[key.0].fleets[n].cargo.is_some())
        })
        .map(|(key, _, ship)| (key, flight::length(flight::sub(ship.pos, from.pos))))
        .filter(|(_, d)| *d < HUNT_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

/// The client has the player in a fight with `gang`: the sim leaves it be.
pub fn engage(state: &mut GalaxyState, gang: GangId) {
    let today = state.time;
    if let Some(p) = state.pirates.prowls.get_mut(&gang) {
        p.aim = Aim::Engaged;
        p.since = today;
    }
}

/// The client has `gang` after the player: it flies the stalk.
pub fn stalk(state: &mut GalaxyState, gang: GangId) {
    let today = state.time;
    if let Some(p) = state.pirates.prowls.get_mut(&gang) {
        p.aim = Aim::Stalk;
        p.since = today;
    }
}

/// A fight with `gang` is over: the ships in `lost` (indices into its
/// ships) were destroyed, and what's left flies home from `at` (lair-system
/// frame), keeping away from the player for a while.
pub fn fought(state: &mut GalaxyState, gang: GangId, lost: &[usize], at: Option<Kinematics>) {
    let today = state.time;
    let Some(g) = state.pirates.gangs.get_mut(gang) else {
        return;
    };
    let mut lost = lost.to_vec();
    lost.sort_unstable();
    lost.dedup();
    for &i in lost.iter().rev() {
        if i < g.ships.len() {
            g.ships.remove(i);
            g.lost += 1;
        }
    }
    if g.ships.is_empty() {
        disband(state, gang);
        return;
    }
    if let Some(p) = state.pirates.prowls.get_mut(&gang) {
        p.aim = Aim::Return;
        p.since = today;
        p.rest_until = GalaxyTime::from_days(today.days() + REST_DAYS);
        if let Some(at) = at {
            p.ship = at;
        }
    }
}

/// Pay the bounty on ship `ship` of `gang`, destroyed by the player: the
/// faction that posted it pays what it can. Returns who paid and how much.
pub fn claim_bounty(state: &mut GalaxyState, gang: GangId, ship: usize) -> Option<(FactionId, f64)> {
    let g = state.pirates.gangs.get(gang)?;
    let b = g.bounty?;
    let want = ship_bounty(state, g, ship);
    let paid = want.min(ledger::spendable(state, Account::Faction(b.faction)));
    if paid <= 0.0 {
        return None;
    }
    state.factions.get_mut(b.faction)?.treasury -= paid;
    Some((b.faction, paid))
}

/// Every gang with a bounty on it within `jumps` jumps of `from`, nearest
/// first, with the distance in jumps.
pub fn wanted_near(state: &GalaxyState, from: SystemId, jumps: usize) -> Vec<(GangId, usize)> {
    let mut dist: HashMap<SystemId, usize> = HashMap::from([(from, 0)]);
    let mut frontier = vec![from];
    for d in 1..=jumps {
        let mut next = Vec::new();
        for s in frontier {
            for n in state.neighbours(s) {
                if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(n) {
                    e.insert(d);
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
    let mut out: Vec<(GangId, usize)> = state
        .pirates
        .gangs
        .iter()
        .filter(|(_, g)| g.bounty.is_some())
        .filter_map(|(id, g)| dist.get(&g.lair).map(|&d| (id, d)))
        .collect();
    out.sort_by_key(|&(id, d)| (d, gang_bits(id)));
    out
}
