use super::{Error, Kind};
const MAX_TEXT_CANDIDATE_PIXELS: i64 = 1 << 20;
const MAX_TEXT_CANDIDATE_GLYPHS: usize = 1024;
const MAX_TEXT_CANDIDATES: usize = 64;
const TEXT_CANDIDATE_CONTRAST: i64 = 56;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    pub const fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect {
            min_x: if x0 < x1 { x0 } else { x1 },
            min_y: if y0 < y1 { y0 } else { y1 },
            max_x: if x0 < x1 { x1 } else { x0 },
            max_y: if y0 < y1 { y1 } else { y0 },
        }
    }
    pub fn dx(&self) -> i64 {
        self.max_x - self.min_x
    }
    pub fn dy(&self) -> i64 {
        self.max_y - self.min_y
    }
    pub fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn inset(mut self, n: i64) -> Rect {
        if self.dx() < 2 * n {
            self.min_x = (self.min_x + self.max_x) / 2;
            self.max_x = self.min_x;
        } else {
            self.min_x += n;
            self.max_x -= n;
        }
        if self.dy() < 2 * n {
            self.min_y = (self.min_y + self.max_y) / 2;
            self.max_y = self.min_y;
        } else {
            self.min_y += n;
            self.max_y -= n;
        }
        self
    }
    fn add(self, x: i64, y: i64) -> Rect {
        Rect {
            min_x: self.min_x + x,
            min_y: self.min_y + y,
            max_x: self.max_x + x,
            max_y: self.max_y + y,
        }
    }
    fn intersect(self, s: Rect) -> Rect {
        let r = Rect {
            min_x: self.min_x.max(s.min_x),
            min_y: self.min_y.max(s.min_y),
            max_x: self.max_x.min(s.max_x),
            max_y: self.max_y.min(s.max_y),
        };
        if r.is_empty() { Rect::default() } else { r }
    }
    fn union(self, s: Rect) -> Rect {
        if self.is_empty() {
            return s;
        }
        if s.is_empty() {
            return self;
        }
        Rect {
            min_x: self.min_x.min(s.min_x),
            min_y: self.min_y.min(s.min_y),
            max_x: self.max_x.max(s.max_x),
            max_y: self.max_y.max(s.max_y),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameModel {
    Gray,
    Gray16,
    Rgba,
    Nrgba,
}
#[derive(Clone, Debug)]
pub struct TextFrame {
    pub bounds: Rect,
    pub model: FrameModel,
    pub stride: usize,
    pub pix: Vec<u8>,
}
impl TextFrame {
    fn gray_at(&self, x: i64, y: i64) -> u8 {
        if x < self.bounds.min_x
            || y < self.bounds.min_y
            || x >= self.bounds.max_x
            || y >= self.bounds.max_y
        {
            return 0;
        }
        let (dx, dy) = (
            (x - self.bounds.min_x) as usize,
            (y - self.bounds.min_y) as usize,
        );
        let at = |i: usize| self.pix.get(i).copied().unwrap_or(0);
        let luminance =
            |r: u32, g: u32, b: u32| ((19595 * r + 38470 * g + 7471 * b + (1 << 15)) >> 24) as u8;
        match self.model {
            FrameModel::Gray => at(dy * self.stride + dx),
            FrameModel::Gray16 => {
                let i = dy * self.stride + dx * 2;
                let y16 = u32::from(at(i)) << 8 | u32::from(at(i + 1));
                luminance(y16, y16, y16)
            }
            FrameModel::Rgba => {
                let i = dy * self.stride + dx * 4;
                let c = |v: u8| u32::from(v) * 0x101;
                luminance(c(at(i)), c(at(i + 1)), c(at(i + 2)))
            }
            FrameModel::Nrgba => {
                let i = dy * self.stride + dx * 4;
                let a = u32::from(at(i + 3)) * 0x101;
                let c = |v: u8| u32::from(v) * 0x101 * a / 0xffff;
                luminance(c(at(i)), c(at(i + 1)), c(at(i + 2)))
            }
        }
    }
}
fn inconclusive() -> Error {
    Error::bare(Kind::PixelTextReviewInconclusive)
}
pub fn pixel_text_candidates(frame: Option<&TextFrame>) -> Result<Vec<Rect>, Error> {
    let Some(frame) = frame else {
        return Err(inconclusive());
    };
    let (width, height) = (frame.bounds.dx(), frame.bounds.dy());
    if width < 24 || height < 32 || width > MAX_TEXT_CANDIDATE_PIXELS / height {
        return Err(inconclusive());
    }
    let band_height = height / 4;
    let mut candidates = Vec::new();
    for offset in [0, height - band_height] {
        let band = Rect::new(0, offset, width, offset + band_height);
        let boxes = text_band_candidates(frame, band)?;
        if boxes.len() > MAX_TEXT_CANDIDATES - candidates.len() {
            return Err(inconclusive());
        }
        candidates.extend(boxes);
    }
    Ok(candidates)
}
fn text_band_candidates(frame: &TextFrame, band: Rect) -> Result<Vec<Rect>, Error> {
    let (width, height) = (band.dx() as usize, band.dy() as usize);
    let mut luminance = vec![0u8; width * height];
    let mut histogram = [0usize; 256];
    let (ox, oy) = (
        frame.bounds.min_x + band.min_x,
        frame.bounds.min_y + band.min_y,
    );
    for y in 0..height {
        for x in 0..width {
            let value = frame.gray_at(ox + x as i64, oy + y as i64);
            luminance[y * width + x] = value;
            histogram[value as usize] += 1;
        }
    }
    let mut median = 0i64;
    let mut count = 0;
    for (value, frequency) in histogram.iter().enumerate() {
        count += frequency;
        if count > luminance.len() / 2 {
            median = value as i64;
            break;
        }
    }
    let mut boxes = Vec::new();
    for direction in [1i64, -1] {
        let threshold = direction * median + TEXT_CANDIDATE_CONTRAST;
        if threshold > (255 * direction).max(0) {
            continue;
        }
        let glyphs = text_glyph_boxes(&luminance, width, height, direction, threshold)?;
        for run in text_glyph_runs(glyphs) {
            boxes.push(run.inset(-2).add(band.min_x, band.min_y).intersect(band));
        }
    }
    boxes.sort_by_key(|a| (a.min_y, a.min_x));
    Ok(boxes)
}
fn text_glyph_boxes(
    luminance: &[u8],
    width: usize,
    height: usize,
    direction: i64,
    threshold: i64,
) -> Result<Vec<Rect>, Error> {
    let mut seen = vec![false; luminance.len()];
    let mut glyphs = Vec::new();
    let mut queue: Vec<usize> = Vec::with_capacity(64);
    let on = |v: u8| direction * i64::from(v) >= threshold;
    for (index, &value) in luminance.iter().enumerate() {
        if seen[index] || !on(value) {
            continue;
        }
        queue.clear();
        queue.push(index);
        seen[index] = true;
        let (x, y) = ((index % width) as i64, (index / width) as i64);
        let mut b = Rect {
            min_x: x,
            min_y: y,
            max_x: x + 1,
            max_y: y + 1,
        };
        let mut head = 0;
        while head < queue.len() {
            let (x, y) = (queue[head] % width, queue[head] / width);
            b.min_x = b.min_x.min(x as i64);
            b.min_y = b.min_y.min(y as i64);
            b.max_x = b.max_x.max(x as i64 + 1);
            b.max_y = b.max_y.max(y as i64 + 1);
            for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                    let next = ny * width + nx;
                    if !seen[next] && on(luminance[next]) {
                        seen[next] = true;
                        queue.push(next);
                    }
                }
            }
            head += 1;
        }
        if b.dy() < 4
            || b.dy() > (height as i64).min(64)
            || b.dx() < 2
            || b.dx() > b.dy() * 2
            || queue.len() < 6
        {
            continue;
        }
        if glyphs.len() == MAX_TEXT_CANDIDATE_GLYPHS {
            return Err(inconclusive());
        }
        glyphs.push(b);
    }
    Ok(glyphs)
}
fn text_glyph_runs(mut glyphs: Vec<Rect>) -> Vec<Rect> {
    struct Run {
        bbox: Rect,
        last: Rect,
        count: usize,
    }
    glyphs.sort_by_key(|a| (a.min_x, a.min_y));
    let mut runs: Vec<Run> = Vec::new();
    for glyph in glyphs {
        let mut matched = None;
        let mut best_gap = 1i64 << 30;
        for (i, run) in runs.iter().enumerate() {
            let gap = glyph.min_x - run.last.max_x;
            let tall = glyph.dy().max(run.last.dy());
            let short = glyph.dy().min(run.last.dy());
            if gap >= 0
                && gap <= tall * 2
                && gap < best_gap
                && tall <= short * 2
                && glyph.max_y.min(run.last.max_y) - glyph.min_y.max(run.last.min_y) >= short / 2
            {
                matched = Some(i);
                best_gap = gap;
            }
        }
        match matched {
            None => runs.push(Run {
                bbox: glyph,
                last: glyph,
                count: 1,
            }),
            Some(i) => {
                let run = &mut runs[i];
                run.bbox = run.bbox.union(glyph);
                run.last = glyph;
                run.count += 1;
            }
        }
    }
    runs.into_iter()
        .filter(|r| r.count >= 3)
        .map(|r| r.bbox)
        .collect()
}
