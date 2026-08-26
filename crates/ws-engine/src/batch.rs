//! Batch execution for training — step many worlds in parallel.
//!
//! The training SLA requires `1024 envs × 100 Hz = 102k steps/s` on a single
//! machine. A serial `for wid in ids { world.step() }` cannot reach it.
//! `BatchWorld` steps a slice of `Arc<Mutex<World>>` with `std::thread::scope`,
//! each thread owning one world lock for the whole tick. This is the
//! “Batch” modality of the final form (Embedded remains <0.3ms via PyO3,
//! Batch gives throughput, Service gives convenience).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::world::World;
use ws_domain::config::WorldConfig;

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
/// Returns per-world results in input order.
pub fn batch_step(worlds: &[Arc<Mutex<World>>], ticks_per_world: usize) -> Vec<BatchStepResult> {
    let n = worlds.len();
    let mut results: Vec<Option<BatchStepResult>> = vec![None; n];

    // Collect results in a Vec<Option<...>>. We avoid unsafe pointers by having
    // each scoped thread return its result via a Mutex<Vec>, then merge.
    // This is the safest pattern for std::thread::scope + mutable collection.
    let result_cells: Vec<Mutex<Option<BatchStepResult>>> =
        (0..n).map(|_| Mutex::new(None)).collect();
    let result_cells_ref = &result_cells;

    std::thread::scope(|s| {
        for (i, warc) in worlds.iter().enumerate() {
            let warc = Arc::clone(warc);
            let cell = &result_cells_ref[i];
            s.spawn(move || {
                let mut total_ms = 0.0;
                let mut last = None;
                for _ in 0..ticks_per_world {
                    let t0 = Instant::now();
                    {
                        let mut w = warc.lock().unwrap();
                        w.step_tick();
                        last = Some((w.id.clone(), w.tick, w.time, w.state_hash(), w.rigid.n, w.fluid.n));
                    }
                    total_ms += t0.elapsed().as_secs_f64() * 1000.0;
                }
                if let Some((id, tick, time, hash, bodies, fluid)) = last {
                    *cell.lock().unwrap() = Some(BatchStepResult {
                        id,
                        tick,
                        time,
                        hash: format!("{hash:016x}"),
                        tick_ms: if ticks_per_world > 0 {
                            total_ms / ticks_per_world as f64
                        } else {
                            0.0
                        },
                        bodies,
                        fluid,
                    });
                }
            });
        }
    });

    for (i, cell) in result_cells.into_iter().enumerate() {
        results[i] = cell.into_inner().unwrap();
    }

    results.into_iter().map(|r|
        r.unwrap_or(BatchStepResult {
            id: String::new(), tick: 0, time: 0.0,
            hash: String::new(), tick_ms: 0.0, bodies: 0, fluid: 0,
        })
    ).collect()
}

/// Convenience: create `n` worlds from a template config, each with a unique id.
pub fn create_batch(template: &ws_domain::config::WorldConfig, n: usize, name_prefix: &str) -> Vec<Arc<Mutex<World>>> {
    (0..n)
        .map(|i| {
            let mut cfg = template.clone();
            // Derive per-world seed so parallel envs don't correlate.
            cfg.seed = cfg.seed.wrapping_add((i as u64).wrapping_mul(0x9E3779B97F4A7C15));
            let id = format!("{name_prefix}-{i:04}");
            Arc::new(Mutex::new(World::new(id, cfg, Some(format!("{name_prefix}-{i}")))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ws_domain::config::WorldConfig;

    #[test]
    fn batch_step_is_equivalent_to_serial() {
        let cfg = WorldConfig::default();
        let a = create_batch(&cfg, 4, "a");
        let b = create_batch(&cfg, 4, "b");
        // a via batch
        let r_batch = batch_step(&a, 10);
        // b via serial
        for w in &b {
            for _ in 0..10 {
                w.lock().unwrap().step_tick();
            }
        }
        for (ra, wb) in r_batch.iter().zip(b.iter()) {
            let hb = {
                let w = wb.lock().unwrap();
                format!("{:016x}", w.state_hash())
            };
            assert_eq!(ra.hash, hb);
        }
    }
}
