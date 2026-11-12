//! Issue #270: the engine heard, not only computed. Its loop is played through rodio's own
//! player and mixer (what Bevy plays it with), and the pitch that comes out is measured.

use std::f32::consts::TAU;
use std::num::NonZero;

use bevy::audio::{AudioSink, AudioSinkPlayback, Decodable};
use rodio::Player;
use rodio::mixer::{MixerSource, mixer};

use super::*;

/// The device's rate in these tests (another one than the loops': the mixer resamples them).
pub(super) const OUT_RATE: u32 = 48_000;
/// The test tone's frequency (Hz).
pub(super) const TONE: f32 = 220.0;

/// A recorded loop of a pure tone at [`TONE`] (a whole number of periods: seamless).
pub(super) fn tone_loop() -> LoopSound {
    let periods = 220;
    let len = (SAMPLE_RATE as f32 * periods as f32 / TONE).round() as usize;
    LoopSound::recorded(
        (0..len)
            .map(|i| 0.5 * (TAU * periods as f32 * i as f32 / len as f32).sin())
            .collect(),
    )
}

/// A stereo mixer at [`OUT_RATE`], as the device's.
pub(super) fn device() -> (rodio::mixer::Mixer, MixerSource) {
    mixer(NonZero::new(2).unwrap(), NonZero::new(OUT_RATE).unwrap())
}

/// Pulls `seconds` of the device's output: its left and right channels.
pub(super) fn pull(out: &mut MixerSource, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let frames = (seconds * OUT_RATE as f32) as usize;
    let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
    for _ in 0..frames {
        left.push(out.next().unwrap_or(0.0));
        right.push(out.next().unwrap_or(0.0));
    }
    (left, right)
}

/// The frequency of `samples` (at [`OUT_RATE`]), from its rising zero crossings.
pub(super) fn frequency(samples: &[f32]) -> f32 {
    let crossings: Vec<usize> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
        .map(|(i, _)| i)
        .collect();
    match (crossings.first(), crossings.last()) {
        (Some(a), Some(b)) if b > a => {
            (crossings.len() - 1) as f32 * OUT_RATE as f32 / (b - a) as f32
        }
        _ => 0.0,
    }
}

/// Issue #270, the cause of « toujours le même bruit »: the playback speed set on a loop's sink
/// while it plays is the pitch heard (rodio reads a source's rate only at the start of each of
/// its spans: a loop with a single endless span kept the pitch it started with).
#[test]
fn a_loop_s_pitch_changes_while_it_plays() {
    let (mixer, mut out) = device();
    let player = Player::connect_new(&mixer);
    player.append(tone_loop().decoder());
    let sink = AudioSink::new(player);
    let (left, _) = pull(&mut out, 0.2);
    let start = frequency(&left[2400..]);
    assert!((start - TONE).abs() < 3.0, "{start} Hz");
    for speed in [1.45, 0.6, 1.0, 1.2] {
        sink.set_speed(speed);
        // Its next span: at most a few hundredths of a second.
        pull(&mut out, 0.06);
        let (left, _) = pull(&mut out, 0.1);
        let heard = frequency(&left);
        assert!(
            (heard / (TONE * speed) - 1.0).abs() < 0.03,
            "speed {speed}: {heard} Hz heard, {} wanted",
            TONE * speed
        );
    }
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// Issue #270: another kart's loop, placed by the mixer ([`LoopSound::placed`]), played as the
/// game plays it: heard on its side, in the middle when it is ahead, duller far away; its pitch
/// follows its sink's speed too (the Doppler, the revs).
#[test]
fn a_placed_loop_is_heard_on_its_side_and_duller_far_away() {
    let (mixer, mut out) = device();
    let (sound, control) = tone_loop().placed();
    let player = Player::connect_new(&mixer);
    player.append(sound.decoder());
    let sink = AudioSink::new(player);
    let mut listen = |pan: f32, lowpass: f32| {
        control.set(pan, lowpass);
        pull(&mut out, 0.05);
        let (left, right) = pull(&mut out, 0.1);
        (rms(&left), rms(&right), frequency(&left))
    };
    let (left, right, pitch) = listen(0.8, 0.0);
    assert!(right > 2.0 * left, "on the right: {left} {right}");
    assert!((pitch - TONE).abs() < 3.0, "{pitch}");
    let (left, right, _) = listen(-0.8, 0.0);
    assert!(left > 2.0 * right, "on the left: {left} {right}");
    let (middle_left, middle_right, _) = listen(0.0, 0.0);
    assert!((middle_left - middle_right).abs() < 0.02 * middle_left);
    let (dull, _, _) = listen(0.0, 110.0);
    assert!(dull < 0.5 * middle_left, "duller: {dull} {middle_left}");
    sink.set_speed(1.3);
    let (_, _, pitch) = listen(0.0, 0.0);
    assert!((pitch / (TONE * 1.3) - 1.0).abs() < 0.03, "{pitch}");
}
