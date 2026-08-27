//! WDL — World Description Language (v0.1 subset).
//!
//! WDL is the declarative scene language of the final form. One document
//! describes a complete world: constants, bounds, materials, entities, laws,
//! QoS. The same WDL runs in the browser WASM mini-kernel, in a batch
//! training job, and in a cloud service.
//!
//! ```yaml
//! {
//!   "world": { "seed": 42, "quality": "medium" },
//!   "entities": [
//!     {"ball": {"material": "steel", "shape": "sphere[r=0.14]", "at": [0, 2.8, 0]}}
//!   ]
//! }
//! ```
//!
//! v0.1 accepts strict JSON. Full YAML support lands in v0.2.
//! JSON comments (`//`) are allowed and stripped by a lightweight pre-pass.

use crate::config::WorldConfig;
use crate::events::SimEvent;
use ws_core::json::Json;

/// A parsed WDL document.
#[derive(Debug, Clone)]
pub struct Wdl {
    pub config: WorldConfig,
    pub name: Option<String>,
    pub events: Vec<SimEvent>,
}

impl Wdl {
    /// Parse a WDL string (JSON with optional `//` comments).
    pub fn parse(input: &str) -> Result<Self, String> {
        let preprocessed = strip_comments(input);
        let json = Json::parse(&preprocessed).map_err(|e| format!("WDL parse: {e}"))?;
        Self::from_json(&json)
    }

    pub fn from_json(v: &Json) -> Result<Self, String> {
        let world = v.get("world").unwrap_or(v);
        let cfg = WorldConfig::from_json(world.get("config").unwrap_or(world))?;
        let name = world
            .get("name")
            .and_then(|j| j.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                v.get("name")
                    .and_then(|j| j.as_str())
                    .map(|s| s.to_string())
            });
        let mut events = Vec::new();

        if let Some(entities) = v.get("entities").and_then(|j| j.as_arr()) {
            for ent in entities {
                if let Json::Obj(pairs) = ent {
                    for (key, val) in pairs {
                        let mat = val
                            .get("material")
                            .and_then(|j| j.as_str())
                            .unwrap_or("steel");
                        let at = val
                            .get("at")
                            .and_then(|j| j.as_vec3())
                            .unwrap_or(ws_core::math::Vec3::new(0.0, 2.0, 0.0));
                        let vel = val
                            .get("vel")
                            .and_then(|j| j.as_vec3())
                            .unwrap_or(ws_core::math::Vec3::zero());
                        let shape_str = val
                            .get("shape")
                            .and_then(|j| j.as_str())
                            .unwrap_or("sphere[r=0.14]");
                        if shape_str.contains("sphere") || mat != "water" {
                            let r = parse_radius(shape_str).unwrap_or(0.14);
                            events.push(SimEvent::SpawnSphere {
                                material: mat.to_string(),
                                pos: at,
                                vel,
                                radius: r,
                            });
                        } else {
                            let r = parse_radius(shape_str).unwrap_or(0.5);
                            events.push(SimEvent::SpawnFluidBlob {
                                center: at,
                                radius: r,
                            });
                        }
                        let _ = key;
                    }
                }
            }
        }

        if let Some(ev_arr) = v.get("events").and_then(|j| j.as_arr()) {
            for ev_json in ev_arr {
                if let Ok(ev) = SimEvent::from_json(ev_json) {
                    events.push(ev);
                }
            }
        }

        Ok(Self {
            config: cfg,
            name,
            events,
        })
    }
}

fn parse_radius(s: &str) -> Option<f64> {
    let start = s.find("r=")?;
    let rest = &s[start + 2..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != 'e' && c != 'E')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Strip `//` line comments, respecting string literals.
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if escape {
            out.push(c);
            escape = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escape = true;
            }
            if c == '"' {
                in_string = false;
            }
            out.push(c);
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == '/' && chars.clone().next() == Some('/') {
            // skip to end of line
            for nc in chars.by_ref() {
                if nc == '\n' {
                    out.push(nc);
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_json_wdl() {
        let src = r#"{"world":{"seed":7,"quality":"low"},"entities":[{"ball":{"material":"wood","shape":"sphere[r=0.2]","at":[0,2,0]}}]}"#;
        let wdl = Wdl::parse(src).unwrap();
        assert_eq!(wdl.config.seed, 7);
        assert_eq!(wdl.events.len(), 1);
    }
    #[test]
    fn parses_json_with_comments() {
        let src = r#"{
            // demo world
            "world": {"seed": 42, "quality": "medium"},
            "entities": [
                {"ball": {"material": "steel", "shape": "sphere[r=0.14]", "at": [0, 2.8, 0]}}
            ]
        }"#;
        let wdl = Wdl::parse(src).unwrap();
        assert_eq!(wdl.config.seed, 42);
        assert_eq!(wdl.events.len(), 1);
    }
}
