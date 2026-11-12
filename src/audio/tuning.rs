//! Issue #103: the mix of the sounds (admin panel, « à régler »): the volume of each family, how
//! far the karts are heard, how many sounds play at once, and how the engine follows the revs.
//! The player's own volumes (general, effects, music) are in the options (`options`), kept in
//! the browser.

use bevy::prelude::*;

use super::catalog::Family;

#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct SoundTuning {
    /// The engines' volume (0 to 2; issue #270: 0.28, was 0.55: under the effects).
    pub engine_volume: f32,
    /// The drift (its squeal, its chimes, the mini-turbos) and the boosts.
    pub drive_volume: f32,
    /// The items.
    pub item_volume: f32,
    /// Hits, shocks, falls, the pelican.
    pub hit_volume: f32,
    /// The countdown, the last lap, the finish's jingles, the croquettes and the roulette.
    pub race_volume: f32,
    /// The animals' cries.
    pub voice_volume: f32,
    /// The screens' sounds.
    pub screen_volume: f32,
    /// The environments (glider, water, off the road, landings) and the lobby on foot.
    pub world_volume: f32,
    /// A kart's sound is at full volume this close to the camera (m), then fades as the square
    /// of the distance (Bevy's spatial audio: a quarter at twice this distance)...
    pub full_volume_distance: f32,
    /// ...and is not played at all beyond this distance (m).
    pub range: f32,
    /// At most this many short sounds at once (everything but the engines and the loops): a
    /// new one takes the place of the farthest, or is not played.
    pub max_sounds: u32,
    /// At most this many engines heard at once: the player's, then the closest (issue #270: 4,
    /// was 5; a kart that is no longer among them fades out before its voice takes another one).
    pub max_engines: u32,
    /// At most this many drifts squealing at once: the player's, then the closest.
    pub max_squeals: u32,
    /// Issue #270 (inspired by `Kart::updateEngineSFX` of SuperTuxKart): the engine's pitch
    /// (playback speed of the loop) at rest...
    pub engine_idle_pitch: f32,
    /// ...plus this much from rest to top speed (smoothly: the whole rise of the speed)...
    pub engine_speed_pitch: f32,
    /// ...in this many gears: within each one, the pitch climbs by `engine_gear_pitch` more, then
    /// drops back by as much when the next one comes in (a sawtooth: « brrrBRRR brrrBRRR »)...
    pub engine_gears: u32,
    pub engine_gear_pitch: f32,
    /// ...plus this much with the accelerator down (a standing kart revs 1.5 times this), and
    /// this much in a boost (the rocket start, a mini-turbo, a pad, a piment).
    pub engine_throttle_pitch: f32,
    pub engine_boost_pitch: f32,
    /// The engine's volume (0 to 1): at rest without the accelerator (low and calm)...
    pub engine_idle_volume: f32,
    /// ...plus this much with the accelerator down (present under load)...
    pub engine_load_volume: f32,
    /// ...plus this much at top speed (the speed adds a little)...
    pub engine_speed_volume: f32,
    /// ...plus this much in a boost.
    pub engine_boost_volume: f32,
    /// A gear change dips the volume by this much (0 to 1), coming back over this part of the
    /// gear (0 to 1): the clutch.
    pub engine_shift_dip: f32,
    pub engine_shift_width: f32,
    /// Issue #270: each other kart's engine its own note (playback speed), by up to this much up
    /// or down (fixed per kart): they do not all sound the same.
    pub engine_detune: f32,
    /// Issue #270: the others' engines in the mix, after `engine_volume` (0 to 2): at
    /// `engine_ref_distance` of the camera, as loud as this times the player's.
    pub other_engine_volume: f32,
    /// Issue #270: how the others' loops (engines, squeals) fade with their distance to the
    /// camera: whole up to `engine_ref_distance` (m), then `ref / (ref + rolloff × (d - ref))`
    /// (OpenAL's inverse distance, as SuperTuxKart), down to nothing at `engine_range` (m; the
    /// last quarter of it fades out smoothly).
    pub engine_ref_distance: f32,
    pub engine_rolloff: f32,
    pub engine_range: f32,
    /// Issue #270: how far to the side the others' loops go (0: in the middle, 1: in one ear
    /// only), by where their kart is seen from the camera.
    pub engine_pan: f32,
    /// Issue #270: the farther a kart, the duller its loops: a low-pass at this frequency (Hz) at
    /// `engine_range` (none at `engine_ref_distance`; 0: never).
    pub engine_far_lowpass: f32,
    /// Issue #270: the Doppler effect of the others' loops: the speed of sound (m/s; lower than
    /// the real 343 m/s: a cartoon, heard at kart speeds), and its most up or down (0 to 0.5).
    pub doppler_sound_speed: f32,
    pub doppler_max: f32,
    /// Issue #270: how fast the engine's pitch follows the revs (1/s): quick enough for a gear
    /// change to drop at once (within a tenth of a second).
    pub engine_pitch_response: f32,
    /// How fast the engine's pitch and volume follow the revs (1/s); the loops fade out at this
    /// rate out of a race too (issue #135).
    pub engine_response: f32,
    /// Issue #135: a loop whose sound is changed in the admin panel fades out this fast (1/s)
    /// before the new one comes in.
    pub switch_response: f32,
    /// A kart does not cry more often than this (s).
    pub cry_interval: f32,
    /// A shock against a wall makes a sound from this loss of speed (m/s in a tick).
    pub wall_hit_speed: f32,
    /// A landing makes a sound after this long in the air (s).
    pub landing_air_time: f32,
}

impl Default for SoundTuning {
    fn default() -> Self {
        Self {
            engine_volume: 0.28,
            drive_volume: 0.8,
            item_volume: 0.9,
            hit_volume: 0.9,
            race_volume: 0.9,
            voice_volume: 0.85,
            screen_volume: 0.7,
            world_volume: 0.7,
            full_volume_distance: 7.0,
            range: 70.0,
            max_sounds: 16,
            max_engines: 4,
            max_squeals: 3,
            engine_idle_pitch: 0.6,
            engine_speed_pitch: 0.35,
            engine_gears: 4,
            engine_gear_pitch: 0.4,
            engine_throttle_pitch: 0.1,
            engine_boost_pitch: 0.25,
            engine_idle_volume: 0.3,
            engine_load_volume: 0.45,
            engine_speed_volume: 0.15,
            engine_boost_volume: 0.15,
            engine_shift_dip: 0.35,
            engine_shift_width: 0.15,
            engine_detune: 0.06,
            other_engine_volume: 0.8,
            engine_ref_distance: 6.0,
            engine_rolloff: 1.0,
            engine_range: 50.0,
            engine_pan: 0.8,
            engine_far_lowpass: 900.0,
            doppler_sound_speed: 80.0,
            doppler_max: 0.25,
            engine_pitch_response: 20.0,
            engine_response: 10.0,
            switch_response: 40.0,
            cry_interval: 0.8,
            wall_hit_speed: 5.0,
            landing_air_time: 0.35,
        }
    }
}

impl SoundTuning {
    /// The volume of a family (0 to 2).
    pub fn family_volume(&self, family: Family) -> f32 {
        match family {
            Family::Drive => self.drive_volume,
            Family::Items => self.item_volume,
            Family::Hits => self.hit_volume,
            Family::Race => self.race_volume,
            Family::Voices => self.voice_volume,
            Family::Screens => self.screen_volume,
            Family::World => self.world_volume,
        }
        .clamp(0.0, 2.0)
    }
}

/// Issue #104: the music's mix and its transitions (admin panel, « à régler »). The player's
/// own volume of the music is in the options (`PlayerOptions::music_volume`, with the general
/// one), kept in the browser.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct MusicTuning {
    /// The music in the mix (0 to 2; 1: the pieces at their full level). Each piece has its own
    /// level in `assets/data/music.ron` (measured: they sound as loud as each other).
    pub volume: f32,
    /// A screen's music (the home, the lobby) comes in over this long (s)...
    pub fade_in: f32,
    /// ...and the music heard goes out over this long when the screen changes (s).
    pub fade_out: f32,
    /// At the finish, the race's music goes out over this long before the finish's jingle (s):
    /// it does not stop dead.
    pub finish_fade: f32,
    /// On the last lap, the race's music goes out this quickly before its jingle (s).
    pub jingle_fade: f32,
    /// During the pause (solo), the music is this much quieter (0 to 1)...
    pub pause_volume: f32,
    /// ...going down and back up at this rate (1/s).
    pub pause_response: f32,
    /// On the last lap, a piece without a faster version (`faster` in `music.ron`) plays this much
    /// faster (playback speed: a little higher too).
    pub final_lap_speed: f32,
    /// At the finish, the « best » jingle and music up to this place...
    pub best_place: u32,
    /// ...the « good » ones up to this one, the « bad » ones after it.
    pub good_place: u32,
}

impl Default for MusicTuning {
    fn default() -> Self {
        Self {
            volume: 0.5,
            fade_in: 0.6,
            fade_out: 0.8,
            finish_fade: 1.2,
            jingle_fade: 0.3,
            pause_volume: 0.35,
            pause_response: 6.0,
            final_lap_speed: 1.06,
            best_place: 1,
            good_place: 3,
        }
    }
}
