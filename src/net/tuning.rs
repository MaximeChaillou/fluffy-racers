//! The kart synchronization's tuning (issue #8), in the admin panel like every other one.

use bevy::prelude::*;

use super::interp::{PlaybackSettings, SampleLimits, TICK_MS};
use super::netsim::NetSimTuning;

/// How the other peers' karts are shown (`docs/ARCHITECTURE.md` § « Synchronisation »), and the
/// network simulator of the debug. Reflected (`Reflect`) for the admin panel (issue #36).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct NetTuning {
    /// A remote kart is shown this long after its fastest states arrive (ms): the buffer that
    /// absorbs the jitter and a lost state or two (issue #8: about 100 ms).
    pub interpolation_delay_ms: f32,
    /// Past its newest state, a remote kart carries on with its velocities this long at most
    /// (ms), then stays where it got to.
    pub max_extrapolation_ms: f32,
    /// Two states further apart than a kart driving this fast could be (m/s) are a teleport (the
    /// tow pelican, a reset): no slide between them.
    pub teleport_speed: f32,
    /// The lag of a remote kart (how late its states arrive) is the minimum of the last this
    /// long (ms).
    pub lag_window_ms: f32,
    /// When that lag changes, the remote kart is shown up to this much faster or slower until
    /// it catches up (0.1: 10 %)...
    pub playback_correction: f32,
    /// ...or jumps, beyond this (ms).
    pub playback_snap_ms: f32,
    /// The collider of a remote kart is moved by its velocity, never faster than its state's
    /// speed plus this (m/s): a correction never throws our kart around...
    pub proxy_speed_margin: f32,
    /// ...but it is put in place at once when further than this from where it should be (m).
    pub proxy_snap_distance: f32,
    /// Our kart's state goes out every this many ticks (2: 30 per second).
    pub send_interval_ticks: u32,
    /// The network simulator (debug, `?netsim=120,20,5`): off by default.
    pub simulator: NetSimTuning,
}

impl Default for NetTuning {
    fn default() -> Self {
        Self {
            interpolation_delay_ms: 100.0,
            max_extrapolation_ms: 250.0,
            teleport_speed: 80.0,
            lag_window_ms: 2_000.0,
            playback_correction: 0.1,
            playback_snap_ms: 250.0,
            proxy_speed_margin: 5.0,
            proxy_snap_distance: 4.0,
            send_interval_ticks: 2,
            simulator: NetSimTuning::default(),
        }
    }
}

impl NetTuning {
    pub fn playback(&self) -> PlaybackSettings {
        PlaybackSettings {
            delay_ms: f64::from(self.interpolation_delay_ms.max(0.0)),
            window_ms: f64::from(self.lag_window_ms.max(1.0)),
            correction: f64::from(self.playback_correction.clamp(0.0, 1.0)),
            snap_ms: f64::from(self.playback_snap_ms.max(0.0)),
        }
    }

    pub fn limits(&self) -> SampleLimits {
        SampleLimits {
            max_extrapolation_ticks: f64::from(self.max_extrapolation_ms.max(0.0)) / TICK_MS,
            teleport_speed: self.teleport_speed.max(1.0),
        }
    }
}
