//! Headless scenarios of the time trial (issue #76): the whole game (`GamePlugin`) with
//! `MinimalPlugins` and the time trial's plugin (added by hand, without the storage: it only comes
//! with a renderer), one fixed tick per `app.update()`.

use std::sync::Arc;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, RigidBody, Velocity};

use super::*;
use crate::admin::AdminOptions;
use crate::characters::{DriverLooks, Loadout, PlayerColor, PlayerLoadout, Roster};
use crate::croquettes::TrackCroquette;
use crate::interpolation::PhysicsPose;
use crate::items::{ItemBox, ItemKind, ItemSlots, ItemStats, Slot};
use crate::kart::{
    BodyPaint, Boost, DriverVisual, ITEM_KEYS, Kart, KartBodyPivot, KartColor, KartInput,
    KartTuning, PlayerKart, Remote, Slipstream,
};
use crate::net::NetTuning;
use crate::race::{RaceProgress, RaceTuning};
use crate::tests::{follow_centerline, game_app_unloaded, single_threaded, wait_for_the_player};
use crate::track::{ItemBoxSpot, TrackLayout};
use crate::ui::HudText;

fn tap(app: &mut App, key_code: KeyCode, logical_key: Key) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

/// The game on Prairie Pompon with `bots` bots, items on, races of one lap without countdown,
/// and the time trial's plugin (no storage), once the player's kart exists.
fn trial_game(bots: usize) -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), bots);
    app.add_plugins(TimeTrialPlugin { storage: false });
    app.world_mut().resource_mut::<RaceTuning>().laps = 1;
    app.world_mut()
        .resource_mut::<AdminOptions>()
        .skip_countdown = true;
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    app
}

/// Runs updates until the level on the circuit `name` is built, with its race and the player's
/// kart.
fn wait_for_the_level(app: &mut App, name: &str) {
    for _ in 0..3000 {
        app.update();
        let world = app.world_mut();
        let built = world
            .get_resource::<TrackLayout>()
            .is_some_and(|layout| layout.name == name);
        let players = world
            .query_filtered::<(), With<PlayerKart>>()
            .iter(world)
            .count();
        if built && players == 1 && world.contains_resource::<Race>() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the level on {name} was never built");
}

/// N, the mode twice to the right (free race, Grand Prix, time trial), Entrée: a time trial on
/// the circuit the menu opens on (Prairie Pompon).
fn start_time_trial(app: &mut App) {
    tap(app, RACE_MENU_KEY, Key::Character("n".into()));
    tap(app, KeyCode::ArrowRight, Key::ArrowRight);
    tap(app, KeyCode::ArrowRight, Key::ArrowRight);
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        GameMode::TimeTrial
    );
    tap(app, KeyCode::Enter, Key::Enter);
    wait_for_the_level(app, "Prairie Pompon");
}

fn count<C: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), With<C>>().iter(world).count()
}

fn the_kart(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, (With<Kart>, With<Racer>)>()
        .single(world)
        .expect("one racer")
}

fn trial_texts(app: &mut App) -> Vec<(TrialText, String)> {
    let world = app.world_mut();
    let mut texts: Vec<(TrialText, String)> = world
        .query::<(&TrialText, &Text)>()
        .iter(world)
        .map(|(field, text)| (*field, text.0.clone()))
        .collect();
    texts.sort_by_key(|(field, _)| format!("{field:?}"));
    texts
}

fn trial_text(app: &mut App, field: TrialText) -> String {
    trial_texts(app)
        .into_iter()
        .find(|(f, _)| *f == field)
        .map(|(_, text)| text)
        .unwrap_or_default()
}

/// Issue #76 acceptance: the race menu's third mode starts a time trial on the circuit chosen:
/// no bot (the player's grid is kept), no item box, the croquettes there, three piments in the
/// hand that boost three times, then nothing; the race HUD's rank is hidden, the record shown at
/// the start. The next free race gives the bots and the boxes back.
#[test]
fn a_time_trial_is_alone_with_three_piments_and_no_box() {
    let mut app = trial_game(3);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(count::<Racer>(&mut app), 4);
    let boxes = count::<ItemBoxSpot>(&mut app);
    assert!(boxes > 0);

    start_time_trial(&mut app);
    assert_eq!(app.world().resource::<TimeTrial>().solo_bots, 3);
    assert_eq!(app.world().resource::<RaceGrid>().bots, 0);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(count::<Racer>(&mut app), 1, "alone");
    assert_eq!(count::<ItemBoxSpot>(&mut app), 0, "no box");
    assert_eq!(count::<ItemBox>(&mut app), 0);
    assert!(count::<TrackCroquette>(&mut app) > 0, "the croquettes stay");
    let kart = the_kart(&mut app);
    let slots = *app.world().get::<ItemSlots>(kart).unwrap();
    assert_eq!(
        slots.hand,
        Slot::Ready {
            item: ItemKind::Piment,
            count: 3
        }
    );
    assert_eq!(slots.reserve, Slot::Empty);
    assert!(app.world().resource::<Race>().started());
    assert_eq!(trial_text(&mut app, TrialText::Record), "");
    let world = app.world_mut();
    let ranks: Vec<Visibility> = world
        .query::<(&HudText, &Visibility)>()
        .iter(world)
        .filter(|(field, _)| matches!(field, HudText::Rank | HudText::RacerCount))
        .map(|(_, visibility)| *visibility)
        .collect();
    assert_eq!(ranks, [Visibility::Hidden; 2], "alone: no rank");

    // Three presses of the item button: three boosts, then the hand is empty.
    for used in 1..=3u32 {
        crate::tests::hold_keys(&mut app, &[KeyCode::ArrowUp, ITEM_KEYS[0]]);
        app.update();
        crate::tests::hold_keys(&mut app, &[KeyCode::ArrowUp]);
        app.update();
        let world = app.world();
        assert_eq!(world.get::<ItemStats>(kart).unwrap().used, used);
        assert!(
            world.get::<Boost>(kart).unwrap().is_active(),
            "piment {used}"
        );
        let left = 3 - used as u8;
        let hand = world.get::<ItemSlots>(kart).unwrap().hand;
        if left > 0 {
            assert_eq!(
                hand,
                Slot::Ready {
                    item: ItemKind::Piment,
                    count: left
                }
            );
        } else {
            assert_eq!(hand, Slot::Empty);
        }
        for _ in 0..30 {
            app.update();
        }
    }
    crate::tests::hold_keys(&mut app, &[KeyCode::ArrowUp, ITEM_KEYS[0]]);
    app.update();
    crate::tests::hold_keys(&mut app, &[]);
    app.update();
    assert_eq!(app.world().get::<ItemStats>(kart).unwrap().used, 3);
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(count::<ItemBoxSpot>(&mut app), 0, "still no box");

    // N opens on the time trial; a free race gives the bots and the boxes back.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        GameMode::TimeTrial
    );
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        GameMode::FreeRace,
        "around"
    );
    tap(&mut app, KeyCode::Enter, Key::Enter);
    wait_for_the_level(&mut app, "Prairie Pompon");
    for _ in 0..3 {
        app.update();
    }
    assert!(!app.world().contains_resource::<TimeTrial>());
    assert_eq!(app.world().resource::<RaceGrid>().bots, 3);
    assert_eq!(count::<Racer>(&mut app), 4);
    assert_eq!(count::<ItemBoxSpot>(&mut app), boxes);
    assert_eq!(count::<TrialHud>(&mut app), 0);
}

/// The time trial's racer drives itself: the keyboard leaves it (no `PlayerKart`), a simple
/// driver follows the centerline once `delay` ticks of the race have passed. Returns the
/// racer's position at each race tick (the end of the tick), to the finish.
fn drive_a_run(
    app: &mut App,
    delay: u32,
    mut each_tick: impl FnMut(&mut App, Entity, u32),
) -> Vec<Vec3> {
    let kart = the_kart(app);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    let layout = app.world().resource::<TrackLayout>().clone();
    let mut positions: Vec<Vec3> = Vec::new();
    for _ in 0..(120 * 60) {
        let world = app.world();
        let race_ticks = world.resource::<Race>().race_ticks();
        let transform = *world.get::<Transform>(kart).unwrap();
        let speed = world.get::<Velocity>(kart).unwrap().linear.length();
        let input = if race_ticks >= delay {
            follow_centerline(&layout, &transform, speed)
        } else {
            KartInput::default()
        };
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
        app.update();
        let world = app.world();
        let race_ticks = world.resource::<Race>().race_ticks() as usize;
        let at = world.get::<Transform>(kart).unwrap().translation;
        positions.truncate(race_ticks);
        while positions.len() <= race_ticks {
            positions.push(at);
        }
        each_tick(app, kart, race_ticks as u32);
        if let Some(finish) = app.world().get::<RaceProgress>(kart).unwrap().finish {
            positions.truncate(finish as usize + 1);
            return positions;
        }
    }
    panic!("the run never finished");
}

/// Issue #76 acceptance: a run is recorded tick by tick and becomes the record with its ghost,
/// which is exactly the run (each frame at the kart's position of that tick, to the
/// millimeter); the next runs race against it: the ghost is shown where the run was, tick by
/// tick. The ghost touches nothing: a kart that drives in its tracks, overlapping it at the
/// start then right in its wake, drives exactly as the same kart alone (bit for bit: no
/// collision, no slipstream), and the ghost has no collider, no rigid body, no item slot, and
/// is neither a kart nor a remote kart (no target for the items, the bots or the paws).
#[test]
fn the_ghost_replays_the_run_and_touches_nothing() {
    // The run recorded.
    let mut recording = trial_game(0);
    start_time_trial(&mut recording);
    let started = recording.world().resource::<Race>().started();
    assert_eq!(
        trial_text(&mut recording, TrialText::Record),
        if started { "" } else { "PAS ENCORE DE RECORD" }
    );
    let run = drive_a_run(&mut recording, 0, |_, _, _| {});
    let finish = run.len() as u32 - 1;
    let state = recording.world().resource::<TimeTrialState>();
    let key = state.run.key.clone().unwrap();
    assert_eq!(key.circuit, "tracks/prairie.track.ron");
    assert_eq!(
        state.run.finish,
        Some((
            finish,
            Verdict {
                record: true,
                previous: None
            }
        ))
    );
    let record = state.records.get(&key).unwrap().clone();
    assert_eq!(record.time, finish);
    assert_eq!(record.laps.len(), 1);
    assert_eq!(record.game, GAME_VERSION);
    let ghost = record.ghost.clone().unwrap();
    assert_eq!(ghost.frames.len(), run.len(), "a frame per race tick");
    for (k, (frame, at)) in ghost.frames.iter().zip(&run).enumerate() {
        let recorded = frame.pose().position;
        assert!(
            (recorded - *at).abs().max_element() < 0.001,
            "tick {k}: {recorded} vs {at}"
        );
    }
    recording.update();
    assert_eq!(trial_text(&mut recording, TrialText::Record), "RECORD !");
    assert_eq!(
        trial_text(&mut recording, TrialText::Verdict),
        "PREMIER RECORD DU CIRCUIT"
    );
    assert!(trial_text(&mut recording, TrialText::Lap(0)).starts_with("TOUR 1  0:"));
    // The stored text gives the same ghost back.
    let text = recording
        .world()
        .resource::<TimeTrialState>()
        .records
        .to_ron();
    assert_eq!(records_from_text(&text), state_records(&recording));
    let records = state_records(&recording);

    // Entrée on the results: the race again, against the record's ghost, three piments in the
    // hand again.
    for _ in 0..(30 * 60) {
        recording.update();
        let race = recording.world().resource::<Race>();
        if race.phase() == crate::race::RacePhase::Results && race.flow.can_restart(&race.timings) {
            break;
        }
    }
    tap(&mut recording, KeyCode::Enter, Key::Enter);
    for _ in 0..3 {
        recording.update();
    }
    let kart = the_kart(&mut recording);
    let state = recording.world().resource::<TimeTrialState>();
    assert_eq!(state.run.number, 2);
    assert_eq!(state.run.record, Some(finish));
    assert!(
        Arc::ptr_eq(state.run.ghost.as_ref().unwrap(), &ghost),
        "the record's ghost"
    );
    assert!(state.recorded().len() < 10, "a new recording");
    assert_eq!(
        recording.world().get::<ItemSlots>(kart).unwrap().hand,
        Slot::Ready {
            item: ItemKind::Piment,
            count: 3
        }
    );
    assert_eq!(count::<GhostKart>(&mut recording), 1);

    // The same run, a few ticks late: with the ghost (`chasing`), and alone (`alone`).
    let delay = 8;
    let mut chasing = trial_game(0);
    chasing.world_mut().resource_mut::<TimeTrialState>().records = records;
    let mut alone = trial_game(0);
    start_time_trial(&mut chasing);
    start_time_trial(&mut alone);
    let limits = NetTuning::default().limits();
    let tuning = KartTuning::default();
    let mut shown_on_ticks = 0;
    let mut in_its_wake = 0;
    let mut overlapping = 0;
    let mut alone_run: Vec<(Transform, Velocity, Slipstream)> = Vec::new();
    drive_a_run(&mut alone, delay, |app, kart, _| {
        let world = app.world();
        alone_run.push((
            *world.get::<Transform>(kart).unwrap(),
            *world.get::<Velocity>(kart).unwrap(),
            *world.get::<Slipstream>(kart).unwrap(),
        ));
    });
    let mut tick = 0;
    drive_a_run(&mut chasing, delay, |app, kart, race_ticks| {
        let world = app.world();
        let transform = *world.get::<Transform>(kart).unwrap();
        let velocity = *world.get::<Velocity>(kart).unwrap();
        let (alone_transform, alone_velocity, alone_slipstream) = alone_run[tick];
        tick += 1;
        assert_eq!(transform, alone_transform, "race tick {race_ticks}");
        assert_eq!(velocity, alone_velocity, "race tick {race_ticks}");
        assert_eq!(*world.get::<Slipstream>(kart).unwrap(), alone_slipstream);
        let state = world.resource::<TimeTrialState>();
        assert!(state.run.ghost.is_some());
        let fraction = world.resource::<Time<Fixed>>().overstep_fraction();
        assert_eq!(fraction, 0.0, "a tick per frame");
        let world = app.world_mut();
        let visuals: Vec<(Transform, Visibility)> = world
            .query_filtered::<(&Transform, &Visibility), With<GhostVisual>>()
            .iter(world)
            .map(|(t, v)| (*t, *v))
            .collect();
        assert_eq!(visuals.len(), 1);
        let (shown, visibility) = visuals[0];
        // Shown between its last two ticks, like the kart: on a tick, the one before.
        let recorded_tick = race_ticks.saturating_sub(1) as usize;
        if recorded_tick < run.len() {
            assert_ne!(visibility, Visibility::Hidden);
            assert!(
                (shown.translation - run[recorded_tick]).abs().max_element() < 0.001,
                "race tick {race_ticks}: the ghost at {} instead of {}",
                shown.translation,
                run[recorded_tick]
            );
            shown_on_ticks += 1;
            let pose = ghost.sample(recorded_tick as f64, &limits).unwrap();
            assert!(shown.translation.distance(pose.position) < 1e-4);
        } else {
            assert_eq!(visibility, Visibility::Hidden, "gone past its finish");
        }
        // Where the ghost is against the kart (as the kart was a tick before, as shown).
        let kart_shown = world.get::<PhysicsPose>(kart).unwrap().previous;
        let to_ghost = shown.translation - kart_shown.translation;
        let ahead = to_ghost.dot(*kart_shown.forward());
        let fast = velocity.linear.length() > tuning.slipstream_min_speed * tuning.max_speed;
        if to_ghost.length() < 2.0 {
            overlapping += 1;
        }
        if fast
            && ahead > 0.0
            && to_ghost.length() < tuning.slipstream_range
            && to_ghost.angle_between(*kart_shown.forward()) < tuning.slipstream_cone
        {
            in_its_wake += 1;
        }
    });
    assert_eq!(tick, alone_run.len(), "the same run, to the finish");
    assert!(shown_on_ticks > 600, "{shown_on_ticks}");
    assert!(
        overlapping > 5,
        "the kart in the ghost: {overlapping} ticks"
    );
    assert!(
        in_its_wake > 120,
        "the kart in the ghost's wake: {in_its_wake} ticks"
    );

    // The ghost is nothing to the physics nor to the race.
    let world = chasing.world_mut();
    let ghost_body = world
        .query_filtered::<Entity, With<GhostKart>>()
        .single(world)
        .unwrap();
    let root = world
        .query_filtered::<Entity, With<GhostVisual>>()
        .single(world)
        .unwrap();
    let body = world.entity(ghost_body);
    assert!(!body.contains::<Kart>() && !body.contains::<Remote>());
    assert!(!body.contains::<Racer>() && !body.contains::<ItemSlots>());
    for entity in descendants(world, root).into_iter().chain([ghost_body]) {
        let entity = world.entity(entity);
        assert!(!entity.contains::<Collider>() && !entity.contains::<RigidBody>());
    }
}

fn state_records(app: &App) -> TrialRecords {
    app.world().resource::<TimeTrialState>().records.clone()
}

/// Every entity under `root`, `root` included.
fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        if let Some(children) = world.get::<Children>(all[i]) {
            all.extend(children.iter());
        }
        i += 1;
    }
    all
}

/// Issue #76: the ghost wears the animal, the parts and the color of the run it replays (not the
/// player's of now), in translucent copies of the kart's materials, made once (none per
/// frame), casting no shadow; a restart races it again, three piments in the hand again; it goes
/// with the time trial.
#[test]
fn the_ghost_wears_the_parts_of_its_run_in_translucent_materials() {
    let mut app = trial_game(0);
    let roster = app.world().resource::<Roster>().clone();
    let theirs = Loadout {
        character: 3 % roster.characters.len(),
        kart: 2 % roster.karts.len(),
        wheels: 1 % roster.wheels.len(),
        wing: 1 % roster.wings.len(),
    };
    assert_ne!(theirs, app.world().resource::<PlayerLoadout>().0);
    let color = KartColor::ALL[5];
    assert_ne!(color, app.world().resource::<PlayerColor>().0);
    let key = TrialKey {
        circuit: "tracks/prairie.track.ron".into(),
        class: crate::kart::SpeedClass::Cc150,
        mirror: false,
    };
    let frames = super::ghost::tests::run(600);
    let mut records = TrialRecords::default();
    records.insert(TrialRecord {
        key,
        time: 600,
        laps: vec![600],
        game: "0.0.1".into(),
        ghost: Some(Arc::new(GhostTrack {
            frames,
            loadout: [theirs.character, theirs.kart, theirs.wheels, theirs.wing].map(|p| p as u8),
            color,
        })),
    });
    app.world_mut().resource_mut::<TimeTrialState>().records = records;
    app.world_mut()
        .resource_mut::<AdminOptions>()
        .skip_countdown = false;
    start_time_trial(&mut app);
    for _ in 0..3 {
        app.update();
    }
    // At the start: the record and its ghost announced.
    assert_eq!(trial_text(&mut app, TrialText::Record), "RECORD 0:10.00");
    assert_eq!(
        trial_text(&mut app, TrialText::Verdict),
        "LE FANTOME DU RECORD COURT AVEC TOI"
    );
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<GhostVisual>>()
        .single(world)
        .unwrap();
    let all = descendants(world, root);
    let pivot = all
        .iter()
        .find_map(|e| world.get::<KartBodyPivot>(*e))
        .unwrap();
    assert_eq!(pivot.looks, roster.looks(&theirs), "its parts");
    let driver = all
        .iter()
        .find_map(|e| world.get::<DriverVisual>(*e))
        .copied()
        .unwrap();
    assert_eq!(driver.character, Some(theirs.character), "its animal");
    let size = world
        .resource::<DriverLooks>()
        .get(theirs.character)
        .unwrap()
        .size;
    assert_eq!(driver.size, size);
    let paint = all.iter().find_map(|e| world.get::<BodyPaint>(*e)).unwrap();
    assert_eq!(paint.shown, color, "its color");

    // Translucent copies, made once.
    let materials_before = world.resource::<Assets<StandardMaterial>>().len();
    let copies = world.resource::<GhostMaterials>().len();
    assert!(copies > 0);
    for _ in 0..120 {
        app.update();
    }
    let world = app.world_mut();
    assert_eq!(
        world.resource::<Assets<StandardMaterial>>().len(),
        materials_before
    );
    assert_eq!(world.resource::<GhostMaterials>().len(), copies);
    let cache = world.resource::<GhostMaterials>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut meshes = 0;
    for entity in descendants(world, root) {
        let Some(material) = world.get::<MeshMaterial3d<StandardMaterial>>(entity) else {
            continue;
        };
        meshes += 1;
        assert!(cache.is_ghost(material.0.id()), "{entity}");
        let material = materials.get(material.0.id()).unwrap();
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        assert!(material.base_color.alpha() <= GHOST_ALPHA + 1e-6);
        assert!(world.get::<bevy::light::NotShadowCaster>(entity).is_some());
    }
    assert!(meshes > 10, "{meshes} meshes");
    // The player's own kart is not translucent.
    let kart = the_kart(&mut app);
    let world = app.world_mut();
    let visual = world
        .query::<(Entity, &crate::interpolation::InterpolatedFrom)>()
        .iter(world)
        .find(|(_, from)| from.0 == kart)
        .map(|(e, _)| e)
        .unwrap();
    for entity in descendants(world, visual) {
        if let Some(material) = world.get::<MeshMaterial3d<StandardMaterial>>(entity) {
            assert!(!world.resource::<GhostMaterials>().is_ghost(material.0.id()));
        }
    }

    // « Relancer » (the admin panel, a new kart): a new run against the same ghost.
    let run = app.world().resource::<TimeTrialState>().run.number;
    app.world_mut()
        .write_message(crate::level::LoadLevel::restart(RaceMode::Race));
    wait_for_the_level(&mut app, "Prairie Pompon");
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<TimeTrialState>().run.number, run + 1);
    assert_eq!(count::<GhostKart>(&mut app), 1);
    assert_eq!(count::<GhostVisual>(&mut app), 1);
    let kart = the_kart(&mut app);
    assert_eq!(
        app.world().get::<ItemSlots>(kart).unwrap().hand,
        Slot::Ready {
            item: ItemKind::Piment,
            count: 3
        }
    );
    // Ending the time trial takes the ghost away with the level.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        GameMode::FreeRace
    );
    tap(&mut app, KeyCode::Enter, Key::Enter);
    wait_for_the_level(&mut app, "Prairie Pompon");
    app.update();
    assert_eq!(count::<GhostKart>(&mut app), 0);
    assert_eq!(count::<GhostVisual>(&mut app), 0);
}

/// Issue #76: an unreadable stored text gives no record (the game warns in the console).
#[test]
fn an_unreadable_stored_text_gives_no_record() {
    assert!(records_from_text("pas du RON").is_empty());
    assert!(records_from_text("(format: 7, records: [])").is_empty());
    let mut records = TrialRecords::default();
    records.insert(TrialRecord {
        key: TrialKey {
            circuit: "tracks/col.track.ron".into(),
            class: crate::kart::SpeedClass::Cc50,
            mirror: true,
        },
        time: 9000,
        laps: vec![3000; 3],
        game: GAME_VERSION.into(),
        ghost: None,
    });
    assert_eq!(records_from_text(&records.to_ron()), records);
}
