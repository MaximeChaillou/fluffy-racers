//! A bot's driving (pure, no ECS): from what it sees to a `KartInput`, exactly what a player's
//! gamepad produces. Everything here is unit-tested.
//!
//! Every tick the bot:
//! 1. finds itself along the track (`RacingLine::locate`);
//! 2. picks its lane: the racing line (more or less of it with the difficulty), plus its own
//!    slowly changing offset (variety), a precision wobble and a shift to overtake the kart in
//!    front (avoidance);
//! 3. aims at a point of that lane ahead (further when faster) and steers toward it (pure
//!    pursuit: the yaw rate that reaches the point, converted to a stick position with the
//!    kart's own steering curve);
//! 4. reads the turns ahead (anticipation) and lifts or brakes so that it can take them, and
//!    brakes when it runs wide at full lock near the edge;
//! 5. drifts the long turns: hop + steer at the turn-in, hold the button, and **release as soon
//!    as the tier it aims for is reached** (it reads the drift charge like a player sees the
//!    sparks). In the drift it feathers the stick (inward / outward each tick) to follow its
//!    lane while charging as fast as the curve allows, and it releases when the turn ends, when
//!    it is about to hit a wall or when it gets too slow;
//! 6. backs up when it is stuck against something;
//! 7. driving techniques (issue #13): accelerates a little after the "2" of the countdown for a
//!    rocket start (never before: no stall), and presses jump/drift right after leaving a trick
//!    ramp for a trick, both with its difficulty's skill.

use bevy::prelude::*;

use super::line::RacingLine;
use super::rng::BotRng;
use super::tuning::{BotSkill, BotTuning};
use crate::kart::{DriftSide, KartInput, KartState, KartTuning, MiniTurbo, drift_yaw_rate};

/// In a drift, "inward" is the kart's threshold (0.5) plus this, so that rounding never turns
/// it into "outward".
const FEATHER_MARGIN: f32 = 0.05;
/// While hopping into a drift, the bot steers at least this much toward the turn, so that the
/// kart picks that side (the kart's threshold is 0.3).
const HOP_STEER: f32 = 0.4;
/// The hop takes about this long (s): the bot hops this much before the turn-in.
const HOP_LEAD_SECONDS: f32 = 0.3;
/// Minimum speed margin above the kart's drift entry speed to start a drift (m/s).
const DRIFT_SPEED_MARGIN: f32 = 3.0;
/// Releases the drift below the kart's drift exit speed plus this (m/s): it would break.
const DRIFT_EXIT_MARGIN: f32 = 2.0;
/// The widest drift turns more than needed below this tightness (over-rotation)...
const OVER_ROTATION: f32 = -0.12;
/// ...for this many ticks near the end of the turn: the turn is ending, release...
const OVER_ROTATION_TICKS: u32 = 4;
/// ...(the last this much of the turn, rad)...
const ENDING_ANGLE: f32 = 0.6;
/// ...or for this many ticks anywhere: the line wants a straighter path.
const LONG_OVER_ROTATION_TICKS: u32 = 30;
/// Releases when closer than this to the inside edge of its lane while turning tight (m).
const INSIDE_ROOM: f32 = 0.3;
/// Past the turn, drifts on for at most this many ticks to reach the next tier...
const STRETCH_TICKS: u32 = 15;
/// ...while it has this much room toward the inside (m).
const STRETCH_ROOM: f32 = 2.0;
/// In a drift, the lane keeps this far from the inside edge of the lane limits (m).
const DRIFT_INSIDE_MARGIN: f32 = 1.5;
/// Hops only with at least this much room toward the inside of the turn (m).
const HOP_INSIDE_ROOM: f32 = 1.5;
/// Hops only when the line at the landing point turns at least this share of the widest
/// drift's turn rate.
const DRIFT_ENTRY_SHARE: f32 = 0.8;
/// Releases when even the tightest drift is this far from turning enough...
const UNDER_ROTATION: f32 = 1.15;
/// ...for this many ticks: it would go wide into the wall.
const UNDER_ROTATION_TICKS: u32 = 8;
/// No new drift for this many ticks after one.
const DRIFT_COOLDOWN_TICKS: u32 = 12;
/// A hop that did not happen (kart not on the ground) is forgotten after this many ticks.
const HOP_TIMEOUT_TICKS: u32 = 45;
/// No drift when a kart is closer than this ahead, in its lane (m).
const DRIFT_TRAFFIC_DISTANCE: f32 = 6.0;
/// Coasts between the target speed and this much above it, brakes beyond (m/s).
const BRAKE_BAND: f32 = 1.5;
/// Brakes when steering at full lock this close to the outside edge of its lane (m)...
const WIDE_ROOM: f32 = 1.5;
/// ...above this speed (m/s).
const WIDE_MIN_SPEED: f32 = 12.0;
/// Karts further apart than this vertically do not see each other (ramp, raised turn) (m).
const NEIGHBOUR_HEIGHT: f32 = 3.0;
/// Mixed into a bot's seed for its technique draws (rocket start, tricks).
const TECHNIQUE_SEED: u64 = 0x7EC4_1C0E_5EED_0013;

/// Another kart, as the bot sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbor {
    pub entity: Entity,
    pub position: Vec3,
    pub velocity: Vec3,
}

/// What the bot knows this tick.
#[derive(Clone, Copy, Debug)]
pub struct Senses<'a> {
    /// The bot's own kart, skipped among `neighbors`.
    pub entity: Entity,
    pub position: Vec3,
    /// The kart's heading (its local -Z) and right.
    pub forward: Vec3,
    pub right: Vec3,
    pub velocity: Vec3,
    pub state: KartState,
    /// Assisted steering (no purple tier).
    pub assisted: bool,
    /// The kart's top speed multiplier (catch-up).
    pub speed_scale: f32,
    /// The race has started (or there is no race): the bot may drive.
    pub racing: bool,
    /// Seconds since the "2" of the countdown, before GO (`None` otherwise): for the rocket
    /// start (GD § 4).
    pub since_two: Option<f32>,
    /// A press of jump/drift now would start a trick: the kart just left a trick ramp (GD § 5).
    pub trick_window: bool,
    /// Tick duration (s).
    pub dt: f32,
    /// Every kart, the bot's own included.
    pub neighbors: &'a [Neighbor],
}

/// What the bot is doing with the jump/drift button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DriftPlan {
    #[default]
    Idle,
    /// Pressed the button to drift toward `side` in `turn` (index into `RacingLine::turns`);
    /// waiting for the landing.
    Hop {
        side: DriftSide,
        target: MiniTurbo,
        turn: usize,
        ticks: u32,
    },
    /// Drifting through `turn`: releases at `target`.
    Drifting { target: MiniTurbo, turn: usize },
    /// Just released: no new hop yet.
    Cooldown { ticks: u32 },
}

/// A bot's memory between ticks.
#[derive(Component, Clone, Debug)]
pub struct BotBrain {
    rng: BotRng,
    /// Centerline sample found last tick (`None`: search the whole track).
    pub sample: Option<usize>,
    /// Where the kart is along the track (see [`crate::bots::Located`]).
    pub distance: f32,
    pub lateral: f32,
    /// Its own lateral offset from the racing line (m), and the one it moves to.
    pub lane: f32,
    lane_target: f32,
    lane_timer: f32,
    /// Precision wobble (m), and the one it moves to.
    wobble: f32,
    wobble_target: f32,
    wobble_timer: f32,
    /// Lateral shift to overtake the kart in front (m).
    pub avoid: f32,
    /// Personal pace: multiplies the corner margin.
    pub pace: f32,
    pub drift: DriftPlan,
    /// Sigma-delta accumulator of the stick feathering in a drift.
    feather: f32,
    over_ticks: u32,
    under_ticks: u32,
    /// Ticks drifted past the end of the turn to reach the next tier.
    stretch_ticks: u32,
    /// The turn the drift chance was rolled for, and the result.
    decided_turn: Option<usize>,
    drift_this_turn: bool,
    /// The jump/drift button last tick: a new hop needs a new press.
    last_jump: bool,
    slow_ticks: u32,
    reverse_ticks: u32,
    /// Draws for the driving techniques (issue #13), apart from the main generator so that they
    /// do not shift the bot's other choices.
    technique_rng: BotRng,
    /// When it accelerates after the "2" for this start (s), drawn once per start.
    start_delay: Option<f32>,
    /// Whether it goes for a trick off the ramp it just left (drawn once per jump); `Some(false)`
    /// also once it pressed.
    trick_roll: Option<bool>,
}

impl BotBrain {
    /// A fresh bot: `seed` picks its personal lane, pace and every later random choice.
    pub fn new(seed: u64, tuning: &BotTuning) -> Self {
        let mut rng = BotRng::new(seed);
        let lane = rng.range(-tuning.lane_spread, tuning.lane_spread);
        let pace = 1.0 + rng.range(-tuning.pace_spread, tuning.pace_spread);
        let lane_timer = rng.range(0.0, tuning.lane_period_max);
        Self {
            rng,
            sample: None,
            distance: 0.0,
            lateral: 0.0,
            lane,
            lane_target: lane,
            lane_timer,
            wobble: 0.0,
            wobble_target: 0.0,
            wobble_timer: 0.0,
            avoid: 0.0,
            pace,
            drift: DriftPlan::Idle,
            feather: 0.0,
            over_ticks: 0,
            under_ticks: 0,
            stretch_ticks: 0,
            decided_turn: None,
            drift_this_turn: false,
            last_jump: false,
            slow_ticks: 0,
            reverse_ticks: 0,
            technique_rng: BotRng::new(seed ^ TECHNIQUE_SEED),
            start_delay: None,
            trick_roll: None,
        }
    }

    /// Forgets what it was doing (carried by the pelican, before GO): no drift, no recovery.
    fn settle(&mut self) {
        self.drift = DriftPlan::Idle;
        self.feather = 0.0;
        self.over_ticks = 0;
        self.under_ticks = 0;
        self.last_jump = false;
        self.slow_ticks = 0;
        self.reverse_ticks = 0;
        self.avoid = 0.0;
    }
}

/// Moves `value` toward `target` by at most `max_delta`.
fn approach(value: f32, target: f32, max_delta: f32) -> f32 {
    value + (target - value).clamp(-max_delta, max_delta)
}

/// Pure pursuit toward `target`: the angle to it (rad, right positive), and the yaw rate that
/// brings a kart driving at `speed` onto it along a circle (rad/s, right turns positive).
pub fn pursuit(position: Vec3, forward: Vec3, right: Vec3, target: Vec3, speed: f32) -> (f32, f32) {
    let to = target - position;
    let (x, z) = (to.dot(right), to.dot(forward));
    let angle = x.atan2(z);
    let distance = (x * x + z * z).sqrt().max(0.1);
    (angle, 2.0 * angle.sin() * speed.max(0.0) / distance)
}

/// Stick position (−1 to 1) for a yaw rate `yaw_right` (right positive) with the normal
/// steering at `speed`, using the kart's own steering curve. When the kart can barely turn (very
/// slow) or the target is behind, full lock toward `angle`.
pub fn steer_for_yaw(kart: &KartTuning, yaw_right: f32, speed: f32, angle: f32) -> f32 {
    let capacity = crate::kart::target_yaw_rate(kart, 1.0, speed).abs();
    if capacity < 0.05 || angle.abs() > 1.4 {
        return angle.signum();
    }
    (yaw_right / capacity).clamp(-1.0, 1.0)
}

/// Fastest speed (m/s) at which a kart follows a curvature `curvature` (1/m) with the normal
/// steering. Mirrors `target_yaw_rate`: the yaw rate goes linearly from `steer_rate` at a
/// standstill to `steer_rate_at_max_speed` at `max_speed`, then stays there.
pub fn corner_speed(kart: &KartTuning, curvature: f32) -> f32 {
    let k = curvature.abs();
    if k < 1e-5 {
        return f32::MAX;
    }
    let a = kart.steer_rate;
    let b = (kart.steer_rate_at_max_speed - kart.steer_rate) / kart.max_speed.max(0.01);
    let v = a / (k - b).max(1e-5);
    if v <= kart.max_speed {
        v
    } else {
        kart.steer_rate_at_max_speed / k
    }
}

/// Same in a drift steered fully inward, counting on `margin` of its turn rate (m/s).
pub fn drift_corner_speed(kart: &KartTuning, curvature: f32, margin: f32) -> f32 {
    let k = curvature.abs();
    if k < 1e-5 {
        return f32::MAX;
    }
    let tight = drift_yaw_rate(kart, DriftSide::Right, 1.0, kart.max_speed).abs();
    tight * margin / k
}

/// The drift's turn rates at full outward and full inward steering (rad/s, positive).
fn drift_rates(kart: &KartTuning, side: DriftSide, speed: f32) -> (f32, f32) {
    (
        drift_yaw_rate(kart, side, -side.sign(), speed).abs(),
        drift_yaw_rate(kart, side, side.sign(), speed).abs(),
    )
}

/// How tight the drift must be to turn at `yaw_right` (right positive): 0 = full outward,
/// 1 = full inward; below 0 even the widest drift turns too much, above 1 the tightest is not
/// enough.
pub fn drift_tightness_needed(
    kart: &KartTuning,
    side: DriftSide,
    yaw_right: f32,
    speed: f32,
) -> f32 {
    let (wide, tight) = drift_rates(kart, side, speed);
    (yaw_right * side.sign() - wide) / (tight - wide).max(1e-3)
}

/// Stick position in a drift toward `side` that turns at `yaw_right` on average.
///
/// The charge only grows fast (+5 instead of +2) with the stick inward past the kart's
/// threshold, which turns tighter than most curves need. So, like a skilled player, the bot
/// alternates tick by tick between "just inward" and "full outward" in the proportion that gives
/// the wanted turn rate (a sigma-delta in `feather`): the yaw rate, smoothed by the kart's
/// steering response, follows the average while the charge grows as fast as the curve allows.
pub fn drift_steer(
    kart: &KartTuning,
    side: DriftSide,
    yaw_right: f32,
    speed: f32,
    feather: &mut f32,
) -> f32 {
    let t = drift_tightness_needed(kart, side, yaw_right, speed);
    let inward = (kart.drift_inner_steer + FEATHER_MARGIN).min(1.0);
    let inward_tightness = (inward + 1.0) / 2.0;
    let stick = if t >= inward_tightness {
        (2.0 * t - 1.0).min(1.0)
    } else {
        *feather += (t / inward_tightness).clamp(0.0, 1.0);
        if *feather >= 1.0 {
            *feather -= 1.0;
            inward
        } else {
            -1.0
        }
    };
    side.sign() * stick
}

/// Mini-turbo charge gained per radian of drift with the feathering (the lower of its two
/// steering positions): what a turn of a given angle can give (GD § 3.3).
pub fn charge_per_radian(kart: &KartTuning, ticks_per_second: f32) -> f32 {
    let inward = (kart.drift_inner_steer + FEATHER_MARGIN).min(1.0);
    let at = |steer: f32| drift_yaw_rate(kart, DriftSide::Right, steer, kart.max_speed).abs();
    let inner = kart.drift_charge_inner * ticks_per_second / at(inward).max(1e-3);
    let outer = kart.drift_charge_outer * ticks_per_second / at(-1.0).max(1e-3);
    inner.min(outer)
}

/// Tier a bot aims for in a drift with `angle` of turning left (rad).
pub fn target_tier(
    skill: &BotSkill,
    kart: &KartTuning,
    angle: f32,
    assisted: bool,
    ticks_per_second: f32,
) -> MiniTurbo {
    let purple = kart.mini_turbo_thresholds[MiniTurbo::Purple.index()];
    if skill.purple_when_possible
        && !assisted
        && angle * charge_per_radian(kart, ticks_per_second) >= purple
    {
        MiniTurbo::Purple
    } else {
        skill.target_tier.min(MiniTurbo::highest(assisted))
    }
}

/// Top speed multiplier of a bot `behind` meters behind the reference along the track
/// (negative: ahead): GD § 12, a light catch-up within ±`catch_up_max`.
pub fn catch_up_scale(behind: f32, tuning: &BotTuning) -> f32 {
    let span = (tuning.catch_up_full_gap - tuning.catch_up_dead_zone).max(1e-3);
    let amount = ((behind.abs() - tuning.catch_up_dead_zone) / span).clamp(0.0, 1.0);
    1.0 + behind.signum() * amount * tuning.catch_up_max.abs()
}

/// The kart to overtake: the closest one ahead in a band around the bot's path that it is
/// catching up. Returns `(distance ahead, lateral offset from the bot, its forward speed)`.
pub fn kart_ahead(
    tuning: &BotTuning,
    me: Entity,
    position: Vec3,
    forward: Vec3,
    right: Vec3,
    velocity: Vec3,
    neighbors: &[Neighbor],
) -> Option<(f32, f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None;
    for other in neighbors {
        if other.entity == me {
            continue;
        }
        let rel = other.position - position;
        if rel.y.abs() > NEIGHBOUR_HEIGHT {
            continue;
        }
        let (ahead, side) = (rel.dot(forward), rel.dot(right));
        if ahead <= 0.5 || ahead > tuning.avoid_distance || side.abs() > tuning.avoid_gap {
            continue;
        }
        let closing = (velocity - other.velocity).dot(forward);
        if closing < tuning.avoid_closing_speed {
            continue;
        }
        if best.is_none_or(|(a, ..)| ahead < a) {
            best = Some((ahead, side, other.velocity.dot(forward)));
        }
    }
    best
}

/// Lane to overtake a kart at lateral `other` (track coordinates) when the bot wants `lane`:
/// the closest of the two lanes `avoid_gap` beside it that fits within `limits`, the side it
/// already chose (`previous` shift sign) first. `None` when neither fits: stay behind.
pub fn passing_lane(
    tuning: &BotTuning,
    lane: f32,
    other: f32,
    limits: [f32; 2],
    previous: f32,
) -> Option<f32> {
    if (lane - other).abs() >= tuning.avoid_gap {
        return Some(lane);
    }
    let left = other - tuning.avoid_gap;
    let right = other + tuning.avoid_gap;
    let fits = |l: f32| l >= limits[0] && l <= limits[1];
    let prefer_right = if previous.abs() > 0.1 {
        previous > 0.0
    } else {
        lane >= other
    };
    let (first, second) = if prefer_right {
        (right, left)
    } else {
        (left, right)
    };
    [first, second].into_iter().find(|l| fits(*l))
}

/// One tick of a bot: what it does with the gamepad.
pub fn drive(
    brain: &mut BotBrain,
    skill: &BotSkill,
    tuning: &BotTuning,
    kart: &KartTuning,
    line: &RacingLine,
    senses: &Senses,
) -> KartInput {
    if senses.state == KartState::Respawning {
        // Carried by the pelican: it will be dropped elsewhere.
        brain.settle();
        brain.sample = None;
        return KartInput::default();
    }
    let here = line.locate(senses.position, brain.sample);
    brain.sample = Some(here.index);
    brain.distance = here.distance;
    brain.lateral = here.lateral;
    if !senses.racing {
        brain.settle();
        // Rocket start (GD § 4): accelerate a little after the "2", never before it.
        let Some(since_two) = senses.since_two else {
            brain.start_delay = None;
            return KartInput::default();
        };
        let delay = match brain.start_delay {
            Some(delay) => delay,
            None => {
                let delay = brain
                    .technique_rng
                    .range(skill.start_delay_min, skill.start_delay_max);
                brain.start_delay = Some(delay);
                delay
            }
        };
        return KartInput {
            throttle: if since_two >= delay { 1.0 } else { 0.0 },
            ..default()
        };
    }
    brain.start_delay = None;

    let dt = senses.dt.max(1e-4);
    let speed = senses.velocity.dot(senses.forward);
    let ticks_per_second = 1.0 / dt;

    // --- Personal lane and precision wobble, slowly changing (variety).
    brain.lane_timer -= dt;
    if brain.lane_timer <= 0.0 {
        brain.lane_target = brain.rng.range(-tuning.lane_spread, tuning.lane_spread);
        brain.lane_timer = brain
            .rng
            .range(tuning.lane_period_min, tuning.lane_period_max);
    }
    brain.lane = approach(brain.lane, brain.lane_target, tuning.lane_change_rate * dt);
    brain.wobble_timer -= dt;
    if brain.wobble_timer <= 0.0 {
        brain.wobble_target = brain.rng.range(-skill.wobble, skill.wobble);
        brain.wobble_timer = skill.wobble_period * brain.rng.range(0.6, 1.4);
    }
    let wobble_rate = 2.0 * skill.wobble / skill.wobble_period.max(0.1);
    brain.wobble = approach(brain.wobble, brain.wobble_target, wobble_rate * dt);

    // --- Where to aim: its lane, a bit ahead.
    let aim = skill.aim_base + skill.aim_per_speed * speed.max(0.0);
    let aim_distance = here.distance + aim;
    let limits = line.limits_at(aim_distance);
    let wanted = (line.offset_at(aim_distance) * skill.line_fidelity + brain.lane + brain.wobble)
        .clamp(limits[0], limits[1]);

    // --- Avoidance: move aside to overtake the kart in front, or stay behind it.
    let ahead = kart_ahead(
        tuning,
        senses.entity,
        senses.position,
        senses.forward,
        senses.right,
        senses.velocity,
        senses.neighbors,
    );
    let mut follow_speed = None;
    let avoid_target = match ahead {
        Some((distance, side, other_speed)) => {
            let other = here.lateral + side;
            match passing_lane(tuning, wanted, other, limits, brain.avoid) {
                Some(lane) => lane - wanted,
                None => {
                    if distance < 3.0 + 0.25 * speed.max(0.0) {
                        follow_speed = Some(other_speed - 0.5);
                    }
                    brain.avoid
                }
            }
        }
        None => 0.0,
    };
    brain.avoid = approach(brain.avoid, avoid_target, tuning.avoid_rate * dt);
    // In a drift, keep away from the inside edge: the drift turns at least its widest rate and
    // would end in the inside wall or the sand.
    let drift_side = match (brain.drift, senses.state) {
        (_, KartState::Drifting { side, .. }) | (DriftPlan::Hop { side, .. }, _) => Some(side),
        _ => None,
    };
    let limits = match drift_side {
        Some(DriftSide::Right) => [limits[0], (limits[1] - DRIFT_INSIDE_MARGIN).max(limits[0])],
        Some(DriftSide::Left) => [(limits[0] + DRIFT_INSIDE_MARGIN).min(limits[1]), limits[1]],
        None => limits,
    };
    let lane = (wanted + brain.avoid).clamp(limits[0], limits[1]);
    let target = line.center().at(aim_distance).point(lane, 0.0);
    let (angle, yaw_right) = pursuit(senses.position, senses.forward, senses.right, target, speed);

    // --- Turns ahead: drift or not (decided once per turn), and how fast to go.
    let look = (skill.anticipation / line.step()).ceil() as usize;
    let upcoming = line.turn_ahead(here.index, look);
    if upcoming.map(|(t, _)| t) != brain.decided_turn {
        brain.decided_turn = upcoming.map(|(t, _)| t);
        brain.drift_this_turn = brain.rng.chance(skill.drift_chance);
    }
    let drift_this_turn = brain.drift_this_turn;
    let worth_drifting = |turn: usize| {
        drift_this_turn && line.remaining_angle(turn, here.index) >= skill.min_drift_angle
    };
    let drifting_turn = match brain.drift {
        DriftPlan::Hop { turn, .. } | DriftPlan::Drifting { turn, .. } => Some(turn),
        _ => upcoming.map(|(t, _)| t).filter(|t| worth_drifting(*t)),
    };

    let mut target_speed = planned_speed(
        line,
        kart,
        skill,
        tuning,
        brain.pace,
        here.index,
        drifting_turn,
        senses.speed_scale,
    );
    if let Some(follow) = follow_speed {
        target_speed = target_speed.min(follow.max(0.0));
    }

    // --- Drift: start, hold, release.
    let horizon = ((speed.max(0.0) * HOP_LEAD_SECONDS + 2.0) / line.step()).ceil() as usize;
    let mut jump = false;
    let mut steer = steer_for_yaw(kart, yaw_right, speed, angle);
    let traffic = ahead.is_some_and(|(d, ..)| d < DRIFT_TRAFFIC_DISTANCE);
    brain.drift = match (brain.drift, senses.state) {
        (DriftPlan::Cooldown { ticks }, _) => {
            if ticks <= 1 {
                DriftPlan::Idle
            } else {
                DriftPlan::Cooldown { ticks: ticks - 1 }
            }
        }
        (DriftPlan::Idle, KartState::Grounded) => {
            let start = upcoming.and_then(|(t, samples_ahead)| {
                let side = line.turns()[t].side;
                let remaining = line.remaining_angle(t, here.index);
                let in_time =
                    samples_ahead as f32 * line.step() <= speed.max(0.0) * HOP_LEAD_SECONDS + 1.0;
                // Where the kart lands, the line must turn at least about as much as the widest
                // drift does, or the drift would cut inside right away.
                let land = here.index + (speed.max(0.0) * HOP_LEAD_SECONDS / line.step()) as usize;
                let land = land % line.sample_count();
                let curved_enough = line.turn_at(land).is_some_and(|(at, _)| at == t)
                    && speed * line.curvature(land, skill.line_fidelity) * side.sign()
                        >= drift_rates(kart, side, speed).0 * DRIFT_ENTRY_SHARE;
                // Not from the very inside edge: the drift would end in the wall or the sand.
                let limits_here = line.limits(here.index);
                let inside_room = if side == DriftSide::Right {
                    limits_here[1] - here.lateral
                } else {
                    here.lateral - limits_here[0]
                };
                (worth_drifting(t)
                    && in_time
                    && curved_enough
                    && inside_room >= HOP_INSIDE_ROOM
                    && speed >= kart.drift_min_speed + DRIFT_SPEED_MARGIN
                    && !traffic
                    && !brain.last_jump)
                    .then_some((t, side, remaining))
            });
            match start {
                Some((turn, side, remaining)) => {
                    jump = true;
                    steer = side.sign() * (steer * side.sign()).max(HOP_STEER);
                    DriftPlan::Hop {
                        side,
                        target: target_tier(
                            skill,
                            kart,
                            remaining,
                            senses.assisted,
                            ticks_per_second,
                        ),
                        turn,
                        ticks: 0,
                    }
                }
                None => DriftPlan::Idle,
            }
        }
        (DriftPlan::Idle, _) => DriftPlan::Idle,
        (
            DriftPlan::Hop {
                side, target, turn, ..
            },
            KartState::Drifting { .. },
        ) => {
            brain.feather = 0.0;
            brain.over_ticks = 0;
            brain.under_ticks = 0;
            brain.stretch_ticks = 0;
            jump = true;
            steer = drift_steer(kart, side, yaw_right, speed, &mut brain.feather);
            DriftPlan::Drifting { target, turn }
        }
        (
            DriftPlan::Hop {
                side,
                target,
                turn,
                ticks,
            },
            state,
        ) => {
            let hopping = matches!(state, KartState::Hop { .. });
            if hopping || (ticks < 2 && state == KartState::Grounded) {
                // In the air (or the hop is about to start): hold, steer into the turn.
                jump = true;
                steer = side.sign() * (steer * side.sign()).max(HOP_STEER);
                if ticks + 1 >= HOP_TIMEOUT_TICKS {
                    DriftPlan::Cooldown {
                        ticks: DRIFT_COOLDOWN_TICKS,
                    }
                } else {
                    DriftPlan::Hop {
                        side,
                        target,
                        turn,
                        ticks: ticks + 1,
                    }
                }
            } else {
                // Landed without drifting (too slow), or never hopped.
                DriftPlan::Cooldown {
                    ticks: DRIFT_COOLDOWN_TICKS,
                }
            }
        }
        (DriftPlan::Drifting { target, turn }, KartState::Drifting { side, charge }) => {
            let t = drift_tightness_needed(kart, side, yaw_right, speed);
            brain.over_ticks = if t < OVER_ROTATION {
                brain.over_ticks + 1
            } else {
                0
            };
            brain.under_ticks = if t > UNDER_ROTATION {
                brain.under_ticks + 1
            } else {
                0
            };
            let tier = KartState::Drifting { side, charge }.mini_turbo(kart, senses.assisted);
            let reached = tier.is_some_and(|tier| tier >= target);
            // Where the kart is relative to the drift's turn.
            let remaining = match line.turn_at(here.index) {
                Some((at, _)) if at == turn => Some(line.remaining_angle(turn, here.index)),
                _ => None,
            };
            let before = remaining.is_none()
                && line.turn_ahead(here.index, horizon).map(|(at, _)| at) == Some(turn);
            let past = remaining.is_none() && !before;
            let ending = remaining.is_some_and(|angle| angle < ENDING_ANGLE);
            // How close the kart is to the inside edge of its lane (sand, wall).
            let limits_here = line.limits(here.index);
            let inside_room = if side == DriftSide::Right {
                limits_here[1] - here.lateral
            } else {
                here.lateral - limits_here[0]
            };
            let outside_room = if side == DriftSide::Right {
                here.lateral - limits_here[0]
            } else {
                limits_here[1] - here.lateral
            };
            // Past the turn, a little more drifting may still reach the next tier.
            let next = MiniTurbo::ALL
                .into_iter()
                .find(|tier| charge < kart.mini_turbo_thresholds[tier.index()])
                .filter(|tier| *tier <= MiniTurbo::highest(senses.assisted));
            let stretch = next.is_some_and(|tier| {
                let missing = kart.mini_turbo_thresholds[tier.index()] - charge;
                brain.stretch_ticks < STRETCH_TICKS
                    && missing
                        <= (STRETCH_TICKS - brain.stretch_ticks) as f32 * kart.drift_charge_outer
                    && inside_room > STRETCH_ROOM
            });
            let turn_done =
                (past && t < 0.2) || (ending && brain.over_ticks >= OVER_ROTATION_TICKS);
            if turn_done && stretch {
                brain.stretch_ticks += 1;
            }
            let release = reached
                || (turn_done && !stretch)
                || brain.over_ticks >= LONG_OVER_ROTATION_TICKS
                || (inside_room < INSIDE_ROOM && t < 0.4)
                || outside_room < -0.5
                || brain.under_ticks >= UNDER_ROTATION_TICKS
                || speed < kart.drift_exit_speed + DRIFT_EXIT_MARGIN;
            if release {
                DriftPlan::Cooldown {
                    ticks: DRIFT_COOLDOWN_TICKS,
                }
            } else {
                jump = true;
                steer = drift_steer(kart, side, yaw_right, speed, &mut brain.feather);
                DriftPlan::Drifting { target, turn }
            }
        }
        (DriftPlan::Drifting { target, turn }, KartState::Airborne) => {
            // A bump or the ramp in the middle of a drift: keep it (the kart allows 0.5 s).
            jump = true;
            DriftPlan::Drifting { target, turn }
        }
        (DriftPlan::Drifting { .. }, _) => DriftPlan::Cooldown {
            // The drift broke (wall, too slow, restart).
            ticks: DRIFT_COOLDOWN_TICKS,
        },
    };

    // --- Speed: lift or brake for the turns ahead; never brake in a drift.
    let in_drift = matches!(senses.state, KartState::Drifting { .. }) && jump;
    let (mut throttle, mut brake) = if speed < target_speed - 0.3 {
        (1.0, 0.0)
    } else if speed < target_speed + BRAKE_BAND || in_drift {
        (if in_drift { 1.0 } else { 0.0 }, 0.0)
    } else {
        (0.0, ((speed - target_speed) / 4.0).clamp(0.3, 1.0))
    };

    // --- Running wide: full lock and the outside edge close → brake to make the turn.
    let edge = line.limits(here.index);
    let outside_room = if steer > 0.0 {
        here.lateral - edge[0]
    } else {
        edge[1] - here.lateral
    };
    if !in_drift && steer.abs() >= 0.95 && outside_room < WIDE_ROOM && speed > WIDE_MIN_SPEED {
        throttle = 0.0;
        brake = 1.0;
    }

    // --- Recovery: stuck against something while trying to drive → back up, steering away.
    let grounded = matches!(
        senses.state,
        KartState::Grounded | KartState::Drifting { .. }
    );
    if brain.reverse_ticks > 0 {
        brain.reverse_ticks -= 1;
        brain.drift = DriftPlan::Cooldown {
            ticks: DRIFT_COOLDOWN_TICKS,
        };
        jump = false;
        throttle = 0.0;
        brake = 1.0;
        // Backing up mirrors the steering: steer away from the target to turn the nose to it.
        steer = -angle.signum();
    } else if grounded && throttle > 0.0 && speed.abs() < tuning.stuck_speed {
        brain.slow_ticks += 1;
        if brain.slow_ticks as f32 * dt >= tuning.stuck_seconds {
            brain.slow_ticks = 0;
            brain.reverse_ticks = (tuning.reverse_seconds * ticks_per_second).round() as u32;
        }
    } else {
        brain.slow_ticks = 0;
    }

    // --- Trick (GD § 5): one press right after leaving a trick ramp, when not drifting.
    if senses.trick_window {
        if brain.trick_roll.is_none() {
            brain.trick_roll = Some(brain.technique_rng.chance(skill.trick_chance));
        }
        let free = matches!(brain.drift, DriftPlan::Idle | DriftPlan::Cooldown { .. });
        if brain.trick_roll == Some(true) && free && !jump && !brain.last_jump {
            jump = true;
            brain.trick_roll = Some(false);
        }
    } else {
        brain.trick_roll = None;
    }

    brain.last_jump = jump;
    KartInput {
        throttle,
        brake,
        steer,
        jump_drift: jump,
        item: false,
        look_back: false,
    }
}

/// The speed to drive at now (m/s): the slowest of what each turn ahead allows once the kart
/// has slowed down for it at `planned_decel`, within `anticipation`; capped by the cruise
/// speed. The turn `drifting_turn` is planned as a drift (faster).
fn planned_speed(
    line: &RacingLine,
    kart: &KartTuning,
    skill: &BotSkill,
    tuning: &BotTuning,
    pace: f32,
    index: usize,
    drifting_turn: Option<usize>,
    speed_scale: f32,
) -> f32 {
    let top = kart.max_speed * speed_scale;
    let cruise = if skill.cruise_speed >= 0.999 {
        f32::MAX
    } else {
        top * skill.cruise_speed
    };
    let margin = skill.corner_margin * pace;
    let samples = (skill.anticipation / line.step()).ceil() as usize;
    let mut speed = cruise;
    for k in 0..=samples {
        let j = (index + k) % line.sample_count();
        let curvature = line.curvature(j, skill.line_fidelity);
        let drifting = drifting_turn.is_some() && line.turn_at(j).map(|(t, _)| t) == drifting_turn;
        let corner = if drifting {
            drift_corner_speed(kart, curvature, tuning.drift_turn_margin)
        } else {
            corner_speed(kart, curvature) * margin
        };
        if corner >= speed {
            continue;
        }
        let distance = k as f32 * line.step();
        let allowed = (corner * corner + 2.0 * skill.planned_decel * distance).sqrt();
        speed = speed.min(allowed);
    }
    speed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::line::tests::prairie_line;
    use crate::bots::tuning::BotDifficulty;

    const DT: f32 = 1.0 / 60.0;

    fn kart() -> KartTuning {
        KartTuning::default()
    }

    #[test]
    fn pursuit_steers_toward_the_target() {
        let k = kart();
        let (pos, fwd, right) = (Vec3::ZERO, Vec3::NEG_Z, Vec3::X);
        // Straight ahead: no steering.
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(0.0, 0.0, -10.0), 20.0);
        assert!(angle.abs() < 1e-6 && yaw.abs() < 1e-6);
        assert_eq!(steer_for_yaw(&k, yaw, 20.0, angle), 0.0);
        // Ahead on the right: steer right, more when further aside.
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(2.0, 0.0, -10.0), 20.0);
        let a = steer_for_yaw(&k, yaw, 20.0, angle);
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(4.0, 0.0, -10.0), 20.0);
        let b = steer_for_yaw(&k, yaw, 20.0, angle);
        assert!(a > 0.0 && b > a, "{a} {b}");
        // On the left: steer left. Behind: full lock.
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(-3.0, 0.0, -10.0), 20.0);
        assert!(steer_for_yaw(&k, yaw, 20.0, angle) < 0.0);
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(1.0, 0.0, 10.0), 20.0);
        assert_eq!(steer_for_yaw(&k, yaw, 20.0, angle), 1.0);
        // The yaw rate is the one of the circle through the target: v · 2 sin(a) / d.
        let (angle, yaw) = pursuit(pos, fwd, right, Vec3::new(10.0, 0.0, -10.0), 10.0);
        let radius = 10.0 / yaw;
        assert!((radius - 10.0).abs() < 1e-3, "{radius} ({angle})");
    }

    #[test]
    fn corner_speed_matches_the_kart_steering() {
        let k = kart();
        for radius in [8.0, 12.0, 15.0, 17.0] {
            let v = corner_speed(&k, 1.0 / radius);
            let capacity = crate::kart::target_yaw_rate(&k, 1.0, v).abs();
            assert!(
                (v / radius - capacity).abs() < 1e-3,
                "radius {radius}: {v} m/s needs {} rad/s, the kart has {capacity}",
                v / radius
            );
        }
        // Gentle curves: flat out (above the top speed).
        assert!(corner_speed(&k, 1.0 / 25.0) > k.max_speed);
        assert_eq!(corner_speed(&k, 0.0), f32::MAX);
        // A drift turns tighter than the normal steering.
        assert!(drift_corner_speed(&k, 1.0 / 15.0, 0.9) > corner_speed(&k, 1.0 / 15.0));
    }

    /// Feathering in a drift: the average turn rate is the wanted one, and the charge grows
    /// faster than with a steady stick.
    #[test]
    fn feathering_follows_the_curve_and_charges_faster() {
        let k = kart();
        let speed = k.max_speed;
        let side = DriftSide::Right;
        let (wide, tight) = drift_rates(&k, side, speed);
        // A curve of 18 m at top speed.
        let wanted = speed / 18.0;
        assert!(wanted > wide && wanted < tight);
        let mut feather = 0.0;
        let (mut yaw, mut charge) = (0.0, 0.0);
        for _ in 0..600 {
            let s = drift_steer(&k, side, wanted, speed, &mut feather);
            assert!(s == -1.0 || s >= k.drift_inner_steer, "{s}");
            yaw += drift_yaw_rate(&k, side, s, speed).abs();
            charge += if s >= k.drift_inner_steer {
                k.drift_charge_inner
            } else {
                k.drift_charge_outer
            };
        }
        let average = yaw / 600.0;
        assert!((average - wanted).abs() < 0.01, "{average} vs {wanted}");
        // A steady stick at that curve would be outward (+2 per tick).
        let steady = 600.0 * k.drift_charge_outer;
        assert!(charge > 1.4 * steady, "{charge} vs {steady}");
        // Tighter than the inward threshold: a steady inward stick.
        let s = drift_steer(&k, side, tight * 0.95, speed, &mut feather);
        assert!(s > k.drift_inner_steer && s < 1.0);
        // Left drift: mirrored.
        let s = drift_steer(&k, DriftSide::Left, -tight * 0.95, speed, &mut feather);
        assert!(s < -k.drift_inner_steer);
    }

    #[test]
    fn the_tier_aimed_for_follows_the_difficulty_and_the_turn() {
        let k = kart();
        let t = BotTuning::default();
        let per_radian = charge_per_radian(&k, 60.0);
        // With the default kart tuning a drift charges about 130 per radian at best.
        assert!((110.0..150.0).contains(&per_radian), "{per_radian}");
        let half_turn = std::f32::consts::PI;
        let easy = t.skill(BotDifficulty::Easy);
        let normal = t.skill(BotDifficulty::Normal);
        let hard = t.skill(BotDifficulty::Hard);
        assert_eq!(
            target_tier(easy, &k, half_turn, false, 60.0),
            MiniTurbo::Blue
        );
        assert_eq!(
            target_tier(normal, &k, half_turn, false, 60.0),
            MiniTurbo::Orange
        );
        assert_eq!(
            target_tier(hard, &k, half_turn, false, 60.0),
            MiniTurbo::Orange
        );
        // A turn long enough for purple: only Hard goes for it, and never with assisted steering.
        let long = 1.1 * k.mini_turbo_thresholds[2] / per_radian;
        assert_eq!(target_tier(hard, &k, long, false, 60.0), MiniTurbo::Purple);
        assert_eq!(
            target_tier(normal, &k, long, false, 60.0),
            MiniTurbo::Orange
        );
        assert_eq!(target_tier(hard, &k, long, true, 60.0), MiniTurbo::Orange);
    }

    #[test]
    fn catch_up_is_bounded_to_3_percent() {
        let t = BotTuning::default();
        assert_eq!(catch_up_scale(0.0, &t), 1.0);
        assert_eq!(catch_up_scale(t.catch_up_dead_zone - 1.0, &t), 1.0);
        assert_eq!(catch_up_scale(-(t.catch_up_dead_zone - 1.0), &t), 1.0);
        // Far behind: faster; far ahead: slower; never more than 3 %.
        assert!((catch_up_scale(1000.0, &t) - 1.03).abs() < 1e-6);
        assert!((catch_up_scale(-1000.0, &t) - 0.97).abs() < 1e-6);
        let mut previous = 0.0;
        for d in -300..300 {
            let s = catch_up_scale(d as f32, &t);
            assert!((0.97 - 1e-6..=1.03 + 1e-6).contains(&s));
            assert!(s >= previous, "monotonic");
            previous = s;
        }
        let halfway = (t.catch_up_dead_zone + t.catch_up_full_gap) / 2.0;
        assert!((catch_up_scale(halfway, &t) - 1.015).abs() < 1e-4);
    }

    fn neighbor(index: u32, position: Vec3, velocity: Vec3) -> Neighbor {
        Neighbor {
            entity: Entity::from_raw_u32(index).unwrap(),
            position,
            velocity,
        }
    }

    #[test]
    fn a_slower_kart_ahead_is_overtaken_on_the_free_side() {
        let t = BotTuning::default();
        let me = Entity::from_raw_u32(1).unwrap();
        let (fwd, right) = (Vec3::NEG_Z, Vec3::X);
        let v = fwd * 25.0;
        let others = [
            // Itself, ignored.
            neighbor(1, Vec3::ZERO, v),
            // Slower, 8 m ahead, slightly right.
            neighbor(2, Vec3::new(0.5, 0.0, -8.0), fwd * 18.0),
            // Behind, and far ahead: ignored.
            neighbor(3, Vec3::new(0.0, 0.0, 6.0), fwd * 30.0),
            neighbor(4, Vec3::new(0.0, 0.0, -40.0), fwd * 10.0),
        ];
        let (ahead, side, other_speed) =
            kart_ahead(&t, me, Vec3::ZERO, fwd, right, v, &others).unwrap();
        assert_eq!((ahead, side, other_speed), (8.0, 0.5, 18.0));
        // It is slightly right of the bot's lane: pass on the left.
        let limits = [-5.0, 5.0];
        let lane = passing_lane(&t, 0.0, 0.5, limits, 0.0).unwrap();
        assert!((lane - (0.5 - t.avoid_gap)).abs() < 1e-6, "{lane}");
        // The left is blocked by the wall: pass on the right.
        let lane = passing_lane(&t, 0.0, 0.5, [-1.0, 5.0], 0.0).unwrap();
        assert!((lane - (0.5 + t.avoid_gap)).abs() < 1e-6, "{lane}");
        // Already moving right: keep that side.
        let lane = passing_lane(&t, 0.0, 0.5, limits, 0.8).unwrap();
        assert!(lane > 0.5);
        // No room on either side: stay behind.
        assert_eq!(passing_lane(&t, 0.0, 0.5, [-1.0, 1.0], 0.0), None);
        // Already beside it: keep the lane.
        assert_eq!(passing_lane(&t, 3.0, 0.5, limits, 0.0), Some(3.0));
        // A kart pulling away, or the same speed, is not a threat.
        let faster = [neighbor(2, Vec3::new(0.5, 0.0, -8.0), fwd * 26.0)];
        assert_eq!(kart_ahead(&t, me, Vec3::ZERO, fwd, right, v, &faster), None);
        // Beside the path (another lane): not in the way.
        let aside = [neighbor(2, Vec3::new(4.0, 0.0, -8.0), fwd * 18.0)];
        assert_eq!(kart_ahead(&t, me, Vec3::ZERO, fwd, right, v, &aside), None);
    }

    /// A kart on the centerline of Prairie Pompon at `distance`, facing the track.
    fn senses_at<'a>(
        line: &RacingLine,
        distance: f32,
        lateral: f32,
        speed: f32,
        state: KartState,
        neighbors: &'a [Neighbor],
    ) -> Senses<'a> {
        let s = line.center().at(distance);
        let t = s.transform(lateral, 0.55);
        Senses {
            entity: Entity::from_raw_u32(1).unwrap(),
            position: t.translation,
            forward: *t.forward(),
            right: *t.right(),
            velocity: *t.forward() * speed,
            state,
            assisted: false,
            speed_scale: 1.0,
            racing: true,
            since_two: None,
            trick_window: false,
            dt: DT,
            neighbors,
        }
    }

    #[test]
    fn the_bot_slows_down_for_the_hairpin_unless_it_drifts_it() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let hairpin = line.turns().iter().position(|t| t.angle > 2.0).unwrap();
        let turn = line.turns()[hairpin];
        // The tightest point of the hairpin.
        let apex = (0..turn.len)
            .map(|m| (turn.start + m) % line.sample_count())
            .max_by(|a, b| line.curvature(*a, 1.0).total_cmp(&line.curvature(*b, 1.0)))
            .unwrap();
        let before_apex = |meters: isize| line.wrap_index(apex as isize - meters);
        let [easy, normal, hard] = BotDifficulty::ALL.map(|d| *t.skill(d));
        let plan = |skill: &BotSkill, i: usize, drift: Option<usize>| {
            planned_speed(&line, &k, skill, &t, 1.0, i, drift, 1.0)
        };
        // At the apex: below top speed with the normal steering, faster when drifting, and
        // faster the harder the bot.
        let at = before_apex(0);
        for skill in [&easy, &normal, &hard] {
            assert!(plan(skill, at, None) < k.max_speed);
            assert!(plan(skill, at, Some(hairpin)) > plan(skill, at, None));
        }
        assert!(plan(&easy, at, None) < plan(&normal, at, None));
        assert!(plan(&normal, at, None) < plan(&hard, at, None));
        // 50 m before the apex, Hard (60 m of anticipation) already sees it, Easy (30 m) not yet.
        let far = before_apex(50);
        assert!(plan(&hard, far, None) < f32::MAX);
        assert_eq!(plan(&easy, far, None), k.max_speed * easy.cruise_speed);
    }

    #[test]
    fn the_bot_hops_into_the_hairpin_then_releases_at_its_tier() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let skill = *t.skill(BotDifficulty::Normal);
        let hairpin = line.turns().iter().position(|t| t.angle > 2.5).unwrap();
        let turn_in = line.turns()[hairpin].start as f32 * line.step();
        let mut brain = BotBrain::new(3, &t);
        brain.drift_this_turn = true;
        brain.decided_turn = Some(hairpin);
        // At the turn-in, at speed, wide: it hops, steering right.
        let senses = senses_at(&line, turn_in + 4.0, -2.0, 26.0, KartState::Grounded, &[]);
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert!(input.jump_drift, "{:?}", brain.drift);
        assert!(input.steer >= HOP_STEER, "{input:?}");
        assert!(matches!(
            brain.drift,
            DriftPlan::Hop {
                side: DriftSide::Right,
                target: MiniTurbo::Orange,
                ..
            }
        ));
        // In the air: still holding, still steering right.
        let hop = KartState::Hop {
            ticks: 5,
            side: Some(DriftSide::Right),
        };
        let senses = senses_at(&line, turn_in, -1.5, 26.0, hop, &[]);
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert!(input.jump_drift && input.steer >= HOP_STEER);
        // Drifting with a blue charge: Normal holds on for orange...
        let mid = turn_in + 20.0;
        let lateral = line.offset_at(mid);
        let blue = KartState::Drifting {
            side: DriftSide::Right,
            charge: 300.0,
        };
        let senses = senses_at(&line, mid, lateral, 26.0, blue, &[]);
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert!(input.jump_drift, "{:?}", brain.drift);
        assert_eq!(input.throttle, 1.0, "no braking in a drift");
        // ...and releases on the tick orange is reached.
        let orange = KartState::Drifting {
            side: DriftSide::Right,
            charge: 570.0,
        };
        let senses = senses_at(&line, mid + 0.5, lateral, 26.0, orange, &[]);
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert!(!input.jump_drift);
        assert!(matches!(brain.drift, DriftPlan::Cooldown { .. }));

        // Easy releases at blue.
        let easy = *t.skill(BotDifficulty::Easy);
        let mut brain = BotBrain::new(4, &t);
        brain.drift = DriftPlan::Drifting {
            target: MiniTurbo::Blue,
            turn: hairpin,
        };
        let senses = senses_at(&line, mid, lateral, 26.0, blue, &[]);
        let input = drive(&mut brain, &easy, &t, &k, &line, &senses);
        assert!(!input.jump_drift);
    }

    #[test]
    fn the_drift_is_released_when_the_turn_ends_or_before_a_wall() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let skill = *t.skill(BotDifficulty::Hard);
        let hairpin_index = line.turns().iter().position(|t| t.angle > 2.5).unwrap();
        let hairpin = line.turns()[hairpin_index];
        let exit = (hairpin.start + hairpin.len) as f32 * line.step();
        let drifting = KartState::Drifting {
            side: DriftSide::Right,
            charge: 400.0,
        };
        // On the straight after the hairpin, still drifting: released (with its blue).
        let mut brain = BotBrain::new(5, &t);
        brain.drift = DriftPlan::Drifting {
            target: MiniTurbo::Orange,
            turn: hairpin_index,
        };
        let mut released = false;
        for k_tick in 0..10 {
            let senses = senses_at(
                &line,
                exit + 5.0 + k_tick as f32 * 0.45,
                0.0,
                27.0,
                drifting,
                &[],
            );
            if !drive(&mut brain, &skill, &t, &k, &line, &senses).jump_drift {
                released = true;
                break;
            }
        }
        assert!(released, "{:?}", brain.drift);
        // Beyond the lane limits (about to scrape the wall): released at once.
        let mid = (hairpin.start + hairpin.len / 2) as f32 * line.step();
        let [_, right] = line.limits_at(mid);
        let mut brain = BotBrain::new(6, &t);
        brain.drift = DriftPlan::Drifting {
            target: MiniTurbo::Orange,
            turn: hairpin_index,
        };
        let senses = senses_at(&line, mid, right + 1.0, 27.0, drifting, &[]);
        assert!(!drive(&mut brain, &skill, &t, &k, &line, &senses).jump_drift);
        // Too slow: released before the kart breaks it.
        let mut brain = BotBrain::new(7, &t);
        brain.drift = DriftPlan::Drifting {
            target: MiniTurbo::Orange,
            turn: hairpin_index,
        };
        let slow = k.drift_exit_speed + 1.0;
        let senses = senses_at(&line, mid, line.offset_at(mid), slow, drifting, &[]);
        assert!(!drive(&mut brain, &skill, &t, &k, &line, &senses).jump_drift);
    }

    #[test]
    fn nothing_before_go_and_nothing_while_carried() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let skill = *t.skill(BotDifficulty::Normal);
        let mut brain = BotBrain::new(1, &t);
        let mut senses = senses_at(&line, -10.0, 2.0, 0.0, KartState::Grounded, &[]);
        senses.racing = false;
        assert_eq!(
            drive(&mut brain, &skill, &t, &k, &line, &senses),
            KartInput::default()
        );
        // On GO it accelerates.
        senses.racing = true;
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert_eq!(input.throttle, 1.0);
        senses.state = KartState::Respawning;
        assert_eq!(
            drive(&mut brain, &skill, &t, &k, &line, &senses),
            KartInput::default()
        );
        assert_eq!(
            brain.sample, None,
            "searches the whole track after the drop"
        );
    }

    #[test]
    fn a_stuck_bot_backs_up_steering_away() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let skill = *t.skill(BotDifficulty::Normal);
        let mut brain = BotBrain::new(1, &t);
        let senses = senses_at(&line, 60.0, 0.0, 0.2, KartState::Grounded, &[]);
        let stuck_ticks = (t.stuck_seconds / DT).round() as usize;
        for _ in 0..stuck_ticks {
            let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
            assert_eq!(input.throttle, 1.0);
        }
        let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
        assert_eq!((input.throttle, input.brake), (0.0, 1.0), "{input:?}");
        let reverse_ticks = (t.reverse_seconds / DT).round() as usize;
        for _ in 1..reverse_ticks {
            assert_eq!(drive(&mut brain, &skill, &t, &k, &line, &senses).brake, 1.0);
        }
        assert_eq!(
            drive(&mut brain, &skill, &t, &k, &line, &senses).throttle,
            1.0
        );
    }

    /// Issue #13: during the countdown a bot accelerates once its delay after the "2" is over
    /// (drawn in its skill's range, never before the "2": no stall); Hard always gets the
    /// perfect start.
    #[test]
    fn bots_go_for_the_rocket_start_after_the_two() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        for difficulty in BotDifficulty::ALL {
            let skill = *t.skill(difficulty);
            for seed in 0..12 {
                let mut brain = BotBrain::new(seed, &t);
                let mut senses = senses_at(&line, -10.0, 2.0, 0.0, KartState::Grounded, &[]);
                senses.racing = false;
                // On the grid and during the "3": nothing.
                assert_eq!(
                    drive(&mut brain, &skill, &t, &k, &line, &senses),
                    KartInput::default()
                );
                let mut first = None;
                for tick in 0..120 {
                    senses.since_two = Some(tick as f32 * DT);
                    let input = drive(&mut brain, &skill, &t, &k, &line, &senses);
                    assert_eq!(
                        (input.brake, input.steer, input.jump_drift),
                        (0.0, 0.0, false)
                    );
                    if input.throttle > 0.0 {
                        first.get_or_insert(tick as f32 * DT);
                    } else {
                        assert!(first.is_none(), "it keeps the accelerator pressed");
                    }
                }
                let first = first.expect("never accelerated");
                assert!(
                    first >= skill.start_delay_min - DT && first <= skill.start_delay_max + DT,
                    "{difficulty:?}: {first} s"
                );
                if difficulty == BotDifficulty::Hard {
                    assert!(first <= k.rocket_start_perfect, "{first}");
                }
            }
        }
    }

    /// Issue #13: right after leaving a trick ramp, a bot presses jump/drift once (Hard always).
    #[test]
    fn bots_press_for_a_trick_off_the_ramp() {
        let (_, line) = prairie_line();
        let k = kart();
        let t = BotTuning::default();
        let skill = *t.skill(BotDifficulty::Hard);
        assert_eq!(skill.trick_chance, 1.0);
        let mut brain = BotBrain::new(3, &t);
        let mut senses = senses_at(&line, 60.0, 0.0, 26.0, KartState::Grounded, &[]);
        assert!(!drive(&mut brain, &skill, &t, &k, &line, &senses).jump_drift);
        senses.trick_window = true;
        senses.state = KartState::Airborne;
        let presses: Vec<bool> = (0..8)
            .map(|_| drive(&mut brain, &skill, &t, &k, &line, &senses).jump_drift)
            .collect();
        assert!(presses[0]);
        assert!(
            presses[1..].iter().all(|p| !p),
            "one press only: {presses:?}"
        );
        // Never on Easy with no chance at all.
        let mut never = *t.skill(BotDifficulty::Easy);
        never.trick_chance = 0.0;
        let mut brain = BotBrain::new(3, &t);
        assert!(!drive(&mut brain, &never, &t, &k, &line, &senses).jump_drift);
    }

    #[test]
    fn bots_with_different_seeds_pick_different_lanes() {
        let t = BotTuning::default();
        let lanes: Vec<f32> = (0..11).map(|seed| BotBrain::new(seed, &t).lane).collect();
        let (min, max) = lanes
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), l| (lo.min(*l), hi.max(*l)));
        assert!(max - min > t.lane_spread, "{lanes:?}");
        assert!(lanes.iter().all(|l| l.abs() <= t.lane_spread));
        // Deterministic.
        assert_eq!(BotBrain::new(5, &t).lane, BotBrain::new(5, &t).lane);
    }
}
