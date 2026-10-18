//! Sounds (GD § 14, issue #103: the sound design).
//!
//! - **The croquettes** (GD § 7 and 14, issue #14): a bright rising "pop" when the local player
//!   picks one up, a little higher with each croquette held; a falling "bloop" when it loses
//!   some. Synthesized ([`Blip`], a short sine sweep with an envelope): [`SoundPlugin`], headless
//!   too (the bot races' world has had it since #14: it stays as it is).
//! - **Everything else** (issue #103): the engines, the drift, the boosts, the start and the
//!   finish, the boxes and the roulette (GD § 8, issue #102: [`RouletteSounds`] and
//!   [`play_roulette`]), each item, the hits and shocks, the environments, the animals' cries,
//!   the screens and the lobby on foot: [`SoundDesignPlugin`] (`design.rs`), **only with a
//!   renderer**. Its clips are files of Kenney's CC0 packs (`assets/audio/`, `catalog::FILES`)
//!   and syntheses rendered when the game starts (`dsp.rs`: the engine, the cries, the whooshes
//!   are original); what plays what is `catalog.rs`, the mixer's rules `mixer.rs`, what a kart's
//!   tick sounds like `cues.rs`, the admin panel's mix `tuning.rs` ([`SoundTuning`]).
//!
//! - **The music** (issue #104): a piece per circuit, in a loop, faster on the last lap; the
//!   home's, the lobby's, the finish's jingles and the results' music by place, the podium's:
//!   [`MusicPlugin`] (`jukebox.rs`), **only with a renderer** too. What each screen plays and the
//!   fades are `music.rs` (pure: the catalog of `assets/data/music.ron`, [`MusicScene`], [`Program`],
//!   [`Deck`]); each piece is a file of `assets/music/` (CC0), decoded as it plays and looping
//!   without a gap (`stream.rs`, [`MusicStream`]); its mix is [`MusicTuning`].
//!
//! The sounds play only with Bevy's `AudioPlugin` (the game, not the headless tests: there the
//! sounds are entities all the same, a [`SoundEffect`] each, which the tests check, but nothing
//! plays them). On the web, the browser only lets sound out after a first click or key press in
//! the page.

mod catalog;
mod cues;
mod design;
mod dsp;
mod jukebox;
mod mixer;
mod music;
mod stream;
mod synth;
mod tuning;

#[cfg(test)]
mod music_tests;
#[cfg(test)]
mod scenario_tests;

pub use catalog::{Animal, Clip, FILES, Family, Mood, SoundEffect, base_volume, cry};
pub use cues::{Cues, KartSnapshot, TickEvents, kart_cues};
pub use design::{
    Cue, KartSounds, LoopVoice, MixerStats, SoundBank, SoundDesignPlugin, SoundDesignSystems,
    SoundOutput, Voice, mix, sound_level,
};
pub use dsp::{
    ENGINE_HZ, LoopKind, LoopSound, Noise, SQUEAL_HZ, Synth, SynthSound, Tone, Voice as CryVoice,
    Wave, render,
};
pub use jukebox::{
    Jukebox, MusicOutput, MusicPlugin, MusicSystems, MusicVoice, circuit_music, play_the_music,
    watch_the_scene,
};
pub use mixer::{Admission, Revs, admit, engine_note, nearest, option_gain, priority};
pub use music::{
    Catalog, Deck, Heard, MUSIC_RON, MusicScene, Piece, Program, Tier, catalog as music_catalog,
};
pub use stream::{Looper, MusicStream, SEAM_FRAMES};
pub use synth::{Blip, BlipDecoder, SAMPLE_RATE, blip_sample};
pub use tuning::{MusicTuning, SoundTuning};

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

/// Issue #102: the roulette plays its sounds (a tick, its stop). Only with the reel of the HUD
/// (`ui::RoulettePlugin`, with a renderer): the headless world has no such resource. Since #103,
/// they are files of Kenney's Interface Sounds (`catalog`: [`SoundEffect::RouletteTick`],
/// [`SoundEffect::RouletteStop`]), played by the mixer of [`SoundDesignPlugin`] like every
/// other sound (its volumes, its cap).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct RouletteSounds;

/// Plays a sound of the roulette: [`SoundEffect::RouletteTick`] or [`SoundEffect::RouletteStop`]
/// (the others are not the roulette's: nothing). The player's own: in front.
pub fn play_roulette(commands: &mut Commands, _sounds: &RouletteSounds, sound: SoundEffect) {
    if matches!(sound, SoundEffect::RouletteTick | SoundEffect::RouletteStop) {
        commands.spawn((sound, Cue::front(None)));
    }
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
