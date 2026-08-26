//! FNV-1a hashing used to fingerprint simulation state.
//!
//! The hash is computed over raw little-endian bytes of simulation buffers,
//! giving every world state a compact, deterministic fingerprint. This powers
//! the determinism and snapshot-roundtrip validation tests.

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a 64-bit over a byte slice.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Hash the bit pattern of an f64 slice (platform-consistent little-endian).
pub fn hash_f64_slice(h: &mut u64, xs: &[f64]) {
    for &v in xs {
        mix_u64(h, v.to_bits());
    }
}

/// Mix one u64 into a running FNV state.
#[inline]
pub fn mix_u64(h: &mut u64, v: u64) {
    // Fold the u64 in as 8 bytes, most significant last (stable ordering).
    for shift in [0u32, 8, 16, 24, 32, 40, 48, 56] {
        *h ^= (v >> shift) as u8 as u64;
        *h = h.wrapping_mul(FNV_PRIME);
    }
}

/// Convenience: combine multiple pieces into a single fingerprint.
pub struct StateHasher(pub u64);

impl StateHasher {
    pub fn new() -> Self {
        Self(FNV_OFFSET)
    }

    pub fn mix_u64(&mut self, v: u64) -> &mut Self {
        mix_u64(&mut self.0, v);
        self
    }

    pub fn mix_f64(&mut self, v: f64) -> &mut Self {
        mix_u64(&mut self.0, v.to_bits());
        self
    }

    pub fn mix_f64s(&mut self, xs: &[f64]) -> &mut Self {
        hash_f64_slice(&mut self.0, xs);
        self
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for StateHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // Precomputed FNV-1a 64 reference values.
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn float_bit_pattern_is_stable() {
        let mut h1 = StateHasher::new();
        h1.mix_f64(0.1).mix_f64(-273.15);
        let mut h2 = StateHasher::new();
        h2.mix_f64(0.1).mix_f64(-273.15);
        assert_eq!(h1.finish(), h2.finish());

        let mut h3 = StateHasher::new();
        h3.mix_f64(0.1 + f64::EPSILON); // different bits → different hash
        assert_ne!(h1.finish(), h3.finish());
    }
}
