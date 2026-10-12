//! Local player input: keyboard and gamepad → `KartInput` (GD § 13), plus debug keys.

use bevy::prelude::*;

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

/// Keys are physical positions (`KeyCode`), so W/A/S/D is also Z/Q/S/D on an AZERTY keyboard.
const THROTTLE_KEYS: [KeyCode; 2] = [KeyCode::ArrowUp, KeyCode::KeyW];
const BRAKE_KEYS: [KeyCode; 2] = [KeyCode::ArrowDown, KeyCode::KeyS];
const LEFT_KEYS: [KeyCode; 2] = [KeyCode::ArrowLeft, KeyCode::KeyA];
const RIGHT_KEYS: [KeyCode; 2] = [KeyCode::ArrowRight, KeyCode::KeyD];
const JUMP_KEYS: [KeyCode; 1] = [KeyCode::Space];
const ITEM_KEYS: [KeyCode; 3] = [KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::KeyE];
const LOOK_BACK_KEYS: [KeyCode; 1] = [KeyCode::KeyC];

/// Reads the keyboard and every connected gamepad for the player kart. Not part of the
/// headless simulation: bots and network peers produce `KartInput` their own way.
pub struct KartInputPlugin;

impl Plugin for KartInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, read_player_input.in_set(KartSystems::Input))
            .add_systems(Update, debug_keys);
    }
}

/// Keyboard state → kart input.
pub fn keyboard_input(keys: &ButtonInput<KeyCode>) -> KartInput {
    let axis = |on: bool| if on { 1.0 } else { 0.0 };
    KartInput {
        throttle: axis(keys.any_pressed(THROTTLE_KEYS)),
        brake: axis(keys.any_pressed(BRAKE_KEYS)),
        steer: axis(keys.any_pressed(RIGHT_KEYS)) - axis(keys.any_pressed(LEFT_KEYS)),
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
    gamepads: Query<&Gamepad>,
    mut karts: Query<&mut KartInput, With<PlayerKart>>,
) {
    let mut input = keyboard_input(&keys);
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
        let input = keyboard_input(&keys);
        assert_eq!(input.throttle, 1.0);
        assert_eq!(input.brake, 0.0);
        assert_eq!(input.steer, -1.0);
        assert!(input.jump_drift);
        assert!(!input.item);

        keys.press(KeyCode::KeyD);
        assert_eq!(keyboard_input(&keys).steer, 0.0, "left + right cancel out");
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
