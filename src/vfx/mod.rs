//! Visual effects (GD § 14 « juice », issue #105): CPU particles, pooled, **only with a
//! renderer**.
//!
//! - **The drift** (GD § 3; redone by issue #174: glowing sparks that bounce and cool, flames
//!   licking the rear wheels from orange on, embers in purple, a halo on the ground, a little
//!   jump at each tier, a blast of flames at the release): sparks out of the rear wheels that grow with the charge (more of
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
//! - **The ground** (GD § 10): dust off the road (issue #130: the color of the circuit's biome's
//!   ground, its shoulders or its sand), dust blown out by a landing, the anti-gravity's cyan
//!   sparks, the glider's wind off its wing tips.
//! - **The water** (GD § 10, issue #130): a splash going in and out of a lake (a crown of drops, a
//!   ring and puffs of foam on the water), the wake of foam of a kart at its surface.
//! - **The wheels and the standing start** (GD § 4 and 14, issue #176, `wheels.rs`): every wheel
//!   rolls at its kart's speed (its band shows the turn: `kart::parts::paint_band`), the front
//!   ones steer; the back tyres spin on the spot on the grid with the accelerator down (smoke,
//!   dust, little shakes, the engine revs: `audio`), setting off and with a rocket start; at GO a
//!   cloud by the start's quality; a smaller one when a kart sets off again from a full stop.
//!
//! **Fluidity first**: a fixed pool of [`ParticlePool::SLOTS`] slots (`pool.rs`), each with its
//! entity made once at startup, sharing 4 meshes and 24 materials (issue #174: what glows and
//! the smoke are quads of two shared meshes instead, `batch.rs`: two draws for all of them); a global cap that the pool
//! never exceeds ([`VfxTuning::max_particles`], the oldest particle gives its slot up); nothing
//! thrown by a kart farther than `emit_range` from the camera; no shadow cast; nothing allocated
//! while racing (`alloc_tests.rs` counts it). No compute shader: the particles are plain meshes
//! (WebGL2).
//!
//! **Purely visual**: the plugin reads the game, never writes to it (but the camera: its field of
//! view, its shake); it is added by `GamePlugin` only with a renderer (the tests add it by hand):
//! the bot races' world has none of it, and stays the one they were measured with.

mod atlas;
mod batch;
mod cues;
mod emit;
mod pool;
mod systems;
mod tuning;
mod wheels;

#[cfg(test)]
mod alloc_tests;
#[cfg(test)]
mod scenario_tests;

pub use atlas::Cell;
pub use batch::{GroundHalo, MAX_HALOS, QuadColors, Quads};
pub use cues::{
    BoostSource, Bursts, SparkStyle, TickEvents, VfxSnapshot, boost_source, ember_paint, fov_punch,
    kart_bursts, spark_style, tier_progress,
};
pub use emit::{KartFrame, take};
pub use pool::{Batch, Curve, Facing, Paint, Particle, ParticlePool, Shape, Sparkle};
pub use systems::{
    DizzyStar, FovKick, KartVfx, MAX_DIZZY_STARS, Particles, VfxAssets, VfxBatch, VfxParticle,
    VfxQueue, WorldBurst, star_mesh,
};
pub use tuning::{
    BoostVfx, DriftVfx, ImpactVfx, VfxColors, VfxTuning, WheelVfx, WorldVfx, ground_dust, road_dust,
};
pub use wheels::{
    Standstill, Start, WheelDrive, go_puffs, roll_rate, spin_share, steer_angle, steer_from_yaw,
    target_slip, wheel_rotation,
};

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
        // Issue #174: the atlas of the glows is an image (the headless tests have no renderer's
        // storage of images: made here).
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        app.init_resource::<VfxTuning>()
            .init_resource::<VfxQueue>()
            .init_resource::<FovKick>()
            .add_systems(Startup, systems::create_vfx)
            // Issue #176: what the drivers press, before the race holds the karts on the grid.
            .add_systems(
                FixedUpdate,
                systems::watch_the_drivers
                    .after(crate::kart::KartSystems::Input)
                    .before(crate::race::RaceSystems::Gate),
            )
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
                    systems::follow_the_ground,
                    systems::emit_karts,
                    systems::emit_world,
                    systems::kick_the_camera,
                    systems::show_particles,
                    systems::show_dizzy_stars,
                    systems::tint_flames,
                    // Issue #176: the wheels, on top of where the kart's visual puts them.
                    systems::spin_wheels.after(crate::kart::place_wheels),
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
