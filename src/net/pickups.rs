//! Who picked up what (issue #53, `docs/ARCHITECTURE.md` § « Objets et croquettes en ligne »):
//! the host decides which kart got an item box, a croquette of the track or a scattered
//! croquette; since issue #61, whether a kart may have the oreiller géant it drew. Pure logic: no
//! Bevy, no socket; unit-tested.
//!
//! - **Claims**: a kart touching a box or a croquette takes it at once on its own screen (its
//!   roulette spins, its croquette counts: a prediction), and its peer sends a [`Claim`] to every
//!   peer: the spot, the kart, the common tick.
//! - **The host decides** ([`Arbiter::decide`]): the claims it has, in order of their tick, then
//!   of the better grid slot (the smaller id). A claim is granted when no other kart got its spot
//!   within the spot's respawn time of it (a box: 2 s, a croquette of the track: 10 s, a scattered
//!   croquette: ever); otherwise it is refused. A claim that comes after a grant is refused, even
//!   if its tick is earlier: the first to reach the host wins. Every [`Verdict`] goes to every peer.
//! - **Every peer keeps the same record** ([`Arbiter::record`]): the grants so far, and the
//!   claims not decided yet (every peer receives every claim: a client's through the host, issue
//!   #63). Only the host decides, from start to end (it created the room); its verdicts reach
//!   every client in its order. A verdict that comes twice counts once: the first one stands.
//! - **The oreiller géant** (issue #61, GD § 8): a box draw that gives one is also a claim
//!   ([`Pickup::Pillow`]). The host grants it when its box was granted and the rules allow one
//!   on the claim's common tick ([`PillowRules`], with `items::pillow_allowed`, the solo rules):
//!   not before 20 s of race, none while another one is in the race, 30 s after the last one
//!   granted. Two claims on the same tick: the better grid slot. The record knows every oreiller
//!   in the race ([`Pillow`]): granted (in its kart's slots), thrown (`ItemOut`), until its flight
//!   ends, its kart's peer says it is gone without a throw (`PillowGone`) or leaves. Issue #63:
//!   every claim reaches the host first (the clients' through their link with it), and it alone
//!   decides: two oreillers are never in the race (the hand-over of issue #61 is gone with the
//!   change of host).

use super::protocol::{Claim, ItemId, KartId, PeerKey, Pickup, Spot, Verdict};
use crate::items::pillow_allowed;

/// A claim, whatever its draws: its spot, its kart, its tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClaimKey {
    pub spot: Spot,
    pub kart: KartId,
    pub tick: u32,
}

impl ClaimKey {
    pub fn of(claim: &Claim) -> Self {
        Self {
            spot: claim.pickup.spot(),
            kart: claim.kart,
            tick: claim.tick,
        }
    }
}

/// The refused claims kept to recognize a repeat, at most.
const REFUSED_KEPT: usize = 256;
/// The oreillers known gone kept (a grant that comes after its kart's peer said it was gone), at
/// most.
const GONE_KEPT: usize = 32;

/// An oreiller géant in the race, as the record knows it (issue #61).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pillow {
    /// Its granted claim (`None`: seen thrown before its grant came).
    pub claim: Option<Claim>,
    /// The peer that claimed it (its kart's): if it leaves, the oreiller is gone.
    pub from: Option<PeerKey>,
    pub kart: KartId,
    /// Thrown: its id (it flies until its copy here is gone); `None`: in its kart's slots.
    pub thrown: Option<ItemId>,
}

/// The oreiller géant's rules on the common clock (issue #61), as the host applies them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillowRules {
    /// The common tick the race's ticks count from (its GO is race tick 1, as solo:
    /// `Race::race_ticks`); `None`: no race, never an oreiller.
    pub race_base: Option<u32>,
    /// No oreiller before this race tick (20 s).
    pub min_race_ticks: u32,
    /// None within this many ticks of the last one granted (30 s).
    pub cooldown_ticks: u32,
}

impl PillowRules {
    /// No race: never an oreiller.
    pub const NONE: Self = Self {
        race_base: None,
        min_race_ticks: 0,
        cooldown_ticks: 0,
    };
}

/// The record of the pickups of a session (a race, or free driving), the same on every peer;
/// the host decides with it (see the module's documentation).
#[derive(Clone, Debug, Default)]
pub struct Arbiter {
    /// Every grant, in the host's order.
    grants: Vec<ClaimKey>,
    /// The latest refused claims.
    refused: Vec<ClaimKey>,
    /// The claims not decided yet, with the peer that sent them.
    pending: Vec<(PeerKey, Claim)>,
    /// The oreillers géants in the race (issue #61).
    pillows: Vec<Pillow>,
    /// The latest oreillers known gone without a throw, by their claim.
    gone: Vec<ClaimKey>,
    /// The common tick of the last oreiller granted (its claim's).
    last_pillow: Option<u32>,
}

impl Arbiter {
    /// Whether this claim was decided already, and how (`Some(true)`: granted).
    pub fn decided(&self, key: &ClaimKey) -> Option<bool> {
        if self.grants.contains(key) {
            Some(true)
        } else if self.refused.contains(key) {
            Some(false)
        } else {
            None
        }
    }

    /// A claim seen (ours, or another peer's): kept until a verdict decides it. Returns false
    /// when it was decided or seen already.
    pub fn claim(&mut self, from: PeerKey, claim: Claim) -> bool {
        let key = ClaimKey::of(&claim);
        if self.decided(&key).is_some() || self.pending.iter().any(|(_, c)| ClaimKey::of(c) == key)
        {
            return false;
        }
        self.pending.push((from, claim));
        true
    }

    /// The claims not decided yet.
    pub fn pending(&self) -> impl Iterator<Item = &Claim> {
        self.pending.iter().map(|(_, claim)| claim)
    }

    /// Who got `spot` within `respawn` ticks of `tick` (`None`: ever), if anyone.
    pub fn taken(&self, spot: Spot, tick: u32, respawn: Option<u32>) -> Option<&ClaimKey> {
        self.grants.iter().rev().find(|grant| {
            grant.spot == spot && respawn.is_none_or(|ticks| grant.tick.abs_diff(tick) < ticks)
        })
    }

    /// The host decides every claim it has (see the module's documentation): `respawn` gives the
    /// respawn time of a spot in ticks (`None`: it never comes back); the oreillers géants follow
    /// `pillows` (issue #61). The verdicts go into `out`, in their order, and into the record.
    pub fn decide(
        &mut self,
        respawn: impl Fn(Spot) -> Option<u32>,
        pillows: &PillowRules,
        out: &mut Vec<Verdict>,
    ) {
        if self.pending.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.pending);
        // Earlier first; on the same tick, the better grid slot; a box before the oreiller
        // drawn from it.
        pending.sort_by_key(|(_, claim)| {
            (
                claim.tick,
                claim.kart,
                matches!(claim.pickup, Pickup::Pillow { .. }),
            )
        });
        for &(from, claim) in &pending {
            let pillow = matches!(claim.pickup, Pickup::Pillow { .. });
            let key = ClaimKey::of(&claim);
            let granted = if pillow {
                self.pillow_allowed(&claim, pillows)
            } else {
                self.taken(key.spot, key.tick, respawn(key.spot)).is_none()
            };
            let verdict = Verdict { claim, granted };
            self.record_from(&verdict, Some(from));
            out.push(verdict);
        }
        // Keep the allocation of the pending list (nothing came in meanwhile).
        pending.clear();
        self.pending = pending;
    }

    /// Whether the oreiller géant of `claim` may be granted (issue #61): its box was, and the
    /// solo rules allow one on its tick (`items::pillow_allowed`).
    fn pillow_allowed(&self, claim: &Claim, rules: &PillowRules) -> bool {
        let Pickup::Pillow { index, .. } = claim.pickup else {
            return false;
        };
        let from_box = ClaimKey {
            spot: Spot::Box(index),
            kart: claim.kart,
            tick: claim.tick,
        };
        let Some(base) = rules.race_base else {
            return false;
        };
        self.grants.contains(&from_box)
            && pillow_allowed(
                claim.tick.checked_sub(base),
                !self.pillows.is_empty(),
                self.last_pillow.map(|tick| tick.saturating_sub(base)),
                rules.min_race_ticks,
                rules.cooldown_ticks,
            )
    }

    /// A verdict (the host's, or ours as the host): into the record, its claim no longer
    /// pending. Returns false when that claim was decided already (the first verdict stands).
    pub fn record(&mut self, verdict: &Verdict) -> bool {
        let key = ClaimKey::of(&verdict.claim);
        let from = self
            .pending
            .iter()
            .find(|(_, claim)| ClaimKey::of(claim) == key)
            .map(|(from, _)| *from);
        self.record_from(verdict, from)
    }

    fn record_from(&mut self, verdict: &Verdict, from: Option<PeerKey>) -> bool {
        let key = ClaimKey::of(&verdict.claim);
        self.pending.retain(|(_, claim)| ClaimKey::of(claim) != key);
        if self.decided(&key).is_some() {
            return false;
        }
        if verdict.granted {
            self.grants.push(key);
            if matches!(key.spot, Spot::Pillow(_)) {
                self.pillow_granted(verdict.claim, from);
            }
        } else {
            if self.refused.len() >= REFUSED_KEPT {
                self.refused.remove(0);
            }
            self.refused.push(key);
        }
        true
    }

    /// An oreiller géant granted: in its kart's slots (or already flying, its throw seen first).
    fn pillow_granted(&mut self, claim: Claim, from: Option<PeerKey>) {
        self.last_pillow = Some(self.last_pillow.map_or(claim.tick, |t| t.max(claim.tick)));
        if self.gone.contains(&ClaimKey::of(&claim)) {
            return;
        }
        match self
            .pillows
            .iter_mut()
            .find(|p| p.kart == claim.kart && p.claim.is_none())
        {
            Some(thrown) => {
                thrown.claim = Some(claim);
                thrown.from = from;
            }
            None => self.pillows.push(Pillow {
                claim: Some(claim),
                from,
                kart: claim.kart,
                thrown: None,
            }),
        }
    }

    /// `kart` threw an oreiller géant (issue #61, its `ItemOut`, as `id`): it flies until its
    /// flight ends ([`Arbiter::pillow_ended`]).
    pub fn pillow_thrown(&mut self, kart: KartId, id: ItemId) {
        if self.pillows.iter().any(|p| p.thrown == Some(id)) {
            return;
        }
        match self
            .pillows
            .iter_mut()
            .find(|p| p.kart == kart && p.thrown.is_none())
        {
            Some(held) => held.thrown = Some(id),
            None => self.pillows.push(Pillow {
                claim: None,
                from: None,
                kart,
                thrown: Some(id),
            }),
        }
    }

    /// The oreiller géant `id` is no more (it exploded, a klaxon-canard destroyed it, its thrower
    /// left): another one may come.
    pub fn pillow_ended(&mut self, id: ItemId) {
        self.pillows.retain(|p| p.thrown != Some(id));
    }

    /// The oreiller géant of claim `key` is gone without a throw (its kart's peer said so,
    /// `PillowGone`): another one may come. Told before its grant came, the grant will not bring
    /// it back.
    pub fn pillow_gone(&mut self, key: ClaimKey) {
        self.pillows.retain(|p| {
            p.thrown.is_some() || p.claim.is_none_or(|claim| ClaimKey::of(&claim) != key)
        });
        if !self.gone.contains(&key) {
            if self.gone.len() >= GONE_KEPT {
                self.gone.remove(0);
            }
            self.gone.push(key);
        }
    }

    /// The oreillers géants in the race.
    pub fn pillows(&self) -> &[Pillow] {
        &self.pillows
    }

    /// `peer` left the room: its claims are dropped (its karts are gone), and the oreillers its
    /// karts held (issue #61).
    pub fn forget(&mut self, peer: PeerKey) {
        self.pending.retain(|(from, _)| *from != peer);
        self.pillows
            .retain(|p| p.thrown.is_some() || p.from != Some(peer));
    }

    /// Every grant so far, in the host's order.
    pub fn grants(&self) -> &[ClaimKey] {
        &self.grants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::{LooseId, Pickup};

    const ANA: PeerKey = PeerKey(1 << 100);
    const BOB: PeerKey = PeerKey(2 << 100);
    const CYD: PeerKey = PeerKey(3 << 100);

    /// 2 s for a box, 10 s for a croquette, once for a scattered one.
    fn respawn(spot: Spot) -> Option<u32> {
        match spot {
            Spot::Box(_) => Some(120),
            Spot::Croquette(_) => Some(600),
            Spot::Loose(_) | Spot::Pillow(_) => None,
        }
    }

    fn on_box(index: u16, kart: KartId, tick: u32) -> Claim {
        Claim {
            session: 1,
            pickup: Pickup::Box {
                index,
                draw: 0,
                draws: 1,
                gap: 0.0,
            },
            kart,
            tick,
        }
    }

    fn on_croquette(pickup: Pickup, kart: KartId, tick: u32) -> Claim {
        Claim {
            session: 1,
            pickup,
            kart,
            tick,
        }
    }

    fn decide(arbiter: &mut Arbiter) -> Vec<Verdict> {
        let mut out = Vec::new();
        arbiter.decide(respawn, &PillowRules::NONE, &mut out);
        out
    }

    fn granted(verdicts: &[Verdict]) -> Vec<(KartId, u32)> {
        verdicts
            .iter()
            .filter(|v| v.granted)
            .map(|v| (v.claim.kart, v.claim.tick))
            .collect()
    }

    /// Issue #53: two karts on the same box on the same tick: the better grid slot gets it, the
    /// other is refused. On different ticks: the earlier one, whatever the order they came in.
    #[test]
    fn two_karts_on_one_box_only_one_gets_it() {
        let mut arbiter = Arbiter::default();
        assert!(arbiter.claim(BOB, on_box(3, 7, 500)));
        assert!(arbiter.claim(ANA, on_box(3, 4, 500)));
        let verdicts = decide(&mut arbiter);
        assert_eq!(verdicts.len(), 2);
        assert_eq!(granted(&verdicts), vec![(4, 500)]);
        assert!(!verdicts.iter().find(|v| v.claim.kart == 7).unwrap().granted);
        // Another box, the later tick first: the earlier tick wins.
        arbiter.claim(ANA, on_box(5, 4, 601));
        arbiter.claim(BOB, on_box(5, 7, 600));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 600)]);
        assert_eq!(arbiter.grants().len(), 2);
        assert_eq!(arbiter.pending().count(), 0);
    }

    /// Issue #53: a box is taken once; it can be taken again once it is back (2 s later), and a
    /// claim that comes after a grant is refused even with an earlier tick (the first to reach
    /// the host wins: no grant is ever taken back).
    #[test]
    fn a_box_is_taken_once_until_it_comes_back() {
        let mut arbiter = Arbiter::default();
        arbiter.claim(ANA, on_box(0, 4, 1_000));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(4, 1_000)]);
        // Late, with an earlier tick: refused.
        arbiter.claim(BOB, on_box(0, 7, 998));
        assert!(granted(&decide(&mut arbiter)).is_empty());
        // Still there 1.98 s later: refused; 2 s later, back: granted.
        arbiter.claim(BOB, on_box(0, 7, 1_119));
        assert!(granted(&decide(&mut arbiter)).is_empty());
        arbiter.claim(BOB, on_box(0, 7, 1_120));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 1_120)]);
        assert_eq!(
            arbiter
                .taken(Spot::Box(0), 1_150, Some(120))
                .map(|g| g.kart),
            Some(7)
        );
        assert!(arbiter.taken(Spot::Box(0), 1_300, Some(120)).is_none());
    }

    /// Issue #53: a croquette of the track is taken once for 10 s; a scattered croquette once and
    /// for good.
    #[test]
    fn a_croquette_is_picked_up_once() {
        let mut arbiter = Arbiter::default();
        let track = Pickup::Croquette { index: 12 };
        let loose = Pickup::Loose(LooseId {
            kart: 2,
            loss: 0,
            index: 1,
        });
        arbiter.claim(ANA, on_croquette(track, 4, 100));
        arbiter.claim(BOB, on_croquette(track, 7, 101));
        arbiter.claim(BOB, on_croquette(loose, 7, 100));
        arbiter.claim(ANA, on_croquette(loose, 4, 100));
        let verdicts = decide(&mut arbiter);
        // The croquette of the track to the earlier tick, the scattered one (the same tick) to
        // the better grid slot.
        assert_eq!(granted(&verdicts), vec![(4, 100), (4, 100)]);
        assert_eq!(
            arbiter
                .grants()
                .iter()
                .map(|g| (g.spot, g.kart))
                .collect::<Vec<_>>(),
            vec![(track.spot(), 4), (loose.spot(), 4)]
        );
        assert_eq!(verdicts.iter().filter(|v| !v.granted).count(), 2);
        // Back after 10 s; the scattered one never.
        arbiter.claim(BOB, on_croquette(track, 7, 700));
        arbiter.claim(ANA, on_croquette(loose, 4, 5_000));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 700)]);
    }

    /// The same claim twice, or a claim decided already: kept once, decided once. A verdict that
    /// comes twice counts once, and the first one stands.
    #[test]
    fn a_claim_is_decided_once() {
        let mut arbiter = Arbiter::default();
        let claim = on_box(1, 4, 50);
        assert!(arbiter.claim(ANA, claim));
        assert!(!arbiter.claim(ANA, claim), "seen already");
        assert_eq!(decide(&mut arbiter).len(), 1);
        assert!(!arbiter.claim(BOB, claim), "decided already");
        assert!(decide(&mut arbiter).is_empty());
        let refused = Verdict {
            claim,
            granted: false,
        };
        assert!(!arbiter.record(&refused), "the first verdict stands");
        assert_eq!(arbiter.decided(&ClaimKey::of(&claim)), Some(true));
    }

    /// Issue #63 (was « a new host decides what the old one left », issue #53): in the star,
    /// every claim reaches the host first, a client's through its link with it, and the host
    /// relays it to the other clients before its verdict; the clients record the host's verdicts
    /// in its order: the same grants everywhere, nothing twice. The host leaving, nobody decides
    /// any more (the room is over): what was left stays undecided, nothing is given.
    #[test]
    fn the_clients_keep_the_hosts_record() {
        // Ana hosts; Bob and Cyd are her clients.
        let mut ana = Arbiter::default();
        let mut bob = Arbiter::default();
        let mut cyd = Arbiter::default();
        let claims = [
            (BOB, on_box(2, 7, 300)),
            (CYD, on_box(2, 9, 301)),
            (CYD, on_box(4, 9, 302)),
            (BOB, on_box(4, 7, 303)),
            (ANA, on_box(6, 4, 304)),
        ];
        // Each claim reaches Ana, then (relayed, or her own) the other clients.
        for (from, claim) in &claims[..4] {
            assert!(ana.claim(*from, *claim));
            for (client, arbiter) in [(BOB, &mut bob), (CYD, &mut cyd)] {
                if client != *from {
                    arbiter.claim(*from, *claim);
                }
            }
        }
        let first = decide(&mut ana);
        assert_eq!(granted(&first), vec![(7, 300), (9, 302)]);
        for verdict in &first {
            assert!(bob.record(verdict));
            assert!(cyd.record(verdict));
        }
        assert_eq!(bob.grants(), ana.grants());
        assert_eq!(cyd.grants(), ana.grants());
        assert_eq!((bob.pending().count(), cyd.pending().count()), (0, 0));
        // A verdict that came twice counts once.
        assert!(!bob.record(&first[0]));
        // Ana's last claim reaches her clients; she leaves before deciding it.
        for arbiter in [&mut bob, &mut cyd] {
            arbiter.claim(ANA, claims[4].1);
            arbiter.forget(ANA);
        }
        for arbiter in [&bob, &cyd] {
            assert_eq!(arbiter.grants(), ana.grants(), "nothing more is given");
        }
    }

    // --- Issue #61: the oreiller géant ------------------------------------------------------

    /// The race's tick 0 at common tick 600; no oreiller before race tick 1200 (20 s), none
    /// within 1800 ticks (30 s) of the last one.
    const RULES: PillowRules = PillowRules {
        race_base: Some(600),
        min_race_ticks: 1200,
        cooldown_ticks: 1800,
    };

    fn pillow(index: u16, kart: KartId, tick: u32) -> Claim {
        Claim {
            session: 1,
            pickup: Pickup::Pillow { index, draw: 0 },
            kart,
            tick,
        }
    }

    /// `kart` breaks box `index` at `tick` and draws an oreiller géant from it: two claims.
    fn draw_pillow(arbiter: &mut Arbiter, from: PeerKey, index: u16, kart: KartId, tick: u32) {
        arbiter.claim(from, on_box(index, kart, tick));
        arbiter.claim(from, pillow(index, kart, tick));
    }

    /// The host decides at common tick `_now` (issue #63: the moment no longer matters, no
    /// change of host to wait for; kept to read when each decision is made).
    fn decide_at(arbiter: &mut Arbiter, _now: u32) -> Vec<Verdict> {
        let mut out = Vec::new();
        arbiter.decide(respawn, &RULES, &mut out);
        out
    }

    /// The oreillers granted among `verdicts`: (kart, tick).
    fn pillows_granted(verdicts: &[Verdict]) -> Vec<(KartId, u32)> {
        verdicts
            .iter()
            .filter(|v| v.granted && matches!(v.claim.pickup, Pickup::Pillow { .. }))
            .map(|v| (v.claim.kart, v.claim.tick))
            .collect()
    }

    /// Issue #61 acceptance: two karts draw an oreiller géant on the same tick (two boxes): only
    /// one gets it (the better grid slot), whatever the order the claims came in. Then none
    /// while it is in the race (held, then flying), and none within 30 s of it, even once it is
    /// gone; 30 s after it, one again.
    #[test]
    fn two_oreillers_drawn_at_once_only_one_is_granted() {
        let mut arbiter = Arbiter::default();
        draw_pillow(&mut arbiter, BOB, 5, 7, 2_000);
        draw_pillow(&mut arbiter, ANA, 3, 4, 2_000);
        let verdicts = decide_at(&mut arbiter, 2_010);
        assert_eq!(verdicts.len(), 4);
        // Both boxes are theirs; the oreiller is kart 4's only.
        assert_eq!(granted(&verdicts).len(), 3);
        assert_eq!(pillows_granted(&verdicts), vec![(4, 2_000)]);
        assert_eq!(arbiter.pillows().len(), 1);
        assert_eq!(arbiter.pillows()[0].kart, 4);
        assert_eq!(arbiter.pillows()[0].from, Some(ANA));
        // Held by kart 4: none for kart 7, nor for kart 4 again.
        draw_pillow(&mut arbiter, BOB, 6, 7, 2_100);
        draw_pillow(&mut arbiter, ANA, 8, 4, 2_100);
        assert!(pillows_granted(&decide_at(&mut arbiter, 2_110)).is_empty());
        // Thrown: it flies, still one in the race.
        let id = ItemId { owner: 4, seq: 0 };
        arbiter.pillow_thrown(4, id);
        assert_eq!(arbiter.pillows()[0].thrown, Some(id));
        draw_pillow(&mut arbiter, BOB, 9, 7, 2_300);
        assert!(pillows_granted(&decide_at(&mut arbiter, 2_310)).is_empty());
        // Its flight ended: none in the race, but 29 s after it, none yet; 30 s after, one.
        arbiter.pillow_ended(id);
        assert!(arbiter.pillows().is_empty());
        draw_pillow(&mut arbiter, BOB, 10, 7, 3_799);
        assert!(pillows_granted(&decide_at(&mut arbiter, 3_800)).is_empty());
        draw_pillow(&mut arbiter, BOB, 11, 7, 3_800);
        assert_eq!(
            pillows_granted(&decide_at(&mut arbiter, 3_801)),
            vec![(7, 3_800)]
        );
        // The record has no oreiller twice: the grants are its claims, one at a time.
        let oreillers = arbiter
            .grants()
            .iter()
            .filter(|g| matches!(g.spot, Spot::Pillow(_)))
            .count();
        assert_eq!(oreillers, 2);
    }

    /// Issue #61: no oreiller before 20 s of race (race tick 1200: common tick 1800 here), none
    /// off a race, none from a box the kart did not get (its roulettes stopped).
    #[test]
    fn no_oreiller_before_20_s_nor_from_a_box_refused() {
        let mut arbiter = Arbiter::default();
        draw_pillow(&mut arbiter, ANA, 1, 4, 1_799);
        assert!(pillows_granted(&decide_at(&mut arbiter, 1_800)).is_empty());
        draw_pillow(&mut arbiter, ANA, 2, 4, 1_800);
        assert_eq!(
            pillows_granted(&decide_at(&mut arbiter, 1_801)),
            vec![(4, 1_800)]
        );
        // Off a race: never.
        let mut free = Arbiter::default();
        draw_pillow(&mut free, ANA, 1, 4, 90_000);
        let mut out = Vec::new();
        free.decide(respawn, &PillowRules::NONE, &mut out);
        assert!(pillows_granted(&out).is_empty());
        assert_eq!(granted(&out), vec![(4, 90_000)], "the box is still its own");
        // Two karts on one box, both draw an oreiller: the box and its oreiller go to the better
        // grid slot; the other one gets neither.
        let mut boxed = Arbiter::default();
        draw_pillow(&mut boxed, BOB, 4, 7, 5_000);
        draw_pillow(&mut boxed, ANA, 4, 9, 5_000);
        let verdicts = decide_at(&mut boxed, 5_001);
        assert_eq!(granted(&verdicts), vec![(7, 5_000), (7, 5_000)]);
        // A late box (refused: another kart had it) brings no oreiller, even when none is in
        // the race.
        let mut late = Arbiter::default();
        late.claim(ANA, on_box(4, 4, 6_000));
        decide_at(&mut late, 6_000);
        draw_pillow(&mut late, BOB, 4, 7, 5_999);
        assert!(granted(&decide_at(&mut late, 6_001)).is_empty());
        assert!(late.pillows().is_empty());
    }

    /// Issue #61: an oreiller gone without a throw (its kart's peer says so: a dégonflage) or
    /// whose kart's peer left is no longer in the race (another may come after the cooldown);
    /// told gone before its grant came, the grant does not bring it back; thrown before its
    /// grant came, the grant goes to the oreiller in flight.
    #[test]
    fn an_oreiller_gone_without_a_throw() {
        let mut arbiter = Arbiter::default();
        draw_pillow(&mut arbiter, ANA, 1, 4, 2_000);
        decide_at(&mut arbiter, 2_000);
        let claim = pillow(1, 4, 2_000);
        arbiter.pillow_gone(ClaimKey::of(&claim));
        assert!(arbiter.pillows().is_empty());
        draw_pillow(&mut arbiter, BOB, 2, 7, 3_800);
        assert_eq!(
            pillows_granted(&decide_at(&mut arbiter, 3_800)),
            vec![(7, 3_800)]
        );
        // Its peer leaves: gone.
        arbiter.forget(BOB);
        assert!(arbiter.pillows().is_empty());
        // Another peer, which never had the claims: the "gone" first, then the grant.
        let mut late = Arbiter::default();
        late.pillow_gone(ClaimKey::of(&claim));
        assert!(late.record(&Verdict {
            claim,
            granted: true
        }));
        assert!(late.pillows().is_empty(), "gone before its grant came");
        // Thrown before its grant came: one oreiller, flying, with its claim.
        let mut early = Arbiter::default();
        let id = ItemId { owner: 4, seq: 2 };
        early.pillow_thrown(4, id);
        early.record(&Verdict {
            claim,
            granted: true,
        });
        assert_eq!(early.pillows().len(), 1);
        assert_eq!(early.pillows()[0].thrown, Some(id));
        assert_eq!(early.pillows()[0].claim, Some(claim));
        early.pillow_ended(id);
        assert!(early.pillows().is_empty());
        // A flying oreiller stays when its thrower's peer leaves (its copy goes with it here).
        let mut flying = Arbiter::default();
        flying.claim(ANA, on_box(1, 4, 2_000));
        flying.claim(ANA, claim);
        decide_at(&mut flying, 2_000);
        flying.pillow_thrown(4, id);
        flying.forget(ANA);
        assert_eq!(flying.pillows().len(), 1);
    }

    /// Issue #63 (was « a new host never grants a second oreiller », issue #61): the host alone
    /// grants the oreillers. Cyd's kart and Dan's draw one, both clients: their claims reach Ana
    /// (the host), which grants Cyd's and refuses Dan's (one in the race), and relays the claims
    /// and her verdicts to Bob, another client: the same record, one oreiller, Cyd's kart's. Ana
    /// leaving changes nothing in the record, and nobody grants another.
    #[test]
    fn the_host_alone_grants_the_oreillers() {
        const DAN: PeerKey = PeerKey(4 << 100);
        let mut ana = Arbiter::default();
        let mut bob = Arbiter::default();
        // Cyd's kart 9 draws an oreiller, then Dan's kart 10.
        for arbiter in [&mut ana, &mut bob] {
            draw_pillow(arbiter, CYD, 2, 9, 2_000);
            draw_pillow(arbiter, DAN, 5, 10, 2_005);
        }
        let verdicts = decide_at(&mut ana, 2_010);
        assert_eq!(pillows_granted(&verdicts), vec![(9, 2_000)]);
        assert_eq!(granted(&verdicts).len(), 3, "both boxes, one oreiller");
        for verdict in &verdicts {
            assert!(bob.record(verdict));
        }
        for arbiter in [&ana, &bob] {
            assert_eq!(arbiter.pillows().len(), 1);
            assert_eq!(arbiter.pillows()[0].kart, 9);
            assert_eq!(arbiter.pending().count(), 0);
        }
        // Ana leaves: Bob's record stays as it was.
        bob.forget(ANA);
        assert_eq!(bob.pillows().len(), 1);
        assert_eq!(bob.grants(), ana.grants());
    }
}
