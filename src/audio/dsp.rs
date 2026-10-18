//! Issue #103: the synthesizer of the sounds no free pack has (pure, unit-tested): original, they
//! owe nothing to anybody.
//!
//! - **One-shots** ([`Synth`]): a [`Tone`] (an oscillator sweeping from one pitch to another,
//!   with a vibrato: beeps, chimes, the cartoon « boing », the slide whistle of a fall), a
//!   [`Noise`] (noise through a resonant band-pass whose center sweeps: whooshes, puffs,
//!   splashes, the fire of a piment) or a [`Voice`] (an animal's cry: a buzzy source following a
//!   pitch contour through two formants, with some breath). Each one is rendered **once**, when
//!   the game starts ([`render`]), into a [`SynthSound`]: playing it only reads its samples (no
//!   allocation, nothing computed while it plays).
//! - **Loops** ([`LoopSound`]): the continuous sounds, computed on the fly and never ending: the
//!   engine ([`LoopKind::Engine`], a wavetable of two periods of a small kart engine), the drift's
//!   squeal, the glider's wind and the rumble off the road. Their pitch and volume are the sink's
//!   (`AudioSinkPlayback::set_speed`, `set_volume`), changed every frame by the mixer.

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

use super::synth::SAMPLE_RATE;

const RATE: f32 = SAMPLE_RATE as f32;

/// A tiny deterministic noise source (xorshift32): the same sound every time.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform in [-1, 1].
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// A state variable filter (the « TPT » form: stable at any frequency): band-pass and low-pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    pub fn new(hz: f32, q: f32) -> Self {
        let mut filter = Self::default();
        filter.tune(hz, q);
        filter
    }

    /// Moves its center to `hz` (kept under the Nyquist frequency), with a resonance `q`.
    pub fn tune(&mut self, hz: f32, q: f32) {
        let hz = hz.clamp(20.0, RATE * 0.45);
        let g = (PI * hz / RATE).tan();
        self.k = 1.0 / q.max(0.1);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// One sample in: (band-pass at unit peak gain, low-pass).
    pub fn step(&mut self, x: f32) -> (f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (self.k * v1, v2)
    }
}

/// The waveform of a [`Tone`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    /// Pure: beeps.
    Sine,
    /// A touch of the second harmonic: rounder (the croquettes' pop).
    Round,
    /// All the harmonics, fading (a soft saw): buzzing (the bee, a kazoo).
    Buzz,
    /// The odd harmonics (a soft square): hollow, like a clarinet or a toy whistle.
    Hollow,
}

impl Wave {
    fn at(self, phase: f32) -> f32 {
        match self {
            Wave::Sine => phase.sin(),
            Wave::Round => (phase.sin() + 0.25 * (2.0 * phase).sin()) / 1.25,
            Wave::Buzz => (1..=6).map(|n| (n as f32 * phase).sin() / n as f32).sum(),
            Wave::Hollow => [1.0f32, 3.0, 5.0, 7.0]
                .iter()
                .map(|n| (n * phase).sin() / n)
                .sum(),
        }
    }
}

/// An oscillator sweeping from `from_hz` to `to_hz` (exponential, like a pitch bend) in
/// `duration` seconds; its pitch jumps by `step.1` at `step.0` seconds (a two-note chime); a
/// vibrato of `vibrato` semitones at `vibrato_hz`, fading by `vibrato_fade` per second (the
/// cartoon spring); a few milliseconds of attack, then an exponential decay (`decay`: 0 holds
/// the volume, 4.6 ends at 1 %).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub from_hz: f32,
    pub to_hz: f32,
    pub duration: f32,
    pub wave: Wave,
    pub step: Option<(f32, f32)>,
    pub vibrato: f32,
    pub vibrato_hz: f32,
    pub vibrato_fade: f32,
    pub decay: f32,
}

impl Tone {
    /// A plain sweep: no step, no vibrato, a full decay.
    pub const fn sweep(from_hz: f32, to_hz: f32, duration: f32, wave: Wave) -> Self {
        Self {
            from_hz,
            to_hz,
            duration,
            wave,
            step: None,
            vibrato: 0.0,
            vibrato_hz: 0.0,
            vibrato_fade: 0.0,
            decay: 4.6,
        }
    }
}

/// Noise through a resonant band-pass (`q`) whose center sweeps from `from_hz` to `to_hz` in
/// `duration` seconds. `swell`: where its envelope peaks (0: at once, a puff; 0.5: in the middle,
/// a whoosh passing by). `crackle`: sparse pops added (a fire's crackle, pops per second).
/// `tremolo_hz`: a pulsing (a boomerang's whup-whup; 0: none). `tone`: a share of a sine at the
/// filter's center (a whistle in the wind; 0: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub from_hz: f32,
    pub to_hz: f32,
    pub q: f32,
    pub duration: f32,
    pub swell: f32,
    pub crackle: f32,
    pub tremolo_hz: f32,
    pub tone: f32,
}

impl Noise {
    pub const fn whoosh(from_hz: f32, to_hz: f32, duration: f32, swell: f32) -> Self {
        Self {
            from_hz,
            to_hz,
            q: 1.4,
            duration,
            swell,
            crackle: 0.0,
            tremolo_hz: 0.0,
            tone: 0.0,
        }
    }
}

/// A voice (an animal's cry, GD § 14): a buzzy source (a band-limited saw) whose pitch goes from
/// `pitch[0]` to `pitch[1]` at `peak_at` (a share of the duration) and to `pitch[2]` at the end,
/// through two formants gliding from `formants_from` to `formants_to` (the vowel: « mi-a-ou »),
/// with `breath` of noise (0 to 1: a squawk is breathy), a vibrato of `vibrato` semitones at
/// 7 Hz, and a grunt (`grunt_hz`: the voice pulses, a pig's oink; 0: none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub pitch: [f32; 3],
    pub peak_at: f32,
    pub duration: f32,
    pub formants_from: [f32; 2],
    pub formants_to: [f32; 2],
    pub breath: f32,
    pub vibrato: f32,
    pub grunt_hz: f32,
}

/// A one-shot sound to synthesize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Synth {
    Tone(Tone),
    Noise(Noise),
    Voice(Voice),
}

impl Synth {
    pub fn duration(&self) -> f32 {
        match self {
            Synth::Tone(tone) => tone.duration,
            Synth::Noise(noise) => noise.duration,
            Synth::Voice(voice) => voice.duration,
        }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        (self.duration().max(0.0) * RATE).round() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The attack (a few milliseconds: no click) and the last milliseconds faded to exactly 0.
const EDGE: f32 = 0.004;

fn edges(t: f32, duration: f32) -> f32 {
    (t / EDGE).min(1.0) * ((duration - t) / EDGE).clamp(0.0, 1.0)
}

/// A band-limited saw (PolyBLEP) at `phase` (0 to 1), `dt` its step per sample.
fn saw(phase: f32, dt: f32) -> f32 {
    let blep = if phase < dt {
        let x = phase / dt;
        x + x - x * x - 1.0
    } else if phase > 1.0 - dt {
        let x = (phase - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    };
    2.0 * phase - 1.0 - blep
}

/// Exponential interpolation from `a` to `b` (`x` from 0 to 1).
fn glide(a: f32, b: f32, x: f32) -> f32 {
    a * (b / a.max(1e-3)).powf(x.clamp(0.0, 1.0))
}

/// The samples of `synth`, peaking at `volume` (0 to 1), into `out` (cleared first).
pub fn render(synth: &Synth, volume: f32, out: &mut Vec<f32>) {
    out.clear();
    let len = synth.len();
    out.reserve(len);
    let duration = synth.duration().max(1e-3);
    match *synth {
        Synth::Tone(tone) => {
            let mut phase = 0.0f32;
            for i in 0..len {
                let t = i as f32 / RATE;
                let x = t / duration;
                let mut hz = glide(tone.from_hz, tone.to_hz, x);
                if let Some((at, ratio)) = tone.step
                    && t >= at
                {
                    hz *= ratio;
                }
                let vibrato = tone.vibrato
                    * (-tone.vibrato_fade * t).exp()
                    * (TAU * tone.vibrato_hz * t).sin();
                hz *= 2f32.powf(vibrato / 12.0);
                let sample = tone.wave.at(phase) * (-tone.decay * x).exp() * edges(t, duration);
                phase = (phase + TAU * hz / RATE) % TAU;
                out.push(sample);
            }
        }
        Synth::Noise(noise) => {
            let mut rng = Rng::new(0x5eed_1e55);
            let mut filter = Svf::new(noise.from_hz, noise.q);
            let mut phase = 0.0f32;
            let swell = noise.swell.clamp(0.0, 0.95);
            let crackle = noise.crackle / RATE;
            for i in 0..len {
                let t = i as f32 / RATE;
                let x = t / duration;
                let hz = glide(noise.from_hz, noise.to_hz, x);
                if i.is_multiple_of(32) {
                    filter.tune(hz, noise.q);
                }
                let mut input = rng.next();
                if crackle > 0.0 && (rng.next() * 0.5 + 0.5) < crackle {
                    input += 6.0 * rng.next().signum();
                }
                let (band, _) = filter.step(input);
                let whistle = noise.tone * phase.sin();
                phase = (phase + TAU * hz / RATE) % TAU;
                let envelope = if x < swell {
                    (x / swell.max(1e-3)).powf(1.5)
                } else {
                    (-3.5 * (x - swell) / (1.0 - swell)).exp()
                };
                let tremolo = if noise.tremolo_hz > 0.0 {
                    0.35 + 0.65 * (PI * noise.tremolo_hz * t).sin().abs()
                } else {
                    1.0
                };
                out.push((band + whistle) * envelope * tremolo * edges(t, duration));
            }
        }
        Synth::Voice(voice) => {
            let mut rng = Rng::new(0xc41e_0001);
            let mut f1 = Svf::new(voice.formants_from[0], 5.0);
            let mut f2 = Svf::new(voice.formants_from[1], 7.0);
            let mut phase = 0.0f32;
            let peak = voice.peak_at.clamp(0.05, 0.95);
            for i in 0..len {
                let t = i as f32 / RATE;
                let x = t / duration;
                let mut hz = if x < peak {
                    glide(voice.pitch[0], voice.pitch[1], x / peak)
                } else {
                    glide(voice.pitch[1], voice.pitch[2], (x - peak) / (1.0 - peak))
                };
                hz *= 2f32.powf(voice.vibrato * (TAU * 7.0 * t).sin() / 12.0);
                if i.is_multiple_of(32) {
                    f1.tune(glide(voice.formants_from[0], voice.formants_to[0], x), 5.0);
                    f2.tune(glide(voice.formants_from[1], voice.formants_to[1], x), 7.0);
                }
                let dt = (hz / RATE).min(0.45);
                let source = saw(phase, dt) + voice.breath * rng.next();
                phase = (phase + dt) % 1.0;
                let (a, _) = f1.step(source);
                let (b, _) = f2.step(source);
                let grunt = if voice.grunt_hz > 0.0 {
                    0.3 + 0.7 * (PI * voice.grunt_hz * t).sin().abs()
                } else {
                    1.0
                };
                // Up quickly, held, and let go over the last quarter.
                let held = (x / 0.08).min(1.0) * (1.0 - ((x - 0.75) / 0.25).clamp(0.0, 1.0));
                out.push((a + 0.6 * b + 0.12 * source) * grunt * held * edges(t, duration));
            }
        }
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 1e-6 {
        let gain = volume / peak;
        for sample in out.iter_mut() {
            *sample *= gain;
        }
    }
}

/// A synthesized one-shot, rendered once ([`render`]): playing it reads its samples.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct SynthSound {
    pub samples: Arc<[f32]>,
}

impl SynthSound {
    pub fn new(synth: &Synth, volume: f32) -> Self {
        let mut samples = Vec::new();
        render(synth, volume, &mut samples);
        Self {
            samples: samples.into(),
        }
    }

    /// Its length (s).
    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / RATE
    }
}

/// Plays a [`SynthSound`]: its samples, one after the other.
pub struct SynthDecoder {
    samples: Arc<[f32]>,
    index: usize,
}

impl Iterator for SynthDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.samples.get(self.index).copied();
        self.index += 1;
        sample
    }
}

fn mono() -> ChannelCount {
    NonZero::<u16>::MIN
}

fn rate() -> SampleRate {
    NonZero::new(SAMPLE_RATE).unwrap_or(NonZero::<u32>::MIN)
}

impl Source for SynthDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.index))
    }

    fn channels(&self) -> ChannelCount {
        mono()
    }

    fn sample_rate(&self) -> SampleRate {
        rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.samples.len() as f32 / RATE))
    }
}

impl Decodable for SynthSound {
    type Decoder = SynthDecoder;

    fn decoder(&self) -> SynthDecoder {
        SynthDecoder {
            samples: self.samples.clone(),
            index: 0,
        }
    }
}

/// The continuous sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopKind {
    /// A small kart engine at [`ENGINE_HZ`] (playback speed 1): its pitch follows the revs.
    Engine,
    /// The drift's squeal: rubber on the road, wobbling.
    Squeal,
    /// The wind of the glider.
    Wind,
    /// The rumble of grass and sand under the wheels.
    Rumble,
}

/// The engine's firing frequency at playback speed 1 (Hz): a small engine at mid revs.
pub const ENGINE_HZ: f32 = 110.0;
/// The squeal's pitch at playback speed 1 (Hz).
pub const SQUEAL_HZ: f32 = 1150.0;
/// The engine's wavetable: two periods (its « putt-putt » every other firing).
const TABLE: usize = 1024;

/// A continuous sound (never ends): the engine's wavetable is built once, with the asset.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct LoopSound {
    pub kind: LoopKind,
    table: Arc<[f32]>,
}

impl LoopSound {
    pub fn new(kind: LoopKind) -> Self {
        let table: Arc<[f32]> = match kind {
            LoopKind::Engine => engine_table().into(),
            _ => Arc::from(Vec::new()),
        };
        Self { kind, table }
    }
}

/// Two periods of the engine: its harmonics, the second firing a little weaker (the beat of a
/// single cylinder), peaking at 0.8.
fn engine_table() -> Vec<f32> {
    const HARMONICS: [f32; 8] = [1.0, 0.62, 0.48, 0.3, 0.22, 0.12, 0.08, 0.05];
    let mut table: Vec<f32> = (0..TABLE)
        .map(|i| {
            // Two periods of the fundamental along the table, and its half (the beat).
            let x = i as f32 / TABLE as f32;
            let phase = TAU * 2.0 * x;
            let tone: f32 = HARMONICS
                .iter()
                .enumerate()
                .map(|(n, a)| a * ((n + 1) as f32 * phase + n as f32 * 0.7).sin())
                .sum();
            tone * (1.0 + 0.32 * (TAU * x).sin()) + 0.18 * (TAU * x).sin()
        })
        .collect();
    let peak = table.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    for sample in &mut table {
        *sample *= 0.8 / peak.max(1e-6);
    }
    table
}

/// Plays a [`LoopSound`]: one sample after the other, forever.
pub struct LoopDecoder {
    kind: LoopKind,
    table: Arc<[f32]>,
    phase: f32,
    rng: Rng,
    filter: Svf,
    wobble: f32,
    slow: f32,
    index: u32,
}

impl LoopDecoder {
    pub fn new(sound: &LoopSound) -> Self {
        let filter = match sound.kind {
            LoopKind::Engine => Svf::new(900.0, 0.7),
            LoopKind::Squeal => Svf::new(3200.0, 2.0),
            LoopKind::Wind => Svf::new(520.0, 0.9),
            LoopKind::Rumble => Svf::new(240.0, 0.7),
        };
        Self {
            kind: sound.kind,
            table: sound.table.clone(),
            phase: 0.0,
            rng: Rng::new(0x100b_0001 ^ sound.kind as u32),
            filter,
            wobble: 0.0,
            slow: 0.0,
            index: 0,
        }
    }

    fn sample(&mut self) -> f32 {
        let t = self.index as f32 / RATE;
        self.index = self.index.wrapping_add(1) % (SAMPLE_RATE * 600);
        match self.kind {
            LoopKind::Engine => {
                // Two periods along the table: it turns at half the firing frequency.
                let len = self.table.len().max(1);
                let at = self.phase * len as f32;
                let i = at as usize % len;
                let next = (i + 1) % len;
                let frac = at.fract();
                let tone = self.table.get(i).copied().unwrap_or(0.0) * (1.0 - frac)
                    + self.table.get(next).copied().unwrap_or(0.0) * frac;
                self.phase = (self.phase + ENGINE_HZ * 0.5 / RATE) % 1.0;
                let (_, rough) = self.filter.step(self.rng.next());
                tone + 0.25 * rough
            }
            LoopKind::Squeal => {
                // A wobbling pitch (smoothed noise), and some hiss.
                self.wobble += (self.rng.next() - self.wobble) * (30.0 / RATE);
                let hz =
                    SQUEAL_HZ * (1.0 + 0.05 * self.wobble * 8.0 + 0.02 * (TAU * 9.0 * t).sin());
                self.phase = (self.phase + hz / RATE) % 1.0;
                let (hiss, _) = self.filter.step(self.rng.next());
                0.55 * (TAU * self.phase).sin() + 0.2 * (TAU * 2.0 * self.phase).sin() + 0.3 * hiss
            }
            LoopKind::Wind => {
                // Gusts: the band's center and the volume drift slowly.
                self.slow += (self.rng.next() - self.slow) * (2.0 / RATE);
                if self.index.is_multiple_of(64) {
                    self.filter.tune(
                        520.0 * (1.0 + 0.35 * (TAU * 0.31 * t).sin()) * (1.0 + self.slow),
                        0.9,
                    );
                }
                let (band, _) = self.filter.step(self.rng.next());
                band * (0.75 + 0.25 * (TAU * 0.23 * t).sin()) * 1.6
            }
            LoopKind::Rumble => {
                // Low noise, shaken quickly (the bumps of the grass).
                let (_, low) = self.filter.step(self.rng.next());
                low * (0.55 + 0.45 * (TAU * 23.0 * t).sin().abs()) * 2.2
            }
        }
    }
}

impl Iterator for LoopDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        Some(self.sample().clamp(-1.0, 1.0))
    }
}

impl Source for LoopDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        mono()
    }

    fn sample_rate(&self) -> SampleRate {
        rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for LoopSound {
    type Decoder = LoopDecoder;

    fn decoder(&self) -> LoopDecoder {
        LoopDecoder::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// The main frequency of `samples` (Hz), by counting the upward zero crossings.
    fn crossings_hz(samples: &[f32]) -> f32 {
        let ups = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        ups as f32 / (samples.len() as f32 / RATE)
    }

    #[test]
    fn a_rendered_sound_has_its_length_its_peak_and_silent_ends() {
        let sounds = [
            Synth::Tone(Tone::sweep(440.0, 880.0, 0.2, Wave::Hollow)),
            Synth::Noise(Noise::whoosh(300.0, 3000.0, 0.5, 0.4)),
            Synth::Voice(Voice {
                pitch: [400.0, 600.0, 300.0],
                peak_at: 0.3,
                duration: 0.4,
                formants_from: [600.0, 1200.0],
                formants_to: [900.0, 1800.0],
                breath: 0.2,
                vibrato: 0.5,
                grunt_hz: 0.0,
            }),
        ];
        let mut out = Vec::new();
        for synth in sounds {
            render(&synth, 0.7, &mut out);
            assert_eq!(out.len(), synth.len());
            assert!((peak(&out) - 0.7).abs() < 1e-4, "{synth:?}");
            assert!(out[0].abs() < 1e-3 && out.last().unwrap().abs() < 1e-3);
            assert!(out.iter().all(|s| s.is_finite()));
        }
        // Rendered once, the same every time.
        let synth = Synth::Noise(Noise::whoosh(300.0, 3000.0, 0.5, 0.4));
        let (a, b) = (SynthSound::new(&synth, 0.5), SynthSound::new(&synth, 0.5));
        assert_eq!(a.samples, b.samples);
        assert!((a.duration() - 0.5).abs() < 1e-3);
        let decoder = a.decoder();
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.sample_rate().get(), SAMPLE_RATE);
        assert_eq!(decoder.count(), a.samples.len());
    }

    #[test]
    fn a_tone_sweeps_and_steps_to_its_pitches() {
        let mut out = Vec::new();
        let flat = Tone {
            decay: 0.0,
            ..Tone::sweep(500.0, 500.0, 0.4, Wave::Sine)
        };
        render(&Synth::Tone(flat), 0.5, &mut out);
        assert!(
            (crossings_hz(&out) - 500.0).abs() < 10.0,
            "{}",
            crossings_hz(&out)
        );
        let two_notes = Tone {
            step: Some((0.2, 2.0)),
            ..flat
        };
        render(&Synth::Tone(two_notes), 0.5, &mut out);
        let half = out.len() / 2;
        assert!((crossings_hz(&out[..half]) - 500.0).abs() < 15.0);
        assert!((crossings_hz(&out[half..]) - 1000.0).abs() < 25.0);
    }

    /// The engine never ends, stays bounded and hums at its firing frequency.
    #[test]
    fn the_loops_never_end_and_the_engine_hums_at_its_frequency() {
        for kind in [
            LoopKind::Engine,
            LoopKind::Squeal,
            LoopKind::Wind,
            LoopKind::Rumble,
        ] {
            let sound = LoopSound::new(kind);
            let decoder = sound.decoder();
            assert_eq!(decoder.total_duration(), None);
            let samples: Vec<f32> = decoder.take(SAMPLE_RATE as usize * 2).collect();
            assert_eq!(samples.len(), SAMPLE_RATE as usize * 2);
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
            assert!(rms > 0.05, "{kind:?} audible: {rms}");
        }
        // The engine's strongest pitch: its fundamental (half the table turns per period).
        let samples: Vec<f32> = LoopSound::new(LoopKind::Engine)
            .decoder()
            .take(SAMPLE_RATE as usize)
            .collect();
        let power = |hz: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in samples.iter().enumerate() {
                let a = TAU * hz * i as f32 / RATE;
                re += s * a.cos();
                im += s * a.sin();
            }
            re * re + im * im
        };
        let fundamental = power(ENGINE_HZ);
        for other in [70.0, 90.0, 130.0, 150.0, 300.0] {
            assert!(fundamental > power(other) * 4.0, "{other} Hz");
        }
    }
}
