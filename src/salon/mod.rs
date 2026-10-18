//! Issue #101: **the lobby in 3D** (`docs/GAME_DESIGN.md` § 13 and 15, `docs/ARCHITECTURE.md` §
//! « Le salon en 3D », `docs/UI_CHARTE.md` § « Le salon (#101) »). Max: « Je veux que le lobby
//! soit une zone où les personnages connectés soient debout et puissent se taper dessus et
//! ragdoll. »
//!
//! - **The scene** (`scene.rs`): a clearing of the meadow, its own level (`TrackChoice::Salon`,
//!   asked by `net` once seated in a room, instead of free driving on the room's circuit), with
//!   giant donuts to bounce on, cakes, rocks, benches, a ring of trees. A race's level replaces it:
//!   **nothing of it is left in a race** (its entities are the level's, its systems wait for it).
//! - **The characters on foot** (`walker.rs`, `walk.rs`): each player of the room has its
//!   animal, standing on its legs (the Cube Pets' `walk` and `run` clips, the wobble of #12), a
//!   party hat of its color, its pseudo over it. Ours walks with the race's keys (and the stick),
//!   jumps (Espace, RT), and **its paw** (the Interaction button: X or F, RB) knocks over the one
//!   in front of it: **the ragdoll** (`characters::ragdoll`): it tumbles, bounces, rolls, gets up
//!   on its own. No damage, no score: it is for fun while the players gather. The camera follows
//!   ours, from a fixed angle.
//! - **Online** (`online.rs`, `net::foot`): each player has authority over its character and
//!   tells its state 30 times a second (`net::FootState`, 33 bytes; the host bundles them); the
//!   others show it interpolated, like the karts. A strike goes to everyone, **its victim
//!   decides** (in reach of where it was at the tick the striker saw, not down, not protected),
//!   and everyone sees it fall by its states.
//! - **The lobby's screen** (`view.rs`, `screen.rs`): over the scene, after the charter: the code
//!   and the invitation link (« Copier »), the players (pseudo, animal, color, ping, host, ready),
//!   « Prêt ! » or « Lancer la course » (disabled with its reason), the host's settings (issue
//!   #122: the mode, « Circuit seul » or « Championnat » and its lines, a championship's program
//!   on the right; the mirror, the bots, the items), « Pilote », « Commandes » (issue #136),
//!   « Quitter »; the banner of #64 when it is closed (issue #136: its button), and the
//!   connection problems of #49. It replaces the egui window.
//! - **The championship online** (issue #122, `online.rs`, `net::championship`): between its
//!   races, every peer shows the host's standings, then its podium (the screens of
//!   `crate::modes`); the host moves on after a moment (`SalonTuning::standings_seconds`,
//!   `podium_seconds`) or with its buttons (issue #136, `menu::end`: « Course suivante »,
//!   « Le podium », « Retour au salon »); after the podium, everyone is back in the lobby.
//!
//! **Only with a renderer** (added by `menu::MainMenuPlugin`), like the other screens: the headless
//! world of the bot races has none of it (no resource, no system, no component). The tests add
//! [`SalonPlugin`] themselves; everything runs headless there (the animals stand in primitives).
//! Without it (the headless tests of the network from before), a room drives freely on its
//! circuit, as before.

mod online;
mod scene;
mod screen;
mod view;
pub mod walk;
mod walker;

#[cfg(test)]
mod tests;

pub use online::{RemoteWalkerBody, SALON_SESSION};
pub use scene::{SALON_MODELS, SalonDecor, SalonRoot, seat_spot};
pub use screen::{
    SalonAction, SalonBanner, SalonBannerPill, SalonButtonNode, SalonLineNode, SalonMenu,
    SalonPlayerNode, SalonScreenRoot,
};
pub use view::{
    ButtonKind, ButtonView, PlayerLine, SalonRow, SalonView, Seat, ViewInput, salon_view,
};
pub use walk::{Bouncer, FootInput};
pub use walker::{
    PlayerWalker, Walker, WalkerControl, WalkerFigure, WalkerHat, WalkerLook, WalkerTag,
    WalkerVisual, walker_body,
};

use bevy::prelude::*;

use crate::admin::Tunings;
use crate::characters::{RagdollPlugin, RagdollSystems, RagdollTuning};

/// Opens the lobby's screen from the lobby on a gamepad (L on the keyboard, `net::LOBBY_KEY`):
/// the « Select » button (« Back », « View », « − »: by the left of the pad's face).
pub const SALON_BUTTON: GamepadButton = GamepadButton::Select;

/// The lobby in 3D (admin panel): walking, jumping, the paw, the camera, the way back after a
/// race. Purely the lobby's: nothing of the race.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct SalonTuning {
    /// A character's top speed on foot (m/s)...
    pub walk_speed: f32,
    /// ...reached (and lost) with this acceleration on the ground (m/s²)...
    pub acceleration: f32,
    /// ...this share of it in the air.
    pub air_control: f32,
    /// It turns toward where it goes this fast (rad/s).
    pub turn_rate: f32,
    /// A jump leaves the ground this fast (m/s)...
    pub jump_speed: f32,
    /// ...and falls this many times the Earth's gravity: snappy cartoon jumps.
    pub gravity_scale: f32,
    /// A giant donut sends a character landing on it up this fast (m/s).
    pub bounce_speed: f32,
    /// The paw reaches a character this far (m, center to center)...
    pub paw_reach: f32,
    /// ...within this angle of its front (degrees, each side)...
    pub paw_cone: f32,
    /// ...at most this much higher or lower (m)...
    pub paw_height: f32,
    /// ...once every this long (s).
    pub paw_cooldown: f32,
    /// A swing shows this long (s).
    pub paw_swing: f32,
    /// The victim's check (as for the karts, GD § 15): in reach plus this much of where it was
    /// at the tick the striker saw (m)...
    pub reach_tolerance: f32,
    /// ...no older than this (ms).
    pub max_strike_age_ms: f32,
    /// The camera: behind our character (m)...
    pub camera_distance: f32,
    /// ...this high (m)...
    pub camera_height: f32,
    /// ...following it this fast (1/s)...
    pub camera_follow: f32,
    /// ...with this vertical field of view (degrees).
    pub camera_fov: f32,
    /// After a race, once the host closed its results, the room goes back to the lobby this long
    /// after (s; the host says so to everyone).
    pub back_after_results: f32,
    /// Issue #122, a championship online: its overall standings show this long after each race
    /// before the host starts the next one, or the podium after the last (s; the host's Entrée
    /// goes on at once)...
    pub standings_seconds: f32,
    /// ...and the podium this long before everyone goes back to the lobby (s).
    pub podium_seconds: f32,
}

impl Default for SalonTuning {
    fn default() -> Self {
        Self {
            walk_speed: 4.5,
            acceleration: 30.0,
            air_control: 0.4,
            turn_rate: 12.0,
            jump_speed: 6.5,
            gravity_scale: 2.0,
            bounce_speed: 12.0,
            paw_reach: 1.8,
            paw_cone: 75.0,
            paw_height: 1.2,
            paw_cooldown: 0.6,
            paw_swing: 0.25,
            reach_tolerance: 0.75,
            max_strike_age_ms: 300.0,
            camera_distance: 9.0,
            camera_height: 6.5,
            camera_follow: 5.0,
            camera_fov: 50.0,
            back_after_results: 6.0,
            standings_seconds: 8.0,
            podium_seconds: 12.0,
        }
    }
}

/// The lobby's systems.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SalonSystems {
    /// `FixedUpdate`: our character's keys.
    Input,
    /// `FixedUpdate`: the characters walk, swing their paws, take the strikes (before the
    /// ragdolls and the physics).
    Walk,
}

/// The lobby in 3D and its screen (see the module's documentation).
pub struct SalonPlugin;

impl Plugin for SalonPlugin {
    fn build(&self, app: &mut App) {
        use bevy::app::AnimationSystems;
        use bevy::transform::TransformSystems;
        use bevy::ui::UiSystems;
        use bevy_rapier3d::prelude::PhysicsSet;

        use crate::interpolation::InterpolationSystems;
        use crate::net::NetSystems;

        app.init_resource::<SalonTuning>()
            .add_plugins(RagdollPlugin)
            .add_message::<SalonAction>()
            .configure_sets(
                FixedUpdate,
                (SalonSystems::Input, SalonSystems::Walk)
                    .chain()
                    .before(RagdollSystems)
                    .before(PhysicsSet::SyncBackend)
                    .run_if(scene::salon_up),
            )
            .add_systems(
                Update,
                (
                    scene::build_the_salon.run_if(scene::salon_wanted),
                    (
                        scene::dress_the_salon.run_if(scene::can_dress),
                        online::follow_the_room.after(NetSystems),
                        walker::spawn_our_walker,
                        walker::dress_walkers,
                        walker::animate_walkers,
                        online::report_walkers,
                    )
                        .chain()
                        .run_if(scene::salon_up),
                    online::back_to_the_salon
                        .after(NetSystems)
                        .run_if(online::results_shown),
                    // Issue #122: the host moves the championship online on.
                    online::run_the_championship
                        .after(NetSystems)
                        .run_if(resource_exists::<crate::net::NetSession>)
                        .run_if(online::hosting),
                    online::forget_the_room
                        .run_if(resource_exists::<crate::net::FootSync>)
                        .run_if(not(scene::salon_up)),
                ),
            )
            .add_systems(
                Update,
                (
                    screen::salon_keys,
                    screen::apply_salon_actions,
                    screen::sync_salon_tree,
                    screen::refresh_salon_tree.run_if(screen::screen_shown),
                )
                    .chain()
                    .after(NetSystems)
                    .run_if(screen::screen_wanted_or_shown),
            )
            .add_systems(
                FixedUpdate,
                (
                    walker::read_foot_input.in_set(SalonSystems::Input),
                    (
                        online::take_strikes,
                        walker::walk_walkers,
                        walker::swing_paws,
                        online::move_remote_walkers,
                    )
                        .chain()
                        .in_set(SalonSystems::Walk),
                ),
            )
            .add_systems(FixedLast, online::send_our_walker.run_if(scene::salon_up))
            // Issue #122: the championship as the host told it, before the screens of `Update`
            // (the results, the standings, the podium) look at it: they all change on the same
            // frame, never one over the other.
            .add_systems(
                PreUpdate,
                online::show_the_championship.run_if(resource_exists::<crate::net::NetSession>),
            )
            .add_systems(
                PostUpdate,
                (
                    online::show_remote_walkers.in_set(InterpolationSystems),
                    (walker::step_walker_wobbles, walker::pose_walker_paws)
                        .chain()
                        .after(InterpolationSystems)
                        .after(AnimationSystems)
                        .before(crate::characters::lay_wobbles)
                        .before(TransformSystems::Propagate),
                    walker::film_the_salon
                        .after(crate::camera::chase)
                        .before(crate::camera::tint_under_water),
                    walker::follow_walker_tags
                        .after(TransformSystems::Propagate)
                        .before(UiSystems::Layout),
                )
                    .run_if(scene::salon_up),
            );
        // Its tunings in the admin panel, next to the others (when the panel is there: the
        // game's).
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<SalonTuning>();
            tunings.register::<RagdollTuning>();
        }
    }
}
