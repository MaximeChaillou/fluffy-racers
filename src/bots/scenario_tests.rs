//! Headless bot scenarios (issue #6): the whole game (`GamePlugin`) with `MinimalPlugins`, one
//! fixed tick per `app.update()`. Run with `--nocapture` to see each race's figures.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use super::*;
use crate::TICK_HZ;
use crate::croquettes::Croquettes;
use crate::kart::{KartBodyPivot, KartInput, KartState, KartTuning, MiniTurbo, PlayerKart};
use crate::race::{
    Course, DrivesOnAfterFinish, Race, RaceGrid, RacePhase, RaceProgress, Racer, Tow, TowReason,
    ticks_to_seconds,
};
use crate::tests::{app_with_bots, game_app_unloaded, player, wait_for_the_player};
use crate::track::{TrackChoice, TrackData, TrackHandle, TrackLayout};
use crate::ui::{DebugOverlay, HudText, SimulationCost};

/// A tow of the pelican, as seen by the race log.
#[derive(Clone, Copy, Debug)]
struct TowEvent {
    reason: TowReason,
    /// Centerline distance where the kart was picked up (m), and how far from the centerline
    /// compared to the inner face of the walls.
    at: f32,
    lateral: f32,
    wall: f32,
    finished: bool,
}

/// What happened to a kart during a race.
#[derive(Clone, Debug, Default)]
struct KartLog {
    slot: usize,
    difficulty: Option<BotDifficulty>,
    /// Mini-turbos fired, per tier, and their sequence.
    mini_turbos: [u32; 3],
    sequence: Vec<MiniTurbo>,
    /// Drifts released before the blue tier (no boost), and drifts that broke with the button
    /// still held (wall, too slow, too long in the air).
    wasted: u32,
    broken: u32,
    tows: Vec<TowEvent>,
    previous: Option<KartState>,
    /// Croquettes picked up and lost (issue #14), most held at once.
    croquettes_picked: u32,
    croquettes_lost: u32,
    croquettes_most: u8,
    croquettes_held: u8,
}

impl KartLog {
    fn fired(&self) -> u32 {
        self.mini_turbos.iter().sum()
    }

    /// Longest run of consecutive mini-turbos of at least `tier`.
    fn longest_chain(&self, tier: MiniTurbo) -> usize {
        let (mut best, mut run) = (0, 0);
        for t in &self.sequence {
            run = if *t >= tier { run + 1 } else { 0 };
            best = best.max(run);
        }
        best
    }
}

/// Every kart of a race, followed tick by tick.
struct RaceLog {
    karts: Vec<(Entity, KartLog)>,
    /// Lateral positions of the racing karts on the start straight of Prairie Pompon (m).
    straight_laterals: Vec<f32>,
    ticks: u32,
}

impl RaceLog {
    fn new(app: &mut App) -> Self {
        let world = app.world_mut();
        let mut karts: Vec<(Entity, KartLog)> = world
            .query::<(Entity, &Racer, Option<&Bot>)>()
            .iter(world)
            .map(|(entity, racer, bot)| {
                let log = KartLog {
                    slot: racer.grid_slot,
                    difficulty: bot.map(|b| b.difficulty),
                    ..default()
                };
                (entity, log)
            })
            .collect();
        karts.sort_by_key(|(_, log)| log.slot);
        Self {
            karts,
            straight_laterals: Vec::new(),
            ticks: 0,
        }
    }

    /// Records the tick that just ran: mini-turbos fired (a drift with a tier reached released
    /// with the button), tows, lateral positions on the start straight.
    fn record(&mut self, app: &App) {
        self.ticks += 1;
        let world = app.world();
        let tuning = world.resource::<KartTuning>();
        let course = world.resource::<Course>();
        for (entity, log) in &mut self.karts {
            let state = *world.get::<KartState>(*entity).unwrap();
            let input = *world.get::<KartInput>(*entity).unwrap();
            if let Some(previous @ KartState::Drifting { .. }) = log.previous
                && !matches!(state, KartState::Drifting { .. })
            {
                match (input.jump_drift, previous.mini_turbo(tuning, false)) {
                    (true, _) => log.broken += 1,
                    (false, Some(tier)) => {
                        log.mini_turbos[tier.index()] += 1;
                        log.sequence.push(tier);
                    }
                    (false, None) => log.wasted += 1,
                }
            }
            let progress = world.get::<RaceProgress>(*entity).unwrap();
            let tow = world.get::<Tow>(*entity).unwrap();
            if let Some(carry) = tow.carry
                && log.previous != Some(KartState::Respawning)
            {
                let p = course.project_anywhere(carry.from.translation);
                log.tows.push(TowEvent {
                    reason: carry.reason,
                    at: p.distance,
                    lateral: p.lateral,
                    wall: p.wall,
                    finished: progress.finished(),
                });
            }
            log.previous = Some(state);
            let held = world.get::<Croquettes>(*entity).map_or(0, |c| c.count);
            if held > log.croquettes_held {
                log.croquettes_picked += (held - log.croquettes_held) as u32;
            } else {
                log.croquettes_lost += (log.croquettes_held - held) as u32;
            }
            log.croquettes_held = held;
            log.croquettes_most = log.croquettes_most.max(held);

            if progress.finished() || state == KartState::Respawning {
                continue;
            }
            let along = course.center.wrap(progress.distance);
            if (10.0..80.0).contains(&along) {
                let position = world.get::<Transform>(*entity).unwrap().translation;
                self.straight_laterals
                    .push(course.project(progress.passed, position).lateral);
            }
        }
    }

    fn tows(&self) -> impl Iterator<Item = (usize, &TowEvent)> {
        self.karts
            .iter()
            .flat_map(|(_, log)| log.tows.iter().map(move |t| (log.slot, t)))
    }

    fn print(&self, app: &App, label: &str) {
        println!("--- {label}: {} ticks", self.ticks);
        for (entity, log) in &self.karts {
            let progress = app.world().get::<RaceProgress>(*entity).unwrap();
            let laps: Vec<String> = progress
                .lap_times
                .iter()
                .map(|t| format!("{:.2}", ticks_to_seconds(*t)))
                .collect();
            println!(
                "slot {:2} {:?}: finish {:?} s, laps {:?}, mini-turbos blue/orange/purple {:?}, \
                 drifts without boost {}, broken {}, croquettes picked {} lost {} (at most {}, \
                 {} at the end)",
                log.slot,
                log.difficulty,
                progress.finish.map(ticks_to_seconds),
                laps,
                log.mini_turbos,
                log.wasted,
                log.broken,
                log.croquettes_picked,
                log.croquettes_lost,
                log.croquettes_most,
                log.croquettes_held,
            );
            for tow in &log.tows {
                println!(
                    "    towed ({:?}) at {:.0} m along the lap, {:.1} m from the centerline \
                     (walls at {:.1} m){}",
                    tow.reason,
                    tow.at,
                    tow.lateral,
                    tow.wall,
                    if tow.finished {
                        ", after the finish"
                    } else {
                        ""
                    }
                );
            }
        }
        if !self.straight_laterals.is_empty() {
            println!(
                "lateral spread on the start straight: {:.2} m ({} samples)",
                spread(&self.straight_laterals),
                self.straight_laterals.len()
            );
        }
    }
}

/// Standard deviation.
fn spread(values: &[f32]) -> f32 {
    let n = values.len().max(1) as f32;
    let mean = values.iter().sum::<f32>() / n;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
}

fn race(app: &App) -> &Race {
    app.world().resource::<Race>()
}

fn progress(app: &App, kart: Entity) -> RaceProgress {
    app.world().get::<RaceProgress>(kart).unwrap().clone()
}

/// Runs updates until the race and the `bots` bots exist.
fn wait_for_the_bots(app: &mut App, bots: usize) {
    for _ in 0..10 {
        let world = app.world_mut();
        let spawned = world.query::<&Bot>().iter(world).count();
        if spawned == bots && world.get_resource::<Race>().is_some() {
            return;
        }
        app.update();
    }
    panic!("the bots or the race never came");
}

/// The game on Prairie Pompon with `bots` bots, once the race and the bots exist.
fn race_with_bots(bots: usize) -> App {
    let mut app = app_with_bots(TrackChoice::default(), bots);
    wait_for_the_bots(&mut app, bots);
    app
}

/// The game on the circuit `data` (built in the test) with `bots` bots.
fn race_on(data: TrackData, bots: usize) -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), bots);
    // A circuit handed over directly: no file is loaded.
    let handle = app
        .world_mut()
        .resource_mut::<Assets<TrackData>>()
        .add(data);
    app.insert_resource(TrackHandle(handle));
    wait_for_the_player(&mut app);
    wait_for_the_bots(&mut app, bots);
    app
}

/// A round circuit (radius 28 m, 176 m): one endless right turn, long enough for an orange
/// mini-turbo, unlike the 180° turns of Prairie Pompon.
fn roundabout() -> TrackData {
    let points: String = (0..12)
        .map(|k| {
            let a = (k as f32 * 30.0).to_radians();
            format!(
                "(pos: ({:.3}, 0.0, {:.3})),",
                28.0 * a.cos(),
                28.0 * a.sin()
            )
        })
        .collect();
    let data = TrackData::from_ron(&format!("(name: \"Rond-point\", points: [{points}])")).unwrap();
    data.validate().unwrap();
    data
}

fn set_difficulties(app: &mut App, difficulty: impl Fn(usize) -> BotDifficulty) {
    let world = app.world_mut();
    for (mut bot, racer) in world.query::<(&mut Bot, &Racer)>().iter_mut(world) {
        bot.difficulty = difficulty(racer.grid_slot);
    }
}

/// The player's kart is driven by a bot too (a race of bots only).
fn player_becomes_a_bot(app: &mut App, difficulty: BotDifficulty) {
    let kart = player(app);
    let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
    let tuning = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(difficulty, slot, &tuning));
}

/// Runs the race until every racer has finished (or `max_seconds` of race), logging every tick.
fn run_race(app: &mut App, log: &mut RaceLog, max_seconds: f32) {
    let max_ticks = (max_seconds * TICK_HZ as f32) as u32 + race(app).timings.before_go();
    while log.ticks < max_ticks {
        app.update();
        log.record(app);
        let world = app.world_mut();
        if world
            .query::<&RaceProgress>()
            .iter(world)
            .all(|p| p.finished())
        {
            return;
        }
    }
}

/// A key pressed this frame, as the window would send it.
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

/// Issue #6 acceptance (load test): the game's grid on Prairie Pompon, 11 bots, plus the
/// player's kart driven by a bot too: 12 karts race 3 laps without getting stuck.
#[test]
fn twelve_karts_race_three_laps_without_getting_stuck() {
    let mut app = race_with_bots(11);
    // The game's default: 11 bots, all Normal.
    assert_eq!(RaceGrid::default().bots, 11);
    assert_eq!(BotTuning::default().grid_difficulty, BotDifficulty::Normal);
    player_becomes_a_bot(&mut app, BotDifficulty::Normal);
    let mut log = RaceLog::new(&mut app);
    assert_eq!(log.karts.len(), 12);
    let started = std::time::Instant::now();
    run_race(&mut app, &mut log, 120.0);
    let elapsed = started.elapsed().as_secs_f64();
    log.print(&app, "12 karts, Normal");
    println!(
        "wall time {elapsed:.2} s for {} ticks: {:.3} ms per tick (test profile, whole app)",
        log.ticks,
        elapsed * 1000.0 / log.ticks as f64
    );

    let mut finishes = Vec::new();
    for (entity, kart) in &log.karts {
        let p = progress(&app, *entity);
        let finish = p.finish.map(ticks_to_seconds);
        assert!(finish.is_some(), "slot {} did not finish: {p:?}", kart.slot);
        let finish = finish.unwrap();
        assert!(
            (50.0..75.0).contains(&finish),
            "slot {}: {finish} s",
            kart.slot
        );
        // About 19 s a lap; the first one starts from the grid, up to 38 m behind the line,
        // in the traffic of the start.
        for (i, lap) in p.lap_times.iter().enumerate() {
            let lap = ticks_to_seconds(*lap);
            let max = if i == 0 { 32.0 } else { 26.0 };
            assert!(
                (16.0..max).contains(&lap),
                "slot {}: lap {lap} s",
                kart.slot
            );
        }
        finishes.push(finish);
    }
    // Without a player, the race is over once everybody has finished.
    app.update();
    assert_eq!(race(&app).phase(), RacePhase::Finished);
    // Hardly ever towed (zero in our runs; the known wall climb #27 may cost one).
    let tows: Vec<_> = log.tows().collect();
    assert!(tows.len() <= 2, "{tows:?}");
    // They drift: blue mini-turbos (the turns of Prairie Pompon are too short for orange, see
    // `hard_bots_chain_orange_mini_turbos_in_a_long_turn`).
    let firing = log.karts.iter().filter(|(_, k)| k.fired() > 0).count();
    let total: u32 = log.karts.iter().map(|(_, k)| k.fired()).sum();
    assert!(firing >= 10, "{firing} bots fired a mini-turbo");
    assert!(total >= 24, "{total} mini-turbos");
    // The bots release their drifts themselves: hardly any drift breaks (wall, too slow).
    let broken: u32 = log.karts.iter().map(|(_, k)| k.broken).sum();
    assert!(broken <= 3, "{broken} broken drifts");
    // Issue #14: driving over them, the bots pick the croquettes of the circuit up (the leaders
    // the most: the lines are empty for 10 s after them).
    let picking = log
        .karts
        .iter()
        .filter(|(_, k)| k.croquettes_picked > 0)
        .count();
    assert!(picking >= 4, "{picking} karts picked croquettes up");
    // No single file: spread across the straight, and spread at the finish.
    let lateral = spread(&log.straight_laterals);
    assert!(lateral > 1.0, "lateral spread {lateral} m");
    let (first, last) = finishes
        .iter()
        .fold((f32::MAX, 0.0f32), |(lo, hi), f| (lo.min(*f), hi.max(*f)));
    assert!(last - first > 1.0, "finishes from {first} to {last} s");

    // After the finish the bots drive on (they do not block the line), and the pelican does
    // not mistake them for karts out of the track.
    let position =
        |app: &App, entity: Entity| app.world().get::<Transform>(entity).unwrap().translation;
    let mut last: Vec<Vec3> = log.karts.iter().map(|(e, _)| position(&app, *e)).collect();
    let mut driven = vec![0.0; last.len()];
    for _ in 0..(5.0 * TICK_HZ) as usize {
        app.update();
        log.record(&app);
        for (i, (entity, _)) in log.karts.iter().enumerate() {
            let now = position(&app, *entity);
            driven[i] += now.distance(last[i]);
            last[i] = now;
        }
    }
    for ((entity, kart), driven) in log.karts.iter().zip(&driven) {
        assert!(*driven > 75.0, "slot {} drove {driven} m in 5 s", kart.slot);
        assert!(app.world().get::<DrivesOnAfterFinish>(*entity).is_some());
    }
    let after: Vec<_> = log.tows().filter(|(_, t)| t.finished).collect();
    assert!(after.is_empty(), "{after:?}");
}

/// Issue #6: the three difficulties drive differently. A mixed grid (Easy, Normal, Hard in
/// turn): Hard finishes first on average, then Normal, then Easy; Easy releases its drifts at
/// blue only; Hard fires a mini-turbo in most big turns.
#[test]
fn the_three_difficulties_drive_differently() {
    let difficulty = |slot: usize| BotDifficulty::ALL[slot % 3];
    let mut app = race_with_bots(11);
    set_difficulties(&mut app, difficulty);
    player_becomes_a_bot(&mut app, difficulty(11));
    let mut log = RaceLog::new(&mut app);
    run_race(&mut app, &mut log, 120.0);
    log.print(&app, "12 karts, mixed difficulties");

    let mut times = [Vec::new(), Vec::new(), Vec::new()];
    let mut fired = [Vec::new(), Vec::new(), Vec::new()];
    for (entity, kart) in &log.karts {
        let level = kart.difficulty.unwrap().index();
        let finish = progress(&app, *entity).finish;
        assert!(finish.is_some(), "slot {} did not finish", kart.slot);
        times[level].push(ticks_to_seconds(finish.unwrap()));
        fired[level].push(kart.fired());
        if kart.difficulty == Some(BotDifficulty::Easy) {
            assert_eq!(
                kart.mini_turbos[1] + kart.mini_turbos[2],
                0,
                "Easy releases at blue: {kart:?}"
            );
        }
    }
    let average = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let [easy, normal, hard] = [&times[0], &times[1], &times[2]].map(|t| average(t));
    println!("average finish: Easy {easy:.2} s, Normal {normal:.2} s, Hard {hard:.2} s");
    assert!(hard < normal && normal < easy, "{hard} {normal} {easy}");
    // Two big turns a lap: 6 chances for a mini-turbo.
    let hard_fired: u32 = fired[2].iter().sum();
    let easy_fired: u32 = fired[0].iter().sum();
    assert!(fired[2].iter().all(|f| *f >= 3), "Hard: {:?}", fired[2]);
    assert!(
        hard_fired > easy_fired,
        "Hard {hard_fired}, Easy {easy_fired}"
    );
    let tows: Vec<_> = log.tows().collect();
    assert!(tows.len() <= 2, "{tows:?}");
}

/// Issue #6 acceptance: at the highest difficulty the bots chain orange mini-turbos, in a turn
/// long enough for it (one endless turn), while Easy bots release at blue.
#[test]
fn hard_bots_chain_orange_mini_turbos_in_a_long_turn() {
    let mut app = race_on(roundabout(), 4);
    let difficulty = |slot: usize| {
        if slot.is_multiple_of(2) {
            BotDifficulty::Hard
        } else {
            BotDifficulty::Easy
        }
    };
    set_difficulties(&mut app, difficulty);
    player_becomes_a_bot(&mut app, difficulty(4));
    assert_eq!(app.world().resource::<TrackLayout>().name, "Rond-point");
    let mut log = RaceLog::new(&mut app);
    run_race(&mut app, &mut log, 60.0);
    log.print(&app, "round circuit, Hard and Easy");
    for (entity, kart) in &log.karts {
        assert!(
            progress(&app, *entity).finished(),
            "slot {} did not finish",
            kart.slot
        );
        match kart.difficulty.unwrap() {
            BotDifficulty::Hard => {
                let orange = kart.mini_turbos[1] + kart.mini_turbos[2];
                assert!(orange >= 3, "slot {}: {kart:?}", kart.slot);
                assert!(kart.longest_chain(MiniTurbo::Orange) >= 3, "{kart:?}");
            }
            _ => {
                assert!(kart.mini_turbos[0] >= 3, "slot {}: {kart:?}", kart.slot);
                assert_eq!(kart.mini_turbos[1] + kart.mini_turbos[2], 0, "{kart:?}");
            }
        }
    }
    assert!(log.tows().count() <= 1);
}

/// Issue #6: the game's grid is 1 player and 11 bots; the player starts last, nobody moves
/// before GO, the HUD counts 12 racers, the bots leave the idle player behind (and pass it a
/// lap later), the catch-up slows down the bots far ahead of it, and F3 shows the overlay.
#[test]
fn the_game_fills_the_grid_with_eleven_bots() {
    let mut app = race_with_bots(11);
    let layout = app.world().resource::<TrackLayout>().clone();
    let me = player(&mut app);
    assert_eq!(app.world().get::<Racer>(me).unwrap().grid_slot, 11);
    let start = app.world().get::<Transform>(me).unwrap().translation;
    assert!(start.distance(layout.grid[11].translation) < 0.5, "{start}");
    assert!(app.world().get::<DrivesOnAfterFinish>(me).is_none());

    let world = app.world_mut();
    let mut slots: Vec<usize> = world
        .query_filtered::<&Racer, With<Bot>>()
        .iter(world)
        .map(|r| r.grid_slot)
        .collect();
    slots.sort();
    assert_eq!(slots, (0..11).collect::<Vec<_>>());
    let bots: Vec<(Entity, usize, Vec3)> = world
        .query_filtered::<(Entity, &Racer, &Transform), With<Bot>>()
        .iter(world)
        .map(|(e, r, t)| (e, r.grid_slot, t.translation))
        .collect();
    for (_, slot, position) in &bots {
        assert!(position.distance(layout.grid[*slot].translation) < 0.5);
    }
    // One visual per kart.
    assert_eq!(world.query::<&KartBodyPivot>().iter(world).count(), 12);

    // Nobody moves before GO.
    while !race(&app).started() {
        app.update();
    }
    for (bot, slot, position) in &bots {
        let now = app.world().get::<Transform>(*bot).unwrap().translation;
        let moved = (now - *position).with_y(0.0).length();
        assert!(moved < 0.1, "slot {slot} moved {moved} m before GO");
    }
    let hud = |app: &mut App, field: HudText| {
        let world = app.world_mut();
        world
            .query::<(&HudText, &Text)>()
            .iter(world)
            .find(|(f, _)| **f == field)
            .map(|(_, t)| t.0.clone())
            .unwrap()
    };
    app.update();
    assert_eq!(hud(&mut app, HudText::Rank), "12e");
    assert_eq!(hud(&mut app, HudText::RacerCount), "/ 12");

    // The player stays on the grid: the bots go, the far ones lose a little top speed.
    for _ in 0..(8.0 * TICK_HZ) as usize {
        app.update();
    }
    let tuning = app.world().resource::<BotTuning>().clone();
    let mut leader_scale = 1.0;
    for (bot, slot, _) in &bots {
        let p = progress(&app, *bot);
        // Along the track from its grid slot (since issue #14 the croquettes make the leaders
        // fast enough to be on the back straight by now, which runs back toward the grid: a
        // straight line from the grid is no longer a measure of how far they went).
        let course = app.world().resource::<Course>();
        let moved = p.distance - course.project(0, layout.grid[*slot].translation).distance;
        assert!(moved > 100.0, "slot {slot} barely moved ({moved} m)");
        // The catch-up is one factor of the top speed; since issue #14 the croquettes held are
        // another one, and `MaxSpeedScale` is their product.
        let factors = *app.world().get::<MaxSpeedFactors>(*bot).unwrap();
        let scale = factors.catch_up;
        let expected = catch_up_scale(progress(&app, me).distance - p.distance, &tuning);
        assert!(
            (scale - expected).abs() < 0.003,
            "slot {slot}: {scale} vs {expected}"
        );
        let combined = app.world().get::<MaxSpeedScale>(*bot).unwrap().0;
        assert!(
            (combined - factors.product()).abs() < 1e-6,
            "slot {slot}: {combined} vs {factors:?}"
        );
        if p.rank == 1 {
            leader_scale = scale;
        }
    }
    assert!(
        (1.0 - tuning.catch_up_max..1.0).contains(&leader_scale),
        "{leader_scale}"
    );
    assert_eq!(app.world().get::<MaxSpeedScale>(me).unwrap().0, 1.0);
    assert_eq!(hud(&mut app, HudText::Rank), "12e");

    // F3: the debug overlay, with the 12 karts.
    let overlay_visible = |app: &mut App| {
        let world = app.world_mut();
        *world
            .query_filtered::<&Visibility, With<DebugOverlay>>()
            .single(world)
            .unwrap()
            != Visibility::Hidden
    };
    assert!(!overlay_visible(&mut app), "hidden by default");
    tap(&mut app, KeyCode::F3, Key::F3);
    assert!(overlay_visible(&mut app));
    app.update();
    let world = app.world_mut();
    let text = world
        .query::<&Text>()
        .iter(world)
        .find(|t| t.0.starts_with("FPS"))
        .map(|t| t.0.clone())
        .unwrap();
    assert!(text.contains("Karts 12"), "{text}");
    assert!(
        app.world()
            .resource::<SimulationCost>()
            .average_ms
            .is_some()
    );
    tap(&mut app, KeyCode::F3, Key::F3);
    assert!(!overlay_visible(&mut app));

    // A lap later the bots come back past the idle player on the grid: nobody gets stuck.
    let mut log = RaceLog::new(&mut app);
    let n = app.world().resource::<Course>().checkpoint_count();
    for _ in 0..(25.0 * TICK_HZ) as usize {
        app.update();
        log.record(&app);
        if bots.iter().all(|(b, ..)| progress(&app, *b).passed > n + 2) {
            break;
        }
    }
    for (bot, slot, _) in &bots {
        assert!(
            progress(&app, *bot).passed > n + 2,
            "slot {slot} did not pass the grid again"
        );
    }
    let tows: Vec<_> = log.tows().collect();
    println!("bots past the idle player: {} tows {tows:?}", tows.len());
    assert!(tows.len() <= 1, "{tows:?}");
}

/// Load test measurement, run by hand (numbers go in STATUS.md):
/// `cargo test --release --lib measure_tick_cost -- --ignored --nocapture`.
/// Headless native: no rendering, so this is the simulation's share of a frame only.
#[test]
#[ignore]
fn measure_tick_cost_with_twelve_karts() {
    let mut app = race_with_bots(11);
    player_becomes_a_bot(&mut app, BotDifficulty::Normal);
    // Countdown, then 60 s of race with 12 karts (the slowest finish around 60 s).
    while !race(&app).started() {
        app.update();
    }
    let mut durations = Vec::with_capacity(3600);
    for _ in 0..3600 {
        let start = std::time::Instant::now();
        app.update();
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let mean = durations.iter().sum::<f64>() / durations.len() as f64;
    let at = |q: f64| durations[((durations.len() - 1) as f64 * q) as usize];
    let sim = app.world().resource::<SimulationCost>().average_ms;
    println!(
        "12 karts, one tick per update: mean {mean:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms; simulation alone (smoothed) {sim:?} ms",
        at(0.5),
        at(0.95),
        at(1.0)
    );
}
