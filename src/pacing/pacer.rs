//! Issue #234: the pure logic of the frame limiter: whether the frame the display offers at a
//! given time is drawn, for a cap of 60 or 30 images per second, or none. No Bevy here: the
//! tests drive it with simulated displays (60, 120, 144, 50 Hz).
//!
//! **Three ways**, chosen each frame from the display's interval `D` (estimated from the frames
//! it offers, or given) and the cap's interval `T`, with `r = T / D`:
//! - **The display is not faster than the cap** (`r ≤ 1 + ratio_tolerance`: 60 Hz or less for a
//!   cap of 60): every frame is drawn. Never fewer images than the display shows.
//! - **The display is a whole multiple of the cap** (`r` within `ratio_tolerance` of 2, 3...:
//!   120 Hz for a cap of 60, 120 or 60 Hz for 30): **locked on the display**, one frame every
//!   `round(r)`, timed from the last frame drawn. Regular, and it never drifts from the display
//!   (a 119.88 Hz screen gives 59.94 images, one frame out of two, never two in a row).
//! - **Otherwise** (144 Hz): **on a clock**. A frame is drawn once the time of the next image
//!   (`next_due`, which moves by exactly `T` at each image) is less than half a display frame
//!   away; the frames drawn alternate gaps of 2 and 3 display frames, 60 per second on average,
//!   without drift (the clock never takes the time of a frame drawn, only `T`).
//!
//! In each case, a frame is drawn at most once (the caller asks once per frame), and never two
//! images closer than half a display frame before their time: at most one image more than the
//! cap over any length of time.

/// How many images per second at most (the player's option, GD § 13).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bevy::reflect::Reflect,
)]
pub enum FrameCap {
    /// 60 images per second: the simulation's rate (by default).
    #[default]
    Sixty,
    /// 30 images per second: for a small machine, or to save the battery.
    Thirty,
    /// As many as the display shows.
    Unlimited,
}

impl FrameCap {
    /// The values in the order of the options' line.
    pub const ALL: [Self; 3] = [FrameCap::Sixty, FrameCap::Thirty, FrameCap::Unlimited];

    /// The time between two images (s), if there is a cap.
    pub fn interval(self) -> Option<f64> {
        match self {
            FrameCap::Sixty => Some(1.0 / 60.0),
            FrameCap::Thirty => Some(1.0 / 30.0),
            FrameCap::Unlimited => None,
        }
    }

    /// As written on the options' line.
    pub fn label(self) -> &'static str {
        match self {
            FrameCap::Sixty => "60",
            FrameCap::Thirty => "30",
            FrameCap::Unlimited => "Sans limite",
        }
    }
}

/// The tunings of the limiter (see [`super::FramePacing`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PacerTuning {
    /// How far the ratio cap interval / display interval may be from a whole number and still
    /// lock on the display (0.05: 120 Hz ± 3 Hz for a cap of 60).
    pub ratio_tolerance: f64,
    /// How early a frame may come, as a share of a display frame, and still be drawn (0.5: the
    /// frame nearest to the time of the image).
    pub early_share: f64,
}

impl Default for PacerTuning {
    fn default() -> Self {
        Self {
            ratio_tolerance: 0.05,
            early_share: 0.5,
        }
    }
}

/// How the limiter decides this frame (see the module's documentation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PacingWay {
    /// Every frame.
    Every,
    /// One frame out of `n`, locked on the display.
    Locked(u32),
    /// On a clock.
    Clock,
}

/// The display's interval, estimated from the frames it offers: it follows small changes
/// quickly and ignores the long gaps of a dropped frame (they move it a little only).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DisplayInterval {
    last: Option<f64>,
    estimate: Option<f64>,
}

impl DisplayInterval {
    /// A gap longer than this many times the estimate is a dropped frame.
    const DROPPED: f64 = 1.5;
    /// How much a usual gap moves the estimate...
    const FOLLOW: f64 = 0.1;
    /// ...and a dropped frame's.
    const FOLLOW_DROPPED: f64 = 0.01;
    /// Gaps outside of this (s) are not a display's (a paused tab, two calls at once).
    const SANE: (f64, f64) = (0.001, 0.25);

    /// A frame offered at `now` (s).
    pub fn observe(&mut self, now: f64) {
        let last = self.last.replace(now);
        let Some(gap) = last.map(|last| now - last) else {
            return;
        };
        if !(Self::SANE.0..=Self::SANE.1).contains(&gap) {
            return;
        }
        self.estimate = Some(match self.estimate {
            None => gap,
            Some(estimate) if gap > estimate * Self::DROPPED => {
                estimate + (gap - estimate) * Self::FOLLOW_DROPPED
            }
            Some(estimate) => estimate + (gap - estimate) * Self::FOLLOW,
        });
    }

    /// The display's interval (s), once two frames were seen.
    pub fn get(&self) -> Option<f64> {
        self.estimate
    }
}

/// The frame limiter (see the module's documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FramePacer {
    /// The display's interval: estimated from the frames offered (the browser), or given (a
    /// native window: its monitor's).
    pub display: DisplayInterval,
    /// Given: the display's interval (s), which wins over the estimate.
    pub known_display: Option<f64>,
    /// The time of the next image, on the clock.
    next_due: Option<f64>,
    /// The time of the last image drawn.
    last_drawn: Option<f64>,
}

impl FramePacer {
    /// The display's interval (s), if known.
    pub fn display_interval(&self) -> Option<f64> {
        self.known_display.or(self.display.get())
    }

    /// How a frame is decided, for `cap`.
    pub fn way(&self, cap: FrameCap, tuning: &PacerTuning) -> PacingWay {
        let Some(interval) = cap.interval() else {
            return PacingWay::Every;
        };
        let Some(display) = self.display_interval().filter(|d| *d > 0.0) else {
            return PacingWay::Clock;
        };
        let ratio = interval / display;
        let whole = ratio.round();
        if ratio <= 1.0 + tuning.ratio_tolerance {
            PacingWay::Every
        } else if (ratio - whole).abs() <= tuning.ratio_tolerance {
            PacingWay::Locked(whole as u32)
        } else {
            PacingWay::Clock
        }
    }

    /// The earliest time (s) the next image may be drawn (`None`: at once). A native window
    /// waits until then; the browser draws the first frame from then.
    pub fn earliest(&self, cap: FrameCap, tuning: &PacerTuning) -> Option<f64> {
        let interval = cap.interval()?;
        let early = self.display_interval().unwrap_or(0.0) * tuning.early_share;
        match self.way(cap, tuning) {
            PacingWay::Every => None,
            PacingWay::Locked(_) => self.last_drawn.map(|last| last + interval - early),
            PacingWay::Clock => self.next_due.map(|due| due - early),
        }
    }

    /// The frame the display offers at `now` (s): whether it is drawn (then it is counted as
    /// drawn). Asked once per frame. `observe`: the frames offered give the display's interval
    /// (the browser: every frame of the display is offered, drawn or not).
    pub fn offer(&mut self, now: f64, cap: FrameCap, tuning: &PacerTuning, observe: bool) -> bool {
        if observe {
            self.display.observe(now);
        }
        if self
            .earliest(cap, tuning)
            .is_some_and(|earliest| now < earliest)
        {
            return false;
        }
        self.drawn(now, cap, tuning);
        true
    }

    /// The image of `now` is drawn: the next one's time.
    fn drawn(&mut self, now: f64, cap: FrameCap, tuning: &PacerTuning) {
        self.last_drawn = Some(now);
        let Some(interval) = cap.interval() else {
            self.next_due = None;
            return;
        };
        self.next_due = Some(match (self.way(cap, tuning), self.next_due) {
            // On the clock: exactly one interval later, unless more than one behind (a long
            // frame, a paused tab): then from now, without a burst of images to catch up.
            (PacingWay::Clock, Some(due)) if due + interval > now => due + interval,
            _ => now + interval,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A display at `hz`, its frames offered for `seconds` to a browser-like limiter (every frame
    /// offered, the display estimated from them), with a jitter of up to `jitter` s on the
    /// times read (a small pseudo-random one, the same each run). Returns the times drawn.
    fn browser(hz: f64, cap: FrameCap, seconds: f64, jitter: f64) -> Vec<f64> {
        let tuning = PacerTuning::default();
        let mut pacer = FramePacer::default();
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut drawn = Vec::new();
        let frames = (seconds * hz).round() as u64;
        let mut seen = std::collections::HashSet::new();
        for frame in 0..frames {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = ((seed % 2001) as f64 / 1000.0 - 1.0) * jitter;
            let now = 10.0 + frame as f64 / hz + noise;
            if pacer.offer(now, cap, &tuning, true) {
                assert!(seen.insert(frame), "frame {frame} drawn twice");
                drawn.push(now);
            }
        }
        drawn
    }

    /// How many images in each whole second (after the first, while the display is learnt).
    fn per_second(drawn: &[f64]) -> Vec<usize> {
        let start = drawn[0].floor() + 1.0;
        let end = drawn[drawn.len() - 1].floor();
        (0..(end - start) as usize)
            .map(|s| {
                let from = start + s as f64;
                drawn
                    .iter()
                    .filter(|t| **t >= from && **t < from + 1.0)
                    .count()
            })
            .collect()
    }

    fn gaps(drawn: &[f64]) -> Vec<f64> {
        drawn.windows(2).skip(120).map(|w| w[1] - w[0]).collect()
    }

    /// 120 Hz, a cap of 60: one frame out of two, exactly, every gap the same.
    #[test]
    fn at_120_hz_one_frame_out_of_two_is_drawn() {
        for jitter in [0.0, 0.0005] {
            let drawn = browser(120.0, FrameCap::Sixty, 30.0, jitter);
            for count in per_second(&drawn) {
                assert!((59..=61).contains(&count), "{count} (jitter {jitter})");
            }
            for gap in gaps(&drawn) {
                assert!(
                    (gap - 2.0 / 120.0).abs() < 0.0015,
                    "{gap} (jitter {jitter})"
                );
            }
            assert!(drawn.len() >= 30 * 60 - 2 && drawn.len() <= 30 * 60 + 1);
        }
    }

    /// 144 Hz, a cap of 60: on a clock: 60 per second, gaps of 2 or 3 display frames, never
    /// two images closer than that.
    #[test]
    fn at_144_hz_a_clock_gives_sixty_images() {
        let drawn = browser(144.0, FrameCap::Sixty, 30.0, 0.0);
        for count in per_second(&drawn) {
            assert!((59..=61).contains(&count), "{count}");
        }
        for gap in gaps(&drawn) {
            let frames = (gap * 144.0).round();
            assert!(frames == 2.0 || frames == 3.0, "{gap}");
        }
        assert_eq!(
            FramePacer {
                known_display: Some(1.0 / 144.0),
                ..default()
            }
            .way(FrameCap::Sixty, &PacerTuning::default()),
            PacingWay::Clock
        );
    }

    fn default() -> FramePacer {
        FramePacer::default()
    }

    /// A display at the cap's rate or slower: every frame drawn, never fewer images than the
    /// display shows (a jitter on the times read does not lose any).
    #[test]
    fn a_display_at_60_hz_or_less_draws_every_frame() {
        for (hz, jitter) in [
            (60.0, 0.0),
            (60.0, 0.002),
            (50.0, 0.0),
            (59.94, 0.001),
            (30.0, 0.0),
        ] {
            let drawn = browser(hz, FrameCap::Sixty, 20.0, jitter);
            assert_eq!(drawn.len(), (20.0 * hz).round() as usize, "{hz} Hz");
        }
        // A cap of 30 at 30 Hz too.
        let drawn = browser(30.0, FrameCap::Thirty, 20.0, 0.0);
        assert_eq!(drawn.len(), 600);
    }

    /// A cap of 30: one frame out of two at 60 Hz, out of four at 120 Hz, a clock at 144 Hz.
    #[test]
    fn a_cap_of_thirty() {
        for (hz, frames) in [(60.0, 2.0), (120.0, 4.0)] {
            let drawn = browser(hz, FrameCap::Thirty, 20.0, 0.0002);
            for count in per_second(&drawn) {
                assert!((29..=31).contains(&count), "{count} at {hz} Hz");
            }
            for gap in gaps(&drawn) {
                assert_eq!((gap * hz).round(), frames, "{gap} at {hz} Hz");
            }
        }
        let drawn = browser(144.0, FrameCap::Thirty, 20.0, 0.0);
        for count in per_second(&drawn) {
            assert!((29..=31).contains(&count), "{count} at 144 Hz");
        }
        for gap in gaps(&drawn) {
            let frames = (gap * 144.0).round();
            assert!(frames == 4.0 || frames == 5.0, "{gap}");
        }
    }

    /// Without a cap: every frame of every display.
    #[test]
    fn without_a_cap_every_frame_is_drawn() {
        for hz in [50.0, 60.0, 120.0, 144.0, 240.0] {
            let drawn = browser(hz, FrameCap::Unlimited, 5.0, 0.0);
            assert_eq!(drawn.len(), (5.0 * hz).round() as usize, "{hz} Hz");
        }
    }

    /// Ten minutes: no drift. At 144 Hz the clock gives 60 per second to the image; at
    /// 119.88 Hz (a TV-like rate) the lock gives one frame out of two all along, never two in a
    /// row, where a clock would slip one frame every few seconds.
    #[test]
    fn no_drift_over_ten_minutes() {
        let seconds = 600.0;
        let drawn = browser(144.0, FrameCap::Sixty, seconds, 0.0);
        let total = drawn.len() as f64;
        assert!((total - seconds * 60.0).abs() <= 2.0, "{total}");
        let drawn = browser(119.88, FrameCap::Sixty, seconds, 0.0);
        for gap in gaps(&drawn) {
            assert_eq!((gap * 119.88).round(), 2.0, "{gap}");
        }
        let drawn = browser(120.0, FrameCap::Sixty, seconds, 0.0003);
        assert!((drawn.len() as f64 - seconds * 60.0).abs() <= 2.0);
    }

    /// Over any second, never more images than the cap (one at most at the edges).
    #[test]
    fn never_more_than_the_cap_in_any_second() {
        for hz in [75.0, 90.0, 100.0, 120.0, 144.0, 165.0, 240.0, 360.0] {
            let drawn = browser(hz, FrameCap::Sixty, 20.0, 0.0);
            for (i, start) in drawn.iter().enumerate().skip(60) {
                let within = drawn[i..].iter().take_while(|t| **t < start + 1.0).count();
                assert!(within <= 61, "{within} in a second at {hz} Hz");
            }
            let total = drawn.len() as f64;
            assert!(total <= 20.0 * 60.0 + 1.0, "{total} at {hz} Hz");
            assert!(total >= 20.0 * 60.0 - 6.0, "{total} at {hz} Hz");
        }
    }

    /// A long frame (a stall of 0.3 s): no burst of images to catch up after it.
    #[test]
    fn no_burst_after_a_stall() {
        let tuning = PacerTuning::default();
        let mut pacer = FramePacer::default();
        let mut drawn = Vec::new();
        let mut now: f64 = 0.0;
        for frame in 0..600 {
            now += if frame == 300 { 0.3 } else { 1.0 / 144.0 };
            if pacer.offer(now, FrameCap::Sixty, &tuning, true) {
                drawn.push(now);
            }
        }
        let after: Vec<f64> = drawn.iter().copied().filter(|t| *t > 2.0).collect();
        for gap in after.windows(2).map(|w| w[1] - w[0]) {
            assert!(gap > 1.5 / 144.0, "{gap}");
        }
    }

    /// A native window waits: its monitor's rate given, the time it waits until.
    #[test]
    fn a_native_window_waits_until_the_next_image() {
        let tuning = PacerTuning::default();
        // A 120 Hz monitor: locked, half a frame early (the vsync then shows it on time).
        let mut pacer = FramePacer {
            known_display: Some(1.0 / 120.0),
            ..default()
        };
        assert_eq!(pacer.earliest(FrameCap::Sixty, &tuning), None);
        assert!(pacer.offer(1.0, FrameCap::Sixty, &tuning, false));
        let earliest = pacer.earliest(FrameCap::Sixty, &tuning).unwrap();
        assert!((earliest - (1.0 + 1.0 / 60.0 - 0.5 / 120.0)).abs() < 1e-9);
        // A 60 Hz monitor: no wait (the vsync paces it).
        let pacer = FramePacer {
            known_display: Some(1.0 / 60.0),
            ..default()
        };
        assert_eq!(pacer.earliest(FrameCap::Sixty, &tuning), None);
        // An unknown monitor: a clock, exactly.
        let mut pacer = default();
        let mut now: f64 = 0.0;
        let mut drawn = 0;
        while now < 10.0 {
            if let Some(earliest) = pacer.earliest(FrameCap::Sixty, &tuning) {
                // The sleep is a little late, never early.
                now = now.max(earliest) + 0.0003;
            }
            assert!(pacer.offer(now, FrameCap::Sixty, &tuning, false));
            drawn += 1;
        }
        assert!((599..=601).contains(&drawn), "{drawn}");
    }

    #[test]
    fn the_caps_and_their_labels() {
        assert_eq!(FrameCap::default(), FrameCap::Sixty);
        assert_eq!(FrameCap::Sixty.interval(), Some(1.0 / 60.0));
        assert_eq!(FrameCap::Thirty.interval(), Some(1.0 / 30.0));
        assert_eq!(FrameCap::Unlimited.interval(), None);
        let labels: Vec<_> = FrameCap::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels, ["60", "30", "Sans limite"]);
    }
}
