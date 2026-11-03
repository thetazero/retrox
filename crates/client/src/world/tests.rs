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

/// Issue #165: mining is a first way in, so a fresh game starts where
/// there's a field to mine. Seed 8's biggest capital has none.
#[test]
fn starts_in_a_system_with_an_asteroid_field() {
    let world = World::new(8);
    assert!(!world.view.fields.is_empty());
    assert!(matches!(world.mining_spot(), Some(MiningSpot::Here(_))));
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
        real_seconds_until(&mut world, 30.0, |w| w.mode == Mode::Normal);
        // A field's trip stops by a rock at its edge (#122); anywhere
        // else at the place's arrival distance.
        let (offset, expected) = match (target, world.field_stop) {
            (Place::Field(f), Some((field, offset, standoff))) if f == field => (offset, standoff),
            (Place::Field(_), _) => panic!("{}: no rock to stop by", world.name(target)),
            _ => (DVec3::ZERO, world.arrival_distance(target)),
        };
        let stop = |w: &World| w.position(target) + offset;
        let d = (stop(&world) - world.ship_position()).length();
        assert!(
            (d - expected).abs() < expected * 0.05 + 1.0,
            "{}: arrived {d:.3e} m away, expected {expected:.3e}",
            world.name(target)
        );
        assert!(world.clock_s > before, "galaxy time must pass");
        // Then turns nose-on (#118) at the ship's own rates, rather than
        // sitting tail-first from the braking.
        let turn = real_seconds_until(&mut world, 8.0, |w| w.turn_over.is_none());
        let nose = world.ship.rot * DVec3::NEG_Z;
        let to = (stop(&world) - world.ship_position()).normalize();
        let off = nose.angle_between(to).to_degrees();
        assert!(
            off < 1.0,
            "{}: {off:.1}° off after {turn:.1} s",
            world.name(target)
        );
        assert!(
            turn > 0.5,
            "{}: a real turn takes time, not {turn:.2} s",
            world.name(target)
        );
    }
}

#[test]
fn the_camera_looks_ahead_while_the_ship_brakes_tail_first() {
    // #158: the camera turned round with the ship as it flipped to brake,
    // so half of every trip looked back where it came from.
    let mut world = World::new(1);
    world.act(Action::Launch);
    open_space(&mut world);
    world.reset_camera();
    let target = world
        .nav_places()
        .into_iter()
        .find(|p| matches!(p, Place::Body(_)))
        .expect("a planet to fly to");
    world.target = Some(target);
    world.act(Action::FastTravel);
    assert!(matches!(world.mode, Mode::FastTravel(_)));
    let dt = 1.0 / 60.0;
    let look = |w: &World| w.camera().1 * DVec3::NEG_Z;
    let mut last = look(&world);
    let (mut braking, mut worst_step, mut worst_off) = (0, 0.0_f64, 0.0_f64);
    let mut t = 0.0;
    while world.mode != Mode::Normal || world.turn_over.is_some() {
        assert!(t < 40.0, "trip never ended");
        world.update(dt, Controls::default());
        t += dt;
        let now = look(&world);
        // Past the swing round to the route at the start (which the ship
        // makes too, faster), the view never jumps.
        if t > 1.0 {
            worst_step = worst_step.max(now.angle_between(last));
        }
        last = now;
        let to = world.position(target) - world.ship_position();
        let nose = world.ship.rot * DVec3::NEG_Z;
        if matches!(world.mode, Mode::FastTravel(_)) && nose.dot(to) < 0.0 && t > 1.5 {
            braking += 1;
            worst_off = worst_off.max(now.angle_between(to).to_degrees());
        }
    }
    assert!(
        braking > 30,
        "the ship should spend a while braking tail-first"
    );
    assert!(
        worst_off < 10.0,
        "camera {worst_off:.1}° off the destination mid-brake"
    );
    // Smooth through the flip and the turn-over: no frame turns the view
    // by more than 1° (60°/s).
    assert!(
        worst_step.to_degrees() < 1.0,
        "the camera snapped {:.1}° in one frame",
        worst_step.to_degrees()
    );
    // Handed back: a couple of seconds on, it's on the nose again, and
    // flying by hand follows the ship as ever.
    for _ in 0..180 {
        world.update(dt, Controls::default());
    }
    assert!(
        world.travel_view < 0.02,
        "view still {:.2} turned",
        world.travel_view
    );
    let off = look(&world).angle_between(world.ship.rot * DVec3::NEG_Z);
    assert!(
        off.to_degrees() < 1.0,
        "chase camera {:.1}° off the nose",
        off.to_degrees()
    );
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

/// Issue #157: the engines burn through a fast-travel trip, going dark
/// while the ship turns over to brake and again once it's there.
#[test]
fn fast_travel_engines_burn_out_and_brake_and_go_dark() {
    let mut world = World::new(1);
    world.act(Action::Launch);
    open_space(&mut world);
    world.target = Some(Place::Body(world.view.bodies[0]));
    world.act(Action::FastTravel);
    let (mut lit, mut dark, mut braking_lit) = (0, 0, 0);
    let dt = 1.0 / 60.0;
    for frame in 0.. {
        assert!(frame < 60 * 60, "still flying after a minute");
        world.update(dt, Controls::default());
        let Mode::FastTravel(trip) = world.mode else {
            break;
        };
        let nose = world.ship.rot * DVec3::NEG_Z;
        if trip.burn > 0.5 {
            lit += 1;
            // Burning tail first: the nose against the way it's going.
            if nose.dot(DVec3::from_array(trip.ship.vel)) < 0.0 {
                braking_lit += 1;
            }
        } else if trip.burn < 0.02 {
            dark += 1;
        }
    }
    assert!(lit > 60, "engines lit for {lit} frames");
    assert!(braking_lit > 30, "lit braking for {braking_lit} frames");
    assert!(dark > 0, "never dark turning over");
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!(world.ship.throttle, 0.0, "dark on arrival");
}

#[test]
fn drive_burn_counts_only_thrust_along_the_nose() {
    let rot = look_rotation(DVec3::X);
    assert_eq!(pilot::drive_burn(DVec3::X * 2.0, rot, 2.0), 1.0);
    assert_eq!(pilot::drive_burn(DVec3::Y * 2.0, rot, 2.0), 0.0);
    assert_eq!(pilot::drive_burn(-DVec3::X * 2.0, rot, 2.0), 0.0);
    assert!((pilot::drive_burn(DVec3::X, rot, 2.0) - 0.5).abs() < 1e-6);
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

/// Issue #188: taking a contract sets its course, and from the launch
/// [J] alone gets the ship there: up out of the well, then lane by lane.
#[test]
fn a_contract_is_reached_with_j_alone() {
    let mut world = World::new(1);
    let i = world
        .board
        .iter()
        .position(|o| world.galaxy.settlements[o.to].system != world.system)
        .expect("a job in another system");
    let dest = world.galaxy.settlements[world.board[i].to].system;
    world.act(Action::AcceptContract(i));
    assert_eq!(world.course, Some(dest), "ACCEPT plots the course");

    world.act(Action::Launch);
    assert!(world.prompts().climb.is_some(), "launched inside a well");
    assert_eq!(world.prompts().jump, None, "no jumping from in the well");
    for _ in 0..20 {
        if world.system == dest {
            break;
        }
        world.act(Action::FastTravel);
        assert_ne!(world.mode, Mode::Normal, "[J] does something");
        for _ in 0..600 {
            run(&mut world, 1.0);
            if world.mode == Mode::Normal {
                break;
            }
        }
    }
    assert_eq!(world.system, dest);
    assert_eq!(world.course, None);
}

/// A course kept from before isn't replaced by taking a job.
#[test]
fn accepting_keeps_a_course_already_plotted() {
    let mut world = World::new(1);
    let i = world
        .board
        .iter()
        .position(|o| world.galaxy.settlements[o.to].system != world.system)
        .expect("a job in another system");
    let dest = world.galaxy.settlements[world.board[i].to].system;
    let other = world
        .lanes()
        .into_iter()
        .map(|l| l.to)
        .find(|&s| s != dest)
        .expect("another lane");
    world.act(Action::PlotCourse(Some(other)));
    world.act(Action::AcceptContract(i));
    assert_eq!(world.course, Some(other));
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
fn leaving_a_port_never_offers_to_fly_back_to_it() {
    // Issue #176: the place just left stays targeted, so [J] offered to fast
    // travel 1.4 km back to it. It moves on to the next stop, or clears.
    let mut world = World::new(1);
    let Mode::Landed { city, body, .. } = world.mode else {
        panic!("starts landed");
    };
    let planet = Place::Body(body);
    let port = highport(&world, body);
    let dock_at = |world: &mut World| {
        world.mode = Mode::Docked(port);
        world.ship.frame = Place::Station(port);
        world.ship.rel = DVec3::ZERO;
        world.target = Some(Place::Station(port));
    };

    // Nothing to do here: the target clears, and [J] climbs out.
    dock_at(&mut world);
    world.act(Action::Undock);
    assert_eq!(world.mode, Mode::Normal);
    assert_eq!(world.target, None);
    assert_eq!(world.prompts().fast_travel, None);
    assert_eq!(world.prompts().climb, Some(planet));

    // The hold is bound for the city below: that's the next stop.
    dock_at(&mut world);
    world.player.sell_at = Some(city);
    world.act(Action::Undock);
    assert_eq!(world.target, Some(planet));
    assert_eq!(world.landing, Some(city));
    assert_eq!(world.prompts().fast_travel, Some(planet));

    // A target the player picked somewhere else is kept.
    dock_at(&mut world);
    world.player.sell_at = None;
    let other = world.view.stations.iter().copied().find(|&s| s != port);
    if let Some(other) = other {
        world.target = Some(Place::Station(other));
        world.act(Action::Undock);
        assert_eq!(world.target, Some(Place::Station(other)));
    }

    // Launching from the city drops the planet just left.
    world.mode = Mode::Landed {
        city,
        body,
        timer: 0.0,
    };
    world.target = Some(planet);
    world.act(Action::Launch);
    assert_eq!(world.mode, Mode::Normal);
    assert_ne!(world.prompts().fast_travel, Some(planet));
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
    let before = world.clock_s;
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
    // The climb takes about as long as its distance says: the goal on the
    // well's edge swings round with the planet's orbit, and the autopilot
    // must match that, not chase it for days (issue #109).
    let climb = world.distance_to(planet);
    let planned = retrox_sim::flight::trip_seconds(climb, world.drive_accel());
    let took = world.clock_s - before;
    assert!(
        took < planned * 3.0,
        "climbed {climb:.3e} m in {took:.0} s of galaxy time, planned {planned:.0} s"
    );
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
fn a_hostile_convoy_left_behind_leaves_the_scene() {
    let mut world = World::new(1);
    assert!(world.start_by_traffic(), "seed 1 has traffic");
    let key = world
        .traffic
        .contacts
        .iter()
        .max_by_key(|c| c.ships)
        .unwrap()
        .key;
    let faction = world.fleet_faction(key).expect("a faction");
    world.player.standing.change(faction, -100.0);
    world.enter_frame(Place::Convoy(key));
    world.ship.flight_assist = true;
    world.update(1.0 / 60.0, Controls::default());
    assert!(
        world.combat.has_convoy(key),
        "its ships come into the scene"
    );
    assert!(world.combat.raiders.iter().any(|r| r.hostile), "and fight");
    // Far off, out of their reach: they fly on, and the sim's fleet is
    // drawn again.
    world.ship.rel += DVec3::new(0.0, 200e3, 0.0);
    world.update(1.0 / 60.0, Controls::default());
    assert!(
        !world.combat.has_convoy(key),
        "the convoy is still in the scene"
    );
}

#[test]
fn a_fight_stays_put_when_the_convoy_it_was_framed_in_is_lost() {
    let mut world = World::new(1);
    assert!(world.start_by_traffic(), "seed 1 has traffic");
    let key = world
        .traffic
        .contacts
        .iter()
        .max_by_key(|c| c.ships)
        .unwrap()
        .key;
    let faction = world.fleet_faction(key).expect("a faction");
    world.player.standing.change(faction, -100.0);
    world.enter_frame(Place::Convoy(key));
    world.ship.flight_assist = true;
    // A gang is on us too, and fights on when the convoy is gone.
    let gang = world.galaxy.pirates.gangs.keys().next().expect("a gang");
    let me = world.combat_position();
    world.engage(gang, None, me + DVec3::new(4e3, 0.0, 0.0), me);
    world.update(1.0 / 60.0, Controls::default());
    assert!(world.combat.has_convoy(key), "its ships are in the scene");
    let pirate = |r: &&crate::combat::Raider| r.crew == crate::combat::Crew::Pirate;
    assert!(
        world.combat.raiders.iter().any(|r| pirate(&r)),
        "the gang is out"
    );
    // Where the pirates and wrecks are, in the system frame.
    let scene = |w: &World| {
        let o = w.combat_origin();
        let ships: Vec<(u32, DVec3)> = (w.combat.raiders.iter())
            .filter(pirate)
            .map(|r| (r.id, o + r.craft.pos))
            .collect();
        let wrecks: Vec<DVec3> = w.combat.wrecks.iter().map(|x| o + x.pos).collect();
        (ships, wrecks)
    };
    // Shoot down every ship in the convoy, the last one too.
    let pilot = crate::combat::Pilot {
        craft: world.player_craft(),
        burn: 0.0,
        trigger: false,
    };
    let traders: Vec<u32> = (world.combat.raiders.iter())
        .filter(|r| r.trader_fleet() == Some(key))
        .map(|r| r.id)
        .collect();
    assert!(!traders.is_empty());
    for id in traders {
        world.combat.hit_raider(id, 1e6, &pilot);
    }
    let mut before = scene(&world);
    for _ in 0..30 {
        world.update(1.0 / 60.0, Controls::default());
        let after = scene(&world);
        for (id, at) in &after.0 {
            if let Some((_, was)) = before.0.iter().find(|(i, _)| i == id) {
                let moved = (*at - *was).length();
                assert!(moved < 2e3, "raider {id} jumped {moved:.0} m");
            }
        }
        for (was, at) in before.1.iter().zip(&after.1) {
            let moved = (*at - *was).length();
            assert!(moved < 2e3, "a wreck jumped {moved:.0} m");
        }
        before = after;
    }
    assert_eq!(world.ship.frame, Place::Star, "the convoy is gone");
    assert!(!before.1.is_empty(), "its wrecks are left");
    assert!(world.hostiles_near() > 0, "the gang fights on, close by");
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
fn mining_never_fires_the_guns() {
    // Issue #189: the courier's autocannon went off with every cut, so a
    // new miner could shoot the harvester crews and traders around them.
    let mut world = World::new(1);
    assert!(world.start_at_field());
    assert!(
        !world.combat.player.guns.is_empty(),
        "the courier has a gun"
    );
    assert_eq!(world.hostiles_near(), 0);
    world.act(Action::Scan);
    let factions: Vec<_> = world.galaxy.factions.iter().map(|(id, _)| id).collect();
    let standing =
        |w: &World| -> Vec<f64> { factions.iter().map(|&f| w.player.standing.get(f)).collect() };
    let before = standing(&world);
    let fire = Controls {
        fire: true,
        ..Default::default()
    };
    let mut lasing = false;
    for _ in 0..(20 * 60) {
        world.update(1.0 / 60.0, fire);
        lasing |= world.mining.firing;
        assert!(!world.guns_hot(), "the guns stay on safe");
    }
    assert!(lasing, "the trigger cuts rock");
    let t = world.combat.tally;
    assert_eq!((t.fired, t.lasers), (0, 0), "no gun fired");
    assert_eq!(standing(&world), before, "no standing lost");
}

#[test]
fn armed_by_hand_the_trigger_fires_the_guns() {
    let mut world = World::new(1);
    assert!(world.start_at_field());
    assert_eq!(world.hostiles_near(), 0);
    world.act(Action::ToggleGuns);
    assert!(world.guns_hot(), "{}", said(&world));
    let fire = Controls {
        fire: true,
        ..Default::default()
    };
    for _ in 0..30 {
        world.update(1.0 / 60.0, fire);
        assert!(!world.mining.firing, "the laser stands down");
    }
    assert!(world.combat.tally.fired > 0, "the guns fire");
    // And back on safe, the trigger is the laser's again.
    world.act(Action::ToggleGuns);
    assert!(!world.guns_hot());
    let mut lasing = false;
    for _ in 0..30 {
        world.update(1.0 / 60.0, fire);
        lasing |= world.mining.firing;
    }
    assert!(lasing);
}

#[test]
fn a_running_raider_can_still_be_shot() {
    let mut world = among_raiders();
    for r in &mut world.combat.raiders {
        r.tactic = crate::combat::Tactic::Flee;
    }
    // Running, it no longer holds the ship from fast travel...
    assert_eq!(world.hostiles_near(), 0);
    // ...but the guns stay hot to finish it.
    assert!(world.guns_hot());
}

#[test]
fn a_hostile_factions_traders_running_by_dont_stop_mining() {
    let mut world = World::new(1);
    assert!(world.start_by_traffic(), "seed 1 has traffic");
    let key = world
        .traffic
        .contacts
        .iter()
        .max_by_key(|c| c.ships)
        .unwrap()
        .key;
    world.enter_frame(Place::Convoy(key));
    world.update(1.0 / 60.0, Controls::default());
    assert!(
        world
            .combat
            .raiders
            .iter()
            .any(|r| r.trader_fleet().is_some())
    );
    for r in &mut world.combat.raiders {
        r.hostile = true;
        r.tactic = crate::combat::Tactic::Flee;
    }
    assert!(!world.guns_hot(), "running traders leave the guns on safe");
    // One that turns to fight heats them.
    world.combat.raiders[0].tactic = crate::combat::Tactic::Attack;
    assert!(world.guns_hot());
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

/// A world in orbit of a body with two cities or more (issue #116), with
/// cargo bound for the last of them.
fn above_a_two_city_planet() -> (World, BodyId, Vec<SettlementId>) {
    let mut world = World::new(1);
    let body = world
        .start_by_cities()
        .expect("a planet with two cities somewhere");
    let cities = world.cities_on(body).collect();
    (world, body, cities)
}

#[test]
fn every_city_on_a_planet_can_be_landed_at() {
    let (mut world, body, cities) = above_a_two_city_planet();
    world.player.contracts.clear();
    assert_eq!(world.prompts().land, Some((cities[0], body)));
    for &city in &cities {
        world.act(Action::TargetCity(city));
        assert_eq!(world.target, Some(Place::Body(body)));
        assert_eq!(
            world.prompts().land,
            Some((city, body)),
            "[L] lands where picked"
        );
    }
    let last = *cities.last().unwrap();
    world.act(Action::Land);
    assert!(
        matches!(world.mode, Mode::Landed { city, .. } if city == last),
        "landed at the city picked: {:?}",
        world.mode
    );
}

#[test]
fn the_landing_prompt_offers_where_the_cargo_is_bound() {
    let (mut world, body, cities) = above_a_two_city_planet();
    let to = *cities.last().unwrap();
    assert_eq!(world.prompts().land, Some((to, body)));
    let hint = crate::guide::hint(&world).expect("a hint");
    let name = world.galaxy.settlements[to].name.clone();
    assert_eq!(hint, format!("[L] to land at {name}."));

    // From afar, the hint names the planet the city is on, until it's
    // picked in NAVIGATION.
    let near = world.ship.clone();
    open_space(&mut world);
    let hint = crate::guide::hint(&world).expect("a hint");
    let planet = world.name(Place::Body(body));
    assert!(hint.contains(&format!("{name}, on {planet}")), "{hint}");
    world.act(Action::TargetCity(cities[0]));
    let hint = crate::guide::hint(&world).expect("a hint");
    assert!(
        hint.contains(&format!("on {planet}")),
        "the wrong city: {hint}"
    );
    world.act(Action::TargetCity(to));
    let hint = crate::guide::hint(&world).expect("a hint");
    assert!(!hint.contains("NAVIGATION"), "picked: {hint}");
    world.ship = near;
    assert_eq!(world.prompts().land, Some((to, body)));
}

#[test]
fn t_cycles_through_cities_under_their_planet() {
    let (mut world, body, cities) = above_a_two_city_planet();
    world.act(Action::Target(Place::Body(body)));
    for &city in &cities {
        world.act(Action::CycleTarget);
        assert_eq!(world.nav_selected(), Some((Place::Body(body), Some(city))));
    }
    world.act(Action::CycleTarget);
    assert_eq!(world.landing.filter(|c| cities.contains(c)), None);
}

#[test]
fn fast_travel_into_a_field_stops_in_laser_reach_of_a_rock() {
    // Issue #122: not 10 km past the field's edge, a long hand flight
    // from its rocks.
    let mut world = World::new(1);
    let field = world.view.fields[0];
    open_space(&mut world);
    world.target = Some(Place::Field(field));
    world.act(Action::FastTravel);
    while matches!(world.mode, Mode::FastTravel(_)) {
        world.update(1.0 / 60.0, Controls::default());
    }
    let (_, rocks) = world.field_rocks.clone().expect("in the field's rocks");
    let f = &world.galaxy.fields[field];
    let ship = world.ship.rel;
    let nearest = world
        .mining
        .live_rocks(field, f, &rocks, world.clock_s)
        .map(|r| (r.pos - ship).length() - r.radius)
        .fold(f64::INFINITY, f64::min);
    let reach = world.stats().mining_range_m;
    assert!(
        nearest < reach,
        "nearest rock {nearest:.0} m off, laser reaches {reach:.0}"
    );
    // Turned nose-on to it: prospect and fire.
    real_seconds_until(&mut world, 8.0, |w| w.turn_over.is_none());
    assert!(
        world.mining.aim.is_some_and(|a| a.distance < reach),
        "the laser is on a rock as the trip ends"
    );
}

/// NAVIGATION (issue #119): stations hang under the body they orbit, the
/// ones round the star come before the fields, and fields read without the
/// system's name.
#[test]
fn navigation_puts_stations_up_top_and_shortens_field_names() {
    let world = World::new(1);
    let rows = world.nav_entries();
    let first_field = rows
        .iter()
        .position(|r| matches!(r.0, Place::Field(_)))
        .expect("seed 1 starts in a system with fields");
    assert!(
        rows[first_field..]
            .iter()
            .all(|r| matches!(r.0, Place::Field(_))),
        "fields come last"
    );
    let stations: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r.0, Place::Station(_)))
        .collect();
    assert!(!stations.is_empty(), "seed 1 starts by a station");
    for &(i, &row) in &stations {
        let Place::Station(s) = row.0 else {
            unreachable!()
        };
        assert!(i < first_field, "a station below the fields");
        match world.station_body(s) {
            Some(b) => {
                // Somewhere in its body's block of rows, one step in.
                let body_row = rows
                    .iter()
                    .position(|r| *r == (Place::Body(b), None))
                    .expect("its body is listed");
                assert!(body_row < i);
                assert!(
                    rows[body_row + 1..i]
                        .iter()
                        .all(|&r| world.nav_depth(r) > world.nav_depth(rows[body_row])),
                    "the station is cut off from its body"
                );
                assert_eq!(world.nav_depth(row), world.nav_depth(rows[body_row]) + 1);
            }
            None => assert_eq!(world.nav_depth(row), 0),
        }
    }
    // Every place is listed once.
    let places: std::collections::HashSet<_> =
        rows.iter().filter(|r| r.1.is_none()).map(|r| r.0).collect();
    assert_eq!(places.len(), world.nav_places().len());

    let system = &world.galaxy.systems[world.system].name;
    for r in &rows[first_field..] {
        let short = world.short_name(r.0);
        assert!(!short.contains(system.as_str()), "{short}");
        assert!(!short.contains("field"), "{short}");
        let letter = world.name(r.0).chars().last().expect("a name");
        assert!(short.ends_with(letter), "{short} lost its letter");
    }
    let shorts: std::collections::HashSet<_> = rows[first_field..]
        .iter()
        .map(|r| world.short_name(r.0))
        .collect();
    assert_eq!(
        shorts.len(),
        rows.len() - first_field,
        "two fields read the same"
    );
}

/// Issue #161: the nearest shipyards are places the player can reach
/// (listed in NAVIGATION on arrival, issue #169), quickest first, and each
/// sells ships; a city with none names one.
#[test]
fn nearest_shipyards_are_reachable_and_ordered() {
    for seed in 1..=4 {
        let world = World::new(seed);
        let yards = world.nearest(Service::Shipyard, 4);
        assert!(!yards.is_empty(), "seed {seed}: no shipyard in reach");
        for pair in yards.windows(2) {
            assert!(pair[0].days <= pair[1].days, "seed {seed}: out of order");
        }
        for y in &yards {
            let st = &world.galaxy.settlements[y.at];
            let view = SystemView::new(&world.galaxy, y.system);
            assert!(
                view.stations.contains(&y.at) || view.cities.iter().any(|&(c, _)| c == y.at),
                "seed {seed}: {} isn't listed",
                st.name
            );
            assert!(crate::hangar::services(&st.economy).shipyard);
            assert_eq!(st.system, y.system);
            let route = world.route(y.system).expect("a route there");
            assert_eq!(route.len() as u32 - 1, y.jumps, "seed {seed}: jumps");
        }
        let refits = world.nearest(Service::Refit, 1);
        assert!(!refits.is_empty(), "seed {seed}: nowhere to refit");
    }
}

/// A new pilot at the start can find a trade run (issue #160): in
/// market space, something to buy here and a port within one jump that
/// pays more for it, and buying it heads the ship there; in planned
/// space, where one price holds everywhere, the way to market space.
#[test]
fn the_start_shows_a_trade_run_or_the_way_to_one() {
    let mut market_starts = 0;
    for seed in 1..=4 {
        let mut w = World::new(seed);
        let here = w.docked_at().expect("starts landed");
        let model = w.galaxy.factions[w.galaxy.settlements[here].owner].economy;
        let runs = w.trade_runs(4);
        if runs.is_empty() {
            assert_eq!(
                model,
                retrox_sim::EconomyModel::Planned,
                "a market start has runs"
            );
            let (port, jumps) = w.nearest_market_space().expect("market space somewhere");
            assert!(jumps >= 1);
            let s = &w.galaxy.settlements[port];
            assert_eq!(
                w.galaxy.factions[s.owner].economy,
                retrox_sim::EconomyModel::Market
            );
            continue;
        }
        if model == retrox_sim::EconomyModel::Market {
            market_starts += 1;
        }
        let run = runs[0];
        assert!(run.profit() > 0.0 && run.jumps <= 1);
        assert!(runs.windows(2).all(|p| p[0].good != p[1].good));
        w.act(Action::TakeRun(run));
        assert_eq!(w.player.cargo[run.good], run.tonnes, "bought the lot");
        assert_eq!(w.player.sell_at, Some(run.to));
        let to_system = w.galaxy.settlements[run.to].system;
        if to_system == w.system {
            assert!(w.target.is_some(), "targeted");
        } else {
            assert_eq!(w.course, Some(to_system), "course plotted");
        }
        let hint = crate::guide::hint(&w).expect("a hint");
        assert!(hint.contains("LAUNCH"), "{hint}");
        // Sold where it goes, it pays more than it cost, and the hold
        // is no longer bound anywhere.
        w.mode = Mode::Docked(run.to);
        if w.city_body(run.to).is_some() {
            w.mode = Mode::Landed {
                city: run.to,
                body: w.city_body(run.to).unwrap(),
                timer: 0.0,
            };
        }
        let before = w.player.credits;
        w.act(Action::Trade(run.good, -run.tonnes));
        assert!(w.player.credits - before > run.cost, "sold at a profit");
        assert_eq!(w.player.sell_at, None);
    }
    assert!(market_starts > 0);
}

#[test]
fn the_ship_stops_at_the_quarry_face_it_sees() {
    // #155: a quarry stopped the ship at a sphere of its nominal radius,
    // a hundred metres and more off its squashed face, so the ore it shed
    // was out of the tractor's reach; and its bulges poked through the
    // ship. Fly into one, from all round, slow and fast: the ship ends up
    // against the face you see, never inside it.
    let mut world = World::new(1);
    assert!(world.start_at_field());
    let (field, rocks) = world.field_rocks.clone().expect("in a field");
    let quarry = rocks
        .iter()
        .enumerate()
        .filter(|(_, r)| r.radius_m > crate::mining::QUARRY_RADIUS)
        .max_by(|a, b| a.1.radius_m.total_cmp(&b.1.radius_m))
        .map(|(i, _)| i)
        .expect("a field has a quarry");
    let solid = |w: &World| {
        w.mining
            .solids(field, &rocks, w.clock_s)
            .nth(quarry)
            .expect("the quarry stands")
    };
    let dirs = [
        DVec3::X,
        DVec3::NEG_Y,
        DVec3::Y,
        DVec3::new(0.4, 0.3, -0.8).normalize(),
    ];
    for dir in dirs {
        for throttle in [0.1, 0.4, 0.6] {
            let s = solid(&world);
            world.ship.frame = Place::Field(field);
            world.ship.rel = s.pos + dir * (s.bound() + 200.0);
            world.ship.vel = DVec3::ZERO;
            world.ship.rot = look_rotation(-dir);
            world.ship.throttle = throttle;
            let mut closest = f64::MAX;
            for _ in 0..60 * 40 {
                world.update(1.0 / 60.0, Controls::default());
                let s = solid(&world);
                let gap = (world.ship.rel - s.pos).length() - s.surface_toward(world.ship.rel);
                assert!(
                    gap > 0.0,
                    "{dir} at throttle {throttle}: the ship is {:.1} m inside the rock",
                    -gap
                );
                closest = closest.min(gap);
            }
            assert!(
                closest < 15.0,
                "{dir} at throttle {throttle}: never got closer than {closest:.0} m to the face"
            );
        }
    }
}

/// Issue #154: a city founded while the player is in the system is in
/// NAVIGATION straight away, [T] reaches it and [L] lands there, without
/// leaving the system and coming back.
#[test]
fn a_city_founded_while_here_can_be_targeted_and_landed_at() {
    let mut world = World::new(1);
    let body = world.colony_due_here().expect("a free region at the start");
    let before: Vec<_> = world.cities_on(body).collect();
    world.act(Action::Wait(1));
    real_seconds_until(&mut world, 30.0, |w| w.waiting.is_none());
    let new: Vec<_> = world
        .cities_on(body)
        .filter(|c| !before.contains(c))
        .collect();
    let [city] = new[..] else {
        panic!("one new city in NAVIGATION, got {new:?}");
    };
    assert!(
        world
            .nav_entries()
            .contains(&(Place::Body(body), Some(city)))
    );

    world.act(Action::Launch);
    world.act(Action::Target(Place::Star));
    for _ in 0..world.nav_entries().len() {
        if world.landing == Some(city) {
            break;
        }
        world.act(Action::CycleTarget);
    }
    assert_eq!(world.nav_selected(), Some((Place::Body(body), Some(city))));
    assert_eq!(world.prompts().land, Some((city, body)));
    world.act(Action::Land);
    assert!(
        matches!(world.mode, Mode::Landed { city: c, .. } if c == city),
        "landed at the new city: {:?}",
        world.mode
    );
}

/// Issue #169: every settlement in a system is a place the player can go:
/// a station to dock at, or a city on its planet or moon to land at. Half
/// of them sit on undivided worlds (`Site::Surface`), with half the yards.
#[test]
fn every_settlement_is_listed_in_navigation() {
    for seed in 1..=3 {
        let world = World::new(seed);
        for (id, _) in world.galaxy.systems.iter() {
            let view = SystemView::new(&world.galaxy, id);
            for (s, st) in world.galaxy.settlements_in(id) {
                let listed = view.stations.contains(&s)
                    || view
                        .cities
                        .iter()
                        .any(|&(c, b)| c == s && view.bodies.contains(&b));
                assert!(
                    listed,
                    "seed {seed}: {} ({:?}) not listed",
                    st.name, st.site
                );
            }
        }
    }
}

/// Issue #169: a city on an undivided world is landed at like any other,
/// from its NAVIGATION row: [J] stops low over it, [L] puts down, and its
/// shipyard sells ships. Its name is its own, not the world's.
#[test]
fn a_city_on_an_undivided_world_can_be_landed_at() {
    let mut world = World::new(1);
    let surface_yard = |w: &World, sys: SystemId| {
        w.galaxy
            .settlements_in(sys)
            .find_map(|(id, s)| match s.site {
                Site::Surface(b) if w.galaxy.bodies[b].kind != BodyKind::Belt => {
                    crate::hangar::services(&s.economy)
                        .shipyard
                        .then_some((id, b))
                }
                _ => None,
            })
    };
    let near = world.nearest(Service::Shipyard, 40);
    let system = near
        .iter()
        .map(|y| y.system)
        .find(|&sys| surface_yard(&world, sys).is_some())
        .expect("a yard on an undivided world in reach");
    for hop in world.route(system).expect("a route").into_iter().skip(1) {
        world.arrive_jump(hop);
    }
    let (city, body) = surface_yard(&world, system).unwrap();
    assert_ne!(
        world.galaxy.settlements[city].name,
        world.galaxy.bodies[body].name
    );
    assert!(
        world
            .nav_entries()
            .contains(&(Place::Body(body), Some(city)))
    );

    world.act(Action::TargetCity(city));
    assert_eq!(world.target, Some(Place::Body(body)));
    // [J] until it stops low over the planet (a climb first if need be).
    for _ in 0..4 {
        if world.prompts().land.is_some() {
            break;
        }
        world.act(Action::FastTravel);
        real_seconds_until(&mut world, 120.0, |w| {
            !matches!(w.mode, Mode::FastTravel(_))
        });
    }
    assert_eq!(world.prompts().land, Some((city, body)));
    world.act(Action::Land);
    while matches!(world.mode, Mode::Landed { timer, .. } if timer > 0.0) {
        world.update(1.0 / 60.0, Controls::default());
    }
    assert_eq!(world.docked_at(), Some(city));
    assert!(world.offers(city, Service::Shipyard));
}
