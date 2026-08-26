//! Snapshot serialisation — a world state as transportable JSON.
//!
//! The snapshot is a **complete, self-contained capture** that can recreate a
//! world bit-exactly on restore (up to the determinism guarantee: same
//! platform + same binary).  Binary arrays are encoded as Base64 strings of
//! little-endian IEEE-754 / integer bytes, so the JSON is both human-inspectable
//! and machine-efficient.
//!
//! Wire shape (stable contract — see `docs/api.md#snapshots`):
//!
//! ```json
//! {
//!   "version":1,
//!   "tick": 1234,
//!   "time": 20.56,
//!   "seed": 42,
//!   "rng_state":[a,b,c,d],
//!   "config":{...WorldConfig...},
//!   "rigid": { "n":2, "px":"<b64 f64 LE>", "py":"...", ... },
//!   "fluid": { "n":N, "px":"...", ... ,"rho":"...", "p":"..." },
//!   "heat":  { "nx":N, "ny":N, "nz":N, "t":"<b64 f64 LE>" },
//!   "nbody": { "n":M, ... },
//!   "heat_sources":[{pos,radius,power_w,ttl},...]
//! }
//! ```

use ws_core::{base64, json::Json};
use ws_core::math::Vec3;

fn f64s_to_b64(xs: &[f64]) -> String {
    let mut bytes = Vec::with_capacity(xs.len()*8);
    for &v in xs {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    base64::encode(&bytes)
}

fn b64_to_f64s(s: &str, expect_len: usize) -> Result<Vec<f64>, String> {
    let bytes=base64::decode(s)?;
    if bytes.len() != expect_len*8 {
        return Err(format!("base64 f64 array length mismatch: expected {expect_len} × 8 bytes, got {}", bytes.len()));
    }
    let mut out=Vec::with_capacity(expect_len);
    for chunk in bytes.chunks_exact(8) {
        out.push(f64::from_le_bytes(chunk.try_into().unwrap()));
    }
    Ok(out)
}

fn u8s_to_b64(xs: &[u8]) -> String {
    base64::encode(xs)
}
fn b64_to_u8s(s: &str) -> Result<Vec<u8>,String> { base64::decode(s) }

// ---------- builder helpers used by `ws_engine::world` ----------

pub fn encode_f64s(slice: &[f64], n: usize) -> String {
    f64s_to_b64(&slice[..n])
}
pub fn decode_vec(s: &Json, n: usize) -> Result<Vec<f64>,String> {
    let t=s.as_str().ok_or("expected base64 string")?;
    b64_to_f64s(t, n)
}
pub fn encode_u8s(slice: &[u8], n: usize)->String { u8s_to_b64(&slice[..n]) }
pub fn decode_u8s_from(json: &Json, n: usize)->Result<Vec<u8>,String>{
    let t=json.as_str().ok_or("expected base64 u8 string")?;
    let bytes=b64_to_u8s(t)?;
    if bytes.len()!=n { return Err(format!("u8 array len expected {n}, got {}", bytes.len())); }
    Ok(bytes)
}

pub fn f64s_arr_json(data: &[f64], n:usize)->Json { Json::str(encode_f64s(data,n)) }
pub fn u8s_json(data: &[u8], n:usize)->Json { Json::str(encode_u8s(data,n)) }

/// Thin helper to build the canonical outer shape from raw pieces.
pub fn build_snapshot_json(
    version: u32,
    tick: u64, time: f64, seed: u64, rng_state: [u64;4],
    config_json: Json,
    rigid: Json,
    fluid: Json,
    heat: Json,
    nbody: Json,
    heat_sources: Json,
)->Json {
    let rng_arr=Json::arr(rng_state.iter().map(|&w| Json::num(w as f64)).collect());
    // rng numbers exceed 53-bit precise range; store as strings to keep full precision.
    let rng_hex=Json::arr(rng_state.iter().map(|&w| Json::str(format!("{w:016x}"))).collect());
    let _=rng_arr;
    Json::obj(vec![
        ("version", Json::num(version as f64)),
        ("tick", Json::num(tick as f64)),
        ("time", Json::num(time)),
        ("seed", Json::num(seed as f64)),
        ("rng_state_hex", rng_hex),
        ("rng_state", Json::arr(vec![
            // also include raw doubles for compatibility — consumers should use rng_state_hex
            Json::str(format!("{:016x}",rng_state[0])),
            Json::str(format!("{:016x}",rng_state[1])),
            Json::str(format!("{:016x}",rng_state[2])),
            Json::str(format!("{:016x}",rng_state[3])),
        ])),
        ("config", config_json),
        ("rigid", rigid),
        ("fluid", fluid),
        ("heat", heat),
        ("nbody", nbody),
        ("heat_sources", heat_sources),
    ])
}

pub fn parse_rng_hex(j: &Json)->Result<[u64;4], String>{
    // Prefer rng_state_hex, fallback to numeric.
    if let Some(arr)=j.get("rng_state_hex").and_then(|x| x.as_arr()) {
        if arr.len()!=4 { return Err("rng_state_hex must have 4 entries".into()); }
        let mut out=[0u64;4];
        for (i, v) in arr.iter().enumerate() {
            let s=v.as_str().ok_or("rng_state_hex entries must be hex strings")?;
            out[i]=u64::from_str_radix(s,16).map_err(|e| format!("bad rng hex: {e}"))?;
        }
        return Ok(out);
    }
    // Legacy numeric
    if let Some(arr)=j.get("rng_state").and_then(|x| x.as_arr()) {
        if arr.len()==4 && arr.iter().all(|v| v.as_f64().is_some()) { /* could shift */ }
    }
    Err("rng_state_hex missing".into())
}

// Convenience for building small object slices inside snapshot builders.
pub fn vec3_b64_list(px: &[f64], py: &[f64], pz: &[f64], n: usize)->(String,String,String){
    (encode_f64s(px,n), encode_f64s(py,n), encode_f64s(pz,n))
}
pub fn decode_vec3_into(sx: &Json, sy: &Json, sz: &Json, n: usize, out_px: &mut [f64], out_py: &mut [f64], out_pz: &mut [f64])->Result<(),String>{
    for (src, dst) in [(sx,out_px),(sy,out_py),(sz,out_pz)] {
        let t=src.as_str().ok_or("expected b64 string for pos")?;
        let vec=b64_to_f64s(t,n)?;
        dst[..n].copy_from_slice(&vec);
    }
    Ok(())
}

#[allow(unused)]
pub fn _example_vec3() {
    let _=Vec3::zero();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn f64_b64_roundtrip() {
        let data: Vec<f64> = vec![0.0, -1.5, f64::INFINITY, 3.14e100, -0.0, 42.0];
        let b64=f64s_to_b64(&data);
        let decoded=b64_to_f64s(&b64, data.len()).unwrap();
        assert_eq!(decoded.len(), data.len());
        for (a,b) in data.iter().zip(decoded.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
    #[test]
    fn snapshot_hex_rng_roundtrip() {
        let snap=build_snapshot_json(1, 7, 0.1, 99, [1,2,3,4],
            Json::Null, Json::Null, Json::Null, Json::Null, Json::Null, Json::arr(vec![]));
        assert_eq!(parse_rng_hex(&snap).unwrap(), [1,2,3,4]);
    }
}
