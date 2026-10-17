//! Headless scenarios of the game modes (issue #19): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, one fixed tick per `app.update()`. The race menu through its keys, a whole
//! Grand Prix of four races (one lap each, the player's kart driven by a bot) through the keys of
//! its screens, to the podium and a new Grand Prix.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use super::*;
use crate::admin::AdminOptions;
use crate::bots::{BotDifficulty, BotTuning, bot, bot_seed};
use crate::camera::FixedView;
use crate::characters::PlayerColor;
use crate::interactions::InteractionTuning;
use crate::items::ItemTuning;
use crate::kart::{
    ColorSet, KartPaint, KartWheels, MaxSpeedFactors, PlayerKart, SpeedClassTuning, bot_colors,
};
use crate::race::{GrandPrixStage, OnPodium, Parked, RaceTuning, RacerName, ResultTime};
use crate::tests::{game_app_unloaded, single_threaded, wait_for_the_player};
use crate::track::TrackLayout;
use crate::ui::{HudRoot, ResultsScreen};

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

fn enter(app: &mut App) {
    tap(app, KeyCode::Enter, Key::Enter);
}

/// The game with `bots` bots on Prairie Pompon, quick races (1 lap, no countdown, no items nor
/// interactions: the bots finish sooner), once the player's kart exists.
fn quick_game(bots: usize) -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), bots);
    app.world_mut().resource_mut::<RaceTuning>().laps = 1;
    app.world_mut().resource_mut::<ItemTuning>().enabled = false;
    app.world_mut().resource_mut::<InteractionTuning>().enabled = false;
    app.world_mut()
        .resource_mut::<AdminOptions>()
        .skip_countdown = true;
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    app
}

/// Runs updates until the level on the circuit `name` is built, with its race and the player's
/// kart (circuit files load in the background).
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

fn grand_prix(app: &App) -> GrandPrix {
    app.world().resource::<GrandPrix>().clone()
}

/// The player's kart is driven by a bot (the race ends once everybody finished).
fn player_becomes_a_bot(app: &mut App) -> Entity {
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
    kart
}

/// Runs the race until its results screen lets the player move on.
fn race_to_the_results(app: &mut App) {
    for _ in 0..(200 * 60) {
        app.update();
        let race = app.world().resource::<Race>();
        if race.phase() == RacePhase::Results && race.flow.can_restart(&race.timings) {
            return;
        }
    }
    panic!("the race never ended");
}

/// Every racer of the level: (entity, grid slot).
fn racers(app: &mut App) -> Vec<(Entity, usize)> {
    let world = app.world_mut();
    let mut all: Vec<(Entity, usize)> = world
        .query::<(Entity, &Racer)>()
        .iter(world)
        .map(|(e, r)| (e, r.grid_slot))
        .collect();
    all.sort_by_key(|(_, slot)| *slot);
    all
}

/// Issue #19 acceptance, from end to end: the race menu (N) starts a Grand Prix in 200cc,
/// mirrored; it races the four circuits of the cup in order; each race's results give the points
/// of GD § 1 (Entrée closes them at once); Entrée shows the overall standings, then starts the
/// next race on its grid: the reverse of the standings, the leader last, each bot the same bot
/// (its color and its name follow it); after the fourth race, the podium: the three first on
/// their steps, dancing, every kart parked, a still camera; Entrée starts a new Grand Prix.
#[test]
fn a_grand_prix_races_the_cup_scores_it_and_ends_on_the_podium() {
    let bots = 3;
    let mut app = quick_game(bots);
    let circuits = tracks::circuits();

    // N, then the mode (Grand Prix), the class (200cc, two lines down) and the mirror.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert!(app.world().resource::<RaceMenu>().open);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    let menu = *app.world().resource::<RaceMenu>();
    assert_eq!(
        menu.setup,
        RaceSetup {
            mode: GameMode::GrandPrix,
            circuit: 0,
            class: SpeedClass::Cc200,
            mirror: true,
        }
    );
    enter(&mut app);
    assert!(!app.world().resource::<RaceMenu>().open);
    let cup = cup_of(&circuits, 4);
    assert_eq!(
        cup.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        circuits
            .iter()
            .take(4)
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
    let player_entrant = bots;
    let players: ColorSet = [PlayerColor::default().0].into_iter().collect();
    let colors = bot_colors(bots, players);
    let mut previous_standings: Option<Vec<usize>> = None;
    for (race, circuit) in cup.iter().enumerate() {
        wait_for_the_level(&mut app, &circuit.name);
        let gp = grand_prix(&app);
        assert_eq!((gp.race, gp.stage), (race, GrandPrixStage::Racing));
        assert_eq!(gp.cup, cup);
        assert_eq!(gp.entrants, bots + 1);
        // The class and the mirror hold for the whole Grand Prix.
        assert_eq!(app.world().resource::<RaceClass>().0, SpeedClass::Cc200);
        assert!(app.world().resource::<TrackLayout>().mirrored);
        let grid = *app.world().resource::<RaceGrid>();
        let all = racers(&mut app);
        assert_eq!(all.len(), bots + 1);
        // The grid: the first race as any race (the player last), then the reverse of the
        // standings.
        match &previous_standings {
            None => assert_eq!(grid.lineup, None),
            Some(standings) => {
                for (slot, entrant) in standings.iter().rev().enumerate() {
                    assert_eq!(grid.entrant_at(slot), *entrant, "race {race}, slot {slot}");
                }
            }
        }
        let player = player_becomes_a_bot(&mut app);
        // A tick of the karts' rules: the class's factor on the new karts.
        app.update();
        let world = app.world();
        let player_slot = world.get::<Racer>(player).unwrap().grid_slot;
        assert_eq!(grid.entrant_at(player_slot), player_entrant);
        let class = SpeedClassTuning::default().speed_200cc;
        for (kart, slot) in &all {
            let factors = world.get::<MaxSpeedFactors>(*kart).unwrap();
            assert_eq!(factors.class, class, "race {race}, slot {slot}");
            let entrant = grid.entrant_at(*slot);
            if entrant == player_entrant {
                continue;
            }
            // Each bot is its entrant: its color, its name.
            assert_eq!(world.get::<KartPaint>(*kart).unwrap().0, colors[entrant]);
            let name = world.get::<RacerName>(*kart).map(|n| n.0.clone());
            if *slot != entrant {
                assert_eq!(name, Some(format!("Kart {}", entrant + 1)));
            }
        }

        race_to_the_results(&mut app);
        // Entrée: the results close at once, the race is scored, the standings come.
        enter(&mut app);
        for _ in 0..5 {
            app.update();
        }
        let gp = grand_prix(&app);
        assert_eq!(gp.stage, GrandPrixStage::Standings, "race {race}");
        assert_eq!(gp.orders.len(), race + 1);
        // Its order is the closed results', as entrants; every one of them got its points.
        let results = app.world().resource::<Race>().results.clone();
        assert_eq!(results.len(), bots + 1);
        assert!(
            results
                .iter()
                .all(|line| matches!(line.time, ResultTime::Finished(_)))
        );
        let order: Vec<usize> = results
            .iter()
            .map(|line| grid.entrant_at(app.world().get::<Racer>(line.id).unwrap().grid_slot))
            .collect();
        assert_eq!(gp.orders[race], order);
        let points = GrandPrixTuning::default().points;
        for (place, entrant) in order.iter().enumerate() {
            assert_eq!(gp.last_points[*entrant], points[place]);
        }
        assert_eq!(
            gp.points.iter().sum::<u32>(),
            (race as u32 + 1) * points[..bots + 1].iter().sum::<u32>()
        );
        let world = app.world_mut();
        let screens: Vec<GrandPrixScreen> = world
            .query::<&GrandPrixScreen>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(screens, [GrandPrixScreen(GrandPrixStage::Standings)]);
        // The standings take the race's results' place.
        let results_screens = world.query::<&ResultsScreen>().iter(world).count();
        assert_eq!(results_screens, 0);
        let lines = standings_lines(&gp);
        assert_eq!(lines.len(), bots + 1);
        assert_eq!(lines[0].rank, "1er");
        assert_eq!(lines.iter().filter(|l| l.player).count(), 1);
        previous_standings = Some(gp.standings());

        // Entrée: the next race, or the podium.
        enter(&mut app);
        if race + 1 < cup.len() {
            assert_eq!(grand_prix(&app).race, race + 1);
        }
    }

    // The podium.
    for _ in 0..30 {
        app.update();
    }
    let gp = grand_prix(&app);
    assert_eq!(gp.stage, GrandPrixStage::Podium);
    let standings = gp.standings();
    let layout = app.world().resource::<TrackLayout>().clone();
    assert!(app.world().contains_resource::<FixedView>());
    let grid = *app.world().resource::<RaceGrid>();
    let all = racers(&mut app);
    let world = app.world();
    for (kart, slot) in &all {
        let entrant = grid.entrant_at(*slot);
        assert!(world.get::<Parked>(*kart).is_some(), "every kart parked");
        let place = standings.iter().position(|e| *e == entrant).unwrap();
        let podium = world.get::<OnPodium>(*kart).copied();
        if place < 3 {
            assert_eq!(podium, Some(OnPodium(place)));
            let spot = podium_place(&layout, place);
            let at = world.get::<Transform>(*kart).unwrap().translation;
            assert!(
                at.distance(spot.translation) < 0.3,
                "{place}: {at} vs {spot:?}"
            );
            assert!(world.get::<KartWheels>(*kart).unwrap().wheels_on_ground() >= 3);
        } else {
            assert_eq!(podium, None);
        }
    }
    let world = app.world_mut();
    let screens: Vec<GrandPrixScreen> = world
        .query::<&GrandPrixScreen>()
        .iter(world)
        .copied()
        .collect();
    assert_eq!(screens, [GrandPrixScreen(GrandPrixStage::Podium)]);
    assert_eq!(world.query::<&PodiumScene>().iter(world).count(), 1);
    // Only the podium's names: no results, no race HUD.
    assert_eq!(world.query::<&ResultsScreen>().iter(world).count(), 0);
    let hud: Vec<Visibility> = world
        .query_filtered::<&Visibility, With<HudRoot>>()
        .iter(world)
        .copied()
        .collect();
    assert_eq!(hud, [Visibility::Hidden]);
    // No result screen key restarts the last race: Entrée starts a new Grand Prix.
    let restarted = app.world().resource::<Race>().restart_requested;
    assert!(!restarted);
    enter(&mut app);
    wait_for_the_level(&mut app, &cup[0].name);
    let gp = grand_prix(&app);
    assert_eq!((gp.race, gp.stage), (0, GrandPrixStage::Racing));
    assert!(gp.points.iter().all(|p| *p == 0));
    assert_eq!(app.world().resource::<RaceGrid>().lineup, None);
    app.update();
    assert!(
        !app.world().contains_resource::<FixedView>(),
        "the chase again"
    );
    let world = app.world_mut();
    assert_eq!(world.query::<&PodiumScene>().iter(world).count(), 0);
}

/// Issue #19: the race menu starts a free race on the circuit chosen, in its class, and ends a
/// Grand Prix in progress (its grid order with it); Échap closes it unchanged; online, N does
/// nothing (the room chooses).
#[test]
fn the_race_menu_starts_a_free_race_and_ends_a_grand_prix() {
    let mut app = quick_game(2);
    let circuits = tracks::circuits();
    let col = circuits
        .iter()
        .position(|c| c.name == "Col des Moufles")
        .unwrap();
    app.insert_resource(GrandPrix::new(cup_of(&circuits, 4), 2));
    app.world_mut().resource_mut::<RaceGrid>().lineup = Some(std::array::from_fn(|i| {
        [2, 1, 0][..].get(i).copied().unwrap_or(i as u8)
    }));

    // Échap: nothing changes.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert_eq!(
        app.world().resource::<RaceMenu>().setup.mode,
        GameMode::GrandPrix
    );
    tap(&mut app, KeyCode::Escape, Key::Escape);
    assert!(!app.world().resource::<RaceMenu>().open);
    assert!(app.world().contains_resource::<GrandPrix>());

    // Free race, Col des Moufles, 50cc, not mirrored.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    for _ in 0..col {
        tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    }
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    assert_eq!(
        app.world().resource::<RaceMenu>().setup,
        RaceSetup {
            mode: GameMode::FreeRace,
            circuit: col,
            class: SpeedClass::Cc50,
            mirror: false,
        }
    );
    enter(&mut app);
    wait_for_the_level(&mut app, "Col des Moufles");
    assert!(!app.world().contains_resource::<GrandPrix>());
    assert_eq!(app.world().resource::<RaceGrid>().lineup, None);
    assert_eq!(app.world().resource::<RaceClass>().0, SpeedClass::Cc50);
    assert!(!app.world().resource::<TrackLayout>().mirrored);
    let world = app.world_mut();
    let factors: Vec<f32> = world
        .query::<&MaxSpeedFactors>()
        .iter(world)
        .map(|f| f.class)
        .collect();
    app.update();
    let world = app.world_mut();
    let after: Vec<f32> = world
        .query::<&MaxSpeedFactors>()
        .iter(world)
        .map(|f| f.class)
        .collect();
    assert_eq!(after.len(), 3);
    assert!(
        after.iter().all(|f| *f == 0.75),
        "{factors:?} then {after:?}"
    );

    // Online, N does nothing.
    app.insert_resource(crate::level::Online);
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert!(!app.world().resource::<RaceMenu>().open);
}

/// The lines of the race menu: the mode (issue #76: free race, Grand Prix, time trial), the
/// circuit (a free race or a time trial: a Grand Prix races the cup), the class and the mirror,
/// each one around.
#[test]
fn the_menu_lines_go_around() {
    let circuits = tracks::circuits();
    let n = circuits.len();
    let setup = RaceSetup::default();
    assert_eq!(setup.class, SpeedClass::Cc150);
    assert_eq!(setup.mode, GameMode::FreeRace);
    let gp = setup.changed(MenuRow::Mode, 1, n);
    assert_eq!(gp.mode, GameMode::GrandPrix);
    let trial = gp.changed(MenuRow::Mode, 1, n);
    assert_eq!(trial.mode, GameMode::TimeTrial);
    assert_eq!(trial.changed(MenuRow::Mode, 1, n), setup, "around");
    assert_eq!(setup.changed(MenuRow::Mode, -1, n), trial);
    assert_eq!(GameMode::TimeTrial.label(), "Contre-la-montre");
    assert_eq!(setup.changed(MenuRow::Circuit, -1, n).circuit, n - 1);
    assert_eq!(gp.changed(MenuRow::Circuit, 1, n).circuit, 0, "the cup");
    assert_eq!(trial.changed(MenuRow::Circuit, 2, n).circuit, 2 % n);
    assert_eq!(
        trial
            .changed(MenuRow::Circuit, 1, n)
            .value(MenuRow::Circuit, &circuits, 4),
        circuits[1 % n].name
    );
    assert_eq!(setup.changed(MenuRow::Class, 1, n).class, SpeedClass::Cc200);
    assert!(setup.changed(MenuRow::Mirror, 1, n).mirror);
    assert!(!setup.changed(MenuRow::Mirror, 2, n).mirror);
    assert_eq!(
        setup.value(MenuRow::Circuit, &circuits, 4),
        "Prairie Pompon"
    );
    assert_eq!(
        gp.value(MenuRow::Circuit, &circuits, 4),
        "Prairie Pompon · Anneau Guimauve · Crique Câline · Col des Moufles"
    );
    assert_eq!(gp.value(MenuRow::Mirror, &circuits, 4), "Non");
    // The texts of the Grand Prix's screens are ASCII (the HUD's font).
    let mut grand_prix = GrandPrix::new(cup_of(&circuits, 4), 11);
    grand_prix.score(
        &(0..12).collect::<Vec<_>>(),
        &GrandPrixTuning::default().points,
    );
    let mut texts = vec![
        screens::standings_title(&grand_prix),
        screens::standings_footer(&grand_prix),
        screens::PODIUM_TITLE.to_string(),
        screens::PODIUM_FOOTER.to_string(),
    ];
    grand_prix.race = 2;
    texts.push(screens::standings_title(&grand_prix));
    texts.push(screens::standings_footer(&grand_prix));
    for line in standings_lines(&grand_prix) {
        texts.extend([line.rank, line.name, line.won, line.total]);
    }
    for text in texts {
        assert!(text.is_ascii(), "{text}");
    }
    assert_eq!(
        screens::standings_title(&grand_prix),
        "GRAND PRIX 3/4 - Crique Caline"
    );
}
