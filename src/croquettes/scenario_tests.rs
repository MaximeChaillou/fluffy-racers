//! Headless croquette scenarios (issue #14 acceptance): the real simulation (Rapier, karts) plus
//! the croquette rules on a flat slab, one fixed tick per `app.update()`; then the whole game on
//! Prairie Pompon (circuit file, HUD, visuals, sound hook, restart).

use bevy::ecs::message::MessageCursor;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::kart::{
    Invincibility, KartInput, KartStats, KartTuning, MaxSpeedScale, Stat, StatEffects, StatsTuning,
    kart_body,
};
use crate::track::{CROQUETTE_HEIGHT, Surface, WALL_RESTITUTION, spawn_static_box};

const TICKS_PER_SECOND: f32 = 60.0;
const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
    interact: false,
};

/// A kart at the origin looking down -Z on a large flat slab, with the croquette rules.
struct Arena {
    app: App,
    kart: Entity,
}

impl Arena {
    fn new() -> Self {
        Self::build(false)
    }

    /// With the item rules too (issue #16: an item hit goes through them).
    fn with_items() -> Self {
        Self::build(true)
    }

    fn build(items: bool) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            SimulationPlugin,
            CroquettePlugin,
        ))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        if items {
            app.add_plugins(crate::items::ItemPlugin);
        }
        app.finish();
        app.cleanup();
        let world = app.world_mut();
        let mut commands = world.commands();
        spawn_static_box(
            &mut commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
        world.flush();
        let kart = Self::spawn_kart(world, Transform::from_xyz(0.0, 0.55, 0.0));
        app.update();
        let mut arena = Self { app, kart };
        arena.run(0.5, KartInput::default());
        arena
    }

    fn spawn_kart(world: &mut World, transform: Transform) -> Entity {
        let tuning = world.resource::<KartTuning>().clone();
        world.spawn(kart_body(&tuning, transform)).id()
    }

    fn other_kart(&mut self, transform: Transform) -> Entity {
        let kart = Self::spawn_kart(self.app.world_mut(), transform);
        self.app.update();
        kart
    }

    fn tuning(&self) -> CroquetteTuning {
        self.app.world().resource::<CroquetteTuning>().clone()
    }

    /// A croquette of the track at `(x, z)`, at its usual height above the slab.
    fn croquette(&mut self, x: f32, z: f32) -> Entity {
        self.app
            .world_mut()
            .spawn((CroquetteSpot, Transform::from_xyz(x, CROQUETTE_HEIGHT, z)))
            .id()
    }

    fn tick_with(&mut self, inputs: &[(Entity, KartInput)]) {
        for (kart, input) in inputs {
            *self.app.world_mut().get_mut::<KartInput>(*kart).unwrap() = *input;
        }
        self.app.update();
    }

    fn tick(&mut self, input: KartInput) {
        self.tick_with(&[(self.kart, input)]);
    }

    fn run(&mut self, seconds: f32, input: KartInput) {
        for _ in 0..(seconds * TICKS_PER_SECOND).round() as usize {
            self.tick(input);
        }
    }

    fn held(&self, kart: Entity) -> u8 {
        self.app.world().get::<Croquettes>(kart).unwrap().count
    }

    fn set_held(&mut self, kart: Entity, count: u8) {
        self.app
            .world_mut()
            .get_mut::<Croquettes>(kart)
            .unwrap()
            .count = count;
    }

    fn boost(&self, kart: Entity) -> Boost {
        *self.app.world().get::<Boost>(kart).unwrap()
    }

    fn forward_speed(&self, kart: Entity) -> f32 {
        let world = self.app.world();
        let forward = *world.get::<Transform>(kart).unwrap().forward();
        world.get::<Velocity>(kart).unwrap().linear.dot(forward)
    }

    fn available(&self, croquette: Entity) -> bool {
        self.app
            .world()
            .get::<TrackCroquette>(croquette)
            .unwrap()
            .available()
    }

    fn loose(&mut self) -> Vec<(Entity, LooseCroquette)> {
        let world = self.app.world_mut();
        let mut all: Vec<(Entity, LooseCroquette)> = world
            .query::<(Entity, &LooseCroquette)>()
            .iter(world)
            .map(|(e, c)| (e, *c))
            .collect();
        all.sort_by_key(|(e, _)| *e);
        all
    }

    /// Puts `kart` at `(x, z)` on the slab, stopped, looking down -Z.
    fn teleport(&mut self, kart: Entity, x: f32, z: f32) {
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(kart).unwrap() = Transform::from_xyz(x, 0.55, z);
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
    }
}

/// Reads every message `M` written since the last call.
struct Inbox<M: Message> {
    cursor: MessageCursor<M>,
}

impl<M: Message + Clone> Inbox<M> {
    fn new(app: &App) -> Self {
        Self {
            cursor: app.world().resource::<Messages<M>>().get_cursor_current(),
        }
    }

    fn read(&mut self, app: &App) -> Vec<M> {
        let messages = app.world().resource::<Messages<M>>();
        self.cursor.read(messages).cloned().collect()
    }
}

/// GD § 7 acceptance: each croquette is +1 % of top speed. Top speed measured flat out with 0,
/// then 10 croquettes; the croquettes combine with the bots' catch-up (both multiply).
#[test]
fn ten_croquettes_raise_the_top_speed_by_ten_percent() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let max = KartTuning::default().max_speed;
    arena.run(4.0, FULL_THROTTLE);
    let empty = arena.forward_speed(kart);
    assert!((empty - max).abs() < 0.05, "{empty} vs {max}");

    arena.set_held(kart, 10);
    arena.run(3.0, FULL_THROTTLE);
    let full = arena.forward_speed(kart);
    assert!((full - 1.10 * max).abs() < 0.05, "{full} vs {}", 1.10 * max);
    let world = arena.app.world();
    assert!((world.get::<MaxSpeedFactors>(kart).unwrap().croquettes - 1.10).abs() < 1e-6);
    assert!((world.get::<MaxSpeedScale>(kart).unwrap().0 - 1.10).abs() < 1e-6);

    // A bot far ahead (catch-up −3 %) with 10 croquettes: both factors apply.
    arena
        .app
        .world_mut()
        .get_mut::<MaxSpeedFactors>(kart)
        .unwrap()
        .catch_up = 0.97;
    arena.run(3.0, FULL_THROTTLE);
    let both = arena.forward_speed(kart);
    let expected = 0.97 * 1.10 * max;
    assert!((both - expected).abs() < 0.05, "{both} vs {expected}");
    let scale = arena.app.world().get::<MaxSpeedScale>(kart).unwrap().0;
    assert!((scale - 0.97 * 1.10).abs() < 1e-6, "{scale}");

    // Losing them all: back to the catch-up alone.
    arena.set_held(kart, 0);
    arena.run(3.0, FULL_THROTTLE);
    let none = arena.forward_speed(kart);
    assert!((none - 0.97 * max).abs() < 0.05, "{none}");
}

/// GD § 7 acceptance: picking a croquette up gives a 0.3 s mini-boost (through the kart's
/// `Boost`, on the very tick of the pick-up) and +1 croquette; the croquette disappears.
#[test]
fn a_croquette_gives_a_mini_boost_of_0_3_s() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let tuning = arena.tuning();
    let croquette = arena.croquette(0.0, -15.0);
    let mut inbox = Inbox::<CroquettePickedUp>::new(&arena.app);
    let mut ticks = 0;
    while arena.held(kart) == 0 {
        ticks += 1;
        assert!(ticks < 120, "never picked up");
        arena.tick(FULL_THROTTLE);
        if arena.held(kart) == 0 {
            assert!(!arena.boost(kart).is_active());
            assert!(arena.available(croquette));
        }
    }
    let boost = arena.boost(kart);
    assert_eq!(boost.strength, tuning.pickup_boost_strength);
    assert!((boost.remaining - tuning.pickup_boost_duration).abs() < 1e-5);
    assert!(!arena.available(croquette));
    let picked = inbox.read(&arena.app);
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].kart, kart);
    assert_eq!(picked[0].count, 1);

    // It lasts 0.3 s to the tick (18 ticks, the pick-up tick included), and the kart goes past
    // its top speed (with 1 croquette).
    let max = KartTuning::default().max_speed * 1.01;
    let mut active_ticks = 1;
    let mut top = 0.0f32;
    while arena.boost(kart).is_active() {
        assert!(active_ticks < 60);
        arena.tick(FULL_THROTTLE);
        top = top.max(arena.forward_speed(kart));
        if arena.boost(kart).is_active() {
            active_ticks += 1;
        }
    }
    let seconds = active_ticks as f32 / TICKS_PER_SECOND;
    assert!(
        (seconds - tuning.pickup_boost_duration).abs() <= 1.0 / TICKS_PER_SECOND,
        "{active_ticks} ticks"
    );
    assert_eq!(arena.held(kart), 1);
    assert!(top > max * 1.05, "top {top} (max {max})");
    // Then it settles back to its top speed.
    arena.run(2.0, FULL_THROTTLE);
    assert!((arena.forward_speed(kart) - max).abs() < 0.05);
}

/// GD § 7 acceptance: 10 croquettes at most. Driving through a line of 14: all 14 are taken
/// (each with its mini-boost), the kart holds 10.
#[test]
fn a_kart_holds_ten_croquettes_at_most_but_still_takes_them() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let line: Vec<Entity> = (0..14)
        .map(|i| arena.croquette(0.0, -12.0 - 3.0 * i as f32))
        .collect();
    let mut inbox = Inbox::<CroquettePickedUp>::new(&arena.app);
    let mut counts = Vec::new();
    for _ in 0..(5.0 * TICKS_PER_SECOND) as usize {
        arena.tick(FULL_THROTTLE);
        counts.extend(inbox.read(&arena.app).into_iter().map(|p| p.count));
    }
    assert!(line.iter().all(|c| !arena.available(*c)), "all taken");
    assert_eq!(arena.held(kart), 10);
    assert_eq!(
        counts,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 10, 10, 10, 10],
        "one pick-up message per croquette"
    );
    let factor = arena
        .app
        .world()
        .get::<MaxSpeedFactors>(kart)
        .unwrap()
        .croquettes;
    assert!((factor - 1.10).abs() < 1e-6, "{factor}");
}

/// GD § 7 acceptance: a croquette of the track comes back about 10 s after being picked up
/// (exactly 600 ticks), and can be taken again.
#[test]
fn a_track_croquette_comes_back_after_ten_seconds() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let croquette = arena.croquette(0.0, -10.0);
    while arena.held(kart) == 0 {
        arena.tick(FULL_THROTTLE);
    }
    // Out of the way (another kart could take it back otherwise).
    arena.teleport(kart, 50.0, 0.0);
    for tick in 1..600 {
        arena.tick(KartInput::default());
        assert!(!arena.available(croquette), "back after {tick} ticks");
    }
    arena.tick(KartInput::default());
    assert!(arena.available(croquette), "back after 600 ticks = 10 s");
    // Taken again on the next pass.
    arena.teleport(kart, 0.0, 0.0);
    arena.run(1.5, FULL_THROTTLE);
    assert_eq!(arena.held(kart), 2);
    assert!(!arena.available(croquette));
}

/// GD § 7 acceptance: hit by an item, a kart loses 3 croquettes (only what it has, never below
/// 0). They fly out behind it and land on the ground; another kart can pick them up first, the
/// kart that lost them a little later, and the ones nobody takes vanish.
#[test]
fn an_item_hit_scatters_three_croquettes_that_can_be_picked_up() {
    let mut arena = Arena::new();
    let victim = arena.kart;
    let tuning = arena.tuning();
    let other = arena.other_kart(Transform::from_xyz(30.0, 0.55, 0.0));
    arena.set_held(victim, 5);
    let mut lost_inbox = Inbox::<CroquettesLost>::new(&arena.app);
    let hit = LoseCroquettes {
        kart: victim,
        cause: LossCause::ItemHit,
    };
    arena.app.world_mut().write_message(hit);
    arena.tick(KartInput::default());
    assert_eq!(arena.held(victim), 2);
    let lost = lost_inbox.read(&arena.app);
    assert_eq!(lost.len(), 1);
    assert_eq!((lost[0].lost, lost[0].count), (3, 2));
    let loose = arena.loose();
    assert_eq!(loose.len(), 3);
    let victim_at = arena
        .app
        .world()
        .get::<Transform>(victim)
        .unwrap()
        .translation;
    for (_, croquette) in &loose {
        assert_eq!(croquette.dropped_by, victim);
        let offset = (croquette.to - victim_at).with_y(0.0);
        // Behind it (the kart looks down -Z), about 2.5 m away, on the ground.
        assert!(offset.z > 1.0, "{offset}");
        assert!(
            (offset.length() - tuning.scatter_distance).abs() < 0.1,
            "{offset}"
        );
        assert!(
            (croquette.to.y - tuning.loose_rest_height).abs() < 0.05,
            "{}",
            croquette.to
        );
    }

    // The other kart waits on the first one's landing point: taken as soon as allowed (0.25 s).
    // The victim waits on the second one's: only after 1 s.
    let (first, first_croquette) = loose[0];
    let (second, second_croquette) = loose[1];
    let (third, _) = loose[2];
    arena.teleport(other, first_croquette.to.x, first_croquette.to.z);
    arena.teleport(victim, second_croquette.to.x, second_croquette.to.z);
    let exists = |arena: &Arena, e: Entity| arena.app.world().get_entity(e).is_ok();
    let mut other_took = None;
    let mut victim_took = None;
    for tick in 1..=90 {
        arena.tick_with(&[
            (other, KartInput::default()),
            (victim, KartInput::default()),
        ]);
        if other_took.is_none() && !exists(&arena, first) {
            other_took = Some(tick);
        }
        if victim_took.is_none() && !exists(&arena, second) {
            victim_took = Some(tick);
        }
    }
    let at = |seconds: f32| (seconds * TICKS_PER_SECOND).round() as u32;
    let other_took = other_took.expect("the other kart never took it");
    let victim_took = victim_took.expect("the victim never took it back");
    // Caught at the end of its flight, as soon as it comes down within reach.
    assert!(
        (at(tuning.loose_pickup_delay)..=at(tuning.scatter_flight_time) + 1).contains(&other_took),
        "{other_took}"
    );
    assert!(
        (at(tuning.owner_pickup_delay)..=at(tuning.owner_pickup_delay) + 1).contains(&victim_took),
        "{victim_took}"
    );
    assert_eq!(arena.held(other), 1);
    assert_eq!(arena.held(victim), 3);

    // Nobody takes the third one: it vanishes after 8 s.
    assert!(exists(&arena, third));
    let lifetime = at(tuning.loose_lifetime);
    for _ in 91..lifetime - 2 {
        arena.tick(KartInput::default());
    }
    assert!(exists(&arena, third), "still there");
    for _ in 0..4 {
        arena.tick(KartInput::default());
    }
    assert!(!exists(&arena, third), "vanished");

    // Hit again with 3 left... well, 3: all gone. Hit with none: nothing, never below 0.
    arena.app.world_mut().write_message(hit);
    arena.tick(KartInput::default());
    assert_eq!(arena.held(victim), 0);
    assert_eq!(arena.loose().len(), 3);
    arena.app.world_mut().write_message(hit);
    arena.tick(KartInput::default());
    assert_eq!(arena.held(victim), 0);
    assert_eq!(arena.loose().len(), 3, "nothing more to lose");
    assert_eq!(lost_inbox.read(&arena.app).len(), 1);
}

/// GD § 7 acceptance: rear-ended by another kart, a kart loses 1 croquette (once for the whole
/// collision); the kart that bumped it loses nothing. Its croquette flies out behind it, under
/// the nose of the bumper, which can grab it.
#[test]
fn a_bumped_kart_loses_one_croquette() {
    let mut arena = Arena::new();
    let bumper = arena.kart;
    let victim = arena.other_kart(Transform::from_xyz(0.0, 0.55, -12.0));
    arena.set_held(bumper, 4);
    arena.set_held(victim, 4);
    let mut inbox = Inbox::<CroquettesLost>::new(&arena.app);
    let mut lost = Vec::new();
    for _ in 0..(2.0 * TICKS_PER_SECOND) as usize {
        arena.tick_with(&[(bumper, FULL_THROTTLE), (victim, KartInput::default())]);
        lost.extend(inbox.read(&arena.app));
    }
    // It was pushed along: they did collide.
    let victim_z = arena
        .app
        .world()
        .get::<Transform>(victim)
        .unwrap()
        .translation
        .z;
    assert!(victim_z < -14.0, "{victim_z}");
    assert_eq!(lost.len(), 1, "{lost:?}");
    assert_eq!(lost[0].kart, victim);
    assert_eq!(lost[0].cause, LossCause::Bump);
    assert_eq!(arena.held(victim), 3);
    // The bumper: its own 4, plus the victim's croquette if it caught it (it drove right over
    // it).
    let bumper_held = arena.held(bumper);
    assert!((4..=5).contains(&bumper_held), "{bumper_held}");
    assert_eq!(
        bumper_held + arena.held(victim) + arena.loose().len() as u8,
        8
    );
}

/// GD § 2: walls only bounce the kart: hitting one head on at full speed costs no croquette.
#[test]
fn hitting_a_wall_costs_no_croquette() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let world = arena.app.world_mut();
    let mut commands = world.commands();
    spawn_static_box(
        &mut commands,
        Transform::from_xyz(0.0, 1.0, -40.0),
        Vec3::new(10.0, 1.0, 0.5),
        Surface::Wall,
        WALL_RESTITUTION,
    );
    world.flush();
    arena.set_held(kart, 5);
    let mut lost = Inbox::<CroquettesLost>::new(&arena.app);
    let mut bounced = false;
    for _ in 0..(4.0 * TICKS_PER_SECOND) as usize {
        arena.tick(FULL_THROTTLE);
        bounced |= arena.forward_speed(kart) < -1.0;
        assert!(lost.read(&arena.app).is_empty());
    }
    assert!(bounced, "never hit the wall");
    assert_eq!(arena.held(kart), 5);
}

// --- The whole game on Prairie Pompon --------------------------------------------------------

mod game {
    use bevy::prelude::*;

    use super::*;
    use crate::audio::SoundEffect;
    use crate::race::{Race, RacePhase};
    use crate::tests::{app_on, hold_keys, player};
    use crate::track::{TrackChoice, TrackData, TrackHandle, TrackLayout};
    use crate::ui::{CroquetteCounter, HudText};

    fn hud_text(app: &mut App, field: HudText) -> String {
        let world = app.world_mut();
        world
            .query::<(&HudText, &Text)>()
            .iter(world)
            .find(|(f, _)| **f == field)
            .map(|(_, t)| t.0.clone())
            .unwrap()
    }

    fn counter_scale(app: &mut App) -> f32 {
        let world = app.world_mut();
        world
            .query_filtered::<&UiTransform, With<CroquetteCounter>>()
            .single(world)
            .unwrap()
            .scale
            .x
    }

    fn race(app: &App) -> &Race {
        app.world().resource::<Race>()
    }

    /// Issue #14 on the real circuit: the croquettes of the file are on the track, dressed;
    /// the player drives over the first line with the keyboard: the HUD counts them with a
    /// "pop", each one plays the pick-up sound and bursts; taken croquettes disappear and come
    /// back 10 s later; a restart empties the pockets and puts every croquette back.
    #[test]
    fn the_player_picks_up_the_croquettes_of_the_circuit() {
        let mut app = app_on(TrackChoice::default());
        let kart = player(&mut app);
        for _ in 0..10 {
            if app.world().get_resource::<Race>().is_some() {
                break;
            }
            app.update();
        }
        app.update();
        let handle = app.world().resource::<TrackHandle>().0.clone();
        let data: TrackData = app
            .world()
            .resource::<Assets<TrackData>>()
            .get(&handle)
            .unwrap()
            .clone();
        let expected: u32 = data.croquettes.iter().map(|line| line.count).sum();
        assert!(expected >= 20, "{expected} croquettes in the file");
        let world = app.world_mut();
        let croquettes: Vec<(Entity, Vec3)> = world
            .query_filtered::<(Entity, &Transform), (With<TrackCroquette>, With<Mesh3d>)>()
            .iter(world)
            .map(|(e, t)| (e, t.translation))
            .collect();
        assert_eq!(croquettes.len() as u32, expected, "all there, with a mesh");
        // None on the grid: nobody starts with a croquette.
        let layout = app.world().resource::<TrackLayout>().clone();
        for slot in &layout.grid {
            for (_, at) in &croquettes {
                assert!(
                    at.distance(slot.translation) > 6.0,
                    "croquette at {at} on the grid"
                );
            }
        }
        assert_eq!(hud_text(&mut app, HudText::Croquettes), "x 0");

        // GO, then the keyboard: full throttle, steering toward the croquettes of the first
        // line (on the right of the start straight).
        while !race(&app).started() {
            app.update();
        }
        let start = app.world().get::<Transform>(kart).unwrap().translation;
        // Issue #108: on the start straight only (other legs of the circuit run beside it).
        let mut line: Vec<(Entity, Vec3)> = croquettes
            .iter()
            .copied()
            .filter(|(_, at)| at.z < start.z && at.z > start.z - 40.0 && at.x > 1.0 && at.x < 8.0)
            .collect();
        line.sort_by(|a, b| b.1.z.total_cmp(&a.1.z));
        assert_eq!(line.len(), 5, "the first line");
        // Follows the line: aims 6 m ahead on it.
        let line_x = line[0].1.x;
        let mut pops = 0;
        let mut biggest_scale = 1.0f32;
        let mut shown = Vec::new();
        for _ in 0..(5.0 * TICKS_PER_SECOND) as usize {
            let t = *app.world().get::<Transform>(kart).unwrap();
            let target = Vec3::new(line_x, t.translation.y, t.translation.z - 6.0);
            let to = target - t.translation;
            let angle = to.dot(*t.right()).atan2(to.dot(*t.forward()));
            let mut keys = vec![KeyCode::ArrowUp];
            if angle > 0.03 {
                keys.push(KeyCode::ArrowRight);
            } else if angle < -0.03 {
                keys.push(KeyCode::ArrowLeft);
            }
            hold_keys(&mut app, &keys);
            app.update();
            let text = hud_text(&mut app, HudText::Croquettes);
            if shown.last() != Some(&text) {
                shown.push(text);
            }
            biggest_scale = biggest_scale.max(counter_scale(&mut app));
            let world = app.world_mut();
            pops = pops.max(
                world
                    .query::<&SoundEffect>()
                    .iter(world)
                    .filter(|s| **s == SoundEffect::CroquettePop)
                    .count(),
            );
            if app.world().get::<Croquettes>(kart).unwrap().count == 5 {
                break;
            }
            let passed = t.translation.z < line.last().unwrap().1.z - 3.0;
            assert!(!passed, "missed the line: {shown:?}");
        }
        assert_eq!(shown, ["x 0", "x 1", "x 2", "x 3", "x 4", "x 5"]);
        assert_eq!(pops, 5, "one pick-up sound per croquette");
        assert!(biggest_scale > 1.2, "the counter pops: {biggest_scale}");
        let world = app.world_mut();
        let bursts = world.query::<&CroquetteBurst>().iter(world).count();
        assert!(bursts > 0, "a burst where the last one was");
        for (croquette, _) in &line {
            assert_eq!(
                app.world().get::<Visibility>(*croquette),
                Some(&Visibility::Hidden)
            );
        }
        // The pop settles; the line comes back 10 s after.
        for _ in 0..30 {
            app.update();
        }
        assert!((counter_scale(&mut app) - 1.0).abs() < 1e-3);
        let first = line[0].0;
        let mut ticks = 0;
        while !app
            .world()
            .get::<TrackCroquette>(first)
            .unwrap()
            .available()
        {
            ticks += 1;
            assert!(ticks < 11 * 60, "never came back");
            hold_keys(&mut app, &[]);
            app.update();
        }
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(first),
            Some(&Visibility::Inherited)
        );

        // Take the second line too, then restart (as if the race were over): pockets emptied,
        // every croquette back.
        // Issue #108: the line on the left of the start straight (the start line is at z = 0
        // since Prairie Pompon was redrawn; 40 before).
        let second = croquettes
            .iter()
            .find(|(_, at)| at.x < -1.0 && at.x > -8.0 && at.z < 0.0 && at.z > -70.0)
            .map(|(e, _)| *e)
            .unwrap();
        app.world_mut()
            .get_mut::<TrackCroquette>(second)
            .unwrap()
            .take(&CroquetteTuning::default());
        let over_at = race(&app).flow.tick;
        app.world_mut().resource_mut::<Race>().flow.over_at = Some(over_at);
        while !race(&app).flow.can_restart(&race(&app).timings) {
            app.update();
        }
        assert_eq!(race(&app).phase(), RacePhase::Results);
        app.world_mut().resource_mut::<Race>().restart_requested = true;
        app.update();
        assert_eq!(race(&app).flow.tick, 1);
        assert_eq!(app.world().get::<Croquettes>(kart).unwrap().count, 0);
        assert!(
            app.world()
                .get::<TrackCroquette>(second)
                .unwrap()
                .available()
        );
        assert_eq!(hud_text(&mut app, HudText::Croquettes), "x 0");
    }
}

/// Issue #15 (GD § 8): hit by an item, a kart is invincible for 1 to 2 s depending on its
/// Invincibility stat (1.5 s in the middle): another item hit meanwhile costs no croquette, and
/// does not extend the protection. Bumps are not concerned (their own 1 s grace, GD § 15).
///
/// Issue #16 moved the protection into the item rules: the hit goes through them (`ItemHit`,
/// the victim decides), the kart spins out first (1 s, immune), then its invincibility starts
/// (1, 1.5 or 2 s with its stat). Same three karts and the same checks, on that timeline.
#[test]
fn after_an_item_hit_the_invincibility_stat_protects_the_kart() {
    use crate::items::{ItemHit, ItemKind};
    let mut arena = Arena::with_items();
    let middle = arena.kart;
    let fragile = arena.other_kart(Transform::from_xyz(20.0, 0.55, 0.0));
    let sturdy = arena.other_kart(Transform::from_xyz(-20.0, 0.55, 0.0));
    let tuning = StatsTuning::default();
    for (kart, level) in [(fragile, 0), (sturdy, 20)] {
        let stats = KartStats::default().with(Stat::Invincibility, level);
        arena
            .app
            .world_mut()
            .entity_mut(kart)
            .insert((stats, StatEffects::new(&stats, &tuning)));
    }
    let karts = [fragile, middle, sturdy];
    for kart in karts {
        arena.set_held(kart, 10);
    }
    let hit_all = |arena: &mut Arena| {
        for kart in karts {
            arena.app.world_mut().write_message(ItemHit {
                kart,
                by: Entity::PLACEHOLDER,
                item: ItemKind::Pelure,
                object: None,
            });
        }
        arena.tick(KartInput::default());
        karts.map(|kart| arena.held(kart))
    };
    let spinning = |arena: &Arena, kart| arena.app.world().get::<KartState>(kart).unwrap().is_hit();
    let remaining = |arena: &Arena, kart| {
        arena
            .app
            .world()
            .get::<Invincibility>(kart)
            .unwrap()
            .remaining
    };
    // t = 0: everybody loses 3 and spins out for 1 s.
    assert_eq!(hit_all(&mut arena), [7, 7, 7]);
    assert!(karts.iter().all(|k| spinning(&arena, *k)));
    // t = 0.5 s: spinning, nobody is hit again.
    arena.run(0.5 - 2.0 / TICKS_PER_SECOND, KartInput::default());
    assert_eq!(hit_all(&mut arena), [7, 7, 7]);
    // The spin ends (60 ticks): invincible for 1, 1.5 and 2 s.
    let mut ticks = 30;
    while spinning(&arena, middle) {
        arena.tick(KartInput::default());
        ticks += 1;
    }
    assert_eq!(ticks, 60);
    assert!((remaining(&arena, fragile) - 1.0).abs() < 1e-6);
    assert!((remaining(&arena, middle) - 1.5).abs() < 1e-6);
    assert!((remaining(&arena, sturdy) - 2.0).abs() < 1e-6);
    // 1.2 s later: only the fragile one is hit again (it spins until 2.2 s).
    arena.run(1.2 - 1.0 / TICKS_PER_SECOND, KartInput::default());
    assert_eq!(hit_all(&mut arena), [4, 7, 7]);
    // 1.7 s: the middle one.
    arena.run(0.5 - 1.0 / TICKS_PER_SECOND, KartInput::default());
    assert_eq!(hit_all(&mut arena), [4, 4, 7]);
    // 2.1 s: the sturdy one (the other two still spin).
    arena.run(0.4 - 1.0 / TICKS_PER_SECOND, KartInput::default());
    assert_eq!(hit_all(&mut arena), [4, 4, 4]);
}
