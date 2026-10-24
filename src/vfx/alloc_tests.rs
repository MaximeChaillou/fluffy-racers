//! Issue #105 acceptance: **nothing allocated per frame** by the particles. A counting allocator
//! (the tests' only: it counts on the thread that asks it to, nothing else changes) watches the
//! pool and every emitter, then the particles' systems themselves running in a race of 12 karts.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, RefCell};

use bevy::ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use bevy::prelude::*;

use super::emit::{self, KartFrame};
use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::{Bot, BotDifficulty, BotTuning, bot, bot_seed};
use crate::kart::{MiniTurbo, PlayerKart};
use crate::race::{Race, RaceGrid, Racer};
use crate::tests::{game_app_unloaded, player, wait_for_the_player};
use crate::track::TrackChoice;

/// The system allocator, counting the allocations of the thread that switched it on.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
    /// Where the first allocations counted were made (what a failing test shows).
    static TRACES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn note() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            // Not counting while noting where it comes from (that allocates too).
            on.set(false);
            let first = COUNT.with(|count| {
                count.set(count.get() + 1);
                count.get() <= 3
            });
            if first {
                let trace = std::backtrace::Backtrace::force_capture().to_string();
                TRACES.with(|traces| traces.borrow_mut().push(trace));
            }
            on.set(true);
        }
    });
}

/// Where the first allocations counted were made.
fn traces() -> String {
    TRACES.with(|traces| traces.borrow().join("\n---\n"))
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How many allocations `f` makes (on this thread).
fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|count| count.set(0));
    TRACES.with(|traces| traces.borrow_mut().clear());
    COUNTING.with(|on| on.set(true));
    f();
    COUNTING.with(|on| on.set(false));
    COUNT.with(Cell::get)
}

/// The counter counts.
#[test]
fn the_counter_counts() {
    assert_eq!(allocations(|| {}), 0);
    assert!(allocations(|| drop(std::hint::black_box(vec![1u8; 64]))) >= 1);
}

/// Every effect thrown into the pool, the pool moving on and every particle's transform, frame
/// after frame: nothing allocated, even with the pool full. Issue #174: the drift's flames and
/// embers too, and the quads of the batched particles and of a halo written every frame.
#[test]
fn the_pool_and_the_emitters_allocate_nothing() {
    let tuning = VfxTuning::default();
    let mut pool = ParticlePool::new(300);
    let mut glow = Quads::new(300 + MAX_HALOS);
    let mut haze = Quads::new(300);
    let colors = QuadColors::default();
    let mut frame = KartFrame {
        velocity: Vec3::new(0.0, 0.0, -25.0),
        ..default()
    };
    let mut carry = [0.0f32; 4];
    let mut sum = Vec3::ZERO;
    let mut smoky = 0;
    let mut one_frame = |pool: &mut ParticlePool, frame: &mut KartFrame, k: usize| {
        let dt = 1.0 / 60.0;
        frame.position += frame.velocity * dt;
        let (drift, boost, impact, world) =
            (&tuning.drift, &tuning.boost, &tuning.impact, &tuning.world);
        let tier = MiniTurbo::ALL[k / 60 % 3];
        let style = spark_style(drift, tier, (k % 60) as f32 / 60.0);
        for side in [-1.0, 1.0] {
            let n = take(&mut carry[0], style.rate, dt, 6);
            emit::drift_sparks(pool, frame, side, &style, drift, n, dt);
            // Issue #174: the flames and the embers of the purple tier.
            let purple = spark_style(drift, MiniTurbo::Purple, 0.5);
            emit::drift_flames(pool, frame, side, &purple, drift, 2, dt);
            emit::drift_embers(pool, frame, side, Paint::Purple, drift, 1, dt);
            let n = take(&mut carry[1], drift.smoke_rate, dt, 6);
            emit::drift_smoke(pool, frame, side, Paint::Road, drift, n, dt);
            emit::dust(pool, frame, side, Paint::Ground, world, 1, dt);
        }
        // Issue #130: the wake on the water.
        emit::wake(pool, frame, frame.position.y - 0.3, world, 1, dt);
        // Issue #176: the tyres spinning, the cloud of a start.
        let wheels = &tuning.wheels;
        emit::spin_smoke(pool, frame, 1.0, Paint::Road, wheels, 1, dt);
        if k.is_multiple_of(30) {
            let look = (
                wheels.go_rocket_puffs,
                wheels.go_size,
                wheels.go_life,
                wheels.go_push,
            );
            emit::start_cloud(pool, frame, Paint::Ground, look, wheels);
        }
        let n = take(&mut carry[2], boost.ember_rate * 2.0, dt, 6);
        emit::embers(pool, frame, Paint::Flame, 1.0, boost, n, dt);
        emit::trail(pool, frame, Paint::Orange, (0.3, 0.13), 1, dt);
        emit::anti_gravity_sparks(pool, frame, 1, dt);
        emit::glide_wind(pool, frame, 1, dt);
        let n = take(&mut carry[3], boost.speed_line_rate, dt, 12);
        emit::speed_lines(pool, frame.position, Vec3::NEG_Z, frame.velocity, boost, n);
        if k.is_multiple_of(30) {
            emit::tier_flash(pool, frame, k % 3, drift);
            emit::release(pool, frame, k % 3, drift);
            emit::rocket_smoke(pool, frame, boost);
            emit::stall_puffs(pool, frame, boost);
            emit::shock(pool, frame.position, Vec3::X, frame.velocity, 6, 6, impact);
            emit::confetti(pool, frame.position, 4.0, impact);
            // Issue #106: a gift box taken.
            emit::gift(pool, frame.position, frame.velocity, impact);
            emit::cream(pool, frame, impact);
            emit::landing(pool, frame, Paint::Dust, world);
            // Issue #130: a splash, in and out of the water.
            emit::water_splash(
                pool,
                frame.position,
                frame.velocity,
                k.is_multiple_of(60),
                world,
            );
            emit::wind_burst(pool, frame, 14);
            emit::spray(pool, frame, Paint::Cyan, 18, false);
            emit::sparkles(
                pool,
                frame.position,
                frame.velocity,
                Paint::Gold,
                12,
                0.2,
                0.5,
                4.0,
            );
        }
        pool.step(dt);
        glow.clear();
        haze.clear();
        let camera = frame.position + Vec3::new(0.0, 3.0, 8.0);
        for particle in pool.slots().iter().filter(|p| p.alive()) {
            sum += particle.transform(Vec3::new(0.0, 3.0, 8.0)).translation;
            let quads = match particle.shape.batched() {
                Some(Batch::Haze) => &mut haze,
                _ => &mut glow,
            };
            super::batch::particle_quad(particle, camera, frame.position, &colors, quads);
        }
        let halo = GroundHalo {
            center: frame.position,
            up: Vec3::Y,
            size: 2.0,
            color: Vec3::ONE,
        };
        super::batch::halo_quad(&halo, frame.position, &mut glow);
        assert!(!glow.is_empty());
        smoky += usize::from(!haze.is_empty());
    };
    // Warm up, then count: the pool fills up and stays full.
    for k in 0..120 {
        one_frame(&mut pool, &mut frame, k);
    }
    let count = allocations(|| {
        for k in 120..1320 {
            one_frame(&mut pool, &mut frame, k);
        }
    });
    assert!(sum.is_finite());
    assert!(smoky > 600, "the smoke drawn: {smoky} frames");
    // Full all along: the new particles took the slots of live ones.
    assert!(pool.live() <= 300);
    assert!(pool.replaced > 1000, "{}", pool.replaced);
    assert_eq!(count, 0, "allocations over 1 200 frames:\n{}", traces());
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Counted;

/// A race of 12 karts on Prairie Pompon (the player's a Hard bot too), with the particles, their
/// systems in a schedule of their own (the game's copies switched off): run it after each frame
/// of the game to count or time them alone. Every kart throws, wherever it is; the stars over a
/// head are made once per kart, the camera's shake is a component put on the player's kart: both
/// left out (a kart's first hit makes them, not a frame). Issue #176: what the drivers press is
/// read by the game's own copy (`systems::watch_the_drivers`, before the race's gate: after the
/// frame, the karts' input on the grid is held at zero). Issue #130: the acting of the karts'
/// bodies and their drivers (`characters::ActingPlugin`) is counted with them.
fn counted_race() -> (App, Schedule) {
    counted_race_from(true)
}

/// The same, from GO (`skip_countdown`) or (issue #176) from the grid, the countdown played.
fn counted_race_from(skip_countdown: bool) -> (App, Schedule) {
    let mut app = game_app_unloaded(TrackChoice::default(), 11);
    app.add_plugins(VfxPlugin);
    app.configure_sets(FixedPostUpdate, VfxSystems::Watch.run_if(|| false));
    app.configure_sets(PostUpdate, VfxSystems::Show.run_if(|| false));
    // Issue #130: the acting of the karts' bodies and their drivers, counted with them.
    app.add_plugins(crate::characters::ActingPlugin);
    app.configure_sets(
        PostUpdate,
        crate::characters::ActingSystems.run_if(|| false),
    );
    wait_for_the_player(&mut app);
    for _ in 0..600 {
        let world = app.world_mut();
        if world.get_resource::<Race>().is_some() && world.query::<&Bot>().iter(world).count() == 11
        {
            break;
        }
        app.update();
    }
    app.update();
    let kart = player(&mut app);
    let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
    let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
    let bots = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(BotDifficulty::Hard, seed, &bots));
    {
        let mut tuning = app.world_mut().resource_mut::<VfxTuning>();
        tuning.emit_range = 10_000.0;
        tuning.impact.dizzy_stars = 0;
    }
    if skip_countdown {
        assert!(skip_countdown_now(app.world_mut()));
    }
    let mut counted = Schedule::new(Counted);
    counted.set_executor(SingleThreadedExecutor::new());
    counted.add_systems(
        (
            (
                systems::remember_karts,
                systems::watch_karts,
                systems::watch_world,
            )
                .chain(),
            (
                systems::follow_tuning,
                systems::follow_the_ground,
                systems::emit_karts,
                systems::emit_world,
                systems::kick_the_camera,
                systems::show_particles,
                systems::show_dizzy_stars,
                systems::tint_flames,
                // Issue #176: the wheels rolled and steered.
                systems::spin_wheels,
                crate::characters::act,
            )
                .chain(),
        )
            .chain(),
    );
    (app, counted)
}

/// The particles' systems themselves (watching the karts and the rules, throwing, moving and
/// showing), in a race of 12 karts that drift, boost, bump and throw their items: after the
/// first seconds, nothing allocated, frame after frame.
#[test]
fn the_particles_systems_allocate_nothing_per_frame() {
    let (mut app, mut counted) = counted_race();
    // The race starts, the karts spread out; every buffer finds its size.
    for _ in 0..(8 * 60) {
        app.update();
        counted.run(app.world_mut());
    }
    let mut total = 0;
    let mut worst = 0;
    let mut first = String::new();
    let mut learning = 0;
    let spawned = app.world().resource::<Particles>().pool.spawned;
    for _ in 0..(20 * 60) {
        let archetypes = app.world().archetypes().len();
        app.update();
        if app.world().archetypes().len() != archetypes {
            // A new kind of entity (a pelican, an item's glow, a kart's first shake...): Bevy's
            // queries learn it, once. Not a frame's cost.
            counted.run(app.world_mut());
            learning += 1;
            continue;
        }
        let n = allocations(|| counted.run(app.world_mut()));
        if n > 0 && first.is_empty() {
            first = traces();
        }
        total += n;
        worst = worst.max(n);
    }
    let thrown = app.world().resource::<Particles>().pool.spawned - spawned;
    println!(
        "20 s of a race of 12 karts: {thrown} particles thrown, {total} allocations \
         ({learning} frames with a new kind of entity, not counted)"
    );
    assert!(learning < 60, "{learning}");
    assert!(thrown > 2000, "{thrown}");
    assert_eq!(
        total, 0,
        "allocations in 1 200 frames (worst frame: {worst}); the first:\n{first}"
    );
}

/// Issue #176: the countdown played (12 karts revving on the grid: their tyres smoke, they
/// shake), GO (each start its cloud), then the first seconds of the race: nothing allocated by
/// the particles' systems, the wheels' included.
#[test]
fn the_standing_start_allocates_nothing() {
    let (mut app, mut counted) = counted_race_from(false);
    // The first frames: every kart gets its memory (a new kind of entity), the buffers their
    // size.
    for _ in 0..30 {
        app.update();
        counted.run(app.world_mut());
    }
    assert!(
        !app.world().resource::<Race>().started(),
        "still on the grid"
    );
    let spawned = app.world().resource::<Particles>().pool.spawned;
    let (mut total, mut learning, mut grid) = (0, 0, 0u64);
    let mut first = String::new();
    loop {
        let archetypes = app.world().archetypes().len();
        app.update();
        let race = app.world().resource::<Race>();
        let (started, ticks) = (race.started(), race.race_ticks());
        if app.world().archetypes().len() != archetypes {
            counted.run(app.world_mut());
            learning += 1;
            continue;
        }
        let n = allocations(|| counted.run(app.world_mut()));
        if n > 0 && first.is_empty() {
            first = traces();
        }
        total += n;
        if !started {
            grid = app.world().resource::<Particles>().pool.spawned - spawned;
        }
        if ticks > 3 * 60 {
            break;
        }
    }
    println!(
        "the countdown and 3 s of race: {grid} particles thrown on the grid, {total} allocations ({learning} frames with a new kind of entity)"
    );
    assert!(learning < 30, "{learning}");
    assert!(grid > 200, "the karts revving on the grid: {grid}");
    assert_eq!(total, 0, "allocations; the first:\n{first}");
}

/// Load measurement, run by hand (numbers go in STATUS.md):
/// `cargo test --lib measure_the_particles -- --ignored --nocapture` (add `--release` for the
/// game's optimizations). The CPU time of the particles' systems per frame in a race of 12
/// karts, next to the game's own frame (headless: its simulation and visuals, no rendering):
/// with the default tuning, then with every slot of the pool alive all along (the cap at its
/// most, the pool topped up before each frame: the most the particles can ever cost), then
/// (issue #174) with the 12 karts drifting in the purple tier all at once, all along (each
/// kart's state set so before each frame of the particles: the race itself is no longer
/// measured then), then (issue #176) a standing start: the countdown played (the 12 karts
/// revving on the grid), GO and the first 3 s of the race.
#[test]
#[ignore]
fn measure_the_particles_with_twelve_karts() {
    for mode in 0..4 {
        let (full, drifting, grid) = (mode == 1, mode == 2, mode == 3);
        let (mut app, mut counted) = counted_race_from(!grid);
        if full {
            app.world_mut().resource_mut::<VfxTuning>().max_particles = ParticlePool::SLOTS;
        }
        for _ in 0..(if grid { 10 } else { 5 * 60 }) {
            app.update();
            counted.run(app.world_mut());
        }
        let mut game = Vec::with_capacity(3600);
        let mut vfx = Vec::with_capacity(3600);
        let mut live = 0usize;
        for _ in 0..(40 * 60) {
            if grid && app.world().resource::<Race>().race_ticks() > 3 * 60 {
                break;
            }
            let start = std::time::Instant::now();
            app.update();
            let middle = std::time::Instant::now();
            if full {
                // Sparks around the camera's kart, until every slot is alive.
                let at = {
                    let world = app.world_mut();
                    world
                        .query_filtered::<&Transform, With<crate::camera::ChaseCamera>>()
                        .single(world)
                        .map_or(Vec3::ZERO, |t| t.translation + t.forward() * 6.0)
                };
                let mut particles = app.world_mut().resource_mut::<Particles>();
                let pool = &mut particles.pool;
                while pool.live() < pool.cap() {
                    let direction = pool.rng.direction();
                    let mut spark =
                        Particle::new(Shape::Cube, Paint::Purple, at + direction, 0.4, 0.08);
                    spark.velocity = direction * 4.0;
                    spark.stretch = 5.0;
                    pool.spawn(spark);
                }
            }
            if drifting {
                let world = app.world_mut();
                for mut state in world.query::<&mut crate::kart::KartState>().iter_mut(world) {
                    *state = crate::kart::KartState::Drifting {
                        side: crate::kart::DriftSide::Left,
                        charge: 2000.0,
                    };
                }
            }
            let topped = std::time::Instant::now();
            counted.run(app.world_mut());
            let end = std::time::Instant::now();
            game.push((middle - start).as_secs_f64() * 1000.0);
            vfx.push((end - topped).as_secs_f64() * 1000.0);
            live += app.world().resource::<Particles>().pool.live();
        }
        let stats = |samples: &mut Vec<f64>| {
            samples.sort_by(f64::total_cmp);
            let mean = samples.iter().sum::<f64>() / samples.len() as f64;
            let at = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
            format!(
                "mean {mean:.3} ms, median {:.3}, p95 {:.3}, max {:.3}",
                at(0.5),
                at(0.95),
                at(1.0)
            )
        };
        println!(
            "{}: {:.0} particles alive on average; particles' systems {}; the game's frame {}",
            if full {
                "every slot alive (1 536)"
            } else if grid {
                "a standing start (the countdown, GO, 3 s)"
            } else if drifting {
                "12 karts drifting purple"
            } else {
                "default tuning"
            },
            live as f64 / game.len() as f64,
            stats(&mut vfx),
            stats(&mut game)
        );
    }
}
