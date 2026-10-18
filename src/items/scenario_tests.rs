//! Headless item scenarios (issue #16 acceptance): the real simulation (Rapier, karts) with the
//! croquette and item rules on a flat slab, one fixed tick per `app.update()`; then the whole
//! game on Prairie Pompon (race, abeilles along the track, keyboard, HUD).

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::croquettes::CroquettePlugin;
use crate::interpolation::PhysicsPose;
use crate::kart::{DriftSide, KartTuning, kart_body};
use crate::track::{ITEM_BOX_HEIGHT, Surface, WALL_RESTITUTION, spawn_static_box};

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
const ITEM: KartInput = KartInput {
    throttle: 0.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: true,
    look_back: false,
    interact: false,
};

/// Karts on a large flat slab with the croquette and item rules (no race: the items are drawn
/// from the leader's row).
struct Arena {
    app: App,
    kart: Entity,
}

impl Arena {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            SimulationPlugin,
            CroquettePlugin,
            ItemPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
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
        arena.run(0.5, &[]);
        arena
    }

    fn spawn_kart(world: &mut World, transform: Transform) -> Entity {
        let tuning = world.resource::<KartTuning>().clone();
        world.spawn(kart_body(&tuning, transform)).id()
    }

    /// Another kart at `transform` (settled on its wheels).
    fn other_kart(&mut self, transform: Transform) -> Entity {
        let kart = Self::spawn_kart(self.app.world_mut(), transform);
        self.app.update();
        kart
    }

    /// A wall: a box of half size `half` at `at`.
    fn wall(&mut self, at: Vec3, half: Vec3) {
        let world = self.app.world_mut();
        let mut commands = world.commands();
        spawn_static_box(
            &mut commands,
            Transform::from_translation(at),
            half,
            Surface::Wall,
            WALL_RESTITUTION,
        );
        world.flush();
    }

    /// An item box at `(x, z)`.
    fn item_box(&mut self, x: f32, z: f32, double: bool) -> Entity {
        self.app
            .world_mut()
            .spawn((
                ItemBoxSpot { double },
                Transform::from_xyz(x, ITEM_BOX_HEIGHT, z),
            ))
            .id()
    }

    /// One tick with these inputs (the other karts keep theirs).
    fn tick(&mut self, inputs: &[(Entity, KartInput)]) {
        for (kart, input) in inputs {
            *self.app.world_mut().get_mut::<KartInput>(*kart).unwrap() = *input;
        }
        self.app.update();
    }

    fn run(&mut self, seconds: f32, inputs: &[(Entity, KartInput)]) {
        for _ in 0..(seconds * TICKS_PER_SECOND).round() as usize {
            self.tick(inputs);
        }
    }

    /// A press of the item button (down one tick, up the next), with `input` otherwise.
    fn tap(&mut self, kart: Entity, input: KartInput) {
        self.tick(&[(
            kart,
            KartInput {
                item: true,
                ..input
            },
        )]);
        self.tick(&[(
            kart,
            KartInput {
                item: false,
                ..input
            },
        )]);
    }

    fn give(&mut self, kart: Entity, item: ItemKind, count: u8) {
        let mut slots = self.app.world_mut().get_mut::<ItemSlots>(kart).unwrap();
        assert!(slots.give(item, count, 0));
    }

    fn slots(&self, kart: Entity) -> ItemSlots {
        *self.app.world().get::<ItemSlots>(kart).unwrap()
    }

    fn stats(&self, kart: Entity) -> ItemStats {
        *self.app.world().get::<ItemStats>(kart).unwrap()
    }

    fn state(&self, kart: Entity) -> KartState {
        *self.app.world().get::<KartState>(kart).unwrap()
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

    fn invincibility(&self, kart: Entity) -> Invincibility {
        *self.app.world().get::<Invincibility>(kart).unwrap()
    }

    fn transform(&self, kart: Entity) -> Transform {
        *self.app.world().get::<Transform>(kart).unwrap()
    }

    fn forward_speed(&self, kart: Entity) -> f32 {
        let world = self.app.world();
        let forward = *world.get::<Transform>(kart).unwrap().forward();
        world.get::<Velocity>(kart).unwrap().linear.dot(forward)
    }

    /// How many items are out (held out too).
    fn objects_len(&self) -> usize {
        let world = self.app.world();
        world
            .try_query::<&ItemObject>()
            .map_or(0, |mut query| query.iter(world).count())
    }

    /// Every item out, by spawn order.
    fn objects(&mut self) -> Vec<(Entity, ItemObject)> {
        let world = self.app.world_mut();
        let mut all: Vec<(Entity, ItemObject)> = world
            .query::<(Entity, &ItemObject)>()
            .iter(world)
            .map(|(e, o)| (e, *o))
            .collect();
        all.sort_by_key(|(e, _)| *e);
        all
    }

    /// Puts `kart` at `transform`, stopped.
    fn place(&mut self, kart: Entity, transform: Transform) {
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(kart).unwrap() = transform;
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
        world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    }
}

/// Acceptance criterion of #16: a box breaks under a kart and gives it an item, which spins in
/// the roulette for about 1.5 s (90 ticks) before it can be used; the box comes back after about
/// 2 s (120 ticks). A double box gives two items; with both slots taken, a box breaks for nothing.
#[test]
fn a_box_gives_an_item_after_the_roulette_and_comes_back_after_two_seconds() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let item_box = arena.item_box(0.0, -12.0, false);
    let available = |arena: &Arena| {
        arena
            .app
            .world()
            .get::<ItemBox>(item_box)
            .unwrap()
            .available()
    };
    let mut ticks = 0;
    while available(&arena) {
        arena.tick(&[(kart, FULL_THROTTLE)]);
        ticks += 1;
        assert!(ticks < 120, "never reached the box");
    }
    let slots = arena.slots(kart);
    assert!(
        matches!(slots.hand, Slot::Spinning { ticks: 90, .. }),
        "{slots:?}"
    );
    assert!(slots.reserve.is_empty());
    assert_eq!(arena.stats(kart).boxes, 1);
    // Off a race the draw is the leader's row: pelure, pelote, croquette dorée or pelures x3.
    let (item, _) = slots.hand.item().unwrap();
    assert!(
        matches!(
            item,
            ItemKind::Pelure | ItemKind::Pelote | ItemKind::CroquetteDoree
        ),
        "{item:?}"
    );
    // Brake to a stop, far past the box.
    let stop = KartInput {
        brake: 1.0,
        ..default()
    };
    for tick in 1..=120 {
        arena.tick(&[(kart, stop)]);
        let spinning = matches!(arena.slots(kart).hand, Slot::Spinning { .. });
        assert_eq!(spinning, tick < 90, "tick {tick}");
        assert_eq!(available(&arena), tick >= 120, "tick {tick}");
    }
    assert!(matches!(arena.slots(kart).hand, Slot::Ready { .. }));

    // A double box: both slots (the hand first emptied).
    arena
        .app
        .world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .clear();
    let double = arena.item_box(0.0, -60.0, true);
    arena.place(kart, Transform::from_xyz(0.0, 0.55, -57.0));
    arena.run(0.5, &[(kart, FULL_THROTTLE)]);
    assert!(
        !arena
            .app
            .world()
            .get::<ItemBox>(double)
            .unwrap()
            .available()
    );
    let slots = arena.slots(kart);
    assert!(
        !slots.hand.is_empty() && !slots.reserve.is_empty(),
        "{slots:?}"
    );
    assert_eq!(slots.draws, 2);
    // Both taken: the next box breaks, the slots stay the same, no draw.
    let third = arena.item_box(0.0, -100.0, false);
    arena.place(kart, Transform::from_xyz(0.0, 0.55, -97.0));
    arena.run(0.5, &[(kart, FULL_THROTTLE)]);
    assert!(!arena.app.world().get::<ItemBox>(third).unwrap().available());
    let after = arena.slots(kart);
    assert_eq!(after.draws, 2);
    assert_eq!(
        (after.hand.item(), after.reserve.item()),
        (slots.hand.item(), slots.reserve.item())
    );
    assert_eq!(arena.stats(kart).boxes, 3);
}

/// Issue #102: the kart runs into a box at `(x, z)` (placed on it, for one tick with `input`),
/// then is put 10 m aside (the box does not give again when it comes back). Returns the box.
fn take_box(arena: &mut Arena, x: f32, z: f32, double: bool, input: KartInput) -> Entity {
    let kart = arena.kart;
    let item_box = arena.item_box(x, z, double);
    arena.place(kart, Transform::from_xyz(x, 0.55, z));
    arena.tick(&[(kart, input)]);
    let world = arena.app.world();
    assert!(!world.get::<ItemBox>(item_box).unwrap().available());
    arena.place(kart, Transform::from_xyz(x + 10.0, 0.55, z));
    item_box
}

/// Issue #102 point 1: every box spins a roulette of 90 ticks (about 1.5 s) before its item can
/// be used, wherever it lands: the hand (the item button pressed meanwhile does nothing), the
/// reserve (the hand used meanwhile: the reserve's roulette moves into it and goes on), both
/// slots of a double box, and a box taken while a pelure is dragged or while an "x3" is out.
#[test]
fn every_box_spins_its_roulette_before_its_item_can_be_used() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let idle = KartInput::default();
    let spinning = |slot: Slot| matches!(slot, Slot::Spinning { .. });
    let used = |arena: &Arena| (arena.stats(kart).used, arena.objects_len());

    // The hand: pressed every other tick through the roulette, nothing comes out.
    take_box(&mut arena, 0.0, -20.0, false, idle);
    assert!(matches!(
        arena.slots(kart).hand,
        Slot::Spinning { ticks: 90, .. }
    ));
    let before = used(&arena);
    for tick in 1..=90 {
        let pressed = KartInput {
            item: tick % 2 == 1,
            ..idle
        };
        arena.tick(&[(kart, pressed)]);
        assert_eq!(spinning(arena.slots(kart).hand), tick < 90, "tick {tick}");
        if tick < 90 {
            assert_eq!(used(&arena), before, "tick {tick}: nothing used");
        }
    }
    arena.tick(&[(kart, idle)]);

    // The reserve, a pelote ready in the hand: thrown on the 20th tick, the reserve's roulette
    // moves into the hand and goes on; ready 90 ticks after the box all the same.
    arena
        .app
        .world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .clear();
    arena.give(kart, ItemKind::Pelote, 1);
    take_box(&mut arena, 0.0, -60.0, false, idle);
    let slots = arena.slots(kart);
    assert!(matches!(
        slots.hand,
        Slot::Ready {
            item: ItemKind::Pelote,
            ..
        }
    ));
    assert!(
        matches!(slots.reserve, Slot::Spinning { ticks: 90, .. }),
        "{slots:?}"
    );
    for tick in 1..=90 {
        let input = KartInput {
            item: tick == 20 || tick == 40,
            ..idle
        };
        arena.tick(&[(kart, input)]);
        let slots = arena.slots(kart);
        match tick {
            1..20 => assert!(spinning(slots.reserve), "tick {tick}"),
            20..90 => {
                assert!(slots.reserve.is_empty(), "tick {tick}: moved into the hand");
                assert!(spinning(slots.hand), "tick {tick}: {slots:?}");
            }
            _ => assert!(matches!(slots.hand, Slot::Ready { .. }), "{slots:?}"),
        }
    }
    // The pelote went (its press, then its release), the press on the 40th tick did nothing.
    assert_eq!(arena.stats(kart).used, before.0 + 1);

    // A double box: both slots spin 90 ticks.
    arena
        .app
        .world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .clear();
    take_box(&mut arena, 0.0, -100.0, true, idle);
    for tick in 1..=90 {
        let slots = arena.slots(kart);
        assert!(
            spinning(slots.hand) && spinning(slots.reserve),
            "tick {tick}"
        );
        arena.tick(&[(kart, idle)]);
    }
    let slots = arena.slots(kart);
    assert!(
        matches!(
            (slots.hand, slots.reserve),
            (Slot::Ready { .. }, Slot::Ready { .. })
        ),
        "{slots:?}"
    );

    // A pelure dragged (the item button held) into a box: the box's item spins in the hand, the
    // pelure is dropped at the release meanwhile, the item ready at the 90th tick.
    arena
        .app
        .world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .clear();
    arena.give(kart, ItemKind::Pelure, 1);
    arena.tick(&[(kart, ITEM)]);
    assert_eq!(arena.slots(kart).dragging, Some(ItemKind::Pelure));
    take_box(&mut arena, 0.0, -140.0, false, ITEM);
    let slots = arena.slots(kart);
    assert_eq!(slots.dragging, Some(ItemKind::Pelure));
    assert!(
        matches!(slots.hand, Slot::Spinning { ticks: 90, .. }),
        "{slots:?}"
    );
    for tick in 1..=90 {
        let input = if tick < 10 { ITEM } else { idle };
        arena.tick(&[(kart, input)]);
        let slots = arena.slots(kart);
        assert_eq!(slots.dragging.is_some(), tick < 10, "tick {tick}");
        assert_eq!(spinning(slots.hand), tick < 90, "tick {tick}");
    }

    // An "x3" out (three pelures behind): the box's item spins in the reserve.
    arena
        .app
        .world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .clear();
    arena.give(kart, ItemKind::Pelure, 3);
    arena.tap(kart, idle);
    assert!(arena.slots(kart).deployed);
    take_box(&mut arena, 0.0, -180.0, false, idle);
    let slots = arena.slots(kart);
    assert!(slots.deployed);
    assert!(
        matches!(slots.reserve, Slot::Spinning { ticks: 90, .. }),
        "{slots:?}"
    );
}

/// Acceptance criterion of #16: a pelure dragged behind a kart (item button held) stops a pelote
/// thrown at it from behind: both break, the kart is not hit. Without the pelure, the same pelote
/// hits it (a tumble).
#[test]
fn a_dragged_pelure_blocks_a_pelote_from_behind() {
    for shield in [true, false] {
        let mut arena = Arena::new();
        let front = arena.kart;
        let back = arena.other_kart(Transform::from_xyz(0.0, 0.55, 25.0));
        arena.run(0.3, &[]);
        arena.give(back, ItemKind::Pelote, 1);
        if shield {
            arena.give(front, ItemKind::Pelure, 1);
            arena.tick(&[(front, ITEM)]);
            assert_eq!(arena.slots(front).dragging, Some(ItemKind::Pelure));
            let dragged = arena.objects();
            assert_eq!(dragged.len(), 1);
            assert_eq!(dragged[0].1.held(), Some(Hold::Dragged));
            // Behind the kart, on the ground.
            let local = arena
                .transform(front)
                .compute_affine()
                .inverse()
                .transform_point3(dragged[0].1.position);
            assert!(local.z > 1.4 && local.y < -0.05, "{local}");
        }
        // The kart behind throws its pelote forward, at the front kart (25 m ahead).
        let hold = if shield { ITEM } else { KartInput::default() };
        arena.tick(&[(back, ITEM), (front, hold)]);
        arena.tick(&[(back, KartInput::default()), (front, hold)]);
        arena.run(1.0, &[(front, hold)]);
        let objects = arena.objects();
        if shield {
            assert_eq!(arena.state(front), KartState::Grounded, "the shield held");
            assert!(objects.is_empty(), "{objects:?}");
            assert_eq!(arena.slots(front).dragging, None);
            assert_eq!(arena.stats(front).blocked, 1);
            assert_eq!(arena.stats(front).hits_taken, 0);
            // Releasing the button now drops nothing.
            arena.run(0.5, &[(front, KartInput::default())]);
            assert!(arena.objects().is_empty());
        } else {
            let state = arena.state(front);
            assert!(
                matches!(
                    state,
                    KartState::Hit {
                        reaction: HitReaction::Tumble,
                        ..
                    }
                ),
                "{state:?}"
            );
            assert_eq!(arena.stats(front).hits_taken, 1);
            assert_eq!(arena.stats(back).hits_landed, 1);
            assert!(objects.is_empty());
        }
    }
}

/// Acceptance criterion of #16 (GD § 8): a pelure spins a kart out for about 1 s (60 ticks), a
/// pelote makes it tumble for about 1.6 s (96 ticks), slowing it down hard and breaking its drift;
/// it loses 3 croquettes; then it is invincible for 1.5 s (its Invincibility stat, in the
/// middle), and blinks: no item hits it meanwhile (the item breaks on it all the same).
#[test]
fn a_hit_spins_or_tumbles_the_kart_then_it_is_invincible() {
    for (item, reaction, ticks) in [
        (ItemKind::Pelure, HitReaction::Spin, 60),
        (ItemKind::Pelote, HitReaction::Tumble, 96),
    ] {
        let mut arena = Arena::new();
        let kart = arena.kart;
        arena.set_held(kart, 10);
        // Flat out, then into a right drift.
        arena.run(2.0, &[(kart, FULL_THROTTLE)]);
        let drift = KartInput {
            steer: 1.0,
            jump_drift: true,
            ..FULL_THROTTLE
        };
        arena.run(0.6, &[(kart, drift)]);
        assert!(
            matches!(
                arena.state(kart),
                KartState::Drifting {
                    side: DriftSide::Right,
                    ..
                }
            ),
            "{:?}",
            arena.state(kart)
        );
        let speed = arena.forward_speed(kart);
        assert!(speed > 20.0, "{speed}");
        arena.app.world_mut().write_message(ItemHit {
            kart,
            by: Entity::PLACEHOLDER,
            item,
            object: None,
        });
        arena.tick(&[(kart, drift)]);
        assert_eq!(arena.held(kart), 7, "3 croquettes lost");
        assert!(!arena.boost(kart).is_active());
        let mut hit_ticks = 1;
        while arena.state(kart).is_hit() {
            assert!(
                matches!(arena.state(kart), KartState::Hit { reaction: r, .. } if r == reaction)
            );
            // Another item meanwhile: nothing.
            if hit_ticks == 30 {
                arena.app.world_mut().write_message(ItemHit {
                    kart,
                    by: Entity::PLACEHOLDER,
                    item: ItemKind::Pelote,
                    object: None,
                });
            }
            // The driver still presses: the kart does not listen.
            arena.tick(&[(kart, drift)]);
            hit_ticks += 1;
            assert!(hit_ticks < 200);
        }
        assert_eq!(hit_ticks, ticks, "{item:?}");
        assert_eq!(arena.held(kart), 7, "no hit while spinning");
        // It slowed down hard, and its drift is gone (the button still held: no new drift).
        let after = arena.forward_speed(kart);
        assert!(after < 0.4 * speed, "{after} from {speed}");
        assert!(!matches!(arena.state(kart), KartState::Drifting { .. }));
        // Invincible for 1.5 s: hits do nothing, the kart blinks.
        assert!((arena.invincibility(kart).remaining - 1.5).abs() < 1e-5);
        let mut blinks = 0;
        let mut shown_before = true;
        for tick in 0..85 {
            if tick % 20 == 0 {
                arena.app.world_mut().write_message(ItemHit {
                    kart,
                    by: Entity::PLACEHOLDER,
                    item: ItemKind::Pelure,
                    object: None,
                });
            }
            arena.tick(&[(kart, FULL_THROTTLE)]);
            assert!(!arena.state(kart).is_hit(), "tick {tick}");
            let shown = crate::kart::shown_while_invincible(
                &arena.invincibility(kart),
                tick as f32 / TICKS_PER_SECOND,
            );
            if shown != shown_before {
                blinks += 1;
            }
            shown_before = shown;
        }
        assert_eq!(arena.held(kart), 7);
        assert!(blinks >= 10, "{blinks}");
        // Then it can be hit again.
        arena.run(0.1, &[(kart, FULL_THROTTLE)]);
        assert!(!arena.invincibility(kart).is_active());
        arena.app.world_mut().write_message(ItemHit {
            kart,
            by: Entity::PLACEHOLDER,
            item: ItemKind::Pelure,
            object: None,
        });
        arena.tick(&[(kart, FULL_THROTTLE)]);
        assert!(arena.state(kart).is_hit());
        assert_eq!(arena.held(kart), 4);
        assert_eq!(arena.stats(kart).hits_taken, 2);
    }
}

/// Acceptance criterion of #16: a pelure dropped on the ground spins out the next kart running
/// over it (and goes away); its owner can run over it too once it is out of its way.
#[test]
fn a_pelure_on_the_ground_spins_out_whoever_runs_over_it() {
    let mut arena = Arena::new();
    let dropper = arena.kart;
    let other = arena.other_kart(Transform::from_xyz(0.0, 0.55, 30.0));
    arena.give(dropper, ItemKind::Pelure, 1);
    arena.tap(dropper, KartInput::default());
    let objects = arena.objects();
    assert_eq!(objects.len(), 1);
    let (_, pelure) = objects[0];
    assert_eq!(pelure.motion, Motion::Ground);
    // Behind the kart, on the ground.
    assert!(pelure.position.z > 1.5, "{}", pelure.position);
    // Flat on the slab (its top at y = 0).
    let lift = ItemTuning::default().pelure_radius * 0.5;
    assert!(
        (pelure.position.y - lift).abs() < 0.05,
        "{}",
        pelure.position
    );
    assert_eq!(arena.stats(dropper).used, 1);
    // The kart behind drives into it.
    let mut ticks = 0;
    while !arena.state(other).is_hit() {
        arena.tick(&[(other, FULL_THROTTLE)]);
        ticks += 1;
        assert!(ticks < 240, "never hit");
    }
    assert!(matches!(
        arena.state(other),
        KartState::Hit {
            reaction: HitReaction::Spin,
            ..
        }
    ));
    assert!(arena.objects().is_empty());
    assert_eq!(arena.stats(dropper).hits_landed, 1);
}

/// Acceptance criterion of #16: thrown ahead by default, backwards with the "back" direction held
/// (the brake, or the look-back button); a pelote rolls at its speed, an abeille thrown backwards
/// rolls straight back.
#[test]
fn items_are_thrown_ahead_or_backwards() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let speed = ItemTuning::default().pelote_speed;
    let throws = [
        (ItemKind::Pelote, KartInput::default(), 1.0),
        (
            ItemKind::Pelote,
            KartInput {
                brake: 1.0,
                ..default()
            },
            -1.0,
        ),
        (
            ItemKind::Pelote,
            KartInput {
                look_back: true,
                ..default()
            },
            -1.0,
        ),
        (
            ItemKind::Abeille,
            KartInput {
                look_back: true,
                ..default()
            },
            -1.0,
        ),
    ];
    for (item, input, direction) in throws {
        arena.place(kart, Transform::from_xyz(0.0, 0.55, 0.0));
        arena.give(kart, item, 1);
        let before = arena.objects().len();
        arena.tap(kart, input);
        let objects = arena.objects();
        assert_eq!(objects.len(), before + 1, "{item:?} {input:?}");
        let (_, thrown) = *objects.last().unwrap();
        let forward = *arena.transform(kart).forward();
        let along = thrown.velocity.dot(forward);
        assert!(
            along * direction > 0.0,
            "{item:?} {input:?}: {}",
            thrown.velocity
        );
        assert!(
            (thrown.position - arena.transform(kart).translation).dot(forward) * direction > 1.0
        );
        if item == ItemKind::Pelote {
            assert!((thrown.velocity.length() - speed).abs() < 0.5);
        } else {
            assert!(matches!(
                thrown.motion,
                Motion::Rolling {
                    bounces_left: 0,
                    ..
                }
            ));
        }
        // Out of the way.
        for (entity, _) in &objects {
            arena.app.world_mut().entity_mut(*entity).despawn();
        }
    }
}

/// Acceptance criterion of #16: a pelote bounces off a wall (mirror reflection on the wall), and
/// rolls on; it breaks on the wall after its last bounce.
#[test]
fn a_pelote_bounces_off_a_wall() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    // A wall along z, 12 m to the right; the kart looks at it at 45°.
    arena.wall(Vec3::new(12.5, 0.5, 0.0), Vec3::new(0.5, 1.0, 100.0));
    let heading =
        Transform::from_xyz(0.0, 0.55, 0.0).looking_to(Vec3::new(1.0, 0.0, -1.0), Vec3::Y);
    arena.place(kart, heading);
    arena.run(0.2, &[]);
    arena.give(kart, ItemKind::Pelote, 1);
    arena.tap(kart, KartInput::default());
    let (pelote, thrown) = *arena.objects().last().unwrap();
    assert!(
        thrown.velocity.x > 20.0 && thrown.velocity.z < -20.0,
        "{}",
        thrown.velocity
    );
    let mut bounced = None;
    for tick in 0..60 {
        arena.tick(&[]);
        let Some((_, now)) = arena.objects().into_iter().find(|(e, _)| *e == pelote) else {
            panic!("the pelote broke at tick {tick}");
        };
        assert!(now.position.x <= 12.0 - 0.35, "{}", now.position);
        if now.velocity.x < 0.0 {
            bounced = Some(now);
            break;
        }
    }
    let now = bounced.expect("no bounce");
    // Mirror reflection: x flipped, z kept.
    assert!(
        (now.velocity.x + thrown.velocity.x).abs() < 0.5,
        "{}",
        now.velocity
    );
    assert!(
        (now.velocity.z - thrown.velocity.z).abs() < 0.5,
        "{}",
        now.velocity
    );
    let bounces = ItemTuning::default().pelote_bounces;
    assert!(
        matches!(now.motion, Motion::Rolling { bounces_left, .. } if bounces_left == bounces - 1)
    );
    // Thrown straight at a wall with no bounce left: it breaks there.
    let mut last = now;
    if let Motion::Rolling { fall, .. } = last.motion {
        last.motion = Motion::Rolling {
            bounces_left: 0,
            fall,
        };
    }
    last.velocity = Vec3::new(30.0, 0.0, 0.0);
    *arena.app.world_mut().get_mut::<ItemObject>(pelote).unwrap() = last;
    arena.run(1.0, &[]);
    assert!(
        arena.app.world().get_entity(pelote).is_err(),
        "broke on the wall"
    );
}

/// Acceptance criterion of #16 (GD § 8): a piment is a boost of about 1.2 s (+30 %) through the
/// kart's `Boost`, from the tick it is used; a "Piment x3" is three of them.
#[test]
fn a_piment_boosts_about_one_point_two_seconds() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    arena.give(kart, ItemKind::Piment, 3);
    arena.tick(&[(
        kart,
        KartInput {
            item: true,
            ..FULL_THROTTLE
        },
    )]);
    let boost = arena.boost(kart);
    assert_eq!(boost.strength, 0.3);
    assert!((boost.remaining - 1.2).abs() < 1e-6, "{boost:?}");
    // Active from the tick of the piment for 1.2 s: as many ticks as the kart's boost timer
    // counts (72, or 73 when the sum of the ticks' f32 durations falls just short of 1.2 s).
    let expected = {
        let mut timer = Boost::default();
        timer.add(0.3, 1.2);
        let mut ticks = 1;
        loop {
            timer.tick(1.0 / TICKS_PER_SECOND);
            if !timer.is_active() {
                break ticks;
            }
            ticks += 1;
        }
    };
    assert!((72..=73).contains(&expected), "{expected}");
    let mut active = 1;
    loop {
        arena.tick(&[(kart, FULL_THROTTLE)]);
        if !arena.boost(kart).is_active() {
            break;
        }
        active += 1;
        assert!(active < 200);
    }
    assert_eq!(active, expected, "1.2 s");
    assert_eq!(
        arena.slots(kart).hand,
        Slot::Ready {
            item: ItemKind::Piment,
            count: 2
        }
    );
    // Faster than the top speed meanwhile.
    arena.tick(&[(
        kart,
        KartInput {
            item: true,
            ..FULL_THROTTLE
        },
    )]);
    arena.run(1.0, &[(kart, FULL_THROTTLE)]);
    let top = KartTuning::default().max_speed;
    assert!(
        arena.forward_speed(kart) > 1.1 * top,
        "{}",
        arena.forward_speed(kart)
    );
    assert_eq!(arena.stats(kart).used, 2);
}

/// Acceptance criterion of #16 (GD § 8): a croquette dorée gives 2 croquettes, up to 10.
#[test]
fn a_croquette_doree_gives_two_croquettes_up_to_ten() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    for (before, after) in [(0, 2), (7, 9), (9, 10), (10, 10)] {
        arena.set_held(kart, before);
        arena.give(kart, ItemKind::CroquetteDoree, 1);
        arena.tap(kart, KartInput::default());
        assert_eq!(arena.held(kart), after, "from {before}");
        assert!(arena.slots(kart).hand.is_empty());
    }
    assert_eq!(arena.stats(kart).used, 4);
}

/// GD § 8: an "x3" of pelotes comes out turning around the kart, and shields it from every side;
/// each press throws one; a kart running into them is hit.
#[test]
fn an_x3_of_pelotes_turns_around_the_kart_and_goes_one_at_a_time() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    arena.give(kart, ItemKind::Pelote, 3);
    arena.tap(kart, KartInput::default());
    let around: Vec<ItemObject> = arena.objects().into_iter().map(|(_, o)| o).collect();
    assert_eq!(around.len(), 3);
    let center = arena.transform(kart).translation;
    let radius = ItemTuning::default().orbit_radius;
    for object in &around {
        assert!(matches!(object.held(), Some(Hold::Orbit(_))));
        let flat = (object.position - center).with_y(0.0).length();
        assert!((flat - radius).abs() < 0.05, "{flat}");
    }
    // They turn.
    arena.run(0.2, &[]);
    let later: Vec<Vec3> = arena.objects().iter().map(|(_, o)| o.position).collect();
    assert!(later[0].distance(around[0].position) > 0.5);
    // One press: one thrown, two still turning.
    arena.tap(kart, KartInput::default());
    let objects = arena.objects();
    let held = objects.iter().filter(|(_, o)| o.held().is_some()).count();
    let rolling = objects
        .iter()
        .filter(|(_, o)| matches!(o.motion, Motion::Rolling { .. }))
        .count();
    assert_eq!((held, rolling), (2, 1));
    assert_eq!(arena.slots(kart).held_out(), Some((ItemKind::Pelote, 2)));
    // A kart standing in the way of the turning pelotes (beside the kart, not touching it) is
    // hit within a turn.
    let other = arena.other_kart(Transform::from_xyz(-2.7, 0.55, 0.0).looking_to(Vec3::X, Vec3::Y));
    let mut ticks = 0;
    while !arena.state(other).is_hit() {
        arena.tick(&[]);
        ticks += 1;
        assert!(ticks < 90, "never hit");
    }
    assert!(!arena.state(kart).is_hit());
    assert_eq!(arena.slots(kart).held_out(), Some((ItemKind::Pelote, 1)));
}

/// `ItemTuning::enabled` off (the bots' driving tests, the admin panel): the boxes give nothing
/// and stay, the slots are emptied, the items out vanish.
#[test]
fn without_items_nothing_happens() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    arena.give(kart, ItemKind::Pelote, 3);
    arena.tap(kart, KartInput::default());
    assert_eq!(arena.objects().len(), 3);
    arena.app.world_mut().resource_mut::<ItemTuning>().enabled = false;
    arena.tick(&[]);
    assert!(arena.objects().is_empty());
    assert_eq!(arena.slots(kart), ItemSlots::default());
    let item_box = arena.item_box(0.0, -12.0, false);
    arena.run(2.0, &[(kart, FULL_THROTTLE)]);
    assert!(
        arena.transform(kart).translation.z < -15.0,
        "drove past the box"
    );
    assert!(
        arena
            .app
            .world()
            .get::<ItemBox>(item_box)
            .unwrap()
            .available()
    );
    assert_eq!(arena.slots(kart), ItemSlots::default());
}

// --- Phase 2 (issue #18) ----------------------------------------------------------------------

impl Arena {
    fn effects(&self, kart: Entity) -> ItemEffects {
        *self.app.world().get::<ItemEffects>(kart).unwrap()
    }

    fn factors(&self, kart: Entity) -> MaxSpeedFactors {
        *self.app.world().get::<MaxSpeedFactors>(kart).unwrap()
    }

    fn set_invincible(&mut self, kart: Entity, seconds: f32) {
        self.app
            .world_mut()
            .get_mut::<Invincibility>(kart)
            .unwrap()
            .remaining = seconds;
    }

    fn hit(&mut self, kart: Entity, item: ItemKind) {
        self.app.world_mut().write_message(ItemHit {
            kart,
            by: Entity::PLACEHOLDER,
            item,
            object: None,
        });
    }

    /// Ticks until `done` (at most `seconds`), the ticks it took.
    fn until(
        &mut self,
        seconds: f32,
        inputs: &[(Entity, KartInput)],
        done: impl Fn(&Self) -> bool,
    ) -> usize {
        let most = (seconds * TICKS_PER_SECOND) as usize;
        for tick in 0..most {
            if done(self) {
                return tick;
            }
            self.tick(inputs);
        }
        assert!(done(self), "not within {seconds} s");
        most
    }

    fn tumbles(&self, kart: Entity) -> bool {
        matches!(
            self.state(kart),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        )
    }

    /// A still kart at `(x, z)`, facing -z like the arena's kart.
    fn parked(&mut self, x: f32, z: f32) -> Entity {
        self.other_kart(Transform::from_xyz(x, 0.55, z))
    }
}

/// Issue #18 (GD § 8): a piment doré opens a window of about 7 s (420 ticks) at its first press;
/// every press meanwhile is a piment's boost (+30 %, 1.2 s), and it stays in the hand; then it is
/// used up and the reserve moves in. A boost protects from nothing: hit meanwhile, the kart spins
/// and loses its boost, the window keeps running, and the kart boosts again once the hit is over.
#[test]
fn a_piment_dore_boosts_at_every_press_for_seven_seconds() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let tuning = ItemTuning::default();
    arena.give(kart, ItemKind::PimentDore, 1);
    arena.give(kart, ItemKind::Pelote, 1);
    let mut ticks = 0;
    let tap = |arena: &mut Arena| {
        arena.tap(kart, FULL_THROTTLE);
        2
    };
    ticks += tap(&mut arena);
    let boost = arena.boost(kart);
    assert!((boost.strength - tuning.piment_strength).abs() < 1e-6);
    assert!(boost.remaining > tuning.piment_duration - 0.05);
    // The window opened on the press (tick 1): 420 ticks.
    assert_eq!(arena.slots(kart).golden, 420 - 1);
    for boost in [2, 3] {
        arena.run(1.5, &[(kart, FULL_THROTTLE)]);
        ticks += 90;
        assert!(!arena.boost(kart).is_active());
        ticks += tap(&mut arena);
        assert!(arena.boost(kart).is_active(), "boost {boost}");
        assert!(matches!(
            arena.slots(kart).hand,
            Slot::Ready {
                item: ItemKind::PimentDore,
                ..
            }
        ));
    }
    // Hit: a spin out, its boost gone, the window going on; a press does nothing meanwhile.
    arena.hit(kart, ItemKind::Pelure);
    arena.tick(&[(kart, FULL_THROTTLE)]);
    ticks += 1;
    assert!(arena.state(kart).is_hit());
    assert!(!arena.boost(kart).is_active());
    ticks += tap(&mut arena);
    assert!(!arena.boost(kart).is_active(), "no item while spinning");
    ticks += arena.until(2.0, &[(kart, FULL_THROTTLE)], |a| !a.state(kart).is_hit());
    ticks += tap(&mut arena);
    assert!(arena.boost(kart).is_active(), "again after the hit");
    // The window closes 420 ticks after the first press: the reserve moves in.
    let golden = arena.slots(kart).golden as usize;
    assert_eq!(golden, 420 - (ticks - 1), "{ticks}");
    arena.run(golden as f32 / TICKS_PER_SECOND, &[(kart, FULL_THROTTLE)]);
    let slots = arena.slots(kart);
    assert_eq!(slots.golden, 0);
    assert_eq!(
        slots.hand,
        Slot::Ready {
            item: ItemKind::Pelote,
            count: 1
        }
    );
    assert_eq!(
        arena.stats(kart).used,
        4,
        "four presses counted (not the one while hit)"
    );
}

/// Issue #18 (GD § 8): a bombe thrown ahead flies over the kart just ahead and explodes where it
/// lands, about 17 m ahead of a still thrower: every kart within 6 m tumbles (about 1.6 s), the
/// thrower and the karts further away do not; an invincible kart in the blast is not hit.
#[test]
fn a_bombe_thrown_ahead_explodes_where_it_lands() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let close = arena.parked(1.0, -14.0);
    let far = arena.parked(0.0, -26.0);
    let shielded = arena.parked(-2.5, -13.0);
    arena.set_invincible(shielded, 5.0);
    arena.give(kart, ItemKind::Bombe, 1);
    arena.tap(kart, KartInput::default());
    let (bomb, thrown) = *arena.objects().last().unwrap();
    assert_eq!(thrown.item, ItemKind::Bombe);
    assert!(thrown.velocity.z < -15.0, "{}", thrown.velocity);
    let ticks = arena.until(2.0, &[], |a| a.state(close).is_hit());
    for (entity, object) in arena.objects() {
        assert_ne!(entity, bomb, "exploded: {object:?}");
    }
    assert!(arena.tumbles(close));
    assert!(
        ticks > 30 && ticks < 60,
        "a lob of about 0.75 s: {ticks} ticks"
    );
    assert!(!arena.state(far).is_hit());
    assert!(!arena.state(kart).is_hit());
    assert!(!arena.state(shielded).is_hit(), "invincible");
    assert_eq!(arena.stats(kart).hits_landed, 1);
    // A tumble of 96 ticks.
    let mut hit_ticks = 0;
    while arena.state(close).is_hit() {
        arena.tick(&[]);
        hit_ticks += 1;
        assert!(hit_ticks < 200);
    }
    assert!((95..=97).contains(&hit_ticks), "{hit_ticks}");
}

/// Issue #18: a bombe dropped behind (with the "back" direction) lies there and explodes at the
/// end of its fuse (3 s): a kart parked 3 m from it tumbles, its thrower, far away by then, does
/// not. Another one, dropped in front of a kart that drives onto it, explodes at once.
#[test]
fn a_bombe_dropped_behind_explodes_at_the_end_of_its_fuse_or_on_a_kart() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let parked = arena.parked(3.0, 2.0);
    arena.give(kart, ItemKind::Bombe, 1);
    let back = KartInput {
        brake: 1.0,
        ..default()
    };
    arena.tap(kart, back);
    let (bomb, dropped) = *arena.objects().last().unwrap();
    assert!(dropped.position.z > 1.0, "behind: {}", dropped.position);
    // The thrower drives away; the bombe lies there.
    let ticks = arena.until(4.0, &[(kart, FULL_THROTTLE)], |a| {
        a.app.world().get::<ItemObject>(bomb).is_none()
    });
    assert!((177..=181).contains(&ticks), "the fuse: {ticks} ticks");
    arena.tick(&[(kart, FULL_THROTTLE)]);
    assert!(arena.tumbles(parked));
    assert!(!arena.state(kart).is_hit());

    // Another one, in front of a kart driving onto it: at once.
    let mut arena = Arena::new();
    let dropper = arena.kart;
    let driver = arena.parked(0.0, 15.0);
    arena.give(dropper, ItemKind::Bombe, 1);
    arena.tap(dropper, back);
    let ticks = arena.until(
        3.0,
        &[(dropper, FULL_THROTTLE), (driver, FULL_THROTTLE)],
        |a| a.state(driver).is_hit(),
    );
    assert!(ticks < 120, "before its fuse: {ticks}");
    assert!(arena.tumbles(driver));
}

/// Issue #18 (GD § 8): the chantilly creams the screen of the karts ahead of its user (off a
/// race: ahead along its heading) for about 4 s, a spinning one too; not those behind, not an
/// invincible one, not its user.
#[test]
fn the_chantilly_creams_the_karts_ahead_for_four_seconds() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let ahead = arena.parked(0.0, -12.0);
    let behind = arena.parked(0.0, 12.0);
    let invincible = arena.parked(4.0, -20.0);
    let spinning = arena.parked(-4.0, -20.0);
    arena.set_invincible(invincible, 3.0);
    arena.hit(spinning, ItemKind::Pelure);
    arena.tick(&[]);
    assert!(arena.state(spinning).is_hit());
    arena.give(kart, ItemKind::Chantilly, 1);
    arena.tap(kart, KartInput::default());
    let tuning = ItemTuning::default();
    for creamed in [ahead, spinning] {
        let effects = arena.effects(creamed);
        assert!(
            effects.creamed > tuning.chantilly_time - 0.05,
            "{effects:?}"
        );
        assert_eq!(effects.cream(), 1.0);
    }
    for clean in [behind, invincible, kart] {
        assert_eq!(arena.effects(clean).creamed, 0.0);
    }
    // About 4 s, fading in the last second.
    let ticks = arena.until(5.0, &[], |a| a.effects(ahead).creamed == 0.0);
    assert!((236..=240).contains(&ticks), "{ticks}");
}

/// Issue #18 (GD § 8): a super câlin is about 7 s of invincibility (no item hits, no blink) and
/// of extra speed (+15 %); the kart it runs into tumbles. Then it is over: normal speed, no
/// protection.
#[test]
fn a_super_calin_is_seven_seconds_of_invincibility_speed_and_rams() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let victim = arena.parked(0.5, -130.0);
    let tuning = ItemTuning::default();
    arena.give(kart, ItemKind::SuperCalin, 1);
    arena.tap(kart, FULL_THROTTLE);
    assert!(arena.invincibility(kart).remaining > tuning.super_calin_time - 0.05);
    assert!(arena.effects(kart).calin > tuning.super_calin_time - 0.05);
    assert!((arena.factors(kart).items - 1.15).abs() < 1e-6);
    // An item meanwhile: nothing.
    arena.hit(kart, ItemKind::Pelote);
    arena.tick(&[(kart, FULL_THROTTLE)]);
    assert!(!arena.state(kart).is_hit());
    // Flat out into the parked kart far ahead (top speed first): it tumbles, the câlin goes
    // on. Invincible all along, without blinking.
    let mut fastest = 0.0f32;
    let mut ticks = 3;
    while arena.effects(kart).calin > 0.0 {
        let invincibility = arena.invincibility(kart);
        let effects = arena.effects(kart);
        assert!(invincibility.is_active());
        assert!(crate::kart::shown_with_effects(
            &invincibility,
            Some(&effects),
            ticks as f32 / TICKS_PER_SECOND
        ));
        arena.tick(&[(kart, FULL_THROTTLE)]);
        fastest = fastest.max(arena.forward_speed(kart));
        ticks += 1;
        assert!(ticks < 500);
    }
    assert_eq!(arena.stats(victim).hits_taken, 1, "rammed once");
    assert!(!arena.state(kart).is_hit());
    assert_eq!(arena.stats(kart).hits_landed, 1);
    assert!((418..=422).contains(&ticks), "about 7 s: {ticks}");
    let top = 28.5;
    assert!(fastest > top * 1.1, "{fastest}");
    // Over: back to normal.
    arena.run(0.1, &[(kart, FULL_THROTTLE)]);
    assert_eq!(arena.factors(kart).items, 1.0);
    assert!(!arena.invincibility(kart).is_active());
}

/// Issue #18 (GD § 8): the dégonflage shrinks and slows down every other kart, and their items
/// fall (slots and what they hold out); not its user, not an invincible kart. Off a race, for the
/// leader's time (7 s).
#[test]
fn the_degonflage_shrinks_slows_and_empties_the_others() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let other = arena.parked(5.0, -10.0);
    let invincible = arena.parked(-5.0, -10.0);
    arena.set_invincible(invincible, 3.0);
    arena.give(other, ItemKind::Pelote, 1);
    arena.give(other, ItemKind::Piment, 3);
    arena.tick(&[(other, ITEM)]);
    assert_eq!(arena.slots(other).dragging, Some(ItemKind::Pelote));
    arena.give(invincible, ItemKind::Abeille, 1);
    arena.give(kart, ItemKind::Degonflage, 1);
    arena.tap(kart, KartInput::default());
    let tuning = ItemTuning::default();
    let deflated = arena.effects(other);
    assert!(
        deflated.deflated > tuning.degonflage_time_first - 0.05,
        "{deflated:?}"
    );
    assert!((arena.factors(other).items - tuning.degonflage_speed).abs() < 1e-6);
    let slots = arena.slots(other);
    assert_eq!(
        (slots.hand, slots.reserve, slots.dragging),
        (Slot::Empty, Slot::Empty, None)
    );
    // What it dragged is gone too.
    arena.tick(&[(other, ITEM)]);
    assert!(arena.objects().is_empty());
    assert_eq!(arena.effects(invincible).deflated, 0.0);
    assert!(!arena.slots(invincible).hand.is_empty());
    assert_eq!(arena.effects(kart).deflated, 0.0);
    // Slower while small: flat out for 3 s.
    arena.run(3.0, &[(other, FULL_THROTTLE), (invincible, FULL_THROTTLE)]);
    let (small, normal) = (arena.forward_speed(other), arena.forward_speed(invincible));
    assert!(small < normal * 0.85, "{small} vs {normal}");
    let ticks = arena.until(6.0, &[(other, FULL_THROTTLE)], |a| {
        a.effects(other).deflated == 0.0
    });
    assert!(ticks < 260, "about 7 s in all: {ticks}");
    arena.tick(&[(other, FULL_THROTTLE)]);
    assert_eq!(arena.factors(other).items, 1.0);
}

/// Issue #18 (GD § 8): the klaxon-canard destroys the items around its user (within 9 m): a
/// pelure on the ground, another kart's shield (its holder loses it); not those further away,
/// not its user's own.
#[test]
fn the_klaxon_destroys_the_items_around() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let other = arena.parked(0.0, -6.0);
    let tuning = ItemTuning::default();
    let spawn = |arena: &mut Arena, owner: Entity, at: Vec3| {
        arena
            .app
            .world_mut()
            .spawn(ItemObject::new(
                ItemKind::Pelure,
                owner,
                Motion::Ground,
                at,
                Vec3::ZERO,
            ))
            .id()
    };
    let near = spawn(&mut arena, other, Vec3::new(5.0, 0.25, 3.0));
    let far = spawn(
        &mut arena,
        other,
        Vec3::new(0.0, 0.25, tuning.klaxon_radius + 6.0),
    );
    let own = spawn(&mut arena, kart, Vec3::new(-3.0, 0.25, 2.0));
    arena.give(other, ItemKind::Pelote, 1);
    arena.tick(&[(other, ITEM)]);
    assert!(arena.slots(other).dragging.is_some());
    arena.give(kart, ItemKind::KlaxonCanard, 1);
    arena.tap(kart, KartInput::default());
    let left: Vec<Entity> = arena.objects().iter().map(|(e, _)| *e).collect();
    assert!(!left.contains(&near));
    assert!(left.contains(&far) && left.contains(&own), "{left:?}");
    assert_eq!(left.len(), 2, "the shield is gone too");
    assert_eq!(arena.slots(other).dragging, None, "its holder lost it");
    assert!(!arena.state(other).is_hit(), "no kart is hit");
}

// --- Phase 3 (issue #21) ----------------------------------------------------------------------

impl Arena {
    /// The boomerangs out, by spawn order.
    fn boomerangs(&mut self) -> Vec<(Entity, ItemObject)> {
        self.objects()
            .into_iter()
            .filter(|(_, o)| o.item == ItemKind::Boomerang)
            .collect()
    }
}

/// Issue #21 (GD § 8: « boomerang qui revient, utilisable 3 fois »): thrown ahead, a boomerang-os
/// flies out about 30 m, curving to its left, then comes back to its thrower, who catches it: the
/// next throw is ready then, not before (a press meanwhile does nothing). Three throws, never a
/// hit on its own thrower; then the reserve moves in.
#[test]
fn a_boomerang_flies_out_comes_back_and_is_thrown_three_times() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let tuning = ItemTuning::default();
    arena.give(kart, ItemKind::Boomerang, 3);
    arena.give(kart, ItemKind::Piment, 1);
    let home = arena.transform(kart).translation;
    for throw in 0..3u8 {
        arena.tap(kart, KartInput::default());
        let flying = arena.boomerangs();
        assert_eq!(flying.len(), 1, "throw {throw}");
        let (entity, start) = flying[0];
        assert!(start.velocity.dot(Vec3::NEG_Z) > tuning.boomerang_speed * 0.9);
        let left = 2 - throw;
        if left > 0 {
            assert_eq!(
                arena.slots(kart).hand,
                Slot::Ready {
                    item: ItemKind::Boomerang,
                    count: left
                }
            );
            // A press while it flies: nothing more.
            arena.tap(kart, KartInput::default());
            assert_eq!(arena.boomerangs().len(), 1);
            assert_eq!(
                arena.slots(kart).hand.item(),
                Some((ItemKind::Boomerang, left))
            );
        }
        let (mut furthest, mut leftmost) = (0.0f32, 0.0f32);
        let mut ticks = 0;
        while arena.app.world().get::<ItemObject>(entity).is_some() {
            let at = arena
                .app
                .world()
                .get::<ItemObject>(entity)
                .unwrap()
                .position;
            furthest = furthest.max(at.with_y(0.0).distance(home.with_y(0.0)));
            leftmost = leftmost.min(at.x);
            arena.tick(&[]);
            ticks += 1;
            assert!(ticks < 200, "never came back");
        }
        assert!(
            (tuning.boomerang_range * 0.85..tuning.boomerang_range * 1.3).contains(&furthest),
            "{furthest}"
        );
        assert!(leftmost < -3.0, "it curves to its left: {leftmost}");
        assert!(ticks < 150, "back in under 2.5 s: {ticks}");
        assert!(!arena.state(kart).is_hit(), "caught, not hit");
    }
    assert_eq!(
        arena.slots(kart).hand,
        Slot::Ready {
            item: ItemKind::Piment,
            count: 1
        }
    );
    assert_eq!(arena.stats(kart).used, 3);
    assert_eq!(arena.stats(kart).hits_taken, 0);
}

/// Issue #21: a boomerang-os breaks on the first kart it touches (a spin out, a light item); the
/// next throw is ready at once. A wall on its way out sends it back early, and it is caught.
#[test]
fn a_boomerang_spins_out_a_kart_and_bounces_off_a_wall() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let victim = arena.parked(0.0, -12.0);
    arena.give(kart, ItemKind::Boomerang, 3);
    arena.tap(kart, KartInput::default());
    let ticks = arena.until(1.0, &[], |a| a.state(victim).is_hit());
    assert!(ticks < 30, "{ticks}");
    assert!(
        matches!(
            arena.state(victim),
            KartState::Hit {
                reaction: HitReaction::Spin,
                ..
            }
        ),
        "{:?}",
        arena.state(victim)
    );
    assert!(arena.boomerangs().is_empty(), "broken on it");
    assert_eq!(arena.stats(kart).hits_landed, 1);
    // The next throw at once (backwards: a wall 8 m behind sends it back).
    arena.wall(Vec3::new(0.0, 1.0, 8.0), Vec3::new(20.0, 1.0, 0.5));
    arena.tap(
        kart,
        KartInput {
            look_back: true,
            ..default()
        },
    );
    let (entity, out) = arena.boomerangs()[0];
    assert!(out.velocity.z > 0.0, "thrown backwards");
    let returning = |a: &Arena| {
        matches!(
            a.app.world().get::<ItemObject>(entity).map(|o| o.motion),
            Some(Motion::Boomerang {
                returning: true,
                ..
            })
        )
    };
    let ticks = arena.until(1.0, &[], returning);
    assert!(ticks < 20, "the wall sent it back: {ticks}");
    let furthest = arena
        .app
        .world()
        .get::<ItemObject>(entity)
        .unwrap()
        .position
        .z;
    assert!(furthest < 8.0, "{furthest}");
    arena.until(2.0, &[], |a| {
        a.app.world().get::<ItemObject>(entity).is_none()
    });
    assert!(!arena.state(kart).is_hit());
    assert_eq!(
        arena.slots(kart).hand,
        Slot::Ready {
            item: ItemKind::Boomerang,
            count: 1
        }
    );
}

/// Issue #88 acceptance (Max: « il tourne avec le kart »): a boomerang-os curves to the side its
/// thrower steers at the throw: full left, to the left, further than thrown straight (which goes
/// left too, as in #21); full right, to the right, never left, the mirror image of full left;
/// half right, to the right but less. The curve is fixed at the throw: steering the other way
/// afterwards changes nothing.
#[test]
fn a_boomerang_curves_to_the_side_its_thrower_steers() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let home = arena.transform(kart).translation;
    // How far it went left and right of its thrower on its way out, and its curve then.
    let throw = |arena: &mut Arena, steer: f32, then: f32| {
        arena.give(kart, ItemKind::Boomerang, 1);
        arena.tap(kart, KartInput { steer, ..default() });
        let (entity, _) = arena.boomerangs()[0];
        let curve_of = |o: &ItemObject| match o.motion {
            Motion::Boomerang { curve, .. } => curve,
            _ => panic!("{o:?}"),
        };
        let first = curve_of(&arena.boomerangs()[0].1);
        let (mut leftmost, mut rightmost) = (0.0f32, 0.0f32);
        let mut ticks = 0;
        loop {
            let object = *arena.app.world().get::<ItemObject>(entity).unwrap();
            if matches!(
                object.motion,
                Motion::Boomerang {
                    returning: true,
                    ..
                }
            ) {
                assert_eq!(curve_of(&object), first, "fixed at the throw");
                break;
            }
            leftmost = leftmost.min(object.position.x - home.x);
            rightmost = rightmost.max(object.position.x - home.x);
            arena.tick(&[(
                kart,
                KartInput {
                    steer: then,
                    ..default()
                },
            )]);
            ticks += 1;
            assert!(ticks < 120, "never turned back");
        }
        arena.until(3.0, &[], |a| {
            a.app.world().get::<ItemObject>(entity).is_none()
        });
        assert!(!arena.state(kart).is_hit(), "caught");
        (leftmost, rightmost, first)
    };
    let straight = throw(&mut arena, 0.0, 1.0);
    let left = throw(&mut arena, -1.0, 1.0);
    let right = throw(&mut arena, 1.0, -1.0);
    let half = throw(&mut arena, 0.5, -1.0);
    println!("straight {straight:?}, left {left:?}, right {right:?}, half right {half:?}");
    assert!(
        straight.0 < -3.0 && straight.1 < 0.5,
        "straight: left (#21)"
    );
    assert!(
        left.2 > 0.0 && left.0 < straight.0 - 3.0 && left.1 < 0.5,
        "full left"
    );
    assert!(
        right.2 < 0.0 && right.1 > 10.0 && right.0 > -0.5,
        "full right"
    );
    assert!(
        (left.0 + right.1).abs() < 0.5,
        "mirror images: {left:?} {right:?}"
    );
    assert!(
        half.2 < 0.0 && half.1 > 3.0 && half.1 < right.1 - 2.0 && half.0 > -0.5,
        "half right"
    );
}

/// Issue #88: a boomerang-os steered to the right breaks on the first kart on its curve (a spin
/// out), and the next one on its way is not touched (Max: « il s'arrête au premier kart
/// touché »). Thrown straight, it curves left, away from both: no hit.
#[test]
fn a_steered_boomerang_breaks_on_the_first_kart_on_its_curve() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let right = KartInput {
        steer: 1.0,
        ..default()
    };
    arena.give(kart, ItemKind::Boomerang, 3);
    // Its path to the right, with nobody around: where it is 12 m and 24 m out.
    arena.tap(kart, right);
    let (entity, _) = arena.boomerangs()[0];
    let (mut near, mut far) = (None, None);
    while let Some(object) = arena.app.world().get::<ItemObject>(entity).copied() {
        if let Motion::Boomerang {
            travelled,
            returning: false,
            ..
        } = object.motion
        {
            if travelled >= 12.0 && near.is_none() {
                near = Some(object.position);
            }
            if travelled >= 24.0 && far.is_none() {
                far = Some(object.position);
            }
        }
        arena.tick(&[]);
    }
    let (near, far) = (near.unwrap(), far.unwrap());
    assert!(near.x > 1.5 && far.x > near.x + 4.0, "{near} {far}");
    let first = arena.parked(near.x, near.z);
    let second = arena.parked(far.x, far.z);
    // Straight: to its left, it misses both and comes back.
    arena.tap(kart, KartInput::default());
    let (entity, _) = arena.boomerangs()[0];
    arena.until(3.0, &[], |a| {
        a.app.world().get::<ItemObject>(entity).is_none()
    });
    assert!(!arena.state(first).is_hit() && !arena.state(second).is_hit());
    assert_eq!(arena.stats(kart).hits_landed, 0);
    // To the right: the first one spins, it breaks there; the second one is never touched.
    arena.tap(kart, right);
    let ticks = arena.until(1.0, &[], |a| a.state(first).is_hit());
    assert!(ticks < 30, "{ticks}");
    assert!(
        matches!(
            arena.state(first),
            KartState::Hit {
                reaction: HitReaction::Spin,
                ..
            }
        ),
        "{:?}",
        arena.state(first)
    );
    assert!(arena.boomerangs().is_empty(), "broken on the first one");
    arena.run(1.5, &[]);
    assert!(!arena.state(second).is_hit());
    assert_eq!(arena.stats(kart).hits_landed, 1);
    assert_eq!(arena.stats(second).hits_taken, 0);
}

/// Issue #21 (GD § 8: « 8 objets autour du kart, à utiliser un par un »): a piñata comes out as
/// its eight items turning around the kart; each press uses the one in front, as itself: the
/// pelure dropped, the pelote rolling, the abeille flying, the bombe lobbed, the piment's boost,
/// the chantilly, the super câlin, the croquette dorée's two croquettes. Eight presses, eight
/// items, then the reserve moves in.
#[test]
fn a_pinata_turns_eight_items_around_the_kart_and_uses_the_one_in_front() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let tuning = ItemTuning::default();
    arena.give(kart, ItemKind::Pinata, 8);
    arena.give(kart, ItemKind::Pelote, 1);
    arena.tap(kart, KartInput::default());
    let around = arena.objects();
    assert_eq!(around.len(), 8);
    let center = arena.transform(kart).translation;
    let mut kinds: Vec<ItemKind> = around.iter().map(|(_, o)| o.item).collect();
    kinds.sort();
    let mut expected = PINATA_ITEMS.to_vec();
    expected.sort();
    assert_eq!(kinds, expected);
    for (_, object) in &around {
        assert!(matches!(object.held(), Some(Hold::Pinata(_))));
        let flat = (object.position - center).with_y(0.0).length();
        assert!((flat - tuning.pinata_orbit_radius).abs() < 0.05, "{flat}");
    }
    let mut used = Vec::new();
    let mut boosted = false;
    let mut hugged = false;
    let mut thrown = Vec::new();
    for press in 0..8 {
        // A lane of its own for each press: the kart never meets what it left in the others
        // (its pelure, its bombe's blast), even boosted.
        arena.place(kart, Transform::from_xyz(30.0 * press as f32, 0.55, 0.0));
        arena.run(0.3, &[]);
        // The one in front now.
        let forward = *arena.transform(kart).forward();
        let at = arena.transform(kart).translation;
        let front = arena
            .objects()
            .into_iter()
            .filter(|(_, o)| matches!(o.held(), Some(Hold::Pinata(_))))
            .max_by(|(_, a), (_, b)| {
                (a.position - at)
                    .dot(forward)
                    .total_cmp(&(b.position - at).dot(forward))
            })
            .map(|(e, o)| (e, o.item))
            .unwrap();
        let before = arena.objects().len();
        arena.tap(kart, KartInput::default());
        assert!(
            arena.app.world().get::<ItemObject>(front.0).is_none(),
            "press {press}: the one in front went"
        );
        used.push(front.1);
        boosted |= arena.boost(kart).is_active();
        hugged |= arena.effects(kart).calin > 0.0;
        let left = arena
            .objects()
            .iter()
            .filter(|(_, o)| matches!(o.held(), Some(Hold::Pinata(_))))
            .count();
        assert_eq!(left, 7 - press, "press {press}");
        if front.1.is_object() {
            assert!(
                arena.objects().len() >= before,
                "press {press}: {:?} out",
                front.1
            );
            thrown.push(front.1);
        }
    }
    used.sort();
    assert_eq!(used, expected, "each one once");
    assert!(boosted && hugged);
    assert_eq!(arena.held(kart), 2, "the croquette dorée");
    thrown.sort();
    assert_eq!(
        thrown,
        [
            ItemKind::Pelure,
            ItemKind::Pelote,
            ItemKind::Abeille,
            ItemKind::Bombe
        ]
    );
    assert_eq!(
        arena.slots(kart).hand,
        Slot::Ready {
            item: ItemKind::Pelote,
            count: 1
        }
    );
    assert_eq!(arena.stats(kart).used, 8);
}

/// Issue #21: a piñata's items are shields, like an "x3": a pelote thrown at the kart breaks on
/// the first one in its way (both gone), the kart is not hit, seven are left.
#[test]
fn a_pinatas_items_are_shields() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let thrower = arena.other_kart(Transform::from_xyz(0.0, 0.55, 20.0));
    arena.give(kart, ItemKind::Pinata, 8);
    arena.tap(kart, KartInput::default());
    arena.give(thrower, ItemKind::Pelote, 1);
    arena.tap(thrower, KartInput::default());
    arena.run(1.5, &[]);
    assert!(!arena.state(kart).is_hit());
    assert_eq!(arena.slots(kart).held_out(), Some((ItemKind::Pinata, 7)));
    assert_eq!(arena.stats(kart).blocked, 1);
    assert!(
        arena
            .objects()
            .iter()
            .all(|(_, o)| o.item != ItemKind::Pelote || o.owner != thrower)
    );
}

/// Issue #21 (GD § 8: « rend invisible environ 3 s et vole l'objet d'un adversaire »): the pie
/// voleuse takes the item of the closest kart that holds one (off a race), ready in the thief's
/// slot; for about 3 s the thief is not shown (a ghost stands for it), invincible without blinking:
/// a pelote goes through it; then it is back, unprotected.
#[test]
fn the_pie_voleuse_hides_the_kart_and_steals_an_item() {
    let mut arena = Arena::new();
    let kart = arena.kart;
    let empty = arena.parked(4.0, -6.0);
    let holder = arena.parked(-4.0, -15.0);
    let far = arena.parked(0.0, -60.0);
    arena.give(holder, ItemKind::Bombe, 1);
    arena.give(holder, ItemKind::Abeille, 3);
    arena.give(far, ItemKind::Fusee, 1);
    arena.give(kart, ItemKind::PieVoleuse, 1);
    arena.tap(kart, KartInput::default());
    let tuning = ItemTuning::default();
    assert_eq!(
        arena.slots(kart).hand,
        Slot::Ready {
            item: ItemKind::Bombe,
            count: 1
        },
        "the closest holder's hand"
    );
    assert_eq!(
        arena.slots(holder).hand,
        Slot::Ready {
            item: ItemKind::Abeille,
            count: 3
        }
    );
    assert_eq!(arena.slots(empty), ItemSlots::default());
    assert!(!arena.slots(far).hand.is_empty());
    let effects = arena.effects(kart);
    assert!(effects.hidden() && effects.ghost > tuning.pie_time - 0.05);
    let invincibility = arena.invincibility(kart);
    assert!(invincibility.is_active());
    for t in 0..60 {
        assert!(!crate::kart::shown_with_effects(
            &invincibility,
            Some(&effects),
            t as f32 / 60.0
        ));
    }
    // A pelote at it: through it, nothing.
    let thrower = arena.other_kart(Transform::from_xyz(0.0, 0.55, 15.0));
    arena.give(thrower, ItemKind::Pelote, 1);
    arena.tap(thrower, KartInput::default());
    arena.run(0.6, &[]);
    assert!(!arena.state(kart).is_hit());
    let pelote = arena
        .objects()
        .into_iter()
        .find(|(_, o)| o.item == ItemKind::Pelote)
        .expect("it went through");
    assert!(pelote.1.position.z < -1.5, "{}", pelote.1.position);
    // About 3 s in all, then shown and unprotected again.
    let left = arena.effects(kart).ghost;
    assert!(left > 2.0, "{left}");
    let ticks = arena.until(3.0, &[], |a| !a.effects(kart).hidden());
    assert!(
        (ticks as f32 - left * TICKS_PER_SECOND).abs() <= 2.0,
        "{ticks} for {left} s"
    );
    arena.tick(&[]);
    assert!(!arena.invincibility(kart).is_active());
    let effects = arena.effects(kart);
    assert!(crate::kart::shown_with_effects(
        &arena.invincibility(kart),
        Some(&effects),
        0.0
    ));
}

/// Issue #21, then #88 (Max: « un kart au hasard, avec une probabilité d'autant plus forte que ce
/// kart est devant »): who a pie voleuse steals from. In a race, a racer that holds something,
/// ahead of it or behind, where the draw falls on the weights of their places (with 5 racers and
/// the power 2: 25, 16, the thief's 9 left out, 4, 1); never the thief, never a kart with nothing
/// to take. Off a race, the closest one that holds something; nobody when nobody holds anything.
/// The way a boomerang-os comes back: it turns toward its thrower at its turn rate, straight at
/// it once close.
#[test]
fn the_pie_voleuse_picks_its_victim_and_the_boomerang_comes_back() {
    let [a, b, c, d, thief] = [1, 2, 3, 4, 5].map(|i| Entity::from_raw_u32(i).unwrap());
    let standings = [a, b, thief, c, d];
    let holds = |set: Vec<Entity>| move |e: Entity| set.contains(&e);
    let at = Vec3::ZERO;
    let victim = |set: Vec<Entity>, unit: f32| {
        steal_victim(thief, at, &standings, &[], holds(set), unit, 2.0)
    };
    // a, b and c hold something: 25 + 16 + 4 = 45.
    for (unit, expected) in [
        (0.0, a),
        (24.9 / 45.0, a),
        (25.1 / 45.0, b),
        (40.9 / 45.0, b),
        (41.1 / 45.0, c),
        (0.999_999, c),
    ] {
        assert_eq!(victim(vec![a, b, c], unit), Some(expected), "{unit}");
    }
    // Only behind it: c (4) and d (1).
    assert_eq!(victim(vec![c, d, thief], 0.79), Some(c));
    assert_eq!(victim(vec![c, d, thief], 0.81), Some(d));
    // Nothing to take: nobody, whatever the draw.
    for unit in [0.0, 0.5, 0.999] {
        assert_eq!(victim(vec![thief], unit), None);
        assert_eq!(victim(vec![], unit), None);
    }
    // The weights: the leader's N^k down to 1 for the last one; 0 makes them all alike.
    assert_eq!(steal_weight(1, 12, 2.0), 144.0);
    assert_eq!(steal_weight(2, 12, 2.0), 121.0);
    assert_eq!(steal_weight(12, 12, 2.0), 1.0);
    assert_eq!(steal_weight(1, 12, 1.0), 12.0);
    assert_eq!(steal_weight(1, 12, 0.0), steal_weight(12, 12, 0.0));
    let positions = [
        (a, Vec3::new(30.0, 0.0, 0.0)),
        (b, Vec3::new(0.0, 0.0, -8.0)),
        (c, Vec3::new(-5.0, 0.0, 0.0)),
        (thief, Vec3::ZERO),
    ];
    let off_race =
        |set: Vec<Entity>| steal_victim(thief, at, &[], &positions, holds(set), 0.5, 2.0);
    assert_eq!(
        off_race(vec![a, b, c, thief]),
        Some(c),
        "off a race: the closest"
    );
    assert_eq!(off_race(vec![a, b]), Some(b));
    assert_eq!(off_race(vec![thief]), None);

    let dt = 1.0 / 60.0;
    // Its thrower far behind: a turn of at most the turn rate.
    let turned = boomerang_heading(Vec3::NEG_Z, Vec3::new(0.0, 0.0, 50.0), dt, 6.0, 8.0);
    assert!((turned.angle_between(Vec3::NEG_Z) - 0.1).abs() < 1e-4);
    let left = boomerang_heading(Vec3::NEG_Z, Vec3::new(-50.0, 0.0, 0.0), dt, 6.0, 8.0);
    assert!(left.x < 0.0, "toward it, to the left");
    // Close: straight at it.
    let close = boomerang_heading(Vec3::NEG_Z, Vec3::new(3.0, 1.0, 4.0), dt, 6.0, 8.0);
    assert!((close - Vec3::new(0.6, 0.0, 0.8)).length() < 1e-5);
}

/// Issue #88 acceptance (statistical): 12 racers, the thief 7th, the 3rd and the 10th with
/// nothing to take. Over 10 000 thefts drawn as the game draws them (`steal_unit`: the race
/// seed, the thief's grid slot, its items used), each racer is robbed as often as its weight says
/// (`(N - rank + 1)^k`, the default power), within 4 standard deviations; the leader the most of
/// all; never the thief, never a kart without an item. With the power 0, every holder alike.
#[test]
fn the_pie_voleuse_robs_the_karts_ahead_most_often() {
    let tuning = ItemTuning::default();
    let karts: Vec<Entity> = (1..=12).map(|i| Entity::from_raw_u32(i).unwrap()).collect();
    let thief = karts[6];
    let empty = [karts[2], karts[9]];
    let holds = |e: Entity| e != thief && !empty.contains(&e);
    const DRAWS: u32 = 10_000;
    for power in [tuning.pie_rank_power, 1.0, 0.0] {
        let mut robbed = [0u32; 12];
        for number in 0..DRAWS {
            let unit = steal_unit(0xbeef, 6, number);
            let victim = steal_victim(thief, Vec3::ZERO, &karts, &[], holds, unit, power)
                .expect("someone holds something");
            robbed[karts.iter().position(|e| *e == victim).unwrap()] += 1;
        }
        let weight = |i: usize| {
            if holds(karts[i]) {
                steal_weight(i + 1, karts.len(), power)
            } else {
                0.0
            }
        };
        let total: f32 = (0..12).map(weight).sum();
        for (i, count) in robbed.iter().enumerate() {
            let p = weight(i) / total;
            let expected = DRAWS as f32 * p;
            let sigma = (DRAWS as f32 * p * (1.0 - p)).sqrt();
            assert!(
                (*count as f32 - expected).abs() <= 4.0 * sigma.max(1.0),
                "power {power}, rank {}: {count} against {expected:.0} ± {sigma:.0}: {robbed:?}",
                i + 1
            );
            if !holds(karts[i]) {
                assert_eq!(*count, 0, "power {power}, rank {}: {robbed:?}", i + 1);
            }
        }
        if power > 0.0 {
            let most = robbed.iter().max().unwrap();
            assert_eq!(robbed[0], *most, "the leader the most: {robbed:?}");
            assert!(
                robbed[0] > robbed[1] && robbed[1] > robbed[11],
                "{robbed:?}"
            );
        }
        println!("power {power}: {robbed:?}");
    }
}

/// Issue #88: the curve of a boomerang-os from its thrower's steering: to the side it steers, as
/// much as it steers (the full curve at full lock), and the default one (to its left) thrown
/// straight or with a light touch.
#[test]
fn a_boomerang_curves_with_the_steering() {
    let t = ItemTuning::default();
    assert_eq!(
        boomerang_curve(-1.0, &t),
        t.boomerang_steer_curve,
        "full left"
    );
    assert_eq!(
        boomerang_curve(1.0, &t),
        -t.boomerang_steer_curve,
        "full right"
    );
    assert_eq!(boomerang_curve(0.5, &t), -0.5 * t.boomerang_steer_curve);
    assert_eq!(boomerang_curve(-0.5, &t), 0.5 * t.boomerang_steer_curve);
    assert_eq!(boomerang_curve(2.0, &t), -t.boomerang_steer_curve);
    for straight in [0.0, 0.1, -0.1, f32::NAN] {
        assert_eq!(
            boomerang_curve(straight, &t),
            t.boomerang_curve,
            "{straight}"
        );
    }
    assert!(t.boomerang_curve > 0.0, "thrown straight: to its left");
}

// --- The whole game -------------------------------------------------------------------------

mod game {
    use super::*;
    use crate::admin::skip_countdown_now;
    use crate::kart::PlayerKart;
    use crate::race::{Course, Race, RacePhase};
    use crate::tests::{app_on, hold_keys, player};
    use crate::track::TrackChoice;
    use crate::ui::{ItemCount, ItemIcon};
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

    /// The game on Prairie Pompon (the player alone on the pole), once the race exists, at GO.
    fn race_at_go() -> (App, Entity) {
        let mut app = app_on(TrackChoice::default());
        let kart = player(&mut app);
        for _ in 0..10 {
            if app.world().get_resource::<Race>().is_some() {
                break;
            }
            app.update();
        }
        // The race (re)starts on its first tick: wait for it, then skip the countdown.
        app.update();
        app.update();
        assert!(skip_countdown_now(app.world_mut()));
        app.update();
        assert!(app.world().resource::<Race>().started());
        (app, kart)
    }

    /// A racer at race distance `distance` (first lap), `lateral` m right of the centerline,
    /// stopped, with the checkpoints up to there validated (as if it had driven there).
    fn put(app: &mut App, kart: Entity, distance: f32, lateral: f32) {
        let course = app.world().resource::<Course>().clone();
        let at = course
            .center
            .at(distance)
            .transform(lateral, crate::track::GRID_HEIGHT);
        let passed = course
            .checkpoints
            .iter()
            .filter(|c| **c <= distance)
            .count() as u32;
        let world = app.world_mut();
        *world.get_mut::<Transform>(kart).unwrap() = at;
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
        world.get_mut::<PhysicsPose>(kart).unwrap().snap(at);
        let mut progress = world.get_mut::<crate::race::RaceProgress>(kart).unwrap();
        progress.passed = passed;
        progress.distance = distance;
    }

    /// Two more racers (slots 1 and 2), standing still.
    fn two_more_racers(app: &mut App) -> (Entity, Entity) {
        let tuning = app.world().resource::<KartTuning>().clone();
        let mut spawn = |slot: usize| {
            app.world_mut()
                .spawn((
                    kart_body(&tuning, Transform::from_xyz(0.0, 50.0, 0.0)),
                    crate::race::Racer::new(slot),
                ))
                .id()
        };
        (spawn(1), spawn(2))
    }

    fn state(app: &App, kart: Entity) -> KartState {
        *app.world().get::<KartState>(kart).unwrap()
    }

    /// Acceptance criterion of #16: an abeille homes in on the kart just ahead in the race (not
    /// the leader further on): it follows the track's centerline, around the hairpin too, above
    /// the road, then dives on its target.
    #[test]
    fn an_abeille_hits_the_kart_just_ahead() {
        // On the start straight, then around the hairpin.
        for (me, ahead, leader) in [(20.0, 45.0, 75.0), (95.0, 175.0, 215.0)] {
            let (mut app, kart) = race_at_go();
            let (target, first) = two_more_racers(&mut app);
            app.update();
            put(&mut app, kart, me, 0.0);
            put(&mut app, target, ahead, 2.0);
            put(&mut app, first, leader, -2.0);
            // The race catches up with the new places.
            for _ in 0..20 {
                app.update();
            }
            let race = app.world().resource::<Race>();
            assert_eq!(race.standings, vec![first, target, kart], "{me} m");
            app.world_mut()
                .get_mut::<ItemSlots>(kart)
                .unwrap()
                .give(ItemKind::Abeille, 1, 0);
            hold_keys(&mut app, &[KeyCode::KeyE]);
            app.update();
            hold_keys(&mut app, &[]);
            let course = app.world().resource::<Course>().clone();
            let mut ticks = 0;
            let mut highest_lateral = f32::NEG_INFINITY;
            while !state(&app, target).is_hit() {
                app.update();
                ticks += 1;
                assert!(ticks < 240, "the abeille never hit ({me} m)");
                let world = app.world_mut();
                for object in world.query::<&ItemObject>().iter(world) {
                    let p = course.project_anywhere(object.position);
                    highest_lateral = highest_lateral.max(p.lateral.abs() - p.wall);
                    if let Motion::Homing {
                        flight: Flight::Track { .. },
                        ..
                    } = object.motion
                    {
                        assert!(p.height > 1.0, "above the karts: {p:?}");
                    }
                }
            }
            // It flew, and stayed over the track (between the walls).
            assert!(
                highest_lateral.is_finite() && highest_lateral < 0.0,
                "{highest_lateral}"
            );
            assert!(matches!(
                state(&app, target),
                KartState::Hit {
                    reaction: HitReaction::Spin,
                    ..
                }
            ));
            assert!(!state(&app, first).is_hit());
            assert!(!state(&app, kart).is_hit());
            assert_eq!(app.world().get::<ItemStats>(kart).unwrap().hits_landed, 1);
            // About the time it takes to catch up at its speed.
            let gap = ahead - me;
            let seconds = ticks as f32 / TICKS_PER_SECOND;
            assert!(seconds < gap / 30.0 + 0.5, "{seconds} s for {gap} m");
            let world = app.world_mut();
            assert_eq!(world.query::<&ItemObject>().iter(world).count(), 0);
        }
    }

    /// Issue #16 on the keyboard: driving straight from the pole (up arrow only), the player
    /// breaks a box of the first row; the HUD's hand slot flashes through the items (roulette),
    /// then shows the item; E uses it.
    #[test]
    fn a_keyboard_player_takes_a_box_and_uses_its_item() {
        let (mut app, kart) = race_at_go();
        assert!(app.world().get::<PlayerKart>(kart).is_some());
        hold_keys(&mut app, &[KeyCode::ArrowUp]);
        let mut ticks = 0;
        while app.world().get::<ItemStats>(kart).unwrap().boxes == 0 {
            app.update();
            ticks += 1;
            assert!(ticks < 600, "no box on the way");
        }
        let icon = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(&ItemIcon, &Visibility, &BackgroundColor)>()
                .iter(world)
                .find(|(icon, ..)| icon.index == 0)
                .map(|(_, visibility, color)| (*visibility, *color))
                .unwrap()
        };
        // The roulette: the hand slot flashes through several items.
        let mut colors = Vec::new();
        for _ in 0..80 {
            app.update();
            let (visibility, color) = icon(&mut app);
            assert_eq!(visibility, Visibility::Inherited);
            if !colors.contains(&color) {
                colors.push(color);
            }
        }
        assert!(colors.len() >= 3, "{}", colors.len());
        // It stops on the item drawn.
        for _ in 0..15 {
            app.update();
        }
        let Slot::Ready { item, count } = app.world().get::<ItemSlots>(kart).unwrap().hand else {
            panic!("no item after the roulette");
        };
        let (shown, _) = icon(&mut app);
        assert_eq!(shown, Visibility::Inherited);
        let world = app.world_mut();
        let count_text = world
            .query::<(&ItemCount, &Text)>()
            .iter(world)
            .find(|(c, _)| c.index == 0)
            .map(|(_, t)| t.0.clone())
            .unwrap();
        assert_eq!(
            count_text,
            if count > 1 {
                format!("x{count}")
            } else {
                String::new()
            }
        );
        // E, as many times as it takes (an "x3" comes out first).
        for _ in 0..=count {
            hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::KeyE]);
            app.update();
            hold_keys(&mut app, &[KeyCode::ArrowUp]);
            app.update();
        }
        let stats = *app.world().get::<ItemStats>(kart).unwrap();
        assert!(stats.used >= 1, "{item:?} x{count}: {stats:?}");
        assert_eq!(app.world().resource::<Race>().phase(), RacePhase::Racing);
    }

    /// A race restart takes everything back: empty slots, nothing on the track, every box there.
    #[test]
    fn a_restart_takes_the_items_back() {
        let (mut app, kart) = race_at_go();
        app.world_mut()
            .get_mut::<ItemSlots>(kart)
            .unwrap()
            .give(ItemKind::Pelure, 3, 0);
        hold_keys(&mut app, &[KeyCode::KeyE]);
        app.update();
        hold_keys(&mut app, &[]);
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&ItemObject>().iter(world).count(), 3);
        // The race clock back to its first tick, as a restart does.
        app.world_mut().resource_mut::<Race>().flow = crate::race::RaceFlow::default();
        app.update();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&ItemObject>().iter(world).count(), 0);
        assert_eq!(*world.get::<ItemSlots>(kart).unwrap(), ItemSlots::default());
    }

    // --- Phase 2 (issue #18) ------------------------------------------------------------------

    fn effects(app: &App, kart: Entity) -> crate::kart::ItemEffects {
        *app.world().get::<crate::kart::ItemEffects>(kart).unwrap()
    }

    /// Presses the player's item button (E) once.
    fn press_e(app: &mut App) {
        hold_keys(app, &[KeyCode::KeyE]);
        app.update();
        hold_keys(app, &[]);
        app.update();
    }

    /// Presses a racer's item button (no keyboard: its input).
    fn press_item(app: &mut App, kart: Entity) {
        app.world_mut().get_mut::<KartInput>(kart).unwrap().item = true;
        app.update();
        app.world_mut().get_mut::<KartInput>(kart).unwrap().item = false;
        app.update();
    }

    fn give(app: &mut App, kart: Entity, item: ItemKind) {
        let mut slots = app.world_mut().get_mut::<ItemSlots>(kart).unwrap();
        slots.clear();
        slots.give(item, 1, 0);
    }

    /// Acceptance criterion of #18 (GD § 8): the fusée drives the kart along the bots' racing
    /// line for about 6 s (360 ticks), much faster than a kart (about 42 m/s), whatever its
    /// driver does (the brake held here): on both circuits, from the straights, into the
    /// hairpin and the banked turn, it never leaves the track nor needs the pelican, and it is
    /// invincible. Then the driver has the kart back.
    #[test]
    fn the_fusee_drives_along_the_racing_line() {
        for (track, starts) in [
            (TrackChoice::default(), [10.0, 90.0, 180.0, 300.0, 420.0]),
            (
                TrackChoice::Circuit("tracks/anneau.track.ron".into()),
                [10.0, 120.0, 200.0, 330.0, 420.0],
            ),
        ] {
            for start in starts {
                let mut app = crate::tests::app_on(track.clone());
                let kart = player(&mut app);
                for _ in 0..10 {
                    if app.world().get_resource::<Race>().is_some() {
                        break;
                    }
                    app.update();
                }
                app.update();
                app.update();
                assert!(skip_countdown_now(app.world_mut()));
                app.update();
                put(&mut app, kart, start, 0.0);
                app.update();
                give(&mut app, kart, ItemKind::Fusee);
                press_e(&mut app);
                assert!(effects(&app, kart).rocket > 5.9);
                let course = app.world().resource::<Course>().clone();
                let (mut ticks, mut fastest) = (2, 0.0f32);
                // The driver brakes all along: the autopilot does not listen.
                hold_keys(&mut app, &[KeyCode::ArrowDown]);
                while effects(&app, kart).rocket > 0.0 {
                    app.update();
                    ticks += 1;
                    assert!(ticks < 400);
                    let world = app.world();
                    assert_ne!(state(&app, kart), KartState::Respawning, "{start} m");
                    assert!(
                        effects(&app, kart).rocket == 0.0
                            || world.get::<Invincibility>(kart).unwrap().is_active()
                    );
                    let at = world.get::<Transform>(kart).unwrap().translation;
                    let p = course.project_anywhere(at);
                    assert!(
                        p.lateral.abs() < p.wall,
                        "{track:?} from {start} m: off the road at {at} ({p:?})"
                    );
                    let speed = world.get::<Velocity>(kart).unwrap().linear.length();
                    fastest = fastest.max(speed);
                }
                assert!((359..=362).contains(&ticks), "about 6 s: {ticks}");
                assert!(fastest > 38.0, "{track:?} from {start} m: {fastest} m/s");
                // The driver again: it brakes.
                let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
                for _ in 0..60 {
                    app.update();
                }
                let after = app.world().get::<Velocity>(kart).unwrap().linear.length();
                assert!(after < speed * 0.5, "{after} from {speed}");
                assert_eq!(
                    *app.world().get::<crate::kart::Autopilot>(kart).unwrap(),
                    crate::kart::Autopilot::default()
                );
            }
        }
    }

    /// Issue #18: the fusée knocks over the kart in its way (a tumble), and nothing hits it.
    #[test]
    fn the_fusee_knocks_over_the_karts_in_its_way() {
        let (mut app, kart) = race_at_go();
        let (target, _) = two_more_racers(&mut app);
        app.update();
        put(&mut app, kart, 20.0, 0.0);
        let line = app.world().resource::<crate::bots::RacingLine>().clone();
        put(&mut app, target, 70.0, line.offset_at(70.0));
        give(&mut app, kart, ItemKind::Fusee);
        press_e(&mut app);
        let mut ticks = 0;
        while !state(&app, target).is_hit() {
            app.update();
            ticks += 1;
            assert!(ticks < 180, "never rammed");
        }
        assert!(matches!(
            state(&app, target),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ));
        assert!(!state(&app, kart).is_hit());
    }

    /// Acceptance criterion of #18 (GD § 8): the oreiller géant flies above the track to the
    /// leader (not the kart just ahead), hovers over it, then crushes it: the leader and the
    /// kart right beside it tumble, the others do not (a small zone). An invincible leader is not
    /// hit (the oreiller is gone all the same).
    #[test]
    fn the_oreiller_geant_crushes_the_leader() {
        for invincible in [false, true] {
            let (mut app, kart) = race_at_go();
            let (target, leader) = two_more_racers(&mut app);
            let tuning = app.world().resource::<KartTuning>().clone();
            let beside = app
                .world_mut()
                .spawn((
                    kart_body(&tuning, Transform::from_xyz(0.0, 50.0, 0.0)),
                    crate::race::Racer::new(3),
                ))
                .id();
            app.update();
            put(&mut app, kart, 20.0, 0.0);
            put(&mut app, target, 45.0, 2.0);
            put(&mut app, leader, 75.0, -2.0);
            put(&mut app, beside, 74.0, 0.5);
            for _ in 0..20 {
                app.update();
            }
            let race = app.world().resource::<Race>();
            assert_eq!(race.standings[0], leader);
            if invincible {
                app.world_mut()
                    .get_mut::<Invincibility>(leader)
                    .unwrap()
                    .remaining = 5.0;
            }
            give(&mut app, kart, ItemKind::OreillerGeant);
            press_e(&mut app);
            let course = app.world().resource::<Course>().clone();
            let mut ticks = 0;
            let mut hovered = false;
            loop {
                app.update();
                ticks += 1;
                assert!(ticks < 240, "the oreiller never landed");
                let world = app.world_mut();
                let pillows: Vec<ItemObject> = world
                    .query::<&ItemObject>()
                    .iter(world)
                    .copied()
                    .filter(|o| o.item == ItemKind::OreillerGeant)
                    .collect();
                let Some(pillow) = pillows.first() else {
                    break;
                };
                if let Motion::Pillow { flight, target } = pillow.motion {
                    assert_eq!(target, Some(leader), "after the leader");
                    match flight {
                        PillowFlight::Track { .. } => {
                            let p = course.project_anywhere(pillow.position);
                            assert!(p.height > 2.5, "high above the karts: {p:?}");
                        }
                        PillowFlight::Hover { .. } => hovered = true,
                        PillowFlight::Drop => {}
                    }
                }
            }
            assert!(hovered);
            assert!(ticks < 200, "{ticks} ticks");
            if invincible {
                assert!(!state(&app, leader).is_hit());
            } else {
                assert!(matches!(
                    state(&app, leader),
                    KartState::Hit {
                        reaction: HitReaction::Tumble,
                        ..
                    }
                ));
            }
            assert!(state(&app, beside).is_hit(), "beside the leader");
            assert!(!state(&app, target).is_hit());
            assert!(!state(&app, kart).is_hit());
        }
    }

    /// Acceptance criterion of #18 (GD § 8): the klaxon-canard destroys the oreiller géant: the
    /// leader honks while it hovers over it, and is not hit.
    #[test]
    fn the_klaxon_destroys_the_oreiller_geant() {
        let (mut app, kart) = race_at_go();
        let (_, leader) = two_more_racers(&mut app);
        app.update();
        put(&mut app, kart, 20.0, 0.0);
        put(&mut app, leader, 70.0, 0.0);
        for _ in 0..20 {
            app.update();
        }
        give(&mut app, kart, ItemKind::OreillerGeant);
        give(&mut app, leader, ItemKind::KlaxonCanard);
        press_e(&mut app);
        let pillow = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<&ItemObject>()
                .iter(world)
                .find(|o| o.item == ItemKind::OreillerGeant)
                .copied()
        };
        let mut ticks = 0;
        while !matches!(
            pillow(&mut app).map(|p| p.motion),
            Some(Motion::Pillow {
                flight: PillowFlight::Hover { .. },
                ..
            })
        ) {
            app.update();
            ticks += 1;
            assert!(ticks < 120, "it never hovered");
        }
        // Right over the leader now.
        for _ in 0..12 {
            app.update();
        }
        press_item(&mut app, leader);
        assert!(pillow(&mut app).is_none(), "destroyed");
        for _ in 0..90 {
            app.update();
            assert!(!state(&app, leader).is_hit());
        }
    }

    /// Acceptance criterion of #18 (GD § 8): the oreiller géant's rules, with a table that gives
    /// it almost always: none before 20 s of race (another item instead), one at a time (none
    /// while a kart holds one), and 30 s between two (from the last one drawn).
    #[test]
    fn the_oreiller_geant_rules_in_a_race() {
        let (mut app, kart) = race_at_go();
        let (other, _) = two_more_racers(&mut app);
        app.update();
        app.insert_resource(
            ItemTable::from_ron("(rows: [(from: 0.0, items: [(OreillerGeant, 99), (Pelure, 1)])])")
                .unwrap(),
        );
        // A box under a kart: what it draws.
        let draw = |app: &mut App, who: Entity| -> ItemKind {
            app.world_mut().get_mut::<ItemSlots>(who).unwrap().clear();
            let at = *app.world().get::<Transform>(who).unwrap();
            app.world_mut().spawn((
                ItemBoxSpot { double: false },
                Transform::from_translation(at.translation),
            ));
            app.update();
            app.update();
            let slots = *app.world().get::<ItemSlots>(who).unwrap();
            slots.hand.item().expect("a box").0
        };
        let skip = |app: &mut App, seconds: f32| {
            app.world_mut().resource_mut::<Race>().flow.tick += (seconds * 60.0) as u32;
            app.update();
        };
        put(&mut app, kart, 40.0, 0.0);
        put(&mut app, other, 140.0, 0.0);
        app.update();
        assert_eq!(draw(&mut app, kart), ItemKind::Pelure, "before 20 s");
        skip(&mut app, 20.0);
        assert_eq!(draw(&mut app, kart), ItemKind::OreillerGeant);
        // The kart holds it, 35 s later: no other.
        skip(&mut app, 35.0);
        assert_eq!(draw(&mut app, other), ItemKind::Pelure, "one at a time");
        // Gone (used up): the cooldown is over too.
        app.world_mut().get_mut::<ItemSlots>(kart).unwrap().clear();
        assert_eq!(draw(&mut app, other), ItemKind::OreillerGeant, "30 s later");
        app.world_mut().get_mut::<ItemSlots>(other).unwrap().clear();
        skip(&mut app, 10.0);
        assert_eq!(
            draw(&mut app, kart),
            ItemKind::Pelure,
            "within 30 s of the last"
        );
        skip(&mut app, 21.0);
        assert_eq!(draw(&mut app, kart), ItemKind::OreillerGeant);
    }

    /// Acceptance criterion of #18 (GD § 8): the dégonflage of the last racer shrinks the others
    /// for a time that grows with their place (the leader the longest), not an invincible one.
    #[test]
    fn the_degonflage_lasts_longer_for_the_first_ones() {
        let (mut app, kart) = race_at_go();
        let (second, first) = two_more_racers(&mut app);
        let tuning = app.world().resource::<KartTuning>().clone();
        let third = app
            .world_mut()
            .spawn((
                kart_body(&tuning, Transform::from_xyz(0.0, 50.0, 0.0)),
                crate::race::Racer::new(3),
            ))
            .id();
        app.update();
        put(&mut app, kart, 20.0, 0.0);
        put(&mut app, third, 30.0, 3.0);
        put(&mut app, second, 45.0, 2.0);
        put(&mut app, first, 75.0, -2.0);
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Race>().standings,
            vec![first, second, third, kart]
        );
        app.world_mut()
            .get_mut::<Invincibility>(third)
            .unwrap()
            .remaining = 2.0;
        give(&mut app, kart, ItemKind::Degonflage);
        press_e(&mut app);
        let items = app.world().resource::<ItemTuning>().clone();
        let (a, b) = (
            effects(&app, first).deflated,
            effects(&app, second).deflated,
        );
        let expected = |rank: u32| {
            deflate_time(
                rank,
                4,
                items.degonflage_time_first,
                items.degonflage_time_last,
            )
        };
        assert!((a - expected(1)).abs() < 0.05, "{a}");
        assert!((b - expected(2)).abs() < 0.05, "{b}");
        assert!(a > b);
        assert_eq!(effects(&app, third).deflated, 0.0, "invincible");
        assert_eq!(effects(&app, kart).deflated, 0.0, "its user");
    }

    /// Acceptance criterion of #18 (GD § 8): the chantilly of the last racer creams the screen of
    /// the player ahead: the cream shows over the HUD, then goes away.
    #[test]
    fn the_chantilly_creams_the_players_screen() {
        let (mut app, kart) = race_at_go();
        let (behind, _) = two_more_racers(&mut app);
        app.update();
        put(&mut app, kart, 80.0, 0.0);
        put(&mut app, behind, 30.0, 0.0);
        for _ in 0..20 {
            app.update();
        }
        let cream = |app: &mut App| {
            let world = app.world_mut();
            *world
                .query_filtered::<&Visibility, With<crate::ui::CreamOverlay>>()
                .single(world)
                .unwrap()
        };
        assert_eq!(cream(&mut app), Visibility::Hidden);
        give(&mut app, behind, ItemKind::Chantilly);
        press_item(&mut app, behind);
        app.update();
        assert!(effects(&app, kart).creamed > 3.9);
        assert_eq!(cream(&mut app), Visibility::Inherited);
        for _ in 0..240 {
            app.update();
        }
        assert_eq!(cream(&mut app), Visibility::Hidden);
    }

    /// Issue #18 (debug): the I key puts the next item of the list in the player's hand, ready:
    /// every item, one press after the other; the admin panel's « Donner » gives any.
    #[test]
    fn the_debug_key_gives_every_item_in_turn() {
        let (mut app, kart) = race_at_go();
        let mut seen = Vec::new();
        for _ in 0..Prize::ALL.len() {
            for state in [ButtonState::Pressed, ButtonState::Released] {
                app.world_mut().write_message(KeyboardInput {
                    key_code: DEBUG_ITEM_KEY,
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                app.update();
            }
            let Slot::Ready { item, count } = app.world().get::<ItemSlots>(kart).unwrap().hand
            else {
                panic!("nothing in the hand");
            };
            seen.push((item, count));
        }
        let all: Vec<(ItemKind, u8)> = Prize::ALL.iter().filter_map(|p| p.item()).collect();
        assert_eq!(seen, all);
        app.world_mut().write_message(GiveItem(Prize::Fusee));
        app.update();
        assert_eq!(
            app.world().get::<ItemSlots>(kart).unwrap().hand,
            Slot::Ready {
                item: ItemKind::Fusee,
                count: 1
            }
        );
    }
}
