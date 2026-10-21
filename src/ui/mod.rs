//! HUD (`bevy_ui`, issue #5): rank, lap, chrono, best lap, countdown and banners, the two item
//! slots (issue #16: hand and reserve, each item a shape and a color of its own, "x3" for a
//! triple; issue #102: the roulette is a slot machine's reel, `roulette.rs`, drawn with a renderer
//! by [`RoulettePlugin`]), the croquettes held (issue #14: the counter pops at
//! each pick-up, shakes when some are lost, turns gold at 10), the lap counter (issue #79: it pops
//! in gold at each new lap), the chantilly's cream over the screen (issue #18), and the results
//! screen (issue #70: live, until the race closes them).
//!
//! It follows the local player's racer (or, without one, the pole sitter) and only reads the
//! race and the croquettes: `crate::race` never depends on it. Texts are rewritten only when
//! their value changes.
//!
//! Issue #100: **it writes in Fredoka**, the charter's font (`menu::MenuAssets`, with a renderer;
//! headless, the default handle), with the accents; its texts keep a dark Myrtille shadow under
//! them, for the bright skies. The chrono is written from the left of a box of a fixed width
//! (Fredoka's digits are not all as wide: right-aligned, every digit of it would shake at each
//! new hundredth).
//! The results screen follows the charter: over the veil, its title, a cream panel of lines, the
//! player's in Mangue, each place on its badge (gold, silver, bronze, then blue).

pub mod debug;
pub mod format;
pub mod pop;
mod reel;
#[cfg(test)]
pub(crate) mod results_tests;
pub mod roulette;
#[cfg(test)]
mod roulette_tests;

pub use debug::{DEBUG_OVERLAY_KEY, DebugOverlay, DebugOverlayPlugin, SimulationCost};
pub use reel::{Reel, ReelCell, RoulettePlugin};

use bevy::prelude::*;

use crate::croquettes::{CroquetteTuning, Croquettes};
use crate::items::{ItemKind, ItemSlots, ItemTuning, Slot};
use crate::kart::{ItemEffects, PlayerKart};
use crate::menu::MenuAssets;
use crate::menu::kit;
use crate::menu::style::{palette, size, u};
use crate::race::{Course, Race, RacePhase, RaceProgress, RaceTuning, Racer, RacerName};
use crate::track::GRID_SLOTS;
use format::{
    HudValues, RESULTS_TITLE, ResultsTime, write_best_lap_or_section, write_croquettes,
    write_lap_or_section, write_ordinal, write_racer_count, write_racer_name, write_time,
};
use roulette::{ReelFrame, RouletteClock};

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
/// (they drive the croquette counter's pop and shake); since it started a new lap (issue #79: the
/// lap counter's pop). One resource for the HUD's counters: a resource is an entity in Bevy 0.20,
/// and one more archetype before the grid would change every bot race (see `STATUS.md`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CounterAnimation {
    pub since_gain: f32,
    pub since_loss: f32,
    pub since_lap: f32,
}

impl Default for CounterAnimation {
    fn default() -> Self {
        Self {
            since_gain: f32::INFINITY,
            since_loss: f32::INFINITY,
            since_lap: f32::INFINITY,
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

/// What a HUD slot shows: the item (`None`: empty), how many, whether the roulette spins, and
/// whether its border is lit (issue #102: on every other item of the reel, slower and slower).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotView {
    pub item: Option<ItemKind>,
    pub count: u8,
    pub spinning: bool,
    pub lit: bool,
}

impl SlotView {
    /// What `slot` shows, `reel` its roulette's window (`roulette::RouletteClock::frames`): a
    /// spinning roulette shows the item in the middle of its reel, the border lit on every other
    /// one.
    pub fn of(slot: &Slot, reel: Option<ReelFrame>) -> Self {
        match (*slot, reel) {
            (Slot::Spinning { .. }, Some(reel)) => Self {
                item: Some(reel.centered()),
                count: 1,
                spinning: true,
                lit: reel.cell.is_multiple_of(2),
            },
            (Slot::Spinning { item, .. }, None) => Self {
                item: Some(item),
                count: 1,
                spinning: true,
                lit: true,
            },
            (Slot::Ready { item, count }, _) => Self {
                item: Some(item),
                count,
                spinning: false,
                lit: false,
            },
            (Slot::Empty, _) => Self::default(),
        }
    }
}

/// The two item slots (issue #102: bigger, for the reel to be read as it scrolls): the hand's and
/// the reserve's sides (px), and the scale of their icons (`icon_shape`'s sizes).
pub const SLOT_SIZES: [f32; 2] = [92.0, 60.0];
pub const ICON_SCALES: [f32; 2] = [1.2, 0.75];
/// A slot's border (px).
const SLOT_BORDER_WIDTH: f32 = 4.0;
/// The slots' top (px from the top of the screen); under them, the paw gauge
/// (`interactions::visual`).
pub const SLOTS_TOP: f32 = 24.0;

/// The shape of an item's icon in a slot of the headless HUD: size (px), fill, border color and
/// width, rotation (degrees). Each item has its own shape and color (GD § 14: recognized at
/// once). With a renderer, the slots show the items' icons instead, their models rendered
/// (issue #106, `items::icons`, in the reel's cells).
pub fn icon_shape(item: ItemKind) -> (Vec2, Color, Color, f32, f32) {
    match item {
        // The Cac's (issue #137): a chocolate mound, its caramel sheen around it.
        ItemKind::Pelure => (
            Vec2::new(40.0, 34.0),
            Color::srgb(0.5, 0.25, 0.08),
            Color::srgb(0.8, 0.5, 0.24),
            5.0,
            0.0,
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
        // Issue #21. A black and white magpie: a black body, a white edge, slanted.
        ItemKind::PieVoleuse => (
            Vec2::new(40.0, 30.0),
            Color::srgb(0.08, 0.08, 0.12),
            Color::srgb(0.95, 0.95, 1.0),
            6.0,
            -20.0,
        ),
        // A piñata: sky blue, a thick pink ring.
        ItemKind::Pinata => (
            Vec2::splat(34.0),
            Color::srgb(0.35, 0.8, 1.0),
            Color::srgb(1.0, 0.25, 0.45),
            7.0,
            45.0,
        ),
        // A cream bone, slanted, edged in brown.
        ItemKind::Boomerang => (
            Vec2::new(52.0, 16.0),
            Color::srgb(0.98, 0.93, 0.8),
            Color::srgb(0.55, 0.4, 0.25),
            4.0,
            35.0,
        ),
    }
}

/// The results screen.
#[derive(Component, Clone, Copy, Debug)]
pub struct ResultsScreen;

/// Issue #100: a line of the results screen (0 = the first): its pill is the player's color
/// (Mangue) when the player is on it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultsRow {
    pub row: usize,
}

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
    /// "Meilleur tour 0:20.12", "Tour 2/3", "Temps estimé".
    Detail,
}

/// The columns, in order (their widths: `style::size::RESULTS_COLUMNS`).
const RESULTS_COLUMNS: [ResultsColumn; 4] = [
    ResultsColumn::Rank,
    ResultsColumn::Name,
    ResultsColumn::Time,
    ResultsColumn::Detail,
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

/// The HUD's colors (issue #100: the charter's palette): white texts, the rank and what pops in
/// Mangue, the slots' panels in see-through Myrtille, a dark Myrtille shadow under the texts
/// (readable on a white sky as on the road).
const TEXT_COLOR: Color = Color::WHITE;
const RANK_COLOR: Color = palette::MANGUE;
const PANEL_COLOR: Color = Color::srgba(0.227, 0.173, 0.431, 0.6);
const SLOT_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.85);
const CROQUETTE_COLOR: Color = Color::srgb(0.75, 0.45, 0.2);
const TEXT_SHADOW: Color = Color::srgba(0.141, 0.102, 0.290, 0.85);
/// The chrono's size, and the width of its box (px): « 0:00.00 » with Fredoka's widest digits
/// (0.58 of its size), written from its left (the box is the one that stays right-aligned).
const CHRONO_SIZE: f32 = 36.0;
const CHRONO_WIDTH: f32 = 128.0;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudShown>()
            .init_resource::<CounterAnimation>()
            .init_resource::<ResultsShown>()
            .add_systems(
                Update,
                (
                    spawn_hud.run_if(not(any_with_component::<HudRoot>)),
                    update_hud,
                    update_item_slots,
                    show_cream,
                    animate_counters,
                    show_results,
                    update_results,
                )
                    .chain()
                    .run_if(resource_exists::<Race>),
            );
    }
}

/// A text of the HUD: `size` px in `face` (Fredoka, issue #100), its shadow under it.
fn text(content: &str, face: &Handle<Font>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
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

fn spawn_hud(mut commands: Commands, mut shown: ResMut<HudShown>, assets: Option<Res<MenuAssets>>) {
    shown.0 = None;
    let (_, face) = kit::faces(assets.as_deref());
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
                top: px(SLOTS_TOP),
                column_gap: px(10),
                align_items: AlignItems::FlexStart,
                ..default()
            })
            .with_children(|slots| {
                for (index, size) in SLOT_SIZES.into_iter().enumerate() {
                    slots
                        .spawn((
                            ItemSlot { index },
                            Node {
                                width: px(size),
                                height: px(size),
                                border: UiRect::all(px(SLOT_BORDER_WIDTH)),
                                border_radius: BorderRadius::all(px(14)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(PANEL_COLOR),
                            BorderColor::all(SLOT_BORDER),
                        ))
                        .with_children(|slot| {
                            // The reserve's icon is smaller. With a renderer, the reel takes
                            // its place (issue #102, `reel.rs`).
                            slot.spawn((
                                ItemIcon { index },
                                Node {
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                UiTransform {
                                    scale: Vec2::splat(ICON_SCALES[index]),
                                    ..default()
                                },
                                Visibility::Hidden,
                            ));
                            slot.spawn((
                                ItemCount { index },
                                text("", &face, if index == 0 { 20.0 } else { 14.0 }, TEXT_COLOR),
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
                // Issue #100: in a box of a fixed width, from its left: Fredoka's digits are not
                // all as wide, but a new hundredth only moves the digits after it.
                times.spawn((
                    HudText::Chrono,
                    text("0:00.00", &face, CHRONO_SIZE, TEXT_COLOR),
                    Node {
                        width: px(CHRONO_WIDTH),
                        ..default()
                    },
                ));
                times.spawn((HudText::BestLap, text("", &face, 18.0, TEXT_COLOR)));
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
                    croquettes.spawn((HudText::Croquettes, text("x 0", &face, 26.0, TEXT_COLOR)));
                });
                left.spawn((HudText::Lap, text("", &face, 36.0, TEXT_COLOR)));
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
                rank.spawn((HudText::Rank, text("", &face, 72.0, RANK_COLOR)));
                rank.spawn((
                    HudText::RacerCount,
                    text("", &face, 30.0, TEXT_COLOR),
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
                    text("", &face, 110.0, TEXT_COLOR),
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
    mut animation: ResMut<CounterAnimation>,
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
        // Issue #79: the lap counter pops as the racer crosses the line into a new lap.
        if pop::lap_pops((old.lap, old.laps), (values.lap, values.laps)) && race.started() {
            animation.since_lap = 0.0;
        }
    }
    let changed = |field: HudText| {
        let Some(old) = previous else {
            return true;
        };
        match field {
            HudText::Rank => old.rank != values.rank,
            HudText::RacerCount => old.racers != values.racers,
            HudText::Lap => {
                old.lap != values.lap || old.laps != values.laps || old.sections != values.sections
            }
            HudText::Chrono => old.time != values.time,
            HudText::BestLap => old.best_lap != values.best_lap || old.sections != values.sections,
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
            HudText::Lap => write_lap_or_section(out, values.lap, values.laps, values.sections),
            HudText::Chrono => {
                out.clear();
                write_time(out, Some(values.time));
            }
            HudText::BestLap => write_best_lap_or_section(out, values.best_lap, values.sections),
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
/// the roulette (issue #102): the item in the middle of its reel, the border lit on every other
/// one. Rewritten only when what a slot shows changes. With a renderer the reel draws the
/// roulette (`reel.rs`: this icon is gone).
#[allow(clippy::too_many_arguments)]
fn update_item_slots(
    fixed: Res<Time<Fixed>>,
    tuning: Res<ItemTuning>,
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
    let items = followed(racers.iter()).and_then(|(entity, _)| held.get(entity).ok());
    let views = match items {
        Some(items) => {
            let [hand, reserve] = RouletteClock::new(&fixed, &tuning).frames(items);
            [
                SlotView::of(&items.hand, hand),
                SlotView::of(&items.reserve, reserve),
            ]
        }
        None => [SlotView::default(); 2],
    };
    if *shown == views {
        return;
    }
    for (slot, mut border) in &mut slots {
        let view = views[slot.index.min(1)];
        // The slot's border flashes gold while its roulette spins: on every other item.
        let color = if view.spinning && view.lit {
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

/// The counters' animations, every frame of them (nothing to do the rest of the time): the
/// croquette counter's pop (scale) and shake (offset); the lap counter's pop (issue #79), which
/// grows from its bottom left corner (it stays on screen, in the corner) and turns gold for
/// [`pop::LAP_POP_TIME`] after a new lap.
fn animate_counters(
    time: Res<Time>,
    mut animation: ResMut<CounterAnimation>,
    mut counters: Query<&mut UiTransform, With<CroquetteCounter>>,
    mut laps: Query<(&HudText, &mut UiTransform, &mut TextColor), Without<CroquetteCounter>>,
) {
    let dt = time.delta_secs();
    if animation.since_gain.is_finite() || animation.since_loss.is_finite() {
        animation.since_gain += dt;
        animation.since_loss += dt;
        if animation.since_gain > pop::POP_TIME && animation.since_loss > pop::SHAKE_TIME {
            animation.since_gain = f32::INFINITY;
            animation.since_loss = f32::INFINITY;
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

    if !animation.since_lap.is_finite() {
        return;
    }
    animation.since_lap += dt;
    let popping = animation.since_lap < pop::LAP_POP_TIME;
    if !popping {
        animation.since_lap = f32::INFINITY;
    }
    let scale = pop::lap_pop_scale(animation.since_lap);
    // Scaled about its middle, then moved by half its growth (a share of its own size): right
    // and up.
    let shift = 50.0 * (scale - 1.0);
    let wanted = UiTransform {
        scale: Vec2::splat(scale),
        translation: Val2::percent(shift, -shift),
        ..default()
    };
    let color = TextColor(if popping { RANK_COLOR } else { TEXT_COLOR });
    for (field, mut transform, mut text_color) in &mut laps {
        if *field == HudText::Lap {
            transform.set_if_neq(wanted);
            text_color.set_if_neq(color);
        }
    }
}

/// Spawns the results screen when the results phase starts, removes it on restart. Online
/// (issue #9): the host's order (`race::OnlineFlow`), the other players' names. Its lines are
/// filled by [`update_results`] (issue #70). Issue #136: under its panel, no line of keys any more
/// but the slot of its buttons (`kit::EndSlot`: « Rejouer », « Changer de circuit », « Accueil »…,
/// `menu::end`, with a renderer).
fn show_results(
    mut commands: Commands,
    race: Res<Race>,
    grand_prix: Option<Res<crate::race::GrandPrix>>,
    assets: Option<Res<MenuAssets>>,
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
    let (medium, semibold) = kit::faces(assets.as_deref());
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
            // Issue #100: the charter's veil, the race seen behind it.
            BackgroundColor(palette::VOILE),
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: u(10.0),
                    ..default()
                })
                .with_children(|column| {
                    column.spawn(kit::title(RESULTS_TITLE, &semibold, size::SCREEN_TITLE));
                    column
                        .spawn(kit::panel(size::RESULTS_WIDTH, size::ROW_GAP))
                        .with_children(|panel| {
                            for row in 0..rows {
                                write_ordinal(&mut rank, row as u32 + 1);
                                panel
                                    .spawn((
                                        ResultsRow { row },
                                        kit::row(size::ROW_HEIGHT, size::CELL_GAP, false),
                                    ))
                                    .with_children(|line| {
                                        spawn_results_cells(line, row, &rank, (&medium, &semibold));
                                    });
                            }
                        });
                    column.spawn(kit::end_slot());
                });
        });
}

/// The cells of the line `row` of the results screen: its place on its badge (written once), then
/// the name, the time and the details (filled by [`update_results`]).
fn spawn_results_cells(
    line: &mut ChildSpawnerCommands,
    row: usize,
    rank: &str,
    (medium, semibold): (&Handle<Font>, &Handle<Font>),
) {
    for (column, width) in RESULTS_COLUMNS.into_iter().zip(size::RESULTS_COLUMNS) {
        let cell = ResultsCell { row, column };
        match column {
            ResultsColumn::Rank => {
                line.spawn(kit::place_badge(row, width))
                    .with_children(|badge| {
                        badge.spawn((
                            cell,
                            kit::text(rank, semibold, size::PLACE_TEXT, palette::MYRTILLE),
                        ));
                    });
            }
            ResultsColumn::Detail => {
                line.spawn((
                    cell,
                    kit::text("", medium, size::ROW_SMALL, palette::MYRTILLE),
                    Node {
                        width: u(width),
                        ..default()
                    },
                ));
            }
            ResultsColumn::Name | ResultsColumn::Time => {
                line.spawn((
                    cell,
                    kit::text("", semibold, size::ROW_TEXT, palette::MYRTILLE),
                    Node {
                        width: u(width),
                        ..default()
                    },
                ));
            }
        }
    }
}

/// The line `row` of the results screen: the racer at that place and its race, live (the
/// standings: online, the host's order), or from the closed results (issue #70).
fn results_row(
    race: &Race,
    course: &Course,
    row: usize,
    racers: &Query<(
        &Racer,
        &RaceProgress,
        Has<PlayerKart>,
        Option<Ref<RacerName>>,
    )>,
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
/// new entity). Issue #80: a player who changes its pseudo (in the lobby, online) has its new
/// name on its line at once. Issue #100: the line the player is on is Mangue, wherever it goes.
fn update_results(
    race: Res<Race>,
    course: Res<Course>,
    mut shown: ResMut<ResultsShown>,
    racers: Query<(
        &Racer,
        &RaceProgress,
        Has<PlayerKart>,
        Option<Ref<RacerName>>,
    )>,
    mut cells: Query<(&ResultsCell, &mut Text)>,
    mut rows: Query<(&ResultsRow, &mut BackgroundColor, &mut BorderColor)>,
) {
    if race.phase() != RacePhase::Results {
        return;
    }
    let mut views = [None; GRID_SLOTS];
    for (row, view) in views.iter_mut().enumerate().take(shown.rows) {
        *view = results_row(&race, &course, row, &racers);
    }
    let renamed = racers
        .iter()
        .any(|(.., name)| name.is_some_and(|name| name.is_changed()));
    if views == shown.views && !renamed {
        return;
    }
    // The local player's line in Mangue, wherever it goes.
    for (line, mut fill, mut depth) in &mut rows {
        let player = views
            .get(line.row)
            .copied()
            .flatten()
            .is_some_and(|view| view.player);
        let (wanted_fill, wanted_depth) = kit::row_colors(player);
        fill.set_if_neq(wanted_fill);
        depth.set_if_neq(wanted_depth);
    }
    for (cell, mut text) in &mut cells {
        let Some(old) = shown.views.get(cell.row).copied() else {
            continue;
        };
        let new = views[cell.row];
        let rename = renamed && cell.column == ResultsColumn::Name;
        if old == new && !rename {
            continue;
        }
        if !cell.column.changed(old, new) && !rename {
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
                let (slot, name) = racers
                    .get(view.racer)
                    .ok()
                    .map_or((0, None), |(racer, .., name)| {
                        (racer.grid_slot, name.map(|name| name.into_inner()))
                    });
                write_racer_name(out, view.player, slot, name.map(|name| name.0.as_str()));
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
