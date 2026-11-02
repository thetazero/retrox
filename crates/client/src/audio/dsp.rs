//! Building blocks for the synthesizer: oscillators, filters, envelopes,
//! a tape delay and a plate-ish reverb. Everything is mono `f32` at
//! whatever rate the caller steps it with, and allocates only when built.

use std::f32::consts::{PI, TAU};

/// The rate cues are rendered at; the mixer resamples if the device differs.
pub const RATE: f32 = 48_000.0;

/// MIDI note number to Hz.
pub fn mtof(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Gentle saturation, like an overdriven tape or transistor stage.
pub fn soft_clip(x: f32) -> f32 {
    // A rational tanh: cheap, smooth, and flat past ±3.
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// A small fast generator for noise and choices.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [-1, 1): white noise.
    pub fn noise(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    pub fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f32) as usize).min(n.saturating_sub(1))
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

/// A phase accumulator with band-limited waveforms (PolyBLEP).
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f32,
}

fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

impl Osc {
    pub fn at(phase: f32) -> Self {
        Self { phase }
    }

    fn step(&mut self, freq: f32, dt: f32) -> f32 {
        let inc = (freq * dt).clamp(0.0, 0.5);
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        inc
    }

    pub fn sine(&mut self, freq: f32, dt: f32) -> f32 {
        self.step(freq, dt);
        (self.phase * TAU).sin()
    }

    pub fn saw(&mut self, freq: f32, dt: f32) -> f32 {
        let inc = self.step(freq, dt);
        2.0 * self.phase - 1.0 - blep(self.phase, inc)
    }

    /// A pulse wave with duty `width` (0.5 is square).
    pub fn pulse(&mut self, freq: f32, width: f32, dt: f32) -> f32 {
        let inc = self.step(freq, dt);
        let t = self.phase;
        let mut v = if t < width { 1.0 } else { -1.0 };
        v += blep(t, inc);
        v -= blep((t - width + 1.0) % 1.0, inc);
        v
    }

    pub fn tri(&mut self, freq: f32, dt: f32) -> f32 {
        self.step(freq, dt);
        let t = self.phase;
        4.0 * (t - 0.5).abs() - 1.0
    }
}

/// A state-variable filter (Simper's trapezoidal form): stable when swept.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    /// `q` around 0.7 is flat; higher rings.
    pub fn run(&mut self, x: f32, cutoff: f32, q: f32, dt: f32) -> SvfOut {
        let fc = cutoff.clamp(10.0, 0.45 / dt);
        let g = (PI * fc * dt).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: x - k * v1 - v2,
        }
    }

    pub fn low(&mut self, x: f32, cutoff: f32, q: f32, dt: f32) -> f32 {
        self.run(x, cutoff, q, dt).low
    }

    pub fn band(&mut self, x: f32, cutoff: f32, q: f32, dt: f32) -> f32 {
        self.run(x, cutoff, q, dt).band
    }

    pub fn high(&mut self, x: f32, cutoff: f32, q: f32, dt: f32) -> f32 {
        self.run(x, cutoff, q, dt).high
    }
}

/// A one-pole smoother / low-pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    pub z: f32,
}

impl OnePole {
    pub fn low(&mut self, x: f32, cutoff: f32, dt: f32) -> f32 {
        let a = 1.0 - (-TAU * cutoff * dt).exp();
        self.z += (x - self.z) * a;
        self.z
    }

    /// Glide toward `x` with time constant `tau` seconds.
    pub fn glide(&mut self, x: f32, tau: f32, dt: f32) -> f32 {
        let a = 1.0 - (-dt / tau.max(1e-4)).exp();
        self.z += (x - self.z) * a;
        self.z
    }
}

/// Attack / decay / sustain / release, linear attack and exponential rest.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    level: f32,
    stage: Stage,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

impl Adsr {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack,
            decay,
            sustain,
            release,
            level: 0.0,
            stage: Stage::Idle,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    pub fn next(&mut self, dt: f32) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += dt / self.attack.max(1e-4);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let a = 1.0 - (-dt / (self.decay.max(1e-4) / 4.0)).exp();
                self.level += (self.sustain - self.level) * a;
            }
            Stage::Release => {
                let a = 1.0 - (-dt / (self.release.max(1e-4) / 5.0)).exp();
                self.level -= self.level * a;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// `e^(-t / tau)`, for one-shot envelopes.
pub fn decay(t: f32, tau: f32) -> f32 {
    if t < 0.0 { 0.0 } else { (-t / tau).exp() }
}

/// A linear ramp from 0 at `t = 0` to 1 at `t = len`.
pub fn ramp(t: f32, len: f32) -> f32 {
    (t / len).clamp(0.0, 1.0)
}

/// A delay line with a fractional, interpolated tap.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

impl Delay {
    pub fn new(max_samples: usize) -> Self {
        Self {
            buf: vec![0.0; max_samples.max(2)],
            pos: 0,
        }
    }

    /// The sample `delay` samples ago.
    pub fn tap(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(1.0, (n - 2) as f32);
        let i = d.floor() as usize;
        let frac = d - i as f32;
        let a = self.buf[(self.pos + n - i) % n];
        let b = self.buf[(self.pos + n - i - 1) % n];
        a + (b - a) * frac
    }

    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }
}

/// A Freeverb-style room: parallel damped combs into series all-passes,
/// with the right channel's lines a little longer for width.
pub struct Reverb {
    combs: [[Comb; 6]; 2],
    passes: [[AllPass; 3]; 2],
}

struct Comb {
    line: Delay,
    len: usize,
    damp: f32,
}

struct AllPass {
    line: Delay,
    len: usize,
}

impl Reverb {
    /// Tuned for `rate`; `size` scales the lines (1.0 is a large hall).
    pub fn new(rate: f32, size: f32) -> Self {
        let scale = rate / 44_100.0 * size;
        let comb_lens = [1557, 1617, 1491, 1422, 1277, 1356];
        let pass_lens = [556, 441, 341];
        let combs = std::array::from_fn(|ch| {
            std::array::from_fn(|i| {
                let len = ((comb_lens[i] + ch * 23) as f32 * scale) as usize;
                Comb {
                    line: Delay::new(len + 2),
                    len,
                    damp: 0.0,
                }
            })
        });
        let passes = std::array::from_fn(|ch| {
            std::array::from_fn(|i| {
                let len = ((pass_lens[i] + ch * 23) as f32 * scale) as usize;
                AllPass {
                    line: Delay::new(len + 2),
                    len,
                }
            })
        });
        Self { combs, passes }
    }

    /// One stereo frame in, the wet signal out. `feedback` ~0.84 is long.
    pub fn run(&mut self, input: [f32; 2], feedback: f32, damping: f32) -> [f32; 2] {
        let mut out = [0.0; 2];
        let mono = (input[0] + input[1]) * 0.5;
        for ch in 0..2 {
            let x = mono * 0.3 + input[ch] * 0.2;
            let mut acc = 0.0;
            for c in &mut self.combs[ch] {
                let y = c.line.tap(c.len as f32);
                c.damp = y * (1.0 - damping) + c.damp * damping;
                c.line.push(x + c.damp * feedback);
                acc += y;
            }
            for a in &mut self.passes[ch] {
                let delayed = a.line.tap(a.len as f32);
                let y = -acc + delayed;
                a.line.push(acc + delayed * 0.5);
                acc = y;
            }
            out[ch] = acc * 0.25;
        }
        out
    }
}

/// Constant-power pan: -1 left, +1 right.
pub fn pan(x: f32, p: f32) -> [f32; 2] {
    let a = (p.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    [x * a.cos(), x * a.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_oscillators_stay_bounded() {
        let dt = 1.0 / RATE;
        let mut osc = Osc::default();
        let mut svf = Svf::default();
        let mut peak = 0f32;
        for i in 0..48_000 {
            let f = 40.0 + i as f32 * 0.4;
            let x = osc.saw(f, dt);
            let y = svf.low(x, 200.0 + i as f32 * 0.3, 4.0, dt);
            assert!(y.is_finite());
            peak = peak.max(y.abs());
        }
        assert!(peak > 0.3 && peak < 6.0, "peak {peak}");
    }

    #[test]
    fn adsr_runs_its_course() {
        let mut env = Adsr::new(0.01, 0.1, 0.5, 0.1);
        env.gate_on();
        let dt = 1.0 / 1000.0;
        let mut top = 0f32;
        for _ in 0..200 {
            top = top.max(env.next(dt));
        }
        assert!((top - 1.0).abs() < 1e-3);
        env.gate_off();
        for _ in 0..1000 {
            env.next(dt);
        }
        assert!(!env.active());
    }
}
