//! A tiny synthesizer (pure): [`Blip`], a sine wave sweeping from one pitch to another under a
//! short envelope (a few milliseconds of attack, then an exponential decay to silence). Played
//! by Bevy's audio as a custom audio source; the samples are computed on the fly.

use std::f32::consts::TAU;
use std::num::NonZero;
use std::time::Duration;

use bevy::audio::{ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

/// Sample rate of the synthesized sounds (Hz).
pub const SAMPLE_RATE: u32 = 44_100;
/// Attack of the envelope (s): long enough not to click.
const ATTACK: f32 = 0.004;
/// The decay reaches about 1 % of the volume at the end of the sound.
const DECAY: f32 = 4.6;
/// A touch of the second harmonic, for a rounder "pop" than a pure sine.
const HARMONIC: f32 = 0.25;

/// A synthesized sound: a sine sweep from `from_hz` to `to_hz` (exponential, like a pitch
/// bend), `duration` seconds long, peaking at `volume` (0 to 1).
#[derive(Asset, TypePath, Clone, Copy, Debug, PartialEq)]
pub struct Blip {
    pub from_hz: f32,
    pub to_hz: f32,
    pub duration: f32,
    pub volume: f32,
}

impl Blip {
    /// Number of samples.
    pub fn len(&self) -> usize {
        (self.duration.max(0.0) * SAMPLE_RATE as f32).round() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pitch at `t` seconds (Hz).
    pub fn frequency(&self, t: f32) -> f32 {
        let x = (t / self.duration.max(1e-4)).clamp(0.0, 1.0);
        self.from_hz * (self.to_hz / self.from_hz.max(1e-3)).powf(x)
    }

    /// Envelope at `t` seconds: 0 → 1 over the attack, then decays toward 0 at the end.
    pub fn envelope(&self, t: f32) -> f32 {
        if t < 0.0 || t >= self.duration {
            return 0.0;
        }
        let attack = (t / ATTACK).min(1.0);
        let decay = (-DECAY * t / self.duration.max(1e-4)).exp();
        // Fades the last bit of the decay to exactly 0 at the end.
        let tail = ((self.duration - t) / ATTACK).min(1.0);
        self.volume * attack * decay * tail
    }
}

/// The sample of `blip` at `t` seconds, with the oscillator at `phase` (radians).
pub fn blip_sample(blip: &Blip, t: f32, phase: f32) -> f32 {
    let wave = (phase.sin() + HARMONIC * (2.0 * phase).sin()) / (1.0 + HARMONIC);
    wave * blip.envelope(t)
}

/// Plays a [`Blip`]: one mono sample after the other, the oscillator's phase following the
/// sweep.
pub struct BlipDecoder {
    blip: Blip,
    index: usize,
    len: usize,
    phase: f32,
}

impl BlipDecoder {
    pub fn new(blip: Blip) -> Self {
        Self {
            blip,
            index: 0,
            len: blip.len(),
            phase: 0.0,
        }
    }
}

impl Iterator for BlipDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.index >= self.len {
            return None;
        }
        let t = self.index as f32 / SAMPLE_RATE as f32;
        let sample = blip_sample(&self.blip, t, self.phase);
        self.phase = (self.phase + TAU * self.blip.frequency(t) / SAMPLE_RATE as f32) % TAU;
        self.index += 1;
        Some(sample)
    }
}

impl Source for BlipDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.len - self.index)
    }

    fn channels(&self) -> ChannelCount {
        NonZero::<u16>::MIN
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.len as f32 / SAMPLE_RATE as f32,
        ))
    }
}

impl Decodable for Blip {
    type Decoder = BlipDecoder;

    fn decoder(&self) -> BlipDecoder {
        BlipDecoder::new(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{CROQUETTE_LOST, CROQUETTE_POP, ROULETTE_STOP, ROULETTE_TICK};

    #[test]
    fn a_blip_is_short_bounded_and_silent_at_both_ends() {
        for blip in [CROQUETTE_POP, CROQUETTE_LOST, ROULETTE_TICK, ROULETTE_STOP] {
            let samples: Vec<f32> = BlipDecoder::new(blip).collect();
            assert_eq!(samples.len(), blip.len());
            assert_eq!(
                samples.len(),
                (blip.duration * SAMPLE_RATE as f32).round() as usize
            );
            assert!(blip.duration <= 0.3, "short: {}", blip.duration);
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak <= blip.volume + 1e-4, "{peak}");
            assert!(peak > blip.volume * 0.3, "audible: {peak}");
            // No click: it starts and ends at (almost) zero.
            assert!(samples[0].abs() < 1e-3);
            assert!(samples.last().unwrap().abs() < 0.01);
            assert!(samples.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn the_pop_sweeps_up_and_the_loss_sweeps_down() {
        let pop = CROQUETTE_POP;
        assert!((pop.frequency(0.0) - pop.from_hz).abs() < 1e-2);
        assert!((pop.frequency(pop.duration) - pop.to_hz).abs() < 1e-1);
        assert!(pop.frequency(pop.duration / 2.0) > pop.from_hz);
        let lost = CROQUETTE_LOST;
        assert!(lost.frequency(lost.duration / 2.0) < lost.from_hz);
        // The decoder tells its length to the mixer.
        let decoder = BlipDecoder::new(pop);
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.sample_rate().get(), SAMPLE_RATE);
        assert_eq!(decoder.current_span_len(), Some(pop.len()));
    }
}
