//! Issue #103: the sound design (GD § 14): the plugin that listens to the game and plays its
//! sounds. **Only with a renderer** (`GamePlugin` adds it with the screen, the tests add it
//! themselves): the bot races' world has none of it, and stays the one they were measured with.
//!
//! - **Watching** (nothing in the rules writes for the sounds): every tick, in `FixedPostUpdate`,
//!   each kart's state is compared with the last one (`cues::kart_cues`: its drift tiers, its
//!   boosts and their source, its hits, its tricks, the pelican, the environments, its items'
//!   effects, its paw), and the messages of the rules are read (the boxes, the items thrown and
//!   their blasts, the paws' strikes and tethers, the bumps between karts). Each frame, the race
//!   (the countdown, the cheer on the podium), the screens (their focus, their changes) and the
//!   lobby's characters on foot (the music, its jingles of the last lap and of the finish
//!   included, is `audio::jukebox`'s since #104). Each sound asked for is an entity: its
//!   [`SoundEffect`] and its [`Cue`] (which kart, where, the player's own or not).
//! - **The mixer** ([`mix`], in `PostUpdate`): each new sound gets a voice, at most
//!   `SoundTuning::max_sounds` at once (`mixer::admit`: the player's own and the screens' first,
//!   then the closest; a farther one is cut, or the new one is not played), its clip (a Kenney
//!   file or a synthesis rendered when the game started: [`SoundBank`]) and its volume (the
//!   options' general and effects volumes, its family's, its own); the others' sounds are
//!   spatialized (Bevy's `SpatialListener` on the camera), and not played beyond
//!   `SoundTuning::range`. A voice is let go when its sound ends ([`Voice::ends_at`]).
//! - **The loops** ([`LoopVoice`], [`drive_loops`]): always the same entities, never spawned
//!   again: the engines (the player's at the front, then the closest, `max_engines` in all), the
//!   drifts' squeal (`max_squeals`), the glider's wind and the rumble off the road (the
//!   player's). Their pitch and volume follow their kart every frame (`mixer::engine_note`); a
//!   voice with nothing to play is paused.
//! - **The web**: the browser lets the sound out after the first click or key press only
//!   (`index.html` resumes Bevy's audio context then); before, the sinks play into a suspended
//!   context: nothing is heard, nothing breaks.
//! - **Only in a race** (issue #135, [`RaceSounds`]): the loops (the engines, the squeals, the
//!   wind, the rumble) play from the countdown to the player's finish, and while driving freely;
//!   not on the home screen (its background race is silent: only the music), nor in the lobby,
//!   the pause, or after the player's finish (its results, the standings, the podium): they fade
//!   out (`engine_response`). The karts' and the items' short sounds follow the same rule.
//! - **The sound picker** (issue #135, `choice.rs`): each sound plays the candidate Max chose in
//!   the admin panel ([`SoundChoices`]): the mixer picks its clip (`choice::pick`), the loops
//!   change their sound at once (a quick fade, then the new one), « ▶ Écouter » plays a slot's
//!   sound at once, even out of a race ([`PreviewSound`]: the engine a short rev). A file or a
//!   synthesis is loaded or rendered once, when it is chosen; a recorded loop is decoded once.

use bevy::audio::{
    AddAudioSource, AudioPlayer, AudioPlugin, AudioSink, AudioSinkPlayback, AudioSource,
    PlaybackMode, PlaybackSettings, SpatialAudioSink, SpatialListener, SpatialScale, Volume,
};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_rapier3d::prelude::Velocity;

use super::catalog::{Animal, Clip, FILES, Mood, SoundEffect, base_volume};
use super::choice::{Picked, Slot, SoundChoices, Source, pick};
use super::cues::{KartSnapshot, TickEvents, kart_cues};
use super::dsp::{LoopKind, LoopSound, SynthSound, seamless_loop};
use super::mixer::{
    Admission, RaceSounds, Revs, Stage, admit, approach, engine_note, nearest, option_gain,
    priority, race_sounds,
};
use super::stream::is_ogg;
use super::tuning::SoundTuning;
use crate::admin::Tunings;
use crate::camera::ChaseCamera;
use crate::characters::{Roster, SelectionAction, SelectionScreen};
use crate::croquettes::CroquettePickedUp;
use crate::interactions::{KartGrabbed, Paw, StrikeAnswer, TetherEnd, TetherEnded};
use crate::items::{
    Blast, BoxBroken, Honked, ItemBlocked, ItemEffect, ItemHitJudged, ItemKind, ItemObject,
    ItemSlots, ItemStolen, Motion, Slot as ItemSlot,
};
use crate::kart::{
    Boost, DriverVisual, ItemEffects, Kart, KartAssists, KartBodyPivot, KartBump, KartEnvironment,
    KartInput, KartState, KartTuning, KartWheels, MaxSpeedScale, PlayerKart, Remote, Slipstream,
    Stall, StatEffects, StepVelocity,
};
use crate::menu::{MainMenu, MainMenuAction};
use crate::modes::{GrandPrixAction, RaceMenu, RaceMenuAction};
use crate::net::LobbyWindow;
use crate::options::{OptionsAction, OptionsMenu, PlayerOptions};
use crate::race::{Course, Race, RaceMode, RaceProgress, Tow};
use crate::salon::{PlayerWalker, SalonAction, SalonMenu, Walker, WalkerLook};
use crate::track::{Breakable, TrackChoice};
use crate::vfx::KartVfx;

/// A sound asked for by the game, with its [`SoundEffect`]: whose it is (`kart`), where (`at`),
/// the player's own or a screen's (`foreground`: in front, not spatialized), its playback speed
/// (pitch) and its gain (0 to 1, on its mixed volume). A [`SoundEffect`] without it is the
/// player's (the croquettes' blips, the roulette's).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub kart: Option<Entity>,
    pub at: Vec3,
    pub foreground: bool,
    pub speed: f32,
    pub gain: f32,
}

impl Cue {
    /// The player's own (or a screen's): in front.
    pub fn front(kart: Option<Entity>) -> Self {
        Self {
            kart,
            at: Vec3::ZERO,
            foreground: true,
            speed: 1.0,
            gain: 1.0,
        }
    }

    /// Another kart's, or the track's: heard from `at`.
    pub fn at(kart: Option<Entity>, at: Vec3) -> Self {
        Self {
            kart,
            at,
            foreground: false,
            speed: 1.0,
            gain: 1.0,
        }
    }
}

/// A short sound playing: it holds one of the `max_sounds` voices until `ends_at` (real time,
/// s), with its `priority` (`mixer::priority`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub started: f64,
    pub ends_at: f64,
    pub priority: f32,
}

/// A continuous voice: an engine, a squeal, the wind or the rumble. `foreground`: the player's
/// (in front), else one of the others' (`slot`: spatialized, heard from its kart). `kart`: whose
/// it plays now; `pitch` and `volume`: what it plays (after the mix; `pitch` is the note of the
/// revs, the sink plays it times its candidate's speed). Issue #135: `source`, the candidate of
/// its slot it plays (`SoundChoices`), none before its sound is ready.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LoopVoice {
    pub kind: LoopKind,
    pub foreground: bool,
    pub slot: u8,
    pub kart: Option<Entity>,
    pub pitch: f32,
    pub volume: f32,
    pub source: Option<u8>,
}

/// The clips: the files (loaded) and the syntheses (rendered once), made when the game starts,
/// with an audio output only. Issue #135: the candidates' files, syntheses and loops are added
/// when they are chosen (once each), keyed by their slot and their index in it.
#[derive(Resource, Clone, Debug)]
pub struct SoundBank {
    pub files: HashMap<&'static str, Handle<AudioSource>>,
    pub synths: HashMap<SoundEffect, Handle<SynthSound>>,
    pub alternatives: HashMap<(Slot, u8), Handle<SynthSound>>,
    pub loops: HashMap<(Slot, u8), Handle<LoopSound>>,
}

/// Issue #135: « ▶ Écouter » in the admin panel: plays `Slot`'s chosen sound at once, even out of
/// a race (a loop: a short sample of it, the engine a rev).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewSound(pub Slot);

/// Issue #135: a loop listened to (« ▶ Écouter »): the candidate it plays, when it started (real
/// time, once its sound is there), its pitch and volume now.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LoopPreview {
    pub kind: LoopKind,
    pub source: u8,
    pub started: Option<f64>,
    pub pitch: f32,
    pub volume: f32,
}

/// A one-shot listened to (« ▶ Écouter »): the mixer plays it like the others.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preview;

/// What the mixer did since the game started (the tests read it, and so can a debugger).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerStats {
    /// Sounds given a voice.
    pub played: u32,
    /// Sounds not played: every voice played something at least as important.
    pub refused: u32,
    /// Sounds cut to let a more important one play.
    pub cut: u32,
    /// Sounds too far away (beyond `SoundTuning::range`), or asked twice at once.
    pub skipped: u32,
    /// The most voices playing at once.
    pub most_voices: u32,
}

/// The sound design's memory of a kart (on each kart, with this plugin only): its last tick, its
/// animal, its last cry.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KartSounds {
    pub last: KartSnapshot,
    pub seen: bool,
    pub animal: Animal,
    pub named: bool,
    pub last_cry: f32,
}

/// The sound design's memory of an item out: how it moved on the last tick.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectSounds {
    motion: u8,
    bounces: u32,
    grounded: bool,
}

/// The sound design's memory of a character on foot (the lobby, issue #101).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WalkerSounds {
    last: WalkerLook,
    stride: f32,
    steps: u8,
}

/// Whether the game has an audio output (Bevy's `AudioPlugin`): without one (the tests), the
/// sounds are only entities.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundOutput(pub bool);

/// The sound design's systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SoundDesignSystems;

/// The sound design (see the module's documentation).
pub struct SoundDesignPlugin;

impl Plugin for SoundDesignPlugin {
    fn build(&self, app: &mut App) {
        let output = app.is_plugin_added::<AudioPlugin>();
        if output {
            app.add_audio_source::<SynthSound>()
                .add_audio_source::<LoopSound>();
        } else {
            app.init_asset::<SynthSound>().init_asset::<LoopSound>();
        }
        app.init_resource::<SoundTuning>()
            .init_resource::<MixerStats>()
            .init_resource::<SoundChoices>()
            .init_resource::<RaceSounds>()
            .insert_resource(SoundOutput(output))
            .add_message::<PreviewSound>()
            // The screens' actions it listens to (there in the game; the tests may lack some).
            .add_message::<MainMenuAction>()
            .add_message::<SalonAction>()
            .add_systems(
                Startup,
                create_bank.run_if(|output: Res<SoundOutput>| output.0),
            )
            .add_systems(
                FixedPostUpdate,
                (remember_karts, watch_karts, watch_items)
                    .chain()
                    .in_set(SoundDesignSystems),
            )
            .add_systems(
                Update,
                (
                    listen,
                    name_animals,
                    watch_stage,
                    watch_race,
                    watch_screens,
                    watch_walkers,
                    (prepare_choices, play_previews).chain(),
                )
                    .in_set(SoundDesignSystems),
            )
            .add_systems(
                PostUpdate,
                (mix, drive_loops, drive_previews)
                    .chain()
                    .in_set(SoundDesignSystems)
                    .before(TransformSystems::Propagate),
            );
        // Its tunings in the admin panel, next to the others (the game's).
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<SoundTuning>();
        }
    }
}

/// The clips: the files to load, the syntheses rendered (once). The loops and the candidates'
/// clips are added when they are chosen ([`prepare_choices`]).
fn create_bank(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut synths: ResMut<Assets<SynthSound>>,
) {
    let files = FILES
        .iter()
        .map(|(path, _)| (*path, server.load(*path)))
        .collect();
    let synths = SoundEffect::synthesized()
        .filter_map(|sound| match sound.clip() {
            Clip::Synth(synth) => Some((sound, synths.add(SynthSound::new(&synth, 0.9)))),
            _ => None,
        })
        .collect();
    commands.insert_resource(SoundBank {
        files,
        synths,
        alternatives: HashMap::default(),
        loops: HashMap::default(),
    });
}

/// A recorded loop is crossfaded over this long where it starts again (s)...
const LOOP_SEAM: f32 = 0.04;
/// ...and brought to this level (RMS), about the syntheses' (each candidate has its gain).
const RECORDED_RMS: f32 = 0.3;

/// Issue #135: the candidates chosen get their clips, once each (files loaded, syntheses rendered,
/// loops built; a recorded loop decoded when its file is there). Nothing to do most frames.
fn prepare_choices(
    choices: Res<SoundChoices>,
    bank: Option<ResMut<SoundBank>>,
    server: Res<AssetServer>,
    sources: Option<Res<Assets<AudioSource>>>,
    mut synths: ResMut<Assets<SynthSound>>,
    mut loops: ResMut<Assets<LoopSound>>,
    mut recordings: Local<Vec<(Slot, u8)>>,
) {
    let Some(mut bank) = bank else {
        return;
    };
    if choices.is_changed() {
        for slot in Slot::ALL {
            let index = choices.index(slot) as u8;
            let candidate = choices.candidate(slot);
            for (path, _) in candidate.files() {
                bank.files.entry(path).or_insert_with(|| server.load(*path));
            }
            match candidate.source {
                Source::Synth(synth) => {
                    bank.alternatives
                        .entry((slot, index))
                        .or_insert_with(|| synths.add(SynthSound::new(&synth, 0.9)));
                }
                Source::Loop(timbre) => {
                    bank.loops
                        .entry((slot, index))
                        .or_insert_with(|| loops.add(LoopSound::synth(timbre)));
                }
                Source::Recording { path, .. } => {
                    bank.files.entry(path).or_insert_with(|| server.load(path));
                    if !bank.loops.contains_key(&(slot, index))
                        && !recordings.contains(&(slot, index))
                    {
                        recordings.push((slot, index));
                    }
                }
                _ => {}
            }
        }
    }
    let Some(sources) = sources else {
        return;
    };
    // The recordings whose file is there: decoded once into a seamless loop.
    recordings.retain(|(slot, index)| {
        let candidate = &slot.candidates()[usize::from(*index)];
        let Source::Recording { path, lowpass, .. } = candidate.source else {
            return false;
        };
        let Some(handle) = bank.files.get(path) else {
            return false;
        };
        let Some(source) = sources.get(handle) else {
            let failed = server.load_state(handle).is_failed();
            if failed {
                warn!("{path}: not loaded, its loop stays silent");
            }
            return !failed;
        };
        if !is_ogg(source) {
            warn!("{path}: not an OGG file, its loop stays silent");
            return false;
        }
        let decoder = bevy::audio::Decodable::decoder(source);
        let channels = usize::from(bevy::audio::Source::channels(&decoder).get());
        let rate = bevy::audio::Source::sample_rate(&decoder).get();
        let samples: Vec<f32> = decoder.collect();
        let looped = seamless_loop(&samples, channels, rate, lowpass, LOOP_SEAM, RECORDED_RMS);
        bank.loops
            .insert((*slot, *index), loops.add(LoopSound::recorded(looped)));
        false
    });
}

/// Issue #135: where the karts may be heard (see [`RaceSounds`]), from what is shown: the home
/// screen, the lobby, the pause, the race's phase, the player's finish.
#[allow(clippy::too_many_arguments)]
fn watch_stage(
    mut sounds: ResMut<RaceSounds>,
    menu: Option<Res<MainMenu>>,
    track: Option<Res<TrackChoice>>,
    time: Res<Time<Virtual>>,
    (race, mode): (Option<Res<Race>>, Option<Res<RaceMode>>),
    players: Query<&RaceProgress, With<PlayerKart>>,
) {
    let stage = Stage {
        home: on_home(menu.as_deref()),
        salon: track.as_deref() == Some(&TrackChoice::Salon),
        paused: time.is_paused(),
        phase: race.map(|race| race.phase()),
        // No race to wait for: the test ground (no race there), or « Roulage libre ».
        free: track.as_deref() == Some(&TrackChoice::TestGround)
            || mode.is_some_and(|mode| *mode == RaceMode::FreeRoam),
        finished: players.iter().any(RaceProgress::finished),
    };
    sounds.set_if_neq(race_sounds(&stage));
}

/// The camera listens (one `SpatialListener`, on the chase camera): the others' sounds come
/// from where they are.
fn listen(
    mut commands: Commands,
    cameras: Query<Entity, (With<ChaseCamera>, Without<SpatialListener>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(SpatialListener::new(2.0));
    }
}

/// Behind the home screen (its background race).
fn on_home(menu: Option<&MainMenu>) -> bool {
    menu.is_some_and(MainMenu::on_home)
}

/// Spawns a sound.
fn play(commands: &mut Commands, sound: SoundEffect, cue: Cue) {
    commands.spawn((sound, cue, Transform::from_translation(cue.at)));
}

/// A kart's sound: in front for the player's, from the kart for the others'.
fn kart_cue(kart: Entity, at: Vec3, player: bool) -> Cue {
    if player {
        Cue::front(Some(kart))
    } else {
        Cue::at(Some(kart), at)
    }
}

/// A kart's cry, if it did not cry just now: its animal's voice, a little higher or lower for
/// each kart (two cats never sound the same).
fn cry(
    commands: &mut Commands,
    memory: &mut KartSounds,
    mood: Mood,
    cue: Cue,
    now: f32,
    tuning: &SoundTuning,
) {
    if now - memory.last_cry < tuning.cry_interval && memory.last_cry > 0.0 {
        return;
    }
    memory.last_cry = now.max(1e-3);
    let spread = cue
        .kart
        .map_or(0.0, |kart| (kart.index_u32() % 5) as f32 - 2.0);
    play(
        commands,
        SoundEffect::Cry(memory.animal, mood),
        Cue {
            speed: 1.0 + 0.035 * spread,
            ..cue
        },
    );
}

/// Every kart gets its memory.
fn remember_karts(
    mut commands: Commands,
    karts: Query<Entity, (Or<(With<Kart>, With<Remote>)>, Without<KartSounds>)>,
) {
    for kart in &karts {
        commands.entity(kart).insert(KartSounds::default());
    }
}

/// Each kart's animal, from its driver (its character's model).
fn name_animals(
    roster: Option<Res<Roster>>,
    drivers: Query<(&DriverVisual, &ChildOf)>,
    pivots: Query<&KartBodyPivot>,
    mut karts: Query<&mut KartSounds>,
) {
    for (driver, child_of) in &drivers {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(mut memory) = karts.get_mut(pivot.body) else {
            continue;
        };
        if memory.named {
            continue;
        }
        let model = driver.character.and_then(|character| {
            roster
                .as_ref()
                .and_then(|r| r.characters.get(character))
                .and_then(|c| c.model.as_deref())
        });
        memory.animal = Animal::of_model(model);
        memory.named = true;
    }
}

type KartView = (
    Entity,
    &'static mut KartSounds,
    &'static Transform,
    (
        &'static KartState,
        &'static Boost,
        &'static Stall,
        &'static Slipstream,
        &'static KartWheels,
        &'static KartEnvironment,
        &'static ItemEffects,
    ),
    (
        Option<&'static StatEffects>,
        Option<&'static KartAssists>,
        Option<&'static Tow>,
        Option<&'static ItemSlots>,
        Option<&'static Paw>,
    ),
    (Option<&'static Velocity>, Option<&'static StepVelocity>),
    Has<PlayerKart>,
);

/// What the sound design sees of a kart now.
fn snapshot(
    kart_tuning: &KartTuning,
    (state, boost, stall, slipstream, wheels, environment, effects): (
        &KartState,
        &Boost,
        &Stall,
        &Slipstream,
        &KartWheels,
        &KartEnvironment,
        &ItemEffects,
    ),
    (stats, assists, tow, slots, paw): (
        Option<&StatEffects>,
        Option<&KartAssists>,
        Option<&Tow>,
        Option<&ItemSlots>,
        Option<&Paw>,
    ),
    (velocity, step): (Option<&Velocity>, Option<&StepVelocity>),
) -> KartSnapshot {
    let assisted = assists.is_some_and(|a| a.assisted_steering);
    let tier = if matches!(state, KartState::Drifting { .. }) {
        match stats {
            Some(stats) => state.mini_turbo(&stats.tuning(kart_tuning), assisted),
            None => state.mini_turbo(kart_tuning, assisted),
        }
    } else {
        None
    };
    // The shock of the physics step: what it took from the speed, across the kart's up (a
    // landing takes the speed along it).
    let impact = match (velocity, step) {
        (Some(velocity), Some(step)) => {
            let up = environment.up;
            let lost = step.linear - velocity.linear;
            (lost - up * lost.dot(up)).length()
        }
        _ => 0.0,
    };
    KartSnapshot {
        state: *state,
        tier,
        boost: boost.remaining,
        stalled: stall.is_active(),
        slip_charge: slipstream.charge,
        slip_charging: slipstream.charging,
        airborne_ticks: wheels.ticks_without_ground,
        gliding: environment.gliding,
        underwater: environment.underwater,
        anti_gravity: environment.anti_gravity,
        carried: tow.and_then(|t| t.carry.as_ref()).map(|c| c.reason),
        hand: slots.and_then(|s| match s.hand {
            ItemSlot::Ready { item, .. } => Some(item),
            _ => None,
        }),
        hand_count: slots.map_or(0, |s| match s.hand {
            ItemSlot::Ready { count, .. } => count,
            _ => 0,
        }),
        golden: slots.is_some_and(|s| s.golden > 0),
        pinata: slots.map_or(0, |s| s.pinata),
        calin: effects.calin > 0.0,
        rocket: effects.rocket > 0.0,
        deflated: effects.deflated > 0.0,
        creamed: effects.creamed > 0.0,
        ghost: effects.ghost > 0.0,
        impact,
        swing: paw.and_then(|p| p.swing).map(|s| (s.kind, s.ticks)),
    }
}

/// Each tick: each kart's sounds (`cues::kart_cues`), the bumps between karts and the paws.
fn watch_karts(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<SoundTuning>,
    kart_tuning: Res<KartTuning>,
    race: Option<Res<Race>>,
    stage: Res<RaceSounds>,
    (mut picked, mut bumps, mut strikes, mut grabs, mut tethers): (
        MessageReader<CroquettePickedUp>,
        MessageReader<KartBump>,
        MessageReader<StrikeAnswer>,
        MessageReader<KartGrabbed>,
        MessageReader<TetherEnded>,
    ),
    mut karts: Query<KartView>,
    mut ticked: Local<(Vec<Entity>, Vec<Entity>, Vec<(Entity, Entity)>)>,
) {
    // Issue #135: out of a race (the home's background race, after the player's finish...),
    // the karts make no sound.
    let home = !stage.karts;
    let now = time.elapsed_secs();
    let dt = time.delta_secs().max(1e-4);
    let (croquettes, bumped, pairs) = &mut *ticked;
    croquettes.clear();
    bumped.clear();
    pairs.clear();
    croquettes.extend(picked.read().map(|p| p.kart));
    let go = race.as_ref().is_some_and(|race| race.race_ticks() == 1);
    // The bumps (GD § 7): one sound per pair of karts, louder the harder.
    for bump in bumps.read() {
        bumped.extend([bump.kart, bump.by]);
        let pair = (bump.kart.min(bump.by), bump.kart.max(bump.by));
        if home || pairs.contains(&pair) {
            continue;
        }
        pairs.push(pair);
        let Ok((_, _, transform, .., player)) = karts.get(bump.kart) else {
            continue;
        };
        let player = player || karts.get(bump.by).is_ok_and(|k| k.6);
        let mut cue = kart_cue(bump.kart, transform.translation, player);
        cue.gain = (bump.closing_speed / 15.0).clamp(0.4, 1.0);
        play(&mut commands, SoundEffect::KartBump, cue);
    }
    for (kart, mut memory, transform, rules, items, physics, player) in &mut karts {
        let now_snapshot = snapshot(&kart_tuning, rules, items, physics);
        let last = std::mem::replace(&mut memory.last, now_snapshot);
        if !std::mem::replace(&mut memory.seen, true) || home {
            continue;
        }
        let events = TickEvents {
            go,
            croquette: croquettes.contains(&kart),
            bumped: bumped.contains(&kart),
        };
        let cues = kart_cues(&last, &now_snapshot, events, &tuning, dt);
        let cue = kart_cue(kart, transform.translation, player);
        for sound in cues.sounds() {
            // The cream lands on the player's screen only.
            if *sound == SoundEffect::Creamed && !player {
                continue;
            }
            play(&mut commands, *sound, cue);
        }
        if let Some(mood) = cues.cry {
            cry(&mut commands, &mut memory, mood, cue, now, &tuning);
        }
    }
    if home {
        strikes.clear();
        grabs.clear();
        tethers.clear();
        return;
    }
    // The paws (GD § 14 and 15): the « pof » and the target's cry, the striker's effort; the
    // grab; the « boing » of a tether that lets go.
    for strike in strikes.read().filter(|s| s.landed) {
        let player = [strike.striker, strike.target]
            .iter()
            .any(|k| karts.get(*k).is_ok_and(|k| k.6));
        if let Ok((target, mut memory, transform, .., is_player)) = karts.get_mut(strike.target) {
            let cue = kart_cue(target, transform.translation, player);
            play(&mut commands, SoundEffect::PawPof, cue);
            let cue = kart_cue(target, transform.translation, is_player);
            cry(&mut commands, &mut memory, Mood::Hurt, cue, now, &tuning);
        }
        if let Ok((striker, mut memory, transform, .., is_player)) = karts.get_mut(strike.striker) {
            let cue = kart_cue(striker, transform.translation, is_player);
            cry(&mut commands, &mut memory, Mood::Effort, cue, now, &tuning);
        }
    }
    for grab in grabs.read() {
        if let Ok((grabber, _, transform, .., is_player)) = karts.get(grab.grabber) {
            let player = is_player || karts.get(grab.target).is_ok_and(|k| k.6);
            play(
                &mut commands,
                SoundEffect::Grab,
                kart_cue(grabber, transform.translation, player),
            );
        }
    }
    for tether in tethers.read().filter(|t| t.reason != TetherEnd::Gone) {
        if let Ok((grabber, _, transform, .., is_player)) = karts.get(tether.grabber) {
            let player = is_player || karts.get(tether.target).is_ok_and(|k| k.6);
            play(
                &mut commands,
                SoundEffect::TetherBoing,
                kart_cue(grabber, transform.translation, player),
            );
        }
    }
}

/// An item's motion, as a number (its kind of move).
fn motion_kind(motion: &Motion) -> u8 {
    match motion {
        Motion::Held(_) => 0,
        Motion::Ground => 1,
        Motion::Rolling { .. } => 2,
        Motion::Homing { .. } => 3,
        Motion::Bomb { .. } => 4,
        Motion::Pillow { .. } => 5,
        Motion::Boomerang { .. } => 6,
    }
}

/// The sound of an item starting a move: dropped, thrown, launched.
fn launch_sound(motion: u8) -> Option<SoundEffect> {
    match motion {
        1 => Some(SoundEffect::Splotch),
        2 | 4 => Some(SoundEffect::Throw),
        3 => Some(SoundEffect::BeeBuzz),
        5 => Some(SoundEffect::PillowLaunch),
        // A boomerang-os: heard from its thrower's hand (`cues::kart_cues`).
        _ => None,
    }
}

/// Its bounces left (a pelote, a boomerang) and whether it lies on the ground (a bombe).
fn bounces(motion: &Motion) -> (u32, bool) {
    match *motion {
        Motion::Rolling { bounces_left, .. } | Motion::Boomerang { bounces_left, .. } => {
            (bounces_left, false)
        }
        Motion::Bomb { grounded, .. } => (0, grounded),
        _ => (0, false),
    }
}

/// A kart slides on a Cac's (issue #137): its « splotch » this much lower (the playback speed),
/// heavier than the Cac's landing.
const SLID_SPLOTCH_SPEED: f32 = 0.8;

/// Each tick: the items (GD § 8): the boxes, what is thrown and dropped, what bounces, the
/// blasts, the chantilly, the klaxon-canard, the pie voleuse's theft, the items that break on
/// each other, a kart sliding on a Cac's (its victim's decision, `ItemHitJudged`: online, the
/// karts this peer judges).
fn watch_items(
    mut commands: Commands,
    stage: Res<RaceSounds>,
    players: Query<(), With<PlayerKart>>,
    karts: Query<&Transform, Or<(With<Kart>, With<Remote>)>>,
    (mut boxes, mut blasts, mut effects, mut honks, mut stolen, mut blocked, mut judged): (
        MessageReader<BoxBroken>,
        MessageReader<Blast>,
        MessageReader<ItemEffect>,
        MessageReader<Honked>,
        MessageReader<ItemStolen>,
        MessageReader<ItemBlocked>,
        MessageReader<ItemHitJudged>,
    ),
    mut objects: Query<(Entity, &ItemObject, Option<&mut ObjectSounds>)>,
    mut last_seen: Local<Vec<(Entity, Vec3)>>,
    // Issue #144: the things to drive through of the circuit (none on most circuits).
    breakables: Query<(&Transform, &Breakable)>,
) {
    // Issue #135: out of a race, the items make no sound either.
    let home = !stage.karts;
    let cue_of = |kart: Entity, at: Vec3| {
        if players.contains(kart) {
            Cue::front(Some(kart))
        } else {
            Cue::at(Some(kart), at)
        }
    };
    let kart_at = |kart: Entity| karts.get(kart).map_or(Vec3::ZERO, |t| t.translation);
    if home {
        boxes.clear();
        blasts.clear();
        effects.clear();
        honks.clear();
        stolen.clear();
        judged.clear();
    }
    for broken in boxes.read() {
        let cue = cue_of(broken.kart, kart_at(broken.kart));
        play(&mut commands, SoundEffect::BoxBreak, cue);
    }
    // Issue #144: a hay bale, a balloon or a crate burst under a kart: a « pof ».
    for (transform, breakable) in &breakables {
        if let Some(kart) = breakable.burst_by.filter(|_| !home) {
            play(
                &mut commands,
                SoundEffect::PawPof,
                cue_of(kart, transform.translation),
            );
        }
    }
    for blast in blasts.read() {
        let sound = if blast.item == ItemKind::OreillerGeant {
            SoundEffect::PillowFall
        } else {
            SoundEffect::Explosion
        };
        // Heard from where it blows, whoever threw it.
        play(&mut commands, sound, Cue::at(Some(blast.owner), blast.at));
    }
    for effect in effects.read() {
        if effect.item == ItemKind::Chantilly {
            play(
                &mut commands,
                SoundEffect::CreamSquirt,
                cue_of(effect.by, effect.at),
            );
        }
    }
    for honk in honks.read() {
        play(&mut commands, SoundEffect::Honk, cue_of(honk.kart, honk.at));
    }
    // A kart ran over a Cac's (hit, or immune: it squashed it all the same).
    for hit in judged.read().filter(|hit| hit.item == ItemKind::Pelure) {
        let cue = Cue {
            speed: SLID_SPLOTCH_SPEED,
            ..cue_of(hit.kart, kart_at(hit.kart))
        };
        play(&mut commands, SoundEffect::Splotch, cue);
    }
    for theft in stolen.read() {
        let player = players.contains(theft.thief) || players.contains(theft.victim);
        let at = kart_at(theft.thief);
        let cue = if player {
            Cue::front(Some(theft.thief))
        } else {
            Cue::at(Some(theft.thief), at)
        };
        play(&mut commands, SoundEffect::Steal, cue);
    }
    // The items that broke on each other: where they were last seen.
    for broken in blocked.read() {
        if home {
            continue;
        }
        if let Some((_, at)) = last_seen.iter().find(|(e, _)| *e == broken.object) {
            play(&mut commands, SoundEffect::ItemBlocked, Cue::at(None, *at));
        }
    }
    last_seen.clear();
    for (entity, object, memory) in &mut objects {
        last_seen.push((entity, object.position));
        let kind = motion_kind(&object.motion);
        let (bounces_left, grounded) = bounces(&object.motion);
        let now = ObjectSounds {
            motion: kind,
            bounces: bounces_left,
            grounded,
        };
        let sound = match memory {
            None => {
                commands.entity(entity).insert(now);
                launch_sound(kind)
            }
            Some(mut memory) => {
                let last = std::mem::replace(&mut *memory, now);
                if last.motion != kind {
                    launch_sound(kind)
                } else if bounces_left < last.bounces || (grounded && !last.grounded) {
                    Some(SoundEffect::PeloteBounce)
                } else {
                    None
                }
            }
        };
        if let Some(sound) = sound.filter(|_| !home) {
            play(&mut commands, sound, cue_of(object.owner, object.position));
        }
    }
}

/// What the race watcher remembers.
#[derive(Default)]
struct RaceMemory {
    countdown: Option<u32>,
    go: bool,
    kart: Option<Entity>,
    laps: u32,
    finished: bool,
}

/// Each frame: the countdown's beeps and its GO, and the player's finish on the podium: its
/// animal's cheer. The last lap's and the finish's jingles are the music's (issue #104,
/// `audio::jukebox`).
fn watch_race(
    mut commands: Commands,
    tuning: Res<SoundTuning>,
    time: Res<Time>,
    race: Option<Res<Race>>,
    course: Option<Res<Course>>,
    menu: Option<Res<MainMenu>>,
    mut players: Query<(Entity, &RaceProgress, Option<&mut KartSounds>), With<PlayerKart>>,
    mut memory: Local<RaceMemory>,
) {
    let (Some(race), Some(course)) = (race, course) else {
        *memory = RaceMemory::default();
        return;
    };
    if on_home(menu.as_deref()) {
        *memory = RaceMemory::default();
        return;
    }
    let number = race.flow.countdown_number(&race.timings);
    if number.is_some() && number != memory.countdown {
        play(&mut commands, SoundEffect::CountdownBeep, Cue::front(None));
    }
    memory.countdown = number;
    let go = race.flow.shows_go(&race.timings);
    if go && !memory.go {
        play(&mut commands, SoundEffect::CountdownGo, Cue::front(None));
    }
    memory.go = go;
    let Ok((kart, progress, sounds)) = players.single_mut() else {
        memory.kart = None;
        return;
    };
    let laps = progress.laps_completed(&course);
    let finished = progress.finished();
    if memory.kart != Some(kart) || laps < memory.laps {
        // A new kart, or the race again: nothing to play.
        memory.kart = Some(kart);
        memory.laps = laps;
        memory.finished = finished;
        return;
    }
    if finished && !memory.finished {
        let podium = progress.rank >= 1 && progress.rank <= 3;
        if podium && let Some(mut sounds) = sounds {
            let now = time.elapsed_secs();
            cry(
                &mut commands,
                &mut sounds,
                Mood::Win,
                Cue::front(Some(kart)),
                now,
                &tuning,
            );
        }
    }
    memory.laps = laps;
    memory.finished = finished;
}

/// What the screens' watcher remembers: each screen open, its focus, the last choice, the last
/// « retour ».
#[derive(Default)]
struct ScreenMemory {
    screens: [(bool, usize); 5],
    confirmed_at: f64,
    backed_at: Option<f64>,
}

/// A screen's close is seen twice when this system runs between the screen's systems (its
/// action one frame, its state closed the next): one « retour » for both, within this long (s).
const SAME_BACK: f64 = 0.1;

/// Each frame: the screens (docs/UI_CHARTE.md): the focus moves (survol), a value changes, a
/// choice (validation), a screen closes (retour). Their actions and their state, read: the
/// screens write nothing for the sounds.
#[allow(clippy::too_many_arguments)]
fn watch_screens(
    mut commands: Commands,
    time: Res<Time<Real>>,
    (main, options, solo, pilot, lobby, salon): (
        Option<Res<MainMenu>>,
        Option<Res<OptionsMenu>>,
        Option<Res<RaceMenu>>,
        Option<Res<SelectionScreen>>,
        Option<Res<LobbyWindow>>,
        Option<Res<SalonMenu>>,
    ),
    (mut main_actions, mut option_actions, mut solo_actions, mut cup_actions): (
        MessageReader<MainMenuAction>,
        MessageReader<OptionsAction>,
        MessageReader<RaceMenuAction>,
        MessageReader<GrandPrixAction>,
    ),
    (mut pilot_actions, mut salon_actions): (
        MessageReader<SelectionAction>,
        MessageReader<SalonAction>,
    ),
    mut memory: Local<ScreenMemory>,
) {
    let now = time.elapsed_secs_f64();
    let (mut change, mut confirm, mut back) = (false, false, false);
    for action in main_actions.read() {
        match action {
            MainMenuAction::Release(_) | MainMenuAction::Open => confirm = true,
            MainMenuAction::Close => back = true,
            _ => {}
        }
    }
    for action in option_actions.read() {
        match action {
            OptionsAction::Change(..) | OptionsAction::Keyboard(_) | OptionsAction::NameTags(_) => {
                change = true
            }
            OptionsAction::Close => back = true,
            _ => {}
        }
    }
    for action in solo_actions.read() {
        match action {
            RaceMenuAction::Change(..) => change = true,
            RaceMenuAction::Confirm => confirm = true,
            RaceMenuAction::Close => back = true,
            _ => {}
        }
    }
    confirm |= cup_actions.read().count() > 0;
    for action in pilot_actions.read() {
        match action {
            SelectionAction::Change(..)
            | SelectionAction::Pick(..)
            | SelectionAction::Color(_)
            | SelectionAction::PickColor(_) => change = true,
            SelectionAction::Confirm => confirm = true,
            SelectionAction::Cancel => back = true,
            _ => {}
        }
    }
    for action in salon_actions.read() {
        match action {
            SalonAction::Change(_) | SalonAction::Step(..) => change = true,
            SalonAction::Choose | SalonAction::Press(_) => confirm = true,
            _ => {}
        }
    }
    // The focus of each screen, while it stays open.
    let screens = [
        main.as_ref().map(|m| (m.open, m.focus)),
        options.as_ref().map(|o| (o.open, o.row)),
        solo.as_ref().map(|s| (s.open, s.row)),
        pilot.as_ref().map(|p| (p.open, p.row)),
        lobby
            .as_ref()
            .map(|l| (l.open, salon.as_ref().map_or(0, |s| s.focus))),
    ];
    let mut hover = false;
    for (state, last) in screens.iter().zip(memory.screens.iter_mut()) {
        let state = state.unwrap_or_default();
        if state.0 && last.0 && state.1 != last.1 {
            hover = true;
        }
        // A screen that closes: back (unless a choice closed it).
        back |= last.0 && !state.0;
        *last = state;
    }
    if confirm {
        memory.confirmed_at = now;
    }
    let sound = if confirm {
        Some(SoundEffect::UiConfirm)
    } else if back
        && now - memory.confirmed_at > 0.4
        && memory.backed_at.is_none_or(|at| now - at > SAME_BACK)
    {
        memory.backed_at = Some(now);
        Some(SoundEffect::UiBack)
    } else if change {
        Some(SoundEffect::UiChange)
    } else if hover {
        Some(SoundEffect::UiHover)
    } else {
        None
    };
    if let Some(sound) = sound {
        play(&mut commands, sound, Cue::front(None));
    }
}

/// Each frame: the characters on foot of the lobby (issue #101): their steps on the lawn, their
/// jumps and landings, their paw, their tumbles (a « pof » and a cry).
fn watch_walkers(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<SoundTuning>,
    roster: Option<Res<Roster>>,
    mut walkers: Query<(
        Entity,
        &Walker,
        &WalkerLook,
        &Transform,
        Option<&mut WalkerSounds>,
        Has<PlayerWalker>,
    )>,
    mut cries: Local<Vec<(Entity, f32)>>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (entity, walker, look, transform, memory, player) in &mut walkers {
        let Some(mut memory) = memory else {
            commands.entity(entity).insert(WalkerSounds {
                last: *look,
                ..default()
            });
            continue;
        };
        let cue = if player {
            Cue::front(None)
        } else {
            Cue::at(None, transform.translation)
        };
        let last = std::mem::replace(&mut memory.last, *look);
        let upright = look.phase == crate::characters::RagdollPhase::Upright;
        if upright && !look.airborne && look.speed > 0.5 {
            memory.stride += look.speed * dt;
            let stride = 0.62 * walker.size.max(0.5);
            if memory.stride >= stride {
                memory.stride -= stride;
                memory.steps = memory.steps.wrapping_add(1);
                play(&mut commands, SoundEffect::Footstep(memory.steps % 3), cue);
            }
        } else {
            memory.stride = 0.0;
        }
        if look.airborne && !last.airborne && look.velocity.y > 1.0 && upright {
            play(&mut commands, SoundEffect::FootJump, cue);
        }
        if !look.airborne && last.airborne && upright {
            play(&mut commands, SoundEffect::Landing, cue);
        }
        if look.swing.is_some() && last.swing.is_none() {
            play(&mut commands, SoundEffect::PawSwing, cue);
        }
        if look.knocks > last.knocks {
            play(&mut commands, SoundEffect::PawPof, cue);
            let recent = cries
                .iter()
                .any(|(e, at)| *e == entity && now - at < tuning.cry_interval);
            if !recent {
                cries.retain(|(e, at)| *e != entity && now - at < tuning.cry_interval);
                cries.push((entity, now));
                let model = roster
                    .as_ref()
                    .and_then(|r| r.characters.get(walker.character))
                    .and_then(|c| c.model.as_deref());
                play(
                    &mut commands,
                    SoundEffect::Cry(Animal::of_model(model), Mood::Hurt),
                    cue,
                );
            }
        }
    }
}

/// The volume a sound is played at: the options' general and effects volumes, its family's in
/// the admin panel, its own mix, its cue's gain.
pub fn sound_level(
    tuning: &SoundTuning,
    options: &PlayerOptions,
    sound: SoundEffect,
    gain: f32,
) -> f32 {
    option_gain(options.master_volume)
        * option_gain(options.effects_volume)
        * tuning.family_volume(sound.family())
        * base_volume(sound)
        * gain.clamp(0.0, 1.0)
}

/// Where the camera listens (the `SpatialListener`), or the world's origin before there is one.
fn listener_position(listener: &Query<&GlobalTransform, With<SpatialListener>>) -> Vec3 {
    listener
        .iter()
        .next()
        .map_or(Vec3::ZERO, |t| t.translation())
}

/// Each frame: each new sound gets a voice (or not: the cap, the range), its clip and its
/// volume; the voices whose sound ended are let go (see the module's documentation).
#[allow(clippy::too_many_arguments)]
pub fn mix(
    mut commands: Commands,
    time: Res<Time<Real>>,
    tuning: Res<SoundTuning>,
    options: Res<PlayerOptions>,
    choices: Res<SoundChoices>,
    (mut bank, server, sources): (
        Option<ResMut<SoundBank>>,
        Option<Res<AssetServer>>,
        Option<Res<Assets<AudioSource>>>,
    ),
    mut stats: ResMut<MixerStats>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut fresh: Query<
        (
            Entity,
            &SoundEffect,
            Option<&Cue>,
            Option<&mut PlaybackSettings>,
        ),
        Without<Voice>,
    >,
    voices: Query<(Entity, &Voice)>,
    mut scratch: Local<(
        Vec<Entity>,
        Vec<(f32, f64)>,
        Vec<(SoundEffect, Option<Entity>, f64)>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let (playing, slots, recent) = &mut *scratch;
    playing.clear();
    slots.clear();
    for (entity, voice) in &voices {
        if voice.ends_at <= now {
            commands.entity(entity).despawn();
        } else {
            playing.push(entity);
            slots.push((voice.priority, voice.started));
        }
    }
    recent.retain(|(_, _, at)| now - at < 0.08);
    let at = listener_position(&listener);
    let cap = tuning.max_sounds as usize;
    let scale = SpatialScale::new(1.0 / tuning.full_volume_distance.max(0.5));
    for (entity, sound, cue, playback) in &mut fresh {
        let cue = cue.copied().unwrap_or(Cue::front(None));
        let distance = if cue.foreground {
            0.0
        } else {
            cue.at.distance(at)
        };
        let twice = recent.iter().any(|(s, k, _)| s == sound && *k == cue.kart);
        // Issue #135: the candidate chosen for its slot.
        let pick = pick(*sound, &choices);
        let level = sound_level(&tuning, &options, *sound, cue.gain) * pick.gain;
        if twice || distance > tuning.range || level <= 1e-4 {
            stats.skipped += 1;
            commands.entity(entity).despawn();
            continue;
        }
        let priority = priority(cue.foreground, distance, tuning.range);
        match admit(slots, cap, priority) {
            Admission::Free => {}
            Admission::Replace(index) => {
                stats.cut += 1;
                commands.entity(playing[index]).despawn();
                playing.swap_remove(index);
                slots.swap_remove(index);
            }
            Admission::Refused => {
                stats.refused += 1;
                commands.entity(entity).despawn();
                continue;
            }
        }
        stats.played += 1;
        recent.push((*sound, cue.kart, now));
        let speed = (cue.speed * pick.speed).clamp(0.25, 4.0);
        // A file chosen just now may still be loading: its voice waits for it a little.
        let loading = match (pick.clip, &bank, &sources) {
            (Picked::File(path), Some(bank), Some(sources)) => bank
                .files
                .get(path)
                .is_none_or(|handle| !sources.contains(handle)),
            _ => false,
        };
        let voice = Voice {
            started: now,
            ends_at: now
                + f64::from(pick.seconds / speed)
                + 0.05
                + if loading { LOADING_WAIT } else { 0.0 },
            priority,
        };
        playing.push(entity);
        slots.push((voice.priority, voice.started));
        let mut sound_entity = commands.entity(entity);
        sound_entity.insert(voice);
        let settings = PlaybackSettings {
            mode: PlaybackMode::Once,
            volume: Volume::Linear(level),
            speed,
            spatial: !cue.foreground,
            spatial_scale: Some(scale),
            ..PlaybackSettings::ONCE
        };
        match (pick.clip, bank.as_deref_mut()) {
            (Picked::Blip, _) => {
                // The croquettes' blips come with their player: only their volume.
                if let Some(mut playback) = playback {
                    let own = playback.volume.to_linear();
                    playback.volume = Volume::Linear(own * level / base_volume(*sound));
                }
            }
            (Picked::File(path), Some(bank)) => {
                // A candidate's file is loaded the first time it plays (if it was not yet).
                let handle = match (bank.files.get(path), &server) {
                    (Some(handle), _) => Some(handle.clone()),
                    (None, Some(server)) => {
                        let handle: Handle<AudioSource> = server.load(path);
                        bank.files.insert(path, handle.clone());
                        Some(handle)
                    }
                    (None, None) => None,
                };
                if let Some(handle) = handle {
                    sound_entity.insert((AudioPlayer::<AudioSource>(handle), settings));
                }
            }
            (Picked::Original(original), Some(bank)) => {
                if let Some(handle) = bank.synths.get(&original) {
                    sound_entity.insert((AudioPlayer::<SynthSound>(handle.clone()), settings));
                }
            }
            (Picked::Synth(slot, index), Some(bank)) => {
                if let Some(handle) = bank.alternatives.get(&(slot, index)) {
                    sound_entity.insert((AudioPlayer::<SynthSound>(handle.clone()), settings));
                }
            }
            _ => {}
        }
    }
    stats.most_voices = stats.most_voices.max(playing.len() as u32);
}

/// A file still loading when its sound is asked for: its voice waits this long more (s).
const LOADING_WAIT: f64 = 1.5;

/// What the loops need of a kart.
type LoopKart = (
    Entity,
    &'static Transform,
    &'static KartState,
    &'static Boost,
    &'static Stall,
    &'static KartWheels,
    &'static KartEnvironment,
    Option<&'static Velocity>,
    Option<&'static Remote>,
    // Issue #176: what its driver presses on the grid (the race holds its input at zero before
    // GO; with a renderer, the particles saw it before: the engine revs there).
    (Option<&'static KartInput>, Option<&'static KartVfx>),
    Option<&'static MaxSpeedScale>,
    Has<PlayerKart>,
);

/// A kart, as the loops hear it.
#[derive(Clone, Copy, Debug)]
struct Heard {
    kart: Entity,
    at: Vec3,
    player: bool,
    revs: Revs,
    tier: Option<f32>,
    gliding: bool,
    offroad: f32,
}

/// The loops' scratch buffers, kept between frames (nothing allocated once they have room).
#[derive(Default)]
struct LoopScratch {
    heard: Vec<Heard>,
    positions: Vec<(Vec3, bool)>,
    picked: Vec<usize>,
    wanted: Vec<Entity>,
    drifting: Vec<usize>,
}

/// The voices of a loop wanted: the player's, and `others` of the others'.
fn wanted_voices(tuning: &SoundTuning, kind: LoopKind) -> u32 {
    match kind {
        LoopKind::Engine => tuning.max_engines,
        LoopKind::Squeal => tuning.max_squeals,
        LoopKind::Wind | LoopKind::Rumble => 0,
    }
}

fn spawn_loop_voice(commands: &mut Commands, kind: LoopKind, foreground: bool, slot: u8) {
    let voice = LoopVoice {
        kind,
        foreground,
        slot,
        kart: None,
        pitch: 1.0,
        volume: 0.0,
        source: None,
    };
    commands.spawn((voice, Transform::default()));
}

/// A loop's sound for `foreground` or not (paused, silent: [`drive_loops`] sets it each frame).
fn loop_player(
    handle: Handle<LoopSound>,
    foreground: bool,
    tuning: &SoundTuning,
) -> (AudioPlayer<LoopSound>, PlaybackSettings) {
    (
        AudioPlayer::<LoopSound>(handle),
        PlaybackSettings {
            mode: PlaybackMode::Once,
            volume: Volume::Linear(0.0),
            spatial: !foreground,
            spatial_scale: Some(SpatialScale::new(
                1.0 / tuning.full_volume_distance.max(0.5),
            )),
            paused: true,
            ..PlaybackSettings::ONCE
        },
    )
}

/// What a loop plays for its kart: its pitch and volume (0 to 1, before the mix).
fn loop_note(tuning: &SoundTuning, kind: LoopKind, heard: &Heard) -> (f32, f32) {
    let ratio = (heard.revs.speed.abs() / heard.revs.top_speed.max(1.0)).clamp(0.0, 1.3);
    match kind {
        LoopKind::Engine => engine_note(tuning, &heard.revs),
        // The squeal climbs with the tiers (a little higher at each one).
        LoopKind::Squeal => match heard.tier {
            Some(tier) => (0.92 + 0.08 * tier, 0.45 + 0.35 * ratio.min(1.0)),
            None => (1.0, 0.0),
        },
        LoopKind::Wind => {
            if heard.gliding {
                (0.8 + 0.4 * ratio, 0.8)
            } else {
                (1.0, 0.0)
            }
        }
        LoopKind::Rumble => (
            0.8 + 0.4 * ratio,
            0.7 * heard.offroad * (heard.revs.speed.abs() / 8.0).min(1.0),
        ),
    }
}

/// The volume of a loop's family in the mix.
fn loop_family(tuning: &SoundTuning, kind: LoopKind) -> f32 {
    match kind {
        LoopKind::Engine => tuning.engine_volume,
        LoopKind::Squeal => tuning.drive_volume,
        LoopKind::Wind | LoopKind::Rumble => tuning.world_volume,
    }
    .clamp(0.0, 2.0)
}

/// Each frame: the loops follow their karts (see the module's documentation).
#[allow(clippy::too_many_arguments)]
fn drive_loops(
    mut commands: Commands,
    time: Res<Time<Real>>,
    (tuning, kart_tuning, options): (Res<SoundTuning>, Res<KartTuning>, Res<PlayerOptions>),
    (choices, stage, output): (Res<SoundChoices>, Res<RaceSounds>, Res<SoundOutput>),
    bank: Option<Res<SoundBank>>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    karts: Query<LoopKart, (Or<(With<Kart>, With<Remote>)>, Without<LoopVoice>)>,
    mut voices: Query<(
        Entity,
        &mut LoopVoice,
        &mut Transform,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
    mut scratch: Local<LoopScratch>,
) {
    let dt = time.delta_secs();
    let scratch = &mut *scratch;
    let listener = listener_position(&listener);
    // The karts heard.
    scratch.heard.clear();
    for (
        kart,
        transform,
        state,
        boost,
        stall,
        wheels,
        environment,
        velocity,
        remote,
        input,
        scale,
        player,
    ) in &karts
    {
        let velocity = velocity
            .map(|v| v.linear)
            .or(remote.map(|r| r.now_velocity))
            .unwrap_or(Vec3::ZERO);
        let speed = velocity.dot(*transform.forward());
        let (input, pressed) = input;
        let throttle = match input {
            Some(_) if pressed.is_some_and(|p| p.held) => pressed.map_or(0.0, |p| p.throttle),
            Some(input) => input.throttle,
            None if speed.abs() > 1.0 => 0.7,
            None => 0.0,
        };
        let tier = match *state {
            KartState::Drifting { charge, .. } => Some(
                kart_tuning
                    .mini_turbo_thresholds
                    .iter()
                    .filter(|threshold| charge >= **threshold)
                    .count() as f32,
            ),
            _ => None,
        };
        scratch.heard.push(Heard {
            kart,
            at: transform.translation,
            player,
            revs: Revs {
                speed,
                top_speed: kart_tuning.max_speed * scale.map_or(1.0, |s| s.0),
                throttle,
                boosting: boost.is_active(),
                airborne: wheels.ticks_without_ground > 6,
                stalled: stall.is_active(),
                underwater: environment.underwater,
            },
            tier,
            gliding: environment.gliding,
            offroad: if wheels.wheels_on_ground() > 0 {
                wheels.offroad_ratio
            } else {
                0.0
            },
        });
    }
    // The voices there: the player's of each loop, and the others' (more if the tuning grew).
    for kind in [
        LoopKind::Engine,
        LoopKind::Squeal,
        LoopKind::Wind,
        LoopKind::Rumble,
    ] {
        let wanted = wanted_voices(&tuning, kind);
        let mut front = false;
        let mut others = 0u32;
        for (_, voice, ..) in &voices {
            if voice.kind == kind {
                if voice.foreground {
                    front = true;
                } else {
                    others = others.max(u32::from(voice.slot) + 1);
                }
            }
        }
        if !front {
            spawn_loop_voice(&mut commands, kind, true, 0);
        }
        for slot in others..wanted.min(u32::from(u8::MAX)) {
            spawn_loop_voice(&mut commands, kind, false, slot as u8);
        }
    }
    let player = scratch.heard.iter().position(|h| h.player);
    // Whether the player's engine and squeal are heard (its squeal: while it drifts).
    let mut player_heard = [false; 2];
    // Who each loop hears: the player first, then the closest (`mixer::nearest`).
    for kind in [LoopKind::Engine, LoopKind::Squeal] {
        scratch.positions.clear();
        scratch.drifting.clear();
        for (index, heard) in scratch.heard.iter().enumerate() {
            if kind == LoopKind::Engine || heard.tier.is_some() {
                scratch.drifting.push(index);
                scratch.positions.push((heard.at, heard.player));
            }
        }
        let max = wanted_voices(&tuning, kind) as usize;
        nearest(
            listener,
            &scratch.positions,
            tuning.range,
            max,
            &mut scratch.picked,
        );
        scratch.wanted.clear();
        for picked in &scratch.picked {
            let heard = scratch.heard[scratch.drifting[*picked]];
            if !heard.player {
                scratch.wanted.push(heard.kart);
            }
        }
        // The others' voices keep their kart while it is heard, the others take the rest.
        let front = scratch
            .picked
            .first()
            .is_some_and(|i| scratch.heard[scratch.drifting[*i]].player);
        player_heard[usize::from(kind == LoopKind::Squeal)] = front;
        let spatial = max.saturating_sub(usize::from(front));
        for (_, mut voice, ..) in &mut voices {
            if voice.kind != kind || voice.foreground {
                continue;
            }
            match voice.kart {
                Some(kart) if usize::from(voice.slot) < spatial => {
                    if let Some(i) = scratch.wanted.iter().position(|k| *k == kart) {
                        scratch.wanted.swap_remove(i);
                    } else {
                        voice.kart = None;
                    }
                }
                _ => voice.kart = None,
            }
        }
        for (_, mut voice, ..) in &mut voices {
            if voice.kind == kind
                && !voice.foreground
                && voice.kart.is_none()
                && usize::from(voice.slot) < spatial
                && let Some(kart) = scratch.wanted.pop()
            {
                voice.kart = Some(kart);
                // A new kart: it comes in from silence.
                voice.volume = 0.0;
            }
        }
    }
    let master = option_gain(options.master_volume) * option_gain(options.effects_volume);
    for (entity, mut voice, mut transform, sink, spatial_sink) in &mut voices {
        if voice.foreground {
            // The player's loops: its engine and its squeal when they are heard (within the
            // cap; the squeal while it drifts), its wind and its rumble.
            let heard = match voice.kind {
                LoopKind::Engine => player_heard[0],
                LoopKind::Squeal => player_heard[1],
                LoopKind::Wind | LoopKind::Rumble => true,
            };
            voice.kart = player.filter(|_| heard).map(|i| scratch.heard[i].kart);
        }
        let heard = voice
            .kart
            .and_then(|kart| scratch.heard.iter().find(|h| h.kart == kart));
        let (pitch, volume) = match heard {
            Some(heard) => {
                let (pitch, volume) = loop_note(&tuning, voice.kind, heard);
                transform.translation = heard.at;
                (pitch, volume)
            }
            None => (voice.pitch, 0.0),
        };
        // Issue #135: the candidate chosen for its slot; another one: a quick fade out, then the
        // new sound (from silence).
        let slot = Slot::of_loop(voice.kind);
        let wanted = choices.index(slot) as u8;
        let candidate = choices.candidate(slot);
        let switching = voice.source != Some(wanted);
        let mut target = volume * loop_family(&tuning, voice.kind) * master * candidate.gain;
        // Issue #135: only in a race (not on the home screen, in the lobby, the pause, after the
        // player's finish): a short fade out.
        if !stage.loops || switching {
            target = 0.0;
        }
        let rate = if switching {
            tuning.switch_response
        } else {
            tuning.engine_response
        }
        .max(0.1);
        voice.pitch = approach(voice.pitch, pitch, tuning.engine_response.max(0.1), dt);
        voice.volume = approach(voice.volume, target, rate, dt);
        if voice.volume < 1e-3 && target == 0.0 {
            voice.volume = 0.0;
        }
        if switching && voice.volume == 0.0 {
            // Silent now: the new sound (with an output, once it is ready).
            match (output.0, bank.as_deref()) {
                (false, _) => voice.source = Some(wanted),
                (true, Some(bank)) => {
                    if let Some(handle) = bank.loops.get(&(slot, wanted)) {
                        commands
                            .entity(entity)
                            .remove::<(AudioSink, SpatialAudioSink)>()
                            .insert(loop_player(handle.clone(), voice.foreground, &tuning));
                        voice.source = Some(wanted);
                    }
                }
                (true, None) => {}
            }
            continue;
        }
        let (pitch, volume) = ((voice.pitch * candidate.speed).max(0.1), voice.volume);
        if let Some(mut sink) = sink {
            apply(&mut *sink, pitch, volume);
        }
        if let Some(mut sink) = spatial_sink {
            apply(&mut *sink, pitch, volume);
        }
    }
}

/// Issue #135: how long « ▶ Écouter » plays a loop (s): the engine revs up, then down...
const ENGINE_PREVIEW: f32 = 2.2;
/// ...the squeal climbs its three tiers, the wind and the rumble play.
const LOOP_PREVIEW: f32 = 1.6;
/// A loop listened to comes in and goes out over this long (s).
const PREVIEW_FADE: f32 = 0.12;

/// The pitch and volume (0 to 1, before the mix) of a loop listened to, `t` seconds in (pure).
pub fn preview_note(tuning: &SoundTuning, kind: LoopKind, t: f32) -> (f32, f32) {
    let length = if kind == LoopKind::Engine {
        ENGINE_PREVIEW
    } else {
        LOOP_PREVIEW
    };
    let envelope =
        (t / PREVIEW_FADE).clamp(0.0, 1.0) * ((length - t) / PREVIEW_FADE).clamp(0.0, 1.0);
    let (pitch, volume) = match kind {
        LoopKind::Engine => {
            // Up for 1.4 s, accelerator down; then let go.
            let up = 1.4;
            let (ratio, throttle) = if t < up {
                ((t / up).powf(1.3), 1.0)
            } else {
                ((1.0 - (t - up) / (length - up)).max(0.0) * 0.8 + 0.1, 0.0)
            };
            engine_note(
                tuning,
                &Revs {
                    speed: 26.0 * ratio,
                    top_speed: 26.0,
                    throttle,
                    ..default()
                },
            )
        }
        LoopKind::Squeal => {
            let tier = (t / length * 3.0).floor().min(3.0);
            (0.92 + 0.08 * tier, 0.8)
        }
        LoopKind::Wind => (1.0, 0.8),
        LoopKind::Rumble => (1.0, 0.7),
    };
    (pitch, volume * envelope)
}

/// Issue #135: « ▶ Écouter »: a one-shot's sounds, in their time (`Slot::preview`), in front; a
/// loop's short sample (a [`LoopPreview`], [`drive_previews`]). Even out of a race, even paused.
fn play_previews(
    mut commands: Commands,
    time: Res<Time<Real>>,
    choices: Res<SoundChoices>,
    mut asked: MessageReader<PreviewSound>,
    previews: Query<(Entity, &LoopPreview)>,
    mut queue: Local<Vec<(f64, SoundEffect)>>,
) {
    let now = time.elapsed_secs_f64();
    for PreviewSound(slot) in asked.read() {
        if let Some(kind) = slot.looping() {
            for (entity, preview) in &previews {
                if preview.kind == kind {
                    commands.entity(entity).despawn();
                }
            }
            commands.spawn((
                LoopPreview {
                    kind,
                    source: choices.index(*slot) as u8,
                    started: None,
                    pitch: 1.0,
                    volume: 0.0,
                },
                Transform::default(),
            ));
            continue;
        }
        let mut at = now;
        for (after, sound) in slot.preview() {
            at += f64::from(*after);
            queue.push((at, *sound));
        }
    }
    queue.retain(|(at, sound)| {
        if *at > now {
            return true;
        }
        commands.spawn((*sound, Cue::front(None), Preview));
        false
    });
}

/// Issue #135: the loops listened to: their sound once it is ready, their pitch and volume
/// ([`preview_note`]), let go at their end.
fn drive_previews(
    mut commands: Commands,
    time: Res<Time<Real>>,
    (tuning, options, output): (Res<SoundTuning>, Res<PlayerOptions>, Res<SoundOutput>),
    bank: Option<Res<SoundBank>>,
    mut previews: Query<(
        Entity,
        &mut LoopPreview,
        Has<AudioPlayer<LoopSound>>,
        Option<&mut AudioSink>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let master = option_gain(options.master_volume) * option_gain(options.effects_volume);
    for (entity, mut preview, playing, sink) in &mut previews {
        let slot = Slot::of_loop(preview.kind);
        let candidate = &slot.candidates()[usize::from(preview.source)];
        let Some(started) = preview.started else {
            // Its sound: there at once without an output; with one, once it is ready.
            if !output.0 {
                preview.started = Some(now);
            } else if !playing
                && let Some(handle) = bank
                    .as_deref()
                    .and_then(|bank| bank.loops.get(&(slot, preview.source)))
            {
                commands
                    .entity(entity)
                    .insert(loop_player(handle.clone(), true, &tuning));
                preview.started = Some(now);
            }
            continue;
        };
        let t = (now - started) as f32;
        let length = if preview.kind == LoopKind::Engine {
            ENGINE_PREVIEW
        } else {
            LOOP_PREVIEW
        };
        if t > length {
            commands.entity(entity).despawn();
            continue;
        }
        let (pitch, volume) = preview_note(&tuning, preview.kind, t);
        preview.pitch = pitch;
        preview.volume = volume * loop_family(&tuning, preview.kind) * master * candidate.gain;
        if let Some(mut sink) = sink {
            apply(
                &mut *sink,
                (pitch * candidate.speed).max(0.1),
                preview.volume,
            );
        }
    }
}

/// A loop's sink plays `pitch` at `volume`; paused while silent (it costs nothing then).
fn apply(sink: &mut impl AudioSinkPlayback, pitch: f32, volume: f32) {
    if volume <= 0.0 {
        if !sink.is_paused() {
            sink.pause();
        }
        return;
    }
    if (sink.speed() - pitch).abs() > 1e-3 {
        sink.set_speed(pitch);
    }
    sink.set_volume(Volume::Linear(volume));
    if sink.is_paused() {
        sink.play();
    }
}
