//! Help for a new pilot (issue #79): the controls, as tables shared by the
//! in-flight controls card and the handbook, and the *first steps* notes,
//! a checklist that lights as the player does each thing for the first
//! time, with one hint about what to do next.
//!
//! The notes are not a scripted tutorial: nothing waits on them, and the
//! hint is read off the situation (where you are, what you carry, what
//! you're aimed at) every frame, so it stays right whatever order you do
//! things in.

use retrox_sim::economy::{Good, GoodMap};
use serde::{Deserialize, Serialize};

use crate::world::{Leg, MiningSpot, Mode, Place, Screen, World};

/// What the player has done at least once, for the first-steps notes.
/// Kept with the player in saves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    /// Took a contract, bought goods, or mined ore.
    pub took_work: bool,
    pub launched: bool,
    pub targeted: bool,
    pub fast_travelled: bool,
    pub jumped: bool,
    /// Docked or landed after a flight.
    pub arrived: bool,
    /// Paid for work: a contract delivered, a bounty, or goods sold that
    /// were bought or mined, not ones kept from a failed contract (#127).
    pub earned: bool,
    /// Tonnes of each good in the hold kept from failed contracts. Selling
    /// these doesn't count as getting paid; they sell first.
    pub forfeit_t: GoodMap<f64>,
}

impl Progress {
    /// The checklist, in the order a first run usually goes.
    pub fn steps(&self) -> [(&'static str, bool); 7] {
        [
            ("Take on work", self.took_work),
            ("Launch", self.launched),
            ("Pick a target", self.targeted),
            ("Fast travel", self.fast_travelled),
            ("Jump a hyperlane", self.jumped),
            ("Dock or land", self.arrived),
            ("Get paid", self.earned),
        ]
    }

    pub fn complete(&self) -> bool {
        self.steps().iter().all(|s| s.1)
    }

    /// Note what the world shows has been done.
    pub fn observe(&mut self, w: &World) {
        match w.mode {
            Mode::Normal => {
                self.launched = true;
                self.targeted |= w.target.is_some();
            }
            Mode::FastTravel(_) => self.fast_travelled = true,
            Mode::Jumping { .. } => self.jumped = true,
            Mode::Docked(_) | Mode::Landed { .. } => self.arrived |= self.launched,
        }
        self.took_work |= !w.player.contracts.is_empty() || w.player.cargo_t() > 0.0;
        // Lost from the hold some other way (raiders), it's gone first.
        for good in Good::ALL {
            self.forfeit_t[good] = self.forfeit_t[good].min(w.player.cargo[good]);
        }
    }

    /// `sold` tonnes of `good` were sold: whether any were the player's own
    /// work rather than goods kept from a failed contract.
    pub fn sold(&mut self, good: Good, sold: f64) -> bool {
        let forfeit = self.forfeit_t[good].min(sold);
        self.forfeit_t[good] -= forfeit;
        sold - forfeit > 1e-9
    }
}

/// A row of a controls table: the keys, and what they do.
pub type KeyRow = (&'static [&'static str], &'static str);

/// The flight keys, with the arrow-key pitch as the *Invert pitch*
/// setting has it (issue #128): by default the keys work like a stick,
/// ↓ pulling the nose up; inverted, ↑ does.
pub fn flight_keys(invert_pitch: bool) -> &'static [KeyRow] {
    if invert_pitch {
        FLIGHT_KEYS_INVERTED
    } else {
        FLIGHT_KEYS
    }
}

pub const FLIGHT_KEYS: &[KeyRow] = &[
    (&["MOUSE"], "aim the amber diamond; the nose follows (click the view)"),
    (&["↑", "↓"], "aim: ↓ nose up · ↑ nose down"),
    (&["←", "→"], "aim left / right · tap to aim finely"),
    (&["W"], "hold to fly ahead, let go to stop"),
    (&["S"], "hold to back off"),
    (&["W", "W"], "let go at 500 m/s, W again: cruise"),
    (&["X"], "cut the cruise throttle"),
    (&["A", "D"], "strafe"),
    (&["SPACE", "CTRL"], "rise / sink"),
    (&["Q", "E"], "roll"),
    (&["SHIFT"], "boost"),
    (&["Z"], "flight assist on / off"),
];

const FLIGHT_KEYS_INVERTED: &[KeyRow] = &[
    (&["MOUSE"], "aim the amber diamond; the nose follows (click the view)"),
    (&["↑", "↓"], "aim: ↑ nose up · ↓ nose down"),
    (&["←", "→"], "aim left / right · tap to aim finely"),
    (&["W"], "hold to fly ahead, let go to stop"),
    (&["S"], "hold to back off"),
    (&["W", "W"], "let go at 500 m/s, W again: cruise"),
    (&["X"], "cut the cruise throttle"),
    (&["A", "D"], "strafe"),
    (&["SPACE", "CTRL"], "rise / sink"),
    (&["Q", "E"], "roll"),
    (&["SHIFT"], "boost"),
    (&["Z"], "flight assist on / off"),
];

pub const TRAVEL_KEYS: &[KeyRow] = &[
    (&["T"], "next target"),
    (
        &["J"],
        "fast travel · on a course: climb out, jump · again: drop out",
    ),
    (&["F"], "dock at a station"),
    (&["L"], "land at a city"),
    (&["M"], "galaxy map"),
];

pub const WORK_KEYS: &[KeyRow] = &[
    (&["G"], "prospect: read the rocks"),
    (&["LMB", "R"], "mining laser · gas scoop · guns when hot"),
    (&["B"], "arm the guns / back on safe"),
    (&["Y"], "next hostile target"),
    (&["I"], "intercept the trader or gang in your sights"),
    (&["V"], "cockpit / chase camera"),
    (&["TAB"], "take or free the mouse"),
    (&["H"], "controls card"),
    (&["ESC"], "menu, or back"),
];

/// A run of text with keys and places in it: `[J]` is drawn as a keycap,
/// `{Belt B}` as a link that picks that place in NAVIGATION (#165).
pub enum Seg<'a> {
    Text(&'a str),
    Key(&'a str),
    Place(&'a str),
}

pub fn segments(text: &str) -> Vec<Seg<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['[', '{']) {
        let key = rest[open..].starts_with('[');
        let shut = if key { ']' } else { '}' };
        let Some(close) = rest[open..].find(shut).map(|c| open + c) else {
            break;
        };
        if open > 0 {
            out.push(Seg::Text(&rest[..open]));
        }
        let inner = &rest[open + 1..close];
        out.push(if key {
            Seg::Key(inner)
        } else {
            Seg::Place(inner)
        });
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        out.push(Seg::Text(rest));
    }
    out
}

/// The place a contract's settlement is reached at, in this system.
fn contract_place(w: &World, to: retrox_sim::SettlementId) -> Option<Place> {
    if w.view.stations.contains(&to) {
        return Some(Place::Station(to));
    }
    w.city_body(to).map(Place::Body)
}

/// A settlement's name, and for a city of this system the planet it's on
/// (issue #116): "Kiolgristio, on Mufath Prime III".
fn where_name(w: &World, to: retrox_sim::SettlementId) -> String {
    let name = &w.galaxy.settlements[to].name;
    match w.city_body(to) {
        Some(body) => format!("{name}, on {}", w.name(Place::Body(body))),
        None => name.clone(),
    }
}

/// The job on the board with the fewest jumps that the player can take
/// now (issue #129): the quickest first payment.
fn nearest_job(w: &World) -> Option<&retrox_sim::freight::Offer> {
    let capacity = w.stats().cargo_t;
    w.board
        .iter()
        .filter(|o| crate::contracts::refusal(&w.player, capacity, o).is_none())
        .min_by_key(|o| o.jumps)
}

/// Where to mine, as a hint's second thought (issue #165): the nearest
/// field here by its NAVIGATION name, or the nearest system with fields.
fn go_mining(w: &World) -> String {
    match w.mining_spot() {
        Some(MiningSpot::Here(f)) => format!(
            "Or LAUNCH and go mining at {{{}}}, the nearest asteroid field (∴).",
            w.short_name(Place::Field(f))
        ),
        Some(MiningSpot::Away { system, jumps }) if w.course == Some(system) => format!(
            "Or go mining: your course is set for {} ({}), where the nearest asteroid fields \
             are. LAUNCH, then jump.",
            w.galaxy.systems[system].name,
            crate::hud::jumps_text(jumps)
        ),
        Some(MiningSpot::Away { system, jumps }) => format!(
            "Or go mining: there are no asteroid fields here. The nearest are in {{{}}} \
             ({}): click it to plot a course, or find it on the galaxy map [M].",
            w.galaxy.systems[system].name,
            crate::hud::jumps_text(jumps)
        ),
        None => "Or buy cheap and sell dear on TRADE.".into(),
    }
}

/// What to do next, from where things stand: one or two sentences, keys
/// in brackets. `None` while there's nothing to say (a jump under way).
pub fn hint(w: &World) -> Option<String> {
    // Where the hold is going: a contract's consignee, or else where the
    // player set out to sell their own goods (issue #160).
    let trading = w.player.contracts.is_empty() && w.player.sell_at.is_some();
    let first = w
        .player
        .contracts
        .first()
        .map(|c| c.offer.to)
        .or(w.player.sell_at)
        .and_then(|to| {
            let s = w.galaxy.settlements.get(to)?;
            Some((to, s.name.clone(), s.system))
        });
    let bound_for = if trading {
        "Your cargo sells best at"
    } else {
        "Your cargo is for"
    };
    let carrying = w.player.cargo_t() > 0.0;
    let own_cargo = w.player.cargo.0.iter().any(|&t| t >= 1.0);
    if let Some(here) = w.docked_at() {
        let selling_here = trading && w.player.sell_at == Some(here);
        let text: String = match w.screen {
            Screen::Contracts if first.is_none() => {
                let pick = "Pick a job and press ACCEPT: the goods load into your hold, and \
                            you're paid when they arrive.";
                // ACCEPT plots the course only when none is set (#188).
                let then = if w.course.is_none() {
                    "Taking a job sets your course for it."
                } else {
                    "Then PLOT COURSE on it."
                };
                match nearest_job(w) {
                    Some(o) => format!(
                        "{pick} The nearest, to {} ({}), pays soonest. {then}",
                        w.galaxy.settlements[o.to].name,
                        crate::hud::jumps_text(o.jumps)
                    ),
                    None => format!("{pick} {then}"),
                }
            }
            // Taking a job plots its course (#188); say where it goes, and
            // how to plot it if another course was kept.
            Screen::Contracts => match &first {
                Some((_, name, system)) if *system != w.system => {
                    let there = &w.galaxy.systems[*system].name;
                    let jumps = w
                        .route(*system)
                        .map(|r| format!(" ({})", crate::hud::jumps_text(r.len() as u32 - 1)))
                        .unwrap_or_default();
                    if w.course == Some(*system) {
                        format!(
                            "Cargo aboard for {name}. Your course is set for {there}{jumps}: \
                             [ESC] back to the menu, then LAUNCH."
                        )
                    } else {
                        format!(
                            "Cargo aboard for {name}, in {there}{jumps}. Press PLOT COURSE on \
                             it, then [ESC] back to the menu and LAUNCH."
                        )
                    }
                }
                _ => "Cargo aboard. [ESC] back to the menu, then LAUNCH.".into(),
            },
            Screen::Trade if selling_here => "Your cargo sells best here: press ALL in the \
                 teal SELL bank on its row."
                .into(),
            Screen::Trade if trading && own_cargo => {
                "Cargo aboard and course set. [ESC] back to the menu, then LAUNCH.".into()
            }
            Screen::Trade if own_cargo => "YOUR CARGO SELLS AT shows where each good fetches \
                 most within one jump: GO sets the course, or sell here in the teal bank."
                .into(),
            Screen::Trade if w.trade_runs(1).is_empty() && w.nearest_market_space().is_some() => {
                "Nothing here sells for more within one jump. TRADE RUNS names the nearest \
                 market space: PLOT COURSE and go look."
                    .into()
            }
            Screen::Trade if w.trade_runs(1).is_empty() => "Nothing here sells for more within \
                 one jump today. Markets move daily: wait, or take a contract or go mining."
                .into(),
            Screen::Trade => "TRADE RUNS lists what sells for more within one jump. BUY & GO \
                 buys the lot and sets your course; sell it there."
                .into(),
            Screen::Bounties => "Warrants pay the moment a pirate ship goes down. PLOT \
                 COURSE to a lair, fly to its field, and bring a gun that can win."
                .into(),
            Screen::Hangar | Screen::Shipyard => {
                "Refits change what your ship carries and how well it mines. [ESC] back.".into()
            }
            Screen::Hub => match &first {
                None if !carrying => format!(
                    "Open CONTRACTS and take a hauling job: the surest first money. Or open \
                     TRADE to buy low and sell high. {}",
                    go_mining(w)
                ),
                _ if selling_here => "Your cargo sells best here: open TRADE and sell it.".into(),
                None => "Open TRADE to see where your cargo sells best, then LAUNCH.".into(),
                Some((_, name, system)) if *system != w.system && w.course != Some(*system) => {
                    let how = if trading {
                        "Open TRADE and press GO"
                    } else {
                        "Open CONTRACTS and PLOT COURSE"
                    };
                    format!(
                        "{bound_for} {name}, in {}. {how}, then LAUNCH.",
                        w.galaxy.systems[*system].name
                    )
                }
                Some((to, _, _)) => {
                    format!("LAUNCH, and take your cargo to {}.", where_name(w, *to))
                }
            },
        };
        // The way out is named as its button is: a station's is UNDOCK.
        return Some(if matches!(w.mode, Mode::Docked(_)) {
            text.replace("LAUNCH", "UNDOCK")
        } else {
            text
        });
    }
    match w.mode {
        Mode::FastTravel(trip) => {
            return Some(match trip.leg {
                Leg::To(_) => "Fast travel flies you there for real while time runs fast. [J] \
                               drops out early."
                    .into(),
                Leg::Climb { .. } => "Climbing out of the gravity well: the autopilot stops at \
                                      its edge, where you can jump or fast travel on."
                    .into(),
                Leg::Intercept(_) => "Intercepting: the autopilot brings you alongside the \
                                      convoy. [B] arms the guns; shooting it makes its \
                                      faction hostile."
                    .into(),
                Leg::Gang(_) => "Going after the gang: unless it's lying low after a fight, \
                                 it turns to meet you and pulls you out of fast travel. [B] \
                                 arms the guns."
                    .into(),
            });
        }
        Mode::Normal => {}
        _ => return None,
    }

    let prompts = w.prompts();
    // Docking or landing is the next thing only where you were going:
    // not back where you just launched from.
    let bound = |s| first.as_ref().is_some_and(|f| f.0 == s);
    if let Some(s) = prompts.dock
        && (w.target == Some(Place::Station(s)) || bound(s))
    {
        return Some(format!("[F] to dock at {}.", w.name(Place::Station(s))));
    }
    if let Some((city, body)) = prompts.land
        && (w.target == Some(Place::Body(body)) || bound(city))
    {
        return Some(format!(
            "[L] to land at {}.",
            w.galaxy.settlements[city].name
        ));
    }
    // In a gang's lair with the gang away: nothing comes of waiting there
    // (time runs 1:1 in space), so say how to go and find it (#171).
    if let Some((field, _)) = &w.field_rocks
        && w.combat.gang.is_none()
        && let Some(g) = retrox_sim::pirates::gang_at(&w.galaxy, *field)
        && let Some(c) = w.traffic.pirate(g)
        && let Some(p) = w.galaxy.pirates.prowls.get(&g)
    {
        let d = crate::hud::distance((c.pos - w.ship_position()).length());
        let name = &c.name;
        return Some(if p.aim == retrox_sim::pirates::Aim::Stalk {
            format!("The {name} are coming for you, {d} out: [I] meets them on the way.")
        } else if w.galaxy.time < p.rest_until {
            format!("The {name} are lying low after the fight, {d} off: [I] goes in after them.")
        } else {
            format!("The {name} lair here but are out, {d} away: [I] goes after them.")
        });
    }
    if w.field_rocks.is_some() {
        return Some(
            "Mining: [G] reads the rocks. Fire [LMB] or [R] on a glowing vein and keep the \
             rock's stress needle in its teal band: let off as it climbs, and before a POCKET \
             goes. In the red it cracks faster but scars the vein; at the end it shatters. \
             Break it clean on a vein and its core may come out whole. Fly close to scoop \
             the ore."
                .into(),
        );
    }
    let well = prompts.well.map(|p| w.name(p));
    // On a plotted course: out of the well, then onto the lane, with [J]
    // both times (#188).
    let jump = || match (&well, prompts.jump) {
        (Some(well), _) => {
            format!("Jumps need open space: [J] to climb out of the gravity well of {well}.")
        }
        (None, Some(next)) => format!(
            "Clear to jump: [J] jumps the lane to {}, or press JUMP on the ▶ lane in \
             HYPERLANES.",
            w.galaxy.systems[next].name
        ),
        (None, None) => "Clear to jump: press JUMP on the ▶ lane in HYPERLANES.".to_owned(),
    };
    if let Some((to, name, system)) = &first {
        if *system != w.system {
            let there = &w.galaxy.systems[*system].name;
            let jumps = w
                .route(*system)
                .map(|r| format!(" ({})", crate::hud::jumps_text(r.len() as u32 - 1)))
                .unwrap_or_default();
            return Some(if w.course == Some(*system) {
                format!(
                    "{bound_for} {name}, in {there}{jumps}: course set. {}",
                    jump()
                )
            } else {
                // The system is a link: a click plots the course (#165).
                format!(
                    "{bound_for} {name}, in {{{there}}}{jumps}: click it to plot a course, or \
                     find it on the galaxy map [M]."
                )
            });
        }
        // Picked means its row in NAVIGATION: for a city, the planet
        // targeted with this city to land at.
        let picked = |place| match place {
            Place::Body(b) => w.target == Some(place) && w.landing_city(b) == Some(*to),
            _ => w.target == Some(place),
        };
        if let Some(place) = contract_place(w, *to)
            && !picked(place)
        {
            let name = where_name(w, *to);
            return Some(format!(
                "{bound_for} {name}: click it in NAVIGATION, or [T] through targets."
            ));
        }
    }
    // Bound for the nearest fields (#165), with nothing else to do.
    if first.is_none()
        && !carrying
        && let Some(MiningSpot::Away { system, .. }) = w.mining_spot()
        && w.course == Some(system)
    {
        return Some(format!(
            "Off to mine in {}. {}",
            w.galaxy.systems[system].name,
            jump()
        ));
    }
    let Some(target) = w.target else {
        return Some(if first.is_none() && !carrying {
            match w.mining_spot() {
                Some(MiningSpot::Here(f)) => format!(
                    "Pick where to go: [T] cycles targets, or click a name in NAVIGATION. \
                     Asteroid fields (∴) can be mined; the nearest is {{{}}}.",
                    w.short_name(Place::Field(f))
                ),
                _ => format!(
                    "Pick where to go: [T] cycles targets, or click a name in NAVIGATION. {}",
                    go_mining(w)
                ),
            }
        } else {
            "Pick where to go: [T] cycles targets, or click a name in NAVIGATION.".into()
        });
    };
    let name = w.name(target);
    if prompts.fast_travel.is_some() {
        return Some(format!("[J] to fast travel to {name}."));
    }
    // A target in the same well (a highport above its city, #112) is just
    // close: fly the rest.
    if let Some(well) = well
        && prompts.well.is_some_and(|p| !w.arrives_within(target, p))
    {
        return Some(format!(
            "{name} is outside the gravity well of {well}: [J] to climb out, then [J] again \
             to fast travel."
        ));
    }
    Some(match target {
        Place::Station(_) => format!("Close in on {name} under 150 m/s, then [F] to dock."),
        Place::Field(_) => format!("Fly into {name} to mine it."),
        _ => format!("{name} is close. [T] for another target."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_in_brackets_become_keycaps() {
        let s = segments("press [J] to go, [ESC] to stop at {Belt B}");
        let shape: Vec<String> = s
            .iter()
            .map(|s| match s {
                Seg::Text(t) => format!("t:{t}"),
                Seg::Key(k) => format!("k:{k}"),
                Seg::Place(p) => format!("p:{p}"),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "t:press ",
                "k:J",
                "t: to go, ",
                "k:ESC",
                "t: to stop at ",
                "p:Belt B"
            ]
        );
    }

    #[test]
    fn a_fresh_game_starts_with_nothing_done_and_a_hint() {
        let w = World::new(1);
        assert!(!w.player.progress.complete());
        assert!(!w.player.progress.launched);
        let hint = hint(&w).expect("landed: a hint");
        assert!(hint.contains("CONTRACTS"), "{hint}");
    }

    /// Issue #165: the hint names a field to mine, as a link, and from a
    /// system without fields names the nearest system that has some.
    #[test]
    fn the_mining_hint_names_somewhere_to_mine() {
        let mut w = World::new(1);
        let Some(MiningSpot::Here(f)) = w.mining_spot() else {
            panic!("a fresh game starts in a system with a field");
        };
        let hint = hint(&w).expect("landed: a hint");
        let link = format!("{{{}}}", w.short_name(Place::Field(f)));
        assert!(hint.contains(&link), "{hint}");

        // Seed 1's first station is in a system with no fields.
        w.start_at_station();
        assert!(w.view.fields.is_empty());
        let Some(MiningSpot::Away { system, jumps }) = w.mining_spot() else {
            panic!("fields somewhere near");
        };
        assert!(jumps >= 1);
        let name = w.galaxy.systems[system].name.clone();
        let hint = super::hint(&w).expect("docked: a hint");
        assert!(hint.contains(&format!("{{{name}}}")), "{hint}");
        assert!(
            !hint.contains("LAUNCH"),
            "a station's button says UNDOCK: {hint}"
        );

        // With the course plotted, the hint says so.
        w.act(crate::world::Action::PlotCourse(Some(system)));
        let hint = super::hint(&w).expect("docked: a hint");
        assert!(hint.contains("course is set"), "{hint}");
    }

    #[test]
    fn the_board_hint_names_the_nearest_job() {
        let mut w = World::new(1);
        w.screen = Screen::Contracts;
        let nearest = nearest_job(&w).expect("work on the first board").clone();
        assert!(w.board.iter().all(|o| o.jumps >= nearest.jumps));
        let hint = hint(&w).expect("a hint");
        let name = &w.galaxy.settlements[nearest.to].name;
        assert!(hint.contains(name.as_str()), "{hint}");
    }

    /// Issue #188: after taking a job, every hint on the way names the
    /// destination system and the next key, docked and in flight.
    #[test]
    fn contract_hints_lead_to_the_destination() {
        use crate::world::Action;
        let mut w = World::new(1);
        w.screen = Screen::Contracts;
        let i = w
            .board
            .iter()
            .position(|o| w.galaxy.settlements[o.to].system != w.system)
            .expect("a job in another system");
        let dest = w.galaxy.settlements[w.board[i].to].system;
        let there = w.galaxy.systems[dest].name.clone();
        w.act(Action::AcceptContract(i));
        let h = hint(&w).unwrap();
        assert!(h.contains(&there) && h.contains("course is set"), "{h}");

        // With the course cleared, the board says to plot it.
        w.act(Action::PlotCourse(None));
        let h = hint(&w).unwrap();
        assert!(h.contains("PLOT COURSE"), "{h}");

        // Launched without a course: the system is a link to plot it.
        w.act(Action::Launch);
        let h = hint(&w).unwrap();
        assert!(h.contains(&format!("{{{there}}}")), "{h}");

        // On course, in the well: [J] climbs.
        w.act(Action::PlotCourse(Some(dest)));
        let h = hint(&w).unwrap();
        assert!(h.contains("[J] to climb"), "{h}");

        // In open space: [J] jumps.
        w.ship.frame = Place::Star;
        w.ship.rel = glam::DVec3::new(0.0, 5.0 * crate::world::AU, 0.0);
        w.target = None;
        assert!(w.prompts().jump.is_some());
        let h = hint(&w).unwrap();
        assert!(h.contains("[J] jumps"), "{h}");
    }

    #[test]
    fn steps_light_as_they_happen() {
        let mut w = World::new(1);
        w.act(crate::world::Action::AcceptContract(0));
        w.act(crate::world::Action::Launch);
        w.act(crate::world::Action::CycleTarget);
        w.update(0.01, Default::default());
        let p = &w.player.progress;
        assert!(p.took_work && p.launched && p.targeted, "{p:?}");
        assert!(!p.arrived && !p.earned, "{p:?}");
        assert!(hint(&w).is_some());
    }

    #[test]
    fn a_highport_in_the_same_well_is_docked_at_not_climbed_to() {
        // Just undocked from the highport above the capital (#112): it's
        // in the planet's well, so the hint is to dock, not climb out.
        let mut w = World::new(1);
        let port = w.view.stations[0];
        w.mode = Mode::Docked(port);
        w.act(crate::world::Action::Undock);
        w.target = Some(crate::world::Place::Station(port));
        assert!(w.prompts().well.is_some(), "undocked inside the well");
        let hint = hint(&w).expect("a hint");
        assert!(hint.contains("[F] to dock"), "{hint}");
    }

    #[test]
    fn selling_goods_from_an_abandoned_contract_is_not_getting_paid() {
        // Issue #127: abandon the first job and sell the food it left.
        let mut w = World::new(1);
        w.act(crate::world::Action::AcceptContract(0));
        let good = w.player.contracts[0].offer.good;
        let amount = w.player.contracts[0].offer.amount;
        w.act(crate::world::Action::AbandonContract(0));
        assert_eq!(w.player.cargo[good], amount);
        let before = w.player.credits;
        w.act(crate::world::Action::Trade(good, -amount));
        assert!(w.player.credits > before, "sold");
        assert_eq!(w.player.cargo[good], 0.0);
        assert!(!w.player.progress.earned);
        assert_eq!(w.player.progress.forfeit_t[good], 0.0);

        // Ore mined is the player's own work.
        w.player.cargo[Good::Ore] = 5.0;
        w.act(crate::world::Action::Trade(Good::Ore, -5.0));
        assert!(w.player.progress.earned, "mined ore sold");
    }

    #[test]
    fn delivering_a_contract_lights_get_paid() {
        let mut w = World::new(1);
        w.act(crate::world::Action::AcceptContract(0));
        let to = w.player.contracts[0].offer.to;
        let before = w.player.credits;
        w.mode = Mode::Docked(to);
        w.update(0.01, Default::default());
        assert!(w.player.contracts.is_empty(), "delivered");
        assert!(w.player.credits > before);
        assert!(w.player.progress.earned);
    }

    /// The inverted flight keys differ from the plain ones only in which
    /// arrow pulls the nose up (issue #128).
    #[test]
    fn inverted_flight_keys_flip_only_the_pitch_row() {
        let (plain, inverted) = (flight_keys(false), flight_keys(true));
        assert_eq!(plain.len(), inverted.len());
        let differ: Vec<_> = (0..plain.len())
            .filter(|&i| plain[i] != inverted[i])
            .collect();
        assert_eq!(differ.len(), 1);
        assert_eq!(plain[differ[0]].1, "aim: ↓ nose up · ↑ nose down");
        assert_eq!(inverted[differ[0]].1, "aim: ↑ nose up · ↓ nose down");
    }
}
