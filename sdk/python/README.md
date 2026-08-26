# WorldSim Python SDK

The official Python SDK for [WorldSim](../..) — the physics-world simulation infrastructure platform.

Zero third-party runtime dependencies (stdlib only).

## Install

```bash
pip install -e .          # from sdk/python/
# or, after the package is on PyPI:
pip install worldsim
```

## Quick start

```python
from worldsim import Client

c = Client("http://127.0.0.1:8090")

# Create a world
w = c.create_world(name="hello", config={"quality": "medium"})
wid = w["id"]
print("world", wid)

# Inject a water blob + a wood ball (which floats) + a steel ball (which sinks)
c.inject(wid, [
    {"type": "spawn_fluid", "center": [0, 1.0, 0], "radius": 0.8},
    {"type": "spawn_sphere", "material": "wood",  "pos": [0.4, 2.8, 0], "radius": 0.18},
    {"type": "spawn_sphere", "material": "steel", "pos": [-0.4, 2.8, 0], "radius": 0.12},
])

# Probe the temperature anywhere in space
print(c.probe(wid, 0, 0.5, 0, quantity="temperature"))
print(c.probe(wid, 0, 0.5, 0, quantity="density"))

# Stream live frames (SSE) — Ctrl+C to stop
for frame in c.watch(wid):
    print(f"tick {frame['tick']}  fluid {frame['fluid']['n']}  bodies {frame['bodies']['n']}")
    break  # just the first frame
```

## API surface

| Method | Description |
|---|---|
| `create_world(config, name)` | `POST /v1/worlds` |
| `list_worlds()` | `GET /v1/worlds` |
| `get_world(id)` | `GET /v1/worlds/:id` |
| `delete_world(id)` | `DELETE /v1/worlds/:id` |
| `control(id, action, rate)` | `POST /v1/worlds/:id/control` — `pause` `resume` `step` `set_rate` |
| `set_precision(id, quality, budget_ms)` | `PUT /v1/worlds/:id/precision` |
| `inject(id, events)` | `POST /v1/worlds/:id/events` |
| `probe(id, x, y, z, quantity)` | `GET /v1/worlds/:id/probe` |
| `state(id)` | `GET /v1/worlds/:id/state` |
| `snapshot_create(id)` | `POST /v1/worlds/:id/snapshots` |
| `snapshot_list(id)` | `GET /v1/worlds/:id/snapshots` |
| `snapshot_get(id, sid)` | `GET /v1/worlds/:id/snapshots/:sid` |
| `restore(id, snapshot_id, snapshot)` | `POST /v1/worlds/:id/restore` |
| `fork(id, name)` | `POST /v1/worlds/:id/fork` |
| `watch(id)` | `GET /v1/worlds/:id/stream?format=sse` — yields frame dicts |

See the platform docs for the full HTTP contract: `docs/api.md`.

## Examples

- `examples/quickstart.py` — hello world + probes.
- `examples/floating.py` — wood floats, steel sinks (buoyancy coupling demo).
- `examples/heat.py` — heat source raises temperature, diffusion spreads.
- `examples/orbit.py` — direct N-body two-body orbit energy check (Python-side analytic).
