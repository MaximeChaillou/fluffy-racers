//! Interactions between karts (GD § 15, issue #34): from its kart, the character strikes a
//! neighbour with its paw to push it off its line, or grabs it to get towed.
//!
//! - **Button** (`KartInput::interact`: X or F, RB on a gamepad): a short press strikes on the
//!   release, a longer one grabs and holds while the button stays down (`logic.rs`,
//!   [`step_paw`]). One cooldown: 1.5 s after a strike or a grab into thin air, 3 s after a
//!   tether.
//! - **Paw strike**: the closest kart in reach to the side ([`pick_target`]) is pushed sideways
//!   (7 m/s fading out in 0.4 s, about 1.4 m: the kart's `Shove`, which neither the grip nor the
//!   drift wipes out, and which a wall stops), loses 10 % of its speed and 1 croquette
//!   (`LossCause::Strike`), then is protected from the strikes for 1 s. It keeps control: no spin,
//!   its drift, charge and boost go on. Into thin air, the striker slides 0.3 m and flails.
//! - **Grab**: the paw reaches out for 0.5 s and grabs the first kart in reach (to the side or just
//!   ahead). The tether is a damped spring that only pulls (no Rapier joint): the grabber is
//!   towed up to its partner's speed, never beyond; the kart held gets a quarter of the pull, never
//!   forward (the kart's `TetherPull`), and is braked a little (its top speed −10 %:
//!   `MaxSpeedFactors::tether`). It ends when the grabber lets go (steering hard after
//!   0.5 s throws: a strike 1.5 times stronger), after 2 s, stretched 2.5 m, when the grabber
//!   hops, when the kart held lands a jump, or when either is hit, towed or finishes.
//! - **Weight** (GD § 9 and 15): it scales the push (`kart::strike_push_factor`) and each kart's
//!   share of the tether (`kart::tether_force_scale`), never the reach nor the croquettes.
//! - **Immunities**: the table of GD § 15 (`logic.rs`), the cooldown, the 1 s of protection.
//!
//! **Network ready** (docs/ARCHITECTURE.md § « Interactions entre karts »): the kart that acts
//! decides its target, its cooldown and its swings into thin air, and tells the other kart
//! through messages that carry what the network will carry ([`KartStruck`], [`KartGrabbed`],
//! [`TetherEnded`]). **The victim decides**: the system of the kart struck or grabbed checks the
//! event (immunities, protection, reach with some tolerance, age) and applies it to its own kart
//! only, then answers ([`StrikeAnswer`], or a `TetherEnded` refusal). For a tether, **each kart
//! pulls itself** toward the other one. Solo, everything happens on the same tick through these
//! messages; online (issues #8 and #9), they will be sent and received.
//!
//! Everything runs in `FixedUpdate` at 60 Hz ([`InteractionSystems`]): after the input, the race
//! gate and the items (an item hit breaks a tether on the same tick), before the croquettes (a
//! strike costs its croquette on the same tick) and the driving (the push and the pull apply on
//! the same tick). `InteractionTuning::enabled` switches them off: the bots' driving tests race
//! without them. Visuals in `visual.rs` ([`InteractionVisualPlugin`]); the bots' use in
//! `crate::bots`.

mod logic;
mod tuning;
mod visual;

#[cfg(test)]
mod scenario_tests;

pub use logic::{
    Held, InReach, KartView, Paw, PawContext, PawMotion, PawOutcome, PawStats, Swing, SwingKind,
    TetherEnd, accepts, aimed_side, can_be_grabbed, can_be_struck, can_grab, can_strike,
    held_accel, held_end, let_go, max_arm_length, max_reach, pick_target, reach, step_paw,
    step_remote_paw, tether_accel, ticks,
};
pub use tuning::InteractionTuning;
#[cfg(test)]
pub(crate) use visual::shown_arms;
pub use visual::{InteractionVisualPlugin, PawArm, PawGauge, PawPof, PawVisualAssets};

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::TICK_HZ;
use crate::croquettes::{CroquetteSystems, LoseCroquettes, LossCause};
use crate::items::ItemSystems;
use crate::kart::{
    Invincibility, Kart, KartInput, KartState, KartSystems, MaxSpeedFactors, Remote, Shove, Stall,
    StatEffects, StatsTuning, TetherPull, strike_push_factor, tether_force_scale,
};
use crate::net::KartHistory;
use crate::race::{Race, RaceProgress, RaceSystems};

/// `striker` strikes `target` with its paw, or throws it at the end of a tether (`throw`): the
/// network's `Coup` (docs/ARCHITECTURE.md). The kart struck decides whether it is hit, and
/// applies the push to its own kart.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct KartStruck {
    pub striker: Entity,
    pub target: Entity,
    /// The tick of the strike, and the tick of the target's state the striker saw (`tick_vu`;
    /// solo, the same).
    pub tick: u32,
    pub seen: u32,
    /// Where the striker was (the target checks the reach from there).
    pub striker_position: Vec3,
    /// Direction of the push: horizontal, unit (world).
    pub direction: Vec3,
    /// Push speed (m/s): `strike_push_speed`, the Weights and a throw's factor applied.
    pub push: f32,
    pub throw: bool,
}

/// The answer of the kart struck (`CoupReçu` / `CoupIgnoré`): it starts the effects on every
/// peer (the "pof", the ragdoll), and counts the strikes landed.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct StrikeAnswer {
    pub striker: Entity,
    pub target: Entity,
    pub landed: bool,
    pub throw: bool,
}

/// `grabber` grabbed `target` (`Attrape`): the grabber holds it at once (prediction); the kart
/// grabbed accepts, or refuses with a `TetherEnded { reason: Refused }`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct KartGrabbed {
    pub grabber: Entity,
    pub target: Entity,
    pub tick: u32,
    pub seen: u32,
    pub grabber_position: Vec3,
    /// The tether's rest length: the distance when it grabbed (m).
    pub rest: f32,
}

/// The tether between `grabber` and `target` ends (`FinAttrape`): either side decides it for its
/// own reasons, both let go.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TetherEnded {
    pub grabber: Entity,
    pub target: Entity,
    pub reason: TetherEnd,
}

/// The interaction systems inside `FixedUpdate`: after the input, the race gate and the items,
/// before the croquettes and the driving.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractionSystems;

/// The tick the events speak in (`tick`, `seen`): the local tick solo; online (issue #9), the
/// room's common tick, which the network sets (`offset`: common minus local), so that the
/// victim compares the tick the striker saw with its own history (`net::KartHistory`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionClock {
    pub offset: i64,
}

impl InteractionClock {
    /// The tick of this simulation tick.
    pub fn tick(&self, time: &Time) -> u32 {
        (i64::from(current_tick(time)) + self.offset).clamp(0, i64::from(u32::MAX)) as u32
    }
}

/// The interaction rules. Headless: tests run them with `MinimalPlugins`.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InteractionTuning>()
            .init_resource::<InteractionClock>()
            .add_message::<KartStruck>()
            .add_message::<StrikeAnswer>()
            .add_message::<KartGrabbed>()
            .add_message::<TetherEnded>()
            .add_message::<LoseCroquettes>()
            // Every kart has a paw.
            .register_required_components::<Kart, Paw>()
            .register_required_components::<Kart, PawStats>()
            .configure_sets(
                FixedUpdate,
                InteractionSystems
                    .after(KartSystems::Input)
                    .after(RaceSystems::Gate)
                    .after(ItemSystems)
                    .before(CroquetteSystems)
                    .before(KartSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (
                    (
                        reset_on_restart.run_if(resource_exists::<Race>),
                        tick_paws,
                        use_paws,
                        receive_interactions,
                        count_landed,
                        end_tethers,
                        follow_remote_paws,
                        pull_tethers,
                    )
                        .chain()
                        .run_if(interactions_on),
                    clear_interactions.run_if(not(interactions_on)),
                )
                    .in_set(InteractionSystems),
            );
    }
}

/// The interactions are in the races (`InteractionTuning::enabled`).
pub fn interactions_on(tuning: Res<InteractionTuning>) -> bool {
    tuning.enabled
}

/// The current tick of the simulation (60 Hz), for the events' `tick` and `seen`.
fn current_tick(time: &Time) -> u32 {
    (time.elapsed_secs_f64() * TICK_HZ).round() as u32
}

/// A kart's heading, horizontal.
fn heading(transform: &Transform) -> Vec3 {
    transform.forward().with_y(0.0).normalize_or(Vec3::NEG_Z)
}

/// The right of a horizontal heading.
fn right_of(heading: Vec3) -> Vec3 {
    heading.cross(Vec3::Y)
}

/// A swing into thin air (GD § 15): the kart slides `whiff_slide` toward `side` (`direction`,
/// world) without losing speed, and its driver flails.
fn whiff(paw: &mut Paw, shove: &mut Shove, tuning: &InteractionTuning, direction: Vec3, side: f32) {
    let time = tuning.strike_push_time.max(1e-3);
    shove.start(direction, 2.0 * tuning.whiff_slide / time, time);
    paw.lose_balance(ticks(tuning.off_balance_time), side);
}

/// Every paw at rest, no tether.
fn rest(paw: &mut Mut<Paw>, pull: &mut Mut<TetherPull>, factors: &mut Mut<MaxSpeedFactors>) {
    paw.set_if_neq(Paw::default());
    pull.set_if_neq(TetherPull::default());
    if factors.tether != 1.0 {
        factors.tether = 1.0;
    }
}

/// Interactions switched off: no paw, no tether.
fn clear_interactions(mut karts: Query<(&mut Paw, &mut TetherPull, &mut MaxSpeedFactors)>) {
    for (mut paw, mut pull, mut factors) in &mut karts {
        rest(&mut paw, &mut pull, &mut factors);
    }
}

/// A race (re)start: every paw at rest, no tether.
fn reset_on_restart(
    race: Res<Race>,
    mut karts: Query<(&mut Paw, &mut TetherPull, &mut MaxSpeedFactors)>,
) {
    if race.flow.tick != 1 {
        return;
    }
    for (mut paw, mut pull, mut factors) in &mut karts {
        rest(&mut paw, &mut pull, &mut factors);
    }
}

/// The visual swings and shakes only last that long (ticks): past it, the counters stop.
const FX_TICKS: u32 = 120;

/// Cooldowns, protections and the visuals' clocks.
fn tick_paws(mut paws: Query<&mut Paw>) {
    for mut paw in &mut paws {
        let busy = paw.cooldown > 0
            || paw.protected > 0
            || paw.off_balance > 0
            || paw.swing.is_some()
            || (paw.struck_count > 0 && paw.struck_ticks < FX_TICKS);
        if !busy {
            continue;
        }
        let paw = &mut *paw;
        paw.cooldown = paw.cooldown.saturating_sub(1);
        paw.protected = paw.protected.saturating_sub(1);
        paw.off_balance = paw.off_balance.saturating_sub(1);
        paw.struck_ticks = (paw.struck_ticks + 1).min(FX_TICKS);
        if let Some(swing) = &mut paw.swing {
            swing.ticks += 1;
            if swing.ticks >= FX_TICKS {
                paw.swing = None;
            }
        }
    }
}

/// Every kart's Interaction button (the side that acts, `step_paw`): strikes, grabs, the end of
/// its tether, its swings into thin air. It tells the other kart by message; it never touches
/// it. The karts are gathered first in a buffer kept between ticks (no allocation per tick).
/// Online (issue #9), the other peers' karts are targets too, where they are shown: the network
/// takes the event to the peer that decides.
fn use_paws(
    tuning: Res<InteractionTuning>,
    stats_tuning: Res<StatsTuning>,
    time: Res<Time>,
    clock: Res<InteractionClock>,
    race: Option<Res<Race>>,
    mut views: Local<Vec<KartView>>,
    remotes: Query<
        (
            Entity,
            &Transform,
            &Velocity,
            &KartState,
            &Invincibility,
            &StatEffects,
            &Remote,
            Option<&Paw>,
            Option<&RaceProgress>,
        ),
        Without<Kart>,
    >,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &Velocity,
            &KartInput,
            &KartState,
            &Stall,
            &Invincibility,
            &StatEffects,
            (&mut Paw, &mut PawStats, &mut Shove),
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut strikes: MessageWriter<KartStruck>,
    mut grabs: MessageWriter<KartGrabbed>,
    mut ends: MessageWriter<TetherEnded>,
) {
    let racing = race.as_ref().is_none_or(|race| race.started());
    let tick = clock.tick(&time);
    views.clear();
    views.extend(karts.iter().map(
        |(
            entity,
            transform,
            velocity,
            _,
            state,
            _,
            invincibility,
            effects,
            (paw, ..),
            progress,
        )| {
            let finished = progress.is_some_and(|p| p.finished());
            let invincible = invincibility.is_active();
            KartView {
                entity,
                position: transform.translation,
                now: transform.translation,
                seen: tick,
                forward: heading(transform),
                velocity: velocity.linear,
                weight: effects.weight,
                strikable: can_be_struck(state, invincible, racing, finished) && paw.protected == 0,
                grabbable: can_be_grabbed(state, invincible, racing, finished, paw.linked()),
            }
        },
    ));
    views.extend(remotes.iter().map(
        |(entity, transform, velocity, state, invincibility, effects, remote, paw, progress)| {
            let finished = progress.is_some_and(|p| p.finished());
            let invincible = invincibility.is_active();
            let (protected, linked) = paw.map_or((false, false), |p| (p.protected > 0, p.linked()));
            KartView {
                entity,
                position: transform.translation,
                now: remote.now,
                seen: remote.seen,
                forward: heading(transform),
                velocity: velocity.linear,
                weight: effects.weight,
                strikable: can_be_struck(state, invincible, racing, finished) && !protected,
                grabbable: can_be_grabbed(state, invincible, racing, finished, linked),
            }
        },
    ));
    for (
        entity,
        transform,
        velocity,
        input,
        state,
        stall,
        _,
        effects,
        (mut paw, mut stats, mut shove),
        progress,
    ) in &mut karts
    {
        // Nothing going on, nothing pressed: nothing to do.
        if !input.interact && !paw.button && paw.motion == PawMotion::Idle {
            continue;
        }
        let finished = progress.is_some_and(|p| p.finished());
        let stalled = stall.is_active();
        let forward = heading(transform);
        let right = right_of(forward);
        let ctx = PawContext {
            entity,
            position: transform.translation,
            forward,
            velocity: velocity.linear,
            button: input.interact,
            steer: input.steer,
            drift: match *state {
                KartState::Drifting { side, .. } => Some(side),
                _ => None,
            },
            can_strike: can_strike(state, stalled, racing, finished),
            can_grab: can_grab(state, stalled, racing, finished, paw.held.is_some()),
            grounded: *state == KartState::Grounded,
            hit: state.is_hit(),
            towed: *state == KartState::Respawning,
            finished,
        };
        let holding_side = match paw.motion {
            PawMotion::Holding { side, .. } => side,
            _ => 1.0,
        };
        let mut next = *paw;
        let weight_of = |kart: Entity| views.iter().find(|k| k.entity == kart).map(|k| k.weight);
        let seen_of = |kart: Entity| {
            views
                .iter()
                .find(|k| k.entity == kart)
                .map_or(tick, |k| k.seen)
        };
        match step_paw(&mut next, &tuning, &ctx, &views) {
            PawOutcome::None => {}
            PawOutcome::Strike { target, side } => {
                stats.strikes += 1;
                next.swing = Some(Swing {
                    kind: if target.is_some() {
                        SwingKind::Strike
                    } else {
                        SwingKind::Whiff
                    },
                    side,
                    target,
                    ticks: 0,
                });
                match target.and_then(|t| weight_of(t).map(|w| (t, w))) {
                    Some((target, weight)) => {
                        strikes.write(KartStruck {
                            striker: entity,
                            target,
                            tick,
                            seen: seen_of(target),
                            striker_position: transform.translation,
                            direction: right * side,
                            push: tuning.strike_push_speed
                                * strike_push_factor(&stats_tuning, effects.weight, weight),
                            throw: false,
                        });
                    }
                    None => {
                        stats.missed += 1;
                        whiff(&mut next, &mut shove, &tuning, right * side, side);
                    }
                }
            }
            PawOutcome::Grab { target, rest, side } => {
                stats.grabs += 1;
                next.swing = Some(Swing {
                    kind: SwingKind::Strike,
                    side,
                    target: Some(target),
                    ticks: 0,
                });
                grabs.write(KartGrabbed {
                    grabber: entity,
                    target,
                    tick,
                    seen: seen_of(target),
                    grabber_position: transform.translation,
                    rest,
                });
            }
            PawOutcome::GrabMiss { side } => {
                stats.missed += 1;
                next.swing = Some(Swing {
                    kind: SwingKind::GrabMiss,
                    side,
                    target: None,
                    ticks: 0,
                });
                whiff(&mut next, &mut shove, &tuning, right * side, side);
            }
            PawOutcome::LetGo {
                target,
                reason,
                throw,
            } => {
                ends.write(TetherEnded {
                    grabber: entity,
                    target,
                    reason,
                });
                next.swing = Some(Swing {
                    kind: SwingKind::LetGo,
                    side: holding_side,
                    target: Some(target),
                    ticks: 0,
                });
                if let Some(side) = throw {
                    stats.throws += 1;
                    next.swing = Some(Swing {
                        kind: SwingKind::Throw,
                        side,
                        target: Some(target),
                        ticks: 0,
                    });
                    let weight = weight_of(target).unwrap_or(effects.weight);
                    strikes.write(KartStruck {
                        striker: entity,
                        target,
                        tick,
                        seen: seen_of(target),
                        striker_position: transform.translation,
                        direction: right * side,
                        push: tuning.strike_push_speed
                            * tuning.throw_push_factor
                            * strike_push_factor(&stats_tuning, effects.weight, weight),
                        throw: true,
                    });
                }
            }
        }
        paw.set_if_neq(next);
    }
}

/// The victim's side (docs/ARCHITECTURE.md): each kart applies to itself the strikes and the
/// grabs aimed at it, if they reach it and it is not immune; and the kart held decides when it
/// breaks free (it lands its jump), or when its tether breaks because of it (hit, towed, past
/// the line). Online (issue #9), only our own karts are here (another peer's kart is its peer's
/// to judge), and the reach is checked from where the kart was at the tick the striker saw
/// (`net::KartHistory`, 30 ticks).
fn receive_interactions(
    tuning: Res<InteractionTuning>,
    time: Res<Time>,
    clock: Res<InteractionClock>,
    race: Option<Res<Race>>,
    mut strikes: MessageReader<KartStruck>,
    mut grabs: MessageReader<KartGrabbed>,
    present: Query<(), Or<(With<Kart>, With<Remote>)>>,
    remote: Query<(), (With<Remote>, Without<Kart>)>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &mut Velocity,
            &KartState,
            &Invincibility,
            (&mut Paw, &mut PawStats, &mut Shove),
            Option<&RaceProgress>,
            Option<&KartHistory>,
        ),
        With<Kart>,
    >,
    mut answers: MessageWriter<StrikeAnswer>,
    mut ends: MessageWriter<TetherEnded>,
    mut losses: MessageWriter<LoseCroquettes>,
) {
    let racing = race.as_ref().is_none_or(|race| race.started());
    let tick = clock.tick(&time);
    // Where our kart was at the tick the striker saw: solo (no history), where it is; online,
    // from its history (30 ticks), and nowhere if that is older (the event is refused).
    let seen_at = |transform: &Transform, history: Option<&KartHistory>, seen: u32| match history {
        Some(history) => history.0.position_at(seen),
        None => Some(transform.translation),
    };
    for strike in strikes.read() {
        let Ok((
            _,
            transform,
            mut velocity,
            state,
            invincibility,
            (mut paw, mut stats, mut shove),
            progress,
            history,
        )) = karts.get_mut(strike.target)
        else {
            continue;
        };
        let finished = progress.is_some_and(|p| p.finished());
        // A throw comes from the kart holding it; a strike from a kart in reach.
        let in_reach = if strike.throw {
            paw.held.is_some_and(|held| held.by == strike.striker)
        } else {
            // Its age from when it was sent; the reach from where we were at the tick it saw.
            seen_at(transform, history, strike.seen).is_some_and(|position| {
                accepts(
                    &tuning,
                    tick,
                    strike.tick,
                    strike.striker_position,
                    position,
                )
            })
        };
        let landed = in_reach
            && paw.protected == 0
            && can_be_struck(state, invincibility.is_active(), racing, finished);
        answers.write(StrikeAnswer {
            striker: strike.striker,
            target: strike.target,
            landed,
            throw: strike.throw,
        });
        if !landed {
            continue;
        }
        // It keeps control (no spin, its drift and boost go on): it loses some speed, whatever
        // the Weights, and is pushed sideways; a croquette; then protected for a while.
        let keep = 1.0 - tuning.strike_speed_loss.clamp(0.0, 1.0);
        let v = velocity.linear;
        velocity.linear = Vec3::new(v.x * keep, v.y, v.z * keep);
        shove.start(strike.direction, strike.push, tuning.strike_push_time);
        paw.protected = ticks(tuning.strike_protection);
        paw.struck_count += 1;
        paw.struck_ticks = 0;
        paw.struck_side = strike.direction;
        stats.struck += 1;
        losses.write(LoseCroquettes {
            kart: strike.target,
            cause: LossCause::Strike,
        });
    }
    for grab in grabs.read() {
        let Ok((_, transform, _, state, invincibility, (mut paw, mut stats, _), progress, history)) =
            karts.get_mut(grab.target)
        else {
            // Another peer's kart decides for itself (online); a kart gone cannot be held.
            if !remote.contains(grab.target) {
                ends.write(TetherEnded {
                    grabber: grab.grabber,
                    target: grab.target,
                    reason: TetherEnd::Gone,
                });
            }
            continue;
        };
        let finished = progress.is_some_and(|p| p.finished());
        let accepted = seen_at(transform, history, grab.seen).is_some_and(|position| {
            accepts(&tuning, tick, grab.tick, grab.grabber_position, position)
        }) && can_be_grabbed(
            state,
            invincibility.is_active(),
            racing,
            finished,
            paw.linked(),
        );
        if accepted {
            paw.held = Some(Held {
                by: grab.grabber,
                rest: grab.rest,
                ticks: 0,
                escaping: false,
            });
            stats.held += 1;
        } else {
            ends.write(TetherEnded {
                grabber: grab.grabber,
                target: grab.target,
                reason: TetherEnd::Refused,
            });
        }
    }
    // The kart held: it breaks free when it lands its jump; hit, towed or past the line, its
    // tether breaks.
    for (entity, _, _, state, _, (mut paw, ..), progress, _) in &mut karts {
        let Some(mut held) = paw.held else {
            continue;
        };
        let reason = if present.contains(held.by) {
            held_end(
                &mut held,
                &tuning,
                state,
                progress.is_some_and(|p| p.finished()),
            )
        } else {
            Some(TetherEnd::Gone)
        };
        paw.held = Some(held);
        if let Some(reason) = reason {
            ends.write(TetherEnded {
                grabber: held.by,
                target: entity,
                reason,
            });
        }
    }
}

/// The striker learns whether its strike landed (the answer of the kart struck).
fn count_landed(mut answers: MessageReader<StrikeAnswer>, mut stats: Query<&mut PawStats>) {
    for answer in answers.read() {
        if answer.landed
            && let Ok(mut stats) = stats.get_mut(answer.striker)
        {
            stats.landed += 1;
        }
    }
}

/// The end of a tether, whoever decided it: the grabber lets go (3 s of cooldown; 1.5 s after a
/// refusal; off balance when the kart held broke free or refused), the kart held is free.
///
/// Online, another peer's kart that held ours lets go on our screen too (issue #81): its paw only
/// shows what its peer told, and when the end is ours (we broke free, refused, finished...), its
/// peer says nothing back. Its arm stayed stretched toward our kart for good otherwise.
fn end_tethers(
    tuning: Res<InteractionTuning>,
    mut ended: MessageReader<TetherEnded>,
    mut karts: Query<(&Transform, &mut Paw, &mut PawStats, &mut Shove), With<Kart>>,
    mut remotes: Query<&mut Paw, (With<Remote>, Without<Kart>)>,
) {
    for end in ended.read() {
        if let Ok((transform, mut paw, mut stats, mut shove)) = karts.get_mut(end.grabber)
            && let PawMotion::Holding { target, side, .. } = paw.motion
            && target == end.target
        {
            let paw = &mut *paw;
            paw.cooldown = ticks(if end.reason == TetherEnd::Refused {
                tuning.strike_cooldown
            } else {
                tuning.tether_cooldown
            });
            if let_go(paw, &tuning, target, side, end.reason) {
                // Off balance, like a grab into thin air (GD § 15).
                if end.reason == TetherEnd::Refused {
                    stats.grabs = stats.grabs.saturating_sub(1);
                    stats.missed += 1;
                }
                let right = right_of(heading(transform));
                whiff(paw, &mut shove, &tuning, right * side, side);
            }
        }
        if let Ok(mut paw) = remotes.get_mut(end.grabber)
            && let PawMotion::Holding { target, side, .. } = paw.motion
            && target == end.target
        {
            let_go(&mut paw, &tuning, target, side, end.reason);
        }
        if let Ok((_, mut paw, mut stats, _)) = karts.get_mut(end.target)
            && paw.held.is_some_and(|held| held.by == end.grabber)
        {
            paw.held = None;
            if end.reason == TetherEnd::Escaped {
                stats.escapes += 1;
            }
        }
    }
}

/// Online, the paws of the other peers' karts (issue #81): one holding a kart lets go by itself
/// once the longest tether is over (`step_remote_paw`), should no end ever come.
fn follow_remote_paws(
    tuning: Res<InteractionTuning>,
    mut remotes: Query<&mut Paw, (With<Remote>, Without<Kart>)>,
) {
    for mut paw in &mut remotes {
        if paw.holding() {
            step_remote_paw(&mut paw, &tuning);
        }
    }
}

/// Each kart of a tether pulls itself toward the other one (docs/ARCHITECTURE.md): the grabber
/// gets the whole pull and is towed up to its partner's speed, the kart held a quarter of it,
/// never forward; each divided by its Weight factor. The others: no pull. Online (issue #9),
/// toward another peer's kart where it is now (extrapolated), not where it is shown.
fn pull_tethers(
    tuning: Res<InteractionTuning>,
    stats_tuning: Res<StatsTuning>,
    karts: Query<(&Transform, &Velocity, &Paw, &StatEffects)>,
    partners: Query<(&Transform, &Velocity, Option<&Remote>)>,
    mut pulls: Query<(Entity, &mut TetherPull, &mut MaxSpeedFactors)>,
) {
    for (entity, mut pull, mut factors) in &mut pulls {
        // The kart held is braked a little (`tether_target_brake`), with its Weight.
        let brake = karts
            .get(entity)
            .ok()
            .filter(|(_, _, paw, _)| paw.held.is_some())
            .map_or(1.0, |(_, _, _, effects)| {
                1.0 - tuning.tether_target_brake * tether_force_scale(&stats_tuning, effects.weight)
            });
        if factors.tether != brake {
            factors.tether = brake;
        }
        let wanted = karts
            .get(entity)
            .ok()
            .and_then(|(transform, velocity, paw, effects)| {
                let (partner, rest, grabber) = match paw.motion {
                    PawMotion::Holding { target, rest, .. } => (target, rest, true),
                    _ => {
                        let held = paw.held?;
                        (held.by, held.rest, false)
                    }
                };
                let (other, other_velocity, remote) = partners.get(partner).ok()?;
                let (other, other_velocity) = match remote {
                    Some(remote) => (remote.now, remote.now_velocity),
                    None => (other.translation, other_velocity.linear),
                };
                let forward = heading(transform);
                let accel = tether_accel(
                    &tuning,
                    transform.translation,
                    velocity.linear,
                    other,
                    other_velocity,
                    rest,
                ) * tether_force_scale(&stats_tuning, effects.weight);
                Some(if grabber {
                    TetherPull {
                        accel,
                        tow_speed: Some(other_velocity.dot(forward)),
                    }
                } else {
                    TetherPull {
                        accel: held_accel(&tuning, accel, forward),
                        tow_speed: None,
                    }
                })
            })
            .unwrap_or_default();
        pull.set_if_neq(wanted);
    }
}
