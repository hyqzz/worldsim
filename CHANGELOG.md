# Changelog

All notable changes to WorldSim are documented here.  The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0] — 2025-08-26

First public release.

### Added

- **ws-core**: CODATA-2022 constants (single source of truth), Vec3/AABB math, splitmix → xoshiro256** deterministic RNG, FNV state hashing, strict JSON parser/serializer, Base64, SHA-1.  Zero third-party dependencies, 22 tests.
- **ws-domain**: `WorldConfig` with partial-JSON parsing and clamping defaults, 8-material library, causal `SimEvent` language (spawn sphere / fluid blob / heat source / apply force / set gravity / remove).  10 tests.
- **ws-solvers**:
  - Rigid spheres — impulse + friction + Baumgarte correction, BVH broad-phase, sleep/wake.
  - WCSPH fluid — cubic-spline kernel (support 2h), Tait EOS, Monaghan viscosity, wall penalty + clamp, lattice blob seeding.
  - Heat field — explicit diffusion + semi-Lagrangian advection on a uniform grid, volumetric sources.
  - Softened N-body — direct-summation leapfrog KDK (planetary scale).
- **ws-engine**: `World` orchestration with fixed-step clock, sub-stepping, buoyancy/drag coupling (Archimedes + quadratic/6πμ drag), event inbox at tick boundaries, replay log, snapshots / restore / fork (CoW), QoS auto controller (PID-free hysteresis), state fingerprint, 7 validation tests (projectile, orbit, heat diffusion, fluid settlement, determinism, snapshot, fork).
- **ws-server**: hand-written HTTP/1.1 + RFC 6455 WebSocket server (no dependencies), JSON API under `/v1`, live streams over WebSocket *and* SSE, embedded WebGL2 viewer served from the binary, scheduler at ~60 Hz with rate control.  Single-binary distribution.
- **Viewer** (`viewer/`): vanilla WebGL2 point-sprite renderer (rigid palette, fluid speed heatmap), wireframe bounds + floor grid, orbit camera, HUD with tick/tps/quality, spawn on click, live WebSocket reconnection.
- **Python SDK** (`sdk/python`): stdlib-only `worldsim.Client` covering every API endpoint plus `watch()` SSE generator and example scripts (`quickstart`, `floating`, `heat`, `orbit`).
- **Docs**: `README.md` + `README.zh-CN.md`, `docs/architecture.md`, `docs/api.md`, `docs/physics.md`, `docs/configuration.md`, `docs/roadmap.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`.
- **CI**: GitHub Actions `test` (Ubuntu + Windows, stable Rust, clippy, fmt) and `sdk` (Python 3.9–3.11 smoke).
