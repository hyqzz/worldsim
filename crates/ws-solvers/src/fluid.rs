//! Weakly-compressible SPH (WCSPH) fluid.
//!
//! Kernel: cubic spline with support `2h`, normalization `σ = 1/(π h³)`.
//! Equation of state: Tait `p = B((ρ/ρ₀)^γ − 1)`, `B = c²ρ₀/γ`, `γ = 7`.
//! Viscous term: Monaghan artificial viscosity (α only, β = 0).
//! Integration: symplectic Euler.  Wall handling via soft penalty + hard
//! position clamp inside the container. Neighbour search via
//! [`crate::spatial::UniformGrid`] with cell `2h` (27 cells per particle).

use ws_core::math::{Aabb, Vec3};

use crate::spatial::UniformGrid;

const GAMMA: f64 = 7.0;
const CORE_EPS_H2: f64 = 0.01; // factor × h² added to r² denominators

#[inline]
fn cubic_kernel(r: f64, h: f64, sigma: f64) -> f64 {
    let q = r / h;
    if r < 0.0 {
        0.0
    } else if q <= 1.0 {
        sigma * (1.0 - 1.5 * q * q + 0.75 * q * q * q)
    } else if q <= 2.0 {
        sigma * 0.25 * (2.0 - q).powi(3)
    } else {
        0.0
    }
}

#[derive(Debug)]
pub struct Fluid {
    pub n: usize,
    pub cap: usize,

    pub px: Vec<f64>,
    pub py: Vec<f64>,
    pub pz: Vec<f64>,
    pub vx: Vec<f64>,
    pub vy: Vec<f64>,
    pub vz: Vec<f64>,
    // Accelerations recomputed every density/force pass.
    pub ax: Vec<f64>,
    pub ay: Vec<f64>,
    pub az: Vec<f64>,
    pub rho: Vec<f64>,
    pub p: Vec<f64>,

    pub h: f64,
    pub rho0: f64,
    pub c: f64,
    pub visc_alpha: f64,
    b_tait: f64, // c²ρ₀/γ
    sigma: f64,  // 1/(π h³)

    pub bounds: Aabb,
    /// Mass per particle assuming uniform lattice spacing.
    pub mass: f64,

    grid: UniformGrid,
}

impl Fluid {
    pub fn new(cap: usize, h: f64, rho0: f64, c: f64, visc_alpha: f64, bounds: Aabb) -> Self {
        let sigma = 1.0 / (std::f64::consts::PI * h.powi(3));
        let b = c * c * rho0 / GAMMA;
        let dp = h / 1.3;
        let mass = rho0 * dp.powi(3);
        Self {
            n: 0,
            cap,
            px: vec![0.0; cap],
            py: vec![0.0; cap],
            pz: vec![0.0; cap],
            vx: vec![0.0; cap],
            vy: vec![0.0; cap],
            vz: vec![0.0; cap],
            ax: vec![0.0; cap],
            ay: vec![0.0; cap],
            az: vec![0.0; cap],
            rho: vec![0.0; cap],
            p: vec![0.0; cap],
            h,
            rho0,
            c,
            visc_alpha,
            b_tait: b,
            sigma,
            bounds,
            mass,
            grid: UniformGrid::default(),
        }
    }

    pub fn configure(&mut self, h: f64, rho0: f64, c: f64, visc_alpha: f64) {
        self.h = h;
        self.rho0 = rho0;
        self.c = c;
        self.visc_alpha = visc_alpha;
        self.sigma = 1.0 / (std::f64::consts::PI * h.powi(3));
        self.b_tait = c * c * rho0 / GAMMA;
        let dp = h / 1.3;
        self.mass = rho0 * dp.powi(3);
    }

    pub fn clear(&mut self) {
        self.n = 0;
    }

    /// Fill a spherical blob at `center` / `radius` with lattice-placed particles
    /// jittered by `jitter` uniform in `[-j,j]`.  Returns number actually added.
    pub fn spawn_blob(
        &mut self,
        center: Vec3,
        radius: f64,
        jitter: f64,
        rng: &mut ws_core::rng::Rng,
    ) -> usize {
        let dp = self.h / 1.3;
        let jitter = jitter.max(0.0).min(dp * 0.4);
        let mut added = 0usize;
        let steps = (radius / dp).ceil() as i32 + 1;
        'outer: for iz in -steps..=steps {
            for iy in -steps..=steps {
                for ix in -steps..=steps {
                    if self.n >= self.cap {
                        break 'outer;
                    }
                    let mut p = Vec3::new(
                        center.x + ix as f64 * dp,
                        center.y + iy as f64 * dp,
                        center.z + iz as f64 * dp,
                    );
                    // Keep only those inside the sphere (with small shrink so edge layer not sparse).
                    if p.sub(center).length_squared() > radius * radius {
                        continue;
                    }
                    // Jitter inside the central region only.
                    if jitter > 0.0 {
                        p.x += rng.next_range(-jitter, jitter);
                        p.y += rng.next_range(-jitter, jitter);
                        p.z += rng.next_range(-jitter, jitter);
                    }
                    // Clamp into container interior (avoid spawning stuck in wall).
                    if p.x < self.bounds.min.x + self.h * 0.5
                        || p.x > self.bounds.max.x - self.h * 0.5
                    {
                        continue;
                    }
                    if p.y < self.bounds.min.y + self.h * 0.5
                        || p.y > self.bounds.max.y - self.h * 0.5
                    {
                        continue;
                    }
                    if p.z < self.bounds.min.z + self.h * 0.5
                        || p.z > self.bounds.max.z - self.h * 0.5
                    {
                        continue;
                    }
                    let i = self.n;
                    self.px[i] = p.x;
                    self.py[i] = p.y;
                    self.pz[i] = p.z;
                    self.vx[i] = 0.0;
                    self.vy[i] = 0.0;
                    self.vz[i] = 0.0;
                    self.rho[i] = self.rho0;
                    self.p[i] = 0.0;
                    self.n += 1;
                    added += 1;
                }
            }
        }
        added
    }

    // ---------- kernel helpers (support 2h) ----------

    // ---------- main step ----------

    /// Perform one sub-step of size `dt`.  Reads gravity, writes accelerations,
    /// then integrates.  Deterministic and self-contained (no RNG inside except
    /// for seed-less lattice — spawn only).
    pub fn step(&mut self, dt: f64, gravity: Vec3, container: bool) {
        let n = self.n;
        if n == 0 {
            return;
        }

        // ---- neighbour grid (cell = 2h)
        let cell = self.h * 2.0;
        self.grid.rebuild(
            self.bounds.min,
            self.bounds.max,
            cell,
            n,
            &self.px,
            &self.py,
            &self.pz,
        );

        // ---- density (self + neighbours)
        let w0 = cubic_kernel(0.0, self.h, self.sigma);
        for i in 0..n {
            self.rho[i] = self.mass * w0;
        }
        {
            let h = self.h;
            let sigma = self.sigma;
            let mass = self.mass;
            let px = &self.px;
            let py = &self.py;
            let pz = &self.pz;
            let rho = &mut self.rho;
            let grid = &self.grid;
            let w_local = |r: f64| -> f64 {
                let q = r / h;
                if r < 0.0 {
                    0.0
                } else if q <= 1.0 {
                    sigma * (1.0 - 1.5 * q * q + 0.75 * q * q * q)
                } else if q <= 2.0 {
                    sigma * 0.25 * (2.0 - q).powi(3)
                } else {
                    0.0
                }
            };
            grid.for_each_candidate_pair(|a, b| {
                let ia = a as usize;
                let ib = b as usize;
                let dx = px[ia] - px[ib];
                let dy = py[ia] - py[ib];
                let dz = pz[ia] - pz[ib];
                let r = (dx * dx + dy * dy + dz * dz).sqrt();
                if r >= 2.0 * h || r == 0.0 {
                    return;
                }
                let w = w_local(r);
                let contrib = mass * w;
                rho[ia] += contrib;
                rho[ib] += contrib;
            });
        }

        // ---- pressure from EOS
        for i in 0..n {
            let ratio = self.rho[i] / self.rho0;
            // Clamp ratio to avoid extreme pressure from density spikes on startup.
            let ratio_clamped = ratio.clamp(0.6, 1.8);
            self.p[i] = self.b_tait * ((ratio_clamped).powf(GAMMA) - 1.0);
        }

        // ---- forces (pressure + viscosity) via pair loop, plus gravity
        for i in 0..n {
            self.ax[i] = gravity.x;
            self.ay[i] = gravity.y;
            self.az[i] = gravity.z;
        }
        {
            let h = self.h;
            let sigma = self.sigma;
            let mass = self.mass;
            let visc_alpha = self.visc_alpha;
            let c = self.c;
            let px = &self.px;
            let py = &self.py;
            let pz = &self.pz;
            let vx = &self.vx;
            let vy = &self.vy;
            let vz = &self.vz;
            let rho = &self.rho;
            let p = &self.p;
            let ax = &mut self.ax;
            let ay = &mut self.ay;
            let az = &mut self.az;
            let grid = &self.grid;
            let dwd_over_r_local = |r: f64| -> f64 {
                if r < 1e-9 {
                    return 0.0;
                }
                let q = r / h;
                let dwd_r = if q <= 1.0 {
                    sigma * (-3.0 * q + 2.25 * q * q) / h
                } else if q <= 2.0 {
                    let om = 2.0 - q;
                    sigma * (-0.75 * om * om) / h
                } else {
                    0.0
                };
                dwd_r / r
            };
            grid.for_each_candidate_pair(|a, b| {
                let ia = a as usize;
                let ib = b as usize;
                let dx = px[ia] - px[ib];
                let dy = py[ia] - py[ib];
                let dz = pz[ia] - pz[ib];
                let r2 = dx * dx + dy * dy + dz * dz;
                let h2 = (2.0 * h) * (2.0 * h);
                if r2 >= h2 || r2 == 0.0 {
                    return;
                }
                let r = r2.sqrt();
                let dwd_r_over_r = dwd_over_r_local(r);
                let rho_a = rho[ia];
                let rho_b = rho[ib];
                let press_factor = -(mass) * (p[ia] / (rho_a * rho_a) + p[ib] / (rho_b * rho_b));
                let gx = dx * dwd_r_over_r;
                let gy = dy * dwd_r_over_r;
                let gz = dz * dwd_r_over_r;
                ax[ia] += press_factor * gx;
                ay[ia] += press_factor * gy;
                az[ia] += press_factor * gz;
                ax[ib] -= press_factor * gx;
                ay[ib] -= press_factor * gy;
                az[ib] -= press_factor * gz;
                if visc_alpha != 0.0 {
                    let dvx = vx[ia] - vx[ib];
                    let dvy = vy[ia] - vy[ib];
                    let dvz = vz[ia] - vz[ib];
                    let dot = dvx * dx + dvy * dy + dvz * dz;
                    if dot < 0.0 {
                        let mu = h * dot / (r2 + CORE_EPS_H2 * h * h);
                        let rho_bar = 0.5 * (rho_a + rho_b);
                        let pi_ab = -visc_alpha * c * mu / rho_bar;
                        let visc_factor = -mass * pi_ab;
                        ax[ia] += visc_factor * gx;
                        ay[ia] += visc_factor * gy;
                        az[ia] += visc_factor * gz;
                        ax[ib] -= visc_factor * gx;
                        ay[ib] -= visc_factor * gy;
                        az[ib] -= visc_factor * gz;
                    }
                }
            });
        }

        // ---- wall penalty (soft, isotropic — only within 0.8h of each face)
        if container {
            let pen_k = self.rho0 * self.c * self.c * 0.12; // tuned acceleration scale
            let skin = self.h * 0.8;
            for i in 0..n {
                let x = self.px[i];
                let y = self.py[i];
                let z = self.pz[i];
                // Each face adds acceleration proportional to penetration inside the skin.
                if x - self.bounds.min.x < skin {
                    let d = skin - (x - self.bounds.min.x);
                    self.ax[i] += pen_k * (d / self.h) / 1000.0;
                }
                if self.bounds.max.x - x < skin {
                    let d = skin - (self.bounds.max.x - x);
                    self.ax[i] -= pen_k * (d / self.h) / 1000.0;
                }
                if y - self.bounds.min.y < skin {
                    let d = skin - (y - self.bounds.min.y);
                    self.ay[i] += pen_k * (d / self.h) / 1000.0;
                }
                if self.bounds.max.y - y < skin {
                    let d = skin - (self.bounds.max.y - y);
                    self.ay[i] -= pen_k * (d / self.h) / 1000.0;
                }
                if z - self.bounds.min.z < skin {
                    let d = skin - (z - self.bounds.min.z);
                    self.az[i] += pen_k * (d / self.h) / 1000.0;
                }
                if self.bounds.max.z - z < skin {
                    let d = skin - (self.bounds.max.z - z);
                    self.az[i] -= pen_k * (d / self.h) / 1000.0;
                }
            }
        }

        // ---- integrate (symplectic Euler)
        for i in 0..n {
            self.vx[i] += self.ax[i] * dt;
            self.vy[i] += self.ay[i] * dt;
            self.vz[i] += self.az[i] * dt;
            self.px[i] += self.vx[i] * dt;
            self.py[i] += self.vy[i] * dt;
            self.pz[i] += self.vz[i] * dt;
        }

        // ---- hard wall clamp (containment guarantee) + velocity reflection
        if container {
            let damp = 0.2; // restitution-like for the wall
            let margin = self.h * 0.5;
            let min = self.bounds.min;
            let max = self.bounds.max;
            for i in 0..n {
                if self.px[i] < min.x + margin {
                    self.px[i] = min.x + margin;
                    if self.vx[i] < 0.0 {
                        self.vx[i] *= -damp;
                    }
                }
                if self.px[i] > max.x - margin {
                    self.px[i] = max.x - margin;
                    if self.vx[i] > 0.0 {
                        self.vx[i] *= -damp;
                    }
                }
                if self.py[i] < min.y + margin {
                    self.py[i] = min.y + margin;
                    if self.vy[i] < 0.0 {
                        self.vy[i] *= -damp;
                    }
                }
                if self.py[i] > max.y - margin {
                    self.py[i] = max.y - margin;
                    if self.vy[i] > 0.0 {
                        self.vy[i] *= -damp;
                    }
                }
                if self.pz[i] < min.z + margin {
                    self.pz[i] = min.z + margin;
                    if self.vz[i] < 0.0 {
                        self.vz[i] *= -damp;
                    }
                }
                if self.pz[i] > max.z - margin {
                    self.pz[i] = max.z - margin;
                    if self.vz[i] > 0.0 {
                        self.vz[i] *= -damp;
                    }
                }
            }
        }
    }

    /// CFL-stable dt estimate for the current state.
    pub fn suggested_dt(&self) -> f64 {
        let mut vmax = 0.0;
        for i in 0..self.n {
            let v2 = self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            let v = v2.sqrt();
            if v > vmax {
                vmax = v;
            }
        }
        let mut amax = 0.0;
        for i in 0..self.n {
            let a2 = self.ax[i] * self.ax[i] + self.ay[i] * self.ay[i] + self.az[i] * self.az[i];
            let a = a2.sqrt();
            if a > amax {
                amax = a;
            }
        }
        let dt_a = if amax > 1e-9 {
            (self.h / amax).sqrt() * 0.25
        } else {
            1e3
        };
        let dt_c = 0.25 * self.h / (self.c + vmax + 1e-9);
        dt_a.min(dt_c).clamp(1e-5, 0.02)
    }

    // ---- sampling helpers for coupling/probes ----

    /// Kernel-weighted density estimate at an arbitrary point. Once a step has
    /// built the uniform grid this visits only the 27 nearby cells; before the
    /// first step it falls back to a bounded linear scan.
    pub fn sample_density(&self, p: Vec3) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let h = self.h;
        let sigma = self.sigma;
        let mass = self.mass;
        let r2cut = (2.0 * h) * (2.0 * h);
        let mut dens = 0.0;
        let mut add = |i: u32| {
            let i = i as usize;
            let dx = p.x - self.px[i];
            let dy = p.y - self.py[i];
            let dz = p.z - self.pz[i];
            let r2 = dx * dx + dy * dy + dz * dz;
            if r2 < r2cut {
                dens += mass * cubic_kernel(r2.sqrt(), h, sigma);
            }
        };
        if self.grid.cell_count() > 0 {
            self.grid.for_each_in_neighbourhood(p, &mut add);
        } else {
            for i in 0..self.n {
                add(i as u32);
            }
        }
        dens
    }

    /// Kernel-weighted velocity. The denominator is the SPH interpolation
    /// weight; this must divide the numerator (not multiply by the denominator).
    pub fn sample_velocity(&self, p: Vec3) -> Vec3 {
        if self.n == 0 {
            return Vec3::zero();
        }
        let h = self.h;
        let sigma = self.sigma;
        let mass = self.mass;
        let r2cut = (2.0 * h) * (2.0 * h);
        let mut nx = 0.0;
        let mut ny = 0.0;
        let mut nz = 0.0;
        let mut den = 0.0;
        let mut add = |i: u32| {
            let i = i as usize;
            let dx = p.x - self.px[i];
            let dy = p.y - self.py[i];
            let dz = p.z - self.pz[i];
            let r2 = dx * dx + dy * dy + dz * dz;
            if r2 < r2cut {
                let w = cubic_kernel(r2.sqrt(), h, sigma);
                let coeff = mass * w / self.rho[i].max(1e-6);
                nx += self.vx[i] * coeff;
                ny += self.vy[i] * coeff;
                nz += self.vz[i] * coeff;
                den += coeff;
            }
        };
        if self.grid.cell_count() > 0 {
            self.grid.for_each_in_neighbourhood(p, &mut add);
        } else {
            for i in 0..self.n {
                add(i as u32);
            }
        }
        if den > 1e-9 {
            Vec3::new(nx / den, ny / den, nz / den)
        } else {
            Vec3::zero()
        }
    }

    pub fn total_mass(&self) -> f64 {
        self.n as f64 * self.mass
    }
    pub fn kinetic_energy(&self) -> f64 {
        let mut e = 0.0;
        for i in 0..self.n {
            let v2 = self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            e += 0.5 * self.mass * v2;
        }
        e
    }
    pub fn max_speed(&self) -> f64 {
        let mut m = 0.0;
        for i in 0..self.n {
            let v2 = self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            let v = v2.sqrt();
            if v > m {
                m = v;
            }
        }
        m
    }
}
