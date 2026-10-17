//! Issue #100: the charter's pieces for the screens drawn outside the menus' trees, with the HUD:
//! the results of a race (`ui`), the overall standings and the podium of a championship
//! (`modes`). Pure bundles and colors: no system, no resource of their own. Their font is
//! [`MenuAssets`]' Fredoka when the game has a renderer; headless (no font asset), the default
//! handle: nothing is drawn anyway, and the headless world keeps the archetypes it had.

use bevy::prelude::*;
use bevy::text::FontSource;

use super::screen::MenuAssets;
use super::style::{font, palette, size, u};

/// The two weights of Fredoka: (medium, semi-bold); the default handles without a renderer.
pub fn faces(assets: Option<&MenuAssets>) -> (Handle<Font>, Handle<Font>) {
    assets.map_or_else(Default::default, |assets| {
        (assets.medium.clone(), assets.semibold.clone())
    })
}

/// A text of `px` pixels of the reference window (`style::font`), in `face`.
pub fn text(content: impl Into<String>, face: &Handle<Font>, px: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: FontSource::from(face),
            font_size: font(px),
            ..default()
        },
        TextColor(color),
    )
}

/// A screen's title over the veil: white, its Fraise shadow under it (marshmallow letters).
pub fn title(content: impl Into<String>, face: &Handle<Font>, px: f32) -> impl Bundle {
    (
        text(content, face, px, palette::NEIGE),
        TextShadow {
            offset: Vec2::new(0.0, px / 12.0),
            color: palette::FRAISE,
        },
    )
}

/// A line of keys under a panel: white over the veil, with a shadow.
pub fn keys_line(content: impl Into<String>, face: &Handle<Font>) -> impl Bundle {
    (
        text(content, face, size::HINT, palette::NEIGE),
        TextShadow {
            offset: Vec2::new(0.0, 2.0),
            color: palette::OMBRE,
        },
    )
}

/// The cream panel (`width` px of the reference window): round corners, a sharp shadow under it,
/// its lines `gap` apart.
pub fn panel(width: f32, gap: f32) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            width: u(width),
            padding: UiRect::all(u(size::PANEL_PADDING)),
            row_gap: u(gap),
            border_radius: BorderRadius::all(u(size::PANEL_RADIUS)),
            ..default()
        },
        BackgroundColor(palette::GUIMAUVE),
        BoxShadow::new(
            palette::OMBRE,
            Val::ZERO,
            u(size::PANEL_SHADOW),
            Val::ZERO,
            Val::ZERO,
        ),
    )
}

/// The look of a line of a table (a pill with its depth): the player's in Mangue (the focus's
/// color: « that's you »), the others in Coton.
pub fn row_colors(player: bool) -> (BackgroundColor, BorderColor) {
    if player {
        (
            BackgroundColor(palette::MANGUE),
            BorderColor::all(palette::ABRICOT),
        )
    } else {
        (
            BackgroundColor(palette::COTON),
            BorderColor::all(palette::LILAS),
        )
    }
}

/// A line of a table: a pill `height` px high, its depth under it, its cells `gap` apart.
pub fn row(height: f32, gap: f32, player: bool) -> impl Bundle {
    let (fill, depth) = row_colors(player);
    (
        Node {
            height: u(height),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: u(gap),
            padding: UiRect::horizontal(u(size::ROW_PADDING)),
            border: UiRect::bottom(u(size::ROW_DEPTH)),
            border_radius: BorderRadius::all(u(height / 2.0)),
            ..default()
        },
        fill,
        depth,
    )
}

/// The color of the round badge of a place (0 = the first): gold (Mangue), silver (Brume),
/// bronze (Carotte), then the Lagon. Its number is written in Myrtille on every one of them.
pub fn place_color(place: usize) -> Color {
    match place {
        0 => palette::MANGUE,
        1 => palette::BRUME_FONCEE,
        2 => palette::CAROTTE,
        _ => palette::LAGON,
    }
}

/// The round badge of a place (0 = the first), `px` px wide.
pub fn place_badge(place: usize, px: f32) -> impl Bundle {
    (
        Node {
            width: u(px),
            height: u(px),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(place_color(place)),
    )
}
