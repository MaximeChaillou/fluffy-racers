//! The « soft » animations of the screens (issue #56, `docs/UI_CHARTE.md` « Animations molles »),
//! pure: a panel that pops in with a bounce, the buttons after it one by one, the focused button
//! that grows a little, and the squash and stretch of a click. Short, so that a screen never
//! makes the player wait: the longest settles in under half a second.
//!
//! Each one is an underdamped spring started from rest ([`spring`]): it overshoots and comes back,
//! like jelly. They run on real time (`Time<Real>`): the game is paused behind a screen in solo.
//! Not tuning values of the gameplay: animation constants, like the HUD's (`ui::pop`).

use std::f32::consts::TAU;

use bevy::math::Vec2;

/// The panel pops in: from this scale...
pub const POP_FROM: f32 = 0.7;
/// ...to 1, with this spring (Hz, damping ratio: about 6 % of overshoot)...
pub const POP_HZ: f32 = 3.2;
pub const POP_DAMPING: f32 = 0.45;
/// ...and is exactly 1 after this long (s).
pub const POP_TIME: f32 = 0.45;
/// Each button pops in this long after the one above it (s)...
pub const STAGGER: f32 = 0.045;
/// ...from this scale, with the panel's spring.
pub const BUTTON_POP_FROM: f32 = 0.5;
/// The veil over the game fades in this long (s).
pub const VEIL_TIME: f32 = 0.18;

/// The focused button grows to this scale, with this spring (Hz, damping), in this long (s)...
pub const FOCUS_SCALE: f32 = 1.05;
pub const FOCUS_HZ: f32 = 4.0;
pub const FOCUS_DAMPING: f32 = 0.5;
pub const FOCUS_TIME: f32 = 0.35;
/// ...and breathes while it keeps the focus: this much (share of its scale), this slowly (s).
pub const BREATH: f32 = 0.012;
pub const BREATH_PERIOD: f32 = 1.4;

/// Pressed, a button squashes to this height (share)...
pub const SQUASH: f32 = 0.86;
/// ...quickly (Hz, damping: no overshoot to speak of)...
pub const PRESS_HZ: f32 = 9.0;
pub const PRESS_DAMPING: f32 = 0.8;
/// ...and widens by this share of what its height lost (half the volume kept: a full one would
/// make a wide button far too wide).
pub const SQUASH_WIDEN: f32 = 0.5;
/// Released, it springs back, stretched past its height (about 5 %), then settles (Hz, damping,
/// s).
pub const RELEASE_HZ: f32 = 3.5;
pub const RELEASE_DAMPING: f32 = 0.3;
pub const RELEASE_TIME: f32 = 0.5;
/// What it does comes this long after it is released (s): the stretch is seen first.
pub const ACTIVATE_DELAY: f32 = 0.12;

/// A spring at rest at `from` when `t` = 0, pulled to `to`: where it is `t` seconds later, for a
/// frequency of `hz` and a damping ratio `damping` (under 1: it overshoots).
pub fn spring(t: f32, from: f32, to: f32, hz: f32, damping: f32) -> f32 {
    if t <= 0.0 {
        return from;
    }
    let damping = damping.clamp(0.01, 0.99);
    let omega = TAU * hz;
    let damped = omega * (1.0 - damping * damping).sqrt();
    let decay = (-damping * omega * t).exp();
    let wave = (damped * t).cos() + damping / (1.0 - damping * damping).sqrt() * (damped * t).sin();
    to + (from - to) * decay * wave
}

/// The scale of the panel `t` seconds after the screen opened: [`POP_FROM`], a bounce past 1,
/// exactly 1 from [`POP_TIME`] on.
pub fn panel_pop(t: f32) -> f32 {
    pop(t, POP_FROM)
}

/// The scale of the `index`-th button (from the top) `t` seconds after the screen opened: the
/// same bounce, one after the other.
pub fn button_pop(t: f32, index: usize) -> f32 {
    let t = t - STAGGER * index as f32;
    if t <= 0.0 {
        return BUTTON_POP_FROM;
    }
    pop(t, BUTTON_POP_FROM)
}

fn pop(t: f32, from: f32) -> f32 {
    if t >= POP_TIME {
        return 1.0;
    }
    spring(t, from, 1.0, POP_HZ, POP_DAMPING)
}

/// The veil's opacity (share of its own) `t` seconds after the screen opened.
pub fn veil(t: f32) -> f32 {
    (t / VEIL_TIME).clamp(0.0, 1.0)
}

/// The scale a button's focus gives it, `t` seconds after it got the focus (`focused`) or lost
/// it: up to [`FOCUS_SCALE`] with a small bounce, then breathing; back to 1.
pub fn focus_scale(focused: bool, t: f32) -> f32 {
    let (from, to) = if focused {
        (1.0, FOCUS_SCALE)
    } else {
        (FOCUS_SCALE, 1.0)
    };
    if t >= FOCUS_TIME {
        if !focused {
            return 1.0;
        }
        let breath = (TAU * (t - FOCUS_TIME) / BREATH_PERIOD).sin();
        return FOCUS_SCALE * (1.0 + BREATH * breath);
    }
    spring(t, from, to, FOCUS_HZ, FOCUS_DAMPING)
}

/// The height of a button (share) `t` seconds after it was pressed: down to [`SQUASH`].
pub fn press_height(t: f32) -> f32 {
    spring(t, 1.0, SQUASH, PRESS_HZ, PRESS_DAMPING)
}

/// The height of a button (share) `t` seconds after it was released at the height `from`: back
/// up, stretched past 1, then exactly 1 from [`RELEASE_TIME`] on.
pub fn release_height(t: f32, from: f32) -> f32 {
    if t >= RELEASE_TIME {
        return 1.0;
    }
    spring(t, from, 1.0, RELEASE_HZ, RELEASE_DAMPING)
}

/// A button's scale for its height `height` (share): it widens when it squashes and thins when it
/// stretches ([`SQUASH_WIDEN`] of the volume kept).
pub fn squash_and_stretch(height: f32) -> Vec2 {
    let height = height.max(0.1);
    Vec2::new(1.0 + (1.0 / height - 1.0) * SQUASH_WIDEN, height)
}

/// Issue #89: the home screen's title is alive. Each of its two words leans its own way
/// (clockwise, rad: « Fluffy » up to the right, « Racers » a little down)...
pub const TITLE_LEAN: [f32; 2] = [-0.05, 0.03];
/// ...and bobs this much (px of the reference window) and tilts this much more (rad), slowly
/// (s), the second word half a period after the first: like a marshmallow on jelly.
pub const TITLE_BOB: f32 = 4.0;
pub const TITLE_TILT: f32 = 0.03;
pub const TITLE_PERIOD: f32 = 2.8;

/// How far down (px of the reference window) and how turned (clockwise, rad) the `word`-th word
/// of the home's title is, `t` seconds after the screen opened.
pub fn title_word(t: f32, word: usize) -> (f32, f32) {
    let phase = TAU * t / TITLE_PERIOD + std::f32::consts::PI * word as f32;
    let lean = TITLE_LEAN[word.min(TITLE_LEAN.len() - 1)];
    (
        TITLE_BOB * phase.sin(),
        lean + TITLE_TILT * (phase + 0.6).sin(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples `f` every 1/240 s over `seconds`.
    fn samples(seconds: f32, f: impl Fn(f32) -> f32) -> Vec<f32> {
        (0..=(seconds * 240.0) as usize)
            .map(|i| f(i as f32 / 240.0))
            .collect()
    }

    #[test]
    fn a_spring_starts_at_rest_and_ends_where_it_is_pulled() {
        assert_eq!(spring(0.0, 0.3, 1.0, 3.0, 0.5), 0.3);
        assert_eq!(spring(-1.0, 0.3, 1.0, 3.0, 0.5), 0.3);
        assert!((spring(5.0, 0.3, 1.0, 3.0, 0.5) - 1.0).abs() < 1e-4);
        // At rest at the start: a tiny step barely moves it.
        assert!((spring(0.001, 0.3, 1.0, 3.0, 0.5) - 0.3).abs() < 0.01);
        // Underdamped: it goes past where it is pulled, once.
        let peak = samples(1.0, |t| spring(t, 0.0, 1.0, 3.0, 0.3))
            .into_iter()
            .fold(0.0f32, f32::max);
        assert!(peak > 1.2 && peak < 1.5, "{peak}");
    }

    /// Issue #56: the panel bounces in (a small overshoot, then a dip) and is exactly 1 at the
    /// end, in under half a second; each button after the one above it.
    #[test]
    fn the_panel_and_its_buttons_pop_in_with_a_bounce() {
        assert_eq!(panel_pop(0.0), POP_FROM);
        assert_eq!(panel_pop(POP_TIME), 1.0);
        assert_eq!(panel_pop(10.0), 1.0);
        const { assert!(POP_TIME <= 0.5) };
        let curve = samples(POP_TIME, panel_pop);
        let peak = curve.iter().copied().fold(0.0f32, f32::max);
        assert!(peak > 1.03 && peak < 1.1, "a soft bounce: {peak}");
        let first_peak = curve.iter().position(|s| *s == peak).unwrap();
        let dip = curve[first_peak..].iter().copied().fold(2.0f32, f32::min);
        assert!(dip < 1.0 && dip > 0.97, "then a small dip: {dip}");
        // No jump at the end.
        assert!((panel_pop(POP_TIME - 1e-3) - 1.0).abs() < 0.015);
        for index in 0..5 {
            let start = STAGGER * index as f32;
            assert_eq!(button_pop(start, index), BUTTON_POP_FROM);
            assert_eq!(button_pop(start + POP_TIME + 1e-4, index), 1.0);
            assert!(button_pop(start + 0.1, index) > BUTTON_POP_FROM);
        }
        assert!(button_pop(0.1, 0) > button_pop(0.1, 3), "the top one first");
        // The fifth one is there quickly.
        const { assert!(STAGGER * 4.0 + POP_TIME < 0.7) };
        assert_eq!(veil(0.0), 0.0);
        assert_eq!(veil(VEIL_TIME), 1.0);
        assert_eq!(veil(1.0), 1.0);
    }

    #[test]
    fn the_focus_grows_a_button_with_a_bounce_then_it_breathes() {
        assert_eq!(focus_scale(true, 0.0), 1.0);
        assert_eq!(focus_scale(false, 0.0), FOCUS_SCALE);
        let grow = samples(FOCUS_TIME, |t| focus_scale(true, t));
        let peak = grow.iter().copied().fold(0.0f32, f32::max);
        assert!(peak > FOCUS_SCALE && peak < FOCUS_SCALE + 0.02, "{peak}");
        assert!((focus_scale(true, FOCUS_TIME - 1e-3) - FOCUS_SCALE).abs() < 0.005);
        // Then it breathes, barely, around its scale.
        let breath = samples(3.0, |t| focus_scale(true, FOCUS_TIME + t));
        let (low, high) = breath
            .iter()
            .fold((2.0f32, 0.0f32), |(l, h), s| (l.min(*s), h.max(*s)));
        assert!(low < FOCUS_SCALE && high > FOCUS_SCALE);
        assert!(high - low <= 2.0 * BREATH * FOCUS_SCALE + 1e-4);
        // Losing the focus: back to exactly 1.
        assert_eq!(focus_scale(false, FOCUS_TIME), 1.0);
        assert!(focus_scale(false, 0.05) < FOCUS_SCALE);
    }

    /// Issue #89: the home's title moves a little, slowly, its two words out of step, never far
    /// from their lean.
    #[test]
    fn the_home_title_bobs_slowly_its_words_out_of_step() {
        let curve = |word: usize| samples(TITLE_PERIOD, move |t| title_word(t, word).0);
        for (word, lean) in TITLE_LEAN.iter().enumerate() {
            let (low, high) = curve(word)
                .iter()
                .fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(*y), h.max(*y)));
            assert!(high > 0.9 * TITLE_BOB && low < -0.9 * TITLE_BOB);
            assert!(high <= TITLE_BOB + 1e-4 && low >= -TITLE_BOB - 1e-4);
            for t in [0.0, 0.7, 1.9, 5.0] {
                let (_, turn) = title_word(t, word);
                assert!((turn - lean).abs() <= TITLE_TILT + 1e-5, "{turn}");
                // Periodic.
                let (a, b) = (title_word(t, word), title_word(t + TITLE_PERIOD, word));
                assert!((a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-4);
            }
        }
        // Out of step: when one is up, the other is down.
        let quarter = TITLE_PERIOD / 4.0;
        assert!(title_word(quarter, 0).0 > 0.9 * TITLE_BOB);
        assert!(title_word(quarter, 1).0 < -0.9 * TITLE_BOB);
        // Slow: never more than 12 px a second (a gentle bob, behind the buttons' bounces).
        let fastest = samples(TITLE_PERIOD, |t| {
            (title_word(t + 1.0 / 240.0, 0).0 - title_word(t, 0).0).abs() * 240.0
        })
        .into_iter()
        .fold(0.0f32, f32::max);
        assert!(fastest < 12.0, "{fastest}");
    }

    /// Issue #56: a click squashes the button (wider, lower), and its release stretches it past
    /// its height before it settles; quickly.
    #[test]
    fn a_click_squashes_then_stretches_the_button() {
        assert_eq!(press_height(0.0), 1.0);
        let pressed = press_height(0.1);
        assert!(
            (pressed - SQUASH).abs() < 0.02,
            "squashed in 0.1 s: {pressed}"
        );
        let squashed = squash_and_stretch(pressed);
        assert!(squashed.x > 1.0 && squashed.y < 1.0, "{squashed}");
        assert_eq!(squash_and_stretch(1.0), Vec2::ONE);
        // Released from the squash: up past 1 (stretched: thinner), back to exactly 1.
        let release = samples(RELEASE_TIME, |t| release_height(t, SQUASH));
        let peak = release.iter().copied().fold(0.0f32, f32::max);
        assert!(peak > 1.03 && peak < 1.1, "stretched: {peak}");
        let stretched = squash_and_stretch(peak);
        assert!(stretched.x < 1.0, "{stretched}");
        assert_eq!(release_height(RELEASE_TIME, SQUASH), 1.0);
        assert!((release_height(RELEASE_TIME - 1e-3, SQUASH) - 1.0).abs() < 0.02);
        assert_eq!(release_height(0.0, 0.9), 0.9, "from where it was released");
        // The stretch is seen before the screen changes.
        let at_activation = release_height(ACTIVATE_DELAY, SQUASH);
        assert!(at_activation > 1.0, "{at_activation}");
        const { assert!(ACTIVATE_DELAY <= 0.15) };
        // Half the volume is kept: never wider than a third more, even squashed far past
        // what a press does.
        for height in [0.6, SQUASH, 1.0, 1.1] {
            let scale = squash_and_stretch(height);
            assert!(scale.x <= 1.34 && scale.x > 0.9, "{height}: {scale}");
        }
    }
}
