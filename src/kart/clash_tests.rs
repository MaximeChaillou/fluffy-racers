//! Issue #154: two karts that hit each other. Headless scenarios on a flat slab (the real
//! `SimulationPlugin`, Rapier and the karts, 60 Hz): each crash is run with both karts and with
//! each kart alone (the same start, the same inputs), and what the crash did to a kart is the
//! difference between the two runs. The measurement and the road test also run their crashes with
//! the contacts left as the solver gives them (`unsoftened`, the physics before #154), to show
//! what changed.

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::track::{Surface, spawn_static_box};
use stats::{KartStats, Stat, StatsTuning};

const TICKS_PER_SECOND: f32 = 60.0;

/// One kart of a crash: where it starts (on the slab, heading `heading` degrees from -Z toward
/// +X, the right), how fast it drives off, boosted or not (the boost's strength), its Weight.
#[derive(Clone, Copy, Debug)]
struct Driver {
    x: f32,
    z: f32,
    heading: f32,
    speed: f32,
    boost: f32,
    weight: u8,
}

impl Driver {
    fn new(x: f32, z: f32, heading: f32, speed: f32) -> Self {
        Self {
            x,
            z,
            heading,
            speed,
            boost: 0.0,
            weight: 10,
        }
    }

    fn boosted(self, boost: f32) -> Self {
        Self { boost, ..self }
    }

    fn weight(self, weight: u8) -> Self {
        Self { weight, ..self }
    }

    fn with_speed(self, speed: f32) -> Self {
        Self { speed, ..self }
    }

    fn shifted(self, dx: f32) -> Self {
        Self {
            x: self.x + dx,
            ..self
        }
    }

    fn direction(&self) -> Vec3 {
        let heading = self.heading.to_radians();
        Vec3::new(heading.sin(), 0.0, -heading.cos())
    }
}

/// A kart at one tick.
#[derive(Clone, Copy, Debug)]
struct Sample {
    position: Vec3,
    /// Degrees from -Z toward +X.
    heading: f32,
    velocity: Vec3,
    yaw_rate: f32,
    uprightness: f32,
}

/// Runs `drivers` together on the slab for `seconds` with this tuning, `input(tick, kart, its last
/// sample)` driving each kart; one sample per kart per tick.
fn run(
    tuning: &KartTuning,
    drivers: &[Driver],
    seconds: f32,
    input: &impl Fn(usize, usize, &Sample) -> KartInput,
) -> Vec<Vec<Sample>> {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, SimulationPlugin))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
    app.finish();
    app.cleanup();
    app.insert_resource(tuning.clone());
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
    let tuning = world.resource::<KartTuning>().clone();
    let stats_tuning = world.resource::<StatsTuning>().clone();
    let karts: Vec<Entity> = drivers
        .iter()
        .map(|driver| {
            let stats = KartStats::default().with(Stat::Weight, driver.weight);
            let transform = Transform::from_xyz(driver.x, 0.55, driver.z)
                .looking_to(driver.direction(), Vec3::Y);
            world
                .spawn(kart_body_with_stats(
                    &tuning,
                    &stats_tuning,
                    stats,
                    transform,
                ))
                .id()
        })
        .collect();
    app.update();
    // The suspension settles, then each kart drives off at its speed.
    for _ in 0..30 {
        app.update();
    }
    for (kart, driver) in karts.iter().zip(drivers) {
        let world = app.world_mut();
        world.get_mut::<Velocity>(*kart).unwrap().linear = driver.direction() * driver.speed;
        if driver.boost > 0.0 {
            world
                .get_mut::<Boost>(*kart)
                .unwrap()
                .add(driver.boost, 10.0);
        }
    }
    let sample = |app: &App, kart: Entity| {
        let world = app.world();
        let transform = world.get::<Transform>(kart).unwrap();
        let velocity = world.get::<Velocity>(kart).unwrap();
        let forward = transform.forward();
        Sample {
            position: transform.translation,
            heading: forward.x.atan2(-forward.z).to_degrees(),
            velocity: velocity.linear,
            yaw_rate: velocity.angular.y,
            uprightness: transform.up().y,
        }
    };
    let mut samples: Vec<Vec<Sample>> = karts.iter().map(|k| vec![sample(&app, *k)]).collect();
    for tick in 0..(seconds * TICKS_PER_SECOND).round() as usize {
        for (i, kart) in karts.iter().enumerate() {
            let last = *samples[i].last().unwrap();
            *app.world_mut().get_mut::<KartInput>(*kart).unwrap() = input(tick, i, &last);
        }
        app.update();
        for (i, kart) in karts.iter().enumerate() {
            samples[i].push(sample(&app, *kart));
        }
    }
    samples
}

/// What a crash did to one kart, against the same kart driving alone.
#[derive(Clone, Copy, Debug, Default)]
struct Effect {
    /// Tick of the first contact (the runs part by more than 2 cm), if any.
    contact: Option<usize>,
    /// Sideways move (along X) 0.5 s and 1 s after the contact (m).
    moved_half: f32,
    moved: f32,
    /// Heading turned by the crash, 1 s after (degrees).
    turned: f32,
    /// Largest sideways speed the crash gave (along X, m/s) and largest yaw rate (rad/s), in that
    /// second.
    side_speed: f32,
    yaw_rate: f32,
}

fn effect(together: &[Sample], alone: &[Sample]) -> Effect {
    let Some(contact) = together
        .iter()
        .zip(alone)
        .position(|(t, a)| t.position.distance(a.position) > 0.02)
    else {
        return Effect::default();
    };
    let contact = contact.saturating_sub(1);
    let at = |seconds: f32| (contact + (seconds * TICKS_PER_SECOND) as usize).min(alone.len() - 1);
    let one_second = at(1.0);
    let moved = |tick: usize| together[tick].position.x - alone[tick].position.x;
    let window = contact..=one_second;
    Effect {
        contact: Some(contact),
        moved_half: moved(at(0.5)),
        moved: moved(one_second),
        turned: together[one_second].heading - alone[one_second].heading,
        side_speed: window
            .clone()
            .map(|t| (together[t].velocity.x - alone[t].velocity.x).abs())
            .fold(0.0, f32::max),
        yaw_rate: window
            .clone()
            .map(|t| together[t].yaw_rate.abs())
            .fold(0.0, f32::max),
    }
}

/// Every kart's effect in this crash, each kart left alone with the same inputs.
fn crash(
    tuning: &KartTuning,
    drivers: &[Driver],
    seconds: f32,
    input: &impl Fn(usize, usize, &Sample) -> KartInput,
) -> Vec<Effect> {
    let together = run(tuning, drivers, seconds, input);
    drivers
        .iter()
        .enumerate()
        .map(|(i, driver)| {
            let alone = run(tuning, &[*driver], seconds, &|tick, _, last| {
                input(tick, i, last)
            });
            effect(&together[i], &alone[0])
        })
        .collect()
}

const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
    interact: false,
};

fn straight(_: usize, _: usize, _: &Sample) -> KartInput {
    FULL_THROTTLE
}

/// The tuning of the game.
fn softened() -> KartTuning {
    KartTuning::default()
}

/// The contacts between karts left as the solver gives them: the physics before issue #154.
fn unsoftened() -> KartTuning {
    KartTuning {
        bump_spin_keep: 1.0,
        bump_spin_max: f32::MAX,
        ..default()
    }
}

/// A kart (`a`, at the origin, 0.5 m behind) crossing at `angle` degrees into the left flank of
/// a kart driving straight down -Z (`b`), 30 cm between them.
fn swerve(angle: f32, speed: f32) -> [Driver; 2] {
    let radians = angle.to_radians();
    let x = 0.7 * radians.cos() + 1.0 * radians.sin() + 0.7 + 0.3;
    [
        Driver::new(0.0, 0.5, angle, speed),
        Driver::new(x, 0.0, 0.0, speed),
    ]
}

/// Two karts side by side at full speed, 2.2 m apart, around x = 0.
fn side_by_side(speed: f32) -> [Driver; 2] {
    [
        Driver::new(-1.1, 0.0, 0.0, speed),
        Driver::new(1.1, 0.0, 0.0, speed),
    ]
}

/// A boosted kart 3 m behind another one at top speed, `offset` m to its left: it rear-ends the
/// other one off center.
fn rear_end(offset: f32) -> [Driver; 2] {
    let top = KartTuning::default().max_speed;
    [
        Driver::new(-offset / 2.0, 3.0, 0.0, top * 1.3).boosted(0.3),
        Driver::new(offset / 2.0, 0.0, 0.0, top),
    ]
}

/// Two karts at `speed` head on, 14 m apart, `offset` m off each other's center.
fn head_on(offset: f32, speed: f32) -> [Driver; 2] {
    [
        Driver::new(-offset / 2.0, 0.0, 0.0, speed),
        Driver::new(offset / 2.0, -14.0, 180.0, speed),
    ]
}

/// The wheel turned fully toward the other kart (right) for `ticks` ticks by the first kart,
/// then straight.
fn wheel_into_the_other(ticks: usize) -> impl Fn(usize, usize, &Sample) -> KartInput {
    move |tick, kart, _| KartInput {
        steer: if kart == 0 && tick < ticks { 1.0 } else { 0.0 },
        ..FULL_THROTTLE
    }
}

/// Issue #154, measured: what a crash between two karts does to each of them, the drivers holding
/// the accelerator and the wheel straight for 1 s, before (`unsoftened`) and after the contacts
/// were softened. STATUS.md has the numbers of `dev` before #154 (a few degrees apart from
/// `unsoftened`: the systems differ, and the contacts with them).
#[test]
#[ignore = "measurements of issue #154: cargo test --lib clash_tests -- --ignored --nocapture"]
fn measure_crashes_between_karts() {
    let top = KartTuning::default().max_speed;
    let boosted = top * 1.3;
    let mut cases: Vec<(String, Vec<Driver>, usize)> = Vec::new();
    for angle in [5.0, 10.0, 20.0, 30.0, 45.0] {
        cases.push((format!("swerve {angle}°"), swerve(angle, top).to_vec(), 0));
    }
    let [a, b] = swerve(20.0, top);
    cases.push((
        "swerve 20°, heavy into light".into(),
        vec![a.weight(20), b.weight(0)],
        0,
    ));
    cases.push((
        "swerve 20°, light into heavy".into(),
        vec![a.weight(0), b.weight(20)],
        0,
    ));
    let [a, b] = swerve(20.0, boosted);
    cases.push((
        "swerve 20°, the swerver boosted".into(),
        vec![a.boosted(0.3), b.with_speed(top)],
        0,
    ));
    for ticks in [6, 12, 18] {
        cases.push((
            format!("wheel turned {ticks} ticks into the other"),
            side_by_side(top).to_vec(),
            ticks,
        ));
    }
    let [a, b] = rear_end(0.7);
    cases.push(("rear end, boosted, 0.7 m off center".into(), vec![a, b], 0));
    cases.push((
        "rear end, boosted, light into heavy".into(),
        vec![a.weight(0), b.weight(20)],
        0,
    ));
    cases.push((
        "head on, 20 m/s each, 0.7 m off center".into(),
        head_on(0.7, 20.0).to_vec(),
        0,
    ));
    for (physics, tuning) in [("before", unsoftened()), ("after", softened())] {
        for (name, drivers, ticks) in &cases {
            for (i, e) in crash(&tuning, drivers, 1.5, &wheel_into_the_other(*ticks))
                .iter()
                .enumerate()
            {
                println!(
                    "{physics:6} {name:40} kart {i}: moved {:+.2} m (0.5 s) {:+.2} m (1 s), \
                     turned {:+.1}°, side speed {:.1} m/s, yaw {:.2} rad/s",
                    e.moved_half, e.moved, e.turned, e.side_speed, e.yaw_rate
                );
            }
        }
    }
}

/// The road of the crashes on a straight: 12 m wide (the narrowest main road, the Ruban
/// Réglisse's; Prairie Pompon's and the Col's are 14 m), along -Z around x = 0. A kart is on it
/// while its center is less than this from the middle: half the road minus half a kart.
const ON_THE_ROAD: f32 = 6.0 - KART_HALF_EXTENTS.x;

/// A driver keeping its lane: it steers back toward the road's direction (`road`: 0 for -Z, 180
/// for +Z) and toward its lane (`lane`, along X), full throttle.
fn keep_lane(road: f32, lane: f32, sample: &Sample) -> KartInput {
    let wrap = |angle: f32| {
        (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
    };
    let road = road.to_radians();
    let heading = wrap(sample.heading.to_radians() - road);
    // How far right of its lane it is, seen from its seat.
    let off = (sample.position.x - lane) * road.cos();
    let wanted = (-off * 0.08).clamp(-0.25, 0.25);
    KartInput {
        steer: (-(heading - wanted) * 3.0).clamp(-1.0, 1.0),
        ..FULL_THROTTLE
    }
}

/// How far from the middle of the road each kart went (m), the worst uprightness of each, and
/// whether they touched: the karts of `drivers` drive as `wheel` says until `reaction` ticks after
/// the start, then they keep their lane.
fn widest(
    tuning: &KartTuning,
    drivers: &[Driver],
    wheel: usize,
    reaction: usize,
) -> (Vec<f32>, Vec<f32>, bool) {
    let lanes: Vec<(f32, f32)> = drivers
        .iter()
        .map(|d| (if d.heading.abs() > 90.0 { 180.0 } else { 0.0 }, d.x))
        .collect();
    let swerve = wheel_into_the_other(wheel);
    let input = |tick: usize, kart: usize, last: &Sample| {
        if tick < reaction {
            swerve(tick, kart, last)
        } else {
            keep_lane(lanes[kart].0, lanes[kart].1, last)
        }
    };
    let together = run(tuning, drivers, 2.5, &input);
    let touched = drivers.iter().enumerate().any(|(i, driver)| {
        let alone = run(tuning, &[*driver], 2.5, &|t, _, l| input(t, i, l));
        effect(&together[i], &alone[0]).contact.is_some()
    });
    let widest = together
        .iter()
        .map(|s| s.iter().map(|s| s.position.x.abs()).fold(0.0, f32::max))
        .collect();
    let upright = together
        .iter()
        .map(|s| s.iter().map(|s| s.uprightness).fold(1.0, f32::min))
        .collect();
    (widest, upright, touched)
}

/// Issue #154 acceptance (Max: « un kart qui tombe ou sort de la piste c'est ok pour moi, mais pas
/// quand tu es au milieu de la piste »): two karts that hit each other in the middle of a
/// straight, at full speed, both stay on the road. Crossing into each other (de biais), head on
/// (de face) and rear-ended off center, at the same Weight, heavy against light, one of them
/// boosted. The drivers react 0.4 s after the start (0.25 to 0.4 s after the contact, a player's
/// reflex) and keep their lane. Alone, each kart stays on the road: whoever leaves it was sent
/// there by the crash.
///
/// Before #154 (the contacts as the solver gives them), the off-center hits threw the karts off
/// a 12 m road (up to 10 m from its middle, a boosted kart rear-ending another 1 m off center):
/// checked here too, so that the test shows what it guards.
#[test]
fn two_karts_crashing_in_the_middle_of_a_straight_both_stay_on_the_road() {
    let top = KartTuning::default().max_speed;
    let boosted = top * 1.3;
    let mut cases: Vec<(String, Vec<Driver>, usize)> = Vec::new();
    // De biais.
    let [a, b] = swerve(20.0, top).map(|d| d.shifted(-1.2));
    cases.push(("swerve 20°".into(), vec![a, b], 0));
    cases.push((
        "swerve 20°, heavy into light".into(),
        vec![a.weight(20), b.weight(0)],
        0,
    ));
    cases.push((
        "swerve 20°, light into heavy".into(),
        vec![a.weight(0), b.weight(20)],
        0,
    ));
    let [a, b] = swerve(10.0, boosted).map(|d| d.shifted(-1.2));
    cases.push((
        "swerve 10°, the swerver boosted".into(),
        vec![a.boosted(0.3), b.with_speed(top)],
        0,
    ));
    let [a, b] = side_by_side(top);
    cases.push(("wheel turned 0.2 s into the other".into(), vec![a, b], 12));
    cases.push((
        "wheel turned 0.2 s, heavy into light".into(),
        vec![a.weight(20), b.weight(0)],
        12,
    ));
    // De face.
    for offset in [0.4, 0.7, 1.0] {
        cases.push((
            format!("head on, {offset} m off center"),
            head_on(offset, top).to_vec(),
            0,
        ));
    }
    let [a, b] = head_on(0.7, top);
    cases.push((
        "head on, 0.7 m off, heavy against light".into(),
        vec![a.weight(20), b.weight(0)],
        0,
    ));
    // Par l'arrière, en boost.
    for offset in [0.4, 0.7, 1.0] {
        cases.push((
            format!("rear end, boosted, {offset} m off center"),
            rear_end(offset).to_vec(),
            0,
        ));
    }
    let [a, b] = rear_end(0.7);
    cases.push((
        "rear end, boosted, heavy into light".into(),
        vec![a.weight(20), b.weight(0)],
        0,
    ));
    cases.push((
        "rear end, boosted, light into heavy".into(),
        vec![a.weight(0), b.weight(20)],
        0,
    ));

    let reaction = 24;
    let mut failures = Vec::new();
    let mut thrown_before = 0;
    for (name, drivers, wheel) in &cases {
        for (i, driver) in drivers.iter().enumerate() {
            let swerve = wheel_into_the_other(*wheel);
            let lane = if driver.heading.abs() > 90.0 {
                180.0
            } else {
                0.0
            };
            let alone = run(&softened(), &[*driver], 2.5, &|t, _, l| {
                if t < reaction {
                    swerve(t, i, l)
                } else {
                    keep_lane(lane, driver.x, l)
                }
            });
            let widest = alone[0]
                .iter()
                .map(|s| s.position.x.abs())
                .fold(0.0, f32::max);
            assert!(widest < ON_THE_ROAD, "{name}: kart {i} alone, {widest} m");
        }
        let (after, upright, touched) = widest(&softened(), drivers, *wheel, reaction);
        let (before, _, _) = widest(&unsoftened(), drivers, *wheel, reaction);
        println!(
            "{name:40} farthest from the middle: {:.2} m and {:.2} m (before #154: {:.2} m and \
             {:.2} m)",
            after[0], after[1], before[0], before[1]
        );
        if !touched {
            failures.push(format!("{name}: the karts never touched"));
        }
        if after.iter().any(|x| *x > ON_THE_ROAD) || upright.iter().any(|up| *up < 0.9) {
            failures.push(format!(
                "{name}: off the road ({after:?}) or tipped ({upright:?})"
            ));
        }
        thrown_before += before.iter().any(|x| *x > ON_THE_ROAD) as usize;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // What the test guards: without the softening, crashes in the middle threw karts off.
    assert!(
        thrown_before >= 4,
        "{thrown_before} crashes threw a kart off before #154"
    );
}

/// Issue #154: a crash between karts still hits, and the Weight still counts; the kart hit is not
/// spun round any more. The drivers hold the wheel straight for 1 s (no correction), so these are
/// what the crash did to each kart (`measure_crashes_between_karts` for the whole table).
#[test]
fn a_crash_still_pushes_the_kart_hit_without_spinning_it_round() {
    let top = KartTuning::default().max_speed;
    let tuning = softened();

    // Rear-ended 0.7 m off center by a boosted kart: both turned by 27° before, both went 13 m
    // sideways in 1 s. Now 6 to 8°, under 3 m: the hit is felt, the line is kept.
    let [a, b] = rear_end(0.7);
    let rear = crash(&tuning, &[a, b], 1.5, &straight);
    for (i, e) in rear.iter().enumerate() {
        println!("rear end, kart {i}: {e:?}");
        assert!(e.contact.is_some());
        assert!(
            (1.0..=10.0).contains(&e.turned.abs()),
            "rear end, kart {i}: {e:?}"
        );
        assert!(e.moved.abs() <= 4.0, "rear end, kart {i}: {e:?}");
    }
    assert!(
        rear[1].side_speed >= 1.0,
        "the kart hit was not pushed: {:?}",
        rear[1]
    );

    // Head on, 0.7 m off center, 20 m/s each: turned by 47° and 86° before, about 10° now.
    for (i, e) in crash(&tuning, &head_on(0.7, 20.0), 1.5, &straight)
        .iter()
        .enumerate()
    {
        println!("head on, kart {i}: {e:?}");
        assert!(
            (1.0..=14.0).contains(&e.turned.abs()),
            "head on, kart {i}: {e:?}"
        );
    }

    // The wheel turned 0.2 s into a kart alongside: the kart hit turned by 12.6° and went 5.5 m
    // aside in 1 s before; now it is still pushed aside (0.8 m at least in 0.5 s), and turned
    // half as much (5.6°, 3.4 m in 1 s).
    let hit = crash(&tuning, &side_by_side(top), 1.5, &wheel_into_the_other(12))[1];
    println!("wheel turned into the other, the kart hit: {hit:?}");
    assert!((0.8..=2.5).contains(&hit.moved_half), "{hit:?}");
    assert!((1.0..=4.0).contains(&hit.moved), "{hit:?}");
    assert!((0.5..=8.0).contains(&hit.turned.abs()), "{hit:?}");
    assert!(hit.side_speed >= 2.0, "{hit:?}");

    // The Weight: a heavy kart swerving into a light one pushes it much further than the other
    // way round (the solver's masses, untouched).
    let [a, b] = swerve(20.0, top);
    let heavy_on_light = crash(&tuning, &[a.weight(20), b.weight(0)], 1.5, &straight)[1];
    let light_on_heavy = crash(&tuning, &[a.weight(0), b.weight(20)], 1.5, &straight)[1];
    println!("heavy on light: {heavy_on_light:?}\nlight on heavy: {light_on_heavy:?}");
    assert!(
        (0.8..=2.0).contains(&heavy_on_light.moved_half),
        "{heavy_on_light:?}"
    );
    assert!(
        heavy_on_light.moved_half > 1.5 * light_on_heavy.moved_half,
        "heavy on light {heavy_on_light:?}, light on heavy {light_on_heavy:?}"
    );
    assert!(heavy_on_light.side_speed > 1.5 * light_on_heavy.side_speed);
}

/// Issue #154 (Max: « un kart qui tombe ou sort de la piste c'est ok pour moi »): a kart already
/// at the edge of the road (its flank 30 cm from it) is still pushed by a kart swerving into it,
/// and may leave the road: that is no error. Only its push is checked.
#[test]
fn a_kart_at_the_edge_of_the_road_is_still_pushed_and_may_leave_it() {
    let top = KartTuning::default().max_speed;
    let [a, b] = swerve(20.0, top);
    let edge = 6.0 - KART_HALF_EXTENTS.x - 0.3;
    let drivers = [a, b].map(|d| d.shifted(edge - b.x));
    let effects = crash(&softened(), &drivers, 1.5, &straight);
    let together = run(&softened(), &drivers, 1.5, &straight);
    let farthest = together[1]
        .iter()
        .map(|s| s.position.x)
        .fold(f32::MIN, f32::max);
    println!(
        "at the edge: {:?}; its center went {:.2} m from the middle ({})",
        effects[1],
        farthest,
        if farthest > ON_THE_ROAD {
            "off the road"
        } else {
            "still on it"
        }
    );
    assert!(effects[1].moved_half >= 0.3, "{:?}", effects[1]);
    assert!(farthest > edge + 0.3);
}
