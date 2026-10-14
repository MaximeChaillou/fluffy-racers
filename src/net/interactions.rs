//! The interactions between karts on the wire (issue #34's events, `docs/ARCHITECTURE.md`
//! § « Interactions entre karts »): `Coup`, `CoupReçu` / `CoupIgnoré`, `Attrape` and
//! `FinAttrape` are already Bevy messages ([`KartStruck`], [`StrikeAnswer`], [`KartGrabbed`],
//! [`TetherEnded`]); here they become protocol messages and back. A kart's entity becomes its
//! network id ([`KartId`]) and back through a mapping given by the caller (issue #8 builds it:
//! the grid slots of the synchronized karts).
//!
//! Sending them is issue #9's: each one goes to the peer that has authority over the kart it
//! targets (the victim decides).

use bevy::prelude::*;

use super::protocol::{Grab, KartId, Strike, StrikeReply, TetherBreak};
use crate::interactions::{KartGrabbed, KartStruck, StrikeAnswer, TetherEnded};

impl Strike {
    /// `None` when a kart has no network id.
    pub fn from_event(event: &KartStruck, id: impl Fn(Entity) -> Option<KartId>) -> Option<Self> {
        Some(Self {
            striker: id(event.striker)?,
            target: id(event.target)?,
            tick: event.tick,
            seen: event.seen,
            striker_position: event.striker_position.to_array(),
            direction: [event.direction.x, event.direction.z],
            push: event.push,
            throw: event.throw,
        })
    }

    /// `None` when a kart is unknown here.
    pub fn to_event(&self, entity: impl Fn(KartId) -> Option<Entity>) -> Option<KartStruck> {
        Some(KartStruck {
            striker: entity(self.striker)?,
            target: entity(self.target)?,
            tick: self.tick,
            seen: self.seen,
            striker_position: Vec3::from_array(self.striker_position),
            // Horizontal: only x and z travel.
            direction: Vec3::new(self.direction[0], 0.0, self.direction[1]),
            push: self.push,
            throw: self.throw,
        })
    }
}

impl StrikeReply {
    pub fn from_event(event: &StrikeAnswer, id: impl Fn(Entity) -> Option<KartId>) -> Option<Self> {
        Some(Self {
            striker: id(event.striker)?,
            target: id(event.target)?,
            landed: event.landed,
            throw: event.throw,
        })
    }

    pub fn to_event(&self, entity: impl Fn(KartId) -> Option<Entity>) -> Option<StrikeAnswer> {
        Some(StrikeAnswer {
            striker: entity(self.striker)?,
            target: entity(self.target)?,
            landed: self.landed,
            throw: self.throw,
        })
    }
}

impl Grab {
    pub fn from_event(event: &KartGrabbed, id: impl Fn(Entity) -> Option<KartId>) -> Option<Self> {
        Some(Self {
            grabber: id(event.grabber)?,
            target: id(event.target)?,
            tick: event.tick,
            seen: event.seen,
            grabber_position: event.grabber_position.to_array(),
            rest: event.rest,
        })
    }

    pub fn to_event(&self, entity: impl Fn(KartId) -> Option<Entity>) -> Option<KartGrabbed> {
        Some(KartGrabbed {
            grabber: entity(self.grabber)?,
            target: entity(self.target)?,
            tick: self.tick,
            seen: self.seen,
            grabber_position: Vec3::from_array(self.grabber_position),
            rest: self.rest,
        })
    }
}

impl TetherBreak {
    pub fn from_event(event: &TetherEnded, id: impl Fn(Entity) -> Option<KartId>) -> Option<Self> {
        Some(Self {
            grabber: id(event.grabber)?,
            target: id(event.target)?,
            reason: event.reason,
        })
    }

    pub fn to_event(&self, entity: impl Fn(KartId) -> Option<Entity>) -> Option<TetherEnded> {
        Some(TetherEnded {
            grabber: entity(self.grabber)?,
            target: entity(self.target)?,
            reason: self.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactions::TetherEnd;
    use crate::net::Message;

    /// The karts of a race on two peers: slot `n` is a different entity on each.
    fn karts(base: u32) -> Vec<Entity> {
        (0..12)
            .map(|n| Entity::from_raw_u32(base + n).unwrap())
            .collect()
    }

    fn id_on(karts: &[Entity]) -> impl Fn(Entity) -> Option<KartId> + '_ {
        |entity| karts.iter().position(|k| *k == entity).map(|n| n as KartId)
    }

    fn entity_on(karts: &[Entity]) -> impl Fn(KartId) -> Option<Entity> + '_ {
        |id| karts.get(usize::from(id)).copied()
    }

    /// Issue #34's events go from one peer's entities to the bytes and to the same karts on
    /// another peer.
    #[test]
    fn interaction_events_travel_between_peers() {
        let (here, there) = (karts(10), karts(500));
        let decode = |message: Message| Message::decode(&message.to_bytes()).unwrap();

        let struck = KartStruck {
            striker: here[3],
            target: here[7],
            tick: 1_234,
            seen: 1_228,
            striker_position: Vec3::new(12.5, 0.6, -40.25),
            direction: Vec3::new(0.6, 0.0, -0.8),
            push: 8.75,
            throw: true,
        };
        let Message::Strike(wire) = decode(Message::Strike(
            Strike::from_event(&struck, id_on(&here)).unwrap(),
        )) else {
            panic!("not a strike");
        };
        let received = wire.to_event(entity_on(&there)).unwrap();
        assert_eq!(received.striker, there[3]);
        assert_eq!(received.target, there[7]);
        assert_eq!(
            KartStruck {
                striker: here[3],
                target: here[7],
                ..received
            },
            struck
        );

        let answer = StrikeAnswer {
            striker: here[0],
            target: here[11],
            landed: true,
            throw: false,
        };
        let Message::StrikeReply(wire) = decode(Message::StrikeReply(
            StrikeReply::from_event(&answer, id_on(&here)).unwrap(),
        )) else {
            panic!("not a reply");
        };
        assert_eq!(
            wire.to_event(entity_on(&there)),
            Some(StrikeAnswer {
                striker: there[0],
                target: there[11],
                ..answer
            })
        );

        let grabbed = KartGrabbed {
            grabber: here[2],
            target: here[5],
            tick: 99,
            seen: 95,
            grabber_position: Vec3::new(-1.0, 0.5, 3.0),
            rest: 2.4,
        };
        let Message::Grab(wire) = decode(Message::Grab(
            Grab::from_event(&grabbed, id_on(&here)).unwrap(),
        )) else {
            panic!("not a grab");
        };
        assert_eq!(
            wire.to_event(entity_on(&there)),
            Some(KartGrabbed {
                grabber: there[2],
                target: there[5],
                ..grabbed
            })
        );

        let ended = TetherEnded {
            grabber: here[5],
            target: here[2],
            reason: TetherEnd::Escaped,
        };
        let Message::TetherBreak(wire) = decode(Message::TetherBreak(
            TetherBreak::from_event(&ended, id_on(&here)).unwrap(),
        )) else {
            panic!("not a tether end");
        };
        assert_eq!(
            wire.to_event(entity_on(&there)),
            Some(TetherEnded {
                grabber: there[5],
                target: there[2],
                ..ended
            })
        );
    }

    /// A kart without a network id (or unknown on the other side) gives nothing.
    #[test]
    fn unknown_karts_give_nothing() {
        let here = karts(10);
        let stranger = Entity::from_raw_u32(9_999).unwrap();
        let answer = StrikeAnswer {
            striker: stranger,
            target: here[1],
            landed: false,
            throw: false,
        };
        assert_eq!(StrikeReply::from_event(&answer, id_on(&here)), None);
        let wire = StrikeReply {
            striker: 0,
            target: 200,
            landed: true,
            throw: true,
        };
        assert_eq!(wire.to_event(entity_on(&here)), None);
    }
}
