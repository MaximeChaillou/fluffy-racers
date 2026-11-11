//! Issue #234: **the images per second capped at 60** (by default), in the browser as natively.
//! The simulation already runs at 60 Hz (`FixedUpdate`) and the render interpolates between two
//! ticks: drawing more images than that (120 on a ProMotion Mac, 144 on some screens) costs GPU,
//! CPU and battery for nothing. The player chooses in the options (« Images/seconde » : 60, 30 or
//! « Sans limite », `options::PlayerOptions::frame_cap`, kept in the browser); the value is
//! [`FramePacing::cap`], a tuning resource, also in the admin panel (« Test »).
//!
//! **When a frame is drawn** is pure logic (`pacer.rs`, tested with simulated displays): every
//! frame of a display not faster than the cap, one frame out of `n` on a whole multiple of it
//! (120 Hz: one out of two), a clock otherwise (144 Hz).
//!
//! **How a frame is not drawn** ([`install_frame_gate`]): the main app's update runs
//! [`PacedMain`] instead of Bevy's `Main`. Its one system asks the limiter, then runs `Main`
//! (the whole frame: `First` to `Last`, `FixedUpdate` with them) or not:
//! - **In the browser** ([`PacingMode::SkipFrames`]): winit runs an update at each
//!   `requestAnimationFrame`, the display's rate. A frame not drawn runs nothing: neither `Main`,
//!   nor the extraction to the render world, nor the render ([`PacedRender`]: the render app's
//!   update is gated the same way). The canvas keeps its image. The next frame drawn sees the
//!   time of both (`Time<Real>`): `FixedUpdate` runs the ticks due (60 per second whatever the
//!   cap) and the interpolation takes the share of the next tick, as before. Its clock is the
//!   frame's own time (`document.timeline.currentTime`, the time of the vsync), not the time the
//!   update started: steadier.
//! - **Natively** ([`PacingMode::Sleep`]): the window's loop runs as fast as its vsync
//!   (`PresentMode::AutoVsync`, kept): the update waits (sleeps) until the next image's time,
//!   half a display frame early (the vsync then shows it on time), then runs. On a monitor at
//!   the cap's rate or slower, it never waits. The monitor's rate is the window's (or the
//!   primary monitor's); unknown, a plain clock.
//!
//! The headless game (the tests, the bot races) and the dedicated server (#225, paced at 60 Hz
//! by its own loop, without a render) have no gate: [`FramePacingPlugin`] only installs it with
//! a renderer. The gate itself is tested headless with a simulated clock (`scenario_tests`).

mod pacer;

#[cfg(test)]
mod scenario_tests;

pub use pacer::{DisplayInterval, FrameCap, FramePacer, PacerTuning, PacingWay};

use bevy::ecs::intern::Interned;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::RenderApp;

/// The tuning of the images per second (issue #234): the cap and how the limiter locks on the
/// display. Its `cap` follows the player's option (`options::PlayerOptions::frame_cap`).
#[derive(Resource, Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct FramePacing {
    /// How many images per second at most: 60 by default (the simulation's rate).
    pub cap: FrameCap,
    /// How far the ratio of the cap's interval to the display's may be from a whole number and
    /// still lock on the display (0.05: 117 to 123 Hz is « one frame out of two » for 60).
    pub ratio_tolerance: f64,
    /// How early a frame may come, as a share of a display frame, and still be drawn (0.5: the
    /// frame nearest to the image's time).
    pub early_share: f64,
}

impl Default for FramePacing {
    fn default() -> Self {
        let tuning = PacerTuning::default();
        Self {
            cap: FrameCap::default(),
            ratio_tolerance: tuning.ratio_tolerance,
            early_share: tuning.early_share,
        }
    }
}

impl FramePacing {
    pub fn tuning(&self) -> PacerTuning {
        PacerTuning {
            ratio_tolerance: self.ratio_tolerance,
            early_share: self.early_share,
        }
    }
}

/// How the gate keeps a frame from being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacingMode {
    /// The browser: the frame runs nothing (see the module's documentation).
    SkipFrames,
    /// A native window: the update waits until the next image's time.
    Sleep,
}

impl PacingMode {
    /// The mode of this build: the browser's in wasm, a native window's otherwise.
    pub const fn of_this_build() -> Self {
        if cfg!(target_arch = "wasm32") {
            PacingMode::SkipFrames
        } else {
            PacingMode::Sleep
        }
    }
}

/// The time the gate reads (s).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub enum FrameClock {
    /// The frame's time: in the browser, the time of the animation frame
    /// (`document.timeline.currentTime`, or `performance.now()`); natively, since `start`.
    System { start: Instant },
    /// Set by hand (the tests): the gate never sleeps on it.
    Manual(f64),
}

impl Default for FrameClock {
    fn default() -> Self {
        FrameClock::System {
            start: Instant::now(),
        }
    }
}

impl FrameClock {
    pub fn now(&self) -> f64 {
        match *self {
            FrameClock::Manual(now) => now,
            FrameClock::System { start } => frame_time().unwrap_or_else(|| {
                Instant::now()
                    .saturating_duration_since(start)
                    .as_secs_f64()
            }),
        }
    }
}

/// The browser's time of the current animation frame (s): the vsync's, the same for the whole
/// frame.
#[cfg(target_arch = "wasm32")]
fn frame_time() -> Option<f64> {
    let time = web_sys::window()?.document()?.timeline().current_time()?;
    Some(time / 1000.0)
}

#[cfg(not(target_arch = "wasm32"))]
fn frame_time() -> Option<f64> {
    None
}

/// The gate's state in the main world: the limiter, the mode, and whether this frame is drawn.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FrameGate {
    pub pacer: FramePacer,
    pub mode: PacingMode,
    /// The current frame is drawn (read by the render app's gate).
    pub drawn: bool,
    /// Frames offered and drawn since the start.
    pub offered: u64,
    pub drawn_count: u64,
}

impl FrameGate {
    pub fn new(mode: PacingMode) -> Self {
        Self {
            pacer: FramePacer::default(),
            mode,
            drawn: true,
            offered: 0,
            drawn_count: 0,
        }
    }
}

/// The main app's update with the gate: runs `Main` or not (see the module's documentation).
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PacedMain;

/// The render app's update with the gate: runs Bevy's own update of the render app, only on a
/// frame drawn.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PacedRender;

/// In the render world: whether this frame is drawn, and the render app's own update.
#[derive(Resource, Clone, Copy, Debug)]
struct RenderGate {
    drawn: bool,
    inner: Interned<dyn ScheduleLabel>,
}

/// The tuning resource everywhere (the options and the admin panel change it); the gate only
/// with a renderer (see the module's documentation).
pub struct FramePacingPlugin;

impl Plugin for FramePacingPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<FramePacing>()
            .init_resource::<FramePacing>();
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            install_frame_gate(app, PacingMode::of_this_build());
            if PacingMode::of_this_build() == PacingMode::Sleep {
                app.add_systems(Last, follow_the_monitor);
            }
        }
    }

    fn finish(&self, app: &mut App) {
        // After the render plugin set its extraction (`build`): wrap it.
        if app
            .world()
            .get_resource::<FrameGate>()
            .is_some_and(|gate| gate.mode == PacingMode::SkipFrames)
        {
            gate_the_render(app);
        }
    }
}

/// Puts the gate on the main app's update (see the module's documentation): `mode` for how a
/// frame is not drawn, the time from [`FrameClock`] (the system's, unless one is there).
pub fn install_frame_gate(app: &mut App, mode: PacingMode) {
    app.init_resource::<FramePacing>()
        .init_resource::<FrameClock>()
        .insert_resource(FrameGate::new(mode))
        .add_schedule(Schedule::new(PacedMain))
        .add_systems(PacedMain, run_main_when_drawn);
    app.main_mut().update_schedule = Some(PacedMain.intern());
}

/// The one system of [`PacedMain`].
fn run_main_when_drawn(world: &mut World) {
    let pacing = *world.resource::<FramePacing>();
    let clock = *world.resource::<FrameClock>();
    let (cap, tuning) = (pacing.cap, pacing.tuning());
    let mut gate = *world.resource::<FrameGate>();
    let drawn = match gate.mode {
        PacingMode::SkipFrames => gate.pacer.offer(clock.now(), cap, &tuning, true),
        PacingMode::Sleep => {
            if let (Some(earliest), FrameClock::System { .. }) =
                (gate.pacer.earliest(cap, &tuning), clock)
            {
                let wait = earliest - clock.now();
                if wait > 0.0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(wait.min(0.1)));
                }
            }
            gate.pacer.offer(clock.now(), cap, &tuning, false);
            true
        }
    };
    gate.drawn = drawn;
    gate.offered += 1;
    gate.drawn_count += u64::from(drawn);
    *world.resource_mut::<FrameGate>() = gate;
    if drawn {
        world.run_schedule(Main);
    }
}

/// The render app's extraction and update only on a frame drawn (the browser).
fn gate_the_render(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    let Some(inner) = render_app.update_schedule else {
        return;
    };
    let mut extract = render_app.take_extract();
    render_app.set_extract(move |main, render| {
        let drawn = main
            .get_resource::<FrameGate>()
            .is_none_or(|gate| gate.drawn);
        render.insert_resource(RenderGate { drawn, inner });
        if drawn && let Some(extract) = extract.as_mut() {
            extract(main, render);
        }
    });
    render_app.insert_resource(RenderGate { drawn: true, inner });
    render_app.add_schedule(Schedule::new(PacedRender));
    render_app.add_systems(PacedRender, run_render_when_drawn);
    render_app.update_schedule = Some(PacedRender.intern());
}

/// The one system of [`PacedRender`].
fn run_render_when_drawn(world: &mut World) {
    let gate = *world.resource::<RenderGate>();
    if gate.drawn {
        world.run_schedule(gate.inner);
    }
}

/// Natively: the rate of the window's monitor (or the primary one's), for the limiter.
fn follow_the_monitor(
    windows: Query<&bevy::window::OnMonitor, With<bevy::window::PrimaryWindow>>,
    monitors: Query<&bevy::window::Monitor>,
    primary: Query<&bevy::window::Monitor, With<bevy::window::PrimaryMonitor>>,
    mut gate: ResMut<FrameGate>,
) {
    let monitor = windows
        .iter()
        .find_map(|on| monitors.get(on.0).ok())
        .or_else(|| primary.iter().next());
    let interval = monitor
        .and_then(|monitor| monitor.refresh_rate_millihertz)
        .filter(|millihertz| *millihertz > 0)
        .map(|millihertz| 1000.0 / f64::from(millihertz));
    if gate.pacer.known_display != interval {
        gate.pacer.known_display = interval;
    }
}
