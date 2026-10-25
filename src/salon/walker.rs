//! Issue #101: the characters on foot of the lobby, on screen and (ours) simulated.
//!
//! - **A body** ([`Walker`]): a Rapier body the shape of its animal's block (a rounded box),
//!   upright with its rotation locked while it walks, free to turn when it tumbles
//!   (`characters::Ragdoll`). Ours ([`PlayerWalker`]) is simulated here ([`WalkerControl`]: the
//!   race's keys and stick, `walk.rs`); the others' are kinematic, placed where their states say
//!   (`online.rs`).
//! - **A visual** ([`WalkerVisual`]), interpolated between ticks for ours, placed by the network
//!   for the others: **its animal on its legs** (`characters::spawn_animal_on_foot`: the `walk` and
//!   `run` clips at the pace of its steps, `idle` standing, the legs paddling while it tumbles, a
//!   shake of the head once up; the wobble of #12 over them, kicked by each fall), **a party hat
//!   of its player's color**, its paw (issue #169: its animal's own, out of its body, like the
//!   karts' arms: `characters::AnimalPaw`) and a « pof » where a strike lands. Headless (no model), primitives: a box and the hat.
//! - **Its pseudo over it** ([`WalkerTag`]), the karts' tags (`net::tags`): ours too.
//! - **The camera** ([`film_the_salon`]): it follows ours from a fixed angle, behind and above
//!   (up on the screen is away from it: `walk.rs`).
//!
//! **Fluidity**: per tick, a raycast and a few vectors for ours; per frame, 12 visuals placed, 12
//! animation players compared, 12 wobbles stepped, 12 tags moved (`UiTransform`): no allocation,
//! no entity spawned (a swing collects the few characters in reach, only when the paw goes).

use std::time::Duration;

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    Ccd, CoefficientCombineRule, Collider, Friction, LockedAxes, QueryFilter, ReadRapierContext,
    Restitution, RigidBody, Sleeping, Velocity,
};

use super::SalonTuning;
use super::online::RemoteWalkerBody;
use super::scene::seat_spot;
use super::walk::{
    Bouncer, FootInput, PawCandidate, foot_input, heading_toward, on_the_right, paw_target,
    stick_axis, turned, walk_velocity,
};
use crate::camera::ChaseCamera;
use crate::characters::{
    AnimalAnimation, AnimalPart, Animals, AnimationNodes, DriverLooks, DriverTuning, DriverWobble,
    Knock, PlayerColor, PlayerLoadout, Ragdoll, RagdollPhase, RagdollTuning, Roster, WobbleInput,
    WobbleTuning, paw_nodes, primitive_paw, spawn_animal_on_foot, transform_in,
};
use crate::interactions::arm_and_hand;
use crate::interpolation::{InterpolatedFrom, PhysicsPose};
use crate::kart::{KartColor, KeyboardLayout, combine, gamepad_input, keyboard_input};
use crate::level::LevelScoped;
use crate::menu::MenuAssets;
use crate::menu::kit;
use crate::net::{
    FootStrike, FootSync, KartSync, LobbyWindow, NetSession, NetSettings, PeerKey, TICK_MS,
    character_name, foot::foot_position, foot::strike_direction, tags,
};
use crate::options::OptionsMenu;
use crate::track::circuit::to_mesh;

/// A character's body at size 1 (half its extents, m): its animal's block on its legs, about a
/// meter high (`DriverTuning::model_scale` times a Cube Pet's 1.25 × 1.58 × 1.35).
const WALKER_HALF: Vec3 = Vec3::new(0.38, 0.49, 0.40);
/// Its edges' rounding (m).
const WALKER_ROUNDING: f32 = 0.08;
/// Below this, a character fell out of the clearing: back to its seat (m).
const FALL_RESET_HEIGHT: f32 = -10.0;
/// The pace of the clips (m/s at which they play at their own speed).
const WALK_PACE: f32 = 1.6;
const RUN_PACE: f32 = 4.0;
/// The « pof » shows this long (s) and this big (m).
const POF_TIME: f32 = 0.3;
const POF_SIZE: f32 = 0.7;
/// The tag over a character: this high over its body's center, at size 1 (m).
const TAG_HEIGHT: f32 = 1.25;
/// The hat on its head (in the animal's file units: it is scaled with it).
const HAT_RADIUS: f32 = 0.32;
const HAT_HEIGHT: f32 = 0.75;
/// The top of a Cube Pet's body (its `body` node's space, file units).
const HEAD_TOP: f32 = 1.38;

/// A character on foot of the lobby (its body): whose it is, its animal, its color, its pseudo.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(LevelScoped)]
pub struct Walker {
    pub peer: PeerKey,
    pub character: usize,
    pub color: KartColor,
    pub name: String,
    /// Its weight class's size (1 = medium).
    pub size: f32,
}

/// Ours: the keys move it, the camera follows it, its states go to the others.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PlayerWalker;

/// What a character simulated here does: where it faces, whether it stands on something, its
/// keys' latches, its paw.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkerControl {
    /// Its yaw (rad: 0 faces -Z).
    pub heading: f32,
    pub grounded: bool,
    jump_held: bool,
    paw_held: bool,
    /// Its paw's swing: ticks since it began, on its right (else its left).
    pub swing: Option<(u32, bool)>,
    /// Ticks before its paw can swing again.
    pub cooldown: u32,
    /// Swings, and strikes it sent toward a character (stats, for the tests and the console).
    pub swings: u32,
    pub strikes: u32,
}

/// How a character looks, ours or another's (from its ragdoll and control here, from its states
/// for another): what its visual, its animation, its wobble and its « pof » follow.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkerLook {
    pub phase: RagdollPhase,
    pub phase_ticks: u32,
    pub airborne: bool,
    pub swing: Option<(u32, bool)>,
    /// Its horizontal speed (m/s), its velocity.
    pub speed: f32,
    pub velocity: Vec3,
    /// Falls so far (a new one kicks its wobble and pops the « pof »).
    pub knocks: u32,
}

/// Our character's positions by common tick: the victim's check of a strike.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WalkerHistory(pub crate::net::PoseHistory);

/// The visual of a character: it follows its body.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(LevelScoped)]
pub struct WalkerVisual {
    pub body: Entity,
}

/// What stands under a visual: its animal (or primitives), for which character, and what it
/// plays.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WalkerFigure {
    pub character: usize,
    pub animal: bool,
    pub playing: AnimalAnimation,
    pub speed: f32,
    pub nodes: Option<AnimationNodes>,
    /// Issue #169: its animal's body (where its paw comes out) and a bird's wings (its left, then
    /// its right), which its paw replaces while out.
    pub body: Option<Entity>,
    pub wings: [Option<Entity>; 2],
}

/// The party hat of a character (its body), its player's color.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkerHat {
    pub body: Entity,
    pub color: KartColor,
}

/// A part of a character's paw (the arm, or the hand at its end), and its « pof».
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkerPaw {
    pub ball: bool,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkerPof {
    /// Falls seen, and when the last one popped (s).
    seen: u32,
    since: f32,
}

/// The tag over a character (a UI node): its body, its nodes, what it writes.
#[derive(Component, Clone, Debug)]
#[require(LevelScoped)]
pub struct WalkerTag {
    pub body: Entity,
    pub name: String,
    pub color: KartColor,
    pill: Entity,
    text: Entity,
}

/// The meshes and materials shared by every character of the lobby.
#[derive(Resource, Clone, Debug)]
pub struct SalonAssets {
    hat: Handle<Mesh>,
    pompom: Handle<Mesh>,
    block: Handle<Mesh>,
    /// The paw of a character in primitives (issue #169: an animal's is its own).
    arm: Handle<Mesh>,
    ball: Handle<Mesh>,
    paw_length: f32,
    pof: Handle<Mesh>,
    /// A hat's material per color of the palette.
    hats: Vec<Handle<StandardMaterial>>,
    white: Handle<StandardMaterial>,
    paw: Handle<StandardMaterial>,
    pof_material: Handle<StandardMaterial>,
}

impl SalonAssets {
    fn hat_of(&self, color: KartColor) -> Handle<StandardMaterial> {
        self.hats[usize::from(color.index()) % self.hats.len()].clone()
    }
}

/// A character's body at `at` (its bottom on the ground), facing `heading`, `size` big: upright,
/// its rotation locked (it turns about the vertical by its walk), sliding along what it touches
/// (it is driven by its velocity), bouncy and rough only while it tumbles (its ragdoll).
pub fn walker_body(at: Vec3, heading: f32, size: f32, tuning: &SalonTuning) -> impl Bundle {
    let half = WALKER_HALF * size;
    let transform = Transform::from_translation(at + Vec3::Y * (half.y + 0.02))
        .with_rotation(Quat::from_rotation_y(heading));
    let friction = Friction {
        coefficient: 0.0,
        combine_rule: CoefficientCombineRule::Min,
    };
    let restitution = Restitution {
        coefficient: 0.0,
        combine_rule: CoefficientCombineRule::Min,
    };
    let rounding = WALKER_ROUNDING * size;
    (
        transform,
        PhysicsPose::new(transform),
        RigidBody::Dynamic,
        Collider::round_cuboid(
            half.x - rounding,
            half.y - rounding,
            half.z - rounding,
            rounding,
        ),
        Velocity::default(),
        bevy_rapier3d::prelude::GravityScale(tuning.gravity_scale),
        LockedAxes::ROTATION_LOCKED,
        friction,
        restitution,
        Ragdoll::new(LockedAxes::ROTATION_LOCKED, restitution, friction),
        Ccd::enabled(),
        Sleeping::disabled(),
        WalkerLook::default(),
    )
}

/// The size of a character (its weight class's).
fn size_of(roster: &Roster, character: usize) -> f32 {
    roster
        .characters
        .get(character)
        .map_or(1.0, |c| c.size)
        .max(0.3)
}

/// Spawns ours, once the scene is up: on its seat's spot (its place in the room), facing the
/// middle, with its visual.
#[allow(clippy::too_many_arguments)]
pub fn spawn_our_walker(
    mut commands: Commands,
    ours: Query<(), With<PlayerWalker>>,
    session: Option<Res<NetSession>>,
    (roster, loadout, color, settings): (
        Res<Roster>,
        Res<PlayerLoadout>,
        Option<Res<PlayerColor>>,
        Res<NetSettings>,
    ),
    tuning: Res<SalonTuning>,
) {
    if !ours.is_empty() {
        return;
    }
    let lobby = session.as_deref().and_then(|s| s.lobby.as_ref());
    let me = lobby.map_or(PeerKey(0), |lobby| lobby.me());
    let seat = lobby
        .and_then(|lobby| lobby.seats().iter().position(|key| *key == me))
        .unwrap_or(0);
    let (at, heading) = seat_spot(seat);
    let character = loadout.character;
    let color = lobby
        .and_then(|lobby| lobby.color_of(me))
        .or(color.map(|c| c.0))
        .unwrap_or_default();
    let name = settings
        .pseudo
        .clone()
        .unwrap_or_else(|| character_name(Some(&loadout.0), Some(&roster)));
    let size = size_of(&roster, character);
    let body = commands
        .spawn((
            Name::new("Salon: notre personnage"),
            Walker {
                peer: me,
                character,
                color,
                name,
                size,
            },
            PlayerWalker,
            FootInput::default(),
            WalkerControl {
                heading,
                ..default()
            },
            WalkerHistory::default(),
            walker_body(at, heading, size, &tuning),
        ))
        .id();
    commands.spawn((
        Name::new("Salon: notre personnage (visuel)"),
        WalkerVisual { body },
        InterpolatedFrom(body),
        Transform::from_translation(at),
        Visibility::default(),
    ));
}

/// The meshes and materials of the characters, made once.
fn salon_assets(
    made: &mut Option<SalonAssets>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> SalonAssets {
    if let Some(made) = made.as_ref() {
        return made.clone();
    }
    let paw = primitive_paw();
    let assets = SalonAssets {
        hat: meshes.add(Cone::new(HAT_RADIUS, HAT_HEIGHT)),
        pompom: meshes.add(Sphere::new(0.12)),
        block: meshes.add(Cuboid::from_size(WALKER_HALF * 2.0)),
        arm: meshes.add(to_mesh(&paw.arm)),
        ball: meshes.add(to_mesh(&paw.hand)),
        paw_length: paw.hand_length,
        pof: meshes.add(Cuboid::new(1.0, 1.0, 0.08)),
        hats: KartColor::ALL
            .iter()
            .map(|color| materials.add(color.color()))
            .collect(),
        white: materials.add(Color::WHITE),
        // The primitives' paw is cream in its vertex colors.
        paw: materials.add(Color::WHITE),
        pof_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.95, 0.35),
            unlit: true,
            ..default()
        }),
    };
    *made = Some(assets.clone());
    assets
}

/// Dresses each visual: its animal on its legs once its model is ready (primitives without
/// one: headless, a missing file), its hat, its paw and its « pof »; again when its character
/// changes (another animal chosen in « Pilote »), its hat repainted when its color does.
#[allow(clippy::too_many_arguments)]
pub fn dress_walkers(
    mut commands: Commands,
    mut made: Local<Option<SalonAssets>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    (animals, driver_tuning, looks): (Res<Animals>, Res<DriverTuning>, Option<Res<DriverLooks>>),
    visuals: Query<(Entity, &WalkerVisual, Option<&Children>)>,
    walkers: Query<&Walker>,
    figures: Query<&WalkerFigure>,
    mut hats: Query<(&mut WalkerHat, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    let assets = salon_assets(&mut made, &mut meshes, &mut materials);
    // Each hat follows its player's color.
    for (mut hat, mut material) in &mut hats {
        if let Ok(walker) = walkers.get(hat.body)
            && walker.color != hat.color
        {
            hat.color = walker.color;
            material.0 = assets.hat_of(walker.color);
        }
    }
    for (visual, of, children) in &visuals {
        let Ok(walker) = walkers.get(of.body) else {
            continue;
        };
        let figure = children
            .into_iter()
            .flatten()
            .find_map(|child| figures.get(*child).ok().map(|f| (*child, *f)));
        let wanted_animal = animals.get(walker.character).is_some();
        let waiting = animals.waiting() && animals.get(walker.character).is_none();
        match figure {
            Some((_, figure))
                if figure.character == walker.character && figure.animal == wanted_animal =>
            {
                continue;
            }
            Some((entity, _)) => {
                commands.entity(entity).despawn();
            }
            None if waiting => continue,
            None => {
                // Its paw and its « pof », once.
                spawn_paw(&mut commands, &assets, visual, walker.size);
            }
        }
        let size = walker.size;
        let figure = WalkerFigure {
            character: walker.character,
            animal: wanted_animal,
            playing: AnimalAnimation::Idle,
            speed: 1.0,
            nodes: None,
            body: None,
            wings: [None; 2],
        };
        match animals.get(walker.character) {
            Some(animal) => {
                let scale = driver_tuning.model_scale.max(0.01) * size;
                let placement = Transform {
                    translation: Vec3::new(0.0, -WALKER_HALF.y * size, 0.0),
                    rotation: Quat::from_rotation_y(std::f32::consts::PI),
                    scale: Vec3::splat(scale),
                };
                let phase =
                    (of.body.to_bits() as u32).wrapping_mul(2_654_435_761) as f32 / u32::MAX as f32;
                let figure = WalkerFigure {
                    nodes: animal.graph.as_ref().map(|(_, nodes)| *nodes),
                    ..figure
                };
                let (root, nodes) = spawn_animal_on_foot(
                    &mut commands,
                    animal,
                    placement,
                    visual,
                    visual,
                    phase,
                    figure,
                );
                // Issue #169: where its paw comes out.
                let (body, wings) = paw_nodes(animal, &nodes);
                commands.entity(root).insert(WalkerFigure {
                    body,
                    wings,
                    ..figure
                });
                // Its hat on the top of its head: under the body's node, which the clips and
                // the wobble move.
                let head = animal
                    .data
                    .node("body")
                    .and_then(|index| nodes.get(index).copied())
                    .unwrap_or(root);
                commands.spawn((
                    hat_bundle(&assets, of.body, walker.color),
                    Transform::from_translation(Vec3::Y * (HEAD_TOP + HAT_HEIGHT / 2.0)),
                    ChildOf(head),
                ));
            }
            None => {
                // Primitives: its block, its hat on it.
                let material = looks
                    .as_deref()
                    .and_then(|looks| looks.get(walker.character))
                    .map_or_else(|| assets.white.clone(), |look| look.material.clone());
                commands
                    .spawn((
                        figure,
                        Transform::from_scale(Vec3::splat(size)),
                        Visibility::default(),
                        ChildOf(visual),
                    ))
                    .with_children(|figure| {
                        figure.spawn((Mesh3d(assets.block.clone()), MeshMaterial3d(material)));
                        let scale = driver_tuning.model_scale;
                        figure.spawn((
                            hat_bundle(&assets, of.body, walker.color),
                            // The hat's mesh is in the animal's units: back to meters.
                            Transform::from_translation(
                                Vec3::Y * (WALKER_HALF.y + HAT_HEIGHT / 2.0 * scale),
                            )
                            .with_scale(Vec3::splat(scale)),
                        ));
                    });
            }
        }
    }
}

/// The party hat of `body`'s character, of `color` (its center at its parent's origin: the
/// caller places it), its pompom on top.
fn hat_bundle(assets: &SalonAssets, body: Entity, color: KartColor) -> impl Bundle {
    (
        WalkerHat { body, color },
        Mesh3d(assets.hat.clone()),
        MeshMaterial3d(assets.hat_of(color)),
        Visibility::default(),
        children![(
            Mesh3d(assets.pompom.clone()),
            MeshMaterial3d(assets.white.clone()),
            Transform::from_xyz(0.0, HAT_HEIGHT / 2.0, 0.0),
        )],
    )
}

/// A character's paw (an arm and a ball, hidden at rest) and its « pof », under its visual.
fn spawn_paw(commands: &mut Commands, assets: &SalonAssets, visual: Entity, size: f32) {
    commands.spawn((
        WalkerPaw { ball: false },
        Mesh3d(assets.arm.clone()),
        MeshMaterial3d(assets.paw.clone()),
        Transform::from_scale(Vec3::splat(size)),
        Visibility::Hidden,
        ChildOf(visual),
    ));
    commands.spawn((
        WalkerPaw { ball: true },
        Mesh3d(assets.ball.clone()),
        MeshMaterial3d(assets.paw.clone()),
        Transform::from_scale(Vec3::splat(size)),
        Visibility::Hidden,
        ChildOf(visual),
    ));
    // Its meshes' lengths, for its pose (`pose_walker_paws`).
    commands.entity(visual).insert(PrimitivePaw {
        arm: assets.arm.clone(),
        hand: assets.ball.clone(),
        material: assets.paw.clone(),
        length: assets.paw_length,
    });
    commands.spawn((
        WalkerPof::default(),
        Mesh3d(assets.pof.clone()),
        MeshMaterial3d(assets.pof_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        ChildOf(visual),
    ));
}

/// Our character's keys, each tick: the race's (the keyboard's layout, every gamepad's stick,
/// A/B, RT, RB). None while a screen has them (the lobby's, the options, « Pilote »...).
#[allow(clippy::too_many_arguments)]
pub fn read_foot_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    layout: Option<Res<KeyboardLayout>>,
    gamepads: Query<&Gamepad>,
    window: Option<Res<LobbyWindow>>,
    options: Option<Res<OptionsMenu>>,
    selection: Option<Res<crate::characters::SelectionScreen>>,
    menu: Option<Res<crate::menu::MainMenu>>,
    capture: Option<Res<crate::admin::KeyboardCapture>>,
    mut ours: Query<&mut FootInput, With<PlayerWalker>>,
) {
    let busy = window.is_some_and(|w| w.open)
        || options.is_some_and(|o| o.open)
        || selection.is_some_and(|s| s.open)
        || menu.is_some_and(|m| m.open)
        || capture.is_some_and(|c| c.0);
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let mut race = keys
        .as_deref()
        .map(|keys| keyboard_input(keys, layout))
        .unwrap_or_default();
    let mut stick_y: f32 = 0.0;
    for gamepad in &gamepads {
        race = combine(race, gamepad_input(gamepad));
        let y = stick_axis(gamepad.left_stick().y);
        if y.abs() > stick_y.abs() {
            stick_y = y;
        }
    }
    let input = if busy {
        FootInput::default()
    } else {
        foot_input(&race, stick_y)
    };
    for mut foot in &mut ours {
        foot.set_if_neq(input);
    }
}

/// The characters simulated here walk, turn, jump and bounce (`walk.rs`), upright; tumbling,
/// the physics and their ragdoll move them. One fallen out of the clearing is back on its seat.
#[allow(clippy::too_many_arguments)]
pub fn walk_walkers(
    time: Res<Time>,
    tuning: Res<SalonTuning>,
    rapier: ReadRapierContext,
    bouncers: Query<&Bouncer>,
    session: Option<Res<NetSession>>,
    mut walkers: Query<(
        Entity,
        &Walker,
        &FootInput,
        &mut WalkerControl,
        &mut Ragdoll,
        &mut Velocity,
        &mut Transform,
        &mut PhysicsPose,
        &mut WalkerLook,
    )>,
) {
    let dt = time.delta_secs();
    let context = rapier.single().ok();
    let swing_ticks = (tuning.paw_swing * crate::TICK_HZ as f32).round() as u32;
    for (
        entity,
        walker,
        input,
        mut control,
        mut ragdoll,
        mut velocity,
        mut transform,
        mut pose,
        mut look,
    ) in &mut walkers
    {
        let half = WALKER_HALF * walker.size;
        if transform.translation.y < FALL_RESET_HEIGHT {
            let seat = session
                .as_deref()
                .and_then(|s| s.lobby.as_ref())
                .and_then(|lobby| lobby.seats().iter().position(|key| *key == walker.peer))
                .unwrap_or(0);
            let (at, heading) = seat_spot(seat);
            *transform = Transform::from_translation(at + Vec3::Y * (half.y + 0.02))
                .with_rotation(Quat::from_rotation_y(heading));
            pose.snap(*transform);
            *velocity = Velocity::zero();
            *ragdoll = Ragdoll::new(
                ragdoll.upright_lock,
                ragdoll.upright_restitution,
                ragdoll.upright_friction,
            );
            control.heading = heading;
            continue;
        }
        // Standing on something (fixed: the lawn, a donut, a rock), its bottom within reach.
        control.grounded = context.as_ref().is_some_and(|context| {
            context
                .cast_ray(
                    transform.translation,
                    Vec3::NEG_Y,
                    half.y + 0.12,
                    true,
                    QueryFilter::only_fixed().exclude_rigid_body(entity),
                )
                .is_some()
        });
        let jump = input.jump && !control.jump_held;
        control.jump_held = input.jump;
        if ragdoll.upright() {
            velocity.linear = walk_velocity(
                velocity.linear,
                input.direction,
                control.grounded,
                jump,
                &tuning,
                dt,
            );
            if let Some(wanted) = heading_toward(super::walk::world_direction(input.direction)) {
                control.heading = turned(control.heading, wanted, tuning.turn_rate, dt);
            }
            let rotation = Quat::from_rotation_y(control.heading);
            if transform.rotation != rotation {
                transform.rotation = rotation;
            }
            velocity.angular = Vec3::ZERO;
        } else {
            // Back on its feet, it faces the way it got up.
            control.heading = crate::characters::ragdoll::heading_of(transform.rotation);
        }
        // A giant donut under it: up it goes (tumbling too).
        let feet = transform.translation - Vec3::Y * half.y.min(half.x);
        if bouncers
            .iter()
            .any(|bouncer| bouncer.catches(feet, velocity.linear.y))
        {
            velocity.linear.y = tuning.bounce_speed;
        }
        // Its paw's swing runs on; its recharge too.
        control.swing = control
            .swing
            .map(|(ticks, right)| (ticks + 1, right))
            .filter(|(ticks, _)| *ticks <= swing_ticks);
        control.cooldown = control.cooldown.saturating_sub(1);
        let flat = Vec3::new(velocity.linear.x, 0.0, velocity.linear.z);
        let next = WalkerLook {
            phase: ragdoll.phase,
            phase_ticks: (ragdoll.elapsed * crate::TICK_HZ as f32).round() as u32,
            airborne: !control.grounded,
            swing: control.swing,
            speed: flat.length(),
            velocity: velocity.linear,
            knocks: ragdoll.knocks,
        };
        look.set_if_neq(next);
    }
}

/// The paws of the characters simulated here: on the Interaction button (its press, its
/// recharge ready, upright), a swing toward the nearest one in front (as it is shown here). A
/// character simulated here falls over at once (its ragdoll decides: not protected); another
/// player's is told (`net::FootStrike`): its peer decides.
#[allow(clippy::too_many_arguments)]
pub fn swing_paws(
    tuning: Res<SalonTuning>,
    ragdoll_tuning: Res<RagdollTuning>,
    time: Res<Time<Fixed>>,
    virtual_time: Res<Time<Virtual>>,
    sync: Option<Res<KartSync>>,
    mut foot: Option<ResMut<FootSync>>,
    remotes: Query<&RemoteWalkerBody>,
    looks: Query<&WalkerLook>,
    mut walkers: Query<(
        Entity,
        &Walker,
        &Transform,
        Option<&FootInput>,
        Option<&mut WalkerControl>,
        Option<&mut Ragdoll>,
    )>,
) {
    let local = (time.elapsed_secs_f64() * crate::TICK_HZ).round() as u32;
    let now_ms = virtual_time.elapsed_secs_f64() * 1_000.0;
    let mut strikers: Vec<(Entity, Vec3, f32)> = Vec::new();
    for (entity, _, transform, input, control, ragdoll) in &mut walkers {
        let (Some(input), Some(mut control), Some(ragdoll)) = (input, control, ragdoll) else {
            continue;
        };
        let pressed = input.paw && !control.paw_held;
        control.paw_held = input.paw;
        if pressed && control.cooldown == 0 && ragdoll.upright() {
            strikers.push((entity, transform.translation, control.heading));
        }
    }
    for (striker, at, heading) in strikers {
        let candidates: Vec<PawCandidate<Entity>> = walkers
            .iter()
            .filter(|(entity, ..)| *entity != striker)
            .map(|(entity, _, transform, _, _, ragdoll)| {
                let open = match ragdoll {
                    Some(ragdoll) => !ragdoll.protected(&ragdoll_tuning),
                    None => looks
                        .get(entity)
                        .is_ok_and(|look| look.phase == RagdollPhase::Upright),
                };
                PawCandidate {
                    id: entity,
                    position: transform.translation,
                    open,
                }
            })
            .collect();
        let target = paw_target(
            at,
            heading,
            candidates,
            (
                tuning.paw_reach,
                tuning.paw_cone.to_radians(),
                tuning.paw_height,
            ),
        );
        let to = target.map_or(super::walk::front_of(heading), |t| t.position - at);
        let right = on_the_right(heading, to);
        if let Ok((_, _, _, _, Some(mut control), _)) = walkers.get_mut(striker) {
            control.swing = Some((0, right));
            control.cooldown = (tuning.paw_cooldown * crate::TICK_HZ as f32).round() as u32;
            control.swings += 1;
            if target.is_some() {
                control.strikes += 1;
            }
        }
        let Some(target) = target else {
            info!("lobby: a paw in the air");
            continue;
        };
        if let Ok((_, walker, ..)) = walkers.get(target.id) {
            info!("lobby: our paw on {}", walker.name);
        }
        let direction = Vec3::new(to.x, 0.0, to.z).normalize_or(super::walk::front_of(heading));
        match remotes.get(target.id) {
            Ok(remote) => {
                // Another player's: its peer decides.
                let (Some(foot), Some(sync)) = (foot.as_deref_mut(), sync.as_deref()) else {
                    continue;
                };
                let Some(tick) = sync.clock.common(local) else {
                    continue;
                };
                let seen = foot
                    .walker(remote.peer)
                    .and_then(|walker| walker.shown_tick(now_ms - TICK_MS))
                    .map_or(tick, |seen| (seen.max(0.0) as u32).min(tick));
                foot.send_strike(FootStrike {
                    target: remote.peer,
                    tick,
                    seen,
                    position: foot_position(at),
                    direction: strike_direction(direction),
                });
            }
            Err(_) => {
                // Simulated here: it falls over now (unless it is protected).
                if let Ok((_, _, _, _, _, Some(mut ragdoll))) = walkers.get_mut(target.id) {
                    ragdoll.knock(
                        Knock {
                            direction,
                            strength: 1.0,
                            twist_left: local.is_multiple_of(2),
                        },
                        &ragdoll_tuning,
                    );
                }
            }
        }
    }
}

/// What each figure plays: standing, walking or running at the pace of its steps, paddling
/// while it tumbles, a shake of the head while it gets up. A new animation fades in; the pace
/// changes in place.
pub fn animate_walkers(
    tuning: Res<SalonTuning>,
    driver_tuning: Res<DriverTuning>,
    visuals: Query<&WalkerVisual>,
    looks: Query<&WalkerLook>,
    mut figures: Query<(
        &ChildOf,
        &mut WalkerFigure,
        Option<&mut AnimationPlayer>,
        Option<&mut AnimationTransitions>,
    )>,
) {
    for (child_of, mut figure, player, transitions) in &mut figures {
        let (Some(nodes), Some(mut player), Some(mut transitions)) =
            (figure.nodes, player, transitions)
        else {
            continue;
        };
        let Some(look) = visuals
            .get(child_of.parent())
            .ok()
            .and_then(|visual| looks.get(visual.body).ok())
        else {
            continue;
        };
        let (wanted, speed) = match look.phase {
            RagdollPhase::Tumbling => (AnimalAnimation::Run, 2.2),
            RagdollPhase::GettingUp => (AnimalAnimation::Shake, 1.4),
            RagdollPhase::Upright if look.airborne => (AnimalAnimation::Idle, 1.0),
            RagdollPhase::Upright if look.speed > 0.55 * tuning.walk_speed => (
                AnimalAnimation::Run,
                (look.speed / RUN_PACE).clamp(0.6, 1.8),
            ),
            RagdollPhase::Upright if look.speed > 0.3 => (
                AnimalAnimation::Walk,
                (look.speed / WALK_PACE).clamp(0.5, 1.8),
            ),
            RagdollPhase::Upright => (AnimalAnimation::Idle, driver_tuning.idle_speed),
        };
        let node = nodes[wanted.index()];
        if wanted != figure.playing {
            figure.playing = wanted;
            figure.speed = speed;
            let fade = Duration::from_secs_f32(driver_tuning.crossfade_seconds.max(0.01));
            transitions
                .play(&mut player, node, fade)
                .set_speed(speed)
                .repeat();
        } else if (speed - figure.speed).abs() > 0.1 * figure.speed {
            figure.speed = speed;
            if let Some(active) = player.animation_mut(node) {
                active.set_speed(speed);
            }
        }
    }
}

/// Each visual's wobble (#12) moves on by this frame, from where the visual is shown: a fall
/// kicks its mini-ragdoll (the head thrown along the push), its landings squash it.
pub fn step_walker_wobbles(
    time: Res<Time>,
    tuning: Res<WobbleTuning>,
    looks: Query<&WalkerLook>,
    mut visuals: Query<(&WalkerVisual, &Transform, &mut DriverWobble)>,
) {
    let dt = time.delta_secs();
    for (visual, transform, mut wobble) in &mut visuals {
        let look = looks.get(visual.body).copied().unwrap_or_default();
        let toward = Vec3::new(look.velocity.x, 0.0, look.velocity.z).normalize_or(Vec3::NEG_Z);
        let input = WobbleInput {
            dt,
            position: transform.translation,
            rotation: transform.rotation,
            hit: None,
            struck: Some((look.knocks, toward)),
            drifting: false,
        };
        wobble.0.step(&input, &tuning);
    }
}

/// Issue #169: a visual's paw in primitives (its meshes and their material, how long its hand
/// is), for a character without its animal (headless, a missing file).
#[derive(Component, Clone, Debug)]
pub struct PrimitivePaw {
    arm: Handle<Mesh>,
    hand: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    length: f32,
}

/// The paws and the « pof »: the arm reaches out on its side and comes back over a swing; the
/// « pof » pops where a fall starts. Hidden otherwise. Issue #169: the paw is its animal's, out
/// of its body (as its clips and its wobble move it this frame), at its size; a bird's wing
/// leaves its body while it is the paw.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn pose_walker_paws(
    time: Res<Time>,
    (tuning, driver_tuning, animals): (Res<SalonTuning>, Res<DriverTuning>, Res<Animals>),
    visuals: Query<(&WalkerVisual, &PrimitivePaw, &Children)>,
    walkers: Query<(&Walker, &WalkerLook)>,
    figures: Query<&WalkerFigure>,
    nodes: Query<(&Transform, &ChildOf), (Without<WalkerPaw>, Without<WalkerPof>)>,
    mut wings: Query<&mut Visibility, (With<AnimalPart>, Without<WalkerPaw>, Without<WalkerPof>)>,
    mut paws: Query<
        (
            &ChildOf,
            &WalkerPaw,
            &mut Transform,
            &mut Visibility,
            &mut Mesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<WalkerPof>,
    >,
    mut pofs: Query<
        (&ChildOf, &mut WalkerPof, &mut Transform, &mut Visibility),
        Without<WalkerPaw>,
    >,
) {
    let swing_ticks = (tuning.paw_swing * crate::TICK_HZ as f32).max(1.0);
    let model_scale = driver_tuning.model_scale.max(0.01);
    for (child_of, paw, mut transform, mut visibility, mut mesh, mut material) in &mut paws {
        let visual = child_of.parent();
        let Some((walker, look, primitive, children)) =
            visuals
                .get(visual)
                .ok()
                .and_then(|(of, primitive, children)| {
                    let (walker, look) = walkers.get(of.body).ok()?;
                    Some((walker, look, primitive, children))
                })
        else {
            continue;
        };
        let size = walker.size;
        // Its animal's paw, out of its body; or its primitives'.
        let figure = children.iter().find_map(|c| figures.get(c).ok());
        let animal = figure.and_then(|figure| {
            let animal = animals.get(figure.character)?;
            let paw = animal.paw.as_ref()?;
            let body = transform_in(figure.body?, visual, &nodes)?;
            Some((paw.shoulders(&body), paw, &animal.material, figure.wings))
        });
        let swing = look.swing.filter(|_| look.phase == RagdollPhase::Upright);
        if !paw.ball
            && let Some((_, _, _, wing_nodes)) = &animal
        {
            for (i, wing) in wing_nodes.iter().enumerate() {
                let out = swing.is_some_and(|(_, right)| usize::from(right) == i);
                if let Some(mut shown) = wing.and_then(|w| wings.get_mut(w).ok()) {
                    shown.set_if_neq(if out {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    });
                }
            }
        }
        let Some((ticks, right)) = swing else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        // Out in the first third, back over the rest.
        let t = ticks as f32 / swing_ticks;
        let out = if t < 0.35 {
            t / 0.35
        } else {
            1.0 - (t - 0.35) / 0.65
        }
        .clamp(0.0, 1.0);
        let side = if right { 1.0 } else { -1.0 };
        let (shoulder, hand_length, arm_mesh, hand_mesh, shared) = match &animal {
            Some((shoulders, paw, shared, _)) => (
                shoulders[usize::from(right)],
                paw.hand_length,
                &paw.arm,
                &paw.hand,
                *shared,
            ),
            None => (
                Vec3::new(0.32 * side, 0.15, -0.2) * size,
                primitive.length,
                &primitive.arm,
                &primitive.hand,
                &primitive.material,
            ),
        };
        let reach = Vec3::new(0.25 * side, 0.1, -1.0).normalize() * (0.2 + 1.1 * out) * size;
        let (arm_at, hand_at) = arm_and_hand(
            shoulder,
            shoulder + reach,
            side,
            model_scale * size,
            hand_length,
        );
        let wanted = if paw.ball { hand_mesh } else { arm_mesh };
        if mesh.0 != *wanted {
            mesh.0 = wanted.clone();
        }
        if material.0 != *shared {
            material.0 = shared.clone();
        }
        transform.set_if_neq(if paw.ball { hand_at } else { arm_at });
        visibility.set_if_neq(Visibility::Inherited);
    }
    let dt = time.delta_secs();
    for (child_of, mut pof, mut transform, mut visibility) in &mut pofs {
        let Some((walker, look)) = visuals
            .get(child_of.parent())
            .ok()
            .and_then(|(visual, ..)| walkers.get(visual.body).ok())
        else {
            continue;
        };
        if look.knocks != pof.seen {
            pof.seen = look.knocks;
            pof.since = 0.0;
        } else {
            pof.since += dt;
        }
        if look.knocks == 0 || pof.since > POF_TIME {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let grow = (pof.since / POF_TIME * 2.0).min(1.0);
        let at = Vec3::new(0.0, 0.3, 0.0) * walker.size;
        transform.set_if_neq(
            Transform::from_translation(at)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
                .with_scale(Vec3::splat(POF_SIZE * grow * walker.size)),
        );
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// The camera follows ours, from a fixed angle (behind it, toward -Z, and above), smoothly;
/// before ours is there, the middle of the clearing.
pub fn film_the_salon(
    time: Res<Time>,
    tuning: Res<SalonTuning>,
    visuals: Query<(&WalkerVisual, &Transform), Without<ChaseCamera>>,
    players: Query<(), With<PlayerWalker>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<ChaseCamera>>,
    mut focus: Local<Option<Vec3>>,
) {
    let target = visuals
        .iter()
        .find(|(visual, _)| players.contains(visual.body))
        .map_or(Vec3::ZERO, |(_, transform)| transform.translation);
    let dt = time.delta_secs();
    let follow = 1.0 - (-tuning.camera_follow.max(0.0) * dt).exp();
    let at = match *focus {
        Some(at) if at.distance(target) < 15.0 => at.lerp(target, follow),
        _ => target,
    };
    *focus = Some(at);
    let look_at = at + Vec3::Y * 0.6;
    let eye = look_at + Vec3::new(0.0, tuning.camera_height, tuning.camera_distance);
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_translation(eye).looking_at(look_at, Vec3::Y);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = tuning.camera_fov.to_radians();
        }
    }
}

/// The tags over the characters: each one's pseudo, ours too; spawned with its visual, moved
/// each frame (`UiTransform`), hidden behind the camera or with the option off.
#[allow(clippy::too_many_arguments)]
pub fn follow_walker_tags(
    mut commands: Commands,
    assets: Option<Res<MenuAssets>>,
    options: Option<Res<crate::options::PlayerOptions>>,
    cameras: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    visuals: Query<(&WalkerVisual, &GlobalTransform)>,
    walkers: Query<&Walker>,
    mut tags: Query<(Entity, &mut WalkerTag, &mut UiTransform, &mut Visibility)>,
    mut borders: Query<&mut BorderColor>,
    mut texts: Query<&mut Text>,
) {
    let enabled = options.is_none_or(|options| options.name_tags);
    let camera = cameras.iter().next();
    let mut tagged: u64 = 0;
    for (entity, mut tag, mut transform, mut visibility) in &mut tags {
        let Some((index, (visual, global))) = visuals
            .iter()
            .enumerate()
            .find(|(_, (visual, _))| visual.body == tag.body)
        else {
            commands.entity(entity).try_despawn();
            continue;
        };
        tagged |= 1 << (index % 64);
        let Ok(walker) = walkers.get(visual.body) else {
            continue;
        };
        if tag.name != walker.name {
            tag.name.clone_from(&walker.name);
            if let Ok(mut text) = texts.get_mut(tag.text) {
                tags::write_tag_text(&mut text.0, &walker.name);
            }
        }
        if tag.color != walker.color {
            tag.color = walker.color;
            if let Ok(mut border) = borders.get_mut(tag.pill) {
                *border = tags::border_of(walker.color);
            }
        }
        let place = camera.and_then(|(camera, eye)| {
            let over = tags::tag_anchor(global.translation(), Vec3::Y, TAG_HEIGHT * walker.size);
            let on_screen = camera.world_to_viewport_with_depth(eye, over).ok()?;
            let distance = eye.translation().distance(over);
            tags::tag_shown(enabled, on_screen.z, distance, 80.0)
                .then(|| tags::tag_corner(on_screen.truncate()))
        });
        match place {
            Some(corner) => {
                let translation = Val2::px(corner.x, corner.y);
                if transform.translation != translation {
                    transform.translation = translation;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (index, (visual, _)) in visuals.iter().enumerate() {
        if tagged & (1 << (index % 64)) != 0 {
            continue;
        }
        let Ok(walker) = walkers.get(visual.body) else {
            continue;
        };
        if walker.name.is_empty() {
            continue;
        }
        let (_, face) = kit::faces(assets.as_deref());
        let nodes = tags::spawn_tag_nodes(&mut commands, &walker.name, walker.color, &face);
        commands.entity(nodes.root).insert((
            Name::new(format!("Étiquette : {}", walker.name)),
            WalkerTag {
                body: visual.body,
                name: walker.name.clone(),
                color: walker.color,
                pill: nodes.pill,
                text: nodes.text,
            },
        ));
    }
}
