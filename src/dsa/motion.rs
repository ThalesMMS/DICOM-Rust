use super::{Error, ErrorKind, Shift, finite, frame_size_matches};
use crate::fmath;
use crate::optimize::{self, Func, Powell};
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rigid {
    pub angle: f64,
    pub tx: f64,
    pub ty: f64,
    pub center: [f64; 2],
}
fn sincos(x: f64) -> (f64, f64) {
    fmath::sincos(x)
}
impl Rigid {
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, c) = sincos(self.angle);
        let (dx, dy) = (x - self.center[0], y - self.center[1]);
        (
            c * dx - s * dy + self.center[0] + self.tx,
            s * dx + c * dy + self.center[1] + self.ty,
        )
    }
    fn is_finite(&self) -> bool {
        finite(self.angle)
            && finite(self.tx)
            && finite(self.ty)
            && finite(self.center[0])
            && finite(self.center[1])
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct RigidOptions {
    pub initial: Shift,
    pub max_samples: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RigidRegistration {
    pub rigid: Rigid,
    pub similarity: f64,
    pub evaluations: i64,
}
pub fn register_rigid(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    rows: i64,
    opts: RigidOptions,
) -> Result<RigidRegistration, Error> {
    if columns < 16 || rows < 16 || !frame_size_matches(columns, rows, &[reference, moving]) {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!("need two {columns}x{rows} frames of at least 16x16"),
        ));
    }
    if !opts.initial.is_finite() {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!("initial shift: {}", ErrorKind::InvalidPixelShift.text()),
        )
        .and(ErrorKind::InvalidPixelShift));
    }
    let mut max_samples = opts.max_samples;
    if max_samples <= 0 {
        max_samples = 256;
    }
    let factor = 1.max((columns.max(rows) + max_samples - 1) / max_samples);
    let centre = [(columns - 1) as f64 / 2.0, (rows - 1) as f64 / 2.0];
    let (dx, dy) = opts.initial.displacement();
    let mut params = vec![0.0, -dx, -dy];
    let mut result = RigidRegistration::default();
    for level in [factor * 2, factor] {
        let (reference_level, cols, rws) = downsample(reference, columns, rows, level);
        let (moving_level, _, _) = downsample(moving, columns, rows, level);
        if cols < 8 || rws < 8 {
            continue;
        }
        let scale = level as f64;
        let level_centre = [
            (centre[0] - (scale - 1.0) / 2.0) / scale,
            (centre[1] - (scale - 1.0) / 2.0) / scale,
        ];
        let metric = GradientDifference::new(&reference_level, cols, rws);
        let radius = fmath::hypot(cols as f64, rws as f64) / 2.0;
        let mut objective = Func(|x: &[f64]| -> Result<f64, optimize::Error> {
            let rigid = Rigid {
                angle: x[0],
                tx: x[1] / scale,
                ty: x[2] / scale,
                center: level_centre,
            };
            if !rigid.is_finite() {
                return Ok(f64::INFINITY);
            }
            match metric.value(&warp_rigid(&moving_level, cols, rws, rigid)) {
                Some(value) => Ok(-value),
                None => Ok(f64::INFINITY),
            }
        });
        let out = Powell {
            step_length: 2.0,
            step_tolerance: 1e-3,
            value_tolerance: 1e-7,
            maximum_iterations: 60,
            scales: Some(vec![radius, scale, scale]),
            ..Powell::default()
        }
        .minimize(&mut objective, &params)
        .map_err(|e| Error::with_message(ErrorKind::Optimize, e.to_string()))?;
        params = out.x;
        result.similarity = -out.value;
        result.evaluations += out.evaluations;
    }
    if result.similarity.is_infinite() || result.similarity.is_nan() {
        return Err(Error::wrap(ErrorKind::Motion, "the frames do not overlap"));
    }
    result.rigid = Rigid {
        angle: params[0],
        tx: params[1],
        ty: params[2],
        center: centre,
    };
    Ok(result)
}
pub fn apply_rigid(mask: &[f64], columns: i64, rows: i64, r: Rigid) -> Result<Vec<f64>, Error> {
    if !frame_size_matches(columns, rows, &[mask]) {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!("{} samples for {columns}x{rows}", mask.len()),
        ));
    }
    if !r.is_finite() {
        return Err(Error::wrap(
            ErrorKind::Motion,
            "rigid transform contains a non-finite parameter",
        ));
    }
    Ok(warp_rigid(mask, columns, rows, r))
}
fn warp_rigid(mask: &[f64], columns: i64, rows: i64, r: Rigid) -> Vec<f64> {
    let mut out = vec![0.0f64; mask.len()];
    let (s, c) = sincos(r.angle);
    for y in 0..rows {
        let dy = y as f64 - r.center[1];
        for x in 0..columns {
            let dx = x as f64 - r.center[0];
            out[(y * columns + x) as usize] = bilinear(
                mask,
                columns,
                rows,
                c * dx - s * dy + r.center[0] + r.tx,
                s * dx + c * dy + r.center[1] + r.ty,
            );
        }
    }
    out
}
struct GradientDifference {
    columns: i64,
    rows: i64,
    gx: Vec<f64>,
    gy: Vec<f64>,
    vx: f64,
    vy: f64,
}
impl GradientDifference {
    fn new(reference: &[f64], columns: i64, rows: i64) -> GradientDifference {
        let (gx, gy) = sobel(reference, columns, rows);
        let (vx, vy) = (variance(&gx, columns, rows), variance(&gy, columns, rows));
        GradientDifference {
            columns,
            rows,
            gx,
            gy,
            vx,
            vy,
        }
    }
    fn value(&self, moved: &[f64]) -> Option<f64> {
        if self.vx <= 0.0 && self.vy <= 0.0 {
            return None;
        }
        let (mx, my) = sobel(moved, self.columns, self.rows);
        let mut sum = 0.0f64;
        let mut n = 0i64;
        for y in 1..self.rows - 1 {
            for x in 1..self.columns - 1 {
                let i = (y * self.columns + x) as usize;
                if self.vx > 0.0 {
                    let d = self.gx[i] - mx[i];
                    sum += self.vx / (self.vx + d * d);
                    n += 1;
                }
                if self.vy > 0.0 {
                    let d = self.gy[i] - my[i];
                    sum += self.vy / (self.vy + d * d);
                    n += 1;
                }
            }
        }
        if n > 0 { Some(sum / n as f64) } else { None }
    }
}
pub fn gradient_difference(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    rows: i64,
) -> Result<f64, Error> {
    if columns < 3 || rows < 3 || !frame_size_matches(columns, rows, &[reference, moving]) {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!("need two {columns}x{rows} frames"),
        ));
    }
    GradientDifference::new(reference, columns, rows)
        .value(moving)
        .ok_or_else(|| Error::wrap(ErrorKind::Motion, "the reference frame is uniform"))
}
fn sobel(values: &[f64], columns: i64, rows: i64) -> (Vec<f64>, Vec<f64>) {
    let mut gx = vec![0.0f64; values.len()];
    let mut gy = vec![0.0f64; values.len()];
    for y in 1..rows - 1 {
        for x in 1..columns - 1 {
            let at = |dx: i64, dy: i64| -> f64 { values[((y + dy) * columns + x + dx) as usize] };
            let i = (y * columns + x) as usize;
            gx[i] =
                (at(1, -1) + 2.0 * at(1, 0) + at(1, 1) - at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1))
                    / 8.0;
            gy[i] =
                (at(-1, 1) + 2.0 * at(0, 1) + at(1, 1) - at(-1, -1) - 2.0 * at(0, -1) - at(1, -1))
                    / 8.0;
        }
    }
    (gx, gy)
}
fn variance(values: &[f64], columns: i64, rows: i64) -> f64 {
    let (mut sum, mut squares) = (0.0f64, 0.0f64);
    let mut n = 0i64;
    for y in 1..rows - 1 {
        for x in 1..columns - 1 {
            let v = values[(y * columns + x) as usize];
            sum += v;
            squares += v * v;
            n += 1;
        }
    }
    if n == 0 {
        return 0.0;
    }
    let mean = sum / n as f64;
    squares / n as f64 - mean * mean
}
fn downsample(values: &[f64], columns: i64, rows: i64, factor: i64) -> (Vec<f64>, i64, i64) {
    if factor <= 1 {
        return (values.to_vec(), columns, rows);
    }
    let (cols, rws) = (columns / factor, rows / factor);
    let mut out = vec![0.0f64; (cols * rws) as usize];
    for y in 0..rws {
        for x in 0..cols {
            let mut sum = 0.0f64;
            for j in 0..factor {
                let row = (y * factor + j) * columns;
                for i in 0..factor {
                    sum += values[(row + x * factor + i) as usize];
                }
            }
            out[(y * cols + x) as usize] = sum / (factor * factor) as f64;
        }
    }
    (out, cols, rws)
}
fn bilinear(values: &[f64], columns: i64, rows: i64, x: f64, y: f64) -> f64 {
    let x = fmath::max(0.0, fmath::min((columns - 1) as f64, x));
    let y = fmath::max(0.0, fmath::min((rows - 1) as f64, y));
    let (x0, y0) = (fmath::f64_to_i64(x), fmath::f64_to_i64(y));
    let (x1, y1) = ((x0 + 1).min(columns - 1), (y0 + 1).min(rows - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let at = |xx: i64, yy: i64| values[(yy * columns + xx) as usize];
    let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}
#[derive(Debug, Clone, Copy, Default)]
pub struct BlockOptions {
    pub block_size: i64,
    pub search_radius: i64,
    pub minimum_correlation: f64,
    pub max_samples: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BlockMatch {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub correlation: f64,
    pub accepted: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplacementField {
    pub columns: i64,
    pub rows: i64,
    pub dx: Vec<f64>,
    pub dy: Vec<f64>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockMatchResult {
    pub field: DisplacementField,
    pub blocks: Vec<BlockMatch>,
}
pub fn match_blocks(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    rows: i64,
    opts: BlockOptions,
) -> Result<BlockMatchResult, Error> {
    if !frame_size_matches(columns, rows, &[reference, moving]) {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!(
                "{} and {} samples for {columns}x{rows}",
                reference.len(),
                moving.len()
            ),
        ));
    }
    let mut max_samples = opts.max_samples;
    if max_samples <= 0 {
        max_samples = 512;
    }
    let factor = 1.max((columns.max(rows) + max_samples - 1) / max_samples);
    if factor == 1 {
        return match_blocks_at(reference, moving, columns, rows, opts);
    }
    let (reference_level, cols, rws) = downsample(reference, columns, rows, factor);
    let (moving_level, _, _) = downsample(moving, columns, rows, factor);
    let mut coarse = match_blocks_at(&reference_level, &moving_level, cols, rws, opts)?;
    let scale = factor as f64;
    let offset = (scale - 1.0) / 2.0;
    for block in &mut coarse.blocks {
        block.x = block.x * scale + offset;
        block.y = block.y * scale + offset;
        block.dx *= scale;
        block.dy *= scale;
    }
    let size = (columns * rows) as usize;
    let mut field = DisplacementField {
        columns,
        rows,
        dx: vec![0.0; size],
        dy: vec![0.0; size],
    };
    for y in 0..rows {
        let v = (y as f64 - offset) / scale;
        for x in 0..columns {
            let u = (x as f64 - offset) / scale;
            let i = (y * columns + x) as usize;
            field.dx[i] = scale * bilinear(&coarse.field.dx, cols, rws, u, v);
            field.dy[i] = scale * bilinear(&coarse.field.dy, cols, rws, u, v);
        }
    }
    Ok(BlockMatchResult {
        field,
        blocks: coarse.blocks,
    })
}
fn match_blocks_at(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    rows: i64,
    opts: BlockOptions,
) -> Result<BlockMatchResult, Error> {
    let mut size = opts.block_size;
    if size <= 0 {
        size = 32;
    }
    let mut radius = opts.search_radius;
    if radius <= 0 {
        radius = 8;
    }
    let mut minimum = opts.minimum_correlation;
    if minimum == 0.0 {
        minimum = 0.6;
    }
    let short_side = columns.min(rows);
    if size > short_side || radius > (short_side - size) / 2 {
        return Err(Error::wrap(
            ErrorKind::Motion,
            format!(
                "{columns}x{rows} frames are too small for {size}-pixel blocks searched ±{radius}"
            ),
        ));
    }
    let step = 1.max(size / 2);
    let half = size / 2;
    let mut xs: Vec<i64> = Vec::new();
    let mut ys: Vec<i64> = Vec::new();
    let mut x = half + radius;
    while x + half + radius <= columns {
        xs.push(x);
        x += step;
    }
    let mut y = half + radius;
    while y + half + radius <= rows {
        ys.push(y);
        y += step;
    }
    let mut blocks = vec![BlockMatch::default(); xs.len() * ys.len()];
    for (j, &cy) in ys.iter().enumerate() {
        for (i, &cx) in xs.iter().enumerate() {
            blocks[j * xs.len() + i] =
                match_block(reference, moving, columns, cx, cy, half, radius, minimum);
        }
    }
    reject_weak_blocks(&mut blocks);
    reject_outliers(&mut blocks, xs.len() as i64, ys.len() as i64);
    let field = spread_blocks(&blocks, &xs, &ys, columns, rows, step as f64);
    Ok(BlockMatchResult { field, blocks })
}
fn match_block(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    cx: i64,
    cy: i64,
    half: i64,
    radius: i64,
    minimum: f64,
) -> BlockMatch {
    let mut m = BlockMatch {
        x: cx as f64,
        y: cy as f64,
        ..BlockMatch::default()
    };
    let (x0, y0) = (cx - half, cy - half);
    let size = 2 * half;
    let mut mean = 0.0f64;
    for y in 0..size {
        for x in 0..size {
            mean += reference[((y0 + y) * columns + x0 + x) as usize];
        }
    }
    let n = (size * size) as f64;
    mean /= n;
    let mut ref_norm = 0.0f64;
    for y in 0..size {
        for x in 0..size {
            let d = reference[((y0 + y) * columns + x0 + x) as usize] - mean;
            ref_norm += d * d;
        }
    }
    if ref_norm < 1e-9 * n {
        return m;
    }
    let (mut best, mut best_x, mut best_y) = (f64::NEG_INFINITY, 0i64, 0i64);
    for oy in -radius..=radius {
        for ox in -radius..=radius {
            let (mut sum, mut squares, mut cross) = (0.0f64, 0.0f64, 0.0f64);
            for y in 0..size {
                let row = (y0 + y + oy) * columns + x0 + ox;
                let ref_row = (y0 + y) * columns + x0;
                for x in 0..size {
                    let v = moving[(row + x) as usize];
                    sum += v;
                    squares += v * v;
                    cross += (reference[(ref_row + x) as usize] - mean) * v;
                }
            }
            let mov_norm = squares - sum * sum / n;
            let mut score = 0.0;
            if mov_norm > 1e-12 {
                score = cross / (ref_norm * mov_norm).sqrt();
            }
            if score > best {
                (best, best_x, best_y) = (score, ox, oy);
            }
        }
    }
    m.dx = best_x as f64;
    m.dy = best_y as f64;
    if best_x.abs() < radius && best_y.abs() < radius {
        (m.dx, m.dy) = refine_block(reference, moving, columns, x0, y0, size, m.dx, m.dy);
    }
    m.correlation = best;
    m.accepted = best >= minimum && best_x.abs() < radius && best_y.abs() < radius;
    m
}
fn refine_block(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    x0: i64,
    y0: i64,
    size: i64,
    mut dx: f64,
    mut dy: f64,
) -> (f64, f64) {
    let rows = moving.len() as i64 / columns;
    let n = (size * size) as usize;
    let sz = size as usize;
    let mut block = vec![0.0f64; n];
    let (mut mean, mut squares) = (0.0f64, 0.0f64);
    for y in 0..size {
        for x in 0..size {
            let v = reference[((y0 + y) * columns + x0 + x) as usize];
            block[(y * size + x) as usize] = v;
            mean += v;
            squares += v * v;
        }
    }
    mean /= n as f64;
    let sd = fmath::max(squares / n as f64 - mean * mean, 0.0).sqrt();
    if sd == 0.0 {
        return (dx, dy);
    }
    for v in &mut block {
        *v = (*v - mean) / sd;
    }
    let (start_x, start_y) = (dx, dy);
    let mut moved = vec![0.0f64; n];
    let mut residuals: Vec<f64> = Vec::with_capacity(n);
    for _ in 0..8 {
        let (mut m_mean, mut m_squares) = (0.0f64, 0.0f64);
        for y in 0..size {
            for x in 0..size {
                let v = bilinear(
                    moving,
                    columns,
                    rows,
                    (x0 + x) as f64 + dx,
                    (y0 + y) as f64 + dy,
                );
                moved[(y * size + x) as usize] = v;
                m_mean += v;
                m_squares += v * v;
            }
        }
        m_mean /= n as f64;
        let m_sd = fmath::max(m_squares / n as f64 - m_mean * m_mean, 0.0).sqrt();
        if m_sd == 0.0 {
            break;
        }
        residuals.clear();
        for y in 1..sz.saturating_sub(1) {
            for x in 1..sz.saturating_sub(1) {
                let i = y * sz + x;
                residuals.push((block[i] - (moved[i] - m_mean) / m_sd).abs());
            }
        }
        let cut = 6.945981 * median(&residuals);
        let (mut gxx, mut gxy, mut gyy, mut bx, mut by) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for y in 1..sz.saturating_sub(1) {
            for x in 1..sz.saturating_sub(1) {
                let i = y * sz + x;
                let gx = (moved[i + 1] - moved[i - 1]) / (2.0 * m_sd);
                let gy = (moved[i + sz] - moved[i - sz]) / (2.0 * m_sd);
                let e = block[i] - (moved[i] - m_mean) / m_sd;
                let mut w = 1.0;
                if cut > 1e-9 {
                    let u = e / cut;
                    if u.abs() < 1.0 {
                        w = (1.0 - u * u) * (1.0 - u * u);
                    } else {
                        w = 0.0;
                    }
                }
                gxx += w * gx * gx;
                gxy += w * gx * gy;
                gyy += w * gy * gy;
                bx += w * gx * e;
                by += w * gy * e;
            }
        }
        let det = gxx * gyy - gxy * gxy;
        if det <= 1e-12 || !finite(det) {
            break;
        }
        let (step_x, step_y) = ((gyy * bx - gxy * by) / det, (gxx * by - gxy * bx) / det);
        if !finite(step_x) || !finite(step_y) {
            break;
        }
        dx = fmath::max(start_x - 1.0, fmath::min(start_x + 1.0, dx + step_x));
        dy = fmath::max(start_y - 1.0, fmath::min(start_y + 1.0, dy + step_y));
        if fmath::hypot(step_x, step_y) < 1e-3 {
            break;
        }
    }
    (dx, dy)
}
fn reject_weak_blocks(blocks: &mut [BlockMatch]) {
    let correlations: Vec<f64> = blocks
        .iter()
        .filter(|b| b.accepted)
        .map(|b| b.correlation)
        .collect();
    if correlations.len() < 4 {
        return;
    }
    let cut = median(&correlations) - 0.05;
    for block in blocks {
        if block.accepted && block.correlation < cut {
            block.accepted = false;
        }
    }
}
fn reject_outliers(blocks: &mut [BlockMatch], nx: i64, ny: i64) {
    let mut reject = vec![false; blocks.len()];
    for j in 0..ny {
        for i in 0..nx {
            let block = blocks[(j * nx + i) as usize];
            if !block.accepted {
                continue;
            }
            let (mut dxs, mut dys) = (Vec::new(), Vec::new());
            for dj in -1..=1 {
                for di in -1..=1 {
                    if (di == 0 && dj == 0)
                        || i + di < 0
                        || i + di >= nx
                        || j + dj < 0
                        || j + dj >= ny
                    {
                        continue;
                    }
                    let neighbour = blocks[((j + dj) * nx + i + di) as usize];
                    if neighbour.accepted {
                        dxs.push(neighbour.dx);
                        dys.push(neighbour.dy);
                    }
                }
            }
            if dxs.len() < 3 {
                continue;
            }
            if fmath::hypot(block.dx - median(&dxs), block.dy - median(&dys)) > 1.5 {
                reject[(j * nx + i) as usize] = true;
            }
        }
    }
    for (block, r) in blocks.iter_mut().zip(reject) {
        if r {
            block.accepted = false;
        }
    }
}
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| {
        if a < b || (a.is_nan() && !b.is_nan()) {
            std::cmp::Ordering::Less
        } else if b < a || (b.is_nan() && !a.is_nan()) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let n = sorted.len();
    if n % 2 == 1 {
        return sorted[n / 2];
    }
    (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
}
fn spread_blocks(
    blocks: &[BlockMatch],
    xs: &[i64],
    ys: &[i64],
    columns: i64,
    rows: i64,
    step: f64,
) -> DisplacementField {
    let (nx, ny) = (xs.len() as i64, ys.len() as i64);
    let mut grid_x = vec![0.0f64; (nx * ny) as usize];
    let mut grid_y = vec![0.0f64; (nx * ny) as usize];
    for j in 0..ny {
        for i in 0..nx {
            let (mut sx, mut sy, mut sw) = (0.0f64, 0.0f64, 0.0f64);
            for jj in 0.max(j - 3)..=(ny - 1).min(j + 3) {
                for ii in 0.max(i - 3)..=(nx - 1).min(i + 3) {
                    let block = blocks[(jj * nx + ii) as usize];
                    if !block.accepted {
                        continue;
                    }
                    let d2 = ((ii - i) * (ii - i) + (jj - j) * (jj - j)) as f64;
                    let w = block.correlation * fmath::exp(-d2 / 2.0);
                    sx += w * block.dx;
                    sy += w * block.dy;
                    sw += w;
                }
            }
            if sw > 1e-6 {
                grid_x[(j * nx + i) as usize] = sx / sw;
                grid_y[(j * nx + i) as usize] = sy / sw;
            }
        }
    }
    let size = (columns * rows) as usize;
    let mut field = DisplacementField {
        columns,
        rows,
        dx: vec![0.0; size],
        dy: vec![0.0; size],
    };
    if nx == 0 || ny == 0 {
        return field;
    }
    for y in 0..rows {
        let v = fmath::max(
            0.0,
            fmath::min((ny - 1) as f64, (y as f64 - ys[0] as f64) / step),
        );
        for x in 0..columns {
            let u = fmath::max(
                0.0,
                fmath::min((nx - 1) as f64, (x as f64 - xs[0] as f64) / step),
            );
            let i = (y * columns + x) as usize;
            field.dx[i] = bilinear(&grid_x, nx, ny, u, v);
            field.dy[i] = bilinear(&grid_y, nx, ny, u, v);
        }
    }
    field
}
pub fn apply_field(
    mask: &[f64],
    columns: i64,
    rows: i64,
    field: &DisplacementField,
) -> Result<Vec<f64>, Error> {
    if field.columns != columns
        || field.rows != rows
        || !frame_size_matches(columns, rows, &[mask, &field.dx, &field.dy])
    {
        return Err(Error::wrap(
            ErrorKind::Motion,
            "field and mask sizes differ",
        ));
    }
    let mut out = vec![0.0f64; mask.len()];
    for y in 0..rows {
        for x in 0..columns {
            let i = (y * columns + x) as usize;
            let (dx, dy) = (field.dx[i], field.dy[i]);
            if !finite(dx) || !finite(dy) {
                return Err(Error::wrap(
                    ErrorKind::Motion,
                    format!("non-finite displacement at ({x},{y})"),
                ));
            }
            out[i] = bilinear(mask, columns, rows, x as f64 + dx, y as f64 + dy);
        }
    }
    Ok(out)
}
