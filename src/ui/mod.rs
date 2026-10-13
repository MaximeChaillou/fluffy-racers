//! HUD (`bevy_ui`, issue #5): rank, lap, chrono, best lap, countdown and banners, empty slots
//! for the items (issue #16) and the croquettes (issue #14), and the results screen.
//!
//! It follows the local player's racer (or, without one, the pole sitter) and only reads the
//! race: `crate::race` never depends on it. Texts are rewritten only when their value changes.

pub mod debug;
pub mod format;

pub use debug::{DEBUG_OVERLAY_KEY, DebugOverlay, DebugOverlayPlugin, SimulationCost};

use bevy::prelude::*;

use crate::kart::PlayerKart;
use crate::race::{Course, Race, RacePhase, RaceProgress, RaceTuning, Racer};
use format::{
    HudValues, RESULTS_FOOTER, RESULTS_TITLE, hundredths, write_best_lap, write_croquettes,
    write_lap, write_ordinal, write_racer_count, write_racer_name, write_time,
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

/// An empty item slot (GD § 8: one in hand, one in reserve). Filled by issue #16.
#[derive(Component, Clone, Copy, Debug)]
pub struct ItemSlot {
    /// 0 = in hand (big), 1 = reserve.
    pub index: usize,
}

/// The results screen.
#[derive(Component, Clone, Copy, Debug)]
pub struct ResultsScreen;

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
        app.init_resource::<HudShown>().add_systems(
            Update,
            (
                spawn_hud.run_if(not(any_with_component::<HudRoot>)),
                update_hud,
                show_results,
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
            // Top left: the two item slots (empty until issue #16).
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
                    slots.spawn((
                        ItemSlot { index },
                        Node {
                            width: px(size),
                            height: px(size),
                            border: UiRect::all(px(4)),
                            border_radius: BorderRadius::all(px(14)),
                            ..default()
                        },
                        BackgroundColor(PANEL_COLOR),
                        BorderColor::all(SLOT_BORDER),
                    ));
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
                left.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
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
    racers: impl Iterator<Item = (&'a Racer, &'a RaceProgress, bool)>,
) -> Option<&'a RaceProgress> {
    let mut best: Option<(bool, usize, &RaceProgress)> = None;
    for (racer, progress, player) in racers {
        let better = match best {
            None => true,
            Some((best_player, best_slot, _)) => {
                (player, std::cmp::Reverse(racer.grid_slot))
                    > (best_player, std::cmp::Reverse(best_slot))
            }
        };
        if better {
            best = Some((player, racer.grid_slot, progress));
        }
    }
    best.map(|(_, _, progress)| progress)
}

fn update_hud(
    race: Res<Race>,
    course: Res<Course>,
    tuning: Res<RaceTuning>,
    mut shown: ResMut<HudShown>,
    racers: Query<(&Racer, &RaceProgress, Has<PlayerKart>)>,
    mut texts: Query<(&HudText, &mut Text)>,
) {
    let Some(progress) = followed(racers.iter()) else {
        return;
    };
    let values = HudValues::new(&race, &course, &tuning, progress, racers.iter().count());
    let previous = shown.0;
    if previous == Some(values) {
        return;
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
            HudText::Croquettes => false,
        }
    };
    for (field, mut text) in &mut texts {
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
            HudText::Croquettes => write_croquettes(out, 0),
        }
    }
    shown.0 = Some(values);
}

/// Spawns the results screen when the results phase starts, removes it on restart.
fn show_results(
    mut commands: Commands,
    race: Res<Race>,
    screens: Query<Entity, With<ResultsScreen>>,
    racers: Query<(&Racer, &RaceProgress, Has<PlayerKart>)>,
) {
    let showing = race.phase() == RacePhase::Results;
    if !showing {
        for screen in &screens {
            commands.entity(screen).despawn();
        }
        return;
    }
    if !screens.is_empty() {
        return;
    }
    let mut line = String::new();
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
                    for (i, entity) in race.standings.iter().enumerate() {
                        let Ok((racer, progress, player)) = racers.get(*entity) else {
                            continue;
                        };
                        let color = if player { RANK_COLOR } else { TEXT_COLOR };
                        panel
                            .spawn(Node {
                                column_gap: px(18),
                                ..default()
                            })
                            .with_children(|row| {
                                let mut cell = |content: &str, width: f32| {
                                    row.spawn((
                                        text(content, 24.0, color),
                                        Node {
                                            width: px(width),
                                            ..default()
                                        },
                                    ));
                                };
                                write_ordinal(&mut line, i as u32 + 1);
                                cell(&line, 60.0);
                                write_racer_name(&mut line, player, racer.grid_slot);
                                cell(&line, 110.0);
                                line.clear();
                                write_time(&mut line, progress.finish.map(hundredths));
                                cell(&line, 110.0);
                                write_best_lap(&mut line, progress.best_lap().map(hundredths));
                                cell(&line, 260.0);
                            });
                    }
                    panel.spawn((
                        text(RESULTS_FOOTER, 20.0, TEXT_COLOR),
                        Node {
                            margin: UiRect::top(px(12)),
                            ..default()
                        },
                    ));
                });
        });
}
