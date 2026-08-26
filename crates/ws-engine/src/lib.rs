//! # ws-engine
//!
//! The simulation kernel. A [`world::World`] bundles the solver systems with:
//!
//! - a fixed-step clock with multi-rate substepping,
//! - a causally ordered event inbox (injections apply at tick boundaries),
//! - quality-of-service control ([`qos`]): compute-budget-driven degradation,
//! - state snapshots / restore / fork ([`snapshot`]),
//! - a replay log enabling deterministic re-simulation,
//! - a state fingerprint ([`ws_core::hash`]) used to prove determinism.
//!
//! The kernel knows nothing about networking or rendering.

pub mod qos;
pub mod snapshot;
pub mod world;
