use super::codestream::{CMODE_BYPASS, CMODE_TERM_ALL, CodeBlockGrid, SCOD_EPH, SCOD_SOP};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PacketError {
    Truncated,
    Invalid,
}
struct HeaderBits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u8,
    avail: u32,
    last_ff: bool,
}
impl HeaderBits<'_> {
    fn bit(&mut self) -> Result<u32, PacketError> {
        if self.avail == 0 {
            if self.pos >= self.data.len() {
                return Err(PacketError::Truncated);
            }
            self.buf = self.data[self.pos];
            self.pos += 1;
            self.avail = if self.last_ff { 7 } else { 8 };
            self.last_ff = self.buf == 0xff;
        }
        self.avail -= 1;
        Ok(u32::from(self.buf >> self.avail) & 1)
    }
    fn bits(&mut self, n: usize) -> Result<u32, PacketError> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Ok(v)
    }
    fn end(&mut self) -> Result<usize, PacketError> {
        if self.last_ff {
            if self.pos >= self.data.len() {
                return Err(PacketError::Truncated);
            }
            self.pos += 1;
        }
        Ok(self.pos)
    }
}
const TAG_UNKNOWN: i32 = 1 << 30;
struct TagTree {
    widths: Vec<usize>,
    value: Vec<Vec<i32>>,
    low: Vec<Vec<i32>>,
}
impl TagTree {
    fn new(mut w: usize, mut h: usize) -> TagTree {
        let mut t = TagTree {
            widths: Vec::new(),
            value: Vec::new(),
            low: Vec::new(),
        };
        loop {
            let n = w * h;
            t.widths.push(w);
            t.value.push(vec![TAG_UNKNOWN; n]);
            t.low.push(vec![0; n]);
            if w <= 1 && h <= 1 {
                return t;
            }
            w = w.div_ceil(2);
            h = h.div_ceil(2);
        }
    }
    fn decode(
        &mut self,
        r: &mut HeaderBits<'_>,
        x: usize,
        y: usize,
        threshold: i32,
    ) -> Result<bool, PacketError> {
        let mut low = 0i32;
        for level in (0..self.widths.len()).rev() {
            let i = (y >> level) * self.widths[level] + (x >> level);
            if low > self.low[level][i] {
                self.low[level][i] = low;
            } else {
                low = self.low[level][i];
            }
            while low < threshold && low < self.value[level][i] {
                if r.bit()? == 1 {
                    self.value[level][i] = low;
                } else {
                    low += 1;
                }
            }
            self.low[level][i] = low;
        }
        Ok(self.value[0][y * self.widths[0] + x] < threshold)
    }
}
#[derive(Clone, Copy, Default)]
struct CodeBlockState {
    included: bool,
    passes: usize,
    lblock: usize,
}
#[derive(Default)]
struct BandState {
    grid: CodeBlockGrid,
    inclusion: Option<TagTree>,
    zero_bit_planes: Option<TagTree>,
    blocks: Vec<CodeBlockState>,
}
pub(crate) struct PrecinctState {
    bands: Vec<BandState>,
}
impl PrecinctState {
    pub fn new(grids: &[CodeBlockGrid]) -> PrecinctState {
        let bands = grids
            .iter()
            .map(|g| {
                if g.cw == 0 || g.ch == 0 {
                    return BandState::default();
                }
                BandState {
                    grid: *g,
                    inclusion: Some(TagTree::new(g.cw, g.ch)),
                    zero_bit_planes: Some(TagTree::new(g.cw, g.ch)),
                    blocks: vec![
                        CodeBlockState {
                            lblock: 3,
                            ..CodeBlockState::default()
                        };
                        g.cw * g.ch
                    ],
                }
            })
            .collect();
        PrecinctState { bands }
    }
}
const MAX_PASSES_PER_BLOCK: usize = 3 * 74 - 2;
pub(crate) fn measure_packet(
    data: &[u8],
    mut pos: usize,
    state: &mut PrecinctState,
    layer: usize,
    scod: u8,
    cmodes: u8,
) -> Result<usize, PacketError> {
    if scod & SCOD_SOP != 0 && pos + 6 <= data.len() && data[pos] == 0xff && data[pos + 1] == 0x91 {
        if data[pos + 2] != 0 || data[pos + 3] != 4 {
            return Err(PacketError::Invalid);
        }
        pos += 6;
    }
    let mut r = HeaderBits {
        data,
        pos,
        buf: 0,
        avail: 0,
        last_ff: false,
    };
    let present = r.bit()?;
    let mut body = 0u64;
    if present == 1 {
        for band in &mut state.bands {
            let cw = band.grid.cw;
            for i in 0..band.blocks.len() {
                let (x, y) = (i % cw, i / cw);
                let block = &mut band.blocks[i];
                let included = if !block.included {
                    match band.inclusion.as_mut() {
                        Some(tree) => tree.decode(&mut r, x, y, (layer + 1) as i32)?,
                        None => false,
                    }
                } else {
                    r.bit()? == 1
                };
                if !included {
                    continue;
                }
                if !block.included {
                    let mut threshold = 1i32;
                    loop {
                        if threshold > 80 {
                            return Err(PacketError::Invalid);
                        }
                        let known = match band.zero_bit_planes.as_mut() {
                            Some(tree) => tree.decode(&mut r, x, y, threshold)?,
                            None => true,
                        };
                        if known {
                            break;
                        }
                        threshold += 1;
                    }
                    block.included = true;
                }
                let passes = read_pass_count(&mut r)?;
                loop {
                    if r.bit()? == 0 {
                        break;
                    }
                    block.lblock += 1;
                    if block.lblock > 32 {
                        return Err(PacketError::Invalid);
                    }
                }
                if block.passes + passes > MAX_PASSES_PER_BLOCK {
                    return Err(PacketError::Invalid);
                }
                let mut first = block.passes;
                while first < block.passes + passes {
                    let last = segment_end(first, cmodes).min(block.passes + passes);
                    let n = bits_len(last - first) - 1 + block.lblock;
                    if n > 32 {
                        return Err(PacketError::Invalid);
                    }
                    body += u64::from(r.bits(n)?);
                    first = last;
                }
                block.passes += passes;
            }
        }
    }
    let mut end = r.end()?;
    if scod & SCOD_EPH != 0 {
        if end + 2 > data.len() || data[end] != 0xff || data[end + 1] != 0x92 {
            return Err(PacketError::Invalid);
        }
        end += 2;
    }
    if body > (data.len() - end) as u64 {
        return Err(PacketError::Truncated);
    }
    Ok(end + body as usize)
}
fn bits_len(v: usize) -> usize {
    (usize::BITS - v.leading_zeros()) as usize
}
fn read_pass_count(r: &mut HeaderBits<'_>) -> Result<usize, PacketError> {
    if r.bit()? == 0 {
        return Ok(1);
    }
    if r.bit()? == 0 {
        return Ok(2);
    }
    let v = r.bits(2)?;
    if v < 3 {
        return Ok(3 + v as usize);
    }
    let v = r.bits(5)?;
    if v < 31 {
        return Ok(6 + v as usize);
    }
    Ok(37 + r.bits(7)? as usize)
}
fn segment_end(p: usize, cmodes: u8) -> usize {
    if cmodes & CMODE_TERM_ALL != 0 {
        return p + 1;
    }
    if cmodes & CMODE_BYPASS != 0 {
        if p < 10 {
            return 10;
        }
        if (p - 10) % 3 == 2 {
            return p + 1;
        }
        return p - (p - 10) % 3 + 2;
    }
    MAX_PASSES_PER_BLOCK + 1
}
