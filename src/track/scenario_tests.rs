//! Headless crashes into the walls of Prairie Pompon (issue #27): the whole game (`GamePlugin`)
//! with `MinimalPlugins`, one fixed tick per `app.update()`.
//!
//! A kart that hits a wall bounces off it and stays on the track, whatever the angle, its speed,
//! a drift, a boost or its weight. Before issue #27 it could climb the 0.8 m walls and fall out:
//! the impact landed above its center of mass (0.15 m below the middle of its collision box),
//! which kicked the side facing the wall up, or its nose, by 20 to 30°; it then drove up onto the
//! top edge of the wall, and its suspension rays found ground on the flat top of the wall.
//!
//! Issue #47, on the circuits of `circuits()` (three since issue #19): the walls are solid
//! exactly as high as they are drawn ([`WALL_HEIGHT`], 1.2 m), and the chase camera still sees
//! the road over them.

use std::collections::HashSet;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_rapier3d::prelude::{Collider, QueryFilter, ReadRapierContext, Velocity};

use crate::TICK_HZ;
use crate::admin::skip_countdown_now;
use crate::bots::{Bot, BotDifficulty, BotTuning, bot, bot_seed};
use crate::camera::{CameraShake, CameraTuning, ChaseCamera};
use crate::characters::{Loadout, PartKind, PlayerLoadout, Roster};
use crate::interpolation::InterpolatedFrom;
use crate::kart::{Boost, KartInput, KartState, PlayerKart};
use crate::race::{Course, Race, RaceGrid, RaceProgress, Racer};
use crate::tests::{
    follow_centerline, game_app_unloaded, hold_keys, player, single_threaded, wait_for_the_player,
};
use crate::track::mesh::{WALL_HEIGHT, WALL_THICKNESS, wall_offset};
use crate::track::shortcut::walled;
use crate::track::{Surface, TrackChoice, TrackLayout};

/// The kart's parts: the default ones (every stat at the middle), or the roster's lightest and
/// heaviest loadouts (issue #15).
#[derive(Clone, Copy, Debug)]
enum Build {
    Middle,
    Light,
    Heavy,
}

impl Build {
    fn loadout(self) -> Option<Loadout> {
        let names = match self {
            Self::Middle => return None,
            Self::Light => ["Poussin", "Panier en osier", "Bobines de fil", "Plume"],
            Self::Heavy => [
                "Cochon",
                "Baignoire",
                "Pneus de tracteur",
                "Taie d'oreiller",
            ],
        };
        let roster = Roster::embedded();
        let mut loadout = Loadout::default();
        for (kind, name) in PartKind::ALL.into_iter().zip(names) {
            loadout.set(kind, roster.find(kind, name).unwrap());
        }
        Some(loadout)
    }
}

/// What the driver does on top of steering into the wall.
#[derive(Clone, Copy, Debug)]
enum Extra {
    Nothing,
    /// A hop steered toward the wall, the button held: a drift into it.
    Drift,
    /// A +30 % boost of 2 s (a piment, an orange or purple mini-turbo) as it turns.
    Boost,
}

/// A crash into a wall.
#[derive(Clone, Copy, Debug)]
struct Crash {
    build: Build,
    /// The test autopilot first drives this far along the lap from the pole (m; 0: not at all).
    autopilot: f32,
    /// Then full throttle, straight on, for this long (s): the speed of the crash.
    straight: f32,
    /// Then full throttle steering this way (-1: left, 1: right) for [`CRASH_SECONDS`].
    steer: f32,
    extra: Extra,
}

/// How long a crash steers into the wall: it hits it within 1.2 s, and a kart that climbed was
/// over the wall less than 1 s later. Shorter than the 3 s after which the pelican comes for a
/// kart that keeps pushing against a wall without getting anywhere.
const CRASH_SECONDS: f32 = 2.5;

/// What happened during a crash.
#[derive(Clone, Copy, Debug)]
struct Outcome {
    /// Speed as the steering started (m/s).
    speed: f32,
    /// Highest point of the kart's center above the road (m; 0.55 at rest, at least 1.45 on top
    /// of a wall, 1.2 m high since issue #47: 1.05 on the 0.8 m walls of #27).
    highest: f32,
    /// Closest the kart's center came to the inner face of a wall (m; negative: past it).
    closest: f32,
    /// Fastest the kart moved away from the wall after touching it, across the road (m/s).
    bounce: f32,
    /// Lowest uprightness of the kart against the road (1: wheels down, 0.94: tilted by 20°).
    lowest_up: f32,
    /// The pelican came for it.
    towed: bool,
}

/// The game on Prairie Pompon, the player alone on the pole with `build`, once the race exists.
fn crash_app(build: Build) -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    if let Some(loadout) = build.loadout() {
        app.insert_resource(PlayerLoadout(loadout));
    }
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    while app.world().get_resource::<Race>().is_none() {
        app.update();
    }
    (app, kart)
}

/// Follows a crash tick by tick.
struct Watch {
    kart: Entity,
    course: Course,
    outcome: Outcome,
    /// Side of the wall touched (sign of the lateral offset), once the kart touched one.
    wall_side: Option<f32>,
}

impl Watch {
    fn new(app: &App, kart: Entity) -> Self {
        Self {
            kart,
            course: app.world().resource::<Course>().clone(),
            outcome: Outcome {
                speed: 0.0,
                highest: 0.0,
                closest: f32::MAX,
                bounce: 0.0,
                lowest_up: 1.0,
                towed: false,
            },
            wall_side: None,
        }
    }

    fn observe(&mut self, app: &App) {
        let world = app.world();
        if *world.get::<KartState>(self.kart).unwrap() == KartState::Respawning {
            self.outcome.towed = true;
            return;
        }
        let transform = world.get::<Transform>(self.kart).unwrap();
        let passed = world.get::<RaceProgress>(self.kart).unwrap().passed;
        let p = self.course.project(passed, transform.translation);
        let sample = self.course.center.at(p.distance);
        let to_wall = p.wall - p.lateral.abs();
        let outcome = &mut self.outcome;
        outcome.highest = outcome.highest.max(p.height);
        outcome.closest = outcome.closest.min(to_wall);
        outcome.lowest_up = outcome.lowest_up.min(transform.up().dot(sample.up));
        // Touching a wall: the kart's center less than its half width (0.7 m) and a margin
        // from it.
        if to_wall < 1.25 {
            self.wall_side.get_or_insert(p.lateral.signum());
        }
        if let Some(side) = self.wall_side {
            let across = world
                .get::<Velocity>(self.kart)
                .unwrap()
                .linear
                .dot(sample.right);
            outcome.bounce = outcome.bounce.max(-side * across);
        }
    }
}

fn drive(app: &mut App, kart: Entity, input: KartInput) {
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
}

/// One tick of the test autopilot.
fn autopilot(app: &mut App, kart: Entity, layout: &TrackLayout) {
    let world = app.world();
    let transform = *world.get::<Transform>(kart).unwrap();
    let speed = world.get::<Velocity>(kart).unwrap().linear.length();
    drive(app, kart, follow_centerline(layout, &transform, speed));
}

fn crash(c: Crash) -> Outcome {
    let (mut app, kart) = crash_app(c.build);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    skip_countdown_now(app.world_mut());
    let layout = app.world().resource::<TrackLayout>().clone();
    let center = &layout.center;
    let along = |app: &App| {
        let position = app.world().get::<Transform>(kart).unwrap().translation;
        center.samples()[center.nearest_index(position)].distance
    };
    let start = along(&app);
    let mut ticks = 0;
    while c.autopilot > 0.0 && center.wrap(along(&app) - start) < c.autopilot {
        ticks += 1;
        assert!(ticks < (30.0 * TICK_HZ) as u32, "{c:?}: never got there");
        autopilot(&mut app, kart, &layout);
    }
    let mut watch = Watch::new(&app, kart);
    let throttle = KartInput {
        throttle: 1.0,
        ..default()
    };
    for _ in 0..(c.straight * TICK_HZ as f32).round() as usize {
        drive(&mut app, kart, throttle);
        watch.observe(&app);
    }
    let world = app.world();
    watch.outcome.speed = world
        .get::<Velocity>(kart)
        .unwrap()
        .linear
        .dot(*world.get::<Transform>(kart).unwrap().forward());
    if matches!(c.extra, Extra::Boost) {
        let mut boost = app.world_mut().get_mut::<Boost>(kart).unwrap();
        boost.add(0.3, 2.0);
    }
    let steer = KartInput {
        steer: c.steer,
        jump_drift: matches!(c.extra, Extra::Drift),
        ..throttle
    };
    for _ in 0..(CRASH_SECONDS * TICK_HZ as f32).round() as usize {
        drive(&mut app, kart, steer);
        watch.observe(&app);
    }
    watch.outcome
}

/// Runs the crashes side by side, a few threads at a time; the outcomes come back in order.
fn crash_all(crashes: &[Crash]) -> Vec<Outcome> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let mut outcomes = Vec::with_capacity(crashes.len());
    for batch in crashes.chunks(threads) {
        std::thread::scope(|scope| {
            let runs: Vec<_> = batch
                .iter()
                .map(|c| scope.spawn(move || crash(*c)))
                .collect();
            outcomes.extend(runs.into_iter().map(|run| {
                run.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            }));
        });
    }
    outcomes
}

/// Issue #27 acceptance, the crash of the issue: on the start straight, the up arrow for 1.75 s
/// after GO, then the left arrow too. The kart hits the left wall at about 21 m/s across the road
/// (60° off its line), bounces off it and stays on the track, wheels down (tilted by 11° at
/// most). Then the down arrow backs it off the wall, and it drives on. Before the fix, it rose
/// onto the wall (its center 0.97 m above the road within 1.5 s, 1.5 m a little later) and fell
/// out of the track.
#[test]
fn the_crash_of_the_issue_bounces_off_the_wall() {
    let (mut app, kart) = crash_app(Build::Middle);
    while !app.world().resource::<Race>().started() {
        app.update();
    }
    let mut watch = Watch::new(&app, kart);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..(1.75 * TICK_HZ) as usize {
        app.update();
    }
    hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::ArrowLeft]);
    for _ in 0..(1.5 * TICK_HZ) as usize {
        app.update();
        watch.observe(&app);
    }
    let crash = watch.outcome;
    println!("the crash of the issue: {crash:?}");
    assert_eq!(watch.wall_side, Some(-1.0), "never reached the left wall");
    assert!(crash.bounce > 2.0, "no bounce: {crash:?}");
    assert!(crash.highest < 0.75, "lifted: {crash:?}");
    assert!(crash.closest > 0.6, "into the wall: {crash:?}");
    assert!(crash.lowest_up > 0.94, "tipped up: {crash:?}");
    assert!(!crash.towed, "{crash:?}");

    // Nose against the wall: backing off it (the down arrow, 1 s) gets it out, and the test
    // autopilot takes it on, through the next checkpoints, without the pelican.
    hold_keys(&mut app, &[KeyCode::ArrowDown]);
    for _ in 0..TICK_HZ as usize {
        app.update();
        watch.observe(&app);
    }
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    hold_keys(&mut app, &[]);
    let passed = app.world().get::<RaceProgress>(kart).unwrap().passed;
    let layout = app.world().resource::<TrackLayout>().clone();
    for _ in 0..(4.0 * TICK_HZ) as usize {
        autopilot(&mut app, kart, &layout);
        watch.observe(&app);
    }
    assert!(!watch.outcome.towed);
    let now = app.world().get::<RaceProgress>(kart).unwrap().passed;
    assert!(now >= passed + 2, "did not drive on: {passed} → {now}");
}

/// Issue #27, the crashes that sent a kart over a wall before the fix, and more: the walls of the
/// start straight at 3 speeds (14 to 26 m/s), steering fully or half into the left wall, or half
/// into the right one, straight in, in a drift or with a +30 % boost, with light, middle and
/// heavy karts; and the walls after the hairpin, at 25 to 28 m/s. The kart never rises onto a
/// wall (its center stays below 1 m: a hop takes it to 0.89 m, on top of a 0.8 m wall it would be
/// at 1.05 m at least, and the walls are 1.2 m high since #47), never comes closer to a wall than its half width, never tips by more than
/// 30° and is never towed. Before the fix, 37 of these 87 crashes broke one of these rules, and
/// 30 ended with the kart's center past the wall's face.
#[test]
fn no_crash_sends_a_kart_over_a_wall() {
    let mut crashes = Vec::new();
    for build in [Build::Middle, Build::Light, Build::Heavy] {
        for straight in [0.75, 1.0, 1.75] {
            for steer in [-1.0, -0.5, 0.5] {
                for extra in [Extra::Nothing, Extra::Drift, Extra::Boost] {
                    crashes.push(Crash {
                        build,
                        autopilot: 0.0,
                        straight,
                        steer,
                        extra,
                    });
                }
            }
        }
    }
    // Out of the hairpin (it ends about 195 m along the lap since issue #108: 125 m before):
    // into its outer wall on the left, or back into the right one.
    for build in [Build::Middle, Build::Heavy] {
        for (autopilot, steer, extra) in [
            (200.0, -1.0, Extra::Boost),
            (200.0, 1.0, Extra::Nothing),
            (210.0, 1.0, Extra::Drift),
        ] {
            crashes.push(Crash {
                build,
                autopilot,
                straight: 0.0,
                steer,
                extra,
            });
        }
    }
    let outcomes = crash_all(&crashes);
    let mut failures = Vec::new();
    for (c, o) in crashes.iter().zip(&outcomes) {
        println!(
            "{:?} {:>3.0} m {:.2} s steer {:+.1} {:?}: {:.1} m/s, highest {:.2} m, closest {:.2} m, \
             bounce {:.1} m/s, up {:.2}{}",
            c.build,
            c.autopilot,
            c.straight,
            c.steer,
            c.extra,
            o.speed,
            o.highest,
            o.closest,
            o.bounce,
            o.lowest_up,
            if o.towed { ", towed" } else { "" }
        );
        if o.highest >= 1.0 || o.closest < 0.6 || o.lowest_up < 0.85 || o.towed {
            failures.push(format!("{c:?}: {o:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} crashes out of {} over the line:\n{}",
        failures.len(),
        crashes.len(),
        failures.join("\n")
    );
}

// --- Issue #47: walls drawn as high as they are solid, and the chase camera over them -------------

/// The circuits of the walls' test (and of the camera's, but issue #148: `camera_circuits()`):
/// the two of issue #47 and the Crique Câline (issue #19). Not the Col des Moufles: on its switchbacks, the walls of the leg the kart
/// is on hide a little more of the next leg, lower down the mountain, than the 2 % of the camera's
/// test (2.05 % measured, never the road just ahead nor the kart: 0 frames); its walls are the
/// same as everywhere (`mesh::WALL_HEIGHT`).
fn circuits() -> [TrackChoice; 4] {
    [
        TrackChoice::default(),
        TrackChoice::Circuit("tracks/anneau.track.ron".into()),
        TrackChoice::Circuit("tracks/crique.track.ron".into()),
        // Issue #78: the camera rolls with the kart onto the anti-gravity wall.
        TrackChoice::Circuit("tracks/ruban.track.ron".into()),
    ]
}

/// The circuits of the camera's test: those of `circuits()`, but the Anneau Guimauve and the
/// Crique Câline redrawn (issue #148) for the Lagon Duvet (issue #20), for the reason the Col des
/// Moufles is not in `circuits()`: their lacets (five hairpins of 22 m, the other leg 44 m away,
/// lower or higher) put more of the next leg behind the walls of the leg the kart is on than the
/// 2 % of the test (3.8 % for the autopilot, 4.1 to 4.3 % for a Hard bot, measured), never the
/// road just ahead nor the kart (0 frames), never the anti-wall (0 frames); their walls are the
/// same as everywhere (`mesh::WALL_HEIGHT`), and solid as drawn (`circuits()`, above).
fn camera_circuits() -> [TrackChoice; 3] {
    [
        TrackChoice::default(),
        TrackChoice::Circuit("tracks/lagon.track.ron".into()),
        TrackChoice::Circuit("tracks/ruban.track.ron".into()),
    ]
}

/// The game on `track` with `bots` bots, once the race and its bots exist.
fn race_app(track: TrackChoice, bots: usize) -> (App, Entity) {
    let mut app = game_app_unloaded(track, bots);
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    for _ in 0..20 {
        let world = app.world_mut();
        if world.get_resource::<Race>().is_some()
            && world.query::<&Bot>().iter(world).count() == bots
        {
            return (app, kart);
        }
        app.update();
    }
    panic!("the race or its bots never came");
}

/// The circuit's walls as the karts meet them: the one trimesh collider with `Surface::Wall`
/// (issue #144: a fixed obstacle is a wall to the karts too, a simple shape; issue #180: the faces
/// of the drops too, a collider of their own).
fn wall_collider(app: &mut App) -> Collider {
    let world = app.world_mut();
    let walls: Vec<Collider> = world
        .query_filtered::<(&Collider, &Surface), Without<super::circuit::DropFace>>()
        .iter(world)
        .filter(|(collider, surface)| **surface == Surface::Wall && collider.as_trimesh().is_some())
        .map(|(collider, _)| collider.clone())
        .collect();
    assert_eq!(walls.len(), 1, "one collider for the walls");
    walls.into_iter().next().unwrap()
}

/// A point, to the millimeter: the same vertex in two meshes.
fn mm(p: Vec3) -> [i64; 3] {
    (p * 1000.0).round().as_i64vec3().to_array()
}

/// Issue #47 acceptance: in the game as built, on the circuits of `circuits()`, the walls are
/// solid exactly as high as they are drawn. Every vertex of their collider is a vertex of the circuit's drawn mesh
/// (nothing solid that is not drawn); the top edge of every wall is drawn [`WALL_HEIGHT`] above
/// the road (issue #180: plus `mesh::wall_lift` along the back of a bump and below a drop); and a ray down onto the middle of every wall meets its collider there, all around
/// the lap. Before #47, the walls were drawn 0.8 m high under a collider of 1.2 m: none of the
/// collider's top vertices was drawn, and the rays met it 0.4 m above the drawn top.
#[test]
fn the_walls_are_solid_exactly_as_high_as_they_are_drawn() {
    for track in circuits() {
        let (mut app, _) = race_app(track, 0);
        let collider = wall_collider(&mut app);
        let world = app.world_mut();
        let layout = world.resource::<TrackLayout>().clone();
        let handle = world
            .query::<(&Name, &Mesh3d)>()
            .iter(world)
            .find(|(name, _)| name.as_str().starts_with("Track:"))
            .map(|(_, mesh)| mesh.0.clone())
            .expect("the circuit's mesh");
        let drawn: HashSet<[i64; 3]> = match world
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
        {
            Some(VertexAttributeValues::Float32x3(positions)) => {
                positions.iter().map(|p| mm(Vec3::from_array(*p))).collect()
            }
            other => panic!("positions: {other:?}"),
        };
        let trimesh = collider.as_trimesh().expect("a trimesh");
        let undrawn = trimesh
            .vertices()
            .filter(|v| !drawn.contains(&mm(*v)))
            .count();
        assert_eq!(
            undrawn,
            0,
            "{}: {undrawn} of the {} vertices of the walls' collider are not drawn",
            layout.name,
            trimesh.vertices().len()
        );
        let mut missed = Vec::new();
        for s in layout.center.samples() {
            for side in [-1.0, 1.0] {
                // Issue #107: no wall on a side without wall (Prairie Pompon has some since
                // issue #108), nor at the very end of a wall (a sample's quads on both sides
                // agree: `mesh`'s test of the sides without wall).
                if ![-1.5f32, 0.0, 1.5]
                    .iter()
                    .all(|o| walled(&layout.open_edges, &layout.center, s.distance + o, side))
                {
                    continue;
                }
                // Issue #180: nor right at the step of a drop (the walls end and start there,
                // between two samples).
                if layout
                    .center
                    .cliffs()
                    .iter()
                    .any(|c| (layout.center.wrap(s.distance - c.at + 1.5)) < 3.0)
                {
                    continue;
                }
                // Issue #180: higher along the back of a bump and below a drop (`wall_lift`).
                let height =
                    WALL_HEIGHT + crate::track::mesh::wall_lift(&layout.center, s.distance);
                let top = s.point(side * wall_offset(s), height);
                if !drawn.contains(&mm(top)) {
                    missed.push(format!("not drawn at {top} ({:.0} m)", s.distance));
                }
                // 1 m above the middle of the wall's top, straight down onto it (1 cm past the
                // sample: a ray exactly along the edge between two triangles can slip through).
                let above = s.point(side * (wall_offset(s) + WALL_THICKNESS / 2.0), 1.0 + height)
                    + s.forward * 0.01;
                let toi = collider.cast_ray(Vec3::ZERO, Quat::IDENTITY, above, -s.up, 3.0, false);
                if toi.is_none_or(|toi| (toi - 1.0).abs() > 0.01) {
                    missed.push(format!(
                        "solid top at {toi:?} m under {above} ({:.0} m)",
                        s.distance
                    ));
                }
            }
        }
        assert!(
            missed.is_empty(),
            "{}: {} misses, first ones:\n{}",
            layout.name,
            missed.len(),
            missed[..missed.len().min(10)].join("\n")
        );
    }
}

/// What the chase camera saw of the road during a race (issue #47).
#[derive(Resource, Clone, Debug, Default)]
struct CameraView {
    frames: usize,
    /// Frames where the camera's anti-wall ray (issue #2) met something and pulled it in.
    pulled_in: usize,
    /// Points of the road ahead on screen (3 to 45 m along the track, 3 across), and those a wall
    /// hides from the camera.
    on_screen: usize,
    hidden: usize,
    /// The same, for the road just ahead (3 to 20 m).
    near_on_screen: usize,
    near_hidden: usize,
    /// The worst frame: share of its road ahead on screen hidden by a wall.
    worst: f32,
    /// Frames where a wall hides the center of the kart followed.
    kart_hidden: usize,
}

/// The walls the camera looks over (the karts' and the drawn ones are the same, see above).
#[derive(Resource)]
struct Walls(Collider);

/// Picture size of the game's window (16:9): what is on screen.
const ASPECT: f32 = 16.0 / 9.0;

/// Runs after the camera has moved, each frame: the anti-wall ray of `camera::chase` cast again
/// (from the kart, up to the camera, a few millimeters further), and rays from the camera to the
/// road ahead and to the kart against the walls.
fn watch_the_view(
    time: Res<Time>,
    rapier: ReadRapierContext,
    tuning: Res<CameraTuning>,
    layout: Res<TrackLayout>,
    walls: Res<Walls>,
    cameras: Query<(&ChaseCamera, &Transform, &Projection)>,
    visuals: Query<(&Transform, &InterpolatedFrom), Without<ChaseCamera>>,
    shakes: Query<&CameraShake>,
    mut view: ResMut<CameraView>,
) {
    let (Ok((camera, eye, projection)), Ok(context)) = (cameras.single(), rapier.single()) else {
        return;
    };
    let Ok((kart, body)) = visuals.get(camera.target) else {
        return;
    };
    view.frames += 1;

    // The anti-wall: where the chase put the camera, without the shake of a paw strike, is short
    // of the rig's pose exactly when its ray met something on the way.
    let shake = shakes
        .get(body.0)
        .map_or(Vec3::ZERO, |shake| shake.offset(time.elapsed_secs()));
    // Issue #78: from above the kart along the camera's up (the world's, but on an anti-gravity
    // wall the kart's), as `camera::chase` casts it.
    let from = kart.translation + camera.rig.up() * tuning.look_height;
    let to_eye = eye.translation - shake - from;
    let length = to_eye.length();
    if length < 1e-3
        || context
            .cast_ray(
                from,
                to_eye / length,
                length + tuning.clearance + 0.005,
                true,
                QueryFilter::only_fixed(),
            )
            .is_some()
    {
        view.pulled_in += 1;
    }

    let hidden = |p: Vec3| {
        let to = p - eye.translation;
        let distance = to.length();
        walls
            .0
            .cast_ray(
                Vec3::ZERO,
                Quat::IDENTITY,
                eye.translation,
                to / distance,
                distance - 0.05,
                false,
            )
            .is_some()
    };
    let Projection::Perspective(perspective) = projection else {
        return;
    };
    let tan = (perspective.fov / 2.0).tan();
    let to_camera = eye.to_matrix().inverse();
    let center = &layout.center;
    let here = center.samples()[center.nearest_index(kart.translation)].distance;
    // Issue #108: on a shortcut (the hidden road, the grass path of a circuit drawn after the
    // guide), the road ahead is its path, then the road after its end.
    let on_a_shortcut = layout.shortcuts.iter().find_map(|path| {
        let at = path.locate(kart.translation);
        (at.along >= 0.0 && at.along <= path.length() && at.lateral.abs() <= path.half_width)
            .then_some((path, at.along))
    });
    let (mut seen, mut lost) = (0, 0);
    for k in 0..29 {
        let ahead = 3.0 + 1.5 * k as f32;
        let s = match on_a_shortcut {
            Some((path, along)) if along + ahead <= path.length() => path.at(along + ahead),
            Some((path, along)) => center.at(path.from + path.span + along + ahead - path.length()),
            None => center.at(here + ahead),
        };
        for across in [-0.8, 0.0, 0.8] {
            let p = s.point(across * s.half_width, 0.05);
            let local = to_camera.transform_point3(p);
            let on_screen = local.z < -0.1
                && (local.x / -local.z).abs() <= tan * ASPECT
                && (local.y / -local.z).abs() <= tan;
            if !on_screen {
                continue;
            }
            let lost_here = hidden(p);
            seen += 1;
            lost += usize::from(lost_here);
            if ahead <= 20.0 {
                view.near_on_screen += 1;
                view.near_hidden += usize::from(lost_here);
            }
        }
    }
    view.on_screen += seen;
    view.hidden += lost;
    if seen >= 10 {
        view.worst = view.worst.max(lost as f32 / seen as f32);
    }
    view.kart_hidden += usize::from(hidden(kart.translation));
}

/// Who drives the kart the camera follows.
#[derive(Clone, Copy, Debug)]
enum Driver {
    /// The test autopilot, alone: one lap along the centerline.
    Autopilot,
    /// A Hard bot among 11 Normal ones: 3 laps of a race, on the racing line, drifting, in the
    /// shunts, against the walls.
    HardBotInTheGrid,
}

/// What the chase camera saw while `driver` drove on `track`.
fn watch_a_race(track: TrackChoice, driver: Driver) -> CameraView {
    let bots = match driver {
        Driver::Autopilot => 0,
        Driver::HardBotInTheGrid => 11,
    };
    let (mut app, kart) = race_app(track, bots);
    let layout = app.world().resource::<TrackLayout>().clone();
    let laps = match driver {
        Driver::Autopilot => {
            app.world_mut().entity_mut(kart).remove::<PlayerKart>();
            skip_countdown_now(app.world_mut());
            1
        }
        Driver::HardBotInTheGrid => {
            let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
            let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
            let tuning = app.world().resource::<BotTuning>().clone();
            app.world_mut()
                .entity_mut(kart)
                .remove::<PlayerKart>()
                .insert(bot(BotDifficulty::Hard, seed, &tuning));
            3
        }
    };
    let walls = wall_collider(&mut app);
    app.insert_resource(Walls(walls))
        .init_resource::<CameraView>()
        .add_systems(
            PostUpdate,
            watch_the_view.after(TransformSystems::Propagate),
        );
    let mut ticks = 0;
    while app
        .world()
        .get::<RaceProgress>(kart)
        .unwrap()
        .lap_times
        .len()
        < laps
    {
        ticks += 1;
        assert!(
            ticks < 150 * TICK_HZ as usize,
            "{driver:?}: the laps never ended"
        );
        match driver {
            Driver::Autopilot => autopilot(&mut app, kart, &layout),
            Driver::HardBotInTheGrid => app.update(),
        }
    }
    app.world().resource::<CameraView>().clone()
}

/// Issue #47: with the walls drawn 1.2 m high (0.8 m before), the chase camera still sees the
/// road, on the circuits of `camera_circuits()`, followed for a lap of the test autopilot and for a race
/// of a Hard bot in a full grid:
/// - its anti-wall (issue #2) never pulls it in: the walls stay under its line of sight, which
///   starts 1.35 m above the road at the kart. Measured: 0 frames, before and after;
/// - a wall never hides the road just ahead (3 to 20 m, on screen) nor the kart. Measured: never,
///   before and after (and still never with walls of 1.6 m);
/// - over the road on screen up to 45 m ahead, the walls hide 2 % of it at most: the road on the
///   other side of the hairpin's island, which the higher walls hide more of (Prairie Pompon:
///   0.5 to 0.6 % before, 1.3 to 1.6 % after, 2.5 % with walls of 1.6 m; Anneau Guimauve: 0 %
///   before, 0.05 to 0.4 % after).
///
/// Issue #108: a kart on a shortcut (Prairie Pompon redrawn has three) has its path ahead, then
/// the road after it.
#[test]
fn the_chase_camera_sees_the_road_over_the_walls() {
    let runs: Vec<(TrackChoice, Driver)> = camera_circuits()
        .into_iter()
        .flat_map(|track| {
            [Driver::Autopilot, Driver::HardBotInTheGrid].map(|driver| (track.clone(), driver))
        })
        .collect();
    let views: Vec<CameraView> = std::thread::scope(|scope| {
        let threads: Vec<_> = runs
            .iter()
            .map(|(track, driver)| scope.spawn(|| watch_a_race(track.clone(), *driver)))
            .collect();
        threads
            .into_iter()
            .map(|thread| {
                thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    let mut failures = Vec::new();
    for ((track, driver), view) in runs.iter().zip(&views) {
        let share = |part: usize, whole: usize| part as f32 / whole.max(1) as f32;
        let hidden = share(view.hidden, view.on_screen);
        println!(
            "{track:?}, {driver:?}: {} frames, anti-wall {}, road ahead hidden {:.2} % (worst \
             frame {:.1} %), just ahead {} of {}, kart hidden {}",
            view.frames,
            view.pulled_in,
            100.0 * hidden,
            100.0 * view.worst,
            view.near_hidden,
            view.near_on_screen,
            view.kart_hidden,
        );
        if share(view.pulled_in, view.frames) > 0.002
            || share(view.near_hidden, view.near_on_screen) > 0.001
            || share(view.kart_hidden, view.frames) > 0.001
            || hidden > 0.02
            || view.near_on_screen < 20 * view.frames
        {
            failures.push(format!("{track:?}, {driver:?}: {view:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// --- Issue #79: the start line, drawn only -------------------------------------------------------

/// Issue #79 acceptance, on every circuit of the game and its mirror, as the game builds them:
/// the start line's band and arch are drawn with the circuit (each of their vertices is one of
/// the circuit's mesh), and none of it is solid, so the physics does not change. A ray down each
/// post (its middle and near its four edges), from above its top to 5 cm above the ground, meets
/// no collider; nor do rays across the banner from post to post (near its bottom, its middle and
/// its top, near its two faces and between them); rays down onto the band meet the road itself,
/// [`LINE_HEIGHT`] under the band, across the road and the curbs.
#[test]
fn the_start_line_is_drawn_and_nothing_of_it_is_solid() {
    use crate::track::start_line::{
        BANNER_BOTTOM, BANNER_HEIGHT, BANNER_THICKNESS, LINE_DEPTH, LINE_HEIGHT, POST_WIDTH,
    };
    use crate::track::water::ground_height;
    use crate::track::{MirrorMode, StartLine};

    let runs: Vec<(TrackChoice, bool)> = crate::admin::tracks::circuits()
        .iter()
        .flat_map(|entry| [false, true].map(|mirror| (entry.choice(), mirror)))
        .collect();
    assert!(runs.len() >= 10);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = runs
            .iter()
            .map(|(track, mirror)| {
                scope.spawn(move || {
                    let mut app = game_app_unloaded(track.clone(), 0);
                    app.insert_resource(MirrorMode(*mirror));
                    single_threaded(&mut app);
                    wait_for_the_player(&mut app);
                    // The circuit file loads asynchronously (ten at once here).
                    for tries in 0.. {
                        let world = app.world();
                        if world.contains_resource::<Race>()
                            && world
                                .get_resource::<TrackLayout>()
                                .is_some_and(|layout| layout.mirrored == *mirror)
                        {
                            break;
                        }
                        assert!(
                            tries < 2000,
                            "{track:?}: the circuit never came: race {}, layout {:?}",
                            world.contains_resource::<Race>(),
                            world.get_resource::<TrackLayout>().map(|l| l.mirrored)
                        );
                        app.update();
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    for _ in 0..3 {
                        app.update();
                    }
                    let world = app.world_mut();
                    let layout = world.resource::<TrackLayout>().clone();
                    assert_eq!(layout.mirrored, *mirror);
                    let name = format!("{}{}", layout.name, if *mirror { " (miroir)" } else { "" });
                    let mut failures = Vec::new();

                    // Drawn: every vertex of the line is one of the circuit's mesh (issue #119:
                    // of its tiles, on an open circuit).
                    let handles: Vec<Handle<Mesh>> = world
                        .query::<(&Name, &Mesh3d)>()
                        .iter(world)
                        .filter(|(name, _)| name.as_str().starts_with("Track:"))
                        .map(|(_, mesh)| mesh.0.clone())
                        .collect();
                    assert!(!handles.is_empty(), "the circuit's mesh");
                    let mut drawn: HashSet<[i64; 3]> = HashSet::new();
                    for handle in &handles {
                        match world
                            .resource::<Assets<Mesh>>()
                            .get(handle)
                            .unwrap()
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                        {
                            Some(VertexAttributeValues::Float32x3(positions)) => {
                                drawn.extend(positions.iter().map(|p| mm(Vec3::from_array(*p))))
                            }
                            other => panic!("positions: {other:?}"),
                        }
                    }
                    // Issue #119: an open circuit's finish line too.
                    for line in StartLine::all(&layout.center) {
                        let mesh = line.mesh(&Default::default(), &layout.lakes);
                        let undrawn = mesh
                            .positions
                            .iter()
                            .filter(|p| !drawn.contains(&mm(Vec3::from_array(**p))))
                            .count();
                        if undrawn > 0 {
                            failures
                                .push(format!("{name}: {undrawn} vertices of the line not drawn"));
                        }

                        // Not solid: rays against every collider of the game.
                        let colliders: Vec<(Collider, GlobalTransform, Option<Surface>)> = world
                            .query::<(&Collider, &GlobalTransform, Option<&Surface>)>()
                            .iter(world)
                            .map(|(c, t, s)| (c.clone(), *t, s.copied()))
                            .collect();
                        let first_hit = |from: Vec3, to: Vec3| {
                            let (direction, length) = ((to - from).normalize(), from.distance(to));
                            colliders
                                .iter()
                                .filter_map(|(collider, transform, surface)| {
                                    let (_, rotation, translation) =
                                        transform.to_scale_rotation_translation();
                                    collider
                                        .cast_ray(
                                            translation,
                                            rotation,
                                            from,
                                            direction,
                                            length,
                                            true,
                                        )
                                        .map(|toi| (toi, *surface))
                                })
                                .min_by(|a, b| a.0.total_cmp(&b.0))
                        };
                        let s = line.sample;
                        let right = s.forward.cross(Vec3::Y).normalize();
                        let back = right.cross(Vec3::Y);
                        let top = s.position.y + line.top();
                        for post in line.posts() {
                            let ground = ground_height(&layout.lakes, post.xz());
                            let edge = 0.45 * POST_WIDTH;
                            for (a, b) in [
                                (0.0, 0.0),
                                (-1.0, -1.0),
                                (-1.0, 1.0),
                                (1.0, -1.0),
                                (1.0, 1.0),
                            ] {
                                let p = post + right * a * edge + back * b * edge;
                                let (from, to) = (p.with_y(top + 1.0), p.with_y(ground + 0.05));
                                if let Some(hit) = first_hit(from, to) {
                                    failures.push(format!(
                                        "{name}: a post is solid at {hit:?} under {from}"
                                    ));
                                }
                            }
                        }
                        let [left, right_post] = line.posts();
                        for height in [0.05, BANNER_HEIGHT / 2.0, BANNER_HEIGHT - 0.05] {
                            for along in [-0.45, 0.0, 0.45] {
                                let shift = Vec3::Y * (BANNER_BOTTOM + height)
                                    + back * along * BANNER_THICKNESS;
                                if let Some(hit) = first_hit(left + shift, right_post + shift) {
                                    failures.push(format!(
                                        "{name}: the banner is solid ({hit:?}) at {height} m"
                                    ));
                                }
                            }
                        }
                        let half = line.half_width();
                        for across in [-0.97, -0.5, 0.0, 0.5, 0.97] {
                            for along in [-0.4, 0.0, 0.4] {
                                let on_band = s.point(across * half, LINE_HEIGHT)
                                    + s.forward * along * LINE_DEPTH;
                                let hit = first_hit(on_band + s.up, on_band - s.up);
                                match hit {
                                    Some((toi, Some(Surface::Road)))
                                        if (toi - 1.0 - LINE_HEIGHT).abs() < 0.002 => {}
                                    other => failures.push(format!(
                                        "{name}: under the band at {on_band}: {other:?}"
                                    )),
                                }
                            }
                        }
                    }
                    failures
                })
            })
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
