//! The score: a generative analog-synth ensemble playing live.
//!
//! Five instruments, all subtractive or FM synthesis with a little tape
//! wow on everything: a detuned-saw string pad, a filtered bass, a
//! sequencer arpeggio through a ping-pong tape delay, a theremin-like
//! lead, and glassy FM bells — plus a soft rhythm box for stations. They
//! play chord progressions in D Dorian / Aeolian (hopeful, with some
//! melancholy), a section at a time; each [`Mood`] weights the
//! instruments, tempo and progressions differently, and changing mood
//! crossfades rather than cutting. Out in space the music rests now and
//! then, so silence is part of it.

use std::f32::consts::TAU;

use super::dsp::{Adsr, Delay, OnePole, Osc, Rng, Svf, mtof};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// The title screen: the full theme.
    Title,
    /// Flying: sparse, spacious, sometimes silent.
    Space,
    /// Docked or landed: warmer, with the rhythm box.
    Station,
    /// Fast travel and jumps: the sequencer driving.
    Transit,
}

impl Mood {
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Mood::Space,
            2 => Mood::Station,
            3 => Mood::Transit,
            _ => Mood::Title,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Mood::Title => 0,
            Mood::Space => 1,
            Mood::Station => 2,
            Mood::Transit => 3,
        }
    }

    fn profile(self) -> Profile {
        match self {
            Mood::Title => Profile {
                tempo: 76.0,
                layers: [1.0, 0.7, 0.55, 0.9, 0.5, 0.0],
                bright: 0.55,
                chord_steps: 32,
                arp_every: 2,
                progressions: &[0, 3, 1, 0],
                lead_chance: 0.8,
                rest_chance: 0.0,
            },
            Mood::Space => Profile {
                tempo: 66.0,
                layers: [0.85, 0.45, 0.45, 0.65, 0.4, 0.0],
                bright: 0.35,
                chord_steps: 32,
                arp_every: 2,
                progressions: &[3, 4, 1, 0, 2],
                lead_chance: 0.45,
                rest_chance: 0.3,
            },
            Mood::Station => Profile {
                tempo: 92.0,
                layers: [0.6, 0.8, 0.45, 0.4, 0.3, 0.55],
                bright: 0.6,
                chord_steps: 16,
                arp_every: 2,
                progressions: &[5, 3, 0],
                lead_chance: 0.35,
                rest_chance: 0.0,
            },
            Mood::Transit => Profile {
                tempo: 104.0,
                layers: [0.7, 0.9, 0.8, 0.25, 0.2, 0.35],
                bright: 0.75,
                chord_steps: 16,
                arp_every: 1,
                progressions: &[2, 0, 4],
                lead_chance: 0.2,
                rest_chance: 0.0,
            },
        }
    }
}

struct Profile {
    tempo: f32,
    /// pad, bass, arp, lead, bells, drums.
    layers: [f32; 6],
    /// Filter brightness, 0..1.
    bright: f32,
    /// Sixteenths per chord.
    chord_steps: u32,
    /// Sixteenths per arpeggio note.
    arp_every: u32,
    progressions: &'static [usize],
    lead_chance: f32,
    /// Chance a section is left silent.
    rest_chance: f32,
}

#[derive(Clone, Copy)]
struct Chord {
    /// Semitones above the key's root.
    root: i32,
    /// Chord tones above the chord root.
    tones: [i32; 4],
}

const MIN: [i32; 4] = [0, 3, 7, 14];
const MAJ: [i32; 4] = [0, 4, 7, 14];
const SUS: [i32; 4] = [0, 2, 7, 12];
const MIN7: [i32; 4] = [0, 3, 7, 10];
const MAJ7: [i32; 4] = [0, 4, 7, 11];

const fn ch(root: i32, tones: [i32; 4]) -> Chord {
    Chord { root, tones }
}

/// Progressions in the key (D): i–VI–III–VII and friends.
const PROGRESSIONS: [[Chord; 4]; 6] = [
    // Hopeful: Dm Bb F C.
    [ch(0, MIN), ch(8, MAJ), ch(3, MAJ), ch(10, MAJ)],
    // Longing: Dm Gm Bb Am.
    [ch(0, MIN7), ch(5, MIN), ch(8, MAJ7), ch(7, MIN)],
    // Driving: Dm C Bb C.
    [ch(0, MIN), ch(10, MAJ), ch(8, MAJ), ch(10, SUS)],
    // Dorian lift: Dm G Dm G — the raised sixth.
    [ch(0, MIN7), ch(5, MAJ), ch(0, SUS), ch(5, MAJ7)],
    // Drift: Dsus F C Gm.
    [ch(0, SUS), ch(3, MAJ7), ch(10, MAJ), ch(5, MIN7)],
    // Home: F C Dm Bb.
    [ch(3, MAJ), ch(10, MAJ), ch(0, MIN7), ch(8, MAJ7)],
];

/// D Dorian, for the melody.
const SCALE: [i32; 7] = [0, 2, 3, 5, 7, 9, 10];
const KEY: i32 = 50;

/// Rhythms for one chord's span of the lead, in sixteenths (of 32).
const PHRASES: [&[u32]; 6] = [
    &[12, 4, 16],
    &[8, 8, 16],
    &[16, 16],
    &[6, 2, 8, 16],
    &[24, 8],
    &[4, 4, 8, 16],
];

struct Pad {
    note: f32,
    osc: [Osc; 3],
    filter: Svf,
    env: Adsr,
    pan: f32,
}

struct Mono {
    osc: [Osc; 2],
    filter: Svf,
    env: Adsr,
    fenv: f32,
    freq: OnePole,
    target: f32,
}

struct Lead {
    osc: Osc,
    tri: Osc,
    vib: Osc,
    env: Adsr,
    freq: OnePole,
    target: f32,
    age: f32,
}

#[derive(Default)]
struct Bell {
    freq: f32,
    age: f32,
    amp: f32,
    pan: f32,
    car: Osc,
    modu: Osc,
}

pub struct Score {
    rng: Rng,
    mood: Mood,
    profile: Profile,
    /// Smoothed layer gains and brightness.
    gains: [OnePole; 6],
    bright: OnePole,
    /// Overall level: rests fade to silence.
    level: OnePole,
    rest: bool,

    step_clock: f32,
    step_len: f32,
    step: u32,
    section_steps: u32,
    pending_section: bool,
    prog: usize,
    chord: usize,
    arp_on: bool,
    arp_i: usize,
    arp_notes: [f32; 8],

    lead_on: bool,
    /// The lead's notes for the current chord: (start step, note).
    phrase: [(u32, f32); 6],
    phrase_len: usize,
    last_lead: f32,

    pads: [Pad; 8],
    next_pad: usize,
    bass: Mono,
    arp: Mono,
    lead: Lead,
    bells: [Bell; 4],
    next_bell: usize,
    kick: (f32, f32),
    hat: (f32, Svf),
    lfo: Osc,
    wow: [Osc; 2],
    echo: [Delay; 2],
    echo_lp: [OnePole; 2],
    time: f32,
}

impl Score {
    pub fn new(seed: u64, rate: f32) -> Self {
        let mood = Mood::Title;
        let pads = std::array::from_fn(|i| Pad {
            note: 60.0,
            osc: [Osc::at(0.13 * i as f32), Osc::at(0.57), Osc::at(0.29)],
            filter: Svf::default(),
            env: Adsr::new(2.2, 1.0, 1.0, 3.5),
            pan: if i % 2 == 0 { -0.45 } else { 0.45 },
        });
        let mono = |a, d, s, r| Mono {
            osc: [Osc::default(), Osc::at(0.3)],
            filter: Svf::default(),
            env: Adsr::new(a, d, s, r),
            fenv: 0.0,
            freq: OnePole { z: 110.0 },
            target: 110.0,
        };
        let mut s = Self {
            rng: Rng::new(seed),
            mood,
            profile: mood.profile(),
            gains: Default::default(),
            bright: OnePole { z: 0.5 },
            level: OnePole { z: 0.0 },
            rest: false,
            step_clock: 0.0,
            step_len: 60.0 / 76.0 / 4.0,
            step: 0,
            section_steps: 0,
            pending_section: true,
            prog: 0,
            chord: 0,
            arp_on: true,
            arp_i: 0,
            arp_notes: [62.0; 8],
            lead_on: false,
            phrase: [(0, 0.0); 6],
            phrase_len: 0,
            last_lead: 69.0,
            pads,
            next_pad: 0,
            bass: mono(0.006, 0.5, 0.55, 0.3),
            arp: mono(0.002, 0.16, 0.0, 0.12),
            lead: Lead {
                osc: Osc::default(),
                tri: Osc::default(),
                vib: Osc::default(),
                env: Adsr::new(0.09, 0.6, 0.75, 0.9),
                freq: OnePole { z: 440.0 },
                target: 440.0,
                age: 0.0,
            },
            bells: Default::default(),
            next_bell: 0,
            kick: (1.0, 0.0),
            hat: (1.0, Svf::default()),
            lfo: Osc::default(),
            wow: [Osc::default(), Osc::at(0.4)],
            echo: [
                Delay::new((rate * 1.2) as usize),
                Delay::new((rate * 1.2) as usize),
            ],
            echo_lp: Default::default(),
            time: 0.0,
        };
        // Start from silence and fade in.
        for g in &mut s.gains {
            g.z = 0.0;
        }
        s
    }

    pub fn set_mood(&mut self, mood: Mood) {
        if mood != self.mood {
            self.mood = mood;
            self.profile = mood.profile();
            self.pending_section = true;
        }
    }

    fn start_section(&mut self) {
        let p = &self.profile;
        self.step_len = 60.0 / p.tempo / 4.0;
        self.prog = self.rng.pick(p.progressions);
        self.chord = 0;
        self.step = 0;
        // Each progression twice round.
        self.section_steps = p.chord_steps * 8;
        self.rest = self.rng.chance(p.rest_chance);
        self.arp_on = self.mood == Mood::Transit || self.rng.chance(0.65);
        self.lead_on = false;
        self.pending_section = false;
    }

    fn chord_now(&self) -> Chord {
        PROGRESSIONS[self.prog][self.chord % 4]
    }

    fn change_chord(&mut self) {
        let chord = self.chord_now();
        let root = KEY + chord.root;
        // Release the old pad, voice the new one into F3..A#4.
        for p in &mut self.pads {
            p.env.gate_off();
        }
        for &t in &chord.tones {
            let mut n = root + t;
            while n > 70 {
                n -= 12;
            }
            while n < 53 {
                n += 12;
            }
            let n_pads = self.pads.len();
            let pad = &mut self.pads[self.next_pad];
            self.next_pad = (self.next_pad + 1) % n_pads;
            pad.note = n as f32;
            pad.env.gate_on();
        }
        // The bass on the root, low.
        let mut b = root;
        while b > 45 {
            b -= 12;
        }
        self.bass_note(b as f32);

        // Arpeggio: chord tones over two octaves, in a shape for this chord.
        let mut notes = [0.0f32; 8];
        for (i, slot) in notes.iter_mut().enumerate() {
            let t = chord.tones[i % 4] + 12 * (i / 4) as i32;
            let mut n = root + t;
            while n < 62 {
                n += 12;
            }
            *slot = n as f32;
        }
        notes.sort_by(f32::total_cmp);
        match self.rng.below(3) {
            0 => {}
            1 => notes.reverse(),
            _ => {
                // Up-down.
                let up = notes;
                for i in 0..8 {
                    notes[i] = up[if i < 5 { i * 2 % 8 } else { 15 - i * 2 }];
                }
            }
        }
        self.arp_notes = notes;
        self.arp_i = 0;

        // Bells: a sparkle of high chord tones now and then.
        if self.rng.chance(0.45) {
            let count = 1 + self.rng.below(3);
            for k in 0..count {
                let t = chord.tones[self.rng.below(4)];
                let mut n = root + t;
                while n < 74 {
                    n += 12;
                }
                let n_bells = self.bells.len();
                let bell = &mut self.bells[self.next_bell];
                self.next_bell = (self.next_bell + 1) % n_bells;
                bell.freq = mtof(n as f32);
                bell.age = -(k as f32) * self.step_len * 3.0;
                bell.amp = 1.0 / (1.0 + k as f32 * 0.5);
                bell.pan = self.rng.range(-0.7, 0.7);
            }
        }

        // The lead: a phrase over this chord, or a rest.
        let chance = if self.lead_on {
            0.7
        } else {
            self.profile.lead_chance * 0.6
        };
        self.lead_on = self.rng.chance(chance);
        self.phrase_len = 0;
        if self.lead_on {
            let rhythm = self.rng.pick(&PHRASES);
            let span = self.profile.chord_steps as f32 / 32.0;
            let mut at = 0u32;
            let mut note = self.last_lead;
            for (i, &len) in rhythm.iter().enumerate() {
                if i == 0 {
                    // Land on the chord tone nearest where we were.
                    note = nearest(root, &chord.tones, note);
                } else {
                    let degree = self.rng.pick(&[-2, -1, -1, 1, 1, 2]);
                    note = step_scale(note, degree);
                }
                note = note.clamp(62.0, 79.0);
                self.phrase[i] = ((at as f32 * span) as u32, note);
                self.phrase_len = i + 1;
                at += len;
            }
            self.last_lead = note;
        }
    }

    fn bass_note(&mut self, note: f32) {
        self.bass.target = mtof(note);
        self.bass.env.gate_on();
        self.bass.fenv = 1.0;
    }

    /// Advance one sixteenth.
    fn tick(&mut self) {
        if self.pending_section && self.step.is_multiple_of(16) {
            self.start_section();
        }
        if self.step >= self.section_steps {
            self.start_section();
        }
        let chord_steps = self.profile.chord_steps;
        let in_chord = self.step % chord_steps;
        if in_chord == 0 {
            self.chord = (self.step / chord_steps) as usize;
            self.change_chord();
        }
        let root = KEY + self.chord_now().root;
        let bass_low = {
            let mut b = root;
            while b > 45 {
                b -= 12;
            }
            b as f32
        };

        // Bass patterns.
        match self.mood {
            Mood::Station => {
                if matches!(in_chord % 16, 6 | 10) {
                    self.bass_note(bass_low + if in_chord % 16 == 10 { 7.0 } else { 0.0 });
                }
            }
            Mood::Transit => {
                if in_chord.is_multiple_of(2) && in_chord > 0 {
                    self.bass_note(bass_low + if in_chord % 8 == 4 { 12.0 } else { 0.0 });
                }
            }
            Mood::Title | Mood::Space => {
                if in_chord == chord_steps / 2 + 4 && self.rng.chance(0.5) {
                    self.bass_note(bass_low + 7.0);
                }
            }
        }
        if in_chord % 16 == 15 && self.mood != Mood::Transit {
            self.bass.env.gate_off();
        }

        // Arpeggio.
        if self.arp_on && self.step.is_multiple_of(self.profile.arp_every) {
            let n = self.arp_notes[self.arp_i % 8];
            self.arp_i += 1;
            self.arp.target = mtof(n);
            self.arp.freq.z = self.arp.target;
            self.arp.env.gate_on();
            self.arp.fenv = 1.0;
        }

        // Lead.
        if self.lead_on {
            for &(at, note) in &self.phrase[..self.phrase_len] {
                if at == in_chord {
                    self.lead.target = mtof(note);
                    self.lead.env.gate_on();
                    self.lead.age = 0.0;
                }
            }
            if in_chord + 2 == chord_steps && self.rng.chance(0.5) {
                self.lead.env.gate_off();
            }
        } else if in_chord == 0 {
            self.lead.env.gate_off();
        }

        // Rhythm box.
        if self.gains[5].z > 0.01 || self.profile.layers[5] > 0.0 {
            let s = self.step % 16;
            let kick = match self.mood {
                Mood::Transit => s.is_multiple_of(4),
                _ => s == 0 || s == 8 || (s == 11 && self.rng.chance(0.3)),
            };
            if kick {
                self.kick = (0.0, 0.0);
            }
            if s.is_multiple_of(2) && !s.is_multiple_of(4)
                || (self.mood == Mood::Transit && s % 2 == 1)
            {
                self.hat.0 = 0.0;
            }
        }

        self.step += 1;
    }

    /// The next stereo frame: (dry, reverb send).
    pub fn next(&mut self, dt: f32) -> ([f32; 2], [f32; 2]) {
        self.step_clock += dt;
        if self.step_clock >= self.step_len {
            self.step_clock -= self.step_len;
            self.tick();
        }
        self.time += dt;
        let p = &self.profile;
        let mut gain = [0f32; 6];
        for (i, g) in self.gains.iter_mut().enumerate() {
            gain[i] = g.glide(p.layers[i], 3.0, dt);
        }
        let bright = self.bright.glide(p.bright, 4.0, dt);
        let level = self.level.glide(if self.rest { 0.0 } else { 1.0 }, 4.0, dt);

        // Tape wow and flutter, on every oscillator.
        let wow = 1.0 + 0.0018 * self.wow[0].sine(0.37, dt) + 0.0006 * self.wow[1].sine(5.3, dt);
        let lfo = self.lfo.sine(0.06, dt);

        let mut dry = [0f32; 2];
        let mut send = [0f32; 2];
        let mut add = |x: [f32; 2], wet: f32| {
            for c in 0..2 {
                dry[c] += x[c];
                send[c] += x[c] * wet;
            }
        };

        // Pad.
        for (i, pad) in self.pads.iter_mut().enumerate() {
            if !pad.env.active() {
                continue;
            }
            let env = pad.env.next(dt);
            let f = mtof(pad.note) * wow;
            let detune = 1.0 + 0.004 * (i as f32 * 0.7).sin();
            let s = pad.osc[0].saw(f * 0.996 * detune, dt)
                + pad.osc[1].saw(f * 1.004, dt)
                + 0.5 * pad.osc[2].tri(f * 0.5, dt);
            let cutoff = (380.0 + 1500.0 * bright) * (1.0 + 0.35 * lfo);
            let y = pad.filter.low(s, cutoff, 0.8, dt) * env * 0.055 * gain[0];
            add(super::dsp::pan(y, pad.pan), 0.6);
        }

        // Bass.
        {
            let b = &mut self.bass;
            let f = b.freq.glide(b.target, 0.03, dt) * wow;
            let env = b.env.next(dt);
            b.fenv *= (-dt / 0.18).exp();
            let s = 0.6 * b.osc[0].saw(f, dt) + 0.5 * b.osc[1].pulse(f * 0.5, 0.5, dt);
            let y = b
                .filter
                .low(s, 90.0 + (250.0 + 900.0 * b.fenv) * bright, 1.6, dt)
                * env
                * 0.2
                * gain[1];
            add([y, y], 0.1);
        }

        // Arpeggio, into a ping-pong tape delay.
        let arp = {
            let a = &mut self.arp;
            let f = a.target * wow;
            let env = a.env.next(dt);
            a.fenv *= (-dt / 0.11).exp();
            let s = 0.7 * a.osc[0].pulse(f, 0.32, dt) + 0.3 * a.osc[1].saw(f * 1.003, dt);
            a.filter
                .low(s, 280.0 + 3600.0 * a.fenv * (0.4 + bright), 2.6, dt)
                * env
                * 0.075
                * gain[2]
        };
        {
            let d = self.step_len * 3.0 / dt;
            let l = self.echo[0].tap(d);
            let r = self.echo[1].tap(d);
            let fl = self.echo_lp[0].low(r, 2400.0, dt);
            let fr = self.echo_lp[1].low(l, 2400.0, dt);
            self.echo[0].push(arp + fr * 0.45);
            self.echo[1].push(fl * 0.45);
            add([arp + l * 0.5, arp + r * 0.5], 0.35);
        }

        // Lead: a sine with a little triangle, gliding, vibrato blooming.
        {
            let l = &mut self.lead;
            if l.env.active() {
                l.age += dt;
                let env = l.env.next(dt);
                let depth = 0.006 * (l.age / 0.7).min(1.0);
                let vib = 1.0 + depth * l.vib.sine(5.4, dt);
                let f = l.freq.glide(l.target, 0.07, dt) * wow * vib;
                let s = l.osc.sine(f, dt) + 0.18 * l.tri.tri(f, dt);
                let y = s * env * 0.07 * gain[3];
                add(super::dsp::pan(y, 0.15), 0.9);
            }
        }

        // Bells.
        for b in &mut self.bells {
            if b.amp <= 0.0 {
                continue;
            }
            b.age += dt;
            if b.age < 0.0 {
                continue;
            }
            let f = b.freq * wow;
            let index = 2.2 * (-b.age / 0.2).exp();
            let m = b.modu.sine(f * 3.5, dt) * index;
            b.car.phase = (b.car.phase + f * dt) % 1.0;
            let y = (b.car.phase * TAU + m).sin() * (-b.age / 1.1).exp() * b.amp * 0.035 * gain[4];
            if b.age > 6.0 {
                b.amp = 0.0;
            }
            add(super::dsp::pan(y, b.pan), 0.9);
        }

        // Rhythm box: a round analog kick and a ticking hat.
        {
            let (age, phase) = &mut self.kick;
            if *age < 0.5 {
                *age += dt;
                let f = 42.0 + 90.0 * (-*age / 0.03).exp();
                *phase = (*phase + f * dt) % 1.0;
                let y = (*phase * TAU).sin() * (-*age / 0.22).exp() * 0.2 * gain[5];
                add([y, y], 0.05);
            }
            let (age, f) = &mut self.hat;
            if *age < 0.1 {
                *age += dt;
                let y = f.high(self.rng.noise(), 7000.0, 0.8, dt)
                    * (-*age / 0.025).exp()
                    * 0.04
                    * gain[5];
                add(super::dsp::pan(y, 0.3), 0.2);
            }
        }

        for c in 0..2 {
            dry[c] *= level;
            send[c] *= level;
        }
        (dry, send)
    }
}

/// The chord tone (in any octave) nearest `near`.
fn nearest(root: i32, tones: &[i32; 4], near: f32) -> f32 {
    let mut best = near;
    let mut dist = f32::MAX;
    for &t in tones {
        for oct in -2..=3 {
            let n = (root + t + 12 * oct) as f32;
            if (n - near).abs() < dist {
                dist = (n - near).abs();
                best = n;
            }
        }
    }
    best
}

/// Move `degrees` steps along the scale from `note`.
fn step_scale(note: f32, degrees: i32) -> f32 {
    let n = note.round() as i32 - KEY;
    let oct = n.div_euclid(12);
    let within = n.rem_euclid(12);
    let idx = SCALE.iter().position(|&s| s >= within).unwrap_or(0) as i32;
    let i = idx + degrees;
    let (o, k) = (i.div_euclid(7), i.rem_euclid(7));
    (KEY + 12 * (oct + o) + SCALE[k as usize]) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mood_plays_and_stays_in_range() {
        let rate = 48_000.0;
        let dt = 1.0 / rate;
        for mood in [Mood::Title, Mood::Space, Mood::Station, Mood::Transit] {
            let mut score = Score::new(7, rate);
            score.set_mood(mood);
            let mut peak = 0f32;
            let mut energy = 0f64;
            let n = (rate * 40.0) as usize;
            for _ in 0..n {
                let (dry, send) = score.next(dt);
                for x in dry.into_iter().chain(send) {
                    assert!(x.is_finite(), "{mood:?} went NaN");
                    peak = peak.max(x.abs());
                }
                energy += f64::from(dry[0] * dry[0]);
            }
            let rms = (energy / n as f64).sqrt();
            assert!(peak < 1.0, "{mood:?} peak {peak}");
            // Space may rest; the others must be heard.
            if mood != Mood::Space {
                assert!(rms > 0.01, "{mood:?} too quiet: {rms}");
            }
        }
    }

    #[test]
    fn melody_stays_on_the_scale() {
        for start in 60..80 {
            for d in -3..=3 {
                let n = step_scale(start as f32, d) as i32;
                assert!(
                    SCALE.contains(&(n - KEY).rem_euclid(12)),
                    "{start} {d} -> {n}"
                );
            }
        }
    }
}
