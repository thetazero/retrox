//! The audio-thread side: one kira [`Sound`] that plays the score, the
//! live loops (engine, laser, drives, ambience) and the one-shot cues,
//! through a shared reverb, to the master bus. The game talks to it with
//! atomics for continuous values and a channel for cues.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Receiver;

use kira::Frame;
use kira::info::Info;
use kira::sound::{Sound, SoundData};

use super::dsp::{OnePole, Osc, RATE, Reverb, Rng, Svf, pan, soft_clip};
use super::score::{Mood, Score};
use super::sfx::{Cue, Library};

/// Continuous values the game sets; the mixer glides to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Master,
    Music,
    Effects,
    /// [`Mood`] index.
    Mood,
    /// The ship's engines are live (flying, not paused): 0 or 1.
    Flight,
    /// Main engine thrust, 0..1.
    Thrust,
    Boost,
    /// Manoeuvring thrusters, 0..1.
    Rcs,
    /// In the cockpit rather than outside: muffles and adds cabin hum.
    Cockpit,
    /// The mining laser is firing: 0 or 1.
    Laser,
    /// The beam is on rock.
    Contact,
    Heat,
    /// Fast travel drone level, and how hard it's pushing (0..1).
    Warp,
    WarpRate,
    /// Jump drive charge, 0..1 while jumping.
    Jump,
    /// Station interior ambience.
    Station,
    /// City (landed) ambience.
    City,
}

const PARAMS: usize = 17;

pub struct Shared {
    values: [AtomicU32; PARAMS],
}

impl Shared {
    pub fn new() -> Self {
        let s = Self {
            values: std::array::from_fn(|_| AtomicU32::new(0)),
        };
        s.set(Param::Master, 0.8);
        s.set(Param::Music, 0.6);
        s.set(Param::Effects, 0.8);
        s
    }

    pub fn set(&self, p: Param, v: f32) {
        self.values[p as usize].store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self, p: Param) -> f32 {
        f32::from_bits(self.values[p as usize].load(Ordering::Relaxed))
    }
}

/// Play a cue.
#[derive(Clone, Copy, Debug)]
pub struct Play {
    pub cue: Cue,
    pub gain: f32,
    pub pitch: f32,
    pub pan: f32,
}

#[derive(Clone, Copy)]
struct Voice {
    cue: usize,
    take: usize,
    pos: f32,
    step: f32,
    gain: f32,
    wet: f32,
    pan: [f32; 2],
}

pub struct Synth {
    lib: Arc<Library>,
    shared: Arc<Shared>,
    rx: Receiver<Play>,
    score: Score,
    reverb: Reverb,
    voices: Vec<Voice>,
    rng: Rng,
    smooth: [OnePole; PARAMS],
    loops: Loops,
}

#[derive(Default)]
struct Loops {
    rumble: Svf,
    turbine: [Osc; 2],
    turbine_f: Svf,
    boost: Svf,
    rcs: Svf,
    cabin: Osc,
    hum: [Osc; 3],
    hum_f: Svf,
    sizzle: Svf,
    crackle: f32,
    whine: Osc,
    drone: [Osc; 3],
    drone_f: Svf,
    shimmer: Svf,
    shimmer_lfo: Osc,
    charge: Osc,
    charge_f: Svf,
    tremolo: Osc,
    mains: [Osc; 2],
    air: Svf,
    wind: Svf,
    wind_lfo: Osc,
    muffle: [Svf; 2],
}

impl Synth {
    pub fn new(lib: Arc<Library>, shared: Arc<Shared>, rx: Receiver<Play>) -> Self {
        Self {
            lib,
            shared,
            rx,
            score: Score::new(0xD0_57E5, RATE),
            reverb: Reverb::new(RATE, 1.0),
            voices: Vec::with_capacity(32),
            rng: Rng::new(99),
            smooth: Default::default(),
            loops: Loops::default(),
        }
    }

    fn take_commands(&mut self) {
        while let Ok(p) = self.rx.try_recv() {
            let cue = p.cue.index();
            let takes = self.lib.takes[cue].len();
            let take = self.rng.below(takes);
            let voice = Voice {
                cue,
                take,
                pos: 0.0,
                step: p.pitch,
                gain: p.gain * p.cue.gain(),
                wet: p.cue.wet(),
                pan: pan(1.0, p.pan),
            };
            if self.voices.len() == self.voices.capacity() {
                self.voices.remove(0);
            }
            self.voices.push(voice);
        }
    }

    fn param(&mut self, p: Param, tau: f32, dt: f32) -> f32 {
        let v = self.shared.get(p);
        self.smooth[p as usize].glide(v, tau, dt)
    }

    /// One frame of the game's continuous sounds.
    fn loops(&mut self, dt: f32) -> ([f32; 2], [f32; 2]) {
        let flight = self.param(Param::Flight, 0.15, dt);
        let thrust = self.param(Param::Thrust, 0.25, dt);
        let boost = self.param(Param::Boost, 0.2, dt);
        let rcs = self.param(Param::Rcs, 0.06, dt);
        let cockpit = self.param(Param::Cockpit, 0.3, dt);
        let laser = self.param(Param::Laser, 0.02, dt);
        let contact = self.param(Param::Contact, 0.03, dt);
        let heat = self.param(Param::Heat, 0.2, dt);
        let warp = self.param(Param::Warp, 0.6, dt);
        let warp_rate = self.param(Param::WarpRate, 1.0, dt);
        let jump = self.param(Param::Jump, 0.05, dt);
        let station = self.param(Param::Station, 1.5, dt);
        let city = self.param(Param::City, 1.5, dt);
        let l = &mut self.loops;
        let rng = &mut self.rng;
        let mut out = [0f32; 2];
        let mut wet = [0f32; 2];

        if flight > 0.001 {
            // Main engine: a low roar and a turbine note rising with thrust.
            let roar = l.rumble.low(rng.noise(), 70.0 + 160.0 * thrust + 200.0 * boost, 0.7, dt)
                * (0.25 + 0.9 * thrust + 0.8 * boost);
            let f = 48.0 + 70.0 * thrust + 30.0 * boost;
            let t = l.turbine[0].saw(f, dt) + 0.6 * l.turbine[1].saw(f * 2.01, dt);
            let turbine = l.turbine_f.low(t, 160.0 + 900.0 * thrust * (1.0 - 0.5 * cockpit), 1.1, dt)
                * (0.05 + 0.18 * thrust);
            let hiss = l.boost.band(rng.noise(), 900.0, 0.6, dt) * boost * 0.35;
            // Thruster puffs: hissing gas.
            let puff = l.rcs.high(rng.noise(), 2200.0, 0.7, dt) * rcs * 0.12 * (1.0 - 0.5 * cockpit);
            // The cabin: fan and electrics, only heard inside.
            let cabin = (l.cabin.sine(100.0, dt) * 0.02 + l.air.low(rng.noise(), 500.0, 0.6, dt) * 0.08)
                * cockpit;
            let engine = (roar * 0.55 + turbine + hiss) * flight;
            let engine = [engine, engine];
            let muffled = [
                l.muffle[0].low(engine[0], 2200.0 - 1500.0 * cockpit, 0.7, dt),
                l.muffle[1].low(engine[1], 2200.0 - 1500.0 * cockpit, 0.7, dt),
            ];
            let p = pan(puff * flight, 0.0);
            for c in 0..2 {
                out[c] += muffled[c] + p[c] + cabin * flight;
            }
        }

        if laser > 0.001 {
            // The laser's hum, rising with heat; a sizzle where it bites.
            let f = 110.0 * (1.0 + 0.3 * heat);
            let s = l.hum[0].saw(f, dt) + l.hum[1].saw(f * 1.007, dt) + 0.5 * l.hum[2].pulse(f * 2.0, 0.3, dt);
            let hum = l.hum_f.band(s, 700.0 + 1400.0 * heat, 1.6, dt) * 0.25;
            if rng.chance(0.02 + 0.08 * contact) {
                l.crackle = rng.noise();
            }
            l.crackle *= 0.9;
            let sizzle = l.sizzle.high(rng.noise() * (0.3 + l.crackle.abs()), 2600.0, 0.7, dt) * 0.3 * contact;
            let whine = l.whine.sine(1800.0 + 2400.0 * heat, dt) * heat * heat * 0.05;
            let y = (hum + sizzle + whine) * laser;
            out[0] += y;
            out[1] += y;
            wet[0] += y * 0.15;
            wet[1] += y * 0.15;
        }

        if warp > 0.001 {
            // Fast travel: a deep chord drone and shimmer.
            let base = 41.2 * (1.0 + 0.15 * warp_rate);
            let d = l.drone[0].saw(base, dt) + l.drone[1].saw(base * 1.5, dt) + l.drone[2].saw(base * 2.003, dt);
            let lfo = l.shimmer_lfo.sine(0.18, dt);
            let drone = l.drone_f.low(d, 260.0 + 300.0 * warp_rate + 120.0 * lfo, 1.2, dt) * 0.16;
            let shimmer = l.shimmer.band(rng.noise(), 3200.0 + 1800.0 * lfo, 3.0, dt) * 0.08;
            let y = (drone + shimmer) * warp;
            out[0] += y;
            out[1] += y;
            wet[0] += y * 0.4;
            wet[1] += y * 0.4;
        }

        if jump > 0.001 {
            // The jump drive charging: rising, pulsing, until it lets go.
            let f = 60.0 + 700.0 * jump * jump;
            let trem = 0.5 + 0.5 * l.tremolo.sine(4.0 + 26.0 * jump, dt);
            let c = l.charge.saw(f, dt);
            let tone = l.charge_f.low(c, 300.0 + 3000.0 * jump, 2.0, dt) * (0.1 + 0.25 * jump) * trem;
            let swell = l.shimmer.band(rng.noise(), 900.0 + 3000.0 * jump, 1.0, dt) * 0.2 * jump;
            let y = (tone + swell) * jump.sqrt();
            out[0] += y;
            out[1] += y;
            wet[0] += y * 0.3;
            wet[1] += y * 0.3;
        }

        if station > 0.001 {
            // A station's insides: mains hum, air handlers.
            let hum = l.mains[0].sine(50.0, dt) * 0.05 + l.mains[1].sine(100.0, dt) * 0.025;
            let air = l.air.low(rng.noise(), 380.0, 0.6, dt) * 0.12;
            let y = (hum + air) * station;
            out[0] += y;
            out[1] += y;
        }
        if city > 0.001 {
            let lfo = l.wind_lfo.sine(0.09, dt);
            let wind = l.wind.band(rng.noise(), 500.0 + 300.0 * lfo, 0.9, dt) * (0.18 + 0.06 * lfo);
            out[0] += wind * city;
            out[1] += wind * city * 0.9;
        }
        (out, wet)
    }
}

impl Sound for Synth {
    fn on_start_processing(&mut self) {
        self.take_commands();
        self.score.set_mood(Mood::from_index(self.shared.get(Param::Mood) as u32));
    }

    fn process(&mut self, out: &mut [Frame], dt: f64, _info: &Info) {
        let dt = dt as f32;
        let step = RATE * dt;
        for frame in out.iter_mut() {
            let master = self.param(Param::Master, 0.05, dt);
            let music = self.param(Param::Music, 0.05, dt);
            let effects = self.param(Param::Effects, 0.05, dt);

            let (score, score_wet) = self.score.next(dt);
            let (mut fx, mut fx_wet) = self.loops(dt);
            let lib = &self.lib;
            self.voices.retain_mut(|v| {
                let buf = &lib.takes[v.cue][v.take];
                let i = v.pos as usize;
                if i + 1 >= buf.len() {
                    return false;
                }
                let frac = v.pos - i as f32;
                let x = (buf[i] + (buf[i + 1] - buf[i]) * frac) * v.gain;
                for c in 0..2 {
                    fx[c] += x * v.pan[c];
                    fx_wet[c] += x * v.pan[c] * v.wet;
                }
                v.pos += v.step * step;
                true
            });

            let send = [
                score_wet[0] * music + fx_wet[0] * effects,
                score_wet[1] * music + fx_wet[1] * effects,
            ];
            let room = self.reverb.run(send, 0.86, 0.35);
            let l = (score[0] * music + fx[0] * effects + room[0] * 0.6) * master;
            let r = (score[1] * music + fx[1] * effects + room[1] * 0.6) * master;
            *frame = Frame::new(soft_clip(l), soft_clip(r));
        }
    }

    fn finished(&self) -> bool {
        false
    }
}

/// Hands the synth to kira.
pub struct SynthData(pub Synth);

impl SoundData for SynthData {
    type Error = ();
    type Handle = ();

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        Ok((Box::new(self.0), ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render `seconds` of the synth offline.
    pub fn render(synth: &mut Synth, seconds: f32) -> Vec<Frame> {
        let mut out = vec![Frame::ZERO; (seconds * RATE) as usize];
        for chunk in out.chunks_mut(512) {
            synth.on_start_processing();
            synth.process_chunk(chunk);
        }
        out
    }

    impl Synth {
        fn process_chunk(&mut self, chunk: &mut [Frame]) {
            let dt = 1.0 / RATE;
            for frame in chunk.iter_mut() {
                let master = self.param(Param::Master, 0.05, dt);
                let (score, _) = self.score.next(dt);
                let (fx, _) = self.loops(dt);
                *frame = Frame::new(
                    soft_clip((score[0] + fx[0]) * master),
                    soft_clip((score[1] + fx[1]) * master),
                );
            }
        }
    }

    #[test]
    fn loops_are_bounded_flat_out() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let shared = Arc::new(Shared::new());
        let lib = Arc::new(Library { takes: Vec::new() });
        let mut synth = Synth::new(lib, shared.clone(), rx);
        for p in [
            Param::Flight,
            Param::Thrust,
            Param::Boost,
            Param::Rcs,
            Param::Cockpit,
            Param::Laser,
            Param::Contact,
            Param::Heat,
            Param::Warp,
            Param::WarpRate,
            Param::Jump,
            Param::Station,
            Param::City,
        ] {
            shared.set(p, 1.0);
        }
        let out = render(&mut synth, 3.0);
        let peak = out.iter().fold(0f32, |m, f| m.max(f.left.abs()).max(f.right.abs()));
        assert!(out.iter().all(|f| f.left.is_finite() && f.right.is_finite()));
        assert!(peak > 0.05 && peak < 1.0, "peak {peak}");
    }
}
