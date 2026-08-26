//! Deterministic, streamable pseudo-random number generation.
//!
//! WorldSim requires *bit-exact replayability*: given the same seed and the
//! same event log, every simulation must produce identical state on the same
//! platform. To guarantee this we use only integer arithmetic:
//!
//! - Seeding: SplitMix64 (Best 2011) expands `(seed, stream_id)` into four
//!   independent u64 words.
//! - Generation: xoshiro256** — fast, high quality, no floating point involved.
//!
//! Random streams are addressed by `stream_id`, so each subsystem owns an
//! independent, reproducible stream: `Rng::new(seed, STREAM_FLUID)` etc.

/// Stream id reserved for fluid-particle lattice jitter.
pub const STREAM_FLUID_JITTER: u64 = 0xF100_0000_0000_0001;
/// Stream id reserved for future measurement noise models.
pub const STREAM_SENSOR_NOISE: u64 = 0x5E15_0000_0000_0002;

#[inline]
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// xoshiro256** generator with explicit save/restore for snapshots.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Derive an independent stream from `(seed, stream_id)`.
    pub fn new(seed: u64, stream_id: u64) -> Self {
        let mut sm = seed ^ stream_id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        let rng = Self { s };
        debug_assert!(!rng.s.iter().all(|&w| w == 0), "degenerate xoshiro state");
        rng
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1]
            .wrapping_mul(5)
            .rotate_left(7)
            .wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform f64 in `[0, 1)` using 53 random bits — exact and portable.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform f64 in `[a, b)`.
    #[inline]
    pub fn next_range(&mut self, a: f64, b: f64) -> f64 {
        a + self.next_f64() * (b - a)
    }

    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    pub fn set_state(&mut self, s: [u64; 4]) {
        self.s = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sequence() {
        let mut a = Rng::new(42, 1);
        let mut b = Rng::new(42, 1);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn streams_are_independent() {
        let mut a = Rng::new(7, 1);
        let mut b = Rng::new(7, 2);
        let da: Vec<u64> = (0..16).map(|_| a.next_u64()).collect();
        let db: Vec<u64> = (0..16).map(|_| b.next_u64()).collect();
        assert_ne!(da, db);
    }

    #[test]
    fn f64_in_unit_interval_and_reproducible() {
        let mut r = Rng::new(u64::MAX, STREAM_FLUID_JITTER);
        let vals: Vec<f64> = (0..10_000).map(|_| r.next_f64()).collect();
        assert!(vals.iter().all(|&v| (0.0..1.0).contains(&v)));
        let mut r2 = Rng::new(u64::MAX, STREAM_FLUID_JITTER);
        let vals2: Vec<f64> = (0..10_000).map(|_| r2.next_f64()).collect();
        assert_eq!(vals, vals2);
    }

    #[test]
    fn state_roundtrip() {
        let mut r = Rng::new(123, 9);
        let _ = r.next_u64();
        let st = r.state();
        let expect = r.next_u64();
        let mut r2 = Rng::new(0, 0);
        r2.set_state(st);
        assert_eq!(r2.next_u64(), expect);
    }
}
