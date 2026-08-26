//! # ws-solvers
//!
//! Physics solvers, each an independent module operating on structure-of-arrays
//! state:
//!
//! | Module | Theory | Scale |
//! |---|---|---|
//! | [`rigid`] | Impulse-based rigid body dynamics (spheres) | macroscopic |
//! | [`fluid`] | Weakly-compressible SPH (Monaghan), Tait EOS | meso/macro |
//! | [`heat`]  | Explicit diffusion + semi-Lagrangian advection on a grid | continuum |
//! | [`nbody`] | Direct-sum softened Newtonian gravitation, leapfrog KDK | planetary |
//!
//! Solvers are deterministic by construction: no implicit RNG use, stable
//! iteration order (index arrays only), fixed integration schemes.
//!
//! Shared neighbor search: [`spatial::UniformGrid`].

pub mod fluid;
pub mod heat;
pub mod law;
pub mod nbody;
pub mod rigid;
pub mod spatial;

/// Solver-layer semantic version of the numerics (bumped when integration
/// behavior changes in a way that alters results).
pub const NUMERICS_VERSION: u32 = 1;
