use super::Error;
pub const RASTER_MASK_PIXEL_LIMIT: i64 = 1 << 30;
pub const ERR_INVALID_RASTER_MASK_DIMENSIONS: &str = "roi: invalid raster mask dimensions";
pub const ERR_RASTER_MASK_DIMENSION_OVERFLOW: &str = "roi: raster mask dimensions overflow";
pub const ERR_RASTER_MASK_TOO_LARGE: &str = "roi: raster mask exceeds pixel limit";
pub const ERR_RASTER_MASK_BUFFER_LENGTH: &str = "roi: raster mask buffer length mismatch";
pub fn raster_mask_pixel_count(columns: i64, rows: i64) -> Result<i64, Error> {
    if columns <= 0 || rows <= 0 {
        return Err(Error::sentinel(ERR_INVALID_RASTER_MASK_DIMENSIONS));
    }
    if columns > i64::MAX / rows {
        return Err(Error::sentinel(ERR_RASTER_MASK_DIMENSION_OVERFLOW));
    }
    let pixels = columns * rows;
    if pixels > RASTER_MASK_PIXEL_LIMIT {
        return Err(Error::sentinel(ERR_RASTER_MASK_TOO_LARGE));
    }
    Ok(pixels)
}
pub fn validate_raster_mask_buffer(columns: i64, rows: i64, buffer: &[u8]) -> Result<(), Error> {
    let pixels = raster_mask_pixel_count(columns, rows)?;
    if buffer.len() as i64 != pixels {
        return Err(Error::wrap(
            ERR_RASTER_MASK_BUFFER_LENGTH,
            format_args!("got {} bytes, want {}", buffer.len(), pixels),
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MaskRun {
    pub start: i64,
    pub end: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RasterMask {
    pub columns: i64,
    pub rows: i64,
    pub(crate) runs: Vec<Vec<MaskRun>>,
}
impl RasterMask {
    pub fn new(columns: i64, rows: i64) -> RasterMask {
        let columns = columns.max(0);
        let rows = rows.max(0);
        RasterMask {
            columns,
            rows,
            runs: vec![Vec::new(); rows as usize],
        }
    }
    pub fn from_buffer(columns: i64, rows: i64, buffer: &[u8]) -> Result<RasterMask, Error> {
        validate_raster_mask_buffer(columns, rows, buffer)?;
        let mut mask = RasterMask::new(columns, rows);
        let width = columns as usize;
        for y in 0..rows as usize {
            let row = &buffer[y * width..(y + 1) * width];
            let mut x = 0;
            while x < width {
                if row[x] == 0 {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < width && row[x] != 0 {
                    x += 1;
                }
                mask.set_run(y as i64, start as i64, x as i64);
            }
        }
        Ok(mask)
    }
    pub fn from_bytes(columns: i64, rows: i64, buffer: &[u8]) -> Result<RasterMask, Error> {
        RasterMask::from_buffer(columns, rows, buffer)
    }
    pub(crate) fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.columns && y < self.rows
    }
    pub(crate) fn stored_rows(&self) -> usize {
        (self.rows.max(0) as usize).min(self.runs.len())
    }
    pub fn get(&self, x: i64, y: i64) -> bool {
        if !self.in_bounds(x, y) {
            return false;
        }
        for r in &self.runs[y as usize] {
            if x < r.start {
                return false;
            }
            if x < r.end {
                return true;
            }
        }
        false
    }
    pub fn set(&mut self, x: i64, y: i64, on: bool) {
        if !self.in_bounds(x, y) {
            return;
        }
        if on {
            self.set_run(y, x, x + 1);
        } else {
            self.clear_run(y, x, x + 1);
        }
    }
    pub fn set_run(&mut self, y: i64, x0: i64, x1: i64) {
        if y < 0 || y >= self.rows {
            return;
        }
        let (x0, x1) = clamp_span(x0, x1, self.columns);
        if x0 >= x1 {
            return;
        }
        let row = &mut self.runs[y as usize];
        *row = add_run(row, x0, x1);
    }
    pub fn clear_run(&mut self, y: i64, x0: i64, x1: i64) {
        if y < 0 || y >= self.rows {
            return;
        }
        let (x0, x1) = clamp_span(x0, x1, self.columns);
        if x0 >= x1 {
            return;
        }
        let row = &mut self.runs[y as usize];
        *row = remove_run(row, x0, x1);
    }
    pub fn runs(&self, y: i64) -> &[MaskRun] {
        if y < 0 || y >= self.rows {
            return &[];
        }
        &self.runs[y as usize]
    }
    pub fn for_each_run(&self, mut f: impl FnMut(i64, MaskRun)) {
        for (y, rs) in self.runs.iter().enumerate() {
            for r in rs {
                f(y as i64, *r);
            }
        }
    }
    pub fn for_each_pixel(&self, mut f: impl FnMut(i64, i64)) {
        for (y, rs) in self.runs.iter().enumerate() {
            for r in rs {
                for x in r.start..r.end {
                    f(x, y as i64);
                }
            }
        }
    }
    pub fn count(&self) -> i64 {
        let mut n = 0i64;
        for rs in &self.runs {
            for r in rs {
                n += r.end - r.start;
            }
        }
        n
    }
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
    pub fn union(&mut self, other: &RasterMask) {
        let rows = self.stored_rows().min(other.stored_rows());
        for y in 0..rows {
            if other.runs[y].is_empty() {
                continue;
            }
            self.runs[y] = union_run_lists(&self.runs[y], &other.runs[y], self.columns);
        }
    }
    pub fn intersect(&mut self, other: &RasterMask) {
        if self.columns != other.columns || self.rows != other.rows {
            self.clear();
            return;
        }
        for y in 0..self.runs.len() {
            let (left, right) = (&self.runs[y], &other.runs[y]);
            let mut out = Vec::with_capacity(left.len().min(right.len()));
            let (mut i, mut j) = (0, 0);
            while i < left.len() && j < right.len() {
                let start = left[i].start.max(right[j].start);
                let end = left[i].end.min(right[j].end);
                if start < end {
                    out.push(MaskRun { start, end });
                }
                if left[i].end < right[j].end {
                    i += 1;
                } else {
                    j += 1;
                }
            }
            self.runs[y] = out;
        }
    }
    pub fn clear(&mut self) {
        for row in self.runs.iter_mut() {
            row.clear();
        }
    }
    pub fn subtract(&mut self, other: &RasterMask) {
        if self.columns != other.columns || self.rows != other.rows {
            return;
        }
        for y in 0..self.runs.len() {
            for run in &other.runs[y] {
                self.runs[y] = remove_run(&self.runs[y], run.start, run.end);
            }
        }
    }
    pub fn xor(&mut self, other: &RasterMask) {
        let rows = self.stored_rows().min(other.stored_rows());
        for y in 0..rows {
            let mut other_runs: Vec<MaskRun> = Vec::with_capacity(other.runs[y].len());
            for run in &other.runs[y] {
                let (x0, x1) = clamp_span(run.start, run.end, self.columns);
                if x0 < x1 {
                    other_runs = add_run(&other_runs, x0, x1);
                }
            }
            self.runs[y] = xor_runs(&self.runs[y], &other_runs);
        }
    }
}
pub(crate) fn clamp_span(x0: i64, x1: i64, columns: i64) -> (i64, i64) {
    (x0.max(0), x1.min(columns))
}
pub(crate) fn add_run(runs: &[MaskRun], x0: i64, x1: i64) -> Vec<MaskRun> {
    let mut out = Vec::with_capacity(runs.len() + 1);
    let mut i = 0;
    while i < runs.len() && runs[i].end < x0 {
        out.push(runs[i]);
        i += 1;
    }
    let (mut ns, mut ne) = (x0, x1);
    while i < runs.len() && runs[i].start <= x1 {
        ns = ns.min(runs[i].start);
        ne = ne.max(runs[i].end);
        i += 1;
    }
    out.push(MaskRun { start: ns, end: ne });
    out.extend_from_slice(&runs[i..]);
    out
}
pub(crate) fn union_run_lists(first: &[MaskRun], second: &[MaskRun], columns: i64) -> Vec<MaskRun> {
    let mut out: Vec<MaskRun> = Vec::with_capacity(first.len() + second.len());
    let (mut fi, mut si) = (0, 0);
    while fi < first.len() || si < second.len() {
        let run = if si >= second.len() || (fi < first.len() && first[fi].start <= second[si].start)
        {
            fi += 1;
            first[fi - 1]
        } else {
            si += 1;
            second[si - 1]
        };
        let (x0, x1) = clamp_span(run.start, run.end, columns);
        if x0 >= x1 {
            continue;
        }
        if let Some(last) = out.last_mut()
            && x0 <= last.end
        {
            if x1 > last.end {
                last.end = x1;
            }
            continue;
        }
        out.push(MaskRun { start: x0, end: x1 });
    }
    out
}
#[allow(dead_code)]
pub(crate) fn intersect_run_lists(first: &[MaskRun], second: &[MaskRun]) -> Vec<MaskRun> {
    let mut out = Vec::with_capacity(first.len().min(second.len()));
    let (mut fi, mut si) = (0, 0);
    while fi < first.len() && si < second.len() {
        let start = first[fi].start.max(second[si].start);
        let end = first[fi].end.min(second[si].end);
        if start < end {
            out.push(MaskRun { start, end });
        }
        if first[fi].end < second[si].end {
            fi += 1;
        } else {
            si += 1;
        }
    }
    out
}
pub(crate) fn remove_run(runs: &[MaskRun], x0: i64, x1: i64) -> Vec<MaskRun> {
    let mut out = Vec::with_capacity(runs.len() + 1);
    for r in runs {
        if r.end <= x0 || r.start >= x1 {
            out.push(*r);
            continue;
        }
        if r.start < x0 {
            out.push(MaskRun {
                start: r.start,
                end: x0,
            });
        }
        if r.end > x1 {
            out.push(MaskRun {
                start: x1,
                end: r.end,
            });
        }
    }
    out
}
pub(crate) fn xor_runs(a: &[MaskRun], b: &[MaskRun]) -> Vec<MaskRun> {
    let (mut ia, mut ib) = (0, 0);
    let (mut in_a, mut in_b) = (false, false);
    let mut start = 0;
    let mut out = Vec::with_capacity(a.len() + b.len());
    loop {
        let next_a = match a.get(ia) {
            Some(r) if in_a => r.end,
            Some(r) => r.start,
            None => i64::MAX,
        };
        let next_b = match b.get(ib) {
            Some(r) if in_b => r.end,
            Some(r) => r.start,
            None => i64::MAX,
        };
        let x = next_a.min(next_b);
        if x == i64::MAX {
            break;
        }
        let was_set = in_a != in_b;
        if next_a == x {
            if in_a {
                in_a = false;
                ia += 1;
            } else {
                in_a = true;
            }
        }
        if next_b == x {
            if in_b {
                in_b = false;
                ib += 1;
            } else {
                in_b = true;
            }
        }
        let is_set = in_a != in_b;
        if !was_set && is_set {
            start = x;
        }
        if was_set && !is_set {
            out.push(MaskRun { start, end: x });
        }
    }
    out
}
