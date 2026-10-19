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

    // --- Shortcuts (issue #107, GD « Conception des circuits ») -----------------------------------
    /// Chance to take a road shortcut (decided once per pass)...
    pub shortcut_chance: f32,
    /// ...a grass one, only with a boost long enough to reach it and cross part of it
    /// (`BotTuning::grass_boost_share`)...
    pub grass_chance: f32,
    /// ...and a risky jump, only fast enough (`BotTuning::jump_speed_share`). GD: only the Hard
    /// bots try the jumps.
    pub jump_chance: f32,

    // --- Items (GD § 8 and 12, issue #16) -------------------------------------------------------
    /// It waits this long once an item is ready before using it (s): how often it uses items.
    pub item_delay: f32,
    /// It throws a pelote at a kart ahead within this angle of its heading (rad): its aim.
    pub item_aim: f32,
    /// Chance to drag a single pelure or pelote behind as a shield before letting it go.
    pub item_shield: f32,

    // --- Interactions between karts (GD § 15, issue #34) ----------------------------------------
    /// Share of the occasions (a kart coming in reach, the paw ready) it strikes at (GD: 10 / 30
    /// / 60 %).
    pub strike_chance: f32,
    /// Preference for pushing toward the nearer edge of the track (GD: Hard): the chance is
    /// `× (1 + bias)` for a kart on that side, `× (1 − bias)` for one on the other side.
    pub strike_edge_bias: f32,
    /// Chance to grab a faster kart passing it (GD: Easy never, Normal sometimes, Hard to get
    /// towed)...
    pub grab_chance: f32,
    /// ...held this long before letting go (s)...
    pub grab_hold: f32,
    /// ...and then thrown away from itself (GD: Hard).
    pub throw: bool,
    /// Held by another kart, it jumps free after this long (s, GD: about 1 / 0.5 / 0.25 s).
    pub escape_delay: f32,
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
    /// Issue #107: and this much farther inside a side without wall (m, a drop: GD « Conception
    /// des circuits »): a bump there would send it over (on the Corniche Cannelle, most of the
    /// falls of a pack).
    pub void_margin: f32,
    /// The racing line keeps this much more room on each side (m), for the lanes, the wobble
    /// and the overtaking.
    pub line_margin: f32,
    /// Issue #144: a kart's center keeps this far from an obstacle or a passer-by (m, from the
    /// ground it may cover: its whole motion)...
    pub hazard_margin: f32,
    /// ...from this far before it to this far after it along the track (m): the bots aim 13 to
    /// 15 m ahead at full speed, the lane must stay clear until the kart itself has passed.
    pub hazard_before: f32,
    pub hazard_after: f32,
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
    // Issue #18.
    /// A bombe thrown ahead lands about 13 m ahead and blows 6 m around: it goes at a kart this
    /// far ahead (m), within `BotSkill::item_aim` × `item_bomb_cone`.
    pub item_bomb_range: [f32; 2],
    pub item_bomb_cone: f32,
    /// A piment doré's boosts: the next one when the last one is over, with at least this share
    /// of `item_piment_straight` ahead.
    pub item_golden_straight: f32,
    /// A super câlin goes when a kart is this close (m), to knock it over (or on a straight).
    pub item_ram_range: f32,
    /// The klaxon-canard sounds when another kart's item comes this close (m: about its reach).
    pub item_danger_range: f32,
    /// Creamed (the chantilly), the bot drives this much less precisely: its wobble's amplitude
    /// grows by this much (m).
    pub creamed_wobble: f32,

    // --- Interactions between karts (GD § 15, issue #34) ----------------------------------------
    /// It never goes for the same kart twice within this long (s, GD: 5 s).
    pub interaction_retarget: f32,
    /// A kart counts as "faster, passing" (worth grabbing) when it goes at least this much
    /// faster along the bot's heading (m/s): a boost, mostly.
    pub grab_min_speed_gain: f32,

    // --- Shortcuts (issue #107) -----------------------------------------------------------------
    /// A bot decides whether it takes a shortcut this far before it (m), and moves to its lane.
    pub shortcut_lookahead: f32,
    /// It takes a grass shortcut only with a boost that lasts the way to it and at least this
    /// share of its path, at its speed of the moment. 0.35: on the Corniche Cannelle, an orange
    /// mini-turbo fired 20 m before the grass bridge (about 1 s gained), not a boost pad's 1 s
    /// fired 9 m before it (about nothing gained).
    pub grass_boost_share: f32,
    /// It tries a risky jump only at this share of its top speed or faster, from the decision
    /// to the ramp.
    pub jump_speed_share: f32,

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

    /// Issue #144: how the racing line keeps clear of the obstacles and the passers-by.
    pub fn hazard_clearance(&self) -> super::line::HazardClearance {
        super::line::HazardClearance {
            margin: self.hazard_margin,
            before: self.hazard_before,
            after: self.hazard_after,
        }
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
                    shortcut_chance: 0.25,
                    grass_chance: 0.25,
                    jump_chance: 0.0,
                    item_delay: 2.5,
                    item_aim: 0.18,
                    item_shield: 0.2,
                    strike_chance: 0.1,
                    strike_edge_bias: 0.0,
                    grab_chance: 0.0,
                    grab_hold: 1.0,
                    throw: false,
                    escape_delay: 1.0,
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
                    shortcut_chance: 0.6,
                    grass_chance: 0.7,
                    jump_chance: 0.0,
                    item_delay: 1.2,
                    item_aim: 0.1,
                    item_shield: 0.6,
                    strike_chance: 0.3,
                    strike_edge_bias: 0.0,
                    grab_chance: 0.3,
                    grab_hold: 1.2,
                    throw: false,
                    escape_delay: 0.5,
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
                    shortcut_chance: 0.9,
                    grass_chance: 1.0,
                    jump_chance: 0.6,
                    item_delay: 0.5,
                    item_aim: 0.06,
                    item_shield: 0.9,
                    strike_chance: 0.6,
                    strike_edge_bias: 0.33,
                    grab_chance: 0.7,
                    grab_hold: 1.6,
                    throw: true,
                    escape_delay: 0.25,
                },
            ],

            lane_spread: 1.5,
            lane_period_min: 3.0,
            lane_period_max: 6.0,
            lane_change_rate: 0.6,
            pace_spread: 0.02,

            road_margin: 0.4,
            sand_margin: 1.2,
            void_margin: 1.5,
            line_margin: 1.5,
            hazard_margin: 1.6,
            hazard_before: 8.0,
            hazard_after: 18.0,
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
            item_bomb_range: [7.0, 24.0],
            item_bomb_cone: 3.0,
            item_golden_straight: 0.5,
            item_ram_range: 12.0,
            item_danger_range: 9.0,
            creamed_wobble: 2.5,

            interaction_retarget: 5.0,
            grab_min_speed_gain: 2.0,

            shortcut_lookahead: 45.0,
            grass_boost_share: 0.35,
            jump_speed_share: 0.9,

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
            // Issue #107: harder = takes more shortcuts.
            assert!(high.shortcut_chance > low.shortcut_chance);
            assert!(high.grass_chance > low.grass_chance);
            assert!(high.jump_chance >= low.jump_chance);
            // Issue #16: harder = uses its items sooner, aims better, shields more.
            assert!(high.item_delay < low.item_delay);
            assert!(high.item_aim < low.item_aim);
            assert!(high.item_shield > low.item_shield);
            // Issue #34: harder = strikes and grabs more, breaks free sooner.
            assert!(high.strike_chance > low.strike_chance);
            assert!(high.grab_chance >= low.grab_chance);
            assert!(high.escape_delay < low.escape_delay);
        }
        // GD § 15: 10 / 30 / 60 % of the occasions; Easy never grabs, Hard throws and prefers
        // the edge; they break free after about 1 / 0.5 / 0.25 s; never the same kart within 5 s.
        let strikes: Vec<f32> = t.skills.iter().map(|s| s.strike_chance).collect();
        assert_eq!(strikes, [0.1, 0.3, 0.6]);
        assert_eq!(t.skill(BotDifficulty::Easy).grab_chance, 0.0);
        assert!(t.skill(BotDifficulty::Normal).grab_chance > 0.0);
        let hard = t.skill(BotDifficulty::Hard);
        assert!(hard.throw && hard.strike_edge_bias > 0.0);
        assert!(!t.skill(BotDifficulty::Normal).throw);
        let escapes: Vec<f32> = t.skills.iter().map(|s| s.escape_delay).collect();
        assert_eq!(escapes, [1.0, 0.5, 0.25]);
        assert_eq!(t.interaction_retarget, 5.0);
        // A grab is held less than the 2 s a tether lasts.
        assert!(t.skills.iter().all(|s| s.grab_hold < 2.0));
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
        // Issue #107 (GD « Conception des circuits »): only the Hard bots try the risky jumps.
        let jumps: Vec<f32> = t.skills.iter().map(|s| s.jump_chance).collect();
        assert_eq!(jumps[..2], [0.0, 0.0]);
        assert!(jumps[2] > 0.0);
        assert!(t.grass_boost_share > 0.0 && t.jump_speed_share > 0.5);
    }
}
