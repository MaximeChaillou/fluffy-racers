//! Issue #103: the mixer's rules (pure, unit-tested): which sound gets a voice when they are all
//! taken, which karts are heard, how loud an option is, and how the engine follows the revs.
//! Issue #135: where the karts are heard at all ([`race_sounds`]).

use bevy::prelude::*;

use super::tuning::SoundTuning;
use crate::race::RacePhase;

/// Issue #135: what is shown, for the sounds: the home screen (its background race), the lobby,
/// the pause, the race's phase (none: no race yet, or driving freely), whether the player drives
/// freely (no race to wait for: the test ground, « Roulage libre »), the player's finish.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stage {
    pub home: bool,
    pub salon: bool,
    pub paused: bool,
    pub phase: Option<RacePhase>,
    pub free: bool,
    pub finished: bool,
}

/// Issue #135: whether the karts are heard: their short sounds (`karts`: their drift, boosts,
/// items, hits, cries) and their loops (`loops`: the engines, the squeals, the wind, the rumble).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RaceSounds {
    pub karts: bool,
    pub loops: bool,
}

/// Issue #135 (Max: « Ils doivent être coupés quand on fait plus la course ou sur l'accueil »):
/// the karts are heard during a race, from its countdown to the player's finish, and while
/// driving freely (no race); not on the home screen (only its music), in the lobby, before the
/// countdown (a race not there yet, its grid), after the player's finish (the results, the
/// standings, the podium) nor once the race is over. The pause silences the loops (nothing else
/// moves then).
pub fn race_sounds(stage: &Stage) -> RaceSounds {
    let racing = !stage.home
        && !stage.salon
        && !stage.finished
        && match stage.phase {
            None => stage.free,
            Some(phase) => matches!(phase, RacePhase::Countdown | RacePhase::Racing),
        };
    RaceSounds {
        karts: racing,
        loops: racing && !stage.paused,
    }
}

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

/// Issue #270: the gear of the engine at `ratio` of its top speed (0 to 1), and how far into it
/// (0 to 1): `engine_gears` even gears (SuperTuxKart's « fake gears », `Kart::updateEngineSFX`).
pub fn gear(tuning: &SoundTuning, ratio: f32) -> (u32, f32) {
    let gears = tuning.engine_gears.max(1);
    let at = ratio.clamp(0.0, 1.0) * gears as f32;
    let gear = (at.floor() as u32).min(gears - 1);
    (gear, (at - gear as f32).clamp(0.0, 1.0))
}

/// The engine's note for `revs`: its pitch (playback speed of the loop) and its volume (0 to 1).
///
/// Issue #270, inspired by `Kart::updateEngineSFX` of SuperTuxKart: the pitch is half the speed,
/// half « fake gears »: from `engine_idle_pitch` at rest, `engine_speed_pitch` over the whole
/// speed, and in each of the `engine_gears` gears `engine_gear_pitch` more, dropping back at the
/// next gear (a sawtooth on a ramp); beyond top speed (a boost), the rise is squeezed. The
/// accelerator adds `engine_throttle_pitch` (a standing kart revs 1.5 times as much more: the
/// grid), a boost `engine_boost_pitch`; in the air, nothing holds the wheels: it revs up with the
/// accelerator. The volume: low and calm at rest without the accelerator, present under load, a
/// little more with the speed and in a boost; dipped for a moment at each gear change. Stalled,
/// it drops low and quiet; under water, muffled (lower and quieter).
pub fn engine_note(tuning: &SoundTuning, revs: &Revs) -> (f32, f32) {
    let mut ratio = (revs.speed.abs() / revs.top_speed.max(1.0)).max(0.0);
    if ratio > 1.0 {
        // SuperTuxKart's squeeze beyond top speed: 1 + (1 - 1/f).
        ratio = (2.0 - 1.0 / ratio).min(1.5);
    }
    let throttle = revs.throttle.clamp(0.0, 1.0);
    let (gear, into) = gear(tuning, ratio);
    let mut pitch = tuning.engine_idle_pitch
        + tuning.engine_speed_pitch * ratio
        + tuning.engine_gear_pitch * into
        + tuning.engine_throttle_pitch * throttle;
    // Nothing holds the wheels (in the air, or standing: fading out by a tenth of the top
    // speed): it revs up with the accelerator.
    let free = if revs.airborne {
        1.0
    } else {
        (1.0 - ratio / 0.1).max(0.0)
    };
    pitch += tuning.engine_throttle_pitch * 1.5 * throttle * free;
    if revs.boosting {
        pitch += tuning.engine_boost_pitch;
    }
    let mut volume = tuning.engine_idle_volume
        + tuning.engine_load_volume * throttle
        + tuning.engine_speed_volume * ratio.min(1.0)
        + if revs.boosting {
            tuning.engine_boost_volume
        } else {
            0.0
        };
    // The clutch: a dip just after a gear change, coming back over `engine_shift_width`.
    let width = tuning.engine_shift_width.max(1e-3);
    if gear > 0 && into < width && !revs.airborne {
        volume *= 1.0 - tuning.engine_shift_dip.clamp(0.0, 1.0) * (1.0 - into / width);
    }
    if revs.stalled {
        pitch = tuning.engine_idle_pitch * 0.7;
        volume = tuning.engine_idle_volume * 0.5;
    }
    if revs.underwater {
        pitch *= 0.85;
        volume *= 0.6;
    }
    (pitch.max(0.1), volume.clamp(0.0, 1.0))
}

/// Issue #270: the highest pitch the engine takes on the ground with the accelerator down: past
/// top speed in a boost (the loops are checked up to there).
pub fn engine_max_pitch(tuning: &SoundTuning) -> f32 {
    engine_note(
        tuning,
        &Revs {
            speed: 3.0,
            top_speed: 1.0,
            throttle: 1.0,
            boosting: true,
            ..Default::default()
        },
    )
    .0
}

/// Issue #270: how loud another kart's loop is at `distance` (m) from the camera (0 to 1): whole
/// up to `engine_ref_distance`, then OpenAL's inverse distance (`engine_rolloff`), fading out
/// smoothly over the last quarter of `engine_range`, nothing beyond.
pub fn rolloff(tuning: &SoundTuning, distance: f32) -> f32 {
    let reference = tuning.engine_ref_distance.max(0.1);
    let range = tuning.engine_range.max(reference + 1.0);
    if distance >= range {
        return 0.0;
    }
    let d = distance.max(reference);
    let inverse = reference / (reference + tuning.engine_rolloff.max(0.0) * (d - reference));
    let edge = ((range - distance) / (0.25 * range)).clamp(0.0, 1.0);
    inverse * edge * edge * (3.0 - 2.0 * edge)
}

/// Issue #270: where another kart's loop is heard, left (-1) to right (1): the side of the camera
/// (`right`: its right) its kart is on, up to `engine_pan`; toward the middle when it is right
/// there (it is all around).
pub fn pan(tuning: &SoundTuning, listener: Vec3, right: Vec3, at: Vec3) -> f32 {
    let to = at - listener;
    let length = to.length();
    if length < 1e-3 {
        return 0.0;
    }
    let side = to.dot(right.normalize_or_zero()) / length;
    let near = (length / 2.0).min(1.0);
    (side * near).clamp(-1.0, 1.0) * tuning.engine_pan.clamp(0.0, 1.0)
}

/// Issue #270: a loop's gains, left and right, for its `pan` (equal power, 1 and 1 in the
/// middle).
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    ((1.0 - p).sqrt().min(1.0), (1.0 + p).sqrt().min(1.0))
}

/// Issue #270: the Doppler effect on another kart's loop (a factor of its pitch): higher while it
/// comes closer, lower while it goes away, from the speeds of the kart (`source`, `velocity`) and
/// of the camera (`listener`, `listener_velocity`) along the line between them, with
/// `doppler_sound_speed` (bounded by `doppler_max`).
pub fn doppler(
    tuning: &SoundTuning,
    listener: Vec3,
    listener_velocity: Vec3,
    source: Vec3,
    velocity: Vec3,
) -> f32 {
    let Some(toward) = (listener - source).try_normalize() else {
        return 1.0;
    };
    let c = tuning.doppler_sound_speed.max(1.0);
    // The kart's speed toward the camera, the camera's away from the kart.
    let source_closing = velocity.dot(toward).clamp(-0.9 * c, 0.9 * c);
    let listener_fleeing = listener_velocity.dot(toward).clamp(-0.9 * c, 0.9 * c);
    let max = tuning.doppler_max.clamp(0.0, 0.5);
    ((c - listener_fleeing) / (c - source_closing)).clamp(1.0 - max, 1.0 + max)
}

/// Issue #270: the low-pass of another kart's loop at `distance` (Hz; 0: none): none up to
/// `engine_ref_distance`, then down to `engine_far_lowpass` at `engine_range` (evenly in
/// octaves, from 16 kHz).
pub fn distance_lowpass(tuning: &SoundTuning, distance: f32) -> f32 {
    let far = tuning.engine_far_lowpass;
    let reference = tuning.engine_ref_distance.max(0.1);
    if far <= 0.0 || distance <= reference {
        return 0.0;
    }
    let t = ((distance - reference) / (tuning.engine_range - reference).max(1.0)).clamp(0.0, 1.0);
    16_000.0 * (far.min(16_000.0) / 16_000.0).powf(t)
}

/// Issue #270: where another kart's loop is heard from the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    /// Its volume's factor ([`rolloff`]).
    pub gain: f32,
    /// Its side ([`pan`]).
    pub pan: f32,
    /// Its low-pass (Hz, 0: none: [`distance_lowpass`]).
    pub lowpass: f32,
    /// Its pitch's factor ([`doppler`]).
    pub doppler: f32,
}

/// Issue #270: where a kart at `at` moving at `velocity` is heard from the camera at `listener`
/// (`right`: its right) moving at `listener_velocity`.
pub fn placement(
    tuning: &SoundTuning,
    (listener, right, listener_velocity): (Vec3, Vec3, Vec3),
    at: Vec3,
    velocity: Vec3,
) -> Placement {
    let distance = at.distance(listener);
    Placement {
        gain: rolloff(tuning, distance),
        pan: pan(tuning, listener, right, at),
        lowpass: distance_lowpass(tuning, distance),
        doppler: doppler(tuning, listener, listener_velocity, at, velocity),
    }
}

/// Issue #270: a kart's own note (a factor of its engine's pitch): up to `engine_detune` up or
/// down, fixed for the kart (from its entity).
pub fn detune(tuning: &SoundTuning, kart: Entity) -> f32 {
    // SplitMix64 of the entity's bits: the same kart, the same note.
    let mut x = kart.to_bits().wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    let unit = (x >> 11) as f32 / (1u64 << 53) as f32;
    1.0 + tuning.engine_detune * (2.0 * unit - 1.0)
}

/// Moves `value` toward `target` at `rate` per second (exponential), over `dt` seconds.
pub fn approach(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    value + (target - value) * (1.0 - (-rate * dt).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #135: the karts are heard from the countdown to the player's finish, and driving
    /// freely; never on the home screen, in the lobby, on the grid, after the finish or once
    /// the race is over; the pause silences the loops.
    #[test]
    fn the_karts_are_heard_only_in_a_race() {
        let racing = |phase| Stage { phase, ..default() };
        let heard = RaceSounds {
            karts: true,
            loops: true,
        };
        let silent = RaceSounds::default();
        let free = Stage {
            free: true,
            ..default()
        };
        assert_eq!(race_sounds(&free), heard, "driving freely");
        assert_eq!(race_sounds(&racing(None)), silent, "the race not there yet");
        assert_eq!(race_sounds(&racing(Some(RacePhase::Countdown))), heard);
        assert_eq!(race_sounds(&racing(Some(RacePhase::Racing))), heard);
        for phase in [RacePhase::Grid, RacePhase::Finished, RacePhase::Results] {
            assert_eq!(race_sounds(&racing(Some(phase))), silent, "{phase:?}");
        }
        for stage in [
            Stage {
                home: true,
                ..racing(Some(RacePhase::Racing))
            },
            Stage {
                salon: true,
                ..free
            },
            Stage {
                finished: true,
                ..racing(Some(RacePhase::Racing))
            },
        ] {
            assert_eq!(race_sounds(&stage), silent, "{stage:?}");
        }
        let paused = Stage {
            paused: true,
            ..racing(Some(RacePhase::Racing))
        };
        assert_eq!(
            race_sounds(&paused),
            RaceSounds {
                karts: true,
                loops: false
            }
        );
    }

    /// Issue #270 (replaces #103's « the pitch always climbs with the speed », at Max's request):
    /// the gears pass. Within each gear the pitch climbs with the speed, and drops at each gear
    /// change, with a dip of the volume; overall it still climbs from rest to top speed.
    #[test]
    fn the_engine_climbs_in_each_gear_and_drops_at_each_change() {
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
        let gears = tuning.engine_gears;
        assert!((4..=5).contains(&gears));
        let (idle, idle_volume) = at(0.0, 0.0, false);
        assert!((idle - tuning.engine_idle_pitch).abs() < 1e-5);
        let steps = 520;
        let mut drops = 0;
        let mut last = at(26.0 / steps as f32, 1.0, false);
        let mut last_gear = 0;
        for step in 2..=steps {
            let speed = 26.0 * step as f32 / steps as f32;
            let now = at(speed, 1.0, false);
            let (gear_now, into) = gear(&tuning, speed / 26.0);
            if gear_now == last_gear {
                assert!(now.0 > last.0, "{speed} m/s: {} after {}", now.0, last.0);
            } else {
                drops += 1;
                // The next gear: down by about `engine_gear_pitch`, and quieter for a moment.
                assert!(
                    now.0 < last.0 - 0.6 * tuning.engine_gear_pitch,
                    "{speed}: {now:?} after {last:?}"
                );
                assert!(
                    now.1 < last.1 * (1.0 - 0.5 * tuning.engine_shift_dip),
                    "{now:?} after {last:?}"
                );
                assert!(into < 0.01);
            }
            last = now;
            last_gear = gear_now;
        }
        assert_eq!(drops, gears - 1, "a drop at each gear change");
        // Overall it climbs: the middle of each gear is higher than the last one.
        let starts: Vec<f32> = (0..gears)
            .map(|g| at(26.0 * (g as f32 + 0.5) / gears as f32, 1.0, false).0)
            .collect();
        assert!(starts.windows(2).all(|w| w[1] > w[0]), "{starts:?}");
        let (top, top_volume) = at(26.0, 1.0, false);
        assert!(top > 1.2, "{top}");
        assert!(top_volume > idle_volume + 0.4);
        // The accelerator: higher and louder; let go, lower and calmer.
        let (coast, coast_volume) = at(18.0, 0.0, false);
        let (pull, pull_volume) = at(18.0, 1.0, false);
        assert!(pull > coast && pull_volume > coast_volume + 0.3);
        assert!(idle_volume < 0.35, "calm at rest");
        // Standing, the accelerator down revs it (the grid).
        assert!(at(0.0, 1.0, false).0 > idle + 0.2);
        // A boost (the rocket start: at rest; a mini-turbo: at speed): a clear rise.
        assert!(at(0.0, 1.0, true).0 > at(0.0, 1.0, false).0 + 0.2);
        assert!(at(20.0, 1.0, true).0 > at(20.0, 1.0, false).0 + 0.2);
        assert!(at(20.0, 1.0, true).1 > at(20.0, 1.0, false).1);
        assert!(
            at(30.0, 1.0, true).0 > at(26.0, 1.0, true).0,
            "past top speed"
        );
        assert!(engine_max_pitch(&tuning) >= at(40.0, 1.0, true).0);
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
    }

    /// Issue #270: the others' loops fade with the distance (whole close by, half at twice the
    /// reference, nothing beyond the range), go to the side their kart is on, duller far away,
    /// higher while a kart comes closer and lower while it goes away; each kart its own note.
    #[test]
    fn the_others_are_placed_by_distance_side_and_speed() {
        let tuning = SoundTuning::default();
        let reference = tuning.engine_ref_distance;
        assert_eq!(rolloff(&tuning, 0.0), 1.0);
        assert_eq!(rolloff(&tuning, reference), 1.0);
        assert!((rolloff(&tuning, 2.0 * reference) - 0.5).abs() < 0.01);
        let mut last = 1.0;
        for d in 1..=60 {
            let gain = rolloff(&tuning, d as f32);
            assert!(gain <= last, "{d} m");
            last = gain;
        }
        assert!(rolloff(&tuning, 30.0) < 0.25);
        assert_eq!(rolloff(&tuning, tuning.engine_range), 0.0);
        // The side: the camera looks along -Z, its right is +X.
        let right = Vec3::X;
        let on_the_right = pan(&tuning, Vec3::ZERO, right, Vec3::new(10.0, 0.0, -2.0));
        let on_the_left = pan(&tuning, Vec3::ZERO, right, Vec3::new(-10.0, 0.0, 3.0));
        let ahead = pan(&tuning, Vec3::ZERO, right, Vec3::new(0.0, 0.0, -20.0));
        assert!(
            on_the_right > 0.6 && on_the_left < -0.6,
            "{on_the_right} {on_the_left}"
        );
        assert!(ahead.abs() < 1e-5);
        let (l, r) = pan_gains(on_the_right);
        assert!(r > 0.99 && l < 0.6, "{l} {r}");
        assert_eq!(pan_gains(0.0), (1.0, 1.0));
        // Duller far away.
        assert_eq!(distance_lowpass(&tuning, reference), 0.0);
        assert!(distance_lowpass(&tuning, 25.0) < 5000.0);
        assert!(
            (distance_lowpass(&tuning, tuning.engine_range) - tuning.engine_far_lowpass).abs()
                < 1.0
        );
        // Doppler: a kart at 30 m/s passing a camera at 22 m/s.
        let camera = Vec3::ZERO;
        let camera_v = Vec3::new(0.0, 0.0, -22.0);
        let kart_v = Vec3::new(0.0, 0.0, -30.0);
        let behind = doppler(&tuning, camera, camera_v, Vec3::new(2.0, 0.0, 15.0), kart_v);
        let gone = doppler(
            &tuning,
            camera,
            camera_v,
            Vec3::new(2.0, 0.0, -15.0),
            kart_v,
        );
        assert!(behind > 1.08 && gone < 0.94, "{behind} {gone}");
        // The same speed: nothing.
        let along = doppler(&tuning, camera, kart_v, Vec3::new(0.0, 0.0, 10.0), kart_v);
        assert!((along - 1.0).abs() < 1e-5);
        let fast = doppler(
            &tuning,
            camera,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -500.0),
        );
        assert!(fast <= 1.0 + tuning.doppler_max + 1e-6);
        // Each kart its note, the same every time.
        let kart = |i: u32| Entity::from_raw_u32(i).unwrap();
        let notes: Vec<f32> = (1..12u32).map(|i| detune(&tuning, kart(i))).collect();
        assert!(
            notes
                .iter()
                .all(|n| (n - 1.0).abs() <= tuning.engine_detune + 1e-6)
        );
        let high = notes.iter().fold(0.0f32, |m, n| m.max(*n));
        let low = notes.iter().fold(2.0f32, |m, n| m.min(*n));
        assert!(high - low > tuning.engine_detune, "{notes:?}");
        assert_eq!(notes[0], detune(&tuning, kart(1)));
    }

    /// Issue #270: a kart that comes up from behind on the right, passes and goes away: heard on
    /// the right all along, louder and louder then quieter and duller, its pitch up while it
    /// comes, down once it is past (the Doppler).
    #[test]
    fn a_kart_passing_on_the_right_is_heard_coming_then_going() {
        let tuning = SoundTuning::default();
        // The camera looks along -Z (its right +X) and runs at 20 m/s; the kart at 28 m/s, 3 m
        // to the right, from 40 m behind to 40 m ahead.
        let camera_v = Vec3::new(0.0, 0.0, -20.0);
        let kart_v = Vec3::new(0.0, 0.0, -28.0);
        let mut heard = Vec::new();
        for step in 0..=80 {
            let z = 40.0 - step as f32;
            let at = Vec3::new(3.0, 0.0, z);
            let ears = (Vec3::ZERO, Vec3::X, camera_v);
            heard.push((z, placement(&tuning, ears, at, kart_v)));
        }
        assert!(heard.iter().all(|(_, p)| p.pan > 0.0), "on the right");
        assert!(heard.iter().any(|(_, p)| p.pan > 0.6), "clearly");
        let loudest = heard
            .iter()
            .max_by(|a, b| a.1.gain.total_cmp(&b.1.gain))
            .unwrap();
        assert!(loudest.0.abs() <= tuning.engine_ref_distance, "{loudest:?}");
        for pair in heard.windows(2) {
            let ((_, a), (z1, b)) = (pair[0], pair[1]);
            if z1 > tuning.engine_ref_distance {
                assert!(
                    b.gain >= a.gain && (b.lowpass == 0.0 || b.lowpass >= a.lowpass - 1e-3),
                    "coming at {z1}: {a:?} {b:?}"
                );
            }
            if z1 < -tuning.engine_ref_distance {
                assert!(
                    b.gain <= a.gain && (a.lowpass == 0.0 || b.lowpass <= a.lowpass + 1e-3),
                    "going at {z1}: {a:?} {b:?}"
                );
            }
        }
        let at = |z: f32| heard.iter().find(|(at, _)| *at == z).unwrap().1;
        let (coming, beside, going) = (at(15.0), at(0.0), at(-15.0));
        assert!(
            coming.doppler > 1.05 && going.doppler < 0.96,
            "{coming:?} {going:?}"
        );
        assert!(coming.lowpass > 0.0 && going.lowpass > 0.0);
        assert!((beside.doppler - 1.0).abs() < 0.01 && beside.lowpass == 0.0);
        assert!(beside.gain == 1.0 && coming.gain < 0.6);
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
