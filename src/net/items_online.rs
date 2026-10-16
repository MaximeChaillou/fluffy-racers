//! Items and croquettes online (issue #53, `docs/ARCHITECTURE.md` § « Objets et croquettes en
//! ligne »): every peer runs the item and croquette rules for its own karts (its player's, and
//! the bots when it hosts), and tells the others what they need, on the reliable channel.
//!
//! - **Boxes and croquettes** (`pickups.rs`): a kart of ours that touches a box or a croquette
//!   takes it at once (its roulette spins, its croquette counts: a prediction) and we send a
//!   [`Claim`]. The host decides ([`Arbiter`]) and sends its [`Verdict`] to everyone: granted, the
//!   box or the croquette is gone on every screen until it comes back, on the same tick for all;
//!   refused (another kart was first), our roulettes stop, empty (`ItemSlots::revoke`), and a
//!   croquette's count goes back down (its mini-boost stays: it is over by then). The roulette is
//!   drawn by the kart's peer, deterministically: the claim carries its draw number and its
//!   distance to the leader, so every peer can draw the same items (`items::draw_unit`).
//! - **Items thrown or dropped** ([`ItemOut`]): the thrower has authority over the path. It tells
//!   the item's state at a common tick; every peer simulates it from there, catching up the ticks
//!   it took to arrive (`items::CatchUp`): rays against the same track, the same centerline, the
//!   same target (an abeille goes after the kart its thrower saw just ahead, by id).
//! - **The victim decides**: an item touching one of **our** karts is judged here (immune or
//!   not), and we tell everyone ([`ItemBreak`]): the item breaks on every screen, the thrower
//!   counts its hit. The others see the kart spin or tumble, blink, through its states. An item
//!   meeting a shield is judged by the shield's holder; two items out, by the peer of either.
//! - **What a kart holds out** (dragged, an "x3" around it) travels in its states
//!   (`KartSnapshot::items`): every peer draws it around the kart's body ([`show_held_items`]).
//! - **Croquettes lost** (a bump, a paw, an item): the victim tells where they fly ([`Scatter`]);
//!   every peer shows them at the same places, of the same age. Picking one up is a claim.
//! - **Phase 2** (issue #18): the **piment doré** is the thrower's own boost (its flames show);
//!   a **bombe** goes out like the others ([`ItemOut`], its lob and its fuse simulated on every
//!   screen); it explodes by itself at the end of its fuse (or where it lands, thrown ahead) on
//!   every screen on the same tick, and on what touches it where the peer that judged the
//!   contact says ([`BlastOut`]); every peer judges its own karts in the blast. The
//!   **klaxon-canard**: its peer judges the items it destroys (as a shield's holder does) and
//!   tells each ([`ItemBreak`]); the others see its shock wave ([`Honk`]).
//! - **Phase 2, the rest** (issue #61): the **chantilly** and the **dégonflage** are told by their
//!   user's peer ([`EffectOut`]) with its ranking at that moment: every peer applies them to its
//!   own karts (the victim decides: immune or not), their rank taken from that ranking. A kart in
//!   a **super câlin** or a **fusée** says so in its states: it glows or wears its shell on every
//!   screen, and a contact of one of **our** karts with it knocks ours over (the victim decides),
//!   which we tell ([`Rammed`]): its rammer counts its hit. The fusée is driven by its own peer.
//!   Deflated, a kart says so in its states (smaller on every screen). The **oreiller géant**: a
//!   draw that gives one is a claim to the host (`Pickup::Pillow`, its rules in `pickups.rs`);
//!   refused, the kart gets the same draw without it (as solo). Thrown, it is an [`ItemOut`]
//!   after the leader of its thrower's ranking, by id, and its thrower's peer tells when that
//!   leader changes ([`Retarget`]); each peer judges its karts in its crush. Gone without a throw
//!   (a dégonflage), its kart's peer says so ([`PillowGone`]).
//! - **Phase 3** (issue #21): the **boomerang-os** goes out like the others ([`ItemOut`],
//!   `OutMotion::Boomerang`); on its way back it goes after its thrower's body on each screen,
//!   and its thrower's peer tells when it caught it (an [`ItemBreak`] without a kart: gone on
//!   every screen). The **piñata** out travels in its kart's states (`KartSnapshot::pinata`):
//!   every peer draws its items around the kart. The **pie voleuse**: invisible, its kart says so
//!   in its states (invincible meanwhile, a faint ghost on the others' screens); its theft is
//!   asked of the victim's peer ([`Steal`]: the victim chosen among the karts whose states say
//!   they hold an item to take), which gives up what its kart holds then and tells everyone
//!   ([`Stolen`]): the thief's peer puts it in its slot.
//! - **The host** (issue #63: the room's creator, from start to end) decides every claim, and
//!   only its verdicts count. The items thrown by a peer that leaves disappear; the host leaving
//!   ends the room.
//!
//! Every message carries the session of the kart states (`KartSync::session`): a race's messages
//! never mix with the free driving's. Nothing here runs without a room.

use bevy::prelude::*;

use super::online::OnlineRace;
use super::pickups::{Arbiter, ClaimKey, PillowRules};
use super::protocol::{
    BlastOut, Claim, EFFECT_ITEMS, EffectOut, Honk, ItemBreak, ItemId, ItemOut, KartId, LooseId,
    MAX_SCATTER, Message, OUT_ITEMS, OutMotion, OutPillow, PeerKey, Pickup, PillowGone, RAM_ITEMS,
    Rammed, Retarget, Scatter, Spot, Steal, Stolen, Verdict,
};
use super::sync::{KartSync, NetKart, RemoteKartProxy};
use super::{NetSession, online::local_tick};
use crate::croquettes::{
    CroquetteSystems, CroquetteTaken, CroquetteTuning, Croquettes, CroquettesLost, LooseCroquette,
    TrackCroquette,
};
use crate::items::{
    Blast, BoxBroken, CatchUp, Flight, Hold, Honked, ItemBlocked, ItemBox, ItemEffect,
    ItemHitJudged, ItemKind, ItemObject, ItemSlots, ItemStats, ItemSteal, ItemStolen, ItemSystems,
    ItemTable, ItemTuning, Motion, OnlineItemTable, PINATA_ITEMS, PillowFlight, Prize,
    SharedStandings, blast_radius, draw_unit, held_offset, item_radius,
};
use crate::kart::{Kart, KartSystems};
use crate::race::{Race, RaceGrid, seconds_to_ticks};
use crate::track::{GRID_SLOTS, SpotIndex};

/// An item from another peer catches up this many ticks at most (2 s): older, it is late anyway.
const MAX_CATCH_UP: u32 = 120;
/// Items broken lately, kept to drop an [`ItemOut`] that comes after its break, at most.
const BROKEN_KEPT: usize = 256;
/// Items exploded lately (issue #18), kept for the victims' decisions of their blast, at most.
const EXPLODED_KEPT: usize = 32;
/// The other karts' oreillers géants waiting for the host's word, kept at most (issue #61).
const PILLOW_DRAWS_KEPT: usize = 16;

/// What we take back if the host gives a box or a croquette to another kart.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Undo {
    /// The roulettes started in the hand, the reserve.
    Box([bool; 2]),
    /// The croquettes it counted.
    Croquettes(u8),
    /// An oreiller géant (issue #61): the prize it gets instead.
    Pillow(Prize),
}

/// One of our claims, until its verdict.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mine {
    claim: Claim,
    kart: Entity,
    undo: Undo,
}

/// What happened, the same on every screen (the tests compare two games; the console).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemLog {
    /// Items thrown or dropped: which, what, the common tick.
    pub thrown: Vec<(ItemId, ItemKind, u32)>,
    /// Items broken on a kart (the victim's decision: landed or not), or on another item, and
    /// the common tick this peer learned it on.
    pub breaks: Vec<(ItemId, Option<KartId>, bool, u32)>,
    /// Croquettes lost: the kart, its loss, the common tick, where they land.
    pub scatters: Vec<(KartId, u16, u32, Vec<[f32; 3]>)>,
    /// The boxes the other karts took, and the items they drew from them, as this peer draws
    /// them again: kart, common tick, items.
    pub draws: Vec<(KartId, u32, Vec<(ItemKind, u8)>)>,
    /// Our claims refused: the spot, our kart, the tick.
    pub refused: Vec<ClaimKey>,
    /// Bombes exploded on what touched them (issue #18): which, and the common tick (on every
    /// screen, the one the peer that judged it told).
    pub blasts: Vec<(ItemId, u32)>,
    /// Klaxons sounded (issue #18): by which kart, the common tick.
    pub honks: Vec<(KartId, u32)>,
    /// Chantilly and dégonflages used (issue #61): by which kart, which, the common tick (its
    /// user's).
    pub effects: Vec<(KartId, ItemKind, u32)>,
    /// Karts knocked over by a super câlin or a fusée (issue #61, the victim's decision): the
    /// victim, its rammer, which, the common tick (the victim's).
    pub rams: Vec<(KartId, KartId, ItemKind, u32)>,
    /// Oreillers géants after another kart (issue #61): which, after whom, the common tick.
    pub retargets: Vec<(ItemId, Option<KartId>, u32)>,
    /// What pies voleuses took (issue #21, the victim's word): the thief, the victim, the item and
    /// how many, the common tick (the victim's).
    pub steals: Vec<(KartId, KartId, ItemKind, u8, u32)>,
}

/// The items and croquettes online (see the module's documentation): inserted with
/// [`KartSync`], once seated in a room.
#[derive(Resource, Default)]
pub struct OnlineItems {
    session: Option<u8>,
    /// The record of who took what (the same on every peer); the host decides with it.
    pub arbiter: Arbiter,
    inbox: Vec<(PeerKey, Message)>,
    outbox: Vec<Message>,
    /// Our claims waiting for their verdict.
    mine: Vec<Mine>,
    /// The items out here that the network knows (ours and the others'): entity, id, its peer.
    objects: Vec<(Entity, ItemId, PeerKey)>,
    /// What the other peers' karts hold out, drawn here: entity, its kart.
    shown_held: Vec<(Entity, Entity)>,
    /// The scattered croquettes here: entity, id.
    loose: Vec<(Entity, LooseId)>,
    /// Items broken lately.
    broken: Vec<ItemId>,
    /// Items exploded lately (issue #18): the entity they had here, their id.
    exploded: Vec<(Entity, ItemId)>,
    /// The next number of each kart's items and losses: ours to come, the others' seen (a new
    /// host goes on numbering the bots' from there).
    item_seq: [u16; GRID_SLOTS],
    loss_seq: [u16; GRID_SLOTS],
    /// Peers gone since the last tick.
    left: Vec<PeerKey>,
    /// Our oreillers géants out, and the target we last told (issue #61).
    targets: Vec<(ItemId, Option<KartId>)>,
    /// The other karts' oreillers géants drawn, until the host's word: their kart, the tick of
    /// their claim, what they get instead if refused (issue #61, for `ItemLog::draws`).
    pillow_draws: Vec<(KartId, u32, (ItemKind, u8))>,
    pub log: ItemLog,
}

impl OnlineItems {
    /// A message for us from `from`: handled on the next tick.
    pub fn receive(&mut self, from: PeerKey, message: Message) {
        self.inbox.push((from, message));
    }

    /// The messages to send to every peer, oldest first.
    pub fn drain_outbox(&mut self) -> std::vec::Drain<'_, Message> {
        self.outbox.drain(..)
    }

    /// `peer` left the room.
    pub fn left(&mut self, peer: PeerKey) {
        self.left.push(peer);
    }

    /// The session (`KartSync::session`) we are in.
    pub fn session(&self) -> Option<u8> {
        self.session
    }

    /// The items out here that the network knows (ours and the others'), by id.
    pub fn objects(&self) -> impl Iterator<Item = (Entity, ItemId)> + '_ {
        self.objects.iter().map(|(entity, id, _)| (*entity, *id))
    }

    /// The scattered croquettes here, by id.
    pub fn loose(&self) -> impl Iterator<Item = (Entity, LooseId)> + '_ {
        self.loose.iter().copied()
    }

    /// What the other peers' karts hold out, drawn here: the item's entity, its kart's body.
    pub fn shown_held(&self) -> impl Iterator<Item = (Entity, Entity)> + '_ {
        self.shown_held.iter().copied()
    }

    /// What happened so far, in a line (the console: the same numbers on every screen).
    pub fn summary(&self) -> String {
        let grants = self.arbiter.grants();
        let boxes = grants
            .iter()
            .filter(|g| matches!(g.spot, Spot::Box(_)))
            .count();
        let hits = self
            .log
            .breaks
            .iter()
            .filter(|(_, _, landed, _)| *landed)
            .count();
        let bombes = self
            .log
            .thrown
            .iter()
            .filter(|(_, item, _)| *item == ItemKind::Bombe)
            .count();
        let pillows = grants
            .iter()
            .filter(|g| matches!(g.spot, Spot::Pillow(_)))
            .count();
        let croquettes = grants.len() - boxes - pillows;
        format!(
            "{boxes} boxes and {croquettes} croquettes taken, {pillows} oreillers granted, {} items out ({bombes} bombes), {} klaxons, {} effects, {} rams, {hits} hits, {} croquette losses",
            self.log.thrown.len(),
            self.log.honks.len(),
            self.log.effects.len(),
            self.log.rams.len(),
            self.log.scatters.len()
        )
    }

    /// A new session (free driving, a race): everything starts over.
    fn start(&mut self, session: u8) {
        *self = Self {
            session: Some(session),
            inbox: std::mem::take(&mut self.inbox),
            outbox: std::mem::take(&mut self.outbox),
            ..Self::default()
        };
    }

    fn broke(&mut self, id: ItemId) {
        if self.broken.len() >= BROKEN_KEPT {
            self.broken.remove(0);
        }
        self.broken.push(id);
    }

    /// The item of `entity` exploded (issue #18): gone, but the victims' decisions of its blast
    /// still name it.
    fn exploded(&mut self, entity: Entity, id: ItemId) {
        if !self.broken.contains(&id) {
            self.broke(id);
        }
        if self.exploded.len() >= EXPLODED_KEPT {
            self.exploded.remove(0);
        }
        self.exploded.push((entity, id));
    }

    /// The next number for an item of `kart` (never [`ItemId::HELD`]).
    fn next_item(&mut self, kart: KartId) -> u16 {
        let seq = &mut self.item_seq[usize::from(kart).min(GRID_SLOTS - 1)];
        let next = *seq;
        *seq = if next >= ItemId::HELD - 1 {
            0
        } else {
            next + 1
        };
        next
    }

    fn next_loss(&mut self, kart: KartId) -> u16 {
        let seq = &mut self.loss_seq[usize::from(kart).min(GRID_SLOTS - 1)];
        let next = *seq;
        *seq = seq.wrapping_add(1);
        next
    }
}

/// The online items' systems, in `FixedUpdate`.
pub struct OnlineItemsPlugin;

impl Plugin for OnlineItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (
                (receive_items, show_held_items)
                    .chain()
                    .after(KartSystems::Input)
                    .before(ItemSystems)
                    .before(CroquetteSystems),
                send_items
                    .after(ItemSystems)
                    .after(CroquetteSystems)
                    .before(KartSystems::Drive),
            )
                .run_if(resource_exists::<OnlineItems>)
                .run_if(resource_exists::<KartSync>)
                .run_if(resource_exists::<NetSession>)
                .run_if(resource_exists::<ItemTuning>)
                .run_if(resource_exists::<CroquetteTuning>),
        );
    }
}

/// The respawn time of a spot, in ticks (`None`: a scattered croquette, once; an oreiller géant
/// has its own rules, `pickups::PillowRules`).
fn respawn(spot: Spot, items: &ItemTuning, croquettes: &CroquetteTuning) -> Option<u32> {
    match spot {
        Spot::Box(_) => Some(seconds_to_ticks(items.box_respawn_time)),
        Spot::Croquette(_) => Some(seconds_to_ticks(croquettes.respawn_time)),
        Spot::Loose(_) | Spot::Pillow(_) => None,
    }
}

/// The oreiller géant's rules on the common clock (issue #61): those of `tuning`, from the race's
/// start on the room's clock (`None`: no race, never one).
fn pillow_rules(race: Option<(&OnlineRace, &Race)>, tuning: &ItemTuning) -> PillowRules {
    let Some((online, race)) = race.filter(|(online, _)| online.racing) else {
        return PillowRules::NONE;
    };
    // The flow's tick is `common - start_tick + 1` (`online::follow_room_clock`), the race's
    // ticks `flow - before_go` (`Race::race_ticks`).
    let base = (online.shared.start.start_tick + race.timings.before_go()).saturating_sub(1);
    PillowRules {
        race_base: Some(base),
        min_race_ticks: seconds_to_ticks(tuning.oreiller_min_race_time),
        cooldown_ticks: seconds_to_ticks(tuning.oreiller_cooldown),
    }
}

/// The prizes of a box a kart drew, as its peer draws them (issue #61, `items::take_boxes`): in a
/// race, the first oreiller géant only (the next draws of the box go without it); off a race
/// (session 0), never one. With an oreiller, what it gets instead if the host refuses it.
fn box_prizes(
    table: &ItemTable,
    (seed, slot): (u64, u64),
    (session, draw, draws, gap): (u8, u32, u8, f32),
) -> (Vec<(ItemKind, u8)>, Option<(ItemKind, u8)>) {
    let mut instead = None;
    let drawn = (0..u32::from(draws))
        .filter_map(|k| {
            let unit = draw_unit(seed, slot, draw + k);
            let allowed = session != 0 && instead.is_none();
            let prize = table.draw_with_pillow(gap, unit, allowed);
            if prize == Prize::OreillerGeant {
                instead = table
                    .row(gap)
                    .pick_without(unit, Prize::OreillerGeant)
                    .item();
            }
            prize.item()
        })
        .collect();
    (drawn, instead)
}

/// The karts of this game by id: ours (`NetKart`), and the others' bodies.
struct Karts<'a, 'w, 's> {
    local: &'a Query<'w, 's, (Entity, &'static NetKart), With<Kart>>,
    sync: &'a KartSync,
}

impl Karts<'_, '_, '_> {
    fn ours(&self, kart: KartId) -> Option<Entity> {
        self.local
            .iter()
            .find(|(_, net)| net.0 == kart)
            .map(|(entity, _)| entity)
    }

    fn entity(&self, kart: KartId) -> Option<Entity> {
        self.ours(kart)
            .or_else(|| self.sync.remote_kart(kart).and_then(|remote| remote.proxy))
    }

    /// The peer that has authority over `kart`: us for ours, its peer for the others'.
    fn authority(&self, kart: KartId) -> Option<PeerKey> {
        if self.ours(kart).is_some() {
            Some(self.sync.me())
        } else {
            self.sync.remote_kart(kart).map(|remote| remote.peer)
        }
    }
}

/// Before the item and croquette rules: what the other peers told (claims, verdicts, items out,
/// breaks, scattered croquettes; issue #61: effects, rams, oreillers retargeted or gone), and the
/// host's verdicts.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn receive_items(
    mut commands: Commands,
    mut items: ResMut<OnlineItems>,
    sync: Res<KartSync>,
    session: Res<NetSession>,
    time: Res<Time<Fixed>>,
    (item_tuning, croquette_tuning, table, grid): (
        Res<ItemTuning>,
        Res<CroquetteTuning>,
        Res<OnlineItemTable>,
        Res<RaceGrid>,
    ),
    (online_race, race): (Option<Res<OnlineRace>>, Option<Res<Race>>),
    local: Query<(Entity, &'static NetKart), With<Kart>>,
    mut ours: Query<(&mut ItemSlots, &mut ItemStats, Option<&mut Croquettes>), With<Kart>>,
    mut spots: (
        Query<(&SpotIndex, &mut ItemBox)>,
        Query<(&SpotIndex, &mut TrackCroquette)>,
    ),
    mut objects: Query<(Entity, &mut ItemObject)>,
    mut inbox: Local<Vec<(PeerKey, Message)>>,
    mut verdicts: Local<Vec<Verdict>>,
    (mut blasts, mut honks, mut effects, mut steals): (
        MessageWriter<Blast>,
        MessageWriter<Honked>,
        MessageWriter<ItemEffect>,
        MessageWriter<ItemSteal>,
    ),
) {
    let Some(lobby) = session.lobby.as_ref() else {
        return;
    };
    let (Some(now), dt) = (
        sync.clock.common(local_tick(&time)),
        time.timestep().as_secs_f32(),
    ) else {
        return;
    };
    let items = &mut *items;
    let current = sync.session();
    if items.session != Some(current) {
        items.start(current);
    }
    let karts = Karts {
        local: &local,
        sync: &sync,
    };
    // Who left: its claims are dropped (and the oreillers its karts held), its items out
    // disappear.
    for peer in std::mem::take(&mut items.left) {
        items.arbiter.forget(peer);
        items.objects.retain(|(entity, _, from)| {
            if *from == peer {
                commands.entity(*entity).try_despawn();
            }
            *from != peer
        });
    }
    // Issue #61: the oreillers géants whose flight ended here (exploded, destroyed, gone with
    // their thrower): another one may come.
    let ended: Vec<ItemId> = items
        .arbiter
        .pillows()
        .iter()
        .filter_map(|pillow| pillow.thrown)
        .filter(|id| !items.objects.iter().any(|(_, out, _)| out == id))
        .collect();
    for id in ended {
        items.arbiter.pillow_ended(id);
        items.targets.retain(|(out, _)| *out != id);
    }
    let respawn = |spot: Spot| respawn(spot, &item_tuning, &croquette_tuning);
    inbox.clear();
    inbox.append(&mut items.inbox);
    for (from, message) in inbox.drain(..) {
        match message {
            Message::Claim(claim) if claim.session == current => {
                if karts.authority(claim.kart) != Some(from) {
                    continue;
                }
                // Decided already: kept as it was (a claim reaches the host once).
                items.arbiter.claim(from, claim);
            }
            Message::Verdict(verdict) if verdict.claim.session == current => {
                // Issue #63: the host's word only.
                if Some(from) != lobby.host() || lobby.is_host() {
                    continue;
                }
                if items.arbiter.record(&verdict) {
                    apply_verdict(
                        items,
                        &verdict,
                        now,
                        &respawn,
                        &karts,
                        (&table.0, grid.seed),
                        &mut commands,
                        &mut ours,
                        &mut spots,
                        &objects,
                    );
                }
            }
            Message::ItemOut(out) if out.session == current => {
                if karts.authority(out.id.owner) != Some(from)
                    || items.objects.iter().any(|(_, id, _)| *id == out.id)
                    || items.log.thrown.iter().any(|(id, ..)| *id == out.id)
                {
                    continue;
                }
                let seq = &mut items.item_seq[usize::from(out.id.owner)];
                *seq = (*seq)
                    .max(out.id.seq.saturating_add(1))
                    .min(ItemId::HELD - 1);
                items.log.thrown.push((out.id, out.item, out.tick));
                if out.item == ItemKind::OreillerGeant {
                    // Issue #61: it flies (one at a time), until its flight ends here.
                    items.arbiter.pillow_thrown(out.id.owner, out.id);
                }
                // Broken already (its victim told us first: a third peer): nothing to show.
                if items.broken.contains(&out.id) {
                    continue;
                }
                let object = ItemObject {
                    age: dt,
                    ..ItemObject::new(
                        out.item,
                        karts.entity(out.id.owner).unwrap_or(Entity::PLACEHOLDER),
                        motion_of(out.motion, &karts),
                        Vec3::from_array(out.position),
                        Vec3::from_array(out.velocity),
                    )
                };
                let entity = commands
                    .spawn((
                        Name::new(out.item.label()),
                        Transform::from_translation(object.position),
                        object,
                        CatchUp(
                            now.saturating_sub(out.tick)
                                .saturating_sub(1)
                                .min(MAX_CATCH_UP),
                        ),
                    ))
                    .id();
                items.objects.push((entity, out.id, from));
            }
            Message::ItemBreak(end) if end.session == current => {
                // The victim's word for its kart; anyone's for two items.
                if end
                    .kart
                    .is_some_and(|kart| karts.authority(kart) != Some(from))
                {
                    continue;
                }
                apply_break(items, &end, now, &karts, &mut commands, &mut ours, &objects);
            }
            Message::Blast(blast) if blast.session == current => {
                // Issue #18: a bombe exploded on what touched it, on another screen: here too,
                // where it says (each peer judges its own karts in the blast). Issue #61: an
                // oreiller géant too.
                if items.broken.contains(&blast.id) {
                    continue;
                }
                items.log.blasts.push((blast.id, blast.tick));
                let Some(at) = items.objects.iter().position(|(_, id, _)| *id == blast.id) else {
                    // Not here (yet): a late `ItemOut` is dropped.
                    items.broke(blast.id);
                    continue;
                };
                let (entity, id, _) = items.objects.swap_remove(at);
                items.exploded(entity, id);
                let item = objects
                    .get(entity)
                    .map_or(ItemKind::Bombe, |(_, object)| object.item);
                commands.entity(entity).try_despawn();
                blasts.write(Blast {
                    at: Vec3::from_array(blast.position),
                    radius: blast_radius(item, &item_tuning),
                    item,
                    owner: karts.entity(blast.id.owner).unwrap_or(Entity::PLACEHOLDER),
                    object: Some(entity),
                    announce: false,
                });
            }
            Message::Honk(honk) if honk.session == current => {
                // Issue #18: its peer judged the items it destroyed; here, its shock wave.
                if karts.authority(honk.kart) != Some(from) {
                    continue;
                }
                items.log.honks.push((honk.kart, honk.tick));
                if let Some(kart) = karts.entity(honk.kart) {
                    honks.write(Honked {
                        kart,
                        at: Vec3::from_array(honk.position),
                    });
                }
            }
            Message::Effect(effect) if effect.session == current => {
                // Issue #61: a chantilly or a dégonflage of another peer's kart: on our karts,
                // with its user's ranking (`items::apply_item_effects`: each victim decides).
                if karts.authority(effect.kart) != Some(from) {
                    continue;
                }
                items
                    .log
                    .effects
                    .push((effect.kart, effect.item, effect.tick));
                let entity = |kart: KartId| karts.entity(kart).unwrap_or(Entity::PLACEHOLDER);
                let rank = effect
                    .order
                    .iter()
                    .position(|kart| *kart == effect.kart)
                    .map_or(0, |i| i as u32 + 1);
                let [x, z] = effect.forward;
                effects.write(ItemEffect {
                    by: entity(effect.kart),
                    item: effect.item,
                    at: Vec3::from_array(effect.position),
                    forward: Vec3::new(x, 0.0, z),
                    rank,
                    standings: Some(SharedStandings::new(
                        effect.order.iter().map(|kart| entity(*kart)),
                    )),
                });
            }
            Message::Rammed(rammed) if rammed.session == current => {
                // Issue #61: the victim's word: its rammer counts its hit.
                if karts.authority(rammed.kart) != Some(from)
                    || items.log.rams.iter().any(|(kart, by, _, tick)| {
                        *kart == rammed.kart && *by == rammed.by && *tick == rammed.tick
                    })
                {
                    continue;
                }
                items
                    .log
                    .rams
                    .push((rammed.kart, rammed.by, rammed.item, rammed.tick));
                if let Some(rammer) = karts.ours(rammed.by)
                    && let Ok((_, mut stats, _)) = ours.get_mut(rammer)
                {
                    stats.hits_landed += 1;
                }
            }
            Message::PillowGone(gone) if gone.session == current => {
                // Issue #61: its kart's peer says so: another oreiller may come.
                if karts.authority(gone.kart) != Some(from) {
                    continue;
                }
                items.arbiter.pillow_gone(ClaimKey {
                    spot: Spot::Pillow(gone.index),
                    kart: gone.kart,
                    tick: gone.tick,
                });
            }
            Message::Retarget(retarget) if retarget.session == current => {
                // Issue #61: its thrower's peer sends it after its leader.
                if karts.authority(retarget.id.owner) != Some(from) {
                    continue;
                }
                items
                    .log
                    .retargets
                    .push((retarget.id, retarget.target, retarget.tick));
                let target = retarget.target.and_then(|kart| karts.entity(kart));
                if let Some((entity, ..)) =
                    items.objects.iter().find(|(_, id, _)| *id == retarget.id)
                    && let Ok((_, mut object)) = objects.get_mut(*entity)
                    && let Motion::Pillow { flight, .. } = object.motion
                {
                    object.motion = Motion::Pillow { target, flight };
                }
            }
            Message::Steal(steal) if steal.session == current => {
                // Issue #21: a pie voleuse of another peer's kart, after one of ours: our kart
                // gives up what it holds now (`items::apply_steals`), and we tell (`send_items`).
                if karts.authority(steal.thief) != Some(from) {
                    continue;
                }
                if let (Some(victim), Some(thief)) =
                    (karts.ours(steal.victim), karts.entity(steal.thief))
                {
                    steals.write(ItemSteal { thief, victim });
                }
            }
            Message::Stolen(stolen) if stolen.session == current => {
                // Issue #21: the victim's word: the thief gets it, ready, if it is ours.
                if karts.authority(stolen.victim) != Some(from) {
                    continue;
                }
                items.log.steals.push((
                    stolen.thief,
                    stolen.victim,
                    stolen.item,
                    stolen.count,
                    stolen.tick,
                ));
                if let Some(thief) = karts.ours(stolen.thief)
                    && let Ok((mut slots, ..)) = ours.get_mut(thief)
                {
                    slots.give(stolen.item, stolen.count, 0);
                }
            }
            Message::Scatter(scatter) if scatter.session == current => {
                if karts.authority(scatter.kart) != Some(from) {
                    continue;
                }
                let seq = &mut items.loss_seq[usize::from(scatter.kart)];
                *seq = (*seq).max(scatter.loss.wrapping_add(1));
                let dropped_by = karts.entity(scatter.kart).unwrap_or(Entity::PLACEHOLDER);
                // Their age at the end of this tick, as on the victim's screen (they age once
                // more in this tick's croquette rules).
                let age = now.saturating_sub(scatter.tick).saturating_sub(1) as f32 * dt;
                for (index, to) in scatter.to.iter().enumerate() {
                    let loose = LooseCroquette {
                        age,
                        ..LooseCroquette::new(
                            Vec3::from_array(scatter.from),
                            Vec3::from_array(*to),
                            dropped_by,
                        )
                    };
                    let entity = commands
                        .spawn((
                            Name::new("Scattered croquette"),
                            Transform::from_translation(loose.position(&croquette_tuning)),
                            loose,
                        ))
                        .id();
                    let id = LooseId {
                        kart: scatter.kart,
                        loss: scatter.loss,
                        index: index as u8,
                    };
                    items.loose.push((entity, id));
                }
                items
                    .log
                    .scatters
                    .push((scatter.kart, scatter.loss, scatter.tick, scatter.to));
            }
            _ => {}
        }
    }
    // The host decides (issue #61: the oreillers géants on the race's clock).
    if lobby.is_host() {
        let rules = pillow_rules(online_race.as_deref().zip(race.as_deref()), &item_tuning);
        verdicts.clear();
        items.arbiter.decide(respawn, &rules, &mut verdicts);
        for verdict in verdicts.drain(..) {
            items.outbox.push(Message::Verdict(verdict));
            apply_verdict(
                items,
                &verdict,
                now,
                &respawn,
                &karts,
                (&table.0, grid.seed),
                &mut commands,
                &mut ours,
                &mut spots,
                &objects,
            );
        }
    }
}

/// An item's motion with the karts by id, as this game's entities.
fn motion_of(motion: OutMotion, karts: &Karts) -> Motion {
    let target = |kart: Option<KartId>| kart.and_then(|kart| karts.entity(kart));
    match motion {
        OutMotion::Ground => Motion::Ground,
        OutMotion::Rolling { bounces_left, fall } => Motion::Rolling {
            bounces_left: u32::from(bounces_left),
            fall,
        },
        OutMotion::Track {
            target: kart,
            distance,
            lateral,
        } => Motion::Homing {
            target: target(kart),
            flight: Flight::Track { distance, lateral },
        },
        OutMotion::Dive { target: kart } => Motion::Homing {
            target: target(kart),
            flight: Flight::Dive,
        },
        OutMotion::Bomb {
            vertical,
            grounded,
            impact,
        } => Motion::Bomb {
            vertical,
            grounded,
            impact,
        },
        OutMotion::Pillow {
            target: kart,
            flight,
        } => Motion::Pillow {
            target: target(kart),
            flight: match flight {
                OutPillow::Track { distance, lateral } => PillowFlight::Track { distance, lateral },
                OutPillow::Hover { left } if left >= OutPillow::WAITING => PillowFlight::Hover {
                    left: f32::INFINITY,
                },
                OutPillow::Hover { left } => PillowFlight::Hover { left },
                OutPillow::Drop => PillowFlight::Drop,
            },
        },
        OutMotion::Boomerang {
            returning,
            travelled,
            bounces_left,
            fall,
        } => Motion::Boomerang {
            returning,
            travelled,
            bounces_left: u32::from(bounces_left),
            fall,
        },
    }
}

/// The host decided a claim: ours refused, its roulettes or its croquette taken back (issue #61:
/// an oreiller géant refused, the other draw instead); granted, the box or the croquette gone here
/// too, back on the same tick as everywhere.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_verdict(
    items: &mut OnlineItems,
    verdict: &Verdict,
    now: u32,
    respawn: &impl Fn(Spot) -> Option<u32>,
    karts: &Karts,
    (table, seed): (&ItemTable, u64),
    commands: &mut Commands,
    ours: &mut Query<(&mut ItemSlots, &mut ItemStats, Option<&mut Croquettes>), With<Kart>>,
    (boxes, croquettes): &mut (
        Query<(&SpotIndex, &mut ItemBox)>,
        Query<(&SpotIndex, &mut TrackCroquette)>,
    ),
    objects: &Query<(Entity, &mut ItemObject)>,
) {
    let claim = verdict.claim;
    let key = ClaimKey::of(&claim);
    // A race: a kart's grid slot is its id (its draws).
    let slot = if claim.session == 0 {
        0
    } else {
        u64::from(claim.kart)
    };
    if let Some(index) = items
        .mine
        .iter()
        .position(|m| ClaimKey::of(&m.claim) == key)
    {
        let mine = items.mine.swap_remove(index);
        if !verdict.granted {
            info!(
                "network: {:?} went to another kart first: kart {} gives it back",
                key.spot, key.kart
            );
            items.log.refused.push(key);
            if let Ok((mut slots, _, held)) = ours.get_mut(mine.kart) {
                match mine.undo {
                    Undo::Box(filled) => slots.revoke(filled),
                    Undo::Croquettes(gained) => {
                        if let Some(mut held) = held {
                            held.lose(gained);
                        }
                    }
                    Undo::Pillow(instead) => {
                        let swapped = instead
                            .item()
                            .is_some_and(|(item, count)| slots.swap_pillow(item, count));
                        if !swapped {
                            // Thrown before the host said no (a very late verdict): it breaks
                            // on every screen.
                            let thrown = items.objects.iter().position(|(entity, id, _)| {
                                id.owner == claim.kart
                                    && objects.get(*entity).is_ok_and(|(_, object)| {
                                        object.item == ItemKind::OreillerGeant
                                    })
                            });
                            if let Some(at) = thrown {
                                let (entity, id, _) = items.objects.swap_remove(at);
                                commands.entity(entity).try_despawn();
                                items.broke(id);
                                let end = ItemBreak {
                                    session: claim.session,
                                    id,
                                    kart: None,
                                    landed: false,
                                };
                                items.log.breaks.push((end.id, end.kart, end.landed, now));
                                items.outbox.push(Message::ItemBreak(end));
                            }
                        }
                    }
                }
            }
        }
    }
    // Issue #61: another kart's oreiller géant decided: refused, it drew the other prize instead,
    // as on its peer's screen.
    if let Pickup::Pillow { .. } = claim.pickup
        && let Some(at) = items
            .pillow_draws
            .iter()
            .position(|(kart, tick, _)| *kart == claim.kart && *tick == claim.tick)
    {
        let (.., instead) = items.pillow_draws.swap_remove(at);
        if !verdict.granted
            && let Some((.., drawn)) = items
                .log
                .draws
                .iter_mut()
                .find(|(kart, tick, _)| *kart == claim.kart && *tick == claim.tick)
            && let Some(pillow) = drawn
                .iter_mut()
                .find(|(item, _)| *item == ItemKind::OreillerGeant)
        {
            *pillow = instead;
        }
    }
    if !verdict.granted {
        return;
    }
    let seconds_left = |spot: Spot| {
        let back = claim.tick.saturating_add(respawn(spot).unwrap_or(0));
        back.saturating_sub(now) as f32 / crate::TICK_HZ as f32
    };
    match claim.pickup {
        Pickup::Box {
            index,
            draw,
            draws,
            gap,
        } => {
            let left = seconds_left(Spot::Box(index));
            if let Some((_, mut item_box)) = boxes.iter_mut().find(|(spot, _)| spot.0 == index)
                && left > 0.0
            {
                item_box.respawn_in = left;
            }
            if karts.ours(claim.kart).is_none() {
                // The same draws as its peer (an oreiller géant until the host says otherwise).
                let (drawn, instead) = box_prizes(
                    table,
                    (seed, slot),
                    (claim.session, u32::from(draw), draws, gap),
                );
                items.log.draws.push((claim.kart, claim.tick, drawn));
                if let Some(instead) = instead {
                    if items.pillow_draws.len() >= PILLOW_DRAWS_KEPT {
                        items.pillow_draws.remove(0);
                    }
                    items.pillow_draws.push((claim.kart, claim.tick, instead));
                }
            }
        }
        Pickup::Croquette { index } => {
            let left = seconds_left(Spot::Croquette(index));
            if let Some((_, mut croquette)) =
                croquettes.iter_mut().find(|(spot, _)| spot.0 == index)
                && left > 0.0
            {
                croquette.respawn_in = left;
            }
        }
        Pickup::Loose(id) => {
            items.loose.retain(|(entity, loose)| {
                if *loose == id {
                    commands.entity(*entity).try_despawn();
                }
                *loose != id
            });
        }
        // Issue #61: the record has it (`Arbiter::record`): one in the race.
        Pickup::Pillow { .. } => {}
    }
}

/// An item broke on another peer's screen: gone here too. Held out by one of our karts: one of
/// our shields broke. A hit of one of our items: counted.
fn apply_break(
    items: &mut OnlineItems,
    end: &ItemBreak,
    now: u32,
    karts: &Karts,
    commands: &mut Commands,
    ours: &mut Query<(&mut ItemSlots, &mut ItemStats, Option<&mut Croquettes>), With<Kart>>,
    objects: &Query<(Entity, &mut ItemObject)>,
) {
    let owner = karts.ours(end.id.owner);
    if end.id.is_held() {
        // One of what our kart holds out broke on another peer's kart (issue #63: another peer's
        // kart's, seen by a third one: its states say it, only the story counts it, below).
        let Some(owner) = owner else {
            items.log.breaks.push((end.id, end.kart, end.landed, now));
            return;
        };
        let mut pinata = false;
        if let Ok((mut slots, ..)) = ours.get_mut(owner) {
            // Issue #21: one of a piñata's (its first one left: its item goes with it,
            // `items::move_items`).
            pinata = slots.pinata != 0;
            slots.lose_one_held_out();
        }
        let leaving = objects
            .iter()
            .filter(|(_, o)| o.owner == owner)
            .filter_map(|(entity, o)| o.held().map(|hold| (entity, hold)))
            .max_by_key(|(_, hold)| match hold {
                Hold::Trail(k) | Hold::Orbit(k) | Hold::Pinata(k) => *k,
                Hold::Dragged => 0,
            });
        if let Some((entity, _)) = leaving.filter(|_| !pinata) {
            commands.entity(entity).try_despawn();
        }
    } else {
        if !items.broken.contains(&end.id) {
            items.broke(end.id);
        }
        items.objects.retain(|(entity, id, _)| {
            if *id == end.id {
                commands.entity(*entity).try_despawn();
            }
            *id != end.id
        });
    }
    // Told twice (two peers judged the same item), it counts once. On two karts (it touched one
    // on each screen: each victim decided), both count. What a kart holds out has no number of its
    // own (`ItemId::held`): each break of one is another item (issue #61: seen with every item in
    // the boxes, two shields of a kart broken on the same kart, the second one was not counted).
    if !end.id.is_held()
        && items
            .log
            .breaks
            .iter()
            .any(|(id, kart, ..)| *id == end.id && *kart == end.kart)
    {
        return;
    }
    if end.landed
        && let Some(owner) = owner
        && let Ok((_, mut stats, _)) = ours.get_mut(owner)
    {
        stats.hits_landed += 1;
    }
    items.log.breaks.push((end.id, end.kart, end.landed, now));
}

/// What the other peers' karts hold out, around their bodies here (their states say it, their
/// slots on our screen follow: `sync::show_remote_karts`): drawn again when it changes. They
/// follow the body (`items::move_items`); only their holder's peer judges them.
#[allow(clippy::type_complexity)]
fn show_held_items(
    mut commands: Commands,
    mut items: ResMut<OnlineItems>,
    tuning: Res<ItemTuning>,
    karts: Query<(Entity, &ItemSlots, &Transform), (With<RemoteKartProxy>, Changed<ItemSlots>)>,
    alive: Query<(), With<ItemObject>>,
) {
    let items = &mut *items;
    items
        .shown_held
        .retain(|(entity, _)| alive.contains(*entity));
    for (kart, slots, transform) in &karts {
        items.shown_held.retain(|(entity, owner)| {
            if *owner == kart {
                commands.entity(*entity).try_despawn();
            }
            *owner != kart
        });
        let Some((item, count)) = slots.held_out() else {
            continue;
        };
        let holds = if slots.dragging.is_some() { 1 } else { count };
        let mut shown = |item: ItemKind, hold: Hold| {
            let radius = item_radius(item, &tuning);
            let at = transform.transform_point(held_offset(hold, 3, radius, 0.0, &tuning));
            let entity = commands
                .spawn((
                    Name::new(item.label()),
                    Transform::from_translation(at),
                    ItemObject::new(item, kart, Motion::Held(hold), at, Vec3::ZERO),
                ))
                .id();
            items.shown_held.push((entity, kart));
        };
        if item == ItemKind::Pinata {
            // Issue #21: the piñata's items left, each as itself.
            for (k, held) in PINATA_ITEMS.iter().enumerate() {
                if slots.has_pinata(k) {
                    shown(*held, Hold::Pinata(k as u8));
                }
            }
            continue;
        }
        for k in 0..holds {
            let hold = match (slots.dragging.is_some(), item) {
                (true, _) => Hold::Dragged,
                (false, ItemKind::Pelure) => Hold::Trail(k),
                (false, _) => Hold::Orbit(k),
            };
            shown(item, hold);
        }
    }
}

/// After the item and croquette rules: what our karts did goes to every peer: their claims on
/// boxes and croquettes (issue #61: and on the oreillers géants they drew), the items they threw
/// or dropped, the items we judged broken (on our karts, on our shields), the croquettes they
/// lost; issue #61: their chantilly and dégonflages, the karts that knocked ours over, the new
/// targets of their oreillers, their oreillers gone without a throw.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn send_items(
    mut items: ResMut<OnlineItems>,
    sync: Res<KartSync>,
    time: Res<Time<Fixed>>,
    (local, slots): (
        Query<&NetKart, With<Kart>>,
        Query<(&NetKart, &ItemSlots), With<Kart>>,
    ),
    proxies: Query<&RemoteKartProxy>,
    spots: Query<&SpotIndex>,
    (mut boxes, mut taken, mut judged, mut blocked, mut lost): (
        MessageReader<BoxBroken>,
        MessageReader<CroquetteTaken>,
        MessageReader<ItemHitJudged>,
        MessageReader<ItemBlocked>,
        MessageReader<CroquettesLost>,
    ),
    (mut blasts, mut honks, mut effects, mut steals, mut stolen): (
        MessageReader<Blast>,
        MessageReader<Honked>,
        MessageReader<ItemEffect>,
        MessageReader<ItemSteal>,
        MessageReader<ItemStolen>,
    ),
    new_objects: Query<(Entity, &ItemObject), Added<ItemObject>>,
    new_loose: Query<(Entity, &LooseCroquette), Added<LooseCroquette>>,
    (alive_objects, alive_loose): (Query<&ItemObject>, Query<(), With<LooseCroquette>>),
) {
    let items = &mut *items;
    let session = sync.session();
    let Some(now) = sync.clock.common(local_tick(&time)) else {
        return;
    };
    if items.session != Some(session) {
        // Not started yet (the next tick does).
        boxes.clear();
        taken.clear();
        judged.clear();
        blocked.clear();
        lost.clear();
        blasts.clear();
        honks.clear();
        effects.clear();
        steals.clear();
        stolen.clear();
        return;
    }
    let me = sync.me();
    let id_of = |entity: Entity| -> Option<KartId> {
        local
            .get(entity)
            .map(|net| net.0)
            .or_else(|_| proxies.get(entity).map(|proxy| proxy.kart))
            .ok()
    };
    let claim = |items: &mut OnlineItems, kart: Entity, pickup: Pickup, undo: Undo| {
        let Ok(net) = local.get(kart) else {
            return;
        };
        let claim = Claim {
            session,
            pickup,
            kart: net.0,
            tick: now,
        };
        items.arbiter.claim(me, claim);
        items.mine.push(Mine { claim, kart, undo });
        items.outbox.push(Message::Claim(claim));
    };
    // Boxes our karts broke.
    for broken in boxes.read() {
        let Ok(index) = spots.get(broken.item_box) else {
            continue;
        };
        let pickup = Pickup::Box {
            index: index.0,
            draw: broken.draw.min(u32::from(u16::MAX)) as u16,
            draws: broken.draws,
            gap: broken.gap,
        };
        claim(items, broken.kart, pickup, Undo::Box(broken.filled));
        // Issue #61: an oreiller géant drawn from it: the host's rules decide.
        if let Some(pillow) = broken.pillow {
            let pickup = Pickup::Pillow {
                index: index.0,
                draw: pillow.draw.min(u32::from(u16::MAX)) as u16,
            };
            claim(items, broken.kart, pickup, Undo::Pillow(pillow.instead));
        }
    }
    // Croquettes our karts picked up.
    for pick in taken.read() {
        let pickup = match spots.get(pick.croquette) {
            Ok(index) => Pickup::Croquette { index: index.0 },
            Err(_) => {
                let Some(at) = items.loose.iter().position(|(e, _)| *e == pick.croquette) else {
                    continue;
                };
                Pickup::Loose(items.loose.swap_remove(at).1)
            }
        };
        claim(items, pick.kart, pickup, Undo::Croquettes(pick.gained));
    }
    // Explosions (issue #18): the bombes are gone (their victims' decisions still name them);
    // one that a kart or a shield of ours set off, we tell where.
    for blast in blasts.read() {
        let Some(object) = blast.object else {
            continue;
        };
        let Some(at) = items.objects.iter().position(|(e, ..)| *e == object) else {
            continue;
        };
        let (entity, id, _) = items.objects.swap_remove(at);
        items.exploded(entity, id);
        if blast.announce {
            items.log.blasts.push((id, now));
            items.outbox.push(Message::Blast(BlastOut {
                session,
                id,
                tick: now,
                position: blast.at.to_array(),
            }));
        }
    }
    // Our klaxons (issue #18): the others see the shock wave.
    for honk in honks.read() {
        let Ok(net) = local.get(honk.kart) else {
            continue;
        };
        items.log.honks.push((net.0, now));
        items.outbox.push(Message::Honk(Honk {
            session,
            kart: net.0,
            tick: now,
            position: honk.at.to_array(),
        }));
    }
    // Our chantilly and dégonflages (issue #61): with the ranking we saw, for every peer.
    for effect in effects.read() {
        let Ok(net) = local.get(effect.by) else {
            continue;
        };
        if !EFFECT_ITEMS.contains(&effect.item) {
            continue;
        }
        let order = effect.standings.map_or_else(Vec::new, |standings| {
            standings
                .karts()
                .iter()
                .filter_map(|kart| id_of(*kart))
                .collect()
        });
        let forward = effect.forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
        items.log.effects.push((net.0, effect.item, now));
        items.outbox.push(Message::Effect(EffectOut {
            session,
            kart: net.0,
            item: effect.item,
            tick: now,
            position: effect.at.to_array(),
            forward: [forward.x, forward.z],
            order,
        }));
    }
    // Issue #21: our pies voleuses after another peer's kart: its peer gives up what it holds.
    for steal in steals.read() {
        let (Ok(thief), None, Some(victim)) = (
            local.get(steal.thief),
            local.get(steal.victim).ok(),
            id_of(steal.victim),
        ) else {
            continue;
        };
        items.outbox.push(Message::Steal(Steal {
            session,
            thief: thief.0,
            victim,
            tick: now,
        }));
    }
    // What our karts gave up to a pie voleuse (ours or another peer's): we tell.
    for theft in stolen.read() {
        let (Ok(victim), Some(thief)) = (local.get(theft.victim), id_of(theft.thief)) else {
            continue;
        };
        items
            .log
            .steals
            .push((thief, victim.0, theft.item, theft.count, now));
        items.outbox.push(Message::Stolen(Stolen {
            session,
            thief,
            victim: victim.0,
            item: theft.item,
            count: theft.count,
            tick: now,
        }));
    }
    // Items our karts threw or dropped (what they hold out goes with their states). Only those
    // that play online (issue #18).
    for (entity, object) in &new_objects {
        let Ok(net) = local.get(object.owner) else {
            continue;
        };
        if object.held().is_some()
            || !OUT_ITEMS.contains(&object.item)
            || items.objects.iter().any(|(e, ..)| *e == entity)
        {
            continue;
        }
        let target = |target: Option<Entity>| target.and_then(id_of);
        let motion = match object.motion {
            Motion::Ground | Motion::Held(_) => OutMotion::Ground,
            Motion::Pillow {
                target: kart,
                flight,
            } => OutMotion::Pillow {
                target: target(kart),
                flight: match flight {
                    PillowFlight::Track { distance, lateral } => {
                        OutPillow::Track { distance, lateral }
                    }
                    PillowFlight::Hover { left } if left.is_finite() => OutPillow::Hover { left },
                    PillowFlight::Hover { .. } => OutPillow::Hover {
                        left: OutPillow::WAITING,
                    },
                    PillowFlight::Drop => OutPillow::Drop,
                },
            },
            Motion::Bomb {
                vertical,
                grounded,
                impact,
            } => OutMotion::Bomb {
                vertical,
                grounded,
                impact,
            },
            Motion::Rolling { bounces_left, fall } => OutMotion::Rolling {
                bounces_left: bounces_left.min(255) as u8,
                fall,
            },
            Motion::Homing {
                target: kart,
                flight: Flight::Track { distance, lateral },
            } => OutMotion::Track {
                target: target(kart),
                distance,
                lateral,
            },
            Motion::Homing {
                target: kart,
                flight: Flight::Dive,
            } => OutMotion::Dive {
                target: target(kart),
            },
            Motion::Boomerang {
                returning,
                travelled,
                bounces_left,
                fall,
            } => OutMotion::Boomerang {
                returning,
                travelled,
                bounces_left: bounces_left.min(255) as u8,
                fall,
            },
        };
        let id = ItemId {
            owner: net.0,
            seq: items.next_item(net.0),
        };
        items.objects.push((entity, id, me));
        items.log.thrown.push((id, object.item, now));
        if let OutMotion::Pillow { target, .. } = motion {
            // Issue #61: it flies (one at a time); its target follows our leader.
            items.arbiter.pillow_thrown(net.0, id);
            items.targets.push((id, target));
        }
        items.outbox.push(Message::ItemOut(ItemOut {
            session,
            id,
            item: object.item,
            tick: now,
            position: object.position.to_array(),
            velocity: object.velocity.to_array(),
            motion,
        }));
    }
    // Issue #61: our oreillers géants after another kart (our leader changed).
    for at in 0..items.targets.len() {
        let (id, told) = items.targets[at];
        let Some((entity, ..)) = items.objects.iter().find(|(_, out, _)| *out == id) else {
            continue;
        };
        let Ok(&ItemObject {
            motion: Motion::Pillow { target, .. },
            ..
        }) = alive_objects.get(*entity)
        else {
            continue;
        };
        let target = target.and_then(id_of);
        if target != told {
            items.targets[at].1 = target;
            items.log.retargets.push((id, target, now));
            items.outbox.push(Message::Retarget(Retarget {
                session,
                id,
                target,
                tick: now,
            }));
        }
    }
    // Issue #61: an oreiller géant the host gave one of our karts, gone without a throw (a
    // dégonflage made it fall): we tell, another one may come.
    let gone: Vec<Claim> = items
        .arbiter
        .pillows()
        .iter()
        .filter(|pillow| pillow.thrown.is_none())
        .filter_map(|pillow| pillow.claim)
        .filter(|claim| {
            slots
                .iter()
                .any(|(net, held)| net.0 == claim.kart && !held.holds(ItemKind::OreillerGeant))
        })
        .collect();
    for claim in gone {
        let Pickup::Pillow { index, .. } = claim.pickup else {
            continue;
        };
        items.arbiter.pillow_gone(ClaimKey::of(&claim));
        items.outbox.push(Message::PillowGone(PillowGone {
            session,
            kart: claim.kart,
            index,
            tick: claim.tick,
        }));
    }
    // Items that touched our karts: we decided. Issue #61: a kart in a super câlin or a fusée
    // ran into one of ours (no item: `items::ram_karts`): knocked over, we tell (an immune kart
    // says nothing: its contacts come every tick).
    for hit in judged.read() {
        let Some(kart) = id_of(hit.kart) else {
            continue;
        };
        let Some(object) = hit.object else {
            if hit.landed
                && RAM_ITEMS.contains(&hit.item)
                && local.contains(hit.kart)
                && let Some(by) = id_of(hit.by)
                && by != kart
            {
                items.log.rams.push((kart, by, hit.item, now));
                items.outbox.push(Message::Rammed(Rammed {
                    session,
                    kart,
                    by,
                    item: hit.item,
                    tick: now,
                }));
            }
            continue;
        };
        let id = if let Some(at) = items.objects.iter().position(|(e, ..)| *e == object) {
            let (_, id, _) = items.objects.swap_remove(at);
            items.broke(id);
            id
        } else if let Some((_, id)) = items.exploded.iter().find(|(e, _)| *e == object) {
            // A blast (issue #18): each of our karts in it.
            *id
        } else if let Some(at) = items.shown_held.iter().position(|(e, _)| *e == object) {
            let (_, holder) = items.shown_held.swap_remove(at);
            let Some(owner) = id_of(holder) else {
                continue;
            };
            ItemId::held(owner)
        } else {
            continue;
        };
        let end = ItemBreak {
            session,
            id,
            kart: Some(kart),
            landed: hit.landed,
        };
        items.log.breaks.push((end.id, end.kart, end.landed, now));
        items.outbox.push(Message::ItemBreak(end));
    }
    // Items that broke on our shields, or met ours, or our klaxons destroyed (issue #18: what
    // another peer's kart held out too: its holder loses it).
    for block in blocked.read() {
        let Some(at) = items.objects.iter().position(|(e, ..)| *e == block.object) else {
            if let Some(at) = items
                .shown_held
                .iter()
                .position(|(e, _)| *e == block.object)
            {
                let (_, holder) = items.shown_held.swap_remove(at);
                if let Some(owner) = id_of(holder) {
                    let end = ItemBreak {
                        session,
                        id: ItemId::held(owner),
                        kart: None,
                        landed: false,
                    };
                    items.log.breaks.push((end.id, end.kart, end.landed, now));
                    items.outbox.push(Message::ItemBreak(end));
                }
            }
            continue;
        };
        let (_, id, _) = items.objects.swap_remove(at);
        if !items.broken.contains(&id) {
            items.broke(id);
        }
        let end = ItemBreak {
            session,
            id,
            kart: None,
            landed: false,
        };
        items.log.breaks.push((end.id, end.kart, end.landed, now));
        items.outbox.push(Message::ItemBreak(end));
    }
    // Croquettes our karts lost: where they fly, the same for everyone.
    let mut scattered: Vec<Entity> = Vec::new();
    for loss in lost.read() {
        let Ok(net) = local.get(loss.kart) else {
            continue;
        };
        if scattered.contains(&loss.kart) {
            continue;
        }
        scattered.push(loss.kart);
        let flying: Vec<(Entity, &LooseCroquette)> = new_loose
            .iter()
            .filter(|(entity, loose)| {
                loose.dropped_by == loss.kart && !items.loose.iter().any(|(e, _)| e == entity)
            })
            .collect();
        for chunk in flying.chunks(MAX_SCATTER) {
            let loss_number = items.next_loss(net.0);
            let scatter = Scatter {
                session,
                kart: net.0,
                loss: loss_number,
                tick: now,
                from: chunk[0].1.from.to_array(),
                to: chunk.iter().map(|(_, loose)| loose.to.to_array()).collect(),
            };
            for (index, (entity, _)) in chunk.iter().enumerate() {
                let id = LooseId {
                    kart: net.0,
                    loss: loss_number,
                    index: index as u8,
                };
                items.loose.push((*entity, id));
            }
            items
                .log
                .scatters
                .push((scatter.kart, scatter.loss, scatter.tick, scatter.to.clone()));
            items.outbox.push(Message::Scatter(scatter));
        }
    }
    // Issue #21: our boomerangs gone by themselves (caught by their thrower, mostly: on the
    // others' screens its body is a little behind, its copy might go on): gone on every screen.
    let mut caught = Vec::new();
    for (entity, id, peer) in &items.objects {
        if *peer == me
            && !alive_objects.contains(*entity)
            && !items.broken.contains(id)
            && items
                .log
                .thrown
                .iter()
                .any(|(thrown, item, _)| thrown == id && *item == ItemKind::Boomerang)
        {
            caught.push(*id);
        }
    }
    for id in caught {
        items.broke(id);
        let end = ItemBreak {
            session,
            id,
            kart: None,
            landed: false,
        };
        items.log.breaks.push((end.id, end.kart, end.landed, now));
        items.outbox.push(Message::ItemBreak(end));
    }
    // What is gone here by itself (too old, broken on a wall, fallen, vanished).
    items
        .objects
        .retain(|(entity, ..)| alive_objects.contains(*entity));
    items
        .loose
        .retain(|(entity, _)| alive_loose.contains(*entity));
}
