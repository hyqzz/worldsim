//! The causal event language: every way a client can touch a running world.
//!
//! Events are injected through the API, queued in the kernel inbox, and applied
//! **atomically at tick boundaries** — never mid-step. Every applied event is
//! appended to the replay log with its tick number, which is what makes worlds
//! deterministically reproducible.
//!
//! The JSON encoding defined here (`docs/api.md#events`) is part of the public
//! wire contract.

use crate::config::vec3_json_pub;
use ws_core::json::Json;
use ws_core::math::Vec3;

/// Reference to an existing object inside a world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetRef {
    Body(u32),
}

/// A causally-ordered mutation of a running world.
#[derive(Clone, Debug)]
pub enum SimEvent {
    /// Spawn a dynamic sphere.
    SpawnSphere { material: String, pos: Vec3, vel: Vec3, radius: f64 },
    /// Spawn a capsule (line segment + hemispherical caps). v0.1 executes as
    /// a sphere of equivalent volume for throughput; v0.2 will use true segment distance.
    SpawnCapsule { material: String, pos: Vec3, vel: Vec3, radius: f64, half_height: f64 },
    /// Fill a sphere of space with fluid particles at rest-density lattice spacing.
    SpawnFluidBlob { center: Vec3, radius: f64 },
    /// Add a volumetric heat source. `duration_ticks == 0` means persistent.
    HeatSource { pos: Vec3, radius: f64, power_w: f64, duration_ticks: u64 },
    /// Remove the i-th heat source (index within the world's source list).
    RemoveHeatSource { index: u32 },
    /// Apply a constant extra force (N) to a body for `ticks` ticks (0 = one tick).
    ApplyForce { target: TargetRef, force: Vec3, ticks: u64 },
    /// Replace the uniform gravity vector.
    SetGravity { g: Vec3 },
    /// Remove a body by current index.
    RemoveBody { index: u32 },
}

impl SimEvent {
    /// Wire-format parser. See `docs/api.md#events` for the schema.
    pub fn from_json(v: &Json) -> Result<SimEvent, String> {
        let kind = v
            .get("type")
            .and_then(|j| j.as_str())
            .ok_or("event.type must be a string")?;

        let pos = |key: &str| -> Result<Vec3, String> {
            v.get(key)
                .and_then(|j| j.as_vec3())
                .ok_or(format!("event.{key} must be [x,y,z]"))
        };
        let num = |key: &str| -> Result<f64, String> {
            v.get(key)
                .and_then(|j| j.as_f64())
                .ok_or(format!("event.{key} must be a number"))
        };
        let idx = |key: &str| -> Result<u32, String> {
            v.get(key)
                .and_then(|j| j.as_u64())
                .filter(|i| *i <= u32::MAX as u64)
                .map(|i| i as u32)
                .ok_or(format!("event.{key} must be an index"))
        };

        match kind {
            "spawn_sphere" => Ok(SimEvent::SpawnSphere {
                material: v
                    .get("material")
                    .and_then(|j| j.as_str())
                    .unwrap_or(crate::materials::DEFAULT_MATERIAL)
                    .to_string(),
                pos: pos("pos")?,
                vel: v.get("vel").and_then(|j| j.as_vec3()).unwrap_or(Vec3::zero()),
                radius: {
                    let r = num("radius")?;
                    if !(1e-4..10.0).contains(&r) {
                        return Err("event.radius out of range".into());
                    }
                    r
                },
            }),
            "spawn_fluid" => {
                let r = num("radius")?;
                if !(0.02..20.0).contains(&r) {
                    return Err("event.radius out of range".into());
                }
                Ok(SimEvent::SpawnFluidBlob { center: pos("center")?, radius: r })
            }
            "spawn_capsule" => Ok(SimEvent::SpawnCapsule {
                material: v.get("material").and_then(|j| j.as_str()).unwrap_or(crate::materials::DEFAULT_MATERIAL).to_string(),
                pos: pos("pos")?,
                vel: v.get("vel").and_then(|j| j.as_vec3()).unwrap_or(Vec3::zero()),
                radius: {
                    let r = num("radius")?;
                    if !(1e-4..10.0).contains(&r) { return Err("event.radius out of range".into()); } r
                },
                half_height: {
                    let h = v.get("half_height").or_else(|| v.get("height")).and_then(|j| j.as_f64()).unwrap_or(0.2);
                    if !(0.0..10.0).contains(&h) { return Err("event.half_height out of range".into()); } h
                },
            }),
            "heat_source" => Ok(SimEvent::HeatSource {
                pos: pos("pos")?,
                radius: num("radius")?.clamp(0.01, 50.0),
                power_w: num("power_watts")?,
                duration_ticks: v.get("duration_ticks").and_then(|j| j.as_u64()).unwrap_or(0),
            }),
            "remove_heat_source" => {
                Ok(SimEvent::RemoveHeatSource { index: idx("index")? })
            }
            "apply_force" => {
                let target = v.get("target").ok_or("event.target required")?;
                let body = target
                    .get("body")
                    .and_then(|j| j.as_u64())
                    .filter(|b| *b <= u32::MAX as u64)
                    .ok_or("event.target.body must be a body index")?
                    as u32;
                Ok(SimEvent::ApplyForce {
                    target: TargetRef::Body(body),
                    force: pos("force")?, // same shape as a position triple
                    ticks: v.get("ticks").and_then(|j| j.as_u64()).unwrap_or(1),
                })
            }
            "set_gravity" => Ok(SimEvent::SetGravity { g: pos("g")? }),
            "remove_body" => Ok(SimEvent::RemoveBody { index: idx("index")? }),
            other => Err(format!("unknown event type {other:?}")),
        }
    }

    /// Wire-format writer (replay log export / SDK reference).
    pub fn to_json(&self) -> Json {
        match self {
            SimEvent::SpawnSphere { material, pos, vel, radius } => Json::obj(vec![
                ("type", Json::str("spawn_sphere")),
                ("material", Json::str(material.clone())),
                ("pos", vec3_json_pub(*pos)),
                ("vel", vec3_json_pub(*vel)),
                ("radius", Json::num(*radius)),
            ]),
            SimEvent::SpawnFluidBlob { center, radius } => Json::obj(vec![
                ("type", Json::str("spawn_fluid")),
                ("center", vec3_json_pub(*center)),
                ("radius", Json::num(*radius)),
            ]),
            SimEvent::SpawnCapsule { material, pos, vel, radius, half_height } => Json::obj(vec![
                ("type", Json::str("spawn_capsule")),
                ("material", Json::str(material.clone())),
                ("pos", vec3_json_pub(*pos)),
                ("vel", vec3_json_pub(*vel)),
                ("radius", Json::num(*radius)),
                ("half_height", Json::num(*half_height)),
            ]),
            SimEvent::HeatSource { pos, radius, power_w, duration_ticks } => Json::obj(vec![
                ("type", Json::str("heat_source")),
                ("pos", vec3_json_pub(*pos)),
                ("radius", Json::num(*radius)),
                ("power_watts", Json::num(*power_w)),
                ("duration_ticks", Json::num(*duration_ticks as f64)),
            ]),
            SimEvent::RemoveHeatSource { index } => Json::obj(vec![
                ("type", Json::str("remove_heat_source")),
                ("index", Json::num(*index as f64)),
            ]),
            SimEvent::ApplyForce { target, force, ticks } => Json::obj(vec![
                ("type", Json::str("apply_force")),
                (
                    "target",
                    Json::obj(vec![match target {
                        TargetRef::Body(i) => ("body", Json::num(*i as f64)),
                    }]),
                ),
                ("force", vec3_json_pub(*force)),
                ("ticks", Json::num(*ticks as f64)),
            ]),
            SimEvent::SetGravity { g } => {
                Json::obj(vec![("type", Json::str("set_gravity")), ("g", vec3_json_pub(*g))])
            }
            SimEvent::RemoveBody { index } => Json::obj(vec![
                ("type", Json::str("remove_body")),
                ("index", Json::num(*index as f64)),
            ]),
        }
    }
}

/// Parse a batch event document: `{"events":[...]}` or a bare array.
pub fn parse_event_batch(v: &Json) -> Result<Vec<SimEvent>, String> {
    let arr = match v {
        Json::Arr(_) => v,
        _ => v.get("events").ok_or("expected {\"events\":[...]}")?,
    };
    let items = arr
        .as_arr()
        .ok_or("\"events\" must be an array")?;
    if items.len() > 4096 {
        return Err("too many events in one batch (max 4096)".into());
    }
    items.iter().map(SimEvent::from_json).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_sphere_roundtrip() {
        let src = r#"{"type":"spawn_sphere","material":"wood","pos":[1,2,3],"vel":[0,5,0],"radius":0.15}"#;
        let ev = SimEvent::from_json(&Json::parse(src).unwrap()).unwrap();
        match &ev {
            SimEvent::SpawnSphere { material, pos, vel, radius } => {
                assert_eq!(material, "wood");
                assert_eq!((pos.x, pos.y, pos.z), (1.0, 2.0, 3.0));
                assert_eq!(vel.y, 5.0);
                assert!((radius - 0.15).abs() < 1e-12);
            }
            _ => panic!("wrong variant"),
        }
        // Re-parse our own serialization.
        let re = SimEvent::from_json(&ev.to_json()).unwrap();
        matches!(re, SimEvent::SpawnSphere { .. });
    }

    #[test]
    fn batch_parse_with_unknown_type_fails_cleanly() {
        let bad = Json::parse(r#"{"events":[{"type":"open_pod_bay_doors"}]}"#).unwrap();
        assert!(parse_event_batch(&bad).is_err());
        let good = Json::parse(r#"[{"type":"spawn_fluid","center":[0,1,0],"radius":0.3}]"#)
            .unwrap();
        assert_eq!(parse_event_batch(&good).unwrap().len(), 1);
    }

    #[test]
    fn gravity_event() {
        let ev = SimEvent::from_json(&Json::parse(r#"{"type":"set_gravity","g":[0,-1.62,0]}"#).unwrap())
            .unwrap();
        assert!(matches!(ev, SimEvent::SetGravity { g } if g.y == -1.62));
    }
}
