//! A tiny deterministic pseudo-random generator for the bots' variety, without any crate:
//! SplitMix64 to spread the seed, then xorshift64*. Same seed, same sequence on every machine,
//! so the headless tests are reproducible.

/// Deterministic pseudo-random numbers (not for anything security related).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotRng {
    state: u64,
}

impl BotRng {
    /// A generator for `seed` (any value, 0 included).
    pub fn new(seed: u64) -> Self {
        // SplitMix64: neighbouring seeds (grid slots 0, 1, 2...) give unrelated sequences.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // xorshift never leaves 0.
        Self {
            state: if z == 0 { 0x2545_F491_4F6C_DD1D } else { z },
        }
    }

    /// Next 64 random bits (xorshift64*).
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        // The top 24 bits: exactly representable in an f32.
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `[min, max)`.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.next_f32() < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence_different_seeds_differ() {
        let mut a = BotRng::new(7);
        let mut b = BotRng::new(7);
        let first: Vec<u64> = (0..100).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..100).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
        // Neighbouring seeds (grid slots) do not share their sequence.
        let mut c = BotRng::new(8);
        let third: Vec<u64> = (0..100).map(|_| c.next_u64()).collect();
        assert!(first.iter().zip(&third).all(|(x, y)| x != y));
        // Seed 0 works too.
        let mut zero = BotRng::new(0);
        assert_ne!(zero.next_u64(), zero.next_u64());
    }

    #[test]
    fn floats_are_uniform_in_range() {
        let mut rng = BotRng::new(42);
        let mut sum = 0.0;
        let mut buckets = [0u32; 10];
        for _ in 0..10_000 {
            let x = rng.next_f32();
            assert!((0.0..1.0).contains(&x), "{x}");
            sum += x;
            buckets[(x * 10.0) as usize] += 1;
        }
        let mean = sum / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        for count in buckets {
            assert!((850..1150).contains(&count), "{buckets:?}");
        }
        for _ in 0..1000 {
            let x = rng.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&x));
        }
        let hits = (0..10_000).filter(|_| rng.chance(0.25)).count();
        assert!((2200..2800).contains(&hits), "{hits}");
    }

    /// The exact values are part of the contract: the bots of a given grid slot always drive
    /// the same way, on every machine (values checked against a reference implementation).
    #[test]
    fn the_sequence_is_stable() {
        let mut rng = BotRng::new(1);
        let values: Vec<u64> = (0..3).map(|_| rng.next_u64()).collect();
        assert_eq!(
            values,
            vec![
                0x4b46_a55d_f361_1b9b,
                0xd7e1_f141_0e76_3ef4,
                0x5f14_ec66_975f_9b06
            ]
        );
        assert_eq!(BotRng::new(0).next_u64(), 0x7bbc_b40d_5506_82d0);
    }
}
