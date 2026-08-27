//! The simulation world — the unit of execution.

use ws_core::hash::StateHasher;
use ws_core::json::Json;
use ws_core::math::Vec3;
use ws_core::rng::{Rng, STREAM_FLUID_JITTER};

use ws_domain::config::{Quality, WorldConfig};
use ws_domain::events::{SimEvent, TargetRef};
use ws_domain::materials;

use ws_solvers::fluid::Fluid;
use ws_solvers::heat::HeatField;
use ws_solvers::nbody::NBodySystem;
use ws_solvers::rigid::{RigidBodies, RigidParams};

use crate::qos::{limits_for, QosController};
use crate::snapshot;

// Material palette index — matches `ws_domain::materials` ordering plus extra.
fn material_palette_index(name: &str) -> u8 {
    const ORDER: &[&str] = &[
        "steel", "aluminum", "copper", "wood", "rubber", "glass", "ice", "ceramic",
    ];
    for (i, &n) in ORDER.iter().enumerate() {
        if n.eq_ignore_ascii_case(name) {
            return i as u8;
        }
    }
    // fallback steel
    0
}
fn material_density(name: &str) -> Result<f64, String> {
    materials::lookup(name)
        .map(|m| m.density)
        .ok_or_else(|| format!("unknown material {name:?}"))
}

/// One active transient force.
#[derive(Clone, Debug)]
struct ActiveForce {
    target: TargetRef,
    force: Vec3,
    remaining: u64, // ticks left
}

/// A complete simulation world.
pub struct World {
    pub id: String,
    pub name: Option<String>,
    pub config: WorldConfig,
    pub tick: u64,
    pub time: f64,
    pub rng: Rng,

    pub rigid: RigidBodies,
    pub fluid: Fluid,
    pub heat: HeatField,
    pub nbody: NBodySystem,

    // causal event inbox: events enqueued by the API, waiting for the next tick boundary
    inbox: Vec<SimEvent>,
    // replay log: all applied events with their tick
    replay_log: Vec<(u64, Json)>,

    pub qos: QosController,
    pub paused: bool,
    pub rate: f64,
    pub pending_steps: u64,
    /// Most recent event-application errors, retained for diagnostics.
    pub event_errors: Vec<String>,

    active_forces: Vec<ActiveForce>,

    /// Accumulator for the scheduler's rate-controlled tick pacing (seconds of world time pending).
    pub scheduler_accum: f64,

    pub created_at_ms: u64,
}

// Manual Debug skipping large arenas
impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("id", &self.id)
            .field("tick", &self.tick)
            .field("rigid_n", &self.rigid.n)
            .field("fluid_n", &self.fluid.n)
            .finish()
    }
}

impl World {
    pub fn new(id: String, config: WorldConfig, name: Option<String>) -> Self {
        let budget_ms = (config.dt * 1000.0 * 0.90).clamp(5.0, 30.0);
        let auto_start = config.auto_start;
        let qos = QosController::new(config.quality, budget_ms);
        let limits = limits_for(config.quality);

        // Allocate according to the requested quality. This matters for batch
        // training: a low-quality 1024-world batch must not reserve 48k fluid
        // particles and 8k rigid slots per environment.
        let rigid_cap = match config.quality {
            Quality::Low => 1024,
            Quality::Medium => 4096,
            Quality::High => 8192,
        };
        let rigid = RigidBodies::new(rigid_cap);

        let nbody_cap = 256;
        let nbody = NBodySystem::new(
            nbody_cap,
            config.systems.nbody.softening,
            ws_core::constants::G,
        );
        let fluid = Fluid::new(
            limits.max_fluid_particles,
            config.systems.fluid.h,
            config.systems.fluid.rho0,
            config.systems.fluid.c,
            config.systems.fluid.visc_alpha,
            config.bounds,
        );

        let heat = HeatField::new(
            config.bounds,
            config.systems.heat.res,
            config.systems.heat.alpha,
            config.systems.heat.volumetric_capacity,
        );

        let rng = Rng::new(config.seed, 0x51A);

        Self {
            id,
            name,
            config,
            tick: 0,
            time: 0.0,
            rng,
            rigid,
            fluid,
            heat,
            nbody,
            inbox: Vec::new(),
            replay_log: Vec::new(),
            qos,
            paused: !auto_start,
            rate: 1.0,
            pending_steps: 0,
            event_errors: Vec::new(),
            active_forces: Vec::new(),
            scheduler_accum: 0.0,
            created_at_ms: now_ms(),
        }
    }

    // ---------- event intake ----------

    pub fn enqueue_events(&mut self, events: Vec<SimEvent>) {
        self.inbox.extend(events);
    }

    fn drain_and_apply(&mut self) {
        if self.inbox.is_empty() {
            return;
        }
        let events = std::mem::take(&mut self.inbox);
        for ev in events {
            let json = ev.to_json();
            let tick = self.tick; // will be the tick this batch is applied at
            let res = self.apply_event(ev);
            // Still log even if apply failed — preserves causality for debugging.
            self.replay_log.push((tick, json));
            if let Err(msg) = res {
                self.event_errors.push(format!("tick {tick}: {msg}"));
                if self.event_errors.len() > 128 {
                    self.event_errors.remove(0);
                }
            }
        }
    }

    fn apply_event(&mut self, ev: SimEvent) -> Result<(), String> {
        match ev {
            SimEvent::SpawnSphere {
                material,
                pos,
                vel,
                radius,
            } => {
                let density = material_density(&material)?;
                let mat_id = material_palette_index(&material);
                let rest = self.config.systems.rigid.restitution;
                self.rigid.spawn(pos, vel, radius, density, rest, mat_id)?;
                Ok(())
            }
            SimEvent::SpawnCapsule {
                material,
                pos,
                vel,
                radius,
                half_height,
            } => {
                // v0.1: capsule approximated as volume-equivalent sphere for throughput;
                // true segment-distance capsule lands in v0.2 (law registry).
                let density = material_density(&material)?;
                let mat_id = material_palette_index(&material);
                let rest = self.config.systems.rigid.restitution;
                let vol_capsule = std::f64::consts::PI
                    * radius
                    * radius
                    * (2.0 * half_height + 4.0 / 3.0 * radius);
                let eq_radius =
                    ((3.0 * vol_capsule) / (4.0 * std::f64::consts::PI)).powf(1.0 / 3.0);
                self.rigid
                    .spawn(pos, vel, eq_radius.clamp(0.02, 2.0), density, rest, mat_id)?;
                Ok(())
            }
            SimEvent::SpawnFluidBlob { center, radius } => {
                if !self.config.systems.fluid.enabled {
                    return Err("fluid subsystem is disabled".into());
                }
                let limits = self.qos.limits();
                let free = limits.max_fluid_particles.saturating_sub(self.fluid.n);
                if free == 0 {
                    return Ok(());
                } // at capacity — silently ignore extra, QoS governs.
                  // Rough lattice count estimate: sphere vol / dp³
                let before = self.fluid.n;
                let mut local_rng = Rng::new(
                    self.config.seed ^ (self.tick.wrapping_mul(0x9E3779B9)),
                    STREAM_FLUID_JITTER.wrapping_add(self.fluid.n as u64),
                );
                let jitter = self.fluid.h * 0.08;
                let added = self
                    .fluid
                    .spawn_blob(center, radius, jitter, &mut local_rng);
                // Truncate if over limit after spawn (spawn may have overshot by a few due to lattice).
                if self.fluid.n > limits.max_fluid_particles {
                    self.fluid.n = limits.max_fluid_particles;
                }
                let _ = added;
                let _ = before;
                Ok(())
            }
            SimEvent::SpawnNBody { mass, pos, vel } => {
                if !self.config.systems.nbody.enabled {
                    return Err("n-body subsystem is disabled".into());
                }
                self.nbody.add_body(mass, pos, vel)?;
                Ok(())
            }
            SimEvent::RemoveNBody { index } => self.nbody.remove(index),
            SimEvent::HeatSource {
                pos,
                radius,
                power_w,
                duration_ticks,
            } => {
                if !self.config.systems.heat.enabled {
                    return Err("heat subsystem is disabled".into());
                }
                if !power_w.is_finite() {
                    return Err("heat source power must be finite".into());
                }
                self.heat.add_source(pos, radius, power_w, duration_ticks);
                Ok(())
            }
            SimEvent::RemoveHeatSource { index } => self.heat.remove_source(index),
            SimEvent::ApplyForce {
                target,
                force,
                ticks,
            } => {
                let ticks = ticks.max(1);
                // Either persist or one-shot: if ticks>1 push to active queue, else immediate.
                if ticks == 1 {
                    match target {
                        TargetRef::Body(i) => self.rigid.apply_force(i, force),
                    }
                } else {
                    self.active_forces.push(ActiveForce {
                        target,
                        force,
                        remaining: ticks,
                    });
                }
                Ok(())
            }
            SimEvent::SetGravity { g } => {
                self.config.gravity = g;
                Ok(())
            }
            SimEvent::RemoveBody { index } => {
                self.rigid.remove(index)?;
                Ok(())
            }
        }
    }

    fn apply_active_forces(&mut self) {
        if self.active_forces.is_empty() {
            return;
        }
        for af in &mut self.active_forces {
            match af.target {
                TargetRef::Body(i) => self.rigid.apply_force(i, af.force),
            }
            if af.remaining > 0 {
                af.remaining -= 1;
            }
        }
        self.active_forces.retain(|af| af.remaining > 0);
    }

    // ---------- main tick ----------

    /// Advance one macro tick (config.dt).  Returns `true` if a tick happened
    /// (not paused).  The caller measures wall-clock around this to feed QoS.
    pub fn step_tick(&mut self) -> bool {
        if self.paused && self.pending_steps == 0 {
            return false;
        }
        let should_step = if self.pending_steps > 0 {
            self.pending_steps -= 1;
            true
        } else {
            !self.paused
        };
        if !should_step {
            return false;
        }

        // Causal event boundary: drain inbox at the start of the tick.
        self.drain_and_apply();
        self.apply_active_forces();

        let dt = self.config.dt;
        if dt <= 0.0 {
            self.tick += 1;
            self.time += dt;
            return true;
        }

        // Determine substeps: honour rigid substeps, fluid CFL needs, and QoS caps.
        let limits = self.qos.limits();
        let mut substeps = self.config.systems.rigid.substeps as usize;
        substeps = substeps.max(1);
        if self.config.systems.fluid.enabled && self.fluid.n > 0 {
            let sug = self.fluid.suggested_dt().max(1e-6);
            let need = (dt / sug).ceil() as usize;
            substeps = substeps
                .max(need)
                .clamp(1, limits.max_fluid_substeps.min(32));
        }
        if self.nbody.n > 0 {
            // N-body is cheap (direct sum, N≤256) — no extra substeps needed beyond rigid's.
        }
        let dt_sub = dt / substeps as f64;
        let rigid_params = RigidParams {
            restitution: self.config.systems.rigid.restitution,
            friction: self.config.systems.rigid.friction,
        };

        for _ in 0..substeps {
            // N-body (leapfrog) first — positions needed only for probing, no coupling with other scales at v0.1.
            if self.config.systems.nbody.enabled && self.nbody.n > 0 {
                self.nbody.step(dt_sub);
            }
            // Fluid
            if self.config.systems.fluid.enabled && self.fluid.n > 0 {
                self.fluid
                    .step(dt_sub, self.config.gravity, self.config.container);
            }
            // Buoyancy + drag coupling (fluid → rigid) before rigid integrates
            if self.rigid.n > 0 && self.config.systems.fluid.enabled && self.fluid.n > 0 {
                self.apply_buoyancy_and_drag();
            }
            // Rigid
            if self.rigid.n > 0 {
                self.rigid.step(
                    dt_sub,
                    self.config.gravity,
                    self.config.bounds,
                    self.config.container,
                    &rigid_params,
                );
            }
            // Heat — diffusion + optional semi-Lagrangian advection. Fluid
            // sampling reuses its 27-cell grid, so this is local rather than
            // O(n_heat × n_fluid).
            if self.config.systems.heat.enabled {
                if self.config.systems.fluid.enabled && self.fluid.n > 0 {
                    let fluid_ptr: *const Fluid = &self.fluid;
                    // SAFETY: the pointer is read-only while the disjoint heat
                    // field is mutably advanced; both fields belong to `self`.
                    unsafe {
                        self.heat
                            .step(dt_sub, Some(|p: Vec3| (*fluid_ptr).sample_velocity(p)));
                    }
                } else {
                    self.heat.step::<fn(Vec3) -> Vec3>(dt_sub, None);
                }
            }
        }

        // Duration is expressed in public macro ticks, independent of solver
        // substepping / quality level.
        if self.config.systems.heat.enabled {
            self.heat.advance_source_lifetimes();
        }
        self.tick += 1;
        self.time += dt;
        true
    }

    /// Advance exactly `ticks` macro ticks synchronously, independent of the
    /// realtime scheduler. Events are applied at the first tick boundary and
    /// the caller receives a state that is guaranteed to be after all ticks.
    /// This is the primitive used by RL and closed-loop robotics control.
    pub fn step_exact(&mut self, ticks: usize) -> usize {
        if ticks == 0 {
            self.drain_and_apply();
            return 0;
        }
        let was_paused = self.paused;
        let pending = self.pending_steps;
        self.paused = false;
        self.pending_steps = 0;
        let mut stepped = 0;
        for _ in 0..ticks {
            if self.step_tick() {
                stepped += 1;
            }
        }
        self.paused = was_paused;
        self.pending_steps = pending;
        self.scheduler_accum = 0.0;
        stepped
    }

    /// Apply queued events without advancing physical time. Useful for reset
    /// paths that need a populated initial observation.
    pub fn flush_events(&mut self) {
        self.drain_and_apply();
    }

    fn apply_buoyancy_and_drag(&mut self) {
        let rho0 = self.fluid.rho0;
        let g = self.config.gravity;
        let g_len = g.length();
        let cd = 0.47;
        let mu = 0.001; // Pa·s water
        let pi = std::f64::consts::PI;
        for i in 0..self.rigid.n {
            let pos = Vec3::new(self.rigid.px[i], self.rigid.py[i], self.rigid.pz[i]);
            let rho_samp = self.fluid.sample_density(pos);
            if rho_samp <= 1e-6 {
                continue;
            }
            let vol = 4.0 / 3.0 * pi * self.rigid.r[i].powi(3);
            // Buoyancy: -g * rho_samp * vol
            let fb = if g_len > 1e-9 {
                Vec3::new(
                    -g.x * rho_samp * vol,
                    -g.y * rho_samp * vol,
                    -g.z * rho_samp * vol,
                )
            } else {
                Vec3::zero()
            };
            // If body is asleep, check wake condition
            if self.rigid.asleep[i] {
                let weight_mag = self.rigid.m[i] * g_len;
                let fb_mag = fb.length();
                // Wake when |Fb - weight| exceeds 15% of weight or when density signal indicates submersion change.
                let net = (fb_mag - weight_mag).abs();
                if net > weight_mag * 0.12 || rho_samp > rho0 * 0.55 {
                    self.rigid.asleep[i] = false;
                    self.rigid.sleep_counter[i] = 0;
                } else {
                    // Still mostly asleep — apply only buoyancy to keep float, skip drag overhead.
                    self.rigid.fx[i] += fb.x;
                    self.rigid.fy[i] += fb.y;
                    self.rigid.fz[i] += fb.z;
                    continue;
                }
            }
            self.rigid.fx[i] += fb.x;
            self.rigid.fy[i] += fb.y;
            self.rigid.fz[i] += fb.z;

            // Drag
            let v_body = Vec3::new(self.rigid.vx[i], self.rigid.vy[i], self.rigid.vz[i]);
            let v_fluid = self.fluid.sample_velocity(pos);
            let v_rel = v_body.sub(v_fluid);
            let speed = v_rel.length();
            if speed > 1e-9 {
                // Quadratic drag: ½ Cd ρ A v² opposite v_rel
                let area = pi * self.rigid.r[i] * self.rigid.r[i];
                let quad_mag = 0.5 * cd * rho_samp * area * speed * speed;
                // Linear Stokes: 6π μ r v
                let lin_mag = 6.0 * pi * mu * self.rigid.r[i] * speed;
                let tot = quad_mag + lin_mag;
                let dir = v_rel.scale(-1.0 / speed);
                let fd = dir.scale(tot);
                self.rigid.fx[i] += fd.x;
                self.rigid.fy[i] += fd.y;
                self.rigid.fz[i] += fd.z;
            }
        }
    }

    // ---------- probes / queries ----------

    pub fn probe_temperature(&self, p: Vec3) -> f64 {
        self.heat.sample(p)
    }
    pub fn probe_density(&self, p: Vec3) -> f64 {
        self.fluid.sample_density(p)
    }
    pub fn probe_velocity(&self, p: Vec3) -> Vec3 {
        self.fluid.sample_velocity(p)
    }
    pub fn probe_pressure(&self, p: Vec3) -> f64 {
        let rho = self.fluid.sample_density(p);
        let ratio = (rho / self.fluid.rho0).clamp(0.6, 1.8);
        let b = self.fluid.c * self.fluid.c * self.fluid.rho0 / 7.0;
        b * ((ratio).powf(7.0) - 1.0)
    }

    // ---------- hash / snapshot / fork ----------

    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.mix_u64(self.tick);
        h.mix_f64(self.time);
        for &v in self.rng.state().iter() {
            h.mix_u64(v);
        }
        // Configuration and runtime scheduling state are part of the future
        // evolution, so include them in the fingerprint as well.
        h.mix_bytes(self.config.to_json().dump().as_bytes());
        h.mix_u64(self.paused as u64);
        h.mix_f64(self.rate);
        h.mix_u64(self.pending_steps);
        h.mix_u64(self.qos.effective as u64);
        h.mix_u64(self.qos.locked as u64);
        h.mix_f64(self.qos.budget_ms);
        // rigid
        h.mix_u64(self.rigid.n as u64);
        h.mix_f64s(&self.rigid.px[..self.rigid.n]);
        h.mix_f64s(&self.rigid.py[..self.rigid.n]);
        h.mix_f64s(&self.rigid.pz[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vx[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vy[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vz[..self.rigid.n]);
        h.mix_f64s(&self.rigid.r[..self.rigid.n]);
        h.mix_f64s(&self.rigid.m[..self.rigid.n]);
        h.mix_bytes(&self.rigid.mat_id[..self.rigid.n]);
        // fluid
        h.mix_u64(self.fluid.n as u64);
        if self.fluid.n > 0 {
            h.mix_f64s(&self.fluid.px[..self.fluid.n]);
            h.mix_f64s(&self.fluid.py[..self.fluid.n]);
            h.mix_f64s(&self.fluid.pz[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vx[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vy[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vz[..self.fluid.n]);
            h.mix_f64s(&self.fluid.rho[..self.fluid.n]);
            h.mix_f64s(&self.fluid.p[..self.fluid.n]);
        }
        // heat
        h.mix_u64(self.heat.t.len() as u64);
        h.mix_f64s(&self.heat.t);
        // nbody
        h.mix_u64(self.nbody.n as u64);
        if self.nbody.n > 0 {
            h.mix_f64s(&self.nbody.px[..self.nbody.n]);
            h.mix_f64s(&self.nbody.py[..self.nbody.n]);
            h.mix_f64s(&self.nbody.pz[..self.nbody.n]);
            h.mix_f64s(&self.nbody.vx[..self.nbody.n]);
            h.mix_f64s(&self.nbody.vy[..self.nbody.n]);
            h.mix_f64s(&self.nbody.vz[..self.nbody.n]);
            h.mix_f64s(&self.nbody.m[..self.nbody.n]);
        }
        for source in self.heat.sources() {
            h.mix_f64(source.pos.x)
                .mix_f64(source.pos.y)
                .mix_f64(source.pos.z)
                .mix_f64(source.radius)
                .mix_f64(source.power_w)
                .mix_u64(source.ttl);
        }
        for force in &self.active_forces {
            match force.target {
                TargetRef::Body(i) => {
                    h.mix_u64(i as u64);
                }
            }
            h.mix_f64(force.force.x)
                .mix_f64(force.force.y)
                .mix_f64(force.force.z)
                .mix_u64(force.remaining);
        }
        h.finish()
    }

    /// Return a columnar, machine-oriented observation/state payload.
    /// Arrays are Base64 little-endian f64, preserving numerical values for
    /// scientific clients. `include_fluid=false` is recommended for RL when
    /// the policy only needs rigid/proprioceptive state.
    pub fn observation_json(&self, include_fluid: bool, fluid_stride: usize) -> Json {
        self.observation_json_with_fields(include_fluid, fluid_stride, false)
    }

    /// Columnar observation with an optional full temperature field. The field
    /// is opt-in because a dense 128³ grid is large for an RL transition.
    pub fn observation_json_with_fields(
        &self,
        include_fluid: bool,
        fluid_stride: usize,
        include_fields: bool,
    ) -> Json {
        let rigid = Json::obj(vec![
            ("n", Json::num(self.rigid.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.rigid.px, self.rigid.n)),
            ("py", snapshot::f64s_arr_json(&self.rigid.py, self.rigid.n)),
            ("pz", snapshot::f64s_arr_json(&self.rigid.pz, self.rigid.n)),
            ("vx", snapshot::f64s_arr_json(&self.rigid.vx, self.rigid.n)),
            ("vy", snapshot::f64s_arr_json(&self.rigid.vy, self.rigid.n)),
            ("vz", snapshot::f64s_arr_json(&self.rigid.vz, self.rigid.n)),
            ("r", snapshot::f64s_arr_json(&self.rigid.r, self.rigid.n)),
            (
                "mat_id",
                snapshot::u8s_json(&self.rigid.mat_id, self.rigid.n),
            ),
        ]);
        let nbody = Json::obj(vec![
            ("n", Json::num(self.nbody.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.nbody.px, self.nbody.n)),
            ("py", snapshot::f64s_arr_json(&self.nbody.py, self.nbody.n)),
            ("pz", snapshot::f64s_arr_json(&self.nbody.pz, self.nbody.n)),
            ("vx", snapshot::f64s_arr_json(&self.nbody.vx, self.nbody.n)),
            ("vy", snapshot::f64s_arr_json(&self.nbody.vy, self.nbody.n)),
            ("vz", snapshot::f64s_arr_json(&self.nbody.vz, self.nbody.n)),
            ("m", snapshot::f64s_arr_json(&self.nbody.m, self.nbody.n)),
        ]);
        let fluid = if include_fluid {
            let stride = fluid_stride.max(1);
            let sample = |values: &[f64]| -> Json {
                let selected: Vec<f64> = values[..self.fluid.n]
                    .iter()
                    .step_by(stride)
                    .copied()
                    .collect();
                snapshot::f64s_arr_json(&selected, selected.len())
            };
            Json::obj(vec![
                ("n", Json::num(self.fluid.n.div_ceil(stride) as f64)),
                ("stride", Json::num(stride as f64)),
                ("px", sample(&self.fluid.px)),
                ("py", sample(&self.fluid.py)),
                ("pz", sample(&self.fluid.pz)),
                ("vx", sample(&self.fluid.vx)),
                ("vy", sample(&self.fluid.vy)),
                ("vz", sample(&self.fluid.vz)),
                ("rho", sample(&self.fluid.rho)),
            ])
        } else {
            Json::Null
        };
        let fields = if include_fields {
            Json::obj(vec![
                (
                    "temperature",
                    snapshot::f64s_arr_json(&self.heat.t, self.heat.t.len()),
                ),
                ("nx", Json::num(self.heat.nx as f64)),
                ("ny", Json::num(self.heat.ny as f64)),
                ("nz", Json::num(self.heat.nz as f64)),
                (
                    "bounds",
                    Json::obj(vec![
                        ("min", vec3_json_export(self.heat.bounds.min)),
                        ("max", vec3_json_export(self.heat.bounds.max)),
                    ]),
                ),
            ])
        } else {
            Json::Null
        };
        Json::obj(vec![
            ("type", Json::str("observation")),
            ("tick", Json::num(self.tick as f64)),
            ("time", Json::num(self.time)),
            ("hash", Json::str(format!("{:016x}", self.state_hash()))),
            ("bodies", rigid),
            ("fluid", fluid),
            ("nbody", nbody),
            ("fields", fields),
            (
                "temperature_mean",
                Json::num(self.heat.average_temperature()),
            ),
        ])
    }

    pub fn snapshot_json(&self) -> Json {
        // Build sub-objects.
        let rigid = Json::obj(vec![
            ("n", Json::num(self.rigid.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.rigid.px, self.rigid.n)),
            ("py", snapshot::f64s_arr_json(&self.rigid.py, self.rigid.n)),
            ("pz", snapshot::f64s_arr_json(&self.rigid.pz, self.rigid.n)),
            ("vx", snapshot::f64s_arr_json(&self.rigid.vx, self.rigid.n)),
            ("vy", snapshot::f64s_arr_json(&self.rigid.vy, self.rigid.n)),
            ("vz", snapshot::f64s_arr_json(&self.rigid.vz, self.rigid.n)),
            ("r", snapshot::f64s_arr_json(&self.rigid.r, self.rigid.n)),
            ("m", snapshot::f64s_arr_json(&self.rigid.m, self.rigid.n)),
            (
                "mat_id",
                snapshot::u8s_json(&self.rigid.mat_id, self.rigid.n),
            ),
        ]);
        let fluid = Json::obj(vec![
            ("n", Json::num(self.fluid.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.fluid.px, self.fluid.n)),
            ("py", snapshot::f64s_arr_json(&self.fluid.py, self.fluid.n)),
            ("pz", snapshot::f64s_arr_json(&self.fluid.pz, self.fluid.n)),
            ("vx", snapshot::f64s_arr_json(&self.fluid.vx, self.fluid.n)),
            ("vy", snapshot::f64s_arr_json(&self.fluid.vy, self.fluid.n)),
            ("vz", snapshot::f64s_arr_json(&self.fluid.vz, self.fluid.n)),
            (
                "rho",
                snapshot::f64s_arr_json(&self.fluid.rho, self.fluid.n),
            ),
            ("p", snapshot::f64s_arr_json(&self.fluid.p, self.fluid.n)),
        ]);
        let heat = Json::obj(vec![
            ("nx", Json::num(self.heat.nx as f64)),
            ("ny", Json::num(self.heat.ny as f64)),
            ("nz", Json::num(self.heat.nz as f64)),
            (
                "t",
                snapshot::f64s_arr_json(&self.heat.t, self.heat.t.len()),
            ),
        ]);
        let nbody = Json::obj(vec![
            ("n", Json::num(self.nbody.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.nbody.px, self.nbody.n)),
            ("py", snapshot::f64s_arr_json(&self.nbody.py, self.nbody.n)),
            ("pz", snapshot::f64s_arr_json(&self.nbody.pz, self.nbody.n)),
            ("vx", snapshot::f64s_arr_json(&self.nbody.vx, self.nbody.n)),
            ("vy", snapshot::f64s_arr_json(&self.nbody.vy, self.nbody.n)),
            ("vz", snapshot::f64s_arr_json(&self.nbody.vz, self.nbody.n)),
            ("m", snapshot::f64s_arr_json(&self.nbody.m, self.nbody.n)),
        ]);
        let heat_sources_out = Json::arr(
            self.heat
                .sources()
                .iter()
                .map(|source| {
                    Json::obj(vec![
                        ("pos", vec3_json_export(source.pos)),
                        ("radius", Json::num(source.radius)),
                        ("power_w", Json::num(source.power_w)),
                        ("ttl", Json::num(source.ttl as f64)),
                    ])
                })
                .collect(),
        );

        let mut result = snapshot::build_snapshot_json(
            1,
            self.tick,
            self.time,
            self.config.seed,
            self.rng.state(),
            self.config.to_json(),
            rigid,
            fluid,
            heat,
            nbody,
            heat_sources_out,
        );
        if let Json::Obj(ref mut pairs) = result {
            pairs.push(("paused".into(), Json::Bool(self.paused)));
            pairs.push(("rate".into(), Json::num(self.rate)));
            pairs.push(("pending_steps".into(), Json::num(self.pending_steps as f64)));
            pairs.push(("scheduler_accum".into(), Json::num(self.scheduler_accum)));
            pairs.push((
                "qos_effective".into(),
                Json::str(self.qos.effective.as_str()),
            ));
            pairs.push(("qos_locked".into(), Json::Bool(self.qos.locked)));
            pairs.push(("qos_budget_ms".into(), Json::num(self.qos.budget_ms)));
            pairs.push((
                "active_forces".into(),
                Json::arr(
                    self.active_forces
                        .iter()
                        .map(|force| {
                            Json::obj(vec![
                                (
                                    "body",
                                    match force.target {
                                        TargetRef::Body(i) => Json::num(i as f64),
                                    },
                                ),
                                ("force", vec3_json_export(force.force)),
                                ("remaining", Json::num(force.remaining as f64)),
                            ])
                        })
                        .collect(),
                ),
            ));
            pairs.push(("replay_log".into(), self.replay_log_json()));
        }
        result
    }

    /// Restore state from a snapshot produced by [`Self::snapshot_json`].
    /// Config is replaced wholesale, then solver arenas are filled.
    pub fn restore_from_json(&mut self, snap: &Json) -> Result<(), String> {
        let ver = snap.get("version").and_then(|j| j.as_u64()).unwrap_or(0);
        if ver != 1 {
            return Err(format!("unsupported snapshot version {ver}"));
        }
        self.tick = snap
            .get("tick")
            .and_then(|j| j.as_u64())
            .ok_or("snapshot tick missing")?;
        self.time = snap
            .get("time")
            .and_then(|j| j.as_f64())
            .ok_or("snapshot time missing")?;
        let seed = if let Some(seed_hex) = snap.get("seed_hex").and_then(|j| j.as_str()) {
            u64::from_str_radix(seed_hex.trim_start_matches("0x"), 16)
                .map_err(|e| format!("bad snapshot seed_hex: {e}"))?
        } else {
            snap.get("seed")
                .and_then(|j| j.as_u64())
                .ok_or("snapshot seed missing")?
        };
        let rng_st = snapshot::parse_rng_hex(snap)?;
        self.rng.set_state(rng_st);
        // config
        if let Some(cfg_json) = snap.get("config") {
            self.config = WorldConfig::from_json(cfg_json)?;
            self.config.seed = seed;
        }
        self.paused = snap
            .get("paused")
            .and_then(|j| j.as_bool())
            .unwrap_or(self.paused);
        self.rate = snap
            .get("rate")
            .and_then(|j| j.as_f64())
            .unwrap_or(self.rate)
            .clamp(0.0, 16.0);
        self.pending_steps = snap
            .get("pending_steps")
            .and_then(|j| j.as_u64())
            .unwrap_or(0);
        self.scheduler_accum = snap
            .get("scheduler_accum")
            .and_then(|j| j.as_f64())
            .unwrap_or(0.0)
            .max(0.0);
        if let Some(q) = snap
            .get("qos_effective")
            .and_then(|j| j.as_str())
            .and_then(Quality::parse)
        {
            let locked = snap
                .get("qos_locked")
                .and_then(|j| j.as_bool())
                .unwrap_or(false);
            let budget = snap
                .get("qos_budget_ms")
                .and_then(|j| j.as_f64())
                .unwrap_or(self.qos.budget_ms);
            self.qos.restore_runtime(q, locked, budget);
        }
        // Reconfigure sub-systems where resolution changed — allocate new backing where needed.
        self.reconfigure_from_config();

        // rigid
        if let Some(r) = snap.get("rigid") {
            let n_raw = r.get("n").and_then(|j| j.as_u64()).ok_or("rigid.n")?;
            let n = usize::try_from(n_raw).map_err(|_| "rigid.n does not fit usize")?;
            if n > self.rigid.cap {
                return Err("rigid snapshot n exceeds cap".into());
            }
            self.rigid.n = n;
            let decode = |key: &str, dst: &mut [f64]| {
                let j = r.get(key).ok_or(format!("rigid.{key}"))?;
                let vec = snapshot::decode_vec(j, n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(), String>(())
            };
            decode("px", &mut self.rigid.px)?;
            decode("py", &mut self.rigid.py)?;
            decode("pz", &mut self.rigid.pz)?;
            decode("vx", &mut self.rigid.vx)?;
            decode("vy", &mut self.rigid.vy)?;
            decode("vz", &mut self.rigid.vz)?;
            decode("r", &mut self.rigid.r)?;
            decode("m", &mut self.rigid.m)?;
            for i in 0..n {
                self.rigid.inv_m[i] = 1.0 / self.rigid.m[i].max(1e-12);
                self.rigid.restitution[i] = self.config.systems.rigid.restitution;
            }
            if let Some(j) = r.get("mat_id") {
                let bytes = snapshot::decode_u8s_from(j, n)?;
                self.rigid.mat_id[..n].copy_from_slice(&bytes);
            }
            self.rigid.asleep.fill(false);
            self.rigid.sleep_counter.fill(0);
        }
        // fluid
        if let Some(f) = snap.get("fluid") {
            let n_raw = f.get("n").and_then(|j| j.as_u64()).ok_or("fluid.n")?;
            let n = usize::try_from(n_raw).map_err(|_| "fluid.n does not fit usize")?;
            if n > self.fluid.cap {
                return Err("fluid snapshot n exceeds cap".into());
            }
            self.fluid.n = n;
            let decode = |key: &str, dst: &mut [f64]| {
                let j = f.get(key).ok_or(format!("fluid.{key}"))?;
                let vec = snapshot::decode_vec(j, n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(), String>(())
            };
            decode("px", &mut self.fluid.px)?;
            decode("py", &mut self.fluid.py)?;
            decode("pz", &mut self.fluid.pz)?;
            decode("vx", &mut self.fluid.vx)?;
            decode("vy", &mut self.fluid.vy)?;
            decode("vz", &mut self.fluid.vz)?;
            if let Some(j) = f.get("rho") {
                let vec = snapshot::decode_vec(j, n)?;
                self.fluid.rho[..n].copy_from_slice(&vec);
            }
            if let Some(j) = f.get("p") {
                let vec = snapshot::decode_vec(j, n)?;
                self.fluid.p[..n].copy_from_slice(&vec);
            }
        }
        // heat
        if let Some(h) = snap.get("heat") {
            let nx_raw = h.get("nx").and_then(|j| j.as_u64()).ok_or("heat.nx")?;
            let ny_raw = h.get("ny").and_then(|j| j.as_u64()).ok_or("heat.ny")?;
            let nz_raw = h.get("nz").and_then(|j| j.as_u64()).ok_or("heat.nz")?;
            if !(4..=256).contains(&nx_raw)
                || !(4..=256).contains(&ny_raw)
                || !(4..=256).contains(&nz_raw)
            {
                return Err("heat snapshot dimensions must be in 4..=256".into());
            }
            let nx = nx_raw as u32;
            let ny = ny_raw as u32;
            let nz = nz_raw as u32;
            if nx != self.heat.nx || ny != self.heat.ny || nz != self.heat.nz {
                self.heat = HeatField::new(
                    self.config.bounds,
                    [nx, ny, nz],
                    self.heat.alpha,
                    self.heat.vol_cap,
                );
            }
            if let Some(j) = h.get("t") {
                let expect = (nx as usize)
                    .checked_mul(ny as usize)
                    .and_then(|v| v.checked_mul(nz as usize))
                    .ok_or("heat grid dimensions overflow")?;
                let vec = snapshot::decode_vec(j, expect)?;
                self.heat.restore_temperature_grid(vec)?;
            }
            if let Some(sources) = snap.get("heat_sources").and_then(|j| j.as_arr()) {
                let mut restored = Vec::with_capacity(sources.len());
                for source in sources {
                    let pos = source
                        .get("pos")
                        .and_then(|j| j.as_vec3())
                        .ok_or("heat source pos")?;
                    let radius = source
                        .get("radius")
                        .and_then(|j| j.as_f64())
                        .ok_or("heat source radius")?;
                    let power_w = source
                        .get("power_w")
                        .and_then(|j| j.as_f64())
                        .ok_or("heat source power_w")?;
                    let ttl = source.get("ttl").and_then(|j| j.as_u64()).unwrap_or(0);
                    restored.push(ws_solvers::heat::HeatSource {
                        pos,
                        radius,
                        power_w,
                        ttl,
                    });
                }
                self.heat.restore_sources(restored);
            }
        }
        // nbody
        if let Some(nb) = snap.get("nbody") {
            let n_raw = nb.get("n").and_then(|j| j.as_u64()).ok_or("nbody.n")?;
            let n = usize::try_from(n_raw).map_err(|_| "nbody.n does not fit usize")?;
            if n > self.nbody.cap {
                return Err("nbody snapshot n exceeds cap".into());
            }
            self.nbody.n = n;
            let dec = |key: &str, dst: &mut [f64]| {
                let j = nb.get(key).ok_or(format!("nbody.{key}"))?;
                let vec = snapshot::decode_vec(j, n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(), String>(())
            };
            dec("px", &mut self.nbody.px)?;
            dec("py", &mut self.nbody.py)?;
            dec("pz", &mut self.nbody.pz)?;
            dec("vx", &mut self.nbody.vx)?;
            dec("vy", &mut self.nbody.vy)?;
            dec("vz", &mut self.nbody.vz)?;
            dec("m", &mut self.nbody.m)?;
            self.nbody.recompute_accelerations();
        }
        self.active_forces.clear();
        if let Some(forces) = snap.get("active_forces").and_then(|j| j.as_arr()) {
            for force in forces {
                let body = force
                    .get("body")
                    .and_then(|j| j.as_u64())
                    .filter(|i| *i <= u32::MAX as u64)
                    .ok_or("active force body")? as u32;
                let f = force
                    .get("force")
                    .and_then(|j| j.as_vec3())
                    .ok_or("active force vector")?;
                let remaining = force.get("remaining").and_then(|j| j.as_u64()).unwrap_or(0);
                if remaining > 0 {
                    self.active_forces.push(ActiveForce {
                        target: TargetRef::Body(body),
                        force: f,
                        remaining,
                    });
                }
            }
        }
        self.replay_log.clear();
        if let Some(log) = snap.get("replay_log").and_then(|j| j.as_arr()) {
            for item in log {
                let tick = item
                    .get("tick")
                    .and_then(|j| j.as_u64())
                    .ok_or("replay tick")?;
                let event = item.get("event").ok_or("replay event")?.clone();
                self.replay_log.push((tick, event));
            }
        }
        self.inbox.clear();
        self.event_errors.clear();
        Ok(())
    }

    fn reconfigure_from_config(&mut self) {
        // fluid
        self.fluid.configure(
            self.config.systems.fluid.h,
            self.config.systems.fluid.rho0,
            self.config.systems.fluid.c,
            self.config.systems.fluid.visc_alpha,
        );
        self.fluid.bounds = self.config.bounds;
        // heat
        if self.heat.nx != self.config.systems.heat.res[0]
            || self.heat.ny != self.config.systems.heat.res[1]
            || self.heat.nz != self.config.systems.heat.res[2]
        {
            self.heat = HeatField::new(
                self.config.bounds,
                self.config.systems.heat.res,
                self.config.systems.heat.alpha,
                self.config.systems.heat.volumetric_capacity,
            );
        } else {
            self.heat.bounds = self.config.bounds;
            self.heat.set_alpha(self.config.systems.heat.alpha);
            self.heat
                .set_vol_cap(self.config.systems.heat.volumetric_capacity);
        }
        // nbody
        self.nbody
            .configure(self.config.systems.nbody.softening, ws_core::constants::G);
    }

    /// Fork this world into a new one with a fresh id.  Snapshots the full
    /// state so the child is bit-exact at fork time.
    pub fn fork(&self, new_id: String) -> World {
        let snap = self.snapshot_json();
        let mut child = World::new(new_id, self.config.clone(), self.name.clone());
        // Copy timing identical to snapshot shape.
        let _ = child.restore_from_json(&snap);
        child.tick = self.tick;
        child.time = self.time;
        // Keep all runtime state identical at the fork moment. Future draws and
        // states diverge when the child receives different events.
        child.rng = self.rng.clone();
        child.qos = self.qos.clone();
        child
    }

    pub fn replay_log_json(&self) -> Json {
        Json::arr(
            self.replay_log
                .iter()
                .map(|(tick, ev)| {
                    Json::obj(vec![
                        ("tick", Json::num(*tick as f64)),
                        ("event", ev.clone()),
                    ])
                })
                .collect(),
        )
    }

    // --- control ---

    pub fn set_paused(&mut self, p: bool) {
        self.paused = p;
        if !p {
            self.pending_steps = 0;
        }
    }
    pub fn set_rate(&mut self, r: f64) {
        self.rate = r.clamp(0.0, 16.0);
    }
    pub fn step_once(&mut self) {
        if self.paused {
            self.pending_steps += 1;
        }
    }
    pub fn set_quality(&mut self, q: Quality) {
        self.config.quality = q;
        self.qos.lock(q);
    }
    pub fn set_quality_auto(&mut self) {
        self.qos.unlock();
    }
    pub fn set_budget_ms(&mut self, ms: f64) {
        self.qos.set_budget(ms);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// Provide the Vec3 -> JSON helper used by domain events in this crate.
pub fn vec3_json_export(p: Vec3) -> Json {
    Json::arr(vec![Json::num(p.x), Json::num(p.y), Json::num(p.z)])
}

// Make the domain helper public for events.rs import resolution via `crate::` alias.
// ws_domain currently imports via `crate::config::vec3_json_pub` — expose it.
pub mod reexport_for_domain {
    pub use super::vec3_json_export as vec3_json_pub;
}
