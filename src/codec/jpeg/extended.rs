#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::{EXTENDED_PRECISION, Metadata};
use crate::fmath;
use std::f64::consts::SQRT_2;
use std::sync::OnceLock;
pub(super) const MAX_DECODED_BYTES: u64 = 512 << 20;
const WORD_BYTES: u64 = 8;
const MAX_INT: u64 = i64::MAX as u64;
pub(super) const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
#[allow(clippy::arithmetic_side_effects)]
pub(super) fn cos_table() -> &'static [[f64; 8]; 8] {
    static TABLE: OnceLock<[[f64; 8]; 8]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [[0.0; 8]; 8];
        for (frequency, row) in t.iter_mut().enumerate() {
            for (sample, v) in row.iter_mut().enumerate() {
                *v =
                    fmath::cos(((2 * sample + 1) * frequency) as f64 * std::f64::consts::PI / 16.0);
            }
        }
        t
    })
}
#[derive(Debug, Clone, Default)]
pub(super) struct HuffmanTable {
    counts: [usize; 17],
    symbols: Vec<u8>,
    pub valid: bool,
}
#[derive(Debug, Clone, Default)]
pub(super) struct Component {
    pub id: u8,
    pub h: usize,
    pub v: usize,
    pub quant_id: u8,
    pub blocks_per_line: usize,
    pub blocks_per_column: usize,
    pub samples: Vec<u16>,
    pub dc_predictor: i64,
    pub scanned: bool,
}
#[derive(Debug)]
pub(super) struct Decoder<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub width: usize,
    pub height: usize,
    pub precision: usize,
    pub components: Vec<Component>,
    pub hmax: usize,
    pub vmax: usize,
    packed_mono: Option<Vec<u8>>,
    padded_raster_bytes: u64,
    reserved_bytes: u64,
    pub adobe_transform: i32,
    pub jfif: bool,
    tolerate_scan_parameters: bool,
    scan_parameters_ignored: bool,
    pub quant: [[i64; 64]; 4],
    pub quant_defined: [bool; 4],
    pub huffman: [[HuffmanTable; 4]; 2],
    pub restart: usize,
    seen_dri: bool,
    pub seen_sof: bool,
    pub seen_sos: bool,
}
impl Default for Decoder<'_> {
    fn default() -> Self {
        Decoder {
            data: &[],
            pos: 0,
            width: 0,
            height: 0,
            precision: 0,
            components: Vec::new(),
            hmax: 0,
            vmax: 0,
            packed_mono: None,
            padded_raster_bytes: 0,
            reserved_bytes: 0,
            adobe_transform: 0,
            jfif: false,
            tolerate_scan_parameters: false,
            scan_parameters_ignored: false,
            quant: [[0; 64]; 4],
            quant_defined: [false; 4],
            huffman: Default::default(),
            restart: 0,
            seen_dri: false,
            seen_sof: false,
            seen_sos: false,
        }
    }
}
pub(super) struct Decoded {
    pub data: Vec<u8>,
    pub ycbcr: bool,
    pub scan_parameters_ignored: bool,
}
pub(super) fn validate_request(m: &Metadata, fragments: &[&[u8]]) -> Result<(), String> {
    let frames = m.number_of_frames as u64;
    if m.number_of_frames <= 0 || fragments.is_empty() {
        return Err("invalid JPEG Extended frame count".into());
    }
    let pixels = (m.rows as u64).saturating_mul(m.columns as u64);
    let samples = m.samples_per_pixel.max(1) as u64;
    let output = pixels.saturating_mul(2).saturating_mul(samples);
    let mut total = output
        .checked_mul(frames)
        .ok_or("JPEG Extended request output overflow")?;
    let overhead = frames
        .checked_mul(6 * WORD_BYTES)
        .ok_or("JPEG Extended request overhead overflow")?;
    total = total
        .checked_add(overhead)
        .ok_or("JPEG Extended request overhead overflow")?;
    for fragment in fragments {
        total = total
            .checked_add(fragment.len() as u64)
            .ok_or("JPEG Extended request size overflow")?;
        if total > MAX_DECODED_BYTES {
            return Err("JPEG Extended request exceeds resource limit".into());
        }
    }
    if total > MAX_DECODED_BYTES {
        return Err("JPEG Extended request exceeds resource limit".into());
    }
    Ok(())
}
pub(super) fn decode(
    stream: &[u8],
    m: &Metadata,
    tolerate_scan_parameters: bool,
    reserved_bytes: u64,
) -> Result<Decoded, String> {
    let mut d = Decoder {
        data: stream,
        pos: 2,
        adobe_transform: -1,
        tolerate_scan_parameters,
        reserved_bytes: reserved_bytes.max(stream.len() as u64),
        ..Default::default()
    };
    let (data, ycbcr) = d.decode(m)?;
    Ok(Decoded {
        data,
        ycbcr,
        scan_parameters_ignored: d.scan_parameters_ignored,
    })
}
impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8]) -> Decoder<'a> {
        Decoder {
            data,
            pos: 2,
            adobe_transform: -1,
            ..Default::default()
        }
    }
    fn decode(&mut self, m: &Metadata) -> Result<(Vec<u8>, bool), String> {
        let stream = self.data;
        if stream.len() as u64 > MAX_DECODED_BYTES {
            return Err("JPEG Extended compressed frame exceeds resource limit".into());
        }
        if stream.len() < 4 || !stream.starts_with(&[0xff, 0xd8]) {
            return Err("missing JPEG SOI".into());
        }
        loop {
            let marker = self.next_marker()?;
            match marker {
                0xc1 => {
                    let segment = self.read_segment()?;
                    self.parse_sof1(segment, m)?;
                }
                0xc4 => {
                    let segment = self.read_segment()?;
                    self.parse_dht(segment)?;
                }
                0xdb => {
                    let segment = self.read_segment()?;
                    self.parse_dqt(segment)?;
                }
                0xdd => {
                    if self.seen_dri || self.all_components_scanned() {
                        return Err("duplicate or out-of-order JPEG DRI".into());
                    }
                    let &[hi, lo] = self.read_segment()? else {
                        return Err("invalid JPEG DRI length".into());
                    };
                    self.restart = usize::from(u16::from_be_bytes([hi, lo]));
                    self.seen_dri = true;
                }
                0xda => {
                    let segment = self.read_segment()?;
                    self.decode_sos(segment)?;
                }
                0xd9 => {
                    if !self.seen_sof || !self.seen_sos {
                        return Err("incomplete JPEG Extended Process 4 frame".into());
                    }
                    for c in &self.components {
                        if !c.scanned {
                            return Err(format!(
                                "incomplete JPEG Extended Process 4 frame: component {} not scanned",
                                c.id
                            ));
                        }
                    }
                    if self.data.len().saturating_sub(self.pos) > 1 {
                        return Err("trailing data after JPEG EOI".into());
                    }
                    let out = self.output();
                    return Ok((out, self.codestream_is_ycbcr()));
                }
                0xe0 | 0xee => {
                    let segment = self.read_segment()?;
                    if marker == 0xe0 && segment.starts_with(b"JFIF\x00") {
                        self.jfif = true;
                    }
                    if marker == 0xee
                        && segment.starts_with(b"Adobe")
                        && let Some(&transform) = segment.get(11)
                    {
                        self.adobe_transform = i32::from(transform);
                    }
                }
                0xe1..=0xed | 0xef | 0xfe => {
                    self.read_segment()?;
                }
                0xd0..=0xd7 => return Err("JPEG restart marker outside entropy data".into()),
                _ => return Err(format!("unsupported JPEG marker 0xff{marker:02x}")),
            }
        }
    }
    fn admit_raster_peak(&self, raster_bytes: u64) -> Result<(), String> {
        let limit = MAX_DECODED_BYTES.min(MAX_INT);
        if raster_bytes > limit || self.reserved_bytes > limit.saturating_sub(raster_bytes) {
            return Err("JPEG Extended decoded frame exceeds resource limit".into());
        }
        Ok(())
    }
    fn next_marker(&mut self) -> Result<u8, String> {
        if self.data.get(self.pos) != Some(&0xff) {
            return Err(format!("expected JPEG marker at byte {}", self.pos));
        }
        while self.data.get(self.pos) == Some(&0xff) {
            self.pos = self.pos.saturating_add(1);
        }
        let Some(&marker) = self.data.get(self.pos).filter(|&&b| b != 0) else {
            return Err("truncated or stuffed JPEG marker".into());
        };
        self.pos = self.pos.saturating_add(1);
        Ok(marker)
    }
    pub(crate) fn read_segment(&mut self) -> Result<&'a [u8], String> {
        let data = self.data;
        let Some(&[hi, lo]) = data.get(self.pos..self.pos.saturating_add(2)) else {
            return Err("truncated JPEG segment length".into());
        };
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let end = self.pos.saturating_add(length);
        let segment = match data.get(self.pos.saturating_add(2)..end) {
            Some(segment) if length >= 2 => segment,
            _ => return Err("invalid JPEG segment length".into()),
        };
        self.pos = end;
        Ok(segment)
    }
    fn parse_sof1(&mut self, segment: &[u8], m: &Metadata) -> Result<(), String> {
        if self.seen_sof || self.seen_sos {
            return Err("duplicate or out-of-order JPEG SOF1".into());
        }
        let &[precision, h0, h1, w0, w1, count, ..] = segment else {
            return Err("truncated JPEG SOF1".into());
        };
        let count = usize::from(count);
        if (count != 1 && count != 3) || segment.len() != count.saturating_mul(3).saturating_add(6)
        {
            return Err("JPEG Extended Process 4 requires one or three components".into());
        }
        if m.samples_per_pixel != 0 && count != m.samples_per_pixel as usize {
            return Err(format!(
                "JPEG Extended components={count}, SamplesPerPixel={}",
                m.samples_per_pixel
            ));
        }
        let precision = usize::from(precision);
        let height = usize::from(u16::from_be_bytes([h0, h1]));
        let width = usize::from(u16::from_be_bytes([w0, w1]));
        if precision != EXTENDED_PRECISION {
            return Err(format!(
                "JPEG Extended Process 4 precision={precision}, want 12"
            ));
        }
        if height == 0 || width == 0 {
            return Err("invalid JPEG Extended dimensions".into());
        }
        if height != m.rows as usize || width != m.columns as usize {
            return Err(format!(
                "dicom: JPEG still-image size does not match metadata: JPEG={width}x{height} metadata={}x{}",
                m.columns, m.rows
            ));
        }
        self.components = Vec::with_capacity(count);
        let (chunks, _) = segment.as_chunks::<3>();
        let triples = chunks.get(2..).unwrap_or_default();
        for &[id, hv, quant_id] in triples.iter().take(count) {
            let (h, v) = (usize::from(hv >> 4), usize::from(hv & 0x0f));
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) || quant_id > 3 {
                return Err("unsupported JPEG Extended component parameters".into());
            }
            if count == 1 && (h != 1 || v != 1) {
                return Err("unsupported JPEG Extended component parameters".into());
            }
            if self.components.iter().any(|p| p.id == id) {
                return Err(format!("duplicate JPEG component identifier {id}"));
            }
            self.components.push(Component {
                id,
                h,
                v,
                quant_id,
                ..Default::default()
            });
            self.hmax = self.hmax.max(h);
            self.vmax = self.vmax.max(v);
        }
        let mcus_x = width.div_ceil(self.hmax.saturating_mul(8));
        let mcus_y = height.div_ceil(self.vmax.saturating_mul(8));
        let mut plane_bytes = 0u64;
        let mut chroma_row_bytes = 0u64;
        for c in &mut self.components {
            c.blocks_per_line = mcus_x.saturating_mul(c.h);
            c.blocks_per_column = mcus_y.saturating_mul(c.v);
            plane_bytes = plane_bytes.saturating_add(
                (c.blocks_per_line as u64)
                    .saturating_mul(c.blocks_per_column as u64)
                    .saturating_mul(128),
            );
            if self.hmax == c.h.saturating_mul(2) && self.vmax == c.v.saturating_mul(2) {
                chroma_row_bytes = (width.div_ceil(2) as u64).saturating_mul(WORD_BYTES);
            }
        }
        let packed_bytes = (width as u64)
            .saturating_mul(height as u64)
            .saturating_mul(2)
            .saturating_mul(count as u64);
        let decoded_bytes = packed_bytes.saturating_add(plane_bytes);
        if decoded_bytes > MAX_DECODED_BYTES || decoded_bytes > MAX_INT {
            return Err("JPEG Extended decoded frame exceeds resource limit".into());
        }
        if count != 1 {
            self.admit_raster_peak(
                plane_bytes
                    .saturating_add(packed_bytes)
                    .saturating_add(packed_bytes.max(chroma_row_bytes)),
            )?;
            for c in &mut self.components {
                c.samples = vec![0; plane_samples(c)];
            }
        }
        self.precision = precision;
        self.height = height;
        self.width = width;
        self.padded_raster_bytes = plane_bytes;
        self.seen_sof = true;
        Ok(())
    }
    pub(crate) fn all_components_scanned(&self) -> bool {
        !self.components.is_empty() && self.components.iter().all(|c| c.scanned)
    }
    pub(crate) fn parse_dqt(&mut self, segment: &[u8]) -> Result<(), String> {
        if self.all_components_scanned() {
            return Err("JPEG quantization table after final scan".into());
        }
        let mut rest = segment;
        while let Some((&info, after)) = rest.split_first() {
            rest = after;
            let precision = info >> 4;
            let table_id = usize::from(info & 0x0f);
            if precision > 1 || table_id > 3 {
                return Err("invalid JPEG quantization table selector".into());
            }
            let value_bytes: usize = if precision == 1 { 2 } else { 1 };
            let Some((values, after)) = rest.split_at_checked(value_bytes.saturating_mul(64))
            else {
                return Err("truncated JPEG quantization table".into());
            };
            rest = after;
            let mut table = [0i64; 64];
            for (k, &natural) in ZIGZAG.iter().enumerate() {
                let value = match values.get(k.saturating_mul(value_bytes)..) {
                    Some(&[hi, lo, ..]) if value_bytes == 2 => {
                        i64::from(u16::from_be_bytes([hi, lo]))
                    }
                    Some(&[b, ..]) => i64::from(b),
                    _ => 0,
                };
                if value == 0 {
                    return Err("zero JPEG quantization value".into());
                }
                if let Some(slot) = table.get_mut(natural) {
                    *slot = value;
                }
            }
            if let (Some(q), Some(defined)) = (
                self.quant.get_mut(table_id),
                self.quant_defined.get_mut(table_id),
            ) {
                *q = table;
                *defined = true;
            }
        }
        if segment.is_empty() {
            return Err("empty JPEG quantization segment".into());
        }
        Ok(())
    }
    pub(crate) fn parse_dht(&mut self, segment: &[u8]) -> Result<(), String> {
        if self.all_components_scanned() {
            return Err("JPEG Huffman table after final scan".into());
        }
        let mut rest = segment;
        while !rest.is_empty() {
            let Some((&[info, ref counts @ ..], after)) = rest.split_first_chunk::<17>() else {
                return Err("truncated JPEG Huffman table".into());
            };
            rest = after;
            let class = usize::from(info >> 4);
            let table_id = usize::from(info & 0x0f);
            if class > 1 || table_id > 3 {
                return Err("invalid JPEG Huffman table selector".into());
            }
            let mut table = HuffmanTable::default();
            let mut symbol_count = 0usize;
            let mut available: i64 = 1;
            for (slot, &count) in table.counts.iter_mut().skip(1).zip(counts) {
                let count = usize::from(count);
                *slot = count;
                symbol_count = symbol_count.saturating_add(count);
                available = available.saturating_mul(2).saturating_sub(count as i64);
                if available < 0 {
                    return Err("oversubscribed JPEG Huffman table".into());
                }
            }
            if available == 0 {
                return Err("complete JPEG Huffman code tree".into());
            }
            let Some((symbols, after)) = rest.split_at_checked(symbol_count) else {
                return Err("invalid JPEG Huffman symbol count".into());
            };
            if symbol_count == 0 || symbol_count > 256 {
                return Err("invalid JPEG Huffman symbol count".into());
            }
            table.symbols = symbols.to_vec();
            if class == 0 {
                for &symbol in &table.symbols {
                    if symbol > 15 {
                        return Err(format!("invalid JPEG DC Huffman symbol {symbol}"));
                    }
                }
            }
            table.valid = true;
            rest = after;
            if let Some(slot) = self
                .huffman
                .get_mut(class)
                .and_then(|c| c.get_mut(table_id))
            {
                *slot = table;
            }
        }
        if segment.is_empty() {
            return Err("empty JPEG Huffman segment".into());
        }
        Ok(())
    }
    fn decode_sos(&mut self, segment: &[u8]) -> Result<(), String> {
        let unsupported = || "unsupported JPEG Extended sequential scan".to_string();
        if !self.seen_sof {
            return Err("duplicate or out-of-order JPEG SOS".into());
        }
        let Some((&count, rest)) = segment.split_first() else {
            return Err(unsupported());
        };
        let count = usize::from(count);
        if count < 1
            || count > self.components.len()
            || rest.len() != count.saturating_mul(2).saturating_add(3)
        {
            return Err(unsupported());
        }
        let (selectors, tail) = rest.split_at(count.saturating_mul(2).min(rest.len()));
        if tail != [0, 63, 0] {
            if !self.tolerate_scan_parameters {
                return Err(unsupported());
            }
            self.scan_parameters_ignored = true;
        }
        let mut scan: Vec<usize> = Vec::with_capacity(count);
        let mut tables: Vec<[HuffmanTable; 2]> = Vec::with_capacity(count);
        let mut blocks_per_mcu = 0usize;
        for &[id, tables_byte] in selectors.as_chunks::<2>().0 {
            let mut found = None;
            for (j, c) in self.components.iter().enumerate() {
                if c.id == id {
                    found = Some(j);
                }
            }
            let (Some(index), Some(component)) =
                (found, found.and_then(|j| self.components.get(j)))
            else {
                return Err(unsupported());
            };
            if component.scanned || scan.contains(&index) {
                return Err(unsupported());
            }
            let (dc_id, ac_id) = (
                usize::from(tables_byte >> 4),
                usize::from(tables_byte & 0x0f),
            );
            let dc = self.huffman.first().and_then(|t| t.get(dc_id));
            let ac = self.huffman.get(1).and_then(|t| t.get(ac_id));
            let (Some(dc), Some(ac)) = (dc.filter(|t| t.valid), ac.filter(|t| t.valid)) else {
                return Err("undefined JPEG Huffman table".into());
            };
            if self.quant_defined.get(usize::from(component.quant_id)) != Some(&true) {
                return Err("undefined JPEG quantization table".into());
            }
            tables.push([dc.clone(), ac.clone()]);
            blocks_per_mcu = blocks_per_mcu.saturating_add(component.h.saturating_mul(component.v));
            scan.push(index);
        }
        if count > 1 && blocks_per_mcu > 10 {
            return Err(format!(
                "JPEG MCU has {blocks_per_mcu} blocks, more than 10"
            ));
        }
        let (entropy, end) = self.entropy_segment()?;
        if self.components.len() == 1 {
            let packed_bytes = (self.width as u64)
                .saturating_mul(self.height as u64)
                .saturating_mul(2);
            if self.scan_parameters_ignored {
                self.admit_raster_peak(
                    self.padded_raster_bytes
                        .saturating_add(packed_bytes.saturating_mul(2)),
                )?;
                if let Some(c) = scan.first().and_then(|&i| self.components.get_mut(i)) {
                    c.samples = vec![0; plane_samples(c)];
                }
            } else {
                self.admit_raster_peak(packed_bytes)?;
                self.packed_mono = Some(vec![0; packed_bytes as usize]);
            }
        }
        self.decode_scan(entropy, &scan, &tables)?;
        for &index in &scan {
            if let Some(c) = self.components.get_mut(index) {
                c.scanned = true;
            }
        }
        self.seen_sos = true;
        self.seen_dri = false;
        self.pos = end;
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_scan(
        &mut self,
        entropy: &[u8],
        scan: &[usize],
        tables: &[[HuffmanTable; 2]],
    ) -> Result<(), String> {
        let mut reader = BitReader::new(entropy);
        let (mcus_x, mcus_y) = if scan.len() == 1 {
            let c = &self.components[scan[0]];
            let cw = (self.width * c.h).div_ceil(self.hmax);
            let ch = (self.height * c.v).div_ceil(self.vmax);
            (cw.div_ceil(8), ch.div_ceil(8))
        } else {
            (
                self.width.div_ceil(8 * self.hmax),
                self.height.div_ceil(8 * self.vmax),
            )
        };
        for &index in scan {
            self.components[index].dc_predictor = 0;
        }
        let mut restart_index = 0u8;
        for mcu in 0..mcus_x * mcus_y {
            if self.restart > 0 && mcu > 0 && mcu % self.restart == 0 {
                reader.consume_restart(0xd0 + restart_index)?;
                restart_index = (restart_index + 1) & 7;
                for &index in scan {
                    self.components[index].dc_predictor = 0;
                }
            }
            let (mx, my) = (mcu % mcus_x, mcu / mcus_x);
            for (i, &index) in scan.iter().enumerate() {
                if scan.len() == 1 {
                    self.decode_block(&mut reader, index, &tables[i], mx, my, mcu)?;
                    continue;
                }
                let (ch, cv) = (self.components[index].h, self.components[index].v);
                for v in 0..cv {
                    for h in 0..ch {
                        self.decode_block(
                            &mut reader,
                            index,
                            &tables[i],
                            mx * ch + h,
                            my * cv + v,
                            mcu,
                        )?;
                    }
                }
            }
        }
        reader.finish()
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_block(
        &mut self,
        reader: &mut BitReader<'_>,
        index: usize,
        tables: &[HuffmanTable; 2],
        block_x: usize,
        block_y: usize,
        mcu: usize,
    ) -> Result<(), String> {
        let mut coefficients = [0f64; 64];
        let quant = self.quant[self.components[index].quant_id as usize];
        let category = reader
            .decode_huffman(&tables[0])
            .map_err(|e| format!("decode JPEG DC block {mcu}: {e}"))?;
        if category > 15 {
            return Err(format!("invalid JPEG DC coefficient category {category}"));
        }
        let difference = reader.receive_extend(category as i64)?;
        let c = &mut self.components[index];
        if difference > 0 && c.dc_predictor > i32::MAX as i64 - difference
            || difference < 0 && c.dc_predictor < i32::MIN as i64 - difference
        {
            return Err("JPEG DC predictor overflow".into());
        }
        c.dc_predictor += difference;
        coefficients[0] = c.dc_predictor as f64 * quant[0] as f64;
        let mut zig = 1;
        while zig < 64 {
            let symbol = reader
                .decode_huffman(&tables[1])
                .map_err(|e| format!("decode JPEG AC block {mcu}: {e}"))?;
            let run = (symbol >> 4) as usize;
            let size = (symbol & 0x0f) as i64;
            if size == 0 {
                if run == 0 {
                    break;
                }
                if run != 15 || zig + 16 > 64 {
                    return Err("invalid JPEG zero run".into());
                }
                zig += 16;
                continue;
            }
            zig += run;
            if zig >= 64 {
                return Err("JPEG AC run exceeds block".into());
            }
            let value = reader.receive_extend(size)?;
            let natural = ZIGZAG[zig];
            coefficients[natural] = value as f64 * quant[natural] as f64;
            zig += 1;
        }
        let c = &self.components[index];
        if block_x < c.blocks_per_line && block_y < c.blocks_per_column {
            self.write_idct_block(index, block_x, block_y, &coefficients);
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn write_idct_block(
        &mut self,
        index: usize,
        block_x: usize,
        block_y: usize,
        coefficients: &[f64; 64],
    ) {
        let cos = cos_table();
        let center = 1i64 << (self.precision - 1);
        let maximum = (1i64 << self.precision) - 1;
        let (x0, y0) = (block_x * 8, block_y * 8);
        let (mut width, mut height) = (8, 8);
        if self.packed_mono.is_some() {
            width = width.min(self.width - x0);
            height = height.min(self.height - y0);
        }
        let mut rows = [[0u16; 8]; 8];
        if coefficients[1..].iter().all(|&c| c == 0.0) {
            let value = round_sample(coefficients[0] / 8.0, center, maximum);
            for row in rows.iter_mut().take(height) {
                row[..width].fill(value);
            }
        } else {
            let mut intermediate = [0f64; 64];
            for v in 0..8 {
                for x in 0..width {
                    let mut sum = coefficients[v * 8] / SQRT_2;
                    for u in 1..8 {
                        sum += coefficients[v * 8 + u] * cos[u][x];
                    }
                    intermediate[v * 8 + x] = sum;
                }
            }
            for (y, row) in rows.iter_mut().enumerate().take(height) {
                for (x, sample) in row.iter_mut().enumerate().take(width) {
                    let mut sum = intermediate[x] / SQRT_2;
                    for v in 1..8 {
                        sum += intermediate[v * 8 + x] * cos[v][y];
                    }
                    *sample = round_sample(sum / 4.0, center, maximum);
                }
            }
        }
        if let Some(packed) = self.packed_mono.as_mut() {
            for (y, row) in rows.iter().enumerate().take(height) {
                let start = ((y0 + y) * self.width + x0) * 2;
                for (x, &sample) in row[..width].iter().enumerate() {
                    packed[start + 2 * x..start + 2 * x + 2].copy_from_slice(&sample.to_le_bytes());
                }
            }
        } else {
            let c = &mut self.components[index];
            let stride = c.blocks_per_line * 8;
            for (y, row) in rows.iter().enumerate().take(height) {
                let start = (y0 + y) * stride + x0;
                c.samples[start..start + width].copy_from_slice(&row[..width]);
            }
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn output(&mut self) -> Vec<u8> {
        if let Some(packed) = self.packed_mono.take() {
            return packed;
        }
        let count = self.components.len();
        let planes: Vec<Vec<u16>> = (0..count).map(|i| self.upsample(i)).collect();
        let mut out = vec![0u8; self.width * self.height * 2 * count];
        for p in 0..self.width * self.height {
            for (i, plane) in planes.iter().enumerate() {
                out[(p * count + i) * 2..(p * count + i) * 2 + 2]
                    .copy_from_slice(&plane[p].to_le_bytes());
            }
        }
        out
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn upsample(&self, index: usize) -> Vec<u16> {
        let c = &self.components[index];
        let (width, height) = (self.width, self.height);
        let stride = c.blocks_per_line * 8;
        let cw = (width * c.h).div_ceil(self.hmax);
        let ch = (height * c.v).div_ceil(self.vmax);
        let at = |x: isize, y: isize| -> i64 {
            let x = x.clamp(0, cw as isize - 1).max(0) as usize;
            let y = y.clamp(0, ch as isize - 1).max(0) as usize;
            c.samples[y * stride + x] as i64
        };
        let mut out = vec![0u16; width * height];
        let (rx, ry) = (self.hmax / c.h, self.vmax / c.v);
        let exact = self.hmax.is_multiple_of(c.h) && self.vmax.is_multiple_of(c.v);
        match (rx, ry, exact) {
            (1, 1, true) => {
                for y in 0..height {
                    out[y * width..(y + 1) * width]
                        .copy_from_slice(&c.samples[y * stride..y * stride + width]);
                }
            }
            (2, 1, true) => {
                for y in 0..height {
                    for x in 0..width {
                        let i = (x / 2) as isize;
                        let y = y as isize;
                        let mut value = 3 * at(i, y);
                        if x % 2 == 0 {
                            if i == 0 {
                                value = at(i, y);
                            } else {
                                value = (value + at(i - 1, y) + 1) >> 2;
                            }
                        } else if i == cw as isize - 1 {
                            value = at(i, y);
                        } else {
                            value = (value + at(i + 1, y) + 2) >> 2;
                        }
                        out[y as usize * width + x] = value as u16;
                    }
                }
            }
            (1, 2, true) => {
                for y in 0..height {
                    let r = (y / 2) as isize;
                    let (far, bias) = if y % 2 == 1 { (r + 1, 2) } else { (r - 1, 1) };
                    for x in 0..width {
                        let x = x as isize;
                        out[y * width + x as usize] =
                            ((3 * at(x, r) + at(x, far) + bias) >> 2) as u16;
                    }
                }
            }
            (2, 2, true) => {
                let mut colsum = vec![0i64; cw];
                for y in 0..height {
                    let r = (y / 2) as isize;
                    let far = if y % 2 == 1 { r + 1 } else { r - 1 };
                    for (i, s) in colsum.iter_mut().enumerate() {
                        *s = 3 * at(i as isize, r) + at(i as isize, far);
                    }
                    for x in 0..width {
                        let i = x / 2;
                        let value = if x % 2 == 0 && i == 0 {
                            (colsum[0] * 4 + 8) >> 4
                        } else if x % 2 == 0 {
                            (3 * colsum[i] + colsum[i - 1] + 8) >> 4
                        } else if i == cw - 1 {
                            (colsum[i] * 4 + 7) >> 4
                        } else {
                            (3 * colsum[i] + colsum[i + 1] + 7) >> 4
                        };
                        out[y * width + x] = value as u16;
                    }
                }
            }
            _ => {
                for y in 0..height {
                    for x in 0..width {
                        out[y * width + x] =
                            c.samples[(y * c.v / self.vmax) * stride + x * c.h / self.hmax];
                    }
                }
            }
        }
        out
    }
    #[allow(clippy::indexing_slicing)]
    pub(crate) fn codestream_is_ycbcr(&self) -> bool {
        if self.components.len() != 3 {
            return false;
        }
        if self.jfif {
            return true;
        }
        if self.adobe_transform >= 0 {
            return self.adobe_transform != 0;
        }
        !(self.components[0].id == b'R'
            && self.components[1].id == b'G'
            && self.components[2].id == b'B')
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn entropy_segment(&self) -> Result<(&'a [u8], usize), String> {
        let data = self.data;
        let start = self.pos;
        let mut i = start;
        while i + 1 < data.len() {
            if data[i] != 0xff {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < data.len() && data[j] == 0xff {
                j += 1;
            }
            if j >= data.len() {
                return Err("truncated JPEG entropy marker".into());
            }
            let marker = data[j];
            if marker == 0 || (0xd0..=0xd7).contains(&marker) {
                i = j + 1;
                continue;
            }
            return Ok((&data[start..i], i));
        }
        Err("unterminated JPEG entropy data".into())
    }
}
fn plane_samples(c: &Component) -> usize {
    c.blocks_per_line
        .saturating_mul(8)
        .saturating_mul(c.blocks_per_column)
        .saturating_mul(8)
}
fn round_sample(value: f64, center: i64, maximum: i64) -> u16 {
    let value = value.round() + center as f64;
    if value <= 0.0 {
        return 0;
    }
    if value >= maximum as f64 {
        return maximum as u16;
    }
    value as u16
}
pub(super) struct BitReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    value: u8,
    pub bits: u32,
}
impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            pos: 0,
            value: 0,
            bits: 0,
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bit(&mut self) -> Result<i64, String> {
        if self.bits == 0 {
            self.value = self.read_entropy_byte()?;
            self.bits = 8;
        }
        self.bits -= 1;
        Ok(((self.value >> self.bits) & 1) as i64)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_entropy_byte(&mut self) -> Result<u8, String> {
        if self.pos >= self.data.len() {
            return Err("unexpected end of JPEG entropy data".into());
        }
        let value = self.data[self.pos];
        self.pos += 1;
        if value != 0xff {
            return Ok(value);
        }
        if self.pos >= self.data.len() || self.data[self.pos] != 0 {
            return Err("unexpected JPEG marker in entropy data".into());
        }
        self.pos += 1;
        Ok(0xff)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bits(&mut self, count: usize) -> Result<i64, String> {
        let mut value = 0i64;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Ok(value)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn receive_extend(&mut self, count: i64) -> Result<i64, String> {
        if count == 0 {
            return Ok(0);
        }
        if !(0..=16).contains(&count) {
            return Err(format!("invalid JPEG coefficient category {count}"));
        }
        let mut value = self.read_bits(count as usize)?;
        let threshold = 1i64 << (count - 1);
        if value < threshold {
            value -= (1i64 << count) - 1;
        }
        Ok(value)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn decode_huffman(&mut self, table: &HuffmanTable) -> Result<u8, String> {
        let mut code = 0usize;
        let mut first_code = 0usize;
        let mut symbol_offset = 0usize;
        for length in 1..=16 {
            let bit = self.read_bit()? as usize;
            code = (code << 1) | bit;
            let count = table.counts[length];
            if code >= first_code && code - first_code < count {
                let index = symbol_offset + code - first_code;
                if index >= table.symbols.len() {
                    return Err("invalid JPEG Huffman symbol index".into());
                }
                return Ok(table.symbols[index]);
            }
            symbol_offset += count;
            first_code = (first_code + count) << 1;
        }
        Err("invalid JPEG Huffman code".into())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn consume_restart(&mut self, want: u8) -> Result<(), String> {
        self.validate_padding()?;
        self.bits = 0;
        if self.pos >= self.data.len() || self.data[self.pos] != 0xff {
            return Err(format!("missing JPEG restart marker 0xff{want:02x}"));
        }
        while self.pos < self.data.len() && self.data[self.pos] == 0xff {
            self.pos += 1;
        }
        if self.pos >= self.data.len() || self.data[self.pos] != want {
            return Err(format!("invalid JPEG restart marker, want 0xff{want:02x}"));
        }
        self.pos += 1;
        Ok(())
    }
    #[doc(hidden)]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn validate_padding(&self) -> Result<(), String> {
        if self.bits == 0 {
            return Ok(());
        }
        let mask = ((1u32 << self.bits) - 1) as u8;
        if self.value & mask != mask {
            return Err("invalid JPEG entropy padding".into());
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<(), String> {
        self.validate_padding()?;
        if self.pos != self.data.len() {
            return Err("trailing JPEG entropy data".into());
        }
        Ok(())
    }
}
