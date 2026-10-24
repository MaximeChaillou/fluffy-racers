//! Issue #103: the sound design, headless (no audio output: the sounds are entities, a
//! [`SoundEffect`] each, never played): the whole game (`GamePlugin`) with `MinimalPlugins`, one
//! fixed tick per update, [`SoundDesignPlugin`] added by hand (the game adds it only with a
//! renderer). Each game event asks for its sound; the engine follows the revs; the cap holds
//! with 12 karts; the files are there and credited.

use std::collections::BTreeSet;

use bevy::audio::{Decodable, Source};
use bevy::prelude::*;

use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::Bot;
use crate::interactions::{KartGrabbed, StrikeAnswer, TetherEnd, TetherEnded};
use crate::items::{GiveItem, ItemHit, ItemKind, ItemObject, ItemStats, Motion, Prize};
use crate::kart::{HitReaction, ITEM_KEYS, KartBump, KartState, MiniTurbo};
use crate::options::{OPTIONS_KEY, PlayerOptions};
use crate::race::{Course, Race, RaceProgress};
use crate::tests::{game_app_unloaded, hold_keys, player, wait_for_the_player};
use crate::track::TrackChoice;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Every sound asked for (spawned), in order, with its cue: recorded before the mixer takes it.
#[derive(Resource, Default)]
struct Asked(Vec<(SoundEffect, Cue)>);

fn record(
    mut asked: ResMut<Asked>,
    sounds: Query<(&SoundEffect, Option<&Cue>), Added<SoundEffect>>,
) {
    for (sound, cue) in &sounds {
        asked
            .0
            .push((*sound, cue.copied().unwrap_or(Cue::front(None))));
    }
}

/// The game on `track` with `bots` bots and the sound design, the player's kart spawned.
fn sound_app(track: TrackChoice, bots: usize) -> App {
    let mut app = game_app_unloaded(track, bots);
    // With the reel of the roulette and the music (issue #104), as in the game (all of them only
    // with a renderer there).
    app.add_plugins((crate::ui::RoulettePlugin, SoundDesignPlugin, MusicPlugin))
        .init_resource::<Asked>()
        .add_systems(PostUpdate, record.before(mix));
    wait_for_the_player(&mut app);
    app
}

/// Prairie Pompon with `bots` bots and the sound design, the race created (its countdown on).
fn race_app(bots: usize) -> (App, Entity) {
    race_app_on(TrackChoice::default(), bots)
}

/// The same on `track`.
fn race_app_on(track: TrackChoice, bots: usize) -> (App, Entity) {
    let mut app = sound_app(track, bots);
    for _ in 0..600 {
        let world = app.world_mut();
        let ready = world.get_resource::<Race>().is_some()
            && world.query::<&Bot>().iter(world).count() == bots;
        if ready {
            break;
        }
        app.update();
    }
    app.update();
    let kart = player(&mut app);
    (app, kart)
}

/// The race at GO (the countdown skipped).
fn started(bots: usize) -> (App, Entity) {
    started_on(TrackChoice::default(), bots)
}

/// The same on `track`.
fn started_on(track: TrackChoice, bots: usize) -> (App, Entity) {
    let (mut app, kart) = race_app_on(track, bots);
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    assert!(app.world().resource::<Race>().started());
    app.world_mut().resource_mut::<Asked>().0.clear();
    (app, kart)
}

/// Taps a key as the keyboard does (its press, then its release: `just_pressed` works).
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

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
    }
}

/// How many times `sound` was asked for (since the last `forget`).
fn asked(app: &App, sound: SoundEffect) -> usize {
    app.world()
        .resource::<Asked>()
        .0
        .iter()
        .filter(|(s, _)| *s == sound)
        .count()
}

fn sounds(app: &App) -> Vec<SoundEffect> {
    app.world()
        .resource::<Asked>()
        .0
        .iter()
        .map(|(s, _)| *s)
        .collect()
}

fn forget(app: &mut App) {
    app.world_mut().resource_mut::<Asked>().0.clear();
}

fn cries(app: &App, mood: Mood) -> usize {
    sounds(app)
        .iter()
        .filter(|s| matches!(s, SoundEffect::Cry(_, m) if *m == mood))
        .count()
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

fn loops(app: &mut App) -> Vec<LoopVoice> {
    let world = app.world_mut();
    world.query::<&LoopVoice>().iter(world).copied().collect()
}

fn front_loop(app: &mut App, kind: LoopKind) -> LoopVoice {
    loops(app)
        .into_iter()
        .find(|v| v.kind == kind && v.foreground)
        .unwrap()
}

/// Issue #103: nothing of the sound design in the headless game (the bot races' world): no
/// tuning, no loop, no memory on the karts; its tuning is in the admin panel with it.
#[test]
fn nothing_of_the_sound_design_in_the_headless_game() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    wait_for_the_player(&mut app);
    run(&mut app, 30);
    assert!(app.world().get_resource::<SoundTuning>().is_none());
    assert!(app.world().get_resource::<MixerStats>().is_none());
    assert_eq!(count::<With<LoopVoice>>(&mut app), 0);
    assert_eq!(count::<With<KartSounds>>(&mut app), 0);
    let names = |app: &App| -> Vec<&'static str> {
        app.world()
            .resource::<crate::admin::Tunings>()
            .list
            .iter()
            .map(|t| t.name)
            .collect()
    };
    assert!(!names(&app).contains(&"SoundTuning"));
    let mut app = sound_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    assert!(names(&app).contains(&"SoundTuning"));
    assert_eq!(count::<With<KartSounds>>(&mut app), 1);
    // No audio output in the tests: no clip, only the entities.
    assert!(app.world().get_resource::<SoundBank>().is_none());
    assert_eq!(*app.world().resource::<SoundOutput>(), SoundOutput(false));
}

/// Issue #103 acceptance: the countdown beeps for 3, 2 and 1, then GO, in front.
#[test]
fn the_countdown_beeps_then_goes() {
    let (mut app, _) = race_app(0);
    forget(&mut app);
    let steps = app.world().resource::<Race>().timings.countdown_steps as usize;
    for _ in 0..1200 {
        app.update();
        if app.world().resource::<Race>().race_ticks() > 30 {
            break;
        }
    }
    assert_eq!(asked(&app, SoundEffect::CountdownBeep), steps);
    assert_eq!(asked(&app, SoundEffect::CountdownGo), 1);
    let asked = &app.world().resource::<Asked>().0;
    let go = asked
        .iter()
        .position(|(s, _)| *s == SoundEffect::CountdownGo)
        .unwrap();
    assert!(
        asked[..go]
            .iter()
            .filter(|(s, _)| *s == SoundEffect::CountdownBeep)
            .count()
            == steps,
        "the beeps before GO"
    );
    assert!(
        asked
            .iter()
            .all(|(_, cue)| cue.foreground || cue.kart.is_some())
    );
}

/// Issue #103 acceptance (GD § 4): the accelerator down before the « 2 »: the engine stalls; on
/// the « 2 »: the rocket start's boost.
#[test]
fn a_stalled_start_and_a_rocket_start() {
    for early in [true, false] {
        let (mut app, _) = race_app(0);
        forget(&mut app);
        if early {
            hold_keys(&mut app, &[KeyCode::ArrowUp]);
        }
        for _ in 0..1200 {
            let race = app.world().resource::<Race>();
            let number = race.flow.countdown_number(&race.timings);
            let ticks = race.race_ticks();
            if !early && number == Some(2) {
                hold_keys(&mut app, &[KeyCode::ArrowUp]);
            }
            if ticks > 10 {
                break;
            }
            app.update();
        }
        if early {
            assert_eq!(asked(&app, SoundEffect::Stall), 1, "{:?}", sounds(&app));
            assert_eq!(asked(&app, SoundEffect::RocketStart), 0);
        } else {
            assert_eq!(
                asked(&app, SoundEffect::RocketStart),
                1,
                "{:?}",
                sounds(&app)
            );
            assert_eq!(asked(&app, SoundEffect::Stall), 0);
        }
    }
}

/// Issue #103 acceptance (GD § 3): a drift chimes at each tier, higher each time, its squeal
/// plays while it lasts, and the release fires the « fwoosh » of the tier reached. The engine
/// revs up meanwhile.
#[test]
fn the_drift_chimes_at_each_tier_and_fires_its_mini_turbo() {
    let mut app = sound_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    run(&mut app, 10);
    let idle = front_loop(&mut app, LoopKind::Engine);
    assert_eq!(idle.kart, Some(kart));
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 90);
    let revving = front_loop(&mut app, LoopKind::Engine);
    assert!(revving.pitch > idle.pitch + 0.3, "{idle:?} {revving:?}");
    assert!(revving.volume > idle.volume);
    forget(&mut app);
    hold_keys(
        &mut app,
        &[KeyCode::ArrowUp, KeyCode::Space, KeyCode::ArrowLeft],
    );
    let mut squealed = false;
    for _ in 0..400 {
        app.update();
        squealed |= front_loop(&mut app, LoopKind::Squeal).volume > 0.05;
        if asked(&app, SoundEffect::DriftTier(MiniTurbo::Purple)) > 0 {
            break;
        }
    }
    assert!(squealed, "the squeal while it drifts");
    let tiers: Vec<SoundEffect> = sounds(&app)
        .into_iter()
        .filter(|s| matches!(s, SoundEffect::DriftTier(_)))
        .collect();
    assert_eq!(
        tiers,
        MiniTurbo::ALL.map(SoundEffect::DriftTier),
        "{:?}",
        sounds(&app)
    );
    hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::ArrowLeft]);
    run(&mut app, 3);
    assert_eq!(asked(&app, SoundEffect::MiniTurbo(MiniTurbo::Purple)), 1);
    assert!(
        app.world()
            .get::<crate::kart::Boost>(kart)
            .unwrap()
            .is_active()
    );
    // The player's own: in front.
    assert!(
        app.world()
            .resource::<Asked>()
            .0
            .iter()
            .all(|(_, cue)| cue.foreground)
    );
    // Not drifting any more: the squeal fades out.
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    assert!(front_loop(&mut app, LoopKind::Squeal).volume < 0.01);
}

/// Issue #103 acceptance: the engine's pitch follows the revs (`mixer::engine_note`): it is the
/// note of its kart, smoothed; a boost raises it.
#[test]
fn the_engine_pitch_follows_the_revs() {
    let mut app = sound_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    run(&mut app, 60);
    let tuning = app.world().resource::<SoundTuning>().clone();
    let rest = front_loop(&mut app, LoopKind::Engine);
    assert!(
        (rest.pitch - tuning.engine_idle_pitch).abs() < 0.05,
        "{rest:?}"
    );
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let mut last = rest.pitch;
    for _ in 0..4 {
        run(&mut app, 20);
        let now = front_loop(&mut app, LoopKind::Engine);
        assert!(now.pitch > last, "{now:?} after {last}");
        last = now.pitch;
    }
    // Steady on the ground (a second on its wheels, past the test ground's ramp): the note of
    // the kart's revs.
    let mut grounded = 0;
    for _ in 0..600 {
        app.update();
        let wheels = app.world().get::<crate::kart::KartWheels>(kart).unwrap();
        grounded = if wheels.ticks_without_ground == 0 {
            grounded + 1
        } else {
            0
        };
        if grounded >= 60 {
            break;
        }
    }
    assert_eq!(grounded, 60, "on its wheels");
    let world = app.world();
    let speed = world
        .get::<bevy_rapier3d::prelude::Velocity>(kart)
        .unwrap()
        .linear
        .dot(*world.get::<Transform>(kart).unwrap().forward());
    let top = world.resource::<crate::kart::KartTuning>().max_speed
        * world.get::<crate::kart::MaxSpeedScale>(kart).unwrap().0;
    let (pitch, _) = engine_note(
        &tuning,
        &Revs {
            speed,
            top_speed: top,
            throttle: 1.0,
            boosting: world.get::<crate::kart::Boost>(kart).unwrap().is_active(),
            ..default()
        },
    );
    let steady = front_loop(&mut app, LoopKind::Engine);
    assert!(
        (steady.pitch - pitch).abs() < 0.03,
        "{steady:?} {pitch} at {speed} m/s"
    );
    assert!(steady.pitch > 1.2, "{steady:?}");
    // A boost (the debug key B): higher.
    tap(&mut app, crate::kart::DEBUG_BOOST_KEY);
    run(&mut app, 20);
    assert!(front_loop(&mut app, LoopKind::Engine).pitch > steady.pitch + 0.1);
    // Alone: no other engine plays.
    assert!(
        loops(&mut app)
            .iter()
            .filter(|v| v.kind == LoopKind::Engine && !v.foreground)
            .all(|v| v.kart.is_none() && v.volume == 0.0)
    );
}

/// Presses the item button for a tick, then lets go (the other keys let go too).
fn use_item(app: &mut App) {
    hold_keys(app, &[ITEM_KEYS[2]]);
    app.update();
    hold_keys(app, &[]);
    app.update();
}

/// Issue #103 acceptance (GD § 8): a box bursts, its roulette ticks and stops; then each item
/// its sound: dropped, thrown, launched, what it does.
#[test]
fn a_box_bursts_and_each_item_sounds() {
    let (mut app, kart) = started(3);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..600 {
        app.update();
        if app.world().get::<ItemStats>(kart).unwrap().boxes > 0 {
            break;
        }
    }
    assert!(
        app.world().get::<ItemStats>(kart).unwrap().boxes > 0,
        "no box"
    );
    run(&mut app, 2);
    // The player's box in front (the bots break theirs too, from where they are).
    let boxes = |app: &App| -> Vec<Cue> {
        app.world()
            .resource::<Asked>()
            .0
            .iter()
            .filter(|(s, _)| *s == SoundEffect::BoxBreak)
            .map(|(_, cue)| *cue)
            .collect()
    };
    let broken = app.world().get::<ItemStats>(kart).unwrap().boxes as usize;
    assert_eq!(
        boxes(&app)
            .iter()
            .filter(|cue| cue.kart == Some(kart))
            .count(),
        broken,
        "{:?}",
        sounds(&app)
    );
    assert!(
        boxes(&app)
            .iter()
            .all(|cue| cue.foreground == (cue.kart == Some(kart))),
        "{kart}: {:?}",
        boxes(&app)
    );
    // It stops there (on the track: no pelican), and uses its items standing.
    hold_keys(&mut app, &[KeyCode::ArrowDown]);
    run(&mut app, 100);
    hold_keys(&mut app, &[]);
    assert!(
        asked(&app, SoundEffect::RouletteTick) > 5,
        "{:?}",
        sounds(&app)
    );
    assert_eq!(asked(&app, SoundEffect::RouletteStop), 1);
    let cues = &app.world().resource::<Asked>().0;
    assert!(
        cues.iter()
            .filter(|(s, _)| matches!(s, SoundEffect::RouletteTick | SoundEffect::RouletteStop))
            .all(|(_, cue)| cue.foreground),
        "the player's own: in front"
    );
    for (prize, wanted) in [
        (Prize::Pelure, SoundEffect::Splotch),
        (Prize::Pelote, SoundEffect::Throw),
        (Prize::Abeille, SoundEffect::BeeBuzz),
        (Prize::Piment, SoundEffect::Piment),
        (Prize::Bombe, SoundEffect::Throw),
        (Prize::Chantilly, SoundEffect::CreamSquirt),
        (Prize::KlaxonCanard, SoundEffect::Honk),
        (Prize::OreillerGeant, SoundEffect::PillowLaunch),
        (Prize::Boomerang, SoundEffect::BoomerangThrow),
        (Prize::Pinata, SoundEffect::PinataBurst),
        (Prize::PimentDore, SoundEffect::Piment),
        (Prize::Degonflage, SoundEffect::Deflate),
        (Prize::PieVoleuse, SoundEffect::Ghost),
        (Prize::SuperCalin, SoundEffect::SuperCalin),
        (Prize::Fusee, SoundEffect::Fusee),
    ] {
        // Not while hit (its own pelote may come back to it): the item would wait.
        for _ in 0..400 {
            if !app.world().get::<KartState>(kart).unwrap().is_hit() {
                break;
            }
            app.update();
        }
        forget(&mut app);
        app.world_mut().write_message(GiveItem(prize));
        run(&mut app, 3);
        use_item(&mut app);
        run(&mut app, 4);
        assert!(
            asked(&app, wanted) >= 1,
            "{prize:?}: {wanted:?} in {:?} ({:?})",
            sounds(&app),
            app.world().get::<KartState>(kart),
        );
        match prize {
            // The bombe explodes where it lands; the pillow falls on the leader.
            Prize::Bombe => {
                run(&mut app, 150);
                assert_eq!(asked(&app, SoundEffect::Explosion), 1, "{:?}", sounds(&app));
            }
            Prize::OreillerGeant => {
                // It flies to the leader, far ahead by now (issue #108: up to a whole lap of
                // Prairie Pompon redrawn, twice as long). Issue #154: a bot's klaxon-canard may
                // blow it away on the way (the race of bots met one once their bumps changed): a
                // real event of the game, heard (the honk), not the pillow's sound; the kart then
                // throws another one, three times at most.
                let pillow_out = |app: &mut App| {
                    let world = app.world_mut();
                    world
                        .query::<&ItemObject>()
                        .iter(world)
                        .any(|o| o.item == ItemKind::OreillerGeant)
                };
                for _ in 0..3 {
                    for _ in 0..3600 {
                        app.update();
                        if asked(&app, SoundEffect::PillowFall) > 0 || !pillow_out(&mut app) {
                            break;
                        }
                    }
                    if asked(&app, SoundEffect::PillowFall) > 0 {
                        break;
                    }
                    assert!(
                        asked(&app, SoundEffect::Honk) > 0,
                        "the pillow vanished, neither fallen nor honked away: {:?}",
                        sounds(&app)
                    );
                    forget(&mut app);
                    app.world_mut()
                        .write_message(GiveItem(Prize::OreillerGeant));
                    run(&mut app, 3);
                    use_item(&mut app);
                    run(&mut app, 4);
                    assert_eq!(asked(&app, SoundEffect::PillowLaunch), 1);
                }
                assert_eq!(
                    asked(&app, SoundEffect::PillowFall),
                    1,
                    "{:?}",
                    sounds(&app)
                );
            }
            // The dégonflage shrinks the others: each one its « pfff », from where it is.
            Prize::Degonflage => {
                let deflated: Vec<Cue> = app
                    .world()
                    .resource::<Asked>()
                    .0
                    .iter()
                    .filter(|(s, _)| *s == SoundEffect::Deflate)
                    .map(|(_, cue)| *cue)
                    .collect();
                let shrunk = {
                    let world = app.world_mut();
                    world
                        .query_filtered::<&crate::kart::ItemEffects, With<Bot>>()
                        .iter(world)
                        .filter(|effects| effects.deflated > 0.0)
                        .count()
                };
                assert!(shrunk > 0);
                assert_eq!(deflated.len(), shrunk, "{deflated:?}");
                assert!(
                    deflated
                        .iter()
                        .all(|cue| !cue.foreground && cue.kart != Some(kart))
                );
            }
            _ => run(&mut app, 30),
        }
    }
}

/// Issue #103 acceptance (GD § 8 and 14): hit by an item, a kart spins out (a light one) or
/// tumbles (a heavy one), and its animal cries; the pelican comes with its squawk.
#[test]
fn hits_cry_and_the_pelican_squawks() {
    let (mut app, kart) = started(1);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    for (item, wanted) in [
        (ItemKind::Pelure, SoundEffect::SpinOut),
        (ItemKind::Pelote, SoundEffect::Tumble),
    ] {
        forget(&mut app);
        app.world_mut().write_message(ItemHit {
            kart,
            by: bot,
            item,
            object: None,
        });
        run(&mut app, 3);
        assert!(app.world().get::<KartState>(kart).unwrap().is_hit());
        assert_eq!(asked(&app, wanted), 1, "{item:?}: {:?}", sounds(&app));
        assert_eq!(cries(&app, Mood::Hurt), 1, "{:?}", sounds(&app));
        // Hit, then invincible a while.
        run(&mut app, 300);
    }
    let reaction = |app: &App| match *app.world().get::<KartState>(kart).unwrap() {
        KartState::Hit { reaction, .. } => Some(reaction),
        _ => None,
    };
    assert_eq!(reaction(&app), None);
    // The bot hit: from where it is.
    forget(&mut app);
    app.world_mut().write_message(ItemHit {
        kart: bot,
        by: kart,
        item: ItemKind::Abeille,
        object: None,
    });
    run(&mut app, 3);
    let asked_now = app.world().resource::<Asked>().0.clone();
    let spin = asked_now
        .iter()
        .find(|(s, _)| *s == SoundEffect::SpinOut)
        .unwrap();
    assert!(!spin.1.foreground && spin.1.kart == Some(bot));
    assert!(matches!(
        app.world().get::<KartState>(bot).unwrap(),
        KartState::Hit {
            reaction: HitReaction::Spin,
            ..
        }
    ));
    // The pelican (the debug key R).
    forget(&mut app);
    tap(&mut app, crate::kart::DEBUG_RESET_KEY);
    run(&mut app, 3);
    assert_eq!(asked(&app, SoundEffect::Pelican), 1, "{:?}", sounds(&app));
}

/// Issue #137: the Cac's « splotch »: as it lands on the track (from where it lies), and as a kart
/// slides on it (from that kart, lower); the player's own in front. Not for another item.
#[test]
fn the_cacs_splotches_as_it_lands_and_under_a_kart() {
    let (mut app, kart) = started(1);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    let splotches = |app: &App| -> Vec<Cue> {
        app.world()
            .resource::<Asked>()
            .0
            .iter()
            .filter(|(s, _)| *s == SoundEffect::Splotch)
            .map(|(_, cue)| *cue)
            .collect()
    };
    // The bot's Cac's lands on the road just ahead of the player's kart: heard from there.
    let ahead = {
        let transform = app.world().get::<Transform>(kart).unwrap();
        transform.translation + transform.forward() * 6.0
    };
    app.world_mut().spawn((
        Transform::from_translation(ahead),
        ItemObject::new(ItemKind::Pelure, bot, Motion::Ground, ahead, Vec3::ZERO),
    ));
    run(&mut app, 2);
    let landed = splotches(&app);
    assert_eq!(landed.len(), 1, "{:?}", sounds(&app));
    assert!(!landed[0].foreground && landed[0].kart == Some(bot));
    assert!(landed[0].at.distance(ahead) < 1.5, "{:?}", landed[0]);
    assert_eq!(landed[0].speed, 1.0);
    // The player drives over it: it spins out, and its « splotch » is in front, lower.
    forget(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..240 {
        app.update();
        if app.world().get::<KartState>(kart).unwrap().is_hit() {
            break;
        }
    }
    hold_keys(&mut app, &[]);
    run(&mut app, 2);
    assert!(app.world().get::<KartState>(kart).unwrap().is_hit());
    let slid = splotches(&app);
    assert_eq!(slid.len(), 1, "{:?}", sounds(&app));
    assert!(slid[0].foreground && slid[0].kart == Some(kart), "{slid:?}");
    assert!(slid[0].speed < 1.0, "{slid:?}");
    assert_eq!(asked(&app, SoundEffect::SpinOut), 1, "{:?}", sounds(&app));
    // The bot hit by a Cac's: from where it is; by a pelote: no « splotch ».
    run(&mut app, 300);
    for (item, wanted) in [(ItemKind::Pelure, 1), (ItemKind::Pelote, 0)] {
        forget(&mut app);
        app.world_mut().write_message(ItemHit {
            kart: bot,
            by: kart,
            item,
            object: None,
        });
        run(&mut app, 3);
        let heard = splotches(&app);
        assert_eq!(heard.len(), wanted, "{item:?}: {:?}", sounds(&app));
        assert!(
            heard
                .iter()
                .all(|cue| !cue.foreground && cue.kart == Some(bot))
        );
        run(&mut app, 300);
    }
}

/// Issue #137: the « splotch » is soft and wet: short, loud at once then dying (a hit, not a
/// whoosh), low (its noise falls from the mids to the lows: fewer zero crossings in its second
/// half than in its first, and few overall), lower and shorter than the water's splash.
#[test]
fn the_splotch_is_short_soft_and_falls() {
    let mut samples = Vec::new();
    let Clip::Synth(synth) = SoundEffect::Splotch.clip() else {
        panic!("synthesized");
    };
    render(&synth, 0.9, &mut samples);
    let rate = samples.len() as f32 / synth.duration();
    assert!(
        (0.15..0.4).contains(&synth.duration()),
        "{}",
        synth.duration()
    );
    let energy = |part: &[f32]| part.iter().map(|s| s * s).sum::<f32>();
    let half = samples.len() / 2;
    let (first, second) = (&samples[..half], &samples[half..]);
    assert!(energy(first) > 6.0 * energy(second), "a hit, not a whoosh");
    let peak_at = samples
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map_or(0, |(i, _)| i);
    assert!(
        (peak_at as f32) < 0.15 * samples.len() as f32,
        "loud at once: {peak_at}"
    );
    // Zero crossings a second: about twice its frequency.
    let crossings = |part: &[f32]| {
        part.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count() as f32
            * rate
            / part.len() as f32
    };
    let (bright, dull) = (crossings(first), crossings(second));
    assert!(dull < bright, "it falls: {bright} then {dull}");
    assert!(
        crossings(&samples) < 2500.0,
        "soft: {}",
        crossings(&samples)
    );
    let Clip::Synth(splash) = SoundEffect::Splash.clip() else {
        panic!("synthesized");
    };
    assert!(synth.duration() < splash.duration());
    if let (Synth::Noise(splotch), Synth::Noise(splash)) = (synth, splash) {
        assert!(splotch.from_hz < splash.from_hz && splotch.to_hz < splash.to_hz);
    } else {
        panic!("two noises");
    }
}

/// Issue #103 acceptance (GD § 14 and 15): a paw strike's « pof », the target's cry and the
/// striker's effort; a grab; the « boing » of a tether letting go; a bump between karts.
#[test]
fn the_paws_and_the_bumps_sound() {
    let (mut app, kart) = started(1);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().write_message(StrikeAnswer {
        striker: kart,
        target: bot,
        landed: true,
        throw: false,
    });
    app.update();
    assert_eq!(asked(&app, SoundEffect::PawPof), 1, "{:?}", sounds(&app));
    assert_eq!(cries(&app, Mood::Hurt), 1);
    assert_eq!(cries(&app, Mood::Effort), 1);
    // The player struck: its « pof » in front, the target's cry from the target.
    let asked_now = app.world().resource::<Asked>().0.clone();
    assert!(
        asked_now
            .iter()
            .any(|(s, cue)| *s == SoundEffect::PawPof && cue.foreground)
    );
    assert!(
        asked_now
            .iter()
            .any(|(s, cue)| matches!(s, SoundEffect::Cry(_, Mood::Hurt)) && cue.kart == Some(bot))
    );
    // A strike into nothing (not landed): no « pof ».
    forget(&mut app);
    app.world_mut().write_message(StrikeAnswer {
        striker: bot,
        target: kart,
        landed: false,
        throw: false,
    });
    app.update();
    assert_eq!(asked(&app, SoundEffect::PawPof), 0);
    app.world_mut().write_message(KartGrabbed {
        grabber: bot,
        target: kart,
        tick: 0,
        seen: 0,
        grabber_position: Vec3::ZERO,
        rest: 2.0,
    });
    app.update();
    assert_eq!(asked(&app, SoundEffect::Grab), 1);
    app.world_mut().write_message(TetherEnded {
        grabber: bot,
        target: kart,
        reason: TetherEnd::Released,
    });
    app.update();
    // (The grab written here is refused by the kart grabbed: out of reach. Its arm comes back
    // all the same: a « boing » too.)
    assert!(
        asked(&app, SoundEffect::TetherBoing) >= 1,
        "{:?}",
        sounds(&app)
    );
    app.world_mut().write_message(KartBump {
        kart,
        by: bot,
        closing_speed: 9.0,
    });
    app.update();
    assert_eq!(asked(&app, SoundEffect::KartBump), 1, "{:?}", sounds(&app));
}

/// Issue #103 acceptance: the player's last lap, then its finish: the podium's jingle and its
/// animal's cheer (alone, it wins). Issue #104: the jingles are the music's now (`MusicPlugin`,
/// the « Musique » volume): on the last lap, the circuit's music goes out quickly, its jingle
/// plays once, then the circuit's faster version (a higher tempo: the last lap changes the
/// music's speed); at the finish, the race's music goes out (slowly: it does not stop dead), the
/// « best » jingle plays (alone, it wins), then the results' music; the cheer is still the sound
/// design's.
#[test]
fn the_last_lap_and_the_finish_have_their_jingles() {
    let (mut app, kart) = started(0);
    let catalog = music_catalog().unwrap();
    let voice = |app: &mut App| {
        let world = app.world_mut();
        let voices: Vec<MusicVoice> = world.query::<&MusicVoice>().iter(world).copied().collect();
        assert!(voices.len() <= 1, "{voices:?}");
        voices.first().copied()
    };
    let prairie = catalog.index("pure-raceway").unwrap();
    run(&mut app, 2);
    assert_eq!(voice(&mut app).unwrap().piece, prairie);
    let (checkpoints, laps) = {
        let course = app.world().resource::<Course>();
        (course.checkpoint_count(), course.laps)
    };
    assert!(laps > 1);
    app.world_mut()
        .get_mut::<RaceProgress>(kart)
        .unwrap()
        .passed = (laps - 1) * checkpoints + 1;
    let tuning = app.world().resource::<MusicTuning>().clone();
    let frames = |seconds: f64| (seconds * 60.0).round() as usize;
    let sounding = |app: &App| {
        let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
        let heard = app.world().resource::<Jukebox>().deck.heard?;
        Some(heard.sounding(now, |p| catalog.seconds(p)))
    };
    run(&mut app, frames(f64::from(tuning.jingle_fade)) + 2);
    let faster = voice(&mut app).unwrap();
    assert_eq!(Some(faster.piece), catalog.faster(prairie), "{faster:?}");
    assert!(catalog.pieces[faster.piece].bpm > catalog.pieces[prairie].bpm);
    assert_eq!(faster.intro, Some(catalog.final_lap), "after its jingle");
    assert!(!faster.jingle);
    assert_eq!(sounding(&app), Some(catalog.final_lap));
    run(&mut app, frames(catalog.seconds(catalog.final_lap)) - 4);
    assert_eq!(
        sounding(&app),
        Some(catalog.final_lap),
        "once, for its length"
    );
    run(&mut app, 6);
    assert_eq!(
        sounding(&app),
        catalog.faster(prairie),
        "then the faster music"
    );
    assert_eq!(
        voice(&mut app).unwrap().number,
        faster.number,
        "on the same voice"
    );
    assert_eq!(cries(&app, Mood::Win), 0);
    {
        let mut progress = app.world_mut().get_mut::<RaceProgress>(kart).unwrap();
        progress.passed = laps * checkpoints + 1;
        progress.finish = Some(1000);
    }
    run(&mut app, 3);
    assert_eq!(cries(&app, Mood::Win), 1);
    // The race's music goes out over `finish_fade`, down frame after frame.
    let mut levels = Vec::new();
    while let Some(v) = voice(&mut app).filter(|v| v.number == faster.number) {
        levels.push(v.level);
        app.update();
        assert!(levels.len() < 200, "it never went out");
    }
    assert!(levels.windows(2).all(|w| w[1] < w[0]), "{levels:?}");
    assert!(
        levels.len() + 3 >= frames(f64::from(tuning.finish_fade)) - 2,
        "not dead: {} frames",
        levels.len()
    );
    let (best, results) = catalog.finish(Tier::Best);
    let then = voice(&mut app).unwrap();
    assert_eq!((then.piece, then.intro), (results, Some(best)));
    assert_eq!(sounding(&app), Some(best));
    run(&mut app, frames(catalog.seconds(best)) + 2);
    assert_eq!(sounding(&app), Some(results));
    assert_eq!(voice(&mut app).unwrap().number, then.number);
}

/// Issue #103 acceptance: with 12 karts racing (11 bots, the player driving straight on into
/// the walls, the items on), the cap of sounds at once is never exceeded, nor the engines' and
/// the squeals'; the player's engine plays, the others' from the closest karts; sounds were
/// refused or cut for it (the cap, lowered here, is reached).
///
/// Issue #108: on a short circuit, where the pack comes back past the player within the 40 s, as
/// it did on Prairie Pompon before it was redrawn (1.3 km a lap now: the pack does not come back
/// in time, out of hearing). Issue #109: the Crique Câline (690 m a lap), the Col des Moufles
/// being redrawn too (1.3 km).
#[test]
fn the_cap_holds_with_twelve_karts() {
    let (mut app, kart) = started_on(TrackChoice::Circuit("tracks/crique.track.ron".into()), 11);
    {
        let mut tuning = app.world_mut().resource_mut::<SoundTuning>();
        tuning.max_sounds = 6;
    }
    let tuning = app.world().resource::<SoundTuning>().clone();
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let mut heard_others = 0usize;
    let mut most = 0usize;
    for tick in 0..(40 * 60) {
        if tick % 300 == 0 {
            // Items for everybody, now and then.
            app.world_mut().write_message(GiveItem(Prize::PeloteX3));
            let world = app.world_mut();
            for mut slots in world
                .query_filtered::<&mut crate::items::ItemSlots, With<Bot>>()
                .iter_mut(world)
            {
                slots.give(ItemKind::Bombe, 1, 0);
            }
        }
        if tick % 300 == 2 {
            use_item(&mut app);
        }
        app.update();
        let voices = count::<With<Voice>>(&mut app);
        most = most.max(voices);
        assert!(
            voices <= tuning.max_sounds as usize,
            "tick {tick}: {voices}"
        );
        let playing = loops(&mut app);
        for kind in [LoopKind::Engine, LoopKind::Squeal] {
            let max = match kind {
                LoopKind::Engine => tuning.max_engines,
                _ => tuning.max_squeals,
            } as usize;
            let busy = playing
                .iter()
                .filter(|v| v.kind == kind && v.kart.is_some())
                .count();
            assert!(busy <= max, "tick {tick}: {kind:?} {busy}");
        }
        heard_others = heard_others.max(
            playing
                .iter()
                .filter(|v| v.kind == LoopKind::Engine && !v.foreground && v.kart.is_some())
                .count(),
        );
        let front = playing
            .iter()
            .find(|v| v.kind == LoopKind::Engine && v.foreground)
            .unwrap();
        assert_eq!(front.kart, Some(kart));
    }
    let stats = *app.world().resource::<MixerStats>();
    assert_eq!(most, tuning.max_sounds as usize, "{stats:?}");
    assert!(stats.most_voices <= tuning.max_sounds, "{stats:?}");
    assert!(stats.played > 50, "{stats:?}");
    assert!(stats.refused + stats.cut > 0, "{stats:?}");
    assert_eq!(
        heard_others,
        tuning.max_engines as usize - 1,
        "the closest karts"
    );
    // Every sound asked for: the player's in front, the others' from their karts.
    let asked = &app.world().resource::<Asked>().0;
    assert!(asked.len() > 100, "{}", asked.len());
    assert!(
        asked
            .iter()
            .any(|(s, cue)| *s == SoundEffect::KartBump || (!cue.foreground && cue.kart.is_some())),
        "the others' sounds"
    );
}

/// Issue #103 acceptance (docs/UI_CHARTE.md): the screens: the focus moves (survol), a value
/// changes, the screen closes (retour); a choice (validation).
#[test]
fn the_screens_sound() {
    let mut app = sound_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    forget(&mut app);
    tap(&mut app, OPTIONS_KEY);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(asked(&app, SoundEffect::UiHover), 1, "{:?}", sounds(&app));
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(asked(&app, SoundEffect::UiChange), 1, "{:?}", sounds(&app));
    run(&mut app, 30);
    tap(&mut app, KeyCode::Escape);
    assert_eq!(asked(&app, SoundEffect::UiBack), 1, "{:?}", sounds(&app));
    app.world_mut()
        .write_message(crate::modes::RaceMenuAction::Confirm);
    app.update();
    assert_eq!(asked(&app, SoundEffect::UiConfirm), 1, "{:?}", sounds(&app));
    assert!(
        app.world()
            .resource::<Asked>()
            .0
            .iter()
            .all(|(_, cue)| cue.foreground)
    );
}

/// Issue #103 (the lobby on foot, issue #101): steps on the lawn, a jump and its landing, a paw
/// swing.
#[test]
fn on_foot_in_the_lobby() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(crate::salon::SalonPlugin)
        .add_plugins(SoundDesignPlugin)
        .init_resource::<Asked>()
        .add_systems(PostUpdate, record.before(mix));
    wait_for_the_player(&mut app);
    app.world_mut().write_message(crate::level::LoadLevel::on(
        TrackChoice::Salon,
        crate::race::RaceMode::FreeRoam,
    ));
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(count::<With<crate::salon::PlayerWalker>>(&mut app), 1);
    forget(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 120);
    let steps = sounds(&app)
        .iter()
        .filter(|s| matches!(s, SoundEffect::Footstep(_)))
        .count();
    assert!(steps >= 3, "{:?}", sounds(&app));
    hold_keys(&mut app, &[KeyCode::Space]);
    run(&mut app, 2);
    hold_keys(&mut app, &[]);
    run(&mut app, 90);
    assert_eq!(asked(&app, SoundEffect::FootJump), 1, "{:?}", sounds(&app));
    assert!(asked(&app, SoundEffect::Landing) >= 1, "{:?}", sounds(&app));
    hold_keys(&mut app, &[KeyCode::KeyX]);
    run(&mut app, 2);
    hold_keys(&mut app, &[]);
    run(&mut app, 10);
    assert_eq!(asked(&app, SoundEffect::PawSwing), 1, "{:?}", sounds(&app));
}

/// Issue #103: the options' volumes and the mix: the general and effects volumes on every
/// sound, its family's; at 0 %, nothing plays (no voice taken).
#[test]
fn the_volumes_of_the_options_and_the_mix() {
    let tuning = SoundTuning::default();
    let mut options = PlayerOptions::default();
    let full = sound_level(&tuning, &options, SoundEffect::PawPof, 1.0);
    assert!(full > 0.1 && full <= 1.0, "{full}");
    options.master_volume = 40;
    let quieter = sound_level(&tuning, &options, SoundEffect::PawPof, 1.0);
    assert!(quieter < full * 0.5, "{quieter}");
    options.effects_volume = 0;
    assert_eq!(
        sound_level(&tuning, &options, SoundEffect::PawPof, 1.0),
        0.0
    );

    let mut app = sound_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    app.world_mut()
        .resource_mut::<PlayerOptions>()
        .master_volume = 0;
    let before = *app.world().resource::<MixerStats>();
    app.world_mut()
        .spawn((SoundEffect::UiConfirm, Cue::front(None)));
    app.update();
    let after = *app.world().resource::<MixerStats>();
    assert_eq!(after.skipped, before.skipped + 1);
    assert_eq!(after.played, before.played);
    assert_eq!(count::<With<Voice>>(&mut app), 0);
    // At 0 %, the engine is silent too.
    run(&mut app, 60);
    assert_eq!(front_loop(&mut app, LoopKind::Engine).volume, 0.0);
    // Back up: a sound takes a voice for its length, then lets it go.
    app.world_mut()
        .resource_mut::<PlayerOptions>()
        .master_volume = 80;
    app.world_mut()
        .spawn((SoundEffect::UiConfirm, Cue::front(None)));
    app.update();
    assert_eq!(count::<With<Voice>>(&mut app), 1);
    run(
        &mut app,
        (SoundEffect::UiConfirm.length() * 60.0) as usize + 6,
    );
    assert_eq!(count::<With<Voice>>(&mut app), 0);
    // Too far: not played.
    let range = app.world().resource::<SoundTuning>().range;
    app.world_mut().spawn((
        SoundEffect::KartBump,
        Cue::at(None, Vec3::new(0.0, 0.0, range * 3.0)),
    ));
    app.update();
    assert_eq!(count::<With<Voice>>(&mut app), 0);
}

/// Issue #103: every synthesized sound renders bounded, at its length, silent at both ends.
#[test]
fn every_synthesized_sound_renders_bounded() {
    let mut samples = Vec::new();
    for sound in SoundEffect::synthesized() {
        let Clip::Synth(synth) = sound.clip() else {
            panic!("{sound:?}");
        };
        render(&synth, 0.9, &mut samples);
        assert_eq!(samples.len(), synth.len(), "{sound:?}");
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.9).abs() < 1e-3, "{sound:?}: {peak}");
        assert!(samples.iter().all(|s| s.is_finite()), "{sound:?}");
        assert!(samples[0].abs() < 0.02, "{sound:?} starts at 0");
        assert!(samples.last().unwrap().abs() < 0.02, "{sound:?} ends at 0");
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!(rms > 0.02, "{sound:?} audible: {rms}");
    }
}

/// Issue #103 (CLAUDE.md, « Assets »), extended by issue #135: every sound file the game plays,
/// and every file of the sound picker's candidates, is in `assets/audio/`, with its source's
/// CC0 licence next to it (`License.txt`: Kenney's packs, or the page of each file on
/// OpenGameArt) and its line in `assets/CREDITS.md` (Kenney's pack, or the OpenGameArt page,
/// and « CC0 »); its length is the one the mixer counts (decoded here); no other file is there.
#[test]
fn every_sound_file_is_there_with_its_licence_and_its_credits() {
    let credits = std::fs::read_to_string(format!("{ASSETS}/CREDITS.md")).unwrap();
    let mut files: Vec<(&str, f32)> = FILES.to_vec();
    for slot in Slot::ALL {
        for candidate in slot.candidates() {
            files.extend(candidate.files());
            files.extend(candidate.recording());
        }
    }
    files.sort_by(|a, b| a.0.cmp(b.0));
    files.dedup_by(|a, b| a.0 == b.0);
    assert!(files.len() > FILES.len() + 60, "{}", files.len());
    let mut wanted = BTreeSet::new();
    let mut packs = BTreeSet::new();
    let mut mismatches = Vec::new();
    for (path, length) in files {
        let pack = path.split('/').nth(1).unwrap();
        let file = path.rsplit('/').next().unwrap();
        packs.insert(pack);
        wanted.insert(path.to_string());
        let bytes = std::fs::read(format!("{ASSETS}/{path}")).unwrap_or_else(|_| panic!("{path}"));
        let source = bevy::audio::AudioSource {
            bytes: bytes.into(),
        };
        let decoder = source.decoder();
        let (channels, rate) = (decoder.channels().get(), decoder.sample_rate().get());
        let seconds = decoder.count() as f32 / f32::from(channels) / rate as f32;
        if !(seconds <= length + 0.005 && seconds > length - 0.06) {
            mismatches.push(format!("{path}: {seconds:.4} s, counted {length} s"));
        }
        let line = credits
            .lines()
            .find(|line| line.contains(&format!("`audio/{pack}/")))
            .unwrap_or_else(|| panic!("no line for audio/{pack}/ in assets/CREDITS.md"));
        assert!(line.contains(file), "{file} not in: {line}");
        assert!(line.contains("CC0"), "{line}");
        assert!(
            (line.contains("Kenney") && line.contains(&format!("https://kenney.nl/assets/{pack}")))
                || line.contains("https://opengameart.org/content/"),
            "{line}"
        );
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
    for pack in &packs {
        let licence = std::fs::read_to_string(format!("{ASSETS}/audio/{pack}/License.txt"))
            .unwrap_or_else(|_| panic!("assets/audio/{pack}/License.txt"));
        assert!(
            licence.contains("CC0")
                && (licence.contains("Kenney")
                    || licence.contains("https://opengameart.org/content/")),
            "{pack}"
        );
        wanted.insert(format!("audio/{pack}/License.txt"));
    }
    let mut present = BTreeSet::new();
    let mut folders = vec![format!("{ASSETS}/audio")];
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
    // Nothing of Mario Kart or of any brand: Kenney's packs, CC0 files of OpenGameArt (and our
    // syntheses).
    assert!(packs.len() >= 5, "{packs:?}");
}

/// Issue #103, changed by issue #135: behind the home screen, its background race makes no sound
/// at all (#103 let its engines hum at 35 %, `home_volume`; Max: « Ils doivent être coupés [...]
/// sur l'accueil »): its karts are there, the engines' voices follow them, silent; no short sound
/// either. The home screen has its music only.
#[test]
fn the_home_background_is_silent() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(crate::menu::MainMenuPlugin { at_launch: true })
        .add_plugins(SoundDesignPlugin)
        .init_resource::<Asked>()
        .add_systems(PostUpdate, record.before(mix));
    wait_for_the_player(&mut app);
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<crate::menu::Showcase>()
            .is_some_and(|showcase| showcase.running())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(app.world().resource::<crate::menu::MainMenu>().on_home());
    forget(&mut app);
    let mut loudest = 0.0f32;
    for _ in 0..600 {
        app.update();
        loudest = loops(&mut app)
            .iter()
            .fold(loudest, |most, voice| most.max(voice.volume));
    }
    assert!(sounds(&app).is_empty(), "{:?}", sounds(&app));
    assert_eq!(loudest, 0.0, "no loop heard on the home screen");
    let engines: Vec<LoopVoice> = loops(&mut app)
        .into_iter()
        .filter(|v| v.kind == LoopKind::Engine && v.kart.is_some())
        .collect();
    assert!(!engines.is_empty(), "the karts are there");
    assert_eq!(
        *app.world().resource::<RaceSounds>(),
        RaceSounds::default(),
        "nothing of the karts heard"
    );
}

/// The loudest of each loop now (the player's and the others').
fn loudness(app: &mut App, kind: LoopKind) -> f32 {
    loops(app)
        .iter()
        .filter(|v| v.kind == kind)
        .fold(0.0, |most, v| most.max(v.volume))
}

/// Issue #135 acceptance: the engine and the drift are heard only during a race: silent on the
/// grid, heard from the countdown, the squeal while drifting; the pause fades them out (not cut
/// dead: down frame after frame) and back; after the player's finish (its results), they fade
/// out for good, and the karts' short sounds stop too.
#[test]
fn the_engine_and_the_drift_are_heard_only_in_a_race() {
    let (mut app, kart) = race_app(3);
    use crate::race::RacePhase;
    let phase = |app: &App| app.world().resource::<Race>().phase();
    // On the grid: nothing yet.
    let mut on_grid = 0;
    while phase(&app) == RacePhase::Grid {
        assert_eq!(loudness(&mut app, LoopKind::Engine), 0.0, "on the grid");
        app.update();
        on_grid += 1;
        assert!(on_grid < 2000);
    }
    assert!(on_grid > 10, "{on_grid}");
    // The countdown: the engines idle.
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(phase(&app), RacePhase::Countdown);
    let idle = front_loop(&mut app, LoopKind::Engine);
    assert_eq!(idle.kart, Some(kart));
    assert!(idle.volume > 0.03, "{idle:?}");
    // GO, and a drift: the squeal.
    assert!(skip_countdown_now(app.world_mut()));
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    hold_keys(
        &mut app,
        &[KeyCode::ArrowUp, KeyCode::Space, KeyCode::ArrowLeft],
    );
    let mut squeal = 0.0f32;
    for _ in 0..90 {
        app.update();
        squeal = squeal.max(front_loop(&mut app, LoopKind::Squeal).volume);
    }
    assert!(squeal > 0.05, "{squeal}");
    // The pause: a short fade out, then nothing; back after it.
    let before = front_loop(&mut app, LoopKind::Engine).volume;
    assert!(before > 0.03);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let mut levels = Vec::new();
    for _ in 0..60 {
        app.update();
        levels.push(front_loop(&mut app, LoopKind::Engine).volume);
    }
    assert!(levels[0] > 0.0 && levels[0] < before, "a fade: {levels:?}");
    assert!(levels.windows(2).all(|w| w[1] <= w[0]), "{levels:?}");
    assert!(
        levels
            .iter()
            .position(|v| *v == 0.0)
            .is_some_and(|at| at < 45),
        "short: {levels:?}"
    );
    assert_eq!(loudness(&mut app, LoopKind::Squeal), 0.0);
    app.world_mut().resource_mut::<Time<Virtual>>().unpause();
    run(&mut app, 30);
    assert!(front_loop(&mut app, LoopKind::Engine).volume > 0.03);
    // The player finishes: its results; the engines and the squeals fade out for good.
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    {
        let (checkpoints, laps) = {
            let course = app.world().resource::<Course>();
            (course.checkpoint_count(), course.laps)
        };
        let mut progress = app.world_mut().get_mut::<RaceProgress>(kart).unwrap();
        progress.passed = laps * checkpoints + 1;
        progress.finish = Some(1000);
    }
    run(&mut app, 60);
    let mut loudest = 0.0f32;
    for _ in 0..300 {
        app.update();
        for kind in LoopKind::ALL {
            loudest = loudest.max(loudness(&mut app, kind));
        }
    }
    assert_eq!(loudest, 0.0, "after the finish");
    // Nor the karts' short sounds (a bump between two bots, a drift of the player's kart).
    let bots: Vec<Entity> = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .iter(world)
            .collect()
    };
    forget(&mut app);
    app.world_mut().write_message(KartBump {
        kart: bots[0],
        by: bots[1],
        closing_speed: 12.0,
    });
    run(&mut app, 3);
    assert_eq!(asked(&app, SoundEffect::KartBump), 0, "{:?}", sounds(&app));
    assert_eq!(*app.world().resource::<RaceSounds>(), RaceSounds::default());
}

/// The same bump during a race: heard (what the finish silences above).
#[test]
fn a_bump_is_heard_during_a_race() {
    let (mut app, _) = started(2);
    let bots: Vec<Entity> = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .iter(world)
            .collect()
    };
    run(&mut app, 2);
    forget(&mut app);
    app.world_mut().write_message(KartBump {
        kart: bots[0],
        by: bots[1],
        closing_speed: 12.0,
    });
    run(&mut app, 3);
    assert_eq!(asked(&app, SoundEffect::KartBump), 1, "{:?}", sounds(&app));
}

/// Issue #135 acceptance: in the lobby, no engine: driving on the test ground, the engine hums;
/// in the lobby's level (on foot), every loop is silent.
#[test]
fn no_engine_in_the_lobby() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(crate::salon::SalonPlugin)
        .add_plugins(SoundDesignPlugin)
        .init_resource::<Asked>()
        .add_systems(PostUpdate, record.before(mix));
    wait_for_the_player(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    assert!(front_loop(&mut app, LoopKind::Engine).volume > 0.03);
    app.world_mut().write_message(crate::level::LoadLevel::on(
        TrackChoice::Salon,
        crate::race::RaceMode::FreeRoam,
    ));
    run(&mut app, 30);
    assert_eq!(count::<With<crate::salon::PlayerWalker>>(&mut app), 1);
    // (The engine's short fade out, then nothing.)
    run(&mut app, 60);
    let mut loudest = 0.0f32;
    for _ in 0..120 {
        app.update();
        for kind in LoopKind::ALL {
            loudest = loudest.max(loudness(&mut app, kind));
        }
    }
    assert_eq!(loudest, 0.0);
    assert_eq!(*app.world().resource::<RaceSounds>(), RaceSounds::default());
}

/// Issue #135 acceptance: a choice in the admin panel (`SoundChoices`, what its list writes)
/// changes the sound played in a race: the player's engine fades out quickly, takes the new
/// candidate and comes back; the squeal the same; a short sound plays its new clip (its voice
/// holds the new clip's length).
#[test]
fn a_choice_in_the_panel_changes_the_sound_in_a_race() {
    let (mut app, kart) = started(0);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    let engine = front_loop(&mut app, LoopKind::Engine);
    assert_eq!((engine.kart, engine.source), (Some(kart), Some(0)));
    assert!(engine.volume > 0.03, "{engine:?}");
    assert!(
        app.world_mut()
            .resource_mut::<SoundChoices>()
            .choose(Slot::Engine, 2)
    );
    let mut frames = 0;
    let mut silent = false;
    while front_loop(&mut app, LoopKind::Engine).source != Some(2) {
        app.update();
        frames += 1;
        let voice = front_loop(&mut app, LoopKind::Engine);
        silent |= voice.volume == 0.0;
        assert!(frames < 20, "changed at once: {voice:?}");
    }
    assert!(silent, "faded out before the new one");
    run(&mut app, 60);
    let changed = front_loop(&mut app, LoopKind::Engine);
    assert_eq!(changed.source, Some(2));
    assert!(changed.volume > 0.03, "{changed:?}");
    // The others' engines too.
    assert!(
        loops(&mut app)
            .iter()
            .filter(|v| v.kind == LoopKind::Engine)
            .all(|v| v.source == Some(2))
    );
    // The squeal.
    app.world_mut()
        .resource_mut::<SoundChoices>()
        .choose(Slot::Squeal, 3);
    run(&mut app, 3);
    assert_eq!(front_loop(&mut app, LoopKind::Squeal).source, Some(3));
    // A short sound: its new clip (qubodup's crash, 0.6 s, instead of Kenney's 0.341 s).
    let voice_length = |app: &mut App| {
        app.world_mut()
            .spawn((SoundEffect::KartBump, Cue::front(Some(kart))));
        app.update();
        let world = app.world_mut();
        let voice = world
            .query_filtered::<&Voice, With<SoundEffect>>()
            .iter(world)
            .filter(|v| v.ends_at - v.started > 0.0)
            .map(|v| v.ends_at - v.started)
            .fold(0.0f64, f64::max);
        world
            .query_filtered::<Entity, With<Voice>>()
            .iter(world)
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|e| {
                world.despawn(e);
            });
        voice
    };
    let before = voice_length(&mut app);
    assert!((before - (0.341 + 0.05)).abs() < 1e-3, "{before}");
    app.world_mut()
        .resource_mut::<SoundChoices>()
        .choose(Slot::KartBump, 1);
    // (Not the same sound of the same kart twice in 80 ms.)
    run(&mut app, 6);
    let after = voice_length(&mut app);
    let crash = Slot::KartBump.candidates()[1].files()[0];
    assert_eq!(crash.0, "audio/qubodup/crash.ogg");
    assert!((after - f64::from(crash.1 + 0.05)).abs() < 1e-3, "{after}");
}

/// Issue #135: « ▶ Écouter » plays a sound at once, even out of a race (here: paused, on the
/// test ground): a short sound in front, the drift's chimes in their time; the engine a short
/// rev (up, then down) that goes away by itself.
#[test]
fn listening_plays_a_sound_at_once_even_out_of_a_race() {
    let mut app = sound_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    run(&mut app, 3);
    forget(&mut app);
    app.world_mut().write_message(PreviewSound(Slot::Splotch));
    run(&mut app, 2);
    assert_eq!(asked(&app, SoundEffect::Splotch), 1, "{:?}", sounds(&app));
    assert!(
        app.world()
            .resource::<Asked>()
            .0
            .iter()
            .all(|(_, cue)| cue.foreground)
    );
    forget(&mut app);
    app.world_mut().write_message(PreviewSound(Slot::DriftTier));
    let mut heard = Vec::new();
    for _ in 0..120 {
        app.update();
        for sound in sounds(&app) {
            if !heard.contains(&sound) {
                heard.push(sound);
            }
        }
    }
    assert_eq!(heard, MiniTurbo::ALL.map(SoundEffect::DriftTier), "in turn");
    // The engine: a rev.
    app.world_mut().write_message(PreviewSound(Slot::Engine));
    let mut pitches = Vec::new();
    let mut frames = 0;
    loop {
        app.update();
        frames += 1;
        let world = app.world_mut();
        let Some(preview) = world.query::<&LoopPreview>().iter(world).next().copied() else {
            break;
        };
        assert_eq!(preview.kind, LoopKind::Engine);
        pitches.push(preview.pitch);
        assert!(frames < 400, "it goes away");
    }
    assert!(frames > 60, "{frames}");
    let top = pitches.iter().copied().fold(0.0f32, f32::max);
    assert!(top > pitches[1] + 0.4, "up: {pitches:?}");
    assert!(
        *pitches.last().unwrap() < top - 0.2,
        "and down: {pitches:?}"
    );
    // The race's loops stay silent meanwhile (paused).
    assert_eq!(loudness(&mut app, LoopKind::Engine), 0.0);
}

/// Issue #176: on the grid, the accelerator down revs the player's engine (the race holds the
/// karts' input at zero until GO; with a renderer, the particles saw what the driver pressed).
#[test]
fn the_engine_revs_on_the_grid() {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.add_plugins((
        crate::ui::RoulettePlugin,
        SoundDesignPlugin,
        MusicPlugin,
        crate::vfx::VfxPlugin,
    ))
    .init_resource::<Asked>()
    .add_systems(PostUpdate, record.before(mix));
    wait_for_the_player(&mut app);
    for _ in 0..600 {
        let world = app.world();
        let counting = world
            .get_resource::<Race>()
            .is_some_and(|race| race.flow.countdown_number(&race.timings) == Some(3));
        if counting {
            break;
        }
        app.update();
    }
    run(&mut app, 20);
    let idle = front_loop(&mut app, LoopKind::Engine);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 50);
    assert!(
        !app.world().resource::<Race>().started(),
        "still on the grid"
    );
    let revving = front_loop(&mut app, LoopKind::Engine);
    println!("{idle:?} {revving:?}");
    assert!(revving.pitch > idle.pitch + 0.05, "{idle:?} {revving:?}");
    assert!(revving.volume > idle.volume, "{idle:?} {revving:?}");
}
