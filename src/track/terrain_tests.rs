//! Issue #195 acceptance, headless (the whole game, `GamePlugin` with `MinimalPlugins`, one fixed
//! tick per `app.update()`): a kart out of the track never goes through the terrain. Dropped on a
//! slope, it lands on it and stays on it; driven up a slope at full throttle, it climbs it, never
//! under it. On the Col des Moufles (the highest terrain, sharp crests) and the Corniche Cannelle
//! (its cliffs in steps), and their mirror images.

use bevy_rapier3d::prelude::{Collider, Velocity};

use super::*;
use crate::admin::skip_countdown_now;
use crate::interpolation::PhysicsPose;
use crate::kart::{KartInput, KartState, KartWheels, PlayerKart};
use crate::race::{Race, Tow};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::terrain::Terrain;

/// The game on the circuit file `path` (its mirror image or not), the player's kart alone, its
/// race started (the test drives it).
fn game_on(path: &str, mirrored: bool) -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::Circuit(path.into()), 0);
    app.insert_resource(MirrorMode(mirrored));
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    for tries in 0.. {
        let world = app.world();
        if world.get_resource::<Race>().is_some()
            && world
                .get_resource::<TrackLayout>()
                .is_some_and(|layout| layout.mirrored == mirrored)
        {
            break;
        }
        assert!(tries < 2000, "the race never came");
        app.update();
    }
    skip_countdown_now(app.world_mut());
    for _ in 0..3 {
        app.update();
    }
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    (app, kart)
}

/// Raised points of the solid terrain (35 to 90 m beyond the walls: a kart out of the track
/// reaches the first of them),
/// at least `height` meters up, on a slope that climbs but can be driven (rise over run 0.15 to 0.6
/// across 4 m), far apart: (the point, uphill there).
fn slopes(terrain: &Terrain, height: f32, count: usize) -> Vec<(Vec2, Vec2)> {
    let mut found: Vec<(Vec2, Vec2)> = Vec::new();
    for j in 0..terrain.rows {
        for i in 0..terrain.cols {
            let k = j * terrain.cols + i;
            if !(35.0..=90.0).contains(&terrain.clearance[k]) || terrain.heights[k] < height {
                continue;
            }
            let mut p = terrain.origin + Vec2::new(i as f32, j as f32) * terrain.cell;
            if terrain.mirrored {
                p.x = -p.x;
            }
            let at = |o: Vec2| terrain.surface(p + o).unwrap_or(f32::MIN);
            let gradient = Vec2::new(
                at(Vec2::X * 2.0) - at(Vec2::NEG_X * 2.0),
                at(Vec2::Y * 2.0) - at(Vec2::NEG_Y * 2.0),
            ) / 4.0;
            if gradient.length() > 0.6
                || gradient.length() < 0.15
                || found.iter().any(|(q, _)| q.distance(p) < 80.0)
            {
                continue;
            }
            found.push((p, gradient.normalize()));
            if found.len() == count {
                return found;
            }
        }
    }
    found
}

/// Puts the kart at `p` (ground plane), `above` meters over the terrain, facing `toward`, at
/// `speed` that way: the race's state left as it is (the pelican comes after 1 s out of the
/// track; the runs are shorter).
fn place(
    app: &mut App,
    kart: Entity,
    terrain: &Terrain,
    p: Vec2,
    above: f32,
    toward: Vec2,
    speed: f32,
) {
    let y = terrain.surface(p).unwrap() + above;
    let at = Vec3::new(p.x, y, p.y);
    let transform =
        Transform::from_translation(at).looking_to(Vec3::new(toward.x, 0.0, toward.y), Vec3::Y);
    let world = app.world_mut();
    *world.get_mut::<Transform>(kart).unwrap() = transform;
    world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    *world.get_mut::<Velocity>(kart).unwrap() = Velocity {
        linear: Vec3::new(toward.x, 0.0, toward.y) * speed,
        angular: Vec3::ZERO,
    };
    *world.get_mut::<KartState>(kart).unwrap() = KartState::Airborne;
    *world.get_mut::<KartWheels>(kart).unwrap() = KartWheels::default();
    world.get_mut::<Tow>(kart).unwrap().reset(at);
}

/// Every collider of the game hit by a ray straight down from `from`, the first one: how far, and
/// its surface.
fn first_hit_down(app: &mut App, from: Vec3) -> Option<(f32, Option<Surface>)> {
    let world = app.world_mut();
    world
        .query::<(&Collider, &GlobalTransform, Option<&Surface>)>()
        .iter(world)
        .filter_map(|(collider, transform, surface)| {
            let (_, rotation, translation) = transform.to_scale_rotation_translation();
            collider
                .cast_ray(translation, rotation, from, Vec3::NEG_Y, 500.0, true)
                .map(|toi| (toi, surface.copied()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

#[test]
fn a_kart_out_of_the_track_never_goes_through_the_terrain() {
    let runs = [
        ("tracks/col.track.ron", false),
        ("tracks/col.track.ron", true),
        ("tracks/corniche.track.ron", false),
        ("tracks/corniche.track.ron", true),
    ];
    let failures: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = runs
            .iter()
            .map(|&(path, mirrored)| scope.spawn(move || terrain_runs(path, mirrored)))
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn terrain_runs(path: &str, mirrored: bool) -> Vec<String> {
    let (mut app, kart) = game_on(path, mirrored);
    let terrain = app.world().resource::<Terrain>().clone();
    assert!(!terrain.is_empty(), "{path}: no terrain");
    assert_eq!(terrain.mirrored, mirrored);
    let name = format!("{path}{}", if mirrored { " (miroir)" } else { "" });
    let points = slopes(&terrain, 4.0, 6);
    assert!(points.len() >= 4, "{name}: only {} slopes", points.len());
    let mut failures = Vec::new();
    for (p, uphill) in points {
        // Solid: a ray down meets the terrain itself, off-road (grass), at its surface.
        let surface = terrain.surface(p).unwrap();
        match first_hit_down(&mut app, Vec3::new(p.x, surface + 50.0, p.y)) {
            Some((toi, Some(Surface::Offroad))) if (50.0 - toi).abs() < 0.05 => {}
            other => failures.push(format!("{name}: at {p}, the ray down met {other:?}")),
        }
        // Dropped 1.5 m above it, then driven up it at full throttle from 12 m/s.
        for (above, speed, input) in [
            (1.5, 0.0, KartInput::default()),
            (
                0.8,
                12.0,
                KartInput {
                    throttle: 1.0,
                    ..default()
                },
            ),
        ] {
            place(&mut app, kart, &terrain, p, above, uphill, speed);
            let mut lowest = f32::MAX;
            // The terrain under the kart as it drove: its lowest and its highest.
            let mut under = (f32::MAX, f32::MIN);
            let mut last = (Vec3::ZERO, Vec3::ZERO, 0);
            for tick in 0..50 {
                *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
                app.update();
                let world = app.world();
                if *world.get::<KartState>(kart).unwrap() == KartState::Respawning {
                    break;
                }
                let at = world.get::<Transform>(kart).unwrap().translation;
                last = (at, world.get::<Velocity>(kart).unwrap().linear, tick);
                let Some(ground) = terrain.surface(at.xz()) else {
                    continue;
                };
                lowest = lowest.min(at.y - ground.max(mesh::GROUND_HEIGHT));
                under = (under.0.min(ground), under.1.max(ground));
            }
            if lowest < 0.2 {
                failures.push(format!(
                    "{name}: at {p} (from {above} m, {speed} m/s), the kart's center {lowest:.2} m \
                     over the terrain"
                ));
            }
            // Driven: it went on over the slope (at least 4 m), and is on it or just over it at the
            // end (over a crest, it may leave the ground a little).
            let (end, _, _) = last;
            let moved = end.xz().distance(p);
            let on_it = terrain
                .surface(end.xz())
                .is_some_and(|ground| (0.2..=3.0).contains(&(end.y - ground)));
            if speed > 0.0 && (moved < 4.0 || !on_it) {
                failures.push(format!(
                    "{name}: at {p}, the kart did not drive over the slope ({moved:.1} m, the \
                     terrain under it {under:?}; at the end {last:?})"
                ));
            }
        }
    }
    failures
}

/// Issue #219: on a circuit whose terrain hugs the road, the embankment comes up to
/// `terrain::EMBANK_LIP` under the road at the foot of its walls: a kart out of the track there
/// (thrown over a wall) drives into the curtain under the wall (`mesh::CURTAIN_DEPTH`), never
/// under the wall and under the road, where the race would not see it out of the track. On the
/// Coteau Citrouille and its mirror, every 60 m or so where a side is walled and its embankment
/// at the foot of the wall: the kart dropped on the embankment 3 m beyond the wall, driven at
/// full throttle straight at the road from 10 m/s: never under the road (between its walls, its
/// center under the road's surface).
#[test]
fn a_kart_on_the_embankment_never_drives_under_the_road() {
    let failures: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = [false, true]
            .into_iter()
            .map(|mirrored| scope.spawn(move || under_road_runs(mirrored)))
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn under_road_runs(mirrored: bool) -> Vec<String> {
    let (mut app, kart) = game_on("tracks/coteau.track.ron", mirrored);
    let terrain = app.world().resource::<Terrain>().clone();
    let layout = app.world().resource::<TrackLayout>().clone();
    let center = &layout.center;
    let name = format!(
        "Coteau Citrouille{}",
        if mirrored { " (miroir)" } else { "" }
    );
    let mut failures = Vec::new();
    let mut runs = 0;
    let mut last = f32::MIN;
    for s in center.samples() {
        if s.distance - last < 60.0 {
            continue;
        }
        for side in [-1.0f32, 1.0] {
            if ![-3.0f32, 0.0, 3.0]
                .iter()
                .all(|o| shortcut::walled(&layout.open_edges, center, s.distance + o, side))
            {
                continue;
            }
            let outer = mesh::wall_offset(s) + mesh::WALL_THICKNESS;
            let flat = Vec3::new(s.right.x, 0.0, s.right.z).normalize_or(Vec3::X) * side;
            let p = (s.position + flat * (outer + 3.0)).xz();
            // On the embankment: the terrain just under the road's level there.
            let Some(ground) = terrain.surface(p) else {
                continue;
            };
            let road = s.position.y;
            if !(road - 1.5..=road - 0.5).contains(&ground) {
                continue;
            }
            last = s.distance;
            runs += 1;
            let toward = -flat.xz();
            place(&mut app, kart, &terrain, p, 0.6, toward, 10.0);
            for tick in 0..90 {
                *app.world_mut().get_mut::<KartInput>(kart).unwrap() = KartInput {
                    throttle: 1.0,
                    ..default()
                };
                app.update();
                let world = app.world();
                if *world.get::<KartState>(kart).unwrap() == KartState::Respawning {
                    break;
                }
                let at = world.get::<Transform>(kart).unwrap().translation;
                let lateral = (at - s.position).dot(flat);
                if lateral < mesh::wall_offset(s) && at.y < s.point(lateral * side, 0.0).y {
                    failures.push(format!(
                        "{name}: from {:.0} m (side {side}), under the road at tick {tick}: {at}",
                        s.distance
                    ));
                    break;
                }
            }
        }
    }
    assert!(runs >= 10, "{name}: {runs} runs only");
    failures
}
