//! The material library.
//!
//! Densities are standard engineering references (room temperature). Colors
//! are linear RGB triples used by the viewer. Materials are *labels* here;
//! their mechanical effect enters through density (mass) at spawn time.

/// A spawnable solid material.
#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub name: &'static str,
    /// kg/m³
    pub density: f64,
    /// Linear RGB for the viewer.
    pub color: [f32; 3],
}

/// Look up a material by (case-insensitive) name.
pub fn lookup(name: &str) -> Option<Material> {
    const LIB: [Material; 8] = [
        Material { name: "steel", density: 7850.0, color: [0.58, 0.60, 0.63] },
        Material { name: "aluminum", density: 2700.0, color: [0.78, 0.79, 0.80] },
        Material { name: "copper", density: 8960.0, color: [0.85, 0.53, 0.35] },
        Material { name: "wood", density: 700.0, color: [0.62, 0.44, 0.26] },
        Material { name: "rubber", density: 1100.0, color: [0.22, 0.22, 0.24] },
        Material { name: "glass", density: 2500.0, color: [0.75, 0.88, 0.90] },
        Material { name: "ice", density: 917.0, color: [0.80, 0.92, 0.98] },
        Material { name: "ceramic", density: 3800.0, color: [0.90, 0.87, 0.80] },
    ];
    LIB.into_iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Default when a spawn event omits the material.
pub const DEFAULT_MATERIAL: &str = "steel";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(lookup("STEEL").unwrap().density, 7850.0);
        assert!(lookup("unobtanium").is_none());
    }

    #[test]
    fn wood_floats_steel_sinks_in_water() {
        assert!(lookup("wood").unwrap().density < 1000.0);
        assert!(lookup("steel").unwrap().density > 1000.0);
    }

    #[test]
    fn every_material_has_a_valid_color() {
        for name in ["steel", "aluminum", "copper", "wood", "rubber", "glass", "ice", "ceramic"] {
            let m = lookup(name).unwrap();
            assert!(m.color.iter().all(|&c| (0.0..=1.0).contains(&c)));
        }
    }
}
