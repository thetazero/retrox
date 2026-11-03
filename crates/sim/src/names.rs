//! Invented names: short words from syllables for star systems, regions
//! and people, and faction names built on them.

use std::collections::HashSet;

use crate::EconomyModel;
use crate::rng::Rng;

const ONSETS: &[&str] = &[
    "", "", "b", "c", "d", "f", "g", "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "br", "cr",
    "dr", "gr", "pr", "st", "tr", "th", "sel", "al", "or",
];
const VOWELS: &[&str] = &["a", "e", "i", "o", "u", "a", "e", "o", "ae", "io"];
const CODAS: &[&str] = &["", "", "", "", "n", "r", "s", "l", "m", "x", "th", "nd"];
const SUFFIXES: &[&str] = &[
    "", "", "", "", "", "", " Prime", " Major", " Minor", " Gate", " Reach", " Deep",
];

pub fn word(rng: &mut Rng) -> String {
    let syllables = if rng.chance(0.7) { 2 } else { 3 };
    let mut w = String::new();
    for i in 0..syllables {
        w.push_str(rng.pick(ONSETS));
        w.push_str(rng.pick(VOWELS));
        if i == syllables - 1 || rng.chance(0.2) {
            w.push_str(rng.pick(CODAS));
        }
    }
    let mut chars = w.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => w,
    }
}

/// `count` distinct names.
pub fn generate_unique(rng: &mut Rng, count: usize) -> Vec<String> {
    let mut seen = HashSet::with_capacity(count);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let name = format!("{}{}", word(rng), rng.pick(SUFFIXES));
        if (4..=13).contains(&name.len()) && seen.insert(name.clone()) {
            out.push(name);
        }
    }
    out
}

/// A person's name: a given name and a family name.
pub fn person(rng: &mut Rng) -> String {
    format!("{} {}", word(rng), word(rng))
}

/// The first word of a place name, without suffixes like " Prime": what a
/// faction named after the place builds on.
pub fn stem(place: &str) -> &str {
    place.split(' ').next().unwrap_or(place)
}

/// A faction name built on `stem`, in the style of its economy.
pub fn faction_name(rng: &mut Rng, stem: &str, economy: EconomyModel) -> String {
    let templates: &[&str] = match economy {
        EconomyModel::Planned => &[
            "{} Commonwealth",
            "Union of {}",
            "{} Cooperative",
            "{} Directorate",
            "Collective of {}",
            "{} Concord",
        ],
        EconomyModel::Market => &[
            "{} Mercantile League",
            "{} Compact",
            "Free Ports of {}",
            "{} Trade Federation",
            "{} Consortium",
            "{} Exchange",
        ],
    };
    rng.pick(templates).replace("{}", stem)
}

/// The name of the one settlement a whole undivided body can hold (a moon
/// base, a camp on a barren world, `Site::Surface`): a place name of its
/// own, like a region's, so it reads "Kiolgristio, on Mufath III" and is
/// never mistaken for the body it's on. Fixed per body and galaxy seed, so
/// whoever settles the body later founds a place of the same name.
pub fn surface_settlement(seed: u64, body: crate::BodyId) -> String {
    let mut rng = Rng::derive(seed, "surface-names").fork(body.index() as u64);
    loop {
        let name = word(&mut rng);
        if (4..=11).contains(&name.len()) {
            return name;
        }
    }
}
