use super::jbrd::{JpegData, JpegHuffmanCode, JpegScanInfo};
use super::{Error, corrupt, unsupported};
pub(crate) const JPEG_NATURAL_ORDER: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
#[derive(Clone)]
struct HuffTable {
    depth: [u8; 256],
    code: [u16; 256],
    ok: bool,
}
impl Default for HuffTable {
    fn default() -> Self {
        HuffTable {
            depth: [0; 256],
            code: [0; 256],
            ok: false,
        }
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn build_huff_table(h: &JpegHuffmanCode) -> Option<HuffTable> {
    let mut sizes = Vec::new();
    for l in 1..=16u8 {
        for _ in 0..h.counts[l as usize] {
            sizes.push(l);
        }
    }
    if h.counts[0] != 0 || sizes.len() != h.values.len() || sizes.is_empty() {
        return None;
    }
    let mut t = HuffTable::default();
    let mut code = 0usize;
    let mut si = sizes[0];
    let mut p = 0;
    let mut codes = vec![0usize; sizes.len()];
    while p < sizes.len() {
        while p < sizes.len() && sizes[p] == si {
            codes[p] = code;
            code += 1;
            p += 1;
        }
        code <<= 1;
        si = si.wrapping_add(1);
    }
    for p in 0..sizes.len() - 1 {
        let v = h.values[p];
        if v > 255 {
            return None;
        }
        t.depth[v as usize] = sizes[p];
        t.code[v as usize] = codes[p] as u16;
    }
    t.ok = true;
    Some(t)
}
struct JpegBitWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
    ok: bool,
}
impl JpegBitWriter {
    fn emit(&mut self, b: u8) {
        self.out.push(b);
        if b == 0xFF {
            self.out.push(0);
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn write_bits(&mut self, n: u32, v: u64) {
        if n == 0 {
            return;
        }
        self.acc = (self.acc << n) | (v & ((1u64 << n) - 1));
        self.nbits += n;
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.emit((self.acc >> self.nbits) as u8);
        }
        self.acc &= (1u64 << self.nbits) - 1;
    }
    #[allow(clippy::indexing_slicing)]
    fn write_symbol(&mut self, sym: i64, t: &HuffTable) {
        if !(0..=255).contains(&sym) || t.depth[sym as usize] == 0 {
            self.ok = false;
            return;
        }
        self.write_bits(
            u32::from(t.depth[sym as usize]),
            u64::from(t.code[sym as usize]),
        );
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn pad_to_byte(&mut self, j: &JpegData, pad_pos: &mut usize) -> bool {
        let n = (8 - self.nbits % 8) % 8;
        if n == 0 {
            return true;
        }
        let mut pattern = 0u64;
        if j.has_zero_padding {
            for _ in 0..n {
                let Some(&b) = j.padding_bits.get(*pad_pos) else {
                    return false;
                };
                pattern = (pattern << 1) | u64::from(b & 1);
                *pad_pos += 1;
            }
        } else {
            pattern = (1 << n) - 1;
        }
        self.write_bits(n, pattern);
        true
    }
}
#[derive(Default)]
struct CodingState {
    eob_run: i64,
    cur_ac: Option<usize>,
    ref_bits: Vec<u8>,
}
const MAX_CORRECTION_BITS: usize = 1 << 16;
impl CodingState {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn flush(&mut self, w: &mut JpegBitWriter, ac: &[HuffTable; 4]) {
        if self.eob_run > 0 {
            let nb = 31 - (self.eob_run as u32).leading_zeros();
            match self.cur_ac {
                Some(t) => w.write_symbol(i64::from(nb << 4), &ac[t]),
                None => w.ok = false,
            }
            w.write_bits(nb, self.eob_run as u64);
            self.eob_run = 0;
        }
        for &b in &self.ref_bits {
            w.write_bits(1, u64::from(b));
        }
        self.ref_bits.clear();
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn buffer_eob(
        &mut self,
        ac_idx: usize,
        ac: &[HuffTable; 4],
        new_bits: &[u8],
        w: &mut JpegBitWriter,
    ) {
        if self.eob_run == 0 {
            self.cur_ac = Some(ac_idx);
        }
        self.eob_run += 1;
        self.ref_bits.extend_from_slice(new_bits);
        if self.eob_run == 0x7FFF || self.ref_bits.len() > MAX_CORRECTION_BITS - 64 + 1 {
            self.flush(w, ac);
        }
    }
}
struct Serializer<'j> {
    j: &'j JpegData,
    out: Vec<u8>,
    dc: [HuffTable; 4],
    ac: [HuffTable; 4],
    dht_index: usize,
    dqt_index: usize,
    app_index: usize,
    com_index: usize,
    inter_index: usize,
    scan_index: usize,
    pad_pos: usize,
    progressive: bool,
    seen_dri: bool,
}
pub(crate) fn write_jpeg(j: &JpegData) -> Result<Vec<u8>, Error> {
    let mut st = Serializer {
        j,
        out: vec![0xFF, 0xD8],
        dc: Default::default(),
        ac: Default::default(),
        dht_index: 0,
        dqt_index: 0,
        app_index: 0,
        com_index: 0,
        inter_index: 0,
        scan_index: 0,
        pad_pos: 0,
        progressive: false,
        seen_dri: false,
    };
    for &m in &j.marker_order {
        match m {
            0xC0..=0xC2 => st.sof(m),
            0xC4 => st.dht()?,
            0xD0..=0xD7 => st.out.extend_from_slice(&[0xFF, m]),
            0xD9 => {
                st.out.extend_from_slice(&[0xFF, 0xD9]);
                st.out.extend_from_slice(&j.tail_data);
            }
            0xDA => st.scan()?,
            0xDB => st.dqt()?,
            0xDD => {
                st.seen_dri = true;
                let r = j.restart_interval;
                st.out
                    .extend_from_slice(&[0xFF, 0xDD, 0, 4, (r >> 8) as u8, r as u8]);
            }
            0xE0..=0xEF => {
                let Some(a) = j.app_data.get(st.app_index) else {
                    return Err(corrupt("APP marker count"));
                };
                st.out.push(0xFF);
                st.out.extend_from_slice(a);
                st.app_index = st.app_index.saturating_add(1);
            }
            0xFE => {
                let Some(c) = j.com_data.get(st.com_index) else {
                    return Err(corrupt("COM marker count"));
                };
                st.out.push(0xFF);
                st.out.extend_from_slice(c);
                st.com_index = st.com_index.saturating_add(1);
            }
            0xFF => {
                let Some(d) = j.inter_marker.get(st.inter_index) else {
                    return Err(corrupt("inter-marker data count"));
                };
                st.out.extend_from_slice(d);
                st.inter_index = st.inter_index.saturating_add(1);
            }
            _ => return Err(unsupported(format!("JPEG marker {m:#x}"))),
        }
    }
    if j.has_zero_padding && st.pad_pos != j.padding_bits.len() {
        return Err(corrupt("unused JPEG padding bits"));
    }
    Ok(st.out)
}
pub(crate) fn mcu_size(j: &JpegData, s: &JpegScanInfo) -> (usize, usize) {
    let interleaved = s.num_components > 1;
    let Some(base) = j.components.get(s.components[0].comp_idx as usize) else {
        return (0, 0);
    };
    let (hg, vg) = if interleaved {
        (1, 1)
    } else {
        (base.h_samp, base.v_samp)
    };
    let max_h = j.components.iter().map(|c| c.h_samp).fold(1, usize::max);
    let max_v = j.components.iter().map(|c| c.v_samp).fold(1, usize::max);
    (
        j.width.saturating_mul(hg).div_ceil(max_h.saturating_mul(8)),
        j.height
            .saturating_mul(vg)
            .div_ceil(max_v.saturating_mul(8)),
    )
}
impl Serializer<'_> {
    fn sof(&mut self, marker: u8) {
        let j = self.j;
        self.progressive = marker == 0xC2;
        let n = j.components.len();
        let length = n.saturating_mul(3).saturating_add(8);
        self.out.extend_from_slice(&[
            0xFF,
            marker,
            (length >> 8) as u8,
            length as u8,
            8,
            (j.height >> 8) as u8,
            j.height as u8,
            (j.width >> 8) as u8,
            j.width as u8,
            n as u8,
        ]);
        for c in &j.components {
            let q = j.quant.get(c.quant_idx as usize).map_or(0, |q| q.index);
            self.out
                .extend_from_slice(&[c.id as u8, (c.h_samp << 4 | c.v_samp) as u8, q as u8]);
        }
    }
    fn dht(&mut self) -> Result<(), Error> {
        let j = self.j;
        let mut length = 2usize;
        for h in j.huffman.get(self.dht_index..).unwrap_or(&[]) {
            let values = h
                .counts
                .iter()
                .fold(0usize, |s, &c| s.saturating_add(c as usize));
            length = length.saturating_add(16).saturating_add(values);
            if h.is_last {
                break;
            }
        }
        self.out
            .extend_from_slice(&[0xFF, 0xC4, (length >> 8) as u8, length as u8]);
        loop {
            let Some(h) = j.huffman.get(self.dht_index) else {
                return Err(corrupt("DHT marker count"));
            };
            self.dht_index = self.dht_index.saturating_add(1);
            let Some(t) = build_huff_table(h) else {
                return Err(corrupt("Huffman table"));
            };
            let tables = if h.slot_id & 0x10 != 0 {
                &mut self.ac
            } else {
                &mut self.dc
            };
            if let Some(slot) = tables.get_mut((h.slot_id & 3) as usize) {
                *slot = t;
            }
            let max_len = h.counts.iter().rposition(|&c| c != 0).unwrap_or(0);
            let total = h
                .counts
                .iter()
                .fold(0usize, |s, &c| s.saturating_add(c as usize));
            self.out.push(h.slot_id as u8);
            for (l, &count) in h.counts.iter().enumerate().skip(1) {
                let mut c = count;
                if l == max_len {
                    c = c.wrapping_sub(1);
                }
                self.out.push(c as u8);
            }
            for &v in h.values.get(..total.saturating_sub(1)).unwrap_or(&[]) {
                self.out.push(v as u8);
            }
            if h.is_last {
                return Ok(());
            }
        }
    }
    fn dqt(&mut self) -> Result<(), Error> {
        let j = self.j;
        let mut length = 2usize;
        for q in j.quant.get(self.dqt_index..).unwrap_or(&[]) {
            let bytes = if q.precision != 0 { 129 } else { 65 };
            length = length.saturating_add(bytes);
            if q.is_last {
                break;
            }
        }
        self.out
            .extend_from_slice(&[0xFF, 0xDB, (length >> 8) as u8, length as u8]);
        loop {
            let Some(q) = j.quant.get(self.dqt_index) else {
                return Err(corrupt("DQT marker count"));
            };
            self.dqt_index = self.dqt_index.saturating_add(1);
            self.out.push((q.precision << 4 | q.index) as u8);
            for &nat in &JPEG_NATURAL_ORDER {
                let v = q.values.get(nat).copied().unwrap_or(0);
                if q.precision != 0 {
                    self.out.push((v >> 8) as u8);
                }
                self.out.push(v as u8);
            }
            if q.is_last {
                return Ok(());
            }
        }
    }
    fn scan(&mut self) -> Result<(), Error> {
        let j = self.j;
        let Some(s) = j.scans.get(self.scan_index) else {
            return Err(corrupt("scan count"));
        };
        self.scan_index = self.scan_index.saturating_add(1);
        let n = s.num_components as usize;
        let Some(scan_components) = s.components.get(..n) else {
            return Err(corrupt("scan component count"));
        };
        let length = n.saturating_mul(2).saturating_add(6);
        self.out
            .extend_from_slice(&[0xFF, 0xDA, (length >> 8) as u8, length as u8, n as u8]);
        for c in scan_components {
            let Some(component) = j.components.get(c.comp_idx as usize) else {
                return Err(corrupt("scan component"));
            };
            self.out
                .extend_from_slice(&[component.id as u8, (c.dc_tbl << 4 | c.ac_tbl) as u8]);
        }
        self.out
            .extend_from_slice(&[s.ss as u8, s.se as u8, (s.ah << 4 | s.al) as u8]);
        let (mut al, mut ah, mut ss, mut se) = (0usize, 0usize, 0usize, 63usize);
        if self.progressive {
            (al, ah, ss, se) = (s.al as usize, s.ah as usize, s.ss as usize, s.se as usize);
        }
        let mut mode = 0;
        if self.progressive && !(ah == 0 && al == 0 && ss == 0 && se == 63) {
            mode = if ah != 0 { 2 } else { 1 };
        }
        if se > 63 {
            return Err(corrupt("scan spectral selection"));
        }
        let restart = if self.seen_dri {
            j.restart_interval as i64
        } else {
            0
        };
        let mut w = JpegBitWriter {
            out: std::mem::take(&mut self.out),
            acc: 0,
            nbits: 0,
            ok: true,
        };
        let mut cs = CodingState::default();
        let mut last_dc = [0i32; 4];
        let mut restarts_to_go = restart;
        let mut next_rst = 0u8;
        let mut block_scan_index: i64 = 0;
        let (mut reset_pos, mut zero_pos) = (0usize, 0usize);
        let mut next_reset: i64 = -1;
        if let Some(&r) = s.reset_points.first() {
            next_reset = i64::from(r);
            reset_pos = 1;
        }
        let mut next_zero: i64 = s
            .extra_zero_runs
            .first()
            .map_or(-1, |z| i64::from(z.block_idx));
        let (per_row, rows) = mcu_size(j, s);
        let interleaved = n > 1;
        for mcu_y in 0..rows {
            for mcu_x in 0..per_row {
                if restart > 0 && restarts_to_go == 0 {
                    cs.flush(&mut w, &self.ac);
                    if !w.pad_to_byte(j, &mut self.pad_pos) {
                        return Err(corrupt("JPEG padding bits exhausted"));
                    }
                    w.out.extend_from_slice(&[0xFF, 0xD0 | next_rst]);
                    next_rst = next_rst.wrapping_add(1) & 7;
                    restarts_to_go = restart;
                    last_dc = [0; 4];
                }
                for sc in scan_components {
                    let Some(comp) = j.components.get(sc.comp_idx as usize) else {
                        return Err(corrupt("scan component"));
                    };
                    let (dc_t, ac_t) = (sc.dc_tbl as usize, sc.ac_tbl as usize);
                    let (Some(dc_table), Some(ac_table)) = (self.dc.get(dc_t), self.ac.get(ac_t))
                    else {
                        return Err(corrupt("Huffman table missing for scan"));
                    };
                    let want_dc = ss == 0;
                    let want_ac = ss != 0 || se != 0;
                    if (want_dc && mode != 2 && !dc_table.ok) || (want_ac && !ac_table.ok) {
                        return Err(corrupt("Huffman table missing for scan"));
                    }
                    let (nby, nbx) = if interleaved {
                        (comp.v_samp, comp.h_samp)
                    } else {
                        (1, 1)
                    };
                    for iy in 0..nby {
                        for ix in 0..nbx {
                            let block_y = mcu_y.saturating_mul(nby).saturating_add(iy);
                            let block_x = mcu_x.saturating_mul(nbx).saturating_add(ix);
                            if block_x >= comp.width_blocks || block_y >= comp.height_blocks {
                                return Err(corrupt("JPEG block outside the component"));
                            }
                            let block_idx = block_y
                                .saturating_mul(comp.width_blocks)
                                .saturating_add(block_x);
                            if block_scan_index == next_reset {
                                cs.flush(&mut w, &self.ac);
                                next_reset = -1;
                                if let Some(&r) = s.reset_points.get(reset_pos) {
                                    next_reset = i64::from(r);
                                    reset_pos = reset_pos.saturating_add(1);
                                }
                            }
                            let mut zero_runs = 0;
                            if block_scan_index == next_zero {
                                zero_runs = s
                                    .extra_zero_runs
                                    .get(zero_pos)
                                    .map_or(0, |z| z.num_runs as usize);
                                zero_pos = zero_pos.saturating_add(1);
                                next_zero = s
                                    .extra_zero_runs
                                    .get(zero_pos)
                                    .map_or(-1, |z| i64::from(z.block_idx));
                            }
                            let at = block_idx.saturating_mul(64);
                            let Some(coeffs) = comp
                                .coeffs
                                .get(at..at.saturating_add(64))
                                .and_then(|c| <&[i16; 64]>::try_from(c).ok())
                            else {
                                return Err(super::fault());
                            };
                            let Some(last) = last_dc.get_mut(sc.comp_idx as usize) else {
                                return Err(corrupt("scan component"));
                            };
                            let ok = match mode {
                                0 => encode_sequential(
                                    coeffs, dc_table, ac_table, zero_runs, last, &mut w,
                                ),
                                1 => {
                                    let p = Progressive {
                                        ss,
                                        se,
                                        al,
                                        zero_runs,
                                    };
                                    encode_progressive(
                                        coeffs, dc_t, ac_t, p, &self.dc, &self.ac, &mut cs, last,
                                        &mut w,
                                    )
                                }
                                _ => encode_refinement(
                                    coeffs, ac_t, &self.ac, ss, se, al, &mut cs, &mut w,
                                ),
                            };
                            if !ok || !w.ok {
                                return Err(corrupt("JPEG coefficient cannot be encoded"));
                            }
                            block_scan_index = block_scan_index.saturating_add(1);
                        }
                    }
                }
                restarts_to_go = restarts_to_go.saturating_sub(1);
            }
        }
        cs.flush(&mut w, &self.ac);
        if !w.pad_to_byte(j, &mut self.pad_pos) {
            return Err(corrupt("JPEG padding bits exhausted"));
        }
        if !w.ok {
            return Err(corrupt("JPEG symbol missing from Huffman table"));
        }
        self.out = w.out;
        Ok(())
    }
}
fn magnitude(v: i32) -> (u32, u32) {
    if v < 0 {
        (v.wrapping_neg() as u32, v.wrapping_sub(1) as u32)
    } else {
        (v as u32, v as u32)
    }
}
fn bit_len(v: u32) -> u32 {
    u32::BITS.saturating_sub(v.leading_zeros())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn encode_sequential(
    coeffs: &[i16; 64],
    dc_t: &HuffTable,
    ac_t: &HuffTable,
    zero_runs: usize,
    last_dc: &mut i32,
    w: &mut JpegBitWriter,
) -> bool {
    let dc = i32::from(coeffs[0]);
    let diff = dc.wrapping_sub(*last_dc);
    *last_dc = dc;
    let (mag, value) = magnitude(diff);
    let nb = bit_len(mag);
    w.write_symbol(i64::from(nb), dc_t);
    if nb >= 12 {
        return false;
    }
    w.write_bits(nb, u64::from(value));
    let mut r: i64 = 0;
    for &nat in &JPEG_NATURAL_ORDER[1..] {
        let c = i32::from(coeffs[nat]);
        if c == 0 {
            r += 1;
            continue;
        }
        while r > 15 {
            w.write_symbol(0xF0, ac_t);
            r -= 16;
        }
        let (mag, value) = magnitude(c);
        let nb = bit_len(mag);
        if nb > 15 {
            return false;
        }
        w.write_symbol(r << 4 | i64::from(nb), ac_t);
        w.write_bits(nb, u64::from(value));
        r = 0;
    }
    for _ in 0..zero_runs {
        w.write_symbol(0xF0, ac_t);
        r -= 16;
    }
    if r > 0 {
        w.write_symbol(0, ac_t);
    }
    true
}
#[derive(Clone, Copy)]
struct Progressive {
    ss: usize,
    se: usize,
    al: usize,
    zero_runs: usize,
}
#[allow(clippy::too_many_arguments)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn encode_progressive(
    coeffs: &[i16; 64],
    dc_t: usize,
    ac_t: usize,
    p: Progressive,
    dc: &[HuffTable; 4],
    ac: &[HuffTable; 4],
    cs: &mut CodingState,
    last_dc: &mut i32,
    w: &mut JpegBitWriter,
) -> bool {
    let Progressive {
        mut ss,
        se,
        al,
        zero_runs,
    } = p;
    let eob_allowed = ss > 0;
    let al_shift = al.min(31) as u32;
    if ss == 0 {
        let d = i32::from(coeffs[0]) >> al_shift;
        let diff = d.wrapping_sub(*last_dc);
        *last_dc = d;
        let (mag, value) = magnitude(diff);
        let nb = bit_len(mag);
        w.write_symbol(i64::from(nb), &dc[dc_t]);
        w.write_bits(nb, u64::from(value));
        ss += 1;
    }
    if ss > se {
        return true;
    }
    let mut r: i64 = 0;
    for k in ss..=se {
        let c = i32::from(coeffs[JPEG_NATURAL_ORDER[k]]);
        if c == 0 {
            r += 1;
            continue;
        }
        let (t, t2) = if c < 0 {
            let t = (-c) >> al_shift;
            (t, !t)
        } else {
            let t = c >> al_shift;
            (t, t)
        };
        if t == 0 {
            r += 1;
            continue;
        }
        cs.flush(w, ac);
        while r > 15 {
            w.write_symbol(0xF0, &ac[ac_t]);
            r -= 16;
        }
        let nb = bit_len(t as u32);
        w.write_symbol(r << 4 | i64::from(nb), &ac[ac_t]);
        w.write_bits(nb, u64::from(t2 as u32));
        r = 0;
    }
    if zero_runs > 0 {
        cs.flush(w, ac);
        for _ in 0..zero_runs {
            w.write_symbol(0xF0, &ac[ac_t]);
            r -= 16;
        }
    }
    if r > 0 {
        cs.buffer_eob(ac_t, ac, &[], w);
        if !eob_allowed {
            cs.flush(w, ac);
        }
    }
    true
}
#[allow(clippy::too_many_arguments)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn encode_refinement(
    coeffs: &[i16; 64],
    ac_t: usize,
    ac: &[HuffTable; 4],
    mut ss: usize,
    se: usize,
    al: usize,
    cs: &mut CodingState,
    w: &mut JpegBitWriter,
) -> bool {
    let eob_allowed = ss > 0;
    let al_shift = al.min(31) as u32;
    if ss == 0 {
        w.write_bits(1, ((i32::from(coeffs[0]) >> al_shift) & 1) as u64);
        ss += 1;
    }
    if ss > se {
        return true;
    }
    let mut abs_vals = [0i32; 64];
    let mut eob = 0;
    for k in ss..=se {
        let v = i32::from(coeffs[JPEG_NATURAL_ORDER[k]]).abs();
        abs_vals[k] = v >> al_shift;
        if abs_vals[k] == 1 {
            eob = k;
        }
    }
    let mut r: i64 = 0;
    let mut ref_bits: Vec<u8> = Vec::new();
    for k in ss..=se {
        if abs_vals[k] == 0 {
            r += 1;
            continue;
        }
        while r > 15 && k <= eob {
            cs.flush(w, ac);
            w.write_symbol(0xF0, &ac[ac_t]);
            r -= 16;
            for &b in &ref_bits {
                w.write_bits(1, u64::from(b));
            }
            ref_bits.clear();
        }
        if abs_vals[k] > 1 {
            ref_bits.push((abs_vals[k] & 1) as u8);
            continue;
        }
        cs.flush(w, ac);
        let bit = u64::from(coeffs[JPEG_NATURAL_ORDER[k]] >= 0);
        w.write_symbol(r << 4 | 1, &ac[ac_t]);
        w.write_bits(1, bit);
        for &b in &ref_bits {
            w.write_bits(1, u64::from(b));
        }
        ref_bits.clear();
        r = 0;
    }
    if r > 0 || !ref_bits.is_empty() {
        cs.buffer_eob(ac_t, ac, &ref_bits, w);
        if !eob_allowed {
            cs.flush(w, ac);
        }
    }
    true
}
