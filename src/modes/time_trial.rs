//! The time trial (issue #76, GD § 1, « Contre-la-montre »): alone on the circuit against the
//! clock, three piments in the hand, and the ghost of the record.
//!
//! - **The mode**: the race menu's third mode (`super::GameMode::TimeTrial`), with the circuit
//!   and the mirror of a free race. The menu inserts [`TimeTrial`] and builds the level;
//!   the next choice of mode, or a room online, ends it.
//! - **Alone**: no bot (`RaceGrid::bots` is 0 while it lasts; the player's value is kept in
//!   [`TimeTrial::solo_bots`] and given back at the end), no item box (the circuit's boxes are
//!   taken away as it is built: no random item), the croquettes stay (like MK8DX). With nobody
//!   else, no interaction either.
//! - **Three piments** (`RaceTuning::time_trial_chilis`): in the hand at each (re)start of the
//!   race, an « x3 » used one press at a time, and nothing else.
//! - **The clock**: the total time and the time of each lap (on the right, under the chrono); the
//!   record of the circuit and mirror (`trial.rs`; issue #157: of the circuit as built, by its
//!   fingerprint, `TrackLayout::fingerprint`) at the start and at the finish, with « RECORD ! »
//!   when it is beaten. The records and their ghosts are kept between sessions
//!   (`admin::storage`, [`super::TIME_TRIAL_STORAGE_KEY`]), when the plugin has the storage.
//! - **The ghost** (`ghost.rs`): each run is recorded tick by tick (the transforms, quantized like
//!   the network's states), in a buffer allocated once; a faster run becomes the record with its
//!   ghost. The next runs race against it: a translucent kart that replays it, with the animal and
//!   the parts of the run (issue #69, #73), in shared translucent copies of the kart's materials
//!   (made once per material, [`GhostMaterials`]). **It touches nothing**: not a `Kart` nor a
//!   `kart::Remote`, no collider, no rigid body, no item slot: the physics, the items, the bots,
//!   the paws and the slipstream never see it (no slipstream behind it: our choice, like the
//!   ghosts of MK8DX's time trial). It shows the run's look (drift and sparks, boost flames, jumps
//!   and tricks, wheels, glider, water) and is gone once it crossed the line.
//!
//! **Only with a renderer** ([`TimeTrialPlugin`], added by `GamePlugin` like the main menu): the
//! headless world, the one of the bot races, has none of its resources, systems or entities
//! (one more archetype before the grid would change every bot race, `STATUS.md`). The tests add
//! it themselves, without the storage.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::ghost::{GhostRecorder, GhostTrack};
use super::trial::{
    GAME_VERSION, TIME_TRIAL_STORAGE_KEY, TrialKey, TrialRecord, TrialRecords, Verdict,
};
use crate::admin::storage;
use crate::characters::{DriverLooks, Loadout, PlayerLoadout, Roster};
use crate::interpolation::InterpolationSystems;
use crate::items::{ItemKind, ItemSlots};
use crate::kart::{
    Boost, DriftMotion, Invincibility, ItemEffects, Kart, KartAssists, KartEnvironment, KartPaint,
    KartState, KartTuning, KartVisualAssets, KartWheels, PlayerKart, Slipstream, Stall,
    StatEffects, StatsTuning, spawn_kart_visual_with_driver,
};
use crate::level::{LevelScoped, Online};
use crate::menu::MenuAssets;
use crate::menu::kit;
use crate::menu::style::palette;
use crate::net::{KartLook, KartPose, KartSnapshot, LookParts, NetTuning};
use crate::race::{Course, Race, RaceGrid, RaceProgress, RaceSystems, RaceTuning, Racer};
use crate::track::{ItemBoxSpot, MirrorMode, TrackChoice, TrackLayout};
use crate::ui::HudText;
use crate::ui::format::{hundredths, write_time};

/// How opaque the ghost is (a share of each material's own opacity). Visual, not gameplay: a
/// constant, like the HUD's sizes.
pub const GHOST_ALPHA: f32 = 0.35;

/// Lap times shown at most (the circuits have 3 laps).
const LAP_LINES: usize = 9;

/// The time trial is on (issue #76): inserted by the race menu, removed by the next choice of
/// mode or by a room online.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeTrial {
    /// The bots of the player's grid (`RaceGrid::bots`), given back when the time trial ends.
    pub solo_bots: usize,
}

/// The run in progress: from a (re)start of the race to the next one.
#[derive(Clone, Debug, Default)]
pub struct TrialRun {
    /// Counts the runs: one more at each (re)start.
    pub number: u32,
    /// Its racer: the player's kart (without one, the tests' kart on the pole).
    pub racer: Option<Entity>,
    /// The race's flow tick last seen: it goes back when the race restarts.
    seen_tick: u32,
    /// Its race (none off a circuit).
    pub key: Option<TrialKey>,
    /// The ghost raced against: the record's when the run started.
    pub ghost: Option<Arc<GhostTrack>>,
    /// The record when the run started (ticks).
    pub record: Option<u32>,
    /// Its time (ticks) and what it did against the record, once it finished.
    pub finish: Option<(u32, Verdict)>,
}

/// The time trial's state: the records, the recording, the run.
#[derive(Resource, Debug, Default)]
pub struct TimeTrialState {
    pub records: TrialRecords,
    /// Allocated at the first run, for `ghost::GHOST_MAX_TICKS` ticks.
    recorder: Option<GhostRecorder>,
    pub run: TrialRun,
    /// The records changed: to be written.
    dirty: bool,
    /// Whether the records are read from and written to the storage.
    storage: bool,
}

impl TimeTrialState {
    /// What the run recorded so far (a frame per race tick).
    pub fn recorded(&self) -> &[KartSnapshot] {
        self.recorder.as_ref().map_or(&[], GhostRecorder::frames)
    }
}

/// The ghost's body: where its visual takes its look from (no physics). `run`: the run it races.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostKart {
    pub run: u32,
}

/// The root of the ghost's visual (`kart::spawn_kart_visual_with_driver`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GhostVisual {
    pub run: u32,
}

/// The translucent copies of the kart materials the ghost wears: one per material, made the
/// first time the ghost shows it, shared afterwards (nothing made per frame).
#[derive(Resource, Debug, Default)]
pub struct GhostMaterials {
    by_source: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    copies: HashSet<AssetId<StandardMaterial>>,
}

impl GhostMaterials {
    /// Whether `id` is one of the ghost's materials.
    pub fn is_ghost(&self, id: AssetId<StandardMaterial>) -> bool {
        self.copies.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.copies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.copies.is_empty()
    }
}

/// The time trial's HUD (`bevy_ui`, ASCII: the HUD's font): part of the level.
#[derive(Component, Clone, Copy, Debug)]
pub struct TrialHud;

/// A text of the time trial's HUD.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrialText {
    /// Top middle: the record (at the start, at the finish: « RECORD ! »).
    Record,
    /// Under it, at the finish: the run's time against the record.
    Verdict,
    /// On the right, under the chrono: the time of lap `n` (from 0).
    Lap(usize),
}

/// The time trial (see the module's documentation). `storage`: the records are read at startup
/// and written at each new record (the game); the tests keep them in memory.
pub struct TimeTrialPlugin {
    pub storage: bool,
}

impl Plugin for TimeTrialPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TimeTrialState {
            storage: self.storage,
            ..default()
        })
        .init_resource::<GhostMaterials>()
        .add_systems(
            Startup,
            load_records.run_if(|state: Res<TimeTrialState>| state.storage),
        )
        .add_systems(
            FixedUpdate,
            run_time_trial
                .after(RaceSystems::Rank)
                .run_if(resource_exists::<TimeTrial>)
                .run_if(resource_exists::<Race>),
        )
        .add_systems(
            Update,
            (
                (
                    keep_alone.run_if(not(resource_exists::<Online>)),
                    clear_item_boxes,
                    spawn_ghost,
                    (spawn_trial_hud, update_trial_hud)
                        .chain()
                        .run_if(resource_exists::<Race>),
                )
                    .chain()
                    .run_if(resource_exists::<TimeTrial>),
                store_records.run_if(|state: Res<TimeTrialState>| state.dirty),
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            show_ghost
                .in_set(InterpolationSystems)
                .run_if(any_with_component::<GhostKart>),
        )
        // Last: every mesh of the frame is there (the animal is seated in `PostUpdate`), and the
        // copies are on before the frame is drawn.
        .add_systems(
            Last,
            wear_ghost_materials.run_if(any_with_component::<GhostVisual>),
        );
    }
}

/// The records kept from the previous sessions. An unreadable text is ignored (no record), with
/// a warning.
fn load_records(mut state: ResMut<TimeTrialState>) {
    if let Some(text) = storage::load_key(TIME_TRIAL_STORAGE_KEY) {
        state.records = records_from_text(&text);
    }
}

/// The records of a kept text; an unreadable one gives none, with a warning in the console.
pub fn records_from_text(text: &str) -> TrialRecords {
    match TrialRecords::from_ron(text) {
        Ok(records) => records,
        Err(error) => {
            warn!("time trial: stored records ignored: {error}");
            TrialRecords::default()
        }
    }
}

/// A new record is kept at once.
fn store_records(mut state: ResMut<TimeTrialState>) {
    state.dirty = false;
    if state.storage {
        storage::save_key(TIME_TRIAL_STORAGE_KEY, &state.records.to_ron());
    }
}

/// No bot while it lasts: a value given in the meantime (the admin panel) becomes the player's,
/// given back at the end.
fn keep_alone(mut trial: ResMut<TimeTrial>, mut grid: ResMut<RaceGrid>) {
    if grid.bots != 0 {
        trial.solo_bots = grid.bots;
        grid.bots = 0;
    }
    if grid.lineup.is_some() {
        grid.lineup = None;
    }
}

/// No item box: the circuit's are taken away as it is built (nothing to draw an item from).
fn clear_item_boxes(mut commands: Commands, boxes: Query<Entity, With<ItemBoxSpot>>) {
    for entity in &boxes {
        commands.entity(entity).try_despawn();
    }
}

/// The run's racer: the player's kart, or (the tests drive it themselves) the one on the pole.
fn the_racer<'a>(racers: impl Iterator<Item = (Entity, &'a Racer, bool)>) -> Option<Entity> {
    racers
        .min_by_key(|(_, racer, player)| (!*player, racer.grid_slot))
        .map(|(entity, ..)| entity)
}

/// Each tick, once the race's rules ran: a (re)start of the race starts a new run (the record's
/// ghost, the piments in the hand, nothing recorded); the racer's pose is recorded; at the
/// finish, the run is judged, and a faster one is the new record with its ghost.
fn run_time_trial(
    mut state: ResMut<TimeTrialState>,
    race: Res<Race>,
    (tuning, kart_tuning): (Res<RaceTuning>, Res<KartTuning>),
    (choice, mirror, layout): (Res<TrackChoice>, Res<MirrorMode>, Option<Res<TrackLayout>>),
    loadout: Option<Res<PlayerLoadout>>,
    mut racers: Query<
        (
            (
                Entity,
                &Racer,
                Has<PlayerKart>,
                &RaceProgress,
                &mut ItemSlots,
            ),
            (&Transform, &Velocity, &KartPaint),
            (&KartState, &KartAssists, &StatEffects, &DriftMotion),
            (&Boost, &Stall, &Invincibility, &Slipstream, &KartWheels),
            (&ItemEffects, &KartEnvironment),
        ),
        With<Kart>,
    >,
) {
    let Some(racer) = the_racer(
        racers
            .iter()
            .map(|((entity, racer, player, ..), ..)| (entity, racer, player)),
    ) else {
        return;
    };
    let Ok((
        (entity, _, _, progress, mut slots),
        (transform, velocity, paint),
        (kart_state, assists, effects, motion),
        (boost, stall, invincibility, slipstream, wheels),
        (item_effects, environment),
    )) = racers.get_mut(racer)
    else {
        return;
    };
    let state = &mut *state;
    let flow_tick = race.flow.tick;
    if state.run.racer != Some(entity) || flow_tick < state.run.seen_tick {
        // Issue #157: the circuit as built, by its fingerprint (a record of another one is not
        // raced).
        let key = match (&*choice, layout.as_deref()) {
            (TrackChoice::Circuit(path), Some(layout)) => Some(TrialKey {
                circuit: path.clone(),
                mirror: mirror.0,
                fingerprint: layout.fingerprint,
            }),
            _ => None,
        };
        let record = key.as_ref().and_then(|key| state.records.get(key));
        state.run = TrialRun {
            number: state.run.number + 1,
            racer: Some(entity),
            seen_tick: flow_tick,
            ghost: record.and_then(|record| record.ghost.clone()),
            record: record.map(|record| record.time),
            key,
            finish: None,
        };
        state
            .recorder
            .get_or_insert_with(GhostRecorder::default)
            .clear();
        // Three piments in the hand (the race's restart emptied it this tick), nothing else.
        slots.clear();
        let chilis = tuning.time_trial_chilis.min(u32::from(u8::MAX)) as u8;
        if chilis > 0 {
            slots.force(ItemKind::Piment, chilis);
        }
    }
    state.run.seen_tick = flow_tick;
    if state.run.key.is_none() {
        return;
    }

    // The pose of this tick, as the network would send it.
    let own = effects.tuning(&kart_tuning);
    let mut look = KartLook::of(&LookParts {
        state: kart_state,
        sparks: kart_state.mini_turbo(&own, assists.assisted_steering),
        motion,
        boost,
        stall,
        invincibility,
        slipstream,
        slipstream_fill: slipstream.fill(&own),
        wheels,
        effects: item_effects,
        environment,
    });
    look.set_items(Some(&slots));
    let race_ticks = race.race_ticks();
    let pose = KartPose {
        tick: race_ticks,
        position: transform.translation,
        rotation: transform.rotation,
        velocity: velocity.linear,
        spin: velocity.angular,
        look,
    };
    let recorder = state.recorder.get_or_insert_with(GhostRecorder::default);
    recorder.record(
        race_ticks,
        KartSnapshot::quantize(0, &pose),
        progress.finish,
    );

    // The finish: the run against the record.
    let (Some(finish), None, Some(key)) = (progress.finish, state.run.finish, &state.run.key)
    else {
        return;
    };
    let verdict = state.records.judge(key, finish);
    if verdict.record {
        let frames = recorder.take();
        let loadout = loadout.map_or_else(Loadout::default, |loadout| loadout.0);
        let parts = [
            loadout.character,
            loadout.kart,
            loadout.wheels,
            loadout.wing,
        ]
        .map(|part| part.min(usize::from(u8::MAX)) as u8);
        let ghost = (!frames.is_empty()).then(|| {
            Arc::new(GhostTrack {
                frames,
                loadout: parts,
                color: paint.0,
            })
        });
        state.records.insert(TrialRecord {
            key: key.clone(),
            time: finish,
            laps: progress.lap_times.clone(),
            game: GAME_VERSION.to_string(),
            ghost,
        });
        state.dirty = true;
    }
    state.run.finish = Some((finish, verdict));
}

/// The ghost of the run: spawned when the run has one (its body, its visual with the run's
/// animal, parts and color); a ghost of another run goes.
fn spawn_ghost(
    mut commands: Commands,
    state: Res<TimeTrialState>,
    visuals: Option<Res<KartVisualAssets>>,
    (roster, stats_tuning, looks): (Res<Roster>, Res<StatsTuning>, Option<Res<DriverLooks>>),
    ghosts: Query<(Entity, &GhostKart)>,
    ghost_visuals: Query<(Entity, &GhostVisual)>,
) {
    let wanted = state.run.ghost.as_ref().map(|_| state.run.number);
    let mut present = false;
    for (entity, ghost) in &ghosts {
        if Some(ghost.run) == wanted {
            present = true;
        } else {
            commands.entity(entity).try_despawn();
        }
    }
    for (entity, visual) in &ghost_visuals {
        if Some(visual.run) != wanted {
            commands.entity(entity).try_despawn();
        }
    }
    let (false, Some(run), Some(ghost), Some(visuals)) =
        (present, wanted, state.run.ghost.as_ref(), visuals)
    else {
        return;
    };
    let [character, kart, wheels, wing] = ghost.loadout.map(usize::from);
    let loadout = Loadout {
        character,
        kart,
        wheels,
        wing,
    };
    let start = ghost.frames[0].pose();
    let body = commands
        .spawn((
            Name::new("Fantome"),
            GhostKart { run },
            LevelScoped,
            Transform::from_translation(start.position).with_rotation(start.rotation),
            KartPaint(ghost.color),
            (
                KartState::default(),
                DriftMotion::default(),
                Boost::default(),
                Stall::default(),
                Invincibility::default(),
                KartWheels::default(),
                KartAssists::default(),
                StatEffects::new(&roster.stats(&loadout), &stats_tuning),
                KartEnvironment::default(),
                Velocity::default(),
            ),
        ))
        .id();
    let driver = looks.as_ref().and_then(|looks| looks.get(character));
    let visual = spawn_kart_visual_with_driver(
        &mut commands,
        &visuals,
        body,
        ghost.color,
        driver,
        roster.looks(&loadout),
    );
    commands.entity(visual).insert(GhostVisual { run });
}

/// Every frame: the ghost where its run was at this moment of the race (between its last two
/// ticks, like the karts' visuals), with its look; hidden once past its last frame.
fn show_ghost(
    state: Res<TimeTrialState>,
    race: Option<Res<Race>>,
    fixed: Res<Time<Fixed>>,
    (net, kart_tuning): (Option<Res<NetTuning>>, Res<KartTuning>),
    mut bodies: Query<
        (
            &GhostKart,
            &mut Transform,
            (&mut KartState, &mut DriftMotion, &mut Boost, &mut Stall),
            (&mut KartWheels, &mut KartEnvironment, &mut Velocity),
            &StatEffects,
        ),
        Without<GhostVisual>,
    >,
    mut visuals: Query<(&GhostVisual, &mut Transform, &mut Visibility), Without<GhostKart>>,
) {
    let (Some(ghost), Some(race)) = (state.run.ghost.as_ref(), race) else {
        return;
    };
    let limits = net.map_or_else(|| NetTuning::default().limits(), |net| net.limits());
    let tick = f64::from(race.race_ticks()) - 1.0 + f64::from(fixed.overstep_fraction());
    let pose = ghost.sample(tick, &limits);
    let shown =
        pose.map(|pose| Transform::from_translation(pose.position).with_rotation(pose.rotation));
    for (visual, mut transform, mut visibility) in &mut visuals {
        if visual.run != state.run.number {
            continue;
        }
        visibility.set_if_neq(if shown.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if let Some(shown) = shown {
            transform.set_if_neq(shown);
        }
    }
    let Some(pose) = pose else {
        return;
    };
    for (
        body,
        mut transform,
        (mut kart_state, mut motion, mut boost, mut stall),
        (mut wheels, mut environment, mut velocity),
        effects,
    ) in &mut bodies
    {
        if body.run != state.run.number {
            continue;
        }
        if let Some(shown) = shown {
            transform.set_if_neq(shown);
        }
        let look = pose.look;
        let own = effects.tuning(&kart_tuning);
        let shown_state = look.kart_state(&own);
        kart_state.set_if_neq(shown_state);
        motion.set_if_neq(DriftMotion {
            side: match shown_state {
                KartState::Drifting { side, .. } => Some(side),
                _ => None,
            },
            drifting: matches!(shown_state, KartState::Drifting { .. }),
            slip: look.slip,
            previous_slip: look.slip,
            ..*motion
        });
        boost.set_if_neq(look.boost());
        stall.set_if_neq(Stall {
            remaining: if look.stalled { 1.0 } else { 0.0 },
        });
        let shown_wheels = look.kart_wheels();
        if wheels.on_ground != shown_wheels.on_ground
            || wheels.compression != shown_wheels.compression
        {
            *wheels = shown_wheels;
        }
        environment.set_if_neq(KartEnvironment {
            gliding: look.gliding,
            underwater: look.underwater,
            // Issue #78: its wheels pivot in anti-gravity.
            anti_gravity: look.anti_gravity,
            up: if look.anti_gravity {
                pose.rotation * Vec3::Y
            } else {
                Vec3::Y
            },
            ..*environment
        });
        let moving = Velocity {
            linear: pose.velocity,
            angular: pose.spin,
        };
        if *velocity != moving {
            *velocity = moving;
        }
    }
}

/// The ghost's meshes wear translucent copies of their materials (made once per material, then
/// shared), and cast no shadow. Only what changed this frame is looked at.
fn wear_ghost_materials(
    mut commands: Commands,
    mut cache: ResMut<GhostMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<(), With<GhostVisual>>,
    parents: Query<&ChildOf>,
    mut meshes: Query<
        (Entity, &mut MeshMaterial3d<StandardMaterial>),
        Changed<MeshMaterial3d<StandardMaterial>>,
    >,
) {
    for (entity, mut material) in &mut meshes {
        let id = material.0.id();
        if cache.copies.contains(&id) {
            continue;
        }
        let under_a_ghost = std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .any(|e| roots.contains(e));
        if !under_a_ghost {
            continue;
        }
        let copy = match cache.by_source.get(&id) {
            Some(copy) => copy.clone(),
            None => {
                let Some(source) = materials.get(id) else {
                    continue;
                };
                let mut ghost = source.clone();
                ghost.base_color = ghost
                    .base_color
                    .with_alpha(ghost.base_color.alpha() * GHOST_ALPHA);
                ghost.alpha_mode = AlphaMode::Blend;
                let copy = materials.add(ghost);
                cache.copies.insert(copy.id());
                cache.by_source.insert(id, copy.clone());
                copy
            }
        };
        material.0 = copy;
        commands.entity(entity).try_insert(NotShadowCaster);
    }
}

/// What the time trial's HUD shows (hundredths of a second), compared from frame to frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TrialHudValues {
    /// Before GO.
    starting: bool,
    record: Option<u32>,
    /// The run's time, whether it is the record, and the record before it.
    finish: Option<(u32, bool, Option<u32>)>,
    laps: [Option<u32>; LAP_LINES],
    /// Issue #119: an open circuit: « SECTION 2 » instead of « TOUR 2 ».
    sections: bool,
}

/// Issue #100: the HUD's colors and font (the charter's: `ui`).
const TEXT_COLOR: Color = Color::WHITE;
const GOLD: Color = palette::MANGUE;
const PANEL_COLOR: Color = Color::srgba(0.227, 0.173, 0.431, 0.6);
const TEXT_SHADOW: Color = Color::srgba(0.141, 0.102, 0.290, 0.85);

fn text(face: &Handle<Font>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(""),
        TextFont {
            font: bevy::text::FontSource::from(face),
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
        TextShadow {
            offset: Vec2::new(2.0, 3.0),
            color: TEXT_SHADOW,
        },
    )
}

/// The HUD of the time trial, with the level: the record at the top, the laps on the right.
fn spawn_trial_hud(
    mut commands: Commands,
    course: Option<Res<Course>>,
    assets: Option<Res<MenuAssets>>,
    huds: Query<(), With<TrialHud>>,
) {
    let Some(course) = course else {
        return;
    };
    if !huds.is_empty() {
        return;
    }
    let (_, face) = kit::faces(assets.as_deref());
    commands
        .spawn((
            Name::new("Contre-la-montre"),
            TrialHud,
            LevelScoped,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|hud| {
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: px(18),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(4),
                ..default()
            })
            .with_children(|top| {
                top.spawn((TrialText::Record, text(&face, 34.0, GOLD)));
                top.spawn((TrialText::Verdict, text(&face, 22.0, TEXT_COLOR)));
            });
            hud.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: px(24),
                    top: px(92),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(2),
                    padding: UiRect::axes(px(10), px(6)),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(PANEL_COLOR),
            ))
            .with_children(|laps| {
                for lap in 0..(course.laps as usize).clamp(1, LAP_LINES) {
                    laps.spawn((TrialText::Lap(lap), text(&face, 20.0, TEXT_COLOR)));
                }
            });
        });
}

/// A gap in hundredths: « +0.70 », « -1:02.35 ».
fn write_gap(out: &mut String, gap: i64) {
    use std::fmt::Write;
    out.push(if gap < 0 { '-' } else { '+' });
    let gap = gap.unsigned_abs() as u32;
    if gap >= 6000 {
        write_time(out, Some(gap));
    } else {
        let _ = write!(out, "{}.{:02}", gap / 100, gap % 100);
    }
}

/// The texts of the time trial's HUD, rewritten only when what they show changes; the rank of
/// the race HUD is hidden (alone, always first).
fn update_trial_hud(
    state: Res<TimeTrialState>,
    race: Res<Race>,
    course: Option<Res<Course>>,
    progress: Query<&RaceProgress>,
    huds: Query<Entity, With<TrialHud>>,
    mut shown: Local<Option<(Entity, TrialHudValues)>>,
    mut texts: Query<(&TrialText, &mut Text, &mut TextColor)>,
    mut ranks: Query<(&HudText, &mut Visibility)>,
) {
    for (field, mut visibility) in &mut ranks {
        if matches!(field, HudText::Rank | HudText::RacerCount) {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
    let run = &state.run;
    let laps = run
        .racer
        .and_then(|racer| progress.get(racer).ok())
        .map(|progress| progress.lap_times.as_slice())
        .unwrap_or_default();
    let values = TrialHudValues {
        starting: !race.started(),
        record: run.record.map(hundredths),
        finish: run.finish.map(|(time, verdict)| {
            (
                hundredths(time),
                verdict.record,
                verdict.previous.map(hundredths),
            )
        }),
        laps: std::array::from_fn(|lap| laps.get(lap).copied().map(hundredths)),
        sections: course.is_some_and(|course| course.is_open()),
    };
    // A new HUD (a new level) is written whole.
    let Ok(hud) = huds.single() else {
        return;
    };
    if *shown == Some((hud, values)) {
        return;
    }
    for (field, mut text, mut color) in &mut texts {
        let out = &mut text.0;
        out.clear();
        match *field {
            TrialText::Record => match (values.finish, values.starting) {
                (Some((_, true, _)), _) => {
                    out.push_str("RECORD !");
                    color.set_if_neq(TextColor(GOLD));
                }
                (Some(_), _) | (None, true) => {
                    match values.record {
                        Some(record) => {
                            out.push_str("RECORD ");
                            write_time(out, Some(record));
                        }
                        None => out.push_str("PAS ENCORE DE RECORD"),
                    }
                    color.set_if_neq(TextColor(TEXT_COLOR));
                }
                (None, false) => {}
            },
            TrialText::Verdict => match values.finish {
                Some((time, true, Some(previous))) => {
                    out.push_str("ANCIEN ");
                    write_time(out, Some(previous));
                    out.push_str(" (");
                    write_gap(out, i64::from(time) - i64::from(previous));
                    out.push(')');
                }
                Some((_, true, None)) => out.push_str("PREMIER RECORD DU CIRCUIT"),
                Some((time, false, previous)) => {
                    out.push_str("TON TEMPS ");
                    write_time(out, Some(time));
                    if let Some(previous) = previous {
                        out.push_str(" (");
                        write_gap(out, i64::from(time) - i64::from(previous));
                        out.push(')');
                    }
                }
                None if values.starting && run.ghost.is_some() => {
                    // Issue #100: with its accent (Fredoka).
                    out.push_str("LE FANTÔME DU RECORD COURT AVEC TOI");
                }
                None => {}
            },
            TrialText::Lap(lap) => {
                if let Some(time) = values.laps.get(lap).copied().flatten() {
                    use std::fmt::Write;
                    let label = if values.sections { "SECTION" } else { "TOUR" };
                    let _ = write!(out, "{label} {}  ", lap + 1);
                    write_time(out, Some(time));
                }
            }
        }
    }
    *shown = Some((hud, values));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_signed_hundredths() {
        let mut out = String::new();
        for (gap, wanted) in [
            (70, "+0.70"),
            (-115, "-1.15"),
            (0, "+0.00"),
            (6235, "+1:02.35"),
            (-12_000, "-2:00.00"),
        ] {
            out.clear();
            write_gap(&mut out, gap);
            assert_eq!(out, wanted);
        }
    }
}
