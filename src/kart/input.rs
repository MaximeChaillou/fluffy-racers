//! Local player input: keyboard and gamepad → `KartInput` (GD § 13), plus debug keys.
//!
//! Keys are physical positions (`KeyCode`): W/A/S/D is Z/Q/S/D on an AZERTY keyboard, the same
//! keys under the fingers. The player's keyboard layout ([`KeyboardLayout`], issue #17, chosen in
//! the options menu) names them as printed on the keyboard, and in AZERTY also reads the Z and Q
//! positions: the keys a browser reports when it gives letters instead of positions.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Boost, KartInput, KartState, KartSystems};
use crate::interpolation::PhysicsPose;
use bevy_rapier3d::prelude::Velocity;

/// Kart driven by the local keyboard / gamepad.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PlayerKart;

/// Where the kart goes back with the debug reset key, or when it falls off the test ground.
/// Only karts outside a race have one: in a race, the tow pelican handles falls and the reset key
/// (`crate::race`, issue #5).
#[derive(Component, Clone, Copy, Debug)]
pub struct SpawnPoint(pub Transform);

/// Debug key: boost the player kart, to test the generic `Boost` (chili-like: +30 %, 1.2 s).
pub const DEBUG_BOOST_KEY: KeyCode = KeyCode::KeyB;
pub const DEBUG_BOOST_STRENGTH: f32 = 0.3;
pub const DEBUG_BOOST_DURATION: f32 = 1.2;
/// Debug key: put the player kart back on its spawn point. In a race (no `SpawnPoint`), the
/// same key calls the tow pelican instead (`crate::race`).
pub const DEBUG_RESET_KEY: KeyCode = KeyCode::KeyR;
/// Below this height the kart fell off the test ground: back to the spawn point. Safety net of
/// the test ground only (karts with a `SpawnPoint`); on a circuit the tow pelican (GD § 2,
/// issue #5) rescues racers.
pub const FALL_RESET_HEIGHT: f32 = -30.0;

/// Stick dead zone: smaller deflections are ignored.
const STICK_DEAD_ZONE: f32 = 0.15;

/// The player's keyboard layout (GD § 13, issue #17): which letters the driving keys carry, and
/// which keys are read.
///
/// - Both read the **physical positions** W/A/S/D (`KeyCode` is a position, not a letter): on an
///   AZERTY keyboard they are the keys printed Z/Q/S/D. Same keys under the fingers, whatever the
///   choice.
/// - **AZERTY** also reads the positions Z (accelerator, up) and Q (left). A browser that reports
///   letters instead of positions (Firefox resisting fingerprinting, some remote desktops and
///   virtual machines) gives those for the keys printed Z and Q. On a real AZERTY keyboard they
///   are the keys printed W and A, which the game does not use: no conflict.
/// - **QWERTY** leaves Z (next to X, the Interaction key) and Q alone.
/// - The labels shown (options menu, selection screen) are the letters printed on the keyboard:
///   Z/Q/S/D or W/A/S/D.
///
/// Every other key (Espace, Shift, E, C, X, F, G, P, O...) carries the same letter at the same
/// place in both layouts.
#[derive(Resource, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeyboardLayout {
    /// The French layout, the game's language: the default.
    #[default]
    Azerty,
    Qwerty,
}

impl KeyboardLayout {
    pub const ALL: [Self; 2] = [KeyboardLayout::Azerty, KeyboardLayout::Qwerty];

    pub fn label(self) -> &'static str {
        match self {
            KeyboardLayout::Azerty => "AZERTY",
            KeyboardLayout::Qwerty => "QWERTY",
        }
    }

    /// Accelerator, and up in the menus.
    pub fn up_keys(self) -> &'static [KeyCode] {
        match self {
            KeyboardLayout::Azerty => &[KeyCode::ArrowUp, KeyCode::KeyW, KeyCode::KeyZ],
            KeyboardLayout::Qwerty => &[KeyCode::ArrowUp, KeyCode::KeyW],
        }
    }

    /// Brake / reverse, and down in the menus.
    pub fn down_keys(self) -> &'static [KeyCode] {
        &[KeyCode::ArrowDown, KeyCode::KeyS]
    }

    pub fn left_keys(self) -> &'static [KeyCode] {
        match self {
            KeyboardLayout::Azerty => &[KeyCode::ArrowLeft, KeyCode::KeyA, KeyCode::KeyQ],
            KeyboardLayout::Qwerty => &[KeyCode::ArrowLeft, KeyCode::KeyA],
        }
    }

    pub fn right_keys(self) -> &'static [KeyCode] {
        &[KeyCode::ArrowRight, KeyCode::KeyD]
    }

    /// The letters printed on the up, left, down and right keys: Z/Q/S/D or W/A/S/D.
    pub fn letters(self) -> [&'static str; 4] {
        match self {
            KeyboardLayout::Azerty => ["Z", "Q", "S", "D"],
            KeyboardLayout::Qwerty => ["W", "A", "S", "D"],
        }
    }
}

/// The other driving keys: the same letter at the same place in AZERTY and QWERTY.
pub const JUMP_KEYS: [KeyCode; 1] = [KeyCode::Space];
pub const ITEM_KEYS: [KeyCode; 3] = [KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::KeyE];
pub const LOOK_BACK_KEYS: [KeyCode; 1] = [KeyCode::KeyC];

/// Reads the keyboard and every connected gamepad for the player kart. Not part of the
/// headless simulation: bots and network peers produce `KartInput` their own way.
pub struct KartInputPlugin;

impl Plugin for KartInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, read_player_input.in_set(KartSystems::Input))
            .add_systems(Update, debug_keys);
    }
}

/// Keyboard state → kart input, with the keys of `layout`.
pub fn keyboard_input(keys: &ButtonInput<KeyCode>, layout: KeyboardLayout) -> KartInput {
    let axis = |on: bool| if on { 1.0 } else { 0.0 };
    let held = |set: &[KeyCode]| keys.any_pressed(set.iter().copied());
    KartInput {
        throttle: axis(held(layout.up_keys())),
        brake: axis(held(layout.down_keys())),
        steer: axis(held(layout.right_keys())) - axis(held(layout.left_keys())),
        jump_drift: keys.any_pressed(JUMP_KEYS),
        item: keys.any_pressed(ITEM_KEYS),
        look_back: keys.any_pressed(LOOK_BACK_KEYS),
    }
}

/// Gamepad state → kart input (GD § 13: A accelerates, B brakes, RB/RT jump, LB/LT item,
/// X/Y look behind).
pub fn gamepad_input(gamepad: &Gamepad) -> KartInput {
    let button = |b: GamepadButton| if gamepad.pressed(b) { 1.0 } else { 0.0 };
    let stick = gamepad.left_stick().x;
    let steer = if stick.abs() < STICK_DEAD_ZONE {
        0.0
    } else {
        (stick - STICK_DEAD_ZONE * stick.signum()) / (1.0 - STICK_DEAD_ZONE)
    };
    KartInput {
        throttle: button(GamepadButton::South),
        brake: button(GamepadButton::East),
        steer,
        jump_drift: gamepad
            .any_pressed([GamepadButton::RightTrigger, GamepadButton::RightTrigger2]),
        item: gamepad.any_pressed([GamepadButton::LeftTrigger, GamepadButton::LeftTrigger2]),
        look_back: gamepad.any_pressed([GamepadButton::West, GamepadButton::North]),
    }
}

/// Merges two input sources: the strongest action wins.
pub fn combine(a: KartInput, b: KartInput) -> KartInput {
    KartInput {
        throttle: a.throttle.max(b.throttle),
        brake: a.brake.max(b.brake),
        steer: (a.steer + b.steer).clamp(-1.0, 1.0),
        jump_drift: a.jump_drift || b.jump_drift,
        item: a.item || b.item,
        look_back: a.look_back || b.look_back,
    }
}

fn read_player_input(
    keys: Res<ButtonInput<KeyCode>>,
    layout: Option<Res<KeyboardLayout>>,
    gamepads: Query<&Gamepad>,
    mut karts: Query<&mut KartInput, With<PlayerKart>>,
) {
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let mut input = keyboard_input(&keys, layout);
    for gamepad in &gamepads {
        input = combine(input, gamepad_input(gamepad));
    }
    for mut kart_input in &mut karts {
        kart_input.set_if_neq(input);
    }
}

fn debug_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut karts: Query<
        (
            &mut Boost,
            &mut KartState,
            &mut Transform,
            &mut Velocity,
            &mut PhysicsPose,
            Option<&SpawnPoint>,
        ),
        With<PlayerKart>,
    >,
) {
    for (mut boost, mut state, mut transform, mut velocity, mut pose, spawn) in &mut karts {
        if keys.just_pressed(DEBUG_BOOST_KEY) {
            boost.add(DEBUG_BOOST_STRENGTH, DEBUG_BOOST_DURATION);
        }
        let fell = transform.translation.y < FALL_RESET_HEIGHT;
        if (keys.just_pressed(DEBUG_RESET_KEY) || fell)
            && let Some(SpawnPoint(spawn)) = spawn
        {
            *transform = *spawn;
            *velocity = Velocity::zero();
            *boost = Boost::default();
            *state = KartState::default();
            pose.snap(*spawn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_maps_arrows_and_letters() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::ArrowLeft);
        keys.press(KeyCode::Space);
        let input = keyboard_input(&keys, KeyboardLayout::Qwerty);
        assert_eq!(input.throttle, 1.0);
        assert_eq!(input.brake, 0.0);
        assert_eq!(input.steer, -1.0);
        assert!(input.jump_drift);
        assert!(!input.item);

        keys.press(KeyCode::KeyD);
        assert_eq!(
            keyboard_input(&keys, KeyboardLayout::Qwerty).steer,
            0.0,
            "left + right cancel out"
        );
    }

    /// Issue #17: both layouts read the physical positions W/A/S/D (printed Z/Q/S/D on an AZERTY
    /// keyboard); AZERTY also reads the positions Z and Q (the letters a browser may report
    /// instead of positions), QWERTY does not.
    #[test]
    fn both_layouts_drive_with_the_same_keys_and_azerty_also_reads_z_and_q() {
        let press = |codes: &[KeyCode]| {
            let mut keys = ButtonInput::<KeyCode>::default();
            for code in codes {
                keys.press(*code);
            }
            keys
        };
        for layout in KeyboardLayout::ALL {
            // The physical W/A/S/D (Z/Q/S/D under an AZERTY player's fingers).
            assert_eq!(
                keyboard_input(&press(&[KeyCode::KeyW]), layout).throttle,
                1.0
            );
            assert_eq!(keyboard_input(&press(&[KeyCode::KeyS]), layout).brake, 1.0);
            assert_eq!(keyboard_input(&press(&[KeyCode::KeyA]), layout).steer, -1.0);
            assert_eq!(keyboard_input(&press(&[KeyCode::KeyD]), layout).steer, 1.0);
            // The arrows, Espace, E, Shift and C, whatever the layout.
            let all = keyboard_input(
                &press(&[
                    KeyCode::ArrowUp,
                    KeyCode::ArrowRight,
                    KeyCode::Space,
                    KeyCode::KeyE,
                    KeyCode::KeyC,
                ]),
                layout,
            );
            assert_eq!((all.throttle, all.steer), (1.0, 1.0));
            assert!(all.jump_drift && all.item && all.look_back);
            assert!(keyboard_input(&press(&[KeyCode::ShiftLeft]), layout).item);
        }
        let azerty = KeyboardLayout::Azerty;
        assert_eq!(
            keyboard_input(&press(&[KeyCode::KeyZ]), azerty).throttle,
            1.0
        );
        assert_eq!(keyboard_input(&press(&[KeyCode::KeyQ]), azerty).steer, -1.0);
        let qwerty = KeyboardLayout::Qwerty;
        assert_eq!(
            keyboard_input(&press(&[KeyCode::KeyZ, KeyCode::KeyQ]), qwerty),
            KartInput::default()
        );
        assert_eq!(KeyboardLayout::default(), azerty);
        assert_eq!(azerty.letters(), ["Z", "Q", "S", "D"]);
        assert_eq!(qwerty.letters(), ["W", "A", "S", "D"]);
    }

    #[test]
    fn combine_keeps_the_strongest_action() {
        let keyboard = KartInput {
            throttle: 1.0,
            steer: 1.0,
            ..default()
        };
        let pad = KartInput {
            brake: 1.0,
            steer: 0.5,
            item: true,
            ..default()
        };
        let both = combine(keyboard, pad);
        assert_eq!(both.throttle, 1.0);
        assert_eq!(both.brake, 1.0);
        assert_eq!(both.steer, 1.0);
        assert!(both.item);
    }
}
