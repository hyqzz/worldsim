# WorldSim Configuration

`WorldConfig` is the complete description of a world.  It can be built in code (`WorldConfig::default()`) or parsed from JSON — partial documents are fine, every field has a physical default (see `crates/ws-domain/src/config.rs`).

## Top-level

```json
{
  "seed": 42,
  "dt": 0.0166667,
  "bounds": {"min": [-2, 0, -2], "max": [2, 4, 2]},
  "container": true,
  "gravity": [0, -9.80665, 0],
  "quality": "medium",
  "systems": { ... }
}
```

| Key | Type | Default | Notes |
|---|---|---|---|
| `seed` | u64 | 42 | Master RNG seed. Streams are derived as `seed ^ stream_id`. |
| `dt` | f64 (s) | 1/60 | Macro tick. Clamped (0, 0.1]. Snapshots record time as `tick×dt`. |
| `bounds` | `{min,max}` (each `[x,y,z]`) | `[-2,0,-2]→[2,4,2]` | Domain. When `container:true` its faces are solid walls for rigid+fluid. |
| `container` | bool | true | Whether the domain boundary confines matter. |
| `gravity` | `[x,y,z]` | `[0,−gₙ,0]` | Uniform acceleration; `gₙ=9.80665` from `ws-core`. Replace with any vector (e.g. lunar `[0,−1.62,0]`). |
| `quality` | `"low"\|"medium"\|"high"` | `"medium"` | Only this field affects QoS limits; the rest is physics. |

## `systems.rigid`

```json
{"restitution": 0.6, "friction": 0.35, "substeps": 2}
```

| Key | Range | Meaning |
|---|---|---|
| `restitution` | [0,1] | `e` for body–body impacts (clamped with per-body values). |
| `friction` | [0,2] | Coulomb μ, clamped on the tangential impulse. |
| `substeps` | 1..16 | Integration substeps per macro tick (clamped). |

## `systems.fluid`

```json
{"enabled": true, "h": 0.06, "rho0": 1000, "c": 12, "visc_alpha": 0.08}
```

| Key | Range | Meaning |
|---|---|---|
| `enabled` | bool | Disable to run rigid/thermal worlds without fluid cost. |
| `h` | 0.01..0.5 | Smoothing length. Smaller → finer but `N ∝ 1/h³` and `dt ∝ h`. |
| `rho0` | 1..20000 | Rest density (water ≈1000). |
| `c` | 1..500 | Artificial speed of sound. Larger → stiffer (less compressibility) but smaller CFL dt. Default 12 keeps `Mach < 0.1` for tank flows. |
| `visc_alpha` | 0..2 | Monaghan α. |

## `systems.heat`

```json
{"enabled": true, "res": [48,48,48], "alpha": 1.43e-7, "volumetric_capacity": 4186000}
```

| Key | Range | Meaning |
|---|---|---|
| `enabled` | bool | |
| `res` | [4..128]³ | Grid resolution. Cost ∝ `nx·ny·nz` per substep. Tests often use `[16,4,4]` for speed. |
| `alpha` | 1e-9..1e-2 | Diffusivity `α=k/(ρcₚ)` m²/s. Water `1.43e-7`; air `1.9e-5`; metal `~1e-4`. |
| `volumetric_capacity` | 1e3..1e8 | `ρcₚ` J/(m³·K). Default water. |

## `systems.nbody`

```json
{"enabled": false, "softening": 0.01}
```

| Key | Meaning |
|---|---|
| `enabled` | Planetary direct-summation path. |
| `softening` | Plummer `ε` preventing singular close encounters. |

## Example  (lunar, high-res, no heat)

```json
{
  "seed": 7,
  "quality": "high",
  "gravity": [0, -1.62, 0],
  "bounds": {"min": [-4,0,-4], "max": [4,6,4]},
  "systems": {"fluid": {"h": 0.045}, "heat": {"enabled": false}, "rigid": {"friction": 0.5}}
}
```

## QoS  (runtime, not part of the stored snapshot)

`PUT /v1/worlds/:id/precision {"quality":"high","budget_ms":8}`

`budget_ms` gates the auto controller (PID-free hysteresis — 60 ticks over → downgrade, 600 well under → upgrade). See `crates/ws-engine/src/qos.rs`.
