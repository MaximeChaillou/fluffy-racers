//! The tow pelican's placeholder visual (GD § 2): a white bird in primitive shapes, beak and
//! flapping wings, holding the kart from above while it carries it. Purely visual: the carry
//! itself is `tow.rs`.

use bevy::prelude::*;

use super::{Racer, Tow};
use crate::interpolation::InterpolatedFrom;

/// A pelican carrying the kart `body`. It follows the kart's interpolated pose.
#[derive(Component, Clone, Copy, Debug)]
pub struct PelicanVisual {
    pub body: Entity,
}

/// A wing of a pelican: `side` -1 (left) or 1 (right).
#[derive(Component, Clone, Copy, Debug)]
struct PelicanWing {
    side: f32,
}

/// Meshes and materials shared by every pelican.
#[derive(Resource)]
struct PelicanAssets {
    body: Handle<Mesh>,
    head: Handle<Mesh>,
    beak: Handle<Mesh>,
    wing: Handle<Mesh>,
    feathers: Handle<StandardMaterial>,
    beak_color: Handle<StandardMaterial>,
}

/// Height of the pelican's body above the kart (m).
const PELICAN_HEIGHT: f32 = 2.2;
/// Wing flaps per second.
const FLAP_RATE: f32 = 3.0;
/// Wing flap amplitude (rad).
const FLAP_ANGLE: f32 = 0.6;

pub struct PelicanPlugin;

impl Plugin for PelicanPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_assets)
            .add_systems(Update, (show_pelicans, flap_wings).chain());
    }
}

fn create_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PelicanAssets {
        body: meshes.add(Sphere::new(0.7)),
        head: meshes.add(Sphere::new(0.32)),
        beak: meshes.add(Cuboid::new(0.22, 0.16, 0.9)),
        wing: meshes.add(Cuboid::new(1.5, 0.08, 0.6)),
        feathers: materials.add(Color::srgb(0.97, 0.97, 0.95)),
        beak_color: materials.add(Color::srgb(1.0, 0.65, 0.15)),
    });
}

/// Spawns a pelican over each carried kart, removes it once the kart is dropped.
fn show_pelicans(
    mut commands: Commands,
    assets: Option<Res<PelicanAssets>>,
    racers: Query<(Entity, &Tow), With<Racer>>,
    pelicans: Query<(Entity, &PelicanVisual)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (pelican, visual) in &pelicans {
        let carrying = racers
            .get(visual.body)
            .is_ok_and(|(_, tow)| tow.is_carrying());
        if !carrying {
            commands.entity(pelican).despawn();
        }
    }
    for (body, tow) in &racers {
        if tow.is_carrying() && !pelicans.iter().any(|(_, visual)| visual.body == body) {
            spawn_pelican(&mut commands, &assets, body);
        }
    }
}

fn spawn_pelican(commands: &mut Commands, assets: &PelicanAssets, body: Entity) {
    commands
        .spawn((
            Name::new("Tow pelican"),
            PelicanVisual { body },
            InterpolatedFrom(body),
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|bird| {
            bird.spawn((
                Mesh3d(assets.body.clone()),
                MeshMaterial3d(assets.feathers.clone()),
                Transform::from_xyz(0.0, PELICAN_HEIGHT, 0.0).with_scale(Vec3::new(0.9, 0.8, 1.3)),
            ));
            bird.spawn((
                Mesh3d(assets.head.clone()),
                MeshMaterial3d(assets.feathers.clone()),
                Transform::from_xyz(0.0, PELICAN_HEIGHT + 0.5, -0.9),
            ));
            bird.spawn((
                Mesh3d(assets.beak.clone()),
                MeshMaterial3d(assets.beak_color.clone()),
                Transform::from_xyz(0.0, PELICAN_HEIGHT + 0.4, -1.5),
            ));
            for side in [-1.0, 1.0] {
                bird.spawn((
                    PelicanWing { side },
                    Mesh3d(assets.wing.clone()),
                    MeshMaterial3d(assets.feathers.clone()),
                    Transform::from_xyz(side * 1.1, PELICAN_HEIGHT + 0.2, 0.0),
                ));
            }
        });
}

fn flap_wings(time: Res<Time>, mut wings: Query<(&PelicanWing, &mut Transform)>) {
    let angle = (time.elapsed_secs() * FLAP_RATE * std::f32::consts::TAU).sin() * FLAP_ANGLE;
    for (wing, mut transform) in &mut wings {
        // Hinged at the body: rotate around the shoulder, on the wing's inner end.
        let shoulder = Vec3::new(wing.side * 0.35, PELICAN_HEIGHT + 0.2, 0.0);
        let rotation = Quat::from_rotation_z(wing.side * angle);
        transform.rotation = rotation;
        transform.translation = shoulder + rotation * Vec3::new(wing.side * 0.75, 0.0, 0.0);
    }
}
