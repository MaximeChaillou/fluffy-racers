//! The wire protocol between peers (issue #7, `docs/ARCHITECTURE.md` § « Réseau »). Pure logic,
//! no Bevy and no socket: unit-tested.
//!
//! **Format**: one message per packet, little-endian, no padding.
//!
//! | byte | content |
//! |---|---|
//! | 0 | [`PROTOCOL_VERSION`]: a peer of another version is shown as such, never misread |
//! | 1 | the kind of message ([`Message::kind`]) |
//! | 2… | its fields, in order |
//!
//! Strings are a length byte then UTF-8; lists a count byte then their items; booleans one byte,
//! 0 or 1. Decoding never panics: a packet that is empty, truncated, too long, of an unknown kind,
//! of another version, or with an impossible value (a boolean of 2, a float that is not finite,
//! more seats than players) is refused with its [`DecodeError`].
//!
//! **Channels** ([`Channel`]): the lobby and the events go on the reliable channel; the pings and
//! the kart states (issue #8, [`KartSnapshot`], 30 per second) on the unreliable one: a
//! retransmitted ping would measure the retransmission, and a lost kart state is replaced by the
//! next one 33 ms later.
//!
//! Version 2 (issue #8): the kart states. Version 3 (issue #9): the race online (the room's
//! circuit, the race's start with its players, the checkpoints, the end of the race and its final
//! order), the kart states' session, and the pongs tell whether they carry the room's clock.
//! Version 4 (issue #53): the items and the croquettes online: a kart's claim on a box or a
//! croquette and the host's verdict ([`Claim`], [`Verdict`]), an item thrown or dropped
//! ([`ItemOut`]), an item that broke ([`ItemBreak`]: on a kart, the victim's decision), the
//! croquettes a kart lost ([`Scatter`]), and in the kart states what a kart holds out
//! (`KartSnapshot::items`, 49 bytes). Version 5 (issue #55): the kart's color in the `Hello`
//! ([`Profile::color`], a byte of the palette). Version 6 (issue #18): the bombe à confettis
//! thrown or dropped ([`OutMotion::Bomb`]), its explosion ([`BlastOut`]: where, when), the
//! klaxon-canard ([`Honk`]). Version 7 (issue #61): the other items of phase 2: the chantilly
//! and the dégonflage told by their user ([`EffectOut`], with its ranking), a kart knocked over by
//! a super câlin or a fusée ([`Rammed`]: the victim's decision), the oreiller géant thrown
//! ([`OutMotion::Pillow`]), its claim to the host ([`Pickup::Pillow`]), its new target
//! ([`Retarget`]) and its end without a throw ([`PillowGone`]); in the kart states, the super
//! câlin, the fusée and the dégonflage (`KartSnapshot::flags`). Version 8 (issue #63): the star
//! around the host: a client's message for the others goes to the host, which relays it with its
//! origin ([`Message::Relayed`]), and the host sends each client one packet with every kart's
//! newest state ([`Message::KartStates`], grouped by the peer that drives them). Version 9 (issue
//! #70): the host's closed results ([`RaceResults`]: every racer's time, estimated for those
//! still on track).

use std::fmt;

use crate::interactions::TetherEnd;
use crate::items::ItemKind;
use crate::kart::KartColor;
use crate::race::{ResultLine, ResultTime};

/// Version of this protocol: first byte of every packet. Bump it whenever a message changes.
pub const PROTOCOL_VERSION: u8 = 9;

/// Players in a room at most (GD § 1: 12 racers).
pub const MAX_PLAYERS: usize = 12;

/// A player's name on the wire: 16 characters at most (`Profile::clean_name`), so at most 64
/// bytes of UTF-8.
pub const MAX_NAME_CHARS: usize = 16;
pub const MAX_NAME_BYTES: usize = 64;

/// A peer of the room: the `PeerId` (a UUID) the signaling server gave it, as a number. Since
/// issue #63, the host is the peer that created the room, whatever its id (`docs/ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerKey(pub u128);

impl PeerKey {
    /// A short tag to tell two players apart on screen (the first 4 hexadecimal digits).
    pub fn tag(self) -> String {
        format!("{:04x}", (self.0 >> 112) as u16)
    }
}

/// The two data channels of the socket, in the order they are created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Ordered, retransmitted: lobby and events.
    Reliable,
    /// Unordered, never retransmitted: pings and kart states (issue #8).
    Unreliable,
}

impl Channel {
    pub const ALL: [Self; 2] = [Channel::Reliable, Channel::Unreliable];

    /// Index of the channel in the socket.
    pub const fn index(self) -> usize {
        match self {
            Channel::Reliable => 0,
            Channel::Unreliable => 1,
        }
    }
}

/// Who a player is, sent to every peer on connection and when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    /// Name shown in the lobby ([`Profile::clean_name`]).
    pub name: String,
    /// Indices of the character, kart, wheels and wing (issue #15, `characters::Loadout`): every
    /// peer knows the stats of every kart from the lobby on.
    pub loadout: [u8; 4],
    /// « Prêt ».
    pub ready: bool,
    /// The player has a seat in the room (see `lobby`): newcomers wait for the host's.
    pub seated: bool,
    /// The color it chose for its kart (issue #55): it wears it unless a player of an earlier
    /// seat chose it too (`Lobby::color_of`).
    pub color: KartColor,
}

impl Profile {
    /// A name fit for the wire and the screen: no control character, no space at either end, at
    /// most [`MAX_NAME_CHARS`] characters.
    pub fn clean_name(name: &str) -> String {
        name.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(MAX_NAME_CHARS)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// The host starts a race (issue #9): everything a peer needs to build the same race, and when it
/// starts on the room's clock (`clock::RoomClock`, `Lobby::room_time_ms`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceStart {
    /// The race's number in the room: the host counts them (a new host goes on). The events of
    /// another race ([`Progress`], [`RaceOver`]) are ignored.
    pub race: u16,
    /// `RaceGrid::seed` (issue #40): every peer draws the same bots.
    pub seed: u64,
    /// Index of the circuit in `assets/tracks/index.ron`.
    pub track: u8,
    pub laps: u8,
    /// Bots that complete the grid: they take its front slots (0, 1...), like solo.
    pub bots: u8,
    /// The bots' level (`bots::BotDifficulty`: 0 easy, 1 normal, 2 hard).
    pub difficulty: u8,
    /// The common tick (60 per second on the room's clock) of the race's first tick on the grid:
    /// the countdown follows, the same for everyone.
    pub start_tick: u32,
    /// The players, in grid order: they start behind the bots (slot `bots`, then `bots + 1`...).
    pub players: Vec<PeerKey>,
}

impl RaceStart {
    /// The grid slot (and network id) of `peer`'s kart, if it races.
    pub fn slot_of(&self, peer: PeerKey) -> Option<KartId> {
        self.players
            .iter()
            .position(|p| *p == peer)
            .map(|index| self.bots + index as u8)
    }

    /// The racers: bots and players.
    pub fn racers(&self) -> usize {
        usize::from(self.bots) + self.players.len()
    }
}

/// A racer validated a checkpoint (issue #9): the peer that has authority over its kart (its
/// player, or the host for a bot) tells everyone. `passed`: the checkpoints validated since the
/// start, the start line included (`race::RaceProgress::passed`); `tick`: the race tick it
/// happened on (ticks since GO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub race: u16,
    pub kart: KartId,
    pub passed: u16,
    pub tick: u32,
}

/// The host ends the race (issue #9): at race tick `tick`, with this final order (first to
/// last, every racer once). Everyone shows these results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceOver {
    pub race: u16,
    pub tick: u32,
    pub order: Vec<KartId>,
}

/// The host closes the results (issue #70, `race::close_results`): the final table, first to
/// last, every racer once, each with its time: finished, estimated (still on track when the
/// results closed), or none (gave up). Everyone shows these lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceResults {
    pub race: u16,
    pub lines: Vec<ResultLine<KartId>>,
}

/// A kart on the network: its slot on the grid (issue #8 maps it to the kart's entity).
pub type KartId = u8;

/// `Coup` (`interactions::KartStruck`, issue #34): the striker tells the victim, which decides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub striker: KartId,
    pub target: KartId,
    pub tick: u32,
    /// Tick of the target's state the striker saw (`tick_vu`).
    pub seen: u32,
    pub striker_position: [f32; 3],
    /// Horizontal direction of the push, unit (x, z of the world).
    pub direction: [f32; 2],
    /// Push speed, the Weights and a throw's factor applied (m/s).
    pub push: f32,
    pub throw: bool,
}

/// `CoupReçu` / `CoupIgnoré` (`interactions::StrikeAnswer`): the victim's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrikeReply {
    pub striker: KartId,
    pub target: KartId,
    pub landed: bool,
    pub throw: bool,
}

/// `Attrape` (`interactions::KartGrabbed`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grab {
    pub grabber: KartId,
    pub target: KartId,
    pub tick: u32,
    pub seen: u32,
    pub grabber_position: [f32; 3],
    /// Rest length of the tether (m).
    pub rest: f32,
}

/// `FinAttrape` (`interactions::TetherEnded`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TetherBreak {
    pub grabber: KartId,
    pub target: KartId,
    pub reason: TetherEnd,
}

/// A kart's state at a tick (issue #8, `docs/ARCHITECTURE.md` § « Synchronisation »): the peer
/// that has authority over the kart sends it 30 times a second, on the unreliable channel.
/// Quantized by `net::snapshot` (`KartSnapshot::quantize`, `KartSnapshot::pose`): every field is
/// the integer that travels, so that what is read writes back to the same bytes. 48 bytes with
/// the version and the kind (fewer than [`MAX_KART_STATE_BYTES`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KartSnapshot {
    /// Tick of the room's common clock (60 per second, on the room's clock: `net::TickClock`).
    pub tick: u32,
    /// The kart: its grid slot (the same id as the interactions' [`KartId`]).
    pub kart: KartId,
    /// What the kart ids mean (issue #9): 0 in free driving (the seats), the race's number (on 8
    /// bits, never 0) in a race (the grid slots). A state of another session is ignored.
    pub session: u8,
    /// Position (mm).
    pub position: [i32; 3],
    /// Rotation, « smallest three » on 48 bits (`snapshot::pack_rotation`); the last bit is 0.
    pub rotation: [u8; 6],
    /// Linear velocity (cm/s).
    pub velocity: [i16; 3],
    /// Angular velocity (mrad/s).
    pub spin: [i16; 3],
    /// Bits 0-2: the phase (grounded, hop, drift, airborne, trick, hit, towed); 3: drifting to
    /// the right; 4-5: the sparks' tier (0: none); 6: tumbling (hit); 7: 0.
    pub state: u8,
    /// Bit 0: stalled engine; 1: invincible; issue #61: 2 in a super câlin, 3 in a fusée, 4
    /// deflated; the others are 0.
    pub flags: u8,
    /// Hit by an item: ticks since the hit, and its duration (ticks).
    pub hit: [u8; 2],
    /// Boost: strength (1/200: 60 = +30 %) and time left (ticks).
    pub boost: [u8; 2],
    /// The drift's slip angle (5 mrad).
    pub slip: i8,
    /// The slipstream gauge while it charges (1 to 255), 0 otherwise.
    pub slipstream: u8,
    /// The 4 wheels, 4 bits each (front left first): 0 in the air, 1 to 15 on the ground with a
    /// spring compressed by 0 to 0.7 m.
    pub wheels: u16,
    /// What the kart holds out (issue #53, `items::ItemSlots::held_out`): bits 0-1 the item
    /// (0 nothing, 1 pelure, 2 pelote, 3 abeille), 2-3 how many (1 to 3), 4 dragged behind (a
    /// single pelure or pelote; else an "x3" out); the others are 0. Every peer draws them and
    /// knows the kart's shields.
    pub items: u8,
}

/// A kart state fits in a small unreliable packet (issue #8: fewer than 64 bytes).
pub const MAX_KART_STATE_BYTES: usize = 64;

/// The items that go out on the track, in their wire order (`ItemOut::item`). Issue #18: the
/// bombe à confettis; issue #61: the oreiller géant.
pub const OUT_ITEMS: [ItemKind; 5] = [
    ItemKind::Pelure,
    ItemKind::Pelote,
    ItemKind::Abeille,
    ItemKind::Bombe,
    ItemKind::OreillerGeant,
];

/// The highest flags of a kart state (`KartSnapshot::flags`): bits 5 to 7 are 0.
pub const KART_FLAGS: u8 = 0b1_1111;

/// The items whose user tells the others what they do to them (issue #61, [`EffectOut`]), in
/// their wire order.
pub const EFFECT_ITEMS: [ItemKind; 2] = [ItemKind::Chantilly, ItemKind::Degonflage];

/// The items that make a kart knock over those it runs into (issue #61, [`Rammed`]), in their
/// wire order.
pub const RAM_ITEMS: [ItemKind; 2] = [ItemKind::SuperCalin, ItemKind::Fusee];

/// The items a kart can hold out, in their wire order (`KartSnapshot::items`, from 1).
pub const HELD_ITEMS: [ItemKind; 3] = [ItemKind::Pelure, ItemKind::Pelote, ItemKind::Abeille];

/// Whether `items` (`KartSnapshot::items`) is a possible value.
pub fn valid_held_items(items: u8) -> bool {
    let (kind, count, dragged) = (items & 0b11, (items >> 2) & 0b11, items & 0b1_0000 != 0);
    if items & 0b1110_0000 != 0 {
        return false;
    }
    if kind == 0 {
        return items == 0;
    }
    // Only a single pelure or pelote is dragged.
    count >= 1 && (!dragged || (count == 1 && kind <= 2))
}

/// An item out on the track (issue #53): the kart that threw it (or holds it out) and its number
/// among that kart's items. `seq` [`ItemId::HELD`]: what the kart holds out (its shields), which
/// are not numbered (the kart states say them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId {
    pub owner: KartId,
    pub seq: u16,
}

impl ItemId {
    /// The number of a kart's items held out.
    pub const HELD: u16 = u16::MAX;

    /// One of the items `owner` holds out.
    pub fn held(owner: KartId) -> Self {
        Self {
            owner,
            seq: Self::HELD,
        }
    }

    pub fn is_held(&self) -> bool {
        self.seq == Self::HELD
    }
}

/// How an item thrown or dropped moves (`items::Motion`, issue #53), with the karts by id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutMotion {
    /// A pelure on the ground.
    Ground,
    /// Rolling straight: a pelote, or an abeille thrown backwards.
    Rolling { bounces_left: u8, fall: f32 },
    /// An abeille along the track's centerline, after `target` (`None`: the leader's).
    Track {
        target: Option<KartId>,
        distance: f32,
        lateral: f32,
    },
    /// An abeille diving at `target`, or straight on without one.
    Dive { target: Option<KartId> },
    /// A bombe à confettis (issue #18): its vertical speed, on the ground or not, and whether it
    /// explodes where it lands (thrown ahead) or at the end of its fuse (dropped behind).
    Bomb {
        vertical: f32,
        grounded: bool,
        impact: bool,
    },
    /// An oreiller géant (issue #61) after `target` (the leader in its thrower's ranking; `None`:
    /// no one), and where it is in its flight.
    Pillow {
        target: Option<KartId>,
        flight: OutPillow,
    },
}

/// An oreiller géant's flight on the wire (`items::PillowFlight`, issue #61).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutPillow {
    /// Along the track's centerline, at race distance `distance`, `lateral` meters right of it.
    Track { distance: f32, lateral: f32 },
    /// Over its target for `left` seconds more ([`OutPillow::WAITING`]: until it is over it).
    Hover { left: f32 },
    /// Dropping on its target.
    Drop,
}

impl OutPillow {
    /// A hover that has not started counting (off a race, `items::PillowFlight::Hover` with an
    /// infinite time): the wire's floats are finite.
    pub const WAITING: f32 = f32::MAX;
}

/// An item thrown or dropped by a kart (issue #53): the thrower has authority over its path. It
/// tells every peer its state at the end of common tick `tick`; each peer simulates it from
/// there (rays against the track, the centerline, its target).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemOut {
    /// The kart states' session (`KartSnapshot::session`): what the kart ids mean.
    pub session: u8,
    pub id: ItemId,
    pub item: ItemKind,
    pub tick: u32,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub motion: OutMotion,
}

/// An item broke (issue #53). On a kart (`kart`): the victim's peer decided whether it was hit
/// (`landed`; an item touching an immune kart breaks anyway). Without a kart: it met another item
/// (a shield, another projectile), told by the peer that judged it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemBreak {
    pub session: u8,
    pub id: ItemId,
    pub kart: Option<KartId>,
    pub landed: bool,
}

/// An item exploded (issue #18): a bombe à confettis that touched a kart or a shield of the peer
/// that tells it, at `position` on common tick `tick`. Every peer makes it explode there (its own
/// copy is gone), and judges its own karts in the blast. A bombe whose fuse ran out explodes on
/// every screen by itself (the same path, the same tick): no message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlastOut {
    pub session: u8,
    pub id: ItemId,
    pub tick: u32,
    pub position: [f32; 3],
}

/// A kart sounded its klaxon-canard (issue #18) at `position`, on common tick `tick`: its peer
/// judged the items it destroyed (and told each with an [`ItemBreak`]); the others see the shock
/// wave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Honk {
    pub session: u8,
    pub kart: KartId,
    pub tick: u32,
    pub position: [f32; 3],
}

/// A kart used a chantilly or a dégonflage (issue #61, [`EFFECT_ITEMS`]) at `position`, heading
/// `forward`, on common tick `tick`. Its peer tells everyone with its ranking at that moment
/// (`order`: the karts first to last; empty off a race): each peer applies it to its own karts
/// (the victim decides: immune or not), their rank taken from that order, the same everywhere.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectOut {
    pub session: u8,
    pub kart: KartId,
    pub item: ItemKind,
    pub tick: u32,
    pub position: [f32; 3],
    /// Its horizontal heading (x, z of the world): off a race, the chantilly reaches the karts
    /// ahead of it.
    pub forward: [f32; 2],
    pub order: Vec<KartId>,
}

/// A kart in a super câlin or a fusée (`by`, issue #61, [`RAM_ITEMS`]) ran into `kart` on common
/// tick `tick`: the victim's peer decided that it was knocked over, and tells everyone (its
/// rammer counts its hit). A kart that was immune says nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rammed {
    pub session: u8,
    pub kart: KartId,
    pub by: KartId,
    pub item: ItemKind,
    pub tick: u32,
}

/// The oreiller géant the host gave `kart` (its claim on box `index` at common tick `tick`) is
/// gone without being thrown (issue #61): a dégonflage made it fall, for instance. Every peer's
/// record forgets it: another one may come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillowGone {
    pub session: u8,
    pub kart: KartId,
    pub index: u16,
    pub tick: u32,
}

/// An oreiller géant goes after another kart from common tick `tick` (issue #61): its thrower's
/// leader changed (`None`: no one to go after). Every peer sends its copy after that kart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retarget {
    pub session: u8,
    pub id: ItemId,
    pub target: Option<KartId>,
    pub tick: u32,
}

/// A scattered croquette (issue #53): the kart that lost it, the number of that loss among the
/// kart's losses, and its place in the loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LooseId {
    pub kart: KartId,
    pub loss: u16,
    pub index: u8,
}

/// The croquettes a kart lost (issue #53): the victim decided, and tells where they fly, from
/// `from` to each of `to`, from common tick `tick`. Every peer shows them at the same places.
#[derive(Clone, Debug, PartialEq)]
pub struct Scatter {
    pub session: u8,
    pub kart: KartId,
    pub loss: u16,
    pub tick: u32,
    pub from: [f32; 3],
    pub to: Vec<[f32; 3]>,
}

/// Croquettes lost at once, at most (`CroquetteTuning::max_held`).
pub const MAX_SCATTER: usize = 10;

/// What a kart picks up on the track (issue #53): the host decides who got it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pickup {
    /// An item box (its number on the circuit), and the draws its kart made from it: the first
    /// draw's number (`ItemSlots::draws`), how many (0 to 2), its distance to the leader then.
    /// Every peer can draw the same items (`items::draw_unit`).
    Box {
        index: u16,
        draw: u16,
        draws: u8,
        gap: f32,
    },
    /// A croquette of the track (its number on the circuit).
    Croquette { index: u16 },
    /// A scattered croquette.
    Loose(LooseId),
    /// An oreiller géant drawn from box `index` (issue #61), by the kart's draw number `draw`:
    /// the host decides whether its rules allow one now (`pickups::Arbiter`). Refused, the kart
    /// gets that draw without the oreiller instead, as solo (`items::DrawRow::pick_without`).
    Pillow { index: u16, draw: u16 },
}

/// What is picked up, whatever the draws: a box, a croquette of the track, a scattered one; an
/// oreiller géant drawn from a box (issue #61).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spot {
    Box(u16),
    Croquette(u16),
    Loose(LooseId),
    Pillow(u16),
}

impl Pickup {
    pub fn spot(&self) -> Spot {
        match *self {
            Pickup::Box { index, .. } => Spot::Box(index),
            Pickup::Croquette { index } => Spot::Croquette(index),
            Pickup::Loose(id) => Spot::Loose(id),
            Pickup::Pillow { index, .. } => Spot::Pillow(index),
        }
    }
}

/// A kart touched a box or a croquette at common tick `tick` (issue #53): its peer took it at
/// once (prediction), and asks the host. Sent to every peer: a new host decides what the old one
/// left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claim {
    pub session: u8,
    pub pickup: Pickup,
    pub kart: KartId,
    pub tick: u32,
}

/// The host's decision on a claim (issue #53): granted (the box or the croquette is that kart's,
/// gone for everyone), or refused (another kart had it first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    pub claim: Claim,
    pub granted: bool,
}

/// Every message of the protocol.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Who I am (on connection, and when my name, loadout or seat changes).
    Hello(Profile),
    /// « Prêt » or not.
    Ready(bool),
    /// The host's list of seated players, in seat order (arrival order).
    Seats(Vec<PeerKey>),
    /// The host refuses a newcomer: the room already has [`MAX_PLAYERS`] players.
    RoomFull,
    /// I leave the room (a closed tab says nothing: the signaling server tells the others).
    Bye,
    /// The host starts the race (issue #9).
    StartRace(RaceStart),
    /// The host's circuit for the room (issue #9), index in `assets/tracks/index.ron`: everyone
    /// drives freely on it, until the host starts a race. Sent after a race too: back to free
    /// driving.
    Circuit(u8),
    /// A racer validated a checkpoint (issue #9).
    Progress(Progress),
    /// The host ends the race, with its final order (issue #9).
    RaceOver(RaceOver),
    /// The host closes the results, with every racer's time (issue #70).
    RaceResults(RaceResults),
    Strike(Strike),
    StrikeReply(StrikeReply),
    Grab(Grab),
    TetherBreak(TetherBreak),
    /// A kart touched a box or a croquette (issue #53): its peer asks the host.
    Claim(Claim),
    /// The host's decision on a claim (issue #53).
    Verdict(Verdict),
    /// An item thrown or dropped (issue #53): its thrower has authority over its path.
    ItemOut(ItemOut),
    /// An item broke (issue #53): on a kart (the victim decided), or on another item.
    ItemBreak(ItemBreak),
    /// The croquettes a kart lost fly out (issue #53).
    Scatter(Scatter),
    /// A bombe à confettis exploded on what touched it (issue #18).
    Blast(BlastOut),
    /// A klaxon-canard sounded (issue #18).
    Honk(Honk),
    /// A chantilly or a dégonflage used (issue #61).
    Effect(EffectOut),
    /// A kart knocked over by a super câlin or a fusée (issue #61).
    Rammed(Rammed),
    /// An oreiller géant gone without a throw (issue #61).
    PillowGone(PillowGone),
    /// An oreiller géant after another kart (issue #61).
    Retarget(Retarget),
    /// Latency and clock: `sent_ms` is the sender's clock, echoed back.
    Ping {
        sent_ms: u32,
    },
    /// The answer: the ping's time, and the clock of the peer that answers: the room's clock
    /// when it knows it (`room`, issue #9: `clock::RoomClock`), its own otherwise.
    Pong {
        echo_ms: u32,
        clock_ms: u32,
        room: bool,
    },
    /// The state of a kart of the sender (issue #8), 30 times a second: from a client to the host
    /// (issue #63: the host sends [`Message::KartStates`]).
    KartState(KartSnapshot),
    /// Issue #63: a message of `origin` (a client), relayed by the host to the other clients. The
    /// receiver handles it as if it came from `origin`: every check of authority relies on it.
    /// Only the messages a client tells everyone are relayed ([`Message::relayable`]).
    Relayed {
        origin: PeerKey,
        message: Box<Message>,
    },
    /// Issue #63: from the host to a client, 30 times a second, the newest state of every kart it
    /// does not drive (the host's, its bots', the other clients'), each with the peer that drives
    /// it. Grouped on the wire by that peer: the states of one peer follow each other here.
    KartStates(Vec<(PeerKey, KartSnapshot)>),
}

/// Kart states in one [`Message::KartStates`] at most: the karts of a grid.
pub const MAX_BUNDLED_STATES: usize = MAX_PLAYERS;

const HELLO: u8 = 1;
const READY: u8 = 2;
const SEATS: u8 = 3;
const ROOM_FULL: u8 = 4;
const BYE: u8 = 5;
const START_RACE: u8 = 6;
const CIRCUIT: u8 = 7;
const PROGRESS: u8 = 8;
const RACE_OVER: u8 = 9;
const RACE_RESULTS: u8 = 11;
const STRIKE: u8 = 16;
const STRIKE_REPLY: u8 = 17;
const GRAB: u8 = 18;
const TETHER_BREAK: u8 = 19;
const CLAIM: u8 = 24;
const VERDICT: u8 = 25;
const ITEM_OUT: u8 = 26;
const ITEM_BREAK: u8 = 27;
const SCATTER: u8 = 28;
const BLAST: u8 = 29;
const HONK: u8 = 30;
const EFFECT: u8 = 31;
const PING: u8 = 32;
const PONG: u8 = 33;
const RAMMED: u8 = 34;
const PILLOW_GONE: u8 = 35;
const RETARGET: u8 = 36;
const KART_STATE: u8 = 40;
const KART_STATES: u8 = 41;
const RELAYED: u8 = 10;

/// The reasons a tether ends, in their wire order.
const TETHER_ENDS: [TetherEnd; 10] = [
    TetherEnd::Released,
    TetherEnd::Thrown,
    TetherEnd::Escaped,
    TetherEnd::TimeUp,
    TetherEnd::Stretched,
    TetherEnd::Hit,
    TetherEnd::Towed,
    TetherEnd::Finished,
    TetherEnd::Refused,
    TetherEnd::Gone,
];

impl Message {
    /// Its kind: the second byte of the packet.
    pub fn kind(&self) -> u8 {
        match self {
            Message::Hello(_) => HELLO,
            Message::Ready(_) => READY,
            Message::Seats(_) => SEATS,
            Message::RoomFull => ROOM_FULL,
            Message::Bye => BYE,
            Message::StartRace(_) => START_RACE,
            Message::Circuit(_) => CIRCUIT,
            Message::Progress(_) => PROGRESS,
            Message::RaceOver(_) => RACE_OVER,
            Message::RaceResults(_) => RACE_RESULTS,
            Message::Strike(_) => STRIKE,
            Message::StrikeReply(_) => STRIKE_REPLY,
            Message::Grab(_) => GRAB,
            Message::TetherBreak(_) => TETHER_BREAK,
            Message::Claim(_) => CLAIM,
            Message::Verdict(_) => VERDICT,
            Message::ItemOut(_) => ITEM_OUT,
            Message::ItemBreak(_) => ITEM_BREAK,
            Message::Scatter(_) => SCATTER,
            Message::Blast(_) => BLAST,
            Message::Honk(_) => HONK,
            Message::Effect(_) => EFFECT,
            Message::Rammed(_) => RAMMED,
            Message::PillowGone(_) => PILLOW_GONE,
            Message::Retarget(_) => RETARGET,
            Message::Ping { .. } => PING,
            Message::Pong { .. } => PONG,
            Message::KartState(_) => KART_STATE,
            Message::KartStates(_) => KART_STATES,
            Message::Relayed { .. } => RELAYED,
        }
    }

    /// Issue #63: a message a client tells every peer, which the host relays to the other clients
    /// with its origin ([`Message::Relayed`]). Not the host's own (the seats, the race's start,
    /// end and results, the verdicts), nor the pings (each link has its own), nor the kart states (the host
    /// bundles them, [`Message::KartStates`]).
    pub fn relayable(&self) -> bool {
        !matches!(
            self,
            Message::Seats(_)
                | Message::RoomFull
                | Message::StartRace(_)
                | Message::Circuit(_)
                | Message::RaceOver(_)
                | Message::RaceResults(_)
                | Message::Verdict(_)
                | Message::Ping { .. }
                | Message::Pong { .. }
                | Message::KartState(_)
                | Message::KartStates(_)
                | Message::Relayed { .. }
        )
    }

    /// The channel it goes on.
    pub fn channel(&self) -> Channel {
        match self {
            Message::Ping { .. }
            | Message::Pong { .. }
            | Message::KartState(_)
            | Message::KartStates(_) => Channel::Unreliable,
            Message::Relayed { message, .. } => message.channel(),
            _ => Channel::Reliable,
        }
    }

    /// Writes the packet into `out` (emptied first: a buffer reused for every message).
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.clear();
        out.push(PROTOCOL_VERSION);
        self.encode_body(out);
    }

    /// Its kind and its fields, after the version byte (a relayed message's own, after its
    /// origin).
    fn encode_body(&self, out: &mut Vec<u8>) {
        out.push(self.kind());
        match self {
            Message::Hello(profile) => {
                put_name(out, &profile.name);
                out.extend_from_slice(&profile.loadout);
                out.push(u8::from(profile.ready) | u8::from(profile.seated) << 1);
                out.push(profile.color.index());
            }
            Message::Ready(ready) => out.push(u8::from(*ready)),
            Message::Seats(seats) => put_keys(out, seats),
            Message::RoomFull | Message::Bye => {}
            Message::StartRace(start) => {
                out.extend_from_slice(&start.race.to_le_bytes());
                out.extend_from_slice(&start.seed.to_le_bytes());
                out.extend_from_slice(&[start.track, start.laps, start.bots, start.difficulty]);
                put_u32(out, start.start_tick);
                put_keys(out, &start.players);
            }
            Message::Circuit(track) => out.push(*track),
            Message::Progress(progress) => {
                out.extend_from_slice(&progress.race.to_le_bytes());
                out.push(progress.kart);
                out.extend_from_slice(&progress.passed.to_le_bytes());
                put_u32(out, progress.tick);
            }
            Message::RaceOver(over) => {
                out.extend_from_slice(&over.race.to_le_bytes());
                put_u32(out, over.tick);
                let count = over.order.len().min(MAX_PLAYERS);
                out.push(count as u8);
                out.extend_from_slice(&over.order[..count]);
            }
            Message::RaceResults(results) => {
                out.extend_from_slice(&results.race.to_le_bytes());
                let count = results.lines.len().min(MAX_PLAYERS);
                out.push(count as u8);
                for line in &results.lines[..count] {
                    out.push(line.id);
                    let (kind, ticks) = match line.time {
                        ResultTime::Finished(ticks) => (0, ticks),
                        ResultTime::Estimated(ticks) => (1, ticks),
                        ResultTime::GaveUp => (2, 0),
                    };
                    out.push(kind);
                    put_u32(out, ticks);
                }
            }
            Message::Strike(strike) => {
                out.extend_from_slice(&[strike.striker, strike.target]);
                put_u32(out, strike.tick);
                put_u32(out, strike.seen);
                put_floats(out, &strike.striker_position);
                put_floats(out, &strike.direction);
                put_floats(out, &[strike.push]);
                out.push(u8::from(strike.throw));
            }
            Message::StrikeReply(reply) => {
                out.extend_from_slice(&[reply.striker, reply.target]);
                out.push(u8::from(reply.landed) | u8::from(reply.throw) << 1);
            }
            Message::Grab(grab) => {
                out.extend_from_slice(&[grab.grabber, grab.target]);
                put_u32(out, grab.tick);
                put_u32(out, grab.seen);
                put_floats(out, &grab.grabber_position);
                put_floats(out, &[grab.rest]);
            }
            Message::TetherBreak(end) => {
                let reason = TETHER_ENDS.iter().position(|r| *r == end.reason);
                out.extend_from_slice(&[end.grabber, end.target, reason.unwrap_or(0) as u8]);
            }
            Message::Claim(claim) => put_claim(out, claim),
            Message::Verdict(verdict) => {
                put_claim(out, &verdict.claim);
                out.push(u8::from(verdict.granted));
            }
            Message::ItemOut(item) => {
                out.push(item.session);
                put_item_id(out, item.id);
                out.push(out_index(item.item));
                put_u32(out, item.tick);
                put_floats(out, &item.position);
                put_floats(out, &item.velocity);
                match item.motion {
                    OutMotion::Ground => out.push(0),
                    OutMotion::Rolling { bounces_left, fall } => {
                        out.extend_from_slice(&[1, bounces_left]);
                        put_floats(out, &[fall]);
                    }
                    OutMotion::Track {
                        target,
                        distance,
                        lateral,
                    } => {
                        out.extend_from_slice(&[2, target.unwrap_or(NO_KART)]);
                        put_floats(out, &[distance, lateral]);
                    }
                    OutMotion::Dive { target } => {
                        out.extend_from_slice(&[3, target.unwrap_or(NO_KART)]);
                    }
                    OutMotion::Bomb {
                        vertical,
                        grounded,
                        impact,
                    } => {
                        out.extend_from_slice(&[4, u8::from(grounded) | u8::from(impact) << 1]);
                        put_floats(out, &[vertical]);
                    }
                    OutMotion::Pillow { target, flight } => {
                        out.extend_from_slice(&[5, target.unwrap_or(NO_KART)]);
                        match flight {
                            OutPillow::Track { distance, lateral } => {
                                out.push(0);
                                put_floats(out, &[distance, lateral]);
                            }
                            OutPillow::Hover { left } => {
                                out.push(1);
                                put_floats(out, &[left]);
                            }
                            OutPillow::Drop => out.push(2),
                        }
                    }
                }
            }
            Message::ItemBreak(end) => {
                out.push(end.session);
                put_item_id(out, end.id);
                out.extend_from_slice(&[end.kart.unwrap_or(NO_KART), u8::from(end.landed)]);
            }
            Message::Scatter(scatter) => {
                out.extend_from_slice(&[scatter.session, scatter.kart]);
                out.extend_from_slice(&scatter.loss.to_le_bytes());
                put_u32(out, scatter.tick);
                put_floats(out, &scatter.from);
                let count = scatter.to.len().min(MAX_SCATTER);
                out.push(count as u8);
                for to in &scatter.to[..count] {
                    put_floats(out, to);
                }
            }
            Message::Blast(blast) => {
                out.push(blast.session);
                put_item_id(out, blast.id);
                put_u32(out, blast.tick);
                put_floats(out, &blast.position);
            }
            Message::Honk(honk) => {
                out.extend_from_slice(&[honk.session, honk.kart]);
                put_u32(out, honk.tick);
                put_floats(out, &honk.position);
            }
            Message::Effect(effect) => {
                let item = EFFECT_ITEMS.iter().position(|i| *i == effect.item);
                out.extend_from_slice(&[effect.session, effect.kart, item.unwrap_or(0) as u8]);
                put_u32(out, effect.tick);
                put_floats(out, &effect.position);
                put_floats(out, &effect.forward);
                let count = effect.order.len().min(MAX_PLAYERS);
                out.push(count as u8);
                out.extend_from_slice(&effect.order[..count]);
            }
            Message::Rammed(rammed) => {
                let item = RAM_ITEMS.iter().position(|i| *i == rammed.item);
                out.extend_from_slice(&[
                    rammed.session,
                    rammed.kart,
                    rammed.by,
                    item.unwrap_or(0) as u8,
                ]);
                put_u32(out, rammed.tick);
            }
            Message::PillowGone(gone) => {
                out.extend_from_slice(&[gone.session, gone.kart]);
                out.extend_from_slice(&gone.index.to_le_bytes());
                put_u32(out, gone.tick);
            }
            Message::Retarget(retarget) => {
                out.push(retarget.session);
                put_item_id(out, retarget.id);
                out.push(retarget.target.unwrap_or(NO_KART));
                put_u32(out, retarget.tick);
            }
            Message::Ping { sent_ms } => put_u32(out, *sent_ms),
            Message::Pong {
                echo_ms,
                clock_ms,
                room,
            } => {
                put_u32(out, *echo_ms);
                put_u32(out, *clock_ms);
                out.push(u8::from(*room));
            }
            Message::KartState(kart) => put_snapshot(out, kart),
            Message::KartStates(states) => {
                // Groups of the states of one peer, in their order: its id, how many, the states.
                let states = &states[..states.len().min(MAX_BUNDLED_STATES)];
                out.push(states.chunk_by(|a, b| a.0 == b.0).count() as u8);
                for group in states.chunk_by(|a, b| a.0 == b.0) {
                    out.extend_from_slice(&group[0].0.0.to_le_bytes());
                    out.push(group.len() as u8);
                    for (_, kart) in group {
                        put_snapshot(out, kart);
                    }
                }
            }
            Message::Relayed { origin, message } => {
                out.extend_from_slice(&origin.0.to_le_bytes());
                message.encode_body(out);
            }
        }
    }

    /// The packet, in a buffer of its own.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    /// Reads a packet. Never panics, whatever the bytes (see the module's documentation).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (&version, rest) = bytes.split_first().ok_or(DecodeError::Empty)?;
        if version != PROTOCOL_VERSION {
            return Err(DecodeError::OtherVersion(version));
        }
        Self::decode_body(rest)
    }

    /// A message from its kind on (after the version byte, or a relayed message's origin), to
    /// the end of the packet.
    fn decode_body(rest: &[u8]) -> Result<Self, DecodeError> {
        let (&kind, rest) = rest.split_first().ok_or(DecodeError::Truncated)?;
        let mut reader = Reader(rest);
        let message = match kind {
            HELLO => {
                let name = reader.name()?;
                let loadout = reader.array::<4>()?;
                let flags = reader.u8()?;
                if flags > 0b11 {
                    return Err(DecodeError::Invalid("profile flags"));
                }
                let color = KartColor::from_index(reader.u8()?)
                    .ok_or(DecodeError::Invalid("kart color"))?;
                Message::Hello(Profile {
                    name,
                    loadout,
                    ready: flags & 1 != 0,
                    seated: flags & 2 != 0,
                    color,
                })
            }
            READY => Message::Ready(reader.bool()?),
            SEATS => Message::Seats(reader.keys("more seats than players")?),
            ROOM_FULL => Message::RoomFull,
            BYE => Message::Bye,
            START_RACE => {
                let start = RaceStart {
                    race: reader.u16()?,
                    seed: u64::from_le_bytes(reader.array()?),
                    track: reader.u8()?,
                    laps: reader.u8()?,
                    bots: reader.u8()?,
                    difficulty: reader.u8()?,
                    start_tick: reader.u32()?,
                    players: reader.keys("more racers than players")?,
                };
                if start.laps == 0 {
                    return Err(DecodeError::Invalid("race without laps"));
                }
                if start.difficulty > MAX_DIFFICULTY {
                    return Err(DecodeError::Invalid("bot difficulty"));
                }
                if start.racers() > MAX_PLAYERS {
                    return Err(DecodeError::Invalid("more racers than grid slots"));
                }
                if has_duplicates(start.players.iter().map(|p| p.0)) {
                    return Err(DecodeError::Invalid("a player twice on the grid"));
                }
                Message::StartRace(start)
            }
            CIRCUIT => Message::Circuit(reader.u8()?),
            PROGRESS => {
                let progress = Progress {
                    race: reader.u16()?,
                    kart: reader.u8()?,
                    passed: reader.u16()?,
                    tick: reader.u32()?,
                };
                if usize::from(progress.kart) >= MAX_PLAYERS {
                    return Err(DecodeError::Invalid("kart"));
                }
                Message::Progress(progress)
            }
            RACE_OVER => {
                let race = reader.u16()?;
                let tick = reader.u32()?;
                let count = usize::from(reader.u8()?);
                if count > MAX_PLAYERS {
                    return Err(DecodeError::Invalid("more racers than grid slots"));
                }
                let order = reader
                    .0
                    .get(..count)
                    .ok_or(DecodeError::Truncated)?
                    .to_vec();
                reader.0 = &reader.0[count..];
                if order.iter().any(|kart| usize::from(*kart) >= MAX_PLAYERS) {
                    return Err(DecodeError::Invalid("kart"));
                }
                if has_duplicates(order.iter().map(|kart| u128::from(*kart))) {
                    return Err(DecodeError::Invalid("a racer twice in the order"));
                }
                Message::RaceOver(RaceOver { race, tick, order })
            }
            RACE_RESULTS => {
                let race = reader.u16()?;
                let count = usize::from(reader.u8()?);
                if count > MAX_PLAYERS {
                    return Err(DecodeError::Invalid("more racers than grid slots"));
                }
                let mut lines = Vec::with_capacity(count);
                for _ in 0..count {
                    let id = reader.u8()?;
                    let time = match (reader.u8()?, reader.u32()?) {
                        (0, ticks) => ResultTime::Finished(ticks),
                        (1, ticks) => ResultTime::Estimated(ticks),
                        (2, 0) => ResultTime::GaveUp,
                        _ => return Err(DecodeError::Invalid("result time")),
                    };
                    if usize::from(id) >= MAX_PLAYERS {
                        return Err(DecodeError::Invalid("kart"));
                    }
                    lines.push(ResultLine { id, time });
                }
                if has_duplicates(lines.iter().map(|line| u128::from(line.id))) {
                    return Err(DecodeError::Invalid("a racer twice in the results"));
                }
                Message::RaceResults(RaceResults { race, lines })
            }
            STRIKE => Message::Strike(Strike {
                striker: reader.u8()?,
                target: reader.u8()?,
                tick: reader.u32()?,
                seen: reader.u32()?,
                striker_position: reader.floats()?,
                direction: reader.floats()?,
                push: reader.floats::<1>()?[0],
                throw: reader.bool()?,
            }),
            STRIKE_REPLY => {
                let striker = reader.u8()?;
                let target = reader.u8()?;
                let flags = reader.u8()?;
                if flags > 0b11 {
                    return Err(DecodeError::Invalid("strike reply flags"));
                }
                Message::StrikeReply(StrikeReply {
                    striker,
                    target,
                    landed: flags & 1 != 0,
                    throw: flags & 2 != 0,
                })
            }
            GRAB => Message::Grab(Grab {
                grabber: reader.u8()?,
                target: reader.u8()?,
                tick: reader.u32()?,
                seen: reader.u32()?,
                grabber_position: reader.floats()?,
                rest: reader.floats::<1>()?[0],
            }),
            TETHER_BREAK => Message::TetherBreak(TetherBreak {
                grabber: reader.u8()?,
                target: reader.u8()?,
                reason: *TETHER_ENDS
                    .get(usize::from(reader.u8()?))
                    .ok_or(DecodeError::Invalid("tether end"))?,
            }),
            CLAIM => Message::Claim(reader.claim()?),
            VERDICT => Message::Verdict(Verdict {
                claim: reader.claim()?,
                granted: reader.bool()?,
            }),
            ITEM_OUT => {
                let session = reader.u8()?;
                let id = reader.item_id()?;
                let item = *OUT_ITEMS
                    .get(usize::from(reader.u8()?))
                    .ok_or(DecodeError::Invalid("item"))?;
                let tick = reader.u32()?;
                let position = reader.floats()?;
                let velocity = reader.floats()?;
                let motion = match reader.u8()? {
                    0 => OutMotion::Ground,
                    1 => OutMotion::Rolling {
                        bounces_left: reader.u8()?,
                        fall: reader.floats::<1>()?[0],
                    },
                    2 => {
                        let target = reader.kart_or_none()?;
                        let [distance, lateral] = reader.floats()?;
                        OutMotion::Track {
                            target,
                            distance,
                            lateral,
                        }
                    }
                    3 => OutMotion::Dive {
                        target: reader.kart_or_none()?,
                    },
                    4 => {
                        let flags = reader.u8()?;
                        if flags > 0b11 {
                            return Err(DecodeError::Invalid("bombe flags"));
                        }
                        OutMotion::Bomb {
                            grounded: flags & 1 != 0,
                            impact: flags & 2 != 0,
                            vertical: reader.floats::<1>()?[0],
                        }
                    }
                    5 => {
                        let target = reader.kart_or_none()?;
                        let flight = match reader.u8()? {
                            0 => {
                                let [distance, lateral] = reader.floats()?;
                                OutPillow::Track { distance, lateral }
                            }
                            1 => OutPillow::Hover {
                                left: reader.floats::<1>()?[0],
                            },
                            2 => OutPillow::Drop,
                            _ => return Err(DecodeError::Invalid("oreiller flight")),
                        };
                        OutMotion::Pillow { target, flight }
                    }
                    _ => return Err(DecodeError::Invalid("item motion")),
                };
                // A pelure lies on the ground, a pelote rolls, an abeille flies (or rolls,
                // thrown backwards), a bombe is a bombe, an oreiller géant an oreiller.
                let fits = match item {
                    ItemKind::Pelure => motion == OutMotion::Ground,
                    ItemKind::Pelote => matches!(motion, OutMotion::Rolling { .. }),
                    ItemKind::Bombe => matches!(motion, OutMotion::Bomb { .. }),
                    ItemKind::OreillerGeant => matches!(motion, OutMotion::Pillow { .. }),
                    _ => !matches!(
                        motion,
                        OutMotion::Ground | OutMotion::Bomb { .. } | OutMotion::Pillow { .. }
                    ),
                };
                if !fits || id.is_held() {
                    return Err(DecodeError::Invalid("item motion"));
                }
                Message::ItemOut(ItemOut {
                    session,
                    id,
                    item,
                    tick,
                    position,
                    velocity,
                    motion,
                })
            }
            ITEM_BREAK => {
                let session = reader.u8()?;
                let id = reader.item_id()?;
                let kart = reader.kart_or_none()?;
                let landed = reader.bool()?;
                if landed && kart.is_none() {
                    return Err(DecodeError::Invalid("a hit without a kart"));
                }
                Message::ItemBreak(ItemBreak {
                    session,
                    id,
                    kart,
                    landed,
                })
            }
            SCATTER => {
                let session = reader.u8()?;
                let kart = reader.kart()?;
                let loss = reader.u16()?;
                let tick = reader.u32()?;
                let from = reader.floats()?;
                let count = usize::from(reader.u8()?);
                if count == 0 || count > MAX_SCATTER {
                    return Err(DecodeError::Invalid("scattered croquettes"));
                }
                let mut to = Vec::with_capacity(count);
                for _ in 0..count {
                    to.push(reader.floats()?);
                }
                Message::Scatter(Scatter {
                    session,
                    kart,
                    loss,
                    tick,
                    from,
                    to,
                })
            }
            BLAST => {
                let session = reader.u8()?;
                let id = reader.item_id()?;
                if id.is_held() {
                    return Err(DecodeError::Invalid("a held item exploded"));
                }
                Message::Blast(BlastOut {
                    session,
                    id,
                    tick: reader.u32()?,
                    position: reader.floats()?,
                })
            }
            HONK => Message::Honk(Honk {
                session: reader.u8()?,
                kart: reader.kart()?,
                tick: reader.u32()?,
                position: reader.floats()?,
            }),
            EFFECT => {
                let session = reader.u8()?;
                let kart = reader.kart()?;
                let item = *EFFECT_ITEMS
                    .get(usize::from(reader.u8()?))
                    .ok_or(DecodeError::Invalid("effect item"))?;
                let tick = reader.u32()?;
                let position = reader.floats()?;
                let forward = reader.floats()?;
                let count = usize::from(reader.u8()?);
                if count > MAX_PLAYERS {
                    return Err(DecodeError::Invalid("more racers than grid slots"));
                }
                let mut order = Vec::with_capacity(count);
                for _ in 0..count {
                    order.push(reader.kart()?);
                }
                if has_duplicates(order.iter().map(|kart| u128::from(*kart))) {
                    return Err(DecodeError::Invalid("a racer twice in the order"));
                }
                Message::Effect(EffectOut {
                    session,
                    kart,
                    item,
                    tick,
                    position,
                    forward,
                    order,
                })
            }
            RAMMED => {
                let session = reader.u8()?;
                let kart = reader.kart()?;
                let by = reader.kart()?;
                let item = *RAM_ITEMS
                    .get(usize::from(reader.u8()?))
                    .ok_or(DecodeError::Invalid("ram item"))?;
                if kart == by {
                    return Err(DecodeError::Invalid("a kart rammed by itself"));
                }
                Message::Rammed(Rammed {
                    session,
                    kart,
                    by,
                    item,
                    tick: reader.u32()?,
                })
            }
            PILLOW_GONE => Message::PillowGone(PillowGone {
                session: reader.u8()?,
                kart: reader.kart()?,
                index: reader.u16()?,
                tick: reader.u32()?,
            }),
            RETARGET => {
                let session = reader.u8()?;
                let id = reader.item_id()?;
                if id.is_held() {
                    return Err(DecodeError::Invalid("a held item retargeted"));
                }
                Message::Retarget(Retarget {
                    session,
                    id,
                    target: reader.kart_or_none()?,
                    tick: reader.u32()?,
                })
            }
            PING => Message::Ping {
                sent_ms: reader.u32()?,
            },
            PONG => Message::Pong {
                echo_ms: reader.u32()?,
                clock_ms: reader.u32()?,
                room: reader.bool()?,
            },
            KART_STATE => Message::KartState(reader.snapshot()?),
            KART_STATES => {
                let groups = usize::from(reader.u8()?);
                if groups == 0 {
                    return Err(DecodeError::Invalid("no kart state"));
                }
                if groups > MAX_BUNDLED_STATES {
                    return Err(DecodeError::Invalid("more kart states than karts"));
                }
                let mut states: Vec<(PeerKey, KartSnapshot)> = Vec::new();
                let mut origins: Vec<PeerKey> = Vec::with_capacity(groups);
                for _ in 0..groups {
                    let origin = PeerKey(u128::from_le_bytes(reader.array()?));
                    let count = usize::from(reader.u8()?);
                    if count == 0 {
                        return Err(DecodeError::Invalid("no kart state"));
                    }
                    if states.len() + count > MAX_BUNDLED_STATES {
                        return Err(DecodeError::Invalid("more kart states than karts"));
                    }
                    // A peer's states are one group: written back the same.
                    if origins.contains(&origin) {
                        return Err(DecodeError::Invalid("a peer twice in the kart states"));
                    }
                    origins.push(origin);
                    for _ in 0..count {
                        states.push((origin, reader.snapshot()?));
                    }
                }
                Message::KartStates(states)
            }
            RELAYED => {
                let origin = PeerKey(u128::from_le_bytes(reader.array()?));
                let message = Self::decode_body(reader.0)?;
                reader.0 = &[];
                if !message.relayable() {
                    return Err(DecodeError::Invalid("relayed message"));
                }
                Message::Relayed {
                    origin,
                    message: Box::new(message),
                }
            }
            other => return Err(DecodeError::UnknownKind(other)),
        };
        if !reader.0.is_empty() {
            return Err(DecodeError::TrailingBytes);
        }
        Ok(message)
    }
}

/// Why a packet was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    /// The first byte: the peer runs another version of the game.
    OtherVersion(u8),
    UnknownKind(u8),
    /// The packet stops before the end of its message.
    Truncated,
    /// The message ends before the end of the packet.
    TrailingBytes,
    /// A value no message can have.
    Invalid(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Empty => write!(f, "empty packet"),
            DecodeError::OtherVersion(version) => {
                write!(f, "protocol version {version} (ours: {PROTOCOL_VERSION})")
            }
            DecodeError::UnknownKind(kind) => write!(f, "unknown message kind {kind}"),
            DecodeError::Truncated => write!(f, "truncated packet"),
            DecodeError::TrailingBytes => write!(f, "bytes after the message"),
            DecodeError::Invalid(what) => write!(f, "invalid {what}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A kart's state (issue #8): 47 bytes.
fn put_snapshot(out: &mut Vec<u8>, kart: &KartSnapshot) {
    put_u32(out, kart.tick);
    out.extend_from_slice(&[kart.kart, kart.session]);
    for value in kart.position {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&kart.rotation);
    for value in kart.velocity.iter().chain(&kart.spin) {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&[kart.state, kart.flags]);
    out.extend_from_slice(&kart.hit);
    out.extend_from_slice(&kart.boost);
    out.extend_from_slice(&kart.slip.to_le_bytes());
    out.push(kart.slipstream);
    out.extend_from_slice(&kart.wheels.to_le_bytes());
    out.push(kart.items);
}

/// A list of peers: a count byte, then 16 bytes each ([`MAX_PLAYERS`] at most).
fn put_keys(out: &mut Vec<u8>, keys: &[PeerKey]) {
    let count = keys.len().min(MAX_PLAYERS);
    out.push(count as u8);
    for key in &keys[..count] {
        out.extend_from_slice(&key.0.to_le_bytes());
    }
}

/// The highest bot difficulty on the wire (`bots::BotDifficulty::Hard`).
const MAX_DIFFICULTY: u8 = 2;

/// Whether a short list (12 at most) holds a value twice.
fn has_duplicates(values: impl Iterator<Item = u128>) -> bool {
    let mut seen: Vec<u128> = Vec::with_capacity(MAX_PLAYERS);
    for value in values {
        if seen.contains(&value) {
            return true;
        }
        seen.push(value);
    }
    false
}

fn put_floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// "No kart" on the wire (an abeille without a target, an item broken on another item).
const NO_KART: u8 = u8::MAX;

/// The wire number of an item that goes out (`OUT_ITEMS`).
fn out_index(item: ItemKind) -> u8 {
    OUT_ITEMS.iter().position(|i| *i == item).unwrap_or(0) as u8
}

fn put_item_id(out: &mut Vec<u8>, id: ItemId) {
    out.push(id.owner);
    out.extend_from_slice(&id.seq.to_le_bytes());
}

fn put_claim(out: &mut Vec<u8>, claim: &Claim) {
    out.push(claim.session);
    match claim.pickup {
        Pickup::Box {
            index,
            draw,
            draws,
            gap,
        } => {
            out.push(0);
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&draw.to_le_bytes());
            out.push(draws);
            put_floats(out, &[gap]);
        }
        Pickup::Croquette { index } => {
            out.push(1);
            out.extend_from_slice(&index.to_le_bytes());
        }
        Pickup::Loose(id) => {
            out.extend_from_slice(&[2, id.kart]);
            out.extend_from_slice(&id.loss.to_le_bytes());
            out.push(id.index);
        }
        Pickup::Pillow { index, draw } => {
            out.push(3);
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&draw.to_le_bytes());
        }
    }
    out.push(claim.kart);
    put_u32(out, claim.tick);
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    // A name longer than the wire allows is cut at a character boundary.
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    out.push(end as u8);
    out.extend_from_slice(&name.as_bytes()[..end]);
}

/// Reads a packet's fields, front to back; every read checks the length.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(DecodeError::Truncated)?;
        self.0 = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A list of peers ([`put_keys`]); `too_many`: what a count beyond [`MAX_PLAYERS`] is.
    fn keys(&mut self, too_many: &'static str) -> Result<Vec<PeerKey>, DecodeError> {
        let count = usize::from(self.u8()?);
        if count > MAX_PLAYERS {
            return Err(DecodeError::Invalid(too_many));
        }
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(PeerKey(u128::from_le_bytes(self.array()?)));
        }
        Ok(keys)
    }

    /// A kart of the grid.
    fn kart(&mut self) -> Result<KartId, DecodeError> {
        let kart = self.u8()?;
        if usize::from(kart) >= MAX_PLAYERS {
            return Err(DecodeError::Invalid("kart"));
        }
        Ok(kart)
    }

    /// A kart of the grid, or none ([`NO_KART`]).
    fn kart_or_none(&mut self) -> Result<Option<KartId>, DecodeError> {
        match self.u8()? {
            NO_KART => Ok(None),
            kart if usize::from(kart) < MAX_PLAYERS => Ok(Some(kart)),
            _ => Err(DecodeError::Invalid("kart")),
        }
    }

    fn item_id(&mut self) -> Result<ItemId, DecodeError> {
        Ok(ItemId {
            owner: self.kart()?,
            seq: self.u16()?,
        })
    }

    fn claim(&mut self) -> Result<Claim, DecodeError> {
        let session = self.u8()?;
        let pickup = match self.u8()? {
            0 => {
                let index = self.u16()?;
                let draw = self.u16()?;
                let draws = self.u8()?;
                let gap = self.floats::<1>()?[0];
                // A double box gives two items at most.
                if draws > 2 {
                    return Err(DecodeError::Invalid("draws"));
                }
                Pickup::Box {
                    index,
                    draw,
                    draws,
                    gap,
                }
            }
            1 => Pickup::Croquette { index: self.u16()? },
            2 => Pickup::Loose(LooseId {
                kart: self.kart()?,
                loss: self.u16()?,
                index: self.u8()?,
            }),
            3 => Pickup::Pillow {
                index: self.u16()?,
                draw: self.u16()?,
            },
            _ => return Err(DecodeError::Invalid("pickup")),
        };
        Ok(Claim {
            session,
            pickup,
            kart: self.kart()?,
            tick: self.u32()?,
        })
    }

    /// A kart's state ([`put_snapshot`]), with its impossible values refused.
    fn snapshot(&mut self) -> Result<KartSnapshot, DecodeError> {
        let kart = KartSnapshot {
            tick: self.u32()?,
            kart: self.u8()?,
            session: self.u8()?,
            position: [self.i32()?, self.i32()?, self.i32()?],
            rotation: self.array()?,
            velocity: [self.i16()?, self.i16()?, self.i16()?],
            spin: [self.i16()?, self.i16()?, self.i16()?],
            state: self.u8()?,
            flags: self.u8()?,
            hit: self.array()?,
            boost: self.array()?,
            slip: i8::from_le_bytes(self.array()?),
            slipstream: self.u8()?,
            wheels: u16::from_le_bytes(self.array()?),
            items: self.u8()?,
        };
        if kart.rotation[5] & 0x80 != 0 {
            return Err(DecodeError::Invalid("kart rotation"));
        }
        // Phase 7 does not exist; bit 7 is free.
        if kart.state & 0x07 == 0x07 || kart.state & 0x80 != 0 {
            return Err(DecodeError::Invalid("kart state"));
        }
        if kart.flags > KART_FLAGS {
            return Err(DecodeError::Invalid("kart flags"));
        }
        if !valid_held_items(kart.items) {
            return Err(DecodeError::Invalid("kart items"));
        }
        Ok(kart)
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid("boolean")),
        }
    }

    /// `N` floats, all finite: a NaN would spread through the physics.
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], DecodeError> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = f32::from_le_bytes(self.array()?);
            if !value.is_finite() {
                return Err(DecodeError::Invalid("float"));
            }
        }
        Ok(values)
    }

    fn name(&mut self) -> Result<String, DecodeError> {
        let length = usize::from(self.u8()?);
        if length > MAX_NAME_BYTES {
            return Err(DecodeError::Invalid("name length"));
        }
        let bytes = self.0.get(..length).ok_or(DecodeError::Truncated)?;
        self.0 = &self.0[length..];
        let name = std::str::from_utf8(bytes).map_err(|_| DecodeError::Invalid("name"))?;
        Ok(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kart state in a bundle (issue #63), its values on every byte.
    fn bundled_state(kart: KartId, tick: u32) -> KartSnapshot {
        KartSnapshot {
            tick,
            kart,
            session: 7,
            position: [-1_234_567, 89, i32::MAX],
            rotation: [1, 2, 3, 4, 5, 0x46],
            velocity: [-3_000, 12, i16::MAX],
            spin: [i16::MIN, 0, 77],
            state: 0x5a,
            flags: 0b1_0011,
            hit: [3, 90],
            boost: [60, 45],
            slip: -40,
            slipstream: 200,
            wheels: 0xfedc,
            items: 0b1_0110,
        }
    }

    /// One of every message, with values that use every byte of their fields.
    fn every_message() -> Vec<Message> {
        vec![
            Message::Hello(Profile {
                name: "Max".into(),
                loadout: [3, 0, 2, 1],
                ready: false,
                seated: true,
                color: KartColor::SkyBlue,
            }),
            Message::Hello(Profile {
                name: "Éloïse la très rapide"
                    .chars()
                    .take(MAX_NAME_CHARS)
                    .collect(),
                loadout: [8, 2, 2, 1],
                ready: true,
                seated: false,
                // The last color of the palette.
                color: KartColor::White,
            }),
            Message::Hello(Profile::default()),
            Message::Ready(true),
            Message::Ready(false),
            Message::Seats(vec![]),
            Message::Seats(
                (0..MAX_PLAYERS as u128)
                    .map(|i| PeerKey(u128::MAX - i * 0x0123_4567_89ab_cdef))
                    .collect(),
            ),
            Message::RoomFull,
            Message::Bye,
            Message::StartRace(RaceStart {
                race: 0xbeef,
                seed: 0xdead_beef_0123_4567,
                track: 1,
                laps: 3,
                bots: 10,
                difficulty: 2,
                start_tick: 4_000_000_007,
                players: vec![PeerKey(u128::MAX), PeerKey(0x0123_4567_89ab_cdef)],
            }),
            Message::StartRace(RaceStart {
                race: 1,
                seed: 0,
                track: 0,
                laps: 1,
                bots: 0,
                difficulty: 0,
                start_tick: 0,
                players: (0..MAX_PLAYERS as u128)
                    .map(|i| PeerKey(i << 100))
                    .collect(),
            }),
            Message::Circuit(1),
            Message::Circuit(255),
            Message::Progress(Progress {
                race: 7,
                kart: 11,
                passed: 0xfedc,
                tick: 3_600,
            }),
            Message::RaceOver(RaceOver {
                race: 0xffff,
                tick: 4_000_000_000,
                order: vec![11, 0, 5, 1, 2, 3, 4, 6, 7, 8, 9, 10],
            }),
            Message::RaceResults(RaceResults {
                race: 0xfffe,
                lines: (0..MAX_PLAYERS as u8)
                    .rev()
                    .map(|kart| ResultLine {
                        id: kart,
                        time: match kart % 3 {
                            0 => ResultTime::Finished(4_000_000_000 + u32::from(kart)),
                            1 => ResultTime::Estimated(u32::MAX - u32::from(kart)),
                            _ => ResultTime::GaveUp,
                        },
                    })
                    .collect(),
            }),
            Message::RaceResults(RaceResults {
                race: 3,
                lines: vec![],
            }),
            Message::RaceOver(RaceOver {
                race: 2,
                tick: 1,
                order: vec![],
            }),
            Message::Strike(Strike {
                striker: 0,
                target: 11,
                tick: 4_000_000_000,
                seen: 3_999_999_994,
                striker_position: [-12.5, 0.75, 301.25],
                direction: [0.6, -0.8],
                push: 9.1,
                throw: true,
            }),
            Message::StrikeReply(StrikeReply {
                striker: 5,
                target: 7,
                landed: true,
                throw: false,
            }),
            Message::StrikeReply(StrikeReply {
                striker: 7,
                target: 5,
                landed: false,
                throw: true,
            }),
            Message::Grab(Grab {
                grabber: 2,
                target: 3,
                tick: 600,
                seen: 594,
                grabber_position: [1.0, 2.0, -3.0],
                rest: 2.75,
            }),
            Message::Ping { sent_ms: u32::MAX },
            Message::Pong {
                echo_ms: 17,
                clock_ms: 1_000_000,
                room: true,
            },
            Message::Pong {
                echo_ms: u32::MAX,
                clock_ms: 0,
                room: false,
            },
            Message::KartState(KartSnapshot::default()),
            Message::KartState(KartSnapshot {
                tick: 4_000_000_123,
                kart: 11,
                session: 255,
                position: [i32::MIN, -1, i32::MAX],
                rotation: [0xff, 0x00, 0x12, 0x34, 0x56, 0x7f],
                velocity: [i16::MIN, 0, i16::MAX],
                spin: [-1, 1_234, -32_000],
                state: 0b0111_1110,
                flags: 0b11,
                hit: [17, 96],
                boost: [60, 255],
                slip: -128,
                slipstream: 255,
                wheels: 0xfedc,
                items: 0b1_0110,
            }),
            Message::KartState(KartSnapshot {
                items: 0b1111,
                ..KartSnapshot::default()
            }),
            // Issue #53: the claims, the verdicts, the items and the croquettes.
            Message::Claim(Claim {
                session: 3,
                pickup: Pickup::Box {
                    index: 0xfedc,
                    draw: 0x1234,
                    draws: 2,
                    gap: 132.5,
                },
                kart: 11,
                tick: 4_000_000_001,
            }),
            Message::Claim(Claim {
                session: 0,
                pickup: Pickup::Croquette { index: 21 },
                kart: 0,
                tick: 0,
            }),
            Message::Verdict(Verdict {
                claim: Claim {
                    session: 255,
                    pickup: Pickup::Loose(LooseId {
                        kart: 7,
                        loss: 0xabcd,
                        index: 2,
                    }),
                    kart: 3,
                    tick: 61,
                },
                granted: true,
            }),
            Message::Verdict(Verdict {
                claim: Claim {
                    session: 1,
                    pickup: Pickup::Box {
                        index: 2,
                        draw: 0,
                        draws: 0,
                        gap: 0.0,
                    },
                    kart: 5,
                    tick: 900,
                },
                granted: false,
            }),
            Message::ItemOut(ItemOut {
                session: 2,
                id: ItemId {
                    owner: 11,
                    seq: 0xfffe,
                },
                item: ItemKind::Pelure,
                tick: 4_000_000_002,
                position: [-120.5, 0.45, 301.25],
                velocity: [0.0; 3],
                motion: OutMotion::Ground,
            }),
            Message::ItemOut(ItemOut {
                session: 2,
                id: ItemId { owner: 0, seq: 1 },
                item: ItemKind::Pelote,
                tick: 600,
                position: [1.0, 2.0, -3.0],
                velocity: [42.0, 0.0, -0.5],
                motion: OutMotion::Rolling {
                    bounces_left: 5,
                    fall: 2.5,
                },
            }),
            Message::ItemOut(ItemOut {
                session: 9,
                id: ItemId { owner: 4, seq: 77 },
                item: ItemKind::Abeille,
                tick: 601,
                position: [10.0, 1.4, 5.0],
                velocity: [0.0, 0.0, -50.0],
                motion: OutMotion::Track {
                    target: Some(3),
                    distance: 812.25,
                    lateral: -1.5,
                },
            }),
            Message::ItemOut(ItemOut {
                session: 9,
                id: ItemId { owner: 4, seq: 78 },
                item: ItemKind::Abeille,
                tick: 602,
                position: [10.0, 1.4, 5.0],
                velocity: [3.0, -1.0, -45.0],
                motion: OutMotion::Dive { target: None },
            }),
            Message::ItemOut(ItemOut {
                session: 9,
                id: ItemId { owner: 4, seq: 79 },
                item: ItemKind::Abeille,
                tick: 603,
                position: [10.0, 1.4, 5.0],
                velocity: [0.0, 0.0, 50.0],
                motion: OutMotion::Rolling {
                    bounces_left: 0,
                    fall: 0.0,
                },
            }),
            Message::ItemBreak(ItemBreak {
                session: 4,
                id: ItemId { owner: 2, seq: 9 },
                kart: Some(11),
                landed: true,
            }),
            Message::ItemBreak(ItemBreak {
                session: 4,
                id: ItemId::held(6),
                kart: None,
                landed: false,
            }),
            Message::Scatter(Scatter {
                session: 4,
                kart: 10,
                loss: 0xffff,
                tick: 4_000_000_003,
                from: [1.5, 0.85, -2.0],
                to: vec![[3.0, 0.4, -4.0], [1.0, 0.41, -4.5], [-0.5, 0.42, -3.0]],
            }),
            Message::Scatter(Scatter {
                session: 0,
                kart: 0,
                loss: 0,
                tick: 0,
                from: [0.0; 3],
                to: vec![[1.0; 3]; MAX_SCATTER],
            }),
            // Issue #18: a bombe lobbed ahead, one dropped on the ground; an explosion, a klaxon.
            Message::ItemOut(ItemOut {
                session: 3,
                id: ItemId { owner: 7, seq: 2 },
                item: ItemKind::Bombe,
                tick: 77,
                position: [1.0, 1.35, -2.0],
                velocity: [0.0, 0.0, -48.0],
                motion: OutMotion::Bomb {
                    vertical: 7.0,
                    grounded: false,
                    impact: true,
                },
            }),
            Message::ItemOut(ItemOut {
                session: 3,
                id: ItemId { owner: 7, seq: 3 },
                item: ItemKind::Bombe,
                tick: 78,
                position: [1.0, 0.55, 2.0],
                velocity: [0.0; 3],
                motion: OutMotion::Bomb {
                    vertical: 0.0,
                    grounded: true,
                    impact: false,
                },
            }),
            Message::Blast(BlastOut {
                session: 3,
                id: ItemId { owner: 7, seq: 2 },
                tick: 120,
                position: [1.0, 0.55, -18.0],
            }),
            Message::Honk(Honk {
                session: 3,
                kart: 11,
                tick: 4_000_000_000,
                position: [-5.0, 0.55, 30.0],
            }),
            // Issue #61: a kart in a super câlin, a fusée and deflated; an oreiller géant claimed,
            // thrown (in each part of its flight), after another kart, gone; a chantilly and a
            // dégonflage told with their ranking; karts knocked over by a super câlin, a fusée.
            Message::KartState(KartSnapshot {
                flags: KART_FLAGS,
                ..KartSnapshot::default()
            }),
            Message::Claim(Claim {
                session: 7,
                pickup: Pickup::Pillow {
                    index: 0xfedc,
                    draw: 0xba98,
                },
                kart: 11,
                tick: 4_000_000_004,
            }),
            Message::Verdict(Verdict {
                claim: Claim {
                    session: 7,
                    pickup: Pickup::Pillow { index: 3, draw: 1 },
                    kart: 0,
                    tick: 1_300,
                },
                granted: false,
            }),
            Message::ItemOut(ItemOut {
                session: 7,
                id: ItemId { owner: 5, seq: 4 },
                item: ItemKind::OreillerGeant,
                tick: 1_400,
                position: [12.0, 4.5, -30.0],
                velocity: [0.0, 0.0, -60.0],
                motion: OutMotion::Pillow {
                    target: Some(2),
                    flight: OutPillow::Track {
                        distance: 501.5,
                        lateral: 2.25,
                    },
                },
            }),
            Message::ItemOut(ItemOut {
                session: 7,
                id: ItemId { owner: 5, seq: 5 },
                item: ItemKind::OreillerGeant,
                tick: 1_401,
                position: [12.0, 4.5, -30.0],
                velocity: [1.0, 0.0, -2.0],
                motion: OutMotion::Pillow {
                    target: None,
                    flight: OutPillow::Hover {
                        left: OutPillow::WAITING,
                    },
                },
            }),
            Message::ItemOut(ItemOut {
                session: 7,
                id: ItemId { owner: 5, seq: 6 },
                item: ItemKind::OreillerGeant,
                tick: 1_402,
                position: [12.0, 2.5, -30.0],
                velocity: [0.0, -20.0, 0.0],
                motion: OutMotion::Pillow {
                    target: Some(11),
                    flight: OutPillow::Drop,
                },
            }),
            Message::Retarget(Retarget {
                session: 7,
                id: ItemId { owner: 5, seq: 4 },
                target: Some(9),
                tick: 1_450,
            }),
            Message::Retarget(Retarget {
                session: 7,
                id: ItemId { owner: 0, seq: 0 },
                target: None,
                tick: 0,
            }),
            Message::PillowGone(PillowGone {
                session: 7,
                kart: 11,
                index: 0xfedc,
                tick: 4_000_000_004,
            }),
            Message::Effect(EffectOut {
                session: 7,
                kart: 6,
                item: ItemKind::Chantilly,
                tick: 4_000_000_005,
                position: [-3.5, 0.55, 120.25],
                forward: [0.6, -0.8],
                order: vec![11, 0, 5, 1, 2, 3, 4, 6, 7, 8, 9, 10],
            }),
            Message::Effect(EffectOut {
                session: 0,
                kart: 0,
                item: ItemKind::Degonflage,
                tick: 0,
                position: [0.0; 3],
                forward: [0.0, -1.0],
                order: vec![],
            }),
            Message::Rammed(Rammed {
                session: 7,
                kart: 11,
                by: 0,
                item: ItemKind::SuperCalin,
                tick: 4_000_000_006,
            }),
            Message::Rammed(Rammed {
                session: 1,
                kart: 0,
                by: 11,
                item: ItemKind::Fusee,
                tick: 7,
            }),
            // Issue #63: what the host relays, and its bundles of kart states.
            Message::Relayed {
                origin: PeerKey(u128::MAX - 7),
                message: Box::new(Message::Hello(Profile {
                    name: "Cyd".into(),
                    loadout: [2, 1, 0, 2],
                    ready: true,
                    seated: true,
                    color: KartColor::Red,
                })),
            },
            Message::Relayed {
                origin: PeerKey(0x0123_4567_89ab_cdef << 64),
                message: Box::new(Message::Bye),
            },
            Message::Relayed {
                origin: PeerKey(1),
                message: Box::new(Message::Strike(Strike {
                    striker: 10,
                    target: 11,
                    tick: 1_234,
                    seen: 1_228,
                    striker_position: [1.0, 0.5, -3.25],
                    direction: [0.0, 1.0],
                    push: 7.5,
                    throw: false,
                })),
            },
            Message::KartStates(vec![(PeerKey(u128::MAX), bundled_state(3, 9))]),
            Message::KartStates(
                (0..MAX_BUNDLED_STATES)
                    .map(|i| {
                        // The host's kart and its bots (one group), then one client per kart.
                        let origin = PeerKey(if i < 8 { 5 << 100 } else { (i as u128) << 64 });
                        (origin, bundled_state(i as u8, 4_000_000_000 + i as u32))
                    })
                    .collect(),
            ),
        ]
        .into_iter()
        .chain(TETHER_ENDS.iter().map(|reason| {
            Message::TetherBreak(TetherBreak {
                grabber: 9,
                target: 4,
                reason: *reason,
            })
        }))
        .collect()
    }

    /// Issue #7 acceptance: every message comes back the same from its bytes.
    #[test]
    fn every_message_survives_a_round_trip() {
        let mut kinds = Vec::new();
        let mut buffer = Vec::new();
        for message in every_message() {
            message.encode(&mut buffer);
            assert_eq!(buffer[0], PROTOCOL_VERSION);
            assert_eq!(buffer[1], message.kind());
            assert_eq!(Message::decode(&buffer), Ok(message.clone()), "{buffer:?}");
            assert_eq!(message.to_bytes(), buffer);
            kinds.push(message.kind());
        }
        // Every kind is tested, and no two messages share one.
        kinds.sort_unstable();
        kinds.dedup();
        // 13 until issue #8; issue #9 adds the circuit, the checkpoints and the end of the race;
        // issue #53 the claims, the verdicts, the items out, their breaks and the scattered
        // croquettes; issue #18 the explosions and the klaxons; issue #61 the effects, the karts
        // knocked over, the oreillers gone and retargeted; issue #63 what the host relays and its
        // bundles of kart states; issue #70 the closed results.
        assert_eq!(kinds.len(), 30);
        assert_eq!(
            TETHER_ENDS.len(),
            10,
            "a TetherEnd was added: give it a wire byte"
        );
    }

    /// The pings and the kart states (issue #8; issue #63: bundled by the host too) go on the
    /// unreliable channel, everything else on the reliable one (a relayed message on its own's).
    #[test]
    fn only_the_pings_and_the_kart_states_are_unreliable() {
        for message in every_message() {
            let unreliable = matches!(
                message,
                Message::Ping { .. }
                    | Message::Pong { .. }
                    | Message::KartState(_)
                    | Message::KartStates(_)
            );
            assert_eq!(
                message.channel() == Channel::Unreliable,
                unreliable,
                "{message:?}"
            );
        }
        assert_eq!(Channel::Reliable.index(), 0);
        assert_eq!(Channel::Unreliable.index(), 1);
    }

    /// Compact: the lobby fits in a few dozen bytes, a ping in 6.
    #[test]
    fn messages_are_compact() {
        assert_eq!(Message::Ping { sent_ms: 1 }.to_bytes().len(), 6);
        assert_eq!(Message::Ready(true).to_bytes().len(), 3);
        let hello = Message::Hello(Profile {
            name: "Max".into(),
            ..Default::default()
        });
        // Issue #55: one more byte, the color.
        assert_eq!(hello.to_bytes().len(), 2 + 1 + 3 + 4 + 1 + 1);
        let seats = Message::Seats(vec![PeerKey(1); MAX_PLAYERS]);
        assert_eq!(seats.to_bytes().len(), 3 + 16 * MAX_PLAYERS);
        // Issue #8: a kart state in fewer than 64 bytes, whatever its values (47, then 48 with
        // the session of issue #9, 49 with the items held out of issue #53).
        for message in every_message() {
            if let Message::KartState(_) = message {
                let length = message.to_bytes().len();
                assert_eq!(length, 49);
                assert!(length < MAX_KART_STATE_BYTES);
            }
        }
        // Issue #9: a checkpoint in 11 bytes, a pong in 11.
        let progress = Message::Progress(Progress {
            race: 1,
            kart: 3,
            passed: 12,
            tick: 900,
        });
        assert_eq!(progress.to_bytes().len(), 11);
        let pong = Message::Pong {
            echo_ms: 1,
            clock_ms: 2,
            room: true,
        };
        assert_eq!(pong.to_bytes().len(), 11);
        // Issue #53: a claim on a box in 18 bytes, on a croquette in 11; a verdict one more; an
        // item thrown in 36 to 45; its break in 8; a croquette scattered in 23 + 12 each.
        let claim = Claim {
            session: 1,
            pickup: Pickup::Box {
                index: 1,
                draw: 2,
                draws: 1,
                gap: 3.0,
            },
            kart: 4,
            tick: 5,
        };
        assert_eq!(Message::Claim(claim).to_bytes().len(), 18);
        let verdict = Verdict {
            claim: Claim {
                pickup: Pickup::Croquette { index: 3 },
                ..claim
            },
            granted: true,
        };
        assert_eq!(Message::Claim(verdict.claim).to_bytes().len(), 11);
        assert_eq!(Message::Verdict(verdict).to_bytes().len(), 12);
        // Issue #61: a claim on an oreiller géant in 13 bytes.
        let pillow = Claim {
            pickup: Pickup::Pillow { index: 3, draw: 4 },
            ..claim
        };
        assert_eq!(Message::Claim(pillow).to_bytes().len(), 13);
        for message in every_message() {
            let length = message.to_bytes().len();
            match &message {
                // Issue #61: 46 for an oreiller géant on its way.
                Message::ItemOut(_) => assert!((36..=46).contains(&length), "{message:?}"),
                Message::ItemBreak(_) => assert_eq!(length, 8),
                Message::Scatter(scatter) => assert_eq!(length, 23 + 12 * scatter.to.len()),
                // Issue #18: an explosion in 22 bytes, a klaxon in 20.
                Message::Blast(_) => assert_eq!(length, 22),
                Message::Honk(_) => assert_eq!(length, 20),
                // Issue #61: an effect in 30 bytes and one per racer of its ranking (42 at most);
                // a kart knocked over in 10, an oreiller gone in 10, retargeted in 11.
                Message::Effect(effect) => assert_eq!(length, 30 + effect.order.len()),
                Message::Rammed(_) | Message::PillowGone(_) => assert_eq!(length, 10),
                Message::Retarget(_) => assert_eq!(length, 11),
                _ => {}
            }
        }
        // Issue #63: a relayed message is its own (without its version byte) after 2 bytes and
        // its origin's 16; a bundle of kart states, 3 bytes, then 17 per peer and 47 per state:
        // at most 771 bytes for 12 karts of 12 peers (one packet, well under 1 200 bytes).
        for message in every_message() {
            let length = message.to_bytes().len();
            match &message {
                Message::Relayed { message: inner, .. } => {
                    assert_eq!(length, 2 + 16 + inner.to_bytes().len() - 1, "{message:?}");
                }
                Message::KartStates(states) => {
                    let peers = states.chunk_by(|a, b| a.0 == b.0).count();
                    assert_eq!(length, 3 + 17 * peers + 47 * states.len());
                }
                _ => {}
            }
        }
        let twelve = Message::KartStates(
            (0..MAX_BUNDLED_STATES as u128)
                .map(|i| (PeerKey(i), KartSnapshot::default()))
                .collect(),
        );
        assert_eq!(twelve.to_bytes().len(), 771);
        for message in every_message() {
            match &message {
                // Issue #9: a race's start lists its players (16 bytes each): 213 bytes with 12.
                Message::StartRace(start) => {
                    assert_eq!(message.to_bytes().len(), 21 + 16 * start.players.len());
                }
                Message::KartStates(_) => {}
                _ => assert!(message.to_bytes().len() <= 200, "{message:?}"),
            }
        }
    }

    /// Every packet cut short is refused, never misread, never a panic.
    #[test]
    fn a_truncated_packet_is_refused() {
        for message in every_message() {
            let bytes = message.to_bytes();
            for end in 0..bytes.len() {
                let expected = if end == 0 {
                    DecodeError::Empty
                } else {
                    DecodeError::Truncated
                };
                assert_eq!(
                    Message::decode(&bytes[..end]),
                    Err(expected),
                    "{message:?} cut at {end}"
                );
            }
        }
    }

    #[test]
    fn bytes_after_the_message_are_refused() {
        for message in every_message() {
            let mut bytes = message.to_bytes();
            bytes.push(0);
            assert_eq!(Message::decode(&bytes), Err(DecodeError::TrailingBytes));
        }
    }

    #[test]
    fn another_version_or_an_unknown_kind_is_refused() {
        let mut bytes = Message::Bye.to_bytes();
        for version in [0, PROTOCOL_VERSION + 1, 255] {
            bytes[0] = version;
            assert_eq!(
                Message::decode(&bytes),
                Err(DecodeError::OtherVersion(version))
            );
        }
        // A future version is recognised from its first byte alone.
        assert_eq!(
            Message::decode(&[PROTOCOL_VERSION + 1]),
            Err(DecodeError::OtherVersion(PROTOCOL_VERSION + 1))
        );
        let known: Vec<u8> = every_message().iter().map(Message::kind).collect();
        for kind in 0..=255u8 {
            if !known.contains(&kind) {
                assert_eq!(
                    Message::decode(&[PROTOCOL_VERSION, kind]),
                    Err(DecodeError::UnknownKind(kind))
                );
            }
        }
        assert!(DecodeError::OtherVersion(9).to_string().contains('9'));
    }

    #[test]
    fn impossible_values_are_refused() {
        let v = PROTOCOL_VERSION;
        let invalid = |bytes: &[u8]| matches!(Message::decode(bytes), Err(DecodeError::Invalid(_)));
        // A boolean of 2.
        assert!(invalid(&[v, READY, 2]));
        // Unknown profile flags.
        assert!(invalid(&[v, HELLO, 0, 0, 0, 0, 0, 0b100]));
        // Issue #55: a color outside the palette.
        assert!(Message::decode(&[v, HELLO, 0, 0, 0, 0, 0, 0, 11]).is_ok());
        for color in [12, 13, 0x7f, 0xff] {
            assert!(invalid(&[v, HELLO, 0, 0, 0, 0, 0, 0, color]), "{color}");
        }
        assert!(invalid(&[v, STRIKE_REPLY, 0, 0, 0b100]));
        // A name too long, a name that is not UTF-8.
        let mut long = vec![v, HELLO, MAX_NAME_BYTES as u8 + 1];
        long.extend(std::iter::repeat_n(b'a', MAX_NAME_BYTES + 1 + 5));
        assert!(invalid(&long));
        assert!(invalid(&[v, HELLO, 2, 0xc3, 0x28, 0, 0, 0, 0, 0]));
        // More seats than players.
        let mut seats = vec![v, SEATS, MAX_PLAYERS as u8 + 1];
        seats.extend(std::iter::repeat_n(0, 16 * (MAX_PLAYERS + 1)));
        assert!(invalid(&seats));
        // An unknown reason to end a tether.
        assert!(invalid(&[v, TETHER_BREAK, 0, 1, TETHER_ENDS.len() as u8]));
        // A kart state: phase 7, the free bits of the state, the flags and the rotation.
        let kart = Message::KartState(KartSnapshot::default()).to_bytes();
        let (rotation, state, flags) = (
            2 + 4 + 2 + 12 + 5,
            2 + 4 + 2 + 12 + 6 + 12,
            2 + 4 + 2 + 12 + 6 + 12 + 1,
        );
        // Issue #61: bits 2 to 4 of the flags are the super câlin, the fusée and the dégonflage;
        // 5 to 7 stay free.
        for (at, value) in [
            (state, 0x07),
            (state, 0x80),
            (flags, 0b10_0000),
            (flags, 0b10_0100),
            (flags, 0x80),
            (rotation, 0x80),
        ] {
            let mut bad = kart.clone();
            bad[at] = value;
            assert!(invalid(&bad), "byte {at} = {value:#x}");
        }
        assert!(Message::decode(&kart).is_ok());
        for value in [0b100, 0b1000, 0b1_0000, KART_FLAGS] {
            let mut good = kart.clone();
            good[flags] = value;
            assert!(Message::decode(&good).is_ok(), "flags {value:#b}");
        }
        // Issue #9: a race without laps, with a bot difficulty beyond Hard, with more racers than
        // the grid has slots, with a player twice; a checkpoint of a kart beyond the grid; a
        // final order with a kart beyond the grid, or twice; a pong's flag of 2.
        let start = RaceStart {
            race: 1,
            seed: 2,
            track: 0,
            laps: 3,
            bots: 10,
            difficulty: 1,
            start_tick: 600,
            players: vec![PeerKey(1), PeerKey(2)],
        };
        assert!(Message::decode(&Message::StartRace(start.clone()).to_bytes()).is_ok());
        for bad in [
            RaceStart {
                laps: 0,
                ..start.clone()
            },
            RaceStart {
                difficulty: 3,
                ..start.clone()
            },
            RaceStart {
                bots: 11,
                ..start.clone()
            },
            RaceStart {
                players: vec![PeerKey(1), PeerKey(1)],
                ..start.clone()
            },
        ] {
            assert!(invalid(&Message::StartRace(bad).to_bytes()));
        }
        let mut too_many = Message::StartRace(start.clone()).to_bytes();
        let count_at = too_many.len() - 1 - 2 * 16;
        too_many[count_at] = MAX_PLAYERS as u8 + 1;
        too_many.extend(std::iter::repeat_n(0, 16 * (MAX_PLAYERS - 1)));
        assert!(invalid(&too_many));
        assert!(invalid(&[v, PROGRESS, 1, 0, 12, 0, 0, 0, 0, 0, 0]));
        assert!(invalid(&[v, RACE_OVER, 1, 0, 0, 0, 0, 0, 2, 3, 12]));
        assert!(invalid(&[v, RACE_OVER, 1, 0, 0, 0, 0, 0, 2, 3, 3]));
        assert!(invalid(&[v, RACE_OVER, 1, 0, 0, 0, 0, 0, 13]));
        // Issue #70: closed results with a kart beyond the grid, a kart twice, an unknown kind of
        // time, a time for a racer that gave up, more lines than grid slots.
        let line = |kart: u8, kind: u8, tick: u8| [kart, kind, tick, 0, 0, 0];
        let results = |lines: &[[u8; 6]]| {
            let mut bytes = vec![v, RACE_RESULTS, 1, 0, lines.len() as u8];
            bytes.extend(lines.iter().flatten());
            bytes
        };
        assert!(Message::decode(&results(&[line(3, 0, 9), line(4, 1, 9), line(5, 2, 0)])).is_ok());
        assert!(invalid(&results(&[line(12, 0, 9)])));
        assert!(invalid(&results(&[line(3, 0, 9), line(3, 1, 9)])));
        assert!(invalid(&results(&[line(3, 3, 9)])));
        assert!(invalid(&results(&[line(3, 2, 1)])));
        assert!(invalid(&results(&[line(0, 0, 0); MAX_PLAYERS + 1])));
        assert!(invalid(&[v, PONG, 0, 0, 0, 0, 0, 0, 0, 0, 2]));
        // Issue #53: a kart holding out impossible items; a claim on an unknown pickup, with
        // three draws, of a kart beyond the grid; a verdict's flag of 2; an item thrown that is
        // not an object, held, or that moves as it cannot; a hit without a kart; a scatter of
        // nothing or of too many croquettes.
        let items_at = kart.len() - 1;
        for value in [0b100, 0b0001, 0b1_1001, 0b1_1011, 0b10_0101, 0b1000_0101] {
            let mut bad = kart.clone();
            bad[items_at] = value;
            assert!(invalid(&bad), "items {value:#b}");
        }
        for value in [0b0101, 0b1101, 0b1_0101, 0b1_0110] {
            let mut good = kart.clone();
            good[items_at] = value;
            assert!(Message::decode(&good).is_ok(), "items {value:#b}");
        }
        let claim = Claim {
            session: 1,
            pickup: Pickup::Box {
                index: 1,
                draw: 2,
                draws: 2,
                gap: 3.0,
            },
            kart: 4,
            tick: 5,
        };
        let bytes = Message::Claim(claim).to_bytes();
        assert!(Message::decode(&bytes).is_ok());
        let mut bad = bytes.clone();
        // Issue #61: 3 is an oreiller géant now.
        bad[3] = 4;
        assert!(invalid(&bad), "unknown pickup");
        let mut bad = bytes.clone();
        bad[8] = 3;
        assert!(invalid(&bad), "three draws");
        let mut bad = bytes.clone();
        bad[13] = MAX_PLAYERS as u8;
        assert!(invalid(&bad), "kart beyond the grid");
        let mut verdict = Message::Verdict(Verdict {
            claim,
            granted: true,
        })
        .to_bytes();
        *verdict.last_mut().unwrap() = 2;
        assert!(invalid(&verdict));
        let out = ItemOut {
            session: 1,
            id: ItemId { owner: 2, seq: 3 },
            item: ItemKind::Pelote,
            tick: 4,
            position: [0.0; 3],
            velocity: [42.0, 0.0, 0.0],
            motion: OutMotion::Rolling {
                bounces_left: 5,
                fall: 0.0,
            },
        };
        let bytes = Message::ItemOut(out).to_bytes();
        assert!(Message::decode(&bytes).is_ok());
        // Not an object (beyond the oreiller géant, issue #61), a kart beyond the grid, the
        // number of what a kart holds out, an unknown motion, a pelote on the ground, a bombe
        // that rolls, an oreiller géant that rolls.
        for changes in [
            &[(6, 5)][..],
            &[(6, 255)],
            &[(3, MAX_PLAYERS as u8)],
            &[(4, 0xff), (5, 0xff)],
            &[(35, 6)],
            &[(35, 0)],
            &[(6, 3)],
            &[(6, 4)],
        ] {
            let mut bad = bytes.clone();
            for (at, value) in changes {
                bad[*at] = *value;
            }
            assert!(invalid(&bad), "item out: {changes:?}");
        }
        let bomb = OutMotion::Bomb {
            vertical: 0.0,
            grounded: true,
            impact: false,
        };
        let pillow = OutMotion::Pillow {
            target: Some(1),
            flight: OutPillow::Hover { left: 0.5 },
        };
        for (item, motion) in [
            (ItemKind::Pelure, OutMotion::Dive { target: None }),
            (ItemKind::Abeille, OutMotion::Ground),
            (ItemKind::Pelote, OutMotion::Dive { target: Some(1) }),
            // Issue #18: a bombe moves as a bombe, and only it.
            (ItemKind::Pelote, bomb),
            (ItemKind::Abeille, bomb),
            (ItemKind::Bombe, OutMotion::Ground),
            // Issue #61: an oreiller géant flies as an oreiller, and only it.
            (ItemKind::OreillerGeant, bomb),
            (ItemKind::OreillerGeant, OutMotion::Dive { target: None }),
            (ItemKind::Abeille, pillow),
            (ItemKind::Bombe, pillow),
            (ItemKind::Pelure, pillow),
        ] {
            let bad = Message::ItemOut(ItemOut {
                item,
                motion,
                ..out
            })
            .to_bytes();
            assert!(invalid(&bad), "{item:?} {motion:?}");
        }
        // Issue #18: the bombe's flags are 2 bits; a held item never explodes; a klaxon of a kart
        // beyond the grid.
        let mut bad = Message::ItemOut(ItemOut {
            item: ItemKind::Bombe,
            motion: bomb,
            ..out
        })
        .to_bytes();
        assert!(Message::decode(&bad).is_ok());
        bad[36] = 4;
        assert!(invalid(&bad), "bombe flags");
        let blast = Message::Blast(BlastOut {
            session: 1,
            id: ItemId::held(2),
            tick: 3,
            position: [0.0; 3],
        })
        .to_bytes();
        assert!(invalid(&blast), "a held item exploded");
        let mut honk = Message::Honk(Honk {
            session: 1,
            kart: 2,
            tick: 3,
            position: [0.0; 3],
        })
        .to_bytes();
        assert!(Message::decode(&honk).is_ok());
        honk[3] = MAX_PLAYERS as u8;
        assert!(invalid(&honk), "a klaxon beyond the grid");
        // Issue #61: an oreiller géant in an unknown part of its flight, after a kart beyond the
        // grid; retargeted while held, or at a kart beyond the grid; gone from a kart beyond the
        // grid.
        let thrown = Message::ItemOut(ItemOut {
            item: ItemKind::OreillerGeant,
            motion: pillow,
            ..out
        })
        .to_bytes();
        assert!(Message::decode(&thrown).is_ok());
        for (at, value) in [(37, 3), (37, 0xff), (36, MAX_PLAYERS as u8)] {
            let mut bad = thrown.clone();
            bad[at] = value;
            assert!(invalid(&bad), "oreiller: byte {at} = {value}");
        }
        let retarget = Retarget {
            session: 1,
            id: ItemId { owner: 2, seq: 3 },
            target: Some(4),
            tick: 5,
        };
        let bytes = Message::Retarget(retarget).to_bytes();
        assert!(Message::decode(&bytes).is_ok());
        let held = Message::Retarget(Retarget {
            id: ItemId::held(2),
            ..retarget
        })
        .to_bytes();
        assert!(invalid(&held), "a held item retargeted");
        let mut bad = bytes.clone();
        bad[6] = MAX_PLAYERS as u8;
        assert!(invalid(&bad), "a target beyond the grid");
        let mut gone = Message::PillowGone(PillowGone {
            session: 1,
            kart: 2,
            index: 3,
            tick: 4,
        })
        .to_bytes();
        assert!(Message::decode(&gone).is_ok());
        gone[3] = MAX_PLAYERS as u8;
        assert!(
            invalid(&gone),
            "an oreiller gone from a kart beyond the grid"
        );
        // Issue #61: an effect of an item that is not the chantilly nor the dégonflage, of a kart
        // beyond the grid, with a racer beyond the grid, twice, or more racers than the grid.
        let effect = EffectOut {
            session: 1,
            kart: 2,
            item: ItemKind::Degonflage,
            tick: 3,
            position: [0.0; 3],
            forward: [0.0, -1.0],
            order: vec![4, 2, 0],
        };
        let bytes = Message::Effect(effect.clone()).to_bytes();
        assert!(Message::decode(&bytes).is_ok());
        let count_at = bytes.len() - 4;
        for (at, value) in [
            (4, 2),
            (4, 0xff),
            (3, MAX_PLAYERS as u8),
            (count_at + 1, MAX_PLAYERS as u8),
            (count_at + 2, 4),
        ] {
            let mut bad = bytes.clone();
            bad[at] = value;
            assert!(invalid(&bad), "effect: byte {at} = {value}");
        }
        let mut too_many = bytes.clone();
        too_many[count_at] = MAX_PLAYERS as u8 + 1;
        too_many.truncate(count_at + 1);
        too_many.extend(0..=MAX_PLAYERS as u8);
        assert!(invalid(&too_many), "more racers than the grid");
        // A kart knocked over by an item that does not ram, by itself, beyond the grid.
        let rammed = Message::Rammed(Rammed {
            session: 1,
            kart: 2,
            by: 3,
            item: ItemKind::Fusee,
            tick: 4,
        })
        .to_bytes();
        assert!(Message::decode(&rammed).is_ok());
        for (at, value) in [(5, 2), (5, 0xff), (4, 2), (3, MAX_PLAYERS as u8), (4, 0xff)] {
            let mut bad = rammed.clone();
            bad[at] = value;
            assert!(invalid(&bad), "rammed: byte {at} = {value}");
        }
        let hit = Message::ItemBreak(ItemBreak {
            session: 1,
            id: ItemId { owner: 2, seq: 3 },
            kart: Some(4),
            landed: true,
        })
        .to_bytes();
        let mut bad = hit.clone();
        bad[6] = NO_KART;
        assert!(invalid(&bad), "a hit without a kart");
        let mut bad = hit.clone();
        bad[6] = MAX_PLAYERS as u8;
        assert!(invalid(&bad), "a kart beyond the grid");
        let scatter = Message::Scatter(Scatter {
            session: 1,
            kart: 2,
            loss: 3,
            tick: 4,
            from: [0.0; 3],
            to: vec![[1.0; 3]],
        })
        .to_bytes();
        let count_at = scatter.len() - 13;
        for count in [0, MAX_SCATTER as u8 + 1] {
            let mut bad = scatter.clone();
            bad[count_at] = count;
            bad.extend(std::iter::repeat_n(0, 12 * usize::from(count.max(1) - 1)));
            assert!(invalid(&bad), "{count} croquettes");
        }
        // A float that is not finite.
        let mut grab = Message::Grab(Grab {
            grabber: 0,
            target: 1,
            tick: 0,
            seen: 0,
            grabber_position: [0.0; 3],
            rest: 1.0,
        })
        .to_bytes();
        let rest = grab.len() - 4;
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            grab[rest..].copy_from_slice(&bad.to_le_bytes());
            assert!(invalid(&grab));
        }
        // Issue #63: the host relays only what a client tells everyone: not a ping, a kart state,
        // the host's own messages, nor a relayed message again.
        let relay = |message: Message| {
            let mut bytes = vec![v, RELAYED];
            bytes.extend_from_slice(&7u128.to_le_bytes());
            bytes.extend_from_slice(&message.to_bytes()[1..]);
            bytes
        };
        assert!(Message::decode(&relay(Message::Ready(true))).is_ok());
        for refused in [
            Message::Ping { sent_ms: 1 },
            Message::KartState(KartSnapshot::default()),
            Message::Seats(vec![PeerKey(1)]),
            Message::Circuit(1),
            Message::RoomFull,
            Message::Relayed {
                origin: PeerKey(2),
                message: Box::new(Message::Bye),
            },
        ] {
            assert!(invalid(&relay(refused.clone())), "{refused:?} relayed");
        }
        // A bundle of no state, a peer without a state, more states than karts, a peer twice,
        // an impossible state inside.
        let one = Message::KartStates(vec![(PeerKey(3), KartSnapshot::default())]).to_bytes();
        assert!(Message::decode(&one).is_ok());
        assert!(invalid(&[v, KART_STATES, 0]));
        let mut empty_group = vec![v, KART_STATES, 1];
        empty_group.extend_from_slice(&3u128.to_le_bytes());
        empty_group.push(0);
        assert!(invalid(&empty_group));
        // Version, kind, one peer (16 bytes), its count, then its state from byte 20.
        let mut too_many = one.clone();
        too_many[19] = MAX_BUNDLED_STATES as u8 + 1;
        for _ in 0..MAX_BUNDLED_STATES {
            too_many.extend_from_slice(&one[20..]);
        }
        assert!(invalid(&too_many), "13 states");
        let mut twice = one.clone();
        twice[2] = 2;
        twice.extend_from_slice(&one[3..]);
        assert!(invalid(&twice), "a peer twice");
        let mut other = twice.clone();
        other[3 + 17 + 47] = 4;
        assert!(Message::decode(&other).is_ok(), "two peers");
        let mut bad = one.clone();
        bad[20 + 4 + 2 + 12 + 6 + 12] = 0x07;
        assert!(invalid(&bad), "phase 7 in a bundle");
    }

    /// Random bytes, and every message with each of its bytes changed: refused or read, never a
    /// panic.
    #[test]
    fn garbage_never_panics() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let length = (next() % 80) as usize;
            let mut bytes: Vec<u8> = (0..length).map(|_| next() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = PROTOCOL_VERSION;
            }
            let _ = Message::decode(&bytes);
        }
        for message in every_message() {
            let bytes = message.to_bytes();
            for at in 0..bytes.len() {
                for value in [0, 1, 2, 0x7f, 0x80, 0xff] {
                    let mut changed = bytes.clone();
                    changed[at] = value;
                    if let Ok(read) = Message::decode(&changed) {
                        // What was read writes back to the same bytes.
                        assert_eq!(read.to_bytes(), changed, "{message:?}");
                    }
                }
            }
        }
    }

    /// Issue #55: the `Hello` carries the kart's color, every color of the palette comes back the
    /// same (its byte, the last of the message), and a byte outside the palette is refused.
    #[test]
    fn the_hello_carries_the_kart_color() {
        for color in KartColor::ALL {
            let hello = Message::Hello(Profile {
                name: "Ana".into(),
                loadout: [1, 2, 0, 1],
                ready: true,
                seated: true,
                color,
            });
            let bytes = hello.to_bytes();
            assert_eq!(bytes.last(), Some(&color.index()));
            assert_eq!(Message::decode(&bytes), Ok(hello));
        }
        let mut bytes = Message::Hello(Profile::default()).to_bytes();
        let Ok(Message::Hello(read)) = Message::decode(&bytes) else {
            panic!("not read back");
        };
        assert_eq!(read.color, KartColor::Pink, "pink by default");
        for outside in KartColor::ALL.len() as u8..=255 {
            *bytes.last_mut().unwrap() = outside;
            assert_eq!(
                Message::decode(&bytes),
                Err(DecodeError::Invalid("kart color")),
                "{outside}"
            );
        }
        // A `Hello` of version 4 (no color) is not misread: the version says it.
        let mut old = Message::Hello(Profile::default()).to_bytes();
        old[0] = 4;
        old.pop();
        assert_eq!(Message::decode(&old), Err(DecodeError::OtherVersion(4)));
    }

    #[test]
    fn names_are_cleaned_and_cut_at_a_character() {
        assert_eq!(Profile::clean_name("  Max \n"), "Max");
        assert_eq!(Profile::clean_name("a\u{7}b"), "ab");
        assert_eq!(
            Profile::clean_name("Éloïse la très rapide"),
            "Éloïse la très r"
        );
        assert_eq!(Profile::clean_name("abcdefghijklmno pq"), "abcdefghijklmno");
        // A name that does not fit the wire is cut at a character boundary, still UTF-8.
        let wide = "é".repeat(40);
        let hello = Message::Hello(Profile {
            name: wide,
            ..Default::default()
        });
        let Ok(Message::Hello(read)) = Message::decode(&hello.to_bytes()) else {
            panic!("not read back");
        };
        assert_eq!(read.name, "é".repeat(MAX_NAME_BYTES / 2));
        assert_eq!(PeerKey(0xabcd << 112).tag(), "abcd");
    }
}
