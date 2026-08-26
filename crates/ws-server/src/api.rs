//! The WorldSim HTTP API — routing, state management, and the live stream hub.

use std::collections::HashMap;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use ws_core::base64;
use ws_core::json::Json;
use ws_core::math::Vec3;

use ws_domain::config::{Quality, WorldConfig};
use ws_domain::events::parse_event_batch;
use ws_domain::wdl::Wdl;
use ws_engine::batch;
use ws_engine::world::World;

use crate::http::{self, Request, Response};
use crate::static_assets;

// ---------- application state ----------

pub struct AppState {
    pub worlds: Mutex<HashMap<String, Arc<Mutex<World>>>>,
    pub snapshots: Mutex<HashMap<String, HashMap<String, Json>>>,
    subscribers: Mutex<HashMap<String, Vec<mpsc::Sender<String>>>>,
    wid_counter: AtomicU64,
    snap_counter: AtomicU64,
}

impl AppState {
    pub fn new()->Self{
        Self{ worlds: Mutex::new(HashMap::new()), snapshots: Mutex::new(HashMap::new()), subscribers: Mutex::new(HashMap::new()), wid_counter: AtomicU64::new(1), snap_counter: AtomicU64::new(1) }
    }
    fn next_world_id(&self)->String{
        let n=self.wid_counter.fetch_add(1, Ordering::Relaxed);
        let t=now_ms();
        format!("{t:013x}{n:05x}")
    }
    fn next_snap_id(&self)->String{
        let n=self.snap_counter.fetch_add(1, Ordering::Relaxed);
        format!("snap_{n:06}")
    }
}

// ---------- helpers ----------

fn now_ms()->u64{
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn json_error(status: u16, code: &str, msg: &str)->Response{
    let j=Json::obj(vec![("error", Json::obj(vec![("code", Json::str(code)), ("message", Json::str(msg.to_string()))]))]);
    Response::new(status).json_body(&j).with_cors()
}
fn json_ok(j: &Json)->Response{ Response::new(200).json_body(j).with_cors() }
fn json_created(j: &Json)->Response{ Response::new(201).json_body(j).with_cors() }

fn parse_body_json(req: &Request)->Result<Json,String>{
    if req.body.is_empty() { return Ok(Json::Null); }
    Json::parse(&String::from_utf8_lossy(&req.body))
}

fn world_not_found(id: &str)->Response{ json_error(404,"world_not_found",&format!("world {id} not found")) }

fn encode_f32_b64(values: &[f64], n: usize, stride: usize)->String{
    // Convert selected values to f32 LE bytes.
    let mut bytes=Vec::with_capacity((n/stride.max(1))*4);
    let step=stride.max(1);
    for i in (0..n).step_by(step){
        bytes.extend_from_slice(&(values[i] as f32).to_le_bytes());
    }
    base64::encode(&bytes)
}
fn encode_u8_b64(values: &[u8], n: usize, stride: usize)->String{
    if stride<=1 { return base64::encode(&values[..n]); }
    let mut out=Vec::with_capacity(n/stride);
    for i in (0..n).step_by(stride){ out.push(values[i]); }
    base64::encode(&out)
}

// Build a streaming frame for a world (caller holds the world lock).
fn build_frame(world: &World)->String{
    let stride=world.qos.limits().stream_stride;
    let n_b=world.rigid.n;
    let n_f=world.fluid.n;
    // bodies
    let b_px=encode_f32_b64(&world.rigid.px, n_b, 1);
    let b_py=encode_f32_b64(&world.rigid.py, n_b, 1);
    let b_pz=encode_f32_b64(&world.rigid.pz, n_b, 1);
    let b_r=encode_f32_b64(&world.rigid.r, n_b, 1);
    let b_mat=encode_u8_b64(&world.rigid.mat_id, n_b, 1);

    // fluid — stride applied
    let f_px=encode_f32_b64(&world.fluid.px, n_f, stride);
    let f_py=encode_f32_b64(&world.fluid.py, n_f, stride);
    let f_pz=encode_f32_b64(&world.fluid.pz, n_f, stride);
    // speed scalar for colour
    let speed: Vec<f64> = (0..n_f).step_by(stride).map(|i| {
        let vx=world.fluid.vx[i]; let vy=world.fluid.vy[i]; let vz=world.fluid.vz[i];
        (vx*vx+vy*vy+vz*vz).sqrt()
    }).collect();
    let f_speed= if speed.is_empty() { String::new() } else {
        let mut b=Vec::with_capacity(speed.len()*4);
        for &v in &speed{ b.extend_from_slice(&(v as f32).to_le_bytes()); }
        base64::encode(&b)
    };
    let eff_n_f = if stride<=1 { n_f } else { n_f.div_ceil(stride) };

    let bounds=Json::obj(vec![
        ("min", Json::arr(vec![Json::num(world.config.bounds.min.x),Json::num(world.config.bounds.min.y),Json::num(world.config.bounds.min.z)])),
        ("max", Json::arr(vec![Json::num(world.config.bounds.max.x),Json::num(world.config.bounds.max.y),Json::num(world.config.bounds.max.z)])),
    ]);
    let tps = if world.config.dt>0.0 { 1.0/world.config.dt } else { 0.0 };
    let stats=Json::obj(vec![
        ("tick", Json::num(world.tick as f64)),
        ("time", Json::num(world.time)),
        ("tps", Json::num(tps)),
        ("tick_ms", Json::num(world.qos.ema())),
        ("bodies", Json::num(n_b as f64)),
        ("fluid", Json::num(n_f as f64)),
        ("quality", Json::str(world.qos.effective.as_str().to_string())),
        ("auto", Json::Bool(!world.qos.locked)),
        ("paused", Json::Bool(world.paused)),
        ("rate", Json::num(world.rate)),
    ]);

    let frame=Json::obj(vec![
        ("t", Json::str("frame")),
        ("tick", Json::num(world.tick as f64)),
        ("time", Json::num(world.time)),
        ("bounds", bounds),
        ("stats", stats),
        ("bodies", Json::obj(vec![
            ("n", Json::num(n_b as f64)),
            ("px", Json::str(b_px)), ("py", Json::str(b_py)), ("pz", Json::str(b_pz)),
            ("r", Json::str(b_r)), ("mat", Json::str(b_mat)),
        ])),
        ("fluid", Json::obj(vec![
            ("n", Json::num(eff_n_f as f64)),
            ("px", Json::str(f_px)), ("py", Json::str(f_py)), ("pz", Json::str(f_pz)),
            ("speed", Json::str(f_speed)),
            ("stride", Json::num(stride as f64)),
        ])),
    ]);
    frame.dump()
}

fn world_summary(world: &World)->Json{
    Json::obj(vec![
        ("id", Json::str(world.id.clone())),
        ("name", world.name.as_deref().map(|s| Json::str(s.to_string())).unwrap_or(Json::Null)),
        ("tick", Json::num(world.tick as f64)),
        ("time", Json::num(world.time)),
        ("paused", Json::Bool(world.paused)),
        ("rate", Json::num(world.rate)),
        ("hash", Json::str(format!("{:016x}", world.state_hash()))),
        ("bodies", Json::num(world.rigid.n as f64)),
        ("fluid", Json::num(world.fluid.n as f64)),
        ("nbody", Json::num(world.nbody.n as f64)),
        ("quality", Json::str(world.qos.effective.as_str().to_string())),
        ("auto_quality", Json::Bool(!world.qos.locked)),
        ("config", world.config.to_json()),
    ])
}

// ---------- REST dispatch ----------

fn dispatch(req: &Request, state: &AppState)->Response{
    // CORS preflight
    if req.method=="OPTIONS"{
        return Response::new(204).with_cors().header("Access-Control-Max-Age","86400");
    }

    // Static viewer + health
    if req.method=="GET" {
        if req.path=="/" { return Response::redirect("/viewer/"); }
        if req.path=="/healthz" {
            return json_ok(&Json::obj(vec![("ok", Json::Bool(true)),("version", Json::str(ws_core::VERSION.to_string())),("name", Json::str(ws_core::NAME.to_string()))]));
        }
        if req.path=="/version"{
            return json_ok(&Json::obj(vec![("version", Json::str(ws_core::VERSION.to_string())),("numerics", Json::num(ws_solvers::NUMERICS_VERSION as f64))]));
        }
        if let Some(asset)=static_assets::get(&req.path){
            return Response::new(200).bytes_body(asset.body.to_vec(), asset.content_type).with_cors();
        }
    }

    // API
    let path=&req.path;
    // Batch & WDL (final form)
    if req.method=="POST" && path=="/v1/batch/create"{ return handle_batch_create(req, state); }
    if req.method=="POST" && path=="/v1/batch/step"{ return handle_batch_step(req, state); }
    if req.method=="POST" && path=="/v1/batch/inject"{ return handle_batch_inject(req, state); }
    if req.method=="POST" && path=="/v1/worlds/from-wdl"{ return handle_create_from_wdl(req, state); }
    if req.method=="POST" && path=="/v1/wdl/parse"{ return handle_wdl_parse(req); }
    // POST /v1/worlds
    if req.method=="POST" && path=="/v1/worlds"{
        return handle_create_world(req, state);
    }
    if req.method=="GET" && path=="/v1/worlds"{
        return handle_list_worlds(state);
    }
    // World-specific sub-routes
    if let Some(rest)=path.strip_prefix("/v1/worlds/"){
        let (id, tail)=match rest.find('/'){
            Some(i)=> (&rest[..i], &rest[i..]),
            None=> (rest, ""),
        };
        // Validate existence early for most routes (except streaming falls through to upgrade handling, but REST streaming not allowed)
        if tail=="/stream"{
            // Handled as upgraded connection — return 400 for plain REST
            return json_error(400,"upgrade_required","use WebSocket Upgrade for /stream or ?format=sse");
        }
        if tail.is_empty() || tail=="/"{
            match req.method.as_str(){
                "GET"=> return handle_get_world(id, state),
                "DELETE"=> return handle_delete_world(id, state),
                _=> return json_error(405,"method_not_allowed",""),
            }
        }
        match (req.method.as_str(), tail){
            ("POST","/control")=> return handle_control(id, req, state),
            ("PUT","/precision")=> return handle_precision(id, req, state),
            ("POST","/events")=> return handle_events(id, req, state),
            ("GET", t) if t.starts_with("/probe")=> return handle_probe(id, req, state),
            ("GET","/state")=> return handle_state(id, state),
            ("POST","/snapshots")=> return handle_create_snapshot(id, state),
            ("GET","/snapshots")=> return handle_list_snapshots(id, state),
            ("POST","/restore")=> return handle_restore(id, req, state),
            ("POST","/fork")=> return handle_fork(id, req, state),
            _ if tail.starts_with("/snapshots/")=> {
                let sid=tail.trim_start_matches("/snapshots/");
                if req.method=="GET"{
                    return handle_get_snapshot(id, sid, state);
                }
            }
            _=>{}
        }
        return json_error(404,"not_found",&format!("no route {tail}"));
    }

    json_error(404,"not_found","route not found")
}

fn handle_create_world(req: &Request, state: &AppState)->Response{
    let body_json = if req.body.is_empty() { Json::Null } else { match Json::parse(&String::from_utf8_lossy(&req.body)) { Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)} };
    let name=body_json.get("name").and_then(|j| j.as_str()).map(|s| s.to_string());
    let cfg_json=body_json.get("config").unwrap_or(&Json::Null);
    let cfg=match WorldConfig::from_json(cfg_json){
        Ok(c)=>c, Err(e)=> return json_error(400,"bad_config",&e)
    };
    let id=state.next_world_id();
    let world=World::new(id.clone(), cfg, name.clone());
    let summary=world_summary(&world);
    state.worlds.lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(world)));
    state.snapshots.lock().unwrap().insert(id.clone(), HashMap::new());
    json_created(&Json::obj(vec![("id", Json::str(id)), ("world", summary)]))
}

fn handle_list_worlds(state: &AppState)->Response{
    let worlds=state.worlds.lock().unwrap();
    let mut list=Vec::new();
    for w in worlds.values(){
        let g=w.lock().unwrap();
        list.push(world_summary(&g));
    }
    // deterministic order by id
    list.sort_by(|a,b| a.get("id").and_then(|j| j.as_str()).cmp(&b.get("id").and_then(|j| j.as_str())));
    json_ok(&Json::obj(vec![("worlds", Json::arr(list))]))
}

fn handle_get_world(id: &str, state: &AppState)->Response{
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=> w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let g=w.lock().unwrap();
    let mut detail=world_summary(&g);
    // enrich with config detail already in summary
    json_ok(&detail)
}

fn handle_delete_world(id: &str, state: &AppState)->Response{
    let mut worlds=state.worlds.lock().unwrap();
    if worlds.remove(id).is_none(){ return world_not_found(id); }
    state.snapshots.lock().unwrap().remove(id);
    state.subscribers.lock().unwrap().remove(id);
    json_ok(&Json::obj(vec![("deleted", Json::str(id.to_string()))]))
}

fn handle_control(id: &str, req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let action=body.get("action").and_then(|j| j.as_str()).unwrap_or("");
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let mut g=w.lock().unwrap();
    match action{
        "pause"=> g.set_paused(true),
        "resume"=> g.set_paused(false),
        "step"=> g.step_once(),
        "set_rate"=> {
            let rate=body.get("rate").and_then(|j| j.as_f64()).unwrap_or(1.0);
            g.set_rate(rate);
        },
        "reset_time"=>{ /* keep tick but reset time? Not in v0.1 */ },
        _=> return json_error(400,"bad_action","action must be pause|resume|step|set_rate"),
    }
    json_ok(&Json::obj(vec![("id", Json::str(id.to_string())),("paused", Json::Bool(g.paused)),("rate", Json::num(g.rate)),("tick", Json::num(g.tick as f64))]))
}

fn handle_precision(id: &str, req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let mut g=w.lock().unwrap();
    if let Some(qs)=body.get("quality").and_then(|j| j.as_str()){
        if qs=="auto"{ g.set_quality_auto(); }
        else if let Some(q)=Quality::parse(qs){ g.set_quality(q); }
        else { return json_error(400,"bad_quality","quality must be low|medium|high|auto"); }
    }
    if let Some(b)=body.get("budget_ms").and_then(|j| j.as_f64()){
        g.set_budget_ms(b);
    }
    json_ok(&Json::obj(vec![("quality", Json::str(g.qos.effective.as_str().to_string())),("auto", Json::Bool(!g.qos.locked)),("budget_ms", Json::num(g.qos.budget_ms))]))
}

fn handle_events(id: &str, req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let events=match parse_event_batch(&body){ Ok(v)=>v, Err(e)=> return json_error(400,"bad_events",&e)};
    let n=events.len();
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    w.lock().unwrap().enqueue_events(events);
    json_ok(&Json::obj(vec![("accepted", Json::num(n as f64))]))
}

fn handle_probe(id: &str, req: &Request, state: &AppState)->Response{
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let g=w.lock().unwrap();
    // query params
    let x: f64 = req.query_param("x").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let y: f64 = req.query_param("y").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let z: f64 = req.query_param("z").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let quantity=req.query_param("quantity").or_else(|| req.query_param("q")).unwrap_or("temperature");
    let pos=Vec3::new(x,y,z);
    let (value, unit)=match quantity{
        "temperature"|"temp"|"T"=> (g.probe_temperature(pos), "K"),
        "density"|"rho"=> (g.probe_density(pos), "kg/m³"),
        "pressure"|"p"=> (g.probe_pressure(pos), "Pa"),
        "velocity"|"speed"|"vel"=> {
            let v=g.probe_velocity(pos);
            let speed=(v.x*v.x+v.y*v.y+v.z*v.z).sqrt();
            return json_ok(&Json::obj(vec![
                ("quantity", Json::str("velocity".to_string())),
                ("value", Json::arr(vec![Json::num(v.x),Json::num(v.y),Json::num(v.z)])),
                ("speed", Json::num(speed)),
                ("unit", Json::str("m/s".to_string())),
                ("tick", Json::num(g.tick as f64)),("time", Json::num(g.time)),
                ("pos", Json::arr(vec![Json::num(x),Json::num(y),Json::num(z)])),
            ]));
        },
        _=> return json_error(400,"bad_quantity","quantity must be temperature|density|pressure|velocity"),
    };
    json_ok(&Json::obj(vec![
        ("quantity", Json::str(quantity.to_string())),
        ("value", Json::num(value)),
        ("unit", Json::str(unit.to_string())),
        ("tick", Json::num(g.tick as f64)),("time", Json::num(g.time)),
        ("pos", Json::arr(vec![Json::num(x),Json::num(y),Json::num(z)])),
    ]))
}

fn handle_state(id: &str, state: &AppState)->Response{
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let g=w.lock().unwrap();
    json_ok(&Json::obj(vec![
        ("id", Json::str(g.id.clone())),
        ("tick", Json::num(g.tick as f64)),("time", Json::num(g.time)),
        ("hash", Json::str(format!("{:016x}", g.state_hash()))),
        ("bodies", Json::num(g.rigid.n as f64)),("fluid", Json::num(g.fluid.n as f64)),
        ("config", g.config.to_json()),
        ("paused", Json::Bool(g.paused)),("rate", Json::num(g.rate)),
        ("replay_log_len", Json::num(g.replay_log_json().as_arr().map(|a| a.len()).unwrap_or(0) as f64)),
    ]))
}

fn handle_create_snapshot(id: &str, state: &AppState)->Response{
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(id){ Some(w)=>w.clone(), None=> return world_not_found(id)};
    drop(worlds);
    let snap={
        let g=w.lock().unwrap();
        g.snapshot_json()
    };
    let sid=state.next_snap_id();
    state.snapshots.lock().unwrap().entry(id.to_string()).or_default().insert(sid.clone(), snap);
    json_created(&Json::obj(vec![("snapshot_id", Json::str(sid)),("world_id", Json::str(id.to_string()))]))
}
fn handle_list_snapshots(id: &str, state: &AppState)->Response{
    if !state.worlds.lock().unwrap().contains_key(id){ return world_not_found(id); }
    let snaps=state.snapshots.lock().unwrap();
    let list=snaps.get(id).map(|m| {
        let mut v: Vec<Json> = m.keys().map(|k| Json::obj(vec![("snapshot_id", Json::str(k.clone()))])).collect();
        v.sort_by(|a,b| a.get("snapshot_id").and_then(|j| j.as_str()).cmp(&b.get("snapshot_id").and_then(|j| j.as_str())));
        v
    }).unwrap_or_default();
    json_ok(&Json::obj(vec![("snapshots", Json::arr(list))]))
}
fn handle_get_snapshot(wid: &str, sid: &str, state: &AppState)->Response{
    if !state.worlds.lock().unwrap().contains_key(wid){ return world_not_found(wid); }
    let snaps=state.snapshots.lock().unwrap();
    let snap=snaps.get(wid).and_then(|m| m.get(sid));
    match snap{
        Some(j)=> json_ok(&Json::obj(vec![("snapshot_id", Json::str(sid.to_string())),("snapshot", j.clone())])),
        None=> json_error(404,"snapshot_not_found","snapshot not found"),
    }
}
fn handle_restore(wid: &str, req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    // Accept {snapshot_id} or {snapshot:{...}}
    let snap_json = if let Some(sid)=body.get("snapshot_id").and_then(|j| j.as_str()){
        let snaps=state.snapshots.lock().unwrap();
        match snaps.get(wid).and_then(|m| m.get(sid)){
            Some(j)=> j.clone(),
            None=> return json_error(404,"snapshot_not_found","snapshot_id not found"),
        }
    } else if let Some(s)=body.get("snapshot"){
        s.clone()
    } else {
        return json_error(400,"bad_request","expected {snapshot_id} or {snapshot:{...}}");
    };
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(wid){ Some(w)=>w.clone(), None=> return world_not_found(wid)};
    drop(worlds);
    let mut g=w.lock().unwrap();
    match g.restore_from_json(&snap_json){
        Ok(())=> json_ok(&Json::obj(vec![("restored", Json::Bool(true)),("tick", Json::num(g.tick as f64))])),
        Err(e)=> json_error(400,"restore_failed",&e),
    }
}
fn handle_fork(wid: &str, req: &Request, state: &AppState)->Response{
    let body=if req.body.is_empty(){ Json::Null } else { match Json::parse(&String::from_utf8_lossy(&req.body)){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)}};
    let name=body.get("name").and_then(|j| j.as_str()).map(|s| s.to_string());
    let worlds=state.worlds.lock().unwrap();
    let w=match worlds.get(wid){ Some(w)=>w.clone(), None=> return world_not_found(wid)};
    drop(worlds);
    let new_id=state.next_world_id();
    let child={
        let g=w.lock().unwrap();
        g.fork(new_id.clone())
    };
    let mut child=child;
    if let Some(n)=name{ child.name=Some(n); }
    let summary=world_summary(&child);
    state.worlds.lock().unwrap().insert(new_id.clone(), Arc::new(Mutex::new(child)));
    state.snapshots.lock().unwrap().insert(new_id.clone(), HashMap::new());
    json_created(&Json::obj(vec![("id", Json::str(new_id)),("world", summary)]))
}

// ---------- Batch API (final form: training & multi-env) ----------

/// POST /v1/batch/create {template_config, count, name_prefix}
fn handle_batch_create(req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let count=body.get("count").and_then(|j| j.as_u64()).filter(|&c| (1..=65536).contains(&c)).unwrap_or(4);
    let cfg_json=body.get("template_config").or_else(|| body.get("config")).unwrap_or(&Json::Null);
    let cfg=match WorldConfig::from_json(cfg_json){ Ok(c)=>c, Err(e)=> return json_error(400,"bad_config",&e) };
    let prefix=body.get("name_prefix").and_then(|j| j.as_str()).unwrap_or("env").to_string();
    let worlds=batch::create_batch(&cfg, count as usize, &prefix);
    let mut ids=Vec::with_capacity(count as usize);
    {
        let mut wm=state.worlds.lock().unwrap();
        let mut sm=state.snapshots.lock().unwrap();
        for w in worlds{
            let id={ let g=w.lock().unwrap(); g.id.clone() };
            ids.push(id.clone());
            sm.insert(id.clone(), HashMap::new());
            wm.insert(id, w);
        }
    }
    json_created(&Json::obj(vec![("count", Json::num(ids.len() as f64)), ("ids", Json::arr(ids.into_iter().map(|id| Json::str(id)).collect()))]))
}

/// POST /v1/batch/step {ids, ticks_per_world}
fn handle_batch_step(req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let ticks=body.get("ticks_per_world").and_then(|j| j.as_u64()).unwrap_or(1) as usize;
    let id_arr: Vec<String> = body.get("ids").and_then(|j| j.as_arr()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if id_arr.is_empty() { return json_error(400,"bad_request","ids array required"); }
    {
        let wm=state.worlds.lock().unwrap();
        let ws: Vec<Arc<Mutex<World>>> = id_arr.iter().filter_map(|id| wm.get(id).cloned()).collect();
        if ws.len()!=id_arr.len() { return json_error(404,"world_not_found","one or more world ids not found"); }
        let results=batch::batch_step(&ws, ticks);
        let result_json: Vec<Json> = results.into_iter().map(|r| Json::obj(vec![
            ("id", Json::str(r.id)),
            ("tick", Json::num(r.tick as f64)),
            ("time", Json::num(r.time)),
            ("hash", Json::str(r.hash)),
            ("tick_ms", Json::num(r.tick_ms)),
            ("bodies", Json::num(r.bodies as f64)),
            ("fluid", Json::num(r.fluid as f64)),
        ])).collect();
        json_ok(&Json::obj(vec![
            ("count", Json::num(result_json.len() as f64)),
            ("results", Json::arr(result_json)),
        ]))
    }
}

/// POST /v1/batch/inject {ids, events}
fn handle_batch_inject(req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let events=match body.get("events").and_then(|j| j.as_arr()).map(|a| { let arr: Vec<Json> = a.to_vec(); parse_event_batch(&Json::Arr(arr)) }).unwrap_or(Ok(vec![])){ Ok(v)=>v, Err(e)=> return json_error(400,"bad_events",&e)};
    let id_arr: Vec<String> = body.get("ids").and_then(|j| j.as_arr()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    let wm=state.worlds.lock().unwrap();
    let mut accepted=0u64;
    for id in &id_arr{
        if let Some(w)=wm.get(id){ w.lock().unwrap().enqueue_events(events.clone()); accepted+=1; }
    }
    json_ok(&Json::obj(vec![("accepted_worlds", Json::num(accepted as f64)),("total_events", Json::num((accepted * events.len() as u64) as f64))]))
}

/// POST /v1/worlds/from-wdl {wdl: "...yaml..."} or {wdl_file: "..."}
fn handle_create_from_wdl(req: &Request, state: &AppState)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let src=match body.get("wdl").and_then(|j| j.as_str()){ Some(s)=>s.to_string(), None=> return json_error(400,"bad_request","wdl string required") };
    let wdl=match Wdl::parse(&src){ Ok(w)=>w, Err(e)=> return json_error(400,"wdl_parse_error",&e) };
    let id=state.next_world_id();
    let mut world=World::new(id.clone(), wdl.config, wdl.name);
    // Enqueue initial events
    if !wdl.events.is_empty(){ world.enqueue_events(wdl.events); }
    let summary=world_summary(&world);
    state.worlds.lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(world)));
    state.snapshots.lock().unwrap().insert(id.clone(), HashMap::new());
    json_created(&Json::obj(vec![("id", Json::str(id)),("world", summary)]))
}

/// POST /v1/wdl/parse {wdl: "..."}
fn handle_wdl_parse(req: &Request)->Response{
    let body=match parse_body_json(req){ Ok(j)=>j, Err(e)=> return json_error(400,"bad_json",&e)};
    let src=match body.get("wdl").and_then(|j| j.as_str()){ Some(s)=>s.to_string(), None=> return json_error(400,"bad_request","wdl string required") };
    match Wdl::parse(&src){
        Ok(wdl)=> json_ok(&Json::obj(vec![
            ("config", wdl.config.to_json()),
            ("name", wdl.name.as_deref().map(|s| Json::str(s.to_string())).unwrap_or(Json::Null)),
            ("events", Json::arr(wdl.events.iter().map(|e| e.to_json()).collect())),
        ])),
        Err(e)=> json_error(400,"wdl_parse_error",&e),
    }
}

// ---------- WebSocket / SSE upgrade handlers ----------

fn handle_websocket_upgrade(req: &Request, stream: std::net::TcpStream, state: Arc<AppState>){
    // Extract world id from path "/v1/worlds/<id>/stream"
    let id = req.path.strip_prefix("/v1/worlds/").and_then(|s| s.strip_suffix("/stream")).or_else(|| req.path.strip_prefix("/v1/worlds/").map(|s| s.split('/').next().unwrap_or(s))).unwrap_or("").trim_matches('/').to_string();
    if id.is_empty() || !state.worlds.lock().unwrap().contains_key(&id){
        let resp=json_error(404,"world_not_found","world not found");
        let _=http::write_response(&mut {let mut s=stream; s}, resp);
        return;
    }
    let key=req.header("sec-websocket-key").unwrap_or("").to_string();
    let accept=http::ws_accept_for(&key);
    let mut s=stream;
    let handshake=format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    if s.write_all(handshake.as_bytes()).is_err(){ return; }
    let _=s.flush();
    s.set_read_timeout(Some(Duration::from_millis(50))).ok();

    // Register subscriber
    let (tx, rx)=mpsc::channel::<String>();
    {
        let mut subs=state.subscribers.lock().unwrap();
        subs.entry(id.clone()).or_default().push(tx);
    }
    // Immediately send current frame so viewer doesn't wait
    if let Some(warc)=state.worlds.lock().unwrap().get(&id).cloned(){
        let frame={ let g=warc.lock().unwrap(); build_frame(&g)};
        let _=http::ws_write_text(&mut s, &frame);
    }
    // Pump loop: read client frames (with timeout) + send broadcast frames
    loop{
        // Client → server (pings/close)
        match http::ws_read_frame(&mut s){
            Ok(Some((opcode, payload)))=>{
                match opcode{
                    0x8=>{ // close
                        let _=http::ws_write_frame(&mut s, 0x8, &payload);
                        break;
                    },
                    0x9=>{ // ping -> pong
                        let _=http::ws_write_frame(&mut s, 0xA, &payload);
                    },
                    _=>{}
                }
            },
            Ok(None)=>{},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock || e.kind()==std::io::ErrorKind::TimedOut =>{},
            Err(_)=> break,
        }
        // Server → client: drain pending broadcasts
        while let Ok(frame)=rx.try_recv(){
            if http::ws_write_text(&mut s, &frame).is_err(){ break; }
        }
        thread::sleep(Duration::from_millis(2));
    }
    // Channel sender lives in map; receiver dropping will cause next broadcast to prune.
}

fn handle_sse(req: &Request, mut stream: std::net::TcpStream, state: Arc<AppState>){
    let id = req.path.strip_prefix("/v1/worlds/").and_then(|s| s.strip_suffix("/stream")).or_else(|| req.path.strip_prefix("/v1/worlds/").map(|s| s.split('/').next().unwrap_or(s))).unwrap_or("").trim_matches('/').to_string();
    if id.is_empty() || !state.worlds.lock().unwrap().contains_key(&id){
        let resp=json_error(404,"world_not_found","world not found");
        let _=http::write_response(&mut stream, resp);
        return;
    }
    let headers="HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
    if stream.write_all(headers.as_bytes()).is_err(){ return; }
    let _=stream.flush();
    let (tx, rx)=mpsc::channel::<String>();
    {
        let mut subs=state.subscribers.lock().unwrap();
        subs.entry(id.clone()).or_default().push(tx);
    }
    // SSE loop: blocking recv per frame
    loop{
        let frame=match rx.recv(){
            Ok(f)=>f,
            Err(_)=> break,
        };
        let payload=format!("data: {}\n\n", frame);
        if stream.write_all(payload.as_bytes()).is_err(){ break; }
        if stream.flush().is_err(){ break; }
    }
}

// ---------- connection handling ----------

fn handle_connection(mut stream: std::net::TcpStream, state: Arc<AppState>){
    stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
    let req_opt=match http::read_one_request(&mut stream){
        Ok(v)=>v,
        Err(e)=>{
            let _=http::write_response(&mut stream, json_error(400,"bad_request",&e.to_string()));
            return;
        }
    };
    let req=match req_opt{ Some(r)=>r, None=> return };

    // Detect upgrades first
    let is_ws=http::is_websocket_upgrade(&req) && req.path.ends_with("/stream");
    let wants_sse = req.query.iter().any(|(k,v)| k=="format" && v=="sse")
        || req.header("accept").map(|v| v.contains("text/event-stream")).unwrap_or(false);

    if is_ws {
        handle_websocket_upgrade(&req, stream, state);
        return;
    }
    if req.path.ends_with("/stream") && wants_sse {
        handle_sse(&req, stream, state);
        return;
    }

    let resp=dispatch(&req, &state);
    let _=http::write_response(&mut stream, resp);
}

// ---------- public server entry ----------

/// Start the HTTP/WebSocket server on `bind` (e.g. `"127.0.0.1:8090"`).
/// Returns the shared state handle for programmatic use.
pub fn start_server(bind: &str)->std::io::Result<Arc<AppState>>{
    let state=Arc::new(AppState::new());
    // scheduler
    {
        let s=state.clone();
        thread::spawn(move|| run_scheduler(s));
    }
    let listener=TcpListener::bind(bind)?;
    listener.set_nonblocking(false)?;
    let s=state.clone();
    thread::spawn(move||{
        for incoming in listener.incoming(){
            match incoming{
                Ok(stream)=>{
                    let st=s.clone();
                    thread::spawn(move|| handle_connection(stream, st));
                },
                Err(_)=> thread::sleep(Duration::from_millis(25)),
            }
        }
    });
    Ok(state)
}

fn run_scheduler(state: Arc<AppState>){
    let mut last=Instant::now();
    loop{
        let now=Instant::now();
        let dt_real=now.duration_since(last).as_secs_f64().min(0.05);
        last=now;

        // Snapshot of world ids to avoid holding lock while ticking
        let ids: Vec<String> = { state.worlds.lock().unwrap().keys().cloned().collect() };
        for wid in &ids{
            let warc_opt={ state.worlds.lock().unwrap().get(wid).cloned() };
            let Some(warc)=warc_opt else { continue };
            // Tick the world (measure wall time for QoS)
            let (did_tick, wall_ms) = {
                let mut w=warc.lock().unwrap();
                if w.paused && w.pending_steps==0 { (false, 0.0) }
                else {
                    // Accumulator-based rate control
                    w.scheduler_accum = (w.scheduler_accum + dt_real * w.rate).min(0.2);
                    let mut ticks_this_iter=0usize;
                    let t0=Instant::now();
                    while w.scheduler_accum >= w.config.dt && ticks_this_iter < 4 {
                        // Don't step more than 4 ticks per scheduler iteration to avoid spiral.
                        let stepped=w.step_tick();
                        if !stepped{ break; }
                        w.scheduler_accum -= w.config.dt;
                        ticks_this_iter+=1;
                    }
                    // If rate is 0, no ticks even though accumulator grew — clamp it.
                    if w.rate==0.0 { w.scheduler_accum=0.0; }
                    let wall=t0.elapsed().as_secs_f64()*1000.0;
                    if ticks_this_iter>0 { w.qos.record(wall / ticks_this_iter as f64); }
                    (ticks_this_iter>0, wall)
                }
            };
            let _=did_tick; let _=wall_ms;
        }

        // Broadcast frames to subscribers (throttled to ~30 Hz effective: every 33 ms we did ticks above, now broadcast)
        {
            let mut subs=state.subscribers.lock().unwrap();
            for (wid, senders) in subs.iter_mut(){
                if senders.is_empty() { continue; }
                let warc_opt={ state.worlds.lock().unwrap().get(wid).cloned() };
                let Some(warc)=warc_opt else { continue };
                let frame={
                    let w=warc.lock().unwrap();
                    build_frame(&w)
                };
                // Send to all, pruning dead ones
                let mut alive: Vec<mpsc::Sender<String>> = Vec::new();
                for tx in senders.drain(..){
                    if tx.send(frame.clone()).is_ok(){
                        alive.push(tx);
                    }
                }
                *senders=alive;
            }
        }

        let elapsed=now.elapsed();
        let period=Duration::from_millis(16);
        if elapsed < period { thread::sleep(period - elapsed); }
    }
}

// Extend World with scheduler accumulator (added via associated field in state, but we patch World directly).
// The field lives in World; we added a slot earlier — ensure it exists.
// If not, this will fail to compile and we patch world.rs.
