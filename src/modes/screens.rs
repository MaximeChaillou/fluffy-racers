//! The screens of a Grand Prix (issue #19; issue #99: a championship): the overall standings
//! after each race (place, racer, the points of the race, the total), then the podium's names
//! over the 3D podium. Spawned once per stage (a handful of texts, no change while shown), over
//! the results screen; part of the level (`LevelScoped`).
//!
//! Issue #100: after the charter (`menu::kit`), in Fredoka with the accents (« Coupe Doudou 3/4 ·
//! Crique Câline », « classement général »): the standings over the veil, their title in white, a
//! cream panel of lines (the player's in Mangue, each place on its badge: gold, silver, bronze,
//! then blue), the keys under it; the podium's names in a small cream panel at the top, the 3D
//! podium in sight under it (no veil).

use bevy::prelude::*;

use crate::level::LevelScoped;
use crate::menu::MenuAssets;
use crate::menu::kit;
use crate::menu::style::{palette, size, u};
use crate::race::{GrandPrix, GrandPrixStage, Race, RacePhase};
use crate::ui::format::{PLAYER_NAME, write_ordinal};

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

/// The title of the standings: « Coupe Doudou 2/4 · Anneau Guimauve » (issue #99: the cup's name,
/// or « Championnat » for a custom one; issue #100: as written, accents included).
pub fn standings_title(grand_prix: &GrandPrix) -> String {
    let name = grand_prix
        .cup
        .get(grand_prix.race)
        .map_or("", |race| race.name.as_str());
    format!(
        "{} {}/{} · {name}",
        grand_prix.title,
        grand_prix.race + 1,
        grand_prix.cup.len()
    )
}

/// What Entrée does next, under the standings.
pub fn standings_footer(grand_prix: &GrandPrix) -> String {
    match grand_prix.cup.get(grand_prix.race + 1) {
        Some(next) => format!(
            "Espace ou Entrée (manette : A) : course suivante ({})",
            next.name
        ),
        None => "Espace ou Entrée (manette : A) : le podium".to_string(),
    }
}

pub const STANDINGS_HEADER: &str = "Classement général";
pub const PODIUM_TITLE: &str = "Podium";
pub const PODIUM_FOOTER: &str = "Entrée : on recommence  ·  N : changer de mode";

/// Shows the screen of the Grand Prix's stage (none while racing), over the results screen (only
/// there: not on a level being rebuilt).
pub(super) fn show_grand_prix_screen(
    mut commands: Commands,
    grand_prix: Option<Res<GrandPrix>>,
    race: Option<Res<Race>>,
    assets: Option<Res<MenuAssets>>,
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
    let faces = kit::faces(assets.as_deref());
    match stage {
        GrandPrixStage::Standings => spawn_standings(&mut commands, &grand_prix, &faces),
        GrandPrixStage::Podium => spawn_podium_names(&mut commands, &grand_prix, &faces),
        GrandPrixStage::Racing => {}
    }
}

/// A line of a table: its place on its badge, then its cells (text, width, semi-bold).
fn spawn_line(
    panel: &mut ChildSpawnerCommands,
    place: usize,
    line: &StandingsLine,
    cells: &[(&str, f32)],
    (_, semibold): &(Handle<Font>, Handle<Font>),
) {
    panel
        .spawn(kit::row(size::ROW_HEIGHT, size::CELL_GAP, line.player))
        .with_children(|row| {
            row.spawn(kit::place_badge(place, size::PLACE_BADGE))
                .with_children(|badge| {
                    badge.spawn(kit::text(
                        line.rank.as_str(),
                        semibold,
                        size::PLACE_TEXT,
                        palette::MYRTILLE,
                    ));
                });
            for (text, width) in cells {
                row.spawn((
                    kit::text(*text, semibold, size::ROW_TEXT, palette::MYRTILLE),
                    Node {
                        width: u(*width),
                        ..default()
                    },
                ));
            }
        });
}

fn spawn_standings(
    commands: &mut Commands,
    grand_prix: &GrandPrix,
    faces: &(Handle<Font>, Handle<Font>),
) {
    let lines = standings_lines(grand_prix);
    let (medium, semibold) = faces;
    let [_, name, won, total] = size::STANDINGS_COLUMNS;
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
                    column.spawn(kit::title(
                        standings_title(grand_prix),
                        semibold,
                        size::SCREEN_TITLE,
                    ));
                    column
                        .spawn(kit::panel(size::STANDINGS_WIDTH, size::ROW_GAP))
                        .with_children(|panel| {
                            panel.spawn((
                                kit::text(
                                    STANDINGS_HEADER,
                                    semibold,
                                    size::SOLO_HEADER,
                                    palette::MYRTILLE,
                                ),
                                Node {
                                    margin: UiRect::left(u(6.0)),
                                    ..default()
                                },
                            ));
                            for (place, line) in lines.iter().enumerate() {
                                let cells = [
                                    (line.name.as_str(), name),
                                    (line.won.as_str(), won),
                                    (line.total.as_str(), total),
                                ];
                                spawn_line(panel, place, line, &cells, faces);
                            }
                        });
                    column.spawn(kit::keys_line(standings_footer(grand_prix), medium));
                });
        });
}

/// The names of the podium, at the top of the screen: the 3D podium stays in sight.
fn spawn_podium_names(
    commands: &mut Commands,
    grand_prix: &GrandPrix,
    faces: &(Handle<Font>, Handle<Font>),
) {
    let lines = standings_lines(grand_prix);
    let (medium, semibold) = faces;
    let [_, name, _, total] = size::STANDINGS_COLUMNS;
    commands
        .spawn((
            Name::new("Grand Prix podium"),
            GrandPrixScreen(GrandPrixStage::Podium),
            LevelScoped,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: u(16.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|screen| {
            screen
                .spawn(kit::panel(size::PODIUM_WIDTH, size::ROW_GAP))
                .with_children(|panel| {
                    panel.spawn((
                        kit::text(
                            PODIUM_TITLE,
                            semibold,
                            size::PILOT_TITLE * 0.75,
                            palette::MYRTILLE,
                        ),
                        TextShadow {
                            offset: Vec2::new(0.0, 3.0),
                            color: palette::FRAISE.with_alpha(0.6),
                        },
                        Node {
                            align_self: AlignSelf::Center,
                            ..default()
                        },
                    ));
                    for (place, line) in lines.iter().enumerate().take(3) {
                        let cells = [(line.name.as_str(), name), (line.total.as_str(), total)];
                        spawn_line(panel, place, line, &cells, faces);
                    }
                    panel.spawn((
                        kit::text(
                            PODIUM_FOOTER,
                            medium,
                            size::HINT,
                            palette::MYRTILLE.with_alpha(0.75),
                        ),
                        Node {
                            align_self: AlignSelf::Center,
                            margin: UiRect::top(u(4.0)),
                            ..default()
                        },
                    ));
                });
        });
}
