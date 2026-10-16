//! Issue #80: the other players' names over their karts, online (« Noms au-dessus des karts » in
//! the options menu, on by default).
//!
//! A small tag per other player's kart (never a bot's, never ours): its pseudo in the HUD's font
//! (accents dropped, `ui::format::ascii_letter`), white on a dark pill edged with its kart's
//! color. One tag (three UI nodes) per player's kart, spawned when the kart is known and despawned
//! with it: never an entity per frame. Each frame only moves it (`UiTransform`: no new layout)
//! over its kart's visual, and hides it behind the camera, beyond
//! [`NetTuning::name_tag_distance`], for a player who left, or with the option off. Under the HUD
//! (the results screen covers them). Solo there is no `KartSync`: nothing runs. Only with a
//! renderer ([`NetPlugin`](super::NetPlugin)); the tests add [`NameTagPlugin`] themselves.

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::ui::UiSystems;

use super::{KartId, KartSync, NetTuning, RemoteKart};
use crate::camera::ChaseCamera;
use crate::kart::KartColor;
use crate::options::PlayerOptions;
use crate::ui::format::ascii_letter;

/// A tag's box: wide enough for any pseudo (`pseudo::PSEUDO_MAX_CHARS`) and for a character's
/// name, the pill centered in it (px). Its bottom edge sits on the point over the kart.
const TAG_WIDTH: f32 = 240.0;
const TAG_HEIGHT: f32 = 26.0;
const TAG_FONT_SIZE: f32 = 16.0;
const TAG_FILL: Color = Color::srgba(0.07, 0.05, 0.15, 0.72);

pub struct NameTagPlugin;

impl Plugin for NameTagPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            // After the karts' visuals and the camera moved (and their global transforms), before
            // the UI's layout places the tags: they follow their kart in the same frame.
            follow_name_tags
                .after(TransformSystems::Propagate)
                .before(UiSystems::Layout)
                .run_if(resource_exists::<KartSync>.or_else(any_with_component::<NameTag>)),
        );
    }
}

/// The tag of a remote player's kart: what it shows now, and its nodes.
#[derive(Component, Clone, Debug)]
pub struct NameTag {
    pub kart: KartId,
    /// The pseudo it writes (as the lobby has it: the text is its HUD letters).
    pub name: String,
    pub color: KartColor,
    pill: Entity,
    text: Entity,
}

impl NameTag {
    /// The text node that writes its name.
    pub fn text(&self) -> Entity {
        self.text
    }
}

/// The name a tag writes: its HUD letters, into a reused string.
pub fn write_tag_text(out: &mut String, name: &str) {
    out.clear();
    out.extend(name.chars().map(ascii_letter));
}

/// Whether a tag shows: the option on, its kart in front of the camera (`depth` > 0, m) and no
/// further than `max_distance` (m).
pub fn tag_shown(enabled: bool, depth: f32, distance: f32, max_distance: f32) -> bool {
    enabled && depth > 0.0 && distance <= max_distance
}

/// Where a tag goes (its top-left corner, px) for the point `over` of its kart on screen: its
/// pill centered, its bottom edge on that point.
pub fn tag_corner(over: Vec2) -> Vec2 {
    over - Vec2::new(TAG_WIDTH / 2.0, TAG_HEIGHT)
}

fn border_of(color: KartColor) -> BorderColor {
    let [r, g, b] = color.rgb();
    BorderColor::all(Color::srgb_u8(r, g, b))
}

/// The remote player's kart a tag is for, if it still has a name.
fn named(sync: &KartSync, kart: KartId) -> Option<&RemoteKart> {
    sync.remote_kart(kart).filter(|remote| has_name(remote))
}

/// A player's kart with a name to write (a bot has none).
fn has_name(remote: &RemoteKart) -> bool {
    remote.name.as_deref().is_some_and(|name| !name.is_empty())
}

#[allow(clippy::too_many_arguments)]
fn follow_name_tags(
    mut commands: Commands,
    sync: Option<Res<KartSync>>,
    options: Option<Res<PlayerOptions>>,
    tuning: Res<NetTuning>,
    cameras: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    visuals: Query<&GlobalTransform, Without<ChaseCamera>>,
    mut tags: Query<(Entity, &mut NameTag, &mut UiTransform, &mut Visibility)>,
    mut borders: Query<&mut BorderColor>,
    mut texts: Query<&mut Text>,
) {
    let Some(sync) = sync else {
        // The room left: its tags go.
        for (entity, ..) in &tags {
            commands.entity(entity).try_despawn();
        }
        return;
    };
    let enabled = options.is_none_or(|options| options.name_tags);
    let camera = cameras.iter().next();
    // The karts that have a tag (ids are seats or grid slots: fewer than 64).
    let mut tagged = 0u64;
    for (entity, mut tag, mut transform, mut visibility) in &mut tags {
        let Some(remote) = named(&sync, tag.kart) else {
            commands.entity(entity).try_despawn();
            continue;
        };
        tagged |= 1 << (tag.kart % 64);
        if let Some(name) = remote.name.as_deref()
            && tag.name != name
        {
            tag.name.clear();
            tag.name.push_str(name);
            if let Ok(mut text) = texts.get_mut(tag.text) {
                write_tag_text(&mut text.0, name);
            }
        }
        if tag.color != remote.color {
            tag.color = remote.color;
            if let Ok(mut border) = borders.get_mut(tag.pill) {
                *border = border_of(remote.color);
            }
        }
        let place = camera.zip(remote.visual.filter(|_| !remote.left)).and_then(
            |((camera, eye), visual)| {
                let over =
                    visuals.get(visual).ok()?.translation() + Vec3::Y * tuning.name_tag_height;
                let on_screen = camera.world_to_viewport_with_depth(eye, over).ok()?;
                let distance = eye.translation().distance(over);
                tag_shown(enabled, on_screen.z, distance, tuning.name_tag_distance)
                    .then(|| tag_corner(on_screen.truncate()))
            },
        );
        match place {
            Some(corner) => {
                let translation = Val2::px(corner.x, corner.y);
                if transform.translation != translation {
                    transform.translation = translation;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for remote in sync.remotes() {
        let Some(name) = remote.name.as_deref().filter(|_| has_name(remote)) else {
            continue;
        };
        if tagged & (1 << (remote.kart % 64)) != 0 {
            continue;
        }
        spawn_tag(&mut commands, remote.kart, name, remote.color);
    }
}

/// A new tag, hidden until its kart is placed (the next frame).
fn spawn_tag(commands: &mut Commands, kart: KartId, name: &str, color: KartColor) {
    let mut letters = String::new();
    write_tag_text(&mut letters, name);
    let text = commands
        .spawn((
            Text::new(letters),
            TextFont::from_font_size(TAG_FONT_SIZE),
            TextColor(Color::WHITE),
        ))
        .id();
    let pill = commands
        .spawn((
            Node {
                padding: UiRect::new(px(8), px(8), px(1), px(2)),
                border: UiRect::bottom(px(3)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(TAG_FILL),
            border_of(color),
        ))
        .add_child(text)
        .id();
    commands
        .spawn((
            Name::new(format!("Étiquette {kart}")),
            NameTag {
                kart,
                name: name.to_string(),
                color,
                pill,
                text,
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(TAG_WIDTH),
                height: px(TAG_HEIGHT),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::End,
                ..default()
            },
            UiTransform::default(),
            // Under the HUD and its results screen.
            GlobalZIndex(-1),
            Visibility::Hidden,
        ))
        .add_child(pill);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag writes the pseudo in the HUD's font: its accents dropped.
    #[test]
    fn a_tag_writes_the_pseudo_without_its_accents() {
        let mut out = String::from("old");
        write_tag_text(&mut out, "Éloïse ça va");
        assert_eq!(out, "Eloise ca va");
        write_tag_text(&mut out, "Max");
        assert_eq!(out, "Max");
    }

    /// Shown in front of the camera, close enough, with the option on; centered over its kart.
    #[test]
    fn a_tag_shows_over_its_kart_in_front_of_the_camera() {
        assert!(tag_shown(true, 12.0, 15.0, 80.0));
        assert!(!tag_shown(false, 12.0, 15.0, 80.0), "option off");
        assert!(!tag_shown(true, -3.0, 5.0, 80.0), "behind the camera");
        assert!(!tag_shown(true, 0.0, 5.0, 80.0));
        assert!(!tag_shown(true, 90.0, 95.0, 80.0), "too far");
        assert!(tag_shown(true, 79.0, 80.0, 80.0));
        let corner = tag_corner(Vec2::new(400.0, 300.0));
        assert_eq!(
            corner + Vec2::new(TAG_WIDTH / 2.0, TAG_HEIGHT),
            Vec2::new(400.0, 300.0)
        );
    }
}
