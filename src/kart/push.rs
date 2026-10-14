//! Pushes from outside the driving (GD § 15, issue #34): the sideways push of a paw strike or a
//! throw ([`Shove`]) and the pull of a tether ([`TetherPull`]). Pure logic, no ECS system: the
//! interaction rules (`crate::interactions`) start them on the kart they concern, and the driving
//! (`drive_karts`) applies them every tick. Everything here is unit-tested.
//!
//! **Why a push apart from the driving**: the driving rewrites the velocity every tick and its
//! grip cancels about 40 % of the sideways speed per tick (`lateral_grip` = 30 /s). A plain
//! impulse of 7 m/s would move a kart 23 cm. The push is kept apart: the driving works on the
//! body's velocity *without* it, then adds it back, fading by itself (7 m/s down to 0 in 0.4 s:
//! about 1.4 m). Neither the grip nor the drift (which turns the velocity with the heading) can
//! wipe it out.
//!
//! **Walls first**: the push only lives in the body as long as nothing stops it. After each
//! physics step the driving measures how much of it the body still carries along its direction
//! ([`Shove::carried`]); a wall or another kart that took it ends it there. A kart struck against
//! a wall bumps into it like any kart (issue #27: flat, through its center of mass), it is never
//! pushed on through.
//!
//! **Tether**: no Rapier joint (docs/ARCHITECTURE.md): the driving rewrites the velocities, and on
//! the network the other kart is a kinematic proxy. Each kart gets its own acceleration toward the
//! other one, computed by the interaction rules ([`TetherPull::accel`]); the grabber is towed up
//! to its partner's speed, never beyond ([`towed_forward_speed`]).

use bevy::prelude::*;

/// A sideways push (GD § 15): a paw strike, a throw, or the kart sliding after a strike into
/// thin air. A velocity added on top of the driving's own, fading to 0 by itself; the grip and
/// the drift do not touch it, a wall or a kart in the way ends it. On every kart.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Shove {
    /// Direction of the push: horizontal, unit (world).
    pub direction: Vec3,
    /// Its speed now (m/s), fading to 0.
    pub speed: f32,
    /// How fast it fades (m/s per second).
    pub fade: f32,
    /// The push speed the driving added to the body's velocity on the last tick.
    pub applied: f32,
}

impl Shove {
    /// Starts a push of `speed` (m/s) along the horizontal of `direction`, fading to 0 in
    /// `seconds` (linearly: the kart moves `speed × seconds / 2`). It replaces a push going on:
    /// what the old one put in the body is left to the driving (its grip takes it).
    pub fn start(&mut self, direction: Vec3, speed: f32, seconds: f32) {
        let direction = direction.with_y(0.0).normalize_or_zero();
        if direction == Vec3::ZERO || speed <= 0.0 {
            return;
        }
        *self = Self {
            direction,
            speed,
            fade: speed / seconds.max(1e-3),
            applied: 0.0,
        };
    }

    pub fn is_active(&self) -> bool {
        self.speed > 0.0 || self.applied > 0.0
    }

    /// The push speed the body still carries after the physics step (its velocity `velocity`):
    /// what the driving added on the last tick, or less if something (a wall, a kart) stopped
    /// it on the way.
    pub fn carried(&self, velocity: Vec3) -> f32 {
        self.applied.min(velocity.dot(self.direction).max(0.0))
    }

    /// One tick of `dt` s: the push speed to add to the velocity the driving computes this tick
    /// (without the `carried` part, see [`Shove::carried`]). A push that was stopped on the last
    /// tick goes on with what got through, at most: against a wall, nothing.
    pub fn step(&mut self, carried: f32, dt: f32) -> f32 {
        if carried < self.applied - 1e-4 {
            self.speed = self.speed.min(carried);
        }
        self.speed = (self.speed - self.fade * dt).max(0.0);
        self.applied = self.speed;
        self.speed
    }
}

/// The pull of a tether on this kart this tick (GD § 15), written by the interaction rules before
/// the driving: zero without a tether. On every kart.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct TetherPull {
    /// Acceleration toward the other kart (m/s², world, horizontal), its Weight applied.
    pub accel: Vec3,
    /// Towed (the grabber): the speed the tether may pull it up to, its partner's along its
    /// heading (m/s). `None` for the kart held, and without a tether.
    pub tow_speed: Option<f32>,
}

impl TetherPull {
    pub fn is_active(&self) -> bool {
        *self != Self::default()
    }
}

/// The top speed the driving aims for, for a kart towed up to `tow_speed` (GD § 15: "sa vitesse
/// rejoint celle de la cible") driving at `forward` (m/s) with a top speed of its own `top`:
/// between its own top speed and its partner's speed, it neither accelerates by itself nor slows
/// down (the tether does the pulling). Not towed: `top`.
pub fn towed_top_speed(top: f32, forward: f32, tow_speed: Option<f32>) -> f32 {
    match tow_speed {
        Some(cap) => top.max(forward.min(cap)),
        None => top,
    }
}

/// The forward speed after the tether's pull along the heading (`accel`, m/s², for `dt` s) of a
/// kart the driving brought to `driven`. A towed kart (`tow_speed`) is pulled up to its
/// partner's speed at most, never beyond (GD § 15: it catches up, it does not overtake); a pull
/// backward always applies.
pub fn towed_forward_speed(driven: f32, accel: f32, tow_speed: Option<f32>, dt: f32) -> f32 {
    let pulled = driven + accel * dt;
    match tow_speed {
        Some(cap) if accel > 0.0 => pulled.min(cap.max(driven)),
        _ => pulled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// GD § 15: 7 m/s fading out in 0.4 s, about 1.4 m sideways, whatever the driving does with
    /// the rest of the velocity.
    #[test]
    fn a_shove_fades_out_after_moving_the_kart_about_a_meter_and_a_half() {
        let mut shove = Shove::default();
        assert!(!shove.is_active());
        shove.start(Vec3::new(3.0, 2.0, 0.0), 7.0, 0.4);
        assert_eq!(shove.direction, Vec3::X, "horizontal, unit");
        let mut moved = 0.0;
        let mut ticks = 0;
        // The body carries exactly what was added (nothing in the way).
        while shove.is_active() {
            let carried = shove.carried(Vec3::new(shove.applied, 0.0, -25.0));
            moved += shove.step(carried, DT) * DT;
            ticks += 1;
            assert!(ticks < 60);
        }
        assert!((24..=25).contains(&ticks), "0.4 s: {ticks} ticks");
        assert!((moved - 1.4).abs() < 0.1, "{moved}");
        // Nothing to start.
        let mut none = Shove::default();
        none.start(Vec3::Y, 7.0, 0.4);
        none.start(Vec3::X, 0.0, 0.4);
        assert!(!none.is_active());
    }

    /// Issue #27 and #34: a wall that stops the push ends it; something that slows it down (a
    /// kart pushed along) leaves only what got through.
    #[test]
    fn a_wall_ends_the_shove_and_a_kart_in_the_way_slows_it_down() {
        let mut shove = Shove::default();
        shove.start(Vec3::NEG_X, 10.0, 0.4);
        let first = shove.step(shove.carried(Vec3::ZERO), DT);
        assert!(first > 9.0);
        // The wall stopped the kart (it even bounced back a little).
        let carried = shove.carried(Vec3::new(2.0, 0.0, -20.0));
        assert_eq!(carried, 0.0);
        assert_eq!(shove.step(carried, DT), 0.0);
        assert!(!shove.is_active());

        let mut shove = Shove::default();
        shove.start(Vec3::X, 10.0, 0.4);
        let first = shove.step(0.0, DT);
        // A kart in the way: half of it got through.
        let carried = shove.carried(Vec3::X * first * 0.5);
        let next = shove.step(carried, DT);
        assert!(next < first * 0.5 && next > 0.0, "{next}");
        // The driving's own sideways speed is not the push: never more than what was added.
        assert_eq!(shove.carried(Vec3::X * 50.0), shove.applied);
    }

    /// GD § 15: towed up to its partner's speed, never beyond; pulled back, always.
    #[test]
    fn a_towed_kart_catches_up_but_never_overtakes() {
        assert_eq!(towed_forward_speed(20.0, 60.0, Some(30.0), DT), 21.0);
        assert_eq!(towed_forward_speed(29.5, 120.0, Some(30.0), DT), 30.0);
        // Already faster on its own: the tether does not slow it down to its partner's speed
        // through the forward pull...
        assert_eq!(towed_forward_speed(31.0, 60.0, Some(30.0), DT), 31.0);
        // ...but a pull backward applies.
        assert_eq!(towed_forward_speed(31.0, -60.0, Some(30.0), DT), 30.0);
        // The kart held: no cap.
        assert_eq!(towed_forward_speed(20.0, 60.0, None, DT), 21.0);
        // Between its own top speed and its partner's, it holds its speed.
        assert_eq!(towed_top_speed(28.5, 32.0, Some(37.0)), 32.0);
        assert_eq!(towed_top_speed(28.5, 40.0, Some(37.0)), 37.0);
        assert_eq!(towed_top_speed(28.5, 20.0, Some(37.0)), 28.5);
        assert_eq!(towed_top_speed(28.5, 32.0, None), 28.5);
        assert!(!TetherPull::default().is_active());
    }
}
