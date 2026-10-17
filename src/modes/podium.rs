//! The podium of a Grand Prix (issue #19): after the last race, the three first of the overall
//! standings stand on their steps, on the road a little after the start line, facing a still
//! camera (`camera::FixedView`); their drivers dance (`race::OnPodium`, `characters`). Every other
//! kart goes back to its grid slot, hidden; all of them are parked (`race::Parked`: their engine
//! off). The steps are part of the level (`LevelScoped`): the next level removes them, and the
//! chase camera comes back ([`release_the_view`]).
//!
//! Issue #122: online, every peer builds the same podium from the host's standings: it puts
//! **its own** karts (its player's, the host's bots) on their steps or back on their slots, the
//! other peers' karts come there by their states (`net`), and those off the steps are hidden here
//! too (those on them dance).

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::camera::FixedView;
use crate::interpolation::{InterpolatedFrom, PhysicsPose};
use crate::kart::{Boost, Kart, KartState, Remote};
use crate::level::LevelScoped;
use crate::race::{
    GrandPrix, GrandPrixStage, OnPodium, Parked, Race, RaceGrid, RacePhase, Racer, Tow,
};
use crate::track::{GRID_HEIGHT, Surface, TrackLayout, spawn_static_box};
use crate::ui::HudRoot;

/// The podium stands this far after the start line, on the road (m).
pub const PODIUM_AHEAD: f32 = 25.0;
/// Each step: across the road from the centerline (m, right positive) and its height (m). The
/// winner in the middle, the 2nd on its right as the camera sees it (the road's left), the 3rd on
/// its left.
const STEPS: [(f32, f32); 3] = [(0.0, 1.2), (-3.2, 0.8), (3.2, 0.5)];
/// Half the width and the depth of a step (m).
const STEP_HALF: f32 = 1.4;
/// The steps' colors: gold, silver, bronze.
const STEP_COLORS: [Color; 3] = [
    Color::srgb(0.95, 0.76, 0.22),
    Color::srgb(0.78, 0.80, 0.84),
    Color::srgb(0.80, 0.50, 0.26),
];
/// The camera stands this far before the podium, this high above the road, and frames it with
/// this vertical field of view (degrees).
const VIEW_DISTANCE: f32 = 11.0;
const VIEW_HEIGHT: f32 = 3.0;
const VIEW_FOV: f32 = 45.0;

/// On the podium's root (the steps are its children's colliders and meshes).
#[derive(Component, Clone, Copy, Debug)]
pub struct PodiumScene;

/// The middle of the top of the step of `place` (0 = the winner), on `layout`.
fn step_top(layout: &TrackLayout, place: usize) -> (Vec3, Vec3, Vec3, f32) {
    let sample = layout.center.at(PODIUM_AHEAD);
    let (lateral, height) = STEPS[place.min(STEPS.len() - 1)];
    (
        sample.point(lateral, height),
        sample.forward,
        sample.up,
        height,
    )
}

/// Where the kart of `place` (0 = the winner) stands: on its step, wheels on it, facing back
/// toward the start line (the camera).
pub fn podium_place(layout: &TrackLayout, place: usize) -> Transform {
    let (top, forward, up, _) = step_top(layout, place);
    Transform::from_translation(top + up * GRID_HEIGHT).looking_to(-forward, up)
}

/// Builds the podium once the Grand Prix reaches it, after the results of its last race (never on
/// a level being rebuilt, before its race and its karts exist): the steps, the three first on
/// them, the others parked back on the grid and hidden, the still camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn set_up_podium(
    mut commands: Commands,
    grand_prix: Option<Res<GrandPrix>>,
    race: Option<Res<Race>>,
    scenes: Query<(), With<PodiumScene>>,
    layout: Option<Res<TrackLayout>>,
    grid: Res<RaceGrid>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut karts: Query<
        (
            Entity,
            &Racer,
            &mut Transform,
            &mut Velocity,
            &mut KartState,
            &mut Boost,
            &mut PhysicsPose,
            &mut Tow,
        ),
        With<Kart>,
    >,
    visuals: Query<(Entity, &InterpolatedFrom)>,
    remotes: Query<(Entity, &Racer), (With<Remote>, Without<Kart>)>,
    hud: Query<Entity, With<HudRoot>>,
) {
    let (Some(grand_prix), Some(layout), Some(race)) = (grand_prix, layout, race) else {
        return;
    };
    if grand_prix.stage != GrandPrixStage::Podium
        || race.phase() != RacePhase::Results
        || !scenes.is_empty()
    {
        return;
    }
    let standings = grand_prix.standings();
    let root = commands
        .spawn((
            Name::new("Podium"),
            PodiumScene,
            LevelScoped,
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let steps = STEPS.len().min(standings.len());
    for (place, color) in STEP_COLORS.iter().enumerate().take(steps) {
        let (top, forward, up, height) = step_top(&layout, place);
        let center = top - up * (height / 2.0);
        let transform = Transform::from_translation(center).looking_to(forward, up);
        let half = Vec3::new(STEP_HALF, height / 2.0, STEP_HALF);
        let step = spawn_static_box(&mut commands, transform, half, Surface::Road, 0.0);
        commands.entity(step).insert(ChildOf(root));
        if let (Some(meshes), Some(materials)) = (meshes.as_mut(), materials.as_mut()) {
            commands.entity(step).insert((
                Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: *color,
                    perceptual_roughness: 0.6,
                    ..default()
                })),
            ));
        }
    }
    let place_of = |racer: &Racer| {
        let entrant = grid.entrant_at(racer.grid_slot);
        standings
            .iter()
            .position(|e| *e == entrant)
            .filter(|place| *place < STEPS.len())
    };
    let hide = |commands: &mut Commands, entity: Entity| {
        for (visual, from) in &visuals {
            if from.0 == entity {
                commands.entity(visual).insert(Visibility::Hidden);
            }
        }
    };
    // Issue #122: online, the other peers' karts off the steps are out of sight too.
    for (entity, racer) in &remotes {
        match place_of(racer) {
            Some(place) => {
                commands.entity(entity).insert(OnPodium(place));
            }
            None => hide(&mut commands, entity),
        }
    }
    for (entity, racer, mut transform, mut velocity, mut state, mut boost, mut pose, mut tow) in
        &mut karts
    {
        let spot = match place_of(racer) {
            Some(place) => {
                commands.entity(entity).insert(OnPodium(place));
                podium_place(&layout, place)
            }
            None => {
                // Back to its slot, out of sight.
                hide(&mut commands, entity);
                layout.grid[racer.grid_slot.min(layout.grid.len() - 1)]
            }
        };
        commands.entity(entity).insert(Parked);
        *transform = spot;
        *velocity = Velocity::zero();
        *state = KartState::Grounded;
        *boost = Boost::default();
        pose.snap(spot);
        tow.reset(spot.translation);
    }
    // The race's HUD steps aside (rank, lap, items): only the podium's names stay.
    for root in &hud {
        commands.entity(root).insert(Visibility::Hidden);
    }
    let (top, forward, up, _) = step_top(&layout, 0);
    let look_at = top + up * 1.0;
    commands.insert_resource(FixedView {
        eye: look_at - forward * VIEW_DISTANCE + up * VIEW_HEIGHT,
        look_at,
        fov_degrees: VIEW_FOV,
    });
}

/// The chase camera comes back once the podium is gone (another level: a new Grand Prix, the race
/// menu, « Relancer »...).
pub(super) fn release_the_view(
    mut commands: Commands,
    view: Option<Res<FixedView>>,
    scenes: Query<(), With<PodiumScene>>,
) {
    if view.is_some() && scenes.is_empty() {
        commands.remove_resource::<FixedView>();
    }
}
