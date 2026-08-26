# WorldSim Physics Notes

This document records the numerical methods shipped in v0.1, their stability limits, and how the validation suite proves each one.  For the architectural vision see `DESIGN.md`.

## 0 · Units and constants

Everything is SI.  Constants are taken from `ws-core/src/constants.rs` (CODATA 2022) — `C`, `H`, `ħ`, `E_CHARGE`, `K_B`, `N_A`, `G`, `MU_0`, `EPSILON_0 = 1/(μ₀c²)`, `G_N = 9.80665` (standard gravity, the platform default).  Solvers must not hard-code physics numbers.

## 1 · Rigid bodies (spheres)

**Model.** Uniform-density spheres, translational state only (6 DoF orientation is approximated as friction, not stored — keeps determinism cheap).  Resting contact uses sleep/wake.

**Integrator.** Symplectic (semi-implicit) Euler:

```
v ← v + (g + F/m)·dt
x ← x + v·dt
```

**Contact.** Impulse `j = −(1+e)·vₙ / (1/m₁+1/m₂)` along the contact normal, plus Baumgarte positional correction `0.8·max(penetration−1e-4,0)/(inv dens)·n`.  Friction: tangential impulse clamped to `μ·|j|` (Coulomb).  Broad-phase is a uniform grid with counting sort, cell `2·max_r`, 27-cell neighbourhood, deterministic cell-major pair order.

**Sleeping.** A body whose `|v|² < 1e-6` while grounded for ~45 frames is put to sleep (velocity zeroed, skipped in integration) and woken by any impulse above `1e-4`.

**Validation.** `projectile_analytic_no_drag`: a single sphere under `g = 9.81` for 2 s matches `x(t)=x₀+v₀t, y(t)=y₀+v₀t−½gt²` within 0.8% (symplectic error O(dt²) at 2 substeps).

## 2 · SPH fluid (WCSPH)

**Kernel.** Cubic spline with support `2h`, `σ = 1/(πh³)` (`q=r/h`):

```
W = σ·(1 − 1.5q²+0.75q³)             q≤1
    σ·0.25·(2−q)³                     1<q≤2
    0                                else
```

**EOS.** Tait: `p = B·((ρ/ρ₀)^γ−1)`, `B=c²ρ₀/γ`, `γ=7`.  Ratio clamped to `[0.6,1.8]` to avoid startup spikes.

**Forces.** For each pair `r<2h`:

```
a_press = −m·(p₁/ρ₁²+p₂/ρ₂²)·∇W
a_visc  = −m·Π·∇W,  Π = −α·c·μ/ρ̄  when v·r<0,  μ=h·(v·r)/(r²+0.01h²)
a_total = a_press + a_visc + g + wall_penalty
```

Wall penalty is a soft spring within `0.8h` of each face, plus a hard clamp `±h/2` from the wall (containment guarantee).

**Neighbour search.** Cell `2h`, so a sphere of radius `2h` lives in 27 cells.  Counting sort over cell indices keeps memory local and order deterministic.

**Substepping.** CFL: `dt ≤ 0.25·h/(c+|v|_max)` and `dt ≤ 0.25·√(h/|a|_max)`.  The engine takes `need=ceil(dt/desired)` substeps capped by the QoS limit (4/10/24 for low/med/high).

**Seeding.** A sphere of `radius` is filled by a lattice with spacing `h/1.3`, jittered by `±0.08h` (SplitMix → xoshiro stream).

**Validation.** `fluid_tank_settles_and_conserves_mass`: ~800 particles, 3 s, mass conserved to `1e-9`, `|v|_max < 1.2 m/s` after settling, bottom-average density ≥ top-average.

## 3 · Heat field

**PDE.** `∂T/∂t = α∇²T` plus semi-Lagrangian advection by the SPH velocity and volumetric sources: `ΔT = P·Δt/(ρcₚ·V_cell·N_cells)`.

**Numerics.** Uniform grid `nx×ny×nz` (default `48³`, tests use smaller).  Advection is unconditionally stable (back-trace + trilinear).  Diffusion is explicit 7-pt with Neumann (insulated) boundaries, sub-stepped so `Δt ≤ dx²/(6α)`.  Sources are distributed uniformly across intersecting cells.

**Sampling.** Trilinear anywhere in the domain (clamped at boundaries).

**Validation.** `heat_1d_cosine_decay`: initial `T(x)=300+40·cos(πx/L)` with insulated walls; the crest amplitude must decay as `exp(−απ²t/L²)` within 5% (medium resolution, `α=0.02`).

## 4 · N-body gravity

**Model.** Direct-summation softened Newtonian force `F = G·m₁m₂·r̂/(r²+ε²)`, `ε` from config.  Symplectic leapfrog KDK (velocity-Verlet):

```
v ← v + a·dt/2,  x ← x+v·dt,  recompute a(x),  v ← v+a·dt/2
```

**Validation.** `circular_orbit_energy_is_conserved`: two `1e10 kg` bodies 2 m apart, circular speed `√(Gm/2d)`, 5 periods, energy drift `<5e-4`, radius drift `<5%`.

## 5 · Coupling  (fluid → rigid)

One-way fluid-to-rigid each fluid sub-step, before rigid integration:

- **Buoyancy** (Archimedes): `Fb = −g·ρ_samp·V`, `ρ_samp` is the SPH density at the body's centre, `V=4π r³/3`.
- **Drag**: `Fd = −v̂·(½·Cd·ρ·A·|v_rel|² + 6πμr·|v_rel|)` with `Cd=0.47`, `μ=0.001 Pa·s`, `v_rel=v_body−v_fluid_samp`.  Sleepers are woken when `|Fb−W| > 0.12·W`.

Result: wood (≈700 kg/m³) floats, steel (≈7850 kg/m³) sinks — the platform's canonical demo and a cross-layer correctness test.

## 6 · Determinism

All ordered loops are index-stable (grid counting sort, SoA arrays).  Randomness comes from `ws_core::rng` (SplitMix → xoshiro256**, integer-only, stream-addressed).  State fingerprints (`ws_core::hash` FNV) cover every arena bit-for-bit, which is how the determinism and snapshot tests prove replayability.
