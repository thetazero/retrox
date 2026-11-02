//! Help for a new pilot (issue #79): the controls, as tables shared by the
//! in-flight controls card and the handbook, and the *first steps* notes,
//! a checklist that lights as the player does each thing for the first
//! time, with one hint about what to do next.
//!
//! The notes are not a scripted tutorial: nothing waits on them, and the
//! hint is read off the situation (where you are, what you carry, what
//! you're aimed at) every frame, so it stays right whatever order you do
//! things in.

use serde::{Deserialize, Serialize};

use crate::world::{Leg, Mode, Place, Screen, World};

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
    /// Sold goods or delivered a contract.
    pub earned: bool,
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
    }
}

/// A row of a controls table: the keys, and what they do.
pub type KeyRow = (&'static [&'static str], &'static str);

pub const FLIGHT_KEYS: &[KeyRow] = &[
    (&["MOUSE"], "steer (click the view first)"),
    (&["↑", "↓", "←", "→"], "steer with the keys"),
    (&["W", "S"], "throttle up / down"),
    (&["X"], "cut the throttle"),
    (&["A", "D"], "strafe"),
    (&["SPACE", "CTRL"], "rise / sink"),
    (&["Q", "E"], "roll"),
    (&["SHIFT"], "boost"),
    (&["Z"], "flight assist on / off"),
];

pub const TRAVEL_KEYS: &[KeyRow] = &[
    (&["T"], "next target"),
    (&["J"], "fast travel · again: drop out"),
    (&["F"], "dock at a station"),
    (&["L"], "land at a city"),
    (&["M"], "galaxy map"),
];

pub const WORK_KEYS: &[KeyRow] = &[
    (&["G"], "prospect: read the rocks"),
    (&["LMB", "R"], "fire guns · mining laser · gas scoop"),
    (&["Y"], "next hostile target"),
    (&["I"], "intercept the trader in your sights"),
    (&["V"], "cockpit / chase camera"),
    (&["TAB"], "take or free the mouse"),
    (&["H"], "controls card"),
    (&["ESC"], "menu, or back"),
];

/// A run of text with keys in it: `[J]` is drawn as a keycap.
pub enum Seg<'a> {
    Text(&'a str),
    Key(&'a str),
}

pub fn segments(text: &str) -> Vec<Seg<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|c| open + c) else {
            break;
        };
        if open > 0 {
            out.push(Seg::Text(&rest[..open]));
        }
        out.push(Seg::Key(&rest[open + 1..close]));
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

/// What to do next, from where things stand: one or two sentences, keys
/// in brackets. `None` while there's nothing to say (a jump under way).
pub fn hint(w: &World) -> Option<String> {
    let first = w.player.contracts.first().map(|c| {
        let s = &w.galaxy.settlements[c.offer.to];
        (c.offer.to, s.name.clone(), s.system)
    });
    let carrying = w.player.cargo_t() > 0.0;
    if w.docked_at().is_some() {
        return Some(match w.screen {
            Screen::Contracts if first.is_none() => {
                let pick = "Pick a job and press ACCEPT: the goods load into your hold, and \
                            you're paid when they arrive.";
                match nearest_job(w) {
                    Some(o) => format!(
                        "{pick} The nearest, to {} ({}), pays soonest. Then PLOT COURSE.",
                        w.galaxy.settlements[o.to].name,
                        crate::hud::jumps_text(o.jumps)
                    ),
                    None => format!("{pick} Then PLOT COURSE."),
                }
            }
            Screen::Contracts => "Cargo aboard. [ESC] back to the menu, then LAUNCH.".into(),
            Screen::Trade => "Buy what's in SURPLUS here and sell it where it's short. Ore \
                 from mining sells here too."
                .into(),
            Screen::Bounties => "Warrants pay the moment a pirate ship goes down. PLOT \
                 COURSE to a lair, fly to its field, and bring a gun that can win."
                .into(),
            Screen::Hangar | Screen::Shipyard => {
                "Refits change what your ship carries and how well it mines. [ESC] back.".into()
            }
            Screen::Hub => match &first {
                None if !carrying => "Open CONTRACTS and take a hauling job: the surest first \
                     money. Or LAUNCH and go mining."
                    .into(),
                None => "LAUNCH, and sell your cargo where it's short.".into(),
                Some((_, name, system)) if *system != w.system && w.course != Some(*system) => {
                    format!(
                        "Your cargo is for {name}, in {}. Open CONTRACTS and PLOT COURSE, then \
                         LAUNCH.",
                        w.galaxy.systems[*system].name
                    )
                }
                Some((to, _, _)) => {
                    format!("LAUNCH, and take your cargo to {}.", where_name(w, *to))
                }
            },
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
                                      convoy. Shooting it makes its faction hostile."
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
    if w.field_rocks.is_some() {
        return Some(
            "Mining: [G] to prospect, then hold [LMB] or [R] on a glowing vein until the rock \
             breaks. Fly close to scoop the ore; sell it at a station."
                .into(),
        );
    }
    let well = prompts.well.map(|p| w.name(p));
    if let Some((to, name, system)) = &first {
        if *system != w.system {
            let there = &w.galaxy.systems[*system].name;
            return Some(match (w.course == Some(*system), &well) {
                (false, _) => format!(
                    "Your cargo is for {name}, in {there}. Open the galaxy map [M] and click \
                     {there} to plot a course."
                ),
                (true, Some(well)) => format!(
                    "Jumps need open space: [J] to climb out of the gravity well of {well}."
                ),
                (true, None) => "Clear to jump: press JUMP on the ▶ lane in HYPERLANES.".to_owned(),
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
                "Your cargo is for {name}: click it in NAVIGATION, or [T] through targets."
            ));
        }
    }
    let Some(target) = w.target else {
        return Some(if first.is_none() && !carrying {
            "Pick where to go: [T] cycles targets, or click a name in NAVIGATION. Asteroid \
             fields (∴) can be mined."
                .into()
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
        let s = segments("press [J] to go, [ESC] to stop");
        let shape: Vec<String> = s
            .iter()
            .map(|s| match s {
                Seg::Text(t) => format!("t:{t}"),
                Seg::Key(k) => format!("k:{k}"),
            })
            .collect();
        assert_eq!(
            shape,
            ["t:press ", "k:J", "t: to go, ", "k:ESC", "t: to stop"]
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
}
