struct HeaderWriter {
    buf: Vec<u8>,
    cur: u8,
    free: u32,
}
impl HeaderWriter {
    fn new() -> HeaderWriter {
        HeaderWriter {
            buf: Vec::new(),
            cur: 0,
            free: 8,
        }
    }
    fn bit(&mut self, b: i64) {
        self.cur = (self.cur << 1) | (b & 1) as u8;
        self.free -= 1;
        if self.free == 0 {
            self.buf.push(self.cur);
            self.free = if self.cur == 0xFF { 7 } else { 8 };
            self.cur = 0;
        }
    }
    fn bits(&mut self, v: u32, n: i64) {
        let mut i = n - 1;
        while i >= 0 {
            self.bit(((v >> i) & 1) as i64);
            i -= 1;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        let full = if self.buf.last() == Some(&0xFF) { 7 } else { 8 };
        if self.free != full {
            self.buf.push(self.cur << self.free);
        } else if self.buf.last() == Some(&0xFF) {
            self.buf.push(0);
        }
        self.buf
    }
}
struct TagLevel {
    w: i64,
    h: i64,
    value: Vec<i64>,
    low: Vec<i64>,
    known: Vec<bool>,
}
struct TagTree {
    levels: Vec<TagLevel>,
}
impl TagTree {
    fn new(mut w: i64, mut h: i64, values: &[i64]) -> TagTree {
        let mut t = TagTree { levels: Vec::new() };
        loop {
            let n = (w * h) as usize;
            t.levels.push(TagLevel {
                w,
                h,
                value: vec![0; n],
                low: vec![0; n],
                known: vec![false; n],
            });
            if w == 1 && h == 1 {
                break;
            }
            w = (w + 1) / 2;
            h = (h + 1) / 2;
        }
        let n = t.levels[0].value.len().min(values.len());
        t.levels[0].value[..n].copy_from_slice(&values[..n]);
        for l in 1..t.levels.len() {
            let (lower, upper) = t.levels.split_at_mut(l);
            let child = &lower[l - 1];
            let parent = &mut upper[0];
            for v in parent.value.iter_mut() {
                *v = i64::MAX;
            }
            for y in 0..child.h {
                for x in 0..child.w {
                    let p = ((y / 2) * parent.w + x / 2) as usize;
                    let v = child.value[(y * child.w + x) as usize];
                    if v < parent.value[p] {
                        parent.value[p] = v;
                    }
                }
            }
        }
        t
    }
    fn encode(&mut self, w: &mut HeaderWriter, x: i64, y: i64, threshold: i64) {
        let mut low = 0i64;
        for l in (0..self.levels.len()).rev() {
            let level = &mut self.levels[l];
            let i = ((y >> l) * level.w + (x >> l)) as usize;
            if low > level.low[i] {
                level.low[i] = low;
            } else {
                low = level.low[i];
            }
            while low < threshold {
                if low >= level.value[i] {
                    if !level.known[i] {
                        w.bit(1);
                        level.known[i] = true;
                    }
                    break;
                }
                w.bit(0);
                low += 1;
            }
            level.low[i] = low;
        }
    }
}
fn pass_count(w: &mut HeaderWriter, n: i64) {
    if n == 1 {
        w.bit(0);
    } else if n == 2 {
        w.bits(2, 2);
    } else if n <= 5 {
        w.bits(3, 2);
        w.bits((n - 3) as u32, 2);
    } else if n <= 36 {
        w.bits(0xF, 4);
        w.bits((n - 6) as u32, 5);
    } else {
        w.bits(0x1FF, 9);
        w.bits((n - 37) as u32, 7);
    }
}
fn floor_log2(mut n: i64) -> i64 {
    let mut r = 0;
    while n > 1 {
        n >>= 1;
        r += 1;
    }
    r
}
pub(super) fn bit_length(mut n: i64) -> i64 {
    let mut r = 0;
    while n > 0 {
        n >>= 1;
        r += 1;
    }
    r
}
#[derive(Debug, Clone, Default)]
pub(super) struct CodedBlock {
    pub data: Vec<u8>,
    pub passes: i64,
    pub planes: i64,
}
#[derive(Debug, Clone, Default)]
pub(super) struct PacketBand {
    pub cols: i64,
    pub rows: i64,
    pub blocks: Vec<CodedBlock>,
    pub mb: i64,
}
pub(super) fn write_packet(out: &mut Vec<u8>, bands: &[PacketBand], ht: bool) {
    let mut w = HeaderWriter::new();
    let empty = !bands
        .iter()
        .any(|b| b.blocks.iter().any(|blk| blk.passes > 0));
    if empty {
        w.bit(0);
        out.extend_from_slice(&w.finish());
        return;
    }
    w.bit(1);
    for band in bands {
        if band.blocks.is_empty() {
            continue;
        }
        let mut inclusion = vec![0i64; band.blocks.len()];
        let mut zero = vec![0i64; band.blocks.len()];
        for (i, block) in band.blocks.iter().enumerate() {
            if block.passes == 0 {
                inclusion[i] = 1;
                zero[i] = band.mb;
                continue;
            }
            zero[i] = if ht {
                band.mb - 1
            } else {
                band.mb - block.planes
            };
        }
        let mut inclusion_tree = TagTree::new(band.cols, band.rows, &inclusion);
        let mut zero_tree = TagTree::new(band.cols, band.rows, &zero);
        for (i, block) in band.blocks.iter().enumerate() {
            let (x, y) = (i as i64 % band.cols, i as i64 / band.cols);
            inclusion_tree.encode(&mut w, x, y, 1);
            if block.passes == 0 {
                continue;
            }
            zero_tree.encode(&mut w, x, y, 1 << 30);
            pass_count(&mut w, block.passes);
            let mut lblock = 3;
            let length_bits = bit_length(block.data.len() as i64);
            let mut extra = length_bits - floor_log2(block.passes) - lblock;
            while extra > 0 {
                w.bit(1);
                lblock += 1;
                extra -= 1;
            }
            w.bit(0);
            w.bits(block.data.len() as u32, lblock + floor_log2(block.passes));
        }
    }
    out.extend_from_slice(&w.finish());
    for band in bands {
        for block in &band.blocks {
            out.extend_from_slice(&block.data);
        }
    }
}
