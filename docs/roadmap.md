# WorldSim Roadmap

v0.1 is the *physics-world operating system skeleton* — the contracts are stable but the implementation covers the macro→meso scales most demos need.

| Milestone | Focus | Key deliverables |
|---|---|---|
| **v0.1** *(now)* | Macro + meso, single binary | Rigid spheres + WCSPH fluid + heat grid + softened N-body; buoyancy/drag coupling; snapshot/restore/fork; QoS (global); HTTP/WebSocket/SSE + WebGL2 viewer; Python SDK; 7-problem validation CI |
| **v0.2** | Per-region precision | Octree regions, attention-driven LOD (`docs/DESIGN.md §5`), region snapshots, per-region QoS, material coarse-graining pipeline (MD → transport coefficients) |
| **v0.3** | Horizontal scale | Multi-worker scheduler, world migration, `worldsim cluster` mode, Parquet trajectory export, distributed stream fan-out |
| **v1.0** | Pluggable laws | Law registry traits (Force/Flux/Constraint/Event) + WDL scene language + in-browser WASM mini-kernel + curated law crates (granular, MHD, …) + USD/URDF adapters |

### Contract stability

- Wire API under `/v1` is stable from v0.1.  New endpoints will be added, not changed.
- Snapshot `version:1` is the first frozen format; future versions will ship migration tests.
- The viewer stream frame shape is stable; additional fields may be added but existing ones keep their encoding (Base64 `f32` LE).

### How to steer the roadmap

Open an issue with the label `roadmap` and a concrete use-case (e.g. “I need URDF import for a 7-DoF arm with 10kHz contact”).  Use-cases with an accompanying validation problem and a willingness to review beat feature wishlists.

### What v0.1 deliberately omits (and why)

- **Molecular dynamics** — scales to µs/day per GPU for 1e5 atoms; useful only in tiny hotspots, so it belongs behind v0.2 region LOD rather than as a global default.
- **Distributed execution** — a single well-utilised desktop already sustains 10k rigid bodies / 15k SPH particles / 48³ heat at 30 Hz; distribution is justified only after per-region budgeting proves its worth.
- **WDL / law registry** — shipped as a design contract (`DESIGN.md §4.2`) with a reference implementation pending; shipping it prematurely would freeze the wrong abstraction.
