use super::bitio::{BitReader, bits_n, bits_off, ceil_log2, div_ceil, unpack_signed, val};
use super::codestream::{FLAG_NOISE, FLAG_PATCHES, FLAG_SPLINES, FrameHeader};
use super::entropy::{
    EntropyCode, SymbolReader, read_context_map, read_entropy_code, read_permutation,
};
use super::modular::{Channel, ModularContext, ModularImage, decode_modular, read_tree};
use super::{Error, corrupt, fault, unsupported};
pub(crate) const NUM_ORDERS: usize = 13;
const NON_ZERO_BUCKETS: usize = 37;
const ZERO_DENSITY_CONTEXT: usize = 458;
pub(crate) const NUM_QUANT_TABLES: usize = 17;
pub(crate) const COLOUR_TILE_BLOCKS: usize = 8;
const AC_STRATEGY_DCT8: u8 = 0;
pub(crate) const DEFAULT_BLOCK_CTX_MAP: [u8; 39] = [
    0, 1, 2, 2, 3, 3, 4, 5, 6, 6, 6, 6, 6, 7, 8, 9, 9, 10, 11, 12, 13, 14, 14, 14, 14, 14, 7, 8, 9,
    9, 10, 11, 12, 13, 14, 14, 14, 14, 14,
];
const ORDER_LLF: [usize; NUM_ORDERS] = [1, 1, 4, 16, 2, 4, 8, 64, 32, 256, 128, 1024, 512];
const COEFF_FREQ_CONTEXT: [usize; 64] = [
    0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19,
    20, 20, 21, 21, 22, 22, 23, 23, 23, 23, 24, 24, 24, 24, 25, 25, 25, 25, 26, 26, 26, 26, 27, 27,
    27, 27, 28, 28, 28, 28, 29, 29, 29, 29, 30, 30, 30, 30,
];
const COEFF_NUM_NONZERO_CONTEXT: [usize; 64] = [
    0, 0, 31, 62, 62, 93, 93, 93, 93, 123, 123, 123, 123, 152, 152, 152, 152, 152, 152, 152, 152,
    180, 180, 180, 180, 180, 180, 180, 180, 180, 180, 180, 180, 206, 206, 206, 206, 206, 206, 206,
    206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206, 206,
    206, 206, 206, 206, 206,
];
const QUANT_TABLE_SIZE_X: [usize; NUM_QUANT_TABLES] =
    [1, 1, 1, 1, 2, 4, 1, 1, 2, 1, 1, 8, 4, 16, 8, 32, 16];
const QUANT_TABLE_SIZE_Y: [usize; NUM_QUANT_TABLES] =
    [1, 1, 1, 1, 2, 4, 2, 4, 4, 1, 1, 8, 8, 16, 16, 32, 32];
#[derive(Clone)]
pub(crate) struct BlockCtxMap {
    pub dc_thresholds: [Vec<i32>; 3],
    pub qf_thresholds: Vec<u32>,
    pub ctx_map: Vec<u8>,
    pub num_dc_ctxs: usize,
    pub num_ctxs: usize,
}
impl BlockCtxMap {
    pub(crate) fn default_map() -> Self {
        BlockCtxMap {
            dc_thresholds: [Vec::new(), Vec::new(), Vec::new()],
            qf_thresholds: Vec::new(),
            ctx_map: DEFAULT_BLOCK_CTX_MAP.to_vec(),
            num_dc_ctxs: 1,
            num_ctxs: 15,
        }
    }
    pub(crate) fn context(&self, dc_idx: usize, qf: i32, ord: usize, c: usize) -> usize {
        let qf_idx = self
            .qf_thresholds
            .iter()
            .filter(|&&t| qf as u32 > t)
            .count();
        let mut idx = if c < 2 { c ^ 1 } else { 2 };
        idx = idx.saturating_mul(NUM_ORDERS).saturating_add(ord);
        idx = idx
            .saturating_mul(self.qf_thresholds.len().saturating_add(1))
            .saturating_add(qf_idx);
        idx = idx.saturating_mul(self.num_dc_ctxs).saturating_add(dc_idx);
        self.ctx_map.get(idx).map_or(0, |&m| m as usize)
    }
    pub(crate) fn non_zero_context(&self, non_zeros: usize, block_ctx: usize) -> usize {
        let ctx = if non_zeros >= 64 {
            36
        } else if non_zeros >= 8 {
            (non_zeros / 2).saturating_add(4)
        } else {
            non_zeros
        };
        ctx.saturating_mul(self.num_ctxs).saturating_add(block_ctx)
    }
    pub(crate) fn zero_density_offset(&self, block_ctx: usize) -> usize {
        self.num_ctxs
            .saturating_mul(NON_ZERO_BUCKETS)
            .saturating_add(ZERO_DENSITY_CONTEXT.saturating_mul(block_ctx))
    }
    pub(crate) fn num_ac_contexts(&self) -> usize {
        self.num_ctxs
            .saturating_mul(NON_ZERO_BUCKETS + ZERO_DENSITY_CONTEXT)
    }
}
fn read_block_ctx_map(br: &mut BitReader) -> Result<BlockCtxMap, Error> {
    let mut b = BlockCtxMap::default_map();
    if br.bool() {
        return Ok(b);
    }
    for thresholds in b.dc_thresholds.iter_mut() {
        let n = br.u(4) as usize;
        b.num_dc_ctxs = b.num_dc_ctxs.saturating_mul(n.saturating_add(1));
        for _ in 0..n {
            let v = br.u32(
                bits_n(4),
                bits_off(8, 16),
                bits_off(16, 272),
                bits_off(32, 65808),
            );
            thresholds.push(unpack_signed(v));
        }
    }
    let nq = br.u(4) as usize;
    for _ in 0..nq {
        let t = br
            .u32(bits_n(2), bits_off(3, 4), bits_off(5, 12), bits_off(8, 44))
            .saturating_add(1);
        b.qf_thresholds.push(t);
    }
    let contexts = b.num_dc_ctxs.saturating_mul(nq.saturating_add(1));
    if contexts > 64 {
        return Err(corrupt("block context map too large"));
    }
    b.ctx_map = vec![0; contexts.saturating_mul(3 * NUM_ORDERS)];
    let n = read_context_map(br, &mut b.ctx_map)?;
    if n > 16 {
        return Err(corrupt("too many block contexts"));
    }
    b.num_ctxs = n;
    Ok(b)
}
pub(crate) struct VarDctState {
    pub f: FrameHeader,
    pub bctx: BlockCtxMap,
    pub global: ModularContext,
    pub ytox_dc: i32,
    pub ytob_dc: i32,
    pub colour_factor: u32,
    pub base_corr_x: f32,
    pub base_corr_b: f32,
    pub acs: Vec<u8>,
    pub qf: Vec<i32>,
    pub qdc: Vec<u8>,
    pub ytox_map: Vec<i32>,
    pub ytob_map: Vec<i32>,
    pub tiles_x: usize,
    pub dc: [Vec<i32>; 3],
    pub dc_w: [usize; 3],
    pub dc_h: [usize; 3],
    pub raw_q_table: Vec<i32>,
    pub raw_q_table_den: f32,
    pub raw_q: bool,
    pub order0: [Vec<usize>; 3],
    pub num_histo: usize,
    pub hf_codes: Vec<EntropyCode>,
    pub coeffs: [Vec<i32>; 3],
    pub lf_precision: Option<Vec<u8>>,
    pub lf_dequant_default: bool,
    pub lf_dequant: [f32; 3],
    pub global_scale: u32,
    pub quant_lf: u32,
}
impl VarDctState {
    pub(crate) fn new(f: &FrameHeader, xyb: bool) -> Self {
        let nb = f.xsize_blocks.saturating_mul(f.ysize_blocks);
        let tiles_x = div_ceil(f.xsize_blocks, COLOUR_TILE_BLOCKS);
        let tiles = tiles_x.saturating_mul(div_ceil(f.ysize_blocks, COLOUR_TILE_BLOCKS));
        let mut s = VarDctState {
            f: f.clone(),
            bctx: BlockCtxMap::default_map(),
            global: ModularContext::default(),
            ytox_dc: 0,
            ytob_dc: 0,
            colour_factor: 0,
            base_corr_x: 0.0,
            base_corr_b: 0.0,
            acs: vec![0xFF; nb],
            qf: vec![0; nb],
            qdc: vec![0; nb],
            ytox_map: vec![0; tiles],
            ytob_map: vec![0; tiles],
            tiles_x,
            dc: Default::default(),
            dc_w: [0; 3],
            dc_h: [0; 3],
            raw_q_table: Vec::new(),
            raw_q_table_den: 0.0,
            raw_q: false,
            order0: Default::default(),
            num_histo: 0,
            hf_codes: Vec::new(),
            coeffs: Default::default(),
            lf_precision: xyb.then(|| vec![0; f.num_lf_groups]),
            lf_dequant_default: false,
            lf_dequant: [0.0; 3],
            global_scale: 0,
            quant_lf: 0,
        };
        let planes = s
            .dc_w
            .iter_mut()
            .zip(&mut s.dc_h)
            .zip(&mut s.dc)
            .zip(&mut s.coeffs);
        for (c, (((dc_w, dc_h), dc), coeffs)) in planes.enumerate() {
            *dc_w = f.xsize_blocks >> f.hshift(c);
            *dc_h = f.ysize_blocks >> f.vshift(c);
            let blocks = dc_w.saturating_mul(*dc_h);
            *dc = vec![0; blocks];
            *coeffs = vec![0; blocks.saturating_mul(64)];
        }
        s
    }
    pub(crate) fn read_lf_global(&mut self, br: &mut BitReader) -> Result<(), Error> {
        if self.f.flags & (FLAG_PATCHES | FLAG_SPLINES | FLAG_NOISE) != 0 {
            return Err(unsupported("patches, splines or noise in a VarDCT frame"));
        }
        self.lf_dequant_default = br.bool();
        if !self.lf_dequant_default {
            for d in self.lf_dequant.iter_mut() {
                *d = br.f16();
            }
        }
        self.global_scale = br.u32(
            bits_off(11, 1),
            bits_off(11, 2049),
            bits_off(12, 4097),
            bits_off(16, 8193),
        );
        self.quant_lf = br.u32(val(16), bits_off(5, 1), bits_off(8, 1), bits_off(16, 1));
        self.bctx = read_block_ctx_map(br)?;
        self.colour_factor = 84;
        if !br.bool() {
            self.colour_factor = br.u32(val(84), val(256), bits_off(8, 2), bits_off(16, 258));
            self.base_corr_x = br.f16();
            self.base_corr_b = br.f16();
            self.ytox_dc = (br.u(8) as i32).wrapping_sub(128);
            self.ytob_dc = (br.u(8) as i32).wrapping_sub(128);
        } else {
            self.base_corr_b = 1.0;
        }
        if br.bool() {
            let samples = self.f.xsize.saturating_mul(self.f.ysize).saturating_mul(3);
            let lim = (1usize << 22).min((samples / 16).saturating_add(1024));
            let tree = read_tree(br, lim)?;
            let code = read_entropy_code(br, tree.num_leaves())?;
            self.global = ModularContext {
                tree: Some(tree),
                code: Some(code),
            };
        }
        br.check()
    }
    pub(crate) fn lf_group_rect(&self, g: usize) -> (usize, usize, usize, usize) {
        let f = &self.f;
        let gx = g.checked_rem(f.x_lf_groups).unwrap_or(0);
        let gy = g.checked_div(f.x_lf_groups).unwrap_or(0);
        let (x0, y0) = (
            gx.saturating_mul(f.group_dim),
            gy.saturating_mul(f.group_dim),
        );
        (
            x0,
            y0,
            f.group_dim.min(f.xsize_blocks.saturating_sub(x0)),
            f.group_dim.min(f.ysize_blocks.saturating_sub(y0)),
        )
    }
    pub(crate) fn read_lf_group(&mut self, br: &mut BitReader, g: usize) -> Result<(), Error> {
        let f = self.f.clone();
        let (x0, y0, w, h) = self.lf_group_rect(g);
        let extra = br.u(2);
        match self.lf_precision.as_mut() {
            Some(p) => {
                let Some(slot) = p.get_mut(g) else {
                    return Err(fault());
                };
                *slot = extra as u8;
            }
            None if extra != 0 => return Err(unsupported("LF extra precision in a JPEG frame")),
            None => {}
        }
        const ORDER: [usize; 3] = [1, 0, 2];
        let mut img = ModularImage {
            bit_depth: 8,
            ..Default::default()
        };
        for &c in &ORDER {
            img.channels
                .push(Channel::new(w >> f.hshift(c), h >> f.vshift(c)));
        }
        decode_modular(
            br,
            &mut img,
            g.saturating_add(1),
            Some(&self.global),
            1 << 30,
            true,
        )?;
        for (ch, &c) in img.channels.iter().zip(&ORDER) {
            let (cx0, cy0) = (x0 >> f.hshift(c), y0 >> f.vshift(c));
            let (Some(&dc_w), Some(dc)) = (self.dc_w.get(c), self.dc.get_mut(c)) else {
                return Err(fault());
            };
            for (y, src) in ch.data.chunks_exact(ch.w.max(1)).take(ch.h).enumerate() {
                let start = cy0
                    .saturating_add(y)
                    .saturating_mul(dc_w)
                    .saturating_add(cx0);
                let Some(dst) = dc.get_mut(start..) else {
                    return Err(fault());
                };
                for (d, &s) in dst.iter_mut().zip(src) {
                    *d = s;
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (bx, by) = (x0.saturating_add(x), y0.saturating_add(y));
                let mut bucket = [0usize; 3];
                let planes = bucket
                    .iter_mut()
                    .zip(&self.dc)
                    .zip(&self.dc_w)
                    .zip(&self.bctx.dc_thresholds);
                for (c, (((b, dc), &dc_w), thresholds)) in planes.enumerate() {
                    let at = (by >> f.vshift(c))
                        .saturating_mul(dc_w)
                        .saturating_add(bx >> f.hshift(c));
                    let v = dc.get(at).copied().unwrap_or(0);
                    *b = thresholds.iter().filter(|&&t| v > t).count();
                }
                let [_, t1, t2] = &self.bctx.dc_thresholds;
                let b = bucket[0]
                    .saturating_mul(t2.len().saturating_add(1))
                    .saturating_add(bucket[2])
                    .saturating_mul(t1.len().saturating_add(1))
                    .saturating_add(bucket[1]);
                if let Some(q) = self
                    .qdc
                    .get_mut(by.saturating_mul(f.xsize_blocks).saturating_add(bx))
                {
                    *q = b as u8;
                }
            }
        }
        let count = (br.u(ceil_log2(w.saturating_mul(h) as u64)) as usize).saturating_add(1);
        let (tw, th) = (w.saturating_add(7) >> 3, h.saturating_add(7) >> 3);
        let mut meta = ModularImage {
            bit_depth: 8,
            channels: vec![
                Channel::new(tw, th),
                Channel::new(tw, th),
                Channel::new(count, 2),
                Channel::new(w, h),
            ],
            ..Default::default()
        };
        for ch in meta.channels.iter_mut().take(2) {
            ch.hshift = 3;
            ch.vshift = 3;
        }
        let stream_id = f
            .num_lf_groups
            .saturating_mul(2)
            .saturating_add(1)
            .saturating_add(g);
        decode_modular(br, &mut meta, stream_id, Some(&self.global), 1 << 30, true)?;
        let [ytox, ytob, strategy, sharpness] = meta.channels.as_slice() else {
            return Err(fault());
        };
        let (tx0, ty0) = (x0 >> 3, y0 >> 3);
        for y in 0..th {
            for x in 0..tw {
                let t = ty0
                    .saturating_add(y)
                    .saturating_mul(self.tiles_x)
                    .saturating_add(tx0)
                    .saturating_add(x);
                let s = y.saturating_mul(tw).saturating_add(x);
                let (Some(dx), Some(db), Some(&sx), Some(&sb)) = (
                    self.ytox_map.get_mut(t),
                    self.ytob_map.get_mut(t),
                    ytox.data.get(s),
                    ytob.data.get(s),
                ) else {
                    return Err(fault());
                };
                *dx = sx.clamp(-128, 127);
                *db = sb.clamp(-128, 127);
            }
        }
        let mut num = 0usize;
        let Some((acs_row, qf_row)) = strategy.data.split_at_checked(count) else {
            return Err(fault());
        };
        for iy in 0..h {
            for ix in 0..w {
                let (bx, by) = (x0.saturating_add(ix), y0.saturating_add(iy));
                let sh = sharpness
                    .data
                    .get(iy.saturating_mul(w).saturating_add(ix))
                    .copied();
                if !sh.is_some_and(|sh| (0..8).contains(&sh)) {
                    return Err(corrupt("EPF sharpness"));
                }
                let at = by.saturating_mul(f.xsize_blocks).saturating_add(bx);
                let (Some(acs), Some(qf)) = (self.acs.get_mut(at), self.qf.get_mut(at)) else {
                    return Err(fault());
                };
                if *acs != 0xFF {
                    continue;
                }
                let (Some(&strat), Some(&q)) = (acs_row.get(num), qf_row.get(num)) else {
                    return Err(corrupt("AC metadata count"));
                };
                if !(0..27).contains(&strat) {
                    return Err(corrupt("AC strategy"));
                }
                if strat as u8 != AC_STRATEGY_DCT8 {
                    return Err(unsupported(format!(
                        "AC strategy {strat} (only DCT8 is supported)"
                    )));
                }
                *acs = strat as u8;
                *qf = q.clamp(0, 255).saturating_add(1);
                num = num.saturating_add(1);
            }
        }
        br.check()
    }
    pub(crate) fn read_hf_global(&mut self, br: &mut BitReader) -> Result<(), Error> {
        if !br.bool() {
            if self.lf_precision.is_some() {
                return Err(unsupported("custom XYB AC quantization"));
            }
            for idx in 0..NUM_QUANT_TABLES {
                self.read_quant_encoding(br, idx)?;
            }
        }
        self.num_histo = (br.u(ceil_log2(self.f.num_groups as u64)) as usize).saturating_add(1);
        self.hf_codes = Vec::with_capacity(self.f.num_passes as usize);
        for _ in 0..self.f.num_passes {
            let used = br.u32(val(0x5F), val(0x13), val(0), bits_n(NUM_ORDERS as u32));
            self.read_coeff_orders(br, used)?;
            let contexts = self.num_histo.saturating_mul(self.bctx.num_ac_contexts());
            let code = read_entropy_code(br, contexts)?;
            self.hf_codes.push(code);
        }
        br.check()
    }
    fn read_quant_encoding(&mut self, br: &mut BitReader, idx: usize) -> Result<(), Error> {
        let mode = br.u(3);
        let (Some(&size_x), Some(&size_y)) =
            (QUANT_TABLE_SIZE_X.get(idx), QUANT_TABLE_SIZE_Y.get(idx))
        else {
            return Err(fault());
        };
        let single = size_x == 1 && size_y == 1;
        let skip = |br: &mut BitReader, n: usize| {
            for _ in 0..n {
                br.f16();
            }
        };
        let dct_params = |br: &mut BitReader| {
            let n = (br.u(4) as usize).saturating_add(1);
            for _ in 0..n.saturating_mul(3) {
                br.f16();
            }
        };
        match mode {
            0 => {}
            1..=5 if !single => return Err(corrupt("quant mode")),
            1 => skip(br, 9),
            2 => skip(br, 18),
            3 => {
                skip(br, 6);
                dct_params(br);
            }
            4 => {
                skip(br, 3);
                dct_params(br);
            }
            5 => {
                skip(br, 27);
                dct_params(br);
                dct_params(br);
            }
            6 => dct_params(br),
            7 => {
                let den = br.f16();
                let (w, h) = (size_x.saturating_mul(8), size_y.saturating_mul(8));
                let mut img = ModularImage {
                    bit_depth: 8,
                    channels: vec![Channel::new(w, h), Channel::new(w, h), Channel::new(w, h)],
                    ..Default::default()
                };
                let stream_id = self
                    .f
                    .num_lf_groups
                    .saturating_mul(3)
                    .saturating_add(1)
                    .saturating_add(idx);
                decode_modular(br, &mut img, stream_id, Some(&self.global), 1 << 30, true)?;
                if idx == 0 {
                    self.raw_q = true;
                    self.raw_q_table_den = den;
                    self.raw_q_table = Vec::with_capacity(w.saturating_mul(h).saturating_mul(3));
                    for ch in img.channels.iter().take(3) {
                        for &v in &ch.data {
                            if v <= 0 {
                                return Err(corrupt("raw quantization table"));
                            }
                            self.raw_q_table.push(v);
                        }
                    }
                }
            }
            _ => return Err(corrupt("quant encoding mode")),
        }
        br.check()
    }
    fn read_coeff_orders(&mut self, br: &mut BitReader, used: u32) -> Result<(), Error> {
        let code;
        let mut sr = None;
        if used != 0 {
            code = read_entropy_code(br, 8)?;
            sr = Some(SymbolReader::new(&code, br, 0));
        }
        for (ord, &llf) in ORDER_LLF.iter().enumerate() {
            let size = llf.saturating_mul(64);
            for order in self.order0.iter_mut() {
                if used & (1 << ord) == 0 {
                    if ord == 0 {
                        *order = natural_order_dct8().to_vec();
                    }
                    continue;
                }
                let Some(sr) = sr.as_mut() else {
                    return Err(fault());
                };
                let perm = read_permutation(br, sr, size, llf)?;
                if ord == 0 {
                    let nat = natural_order_dct8();
                    *order = perm
                        .iter()
                        .take(64)
                        .map(|&p| nat.get(p).copied().unwrap_or(0))
                        .collect();
                }
            }
        }
        if let Some(sr) = &sr
            && !sr.final_state_ok()
        {
            return Err(corrupt("coefficient order final state"));
        }
        br.check()
    }
    pub(crate) fn read_pass_group(
        &mut self,
        br: &mut BitReader,
        pass: usize,
        g: usize,
    ) -> Result<(), Error> {
        let f = &self.f;
        let histo = br.u(ceil_log2(self.num_histo as u64)) as usize;
        if histo >= self.num_histo {
            return Err(corrupt("histogram selector"));
        }
        let ctx_offset = histo.saturating_mul(self.bctx.num_ac_contexts());
        let Some(code) = self.hf_codes.get(pass) else {
            return Err(fault());
        };
        let mut sr = SymbolReader::new(code, br, 0);
        let gx = g.checked_rem(f.x_groups).unwrap_or(0);
        let gy = g.checked_div(f.x_groups).unwrap_or(0);
        let gb = f.group_dim / 8;
        let (bx0, by0) = (gx.saturating_mul(gb), gy.saturating_mul(gb));
        let bw = gb.min(f.xsize_blocks.saturating_sub(bx0));
        let bh = gb.min(f.ysize_blocks.saturating_sub(by0));
        let shift = f.pass_shift.get(pass).copied().unwrap_or(0);
        let area = gb.saturating_mul(gb);
        let mut nz: [Vec<i32>; 3] = [vec![0; area], vec![0; area], vec![0; area]];
        for by in 0..bh {
            br.check()?;
            for bx in 0..bw {
                let (abx, aby) = (bx0.saturating_add(bx), by0.saturating_add(by));
                let at = aby.saturating_mul(f.xsize_blocks).saturating_add(abx);
                if self.acs.get(at) != Some(&AC_STRATEGY_DCT8) {
                    return Err(unsupported("AC strategy"));
                }
                let qf_row_base = aby.saturating_mul(f.xsize_blocks);
                for c in [1usize, 0, 2] {
                    let (hs, vs) = (f.hshift(c), f.vshift(c));
                    let (sbx, sby) = (bx >> hs, by >> vs);
                    if sbx << hs != bx || sby << vs != by {
                        continue;
                    }
                    let (Some(nzc), Some(&dc_w), Some(coeffs), Some(order)) = (
                        nz.get_mut(c),
                        self.dc_w.get(c),
                        self.coeffs.get_mut(c),
                        self.order0.get(c),
                    ) else {
                        return Err(fault());
                    };
                    let pred = {
                        let row_at = sby.saturating_mul(gb);
                        let row = nzc.get(row_at..row_at.saturating_add(gb)).unwrap_or(&[]);
                        let top = (sby > 0)
                            .then(|| nzc.get(row_at.saturating_sub(gb)..row_at))
                            .flatten();
                        predict_non_zeros(top, row, sbx)
                    };
                    let qf = self
                        .qf
                        .get(qf_row_base.saturating_add(bx0).saturating_add(sbx))
                        .copied()
                        .unwrap_or(0);
                    let qdc = self.qdc.get(at).copied().unwrap_or(0);
                    let block_ctx = self.bctx.context(qdc as usize, qf, 0, c);
                    let nz_ctx = self
                        .bctx
                        .non_zero_context(pred, block_ctx)
                        .saturating_add(ctx_offset);
                    let mut nzeros = sr.read_uint(br, nz_ctx) as usize;
                    if nzeros > 63 {
                        return Err(corrupt("too many non-zero coefficients"));
                    }
                    if let Some(n) = nzc.get_mut(sby.saturating_mul(gb).saturating_add(sbx)) {
                        *n = nzeros as i32;
                    }
                    let hist_off =
                        ctx_offset.saturating_add(self.bctx.zero_density_offset(block_ctx));
                    let cbx = (bx0 >> hs).saturating_add(sbx);
                    let cby = (by0 >> vs).saturating_add(sby);
                    let base = cby
                        .saturating_mul(dc_w)
                        .saturating_add(cbx)
                        .saturating_mul(64);
                    let Some(block) = coeffs.get_mut(base..base.saturating_add(64)) else {
                        return Err(fault());
                    };
                    let mut prev = usize::from(nzeros <= 4);
                    let mut k = 1;
                    while k < 64 && nzeros != 0 {
                        let ctx = hist_off.saturating_add(zero_density_ctx(nzeros, k, prev));
                        let u = sr.read_uint(br, ctx);
                        let coeff = unpack_signed(u).wrapping_shl(shift);
                        if let Some(b) = order.get(k).and_then(|&o| block.get_mut(o)) {
                            *b = b.wrapping_add(coeff);
                        }
                        if u != 0 {
                            prev = 1;
                            nzeros = nzeros.saturating_sub(1);
                        } else {
                            prev = 0;
                        }
                        k = k.saturating_add(1);
                    }
                    if nzeros != 0 {
                        return Err(corrupt("non-zero count mismatch"));
                    }
                }
            }
            br.check()?;
        }
        if !sr.final_state_ok() {
            return Err(corrupt("HF ANS final state"));
        }
        br.check()
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn natural_order_dct8() -> [usize; 64] {
    let mut order = [0usize; 64];
    let mut n = 1;
    for i in 0..8 {
        for j in 0..=i {
            let (mut x, mut y) = (j, i - j);
            if i % 2 == 1 {
                std::mem::swap(&mut x, &mut y);
            }
            if x == 0 && y == 0 {
                continue;
            }
            order[n] = y * 8 + x;
            n += 1;
        }
    }
    for ip in (1..8).rev() {
        let i = ip - 1;
        for j in 0..=i {
            let (mut x, mut y) = (7 - (i - j), 7 - j);
            if i % 2 == 1 {
                std::mem::swap(&mut x, &mut y);
            }
            order[n] = y * 8 + x;
            n += 1;
        }
    }
    order
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn predict_non_zeros(top: Option<&[i32]>, cur: &[i32], x: usize) -> usize {
    match (x, top) {
        (0, None) => 32,
        (0, Some(t)) => t[0] as usize,
        (_, None) => cur[x - 1] as usize,
        (_, Some(t)) => ((t[x] + cur[x - 1] + 1) / 2) as usize,
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn zero_density_ctx(non_zeros_left: usize, k: usize, prev: usize) -> usize {
    (COEFF_NUM_NONZERO_CONTEXT[non_zeros_left] + COEFF_FREQ_CONTEXT[k]) * 2 + prev
}
