use super::bitio::{BitReader, bits_n, bits_off, val};
use super::{Error, brotli, corrupt};
pub(crate) const APP_UNKNOWN: u32 = 0;
pub(crate) const APP_ICC: u32 = 1;
pub(crate) const APP_EXIF: u32 = 2;
pub(crate) const APP_XMP: u32 = 3;
pub(crate) const ICC_PROFILE_TAG: &[u8] = b"ICC_PROFILE\x00";
pub(crate) const EXIF_TAG: &[u8] = b"Exif\x00\x00";
pub(crate) const XMP_TAG: &[u8] = b"http://ns.adobe.com/xap/1.0/\x00";
const MAX_BOX_PAYLOAD: usize = 64 << 20;
#[derive(Clone, Copy)]
pub(crate) struct JpegQuantTable {
    pub values: [i32; 64],
    pub precision: u32,
    pub index: u32,
    pub is_last: bool,
}
impl Default for JpegQuantTable {
    fn default() -> Self {
        JpegQuantTable {
            values: [0; 64],
            precision: 0,
            index: 0,
            is_last: false,
        }
    }
}
#[derive(Clone, Default)]
pub(crate) struct JpegComponent {
    pub id: u32,
    pub h_samp: usize,
    pub v_samp: usize,
    pub quant_idx: u32,
    pub width_blocks: usize,
    pub height_blocks: usize,
    pub coeffs: Vec<i16>,
}
#[derive(Clone, Default)]
pub(crate) struct JpegHuffmanCode {
    pub slot_id: u32,
    pub is_last: bool,
    pub counts: [u32; 17],
    pub values: Vec<u32>,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct JpegScanComponent {
    pub comp_idx: u32,
    pub ac_tbl: u32,
    pub dc_tbl: u32,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct ExtraZeroRun {
    pub block_idx: u32,
    pub num_runs: u32,
}
#[derive(Clone, Default)]
pub(crate) struct JpegScanInfo {
    pub num_components: u32,
    pub ss: u32,
    pub se: u32,
    pub al: u32,
    pub ah: u32,
    pub components: [JpegScanComponent; 4],
    pub reset_points: Vec<u32>,
    pub extra_zero_runs: Vec<ExtraZeroRun>,
}
#[derive(Clone, Default)]
pub(crate) struct JpegData {
    pub width: usize,
    pub height: usize,
    pub marker_order: Vec<u8>,
    pub app_data: Vec<Vec<u8>>,
    pub app_marker_type: Vec<u32>,
    pub com_data: Vec<Vec<u8>>,
    pub quant: Vec<JpegQuantTable>,
    pub components: Vec<JpegComponent>,
    pub huffman: Vec<JpegHuffmanCode>,
    pub scans: Vec<JpegScanInfo>,
    pub restart_interval: u32,
    pub inter_marker: Vec<Vec<u8>>,
    pub tail_data: Vec<u8>,
    pub has_zero_padding: bool,
    pub padding_bits: Vec<u8>,
}
pub(crate) fn read_jpeg_data(payload: &[u8]) -> Result<JpegData, Error> {
    let mut br = BitReader::new(payload);
    let mut j = JpegData::default();
    let is_gray = br.bool();
    let (mut num_app, mut num_com, mut num_scans, mut num_inter) = (0usize, 0usize, 0usize, 0usize);
    let mut has_dri = false;
    loop {
        let m = (br.u(6) as u8).wrapping_add(0xC0);
        j.marker_order.push(m);
        if j.marker_order.len() > 16384 {
            return Err(corrupt("too many JPEG markers"));
        }
        match m {
            0xE0..=0xEF => num_app = num_app.saturating_add(1),
            0xFE => num_com = num_com.saturating_add(1),
            0xDA => num_scans = num_scans.saturating_add(1),
            0xFF => num_inter = num_inter.saturating_add(1),
            0xDD => has_dri = true,
            _ => {}
        }
        if m == 0xD9 || br.err.is_some() {
            break;
        }
    }
    j.app_marker_type = Vec::with_capacity(num_app);
    for _ in 0..num_app {
        let t = br.u32(val(0), val(1), bits_off(1, 2), bits_off(2, 4));
        if t > APP_XMP {
            return Err(corrupt("unknown app marker type"));
        }
        j.app_marker_type.push(t);
        let n = (br.u(16) as usize).saturating_add(1);
        if n < 3 {
            return Err(corrupt("app marker too small"));
        }
        j.app_data.push(vec![0; n]);
    }
    for _ in 0..num_com {
        let n = (br.u(16) as usize).saturating_add(1);
        if n < 3 {
            return Err(corrupt("COM marker too small"));
        }
        j.com_data.push(vec![0; n]);
    }
    let nq = br.u32(val(1), val(2), val(3), val(4));
    if nq == 4 {
        return Err(corrupt("quant table count"));
    }
    j.quant = (0..nq)
        .map(|_| JpegQuantTable {
            precision: br.u(1),
            index: br.u(2),
            is_last: br.bool(),
            ..Default::default()
        })
        .collect();
    let comp_type = br.u(2);
    let nc = match comp_type {
        0 => 1,
        1 | 2 => 3,
        _ => {
            let nc = br.u32(val(1), val(2), val(3), val(4));
            if nc != 1 && nc != 3 {
                return Err(corrupt("JPEG component count"));
            }
            nc
        }
    };
    if is_gray != (nc == 1) {
        return Err(corrupt("JPEG gray flag mismatch"));
    }
    j.components = vec![JpegComponent::default(); nc as usize];
    match comp_type {
        0 => {
            if let Some(c) = j.components.first_mut() {
                c.id = 1;
            }
        }
        1 => {
            for (c, id) in j.components.iter_mut().zip(1u32..) {
                c.id = id;
            }
        }
        2 => {
            for (c, id) in j.components.iter_mut().zip(*b"RGB") {
                c.id = u32::from(id);
            }
        }
        _ => {
            for c in j.components.iter_mut() {
                c.id = br.u(8);
            }
        }
    }
    for c in j.components.iter_mut() {
        c.quant_idx = br.u(2);
        if c.quant_idx as usize >= j.quant.len() {
            return Err(corrupt("component quant index"));
        }
    }
    let nh = br.u32(val(4), bits_off(3, 2), bits_off(4, 10), bits_off(6, 26));
    for _ in 0..nh {
        let mut hc = JpegHuffmanCode::default();
        let is_ac = br.bool();
        hc.slot_id = br.u(2);
        if is_ac {
            hc.slot_id |= 0x10;
        }
        hc.is_last = br.bool();
        let mut total: u32 = 0;
        for count in hc.counts.iter_mut() {
            *count = br.u32(val(0), val(1), bits_off(3, 2), bits_n(8));
            total = total.wrapping_add(*count);
        }
        if !(1..=257).contains(&total) {
            return Err(corrupt("Huffman table size"));
        }
        let mut slots = [0u64; 5];
        hc.values = Vec::with_capacity(total as usize);
        for _ in 0..total {
            let v = br.u32(bits_n(2), bits_off(2, 4), bits_off(4, 8), bits_off(8, 1));
            if v > 256 {
                return Err(corrupt("Huffman symbol"));
            }
            hc.values.push(v);
            if let Some(slot) = slots.get_mut((v >> 6) as usize) {
                *slot |= 1 << (v & 63);
            }
        }
        br.check()?;
        if hc.values.last() != Some(&256) || slots[4] != 1 {
            return Err(corrupt("Huffman table without EOI symbol"));
        }
        let [s0, s1, s2, s3, _] = slots;
        let distinct = s0
            .count_ones()
            .saturating_add(s1.count_ones())
            .saturating_add(s2.count_ones())
            .saturating_add(s3.count_ones())
            .saturating_add(1);
        if distinct != total {
            return Err(corrupt("duplicate Huffman symbols"));
        }
        if !is_ac && (s0.count_ones().saturating_add(1) != distinct || s0 >> 12 != 0) {
            return Err(corrupt("DC Huffman symbols out of range"));
        }
        j.huffman.push(hc);
    }
    j.scans = vec![JpegScanInfo::default(); num_scans];
    for s in j.scans.iter_mut() {
        s.num_components = br.u32(val(1), val(2), val(3), val(4));
        if s.num_components >= 4 {
            return Err(corrupt("scan component count"));
        }
        s.ss = br.u(6);
        s.se = br.u(6);
        s.al = br.u(4);
        s.ah = br.u(4);
        for c in s.components.iter_mut().take(s.num_components as usize) {
            c.comp_idx = br.u(2);
            if c.comp_idx >= nc {
                return Err(corrupt("scan component index"));
            }
            c.ac_tbl = br.u(2);
            c.dc_tbl = br.u(2);
        }
        br.u32(val(0), val(1), val(2), bits_off(3, 3));
    }
    if has_dri {
        j.restart_interval = br.u(16);
    }
    for s in j.scans.iter_mut() {
        let n = br.u32(val(0), bits_off(2, 1), bits_off(4, 4), bits_off(16, 20));
        if n > 1 << 24 {
            return Err(corrupt("reset point count"));
        }
        let mut last: i64 = -1;
        for _ in 0..n {
            let v = i64::from(br.u32(val(0), bits_off(3, 1), bits_off(5, 9), bits_off(28, 41)))
                .saturating_add(last)
                .saturating_add(1);
            if v >= 3 << 26 {
                return Err(corrupt("reset point"));
            }
            s.reset_points.push(v as u32);
            last = v;
        }
        let n = br.u32(val(0), bits_off(2, 1), bits_off(4, 4), bits_off(16, 20));
        if n > 1 << 24 {
            return Err(corrupt("extra zero run count"));
        }
        last = -1;
        for _ in 0..n {
            let runs = br.u32(val(1), bits_off(2, 2), bits_off(4, 5), bits_off(8, 20));
            let v = i64::from(br.u32(val(0), bits_off(3, 1), bits_off(5, 9), bits_off(28, 41)))
                .saturating_add(last)
                .saturating_add(1);
            if v > 3 << 26 {
                return Err(corrupt("extra zero run block"));
            }
            s.extra_zero_runs.push(ExtraZeroRun {
                block_idx: v as u32,
                num_runs: runs,
            });
            last = v;
        }
        br.check()?;
    }
    let inter_sizes: Vec<usize> = (0..num_inter).map(|_| br.u(16) as usize).collect();
    let tail_len = br.u32(
        val(0),
        bits_off(8, 1),
        bits_off(16, 257),
        bits_off(22, 65793),
    ) as usize;
    j.has_zero_padding = br.bool();
    if j.has_zero_padding {
        let n = br.u(24);
        if u64::from(n) > (payload.len() as u64).saturating_mul(8) {
            return Err(corrupt("padding bit count"));
        }
        j.padding_bits = (0..n).map(|_| br.u(1) as u8).collect();
    }
    j.check_table_order()?;
    br.zero_pad_to_byte();
    br.check()?;
    let rest = payload.get(br.byte_pos()..).unwrap_or_default();
    let plain = brotli::decode(rest, MAX_BOX_PAYLOAD)
        .map_err(|e| corrupt(format!("jbrd Brotli stream: {e}")))?;
    let mut plain: &[u8] = &plain;
    let mut take = |n: usize| -> Result<Vec<u8>, Error> {
        if n > plain.len() {
            return Err(corrupt("jbrd Brotli stream too short"));
        }
        let (b, rest) = plain.split_at(n);
        plain = rest;
        Ok(b.to_vec())
    };
    let length = |m: &[u8]| match m {
        [_, hi, lo, ..] => (usize::from(*hi) << 8 | usize::from(*lo)).saturating_add(1),
        _ => 0,
    };
    let mut num_icc = 0u8;
    for (m, &kind) in j.app_data.iter_mut().zip(&j.app_marker_type) {
        if kind != APP_UNKNOWN {
            let n = m.len().saturating_sub(1);
            if let [_, hi, lo, ..] = m.as_mut_slice() {
                (*hi, *lo) = ((n >> 8) as u8, n as u8);
            }
            if kind == APP_ICC {
                if m.len() < 17 {
                    return Err(corrupt("ICC marker too small"));
                }
                num_icc = num_icc.wrapping_add(1);
                if let Some([marker, _, _, name @ .., count]) = m.get_mut(..16) {
                    *marker = 0xE2;
                    name.copy_from_slice(ICC_PROFILE_TAG);
                    *count = num_icc;
                }
            }
            continue;
        }
        let b = take(m.len())?;
        m.copy_from_slice(&b);
        if length(m) != m.len() {
            return Err(corrupt("app marker length"));
        }
    }
    for (m, &kind) in j.app_data.iter_mut().zip(&j.app_marker_type) {
        let tag = match kind {
            APP_ICC => {
                if let Some(b) = m.get_mut(16) {
                    *b = num_icc;
                }
                continue;
            }
            APP_EXIF => EXIF_TAG,
            APP_XMP => XMP_TAG,
            _ => continue,
        };
        let Some(t) = m.get_mut(3..tag.len().saturating_add(3)) else {
            return Err(corrupt(if kind == APP_EXIF {
                "Exif marker too small"
            } else {
                "XMP marker too small"
            }));
        };
        t.copy_from_slice(tag);
        if let Some(b) = m.first_mut() {
            *b = 0xE1;
        }
    }
    for m in j.com_data.iter_mut() {
        let b = take(m.len())?;
        m.copy_from_slice(&b);
        if length(m) != m.len() {
            return Err(corrupt("COM marker length"));
        }
    }
    j.inter_marker = Vec::with_capacity(num_inter);
    for n in inter_sizes {
        j.inter_marker.push(take(n)?);
    }
    j.tail_data = take(tail_len)?;
    if !plain.is_empty() {
        return Err(corrupt("excess jbrd Brotli data"));
    }
    Ok(j)
}
impl JpegData {
    fn check_table_order(&self) -> Result<(), Error> {
        let (mut dht, mut scan) = (0, 0);
        let mut progressive = false;
        let mut ac_ok = [false; 4];
        let mut dc_ok = [false; 4];
        for &m in &self.marker_order {
            match m {
                0xC2 => progressive = true,
                0xC4 => {
                    while let Some(h) = self.huffman.get(dht) {
                        dht = dht.saturating_add(1);
                        let ok = if h.slot_id & 0x10 != 0 {
                            &mut ac_ok
                        } else {
                            &mut dc_ok
                        };
                        if let Some(b) = ok.get_mut((h.slot_id & 3) as usize) {
                            *b = true;
                        }
                        if h.is_last {
                            break;
                        }
                    }
                }
                0xDA => {
                    let Some(s) = self.scans.get(scan) else {
                        return Err(corrupt("scan count"));
                    };
                    scan = scan.saturating_add(1);
                    let defined =
                        |ok: &[bool; 4], t: u32| ok.get(t as usize).copied().unwrap_or(false);
                    for c in s.components.iter().take(s.num_components as usize) {
                        if (!progressive || s.ss == 0) && !defined(&dc_ok, c.dc_tbl) {
                            return Err(corrupt("DC Huffman table used before definition"));
                        }
                        if (!progressive || s.ss != 0 || s.se != 0) && !defined(&ac_ok, c.ac_tbl) {
                            return Err(corrupt("AC Huffman table used before definition"));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
