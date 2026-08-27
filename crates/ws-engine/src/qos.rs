//! Quality-of-service control — the precision budget that implements the
//! platform promise *"arbitrary accuracy as compute allows"*.
//!
//! The v0.1 controller operates at **world granularity** (a single quality
//! level per world).  Future work (see `docs/roadmap.md`) will split this into
//! per-region octree attention maps.  The public API is forward-compatible:
//! the precision endpoints already expose per-region hints, but the v0.1 kernel
//! treats them as global.
//!
//! The controller is a PID-free hysteresis machine: it measures wall-clock
//! time per tick, maintains an EMA, and steps the world up or down after
//! sustained over/under-budget intervals.  This avoids oscillation while still
//! reacting to scene changes within a few seconds.

use ws_domain::config::Quality;

/// Concrete limits for a quality level.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Upper bound on the number of fluid sub-steps per macro tick.
    pub max_fluid_substeps: usize,
    /// Cap on live fluid particles (older excess particles are kept but new
    /// blobs are truncated).
    pub max_fluid_particles: usize,
    /// Downsampling stride for the streaming wire frame (1 = every particle).
    /// Larger means less bytes per tick at the cost of viewer fidelity only.
    pub stream_stride: usize,
    /// Target streaming frames per second for this quality (informational).
    pub stream_fps: u32,
}

/// Mapping from [`Quality`] to [`Limits`].
pub fn limits_for(q: Quality) -> Limits {
    // Tuned so that a 2 GHz single core can sustain ~30 Hz ticks at `medium`
    // with a full tank, while `high` requires a modern desktop CPU.
    match q {
        Quality::Low => Limits {
            max_fluid_substeps: 4,
            max_fluid_particles: 4_000,
            stream_stride: 4,
            stream_fps: 24,
        },
        Quality::Medium => Limits {
            max_fluid_substeps: 10,
            max_fluid_particles: 16_000,
            stream_stride: 2,
            stream_fps: 30,
        },
        Quality::High => Limits {
            max_fluid_substeps: 24,
            max_fluid_particles: 48_000,
            stream_stride: 1,
            stream_fps: 60,
        },
    }
}

/// Hysteresis-based auto controller used by the API scheduler.
///
/// The API layer measures real wall-clock time per tick and calls
/// [`QosController::record`] after each tick.  The controller reports the
/// *effective* level the world should run at via [`QosController::effective_quality`].
/// User-locked quality (via `PUT /precision`) bypasses auto decisions.
#[derive(Debug, Clone)]
pub struct QosController {
    /// User-requested level (if locked) or auto level.
    pub requested: Quality,
    /// `true` when the client locked precision explicitly.
    pub locked: bool,
    /// Effective level currently enforced (may differ from `requested` when auto).
    pub effective: Quality,

    /// Per-world compute budget in milliseconds per tick (default = one macro dt budget).
    pub budget_ms: f64,

    // Auto bookkeeping
    over_streak: u32,
    under_streak: u32,
    ema_ms: f64,
}

impl QosController {
    pub fn new(quality: Quality, budget_ms: f64) -> Self {
        Self {
            requested: quality,
            locked: false,
            effective: quality,
            budget_ms: budget_ms.clamp(1.0, 500.0),
            over_streak: 0,
            under_streak: 0,
            ema_ms: budget_ms,
        }
    }

    pub fn lock(&mut self, q: Quality) {
        self.requested = q;
        self.locked = true;
        self.effective = q;
        self.over_streak = 0;
        self.under_streak = 0;
    }

    pub fn unlock(&mut self) {
        self.locked = false;
        self.effective = self.requested;
        self.over_streak = 0;
        self.under_streak = 0;
    }

    pub fn set_budget(&mut self, ms: f64) {
        self.budget_ms = ms.clamp(1.0, 500.0);
    }

    pub fn limits(&self) -> Limits {
        limits_for(self.effective)
    }

    /// Restore the runtime QoS state captured in a snapshot. Streaks are
    /// intentionally reset; they affect future adaptation but not the current
    /// physical tick, while effective level/lock/budget do.
    pub fn restore_runtime(&mut self, effective: Quality, locked: bool, budget_ms: f64) {
        self.effective = effective;
        self.requested = effective;
        self.locked = locked;
        self.budget_ms = budget_ms.clamp(1.0, 500.0);
        self.over_streak = 0;
        self.under_streak = 0;
        self.ema_ms = self.budget_ms;
    }

    /// Record a wall-clock measurement and maybe step the level.
    ///
    /// Thresholds:
    /// - Over budget for 60 consecutive ticks → demote one level (High→Mid→Low, stays at Low).
    /// - Well under budget (≤ 60% of budget) for 600 ticks → promote one level.
    pub fn record(&mut self, measured_ms: f64) {
        if self.locked {
            return;
        }
        // EMA α=0.08 for smoother reporting (the streak decides promotions, the EMA just for stats).
        self.ema_ms = self.ema_ms * 0.92 + measured_ms * 0.08;

        if measured_ms > self.budget_ms {
            self.over_streak += 1;
            self.under_streak = 0;
            if self.over_streak >= 60 {
                self.effective = match self.effective {
                    Quality::High => Quality::Medium,
                    Quality::Medium => Quality::Low,
                    Quality::Low => Quality::Low,
                };
                self.over_streak = 0;
                // Stay demoted for at least 300 ticks before reconsidering promotion.
                self.under_streak = 0;
            }
        } else if measured_ms < self.budget_ms * 0.60 {
            self.under_streak += 1;
            self.over_streak = 0;
            if self.under_streak >= 600 {
                self.effective = match self.effective {
                    Quality::Low => Quality::Medium,
                    Quality::Medium => Quality::High,
                    Quality::High => Quality::High,
                };
                self.under_streak = 0;
            }
        } else {
            // Within budget but not comfortably under — decay counters slowly instead of resetting.
            if self.over_streak > 0 {
                self.over_streak = self.over_streak.saturating_sub(1);
            }
            if self.under_streak > 0 {
                self.under_streak = self.under_streak.saturating_sub(1);
            }
        }
    }

    pub fn ema(&self) -> f64 {
        self.ema_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downgrades_when_over_budget() {
        let mut c = QosController::new(Quality::High, 16.0);
        for _ in 0..70 {
            c.record(30.0);
        }
        assert_eq!(c.effective, Quality::Medium);
        for _ in 0..70 {
            c.record(30.0);
        }
        assert_eq!(c.effective, Quality::Low);
        // Stays at Low.
        for _ in 0..70 {
            c.record(30.0);
        }
        assert_eq!(c.effective, Quality::Low);
    }
    #[test]
    fn upgrades_when_well_under() {
        let mut c = QosController::new(Quality::Low, 16.0);
        for _ in 0..650 {
            c.record(5.0);
        }
        assert_eq!(c.effective, Quality::Medium);
        for _ in 0..650 {
            c.record(5.0);
        }
        assert_eq!(c.effective, Quality::High);
    }
    #[test]
    fn locked_never_moves() {
        let mut c = QosController::new(Quality::High, 16.0);
        c.lock(Quality::High);
        for _ in 0..500 {
            c.record(999.0);
        }
        assert_eq!(c.effective, Quality::High);
        c.unlock();
        for _ in 0..70 {
            c.record(999.0);
        }
        assert_eq!(c.effective, Quality::Medium);
    }
    #[test]
    fn limits_match_design_doc() {
        assert_eq!(limits_for(Quality::Low).max_fluid_substeps, 4);
        assert_eq!(limits_for(Quality::Medium).max_fluid_particles, 16_000);
        assert_eq!(limits_for(Quality::High).stream_stride, 1);
    }
}
