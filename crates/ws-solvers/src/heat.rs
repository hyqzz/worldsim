//! Scalar heat-field solver on a uniform grid.
//!
//! Physics: linear heat equation `∂T/∂t = α ∇²T` plus semi-Lagrangian advection
//! by the interpolated fluid velocity (one-way fluid → heat at v0.1) plus
//! volumetric heat sources.
//!
//! Numerics:
//! - Grid: uniform, dimensions `nx × ny × nz` over `bounds`.
//! - Advection (if a velocity sampler is provided): semi-Lagrangian
//!   back-trace (`x* = x − v·dt`, trilinear probe) — unconditionally stable.
//! - Diffusion: explicit 7-point Laplacian with Neumann (insulated) boundaries.
//!   Internal substepping ensures `dt ≤ dx²/(6α)`.
//! - Sources: uniform energy injection across cells intersecting each sphere.
//!
//! The field is sampled via trilinear interpolation for probe and coupling use.

use ws_core::math::{Aabb, Vec3};

#[inline]
fn heat_idx(nx: u32, ny: u32, ix: u32, iy: u32, iz: u32) -> usize {
    ix as usize + nx as usize * (iy as usize + ny as usize * iz as usize)
}

/// Serializable state of a volumetric heat source.
#[derive(Clone, Debug)]
pub struct HeatSource {
    pub pos: Vec3,
    pub radius: f64,
    pub power_w: f64,
    pub ttl: u64, // 0 = persistent, else decrement each step and auto-remove when 0
}

/// Dense temperature field.
#[derive(Debug)]
pub struct HeatField {
    pub nx: u32,
    pub ny: u32,
    pub nz: u32,
    /// Row-major `(ix, iy, iz)` linearisation: `ix + nx·(iy + ny·iz)`.
    pub t: Vec<f64>,
    next_t: Vec<f64>,
    pub bounds: Aabb,
    pub dx: f64,
    pub dy: f64,
    pub dz: f64,
    pub alpha: f64,
    pub vol_cap: f64, // ρc_p

    sources: Vec<HeatSource>,
    /// Default temperature at which the grid is initialised/reset.
    pub default_temp: f64,
}

impl HeatField {
    pub fn new(bounds: Aabb, res: [u32; 3], alpha: f64, vol_cap: f64) -> Self {
        let nx = res[0].max(4);
        let ny = res[1].max(4);
        let nz = res[2].max(4);
        let dx = (bounds.max.x - bounds.min.x) / (nx as f64);
        let dy = (bounds.max.y - bounds.min.y) / (ny as f64);
        let dz = (bounds.max.z - bounds.min.z) / (nz as f64);
        let n = (nx * ny * nz) as usize;
        let t = vec![293.15; n];
        Self {
            nx,
            ny,
            nz,
            t: t.clone(),
            next_t: t.clone(),
            bounds,
            dx,
            dy,
            dz,
            alpha,
            vol_cap,
            sources: Vec::new(),
            default_temp: 293.15,
        }
    }

    pub fn configure(&mut self, res: [u32; 3], alpha: f64, vol_cap: f64) {
        let old_t = self.t.clone();
        let old_bounds = self.bounds;
        let old_nx = self.nx;
        let old_ny = self.ny;
        let old_nz = self.nz;
        let sources = self.sources.clone();
        let default_temp = self.default_temp;
        *self = Self::new(self.bounds, res, alpha, vol_cap);
        self.sources = sources;
        self.default_temp = default_temp;
        // Resample old field onto new grid (trilinear).
        if !old_t.is_empty() {
            for iz in 0..self.nz {
                for iy in 0..self.ny {
                    for ix in 0..self.nx {
                        let centre = self.cell_centre(ix, iy, iz);
                        let v =
                            trilinear(&old_t, old_bounds, [old_nx, old_ny, old_nz], centre, 293.15);
                        let idx = heat_idx(self.nx, self.ny, ix, iy, iz);
                        self.t[idx] = v;
                    }
                }
            }
            self.next_t = self.t.clone();
        }
    }

    pub fn add_source(&mut self, pos: Vec3, radius: f64, power_w: f64, ttl_ticks: u64) -> u32 {
        let id = self.sources.len() as u32;
        self.sources.push(HeatSource {
            pos,
            radius,
            power_w,
            ttl: ttl_ticks,
        });
        id
    }
    pub fn remove_source(&mut self, idx: u32) -> Result<(), String> {
        let i = idx as usize;
        if i >= self.sources.len() {
            return Err(format!("heat source {idx} out of range"));
        }
        self.sources.remove(i);
        Ok(())
    }
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
    /// Advance finite source lifetimes by one **macro** simulation tick.
    /// The engine calls this once after all substeps; decrementing inside
    /// `step()` would make duration depend on the selected precision.
    pub fn advance_source_lifetimes(&mut self) {
        let mut expired = Vec::new();
        for (i, source) in self.sources.iter_mut().enumerate() {
            if source.ttl > 0 {
                source.ttl -= 1;
                if source.ttl == 0 {
                    expired.push(i);
                }
            }
        }
        for i in expired.into_iter().rev() {
            self.sources.remove(i);
        }
    }
    /// Read-only source state for snapshots and diagnostics.
    pub fn sources(&self) -> &[HeatSource] {
        &self.sources
    }
    /// Replace all sources while restoring a snapshot.
    pub fn restore_sources(&mut self, sources: Vec<HeatSource>) {
        self.sources = sources;
    }
    /// Replace both temperature buffers with a validated grid.
    pub fn restore_temperature_grid(&mut self, values: Vec<f64>) -> Result<(), String> {
        if values.len() != self.t.len() {
            return Err(format!(
                "temperature grid length {} != {}",
                values.len(),
                self.t.len()
            ));
        }
        self.t = values.clone();
        self.next_t = values;
        Ok(())
    }
    pub fn set_alpha(&mut self, a: f64) {
        self.alpha = a.max(1e-12);
    }
    pub fn set_vol_cap(&mut self, c: f64) {
        self.vol_cap = c.max(1e3);
    }
    pub fn reset(&mut self) {
        self.t.fill(self.default_temp);
        self.next_t = self.t.clone();
        // persistent sources (ttl==0) survive reset — consistent with "apparatus in the lab".
    }

    /// One macro-step of physical duration `dt` seconds.  `sample_vel` may be
    /// `None` if no fluid is present (pure diffusion + sources).
    pub fn step<F>(&mut self, dt: f64, sample_vel: Option<F>)
    where
        F: Fn(Vec3) -> Vec3,
    {
        if dt <= 0.0 {
            return;
        }
        // Source lifetimes are macro-tick state and are advanced by the
        // engine after this call, not once per diffusion substep.
        // Decide explicit-diffusion substeps: dt ≤ dx²/(6α).  Take min(dx,dy,dz)² as limiting spacing.
        let dx_min = self.dx.min(self.dy).min(self.dz);
        let dt_stab = if self.alpha > 1e-12 {
            dx_min * dx_min / (6.0 * self.alpha)
        } else {
            dt
        };
        let n_sub = ((dt / dt_stab).ceil() as usize).clamp(1, 1024);
        let dt_sub = dt / (n_sub as f64);

        let nx = self.nx as usize;
        let ny = self.ny as usize;
        let nz = self.nz as usize;
        let inv_dx = 1.0 / self.dx;
        let inv_dy = 1.0 / self.dy;
        let inv_dz = 1.0 / self.dz;
        let inv_dx2 = inv_dx * inv_dx;
        let inv_dy2 = inv_dy * inv_dy;
        let inv_dz2 = inv_dz * inv_dz;

        for _ in 0..n_sub {
            // ---- semi-Lagrangian advection into `next_t`
            if let Some(ref sampler) = sample_vel {
                let nx_u = self.nx;
                let ny_u = self.ny;
                let nz_u = self.nz;
                let b = self.bounds;
                for iz in 0..nz {
                    for iy in 0..ny {
                        for ix in 0..nx {
                            let c = self.cell_centre(ix as u32, iy as u32, iz as u32);
                            let v = sampler(c);
                            let back = Vec3::new(
                                c.x - v.x * dt_sub,
                                c.y - v.y * dt_sub,
                                c.z - v.z * dt_sub,
                            );
                            let idx = heat_idx(nx_u, ny_u, ix as u32, iy as u32, iz as u32);
                            self.next_t[idx] =
                                trilinear(&self.t, b, [nx_u, ny_u, nz_u], back, self.default_temp);
                        }
                    }
                }
                // Swap roles so diffusion reads the advected field.
                std::mem::swap(&mut self.t, &mut self.next_t);
            }

            // ---- explicit diffusion (reads `self.t`, writes `self.next_t`)
            if self.alpha > 1e-14 && dt_sub > 0.0 {
                for iz in 0..nz {
                    for iy in 0..ny {
                        for ix in 0..nx {
                            let i0 = heat_idx(self.nx, self.ny, ix as u32, iy as u32, iz as u32);
                            let centre = self.t[i0];
                            let xm = if ix > 0 {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    (ix - 1) as u32,
                                    iy as u32,
                                    iz as u32,
                                )]
                            } else {
                                centre
                            };
                            let xp = if ix + 1 < nx {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    (ix + 1) as u32,
                                    iy as u32,
                                    iz as u32,
                                )]
                            } else {
                                centre
                            };
                            let ym = if iy > 0 {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    ix as u32,
                                    (iy - 1) as u32,
                                    iz as u32,
                                )]
                            } else {
                                centre
                            };
                            let yp = if iy + 1 < ny {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    ix as u32,
                                    (iy + 1) as u32,
                                    iz as u32,
                                )]
                            } else {
                                centre
                            };
                            let zm = if iz > 0 {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    ix as u32,
                                    iy as u32,
                                    (iz - 1) as u32,
                                )]
                            } else {
                                centre
                            };
                            let zp = if iz + 1 < nz {
                                self.t[heat_idx(
                                    self.nx,
                                    self.ny,
                                    ix as u32,
                                    iy as u32,
                                    (iz + 1) as u32,
                                )]
                            } else {
                                centre
                            };
                            let lap = (xm - 2.0 * centre + xp) * inv_dx2
                                + (ym - 2.0 * centre + yp) * inv_dy2
                                + (zm - 2.0 * centre + zp) * inv_dz2;
                            self.next_t[i0] = centre + self.alpha * dt_sub * lap;
                        }
                    }
                }
                std::mem::swap(&mut self.t, &mut self.next_t);
            }

            // ---- sources (applied per sub-step split equally)
            if !self.sources.is_empty() && dt_sub > 0.0 {
                let cell_vol = self.dx * self.dy * self.dz;
                let dt_fraction = dt_sub; // energy per sub-step is power*dt_sub
                                          // Collect affected cell indices per source then distribute.
                                          // Small source count (usually 1–3) so O(nx*ny*nz * ns) linear scan is acceptable in Rust
                                          // (≈ 50k cells × 3 ⇒ 150k checks per step, trivial).
                                          // Batch into a vec of (src_idx, cell_idx) pairs.
                for src in &self.sources {
                    let r2 = src.radius * src.radius;
                    let mut touched = Vec::new();
                    for iz in 0..nz {
                        for iy in 0..ny {
                            for ix in 0..nx {
                                let c = self.cell_centre(ix as u32, iy as u32, iz as u32);
                                let dx = c.x - src.pos.x;
                                let dy = c.y - src.pos.y;
                                let dz = c.z - src.pos.z;
                                if dx * dx + dy * dy + dz * dz <= r2 {
                                    touched.push(heat_idx(
                                        self.nx, self.ny, ix as u32, iy as u32, iz as u32,
                                    ));
                                }
                            }
                        }
                    }
                    if touched.is_empty() {
                        continue;
                    }
                    // Total energy for this sub-step.
                    let e_total = src.power_w * dt_fraction;
                    let e_per = e_total / (touched.len() as f64);
                    let d_t_per = e_per / (self.vol_cap * cell_vol);
                    for idx in touched {
                        self.t[idx] += d_t_per;
                    }
                }
            }
        }
    }

    // ---- sampling ----

    /// Trilinear interpolation at an arbitrary world point.  Points outside the
    /// domain are clamped to the boundary value.
    pub fn sample(&self, p: Vec3) -> f64 {
        trilinear(
            &self.t,
            self.bounds,
            [self.nx, self.ny, self.nz],
            p,
            self.default_temp,
        )
    }

    pub fn total_heat(&self) -> f64 {
        // ∫ ρc_p · T  dV  (up to a reference — report as thermal energy relative to 0K for comparisons)
        let vol = self.dx * self.dy * self.dz;
        self.t.iter().map(|&t| t * self.vol_cap * vol).sum()
    }

    pub fn average_temperature(&self) -> f64 {
        if self.t.is_empty() {
            return self.default_temp;
        }
        self.t.iter().sum::<f64>() / (self.t.len() as f64)
    }

    // ---- index helpers ----

    #[inline]
    fn cell_centre(&self, ix: u32, iy: u32, iz: u32) -> Vec3 {
        Vec3::new(
            self.bounds.min.x + (ix as f64 + 0.5) * self.dx,
            self.bounds.min.y + (iy as f64 + 0.5) * self.dy,
            self.bounds.min.z + (iz as f64 + 0.5) * self.dz,
        )
    }
}

fn trilinear(t: &[f64], bounds: Aabb, res: [u32; 3], p: Vec3, default: f64) -> f64 {
    if t.is_empty() {
        return default;
    }
    let nx = res[0] as f64;
    let ny = res[1] as f64;
    let nz = res[2] as f64;
    let fx =
        ((p.x - bounds.min.x) / (bounds.max.x - bounds.min.x) * nx - 0.5).clamp(0.0, nx - 1.0001);
    let fy =
        ((p.y - bounds.min.y) / (bounds.max.y - bounds.min.y) * ny - 0.5).clamp(0.0, ny - 1.0001);
    let fz =
        ((p.z - bounds.min.z) / (bounds.max.z - bounds.min.z) * nz - 0.5).clamp(0.0, nz - 1.0001);
    let ix0 = fx.floor() as usize;
    let iy0 = fy.floor() as usize;
    let iz0 = fz.floor() as usize;
    let ix1 = (ix0 + 1).min(res[0] as usize - 1);
    let iy1 = (iy0 + 1).min(res[1] as usize - 1);
    let iz1 = (iz0 + 1).min(res[2] as usize - 1);
    let tx = fx - ix0 as f64;
    let ty = fy - iy0 as f64;
    let tz = fz - iz0 as f64;
    let idx = |x, y, z| x + res[0] as usize * (y + res[1] as usize * z);
    let c000 = t[idx(ix0, iy0, iz0)];
    let c100 = t[idx(ix1, iy0, iz0)];
    let c010 = t[idx(ix0, iy1, iz0)];
    let c110 = t[idx(ix1, iy1, iz0)];
    let c001 = t[idx(ix0, iy0, iz1)];
    let c101 = t[idx(ix1, iy0, iz1)];
    let c011 = t[idx(ix0, iy1, iz1)];
    let c111 = t[idx(ix1, iy1, iz1)];
    let c00 = c000 * (1.0 - tx) + c100 * tx;
    let c01 = c001 * (1.0 - tx) + c101 * tx;
    let c10 = c010 * (1.0 - tx) + c110 * tx;
    let c11 = c011 * (1.0 - tx) + c111 * tx;
    let c0 = c00 * (1.0 - ty) + c10 * ty;
    let c1 = c01 * (1.0 - ty) + c11 * ty;
    c0 * (1.0 - tz) + c1 * tz
}
