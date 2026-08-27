# WorldSim API Reference

Base URL: `http://<host>:<port>` (default `http://127.0.0.1:8090`). All bodies and responses are `application/json; charset=utf-8`. CORS is enabled (`Access-Control-Allow-Origin: *`).

## Conventions

- Units are SI. Positions/velocities in metres / m/s, temperature in K, density in kg/m³, pressure in Pa.
- All `quantity` probes return `{"quantity","value","unit","tick","time","pos"}`.
- Errors are `{"error":{"code":string,"message":string}}` with the matching HTTP status.
- World ids are 13-hex-digit strings; snapshot ids are `snap_000001` style.
- Ticks are integer `world.tick`; wall-clock `time = tick × dt`.

## Worlds

### `POST /v1/worlds`

Create a world. Body is optional and may contain `name`, `auto_start` and/or `config` (partial — defaults fill the rest, see `docs/configuration.md`). `auto_start:false` creates an explicitly stepped world for RL/robotics so the background scheduler never races the control loop. `seed_hex` preserves full 64-bit seeds; plain `seed` is kept for compatibility.

```json
{ "name": "demo", "auto_start": false, "config": {"seed_hex":"ffff...","quality":"medium","gravity":[0,-9.81,0]} }
→ 201 { "id":"...", "world":{...summary...} }
```

Summary shape (also returned by `GET /v1/worlds/:id`): `{id,name,tick,time,paused,rate,hash,bodies,fluid,nbody,quality,auto_quality,event_errors,config}`.

### `GET /v1/worlds`

List all worlds: `{worlds:[summary,...]}`.

### `GET /v1/worlds/:id` · `DELETE /v1/worlds/:id`

Fetch or delete. Delete also drops snapshots and stream subscriptions.

## Control

### `POST /v1/worlds/:id/control`

`{action:"pause"|"resume"|"step"|"set_rate", rate?:number}`

- `pause` / `resume` — gate the scheduler.
- `step` — when paused, advance exactly one tick on the next scheduler iteration.
- `set_rate` — time-scale (0 = frozen, 1 = real time, 2 = double speed). Clamped 0..16.

## Precision

### `PUT /v1/worlds/:id/precision`

`{quality?:"low"|"medium"|"high"|"auto", budget_ms?:number}`

- `quality:"auto"` unlocks the auto controller (PID-free hysteresis: downgrade after ~60 ticks over budget, upgrade after ~600 ticks well under). Default budget is `dt×1000×0.9` ms.
- Locked quality never moves.

## Events  (`POST /v1/worlds/:id/events`)

Inject causally: `{"events":[{type:...},...]}` (or a bare array). Accepted at most 4096 per batch, applied atomically at the next tick boundary and appended to the replay log.

| `type` | Required fields | Notes |
|---|---|---|
| `spawn_sphere` | `pos:[x,y,z]`, `radius`, `material?`, `vel?` | `material` ∈ steel/aluminum/copper/wood/rubber/glass/ice/ceramic |
| `spawn_capsule` | `pos:[x,y,z]`, `radius`, `half_height?`, `material?`, `vel?` | v0.1 as volume-equivalent sphere, v0.2 true segment distance |
| `spawn_nbody` | `pos:[x,y,z]`, `mass`, `vel?` | Point mass in the N-body subsystem (softened KDK) |
| `remove_nbody` | `index` | Remove N-body mass by index |
| `spawn_fluid` | `center:[x,y,z]`, `radius` | Lattice-filled sphere, jittered; truncated by QoS cap |
| `heat_source` | `pos`, `radius`, `power_watts`, `duration_ticks?` | `duration_ticks` counts macro ticks, `0` = persistent |
| `remove_heat_source` | `index` | index in the world's source list |
| `apply_force` | `target:{body:index}`, `force:[x,y,z]`, `ticks?` | 1 = one tick; >1 persists |
| `set_gravity` | `g:[x,y,z]` | replaces uniform gravity |
| `remove_body` | `index` | swap-remove |

## Probes  (`GET /v1/worlds/:id/probe`)

Query params: `x&y&z` (numbers), `quantity` (or `q`):

- `temperature` (K) — trilinear sample of the heat grid
- `density` (kg/m³) — kernel-weighted SPH estimate
- `pressure` (Pa) — Tait EOS at the sampled density
- `velocity` — returns `{"value":[vx,vy,vz],"speed":...}`

### `POST /v1/worlds/:id/control`

`{action:"pause"|"resume"|"step"|"flush"|"set_rate", rate?: number}` — `step` is now synchronous and exact (`flush` drains the inbox without advancing time).

### `POST /v1/worlds/:id/step`

Atomically enqueue optional `events` and advance `ticks` (1..10000). Returns `{id,stepped,state,observation}` where `observation` holds columnar Base64 `bodies`/`nbody` arrays and, when requested, `fluid` and `fields` (temperature). This is the RL fast path.

```json
{"ticks":1,"events":[{"type":"apply_force","target":{"body":0},"force":[1,0,0]}],"include_fluid":false,"include_fields":false}
```

### `POST /v1/worlds/:id/observe`

Batch `observe` without stepping. Body: `{probes:[{pos:[x,y,z],quantity}],include_fluid,fluid_stride,include_fields}`.

### `GET /v1/worlds/:id/state/full`

Columnar state for the same payload as `/step` `observation`. Query: `?include_fluid&include_fields&fluid_stride=1`.

## State

### `GET /v1/worlds/:id/state`

`{id,tick,time,hash,bodies,fluid,config,paused,rate,replay_log_len,event_errors}`

## Streams

### `GET /v1/worlds/:id/stream`

- **WebSocket** (preferred): client sends `Upgrade: websocket`. Server replies `101` and streams text frames (JSON `{"t":"frame",...}`) at ~30 Hz. Browser viewer uses this.
- **SSE** fallback: add `?format=sse` or `Accept: text/event-stream`. Server streams `data: {json}\n\n` (Python SDK `watch()` uses this — stdlib only).

Frame shape (see `crates/ws-server/src/api.rs::build_frame`):

```json
{
  "t":"frame","tick":123,"time":2.05,
  "bounds":{"min":[-2,0,-2],"max":[2,4,2]},
  "stats":{"tick":123,"time":2.05,"tps":60,"tick_ms":8.2,"bodies":3,"fluid":4200,"quality":"medium","auto":true,"paused":false,"rate":1},
  "bodies":{"n":3,"px":"<b64 f32 LE>","py":"...","pz":"...","r":"...","mat":"<b64 u8>"},
  "fluid": {"n":4200,"px":"...","py":"...","pz":"...","speed":"<b64 f32 LE>","stride":2},
  "nbody": {"n":2,"px":"...","py":"...","pz":"...","r":"..."}
}
```

Fluid positions are `f32` streams; stride is the QoS downsampling factor (1 at `high`).

## Snapshots

### `POST /v1/worlds/:id/snapshots` → `{snapshot_id, world_id}`
### `GET  /v1/worlds/:id/snapshots` → `{snapshots:[{snapshot_id},...]}`
### `GET  /v1/worlds/:id/snapshots/:sid` → `{snapshot_id, snapshot:{...}}`

Snapshot JSON (version 1): `{version:1,tick,time,seed,seed_hex,rng_state_hex,config,rigid:{…},fluid:{…},heat:{…},nbody:{…},heat_sources,active_forces,replay_log,paused,rate,pending_steps,scheduler_accum,qos_effective,qos_locked,qos_budget_ms}`. Arrays are Base64 of little-endian `f64`/`u8`; `seed_hex` is authoritative (`seed` is JSON-lossy above 2^53).

### `POST /v1/worlds/:id/restore`  `{snapshot_id}` or `{snapshot:{...}}` → `{restored:true,tick}`

### `POST /v1/worlds/:id/fork`  `{name?}` → `{id, world}` (child is a bit-exact CoW clone, futures diverge)

## Static & meta

- `GET /` → `302 /viewer/`
- `GET /viewer`, `/viewer/app.js`, `/viewer/style.css`, `/favicon.svg` — embedded assets
- `GET /healthz` → `{ok:true,version,name}` · `GET /version` → `{version,numerics}`
- `OPTIONS *` → `204` with CORS headers
