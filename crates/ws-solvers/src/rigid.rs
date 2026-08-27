//! Impulse-based rigid-body solver for **spheres only** (v0.1).
//!
//! Each body is a uniform-density sphere with translational state only (no spin
//! stored — friction is approximated as tangential impulse clamping).  The
//! integration scheme is symplectic Euler (`v ← v + a·dt`, `x ← x + v·dt`) with
//! multi-step substepping controlled by `ws_engine::world`.
//!
//! Contact handling is velocity-level impulse plus Baumgarte positional
//! correction, fed by a [`crate::spatial::UniformGrid`] broad-phase.  Bodies
//! are put to sleep when grounded and almost at rest, and woken by any
//! impulse above a threshold.
//!
//! The solver owns **no RNG** and is fully deterministic (sorted index loops,
//! stable floating point order).

use crate::spatial::UniformGrid;
use ws_core::math::{Aabb, Vec3};

/// Solver tuning knobs (see `ws_domain::config::RigidCfg`).
#[derive(Clone, Debug)]
pub struct RigidParams {
    pub restitution: f64, // 0 = inelastic, 1 = elastic
    pub friction: f64,
}

impl Default for RigidParams {
    fn default() -> Self {
        Self {
            restitution: 0.6,
            friction: 0.35,
        }
    }
}

/// Structure-of-arrays state for sphere bodies.
#[derive(Debug)]
pub struct RigidBodies {
    /// Logical count (active prefix).
    pub n: usize,
    pub cap: usize,

    pub px: Vec<f64>,
    pub py: Vec<f64>,
    pub pz: Vec<f64>,
    pub vx: Vec<f64>,
    pub vy: Vec<f64>,
    pub vz: Vec<f64>,
    /// Extra persistent force accumulators (N); cleared after each step.
    pub fx: Vec<f64>,
    pub fy: Vec<f64>,
    pub fz: Vec<f64>,

    pub r: Vec<f64>,
    pub m: Vec<f64>,
    pub inv_m: Vec<f64>,
    pub restitution: Vec<f64>,

    /// Index into the material palette for the viewer.
    pub mat_id: Vec<u8>,

    pub sleep_counter: Vec<u32>,
    pub asleep: Vec<bool>,
    /// Touched ground or contact this sub-step (reset every step).
    grounded: Vec<bool>,

    // Temporary broad-phase grid, reused across steps to avoid allocation.
    grid: UniformGrid,
}

impl RigidBodies {
    pub fn new(cap: usize) -> Self {
        Self {
            n: 0,
            cap,
            px: vec![0.0; cap],
            py: vec![0.0; cap],
            pz: vec![0.0; cap],
            vx: vec![0.0; cap],
            vy: vec![0.0; cap],
            vz: vec![0.0; cap],
            fx: vec![0.0; cap],
            fy: vec![0.0; cap],
            fz: vec![0.0; cap],
            r: vec![0.0; cap],
            m: vec![0.0; cap],
            inv_m: vec![0.0; cap],
            restitution: vec![0.0; cap],
            mat_id: vec![0; cap],
            sleep_counter: vec![0; cap],
            asleep: vec![false; cap],
            grounded: vec![false; cap],
            grid: UniformGrid::default(),
        }
    }

    pub fn spawn(
        &mut self,
        pos: Vec3,
        vel: Vec3,
        radius: f64,
        density: f64,
        restitution: f64,
        mat_id: u8,
    ) -> Result<u32, String> {
        if self.n >= self.cap {
            return Err("rigid body capacity reached".into());
        }
        let i = self.n;
        self.px[i] = pos.x;
        self.py[i] = pos.y;
        self.pz[i] = pos.z;
        self.vx[i] = vel.x;
        self.vy[i] = vel.y;
        self.vz[i] = vel.z;
        self.r[i] = radius;
        let vol = (4.0 / 3.0) * std::f64::consts::PI * radius.powi(3);
        let mass = (density * vol).max(1e-6);
        self.m[i] = mass;
        self.inv_m[i] = 1.0 / mass;
        self.restitution[i] = restitution.clamp(0.0, 1.0);
        self.mat_id[i] = mat_id;
        self.fx[i] = 0.0;
        self.fy[i] = 0.0;
        self.fz[i] = 0.0;
        self.sleep_counter[i] = 0;
        self.asleep[i] = false;
        self.grounded[i] = false;
        self.n += 1;
        Ok(i as u32)
    }

    pub fn remove(&mut self, idx: u32) -> Result<(), String> {
        let i = idx as usize;
        if i >= self.n {
            return Err(format!("body {idx} out of range (n={})", self.n));
        }
        let last = self.n - 1;
        if i != last {
            for arr in [
                &mut self.px,
                &mut self.py,
                &mut self.pz,
                &mut self.vx,
                &mut self.vy,
                &mut self.vz,
                &mut self.fx,
                &mut self.fy,
                &mut self.fz,
                &mut self.r,
                &mut self.m,
                &mut self.inv_m,
                &mut self.restitution,
            ] {
                arr[i] = arr[last];
            }
            self.mat_id[i] = self.mat_id[last];
            self.sleep_counter[i] = self.sleep_counter[last];
            self.asleep[i] = self.asleep[last];
            self.grounded[i] = self.grounded[last];
        }
        self.n -= 1;
        Ok(())
    }

    pub fn apply_force(&mut self, idx: u32, f: Vec3) {
        let i = idx as usize;
        if i < self.n {
            self.fx[i] += f.x;
            self.fy[i] += f.y;
            self.fz[i] += f.z;
            self.asleep[i] = false;
            self.sleep_counter[i] = 0;
        }
    }

    /// One physics sub-step of `dt` seconds.
    pub fn step(
        &mut self,
        dt: f64,
        gravity: Vec3,
        bounds: Aabb,
        container: bool,
        params: &RigidParams,
    ) {
        if self.n == 0 {
            return;
        }
        let n = self.n;

        // ---- integrate velocities & positions (semi-implicit Euler), skipping deeply asleep bodies
        for i in 0..n {
            if self.asleep[i] {
                continue;
            }
            // Total acceleration = gravity + accumulated external forces / m.
            let ax = gravity.x + self.fx[i] * self.inv_m[i];
            let ay = gravity.y + self.fy[i] * self.inv_m[i];
            let az = gravity.z + self.fz[i] * self.inv_m[i];
            self.vx[i] += ax * dt;
            self.vy[i] += ay * dt;
            self.vz[i] += az * dt;

            // Damping for almost-sleeping near rest (tiny linear viscous term improves stability of resting stacks at negligible cost).
            let speed2 =
                self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            if speed2 < 1e-6 && self.grounded[i] {
                self.vx[i] *= 0.98;
                self.vy[i] *= 0.98;
                self.vz[i] *= 0.98;
            }

            self.px[i] += self.vx[i] * dt;
            self.py[i] += self.vy[i] * dt;
            self.pz[i] += self.vz[i] * dt;

            // clear accumulators — engine re-fills coupling forces each sub-step
            self.fx[i] = 0.0;
            self.fy[i] = 0.0;
            self.fz[i] = 0.0;
            self.grounded[i] = false;
        }

        // ---- wall constraints (container walls at `bounds`)
        if container {
            for i in 0..n {
                if self.asleep[i] {
                    continue;
                }
                let rad = self.r[i];
                // X-
                if self.px[i] - rad < bounds.min.x {
                    self.px[i] = bounds.min.x + rad;
                    if self.vx[i] < 0.0 {
                        self.vx[i] = -self.vx[i] * params.restitution.min(self.restitution[i]);
                    }
                    self.vx[i] *= 0.92; // wall friction bleed
                    if rad > 0.0 {
                        self.grounded[i] = false;
                    }
                }
                // X+
                if self.px[i] + rad > bounds.max.x {
                    self.px[i] = bounds.max.x - rad;
                    if self.vx[i] > 0.0 {
                        self.vx[i] = -self.vx[i] * params.restitution.min(self.restitution[i]);
                    }
                    self.vx[i] *= 0.92;
                }
                // Y-
                if self.py[i] - rad < bounds.min.y {
                    self.py[i] = bounds.min.y + rad;
                    if self.vy[i] < 0.0 {
                        self.vy[i] = -self.vy[i] * params.restitution.min(self.restitution[i]);
                    }
                    if self.vy[i].abs() < 0.05 {
                        self.vy[i] = 0.0;
                    }
                    self.grounded[i] = true;
                    // tangential friction on the ground plane
                    self.vx[i] *= 1.0 - (params.friction * 0.25).clamp(0.0, 0.95);
                    self.vz[i] *= 1.0 - (params.friction * 0.25).clamp(0.0, 0.95);
                }
                // Y+
                if self.py[i] + rad > bounds.max.y {
                    self.py[i] = bounds.max.y - rad;
                    if self.vy[i] > 0.0 {
                        self.vy[i] = -self.vy[i] * params.restitution.min(self.restitution[i]);
                    }
                }
                // Z-
                if self.pz[i] - rad < bounds.min.z {
                    self.pz[i] = bounds.min.z + rad;
                    if self.vz[i] < 0.0 {
                        self.vz[i] = -self.vz[i] * params.restitution.min(self.restitution[i]);
                    }
                    self.vz[i] *= 0.92;
                }
                // Z+
                if self.pz[i] + rad > bounds.max.z {
                    self.pz[i] = bounds.max.z - rad;
                    if self.vz[i] > 0.0 {
                        self.vz[i] = -self.vz[i] * params.restitution.min(self.restitution[i]);
                    }
                    self.vz[i] *= 0.92;
                }
            }
        }

        // ---- broad-phase grid
        // Cell = 2 * max_r gives each body at most ~27 candidates.
        let mut max_r = 0.0f64;
        for &rad in &self.r[..n] {
            if rad > max_r {
                max_r = rad;
            }
        }
        let cell = (max_r * 2.2).max(0.1);
        self.grid.rebuild(
            bounds.min, bounds.max, cell, n, &self.px, &self.py, &self.pz,
        );

        // ---- narrow-phase contacts (impulse + position correction)
        const SLOP: f64 = 1e-4;
        const PERCENT: f64 = 0.80;
        const SLEEP_SPEED2: f64 = 1e-6; // ~1 mm/s
        const SLEEP_FRAMES: u32 = 45; // ~0.75s at 60Hz after grounding
        const WAKE_IMPULSE: f64 = 1e-4;

        // Track which pairs were handled to skip duplicate corrections across the grid's duplicate-cell visitation.
        // Our grid's candidate iterator already de-duplicates, so a single pass suffices.
        self.grid.for_each_candidate_pair(|a, b| {
            let ia = a as usize;
            let ib = b as usize;
            // Skip pair of two sleeping bodies.
            if self.asleep[ia] && self.asleep[ib] {
                return;
            }

            let dx = self.px[ib] - self.px[ia];
            let dy = self.py[ib] - self.py[ia];
            let dz = self.pz[ib] - self.pz[ia];
            let dist2 = dx * dx + dy * dy + dz * dz;
            let sum_r = self.r[ia] + self.r[ib];
            if dist2 >= sum_r * sum_r || dist2 == 0.0 {
                return;
            }
            let dist = dist2.sqrt();
            let penetration = sum_r - dist;
            // Normal from A to B.
            let inv_d = 1.0 / dist;
            let nx = dx * inv_d;
            let ny = dy * inv_d;
            let nz = dz * inv_d;

            // Velocity impulse (only if approaching).
            let dvx = self.vx[ib] - self.vx[ia];
            let dvy = self.vy[ib] - self.vy[ia];
            let dvz = self.vz[ib] - self.vz[ia];
            let vn = dvx * nx + dvy * ny + dvz * nz;
            if vn < -1e-9 {
                let e = (self.restitution[ia].min(self.restitution[ib])).min(params.restitution);
                let inv_sum = self.inv_m[ia] + self.inv_m[ib];
                if inv_sum > 0.0 {
                    let j = -(1.0 + e) * vn / inv_sum;
                    // Decide whether to wake sleepers.
                    let wake = j * inv_sum; // normalized impulse magnitude
                    if wake.abs() > WAKE_IMPULSE {
                        if self.asleep[ia] {
                            self.asleep[ia] = false;
                            self.sleep_counter[ia] = 0;
                        }
                        if self.asleep[ib] {
                            self.asleep[ib] = false;
                            self.sleep_counter[ib] = 0;
                        }
                    }
                    if !self.asleep[ia] || !self.asleep[ib] {
                        // Normal impulse
                        let imp = j;
                        if !self.asleep[ia] {
                            self.vx[ia] -= imp * nx * self.inv_m[ia];
                            self.vy[ia] -= imp * ny * self.inv_m[ia];
                            self.vz[ia] -= imp * nz * self.inv_m[ia];
                        }
                        if !self.asleep[ib] {
                            self.vx[ib] += imp * nx * self.inv_m[ib];
                            self.vy[ib] += imp * ny * self.inv_m[ib];
                            self.vz[ib] += imp * nz * self.inv_m[ib];
                        }
                        // Friction impulse — clamp by μ|j|.
                        let vt_x = dvx - vn * nx;
                        let vt_y = dvy - vn * ny;
                        let vt_z = dvz - vn * nz;
                        let vt_len2 = vt_x * vt_x + vt_y * vt_y + vt_z * vt_z;
                        if vt_len2 > 1e-12 {
                            let vt_len = vt_len2.sqrt();
                            let tx = vt_x / vt_len;
                            let ty = vt_y / vt_len;
                            let tz = vt_z / vt_len;
                            let jt_raw = -vt_len / inv_sum;
                            let mu = params.friction;
                            let limit = mu * j.abs();
                            let jt = jt_raw.clamp(-limit, limit);
                            if !self.asleep[ia] {
                                self.vx[ia] -= jt * tx * self.inv_m[ia];
                                self.vy[ia] -= jt * ty * self.inv_m[ia];
                                self.vz[ia] -= jt * tz * self.inv_m[ia];
                            }
                            if !self.asleep[ib] {
                                self.vx[ib] += jt * tx * self.inv_m[ib];
                                self.vy[ib] += jt * ty * self.inv_m[ib];
                                self.vz[ib] += jt * tz * self.inv_m[ib];
                            }
                        }
                    }
                }
            }
            // Baumgarte position correction (always, even if separating — prevents sinking).
            let corr_depth = (penetration - SLOP).max(0.0);
            if corr_depth > 0.0 {
                let inv_sum = self.inv_m[ia] + self.inv_m[ib];
                if inv_sum > 0.0 {
                    let c = (corr_depth / inv_sum) * PERCENT;
                    if !self.asleep[ia] {
                        self.px[ia] -= c * nx * self.inv_m[ia];
                        self.py[ia] -= c * ny * self.inv_m[ia];
                        self.pz[ia] -= c * nz * self.inv_m[ia];
                    }
                    if !self.asleep[ib] {
                        self.px[ib] += c * nx * self.inv_m[ib];
                        self.py[ib] += c * ny * self.inv_m[ib];
                        self.pz[ib] += c * nz * self.inv_m[ib];
                    }
                }
            }
            // Mark grounded if contact normal has significant upward component.
            if ny > 0.35 {
                // A's top was hit (B above A) → A feels contact.
                // B's bottom was hit       → B grounded.
                if !self.asleep[ib] {
                    self.grounded[ib] = true;
                }
            }
        });

        // ---- sleep / wake update (after contacts — newly woken bodies keep their computed velocities)
        for i in 0..n {
            if self.asleep[i] {
                continue;
            }
            let speed2 =
                self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            if speed2 < SLEEP_SPEED2 && self.grounded[i] {
                self.sleep_counter[i] += 1;
                if self.sleep_counter[i] >= SLEEP_FRAMES {
                    self.asleep[i] = true;
                    self.vx[i] = 0.0;
                    self.vy[i] = 0.0;
                    self.vz[i] = 0.0;
                }
            } else {
                self.sleep_counter[i] = 0;
                // Not grounded or moving → cannot sleep.
                if !self.grounded[i] && speed2 < 1e-9 {
                    // Allow drift without grounding to damp slowly — not sleep.
                }
            }
        }
    }

    pub fn kinetic_energy(&self) -> f64 {
        let mut e = 0.0;
        for i in 0..self.n {
            let v2 = self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            e += 0.5 * self.m[i] * v2;
        }
        e
    }

    #[allow(dead_code)]
    pub fn potential_energy(&self, gravity: Vec3, y0: f64) -> f64 {
        let g = gravity.length();
        if g == 0.0 {
            return 0.0;
        }
        let mut e = 0.0;
        for i in 0..self.n {
            // m·g·h with reference y0 (bottom of domain).
            let h = (self.py[i] - y0).max(0.0);
            e += self.m[i] * g * h;
        }
        e
    }
}
