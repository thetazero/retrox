//! Deterministic randomness for generation and the sim (DESIGN §5.1).
//!
//! Every random draw comes from a stream derived from what it's for, never
//! from a shared generator or thread order: generation uses
//! [`Rng::derive`] and [`Rng::fork`]; the sim uses [`Rng::stream`], keyed
//! by (seed, purpose, day, entity), so a draw doesn't depend on how many
//! draws happened elsewhere or in what order phases visited entities.

/// A small, fast, deterministic PRNG (SplitMix64).
///
/// Good enough for procedural generation, and fully under our control so the
/// output for a given seed never changes because a dependency did.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a, to turn stream names into numbers.
fn hash_str(s: &str) -> u64 {
    s.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
    })
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: mix(seed) }
    }

    /// An independent stream for one purpose ("names", "lanes", …), so
    /// changing how one part of generation draws numbers doesn't reshuffle
    /// every other part.
    pub fn derive(seed: u64, stream: &str) -> Self {
        Self::new(seed ^ mix(hash_str(stream)))
    }

    /// The sim's stream for one purpose, one day and one entity: e.g.
    /// `Rng::stream(state.seed, "ship-loss", day, company_bits)`.
    pub fn stream(seed: u64, purpose: &str, day: u64, entity: u64) -> Self {
        Self::derive(seed, purpose).fork(day).fork(entity)
    }

    /// An independent stream for one item, e.g. one star system.
    pub fn fork(&self, index: u64) -> Self {
        Self::new(self.state ^ mix(index.wrapping_add(GOLDEN)))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN);
        mix(self.state)
    }

    /// Uniform in `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    /// Uniform integer in `[0, n)`.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below(0)");
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// Pick an index with probability proportional to its weight.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut x = self.f64() * total;
        for (i, &w) in weights.iter().enumerate() {
            if x < w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a: Vec<u64> = (0..5).map(|_| Rng::new(1).next_u64()).collect();
        assert!(a.windows(2).all(|w| w[0] == w[1]));
        let mut x = Rng::derive(9, "names");
        let mut y = Rng::derive(9, "names");
        assert_eq!(x.next_u64(), y.next_u64());
        assert_ne!(
            Rng::derive(9, "names").next_u64(),
            Rng::derive(9, "lanes").next_u64()
        );
    }

    #[test]
    fn ranges() {
        let mut rng = Rng::new(3);
        for _ in 0..10_000 {
            let f = rng.f64();
            assert!((0.0..1.0).contains(&f));
            assert!(rng.below(7) < 7);
        }
    }
}
