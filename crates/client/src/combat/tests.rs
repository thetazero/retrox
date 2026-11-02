use super::*;
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
    let fit = stock(raider).unwrap();
    c.raiders.push(Raider {
        id: 0,
        vessel: Vessel::new(fit, Damage::default()),
        craft: Craft {
            pos: DVec3::new(300.0, 100.0, -5000.0),
            vel: DVec3::new(0.0, 0.0, 150.0),
            rot: look_rotation(DVec3::Z),
            spin: DVec3::ZERO,
        },
        burn: 0.0,
        tactic: Tactic::Attack,
        timer: 0.0,
        skill: 0.7,
        rng: Rng::new(7),
        jink: DVec3::ZERO,
        jink_t: 0.0,
        burst: 0.0,
    });
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

#[test]
fn bands_never_gather_at_a_capital_and_stay_put_for_their_stretch() {
    let galaxy = retrox_gen::generate(1, &retrox_gen::GalaxyConfig::default());
    for f in galaxy.factions.values() {
        let system = galaxy.settlements[f.capital].system;
        assert_eq!(band_chance(&galaxy, system), 0.0);
    }
    let mut seen = 0;
    for (id, field) in galaxy.fields.iter() {
        let system = galaxy.bodies[field.belt].system;
        let a = band(&galaxy, system, id, 3);
        assert_eq!(a, band(&galaxy, system, id, 3));
        seen += usize::from(!a.is_empty());
    }
    assert!(seen > 0, "somewhere has raiders");
}
