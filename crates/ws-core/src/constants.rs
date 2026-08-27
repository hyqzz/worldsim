//! CODATA-2022 fundamental physical constants.
//!
//! This module is the **single source of truth** for every numeric constant in
//! WorldSim. Solvers must consume values from here (or derive them) — hardcoded
//! physics numbers elsewhere are forbidden by project convention.
//!
//! Values marked *exact* are SI defining constants (fixed by definition since
//! the 2019 SI redefinition). Values marked *measured* carry the CODATA-2022
//! relative uncertainty in the doc comment.

/// Speed of light in vacuum. Exact: `c = 299 792 458 m/s`.
pub const C: f64 = 299_792_458.0;

/// Planck constant. Exact: `h = 6.626 070 15e-34 J s`.
pub const H: f64 = 6.626_070_15e-34;

/// Reduced Planck constant, derived: `ħ = h / 2π`.
pub const H_BAR: f64 = H / core::f64::consts::TAU;

/// Elementary charge. Exact: `e = 1.602 176 634e-19 C`.
pub const E_CHARGE: f64 = 1.602_176_634e-19;

/// Boltzmann constant. Exact: `k_B = 1.380 649e-23 J/K`.
pub const K_B: f64 = 1.380_649e-23;

/// Avogadro constant. Exact: `N_A = 6.022 140 76e23 mol⁻¹`.
pub const N_A: f64 = 6.022_140_76e23;

/// Newtonian constant of gravitation. Measured: `G = 6.674 30(15)e-11 m³ kg⁻¹ s⁻²`
/// (relative uncertainty 2.2e-5).
pub const G: f64 = 6.674_30e-11;

/// Vacuum magnetic permeability. Measured: `μ₀ = 1.256 637 061 27(20)e-6 N A⁻²`.
pub const MU_0: f64 = 1.256_637_061_27e-6;

/// Vacuum electric permittivity, derived: `ε₀ = 1 / (μ₀ c²)`.
pub const EPSILON_0: f64 = 1.0 / (MU_0 * C * C);

/// Standard acceleration of gravity (ISO 80000-3): `gₙ = 9.806 65 m/s²`.
///
/// Note: this is a *convention* value used for unit definitions and as the
/// platform's default uniform gravity. It is not a measurement of local g.
pub const G_N: f64 = 9.806_65;

/// Stefan-Boltzmann constant, derived from other constants:
/// `σ = π²k_B⁴/(60 ħ³ c²) ≈ 5.670 374 419e-8 W m⁻² K⁻⁴`.
pub const STEFAN_BOLTZMANN: f64 =
    core::f64::consts::PI * core::f64::consts::PI * K_B * K_B * K_B * K_B
        / (60.0 * H_BAR * H_BAR * H_BAR * C * C);

/// Molar gas constant, derived: `R = N_A k_B = 8.314 462 618 J mol⁻¹ K⁻¹` (exact).
pub const R_GAS: f64 = N_A * K_B;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_constants_are_consistent() {
        // ε₀ must satisfy the electromagnetic relation α = e²/(4πε₀ħc) ≈ 1/137.036
        let alpha = E_CHARGE * E_CHARGE / (4.0 * core::f64::consts::PI * EPSILON_0 * H_BAR * C);
        let expected = 1.0 / 137.035_999_084;
        let rel = ((alpha - expected) / expected).abs();
        assert!(
            rel < 1e-9,
            "fine structure constant mismatch: {alpha} vs {expected}"
        );

        assert!((STEFAN_BOLTZMANN - 5.670_374_419e-8).abs() < 1e-15);
        assert!((R_GAS - 8.314_462_618).abs() < 1e-8);
    }
}
