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

use super::biome::{Palette, Rgb, RoadLook};
use super::data::TrackData;
use super::layout::{TrackLayout, TrackSample};
use super::mesh::{self, GROUND_HEIGHT, MeshData, PaintedMesh};
use super::paving;
use super::start_line::StartLine;
use super::water;
use super::{
    AntiGravity, MirrorMode, NoItemBoxes, PlayerStart, Surface, WALL_RESTITUTION, anti_gravity_at,
    ramp_transform_on,
};
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

/// Issue #180: on the collider of the faces of the drops (a wall to the karts, apart from the low
/// walls).
#[derive(Component, Clone, Copy, Debug)]
pub struct DropFace;

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

/// Issue #119: an open circuit's meshes (its road and walls, its decor) are drawn in square tiles
/// this wide (m) on the ground plane, each culled on its own: a few draw calls more, and only
/// what the camera may see is drawn. (Its colliders stay whole: Rapier's trimeshes have their own
/// tree, and a seam between two of them would bump the wheels.)
pub const TILE_SIZE: f32 = 200.0;

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
/// [`MirrorMode`]); issue #99: without its item boxes when the items are off ([`NoItemBoxes`]).
#[allow(clippy::too_many_arguments)]
pub fn build_circuit_when_loaded(
    mut commands: Commands,
    handle: Res<TrackHandle>,
    tracks: Res<Assets<TrackData>>,
    mirror: Option<Res<MirrorMode>>,
    no_boxes: Option<Res<NoItemBoxes>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Issue #187: with a renderer's storage of images (never headless: the bot races' world has
    // none), the road drawn with its paving and its borders; the materials of the last few
    // roads' looks kept for the next circuits of the same looks.
    mut images: Option<ResMut<Assets<Image>>>,
    mut road_materials: Local<Vec<RoadMaterials>>,
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
    // Issue #187: its biome's colors, its own road's in them.
    let palette = data.palette();
    let generated = mesh::generate(&layout.center, data, &palette);
    light_the_sky(&mut commands, &palette);

    // One mesh and one material (vertex colors) for the whole circuit: a single draw call. Issue
    // #79: the start line and its arch in it, drawn only (no collider).
    let track_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    // Issue #187: with a renderer, the road's paving and its borders apart, with their
    // textures; the rest in the one mesh. Headless, everything in the one mesh, as before.
    let painted = images.as_deref_mut().map(|images| {
        let look = data.road_look();
        let road = RoadMaterials::of(&mut road_materials, images, &mut materials, look, &palette);
        (generated.drawn(), road)
    });
    let mut drawn = match &painted {
        Some((parts, _)) => parts.plain.clone(),
        None => generated.combined(),
    };
    // Issue #119: and an open circuit's finish line.
    for line in StartLine::all(&layout.center) {
        drawn.append(&line.mesh(&palette, &layout.lakes));
    }
    // Issue #119: a long open circuit in tiles, each culled on its own (`TILE_SIZE`); a circuit
    // of laps in one mesh, as before.
    let open = layout.open().is_some();
    let tiles = if open {
        drawn.tiles(TILE_SIZE)
    } else {
        vec![drawn]
    };
    for tile in &tiles {
        commands.spawn((
            LevelScoped,
            Name::new(format!("Track: {}", layout.name)),
            Mesh3d(meshes.add(to_mesh(tile))),
            MeshMaterial3d(track_material.clone()),
            Transform::default(),
        ));
    }
    if let Some((parts, road)) = &painted {
        for (what, part, material) in [
            ("paving", &parts.paving, &road.paving),
            ("borders", &parts.borders, &road.borders),
        ] {
            if part.is_empty() {
                continue;
            }
            let tiles = if open {
                part.tiles(TILE_SIZE)
            } else {
                vec![part.clone()]
            };
            for tile in &tiles {
                commands.spawn((
                    LevelScoped,
                    Name::new(format!("Track {what}: {}", layout.name)),
                    Mesh3d(meshes.add(to_painted_mesh(tile))),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                ));
            }
        }
    }

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
    // Issue #78: the road and the shoulders of the anti-gravity zones, marked: a wheel on them
    // puts the kart in anti-gravity. Only on a circuit that has a zone (nothing new elsewhere: the
    // same archetypes as before, so the same bot races).
    for (mesh, surface) in [
        (&generated.anti_gravity_road, Surface::Road),
        (&generated.anti_gravity_offroad, Surface::Offroad),
    ] {
        if mesh.indices.is_empty() {
            continue;
        }
        if let Some(entity) = spawn_trimesh(
            &mut commands,
            mesh,
            surface,
            0.0,
            TriMeshFlags::FIX_INTERNAL_EDGES,
        ) {
            commands.entity(entity).insert(AntiGravity);
        }
    }

    // Issue #107: the shortcuts' grass paths (issue #193: every shortcut is grass), only on a
    // circuit that has them (the same archetypes as the road's: nothing new elsewhere).
    if !generated.shortcut_grass.indices.is_empty() {
        spawn_trimesh(
            &mut commands,
            &generated.shortcut_grass,
            Surface::Offroad,
            0.0,
            TriMeshFlags::FIX_INTERNAL_EDGES,
        );
    }

    // Issue #180: the faces of the drops, solid like a wall (two-sided), only on a circuit that
    // has some.
    if !generated.cliffs.indices.is_empty()
        && let Some(entity) = spawn_trimesh(
            &mut commands,
            &generated.cliffs,
            Surface::Wall,
            WALL_RESTITUTION,
            TriMeshFlags::FIX_INTERNAL_EDGES_TWO_SIDED,
        )
    {
        commands.entity(entity).insert(DropFace);
    }
    // Issue #180: the trick stretches of the bumps and the drops (their road and shoulders),
    // marked as trick ramps (GD § 5), only on a circuit that has some (nothing new elsewhere).
    for (mesh, surface) in [
        (&generated.trick_road, Surface::Road),
        (&generated.trick_offroad, Surface::Offroad),
    ] {
        if mesh.indices.is_empty() {
            continue;
        }
        if let Some(entity) = spawn_trimesh(
            &mut commands,
            mesh,
            surface,
            0.0,
            TriMeshFlags::FIX_INTERNAL_EDGES,
        ) {
            commands.entity(entity).insert(TrickRamp);
        }
    }

    spawn_ground(
        &mut commands,
        &mut meshes,
        &mut materials,
        &layout,
        &palette,
    );
    spawn_trick_ramps(
        &mut commands,
        &mut meshes,
        &mut materials,
        &layout,
        data,
        &palette,
    );
    spawn_glide_ramps(&mut commands, &mut meshes, &mut materials, &layout, data);
    let item_boxes = no_boxes.is_none();
    spawn_spots(
        &mut commands,
        (&mut *meshes, &mut *materials),
        &layout,
        data,
        item_boxes,
    );

    // Issue #144: the obstacles, the passers-by and the things to drive through, and their
    // systems, only on a circuit that has some (nothing new elsewhere).
    super::hazards::spawn_hazards(&mut commands, &layout);

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

/// Issue #187: a mesh of [`PaintedMesh`]: flat normals, vertex colors and its UVs, for the
/// materials of the road's paving and borders.
pub fn to_painted_mesh(data: &PaintedMesh) -> Mesh {
    let mesh = &data.mesh;
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, mesh.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, mesh.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, mesh.colors.clone())
    .with_inserted_indices(Indices::U32(mesh.indices.clone()))
}

/// Issue #187: the materials of a road's look: its paving's and its borders', each with its
/// texture (`super::paving`, made here), shared by every mesh of the road. Those of the last
/// [`KEPT_ROADS`] looks are kept for the next circuits that have them (their textures are not made
/// again: the menu's background races go from circuit to circuit).
pub struct RoadMaterials {
    look: RoadLook,
    asphalt: Rgb,
    curbs: (Rgb, Rgb),
    pub paving: Handle<StandardMaterial>,
    pub borders: Handle<StandardMaterial>,
}

impl RoadMaterials {
    /// Those of `look` in `palette`'s colors: the ones kept if they are, else made (and kept,
    /// first, the oldest dropped beyond [`KEPT_ROADS`]).
    pub fn of<'a>(
        kept: &'a mut Vec<RoadMaterials>,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
        look: RoadLook,
        palette: &Palette,
    ) -> &'a RoadMaterials {
        let same = kept.iter().position(|k| {
            k.look == look && k.asphalt == palette.asphalt && k.curbs == palette.curbs
        });
        if let Some(i) = same {
            let found = kept.remove(i);
            kept.insert(0, found);
        } else {
            let started = bevy::platform::time::Instant::now();
            let paving = paving::paving_texture(look.paving, palette.asphalt, look.accent);
            let borders = paving::border_texture(look.border, palette.curbs);
            let made = started.elapsed();
            let bytes = paving.data.len() + borders.data.len();
            info!(
                "road: {:?} and {:?}, textures made in {:.1} ms ({:.1} MB with their mipmaps)",
                look.paving,
                look.border,
                made.as_secs_f64() * 1000.0,
                bytes as f64 / 1e6
            );
            let paving_material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(images.add(texture_image(&paving, true))),
                perceptual_roughness: paving::roughness(look.paving),
                ..default()
            });
            let borders_material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(images.add(texture_image(&borders, false))),
                perceptual_roughness: 0.85,
                ..default()
            });
            kept.insert(
                0,
                RoadMaterials {
                    look,
                    asphalt: palette.asphalt,
                    curbs: palette.curbs,
                    paving: paving_material,
                    borders: borders_material,
                },
            );
            kept.truncate(KEPT_ROADS);
        }
        &kept[0]
    }
}

/// Issue #187: how many roads' materials are kept (about 2 MB of textures each).
pub const KEPT_ROADS: usize = 4;

/// Issue #187: a texture of `super::paving` as an image: sRGB, its mipmaps, filtered (and
/// anisotropic where the GPU can: a road seen at a grazing angle stays sharp), repeating along
/// (and across when `repeat_across`: the paving; a border is its whole width).
fn texture_image(texture: &paving::Texture, repeat_across: bool) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut image = Image::new_uninit(
        Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(texture.data.clone());
    image.texture_descriptor.mip_level_count = texture.mips;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: if repeat_across {
            ImageAddressMode::Repeat
        } else {
            ImageAddressMode::ClampToEdge
        },
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

fn spawn_trimesh(
    commands: &mut Commands,
    data: &MeshData,
    surface: Surface,
    restitution: f32,
    flags: TriMeshFlags,
) -> Option<Entity> {
    let (vertices, triangles) = data.collider_data();
    match Collider::trimesh_with_flags(vertices, triangles, flags) {
        Ok(collider) => Some(
            commands
                .spawn((
                    LevelScoped,
                    Transform::default(),
                    RigidBody::Fixed,
                    collider,
                    Restitution::coefficient(restitution),
                    Friction::coefficient(0.0),
                    surface,
                ))
                .id(),
        ),
        Err(error) => {
            error!("track collider ({surface:?}) could not be built: {error:?}");
            None
        }
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
    // Issue #146: computed in one place, for the ground beyond to meet it edge to edge.
    let (center, half) = super::far::slab(&layout.center);
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

/// The boost pads' color (sRGB), orange (issue #187: it stands out on every road, a test).
pub const BOOST_PAD_COLOR: Rgb = (1.0, 0.55, 0.1);
/// The anti-gravity zones' pads' color (sRGB, issue #78), cyan blue.
pub const ANTI_GRAVITY_PAD_COLOR: Rgb = (0.1, 0.65, 1.0);

fn srgb((r, g, b): Rgb) -> Color {
    Color::srgb(r, g, b)
}

fn spawn_trick_ramps(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    layout: &TrackLayout,
    data: &TrackData,
    palette: &Palette,
) {
    let material = materials.add(Color::srgb(0.98, 0.72, 0.25));
    // Issue #193: a risky jump's ramp is grass (off-road, the green of the shoulders): without a
    // boost, a kart slows down on it and falls short; with one, it flies. One material more, only
    // on a circuit with a risky jump.
    let grass = layout.shortcuts.iter().any(|s| s.ramp.is_some()).then(|| {
        let (r, g, b) = palette.shoulder;
        materials.add(Color::srgb(r, g, b))
    });
    // White stripes mark a trick ramp and its lip, where to press the jump/drift button.
    let stripe_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.5, 0.5, 0.5),
        ..default()
    });
    let stripe_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let half_thickness = 0.5;
    // The file's trick ramps, along the centerline; then (issue #107) the risky jumps' ramps,
    // along their shortcuts: the same ramps (a trick works off them).
    let ramps = data
        .trick_ramps
        .iter()
        .map(|ramp| {
            let sample = layout.center.at(layout.center.param_to_distance(ramp.at));
            let start = sample.point(ramp.offset * sample.half_width, 0.0);
            let base = sample.transform(0.0, 0.0).rotation;
            (
                start,
                base,
                ramp.length,
                ramp.angle.to_radians(),
                ramp.width,
                None,
            )
        })
        .chain(layout.shortcuts.iter().filter_map(|shortcut| {
            let ramp = shortcut.ramp?;
            Some((
                ramp.start,
                ramp.base(),
                ramp.length,
                ramp.angle,
                ramp.width,
                grass.clone(),
            ))
        }));
    for (start, base, length, angle, width, grass) in ramps {
        let transform = ramp_transform_on(start, base, length, angle, half_thickness);
        let half = Vec3::new(width / 2.0, half_thickness, length / 2.0);
        let surface = if grass.is_some() {
            Surface::Offroad
        } else {
            Surface::Road
        };
        let entity = super::spawn_static_box(commands, transform, half, surface, 0.0);
        commands
            .entity(entity)
            .insert((
                TrickRamp,
                Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
                MeshMaterial3d(grass.unwrap_or_else(|| material.clone())),
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
/// `crate::items`, which also dresses them), unless the items are off (`item_boxes`, issue #99:
/// [`NoItemBoxes`]; their numbers, `SpotIndex`, are the boxes' only: the croquettes keep theirs).
fn spawn_spots(
    commands: &mut Commands,
    (meshes, materials): (&mut Assets<Mesh>, &mut Assets<StandardMaterial>),
    layout: &TrackLayout,
    data: &TrackData,
    item_boxes: bool,
) {
    let center = &layout.center;
    let at = |param: f32| center.at(center.param_to_distance(param));
    let place = |sample: &TrackSample, offset: f32, height: f32| {
        sample.transform(offset * sample.half_width, height)
    };

    let pad_mesh = meshes.add(Cuboid::new(BOOST_PAD_SIZE.x, 0.04, BOOST_PAD_SIZE.y));
    let pad_material = materials.add(StandardMaterial {
        base_color: srgb(BOOST_PAD_COLOR),
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
    // Issue #78: the pads of the anti-gravity zones are theirs, cyan blue (the same boost).
    let zones = &layout.anti_gravity;
    let mut zone_pad_material = None;
    for pad in &data.boost_pads {
        let sample = at(pad.at);
        let material = if anti_gravity_at(zones, center, sample.distance).is_some() {
            zone_pad_material
                .get_or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: srgb(ANTI_GRAVITY_PAD_COLOR),
                        emissive: LinearRgba::rgb(0.05, 0.35, 0.8),
                        ..default()
                    })
                })
                .clone()
        } else {
            pad_material.clone()
        };
        commands
            .spawn((
                LevelScoped,
                BoostPad {
                    half_size: BOOST_PAD_SIZE / 2.0,
                },
                Mesh3d(pad_mesh.clone()),
                MeshMaterial3d(material),
                place(&sample, pad.offset, 0.03),
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
    let rows = if item_boxes {
        data.item_boxes.as_slice()
    } else {
        &[]
    };
    for row in rows {
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
