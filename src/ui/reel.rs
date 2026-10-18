//! The reel of the HUD's item slots (issue #102), drawn: the roulette as a slot machine
//! (`roulette.rs`). Only with a renderer ([`RoulettePlugin`], added by `GamePlugin` when there is
//! a `RenderPlugin`; the tests add it by hand): the headless world, the bot races', keeps the
//! single icon of the HUD and every archetype it was measured with.
//!
//! - Each slot holds a cell per item ([`ReelCell`], sixteen, built once and hidden), in place of
//!   the single icon; at most two are seen at a time: the item in the window and the next one
//!   coming in from the top. They move with `UiTransform` only (a share of their own height):
//!   nothing is laid out again while the reel turns. The slot clips them (`Overflow::clip`).
//! - While it spins, its window is darker (the bright icons read better on the sky).
//! - At the stop, the « ding »: the slot squashes and bounces back (its `UiTransform`), a ring
//!   bursts out of it (its `Outline`), its window flashes; the stop's sound.
//! - A tick for each item that passes, a « ding » at the stop (`audio::RouletteSounds`): only for
//!   the player's own kart (not the home screen's race), one of each at most per frame (a double
//!   box turns both reels together).

use bevy::prelude::*;

use super::roulette::{self, REEL, ReelFrame, RouletteClock};
use super::{ICON_SCALES, ItemIcon, ItemSlot, PANEL_COLOR, followed, icon_shape};
use crate::audio::{Blip, RouletteSounds, SoundEffect, play_roulette};
use crate::items::{ItemKind, ItemSlots, ItemTuning, Slot};
use crate::kart::PlayerKart;
use crate::menu::style::palette;
use crate::race::{Race, RaceProgress, Racer};

/// The reel of the HUD's slots, with a renderer.
pub struct RoulettePlugin;

impl Plugin for RoulettePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sounds).add_systems(
            Update,
            (build_reels, spin_reels)
                .chain()
                .after(super::update_item_slots)
                .run_if(resource_exists::<Race>),
        );
    }
}

/// An item slot of the HUD whose reel is built.
#[derive(Component, Clone, Copy, Debug)]
pub struct Reel;

/// A cell of a slot's reel: `item`'s icon, in slot `index` (0 = the hand).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReelCell {
    pub index: usize,
    pub item: ItemKind,
}

/// The window of a spinning slot is this dark (Myrtille, a little more opaque than the panel).
const SPIN_PANEL: Color = Color::srgba(0.227, 0.173, 0.431, 0.88);
/// The flash of the stop, and the ring that bursts out (the charter's Mangue: what pops).
const FLASH_COLOR: Color = Color::srgb(1.0, 0.95, 0.75);
const BURST_COLOR: Color = palette::MANGUE;

fn create_sounds(mut commands: Commands, mut blips: ResMut<Assets<Blip>>) {
    commands.insert_resource(RouletteSounds::new(&mut blips));
}

/// The reel of each new slot: its cells, in place of the single icon.
fn build_reels(
    mut commands: Commands,
    mut slots: Query<(Entity, &ItemSlot, &mut Node, &Children), Without<Reel>>,
    icons: Query<(), With<ItemIcon>>,
) {
    for (entity, slot, mut node, children) in &mut slots {
        for child in children.iter() {
            if icons.contains(child) {
                commands.entity(child).despawn();
            }
        }
        node.overflow = Overflow::clip();
        let scale = ICON_SCALES[slot.index.min(1)];
        let cells: Vec<Entity> = REEL
            .iter()
            .map(|&item| {
                let (size, fill, edge, width, degrees) = icon_shape(item);
                commands
                    .spawn((
                        ReelCell {
                            index: slot.index,
                            item,
                        },
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(0),
                            width: percent(100),
                            height: percent(100),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Visibility::Hidden,
                        children![(
                            Node {
                                width: px(size.x),
                                height: px(size.y),
                                border: UiRect::all(px(width)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(fill),
                            BorderColor::all(edge),
                            UiTransform {
                                scale: Vec2::splat(scale),
                                rotation: Rot2::degrees(degrees),
                                ..default()
                            },
                        )],
                    ))
                    .id()
            })
            .collect();
        // Under the "x3".
        commands.entity(entity).insert_children(0, &cells).insert((
            Reel,
            Outline::new(px(roulette::BURST_WIDTH), px(0), Color::NONE),
        ));
    }
}

/// What a slot's reel remembers between two frames.
#[derive(Clone, Copy, Debug)]
struct ReelMemory {
    /// The item passed at the last frame of its roulette (`None`: no roulette).
    cell: Option<u32>,
    /// Seconds since the reel stopped (its ding), infinite once it settled.
    since_stop: f32,
}

impl Default for ReelMemory {
    fn default() -> Self {
        Self {
            cell: None,
            since_stop: f32::INFINITY,
        }
    }
}

/// What a slot's window shows: nothing, its reel turning, or an item (stopped).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Window {
    Empty,
    Turning(ReelFrame),
    Item(ItemKind),
}

impl Window {
    fn of(slot: &Slot, frame: Option<ReelFrame>) -> Self {
        match (*slot, frame) {
            (Slot::Spinning { .. }, Some(frame)) => Self::Turning(frame),
            (Slot::Spinning { item, .. } | Slot::Ready { item, .. }, _) => Self::Item(item),
            (Slot::Empty, _) => Self::Empty,
        }
    }

    /// Where `item`'s cell is (a share of its height, down), if it is seen.
    fn cell(&self, item: ItemKind) -> Option<f32> {
        match *self {
            Self::Turning(frame) if frame.shown == item => Some(frame.slide),
            Self::Turning(frame) if frame.next == item && frame.slide > 0.0 => {
                Some(frame.slide - 1.0)
            }
            Self::Item(shown) if shown == item => Some(0.0),
            _ => None,
        }
    }
}

/// Each frame: the reels turn with the rules' ticks, then stop with a ding; a tick for each item
/// that passes, a ding at the stop.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn spin_reels(
    mut commands: Commands,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<ItemTuning>,
    sounds: Option<Res<RouletteSounds>>,
    mut memory: Local<[ReelMemory; 2]>,
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    held: Query<&ItemSlots>,
    mut slots: Query<
        (
            &ItemSlot,
            &mut UiTransform,
            &mut Outline,
            &mut BackgroundColor,
        ),
        With<Reel>,
    >,
    mut cells: Query<(&ReelCell, &mut UiTransform, &mut Visibility), Without<ItemSlot>>,
) {
    let followed = followed(racers.iter()).map(|(entity, _)| entity);
    let items = followed.and_then(|entity| held.get(entity).ok());
    let player = followed.is_some_and(|entity| racers.get(entity).is_ok_and(|r| r.3));
    let clock = RouletteClock::new(&fixed, &tuning);
    let frames = items.map_or([None; 2], |items| clock.frames(items));
    let windows = [0, 1].map(|index| {
        items.map_or(Window::Empty, |items| {
            Window::of(&[items.hand, items.reserve][index], frames[index])
        })
    });
    let dt = time.delta_secs();
    let (mut tick, mut stop) = (false, false);
    for (window, memory) in windows.iter().zip(memory.iter_mut()) {
        match *window {
            Window::Turning(frame) => {
                // A tick for each item that comes into the middle (not the first one, already
                // there; the last one is the stop's ding).
                tick |= memory.cell.is_some_and(|last| frame.cell > last);
                memory.cell = Some(frame.cell);
                memory.since_stop = f32::INFINITY;
            }
            Window::Item(_) => {
                // The roulette just stopped: its ding.
                if memory.cell.take().is_some() {
                    stop = true;
                    memory.since_stop = 0.0;
                } else {
                    memory.since_stop += dt;
                }
            }
            Window::Empty => *memory = ReelMemory::default(),
        }
    }
    if player && let Some(sounds) = &sounds {
        if stop {
            play_roulette(&mut commands, sounds, SoundEffect::RouletteStop);
        } else if tick {
            play_roulette(&mut commands, sounds, SoundEffect::RouletteTick);
        }
    }
    for (slot, mut transform, mut outline, mut background) in &mut slots {
        let index = slot.index.min(1);
        let since = memory[index].since_stop;
        transform.set_if_neq(UiTransform::from_scale(roulette::ding_scale(since)));
        let ring = match roulette::burst(since) {
            Some((out, alpha)) => Outline::new(
                px(roulette::BURST_WIDTH),
                px(out),
                BURST_COLOR.with_alpha(alpha),
            ),
            None => Outline::new(px(roulette::BURST_WIDTH), px(0), Color::NONE),
        };
        outline.set_if_neq(ring);
        let panel = match windows[index] {
            Window::Turning(_) => SPIN_PANEL,
            _ => mix(PANEL_COLOR, FLASH_COLOR, roulette::flash(since)),
        };
        background.set_if_neq(BackgroundColor(panel));
    }
    for (cell, mut transform, mut visibility) in &mut cells {
        match windows[cell.index.min(1)].cell(cell.item) {
            Some(down) => {
                transform.set_if_neq(UiTransform::from_translation(Val2::percent(
                    0.0,
                    down * 100.0,
                )));
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// `a` toward `b` by `t` (0 to 1), in sRGB with its alpha.
fn mix(a: Color, b: Color, t: f32) -> Color {
    if t <= 0.0 {
        return a;
    }
    let (a, b) = (a.to_srgba(), b.to_srgba());
    let lerp = |x: f32, y: f32| x + (y - x) * t.min(1.0);
    Color::srgba(
        lerp(a.red, b.red),
        lerp(a.green, b.green),
        lerp(a.blue, b.blue),
        lerp(a.alpha, b.alpha),
    )
}
