//! Career bench (issue #84): bots play an hour of each career in a fresh
//! game and report what it earned, so the earning rates in DESIGN §7.8
//! are measured, not guessed.
//!
//! Run with `cargo test -p retrox-client careers -- --ignored
//! --nocapture`. Environment variables pick what runs: `CAREERS` (any of
//! `mining,trading,freight`), `SEEDS` (`1,2,3,4`), `HULL` (a stock hull
//! id, or `gunship-miner` for a mining refit), `CREDITS` (the starting
//! purse) and `BENCH_DEBUG` (print each trade and contract).
//!
//! Each bot flies the real world: fast travel, jumps,
//! approach and docking run through [`World::update`] frame by frame and
//! count as real time. Where a bot would need hands (turning the ship,
//! nudging up to a rock or a loose chunk, clicking through menus) it
//! moves the ship directly and is charged the seconds a practised pilot
//! would take, with the world running meanwhile (rocks spin, the laser
//! cools). The bots play well but not perfectly: they don't plan ahead,
//! and the trader only knows prices within a couple of jumps.

use glam::DVec3;
use retrox_sim::SettlementId;
use retrox_sim::economy::Good;
use retrox_sim::settlement::Site;

use super::pilot::look_rotation;
use super::*;

const DT: f64 = 1.0 / 30.0;
/// Seconds at a station's screens per visit: trading, the board.
const MENU_S: f64 = 12.0;
/// Seconds to turn onto a new heading by hand.
const TURN_S: f64 = 2.0;
/// Acceleration a pilot uses nudging around a field, m/s²: speeding up
/// and braking for a hop, capped well below combat speed.
const FIELD_ACCEL: f64 = 30.0;
/// One hour of play.
const HOUR: f64 = 3600.0;

/// A world plus the real seconds played in it.
struct Bot {
    w: World,
    real_s: f64,
    /// Real seconds spent on each kind of leg, for the report.
    legs: std::collections::BTreeMap<&'static str, f64>,
}

impl Bot {
    fn new(seed: u64) -> Self {
        let mut w = World::new(seed);
        // HULL=hauler etc. flies that stock ship instead of the courier.
        if let Ok(hull) = std::env::var("HULL") {
            w.player.ship.fit = if hull == "gunship-miner" {
                miner_gunship()
            } else {
                retrox_content::ships::stock(&hull).expect("a stock hull")
            };
        }
        if let Ok(c) = std::env::var("CREDITS") {
            w.player.credits = c.parse().expect("credits");
        }
        Self {
            w,
            real_s: 0.0,
            legs: Default::default(),
        }
    }

    fn frame(&mut self, c: Controls) {
        self.w.update(DT, c);
        self.real_s += DT;
    }

    /// Let `s` seconds pass hands-off.
    fn idle(&mut self, s: f64) {
        for _ in 0..(s / DT).ceil() as usize {
            self.frame(Controls::default());
        }
    }

    fn place_of(&self, s: SettlementId) -> Option<Place> {
        match self.w.galaxy.settlements[s].site {
            Site::Space(_) => Some(Place::Station(s)),
            Site::Region(r) => Some(Place::Body(self.w.galaxy.regions[r].body)),
            Site::Surface(_) => None,
        }
    }

    /// Undock or launch, and climb out of any gravity well.
    fn leave(&mut self) {
        let t0 = self.real_s;
        self.leave_inner();
        *self.legs.entry("leave").or_default() += self.real_s - t0;
    }

    fn leave_inner(&mut self) {
        match self.w.mode {
            Mode::Docked(_) => self.w.act(Action::Undock),
            Mode::Landed { .. } => self.w.act(Action::Launch),
            _ => {}
        }
        self.real_s += TURN_S;
        let mut n = 0;
        while let Some(well) = self.w.current_well() {
            let away = self.w.ship_position() - self.w.position(well);
            self.w.ship.rot = look_rotation(away);
            self.w.ship.throttle = 1.0;
            self.frame(Controls::default());
            n += 1;
            assert!(n < 30 * 600, "stuck in a well");
        }
    }

    /// Fast travel to a place (from open space) and wait to arrive.
    fn cruise(&mut self, place: Place) {
        let t0 = self.real_s;
        self.cruise_inner(place);
        *self.legs.entry("cruise").or_default() += self.real_s - t0;
    }

    fn cruise_inner(&mut self, place: Place) {
        // Raiders block fast travel: run until they give up the chase.
        let mut n = 0;
        while self.w.hostiles_near() > 0 {
            let away = self.w.combat_position().normalize_or(DVec3::X);
            self.w.ship.rot = look_rotation(away);
            self.w.ship.throttle = throttle_for(450.0);
            self.frame(Controls {
                boost: true,
                ..Default::default()
            });
            n += 1;
            assert!(
                n < 30 * 300,
                "couldn't shake raiders in 5 minutes: at {:.0} s, anchor {:?}, frame {:?}, {:.0} m from anchor, {:.0} m/s, target {place:?}",
                self.real_s,
                self.w.combat_anchor(),
                self.w.ship.frame,
                self.w.combat_position().length(),
                self.w.ship.vel.length()
            );
        }
        self.w.target = Some(place);
        self.w.act(Action::FastTravel);
        let mut n = 0;
        while matches!(self.w.mode, Mode::FastTravel(_)) {
            self.frame(Controls::default());
            n += 1;
            assert!(n < 30 * 300, "fast travel took over 5 minutes");
        }
    }

    /// Fly by hand to within `within` metres of a place, arriving slow.
    fn approach(&mut self, place: Place, within: f64) {
        let t0 = self.real_s;
        self.approach_inner(place, within);
        let key = if matches!(place, Place::Body(_)) {
            "approach body"
        } else {
            "approach"
        };
        if std::env::var("BENCH_DEBUG").is_ok() {
            println!(
                "      {key}: {:.0} s, radius {:.0} km, from {:.0} km",
                self.real_s - t0,
                self.w.place_radius(place) / 1e3,
                self.w.arrival_distance(place) / 1e3
            );
        }
        *self.legs.entry(key).or_default() += self.real_s - t0;
    }

    fn approach_inner(&mut self, place: Place, within: f64) {
        self.real_s += TURN_S;
        let mut n = 0;
        loop {
            let d = self.w.distance_to(place);
            if d < within && (self.w.ship.vel.length() < 120.0 || matches!(place, Place::Body(_))) {
                break;
            }
            let dir = self.w.position(place) - self.w.ship_position();
            self.w.ship.rot = look_rotation(dir);
            // Full throttle, easing off near the end: the flight model
            // brakes in proportion to speed, so this arrives in seconds.
            let want = (0.4 * (d - within * 0.5).max(0.0)).clamp(60.0, 150_000.0);
            self.w.ship.throttle = throttle_for(want);
            self.frame(Controls::default());
            n += 1;
            assert!(
                n < 30 * 600,
                "approach to {place:?} took over 10 minutes: {d:.0} m off, {:.0} m/s, within {within:.0}, hostiles {}, accel {:.1}, said {:?}, mode {:?}",
                self.w.ship.vel.length(),
                self.w.hostiles_near(),
                self.w.stats().acceleration(),
                self.w.message,
                self.w.mode
            );
        }
    }

    /// From anywhere, go and dock (or land) at a settlement.
    fn go(&mut self, to: SettlementId) {
        if self.w.docked_at() == Some(to) {
            return;
        }
        let place = self.place_of(to).expect("a settlement you can visit");
        if !matches!(self.w.mode, Mode::Normal) || self.w.current_well().is_some() {
            self.leave();
        }
        let dest = self.w.galaxy.settlements[to].system;
        if dest != self.w.system {
            let route = self.w.route(dest).expect("a route");
            for &hop in &route[1..] {
                if self.w.current_well().is_some() {
                    self.leave();
                }
                self.real_s += TURN_S;
                self.w.act(Action::Jump(hop));
                assert!(matches!(self.w.mode, Mode::Jumping { .. }), "jump refused");
                let t0 = self.real_s;
                while self.w.system != hop || !matches!(self.w.mode, Mode::Normal) {
                    self.frame(Controls::default());
                }
                *self.legs.entry("jump").or_default() += self.real_s - t0;
            }
        }
        if self.w.current_well().is_some() && !matches!(place, Place::Body(_)) {
            self.leave();
        }
        if self.w.distance_to(place) > self.w.arrival_distance(place) * 1.2 {
            if self.w.current_well().is_some() {
                self.leave();
            }
            self.cruise(place);
        }
        match place {
            Place::Station(s) => {
                let reach = self.w.station_size_m(s) * 1.5 + 400.0;
                self.approach(place, reach * 0.7);
                self.w.act(Action::Dock);
                // Contracts settle on the first frame docked.
                self.frame(Controls::default());
            }
            Place::Body(_) => {
                let r = self.w.place_radius(place);
                self.approach(place, r * 1.9);
                // A body may have several cities: pick this one, as a
                // player does in NAVIGATION.
                self.w.act(Action::TargetCity(to));
                self.w.act(Action::Land);
                self.idle(f64::from(LANDING_DURATION) + DT);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            self.w.docked_at(),
            Some(to),
            "failed to dock at {place:?}: mode {:?}, {:.0} m off, well {:?}",
            self.w.mode,
            self.w.distance_to(place),
            self.w.current_well()
        );
        self.real_s += MENU_S;
    }

    fn sell_all(&mut self) {
        for good in Good::ALL {
            let t = self.w.player.cargo[good] - self.held_under_contract(good);
            let t = t.floor();
            if t >= 1.0 {
                self.w.act(Action::Trade(good, -t));
            }
        }
    }

    fn held_under_contract(&self, good: Good) -> f64 {
        self.w
            .player
            .contracts
            .iter()
            .filter(|c| c.offer.good == good)
            .map(|c| c.offer.amount)
            .sum()
    }

    fn worth(&self) -> f64 {
        self.w.player.credits
    }
}

/// A miner's medium ship: a gunship with mining lasers in the bow and a
/// cargo bay amidships, as a player would refit one.
fn miner_gunship() -> retrox_sim::ship::Loadout {
    use retrox_content::ships::{CARGO_BAY_M, CARGO_POD, MINING_LASER};
    let mut fit = retrox_content::ships::stock("gunship").unwrap();
    fit.set_section(1, &CARGO_BAY_M).unwrap();
    for slot in 0..3 {
        fit.fit(1, slot, &CARGO_POD).unwrap();
    }
    fit.fit(0, 0, &MINING_LASER).unwrap();
    fit.fit(0, 1, &MINING_LASER).unwrap();
    println!(
        "gunship-miner: {:.0} cr, {:.0} t hold, cut {:.1}",
        retrox_content::ships::price::loadout_price(&fit),
        fit.stats().cargo_t,
        fit.stats().mining_cut
    );
    fit
}

/// Throttle setting for a forward speed (inverse of `throttle_speed`).
fn throttle_for(speed: f64) -> f64 {
    use super::pilot::{COMBAT_SPEED, CRUISE_SPEED};
    if speed <= COMBAT_SPEED {
        speed / COMBAT_SPEED * 0.5
    } else {
        0.5 + 0.5 * (speed / COMBAT_SPEED).ln() / (CRUISE_SPEED / COMBAT_SPEED).ln()
    }
}

// ---------------------------------------------------------------- mining

/// Mine in the nearest field until the hold is full or `until` real
/// seconds; returns tonnes mined.
fn mine_hold(b: &mut Bot, until: f64) -> f64 {
    let start_t = b.w.player.cargo_t();
    let Some((field, _)) = b.w.field_rocks.clone() else {
        return 0.0;
    };
    let cap = b.w.stats().cargo_t;
    let full = |b: &Bot| cap - b.w.player.cargo_t() < 0.3;
    while !full(b) && b.real_s < until {
        b.w.act(Action::Scan);
        // Sweep up loose chunks first, nearest first.
        let ship = b.w.ship.rel;
        let nugget = b.w.mining.field(field).and_then(|s| {
            s.nuggets
                .iter()
                .map(|n| n.pos)
                .min_by(|a, c| (*a - ship).length().total_cmp(&(*c - ship).length()))
        });
        if let Some(pos) = nugget {
            // Within the tractor's reach it comes to you; else go to it.
            if (pos - ship).length() > crate::mining::TRACTOR_RANGE * 0.8 {
                hop(b, pos + DVec3::Y * 12.0);
            }
            b.idle(1.0);
            continue;
        }
        // Otherwise the best rock nearby: grade and veins over the time to
        // get there.
        let (_, rocks) = b.w.field_rocks.clone().unwrap();
        let ship = b.w.ship.rel;
        let best =
            b.w.mining
                .live_rocks(field, &b.w.galaxy.fields[field], &rocks, b.w.clock_s)
                .filter(|r| !r.worked_out && r.radius >= 6.0)
                .map(|r| {
                    let d = (r.pos - ship).length();
                    let value = r.lode.grade
                        * (1.0 + 0.5 * r.lode.veins().len() as f64)
                        * crate::mining::fracture_yield(r.radius, &r.lode).max(0.2);
                    let work = crate::mining::integrity(r.radius) * (1.0 - r.damage)
                        / crate::mining::laser_power(&b.w.stats()).max(0.1)
                        / 2.5;
                    (value / (work + field_hop_s(d)), r)
                })
                .max_by(|a, c| a.0.total_cmp(&c.0))
                .map(|(_, r)| r);
        let Some(rock) = best else { break };
        let away = (ship - rock.pos).normalize_or(DVec3::Z);
        hop(b, rock.pos + away * (rock.radius * 1.3 + 40.0));
        cut(b, field, rock.id, until);
    }
    b.w.player.cargo_t() - start_t
}

/// Seconds to hop `d` metres inside a field: turn, then speed up and
/// brake.
fn field_hop_s(d: f64) -> f64 {
    TURN_S + 2.0 * (d / FIELD_ACCEL).sqrt()
}

/// Nudge the ship to a spot in the field: moved there, charged the time.
fn hop(b: &mut Bot, to: DVec3) {
    let t0 = b.real_s;
    hop_inner(b, to);
    *b.legs.entry("field hop").or_default() += b.real_s - t0;
}

fn hop_inner(b: &mut Bot, to: DVec3) {
    let d = (to - b.w.ship.rel).length();
    b.w.ship.rel = to;
    b.w.ship.vel = DVec3::ZERO;
    b.w.ship.throttle = 0.0;
    b.idle(field_hop_s(d));
}

/// Hold the laser on a rock, tracking its veins as it spins, until it
/// fractures (loose chunks appear) or 90 s pass.
fn cut(b: &mut Bot, field: FieldId, id: crate::mining::RockId, until: f64) {
    let t0 = b.real_s;
    cut_inner(b, field, id, until);
    *b.legs.entry("field cut").or_default() += b.real_s - t0;
}

fn cut_inner(b: &mut Bot, field: FieldId, id: crate::mining::RockId, until: f64) {
    let started = b.real_s;
    let nuggets = |b: &Bot| b.w.mining.field(field).map_or(0, |s| s.nuggets.len());
    let before = nuggets(b);
    while b.real_s - started < 90.0 && b.real_s < until {
        let (_, rocks) = b.w.field_rocks.clone().unwrap();
        let Some(rock) =
            b.w.mining
                .live_rocks(field, &b.w.galaxy.fields[field], &rocks, b.w.clock_s)
                .find(|r| r.id == id)
        else {
            break;
        };
        let nose = b.w.ship.rel;
        // A vein on the near side if there is one, else the middle.
        let aim = rock
            .vein_points()
            .filter(|p| (*p - rock.pos).dot(nose - rock.pos) > 0.0)
            .min_by(|a, c| (*a - nose).length().total_cmp(&(*c - nose).length()))
            .unwrap_or(rock.pos);
        b.w.ship.rot = look_rotation(aim - nose);
        b.w.ship.vel = DVec3::ZERO;
        // Keep a steady stand-off as fragments drift.
        let off = nose - rock.pos;
        let want = rock.radius * 1.3 + 40.0;
        if (off.length() - want).abs() > 30.0 {
            b.w.ship.rel = rock.pos + off.normalize_or(DVec3::Z) * want;
        }
        // A pilot lets a locked laser vent rather than hold the trigger.
        let fire = !b.w.mining.locked;
        b.frame(Controls {
            fire,
            ..Default::default()
        });
        if nuggets(b) > before {
            break;
        }
    }
}

/// An hour of mining: mine a hold in the nearest field, sell it at the
/// best price in the system, repeat.
fn mining_hour(seed: u64) {
    let mut b = Bot::new(seed);
    let start = b.worth();
    let mut mined = 0.0;
    let mut in_field = 0.0;
    // A miner sets up where there's a field without raiders (the nav list
    // marks them) and a station to sell at (a city costs minutes of flying
    // down its well and back out each trip).
    let systems: Vec<_> = b.w.galaxy.systems.iter().map(|(id, _)| id).collect();
    let quiet = |w: &World, _sys, f| retrox_sim::pirates::gang_at(&w.galaxy, f).is_none();
    let home = systems
        .iter()
        .copied()
        .find(|&id| {
            let v = SystemView::new(&b.w.galaxy, id);
            !v.stations.is_empty() && v.fields.iter().any(|&f| quiet(&b.w, id, f))
        })
        .or_else(|| {
            systems.iter().copied().find(|&id| {
                let v = SystemView::new(&b.w.galaxy, id);
                !v.fields.is_empty() && !v.cities.is_empty()
            })
        })
        .expect("a system with a field and a port");
    let view = SystemView::new(&b.w.galaxy, home);
    let port = view
        .stations
        .first()
        .copied()
        .unwrap_or_else(|| view.cities[0].0);
    b.go(port);
    let begin = b.real_s;
    while b.real_s - begin < HOUR {
        let quiet_field = |w: &World| w.view.fields.iter().copied().find(|&f| !w.gang_reported(f));
        // Every field has a band out: sit it out docked, a day at a time.
        let mut waited = 0;
        while quiet_field(&b.w).is_none() && waited < 60 {
            b.w.act(Action::Wait(1));
            while b.w.waiting.is_some() {
                b.frame(Controls::default());
            }
            waited += 1;
        }
        let field = quiet_field(&b.w).unwrap_or(b.w.view.fields[0]);
        b.leave();
        b.cruise(Place::Field(field));
        b.approach(
            Place::Field(field),
            b.w.place_radius(Place::Field(field)) * 0.5,
        );
        let t0 = b.real_s;
        mined += mine_hold(&mut b, begin + HOUR);
        in_field += b.real_s - t0;
        // Sell where it pays best in this system.
        let best = if b.w.view.stations.is_empty() {
            port
        } else {
            *b.w.view
                .stations
                .iter()
                .max_by(|x, y| {
                    let p = |s: SettlementId| {
                        let st = &b.w.galaxy.settlements[s];
                        crate::trade::sell_value(&st.economy.market, model(&b.w, s), Good::Ore, 1.0)
                    };
                    p(**x).total_cmp(&p(**y))
                })
                .unwrap()
        };
        b.go(best);
        b.sell_all();
    }
    let hours = (b.real_s - begin) / HOUR;
    println!(
        "mining  seed {seed}: {mined:.0} t in {:.0} min ({:.0} in the field), earned {:.0} cr/h",
        (b.real_s - begin) / 60.0,
        in_field / 60.0,
        (b.worth() - start) / hours
    );
    println!(
        "    legs (min): {:?}",
        b.legs
            .iter()
            .map(|(k, v)| (*k, (v / 60.0 * 10.0).round() / 10.0))
            .collect::<Vec<_>>()
    );
}

// --------------------------------------------------------------- trading

/// The most populous other port within a few jumps: where a captain
/// with nothing to do here goes looking for work.
fn elsewhere(w: &World, here: SettlementId, visited: &[SettlementId]) -> SettlementId {
    (1..=6)
        .find_map(|j| {
            ports_near(w, j)
                .into_iter()
                .filter(|&s| s != here && !visited.contains(&s))
                .max_by(|a, c| {
                    let p = |s: &SettlementId| w.galaxy.settlements[*s].economy.population;
                    p(a).total_cmp(&p(c))
                })
        })
        .expect("somewhere to go")
}

/// Settlements within `jumps` of a system, that a ship can dock at.
fn ports_near(w: &World, jumps: usize) -> Vec<SettlementId> {
    w.galaxy
        .settlements
        .iter()
        .filter(|(_, s)| !matches!(s.site, Site::Surface(_)))
        .filter(|(_, s)| {
            s.system == w.system || w.route(s.system).is_some_and(|r| r.len() <= jumps + 1)
        })
        .map(|(id, _)| id)
        .collect()
}

fn model(w: &World, s: SettlementId) -> retrox_sim::EconomyModel {
    w.galaxy.factions[w.galaxy.settlements[s].owner].economy
}

/// An hour of trading: buy what pays best within two jumps, carry it,
/// sell, repeat. Runs are ranked by profit per real minute.
fn trading_hour(seed: u64) {
    let mut b = Bot::new(seed);
    let start = b.worth();
    let begin = b.real_s;
    let cap = b.w.stats().cargo_t;
    let mut visited: Vec<SettlementId> = Vec::new();
    while b.real_s - begin < HOUR {
        let here = b.w.docked_at().expect("docked");
        let ports = ports_near(&b.w, 2);
        let mut best: Option<(f64, Good, SettlementId, f64)> = None;
        let src = &b.w.galaxy.settlements[here].economy;
        for good in Good::ALL {
            let n = cap.min(src.market.stock[good].floor()).min(
                (b.w.player.credits
                    / src
                        .market
                        .price_at(good, src.market.stock[good], model(&b.w, here))
                        .max(1.0))
                .floor(),
            );
            if n < 1.0 {
                continue;
            }
            let cost = crate::trade::buy_cost(&src.market, model(&b.w, here), good, n);
            for &to in &ports {
                if to == here {
                    continue;
                }
                let dst = &b.w.galaxy.settlements[to].economy;
                let value = crate::trade::sell_value(&dst.market, model(&b.w, to), good, n)
                    .min(dst.treasury);
                let jumps =
                    b.w.route(b.w.galaxy.settlements[to].system)
                        .map_or(9, |r| r.len() - 1);
                // Rough real minutes for the trip: a minute a system, a
                // jump's sequence, docking.
                let minutes = 1.5 + 1.0 * jumps as f64;
                let rate = (value - cost) / minutes;
                if best.is_none_or(|b| rate > b.0) {
                    best = Some((rate, good, to, n));
                }
            }
        }
        match best {
            Some((rate, good, to, n)) if rate > 0.0 => {
                let c0 = b.w.player.credits;
                b.w.act(Action::Trade(good, n));
                if std::env::var("BENCH_DEBUG").is_ok() {
                    println!(
                        "    buy {n:.0} t {} for {:.0} ({:?}), est {rate:.0}/min",
                        good.name(),
                        c0 - b.w.player.credits,
                        b.w.message
                    );
                }
                b.go(to);
                b.sell_all();
            }
            _ => {
                if std::env::var("BENCH_DEBUG").is_ok() {
                    println!(
                        "    nothing pays at {here:?}; credits {:.0}",
                        b.w.player.credits
                    );
                }
                // Nothing pays from here. Planned economies hold one price
                // everywhere, so head for the nearest market economy (the
                // map shows whose space is whose), else the biggest port.
                let market =
                    b.w.galaxy
                        .settlements
                        .iter()
                        .filter(|(id, s)| {
                            !matches!(s.site, Site::Surface(_))
                                && *id != here
                                && !visited.contains(id)
                                && model(&b.w, *id) == retrox_sim::EconomyModel::Market
                        })
                        .filter_map(|(id, s)| Some((b.w.route(s.system)?.len(), id)))
                        .min_by_key(|&(jumps, _)| jumps)
                        .map(|(_, id)| id);
                let to = market.unwrap_or_else(|| elsewhere(&b.w, here, &visited));
                visited.push(to);
                b.go(to);
            }
        }
    }
    let hours = (b.real_s - begin) / HOUR;
    let rate = (b.worth() - start) / hours;
    println!(
        "trading seed {seed}: {:.0} min, earned {rate:.0} cr/h",
        (b.real_s - begin) / 60.0
    );
}

// ------------------------------------------------------------- contracts

/// An hour of freight contracts: take the best-paying one on the board
/// per expected day, deliver it, repeat; wait a day when the board is bare.
fn contracts_hour(seed: u64) {
    let mut b = Bot::new(seed);
    let start = b.worth();
    let begin = b.real_s;
    let mut done = 0;
    let mut idle_days = 0;
    let mut visited: Vec<SettlementId> = Vec::new();
    while b.real_s - begin < HOUR {
        let here = b.w.docked_at().expect("docked");
        // Pay per real minute: about a minute per system and per jump.
        let pick =
            b.w.board
                .iter()
                .enumerate()
                .filter(|(_, o)| o.value <= b.w.player.credits && b.place_of(o.to).is_some())
                .max_by(|(_, a), (_, c)| {
                    let r = |o: &Offer| o.fee / (1.5 + o.jumps as f64);
                    r(a).total_cmp(&r(c))
                })
                .map(|(i, o)| (i, o.to));
        if let Some((i, to)) = pick {
            b.w.act(Action::Open(Screen::Contracts));
            let o = b.w.board[i].clone();
            let (t0, d0) = (b.real_s, b.w.galaxy.time.days());
            b.w.act(Action::AcceptContract(i));
            b.go(to);
            if std::env::var("BENCH_DEBUG").is_ok() {
                println!(
                    "    {:>3.0} t {:10} {} jumps, {:>4.1} days: fee {:>5.0} (carriage) on goods worth {:>4.0}; {:.1} min, {} days",
                    o.amount,
                    o.good.name(),
                    o.jumps,
                    o.trip_days,
                    o.fee,
                    o.value,
                    (b.real_s - t0) / 60.0,
                    b.w.galaxy.time.days() - d0
                );
            }
            done += 1;
            idle_days = 0;
        } else if idle_days < 3 {
            b.w.act(Action::Wait(1));
            while b.w.waiting.is_some() {
                b.frame(Controls::default());
            }
            idle_days += 1;
        } else {
            let to = elsewhere(&b.w, here, &visited);
            visited.push(to);
            b.go(to);
            idle_days = 0;
        }
    }
    let hours = (b.real_s - begin) / HOUR;
    let rate = (b.worth() - start) / hours;
    println!(
        "freight seed {seed}: {done} contracts in {:.0} min, {:.0} galaxy days, earned {rate:.0} cr/h",
        (b.real_s - begin) / 60.0,
        b.w.galaxy.time.days()
    );
}

#[test]
#[ignore = "slow: plays an hour of each career; run with --ignored --nocapture"]
fn careers() {
    use retrox_content::ships::{price, stock};
    let ladder: Vec<(&str, f64)> = retrox_content::ships::STOCK
        .iter()
        .map(|s| (s.hull.id, price::loadout_price(&stock(s.hull.id).unwrap())))
        .collect();
    println!(
        "start {:.0} cr; ships: {ladder:?}",
        hangar::STARTING_CREDITS
    );
    // See the module docs for the environment variables.
    let pick = std::env::var("CAREERS").unwrap_or_else(|_| "mining,trading,freight".into());
    let seeds: Vec<u64> = std::env::var("SEEDS")
        .unwrap_or_else(|_| "1,2,3,4".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    for seed in seeds {
        if pick.contains("mining") {
            mining_hour(seed);
        }
        if pick.contains("trading") {
            trading_hour(seed);
        }
        if pick.contains("freight") {
            contracts_hour(seed);
        }
    }
}
