#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::Error;
use super::edge_levelset::{EdgeLevelSetStopping, LevelSetStopReason};
use super::levelset::{
    ERR_LEVEL_SET_RESOURCE_LIMIT, HeapItem, LS_FAR, LS_FROZEN, LS_TRIAL, LevelSetGrid,
    LevelSetHeap, LevelSetProgress, LevelSetResult, LevelSetState, LevelSetTerms, abs32, check,
    invalid,
};
use super::roimask::{MaskRun, RasterMask};
use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShapeEvolutionProfile(pub u8);
impl ShapeEvolutionProfile {
    pub const DENSE_BAND: ShapeEvolutionProfile = ShapeEvolutionProfile(0);
    pub const INTERFACE_LOCAL: ShapeEvolutionProfile = ShapeEvolutionProfile(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShapeSurfaceSampling(pub u8);
impl ShapeSurfaceSampling {
    pub const GRID_CENTRES: ShapeSurfaceSampling = ShapeSurfaceSampling(0);
    pub const INTERPOLATED_INTERFACE: ShapeSurfaceSampling = ShapeSurfaceSampling(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShapeInitializationProfile(pub u8);
impl ShapeInitializationProfile {
    pub const CITY_BLOCK_SEEDS: ShapeInitializationProfile = ShapeInitializationProfile(0);
    pub const BOUNDED_FACE_NORMAL_SEEDS: ShapeInitializationProfile = ShapeInitializationProfile(1);
}
impl LevelSetState<'_, '_> {
    fn coords_array(&self, i: usize) -> [usize; 3] {
        let (x, y, z) = self.g.coords(i);
        [x, y, z]
    }
    fn dims(&self) -> usize {
        self.g.dims as usize
    }
    fn step(&self, i: usize, pos: [usize; 3], axis: usize, direction: i64) -> Option<usize> {
        let c = pos[axis] as i64 + direction;
        if c < 0 || c >= self.g.size[axis] as i64 {
            return None;
        }
        Some((i as i64 + direction * self.g.stride[axis] as i64) as usize)
    }
    fn supported(&self, pos: [usize; 3]) -> bool {
        (0..self.dims()).all(|a| pos[a] != 0 && pos[a] != self.g.size[a] - 1)
    }
    fn interface_active(&self) -> Result<Vec<i32>, Error> {
        let mut out = Vec::new();
        for (k, &index) in self.band.iter().enumerate() {
            if k % 4096 == 0 {
                check(self.g.ctx)?;
            }
            let i = index as usize;
            let pos = self.coords_array(i);
            if !self.supported(pos) {
                continue;
            }
            let inside = self.phi[i] < 0.0;
            'axes: for a in 0..self.dims() {
                for d in [-1i64, 1] {
                    let Some(j) = self.step(i, pos, a, d) else {
                        continue;
                    };
                    if (self.phi[j] < 0.0) != inside && abs32(self.phi[i]) <= abs32(self.phi[j]) {
                        out.push(index);
                        break 'axes;
                    }
                }
            }
        }
        check(self.g.ctx)?;
        Ok(out)
    }
    fn interface_crossing(&self) -> Result<bool, Error> {
        let mut visits = 0u64;
        for i in 0..self.g.n {
            let pos = self.coords_array(i);
            for a in 0..self.dims() {
                for direction in [-1i64, 1] {
                    if visits.is_multiple_of(4096) {
                        check(self.g.ctx)?;
                    }
                    visits += 1;
                    let Some(j) = self.step(i, pos, a, direction) else {
                        continue;
                    };
                    if (self.phi[i] < 0.0) != (self.phi[j] < 0.0) {
                        return Ok(true);
                    }
                }
            }
        }
        check(self.g.ctx)?;
        Ok(false)
    }
    fn initial_interface_active(&self) -> Result<Vec<i32>, Error> {
        let out = self.interface_active()?;
        let half = (self.g.hmin / 2.0) as f32;
        let mut selected = Vec::with_capacity(out.len());
        for (k, &index) in out.iter().enumerate() {
            if k % 4096 == 0 {
                check(self.g.ctx)?;
            }
            let p = self.phi[index as usize];
            if p >= -half && p <= half {
                selected.push(index);
            }
        }
        check(self.g.ctx)?;
        Ok(selected)
    }
    fn prune_interface_anchors(
        &mut self,
        active: Vec<i32>,
        saved: &mut [f32],
        member: &mut [u8],
    ) -> Result<Vec<i32>, Error> {
        let mut selected = Vec::with_capacity(active.len());
        let mut visits = 0u64;
        for (k, &index) in active.iter().enumerate() {
            if k % 4096 == 0 {
                check(self.g.ctx)?;
            }
            let i = index as usize;
            let (pos, inside) = (self.coords_array(i), self.phi[i] < 0.0);
            let mut crossing = false;
            let mut axis = 0;
            while axis < self.dims() && !crossing {
                for direction in [-1i64, 1] {
                    if visits.is_multiple_of(4096) {
                        check(self.g.ctx)?;
                    }
                    visits += 1;
                    if let Some(j) = self.step(i, pos, axis, direction)
                        && (self.phi[j] < 0.0) != inside
                    {
                        crossing = true;
                        break;
                    }
                }
                axis += 1;
            }
            if crossing {
                selected.push(index);
            }
        }
        if selected.len() == active.len() || selected.is_empty() {
            check(self.g.ctx)?;
            return Ok(selected);
        }
        for (i, &p) in self.phi.iter().enumerate() {
            if i % 4096 == 0 {
                check(self.g.ctx)?;
            }
            saved[i] = p;
        }
        self.refresh_interface_layers(&selected, saved, member, true, true)?;
        check(self.g.ctx)?;
        Ok(selected)
    }
    fn initialize_interface_anchors(
        &mut self,
        profile: ShapeInitializationProfile,
    ) -> Result<(), Error> {
        if profile.0 > ShapeInitializationProfile::BOUNDED_FACE_NORMAL_SEEDS.0 {
            return Err(invalid("interface seed profile"));
        }
        check(self.g.ctx)?;
        let g = self.g;
        let mut seeds: Vec<(i32, f32)> = Vec::new();
        let mut visits = 0u64;
        for (i, &value) in self.phi.iter().enumerate() {
            let pos = self.coords_array(i);
            let mut best = f64::INFINITY;
            let mut face = [f64::INFINITY; 3];
            for a in 0..self.dims() {
                for direction in [-1i64, 1] {
                    if visits.is_multiple_of(4096) {
                        check(g.ctx)?;
                    }
                    visits += 1;
                    let Some(j) = self.step(i, pos, a, direction) else {
                        continue;
                    };
                    let other = self.phi[j];
                    if (value < 0.0) == (other < 0.0) {
                        continue;
                    }
                    let mut distance = 0.0;
                    let total = f64::from(value).abs() + f64::from(other).abs();
                    if total > 0.0 {
                        distance = g.hmin * f64::from(value).abs() / total;
                        if profile == ShapeInitializationProfile::BOUNDED_FACE_NORMAL_SEEDS {
                            let physical = g.h[a] * (f64::from(value).abs() / total);
                            face[a] = fmath::min(face[a], physical);
                        }
                    }
                    best = fmath::min(best, distance);
                }
            }
            if profile == ShapeInitializationProfile::BOUNDED_FACE_NORMAL_SEEDS
                && best > 0.0
                && best != f64::INFINITY
            {
                let mut nearest = f64::INFINITY;
                for &f in face.iter().take(self.dims()) {
                    nearest = fmath::min(nearest, f);
                }
                if nearest == 0.0 {
                    best = 0.0;
                } else if nearest != f64::INFINITY {
                    let mut normal = 0.0;
                    for &f in face.iter().take(self.dims()) {
                        if f != f64::INFINITY {
                            normal = fmath::hypot(normal, nearest / f);
                        }
                    }
                    best = fmath::min(best, nearest / normal);
                }
            }
            if best != f64::INFINITY {
                seeds.push((i as i32, best as f32));
            }
        }
        for i in 0..self.phi.len() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            let p = self.phi[i];
            self.phi[i] = if p < 0.0 { -self.width } else { self.width };
        }
        let mut band = Vec::with_capacity(seeds.len());
        for (k, &(i, d)) in seeds.iter().enumerate() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let iu = i as usize;
            self.phi[iu] = if self.phi[iu] < 0.0 { -d } else { d };
            band.push(i);
        }
        self.band = band;
        check(g.ctx)
    }
    fn interface_promote(
        &mut self,
        active: &[i32],
        old: &[f32],
        staged: &mut [f32],
        member: &mut [u8],
    ) -> Result<Vec<i32>, Error> {
        let g = self.g;
        let (half, c) = ((g.hmin / 2.0) as f32, g.hmin as f32);
        for (i, m) in member.iter_mut().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            *m = 0;
        }
        for (k, &index) in active.iter().enumerate() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let p = self.phi[index as usize];
            member[index as usize] = if p > -half && p <= half { 1 } else { 3 };
            if p < -half - c || p > half + c {
                return Err(invalid("interface update jumps beyond one layer"));
            }
        }
        let mut visits = 0u64;
        for (k, &index) in active.iter().enumerate() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let i = index as usize;
            if member[i] != 3 {
                continue;
            }
            let pos = self.coords_array(i);
            let p = self.phi[i];
            for a in 0..self.dims() {
                for direction in [-1i64, 1] {
                    if visits.is_multiple_of(4096) {
                        check(g.ctx)?;
                    }
                    visits += 1;
                    let Some(j) = self.step(i, pos, a, direction) else {
                        continue;
                    };
                    if member[j] == 1 || member[j] == 3 {
                        continue;
                    }
                    if !self.supported(self.coords_array(j)) {
                        continue;
                    }
                    let candidate = if p <= -half && old[j] >= 0.0 {
                        p + c
                    } else if p > half && old[j] < 0.0 {
                        p - c
                    } else {
                        continue;
                    };
                    if candidate <= -half || candidate > half {
                        return Err(invalid("interface promotion exceeds range"));
                    }
                    if member[j] != 2
                        || old[j] >= 0.0 && candidate < staged[j]
                        || old[j] < 0.0 && candidate > staged[j]
                    {
                        member[j] = 2;
                        staged[j] = candidate;
                    }
                }
            }
        }
        let mut next = Vec::new();
        for (i, &m) in member.iter().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            if m == 1 || m == 2 {
                if m == 2 {
                    self.phi[i] = staged[i];
                }
                next.push(i as i32);
            }
        }
        check(g.ctx)?;
        Ok(next)
    }
    fn refresh_interface_layers(
        &mut self,
        active: &[i32],
        saved: &mut [f32],
        member: &mut [u8],
        use_previous: bool,
        preserve_readiness: bool,
    ) -> Result<(), Error> {
        let g = self.g;
        check(g.ctx)?;
        if active.is_empty() {
            return Ok(());
        }
        for i in 0..member.len() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            let prior_readiness = member[i] & 4;
            member[i] = 0;
            if preserve_readiness {
                member[i] = prior_readiness << 1;
            }
            if self.phi[i] < 0.0 {
                member[i] |= 2;
            }
            if use_previous {
                let position = self.coords_array(i);
                let mut projected = position;
                let mut boundary = false;
                for axis in 0..self.dims() {
                    if position[axis] == 0 {
                        projected[axis] = 1;
                        boundary = true;
                    } else if position[axis] == g.size[axis] - 1 {
                        projected[axis] = g.size[axis] - 2;
                        boundary = true;
                    }
                }
                if boundary {
                    let j = g.index(projected[0], projected[1], projected[2]);
                    if (saved[i] < 0.0) == (saved[j] < 0.0)
                        && (!preserve_readiness || prior_readiness != 0)
                    {
                        member[i] |= 4;
                    }
                }
            }
            self.status[i] = LS_FAR;
        }
        for (k, &index) in active.iter().enumerate() {
            if k % 4096 == 0 {
                check(g.ctx)?;
            }
            let i = index as usize;
            member[i] |= 1;
            saved[i] = self.phi[i];
            self.status[i] = LS_FROZEN;
        }
        for (i, &m) in member.iter().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            if m & 1 == 0 {
                self.phi[i] = if m & 2 != 0 { -self.width } else { self.width };
            }
        }
        self.heap.items.clear();
        let mut visits = 0u64;
        let c = g.hmin as f32;
        for &index in active {
            let i = index as usize;
            let p = self.phi[i];
            for j in self.neighbours(i, &mut visits)?.into_iter().flatten() {
                let v = if member[j] & 2 != 0 { p - c } else { p + c };
                self.relax(j, v);
            }
        }
        let mut pops = 0u64;
        while !self.heap.items.is_empty() {
            if pops.is_multiple_of(4096) {
                check(g.ctx)?;
            }
            pops += 1;
            let item = self.heap.pop();
            let i = item.i as usize;
            if self.status[i] == LS_FROZEN || item.d != abs32(self.phi[i]) {
                continue;
            }
            if item.d >= self.width {
                break;
            }
            self.status[i] = LS_FROZEN;
            let value = if member[i] & 2 != 0 {
                self.phi[i] - c
            } else {
                self.phi[i] + c
            };
            for j in self.neighbours(i, &mut visits)?.into_iter().flatten() {
                if member[j] & 2 == member[i] & 2 {
                    self.relax(j, value);
                }
            }
        }
        if self.heap.overflow {
            return Err(Error::wrap(
                ERR_LEVEL_SET_RESOURCE_LIMIT,
                "interface layer heap",
            ));
        }
        self.heap.items.clear();
        let allowed = if use_previous { Some(&*member) } else { None };
        self.project_interface_boundary(saved, allowed)?;
        let mut band = Vec::new();
        for i in 0..self.phi.len() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            if abs32(self.phi[i]) < self.width {
                band.push(i as i32);
            }
            if preserve_readiness {
                member[i] = (member[i] & !12) | ((member[i] & 8) >> 1);
            }
        }
        self.band = band;
        check(g.ctx)
    }
    fn neighbours(&self, i: usize, visits: &mut u64) -> Result<[Option<usize>; 6], Error> {
        let pos = self.coords_array(i);
        let mut out = [None; 6];
        let mut k = 0;
        for a in 0..self.dims() {
            for direction in [-1i64, 1] {
                if (*visits).is_multiple_of(4096) {
                    check(self.g.ctx)?;
                }
                *visits += 1;
                out[k] = self.step(i, pos, a, direction);
                k += 1;
            }
        }
        Ok(out)
    }
    fn relax(&mut self, j: usize, value: f32) {
        if self.status[j] == LS_FROZEN || abs32(value) >= abs32(self.phi[j]) {
            return;
        }
        self.phi[j] = value;
        self.status[j] = LS_TRIAL;
        self.heap.push(HeapItem {
            d: abs32(value),
            i: j as i32,
        });
    }
    fn project_interface_boundary(
        &mut self,
        saved: &mut [f32],
        allowed: Option<&[u8]>,
    ) -> Result<(), Error> {
        let g = self.g;
        for (i, &p) in self.phi.iter().enumerate() {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            saved[i] = p;
        }
        for i in 0..g.n {
            if i % 4096 == 0 {
                check(g.ctx)?;
            }
            let pos = self.coords_array(i);
            let mut projected = pos;
            let mut directions = [0i64; 3];
            let mut boundary = false;
            for a in 0..self.dims() {
                if pos[a] == 0 {
                    projected[a] = 1;
                    directions[a] = 1;
                    boundary = true;
                } else if pos[a] == g.size[a] - 1 {
                    projected[a] = g.size[a] - 2;
                    directions[a] = -1;
                    boundary = true;
                }
            }
            if !boundary || allowed.is_some_and(|al| al[i] & 4 == 0) {
                continue;
            }
            let j = g.index(projected[0], projected[1], projected[2]);
            let mut compatible = abs32(saved[j]) < self.width;
            for a in 0..self.dims() {
                if directions[a] != 0
                    && saved[j] != saved[(j as i64 + directions[a] * g.stride[a] as i64) as usize]
                {
                    compatible = false;
                    break;
                }
            }
            if compatible {
                self.phi[i] = saved[j];
            }
        }
        check(g.ctx)
    }
    pub(crate) fn interface_stencil_value(&self, i: usize, allowed: &[u8]) -> f32 {
        if allowed[i] & 4 == 0 {
            return self.phi[i];
        }
        let mut pos = self.coords_array(i);
        for (axis, p) in pos.iter_mut().enumerate().take(self.dims()) {
            *p = (*p).min(self.g.size[axis] - 2).max(1);
        }
        let j = self.g.index(pos[0], pos[1], pos[2]);
        if (self.phi[i] < 0.0) != (self.phi[j] < 0.0) {
            return self.phi[i];
        }
        self.phi[j]
    }
    fn interface_potential(&self, i: usize, field: &[f32], allowed: &[u8]) -> f32 {
        let g = self.g;
        let pos = self.coords_array(i);
        let mut gradient = [0.0f64; 3];
        let mut norm = 0.0;
        for a in 0..self.dims() {
            let (lo, hi) = (self.neighbour(i, pos, a, -1), self.neighbour(i, pos, a, 1));
            gradient[a] = f64::from(
                self.interface_stencil_value(hi, allowed)
                    - self.interface_stencil_value(lo, allowed),
            ) / (2.0 * g.h[a]);
            norm += gradient[a] * gradient[a];
        }
        if norm <= 1e-12 {
            return field[i];
        }
        let mut lo = [0usize; 3];
        let mut fraction = [0.0f64; 3];
        for a in 0..self.dims() {
            let c = fmath::max(
                0.0,
                fmath::min(
                    (g.size[a] - 1) as f64,
                    pos[a] as f64 - f64::from(self.phi[i]) * gradient[a] / (norm * g.h[a]),
                ),
            );
            lo[a] = c.floor() as usize;
            fraction[a] = c - lo[a] as f64;
        }
        let mut value = 0.0;
        for corner in 0..(1usize << self.dims()) {
            let (mut weight, mut j) = (1.0f64, 0usize);
            for a in 0..3 {
                let mut p = lo[a];
                if a < self.dims() {
                    if corner & (1 << a) != 0 {
                        p = (p + 1).min(g.size[a] - 1);
                        weight *= fraction[a];
                    } else {
                        weight *= 1.0 - fraction[a];
                    }
                }
                j += p * g.stride[a];
            }
            value += weight * f64::from(field[j]);
        }
        fmath::max(0.0, fmath::min(1.0, value)) as f32
    }
}
fn interface_time_step(first: f64, curvature: f64, h: f64, dimensions: i64) -> f64 {
    if first == 0.0 && curvature == 0.0 {
        return 0.0;
    }
    let d = dimensions as f64;
    let constraint = 1.0 / (2.0 * d);
    let mut dt = f64::INFINITY;
    if first > 0.0 {
        dt = constraint * h / first;
    }
    if curvature > 0.0 {
        dt = fmath::min(dt, constraint * h * h / curvature);
    }
    let combined = (2.0 * d).sqrt() * first / h + 2.0 * d * curvature / (h * h);
    if combined > 0.0 {
        dt = fmath::min(dt, 1.0 / combined);
    }
    dt
}
fn non_finite(v: f64) -> bool {
    v.is_nan() || v.is_infinite()
}
impl LevelSetGrid<'_> {
    pub(crate) fn evolve_interface(
        &self,
        phi: Vec<f32>,
        terms: &LevelSetTerms,
        sampling: ShapeSurfaceSampling,
        initialization: ShapeInitializationProfile,
        maximum: i64,
        tolerance: f64,
        progress: Option<&dyn Fn(LevelSetProgress)>,
    ) -> Result<LevelSetResult, Error> {
        check(self.ctx)?;
        let mut s = LevelSetState {
            g: self,
            phi,
            band: Vec::new(),
            width: ((self.dims + 1) as f64 * self.hmin) as f32,
            status: vec![0u8; self.n],
            touched: Vec::new(),
            heap: LevelSetHeap {
                max_items: 7 * self.n,
                ..Default::default()
            },
        };
        s.initialize_interface_anchors(initialization)?;
        let mut rates = vec![0.0f32; self.n];
        let mut window = vec![false; self.n];
        let (mut old, mut staged, mut membership) = (
            vec![0.0f32; self.n],
            vec![0.0f32; self.n],
            vec![0u8; self.n],
        );
        let mut active = s.initial_interface_active()?;
        for (i, &p) in s.phi.iter().enumerate() {
            if i % 4096 == 0 {
                check(self.ctx)?;
            }
            old[i] = p;
        }
        s.refresh_interface_layers(&active, &mut old, &mut membership, true, false)?;
        active = s.prune_interface_anchors(active, &mut old, &mut membership)?;
        let snapshot = |window: &mut [bool], phi: &[f32]| -> Result<(), Error> {
            for (i, &p) in phi.iter().enumerate() {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                window[i] = p < 0.0;
            }
            check(self.ctx)
        };
        snapshot(&mut window, &s.phi)?;
        let (mut iterations, mut quiet, mut rms, mut converged) = (0i64, 0, 0.0f64, false);
        let mut reason = LevelSetStopReason::ITERATION_LIMIT;
        while iterations < maximum {
            if active.is_empty() {
                let crossing = s.interface_crossing()?;
                reason = if crossing {
                    LevelSetStopReason::BOUNDARY_ACTIVE_EXHAUSTED
                } else {
                    LevelSetStopReason::EMPTY_BAND
                };
                break;
            }
            let (mut first, mut curvature) = (0.0f64, 0.0f64);
            for (k, &index) in active.iter().enumerate() {
                if k % 4096 == 0 {
                    check(self.ctx)?;
                }
                let i = index as usize;
                let sampled = if sampling == ShapeSurfaceSampling::INTERPOLATED_INTERFACE {
                    let field = terms.speed.unwrap_or(&[]);
                    Some(s.interface_potential(i, field, &membership))
                } else {
                    None
                };
                let (rate, f, c) =
                    s.rate_potential_boundary(i, terms, sampled, 1e-6, Some(membership.as_slice()));
                if non_finite(f64::from(rate)) || non_finite(f) || non_finite(c) {
                    return Err(invalid("nonfinite interface rate"));
                }
                rates[k] = rate;
                first = fmath::max(first, f);
                curvature = fmath::max(curvature, c);
            }
            let dt = interface_time_step(first, curvature, self.hmin, self.dims);
            if dt == 0.0 {
                reason = LevelSetStopReason::ZERO_TIME_STEP;
                break;
            }
            for (i, &p) in s.phi.iter().enumerate() {
                if i % 4096 == 0 {
                    check(self.ctx)?;
                }
                old[i] = p;
            }
            let mut sum = 0.0f64;
            for (k, &index) in active.iter().enumerate() {
                if k % 4096 == 0 {
                    check(self.ctx)?;
                }
                let i = index as usize;
                let p = s.phi[i];
                old[i] = p;
                let change = dt * f64::from(rates[k]);
                let next = f64::from(p) + change;
                if non_finite(next) || next.abs() > f64::from(f32::MAX) {
                    return Err(invalid("nonfinite interface update"));
                }
                s.phi[i] = next as f32;
                let actual = f64::from(s.phi[i] - p);
                sum += actual * actual;
            }
            iterations += 1;
            rms = (sum / active.len() as f64).sqrt();
            let next = s.interface_promote(&active, &old, &mut staged, &mut membership)?;
            s.refresh_interface_layers(&next, &mut old, &mut membership, true, false)?;
            active = s.prune_interface_anchors(next, &mut old, &mut membership)?;
            let crossing = s.interface_crossing()?;
            let report = |progress: Option<&dyn Fn(LevelSetProgress)>| {
                if let Some(progress) = progress {
                    progress(LevelSetProgress {
                        iteration: iterations,
                        max_iterations: maximum,
                        rms_change: rms,
                    });
                }
            };
            if active.is_empty() || !crossing {
                reason = if crossing {
                    LevelSetStopReason::BOUNDARY_ACTIVE_EXHAUSTED
                } else {
                    LevelSetStopReason::EMPTY_BAND
                };
                report(progress);
                break;
            }
            report(progress);
            check(self.ctx)?;
            if rms < tolerance * self.hmin
                && terms.stopping != EdgeLevelSetStopping::FIXED_ITERATIONS
                && terms.stopping != EdgeLevelSetStopping::MASK_STABILITY_ONLY
            {
                reason = LevelSetStopReason::RMS;
                converged = true;
                break;
            }
            if iterations % 10 == 0 {
                let mut flips = 0i64;
                for (i, &p) in s.phi.iter().enumerate() {
                    if i % 4096 == 0 {
                        check(self.ctx)?;
                    }
                    if (p < 0.0) != window[i] {
                        flips += 1;
                    }
                }
                if flips as f64 <= 0.002 * active.len() as f64 {
                    quiet += 1;
                } else {
                    quiet = 0;
                }
                if quiet >= 2
                    && terms.stopping != EdgeLevelSetStopping::RMS_ONLY
                    && terms.stopping != EdgeLevelSetStopping::FIXED_ITERATIONS
                {
                    reason = LevelSetStopReason::STABLE_MASK;
                    converged = true;
                    break;
                }
                snapshot(&mut window, &s.phi)?;
            }
        }
        let mut result = self.interface_result(&s.phi)?;
        result.iterations = iterations;
        result.rms_change = rms;
        result.converged = converged;
        result.stop_reason = reason;
        check(self.ctx)?;
        Ok(result)
    }
    fn interface_result(&self, phi: &[f32]) -> Result<LevelSetResult, Error> {
        check(self.ctx)?;
        let mut counts = vec![0usize; self.ny * self.nz];
        let mut total = 0usize;
        for z in 0..self.nz {
            for y in 0..self.ny {
                let mut previous = false;
                for x in 0..self.nx {
                    let i = self.index(x, y, z);
                    if i.is_multiple_of(4096) {
                        check(self.ctx)?;
                    }
                    let inside = phi[i] < 0.0;
                    if inside && !previous {
                        counts[y + self.ny * z] += 1;
                        total += 1;
                    }
                    previous = inside;
                }
            }
        }
        if total > self.n {
            return Err(Error::sentinel(ERR_LEVEL_SET_RESOURCE_LIMIT));
        }
        check(self.ctx)?;
        let v = &self.volume;
        let mut out = LevelSetResult {
            region_origin: self.origin,
            region_size: [self.nx as i64, self.ny as i64, self.nz as i64],
            ..Default::default()
        };
        let sizes = [v.columns, v.rows, v.slices];
        let dims = self.dims as usize;
        for z in 0..self.nz {
            let mut mask: Option<RasterMask> = None;
            for y in 0..self.ny {
                let n = counts[y + self.ny * z];
                if n == 0 {
                    continue;
                }
                check(self.ctx)?;
                let m = mask.get_or_insert_with(|| RasterMask::new(v.columns, v.rows));
                let mut row = Vec::with_capacity(n);
                let mut start: i64 = -1;
                for x in 0..=self.nx {
                    if x % 4096 == 0 {
                        check(self.ctx)?;
                    }
                    let inside = x < self.nx && phi[self.index(x, y, z)] < 0.0;
                    if inside {
                        out.voxel_count += 1;
                        if start < 0 {
                            start = x as i64;
                        }
                        for (a, c) in [x, y, z].into_iter().enumerate() {
                            if a >= dims {
                                continue;
                            }
                            let vv = c as i64 + self.origin[a];
                            if vv == 0 || vv == sizes[a] - 1 {
                                out.touches_volume_boundary = true;
                            }
                            if c == 0 && self.origin[a] > 0
                                || c == self.size[a] - 1
                                    && self.origin[a] + (self.size[a] as i64) < sizes[a]
                            {
                                out.touches_region_boundary = true;
                            }
                        }
                    } else if start >= 0 {
                        row.push(MaskRun {
                            start: start + self.origin[0],
                            end: x as i64 + self.origin[0],
                        });
                        start = -1;
                    }
                }
                if row.len() != n {
                    return Err(invalid("unstable owned interface field"));
                }
                m.runs[(y as i64 + self.origin[1]) as usize] = row;
            }
            if let Some(mask) = mask {
                out.masks.insert(z as i64 + self.origin[2], mask);
            }
        }
        let mut voxel = self.h[0] * self.h[1];
        if self.dims == 3 {
            voxel *= self.h[2];
        }
        out.volume_mm3 = out.voxel_count as f64 * voxel;
        check(self.ctx)?;
        Ok(out)
    }
}
