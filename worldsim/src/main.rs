use std::sync::Arc;

use ws_core::math::Vec3;
use ws_engine::world::World;
use ws_domain::config::{Quality, WorldConfig};
use ws_server::api::start_server;

fn now_hex()->String{
    use std::time::{SystemTime, UNIX_EPOCH};
    let t=SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    format!("{t:09x}{}", t % 997)
}

fn print_help(){
    println!("WorldSim v{} — physics-world simulation infrastructure", ws_core::VERSION);
    println!();
    println!("USAGE:");
    println!("  worldsim serve [--port <PORT>] [--host <HOST>]");
    println!("  worldsim demo  [--port <PORT>] [--host <HOST>]   (serve + pre-create a demo world)");
    println!("  worldsim --help");
    println!("  worldsim --version");
    println!();
    println!("EXAMPLES:");
    println!("  worldsim demo --port 8090");
    println!("  curl http://127.0.0.1:8090/healthz");
    println!("  curl -X POST http://127.0.0.1:8090/v1/worlds -H 'Content-Type: application/json' -d '{{\"name\":\"my-world\"}}'");
    println!();
    println!("VIEWER:  http://<host>:<port>/viewer/");
    println!("API:     http://<host>:<port>/v1/worlds");
    println!("DOCS:    https://github.com/hyqzz/worldsim");
}

fn parse_port(args: &[String])->u16{
    if let Some(pos)=args.iter().position(|a| a=="--port"){
        if let Some(v)=args.get(pos+1){ if let Ok(n)=v.parse::<u16>(){ return n; } }
    }
    8090
}
fn parse_host(args: &[String])->String{
    if let Some(pos)=args.iter().position(|a| a=="--host"){
        if let Some(v)=args.get(pos+1){ return v.clone(); }
    }
    "127.0.0.1".into()
}

fn create_demo_world(state: &Arc<ws_server::api::AppState>)->String{
    let mut cfg=WorldConfig{ quality: Quality::Medium, ..WorldConfig::default() };
    // Keep the demo light enough for 60 Hz interactivity on a laptop.
    cfg.systems.fluid.h = 0.09;
    cfg.systems.heat.enabled = false; // heat diffusion is available via config, but the demo focuses on buoyancy
    let name=Some("demo".to_string());
    let id=format!("demo-{}", now_hex());
    let world=World::new(id.clone(), cfg, name);
    state.worlds.lock().unwrap().insert(id.clone(), std::sync::Arc::new(std::sync::Mutex::new(world)));
    state.snapshots.lock().unwrap().insert(id.clone(), std::collections::HashMap::new());
    let warc=state.worlds.lock().unwrap().get(&id).cloned().unwrap();
    // Inject demo events: fluid blob + balls + heat source, matching viewer "Create demo world" behaviour.
    {
        let mut w=warc.lock().unwrap();
        // Use direct apply (bypass inbox) so demo is immediate without waiting a tick.
        use ws_domain::events::SimEvent;
        let events=vec![
            SimEvent::SpawnFluidBlob{ center: Vec3::new(0.0,1.0,0.0), radius: 0.52 },
            SimEvent::SpawnSphere{ material:"wood".into(), pos: Vec3::new(0.35,2.9,0.2), vel: Vec3::zero(), radius: 0.20 },
            SimEvent::SpawnSphere{ material:"steel".into(), pos: Vec3::new(-0.45,2.9,-0.15), vel: Vec3::zero(), radius: 0.14 },
            SimEvent::SpawnSphere{ material:"rubber".into(), pos: Vec3::new(0.0,2.7,0.45), vel: Vec3::zero(), radius: 0.15 },
            SimEvent::HeatSource{ pos: Vec3::new(-0.95,0.35,0.1), radius: 0.32, power_w: 7000.0, duration_ticks: 0 },
        ];
        w.enqueue_events(events);
        // Step a couple ticks to settle initial fluid a bit before clients connect? Don't block.
        // Let the scheduler handle stepping.
    }
    id
}

fn main(){
    let args: Vec<String> = std::env::args().collect();
    let cmd=args.get(1).map(|s| s.as_str()).unwrap_or("demo");

    if cmd=="--help" || cmd=="-h" || cmd=="help" { print_help(); return; }
    if cmd=="--version" || cmd=="-V" || cmd=="version" { println!("worldsim {}", ws_core::VERSION); return; }

    let host=parse_host(&args);
    let port=parse_port(&args);
    let bind=format!("{host}:{port}");

    let demo = matches!(cmd, "demo");

    println!("WorldSim v{} starting on http://{bind}/", ws_core::VERSION);
    println!("Viewer  http://{bind}/viewer/");
    println!("Health  http://{bind}/healthz");
    println!("API     http://{bind}/v1/worlds");
    println!();

    let state=match start_server(&bind){
        Ok(s)=> s,
        Err(e)=>{ eprintln!("failed to bind {bind}: {e}"); std::process::exit(1); }
    };

    if demo {
        let id=create_demo_world(&state);
        println!("Demo world created: {id}");
        println!("Injecting fluid blob + 3 balls + heat source …");
    } else if cmd=="serve" {
        println!("Serving — create worlds via POST /v1/worlds");
    } else {
        // Unknown command: default to demo for friendliness, but warn.
        if cmd!="demo" && cmd!="serve"{
            eprintln!("unknown command {cmd:?} — defaulting to demo. Try --help");
            let id=create_demo_world(&state);
            println!("Demo world: {id}");
        }
    }

    println!("\nPress Ctrl+C to stop.\n");

    // Block main thread forever (server runs in background threads).
    loop{ std::thread::sleep(std::time::Duration::from_secs(3600)); }
}
