//! The menus drawn with `bevy_ui`, after the charter (issue #56, `style.rs`, `anim.rs`): a veil
//! over the game, the title, a cream panel of pill buttons (an icon on a round badge, a label, a
//! small line), the keys at the bottom. Opened from the game, all of it in the middle of the
//! screen, over a veil. The home screen (issue #89): the column on the left, its title big and
//! alive (its two words bob), over a veil that fades out toward the right, where the background's
//! karts race.
//!
//! **Closed, nothing is left**: the tree is spawned when the menu opens and despawned (with its
//! observers) when it closes; the systems that animate it only run while it is open. Opened, one
//! system moves 6 transforms and sets a few colors per frame, without laying the tree out again
//! (`UiTransform`); a press changes a button's depth (one layout).
//!
//! The mouse goes through the buttons' observers (picking): the pointer over a button gives it
//! the focus, a press squashes it, its release chooses it. Headless, there is no picking: the
//! tests write the same [`MainMenuAction`]s.

use bevy::picking::Pickable;
use bevy::picking::events::{PointerOut, PointerOver, PointerPress, PointerRelease};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{FontSource, Justify, LineHeight};

use super::anim;
use super::nav::{self, Entry};
use super::style::{self, ButtonState, font, palette, size, u};
use super::{MainMenu, MainMenuAction};
use crate::admin::tracks::CupIcon;
use crate::kart::KeyboardLayout;
use crate::level::Online;

/// On every entity of the menu's tree: none is left once it is closed.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MenuNode;

/// The root of the tree (the veil over the game), with what it was built for: another menu
/// (opened from the game or at launch, online or not, another keyboard) is another tree.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct MainMenuRoot {
    pub key: TreeKey,
}

/// What the tree shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeKey {
    pub in_game: bool,
    pub online: bool,
    pub layout: KeyboardLayout,
}

/// The column of the title, the panel and the keys: it pops in.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct MenuColumn;

/// Issue #89: a word of the home screen's big title (0: « Fluffy », 1: « Racers »): it bobs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct TitleWord(pub usize);

/// A button of the menu: its entry, by its index in the menu.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct MenuButton {
    pub index: usize,
    pub entry: Entry,
}

/// The round badge of a button's icon.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct MenuBadge {
    pub index: usize,
}

/// A text of a button: its label or its small line.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct MenuText {
    pub index: usize,
    pub caption: bool,
}

/// The fonts and icons of the screens, loaded once with a renderer. Headless (no font, no image
/// assets), the default handles: nothing is drawn anyway.
#[derive(Resource, Clone, Debug, Default)]
pub struct MenuAssets {
    pub medium: Handle<Font>,
    pub semibold: Handle<Font>,
    pub icons: [Handle<Image>; 7],
    /// Issue #99: the cups' icons, in the order of `CupIcon::ALL`.
    pub cups: [Handle<Image>; 5],
}

impl MenuAssets {
    fn icon(&self, entry: Entry) -> Handle<Image> {
        self.icons[icon_index(entry)].clone()
    }

    /// Issue #99: a cup's icon.
    pub fn cup(&self, icon: CupIcon) -> Handle<Image> {
        let index = CupIcon::ALL.iter().position(|i| *i == icon).unwrap_or(0);
        self.cups[index].clone()
    }
}

fn icon_index(entry: Entry) -> usize {
    match entry {
        Entry::Resume => 0,
        Entry::Solo => 1,
        Entry::Online => 2,
        Entry::Options => 3,
        Entry::Garage => 4,
        Entry::Quit => 5,
        Entry::Restart => 6,
    }
}

/// The color of an entry's badge (`docs/UI_CHARTE.md` « Icônes »).
pub fn accent(entry: Entry) -> Color {
    match entry {
        Entry::Resume => palette::MENTHE,
        Entry::Solo => palette::LAGON,
        Entry::Online => palette::FRAISE,
        Entry::Options => palette::PRUNE,
        Entry::Garage => palette::CAROTTE,
        // Issue #89: the ink, home.
        Entry::Quit => palette::MYRTILLE,
        // Issue #100: the blue of « Jouer solo »: the race again.
        Entry::Restart => palette::LAGON,
    }
}

pub(super) fn load_menu_assets(
    mut commands: Commands,
    server: Option<Res<AssetServer>>,
    fonts: Option<Res<Assets<Font>>>,
    images: Option<Res<Assets<Image>>>,
) {
    let (Some(server), Some(_), Some(_)) = (server, fonts, images) else {
        commands.insert_resource(MenuAssets::default());
        return;
    };
    commands.insert_resource(MenuAssets {
        medium: server.load(style::FONT_MEDIUM),
        semibold: server.load(style::FONT_SEMIBOLD),
        icons: [
            style::ICON_RESUME,
            style::ICON_SOLO,
            style::ICON_ONLINE,
            style::ICON_OPTIONS,
            style::ICON_GARAGE,
            style::ICON_QUIT,
            style::ICON_RESTART,
        ]
        .map(|path| server.load(path)),
        cups: CupIcon::ALL.map(|icon| server.load(style::cup_icon(icon))),
    });
}

/// The tree must be built, rebuilt or removed: the menu is open, or a tree is still there.
pub(super) fn tree_wanted_or_shown(
    menu: Res<MainMenu>,
    roots: Query<(), With<MainMenuRoot>>,
) -> bool {
    menu.open || !roots.is_empty()
}

/// The menu open: its tree, for what it shows now. Closed: none. Not before the game has a
/// camera (at launch, the circuit is still loading): the sizes are shares of its view; its
/// pop-in starts when it shows up.
pub(super) fn sync_tree(
    mut commands: Commands,
    mut menu: ResMut<MainMenu>,
    time: Res<Time<Real>>,
    roots: Query<(Entity, &MainMenuRoot)>,
    assets: Option<Res<MenuAssets>>,
    layout: Option<Res<KeyboardLayout>>,
    online: Option<Res<Online>>,
    cameras: Query<(), With<Camera>>,
) {
    if menu.open && cameras.is_empty() {
        return;
    }
    let key = TreeKey {
        in_game: menu.in_game,
        online: online.is_some(),
        layout: layout.map_or_else(KeyboardLayout::default, |layout| *layout),
    };
    let mut shown = false;
    for (entity, root) in &roots {
        if menu.open && !shown && root.key == key {
            shown = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if menu.open && !shown {
        let assets = assets.map(|assets| assets.clone()).unwrap_or_default();
        spawn_tree(&mut commands, key, &assets);
        // It pops in from now: when it shows up (at launch, once the game has a camera).
        menu.opened_at = super::now(&time);
    }
}

fn text(content: impl Into<String>, face: &Handle<Font>, px: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: FontSource::from(face),
            font_size: font(px),
            ..default()
        },
        TextColor(color),
        Pickable::IGNORE,
        MenuNode,
    )
}

fn spawn_tree(commands: &mut Commands, key: TreeKey, assets: &MenuAssets) {
    if key.in_game {
        spawn_game_menu(commands, key, assets);
    } else {
        spawn_home(commands, key, assets);
    }
}

/// The panel of buttons of a menu.
fn spawn_panel(column: &mut ChildSpawnerCommands, key: TreeKey, assets: &MenuAssets) {
    column
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                width: u(size::PANEL_WIDTH),
                padding: UiRect::all(u(size::PANEL_PADDING)),
                row_gap: u(size::PANEL_GAP),
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
            Pickable::IGNORE,
        ))
        .with_children(|panel| {
            for (index, entry) in nav::entries(key.in_game).iter().copied().enumerate() {
                spawn_button(panel, index, entry, key.online, assets);
            }
        });
}

/// The keys at the bottom of a menu, `justify`-ed.
fn spawn_hints(
    column: &mut ChildSpawnerCommands,
    key: TreeKey,
    assets: &MenuAssets,
    justify: Justify,
) {
    for (line, hint) in nav::hint(key.layout, key.in_game).into_iter().enumerate() {
        column.spawn((
            text(hint, &assets.medium, size::HINT, palette::NEIGE),
            TextLayout::justify(justify),
            TextShadow {
                offset: Vec2::new(0.0, 2.0),
                color: palette::OMBRE,
            },
            Node {
                margin: UiRect::top(u(if line == 0 { 6.0 } else { 0.0 })),
                ..default()
            },
        ));
    }
}

/// The menu opened from the game, the pause (issue #100): in the middle, over the veil, its title
/// « Pause » (online, « Pas de pause ! »: the game goes on).
fn spawn_game_menu(commands: &mut Commands, key: TreeKey, assets: &MenuAssets) {
    commands
        .spawn((
            Name::new("Main menu"),
            MainMenuRoot { key },
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(palette::VOILE.with_alpha(0.0)),
        ))
        .with_children(|root| {
            root.spawn((
                MenuColumn,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: u(10.0),
                    ..default()
                },
                UiTransform::from_scale(Vec2::splat(anim::POP_FROM)),
                Pickable::IGNORE,
            ))
            .with_children(|column| {
                column.spawn((
                    text(
                        nav::game_title(key.online),
                        &assets.semibold,
                        size::TITLE,
                        palette::NEIGE,
                    ),
                    TextShadow {
                        offset: Vec2::new(0.0, 5.0),
                        color: palette::FRAISE,
                    },
                ));
                spawn_panel(column, key, assets);
                spawn_hints(column, key, assets, Justify::Center);
            });
        });
}

/// The home's veil: full behind the column, fading out toward the right (`opacity`: a share of
/// its own, for its fade in).
pub fn home_veil(opacity: f32) -> BackgroundGradient {
    let color = palette::VOILE.with_alpha(palette::VOILE.alpha() * opacity.clamp(0.0, 1.0));
    BackgroundGradient::from(LinearGradient::to_right(vec![
        ColorStop::new(color, Val::ZERO),
        ColorStop::new(color, u(size::HOME_VEIL_FULL)),
        ColorStop::new(color.with_alpha(0.0), u(size::HOME_VEIL_END)),
    ]))
}

/// Issue #89: the home screen: on the left, the title big and alive, its phrase, the panel and
/// the keys; the veil fading out toward the right, where its background races.
fn spawn_home(commands: &mut Commands, key: TreeKey, assets: &MenuAssets) {
    commands
        .spawn((
            Name::new("Home"),
            MainMenuRoot { key },
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                padding: UiRect::left(u(size::HOME_MARGIN)),
                ..default()
            },
            home_veil(0.0),
        ))
        .with_children(|root| {
            root.spawn((
                MenuColumn,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexStart,
                    row_gap: u(10.0),
                    ..default()
                },
                UiTransform::from_scale(Vec2::splat(anim::POP_FROM)),
                Pickable::IGNORE,
            ))
            .with_children(|column| {
                column
                    .spawn((
                        MenuNode,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexStart,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|title| {
                        for (word, (name, indent)) in
                            [("Fluffy", 0.0), ("Racers", 36.0)].into_iter().enumerate()
                        {
                            title.spawn((
                                TitleWord(word),
                                text(name, &assets.semibold, size::HOME_TITLE, palette::NEIGE),
                                LineHeight::RelativeToFont(size::HOME_TITLE_LINE),
                                TextShadow {
                                    offset: Vec2::new(0.0, 7.0),
                                    color: palette::FRAISE,
                                },
                                Node {
                                    margin: UiRect::left(u(indent)),
                                    ..default()
                                },
                                UiTransform::IDENTITY,
                            ));
                        }
                    });
                column.spawn((
                    text(
                        "Des karts tout doux, des virages tout fous.",
                        &assets.medium,
                        size::TAGLINE,
                        palette::NEIGE,
                    ),
                    TextShadow {
                        offset: Vec2::new(0.0, 2.0),
                        color: palette::OMBRE,
                    },
                    Node {
                        margin: UiRect::bottom(u(6.0)),
                        ..default()
                    },
                ));
                spawn_panel(column, key, assets);
                spawn_hints(column, key, assets, Justify::Left);
            });
        });
}

fn spawn_button(
    panel: &mut ChildSpawnerCommands,
    index: usize,
    entry: Entry,
    online: bool,
    assets: &MenuAssets,
) {
    let usable = nav::enabled(entry, online);
    let state = if usable {
        ButtonState::Normal
    } else {
        ButtonState::Disabled
    };
    let look = style::button_look(state, accent(entry));
    let caption = if !usable {
        entry.why_disabled()
    } else if online {
        entry.caption_online()
    } else {
        entry.caption()
    };
    panel
        .spawn((
            Name::new(entry.label()),
            MenuButton { index, entry },
            Node {
                width: percent(100),
                height: u(size::BUTTON_HEIGHT),
                align_items: AlignItems::Center,
                column_gap: u(size::BUTTON_GAP),
                padding: UiRect {
                    left: u(size::BUTTON_PADDING_LEFT),
                    right: u(size::BUTTON_PADDING_RIGHT),
                    ..default()
                },
                border: UiRect::bottom(u(look.depth_px)),
                border_radius: BorderRadius::all(u(size::BUTTON_RADIUS)),
                ..default()
            },
            BackgroundColor(look.fill),
            BorderColor::all(look.depth),
            UiTransform::from_scale(Vec2::splat(anim::BUTTON_POP_FROM)),
        ))
        .with_children(|button| {
            button
                .spawn((
                    MenuBadge { index },
                    Node {
                        width: u(size::BADGE),
                        height: u(size::BADGE),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(look.badge),
                    Pickable::IGNORE,
                ))
                .with_children(|badge| {
                    badge.spawn((
                        MenuNode,
                        ImageNode::new(assets.icon(entry)),
                        Node {
                            width: u(size::ICON),
                            height: u(size::ICON),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
            button
                .spawn((
                    MenuNode,
                    Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|lines| {
                    lines.spawn((
                        text(entry.label(), &assets.semibold, size::LABEL, look.label),
                        MenuText {
                            index,
                            caption: false,
                        },
                    ));
                    lines.spawn((
                        text(caption, &assets.medium, size::CAPTION, look.caption),
                        MenuText {
                            index,
                            caption: true,
                        },
                    ));
                });
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<MainMenuAction>| {
                actions.write(MainMenuAction::Hover(index));
            },
        )
        .observe(
            move |press: On<PointerPress>, mut actions: MessageWriter<MainMenuAction>| {
                if press.button == PointerButton::Primary {
                    actions.write(MainMenuAction::Press(index));
                }
            },
        )
        .observe(
            move |release: On<PointerRelease>, mut actions: MessageWriter<MainMenuAction>| {
                if release.button == PointerButton::Primary {
                    actions.write(MainMenuAction::Release(index));
                }
            },
        )
        .observe(
            |_: On<PointerOut>, mut actions: MessageWriter<MainMenuAction>| {
                actions.write(MainMenuAction::Unpress);
            },
        );
}

/// The state of the button of the `index`-th entry.
pub fn state_of(menu: &MainMenu, index: usize, entry: Entry, online: bool) -> ButtonState {
    if !nav::enabled(entry, online) {
        ButtonState::Disabled
    } else if menu.pressed == Some(index) {
        ButtonState::Pressed
    } else if menu.focus == index {
        ButtonState::Focused
    } else {
        ButtonState::Normal
    }
}

/// Where a button is drawn at `now` (s, real time): its scale (the pop-in, the focus, the squash
/// and stretch) and how far down it goes (pressed, and squashed onto its bottom).
pub fn button_pose(menu: &MainMenu, index: usize, now: f64) -> UiTransform {
    let since = |at: f64| (now - at) as f32;
    let pop = anim::button_pop(since(menu.opened_at), index);
    let focus = if menu.focus == index {
        anim::focus_scale(true, since(menu.focused_at))
    } else if menu.unfocused == Some(index) {
        anim::focus_scale(false, since(menu.focused_at))
    } else {
        1.0
    };
    let height = if menu.pressed == Some(index) {
        anim::press_height(since(menu.pressed_at))
    } else if menu.released == Some(index) {
        anim::release_height(since(menu.released_at), menu.released_from)
    } else {
        1.0
    };
    let scale = Vec2::splat(pop * focus) * anim::squash_and_stretch(height);
    let mut down = (1.0 - height) * size::BUTTON_HEIGHT / 2.0;
    if menu.pressed == Some(index) {
        down += size::BUTTON_DEPTH - size::BUTTON_DEPTH_PRESSED;
    }
    UiTransform {
        translation: Val2::new(Val::ZERO, u(down)),
        scale,
        rotation: Rot2::IDENTITY,
    }
}

/// The menu open: the veil fades in, the column pops in, each button takes its state's look and
/// its pose; on the home screen, the title's words bob. Only what changed is written.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn animate_tree(
    menu: Res<MainMenu>,
    time: Res<Time<Real>>,
    online: Option<Res<Online>>,
    (mut veils, mut home_veils): (
        Query<&mut BackgroundColor, With<MainMenuRoot>>,
        Query<&mut BackgroundGradient, With<MainMenuRoot>>,
    ),
    mut words: Query<(&TitleWord, &mut UiTransform), (Without<MenuColumn>, Without<MenuButton>)>,
    mut columns: Query<&mut UiTransform, (With<MenuColumn>, Without<MenuButton>)>,
    mut buttons: Query<
        (
            &MenuButton,
            &mut UiTransform,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Node,
        ),
        (Without<MainMenuRoot>, Without<MenuColumn>),
    >,
    mut badges: Query<
        (&MenuBadge, &mut BackgroundColor),
        (Without<MainMenuRoot>, Without<MenuButton>),
    >,
    mut texts: Query<(&MenuText, &mut TextColor)>,
) {
    let now = super::now(&time);
    let online = online.is_some();
    let opened = (now - menu.opened_at) as f32;
    for mut veil in &mut veils {
        veil.set_if_neq(BackgroundColor(
            palette::VOILE.with_alpha(palette::VOILE.alpha() * anim::veil(opened)),
        ));
    }
    // The home's veil: its stops' colors, in place (no new gradient each frame).
    let alpha = palette::VOILE.alpha() * anim::veil(opened);
    for mut veil in &mut home_veils {
        let stale = veil.0.iter().any(|gradient| match gradient {
            Gradient::Linear(linear) => linear
                .stops
                .first()
                .is_some_and(|stop| (stop.color.alpha() - alpha).abs() > 1e-4),
            _ => false,
        });
        if !stale {
            continue;
        }
        for gradient in veil.0.iter_mut() {
            if let Gradient::Linear(linear) = gradient {
                let last = linear.stops.len().saturating_sub(1);
                for (i, stop) in linear.stops.iter_mut().enumerate() {
                    stop.color = stop.color.with_alpha(if i == last { 0.0 } else { alpha });
                }
            }
        }
    }
    for (word, mut transform) in &mut words {
        let (down, turn) = anim::title_word(opened, word.0);
        transform.set_if_neq(UiTransform {
            translation: Val2::new(Val::ZERO, u(down)),
            scale: Vec2::ONE,
            rotation: Rot2::radians(turn),
        });
    }
    for mut transform in &mut columns {
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(anim::panel_pop(
            opened,
        ))));
    }
    let entries = menu.entries();
    let look_of = |index: usize| {
        let entry = entries[index.min(entries.len() - 1)];
        style::button_look(state_of(&menu, index, entry, online), accent(entry))
    };
    for (button, mut transform, mut fill, mut depth, mut node) in &mut buttons {
        let look = look_of(button.index);
        transform.set_if_neq(button_pose(&menu, button.index, now));
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
        let bottom = u(look.depth_px);
        if node.border.bottom != bottom {
            node.border.bottom = bottom;
        }
    }
    for (badge, mut fill) in &mut badges {
        fill.set_if_neq(BackgroundColor(look_of(badge.index).badge));
    }
    for (text, mut color) in &mut texts {
        let look = look_of(text.index);
        color.set_if_neq(TextColor(if text.caption {
            look.caption
        } else {
            look.label
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pose of a button: popping in, then still at rest; grown with the focus; squashed and
    /// lower while pressed; stretched just after its release, then still.
    #[test]
    fn a_button_pops_grows_squashes_and_stretches() {
        let menu = MainMenu {
            open: true,
            focus: 1,
            opened_at: 10.0,
            focused_at: 10.0,
            ..default()
        };
        let start = button_pose(&menu, 0, 10.0);
        assert_eq!(start.scale, Vec2::splat(anim::BUTTON_POP_FROM));
        let rest = button_pose(&menu, 0, 12.0);
        assert_eq!(rest.scale, Vec2::ONE);
        assert_eq!(rest.translation, Val2::new(Val::ZERO, u(0.0)));
        let focused = button_pose(&menu, 1, 12.0);
        assert!(focused.scale.x > 1.03 && focused.scale.x == focused.scale.y);

        let pressed = MainMenu {
            pressed: Some(1),
            pressed_at: 12.0,
            ..menu
        };
        let squashed = button_pose(&pressed, 1, 12.2);
        assert!(squashed.scale.y < focused.scale.y * 0.9, "{squashed:?}");
        assert!(squashed.scale.x > squashed.scale.y);
        let Val::Vh(down) = squashed.translation.y else {
            panic!("{squashed:?}")
        };
        assert!(down > 0.0, "it goes down");

        let released = MainMenu {
            released: Some(1),
            released_at: 12.2,
            released_from: anim::SQUASH,
            ..menu
        };
        let stretched = button_pose(&released, 1, 12.2 + 0.15);
        assert!(stretched.scale.y > stretched.scale.x, "{stretched:?}");
        assert_eq!(
            button_pose(&released, 1, 14.0),
            button_pose(&menu, 1, 14.0),
            "then still"
        );
    }

    /// The home's veil: full behind the column, nothing on the right; its fade in only changes
    /// its strength.
    #[test]
    fn the_home_veil_fades_out_toward_the_right() {
        let BackgroundGradient(gradients) = home_veil(1.0);
        let Some(Gradient::Linear(linear)) = gradients.first() else {
            panic!("{gradients:?}")
        };
        assert_eq!(linear.angle, LinearGradient::TO_RIGHT);
        let alphas: Vec<f32> = linear.stops.iter().map(|s| s.color.alpha()).collect();
        assert_eq!(
            alphas,
            [palette::VOILE.alpha(), palette::VOILE.alpha(), 0.0]
        );
        assert_eq!(linear.stops[1].point, u(size::HOME_VEIL_FULL));
        let BackgroundGradient(faded) = home_veil(0.0);
        let Some(Gradient::Linear(faded)) = faded.first() else {
            panic!()
        };
        assert!(faded.stops.iter().all(|s| s.color.alpha() == 0.0));
    }

    #[test]
    fn every_entry_has_its_own_color_and_icon() {
        let entries = nav::GAME_ENTRIES;
        for (i, a) in entries.iter().enumerate() {
            for b in &entries[i + 1..] {
                assert_ne!(accent(*a), accent(*b));
                assert_ne!(icon_index(*a), icon_index(*b));
            }
        }
        let menu = MainMenu {
            focus: 2,
            pressed: Some(3),
            ..default()
        };
        assert_eq!(
            state_of(&menu, 0, Entry::Resume, false),
            ButtonState::Normal
        );
        assert_eq!(
            state_of(&menu, 2, Entry::Online, false),
            ButtonState::Focused
        );
        assert_eq!(
            state_of(&menu, 3, Entry::Options, false),
            ButtonState::Pressed
        );
        assert_eq!(state_of(&menu, 1, Entry::Solo, true), ButtonState::Disabled);
        assert_eq!(state_of(&menu, 1, Entry::Solo, false), ButtonState::Normal);
    }
}
