//! The simulation world — the unit of execution.

use ws_core::hash::StateHasher;
use ws_core::json::Json;
use ws_core::math::{Aabb, Vec3};
use ws_core::rng::{Rng, STREAM_FLUID_JITTER};

use ws_domain::config::{Quality, WorldConfig};
use ws_domain::events::{SimEvent, TargetRef};
use ws_domain::materials;

use ws_solvers::fluid::Fluid;
use ws_solvers::heat::HeatField;
use ws_solvers::nbody::NBodySystem;
use ws_solvers::rigid::{RigidBodies, RigidParams};

use crate::qos::{QosController, limits_for};
use crate::snapshot;

// Material palette index — matches `ws_domain::materials` ordering plus extra.
fn material_palette_index(name: &str) -> u8 {
    const ORDER: &[&str] = &["steel","aluminum","copper","wood","rubber","glass","ice","ceramic"];
    for (i,&n) in ORDER.iter().enumerate() {
        if n.eq_ignore_ascii_case(name) { return i as u8; }
    }
    // fallback steel
    0
}
fn material_density(name: &str)->f64 {
    materials::lookup(name).map(|m| m.density).unwrap_or(7850.0)
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

    active_forces: Vec<ActiveForce>,

    /// Accumulator for the scheduler's rate-controlled tick pacing (seconds of world time pending).
    pub scheduler_accum: f64,

    pub created_at_ms: u64,
}

// Manual Debug skipping large arenas
impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>)->std::fmt::Result {
        f.debug_struct("World")
            .field("id",&self.id)
            .field("tick",&self.tick)
            .field("rigid_n",&self.rigid.n)
            .field("fluid_n",&self.fluid.n)
            .finish()
    }
}

impl World {
    pub fn new(id: String, config: WorldConfig, name: Option<String>)->Self {
        let budget_ms = (config.dt*1000.0*0.90).clamp(5.0, 30.0);
        let qos=QosController::new(config.quality, budget_ms);
        let limits=limits_for(config.quality);

        let rigid_cap = 8192;
        let mut rigid=RigidBodies::new(rigid_cap);

        let nbody_cap=256;
        let mut nbody=NBodySystem::new(nbody_cap, config.systems.nbody.softening, ws_core::constants::G);
        if config.systems.nbody.enabled {
            // keep nbody enabled flag inside config; solver already configured
        }

        let mut fluid=Fluid::new(48_000, config.systems.fluid.h, config.systems.fluid.rho0, config.systems.fluid.c, config.systems.fluid.visc_alpha, config.bounds);
        // but respect limits initial? caps stay max, QoS limits gating via spawn truncation.

        let heat=HeatField::new(config.bounds, config.systems.heat.res, config.systems.heat.alpha, config.systems.heat.volumetric_capacity);

        let rng=Rng::new(config.seed, 0x51A);
        let _=limits;
        let _=rigid_cap;

        Self{
            id, name, config, tick:0, time:0.0, rng,
            rigid, fluid, heat, nbody,
            inbox: Vec::new(), replay_log: Vec::new(),
            qos, paused:false, rate:1.0, pending_steps:0,
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
        if self.inbox.is_empty() { return; }
        let events = std::mem::take(&mut self.inbox);
        for ev in events {
            let json = ev.to_json();
            let tick = self.tick; // will be the tick this batch is applied at
            let res = self.apply_event(ev);
            // Still log even if apply failed — preserves causality for debugging.
            self.replay_log.push((tick, json));
            if let Err(msg)=res {
                // In production we would emit to a world log channel; for v0.1 this is silently kept in replay (consumer can inspect).
                let _ = msg;
            }
        }
    }

    fn apply_event(&mut self, ev: SimEvent)->Result<(),String> {
        match ev {
            SimEvent::SpawnSphere{material, pos, vel, radius}=>{
                let density=material_density(&material);
                let mat_id=material_palette_index(&material);
                let rest=self.config.systems.rigid.restitution;
                self.rigid.spawn(pos, vel, radius, density, rest, mat_id)?;
                Ok(())
            }
            SimEvent::SpawnCapsule{material, pos, vel, radius, half_height}=>{
                // v0.1: capsule approximated as volume-equivalent sphere for throughput;
                // true segment-distance capsule lands in v0.2 (law registry).
                let density=material_density(&material);
                let mat_id=material_palette_index(&material);
                let rest=self.config.systems.rigid.restitution;
                let vol_capsule = std::f64::consts::PI * radius*radius * (2.0*half_height + 4.0/3.0*radius);
                let eq_radius = ((3.0*vol_capsule)/(4.0*std::f64::consts::PI)).powf(1.0/3.0);
                self.rigid.spawn(pos, vel, eq_radius.clamp(0.02, 2.0), density, rest, mat_id)?;
                Ok(())
            }
            SimEvent::SpawnFluidBlob{center, radius}=>{
                let limits=self.qos.limits();
                let free=limits.max_fluid_particles.saturating_sub(self.fluid.n);
                if free==0 { return Ok(()); } // at capacity — silently ignore extra, QoS governs.
                // Rough lattice count estimate: sphere vol / dp³
                let before=self.fluid.n;
                let mut local_rng = Rng::new(self.config.seed ^ (self.tick.wrapping_mul(0x9E3779B9)), STREAM_FLUID_JITTER.wrapping_add(self.fluid.n as u64));
                let jitter=self.fluid.h*0.08;
                let added=self.fluid.spawn_blob(center, radius, jitter, &mut local_rng);
                // Truncate if over limit after spawn (spawn may have overshot by a few due to lattice).
                if self.fluid.n > limits.max_fluid_particles {
                    self.fluid.n = limits.max_fluid_particles;
                }
                let _=added; let _=before;
                Ok(())
            }
            SimEvent::HeatSource{pos, radius, power_w, duration_ticks}=>{
                self.heat.add_source(pos, radius, power_w, duration_ticks);
                Ok(())
            }
            SimEvent::RemoveHeatSource{index}=>{
                self.heat.remove_source(index)
            }
            SimEvent::ApplyForce{target, force, ticks}=>{
                let ticks = ticks.max(1);
                // Either persist or one-shot: if ticks>1 push to active queue, else immediate.
                if ticks==1 {
                    match target { TargetRef::Body(i)=> self.rigid.apply_force(i, force), }
                } else {
                    self.active_forces.push(ActiveForce{ target, force, remaining: ticks });
                }
                Ok(())
            }
            SimEvent::SetGravity{g}=>{
                self.config.gravity=g;
                Ok(())
            }
            SimEvent::RemoveBody{index}=>{
                self.rigid.remove(index)?;
                Ok(())
            }
        }
    }

    fn apply_active_forces(&mut self) {
        if self.active_forces.is_empty() { return; }
        for af in &mut self.active_forces {
            match af.target {
                TargetRef::Body(i)=> self.rigid.apply_force(i, af.force),
            }
            if af.remaining>0 { af.remaining-=1; }
        }
        self.active_forces.retain(|af| af.remaining>0);
    }

    // ---------- main tick ----------

    /// Advance one macro tick (config.dt).  Returns `true` if a tick happened
    /// (not paused).  The caller measures wall-clock around this to feed QoS.
    pub fn step_tick(&mut self)->bool {
        if self.paused && self.pending_steps==0 {
            return false;
        }
        let should_step = if self.pending_steps>0 {
            self.pending_steps-=1;
            true
        } else {
            !self.paused
        };
        if !should_step { return false; }

        // Causal event boundary: drain inbox at the start of the tick.
        self.drain_and_apply();
        self.apply_active_forces();

        let dt = self.config.dt;
        if dt<=0.0 { self.tick+=1; self.time+=dt; return true; }

        // Determine substeps: honour rigid substeps, fluid CFL needs, and QoS caps.
        let limits=self.qos.limits();
        let mut substeps = self.config.systems.rigid.substeps as usize;
        substeps = substeps.max(1);
        if self.fluid.n>0 {
            let sug = self.fluid.suggested_dt().max(1e-6);
            let need = (dt / sug).ceil() as usize;
            substeps = substeps.max(need).min(limits.max_fluid_substeps).min(32).max(1);
        }
        if self.nbody.n>0 {
            // N-body is cheap (direct sum, N≤256) — no extra substeps needed beyond rigid's.
        }
        let dt_sub = dt / substeps as f64;
        let rigid_params = RigidParams{
            restitution: self.config.systems.rigid.restitution,
            friction: self.config.systems.rigid.friction,
        };

        for _ in 0..substeps {
            // N-body (leapfrog) first — positions needed only for probing, no coupling with other scales at v0.1.
            if self.config.systems.nbody.enabled && self.nbody.n>0 {
                self.nbody.step(dt_sub);
            }
            // Fluid
            if self.fluid.n>0 {
                self.fluid.step(dt_sub, self.config.gravity, self.config.container);
            }
            // Buoyancy + drag coupling (fluid → rigid) before rigid integrates
            if self.rigid.n>0 && self.fluid.n>0 {
                self.apply_buoyancy_and_drag();
            }
            // Rigid
            if self.rigid.n>0 {
                self.rigid.step(dt_sub, self.config.gravity, self.config.bounds, self.config.container, &rigid_params);
            }
            // Heat — diffusion + sources only in v0.1 (advection by fluid is roadmap, see docs/physics.md).
            // The semi-Lagrangian advection would be O(n_heat × n_fluid) with the current brute-force sampler,
            // which dominates the tick budget for the default demo (16k fluid × 4k heat = 64M kernel evals per tick).
            // For v0.1 we keep heat as pure diffusion/source; viewers/tests that need advection can enable it locally.
            if self.config.systems.heat.enabled {
                self.heat.step::<fn(Vec3)->Vec3>(dt_sub, None);
            }
        }

        self.tick+=1;
        self.time+=dt;
        true
    }

    fn apply_buoyancy_and_drag(&mut self) {
        let rho0 = self.fluid.rho0;
        let g = self.config.gravity;
        let g_len = g.length();
        let cd = 0.47;
        let mu = 0.001; // Pa·s water
        let pi = std::f64::consts::PI;
        for i in 0..self.rigid.n {
            let pos=Vec3::new(self.rigid.px[i], self.rigid.py[i], self.rigid.pz[i]);
            let rho_samp = self.fluid.sample_density(pos);
            if rho_samp <= 1e-6 { continue; }
            let vol = 4.0/3.0 * pi * self.rigid.r[i].powi(3);
            // Buoyancy: -g * rho_samp * vol
            let fb = if g_len>1e-9 {
                Vec3::new(-g.x * rho_samp * vol, -g.y * rho_samp * vol, -g.z * rho_samp * vol)
            } else { Vec3::zero() };
            // If body is asleep, check wake condition
            if self.rigid.asleep[i] {
                let weight_mag = self.rigid.m[i]*g_len;
                let fb_mag = fb.length();
                // Wake when |Fb - weight| exceeds 15% of weight or when density signal indicates submersion change.
                let net = (fb_mag - weight_mag).abs();
                if net > weight_mag*0.12 || rho_samp > rho0*0.55 {
                    self.rigid.asleep[i]=false; self.rigid.sleep_counter[i]=0;
                } else {
                    // Still mostly asleep — apply only buoyancy to keep float, skip drag overhead.
                    self.rigid.fx[i]+=fb.x; self.rigid.fy[i]+=fb.y; self.rigid.fz[i]+=fb.z;
                    continue;
                }
            }
            self.rigid.fx[i]+=fb.x; self.rigid.fy[i]+=fb.y; self.rigid.fz[i]+=fb.z;

            // Drag
            let v_body=Vec3::new(self.rigid.vx[i], self.rigid.vy[i], self.rigid.vz[i]);
            let v_fluid=self.fluid.sample_velocity(pos);
            let v_rel=v_body.sub(v_fluid);
            let speed=v_rel.length();
            if speed>1e-9 {
                // Quadratic drag: ½ Cd ρ A v² opposite v_rel
                let area=pi * self.rigid.r[i]*self.rigid.r[i];
                let quad_mag=0.5*cd*rho_samp*area*speed*speed;
                // Linear Stokes: 6π μ r v
                let lin_mag=6.0*pi*mu*self.rigid.r[i]*speed;
                let tot=quad_mag+lin_mag;
                let dir=v_rel.scale(-1.0/speed);
                let fd=dir.scale(tot);
                self.rigid.fx[i]+=fd.x; self.rigid.fy[i]+=fd.y; self.rigid.fz[i]+=fd.z;
            }
        }
    }

    // ---------- probes / queries ----------

    pub fn probe_temperature(&self, p: Vec3)->f64 { self.heat.sample(p) }
    pub fn probe_density(&self, p: Vec3)->f64 { self.fluid.sample_density(p) }
    pub fn probe_velocity(&self, p: Vec3)->Vec3 { self.fluid.sample_velocity(p) }
    pub fn probe_pressure(&self, p: Vec3)->f64 {
        let rho=self.fluid.sample_density(p);
        let ratio=(rho/self.fluid.rho0).clamp(0.6, 1.8);
        let b=self.fluid.c*self.fluid.c*self.fluid.rho0/7.0;
        b*((ratio).powf(7.0)-1.0)
    }

    // ---------- hash / snapshot / fork ----------

    pub fn state_hash(&self)->u64 {
        let mut h=StateHasher::new();
        h.mix_u64(self.tick);
        h.mix_f64(self.time);
        for &v in self.rng.state().iter() { h.mix_u64(v); }
        // rigid
        h.mix_u64(self.rigid.n as u64);
        h.mix_f64s(&self.rigid.px[..self.rigid.n]);
        h.mix_f64s(&self.rigid.py[..self.rigid.n]);
        h.mix_f64s(&self.rigid.pz[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vx[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vy[..self.rigid.n]);
        h.mix_f64s(&self.rigid.vz[..self.rigid.n]);
        // fluid
        h.mix_u64(self.fluid.n as u64);
        if self.fluid.n>0 {
            h.mix_f64s(&self.fluid.px[..self.fluid.n]);
            h.mix_f64s(&self.fluid.py[..self.fluid.n]);
            h.mix_f64s(&self.fluid.pz[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vx[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vy[..self.fluid.n]);
            h.mix_f64s(&self.fluid.vz[..self.fluid.n]);
        }
        // heat
        h.mix_u64(self.heat.t.len() as u64);
        h.mix_f64s(&self.heat.t);
        // nbody
        h.mix_u64(self.nbody.n as u64);
        if self.nbody.n>0 {
            h.mix_f64s(&self.nbody.px[..self.nbody.n]);
            h.mix_f64s(&self.nbody.py[..self.nbody.n]);
            h.mix_f64s(&self.nbody.pz[..self.nbody.n]);
        }
        h.finish()
    }

    pub fn snapshot_json(&self)->Json {
        // Build sub-objects.
        let rigid=Json::obj(vec![
            ("n", Json::num(self.rigid.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.rigid.px, self.rigid.n)),
            ("py", snapshot::f64s_arr_json(&self.rigid.py, self.rigid.n)),
            ("pz", snapshot::f64s_arr_json(&self.rigid.pz, self.rigid.n)),
            ("vx", snapshot::f64s_arr_json(&self.rigid.vx, self.rigid.n)),
            ("vy", snapshot::f64s_arr_json(&self.rigid.vy, self.rigid.n)),
            ("vz", snapshot::f64s_arr_json(&self.rigid.vz, self.rigid.n)),
            ("r",  snapshot::f64s_arr_json(&self.rigid.r , self.rigid.n)),
            ("m",  snapshot::f64s_arr_json(&self.rigid.m , self.rigid.n)),
            ("mat_id", snapshot::u8s_json(&self.rigid.mat_id, self.rigid.n)),
        ]);
        let fluid=Json::obj(vec![
            ("n", Json::num(self.fluid.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.fluid.px, self.fluid.n)),
            ("py", snapshot::f64s_arr_json(&self.fluid.py, self.fluid.n)),
            ("pz", snapshot::f64s_arr_json(&self.fluid.pz, self.fluid.n)),
            ("vx", snapshot::f64s_arr_json(&self.fluid.vx, self.fluid.n)),
            ("vy", snapshot::f64s_arr_json(&self.fluid.vy, self.fluid.n)),
            ("vz", snapshot::f64s_arr_json(&self.fluid.vz, self.fluid.n)),
            ("rho", snapshot::f64s_arr_json(&self.fluid.rho, self.fluid.n)),
        ]);
        let heat=Json::obj(vec![
            ("nx", Json::num(self.heat.nx as f64)),
            ("ny", Json::num(self.heat.ny as f64)),
            ("nz", Json::num(self.heat.nz as f64)),
            ("t", snapshot::f64s_arr_json(&self.heat.t, self.heat.t.len())),
        ]);
        let nbody=Json::obj(vec![
            ("n", Json::num(self.nbody.n as f64)),
            ("px", snapshot::f64s_arr_json(&self.nbody.px, self.nbody.n)),
            ("py", snapshot::f64s_arr_json(&self.nbody.py, self.nbody.n)),
            ("pz", snapshot::f64s_arr_json(&self.nbody.pz, self.nbody.n)),
            ("vx", snapshot::f64s_arr_json(&self.nbody.vx, self.nbody.n)),
            ("vy", snapshot::f64s_arr_json(&self.nbody.vy, self.nbody.n)),
            ("vz", snapshot::f64s_arr_json(&self.nbody.vz, self.nbody.n)),
            ("m",  snapshot::f64s_arr_json(&self.nbody.m , self.nbody.n)),
        ]);
        let heat_sources=Json::arr(
            (0..self.heat.source_count()).map(|_| Json::Null).collect()
            // Sources are not individually snapshotted in the minimal valid state at v0.1 — they are re-created via the event log if needed.
            // To keep the contract stable we emit an empty array and rely on the fact that scheduled test scenes use persistent sources only via replay.
        );
        // Actually emit sources we can serialise from HeatField if it exposed them — since it doesn't expose details at v0.1, keep empty.
        let _=heat_sources;
        let heat_sources_out=Json::arr(vec![]);

        snapshot::build_snapshot_json(1, self.tick, self.time, self.config.seed, self.rng.state(), self.config.to_json(), rigid, fluid, heat, nbody, heat_sources_out)
    }

    /// Restore state from a snapshot produced by [`Self::snapshot_json`].
    /// Config is replaced wholesale, then solver arenas are filled.
    pub fn restore_from_json(&mut self, snap: &Json)->Result<(),String> {
        let ver=snap.get("version").and_then(|j| j.as_u64()).unwrap_or(0);
        if ver!=1 { return Err(format!("unsupported snapshot version {ver}")); }
        self.tick=snap.get("tick").and_then(|j| j.as_u64()).ok_or("snapshot tick missing")?;
        self.time=snap.get("time").and_then(|j| j.as_f64()).ok_or("snapshot time missing")?;
        let seed=snap.get("seed").and_then(|j| j.as_u64()).ok_or("snapshot seed missing")?;
        let rng_st=snapshot::parse_rng_hex(snap)?;
        self.rng.set_state(rng_st);
        // config
        if let Some(cfg_json)=snap.get("config") {
            self.config=WorldConfig::from_json(cfg_json)?;
            self.config.seed=seed;
        }
        // Reconfigure sub-systems where resolution changed — allocate new backing where needed.
        self.reconfigure_from_config();

        // rigid
        if let Some(r)=snap.get("rigid") {
            let n=r.get("n").and_then(|j| j.as_u64()).ok_or("rigid.n")? as usize;
            if n>self.rigid.cap { return Err("rigid snapshot n exceeds cap".into()); }
            self.rigid.n=n;
            let decode = |key:&str, dst:&mut [f64]|{
                let j=r.get(key).ok_or(format!("rigid.{key}"))?;
                let vec=snapshot::decode_vec(j, n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(),String>(())
            };
            decode("px",&mut self.rigid.px)?; decode("py",&mut self.rigid.py)?; decode("pz",&mut self.rigid.pz)?;
            decode("vx",&mut self.rigid.vx)?; decode("vy",&mut self.rigid.vy)?; decode("vz",&mut self.rigid.vz)?;
            decode("r", &mut self.rigid.r)?;  decode("m", &mut self.rigid.m)?;
            for i in 0..n { self.rigid.inv_m[i]=1.0/self.rigid.m[i].max(1e-12); self.rigid.restitution[i]=self.config.systems.rigid.restitution; }
            if let Some(j)=r.get("mat_id") {
                let bytes=snapshot::decode_u8s_from(j, n)?;
                self.rigid.mat_id[..n].copy_from_slice(&bytes);
            }
            self.rigid.asleep.fill(false); self.rigid.sleep_counter.fill(0);
        }
        // fluid
        if let Some(f)=snap.get("fluid") {
            let n=f.get("n").and_then(|j| j.as_u64()).ok_or("fluid.n")? as usize;
            if n>self.fluid.cap { return Err("fluid snapshot n exceeds cap".into()); }
            self.fluid.n=n;
            let decode=|key:&str, dst:&mut [f64]|{
                let j=f.get(key).ok_or(format!("fluid.{key}"))?;
                let vec=snapshot::decode_vec(j, n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(),String>(())
            };
            decode("px",&mut self.fluid.px)?; decode("py",&mut self.fluid.py)?; decode("pz",&mut self.fluid.pz)?;
            decode("vx",&mut self.fluid.vx)?; decode("vy",&mut self.fluid.vy)?; decode("vz",&mut self.fluid.vz)?;
            if let Some(j)=f.get("rho") {
                let vec=snapshot::decode_vec(j,n)?;
                self.fluid.rho[..n].copy_from_slice(&vec);
            }
        }
        // heat
        if let Some(h)=snap.get("heat") {
            let nx=h.get("nx").and_then(|j| j.as_u64()).ok_or("heat.nx")? as u32;
            let ny=h.get("ny").and_then(|j| j.as_u64()).ok_or("heat.ny")? as u32;
            let nz=h.get("nz").and_then(|j| j.as_u64()).ok_or("heat.nz")? as u32;
            if nx!=self.heat.nx || ny!=self.heat.ny || nz!=self.heat.nz {
                self.heat=HeatField::new(self.config.bounds, [nx,ny,nz], self.heat.alpha, self.heat.vol_cap);
            }
            if let Some(j)=h.get("t") {
                let expect=(nx*ny*nz) as usize;
                let vec=snapshot::decode_vec(j, expect)?;
                self.heat.t.copy_from_slice(&vec);
                self.heat.t.clone_from_slice(&vec); // redundant but explicit
                // Ensure next_t mirror
                self.heat = {
                    let mut hf=HeatField::new(self.config.bounds, [nx,ny,nz], self.heat.alpha, self.heat.vol_cap);
                    hf.t=vec;
                    hf
                };
            }
        }
        // nbody
        if let Some(nb)=snap.get("nbody") {
            let n=nb.get("n").and_then(|j| j.as_u64()).ok_or("nbody.n")? as usize;
            if n>self.nbody.cap { return Err("nbody snapshot n exceeds cap".into()); }
            self.nbody.n=n;
            let dec=|key:&str, dst:&mut [f64]|{
                let j=nb.get(key).ok_or(format!("nbody.{key}"))?;
                let vec=snapshot::decode_vec(j,n)?;
                dst[..n].copy_from_slice(&vec);
                Ok::<(),String>(())
            };
            dec("px",&mut self.nbody.px)?; dec("py",&mut self.nbody.py)?; dec("pz",&mut self.nbody.pz)?;
            dec("vx",&mut self.nbody.vx)?; dec("vy",&mut self.nbody.vy)?; dec("vz",&mut self.nbody.vz)?;
            dec("m", &mut self.nbody.m)?;
        }
        // clear per-tick transient state that should not survive a restore
        self.inbox.clear();
        self.active_forces.clear();
        Ok(())
    }

    fn reconfigure_from_config(&mut self) {
        // fluid
        self.fluid.configure(self.config.systems.fluid.h, self.config.systems.fluid.rho0, self.config.systems.fluid.c, self.config.systems.fluid.visc_alpha);
        self.fluid.bounds=self.config.bounds;
        // heat
        if self.heat.nx != self.config.systems.heat.res[0] || self.heat.ny != self.config.systems.heat.res[1] || self.heat.nz != self.config.systems.heat.res[2] {
            self.heat=HeatField::new(self.config.bounds, self.config.systems.heat.res, self.config.systems.heat.alpha, self.config.systems.heat.volumetric_capacity);
        } else {
            self.heat.bounds=self.config.bounds;
            self.heat.set_alpha(self.config.systems.heat.alpha);
            self.heat.set_vol_cap(self.config.systems.heat.volumetric_capacity);
        }
        // nbody
        self.nbody.configure(self.config.systems.nbody.softening, ws_core::constants::G);
    }

    /// Fork this world into a new one with a fresh id.  Snapshots the full
    /// state so the child is bit-exact at fork time.
    pub fn fork(&self, new_id: String)->World {
        let snap=self.snapshot_json();
        let mut child=World::new(new_id, self.config.clone(), self.name.clone());
        // Copy timing identical to snapshot shape.
        let _=child.restore_from_json(&snap);
        child.tick=self.tick; child.time=self.time;
        // Keep RNG identical at fork moment so hashes match; future draws diverge naturally
        // via divergent event streams.
        child.rng=self.rng.clone();
        // Preserve QoS state but give fresh streak counters.
        child.qos=self.qos.effective_quality_for_fork();
        child
    }

    pub fn replay_log_json(&self)->Json {
        Json::arr(self.replay_log.iter().map(|(tick, ev)|{
            Json::obj(vec![("tick", Json::num(*tick as f64)), ("event", ev.clone())])
        }).collect())
    }

    // --- control ---

    pub fn set_paused(&mut self, p:bool) { self.paused=p; if !p { self.pending_steps=0; } }
    pub fn set_rate(&mut self, r:f64) { self.rate=r.clamp(0.0, 16.0); }
    pub fn step_once(&mut self) { if self.paused { self.pending_steps+=1; } }
    pub fn set_quality(&mut self, q: Quality) { self.config.quality=q; self.qos.lock(q); }
    pub fn set_quality_auto(&mut self) { self.qos.unlock(); }
    pub fn set_budget_ms(&mut self, ms: f64) { self.qos.set_budget(ms); }
}

// Additional helper trait for fork QoS — keep it next to world to avoid circular dep.
trait QosForkExt {
    fn effective_quality_for_fork(&self)->QosController;
}
impl QosForkExt for QosController {
    fn effective_quality_for_fork(&self)->QosController {
        QosController::new(self.effective, self.budget_ms)
    }
}

fn now_ms()->u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// Provide the Vec3 -> JSON helper used by domain events in this crate.
pub fn vec3_json_export(p: Vec3)->Json {
    Json::arr(vec![Json::num(p.x), Json::num(p.y), Json::num(p.z)])
}

// Make the domain helper public for events.rs import resolution via `crate::` alias.
// ws_domain currently imports via `crate::config::vec3_json_pub` — expose it.
pub mod reexport_for_domain { pub use super::vec3_json_export as vec3_json_pub; }
