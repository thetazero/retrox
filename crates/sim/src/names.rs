//! Invented names: short words from syllables, for star systems, regions,
//! people and anything else that needs one.

use std::collections::HashSet;

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
