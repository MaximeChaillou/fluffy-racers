//! Under water (GD § 10, issue #20): while the chase camera is under the water of a lake, a
//! translucent blue covers the screen (under the HUD). One UI node for the whole game, made the
//! first time the camera goes under water, then shown or hidden: nothing is drawn above the water.

use bevy::prelude::*;

use super::ChaseCamera;
use crate::track::{Lake, water_depth};

/// The blue over the screen under water.
#[derive(Component, Clone, Copy, Debug)]
pub struct UnderwaterTint;

/// Its color: the lakes' blue, see-through.
pub const UNDERWATER_TINT: Color = Color::srgba(0.08, 0.38, 0.62, 0.38);

fn spawn_tint(commands: &mut Commands) {
    commands.spawn((
        Name::new("Underwater tint"),
        UnderwaterTint,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(UNDERWATER_TINT),
        // Under the HUD and the menus.
        GlobalZIndex(-1),
        Visibility::Inherited,
    ));
}

/// Shows the tint while the camera is under the water (made the first time).
pub(super) fn tint_under_water(
    mut commands: Commands,
    cameras: Query<&Transform, With<ChaseCamera>>,
    lakes: Query<&Lake>,
    mut tints: Query<&mut Visibility, With<UnderwaterTint>>,
) {
    let under = cameras.iter().any(|camera| {
        water_depth(lakes.iter(), camera.translation).is_some_and(|depth| depth > 0.0)
    });
    if under && tints.is_empty() {
        spawn_tint(&mut commands);
    }
    for mut visibility in &mut tints {
        visibility.set_if_neq(if under {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
