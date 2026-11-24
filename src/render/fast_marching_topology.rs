#![forbid(unsafe_code)]
use super::cpr_path_assistant::{bounded_cpr_assistant_product, positive_finite};
use super::fast_marching::{
    FastMarchingError, FastMarchingErrorKind, FastMarchingGrid, FastMarchingOptions,
    FastMarchingResult, FastMarchingSeed, MAX_FAST_MARCHING_NODES, ctx_check, fast_marching,
    fast_marching_controlled,
};
use super::parallel::Cancellation;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct FastMarchingTopologyMode(pub u8);
impl FastMarchingTopologyMode {
    pub const NONE: FastMarchingTopologyMode = FastMarchingTopologyMode(0);
    pub const STRICT: FastMarchingTopologyMode = FastMarchingTopologyMode(1);
    pub const NO_HANDLES: FastMarchingTopologyMode = FastMarchingTopologyMode(2);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FastMarchingTopologyOptions {
    pub mode: FastMarchingTopologyMode,
    pub max_bytes: u64,
    pub max_work: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FastMarchingTopologyReport {
    pub mode: FastMarchingTopologyMode,
    pub dimension: i64,
    pub initial_components: i64,
    pub final_components: i64,
    pub rejected_candidates: i64,
    pub rejected_merge: i64,
    pub rejected_handle: i64,
    pub rejected_background: i64,
    pub available_unreached: i64,
    pub unavailable: i64,
    pub frozen: i64,
    pub logical_bytes: u64,
    pub work: u64,
    pub stop_reason: String,
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct FastMarchingTopologyResult {
    pub result: FastMarchingResult,
    pub report: FastMarchingTopologyReport,
}
fn topology_limit(detail: impl AsRef<str>) -> FastMarchingError {
    FastMarchingError::new(
        FastMarchingErrorKind::TopologyLimit,
        format!(
            "render: topology fast marching resource limit: {}",
            detail.as_ref()
        ),
    )
}
pub fn fast_marching_constrained(
    ctx: Option<&dyn Cancellation>,
    grid: &FastMarchingGrid,
    seeds: &[FastMarchingSeed],
    options: &FastMarchingOptions,
    topology: FastMarchingTopologyOptions,
) -> Result<FastMarchingTopologyResult, FastMarchingError> {
    if topology.mode > FastMarchingTopologyMode::NO_HANDLES {
        return Err(FastMarchingError::input("topology mode"));
    }
    if topology.mode == FastMarchingTopologyMode::NONE {
        let result = fast_marching(ctx, grid, seeds, options)?;
        return Ok(FastMarchingTopologyResult {
            result,
            report: FastMarchingTopologyReport {
                mode: topology.mode,
                stop_reason: "disabled".into(),
                ..Default::default()
            },
        });
    }
    let mut c = match FmTopology::new(ctx, grid, seeds, options, topology) {
        Ok(c) => c,
        Err((e, own_report)) => {
            let report = own_report.unwrap_or_else(|| FastMarchingTopologyReport {
                mode: topology.mode,
                dimension: if grid.nz == 1 {
                    2
                } else if grid.nz > 1 {
                    3
                } else {
                    0
                },
                ..Default::default()
            });
            return Err(fm_topology_failure(e, report));
        }
    };
    if let Err(e) = c.charge(grid.speed.len() as u64 * 2) {
        return Err(fm_topology_failure(e, c.report));
    }
    let r = match fast_marching_controlled(ctx, grid, seeds, options, true, Some(&mut c)) {
        Ok(r) => r,
        Err(e) => return Err(fm_topology_failure(e, c.report)),
    };
    c.report.frozen = r.frozen;
    for (i, t) in r.times.iter().enumerate() {
        if let Err(e) = c.charge(1) {
            return Err(fm_topology_failure(e, c.report));
        }
        if positive_finite(grid.speed[i]) {
            if t.is_infinite() && *t > 0.0 {
                c.report.available_unreached += 1;
            }
        } else {
            c.report.unavailable += 1;
        }
    }
    if c.report.stop_reason.is_empty() {
        c.report.stop_reason = if c.report.rejected_candidates > 0 {
            "topology-rejections".into()
        } else {
            "front-exhausted".into()
        };
    }
    c.report.complete = c.report.available_unreached == 0;
    if let Err(e) = ctx_check(ctx) {
        return Err(fm_topology_failure(e, c.report));
    }
    Ok(FastMarchingTopologyResult {
        result: r,
        report: c.report,
    })
}
fn fm_topology_failure(
    cause: FastMarchingError,
    mut report: FastMarchingTopologyReport,
) -> FastMarchingError {
    report.complete = false;
    report.stop_reason = match cause.kind() {
        FastMarchingErrorKind::TopologyLimit | FastMarchingErrorKind::Limit => "budget",
        FastMarchingErrorKind::DeadlineExceeded => "deadline",
        FastMarchingErrorKind::Canceled => "canceled",
        FastMarchingErrorKind::Input => "invalid-input",
    }
    .into();
    cause.with_topology(report)
}
pub(crate) struct FmTopology<'a> {
    ctx: Option<&'a dyn Cancellation>,
    nx: i64,
    ny: i64,
    nz: i64,
    dim: i64,
    parent: Vec<i32>,
    rank: Vec<u8>,
    limit: u64,
    pub(crate) report: FastMarchingTopologyReport,
}
type NewTopologyError = (FastMarchingError, Option<FastMarchingTopologyReport>);
impl<'a> FmTopology<'a> {
    #[allow(clippy::result_large_err)]
    fn new(
        ctx: Option<&'a dyn Cancellation>,
        g: &FastMarchingGrid,
        seeds: &[FastMarchingSeed],
        o: &FastMarchingOptions,
        t: FastMarchingTopologyOptions,
    ) -> Result<FmTopology<'a>, NewTopologyError> {
        let plain = |e: FastMarchingError| (e, None);
        let (n, ok) = bounded_cpr_assistant_product(g.nx, g.ny, g.nz, MAX_FAST_MARCHING_NODES);
        if g.nx < 2
            || g.ny < 2
            || g.nz < 1
            || !ok
            || n as usize != g.speed.len()
            || !positive_finite(g.spacing.x)
            || !positive_finite(g.spacing.y)
            || !positive_finite(g.spacing.z)
            || seeds.is_empty()
        {
            return Err(plain(FastMarchingError::input(
                "constrained 2D/3D geometry/domain",
            )));
        }
        if o.stopping_value < 0.0
            || o.stopping_value.is_nan()
            || o.max_frozen < 0
            || o.max_trial < 0
        {
            return Err(plain(FastMarchingError::input("invalid stopping criteria")));
        }
        let max_bytes = if t.max_bytes == 0 {
            512 << 20
        } else {
            t.max_bytes
        };
        let max_work = if t.max_work == 0 {
            1_000_000_000
        } else {
            t.max_work
        };
        let n = n as u64;
        let mut bytes = n * 30 + 8192;
        for (count, each) in [(seeds.len() as u64, 64u64), (o.targets.len() as u64, 32)] {
            if count > (u64::MAX - bytes) / each {
                return Err(plain(topology_limit("bytes overflow")));
            }
            bytes += count * each;
        }
        if bytes > max_bytes {
            return Err((
                topology_limit(format!("{bytes} bytes > {max_bytes}")),
                Some(FastMarchingTopologyReport {
                    mode: t.mode,
                    logical_bytes: bytes,
                    stop_reason: "budget".into(),
                    ..Default::default()
                }),
            ));
        }
        let targets = o.targets.len() as u64;
        if n * 3 > max_work
            || targets > (max_work - n * 3) / 2
            || seeds.len() as u64 > (max_work - n * 3 - 2 * targets) / 8192
        {
            return Err((
                topology_limit("initial work"),
                Some(FastMarchingTopologyReport {
                    mode: t.mode,
                    dimension: if g.nz == 1 { 2 } else { 3 },
                    logical_bytes: bytes,
                    stop_reason: "budget".into(),
                    ..Default::default()
                }),
            ));
        }
        ctx_check(ctx).map_err(plain)?;
        for (i, target) in o.targets.iter().enumerate() {
            if i & 1023 == 0 {
                ctx_check(ctx).map_err(plain)?;
            }
            if target[0] < 0
                || target[0] >= g.nx
                || target[1] < 0
                || target[1] >= g.ny
                || target[2] < 0
                || target[2] >= g.nz
            {
                return Err(plain(FastMarchingError::input("target")));
            }
        }
        let first = seeds[0].value;
        let node =
            |s: &FastMarchingSeed| ((s.index[2] * g.ny + s.index[1]) * g.nx + s.index[0]) as usize;
        for (i, s) in seeds.iter().enumerate() {
            if i & 1023 == 0 {
                ctx_check(ctx).map_err(plain)?;
            }
            if s.index[0] < 0
                || s.index[0] >= g.nx
                || s.index[1] < 0
                || s.index[1] >= g.ny
                || s.index[2] < 0
                || s.index[2] >= g.nz
                || s.value.is_nan()
                || s.value.is_infinite()
                || s.value != first
                || (o.stopping_value > 0.0 && s.value > o.stopping_value)
            {
                return Err(plain(FastMarchingError::input(
                    "incompatible initial topology seed",
                )));
            }
            if !positive_finite(g.speed[node(s)]) {
                return Err(plain(FastMarchingError::input(
                    "seed on unavailable mask node",
                )));
            }
        }
        let mut c = FmTopology {
            ctx,
            nx: g.nx,
            ny: g.ny,
            nz: g.nz,
            dim: if g.nz == 1 { 2 } else { 3 },
            parent: vec![0; n as usize],
            rank: vec![0; n as usize],
            limit: max_work,
            report: FastMarchingTopologyReport {
                mode: t.mode,
                logical_bytes: bytes,
                work: targets,
                ..Default::default()
            },
        };
        c.report.dimension = c.dim;
        for i in 0..c.parent.len() {
            if let Err(e) = c.charge(1) {
                return Err((e, Some(c.report)));
            }
            c.parent[i] = -1;
        }
        for s in seeds {
            let i = node(s);
            if c.parent[i] < 0 {
                c.parent[i] = i as i32;
                c.report.final_components += 1;
            }
        }
        for s in seeds {
            if let Err(e) = c.charge(8192) {
                return Err((e, Some(c.report)));
            }
            c.join_neighbors(node(s));
        }
        c.report.initial_components = c.report.final_components;
        Ok(c)
    }
    pub(crate) fn charge(&mut self, n: u64) -> Result<(), FastMarchingError> {
        ctx_check(self.ctx)?;
        if n > self.limit - self.report.work {
            return Err(topology_limit(format!("work > {}", self.limit)));
        }
        self.report.work += n;
        Ok(())
    }
    fn at(&self, x: i64, y: i64, z: i64) -> i64 {
        if x < 0 || x >= self.nx || y < 0 || y >= self.ny || z < 0 || z >= self.nz {
            return -1;
        }
        (z * self.ny + y) * self.nx + x
    }
    fn coords(&self, i: usize) -> (i64, i64, i64) {
        let i = i as i64;
        (
            i % self.nx,
            (i / self.nx) % self.ny,
            i / (self.nx * self.ny),
        )
    }
    fn occupied(&self, x: i64, y: i64, z: i64) -> bool {
        let i = self.at(x, y, z);
        i >= 0 && self.parent[i as usize] >= 0
    }
    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.parent[root] != root as i32 {
            root = self.parent[root] as usize;
        }
        let mut i = i;
        while i != root {
            let next = self.parent[i] as usize;
            self.parent[i] = root as i32;
            i = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let mut a = self.find(a);
        let mut b = self.find(b);
        if a == b {
            return;
        }
        if self.rank[a] < self.rank[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a as i32;
        if self.rank[a] == self.rank[b] {
            self.rank[a] = self.rank[a].wrapping_add(1);
        }
        self.report.final_components -= 1;
    }
    fn z_range(&self) -> (i64, i64) {
        if self.dim == 2 { (0, 0) } else { (-1, 1) }
    }
    fn join_neighbors(&mut self, i: usize) {
        let (x, y, z) = self.coords(i);
        let (lo, hi) = self.z_range();
        for dz in lo..=hi {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let j = self.at(x + dx, y + dy, z + dz);
                    if j >= 0 && self.parent[j as usize] >= 0 {
                        self.union(i, j as usize);
                    }
                }
            }
        }
    }
    pub(crate) fn admit(&mut self, i: usize) -> Result<bool, FastMarchingError> {
        self.charge(8192)?;
        if self.parent[i] >= 0 {
            return Ok(true);
        }
        let (x, y, z) = self.coords(i);
        let mut roots = [0usize; 26];
        let mut nr = 0usize;
        let (lo, hi) = self.z_range();
        for dz in lo..=hi {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let j = self.at(x + dx, y + dy, z + dz);
                    if j < 0 || self.parent[j as usize] < 0 {
                        continue;
                    }
                    let r = self.find(j as usize);
                    if !roots[..nr].contains(&r) {
                        roots[nr] = r;
                        nr += 1;
                    }
                }
            }
        }
        if self.report.mode == FastMarchingTopologyMode::STRICT && nr != 1 {
            self.report.rejected_candidates += 1;
            self.report.rejected_merge += 1;
            return Ok(false);
        }
        if self.dim == 3 && self.background_components(x, y, z) != 1 {
            self.report.rejected_candidates += 1;
            self.report.rejected_background += 1;
            return Ok(false);
        }
        if nr == 0 || self.euler_delta(x, y, z) != 1 - nr as i64 {
            self.report.rejected_candidates += 1;
            self.report.rejected_handle += 1;
            return Ok(false);
        }
        self.parent[i] = i as i32;
        self.report.final_components += 1;
        self.join_neighbors(i);
        Ok(true)
    }
    fn euler_delta(&self, x: i64, y: i64, z: i64) -> i64 {
        let mut delta = 0i64;
        let dim = self.dim as usize;
        let zmax = if self.dim == 2 { 0 } else { 2 };
        for a in 0..3 {
            for b in 0..3 {
                for d in 0..=zmax {
                    let s = [a, b, d];
                    let degree = s[..dim].iter().filter(|v| **v == 2).count();
                    let mut covered = false;
                    for bits in 0..(1usize << dim) {
                        let mut off = [0i64; 3];
                        for axis in 0..dim {
                            if s[axis] != 2 && bits & (1 << axis) != 0 {
                                off[axis] = if s[axis] == 0 { -1 } else { 1 };
                            }
                        }
                        if off == [0, 0, 0] {
                            continue;
                        }
                        if self.occupied(x + off[0], y + off[1], z + off[2]) {
                            covered = true;
                            break;
                        }
                    }
                    if !covered {
                        if degree & 1 == 0 {
                            delta += 1;
                        } else {
                            delta -= 1;
                        }
                    }
                }
            }
        }
        delta
    }
    fn background_components(&self, x: i64, y: i64, z: i64) -> i64 {
        let mut bg = [false; 27];
        let mut seen = [false; 27];
        let mut queue = [0usize; 27];
        let (lo, hi) = self.z_range();
        for dz in lo..=hi {
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let j = ((dz + 1) * 9 + (dy + 1) * 3 + dx + 1) as usize;
                    bg[j] =
                        (dx != 0 || dy != 0 || dz != 0) && !self.occupied(x + dx, y + dy, z + dz);
                }
            }
        }
        let mut count = 0;
        for start in 0..27 {
            if !bg[start] || seen[start] {
                continue;
            }
            let mut tail = 1;
            queue[0] = start;
            seen[start] = true;
            let mut touches = false;
            let mut head = 0;
            while head < tail {
                let j = queue[head] as i64;
                head += 1;
                let (qx, qy, qz) = (j % 3, j / 3 % 3, j / 9);
                let dist = (qx - 1).abs() + (qy - 1).abs() + (qz - 1).abs();
                if dist == 1 {
                    touches = true;
                }
                for step in [
                    [-1i64, 0, 0],
                    [1, 0, 0],
                    [0, -1, 0],
                    [0, 1, 0],
                    [0, 0, -1],
                    [0, 0, 1],
                ] {
                    if self.dim == 2 && step[2] != 0 {
                        continue;
                    }
                    let (nx, ny, nz) = (qx + step[0], qy + step[1], qz + step[2]);
                    if !(0..=2).contains(&nx) || !(0..=2).contains(&ny) || !(0..=2).contains(&nz) {
                        continue;
                    }
                    let k = (nz * 9 + ny * 3 + nx) as usize;
                    if bg[k] && !seen[k] {
                        seen[k] = true;
                        queue[tail] = k;
                        tail += 1;
                    }
                }
            }
            if touches {
                count += 1;
            }
        }
        count
    }
}
