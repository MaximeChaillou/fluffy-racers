//! Issue #103: the mixer's rules (pure, unit-tested): which sound gets a voice when they are all
//! taken, which karts are heard, how loud an option is, and how the engine follows the revs.

use bevy::prelude::*;

use super::tuning::SoundTuning;

/// The priority of a sound: the player's own and the screens' first (2), then the others by
/// their distance to the camera (1 at the camera, 0 at the range).
pub fn priority(foreground: bool, distance: f32, range: f32) -> f32 {
    if foreground {
        2.0
    } else {
        (1.0 - distance / range.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Whether a new sound gets a voice ([`admit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// A voice is free.
    Free,
    /// It takes the place of this one (an index of the voices given).
    Replace(usize),
    /// Not played: every voice plays something at least as important.
    Refused,
}

/// A new sound of `priority` among the `voices` playing (their priority and when they started),
/// at most `cap` at once: a free voice, or the place of the least important one (the oldest
/// among equals) if it is less important than the new one.
pub fn admit(voices: &[(f32, f64)], cap: usize, priority: f32) -> Admission {
    if voices.len() < cap {
        return Admission::Free;
    }
    let weakest = voices
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    match weakest {
        Some((index, (weakest, _))) if *weakest < priority => Admission::Replace(index),
        _ => Admission::Refused,
    }
}

/// The amplitude of an option's volume (0 to 100 %): its square, so that its steps sound even
/// (80 %: 0.64; 50 %: a quarter, about half as loud to the ear).
pub fn option_gain(percent: u8) -> f32 {
    let x = f32::from(percent.min(100)) / 100.0;
    x * x
}

/// The karts heard: the player's first (`player`), then the closest to `listener` within
/// `range`, `max` in all. `karts`: their positions; `out` (cleared): their indices, the player
/// first. Nothing allocated once `out` has room.
pub fn nearest(
    listener: Vec3,
    karts: &[(Vec3, bool)],
    range: f32,
    max: usize,
    out: &mut Vec<usize>,
) {
    out.clear();
    if let Some(player) = karts.iter().position(|(_, player)| *player)
        && max > 0
    {
        out.push(player);
    }
    while out.len() < max {
        let next = karts
            .iter()
            .enumerate()
            .filter(|(i, (at, _))| !out.contains(i) && at.distance(listener) <= range)
            .min_by(|(_, a), (_, b)| a.0.distance(listener).total_cmp(&b.0.distance(listener)));
        match next {
            Some((i, _)) => out.push(i),
            None => break,
        }
    }
}

/// What the engine of a kart hears: its speed along its heading (m/s), its top speed (m/s), its
/// accelerator (0 to 1), and whether it boosts, flies (no grip: it revs freely), stalls or is
/// under water.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Revs {
    pub speed: f32,
    pub top_speed: f32,
    pub throttle: f32,
    pub boosting: bool,
    pub airborne: bool,
    pub stalled: bool,
    pub underwater: bool,
}

/// The engine's note for `revs`: its pitch (playback speed of the hum: `engine_idle_pitch` at
/// rest, `engine_top_pitch` at top speed, more with the accelerator down and in a boost) and its
/// volume (0 to 1: louder under load). Stalled, it drops low and quiet; under water, muffled
/// (lower and quieter).
pub fn engine_note(tuning: &SoundTuning, revs: &Revs) -> (f32, f32) {
    let ratio = (revs.speed.abs() / revs.top_speed.max(1.0)).clamp(0.0, 1.3);
    let throttle = revs.throttle.clamp(0.0, 1.0);
    let mut pitch = tuning.engine_idle_pitch
        + (tuning.engine_top_pitch - tuning.engine_idle_pitch) * ratio.powf(0.8)
        + tuning.engine_throttle_pitch * throttle;
    if revs.airborne {
        // Nothing holds the wheels: the engine revs up with the accelerator.
        pitch += tuning.engine_throttle_pitch * 1.5 * throttle;
    }
    if revs.boosting {
        pitch += tuning.engine_boost_pitch;
    }
    let mut volume = tuning.engine_idle_volume
        + tuning.engine_load_volume * throttle.max(ratio.min(1.0))
        + if revs.boosting { 0.1 } else { 0.0 };
    if revs.stalled {
        pitch = tuning.engine_idle_pitch * 0.7;
        volume *= 0.4;
    }
    if revs.underwater {
        pitch *= 0.85;
        volume *= 0.6;
    }
    (pitch.max(0.1), volume.clamp(0.0, 1.0))
}

/// Moves `value` toward `target` at `rate` per second (exponential), over `dt` seconds.
pub fn approach(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    value + (target - value) * (1.0 - (-rate * dt).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #103: the engine follows the revs: higher and louder with the speed, higher still
    /// with the accelerator and in a boost; low and quiet stalled.
    #[test]
    fn the_engine_pitch_follows_the_revs() {
        let tuning = SoundTuning::default();
        let at = |speed: f32, throttle: f32, boosting: bool| {
            engine_note(
                &tuning,
                &Revs {
                    speed,
                    top_speed: 26.0,
                    throttle,
                    boosting,
                    ..default()
                },
            )
        };
        let (idle, idle_volume) = at(0.0, 0.0, false);
        assert!((idle - tuning.engine_idle_pitch).abs() < 1e-5);
        let mut last = idle;
        for speed in [3.0, 8.0, 13.0, 20.0, 26.0] {
            let (pitch, volume) = at(speed, 1.0, false);
            assert!(pitch > last, "{speed}: {pitch} after {last}");
            assert!(volume > idle_volume);
            last = pitch;
        }
        let (top, _) = at(26.0, 0.0, false);
        assert!((top - tuning.engine_top_pitch).abs() < 1e-4, "{top}");
        assert!(
            at(20.0, 1.0, false).0 > at(20.0, 0.0, false).0,
            "the accelerator"
        );
        assert!(at(20.0, 1.0, true).0 > at(20.0, 1.0, false).0, "the boost");
        assert!(
            at(30.0, 1.0, true).0 > at(26.0, 1.0, true).0,
            "boosted past top speed"
        );
        // Going backwards revs the same.
        assert_eq!(at(-8.0, 1.0, false), at(8.0, 1.0, false));
        let stalled = engine_note(
            &tuning,
            &Revs {
                stalled: true,
                throttle: 1.0,
                top_speed: 26.0,
                ..default()
            },
        );
        assert!(stalled.0 < idle && stalled.1 < idle_volume);
        let airborne = engine_note(
            &tuning,
            &Revs {
                speed: 20.0,
                top_speed: 26.0,
                throttle: 1.0,
                airborne: true,
                ..default()
            },
        );
        assert!(airborne.0 > at(20.0, 1.0, false).0, "it revs in the air");
        // A pitch of 1 is the hum's own frequency: reached on the way to top speed.
        assert!(at(5.0, 1.0, false).0 < 1.0 && at(26.0, 1.0, false).0 > 1.0);
    }

    #[test]
    fn a_new_sound_takes_the_place_of_the_least_important_one() {
        assert_eq!(admit(&[], 2, 0.1), Admission::Free);
        assert_eq!(admit(&[(0.5, 0.0)], 2, 0.1), Admission::Free);
        let full = [(0.5, 1.0), (0.2, 2.0), (0.2, 1.5), (2.0, 0.0)];
        // The least important, the oldest among equals.
        assert_eq!(admit(&full, 4, 0.6), Admission::Replace(2));
        assert_eq!(admit(&full, 4, 0.2), Admission::Refused);
        assert_eq!(admit(&full, 4, 0.1), Admission::Refused);
        // The player's own beat the others; the cap is never exceeded.
        assert_eq!(admit(&[(2.0, 0.0); 4], 4, 2.0), Admission::Refused);
        assert_eq!(admit(&full, 3, 2.0), Admission::Replace(2));
        assert!(priority(true, 500.0, 70.0) > priority(false, 0.0, 70.0));
        assert!(priority(false, 10.0, 70.0) > priority(false, 40.0, 70.0));
        assert_eq!(priority(false, 90.0, 70.0), 0.0);
    }

    #[test]
    fn the_player_and_the_closest_karts_are_heard() {
        let karts = [
            (Vec3::new(50.0, 0.0, 0.0), false),
            (Vec3::new(5.0, 0.0, 0.0), false),
            (Vec3::new(500.0, 0.0, 0.0), true),
            (Vec3::new(20.0, 0.0, 0.0), false),
            (Vec3::new(90.0, 0.0, 0.0), false),
        ];
        let mut out = Vec::with_capacity(8);
        nearest(Vec3::ZERO, &karts, 70.0, 3, &mut out);
        assert_eq!(out, [2, 1, 3]);
        nearest(Vec3::ZERO, &karts, 70.0, 10, &mut out);
        assert_eq!(out, [2, 1, 3, 0], "none beyond the range");
        nearest(Vec3::ZERO, &karts[..2], 70.0, 1, &mut out);
        assert_eq!(out, [1]);
        nearest(Vec3::ZERO, &karts, 70.0, 0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn the_options_volumes_sound_even() {
        assert_eq!(option_gain(0), 0.0);
        assert_eq!(option_gain(100), 1.0);
        assert!((option_gain(80) - 0.64).abs() < 1e-6);
        assert_eq!(option_gain(200), 1.0);
        let steps: Vec<f32> = (0..=10).map(|i| option_gain(i * 10)).collect();
        assert!(steps.windows(2).all(|w| w[1] > w[0]));
        let x = approach(0.0, 1.0, 10.0, 0.1);
        assert!(x > 0.6 && x < 0.65, "{x}");
        assert_eq!(approach(0.5, 0.5, 10.0, 0.1), 0.5);
    }
}
