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
};
const ITEM: KartInput = KartInput {
    throttle: 0.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: true,
    look_back: false,
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

// --- The whole game -------------------------------------------------------------------------

mod game {
    use super::*;
    use crate::admin::skip_countdown_now;
    use crate::kart::PlayerKart;
    use crate::race::{Course, Race, RacePhase};
    use crate::tests::{app_on, hold_keys, player};
    use crate::track::TrackChoice;
    use crate::ui::{ItemCount, ItemIcon};

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
}
