use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use std::f64::consts::PI;
pub const ERR_LIVEWIRE: &str = "roi: invalid livewire request";
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LivewireOptions {
    pub image_weight: f64,
    pub length_weight: f64,
    pub curvature_weight: f64,
    pub col_spacing: f64,
    pub row_spacing: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LivewireCostMap {
    pub columns: i64,
    pub rows: i64,
    pub(crate) cost: Vec<f64>,
    pub(crate) opts: LivewireOptions,
    pub(crate) max_bytes: i64,
}
pub(crate) type LivewireValue<'a> = &'a dyn Fn(i64, i64) -> Option<f64>;
pub fn new_livewire_cost_map(
    columns: i64,
    rows: i64,
    value: Option<LivewireValue<'_>>,
    mut opts: LivewireOptions,
) -> Result<LivewireCostMap, Error> {
    let Some(value) = value.filter(|_| columns > 0 && rows > 0) else {
        return Err(Error::wrap(ERR_LIVEWIRE, format!("{columns}x{rows} slice")));
    };
    if opts.image_weight == 0.0 {
        opts.image_weight = 1.0;
    }
    if opts.length_weight == 0.0 {
        opts.length_weight = 0.1;
    }
    if opts.col_spacing <= 0.0 {
        opts.col_spacing = 1.0;
    }
    if opts.row_spacing <= 0.0 {
        opts.row_spacing = 1.0;
    }
    let n = (columns * rows) as usize;
    let mut values = vec![0.0f64; n];
    let mut valid = vec![false; n];
    for y in 0..rows {
        for x in 0..columns {
            if let Some(v) = value(x, y)
                && v.is_finite()
            {
                let i = (y * columns + x) as usize;
                values[i] = v;
                valid[i] = true;
            }
        }
    }
    let mut magnitude = vec![0.0f64; n];
    let mut largest = 0.0;
    let at = |x: i64, y: i64| -> Option<f64> {
        if x < 0 || y < 0 || x >= columns || y >= rows || !valid[(y * columns + x) as usize] {
            return None;
        }
        Some(values[(y * columns + x) as usize])
    };
    let derivative = |x: i64, y: i64, dx: i64, dy: i64, spacing: f64| -> f64 {
        let center = at(x, y).unwrap_or(0.0);
        match (at(x + dx, y + dy), at(x - dx, y - dy)) {
            (Some(plus), Some(minus)) => (plus - minus) / (2.0 * spacing),
            (Some(plus), None) => (plus - center) / spacing,
            (None, Some(minus)) => (center - minus) / spacing,
            (None, None) => 0.0,
        }
    };
    for y in 0..rows {
        for x in 0..columns {
            let i = (y * columns + x) as usize;
            if !valid[i] {
                continue;
            }
            let gx = derivative(x, y, 1, 0, opts.col_spacing);
            let gy = derivative(x, y, 0, 1, opts.row_spacing);
            let m = fmath::hypot(gx, gy);
            magnitude[i] = m;
            largest = fmath::max(largest, m);
        }
    }
    let cost = (0..n)
        .map(|i| {
            if !valid[i] {
                f64::INFINITY
            } else if largest > 0.0 {
                1.0 - magnitude[i] / largest
            } else {
                1.0
            }
        })
        .collect();
    Ok(LivewireCostMap {
        columns,
        rows,
        cost,
        opts,
        max_bytes: 0,
    })
}
const LIVEWIRE_STEPS: [[i64; 2]; 8] = [
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
    [-1, 0],
    [-1, -1],
    [0, -1],
    [1, -1],
];
#[derive(Debug, Clone)]
pub struct LivewireTree {
    columns: i64,
    rows: i64,
    anchor: [i64; 2],
    parent: Vec<i32>,
    distance: Vec<f64>,
    states: i64,
}
#[derive(Clone, Copy)]
struct LivewireItem {
    state: i32,
    distance: f64,
}
#[derive(Default)]
struct LivewireQueue {
    items: Vec<LivewireItem>,
}
impl LivewireQueue {
    fn less(&self, i: usize, j: usize) -> bool {
        self.items[i].distance < self.items[j].distance
    }
    fn push(&mut self, item: LivewireItem) {
        self.items.push(item);
        let mut j = self.items.len() - 1;
        while j > 0 {
            let i = (j - 1) / 2;
            if !self.less(j, i) {
                break;
            }
            self.items.swap(i, j);
            j = i;
        }
    }
    fn pop(&mut self) -> Option<LivewireItem> {
        let n = self.items.len().checked_sub(1)?;
        self.items.swap(0, n);
        let mut i = 0;
        loop {
            let j1 = 2 * i + 1;
            if j1 >= n {
                break;
            }
            let mut j = j1;
            let j2 = j1 + 1;
            if j2 < n && self.less(j2, j1) {
                j = j2;
            }
            if !self.less(j, i) {
                break;
            }
            self.items.swap(i, j);
            i = j;
        }
        self.items.pop()
    }
}
fn pair_v(p: [i64; 2]) -> String {
    format!("[{} {}]", p[0], p[1])
}
impl LivewireCostMap {
    pub fn cost(&self, x: i64, y: i64) -> f64 {
        if x < 0 || y < 0 || x >= self.columns || y >= self.rows {
            return f64::INFINITY;
        }
        self.cost[(y * self.columns + x) as usize]
    }
    pub fn tree(
        &self,
        ctx: Option<&dyn Cancellation>,
        anchor: [i64; 2],
    ) -> Result<LivewireTree, Error> {
        if anchor[0] < 0
            || anchor[1] < 0
            || anchor[0] >= self.columns
            || anchor[1] >= self.rows
            || self.cost(anchor[0], anchor[1]) == f64::INFINITY
        {
            return Err(Error::wrap(
                ERR_LIVEWIRE,
                format!("anchor {}", pair_v(anchor)),
            ));
        }
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        let twice = |first: Error| ctx_err(ctx).unwrap_or(first);
        let pixels = self.columns * self.rows;
        let directions: i64 = if self.opts.curvature_weight > 0.0 {
            8
        } else {
            1
        };
        if self.max_bytes > 0 && pixels > (16 << 20) / directions {
            return Err(Error::wrap(ERR_LIVEWIRE, "livewire state ceiling"));
        }
        let states = pixels * directions;
        let mut queue_limit = 0i64;
        if self.max_bytes > 0 {
            queue_limit = self.canny_tree_admission(states, directions)?;
        }
        let mut t = LivewireTree {
            columns: self.columns,
            rows: self.rows,
            anchor,
            parent: vec![0; states as usize],
            distance: vec![0.0; states as usize],
            states: directions,
        };
        for i in 0..states as usize {
            if i % 4096 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(twice(err));
            }
            t.distance[i] = f64::INFINITY;
            t.parent[i] = -1;
        }
        let mut steps = [0.0f64; 8];
        for (k, s) in LIVEWIRE_STEPS.iter().enumerate() {
            steps[k] = fmath::hypot(
                s[0] as f64 * self.opts.col_spacing,
                s[1] as f64 * self.opts.row_spacing,
            );
        }
        let start = ((anchor[1] * self.columns + anchor[0]) * directions) as i32;
        let mut queue = LivewireQueue::default();
        let push = |queue: &mut LivewireQueue, item: LivewireItem| -> Result<(), Error> {
            if queue_limit > 0 && queue.items.len() as i64 >= queue_limit {
                return Err(Error::wrap(ERR_LIVEWIRE, "livewire queue"));
            }
            queue.push(item);
            Ok(())
        };
        for d in 0..directions {
            t.distance[(i64::from(start) + d) as usize] = 0.0;
            push(
                &mut queue,
                LivewireItem {
                    state: start.wrapping_add(d as i32),
                    distance: 0.0,
                },
            )?;
        }
        let mut done = vec![false; states as usize];
        let mut settled = 0i64;
        let mut pops = 0i64;
        while !queue.items.is_empty() {
            if pops % 4096 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(twice(err));
            }
            pops += 1;
            let Some(item) = queue.pop() else {
                break;
            };
            let state = i64::from(item.state);
            if done[state as usize] {
                continue;
            }
            done[state as usize] = true;
            settled += 1;
            if settled % 4096 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(err);
            }
            let (pixel, incoming) = (state / directions, state % directions);
            let (x, y) = (pixel % self.columns, pixel / self.columns);
            for (k, s) in LIVEWIRE_STEPS.iter().enumerate() {
                let (nx, ny) = (x + s[0], y + s[1]);
                if nx < 0 || ny < 0 || nx >= self.columns || ny >= self.rows {
                    continue;
                }
                let image = self.cost[(ny * self.columns + nx) as usize];
                if image == f64::INFINITY {
                    continue;
                }
                let mut step = self.opts.image_weight * image;
                if self.opts.length_weight > 0.0 {
                    step += self.opts.length_weight * steps[k];
                }
                let mut next = (ny * self.columns + nx) * directions;
                if directions == 8 {
                    next += k as i64;
                    if t.parent[state as usize] >= 0 {
                        let mut turn = ((k as i64 - incoming) as f64).abs();
                        if turn > 4.0 {
                            turn = 8.0 - turn;
                        }
                        step += self.opts.curvature_weight * (1.0 - fmath::cos(turn * PI / 4.0));
                    }
                }
                if done[next as usize] {
                    continue;
                }
                let d = item.distance + step;
                if self.max_bytes > 0 && !d.is_finite() {
                    return Err(Error::wrap(ERR_LIVEWIRE, "path distance overflow"));
                }
                if d < t.distance[next as usize] {
                    t.distance[next as usize] = d;
                    t.parent[next as usize] = state as i32;
                    push(
                        &mut queue,
                        LivewireItem {
                            state: next as i32,
                            distance: d,
                        },
                    )?;
                }
            }
        }
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        Ok(t)
    }
    pub fn livewire_path(
        &self,
        ctx: Option<&dyn Cancellation>,
        from: [i64; 2],
        to: [i64; 2],
    ) -> Result<Vec<[f64; 2]>, Error> {
        let tree = self.tree(ctx, from)?;
        tree.path_to(to).ok_or_else(|| {
            Error::wrap(
                ERR_LIVEWIRE,
                format!("{} is unreachable from {}", pair_v(to), pair_v(from)),
            )
        })
    }
}
impl LivewireTree {
    pub fn anchor(&self) -> [i64; 2] {
        self.anchor
    }
    pub fn size(&self) -> (i64, i64) {
        (self.columns, self.rows)
    }
    fn contains(&self, p: [i64; 2]) -> bool {
        p[0] >= 0 && p[1] >= 0 && p[0] < self.columns && p[1] < self.rows
    }
    pub fn path_to(&self, target: [i64; 2]) -> Option<Vec<[f64; 2]>> {
        if !self.contains(target) {
            return None;
        }
        let pixel = target[1] * self.columns + target[0];
        let mut best = f64::INFINITY;
        let mut best_state: Option<i64> = None;
        for d in 0..self.states {
            let s = pixel * self.states + d;
            if self.distance[s as usize] < best {
                best = self.distance[s as usize];
                best_state = Some(s);
            }
        }
        let mut s = best_state?;
        let mut path = Vec::new();
        loop {
            let p = s / self.states;
            path.push([(p % self.columns) as f64, (p / self.columns) as f64]);
            let parent = self.parent[s as usize];
            if parent < 0 {
                break;
            }
            s = i64::from(parent);
        }
        path.reverse();
        Some(path)
    }
    pub fn distance(&self, target: [i64; 2]) -> f64 {
        if !self.contains(target) {
            return f64::INFINITY;
        }
        let mut best = f64::INFINITY;
        let pixel = target[1] * self.columns + target[0];
        for d in 0..self.states {
            best = fmath::min(best, self.distance[(pixel * self.states + d) as usize]);
        }
        best
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn owned<T: Send + Sync + 'static>(_: &T) {}
    #[test]
    fn tree_owns_its_data() {
        let value = |x: i64, y: i64| Some(if x == 4 { 100.0 } else { (x + y) as f64 });
        let make = || new_livewire_cost_map(9, 7, Some(&value), LivewireOptions::default());
        let Ok(costs) = make() else {
            unreachable!("valid map")
        };
        let Ok(kept) = costs.tree(None, [1, 1]) else {
            unreachable!("valid anchor")
        };
        let live = costs.livewire_path(None, [1, 1], [7, 5]);
        drop(costs);
        owned(&kept);
        assert_eq!(kept.size(), (9, 7));
        assert_eq!(kept.anchor(), [1, 1]);
        assert_eq!(live.ok(), kept.path_to([7, 5]));
        assert!(kept.path_to([9, 0]).is_none());
        assert_eq!(kept.distance([-1, 0]), f64::INFINITY);
    }
}
