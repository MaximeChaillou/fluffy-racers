//! Issue #104: the music, headless (no audio output: the music heard is its entity, a
//! [`MusicVoice`], never played): the whole game (`GamePlugin`) with `MinimalPlugins`, one fixed
//! tick per update (and 1/60 s of real time), [`MusicPlugin`] added by hand (the game adds it
//! only with a renderer). Each screen chooses its music, the race its circuit's from GO, the
//! pause dips it, the volumes of the options reach it; every circuit has its piece, there and
//! credited. The last lap and the finish: `scenario_tests::the_last_lap_and_the_finish_have_their_jingles`.

use std::collections::BTreeSet;

use bevy::audio::{Decodable, Source};
use bevy::prelude::*;

use super::*;
use crate::admin::skip_countdown_now;
use crate::admin::tracks::circuits;
use crate::level::LoadLevel;
use crate::options::PlayerOptions;
use crate::race::{CupRace, GrandPrix, GrandPrixStage, Race, RaceMode};
use crate::tests::{game_app_unloaded, wait_for_the_player};
use crate::track::{TrackChoice, TrackData};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

fn catalog() -> &'static Catalog {
    music_catalog().expect("music.ron")
}

/// The game on `track` with `bots` bots and the music, the player's kart spawned.
fn music_app(track: TrackChoice, bots: usize) -> App {
    let mut app = game_app_unloaded(track, bots);
    app.add_plugins(MusicPlugin);
    wait_for_the_player(&mut app);
    app
}

/// Prairie Pompon, alone, its race created (its countdown on).
fn race_app() -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    // The menu opened from the game (its pause), not at launch: the race starts at once.
    app.add_plugins(crate::menu::MainMenuPlugin { at_launch: false })
        .add_plugins(MusicPlugin);
    wait_for_the_player(&mut app);
    for _ in 0..600 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    app.update();
    app
}

/// Taps a key as the keyboard does (its press, then its release).
fn tap(app: &mut App, key_code: KeyCode) {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// The music heard (its entity), if any: never more than one.
fn voice(app: &mut App) -> Option<MusicVoice> {
    let world = app.world_mut();
    let voices: Vec<MusicVoice> = world.query::<&MusicVoice>().iter(world).copied().collect();
    assert!(voices.len() <= 1, "one music at a time: {voices:?}");
    voices.first().copied()
}

/// What is heard now: the voice's intro for its length, then its piece.
fn sounding(app: &App) -> Option<usize> {
    let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
    let heard = app.world().resource::<Jukebox>().deck.heard?;
    Some(heard.sounding(now, |p| catalog().seconds(p)))
}

fn heard(app: &mut App) -> Option<&'static str> {
    voice(app).map(|v| catalog().pieces[v.piece].id.as_str())
}

/// Runs until `piece` is heard (at most `frames` frames): its voice.
fn until_heard(app: &mut App, piece: usize, frames: usize) -> MusicVoice {
    for _ in 0..frames {
        app.update();
        if let Some(voice) = voice(app).filter(|v| v.piece == piece) {
            return voice;
        }
    }
    panic!(
        "{} never heard, {:?} instead",
        catalog().pieces[piece].id,
        heard(app)
    );
}

/// Issue #104: nothing of the music in the headless game (the bot races' world): no resource, no
/// entity; its tuning is in the admin panel with it, and without an audio output it loads no
/// file.
#[test]
fn nothing_of_the_music_in_the_headless_game() {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    wait_for_the_player(&mut app);
    run(&mut app, 30);
    assert!(app.world().get_resource::<Jukebox>().is_none());
    assert!(app.world().get_resource::<MusicTuning>().is_none());
    assert!(voice(&mut app).is_none());
    let mut app = music_app(TrackChoice::default(), 0);
    run(&mut app, 3);
    let names: Vec<&str> = app
        .world()
        .resource::<crate::admin::Tunings>()
        .list
        .iter()
        .map(|t| t.name)
        .collect();
    assert!(names.contains(&"MusicTuning"), "{names:?}");
    assert_eq!(*app.world().resource::<MusicOutput>(), MusicOutput(false));
    assert!(app.world().resource::<Assets<MusicStream>>().is_empty());
}

/// Issue #104 acceptance: a race plays its circuit's music, from GO (the countdown beeps over
/// silence), at once and at its speed; the pause (solo) dips it, and it comes back up after.
#[test]
fn a_race_plays_its_circuit_from_go_and_the_pause_dips_it() {
    let mut app = race_app();
    let prairie = catalog().index("pure-raceway").unwrap();
    run(&mut app, 30);
    assert!(!app.world().resource::<Race>().started());
    assert_eq!(
        app.world().resource::<Jukebox>().scene,
        MusicScene::Circuit {
            piece: prairie,
            started: false,
            last_lap: false
        }
    );
    assert_eq!(heard(&mut app), None, "the countdown");
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    app.update();
    let racing = voice(&mut app).expect("GO");
    assert_eq!(racing.piece, prairie);
    assert!(!racing.jingle);
    assert_eq!(racing.speed, 1.0);
    let options = *app.world().resource::<PlayerOptions>();
    let tuning = app.world().resource::<MusicTuning>().clone();
    let full = option_gain(options.master_volume)
        * option_gain(options.music_volume)
        * tuning.volume
        * catalog().pieces[prairie].volume;
    assert!((racing.level - full).abs() < 1e-4, "at once: {racing:?}");
    run(&mut app, 120);
    assert_eq!(voice(&mut app).unwrap().number, racing.number, "it loops");

    // The pause (Échap, the menu opened from the game): the music dips to `pause_volume`, the
    // same piece going on.
    tap(&mut app, KeyCode::Escape);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    run(&mut app, 60);
    let paused = voice(&mut app).unwrap();
    assert_eq!(paused.number, racing.number);
    assert!(
        (paused.level - full * tuning.pause_volume).abs() < full * 0.02,
        "{} vs {}",
        paused.level,
        full * tuning.pause_volume
    );
    tap(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    run(&mut app, 60);
    let back = voice(&mut app).unwrap();
    assert_eq!(back.number, racing.number);
    assert!((back.level - full).abs() < full * 0.02);
}

/// Issue #104 acceptance: the screens choose their music: the home screen its own (with its
/// background race behind it), the lobby its own; from the lobby to a race, the lobby's goes out
/// (down, frame after frame) before the countdown, then the circuit's from GO.
#[test]
fn the_screens_choose_their_music() {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.add_plugins(crate::menu::MainMenuPlugin { at_launch: true })
        .add_plugins(MusicPlugin);
    wait_for_the_player(&mut app);
    run(&mut app, 3);
    assert!(app.world().resource::<crate::menu::MainMenu>().on_home());
    assert_eq!(app.world().resource::<Jukebox>().scene, MusicScene::Home);
    let home = until_heard(&mut app, catalog().home, 30);
    assert!(home.level < 0.2, "it comes in: {home:?}");
    run(&mut app, 60);
    assert!(voice(&mut app).unwrap().level > home.level);

    // The lobby (online, issue #101), from the test ground (silent).
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(crate::salon::SalonPlugin)
        .add_plugins(MusicPlugin);
    wait_for_the_player(&mut app);
    run(&mut app, 30);
    assert_eq!(app.world().resource::<Jukebox>().scene, MusicScene::Silent);
    assert_eq!(heard(&mut app), None);
    app.world_mut()
        .write_message(LoadLevel::on(TrackChoice::Salon, RaceMode::FreeRoam));
    until_heard(&mut app, catalog().salon, 120);
    assert_eq!(app.world().resource::<Jukebox>().scene, MusicScene::Salon);
    run(&mut app, 60);
    let salon = voice(&mut app).unwrap();

    // Then a race: the lobby's music goes out, the countdown is silent, GO plays the circuit's.
    app.world_mut()
        .write_message(LoadLevel::on(TrackChoice::default(), RaceMode::Race));
    let mut levels = Vec::new();
    for _ in 0..600 {
        app.update();
        levels.push(voice(&mut app));
        if app.world().get_resource::<Race>().is_some() && levels.last().unwrap().is_none() {
            break;
        }
    }
    let out: Vec<f32> = levels
        .iter()
        .map_while(|v| v.filter(|v| v.number == salon.number).map(|v| v.level))
        .collect();
    assert!(out.len() > 10, "{out:?}");
    assert!(
        out.windows(2).all(|w| w[1] <= w[0]),
        "it goes down: {out:?}"
    );
    assert!(*out.last().unwrap() < salon.level * 0.1, "{out:?}");
    assert_eq!(heard(&mut app), None);
    assert!(skip_countdown_now(app.world_mut()));
    let prairie = catalog().index("pure-raceway").unwrap();
    until_heard(&mut app, prairie, 3);
}

/// Issue #104 acceptance: the podium of a championship: its jingle, then its music (one voice:
/// the music starts on the jingle's last sample).
#[test]
fn the_podium_has_its_jingle_then_its_music() {
    let mut app = race_app();
    assert!(skip_countdown_now(app.world_mut()));
    run(&mut app, 3);
    let mut grand_prix = GrandPrix::new(
        vec![CupRace {
            name: "Prairie Pompon".into(),
            path: crate::track::DEFAULT_CIRCUIT.into(),
        }],
        0,
    );
    grand_prix.stage = GrandPrixStage::Podium;
    app.insert_resource(grand_prix);
    app.update();
    assert_eq!(app.world().resource::<Jukebox>().scene, MusicScene::Podium);
    let (jingle, music) = catalog().podium;
    let fade = app.world().resource::<MusicTuning>().fade_out;
    let podium = until_heard(&mut app, music, (fade * 60.0) as usize + 5);
    assert_eq!(podium.intro, Some(jingle));
    assert_eq!(sounding(&app), Some(jingle), "the jingle first");
    let length = (catalog().seconds(jingle) * 60.0) as usize;
    run(&mut app, length - 3);
    assert_eq!(sounding(&app), Some(jingle), "for its length");
    run(&mut app, 5);
    assert_eq!(sounding(&app), Some(music), "then its music");
    assert_eq!(
        voice(&mut app).unwrap().number,
        podium.number,
        "the same voice"
    );
}

/// Issue #104: the options' volumes (#100, #103): the general one and the music's reach the
/// music, the effects' does not; at 0 %, silent.
#[test]
fn the_music_follows_the_options_volumes() {
    let mut app = race_app();
    assert!(skip_countdown_now(app.world_mut()));
    run(&mut app, 3);
    let level = |app: &mut App| voice(app).unwrap().level;
    let full = level(&mut app);
    assert!(full > 0.05, "{full}");
    app.world_mut()
        .resource_mut::<PlayerOptions>()
        .effects_volume = 0;
    app.update();
    assert_eq!(
        level(&mut app),
        full,
        "the effects' volume is not the music's"
    );
    app.world_mut().resource_mut::<PlayerOptions>().music_volume = 40;
    app.update();
    assert!(level(&mut app) < full * 0.5);
    app.world_mut().resource_mut::<PlayerOptions>().music_volume = 0;
    app.update();
    assert_eq!(level(&mut app), 0.0);
    app.world_mut().resource_mut::<PlayerOptions>().music_volume = 80;
    app.world_mut()
        .resource_mut::<PlayerOptions>()
        .master_volume = 0;
    app.update();
    assert_eq!(level(&mut app), 0.0);
}

/// Issue #104 acceptance (CLAUDE.md, « Assets »): every circuit of the manifest has a piece of
/// its own (named in its file or by its biome), and a faster one for its last lap; every piece's
/// file is in `assets/music/`, with its source's CC0 licence and its line in `assets/CREDITS.md`
/// (its page on OpenGameArt); its length is the decoded one (what loops); no other file is
/// there.
#[test]
fn every_circuit_has_its_music_there_and_credited() {
    let catalog = catalog();
    let mut pieces = BTreeSet::new();
    let manifest = circuits();
    assert!(manifest.len() >= 6);
    for entry in &manifest {
        let text = std::fs::read_to_string(format!("{ASSETS}/{}", entry.asset_path())).unwrap();
        let data = TrackData::from_ron(&text).unwrap();
        let piece =
            circuit_music(catalog, &data).unwrap_or_else(|| panic!("{}: no music", entry.name));
        assert!(!catalog.pieces[piece].jingle, "{}", entry.name);
        assert!(
            pieces.insert(piece),
            "{}: a music of its own, not {}'s",
            entry.name,
            catalog.pieces[piece].id
        );
        let faster = catalog
            .faster(piece)
            .unwrap_or_else(|| panic!("{}: no faster version", entry.name));
        assert!(catalog.pieces[faster].bpm > catalog.pieces[piece].bpm);
        // Its biome has a music too (the next circuits of the biome).
        if let Some(biome) = data.biome.as_deref() {
            let biome = crate::track::biome::biome(biome).unwrap();
            let id = biome
                .music
                .as_deref()
                .unwrap_or_else(|| panic!("{}", biome.name));
            assert!(catalog.index(id).is_some(), "{id}");
        }
    }
    let credits = std::fs::read_to_string(format!("{ASSETS}/CREDITS.md")).unwrap();
    let mut wanted = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut mismatches = Vec::new();
    for piece in &catalog.pieces {
        let source = piece.file.split('/').nth(1).unwrap();
        let file = piece.file.rsplit('/').next().unwrap();
        sources.insert(source);
        wanted.insert(piece.file.clone());
        let bytes = std::fs::read(format!("{ASSETS}/{}", piece.file))
            .unwrap_or_else(|_| panic!("{}", piece.file));
        let audio = bevy::audio::AudioSource {
            bytes: bytes.into(),
        };
        let stream = MusicStream::new(&audio, None).unwrap_or_else(|| panic!("{}: OGG", piece.id));
        let decoder = stream.decoder();
        let (channels, rate) = (decoder.channels().get(), decoder.sample_rate().get());
        let frames = decoder.count() as f64 / f64::from(channels);
        let counted = (piece.seconds * f64::from(rate)).round();
        // What loops is all of it: to 1.5 ms (64 frames) at the end of a pass (the decoder may
        // stop that much early on a re-encoded file: the loop starts again at once), a few
        // samples more from the decoder at most (left out).
        if !(frames >= counted - 64.0 && frames < counted + 2048.0) {
            mismatches.push(format!(
                "{}: {frames} frames decoded, {counted} counted",
                piece.file
            ));
        }
        let line = credits
            .lines()
            .find(|line| line.contains(&format!("`music/{source}/")))
            .unwrap_or_else(|| panic!("no line for music/{source}/ in assets/CREDITS.md"));
        assert!(line.contains(file), "{file} not in: {line}");
        assert!(
            line.contains("CC0") && line.contains("https://opengameart.org/content/"),
            "{line}"
        );
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
    for source in &sources {
        let licence = std::fs::read_to_string(format!("{ASSETS}/music/{source}/License.txt"))
            .unwrap_or_else(|_| panic!("assets/music/{source}/License.txt"));
        assert!(licence.contains("CC0"), "{source}");
        wanted.insert(format!("music/{source}/License.txt"));
    }
    let mut present = BTreeSet::new();
    let mut folders = vec![format!("{ASSETS}/music")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path.to_string_lossy().into_owned());
            } else {
                let relative = path
                    .strip_prefix(ASSETS)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                present.insert(relative);
            }
        }
    }
    assert_eq!(present, wanted, "only the files the game plays");
}

/// Issue #104: a real piece loops without a gap: its stream decodes its length, then starts
/// again from its first sample (smoothed over the seam), and never ends.
#[test]
fn a_piece_loops_on_its_decoded_samples() {
    let catalog = catalog();
    let piece = &catalog.pieces[catalog.finish(Tier::Good).1];
    let bytes = std::fs::read(format!("{ASSETS}/{}", piece.file)).unwrap();
    let audio = bevy::audio::AudioSource {
        bytes: bytes.into(),
    };
    let once: Vec<f32> = audio.decoder().collect();
    let stream = MusicStream::new(&audio, Some(piece.seconds)).unwrap();
    let decoder = stream.decoder();
    assert_eq!(decoder.total_duration(), None);
    let channels = usize::from(decoder.channels().get());
    let pass = (piece.seconds * f64::from(decoder.sample_rate().get())).round() as usize * channels;
    let looped: Vec<f32> = decoder.take(pass * 2 + 10_000).collect();
    assert_eq!(looped.len(), pass * 2 + 10_000);
    // The first pass is the file's own, then the second starts over (after the seam).
    assert_eq!(&looped[..pass], &once[..pass]);
    let after = (SEAM_FRAMES as usize + 4) * channels;
    assert_eq!(
        &looped[pass + after..pass + after + 5000],
        &once[after..after + 5000]
    );
}
