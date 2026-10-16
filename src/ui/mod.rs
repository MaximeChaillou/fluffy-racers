//! HUD (`bevy_ui`, issue #5): rank, lap, chrono, best lap, countdown and banners, the two item
//! slots (issue #16: hand and reserve, each item a shape and a color of its own, the roulette
//! flashing through them, "x3" for a triple), the croquettes held (issue #14: the counter pops at
//! each pick-up, shakes when some are lost, turns gold at 10), the chantilly's cream over the
//! screen (issue #18), and the results screen (issue #70: live, until the race closes them).
//!
//! It follows the local player's racer (or, without one, the pole sitter) and only reads the
//! race and the croquettes: `crate::race` never depends on it. Texts are rewritten only when
//! their value changes.

pub mod debug;
pub mod format;
pub mod pop;
#[cfg(test)]
pub(crate) mod results_tests;

pub use debug::{DEBUG_OVERLAY_KEY, DebugOverlay, DebugOverlayPlugin, SimulationCost};

use bevy::prelude::*;

use crate::croquettes::{CroquetteTuning, Croquettes};
use crate::items::{ItemKind, ItemSlots, Slot};
use crate::kart::{ItemEffects, PlayerKart};
use crate::race::{Course, Race, RacePhase, RaceProgress, RaceTuning, Racer, RacerName};
use crate::track::GRID_SLOTS;
use format::{
    HudValues, RESULTS_FOOTER, RESULTS_FOOTER_GRAND_PRIX, RESULTS_FOOTER_ONLINE, RESULTS_TITLE,
    ResultsTime, write_best_lap, write_croquettes, write_lap, write_ordinal, write_racer_count,
    write_racer_name, write_time,
};

/// Root of the in-race HUD.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudRoot;

/// A HUD text, by what it shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudText {
    Rank,
    RacerCount,
    Lap,
    Chrono,
    BestLap,
    Banner,
    Croquettes,
}

/// The croquette counter (icon and number), which pops and shakes.
#[derive(Component, Clone, Copy, Debug)]
pub struct CroquetteCounter;

/// Seconds since the followed racer last picked a croquette up, and since it last lost some
/// (they drive the counter's pop and shake).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CroquetteCounterAnimation {
    pub since_gain: f32,
    pub since_loss: f32,
}

impl Default for CroquetteCounterAnimation {
    fn default() -> Self {
        Self {
            since_gain: f32::INFINITY,
            since_loss: f32::INFINITY,
        }
    }
}

/// An item slot (GD § 8: one in hand, one in reserve).
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemSlot {
    /// 0 = in hand (big), 1 = reserve.
    pub index: usize,
}

/// The item shown in a slot (issue #16): a shape and a color per item.
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemIcon {
    pub index: usize,
}

/// "x3", "x2" under the item of a slot (an "x3" of the table).
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemCount {
    pub index: usize,
}

/// The chantilly's cream over the screen (issue #18): shown while the followed racer is creamed.
#[derive(Component, Clone, Copy, Debug)]
pub struct CreamOverlay;

/// A blob of cream: its full opacity.
#[derive(Component, Clone, Copy, Debug)]
pub struct CreamBlob {
    pub alpha: f32,
}

/// The cream's blobs: left and top (% of the screen), size (% of the height), opacity.
const CREAM_BLOBS: [(f32, f32, f32, f32); 7] = [
    (8.0, 30.0, 34.0, 0.95),
    (30.0, 10.0, 26.0, 0.9),
    (55.0, 38.0, 38.0, 0.95),
    (72.0, 8.0, 24.0, 0.85),
    (20.0, 58.0, 28.0, 0.9),
    (78.0, 55.0, 30.0, 0.92),
    (42.0, 64.0, 20.0, 0.8),
];
const CREAM_COLOR: Color = Color::srgb(1.0, 0.97, 0.88);

/// What a HUD slot shows: the item (`None`: empty), how many, and whether the roulette spins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotView {
    pub item: Option<ItemKind>,
    pub count: u8,
    pub spinning: bool,
}

impl SlotView {
    /// What `slot` shows at `now` (s): a spinning roulette flashes through the items.
    pub fn of(slot: &Slot, now: f32) -> Self {
        match *slot {
            Slot::Empty => Self::default(),
            Slot::Spinning { .. } => Self {
                item: Some(ItemKind::ALL[(now / ROULETTE_FLASH) as usize % ItemKind::ALL.len()]),
                count: 1,
                spinning: true,
            },
            Slot::Ready { item, count } => Self {
                item: Some(item),
                count,
                spinning: false,
            },
        }
    }
}

/// The roulette shows another item this often (s).
const ROULETTE_FLASH: f32 = 0.08;

/// The shape of an item's icon in a slot: size (px), fill, border color and width, rotation
/// (degrees). Each item has its own shape and color (GD § 14: recognized at once).
pub fn icon_shape(item: ItemKind) -> (Vec2, Color, Color, f32, f32) {
    match item {
        // A yellow peel, slanted.
        ItemKind::Pelure => (
            Vec2::new(46.0, 18.0),
            Color::srgb(1.0, 0.86, 0.15),
            Color::srgb(0.6, 0.45, 0.05),
            3.0,
            -35.0,
        ),
        // A pink ball of yarn.
        ItemKind::Pelote => (
            Vec2::splat(42.0),
            Color::srgb(0.95, 0.35, 0.6),
            Color::srgb(0.65, 0.12, 0.38),
            6.0,
            0.0,
        ),
        // A yellow ball ringed in black.
        ItemKind::Abeille => (
            Vec2::splat(38.0),
            Color::srgb(1.0, 0.8, 0.1),
            Color::srgb(0.1, 0.08, 0.05),
            7.0,
            0.0,
        ),
        // A red chili, a green tip.
        ItemKind::Piment => (
            Vec2::new(18.0, 48.0),
            Color::srgb(0.9, 0.12, 0.08),
            Color::srgb(0.2, 0.65, 0.2),
            3.0,
            30.0,
        ),
        // A golden ring.
        ItemKind::CroquetteDoree => (
            Vec2::splat(40.0),
            Color::srgba(0.0, 0.0, 0.0, 0.0),
            Color::srgb(1.0, 0.78, 0.15),
            10.0,
            0.0,
        ),
        // Issue #18. A golden chili, a red edge.
        ItemKind::PimentDore => (
            Vec2::new(20.0, 50.0),
            Color::srgb(1.0, 0.8, 0.15),
            Color::srgb(0.85, 0.2, 0.05),
            4.0,
            30.0,
        ),
        // A navy ball, a yellow ring of confetti.
        ItemKind::Bombe => (
            Vec2::splat(40.0),
            Color::srgb(0.16, 0.2, 0.5),
            Color::srgb(1.0, 0.85, 0.3),
            5.0,
            0.0,
        ),
        // A dollop of cream, edged in pink.
        ItemKind::Chantilly => (
            Vec2::new(46.0, 32.0),
            Color::srgb(1.0, 0.97, 0.88),
            Color::srgb(1.0, 0.55, 0.7),
            4.0,
            -10.0,
        ),
        // A hot pink ball ringed in white.
        ItemKind::SuperCalin => (
            Vec2::splat(42.0),
            Color::srgb(1.0, 0.4, 0.7),
            Color::srgb(1.0, 1.0, 1.0),
            6.0,
            0.0,
        ),
        // An orange rocket, slanted, a red edge.
        ItemKind::Fusee => (
            Vec2::new(22.0, 52.0),
            Color::srgb(1.0, 0.5, 0.1),
            Color::srgb(0.8, 0.1, 0.1),
            4.0,
            -45.0,
        ),
        // A pale blue balloon going flat.
        ItemKind::Degonflage => (
            Vec2::new(44.0, 24.0),
            Color::srgb(0.6, 0.85, 1.0),
            Color::srgb(0.15, 0.3, 0.6),
            4.0,
            15.0,
        ),
        // A big white cushion edged in blue.
        ItemKind::OreillerGeant => (
            Vec2::new(56.0, 32.0),
            Color::srgb(0.98, 0.98, 1.0),
            Color::srgb(0.35, 0.55, 1.0),
            7.0,
            0.0,
        ),
        // A yellow duck's horn ringed in orange.
        ItemKind::KlaxonCanard => (
            Vec2::splat(40.0),
            Color::srgb(1.0, 0.92, 0.25),
            Color::srgb(1.0, 0.5, 0.0),
            9.0,
            0.0,
        ),
    }
}

/// The results screen.
#[derive(Component, Clone, Copy, Debug)]
pub struct ResultsScreen;

/// A cell of the results screen (issue #70): its line (0 = the first) and its column. Spawned
/// once with the screen; only its text changes, when what it shows changes.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsCell {
    pub row: usize,
    pub column: ResultsColumn,
}

/// The columns of the results screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultsColumn {
    /// "1er": written once.
    Rank,
    /// "Toi", "Kart 3" or another player's name.
    Name,
    /// "1:02.35", "en course", "~1:45.20" (estimated), "abandon".
    Time,
    /// "Meilleur tour 0:20.12", "Tour 2/3", "Temps estime".
    Detail,
}

/// The columns and their width (px).
const RESULTS_COLUMNS: [(ResultsColumn, f32); 4] = [
    (ResultsColumn::Rank, 60.0),
    (ResultsColumn::Name, 110.0),
    (ResultsColumn::Time, 140.0),
    (ResultsColumn::Detail, 320.0),
];

/// What a line of the results screen shows: its racer and that racer's race.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsRowView {
    pub racer: Entity,
    pub player: bool,
    pub time: ResultsTime,
}

impl ResultsColumn {
    /// Whether this column's text changes from `old` to `new`.
    fn changed(self, old: Option<ResultsRowView>, new: Option<ResultsRowView>) -> bool {
        match (old, new) {
            _ if self == Self::Rank => false,
            (Some(old), Some(new)) if self == Self::Name => {
                (old.racer, old.player) != (new.racer, new.player)
            }
            (Some(old), Some(new)) => old.time != new.time,
            (old, new) => old.is_some() || new.is_some(),
        }
    }
}

/// What the results screen shows now (issue #70): its line count, and each line's view.
#[derive(Resource, Default)]
struct ResultsShown {
    rows: usize,
    views: [Option<ResultsRowView>; GRID_SLOTS],
}

/// What the HUD shows now, to rewrite only what changed.
#[derive(Resource, Default)]
struct HudShown(Option<HudValues>);

const TEXT_COLOR: Color = Color::WHITE;
const RANK_COLOR: Color = Color::srgb(1.0, 0.85, 0.25);
const PANEL_COLOR: Color = Color::srgba(0.08, 0.06, 0.16, 0.6);
const SLOT_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.85);
const CROQUETTE_COLOR: Color = Color::srgb(0.75, 0.45, 0.2);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudShown>()
            .init_resource::<CroquetteCounterAnimation>()
            .init_resource::<ResultsShown>()
            .add_systems(
                Update,
                (
                    spawn_hud.run_if(not(any_with_component::<HudRoot>)),
                    update_hud,
                    update_item_slots,
                    show_cream,
                    animate_croquette_counter,
                    show_results,
                    update_results,
                )
                    .chain()
                    .run_if(resource_exists::<Race>),
            );
    }
}

fn text(content: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont::from_font_size(size),
        TextColor(color),
        TextShadow {
            offset: Vec2::splat(2.0),
            ..default()
        },
    )
}

fn spawn_hud(mut commands: Commands, mut shown: ResMut<HudShown>) {
    shown.0 = None;
    commands
        .spawn((
            Name::new("HUD"),
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|hud| {
            // Under everything else: the chantilly's cream (issue #18).
            hud.spawn((
                CreamOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|cream| {
                for (left, top, size, alpha) in CREAM_BLOBS {
                    cream.spawn((
                        CreamBlob { alpha },
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            top: percent(top),
                            width: vh(size * 1.4),
                            height: vh(size),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(CREAM_COLOR.with_alpha(alpha)),
                    ));
                }
            });
            // Top left: the two item slots (issue #16): hand, then reserve.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(24),
                top: px(24),
                column_gap: px(10),
                align_items: AlignItems::FlexStart,
                ..default()
            })
            .with_children(|slots| {
                for (index, size) in [(0, 76), (1, 50)] {
                    slots
                        .spawn((
                            ItemSlot { index },
                            Node {
                                width: px(size),
                                height: px(size),
                                border: UiRect::all(px(4)),
                                border_radius: BorderRadius::all(px(14)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(PANEL_COLOR),
                            BorderColor::all(SLOT_BORDER),
                        ))
                        .with_children(|slot| {
                            // The reserve's icon is smaller.
                            let scale = if index == 0 { 1.0 } else { 0.62 };
                            slot.spawn((
                                ItemIcon { index },
                                Node {
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                UiTransform {
                                    scale: Vec2::splat(scale),
                                    ..default()
                                },
                                Visibility::Hidden,
                            ));
                            slot.spawn((
                                ItemCount { index },
                                text("", if index == 0 { 20.0 } else { 14.0 }, TEXT_COLOR),
                                Node {
                                    position_type: PositionType::Absolute,
                                    right: px(2),
                                    bottom: px(0),
                                    ..default()
                                },
                            ));
                        });
                }
            });

            // Top right: chrono and best lap.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(24),
                top: px(20),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..default()
            })
            .with_children(|times| {
                times.spawn((HudText::Chrono, text("0:00.00", 36.0, TEXT_COLOR)));
                times.spawn((HudText::BestLap, text("", 18.0, TEXT_COLOR)));
            });

            // Bottom left: croquettes (issue #14) and lap.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(24),
                bottom: px(20),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(6),
                ..default()
            })
            .with_children(|left| {
                left.spawn((
                    CroquetteCounter,
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        ..default()
                    },
                ))
                .with_children(|croquettes| {
                    croquettes.spawn((
                        Node {
                            width: px(24),
                            height: px(24),
                            border: UiRect::all(px(3)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(CROQUETTE_COLOR),
                        BorderColor::all(SLOT_BORDER),
                    ));
                    croquettes.spawn((HudText::Croquettes, text("x 0", 26.0, TEXT_COLOR)));
                });
                left.spawn((HudText::Lap, text("", 36.0, TEXT_COLOR)));
            });

            // Bottom right: rank.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(28),
                bottom: px(14),
                align_items: AlignItems::FlexEnd,
                column_gap: px(6),
                ..default()
            })
            .with_children(|rank| {
                rank.spawn((HudText::Rank, text("", 72.0, RANK_COLOR)));
                rank.spawn((
                    HudText::RacerCount,
                    text("", 30.0, TEXT_COLOR),
                    Node {
                        margin: UiRect::bottom(px(12)),
                        ..default()
                    },
                ));
            });

            // Middle: countdown and banners.
            hud.spawn(Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(26),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|middle| {
                middle.spawn((
                    HudText::Banner,
                    text("", 110.0, TEXT_COLOR),
                    TextLayout::justify(Justify::Center),
                ));
            });
        });
}

/// The racer the HUD follows: the local player, or the pole sitter without one.
fn followed<'a>(
    racers: impl Iterator<Item = (Entity, &'a Racer, &'a RaceProgress, bool)>,
) -> Option<(Entity, &'a RaceProgress)> {
    let mut best: Option<(bool, usize, Entity, &RaceProgress)> = None;
    for (entity, racer, progress, player) in racers {
        let better = match best {
            None => true,
            Some((best_player, best_slot, ..)) => {
                (player, std::cmp::Reverse(racer.grid_slot))
                    > (best_player, std::cmp::Reverse(best_slot))
            }
        };
        if better {
            best = Some((player, racer.grid_slot, entity, progress));
        }
    }
    best.map(|(_, _, entity, progress)| (entity, progress))
}

fn update_hud(
    race: Res<Race>,
    course: Res<Course>,
    tuning: Res<RaceTuning>,
    croquette_tuning: Option<Res<CroquetteTuning>>,
    mut shown: ResMut<HudShown>,
    mut animation: ResMut<CroquetteCounterAnimation>,
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    held: Query<&Croquettes>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
) {
    let Some((entity, progress)) = followed(racers.iter()) else {
        return;
    };
    let croquettes = held.get(entity).map_or(0, |held| held.count as u32);
    let values = HudValues::new(
        &race,
        &course,
        &tuning,
        progress,
        racers.iter().count(),
        croquettes,
    );
    let previous = shown.0;
    if previous == Some(values) {
        return;
    }
    // The counter pops at each pick-up and shakes when croquettes are lost (not when a restart
    // empties the pockets).
    if let Some(old) = previous {
        if values.croquettes > old.croquettes {
            animation.since_gain = 0.0;
        } else if values.croquettes < old.croquettes && race.started() {
            animation.since_loss = 0.0;
        }
    }
    let changed = |field: HudText| {
        let Some(old) = previous else {
            return true;
        };
        match field {
            HudText::Rank => old.rank != values.rank,
            HudText::RacerCount => old.racers != values.racers,
            HudText::Lap => old.lap != values.lap || old.laps != values.laps,
            HudText::Chrono => old.time != values.time,
            HudText::BestLap => old.best_lap != values.best_lap,
            HudText::Banner => old.banner != values.banner,
            HudText::Croquettes => old.croquettes != values.croquettes,
        }
    };
    let full = croquette_tuning
        .as_ref()
        .is_some_and(|t| values.croquettes >= t.max_held as u32);
    for (field, mut text, mut color) in &mut texts {
        if !changed(*field) {
            continue;
        }
        let out = &mut text.0;
        match field {
            HudText::Rank => write_ordinal(out, values.rank),
            HudText::RacerCount => write_racer_count(out, values.racers),
            HudText::Lap => write_lap(out, values.lap, values.laps),
            HudText::Chrono => {
                out.clear();
                write_time(out, Some(values.time));
            }
            HudText::BestLap => write_best_lap(out, values.best_lap),
            HudText::Banner => values.banner.write(out),
            HudText::Croquettes => {
                write_croquettes(out, values.croquettes);
                // Gold when full: more croquettes add no speed.
                color.set_if_neq(TextColor(if full { RANK_COLOR } else { TEXT_COLOR }));
            }
        }
    }
    shown.0 = Some(values);
}

/// The two item slots of the followed racer (issue #16): each item's icon, "x3" for a triple,
/// the roulette flashing through the items. Rewritten only when what a slot shows changes.
fn update_item_slots(
    time: Res<Time>,
    mut shown: Local<[SlotView; 2]>,
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    held: Query<&ItemSlots>,
    mut slots: Query<(&ItemSlot, &mut BorderColor)>,
    mut icons: Query<
        (
            &ItemIcon,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut UiTransform,
            &mut Visibility,
        ),
        Without<ItemSlot>,
    >,
    mut counts: Query<(&ItemCount, &mut Text)>,
) {
    let now = time.elapsed_secs();
    let items = followed(racers.iter()).and_then(|(entity, _)| held.get(entity).ok());
    let views = match items {
        Some(items) => [
            SlotView::of(&items.hand, now),
            SlotView::of(&items.reserve, now),
        ],
        None => [SlotView::default(); 2],
    };
    if *shown == views {
        return;
    }
    for (slot, mut border) in &mut slots {
        let view = views[slot.index.min(1)];
        // The slot's border flashes gold while its roulette spins.
        let color = if view.spinning && ((now / ROULETTE_FLASH) as u32).is_multiple_of(2) {
            RANK_COLOR
        } else {
            SLOT_BORDER
        };
        border.set_if_neq(BorderColor::all(color));
    }
    for (icon, mut node, mut fill, mut border, mut transform, mut visibility) in &mut icons {
        let view = views[icon.index.min(1)];
        if view == shown[icon.index.min(1)] {
            continue;
        }
        let Some(item) = view.item else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let (size, color, edge, width, degrees) = icon_shape(item);
        node.width = px(size.x);
        node.height = px(size.y);
        node.border = UiRect::all(px(width));
        fill.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(edge));
        transform.rotation = Rot2::degrees(degrees);
        visibility.set_if_neq(Visibility::Inherited);
    }
    for (count, mut text) in &mut counts {
        let view = views[count.index.min(1)];
        let wanted = match view.count {
            n if n > 1 && !view.spinning => format!("x{n}"),
            _ => String::new(),
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    *shown = views;
}

/// The chantilly's cream over the screen (issue #18): while the followed racer is creamed, it
/// fades out in its last second. Rewritten only when its thickness changes.
fn show_cream(
    mut shown: Local<f32>,
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    effects: Query<&ItemEffects>,
    mut overlays: Query<&mut Visibility, With<CreamOverlay>>,
    mut blobs: Query<(&CreamBlob, &mut BackgroundColor)>,
) {
    let cream = followed(racers.iter())
        .and_then(|(entity, _)| effects.get(entity).ok())
        .map_or(0.0, |e| e.cream());
    if cream == *shown {
        return;
    }
    *shown = cream;
    for mut visibility in &mut overlays {
        visibility.set_if_neq(if cream > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (blob, mut color) in &mut blobs {
        color.set_if_neq(BackgroundColor(CREAM_COLOR.with_alpha(blob.alpha * cream)));
    }
}

/// The croquette counter's pop (scale) and shake (offset), every frame.
fn animate_croquette_counter(
    time: Res<Time>,
    mut animation: ResMut<CroquetteCounterAnimation>,
    mut counters: Query<&mut UiTransform, With<CroquetteCounter>>,
) {
    let dt = time.delta_secs();
    if animation.since_gain.is_finite() || animation.since_loss.is_finite() {
        animation.since_gain += dt;
        animation.since_loss += dt;
        if animation.since_gain > pop::POP_TIME && animation.since_loss > pop::SHAKE_TIME {
            *animation = CroquetteCounterAnimation::default();
        }
    }
    let wanted = UiTransform {
        scale: Vec2::splat(pop::pop_scale(animation.since_gain)),
        translation: Val2::px(pop::shake_offset(animation.since_loss), 0.0),
        ..default()
    };
    for mut transform in &mut counters {
        transform.set_if_neq(wanted);
    }
}

/// Spawns the results screen when the results phase starts, removes it on restart. Online
/// (issue #9): the host's order (`race::OnlineFlow`), the other players' names, and the host
/// starts the next race. Its lines are filled by [`update_results`] (issue #70).
fn show_results(
    mut commands: Commands,
    race: Res<Race>,
    online: Option<Res<crate::level::Online>>,
    grand_prix: Option<Res<crate::race::GrandPrix>>,
    mut shown: ResMut<ResultsShown>,
    screens: Query<Entity, With<ResultsScreen>>,
) {
    // Issue #19: in a Grand Prix, its standings and its podium take the results' place.
    let showing = race.phase() == RacePhase::Results
        && grand_prix
            .as_ref()
            .is_none_or(|gp| gp.stage == crate::race::GrandPrixStage::Racing);
    if !showing {
        for screen in &screens {
            commands.entity(screen).despawn();
        }
        return;
    }
    if !screens.is_empty() {
        return;
    }
    let rows = if race.results_closed() {
        race.results.len()
    } else {
        race.standings.len()
    }
    .min(GRID_SLOTS);
    *shown = ResultsShown { rows, ..default() };
    let mut rank = String::new();
    commands
        .spawn((
            Name::new("Results"),
            ResultsScreen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(24)),
                        row_gap: px(8),
                        border_radius: BorderRadius::all(px(18)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.06, 0.16, 0.85)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        text(RESULTS_TITLE, 40.0, RANK_COLOR),
                        Node {
                            margin: UiRect::bottom(px(8)),
                            ..default()
                        },
                    ));
                    for row in 0..rows {
                        write_ordinal(&mut rank, row as u32 + 1);
                        panel
                            .spawn(Node {
                                column_gap: px(18),
                                ..default()
                            })
                            .with_children(|line| {
                                for (column, width) in RESULTS_COLUMNS {
                                    let content = match column {
                                        ResultsColumn::Rank => rank.as_str(),
                                        _ => "",
                                    };
                                    line.spawn((
                                        ResultsCell { row, column },
                                        text(content, 24.0, TEXT_COLOR),
                                        Node {
                                            width: px(width),
                                            ..default()
                                        },
                                    ));
                                }
                            });
                    }
                    panel.spawn((
                        text(
                            if online.is_some() {
                                RESULTS_FOOTER_ONLINE
                            } else if grand_prix.is_some() {
                                RESULTS_FOOTER_GRAND_PRIX
                            } else {
                                RESULTS_FOOTER
                            },
                            20.0,
                            TEXT_COLOR,
                        ),
                        Node {
                            margin: UiRect::top(px(12)),
                            ..default()
                        },
                    ));
                });
        });
}

/// The line `row` of the results screen: the racer at that place and its race, live (the
/// standings: online, the host's order), or from the closed results (issue #70).
fn results_row(
    race: &Race,
    course: &Course,
    row: usize,
    racers: &Query<(&Racer, &RaceProgress, Has<PlayerKart>, Option<&RacerName>)>,
) -> Option<ResultsRowView> {
    let (racer, closed) = if race.results_closed() {
        let line = race.results.get(row)?;
        (line.id, Some(line.time))
    } else {
        (*race.standings.get(row)?, None)
    };
    let (_, progress, player, _) = racers.get(racer).ok()?;
    Some(ResultsRowView {
        racer,
        player,
        time: ResultsTime::of(progress, course, closed),
    })
}

/// Issue #70: the lines of the results screen follow the race: a racer that finishes gets its
/// time and best lap, the order follows the standings, a racer still on track says so (with its
/// lap), and once the results closed, the final table (estimated times). Every frame it only
/// compares what each line shows; a text is rewritten when its value changes (no allocation, no
/// new entity).
fn update_results(
    race: Res<Race>,
    course: Res<Course>,
    mut shown: ResMut<ResultsShown>,
    racers: Query<(&Racer, &RaceProgress, Has<PlayerKart>, Option<&RacerName>)>,
    mut cells: Query<(&ResultsCell, &mut Text, &mut TextColor)>,
) {
    if race.phase() != RacePhase::Results {
        return;
    }
    let mut views = [None; GRID_SLOTS];
    for (row, view) in views.iter_mut().enumerate().take(shown.rows) {
        *view = results_row(&race, &course, row, &racers);
    }
    if views == shown.views {
        return;
    }
    for (cell, mut text, mut color) in &mut cells {
        let Some(old) = shown.views.get(cell.row).copied() else {
            continue;
        };
        let new = views[cell.row];
        if old == new {
            continue;
        }
        // The local player's line in gold, wherever it goes.
        let player = new.is_some_and(|view| view.player);
        color.set_if_neq(TextColor(if player { RANK_COLOR } else { TEXT_COLOR }));
        if !cell.column.changed(old, new) {
            continue;
        }
        let out = &mut text.0;
        let Some(view) = new else {
            out.clear();
            continue;
        };
        match cell.column {
            ResultsColumn::Rank => {}
            ResultsColumn::Name => {
                let name = racers.get(view.racer).ok();
                write_racer_name(
                    out,
                    view.player,
                    name.map_or(0, |(racer, ..)| racer.grid_slot),
                    name.and_then(|(.., name)| name).map(|n| n.0.as_str()),
                );
            }
            ResultsColumn::Time => view.time.write_time(out),
            ResultsColumn::Detail => view.time.write_detail(out),
        }
    }
    shown.views = views;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GD § 14 (issues #16 and #18): every item has an icon of its own in the HUD: no two items
    /// share their colors (fill and edge).
    #[test]
    fn every_item_has_its_own_icon() {
        let icons: Vec<_> = ItemKind::ALL.iter().map(|item| icon_shape(*item)).collect();
        for (i, a) in icons.iter().enumerate() {
            for (j, b) in icons.iter().enumerate().skip(i + 1) {
                let same_colors = a.1 == b.1 && a.2 == b.2;
                assert!(
                    !same_colors,
                    "{:?} and {:?} share their colors",
                    ItemKind::ALL[i],
                    ItemKind::ALL[j]
                );
            }
        }
    }
}
