//! A small seeded random number generator (SplitMix64) and a stable string hash (FNV-1a).
//! Both are fixed algorithms, so a seed gives the same world on every OS and Rust version
//! (the standard library's hashers make no such promise).

/// SplitMix64.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// A generator for one part of the world, independent of every other part: the same
    /// `(seed, key, index)` always gives the same numbers, whatever was generated before.
    pub fn keyed(seed: u64, key: &str, index: u64) -> Self {
        let mut r = Self::new(seed ^ hash(key));
        r.0 ^= r.next_u64().wrapping_add(index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n` > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// `true` with probability `percent`/100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// One element of a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// FNV-1a, 64 bit.
pub fn hash(s: &str) -> u64 {
    s.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3))
}

#[cfg(test)]
mod tests;
