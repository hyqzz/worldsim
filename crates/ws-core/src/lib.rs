//! # ws-core
//!
//! Foundation layer of the WorldSim platform.
//!
//! Everything above this layer is built from these primitives:
//! - [`constants`]: CODATA-2022 fundamental physical constants (single source of truth).
//! - [`math`]: fixed-size 3-vector and AABB algebra used by every solver.
//! - [`rng`]: seeded, streamable, fully deterministic PRNG (SplitMix64 seeding + xoshiro256**).
//! - [`hash`]: FNV-1a hashing used for simulation-state fingerprints (determinism proofs).
//! - [`base64`], [`sha1`]: self-contained encodings (used by snapshots and WebSocket).
//! - [`json`]: a strict, dependency-free JSON parser/serializer used by the whole API surface.
//!
//! ## Design rules
//! 1. No third-party dependencies. Every algorithm here is auditable.
//! 2. Deterministic across runs on the same platform: integer-only PRNG math,
//!     bit-exact float serialization for state fingerprints.
//! 3. SI units everywhere. No magic numbers outside [`constants`].

pub mod base64;
pub mod constants;
pub mod hash;
pub mod json;
pub mod math;
pub mod rng;
pub mod sha1;

/// Crate version, surfaced through the HTTP API.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Name of this platform, surfaced through the HTTP API.
pub const NAME: &str = "WorldSim";
