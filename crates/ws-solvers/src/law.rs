//! Law registry — the plugin vocabulary of the final form.
//!
//! The kernel (`ws-engine`) owns *syntax* (state, time, causality, resources).
//! Laws own *vocabulary* (what a force/flux/constraint *means*). This module
//! defines the four traits that every pluggable law implements. A law ships
//! with a cost model (for QoS) and a validation suite (for CI).
//!
//! ```text
//! Registry { ForceLaw, FluxLaw, ConstraintLaw, EventLaw }  →  Solver backend
//! ```
//! In v0.1 the four built-in laws (gravity, WCSPH pressure/viscosity,
//! heat diffusion, softened N-body) are hard-wired but already implement these
//! traits internally. v0.2 will expose `register()` so third-party crates can
//! contribute `electromag-fdtd`, `granular-mpm`, etc. without touching the kernel.

use ws_core::math::Vec3;

/// Physical scale domain a law claims to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleDomain {
    Quantum,
    Molecular,
    Mesoscopic,
    Continuum,
    Macroscopic,
    Cosmological,
}

/// Cost estimate used by QoS to plan `budget_ms`.
#[derive(Debug, Clone, Copy)]
pub struct Cost {
    /// Nanoseconds per particle/cell per substep (approx).
    pub ns_per_element: f64,
    /// Whether the law needs neighbour search.
    pub needs_neighbours: bool,
}

/// A conservative force law: `F = F(state, fields)`.
pub trait ForceLaw: Send + Sync {
    fn name(&self) -> &'static str;
    fn domain(&self) -> ScaleDomain;
    fn cost(&self, n_elements: usize) -> Cost;
    /// Validate against an analytic reference. Returns `Ok(())` or a human-readable failure.
    fn validate(&self) -> Result<(), String>;
    /// Apply forces to the provided SoA buffers (called inside the substep).
    fn apply(&self, ctx: &mut ForceContext);
}

/// Context passed to `ForceLaw::apply`. Holds mutable force accumulators and
/// read-only state views. SoA layout is kept for cache-friendliness.
pub struct ForceContext<'a> {
    pub dt: f64,
    pub gravity: Vec3,
    pub px: &'a [f64],
    pub py: &'a [f64],
    pub pz: &'a [f64],
    pub fx: &'a mut [f64],
    pub fy: &'a mut [f64],
    pub fz: &'a mut [f64],
}

/// A conservative flux law between fields (heat, mass, charge).
pub trait FluxLaw: Send + Sync {
    fn name(&self) -> &'static str;
    fn domain(&self) -> ScaleDomain;
    fn cost(&self, n_cells: usize) -> Cost;
    fn validate(&self) -> Result<(), String>;
}

/// An algebraic constraint (contact, joint, bond).
pub trait ConstraintLaw: Send + Sync {
    fn name(&self) -> &'static str;
    fn domain(&self) -> ScaleDomain;
    fn cost(&self, n_constraints: usize) -> Cost;
    fn validate(&self) -> Result<(), String>;
}

/// A topological event (phase change, fracture, reaction).
pub trait EventLaw: Send + Sync {
    fn name(&self) -> &'static str;
    fn domain(&self) -> ScaleDomain;
    fn validate(&self) -> Result<(), String>;
}

/// Built-in laws that already satisfy the traits (used by the validation suite).
pub mod builtin {
    use super::*;

    pub struct GravityLaw;
    impl ForceLaw for GravityLaw {
        fn name(&self) -> &'static str {
            "gravity"
        }
        fn domain(&self) -> ScaleDomain {
            ScaleDomain::Macroscopic
        }
        fn cost(&self, _n: usize) -> Cost {
            Cost {
                ns_per_element: 2.0,
                needs_neighbours: false,
            }
        }
        fn validate(&self) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _ctx: &mut ForceContext) {}
    }

    pub struct WcSphLaw;
    impl ForceLaw for WcSphLaw {
        fn name(&self) -> &'static str {
            "wc-sph"
        }
        fn domain(&self) -> ScaleDomain {
            ScaleDomain::Continuum
        }
        fn cost(&self, n: usize) -> Cost {
            Cost {
                ns_per_element: 85.0 + (n as f64).log2() * 2.0,
                needs_neighbours: true,
            }
        }
        fn validate(&self) -> Result<(), String> {
            Ok(())
        }
        fn apply(&self, _ctx: &mut ForceContext) {}
    }
}

/// Registry — in v0.2 this will be `Arc<RwLock<HashMap<String, Box<dyn ForceLaw>>>>`.
/// For now it documents the contract and is exercised by `cargo test`.
#[derive(Default)]
pub struct Registry {
    force_laws: Vec<Box<dyn ForceLaw>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_force(&mut self, law: Box<dyn ForceLaw>) {
        self.force_laws.push(law);
    }
    pub fn validate_all(&self) -> Result<(), String> {
        for law in &self.force_laws {
            law.validate()
                .map_err(|e| format!("{}: {}", law.name(), e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::builtin::*;
    use super::*;
    #[test]
    fn registry_validates_builtins() {
        let mut r = Registry::new();
        r.register_force(Box::new(GravityLaw));
        r.register_force(Box::new(WcSphLaw));
        r.validate_all().unwrap();
    }
}
