//! Loading a circuit file and building it: meshes, Rapier colliders, gameplay spots, decor.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use super::data::TrackData;
use super::layout::{TrackLayout, TrackSample};
use super::mesh::{self, CURB_WIDTH, GROUND_HEIGHT, MeshData, WALL_THICKNESS};
use super::{PlayerStart, Surface, WALL_RESTITUTION, ramp_transform_on};

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

/// Where an item box goes (GD § 8). Not active yet (issue #16).
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemBoxSpot {
    pub double: bool,
}

/// Where a croquette goes (GD § 7). Not active yet (issue #14).
#[derive(Component, Clone, Copy, Debug)]
pub struct CroquetteSpot;

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

/// Builds the circuit once its file is loaded.
pub fn build_circuit_when_loaded(
    mut commands: Commands,
    handle: Res<TrackHandle>,
    tracks: Res<Assets<TrackData>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(data) = tracks.get(&handle.0) else {
        return;
    };
    let layout = TrackLayout::new(data);
    let generated = mesh::generate(&layout.center, data);

    // One mesh and one material (vertex colors) for the whole circuit: a single draw call.
    let track_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Name::new(format!("Track: {}", layout.name)),
        Mesh3d(meshes.add(to_mesh(&generated.combined()))),
        MeshMaterial3d(track_material),
        Transform::default(),
    ));

    // Colliders, one per surface. Internal edges are fixed so karts slide along triangle
    // seams without bumps or snags. Floors are one-sided (seen from above), walls two-sided.
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
        Surface::Road,
        WALL_RESTITUTION,
        TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED,
    );

    spawn_ground(&mut commands, &mut meshes, &mut materials, &layout);
    spawn_trick_ramps(&mut commands, &mut meshes, &mut materials, &layout, data);
    spawn_spots(&mut commands, &mut meshes, &mut materials, &layout, data);
    spawn_trees(&mut commands, &mut meshes, &mut materials, &layout);

    commands.insert_resource(PlayerStart(layout.grid[0]));
    commands.insert_resource(layout);
}

fn to_mesh(data: &MeshData) -> Mesh {
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

/// Grass all around, just below the road.
fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
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
    let entity = super::spawn_static_box(
        commands,
        Transform::from_xyz(center.x, GROUND_HEIGHT - 0.5, center.y),
        Vec3::new(half.x, 0.5, half.y),
        Surface::Offroad,
        0.0,
    );
    commands.entity(entity).insert((
        Mesh3d(meshes.add(Cuboid::new(half.x * 2.0, 1.0, half.y * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.75, 0.35),
            perceptual_roughness: 1.0,
            ..default()
        })),
    ));
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
/// [`BoostPad`]); item boxes and croquettes are placeholders that do nothing yet (#16, #14).
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

    let box_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let box_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.8, 1.0, 0.6),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let double_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.85, 0.3, 0.6),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    for row in &data.item_boxes {
        let sample = at(row.at);
        for offset in &row.offsets {
            let material = if row.double {
                &double_material
            } else {
                &box_material
            };
            commands.spawn((
                ItemBoxSpot { double: row.double },
                Mesh3d(box_mesh.clone()),
                MeshMaterial3d(material.clone()),
                place(&sample, *offset, 1.0)
                    .with_rotation(Quat::from_rotation_y(0.6) * Quat::from_rotation_x(0.6)),
            ));
        }
    }

    let croquette_mesh = meshes.add(Sphere::new(0.3));
    let croquette_material = materials.add(Color::srgb(0.75, 0.45, 0.2));
    for line in &data.croquettes {
        let start = center.param_to_distance(line.from);
        for i in 0..line.count {
            let sample = center.at(start + i as f32 * line.spacing);
            commands.spawn((
                CroquetteSpot,
                Mesh3d(croquette_mesh.clone()),
                MeshMaterial3d(croquette_material.clone()),
                place(&sample, line.offset, 0.6),
            ));
        }
    }
}

/// A few primitive trees around the circuit, away from the road (deterministic).
fn spawn_trees(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
) {
    let trunk_mesh = meshes.add(Cylinder::new(0.3, 2.4));
    let trunk_material = materials.add(Color::srgb(0.50, 0.33, 0.20));
    let leaves_mesh = meshes.add(Sphere::new(1.8));
    let leaves_material = materials.add(Color::srgb(0.25, 0.60, 0.30));

    let samples = layout.center.samples();
    let mut seed: u32 = 0x9e37_79b9;
    let mut random = move || {
        // xorshift: cheap, deterministic, good enough for decor.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut planted = 0;
    for _ in 0..200 {
        if planted >= 60 {
            break;
        }
        let sample = &samples[(random() * samples.len() as f32) as usize % samples.len()];
        let side = if random() < 0.5 { -1.0 } else { 1.0 };
        let flat_right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let reach = mesh::wall_offset(sample) + WALL_THICKNESS + 4.0 + random() * 20.0;
        let mut position = sample.position + flat_right * side * reach;
        position.y = GROUND_HEIGHT;
        // Keep clear of every part of the circuit (the loop passes close to itself).
        let clear = samples.iter().all(|s| {
            let keep_out = s.half_width + CURB_WIDTH + s.shoulder + WALL_THICKNESS + 3.0;
            Vec2::new(s.position.x - position.x, s.position.z - position.z).length() > keep_out
        });
        if !clear {
            continue;
        }
        planted += 1;
        commands
            .spawn((Transform::from_translation(position), Visibility::default()))
            .with_children(|tree| {
                tree.spawn((
                    Mesh3d(trunk_mesh.clone()),
                    MeshMaterial3d(trunk_material.clone()),
                    Transform::from_xyz(0.0, 1.2, 0.0),
                ));
                tree.spawn((
                    Mesh3d(leaves_mesh.clone()),
                    MeshMaterial3d(leaves_material.clone()),
                    Transform::from_xyz(0.0, 3.4, 0.0),
                ));
            });
    }
}
