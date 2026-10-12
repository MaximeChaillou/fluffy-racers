//! The race's state machine (GD § 1), pure and deterministic: grid → countdown "3, 2, 1, GO"
//! → race → finish → results → restart.
//!
//! The whole state is two tick counts ([`RaceFlow`]), so it is trivial to serialize: the host
//! will send it to the other peers when the race goes online (docs/ARCHITECTURE.md).

use serde::{Deserialize, Serialize};

use super::tuning::{RaceTuning, seconds_to_ticks};

/// Phase of the race.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RacePhase {
    /// Karts on the grid, before the countdown: nothing moves.
    Grid,
    /// "3, 2, 1": nothing moves yet (the rocket start of issue #13 will listen to the throttle).
    Countdown,
    /// From GO until every awaited racer has finished.
    Racing,
    /// The race is over: finish banner.
    Finished,
    /// Results screen, until a restart.
    Results,
}

/// Phase durations in ticks, from [`RaceTuning`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowTimings {
    pub grid: u32,
    pub countdown_steps: u32,
    pub countdown_step: u32,
    pub go_display: u32,
    pub finish: u32,
    pub restart_delay: u32,
}

impl FlowTimings {
    pub fn new(tuning: &RaceTuning) -> Self {
        Self {
            grid: seconds_to_ticks(tuning.grid_seconds),
            countdown_steps: tuning.countdown_steps,
            countdown_step: seconds_to_ticks(tuning.countdown_step_seconds).max(1),
            go_display: seconds_to_ticks(tuning.go_display_seconds),
            finish: seconds_to_ticks(tuning.finish_seconds),
            restart_delay: seconds_to_ticks(tuning.restart_delay_seconds),
        }
    }

    /// Ticks from the race (re)start to the end of the countdown.
    pub fn before_go(&self) -> u32 {
        self.grid + self.countdown_steps * self.countdown_step
    }
}

/// Clock and state of a race. Advance it once per simulation tick with [`RaceFlow::advance`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RaceFlow {
    /// The tick being simulated, counted from the race (re)start: 1 on the first tick on the
    /// grid, 0 before it.
    pub tick: u32,
    /// Tick at which every awaited racer had finished.
    pub over_at: Option<u32>,
}

impl RaceFlow {
    /// Moves to the next tick. `race_over`: every awaited racer has finished (see
    /// [`race_over`]); `restart`: a restart was asked (honoured on the results screen only,
    /// after `restart_delay`). Returns true when the race restarted: the caller puts every
    /// racer back on the grid. The restart tick is the first tick on the grid.
    pub fn advance(&mut self, timings: &FlowTimings, race_over: bool, restart: bool) -> bool {
        let restarted = restart && self.can_restart(timings);
        if restarted {
            *self = Self::default();
        }
        self.tick += 1;
        if race_over && self.over_at.is_none() && self.phase(timings) == RacePhase::Racing {
            self.over_at = Some(self.tick);
        }
        restarted
    }

    pub fn phase(&self, timings: &FlowTimings) -> RacePhase {
        if let Some(over) = self.over_at {
            return if self.tick < over + timings.finish {
                RacePhase::Finished
            } else {
                RacePhase::Results
            };
        }
        if self.tick <= timings.grid {
            RacePhase::Grid
        } else if self.tick <= timings.before_go() {
            RacePhase::Countdown
        } else {
            RacePhase::Racing
        }
    }

    /// Whether a restart asked now would be honoured on the next tick: once the results have
    /// been on screen for `restart_delay` ticks.
    pub fn can_restart(&self, timings: &FlowTimings) -> bool {
        self.phase(timings) == RacePhase::Results
            && self
                .over_at
                .is_some_and(|over| self.tick + 1 >= over + timings.finish + timings.restart_delay)
    }

    /// Racing ticks simulated so far, the current one included: the race clock (0 before GO).
    /// It keeps running after the race is over, for the racers still on track.
    pub fn race_ticks(&self, timings: &FlowTimings) -> u32 {
        self.tick.saturating_sub(timings.before_go())
    }

    /// First racing tick: the karts move from this tick on.
    pub fn go_tick(timings: &FlowTimings) -> u32 {
        timings.before_go() + 1
    }

    /// First tick showing "2", the reference of the rocket start (GD § 4, issue #13).
    pub fn two_tick(timings: &FlowTimings) -> u32 {
        let steps_before_two = timings.countdown_steps.saturating_sub(2);
        timings.grid + steps_before_two * timings.countdown_step + 1
    }

    /// Ticks since "2" appeared (0 on that tick), or `None` before it. For the rocket start.
    pub fn ticks_since_two(&self, timings: &FlowTimings) -> Option<u32> {
        self.tick.checked_sub(Self::two_tick(timings))
    }

    /// Number shown by the countdown (3, 2, 1), during the countdown only.
    pub fn countdown_number(&self, timings: &FlowTimings) -> Option<u32> {
        if self.phase(timings) != RacePhase::Countdown {
            return None;
        }
        let elapsed = self.tick - timings.grid - 1;
        Some(timings.countdown_steps - elapsed / timings.countdown_step)
    }

    /// Whether "GO !" is on screen: the first `go_display` racing ticks.
    pub fn shows_go(&self, timings: &FlowTimings) -> bool {
        self.over_at.is_none() && (1..=timings.go_display).contains(&self.race_ticks(timings))
    }

    /// Whether the karts may drive: from GO on.
    pub fn started(&self, timings: &FlowTimings) -> bool {
        self.race_ticks(timings) > 0
    }
}

/// The race is over once every awaited racer (the local players) has finished; with no awaited
/// racer at all (a test autopilot, later a bots-only race), once every racer has finished.
/// `racers` yields `(awaited, finished)`.
pub fn race_over(racers: impl IntoIterator<Item = (bool, bool)>) -> bool {
    let (mut any, mut any_awaited) = (false, false);
    let (mut all_finished, mut awaited_finished) = (true, true);
    for (awaited, finished) in racers {
        any = true;
        all_finished &= finished;
        if awaited {
            any_awaited = true;
            awaited_finished &= finished;
        }
    }
    if any_awaited {
        awaited_finished
    } else {
        any && all_finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timings() -> FlowTimings {
        FlowTimings::new(&RaceTuning::default())
    }

    /// Advances `ticks` ticks without event, returning the phase of each tick.
    fn run(flow: &mut RaceFlow, t: &FlowTimings, ticks: u32) -> Vec<RacePhase> {
        (0..ticks)
            .map(|_| {
                assert!(!flow.advance(t, false, false));
                flow.phase(t)
            })
            .collect()
    }

    #[test]
    fn grid_then_one_second_per_countdown_step_then_go() {
        let t = timings();
        assert_eq!((t.grid, t.countdown_step, t.countdown_steps), (60, 60, 3));
        let mut flow = RaceFlow::default();
        assert_eq!(flow.phase(&t), RacePhase::Grid);
        let phases = run(&mut flow, &t, 60);
        assert!(phases.iter().all(|p| *p == RacePhase::Grid));
        assert_eq!(flow.countdown_number(&t), None);

        let mut numbers = Vec::new();
        for _ in 0..180 {
            flow.advance(&t, false, false);
            assert_eq!(flow.phase(&t), RacePhase::Countdown);
            assert!(!flow.started(&t));
            numbers.push(flow.countdown_number(&t).unwrap());
        }
        // 60 ticks of "3", then "2", then "1".
        assert!(numbers[..60].iter().all(|n| *n == 3));
        assert!(numbers[60..120].iter().all(|n| *n == 2));
        assert!(numbers[120..].iter().all(|n| *n == 1));

        // GO: the next tick is the first racing tick.
        flow.advance(&t, false, false);
        assert_eq!(flow.tick, RaceFlow::go_tick(&t));
        assert_eq!(flow.phase(&t), RacePhase::Racing);
        assert!(flow.started(&t));
        assert_eq!(flow.race_ticks(&t), 1);
        assert_eq!(flow.countdown_number(&t), None);
        assert!(flow.shows_go(&t));
        run(&mut flow, &t, 59);
        assert!(flow.shows_go(&t), "GO stays one second");
        run(&mut flow, &t, 1);
        assert!(!flow.shows_go(&t));
        assert_eq!(flow.race_ticks(&t), 61);
    }

    #[test]
    fn the_two_is_exposed_for_the_rocket_start() {
        let t = timings();
        let mut flow = RaceFlow::default();
        let two = RaceFlow::two_tick(&t);
        // Grid (60) + one second of "3".
        assert_eq!(two, 121);
        while flow.tick + 1 < two {
            flow.advance(&t, false, false);
            assert_eq!(flow.ticks_since_two(&t), None);
        }
        flow.advance(&t, false, false);
        assert_eq!(flow.countdown_number(&t), Some(2));
        assert_eq!(flow.ticks_since_two(&t), Some(0));
        flow.advance(&t, false, false);
        assert_eq!(flow.ticks_since_two(&t), Some(1));
        // GO is 2 s after the "2".
        assert_eq!(RaceFlow::go_tick(&t) - two, 120);
    }

    #[test]
    fn finish_then_results_then_restart_after_the_delay() {
        let t = timings();
        let mut flow = RaceFlow::default();
        // "Over" before GO is ignored (nobody can have finished).
        flow.advance(&t, true, false);
        assert_eq!(flow.over_at, None);
        run(&mut flow, &t, t.before_go() + 600);
        assert_eq!(flow.phase(&t), RacePhase::Racing);
        // A restart during the race is ignored.
        assert!(!flow.advance(&t, false, true));
        assert_eq!(flow.phase(&t), RacePhase::Racing);

        flow.advance(&t, true, false);
        let over = flow.tick;
        assert_eq!(flow.over_at, Some(over));
        assert_eq!(flow.phase(&t), RacePhase::Finished);
        assert!(!flow.shows_go(&t));
        let race_ticks = flow.race_ticks(&t);
        for _ in 1..t.finish {
            assert!(
                !flow.advance(&t, true, true),
                "no restart on the finish banner"
            );
            assert_eq!(flow.phase(&t), RacePhase::Finished);
        }
        flow.advance(&t, true, false);
        assert_eq!(flow.phase(&t), RacePhase::Results);
        assert_eq!(flow.tick, over + t.finish);
        // The clock kept running for racers still on track.
        assert_eq!(flow.race_ticks(&t), race_ticks + t.finish);

        for _ in 0..t.restart_delay - 1 {
            assert!(!flow.advance(&t, true, true), "restart delay");
            assert_eq!(flow.phase(&t), RacePhase::Results);
        }
        assert!(flow.can_restart(&t));
        assert!(flow.advance(&t, true, true));
        // The restart tick is the first tick on the grid.
        assert_eq!(flow.tick, 1);
        assert_eq!(flow.over_at, None);
        assert_eq!(flow.phase(&t), RacePhase::Grid);
        assert_eq!(flow.race_ticks(&t), 0);
    }

    #[test]
    fn race_over_waits_for_the_awaited_racers() {
        assert!(!race_over([]));
        // Solo player.
        assert!(!race_over([(true, false)]));
        assert!(race_over([(true, true)]));
        // The player finished, bots still racing: over (their ranks come from their progress).
        assert!(race_over([(true, true), (false, false), (false, false)]));
        // Two players, one finished.
        assert!(!race_over([(true, true), (true, false), (false, true)]));
        // No awaited racer (autopilot test, bots only): everyone must finish.
        assert!(!race_over([(false, true), (false, false)]));
        assert!(race_over([(false, true), (false, true)]));
    }

    #[test]
    fn the_flow_serializes() {
        let flow = RaceFlow {
            tick: 1234,
            over_at: Some(1200),
        };
        let text = ron::to_string(&flow).unwrap();
        assert_eq!(ron::from_str::<RaceFlow>(&text).unwrap(), flow);
    }
}
