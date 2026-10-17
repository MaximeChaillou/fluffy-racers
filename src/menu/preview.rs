//! The « Pilote » screen's 3D (issue #98): the kart seen from 3/4 front with its driver in it,
//! and the portraits of the characters.
//!
//! - **The preview**: a camera renders, into an image that the screen's tree shows
//!   (`ViewportNode`, the size of its frame), a kart visual of the choice (the same one as the
//!   game's: `kart::spawn_kart_visual_with_driver`, dressed with the models and its animal by the
//!   same systems) on a body of its own ([`PreviewBody`]: its color, `KartPaint`; its wing open
//!   while the wing's line has the focus, `KartEnvironment::gliding`; no physics, no `Kart`).
//!   The kart sways a little, its driver does its soft loop (`pilot::soft_pose`), the camera
//!   steps back to show the open wing. A change of the choice spawns another visual on the same
//!   frame (it is dressed in `PostUpdate`, before the frame is drawn): it shows at once.
//! - **The portraits**: the nine animals standing still in a grid, seen from 3/4 front, rendered
//!   **once** into one sheet (a few frames, then the camera and the scene are gone); the grid of
//!   the screen shows its cells (`ImageNode::rect`). Kept for the whole game: reopening the screen
//!   renders nothing.
//!
//! **Out of the game's sight**: both scenes are far under the circuits (3 km: beyond the far plane
//! of the game's camera), on render layers of their own (with their own light, without shadows,
//! and their own ambient light): the game's camera does not draw them, theirs draw nothing of
//! the game, and the game's sun casts no shadow for them. **Closed, nothing is left**: the
//! preview's camera, light, body, visual and image go with the screen.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::pilot::{self, PORTRAIT_PX};
use crate::characters::{
    Animals, DriverLooks, DriverTuning, PartKind, Roster, SelectionScreen, spawn_animal_still,
};
use crate::kart::{
    DRIVER_SEAT, DriverVisual, KartBodyPivot, KartColor, KartEnvironment, KartPaint, KartTuning,
    KartVisualAssets, Wing, spawn_kart_visual_with_driver, wing_opening,
};

/// The render layer of the preview's scene and camera...
pub const PREVIEW_LAYER: usize = 3;
/// ...and of the portraits'.
pub const PORTRAIT_LAYER: usize = 4;
/// Where the preview's kart stands: far under every circuit.
pub const PREVIEW_ORIGIN: Vec3 = Vec3::new(0.0, -3000.0, 0.0);
/// Where the portraits' grid is: next to it.
pub const PORTRAIT_ORIGIN: Vec3 = Vec3::new(60.0, -3000.0, 0.0);
/// The portraits' camera renders this many frames, then it is gone.
pub const PORTRAIT_FRAMES: u32 = 8;
/// The light of the two scenes (lux) and their ambient light (cd/m², like a biome's).
const LIGHT: f32 = 9000.0;
const AMBIENT: f32 = 700.0;

/// The preview, while the screen is open.
#[derive(Resource, Clone, Debug)]
pub struct PilotPreview {
    pub camera: Entity,
    pub light: Entity,
    pub body: Entity,
    /// The kart visual of the choice shown (None: not yet, or no kart assets).
    pub kart: Option<Entity>,
    /// The choice it shows.
    pub shown: Option<crate::characters::Loadout>,
    /// The image its camera renders into.
    pub image: Handle<Image>,
    /// When it opened (s, real time): its loops start there.
    pub opened_at: f64,
}

/// The preview's camera.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PreviewCamera;

/// The preview's kart body: what its visual follows (no physics).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PreviewBody;

/// The preview's kart visual.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PreviewKart;

/// The root of a scene of the screen: it and everything under it are drawn on this render layer
/// only ([`spread_the_layers`]).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerRoot(pub usize);

/// Something of the preview's scene (its camera, its light, its body).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PreviewScene;

/// The portraits' sheet, and where its rendering is.
#[derive(Resource, Clone, Debug)]
pub struct Portraits {
    pub image: Handle<Image>,
    pub state: PortraitState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PortraitState {
    /// Not rendered yet (the animals are loading, or nobody opened the screen).
    Waiting,
    /// Its camera renders (frames rendered so far), then the scene goes.
    Rendering {
        camera: Entity,
        light: Entity,
        scene: Entity,
        frames: u32,
    },
    /// Rendered: the sheet stays as it is.
    Ready,
}

/// Something of the portraits' scene.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PortraitScene;

/// An image a camera renders into, `width` × `height` px.
fn target(images: &mut Assets<Image>, width: u32, height: u32) -> Handle<Image> {
    images.add(Image::new_target_texture(
        width,
        height,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ))
}

/// A camera of the screen's 3D: rendered before the game's (into its image), on its layer,
/// on a see-through background (the panel shows through), with its ambient light.
fn camera_bundle(image: &Handle<Image>, order: isize, layer: usize) -> impl Bundle {
    (
        Camera3d::default(),
        Camera {
            order,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        RenderLayers::layer(layer),
        AmbientLight {
            color: Color::WHITE,
            brightness: AMBIENT,
            ..default()
        },
    )
}

/// The light of a scene at `origin`, on its layer, without shadows: from the front left, high.
fn light_bundle(origin: Vec3, layer: usize) -> impl Bundle {
    (
        DirectionalLight {
            illuminance: LIGHT,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(origin + Vec3::new(3.0, 6.0, -5.0)).looking_at(origin, Vec3::Y),
        RenderLayers::layer(layer),
    )
}

/// Spawns the preview (no kart yet: `follow_the_choice` puts it in), its image in `images` (none
/// headless without images).
pub fn spawn_preview(
    commands: &mut Commands,
    images: Option<&mut Assets<Image>>,
    color: KartColor,
    now: f64,
) -> PilotPreview {
    let image = images
        .map(|images| target(images, 1, 1))
        .unwrap_or_default();
    let camera = commands
        .spawn((
            Name::new("Pilot preview camera"),
            PreviewCamera,
            PreviewScene,
            camera_bundle(&image, -1, PREVIEW_LAYER),
            Projection::Perspective(PerspectiveProjection {
                fov: pilot::PREVIEW_FOV,
                ..default()
            }),
            place(pilot::preview_camera(0.0)),
        ))
        .id();
    let light = commands
        .spawn((
            Name::new("Pilot preview light"),
            PreviewScene,
            light_bundle(PREVIEW_ORIGIN, PREVIEW_LAYER),
        ))
        .id();
    let body = commands
        .spawn((
            Name::new("Pilot preview body"),
            PreviewScene,
            PreviewBody,
            KartPaint(color),
            KartEnvironment::default(),
            Transform::from_translation(PREVIEW_ORIGIN),
        ))
        .id();
    PilotPreview {
        camera,
        light,
        body,
        kart: None,
        shown: None,
        image,
        opened_at: now,
    }
}

/// A transform around the preview's kart, in the world.
fn place(mut transform: Transform) -> Transform {
    transform.translation += PREVIEW_ORIGIN;
    transform
}

/// Everything of the preview goes (the screen closed), its image too.
pub fn despawn_preview(
    commands: &mut Commands,
    preview: &PilotPreview,
    images: Option<&mut Assets<Image>>,
) {
    for entity in [
        Some(preview.camera),
        Some(preview.light),
        Some(preview.body),
        preview.kart,
    ]
    .into_iter()
    .flatten()
    {
        commands.entity(entity).try_despawn();
    }
    if let Some(images) = images {
        images.remove(&preview.image);
    }
}

/// The kart of the choice in the preview: another visual when the choice changes (or when a new
/// level took it away: kart visuals belong to a level); its color and its open wing on its body.
pub(super) fn follow_the_choice(
    mut commands: Commands,
    screen: Res<SelectionScreen>,
    preview: Option<ResMut<PilotPreview>>,
    roster: Res<Roster>,
    time: Res<Time<Real>>,
    (assets, looks): (Option<Res<KartVisualAssets>>, Option<Res<DriverLooks>>),
    karts: Query<(), With<PreviewKart>>,
    mut bodies: Query<(&mut KartPaint, &mut KartEnvironment), With<PreviewBody>>,
) {
    let Some(mut preview) = preview else {
        return;
    };
    let alive = preview.kart.is_some_and(|kart| karts.contains(kart));
    if !alive || preview.shown != Some(screen.choice) {
        if let Some(old) = preview.kart.take() {
            commands.entity(old).try_despawn();
        }
        if let Some(assets) = assets.as_deref() {
            let driver = looks
                .as_deref()
                .and_then(|looks| looks.get(screen.choice.character));
            let visual = spawn_kart_visual_with_driver(
                &mut commands,
                assets,
                preview.body,
                screen.color,
                driver,
                roster.looks(&screen.choice),
            );
            let yaw = pilot::preview_yaw((time.elapsed_secs_f64() - preview.opened_at) as f32);
            commands.entity(visual).insert((
                Name::new("Pilot preview kart"),
                PreviewKart,
                Transform::from_translation(PREVIEW_ORIGIN)
                    .with_rotation(Quat::from_rotation_y(yaw)),
                LayerRoot(PREVIEW_LAYER),
            ));
            preview.kart = Some(visual);
        }
        preview.shown = Some(screen.choice);
    }
    if let Ok((mut paint, mut environment)) = bodies.get_mut(preview.body) {
        paint.set_if_neq(KartPaint(screen.color));
        // The wing opens while its line has the focus.
        let gliding = screen.row == PartKind::Wing.index();
        if environment.gliding != gliding {
            environment.gliding = gliding;
        }
    }
}

/// Each frame of the preview (after the visuals are placed, before the transforms propagate):
/// the kart sways, its driver does its soft loop on its seat, the camera stands back as much as
/// its wing is open. The game paused behind the screen (solo, from the game), its wing still
/// opens and folds (on real time: the game's own systems wait).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn pose_the_preview(
    time: Res<Time<Real>>,
    game_time: Res<Time<Virtual>>,
    tuning: Option<Res<KartTuning>>,
    (preview, screen, assets): (
        Option<Res<PilotPreview>>,
        Res<SelectionScreen>,
        Option<Res<KartVisualAssets>>,
    ),
    pivots: Query<(Entity, &KartBodyPivot, &Children)>,
    mut drivers: Query<
        (&DriverVisual, &mut Transform),
        (Without<PreviewKart>, Without<PreviewCamera>),
    >,
    mut wings: Query<(&mut Wing, &ChildOf)>,
    mut karts: Query<&mut Transform, (With<PreviewKart>, Without<PreviewCamera>)>,
    mut cameras: Query<&mut Transform, (With<PreviewCamera>, Without<PreviewKart>)>,
) {
    let Some(preview) = preview else {
        return;
    };
    let t = (time.elapsed_secs_f64() - preview.opened_at) as f32;
    if let Some(mut kart) = preview.kart.and_then(|kart| karts.get_mut(kart).ok()) {
        kart.rotation = Quat::from_rotation_y(pilot::preview_yaw(t));
    }
    let seconds = tuning.map_or(0.3, |t| t.environment.wing_open_seconds);
    let open_it = screen.row == PartKind::Wing.index();
    let mut open = 0.0f32;
    let pose = pilot::soft_pose(t);
    for (pivot_entity, pivot, children) in &pivots {
        if pivot.body != preview.body {
            continue;
        }
        let seat = assets
            .as_deref()
            .and_then(|assets| assets.model())
            .map_or(DRIVER_SEAT, |model| model.seat(pivot.looks.body));
        for child in children.iter() {
            if let Ok((driver, mut transform)) = drivers.get_mut(child) {
                transform.set_if_neq(pose.transform(seat, driver.size));
            }
        }
        for (mut wing, child_of) in &mut wings {
            if child_of.parent() != pivot_entity {
                continue;
            }
            if game_time.is_paused() {
                let next = wing_opening(wing.open, open_it, time.delta_secs(), seconds);
                if next != wing.open {
                    wing.open = next;
                }
            }
            open = open.max(wing.open);
        }
    }
    if let Ok(mut camera) = cameras.get_mut(preview.camera) {
        camera.set_if_neq(place(pilot::preview_camera(open)));
    }
}

/// The portraits' sheet, `PORTRAIT_PX` px a cell (none headless without images).
pub fn new_portraits(images: Option<&mut Assets<Image>>) -> Option<Portraits> {
    let side = PORTRAIT_PX * pilot::GRID_COLUMNS as u32;
    images.map(|images| Portraits {
        image: target(images, side, side),
        state: PortraitState::Waiting,
    })
}

/// Renders the portraits once: when the screen first opens and every animal is loaded (or
/// missing: its ball of primitives instead), the scene and its camera for a few frames, then
/// nothing ever again.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_portraits(
    mut commands: Commands,
    screen: Res<SelectionScreen>,
    portraits: Option<ResMut<Portraits>>,
    roster: Res<Roster>,
    animals: Option<Res<Animals>>,
    tuning: Option<Res<DriverTuning>>,
    looks: Option<Res<DriverLooks>>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
) {
    let Some(mut portraits) = portraits else {
        return;
    };
    match portraits.state {
        PortraitState::Ready => {}
        PortraitState::Rendering {
            camera,
            light,
            scene,
            frames,
        } => {
            if frames + 1 >= PORTRAIT_FRAMES {
                for entity in [camera, light, scene] {
                    commands.entity(entity).try_despawn();
                }
                portraits.state = PortraitState::Ready;
            } else {
                portraits.state = PortraitState::Rendering {
                    camera,
                    light,
                    scene,
                    frames: frames + 1,
                };
            }
        }
        PortraitState::Waiting => {
            if !screen.open || animals.as_deref().is_some_and(Animals::waiting) {
                return;
            }
            let tuning = tuning.map(|t| t.clone()).unwrap_or_default();
            let ball = meshes.as_mut().map(|meshes| meshes.add(Sphere::new(0.45)));
            let scene = commands
                .spawn((
                    Name::new("Pilot portraits"),
                    PortraitScene,
                    Transform::from_translation(PORTRAIT_ORIGIN),
                    Visibility::default(),
                    LayerRoot(PORTRAIT_LAYER),
                ))
                .id();
            for (line, characters) in pilot::character_grid(&roster).iter().enumerate() {
                for (column, index) in characters.iter().enumerate() {
                    let character = &roster.characters[*index];
                    let seat = pilot::portrait_center(line, column)
                        - Vec3::Y * pilot::PORTRAIT_SEAT * pilot::PORTRAIT_CELL;
                    let turn = Quat::from_rotation_x(pilot::PORTRAIT_TILT)
                        * Quat::from_rotation_y(pilot::PORTRAIT_YAW);
                    match animals.as_deref().and_then(|a| a.get(*index)) {
                        Some(animal) => {
                            let scale = tuning.model_scale.max(0.01) * character.size;
                            let at = Transform {
                                translation: seat - turn * (animal.data.seat_point * scale),
                                rotation: turn,
                                scale: Vec3::splat(scale),
                            };
                            let root = spawn_animal_still(&mut commands, animal, at, ());
                            commands.entity(root).insert(ChildOf(scene));
                        }
                        None => {
                            // Its ball of primitives, in its color.
                            let (Some(ball), Some(look)) = (
                                ball.clone(),
                                looks.as_deref().and_then(|looks| looks.get(*index)),
                            ) else {
                                continue;
                            };
                            commands.spawn((
                                Mesh3d(ball),
                                MeshMaterial3d(look.material.clone()),
                                Transform::from_translation(seat + Vec3::Y * 0.45 * character.size)
                                    .with_scale(Vec3::splat(character.size)),
                                ChildOf(scene),
                            ));
                        }
                    }
                }
            }
            let side = pilot::PORTRAIT_CELL * pilot::GRID_COLUMNS as f32;
            let camera = commands
                .spawn((
                    Name::new("Pilot portraits camera"),
                    PortraitScene,
                    camera_bundle(&portraits.image, -2, PORTRAIT_LAYER),
                    Projection::Orthographic(OrthographicProjection {
                        scaling_mode: ScalingMode::Fixed {
                            width: side,
                            height: side,
                        },
                        ..OrthographicProjection::default_3d()
                    }),
                    Transform::from_translation(PORTRAIT_ORIGIN + Vec3::Z * 10.0)
                        .looking_at(PORTRAIT_ORIGIN, Vec3::Y),
                ))
                .id();
            let light = commands
                .spawn((
                    Name::new("Pilot portraits light"),
                    PortraitScene,
                    light_bundle_facing(PORTRAIT_ORIGIN, PORTRAIT_LAYER),
                ))
                .id();
            portraits.state = PortraitState::Rendering {
                camera,
                light,
                scene,
                frames: 0,
            };
        }
    }
}

/// A scene of the screen is there (the preview, or the portraits being rendered).
pub(super) fn layers_to_spread(roots: Query<(), With<LayerRoot>>) -> bool {
    !roots.is_empty()
}

/// Every entity under a [`LayerRoot`] (the kart visual's children and the animal's, spawned by
/// their own systems) is put on its layer before the visibility is checked: the game's camera
/// never draws them, nor theirs the game. A few dozen entities, while the screen is open.
pub(super) fn spread_the_layers(
    mut commands: Commands,
    roots: Query<(Entity, &LayerRoot)>,
    children: Query<&Children>,
    layered: Query<(), With<RenderLayers>>,
) {
    for (root, layer) in &roots {
        for entity in std::iter::once(root).chain(children.iter_descendants(root)) {
            if !layered.contains(entity) {
                commands
                    .entity(entity)
                    .try_insert(RenderLayers::layer(layer.0));
            }
        }
    }
}

/// The portraits' light: from the camera's side (+Z), high and to the left.
fn light_bundle_facing(origin: Vec3, layer: usize) -> impl Bundle {
    (
        DirectionalLight {
            illuminance: LIGHT,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(origin + Vec3::new(-3.0, 5.0, 6.0)).looking_at(origin, Vec3::Y),
        RenderLayers::layer(layer),
    )
}
