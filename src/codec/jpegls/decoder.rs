#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::bitstream::BitReader;
use super::loco::{
    self, BASIC_RESET, BASIC_T1, BASIC_T2, BASIC_T3, LocoState, Preset, RUN_J, context_index,
    neighbors, predict, read_limited_golomb,
};
use super::{Error, Metadata, Region};
#[derive(Debug, Clone, Default)]
pub(crate) struct FrameHeader {
    pub(crate) precision: i64,
    pub(crate) rows: i64,
    pub(crate) columns: i64,
    pub(crate) components: i64,
    pub(crate) component_ids: Vec<u8>,
    pub(crate) near: i64,
    pub(crate) ilv: i64,
    pub(crate) preset: Preset,
    pub(crate) explicit_thresholds: bool,
    pub(crate) hp_transform: i64,
}
pub(crate) struct ScanData<'a> {
    pub(crate) component_ids: Vec<u8>,
    pub(crate) entropy: &'a [u8],
}
const MAX_WORKING_SET_BYTES: u64 = 512 << 20;
const INT_BYTES: u64 = 8;
pub(crate) fn split_scans(
    stream: &[u8],
    allow_hp: bool,
) -> Result<(FrameHeader, Vec<ScanData<'_>>), Error> {
    let mut header = FrameHeader::default();
    if stream.len() < 4 || !stream.starts_with(&[0xff, 0xd8]) {
        return Err(Error::invalid("missing SOI"));
    }
    let mut i = 2usize;
    let mut scans: Vec<ScanData> = Vec::new();
    let mut seen_sof = false;
    let mut seen_lse = false;
    let mut seen_hp = false;
    let mut seen_scan = [false; 256];
    let mut seen_scan_count = 0i64;
    while i.saturating_add(1) < stream.len() {
        if stream.get(i) != Some(&0xff) {
            return Err(Error::invalid("expected marker"));
        }
        while stream.get(i) == Some(&0xff) {
            i = i.saturating_add(1);
        }
        let Some(&marker) = stream.get(i) else {
            return Err(Error::invalid("truncated marker"));
        };
        i = i.saturating_add(1);
        match marker {
            0xd9 => {
                if !seen_sof {
                    return Err(Error::invalid("missing SOF55"));
                }
                if seen_scan_count != header.components {
                    return Err(Error::invalid("incomplete component coverage"));
                }
                header.preset =
                    resolve_coding_parameters(header.preset, header.precision, header.near)?;
                if header.hp_transform != 0 {
                    validate_hp_header(&header)?;
                }
                let trailing = stream.get(i..).unwrap_or_default();
                if trailing.len() > 1 || matches!(trailing, [pad] if *pad != 0) {
                    return Err(Error::invalid("trailing data after EOI"));
                }
                return Ok((header, scans));
            }
            0xf7 => {
                if seen_sof || !scans.is_empty() {
                    return Err(Error::invalid("duplicate or out-of-order SOF55"));
                }
                let (seg, next) = read_segment(stream, i)?;
                let mut parsed = parse_sof55(seg)?;
                parsed.hp_transform = header.hp_transform;
                header = parsed;
                seen_sof = true;
                i = next;
            }
            0xda => {
                if !seen_sof {
                    return Err(Error::invalid("SOS before SOF55"));
                }
                let (seg, next) = read_segment(stream, i)?;
                let (ids, near, ilv) = parse_sos(seg, &header)?;
                for &id in &ids {
                    match seen_scan.get_mut(usize::from(id)) {
                        Some(seen) if !*seen => *seen = true,
                        _ => {
                            return Err(Error::invalid(format_args!(
                                "duplicate component scan {id}"
                            )));
                        }
                    }
                    seen_scan_count = seen_scan_count.saturating_add(1);
                }
                if scans.is_empty() {
                    header.near = near;
                    header.ilv = ilv;
                } else if header.near != near || header.ilv != ilv {
                    return Err(Error::invalid("mixed SOS parameters"));
                }
                let (entropy, end) = read_entropy(stream, next)?;
                scans.push(ScanData {
                    component_ids: ids,
                    entropy,
                });
                i = end;
            }
            0xf8 => {
                if !seen_sof || !scans.is_empty() || seen_lse {
                    return Err(Error::invalid("duplicate or out-of-order LSE"));
                }
                let (seg, next) = read_segment(stream, i)?;
                let preset = parse_lse(seg)?;
                header.preset = preset;
                header.explicit_thresholds = preset.t1 != 0 || preset.t2 != 0 || preset.t3 != 0;
                seen_lse = true;
                i = next;
            }
            0xe0..=0xef | 0xfe => {
                let (seg, next) = read_segment(stream, i)?;
                if marker == 0xe8 && seg.get(2..6) == Some(b"mrfx".as_slice()) {
                    let transform = match seg {
                        &[_, _, _, _, _, _, t] => t,
                        _ => u8::MAX,
                    };
                    if transform > 3 || seen_hp || !scans.is_empty() || transform != 0 && !allow_hp
                    {
                        return Err(Error::invalid(
                            "unsupported, duplicate or misplaced HP color transform",
                        ));
                    }
                    seen_hp = true;
                    header.hp_transform = i64::from(transform);
                }
                i = next;
            }
            _ => {
                return Err(Error::invalid(format_args!(
                    "unsupported marker 0x{marker:02x}"
                )));
            }
        }
    }
    Err(Error::invalid("missing EOI"))
}
fn read_segment(stream: &[u8], i: usize) -> Result<(&[u8], usize), Error> {
    let Some(&[hi, lo]) = stream.get(i..i.saturating_add(2)) else {
        return Err(Error::invalid("truncated segment length"));
    };
    let length = usize::from(u16::from_be_bytes([hi, lo]));
    let end = i.saturating_add(length);
    match stream.get(i..end) {
        Some(segment) if length >= 2 => Ok((segment, end)),
        _ => Err(Error::invalid("invalid segment length")),
    }
}
fn parse_sof55(seg: &[u8]) -> Result<FrameHeader, Error> {
    let &[_, _, precision, r0, r1, c0, c1, nf, ..] = seg else {
        return Err(Error::invalid("truncated SOF55"));
    };
    let nf = usize::from(nf);
    if nf != 1 && nf != 3 || nf.saturating_mul(3).saturating_add(8) != seg.len() {
        return Err(Error::invalid("invalid SOF55 component count"));
    }
    let precision = i64::from(precision);
    let rows = i64::from(u16::from_be_bytes([r0, r1]));
    let columns = i64::from(u16::from_be_bytes([c0, c1]));
    if !(2..=16).contains(&precision) || rows == 0 || columns == 0 {
        return Err(Error::invalid("invalid SOF55 geometry or precision"));
    }
    let mut ids = Vec::with_capacity(nf);
    let triples = seg.get(8..).unwrap_or_default().as_chunks::<3>().0;
    for &[id, sampling, table] in triples {
        if ids.contains(&id) || sampling != 0x11 || table != 0 {
            return Err(Error::invalid("invalid SOF55 component parameters"));
        }
        ids.push(id);
    }
    Ok(FrameHeader {
        precision,
        rows,
        columns,
        components: nf as i64,
        component_ids: ids,
        ..Default::default()
    })
}
fn parse_sos(seg: &[u8], header: &FrameHeader) -> Result<(Vec<u8>, i64, i64), Error> {
    let &[_, _, ns, ..] = seg else {
        return Err(Error::invalid("truncated SOS"));
    };
    let ns = usize::from(ns);
    let need = ns.saturating_mul(2).saturating_add(6);
    if ns != 1 && ns as i64 != header.components || seg.len() != need {
        return Err(Error::invalid("invalid SOS"));
    }
    let body = seg.get(3..).unwrap_or_default();
    let (selectors, tail) = body.split_at(ns.saturating_mul(2).min(body.len()));
    let &[near, ilv, point_transform] = tail else {
        return Err(Error::invalid("invalid SOS"));
    };
    let (near, ilv) = (i64::from(near), i64::from(ilv));
    if ilv > 2 || (ns == 1) != (ilv == 0) || point_transform != 0 {
        return Err(Error::invalid("unsupported SOS parameters"));
    }
    let mut ids = Vec::with_capacity(ns);
    for (component, &[id, mapping]) in selectors.as_chunks::<2>().0.iter().enumerate() {
        let found = header.component_ids.contains(&id);
        if !found || mapping != 0 || ilv != 0 && Some(&id) != header.component_ids.get(component) {
            return Err(Error::invalid("invalid SOS component selector or order"));
        }
        ids.push(id);
    }
    Ok((ids, near, ilv))
}
fn parse_lse(seg: &[u8]) -> Result<Preset, Error> {
    let &[_, _, 1, m0, m1, a0, a1, b0, b1, c0, c1, r0, r1] = seg else {
        return Err(Error::invalid("unsupported LSE parameters"));
    };
    let be = |hi: u8, lo: u8| i64::from(u16::from_be_bytes([hi, lo]));
    Ok(Preset {
        near: 0,
        maxval: be(m0, m1),
        t1: be(a0, a1),
        t2: be(b0, b1),
        t3: be(c0, c1),
        reset: be(r0, r1),
    })
}
pub(crate) fn resolve_coding_parameters(
    mut preset: Preset,
    precision: i64,
    near: i64,
) -> Result<Preset, Error> {
    let maximum = (1i64 << precision.clamp(0, 62)).wrapping_sub(1);
    if preset.maxval == 0 {
        preset.maxval = maximum;
    }
    if !(0..=255.min(preset.maxval / 2)).contains(&near) {
        return Err(Error::invalid("NEAR exceeds MAXVAL bound"));
    }
    preset.near = near;
    let (t1, t2, t3) = loco::default_near_thresholds(preset.maxval, near);
    if preset.t1 == 0 {
        preset.t1 = t1;
    }
    if preset.t2 == 0 {
        preset.t2 = t2;
    }
    if preset.t3 == 0 {
        preset.t3 = t3;
    }
    if preset.reset == 0 {
        preset.reset = BASIC_RESET;
    }
    if preset.maxval < 1
        || preset.maxval > maximum
        || preset.t1 < near.saturating_add(1)
        || preset.t1 > preset.t2
        || preset.t2 > preset.t3
        || preset.t3 > preset.maxval
        || preset.reset < 3
        || preset.reset > 255.max(preset.maxval)
    {
        return Err(Error::invalid("invalid LSE preset coding parameters"));
    }
    Ok(preset)
}
fn read_entropy(stream: &[u8], start: usize) -> Result<(&[u8], usize), Error> {
    let mut i = start;
    while let Some(&[b, next]) = stream.get(i..i.saturating_add(2)) {
        if b == 0xff {
            if next >= 0x80 {
                let entropy = stream.get(start..i).unwrap_or_default();
                return Ok((entropy, i));
            }
            i = i.saturating_add(2);
        } else {
            i = i.saturating_add(1);
        }
    }
    Err(Error::invalid("truncated entropy"))
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn with_jai_thresholds(mut preset: Preset, precision: i64) -> Result<Preset, Error> {
    let factor = ((1i64 << precision) - 1 + 128) / 256;
    let near = preset.near;
    preset.t1 = factor * (BASIC_T1 - 2) + 2 + 3 * near;
    preset.t2 = factor * (BASIC_T2 - 3) + 3 + 5 * near;
    preset.t3 = factor * (BASIC_T3 - 4) + 4 + 7 * near;
    if preset.t1 < near + 1
        || preset.t1 > preset.t2
        || preset.t2 > preset.t3
        || preset.t3 > preset.maxval
    {
        return Err(Error::invalid(format_args!(
            "JAI thresholds do not fit MAXVAL {}",
            preset.maxval
        )));
    }
    Ok(preset)
}
pub(crate) fn validate_split_frame(
    header: &FrameHeader,
    metadata: &Metadata,
    near_lossless: bool,
) -> Result<(), Error> {
    if header.near != 0 && !near_lossless {
        return Err(Error::invalid(format_args!("NEAR={}", header.near)));
    }
    if header.rows != metadata.rows as i64 || header.columns != metadata.columns as i64 {
        return Err(Error::size(format_args!(
            "JPEG-LS size {}x{} metadata {}x{}",
            header.columns, header.rows, metadata.columns, metadata.rows
        )));
    }
    if header.components != metadata.samples_per_pixel as i64 {
        return Err(Error::size(format_args!(
            "JPEG-LS components={} SamplesPerPixel={}",
            header.components, metadata.samples_per_pixel
        )));
    }
    if header.precision != metadata.bits_stored as i64
        && header.precision != metadata.bits_allocated as i64
    {
        return Err(Error::size(format_args!(
            "JPEG-LS precision={} BitsStored={} BitsAllocated={}",
            header.precision, metadata.bits_stored, metadata.bits_allocated
        )));
    }
    if (header.ilv != 0 || near_lossless) && header.precision != metadata.bits_stored as i64 {
        return Err(Error::size(
            "JPEG-LS precision must match BitsStored for this profile",
        ));
    }
    validate_decode_allocation(header, metadata)?;
    let maxval = (1i64 << header.precision.clamp(0, 62)).wrapping_sub(1);
    if (header.ilv != 0 || near_lossless) && header.preset.maxval != maxval {
        return Err(Error::invalid(
            "custom MAXVAL is outside this qualified profile",
        ));
    }
    Ok(())
}
#[allow(clippy::arithmetic_side_effects)]
fn validate_decode_allocation(header: &FrameHeader, metadata: &Metadata) -> Result<(), Error> {
    let max_int = i64::MAX as u64;
    let pixels = header.rows as u64 * header.columns as u64;
    let components = header.components as u64;
    let bytes_per_sample = (metadata.bits_allocated / 8) as u64;
    if pixels == 0
        || components == 0
        || bytes_per_sample == 0
        || pixels > max_int
        || components > max_int / pixels
    {
        return Err(Error::invalid("decoded frame size overflow"));
    }
    let samples = pixels * components;
    if bytes_per_sample > max_int / samples {
        return Err(Error::invalid("decoded frame size overflow"));
    }
    let output = samples * bytes_per_sample;
    if samples > u64::MAX / INT_BYTES {
        return Err(Error::invalid("decoded working-set overflow"));
    }
    if samples * INT_BYTES + output > MAX_WORKING_SET_BYTES {
        return Err(Error::invalid("decoded frame exceeds working-set limit"));
    }
    Ok(())
}
pub(crate) fn validate_hp_header(h: &FrameHeader) -> Result<(), Error> {
    if h.hp_transform < 1
        || h.hp_transform > 3
        || h.components != 3
        || (h.precision != 8 && h.precision != 16)
        || h.near != 0
        || (h.ilv != 1 && h.ilv != 2)
        || h.preset.maxval != (1i64 << h.precision.clamp(0, 62)).wrapping_sub(1)
    {
        return Err(Error::invalid(
            "HP transforms require RGB8/RGB16, NEAR=0, ILV=1/2 and full-range MAXVAL",
        ));
    }
    Ok(())
}
pub(crate) fn validate_hp_metadata(h: &FrameHeader, m: &Metadata) -> Result<(), Error> {
    if m.samples_per_pixel != 3
        || m.pixel_representation != 0
        || m.planar_configuration != 0
        || !m
            .photometric_interpretation
            .trim()
            .eq_ignore_ascii_case("RGB")
        || m.bits_allocated as i64 != h.precision
        || m.bits_stored as i64 != h.precision
        || m.high_bit != m.bits_stored.wrapping_sub(1)
    {
        return Err(Error::incompatible(
            "HP transform requires canonical unsigned RGB at the codestream precision",
        ));
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn inverse_hp(native: &mut [u8], precision: i64, hp: i64) -> Result<(), Error> {
    let step = (precision / 8) as usize;
    if step != 1 && step != 2 || !(1..=3).contains(&hp) || !native.len().is_multiple_of(3 * step) {
        return Err(Error::invalid("HP sample layout"));
    }
    let mask = (1i64 << precision) - 1;
    let half = 1i64 << (precision - 1);
    let quarter = 1i64 << (precision - 2);
    for px in native.chunks_exact_mut(3 * step) {
        let load = |i: usize| {
            if step == 1 {
                px[i] as i64
            } else {
                u16::from_le_bytes([px[2 * i], px[2 * i + 1]]) as i64
            }
        };
        let (x, y, z) = (load(0), load(1), load(2));
        let (r, g, b);
        match hp {
            1 => {
                g = y;
                r = (x + g - half) & mask;
                b = (z + g - half) & mask;
            }
            2 => {
                g = y;
                r = (x + g - half) & mask;
                b = (z + (r + g) / 2 - half) & mask;
            }
            _ => {
                g = (x - (y + z) / 4 + quarter) & mask;
                r = (z + g - half) & mask;
                b = (y + g - half) & mask;
            }
        }
        for (i, v) in [r, g, b].into_iter().enumerate() {
            if step == 1 {
                px[i] = v as u8;
            } else {
                px[2 * i..2 * i + 2].copy_from_slice(&(v as u16).to_le_bytes());
            }
        }
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_line(
    r: &mut BitReader,
    st: &mut LocoState,
    recon: &mut [i64],
    width: usize,
    y: usize,
) -> Result<(), Error> {
    let mut x = 0;
    while x < width {
        let (ra, rb, rc, rd) = if x > 0 && y > 0 && x + 1 < width {
            let i = y * width + x;
            (
                recon[i - 1],
                recon[i - width],
                recon[i - width - 1],
                recon[i - width + 1],
            )
        } else {
            neighbors(recon, width, x, y)
        };
        let q1 = st.quantize(rd - rb);
        let q2 = st.quantize(rb - rc);
        let q3 = st.quantize(rc - ra);
        if q1 == 0 && q2 == 0 && q3 == 0 {
            x = decode_run(r, st, recon, width, x, y, ra)
                .map_err(|e| e.wrap(format_args!("run mode at ({x},{y})")))?;
            continue;
        }
        let ix = decode_regular(r, st, ra, rb, rc, q1, q2, q3)
            .map_err(|e| e.wrap(format_args!("regular mode at ({x},{y})")))?;
        recon[y * width + x] = ix;
        x += 1;
    }
    Ok(())
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn validate_entropy_padding(r: &mut BitReader) -> Result<(), Error> {
    let mut padding_bits = 0;
    while r.count > 0 || r.remaining_bytes() {
        let bit = r.read_bit().map_err(|e| {
            Error::invalid(format_args!("invalid entropy padding: {}", Error::from(e)))
        })?;
        padding_bits += 1;
        if padding_bits > 15 || bit != 0 {
            return Err(Error::invalid("trailing entropy data"));
        }
    }
    if r.last_was_ff {
        return Err(Error::invalid("missing stuffed padding after 0xff"));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_regular(
    r: &mut BitReader,
    st: &mut LocoState,
    ra: i64,
    rb: i64,
    rc: i64,
    q1: i64,
    q2: i64,
    q3: i64,
) -> Result<i64, Error> {
    let (q, sign) = context_index(q1, q2, q3);
    let cx = st.ctx[q];
    let px = (predict(ra, rb, rc) + sign * cx.c).clamp(0, st.maxval);
    let k = cx.k;
    let mapped = read_limited_golomb(r, k, st.limit, st.qbpp)?;
    let mut errval = st.unmap_regular(mapped, q, k);
    st.update_regular(q, errval);
    if sign < 0 {
        errval = -errval;
    }
    Ok(st.reconstruct(px, errval))
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_run(
    r: &mut BitReader,
    st: &mut LocoState,
    recon: &mut [i64],
    width: usize,
    x: usize,
    y: usize,
    ra: i64,
) -> Result<usize, Error> {
    let run_val = ra;
    let mut xx = x;
    loop {
        let bit = r.read_bit()?;
        if bit == 1 {
            let fill = 1usize << RUN_J[st.run_idx];
            let remaining = width - xx;
            if remaining >= fill {
                for _ in 0..fill {
                    recon[y * width + xx] = run_val;
                    xx += 1;
                }
                if st.run_idx < 31 {
                    st.run_idx += 1;
                }
                if xx == width {
                    return Ok(width);
                }
                continue;
            }
            while xx < width {
                recon[y * width + xx] = run_val;
                xx += 1;
            }
            return Ok(width);
        }
        let bits = RUN_J[st.run_idx];
        let mut rem = 0;
        if bits > 0 {
            rem = r.read_bits(bits)? as usize;
        }
        for _ in 0..rem {
            if xx >= width {
                return Err(Error::invalid("run exceeds line"));
            }
            recon[y * width + xx] = run_val;
            xx += 1;
        }
        if xx >= width {
            return Err(Error::invalid("missing run interruption sample"));
        }
        let glimit_index = st.run_idx;
        if st.run_idx > 0 {
            st.run_idx -= 1;
        }
        let (_, rb, _, _) = neighbors(recon, width, xx, y);
        let ri_type = if (ra - rb).abs() <= st.near { 1 } else { 0 };
        let ix = decode_run_interruption(r, st, ra, rb, glimit_index, ri_type)?;
        recon[y * width + xx] = ix;
        return Ok(xx + 1);
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_run_interruption(
    r: &mut BitReader,
    st: &mut LocoState,
    ra: i64,
    rb: i64,
    glimit_index: usize,
    ri_type: usize,
) -> Result<i64, Error> {
    let px = if ri_type == 1 { ra } else { rb };
    let q = 365 + ri_type;
    let rt = ri_type as i64;
    let mut temp = st.ctx[q].a;
    if ri_type == 1 {
        temp = st.ctx[q].a.wrapping_add(st.ctx[q].n >> 1);
    }
    let k = loco::golomb_k(temp, st.ctx[q].n);
    let glimit = st.limit - RUN_J[glimit_index] as i64 - 1;
    let mapped = read_limited_golomb(r, k, glimit, st.qbpp)?;
    let n = st.ctx[q].n;
    let nn = st.nn[ri_type];
    let pos_bias = k == 0 && 2 * nn < n;
    let t = mapped.wrapping_add(rt);
    let mut errval = if pos_bias {
        if t % 2 != 0 {
            t.wrapping_add(1) / 2
        } else {
            t.wrapping_neg() / 2
        }
    } else if t % 2 == 0 {
        t / 2
    } else {
        t.wrapping_add(1).wrapping_neg() / 2
    };
    let map_flag = (k == 0 && errval > 0 && 2 * nn < n)
        || (errval < 0 && 2 * nn >= n)
        || (errval < 0 && k != 0);
    let remapped = errval
        .wrapping_abs()
        .wrapping_mul(2)
        .wrapping_sub(rt)
        .wrapping_sub(i64::from(map_flag));
    if remapped != mapped {
        return Err(Error::invalid("run interruption mapping"));
    }
    if errval < 0 {
        st.nn[ri_type] = st.nn[ri_type].saturating_add(1);
    }
    st.ctx[q].a = st.ctx[q]
        .a
        .wrapping_add(mapped.wrapping_add(1).wrapping_sub(rt) >> 1);
    if st.ctx[q].n == st.reset {
        st.ctx[q].a >>= 1;
        st.ctx[q].n >>= 1;
        st.nn[ri_type] >>= 1;
    }
    st.ctx[q].n = st.ctx[q].n.saturating_add(1);
    if ri_type == 0 && ra > rb {
        errval = errval.wrapping_neg();
    }
    Ok(st.reconstruct(px, errval))
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_sample_line(
    r: &mut BitReader,
    st: &mut LocoState,
    planes: &mut [Vec<i64>],
    width: usize,
    y: usize,
) -> Result<(), Error> {
    let mut x = 0;
    while x < width {
        let mut a = [0i64; 3];
        let mut b = [0i64; 3];
        let mut c = [0i64; 3];
        let mut q1 = [0i64; 3];
        let mut q2 = [0i64; 3];
        let mut q3 = [0i64; 3];
        let mut run = true;
        for (component, plane) in planes.iter().enumerate() {
            let (ra, rb, rc, rd) = neighbors(plane, width, x, y);
            a[component] = ra;
            b[component] = rb;
            c[component] = rc;
            q1[component] = st.quantize(rd - rb);
            q2[component] = st.quantize(rb - rc);
            q3[component] = st.quantize(rc - ra);
            run = run && q1[component] == 0 && q2[component] == 0 && q3[component] == 0;
        }
        if run {
            x = decode_vector_run(r, st, planes, width, x, y, a)
                .map_err(|e| e.wrap(format_args!("vector run at ({x},{y})")))?;
            continue;
        }
        for (component, plane) in planes.iter_mut().enumerate() {
            let value = decode_regular(
                r,
                st,
                a[component],
                b[component],
                c[component],
                q1[component],
                q2[component],
                q3[component],
            )
            .map_err(|e| e.wrap(format_args!("sample ({x},{y}), component {component}")))?;
            plane[y * width + x] = value;
        }
        x += 1;
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_vector_run(
    r: &mut BitReader,
    st: &mut LocoState,
    planes: &mut [Vec<i64>],
    width: usize,
    mut x: usize,
    y: usize,
    values: [i64; 3],
) -> Result<usize, Error> {
    let start = x;
    while x < width {
        let bit = r.read_bit()?;
        if bit == 0 {
            let remaining = r.read_bits(RUN_J[st.run_idx])?;
            x += remaining as usize;
            if x >= width {
                return Err(Error::invalid(
                    "vector run exceeds line or lacks interruption",
                ));
            }
            break;
        }
        let segment = 1usize << RUN_J[st.run_idx];
        let remaining = width - x;
        x += segment.min(remaining);
        if segment <= remaining && st.run_idx < 31 {
            st.run_idx += 1;
        }
    }
    for (component, plane) in planes.iter_mut().enumerate() {
        for i in start..x {
            plane[y * width + i] = values[component];
        }
    }
    if x == width {
        return Ok(width);
    }
    for (component, plane) in planes.iter_mut().enumerate() {
        let (_, rb, _, _) = neighbors(plane, width, x, y);
        let value = decode_run_interruption(r, st, values[component], rb, st.run_idx, 0)?;
        plane[y * width + x] = value;
    }
    if st.run_idx > 0 {
        st.run_idx -= 1;
    }
    Ok(x + 1)
}
#[allow(clippy::too_many_arguments)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn decode_native_scan(
    entropy: &[u8],
    header: &FrameHeader,
    preset: &Preset,
    native: &mut [u8],
    region: Region,
    first_component: usize,
    bytes_per_sample: usize,
) -> Result<(), Error> {
    let width = header.columns as usize;
    let height = header.rows as usize;
    let components = if header.ilv != 0 {
        header.components as usize
    } else {
        1
    };
    let stride = header.components as usize;
    let mut planes: Vec<Vec<i64>> = (0..components).map(|_| vec![0; 3 * width]).collect();
    let mut r = BitReader::new(entropy);
    let mut st = LocoState::new(preset);
    let mut run_indices = [0usize; 3];
    let rx = region.x0 as usize;
    let rdx = (region.x1 - region.x0) as usize;
    for y in 0..height {
        let row = y.min(2);
        if header.ilv == 2 {
            decode_sample_line(&mut r, &mut st, &mut planes, width, row)
                .map_err(|e| e.wrap(format_args!("row {y}")))?;
        } else {
            for (c, plane) in planes.iter_mut().enumerate() {
                if header.ilv == 1 {
                    st.run_idx = run_indices[c];
                }
                decode_line(&mut r, &mut st, plane, width, row).map_err(|e| {
                    e.wrap(format_args!("row {y} component {}", first_component + c))
                })?;
                run_indices[c] = st.run_idx;
            }
        }
        for (c, plane) in planes.iter_mut().enumerate() {
            if (y as i64) >= region.y0 && (y as i64) < region.y1 {
                let start = row * width + rx;
                let samples = &plane[start..start + rdx];
                let offset = ((y - region.y0 as usize) * rdx * stride + first_component + c)
                    * bytes_per_sample;
                emit_native_row(
                    samples,
                    &mut native[offset..],
                    stride,
                    bytes_per_sample,
                    preset.maxval,
                )?;
            }
            if row == 2 {
                plane.copy_within(width..3 * width, 0);
            }
        }
    }
    validate_entropy_padding(&mut r)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn emit_native_row(
    samples: &[i64],
    native: &mut [u8],
    components: usize,
    bytes_per_sample: usize,
    maxval: i64,
) -> Result<(), Error> {
    for (x, &sample) in samples.iter().enumerate() {
        if sample < 0 || sample > maxval {
            return Err(Error::invalid("reconstructed sample"));
        }
        if bytes_per_sample == 1 {
            native[x * components] = sample as u8;
        } else {
            let o = x * components * 2;
            native[o..o + 2].copy_from_slice(&(sample as u16).to_le_bytes());
        }
    }
    Ok(())
}
