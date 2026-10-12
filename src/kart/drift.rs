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
//!   the inward rate the blue tier (270) is reached on the 54th tick spent drifting, orange
//!   (570) on the 114th and purple (870) on the 174th.
//! - Releasing the button fires the boost of the reached tier; before blue, nothing.
//! - The drift breaks, without boost, below `drift_exit_speed` (wall, brake) or after
//!   `drift_max_air_ticks` in the air. Shorter flights keep it, charge frozen.

use super::KartInput;
use super::physics::{KartState, next_state};
use super::tuning::KartTuning;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Visual slip angle of a drifting kart (radians): how far its nose points inside the curve,
/// relative to where it goes. Positive = nose to the right (right drift). Purely visual: the
/// simulated body keeps its normal grip (see `visual.rs`).
pub fn drift_slip_angle(tuning: &KartTuning, side: DriftSide, steer: f32) -> f32 {
    let t = drift_tightness(side, steer);
    side.sign()
        * (tuning.drift_slip_angle_wide
            + (tuning.drift_slip_angle_tight - tuning.drift_slip_angle_wide) * t)
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
}

/// Result of one state machine tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateStep {
    pub state: KartState,
    /// A hop starts this tick: give the kart `hop_speed` along the ground normal.
    pub hop: bool,
    /// The drift was released with this tier reached: fire its boost.
    pub boost: Option<MiniTurbo>,
}

impl StateStep {
    fn to(state: KartState) -> Self {
        Self {
            state,
            hop: false,
            boost: None,
        }
    }
}

/// Next driving state (docs/ARCHITECTURE.md: `Grounded`, `Hop`, `Drifting`, `Airborne`,
/// `Respawning`).
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
        KartState::Grounded | KartState::Airborne => {
            if grounded && pressed {
                StateStep {
                    state: KartState::Hop {
                        ticks: 0,
                        side: steered,
                    },
                    hop: true,
                    boost: None,
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
                    state: base,
                    hop: false,
                    boost: mini_turbo_tier(tuning, charge, ctx.assisted),
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

    #[test]
    fn inward_steering_reaches_the_tiers_at_54_114_and_174_ticks() {
        let t = KartTuning::default();
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Blue), Some(54));
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Orange), Some(114));
        assert_eq!(ticks_to(&t, 1.0, false, MiniTurbo::Purple), Some(174));
        // Exactly the threshold for "inward" (GD: ≥ 0.5).
        assert_eq!(ticks_to(&t, 0.5, false, MiniTurbo::Blue), Some(54));
    }

    #[test]
    fn outward_or_neutral_steering_charges_2_5_times_slower() {
        let t = KartTuning::default();
        // 270 / 2 = 135 ticks = 2.5 × 54.
        assert_eq!(ticks_to(&t, -1.0, false, MiniTurbo::Blue), Some(135));
        assert_eq!(ticks_to(&t, 0.0, false, MiniTurbo::Blue), Some(135));
        assert_eq!(ticks_to(&t, 0.49, false, MiniTurbo::Blue), Some(135));
    }

    #[test]
    fn assisted_steering_charges_slower_and_never_reaches_purple() {
        let t = KartTuning::default();
        assert_eq!(charge_rate(&t, DriftSide::Right, 1.0, true), 2.0);
        assert_eq!(charge_rate(&t, DriftSide::Right, -1.0, true), 0.8);
        assert_eq!(charge_rate(&t, DriftSide::Left, -1.0, true), 2.0);
        // 270 / 2 = 135 ticks; 570 / 2 = 285.
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Blue), Some(135));
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Orange), Some(285));
        assert_eq!(ticks_to(&t, 1.0, true, MiniTurbo::Purple), None);
        // 270 / 0.8 = 337.5 → 338 ticks.
        assert_eq!(ticks_to(&t, -1.0, true, MiniTurbo::Blue), Some(338));
        // Even a huge charge stays orange.
        assert_eq!(mini_turbo_tier(&t, 10_000.0, true), Some(MiniTurbo::Orange));
        assert_eq!(max_charge(&t, true), 570.0);
    }

    #[test]
    fn charge_is_capped_at_the_highest_tier() {
        let t = KartTuning::default();
        let mut state = drifting(DriftSide::Right, 860.0);
        for _ in 0..10 {
            state = step_state(&t, state, &ctx(true, true, 1.0)).state;
        }
        assert_eq!(state, drifting(DriftSide::Right, 870.0));
    }

    #[test]
    fn releasing_fires_the_boost_of_the_reached_tier() {
        let t = KartTuning::default();
        let release = ctx(false, true, 1.0);
        let cases = [
            (269.0, None),
            (270.0, Some(MiniTurbo::Blue)),
            (569.0, Some(MiniTurbo::Blue)),
            (570.0, Some(MiniTurbo::Orange)),
            (870.0, Some(MiniTurbo::Purple)),
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
            drifting(DriftSide::Left, 600.0),
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
}
