//! A bad network, on purpose (issue #8, debug): latency, jitter and losses applied to the kart
//! states a peer receives, to see how the remote karts behave on a poor connection. Pure logic,
//! deterministic from its seed: unit-tested.
//!
//! Set in the admin panel (`NetTuning.simulator`), or by the page address: `?netsim=120,20,5`
//! (120 ms of latency, ±20 ms of jitter, 5 % of losses; `--netsim 120,20,5` natively). Only the
//! kart states go through it: the lobby, the pings (the ping shown stays the real one) and the
//! events are untouched. Each peer simulates the way in: to slow both ways, both set it.

use bevy::prelude::*;

/// The simulated network (0 everywhere: off).
#[derive(Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Default)]
pub struct NetSimTuning {
    /// Added to every kart state received (ms).
    pub latency_ms: f32,
    /// Plus or minus this much, drawn for each state (ms): they can come out of order.
    pub jitter_ms: f32,
    /// Share of the states lost (%).
    pub loss_percent: f32,
}

impl NetSimTuning {
    /// Anything to simulate.
    pub fn active(&self) -> bool {
        self.latency_ms > 0.0 || self.jitter_ms > 0.0 || self.loss_percent > 0.0
    }

    /// `120,20,5` (latency, jitter, losses), `120,20` or `120`; `off` or `0` for none. `None`:
    /// unreadable, or a negative value.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("off") {
            return Some(Self::default());
        }
        let mut values = [0.0f32; 3];
        for (count, part) in text.split(',').enumerate() {
            let value: f32 = part.trim().parse().ok()?;
            if count == values.len() || !value.is_finite() || value < 0.0 {
                return None;
            }
            values[count] = value;
        }
        Some(Self {
            latency_ms: values[0],
            jitter_ms: values[1],
            loss_percent: values[2].min(100.0),
        })
    }
}

/// Packets held back by the simulator, each until its time.
#[derive(Clone, Debug)]
pub struct NetSim<T> {
    state: u64,
    queue: Vec<(f64, u64, T)>,
    order: u64,
    /// Packets given to it, and lost.
    pub received: u64,
    pub lost: u64,
}

impl<T> NetSim<T> {
    /// A simulator whose draws come from `seed`: the same seed, the same packets, the same fate.
    pub fn new(seed: u64) -> Self {
        Self {
            // SplitMix64 of the seed: never the zero state of xorshift.
            state: splitmix(seed) | 1,
            queue: Vec::with_capacity(256),
            order: 0,
            received: 0,
            lost: 0,
        }
    }

    /// A packet arrives at `now_ms`: lost (`false`), or held until its latency has passed.
    pub fn push(&mut self, now_ms: f64, packet: T, tuning: &NetSimTuning) -> bool {
        self.received += 1;
        let loss = self.unit();
        let jitter = self.unit() * 2.0 - 1.0;
        if loss * 100.0 < f64::from(tuning.loss_percent) {
            self.lost += 1;
            return false;
        }
        let delay = (f64::from(tuning.latency_ms) + jitter * f64::from(tuning.jitter_ms)).max(0.0);
        self.queue.push((now_ms + delay, self.order, packet));
        self.order += 1;
        true
    }

    /// The packets whose time has come at `now_ms`, in the order of their time.
    pub fn release(&mut self, now_ms: f64, mut deliver: impl FnMut(T)) {
        loop {
            let next = self
                .queue
                .iter()
                .enumerate()
                .filter(|(_, (due, _, _))| *due <= now_ms)
                .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                .map(|(index, _)| index);
            let Some(index) = next else {
                return;
            };
            let (_, _, packet) = self.queue.swap_remove(index);
            deliver(packet);
        }
    }

    /// Packets held back.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn splitmix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad() -> NetSimTuning {
        NetSimTuning {
            latency_ms: 120.0,
            jitter_ms: 20.0,
            loss_percent: 5.0,
        }
    }

    /// What comes out: (packet, time it came out), for 3 000 packets sent every 33 ms.
    fn run(seed: u64, tuning: &NetSimTuning) -> Vec<(u32, f64)> {
        let mut sim = NetSim::new(seed);
        let mut out = Vec::new();
        for frame in 0..12_000u32 {
            let now = f64::from(frame) * 1_000.0 / 120.0;
            if frame % 4 == 0 {
                sim.push(now, frame / 4, tuning);
            }
            sim.release(now, |packet| out.push((packet, now)));
        }
        out
    }

    /// Issue #8: the simulator is deterministic from its seed; 120 ms ± 20 ms and 5 % lost.
    #[test]
    fn the_simulator_is_deterministic_and_does_what_it_says() {
        let tuning = bad();
        let first = run(3, &tuning);
        assert_eq!(first, run(3, &tuning), "same seed, same fate");
        assert_ne!(first, run(4, &tuning), "another seed, another fate");
        let lost = 3_000 - first.len();
        assert!((110..=190).contains(&lost), "{lost} lost of 3 000");
        let mut disorder = 0;
        for window in first.windows(2) {
            if window[1].0 < window[0].0 {
                disorder += 1;
            }
        }
        assert!(disorder > 0, "the jitter reorders some");
        for (packet, out) in &first {
            let sent = f64::from(*packet * 4) * 1_000.0 / 120.0;
            let delay = out - sent;
            // Released on the first frame (8.3 ms) after its time.
            assert!(
                (100.0..=140.0 + 8.4).contains(&delay),
                "packet {packet}: {delay}"
            );
        }
        // Off: everything, at once, in order.
        let clean = run(3, &NetSimTuning::default());
        assert_eq!(clean.len(), 3_000);
        assert!(clean.iter().enumerate().all(|(i, (p, _))| *p == i as u32));
        assert!(!NetSimTuning::default().active() && tuning.active());
    }

    #[test]
    fn the_address_says_how_bad() {
        assert_eq!(NetSimTuning::parse("120,20,5"), Some(bad()));
        assert_eq!(
            NetSimTuning::parse(" 80 "),
            Some(NetSimTuning {
                latency_ms: 80.0,
                ..Default::default()
            })
        );
        assert_eq!(
            NetSimTuning::parse("50,10"),
            Some(NetSimTuning {
                latency_ms: 50.0,
                jitter_ms: 10.0,
                loss_percent: 0.0
            })
        );
        assert_eq!(NetSimTuning::parse("0"), Some(NetSimTuning::default()));
        assert_eq!(NetSimTuning::parse("off"), Some(NetSimTuning::default()));
        assert_eq!(NetSimTuning::parse("0,0,250").unwrap().loss_percent, 100.0);
        for refused in ["", "fast", "1,2,3,4", "-5", "10,,2", "nan"] {
            assert_eq!(NetSimTuning::parse(refused), None, "{refused}");
        }
    }
}
