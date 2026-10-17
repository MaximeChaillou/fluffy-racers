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
use crate::kart::{ColorSet, KartPaint, KartWheels, PlayerKart, bot_colors};
use crate::race::{GrandPrixStage, OnPodium, Parked, RaceTuning, RacerName, ResultTime};
use crate::tests::{game_app_unloaded, single_threaded, wait_for_the_player};
use crate::track::{CroquetteSpot, ItemBoxSpot, TrackLayout};
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

/// Issue #19 acceptance, from end to end: the race menu (N) starts a Grand Prix, mirrored; it
/// races the four circuits of the cup in order; each race's results give the points
/// of GD § 1 (Entrée closes them at once); Entrée shows the overall standings, then starts the
/// next race on its grid: the reverse of the standings, the leader last, each bot the same bot
/// (its color and its name follow it); after the fourth race, the podium: the three first on
/// their steps, dancing, every kart parked, a still camera; Entrée starts a new Grand Prix.
#[test]
fn a_grand_prix_races_the_cup_scores_it_and_ends_on_the_podium() {
    let bots = 3;
    let mut app = quick_game(bots);
    let circuits = tracks::circuits();

    // N, then the mode (the championship, a cup: issue #99) and the mirror (three lines down:
    // the formula, the cup, the mirror; issue #118, no class).
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert!(app.world().resource::<RaceMenu>().open);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    for _ in 0..3 {
        tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    }
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    let menu = app.world().resource::<RaceMenu>().clone();
    assert_eq!(menu.row(), MenuRow::Mirror);
    assert_eq!(
        (menu.setup.mode, menu.setup.formula, menu.setup.cup),
        (GameMode::GrandPrix, Formula::Cup, 0)
    );
    assert!(menu.setup.mirror && menu.setup.bots && menu.setup.items);
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
        assert_eq!(gp.title, "Coupe Doudou", "issue #99: the cup's name");
        assert_eq!(gp.entrants, bots + 1);
        // The mirror holds for the whole Grand Prix.
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
        let world = app.world();
        let player_slot = world.get::<Racer>(player).unwrap().grid_slot;
        assert_eq!(grid.entrant_at(player_slot), player_entrant);
        for (kart, slot) in &all {
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

/// Issue #19: the race menu starts a free race on the circuit chosen, in its mirror mode, and ends
/// a Grand Prix in progress (its grid order with it); Échap closes it unchanged; online, N does
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

    // Free race, Col des Moufles, not mirrored (the mirror line, the last one, set and unset:
    // around).
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    for _ in 0..col {
        tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    }
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
    let menu = app.world().resource::<RaceMenu>().clone();
    assert_eq!(menu.row(), MenuRow::Mirror);
    assert_eq!(
        (menu.setup.mode, menu.setup.circuit, menu.setup.mirror),
        (GameMode::FreeRace, col, false)
    );
    enter(&mut app);
    wait_for_the_level(&mut app, "Col des Moufles");
    assert!(!app.world().contains_resource::<GrandPrix>());
    assert_eq!(app.world().resource::<RaceGrid>().lineup, None);
    assert!(!app.world().resource::<TrackLayout>().mirrored);

    // Online, N does nothing.
    app.insert_resource(crate::level::Online);
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert!(!app.world().resource::<RaceMenu>().open);
}

/// Issue #99: the standings say the cup's name (or « Championnat »). Issue #100: the texts of the
/// championship's screens are written in Fredoka, with their accents (they were ASCII, the old
/// font's): every character is one Fredoka draws, and the names are as written (« Crique
/// Câline », « Entrée »).
#[test]
fn the_screens_of_a_championship_write_their_accents() {
    let circuits = tracks::circuits();
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
    texts.push(screens::STANDINGS_HEADER.to_string());
    for text in &texts {
        assert!(text.chars().all(crate::ui::format::writable), "{text}");
    }
    grand_prix.title = "Championnat".into();
    assert_eq!(
        screens::standings_title(&grand_prix),
        "Championnat 3/4 · Crique Câline"
    );
    grand_prix.title = "Coupe Doudou".into();
    assert_eq!(
        screens::standings_title(&grand_prix),
        "Coupe Doudou 3/4 · Crique Câline"
    );
    assert_eq!(
        screens::standings_footer(&grand_prix),
        "Espace ou Entrée (manette : A) : course suivante (Col des Moufles)"
    );
    assert_eq!(screens::STANDINGS_HEADER, "Classement général");
    // Issue #122: online, the players' pseudos (ours « Toi »), the host's standings, and what
    // comes next: the host moves on after a moment (or Entrée), the others wait.
    let mut names: Vec<String> = (0..12).map(|e| format!("Kart {}", e + 1)).collect();
    names[10] = "Éloïse".into();
    names[11] = "Zoé".into();
    let standings: Vec<usize> = (0..12).rev().collect();
    grand_prix.online = Some(crate::race::OnlineStandings {
        names,
        me: Some(10),
        standings: standings.clone(),
        hosting: false,
    });
    assert_eq!(grand_prix.standings(), standings);
    let lines = standings_lines(&grand_prix);
    assert_eq!((lines[0].name.as_str(), lines[0].player), ("Zoé", false));
    assert_eq!((lines[1].name.as_str(), lines[1].player), ("Toi", true));
    assert_eq!(lines[2].name, "Kart 10");
    assert_eq!(
        screens::standings_footer(&grand_prix),
        "Course suivante (Col des Moufles) dans un instant"
    );
    let mut online_texts = vec![
        screens::standings_footer(&grand_prix),
        screens::podium_footer(&grand_prix),
    ];
    grand_prix.online.as_mut().unwrap().hosting = true;
    grand_prix.race = 3;
    assert_eq!(
        screens::standings_footer(&grand_prix),
        "Le podium dans un instant  ·  Entrée : tout de suite"
    );
    assert_eq!(
        screens::podium_footer(&grand_prix),
        "Retour au salon dans un instant  ·  Entrée : tout de suite"
    );
    online_texts.extend([
        screens::standings_footer(&grand_prix),
        screens::podium_footer(&grand_prix),
    ]);
    for line in standings_lines(&grand_prix) {
        online_texts.push(line.name);
    }
    for text in &online_texts {
        assert!(text.chars().all(crate::ui::format::writable), "{text}");
    }
}

fn count<C: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), With<C>>().iter(world).count()
}

fn menu(app: &App) -> RaceMenu {
    app.world().resource::<RaceMenu>().clone()
}

fn arrow(app: &mut App, key: KeyCode, times: usize) {
    let logical = match key {
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        _ => Key::ArrowRight,
    };
    for _ in 0..times {
        tap(app, key, logical.clone());
    }
}

/// Issue #99 acceptance: a custom championship of two races, its circuits chosen one by one
/// (Col des Moufles, then Prairie Pompon), races them in that order, under the name
/// « Championnat », with its points and its standings, and ends on the podium after the second.
#[test]
fn a_championship_with_its_circuits_chosen_races_them_in_order() {
    let mut app = quick_game(2);
    let circuits = tracks::circuits();
    let col = circuits
        .iter()
        .position(|c| c.name == "Col des Moufles")
        .unwrap();
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    // The championship, a custom one, two races (4, 3, 2), chosen.
    arrow(&mut app, KeyCode::ArrowRight, 1);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowLeft, 2);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    let setup = menu(&app).setup;
    assert_eq!(
        (setup.mode, setup.formula, setup.races, setup.pick),
        (GameMode::GrandPrix, Formula::Custom, 2, Pick::Chosen)
    );
    assert_eq!(setup.chosen, [0, 1], "the first circuits not chosen yet");
    // Its races are its last lines (past the mirror, the bots and the items): the first one on
    // the Col des Moufles, the second on Prairie Pompon.
    arrow(&mut app, KeyCode::ArrowDown, 4);
    assert_eq!(menu(&app).row(), MenuRow::Race(0));
    arrow(&mut app, KeyCode::ArrowRight, col);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    assert_eq!(menu(&app).row(), MenuRow::Race(1));
    arrow(&mut app, KeyCode::ArrowLeft, 1);
    assert_eq!(menu(&app).setup.chosen, [col, 0]);
    enter(&mut app);
    for (race, name) in [(0, "Col des Moufles"), (1, "Prairie Pompon")] {
        wait_for_the_level(&mut app, name);
        let gp = grand_prix(&app);
        assert_eq!((gp.race, gp.stage), (race, GrandPrixStage::Racing));
        assert_eq!(
            gp.cup.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Col des Moufles", "Prairie Pompon"]
        );
        assert_eq!(gp.title, "Championnat");
        assert_eq!(gp.entrants, 3);
        player_becomes_a_bot(&mut app);
        race_to_the_results(&mut app);
        enter(&mut app);
        for _ in 0..5 {
            app.update();
        }
        let gp = grand_prix(&app);
        assert_eq!(gp.stage, GrandPrixStage::Standings, "race {race}");
        assert_eq!(gp.orders.len(), race + 1);
        assert_eq!(
            screens::standings_title(&gp),
            format!("Championnat {}/2 · {name}", race + 1)
        );
        enter(&mut app);
    }
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(grand_prix(&app).stage, GrandPrixStage::Podium);
}

/// Issue #99 acceptance: a random custom championship draws its circuits from the race's seed:
/// what the screen shows is what is raced, the same seed and draw give the same circuits, none
/// twice; the next championship is another draw. Without bots, the championship is the player
/// alone (its bots kept for later).
#[test]
fn a_random_championship_draws_its_circuits_from_the_seed() {
    let mut app = quick_game(2);
    app.world_mut().resource_mut::<RaceGrid>().seed = 77;
    let catalog = Catalog::game(4);
    let n = catalog.circuits.len();
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    arrow(&mut app, KeyCode::ArrowRight, 1);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    // Five races, drawn at random; no bots.
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    arrow(&mut app, KeyCode::ArrowDown, 3);
    assert_eq!(menu(&app).row(), MenuRow::Bots);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    let setup = menu(&app).setup;
    assert_eq!(
        (setup.races, setup.pick, setup.bots),
        (5, Pick::Random, false)
    );
    let shown = setup.program(77, &catalog);
    assert_eq!(shown, draw_circuits(77, setup.draw, 5, n));
    let mut distinct = shown.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 5, "no circuit twice: {shown:?}");
    enter(&mut app);
    let gp = grand_prix(&app);
    assert_eq!(gp.cup, catalog.races(&shown), "the draw shown is raced");
    assert_eq!(gp.entrants, 1, "without bots, the player alone");
    assert_eq!(app.world().resource::<RaceGrid>().bots, 0);
    assert_eq!(menu(&app).kept_bots, Some(2));
    wait_for_the_level(&mut app, catalog.name(shown[0]));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(count::<Racer>(&mut app), 1);
    // The same seed and draw: the same circuits, every time.
    assert_eq!(draw_circuits(77, setup.draw, 5, n), shown);
    // The next championship: another draw.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    let next = menu(&app).setup;
    assert_eq!(
        next.mode,
        GameMode::GrandPrix,
        "it opens on the championship"
    );
    assert_ne!(next.draw, setup.draw);
    assert_ne!(next.program(77, &catalog), shown);
    tap(&mut app, KeyCode::Escape, Key::Escape);
}

/// Issue #99 acceptance: « Bots : non » races the player alone (its bots kept, back with « Bots :
/// oui »); « Objets : non » builds the circuit without any item box (the croquettes stay), back
/// with « Objets : oui ». The screen opens on what is raced.
#[test]
fn a_race_alone_without_bots_nor_items() {
    let bots = 3;
    let mut app = quick_game(bots);
    let boxes = count::<ItemBoxSpot>(&mut app);
    let croquettes = count::<CroquetteSpot>(&mut app);
    assert!(boxes > 0 && croquettes > 0);
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    assert_eq!(
        menu(&app).rows(),
        [
            MenuRow::Mode,
            MenuRow::Circuit,
            MenuRow::Mirror,
            MenuRow::Bots,
            MenuRow::Items
        ]
    );
    arrow(&mut app, KeyCode::ArrowDown, 3);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowRight, 1);
    let setup = menu(&app).setup;
    assert!(!setup.bots && !setup.items);
    enter(&mut app);
    wait_for_the_level(&mut app, "Prairie Pompon");
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(count::<Racer>(&mut app), 1, "the player alone");
    assert_eq!(app.world().resource::<RaceGrid>().bots, 0);
    assert_eq!(menu(&app).kept_bots, Some(bots));
    assert_eq!(count::<ItemBoxSpot>(&mut app), 0, "no item box");
    assert_eq!(count::<crate::items::ItemBox>(&mut app), 0);
    assert_eq!(
        count::<CroquetteSpot>(&mut app),
        croquettes,
        "the croquettes stay"
    );
    assert!(app.world().contains_resource::<NoItemBoxes>());

    // It opens on that; both back on.
    tap(&mut app, RACE_MENU_KEY, Key::Character("n".into()));
    let setup = menu(&app).setup;
    assert!(!setup.bots && !setup.items);
    arrow(&mut app, KeyCode::ArrowDown, 3);
    arrow(&mut app, KeyCode::ArrowLeft, 1);
    arrow(&mut app, KeyCode::ArrowDown, 1);
    arrow(&mut app, KeyCode::ArrowLeft, 1);
    enter(&mut app);
    wait_for_the_level(&mut app, "Prairie Pompon");
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(count::<Racer>(&mut app), bots + 1);
    assert_eq!(app.world().resource::<RaceGrid>().bots, bots);
    assert_eq!(menu(&app).kept_bots, None);
    assert_eq!(count::<ItemBoxSpot>(&mut app), boxes);
    assert!(!app.world().contains_resource::<NoItemBoxes>());
}
