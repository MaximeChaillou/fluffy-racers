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
use crate::kart::{
    Boost, BoostFlame, KartBump, KartInput, KartState, KartTuning, MiniTurbo, PlayerKart,
};
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
    /// Issue #174: its flames licking the wheels, its embers, the frames with its halo on the
    /// ground (of its color), the frames whose glows were drawn in the shared mesh, the frame
    /// the kart jumped.
    flames: usize,
    embers: usize,
    halos: usize,
    drawn: usize,
    hop: f32,
}

/// Issue #105 acceptance (GD § 3.4 and 14), on the keyboard: a long drift on the test ground. Its
/// sparks come with the blue tier, then orange, then purple: more of them per second at each tier,
/// bigger, each tier only in its color, a flash (a ring of its color) when it is reached; the
/// tyres smoke all along. The release fires the « fwoosh » of the purple tier (its ring), its
/// trail, and the flames turn purple.
///
/// Issue #174 (three tiers really apart): the sparks are glowing streaks (drawn in the shared
/// mesh of the glows); blue has no flame, orange its flames, purple more of them and its embers;
/// each tier its halo of its color on the ground; the kart jumps a little at each tier; the
/// release blasts flames of its color out of the exhausts, the halo stays while the boost lasts.
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
            p.shape == Shape::Glint
                && matches!(p.paint, Paint::Blue | Paint::Orange | Paint::Purple)
        });
        match tier {
            None => before_blue += sparks.count(),
            Some(tier) => {
                let paint = Paint::tier(tier.index());
                let log = &mut logs[tier.index()];
                for p in thrown.iter().filter(|p| p.shape == Shape::Shock) {
                    assert_eq!(p.paint, paint, "the flash of {tier:?}");
                    log.rings += 1;
                }
                // Issue #174: its flames, its embers, its halo, the glows drawn, the jump.
                for p in &thrown {
                    if p.paint == paint && p.shape == Shape::Tongue {
                        log.flames += 1;
                    }
                    if p.paint == paint && p.shape == Shape::Glow && p.cool == 1.0 {
                        log.embers += 1;
                    }
                    assert!(
                        p.shape != Shape::Tongue || p.paint == paint,
                        "{tier:?}: only flames of its color"
                    );
                }
                let particles = app.world().resource::<Particles>();
                let color = batch_color(&app, paint);
                if particles.halos.len() == 1
                    && (particles.halos[0].color.normalize() - color).length() < 1e-3
                {
                    log.halos += 1;
                }
                // Every glowing particle alive, and the halo, is a quad of the shared mesh.
                let glowing = particles
                    .pool
                    .alive()
                    .filter(|p| p.shape.batched() == Some(Batch::Glow))
                    .count();
                if glowing > 0 && particles.glow.len() == glowing + particles.halos.len() {
                    log.drawn += 1;
                }
                log.hop = log.hop.max(hop(&mut app, kart));
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
    println!("issue #174, by tier: {logs:?}");
    let drift_vfx = app.world().resource::<VfxTuning>().drift.clone();
    assert_eq!(logs[0].flames, 0, "blue: sparks only");
    assert!(logs[1].flames > 20 && logs[2].flames > logs[1].flames);
    assert_eq!(logs[0].embers + logs[1].embers, 0);
    assert!(logs[2].embers > 5, "purple: its embers");
    for (i, log) in logs.iter().enumerate() {
        assert!(log.halos + 2 >= log.frames, "tier {i}: its halo: {log:?}");
        assert!(log.drawn + 2 >= log.frames, "tier {i}: drawn: {log:?}");
        assert!(
            log.hop > drift_vfx.hop_height[i] * 0.5 && log.hop <= drift_vfx.hop_height[i] + 1e-3,
            "tier {i}: its jump: {log:?}"
        );
    }
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
            .filter(|p| p.shape == Shape::Shock && p.paint == Paint::Purple)
            .count(),
        1,
        "the fwoosh's ring"
    );
    assert!(
        thrown
            .iter()
            .filter(|p| p.shape == Shape::Glint && p.paint == Paint::Purple)
            .count() as u32
            >= drift.release_sparks[2]
    );
    // Issue #174: the blast of purple flames out of the exhausts.
    assert_eq!(
        thrown
            .iter()
            .filter(|p| p.shape == Shape::Tongue && p.paint == Paint::Purple)
            .count() as u32,
        drift.release_flames[2]
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
    // Issue #174: the halo stays while the mini-turbo lasts.
    let halos = &app.world().resource::<Particles>().halos;
    assert_eq!(halos.len(), 1, "the boost's halo");
    let full = batch_color(&app, Paint::Purple);
    assert!((halos[0].color.normalize() - full).length() < 1e-3);
    assert_eq!(
        app.world().get::<KartVfx>(kart).unwrap().source,
        Some(BoostSource::MiniTurbo(MiniTurbo::Purple))
    );
    // Issue #174: the glowing flames' material of purple, on their mesh.
    let assets = app.world().resource::<VfxAssets>().clone();
    let purple = assets.flame(Paint::Purple).clone();
    let world = app.world_mut();
    let mut meshes = world.query_filtered::<&Mesh3d, With<BoostFlame>>();
    assert!(meshes.iter(world).all(|m| m.0 == assets.flame_mesh));
    let world = app.world_mut();
    let mut flames = world.query_filtered::<&MeshMaterial3d<StandardMaterial>, With<BoostFlame>>();
    assert!(flames.iter(world).all(|m| m.0 == purple));
}

/// Issue #174: the direction of `paint`'s linear color, as the quads take it.
fn batch_color(app: &App, paint: Paint) -> Vec3 {
    app.world().resource::<VfxAssets>().quads.paint[paint.index()].normalize()
}

/// Issue #174: how far the kart's visual is above where the interpolation put it (its jump).
fn hop(app: &mut App, kart: Entity) -> f32 {
    use crate::interpolation::{InterpolatedFrom, PhysicsPose};
    let alpha = app.world().resource::<Time<Fixed>>().overstep_fraction();
    let pose = app
        .world()
        .get::<PhysicsPose>(kart)
        .unwrap()
        .interpolate(alpha);
    let world = app.world_mut();
    let mut roots = world.query::<(&InterpolatedFrom, &Transform)>();
    roots
        .iter(world)
        .filter(|(from, _)| from.0 == kart)
        .map(|(_, t)| (t.translation - pose.translation).dot(pose.up().as_vec3()))
        .fold(0.0, f32::max)
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
                if p.shape == Shape::Glow && p.paint == Paint::Flame {
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
    // Issue #171: a ring around the driver's head (in its frame: their height around its head,
    // all as far from it), under its driver.
    assert!(
        stars
            .iter()
            .all(|t| (t.translation.y - tuning.dizzy_height).abs() < 0.3),
        "around the head: {stars:?}"
    );
    assert!(stars.iter().all(|t| {
        let flat = Vec2::new(t.translation.x, t.translation.z).length();
        (flat - tuning.dizzy_radius).abs() < 0.1
    }));
    {
        let world = app.world_mut();
        let parents: Vec<Entity> = world
            .query_filtered::<&ChildOf, With<DizzyStar>>()
            .iter(world)
            .map(|c| c.parent())
            .collect();
        assert!(
            parents
                .iter()
                .all(|p| world.get::<crate::kart::DriverVisual>(*p).is_some()),
            "under the driver"
        );
    }
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

/// Issue #106: the player drives into a gift box: it bursts (it swells, its lid pops off, then it
/// is gone) and its confetti fly out, as many as the tuning says for each box taken; it pops back
/// in later, its lid back on.
#[test]
fn a_gift_box_taken_bursts_into_confetti() {
    use crate::items::{ItemBox, ItemBoxLook, ItemStats};
    let (mut app, kart) = started(0);
    let impact = app.world().resource::<VfxTuning>().impact.clone();
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let mut taken = 0;
    for _ in 0..600 {
        app.update();
        taken = app.world().get::<ItemStats>(kart).unwrap().boxes;
        if taken > 0 {
            break;
        }
    }
    assert!(taken > 0, "no box on the way");
    let confetti = new_particles(&app)
        .iter()
        .filter(|p| Paint::CONFETTI.contains(&p.paint))
        .count() as u32;
    assert_eq!(confetti, impact.gift_confetti * taken);
    // The boxes taken: their look swells while it bursts, its lid rises, then it hides.
    let looks = |app: &mut App| {
        let world = app.world_mut();
        let mut looks: Vec<(Entity, Vec3, Visibility, f32)> = Vec::new();
        let mut query =
            world.query::<(&ChildOf, &ItemBoxLook, &Transform, &Visibility, &Children)>();
        let rows: Vec<_> = query
            .iter(world)
            .map(|(parent, look, transform, visibility, children)| {
                (
                    parent.parent(),
                    look.gone_for,
                    transform.scale,
                    *visibility,
                    children[0],
                )
            })
            .collect();
        for (item_box, gone_for, scale, visibility, lid) in rows {
            if world
                .get::<ItemBox>(item_box)
                .is_some_and(|b| !b.available())
            {
                let lid_y = world.get::<Transform>(lid).unwrap().translation.y;
                looks.push((item_box, scale, visibility, lid_y));
                assert!(gone_for.is_finite() || visibility == Visibility::Hidden);
            }
        }
        looks
    };
    app.update();
    let bursting = looks(&mut app);
    assert_eq!(bursting.len() as u32, taken);
    for (_, scale, visibility, _) in &bursting {
        assert_eq!(*visibility, Visibility::Inherited, "seen while it bursts");
        assert!(scale.x > 1.0, "it swells: {scale}");
    }
    run(&mut app, 4);
    let later = looks(&mut app);
    for ((_, scale, _, lid), (_, before, _, lid_before)) in later.iter().zip(&bursting) {
        assert!(scale.x > before.x && lid > lid_before, "{scale} {lid}");
    }
    run(&mut app, 20);
    for (_, _, visibility, _) in looks(&mut app) {
        assert_eq!(visibility, Visibility::Hidden, "gone after its burst");
    }
}

/// Puts `kart` at centerline distance `distance` of `layout`, `lateral` m right of it, heading
/// down the track at `speed`, its race progress there (as `kart::environment_tests` does).
fn place(app: &mut App, kart: Entity, distance: f32, lateral: f32, speed: f32) {
    use crate::interpolation::PhysicsPose;
    use crate::race::{Course, RaceProgress};
    use bevy_rapier3d::prelude::Velocity;
    let layout = app.world().resource::<crate::track::TrackLayout>().clone();
    let spacing = app.world().resource::<Course>().checkpoints[1];
    let sample = layout.center.at(distance);
    let transform = sample.transform(lateral, crate::track::GRID_HEIGHT);
    let world = app.world_mut();
    *world.get_mut::<Transform>(kart).unwrap() = transform;
    *world.get_mut::<Velocity>(kart).unwrap() = Velocity {
        linear: sample.forward * speed,
        angular: Vec3::ZERO,
    };
    world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    *world.get_mut::<KartState>(kart).unwrap() = KartState::Grounded;
    let mut progress = world.get_mut::<RaceProgress>(kart).unwrap();
    progress.passed = 1 + (distance / spacing).floor() as u32;
    progress.distance = distance;
}

/// Issue #130: on a circuit, the dust off the road takes the colors of its biome's ground, a
/// little lighter (`tuning::ground_dust`): the shoulders' beside the road, the sand's on a sand
/// patch; a landing on the road keeps the road's dust. Off a circuit (the test ground), the
/// dust's color of the tuning.
#[test]
fn the_dust_off_the_road_is_the_color_of_the_biomes_ground() {
    use crate::track::biome::biome;
    let (mut app, kart) = started(0);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    let tuning = app.world().resource::<VfxTuning>().clone();
    let palette = biome("Prairie").unwrap().palette;
    let [shoulder, sand] = ground_dust(&palette, tuning.world.ground_lighten);
    let assets = app.world().resource::<VfxAssets>().clone();
    assert_eq!(assets.ground, Some([shoulder, sand]));
    let color = |app: &App, paint: Paint| {
        let assets = app.world().resource::<VfxAssets>();
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let color = materials
            .get(assets.material(paint))
            .unwrap()
            .base_color
            .to_srgba();
        [color.red, color.green, color.blue]
    };
    for (paint, expected) in [(Paint::Ground, shoulder), (Paint::Sand, sand)] {
        let shown = color(&app, paint);
        for (a, b) in shown.iter().zip(expected) {
            assert!(
                (a - b).abs() < 1e-4,
                "{paint:?}: {shown:?} for {expected:?}"
            );
        }
    }
    // Lighter than the ground, the same hue: the meadow's grass is green dust.
    assert!(shoulder[1] > shoulder[0] && shoulder[1] > palette.shoulder.1);
    let layout = app.world().resource::<crate::track::TrackLayout>().clone();
    // On the shoulder of the start straight, then on the sand of the hairpin's apex.
    let straight = layout.center.length() - 40.0;
    let sample = layout.center.at(straight);
    let on_shoulder = sample.half_width + crate::track::mesh::CURB_WIDTH + sample.shoulder / 2.0;
    let patch = layout.offroad[0];
    let middle = layout
        .center
        .param_to_distance((patch.from + patch.to) / 2.0);
    let half = layout.center.at(middle).half_width;
    let on_sand = half * (patch.left + patch.right) / 2.0;
    for (distance, lateral, paint) in [
        (straight, on_shoulder, Paint::Ground),
        (middle, on_sand, Paint::Sand),
    ] {
        place(&mut app, kart, distance, lateral, 14.0);
        let mut thrown = (0, 0);
        for _ in 0..40 {
            *app.world_mut().get_mut::<KartInput>(kart).unwrap() = KartInput {
                throttle: 1.0,
                ..default()
            };
            app.update();
            for p in new_particles(&app) {
                if p.paint == paint {
                    thrown.0 += 1;
                } else if matches!(p.paint, Paint::Ground | Paint::Sand | Paint::Dust) {
                    thrown.1 += 1;
                }
            }
        }
        assert!(thrown.0 > 5, "{paint:?}: {thrown:?}");
        assert_eq!(thrown.1, 0, "{paint:?}: {thrown:?}");
    }
    // The test ground: the tuning's dust.
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    run(&mut app, 3);
    assert_eq!(app.world().resource::<VfxAssets>().ground, None);
    let shown = color(&app, Paint::Ground);
    for (a, b) in shown.iter().zip(tuning.colors.dust) {
        assert!((a - b).abs() < 1e-4);
    }
}

/// Issue #130 (GD § 10): on the Lagon Duvet, a kart that dives into the lagoon and comes out of
/// it splashes twice (a crown of drops, a ring and puffs of foam on the water), and leaves a
/// wake of foam on the water while it is at the surface; under it, deep, none.
#[test]
fn the_lagoon_splashes_and_leaves_a_wake() {
    use crate::kart::KartEnvironment;
    use bevy_rapier3d::prelude::Velocity;
    let mut app = vfx_app(TrackChoice::Circuit("tracks/lagon.track.ron".into()), 0);
    for _ in 0..600 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    assert!(skip_countdown_now(app.world_mut()));
    run(&mut app, 3);
    let kart = player(&mut app);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    let world = &app.world().resource::<VfxTuning>().world.clone();
    let layout = app.world().resource::<crate::track::TrackLayout>().clone();
    let lake = layout.lakes[0];
    let margin = app
        .world()
        .resource::<KartTuning>()
        .environment
        .water_margin;
    let entry = layout
        .center
        .samples()
        .iter()
        .find(|s| {
            let at = s.position + s.up * crate::track::GRID_HEIGHT;
            lake.depth(at).is_some_and(|d| d > margin)
        })
        .unwrap()
        .distance;
    place(&mut app, kart, entry - 50.0, 0.0, 16.0);
    let mut splashes = Vec::new();
    let mut wake = 0;
    let mut deep_wake = 0;
    let mut was_under = false;
    let mut flips = 0;
    for _ in 0..(14 * 60) {
        let transform = *app.world().get::<Transform>(kart).unwrap();
        let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
        let mut input = crate::tests::follow_centerline(&layout, &transform, speed);
        if speed > 18.0 {
            input.throttle = 0.0;
        }
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
        app.update();
        let under = app.world().get::<KartEnvironment>(kart).unwrap().underwater;
        if under != was_under {
            flips += 1;
            was_under = under;
        }
        let thrown = new_particles(&app);
        let drops = thrown.iter().filter(|p| p.paint == Paint::Water).count();
        let rings = thrown
            .iter()
            .filter(|p| p.paint == Paint::Foam && p.shape == Shape::Ring)
            .count();
        if rings > 0 {
            splashes.push((drops, under));
        }
        let depth = lake.depth(transform.translation);
        for p in thrown
            .iter()
            .filter(|p| p.paint == Paint::Foam && p.shape == Shape::Ball)
        {
            assert!(
                (p.position.y - (lake.level + 0.03)).abs() < 0.05,
                "foam on the water: {}",
                p.position
            );
            wake += 1;
            if depth.is_some_and(|d| d > world.wake_below + 0.3) {
                deep_wake += 1;
            }
        }
    }
    assert!(flips >= 2, "in and out of the lagoon: {flips}");
    assert_eq!(splashes.len(), 2, "{splashes:?}");
    let (in_drops, in_under) = splashes[0];
    let (out_drops, out_under) = splashes[1];
    assert!(in_under && !out_under, "{splashes:?}");
    assert!(in_drops as u32 >= world.splash_drops, "{splashes:?}");
    assert!(out_drops > 0, "{splashes:?}");
    assert!(wake > 30, "a wake: {wake}");
    assert_eq!(deep_wake, 0, "no wake from deep under the water");
}

/// The stars shown now: each one's kart.
fn stars_shown(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    world
        .query::<(&DizzyStar, &Visibility)>()
        .iter(world)
        .filter(|(_, v)| **v != Visibility::Hidden)
        .map(|(star, _)| star.body)
        .collect()
}

/// Issue #171 (« les étoiles restent trop longtemps, voire ne disparaissent pas »): two karts
/// rubbing side by side bump at every tick, hard (14 m/s): only the moment of the shock counts,
/// the stars are gone 0.9 s after it although they still rub; a new shock after they parted (0.5
/// s later) brings them back for 0.9 s.
#[test]
fn rubbing_against_a_kart_does_not_keep_the_stars() {
    let (mut app, kart) = started(1);
    run(&mut app, 30);
    let bot = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Bot>>()
            .single(world)
            .unwrap()
    };
    let impact = app.world().resource::<VfxTuning>().impact.clone();
    let bump = |app: &mut App| {
        app.world_mut().write_message(KartBump {
            kart,
            by: bot,
            closing_speed: 14.0,
        });
    };
    let mut last_seen = None;
    for tick in 0..180 {
        bump(&mut app);
        app.update();
        if stars_shown(&mut app).contains(&kart) {
            last_seen = Some(tick);
        }
    }
    let last = last_seen.expect("the first shock shows them") as f32 * DT;
    assert!(
        last <= impact.dizzy_after_shock + 2.0 * DT && last >= impact.dizzy_after_shock - 0.1,
        "{last} s"
    );
    run(&mut app, 30);
    assert!(stars_shown(&mut app).is_empty());
    bump(&mut app);
    app.update();
    assert!(stars_shown(&mut app).contains(&kart), "a new shock");
    run(&mut app, (impact.dizzy_after_shock * 60.0) as usize + 2);
    assert!(stars_shown(&mut app).is_empty());
}

/// Issue #171: another visual that follows the kart (here an item's flourish, as when it uses an
/// item while it sees stars; the pelican, the looks of the super câlin or the fusée are made the
/// same way) leaves no ring behind: before #171, each of them made a new ring at every frame,
/// and the old ones stood still, shown for good. One ring, gone with its time.
#[test]
fn another_visual_of_the_kart_leaves_no_stars_behind() {
    let (mut app, kart) = started(1);
    run(&mut app, 30);
    let impact = app.world().resource::<VfxTuning>().impact.clone();
    app.world_mut().write_message(ItemHit {
        kart,
        by: kart,
        item: ItemKind::Abeille,
        object: None,
    });
    run(&mut app, 3);
    app.world_mut().spawn((
        crate::interpolation::InterpolatedFrom(kart),
        Transform::default(),
        Visibility::default(),
    ));
    let mut ticks = 0;
    while app.world().get::<KartState>(kart).unwrap().is_hit() {
        app.update();
        ticks += 1;
        assert!(ticks < 600);
    }
    assert_eq!(
        count::<With<DizzyStar>>(&mut app) as u32,
        impact.dizzy_stars,
        "one ring"
    );
    run(&mut app, (impact.dizzy_after_hit * 60.0) as usize + 2);
    assert!(stars_shown(&mut app).is_empty());
    assert_eq!(
        count::<With<DizzyStar>>(&mut app) as u32,
        impact.dizzy_stars
    );
}

/// What a kart went through, tick by tick (issue #171): the last tick it was spinning out or
/// tumbling, the last new shock (a hard bump after `dizzy_contact_gap` without any, or a paw
/// strike), and since when it sees stars.
#[derive(Clone, Copy, Debug, Default)]
struct Dazed {
    hit: Option<u32>,
    shock: Option<u32>,
    last_bump: Option<u32>,
    struck: u32,
    shown_since: Option<u32>,
}

/// The bumps of each tick, for the test (`KartBump` is read by the particles too).
#[derive(Resource, Default)]
struct TickBumps(Vec<(Entity, f32)>);

fn note_bumps(mut bumps: MessageReader<KartBump>, mut noted: ResMut<TickBumps>) {
    noted.0.clear();
    for bump in bumps.read() {
        noted.0.push((bump.kart, bump.closing_speed));
        noted.0.push((bump.by, bump.closing_speed));
    }
}

/// Issue #171 acceptance (« aucun kart ne garde ses étoiles plus longtemps que la règle, même en
/// frottant contre un autre »): the game's grid (12 karts, all bots, with items and the paws) on
/// Prairie Pompon, 50 s from GO. At every frame, a kart that sees stars is spinning out or
/// tumbling, or did less than 1.2 s ago, or had a new shock less than 0.9 s ago (a hard bump,
/// 12 m/s, after 0.3 s without any contact, or a paw strike); no ring is ever made twice for a
/// kart. Its longest stars are printed.
#[test]
fn no_kart_keeps_its_stars_longer_than_the_rule() {
    let (mut app, player_kart) = started(11);
    app.init_resource::<TickBumps>()
        .add_systems(FixedPostUpdate, note_bumps.before(VfxSystems::Watch));
    let slot = app.world().get::<Racer>(player_kart).unwrap().grid_slot;
    let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
    let bots = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(player_kart)
        .remove::<PlayerKart>()
        .insert(bot(BotDifficulty::Normal, seed, &bots));
    let impact = app.world().resource::<VfxTuning>().impact.clone();
    let karts: Vec<Entity> = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Racer>>()
            .iter(world)
            .collect()
    };
    assert_eq!(karts.len(), 12);
    let mut dazed = vec![Dazed::default(); karts.len()];
    let gap = (impact.dizzy_contact_gap * 60.0).round() as u32;
    let slack = 2;
    let (mut longest, mut shown_frames) = (0u32, 0u32);
    for tick in 0..(50 * 60) {
        app.update();
        let bumps = app.world().resource::<TickBumps>().0.clone();
        let shown = stars_shown(&mut app);
        for (i, kart) in karts.iter().enumerate() {
            let d = &mut dazed[i];
            let world = app.world();
            if world.get::<KartState>(*kart).unwrap().is_hit() {
                d.hit = Some(tick);
            }
            let struck = world
                .get::<crate::interactions::Paw>(*kart)
                .map_or(0, |p| p.struck_count);
            if struck > d.struck {
                d.shock = Some(tick);
            }
            d.struck = struck;
            let bump = bumps
                .iter()
                .filter(|(k, _)| k == kart)
                .map(|(_, speed)| *speed)
                .fold(0.0, f32::max);
            if bump > 0.0 {
                let fresh = d.last_bump.is_none_or(|last| tick - last > gap);
                if bump >= impact.dizzy_bump_speed && fresh {
                    d.shock = Some(tick);
                }
                d.last_bump = Some(tick);
            }
            if shown.contains(kart) {
                let since = d.shown_since.get_or_insert(tick);
                longest = longest.max(tick - *since);
                shown_frames += 1;
                let after_hit = d.hit.is_some_and(|hit| {
                    tick - hit <= (impact.dizzy_after_hit * 60.0) as u32 + slack
                });
                let after_shock = d.shock.is_some_and(|shock| {
                    tick - shock <= (impact.dizzy_after_shock * 60.0) as u32 + slack
                });
                assert!(
                    after_hit || after_shock,
                    "tick {tick}: kart {i} sees stars ({d:?})"
                );
            } else {
                d.shown_since = None;
            }
        }
    }
    let rings = count::<With<DizzyStar>>(&mut app) as u32;
    println!(
        "12 karts, 50 s: {shown_frames} kart-frames with stars, the longest {:.2} s, {} rings",
        longest as f32 / 60.0,
        rings / impact.dizzy_stars.max(1)
    );
    assert!(shown_frames > 0, "someone saw stars");
    assert!(rings <= 12 * impact.dizzy_stars, "{rings} stars made");
}

/// Issue #176: the visual of `kart`, its pivot and its wheels (by index).
fn wheels_of(app: &mut App, kart: Entity) -> (Entity, [Entity; 4]) {
    let world = app.world_mut();
    let pivot = world
        .query::<(Entity, &crate::kart::KartBodyPivot)>()
        .iter(world)
        .find(|(_, p)| p.body == kart)
        .unwrap()
        .0;
    let mut wheels = [Entity::PLACEHOLDER; 4];
    for (entity, wheel, child_of) in world
        .query::<(Entity, &crate::kart::WheelVisual, &ChildOf)>()
        .iter(world)
    {
        if child_of.parent() == pivot {
            wheels[wheel.index] = entity;
        }
    }
    assert!(wheels.iter().all(|w| *w != Entity::PLACEHOLDER));
    let root = world.get::<ChildOf>(pivot).unwrap().parent();
    (root, wheels)
}

/// Issue #176: a wheel's turn rate (rad/s), its radius, its rotation on the kart.
fn wheel(app: &App, entity: Entity) -> (f32, f32, Quat, crate::kart::WheelVisual) {
    let world = app.world();
    let visual = *world.get::<crate::kart::WheelVisual>(entity).unwrap();
    let rotation = world.get::<Transform>(entity).unwrap().rotation;
    (visual.roll_rate, visual.radius, rotation, visual)
}

/// Issue #176 acceptance: on the keyboard, the wheels roll at the kart's speed, the right way (the
/// top of each wheel goes forward), shown at most `max_spin` fast; setting off, the back tyres spin
/// faster than the kart goes; the front wheels steer with the stick, the back ones never.
#[test]
fn the_wheels_turn_with_the_kart_and_steer() {
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    let (_, wheels) = wheels_of(&mut app, kart);
    run(&mut app, 60);
    for w in wheels {
        assert!(wheel(&app, w).0.abs() < 0.5, "still");
    }
    let tuning = app.world().resource::<VfxTuning>().wheels.clone();
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let mut spun = false;
    let mut followed = 0;
    for frame in 0..150 {
        let before: Vec<f32> = wheels.iter().map(|w| wheel(&app, *w).3.roll).collect();
        app.update();
        let speed = app.world().get::<KartVfx>(kart).unwrap().forward_speed;
        for (i, w) in wheels.iter().enumerate() {
            let (rate, radius, rotation, visual) = wheel(&app, *w);
            // The rotation shown is the wheel rolled (and steered: straight ahead here).
            let expected = wheel_rotation(
                visual.rest,
                if i % 2 == 0 { -1.0 } else { 1.0 },
                0.0,
                0.0,
                visual.roll,
            );
            assert!(
                rotation.dot(expected).abs() > 1.0 - 1e-5,
                "wheel {i}: {rotation} {expected} {visual:?} {}",
                app.world().get::<KartVfx>(kart).unwrap().steer_shown
            );
            // It turned by its rate over the frame.
            let turned = (visual.roll - before[i] - rate * DT).rem_euclid(std::f32::consts::TAU);
            assert!(
                turned < 1e-3 || turned > std::f32::consts::TAU - 1e-3,
                "{turned}"
            );
            if i < 2 {
                // The front wheels roll at the kart's speed (or at most `max_spin`).
                let roll = (-speed / radius).clamp(-tuning.max_spin, tuning.max_spin);
                assert!((rate - roll).abs() < 1e-3, "front {i}: {rate} {roll}");
                if speed > 3.0 && rate > -tuning.max_spin + 0.1 {
                    followed += 1;
                }
            } else if frame < 20 && speed > 0.2 {
                // Setting off, the back tyres spin faster than the kart goes.
                spun |= -rate * radius > speed + 1.0;
            }
            if speed > 1.0 {
                assert!(rate < 0.0, "forward: the top of the wheel goes forward");
                assert!(rate.abs() <= tuning.max_spin + 1e-3);
            }
        }
    }
    assert!(spun, "the back tyres spin setting off");
    assert!(followed > 20, "{followed}");
    let speed = app.world().get::<KartVfx>(kart).unwrap().forward_speed;
    assert!(speed > 15.0, "{speed}");
    // At speed, the band never turns more than half a turn in a frame.
    for w in wheels {
        assert!(wheel(&app, w).0.abs() * DT < std::f32::consts::PI);
    }
    // The stick to the left: the front wheels turn to the left, the back ones stay straight.
    hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::ArrowLeft]);
    run(&mut app, 20);
    for (i, w) in wheels.iter().enumerate() {
        let (_, _, rotation, visual) = wheel(&app, *w);
        let rolled = wheel_rotation(visual.rest, 1.0, 0.0, 0.0, visual.roll);
        // Its axle (the rolled wheel's X), turned toward the left at the front.
        let axle = rotation * (rolled.inverse() * Vec3::X);
        if i < 2 {
            assert!(axle.z < -0.3, "front {i} steers left: {axle}");
        } else {
            assert!(axle.z.abs() < 1e-3, "back {i}: {axle}");
        }
    }
    let steer = app.world().get::<KartVfx>(kart).unwrap().steer_shown;
    assert!((steer - tuning.steer_angle).abs() < 0.05, "{steer}");
}

/// Issue #176: what a start throws, frame by frame, from the first frame of the race (the grid):
/// the smoke of the tyres spinning on the spot during the countdown, the frames the kart's visual
/// shook, the back wheels' fastest turn, and the cloud at GO (the big puffs of its first frames).
#[derive(Debug, Default)]
struct StartLog {
    grid_smoke: usize,
    shaken: usize,
    back_spin: f32,
    front_spin: f32,
    go_cloud: usize,
    soot: usize,
}

/// Prairie Pompon alone, the countdown played: the accelerator pressed when the countdown shows
/// `press_at` (`Some(3)`: before the « 2 », the engine stalls; `Some(2)`: on it, a rocket start;
/// `Some(1)`, 20 ticks after the « 1 »: a normal start; `None`: never), held up to GO and after.
fn a_start(press_at: Option<u32>) -> StartLog {
    let mut app = vfx_app(TrackChoice::default(), 0);
    for _ in 0..600 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    let kart = player(&mut app);
    let (root, wheels) = wheels_of(&mut app, kart);
    // The cloud's puffs (at least 70 % of `go_size`), not the tyres' smoke (at most 120 % of
    // `smoke_size`).
    let look = app.world().resource::<VfxTuning>().wheels.clone();
    let big = look.go_size * 0.69;
    assert!(look.smoke_size * 1.2 < big);
    let mut log = StartLog::default();
    let mut since_one = 0;
    for _ in 0..1200 {
        let (number, ticks) = {
            let race = app.world().resource::<Race>();
            (race.flow.countdown_number(&race.timings), race.race_ticks())
        };
        if number == Some(1) {
            since_one += 1;
        }
        let press = match press_at {
            Some(1) => since_one > 20,
            Some(at) => number.is_some_and(|n| n <= at) || ticks > 0,
            None => false,
        };
        if press {
            hold_keys(&mut app, &[KeyCode::ArrowUp]);
        }
        app.update();
        let ticks = app.world().resource::<Race>().race_ticks();
        let new = new_particles(&app);
        let smoke = new
            .iter()
            .filter(|p| p.shape == Shape::Haze && p.paint == Paint::Smoke);
        if ticks == 0 {
            log.grid_smoke += smoke.count();
            // The visual where the kart is (still on the grid), but for its shakes.
            let world = app.world();
            let shown = world.get::<Transform>(root).unwrap().translation;
            let body = world.get::<Transform>(kart).unwrap().translation;
            if shown.distance(body) > 2e-3 {
                log.shaken += 1;
            }
            for (i, w) in wheels.iter().enumerate() {
                let spin = wheel(&app, *w).0.abs();
                if i < 2 {
                    log.front_spin = log.front_spin.max(spin);
                } else {
                    log.back_spin = log.back_spin.max(spin);
                }
            }
        } else if ticks <= 3 {
            log.go_cloud += smoke.filter(|p| p.size > big).count();
            log.soot += new.iter().filter(|p| p.paint == Paint::Soot).count();
        } else {
            break;
        }
    }
    log
}

/// Issue #176 acceptance (GD § 4): during the countdown, the accelerator down spins the back
/// tyres on the spot (the front wheels stay still): they smoke and the kart shakes a little;
/// nothing without it. At GO, a cloud by the start's quality: the biggest for a rocket start, a
/// smaller one for a normal start, a splutter for a stall (with its soot), none without the
/// accelerator.
#[test]
fn the_tyres_spin_on_the_grid_and_the_start_has_its_cloud() {
    let tuning = WheelVfx::default();
    let rocket = a_start(Some(2));
    let normal = a_start(Some(1));
    let stall = a_start(Some(3));
    let none = a_start(None);
    println!("rocket {rocket:?}\nnormal {normal:?}\nstall {stall:?}\nnone {none:?}");
    // On the grid: two seconds of the accelerator down (from the « 2 »).
    for log in [&rocket, &stall] {
        assert!(log.grid_smoke > 40, "{log:?}");
        assert!(log.shaken > 60, "{log:?}");
        assert!(log.back_spin > 25.0, "{log:?}");
        assert!(log.front_spin < 1.0, "{log:?}");
    }
    assert!(stall.grid_smoke > rocket.grid_smoke, "pressed longer");
    assert!(normal.grid_smoke > 0 && normal.grid_smoke < rocket.grid_smoke);
    assert_eq!((none.grid_smoke, none.shaken), (0, 0), "{none:?}");
    assert!(none.back_spin < 1.0, "{none:?}");
    // At GO, by the start's quality.
    assert!(
        rocket.go_cloud >= tuning.go_rocket_puffs as usize,
        "{rocket:?}"
    );
    assert!(
        normal.go_cloud >= tuning.go_normal_puffs as usize && normal.go_cloud < rocket.go_cloud,
        "{normal:?}"
    );
    assert!(
        stall.go_cloud >= tuning.go_stall_puffs as usize && stall.go_cloud < normal.go_cloud,
        "{stall:?}"
    );
    assert!(stall.soot > 0, "{stall:?}");
    assert_eq!(none.go_cloud, 0, "{none:?}");
}

/// Issue #176: setting off again from a full stop, the tyres spin and smoke too, more discreetly
/// than on the grid (a smaller puff, a short spin), then nothing once the kart is going.
#[test]
fn setting_off_from_a_stop_smokes_more_discreetly() {
    let mut app = vfx_app(TrackChoice::TestGround, 0);
    let kart = player(&mut app);
    run(&mut app, 60);
    let smoke = |app: &App| {
        new_particles(app)
            .iter()
            .filter(|p| p.shape == Shape::Haze && p.paint == Paint::Smoke)
            .count()
    };
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let mut first = 0;
    for _ in 0..60 {
        app.update();
        first += smoke(&app);
    }
    let mut later = 0;
    for _ in 0..60 {
        app.update();
        later += smoke(&app);
    }
    let speed = app.world().get::<KartVfx>(kart).unwrap().forward_speed;
    let grid = a_start(Some(2));
    println!(
        "setting off: {first} puffs in its first second, {later} in the next; the grid's: {grid:?}"
    );
    let tuning = WheelVfx::default();
    assert!(first >= tuning.launch_puffs as usize, "{first}");
    // The grid's smoke in a second (two seconds from the « 2 »), and its cloud at GO.
    assert!(
        first < grid.grid_smoke / 2 + grid.go_cloud,
        "{first} {grid:?}"
    );
    assert_eq!(later, 0, "going: no more smoke ({speed} m/s)");
}
