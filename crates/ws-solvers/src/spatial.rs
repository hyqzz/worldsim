//! Uniform hashed grid for neighbour search.
//!
//! Two flavours share this module:
//!
//! - Fluid neighbour search (cell = 2h, support 2h, 27 cells per query).
//! - Rigid broad-phase (cell = 2·max_radius, 27 cells per query).
//!
//! The grid is rebuilt every sub-step from SoA position arrays.  A counting
//! sort over cell indices keeps memory local and iteration order deterministic
//! (sorted by cell key, then by particle index).

use ws_core::math::Vec3;

/// A bounded uniform grid built for a single rebuild cycle.
#[derive(Debug, Default)]
pub struct UniformGrid {
    cell: f64,
    inv_cell: f64,
    min: Vec3,
    dims: [i32; 3],
    /// Prefix sums: start[i] inclusive for cell i, length = cell_count+1.
    start: Vec<u32>,
    /// Sorted particle indices.
    items: Vec<u32>,
}

impl UniformGrid {
    /// Build from position SoAs. `cell` must be positive.  Positions are
    /// assumed to lie inside or near `bounds` (outliers clamped to the edge
    /// cell — they are still findable).
    pub fn rebuild(
        &mut self,
        min: Vec3,
        max: Vec3,
        cell: f64,
        n: usize,
        px: &[f64],
        py: &[f64],
        pz: &[f64],
    ) {
        assert!(cell > 0.0);
        assert!(px.len() >= n && py.len() >= n && pz.len() >= n);
        if !(max.x > min.x && max.y > min.y && max.z > min.z) {
            *self = UniformGrid::default();
            return;
        }
        self.cell = cell;
        self.inv_cell = 1.0 / cell;
        self.min = min;
        let raw = [
            ((max.x - min.x) * self.inv_cell).ceil() as i32,
            ((max.y - min.y) * self.inv_cell).ceil() as i32,
            ((max.z - min.z) * self.inv_cell).ceil() as i32,
        ];
        // Guard against insane bounds (malicious config).
        let cap_cells = 4_000_000usize;
        let mut dims = [raw[0].max(1), raw[1].max(1), raw[2].max(1)];
        let mut cells = dims[0] as usize * dims[1] as usize * dims[2] as usize;
        if cells > cap_cells {
            let scale = (cap_cells as f64 / cells as f64).powf(1.0 / 3.0);
            dims[0] = ((dims[0] as f64 * scale).floor() as i32).max(1);
            dims[1] = ((dims[1] as f64 * scale).floor() as i32).max(1);
            dims[2] = ((dims[2] as f64 * scale).floor() as i32).max(1);
            cells = dims[0] as usize * dims[1] as usize * dims[2] as usize;
            while cells > cap_cells && (dims[0] > 1 || dims[1] > 1 || dims[2] > 1) {
                if dims[0] > 1 { dims[0] -= 1; }
                else if dims[1] > 1 { dims[1] -= 1; }
                else if dims[2] > 1 { dims[2] -= 1; }
                cells = dims[0] as usize * dims[1] as usize * dims[2] as usize;
            }
        }
        self.dims = dims;
        let nx = self.dims[0] as usize;
        let ny = self.dims[1] as usize;
        let nz = self.dims[2] as usize;
        let cell_count = nx * ny * nz;
        self.start = vec![0u32; cell_count + 1];
        if n == 0 || cell_count == 0 {
            self.items.clear();
            for v in &mut self.start {
                *v = 0;
            }
            return;
        }

        // Pass 1: counts
        let mut cnt = vec![0u32; cell_count];
        for i in 0..n {
            let idx = cell_index_of(px[i], py[i], pz[i], min, nx, ny, self.inv_cell, &dims);
            cnt[idx] += 1;
        }
        // Pass 2: prefix sums → start
        let mut acc = 0u32;
        for i in 0..cell_count {
            self.start[i] = acc;
            acc += cnt[i];
        }
        self.start[cell_count] = acc;
        assert_eq!(acc as usize, n);

        // Pass 3: scatter
        self.items.resize(n, 0);
        let mut cursor = self.start[..cell_count].to_vec();
        for i in 0..n {
            let idx = cell_index_of(px[i], py[i], pz[i], min, nx, ny, self.inv_cell, &dims);
            let pos = cursor[idx] as usize;
            self.items[pos] = i as u32;
            cursor[idx] += 1;
        }
    }

    /// Iterate every particle index stored in the cell containing `p`.
    pub fn for_each_in_cell<F: FnMut(u32)>(&self, p: Vec3, mut f: F) {
        let Some(base) = self.coord_of(p) else { return };
        let idx = self.linear_idx(base[0], base[1], base[2]);
        for k in self.start[idx] as usize..self.start[idx + 1] as usize {
            f(self.items[k]);
        }
    }

    /// Iterate every particle index stored in the 27 neighbours (including the
    /// centre cell) of `p`.  This is the query for rigid broad-phase and for
    /// fluid when cell = support radius.
    pub fn for_each_in_neighbourhood<F: FnMut(u32)>(&self, p: Vec3, mut f: F) {
        let Some([cx, cy, cz]) = self.coord_of(p) else { return };
        for dz in -1..=1 {
            let nz = cz + dz;
            if !(0..self.dims[2]).contains(&nz) { continue; }
            for dy in -1..=1 {
                let ny = cy + dy;
                if !(0..self.dims[1]).contains(&ny) { continue; }
                for dx in -1..=1 {
                    let nx = cx + dx;
                    if !(0..self.dims[0]).contains(&nx) { continue; }
                    let idx = self.linear_idx(nx, ny, nz);
                    for k in self.start[idx] as usize..self.start[idx + 1] as usize {
                        f(self.items[k]);
                    }
                }
            }
        }
    }

    /// Collect neighbourhood into `out`.
    pub fn collect_neighbourhood(&self, p: Vec3, out: &mut Vec<u32>) {
        out.clear();
        self.for_each_in_neighbourhood(p, |id| out.push(id));
    }

    /// Iterate every unordered pair of particle indices that can collide
    /// (same or neighbouring cells, each pair once).  Callback receives
    /// `(a, b)` with `a < b` not guaranteed — callers must not assume ordering,
    /// but every pair is visited at most once.
    ///
    /// This is used by the rigid solver's collision loop.  Complexity
    /// O(n × avg_neighbours) and fully deterministic (cell-major order).
    pub fn for_each_candidate_pair<F>(&self, mut f: F)
    where
        F: FnMut(u32, u32),
    {
        let nx = self.dims[0];
        let ny = self.dims[1];
        let nz = self.dims[2];
        let lin = |x, y, z| (x + y * nx + z * nx * ny) as usize;
        for cz in 0..nz {
            for cy in 0..ny {
                for cx in 0..nx {
                    let center = lin(cx, cy, cz);
                    let a0 = self.start[center] as usize;
                    let a1 = self.start[center + 1] as usize;
                    // Pairs within the same cell
                    for ia in a0..a1 {
                        let a = self.items[ia];
                        for ib in (ia + 1)..a1 {
                            f(a, self.items[ib]);
                        }
                    }
                    // Pairs with neighbours of strictly larger linear index
                    // (lexicographically after centre) — avoids duplicates.
                    for dz in -1..=1 {
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                if dx == 0 && dy == 0 && dz == 0 { continue; }
                                let nx2 = cx + dx;
                                let ny2 = cy + dy;
                                let nz2 = cz + dz;
                                if !(0..self.dims[0]).contains(&nx2) { continue; }
                                if !(0..self.dims[1]).contains(&ny2) { continue; }
                                if !(0..self.dims[2]).contains(&nz2) { continue; }
                                let neigh = lin(nx2, ny2, nz2);
                                if neigh <= center { continue; }
                                let b0 = self.start[neigh] as usize;
                                let b1 = self.start[neigh + 1] as usize;
                                for ia in a0..a1 {
                                    for ib in b0..b1 {
                                        f(self.items[ia], self.items[ib]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- internal helpers ----

    fn coord_of(&self, p: Vec3) -> Option<[i32; 3]> {
        let x = ((p.x - self.min.x) * self.inv_cell).floor() as i32;
        let y = ((p.y - self.min.y) * self.inv_cell).floor() as i32;
        let z = ((p.z - self.min.z) * self.inv_cell).floor() as i32;
        if !(0..self.dims[0]).contains(&x)
            || !(0..self.dims[1]).contains(&y)
            || !(0..self.dims[2]).contains(&z)
        {
            // Clamp outlier to nearest edge cell rather than dropping.
            let cx = x.clamp(0, self.dims[0] - 1);
            let cy = y.clamp(0, self.dims[1] - 1);
            let cz = z.clamp(0, self.dims[2] - 1);
            return Some([cx, cy, cz]);
        }
        Some([x, y, z])
    }

    fn linear_idx(&self, x: i32, y: i32, z: i32) -> usize {
        (x + y * self.dims[0] + z * self.dims[0] * self.dims[1]) as usize
    }

    pub fn dims(&self) -> [i32; 3] { self.dims }
    pub fn cell_count(&self) -> usize { self.dims[0] as usize * self.dims[1] as usize * self.dims[2] as usize }
}

#[inline]
fn cell_index_of(
    x: f64, y: f64, z: f64,
    min: Vec3, nx: usize, _ny: usize, inv_cell: f64, dims: &[i32;3],
) -> usize {
    let ix = (((x - min.x) * inv_cell).floor() as i32).clamp(0, dims[0]-1) as usize;
    let iy = (((y - min.y) * inv_cell).floor() as i32).clamp(0, dims[1]-1) as usize;
    let iz = (((z - min.z) * inv_cell).floor() as i32).clamp(0, dims[2]-1) as usize;
    ix + iy*nx + iz*nx*dims[1] as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_single_cell_and_neighbourhood() {
        let mut g = UniformGrid::default();
        let min = Vec3::new(0.0,0.0,0.0);
        let max = Vec3::new(1.0,1.0,1.0);
        let px=[0.1,0.8,0.5]; let py=[0.1,0.8,0.5]; let pz=[0.1,0.8,0.5];
        g.rebuild(min,max,0.5,3,&px,&py,&pz);
        // 2×2×2 = 8 cells expected.
        assert_eq!(g.cell_count(), 8);
        let mut collected=Vec::new();
        g.collect_neighbourhood(Vec3::new(0.1,0.1,0.1), &mut collected);
        // With 27-cell neighbourhood covering the whole domain for these small dims, all particles reachable.
        assert!(collected.contains(&0));
        // Candidate pairs enumerates without duplicates.
        let mut pairs=Vec::new();
        g.for_each_candidate_pair(|a,b| pairs.push((a.min(b),a.max(b))));
        if pairs.len()>0 {
            let mut dedup=pairs.clone();
            dedup.sort_unstable(); dedup.dedup();
            assert_eq!(dedup.len(), pairs.len());
        }
    }
}
