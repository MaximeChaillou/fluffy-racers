//! Issue #101: the lobby's scene, a clearing of the meadow (the Prairie biome's colors and
//! models): a round lawn closed by a ring of trees, a patch of sand in the middle where the
//! characters gather, **three giant donuts to bounce on** (`walk::Bouncer`), a birthday cake to
//! climb, cupcakes, rocks, benches and checkered flags. Its own level: `TrackChoice::Salon`
//! (asked by `net` once seated in a room); a race's `LoadLevel` takes it all away.
//!
//! Its colliders are there at once (headless too); its models come once baked (with a
//! renderer), merged into one mesh in the models' shared material (`track::dressing`'s way):
//! one draw call for the whole decor. Only models already in the game's packs.

use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy_rapier3d::prelude::{CoefficientCombineRule, Collider, Friction, Restitution, RigidBody};

use super::walk::Bouncer;
use crate::level::LevelScoped;
use crate::models::{Keep, ModelMaterials, Models};
use crate::track::biome::{Palette, biome};
use crate::track::circuit::to_mesh;
use crate::track::dressing::stand_in_tree;
use crate::track::mesh::MeshData;
use crate::track::{GRID_SLOTS, TrackChoice};

/// On the lawn of the lobby: the scene is up.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SalonRoot;

/// On the lobby's decor, once dressed: how many pieces, how many in primitives (a missing
/// model).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SalonDecor {
    pub pieces: usize,
    pub stand_ins: usize,
}

/// The lawn's radius (m), and the invisible fence that keeps the characters in (its radius, its
/// height).
const LAWN_RADIUS: f32 = 26.0;
const FENCE_RADIUS: f32 = 19.5;
const FENCE_HEIGHT: f32 = 4.0;
const FENCE_PIECES: usize = 32;
/// The sand in the middle.
const SAND_RADIUS: f32 = 6.5;
/// The characters start on a circle this wide around the middle, facing it.
const SEAT_RADIUS: f32 = 4.5;

/// What stands where, and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Solid {
    /// Only seen.
    None,
    /// A round post (radius, top): it stops the characters, they can climb on it.
    Post(f32, f32),
    /// A round bouncer (radius, top): it sends whoever lands on it up.
    Bouncer(f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Prop {
    model: &'static str,
    at: (f32, f32),
    yaw: f32,
    height: f32,
    radius: f32,
    solid: Solid,
    shadows: bool,
}

const fn prop(model: &'static str, at: (f32, f32), height: f32, radius: f32, solid: Solid) -> Prop {
    Prop {
        model,
        at,
        yaw: 0.0,
        height,
        radius,
        solid,
        shadows: true,
    }
}

/// The pieces of the clearing, by hand (the camera looks toward -Z: the donuts in front).
const PROPS: [Prop; 15] = [
    prop(
        "food-kit/donut-sprinkles",
        (-8.0, -5.0),
        1.1,
        2.6,
        Solid::Bouncer(2.3, 1.0),
    ),
    prop(
        "food-kit/donut-chocolate",
        (8.5, -4.5),
        1.1,
        2.6,
        Solid::Bouncer(2.3, 1.0),
    ),
    prop(
        "food-kit/donut-sprinkles",
        (0.5, -12.0),
        1.1,
        2.6,
        Solid::Bouncer(2.3, 1.0),
    ),
    prop(
        "food-kit/cake-birthday",
        (-12.0, 5.5),
        2.2,
        2.8,
        Solid::Post(2.4, 2.0),
    ),
    prop(
        "food-kit/cupcake",
        (12.0, 4.0),
        1.6,
        1.4,
        Solid::Post(1.1, 1.4),
    ),
    prop(
        "food-kit/cupcake",
        (-3.5, -16.0),
        1.4,
        1.2,
        Solid::Post(1.0, 1.2),
    ),
    prop(
        "nature-kit/rock_largeA",
        (6.0, 11.0),
        1.5,
        1.8,
        Solid::Post(1.4, 1.3),
    ),
    prop(
        "nature-kit/rock_largeB",
        (-6.5, 12.5),
        1.2,
        1.6,
        Solid::Post(1.3, 1.0),
    ),
    prop(
        "nature-kit/stump_round",
        (14.5, -7.0),
        0.7,
        0.8,
        Solid::Post(0.6, 0.6),
    ),
    Prop {
        yaw: 0.5,
        ..prop(
            "holiday-kit/bench",
            (-3.0, 7.5),
            0.9,
            1.3,
            Solid::Post(0.6, 0.5),
        )
    },
    Prop {
        yaw: -0.6,
        ..prop(
            "holiday-kit/bench",
            (4.0, 7.0),
            0.9,
            1.3,
            Solid::Post(0.6, 0.5),
        )
    },
    prop(
        "racing-kit/flagCheckers",
        (-4.5, -19.0),
        4.0,
        1.0,
        Solid::None,
    ),
    prop(
        "racing-kit/flagCheckers",
        (5.5, -19.0),
        4.0,
        1.0,
        Solid::None,
    ),
    prop(
        "racing-kit/bannerTowerRed",
        (-15.0, -12.0),
        5.0,
        1.4,
        Solid::None,
    ),
    prop(
        "racing-kit/bannerTowerGreen",
        (15.5, 11.0),
        5.0,
        1.4,
        Solid::None,
    ),
];

/// The trees around (outside the fence), and the bushes and flowers by it (no shadows).
const TREES: [&str; 4] = [
    "nature-kit/tree_oak",
    "nature-kit/tree_default",
    "nature-kit/tree_fat",
    "nature-kit/tree_detailed",
];
const TREE_COUNT: usize = 16;
const BUSHES: [&str; 2] = ["nature-kit/plant_bushLarge", "nature-kit/plant_bush"];
const BUSH_COUNT: usize = 10;
const FLOWERS: [&str; 3] = [
    "nature-kit/flower_redA",
    "nature-kit/flower_yellowA",
    "nature-kit/flower_purpleA",
];
const FLOWER_COUNT: usize = 24;

/// Every model of the lobby (the models' test checks they are all in `assets/models/`, with
/// their credits).
pub const SALON_MODELS: [&str; 20] = [
    "food-kit/donut-sprinkles",
    "food-kit/donut-chocolate",
    "food-kit/cake-birthday",
    "food-kit/cupcake",
    "nature-kit/rock_largeA",
    "nature-kit/rock_largeB",
    "nature-kit/stump_round",
    "holiday-kit/bench",
    "racing-kit/flagCheckers",
    "racing-kit/bannerTowerRed",
    "racing-kit/bannerTowerGreen",
    TREES[0],
    TREES[1],
    TREES[2],
    TREES[3],
    BUSHES[0],
    BUSHES[1],
    FLOWERS[0],
    FLOWERS[1],
    FLOWERS[2],
];

/// A number from `seed` and `index`, 0 to 1: the same clearing every time.
fn draw(seed: u32, index: usize) -> f32 {
    let mut x = (seed ^ (index as u32).wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

/// Where the character of seat `seat` starts (its body's bottom on the lawn), facing the middle.
pub fn seat_spot(seat: usize) -> (Vec3, f32) {
    let angle = std::f32::consts::FRAC_PI_2
        + seat.min(GRID_SLOTS - 1) as f32 * std::f32::consts::TAU / GRID_SLOTS as f32;
    let at = Vec3::new(angle.cos(), 0.0, angle.sin()) * SEAT_RADIUS;
    let heading = super::walk::heading_toward(-at).unwrap_or(0.0);
    (at, heading)
}

/// Each piece to dress the clearing with: (model, position, yaw, height, radius, shadows).
fn pieces() -> Vec<(&'static str, Vec3, f32, f32, f32, bool)> {
    let mut all: Vec<(&'static str, Vec3, f32, f32, f32, bool)> = PROPS
        .iter()
        .map(|p| {
            (
                p.model,
                Vec3::new(p.at.0, 0.0, p.at.1),
                p.yaw,
                p.height,
                p.radius,
                p.shadows,
            )
        })
        .collect();
    let ring = |count: usize, radius: (f32, f32), seed: u32| {
        (0..count).map(move |i| {
            let angle = (i as f32 + draw(seed, i) * 0.6) * std::f32::consts::TAU / count as f32;
            let r = radius.0 + draw(seed + 1, i) * (radius.1 - radius.0);
            (
                Vec3::new(angle.cos() * r, 0.0, angle.sin() * r),
                draw(seed + 2, i),
            )
        })
    };
    for (i, (at, t)) in ring(TREE_COUNT, (21.5, 24.0), 11).enumerate() {
        all.push((
            TREES[i % TREES.len()],
            at,
            t * 6.0,
            6.0 + 2.5 * t,
            2.6,
            true,
        ));
    }
    for (i, (at, t)) in ring(BUSH_COUNT, (18.0, 19.0), 23).enumerate() {
        all.push((
            BUSHES[i % BUSHES.len()],
            at,
            t * 6.0,
            1.0 + 0.4 * t,
            1.1,
            false,
        ));
    }
    for (i, (at, t)) in ring(FLOWER_COUNT, (7.5, 17.0), 37).enumerate() {
        all.push((FLOWERS[i % FLOWERS.len()], at, t * 6.0, 0.6, 0.35, false));
    }
    all
}

/// The lobby is asked (its level) and not built yet.
pub fn salon_wanted(choice: Res<TrackChoice>, built: Query<(), With<SalonRoot>>) -> bool {
    *choice == TrackChoice::Salon && built.is_empty()
}

/// The lobby's scene is up.
pub fn salon_up(built: Query<(), With<SalonRoot>>) -> bool {
    !built.is_empty()
}

/// The models can be loaded (a renderer) and the decor is not there yet.
pub fn can_dress(
    gltfs: Option<Res<Assets<bevy::gltf::Gltf>>>,
    dressed: Query<(), With<SalonDecor>>,
) -> bool {
    gltfs.is_some() && dressed.is_empty()
}

fn srgb((r, g, b): (f32, f32, f32)) -> Color {
    Color::srgb(r, g, b)
}

/// The lobby's palette: the meadow's.
fn palette() -> Palette {
    biome("Prairie").map_or_else(Palette::default, |biome| biome.palette)
}

/// Builds the clearing: its lawn and sand, its fence, its pieces' colliders, the meadow's sky
/// and light.
pub fn build_the_salon(
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let palette = palette();
    commands.insert_resource(ClearColor(srgb(palette.sky)));
    commands.insert_resource(GlobalAmbientLight {
        color: srgb(palette.ambient),
        brightness: palette.ambient_brightness,
        ..default()
    });
    let ground = (
        RigidBody::Fixed,
        Friction {
            coefficient: 0.0,
            combine_rule: CoefficientCombineRule::Min,
        },
        Restitution::coefficient(0.3),
    );
    let mut lawn = commands.spawn((
        Name::new("Salon"),
        SalonRoot,
        LevelScoped,
        Transform::from_xyz(0.0, -0.25, 0.0),
        Visibility::default(),
        Collider::cylinder(0.25, LAWN_RADIUS),
        ground,
    ));
    if let (Some(mut meshes), Some(mut materials)) = (meshes, materials) {
        let grass = materials.add(StandardMaterial {
            base_color: srgb(palette.ground),
            perceptual_roughness: 0.95,
            ..default()
        });
        let sand = materials.add(StandardMaterial {
            base_color: srgb(palette.sand),
            perceptual_roughness: 0.95,
            ..default()
        });
        let lawn_mesh = meshes.add(Cylinder::new(LAWN_RADIUS, 0.5).mesh().resolution(48));
        let sand_mesh = meshes.add(Cylinder::new(SAND_RADIUS, 0.04).mesh().resolution(40));
        lawn.with_children(|lawn| {
            lawn.spawn((Mesh3d(lawn_mesh), MeshMaterial3d(grass)));
            lawn.spawn((
                Mesh3d(sand_mesh),
                MeshMaterial3d(sand),
                Transform::from_xyz(0.0, 0.25, 0.0),
            ));
        });
    }
    // The fence: invisible boards in a ring (the characters bounce off it).
    for i in 0..FENCE_PIECES {
        let angle = i as f32 * std::f32::consts::TAU / FENCE_PIECES as f32;
        let at = Vec3::new(angle.cos(), 0.0, angle.sin()) * (FENCE_RADIUS + 0.25);
        let width = FENCE_RADIUS * std::f32::consts::TAU / FENCE_PIECES as f32 * 0.6 + 0.2;
        commands.spawn((
            Name::new("Salon: clôture"),
            LevelScoped,
            Transform::from_translation(at + Vec3::Y * FENCE_HEIGHT / 2.0)
                .looking_to(-at.normalize_or(Vec3::Z), Vec3::Y),
            Collider::cuboid(width, FENCE_HEIGHT / 2.0, 0.25),
            ground,
        ));
    }
    for prop in &PROPS {
        let center = Vec2::new(prop.at.0, prop.at.1);
        let (radius, top) = match prop.solid {
            Solid::None => continue,
            Solid::Post(radius, top) | Solid::Bouncer(radius, top) => (radius, top),
        };
        let mut piece = commands.spawn((
            Name::new(format!("Salon: {}", prop.model)),
            LevelScoped,
            Transform::from_xyz(center.x, top / 2.0, center.y),
            Collider::cylinder(top / 2.0, radius),
            ground,
        ));
        if let Solid::Bouncer(radius, top) = prop.solid {
            piece.insert(Bouncer {
                center,
                radius,
                top,
            });
        }
    }
}

/// Once the models are baked (or missing): the clearing's pieces in one mesh (a missing model
/// stands in a primitive tree).
pub fn dress_the_salon(
    mut commands: Commands,
    mut models: ResMut<Models>,
    server: Res<AssetServer>,
    shared: Option<Res<ModelMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    cameras: Query<Entity, With<crate::camera::ChaseCamera>>,
    mut asked: Local<bool>,
) {
    let Some(shared) = shared else {
        return;
    };
    let pieces = pieces();
    if !*asked {
        for (model, ..) in &pieces {
            models.request(model, Keep::Level, Some(&server));
        }
        *asked = true;
    }
    if !pieces.iter().all(|(model, ..)| models.settled(model)) {
        return;
    }
    *asked = false;
    // Issue #146: no far decor here, nor the fog of the circuit before (`track::dressing`).
    for camera in &cameras {
        commands.entity(camera).remove::<DistanceFog>();
    }
    let stand_in = stand_in_tree();
    let (mut casting, mut small) = (MeshData::default(), MeshData::default());
    let mut stand_ins = 0;
    for (model, at, yaw, height, radius, shadows) in &pieces {
        let baked = match models.get(model) {
            Some(baked) => baked.as_ref(),
            None => {
                stand_ins += 1;
                &stand_in
            }
        };
        let out = if *shadows { &mut casting } else { &mut small };
        baked.place(out, *at, *yaw, *height, *radius);
    }
    info!(
        "lobby: its decor, {} pieces ({stand_ins} in primitives)",
        pieces.len()
    );
    commands
        .spawn((
            Name::new("Salon: décor"),
            SalonDecor {
                pieces: pieces.len(),
                stand_ins,
            },
            LevelScoped,
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|decor| {
            if !casting.indices.is_empty() {
                decor.spawn((
                    Mesh3d(meshes.add(to_mesh(&casting))),
                    MeshMaterial3d(shared.vertex_colors.clone()),
                ));
            }
            if !small.indices.is_empty() {
                decor.spawn((
                    Mesh3d(meshes.add(to_mesh(&small))),
                    MeshMaterial3d(shared.vertex_colors.clone()),
                    bevy::light::NotShadowCaster,
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seats start on a circle around the middle, facing it, each its own spot; every
    /// piece stands on the lawn, the solid ones inside the fence and apart from the seats; every
    /// model it names is listed.
    #[test]
    fn the_clearing_has_room_for_everyone() {
        let mut spots: Vec<Vec3> = Vec::new();
        for seat in 0..GRID_SLOTS {
            let (at, heading) = seat_spot(seat);
            assert!((at.length() - SEAT_RADIUS).abs() < 1e-4);
            let front = super::super::walk::front_of(heading);
            assert!(front.dot(-at.normalize()) > 0.999, "faces the middle");
            assert!(spots.iter().all(|s| s.distance(at) > 2.0), "seat {seat}");
            spots.push(at);
        }
        for prop in &PROPS {
            let at = Vec3::new(prop.at.0, 0.0, prop.at.1);
            assert!(at.length() + prop.radius < LAWN_RADIUS, "{}", prop.model);
            if let Solid::Post(radius, _) | Solid::Bouncer(radius, _) = prop.solid {
                assert!(at.length() + radius < FENCE_RADIUS, "{}", prop.model);
                assert!(
                    spots.iter().all(|s| s.distance(at) > radius + 1.0),
                    "{} on a seat",
                    prop.model
                );
            }
        }
        for (model, at, ..) in pieces() {
            assert!(SALON_MODELS.contains(&model), "{model}");
            assert!(at.length() < LAWN_RADIUS, "{model}");
        }
        let bouncers = PROPS
            .iter()
            .filter(|p| matches!(p.solid, Solid::Bouncer(..)))
            .count();
        assert_eq!(bouncers, 3, "three giant donuts");
    }
}
