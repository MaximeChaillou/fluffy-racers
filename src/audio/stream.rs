//! Issue #104: a piece of music played as it is decoded (streaming): only the file's bytes are in
//! memory (an OGG Vorbis file, a few hundred kilobytes a minute), never its decoded samples.
//!
//! Bevy's own loop (`PlaybackMode::Loop`) keeps every decoded sample of the first pass to play it
//! again (rodio's `repeat_infinite` buffers its source): some 40 MB for two minutes of stereo. A
//! [`MusicStream`] loops by starting its decoder again from the bytes ([`Looper`]), exactly at its
//! length (`seconds` in `music.ron`, measured: what the decoder may add after it is left out),
//! and smooths the step between the last sample of a pass and the first of the next one over a
//! couple of milliseconds (no click, whatever the file). A jingle plays once. A jingle before the
//! music (its `intro`: the last lap's, the finish's) is played by the same stream, the music
//! starting on its last sample: no gap, no overlap, as the composer chained them.

use bevy::audio::{AudioSource, ChannelCount, Decodable, SampleRate, Source};
use bevy::prelude::*;

/// Frames over which the step at a loop's seam is smoothed out (about 2 ms at 44.1 kHz).
pub const SEAM_FRAMES: u32 = 96;
/// The most channels a piece may have.
const MAX_CHANNELS: usize = 8;

/// A piece of music, from its file (an `AudioSource` loaded by Bevy: its bytes), after its intro
/// (a jingle's file, and its gain against the piece: their levels).
#[derive(Asset, TypePath, Clone, Debug)]
pub struct MusicStream {
    pub source: AudioSource,
    /// The length that loops (s): the whole piece, again and again. `None`: played once.
    pub looping: Option<f64>,
    pub intro: Option<(AudioSource, f32)>,
}

/// Whether Bevy's decoder can read `source`: an OGG file (it would stop the game on anything
/// else: a broken download is left silent instead).
pub fn is_ogg(source: &AudioSource) -> bool {
    source.bytes.starts_with(b"OggS")
}

impl MusicStream {
    /// A stream of `source` (an OGG file, or nothing).
    pub fn new(source: &AudioSource, looping: Option<f64>) -> Option<Self> {
        is_ogg(source).then(|| Self {
            source: source.clone(),
            looping,
            intro: None,
        })
    }

    /// The same, after `intro` at `gain` (left out if it is not an OGG file).
    pub fn after(mut self, intro: &AudioSource, gain: f32) -> Self {
        self.intro = is_ogg(intro).then(|| (intro.clone(), gain));
        self
    }
}

/// The decoder of a source, made again for each pass of a loop.
pub type Fresh<S> = Box<dyn Fn() -> S + Send>;

/// Plays its `intro` (once, at its gain), then `inner`, then (looping) a fresh one from the start,
/// forever: each pass exactly `pass` samples long (or shorter if the decoder ends first), each
/// seam smoothed. Not looping: once.
pub struct Looper<S: Source + Send> {
    intro: Option<(S, f32)>,
    inner: S,
    fresh: Fresh<S>,
    channels: ChannelCount,
    rate: SampleRate,
    /// Samples (every channel) of a pass, and those left in this one.
    pass: Option<u64>,
    left: u64,
    /// The channel of the next sample.
    channel: usize,
    /// The last sample out of each channel.
    last: [f32; MAX_CHANNELS],
    /// The step at the seam, for each channel (taken on the first frame after it)...
    step: [f32; MAX_CHANNELS],
    /// ...fading out over these frames.
    seam_left: u32,
    /// The first frame after a seam is being read: its steps are taken.
    first_frame: bool,
    /// Samples out of `inner` in this pass.
    out: u64,
    ended: bool,
}

impl<S: Source + Send> Looper<S> {
    /// `intro` (if it has the same channels and rate), then `inner` played through, then again
    /// from `fresh()` if `looping` (s) is some (and not 0).
    pub fn new(inner: S, fresh: Fresh<S>, looping: Option<f64>, intro: Option<(S, f32)>) -> Self {
        let channels = inner.channels();
        let rate = inner.sample_rate();
        let pass = looping
            .map(|seconds| {
                (seconds.max(0.0) * f64::from(rate.get())).round() as u64
                    * u64::from(channels.get())
            })
            .filter(|samples| *samples > 0);
        let intro =
            intro.filter(|(intro, _)| intro.channels() == channels && intro.sample_rate() == rate);
        Self {
            intro,
            inner,
            fresh,
            channels,
            rate,
            pass,
            left: pass.unwrap_or(0),
            channel: 0,
            last: [0.0; MAX_CHANNELS],
            step: [0.0; MAX_CHANNELS],
            seam_left: 0,
            first_frame: false,
            out: 0,
            ended: false,
        }
    }

    /// A seam: the next frame is smoothed from the last one out.
    fn seam(&mut self) {
        self.channel = 0;
        self.seam_left = SEAM_FRAMES;
        self.first_frame = true;
    }

    fn restart(&mut self) {
        self.inner = (self.fresh)();
        self.left = self.pass.unwrap_or(0);
        self.out = 0;
        self.seam();
    }

    /// A sample out, smoothed after a seam.
    fn emit(&mut self, sample: f32) -> f32 {
        let channels = usize::from(self.channels.get()).min(MAX_CHANNELS);
        let channel = self.channel.min(MAX_CHANNELS - 1);
        let mut out = sample;
        if self.seam_left > 0 {
            if self.first_frame {
                self.step[channel] = self.last[channel] - sample;
            }
            out += self.step[channel] * self.seam_left as f32 / SEAM_FRAMES as f32;
        }
        self.last[channel] = out;
        self.channel += 1;
        if self.channel >= channels {
            self.channel = 0;
            self.first_frame = false;
            self.seam_left = self.seam_left.saturating_sub(1);
        }
        out
    }
}

impl<S: Source + Send> Iterator for Looper<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if self.ended {
                return None;
            }
            if let Some((intro, gain)) = &mut self.intro {
                let gain = *gain;
                if let Some(sample) = intro.next() {
                    return Some(self.emit(sample * gain));
                }
                // The intro is over: the music, from its first sample.
                self.intro = None;
                self.seam();
                continue;
            }
            if self.pass.is_some() && self.left == 0 {
                self.restart();
            }
            match self.inner.next() {
                Some(sample) => {
                    self.left = self.left.saturating_sub(1);
                    self.out += 1;
                    return Some(self.emit(sample));
                }
                // Played once, or nothing came out of this pass: the end.
                None if self.pass.is_none() || self.out == 0 => {
                    self.ended = true;
                    return None;
                }
                // The decoder ended before the length: the next pass now.
                None => self.restart(),
            }
        }
    }
}

impl<S: Source + Send> Source for Looper<S> {
    fn current_span_len(&self) -> Option<usize> {
        match (self.pass, &self.intro) {
            // The same format all along (the intro's is the music's).
            (None, None) => self.inner.current_span_len(),
            _ => None,
        }
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        match (self.pass, &self.intro) {
            (None, None) => self.inner.total_duration(),
            _ => None,
        }
    }
}

/// Bevy's decoder of an OGG file.
pub type FileDecoder = <AudioSource as Decodable>::Decoder;

impl Decodable for MusicStream {
    type Decoder = Looper<FileDecoder>;

    fn decoder(&self) -> Self::Decoder {
        let source = self.source.clone();
        let inner = source.decoder();
        let intro = self
            .intro
            .as_ref()
            .map(|(intro, gain)| (intro.decoder(), *gain));
        Looper::new(
            inner,
            Box::new(move || source.decoder()),
            self.looping,
            intro,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;
    use std::time::Duration;

    use super::*;

    /// A stereo test source: a rising ramp on the left, a falling one on the right, `frames` long,
    /// then a few frames of junk (what a decoder may add after the length), at 1 kHz.
    struct Ramp {
        frames: u32,
        junk: u32,
        index: u32,
    }

    impl Iterator for Ramp {
        type Item = f32;

        fn next(&mut self) -> Option<f32> {
            let total = (self.frames + self.junk) * 2;
            if self.index >= total {
                return None;
            }
            let frame = self.index / 2;
            let left = self.index.is_multiple_of(2);
            self.index += 1;
            if frame >= self.frames {
                return Some(0.9);
            }
            let x = frame as f32 / self.frames as f32;
            Some(if left { -0.5 + x } else { 0.5 - x })
        }
    }

    impl Source for Ramp {
        fn current_span_len(&self) -> Option<usize> {
            None
        }
        fn channels(&self) -> ChannelCount {
            NonZero::new(2).unwrap()
        }
        fn sample_rate(&self) -> SampleRate {
            NonZero::new(1000).unwrap()
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    fn ramp(frames: u32, junk: u32) -> Ramp {
        Ramp {
            frames,
            junk,
            index: 0,
        }
    }

    /// Issue #104: a loop plays its length again and again (what the decoder adds after it is
    /// left out), without a click at the seam (the ramp jumps back by 1 there: smoothed, no step
    /// is much bigger than the ramp's own); a jingle plays once.
    #[test]
    fn a_loop_restarts_at_its_length_without_a_click() {
        let fresh: Fresh<Ramp> = Box::new(|| ramp(500, 30));
        let looper = Looper::new(ramp(500, 30), fresh, Some(0.5), None);
        assert_eq!(looper.total_duration(), None);
        assert_eq!(looper.channels().get(), 2);
        let samples: Vec<f32> = looper.take(500 * 2 * 4).collect();
        assert_eq!(samples.len(), 4000, "it never ends");
        assert!(!samples.contains(&0.9), "nothing after the length");
        let ramp_step = 1.0 / 500.0;
        let biggest = samples
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
            .fold(0.0f32, f32::max);
        assert!(
            biggest < 1.0 / SEAM_FRAMES as f32 + 2.0 * ramp_step,
            "{biggest}: smoothed"
        );
        // Away from the seams, the samples are the file's own.
        let second_pass = &samples[1000..2000];
        let middle = 250 * 2;
        assert_eq!(second_pass[middle], -0.5 + 0.5);
        assert!((second_pass[middle + 1] - (0.5 - 0.5)).abs() < 1e-6);

        // A decoder shorter than the length: the next pass when it ends.
        let fresh: Fresh<Ramp> = Box::new(|| ramp(300, 0));
        let looper = Looper::new(ramp(300, 0), fresh, Some(0.5), None);
        assert_eq!(looper.take(3000).count(), 3000);
        // Played once: as it is, to its end.
        let fresh: Fresh<Ramp> = Box::new(|| ramp(500, 30));
        let once = Looper::new(ramp(500, 30), fresh, None, None);
        assert_eq!(once.count(), 1060);
        // An empty file does not loop forever on nothing.
        let fresh: Fresh<Ramp> = Box::new(|| ramp(0, 0));
        assert_eq!(Looper::new(ramp(0, 0), fresh, Some(1.0), None).count(), 0);
    }

    /// Issue #104: a jingle before its music, in the same stream: the jingle's samples (at its
    /// gain), then the music's first pass from its first sample (the seam smoothed), then its
    /// loop; nothing left out, nothing added.
    #[test]
    fn an_intro_chains_into_its_music() {
        let fresh: Fresh<Ramp> = Box::new(|| ramp(500, 30));
        let intro = (ramp(200, 0), 0.5);
        let looper = Looper::new(ramp(500, 30), fresh, Some(0.5), Some(intro));
        assert_eq!(looper.total_duration(), None);
        let samples: Vec<f32> = looper.take(400 + 1000 + 1000).collect();
        let jingle: Vec<f32> = ramp(200, 0).map(|s| s * 0.5).collect();
        assert_eq!(&samples[..400], &jingle[..], "the intro, at its gain");
        let music: Vec<f32> = ramp(500, 0).collect();
        let after = (SEAM_FRAMES as usize + 2) * 2;
        assert_eq!(
            &samples[400 + after..1400],
            &music[after..1000],
            "then the music"
        );
        assert_eq!(
            &samples[1400 + after..2400],
            &music[after..1000],
            "in a loop"
        );
        let biggest = samples
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
            .fold(0.0f32, f32::max);
        assert!(biggest < 1.0 / SEAM_FRAMES as f32 + 0.01, "{biggest}");
        // Played once after its intro: both, then the end.
        let fresh: Fresh<Ramp> = Box::new(|| ramp(500, 0));
        let once = Looper::new(ramp(500, 0), fresh, None, Some((ramp(200, 0), 1.0)));
        assert_eq!(once.count(), 1400);
    }

    #[test]
    fn only_an_ogg_file_streams() {
        let ogg = AudioSource {
            bytes: b"OggS\0\x02rest".to_vec().into(),
        };
        assert!(MusicStream::new(&ogg, Some(1.0)).is_some());
        let html = AudioSource {
            bytes: b"<!doctype html>".to_vec().into(),
        };
        assert!(MusicStream::new(&html, Some(1.0)).is_none());
    }
}
