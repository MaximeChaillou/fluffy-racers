//! Sounds. No sound file yet (assets come with issue #11): the few sounds are synthesized
//! ([`Blip`], a short sine sweep with an envelope), so they weigh nothing and owe nothing to
//! anybody.
//!
//! - Croquettes (GD § 7 and 14, issue #14): a bright rising "pop" when the local player picks
//!   one up, a little higher with each croquette held; a falling "bloop" when it loses some.
//!
//! The sounds play only with Bevy's `AudioPlugin` (the game, not the headless tests: there the
//! players are spawned all the same, which the tests check, but nothing plays them). On the
//! web, the browser only lets sound out after a first click or key press in the page.

mod synth;

pub use synth::{Blip, BlipDecoder, SAMPLE_RATE, blip_sample};

use bevy::audio::{AddAudioSource, AudioPlugin, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::croquettes::{CroquettePickedUp, CroquettesLost};
use crate::kart::PlayerKart;

/// The synthesized sounds.
#[derive(Resource, Clone)]
pub struct Sounds {
    pub croquette: Handle<Blip>,
    pub croquette_lost: Handle<Blip>,
}

/// The pick-up "pop": an octave up in 90 ms.
pub const CROQUETTE_POP: Blip = Blip {
    from_hz: 700.0,
    to_hz: 1400.0,
    duration: 0.09,
    volume: 0.5,
};

/// Losing croquettes: down from 520 Hz to 170 Hz in a quarter of a second.
pub const CROQUETTE_LOST: Blip = Blip {
    from_hz: 520.0,
    to_hz: 170.0,
    duration: 0.25,
    volume: 0.55,
};

/// Each croquette held raises the pick-up pop by this much (playback speed): 10 croquettes are a
/// bit more than two semitones higher than the first one.
pub const POP_RISE_PER_CROQUETTE: f32 = 0.015;

/// Playback speed (pitch) of the pick-up pop when the kart now holds `count` croquettes.
pub fn pop_pitch(count: u8) -> f32 {
    1.0 + POP_RISE_PER_CROQUETTE * count.saturating_sub(1) as f32
}

/// A sound being played (despawned when it ends).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundEffect {
    CroquettePop,
    CroquettesLost,
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<AudioPlugin>() {
            app.add_audio_source::<Blip>();
        } else {
            app.init_asset::<Blip>();
        }
        app.add_systems(Startup, create_sounds)
            .add_systems(Update, play_croquette_sounds);
    }
}

fn create_sounds(mut commands: Commands, mut blips: ResMut<Assets<Blip>>) {
    commands.insert_resource(Sounds {
        croquette: blips.add(CROQUETTE_POP),
        croquette_lost: blips.add(CROQUETTE_LOST),
    });
}

/// The local player's croquettes: a pop per croquette picked up, a bloop when it loses some.
fn play_croquette_sounds(
    mut commands: Commands,
    sounds: Res<Sounds>,
    mut picked: MessageReader<CroquettePickedUp>,
    mut lost: MessageReader<CroquettesLost>,
    players: Query<(), With<PlayerKart>>,
) {
    for pick in picked.read() {
        if players.contains(pick.kart) {
            commands.spawn((
                SoundEffect::CroquettePop,
                AudioPlayer::<Blip>(sounds.croquette.clone()),
                PlaybackSettings::DESPAWN
                    .with_volume(Volume::Linear(1.0))
                    .with_speed(pop_pitch(pick.count)),
            ));
        }
    }
    for loss in lost.read() {
        if players.contains(loss.kart) {
            commands.spawn((
                SoundEffect::CroquettesLost,
                AudioPlayer::<Blip>(sounds.croquette_lost.clone()),
                PlaybackSettings::DESPAWN,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pop_rises_a_little_with_each_croquette() {
        assert_eq!(pop_pitch(0), 1.0);
        assert_eq!(pop_pitch(1), 1.0);
        assert!(pop_pitch(2) > pop_pitch(1));
        let ten = pop_pitch(10);
        // A bit more than two semitones (×1.122), less than three (×1.189).
        assert!(ten > 1.12 && ten < 1.19, "{ten}");
    }
}
