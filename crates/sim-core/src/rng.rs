//! Deterministic random number generator stored in `State`.
//!
//! Hand-written so the exact sequence is identical on native and WASM builds
//! and across Rust versions. Seeded once by `reset(seed)`.
//!
//! Algorithm: SplitMix64. One `u64` of state; each call adds a constant and
//! scrambles the result with shifts, XORs and multiplies. Fast, tiny, and good
//! enough for games.

/// Not `Copy` on purpose: a copied RNG produces the same numbers as the
/// original, and rolling on the copy doesn't advance the real one. Cloning
/// has to be written out (`rng.clone()`), so it can't happen by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Internal state, for the per-tick state hash
    pub const fn state_bits(&self) -> u64 {
        self.state
    }

    pub fn next_u64(&mut self) -> u64 {
        // `wrapping_*` because overflow is intended here: the math is mod
        // 2^64. Plain `+` and `*` would panic, since overflow checks are on.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Upper 32 bits, which are better mixed than the lower ones
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in `0..n`. Panics if `n == 0`
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "below(0) has no valid result");
        // Plain `x % n` is biased: 2^32 values don't split evenly into `n`
        // buckets, so low results come up slightly more often. Rejecting the
        // leftover top values makes every bucket the same size.
        let n = u64::from(n);
        let limit = (1u64 << 32) - (1u64 << 32) % n;
        loop {
            let x = u64::from(self.next_u32());
            if x < limit {
                return (x % n) as u32;
            }
        }
    }

    /// Uniform in `lo..hi` (`hi` excluded). Panics if `lo >= hi`
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo < hi, "empty range {lo}..{hi}");
        let span = (i64::from(hi) - i64::from(lo)) as u32;
        (i64::from(lo) + i64::from(self.below(span))) as i32
    }

    /// True with probability `numerator / denominator`
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        self.below(denominator) < numerator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sequence() {
        // Pins the exact output. If this fails, the algorithm changed, and
        // every saved replay would play out differently.
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn clone_continues_identically() {
        let mut a = Rng::new(7);
        a.next_u64();
        let mut b = a.clone();
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_stays_in_bounds() {
        let mut rng = Rng::new(3);
        for _ in 0..1000 {
            assert!(rng.below(6) < 6);
        }
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut rng = Rng::new(4);
        for _ in 0..1000 {
            let x = rng.range(-5, 5);
            assert!((-5..5).contains(&x), "{x} out of range");
        }
    }

    #[test]
    fn range_handles_full_i32_span() {
        let mut rng = Rng::new(5);
        rng.range(i32::MIN, i32::MAX);
    }

    #[test]
    fn chance_extremes() {
        let mut rng = Rng::new(6);
        for _ in 0..100 {
            assert!(!rng.chance(0, 10));
            assert!(rng.chance(10, 10));
        }
    }
}
