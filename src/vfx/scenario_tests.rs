//! Issue #105: the particles, headless: the whole game (`GamePlugin`) with `MinimalPlugins`, one
//! fixed tick per update, [`VfxPlugin`] added by hand (the game adds it only with a renderer).
//! The drift's sparks follow its tier, the flames and their embers the boost, the shocks and the
//! items throw theirs, the pool never exceeds its cap with 12 karts.

use bevy::prelude::*;

use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::{Bot, BotDifficulty, BotTuning, bot, bot_seed};
use crate::croquettes::CroquettePickedUp;
use crate::items::{Blast, ItemHit, ItemKind};
use crate::kart::{Boost, BoostFlame, KartBump, KartState, KartTuning, MiniTurbo, PlayerKart};
use crate::race::{Race, RaceGrid, Racer};
use crate::tests::{game_app_unloaded, hold_keys, player, wait_for_the_player};
use crate::track::TrackChoice;

const DT: f32 = 1.0 / 60.0;

/// The game on `track` with `bots` bots and the particles, the player's kart spawned.
fn vfx_app(track: TrackChoice, bots: usize) -> App {
    let mut app = game_app_unloaded(track, bots);
    app.add_plugins(VfxPlugin);
    wait_for_the_player(&mut app);
    app.update();
    app
}

/// Prairie Pompon with `bots` bots and the particles, at GO (the countdown skipped).
fn started(bots: usize) -> (App, Entity) {
    let mut app = vfx_app(TrackChoice::default(), bots);
    for _ in 0..600 {
        let world = app.world_mut();
        let ready = world.get_resource::<Race>().is_some()
            && world.query::<&Bot>().iter(world).count() == bots;
        if ready {
            break;
        }
        app.update();
    }
    app.update();
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    let kart = player(&mut app);
    (app, kart)
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

fn pool(app: &App) -> &ParticlePool {
    &app.world().resource::<Particles>().pool
}

/// The particles thrown on the last frame (they have moved on once since).
fn new_particles(app: &App) -> Vec<Particle> {
    pool(app)
        .alive()
        .filter(|p| p.age <= DT * 1.01)
        .copied()
        .collect()
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

/// Issue #105: nothing of the particles in the headless game (the bot races' world): no tuning,
/// no pool, no memory on the karts; with the plugin, its tuning is in the admin panel, its slots
/// made once.
#[test]
fn nothing_of_the_particles_in_the_headless_game() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    wait_for_the_player(&mut app);
    run(&mut app, 30);
    assert!(app.world().get_resource::<VfxTuning>().is_none());
    assert!(app.world().get_resource::<Particles>().is_none());
    assert_eq!(count::<With<VfxParticle>>(&mut app), 0);
    assert_eq!(count::<With<KartVfx>>(&mut app), 0);
    let names = |app: &App| -> Vec<&'static str> {
        app.world()
            .resource::<crate::admin::Tunings>()
            .list
            .iter()
            .map(|t| t.name)
            .collect()
    };
    assert!(!names(&app).contains(&"VfxTuning"));
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    assert!(names(&app).contains(&"VfxTuning"));
    assert_eq!(count::<With<KartVfx>>(&mut app), 1);
    assert_eq!(count::<With<VfxParticle>>(&mut app), ParticlePool::SLOTS);
    assert_eq!(
        pool(&app).cap(),
        app.world().resource::<VfxTuning>().max_particles
    );
}

/// What a frame of the drift threw.
#[derive(Default, Debug)]
struct TierLog {
    frames: usize,
    sparks: usize,
    size: f32,
    rings: usize,
    other_tiers: usize,
}

/// Issue #105 acceptance (GD § 3.4 and 14), on the keyboard: a long drift on the test ground. Its
/// sparks come with the blue tier, then orange, then purple: more of them per second at each tier,
/// bigger, each tier only in its color, a flash (a ring of its color) when it is reached; the
/// tyres smoke all along. The release fires the « fwoosh » of the purple tier (its ring), its
/// trail, and the flames turn purple.
#[test]
fn the_drift_sparks_grow_with_their_tier_and_take_its_color() {
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 90);
    hold_keys(
        &mut app,
        &[KeyCode::ArrowUp, KeyCode::Space, KeyCode::ArrowLeft],
    );
    let tuning = KartTuning::default();
    let mut logs: [TierLog; 3] = Default::default();
    let mut smoke = 0;
    let mut before_blue = 0;
    let mut last_tier = None;
    for _ in 0..(4.2 * 60.0) as usize {
        app.update();
        let state = *app.world().get::<KartState>(kart).unwrap();
        let tier = state.mini_turbo(&tuning, false);
        let thrown = new_particles(&app);
        smoke += thrown.iter().filter(|p| p.paint == Paint::Smoke).count();
        let sparks = thrown.iter().filter(|p| {
            p.shape == Shape::Cube && matches!(p.paint, Paint::Blue | Paint::Orange | Paint::Purple)
        });
        match tier {
            None => before_blue += sparks.count(),
            Some(tier) => {
                let paint = Paint::tier(tier.index());
                let log = &mut logs[tier.index()];
                for p in thrown.iter().filter(|p| p.shape == Shape::Ring) {
                    assert_eq!(p.paint, paint, "the flash of {tier:?}");
                    log.rings += 1;
                }
                // The frame of the flash has its burst: the rate is measured on the others.
                if last_tier == Some(tier) {
                    log.frames += 1;
                    for spark in sparks {
                        if spark.paint == paint {
                            log.sparks += 1;
                            log.size += spark.size;
                        } else {
                            log.other_tiers += 1;
                        }
                    }
                }
            }
        }
        last_tier = tier;
        if tier == Some(MiniTurbo::Purple) && logs[2].frames >= 40 {
            break;
        }
    }
    assert!(
        matches!(
            app.world().get::<KartState>(kart).unwrap(),
            KartState::Drifting { .. }
        ),
        "still drifting"
    );
    assert_eq!(before_blue, 0, "no spark before the blue tier");
    assert!(smoke > 20, "the tyres smoke: {smoke}");
    let rate = |log: &TierLog| log.sparks as f32 / (log.frames as f32 * DT);
    let size = |log: &TierLog| log.size / log.sparks.max(1) as f32;
    println!(
        "sparks per second by tier: {:.0} {:.0} {:.0}; size {:.3} {:.3} {:.3}",
        rate(&logs[0]),
        rate(&logs[1]),
        rate(&logs[2]),
        size(&logs[0]),
        size(&logs[1]),
        size(&logs[2]),
    );
    for (i, log) in logs.iter().enumerate() {
        assert!(log.frames >= 30, "tier {i}: {log:?}");
        assert_eq!(log.rings, 1, "tier {i}: one flash");
        assert_eq!(log.other_tiers, 0, "tier {i}: only its color");
    }
    let drift = &app.world().resource::<VfxTuning>().drift.clone();
    for (i, log) in logs.iter().enumerate() {
        // Two wheels, between the rate at the start of the tier and at its end.
        let lowest = 2.0 * drift.spark_rate[i] * (1.0 - drift.spark_ramp) * 0.9;
        let highest = 2.0 * drift.spark_rate[i] * 1.05;
        assert!(
            (lowest..=highest).contains(&rate(log)),
            "tier {i}: {} per second",
            rate(log)
        );
    }
    assert!(rate(&logs[0]) < rate(&logs[1]) && rate(&logs[1]) < rate(&logs[2]));
    assert!(size(&logs[0]) < size(&logs[1]) && size(&logs[1]) < size(&logs[2]));

    // The release: the purple « fwoosh » (its ring and burst), its trail, the flames in purple.
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    app.update();
    let thrown = new_particles(&app);
    assert_eq!(
        thrown
            .iter()
            .filter(|p| p.shape == Shape::Ring && p.paint == Paint::Purple)
            .count(),
        1,
        "the fwoosh's ring"
    );
    assert!(
        thrown
            .iter()
            .filter(|p| p.shape == Shape::Cube && p.paint == Paint::Purple)
            .count() as u32
            >= drift.release_sparks[2]
    );
    let mut trail = 0;
    for _ in 0..30 {
        app.update();
        trail += new_particles(&app)
            .iter()
            .filter(|p| p.paint == Paint::Purple)
            .count();
    }
    assert!(trail >= 10, "the trail: {trail}");
    assert_eq!(
        app.world().get::<KartVfx>(kart).unwrap().source,
        Some(BoostSource::MiniTurbo(MiniTurbo::Purple))
    );
    let purple = app
        .world()
        .resource::<VfxAssets>()
        .material(Paint::Purple)
        .clone();
    let world = app.world_mut();
    let mut flames = world.query_filtered::<&MeshMaterial3d<StandardMaterial>, With<BoostFlame>>();
    assert!(flames.iter(world).all(|m| m.0 == purple));
}

/// Issue #105 acceptance (GD § 4 and 14): the flames' size follows the boost's strength (and its
/// time left), and so do their embers: more and bigger for a stronger boost.
#[test]
fn the_flames_and_their_embers_follow_the_boost() {
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    let mut last = (0.0f32, 0usize, 0.0f32);
    for strength in [0.12, 0.3, 0.5] {
        run(&mut app, 60);
        app.world_mut()
            .get_mut::<Boost>(kart)
            .unwrap()
            .add(strength, 2.0);
        let mut embers = 0;
        let mut size = 0.0;
        let mut flame = 0.0f32;
        for _ in 0..60 {
            app.update();
            for p in new_particles(&app) {
                if p.shape == Shape::Ball && p.paint == Paint::Flame {
                    embers += 1;
                    size += p.size;
                }
            }
            // The flames' length (they flicker: on average over the second).
            let world = app.world_mut();
            flame += world
                .query_filtered::<&Transform, With<BoostFlame>>()
                .iter(world)
                .map(|t| t.scale.z)
                .sum::<f32>()
                / 120.0;
        }
        let size = size / embers.max(1) as f32;
        println!("boost {strength}: flames {flame:.2} m, {embers} embers of {size:.3} m");
        assert!(flame > last.0, "{strength}: the flames grow");
        assert!(embers > last.1, "{strength}: more embers");
        assert!(size > last.2, "{strength}: bigger embers");
        last = (flame, embers, size);
        // The boost runs out: no more embers.
        run(&mut app, 120);
        assert!(new_particles(&app).iter().all(|p| p.paint != Paint::Flame));
    }
}

/// Issue #105 acceptance: the pool never holds more than its cap, with 12 karts racing (they
/// drift, boost, bump, throw their items), and a lower cap set in the panel holds at once.
#[test]
fn the_pool_never_exceeds_its_cap_with_twelve_karts() {
    let (mut app, kart) = started(11);
    // The player's kart is a bot too; every kart throws, wherever it is.
    let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
    let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
    let bots = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(BotDifficulty::Hard, seed, &bots));
    {
        let mut tuning = app.world_mut().resource_mut::<VfxTuning>();
        tuning.max_particles = 200;
        tuning.emit_range = 10_000.0;
    }
    let mut peak = 0;
    for frame in 0..(30 * 60) {
        if frame == 20 * 60 {
            app.world_mut().resource_mut::<VfxTuning>().max_particles = 80;
        }
        app.update();
        let cap = app.world().resource::<VfxTuning>().max_particles;
        let live = pool(&app).live();
        assert!(live <= cap, "frame {frame}: {live} > {cap}");
        let world = app.world_mut();
        let shown = world
            .query_filtered::<&Visibility, With<VfxParticle>>()
            .iter(world)
            .filter(|v| **v != Visibility::Hidden)
            .count();
        assert!(shown <= cap, "frame {frame}: {shown} shown > {cap}");
        peak = peak.max(live);
    }
    let pool = pool(&app);
    println!(
        "12 karts, 30 s: peak {peak} particles, {} thrown, {} took the slot of a live one",
        pool.spawned, pool.replaced
    );
    // More was asked for than the cap: new particles took the slots of live ones.
    assert!(peak >= 180, "the cap was reached: {peak}");
    assert!(pool.replaced > 0);
}

/// Issue #105 (GD § 14): a bump throws stars and « pof » between the two karts; an explosion its
/// confetti (which land and lie on the ground); a croquette its sparkles.
#[test]
fn bumps_blasts_and_croquettes_throw_theirs() {
    let (mut app, kart) = started(1);
    run(&mut app, 30);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    let at = |app: &App, kart: Entity| app.world().get::<Transform>(kart).unwrap().translation;
    app.world_mut().write_message(KartBump {
        kart,
        by: bot,
        closing_speed: 15.0,
    });
    app.update();
    let thrown = new_particles(&app);
    let stars: Vec<&Particle> = thrown.iter().filter(|p| p.shape == Shape::Star).collect();
    assert_eq!(stars.len(), 6, "15 m/s: 6 stars");
    let middle = (at(&app, kart) + at(&app, bot)) * 0.5;
    assert!(stars.iter().all(|p| p.position.distance(middle) < 3.0));
    let impact = app.world().resource::<VfxTuning>().impact.clone();
    assert_eq!(
        thrown.iter().filter(|p| p.paint == Paint::Smoke).count() as u32,
        impact.pof_puffs
    );
    // An explosion.
    let ground = at(&app, kart) + Vec3::new(0.0, -0.3, 8.0);
    app.world_mut().write_message(Blast {
        at: ground,
        radius: 4.0,
        item: ItemKind::Bombe,
        owner: bot,
        object: None,
        announce: false,
    });
    app.update();
    let confetti = new_particles(&app)
        .iter()
        .filter(|p| Paint::CONFETTI.contains(&p.paint))
        .count() as u32;
    assert_eq!(confetti, impact.confetti);
    run(&mut app, 90);
    let landed: Vec<Particle> = pool(&app)
        .alive()
        .filter(|p| Paint::CONFETTI.contains(&p.paint))
        .copied()
        .collect();
    assert!(!landed.is_empty());
    assert!(landed.iter().all(|p| p.position.y >= ground.y - 0.46));
    // A croquette.
    let here = at(&app, kart);
    app.world_mut().write_message(CroquettePickedUp {
        kart,
        at: here,
        count: 1,
    });
    app.update();
    let world = app.world().resource::<VfxTuning>().world.clone();
    assert_eq!(
        new_particles(&app)
            .iter()
            .filter(|p| p.shape == Shape::Star && p.paint == Paint::Gold)
            .count() as u32,
        world.croquette_sparkles
    );
}

/// Issue #105 (GD § 8 and 14): hit by an item, the kart sees stars turning over its head while it
/// spins out or tumbles, and a while after (« sonné »); the player's camera shakes.
#[test]
fn a_hit_kart_sees_stars_over_its_head() {
    let (mut app, kart) = started(1);
    run(&mut app, 30);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    assert_eq!(count::<With<DizzyStar>>(&mut app), 0, "made when needed");
    app.world_mut().write_message(ItemHit {
        kart,
        by: bot,
        item: ItemKind::Pelote,
        object: None,
    });
    run(&mut app, 3);
    assert!(app.world().get::<KartState>(kart).unwrap().is_hit());
    assert!(
        app.world()
            .get::<crate::camera::CameraShake>(kart)
            .is_some(),
        "the player's camera shakes"
    );
    let shown = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&DizzyStar, &Visibility, &Transform)>()
            .iter(world)
            .filter(|(_, v, _)| **v != Visibility::Hidden)
            .map(|(_, _, t)| *t)
            .collect::<Vec<_>>()
    };
    let tuning = app.world().resource::<VfxTuning>().impact.clone();
    let stars = shown(&mut app);
    assert_eq!(stars.len() as u32, tuning.dizzy_stars);
    assert!(stars.iter().all(|t| t.translation.y > 1.0), "over the head");
    // They turn.
    let before = stars[0].translation;
    run(&mut app, 5);
    assert!(shown(&mut app)[0].translation.distance(before) > 0.05);
    // Still there just after the hit, gone a while later.
    let mut ticks = 0;
    while app.world().get::<KartState>(kart).unwrap().is_hit() {
        app.update();
        ticks += 1;
        assert!(ticks < 600);
    }
    run(&mut app, 20);
    assert_eq!(
        shown(&mut app).len() as u32,
        tuning.dizzy_stars,
        "« sonné »"
    );
    run(&mut app, (tuning.dizzy_after_hit * 60.0) as usize);
    assert!(shown(&mut app).is_empty());
}

/// Issue #105: the camera's field of view widens while its kart boosts, with a punch when the
/// boost starts, and is back to the chase's when it is over; speed lines rush past meanwhile.
#[test]
fn a_boost_widens_the_view_and_draws_speed_lines() {
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 120);
    let fov = |app: &mut App| {
        let world = app.world_mut();
        let projection = world
            .query_filtered::<&Projection, With<crate::camera::ChaseCamera>>()
            .single(world)
            .unwrap()
            .clone();
        match projection {
            Projection::Perspective(p) => p.fov.to_degrees(),
            _ => unreachable!(),
        }
    };
    let cruising = fov(&mut app);
    app.world_mut()
        .get_mut::<Boost>(kart)
        .unwrap()
        .add(0.3, 1.5);
    let mut widest = 0.0f32;
    let mut lines = 0;
    for _ in 0..60 {
        app.update();
        widest = widest.max(fov(&mut app));
        lines += new_particles(&app)
            .iter()
            .filter(|p| p.paint == Paint::Wind && p.stretch > 20.0)
            .count();
    }
    let boost = app.world().resource::<VfxTuning>().boost.clone();
    println!("cruising {cruising:.1}°, boosting up to {widest:.1}°, {lines} speed lines");
    assert!(
        widest > cruising + boost.fov_extra,
        "{widest} vs {cruising}"
    );
    assert!(lines > 30, "{lines}");
    run(&mut app, 240);
    let kick = *app.world().resource::<FovKick>();
    assert!(
        kick.extra < 0.05 && fov(&mut app) < widest - boost.fov_extra,
        "{kick:?}"
    );
}
