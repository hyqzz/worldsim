# WorldSim Architecture — pointed summary

> For the exhaustive design with diagrams, see [`DESIGN.md`](DESIGN.md).  This note maps that design to the v0.1 implementation.

## The idea in one sentence

**Make the physical world a service.**  Keep the kernel's job to *scheduling, state, time, and causality*; keep the laws *pluggable*; keep rendering *a read-only observer*.

## Layers (what shipped in v0.1)

```
Viewer (WebGL2)  ───┐
Python SDK  ────────┤
curl / any client ──┤
        │  HTTP/WebSocket/SSE (ws-server, 0 deps, embedded viewer)
        ▼
   World registry + scheduler (~60 Hz, rate-controlled, QoS)
        │
   World { tick, time, RNG, inbox, replay log, QoS }
        ├── RigidBodies (impulse/BVH, SoA)
        ├── Fluid (WCSPH 2h, Tait, Monaghan)
        ├── HeatField (explicit + semi-Lagrangian)
        └── NBody (KDK, softened)
        │
   Buoyancy + drag coupling (fluid → rigid)
        │
   Snapshot / hash / fork  (Base64 f64 LE, FNV-1a)
```

## Key invariants

- **SI everywhere, constants from `ws-core`.**  No magic numbers in solver code.
- **Deterministic.**  Seeded `SplitMix → xoshiro256**`, sorted grid order, integer ticks.  `cargo test determinism_double_run` proves bit-exact replay on the same platform.
- **Fixed-step.**  Macro `dt = 1/60 s`, fluid/heat sub-step to satisfy CFL, rigid substeps from config, all applied atomically at tick boundaries.
- **QoS as a first-class resource.**  `qos.limits()` maps `low|medium|high` to concrete caps (fluid substeps, particle cap, stream stride) and the controller auto-steps after sustained over/under-budget intervals.

## What "pluggable laws" means later

`DESIGN.md §4.2` defines four traits (`ForceLaw`, `FluxLaw`, `ConstraintLaw`, `EventLaw`).  Each law ships a cost model (so the QoS allocator can plan) and a validation suite (so the platform's correctness grows with the ecosystem).  v0.1 hard-codes the four laws above behind those trait boundaries — adding the registry in v1 will not touch the kernel.

## Supply chain

```
ws-core (0 deps) → ws-domain → ws-solvers → ws-engine → ws-server → worldsim (binary)
```

Every runtime crate stays at **zero third-party dependencies** in v0.1 — the whole server is `std::net` + `std::sync`, so `cargo audit` has nothing to do and the binary builds offline.
