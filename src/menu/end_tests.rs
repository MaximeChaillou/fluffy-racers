//! Issue #136: headless scenarios of the end screens' buttons (`end.rs`) and of the screens
//! without their lines of keys: the whole game (`GamePlugin`) with `MinimalPlugins`, one fixed
//! tick per `app.update()`, [`MainMenuPlugin`] added by hand (the game only adds it with a
//! renderer). Quick races: one lap, no countdown, no item nor interaction, the player's kart
//! driven by a bot (the race ends once it finished). No picking headless: the buttons are chosen
//! with the keys and the gamepad (their observers set the same focus and choice).

use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

use super::end::{END_INPUT_DELAY, EndScreen};
use super::*;
use crate::admin::AdminOptions;
use crate::admin::tracks;
use crate::bots::{BotDifficulty, BotTuning, bot, bot_seed};
use crate::characters::SelectionAction;
use crate::interactions::InteractionTuning;
use crate::items::ItemTuning;
use crate::kart::PlayerKart;
use crate::modes::{RACE_MENU_KEY, TimeTrial, TimeTrialPlugin, cup_of};
use crate::race::{GrandPrix, GrandPrixStage, Race, RaceGrid, RacePhase, RaceTuning, Racer};
use crate::tests::{game_app_unloaded, single_threaded, wait_for_the_player};
use crate::track::{TrackChoice, TrackLayout};
use crate::ui::format::names_a_key;

/// The test's gamepad.
#[derive(Resource)]
struct Pad(Entity);

/// The game on Prairie Pompon, alone, quick races, with its screens (and with `time_trial`, the
/// time trial's plugin, without its storage), once the player's kart exists.
fn quick_game(time_trial: bool) -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.world_mut().resource_mut::<RaceTuning>().laps = 1;
    app.world_mut().resource_mut::<ItemTuning>().enabled = false;
    app.world_mut().resource_mut::<InteractionTuning>().enabled = false;
    app.world_mut()
        .resource_mut::<AdminOptions>()
        .skip_countdown = true;
    app.add_plugins(MainMenuPlugin { at_launch: false });
    if time_trial {
        app.add_plugins(TimeTrialPlugin { storage: false });
    }
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let pad = app
        .world_mut()
        .spawn((Gamepad::default(), GamepadSettings::default()))
        .id();
    app.insert_resource(Pad(pad));
    app
}

fn press(app: &mut App, key_code: KeyCode, state: KeyState) {
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

/// A key pressed (one frame) then released (the next).
fn tap(app: &mut App, key_code: KeyCode) {
    press(app, key_code, KeyState::Pressed);
    press(app, key_code, KeyState::Released);
}

/// A button of the gamepad pressed (one frame) then released (the next).
fn pad(app: &mut App, button: GamepadButton) {
    let pad = app.world().resource::<Pad>().0;
    for value in [1.0, 0.0] {
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad, button, value,
            )));
        app.update();
    }
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// The player's kart is driven by a bot (the race ends once it finished).
fn player_becomes_a_bot(app: &mut App) {
    let world = app.world_mut();
    let kart = world
        .query_filtered::<Entity, With<PlayerKart>>()
        .single(world)
        .unwrap();
    let slot = world.get::<Racer>(kart).unwrap().grid_slot;
    let seed = bot_seed(world.resource::<RaceGrid>().seed, slot);
    let tuning = world.resource::<BotTuning>().clone();
    world
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(BotDifficulty::Hard, seed, &tuning));
}

/// Runs updates until the level on the circuit `name` is built, with its race and its racer.
fn wait_for_the_level(app: &mut App, name: &str) {
    for _ in 0..3000 {
        app.update();
        let world = app.world_mut();
        let built = world
            .get_resource::<TrackLayout>()
            .is_some_and(|layout| layout.name == name);
        let racers = world
            .query_filtered::<(), With<Racer>>()
            .iter(world)
            .count();
        if built && racers == 1 && world.contains_resource::<Race>() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the level on {name} was never built");
}

/// Runs the race until the end screen `screen` shows its buttons and takes the keys
/// ([`END_INPUT_DELAY`], real time, after it showed).
fn to_the_end(app: &mut App, screen: EndScreen) {
    for _ in 0..(300 * 60) {
        app.update();
        let Some(menu) = app.world().get_resource::<EndMenu>() else {
            continue;
        };
        if menu.screen != Some(screen) || menu.usable().is_empty() {
            continue;
        }
        let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
        if now - menu.shown_at >= END_INPUT_DELAY {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let shown = app.world().get_resource::<EndMenu>().and_then(|m| m.screen);
    panic!("the end screen {screen:?} never came ({shown:?})");
}

/// Runs updates until the home's background races (its circuit loads first).
fn wait_for_the_home(app: &mut App) {
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<Showcase>()
            .is_some_and(|showcase| showcase.running())
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the home's background never raced");
}

fn end(app: &App) -> &EndMenu {
    app.world().resource::<EndMenu>()
}

fn focus(app: &App) -> Option<usize> {
    end(app).focus
}

/// The end screen's buttons, as written: (label, small line), in their order.
fn buttons(app: &mut App) -> Vec<(String, String)> {
    let world = app.world_mut();
    let mut texts: Vec<(usize, bool, String)> = world
        .query::<(&EndText, &Text)>()
        .iter(world)
        .map(|(text, written)| (text.index, text.caption, written.0.clone()))
        .collect();
    texts.sort_by_key(|(index, caption, _)| (*index, *caption));
    texts
        .chunks(2)
        .map(|pair| (pair[0].2.clone(), pair[1].2.clone()))
        .collect()
}

fn labels(app: &mut App) -> Vec<String> {
    buttons(app).into_iter().map(|(label, _)| label).collect()
}

/// Issue #136: every text of the screens shown, but the options' list of commands (the one place
/// the keys are named): none is a line of keys.
pub(crate) fn assert_no_line_of_keys(app: &mut App) {
    let world = app.world_mut();
    let commands: Vec<Entity> = world
        .query_filtered::<Entity, With<ControlLine>>()
        .iter(world)
        .collect();
    let mut parents = world.query::<&ChildOf>();
    let mut texts = world.query::<(Entity, &Text)>();
    let shown: Vec<(Entity, String)> = texts
        .iter(world)
        .map(|(entity, text)| (entity, text.0.clone()))
        .collect();
    let mut checked = 0;
    for (entity, text) in shown {
        let mut up = Some(entity);
        let mut in_commands = false;
        while let Some(at) = up {
            if commands.contains(&at) {
                in_commands = true;
                break;
            }
            up = parents.get(world, at).ok().map(ChildOf::parent);
        }
        if in_commands {
            continue;
        }
        checked += 1;
        assert!(!names_a_key(&text), "a line of keys: « {text} »");
    }
    assert!(checked > 0, "no text at all");
}

/// Issue #136 acceptance, a circuit alone: its results have three buttons (« Rejouer » has the
/// focus) and no line of keys; « Changer de circuit » (Droite, Entrée) opens « Jouer solo » (the
/// game waits), Échap comes back to the results and their buttons; « Rejouer » (the gamepad:
/// Gauche, A) races again from the grid; then « Accueil » (Gauche, around to the last; Entrée)
/// goes home, its background replacing the race.
#[test]
fn the_results_race_again_change_the_circuit_and_go_home() {
    let mut app = quick_game(false);
    player_becomes_a_bot(&mut app);
    to_the_end(&mut app, EndScreen::Race);
    assert_eq!(
        buttons(&mut app),
        [
            (
                "Rejouer".to_string(),
                "La même course, chrono à zéro.".to_string()
            ),
            (
                "Changer de circuit".to_string(),
                "Un autre circuit, un autre mode.".to_string()
            ),
            ("Accueil".to_string(), "Retour à l'accueil.".to_string()),
        ]
    );
    assert_eq!(focus(&app), Some(0), "the one that goes on");
    assert_eq!(end(&app).usable(), [true, true, true]);
    assert_no_line_of_keys(&mut app);

    // « Changer de circuit »: « Jouer solo », the game waiting; Échap: the results again.
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(focus(&app), Some(1));
    tap(&mut app, KeyCode::Enter);
    assert!(app.world().resource::<RaceMenu>().open, "« Jouer solo »");
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert!(!app.world().resource::<RaceMenu>().open);
    assert!(!menu_open(&app), "Échap closed « Jouer solo », not more");
    assert_eq!(
        end(&app).screen,
        Some(EndScreen::Race),
        "back on the results"
    );
    assert_eq!(labels(&mut app).len(), 3);

    // « Rejouer », with the gamepad: the race again, from its grid.
    pad(&mut app, GamepadButton::DPadLeft);
    assert_eq!(focus(&app), Some(0));
    pad(&mut app, GamepadButton::South);
    run(&mut app, 2);
    let race = app.world().resource::<Race>();
    assert_ne!(race.phase(), RacePhase::Results, "raced again");
    assert!(race.race_ticks() < 30, "from its grid");
    assert!(
        labels(&mut app).is_empty(),
        "the buttons went with the results"
    );

    // « Accueil », with the keys: Gauche goes around to the last one.
    to_the_end(&mut app, EndScreen::Race);
    assert_eq!(
        focus(&app),
        Some(0),
        "the screen shows again: on « Rejouer »"
    );
    tap(&mut app, KeyCode::ArrowLeft);
    assert_eq!(focus(&app), Some(2));
    tap(&mut app, KeyCode::Enter);
    let menu = *app.world().resource::<MainMenu>();
    assert!(menu.open && !menu.in_game, "the home screen");
    wait_for_the_home(&mut app);
    assert!(labels(&mut app).is_empty());
    assert_eq!(end(&app).screen, None);
}

fn menu_open(app: &App) -> bool {
    app.world().resource::<MainMenu>().open
}

fn paused(app: &App) -> bool {
    app.world().resource::<Time<Virtual>>().is_paused()
}

/// The lobby's panel shown: its view (`salon::view`), once its tree is there.
fn lobby_panel(app: &mut App) -> Option<crate::salon::SalonView> {
    if count::<With<crate::salon::SalonScreenRoot>>(app) == 0 {
        return None;
    }
    app.world()
        .get_resource::<crate::salon::SalonMenu>()
        .and_then(|menu| menu.view.clone())
}

/// Issue #141: out of a room, L shows the lobby's panel (« Multijoueur ») over a race, as the
/// charter has it (« leurs raccourcis (N, L, G) les ouvrent toujours par-dessus »; it showed
/// nothing: no race let it show), and the game waits under it, as under the other screens (its
/// keys are the panel's); Échap closes it, and the race goes on, without the pause. Over the
/// pause, L takes its place, and Échap brings the pause back. Over the race's results too (out
/// of a room, nothing comes after them): the panel over them, their buttons holding back; Échap,
/// and they are back.
#[test]
fn l_shows_the_lobby_s_panel_over_a_solo_race_and_its_results() {
    let mut app = quick_game(false);
    run(&mut app, 30);
    let tick = |app: &App| app.world().resource::<Race>().flow.tick;
    assert_eq!(app.world().resource::<Race>().phase(), RacePhase::Racing);
    assert!(!paused(&app));

    // L in the race: the panel, out of a room; the race waits.
    tap(&mut app, crate::net::LOBBY_KEY);
    run(&mut app, 3);
    let view = lobby_panel(&mut app).expect("the lobby's panel over the race");
    assert_eq!(view.title, "Multijoueur");
    let kinds: Vec<_> = view.buttons.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            crate::salon::ButtonKind::CreateRoom,
            crate::salon::ButtonKind::Back
        ]
    );
    assert!(paused(&app), "the game waits under it, as under « Pilote »");
    let at = tick(&app);
    run(&mut app, 30);
    assert_eq!(tick(&app), at, "the race waits");
    assert!(!menu_open(&app));
    assert_eq!(count::<With<crate::salon::SalonBanner>>(&mut app), 0);

    // Échap: back to the race, which goes on; no pause.
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert!(lobby_panel(&mut app).is_none());
    assert!(!app.world().resource::<LobbyWindow>().open);
    assert!(!menu_open(&app), "Échap closed the panel, and that is all");
    assert!(!paused(&app));
    let at = tick(&app);
    run(&mut app, 10);
    assert!(tick(&app) > at, "the race goes on");

    // Over the pause: L takes its place (the game still waits); Échap, the pause again.
    tap(&mut app, KeyCode::Escape);
    assert!(menu_open(&app) && paused(&app));
    tap(&mut app, crate::net::LOBBY_KEY);
    run(&mut app, 3);
    assert!(lobby_panel(&mut app).is_some());
    assert!(!menu_open(&app), "one screen at a time");
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert!(lobby_panel(&mut app).is_none());
    assert!(menu_open(&app), "back to the pause");
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert!(!menu_open(&app) && !paused(&app));

    // The results: L shows the panel over them; their buttons hold back (Droite moves the
    // panel's focus, not theirs); Échap, the results and their buttons again.
    player_becomes_a_bot(&mut app);
    to_the_end(&mut app, EndScreen::Race);
    assert_eq!(focus(&app), Some(0));
    tap(&mut app, crate::net::LOBBY_KEY);
    run(&mut app, 3);
    assert!(
        lobby_panel(&mut app).is_some(),
        "over the results, out of a room"
    );
    assert!(paused(&app));
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(focus(&app), Some(0), "the panel has the keys");
    assert_eq!(end(&app).screen, Some(EndScreen::Race));
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert!(lobby_panel(&mut app).is_none());
    assert!(!menu_open(&app) && !paused(&app));
    assert_eq!(end(&app).screen, Some(EndScreen::Race), "the results again");
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(focus(&app), Some(1), "their buttons have the keys again");
}

/// Issue #136 acceptance, the time trial: « Réessayer » (Entrée) races the circuit again (still
/// a time trial), « Changer de circuit » is there, « Accueil » (the gamepad: Gauche, A) goes home:
/// the time trial is over.
#[test]
fn the_time_trial_tries_again_and_goes_home() {
    let mut app = quick_game(true);
    for key in [RACE_MENU_KEY, KeyCode::ArrowRight, KeyCode::ArrowRight] {
        tap(&mut app, key);
    }
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        crate::modes::GameMode::TimeTrial
    );
    tap(&mut app, KeyCode::Enter);
    wait_for_the_level(&mut app, "Prairie Pompon");
    assert!(app.world().contains_resource::<TimeTrial>());
    player_becomes_a_bot(&mut app);
    to_the_end(&mut app, EndScreen::TimeTrial);
    assert_eq!(
        labels(&mut app),
        ["Réessayer", "Changer de circuit", "Accueil"]
    );
    assert_no_line_of_keys(&mut app);

    // « Réessayer »: the circuit again, still against the clock.
    tap(&mut app, KeyCode::Enter);
    run(&mut app, 2);
    assert_ne!(app.world().resource::<Race>().phase(), RacePhase::Results);
    assert!(app.world().contains_resource::<TimeTrial>());
    assert!(labels(&mut app).is_empty());

    // « Accueil », with the gamepad: home, the time trial over.
    to_the_end(&mut app, EndScreen::TimeTrial);
    pad(&mut app, GamepadButton::DPadLeft);
    assert_eq!(focus(&app), Some(2));
    pad(&mut app, GamepadButton::South);
    wait_for_the_home(&mut app);
    assert!(!app.world().contains_resource::<TimeTrial>());
    assert!(!app.world().resource::<MainMenu>().in_game);
}

/// Issue #136 acceptance, a championship of two races: its results: « Classement général »
/// (Entrée) and « Abandonner »; its standings: « Course suivante » (its circuit under it; the
/// gamepad's A) and « Abandonner »; after the last race, « Le podium » (Entrée); the podium:
/// « Rejouer le championnat » (Entrée: the same races from the first, no points) and « Accueil »;
/// then « Abandonner » (Droite, Entrée) from the results of a race: home, the championship over.
#[test]
fn the_championship_goes_on_to_its_podium_plays_again_and_gives_up() {
    let mut app = quick_game(false);
    let circuits = tracks::circuits();
    let cup = cup_of(&circuits, 2);
    let (first, second) = (cup[0].clone(), cup[1].clone());
    app.world_mut().insert_resource(GrandPrix::new(cup, 0));
    app.world_mut().write_message(crate::level::LoadLevel::on(
        TrackChoice::Circuit(first.path.clone()),
        RaceMode::Race,
    ));
    wait_for_the_level(&mut app, &first.name);
    player_becomes_a_bot(&mut app);

    to_the_end(&mut app, EndScreen::ChampionshipRace);
    assert_eq!(
        buttons(&mut app),
        [
            (
                "Classement général".to_string(),
                "Les points de cette course.".to_string()
            ),
            (
                "Abandonner".to_string(),
                "Le championnat s'arrête là.".to_string()
            ),
        ]
    );
    assert_no_line_of_keys(&mut app);
    tap(&mut app, KeyCode::Enter);
    to_the_end(&mut app, EndScreen::Standings { last: false });
    assert_eq!(
        buttons(&mut app)[0],
        (
            "Course suivante".to_string(),
            format!("Course 2/2 : {}", second.name)
        )
    );
    assert_eq!(labels(&mut app)[1], "Abandonner");
    assert_no_line_of_keys(&mut app);

    // « Course suivante », with the gamepad.
    pad(&mut app, GamepadButton::South);
    wait_for_the_level(&mut app, &second.name);
    assert_eq!(app.world().resource::<GrandPrix>().race, 1);
    player_becomes_a_bot(&mut app);
    to_the_end(&mut app, EndScreen::ChampionshipRace);
    tap(&mut app, KeyCode::Enter);
    to_the_end(&mut app, EndScreen::Standings { last: true });
    assert_eq!(labels(&mut app), ["Le podium", "Abandonner"]);
    tap(&mut app, KeyCode::Enter);
    to_the_end(&mut app, EndScreen::Podium);
    assert_eq!(
        app.world().resource::<GrandPrix>().stage,
        GrandPrixStage::Podium
    );
    assert_eq!(labels(&mut app), ["Rejouer le championnat", "Accueil"]);
    assert_no_line_of_keys(&mut app);

    // « Rejouer le championnat »: the same races from the first, no points yet.
    tap(&mut app, KeyCode::Enter);
    wait_for_the_level(&mut app, &first.name);
    let grand_prix = app.world().resource::<GrandPrix>();
    assert_eq!(
        (grand_prix.race, grand_prix.stage),
        (0, GrandPrixStage::Racing)
    );
    assert!(grand_prix.points.iter().all(|points| *points == 0));

    // « Abandonner », from the results of its first race: home, the championship over.
    player_becomes_a_bot(&mut app);
    to_the_end(&mut app, EndScreen::ChampionshipRace);
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(focus(&app), Some(1));
    tap(&mut app, KeyCode::Enter);
    wait_for_the_home(&mut app);
    assert!(!app.world().contains_resource::<GrandPrix>());
}

/// Issue #136 acceptance: no screen writes a line of keys any more (Max: « Sur l'accueil enlève
/// les indications de "comment utiliser un menu" »): the home screen, « Jouer solo », the options
/// (but their list of commands), « Pilote » (its pseudo typed too: « 12 lettres au plus », without
/// keys), the lobby without a room, the pause. The end screens are checked with their buttons
/// above, the lobby in a room in `net::end_tests`.
#[test]
fn no_screen_writes_a_line_of_keys() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(MainMenuPlugin { at_launch: false });
    // Bevy's focus, which the pseudo's field takes while one types (the game's input plugins).
    app.init_resource::<bevy::input_focus::InputFocus>();
    wait_for_the_player(&mut app);
    // The lobby out of a room first, off a race (it showed over none before #141; the test
    // ground keeps the race's screens out of the count).
    app.world_mut().write_message(crate::level::LoadLevel::on(
        TrackChoice::TestGround,
        RaceMode::FreeRoam,
    ));
    for _ in 0..600 {
        app.update();
        if !app.world().contains_resource::<Race>() {
            break;
        }
    }
    assert!(!app.world().contains_resource::<Race>());
    let screens: [(&str, fn(&mut App)); 6] = [
        ("Multijoueur", |app| {
            app.world_mut().resource_mut::<LobbyWindow>().open = true;
        }),
        ("home", |app| {
            app.world_mut().write_message(MainMenuAction::OpenAtLaunch);
        }),
        ("Jouer solo", |app| {
            app.world_mut().write_message(RaceMenuAction::Toggle);
        }),
        ("options", |app| {
            app.world_mut().write_message(OptionsAction::Toggle);
        }),
        ("Pilote", |app| {
            app.world_mut().write_message(SelectionAction::Open);
        }),
        ("pause", |app| {
            app.world_mut().write_message(MainMenuAction::Open);
        }),
    ];
    for (name, open) in screens {
        open(&mut app);
        run(&mut app, 4);
        let roots = count::<With<MainMenuRoot>>(&mut app)
            + count::<With<SoloRoot>>(&mut app)
            + count::<With<OptionsRoot>>(&mut app)
            + count::<With<PilotRoot>>(&mut app)
            + count::<With<crate::salon::SalonScreenRoot>>(&mut app);
        assert_eq!(roots, 1, "{name}: its screen");
        assert_no_line_of_keys(&mut app);
        if name == "Pilote" {
            // Its pseudo typed: its help, without keys.
            app.world_mut().resource_mut::<SelectionScreen>().typing = true;
            run(&mut app, 2);
            let help = app
                .world_mut()
                .query_filtered::<&Text, With<PilotHint>>()
                .single(app.world())
                .unwrap()
                .0
                .clone();
            assert!(help.contains("12 lettres au plus"), "{help}");
            assert_no_line_of_keys(&mut app);
            app.world_mut().resource_mut::<SelectionScreen>().typing = false;
        }
        // Closed: back to the game, no screen left.
        match name {
            // From home, a race at once (on its background's circuit); from the pause, back to
            // the race.
            "home" | "pause" => {
                app.world_mut().write_message(MainMenuAction::Close);
            }
            "Jouer solo" => {
                app.world_mut().write_message(RaceMenuAction::Close);
            }
            "options" => {
                app.world_mut().write_message(OptionsAction::Toggle);
            }
            "Pilote" => {
                app.world_mut().write_message(SelectionAction::Cancel);
            }
            _ => app.world_mut().resource_mut::<LobbyWindow>().open = false,
        }
        run(&mut app, 4);
    }
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}
