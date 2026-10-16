//! Loading a circuit file and building it: meshes, Rapier colliders, gameplay spots, in the colors
//! of its biome (issue #11; its decor comes once its models are there, `dressing.rs`), and its start
//! line (issue #79, `start_line.rs`: drawn only). Every root entity built here is `LevelScoped`:
//! reloading the level (issue #36) removes them all.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages};
use bevy::light::GlobalAmbientLight;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use super::biome::{Palette, Rgb};
use super::data::TrackData;
use super::layout::{TrackLayout, TrackSample};
use super::mesh::{self, GROUND_HEIGHT, MeshData};
use super::start_line::StartLine;
use super::water;
use super::{MirrorMode, PlayerStart, Surface, WALL_RESTITUTION, ramp_transform_on};
use crate::level::LevelScoped;
use bevy::light::NotShadowCaster;

/// Loads `*.track.ron` files as [`TrackData`].
#[derive(Default, TypePath)]
pub struct TrackLoader;

impl AssetLoader for TrackLoader {
    type Asset = TrackData;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _context: &mut LoadContext<'_>,
    ) -> Result<TrackData, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let data = TrackData::from_ron(std::str::from_utf8(&bytes)?)?;
        data.validate()?;
        Ok(data)
    }

    fn extensions(&self) -> &[&str] {
        &["track.ron"]
    }
}

/// The circuit file being loaded.
#[derive(Resource)]
pub struct TrackHandle(pub Handle<TrackData>);

/// On the collider of a trick ramp (GD § 5, issue #13): pressing jump/drift as the kart leaves
/// it starts a trick (`crate::kart`, `trick.rs`).
#[derive(Component, Clone, Copy, Debug)]
pub struct TrickRamp;

/// On the collider of a glide ramp (GD § 10, issue #20), with [`TrickRamp`] (a trick works off
/// it too): leaving it opens the kart's glider (`crate::kart::KartEnvironment`).
#[derive(Component, Clone, Copy, Debug)]
pub struct GlideRamp;

/// A boost pad on the road (GD § 2, issue #13): a wheel touching the ground over it gets the
/// pad's boost (`KartTuning::boost_pad_*`). The entity's `Transform` is the pad itself, local
/// -Z along the track and local Y up from the road; `half_size` is its half width (x) and half
/// length (y).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BoostPad {
    pub half_size: Vec2,
}

/// Size of a boost pad on the road: width across the track, length along it (m).
pub const BOOST_PAD_SIZE: Vec2 = Vec2::new(3.0, 4.0);

/// Where an item box of the circuit file stands (GD § 8): the entity's `Transform` is the box's
/// center, `ITEM_BOX_HEIGHT` above the road. `crate::items` makes it a real box (break, draw,
/// respawn, visual). A double box gives two items.
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemBoxSpot {
    pub double: bool,
}

/// Height of an item box's center above the road (m): a bit above a kart's body center, so that
/// a kart drives through it and a kart flying over it on a jump does not.
pub const ITEM_BOX_HEIGHT: f32 = 0.9;

/// Where a croquette of the circuit file lies (GD § 7): the entity's `Transform` is the
/// croquette's center, `CROQUETTE_HEIGHT` above the road. `crate::croquettes` makes it a real
/// croquette (pick-up, respawn, visual).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CroquetteSpot;

/// Height of a croquette's center above the road (m): about the middle of a kart's body.
pub const CROQUETTE_HEIGHT: f32 = 0.6;

/// The number of an item box or a croquette of the circuit, in the order of the circuit file
/// (boxes and croquettes counted apart): the same on every peer (issue #53, `crate::net`: the
/// host decides who took it).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpotIndex(pub u16);

/// If the circuit file is missing or invalid, say why and fall back to the test ground
/// instead of leaving a blank screen.
pub fn fall_back_when_loading_fails(
    mut commands: Commands,
    handle: Res<TrackHandle>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if let bevy::asset::LoadState::Failed(error) = server.load_state(&handle.0) {
        error!("could not load the circuit, using the test ground instead: {error}");
        commands.remove_resource::<TrackHandle>();
        super::test_ground::build(&mut commands, &mut meshes, &mut materials);
    }
}

/// Builds the circuit once its file is loaded: its mirror image in the mirror mode (issue #19,
/// [`MirrorMode`]).
pub fn build_circuit_when_loaded(
    mut commands: Commands,
    handle: Res<TrackHandle>,
    tracks: Res<Assets<TrackData>>,
    mirror: Option<Res<MirrorMode>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(file) = tracks.get(&handle.0) else {
        return;
    };
    let mirrored;
    let data = if mirror.is_some_and(|mirror| mirror.0) {
        mirrored = file.mirrored();
        &mirrored
    } else {
        file
    };
    let layout = TrackLayout::new(data);
    let palette = data.biome().map_or_else(Palette::default, |b| b.palette);
    let generated = mesh::generate(&layout.center, data, &palette);
    light_the_sky(&mut commands, &palette);

    // One mesh and one material (vertex colors) for the whole circuit: a single draw call. Issue
    // #79: the start line and its arch in it, drawn only (no collider).
    let track_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    let mut drawn = generated.combined();
    drawn.append(&StartLine::of(&layout.center).mesh(&palette, &layout.lakes));
    commands.spawn((
        LevelScoped,
        Name::new(format!("Track: {}", layout.name)),
        Mesh3d(meshes.add(to_mesh(&drawn))),
        MeshMaterial3d(track_material),
        Transform::default(),
    ));

    // Colliders, one per surface. Internal edges are fixed so karts slide along triangle
    // seams without bumps or snags. Floors are one-sided (seen from above), walls two-sided. The
    // walls' collider is their drawn mesh: solid exactly as high as they are drawn (issue #47).
    spawn_trimesh(
        &mut commands,
        &generated.road,
        Surface::Road,
        0.0,
        TriMeshFlags::FIX_INTERNAL_EDGES,
    );
    spawn_trimesh(
        &mut commands,
        &generated.offroad,
        Surface::Offroad,
        0.0,
        TriMeshFlags::FIX_INTERNAL_EDGES,
    );
    spawn_trimesh(
        &mut commands,
        &generated.walls,
        Surface::Wall,
        WALL_RESTITUTION,
        TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED,
    );

    spawn_ground(
        &mut commands,
        &mut meshes,
        &mut materials,
        &layout,
        &palette,
    );
    spawn_trick_ramps(&mut commands, &mut meshes, &mut materials, &layout, data);
    spawn_glide_ramps(&mut commands, &mut meshes, &mut materials, &layout, data);
    spawn_spots(&mut commands, &mut meshes, &mut materials, &layout, data);

    commands.insert_resource(PlayerStart(layout.grid[0]));
    commands.insert_resource(layout);
}

/// The sky (the clear color) and the ambient light of a palette (issue #11): the light in the
/// shade, which keeps the walls' side away from the sun from being almost black (issue #47).
pub fn light_the_sky(commands: &mut Commands, palette: &Palette) {
    let srgb = |(r, g, b): Rgb| Color::srgb(r, g, b);
    commands.insert_resource(ClearColor(srgb(palette.sky)));
    commands.insert_resource(GlobalAmbientLight {
        color: srgb(palette.ambient),
        brightness: palette.ambient_brightness,
        ..default()
    });
}

/// A mesh of [`MeshData`]: flat normals and vertex colors, for the material of the circuit or of
/// the models (white, its colors from the vertices).
pub fn to_mesh(data: &MeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0f32, 0.0]; data.positions.len()],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
    .with_inserted_indices(Indices::U32(data.indices.clone()))
}

fn spawn_trimesh(
    commands: &mut Commands,
    data: &MeshData,
    surface: Surface,
    restitution: f32,
    flags: TriMeshFlags,
) {
    let (vertices, triangles) = data.collider_data();
    match Collider::trimesh_with_flags(vertices, triangles, flags) {
        Ok(collider) => {
            commands.spawn((
                LevelScoped,
                Transform::default(),
                RigidBody::Fixed,
                collider,
                Restitution::coefficient(restitution),
                Friction::coefficient(0.0),
                surface,
            ));
        }
        Err(error) => error!("track collider ({surface:?}) could not be built: {error:?}"),
    }
}

/// Grass (or the biome's ground) all around, just below the road. Issue #20: around the lakes,
/// whose basins it leaves open, each with its floor and its water.
fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
    palette: &Palette,
) {
    let (min, max) = layout.center.samples().iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), s| {
            let p = Vec2::new(s.position.x, s.position.z);
            (lo.min(p), hi.max(p))
        },
    );
    let margin = 80.0;
    let half = (max - min) / 2.0 + Vec2::splat(margin);
    let center = (min + max) / 2.0;
    let srgb = |(r, g, b): Rgb| Color::srgb(r, g, b);
    let ground_material = materials.add(StandardMaterial {
        base_color: srgb(palette.ground),
        perceptual_roughness: 1.0,
        ..default()
    });
    // With lakes, the ground goes down to the deepest floor: its sides are the basins' walls.
    // Without one, a single box, exactly the ground before issue #20 (to the bit: the physics
    // of the circuits without a lake does not change at all).
    let bottom = layout
        .lakes
        .iter()
        .map(|lake| lake.floor)
        .fold(GROUND_HEIGHT - 1.0, f32::min);
    let thickness = GROUND_HEIGHT - bottom;
    let cells = if layout.lakes.is_empty() {
        vec![(center, half)]
    } else {
        water::ground_cells(center - half, center + half, &layout.lakes)
            .into_iter()
            .map(|(lo, hi)| ((lo + hi) / 2.0, (hi - lo) / 2.0))
            .collect()
    };
    for (center, half) in cells {
        let entity = super::spawn_static_box(
            commands,
            Transform::from_xyz(center.x, GROUND_HEIGHT - thickness / 2.0, center.y),
            Vec3::new(half.x, thickness / 2.0, half.y),
            Surface::Offroad,
            0.0,
        );
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::new(half.x * 2.0, thickness, half.y * 2.0))),
            MeshMaterial3d(ground_material.clone()),
        ));
    }
    if layout.lakes.is_empty() {
        return;
    }
    // The basin's floor in the skirts' color (rock), darker than the sand: the water looks
    // deep.
    let floor_material = materials.add(StandardMaterial {
        base_color: srgb(palette.skirt),
        perceptual_roughness: 1.0,
        ..default()
    });
    // The water: one translucent sheet per lake, seen from above and from below (a kart under
    // water sees it overhead). It casts no shadow and has no collider.
    let water_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.10, 0.48, 0.72, 0.62),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.15,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    for lake in &layout.lakes {
        let (center, size) = (lake.center(), lake.size());
        let entity = super::spawn_static_box(
            commands,
            Transform::from_xyz(center.x, lake.floor - 0.5, center.y),
            Vec3::new(size.x / 2.0, 0.5, size.y / 2.0),
            Surface::Offroad,
            0.0,
        );
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::new(size.x, 1.0, size.y))),
            MeshMaterial3d(floor_material.clone()),
        ));
        commands.spawn((
            LevelScoped,
            Name::new("Lake"),
            *lake,
            Mesh3d(meshes.add(Plane3d::new(Vec3::Y, size / 2.0))),
            MeshMaterial3d(water_material.clone()),
            Transform::from_xyz(center.x, lake.level, center.y),
            NotShadowCaster,
        ));
    }
}

/// The glide ramps of the file (issue #20): boxes like the trick ramps (a trick works off them
/// too), sky blue with white chevrons, whose top leads into a gap.
fn spawn_glide_ramps(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
    data: &TrackData,
) {
    if data.glide_ramps.is_empty() {
        return;
    }
    let material = materials.add(Color::srgb(0.35, 0.72, 0.98));
    let stripe_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.5, 0.5, 0.5),
        ..default()
    });
    let stripe_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let half_thickness = 0.5;
    for ramp in &data.glide_ramps {
        let sample = layout.center.at(layout.center.param_to_distance(ramp.at));
        let start = sample.point(ramp.offset * sample.half_width, 0.0);
        let base = sample.transform(0.0, 0.0).rotation;
        let transform = ramp_transform_on(
            start,
            base,
            ramp.length,
            ramp.angle.to_radians(),
            half_thickness,
        );
        let half = Vec3::new(ramp.width / 2.0, half_thickness, ramp.length / 2.0);
        let entity = super::spawn_static_box(commands, transform, half, Surface::Road, 0.0);
        commands
            .entity(entity)
            .insert((
                GlideRamp,
                TrickRamp,
                Name::new("Glide ramp"),
                Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
                MeshMaterial3d(material.clone()),
            ))
            .with_children(|ramp| {
                // Three chevrons pointing up the ramp, and the lip.
                let top = half_thickness + 0.02;
                let mut stripes = vec![(
                    Vec3::new(0.0, top, -half.z + 0.35),
                    0.0,
                    Vec3::new(half.x * 2.0, 0.04, 0.7),
                )];
                for z in [-0.35, 0.3, 0.95] {
                    let z = z * half.z;
                    stripes.push((Vec3::new(-0.9, top, z), -0.7, Vec3::new(0.4, 0.04, 2.6)));
                    stripes.push((Vec3::new(0.9, top, z), 0.7, Vec3::new(0.4, 0.04, 2.6)));
                }
                for (at, yaw, size) in stripes {
                    ramp.spawn((
                        Mesh3d(stripe_mesh.clone()),
                        MeshMaterial3d(stripe_material.clone()),
                        Transform::from_translation(at)
                            .with_rotation(Quat::from_rotation_y(yaw))
                            .with_scale(size),
                    ));
                }
            });
    }
}

fn spawn_trick_ramps(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
    data: &TrackData,
) {
    let material = materials.add(Color::srgb(0.98, 0.72, 0.25));
    // White stripes mark a trick ramp and its lip, where to press the jump/drift button.
    let stripe_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.5, 0.5, 0.5),
        ..default()
    });
    let stripe_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let half_thickness = 0.5;
    for ramp in &data.trick_ramps {
        let sample = layout.center.at(layout.center.param_to_distance(ramp.at));
        let start = sample.point(ramp.offset * sample.half_width, 0.0);
        let base = sample.transform(0.0, 0.0).rotation;
        let transform = ramp_transform_on(
            start,
            base,
            ramp.length,
            ramp.angle.to_radians(),
            half_thickness,
        );
        let half = Vec3::new(ramp.width / 2.0, half_thickness, ramp.length / 2.0);
        let entity = super::spawn_static_box(commands, transform, half, Surface::Road, 0.0);
        commands
            .entity(entity)
            .insert((
                TrickRamp,
                Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
                MeshMaterial3d(material.clone()),
            ))
            .with_children(|ramp| {
                // The lip (top end, local -Z) and two chevrons pointing up the ramp.
                let top = half_thickness + 0.02;
                let stripes = [
                    (
                        Vec3::new(0.0, top, -half.z + 0.35),
                        0.0,
                        Vec3::new(half.x * 2.0, 0.04, 0.7),
                    ),
                    (Vec3::new(-0.62, top, 0.6), -0.6, Vec3::new(0.35, 0.04, 2.2)),
                    (Vec3::new(0.62, top, 0.6), 0.6, Vec3::new(0.35, 0.04, 2.2)),
                ];
                for (at, yaw, size) in stripes {
                    ramp.spawn((
                        Mesh3d(stripe_mesh.clone()),
                        MeshMaterial3d(stripe_material.clone()),
                        Transform::from_translation(at)
                            .with_rotation(Quat::from_rotation_y(yaw))
                            .with_scale(size),
                    ));
                }
            });
    }
}

/// The gameplay spots of the file. Boost pads are active (issue #13, the kart reads
/// [`BoostPad`]), croquettes too (issue #14, `crate::croquettes`), and the item boxes (issue #16,
/// `crate::items`, which also dresses them).
fn spawn_spots(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
    data: &TrackData,
) {
    let center = &layout.center;
    let at = |param: f32| center.at(center.param_to_distance(param));
    let place = |sample: &TrackSample, offset: f32, height: f32| {
        sample.transform(offset * sample.half_width, height)
    };

    let pad_mesh = meshes.add(Cuboid::new(BOOST_PAD_SIZE.x, 0.04, BOOST_PAD_SIZE.y));
    let pad_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.55, 0.1),
        emissive: LinearRgba::rgb(0.6, 0.25, 0.0),
        ..default()
    });
    // Two yellow chevrons on each pad, pointing down the track.
    let chevron_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let chevron_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.4),
        emissive: LinearRgba::rgb(0.9, 0.8, 0.2),
        ..default()
    });
    for pad in &data.boost_pads {
        commands
            .spawn((
                LevelScoped,
                BoostPad {
                    half_size: BOOST_PAD_SIZE / 2.0,
                },
                Mesh3d(pad_mesh.clone()),
                MeshMaterial3d(pad_material.clone()),
                place(&at(pad.at), pad.offset, 0.03),
            ))
            .with_children(|pad| {
                for z in [-0.9, 0.7] {
                    for side in [-1.0f32, 1.0] {
                        pad.spawn((
                            Mesh3d(chevron_mesh.clone()),
                            MeshMaterial3d(chevron_material.clone()),
                            Transform::from_xyz(side * 0.45, 0.03, z + 0.35)
                                .with_rotation(Quat::from_rotation_y(side * 0.8))
                                .with_scale(Vec3::new(0.3, 0.02, 1.3)),
                        ));
                    }
                }
            });
    }

    let mut index = 0u16;
    for row in &data.item_boxes {
        let sample = at(row.at);
        for offset in &row.offsets {
            index += 1;
            commands.spawn((
                SpotIndex(index - 1),
                LevelScoped,
                Name::new(if row.double {
                    "Double item box"
                } else {
                    "Item box"
                }),
                ItemBoxSpot { double: row.double },
                place(&sample, *offset, ITEM_BOX_HEIGHT),
                Visibility::default(),
            ));
        }
    }

    let mut index = 0u16;
    for line in &data.croquettes {
        let start = center.param_to_distance(line.from);
        for i in 0..line.count {
            let sample = center.at(start + i as f32 * line.spacing);
            index += 1;
            commands.spawn((
                SpotIndex(index - 1),
                LevelScoped,
                Name::new("Croquette"),
                CroquetteSpot,
                place(&sample, line.offset, CROQUETTE_HEIGHT),
                Visibility::default(),
            ));
        }
    }
}
