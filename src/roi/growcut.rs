use super::growcut_context::{default_grow_cut_limits, validate_grow_cut_dimensions};
use super::roimask::RasterMask;
use crate::fmath;
use std::collections::BTreeMap;
pub(crate) const GROW_CUT_FOREGROUND: u8 = 1;
pub(crate) const GROW_CUT_BACKGROUND: u8 = 2;
pub(crate) type GrowCutValue2D<'a> = &'a dyn Fn(i64, i64) -> Option<f64>;
pub(crate) type GrowCutValue3D<'a> = &'a dyn Fn(i64, i64, i64) -> Option<f64>;
pub fn grow_cut_2d(
    columns: i64,
    rows: i64,
    foreground: Option<&RasterMask>,
    background: Option<&RasterMask>,
    value_at: Option<GrowCutValue2D<'_>>,
) -> RasterMask {
    if validate_grow_cut_dimensions(columns, rows, 1, &default_grow_cut_limits()).is_err() {
        return RasterMask::new(0, 0);
    }
    let Some(value_at) = value_at else {
        return RasterMask::new(columns, rows);
    };
    let seeds = |mask: Option<&RasterMask>| -> BTreeMap<i64, RasterMask> {
        mask.map(|m| BTreeMap::from([(0, m.clone())]))
            .unwrap_or_default()
    };
    let mut result = grow_cut_labels(
        columns,
        rows,
        1,
        &seeds(foreground),
        &seeds(background),
        false,
        Some(&|x, y, _| value_at(x, y)),
    );
    result
        .remove(&0)
        .unwrap_or_else(|| RasterMask::new(columns, rows))
}
pub fn grow_cut_3d(
    columns: i64,
    rows: i64,
    slices: i64,
    foreground: &BTreeMap<i64, RasterMask>,
    background: &BTreeMap<i64, RasterMask>,
    value_at: Option<GrowCutValue3D<'_>>,
) -> BTreeMap<i64, RasterMask> {
    if validate_grow_cut_dimensions(columns, rows, slices, &default_grow_cut_limits()).is_err() {
        return BTreeMap::new();
    }
    grow_cut_labels(
        columns, rows, slices, foreground, background, true, value_at,
    )
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct GrowCutQueueItem {
    pub(crate) index: i64,
    pub(crate) strength: f64,
    pub(crate) label: u8,
    pub(crate) order: i64,
}
#[derive(Debug, Default)]
pub(crate) struct GrowCutPriorityQueue {
    items: Vec<GrowCutQueueItem>,
}
impl GrowCutPriorityQueue {
    pub(crate) fn len(&self) -> i64 {
        self.items.len() as i64
    }
    fn less(&self, i: usize, j: usize) -> bool {
        let (a, b) = (&self.items[i], &self.items[j]);
        if a.strength != b.strength {
            return a.strength > b.strength;
        }
        a.order < b.order
    }
    pub(crate) fn push(&mut self, item: GrowCutQueueItem) {
        self.items.push(item);
        self.up(self.items.len() - 1);
    }
    pub(crate) fn pop(&mut self) -> GrowCutQueueItem {
        let last = self.items.len() - 1;
        self.items.swap(0, last);
        self.down(0, last);
        self.items.pop().expect("a queued item")
    }
    fn up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            if !self.less(index, parent) {
                break;
            }
            self.items.swap(parent, index);
            index = parent;
        }
    }
    fn down(&mut self, mut index: usize, length: usize) {
        loop {
            let left = 2 * index + 1;
            if left >= length {
                break;
            }
            let mut smallest = left;
            let right = left + 1;
            if right < length && self.less(right, left) {
                smallest = right;
            }
            if !self.less(smallest, index) {
                break;
            }
            self.items.swap(index, smallest);
            index = smallest;
        }
    }
}
pub(crate) struct GrowCutField {
    pub(crate) values: Vec<f64>,
    pub(crate) valid: Vec<bool>,
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
}
impl GrowCutField {
    pub(crate) fn new(total: i64) -> GrowCutField {
        GrowCutField {
            values: vec![0.0; total as usize],
            valid: vec![false; total as usize],
            minimum: f64::INFINITY,
            maximum: f64::NEG_INFINITY,
        }
    }
    pub(crate) fn set(&mut self, index: i64, value: f64) {
        self.values[index as usize] = value;
        self.valid[index as usize] = true;
        self.minimum = fmath::min(self.minimum, value);
        self.maximum = fmath::max(self.maximum, value);
    }
    pub(crate) fn contrast(&self) -> f64 {
        let contrast = self.maximum - self.minimum;
        if contrast <= 0.0 || !contrast.is_finite() {
            return 1.0;
        }
        contrast
    }
}
fn grow_cut_labels(
    columns: i64,
    rows: i64,
    slices: i64,
    foreground: &BTreeMap<i64, RasterMask>,
    background: &BTreeMap<i64, RasterMask>,
    include_slice_neighbours: bool,
    value_at: Option<GrowCutValue3D<'_>>,
) -> BTreeMap<i64, RasterMask> {
    let mut result = BTreeMap::new();
    let Some(value_at) = value_at else {
        return result;
    };
    if columns <= 0 || rows <= 0 || slices <= 0 {
        return result;
    }
    let limits = default_grow_cut_limits();
    let Ok((plane, total)) = validate_grow_cut_dimensions(columns, rows, slices, &limits) else {
        return result;
    };
    let mut field = GrowCutField::new(total);
    for z in 0..slices {
        for y in 0..rows {
            for x in 0..columns {
                let Some(value) = value_at(x, y, z).filter(|v| v.is_finite()) else {
                    continue;
                };
                field.set(z * plane + y * columns + x, value);
            }
        }
    }
    if field.minimum == f64::INFINITY {
        return result;
    }
    let contrast = field.contrast();
    let (values, valid) = (&field.values, &field.valid);
    let mut labels = vec![0u8; total as usize];
    let mut strengths = vec![0.0f64; total as usize];
    let mut queue = GrowCutPriorityQueue::default();
    let queue_limit = limits.max_queue_items;
    let mut next_order = 0i64;
    let mut seed = |masks: &BTreeMap<i64, RasterMask>, label: u8| -> bool {
        let mut queue_limit_hit = false;
        for (&z, mask) in masks {
            if z < 0 || z >= slices {
                continue;
            }
            mask.for_each_pixel(|x, y| {
                if queue_limit_hit {
                    return;
                }
                if x < 0 || x >= columns || y < 0 || y >= rows {
                    return;
                }
                let index = z * plane + y * columns + x;
                let at = index as usize;
                if !valid[at] || strengths[at] >= 1.0 {
                    return;
                }
                labels[at] = label;
                strengths[at] = 1.0;
                if queue.len() >= queue_limit {
                    queue_limit_hit = true;
                    return;
                }
                queue.push(GrowCutQueueItem {
                    index,
                    strength: 1.0,
                    label,
                    order: next_order,
                });
                next_order += 1;
            });
        }
        !queue_limit_hit
    };
    if !seed(foreground, GROW_CUT_FOREGROUND) || !seed(background, GROW_CUT_BACKGROUND) {
        return result;
    }
    if queue.len() == 0 {
        return result;
    }
    while queue.len() > 0 {
        let item = queue.pop();
        let at = item.index as usize;
        if labels[at] != item.label || (strengths[at] - item.strength).abs() > 1e-12 {
            continue;
        }
        let z = item.index / plane;
        let remainder = item.index % plane;
        let y = remainder / columns;
        let x = remainder % columns;
        let mut queue_limit_hit = false;
        let mut visit = |nx: i64, ny: i64, nz: i64| {
            if queue_limit_hit {
                return;
            }
            if nx < 0 || nx >= columns || ny < 0 || ny >= rows || nz < 0 || nz >= slices {
                return;
            }
            let neighbour = nz * plane + ny * columns + nx;
            let n = neighbour as usize;
            if !valid[n] || strengths[n] >= 1.0 {
                return;
            }
            let mut affinity = 1.0 - (values[at] - values[n]).abs() / contrast;
            if affinity < 0.0 {
                affinity = 0.0;
            }
            let attack = item.strength * affinity;
            if attack <= strengths[n] + 1e-12 {
                return;
            }
            labels[n] = item.label;
            strengths[n] = attack;
            if queue.len() >= queue_limit {
                queue_limit_hit = true;
                return;
            }
            queue.push(GrowCutQueueItem {
                index: neighbour,
                strength: attack,
                label: item.label,
                order: next_order,
            });
            next_order += 1;
        };
        visit(x - 1, y, z);
        visit(x + 1, y, z);
        visit(x, y - 1, z);
        visit(x, y + 1, z);
        if include_slice_neighbours {
            visit(x, y, z - 1);
            visit(x, y, z + 1);
        }
        if queue_limit_hit {
            return result;
        }
    }
    grow_cut_append_result_runs(&mut result, &labels, columns, rows, slices, plane);
    result
}
pub(crate) fn grow_cut_append_result_runs(
    result: &mut BTreeMap<i64, RasterMask>,
    labels: &[u8],
    columns: i64,
    rows: i64,
    slices: i64,
    plane: i64,
) {
    for z in 0..slices {
        for y in 0..rows {
            let row_start = z * plane + y * columns;
            let label = |x: i64| labels[(row_start + x) as usize];
            let mut x = 0i64;
            while x < columns {
                if label(x) != GROW_CUT_FOREGROUND {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < columns && label(x) == GROW_CUT_FOREGROUND {
                    x += 1;
                }
                result
                    .entry(z)
                    .or_insert_with(|| RasterMask::new(columns, rows))
                    .set_run(y, start, x);
            }
        }
    }
}
