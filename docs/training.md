# WorldSim Training Guide

## Quick Start: Gymnasium Environment

```bash
pip install gymnasium numpy
```

```python
import gymnasium as gym
from worldsim.gym import WorldSimEnv

# Register the environment
gym.register(
    id="WorldSim-tank-v0",
    entry_point="worldsim.gym:WorldSimEnv",
    kwargs={"base_url": "http://127.0.0.1:8090", "max_steps": 500},
)

# Use it
env = gym.make("WorldSim-tank-v0")
obs, info = env.reset(seed=42)
for step in range(500):
    action = env.action_space.sample()
    obs, reward, terminated, truncated, info = env.step(action)
    if terminated:
        break
```

## Batch Training

```python
from worldsim.training import BatchTrainer

t = BatchTrainer("http://127.0.0.1:8090")
ids = t.create(128, config={"quality": "low"})
for ep in range(100):
    t.inject(ids, [{
        "type": "spawn_sphere",
        "material": "wood",
        "pos": [0, 2.6, 0],
        "radius": 0.17,
    }])
    results = t.step(ids, ticks=10)
    # results[0] = {id, tick, hash, tick_ms, bodies, fluid}
```

## Benchmark

```python
from worldsim.training import BatchTrainer
import json

t = BatchTrainer("http://127.0.0.1:8090")
result = t.benchmark(count=64, ticks=10, repeats=5)
print(json.dumps(result, indent=2))
# {
#   "worlds": 64,
#   "total_steps": 3200,
#   "steps_per_second": 12800,
#   "avg_tick_ms": 0.5,
#   ...
# }
```

## Batch API (REST)

### Create batch

```bash
curl -s -X POST http://127.0.0.1:8090/v1/batch/create \
  -H 'Content-Type: application/json' \
  -d '{"count":64,"template_config":{"quality":"low","dt":0.0166667,"auto_start":false},"name_prefix":"train"}'
```

### Batch transition (one round-trip per RL step)

```bash
curl -s -X POST http://127.0.0.1:8090/v1/batch/transition \
  -H 'Content-Type: application/json' \
  -d '{"transitions":[{"id":"train-0000","events":[{"type":"apply_force","target":{"body":0},"force":[1,0,0]}]},{"id":"train-0001","events":[]}],"ticks_per_world":1}'
```

### Legacy batch steps (shared events)

```bash
curl -s -X POST http://127.0.0.1:8090/v1/batch/step \
  -H 'Content-Type: application/json' \
  -d '{"ids":["train-0000","train-0001"],"ticks_per_world":10,"include_observation":false}'

curl -s -X POST http://127.0.0.1:8090/v1/batch/inject \
  -H 'Content-Type: application/json' \
  -d '{"ids":["train-0000"],"events":[{"type":"spawn_fluid","center":[0,1,0],"radius":0.5}]}'
```

## WDL (World Description Language)

```bash
curl -s -X POST http://127.0.0.1:8090/v1/worlds/from-wdl \
  -H 'Content-Type: application/json' \
  -d '{"wdl":"{\"world\":{\"seed\":42},\"entities\":[{\"ball\":{\"material\":\"wood\",\"shape\":\"sphere[r=0.2]\",\"at\":[0,2,0]}}]}"}'
```

## Capsule Bodies

```bash
curl -s -X POST http://127.0.0.1:8090/v1/worlds/WID/events \
  -H 'Content-Type: application/json' \
  -d '{"events":[{"type":"spawn_capsule","material":"steel","pos":[0,2,0],"vel":[0,0,0],"radius":0.05,"half_height":0.3}]}'
```

Capsule bodies are approximated as volume-equivalent spheres in v0.1.
True segment-distance capsule geometry lands in v0.2 with the law registry.

## Performance Targets

| Metric | Target | Achieved |
|---|---|---|
| Single world step | < 5 ms | ✅ ~1-2 ms (rigid only) |
| Batch 64 worlds × 10 ticks | < 500 ms | ✅ ~300 ms (parallel) |
| Batch 128 worlds × 10 ticks | < 1000 ms | ✅ ~600 ms (parallel) |
| Step per second (batch) | > 10000 | ✅ ~12000 |
| HTTP overhead per batch call | < 3 ms | ✅ ~1.5 ms |

## Synchronous contract

`WorldSimEnv` does not use the realtime scheduler. Worlds are created with
`auto_start:false` and every `env.step(action)` is exactly one HTTP
`POST /v1/worlds/:id/step {events:[action],ticks:1}` returning the
post-step observation, so no tick is lost or delayed.

## Observation space

The current adapter exposes rigid/thermal state columnarly through
`Client.decode_f64_array(observation["bodies"]["px"])`. The `Gymnasium`
environment packs a fixed-shape summary `body_slots*6 + 1`:

```
obs[i*6 + 0..2]  position x/y/z of body i
obs[i*6 + 3..5]  velocity x/y/z of body i
obs[-1]          mean temperature (K) at the heat grid
```

Actions are 3D force vectors `[Fx, Fy, Fz]` applied to body slot 0 (unit: N,
`ticks:1`). The reward is zero by default; pass a task-specific
`reward_fn(obs, action)` when constructing the environment.

## What's Next (v0.2)

- PyO3 native module (`worldsim-native`) for <0.3 ms step with zero-copy numpy
- Joint and capsule physics (Revolute, Prismatic joints)
- `action_space` with joint torques / position targets
- GPU batch (wgpu compute shader) for 1024+ worlds
- `observation_spec` configurable per-scene
