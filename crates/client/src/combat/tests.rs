use super::*;
use crate::world::{assist, handling, steer};
use retrox_content::ships::stock;

fn vessel(hull: &str) -> Vessel {
    Vessel::new(stock(hull).unwrap(), Damage::default())
}

#[test]
fn segments_enter_ellipsoids_where_they_should() {
    let axes = DVec3::new(2.0, 2.0, 10.0);
    // Straight down the long axis from the front: enters at z = -10.
    let t = segment_ellipsoid(
        DVec3::new(0.0, 0.0, -30.0),
        DVec3::new(0.0, 0.0, 30.0),
        axes,
    );
    assert!((t.unwrap() - 20.0 / 60.0).abs() < 1e-9);
    // Passing wide misses.
    assert_eq!(
        segment_ellipsoid(
            DVec3::new(3.0, 0.0, -30.0),
            DVec3::new(3.0, 0.0, 30.0),
            axes
        ),
        None
    );
    // Stopping short misses.
    assert_eq!(
        segment_ellipsoid(
            DVec3::new(0.0, 0.0, -30.0),
            DVec3::new(0.0, 0.0, -11.0),
            axes
        ),
        None
    );
}

#[test]
fn shields_take_hits_first_and_come_back() {
    let mut v = vessel("interceptor");
    let full = v.shield;
    assert!(full > 0.0);
    assert_eq!(v.hit(full * 0.5, DVec3::ZERO), Hit::Shield);
    assert!(v.damage.total() == 0.0);
    assert_eq!(v.hit(full, DVec3::ZERO), Hit::Hull);
    assert!(
        (v.damage.total() - full * 0.5).abs() < 1e-9,
        "the rest went to the hull"
    );
    assert_eq!(v.shield, 0.0);
    // Nothing comes back while hits keep landing...
    v.systems(SHIELD_DELAY * 0.5, 0.0, false);
    assert_eq!(v.shield, 0.0);
    // ...then it recharges.
    for _ in 0..200 {
        v.systems(0.1, 0.0, false);
    }
    assert!((v.shield - full).abs() < 1e-9);
    assert!(v.damage.total() > 0.0, "hulls don't heal on their own");
}

#[test]
fn a_small_ship_dies_with_its_only_section() {
    let mut v = vessel("interceptor");
    v.shield = 0.0;
    let structure = v.parts[0].structure;
    assert_eq!(v.hit(structure * 0.9, DVec3::ZERO), Hit::Hull);
    assert!(!v.destroyed());
    assert_eq!(v.hit(structure * 0.2, DVec3::ZERO), Hit::Destroyed);
}

#[test]
fn a_framed_ship_loses_what_a_wrecked_section_held() {
    let mut v = vessel("gunship");
    v.shield = 0.0;
    assert!(v.parts.len() >= 3);
    assert!(v.stats.thrust_kn > 0.0);
    let stern = v.parts.len() - 1;
    let at = DVec3::new(0.0, 0.0, v.parts[stern].back - 0.5);
    let hit = v.hit(v.parts[stern].structure + 1.0, at);
    assert_eq!(hit, Hit::Wrecked(stern));
    assert_eq!(v.stats.thrust_kn, 0.0, "the engines were in the stern");
    assert!(!v.destroyed(), "the frame holds");
    // Hits on the wreckage go to the frame, until it gives out.
    let mut last = Hit::Hull;
    for _ in 0..100 {
        last = v.hit(v.frame_max / 10.0, at);
        if last == Hit::Destroyed {
            break;
        }
    }
    assert_eq!(last, Hit::Destroyed);
}

#[test]
fn guns_lock_out_when_the_ship_overheats_then_come_back() {
    let mut v = vessel("interceptor");
    let mut shots = 0;
    let mut locked_at = None;
    for i in 0..2000 {
        shots += v.systems(0.05, 1.0, true).len();
        if v.locked {
            locked_at = Some(i);
            break;
        }
    }
    let locked_at = locked_at.expect("firing at full burn overheats an interceptor");
    let secs = locked_at as f64 * 0.05;
    assert!((10.0..120.0).contains(&secs), "overheated after {secs}s");
    assert!(shots > 50);
    assert!(
        v.systems(0.05, 1.0, true).is_empty(),
        "locked guns don't fire"
    );
    for _ in 0..2000 {
        v.systems(0.05, 0.0, false);
        if !v.locked {
            break;
        }
    }
    assert!(!v.locked, "it vents and the guns come back");
}

#[test]
fn a_ship_short_of_power_fires_slower() {
    // The stock interceptor can't fire, shield and burn all at once.
    let mut v = vessel("interceptor");
    v.systems(0.05, 0.0, true);
    assert!((v.gun_power - 1.0).abs() < 1e-9);
    v.shield = 0.0;
    v.since_hit = SHIELD_DELAY;
    v.systems(0.05, 1.0, true);
    assert!(v.gun_power < 0.9, "gun power {}", v.gun_power);
}

#[test]
fn the_starting_ship_carries_a_gun() {
    let v = vessel(crate::hangar::STARTING_HULL);
    assert!(!v.guns.is_empty());
    assert!(v.shield > 0.0);
}

#[test]
fn rolling_right_drops_the_right_wing() {
    let (mut rot, mut spin) = (DQuat::IDENTITY, DVec3::ZERO);
    for _ in 0..10 {
        steer(&mut rot, &mut spin, DVec3::new(0.0, 0.0, 1.0), 1.0, 0.02);
    }
    assert!((rot * DVec3::X).y < 0.0);
    // And the stick rolls toward a target up and to the right that way.
    let s = stick_toward(DQuat::IDENTITY, DVec3::new(1.0, 1.0, -0.5));
    assert!(s.z > 0.0 && s.x > 0.0 && s.y > 0.0);
}

#[test]
fn leading_a_crossing_target_meets_it() {
    let (pos, vel) = (DVec3::ZERO, DVec3::ZERO);
    let (tpos, tvel) = (DVec3::new(0.0, 0.0, -1000.0), DVec3::new(200.0, 0.0, 0.0));
    let lead = lead_point(pos, vel, tpos, tvel, Some(SHELL_SPEED));
    let t = lead.length() / SHELL_SPEED;
    assert!((tpos + tvel * t - lead).length() < 0.5);
}

/// A pirate of hull `hull` at `pos`, flying toward +Z at 150 m/s,
/// guarding the origin.
fn raider_at(hull: &str, pos: DVec3) -> Raider {
    let craft = Craft {
        pos,
        vel: DVec3::new(0.0, 0.0, 150.0),
        rot: look_rotation(DVec3::Z),
        spin: DVec3::ZERO,
    };
    let mut r = Raider::new(
        0,
        vessel(hull),
        craft,
        Crew::Pirate,
        DVec3::ZERO,
        (0.7, 0.7),
        &mut Rng::new(1),
    );
    r.rng = Rng::new(7);
    r.burst = 0.0;
    r
}

/// A pilot that does what a player should: nose on the lead, close to gun
/// range, trigger down when it's lined up.
fn pilot_step(c: &Combat, me: &mut Craft, burn: &mut f64, dt: f64) -> bool {
    let target = c.target.and_then(|t| c.raider(t));
    let Some(r) = target else {
        return false;
    };
    let lead = c.lead(me, r.id).unwrap();
    let to = lead - me.pos;
    let stick = stick_toward(me.rot, to);
    let stats = c.player.stats;
    let (thrust, turn) = handling(&stats);
    steer(&mut me.rot, &mut me.spin, stick, turn, dt);
    let dist = (r.craft.pos - me.pos).length();
    let speed = if dist > 1500.0 { 400.0 } else { 220.0 };
    *burn = assist(
        &mut me.vel,
        me.rot,
        DVec3::new(0.0, 0.0, -speed),
        false,
        thrust,
        dt,
    );
    me.pos += me.vel * dt;
    me.forward().angle_between(to.normalize()) < GIMBAL && dist < c.player.gun_range()
}

/// A fight in an empty field between the starting ship, flown by
/// `pilot_step` (or sitting still), and one raider. Returns (seconds,
/// whether the player won, whether the player lost).
fn duel(raider: &str, fight_back: bool, seconds: f64) -> (f64, bool, bool) {
    let mut c = Combat::new(
        stock(crate::hangar::STARTING_HULL).unwrap(),
        Damage::default(),
    );
    let mut me = Craft {
        pos: DVec3::ZERO,
        vel: DVec3::ZERO,
        rot: DQuat::IDENTITY,
        spin: DVec3::ZERO,
    };
    c.raiders.push(raider_at(raider, DVec3::new(300.0, 100.0, -5000.0)));
    c.target = Some(0);
    let dt = 1.0 / 60.0;
    let mut t = 0.0;
    let mut burn = 0.0;
    while t < seconds {
        let trigger = fight_back && pilot_step(&c, &mut me, &mut burn, dt);
        c.update(
            dt,
            Pilot {
                craft: me,
                burn,
                trigger,
            },
            &[],
        );
        t += dt;
        if c.raiders.is_empty() {
            return (t, true, false);
        }
        if c.lost.is_some() {
            return (t, false, true);
        }
    }
    (t, false, false)
}

#[test]
fn raiders_attack_a_ship_that_sits_still() {
    let (t, won, lost) = duel("interceptor", false, 180.0);
    assert!(!won);
    assert!(lost, "an interceptor should finish off a sitting duck");
    assert!(t > 10.0, "but not instantly: {t:.1}s");
}

#[test]
fn a_new_pilot_who_fights_back_can_win() {
    let (t, won, _) = duel("interceptor", true, 240.0);
    assert!(
        won,
        "the starting ship should be able to beat one interceptor ({t:.1}s)"
    );
}

const DT: f64 = 1.0 / 60.0;

/// A ship sitting still at `pos`, nose to -Z.
fn still(pos: DVec3) -> Craft {
    Craft {
        pos,
        vel: DVec3::ZERO,
        rot: DQuat::IDENTITY,
        spin: DVec3::ZERO,
    }
}

fn idle(craft: Craft) -> Pilot {
    Pilot {
        craft,
        burn: 0.0,
        trigger: false,
    }
}

fn starting_fight() -> Combat {
    Combat::new(
        stock(crate::hangar::STARTING_HULL).unwrap(),
        Damage::default(),
    )
}

#[test]
fn raiders_give_up_past_their_leash_and_come_back_inside_it() {
    // The raider guards the origin; the player is in reach, then not.
    let mut r = raider_at("interceptor", DVec3::new(0.0, 0.0, -2000.0));
    r.think(DT, Some(still(DVec3::new(0.0, 0.0, LEASH - 1000.0))), &[]);
    assert_eq!(r.tactic, Tactic::Attack, "inside 16 km it attacks");

    let far = still(DVec3::new(0.0, 0.0, LEASH + 500.0));
    r.think(DT, Some(far), &[]);
    assert_eq!(r.tactic, Tactic::Return, "past 16 km it gives up");

    // It doesn't come straight back the moment the player edges inside:
    // only once they're well within.
    let edge = still(DVec3::new(0.0, 0.0, LEASH * 0.85));
    for _ in 0..120 {
        r.think(DT, Some(edge), &[]);
    }
    assert_eq!(r.tactic, Tactic::Return);
    r.think(DT, Some(still(DVec3::new(0.0, 0.0, LEASH * 0.7))), &[]);
    assert_eq!(r.tactic, Tactic::Attack);
}

#[test]
fn a_badly_hurt_raider_runs_and_a_lightly_hurt_one_does_not() {
    let player = still(DVec3::ZERO);
    let mut r = raider_at("interceptor", DVec3::new(0.0, 0.0, -3000.0));
    let structure = r.vessel.parts[0].structure;
    r.vessel.damage.sections[0] = structure * 0.5;
    for _ in 0..(60 * 20) {
        r.think(DT, Some(player), &[]);
        assert_ne!(r.tactic, Tactic::Flee, "half its hull left: it fights on");
    }

    r.vessel.damage.sections[0] = structure * 0.8;
    let mut t = 0.0;
    while r.tactic != Tactic::Flee {
        r.think(DT, Some(player), &[]);
        t += DT;
        assert!(t < 30.0, "a fifth of its hull left and it never ran");
    }
    // Running: it opens the range.
    let from = r.craft.pos.length();
    for _ in 0..(60 * 5) {
        r.think(DT, Some(player), &[]);
    }
    assert_eq!(r.tactic, Tactic::Flee, "once running it keeps running");
    assert!(r.craft.pos.length() > from + 500.0);
}

#[test]
fn a_running_raider_escapes_past_14_km_and_does_not_hold_the_ship() {
    let mut c = starting_fight();
    let mut r = raider_at("interceptor", DVec3::new(0.0, 0.0, -12_000.0));
    r.tactic = Tactic::Flee;
    c.raiders.push(r);
    let me = still(DVec3::ZERO);
    assert_eq!(
        c.hostiles_within(me.pos, HOSTILE_RANGE),
        0,
        "a raider on the run doesn't keep the ship from travel"
    );
    c.update(DT, idle(me), &[]);
    assert_eq!(c.raiders.len(), 1, "12 km off, it is still in the scene");
    let mut t = 0.0;
    while !c.raiders.is_empty() {
        let d = c.raiders[0].craft.pos.length();
        c.update(DT, idle(me), &[]);
        t += DT;
        assert!(t < 60.0, "never got away");
        if c.raiders.is_empty() {
            assert!(d > 13_900.0, "gone at {d:.0} m");
        }
    }
    assert!(c.killed.is_empty(), "escaping isn't dying");
}

#[test]
fn a_destroyed_raider_spills_salvage_and_leaves_a_wreck_for_three_minutes() {
    let mut c = starting_fight();
    let at = DVec3::new(0.0, 0.0, -3000.0);
    c.raiders.push(raider_at("interceptor", at));
    let pilot = idle(still(DVec3::ZERO));
    c.hit_raider(0, 1e6, &pilot);
    let out = c.update(DT, pilot, &[]);
    assert!(c.raiders.is_empty());
    assert_eq!(c.killed, vec![0]);
    assert_eq!(out.kills, vec![(0, Crew::Pirate)]);
    assert!(
        out.messages
            .iter()
            .any(|m| m == "Raider destroyed · the gang is broken"),
        "{:?}",
        out.messages
    );

    // Components and metal, three to two, a few tonnes, tumbling out
    // from where it died.
    let goods: Vec<Good> = out.salvage.iter().map(|s| s.2).collect();
    assert_eq!(goods, vec![Good::Components, Good::Metal]);
    let tonnes: f64 = out.salvage.iter().map(|s| s.3).sum();
    assert!((0.5..=9.6).contains(&tonnes), "{tonnes} t");
    assert!((out.salvage[0].3 / tonnes - 0.6).abs() < 1e-9);
    for s in &out.salvage {
        assert!((s.0 - at).length() < 10.0, "spilled where it died");
    }

    assert_eq!(c.wrecks.len(), 1);
    assert!((c.wrecks[0].pos - at).length() < 10.0);
    for _ in 0..179 {
        c.update(1.0, pilot, &[]);
    }
    assert_eq!(c.wrecks.len(), 1, "the wreck tumbles on for three minutes");
    c.update(1.5, pilot, &[]);
    assert!(c.wrecks.is_empty(), "then drifts out of view");
}
