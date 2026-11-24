use super::shape_stats::{ShapeGrid, ShapeRun};
use crate::fmath;
const CROFTON_WEIGHTS_3D: [f64; 7] = [
    0.04577789120476 * 2.0,
    0.04577789120476 * 2.0,
    0.04577789120476 * 2.0,
    0.03698062787608 * 2.0,
    0.03698062787608 * 2.0,
    0.03698062787608 * 2.0,
    0.03519563978232 * 2.0,
];
const CROFTON_CLASSES_3D: [[f64; 3]; 7] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
];
impl ShapeGrid {
    pub(crate) fn crofton_perimeter(&self, runs: &[ShapeRun], lo: [i64; 3], hi: [i64; 3]) -> f64 {
        let g = self;
        let (ny, nz) = (hi[1] - lo[1] + 1, hi[2] - lo[2] + 1);
        let mut row_start = vec![0usize; (ny * nz) as usize + 1];
        for r in runs {
            row_start[((r.z - lo[2]) * ny + (r.y - lo[1])) as usize + 1] += 1;
        }
        for i in 1..row_start.len() {
            row_start[i] += row_start[i - 1];
        }
        let row = |y: i64, z: i64| -> &[ShapeRun] {
            if y < lo[1] || y > hi[1] || z < lo[2] || z > hi[2] {
                return &[];
            }
            let i = ((z - lo[2]) * ny + (y - lo[1])) as usize;
            &runs[row_start[i]..row_start[i + 1]]
        };
        let mut counts = [0.0f64; 7];
        let dzs: &[i64] = if g.dim == 2 { &[0] } else { &[-1, 0, 1] };
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                let ls = row(y, z);
                if ls.is_empty() {
                    continue;
                }
                let mut length = 0i64;
                for l in ls {
                    length += l.x1 - l.x0 + 1;
                }
                counts[0] += (2 * ls.len()) as f64;
                for &dz in dzs {
                    for dy in -1..=1i64 {
                        if dy == 0 && dz == 0 {
                            continue;
                        }
                        let ns = row(y + dy, z + dz);
                        let straight = length - run_overlap(ls, ns, 0);
                        let diagonal =
                            2 * length - run_overlap(ls, ns, 1) - run_overlap(ls, ns, -1);
                        if dz == 0 {
                            counts[1] += straight as f64;
                            counts[3] += diagonal as f64;
                        } else if dy == 0 {
                            counts[2] += straight as f64;
                            counts[4] += diagonal as f64;
                        } else {
                            counts[5] += straight as f64;
                            counts[6] += diagonal as f64;
                        }
                    }
                }
            }
        }
        let (dx, dy, dz) = (g.space[0], g.space[1], g.space[2]);
        if g.dim == 2 {
            let p = dy * counts[0] / 2.0
                + dx * counts[1] / 2.0
                + dx * dy / fmath::hypot(dx, dy) * counts[3] / 2.0;
            return std::f64::consts::PI / 4.0 * p;
        }
        let vol = dx * dy * dz;
        let mut area = 0.0f64;
        for (i, step) in CROFTON_CLASSES_3D.iter().enumerate() {
            let length = (step[0] * dx * dx + step[1] * dy * dy + step[2] * dz * dz).sqrt();
            area += CROFTON_WEIGHTS_3D[i] * vol / length * counts[i] / 2.0;
        }
        4.0 * area
    }
}
fn run_overlap(ls: &[ShapeRun], ns: &[ShapeRun], shift: i64) -> i64 {
    let mut total = 0i64;
    let (mut i, mut j) = (0usize, 0usize);
    while i < ls.len() && j < ns.len() {
        let (n0, n1) = (ns[j].x0 + shift, ns[j].x1 + shift);
        let (lo, hi) = (ls[i].x0.max(n0), ls[i].x1.min(n1));
        if hi >= lo {
            total += hi - lo + 1;
        }
        if ls[i].x1 < n1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    total
}
