//! Headless scenarios of the interactions between karts (issue #34 acceptance): the real
//! simulation (Rapier, karts) with the croquette and interaction rules on a flat slab, one fixed
//! tick per `app.update()`; then the whole game on Prairie Pompon (keyboard, visuals, camera,
//! HUD).

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::croquettes::{CroquettePlugin, Croquettes};
use crate::interpolation::PhysicsPose;
use crate::kart::{
    Boost, DriftSide, KartStats, KartTuning, Stat, StatsTuning, kart_body_with_stats,
};
use crate::track::{Surface, WALL_RESTITUTION, spawn_static_box};

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
const PAW: KartInput = KartInput {
    interact: true,
    ..FULL_THROTTLE
};

/// Karts on a large flat slab with the croquette and interaction rules (no race).
struct Arena {
    app: App,
}

impl Arena {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            SimulationPlugin,
            CroquettePlugin,
            InteractionPlugin,
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
        app.update();
        Self { app }
    }

    /// A kart at `(x, z)` heading -Z, with this Weight level (10: the middle).
    fn kart(&mut self, x: f32, z: f32, weight: u8) -> Entity {
        let world = self.app.world_mut();
        let tuning = world.resource::<KartTuning>().clone();
        let stats_tuning = world.resource::<StatsTuning>().clone();
        let stats = KartStats::default().with(Stat::Weight, weight);
        world
            .spawn(kart_body_with_stats(
                &tuning,
                &stats_tuning,
                stats,
                Transform::from_xyz(x, 0.55, z),
            ))
            .id()
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

    fn get<C: Component + Copy>(&self, kart: Entity) -> C {
        *self.app.world().get::<C>(kart).unwrap()
    }

    fn position(&self, kart: Entity) -> Vec3 {
        self.get::<Transform>(kart).translation
    }

    fn speed(&self, kart: Entity) -> f32 {
        let forward = *self.get::<Transform>(kart).forward();
        self.get::<Velocity>(kart).linear.dot(forward)
    }

    fn paw(&self, kart: Entity) -> Paw {
        self.get::<Paw>(kart)
    }

    fn stats(&self, kart: Entity) -> PawStats {
        self.get::<PawStats>(kart)
    }

    fn croquettes(&self, kart: Entity) -> u8 {
        self.get::<Croquettes>(kart).count
    }

    fn set_croquettes(&mut self, kart: Entity, count: u8) {
        self.app
            .world_mut()
            .get_mut::<Croquettes>(kart)
            .unwrap()
            .count = count;
    }

    /// Puts `kart` at `transform`, driving with `velocity` (a test moving a kart next to another).
    fn place(&mut self, kart: Entity, transform: Transform, velocity: Velocity) {
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(kart).unwrap() = transform;
        *world.get_mut::<Velocity>(kart).unwrap() = velocity;
        world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    }
}

/// Two karts side by side at full speed on a straight line, `b` `gap` m to the right of `a`
/// (heading -Z, the right is +X), Weights `a_weight` and `b_weight`; `b` holds 3 croquettes.
fn side_by_side(a_weight: u8, b_weight: u8, gap: f32) -> (Arena, Entity, Entity) {
    let mut arena = Arena::new();
    let a = arena.kart(0.0, 0.0, a_weight);
    let b = arena.kart(gap, 0.0, b_weight);
    arena.run(0.5, &[]);
    arena.run(3.0, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    // Given now: they make it faster (+1 % each), it would have pulled ahead.
    arena.set_croquettes(b, 3);
    (arena, a, b)
}

/// A paw strike: the button down one tick, released the next, at full throttle.
fn tap(arena: &mut Arena, kart: Entity, others: &[Entity]) {
    let mut inputs: Vec<(Entity, KartInput)> = others.iter().map(|k| (*k, FULL_THROTTLE)).collect();
    inputs.push((kart, PAW));
    arena.tick(&inputs);
    *inputs.last_mut().unwrap() = (kart, FULL_THROTTLE);
    arena.tick(&inputs);
}

/// What a strike from `a` did to `b`, side by side: `b`'s sideways move 1 s later and its
/// forward speed on the tick of the strike, compared to the same race without it; its
/// croquettes after.
struct StrikeEffect {
    moved: f32,
    speed_ratio: f32,
    croquettes: u8,
    struck: PawStats,
}

fn strike_effect(a_weight: u8, b_weight: u8, gap: f32) -> StrikeEffect {
    let (mut struck, a, b) = side_by_side(a_weight, b_weight, gap);
    let (mut control, ca, cb) = side_by_side(a_weight, b_weight, gap);
    tap(&mut struck, a, &[b]);
    control.run(
        2.0 / TICKS_PER_SECOND,
        &[(ca, FULL_THROTTLE), (cb, FULL_THROTTLE)],
    );
    let speed_ratio = struck.speed(b) / control.speed(cb);
    struck.run(1.0, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    control.run(1.0, &[(ca, FULL_THROTTLE), (cb, FULL_THROTTLE)]);
    StrikeEffect {
        moved: struck.position(b).x - control.position(cb).x,
        speed_ratio,
        croquettes: struck.croquettes(b),
        struck: struck.stats(b),
    }
}

/// Issue #34 acceptance: a strike on a kart in reach pushes it sideways (about 1.4 m, GD § 15),
/// slows it down (−10 %) and costs it 1 croquette. It keeps control (no spin), and the striker
/// rests for 1.5 s.
#[test]
fn a_strike_in_reach_pushes_the_kart_aside_slows_it_and_costs_a_croquette() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    tap(&mut arena, a, &[b]);
    assert_eq!(arena.croquettes(b), 2, "1 croquette lost");
    assert_eq!(arena.get::<KartState>(b), KartState::Grounded, "no spin");
    assert_eq!(arena.paw(b).protected, 60, "protected for 1 s");
    assert_eq!(arena.paw(a).cooldown, 90, "the paw rests 1.5 s");
    assert_eq!(arena.stats(a).strikes, 1);
    assert_eq!(arena.stats(a).landed, 1);
    assert_eq!(arena.stats(b).struck, 1);
    assert!(arena.get::<Shove>(b).is_active());

    let effect = strike_effect(10, 10, 2.2);
    assert!(
        (1.1..=1.7).contains(&effect.moved),
        "pushed {:.2} m aside",
        effect.moved
    );
    assert!(
        (0.88..=0.93).contains(&effect.speed_ratio),
        "speed × {:.3}",
        effect.speed_ratio
    );
    assert_eq!(effect.croquettes, 2);
    assert_eq!(effect.struck.struck, 1);
}

/// Issue #34 acceptance: a strike with nobody in reach does nothing to the other karts; the
/// striker slides about 0.3 m toward its swing, without losing speed, and rests.
#[test]
fn a_strike_out_of_reach_does_nothing_to_the_other_karts() {
    let (mut struck, a, b) = side_by_side(10, 10, 5.0);
    let (mut control, ca, cb) = side_by_side(10, 10, 5.0);
    tap(&mut struck, a, &[b]);
    control.run(
        2.0 / TICKS_PER_SECOND,
        &[(ca, FULL_THROTTLE), (cb, FULL_THROTTLE)],
    );
    let no_loss = struck.speed(a) / control.speed(ca);
    struck.run(1.0, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    control.run(1.0, &[(ca, FULL_THROTTLE), (cb, FULL_THROTTLE)]);
    assert!(
        struck.position(b).distance(control.position(cb)) < 1e-4,
        "the other kart is untouched"
    );
    assert!((struck.speed(b) - control.speed(cb)).abs() < 1e-4);
    assert_eq!(struck.croquettes(b), 3);
    assert_eq!(struck.stats(b), PawStats::default());
    let slide = struck.position(a).x - control.position(ca).x;
    assert!((0.2..=0.4).contains(&slide), "slid {slide:.2} m");
    assert!((no_loss - 1.0).abs() < 0.005, "speed × {no_loss}");
    assert_eq!(struck.stats(a).missed, 1);
    assert_eq!(struck.paw(a).cooldown, 90 - 60);
}

/// Issue #34 acceptance: a grab held stays linked at most 2 s (GD § 15), the two karts within
/// the tether's reach all along; then the paw rests 3 s, and pressing again meanwhile does
/// nothing.
#[test]
fn a_grab_held_links_the_two_karts_for_two_seconds_at_most() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    let tuning = InteractionTuning::default();
    let mut linked = Vec::new();
    for tick in 0..(4.0 * TICKS_PER_SECOND) as usize {
        arena.tick(&[(a, PAW), (b, FULL_THROTTLE)]);
        let (pa, pb) = (arena.paw(a), arena.paw(b));
        let both = pa.partner() == Some(b) && pb.partner() == Some(a);
        assert_eq!(pa.partner().is_some(), both, "tick {tick}: one side only");
        if both {
            linked.push(tick);
            let PawMotion::Holding { rest, .. } = pa.motion else {
                unreachable!()
            };
            let distance = (arena.position(b) - arena.position(a)).with_y(0.0).length();
            assert!(distance <= rest + tuning.tether_max_stretch);
        }
    }
    // Grabbed 0.2 s after the press, held 2 s.
    assert_eq!(linked.first(), Some(&11));
    assert_eq!(linked.len(), 120);
    assert_eq!(*linked.last().unwrap() - linked[0], 119, "in one go");
    let pa = arena.paw(a);
    assert_eq!(
        pa.motion,
        PawMotion::Spent,
        "the button still down does nothing"
    );
    assert!(pa.cooldown > 0 && pa.cooldown <= 180);
    assert_eq!(arena.stats(a).grabs, 1);
    assert_eq!(arena.stats(b).held, 1);
    assert!(!arena.get::<TetherPull>(a).is_active());
    // Released and pressed again during the cooldown: nothing.
    arena.tick(&[(a, FULL_THROTTLE)]);
    arena.run(0.5, &[(a, PAW)]);
    assert_eq!(arena.paw(a).partner(), None);
    assert_eq!(arena.stats(a).grabs, 1);
    // A croquette is never lost to a grab.
    assert_eq!(arena.croquettes(b), 3);
}

/// GD § 15: the grabber is towed up to the speed of a faster kart (a boost) in about 0.5 s,
/// never beyond it; the kart held is braked a little. Prints how much (the GD's tuning goal:
/// about 10 % on a full grab).
#[test]
fn the_grabber_is_towed_up_to_the_speed_of_a_faster_kart_never_beyond() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    let (mut alone, _, ab) = side_by_side(10, 10, 2.2);
    for (arena, kart) in [(&mut arena, b), (&mut alone, ab)] {
        arena
            .app
            .world_mut()
            .get_mut::<Boost>(kart)
            .unwrap()
            .add(0.3, 3.0);
    }
    let mut grabbed_at = None;
    let mut slowest = 1.0f32;
    for tick in 0..120 {
        arena.tick(&[(a, PAW), (b, FULL_THROTTLE)]);
        alone.tick(&[(ab, FULL_THROTTLE)]);
        let forward = *arena.get::<Transform>(a).forward();
        let b_along = arena.get::<Velocity>(b).linear.dot(forward);

        if arena.paw(a).holding() {
            let since = *grabbed_at.get_or_insert(tick);
            assert!(
                arena.speed(a) <= b_along + 0.05,
                "tick {tick}: {} beyond {b_along}",
                arena.speed(a)
            );
            if tick - since == 30 {
                assert!(
                    arena.speed(a) > b_along - 1.5 && arena.speed(a) > 30.0,
                    "towed: {} for {b_along}",
                    arena.speed(a)
                );
            }
            slowest = slowest.min(arena.speed(b) / alone.speed(ab));
        }
    }
    assert!(grabbed_at.is_some(), "never grabbed");
    println!(
        "the kart held (boosted) lost at most {:.1} % of its speed",
        100.0 * (1.0 - slowest)
    );
    assert!(
        (0.86..=0.94).contains(&slowest),
        "the kart held lost {:.1} %",
        100.0 * (1.0 - slowest)
    );
}

/// GD § 15's tuning goal: at equal Weights, a full grab of 2 s costs a kart at full speed about
/// 10 % of its speed; a light kart held is braked more than a heavy one (its Weight factor). It
/// gets its speed back once free.
#[test]
fn a_full_grab_costs_the_kart_held_about_ten_percent_of_its_speed() {
    let mut lost = Vec::new();
    for target_weight in [10, 0, 20] {
        let (mut arena, a, b) = side_by_side(10, target_weight, 2.2);
        let (mut alone, _, ab) = side_by_side(10, target_weight, 2.2);
        let mut slowest = 1.0f32;
        let mut ticks_held = 0;
        for _ in 0..150 {
            arena.tick(&[(a, PAW), (b, FULL_THROTTLE)]);
            alone.tick(&[(ab, FULL_THROTTLE)]);
            if arena.paw(b).held.is_some() {
                ticks_held += 1;
            }
            slowest = slowest.min(arena.speed(b) / alone.speed(ab));
        }
        assert_eq!(ticks_held, 120, "weight {target_weight}: a full grab");
        lost.push(1.0 - slowest);
        // Free again: back to its speed.
        arena.run(2.0, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
        alone.run(2.0, &[(ab, FULL_THROTTLE)]);
        assert!((arena.speed(b) - alone.speed(ab)).abs() < 0.05);
    }
    println!(
        "a full grab costs the kart held (Weight 10, 0, 20): {:.1} %, {:.1} %, {:.1} %",
        100.0 * lost[0],
        100.0 * lost[1],
        100.0 * lost[2]
    );
    assert!((0.08..=0.12).contains(&lost[0]), "lost {:.3}", lost[0]);
    assert!(lost[1] > lost[0] && lost[0] > lost[2], "{lost:?}");
}

/// Issue #34 acceptance: the kart held jumps, the tether breaks (when it lands, GD § 15), and
/// the grabber flails off balance.
#[test]
fn the_kart_held_jumps_and_the_tether_breaks_when_it_lands() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    arena.run(0.5, &[(a, PAW), (b, FULL_THROTTLE)]);
    assert!(arena.paw(a).holding() && arena.paw(b).held.is_some());
    let jump = KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    };
    arena.tick(&[(a, PAW), (b, jump)]);
    assert!(matches!(arena.get::<KartState>(b), KartState::Hop { .. }));
    let mut landed = None;
    for tick in 1..60 {
        arena.tick(&[(a, PAW), (b, FULL_THROTTLE)]);
        let state = arena.get::<KartState>(b);
        if landed.is_none() && !matches!(state, KartState::Hop { .. }) {
            landed = Some(tick);
        }
        if arena.paw(b).held.is_none() {
            let landed = landed.expect("the tether broke before the landing");
            assert!(tick - landed <= 1, "broke {} ticks after", tick - landed);
            assert!(tick >= 8, "not on the press");
            break;
        }
        assert!(arena.paw(a).holding(), "tick {tick}");
    }
    assert!(arena.paw(b).held.is_none() && !arena.paw(a).holding());
    assert_eq!(arena.stats(b).escapes, 1);
    let pa = arena.paw(a);
    assert!(pa.off_balance > 0, "the grabber flails");
    assert!(pa.cooldown > 150);
}

/// Issue #34 acceptance: a strike or a grab during the cooldown does nothing.
#[test]
fn a_strike_or_a_grab_during_the_cooldown_does_nothing() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    tap(&mut arena, a, &[b]);
    assert_eq!(arena.stats(b).struck, 1);
    arena.run(1.1, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    // B is no longer protected, A still rests: a strike...
    assert_eq!(arena.paw(b).protected, 0);
    assert!(arena.paw(a).cooldown > 0);
    tap(&mut arena, a, &[b]);
    // ...and a grab.
    arena.run(0.6, &[(a, PAW), (b, FULL_THROTTLE)]);
    arena.tick(&[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    assert_eq!(arena.stats(b).struck, 1, "not struck again");
    assert_eq!(arena.stats(b).held, 0, "not grabbed");
    assert_eq!(arena.stats(a).strikes, 1);
    assert_eq!(arena.croquettes(b), 2);
    // Once rested (the karts drifted apart a little: back side by side), it works again.
    assert_eq!(arena.paw(a).cooldown, 0);
    let pa = arena.get::<Transform>(a);
    let pb =
        Transform::from_translation(pa.translation + *pa.right() * 2.2).with_rotation(pa.rotation);
    let v = arena.get::<Velocity>(a);
    arena.place(b, pb, v);
    tap(&mut arena, a, &[b]);
    assert_eq!(arena.stats(b).struck, 2);
}

/// Enters a right drift at full speed: a hop steered right, the button held through the landing.
fn drift_right(arena: &mut Arena, kart: Entity) {
    let hop = KartInput {
        jump_drift: true,
        steer: 1.0,
        ..FULL_THROTTLE
    };
    for _ in 0..40 {
        arena.tick(&[(kart, hop)]);
        if matches!(arena.get::<KartState>(kart), KartState::Drifting { .. }) {
            return;
        }
    }
    panic!("no drift: {:?}", arena.get::<KartState>(kart));
}

fn drift_charge(arena: &Arena, kart: Entity) -> f32 {
    match arena.get::<KartState>(kart) {
        KartState::Drifting { charge, .. } => charge,
        other => panic!("not drifting: {other:?}"),
    }
}

/// GD § 15: striking in a drift keeps the drift and its charge; struck in a drift, too.
#[test]
fn a_strike_never_breaks_a_drift_given_or_received() {
    let drifting = KartInput {
        jump_drift: true,
        steer: 1.0,
        ..FULL_THROTTLE
    };
    for striker_drifts in [true, false] {
        let (mut arena, a, b) = side_by_side(10, 10, 2.2);
        let (drifter, other) = if striker_drifts { (a, b) } else { (b, a) };
        drift_right(&mut arena, drifter);
        arena.run(0.3, &[(drifter, drifting), (other, FULL_THROTTLE)]);
        // The other kart next to the drifter (the striker on the left of its target).
        let at = arena.get::<Transform>(drifter);
        let side = if striker_drifts { 2.2 } else { -2.2 };
        let place = Transform::from_translation(at.translation + *at.right() * side)
            .with_rotation(at.rotation);
        let velocity = arena.get::<Velocity>(drifter);
        arena.place(other, place, velocity);
        let before = drift_charge(&arena, drifter);
        let striker_input = |drifts: bool, interact: bool| KartInput {
            interact,
            ..if drifts { drifting } else { FULL_THROTTLE }
        };
        arena.tick(&[
            (a, striker_input(striker_drifts, true)),
            (b, striker_input(!striker_drifts, false)),
        ]);
        arena.tick(&[
            (a, striker_input(striker_drifts, false)),
            (b, striker_input(!striker_drifts, false)),
        ]);
        assert_eq!(arena.stats(b).struck, 1, "striker drifts: {striker_drifts}");
        let side_of = |drifts: bool| if drifts { drifting } else { FULL_THROTTLE };
        arena.run(
            0.3,
            &[(a, side_of(striker_drifts)), (b, side_of(!striker_drifts))],
        );
        assert!(
            matches!(
                arena.get::<KartState>(drifter),
                KartState::Drifting {
                    side: DriftSide::Right,
                    ..
                }
            ),
            "striker drifts: {striker_drifts}"
        );
        assert!(drift_charge(&arena, drifter) > before + 50.0);
    }
}

/// GD § 15 and issue #27: a kart against a wall, struck or thrown toward it (the heaviest
/// striker on the lightest kart: the strongest push), bumps into the wall and stays on its
/// side, on its wheels; it never goes over.
#[test]
fn a_kart_struck_against_a_wall_never_goes_over_it() {
    for throw in [false, true] {
        let mut arena = Arena::new();
        let a = arena.kart(0.0, 0.0, 20);
        let b = arena.kart(2.2, 0.0, 0);
        // The wall's face 5 cm from the kart's side, 1.2 m high (the circuit's walls, #27).
        let face = 2.2 + 0.7 + 0.05;
        arena.wall(
            Vec3::new(face + 0.5, 0.6, -150.0),
            Vec3::new(0.5, 0.6, 200.0),
        );
        arena.run(0.5, &[]);
        arena.run(2.5, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
        if throw {
            arena.run(0.8, &[(a, PAW), (b, FULL_THROTTLE)]);
            assert!(arena.paw(a).holding());
            let right = KartInput {
                steer: 1.0,
                ..FULL_THROTTLE
            };
            arena.tick(&[(a, right), (b, FULL_THROTTLE)]);
            assert_eq!(arena.stats(a).throws, 1);
        } else {
            tap(&mut arena, a, &[b]);
        }
        assert_eq!(arena.stats(b).struck, 1, "throw: {throw}");
        for _ in 0..120 {
            arena.tick(&[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
            let at = arena.position(b);
            let up = arena.get::<Transform>(b).up().y;
            assert!(at.x < face - 0.5, "throw {throw}: beyond the wall at {at}");
            assert!(at.y < 0.9, "throw {throw}: lifted to {at}");
            assert!(up > 0.9, "throw {throw}: tipped ({up})");
        }
    }
}

/// GD § 15 (Weight): a heavy kart pushes a light one further than the other way round (× 1.3
/// against × 0.7); the croquettes do not change.
#[test]
fn a_heavy_kart_pushes_a_light_one_further_than_the_reverse() {
    let heavy_on_light = strike_effect(20, 0, 2.2);
    let even = strike_effect(10, 10, 2.2);
    let light_on_heavy = strike_effect(0, 20, 2.2);
    assert!(
        heavy_on_light.moved > even.moved && even.moved > light_on_heavy.moved,
        "{} {} {}",
        heavy_on_light.moved,
        even.moved,
        light_on_heavy.moved
    );
    let ratio = heavy_on_light.moved / light_on_heavy.moved;
    println!(
        "pushed aside 1 s after the strike: heavy on light {:.2} m, even {:.2} m, light on heavy \
         {:.2} m; speed × {:.3}",
        heavy_on_light.moved, even.moved, light_on_heavy.moved, even.speed_ratio
    );
    assert!((1.6..=2.1).contains(&ratio), "{ratio}");
    for effect in [heavy_on_light, even, light_on_heavy] {
        assert_eq!(effect.croquettes, 2);
        assert!((0.88..=0.93).contains(&effect.speed_ratio));
    }
}

/// The immunities of GD § 15 on real karts: an invincible kart or one spinning after an item is
/// not struck (the paw strikes thin air); a stalled engine cannot strike; a kart drifting cannot
/// be grabbed, nor grab; a kart struck is protected for 1 s from another kart's paw.
#[test]
fn immune_karts_are_neither_struck_nor_grabbed() {
    // Invincible, and spinning after an item hit.
    for spinning in [false, true] {
        let (mut arena, a, b) = side_by_side(10, 10, 2.2);
        if spinning {
            *arena.app.world_mut().get_mut::<KartState>(b).unwrap() =
                KartState::hit(crate::kart::HitReaction::Spin, 1.0, 60.0);
        } else {
            arena
                .app
                .world_mut()
                .get_mut::<Invincibility>(b)
                .unwrap()
                .start(2.0);
        }
        tap(&mut arena, a, &[b]);
        assert_eq!(arena.stats(b).struck, 0, "spinning: {spinning}");
        assert_eq!(arena.stats(a).missed, 1);
        assert_eq!(arena.croquettes(b), 3);
    }
    // A stalled engine (rocket start pressed too early).
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    arena.app.world_mut().get_mut::<Stall>(a).unwrap().remaining = 1.0;
    tap(&mut arena, a, &[b]);
    assert_eq!(arena.stats(a), PawStats::default());
    assert_eq!(arena.stats(b).struck, 0);
    // A kart drifting is not grabbed; a kart drifting does not grab.
    let drifting = KartInput {
        jump_drift: true,
        steer: 1.0,
        ..FULL_THROTTLE
    };
    for grabber_drifts in [false, true] {
        let (mut arena, a, b) = side_by_side(10, 10, 2.2);
        let drifter = if grabber_drifts { a } else { b };
        drift_right(&mut arena, drifter);
        let at = arena.get::<Transform>(drifter);
        let side = if grabber_drifts { 2.2 } else { -2.2 };
        let place = Transform::from_translation(at.translation + *at.right() * side)
            .with_rotation(at.rotation);
        let velocity = arena.get::<Velocity>(drifter);
        let other = if grabber_drifts { b } else { a };
        arena.place(other, place, velocity);
        let input = |kart: Entity| {
            let base = if kart == drifter {
                drifting
            } else {
                FULL_THROTTLE
            };
            KartInput {
                interact: kart == a,
                ..base
            }
        };
        arena.run(0.5, &[(a, input(a)), (b, input(b))]);
        assert_eq!(arena.stats(b).held, 0, "grabber drifts: {grabber_drifts}");
        assert!(!arena.paw(a).holding());
    }
    // Protected 1 s after a strike: a third kart's paw goes to thin air.
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    let c = arena.kart(4.4, 0.0, 10);
    let at = arena.get::<Transform>(b);
    let velocity = arena.get::<Velocity>(b);
    arena.place(
        c,
        Transform::from_translation(at.translation + Vec3::X * 2.2),
        velocity,
    );
    arena.tick(&[(c, FULL_THROTTLE)]);
    tap(&mut arena, a, &[b, c]);
    assert_eq!(arena.stats(b).struck, 1);
    // C, on B's right, strikes left (toward B) during the protection.
    let left = KartInput {
        steer: -1.0,
        ..FULL_THROTTLE
    };
    arena.tick(&[
        (a, FULL_THROTTLE),
        (b, FULL_THROTTLE),
        (
            c,
            KartInput {
                interact: true,
                ..left
            },
        ),
    ]);
    arena.tick(&[(a, FULL_THROTTLE), (b, FULL_THROTTLE), (c, left)]);
    assert_eq!(arena.stats(c).strikes, 1);
    assert_eq!(arena.stats(b).struck, 1, "protected");
}

/// GD § 15: letting go after 0.5 s while steering hard throws the kart held to that side: a
/// strike 1.5 times stronger, with the speed loss and the croquette; 3 s of cooldown.
#[test]
fn letting_go_while_steering_throws_the_kart_held() {
    let steer_right = KartInput {
        steer: 1.0,
        ..FULL_THROTTLE
    };
    let mut moved = [0.0; 2];
    for (i, throw) in [false, true].into_iter().enumerate() {
        let (mut arena, a, b) = side_by_side(10, 10, 2.2);
        arena.run(0.8, &[(a, PAW), (b, FULL_THROTTLE)]);
        assert!(arena.paw(a).holding());
        let x = arena.position(b).x;
        let release = if throw { steer_right } else { FULL_THROTTLE };
        arena.tick(&[(a, release), (b, FULL_THROTTLE)]);
        assert!(!arena.paw(a).holding());
        assert_eq!(arena.paw(a).cooldown, 180);
        arena.run(1.0, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
        moved[i] = arena.position(b).x - x;
        assert_eq!(arena.stats(a).throws, throw as u32);
        assert_eq!(arena.croquettes(b), if throw { 2 } else { 3 });
    }
    let thrown = moved[1] - moved[0];
    let strike = strike_effect(10, 10, 2.2).moved;
    println!("thrown {thrown:.2} m aside, a strike {strike:.2} m");
    assert!(
        thrown > 1.3 * strike && thrown < 1.8 * strike,
        "thrown {thrown:.2} m, a strike {strike:.2} m"
    );
}

/// docs/ARCHITECTURE.md: the kart struck applies the strike itself, from the message (what the
/// network will carry); a message too old, or from out of reach, is ignored, and the answer
/// says so.
#[test]
fn the_kart_struck_applies_the_message_itself() {
    let (mut arena, a, b) = side_by_side(10, 10, 2.2);
    // Issue #9: an event's age counts from when it was sent (`tick`; solo, the tick it saw too).
    let message = |arena: &Arena, ago: u32, from: Vec3| {
        let now = (arena
            .app
            .world()
            .resource::<Time<Fixed>>()
            .elapsed_secs_f64()
            * crate::TICK_HZ)
            .round() as u32;
        KartStruck {
            striker: a,
            target: b,
            tick: now - ago,
            seen: now - ago,
            striker_position: from,
            direction: Vec3::X,
            push: 7.0,
            throw: false,
        }
    };
    let a_at = arena.position(a);
    // Too old (0.5 s), then too far (8 m): ignored.
    for (ago, from) in [(30, a_at), (0, a_at - Vec3::X * 6.0)] {
        let struck = message(&arena, ago, from);
        arena.app.world_mut().write_message(struck);
        arena.tick(&[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
        assert_eq!(arena.stats(b).struck, 0);
        assert_eq!(arena.stats(a).landed, 0);
    }
    // Seen 0.2 s ago from where the striker is: applied by the kart struck.
    let speed = arena.speed(b);
    let struck = message(&arena, 12, arena.position(a));
    arena.app.world_mut().write_message(struck);
    arena.tick(&[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    assert_eq!(arena.stats(b).struck, 1);
    assert_eq!(arena.stats(a).landed, 1, "the answer reached the striker");
    assert!(arena.get::<Shove>(b).is_active());
    assert!(arena.speed(b) < speed * 0.93);
    assert_eq!(arena.croquettes(b), 2);
    // The striker's own paw did nothing.
    assert_eq!(arena.paw(a).cooldown, 0);
}

/// The whole game on Prairie Pompon: X strikes the kart alongside (keyboard → `interact`), the
/// paw shows, the kart struck gets its "pof"; struck back, the player's camera shakes and the
/// HUD's paw gauge empties then fills up.
#[test]
fn x_strikes_on_the_keyboard_the_paw_shows_and_the_camera_shakes() {
    use crate::camera::CameraShake;
    use crate::tests::{app_on, hold_keys, player};
    use crate::track::TrackChoice;

    let mut app = app_on(TrackChoice::default());
    let me = player(&mut app);
    while app.world().get_resource::<crate::race::Race>().is_none() {
        app.update();
    }
    assert!(crate::admin::skip_countdown_now(app.world_mut()));
    for _ in 0..3 {
        app.update();
    }
    // Another kart (with its visual) alongside, to the right.
    let at = *app.world().get::<Transform>(me).unwrap();
    let other = {
        let world = app.world_mut();
        let tuning = world.resource::<KartTuning>().clone();
        let place = Transform::from_translation(at.translation + *at.right() * 2.2)
            .with_rotation(at.rotation);
        let kart = world.spawn(crate::kart::kart_body(&tuning, place)).id();
        let visuals = world.resource::<crate::kart::KartVisualAssets>().clone();
        let mut commands = world.commands();
        crate::kart::spawn_kart_visual(
            &mut commands,
            &visuals,
            kart,
            crate::kart::KartColor::White,
        );
        world.flush();
        kart
    };
    for _ in 0..20 {
        app.update();
    }
    let visible = |app: &mut App, body: Entity, arm: bool| {
        let world = app.world_mut();
        if arm {
            world
                .query::<(&PawArm, &Visibility)>()
                .iter(world)
                .any(|(a, v)| a.body == body && *v != Visibility::Hidden)
        } else {
            world
                .query::<(&PawPof, &Visibility)>()
                .iter(world)
                .any(|(p, v)| p.body == body && *v != Visibility::Hidden)
        }
    };
    assert!(!visible(&mut app, me, true));
    // X down: the paw rises at once.
    hold_keys(&mut app, &[KeyCode::KeyX]);
    app.update();
    assert!(app.world().get::<Paw>(me).unwrap().button);
    assert!(visible(&mut app, me, true), "the paw is up");
    // Released: the strike; the other kart's "pof".
    hold_keys(&mut app, &[]);
    app.update();
    app.update();
    assert_eq!(app.world().get::<PawStats>(other).unwrap().struck, 1);
    assert!(visible(&mut app, other, false), "pof");
    let gauge = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Node, With<visual::PawGaugeFill>>()
            .single(world)
            .map(|node| node.height)
            .unwrap()
    };
    assert!(gauge(&mut app) != Val::Percent(100.0), "the gauge emptied");
    // The other kart strikes back (on its left): the player's camera shakes.
    assert!(app.world().get::<CameraShake>(me).is_none());
    let left = KartInput {
        steer: -1.0,
        interact: true,
        ..default()
    };
    *app.world_mut().get_mut::<KartInput>(other).unwrap() = left;
    app.update();
    *app.world_mut().get_mut::<KartInput>(other).unwrap() = KartInput {
        interact: false,
        ..left
    };
    app.update();
    app.update();
    assert_eq!(app.world().get::<PawStats>(me).unwrap().struck, 1);
    assert!(
        app.world().get::<CameraShake>(me).is_some(),
        "the camera shakes"
    );
    // 1.5 s later the gauge is full again.
    for _ in 0..100 {
        app.update();
    }
    assert_eq!(gauge(&mut app), Val::Percent(100.0));
}
