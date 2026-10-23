//! The reel of the HUD's item slots (issue #102), drawn: the roulette as a slot machine
//! (`roulette.rs`). Only with a renderer ([`RoulettePlugin`], added by `GamePlugin` when there is
//! a `RenderPlugin`; the tests add it by hand): the headless world, the bot races', keeps the
//! single icon of the HUD and every archetype it was measured with.
//!
//! - Each slot holds a cell per item ([`ReelCell`], sixteen, built once and hidden), in place of
//!   the single icon; at most two are seen at a time: the item in the window and the next one
//!   coming in from the top. Issue #106: each cell is the item's icon, its model rendered once
//!   (`items::icons`), [`ICON_SIDE`] px (bigger in the hand, smaller in the reserve). They move
//!   with `UiTransform` only (a share of their own height): nothing is laid out again while the
//!   reel turns. The slot clips them (`Overflow::clip`).
//! - While it spins, its window is darker (the bright icons read better on the sky).
//! - At the stop, the « ding »: the slot squashes and bounces back (its `UiTransform`), a ring
//!   bursts out of it (its `Outline`), its window flashes; the stop's sound.
//! - Issue #106: the croquette counter's dot is the croquette's icon too.
//! - A tick for each item that passes, a « ding » at the stop (`audio::RouletteSounds`): only for
//!   the player's own kart (not the home screen's race), one of each at most per frame (a double
//!   box turns both reels together).
//! - Issue #171: **an item lost** (hit, the pelican, deflated, stolen, a shield that broke...:
//!   `items::ItemStats::last_loss`): the hand's slot shakes and flashes in Fraise, a short text
//!   pops out beside the slots (« Volé ! », « Dégonflé ! »: `items::ItemLoss::label`, never a
//!   key) and fades away, with a sound for the player's own kart.

use bevy::prelude::*;

use super::roulette::{self, REEL, ReelFrame, RouletteClock};
use super::{
    CroquetteCounter, ICON_SCALES, ItemIcon, ItemSlot, PANEL_COLOR, SLOT_GAP, SLOT_SIZES,
    TEXT_SHADOW, followed,
};
use crate::audio::{RouletteSounds, SoundEffect, play_roulette};
use crate::items::{
    ItemIconPlugin, ItemIcons, ItemKind, ItemLoss, ItemSlots, ItemStats, ItemTuning, Slot,
};
use crate::kart::PlayerKart;
use crate::menu::style::palette;
use crate::menu::{MenuAssets, kit};
use crate::race::{Race, RaceProgress, Racer};

/// The reel of the HUD's slots, with a renderer.
pub struct RoulettePlugin;

impl Plugin for RoulettePlugin {
    fn build(&self, app: &mut App) {
        // Issue #106: the items' icons. Their images need a renderer's storage of images (the
        // headless tests have none: made here).
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        if !app.is_plugin_added::<ItemIconPlugin>() {
            app.add_plugins(ItemIconPlugin);
        }
        app.add_systems(Startup, create_sounds).add_systems(
            Update,
            (
                (build_reels, spin_reels)
                    .chain()
                    .after(super::update_item_slots)
                    .run_if(resource_exists::<Race>),
                dress_croquette_counter,
            ),
        );
    }
}

/// An item slot of the HUD whose reel is built.
#[derive(Component, Clone, Copy, Debug)]
pub struct Reel;

/// Issue #171: the short text of an item lost, under the slots.
#[derive(Component, Clone, Copy, Debug)]
pub struct LossLabel;

/// Issue #171: its size (px of the reference window) and color (the charter's Fraise: what
/// hurts), and how far right of the reserve's slot it is (px), level with the slots.
const LOSS_LABEL_SIZE: f32 = 30.0;
const LOSS_COLOR: Color = palette::FRAISE;
const LOSS_LABEL_GAP: f32 = 14.0;

/// A cell of a slot's reel: `item`'s icon, in slot `index` (0 = the hand).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReelCell {
    pub index: usize,
    pub item: ItemKind,
}

/// An item's icon in a cell (px), before the slot's scale (`ICON_SCALES`): 79 px in the hand,
/// 49 in the reserve.
pub const ICON_SIDE: f32 = 66.0;
/// The croquette counter's icon (px).
const COUNTER_ICON_SIDE: f32 = 34.0;

/// The window of a spinning slot is this dark (Myrtille, a little more opaque than the panel).
const SPIN_PANEL: Color = Color::srgba(0.227, 0.173, 0.431, 0.88);
/// The flash of the stop, and the ring that bursts out (the charter's Mangue: what pops).
const FLASH_COLOR: Color = Color::srgb(1.0, 0.95, 0.75);
const BURST_COLOR: Color = palette::MANGUE;

fn create_sounds(mut commands: Commands) {
    commands.insert_resource(RouletteSounds);
}

/// The reel of each new slot: its cells, in place of the single icon.
fn build_reels(
    mut commands: Commands,
    item_icons: Res<ItemIcons>,
    menu_assets: Option<Res<MenuAssets>>,
    mut slots: Query<(Entity, &ItemSlot, &mut Node, &Children, &ChildOf), Without<Reel>>,
    icons: Query<(), With<ItemIcon>>,
) {
    for (entity, slot, mut node, children, child_of) in &mut slots {
        if slot.index == 0 {
            // Issue #171: the text of an item lost, beside the slots (hidden until then).
            let (_, face) = kit::faces(menu_assets.as_deref());
            commands.spawn((
                LossLabel,
                kit::text("", &face, LOSS_LABEL_SIZE, LOSS_COLOR),
                TextShadow {
                    offset: Vec2::new(2.0, 3.0),
                    color: TEXT_SHADOW,
                },
                TextLayout {
                    linebreak: bevy::text::LineBreak::NoWrap,
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: px(SLOT_SIZES[0] + SLOT_GAP + SLOT_SIZES[1] + LOSS_LABEL_GAP),
                    top: px((SLOT_SIZES[1] - LOSS_LABEL_SIZE) / 2.0),
                    ..default()
                },
                UiTransform::default(),
                Visibility::Hidden,
                ChildOf(child_of.parent()),
            ));
        }
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
                            ImageNode::new(item_icons.get(item).clone()),
                            Node {
                                width: px(ICON_SIDE),
                                height: px(ICON_SIDE),
                                ..default()
                            },
                            UiTransform {
                                scale: Vec2::splat(scale),
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

/// Issue #106: the croquette counter shows the croquette's icon (its model rendered) in place of
/// its dot (once).
fn dress_croquette_counter(
    mut commands: Commands,
    icons: Res<ItemIcons>,
    counters: Query<&Children, With<CroquetteCounter>>,
    mut dots: Query<&mut Node, (With<BackgroundColor>, Without<ImageNode>, Without<Text>)>,
) {
    for children in &counters {
        let Some(dot) = children.iter().find(|child| dots.contains(*child)) else {
            continue;
        };
        if let Ok(mut node) = dots.get_mut(dot) {
            node.width = px(COUNTER_ICON_SIDE);
            node.height = px(COUNTER_ICON_SIDE);
            node.border = UiRect::ZERO;
        }
        commands.entity(dot).insert((
            ImageNode::new(icons.croquette.clone()),
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
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

/// Issue #171: the last item loss of the followed kart, and how long ago (s).
#[derive(Clone, Copy, Debug)]
struct LossMemory {
    kart: Option<Entity>,
    losses: u32,
    since: f32,
}

impl Default for LossMemory {
    fn default() -> Self {
        Self {
            kart: None,
            losses: 0,
            since: f32::INFINITY,
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
    (mut memory, mut lost): (Local<[ReelMemory; 2]>, Local<LossMemory>),
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    (held, stats): (Query<&ItemSlots>, Query<&ItemStats>),
    mut slots: Query<
        (
            &ItemSlot,
            &mut UiTransform,
            &mut Outline,
            &mut BackgroundColor,
        ),
        With<Reel>,
    >,
    mut cells: Query<
        (&ReelCell, &mut UiTransform, &mut Visibility),
        (Without<ItemSlot>, Without<LossLabel>),
    >,
    mut labels: Query<
        (&mut Text, &mut TextColor, &mut UiTransform, &mut Visibility),
        (With<LossLabel>, Without<ItemSlot>, Without<ReelCell>),
    >,
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
    // Issue #171: an item lost by the followed kart (none counted when it starts being followed).
    let stat = followed.and_then(|entity| stats.get(entity).ok());
    let losses = stat.map_or(0, |s| s.losses());
    let cause = stat.and_then(|s| s.last_loss).map(|(cause, _)| cause);
    let new_loss = lost.kart == followed && losses > lost.losses;
    if lost.kart != followed {
        *lost = LossMemory {
            kart: followed,
            ..default()
        };
    }
    lost.losses = losses;
    let label = cause.and_then(ItemLoss::label);
    if new_loss && label.is_some() {
        lost.since = 0.0;
        for (mut text, ..) in &mut labels {
            if let Some(label) = label
                && text.0 != label
            {
                text.0 = label.to_string();
            }
        }
    } else if lost.since.is_finite() {
        lost.since += dt;
        if lost.since > roulette::LOSS_TIME {
            lost.since = f32::INFINITY;
        }
    }
    let shown_loss = new_loss && label.is_some();
    if player && let Some(sounds) = &sounds {
        if shown_loss {
            play_roulette(&mut commands, sounds, SoundEffect::CroquettesLost);
        } else if stop {
            play_roulette(&mut commands, sounds, SoundEffect::RouletteStop);
        } else if tick {
            play_roulette(&mut commands, sounds, SoundEffect::RouletteTick);
        }
    }
    for (_, mut color, mut transform, mut visibility) in &mut labels {
        match roulette::loss_label(lost.since) {
            Some((scale, alpha)) => {
                transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale)));
                color.set_if_neq(TextColor(LOSS_COLOR.with_alpha(alpha)));
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (slot, mut transform, mut outline, mut background) in &mut slots {
        let index = slot.index.min(1);
        let since = memory[index].since_stop;
        let mut wanted = UiTransform::from_scale(roulette::ding_scale(since));
        let (mut shake, mut hurt) = (0.0, 0.0);
        if index == 0 {
            shake = roulette::loss_shake(lost.since);
            hurt = roulette::loss_flash(lost.since);
        }
        if shake != 0.0 {
            wanted.translation = Val2::px(shake, 0.0);
        }
        transform.set_if_neq(wanted);
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
        let panel = mix(panel, LOSS_COLOR.with_alpha(0.85), hurt);
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
