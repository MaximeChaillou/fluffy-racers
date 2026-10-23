//! Hop, drift and mini-turbos (GD § 3, the heart of the game): pure logic, no ECS, no Rapier.
//! Everything here is unit-tested.
//!
//! - The jump/drift button makes the kart **hop**. Steering during the hop (past
//!   `drift_steer_threshold`) picks a side; still holding the button on landing, fast enough
//!   (`drift_min_speed`), starts a **drift** on that side.
//! - While drifting the kart always turns toward its side: steering inward tightens the curve,
//!   outward widens it, and the side never changes.
//! - The mini-turbo charges every tick: +5 steering inward (≥ 0.5), +2 otherwise (assisted
//!   steering: 2 and 0.8). The tick that enters the drift is the first charging tick, so with
//!   the inward rate the blue tier (302.4 since #170, Max's drift; 270 before) is reached on the
//!   61st tick spent drifting (54th before), orange (638.4) on the 128th and purple (974.4) on
//!   the 195th.
//! - Releasing the button fires the boost of the reached tier; before blue, nothing.
//! - The drift breaks, without boost, below `drift_exit_speed` (wall, brake) or after
//!   `drift_max_air_ticks` in the air. Shorter flights keep it, charge frozen.
//!
//! How the drift moves (issue #30, Max: "fluide par rapport au mouvement en cours, arrière
//! léger et glissant, sans angle serré"), in [`DriftMotion`]:
//! - the drift's curve sets in progressively: from the yaw rate the kart had on landing to the
//!   drift's one, along an eased ramp (`drift_curve_in_time`), and fades back to the normal
//!   steering the same way when the drift ends (`drift_curve_out_time`). The direction of
//!   travel follows the same blend: no jolt of the heading or of the velocity, neither on
//!   landing nor on release;
//! - the slip angle (the body against its direction of travel) follows its target like a
//!   damped spring, the target growing with the curve: the rear breaks loose and slides out
//!   progressively, and comes back in line without a snap. The simulated body follows the
//!   trajectory (camera, collisions, network unchanged); the visual turns the body around its
//!   front axle ([`slip_pose`]): the front follows the trajectory, the rear swings out.

use bevy::prelude::*;

use super::physics::{KartState, next_state};
use super::tuning::KartTuning;
use super::{KartInput, WHEEL_ANCHORS};

/// Side of a drift: the side the kart turns toward.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DriftSide {
    Left,
    Right,
}

impl DriftSide {
    /// Steering sign of the side, `KartInput::steer` convention: -1 left, 1 right.
    pub fn sign(self) -> f32 {
        match self {
            DriftSide::Left => -1.0,
            DriftSide::Right => 1.0,
        }
    }

    /// Side asked by `steer`, if it is past `threshold`.
    pub fn from_steer(steer: f32, threshold: f32) -> Option<Self> {
        if steer >= threshold {
            Some(DriftSide::Right)
        } else if steer <= -threshold {
            Some(DriftSide::Left)
        } else {
            None
        }
    }
}

/// Mini-turbo tiers (GD § 3.4), in increasing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Reflect)]
pub enum MiniTurbo {
    /// Mini-turbo: blue sparks.
    Blue,
    /// Super mini-turbo: orange sparks.
    Orange,
    /// Ultra mini-turbo: purple sparks. Disabled with assisted steering (GD § 3.6).
    Purple,
}

impl MiniTurbo {
    pub const ALL: [Self; 3] = [MiniTurbo::Blue, MiniTurbo::Orange, MiniTurbo::Purple];

    /// Index into the `mini_turbo_*` arrays of `KartTuning`.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Highest tier reachable: purple, or orange with assisted steering.
    pub fn highest(assisted: bool) -> Self {
        if assisted {
            MiniTurbo::Orange
        } else {
            MiniTurbo::Purple
        }
    }
}

/// Tier reached with `charge`, if any.
pub fn mini_turbo_tier(tuning: &KartTuning, charge: f32, assisted: bool) -> Option<MiniTurbo> {
    let highest = MiniTurbo::highest(assisted);
    MiniTurbo::ALL
        .into_iter()
        .rev()
        .filter(|tier| *tier <= highest)
        .find(|tier| charge >= tuning.mini_turbo_thresholds[tier.index()])
}

/// The charge stops growing at the threshold of the highest reachable tier.
pub fn max_charge(tuning: &KartTuning, assisted: bool) -> f32 {
    tuning.mini_turbo_thresholds[MiniTurbo::highest(assisted).index()]
}

/// Charge added by one drifting tick. Steering inward means `steer` toward the drift side by
/// at least `drift_inner_steer` (GD § 3.3: ≥ 0.5).
pub fn charge_rate(tuning: &KartTuning, side: DriftSide, steer: f32, assisted: bool) -> f32 {
    let inward = steer * side.sign() >= tuning.drift_inner_steer;
    match (assisted, inward) {
        (false, true) => tuning.drift_charge_inner,
        (false, false) => tuning.drift_charge_outer,
        (true, true) => tuning.drift_charge_inner_assisted,
        (true, false) => tuning.drift_charge_outer_assisted,
    }
}

/// Boost of a tier, as `(strength, duration in seconds)` for `Boost::add`.
pub fn mini_turbo_boost(tuning: &KartTuning, tier: MiniTurbo) -> (f32, f32) {
    (
        tuning.mini_turbo_strengths[tier.index()],
        tuning.mini_turbo_durations[tier.index()],
    )
}

/// How tight the drift is asked to be: 0 = full outward, 0.5 = neutral, 1 = full inward.
pub fn drift_tightness(side: DriftSide, steer: f32) -> f32 {
    (steer.clamp(-1.0, 1.0) * side.sign() + 1.0) * 0.5
}

/// Target yaw rate while drifting (rad/s, counter-clockwise positive like `target_yaw_rate`).
///
/// Always toward the drift side, whatever the steering: between `max_speed /
/// drift_radius_wide` (full outward) and `max_speed / drift_radius_tight` (full inward). The
/// yaw rate does not depend on the speed (like the normal steering), so the radii are exact at
/// `max_speed`, tighter when slower and wider while boosting.
pub fn drift_yaw_rate(tuning: &KartTuning, side: DriftSide, steer: f32, forward_speed: f32) -> f32 {
    let wide = tuning.max_speed / tuning.drift_radius_wide.max(0.1);
    let tight = tuning.max_speed / tuning.drift_radius_tight.max(0.1);
    let rate = wide + (tight - wide) * drift_tightness(side, steer);
    let low_speed_fade = (forward_speed.abs() / tuning.steer_full_speed.max(0.01)).clamp(0.0, 1.0);
    // Turning right is clockwise seen from above: negative yaw.
    -side.sign() * rate * low_speed_fade
}

/// Slip angle of a settled drift (radians): how far the body points inside the curve,
/// relative to where it goes. Positive = nose to the right (right drift). Shown by the visual
/// only: the simulated body keeps its grip (see [`DriftMotion`] and `visual.rs`).
pub fn drift_slip_angle(tuning: &KartTuning, side: DriftSide, steer: f32) -> f32 {
    let t = drift_tightness(side, steer);
    side.sign()
        * (tuning.drift_slip_angle_wide
            + (tuning.drift_slip_angle_tight - tuning.drift_slip_angle_wide) * t)
}

/// Smoothstep: 0 to 1 with a zero slope at both ends.
pub fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// One tick of the slip angle's damped spring toward `target` (`drift_slip_frequency`,
/// `drift_slip_damping`): returns the new angle and its rate (rad, rad/s). Semi-implicit Euler,
/// stable at the 60 Hz tick for any reasonable frequency.
pub fn slip_spring(tuning: &KartTuning, slip: f32, rate: f32, target: f32, dt: f32) -> (f32, f32) {
    let w = tuning.drift_slip_frequency.max(0.0);
    let accel = w * w * (target - slip) - 2.0 * tuning.drift_slip_damping.max(0.0) * w * rate;
    let rate = rate + accel * dt;
    (slip + rate * dt, rate)
}

/// Where the visible body of a kart slipping by `slip` sits, relative to its simulated body:
/// turned by `slip` around the front axle (rotation, translation). The front axle stays on
/// the trajectory and the rear swings out of the turn.
pub fn slip_pose(slip: f32) -> (Quat, Vec3) {
    // Nose to the right = clockwise seen from above = negative rotation around Y.
    let rotation = Quat::from_rotation_y(-slip);
    let front_axle = Vec3::new(0.0, 0.0, WHEEL_ANCHORS[0].z);
    (rotation, front_axle - rotation * front_axle)
}

/// How the drift sets in and fades out (issue #30), on every kart, stepped by the driving each
/// tick (`DriftMotion::step`): the share of the drift's curve applied, the yaw rate the curve
/// started from, and the slip angle's spring.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct DriftMotion {
    /// Side of the current drift, or of the last one while its curve fades out.
    pub side: Option<DriftSide>,
    /// The kart drifts this tick (`KartState::Drifting`).
    pub drifting: bool,
    /// Progress of the drift's curve: 0 = the normal steering, 1 = the drift's. Rises over
    /// `drift_curve_in_time` while drifting, falls over `drift_curve_out_time` after.
    pub curve: f32,
    /// Yaw rate the kart had when the drift started (rad/s, counter-clockwise positive): the
    /// drift's curve sets in from it.
    pub entry_yaw: f32,
    /// Slip angle of the body against its direction of travel (rad, positive = nose to the
    /// right of it), and its rate (rad/s).
    pub slip: f32,
    pub slip_rate: f32,
    /// Slip angle on the previous tick: the visual interpolates between the two.
    pub previous_slip: f32,
}

impl DriftMotion {
    /// Eased share of the drift's curve this tick: 0 = normal steering, 1 = the drift's.
    pub fn blend(&self) -> f32 {
        if self.side.is_some() {
            ease(self.curve)
        } else {
            0.0
        }
    }

    /// One tick. `state` is the kart's state after this tick's transitions, `yaw_rate` its yaw
    /// rate before this tick's steering (rad/s), `steer` the stick.
    pub fn step(
        &mut self,
        tuning: &KartTuning,
        state: &KartState,
        yaw_rate: f32,
        steer: f32,
        dt: f32,
    ) {
        self.previous_slip = self.slip;
        match *state {
            KartState::Respawning | KartState::Hit { .. } => {
                // Carried by the pelican: dropped straight, nothing left of the drift. Hit by an
                // item: the drift is lost, the spin or tumble shows instead (issue #16).
                *self = Self::default();
                return;
            }
            KartState::Drifting { side, .. } => {
                if !self.drifting || self.side != Some(side) {
                    // A new drift: its curve starts from the rotation the kart has now.
                    self.side = Some(side);
                    self.drifting = true;
                    self.curve = 0.0;
                    self.entry_yaw = yaw_rate;
                }
                self.curve = (self.curve + dt / tuning.drift_curve_in_time.max(dt)).min(1.0);
            }
            _ => {
                self.drifting = false;
                self.curve = (self.curve - dt / tuning.drift_curve_out_time.max(dt)).max(0.0);
            }
        }
        let target = match self.side {
            Some(side) => ease(self.curve) * drift_slip_angle(tuning, side, steer),
            None => 0.0,
        };
        (self.slip, self.slip_rate) = slip_spring(tuning, self.slip, self.slip_rate, target, dt);
        if !self.drifting && self.curve == 0.0 {
            self.side = None;
            if self.slip.abs() < 1e-4 && self.slip_rate.abs() < 1e-3 {
                self.slip = 0.0;
                self.slip_rate = 0.0;
            }
        }
    }

    /// Yaw rate the steering aims for this tick (rad/s): `normal` is the normal steering's
    /// target, `drift` the drift's one for `self.side` (same stick, same speed). In a drift the
    /// curve goes from the yaw rate it started from to the drift's; after it, from the drift's
    /// back to the normal steering.
    pub fn yaw_target(&self, normal: f32, drift: f32) -> f32 {
        if self.side.is_none() {
            return normal;
        }
        let base = if self.drifting {
            self.entry_yaw
        } else {
            normal
        };
        base + (drift - base) * self.blend()
    }

    /// Eased share of a drift's curve `seconds` after the drift started (what a bot expects:
    /// it knows how long it has been drifting).
    pub fn blend_after(tuning: &KartTuning, seconds: f32) -> f32 {
        ease(seconds / tuning.drift_curve_in_time.max(1e-3))
    }

    /// Slip angle at `alpha` between the previous tick (0) and this one (1).
    pub fn interpolated_slip(&self, alpha: f32) -> f32 {
        self.previous_slip + (self.slip - self.previous_slip) * alpha.clamp(0.0, 1.0)
    }
}

/// What the state machine needs to know about this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StateContext {
    /// Wheels touching the ground this tick.
    pub wheels_on_ground: usize,
    /// Consecutive ticks without any wheel on the ground.
    pub ticks_without_ground: u32,
    /// Speed along the kart's heading (negative backwards).
    pub forward_speed: f32,
    pub input: KartInput,
    /// Whether the jump/drift button was held on the previous tick: a hop starts on a press,
    /// not while the button stays held.
    pub jump_was_held: bool,
    /// Assisted steering (GD § 11): slower charge, no purple tier.
    pub assisted: bool,
    /// A wheel stands on a trick ramp: the button arms a trick there instead of hopping
    /// (issue #13, `trick.rs`).
    pub on_trick_ramp: bool,
    /// The last press of the button falls in the trick window of a takeoff from a trick ramp
    /// (`TrickTracker::trick_ready`): a trick starts.
    pub trick_ready: bool,
}

/// Result of one state machine tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateStep {
    pub state: KartState,
    /// A hop starts this tick: give the kart `hop_speed` along the ground normal.
    pub hop: bool,
    /// The drift was released with this tier reached: fire its boost.
    pub boost: Option<MiniTurbo>,
    /// A trick landed this tick: fire the trick boost (GD § 5).
    pub trick_landed: bool,
    /// An item hit's spin or tumble ended this tick: the kart's invincibility starts (GD § 8).
    pub hit_ended: bool,
}

impl StateStep {
    fn to(state: KartState) -> Self {
        Self {
            state,
            hop: false,
            boost: None,
            trick_landed: false,
            hit_ended: false,
        }
    }
}

/// Next driving state (docs/ARCHITECTURE.md: `Grounded`, `Hop`, `Drifting`, `Airborne`,
/// `Trick`, `Hit`, `Respawning`).
pub fn step_state(tuning: &KartTuning, state: KartState, ctx: &StateContext) -> StateStep {
    // Ground contact with the usual grace period against bumps.
    let base = next_state(
        ctx.wheels_on_ground,
        ctx.ticks_without_ground,
        tuning.airborne_grace_ticks,
    );
    let grounded = base == KartState::Grounded;
    let held = ctx.input.jump_drift;
    let pressed = held && !ctx.jump_was_held;
    let steered = DriftSide::from_steer(ctx.input.steer, tuning.drift_steer_threshold);

    match state {
        // The tow pelican carries the kart: nothing the driver does matters until the race rules
        // drop it back on the track.
        KartState::Respawning => StateStep::to(KartState::Respawning),
        // Hit by an item (GD § 8): deaf to the driver until the spin or tumble ends, then
        // invincible for a while.
        KartState::Hit {
            reaction,
            elapsed,
            duration,
        } => {
            let elapsed = elapsed.saturating_add(1);
            if elapsed >= duration {
                StateStep {
                    hit_ended: true,
                    ..StateStep::to(base)
                }
            } else {
                StateStep::to(KartState::Hit {
                    reaction,
                    elapsed,
                    duration,
                })
            }
        }
        // Pressed in the trick window after leaving a trick ramp (the kart may still count as
        // grounded for the short grace period): a trick, not a hop.
        KartState::Grounded | KartState::Airborne | KartState::Hop { .. }
            if ctx.trick_ready && ctx.wheels_on_ground == 0 =>
        {
            StateStep::to(KartState::Trick)
        }
        KartState::Trick => {
            if ctx.wheels_on_ground > 0 {
                StateStep {
                    trick_landed: true,
                    ..StateStep::to(KartState::Grounded)
                }
            } else {
                StateStep::to(KartState::Trick)
            }
        }
        KartState::Grounded | KartState::Airborne => {
            // On a trick ramp the button is the trick button: no hop there.
            if grounded && pressed && !ctx.on_trick_ramp {
                StateStep {
                    hop: true,
                    ..StateStep::to(KartState::Hop {
                        ticks: 0,
                        side: steered,
                    })
                }
            } else {
                StateStep::to(base)
            }
        }
        KartState::Hop { ticks, side } => {
            let ticks = ticks.saturating_add(1);
            // The latest side steered during the hop wins.
            let side = steered.or(side);
            if ticks >= tuning.hop_min_ticks && ctx.wheels_on_ground > 0 {
                // Landing.
                return match side {
                    Some(side) if held && ctx.forward_speed >= tuning.drift_min_speed => {
                        // The landing tick is the first charging tick.
                        let charge = charge_rate(tuning, side, ctx.input.steer, ctx.assisted)
                            .min(max_charge(tuning, ctx.assisted));
                        StateStep::to(KartState::Drifting { side, charge })
                    }
                    _ => StateStep::to(KartState::Grounded),
                };
            }
            if ticks >= tuning.hop_max_ticks {
                // Hopped off a ledge: a real flight, no drift.
                return StateStep::to(base);
            }
            StateStep::to(KartState::Hop { ticks, side })
        }
        KartState::Drifting { side, charge } => {
            if !held {
                return StateStep {
                    boost: mini_turbo_tier(tuning, charge, ctx.assisted),
                    ..StateStep::to(base)
                };
            }
            if ctx.ticks_without_ground >= tuning.drift_max_air_ticks
                || ctx.forward_speed < tuning.drift_exit_speed
            {
                // Too long in the air, or too slow (wall, brake): the drift and its charge
                // are lost.
                return StateStep::to(base);
            }
            let charge = if grounded {
                (charge + charge_rate(tuning, side, ctx.input.steer, ctx.assisted))
                    .min(max_charge(tuning, ctx.assisted))
            } else {
                charge
            };
            StateStep::to(KartState::Drifting { side, charge })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::Boost;

    const SPEED: f32 = 25.0;

    fn ctx(jump: bool, was_held: bool, steer: f32) -> StateContext {
        StateContext {
            wheels_on_ground: 4,
            ticks_without_ground: 0,
            forward_speed: SPEED,
            input: KartInput {
                throttle: 1.0,
                steer,
                jump_drift: jump,
                ..Default::default()
            },
            jump_was_held: was_held,
            assisted: false,
            on_trick_ramp: false,
            trick_ready: false,
        }
    }

    fn airborne(mut c: StateContext, ticks: u32) -> StateContext {
        c.wheels_on_ground = 0;
        c.ticks_without_ground = ticks;
        c
    }

    /// Press the button with `steer`, then hold it through a hop of `hop_min_ticks` with the
    /// wheels in the air, and land. Returns the state after landing.
    fn hop_and_land(t: &KartTuning, steer: f32, land: StateContext) -> KartState {
        let step = step_state(t, KartState::Grounded, &ctx(true, false, steer));
        assert!(step.hop);
        let mut state = step.state;
        for i in 1..t.hop_min_ticks {
            state = step_state(t, state, &airborne(ctx(true, true, steer), i)).state;
            assert!(matches!(state, KartState::Hop { .. }), "{state:?}");
        }
        step_state(t, state, &land).state
    }

    fn drifting(side: DriftSide, charge: f32) -> KartState {
        KartState::Drifting { side, charge }
    }

    /// Ticks spent drifting (the landing tick is the first one) until `tier` is reached, in a
    /// right drift steered with a constant `steer`.
    fn ticks_to(t: &KartTuning, steer: f32, assisted: bool, tier: MiniTurbo) -> Option<u32> {
        let mut c = ctx(true, true, steer);
        c.assisted = assisted;
        // Steering left on the landing tick would pick the left side: land with the stick
        // neutral instead, which charges like outward steering.
        let mut land = c;
        land.input.steer = steer.max(0.0);
        let mut state = hop_and_land(t, 1.0, land);
        assert!(matches!(
            state,
            KartState::Drifting {
                side: DriftSide::Right,
                ..
            }
        ));
        for tick in 1..=2000 {
            if tick > 1 {
                state = step_state(t, state, &c).state;
            }
            let KartState::Drifting { charge, .. } = state else {
                panic!("not drifting: {state:?}");
            };
            if mini_turbo_tier(t, charge, assisted) == Some(tier) {
                return Some(tick);
            }
        }
        None
    }

    #[test]
    fn pressing_the_button_hops_but_holding_it_does_not_hop_again() {
        let t = KartTuning::default();
        let step = step_state(&t, KartState::Grounded, &ctx(true, false, 0.0));
        assert!(step.hop);
        assert_eq!(
            step.state,
            KartState::Hop {
                ticks: 0,
                side: None
            }
        );
        // Still held after a hop without drift: no new hop.
        let step = step_state(&t, KartState::Grounded, &ctx(true, true, 0.0));
        assert!(!step.hop);
        assert_eq!(step.state, KartState::Grounded);
        // No hop from the air (tricks come with issue #13).
        let step = step_state(
            &t,
            KartState::Airborne,
            &airborne(ctx(true, false, 0.0), 20),
        );
        assert!(!step.hop);
        assert_eq!(step.state, KartState::Airborne);
    }

    #[test]
    fn hop_with_steering_and_button_held_drifts_on_the_steered_side() {
        let t = KartTuning::default();
        let state = hop_and_land(&t, 1.0, ctx(true, true, 1.0));
        assert!(matches!(
            state,
            KartState::Drifting {
                side: DriftSide::Right,
                ..
            }
        ));
        let state = hop_and_land(&t, -0.8, ctx(true, true, 0.0));
        assert!(matches!(
            state,
            KartState::Drifting {
                side: DriftSide::Left,
                ..
            }
        ));
    }

    #[test]
    fn the_last_side_steered_during_the_hop_wins() {
        let t = KartTuning::default();
        // Steer left at the press, right on landing.
        let state = hop_and_land(&t, -1.0, ctx(true, true, 1.0));
        assert!(matches!(
            state,
            KartState::Drifting {
                side: DriftSide::Right,
                ..
            }
        ));
    }

    #[test]
    fn no_drift_without_steering_without_the_button_or_too_slow() {
        let t = KartTuning::default();
        assert_eq!(
            hop_and_land(&t, 0.0, ctx(true, true, 0.0)),
            KartState::Grounded
        );
        // Light steering below the threshold does not pick a side.
        assert_eq!(
            hop_and_land(&t, 0.1, ctx(true, true, 0.1)),
            KartState::Grounded
        );
        // Released before landing.
        assert_eq!(
            hop_and_land(&t, 1.0, ctx(false, true, 1.0)),
            KartState::Grounded
        );
        // Too slow on landing, or standing still.
        let mut slow = ctx(true, true, 1.0);
        slow.forward_speed = t.drift_min_speed - 0.1;
        assert_eq!(hop_and_land(&t, 1.0, slow), KartState::Grounded);
        slow.forward_speed = 0.0;
        assert_eq!(hop_and_land(&t, 1.0, slow), KartState::Grounded);
    }

    #[test]
    fn hop_lands_only_after_its_minimum_duration() {
        let t = KartTuning::default();
        let mut state = step_state(&t, KartState::Grounded, &ctx(true, false, 1.0)).state;
        // Wheels still on the ground right after the press (springs extending).
        for _ in 1..t.hop_min_ticks {
            state = step_state(&t, state, &ctx(true, true, 1.0)).state;
            assert!(matches!(state, KartState::Hop { .. }));
        }
        state = step_state(&t, state, &ctx(true, true, 1.0)).state;
        assert!(matches!(state, KartState::Drifting { .. }));
    }

    #[test]
    fn a_hop_that_never_lands_becomes_a_flight() {
        let t = KartTuning::default();
        let mut state = step_state(&t, KartState::Grounded, &ctx(true, false, 1.0)).state;
        for i in 1..=t.hop_max_ticks {
            state = step_state(&t, state, &airborne(ctx(true, true, 1.0), i)).state;
        }
        assert_eq!(state, KartState::Airborne);
    }

    /// Issue #170 (Max's drift): the tiers 302.4 / 638.4 / 974.4 at +5 per tick (54 / 114 / 174
    /// ticks with the 270 / 570 / 870 of before).
    #[test]
    fn inward_steering_reaches_the_tiers_at_61_128_and_195_ticks() {
        let t = KartTuning::default();
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Blue), Some(61));
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Orange), Some(128));
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Purple), Some(195));
        // Exactly the threshold for "inward" (GD: ≥ 0.5).
        assert_eq!(ticks_to(&t, 0.5, false, MiniTurbo::Blue), Some(61));
    }

    #[test]
    fn outward_or_neutral_steering_charges_2_5_times_slower() {
        let t = KartTuning::default();
        // 302.4 / 2 = 151.2 → 152 ticks ≈ 2.5 × 61.
        assert_eq!(ticks_to(&t, -1.0, false, MiniTurbo::Blue), Some(152));
        assert_eq!(ticks_to(&t, 0.0, false, MiniTurbo::Blue), Some(152));
        assert_eq!(ticks_to(&t, 0.49, false, MiniTurbo::Blue), Some(152));
    }

    #[test]
    fn assisted_steering_charges_slower_and_never_reaches_purple() {
        let t = KartTuning::default();
        assert_eq!(charge_rate(&t, DriftSide::Right, 1.0, true), 2.0);
        assert_eq!(charge_rate(&t, DriftSide::Right, -1.0, true), 0.8);
        assert_eq!(charge_rate(&t, DriftSide::Left, -1.0, true), 2.0);
        // 302.4 / 2 = 151.2 → 152 ticks; 638.4 / 2 = 319.2 → 320.
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Blue), Some(152));
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Orange), Some(320));
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Purple), None);
        // 302.4 / 0.8 = 378 ticks, and one more: 0.8 added up in f32 stays just under on the
        // 378th.
        assert_eq!(ticks_to(&t, -1.0, true, MiniTurbo::Blue), Some(379));
        // Even a huge charge stays orange.
        assert_eq!(mini_turbo_tier(&t, 10_000.0, true), Some(MiniTurbo::Orange));
        assert_eq!(max_charge(&t, true), 638.4);
    }

    #[test]
    fn charge_is_capped_at_the_highest_tier() {
        let t = KartTuning::default();
        let mut state = drifting(DriftSide::Right, 964.4);
        for _ in 0..10 {
            state = step_state(&t, state, &ctx(true, true, 1.0)).state;
        }
        assert_eq!(state, drifting(DriftSide::Right, 974.4));
    }

    #[test]
    fn releasing_fires_the_boost_of_the_reached_tier() {
        let t = KartTuning::default();
        let release = ctx(false, true, 1.0);
        let cases = [
            (302.0, None),
            (302.4, Some(MiniTurbo::Blue)),
            (638.0, Some(MiniTurbo::Blue)),
            (638.4, Some(MiniTurbo::Orange)),
            (974.0, Some(MiniTurbo::Orange)),
            (974.4, Some(MiniTurbo::Purple)),
        ];
        for (charge, tier) in cases {
            let step = step_state(&t, drifting(DriftSide::Left, charge), &release);
            assert_eq!(step.boost, tier, "charge {charge}");
            assert_eq!(step.state, KartState::Grounded);
            assert!(!step.hop, "releasing does not hop");
        }
        assert_eq!(mini_turbo_boost(&t, MiniTurbo::Blue).1, 0.62);
        assert_eq!(mini_turbo_boost(&t, MiniTurbo::Orange).1, 1.67);
        assert_eq!(mini_turbo_boost(&t, MiniTurbo::Purple).1, 2.63);
    }

    #[test]
    fn releasing_before_blue_gives_nothing() {
        let t = KartTuning::default();
        let mut state = hop_and_land(&t, 1.0, ctx(true, true, 1.0));
        for _ in 0..52 {
            state = step_state(&t, state, &ctx(true, true, 1.0)).state;
        }
        // 53 drifting ticks: 265 units, just below blue.
        assert_eq!(state, drifting(DriftSide::Right, 265.0));
        let step = step_state(&t, state, &ctx(false, true, 1.0));
        assert_eq!(step.boost, None);
        assert_eq!(step.state, KartState::Grounded);
    }

    #[test]
    fn the_drift_side_never_changes() {
        let t = KartTuning::default();
        let mut state = drifting(DriftSide::Right, 0.0);
        for _ in 0..100 {
            state = step_state(&t, state, &ctx(true, true, -1.0)).state;
            assert!(matches!(
                state,
                KartState::Drifting {
                    side: DriftSide::Right,
                    ..
                }
            ));
        }
        // Full outward steering still turns toward the drift side, just wider.
        let tight = drift_yaw_rate(&t, DriftSide::Right, 1.0, SPEED);
        let neutral = drift_yaw_rate(&t, DriftSide::Right, 0.0, SPEED);
        let wide = drift_yaw_rate(&t, DriftSide::Right, -1.0, SPEED);
        assert!(
            tight < neutral && neutral < wide && wide < 0.0,
            "{tight} {neutral} {wide}"
        );
        let left = drift_yaw_rate(&t, DriftSide::Left, 1.0, SPEED);
        assert!(left > 0.0, "a left drift turns counter-clockwise: {left}");
    }

    #[test]
    fn drift_radii_are_exact_at_max_speed() {
        let t = KartTuning::default();
        let radius = |steer: f32| t.max_speed / drift_yaw_rate(&t, DriftSide::Left, steer, 1e3);
        assert!((radius(-1.0) - t.drift_radius_tight).abs() < 1e-3);
        assert!((radius(1.0) - t.drift_radius_wide).abs() < 1e-3);
        let neutral = radius(0.0);
        assert!(neutral > t.drift_radius_tight && neutral < t.drift_radius_wide);
    }

    #[test]
    fn slip_angle_points_the_nose_inside_more_when_tight() {
        let t = KartTuning::default();
        let right_tight = drift_slip_angle(&t, DriftSide::Right, 1.0);
        let right_wide = drift_slip_angle(&t, DriftSide::Right, -1.0);
        assert!((right_tight - t.drift_slip_angle_tight).abs() < 1e-6);
        assert!((right_wide - t.drift_slip_angle_wide).abs() < 1e-6);
        assert!(right_tight > right_wide && right_wide > 0.0);
        assert!(drift_slip_angle(&t, DriftSide::Left, 0.0) < 0.0);
    }

    #[test]
    fn drift_breaks_without_boost_when_too_slow() {
        let t = KartTuning::default();
        let mut slow = ctx(true, true, 1.0);
        slow.forward_speed = t.drift_exit_speed - 0.1;
        let step = step_state(&t, drifting(DriftSide::Right, 600.0), &slow);
        assert_eq!(step.state, KartState::Grounded);
        assert_eq!(step.boost, None);
        // Between the exit speed and the entry speed: the drift goes on (hysteresis).
        slow.forward_speed = t.drift_exit_speed + 0.1;
        let step = step_state(&t, drifting(DriftSide::Right, 600.0), &slow);
        assert!(matches!(step.state, KartState::Drifting { .. }));
    }

    #[test]
    fn short_flights_keep_the_drift_with_its_charge_frozen_long_ones_break_it() {
        let t = KartTuning::default();
        let held = ctx(true, true, 1.0);
        // Within the ground grace period: still charging.
        let step = step_state(&t, drifting(DriftSide::Left, 100.0), &airborne(held, 1));
        assert_eq!(step.state, drifting(DriftSide::Left, 102.0));
        // A real flight: frozen.
        let step = step_state(
            &t,
            drifting(DriftSide::Left, 100.0),
            &airborne(held, t.airborne_grace_ticks),
        );
        assert_eq!(step.state, drifting(DriftSide::Left, 100.0));
        // Too long: lost, no boost.
        let step = step_state(
            &t,
            drifting(DriftSide::Left, 600.0),
            &airborne(held, t.drift_max_air_ticks),
        );
        assert_eq!(step.state, KartState::Airborne);
        assert_eq!(step.boost, None);
        // Releasing in the air still fires the boost.
        let step = step_state(
            &t,
            drifting(DriftSide::Left, 700.0),
            &airborne(ctx(false, true, 0.0), 10),
        );
        assert_eq!(step.boost, Some(MiniTurbo::Orange));
        assert_eq!(step.state, KartState::Airborne);
    }

    #[test]
    fn a_new_hop_needs_a_new_press_after_a_drift() {
        let t = KartTuning::default();
        let step = step_state(
            &t,
            drifting(DriftSide::Right, 300.0),
            &ctx(false, true, 1.0),
        );
        assert_eq!(step.state, KartState::Grounded);
        let step = step_state(&t, step.state, &ctx(true, false, 1.0));
        assert!(step.hop, "chaining drifts: release, then press again");
    }

    #[test]
    fn mini_turbo_boost_combines_with_other_boosts() {
        let t = KartTuning::default();
        // A long, weaker boost is running: the mini-turbo keeps the longest time and the
        // strongest intensity (GD § 2).
        let (strength, duration) = mini_turbo_boost(&t, MiniTurbo::Blue);
        let mut boost = Boost {
            strength: strength / 2.0,
            remaining: 3.0,
        };
        boost.add(strength, duration);
        assert_eq!(
            boost,
            Boost {
                strength,
                remaining: 3.0
            }
        );
        // Released again while a stronger, shorter boost runs.
        let (purple_strength, purple_duration) = mini_turbo_boost(&t, MiniTurbo::Purple);
        let mut boost = Boost {
            strength: purple_strength * 2.0,
            remaining: 0.2,
        };
        boost.add(purple_strength, purple_duration);
        assert_eq!(boost.strength, purple_strength * 2.0);
        assert_eq!(boost.remaining, purple_duration);
    }

    #[test]
    fn higher_tiers_last_longer_and_are_at_least_as_strong() {
        let t = KartTuning::default();
        for pair in MiniTurbo::ALL.windows(2) {
            let (low, high) = (mini_turbo_boost(&t, pair[0]), mini_turbo_boost(&t, pair[1]));
            assert!(high.0 >= low.0 && high.1 > low.1, "{low:?} vs {high:?}");
            assert!(
                t.mini_turbo_thresholds[pair[1].index()] > t.mini_turbo_thresholds[pair[0].index()]
            );
        }
    }

    // --- Issue #30: the drift sets in and fades out progressively ------------------------------

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn ease_is_a_smoothstep() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
        // Flat at both ends, steepest in the middle.
        assert!(ease(0.01) < 0.001 && ease(0.99) > 0.999);
        for i in 0..100 {
            let (a, b) = (i as f32 / 100.0, (i + 1) as f32 / 100.0);
            assert!(ease(b) >= ease(a));
        }
    }

    /// The curve rises linearly over `drift_curve_in_time` (eased by `blend`), starting from the
    /// yaw rate the kart had on landing, then fades back to the normal steering over
    /// `drift_curve_out_time` once the drift ends.
    #[test]
    fn the_drift_curve_sets_in_from_the_entry_yaw_and_fades_out() {
        let t = KartTuning::default();
        let mut motion = DriftMotion::default();
        assert_eq!(
            motion.yaw_target(-1.0, -2.0),
            -1.0,
            "no drift: the normal steering"
        );
        let entry = -0.8;
        let (normal, drift) = (-1.2, -1.6);
        let state = drifting(DriftSide::Right, 5.0);
        motion.step(&t, &state, entry, 1.0, DT);
        assert_eq!(motion.side, Some(DriftSide::Right));
        assert!(motion.drifting);
        assert_eq!(motion.entry_yaw, entry);
        // First tick: barely off the rotation in progress.
        let first = motion.yaw_target(normal, drift);
        assert!((first - entry).abs() < 0.01, "{first}");
        // Whole ticks: 0.455 s is 27.3 ticks, set in on the 28th.
        let ticks_in = (t.drift_curve_in_time / DT).ceil() as usize;
        let mut previous = first;
        for _ in 1..ticks_in {
            motion.step(&t, &state, -1.0, 1.0, DT);
            let target = motion.yaw_target(normal, drift);
            assert!(target <= previous + 1e-6, "monotonic toward the drift's");
            previous = target;
        }
        assert!((motion.curve - 1.0).abs() < 1e-3, "{}", motion.curve);
        assert!((motion.yaw_target(normal, drift) - drift).abs() < 1e-3);
        // Released: from the drift's back to the normal steering, then nothing left.
        let ticks_out = (t.drift_curve_out_time / DT).ceil() as usize;
        motion.step(&t, &KartState::Grounded, drift, 0.0, DT);
        assert!(!motion.drifting);
        assert_eq!(motion.side, Some(DriftSide::Right));
        let after = motion.yaw_target(normal, drift);
        assert!((after - drift).abs() < 0.01, "no snap on release: {after}");
        for _ in 1..ticks_out {
            motion.step(&t, &KartState::Grounded, -1.3, 0.0, DT);
        }
        assert_eq!(motion.curve, 0.0);
        assert_eq!(motion.side, None);
        assert_eq!(motion.yaw_target(normal, drift), normal);
        assert_eq!(motion.blend(), 0.0);
    }

    #[test]
    fn a_new_drift_starts_from_the_rotation_in_progress_and_the_pelican_resets_it() {
        let t = KartTuning::default();
        let mut motion = DriftMotion::default();
        for _ in 0..60 {
            motion.step(&t, &drifting(DriftSide::Right, 100.0), -1.5, 1.0, DT);
        }
        // Released, then a drift on the other side before the curve faded out: it starts again
        // from the rotation the kart has, not from the old curve.
        motion.step(&t, &KartState::Grounded, -1.5, 0.0, DT);
        motion.step(&t, &drifting(DriftSide::Left, 2.0), -0.9, -1.0, DT);
        assert_eq!(motion.side, Some(DriftSide::Left));
        assert_eq!(motion.entry_yaw, -0.9);
        assert!(motion.curve < 0.1);
        assert!((motion.yaw_target(1.0, 1.4) - -0.9).abs() < 0.05);
        // Carried by the pelican: nothing left of the drift, slip included.
        motion.step(&t, &KartState::Respawning, 0.0, 0.0, DT);
        assert_eq!(motion, DriftMotion::default());
    }

    /// The slip angle follows its target like a damped spring: it starts with no jump, rises
    /// smoothly, overshoots a little (damping below 1: a light, slippery rear) and settles.
    #[test]
    fn the_slip_spring_settles_on_its_target_with_a_light_overshoot() {
        let t = KartTuning::default();
        let target = t.drift_slip_angle_tight;
        let (mut slip, mut rate) = (0.0, 0.0);
        let mut slips = Vec::new();
        for _ in 0..120 {
            (slip, rate) = slip_spring(&t, slip, rate, target, DT);
            slips.push(slip);
        }
        // Even toward a target that jumps (the drift's own target grows with the curve), the
        // first tick moves by w² dt² of it: under 5 %.
        assert!(
            slips[0] < 0.05 * target,
            "no jump on the first tick: {}",
            slips[0]
        );
        // Issue #170 (Max's drift: damping 0.532, a lighter rear): the overshoot of a damped
        // spring, exp(−πζ / √(1 − ζ²)), 14 % (5 % with the 0.7 of #30), within 2 % of the target
        // (the 60 Hz steps overshoot a little less: 12.6 %).
        let highest = slips.iter().copied().fold(0.0, f32::max);
        let zeta = t.drift_slip_damping;
        let overshoot = (-std::f32::consts::PI * zeta / (1.0 - zeta * zeta).sqrt()).exp();
        assert!(highest > target && highest < 1.15 * target, "{highest}");
        assert!(
            (highest / target - 1.0 - overshoot).abs() < 0.02,
            "{highest}: {overshoot} expected"
        );
        assert!(
            (slips[119] - target).abs() < 0.005 * target,
            "{}",
            slips[119]
        );
        // Critically damped (1): no overshoot at all.
        let critical = KartTuning {
            drift_slip_damping: 1.0,
            ..t
        };
        let (mut slip, mut rate) = (0.0, 0.0);
        for _ in 0..240 {
            (slip, rate) = slip_spring(&critical, slip, rate, target, DT);
            assert!(slip <= target + 1e-5);
        }
    }

    /// The slip angle turns the body around its front axle: the front axle stays put, the rear
    /// axle swings out of the turn (to the left in a right drift, nose to the right).
    #[test]
    fn slip_pose_keeps_the_front_axle_and_swings_the_rear_out() {
        let slip = 0.3;
        let (rotation, offset) = slip_pose(slip);
        let pose = |p: Vec3| rotation * p + offset;
        let front = Vec3::Z * WHEEL_ANCHORS[0].z;
        let rear = Vec3::Z * WHEEL_ANCHORS[2].z;
        assert!(pose(front).distance(front) < 1e-5);
        let moved = pose(rear) - rear;
        let wheelbase = WHEEL_ANCHORS[2].z - WHEEL_ANCHORS[0].z;
        assert!((moved.x - -wheelbase * slip.sin()).abs() < 1e-4, "{moved}");
        // The nose points to the right (+X) of the direction of travel (-Z).
        assert!((rotation * Vec3::NEG_Z).x > 0.2);
        assert_eq!(slip_pose(0.0), (Quat::IDENTITY, Vec3::ZERO));
        // The visual interpolates the angle between two ticks.
        let motion = DriftMotion {
            previous_slip: 0.1,
            slip: 0.2,
            ..default()
        };
        assert!((motion.interpolated_slip(0.5) - 0.15).abs() < 1e-6);
        assert_eq!(motion.interpolated_slip(2.0), 0.2);
    }

    /// Issue #5: the tow pelican's state ignores the driver (no hop, no drift, no boost) and
    /// only the race rules end it.
    #[test]
    fn respawning_ignores_the_driver_until_the_race_rules_end_it() {
        let t = KartTuning::default();
        for c in [
            ctx(true, false, 1.0),
            ctx(false, true, -1.0),
            airborne(ctx(true, false, 0.0), 100),
        ] {
            let step = step_state(&t, KartState::Respawning, &c);
            assert_eq!(step.state, KartState::Respawning);
            assert!(!step.hop);
            assert_eq!(step.boost, None);
        }
        assert_eq!(KartState::Respawning.mini_turbo(&t, false), None);
    }

    /// Issue #13: in the trick window after a trick ramp the button starts a trick, not a hop;
    /// the trick lasts until the landing, which fires the trick boost.
    #[test]
    fn a_press_in_the_trick_window_starts_a_trick_that_lands_with_a_boost() {
        let t = KartTuning::default();
        let mut air = airborne(ctx(true, false, 0.0), 2);
        air.trick_ready = true;
        // Even during the grace period, when the kart still counts as grounded: no hop.
        for state in [
            KartState::Grounded,
            KartState::Airborne,
            KartState::Hop {
                ticks: 3,
                side: None,
            },
        ] {
            let step = step_state(&t, state, &air);
            assert_eq!(step.state, KartState::Trick, "{state:?}");
            assert!(!step.hop);
            assert!(!step.trick_landed);
        }
        // In the air the trick goes on, whatever the driver does.
        for c in [ctx(false, true, 1.0), ctx(true, false, -1.0)] {
            let step = step_state(&t, KartState::Trick, &airborne(c, 10));
            assert_eq!(step.state, KartState::Trick);
            assert!(!step.trick_landed && !step.hop);
        }
        // Landing: the trick boost.
        let step = step_state(&t, KartState::Trick, &ctx(false, false, 0.0));
        assert_eq!(step.state, KartState::Grounded);
        assert!(step.trick_landed);
        assert_eq!(step.boost, None);
        assert_eq!(KartState::Trick.mini_turbo(&t, false), None);
    }

    #[test]
    fn the_button_does_not_hop_on_a_trick_ramp_and_nothing_tricks_outside_the_window() {
        let t = KartTuning::default();
        let mut on_ramp = ctx(true, false, 1.0);
        on_ramp.on_trick_ramp = true;
        let step = step_state(&t, KartState::Grounded, &on_ramp);
        assert!(!step.hop);
        assert_eq!(step.state, KartState::Grounded);
        // In the air outside the window: nothing.
        let step = step_state(
            &t,
            KartState::Airborne,
            &airborne(ctx(true, false, 0.0), 10),
        );
        assert_eq!(step.state, KartState::Airborne);
        // A wheel on the ground: a hop as usual, no trick.
        let mut grounded = ctx(true, false, 0.0);
        grounded.trick_ready = true;
        let step = step_state(&t, KartState::Grounded, &grounded);
        assert!(step.hop);
        assert_ne!(step.state, KartState::Trick);
        // A plain landing: no trick boost.
        let step = step_state(&t, KartState::Airborne, &ctx(false, false, 0.0));
        assert!(!step.trick_landed);
    }
}
