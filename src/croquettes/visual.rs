//! Croquette visuals (GD § 14: a "pop" at each croquette), all sharing one mesh and a handful of
//! materials.
//!
//! - A croquette is a puffy ring of kibble standing on its edge, golden brown, toasted at its rim
//!   (issue #106: its model, `items::models`), spinning slowly. Taken, it disappears; back, it
//!   pops in (grows with a little overshoot).
//! - A scattered croquette hops from the kart to the ground (smoothly between two ticks), spins
//!   fast while flying, and blinks before it vanishes.
//! - Picking one up: a burst, the croquette jumping up as it shrinks and four golden sparks.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::CroquettePickedUp;
use super::logic::{LooseCroquette, TrackCroquette};
use super::tuning::CroquetteTuning;
use crate::items::models::{Look, model};
use crate::track::circuit::to_mesh;

/// Spin of a croquette on the track, and of a flying one (rad/s).
const SPIN: f32 = 2.5;
const FLYING_SPIN: f32 = 12.0;
/// A croquette coming back grows in this long (s).
const POP_IN: f32 = 0.3;
/// A scattered croquette blinks during its last seconds (s), this many times per second.
const BLINK_TIME: f32 = 1.5;
const BLINK_HZ: f32 = 8.0;
/// The pick-up burst lasts this long (s); the croquette jumps up this fast (m/s) and grows by
/// this much before shrinking away.
const BURST_TIME: f32 = 0.3;
const BURST_RISE: f32 = 3.5;
const BURST_GROWTH: f32 = 0.5;
/// Sparks of the burst: speed outward (m/s) and size (m).
const SPARK_SPEED: f32 = 4.0;
const SPARK_SIZE: f32 = 0.12;
const SPARKS: usize = 4;

/// Shared meshes and materials of the croquettes.
#[derive(Resource, Clone)]
pub struct CroquetteVisualAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub spark_mesh: Handle<Mesh>,
    pub spark_material: Handle<StandardMaterial>,
}

/// Visual state of a croquette: its spin phase (so they do not all turn in sync) and the time
/// since it (re)appeared.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CroquetteLook {
    pub phase: f32,
    pub shown_for: f32,
}

/// A piece of a pick-up burst: the croquette (`spark` false) or a spark flying `outward`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CroquetteBurst {
    pub origin: Vec3,
    pub outward: Vec3,
    pub spark: bool,
    pub age: f32,
}

pub struct CroquetteVisualPlugin;

impl Plugin for CroquetteVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_croquette_visual_assets)
            .add_systems(
                Update,
                (
                    dress_croquettes,
                    animate_croquettes,
                    spawn_bursts,
                    animate_bursts,
                )
                    .chain(),
            );
    }
}

fn create_croquette_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Issue #106: its model, standing on its edge, facing the karts (low poly: 224 triangles),
    // its colors in its vertices.
    let kibble = model(Look::Croquette).body;
    commands.insert_resource(CroquetteVisualAssets {
        mesh: meshes.add(to_mesh(&kibble)),
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(0.3, 0.15, 0.02),
            perceptual_roughness: 0.6,
            ..default()
        }),
        spark_mesh: meshes.add(Cuboid::from_length(1.0)),
        spark_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.3),
            emissive: LinearRgba::rgb(1.0, 0.75, 0.2),
            unlit: true,
            ..default()
        }),
    });
}

/// New croquettes (of the track, or scattered) get the shared mesh and material.
fn dress_croquettes(
    mut commands: Commands,
    assets: Res<CroquetteVisualAssets>,
    new: Query<(Entity, Has<LooseCroquette>), Or<(Added<TrackCroquette>, Added<LooseCroquette>)>>,
) {
    for (entity, loose) in &new {
        commands.entity(entity).insert((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            CroquetteLook {
                phase: (entity.to_bits() % 97) as f32 * 0.61,
                // The croquettes of the track are simply there at the start; a scattered one
                // pops out of the kart.
                shown_for: if loose { 0.0 } else { POP_IN },
            },
        ));
    }
}

/// Scale of a croquette `t` seconds after it (re)appeared: grows to 1 with a small overshoot.
pub fn pop_in_scale(t: f32) -> f32 {
    let x = (t / POP_IN).clamp(0.0, 1.0);
    // Ease out back: overshoots ~10 % around x = 0.6, then settles at exactly 1.
    let c = 1.7;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

fn animate_croquettes(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<CroquetteTuning>,
    mut track: Query<
        (
            &TrackCroquette,
            &mut Transform,
            &mut Visibility,
            &mut CroquetteLook,
        ),
        Without<LooseCroquette>,
    >,
    mut loose: Query<
        (
            &LooseCroquette,
            &mut Transform,
            &mut Visibility,
            &mut CroquetteLook,
        ),
        Without<TrackCroquette>,
    >,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (croquette, mut transform, mut visibility, mut look) in &mut track {
        if !croquette.available() {
            visibility.set_if_neq(Visibility::Hidden);
            look.shown_for = 0.0;
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);
        look.shown_for += dt;
        transform.rotation = Quat::from_rotation_y(now * SPIN + look.phase);
        transform.scale = Vec3::splat(pop_in_scale(look.shown_for));
    }
    // Smooth between two ticks: the flight is a function of time.
    let overstep = fixed.overstep_fraction() * fixed.timestep().as_secs_f32();
    for (croquette, mut transform, mut visibility, mut look) in &mut loose {
        let age = croquette.age + overstep;
        look.shown_for += dt;
        transform.translation = croquette.position_at(&tuning, age);
        let flying = age < tuning.scatter_flight_time;
        let spin = if flying { FLYING_SPIN } else { SPIN };
        transform.rotation = Quat::from_rotation_y(now * spin + look.phase);
        transform.scale = Vec3::splat(pop_in_scale(look.shown_for));
        let left = tuning.loose_lifetime - age;
        let shown = left > BLINK_TIME || (left * BLINK_HZ).fract() > 0.4;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// A burst where a croquette was picked up: the croquette itself and a few sparks.
fn spawn_bursts(
    mut commands: Commands,
    assets: Res<CroquetteVisualAssets>,
    mut picked: MessageReader<CroquettePickedUp>,
) {
    for pick in picked.read() {
        commands.spawn((
            CroquetteBurst {
                origin: pick.at,
                outward: Vec3::Y,
                spark: false,
                age: 0.0,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(pick.at),
        ));
        for k in 0..SPARKS {
            let angle = TAU * (k as f32 + 0.5) / SPARKS as f32;
            let outward = Vec3::new(angle.cos(), 0.8, angle.sin()).normalize();
            commands.spawn((
                CroquetteBurst {
                    origin: pick.at,
                    outward,
                    spark: true,
                    age: 0.0,
                },
                Mesh3d(assets.spark_mesh.clone()),
                MeshMaterial3d(assets.spark_material.clone()),
                Transform::from_translation(pick.at).with_scale(Vec3::splat(SPARK_SIZE)),
            ));
        }
    }
}

fn animate_bursts(
    time: Res<Time>,
    mut commands: Commands,
    mut bursts: Query<(Entity, &mut CroquetteBurst, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut burst, mut transform) in &mut bursts {
        burst.age += dt;
        let t = burst.age / BURST_TIME;
        if t >= 1.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        if burst.spark {
            transform.translation = burst.origin + burst.outward * SPARK_SPEED * burst.age;
            transform.scale = Vec3::splat(SPARK_SIZE * (1.0 - t));
        } else {
            // Jumps up, grows a little, then shrinks away while spinning.
            transform.translation = burst.origin + Vec3::Y * BURST_RISE * burst.age;
            let size = (1.0 + BURST_GROWTH * (t * TAU / 2.0).sin()) * (1.0 - t * t);
            transform.scale = Vec3::splat(size);
            transform.rotation = Quat::from_rotation_y(burst.age * FLYING_SPIN * 2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_croquette_pops_in_with_a_small_overshoot() {
        assert!(pop_in_scale(0.0).abs() < 1e-5);
        assert!((pop_in_scale(POP_IN) - 1.0).abs() < 1e-5);
        assert!((pop_in_scale(10.0) - 1.0).abs() < 1e-5);
        let peak = (1..30)
            .map(|i| pop_in_scale(POP_IN * i as f32 / 30.0))
            .fold(0.0f32, f32::max);
        assert!(peak > 1.05 && peak < 1.2, "{peak}");
    }
}
