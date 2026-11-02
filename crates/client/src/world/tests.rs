use super::pilot::{CRUISE_SPEED, throttle_speed};
use super::*;

fn run(world: &mut World, seconds: f64) {
    let dt = 1.0 / 60.0;
    for _ in 0..(seconds / dt) as usize {
        world.update(dt, Controls::default());
    }
}

/// Somewhere outside every gravity well.
fn open_space(world: &mut World) {
    world.ship.frame = Place::Star;
    world.ship.rel = DVec3::new(0.0, 5.0 * AU, 0.0);
    world.mode = Mode::Normal;
    assert_eq!(world.current_well(), None);
}

#[test]
fn throttle_curve() {
    assert_eq!(throttle_speed(0.0), 0.0);
    assert!((throttle_speed(0.5) - COMBAT_SPEED).abs() < 1e-9);
    assert!((throttle_speed(1.0) - CRUISE_SPEED).abs() < 1e-6);
}

#[test]
fn starts_landed_at_a_capital_city() {
    let world = World::new(1);
    let Mode::Landed { city, .. } = world.mode else {
        panic!("should start landed");
    };
    assert!(world.galaxy.factions.values().any(|f| f.capital == city));
}

#[test]
fn can_leave_a_planets_well_after_launch() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    let well = world
        .current_well()
        .expect("launch puts us in orbit, inside the well");
    let away = world.ship_position() - world.position(well);
    world.ship.rot = look_rotation(away);
    world.ship.throttle = 1.0;
    run(&mut world, 180.0);
    assert_eq!(
        world.current_well(),
        None,
        "still in a well after 3 minutes"
    );
}

#[test]
fn fast_travel_reaches_every_place_and_only_climbs_out_of_a_well() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    let well = world.current_well().expect("launched into a well");
    world.target = Some(Place::Star);
    world.act(Action::FastTravel);
    let Mode::FastTravel(trip) = world.mode else {
        panic!("[J] in a well climbs out");
    };
    assert_eq!(
        trip.leg,
        Leg::Climb {
            well,
            toward: Some(Place::Star)
        },
        "fast travel out of a well stops at its edge"
    );

    for target in world.nav_places().into_iter().filter(|p| *p != Place::Star) {
        open_space(&mut world);
        world.target = Some(target);
        let before = world.clock_s;
        world.act(Action::FastTravel);
        assert!(matches!(world.mode, Mode::FastTravel(_)), "{target:?}");
        // How far off the nose the place was on the trip's last frame: the
        // turn-over must be done by then, not snapped at the hand-back.
        let mut last_off = 180.0_f64;
        for _ in 0..30 * 60 {
            if world.mode == Mode::Normal {
                break;
            }
            let nose = world.ship.rot * DVec3::NEG_Z;
            let face = world.arrival_facing(Leg::To(target), world.ship_position(), world.t_days());
            last_off = nose
                .dot(face.normalize())
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            world.update(1.0 / 60.0, Controls::default());
        }
        assert_eq!(world.mode, Mode::Normal, "{target:?}");
        assert!(
            last_off < 10.0,
            "{}: still {last_off:.0}° off just before arriving",
            world.name(target)
        );
        let (d, expected) = (world.distance_to(target), world.arrival_distance(target));
        assert!(
            (d - expected).abs() < expected * 0.05 + 1.0,
            "{}: arrived {d:.3e} m away, expected {expected:.3e}",
            world.name(target)
        );
        assert!(world.clock_s > before, "galaxy time must pass");
        // Arrives nose-on (#118), not braking tail-first.
        let nose = world.ship.rot * DVec3::NEG_Z;
        let to = (world.position(target) - world.ship_position()).normalize();
        assert!(
            nose.dot(to) > 0.99,
            "{}: arrived facing {:.0}° off",
            world.name(target),
            nose.dot(to).clamp(-1.0, 1.0).acos().to_degrees()
        );
    }
}

#[test]
fn buy_and_refit_a_ship_at_the_starting_yard() {
    let mut world = World::new(1);
    // The starting purse doesn't stretch to a ship.
    world.player.credits = 100_000.0;
    let here = world.docked_at().expect("starts landed");
    assert!(hangar::services(&world.galaxy.settlements[here].economy).shipyard);
    // The yard builds hull over time; the cheapest ship is soon in stock.
    let cheapest = hangar::lot().remove(0);
    for _ in 0..60 {
        if hangar::can_build(&world.galaxy.settlements[here].economy, &cheapest) {
            break;
        }
        world.wait_days(1);
    }
    let credits = world.player.credits;
    world.act(Action::Hangar(Job::BuyShip(cheapest.hull)));
    assert_eq!(world.player.ship.fit.hull().id, cheapest.hull.id);
    assert_eq!(world.player.credits, credits - cheapest.price);
    assert_eq!(world.player.stored.len(), 1);

    // Strip the engine: the ship can't leave until one is fitted again.
    let (mount, slot) = engine_slot(&world);
    let engine = hangar::module_in(&world.player.ship.fit, mount, slot).unwrap();
    world.act(Action::Hangar(Job::Strip { mount, slot }));
    world.act(Action::Launch);
    assert!(matches!(world.mode, Mode::Landed { .. }));
    world.act(Action::Hangar(Job::Fit {
        mount,
        slot,
        module: engine,
    }));
    world.act(Action::Launch);
    assert_eq!(world.mode, Mode::Normal);
}

#[test]
fn trading_from_the_station_menu() {
    let mut world = World::new(1);
    assert_eq!(world.screen, Screen::Hub);
    world.act(Action::Open(Screen::Trade));
    assert_eq!(world.screen, Screen::Trade);
    let credits = world.player.credits;
    world.act(Action::Trade(Good::Food, 5.0));
    assert_eq!(world.player.cargo[Good::Food], 5.0);
    assert!(world.player.credits < credits);
    // More than the hold takes is refused.
    world.act(Action::Trade(Good::Food, 1000.0));
    assert_eq!(world.player.cargo[Good::Food], 5.0);
    world.act(Action::Open(Screen::Hub));
    world.act(Action::Launch);
    // Screens only open while docked or landed.
    world.act(Action::Open(Screen::Trade));
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!(world.docked_at(), None);
}

#[test]
fn handling_follows_the_fit() {
    let mut world = World::new(1);
    assert_eq!(world.handling(), (1.0, 1.0));
    world.player.ship.fit = retrox_content::ships::stock("bulk-freighter").unwrap();
    let (thrust, turn) = world.handling();
    assert!(thrust < 1.0 && turn < 1.0, "{thrust} {turn}");
}

#[test]
fn mine_a_rock_and_sell_the_ore() {
    let mut world = World::new(1);
    assert!(
        world.stats().mining_cut > 0.0,
        "the courier comes with a laser"
    );
    assert!(world.start_at_field());
    world.act(Action::Scan);
    let (field, _) = world.field_rocks.clone().expect("in a field");
    let mine = Controls {
        fire: true,
        ..Default::default()
    };
    let dt = 1.0 / 60.0;
    // Fire until something fractures, letting the laser vent.
    let mut frames = 0;
    while world
        .mining
        .field(field)
        .is_none_or(|s| s.nuggets.is_empty())
    {
        frames += 1;
        assert!(frames < 60 * 180, "nothing fractured in three minutes");
        world.update(dt, mine);
    }
    // Sweep up: park by each loose chunk in turn and let the tractor
    // pull it in.
    for _ in 0..200 {
        let Some(next) = world
            .mining
            .field(field)
            .and_then(|s| s.nuggets.first())
            .map(|n| n.pos)
        else {
            break;
        };
        world.ship.frame = Place::Field(field);
        world.ship.rel = next + DVec3::Y * 20.0;
        world.ship.vel = DVec3::ZERO;
        run(&mut world, 3.0);
    }
    let mined = world.player.cargo[Good::Ore] + world.player.cargo[Good::Ice];
    assert!(mined > 0.1, "hold has {mined} t");
    assert!(world.player.cargo_t() <= world.stats().cargo_t + 1e-9);

    // Sell it at a station.
    world.start_at_station();
    let credits = world.player.credits;
    for good in [Good::Ore, Good::Ice] {
        let t = world.player.cargo[good].floor();
        if t >= 1.0 {
            world.act(Action::Trade(good, -t));
        }
    }
    if mined >= 1.0 {
        assert!(world.player.credits > credits);
    }
}

fn engine_slot(world: &World) -> (usize, usize) {
    let fit = &world.player.ship.fit;
    fit.mounts()
        .enumerate()
        .find_map(|(m, (_, section))| {
            let s = section?.slots().position(|(slot, module)| {
                slot.kind == retrox_sim::ship::SlotKind::Engine && module.is_some()
            })?;
            Some((m, s))
        })
        .expect("an engine")
}

#[test]
fn fast_travel_is_a_real_flight() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    for target in world.nav_places().into_iter().filter(|p| *p != Place::Star) {
        open_space(&mut world);
        world.target = Some(target);
        world.act(Action::FastTravel);
        let name = world.name(target);
        let dt = 1.0 / 60.0;
        for frame in 0.. {
            assert!(frame < 60 * 60, "{name}: still flying after a minute");
            let (pos, vel, clock) = (world.ship_position(), world.ship.vel, world.clock_s);
            world.update(dt, Controls::default());
            // Moved only as far as its speed allows in the galaxy time
            // that passed: no teleporting.
            let moved = (world.ship_position() - pos).length();
            let flown = world.clock_s - clock;
            let reach = vel.length().max(world.ship.vel.length()) * flown;
            if matches!(world.mode, Mode::FastTravel(_)) {
                assert!(
                    moved <= reach * 1.01 + 1.0,
                    "{name}: jumped {moved:.3e} m in a {flown:.0} s step"
                );
            } else {
                break;
            }
        }
        // At rest next to the target, carrying on in its frame.
        assert_eq!(world.ship.frame, target);
        assert!(
            world.ship.vel.length() <= retrox_sim::flight::ARRIVE_SPEED + 1.0,
            "{name}"
        );
    }
}

#[test]
fn dropping_out_keeps_position_and_speed() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    open_space(&mut world);
    let target = world.view.bodies[0];
    world.target = Some(Place::Body(target));
    world.act(Action::FastTravel);
    run(&mut world, 1.0);
    let (pos, vel) = (world.ship_position(), world.ship.vel);
    assert!(
        vel.length() > COMBAT_SPEED,
        "should be flying fast mid-trip"
    );
    world.act(Action::FastTravel);
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!((world.ship_position(), world.ship.vel), (pos, vel));
}

#[test]
fn fast_travel_from_a_fast_drop_out_still_takes_seconds() {
    // Dropping out of a trip leaves the ship near its target at
    // thousands of km/s. Going again must still play out in seconds,
    // braking included, not crawl at the warp a short hop would get.
    let mut world = World::new(1);
    world.act(Action::Launch);
    let field = world
        .nav_places()
        .into_iter()
        .find(|p| matches!(p, Place::Field(_)))
        .expect("a field");
    world.ship.frame = field;
    world.ship.rel = DVec3::X * 1e6;
    world.ship.vel = DVec3::new(0.7, 0.7, 0.0) * 1e6;
    world.mode = Mode::Normal;
    world.target = Some(field);
    world.act(Action::FastTravel);
    assert!(matches!(world.mode, Mode::FastTravel(_)));
    run(&mut world, 10.0);
    assert_eq!(world.mode, Mode::Normal, "still flying after 10 s");
    assert_eq!(world.ship.frame, field);
}

#[test]
fn jumping_needs_open_space_and_takes_days() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    let lane = world.lanes()[0].clone();
    world.act(Action::Jump(lane.to));
    assert_eq!(world.mode, Mode::Normal, "can't jump inside a well");

    open_space(&mut world);
    let before = world.date();
    world.act(Action::Jump(lane.to));
    run(&mut world, 5.0);
    assert_eq!(world.system, lane.to);
    assert_eq!(world.current_well(), None, "arrive outside the star's well");
    assert!(world.date().days() >= before.days() + lane.days as u64);
}

#[test]
fn a_plotted_course_is_followed_hop_by_hop() {
    let mut world = World::new(1);
    // Somewhere a few jumps away.
    let far = world
        .galaxy
        .systems
        .iter()
        .map(|(s, _)| s)
        .filter_map(|s| world.route(s).map(|r| (s, r)))
        .find(|(_, r)| r.len() >= 4)
        .map(|(s, _)| s)
        .expect("a system several jumps away");
    world.act(Action::PlotCourse(Some(far)));
    assert_eq!(world.course, Some(far));
    let route = world.route(far).unwrap();
    assert_eq!(route.first(), Some(&world.system));
    assert_eq!(route.last(), Some(&far));
    assert!(world.route_days(&route) > 0.0);

    world.act(Action::Launch);
    world.act(Action::ToggleMap);
    assert!(world.map_open);
    for &expected in &route[1..] {
        open_space(&mut world);
        let hop = world.next_hop().expect("course has a next hop");
        assert_eq!(hop, expected);
        world.act(Action::Jump(hop));
        assert!(!world.map_open, "jumping closes the map");
        run(&mut world, 5.0);
        assert_eq!(world.system, hop);
    }
    assert_eq!(world.course, None, "arriving clears the course");
    world.act(Action::PlotCourse(Some(world.system)));
    assert_eq!(world.course, None, "no course to where you are");
}

#[test]
fn the_starting_board_carries_real_shortages() {
    for seed in 1..=3 {
        let world = World::new(seed);
        let here = world.docked_at().unwrap();
        assert!(!world.board.is_empty(), "seed {seed}: empty board");
        let origin = &world.galaxy.settlements[here].economy.market;
        for o in &world.board {
            let dest = &world.galaxy.settlements[o.to].economy.market;
            let coming = world.galaxy.traffic.inbound(o.to, o.good);
            // The consignee is short even counting what's on the way,
            // and this market can spare the goods without going short.
            assert!(dest.stock[o.good] + coming + o.amount <= dest.target(o.good));
            assert!(origin.stock[o.good] - o.amount >= 0.75 * origin.target(o.good) - 1e-6);
            assert!(o.fee > 0.0 && o.deadline > world.date());
        }
    }
}

#[test]
fn take_a_contract_and_deliver_it_in_another_system() {
    let mut world = World::new(1);
    let i = world
        .board
        .iter()
        .position(|o| o.jumps > 0)
        .expect("a contract to another system");
    let offer = world.board[i].clone();
    let credits = world.player.credits;
    let before = world.galaxy.settlements[offer.to].economy.market.stock[offer.good];
    world.act(Action::Open(Screen::Contracts));
    world.act(Action::AcceptContract(i));
    assert_eq!(world.player.contracts.len(), 1);
    assert_eq!(world.player.credits, credits - offer.value);
    assert_eq!(world.player.contract_t(), offer.amount);

    // Fly it there, hop by hop.
    let dest = world.galaxy.settlements[offer.to].system;
    world.act(Action::Launch);
    world.act(Action::PlotCourse(Some(dest)));
    while let Some(hop) = world.next_hop() {
        world.arrive_jump(hop);
    }
    assert_eq!(world.system, dest);
    assert!(world.date() <= offer.deadline, "should arrive on time");
    let flow = world.galaxy.settlements[offer.to].economy.market.stock[offer.good];
    world.mode = match world.galaxy.settlements[offer.to].site {
        Site::Region(r) => Mode::Landed {
            city: offer.to,
            body: world.galaxy.regions[r].body,
            timer: 0.0,
        },
        _ => Mode::Docked(offer.to),
    };
    world.update(1.0 / 60.0, Controls::default());
    assert!(world.player.contracts.is_empty());
    assert_eq!(world.player.contract_t(), 0.0);
    assert!((world.player.credits - (credits + offer.fee)).abs() < 1e-6);
    let after = world.galaxy.settlements[offer.to].economy.market.stock[offer.good];
    assert!(
        (after - flow - offer.amount).abs() < 1e-6,
        "{before} {flow} {after}"
    );
}

#[test]
fn waiting_stops_when_a_contract_falls_due() {
    let mut world = World::new(1);
    let offer = world.board[0].clone();
    world.act(Action::AcceptContract(0));
    world.wait_days(1000);
    assert_eq!(world.date(), offer.deadline);
    assert_eq!(world.player.contracts.len(), 1);
    // Waiting on past the grace period fails it: the goods are ours.
    world.wait_days(1000);
    assert!(world.player.contracts.is_empty());
    assert_eq!(world.player.cargo[offer.good], offer.amount);
}

#[test]
fn a_wait_runs_over_frames_and_can_be_stopped() {
    let mut world = World::new(1);
    let start = world.date();
    world.act(Action::Wait(30));
    // Nothing passes until frames run: the action itself is instant.
    assert_eq!(world.date(), start);
    world.update(0.0, Controls::default());
    let waited = world.waiting.map_or(30, |w| w.waited);
    assert!(waited >= 1);
    assert_eq!(world.date().days(), start.days() + waited);
    world.act(Action::StopWaiting);
    assert_eq!(world.waiting, None);
    // Leaving ends a wait too.
    world.act(Action::Wait(30));
    world.act(Action::Launch);
    world.update(0.0, Controls::default());
    assert_eq!(world.waiting, None);
}

#[test]
fn a_jumps_days_pass_during_the_sequence() {
    let mut world = World::new(1);
    open_space(&mut world);
    let lane = world.lanes()[0].clone();
    let start = world.clock_s;
    world.act(Action::Jump(lane.to));
    assert!(matches!(world.mode, Mode::Jumping { .. }));
    run(&mut world, f64::from(JUMP_DURATION) / 2.0);
    let passed = (world.clock_s - start) / SECONDS_PER_DAY;
    assert!(
        passed > lane.days * 0.3 && passed < lane.days * 0.7,
        "{passed} of {} days half way",
        lane.days
    );
    run(&mut world, f64::from(JUMP_DURATION));
    assert_eq!(world.system, lane.to);
    let passed = (world.clock_s - start) / SECONDS_PER_DAY;
    // The jump's days exactly, plus the frames flown since arriving.
    assert!(
        (passed - lane.days).abs() < 0.01,
        "{passed} vs {}",
        lane.days
    );
}

#[test]
fn a_fresh_game_has_raiders_within_reach_and_they_come() {
    let mut world = World::new(1);
    let start = world.system;
    assert!(world.start_by_raiders(), "some field has raiders");
    let jumps = world.route(start).map_or(0, |r| r.len());
    assert!(
        jumps <= 3,
        "the nearest raiders are {jumps} jumps from the start"
    );
    world.update(0.02, Controls::default());
    assert!(!world.combat.raiders.is_empty(), "the band comes out");
    assert!(world.hostiles_near() > 0);
    assert!(
        crate::save::can_save(&world).is_err(),
        "no saving with raiders near"
    );
    world.act(Action::FastTravel);
    assert_eq!(world.mode, Mode::Normal, "no fast travel either");
    // Sitting still, the ship takes fire.
    for _ in 0..(60 * 30) {
        world.update(1.0 / 60.0, Controls::default());
    }
    assert!(
        world.player.ship.damage.total() > 0.0 || world.combat.lost.is_some(),
        "the raiders attacked"
    );
}

#[test]
fn docking_repairs_the_hull_for_a_price() {
    let mut world = World::new(1);
    world.start_at_station();
    world.player.ship.damage.sections = vec![100.0];
    let credits = world.player.credits;
    world.update(0.02, Controls::default());
    assert_eq!(world.player.ship.damage.total(), 0.0);
    assert!(world.player.credits < credits);
}

/// Real seconds until `done`, flying with hands off; panics past `limit`.
fn real_seconds_until(world: &mut World, limit: f64, done: impl Fn(&World) -> bool) -> f64 {
    let dt = 1.0 / 60.0;
    let mut t = 0.0;
    while !done(world) {
        assert!(t < limit, "still not there after {limit} s");
        world.update(dt, Controls::default());
        t += dt;
    }
    t
}

#[test]
fn a_planet_city_stop_takes_seconds_not_minutes() {
    // #95: flying in from the well's edge and back out at cruise speed
    // took about five real minutes a visit.
    let mut world = World::new(1);
    let Mode::Landed { body, .. } = world.mode else {
        panic!("starts landed");
    };
    let planet = Place::Body(body);

    // Launch, and climb out under the autopilot: a few seconds, and
    // then we're free to fast travel.
    world.act(Action::Launch);
    assert_eq!(world.prompts().climb, Some(planet));
    world.act(Action::FastTravel);
    let out = real_seconds_until(&mut world, 15.0, |w| w.mode == Mode::Normal);
    assert_eq!(
        world.current_well(),
        None,
        "clear of the well after the climb"
    );
    assert!(
        world.distance_to(planet) < world.well_radius(planet) * 1.1,
        "stops at the edge, not beyond"
    );
    let clear = (world.ship.vel.length(), world.ship.frame);
    assert!(clear.0 <= retrox_sim::flight::ARRIVE_SPEED + 1.0);
    assert_eq!(clear.1, planet);

    // Back in from open space: fast travel stops low enough to land.
    open_space(&mut world);
    world.target = Some(planet);
    world.act(Action::FastTravel);
    let back = real_seconds_until(&mut world, 15.0, |w| w.mode == Mode::Normal);
    assert!(world.prompts().land.is_some(), "can land on arrival");
    assert!(
        out + back < 20.0,
        "a planet stop took {out:.1} s out and {back:.1} s in"
    );
}

/// The highport above the starting city planet (#112).
fn highport(world: &World, body: BodyId) -> SettlementId {
    world
        .view
        .stations
        .iter()
        .copied()
        .find(|&s| {
            matches!(world.galaxy.settlements[s].site,
                Site::Space(o) if o.parent == retrox_sim::Orbiting::Body(body))
        })
        .expect("a highport above the capital")
}

#[test]
fn a_highport_is_a_hop_from_the_city_below() {
    // DESIGN §7.1.1: inside a well the autopilot flies down to anything
    // whose arrival point is in the same well. Every city planet has a
    // station above it, so this happens in play: undock, hop down, land;
    // launch, hop up. No climb out and back either way.
    let mut world = World::new(1);
    let Mode::Landed { body, .. } = world.mode else {
        panic!("starts landed");
    };
    let planet = Place::Body(body);
    let port = highport(&world, body);
    assert!(world.arrives_within(Place::Station(port), planet));

    // Undock at the highport: still inside the planet's well.
    world.mode = Mode::Docked(port);
    world.ship.frame = Place::Station(port);
    world.ship.rel = DVec3::ZERO;
    world.act(Action::Undock);
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!(
        world.current_well(),
        Some(planet),
        "undocked inside the well"
    );

    // Down to the city.
    world.target = Some(planet);
    assert_eq!(world.prompts().fast_travel, Some(planet));
    world.act(Action::FastTravel);
    let Mode::FastTravel(trip) = world.mode else {
        panic!("[J] flies down");
    };
    assert_eq!(trip.leg, Leg::To(planet), "no climb out first");
    let down = real_seconds_until(&mut world, 15.0, |w| w.mode == Mode::Normal);
    assert!(world.prompts().land.is_some(), "can land on arrival");

    // Land, launch, and back up to the port.
    world.act(Action::Land);
    while matches!(world.mode, Mode::Landed { timer, .. } if timer > 0.0) {
        world.update(1.0 / 60.0, Controls::default());
    }
    assert!(matches!(world.mode, Mode::Landed { .. }), "landed");
    world.act(Action::Launch);
    world.target = Some(Place::Station(port));
    assert_eq!(world.prompts().fast_travel, Some(Place::Station(port)));
    world.act(Action::FastTravel);
    let Mode::FastTravel(trip) = world.mode else {
        panic!("[J] flies up");
    };
    assert_eq!(
        trip.leg,
        Leg::To(Place::Station(port)),
        "no climb out first"
    );
    let up = real_seconds_until(&mut world, 15.0, |w| w.mode == Mode::Normal);
    let station = Place::Station(port);
    assert!(
        world.distance_to(station) < world.arrival_distance(station) * 1.2,
        "stopped at the port's arrival point"
    );
    assert_eq!(
        world.prompts().dock,
        Some(port),
        "arrives in reach of [F] dock"
    );
    assert_eq!(world.current_well(), Some(planet), "never left the well");
    assert!(down + up < 20.0, "{down:.1} s down, {up:.1} s up");
}

#[test]
fn climbing_out_is_a_real_flight_toward_the_next_target() {
    let mut world = World::new(1);
    let Mode::Landed { body, .. } = world.mode else {
        panic!("starts landed");
    };
    let planet = Place::Body(body);
    world.act(Action::Launch);
    world.target = Some(Place::Star);
    world.act(Action::FastTravel);
    let dt = 1.0 / 60.0;
    while matches!(world.mode, Mode::FastTravel(_)) {
        let (pos, vel, clock) = (world.ship_position(), world.ship.vel, world.clock_s);
        world.update(dt, Controls::default());
        let moved = (world.ship_position() - pos).length();
        let reach = vel.length().max(world.ship.vel.length()) * (world.clock_s - clock);
        if matches!(world.mode, Mode::FastTravel(_)) {
            assert!(moved <= reach * 1.01 + 1.0, "jumped {moved:.3e} m");
        }
    }
    // Out on the star's side of the planet, ready for the next leg.
    let out = (world.ship_position() - world.position(planet)).normalize();
    let sunward = (-world.position(planet)).normalize();
    assert!(out.dot(sunward) > 0.95, "came out facing the star");
    assert_eq!(world.prompts().fast_travel, Some(Place::Star));
}

/// A gang in a system of its own, at home, with the player arriving there
/// (DESIGN §7.2.2).
fn by_a_gang(world: &mut World) -> (retrox_sim::pirates::GangId, FieldId) {
    let (id, lair, field) = world
        .galaxy
        .pirates
        .gangs
        .iter()
        .map(|(id, g)| (id, g.lair, g.field))
        .next()
        .expect("seed 1 has gangs");
    if world.system != lair {
        world.arrive_jump(lair);
    }
    let today = world.galaxy.time;
    let p = world
        .galaxy
        .pirates
        .prowls
        .get_mut(&id)
        .expect("a gang in the bubble flies");
    p.aim = retrox_sim::pirates::Aim::Lurk;
    p.since = today;
    (id, field)
}

#[test]
fn pirates_pull_a_ship_out_of_fast_travel() {
    let mut world = World::new(1);
    let (id, field) = by_a_gang(&mut world);
    // A third of an AU out from their field, heading for it.
    let centre = world.position(Place::Field(field));
    world.mode = Mode::Normal;
    world.ship.frame = Place::Star;
    world.ship.rel = centre + DVec3::new(0.3 * AU, 0.02 * AU, 0.0) - world.position(Place::Star);
    world.ship.vel = DVec3::ZERO;
    if world.current_well().is_some() {
        world.ship.rel = centre + DVec3::new(-0.3 * AU, 0.02 * AU, 0.0);
    }
    world.target = Some(Place::Field(field));
    world.update(1.0 / 60.0, Controls::default());
    world.act(Action::FastTravel);
    assert!(
        matches!(world.mode, Mode::FastTravel(_)),
        "{:?}",
        world.message
    );
    let mut frames = 0;
    while matches!(world.mode, Mode::FastTravel(_)) && frames < 60 * 20 {
        world.update(1.0 / 60.0, Controls::default());
        frames += 1;
    }
    assert!(
        world.interdicted.is_some(),
        "not interdicted: {:?}, {:?}",
        world.message,
        world.galaxy.pirates.prowls[&id]
    );
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!(world.combat.gang, Some(id));
    assert!(world.hostiles_near() > 0, "the gang comes in");
    assert!(
        world.ship.vel.length() < 1000.0,
        "dragged down to fighting speed"
    );
    assert_eq!(
        world.galaxy.pirates.prowls[&id].aim,
        retrox_sim::pirates::Aim::Engaged
    );
}

#[test]
fn a_bounty_pays_on_the_kill_and_the_sim_hears_of_it() {
    let mut world = World::new(1);
    assert!(world.start_by_raiders());
    world.update(0.02, Controls::default());
    let id = world.combat.gang.expect("the gang comes out");
    let faction = world.galaxy.factions.keys().next().unwrap();
    world.galaxy.pirates.gangs[id].bounty = Some(retrox_sim::pirates::Bounty {
        faction,
        per_ship: 3000.0,
        posted: world.galaxy.time,
    });
    let ships = world.galaxy.pirates.gangs[id].ships.len();
    let credits = world.player.credits;
    let pilot = crate::combat::Pilot {
        craft: world.player_craft(),
        burn: 0.0,
        trigger: false,
    };
    let first = world.combat.raiders[0].id;
    world.combat.hit_raider(first, 1e6, &pilot);
    world.update(0.02, Controls::default());
    let paid = world.player.credits - credits;
    assert!(paid >= 3000.0 - 1e-6, "paid {paid}");
    assert!(world.player.standing.get(faction) > 0.0);
    // The rest go down too: the gang is gone from the galaxy.
    for r in world
        .combat
        .raiders
        .iter()
        .map(|r| r.id)
        .collect::<Vec<_>>()
    {
        world.combat.hit_raider(r, 1e6, &pilot);
    }
    world.update(0.02, Controls::default());
    assert!(world.combat.gang.is_none(), "the fight is over");
    assert!(
        world.galaxy.pirates.gangs.get(id).is_none(),
        "all {ships} ships lost: the gang is broken"
    );
}

#[test]
fn shooting_a_traders_ships_makes_its_faction_hostile() {
    let mut world = World::new(1);
    assert!(world.start_by_traffic(), "seed 1 has traffic");
    // Alongside, as an intercept leaves us: flying in the convoy's frame.
    let key = world
        .traffic
        .contacts
        .iter()
        .max_by_key(|c| c.ships)
        .unwrap()
        .key;
    world.enter_frame(Place::Convoy(key));
    world.ship.flight_assist = true;
    world.update(1.0 / 60.0, Controls::default());
    let trader = world
        .combat
        .raiders
        .iter()
        .find(|r| r.trader_fleet().is_some())
        .map(|r| (r.id, r.crew))
        .expect("the fleet's ships are in the scene");
    let crate::combat::Crew::Trader { fleet, faction } = trader.1 else {
        unreachable!()
    };
    assert!(
        !world.combat.raiders.iter().any(|r| r.hostile),
        "peaceful at first"
    );
    assert_eq!(world.hostiles_near(), 0);
    let ships = world
        .galaxy
        .companies
        .get(fleet.0)
        .and_then(|c| c.fleets.iter().find(|f| f.id == fleet.1))
        .map_or(0, |f| f.ships);
    // A few bursts, seconds apart.
    for _ in 0..6 {
        let pilot = crate::combat::Pilot {
            craft: world.player_craft(),
            burn: 0.0,
            trigger: false,
        };
        if world.combat.raider(trader.0).is_some() {
            world.combat.hit_raider(trader.0, 1.0, &pilot);
        }
        run(&mut world, 1.6);
    }
    assert!(
        world.player.standing.hostile(faction),
        "{}",
        world.player.standing.get(faction)
    );
    // Destroyed, it's gone from the sim's fleet too.
    let pilot = crate::combat::Pilot {
        craft: world.player_craft(),
        burn: 0.0,
        trigger: false,
    };
    if world.combat.raider(trader.0).is_some() {
        world.combat.hit_raider(trader.0, 1e6, &pilot);
        world.update(1.0 / 60.0, Controls::default());
        let left = world
            .galaxy
            .companies
            .get(fleet.0)
            .and_then(|c| c.fleets.iter().find(|f| f.id == fleet.1))
            .map_or(0, |f| f.ships);
        assert_eq!(left + 1, ships, "one ship fewer");
    }
    // Its stations turn us away.
    let station = world
        .galaxy
        .settlements
        .iter()
        .find(|(_, s)| s.owner == faction)
        .map(|(id, _)| id)
        .expect("the faction has settlements");
    assert!(!world.cleared(station));
}

#[test]
fn intercept_brings_the_ship_alongside_a_convoy() {
    let mut world = World::new(1);
    assert!(world.start_by_traffic(), "seed 1 has traffic");
    let c = world
        .traffic
        .contacts
        .iter()
        .max_by_key(|c| c.ships)
        .unwrap()
        .clone();
    // Well off from it, at rest in the star's frame, looking at it.
    let off = c.pos + DVec3::new(0.0, 0.02 * AU, 0.01 * AU);
    world.ship.frame = Place::Star;
    world.ship.rel = off;
    world.ship.vel = DVec3::ZERO;
    world.ship.flight_assist = true;
    world.ship.rot = look_rotation(c.pos - off);
    world.cam_rot = world.ship.rot;
    world.update(1.0 / 60.0, Controls::default());
    world.act(Action::Intercept);
    assert!(
        matches!(world.mode, Mode::FastTravel(FastTravel { leg: Leg::Intercept(k), .. }) if k == c.key),
        "{:?}",
        world.message
    );
    let mut frames = 0;
    while matches!(world.mode, Mode::FastTravel(_)) && frames < 60 * 30 {
        world.update(1.0 / 60.0, Controls::default());
        frames += 1;
    }
    assert_eq!(world.mode, Mode::Normal, "{:?}", world.message);
    if world.ship.frame != Place::Convoy(c.key) {
        // The fleet reached port or a jump point first: fair, but rare.
        eprintln!("lost the convoy: {:?}", world.message);
        return;
    }
    run(&mut world, 1.0);
    assert!(world.combat.has_convoy(c.key), "its ships are alongside");
    let me = world.player_craft();
    let near = world
        .combat
        .raiders
        .iter()
        .filter(|r| r.trader_fleet() == Some(c.key))
        .map(|r| (r.craft.pos - me.pos).length())
        .fold(f64::INFINITY, f64::min);
    assert!(near < 5e3, "{near:.0} m off");
    // In its frame, flight assist holds us with it.
    let rel = world.ship.vel.length();
    assert!(rel < 200.0, "{rel:.0} m/s relative");
}

#[test]
fn skimming_a_gas_giant_fills_the_hold_with_gas() {
    use retrox_sim::harvest::{self, Source};
    let mut world = World::new(1);
    assert!(world.start_skimming(), "no gas giant to skim");
    let pass = world.skim_pass().expect("over a gas giant");
    let source = Source::Body(pass.giant);
    let strain = |w: &World| w.galaxy.strain.get(source, w.galaxy.time);
    let before = strain(&world);
    let scoop = Controls {
        fire: true,
        ..Controls::default()
    };
    let dt = 1.0 / 60.0;
    for _ in 0..(20.0 / dt) as usize {
        world.update(dt, scoop);
    }
    let gas = world.player.cargo[Good::Gas];
    assert!(gas > 0.5, "only {gas:.2} t of gas in 20 s");
    assert!(world.player.cargo_t() <= world.stats().cargo_t + 1e-9);
    assert!(strain(&world) > before, "the giant doesn't feel it");
    assert!(harvest::effective_richness(&world.galaxy, source) > 0.0);
}

#[test]
fn fast_travel_to_a_gas_giant_stops_over_its_clouds() {
    let world = World::new(1);
    let giant = world
        .galaxy
        .bodies
        .iter()
        .find(|(_, b)| b.kind == BodyKind::GasGiant)
        .map(|(id, _)| id)
        .expect("a gas giant");
    let place = Place::Body(giant);
    let above = world.arrival_distance(place) - world.place_radius(place);
    assert!(above < crate::skim::BAND_M * 2.0, "{above} m up");
    assert!(above > crate::skim::BAND_M, "arrives inside the band");
}

/// At the nearest gang's field, with the gang out and closing.
fn among_raiders() -> World {
    let mut world = World::new(1);
    assert!(world.start_by_raiders());
    world.update(0.02, Controls::default());
    assert!(world.hostiles_near() > 0, "the gang comes out");
    world
}

fn said(world: &World) -> &str {
    world.message.as_ref().map_or("", |m| m.0.as_str())
}

#[test]
fn with_hostiles_near_the_trigger_fires_the_guns_and_not_the_mining_laser() {
    let mut world = among_raiders();
    let fire = Controls {
        fire: true,
        ..Default::default()
    };
    let shells = world.combat.tally.fired;
    for _ in 0..30 {
        world.update(1.0 / 60.0, fire);
        assert!(!world.mining.firing, "the laser stands down");
        assert!(world.beam.is_none());
    }
    assert!(world.combat.tally.fired > shells, "the guns fire");

    // Once they're gone the trigger is the laser's again.
    world.combat.leave();
    assert_eq!(world.hostiles_near(), 0);
    let mut lasing = false;
    for _ in 0..30 {
        world.update(1.0 / 60.0, fire);
        lasing |= world.mining.firing;
    }
    assert!(lasing, "with nobody near, the trigger cuts rock");
}

#[test]
fn no_jump_with_hostiles_near() {
    // Hostiles are checked before anything else, gravity wells included.
    let mut world = among_raiders();
    let system = world.system;
    let lane = world.lanes()[0].clone();
    world.act(Action::Jump(lane.to));
    assert_eq!(said(&world), "Can't jump: hostiles within 20 km");
    run(&mut world, 3.0);
    assert_eq!(world.system, system, "the jump never started");
    assert!(world.jump_start_s.is_none());

    // With them gone, out in open space, the same jump goes.
    world.combat.leave();
    open_space(&mut world);
    world.act(Action::Jump(lane.to));
    assert!(world.jump_start_s.is_some(), "{}", said(&world));
}

#[test]
fn repairs_cost_three_credits_a_point_paid_to_the_settlement() {
    let mut world = World::new(1);
    world.start_at_station();
    let here = world.docked_at().expect("docked");
    let treasury = |w: &World| w.galaxy.settlements[here].economy.treasury;

    // Enough money: all of it, at 3 cr a point.
    world.player.ship.damage.sections = vec![100.0];
    world.player.credits = 1000.0;
    let before = treasury(&world);
    world.repair_docked();
    assert_eq!(world.player.ship.damage.total(), 0.0);
    assert!((world.player.credits - 700.0).abs() < 1e-9);
    assert!((treasury(&world) - before - 300.0).abs() < 1e-9);

    // Short of it: as much as the money buys, and the rest stays.
    world.player.ship.damage.sections = vec![100.0];
    world.player.credits = 120.0;
    let before = treasury(&world);
    world.repair_docked();
    assert!((world.player.ship.damage.total() - 60.0).abs() < 1e-9);
    assert_eq!(world.player.credits, 0.0);
    assert!((treasury(&world) - before - 120.0).abs() < 1e-9);
    assert!(said(&world).starts_with("Hull partly repaired"));

    // Broke: nothing changes hands.
    let before = treasury(&world);
    world.repair_docked();
    assert!((world.player.ship.damage.total() - 60.0).abs() < 1e-9);
    assert_eq!(treasury(&world), before);
}
