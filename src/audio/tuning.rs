//! Issue #103: the mix of the sounds (admin panel, « à régler »): the volume of each family, how
//! far the karts are heard, how many sounds play at once, and how the engine follows the revs.
//! The player's own volumes (general, effects, music) are in the options (`options`), kept in
//! the browser.

use bevy::prelude::*;

use super::catalog::Family;

#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct SoundTuning {
    /// The engines' volume (0 to 2, 1 as mixed).
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
    /// Behind the home screen (its background race): everything this much quieter.
    pub home_volume: f32,
    /// A kart's sound is at full volume this close to the camera (m), then fades as the square
    /// of the distance (Bevy's spatial audio: a quarter at twice this distance)...
    pub full_volume_distance: f32,
    /// ...and is not played at all beyond this distance (m).
    pub range: f32,
    /// At most this many short sounds at once (everything but the engines and the loops): a
    /// new one takes the place of the farthest, or is not played.
    pub max_sounds: u32,
    /// At most this many engines heard at once: the player's, then the closest.
    pub max_engines: u32,
    /// At most this many drifts squealing at once: the player's, then the closest.
    pub max_squeals: u32,
    /// The engine's pitch (playback speed of the hum) at rest...
    pub engine_idle_pitch: f32,
    /// ...at top speed...
    pub engine_top_pitch: f32,
    /// ...plus this much with the accelerator down, and this much in a boost.
    pub engine_throttle_pitch: f32,
    pub engine_boost_pitch: f32,
    /// The engine's volume (0 to 1) at rest, and how much the load (accelerator, speed) adds.
    pub engine_idle_volume: f32,
    pub engine_load_volume: f32,
    /// How fast the engine's pitch and volume follow the revs (1/s).
    pub engine_response: f32,
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
            engine_volume: 0.55,
            drive_volume: 0.8,
            item_volume: 0.9,
            hit_volume: 0.9,
            race_volume: 0.9,
            voice_volume: 0.85,
            screen_volume: 0.7,
            world_volume: 0.7,
            home_volume: 0.35,
            full_volume_distance: 7.0,
            range: 70.0,
            max_sounds: 16,
            max_engines: 5,
            max_squeals: 3,
            engine_idle_pitch: 0.55,
            engine_top_pitch: 1.45,
            engine_throttle_pitch: 0.1,
            engine_boost_pitch: 0.2,
            engine_idle_volume: 0.35,
            engine_load_volume: 0.45,
            engine_response: 10.0,
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
