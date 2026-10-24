//! Issue #176: the wheels on screen and the standing start (pure, unit-tested; the systems are in
//! `systems.rs`, with a renderer only).
//!
//! - **The wheels turn** at the kart's speed (its speed along its heading over their radius,
//!   [`roll_rate`]), the right way, shown at most `WheelVfx::max_spin` fast: their band
//!   (`kart::parts::paint_band`) never turns more than half a turn in a frame, so it never seems
//!   to turn backward. **The front wheels steer** with the driver's stick ([`steer_angle`]; a kart
//!   driven elsewhere, online, shows its turn: [`steer_from_yaw`]).
//! - **The back wheels spin** on top of that ([`target_slip`]): on the spot while the karts are
//!   held on the grid with the accelerator down; less and less as a kart sets off; harder with a
//!   rocket start's boost. Their spin makes the tyres smoke ([`spin_share`]).
//! - **A standing start**: at GO a cloud by the start's quality ([`Start`], [`go_puffs`]); a
//!   smaller puff when a kart sets off again from a full stop ([`Standstill`]).

use bevy::prelude::*;

use super::tuning::WheelVfx;
use crate::kart::pivoted_wheel;

/// What the wheels see of a kart on a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelDrive {
    /// Its speed along its heading (m/s, forward: positive).
    pub speed: f32,
    /// Its driver's accelerator, 0 to 1 (as the driver pressed it: on the grid too).
    pub throttle: f32,
    /// Held on the grid (before GO): its wheels spin on the spot.
    pub held: bool,
    /// A rocket start's boost runs.
    pub rocket: bool,
    /// Nothing drives its wheels: its engine stalled, it spins out or tumbles, the pelican
    /// carries it.
    pub idle: bool,
}

/// How fast the back wheels' tyres go on top of the kart's speed (m/s): on the spot on the grid
/// (`grid_slip` at full accelerator), setting off (`launch_slip`, none from `slip_fade_speed`),
/// with a rocket start's boost (`rocket_slip`, none from `rocket_fade_speed`).
pub fn target_slip(tuning: &WheelVfx, drive: &WheelDrive) -> f32 {
    if drive.idle {
        return 0.0;
    }
    let throttle = drive.throttle.clamp(0.0, 1.0);
    if drive.held {
        return tuning.grid_slip.max(0.0) * throttle;
    }
    let fade = |until: f32| {
        if until > 0.0 {
            (1.0 - drive.speed.max(0.0) / until).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let launch = tuning.launch_slip.max(0.0) * throttle * fade(tuning.slip_fade_speed);
    let rocket = if drive.rocket {
        tuning.rocket_slip.max(0.0) * fade(tuning.rocket_fade_speed)
    } else {
        0.0
    };
    launch.max(rocket)
}

/// `value` moved toward `target` at `rate` per second (exponential), over `dt` s.
pub fn follow(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    if !value.is_finite() {
        return target;
    }
    value + (target - value) * (1.0 - (-rate.max(0.0) * dt.max(0.0)).exp())
}

/// How fast a wheel of `radius` turns (rad/s around the kart's X: negative rolls it forward)
/// when its tyre goes `surface` m/s (the kart's speed, and a back wheel's spin on top), at most
/// `max_spin`.
pub fn roll_rate(tuning: &WheelVfx, surface: f32, radius: f32) -> f32 {
    let most = tuning.max_spin.max(0.0);
    (-surface / radius.max(0.05)).clamp(-most, most)
}

/// The front wheels' angle (rad around the kart's up: positive to the left) for the stick
/// `steer` (-1 full left, 1 full right).
pub fn steer_angle(tuning: &WheelVfx, steer: f32) -> f32 {
    -steer.clamp(-1.0, 1.0) * tuning.steer_angle
}

/// The stick (-1 to 1) that shows a kart turning at `yaw_rate` (rad/s around its up, positive to
/// the left): full lock at `steer_yaw_rate`.
pub fn steer_from_yaw(tuning: &WheelVfx, yaw_rate: f32) -> f32 {
    if !yaw_rate.is_finite() {
        return 0.0;
    }
    (-yaw_rate / tuning.steer_yaw_rate.max(0.1)).clamp(-1.0, 1.0)
}

/// A wheel's rotation on its kart: on top of its `rest` (the mesh's turn, `kart::WheelVisual`),
/// rolled by `roll` around the kart's X, steered by `steer` around its up, then pivoted `pivot`
/// of the way flat in anti-gravity (`kart::pivoted_wheel`; `side`: the sign of its anchor's x).
pub fn wheel_rotation(rest: Quat, side: f32, pivot: f32, steer: f32, roll: f32) -> Quat {
    let turned = Quat::from_rotation_y(steer) * Quat::from_rotation_x(roll) * rest;
    if pivot > 0.0 {
        pivoted_wheel(turned, side, pivot)
    } else {
        turned
    }
}

/// How hard the tyres spin (their spin over `grid_slip`: 1 on the spot at full accelerator): the
/// share of their smoke, their dust and the kart's shakes.
pub fn spin_share(tuning: &WheelVfx, slip: f32) -> f32 {
    if tuning.grid_slip > 0.0 {
        (slip / tuning.grid_slip).clamp(0.0, 1.5)
    } else {
        0.0
    }
}

/// How a kart starts, seen at GO (GD § 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// A rocket start: its boost, this long (s).
    Rocket(f32),
    /// A normal start, the accelerator down.
    Normal,
    /// Pressed too early: the engine stalls.
    Stall,
}

/// How many puffs the cloud at GO has (`longest`: a perfect rocket start's boost, s): the most
/// for a perfect rocket start, fewer for a later one (down to `go_rocket_least` of them), then a
/// normal start's, then a stall's.
pub fn go_puffs(tuning: &WheelVfx, start: Start, longest: f32) -> u32 {
    match start {
        Start::Rocket(boost) => {
            let quality = if longest > 0.0 {
                (boost / longest).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let least = tuning.go_rocket_least.clamp(0.0, 1.0);
            let share = least + (1.0 - least) * quality;
            (tuning.go_rocket_puffs as f32 * share).round() as u32
        }
        Start::Normal => tuning.go_normal_puffs,
        Start::Stall => tuning.go_stall_puffs,
    }
}

/// How long a kart has been standing still (s), to see it set off again from a full stop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Standstill {
    pub still: f32,
}

impl Standstill {
    /// One frame at `speed` (m/s along its heading): whether the kart sets off now, after
    /// standing still for at least `stop_time` (slower than `stop_speed`). Backing away is not
    /// setting off.
    pub fn step(&mut self, tuning: &WheelVfx, speed: f32, dt: f32) -> bool {
        if speed.abs() < tuning.stop_speed {
            self.still += dt;
            return false;
        }
        let off = speed > 0.0 && self.still >= tuning.stop_time;
        self.still = 0.0;
        off
    }

    /// Forgets the stop (the start at GO has its own cloud).
    pub fn forget(&mut self) {
        self.still = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn drive(speed: f32, throttle: f32) -> WheelDrive {
        WheelDrive {
            speed,
            throttle,
            ..default()
        }
    }

    /// The wheels roll at the kart's speed, the right way (forward: the top of the wheel goes
    /// forward, toward -Z), never faster than `max_spin`; the front wheels steer with the stick
    /// (left: their front to the left), or with the yaw rate of a kart driven elsewhere.
    #[test]
    fn the_wheels_roll_the_right_way_and_steer() {
        let t = WheelVfx::default();
        let rate = roll_rate(&t, 6.0, 0.3);
        assert!((rate + 20.0).abs() < 1e-4, "{rate}");
        // Rolled forward a little, the top of the wheel has gone forward.
        let top = wheel_rotation(Quat::IDENTITY, -1.0, 0.0, 0.0, rate * DT) * Vec3::Y;
        assert!(top.z < 0.0, "{top}");
        assert_eq!(roll_rate(&t, 0.0, 0.3), 0.0);
        assert!(roll_rate(&t, -3.0, 0.3) > 0.0, "backward, the other way");
        assert_eq!(roll_rate(&t, 40.0, 0.2), -t.max_spin);
        // Never more than half a turn in a frame, down to 15 frames per second.
        assert!(t.max_spin / 15.0 < PI);
        // Left: the front of the wheel turns to the left (-X).
        let left = steer_angle(&t, -1.0);
        let front = wheel_rotation(Quat::IDENTITY, -1.0, 0.0, left, 0.0) * Vec3::NEG_Z;
        assert!(front.x < -0.3, "{front}");
        assert_eq!(steer_angle(&t, 0.0), 0.0);
        assert!((steer_angle(&t, 2.0) + t.steer_angle).abs() < 1e-6);
        // Turning left (a positive yaw rate) shows the stick to the left.
        assert!(steer_from_yaw(&t, 1.0) < 0.0);
        assert_eq!(steer_from_yaw(&t, 100.0), -1.0);
        assert_eq!(steer_from_yaw(&t, f32::NAN), 0.0);
        // Flat in anti-gravity: the rolled and steered wheel pivoted like `pivoted_wheel`.
        let rest = Quat::from_rotation_z(FRAC_PI_2);
        let flat = wheel_rotation(rest, 1.0, 1.0, 0.0, 0.0);
        assert!(flat.dot(pivoted_wheel(rest, 1.0, 1.0)).abs() > 1.0 - 1e-6);
    }

    /// The back wheels spin on the spot on the grid (with the accelerator), less and less as the
    /// kart sets off in the race, harder with a rocket start's boost; never when nothing drives
    /// them.
    #[test]
    fn the_back_wheels_spin_at_a_standing_start() {
        let t = WheelVfx::default();
        let held = |throttle| WheelDrive {
            held: true,
            ..drive(0.0, throttle)
        };
        assert_eq!(target_slip(&t, &held(1.0)), t.grid_slip);
        assert_eq!(target_slip(&t, &held(0.0)), 0.0);
        assert_eq!(spin_share(&t, target_slip(&t, &held(1.0))), 1.0);
        // Setting off: less than on the grid, less and less with the speed.
        let start = target_slip(&t, &drive(0.0, 1.0));
        let going = target_slip(&t, &drive(5.0, 1.0));
        assert!(start > going && going > 0.0 && start < t.grid_slip);
        assert_eq!(target_slip(&t, &drive(t.slip_fade_speed, 1.0)), 0.0);
        assert_eq!(target_slip(&t, &drive(0.0, 0.0)), 0.0);
        // A rocket start spins them harder, longer.
        let rocket = |speed| WheelDrive {
            rocket: true,
            ..drive(speed, 1.0)
        };
        assert!(target_slip(&t, &rocket(0.0)) > t.grid_slip);
        assert!(target_slip(&t, &rocket(12.0)) > 0.0);
        assert_eq!(target_slip(&t, &rocket(t.rocket_fade_speed)), 0.0);
        let idle = WheelDrive {
            idle: true,
            ..held(1.0)
        };
        assert_eq!(target_slip(&t, &idle), 0.0);
        // The spin follows, smoothly.
        let mut slip = 0.0;
        for _ in 0..6 {
            slip = follow(slip, t.grid_slip, t.slip_response, DT);
        }
        assert!(slip > 0.3 * t.grid_slip && slip < t.grid_slip, "{slip}");
        assert_eq!(follow(f32::NAN, 2.0, 5.0, DT), 2.0);
    }

    /// The cloud at GO by the start: a perfect rocket start's is the biggest, a later one's
    /// smaller, then a normal start's, then a stall's.
    #[test]
    fn the_cloud_at_go_follows_the_start() {
        let t = WheelVfx::default();
        let perfect = go_puffs(&t, Start::Rocket(2.5), 2.5);
        let late = go_puffs(&t, Start::Rocket(0.5), 2.5);
        let normal = go_puffs(&t, Start::Normal, 2.5);
        let stall = go_puffs(&t, Start::Stall, 2.5);
        assert_eq!(perfect, t.go_rocket_puffs);
        assert!(
            perfect > late && late > normal && normal > stall,
            "{perfect} {late} {normal} {stall}"
        );
        assert!(stall > 0);
    }

    /// Setting off again after a full stop is seen once, after `stop_time` still; a kart that
    /// only slows down, or backs away, does not set off.
    #[test]
    fn setting_off_from_a_full_stop() {
        let t = WheelVfx::default();
        let mut stop = Standstill::default();
        assert!(!stop.step(&t, 20.0, DT));
        // Slowing down to a stop for a moment only: no puff.
        for _ in 0..10 {
            assert!(!stop.step(&t, 0.2, DT));
        }
        assert!(!stop.step(&t, 3.0, DT));
        for _ in 0..60 {
            assert!(!stop.step(&t, 0.0, DT));
        }
        assert!(stop.step(&t, 1.5, DT), "off");
        assert!(!stop.step(&t, 3.0, DT), "once");
        for _ in 0..60 {
            stop.step(&t, 0.0, DT);
        }
        assert!(!stop.step(&t, -2.0, DT), "backing away");
        for _ in 0..60 {
            stop.step(&t, 0.0, DT);
        }
        stop.forget();
        assert!(
            !stop.step(&t, 2.0, DT),
            "forgotten: the start has its own cloud"
        );
    }
}
