# Changelog

All notable changes to WorldSim are documented here.  The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [0.1.2] — 2025-08-27

### Fixed

- **正确性/确定性**：修复 `sample_velocity` 归一化、修复热源 TTL 按宏观 tick 递减、修复快照遗漏热源/活动力/QoS 状态、`seed_hex` 避免 JSON 截断、`as_vec3` 拒绝非有限值、未知材质返回错误而不是静默回退、`auto_start` 可配置。
- **训练可接入性**：新增 `POST /v1/worlds/:id/step`（`events` + `ticks` 原子执行，保持互斥）、`GET /v1/worlds/:id/state/full` 与 `POST /v1/worlds/:id/observe` 返回 Base64 列式实体数组与探针、`POST /v1/batch/transition` 差异化动作批量、`World::step_exact` 绕过实时调度器、`spawn_nbody`/`remove_nbody`/`spawn_capsule` 事件、固定 worker 数的 batch 阈值与 4096 上限、唯一 ID 校验、WS 流包含 `nbody` 第三组 3D 坐标。
- **3D 显示**：WebGL2 查看器使用持久化 GPU 缓冲、独立着色分支渲染刚体/流体/N-body，并支持地形单击生成；离线提供 `demos/three_d_scene.py` 真 3D 状态 GIF。
- **可发布性**：`cargo fmt` 与 `clippy -D warnings` 全绿、`node --check` 与 SDK `py_compile` 全绿、API 合约与 batch 测试、真实 WebSocket 帧验收、11 个 Demo（含 3D）全部成功。

## [0.1.1] — 2025-08-27

### Added

- **Batch API**: `POST /v1/batch/{create,step,inject}` — 并行 `std::thread::scope` 执行
- **WDL**: `POST /v1/worlds/from-wdl` + `POST /v1/wdl/parse` — 声明式场景描述
- **Capsules**: `SpawnCapsule` 事件（v0.1 体积等价球，v0.2 起几何精确）
- **Law registry**: `ws-solvers::law` — `ForceLaw`/`FluxLaw`/`ConstraintLaw`/`EventLaw` 合约
- **Gym**: `worldsim.gym.WorldSimEnv`（HTTP，原子 step）
- **Training**: `worldsim.training.BatchTrainer`（批量客户端 + `benchmark`）

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
