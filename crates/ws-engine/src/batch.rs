//! Batch execution for training — fixed-worker parallel stepping.
//!
//! Batch worlds are explicit-control environments: callers create them with
//! `auto_start=false` and advance them only through [`batch_step`]. A bounded
//! worker set is used rather than one OS thread per environment; this keeps
//! 1024-environment jobs within scheduler and memory limits.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::world::World;

/// Result of one world in a batch step.
#[derive(Debug, Clone)]
pub struct BatchStepResult {
    pub id: String,
    pub tick: u64,
    pub time: f64,
    pub hash: String,
    pub tick_ms: f64,
    pub bodies: usize,
    pub fluid: usize,
}

/// Step many worlds in parallel, each for `ticks_per_world` ticks.
///
/// Work is distributed over at most `available_parallelism()` scoped workers.
/// The output order always matches the input order. Each world is independently
/// locked, so worlds do not share mutable simulation state.
pub fn batch_step(worlds: &[Arc<Mutex<World>>], ticks_per_world: usize) -> Vec<BatchStepResult> {
    if worlds.is_empty() {
        return Vec::new();
    }

    let result_cells: Arc<Vec<Mutex<Option<BatchStepResult>>>> =
        Arc::new((0..worlds.len()).map(|_| Mutex::new(None)).collect());
    let next = Arc::new(AtomicUsize::new(0));
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(worlds.len());

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let cells = Arc::clone(&result_cells);
            let next_index = Arc::clone(&next);
            scope.spawn(move || loop {
                let index = next_index.fetch_add(1, Ordering::Relaxed);
                if index >= worlds.len() {
                    break;
                }
                let world = Arc::clone(&worlds[index]);
                let started = Instant::now();
                {
                    let mut guard = world.lock().unwrap();
                    guard.step_exact(ticks_per_world);
                    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                    let result = BatchStepResult {
                        id: guard.id.clone(),
                        tick: guard.tick,
                        time: guard.time,
                        hash: format!("{:016x}", guard.state_hash()),
                        tick_ms: if ticks_per_world == 0 {
                            0.0
                        } else {
                            elapsed / ticks_per_world as f64
                        },
                        bodies: guard.rigid.n,
                        fluid: guard.fluid.n,
                    };
                    *cells[index].lock().unwrap() = Some(result);
                }
            });
        }
    });

    result_cells
        .iter()
        .map(|cell| {
            cell.lock().unwrap().clone().unwrap_or(BatchStepResult {
                id: String::new(),
                tick: 0,
                time: 0.0,
                hash: String::new(),
                tick_ms: 0.0,
                bodies: 0,
                fluid: 0,
            })
        })
        .collect()
}

/// Convenience for embedded Rust users: create independent, manually-stepped
/// worlds from a template. API callers use the equivalent batch endpoint.
pub fn create_batch(
    template: &ws_domain::config::WorldConfig,
    n: usize,
    name_prefix: &str,
) -> Vec<Arc<Mutex<World>>> {
    (0..n)
        .map(|i| {
            let mut cfg = template.clone();
            cfg.auto_start = false;
            cfg.seed = cfg
                .seed
                .wrapping_add((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let id = format!("{name_prefix}-{i:04}");
            Arc::new(Mutex::new(World::new(
                id,
                cfg,
                Some(format!("{name_prefix}-{i}")),
            )))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ws_domain::config::WorldConfig;

    #[test]
    fn batch_step_is_equivalent_to_serial() {
        let mut cfg = WorldConfig::default();
        cfg.systems.fluid.enabled = false;
        cfg.systems.heat.enabled = false;
        let a = create_batch(&cfg, 4, "a");
        let b = create_batch(&cfg, 4, "b");
        let r_batch = batch_step(&a, 10);
        for w in &b {
            w.lock().unwrap().step_exact(10);
        }
        for (ra, wb) in r_batch.iter().zip(b.iter()) {
            let hb = format!("{:016x}", wb.lock().unwrap().state_hash());
            assert_eq!(ra.hash, hb);
        }
    }

    #[test]
    fn zero_tick_returns_current_state() {
        let cfg = WorldConfig::default();
        let worlds = create_batch(&cfg, 2, "zero");
        let results = batch_step(&worlds, 0);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].tick, 0);
    }
}
