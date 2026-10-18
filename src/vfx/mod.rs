//! Visual effects (GD § 14 « juice », issue #105): CPU particles, pooled, **only with a
//! renderer**.
//!
//! - **The drift** (GD § 3): sparks out of the rear wheels that grow with the charge (more of
//!   them, bigger, longer, faster at each tier, in its color: blue, orange, purple), a flash at
//!   each tier (a burst of sparks, a white flash at each wheel, a ring of its color), the tyres'
//!   smoke; the « fwoosh » of the release (a burst out of the exhaust, a flash, a ring blown out
//!   behind) and the trail of its color while the mini-turbo lasts.
//! - **The boosts** (GD § 2, one look per source): embers out of the exhausts that follow the
//!   flames (`kart::flame_size`: the boost's strength and its time left), the flames in the
//!   source's color; a cloud of smoke at a rocket start, gold sparkles for a trick, wind for the
//!   slipstream, red sparks for a piment, cyan for the spin boost and the anti-gravity's pads,
//!   orange for a pad; the camera's kart gets speed lines, a wider field of view and a punch of
//!   it when a boost starts.
//! - **The shocks and the items**: stars and white « pof » where two karts bump and on a wall,
//!   stars on a kart struck by a paw, the confetti of the explosions, the chantilly's cream, the
//!   stars over the head of a kart spinning out or tumbling (and a while after: « sonné »), the
//!   player's camera shaking; gold sparkles for a croquette picked up or lost.
//! - **The ground** (GD § 10): dust off the road, dust blown out by a landing, the anti-gravity's
//!   cyan sparks, the glider's wind off its wing tips.
//!
//! **Fluidity first**: a fixed pool of [`ParticlePool::SLOTS`] slots (`pool.rs`), each with its
//! entity made once at startup, sharing 4 meshes and 19 materials; a global cap that the pool
//! never exceeds ([`VfxTuning::max_particles`], the oldest particle gives its slot up); nothing
//! thrown by a kart farther than `emit_range` from the camera; no shadow cast; nothing allocated
//! while racing (`alloc_tests.rs` counts it). No compute shader: the particles are plain meshes
//! (WebGL2).
//!
//! **Purely visual**: the plugin reads the game, never writes to it (but the camera: its field of
//! view, its shake); it is added by `GamePlugin` only with a renderer (the tests add it by hand):
//! the bot races' world has none of it, and stays the one they were measured with.

mod cues;
mod emit;
mod pool;
mod systems;
mod tuning;

#[cfg(test)]
mod alloc_tests;
#[cfg(test)]
mod scenario_tests;

pub use cues::{
    BoostSource, Bursts, SparkStyle, TickEvents, VfxSnapshot, boost_source, ember_paint, fov_punch,
    kart_bursts, spark_style, tier_progress,
};
pub use emit::{KartFrame, take};
pub use pool::{Curve, Facing, Paint, Particle, ParticlePool, Shape, Sparkle};
pub use systems::{
    DizzyStar, FovKick, KartVfx, MAX_DIZZY_STARS, Particles, VfxAssets, VfxParticle, VfxQueue,
    WorldBurst, star_mesh,
};
pub use tuning::{BoostVfx, DriftVfx, ImpactVfx, VfxColors, VfxTuning, WorldVfx};

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::admin::Tunings;
use crate::interpolation::InterpolationSystems;

/// The particles' systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum VfxSystems {
    /// Each tick: what the karts and the rules did (`FixedPostUpdate`).
    Watch,
    /// Each frame: throw, move and show (`PostUpdate`, the karts and the camera placed).
    Show,
}

/// The visual effects (see the module's documentation). Only with a renderer.
pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VfxTuning>()
            .init_resource::<VfxQueue>()
            .init_resource::<FovKick>()
            .add_systems(Startup, systems::create_vfx)
            .add_systems(
                FixedPostUpdate,
                (
                    systems::remember_karts,
                    systems::watch_karts,
                    systems::watch_world,
                )
                    .chain()
                    .in_set(VfxSystems::Watch),
            )
            .add_systems(
                PostUpdate,
                (
                    systems::follow_tuning,
                    systems::emit_karts,
                    systems::emit_world,
                    systems::kick_the_camera,
                    systems::show_particles,
                    systems::show_dizzy_stars,
                    systems::tint_flames,
                )
                    .chain()
                    .in_set(VfxSystems::Show)
                    .run_if(resource_exists::<Particles>),
            )
            .configure_sets(
                PostUpdate,
                VfxSystems::Show
                    .after(InterpolationSystems)
                    // The camera placed (and the home's trailer, and the water's tint after it):
                    // the field of view goes on top, the particles face it.
                    .after(crate::camera::tint_under_water)
                    .before(TransformSystems::Propagate),
            );
        // Its tuning in the admin panel, next to the others (the game's).
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<VfxTuning>();
        }
    }
}
