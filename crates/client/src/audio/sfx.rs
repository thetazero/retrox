//! One-shot sound cues, synthesized once at startup into mono buffers.
//!
//! The palette (DESIGN §8): heavy mechanisms — clamps, breakers, relays,
//! bakelite toggles, hull plates groaning under load; radio static and
//! crackle on comms; VFD-panel beeps for instruments. No samples: every
//! cue is built from oscillators, noise and resonators, so they share one
//! analog character.

use std::f32::consts::TAU;

use super::dsp::{OnePole, Osc, RATE, Rng, Svf, decay, ramp};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// A heavy panel toggle: buttons, switches, camera.
    Switch,
    /// A softer VFD beep: menu cursor, target cycling.
    Tick,
    Confirm,
    Back,
    /// A small relay: course plotting, instrument steps.
    Relay,
    /// Teletype chatter: saved.
    Teletype,
    /// Radio static and crackle as a message comes in.
    Comms,
    /// Docking clamps closing, then the airlock equalizing.
    DockClamp,
    Undock,
    /// Hull plates groaning under load.
    HullCreak,
    /// Hitting something.
    Bump,
    /// The jump drive's breaker closing.
    JumpEngage,
    JumpArrive,
    /// Fast-travel autopilot spooling up.
    TravelEngage,
    TravelDrop,
    Land,
    Launch,
    /// A rock cracking apart under the laser.
    Fracture,
    /// A volatile lode going up.
    Blast,
    /// Ore tractored into the hold.
    Collect,
    /// The prospector's ping.
    Scan,
    /// The laser overheating and cutting out.
    Overheat,
    Cooled,
    /// Credits counted out at the market.
    Trade,
    /// A contract stamped.
    Stamp,
    /// Delivery paid: a chime.
    Delivered,
    /// The star-chart instrument's motors.
    MapOpen,
    MapClose,
    /// A distant clank somewhere in the station.
    Clank,
    /// Pneumatic tools in the hangar: a refit.
    Refit,
}

impl Cue {
    pub const ALL: [Cue; 30] = [
        Cue::Switch,
        Cue::Tick,
        Cue::Confirm,
        Cue::Back,
        Cue::Relay,
        Cue::Teletype,
        Cue::Comms,
        Cue::DockClamp,
        Cue::Undock,
        Cue::HullCreak,
        Cue::Bump,
        Cue::JumpEngage,
        Cue::JumpArrive,
        Cue::TravelEngage,
        Cue::TravelDrop,
        Cue::Land,
        Cue::Launch,
        Cue::Fracture,
        Cue::Blast,
        Cue::Collect,
        Cue::Scan,
        Cue::Overheat,
        Cue::Cooled,
        Cue::Trade,
        Cue::Stamp,
        Cue::Delivered,
        Cue::MapOpen,
        Cue::MapClose,
        Cue::Clank,
        Cue::Refit,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&c| c == self).expect("listed")
    }

    /// How many takes to render, so repeats don't sound copied.
    fn variants(self) -> usize {
        match self {
            Cue::Comms | Cue::Clank | Cue::HullCreak | Cue::Collect => 4,
            Cue::Switch | Cue::Bump | Cue::Relay | Cue::Fracture => 3,
            _ => 1,
        }
    }

    /// Mix level relative to the effects bus.
    pub fn gain(self) -> f32 {
        match self {
            Cue::Tick => 0.22,
            Cue::Confirm | Cue::Back | Cue::Cooled => 0.25,
            Cue::Relay | Cue::Teletype => 0.3,
            Cue::Switch => 0.42,
            Cue::Comms => 0.3,
            Cue::Scan => 0.42,
            Cue::Collect => 0.32,
            Cue::Clank => 0.2,
            Cue::Delivered => 0.35,
            Cue::Overheat => 0.36,
            Cue::Stamp | Cue::Trade | Cue::Refit => 0.42,
            Cue::MapOpen | Cue::MapClose => 0.4,
            Cue::HullCreak => 0.45,
            Cue::Fracture => 0.62,
            Cue::Blast => 0.72,
            Cue::JumpArrive | Cue::Land | Cue::Launch => 0.62,
            _ => 0.55,
        }
    }

    /// The share sent to the reverb: instruments dry, big events wet.
    pub fn wet(self) -> f32 {
        match self {
            Cue::Tick | Cue::Confirm | Cue::Back | Cue::Switch | Cue::Relay | Cue::Teletype => 0.08,
            Cue::Comms => 0.05,
            Cue::Scan | Cue::Delivered | Cue::Clank => 0.5,
            Cue::Fracture | Cue::Blast | Cue::JumpArrive => 0.35,
            _ => 0.2,
        }
    }
}

/// Every cue's takes, peak-normalized.
pub struct Library {
    pub takes: Vec<Vec<Vec<f32>>>,
}

impl Library {
    pub fn render() -> Self {
        let takes = Cue::ALL
            .iter()
            .map(|&cue| {
                (0..cue.variants())
                    .map(|v| normalize(render(cue, 0x5EED + v as u64 * 7919, v)))
                    .collect()
            })
            .collect();
        Self { takes }
    }
}

fn normalize(mut buf: Vec<f32>) -> Vec<f32> {
    let peak = buf.iter().fold(0f32, |m, x| m.max(x.abs()));
    if peak > 1e-6 {
        let k = 0.9 / peak;
        for x in &mut buf {
            *x *= k;
        }
    }
    // A few milliseconds' fade at the tail, so nothing ends on a click.
    let fade = (RATE * 0.004) as usize;
    let n = buf.len();
    for i in 0..fade.min(n) {
        buf[n - 1 - i] *= i as f32 / fade as f32;
    }
    buf
}

/// Run `f(t)` for `len` seconds.
fn synth(len: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (len * RATE) as usize;
    (0..n).map(|i| f(i as f32 / RATE)).collect()
}

/// Mix `other` into `buf` starting at `at` seconds, scaled by `gain`.
fn mix_in(buf: &mut Vec<f32>, other: &[f32], at: f32, gain: f32) {
    let start = (at * RATE) as usize;
    if buf.len() < start + other.len() {
        buf.resize(start + other.len(), 0.0);
    }
    for (i, x) in other.iter().enumerate() {
        buf[start + i] += x * gain;
    }
}

const DT: f32 = 1.0 / RATE;

/// A struck metal body: inharmonic partials, each `(Hz, amplitude, tau)`.
fn metal(t: f32, partials: &[(f32, f32, f32)], pitch: f32) -> f32 {
    partials
        .iter()
        .map(|&(f, a, tau)| a * decay(t, tau) * (TAU * f * pitch * t).sin())
        .sum()
}

/// A falling sine thump, `from` to `to` Hz.
fn thud(t: f32, from: f32, to: f32, tau: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    // Phase of an exponential glide, integrated.
    let k = 1.0 / (tau * 0.6);
    let phase = to * t + (from - to) * (1.0 - (-k * t).exp()) / k;
    (TAU * phase).sin() * decay(t, tau)
}

/// A burst of filtered noise as a transient.
fn transient(seed: u64, len: f32, cutoff: f32, tau: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let mut f = Svf::default();
    synth(len, |t| f.band(rng.noise(), cutoff, 0.9, DT) * decay(t, tau))
}

fn clank(seed: u64, pitch: f32, weight: f32) -> Vec<f32> {
    let parts = [
        (143.0, 0.5, 0.9),
        (227.0, 0.45, 0.6),
        (389.0, 0.35, 0.45),
        (581.0, 0.28, 0.32),
        (811.0, 0.2, 0.22),
        (1230.0, 0.14, 0.14),
        (1777.0, 0.08, 0.08),
    ];
    let mut buf = synth(1.4, |t| {
        metal(t, &parts, pitch) + weight * 0.9 * thud(t, 95.0 * pitch, 48.0 * pitch, 0.22)
    });
    mix_in(&mut buf, &transient(seed, 0.08, 2200.0, 0.012), 0.0, 1.2);
    buf
}

/// Stick-slip groaning of hull plates: an irregular pulse train ringing
/// two resonances.
fn creak(seed: u64, len: f32, low: f32, high: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let mut phase = 0.0f32;
    let (mut a, mut b) = (Svf::default(), Svf::default());
    let wander = rng.range(0.0, 6.0);
    synth(len, |t| {
        let x = t / len;
        let env = (x * std::f32::consts::PI).sin().powf(0.7);
        let rate = 32.0 + 26.0 * (TAU * 0.7 * t + wander).sin() + rng.range(-8.0, 8.0);
        phase += rate * DT;
        let mut impulse = 0.0;
        if phase >= 1.0 {
            phase -= 1.0;
            impulse = rng.range(0.4, 1.0);
        }
        let bend = 1.0 + 0.08 * (TAU * 0.45 * t).sin();
        let r = a.band(impulse, low * bend, 14.0, DT) + 0.6 * b.band(impulse, high * bend, 10.0, DT);
        r * env
    })
}

fn beep(len: f32, notes: &[(f32, f32)], bright: f32) -> Vec<f32> {
    // VFD-panel beeper: a sine with some square, through a soft low-pass.
    let mut osc = Osc::default();
    let mut lp = OnePole::default();
    synth(len, |t| {
        let Some(&(at, f)) = notes.iter().rev().find(|(at, _)| t >= *at) else {
            return 0.0;
        };
        let local = t - at;
        let next = notes
            .iter()
            .find(|(n, _)| *n > at)
            .map_or(len, |(n, _)| *n);
        let env = ramp(local, 0.003) * decay(local, 0.05).max(0.0) * (1.0 - ramp(t - next + 0.01, 0.01));
        let s = osc.sine(f, DT) * 0.7 + osc.phase.round() * bright * 0.3 - bright * 0.15;
        lp.low(s * env, 3500.0, DT)
    })
}

fn render(cue: Cue, seed: u64, variant: usize) -> Vec<f32> {
    let mut rng = Rng::new(seed ^ cue.index() as u64 * 0x1234_5678);
    let v = variant as f32;
    match cue {
        Cue::Switch => {
            let pitch = 1.0 - 0.06 * v;
            let mut buf = synth(0.14, |t| {
                0.6 * decay(t, 0.02) * (TAU * 190.0 * pitch * t).sin()
                    + 0.3 * decay(t, 0.012) * (TAU * 410.0 * pitch * t).sin()
                    + 0.08 * decay(t, 0.03) * (TAU * 2900.0 * pitch * t).sin()
                    + 0.05 * decay(t, 0.02) * (TAU * 4100.0 * pitch * t).sin()
            });
            mix_in(&mut buf, &transient(seed, 0.02, 3800.0, 0.0015), 0.0, 1.4);
            // The latch catching a moment later.
            mix_in(&mut buf, &transient(seed + 1, 0.02, 2600.0, 0.001), 0.017, 0.6);
            buf
        }
        Cue::Tick => beep(0.07, &[(0.0, 1320.0)], 0.4),
        Cue::Confirm => beep(0.22, &[(0.0, 880.0), (0.075, 1320.0)], 0.4),
        Cue::Back => beep(0.18, &[(0.0, 1320.0), (0.065, 880.0)], 0.4),
        Cue::Relay => {
            let ring = 1700.0 + v * 260.0;
            let mut buf = synth(0.07, |t| 0.25 * decay(t, 0.008) * (TAU * ring * t).sin());
            mix_in(&mut buf, &transient(seed, 0.02, 3000.0, 0.0012), 0.0, 1.0);
            mix_in(&mut buf, &transient(seed + 1, 0.02, 2400.0, 0.0010), 0.022, 0.5);
            buf
        }
        Cue::Teletype => {
            let mut buf = vec![0.0; (0.65 * RATE) as usize];
            let mut at = 0.0;
            for i in 0..9 {
                let thump = synth(0.03, |t| decay(t, 0.007) * (TAU * 310.0 * t).sin());
                mix_in(&mut buf, &thump, at, 0.5);
                mix_in(&mut buf, &transient(seed + i, 0.02, 2500.0, 0.004), at, 1.0);
                at += rng.range(0.035, 0.07);
            }
            buf
        }
        Cue::Comms => {
            let len = 0.5 + 0.08 * v;
            let mut band = Svf::default();
            let mut pops = Svf::default();
            let mut tail = Svf::default();
            let mut carrier = Osc::default();
            let mut pop = 0.0f32;
            let flutter = rng.range(17.0, 29.0);
            let squelch = len - 0.13;
            synth(len, |t| {
                let open = ramp(t, 0.012) * (1.0 - ramp(t - (len - 0.03), 0.03));
                let am = 0.65 + 0.35 * (TAU * flutter * t).sin() * (TAU * 3.1 * t).sin();
                let hiss = band.band(rng.noise(), 1600.0, 0.7, DT) * 0.45 * am;
                if rng.chance(0.0025) {
                    pop = rng.range(-1.0, 1.0);
                }
                pop *= 0.93;
                let crackle = pops.high(pop, 900.0, 0.7, DT) * 0.9;
                let tone = if t < 0.05 {
                    carrier.sine(1050.0 + 900.0 * t, DT) * 0.18 * (1.0 - t / 0.05)
                } else {
                    0.0
                };
                // The squelch closing: a "kssh" with falling cutoff.
                let s = t - squelch;
                let kssh = if s > 0.0 {
                    tail.low(rng.noise(), 6000.0 * decay(s, 0.05) + 300.0, 0.8, DT)
                        * 0.8
                        * decay(s, 0.06)
                } else {
                    0.0
                };
                (hiss + crackle + tone) * open + kssh
            })
        }
        Cue::DockClamp => {
            let mut whine = Osc::default();
            let mut wf = Svf::default();
            let mut hissf = Svf::default();
            let mut buf = synth(0.62, |t| {
                let f = 110.0 + 70.0 * ramp(t, 0.55);
                let env = ramp(t, 0.15) * (1.0 - ramp(t - 0.56, 0.05));
                wf.low(whine.saw(f, DT), 700.0, 1.2, DT) * 0.25 * env
                    + hissf.high(rng.noise(), 3500.0, 0.7, DT) * 0.08 * env
            });
            mix_in(&mut buf, &clank(seed, 1.0, 1.0), 0.55, 1.0);
            mix_in(&mut buf, &clank(seed + 3, 1.07, 0.7), 0.95, 0.7);
            let mut eq = Svf::default();
            let hiss = synth(1.6, |t| {
                eq.band(rng.noise(), 4200.0, 0.6, DT) * ramp(t, 0.05) * decay(t, 0.5) * 0.35
            });
            mix_in(&mut buf, &hiss, 1.2, 1.0);
            mix_in(&mut buf, &creak(seed + 5, 1.1, 150.0, 380.0), 1.3, 0.35);
            buf
        }
        Cue::Undock => {
            let mut buf = clank(seed, 0.85, 1.0);
            let mut vent = Svf::default();
            let hiss = synth(0.8, |t| {
                vent.band(rng.noise(), 2800.0, 0.5, DT) * ramp(t, 0.02) * decay(t, 0.25) * 0.6
            });
            mix_in(&mut buf, &hiss, 0.05, 1.0);
            mix_in(&mut buf, &clank(seed + 1, 0.92, 0.6), 0.35, 0.6);
            let mut puff = Svf::default();
            let thrust = synth(0.9, |t| {
                let env = (t / 0.9 * std::f32::consts::PI).sin();
                puff.low(rng.noise(), 500.0, 0.8, DT) * env * 0.9
            });
            mix_in(&mut buf, &thrust, 0.9, 1.0);
            buf
        }
        Cue::HullCreak => {
            let (lo, hi) = [(170.0, 420.0), (130.0, 350.0), (210.0, 510.0), (150.0, 610.0)][variant];
            let mut buf = creak(seed, 1.3 + 0.15 * v, lo, hi);
            let rumble = synth(1.3, |t| 0.15 * (TAU * 48.0 * t).sin() * (t / 1.3 * 3.14).sin());
            mix_in(&mut buf, &rumble, 0.0, 1.0);
            buf
        }
        Cue::Bump => {
            let pitch = 1.0 + 0.1 * v;
            let mut buf = synth(0.5, |t| thud(t, 80.0, 38.0, 0.16));
            let short = [(260.0, 0.4, 0.2), (433.0, 0.3, 0.12), (697.0, 0.2, 0.08)];
            let ring = synth(0.4, |t| metal(t, &short, pitch));
            mix_in(&mut buf, &ring, 0.0, 0.6);
            mix_in(&mut buf, &transient(seed, 0.05, 1200.0, 0.01), 0.0, 1.0);
            mix_in(&mut buf, &creak(seed + 2, 0.6, 160.0 * pitch, 400.0), 0.15, 0.3);
            buf
        }
        Cue::JumpEngage => {
            let mut buf = clank(seed, 0.7, 1.2);
            mix_in(&mut buf, &clank(seed + 1, 0.62, 1.0), 0.09, 0.8);
            // An arc across the breaker contacts.
            let mut hp = Svf::default();
            let arc = synth(0.35, |t| {
                let gate = if rng.chance(0.3) { 1.0 } else { 0.0 };
                hp.high(rng.noise() * gate, 2500.0, 0.7, DT) * decay(t, 0.12) * 0.5
            });
            mix_in(&mut buf, &arc, 0.0, 1.0);
            buf
        }
        Cue::JumpArrive => {
            let mut lp = Svf::default();
            let mut buf = synth(3.0, |t| {
                let roar = lp.low(rng.noise(), 900.0 * decay(t, 0.7) + 90.0, 0.9, DT)
                    * decay(t, 0.9)
                    * 1.1;
                roar + 1.2 * thud(t, 110.0, 32.0, 1.1)
            });
            // A shimmer settling: high partials falling silent.
            let shimmer = [(1863.0, 0.08, 1.2), (2417.0, 0.06, 0.9), (3109.0, 0.04, 0.7)];
            let glass = synth(1.6, |t| metal(t, &shimmer, 1.0) * ramp(t, 0.05));
            mix_in(&mut buf, &glass, 0.02, 1.0);
            mix_in(&mut buf, &creak(seed, 1.2, 140.0, 360.0), 0.8, 0.4);
            buf
        }
        Cue::TravelEngage => {
            let mut whine = Osc::default();
            let mut bp = Svf::default();
            let mut nf = Svf::default();
            let mut buf = synth(1.7, |t| {
                let f = 120.0 * 2f32.powf(t * 2.0);
                let env = ramp(t, 0.6) * (1.0 - ramp(t - 1.3, 0.35));
                let w = bp.band(whine.saw(f, DT), f * 2.0, 2.0, DT) * 0.5;
                let rush = nf.low(rng.noise(), 200.0 + 1800.0 * ramp(t, 1.4), 0.7, DT) * 0.35;
                (w + rush) * env
            });
            mix_in(&mut buf, &transient(seed, 0.02, 3000.0, 0.0012), 0.0, 0.6);
            let kick = synth(0.8, |t| thud(t, 120.0, 45.0, 0.3));
            mix_in(&mut buf, &kick, 1.25, 0.9);
            buf
        }
        Cue::TravelDrop => {
            let mut lp = Svf::default();
            let mut fall = Osc::default();
            let mut buf = synth(1.3, |t| {
                let rush = lp.low(rng.noise(), 3000.0 * decay(t, 0.3) + 150.0, 0.8, DT)
                    * decay(t, 0.45)
                    * 0.8;
                let f = 60.0 + 380.0 * decay(t, 0.25);
                rush + fall.sine(f, DT) * 0.3 * decay(t, 0.5)
            });
            let kick = synth(0.6, |t| thud(t, 90.0, 40.0, 0.2));
            mix_in(&mut buf, &kick, 0.0, 0.8);
            buf
        }
        Cue::Land => {
            let mut lp = Svf::default();
            let mut buffet = OnePole::default();
            let mut buf = synth(3.0, |t| {
                let bell = (t / 2.6 * std::f32::consts::PI).sin().max(0.0);
                let shake = buffet.low(rng.noise(), 11.0, DT) * 6.0;
                lp.low(rng.noise(), 250.0 + 1400.0 * bell, 0.7, DT)
                    * bell
                    * (0.7 + 0.3 * shake.clamp(-1.0, 1.0))
            });
            let touch = synth(0.6, |t| thud(t, 70.0, 35.0, 0.2));
            mix_in(&mut buf, &touch, 2.6, 1.0);
            mix_in(&mut buf, &clank(seed, 0.9, 0.5), 2.65, 0.4);
            buf
        }
        Cue::Launch => {
            let mut lp = Svf::default();
            let mut crackle = Svf::default();
            let mut buf = synth(2.6, |t| {
                let env = ramp(t, 0.15) * decay(t - 0.3, 0.9).min(1.0);
                let roar = lp.low(rng.noise(), 600.0, 0.7, DT);
                let pop = if rng.chance(0.004) { rng.noise() * 3.0 } else { 0.0 };
                (roar + crackle.band(pop, 1200.0, 2.0, DT)) * env
            });
            let kick = synth(0.6, |t| thud(t, 85.0, 38.0, 0.25));
            mix_in(&mut buf, &kick, 0.0, 1.0);
            buf
        }
        Cue::Fracture => {
            let mut buf = synth(0.6, |t| 1.1 * thud(t, 85.0 - 8.0 * v, 34.0, 0.32));
            mix_in(&mut buf, &transient(seed, 0.08, 1400.0, 0.006), 0.0, 2.5);
            mix_in(&mut buf, &transient(seed + 1, 0.12, 900.0, 0.04), 0.0, 1.0);
            let mut rumble = Svf::default();
            let tail = synth(1.0, |t| rumble.low(rng.noise(), 160.0, 0.7, DT) * decay(t, 0.4) * 1.5);
            mix_in(&mut buf, &tail, 0.0, 1.0);
            // Debris pattering off the hull, thinning out.
            for i in 0..40 {
                let at = 0.05 + rng.unit().powf(1.8) * 1.2;
                let cutoff = rng.range(1200.0, 4200.0);
                let grain = transient(seed + 10 + i, 0.02, cutoff, 0.002);
                mix_in(&mut buf, &grain, at, rng.range(0.1, 0.4) * (1.0 - at / 1.4));
            }
            buf
        }
        Cue::Blast => {
            let mut lp = Svf::default();
            let mut buf = synth(2.6, |t| {
                let roar = lp.low(rng.noise(), 4000.0 * decay(t, 0.25) + 120.0, 0.8, DT)
                    * decay(t, 0.7)
                    * 1.4;
                super::dsp::soft_clip(roar + 1.4 * thud(t, 70.0, 24.0, 0.8))
            });
            for i in 0..70 {
                let at = 0.03 + rng.unit().powf(2.0) * 1.8;
                let grain = transient(seed + 10 + i, 0.02, rng.range(900.0, 4000.0), 0.002);
                mix_in(&mut buf, &grain, at, rng.range(0.1, 0.35) * (1.0 - at / 2.0));
            }
            buf
        }
        Cue::Collect => {
            let pitch = 1.0 + 0.07 * v;
            let small = [(620.0, 0.3, 0.07), (981.0, 0.2, 0.05), (1373.0, 0.12, 0.04)];
            let mut buf = synth(0.4, |t| {
                0.7 * thud(t, 230.0 * pitch, 110.0, 0.06) + metal(t, &small, pitch)
            });
            for i in 0..4 {
                let at = 0.06 + i as f32 * rng.range(0.03, 0.06);
                mix_in(&mut buf, &transient(seed + i, 0.02, 1800.0, 0.003), at, 0.4);
            }
            buf
        }
        Cue::Scan => {
            let mut osc = Osc::default();
            let ping = synth(0.9, |t| {
                let f = 1180.0 - 60.0 * ramp(t, 0.6);
                let s = osc.sine(f, DT) + 0.25 * (TAU * 2.0 * f * t).sin();
                s * ramp(t, 0.003) * decay(t, 0.22)
            });
            let mut buf = synth(0.4, |t| 0.6 * thud(t, 140.0, 70.0, 0.12));
            mix_in(&mut buf, &ping, 0.0, 1.0);
            let mut lp = OnePole::default();
            let soft: Vec<f32> = ping.iter().map(|&x| lp.low(x, 1800.0, DT)).collect();
            for (k, g) in [(0.38, 0.35), (0.76, 0.16), (1.14, 0.07)] {
                mix_in(&mut buf, &soft, k, g);
            }
            buf
        }
        Cue::Overheat => {
            let mut buf = clank(seed, 1.25, 0.4);
            for (i, f) in [520.0, 390.0, 520.0].into_iter().enumerate() {
                let mut osc = Osc::default();
                let mut lp = Svf::default();
                let tone = synth(0.12, |t| {
                    let env = ramp(t, 0.004) * (1.0 - ramp(t - 0.105, 0.015));
                    lp.low(osc.pulse(f, 0.5, DT), 1800.0, 0.8, DT) * env * 0.7
                });
                mix_in(&mut buf, &tone, 0.08 + i as f32 * 0.17, 1.0);
            }
            buf
        }
        Cue::Cooled => beep(0.2, &[(0.0, 660.0), (0.07, 990.0)], 0.1),
        Cue::Trade => {
            let mut buf = vec![0.0; (0.7 * RATE) as usize];
            for i in 0..6 {
                let ring = synth(0.03, |t| 0.3 * decay(t, 0.006) * (TAU * 2100.0 * t).sin());
                mix_in(&mut buf, &ring, i as f32 * 0.032, 1.0);
                mix_in(&mut buf, &transient(seed + i, 0.02, 3200.0, 0.0012), i as f32 * 0.032, 0.8);
            }
            let chunk = synth(0.5, |t| {
                0.8 * thud(t, 160.0, 80.0, 0.06) + 0.18 * decay(t, 0.25) * (TAU * 1890.0 * t).sin()
            });
            mix_in(&mut buf, &chunk, 0.21, 1.0);
            buf
        }
        Cue::Stamp => {
            let mut buf = synth(0.45, |t| thud(t, 130.0, 70.0, 0.05));
            mix_in(&mut buf, &transient(seed, 0.06, 1500.0, 0.02), 0.0, 1.0);
            let mut hp = Svf::default();
            let paper = synth(0.18, |t| hp.high(rng.noise(), 3500.0, 0.7, DT) * (t / 0.18 * 3.14).sin() * 0.15);
            mix_in(&mut buf, &paper, 0.1, 1.0);
            buf
        }
        Cue::Delivered => {
            // A three-note bell: two-operator FM, a little glassy.
            let mut buf = vec![0.0; (1.5 * RATE) as usize];
            for (i, note) in [74.0, 78.0, 81.0].into_iter().enumerate() {
                let f = super::dsp::mtof(note);
                let bell = synth(1.2, |t| {
                    let m = (TAU * f * 3.5 * t).sin() * 1.8 * decay(t, 0.15);
                    (TAU * f * t + m).sin() * decay(t, 0.45) * ramp(t, 0.002)
                });
                mix_in(&mut buf, &bell, i as f32 * 0.11, 0.5);
            }
            buf
        }
        Cue::MapOpen | Cue::MapClose => {
            let opening = cue == Cue::MapOpen;
            let len = if opening { 1.0 } else { 0.75 };
            let mut motor = Osc::default();
            let mut gear = Osc::default();
            let mut lp = Svf::default();
            let mut buf = synth(len, |t| {
                let x = ramp(t, len * 0.9);
                let f = if opening { 70.0 + 70.0 * x } else { 140.0 - 80.0 * x };
                let env = ramp(t, 0.06) * (1.0 - ramp(t - len * 0.82, len * 0.15));
                let teeth = 0.6 + 0.4 * gear.pulse(f / 7.0, 0.3, DT);
                lp.low(motor.saw(f, DT), 900.0, 1.4, DT) * env * teeth * 0.6
            });
            mix_in(&mut buf, &transient(seed, 0.02, 3000.0, 0.0012), 0.0, 0.8);
            mix_in(&mut buf, &clank(seed + 1, 1.6, 0.6), len * 0.85, 0.35);
            buf
        }
        Cue::Clank => {
            let raw = clank(seed, 0.75 + 0.15 * v, 0.6);
            let mut lp = Svf::default();
            let mut buf: Vec<f32> = raw.iter().map(|&x| lp.low(x, 700.0, 0.7, DT)).collect();
            if variant % 2 == 1 {
                let mut hiss = Svf::default();
                let vent = synth(1.2, |t| {
                    hiss.band(rng.noise(), 1800.0, 0.5, DT) * ramp(t, 0.1) * decay(t, 0.4) * 0.3
                });
                mix_in(&mut buf, &vent, 0.3, 1.0);
            }
            buf
        }
        Cue::Refit => {
            let mut hp = Svf::default();
            let mut buf = synth(0.5, |t| {
                let gate = 0.5 + 0.5 * (TAU * 38.0 * t).sin();
                hp.band(rng.noise(), 1500.0, 1.5, DT) * gate * ramp(t, 0.02) * (1.0 - ramp(t - 0.4, 0.1))
            });
            mix_in(&mut buf, &clank(seed, 1.15, 0.6), 0.48, 0.7);
            buf
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_renders_audible_and_clean() {
        let lib = Library::render();
        for &cue in &Cue::ALL {
            for take in &lib.takes[cue.index()] {
                assert!(!take.is_empty(), "{cue:?} is empty");
                assert!(take.len() < (RATE * 4.0) as usize, "{cue:?} is too long");
                assert!(take.iter().all(|x| x.is_finite()), "{cue:?} has NaN");
                let peak = take.iter().fold(0f32, |m, x| m.max(x.abs()));
                assert!(peak > 0.5 && peak <= 0.91, "{cue:?} peak {peak}");
                let rms = (take.iter().map(|x| x * x).sum::<f32>() / take.len() as f32).sqrt();
                assert!(rms > 0.005, "{cue:?} is nearly silent: rms {rms}");
            }
        }
    }
}
