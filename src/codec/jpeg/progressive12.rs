#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::extended::{BitReader, Decoder, HuffmanTable, ZIGZAG};
use super::{ErrorKind, Metadata};
const MAX_BYTES: u64 = 512 << 20;
const MAX_SCANS: usize = 256;
const MAX_BLOCK_VISITS: u64 = 8 << 20;
const WORD_BYTES: u64 = 8;
#[derive(Debug, Clone, Copy)]
pub(super) struct WorkBudget {
    remaining: u64,
}
impl Default for WorkBudget {
    fn default() -> Self {
        WorkBudget {
            remaining: MAX_BLOCK_VISITS,
        }
    }
}
fn resource_limit() -> String {
    ErrorKind::ProgressiveResourceLimit.text().to_string()
}
pub(super) fn request_bytes(m: &Metadata, input: u64, joined: u64) -> Result<u64, String> {
    if m.number_of_frames <= 0 {
        return Err(format!(
            "{}: invalid frame count",
            ErrorKind::InvalidFragment.text()
        ));
    }
    let pixels = (m.rows as u64).saturating_mul(m.columns as u64);
    let count = m.samples_per_pixel as u64;
    let frame_bytes = pixels.saturating_mul(count).saturating_mul(2);
    let frames = m.number_of_frames as u64;
    if frame_bytes == 0 || frames > MAX_BYTES.checked_div(frame_bytes).unwrap_or(0) {
        return Err(resource_limit());
    }
    let mut base = frame_bytes.saturating_mul(frames);
    let overhead = frames.saturating_mul(6 * WORD_BYTES);
    for addition in [input, joined, overhead, 16384] {
        if addition > MAX_BYTES || base > MAX_BYTES.saturating_sub(addition) {
            return Err(resource_limit());
        }
        base = base.saturating_add(addition);
    }
    let blocks = (m.columns as u64)
        .div_ceil(8)
        .saturating_mul((m.rows as u64).div_ceil(8))
        .saturating_mul(count);
    let scratch = blocks.saturating_mul(384);
    if scratch > MAX_BYTES
        || base > MAX_BYTES.saturating_sub(scratch)
        || base.saturating_add(scratch) > i64::MAX as u64
    {
        return Err(resource_limit());
    }
    Ok(base.saturating_add(scratch))
}
struct Progressive<'a, 'w> {
    d: Decoder<'a>,
    work: &'w mut WorkBudget,
    spectral_only: bool,
    coefficients: Vec<Vec<[i32; 64]>>,
    approx: [[i8; 64]; 3],
    quant_used: [bool; 4],
    scans: usize,
}
pub(super) fn decode(
    stream: &[u8],
    m: &Metadata,
    spectral_only: bool,
    work: &mut WorkBudget,
) -> Result<Vec<u8>, String> {
    let mut p = Progressive {
        d: Decoder::new(stream),
        work,
        spectral_only,
        coefficients: Vec::new(),
        approx: [[-1; 64]; 3],
        quant_used: [false; 4],
        scans: 0,
    };
    request_bytes(m, stream.len() as u64, 0)?;
    if stream.len() < 4 || !stream.starts_with(&[0xff, 0xd8]) {
        return Err("missing JPEG SOI".into());
    }
    loop {
        let marker = p.next_marker()?;
        if marker == 0xd9 {
            return p.finish();
        }
        if marker != 0xc2
            && marker != 0xc4
            && marker != 0xdb
            && marker != 0xdd
            && marker != 0xda
            && marker != 0xfe
            && !(0xe0..=0xef).contains(&marker)
        {
            return Err(format!(
                "unsupported progressive JPEG marker 0xff{marker:02x}"
            ));
        }
        let segment = p.d.read_segment()?;
        let result = match marker {
            0xc2 => p.parse_sof2(segment, m),
            0xc4 => p.d.parse_dht(segment),
            0xdb => {
                let previous = p.d.quant;
                let mut r = p.d.parse_dqt(segment);
                for ((&used, before), after) in p.quant_used.iter().zip(&previous).zip(&p.d.quant) {
                    if used && before != after {
                        r = Err("quantization table changed during progression".into());
                    }
                }
                r
            }
            0xdd => {
                if let &[hi, lo] = segment {
                    p.d.restart = usize::from(u16::from_be_bytes([hi, lo]));
                    Ok(())
                } else {
                    Err("invalid JPEG DRI length".into())
                }
            }
            0xda => p.decode_scan(segment),
            0xe0 => {
                if p.d.seen_sos {
                    return Err("color marker after initial scan".into());
                }
                if segment.starts_with(b"JFIF\x00") {
                    p.d.jfif = true;
                }
                Ok(())
            }
            0xee => {
                if p.d.seen_sos {
                    return Err("color marker after initial scan".into());
                }
                let mut r = Ok(());
                if segment.starts_with(b"Adobe")
                    && let Some(&transform) = segment.get(11)
                {
                    let transform = i32::from(transform);
                    if p.d.adobe_transform >= 0 && p.d.adobe_transform != transform {
                        return Err("conflicting Adobe color markers".into());
                    }
                    p.d.adobe_transform = transform;
                    if p.d.adobe_transform > 1 {
                        r = Err("unsupported Adobe color transform".into());
                    }
                }
                r
            }
            _ => Ok(()),
        };
        result?;
    }
}
impl<'a> Progressive<'a, '_> {
    fn next_marker(&mut self) -> Result<u8, String> {
        let d = &mut self.d;
        if d.data.get(d.pos) != Some(&0xff) {
            return Err(format!("expected JPEG marker at byte {}", d.pos));
        }
        while d.data.get(d.pos) == Some(&0xff) {
            d.pos = d.pos.saturating_add(1);
        }
        let Some(&marker) = d.data.get(d.pos).filter(|&&b| b != 0) else {
            return Err("truncated or stuffed JPEG marker".into());
        };
        d.pos = d.pos.saturating_add(1);
        Ok(marker)
    }
    fn parse_sof2(&mut self, segment: &[u8], m: &Metadata) -> Result<(), String> {
        let d = &mut self.d;
        let (false, false, &[precision, h0, h1, w0, w1, count, ..]) =
            (d.seen_sof, d.seen_sos, segment)
        else {
            return Err("duplicate, out-of-order or truncated SOF2".into());
        };
        let count = usize::from(count);
        if (count != 1 && count != 3)
            || count != m.samples_per_pixel as usize
            || segment.len() != count.saturating_mul(3).saturating_add(6)
        {
            return Err("invalid progressive component count".into());
        }
        if precision != 12 {
            return Err(format!(
                "{}: SOF2 precision={precision}, want 12",
                ErrorKind::UnsupportedBitsStored.text(),
            ));
        }
        let width = usize::from(u16::from_be_bytes([w0, w1]));
        let height = usize::from(u16::from_be_bytes([h0, h1]));
        if width == 0 || height == 0 || width != m.columns as usize || height != m.rows as usize {
            return Err(ErrorKind::ImageSizeMismatch.text().to_string());
        }
        d.components = Vec::with_capacity(count);
        let (chunks, _) = segment.as_chunks::<3>();
        for &[id, hv, quant_id] in chunks.get(2..).unwrap_or_default() {
            if hv != 0x11 || quant_id > 3 {
                return Err("unqualified progressive sampling or quantization selector".into());
            }
            if d.components.iter().any(|c| c.id == id) {
                return Err("duplicate component ID".into());
            }
            d.components.push(super::extended::Component {
                id,
                h: 1,
                v: 1,
                quant_id,
                blocks_per_line: width.div_ceil(8),
                blocks_per_column: height.div_ceil(8),
                ..Default::default()
            });
        }
        self.coefficients = Vec::with_capacity(count);
        for c in &mut d.components {
            let blocks = c.blocks_per_line.saturating_mul(c.blocks_per_column);
            self.coefficients.push(vec![[0; 64]; blocks]);
            c.samples = vec![0; blocks.saturating_mul(64)];
        }
        d.width = width;
        d.height = height;
        d.precision = 12;
        d.hmax = 1;
        d.vmax = 1;
        d.seen_sof = true;
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn entropy(&self) -> Result<(&'a [u8], usize), String> {
        let data: &'a [u8] = self.d.data;
        let start = self.d.pos;
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
            if j == data.len() {
                return Err("truncated entropy marker".into());
            }
            if data[j] == 0 || (0xd0..=0xd7).contains(&data[j]) {
                i = j + 1;
                continue;
            }
            return Ok((&data[start..i], i));
        }
        Err("unterminated entropy data".into())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_scan(&mut self, segment: &[u8]) -> Result<(), String> {
        if !self.d.seen_sof || segment.len() < 6 {
            return Err("out-of-order or truncated progressive SOS".into());
        }
        if self.d.components.len() == 3 && self.d.codestream_is_ycbcr() {
            return Err(format!(
                "{}: only raw RGB progressive12 is qualified",
                ErrorKind::UnsupportedPhotometricInterpretation.text()
            ));
        }
        let Some((&count, rest)) = segment.split_first() else {
            return Err("out-of-order or truncated progressive SOS".into());
        };
        let count = usize::from(count);
        if count < 1
            || count > self.d.components.len()
            || rest.len() != count.saturating_mul(2).saturating_add(3)
        {
            return Err("invalid progressive scan components".into());
        }
        let (_, tail) = rest.split_at(count.saturating_mul(2).min(rest.len()));
        let &[ss, se, a] = tail else {
            return Err("invalid progressive scan components".into());
        };
        let (ss, se, ah, al) = (
            usize::from(ss),
            usize::from(se),
            usize::from(a >> 4),
            usize::from(a & 15),
        );
        if ss > se
            || se > 63
            || ah > 13
            || al > 13
            || (ss == 0 && se != 0)
            || (ss != 0 && count != 1)
            || (ah != 0 && ah != al + 1)
        {
            return Err("invalid progressive spectral/approximation parameters".into());
        }
        if self.spectral_only && (ah != 0 || al != 0) {
            return Err("successive approximation under Spectral Selection syntax".into());
        }
        let mut indices = [0usize; 3];
        let mut tables: [HuffmanTable; 3] = Default::default();
        let mut last_index: isize = -1;
        for i in 0..count {
            let mut index: isize = -1;
            for (j, c) in self.d.components.iter().enumerate() {
                if c.id == segment[1 + 2 * i] {
                    index = j as isize;
                }
            }
            if index < 0 || index <= last_index {
                return Err("unknown, duplicate or unordered scan component".into());
            }
            last_index = index;
            let index = index as usize;
            indices[i] = index;
            let quant_id = self.d.components[index].quant_id as usize;
            let (dc, ac) = (
                (segment[2 + 2 * i] >> 4) as usize,
                (segment[2 + 2 * i] & 15) as usize,
            );
            if dc > 3
                || ac > 3
                || !self.d.quant_defined[quant_id]
                || (ss == 0 && ac != 0)
                || (ss != 0 && dc != 0)
            {
                return Err("invalid or undefined progressive table selector".into());
            }
            if ss != 0 && self.approx[index][0] < 0 {
                return Err("AC scan precedes first DC scan".into());
            }
            for &previous in &self.approx[index][ss..=se] {
                if (ah == 0 && previous != -1) || (ah != 0 && previous != ah as i8) {
                    return Err("duplicate initial band or incompatible refinement".into());
                }
            }
            tables[i] = if ss == 0 {
                self.d.huffman[0][dc].clone()
            } else {
                self.d.huffman[1][ac].clone()
            };
            if (ss != 0 || ah == 0) && !tables[i].valid {
                return Err("undefined progressive Huffman table".into());
            }
        }
        let blocks = self.coefficients[indices[0]].len();
        let visits = blocks as u64 * count as u64;
        if self.scans >= MAX_SCANS || visits > self.work.remaining {
            return Err(resource_limit());
        }
        self.scans += 1;
        self.work.remaining -= visits;
        let (entropy, end) = self.entropy()?;
        let mut r = BitReader::new(entropy);
        let mut predictors = [0i64; 3];
        let mut eob_run: i64 = 0;
        let mut restart_index = 0u8;
        for block in 0..blocks {
            if self.d.restart > 0 && block > 0 && block % self.d.restart == 0 {
                if eob_run != 0 {
                    return Err("EOB run crosses restart boundary".into());
                }
                consume_restart(&mut r, 0xd0 + restart_index)?;
                restart_index = (restart_index + 1) & 7;
                predictors = [0; 3];
            }
            for i in 0..count {
                let coeff = &mut self.coefficients[indices[i]][block];
                if ss == 0 {
                    if ah == 0 {
                        let category = r.decode_huffman(&tables[i]);
                        let category = match category {
                            Ok(c) if c <= 15 => c,
                            Ok(_) => {
                                return Err("invalid progressive DC category: <nil>".into());
                            }
                            Err(e) => return Err(format!("invalid progressive DC category: {e}")),
                        };
                        let difference = r.receive_extend(category as i64)?;
                        predictors[i] += difference;
                        let value = predictors[i] << al;
                        if value < i32::MIN as i64 || value > i32::MAX as i64 {
                            return Err("progressive DC coefficient overflow".into());
                        }
                        coeff[0] = value as i32;
                    } else {
                        let bit = r.read_bit()?;
                        coeff[0] |= (bit as i32) << al;
                    }
                } else if ah == 0 {
                    ac_initial(&mut r, &tables[i], coeff, ss, se, al, &mut eob_run)?;
                } else {
                    ac_refine(&mut r, &tables[i], coeff, ss, se, al, &mut eob_run)?;
                }
            }
        }
        if eob_run != 0 {
            return Err("EOB run exceeds scan".into());
        }
        r.finish()?;
        for &index in &indices[..count] {
            self.approx[index][ss..=se].fill(al as i8);
            self.quant_used[self.d.components[index].quant_id as usize] = true;
        }
        self.d.seen_sos = true;
        self.d.pos = end;
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn finish(&mut self) -> Result<Vec<u8>, String> {
        let d = &self.d;
        if !d.seen_sof || !d.seen_sos || d.data.len() - d.pos > 1 {
            return Err("incomplete progressive frame or trailing data".into());
        }
        if d.components.len() == 3 && d.codestream_is_ycbcr() {
            return Err(format!(
                "{}: only raw RGB progressive12 is qualified",
                ErrorKind::UnsupportedPhotometricInterpretation.text()
            ));
        }
        for i in 0..self.d.components.len() {
            if self.approx[i].iter().any(|&al| al != 0) {
                return Err("incomplete progressive coefficient coverage/refinement".into());
            }
            let quant_id = self.d.components[i].quant_id as usize;
            let per_line = self.d.components[i].blocks_per_line;
            for b in 0..self.coefficients[i].len() {
                let mut coefficients = [0f64; 64];
                for (k, &v) in self.coefficients[i][b].iter().enumerate() {
                    coefficients[k] = v as f64 * self.d.quant[quant_id][k] as f64;
                }
                self.d
                    .write_idct_block(i, b % per_line, b / per_line, &coefficients);
            }
        }
        let d = &self.d;
        let n = d.components.len();
        let mut out = vec![0u8; d.width * d.height * n * 2];
        for y in 0..d.height {
            for x in 0..d.width {
                for (i, c) in d.components.iter().enumerate() {
                    let v = c.samples[y * c.blocks_per_line * 8 + x];
                    let o = ((y * d.width + x) * n + i) * 2;
                    out[o..o + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        Ok(out)
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn consume_restart(r: &mut BitReader<'_>, want: u8) -> Result<(), String> {
    r.validate_padding()?;
    r.bits = 0;
    if r.pos >= r.data.len() || r.data[r.pos] != 0xff {
        return Err("missing JPEG restart marker".into());
    }
    while r.pos < r.data.len() && r.data[r.pos] == 0xff {
        r.pos += 1;
    }
    if r.pos >= r.data.len() || r.data[r.pos] != want {
        return Err(format!("incorrect JPEG restart marker, want {want:02x}"));
    }
    r.pos += 1;
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn ac_initial(
    r: &mut BitReader<'_>,
    table: &HuffmanTable,
    coeff: &mut [i32; 64],
    ss: usize,
    se: usize,
    al: usize,
    eob: &mut i64,
) -> Result<(), String> {
    if *eob > 0 {
        *eob -= 1;
        return Ok(());
    }
    let mut k = ss;
    while k <= se {
        let symbol = r.decode_huffman(table)?;
        let (run, size) = ((symbol >> 4) as usize, (symbol & 15) as i64);
        if size == 0 {
            if run == 15 {
                if k + 16 > se + 1 {
                    return Err("progressive zero run exceeds band".into());
                }
                k += 16;
                continue;
            }
            let extra = r.read_bits(run)?;
            *eob = (1i64 << run) + extra - 1;
            return Ok(());
        }
        if size > 14 || k + run > se {
            return Err("invalid progressive AC category or run".into());
        }
        let value = r.receive_extend(size)?;
        k += run;
        coeff[ZIGZAG[k]] = (value as i32) << al;
        k += 1;
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn ac_refine(
    r: &mut BitReader<'_>,
    table: &HuffmanTable,
    coeff: &mut [i32; 64],
    ss: usize,
    se: usize,
    al: usize,
    eob: &mut i64,
) -> Result<(), String> {
    let step = 1i32 << al;
    let correct = |r: &mut BitReader<'_>, value: &mut i32| -> Result<(), String> {
        let bit = r.read_bit()?;
        if bit != 0 {
            if *value > 0 {
                *value += step;
            } else {
                *value -= step;
            }
        }
        Ok(())
    };
    let mut k = ss;
    if *eob == 0 {
        while k <= se {
            let symbol = r.decode_huffman(table)?;
            let (mut run, size) = ((symbol >> 4) as usize, (symbol & 15) as usize);
            let mut new_value = 0i32;
            if size == 0 && run != 15 {
                let extra = r.read_bits(run)?;
                *eob = (1i64 << run) + extra;
                break;
            }
            if size != 0 {
                if size != 1 {
                    return Err("invalid refinement category".into());
                }
                let bit = r.read_bit()?;
                new_value = step;
                if bit == 0 {
                    new_value = -step;
                }
            } else {
                run = 16;
            }
            let mut placed = false;
            while k <= se {
                if coeff[ZIGZAG[k]] != 0 {
                    correct(r, &mut coeff[ZIGZAG[k]])?;
                } else if run != 0 {
                    run -= 1;
                } else {
                    coeff[ZIGZAG[k]] = new_value;
                    placed = true;
                    k += 1;
                    break;
                }
                k += 1;
                if new_value == 0 && run == 0 {
                    placed = true;
                    break;
                }
            }
            if !placed {
                return Err("refinement run exceeds band".into());
            }
        }
    }
    if *eob > 0 {
        while k <= se {
            if coeff[ZIGZAG[k]] != 0 {
                correct(r, &mut coeff[ZIGZAG[k]])?;
            }
            k += 1;
        }
        *eob -= 1;
    }
    Ok(())
}
