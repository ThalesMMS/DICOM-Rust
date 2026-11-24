#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::distance3d::{ERR_INVALID_VOLUME_MASK, VolumeMask, ctx_check};
use super::{Cancellation, Error};
pub const ERR_SKELETON_PATH: &str = "roi: no skeleton path";
pub const MAX_SKELETON_VOXELS: i64 = 1 << 28;
const EULER_LUT_ODD: [i8; 128] = [
    1, -1, -1, 1, -3, -1, -1, 1, -1, 1, 1, -1, 3, 1, 1, -1, -3, -1, 3, 1, 1, -1, 3, 1, -1, 1, 1,
    -1, 3, 1, 1, -1, -3, 3, -1, 1, 1, 3, -1, 1, -1, 1, 1, -1, 3, 1, 1, -1, 1, 3, 3, 1, 5, 3, 3, 1,
    -1, 1, 1, -1, 3, 1, 1, -1, -7, -1, -1, 1, -3, -1, -1, 1, -1, 1, 1, -1, 3, 1, 1, -1, -3, -1, 3,
    1, 1, -1, 3, 1, -1, 1, 1, -1, 3, 1, 1, -1, -3, 3, -1, 1, 1, 3, -1, 1, -1, 1, 1, -1, 3, 1, 1,
    -1, 1, 3, 3, 1, 5, 3, 3, 1, -1, 1, 1, -1, 3, 1, 1, -1,
];
const EULER_OCTANTS: [[usize; 7]; 8] = [
    [24, 25, 15, 16, 21, 22, 12],
    [26, 23, 17, 14, 25, 22, 16],
    [18, 21, 9, 12, 19, 22, 10],
    [20, 23, 19, 22, 11, 14, 10],
    [6, 15, 7, 16, 3, 12, 4],
    [8, 7, 17, 16, 5, 4, 14],
    [0, 9, 3, 12, 1, 10, 4],
    [2, 1, 11, 10, 5, 4, 14],
];
fn neighbour_adjacent(a: usize, b: usize) -> bool {
    if a == b || a == 13 || b == 13 {
        return false;
    }
    let (ax, ay, az) = (a % 3, a / 3 % 3, a / 9);
    let (bx, by, bz) = (b % 3, b / 3 % 3, b / 9);
    ax.abs_diff(bx) <= 1 && ay.abs_diff(by) <= 1 && az.abs_diff(bz) <= 1
}
impl VolumeMask {
    pub fn thin(&self, ctx: Option<&dyn Cancellation>) -> Result<VolumeMask, Error> {
        self.validate()?;
        let (px, py, pz) = (self.columns + 2, self.rows + 2, self.slices + 2);
        if px as f64 * py as f64 * pz as f64 > MAX_SKELETON_VOXELS as f64 {
            return Err(Error::wrap(
                ERR_INVALID_VOLUME_MASK,
                format_args!(
                    "{}×{}×{} voxels exceed the skeleton limit",
                    self.columns, self.rows, self.slices
                ),
            ));
        }
        let mut image = vec![0u8; (px * py * pz) as usize];
        let mut foreground: Vec<i64> = Vec::new();
        for z in 0..self.slices {
            for y in 0..self.rows {
                for x in 0..self.columns {
                    if self.voxels[self.index(x, y, z)] {
                        let p = ((z + 1) * py + y + 1) * px + x + 1;
                        image[p as usize] = 1;
                        foreground.push(p);
                    }
                }
            }
        }
        let mut offsets = [0i64; 27];
        for (i, offset) in offsets.iter_mut().enumerate() {
            let i = i as i64;
            *offset = (i / 9 - 1) * px * py + (i / 3 % 3 - 1) * px + (i % 3 - 1);
        }
        let borders = [-px, px, 1, -1, px * py, -px * py];
        let gather = |image: &[u8], p: i64| -> [u8; 27] {
            let mut neighbourhood = [0u8; 27];
            for (i, offset) in offsets.iter().enumerate() {
                neighbourhood[i] = image[(p + offset) as usize];
            }
            neighbourhood
        };
        let mut candidates: Vec<i64> = Vec::new();
        let mut unchanged = 0;
        while unchanged < 6 {
            unchanged = 0;
            for border in borders {
                ctx_check(ctx)?;
                candidates.clear();
                for &p in &foreground {
                    if image[p as usize] != 1 || image[(p + border) as usize] != 0 {
                        continue;
                    }
                    let neighbourhood = gather(&image, p);
                    let mut neighbours = -1i64;
                    for v in neighbourhood {
                        neighbours += i64::from(v);
                    }
                    if neighbours == 1
                        || !euler_invariant(&neighbourhood)
                        || !simple_point(&neighbourhood)
                    {
                        continue;
                    }
                    candidates.push(p);
                }
                let mut changed = false;
                for &p in &candidates {
                    image[p as usize] = 0;
                    if simple_point(&gather(&image, p)) {
                        changed = true;
                    } else {
                        image[p as usize] = 1;
                    }
                }
                if !changed {
                    unchanged += 1;
                }
                foreground.retain(|&p| image[p as usize] == 1);
            }
        }
        let mut out = VolumeMask {
            columns: self.columns,
            rows: self.rows,
            slices: self.slices,
            x: self.x.clone(),
            y: self.y.clone(),
            z: self.z.clone(),
            voxels: vec![false; self.voxels.len()],
        };
        for &p in &foreground {
            let (x, y, z) = (p % px - 1, p / px % py - 1, p / (px * py) - 1);
            let i = self.index(x, y, z);
            out.voxels[i] = true;
        }
        Ok(out)
    }
}
fn euler_invariant(n: &[u8; 27]) -> bool {
    let mut sum = 0i64;
    for octant in &EULER_OCTANTS {
        let mut index = 1usize;
        for (bit, &position) in octant.iter().enumerate() {
            if n[position] == 1 {
                index |= 1 << (7 - bit);
            }
        }
        sum += i64::from(EULER_LUT_ODD[index / 2]);
    }
    sum == 0
}
fn simple_point(n: &[u8; 27]) -> bool {
    let mut seen = [false; 27];
    let mut stack = [0usize; 27];
    let mut components = 0;
    for start in 0..27 {
        if start == 13 || n[start] != 1 || seen[start] {
            continue;
        }
        components += 1;
        if components > 1 {
            return false;
        }
        let mut top = 0isize;
        stack[0] = start;
        seen[start] = true;
        while top >= 0 {
            let current = stack[top as usize];
            top -= 1;
            for next in 0..27 {
                if neighbour_adjacent(current, next) && n[next] == 1 && !seen[next] {
                    seen[next] = true;
                    top += 1;
                    stack[top as usize] = next;
                }
            }
        }
    }
    true
}
#[derive(Debug, Clone, Copy)]
struct SkeletonEdge {
    to: usize,
    length: f64,
}
struct SkeletonGraph<'a> {
    mask: &'a VolumeMask,
    nodes: Vec<usize>,
    adj: Vec<Vec<SkeletonEdge>>,
}
fn sq(v: f64) -> f64 {
    v * v
}
impl<'a> SkeletonGraph<'a> {
    fn new(m: &'a VolumeMask) -> SkeletonGraph<'a> {
        const NONE: usize = usize::MAX;
        let mut node = vec![NONE; m.voxels.len()];
        let mut nodes = Vec::new();
        for (i, &v) in m.voxels.iter().enumerate() {
            if v {
                node[i] = nodes.len();
                nodes.push(i);
            }
        }
        let mut adj: Vec<Vec<SkeletonEdge>> = vec![Vec::new(); nodes.len()];
        let (columns, rows, slices) = (m.columns, m.rows, m.slices);
        for (n, &voxel) in nodes.iter().enumerate() {
            let voxel = voxel as i64;
            let (x, y, z) = (
                voxel % columns,
                voxel / columns % rows,
                voxel / (columns * rows),
            );
            for dz in -1..=1i64 {
                for dy in -1..=1i64 {
                    for dx in -1..=1i64 {
                        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                        if dx == 0 && dy == 0 && dz == 0
                            || nx < 0
                            || ny < 0
                            || nz < 0
                            || nx >= columns
                            || ny >= rows
                            || nz >= slices
                        {
                            continue;
                        }
                        let other = node[m.index(nx, ny, nz)];
                        if other != NONE {
                            adj[n].push(SkeletonEdge {
                                to: other,
                                length: (sq(m.x[nx as usize] - m.x[x as usize])
                                    + sq(m.y[ny as usize] - m.y[y as usize])
                                    + sq(m.z[nz as usize] - m.z[z as usize]))
                                .sqrt(),
                            });
                        }
                    }
                }
            }
        }
        SkeletonGraph {
            mask: m,
            nodes,
            adj,
        }
    }
    fn neighbour_components(&self, n: usize, removed: &[bool]) -> usize {
        let neighbours: Vec<usize> = self.adj[n]
            .iter()
            .filter(|e| !removed[e.to])
            .map(|e| e.to)
            .collect();
        let mut components = 0;
        let mut seen = vec![false; neighbours.len()];
        for start in 0..neighbours.len() {
            if seen[start] {
                continue;
            }
            components += 1;
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(top) = stack.pop() {
                let current = neighbours[top];
                for e in &self.adj[current] {
                    if let Some(i) = neighbours.iter().position(|&v| v == e.to)
                        && !seen[i]
                    {
                        seen[i] = true;
                        stack.push(i);
                    }
                }
            }
        }
        components
    }
    fn degree(&self, n: usize, removed: &[bool]) -> usize {
        self.adj[n].iter().filter(|e| !removed[e.to]).count()
    }
    fn remove_redundant(
        &self,
        ctx: Option<&dyn Cancellation>,
        removed: &mut [bool],
    ) -> Result<(), Error> {
        let mut changed = true;
        while changed {
            ctx_check(ctx)?;
            changed = false;
            for n in 0..self.nodes.len() {
                if !removed[n]
                    && self.degree(n, removed) >= 2
                    && self.neighbour_components(n, removed) == 1
                {
                    removed[n] = true;
                    changed = true;
                }
            }
        }
        Ok(())
    }
    fn spur(&self, n: usize, removed: &[bool], limit: f64) -> Option<Vec<usize>> {
        let mut branch = vec![n];
        let (mut previous, mut current) = (usize::MAX, n);
        let mut length = 0.0f64;
        loop {
            let edge = self.adj[current]
                .iter()
                .find(|e| !removed[e.to] && e.to != previous)?;
            let (next, step) = (edge.to, edge.length);
            length += step;
            if length >= limit {
                return None;
            }
            match self.degree(next, removed) {
                1 => return None,
                2 => {
                    branch.push(next);
                    previous = current;
                    current = next;
                }
                _ => return Some(branch),
            }
        }
    }
    fn nearest(&self, p: [f64; 3]) -> usize {
        let (mut best, mut best_distance) = (0usize, f64::INFINITY);
        let m = self.mask;
        let (columns, rows) = (m.columns as usize, m.rows as usize);
        for (n, &voxel) in self.nodes.iter().enumerate() {
            let (x, y, z) = (
                voxel % columns,
                voxel / columns % rows,
                voxel / (columns * rows),
            );
            let d = sq(m.x[x] - p[0]) + sq(m.y[y] - p[1]) + sq(m.z[z] - p[2]);
            if d < best_distance {
                best = n;
                best_distance = d;
            }
        }
        best
    }
    fn largest_component_node(&self) -> usize {
        let mut label = vec![0usize; self.nodes.len()];
        let (mut best, mut best_size) = (0usize, 0usize);
        let mut next = 0usize;
        for start in 0..self.nodes.len() {
            if label[start] != 0 {
                continue;
            }
            next += 1;
            let mut size = 0usize;
            let mut stack = vec![start];
            label[start] = next;
            while let Some(current) = stack.pop() {
                size += 1;
                for e in &self.adj[current] {
                    if label[e.to] == 0 {
                        label[e.to] = next;
                        stack.push(e.to);
                    }
                }
            }
            if size > best_size {
                best = start;
                best_size = size;
            }
        }
        best
    }
    fn dijkstra(
        &self,
        ctx: Option<&dyn Cancellation>,
        source: usize,
    ) -> Result<(Vec<f64>, Vec<usize>), Error> {
        let mut dist = vec![f64::INFINITY; self.nodes.len()];
        let mut previous = vec![usize::MAX; self.nodes.len()];
        dist[source] = 0.0;
        let mut queue: Vec<SkeletonItem> = vec![SkeletonItem {
            node: source,
            dist: 0.0,
        }];
        let mut pops = 0u64;
        while !queue.is_empty() {
            if pops & 4095 == 0 {
                ctx_check(ctx)?;
            }
            pops += 1;
            let item = heap_pop(&mut queue);
            if item.dist > dist[item.node] {
                continue;
            }
            for e in &self.adj[item.node] {
                let d = item.dist + e.length;
                if d < dist[e.to] {
                    dist[e.to] = d;
                    previous[e.to] = item.node;
                    heap_push(
                        &mut queue,
                        SkeletonItem {
                            node: e.to,
                            dist: d,
                        },
                    );
                }
            }
        }
        Ok((dist, previous))
    }
}
#[derive(Debug, Clone, Copy)]
struct SkeletonItem {
    node: usize,
    dist: f64,
}
fn heap_push(q: &mut Vec<SkeletonItem>, item: SkeletonItem) {
    q.push(item);
    let mut j = q.len() - 1;
    while j > 0 {
        let i = (j - 1) / 2;
        if !(q[j].dist < q[i].dist) {
            break;
        }
        q.swap(i, j);
        j = i;
    }
}
fn heap_pop(q: &mut Vec<SkeletonItem>) -> SkeletonItem {
    let n = q.len() - 1;
    q.swap(0, n);
    let mut i = 0usize;
    loop {
        let j1 = 2 * i + 1;
        if j1 >= n {
            break;
        }
        let mut j = j1;
        let j2 = j1 + 1;
        if j2 < n && q[j2].dist < q[j1].dist {
            j = j2;
        }
        if !(q[j].dist < q[i].dist) {
            break;
        }
        q.swap(i, j);
        i = j;
    }
    q.pop().expect("a queued item")
}
fn farthest(dist: &[f64]) -> usize {
    let (mut best, mut best_distance) = (0usize, -1.0f64);
    for (n, &d) in dist.iter().enumerate() {
        if d != f64::INFINITY && d > best_distance {
            best = n;
            best_distance = d;
        }
    }
    best
}
pub fn prune_skeleton(
    ctx: Option<&dyn Cancellation>,
    skeleton: &VolumeMask,
    min_branch_mm: f64,
) -> Result<VolumeMask, Error> {
    skeleton.validate()?;
    if min_branch_mm < 0.0 || !min_branch_mm.is_finite() {
        return Err(Error::wrap(
            ERR_INVALID_VOLUME_MASK,
            "branch length must be finite and not negative",
        ));
    }
    let g = SkeletonGraph::new(skeleton);
    let mut removed = vec![false; g.nodes.len()];
    loop {
        g.remove_redundant(ctx, &mut removed)?;
        let mut spurs: Vec<Vec<usize>> = Vec::new();
        for n in 0..g.nodes.len() {
            if !removed[n]
                && g.degree(n, &removed) == 1
                && let Some(branch) = g.spur(n, &removed, min_branch_mm)
            {
                spurs.push(branch);
            }
        }
        if spurs.is_empty() {
            break;
        }
        for branch in &spurs {
            for &n in branch {
                removed[n] = true;
            }
        }
    }
    let mut out = skeleton.clone();
    for (n, &voxel) in g.nodes.iter().enumerate() {
        if removed[n] {
            out.voxels[voxel] = false;
        }
    }
    Ok(out)
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SkeletonPathOptions {
    pub from: Option<[f64; 3]>,
    pub to: Option<[f64; 3]>,
}
pub fn skeleton_path(
    ctx: Option<&dyn Cancellation>,
    skeleton: &VolumeMask,
    options: SkeletonPathOptions,
) -> Result<Vec<[i64; 3]>, Error> {
    skeleton.validate()?;
    let g = SkeletonGraph::new(skeleton);
    if g.nodes.is_empty() {
        return Err(Error::wrap(ERR_SKELETON_PATH, "the skeleton is empty"));
    }
    let (start, end) = match (options.from, options.to) {
        (Some(from), Some(to)) => (g.nearest(from), g.nearest(to)),
        (None, None) => {
            let seed = g.largest_component_node();
            let (dist, _) = g.dijkstra(ctx, seed)?;
            let start = farthest(&dist);
            let (dist, _) = g.dijkstra(ctx, start)?;
            (start, farthest(&dist))
        }
        _ => {
            return Err(Error::wrap(
                ERR_INVALID_VOLUME_MASK,
                "set both ends or neither",
            ));
        }
    };
    let (dist, previous) = g.dijkstra(ctx, start)?;
    if dist[end] == f64::INFINITY {
        return Err(Error::wrap(
            ERR_SKELETON_PATH,
            "the ends lie on different skeleton components",
        ));
    }
    let m = skeleton;
    let (columns, rows) = (m.columns as usize, m.rows as usize);
    let mut path: Vec<[i64; 3]> = Vec::new();
    let mut n = end;
    loop {
        let voxel = g.nodes[n];
        path.push([
            (voxel % columns) as i64,
            (voxel / columns % rows) as i64,
            (voxel / (columns * rows)) as i64,
        ]);
        if n == start {
            break;
        }
        n = previous[n];
    }
    path.reverse();
    Ok(path)
}
