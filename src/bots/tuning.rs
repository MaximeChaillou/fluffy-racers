//! Bot tuning (GD § 12, issue #6). Our own starting points, à régler: tweak them here, never
//! hard-code them in systems.

use bevy::prelude::*;

use crate::kart::MiniTurbo;

/// Bot difficulty (GD § 12: precision, drift tier, anticipation, use of the items).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect)]
pub enum BotDifficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl BotDifficulty {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];

    /// Index into [`BotTuning::skills`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How well a bot drives at one difficulty.
///
/// Distances in meters, speeds in m/s, angles in radians.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
pub struct BotSkill {
    // --- Trajectory precision -------------------------------------------------------------------
    /// Share of the racing line followed: 0 = the centerline, 1 = the full racing line (wide
    /// entry, apex, wide exit).
    pub line_fidelity: f32,
    /// Amplitude of the slow random wobble around its line (m): how precisely it drives.
    pub wobble: f32,
    /// The wobble picks a new target about this often (s).
    pub wobble_period: f32,

    // --- Anticipation -------------------------------------------------------------------------
    /// The bot steers toward the point of its line `aim_base + aim_per_speed × speed` ahead (m).
    pub aim_base: f32,
    pub aim_per_speed: f32,
    /// It looks this far ahead for turns to slow down for (m): shorter = brakes later.
    pub anticipation: f32,
    /// Corner speed, as a share of the fastest speed the kart can turn at there.
    pub corner_margin: f32,
    /// Deceleration it plans with before a turn (m/s²): lower = brakes earlier and longer.
    pub planned_decel: f32,
    /// Speed it aims for on straights, as a share of the kart's top speed (1 = flat out).
    pub cruise_speed: f32,

    // --- Drift (GD § 3) -----------------------------------------------------------------------
    /// Mini-turbo tier it releases the drift at (as soon as it is reached).
    pub target_tier: MiniTurbo,
    /// Goes for the purple tier when the turn left is long enough for it.
    pub purple_when_possible: bool,
    /// Only drifts a turn with at least this much turning left (rad): shorter drifts cannot
    /// reach the blue tier (≈ 2 rad of drift for blue with the default kart tuning).
    pub min_drift_angle: f32,
    /// Chance to drift a turn long enough for it (decided once per turn).
    pub drift_chance: f32,

    // --- Driving techniques (GD § 4 and 5, issue #13) -------------------------------------------
    /// It accelerates this long after the "2" of the countdown, drawn in this range at each
    /// start (s): up to 0.25 s is a perfect rocket start, after 1 s none. Never before the "2".
    pub start_delay_min: f32,
    pub start_delay_max: f32,
    /// Chance to press jump/drift right after leaving a trick ramp (a trick).
    pub trick_chance: f32,

    // --- Items (GD § 8 and 12, issue #16) -------------------------------------------------------
    /// It waits this long once an item is ready before using it (s): how often it uses items.
    pub item_delay: f32,
    /// It throws a pelote at a kart ahead within this angle of its heading (rad): its aim.
    pub item_aim: f32,
    /// Chance to drag a single pelure or pelote behind as a shield before letting it go.
    pub item_shield: f32,
}

/// All the knobs of the bots.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct BotTuning {
    /// Difficulty of the bots of the game's grid. Normal: the default level of a kart game.
    pub grid_difficulty: BotDifficulty,
    /// Easy, Normal, Hard.
    pub skills: [BotSkill; 3],

    // --- Variety (no single file) ---------------------------------------------------------------
    /// Each bot keeps its own lateral offset from the racing line, within ± this (m)...
    pub lane_spread: f32,
    /// ...picks a new one every `lane_period_min` to `lane_period_max` seconds...
    pub lane_period_min: f32,
    pub lane_period_max: f32,
    /// ...and moves toward it at this speed (m/s).
    pub lane_change_rate: f32,
    /// Personal pace: each bot's corner margin is scaled by 1 ± up to this.
    pub pace_spread: f32,

    // --- Racing line (built once per circuit) -----------------------------------------------------
    /// A kart's center stays this far inside the road edge (m; the curbs beyond are road too).
    pub road_margin: f32,
    /// A kart's center stays this far from a sand patch (m): its wheels stay on the asphalt.
    pub sand_margin: f32,
    /// The racing line keeps this much more room on each side (m), for the lanes, the wobble
    /// and the overtaking.
    pub line_margin: f32,
    /// In a drift, the bot counts on this share of the drift's tightest turn rate (planning).
    pub drift_turn_margin: f32,

    // --- Avoidance ----------------------------------------------------------------------------
    /// Looks this far ahead for a kart to overtake (m).
    pub avoid_distance: f32,
    /// Overtakes with this lateral gap, center to center (m; a kart is 1.4 m wide).
    pub avoid_gap: f32,
    /// Only karts it is catching up by at least this much count (m/s).
    pub avoid_closing_speed: f32,
    /// Moves its line aside at most this fast (m/s).
    pub avoid_rate: f32,

    // --- Catch-up (GD § 12: ≤ 3 %) ---------------------------------------------------------------
    /// Largest change of a bot's top speed: +3 % far behind, −3 % far ahead.
    pub catch_up_max: f32,
    /// No change within this distance of the reference (the local player, else the field's
    /// average), along the track (m).
    pub catch_up_dead_zone: f32,
    /// Full change from this distance on (m).
    pub catch_up_full_gap: f32,

    // --- Items (GD § 8, issue #16) ----------------------------------------------------------------
    /// A kart counts as "ahead" for its items up to this far (m)...
    pub item_ahead_range: f32,
    /// ...and as "close behind" up to this far (m, within about 20° of straight back).
    pub item_behind_range: f32,
    /// A shield is held between these two durations (s), unless a kart comes close behind.
    pub item_shield_time: [f32; 2],
    /// The items of an "x3" go at most this often (s).
    pub item_shot_period: f32,
    /// The pelures of an "x3" are dropped about this often anyway (s).
    pub item_drop_period: f32,
    /// Waiting for the right moment ends this long after `BotSkill::item_delay` (s): the item
    /// goes anyway (thrown ahead, eaten), so that the slots turn over.
    pub item_patience: f32,
    /// A piment is eaten with at least this much straight track ahead (m).
    pub item_piment_straight: f32,
    /// A bot with a free slot steers toward an item box ahead (up to `item_box_range` m away)
    /// by at most this much sideways (m), on the straights only...
    pub item_box_pull: f32,
    pub item_box_range: f32,

    // --- Recovery -----------------------------------------------------------------------------
    /// Slower than this (m/s) while trying to drive for `stuck_seconds`: the bot backs up for
    /// `reverse_seconds`, steering away. The tow pelican (3 s) remains the last resort.
    pub stuck_speed: f32,
    pub stuck_seconds: f32,
    pub reverse_seconds: f32,
}

impl BotTuning {
    pub fn skill(&self, difficulty: BotDifficulty) -> &BotSkill {
        &self.skills[difficulty.index()]
    }
}

impl Default for BotTuning {
    fn default() -> Self {
        Self {
            grid_difficulty: BotDifficulty::Normal,
            skills: [
                // Easy: sloppy line, brakes early and slow, short drifts for a blue mini-turbo.
                BotSkill {
                    line_fidelity: 0.4,
                    wobble: 1.2,
                    wobble_period: 1.0,
                    aim_base: 5.0,
                    aim_per_speed: 0.3,
                    anticipation: 30.0,
                    corner_margin: 0.85,
                    planned_decel: 8.0,
                    cruise_speed: 0.92,
                    target_tier: MiniTurbo::Blue,
                    purple_when_possible: false,
                    min_drift_angle: 130f32.to_radians(),
                    drift_chance: 0.5,
                    start_delay_min: 0.2,
                    start_delay_max: 1.3,
                    trick_chance: 0.4,
                    item_delay: 2.5,
                    item_aim: 0.18,
                    item_shield: 0.2,
                },
                // Normal: good line, aims for orange.
                BotSkill {
                    line_fidelity: 0.75,
                    wobble: 0.5,
                    wobble_period: 1.2,
                    aim_base: 5.0,
                    aim_per_speed: 0.32,
                    anticipation: 45.0,
                    corner_margin: 0.93,
                    planned_decel: 12.0,
                    cruise_speed: 0.97,
                    target_tier: MiniTurbo::Orange,
                    purple_when_possible: false,
                    min_drift_angle: 120f32.to_radians(),
                    drift_chance: 0.85,
                    start_delay_min: 0.05,
                    start_delay_max: 0.7,
                    trick_chance: 0.8,
                    item_delay: 1.2,
                    item_aim: 0.1,
                    item_shield: 0.6,
                },
                // Hard: full racing line, late braking, orange, purple when the turn allows it.
                BotSkill {
                    line_fidelity: 1.0,
                    wobble: 0.15,
                    wobble_period: 1.5,
                    aim_base: 5.0,
                    aim_per_speed: 0.35,
                    anticipation: 60.0,
                    corner_margin: 0.98,
                    planned_decel: 16.0,
                    cruise_speed: 1.0,
                    target_tier: MiniTurbo::Orange,
                    purple_when_possible: true,
                    min_drift_angle: 115f32.to_radians(),
                    drift_chance: 1.0,
                    start_delay_min: 0.0,
                    start_delay_max: 0.22,
                    trick_chance: 1.0,
                    item_delay: 0.5,
                    item_aim: 0.06,
                    item_shield: 0.9,
                },
            ],

            lane_spread: 1.5,
            lane_period_min: 3.0,
            lane_period_max: 6.0,
            lane_change_rate: 0.6,
            pace_spread: 0.02,

            road_margin: 0.4,
            sand_margin: 1.2,
            line_margin: 1.5,
            drift_turn_margin: 0.9,

            avoid_distance: 14.0,
            avoid_gap: 2.4,
            avoid_closing_speed: 0.5,
            avoid_rate: 5.0,

            catch_up_max: 0.03,
            catch_up_dead_zone: 15.0,
            catch_up_full_gap: 120.0,

            item_ahead_range: 35.0,
            item_behind_range: 12.0,
            item_shield_time: [2.0, 6.0],
            item_shot_period: 1.0,
            item_drop_period: 4.0,
            item_patience: 5.0,
            item_piment_straight: 45.0,
            item_box_pull: 2.5,
            item_box_range: 35.0,

            stuck_speed: 1.5,
            stuck_seconds: 0.6,
            reverse_seconds: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_game_design() {
        let t = BotTuning::default();
        // GD § 12: a light catch-up, at most 3 %.
        assert!(t.catch_up_max > 0.0 && t.catch_up_max <= 0.03);
        assert!(t.catch_up_dead_zone < t.catch_up_full_gap);
        // Harder = more precise, anticipates further, faster in turns, higher tier.
        for pair in t.skills.windows(2) {
            let (low, high) = (&pair[0], &pair[1]);
            assert!(high.line_fidelity >= low.line_fidelity);
            assert!(high.wobble < low.wobble);
            assert!(high.anticipation > low.anticipation);
            assert!(high.corner_margin > low.corner_margin);
            assert!(high.target_tier >= low.target_tier);
            assert!(high.drift_chance >= low.drift_chance);
            assert!(high.start_delay_max < low.start_delay_max);
            assert!(high.trick_chance >= low.trick_chance);
            // Issue #16: harder = uses its items sooner, aims better, shields more.
            assert!(high.item_delay < low.item_delay);
            assert!(high.item_aim < low.item_aim);
            assert!(high.item_shield > low.item_shield);
        }
        assert!(t.item_shield_time[0] < t.item_shield_time[1]);
        // Rocket start: never before the "2" (no stall); Hard always perfect (≤ 0.25 s).
        assert!(t.skills.iter().all(|s| s.start_delay_min >= 0.0));
        assert!(t.skill(BotDifficulty::Hard).start_delay_max <= 0.25);
        assert_eq!(t.skill(BotDifficulty::Easy).target_tier, MiniTurbo::Blue);
        assert_eq!(
            t.skill(BotDifficulty::Normal).target_tier,
            MiniTurbo::Orange
        );
        assert_eq!(t.skill(BotDifficulty::Hard).target_tier, MiniTurbo::Orange);
        assert!(t.skill(BotDifficulty::Hard).purple_when_possible);
        assert_eq!(BotDifficulty::default(), BotDifficulty::Normal);
    }
}
