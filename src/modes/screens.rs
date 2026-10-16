//! The screens of a Grand Prix (issue #19), with the HUD's `bevy_ui` (its font is ASCII only): the
//! overall standings after each race (place, racer, the points of the race, the total), then the
//! podium's names over the 3D podium. Spawned once per stage (a handful of texts, no change while
//! shown), over the results screen; part of the level (`LevelScoped`).

use bevy::prelude::*;

use crate::level::LevelScoped;
use crate::race::{GrandPrix, GrandPrixStage, Race, RacePhase};
use crate::ui::format::{PLAYER_NAME, ascii_letter, write_ordinal};

/// The root of a Grand Prix screen, and the stage it shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrandPrixScreen(pub GrandPrixStage);

/// A line of the overall standings, as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandingsLine {
    /// « 1er », « 2e »...
    pub rank: String,
    /// « Toi » or « Kart N ».
    pub name: String,
    /// « +15 »: the points of the last race.
    pub won: String,
    /// « 42 pts »: the total.
    pub total: String,
    pub player: bool,
}

/// The name of an entrant: the player's, or « Kart N » (the bots' numbers, as on the results).
pub fn entrant_name(grand_prix: &GrandPrix, entrant: usize) -> String {
    if entrant == grand_prix.player() {
        PLAYER_NAME.to_string()
    } else {
        format!("Kart {}", entrant + 1)
    }
}

/// The lines of the overall standings, first to last.
pub fn standings_lines(grand_prix: &GrandPrix) -> Vec<StandingsLine> {
    grand_prix
        .standings()
        .iter()
        .enumerate()
        .map(|(place, entrant)| {
            let mut rank = String::new();
            write_ordinal(&mut rank, place as u32 + 1);
            StandingsLine {
                rank,
                name: entrant_name(grand_prix, *entrant),
                won: format!("+{}", grand_prix.last_points[*entrant]),
                total: format!("{} pts", grand_prix.points[*entrant]),
                player: *entrant == grand_prix.player(),
            }
        })
        .collect()
}

/// A text the HUD's font can write (accents dropped).
fn ascii(text: &str) -> String {
    text.chars().map(ascii_letter).collect()
}

/// The title of the standings: « GRAND PRIX 2/4 - Anneau Guimauve ».
pub fn standings_title(grand_prix: &GrandPrix) -> String {
    let name = grand_prix
        .cup
        .get(grand_prix.race)
        .map_or_else(String::new, |race| ascii(&race.name));
    format!(
        "GRAND PRIX {}/{} - {name}",
        grand_prix.race + 1,
        grand_prix.cup.len()
    )
}

/// What Entrée does next, under the standings.
pub fn standings_footer(grand_prix: &GrandPrix) -> String {
    match grand_prix.cup.get(grand_prix.race + 1) {
        Some(next) => format!(
            "ESPACE ou ENTREE (manette : A) : course suivante ({})",
            ascii(&next.name)
        ),
        None => "ESPACE ou ENTREE (manette : A) : le podium".to_string(),
    }
}

pub const PODIUM_TITLE: &str = "PODIUM";
pub const PODIUM_FOOTER: &str = "ENTREE : nouveau Grand Prix - N : changer de mode";

const TEXT_COLOR: Color = Color::WHITE;
const GOLD: Color = Color::srgb(1.0, 0.85, 0.25);
/// Column widths of the standings (px): rank, name, points of the race, total.
const COLUMNS: [f32; 4] = [60.0, 120.0, 70.0, 100.0];

fn text(content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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

/// Shows the screen of the Grand Prix's stage (none while racing), over the results screen (only
/// there: not on a level being rebuilt).
pub(super) fn show_grand_prix_screen(
    mut commands: Commands,
    grand_prix: Option<Res<GrandPrix>>,
    race: Option<Res<Race>>,
    screens: Query<(Entity, &GrandPrixScreen)>,
) {
    let results = race.is_some_and(|race| race.phase() == RacePhase::Results);
    let wanted = grand_prix
        .as_deref()
        .map(|gp| gp.stage)
        .filter(|stage| *stage != GrandPrixStage::Racing && results);
    let mut shown = false;
    for (entity, screen) in &screens {
        if Some(screen.0) == wanted {
            shown = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    let (Some(grand_prix), Some(stage), false) = (grand_prix, wanted, shown) else {
        return;
    };
    match stage {
        GrandPrixStage::Standings => spawn_standings(&mut commands, &grand_prix),
        GrandPrixStage::Podium => spawn_podium_names(&mut commands, &grand_prix),
        GrandPrixStage::Racing => {}
    }
}

fn spawn_standings(commands: &mut Commands, grand_prix: &GrandPrix) {
    let lines = standings_lines(grand_prix);
    commands
        .spawn((
            Name::new("Grand Prix standings"),
            GrandPrixScreen(GrandPrixStage::Standings),
            LevelScoped,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(24)),
                        row_gap: px(6),
                        border_radius: BorderRadius::all(px(18)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.06, 0.20, 0.95)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        text(standings_title(grand_prix), 34.0, GOLD),
                        Node {
                            margin: UiRect::bottom(px(8)),
                            ..default()
                        },
                    ));
                    for line in &lines {
                        let color = if line.player { GOLD } else { TEXT_COLOR };
                        panel
                            .spawn(Node {
                                column_gap: px(18),
                                ..default()
                            })
                            .with_children(|row| {
                                let cells = [&line.rank, &line.name, &line.won, &line.total];
                                for (cell, width) in cells.into_iter().zip(COLUMNS) {
                                    row.spawn((
                                        text(cell.as_str(), 24.0, color),
                                        Node {
                                            width: px(width),
                                            ..default()
                                        },
                                    ));
                                }
                            });
                    }
                    panel.spawn((
                        text(standings_footer(grand_prix), 20.0, TEXT_COLOR),
                        Node {
                            margin: UiRect::top(px(12)),
                            ..default()
                        },
                    ));
                });
        });
}

/// The names of the podium, at the top of the screen: the 3D podium stays in sight.
fn spawn_podium_names(commands: &mut Commands, grand_prix: &GrandPrix) {
    let lines = standings_lines(grand_prix);
    commands
        .spawn((
            Name::new("Grand Prix podium"),
            GrandPrixScreen(GrandPrixStage::Podium),
            LevelScoped,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: px(16),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(16)),
                        row_gap: px(4),
                        border_radius: BorderRadius::all(px(16)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.06, 0.20, 0.8)),
                ))
                .with_children(|panel| {
                    panel.spawn(text(PODIUM_TITLE, 40.0, GOLD));
                    for line in lines.iter().take(3) {
                        let color = if line.player { GOLD } else { TEXT_COLOR };
                        panel.spawn(text(
                            format!("{}  {}  {}", line.rank, line.name, line.total),
                            26.0,
                            color,
                        ));
                    }
                    panel.spawn((
                        text(PODIUM_FOOTER, 18.0, TEXT_COLOR),
                        Node {
                            margin: UiRect::top(px(8)),
                            ..default()
                        },
                    ));
                });
        });
}
