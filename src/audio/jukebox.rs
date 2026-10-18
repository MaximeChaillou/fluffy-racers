//! Issue #104: the music player (GD § 14, « La musique »). **Only with a renderer** (`GamePlugin`
//! adds it with the screen, the tests add it themselves): the bot races' world has none of it.
//!
//! - **What is shown** ([`watch_the_scene`], each frame): the home screen, the lobby, a circuit
//!   (before GO, racing, the player's last lap), the player's finish (its place), the podium of a
//!   championship: a [`MusicScene`] (`music.rs`), read from the game's state (nothing in the rules
//!   writes for the music).
//! - **What is heard** ([`play_the_music`], each frame): the scene's program played by the
//!   [`Deck`] (`music.rs`: one piece at a time, its fades), as a single entity, a [`MusicVoice`]
//!   (a new one for each piece), its volume set every frame (the options' general and music
//!   volumes, `MusicTuning::volume`, the piece's level, its fade, the pause's dip).
//! - **The files** (with an audio output only): the pieces of the scene and those that may come
//!   next are loaded (an `AudioSource`, its bytes), the others let go; each voice plays as a
//!   [`MusicStream`] (`stream.rs`: decoded as it plays, looping without a gap, its jingle first
//!   if it has one, chained on the sample).
//! - **The web**: the browser lets the sound out after the first click or key press only; the
//!   music plays into the suspended context until then (nothing heard, nothing breaks).

use bevy::audio::{
    AddAudioSource, AudioPlayer, AudioPlugin, AudioSink, AudioSinkPlayback, AudioSource,
    PlaybackMode, PlaybackSettings, Volume,
};
use bevy::prelude::*;

use super::mixer::{approach, option_gain};
use super::music::{Catalog, Deck, MusicScene, Tier, catalog};
use super::stream::{MusicStream, is_ogg};
use super::tuning::MusicTuning;
use crate::admin::Tunings;
use crate::kart::PlayerKart;
use crate::menu::MainMenu;
use crate::menu::MainMenuSystems;
use crate::options::PlayerOptions;
use crate::race::{Course, GrandPrix, GrandPrixStage, Race, RaceMode, RacePhase, RaceProgress};
use crate::track::{TrackChoice, TrackData, TrackHandle};

/// The music heard: one entity at a time, a new one for each piece.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MusicVoice {
    /// The piece (an index of the catalog's `pieces`)...
    pub piece: usize,
    /// ...after this jingle (the same voice, the jingle first).
    pub intro: Option<usize>,
    /// The piece is a jingle: once.
    pub jingle: bool,
    /// Its playback speed.
    pub speed: f32,
    /// Its volume, after the mix.
    pub level: f32,
    /// Its number (`Deck::started`).
    pub number: u32,
}

/// A piece's file, loaded for the scene (with an audio output only): ready once it is there and
/// an OGG file.
#[derive(Debug)]
struct Loaded {
    piece: usize,
    source: Handle<AudioSource>,
    ready: bool,
    failed: bool,
}

/// The music player's state.
#[derive(Resource, Debug)]
pub struct Jukebox {
    /// What is shown.
    pub scene: MusicScene,
    /// What is played.
    pub deck: Deck,
    /// The pause's dip (1: none, `MusicTuning::pause_volume` in the pause).
    pub duck: f32,
    voice: Option<Entity>,
    loaded: Vec<Loaded>,
    wanted: Vec<usize>,
    /// The player's kart and its tier when it finished (kept until it races again).
    finish: Option<(Entity, Tier)>,
}

impl Default for Jukebox {
    fn default() -> Self {
        Self {
            scene: MusicScene::Silent,
            deck: Deck::default(),
            duck: 1.0,
            voice: None,
            loaded: Vec::with_capacity(12),
            wanted: Vec::with_capacity(12),
            finish: None,
        }
    }
}

/// Whether the game has an audio output (Bevy's `AudioPlugin`): without one (the tests), the
/// music is only its entity.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicOutput(pub bool);

/// The music's systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MusicSystems;

/// The music (see the module's documentation).
pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        let output = app.is_plugin_added::<AudioPlugin>();
        if output {
            app.add_audio_source::<MusicStream>();
        } else {
            app.init_asset::<MusicStream>();
        }
        app.init_resource::<MusicTuning>()
            .init_resource::<Jukebox>()
            .insert_resource(MusicOutput(output))
            .add_systems(
                Update,
                (watch_the_scene, play_the_music)
                    .chain()
                    .in_set(MusicSystems)
                    .after(MainMenuSystems),
            );
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<MusicTuning>();
        }
    }
}

/// The piece of a circuit: its own (`music` in its file), or its biome's.
pub fn circuit_music(catalog: &Catalog, data: &TrackData) -> Option<usize> {
    catalog.index(data.music_id()?)
}

/// Each frame: what is shown (see the module's documentation).
#[allow(clippy::too_many_arguments)]
pub fn watch_the_scene(
    mut jukebox: ResMut<Jukebox>,
    tuning: Res<MusicTuning>,
    menu: Option<Res<MainMenu>>,
    (track, handle, tracks): (
        Option<Res<TrackChoice>>,
        Option<Res<TrackHandle>>,
        Res<Assets<TrackData>>,
    ),
    (race, course, grand_prix, mode): (
        Option<Res<Race>>,
        Option<Res<Course>>,
        Option<Res<GrandPrix>>,
        Option<Res<RaceMode>>,
    ),
    players: Query<(Entity, &RaceProgress), With<PlayerKart>>,
) {
    let jukebox = &mut *jukebox;
    let Some(catalog) = catalog() else {
        jukebox.scene = MusicScene::Silent;
        return;
    };
    let player = players.single().ok();
    // The player's tier at its finish, kept until it races again.
    match player {
        Some((kart, progress)) if progress.finished() => {
            if jukebox.finish.is_none_or(|(k, _)| k != kart) {
                jukebox.finish = Some((kart, Tier::of(progress.rank, &tuning)));
            }
        }
        _ => jukebox.finish = None,
    }
    jukebox.scene = if menu.is_some_and(|menu| menu.on_home()) {
        MusicScene::Home
    } else {
        match track.as_deref() {
            Some(TrackChoice::Salon) => MusicScene::Salon,
            Some(TrackChoice::Circuit(_)) => {
                let piece = handle
                    .and_then(|handle| tracks.get(&handle.0))
                    .and_then(|data| circuit_music(catalog, data));
                match piece {
                    None => MusicScene::Silent,
                    Some(_) if grand_prix.is_some_and(|gp| gp.stage == GrandPrixStage::Podium) => {
                        MusicScene::Podium
                    }
                    Some(piece) => {
                        let free = mode.is_some_and(|mode| *mode == RaceMode::FreeRoam);
                        circuit_scene(
                            piece,
                            (free, race.as_deref(), course.as_deref()),
                            player,
                            jukebox,
                        )
                    }
                }
            }
            _ => MusicScene::Silent,
        }
    };
}

/// A circuit's scene: before GO (or before its race is there), racing, the player's last lap,
/// its finish (or the results of a race it could not finish). Driving freely (`free`, no race),
/// its music.
fn circuit_scene(
    piece: usize,
    (free, race, course): (bool, Option<&Race>, Option<&Course>),
    player: Option<(Entity, &RaceProgress)>,
    jukebox: &Jukebox,
) -> MusicScene {
    let racing = |last_lap| MusicScene::Circuit {
        piece,
        started: true,
        last_lap,
    };
    let waiting = MusicScene::Circuit {
        piece,
        started: false,
        last_lap: false,
    };
    let race = match race {
        Some(race) => race,
        None if free => return racing(false),
        None => return waiting,
    };
    if !race.started() {
        return waiting;
    }
    if let Some((_, tier)) = jukebox.finish {
        return MusicScene::Finished(tier);
    }
    if race.flow.phase(&race.timings) == RacePhase::Results {
        return MusicScene::Finished(Tier::Bad);
    }
    let last_lap = match (player, course) {
        (Some((_, progress)), Some(course)) => {
            course.laps > 1 && progress.laps_completed(course) + 1 == course.laps
        }
        _ => false,
    };
    racing(last_lap)
}

/// Each frame: the scene's program played (see the module's documentation).
#[allow(clippy::too_many_arguments)]
pub fn play_the_music(
    mut commands: Commands,
    mut jukebox: ResMut<Jukebox>,
    time: Res<Time<Real>>,
    game_time: Res<Time<Virtual>>,
    tuning: Res<MusicTuning>,
    options: Res<PlayerOptions>,
    output: Res<MusicOutput>,
    server: Res<AssetServer>,
    (sources, mut streams): (
        Option<Res<Assets<AudioSource>>>,
        ResMut<Assets<MusicStream>>,
    ),
    mut voices: Query<(&mut MusicVoice, Option<&mut AudioSink>)>,
) {
    let Some(catalog) = catalog() else {
        return;
    };
    let jukebox = &mut *jukebox;
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    let scene = jukebox.scene;
    let program = scene.program(catalog, &tuning);
    if output.0
        && let Some(sources) = &sources
    {
        load_the_files(jukebox, catalog, &tuning, &server, sources);
    }
    let loaded = &jukebox.loaded;
    let ready = |piece: usize| !output.0 || loaded.iter().any(|l| l.piece == piece && l.ready);
    jukebox
        .deck
        .update(program, now, dt, |p| catalog.seconds(p), ready);
    // The pause (solo): the music dips.
    let dip = if game_time.is_paused() {
        tuning.pause_volume.clamp(0.0, 1.0)
    } else {
        1.0
    };
    jukebox.duck = approach(jukebox.duck, dip, tuning.pause_response.max(0.1), dt);
    let heard = jukebox.deck.heard;
    let number = jukebox.deck.started;
    // The voice of another piece (or of nothing) goes.
    if let Some(entity) = jukebox.voice
        && (heard.is_none() || voices.get(entity).is_ok_and(|(v, _)| v.number != number))
    {
        commands.entity(entity).despawn();
        jukebox.voice = None;
    }
    let Some(heard) = heard else {
        return;
    };
    let level = option_gain(options.master_volume)
        * option_gain(options.music_volume)
        * tuning.volume.clamp(0.0, 2.0)
        * catalog.pieces[heard.piece].volume
        * heard.gain.clamp(0.0, 1.0)
        * jukebox.duck;
    let voice = MusicVoice {
        piece: heard.piece,
        intro: heard.intro,
        jingle: heard.jingle,
        speed: heard.speed.clamp(0.25, 4.0),
        level,
        number,
    };
    match jukebox.voice.and_then(|entity| voices.get_mut(entity).ok()) {
        Some((mut playing, sink)) => {
            *playing = voice;
            if let Some(mut sink) = sink {
                if (sink.volume().to_linear() - level).abs() > 1e-4 {
                    sink.set_volume(Volume::Linear(level));
                }
                if (sink.speed() - voice.speed).abs() > 1e-4 {
                    sink.set_speed(voice.speed);
                }
            }
        }
        None => {
            let mut entity = commands.spawn(voice);
            let stream = sources
                .as_deref()
                .filter(|_| output.0)
                .and_then(|sources| stream_of(&voice, catalog, &jukebox.loaded, sources));
            if let Some(stream) = stream {
                entity.insert((
                    AudioPlayer::<MusicStream>(streams.add(stream)),
                    PlaybackSettings {
                        mode: PlaybackMode::Once,
                        volume: Volume::Linear(level),
                        speed: voice.speed,
                        ..PlaybackSettings::ONCE
                    },
                ));
            }
            jukebox.voice = Some(entity.id());
        }
    }
}

/// The stream of a voice: its piece's file (in a loop, or once for a jingle) after its intro's,
/// at the intro's level against the piece's. Its asset goes with the voice (the only handle).
fn stream_of(
    voice: &MusicVoice,
    catalog: &Catalog,
    loaded: &[Loaded],
    sources: &Assets<AudioSource>,
) -> Option<MusicStream> {
    let source = |piece: usize| {
        loaded
            .iter()
            .find(|l| l.piece == piece && l.ready)
            .and_then(|l| sources.get(&l.source))
    };
    let piece = &catalog.pieces[voice.piece];
    let looping = (!piece.jingle).then_some(piece.seconds);
    let stream = MusicStream::new(source(voice.piece)?, looping)?;
    Some(
        match voice.intro.and_then(|intro| Some((intro, source(intro)?))) {
            Some((intro, file)) => {
                let gain = catalog.pieces[intro].volume / piece.volume.max(1e-3);
                stream.after(file, gain)
            }
            None => stream,
        },
    )
}

/// The files of the scene's pieces (and of those that may come next) are loaded; the others are
/// let go (but those heard). Nothing allocated once the lists have room.
fn load_the_files(
    jukebox: &mut Jukebox,
    catalog: &Catalog,
    tuning: &MusicTuning,
    server: &AssetServer,
    sources: &Assets<AudioSource>,
) {
    jukebox.scene.wanted(catalog, tuning, &mut jukebox.wanted);
    let heard = jukebox.deck.heard;
    let wanted = &jukebox.wanted;
    jukebox.loaded.retain(|l| {
        wanted.contains(&l.piece)
            || heard.is_some_and(|h| h.piece == l.piece || h.intro == Some(l.piece))
    });
    for piece in wanted.iter().copied() {
        if !jukebox.loaded.iter().any(|l| l.piece == piece) {
            jukebox.loaded.push(Loaded {
                piece,
                source: server.load(catalog.pieces[piece].file.clone()),
                ready: false,
                failed: false,
            });
        }
    }
    for loaded in jukebox.loaded.iter_mut().filter(|l| !l.ready && !l.failed) {
        if let Some(source) = sources.get(&loaded.source) {
            if is_ogg(source) {
                loaded.ready = true;
            } else {
                let file = &catalog.pieces[loaded.piece].file;
                warn!("{file}: not an OGG file, the music leaves it out");
                loaded.failed = true;
            }
        } else if server.load_state(&loaded.source).is_failed() {
            loaded.failed = true;
        }
    }
}
