//! The rules of the paw strike and the grab (GD § 15, issue #34): pure logic, no ECS. Everything
//! here is unit-tested; `mod.rs` runs it every tick for every kart.
//!
//! - **Button** ([`step_paw`]): a press released before `tap_time` (0.2 s) strikes on the release;
//!   held longer, the paw reaches out for `grab_window` (0.5 s) and grabs the first kart in reach;
//!   the tether lasts while the button is held, `tether_max_time` (2 s) at most. One cooldown for
//!   the button: 1.5 s after a strike or a grab into thin air, 3 s after a tether. During the
//!   cooldown, and when the kart may not act, a press does nothing until it is released.
//! - **Reach** ([`reach`]): in the kart's frame, to the center of the other kart: to the side
//!   (strike and grab) or just ahead (grab only). The target ([`pick_target`]) is the closest kart
//!   in reach that is not immune; with karts on both sides, the side steered toward wins (in a
//!   drift, its inner side).
//! - **Immunities** (the table of GD § 15): [`can_strike`], [`can_grab`], [`can_be_struck`],
//!   [`can_be_grabbed`].
//! - **Tether** ([`tether_accel`], [`held_accel`]): a damped spring between the two karts that
//!   only pulls; each kart computes its own pull toward the other one (docs/ARCHITECTURE.md).

use bevy::prelude::*;

use super::tuning::InteractionTuning;
use crate::TICK_HZ;
use crate::kart::{DriftSide, KART_HALF_EXTENTS, KartState};

/// Seconds → whole ticks at 60 Hz.
pub fn ticks(seconds: f32) -> u32 {
    (seconds.max(0.0) as f64 * TICK_HZ).round() as u32
}

/// What a kart's paw is doing (the Interaction button).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PawMotion {
    #[default]
    Idle,
    /// The button is down for `ticks` ticks, less than `tap_time`: the paw is raised (GD § 15:
    /// it shows at once); a release now strikes.
    Raised { ticks: u32 },
    /// Held past `tap_time`: the paw reaches out, for `ticks` ticks so far.
    Reaching { ticks: u32 },
    /// Holding `target` (the grabber's side of a tether) for `ticks` ticks. `rest`: the tether's
    /// rest length (the distance when it grabbed, m); `side`: the side the target was on (-1
    /// left, 1 right).
    Holding {
        target: Entity,
        ticks: u32,
        rest: f32,
        side: f32,
    },
    /// The button is down but does nothing (pressed during the cooldown or when the kart could
    /// not act, a long press that cannot grab, held after a tether): it waits for the release.
    Spent,
}

/// The held side of a tether: who holds this kart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub by: Entity,
    /// The tether's rest length (m).
    pub rest: f32,
    /// Ticks held so far.
    pub ticks: u32,
    /// It jumped (or flew off a ramp): the tether breaks when it lands (GD § 15).
    pub escaping: bool,
}

/// What a paw did lately, for the visuals (the arm, the "pof") and the sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingKind {
    /// A strike that reached a kart.
    Strike,
    /// A strike into thin air.
    Whiff,
    /// A grab into thin air.
    GrabMiss,
    /// A throw at the end of a tether.
    Throw,
    /// Let go of a tether without a throw: the arm comes back from the kart it held.
    LetGo,
}

/// The last swing of a paw: what, toward which side (-1 left, 1 right), at whom, `ticks` ago.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    pub kind: SwingKind,
    pub side: f32,
    pub target: Option<Entity>,
    pub ticks: u32,
}

/// A kart's paw (GD § 15): its Interaction button, cooldown, tether, protection, and what the
/// visuals show. On every kart (`InteractionPlugin` makes it required by `Kart`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Paw {
    pub motion: PawMotion,
    /// The button on the last tick.
    pub button: bool,
    /// The side the paw aims at while the button is down (-1 left, 1 right; visual).
    pub aim: f32,
    /// Ticks before the button works again.
    pub cooldown: u32,
    /// Held by another kart's paw (one tether at a time).
    pub held: Option<Held>,
    /// Ticks of protection left against the paw strikes (GD § 15: 1 s after one).
    pub protected: u32,
    /// The last swing (visual).
    pub swing: Option<Swing>,
    /// Ticks left flailing off balance after a swing into thin air (visual), and toward which
    /// side.
    pub off_balance: u32,
    pub off_balance_side: f32,
    /// Strikes received so far (a new one shakes the camera), the last one `struck_ticks` ago,
    /// pushed toward `struck_side` (world direction).
    pub struck_count: u32,
    pub struck_ticks: u32,
    pub struck_side: Vec3,
}

impl Paw {
    /// The kart holding this one, or held by this one: its partner in a tether.
    pub fn partner(&self) -> Option<Entity> {
        match self.motion {
            PawMotion::Holding { target, .. } => Some(target),
            _ => self.held.map(|held| held.by),
        }
    }

    pub fn holding(&self) -> bool {
        matches!(self.motion, PawMotion::Holding { .. })
    }

    /// In a tether, either side.
    pub fn linked(&self) -> bool {
        self.partner().is_some()
    }

    /// The button works: no cooldown, nothing going on.
    pub fn ready(&self) -> bool {
        self.cooldown == 0 && self.motion == PawMotion::Idle
    }

    /// Off balance for `ticks`, toward `side`.
    pub fn lose_balance(&mut self, ticks: u32, side: f32) {
        self.off_balance = ticks;
        self.off_balance_side = side;
    }
}

/// What a kart's paw did, for the race logs, the tests and the telemetry.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PawStats {
    /// Paw strikes given (reaching a kart or into thin air), throws apart.
    pub strikes: u32,
    /// Strikes and throws the kart struck accepted.
    pub landed: u32,
    /// Strikes and grabs into thin air.
    pub missed: u32,
    /// Tethers started (the kart grabbed).
    pub grabs: u32,
    pub throws: u32,
    /// Strikes and throws received.
    pub struck: u32,
    /// Times held by another kart, and times it broke free by jumping.
    pub held: u32,
    pub escapes: u32,
}

/// Why a tether ends (docs/ARCHITECTURE.md, `FinAttrape`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TetherEnd {
    /// The grabber let go (button released, or it jumped: no throw).
    Released,
    /// The grabber let go steering: a throw (a strike follows).
    Thrown,
    /// The kart held jumped and landed.
    Escaped,
    /// `tether_max_time` is over.
    TimeUp,
    /// Stretched more than `tether_max_stretch`.
    Stretched,
    /// One of the two was hit by an item.
    Hit,
    /// One of the two was picked up by the pelican.
    Towed,
    /// One of the two crossed the finish line.
    Finished,
    /// The kart grabbed refused (immune, out of reach).
    Refused,
    /// The other kart is gone (on the network: disconnected).
    Gone,
}

/// Where a kart is, as another kart's paw sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KartView {
    pub entity: Entity,
    /// Where it is shown: what the paw reaches for (online, another peer's kart is shown about
    /// 100 ms in the past: what the player sees is what the paw strikes).
    pub position: Vec3,
    /// Where it is now: what a tether measures and pulls toward. The same as `position` for our
    /// own karts; online, another peer's kart extrapolated to the current tick (issue #9).
    pub now: Vec3,
    /// The tick of the state shown (`tick_vu`): the current one for our own karts.
    pub seen: u32,
    /// Its heading, horizontal (unit).
    pub forward: Vec3,
    pub velocity: Vec3,
    /// Its Weight level (0 to 20).
    pub weight: u8,
    /// A paw strike may reach it now ([`can_be_struck`], and not protected).
    pub strikable: bool,
    /// A paw may grab it now ([`can_be_grabbed`]).
    pub grabbable: bool,
}

/// How a kart is within reach of another one's paw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InReach {
    /// Just ahead (grab only), rather than to the side.
    pub ahead: bool,
    /// Its center to the right (> 0) or left (< 0) of the kart's axis (m).
    pub lateral: f32,
    /// Center to center, horizontally (m).
    pub distance: f32,
}

/// Whether `other` is within the paw's reach of a kart at `position` heading `forward` (GD § 15,
/// in the kart's frame, to the other kart's center): to the side, or just ahead.
pub fn reach(
    tuning: &InteractionTuning,
    position: Vec3,
    forward: Vec3,
    other: Vec3,
) -> Option<InReach> {
    let forward = forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y);
    let to = other - position;
    if to.y.abs() > tuning.max_height_difference {
        return None;
    }
    let (lateral, along) = (to.dot(right), to.dot(forward));
    let distance = to.with_y(0.0).length();
    let side = (tuning.side_min_lateral..=tuning.side_max_lateral).contains(&lateral.abs())
        && along.abs() <= tuning.side_max_longitudinal;
    // Between the front of the kart and the back of the target (both 2 m long).
    let gap = along - 2.0 * KART_HALF_EXTENTS.z;
    let ahead =
        along > 0.0 && gap <= tuning.front_max_gap && lateral.abs() <= tuning.front_max_lateral;
    if side {
        Some(InReach {
            ahead: false,
            lateral,
            distance,
        })
    } else if ahead {
        Some(InReach {
            ahead: true,
            lateral,
            distance,
        })
    } else {
        None
    }
}

/// The farthest a kart in reach can be, center to center (m): what the kart struck or grabbed
/// checks, plus `reach_tolerance` (docs/ARCHITECTURE.md: the event was judged on the other
/// peer's screen).
pub fn max_reach(tuning: &InteractionTuning) -> f32 {
    let side = Vec2::new(tuning.side_max_lateral, tuning.side_max_longitudinal).length();
    let ahead = Vec2::new(
        tuning.front_max_lateral,
        2.0 * KART_HALF_EXTENTS.z + tuning.front_max_gap,
    )
    .length();
    side.max(ahead)
}

/// Whether the kart struck or grabbed accepts, at tick `now`, an event sent at tick `sent` by a
/// kart that was at `from` (docs/ARCHITECTURE.md): not too old, and in reach of `position` with
/// some tolerance. `position`: where the kart was at the tick the striker saw (`tick_vu`; online,
/// from its history, issue #9). Solo, the event is judged on this very tick, where it is.
pub fn accepts(
    tuning: &InteractionTuning,
    now: u32,
    sent: u32,
    from: Vec3,
    position: Vec3,
) -> bool {
    let to = position - from;
    now.saturating_sub(sent) <= ticks(tuning.max_event_age)
        && to.with_y(0.0).length() <= max_reach(tuning) + tuning.reach_tolerance
        && to.y.abs() <= tuning.max_height_difference + tuning.reach_tolerance
}

/// The side a kart aims at (-1 left, 1 right) when there are karts on both sides: the side it
/// steers toward by at least `side_steer`, or in a drift the drift's inner side (GD § 15).
pub fn aimed_side(tuning: &InteractionTuning, steer: f32, drift: Option<DriftSide>) -> Option<f32> {
    match drift {
        Some(side) => Some(side.sign()),
        None if steer.abs() >= tuning.side_steer => Some(steer.signum()),
        None => None,
    }
}

/// The kart a paw goes for, among `karts`, from a kart (`me`) at `position` heading `forward`:
/// the closest one in reach that may be struck (`grab`: grabbed), center to center. A paw strike
/// only reaches to the side; a grab also just ahead. With karts in reach on both sides, the side
/// aimed at ([`aimed_side`]) wins. An immune kart is never picked: the paw goes to the next one.
pub fn pick_target(
    tuning: &InteractionTuning,
    me: Entity,
    position: Vec3,
    forward: Vec3,
    aimed: Option<f32>,
    grab: bool,
    karts: &[KartView],
) -> Option<(KartView, InReach)> {
    let candidates = || {
        karts
            .iter()
            .filter(move |k| k.entity != me && if grab { k.grabbable } else { k.strikable })
            .filter_map(move |k| {
                reach(tuning, position, forward, k.position)
                    .filter(|r| grab || !r.ahead)
                    .map(|r| (*k, r))
            })
    };
    let on = |sign: f32| candidates().any(|(_, r)| !r.ahead && r.lateral.signum() == sign);
    let only = match aimed {
        Some(side) if on(1.0) && on(-1.0) => Some(side),
        _ => None,
    };
    candidates()
        .filter(|(_, r)| r.ahead || only.is_none_or(|side| r.lateral.signum() == side))
        .min_by(|(a, ra), (b, rb)| {
            ra.distance
                .total_cmp(&rb.distance)
                .then(a.entity.cmp(&b.entity))
        })
}

/// GD § 15, "Peut frapper": on the ground, in a hop or a drift; not in the air or a trick, not hit
/// by an item, not carried by the pelican, not while the engine stalls, before GO or after the
/// finish. An invincible kart may strike.
pub fn can_strike(state: &KartState, stalled: bool, racing: bool, finished: bool) -> bool {
    racing
        && !finished
        && !stalled
        && matches!(
            state,
            KartState::Grounded | KartState::Hop { .. } | KartState::Drifting { .. }
        )
}

/// GD § 15, "Peut attraper": on the ground only (neither in a hop nor in a drift), not already in
/// a tether (`linked`), and as [`can_strike`].
pub fn can_grab(
    state: &KartState,
    stalled: bool,
    racing: bool,
    finished: bool,
    linked: bool,
) -> bool {
    can_strike(state, stalled, racing, finished) && !linked && *state == KartState::Grounded
}

/// GD § 15, "Peut recevoir un coup": on the ground, in a hop or a drift (stalled too); not in the
/// air or a trick, not hit by an item, not carried by the pelican, not invincible, not before GO
/// or after the finish. In a tether too (the tether holds). The 1 s of protection after a strike
/// is the caller's.
pub fn can_be_struck(state: &KartState, invincible: bool, racing: bool, finished: bool) -> bool {
    racing
        && !finished
        && !invincible
        && matches!(
            state,
            KartState::Grounded | KartState::Hop { .. } | KartState::Drifting { .. }
        )
}

/// GD § 15, "Peut être attrapé": on the ground only (not in a hop or a drift: that is how one
/// breaks free), not already in a tether (`linked`), and as [`can_be_struck`].
pub fn can_be_grabbed(
    state: &KartState,
    invincible: bool,
    racing: bool,
    finished: bool,
    linked: bool,
) -> bool {
    can_be_struck(state, invincible, racing, finished) && !linked && *state == KartState::Grounded
}

/// What the paw of a kart sees and may do this tick.
#[derive(Clone, Copy, Debug)]
pub struct PawContext {
    pub entity: Entity,
    pub position: Vec3,
    /// Its heading, horizontal.
    pub forward: Vec3,
    pub velocity: Vec3,
    /// The Interaction button now, and the stick.
    pub button: bool,
    pub steer: f32,
    /// The drift's side, in a drift.
    pub drift: Option<DriftSide>,
    /// [`can_strike`], [`can_grab`].
    pub can_strike: bool,
    pub can_grab: bool,
    /// On the ground (`KartState::Grounded`): a grabber that leaves it (a hop) lets go.
    pub grounded: bool,
    /// Hit by an item, carried by the pelican, past the finish line: a tether breaks.
    pub hit: bool,
    pub towed: bool,
    pub finished: bool,
}

/// What a paw does this tick ([`step_paw`]); the rules carry it out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PawOutcome {
    None,
    /// A paw strike, toward `side` (-1 left, 1 right): at `target`, or into thin air.
    Strike {
        target: Option<Entity>,
        side: f32,
    },
    /// Grabbed `target`, `rest` m away, on `side`.
    Grab {
        target: Entity,
        rest: f32,
        side: f32,
    },
    /// A grab into thin air, toward `side`.
    GrabMiss {
        side: f32,
    },
    /// The grabber ends its tether with `target`, for `reason`; a throw toward `throw` (-1 left,
    /// 1 right) when it let go steering.
    LetGo {
        target: Entity,
        reason: TetherEnd,
        throw: Option<f32>,
    },
}

/// One tick of a kart's paw: the Interaction button, the reach for a grab, the tether's end on
/// the grabber's side. `karts`: every kart as the paw sees it (its own included, skipped). The
/// paw's state moves on here; the cooldowns of a strike and of a grab into thin air too (the end
/// of a tether, either side's, is `end_tether`).
pub fn step_paw(
    paw: &mut Paw,
    tuning: &InteractionTuning,
    ctx: &PawContext,
    karts: &[KartView],
) -> PawOutcome {
    let pressed = ctx.button && !paw.button;
    let released = !ctx.button && paw.button;
    paw.button = ctx.button;
    let aimed = aimed_side(tuning, ctx.steer, ctx.drift);
    if ctx.button {
        paw.aim = aimed.unwrap_or(1.0);
    }
    // After the action, the button waits for its release if it is still down.
    let after = if ctx.button {
        PawMotion::Spent
    } else {
        PawMotion::Idle
    };
    match paw.motion {
        PawMotion::Idle => {
            if pressed {
                paw.motion = if paw.cooldown == 0 && ctx.can_strike {
                    PawMotion::Raised { ticks: 1 }
                } else {
                    PawMotion::Spent
                };
            }
            PawOutcome::None
        }
        PawMotion::Spent => {
            if !ctx.button {
                paw.motion = PawMotion::Idle;
            }
            PawOutcome::None
        }
        PawMotion::Raised { ticks: held } => {
            if released {
                paw.motion = PawMotion::Idle;
                if !ctx.can_strike {
                    return PawOutcome::None;
                }
                paw.cooldown = ticks(tuning.strike_cooldown);
                let target = pick_target(
                    tuning,
                    ctx.entity,
                    ctx.position,
                    ctx.forward,
                    aimed,
                    false,
                    karts,
                );
                return match target {
                    Some((kart, reach)) => PawOutcome::Strike {
                        target: Some(kart.entity),
                        side: reach.lateral.signum(),
                    },
                    None => PawOutcome::Strike {
                        target: None,
                        side: aimed.unwrap_or(1.0),
                    },
                };
            }
            if held + 1 < ticks(tuning.tap_time) {
                paw.motion = PawMotion::Raised { ticks: held + 1 };
                return PawOutcome::None;
            }
            // Held long enough: a grab, if the kart may grab (not in a drift or a hop).
            if !ctx.can_grab {
                paw.motion = PawMotion::Spent;
                return PawOutcome::None;
            }
            paw.motion = PawMotion::Reaching { ticks: 0 };
            reach_out(paw, tuning, ctx, aimed, released, karts, after)
        }
        PawMotion::Reaching { .. } => {
            if ctx.hit || ctx.towed || ctx.finished {
                // Hit, towed or past the line: no grab, no whiff either.
                paw.motion = after;
                return PawOutcome::None;
            }
            if !ctx.can_grab {
                // Hopped, drifted or took off while reaching: a grab into thin air.
                paw.motion = after;
                paw.cooldown = ticks(tuning.strike_cooldown);
                return PawOutcome::GrabMiss {
                    side: aimed.unwrap_or(1.0),
                };
            }
            reach_out(paw, tuning, ctx, aimed, released, karts, after)
        }
        PawMotion::Holding {
            target,
            ticks: held,
            rest,
            side,
        } => {
            let let_go = |paw: &mut Paw, reason, throw| {
                paw.motion = after;
                paw.cooldown = ticks(tuning.tether_cooldown);
                PawOutcome::LetGo {
                    target,
                    reason,
                    throw,
                }
            };
            let Some(partner) = karts.iter().find(|k| k.entity == target) else {
                return let_go(paw, TetherEnd::Gone, None);
            };
            if ctx.hit {
                return let_go(paw, TetherEnd::Hit, None);
            }
            if ctx.towed {
                return let_go(paw, TetherEnd::Towed, None);
            }
            if ctx.finished {
                return let_go(paw, TetherEnd::Finished, None);
            }
            if !ctx.grounded {
                // It jumped: it lets go, without a throw (GD § 15).
                return let_go(paw, TetherEnd::Released, None);
            }
            let held = held + 1;
            let time_up = held >= ticks(tuning.tether_max_time);
            if released || time_up {
                let throw = (held >= ticks(tuning.throw_min_hold)
                    && ctx.steer.abs() >= tuning.throw_steer)
                    .then_some(ctx.steer.signum());
                let reason = match (throw, time_up) {
                    (Some(_), _) => TetherEnd::Thrown,
                    (None, true) => TetherEnd::TimeUp,
                    (None, false) => TetherEnd::Released,
                };
                return let_go(paw, reason, throw);
            }
            let stretch = (partner.now - ctx.position).with_y(0.0).length() - rest;
            if stretch > tuning.tether_max_stretch {
                return let_go(paw, TetherEnd::Stretched, None);
            }
            paw.motion = PawMotion::Holding {
                target,
                ticks: held,
                rest,
                side,
            };
            PawOutcome::None
        }
    }
}

/// One tick of the paw reaching out (`PawMotion::Reaching`): grabs the closest kart in reach, or
/// waits for one until the window is over or the button released (a grab into thin air).
fn reach_out(
    paw: &mut Paw,
    tuning: &InteractionTuning,
    ctx: &PawContext,
    aimed: Option<f32>,
    released: bool,
    karts: &[KartView],
    after: PawMotion,
) -> PawOutcome {
    let PawMotion::Reaching { ticks: reaching } = paw.motion else {
        return PawOutcome::None;
    };
    if !released
        && let Some((kart, reach)) = pick_target(
            tuning,
            ctx.entity,
            ctx.position,
            ctx.forward,
            aimed,
            true,
            karts,
        )
    {
        let side = reach.lateral.signum();
        // The tether's rest length, to where the kart is now (online, not where it is shown):
        // each side pulls toward the other one's position now.
        let rest = (kart.now - ctx.position).with_y(0.0).length();
        paw.motion = PawMotion::Holding {
            target: kart.entity,
            ticks: 0,
            rest,
            side,
        };
        return PawOutcome::Grab {
            target: kart.entity,
            rest,
            side,
        };
    }
    let reaching = reaching + 1;
    if released || reaching >= ticks(tuning.grab_window) {
        paw.motion = after;
        paw.cooldown = ticks(tuning.strike_cooldown);
        return PawOutcome::GrabMiss {
            side: aimed.unwrap_or(1.0),
        };
    }
    paw.motion = PawMotion::Reaching { ticks: reaching };
    PawOutcome::None
}

/// The held side of a tether, one tick: whether it ends for this kart (GD § 15). It breaks free by
/// jumping (or flying off a ramp): the tether breaks when it lands. Hit by an item, picked up by
/// the pelican or past the finish line, it ends at once. Never longer than `tether_max_time`
/// (and a tick of margin: the grabber ends it first, solo).
pub fn held_end(
    held: &mut Held,
    tuning: &InteractionTuning,
    state: &KartState,
    finished: bool,
) -> Option<TetherEnd> {
    held.ticks += 1;
    match state {
        KartState::Hit { .. } => return Some(TetherEnd::Hit),
        KartState::Respawning => return Some(TetherEnd::Towed),
        _ if finished => return Some(TetherEnd::Finished),
        KartState::Hop { .. } | KartState::Airborne | KartState::Trick => held.escaping = true,
        _ if held.escaping => return Some(TetherEnd::Escaped),
        _ => {}
    }
    (held.ticks > ticks(tuning.tether_max_time) + 1).then_some(TetherEnd::TimeUp)
}

/// The tether's pull on a kart at `position` (velocity `velocity`) toward its partner at `other`
/// (`other_velocity`): a damped spring along the horizontal line between them that only pulls,
/// `stiffness` × (stretch past `rest`) + `damping` × (speed at which they move apart) (m/s²,
/// world). The grabber's share, before its Weight; see [`held_accel`] for the kart held.
pub fn tether_accel(
    tuning: &InteractionTuning,
    position: Vec3,
    velocity: Vec3,
    other: Vec3,
    other_velocity: Vec3,
    rest: f32,
) -> Vec3 {
    let to = (other - position).with_y(0.0);
    let distance = to.length();
    if distance < 1e-3 {
        return Vec3::ZERO;
    }
    let toward = to / distance;
    let apart = (other_velocity - velocity).with_y(0.0).dot(toward);
    let pull = tuning.tether_stiffness * (distance - rest) + tuning.tether_damping * apart;
    toward * pull.max(0.0)
}

/// The kart held gets `tether_target_share` of the tether's pull `accel` (GD § 15: 25 %), only what
/// holds it back or pulls it sideways, never forward along its heading `forward`.
pub fn held_accel(tuning: &InteractionTuning, accel: Vec3, forward: Vec3) -> Vec3 {
    let forward = forward.with_y(0.0).normalize_or_zero();
    let share = accel * tuning.tether_target_share;
    share - forward * share.dot(forward).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> InteractionTuning {
        InteractionTuning::default()
    }

    fn entity(index: u32) -> Entity {
        Entity::from_raw_u32(index).unwrap()
    }

    /// A kart at `(x, z)` heading -Z, free to be struck and grabbed.
    fn kart(index: u32, x: f32, z: f32) -> KartView {
        KartView {
            entity: entity(index),
            position: Vec3::new(x, 0.55, z),
            now: Vec3::new(x, 0.55, z),
            seen: 0,
            forward: Vec3::NEG_Z,
            velocity: Vec3::new(0.0, 0.0, -25.0),
            weight: 10,
            strikable: true,
            grabbable: true,
        }
    }

    fn me() -> KartView {
        kart(1, 0.0, 0.0)
    }

    fn reach_of(x: f32, z: f32) -> Option<InReach> {
        let me = me();
        reach(&tuning(), me.position, me.forward, Vec3::new(x, 0.55, z))
    }

    /// GD § 15: to the side, 0.7 to 3.9 m from the axis, ±2.5 m along it, 1.5 m of height at most;
    /// just ahead, up to 2.5 m between the front and the target's back, 1.4 m to the side.
    #[test]
    fn the_paw_reaches_to_the_side_and_just_ahead() {
        // Heading -Z: +X is the right.
        let right = reach_of(2.0, 0.0).unwrap();
        assert!(!right.ahead && right.lateral > 1.9 && (right.distance - 2.0).abs() < 1e-5);
        assert!(reach_of(-3.9, 2.5).is_some_and(|r| !r.ahead && r.lateral < 0.0));
        assert!(reach_of(0.7, -2.5).is_some());
        for (x, z) in [(4.0, 0.0), (2.0, 2.6), (-2.0, -2.6), (0.5, 0.0), (0.0, 5.0)] {
            assert_eq!(reach_of(x, z), None, "({x}, {z})");
        }
        // Just ahead: the front is 1 m ahead of the center, the target's back 1 m behind its.
        let ahead = reach_of(0.3, -4.5).unwrap();
        assert!(ahead.ahead);
        assert!(reach_of(1.4, -3.0).is_some_and(|r| r.ahead));
        assert_eq!(reach_of(0.0, -4.6), None);
        assert_eq!(reach_of(1.5, -3.0), None);
        // Behind is not ahead.
        assert_eq!(reach_of(0.0, 3.0), None);
        // Too high or too low.
        let me = me();
        assert_eq!(
            reach(&tuning(), me.position, me.forward, Vec3::new(2.0, 2.1, 0.0)),
            None
        );
        // The frame of the kart: heading +X, its right is +Z.
        let turned = reach(&tuning(), Vec3::ZERO, Vec3::X, Vec3::new(0.0, 0.0, 2.0)).unwrap();
        assert!(turned.lateral > 1.9);
        assert!(max_reach(&tuning()) > 4.6 && max_reach(&tuning()) < 4.8);
    }

    /// GD § 15: the closest kart in reach; with karts on both sides, the side steered toward
    /// (at least 0.3), in a drift its inner side; an immune kart is skipped; a strike never goes
    /// ahead, a grab does.
    #[test]
    fn the_target_is_the_closest_kart_on_the_side_aimed_at() {
        let t = tuning();
        let me = me();
        let left = kart(2, -1.8, 0.0);
        let right = kart(3, 2.5, 0.5);
        let far = kart(4, 8.0, 0.0);
        let karts = [me, left, right, far];
        let pick = |aimed, grab, karts: &[KartView]| {
            pick_target(&t, me.entity, me.position, me.forward, aimed, grab, karts)
                .map(|(k, _)| k.entity)
        };
        assert_eq!(pick(None, false, &karts), Some(left.entity), "closest");
        assert_eq!(
            pick(Some(1.0), false, &karts),
            Some(right.entity),
            "steered right"
        );
        assert_eq!(pick(Some(-1.0), false, &karts), Some(left.entity));
        assert_eq!(aimed_side(&t, 0.2, None), None);
        assert_eq!(aimed_side(&t, 0.3, None), Some(1.0));
        assert_eq!(aimed_side(&t, -0.9, None), Some(-1.0));
        assert_eq!(aimed_side(&t, 1.0, Some(DriftSide::Left)), Some(-1.0));
        // One side only: the steering does not matter.
        assert_eq!(
            pick(Some(-1.0), false, &[me, right, far]),
            Some(right.entity)
        );
        // Immune: the next one.
        let protected = KartView {
            strikable: false,
            ..left
        };
        assert_eq!(
            pick(None, false, &[me, protected, right]),
            Some(right.entity)
        );
        assert_eq!(pick(None, false, &[me, protected, far]), None);
        // Just ahead: grabbed, not struck.
        let ahead = kart(5, 0.0, -3.5);
        assert_eq!(pick(None, false, &[me, ahead]), None);
        assert_eq!(pick(None, true, &[me, ahead]), Some(ahead.entity));
        let busy = KartView {
            grabbable: false,
            ..ahead
        };
        assert_eq!(pick(None, true, &[me, busy, far]), None);
    }

    /// The table of GD § 15.
    #[test]
    fn the_immunities_follow_the_table_of_the_game_design() {
        use crate::kart::{HitReaction, KartState as S};
        let hop = S::Hop {
            ticks: 3,
            side: None,
        };
        let drift = S::Drifting {
            side: DriftSide::Left,
            charge: 100.0,
        };
        let hit = S::hit(HitReaction::Spin, 1.0, 60.0);
        // (state, strike, grab, struck, grabbed)
        let table = [
            (S::Grounded, true, true, true, true),
            (hop, true, false, true, false),
            (drift, true, false, true, false),
            (S::Airborne, false, false, false, false),
            (S::Trick, false, false, false, false),
            (hit, false, false, false, false),
            (S::Respawning, false, false, false, false),
        ];
        for (state, strike, grab, struck, grabbed) in table {
            assert_eq!(can_strike(&state, false, true, false), strike, "{state:?}");
            assert_eq!(
                can_grab(&state, false, true, false, false),
                grab,
                "{state:?}"
            );
            assert_eq!(
                can_be_struck(&state, false, true, false),
                struck,
                "{state:?}"
            );
            assert_eq!(
                can_be_grabbed(&state, false, true, false, false),
                grabbed,
                "{state:?}"
            );
        }
        let g = S::Grounded;
        // Invincible: strikes and grabs, cannot be struck nor grabbed.
        assert!(can_strike(&g, false, true, false) && can_grab(&g, false, true, false, false));
        assert!(
            !can_be_struck(&g, true, true, false) && !can_be_grabbed(&g, true, true, false, false)
        );
        // Stalled at the start: neither strikes nor grabs, can be struck and grabbed.
        assert!(!can_strike(&g, true, true, false) && !can_grab(&g, true, true, false, false));
        assert!(
            can_be_struck(&g, false, true, false) && can_be_grabbed(&g, false, true, false, false)
        );
        // Before GO, after the finish: nothing.
        for (racing, finished) in [(false, false), (true, true)] {
            assert!(!can_strike(&g, false, racing, finished));
            assert!(!can_grab(&g, false, racing, finished, false));
            assert!(!can_be_struck(&g, false, racing, finished));
            assert!(!can_be_grabbed(&g, false, racing, finished, false));
        }
        // One tether at a time.
        assert!(!can_grab(&g, false, true, false, true));
        assert!(!can_be_grabbed(&g, false, true, false, true));
    }

    fn context(button: bool) -> PawContext {
        let me = me();
        PawContext {
            entity: me.entity,
            position: me.position,
            forward: me.forward,
            velocity: me.velocity,
            button,
            steer: 0.0,
            drift: None,
            can_strike: true,
            can_grab: true,
            grounded: true,
            hit: false,
            towed: false,
            finished: false,
        }
    }

    /// Runs `ticks` ticks of the button held (or not), returns the outcomes that happened.
    fn run(paw: &mut Paw, ctx: PawContext, ticks: usize, karts: &[KartView]) -> Vec<PawOutcome> {
        (0..ticks)
            .map(|_| step_paw(paw, &tuning(), &ctx, karts))
            .filter(|o| *o != PawOutcome::None)
            .collect()
    }

    /// GD § 15: a short press strikes on the release (the paw raised meanwhile), then 1.5 s of
    /// cooldown during which the button does nothing.
    #[test]
    fn a_short_press_strikes_on_the_release_then_the_button_rests() {
        let t = tuning();
        let right = kart(2, 2.0, 0.0);
        let karts = [me(), right];
        let mut paw = Paw::default();
        assert!(run(&mut paw, context(true), 11, &karts).is_empty());
        assert_eq!(paw.motion, PawMotion::Raised { ticks: 11 });
        assert_eq!(
            run(&mut paw, context(false), 1, &karts),
            [PawOutcome::Strike {
                target: Some(right.entity),
                side: 1.0
            }]
        );
        assert_eq!(paw.cooldown, ticks(t.strike_cooldown));
        assert_eq!(paw.cooldown, 90);
        // During the cooldown: nothing, even released and pressed again.
        assert!(run(&mut paw, context(true), 3, &karts).is_empty());
        assert!(run(&mut paw, context(false), 1, &karts).is_empty());
        assert_eq!(paw.motion, PawMotion::Idle);
        // Into thin air: toward the side steered, to the right in neutral.
        let mut paw = Paw::default();
        run(&mut paw, context(true), 2, &[me()]);
        assert_eq!(
            run(&mut paw, context(false), 1, &[me()]),
            [PawOutcome::Strike {
                target: None,
                side: 1.0
            }]
        );
        let mut paw = Paw::default();
        let left = PawContext {
            steer: -0.5,
            ..context(true)
        };
        run(&mut paw, left, 2, &[me()]);
        assert_eq!(
            run(
                &mut paw,
                PawContext {
                    button: false,
                    ..left
                },
                1,
                &[me()]
            ),
            [PawOutcome::Strike {
                target: None,
                side: -1.0
            }]
        );
        // A press when the kart may not strike (in the air, stalled...): nothing until released.
        let mut paw = Paw::default();
        let grounded_later = PawContext {
            can_strike: false,
            can_grab: false,
            ..context(true)
        };
        assert!(run(&mut paw, grounded_later, 3, &karts).is_empty());
        assert_eq!(paw.motion, PawMotion::Spent);
        assert!(run(&mut paw, context(true), 20, &karts).is_empty());
        assert!(run(&mut paw, context(false), 1, &karts).is_empty());
        assert_eq!((paw.motion, paw.cooldown), (PawMotion::Idle, 0));
    }

    /// GD § 15: held past 0.2 s, the paw reaches out for 0.5 s and grabs the first kart in reach;
    /// nobody: a grab into thin air and 1.5 s of cooldown.
    #[test]
    fn a_long_press_grabs_the_first_kart_in_reach_or_claws_thin_air() {
        let t = tuning();
        let right = kart(2, 2.0, 0.0);
        let mut paw = Paw::default();
        // Nobody at first: 12 ticks raised, then reaching.
        assert!(run(&mut paw, context(true), 20, &[me()]).is_empty());
        assert_eq!(paw.motion, PawMotion::Reaching { ticks: 9 });
        // A kart comes in reach.
        assert_eq!(
            run(&mut paw, context(true), 1, &[me(), right]),
            [PawOutcome::Grab {
                target: right.entity,
                rest: 2.0,
                side: 1.0
            }]
        );
        assert!(paw.holding() && paw.partner() == Some(right.entity));

        let mut paw = Paw::default();
        let outcomes = run(&mut paw, context(true), 60, &[me()]);
        assert_eq!(outcomes, [PawOutcome::GrabMiss { side: 1.0 }]);
        assert_eq!(paw.cooldown, ticks(t.strike_cooldown));
        assert_eq!(paw.motion, PawMotion::Spent, "held: waits for the release");
        // Released early: a grab into thin air too.
        let mut paw = Paw::default();
        run(&mut paw, context(true), 15, &[me()]);
        assert_eq!(
            run(&mut paw, context(false), 1, &[me()]),
            [PawOutcome::GrabMiss { side: 1.0 }]
        );
        // No grab in a drift or a hop: a long press does nothing (no cooldown).
        let mut paw = Paw::default();
        let drifting = PawContext {
            can_grab: false,
            drift: Some(DriftSide::Right),
            ..context(true)
        };
        assert!(run(&mut paw, drifting, 30, &[me(), right]).is_empty());
        assert_eq!((paw.motion, paw.cooldown), (PawMotion::Spent, 0));
    }

    fn holding(rest: f32) -> Paw {
        Paw {
            motion: PawMotion::Holding {
                target: entity(2),
                ticks: 0,
                rest,
                side: 1.0,
            },
            button: true,
            ..default()
        }
    }

    /// GD § 15: a tether lasts while the button is held, 2 s at most; letting go steering hard
    /// after 0.5 s throws; it breaks when stretched 2.5 m, when the grabber hops, is hit, towed
    /// or finishes; 3 s of cooldown after it.
    #[test]
    fn a_tether_lasts_two_seconds_at_most_and_ends_with_a_throw_when_steering() {
        let t = tuning();
        let right = kart(2, 2.0, 0.0);
        let karts = [me(), right];
        let let_go = |reason, throw| PawOutcome::LetGo {
            target: right.entity,
            reason,
            throw,
        };
        // Held: 2 s exactly.
        let mut paw = holding(2.0);
        let outcomes = run(&mut paw, context(true), 120, &karts);
        assert_eq!(outcomes, [let_go(TetherEnd::TimeUp, None)]);
        assert_eq!(paw.cooldown, ticks(t.tether_cooldown));
        assert_eq!(paw.motion, PawMotion::Spent);
        let mut paw = holding(2.0);
        assert!(run(&mut paw, context(true), 119, &karts).is_empty());
        // Released steering hard after 0.5 s: thrown that way.
        let mut paw = holding(2.0);
        run(&mut paw, context(true), 30, &karts);
        let steering = PawContext {
            steer: -0.6,
            ..context(false)
        };
        assert_eq!(
            run(&mut paw, steering, 1, &karts),
            [let_go(TetherEnd::Thrown, Some(-1.0))]
        );
        // Too soon, or steering softly: just released.
        let mut paw = holding(2.0);
        run(&mut paw, context(true), 20, &karts);
        assert_eq!(
            run(&mut paw, steering, 1, &karts),
            [let_go(TetherEnd::Released, None)]
        );
        let mut paw = holding(2.0);
        run(&mut paw, context(true), 40, &karts);
        let softly = PawContext {
            steer: 0.4,
            ..context(false)
        };
        assert_eq!(
            run(&mut paw, softly, 1, &karts),
            [let_go(TetherEnd::Released, None)]
        );
        // Time up while steering hard: a throw too.
        let mut paw = holding(2.0);
        let hard = PawContext {
            steer: 1.0,
            ..context(true)
        };
        assert_eq!(
            run(&mut paw, hard, 120, &karts),
            [let_go(TetherEnd::Thrown, Some(1.0))]
        );
        // Stretched: rest 2 m, the partner 4.6 m away.
        let mut paw = holding(2.0);
        let away = kart(2, 4.6, 0.0);
        assert_eq!(
            run(&mut paw, context(true), 1, &[me(), away]),
            [let_go(TetherEnd::Stretched, None)]
        );
        let mut paw = holding(2.0);
        assert!(run(&mut paw, context(true), 1, &[me(), kart(2, 4.4, 0.0)]).is_empty());
        // The grabber hops, is hit, towed, finishes; the partner is gone.
        for (ctx, reason) in [
            (
                PawContext {
                    grounded: false,
                    can_grab: false,
                    ..context(true)
                },
                TetherEnd::Released,
            ),
            (
                PawContext {
                    hit: true,
                    ..context(true)
                },
                TetherEnd::Hit,
            ),
            (
                PawContext {
                    towed: true,
                    ..context(true)
                },
                TetherEnd::Towed,
            ),
            (
                PawContext {
                    finished: true,
                    ..context(true)
                },
                TetherEnd::Finished,
            ),
        ] {
            let mut paw = holding(2.0);
            assert_eq!(run(&mut paw, ctx, 1, &karts), [let_go(reason, None)]);
        }
        let mut paw = holding(2.0);
        assert_eq!(
            run(&mut paw, context(true), 1, &[me()]),
            [let_go(TetherEnd::Gone, None)]
        );
    }

    /// GD § 15: the kart held breaks free by jumping, the tether breaking when it lands (not on
    /// the press); hit, towed or past the line, at once.
    #[test]
    fn the_kart_held_breaks_free_when_it_lands_its_jump() {
        let t = tuning();
        let mut held = Held {
            by: entity(1),
            rest: 2.0,
            ticks: 0,
            escaping: false,
        };
        assert_eq!(held_end(&mut held, &t, &KartState::Grounded, false), None);
        let hop = KartState::Hop {
            ticks: 1,
            side: None,
        };
        for _ in 0..20 {
            assert_eq!(held_end(&mut held, &t, &hop, false), None);
        }
        assert!(held.escaping);
        assert_eq!(
            held_end(&mut held, &t, &KartState::Grounded, false),
            Some(TetherEnd::Escaped)
        );
        let fresh = || Held {
            by: entity(1),
            rest: 2.0,
            ticks: 0,
            escaping: false,
        };
        let hit = KartState::hit(crate::kart::HitReaction::Spin, 1.0, 60.0);
        assert_eq!(
            held_end(&mut fresh(), &t, &hit, false),
            Some(TetherEnd::Hit)
        );
        assert_eq!(
            held_end(&mut fresh(), &t, &KartState::Respawning, false),
            Some(TetherEnd::Towed)
        );
        assert_eq!(
            held_end(&mut fresh(), &t, &KartState::Grounded, true),
            Some(TetherEnd::Finished)
        );
        // A drift landing from the hop breaks it too.
        let mut held = fresh();
        held_end(&mut held, &t, &hop, false);
        let drift = KartState::Drifting {
            side: DriftSide::Left,
            charge: 5.0,
        };
        assert_eq!(
            held_end(&mut held, &t, &drift, false),
            Some(TetherEnd::Escaped)
        );
        // Never longer than 2 s (and a tick of margin).
        let mut held = fresh();
        let ends: Vec<_> = (0..125)
            .filter_map(|_| held_end(&mut held, &t, &KartState::Grounded, false))
            .collect();
        assert_eq!(ends.first(), Some(&TetherEnd::TimeUp));
        assert_eq!(held.ticks, 125);
    }

    /// The tether only pulls: a damped spring along the line between the karts; the kart held
    /// gets a quarter of it, never forward.
    #[test]
    fn the_tether_pulls_toward_the_other_kart_and_never_pushes() {
        let t = tuning();
        let v = Vec3::new(0.0, 0.0, -20.0);
        // Stretched 1 m to the right, same speeds: stiffness × 1 toward the other kart.
        let pull = tether_accel(&t, Vec3::ZERO, v, Vec3::new(3.0, 0.0, 0.0), v, 2.0);
        assert!(
            (pull - Vec3::X * t.tether_stiffness).length() < 1e-4,
            "{pull}"
        );
        // Shorter than its rest length, coming together: no push.
        let none = tether_accel(&t, Vec3::ZERO, v, Vec3::new(1.5, 0.0, 0.0), v, 2.0);
        assert_eq!(none, Vec3::ZERO);
        // At rest length, the other kart drives away ahead: the damping pulls the grabber on.
        let ahead = tether_accel(&t, Vec3::ZERO, v, Vec3::new(0.0, 0.0, -2.0), v * 1.3, 2.0);
        assert!(ahead.z < -50.0, "{ahead}");
        // The kart held: a quarter, what holds it back only.
        let back = held_accel(&t, Vec3::new(0.0, 0.0, 40.0), Vec3::NEG_Z);
        assert!((back - Vec3::new(0.0, 0.0, 10.0)).length() < 1e-4);
        let forward = held_accel(&t, Vec3::new(8.0, 0.0, -40.0), Vec3::NEG_Z);
        assert!(
            (forward - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4,
            "{forward}"
        );
    }

    /// docs/ARCHITECTURE.md: the kart struck or grabbed checks the event: in reach (with 0.75 m
    /// of tolerance), at most 0.3 s old.
    #[test]
    fn the_kart_struck_checks_the_reach_and_the_age_of_the_event() {
        let t = tuning();
        let at = Vec3::new(0.0, 0.55, 0.0);
        assert!(accepts(&t, 100, 100, at + Vec3::X * 3.9, at));
        assert!(accepts(&t, 100, 82, at + Vec3::X * 5.4, at));
        assert!(!accepts(&t, 100, 81, at + Vec3::X * 2.0, at), "too old");
        assert!(!accepts(&t, 100, 100, at + Vec3::X * 5.6, at), "too far");
        assert!(!accepts(&t, 100, 100, at + Vec3::Y * 2.4, at), "too high");
    }
}
