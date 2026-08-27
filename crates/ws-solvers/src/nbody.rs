//! Direct-sum softened Newtonian gravitation (leapfrog KDK).
//!
//! Treats each body as a point mass; forces are softened by Plummer length ε
//! to avoid singular close encounters. Integration is the symplectic leapfrog
//! (velocity-Verlet / KDK) which conserves energy well for gravitational orbits
//! and is time-reversible.
//!
//! This sub-system operates at container scale (planetary scenes) and runs
//! independently from the short-range WCSPH/rigid collision physics.  Its
//! parameters come from `ws_domain::config::NBodyCfg` and the global gravity
//! constant `G` (or an overridden per-scene gravitational parameter).

use ws_core::constants::G;

#[derive(Debug)]
pub struct NBodySystem {
    pub n: usize,
    pub cap: usize,
    pub px: Vec<f64>,
    pub py: Vec<f64>,
    pub pz: Vec<f64>,
    pub vx: Vec<f64>,
    pub vy: Vec<f64>,
    pub vz: Vec<f64>,
    pub ax: Vec<f64>,
    pub ay: Vec<f64>,
    pub az: Vec<f64>,
    pub m: Vec<f64>,
    pub g: f64,
    pub softening: f64,
    pub softening2: f64,
}

impl NBodySystem {
    pub fn new(cap: usize, softening: f64, g: f64) -> Self {
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
            m: vec![0.0; cap],
            g,
            softening: softening.max(1e-6),
            softening2: softening.max(1e-6).powi(2),
        }
    }

    pub fn configure(&mut self, softening: f64, g: f64) {
        self.softening = softening.max(1e-6);
        self.softening2 = self.softening.powi(2);
        self.g = g;
    }

    pub fn add_body(
        &mut self,
        mass: f64,
        pos: ws_core::math::Vec3,
        vel: ws_core::math::Vec3,
    ) -> Result<u32, String> {
        if self.n >= self.cap {
            return Err("n-body capacity reached".into());
        }
        let i = self.n;
        self.px[i] = pos.x;
        self.py[i] = pos.y;
        self.pz[i] = pos.z;
        self.vx[i] = vel.x;
        self.vy[i] = vel.y;
        self.vz[i] = vel.z;
        self.m[i] = mass.max(1e-12);
        self.n += 1;
        // Initial accelerations must reflect the new configuration.
        self.compute_acc();
        Ok(i as u32)
    }

    pub fn remove(&mut self, idx: u32) -> Result<(), String> {
        let i = idx as usize;
        if i >= self.n {
            return Err(format!("n-body {idx} out of range"));
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
                &mut self.ax,
                &mut self.ay,
                &mut self.az,
                &mut self.m,
            ] {
                arr[i] = arr[last];
            }
        }
        self.n -= 1;
        self.compute_acc();
        Ok(())
    }

    pub fn total_energy(&self) -> f64 {
        let mut kin = 0.0;
        for i in 0..self.n {
            let v2 = self.vx[i] * self.vx[i] + self.vy[i] * self.vy[i] + self.vz[i] * self.vz[i];
            kin += 0.5 * self.m[i] * v2;
        }
        let mut pot = 0.0;
        for i in 0..self.n {
            for j in (i + 1)..self.n {
                let dx = self.px[j] - self.px[i];
                let dy = self.py[j] - self.py[i];
                let dz = self.pz[j] - self.pz[i];
                let r = (dx * dx + dy * dy + dz * dz + self.softening2).sqrt();
                pot -= self.g * self.m[i] * self.m[j] / r;
            }
        }
        kin + pot
    }

    pub fn orbital_period_two_body(&self, i: usize, j: usize) -> Option<f64> {
        if self.n < 2 || i >= self.n || j >= self.n {
            return None;
        }
        let dx = self.px[j] - self.px[i];
        let dy = self.py[j] - self.py[i];
        let dz = self.pz[j] - self.pz[i];
        let r = (dx * dx + dy * dy + dz * dz).sqrt();
        let mu = self.g * (self.m[i] + self.m[j]);
        if r == 0.0 || mu == 0.0 {
            return None;
        }
        Some(2.0 * std::f64::consts::PI * (r.powi(3) / mu).sqrt())
    }

    // ---- integration ----

    /// Recompute accelerations after direct state restoration or external edits.
    pub fn recompute_accelerations(&mut self) {
        self.compute_acc();
    }

    fn compute_acc(&mut self) {
        let n = self.n;
        for i in 0..n {
            self.ax[i] = 0.0;
            self.ay[i] = 0.0;
            self.az[i] = 0.0;
        }
        let g = self.g;
        let eps2 = self.softening2;
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.px[j] - self.px[i];
                let dy = self.py[j] - self.py[i];
                let dz = self.pz[j] - self.pz[i];
                let r2 = dx * dx + dy * dy + dz * dz + eps2;
                let r = r2.sqrt();
                let inv_r3 = 1.0 / (r * r2); // 1/r^3
                let f = g * inv_r3;
                let ax = f * dx;
                let ay = f * dy;
                let az = f * dz;
                self.ax[i] += ax * self.m[j];
                self.ay[i] += ay * self.m[j];
                self.az[i] += az * self.m[j];
                self.ax[j] -= ax * self.m[i];
                self.ay[j] -= ay * self.m[i];
                self.az[j] -= az * self.m[i];
            }
        }
    }

    /// One leapfrog KDK sub-step of size `dt`.
    pub fn step(&mut self, dt: f64) {
        if self.n == 0 {
            return;
        }
        let n = self.n;
        // K1
        for i in 0..n {
            self.vx[i] += self.ax[i] * dt * 0.5;
            self.vy[i] += self.ay[i] * dt * 0.5;
            self.vz[i] += self.az[i] * dt * 0.5;
        }
        // D
        for i in 0..n {
            self.px[i] += self.vx[i] * dt;
            self.py[i] += self.vy[i] * dt;
            self.pz[i] += self.vz[i] * dt;
        }
        self.compute_acc();
        // K2
        for i in 0..n {
            self.vx[i] += self.ax[i] * dt * 0.5;
            self.vy[i] += self.ay[i] * dt * 0.5;
            self.vz[i] += self.az[i] * dt * 0.5;
        }
    }
}

#[allow(dead_code)]
pub fn earth_moon_example() -> (f64, f64) {
    // Convenience constants for docs/tests: G masses folded.
    (G * 5.972e24, G * 7.342e22)
}
