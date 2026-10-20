//! Items (GD § 8, issue #16): item boxes, the roulette, two slots, and the items of phase 1
//! (pelure, pelote de laine, abeille, piment, croquette dorée), with the MK8DX rules and our own
//! items.
//!
//! - **Boxes**: the circuit file places them (`item_boxes`, spawned by the track as
//!   `ItemBoxSpot`s, which become [`ItemBox`]es). A kart running into one breaks it and draws an
//!   item (two for a double box) from the table of its distance to the leader (`table.rs`,
//!   `assets/data/items.ron`); the box comes back about 2 s later. The draw is deterministic
//!   (race seed, grid slot, draws so far).
//! - **Slots** (`slots.rs`): hand and reserve, the roulette (about 1.5 s), the item button
//!   (`KartInput::item`: used on the press; a pelure or pelote is dragged behind while held and
//!   goes on the release; an "x3" comes out at the first press). Thrown backwards with the
//!   "back" direction held: the brake (Bas/S, B) or the look-back button (C).
//! - **Items out** ([`ItemObject`], `objects.rs`): a pelure lies on the ground; a pelote rolls
//!   straight and bounces off the walls; an abeille follows the track's centerline, then dives on
//!   the kart just ahead in the race. Held out (dragged, or an "x3" around the kart), they are
//!   shields: two items touching break each other. None is a Rapier body: rays against the track
//!   and swept contacts, a few dozen per tick even with 12 karts throwing.
//! - **Hits**: an item touching a kart breaks and tells the kart ([`ItemHit`]); the victim
//!   decides (docs/ARCHITECTURE.md: on the network, the victim's own peer): nothing during a spin
//!   or tumble, the pelican, after the finish or while invincible; otherwise a spin out (pelure,
//!   abeille: about 1 s) or a tumble (pelote: about 1.6 s) (`KartState::Hit`, its drift and boost
//!   lost, its items held out too), 3 croquettes lost (`LoseCroquettes`), then 1 to 2 s of
//!   invincibility with its stat (the kart starts it when the hit ends).
//! - **Piment**: a boost of about 1.2 s through the kart's `Boost`, like every boost (GD § 2).
//!   **Croquette dorée**: 2 croquettes (`Croquettes::gain`, up to 10) and a croquette's mini-boost.
//! - **Phase 2** (issue #18, GD § 8):
//!   - **piment doré**: about 7 s of boosts at will, one per press (it stays in the hand);
//!   - **bombe à confettis**: lobbed ahead (or dropped behind), it explodes on what it touches or
//!     after its fuse ([`Blast`]): every kart in its zone tumbles;
//!   - **chantilly**: cream on the screen of the racers ahead ([`ItemEffects::creamed`]; the bots
//!     drive less precisely);
//!   - **super câlin**: about 7 s invincible and faster; the karts it runs into tumble;
//!   - **fusée à confettis**: about 6 s of a very fast autopilot along the bots' racing line
//!     (`kart::Autopilot`), invincible; the karts it runs into tumble;
//!   - **dégonflage**: every opponent shrinks, slows down and drops its items; the first ones
//!     stay small longer;
//!   - **oreiller géant**: flies to the leader, hovers over it, then crushes it (tumble, a small
//!     zone); one at a time, none before 20 s of race, 30 s between two ([`pillow_allowed`]);
//!   - **klaxon-canard**: destroys the items close by ([`ItemEffect`]), the oreiller included.
//!
//!   Immunities: the items that touch or explode ([`immune_to_items`]) and the dégonflage and
//!   the chantilly ([`immune_to_effects`]: they reach a kart spinning too) spare the invincible
//!   karts (after a hit, in a super câlin, in a fusée), the pelican's and those past the finish.
//! - **Phase 3** (issue #21, GD § 8):
//!   - **pie voleuse**: about 3 s invisible (`ItemEffects::ghost`, and invincible meanwhile: the
//!     items go through it, nothing aims at it, no paw reaches it), and it steals the item of a
//!     racer that holds one, drawn with the leader the most likely (issue #88, [`steal_victim`],
//!     [`ItemSteal`]: the victim gives it up);
//!   - **piñata**: its eight items ([`PINATA_ITEMS`]) turn around the kart (shields, like an
//!     "x3"); each press uses the one in front ([`pinata_in_front`]) as itself;
//!   - **boomerang-os**: thrown ahead (or behind), it curves out (issue #88: to the side its
//!     thrower steers, [`boomerang_curve`]), then comes back to its thrower
//!     ([`Motion::Boomerang`]), who catches it; three throws, the next one once it is back (or
//!     broken). It breaks on the first kart it touches (a spin out), never on its thrower.
//! - **Debug** ([`GiveItem`]): the I key cycles through every item into the player's hand, the
//!   admin panel gives any (online: only in a debug build, and only what plays online).
//!
//! - **Online** (issue #53, `crate::net`): these rules run for our karts; a box broken is a claim
//!   to the host ([`BoxBroken`]), an item thrown goes to every peer, which simulates it
//!   ([`CatchUp`] for the ticks of its trip); each peer judges only the contacts that are its own
//!   (`collide_items`: its karts, its shields) and tells the others ([`ItemHitJudged`],
//!   [`ItemBlocked`]). The other peers' karts (`kart::Remote`) are targets for the abeilles, and
//!   what they hold out follows them. Issue #61: an oreiller géant drawn is also a claim to the
//!   host ([`PillowDraw`]: its rules are the host's); the chantilly and the dégonflage carry the
//!   ranking of their user's peer ([`SharedStandings`]); a kart of another peer in a super câlin
//!   or a fusée knocks ours over (`ram_karts`); an oreiller géant of another peer follows the
//!   target its thrower's peer gives it.
//!
//! Everything runs in `FixedUpdate` at 60 Hz ([`ItemSystems`]): after the karts' input and the
//! race gate, before the croquettes and the driving. `ItemTuning::enabled` switches the items
//! off (no box, nothing held, nothing on the track): the bots' driving tests race without items.
//! Visuals in `visual.rs` ([`ItemVisualPlugin`]; issue #106: the items' models in `models.rs`,
//! their icons in `icons.rs`, their files and the showcase with a renderer, [`ItemModelPlugin`]);
//! the HUD's two slots in `crate::ui`; the bots' use in `crate::bots`.

// Issue #106: the items' icons (with a renderer) and models.
pub mod icons;
pub mod models;
mod objects;
mod slots;
mod table;
mod tuning;
mod visual;

#[cfg(test)]
mod scenario_tests;

pub use icons::{ItemIconPlugin, ItemIcons};
pub use models::{Look, MODEL_FILES};
pub use objects::{
    FALL_LIMIT_SPEED, GROUND_PROBE_DOWN, GROUND_PROBE_UP, Hold, ITEM_GRAVITY, KART_RIDE_HEIGHT,
    Roll, RollStep, WALL_NORMAL_Y, held_offset, pinata_angle, pinata_in_front, roll, segment_box,
    segment_sphere,
};
pub use slots::{ItemSlots, PINATA_FULL, Slot, Use};
pub use table::{
    BOOMERANG_THROWS, DrawRow, ITEMS_RON, InvalidTable, ItemKind, ItemTable, PINATA_ITEMS, Prize,
    TableFile, TableRow, draw_unit, pillow_allowed, steal_unit,
};
pub use tuning::ItemTuning;
pub use visual::{
    EffectKind, EffectLook, ItemBoxLook, ItemModelPlugin, ItemShowcase, ItemVisualAssets,
    ItemVisualPlugin,
};

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext, Velocity};

use crate::TICK_HZ;
use crate::bots::RacingLine;
use crate::croquettes::{
    CroquettePickedUp, CroquetteSystems, CroquetteTuning, Croquettes, LoseCroquettes, LossCause,
    touches,
};
use crate::kart::{
    Autopilot, Boost, DeflatedScale, HitReaction, Invincibility, ItemEffects, KART_HALF_EXTENTS,
    Kart, KartInput, KartState, KartSystems, MaxSpeedFactors, PlayerKart, Remote,
};
use crate::level::{LevelScoped, Online};
use crate::race::{Course, Race, RaceGrid, RaceProgress, RaceSystems, Racer, seconds_to_ticks};
use crate::track::{GRID_SLOTS, ItemBoxSpot};

/// An item box of the track (GD § 8), on each `ItemBoxSpot`: there to take when `respawn_in` is 0.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemBox {
    /// Seconds before it comes back, 0 when it is there.
    pub respawn_in: f32,
}

impl ItemBox {
    pub fn available(&self) -> bool {
        self.respawn_in <= 0.0
    }
}

/// A kart's item counters (race logs, tests, telemetry).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemStats {
    /// Boxes broken.
    pub boxes: u32,
    /// Items used: thrown, dropped, eaten (a piment), cashed (a croquette dorée).
    pub used: u32,
    /// Times it was hit (spin out or tumble).
    pub hits_taken: u32,
    /// Karts its items hit.
    pub hits_landed: u32,
    /// Items stopped by what it held out (shields).
    pub blocked: u32,
}

/// How an item out moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Held out by its kart (it follows the kart).
    Held(Hold),
    /// A pelure lying on the ground.
    Ground,
    /// Rolling straight on the ground: a pelote (or an abeille thrown backwards). `fall`: its
    /// falling speed off an edge.
    Rolling { bounces_left: u32, fall: f32 },
    /// An abeille after `target`, the kart that was just ahead of its thrower.
    Homing {
        target: Option<Entity>,
        flight: Flight,
    },
    /// A bombe à confettis (issue #18): lobbed or dropped (its `vertical` speed, m/s up), then
    /// on the ground (`grounded`), sliding to a stop, until it explodes. `impact`: thrown ahead,
    /// it explodes where it lands; dropped behind, at the end of its fuse (or on what touches
    /// it).
    Bomb {
        vertical: f32,
        grounded: bool,
        impact: bool,
    },
    /// An oreiller géant (issue #18) after `target`, the leader (looked up again every tick).
    Pillow {
        target: Option<Entity>,
        flight: PillowFlight,
    },
    /// A boomerang-os (issue #21) on the ground: flying out (`travelled` m so far), curving at
    /// `curve` (rad/s, positive to its left: issue #88, the steering of its throw,
    /// [`boomerang_curve`]), then `returning` to its thrower; it bounces off `bounces_left` walls
    /// more. `fall`: its falling speed off an edge.
    Boomerang {
        returning: bool,
        travelled: f32,
        bounces_left: u32,
        fall: f32,
        curve: f32,
    },
}

/// An oreiller géant's flight (GD § 8, issue #18).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PillowFlight {
    /// Along the track's centerline, at race distance `distance`, `lateral` meters right of it,
    /// high above the karts.
    Track { distance: f32, lateral: f32 },
    /// Over its target, for `left` seconds more (a klaxon-canard can still destroy it).
    Hover { left: f32 },
    /// Dropping on it.
    Drop,
}

/// An abeille's flight (GD § 8: homing in on the kart just ahead).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flight {
    /// Along the track's centerline, at race distance `distance`, `lateral` meters right of it,
    /// above the other karts.
    Track { distance: f32, lateral: f32 },
    /// Straight at its target (straight on once it lost it).
    Dive,
}

/// An item out on the track, or held out around its kart: a pelure, a pelote, an abeille. Its
/// simulated position is `position`; its `Transform` is only the visual, interpolated between
/// the last two ticks (`visual.rs`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(Transform, Visibility, LevelScoped)]
pub struct ItemObject {
    pub item: ItemKind,
    /// The kart that threw it (or holds it).
    pub owner: Entity,
    pub motion: Motion,
    pub position: Vec3,
    /// Its position on the previous tick (the swept contacts, the visual).
    pub previous: Vec3,
    pub velocity: Vec3,
    /// Seconds since it came out.
    pub age: f32,
}

impl ItemObject {
    pub fn new(
        item: ItemKind,
        owner: Entity,
        motion: Motion,
        position: Vec3,
        velocity: Vec3,
    ) -> Self {
        Self {
            item,
            owner,
            motion,
            position,
            previous: position,
            velocity,
            age: 0.0,
        }
    }

    /// Its radius (what touches it).
    pub fn radius(&self, tuning: &ItemTuning) -> f32 {
        item_radius(self.item, tuning)
    }

    pub fn held(&self) -> Option<Hold> {
        match self.motion {
            Motion::Held(hold) => Some(hold),
            _ => None,
        }
    }
}

/// The radius of an item out.
pub fn item_radius(item: ItemKind, tuning: &ItemTuning) -> f32 {
    match item {
        ItemKind::Pelure => tuning.pelure_radius,
        ItemKind::Pelote => tuning.pelote_radius,
        ItemKind::Abeille => tuning.abeille_radius,
        ItemKind::Bombe => tuning.bombe_radius,
        ItemKind::OreillerGeant => tuning.oreiller_radius,
        ItemKind::Boomerang => tuning.boomerang_radius,
        _ => 0.3,
    }
}

/// How a kart reacts to `item` (GD § 8): a tumble for the heavy pelote, the explosions (bombe,
/// oreiller géant) and the karts that ram (super câlin, fusée); a spin out for the light pelure
/// and abeille.
pub fn reaction_to(item: ItemKind) -> HitReaction {
    match item {
        ItemKind::Pelote
        | ItemKind::Bombe
        | ItemKind::OreillerGeant
        | ItemKind::SuperCalin
        | ItemKind::Fusee => HitReaction::Tumble,
        _ => HitReaction::Spin,
    }
}

/// Whether an item hit does nothing to a kart (GD § 8 and § 15): already spinning or tumbling,
/// carried by the pelican, invincible (after its last hit, in a super câlin or a fusée: they
/// start its `Invincibility`), or past the finish line.
pub fn immune_to_items(state: &KartState, invincibility: &Invincibility, finished: bool) -> bool {
    matches!(state, KartState::Hit { .. } | KartState::Respawning)
        || invincibility.is_active()
        || finished
}

/// Whether the dégonflage and the chantilly spare a kart (issue #18): invincible (after a hit,
/// in a super câlin, in a fusée), carried by the pelican, or past the finish line. Unlike an
/// item that touches, they reach a kart spinning or tumbling too.
pub fn immune_to_effects(state: &KartState, invincibility: &Invincibility, finished: bool) -> bool {
    *state == KartState::Respawning || invincibility.is_active() || finished
}

/// How long a dégonflage lasts for the kart in `rank` (1 = the leader) of `racers` (GD § 8: the
/// first ones stay small longer): from `first` for the leader to `last` for the last one.
pub fn deflate_time(rank: u32, racers: usize, first: f32, last: f32) -> f32 {
    if racers <= 1 || rank == 0 {
        return first;
    }
    let t = (rank.saturating_sub(1) as f32 / (racers - 1) as f32).clamp(0.0, 1.0);
    first + (last - first) * t
}

/// Who a pie voleuse used by `thief` steals from (GD § 8: « vole l'objet d'un adversaire »;
/// issue #88, Max: « un kart au hasard, avec une probabilité d'autant plus forte que ce kart est
/// devant »): in a race (`standings`, first to last), one of the racers that hold an item to
/// take (`holds`), ahead of it or behind, drawn with `unit` (in `[0, 1)`: [`steal_unit`]) with
/// the weight of its place ([`steal_weight`], `power`: `ItemTuning::pie_rank_power`): the leader
/// the most likely. Off a race (or a kart not in it), the closest kart (`positions`) that holds
/// one (issue #21). `None`: nobody holds anything.
pub fn steal_victim(
    thief: Entity,
    at: Vec3,
    standings: &[Entity],
    positions: &[(Entity, Vec3)],
    holds: impl Fn(Entity) -> bool,
    unit: f32,
    power: f32,
) -> Option<Entity> {
    if standings.contains(&thief) {
        let racers = standings.len();
        let candidates = || {
            standings
                .iter()
                .enumerate()
                .filter(|(_, e)| **e != thief && holds(**e))
                .map(|(i, e)| (*e, steal_weight(i + 1, racers, power)))
        };
        let total: f32 = candidates().map(|(_, weight)| weight).sum();
        let mut left = unit.clamp(0.0, 1.0) * total;
        let mut last = None;
        for (kart, weight) in candidates() {
            if left < weight {
                return Some(kart);
            }
            left -= weight;
            last = Some(kart);
        }
        // Rounding at the very end of the draw: the last one.
        return last;
    }
    positions
        .iter()
        .filter(|(e, _)| *e != thief && holds(*e))
        .min_by(|(_, a), (_, b)| a.distance_squared(at).total_cmp(&b.distance_squared(at)))
        .map(|(e, _)| *e)
}

/// The weight of the racer in `rank` (1 = the leader) of `racers` in a pie voleuse's draw (issue
/// #88, [`steal_victim`]): `(racers - rank + 1)^power`, from `racers^power` for the leader down to
/// 1 for the last one (`power` 0: every racer alike).
pub fn steal_weight(rank: usize, racers: usize, power: f32) -> f32 {
    ((racers + 1).saturating_sub(rank).max(1) as f32).powf(power)
}

/// The curve a boomerang-os is thrown with (issue #88, Max: « il tourne avec le kart »): its
/// turn on its way out (rad/s, positive to its left, as `Quat::from_rotation_y`), from its
/// thrower's `steer` at the throw (`KartInput::steer`, right positive): to the side it steers,
/// as much as it steers (`ItemTuning::boomerang_steer_curve` at full lock); thrown straight (the
/// steering under `boomerang_straight_steer`), `boomerang_curve` (to its left by default). The
/// same turn thrown backwards: steering left, it turns to its own left (to the left of the
/// look-back camera's picture).
pub fn boomerang_curve(steer: f32, tuning: &ItemTuning) -> f32 {
    if !steer.is_finite() || steer.abs() < tuning.boomerang_straight_steer {
        tuning.boomerang_curve
    } else {
        -steer.clamp(-1.0, 1.0) * tuning.boomerang_steer_curve
    }
}

/// A pie voleuse (issue #21): `thief` steals from `victim` ([`steal_victim`]). The victim gives up
/// what it holds ([`ItemSlots::steal`]: its peer, online), and the thief gets it, ready.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemSteal {
    pub thief: Entity,
    pub victim: Entity,
}

/// What a pie voleuse took (issue #21): `item` (`count` of it) went from `victim` to `thief`.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemStolen {
    pub thief: Entity,
    pub victim: Entity,
    pub item: ItemKind,
    pub count: u8,
}

/// `kart` was touched by an item of `by` (the item broke on it). The victim decides whether it
/// is hit, and applies the hit to its own kart (docs/ARCHITECTURE.md: on the network the item
/// is the thrower's, the hit the victim's).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ItemHit {
    pub kart: Entity,
    pub by: Entity,
    pub item: ItemKind,
    /// The item that touched it ([`ItemObject`], gone now), if one did: online (issue #53), the
    /// victim tells everyone which item broke on its kart.
    pub object: Option<Entity>,
}

/// The victim decided ([`ItemHit`]): `landed` when its kart was hit, not when it was immune
/// (the item broke all the same). Online (issue #53), its peer tells everyone.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ItemHitJudged {
    pub kart: Entity,
    pub by: Entity,
    pub item: ItemKind,
    pub object: Option<Entity>,
    pub landed: bool,
}

/// An item out broke on another item (a shield, another projectile): `object`, gone now.
/// Online (issue #53), the peer that judged it tells everyone.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ItemBlocked {
    pub object: Entity,
    pub owner: Entity,
}

/// `kart` broke `item_box`, and drew `draws` items from it (0: both slots were taken), the
/// first one with draw number `draw` (`ItemSlots::draws` before it), at `gap` meters from the
/// leader; `filled` says which slots they went to (hand, reserve). Online (issue #53), its peer
/// asks the host for the box; refused, the roulettes stop (`ItemSlots::revoke`). Online too
/// (issue #61), an oreiller géant it drew (`pillow`) is a claim to the host.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct BoxBroken {
    pub item_box: Entity,
    pub kart: Entity,
    pub draw: u32,
    pub draws: u8,
    pub gap: f32,
    pub filled: [bool; 2],
    pub pillow: Option<PillowDraw>,
}

/// An oreiller géant drawn from a box online (issue #61): its draw number, and what the kart gets
/// instead if the host's rules forbid one now (the same draw without it, as solo:
/// [`DrawRow::pick_without`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillowDraw {
    pub draw: u32,
    pub instead: Prize,
}

/// An explosion (issue #18): a bombe à confettis (on what it touched, or at the end of its fuse)
/// or an oreiller géant (on its target). Every kart within `radius` of `at` is hit: the victim
/// decides (`ItemHit`, each peer for its karts online). `announce`: online, this peer judged
/// what set it off (a kart or a shield of ours touched it) and tells the others where.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub at: Vec3,
    pub radius: f32,
    pub item: ItemKind,
    pub owner: Entity,
    /// The item that exploded (gone now).
    pub object: Option<Entity>,
    pub announce: bool,
}

/// A kart used an item that acts on the others (issue #18): the chantilly (on the racers ahead
/// of its `rank`), the dégonflage (on every opponent), the klaxon-canard (on the items around
/// `at`). Online (issue #61), the chantilly and the dégonflage of another peer's kart come from
/// its peer, with its ranking.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ItemEffect {
    pub by: Entity,
    pub item: ItemKind,
    pub at: Vec3,
    /// Its heading: off a race, the chantilly reaches the karts ahead of it.
    pub forward: Vec3,
    /// Its place in the race (1 = the leader; 0 off a race).
    pub rank: u32,
    /// Online (issue #61): the ranking its user's peer saw, first to last, the same on every
    /// peer: each one takes its karts' ranks from it. Solo: `None`, the karts' own ranks.
    pub standings: Option<SharedStandings>,
}

/// The race's ranking when an item acted on the others (issue #61, [`ItemEffect`]), first to
/// last: online, every peer takes its karts' ranks from the one its user's peer saw. At most
/// [`GRID_SLOTS`] karts, nothing allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedStandings {
    karts: [Entity; GRID_SLOTS],
    len: u8,
}

impl SharedStandings {
    /// The first [`GRID_SLOTS`] of `karts`, in their order.
    pub fn new(karts: impl IntoIterator<Item = Entity>) -> Self {
        let mut standings = Self {
            karts: [Entity::PLACEHOLDER; GRID_SLOTS],
            len: 0,
        };
        for kart in karts.into_iter().take(GRID_SLOTS) {
            standings.karts[usize::from(standings.len)] = kart;
            standings.len += 1;
        }
        standings
    }

    /// The karts, first to last.
    pub fn karts(&self) -> &[Entity] {
        &self.karts[..usize::from(self.len)]
    }

    /// The place of `kart` (1 = the leader), 0 when it is not in it.
    pub fn rank(&self, kart: Entity) -> u32 {
        self.karts()
            .iter()
            .position(|k| *k == kart)
            .map_or(0, |i| i as u32 + 1)
    }

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// A klaxon-canard sounded around `at` (issue #18): the visual's shock wave; online, the others
/// see it too.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Honked {
    pub kart: Entity,
    pub at: Vec3,
}

/// Debug (issue #18): the player's kart gets `prize` in its hand, ready (what was there is gone).
/// The I key and the admin panel send it; online, only in a debug build (`cargo run`, `trunk
/// serve`), and only what plays online.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GiveItem(pub Prize);

/// Debug key (issue #18): gives the player the next item of the list ([`Prize::ALL`]): I, the
/// same letter at the same place in AZERTY and QWERTY, no Fn on a Mac.
pub const DEBUG_ITEM_KEY: KeyCode = KeyCode::KeyI;

/// The oreiller géant's record in this race (issue #18): when the last one was drawn (race
/// ticks).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PillowLog {
    pub last_draw: Option<u32>,
}

/// An item that another peer threw some ticks ago (issue #53): it catches up on its first tick
/// here, `0` ticks behind then (its first step sweeps the whole way for contacts).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatchUp(pub u32);

/// The game's table online (issue #18): the items that play online only (`ItemTable::online`).
/// Every peer of a room draws from it (the draws must be the same everywhere).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct OnlineItemTable(pub ItemTable);

/// The item systems inside `FixedUpdate`: after the input and the race gate, before the
/// croquettes (an item hit costs its croquettes on the same tick) and the driving (a piment boosts
/// on the same tick, a hit kart spins from it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemSystems;

/// The item rules. Headless: tests run them with `MinimalPlugins`.
pub struct ItemPlugin;

impl Plugin for ItemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemTuning>()
            .insert_resource(ItemTable::embedded())
            .insert_resource(OnlineItemTable(ItemTable::online()))
            .init_resource::<PillowLog>()
            .insert_resource(DeflatedScale(ItemTuning::default().degonflage_scale))
            .add_message::<ItemHit>()
            .add_message::<ItemHitJudged>()
            .add_message::<ItemBlocked>()
            .add_message::<BoxBroken>()
            .add_message::<Blast>()
            .add_message::<ItemEffect>()
            .add_message::<Honked>()
            .add_message::<GiveItem>()
            .add_message::<ItemSteal>()
            .add_message::<ItemStolen>()
            .add_message::<LoseCroquettes>()
            .add_message::<CroquettePickedUp>()
            // Every kart holds items; every box spot of the track is a box.
            .register_required_components::<Kart, ItemSlots>()
            .register_required_components::<Kart, ItemStats>()
            .register_required_components::<ItemBoxSpot, ItemBox>()
            .configure_sets(
                FixedUpdate,
                ItemSystems
                    .after(KartSystems::Input)
                    .after(RaceSystems::Gate)
                    .before(CroquetteSystems)
                    .before(KartSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (
                    (
                        reset_on_restart.run_if(resource_exists::<Race>),
                        tick_items,
                        take_boxes,
                        use_items,
                        apply_steals,
                        apply_item_effects,
                        steer_rockets,
                        move_items,
                        collide_items,
                        explode,
                        apply_item_hits,
                        update_item_speeds,
                    )
                        .chain()
                        .run_if(items_on),
                    clear_items.run_if(not(items_on)),
                )
                    .in_set(ItemSystems),
            )
            // The karts in a super câlin or a fusée knock over those they run into: the
            // contacts of the physics step (the victims decide on the next tick).
            .add_systems(
                FixedUpdate,
                ram_karts.in_set(KartSystems::Bumps).run_if(items_on),
            )
            .add_systems(Update, give_items);
    }
}

/// The items are in the races (`ItemTuning::enabled`), solo and online (issue #53: shared by
/// `crate::net`).
pub fn items_on(tuning: Res<ItemTuning>) -> bool {
    tuning.enabled
}

/// No item effect left on a kart: what a race restart or the items switched off do.
fn clear_effects(
    effects: &mut Mut<ItemEffects>,
    pilot: &mut Mut<Autopilot>,
    factors: &mut Mut<MaxSpeedFactors>,
) {
    if **effects != ItemEffects::default() {
        **effects = ItemEffects::default();
    }
    if **pilot != Autopilot::default() {
        **pilot = Autopilot::default();
    }
    if factors.items != 1.0 {
        factors.items = 1.0;
    }
}

/// Items switched off: nothing held, nothing out, no effect left, every box back (and hidden,
/// `visual.rs`).
fn clear_items(
    mut commands: Commands,
    mut karts: Query<&mut ItemSlots>,
    mut effects: Query<(&mut ItemEffects, &mut Autopilot, &mut MaxSpeedFactors)>,
    mut boxes: Query<&mut ItemBox>,
    objects: Query<Entity, With<ItemObject>>,
) {
    for mut slots in &mut karts {
        if slots.hand != Slot::Empty
            || slots.reserve != Slot::Empty
            || slots.dragging.is_some()
            || slots.draws != 0
        {
            slots.clear();
        }
    }
    for (mut effects, mut pilot, mut factors) in &mut effects {
        clear_effects(&mut effects, &mut pilot, &mut factors);
    }
    for mut item_box in &mut boxes {
        item_box.set_if_neq(ItemBox::default());
    }
    for entity in &objects {
        commands.entity(entity).try_despawn();
    }
}

/// A race (re)start: empty slots, nothing on the track, no effect left, every box back, no
/// oreiller géant drawn yet.
fn reset_on_restart(
    race: Res<Race>,
    mut commands: Commands,
    mut pillows: ResMut<PillowLog>,
    mut karts: Query<&mut ItemSlots>,
    mut effects: Query<(&mut ItemEffects, &mut Autopilot, &mut MaxSpeedFactors)>,
    mut boxes: Query<&mut ItemBox>,
    objects: Query<Entity, With<ItemObject>>,
) {
    if race.flow.tick != 1 {
        return;
    }
    pillows.set_if_neq(PillowLog::default());
    for mut slots in &mut karts {
        if *slots != ItemSlots::default() {
            slots.clear();
        }
    }
    for (mut effects, mut pilot, mut factors) in &mut effects {
        clear_effects(&mut effects, &mut pilot, &mut factors);
    }
    for mut item_box in &mut boxes {
        item_box.set_if_neq(ItemBox::default());
    }
    for entity in &objects {
        commands.entity(entity).try_despawn();
    }
}

/// The boxes' respawn timers, the roulettes and the piments dorés' windows.
fn tick_items(time: Res<Time>, mut boxes: Query<&mut ItemBox>, mut karts: Query<&mut ItemSlots>) {
    let dt = time.delta_secs();
    for mut item_box in &mut boxes {
        if !item_box.available() {
            // Half a tick of tolerance: 2 s are exactly 120 ticks.
            item_box.respawn_in -= dt;
            if item_box.respawn_in <= dt * 0.5 {
                item_box.respawn_in = 0.0;
            }
        }
    }
    for mut slots in &mut karts {
        if slots.ticking() {
            slots.tick();
        }
    }
}

/// A kart's grid slot for its draws (0 off the race), and its distance to the leader (0 off the
/// race: the leader's row) times `ItemTuning::gap_scale` (issue #155).
fn draw_context(
    racer: Option<&Racer>,
    progress: Option<&RaceProgress>,
    gap_scale: f32,
) -> (u64, f32) {
    (
        racer.map_or(0, |r| r.grid_slot as u64),
        // Issue #155: 1 by default, the distance as it is.
        progress.map_or(0.0, |p| p.gap_to_leader * gap_scale),
    )
}

/// Whether `slots` hold an oreiller géant (ready, or in its roulette).
fn holds_pillow(slots: &ItemSlots) -> bool {
    [slots.hand, slots.reserve].iter().any(|slot| {
        slot.item()
            .is_some_and(|(item, _)| item == ItemKind::OreillerGeant)
    })
}

/// Karts running into a box break it and draw their items (one per free slot, two for a double
/// box), from the table of their distance to the leader (online, the online table: issue #18).
/// A kart with both slots taken breaks it for nothing (MK8DX). The oreiller géant's rules (GD
/// § 8, [`pillow_allowed`]): left out of the draws when they forbid one. Online (issue #61), the
/// host applies them: in a race, an oreiller drawn (the first one of a box) goes into the slot at
/// once and is a claim to the host ([`PillowDraw`]); refused, the kart gets the same draw without
/// it. Off a race, never.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn take_boxes(
    tuning: Res<ItemTuning>,
    (table, online_table, online): (Res<ItemTable>, Res<OnlineItemTable>, Option<Res<Online>>),
    grid: Option<Res<RaceGrid>>,
    race: Option<Res<Race>>,
    mut pillows: ResMut<PillowLog>,
    objects: Query<&ItemObject>,
    mut boxes: Query<(Entity, &Transform, &ItemBoxSpot, &mut ItemBox)>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartState,
            &mut ItemSlots,
            &mut ItemStats,
            Option<&Racer>,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut broken: MessageWriter<BoxBroken>,
) {
    let table = if online.is_some() {
        &online_table.0
    } else {
        &*table
    };
    let seed = grid.map_or(RaceGrid::GAME_SEED, |g| g.seed);
    let roulette = seconds_to_ticks(tuning.roulette_time);
    // The oreiller géant's rules, once per tick (only when a box is broken).
    let race_ticks = race
        .as_ref()
        .filter(|r| r.started())
        .map(|r| r.race_ticks());
    let mut pillow: Option<bool> = None;
    let online = online.is_some();
    for (item_box_entity, box_transform, spot, mut item_box) in &mut boxes {
        if !item_box.available() {
            continue;
        }
        let touching = karts
            .iter()
            .position(|(_, transform, state, _, _, _, progress)| {
                *state != KartState::Respawning
                    && !progress.is_some_and(|p| p.finished())
                    && touches(
                        transform,
                        KART_HALF_EXTENTS,
                        tuning.box_reach,
                        box_transform.translation,
                    )
            });
        if touching.is_none() {
            continue;
        }
        let allowed = if online {
            // Issue #61: the host decides (a claim); off a race, never.
            race_ticks.is_some()
        } else {
            *pillow.get_or_insert_with(|| {
                let exists = karts.iter().any(|(.., slots, _, _, _)| holds_pillow(slots))
                    || objects.iter().any(|o| o.item == ItemKind::OreillerGeant);
                pillow_allowed(
                    race_ticks,
                    exists,
                    pillows.last_draw,
                    seconds_to_ticks(tuning.oreiller_min_race_time),
                    seconds_to_ticks(tuning.oreiller_cooldown),
                )
            })
        };
        for (kart, transform, state, mut slots, mut stats, racer, progress) in &mut karts {
            if *state == KartState::Respawning
                || progress.is_some_and(|p| p.finished())
                || !touches(
                    transform,
                    KART_HALF_EXTENTS,
                    tuning.box_reach,
                    box_transform.translation,
                )
            {
                continue;
            }
            item_box.respawn_in = tuning.box_respawn_time.max(f32::MIN_POSITIVE);
            stats.boxes += 1;
            let (slot, gap) = draw_context(racer, progress, tuning.gap_scale);
            let (draw, mut draws, mut filled) = (slots.draws, 0, [false; 2]);
            let mut claimed = None;
            for _ in 0..if spot.double { 2 } else { 1 } {
                if !slots.has_room() {
                    break;
                }
                let number = slots.draws;
                let unit = draw_unit(seed, slot, number);
                slots.draws += 1;
                draws += 1;
                let prize = if online {
                    // One oreiller per box at most: the next draw goes without it.
                    table.draw_with_pillow(gap, unit, allowed && claimed.is_none())
                } else {
                    table.draw_with_pillow(gap, unit, allowed && pillow != Some(false))
                };
                if prize == Prize::OreillerGeant {
                    if online {
                        claimed = Some(PillowDraw {
                            draw: number,
                            instead: table.row(gap).pick_without(unit, Prize::OreillerGeant),
                        });
                    } else {
                        // One at a time: none more until this one is gone, and the cooldown.
                        pillows.last_draw = race_ticks;
                        pillow = Some(false);
                    }
                }
                if let Some((item, count)) = prize.item() {
                    let into = usize::from(!slots.hand.is_empty());
                    if slots.give(item, count, roulette) {
                        filled[into] = true;
                    }
                }
            }
            broken.write(BoxBroken {
                item_box: item_box_entity,
                kart,
                draw,
                draws,
                gap,
                filled,
                pillow: claimed,
            });
            break;
        }
    }
}

/// The "back" direction of the item button (GD § 8: thrown backwards): the brake (Bas/S, B) or
/// the look-back button (C).
pub fn aims_back(input: &KartInput) -> bool {
    input.brake > 0.5 || input.look_back
}

/// Where a kart's held item `hold` of `item` is now (world).
fn held_position(
    owner: &Transform,
    hold: Hold,
    item: ItemKind,
    age: f32,
    tuning: &ItemTuning,
) -> Vec3 {
    owner.transform_point(held_offset(hold, 3, item_radius(item, tuning), age, tuning))
}

/// What an abeille thrown by `owner` goes after: the kart just ahead of it in the race, or, off a
/// race, the closest kart ahead of it.
fn abeille_target(
    owner: Entity,
    transform: &Transform,
    race: Option<&Race>,
    karts: &[(Entity, Vec3)],
) -> Option<Entity> {
    if let Some(race) = race
        && let Some(i) = race.standings.iter().position(|e| *e == owner)
    {
        return i.checked_sub(1).map(|ahead| race.standings[ahead]);
    }
    closest_ahead(owner, transform, karts)
}

/// The closest kart ahead of `owner` (off a race).
fn closest_ahead(owner: Entity, transform: &Transform, karts: &[(Entity, Vec3)]) -> Option<Entity> {
    let forward = *transform.forward();
    karts
        .iter()
        .filter(|(e, at)| *e != owner && (*at - transform.translation).dot(forward) > 1.0)
        .min_by(|(_, a), (_, b)| {
            a.distance_squared(transform.translation)
                .total_cmp(&b.distance_squared(transform.translation))
        })
        .map(|(e, _)| *e)
}

/// What an oreiller géant goes after (issue #18): the leader of the race (whoever it is, its
/// thrower too), or off a race the closest kart ahead.
fn pillow_target(
    owner: Entity,
    transform: &Transform,
    race: Option<&Race>,
    karts: &[(Entity, Vec3)],
) -> Option<Entity> {
    match race.and_then(|race| race.standings.first()) {
        Some(leader) => Some(*leader),
        None => closest_ahead(owner, transform, karts),
    }
}

/// Where an item thrown by a kart starts and how it moves: `aim_back` thrown backwards, `from`
/// where it was held (a pelure of a row is dropped there). `None` for the items that are not
/// objects (piment, croquette dorée...).
struct Launch<'a> {
    kart: Entity,
    transform: &'a Transform,
    /// The kart's velocity (a bombe flies off with it).
    velocity: Vec3,
    /// Its steering at the throw (issue #88: a boomerang-os curves with it).
    steer: f32,
    progress: Option<&'a RaceProgress>,
    course: Option<&'a Course>,
    race: Option<&'a Race>,
    positions: &'a [(Entity, Vec3)],
}

impl Launch<'_> {
    fn object(
        &self,
        tuning: &ItemTuning,
        item: ItemKind,
        aim_back: bool,
        from: Option<Vec3>,
    ) -> Option<ItemObject> {
        let transform = self.transform;
        // Issue #96: out of the kart in the frame of the road under it: the world's up, or over
        // an anti-gravity zone the road's normal (on a wall, under a ceiling: the items come out
        // on the road, never behind it). Everywhere else, exactly the launch before #96.
        let up = self
            .course
            .map_or(Vec3::Y, |course| course.up_at(transform.translation));
        let forward = if up == Vec3::Y {
            transform.forward().with_y(0.0).normalize_or(Vec3::NEG_Z)
        } else {
            let heading = *transform.forward();
            (heading - up * heading.dot(up)).normalize_or(up.any_orthonormal_vector())
        };
        let radius = item_radius(item, tuning);
        let low = up * (KART_RIDE_HEIGHT - radius);
        // Just off the front (or the back) of the kart, on the ground.
        let reach = forward * (KART_HALF_EXTENTS.z + radius + 0.2);
        let (front, back) = (
            transform.translation + reach - low,
            transform.translation - reach - low,
        );
        let rolling = |bounces_left| Motion::Rolling {
            bounces_left,
            fall: 0.0,
        };
        Some(match item {
            ItemKind::Pelure => {
                // Dropped behind (where it was held, if it was).
                let at = from
                    .unwrap_or(transform.translation - forward * tuning.pelure_drop_distance - low);
                ItemObject::new(item, self.kart, Motion::Ground, at, Vec3::ZERO)
            }
            ItemKind::Pelote => {
                let (at, direction) = if aim_back {
                    (back, -forward)
                } else {
                    (front, forward)
                };
                ItemObject::new(
                    item,
                    self.kart,
                    rolling(tuning.pelote_bounces),
                    at,
                    direction * tuning.pelote_speed,
                )
            }
            // Backwards: straight back on the ground, broken by the first wall.
            ItemKind::Abeille if aim_back => ItemObject::new(
                item,
                self.kart,
                rolling(0),
                back,
                -forward * tuning.abeille_speed,
            ),
            ItemKind::Abeille => {
                let target = abeille_target(self.kart, transform, self.race, self.positions);
                let (flight, at) = match (self.course, self.progress) {
                    (Some(course), Some(progress)) => {
                        let distance = progress.distance + KART_HALF_EXTENTS.z + 0.5;
                        let sample = course.center.at(distance);
                        let lateral = (transform.translation - sample.position).dot(sample.right);
                        (
                            Flight::Track { distance, lateral },
                            sample.point(lateral, tuning.abeille_height),
                        )
                    }
                    _ => (Flight::Dive, front + up * 0.5),
                };
                ItemObject::new(
                    item,
                    self.kart,
                    Motion::Homing { target, flight },
                    at,
                    forward * tuning.abeille_dive_speed,
                )
            }
            // Issue #18: lobbed ahead with the kart's speed, or dropped behind (rolling back a
            // little).
            ItemKind::Bombe if aim_back => ItemObject::new(
                item,
                self.kart,
                Motion::Bomb {
                    vertical: 0.0,
                    grounded: false,
                    impact: false,
                },
                back,
                -forward * tuning.bombe_drop_speed,
            ),
            ItemKind::Bombe => {
                let speed = self.velocity.dot(forward).max(0.0);
                ItemObject::new(
                    item,
                    self.kart,
                    Motion::Bomb {
                        vertical: tuning.bombe_throw_up,
                        grounded: false,
                        impact: true,
                    },
                    front + up * (KART_HALF_EXTENTS.y + radius),
                    forward * (speed + tuning.bombe_throw_speed),
                )
            }
            // Issue #18: up above the kart, after the leader.
            ItemKind::OreillerGeant => {
                let target = pillow_target(self.kart, transform, self.race, self.positions);
                let (flight, at) = match (self.course, self.progress) {
                    (Some(course), Some(progress)) => {
                        let distance = progress.distance;
                        let sample = course.center.at(distance);
                        let lateral = (transform.translation - sample.position).dot(sample.right);
                        (
                            PillowFlight::Track { distance, lateral },
                            sample.point(lateral, tuning.oreiller_height),
                        )
                    }
                    _ => (
                        PillowFlight::Hover {
                            left: f32::INFINITY,
                        },
                        transform.translation + up * tuning.oreiller_height,
                    ),
                };
                ItemObject::new(
                    item,
                    self.kart,
                    Motion::Pillow { target, flight },
                    at,
                    forward * tuning.oreiller_speed,
                )
            }
            // Issue #21: off the front (or the back) on the ground, flying out; the first wall
            // sends it back. Issue #88: curving as its thrower steers.
            ItemKind::Boomerang => {
                let (at, direction) = if aim_back {
                    (back, -forward)
                } else {
                    (front, forward)
                };
                ItemObject::new(
                    item,
                    self.kart,
                    Motion::Boomerang {
                        returning: false,
                        travelled: 0.0,
                        bounces_left: 1,
                        fall: 0.0,
                        curve: boomerang_curve(self.steer, tuning),
                    },
                    at,
                    direction * tuning.boomerang_speed,
                )
            }
            _ => return None,
        })
    }
}

fn spawn_object(commands: &mut Commands, object: ItemObject) {
    commands.spawn((
        Name::new(object.item.label()),
        Transform::from_translation(object.position),
        object,
    ));
}

/// Every kart's item button: drag, release, deploy, fire, use (`slots.rs`). Items go out from
/// the kart; a piment boosts it, a croquette dorée fills its pockets; a piment doré opens its
/// window, a super câlin or a fusée starts on the kart, the chantilly, the dégonflage and the
/// klaxon-canard act on the others ([`ItemEffect`], issue #18). Issue #21: a pie voleuse makes it
/// invisible and steals ([`ItemSteal`]); a piñata comes out around it, then its item in front
/// goes at each press, as itself; a boomerang-os waits for the last one to come back. A kart
/// carried by the pelican or past the finish line loses what it held out and uses nothing; a
/// kart spinning or tumbling, or in a fusée, uses nothing.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_items(
    tuning: Res<ItemTuning>,
    croquette_tuning: Option<Res<CroquetteTuning>>,
    online: Option<Res<Online>>,
    (race, grid): (Option<Res<Race>>, Option<Res<RaceGrid>>),
    course: Option<Res<Course>>,
    mut commands: Commands,
    (mut positions, mut holders): (Local<Vec<(Entity, Vec3)>>, Local<Vec<Entity>>),
    objects: Query<(Entity, &ItemObject)>,
    remotes: Query<(Entity, &Transform, Option<&ItemSlots>), (With<Remote>, Without<Kart>)>,
    mut karts: Query<
        (
            Entity,
            (&Transform, &Velocity),
            &KartInput,
            &KartState,
            &mut ItemSlots,
            &mut ItemStats,
            &mut Boost,
            (&mut ItemEffects, &mut Invincibility),
            Option<&mut Croquettes>,
            (Option<&RaceProgress>, Option<&Racer>),
        ),
        With<Kart>,
    >,
    mut picked: MessageWriter<CroquettePickedUp>,
    mut effects_out: MessageWriter<ItemEffect>,
    mut steals: MessageWriter<ItemSteal>,
) {
    positions.clear();
    positions.extend(karts.iter().map(|(e, (t, _), ..)| (e, t.translation)));
    // Online (issue #53), the other peers' karts are targets too.
    positions.extend(remotes.iter().map(|(e, t, _)| (e, t.translation)));
    // Issue #21: the karts a pie voleuse may steal from (online, the other peers' karts as their
    // states say: `ItemSlots::stealable` of what their bodies hold).
    holders.clear();
    holders.extend(
        karts
            .iter()
            .filter(|(_, _, _, _, slots, ..)| slots.stealable().is_some())
            .map(|(e, ..)| e),
    );
    holders.extend(
        remotes
            .iter()
            .filter(|(_, _, slots)| slots.is_some_and(|s| s.stealable().is_some()))
            .map(|(e, ..)| e),
    );
    let croquettes_tuning = croquette_tuning.as_deref().cloned().unwrap_or_default();
    let started = race.as_ref().is_none_or(|r| r.started());
    for (
        kart,
        (transform, velocity),
        input,
        state,
        mut slots,
        mut stats,
        mut boost,
        (mut effects, mut invincibility),
        croquettes,
        (progress, racer),
    ) in &mut karts
    {
        // The items this kart holds out, and the one that leaves now: the dragged one, or the
        // last one of an "x3".
        let held_out = || {
            objects
                .iter()
                .filter(move |(_, o)| o.owner == kart && o.held().is_some())
        };
        let leaving = |dragged: bool| {
            held_out()
                .filter(|(_, o)| !matches!(o.held(), Some(Hold::Pinata(_))))
                .filter(|(_, o)| (o.held() == Some(Hold::Dragged)) == dragged)
                .max_by_key(|(_, o)| match o.held() {
                    Some(Hold::Trail(k) | Hold::Orbit(k)) => k,
                    _ => 0,
                })
                .map(|(e, o)| (e, o.position))
        };
        let out = *state == KartState::Respawning || progress.is_some_and(|p| p.finished());
        if out || state.is_hit() || !started || effects.rocket > 0.0 {
            if out && slots.lose_held_out() {
                for (entity, _) in held_out() {
                    commands.entity(entity).try_despawn();
                }
            }
            if slots.button != input.item {
                slots.button = input.item;
            }
            continue;
        }
        // Issue #21: the next throw of a boomerang-os once the last one is back (or broken).
        let boomerang_out = matches!(
            slots.hand,
            Slot::Ready {
                item: ItemKind::Boomerang,
                ..
            }
        ) && has_out(&objects, kart, ItemKind::Boomerang);
        if boomerang_out {
            if slots.button != input.item {
                slots.button = input.item;
            }
            continue;
        }
        let action = {
            let mut next = *slots;
            let action = next.step_button(input.item, aims_back(input));
            slots.set_if_neq(next);
            action
        };
        let launch = Launch {
            kart,
            transform,
            velocity: velocity.linear,
            steer: input.steer,
            progress,
            course: course.as_deref(),
            race: race.as_deref(),
            positions: &positions,
        };
        let (item, aim_back, from) = match action {
            Use::None => continue,
            Use::Drag(item) => {
                let at = held_position(transform, Hold::Dragged, item, 0.0, &tuning);
                let held = ItemObject::new(item, kart, Motion::Held(Hold::Dragged), at, Vec3::ZERO);
                spawn_object(&mut commands, held);
                continue;
            }
            Use::Deploy { item, count } => {
                for k in 0..count {
                    // Issue #21: a piñata's eight are its items, each as itself.
                    let (held_item, hold) = match item {
                        ItemKind::Pinata => (
                            PINATA_ITEMS[usize::from(k) % PINATA_ITEMS.len()],
                            Hold::Pinata(k),
                        ),
                        ItemKind::Pelure => (item, Hold::Trail(k)),
                        _ => (item, Hold::Orbit(k)),
                    };
                    let at = held_position(transform, hold, held_item, 0.0, &tuning);
                    let held = ItemObject::new(held_item, kart, Motion::Held(hold), at, Vec3::ZERO);
                    spawn_object(&mut commands, held);
                }
                continue;
            }
            Use::Fire {
                item: ItemKind::Pinata,
                back,
            } => {
                // Issue #21: the piñata's item in front goes, as itself (a pelure where it was).
                let age = held_out()
                    .find_map(|(_, o)| matches!(o.held(), Some(Hold::Pinata(_))).then_some(o.age))
                    .unwrap_or(0.0);
                let k = pinata_in_front(slots.pinata, age, &tuning).unwrap_or(0);
                let mut next = *slots;
                let Some(real) = next.take_pinata(usize::from(k)) else {
                    continue;
                };
                slots.set_if_neq(next);
                let from = held_out()
                    .find(|(_, o)| o.held() == Some(Hold::Pinata(k)))
                    .map(|(e, o)| (e, o.position));
                if let Some((entity, _)) = from {
                    commands.entity(entity).try_despawn();
                }
                let at = from.map(|(_, at)| at).filter(|_| real == ItemKind::Pelure);
                (real, back, at)
            }
            Use::Release { item, back } | Use::Fire { item, back } => {
                let from = leaving(matches!(action, Use::Release { .. }));
                if let Some((entity, _)) = from {
                    commands.entity(entity).try_despawn();
                }
                // A pelure is dropped where it was held; pelotes and abeilles leave from the
                // front (or the back).
                let at = from.map(|(_, at)| at).filter(|_| item == ItemKind::Pelure);
                (item, back, at)
            }
            Use::Instant { item, back } => (item, back, None),
        };
        stats.used += 1;
        let rank = progress.map_or(0, |p| p.rank);
        match item {
            ItemKind::Piment => boost.add(tuning.piment_strength, tuning.piment_duration),
            ItemKind::PimentDore => {
                // Its window opens at the first press; every press is a piment's boost.
                let mut next = *slots;
                next.start_golden(seconds_to_ticks(tuning.golden_piment_time));
                slots.set_if_neq(next);
                boost.add(tuning.piment_strength, tuning.piment_duration);
            }
            ItemKind::CroquetteDoree => {
                if let Some(mut held) = croquettes {
                    let gained = held.gain(tuning.golden_croquettes, croquettes_tuning.max_held);
                    for _ in 0..gained {
                        picked.write(CroquettePickedUp {
                            kart,
                            at: transform.translation,
                            count: held.count,
                        });
                    }
                }
                // A croquette's mini-boost, as for every croquette (GD § 7).
                boost.add(
                    croquettes_tuning.pickup_boost_strength,
                    croquettes_tuning.pickup_boost_duration,
                );
            }
            ItemKind::SuperCalin => {
                effects.calin = tuning.super_calin_time;
                invincibility.start(tuning.super_calin_time);
            }
            ItemKind::Fusee => {
                // The autopilot drives from the next tick (`steer_rockets`); what it held out
                // stays.
                effects.rocket = tuning.fusee_time;
                invincibility.start(tuning.fusee_time);
            }
            ItemKind::PieVoleuse => {
                // Issue #21: invisible (and invincible, without blinking) for its time, and a
                // theft. Issue #88: from a racer that holds something, drawn (deterministic: the
                // race seed, its grid slot, its items used) with the leader the most likely;
                // online, this peer draws on its ranking and asks the victim's peer.
                effects.ghost = tuning.pie_time;
                invincibility.start(tuning.pie_time);
                let standings = race.as_ref().map_or(&[][..], |r| &r.standings[..]);
                let unit = steal_unit(
                    grid.as_ref().map_or(RaceGrid::GAME_SEED, |g| g.seed),
                    racer.map_or(0, |r| r.grid_slot as u64),
                    stats.used,
                );
                if let Some(victim) = steal_victim(
                    kart,
                    transform.translation,
                    standings,
                    &positions,
                    |e| holders.contains(&e),
                    unit,
                    tuning.pie_rank_power,
                ) {
                    steals.write(ItemSteal {
                        thief: kart,
                        victim,
                    });
                }
            }
            ItemKind::Chantilly | ItemKind::Degonflage | ItemKind::KlaxonCanard => {
                // Online (issue #61): with the ranking this peer sees, for every peer.
                let standings = online.as_ref().map(|_| {
                    SharedStandings::new(
                        race.iter().flat_map(|race| race.standings.iter().copied()),
                    )
                });
                effects_out.write(ItemEffect {
                    by: kart,
                    item,
                    at: transform.translation,
                    forward: *transform.forward(),
                    rank,
                    standings,
                });
            }
            _ => {
                if let Some(object) = launch.object(&tuning, item, aim_back, from) {
                    spawn_object(&mut commands, object);
                }
            }
        }
    }
}

/// Whether `kart` has one of its own `item` out (thrown, or held out around it).
fn has_out(objects: &Query<(Entity, &ItemObject)>, kart: Entity, item: ItemKind) -> bool {
    objects
        .iter()
        .any(|(_, o)| o.owner == kart && o.item == item)
}

/// One of the items `slots` held out (`hold`) broke: one of the piñata's (issue #21), else the
/// dragged one or one of an "x3".
fn lose_held(slots: &mut ItemSlots, hold: Option<Hold>) {
    match hold {
        Some(Hold::Pinata(k)) => {
            slots.take_pinata(usize::from(k));
        }
        _ => slots.lose_one_held_out(),
    }
}

/// The pie voleuse's thefts (issue #21, [`ItemSteal`]): a victim of ours gives up what it holds
/// ([`ItemSlots::steal`]), and the thief gets it, ready ([`ItemStolen`]; online, a thief or a
/// victim of another peer goes through `crate::net`). A victim with nothing left to take gives
/// nothing.
fn apply_steals(
    mut steals: MessageReader<ItemSteal>,
    mut karts: Query<&mut ItemSlots, With<Kart>>,
    mut stolen: MessageWriter<ItemStolen>,
) {
    for steal in steals.read() {
        if steal.thief == steal.victim {
            continue;
        }
        let Ok(mut victim) = karts.get_mut(steal.victim) else {
            continue;
        };
        let mut next = *victim;
        let Some((item, count)) = next.steal() else {
            continue;
        };
        victim.set_if_neq(next);
        if let Ok(mut thief) = karts.get_mut(steal.thief) {
            thief.give(item, count, 0);
        }
        stolen.write(ItemStolen {
            thief: steal.thief,
            victim: steal.victim,
            item,
            count,
        });
    }
}

/// The items that act on the others (issue #18, [`ItemEffect`]), on our karts (the victims
/// decide: online, each peer for its own; issue #61: their ranks from the ranking the user's peer
/// saw, [`SharedStandings`]):
/// - **chantilly**: the racers ahead of its user in the race (off a race: the karts ahead of
///   it) get cream on their screen, unless immune ([`immune_to_effects`]);
/// - **dégonflage**: every other kart shrinks and slows down for a time that depends on its rank
///   (the leader the longest), and drops its items, unless immune;
/// - **klaxon-canard**: every item out within its reach that is not its user's breaks (thrown,
///   on the ground, held out by another kart, the oreiller géant too). Online, its user's peer
///   judges them and tells everyone ([`ItemBlocked`]), as for a shield.
#[allow(clippy::type_complexity)]
fn apply_item_effects(
    tuning: Res<ItemTuning>,
    race: Option<Res<Race>>,
    mut commands: Commands,
    mut used: MessageReader<ItemEffect>,
    objects: Query<(Entity, &ItemObject)>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartState,
            &Invincibility,
            &mut ItemEffects,
            &mut ItemSlots,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut blocked: MessageWriter<ItemBlocked>,
    mut honked: MessageWriter<Honked>,
) {
    let racers_here = race.as_ref().map_or(0, |r| r.standings.len());
    for effect in used.read() {
        let forward = effect.forward;
        let racers = effect.standings.map_or(racers_here, |s| s.len());
        match effect.item {
            ItemKind::Chantilly | ItemKind::Degonflage => {
                for (kart, transform, state, invincibility, mut effects, mut slots, progress) in
                    &mut karts
                {
                    let finished = progress.is_some_and(|p| p.finished());
                    if kart == effect.by || immune_to_effects(state, invincibility, finished) {
                        continue;
                    }
                    let rank = match &effect.standings {
                        Some(standings) => standings.rank(kart),
                        None => progress.map_or(0, |p| p.rank),
                    };
                    if effect.item == ItemKind::Chantilly {
                        let ahead = if effect.rank > 0 && rank > 0 {
                            rank < effect.rank
                        } else {
                            (transform.translation - effect.at).dot(forward) > 0.0
                        };
                        if ahead {
                            effects.creamed = tuning.chantilly_time;
                            effects.creamed_for = tuning.chantilly_time;
                        }
                        continue;
                    }
                    let seconds = deflate_time(
                        rank,
                        racers,
                        tuning.degonflage_time_first,
                        tuning.degonflage_time_last,
                    );
                    effects.deflated = seconds;
                    effects.deflated_for = seconds;
                    let mut next = *slots;
                    if next.drop_all() {
                        slots.set_if_neq(next);
                        for (entity, object) in &objects {
                            if object.owner == kart && object.held().is_some() {
                                commands.entity(entity).try_despawn();
                            }
                        }
                    }
                }
            }
            ItemKind::KlaxonCanard => {
                for (entity, object) in &objects {
                    if object.owner == effect.by
                        || object.position.distance(effect.at) > tuning.klaxon_radius
                    {
                        continue;
                    }
                    commands.entity(entity).try_despawn();
                    if object.held().is_some()
                        && let Ok((.., mut slots, _)) = karts.get_mut(object.owner)
                    {
                        lose_held(&mut slots, object.held());
                    }
                    blocked.write(ItemBlocked {
                        object: entity,
                        owner: object.owner,
                    });
                }
                honked.write(Honked {
                    kart: effect.by,
                    at: effect.at,
                });
            }
            _ => {}
        }
    }
}

/// The fusées' autopilots (issue #18): a point of the bots' racing line ahead of the kart (its
/// lane), the fusée's speed. Without a racing line (the test ground), straight ahead. Done (or
/// past the finish line), the driver has the kart back.
fn steer_rockets(
    tuning: Res<ItemTuning>,
    line: Option<Res<RacingLine>>,
    mut karts: Query<
        (
            &Transform,
            &mut ItemEffects,
            &mut Autopilot,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
) {
    for (transform, mut effects, mut pilot, progress) in &mut karts {
        // Past the finish line, the driver has the kart back (it coasts).
        if effects.rocket > 0.0 && progress.is_some_and(|p| p.finished()) {
            effects.rocket = 0.0;
        }
        if effects.rocket <= 0.0 {
            if *pilot != Autopilot::default() {
                *pilot = Autopilot::default();
            }
            continue;
        }
        let target = match line.as_deref() {
            Some(line) => {
                let here = line.locate(transform.translation, pilot.sample);
                pilot.sample = Some(here.index);
                let ahead = here.distance + tuning.fusee_lookahead;
                line.center()
                    .at(ahead)
                    .point(line.offset_at(ahead), KART_RIDE_HEIGHT)
            }
            None => {
                transform.translation
                    + transform.forward().with_y(0.0).normalize_or(Vec3::NEG_Z)
                        * tuning.fusee_lookahead
            }
        };
        pilot.target = target;
        pilot.speed = tuning.fusee_speed;
        pilot.accel = tuning.fusee_accel;
        pilot.turn_response = tuning.fusee_turn_response;
        pilot.max_turn_rate = tuning.fusee_max_turn_rate;
    }
}

/// The ground under an item at `position` (its height, if any within `below` of its bottom).
fn ground_under(
    cast: &impl Fn(Vec3, Vec3, f32) -> Option<(f32, Vec3)>,
    position: Vec3,
    radius: f32,
    below: f32,
) -> Option<f32> {
    cast(
        position + Vec3::Y * GROUND_PROBE_UP,
        Vec3::NEG_Y,
        GROUND_PROBE_UP + radius + below,
    )
    .filter(|(_, normal)| normal.y >= WALL_NORMAL_Y)
    .map(|(distance, _)| position.y + GROUND_PROBE_UP - distance)
}

/// Moves every item out: the held ones with their kart, the pelotes rolling (rays against the
/// track), the abeilles along the centerline then diving, the bombes lobbed then lying, the
/// oreillers géants after the leader (issue #18). Too old, fallen, broken on a wall, or held by a
/// kart that no longer holds it: gone. A bombe whose fuse is over, or an oreiller géant that
/// lands, explodes ([`Blast`]). Online (issue #53), the other peers' karts are here too
/// (`kart::Remote`): what they hold out follows them (their slots as their states say), an
/// abeille goes after them; an item thrown by another peer catches up the ticks since its throw
/// ([`CatchUp`]) on its first tick here. An oreiller géant of another peer (issue #61) goes after
/// the target its thrower's peer gives it (the leader it sees), not after ours.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn move_items(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    rapier: ReadRapierContext,
    course: Option<Res<Course>>,
    race: Option<Res<Race>>,
    mut commands: Commands,
    karts: Query<
        (
            Entity,
            &Transform,
            &KartState,
            Option<&ItemSlots>,
            Option<&RaceProgress>,
            Has<Remote>,
            Option<&ItemEffects>,
        ),
        Or<(With<Kart>, With<Remote>)>,
    >,
    mut objects: Query<(Entity, &mut ItemObject, Option<&mut CatchUp>)>,
    mut blasts: MessageWriter<Blast>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let context = rapier.single().ok();
    // Only the track and the static obstacles, never a kart.
    let filter = QueryFilter::only_fixed().exclude_sensors();
    let cast = |origin: Vec3, direction: Vec3, length: f32| {
        context.as_ref().and_then(|c| {
            c.cast_ray_and_get_normal(origin, direction, length, true, filter)
                .map(|(_, hit)| (hit.time_of_impact, hit.normal))
        })
    };
    // A kart an abeille may go after: on the track, racing, not invisible (issue #21).
    let chaseable = |entity: Entity| {
        karts
            .get(entity)
            .ok()
            .filter(|(_, _, state, _, progress, _, effects)| {
                **state != KartState::Respawning
                    && !progress.is_some_and(|p| p.finished())
                    && !effects.is_some_and(|e| e.hidden())
            })
            .map(|(_, transform, _, _, progress, _, _)| {
                (transform.translation, progress.map(|p| p.distance))
            })
    };
    // Issue #96: the road's normal at a point (an anti-gravity zone's), the world's up anywhere
    // else.
    let up_at = |at: Vec3| course.as_deref().map_or(Vec3::Y, |course| course.up_at(at));
    // The leader now, for the oreillers géants.
    let leader = race
        .as_ref()
        .and_then(|race| race.standings.first().copied())
        .filter(|leader| chaseable(*leader).is_some());
    // One tick of an item; false once it is gone (`Some(at)`: it exploded there). `cast`: the
    // rays against the track (turned in the frame of an anti-gravity zone's road, issue #78);
    // `local`: a point of the world in that frame (issue #96: the boomerang's thrower).
    let step = |item: &mut ItemObject,
                exploded: &mut Option<Vec3>,
                cast: &dyn Fn(Vec3, Vec3, f32) -> Option<(f32, Vec3)>,
                local: &dyn Fn(Vec3) -> Vec3|
     -> bool {
        item.previous = item.position;
        item.age += dt;
        let radius = item.radius(&tuning);
        match item.motion {
            // Issue #21: one of a piñata's, while it is still out.
            Motion::Held(Hold::Pinata(k)) => match karts.get(item.owner) {
                Ok((_, transform, _, Some(slots), ..)) if slots.has_pinata(usize::from(k)) => {
                    item.position =
                        held_position(transform, Hold::Pinata(k), item.item, item.age, &tuning);
                    item.velocity = (item.position - item.previous) / dt;
                    true
                }
                _ => false,
            },
            Motion::Held(hold) => match karts.get(item.owner) {
                Ok((_, transform, _, Some(slots), ..))
                    if slots.held_out().is_some_and(|(held, _)| held == item.item)
                        && (hold == Hold::Dragged) == slots.dragging.is_some() =>
                {
                    item.position = held_position(transform, hold, item.item, item.age, &tuning);
                    item.velocity = (item.position - item.previous) / dt;
                    true
                }
                _ => false,
            },
            Motion::Ground => {
                // Dropped: it lands on the ground under it (a kart in the air drops it there);
                // with no ground below, it is gone.
                if item.age <= 1.5 * dt {
                    match cast(item.position + Vec3::Y, Vec3::NEG_Y, PELURE_DROP_DEPTH)
                        .filter(|(_, normal)| normal.y >= WALL_NORMAL_Y)
                    {
                        Some((distance, normal)) => {
                            let ground = item.position + Vec3::Y * (1.0 - distance);
                            item.position = ground + normal * radius * PELURE_LIFT;
                            item.previous = item.position;
                        }
                        None if context.is_some() => item.age = f32::INFINITY,
                        None => {}
                    }
                }
                item.age < tuning.pelure_lifetime
            }
            Motion::Rolling { bounces_left, fall } => {
                let mut rolling = Roll {
                    position: item.position,
                    velocity: item.velocity,
                    fall,
                    bounces_left,
                };
                let rolled = roll(&mut rolling, radius, dt, cast);
                item.position = rolling.position;
                item.velocity = rolling.velocity;
                item.motion = Motion::Rolling {
                    bounces_left: rolling.bounces_left,
                    fall: rolling.fall,
                };
                let lifetime = if item.item == ItemKind::Abeille {
                    tuning.abeille_lifetime
                } else {
                    tuning.pelote_lifetime
                };
                !matches!(rolled, RollStep::Broke | RollStep::Fell) && item.age < lifetime
            }
            Motion::Bomb {
                mut vertical,
                mut grounded,
                impact,
            } => {
                // Across: straight on; a wall stops it (it drops at its foot).
                let flat = item.velocity.with_y(0.0);
                let length = flat.length() * dt;
                if length > 1e-6 {
                    let direction = flat / flat.length();
                    match cast(item.position, direction, length + radius) {
                        Some((distance, normal)) if normal.y.abs() < WALL_NORMAL_Y => {
                            item.position += direction * (distance - radius).max(0.0);
                            item.velocity = Vec3::ZERO;
                        }
                        _ => item.position += direction * length,
                    }
                }
                if grounded {
                    // Sliding to a stop on the ground (down slopes too).
                    let speed = item.velocity.length();
                    let slowed = (speed - tuning.bombe_ground_decel * dt).max(0.0);
                    item.velocity = item.velocity.normalize_or_zero() * slowed;
                    match ground_under(&cast, item.position, radius, GROUND_PROBE_DOWN) {
                        Some(ground) => item.position.y = ground + radius,
                        None => grounded = false,
                    }
                } else {
                    // Lobbed (or falling off an edge).
                    vertical -= ITEM_GRAVITY * dt;
                    item.position.y += vertical * dt;
                    let below = (-vertical * dt).max(0.0) + 0.05;
                    if vertical <= 0.0
                        && let Some(ground) = ground_under(&cast, item.position, radius, below)
                        && item.position.y - radius <= ground + 0.02
                    {
                        item.position.y = ground + radius;
                        // Thrown ahead, it explodes where it lands.
                        if impact {
                            *exploded = Some(item.position);
                            return false;
                        }
                        vertical = 0.0;
                        grounded = true;
                    }
                    if -vertical >= FALL_LIMIT_SPEED {
                        return false;
                    }
                }
                item.motion = Motion::Bomb {
                    vertical,
                    grounded,
                    impact,
                };
                if item.age >= tuning.bombe_fuse {
                    *exploded = Some(item.position);
                    return false;
                }
                true
            }
            Motion::Pillow { target, flight } => {
                // After the leader now (the one it had, without a race). Online (issue #61), one
                // of another peer goes after the leader its thrower's peer sees (`Retarget`).
                let ours = karts
                    .get(item.owner)
                    .is_ok_and(|(_, _, _, _, _, remote, _)| !remote);
                let leader = if ours { leader } else { None };
                let target = leader.or(target).filter(|t| chaseable(*t).is_some());
                let chased = target.and_then(chaseable);
                let flight = match (flight, course.as_deref(), chased) {
                    (PillowFlight::Track { distance, lateral }, Some(course), _) => {
                        let distance = distance + tuning.oreiller_speed * dt;
                        let sample = course.center.at(distance);
                        let mut lateral = lateral;
                        if let Some((at, _)) = chased {
                            let wanted = (at - sample.position).dot(sample.right);
                            let most = tuning.oreiller_lateral_speed * dt;
                            lateral += (wanted - lateral).clamp(-most, most);
                        }
                        let room = (sample.half_width - 0.5).max(0.0);
                        lateral = lateral.clamp(-room, room);
                        item.position = sample.point(lateral, tuning.oreiller_height);
                        item.velocity = (item.position - item.previous) / dt;
                        // Over its target (or past it: a leader that threw it): it hovers.
                        match chased {
                            Some((_, Some(along))) if along <= distance + tuning.oreiller_reach => {
                                PillowFlight::Hover {
                                    left: tuning.oreiller_hover_time,
                                }
                            }
                            Some((at, None))
                                if at.distance(item.position) < tuning.oreiller_reach =>
                            {
                                PillowFlight::Hover {
                                    left: tuning.oreiller_hover_time,
                                }
                            }
                            _ => PillowFlight::Track { distance, lateral },
                        }
                    }
                    (PillowFlight::Track { .. } | PillowFlight::Hover { .. }, _, Some((at, _))) => {
                        // Above its target (it catches up from afar first). Issue #96: along the
                        // road's normal there (on a wall, under a ceiling: on its side of the
                        // road, never through it).
                        let up = up_at(at);
                        let over = at + up * tuning.oreiller_height;
                        let to = over - item.position;
                        let most = tuning.oreiller_speed * dt;
                        item.position += to.clamp_length_max(most);
                        item.velocity = (item.position - item.previous) / dt;
                        let left = match flight {
                            PillowFlight::Hover { left } if to.length() < tuning.oreiller_reach => {
                                if left.is_finite() {
                                    left - dt
                                } else {
                                    tuning.oreiller_hover_time
                                }
                            }
                            PillowFlight::Hover { left } => left,
                            _ => tuning.oreiller_hover_time,
                        };
                        if left <= 0.0 {
                            PillowFlight::Drop
                        } else {
                            PillowFlight::Hover { left }
                        }
                    }
                    (PillowFlight::Drop, _, Some((at, _))) => {
                        // Down on its target, wherever it drives (issue #96: along the road's
                        // normal there).
                        let up = up_at(at);
                        if up == Vec3::Y {
                            let y = item.position.y - tuning.oreiller_drop_speed * dt;
                            if y - radius <= at.y + KART_HALF_EXTENTS.y {
                                *exploded = Some(at);
                                return false;
                            }
                            item.position = Vec3::new(at.x, y, at.z);
                        } else {
                            let height =
                                (item.position - at).dot(up) - tuning.oreiller_drop_speed * dt;
                            if height - radius <= KART_HALF_EXTENTS.y {
                                *exploded = Some(at);
                                return false;
                            }
                            item.position = at + up * height;
                        }
                        item.velocity = (item.position - item.previous) / dt;
                        PillowFlight::Drop
                    }
                    (_, _, None) => {
                        // No one to go after: straight down where it is (issue #96: down to the
                        // road of an anti-gravity zone along its normal).
                        let up = up_at(item.position);
                        if up == Vec3::Y {
                            let ground = ground_under(&cast, item.position, radius, 50.0);
                            item.position.y -= tuning.oreiller_drop_speed * dt;
                            if ground.is_none_or(|g| item.position.y - radius <= g) {
                                *exploded = Some(item.position);
                                return false;
                            }
                        } else {
                            let reach = GROUND_PROBE_UP + radius + 50.0;
                            let ground = cast(item.position + up * GROUND_PROBE_UP, -up, reach)
                                .filter(|(_, normal)| normal.dot(up) >= WALL_NORMAL_Y)
                                .map(|(distance, _)| GROUND_PROBE_UP - distance);
                            let fall = tuning.oreiller_drop_speed * dt;
                            item.position -= up * fall;
                            if ground.is_none_or(|g| g + fall >= -radius) {
                                *exploded = Some(item.position);
                                return false;
                            }
                        }
                        PillowFlight::Drop
                    }
                };
                item.motion = Motion::Pillow { target, flight };
                item.age < tuning.oreiller_lifetime
            }
            Motion::Boomerang {
                mut returning,
                mut travelled,
                bounces_left,
                fall,
                curve,
            } => {
                // Issue #21: out, curving (issue #88: as its thrower steered); then back to its
                // thrower (where its body is: online, where this peer shows it).
                let heading = item.velocity.with_y(0.0).normalize_or(Vec3::NEG_Z);
                let owner = karts
                    .get(item.owner)
                    .ok()
                    .map(|(_, t, ..)| local(t.translation));
                let direction = match (returning, owner) {
                    (false, _) => Quat::from_rotation_y(curve * dt) * heading,
                    (true, Some(owner)) => boomerang_heading(
                        heading,
                        owner - item.position,
                        dt,
                        tuning.boomerang_turn_rate,
                        tuning.boomerang_home_distance,
                    ),
                    (true, None) => heading,
                };
                let mut rolling = Roll {
                    position: item.position,
                    velocity: direction * tuning.boomerang_speed,
                    fall,
                    bounces_left,
                };
                let rolled = roll(&mut rolling, radius, dt, cast);
                item.position = rolling.position;
                item.velocity = rolling.velocity;
                travelled += tuning.boomerang_speed * dt;
                let mut bounces_left = rolling.bounces_left;
                if !returning
                    && (rolled == RollStep::Bounced || travelled >= tuning.boomerang_range)
                {
                    returning = true;
                    bounces_left = tuning.boomerang_bounces;
                }
                item.motion = Motion::Boomerang {
                    returning,
                    travelled,
                    bounces_left,
                    fall: rolling.fall,
                    curve,
                };
                // Back in its thrower's hand: gone, the next throw is ready.
                let caught = returning
                    && owner.is_some_and(|owner| {
                        owner.with_y(0.0).distance(item.position.with_y(0.0))
                            <= tuning.boomerang_catch
                            && (owner.y - item.position.y).abs() < 3.0
                    });
                !caught
                    && !matches!(rolled, RollStep::Broke | RollStep::Fell)
                    && item.age < tuning.boomerang_lifetime
            }
            Motion::Homing { target, flight } => {
                let mut target = target.filter(|t| chaseable(*t).is_some());
                let flight = match (flight, course.as_deref()) {
                    (Flight::Track { distance, lateral }, Some(course)) => {
                        let distance = distance + tuning.abeille_speed * dt;
                        let sample = course.center.at(distance);
                        let chased = target.and_then(chaseable);
                        let mut lateral = lateral;
                        if let Some((at, _)) = chased {
                            let wanted = (at - sample.position).dot(sample.right);
                            let most = tuning.abeille_lateral_speed * dt;
                            lateral += (wanted - lateral).clamp(-most, most);
                        }
                        let room = (sample.half_width - 0.5).max(0.0);
                        lateral = lateral.clamp(-room, room);
                        item.position = sample.point(lateral, tuning.abeille_height);
                        item.velocity = (item.position - item.previous) / dt;
                        // Close enough, along the track too (not across the infield): dive.
                        let near = |at: Vec3, along: Option<f32>| {
                            at.distance(item.position) < tuning.abeille_dive_distance
                                && along.is_none_or(|d| {
                                    (d - distance).abs() < 2.0 * tuning.abeille_dive_distance
                                })
                        };
                        if target.is_none() {
                            // The leader's abeille: the first kart it comes close to.
                            target = karts
                                .iter()
                                .filter(|(e, ..)| *e != item.owner)
                                .filter_map(|(e, ..)| chaseable(e).map(|c| (e, c)))
                                .find(|(_, (at, along))| near(*at, *along))
                                .map(|(e, _)| e);
                        }
                        match target.and_then(chaseable) {
                            Some((at, along)) if near(at, along) => Flight::Dive,
                            _ => Flight::Track { distance, lateral },
                        }
                    }
                    _ => {
                        // Diving at its target, or straight on without one (or off a race).
                        if let Some((at, _)) = target.and_then(chaseable) {
                            let to = at - item.position;
                            let most = tuning.abeille_dive_speed * dt;
                            item.velocity = to.normalize_or(item.velocity.normalize_or_zero())
                                * tuning.abeille_dive_speed;
                            item.position = if to.length() <= most {
                                at
                            } else {
                                item.position + item.velocity * dt
                            };
                        } else {
                            item.position += item.velocity * dt;
                        }
                        Flight::Dive
                    }
                };
                item.motion = Motion::Homing { target, flight };
                item.age < tuning.abeille_lifetime
            }
        }
    };
    // Issue #78: over the road of an anti-gravity zone, an item that lies, rolls or bounces on the
    // road (a pelure, a pelote, a bombe) does it in the road's frame, its normal as the item's up:
    // the same motion, turned. Issue #96: the boomerang-os too (it flies low over the road, and
    // curves around its normal), its thrower seen in that frame. Everywhere else, exactly the
    // motion before #78.
    let step_on_road = |item: &mut ItemObject, exploded: &mut Option<Vec3>| -> bool {
        let up = match (item.motion, course.as_deref()) {
            (
                Motion::Ground
                | Motion::Rolling { .. }
                | Motion::Bomb { .. }
                | Motion::Boomerang { .. },
                Some(course),
            ) => course.up_at(item.position),
            _ => Vec3::Y,
        };
        if up == Vec3::Y {
            return step(item, exploded, &cast, &|p| p);
        }
        let to_road = Quat::from_rotation_arc(up, Vec3::Y);
        let to_world = to_road.inverse();
        let origin = item.position;
        let world = |p: Vec3| origin + to_world * p;
        let road_cast = |from: Vec3, direction: Vec3, length: f32| {
            cast(world(from), to_world * direction, length).map(|(t, normal)| (t, to_road * normal))
        };
        let mut local = ItemObject {
            position: Vec3::ZERO,
            previous: to_road * (item.previous - origin),
            velocity: to_road * item.velocity,
            ..*item
        };
        let mut blast = None;
        let alive = step(&mut local, &mut blast, &road_cast, &|p| {
            to_road * (p - origin)
        });
        *item = ItemObject {
            position: world(local.position),
            previous: world(local.previous),
            velocity: to_world * local.velocity,
            ..local
        };
        *exploded = blast.map(world);
        alive
    };
    for (entity, mut object, catch_up) in &mut objects {
        let mut item = *object;
        // An item from another peer: the ticks since its throw first, then this one. Its
        // contacts sweep the whole way (`previous` where it was received).
        let behind = catch_up.map_or(0, |mut catch_up| std::mem::take(&mut catch_up.0));
        let from = item.position;
        let mut alive = true;
        let mut exploded = None;
        for _ in 0..=behind {
            alive = step_on_road(&mut item, &mut exploded);
            if !alive {
                break;
            }
        }
        if behind > 0
            && matches!(
                item.motion,
                Motion::Rolling { .. }
                    | Motion::Homing { .. }
                    | Motion::Bomb { .. }
                    | Motion::Boomerang { .. }
            )
        {
            item.previous = from;
        }
        if alive {
            if *object != item {
                *object = item;
            }
        } else {
            commands.entity(entity).try_despawn();
            if let Some(at) = exploded {
                blasts.write(Blast {
                    at,
                    radius: blast_radius(item.item, &tuning),
                    item: item.item,
                    owner: item.owner,
                    object: Some(entity),
                    announce: false,
                });
            }
        }
    }
}

/// The heading of a boomerang-os coming back (issue #21): from `heading` toward its thrower
/// (`to`: from the boomerang to it), turning at most `turn_rate` (rad/s) over `dt`, or straight at
/// it once within `home` m (it never circles around its thrower). Horizontal.
pub fn boomerang_heading(heading: Vec3, to: Vec3, dt: f32, turn_rate: f32, home: f32) -> Vec3 {
    let heading = heading.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let flat = to.with_y(0.0);
    let Some(want) = flat.try_normalize() else {
        return heading;
    };
    if flat.length() <= home {
        return want;
    }
    let angle = heading.cross(want).y.atan2(heading.dot(want));
    let most = turn_rate * dt;
    Quat::from_rotation_y(angle.clamp(-most, most)) * heading
}

/// How far an explosion of `item` reaches (m).
pub fn blast_radius(item: ItemKind, tuning: &ItemTuning) -> f32 {
    match item {
        ItemKind::OreillerGeant => tuning.oreiller_blast_radius,
        _ => tuning.bombe_blast_radius,
    }
}

/// A dropped pelure finds the ground up to this far below it (m); further, it is gone.
const PELURE_DROP_DEPTH: f32 = 40.0;
/// A pelure lies flat: its center is this share of its radius above the ground.
const PELURE_LIFT: f32 = 0.5;

/// One item out, as the contacts see it.
#[derive(Clone, Copy, Debug)]
struct Contact {
    entity: Entity,
    item: ItemKind,
    owner: Entity,
    from: Vec3,
    to: Vec3,
    radius: f32,
    age: f32,
    held: Option<Hold>,
    moving: bool,
}

impl Contact {
    /// It explodes on what it touches (issue #18): a bombe out (issue #21: one of a piñata's,
    /// held out around its kart, breaks like a shield, or hits the kart it touches, without a
    /// blast that would knock over its own kart).
    fn explodes(&self) -> bool {
        self.item == ItemKind::Bombe && self.held.is_none()
    }

    /// Nothing stops it but a klaxon-canard (issue #18): an oreiller géant.
    fn unstoppable(&self) -> bool {
        self.item == ItemKind::OreillerGeant
    }
}

/// Contacts this tick, along each item's path since the last tick: two items of different karts
/// touching break each other (a held item is a shield); an item touching a kart breaks on it
/// and tells the kart ([`ItemHit`]). A kart's own items never touch each other; its own pelote,
/// pelure or bombe can hit it after `own_item_delay`, its own abeille never. A bombe explodes on
/// what it touches ([`Blast`]); an oreiller géant goes through the other items, and explodes on
/// a kart it falls on (issue #18).
///
/// Online (issue #53), each peer judges what is its own to judge, and tells the others
/// (`crate::net`): an item touching one of **our** karts (the victim decides; the other peers'
/// karts are not here); a shield **we** hold out against anything (the holder decides: what
/// another peer's kart holds out is only drawn here); two items out when one of them is ours.
#[allow(clippy::too_many_arguments)]
fn collide_items(
    tuning: Res<ItemTuning>,
    online: Option<Res<Online>>,
    mut commands: Commands,
    mut items: Local<Vec<Contact>>,
    mut broken: Local<Vec<bool>>,
    mut bodies: Local<Vec<(Entity, Transform)>>,
    objects: Query<(Entity, &ItemObject)>,
    karts: Query<(Entity, &Transform, &KartState, Option<&ItemEffects>), With<Kart>>,
    mut slots: Query<(&mut ItemSlots, &mut ItemStats)>,
    mut hits: MessageWriter<ItemHit>,
    mut blocked: MessageWriter<ItemBlocked>,
    mut blasts: MessageWriter<Blast>,
) {
    items.clear();
    items.extend(objects.iter().map(|(entity, o)| Contact {
        entity,
        item: o.item,
        owner: o.owner,
        from: o.previous,
        to: o.position,
        radius: o.radius(&tuning),
        age: o.age,
        held: o.held(),
        moving: o.motion != Motion::Ground,
    }));
    if items.is_empty() {
        return;
    }
    bodies.clear();
    // Issue #21: the items go through an invisible kart (a pie voleuse).
    bodies.extend(
        karts
            .iter()
            .filter(|(_, _, state, effects)| {
                **state != KartState::Respawning && !effects.is_some_and(|e| e.hidden())
            })
            .map(|(e, t, ..)| (e, *t)),
    );
    broken.clear();
    broken.resize(items.len(), false);
    // Whether two items meet here: always solo; online, see above.
    let ours = |owner: Entity| karts.contains(owner);
    let judged = |a: &Contact, b: &Contact| {
        let shown_shield = |c: &Contact| c.held.is_some() && !ours(c.owner);
        online.is_none()
            || (!shown_shield(a) && !shown_shield(b) && (ours(a.owner) || ours(b.owner)))
    };
    let burst = |c: &Contact, at: Vec3, blasts: &mut MessageWriter<Blast>| {
        blasts.write(Blast {
            at,
            radius: blast_radius(c.item, &tuning),
            item: c.item,
            owner: c.owner,
            object: Some(c.entity),
            announce: online.is_some(),
        });
    };
    for i in 0..items.len() {
        if broken[i] {
            continue;
        }
        let a = items[i];
        // The first thing along its path: another item, or a kart.
        let mut first: Option<(f32, Result<usize, Entity>)> = None;
        let mut consider = |t: f32, what: Result<usize, Entity>| {
            if first.is_none_or(|(best, _)| t < best) {
                first = Some((t, what));
            }
        };
        for (j, b) in items.iter().enumerate() {
            if j == i
                || broken[j]
                || b.owner == a.owner
                || !(a.moving || b.moving)
                || a.unstoppable()
                || b.unstoppable()
                || !judged(&a, b)
            {
                continue;
            }
            if let Some(t) = segment_sphere(a.from, a.to, b.to, a.radius + b.radius) {
                consider(t, Ok(j));
            }
        }
        for (kart, transform) in bodies.iter() {
            // Its own abeille never hits its thrower, nor its own boomerang-os (issue #21: it
            // catches it).
            if *kart == a.owner
                && (a.held.is_some()
                    || matches!(a.item, ItemKind::Abeille | ItemKind::Boomerang)
                    || a.age < tuning.own_item_delay)
            {
                continue;
            }
            let inverse = transform.rotation.inverse();
            let from = inverse * (a.from - transform.translation);
            let to = inverse * (a.to - transform.translation);
            if let Some(t) = segment_box(from, to, KART_HALF_EXTENTS + Vec3::splat(a.radius)) {
                // An item meets a shield before the kart behind it.
                consider(t + 1e-4, Err(*kart));
            }
        }
        match first {
            Some((t, Ok(j))) => {
                broken[i] = true;
                broken[j] = true;
                for c in [a, items[j]] {
                    if c.held.is_some()
                        && let Ok((mut held, mut stats)) = slots.get_mut(c.owner)
                    {
                        lose_held(&mut held, c.held);
                        stats.blocked += 1;
                    }
                    if c.explodes() {
                        burst(&c, c.from.lerp(c.to, t.min(1.0)), &mut blasts);
                    } else {
                        blocked.write(ItemBlocked {
                            object: c.entity,
                            owner: c.owner,
                        });
                    }
                }
            }
            Some((t, Err(kart))) => {
                broken[i] = true;
                if a.held.is_some()
                    && let Ok((mut held, _)) = slots.get_mut(a.owner)
                {
                    lose_held(&mut held, a.held);
                }
                if a.explodes() || a.unstoppable() {
                    burst(&a, a.from.lerp(a.to, t.min(1.0)), &mut blasts);
                } else {
                    hits.write(ItemHit {
                        kart,
                        by: a.owner,
                        item: a.item,
                        object: Some(a.entity),
                    });
                }
            }
            None => {}
        }
    }
    for (i, c) in items.iter().enumerate() {
        if broken[i] {
            commands.entity(c.entity).try_despawn();
        }
    }
}

/// Explosions ([`Blast`], issue #18): every kart of ours within their reach is hit (the victim
/// decides: immune or not), its shields do not protect it.
fn explode(
    mut blasts: MessageReader<Blast>,
    karts: Query<(Entity, &Transform, &KartState), With<Kart>>,
    mut hits: MessageWriter<ItemHit>,
) {
    for blast in blasts.read() {
        for (kart, transform, state) in &karts {
            if *state == KartState::Respawning
                || transform.translation.distance(blast.at) > blast.radius
            {
                continue;
            }
            hits.write(ItemHit {
                kart,
                by: blast.owner,
                item: blast.item,
                object: blast.object,
            });
        }
    }
}

/// The victim decides (docs/ARCHITECTURE.md): an item touching a kart hits it unless it is
/// immune ([`immune_to_items`]). Hit: it spins out or tumbles (`KartState::Hit`, which loses its
/// drift; its boost and what it held out are lost too), and loses its croquettes. Its
/// invincibility starts when the spin or tumble ends (the kart's own rules). One hit per kart and
/// tick (an explosion and a kart that rams it on the same tick: the first one).
fn apply_item_hits(
    tuning: Res<ItemTuning>,
    mut commands: Commands,
    mut hits: MessageReader<ItemHit>,
    objects: Query<(Entity, &ItemObject)>,
    mut karts: Query<
        (
            &mut KartState,
            &mut Boost,
            &Invincibility,
            &mut ItemSlots,
            &mut ItemStats,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut losses: MessageWriter<LoseCroquettes>,
    mut judged: MessageWriter<ItemHitJudged>,
) {
    for hit in hits.read() {
        let Ok((mut state, mut boost, invincibility, mut slots, mut stats, progress)) =
            karts.get_mut(hit.kart)
        else {
            continue;
        };
        let immune = immune_to_items(
            &state,
            invincibility,
            progress.is_some_and(|p| p.finished()),
        );
        judged.write(ItemHitJudged {
            kart: hit.kart,
            by: hit.by,
            item: hit.item,
            object: hit.object,
            landed: !immune,
        });
        if immune {
            continue;
        }
        let reaction = reaction_to(hit.item);
        let seconds = match reaction {
            HitReaction::Spin => tuning.spin_time,
            HitReaction::Tumble => tuning.tumble_time,
        };
        *state = KartState::hit(reaction, seconds, TICK_HZ as f32);
        *boost = Boost::default();
        stats.hits_taken += 1;
        if slots.lose_held_out() {
            for (entity, object) in &objects {
                if object.owner == hit.kart && object.held().is_some() {
                    commands.entity(entity).try_despawn();
                }
            }
        }
        losses.write(LoseCroquettes {
            kart: hit.kart,
            cause: LossCause::ItemHit,
        });
        if let Ok((.., mut by, _)) = karts.get_mut(hit.by)
            && hit.by != hit.kart
        {
            by.hits_landed += 1;
        }
    }
}

/// The top speed of the item effects (issue #18): faster in a super câlin, slower deflated
/// (`MaxSpeedFactors::items`); and how small a deflated kart looks, for its visual.
fn update_item_speeds(
    tuning: Res<ItemTuning>,
    mut scale: ResMut<DeflatedScale>,
    mut karts: Query<(&ItemEffects, &mut MaxSpeedFactors)>,
) {
    for (effects, mut factors) in &mut karts {
        let factor = effects.speed_factor(tuning.super_calin_speed, tuning.degonflage_speed);
        if factors.items != factor {
            factors.items = factor;
        }
    }
    scale.set_if_neq(DeflatedScale(tuning.degonflage_scale));
}

/// After the physics step (issue #18): a kart in a super câlin or a fusée knocks over every kart
/// of ours it is in contact with (a tumble on the next tick; the victim decides, immune or not).
/// Online (issue #61), the other peers' karts too (`kart::Remote`: their states say they ram):
/// their contacts with ours are judged here, ours with theirs on their peers.
fn ram_karts(
    rapier: ReadRapierContext,
    rammers: Query<(Entity, &ItemEffects), Or<(With<Kart>, With<Remote>)>>,
    victims: Query<(), With<Kart>>,
    mut hits: MessageWriter<ItemHit>,
) {
    if rammers.iter().all(|(_, effects)| !effects.rams()) {
        return;
    }
    let Ok(context) = rapier.single() else {
        return;
    };
    for (rammer, effects) in &rammers {
        if !effects.rams() {
            continue;
        }
        let item = if effects.rocket > 0.0 {
            ItemKind::Fusee
        } else {
            ItemKind::SuperCalin
        };
        for pair in context.contact_pairs_with(rammer) {
            let (Some(c1), Some(c2)) = (pair.collider1(), pair.collider2()) else {
                continue;
            };
            let other = if c1 == rammer { c2 } else { c1 };
            if other == rammer || !victims.contains(other) || !pair.has_any_active_contact() {
                continue;
            }
            hits.write(ItemHit {
                kart: other,
                by: rammer,
                item,
                object: None,
            });
        }
    }
}

/// Debug (issue #18): the I key gives the player the next item of the list, the admin panel any
/// ([`GiveItem`]), ready in its hand. Online, only in a debug build, and only the items that play
/// online.
fn give_items(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    online: Option<Res<Online>>,
    tuning: Res<ItemTuning>,
    mut next: Local<usize>,
    mut gifts: MessageReader<GiveItem>,
    mut karts: Query<&mut ItemSlots, With<PlayerKart>>,
) {
    let online = online.is_some();
    let pressed = keys.is_some_and(|keys| keys.just_pressed(DEBUG_ITEM_KEY));
    if (online && !cfg!(debug_assertions)) || !tuning.enabled {
        gifts.clear();
        return;
    }
    let playable = |prize: &Prize| !online || prize.online();
    let mut give = |prize: Prize| {
        let Some((item, count)) = prize.item() else {
            return;
        };
        for mut slots in &mut karts {
            slots.force(item, count);
        }
    };
    if pressed {
        for _ in 0..Prize::ALL.len() {
            let prize = Prize::ALL[*next % Prize::ALL.len()];
            *next = (*next + 1) % Prize::ALL.len();
            if playable(&prize) {
                info!("debug: {} in the hand ({:?})", prize.name(), DEBUG_ITEM_KEY);
                give(prize);
                break;
            }
        }
    }
    for gift in gifts.read() {
        if playable(&gift.0) {
            give(gift.0);
        }
    }
}
