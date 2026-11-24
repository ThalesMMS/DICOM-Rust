use super::jbrd::{
    APP_UNKNOWN, ExtraZeroRun, JpegComponent, JpegData, JpegHuffmanCode, JpegQuantTable,
    JpegScanComponent, JpegScanInfo,
};
use super::jpegwriter::{JPEG_NATURAL_ORDER, mcu_size};
use super::{Error, ErrorKind};
use std::collections::HashMap;
pub(crate) fn not_reversible(detail: impl Into<String>) -> Error {
    Error::new(ErrorKind::NotReversible, Some(detail.into()))
}
#[derive(Clone, Default)]
struct HuffDecoder {
    max_code: [i32; 18],
    val_ptr: [i32; 17],
    min_code: [i32; 17],
    values: Vec<u8>,
    ok: bool,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn new_huff_decoder(h: &JpegHuffmanCode) -> HuffDecoder {
    let mut d = HuffDecoder::default();
    let mut code: i32 = 0;
    let mut k: i32 = 0;
    for l in 1..=16 {
        let n = h.counts[l] as i32;
        d.val_ptr[l] = k;
        d.min_code[l] = code;
        code += n;
        k += n;
        d.max_code[l] = if n > 0 { code - 1 } else { -1 };
        code <<= 1;
    }
    d.max_code[17] = 0x7FFF_FFFF;
    d.values = h
        .values
        .iter()
        .filter(|&&v| v < 256)
        .map(|&v| v as u8)
        .collect();
    d.ok = true;
    d
}
struct ScanReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    nbits: u32,
    marker: bool,
    fake: u32,
    fake_all: u32,
    pad_bits: Vec<u8>,
}
impl<'a> ScanReader<'a> {
    fn new(data: &'a [u8], pos: usize, pad_bits: Vec<u8>) -> Self {
        ScanReader {
            data,
            pos,
            acc: 0,
            nbits: 0,
            marker: false,
            fake: 0,
            fake_all: 0,
            pad_bits,
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn fill(&mut self) {
        while self.nbits <= 24 {
            if self.marker || self.pos >= self.data.len() {
                self.marker = true;
                self.nbits += 8;
                self.fake += 8;
                self.fake_all = self.fake_all.wrapping_add(8);
                continue;
            }
            let b = self.data[self.pos];
            if b == 0xFF {
                if self.pos + 1 < self.data.len() && self.data[self.pos + 1] == 0 {
                    self.pos += 2;
                } else {
                    self.marker = true;
                    self.nbits += 8;
                    self.fake += 8;
                    self.fake_all = self.fake_all.wrapping_add(8);
                    continue;
                }
            } else {
                self.pos += 1;
            }
            self.acc |= u32::from(b) << (24 - self.nbits);
            self.nbits += 8;
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.fill();
        let v = self.acc >> (32 - n);
        self.acc = self.acc.checked_shl(n).unwrap_or(0);
        self.nbits -= n;
        if self.fake > self.nbits {
            self.fake = self.nbits;
        }
        v
    }
    fn overrun(&self) -> bool {
        self.fake < self.fake_all
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode(&mut self, d: &HuffDecoder) -> Option<usize> {
        let mut code: i32 = 0;
        for l in 1..=16 {
            code = (code << 1) | self.bits(1) as i32;
            if code <= d.max_code[l] {
                let idx = d.val_ptr[l] + code - d.min_code[l];
                return d
                    .values
                    .get(usize::try_from(idx).ok()?)
                    .map(|&v| usize::from(v));
            }
        }
        None
    }
    fn align_to_byte(&mut self) {
        let extra = self.nbits % 8;
        for _ in 0..extra {
            let b = self.bits(1) as u8;
            self.pad_bits.push(b);
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn byte_pos(&self) -> usize {
        let mut pos = self.pos;
        let n = (self.nbits - self.fake) / 8;
        for _ in 0..n {
            pos -= 1;
            if pos > 0 && self.data[pos] == 0 && self.data[pos - 1] == 0xFF {
                pos -= 1;
            }
        }
        pos
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn extend_sign(v: u32, n: u32) -> i32 {
    if n == 0 {
        return 0;
    }
    if v < 1 << (n - 1) {
        return (v as i32).wrapping_sub(1 << n).wrapping_add(1);
    }
    v as i32
}
pub(crate) struct ParsedJpeg {
    pub data: JpegData,
    pub sof_marker: u8,
}
fn be16(b: &[u8]) -> usize {
    b.first_chunk::<2>()
        .map_or(0, |&x| usize::from(u16::from_be_bytes(x)))
}
pub(crate) fn parse_jpeg(b: &[u8]) -> Result<ParsedJpeg, Error> {
    if b.len() < 4 || !b.starts_with(&[0xFF, 0xD8]) {
        return Err(not_reversible("missing SOI marker"));
    }
    let mut j = JpegData::default();
    let mut sof_marker = 0u8;
    let mut pos = 2;
    let mut slot_table: HashMap<u32, usize> = HashMap::new();
    let mut dc_dec: [HuffDecoder; 4] = Default::default();
    let mut ac_dec: [HuffDecoder; 4] = Default::default();
    let mut saw_sof = false;
    let mut progressive = false;
    let mut pad_bits: Vec<u8> = Vec::new();
    loop {
        let Some(rest) = b.get(pos..).filter(|r| !r.is_empty()) else {
            return Err(not_reversible("missing EOI marker"));
        };
        if rest.first() != Some(&0xFF) {
            let run = rest.iter().position(|&c| c == 0xFF).unwrap_or(rest.len());
            if j.marker_order.is_empty() || run > 0xFFFF {
                return Err(not_reversible("data between markers"));
            }
            j.marker_order.push(0xFF);
            j.inter_marker.push(rest.get(..run).unwrap_or(&[]).to_vec());
            pos = pos.saturating_add(run);
            continue;
        }
        let Some(&m) = rest.get(1) else {
            return Err(not_reversible("truncated marker"));
        };
        let seg_start = pos.saturating_add(1);
        pos = pos.saturating_add(2);
        if m == 0xD9 {
            j.marker_order.push(m);
            j.tail_data = b.get(pos..).unwrap_or(&[]).to_vec();
            break;
        }
        if m < 0xC0 || m == 0xFF || (0xD0..=0xD8).contains(&m) {
            return Err(not_reversible(format!("unexpected marker {m:#x}")));
        }
        let Some(body) = b.get(pos..).filter(|r| r.len() >= 2) else {
            return Err(not_reversible("truncated segment"));
        };
        let length = be16(body);
        let Some(mut seg) = body.get(2..length).filter(|_| length >= 2) else {
            return Err(not_reversible("segment length"));
        };
        pos = pos.saturating_add(length);
        let segment = || b.get(seg_start..pos).unwrap_or(&[]).to_vec();
        match m {
            0xE0..=0xEF => {
                j.marker_order.push(m);
                j.app_data.push(segment());
                j.app_marker_type.push(APP_UNKNOWN);
            }
            0xFE => {
                j.marker_order.push(m);
                j.com_data.push(segment());
            }
            0xDB => {
                j.marker_order.push(m);
                let first = j.quant.len();
                while let Some((&pqtq, body)) = seg.split_first() {
                    let (pq, tq) = (u32::from(pqtq >> 4), u32::from(pqtq & 15));
                    if pq > 1 || tq > 3 {
                        return Err(not_reversible("DQT table"));
                    }
                    let Some((table, rest)) = body.split_at_checked(if pq == 1 { 128 } else { 64 })
                    else {
                        return Err(not_reversible("truncated DQT"));
                    };
                    let mut q = JpegQuantTable {
                        precision: pq,
                        index: tq,
                        ..Default::default()
                    };
                    for (i, &nat) in JPEG_NATURAL_ORDER.iter().enumerate() {
                        let v = if pq == 1 {
                            be16(table.get(i.saturating_mul(2)..).unwrap_or(&[])) as i32
                        } else {
                            table.get(i).map_or(0, |&v| i32::from(v))
                        };
                        if v == 0 {
                            return Err(not_reversible("zero quantizer"));
                        }
                        if let Some(slot) = q.values.get_mut(nat) {
                            *slot = v;
                        }
                    }
                    if slot_table.contains_key(&tq) {
                        return Err(not_reversible("redefined quantization table"));
                    }
                    slot_table.insert(tq, j.quant.len());
                    j.quant.push(q);
                    seg = rest;
                }
                if j.quant.len() == first {
                    return Err(not_reversible("empty DQT"));
                }
                if let Some(q) = j.quant.last_mut() {
                    q.is_last = true;
                }
            }
            0xC4 => {
                j.marker_order.push(m);
                let first = j.huffman.len();
                while !seg.is_empty() {
                    let Some((&[tcth, ref counts @ ..], rest)) = seg.split_first_chunk::<17>()
                    else {
                        return Err(not_reversible("truncated DHT"));
                    };
                    let (tc, th) = (tcth >> 4, tcth & 15);
                    if tc > 1 || th > 3 {
                        return Err(not_reversible("DHT table"));
                    }
                    let mut h = JpegHuffmanCode {
                        slot_id: u32::from(tc) << 4 | u32::from(th),
                        ..Default::default()
                    };
                    let mut total = 0usize;
                    let mut max_len = 0;
                    let mut space: i64 = 1 << 16;
                    for ((l, &count), slot) in
                        (1usize..).zip(counts).zip(h.counts.iter_mut().skip(1))
                    {
                        *slot = u32::from(count);
                        total = total.saturating_add(usize::from(count));
                        if count != 0 {
                            max_len = l;
                        }
                        space = space.saturating_sub(i64::from(count) << 16usize.saturating_sub(l));
                    }
                    let values = rest
                        .get(..total)
                        .filter(|_| total != 0 && space > 0 && total <= 256);
                    let Some(values) = values else {
                        return Err(not_reversible("DHT code"));
                    };
                    for &v in values {
                        if tc == 0 && v > 11 {
                            return Err(not_reversible("DC Huffman symbol"));
                        }
                        h.values.push(u32::from(v));
                    }
                    if let Some(c) = h.counts.get_mut(max_len) {
                        *c = c.saturating_add(1);
                    }
                    h.values.push(256);
                    let d = new_huff_decoder(&h);
                    let decoders = if tc == 0 { &mut dc_dec } else { &mut ac_dec };
                    if let Some(slot) = decoders.get_mut(th as usize) {
                        *slot = d;
                    }
                    j.huffman.push(h);
                    seg = rest.get(total..).unwrap_or(&[]);
                }
                if j.huffman.len() == first {
                    return Err(not_reversible("empty DHT"));
                }
                if let Some(h) = j.huffman.last_mut() {
                    h.is_last = true;
                }
            }
            0xDD => {
                if seg.len() != 2 {
                    return Err(not_reversible("DRI length"));
                }
                let ri = be16(seg) as u32;
                if j.marker_order.contains(&0xDD) && j.restart_interval != ri {
                    return Err(not_reversible("changing restart interval"));
                }
                j.marker_order.push(m);
                j.restart_interval = ri;
            }
            0xC0..=0xC2 => {
                if saw_sof {
                    return Err(not_reversible("multiple frames"));
                }
                saw_sof = true;
                progressive = m == 0xC2;
                j.marker_order.push(m);
                sof_marker = m;
                let Some((&[precision, y0, y1, x0, x1, nc], entries)) =
                    seg.split_first_chunk::<6>()
                else {
                    return Err(not_reversible("SOF length"));
                };
                if precision != 8 {
                    return Err(not_reversible(format!("{precision}-bit JPEG")));
                }
                j.height = usize::from(u16::from_be_bytes([y0, y1]));
                j.width = usize::from(u16::from_be_bytes([x0, x1]));
                let nc = usize::from(nc);
                let (entries, tail) = entries.as_chunks::<3>();
                if j.width == 0
                    || j.height == 0
                    || (nc != 1 && nc != 3)
                    || entries.len() != nc
                    || !tail.is_empty()
                {
                    return Err(not_reversible("SOF parameters"));
                }
                let (mut max_h, mut max_v) = (1, 1);
                for &[id, hv, tq] in entries {
                    let mut comp = JpegComponent {
                        id: u32::from(id),
                        h_samp: usize::from(hv >> 4),
                        v_samp: usize::from(hv & 15),
                        ..Default::default()
                    };
                    if !(1..=2).contains(&comp.h_samp) || !(1..=2).contains(&comp.v_samp) {
                        return Err(not_reversible("sampling factors"));
                    }
                    let Some(&qi) = slot_table.get(&u32::from(tq)) else {
                        return Err(not_reversible("component quantization table undefined"));
                    };
                    comp.quant_idx = qi as u32;
                    max_h = max_h.max(comp.h_samp);
                    max_v = max_v.max(comp.v_samp);
                    j.components.push(comp);
                }
                for (i, c) in j.components.iter().enumerate() {
                    if j.components
                        .iter()
                        .skip(i.saturating_add(1))
                        .any(|d| d.id == c.id)
                    {
                        return Err(not_reversible("duplicate component id"));
                    }
                }
                let (mcu_x, mcu_y) = (
                    j.width.div_ceil(max_h.saturating_mul(8)),
                    j.height.div_ceil(max_v.saturating_mul(8)),
                );
                for c in j.components.iter_mut() {
                    c.width_blocks = mcu_x.saturating_mul(c.h_samp);
                    c.height_blocks = mcu_y.saturating_mul(c.v_samp);
                    c.coeffs = vec![
                        0;
                        c.width_blocks
                            .saturating_mul(c.height_blocks)
                            .saturating_mul(64)
                    ];
                }
            }
            0xDA => {
                if !saw_sof {
                    return Err(not_reversible("scan before frame"));
                }
                j.marker_order.push(m);
                let Some((&ns, rest)) = seg.split_first() else {
                    return Err(not_reversible("SOS length"));
                };
                let ns = usize::from(ns);
                let Some((pairs, &[ss, se, ahal])) = rest
                    .split_at_checked(ns.saturating_mul(2))
                    .and_then(|(p, t)| Some((p.as_chunks::<2>().0, <&[u8; 3]>::try_from(t).ok()?)))
                    .filter(|_| (1..=3).contains(&ns))
                else {
                    return Err(not_reversible("SOS parameters"));
                };
                let mut s = JpegScanInfo {
                    num_components: ns as u32,
                    ..Default::default()
                };
                for (&[id, t], slot) in pairs.iter().zip(s.components.iter_mut()) {
                    let id = u32::from(id);
                    let Some(idx) = j.components.iter().rposition(|c| c.id == id) else {
                        return Err(not_reversible("scan component"));
                    };
                    *slot = JpegScanComponent {
                        comp_idx: idx as u32,
                        dc_tbl: u32::from(t >> 4),
                        ac_tbl: u32::from(t & 15),
                    };
                    if slot.dc_tbl > 3 || slot.ac_tbl > 3 {
                        return Err(not_reversible("scan table index"));
                    }
                }
                (s.ss, s.se, s.ah, s.al) = (
                    u32::from(ss),
                    u32::from(se),
                    u32::from(ahal >> 4),
                    u32::from(ahal & 15),
                );
                if !progressive && (s.ss != 0 || s.se != 63 || s.ah != 0 || s.al != 0) {
                    return Err(not_reversible("sequential scan parameters"));
                }
                if s.se > 63 || s.ss > s.se || s.al > 13 || s.ah > 13 {
                    return Err(not_reversible("scan parameters"));
                }
                let have_dri = j.marker_order.contains(&0xDD);
                let end = decode_scan(
                    b,
                    pos,
                    &mut j,
                    &mut s,
                    progressive,
                    &dc_dec,
                    &ac_dec,
                    have_dri,
                    &mut pad_bits,
                )?;
                j.scans.push(s);
                pos = end;
            }
            _ => return Err(not_reversible(format!("unsupported marker {m:#x}"))),
        }
    }
    if !saw_sof || j.scans.is_empty() {
        return Err(not_reversible("no image data"));
    }
    if j.quant.len() > 3 {
        return Err(not_reversible("more than three quantization tables"));
    }
    if pad_bits.contains(&0) {
        j.has_zero_padding = true;
        j.padding_bits = pad_bits;
    }
    Ok(ParsedJpeg {
        data: j,
        sof_marker,
    })
}
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    b: &[u8],
    pos: usize,
    j: &mut JpegData,
    s: &mut JpegScanInfo,
    progressive: bool,
    dc_dec: &[HuffDecoder; 4],
    ac_dec: &[HuffDecoder; 4],
    have_dri: bool,
    pad_bits: &mut Vec<u8>,
) -> Result<usize, Error> {
    if progressive && !(s.ss == 0 && s.se == 63 && s.ah == 0 && s.al == 0) {
        return Err(not_reversible(
            "progressive JPEG recompression is not supported",
        ));
    }
    let restart = if have_dri {
        j.restart_interval as i64
    } else {
        0
    };
    let mut r = ScanReader::new(b, pos, Vec::new());
    let (per_row, rows) = mcu_size(j, s);
    let n = s.num_components as usize;
    let mut last_dc = [0i32; 4];
    let mut block_idx: u32 = 0;
    let mut restarts_to_go = restart;
    let mut next_rst: u8 = 0;
    for mcu_y in 0..rows {
        for mcu_x in 0..per_row {
            if restart > 0 && restarts_to_go == 0 {
                r.align_to_byte();
                let p = r.byte_pos();
                if b.get(p..p.saturating_add(2)) != Some(&[0xFF, 0xD0 | next_rst]) {
                    return Err(not_reversible("missing restart marker"));
                }
                next_rst = next_rst.wrapping_add(1) & 7;
                r = ScanReader::new(b, p.saturating_add(2), std::mem::take(&mut r.pad_bits));
                restarts_to_go = restart;
                last_dc = [0; 4];
            }
            for sc in s.components.iter().take(n).copied() {
                let (Some(comp), Some(dc_d), Some(ac_d)) = (
                    j.components.get_mut(sc.comp_idx as usize),
                    dc_dec.get(sc.dc_tbl as usize),
                    ac_dec.get(sc.ac_tbl as usize),
                ) else {
                    return Err(super::fault());
                };
                if !dc_d.ok || !ac_d.ok {
                    return Err(not_reversible("scan uses an undefined Huffman table"));
                }
                let (nby, nbx) = if n > 1 {
                    (comp.v_samp, comp.h_samp)
                } else {
                    (1, 1)
                };
                for iy in 0..nby {
                    for ix in 0..nbx {
                        let by = mcu_y.saturating_mul(nby).saturating_add(iy);
                        let bx = mcu_x.saturating_mul(nbx).saturating_add(ix);
                        let at = by
                            .saturating_mul(comp.width_blocks)
                            .saturating_add(bx)
                            .saturating_mul(64);
                        let Some(blk) = comp
                            .coeffs
                            .get_mut(at..at.saturating_add(64))
                            .and_then(|b| <&mut [i16; 64]>::try_from(b).ok())
                        else {
                            return Err(super::fault());
                        };
                        let Some(last) = last_dc.get_mut(sc.comp_idx as usize) else {
                            return Err(super::fault());
                        };
                        let runs = decode_block_sequential(&mut r, dc_d, ac_d, blk, last)?;
                        if runs > 0 {
                            s.extra_zero_runs.push(ExtraZeroRun {
                                block_idx,
                                num_runs: runs,
                            });
                        }
                        block_idx = block_idx.wrapping_add(1);
                    }
                }
            }
            restarts_to_go = restarts_to_go.saturating_sub(1);
            if r.overrun() {
                return Err(not_reversible("scan data ends early"));
            }
        }
    }
    r.align_to_byte();
    pad_bits.append(&mut r.pad_bits);
    let end = r.byte_pos();
    if end.saturating_add(1) >= b.len() || b.get(end) != Some(&0xFF) {
        return Err(not_reversible("scan not followed by a marker"));
    }
    Ok(end)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_block_sequential(
    r: &mut ScanReader,
    dc_d: &HuffDecoder,
    ac_d: &HuffDecoder,
    blk: &mut [i16; 64],
    last_dc: &mut i32,
) -> Result<u32, Error> {
    let s = match r.decode(dc_d) {
        Some(s) if s <= 11 => s as u32,
        _ => return Err(not_reversible("bad DC code")),
    };
    let diff = extend_sign(r.bits(s), s);
    let dc = last_dc.wrapping_add(diff);
    if !(-32768..=32767).contains(&dc) {
        return Err(not_reversible("DC overflow"));
    }
    *last_dc = dc;
    blk[0] = dc as i16;
    let mut runs = 0;
    let mut k = 1;
    while k < 64 {
        let Some(sym) = r.decode(ac_d) else {
            return Err(not_reversible("bad AC code"));
        };
        let (run, size) = (sym >> 4, (sym & 15) as u32);
        if size == 0 {
            if run == 15 {
                runs += 1;
                k += 16;
                if k > 64 {
                    return Err(not_reversible("zero run past the block end"));
                }
                continue;
            }
            if run != 0 {
                return Err(not_reversible("invalid AC symbol"));
            }
            return Ok(runs);
        }
        runs = 0;
        k += run;
        if k > 63 {
            return Err(not_reversible("AC index past the block end"));
        }
        blk[JPEG_NATURAL_ORDER[k]] = extend_sign(r.bits(size), size) as i16;
        k += 1;
    }
    Ok(runs)
}
