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
fn fast_travel_reaches_every_place_and_cannot_start_in_a_well() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    world.target = Some(Place::Star);
    world.act(Action::FastTravel);
    assert_eq!(
        world.mode,
        Mode::Normal,
        "fast travel must not start inside a well"
    );

    for target in world.nav_places().into_iter().filter(|p| *p != Place::Star) {
        open_space(&mut world);
        world.target = Some(target);
        let before = world.clock_s;
        world.act(Action::FastTravel);
        assert!(matches!(world.mode, Mode::FastTravel(_)), "{target:?}");
        run(&mut world, 30.0);
        assert_eq!(world.mode, Mode::Normal, "{target:?}");
        let (d, expected) = (world.distance_to(target), world.arrival_distance(target));
        assert!(
            (d - expected).abs() < expected * 0.05 + 1.0,
            "{}: arrived {d:.3e} m away, expected {expected:.3e}",
            world.name(target)
        );
        assert!(world.clock_s > before, "galaxy time must pass");
    }
}

#[test]
fn buy_and_refit_a_ship_at_the_starting_yard() {
    let mut world = World::new(1);
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
