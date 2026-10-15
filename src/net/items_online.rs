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
//!   tells each ([`ItemBreak`]); the others see its shock wave ([`Honk`]). The other items of
//!   phase 2 do not play online yet (`items::ItemKind::online`): the online table leaves them
//!   out.
//! - **A new host** decides what the old one left undecided (every peer keeps every claim), and
//!   every peer sends its claims still undecided again: the new host answers again those it had
//!   decided already. The items thrown by a peer that leaves disappear.
//!
//! Every message carries the session of the kart states (`KartSync::session`): a race's messages
//! never mix with the free driving's. Nothing here runs without a room.

use bevy::prelude::*;

use super::pickups::{Arbiter, ClaimKey};
use super::protocol::{
    BlastOut, Claim, Honk, ItemBreak, ItemId, ItemOut, KartId, LooseId, MAX_SCATTER, Message,
    OUT_ITEMS, OutMotion, PeerKey, Pickup, Scatter, Spot, Verdict,
};
use super::sync::{KartSync, NetKart, RemoteKartProxy};
use super::{NetSession, online::local_tick};
use crate::croquettes::{
    CroquetteSystems, CroquetteTaken, CroquetteTuning, Croquettes, CroquettesLost, LooseCroquette,
    TrackCroquette,
};
use crate::items::{
    Blast, BoxBroken, CatchUp, Flight, Hold, Honked, ItemBlocked, ItemBox, ItemHitJudged, ItemKind,
    ItemObject, ItemSlots, ItemStats, ItemSystems, ItemTable, ItemTuning, Motion, OnlineItemTable,
    blast_radius, draw_unit, held_offset, item_radius,
};
use crate::kart::{Kart, KartSystems};
use crate::race::{RaceGrid, seconds_to_ticks};
use crate::track::{GRID_SLOTS, SpotIndex};

/// An item from another peer catches up this many ticks at most (2 s): older, it is late anyway.
const MAX_CATCH_UP: u32 = 120;
/// Items broken lately, kept to drop an [`ItemOut`] that comes after its break, at most.
const BROKEN_KEPT: usize = 256;
/// Items exploded lately (issue #18), kept for the victims' decisions of their blast, at most.
const EXPLODED_KEPT: usize = 32;

/// What we take back if the host gives a box or a croquette to another kart.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Undo {
    /// The roulettes started in the hand, the reserve.
    Box([bool; 2]),
    /// The croquettes it counted.
    Croquettes(u8),
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
    /// The host on the last tick: when it changes, our undecided claims go again.
    last_host: Option<PeerKey>,
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
        format!(
            "{boxes} boxes and {} croquettes taken, {} items out ({bombes} bombes), {} klaxons, {hits} hits, {} croquette losses",
            grants.len() - boxes,
            self.log.thrown.len(),
            self.log.honks.len(),
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

/// The respawn time of a spot, in ticks (`None`: a scattered croquette, once).
fn respawn(spot: Spot, items: &ItemTuning, croquettes: &CroquetteTuning) -> Option<u32> {
    match spot {
        Spot::Box(_) => Some(seconds_to_ticks(items.box_respawn_time)),
        Spot::Croquette(_) => Some(seconds_to_ticks(croquettes.respawn_time)),
        Spot::Loose(_) => None,
    }
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
/// breaks, scattered croquettes), and the host's verdicts.
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
    local: Query<(Entity, &'static NetKart), With<Kart>>,
    mut ours: Query<(&mut ItemSlots, &mut ItemStats, Option<&mut Croquettes>), With<Kart>>,
    mut spots: (
        Query<(&SpotIndex, &mut ItemBox)>,
        Query<(&SpotIndex, &mut TrackCroquette)>,
    ),
    objects: Query<(Entity, &ItemObject)>,
    mut inbox: Local<Vec<(PeerKey, Message)>>,
    mut verdicts: Local<Vec<Verdict>>,
    (mut blasts, mut honks): (MessageWriter<Blast>, MessageWriter<Honked>),
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
    // A new host: our undecided claims go again (the new one may not have them all, and answers
    // again those it decided already).
    let host = lobby.host();
    if items.last_host != host {
        if items.last_host.is_some() {
            for mine in &items.mine {
                items.outbox.push(Message::Claim(mine.claim));
            }
        }
        items.last_host = host;
    }
    // Who left: its claims are dropped, its items out disappear.
    for peer in std::mem::take(&mut items.left) {
        items.arbiter.forget(peer);
        items.objects.retain(|(entity, _, from)| {
            if *from == peer {
                commands.entity(*entity).try_despawn();
            }
            *from != peer
        });
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
                let key = ClaimKey::of(&claim);
                match items.arbiter.decided(&key) {
                    // Decided already (a claim sent again to a new host): its verdict again.
                    Some(granted) if lobby.is_host() => {
                        items
                            .outbox
                            .push(Message::Verdict(Verdict { claim, granted }));
                    }
                    Some(_) => {}
                    None => {
                        items.arbiter.claim(from, claim);
                    }
                }
            }
            Message::Verdict(verdict) if verdict.claim.session == current => {
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
                // where it says (each peer judges its own karts in the blast).
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
    // The host decides.
    if lobby.is_host() {
        verdicts.clear();
        items.arbiter.decide(respawn, &mut verdicts);
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
    }
}

/// The host decided a claim: ours refused, its roulettes or its croquette taken back; granted,
/// the box or the croquette gone here too, back on the same tick as everywhere.
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
) {
    let claim = verdict.claim;
    let key = ClaimKey::of(&claim);
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
                }
            }
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
                // The same draws as its peer (a race: its grid slot is its id).
                let slot = if claim.session == 0 {
                    0
                } else {
                    u64::from(claim.kart)
                };
                let drawn = (0..u32::from(draws))
                    .filter_map(|k| {
                        table
                            .draw(gap, draw_unit(seed, slot, u32::from(draw) + k))
                            .item()
                    })
                    .collect();
                items.log.draws.push((claim.kart, claim.tick, drawn));
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
    objects: &Query<(Entity, &ItemObject)>,
) {
    let owner = karts.ours(end.id.owner);
    if end.id.is_held() {
        // One of what our kart holds out broke on another peer's kart.
        let Some(owner) = owner else {
            return;
        };
        if let Ok((mut slots, ..)) = ours.get_mut(owner) {
            slots.lose_one_held_out();
        }
        let leaving = objects
            .iter()
            .filter(|(_, o)| o.owner == owner)
            .filter_map(|(entity, o)| o.held().map(|hold| (entity, hold)))
            .max_by_key(|(_, hold)| match hold {
                Hold::Trail(k) | Hold::Orbit(k) => *k,
                Hold::Dragged => 0,
            });
        if let Some((entity, _)) = leaving {
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
    // on each screen: each victim decided), both count.
    if items
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
        let radius = item_radius(item, &tuning);
        let holds = if slots.dragging.is_some() { 1 } else { count };
        for k in 0..holds {
            let hold = match (slots.dragging.is_some(), item) {
                (true, _) => Hold::Dragged,
                (false, ItemKind::Pelure) => Hold::Trail(k),
                (false, _) => Hold::Orbit(k),
            };
            let at = transform.transform_point(held_offset(hold, 3, radius, 0.0, &tuning));
            let entity = commands
                .spawn((
                    Name::new(item.label()),
                    Transform::from_translation(at),
                    ItemObject::new(item, kart, Motion::Held(hold), at, Vec3::ZERO),
                ))
                .id();
            items.shown_held.push((entity, kart));
        }
    }
}

/// After the item and croquette rules: what our karts did goes to every peer: their claims on
/// boxes and croquettes, the items they threw or dropped, the items we judged broken (on our
/// karts, on our shields), the croquettes they lost.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn send_items(
    mut items: ResMut<OnlineItems>,
    sync: Res<KartSync>,
    time: Res<Time<Fixed>>,
    local: Query<&NetKart, With<Kart>>,
    proxies: Query<&RemoteKartProxy>,
    spots: Query<&SpotIndex>,
    (mut boxes, mut taken, mut judged, mut blocked, mut lost): (
        MessageReader<BoxBroken>,
        MessageReader<CroquetteTaken>,
        MessageReader<ItemHitJudged>,
        MessageReader<ItemBlocked>,
        MessageReader<CroquettesLost>,
    ),
    (mut blasts, mut honks): (MessageReader<Blast>, MessageReader<Honked>),
    new_objects: Query<(Entity, &ItemObject), Added<ItemObject>>,
    new_loose: Query<(Entity, &LooseCroquette), Added<LooseCroquette>>,
    (alive_objects, alive_loose): (Query<(), With<ItemObject>>, Query<(), With<LooseCroquette>>),
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
            Motion::Ground | Motion::Held(_) | Motion::Pillow { .. } => OutMotion::Ground,
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
        };
        let id = ItemId {
            owner: net.0,
            seq: items.next_item(net.0),
        };
        items.objects.push((entity, id, me));
        items.log.thrown.push((id, object.item, now));
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
    // Items that touched our karts: we decided.
    for hit in judged.read() {
        let (Some(object), Some(kart)) = (hit.object, id_of(hit.kart)) else {
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
    // What is gone here by itself (too old, broken on a wall, fallen, vanished).
    items
        .objects
        .retain(|(entity, ..)| alive_objects.contains(*entity));
    items
        .loose
        .retain(|(entity, _)| alive_loose.contains(*entity));
}
