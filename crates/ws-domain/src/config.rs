//! World configuration: everything needed to instantiate a universe.
//!
//! A [`WorldConfig`] is a pure data structure; it can be built in code or
//! parsed from JSON (partial documents are fine — every field has a physical,
//! documented default). Configuration is part of world identity: it is hashed
//! into snapshots and replay logs.

use ws_core::json::Json;
use ws_core::math::{Aabb, Vec3};

/// Global quality preset. Maps to concrete solver limits via
/// `ws_engine::qos`. Higher is more accurate and more expensive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    pub fn parse(s: &str) -> Option<Quality> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Some(Quality::Low),
            "medium" => Some(Quality::Medium),
            "high" => Some(Quality::High),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Medium => "medium",
            Quality::High => "high",
        }
    }
}

/// Rigid-body subsystem configuration.
#[derive(Clone, Debug)]
pub struct RigidCfg {
    /// Coefficient of restitution for body–body impacts `[0, 1]`.
    pub restitution: f64,
    /// Coulomb friction coefficient used to clamp tangential impulses.
    pub friction: f64,
    /// Integration substeps per macro tick.
    pub substeps: u32,
}

impl Default for RigidCfg {
    fn default() -> Self {
        Self { restitution: 0.6, friction: 0.35, substeps: 2 }
    }
}

/// WCSPH fluid configuration.
#[derive(Clone, Debug)]
pub struct FluidCfg {
    pub enabled: bool,
    /// Smoothing length (support radius = 2h). Sets spatial resolution.
    pub h: f64,
    /// Rest density kg/m³.
    pub rho0: f64,
    /// Artificial speed of sound m/s. Larger = stiffer = smaller stable dt.
    pub c: f64,
    /// Monaghan artificial viscosity coefficient α.
    pub visc_alpha: f64,
}

impl Default for FluidCfg {
    fn default() -> Self {
        // Water-like defaults tuned for interactive tank scenes:
        // c = 12 m/s gives Mach numbers ≪ 0.1 for typical flows while keeping
        // the CFL-limited dt affordable.
        Self { enabled: true, h: 0.06, rho0: 1000.0, c: 12.0, visc_alpha: 0.08 }
    }
}

/// Grid heat-field configuration.
#[derive(Clone, Debug)]
pub struct HeatCfg {
    pub enabled: bool,
    /// Grid resolution along each axis.
    pub res: [u32; 3],
    /// Thermal diffusivity α = k/(ρc_p) of the tracked medium, m²/s.
    pub alpha: f64,
    /// Volumetric heat capacity ρc_p of the medium, J/(m³·K). Default: water.
    pub volumetric_capacity: f64,
}

impl Default for HeatCfg {
    fn default() -> Self {
        // α of water ≈ 0.143 mm²/s; ρc_p of water = 4.186 MJ/(m³K).
        // Default 16³ keeps the heat field cheap (≈4k cells) while remaining useful.
        // Larger resolutions are available via config, but 48³ is reserved for high-quality offline runs.
        Self {
            enabled: true,
            res: [16, 16, 16],
            alpha: 1.43e-7,
            volumetric_capacity: 4.186e6,
        }
    }
}

/// Direct-sum Newtonian N-body configuration (planetary scale).
#[derive(Clone, Debug)]
pub struct NBodyCfg {
    pub enabled: bool,
    /// Plummer softening length ε (m) preventing singular close encounters.
    pub softening: f64,
}

impl Default for NBodyCfg {
    fn default() -> Self {
        Self { enabled: false, softening: 0.01 }
    }
}

/// Per-system configuration bundle.
#[derive(Clone, Debug, Default)]
pub struct SystemsCfg {
    pub rigid: RigidCfg,
    pub fluid: FluidCfg,
    pub heat: HeatCfg,
    pub nbody: NBodyCfg,
}

/// Complete world configuration.
#[derive(Clone, Debug)]
pub struct WorldConfig {
    /// Deterministic RNG master seed.
    pub seed: u64,
    /// Macro tick duration (s). Simulation advances in integer ticks.
    pub dt: f64,
    /// Simulated domain. With `container = true`, its boundary is a solid wall.
    pub bounds: Aabb,
    pub container: bool,
    /// Uniform gravitational acceleration (m/s²).
    pub gravity: Vec3,
    pub quality: Quality,
    pub systems: SystemsCfg,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            dt: 1.0 / 60.0,
            bounds: Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 4.0, 2.0)),
            container: true,
            gravity: Vec3::new(0.0, -ws_core::constants::G_N, 0.0),
            quality: Quality::Medium,
            systems: SystemsCfg::default(),
        }
    }
}

impl WorldConfig {
    /// Parse a (possibly partial) JSON object into a config, filling defaults.
    ///
    /// Recognized keys (see docs/configuration.md):
    /// `seed`, `dt`, `bounds:{min,max}`, `container`, `gravity`,
    /// `quality`, `systems:{rigid:{...},fluid:{...},heat:{...},nbody:{...}}`.
    pub fn from_json(v: &Json) -> Result<WorldConfig, String> {
        let mut cfg = WorldConfig::default();
        if v.is_null() {
            return Ok(cfg);
        }
        if let Some(seed) = v.get("seed") {
            cfg.seed = seed.as_u64().ok_or("config.seed must be an unsigned integer")?;
        }
        if let Some(dt) = v.get("dt") {
            cfg.dt = dt
                .as_f64()
                .filter(|d| *d > 0.0 && *d <= 0.1)
                .ok_or("config.dt must be in (0, 0.1] seconds")?;
        }
        if let Some(b) = v.get("bounds") {
            let min = b
                .get("min")
                .and_then(|j| j.as_vec3())
                .ok_or("config.bounds.min must be [x,y,z]")?;
            let max = b
                .get("max")
                .and_then(|j| j.as_vec3())
                .ok_or("config.bounds.max must be [x,y,z]")?;
            if !(max.x > min.x && max.y > min.y && max.z > min.z) {
                return Err("config.bounds.max must exceed min on every axis".into());
            }
            cfg.bounds = Aabb::new(min, max);
        }
        if let Some(c) = v.get("container") {
            cfg.container =
                c.as_bool().ok_or("config.container must be a boolean")?;
        }
        if let Some(g) = v.get("gravity") {
            cfg.gravity = g.as_vec3().ok_or("config.gravity must be [x,y,z]")?;
        }
        if let Some(q) = v.get("quality") {
            let s = q.as_str().ok_or("config.quality must be a string")?;
            cfg.quality = Quality::parse(s).ok_or(format!(
                "config.quality must be one of low|medium|high, got {s:?}"
            ))?;
        }
        if let Some(sys) = v.get("systems") {
            parse_systems(sys, &mut cfg.systems)?;
        }
        Ok(cfg)
    }

    /// Compact JSON description (API echo / snapshot metadata).
    pub fn to_json(&self) -> Json {
        let s = &self.systems;
        Json::obj(vec![
            ("seed", Json::num(self.seed as f64)),
            ("dt", Json::num(self.dt)),
            (
                "bounds",
                Json::obj(vec![
                    ("min", vec3_json(self.bounds.min)),
                    ("max", vec3_json(self.bounds.max)),
                ]),
            ),
            ("container", Json::Bool(self.container)),
            ("gravity", vec3_json(self.gravity)),
            ("quality", Json::str(self.quality.as_str())),
            (
                "systems",
                Json::obj(vec![
                    (
                        "rigid",
                        Json::obj(vec![
                            ("restitution", Json::num(s.rigid.restitution)),
                            ("friction", Json::num(s.rigid.friction)),
                            ("substeps", Json::num(s.rigid.substeps as f64)),
                        ]),
                    ),
                    (
                        "fluid",
                        Json::obj(vec![
                            ("enabled", Json::Bool(s.fluid.enabled)),
                            ("h", Json::num(s.fluid.h)),
                            ("rho0", Json::num(s.fluid.rho0)),
                            ("c", Json::num(s.fluid.c)),
                            ("visc_alpha", Json::num(s.fluid.visc_alpha)),
                        ]),
                    ),
                    (
                        "heat",
                        Json::obj(vec![
                            ("enabled", Json::Bool(s.heat.enabled)),
                            ("res", Json::arr(s.heat.res.iter().map(|&r| Json::num(r as f64)).collect())),
                            ("alpha", Json::num(s.heat.alpha)),
                            ("volumetric_capacity", Json::num(s.heat.volumetric_capacity)),
                        ]),
                    ),
                    (
                        "nbody",
                        Json::obj(vec![
                            ("enabled", Json::Bool(s.nbody.enabled)),
                            ("softening", Json::num(s.nbody.softening)),
                        ]),
                    ),
                ]),
            ),
        ])
    }
}

fn parse_systems(v: &Json, out: &mut SystemsCfg) -> Result<(), String> {
    if let Some(r) = v.get("rigid") {
        if let Some(x) = num_field(r, "restitution")? {
            out.rigid.restitution = x.clamp(0.0, 1.0);
        }
        if let Some(x) = num_field(r, "friction")? {
            out.rigid.friction = x.clamp(0.0, 2.0);
        }
        if let Some(x) = r.get("substeps") {
            let v = x.as_u64().ok_or("systems.rigid.substeps must be an integer")?;
            out.rigid.substeps = v.clamp(1, 16) as u32;
        }
    }
    if let Some(f) = v.get("fluid") {
        if let Some(x) = bool_field(f, "enabled")? {
            out.fluid.enabled = x;
        }
        if let Some(x) = num_field(f, "h")? {
            out.fluid.h = x.clamp(0.01, 0.5);
        }
        if let Some(x) = num_field(f, "rho0")? {
            out.fluid.rho0 = x.clamp(1.0, 20000.0);
        }
        if let Some(x) = num_field(f, "c")? {
            out.fluid.c = x.clamp(1.0, 500.0);
        }
        if let Some(x) = num_field(f, "visc_alpha")? {
            out.fluid.visc_alpha = x.clamp(0.0, 2.0);
        }
    }
    if let Some(h) = v.get("heat") {
        if let Some(x) = bool_field(h, "enabled")? {
            out.heat.enabled = x;
        }
        if let Some(arr) = h.get("res") {
            let items = arr.as_arr().filter(|a| a.len() == 3).ok_or("systems.heat.res must be [nx,ny,nz]")?;
            let mut res = [0u32; 3];
            for (dst, src) in res.iter_mut().zip(items) {
                *dst = src
                    .as_u64()
                    .filter(|r| (4..=128).contains(r))
                    .ok_or("systems.heat.res entries in 4..=128")?
                    as u32;
            }
            out.heat.res = res;
        }
        if let Some(x) = num_field(h, "alpha")? {
            out.heat.alpha = x.clamp(1e-9, 1e-2);
        }
        if let Some(x) = num_field(h, "volumetric_capacity")? {
            out.heat.volumetric_capacity = x.clamp(1e3, 1e8);
        }
    }
    if let Some(n) = v.get("nbody") {
        if let Some(x) = bool_field(n, "enabled")? {
            out.nbody.enabled = x;
        }
        if let Some(x) = num_field(n, "softening")? {
            out.nbody.softening = x.max(1e-6);
        }
    }
    Ok(())
}

fn num_field(v: &Json, key: &str) -> Result<Option<f64>, String> {
    match v.get(key) {
        None => Ok(None),
        Some(j) => j.as_f64().map(Some).ok_or(format!("{key} must be a number")),
    }
}

fn bool_field(v: &Json, key: &str) -> Result<Option<bool>, String> {
    match v.get(key) {
        None => Ok(None),
        Some(j) => j.as_bool().map(Some).ok_or(format!("{key} must be a boolean")),
    }
}

fn vec3_json(p: Vec3) -> Json {
    Json::arr(vec![Json::num(p.x), Json::num(p.y), Json::num(p.z)])
}

/// Public helper re-exported for the `events` module and other consumers.
pub fn vec3_json_pub(p: Vec3) -> Json { vec3_json(p) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip_through_json() {
        let cfg = WorldConfig::default();
        let parsed = WorldConfig::from_json(&cfg.to_json()).unwrap();
        assert_eq!(parsed.seed, cfg.seed);
        assert_eq!(parsed.dt, cfg.dt);
        assert_eq!(parsed.gravity, cfg.gravity);
        assert_eq!(parsed.quality, cfg.quality);
        assert_eq!(parsed.systems.fluid.h, cfg.systems.fluid.h);
    }

    #[test]
    fn partial_document_fills_defaults() {
        let v = Json::parse(r#"{"seed":7,"gravity":[0,0,-9.81]}"#).unwrap();
        let cfg = WorldConfig::from_json(&v).unwrap();
        assert_eq!(cfg.seed, 7);
        assert_eq!(cfg.gravity.z, -9.81);
        assert_eq!(cfg.quality, Quality::Medium); // default preserved
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(WorldConfig::from_json(&Json::parse(r#"{"dt":-1}"#).unwrap()).is_err());
        assert!(WorldConfig::from_json(&Json::parse(r#"{"quality":"ultra"}"#).unwrap()).is_err());
        assert!(WorldConfig::from_json(
            &Json::parse(r#"{"bounds":{"min":[0,0,0],"max":[0,5,5]}}"#).unwrap()
        )
        .is_err());
    }

    #[test]
    fn systems_overrides_apply_and_clamp() {
        let v = Json::parse(
            r#"{"systems":{"fluid":{"h":0.05,"enabled":false},"heat":{"res":[16,16,16]},"rigid":{"substeps":99}}}"#,
        )
        .unwrap();
        let cfg = WorldConfig::from_json(&v).unwrap();
        assert!(!cfg.systems.fluid.enabled);
        assert_eq!(cfg.systems.fluid.h, 0.05);
        assert_eq!(cfg.systems.heat.res, [16, 16, 16]);
        assert_eq!(cfg.systems.rigid.substeps, 16); // clamped
    }
}
