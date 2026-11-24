#![forbid(unsafe_code)]
use super::cpr_path_assistant::{
    bounded_cpr_assistant_product, clamp_int, finite_vec3, positive_finite,
};
use super::fast_marching_topology::{FastMarchingTopologyReport, FmTopology};
use super::grid::nearest_index;
use super::parallel::Cancellation;
use super::vec3::Vec3;
use crate::fmath;
use crate::textfmt::float_v;
use std::fmt;
pub const MAX_FAST_MARCHING_NODES: i64 = 64 << 20;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FastMarchingErrorKind {
    Input,
    Limit,
    TopologyLimit,
    Canceled,
    DeadlineExceeded,
}
#[derive(Debug, Clone, PartialEq)]
pub struct FastMarchingError {
    kind: FastMarchingErrorKind,
    message: String,
    limit: Option<(&'static str, i64)>,
    topology: Option<Box<FastMarchingTopologyReport>>,
}
impl FastMarchingError {
    pub(crate) fn new(kind: FastMarchingErrorKind, message: impl Into<String>) -> Self {
        FastMarchingError {
            kind,
            message: message.into(),
            limit: None,
            topology: None,
        }
    }
    pub(crate) fn input(detail: impl AsRef<str>) -> Self {
        Self::new(
            FastMarchingErrorKind::Input,
            format!("render: invalid fast marching input: {}", detail.as_ref()),
        )
    }
    pub(crate) fn limit_exceeded(resource: &'static str, limit: i64) -> Self {
        let mut e = Self::new(
            FastMarchingErrorKind::Limit,
            format!("render: fast marching exceeded {limit} {resource} nodes"),
        );
        e.limit = Some((resource, limit));
        e
    }
    pub(crate) fn from_ctx(e: super::RenderError) -> Self {
        let kind = if e.is(super::ErrorKind::DeadlineExceeded) {
            FastMarchingErrorKind::DeadlineExceeded
        } else {
            FastMarchingErrorKind::Canceled
        };
        Self::new(kind, e.message())
    }
    pub(crate) fn with_topology(mut self, report: FastMarchingTopologyReport) -> Self {
        self.topology = Some(Box::new(report));
        self
    }
    pub fn kind(&self) -> FastMarchingErrorKind {
        self.kind
    }
    pub fn is(&self, kind: FastMarchingErrorKind) -> bool {
        self.kind == kind
    }
    #[doc(hidden)]
    pub fn cause_message(&self) -> &str {
        &self.message
    }
    pub fn limit(&self) -> Option<(&'static str, i64)> {
        self.limit
    }
    #[doc(hidden)]
    pub fn topology_report(&self) -> Option<&FastMarchingTopologyReport> {
        self.topology.as_deref()
    }
}
impl fmt::Display for FastMarchingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.topology.is_some() {
            f.write_str("render: topology fast marching: ")?;
        }
        f.write_str(&self.message)
    }
}
impl std::error::Error for FastMarchingError {}
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub(crate) fn ctx_check(ctx: Ctx<'_>) -> Result<(), FastMarchingError> {
    match ctx.and_then(|c| c.err()) {
        None => Ok(()),
        Some(e) => Err(FastMarchingError::from_ctx(e)),
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FastMarchingGrid {
    pub nx: i64,
    pub ny: i64,
    pub nz: i64,
    pub spacing: Vec3,
    pub speed: Vec<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FastMarchingSeed {
    pub index: [i64; 3],
    pub value: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FastMarchingOptions {
    pub stopping_value: f64,
    pub targets: Vec<[i64; 3]>,
    pub max_frozen: i64,
    pub max_trial: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FastMarchingResult {
    pub nx: i64,
    pub ny: i64,
    pub nz: i64,
    pub spacing: Vec3,
    pub times: Vec<f64>,
    pub seeds: Vec<[i64; 3]>,
    pub frozen: i64,
    pub targets_reached: bool,
}
const FM_FAR: u8 = 0;
const FM_TRIAL: u8 = 1;
const FM_FROZEN: u8 = 2;
const FM_TOPOLOGY_REJECTED: u8 = 3;
const FM_TOPOLOGY_SEED_TRIAL: u8 = 4;
pub fn fast_marching(
    ctx: Ctx<'_>,
    grid: &FastMarchingGrid,
    seeds: &[FastMarchingSeed],
    options: &FastMarchingOptions,
) -> Result<FastMarchingResult, FastMarchingError> {
    fast_marching_controlled(ctx, grid, seeds, options, false, None)
}
pub fn fast_marching_fixed_buffers(
    ctx: Ctx<'_>,
    grid: &FastMarchingGrid,
    seeds: &[FastMarchingSeed],
    options: &FastMarchingOptions,
) -> Result<FastMarchingResult, FastMarchingError> {
    fast_marching_controlled(ctx, grid, seeds, options, true, None)
}
fn index_v(index: [i64; 3]) -> String {
    format!("[{} {} {}]", index[0], index[1], index[2])
}
pub(crate) fn fast_marching_controlled(
    ctx: Ctx<'_>,
    grid: &FastMarchingGrid,
    seeds: &[FastMarchingSeed],
    options: &FastMarchingOptions,
    fixed_buffers: bool,
    mut control: Option<&mut FmTopology<'_>>,
) -> Result<FastMarchingResult, FastMarchingError> {
    if grid.nx <= 0 || grid.ny <= 0 || grid.nz <= 0 {
        return Err(FastMarchingError::input(format!(
            "grid size {}x{}x{}",
            grid.nx, grid.ny, grid.nz
        )));
    }
    let (count, ok) =
        bounded_cpr_assistant_product(grid.nx, grid.ny, grid.nz, MAX_FAST_MARCHING_NODES);
    if !ok || count as usize != grid.speed.len() {
        return Err(FastMarchingError::input(format!(
            "{} speeds for a {}x{}x{} grid",
            grid.speed.len(),
            grid.nx,
            grid.ny,
            grid.nz
        )));
    }
    if !positive_finite(grid.spacing.x)
        || !positive_finite(grid.spacing.y)
        || !positive_finite(grid.spacing.z)
    {
        return Err(FastMarchingError::input(
            "spacing must be positive and finite",
        ));
    }
    if seeds.is_empty() {
        return Err(FastMarchingError::input("no seed"));
    }
    if options.stopping_value < 0.0
        || options.stopping_value.is_nan()
        || options.max_frozen < 0
        || options.max_trial < 0
    {
        return Err(FastMarchingError::input("invalid stopping criteria"));
    }
    ctx_check(ctx)?;
    let count = count as usize;
    let mut m = FastMarcher {
        nx: grid.nx,
        ny: grid.ny,
        nz: grid.nz,
        h: [grid.spacing.x, grid.spacing.y, grid.spacing.z],
        speed: &grid.speed,
        times: vec![f64::INFINITY; count],
        state: vec![FM_FAR; count],
        queue: FmHeap {
            nodes: if fixed_buffers {
                Vec::with_capacity(count)
            } else {
                Vec::new()
            },
            pos: vec![-1; count],
        },
    };
    let mut result = FastMarchingResult {
        nx: grid.nx,
        ny: grid.ny,
        nz: grid.nz,
        spacing: grid.spacing,
        times: Vec::new(),
        seeds: Vec::with_capacity(seeds.len()),
        frozen: 0,
        targets_reached: false,
    };
    for seed in seeds {
        if !m.inside(seed.index) || seed.value.is_nan() || seed.value.is_infinite() {
            return Err(FastMarchingError::input(format!(
                "seed {}",
                index_v(seed.index)
            )));
        }
        let index = m.index(seed.index[0], seed.index[1], seed.index[2]);
        if seed.value < m.times[index] {
            m.times[index] = seed.value;
            m.state[index] = if control.is_some() {
                FM_TOPOLOGY_SEED_TRIAL
            } else {
                FM_TRIAL
            };
            m.queue.update(index, &m.times);
        }
        result.seeds.push(seed.index);
    }
    let mut targets: Vec<usize> = Vec::with_capacity(options.targets.len());
    for target in &options.targets {
        if let Some(c) = control.as_deref_mut() {
            c.charge(1)?;
        }
        if !m.inside(*target) {
            return Err(FastMarchingError::input(format!(
                "target {}",
                index_v(*target)
            )));
        }
        targets.push(m.index(target[0], target[1], target[2]));
    }
    if control.is_some() && options.max_trial > 0 && m.queue.nodes.len() as i64 > options.max_trial
    {
        return Err(FastMarchingError::limit_exceeded(
            "trial",
            options.max_trial,
        ));
    }
    let mut pending = targets.len() as i64;
    let mut pending_seeds = if control.is_some() {
        m.queue.nodes.len() as i64
    } else {
        0
    };
    let mut stop_after = f64::INFINITY;
    let mut pops: u64 = 0;
    while !m.queue.nodes.is_empty() {
        if pops & 4095 == 0 {
            ctx_check(ctx)?;
        }
        pops += 1;
        let next = m.times[m.queue.nodes[0] as usize];
        let by_value = options.stopping_value > 0.0 && next > options.stopping_value;
        if by_value || (next > stop_after && pending_seeds == 0) {
            if let Some(c) = control.as_deref_mut() {
                c.report.stop_reason = if by_value {
                    "stopping-value".into()
                } else {
                    "targets".into()
                };
            }
            break;
        }
        let index = m.queue.pop(&m.times);
        if m.state[index] == FM_TOPOLOGY_SEED_TRIAL {
            pending_seeds -= 1;
        }
        if let Some(c) = control.as_deref_mut()
            && !c.admit(index)?
        {
            m.state[index] = FM_TOPOLOGY_REJECTED;
            continue;
        }
        let item_time = m.times[index];
        m.state[index] = FM_FROZEN;
        result.frozen += 1;
        if let Some(c) = control.as_deref_mut() {
            c.report.frozen = result.frozen;
        }
        if options.max_frozen > 0 && result.frozen > options.max_frozen {
            return Err(FastMarchingError::limit_exceeded(
                "frozen",
                options.max_frozen,
            ));
        }
        if pending > 0 {
            for target in &targets {
                if let Some(c) = control.as_deref_mut() {
                    c.charge(1)?;
                }
                if *target == index {
                    pending -= 1;
                }
            }
            if pending == 0 {
                stop_after = item_time + 2.0 * m.max_step() / m.speed_at(index);
            }
        }
        m.update_neighbours(index);
        if options.max_trial > 0 && m.queue.nodes.len() as i64 > options.max_trial {
            return Err(FastMarchingError::limit_exceeded(
                "trial",
                options.max_trial,
            ));
        }
    }
    result.targets_reached = !targets.is_empty() && pending <= 0;
    for target in &targets {
        if let Some(c) = control.as_deref_mut() {
            c.charge(1)?;
        }
        if m.state[*target] != FM_FROZEN {
            result.targets_reached = false;
        }
    }
    for i in 0..count {
        if let Some(c) = control.as_deref_mut() {
            c.charge(1)?;
        }
        if i % 4096 == 0 {
            ctx_check(ctx)?;
        }
        if m.state[i] != FM_FROZEN {
            m.times[i] = f64::INFINITY;
        }
    }
    if control.is_some() {
        ctx_check(ctx)?;
    }
    result.times = m.times;
    Ok(result)
}
struct FastMarcher<'a> {
    nx: i64,
    ny: i64,
    nz: i64,
    h: [f64; 3],
    speed: &'a [f64],
    times: Vec<f64>,
    state: Vec<u8>,
    queue: FmHeap,
}
impl FastMarcher<'_> {
    fn inside(&self, index: [i64; 3]) -> bool {
        index[0] >= 0
            && index[1] >= 0
            && index[2] >= 0
            && index[0] < self.nx
            && index[1] < self.ny
            && index[2] < self.nz
    }
    fn index(&self, x: i64, y: i64, z: i64) -> usize {
        ((z * self.ny + y) * self.nx + x) as usize
    }
    fn max_step(&self) -> f64 {
        fmath::max(self.h[0], fmath::max(self.h[1], self.h[2]))
    }
    fn speed_at(&self, index: usize) -> f64 {
        let speed = self.speed[index];
        if positive_finite(speed) {
            return speed;
        }
        f64::from_bits(1)
    }
    fn update_neighbours(&mut self, index: usize) {
        let index = index as i64;
        let x = index % self.nx;
        let y = (index / self.nx) % self.ny;
        let z = index / (self.nx * self.ny);
        for n in [
            [x - 1, y, z],
            [x + 1, y, z],
            [x, y - 1, z],
            [x, y + 1, z],
            [x, y, z - 1],
            [x, y, z + 1],
        ] {
            if !self.inside(n) {
                continue;
            }
            let neighbour = self.index(n[0], n[1], n[2]);
            let state = self.state[neighbour];
            if state == FM_FROZEN
                || state == FM_TOPOLOGY_REJECTED
                || state == FM_TOPOLOGY_SEED_TRIAL
                || !positive_finite(self.speed[neighbour])
            {
                continue;
            }
            let value = self.solve(n[0], n[1], n[2], neighbour);
            if value < self.times[neighbour] {
                self.times[neighbour] = value;
                self.state[neighbour] = FM_TRIAL;
                self.queue.update(neighbour, &self.times);
            }
        }
    }
    fn solve(&self, x: i64, y: i64, z: i64, index: usize) -> f64 {
        let mut a = [0f64; 3];
        let mut h = [0f64; 3];
        let mut n = 0usize;
        let coords = [x, y, z];
        let sizes = [self.nx, self.ny, self.nz];
        for axis in 0..3 {
            let mut best = f64::INFINITY;
            for delta in [-1i64, 1] {
                let mut c = coords;
                c[axis] += delta;
                if c[axis] < 0 || c[axis] >= sizes[axis] {
                    continue;
                }
                let neighbour = self.index(c[0], c[1], c[2]);
                if self.state[neighbour] == FM_FROZEN && self.times[neighbour] < best {
                    best = self.times[neighbour];
                }
            }
            if !(best.is_infinite() && best > 0.0) {
                a[n] = best;
                h[n] = self.h[axis];
                n += 1;
            }
        }
        for i in 1..n {
            let mut j = i;
            while j > 0 && a[j] < a[j - 1] {
                a.swap(j, j - 1);
                h.swap(j, j - 1);
                j -= 1;
            }
        }
        let rhs = 1.0 / (self.speed[index] * self.speed[index]);
        let (mut sum_w, mut sum_wa, mut sum_wa2) = (0f64, 0f64, 0f64);
        let mut solution = f64::INFINITY;
        for k in 0..n {
            if k > 0 && solution <= a[k] {
                break;
            }
            let w = 1.0 / (h[k] * h[k]);
            sum_w += w;
            sum_wa += w * a[k];
            sum_wa2 += w * a[k] * a[k];
            let disc = sum_wa * sum_wa - sum_w * (sum_wa2 - rhs);
            if disc < 0.0 {
                break;
            }
            solution = (sum_wa + disc.sqrt()) / sum_w;
        }
        solution
    }
}
impl FastMarchingResult {
    fn sizes(&self) -> [i64; 3] {
        [self.nx, self.ny, self.nz]
    }
    pub fn gradient(&self, index: [i64; 3]) -> Option<Vec3> {
        if index[0] < 0
            || index[1] < 0
            || index[2] < 0
            || index[0] >= self.nx
            || index[1] >= self.ny
            || index[2] >= self.nz
        {
            return None;
        }
        let at = self.times[((index[2] * self.ny + index[1]) * self.nx + index[0]) as usize];
        if at.is_infinite() && at > 0.0 {
            return None;
        }
        let sizes = self.sizes();
        let h = [self.spacing.x, self.spacing.y, self.spacing.z];
        let mut g = [0f64; 3];
        for axis in 0..3 {
            let (mut best, mut sign) = (at, 0.0);
            for delta in [-1i64, 1] {
                let mut c = index;
                c[axis] += delta;
                if c[axis] < 0 || c[axis] >= sizes[axis] {
                    continue;
                }
                let value = self.times[((c[2] * self.ny + c[1]) * self.nx + c[0]) as usize];
                if value < best {
                    best = value;
                    sign = -delta as f64;
                }
            }
            if sign != 0.0 {
                g[axis] = sign * (at - best) / h[axis];
            }
        }
        Some(Vec3::new(g[0], g[1], g[2]))
    }
    fn time_at(&self, x: i64, y: i64, z: i64) -> f64 {
        if x < 0 || y < 0 || z < 0 || x >= self.nx || y >= self.ny || z >= self.nz {
            return f64::INFINITY;
        }
        self.times[((z * self.ny + y) * self.nx + x) as usize]
    }
    fn interpolate(&self, p: Vec3) -> Option<(Vec3, f64)> {
        let mut base = [0i64; 3];
        let mut frac = [0f64; 3];
        let sizes = self.sizes();
        for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
            if sizes[axis] == 1 {
                continue;
            }
            let b = clamp_int(fmath::f64_to_i64(value.floor()), 0, sizes[axis] - 2);
            base[axis] = b;
            frac[axis] = fmath::min(1.0, fmath::max(0.0, value - b as f64));
        }
        let mut gradient = Vec3::ZERO;
        let (mut time, mut weights) = (0f64, 0f64);
        for corner in 0..8 {
            let mut c = base;
            let mut w = 1.0;
            for axis in 0..3 {
                if corner >> axis & 1 == 1 {
                    if sizes[axis] == 1 {
                        w = 0.0;
                        break;
                    }
                    c[axis] += 1;
                    w *= frac[axis];
                } else {
                    w *= 1.0 - frac[axis];
                }
            }
            if w == 0.0 {
                continue;
            }
            let Some(g) = self.gradient(c) else {
                continue;
            };
            gradient = gradient.add(g.scale(w));
            time += w * self.time_at(c[0], c[1], c[2]);
            weights += w;
        }
        if weights < 1e-9 {
            return None;
        }
        Some((gradient.scale(1.0 / weights), time / weights))
    }
    pub fn minimal_path(
        &self,
        ctx: Ctx<'_>,
        from: Vec3,
        step_size: f64,
    ) -> Result<Vec<Vec3>, FastMarchingError> {
        if !finite_vec3(from)
            || from.x < 0.0
            || from.y < 0.0
            || from.z < 0.0
            || from.x > (self.nx - 1) as f64
            || from.y > (self.ny - 1) as f64
            || from.z > (self.nz - 1) as f64
        {
            return Err(FastMarchingError::input(format!(
                "path start {{X:{} Y:{} Z:{}}} is outside the grid",
                float_v(from.x),
                float_v(from.y),
                float_v(from.z)
            )));
        }
        let min_step = fmath::min(self.spacing.x, fmath::min(self.spacing.y, self.spacing.z));
        let mut step_size = step_size;
        if step_size <= 0.0 || step_size.is_nan() || step_size.is_infinite() {
            step_size = min_step / 2.0;
        }
        if self.seeds.is_empty() {
            return Err(FastMarchingError::input("no seed"));
        }
        let spacing = self.spacing;
        let physical = |p: Vec3| Vec3::new(p.x * spacing.x, p.y * spacing.y, p.z * spacing.z);
        let seed_distance = |p: Vec3| -> (f64, Vec3) {
            let (mut best, mut at) = (f64::INFINITY, Vec3::ZERO);
            for seed in &self.seeds {
                let s = Vec3::new(seed[0] as f64, seed[1] as f64, seed[2] as f64);
                let d = physical(p.sub(s)).length();
                if d < best {
                    best = d;
                    at = s;
                }
            }
            (best, at)
        };
        let mut path = vec![from];
        let mut p = from;
        let extent = physical(Vec3::new(self.nx as f64, self.ny as f64, self.nz as f64)).length();
        let max_steps = fmath::f64_to_i64(fmath::min(4.0 * extent / step_size + 64.0, 4_000_000.0));
        let (mut best_time, mut since_best) = (f64::INFINITY, 0i64);
        for step in 0..max_steps {
            if step & 1023 == 0 {
                ctx_check(ctx)?;
            }
            let (d, seed) = seed_distance(p);
            if d <= 1.5 * step_size {
                if d > 0.0 {
                    path.push(seed);
                }
                return Ok(path);
            }
            let Some((gradient, time)) = self.interpolate(p) else {
                break;
            };
            let norm = gradient.length();
            if !(norm > 0.0) || norm.is_infinite() {
                break;
            }
            if time < best_time - 1e-12 {
                best_time = time;
                since_best = 0;
            } else {
                since_best += 1;
                if since_best > 32 {
                    break;
                }
            }
            let direction = gradient.scale(1.0 / norm);
            let mut next = Vec3::new(
                p.x - direction.x * step_size / self.spacing.x,
                p.y - direction.y * step_size / self.spacing.y,
                p.z - direction.z * step_size / self.spacing.z,
            );
            next.x = fmath::min((self.nx - 1) as f64, fmath::max(0.0, next.x));
            next.y = fmath::min((self.ny - 1) as f64, fmath::max(0.0, next.y));
            next.z = fmath::min((self.nz - 1) as f64, fmath::max(0.0, next.z));
            p = next;
            path.push(p);
        }
        self.finish_discrete(ctx, path)
    }
    fn finish_discrete(
        &self,
        ctx: Ctx<'_>,
        mut path: Vec<Vec3>,
    ) -> Result<Vec<Vec3>, FastMarchingError> {
        let last = path[path.len() - 1];
        let mut x = clamp_int(nearest_index(last.x), 0, self.nx - 1);
        let mut y = clamp_int(nearest_index(last.y), 0, self.ny - 1);
        let mut z = clamp_int(nearest_index(last.z), 0, self.nz - 1);
        let unreached = |t: f64| t.is_infinite() && t > 0.0;
        if unreached(self.time_at(x, y, z)) {
            let mut best = f64::INFINITY;
            for dz in 0..=1 {
                for dy in 0..=1 {
                    for dx in 0..=1 {
                        let (cx, cy, cz) = (
                            fmath::f64_to_i64(last.x.floor()) + dx,
                            fmath::f64_to_i64(last.y.floor()) + dy,
                            fmath::f64_to_i64(last.z.floor()) + dz,
                        );
                        let value = self.time_at(cx, cy, cz);
                        if value < best {
                            best = value;
                            x = cx;
                            y = cy;
                            z = cz;
                        }
                    }
                }
            }
            if unreached(best) {
                return Err(FastMarchingError::input(
                    "the path start was not reached by the front",
                ));
            }
        }
        let is_seed = |x: i64, y: i64, z: i64| self.seeds.contains(&[x, y, z]);
        let mut steps: u64 = 0;
        loop {
            if steps & 1023 == 0 {
                ctx_check(ctx)?;
            }
            steps += 1;
            let node = Vec3::new(x as f64, y as f64, z as f64);
            if path[path.len() - 1] != node {
                path.push(node);
            }
            if is_seed(x, y, z) {
                return Ok(path);
            }
            let current = self.time_at(x, y, z);
            let (mut bx, mut by, mut bz, mut best) = (x, y, z, current);
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let value = self.time_at(x + dx, y + dy, z + dz);
                        if value < best {
                            bx = x + dx;
                            by = y + dy;
                            bz = z + dz;
                            best = value;
                        }
                    }
                }
            }
            if best >= current {
                return Ok(path);
            }
            x = bx;
            y = by;
            z = bz;
        }
    }
}
struct FmHeap {
    nodes: Vec<i32>,
    pos: Vec<i32>,
}
impl FmHeap {
    fn update(&mut self, node: usize, times: &[f64]) {
        let mut at = self.pos[node];
        if at < 0 {
            at = self.nodes.len() as i32;
            self.nodes.push(node as i32);
            self.pos[node] = at;
        }
        self.up(at as usize, times);
    }
    fn pop(&mut self, times: &[f64]) -> usize {
        let top = self.nodes[0] as usize;
        let last = self.nodes.len() - 1;
        self.swap(0, last);
        self.nodes.truncate(last);
        self.pos[top] = -1;
        if last > 0 {
            self.down(0, times);
        }
        top
    }
    fn less(&self, i: usize, j: usize, times: &[f64]) -> bool {
        times[self.nodes[i] as usize] < times[self.nodes[j] as usize]
    }
    fn swap(&mut self, i: usize, j: usize) {
        self.nodes.swap(i, j);
        self.pos[self.nodes[i] as usize] = i as i32;
        self.pos[self.nodes[j] as usize] = j as i32;
    }
    fn up(&mut self, mut i: usize, times: &[f64]) {
        while i > 0 {
            let parent = (i - 1) / 2;
            if !self.less(i, parent, times) {
                return;
            }
            self.swap(i, parent);
            i = parent;
        }
    }
    fn down(&mut self, mut i: usize, times: &[f64]) {
        let n = self.nodes.len();
        loop {
            let mut smallest = i;
            for child in [2 * i + 1, 2 * i + 2] {
                if child < n && self.less(child, smallest, times) {
                    smallest = child;
                }
            }
            if smallest == i {
                return;
            }
            self.swap(i, smallest);
            i = smallest;
        }
    }
}
