use super::bitio::BitReader;
use super::codestream::{
    CodestreamHeader, FrameHeader, read_codestream_header, read_first_frame, section_readers,
};
use super::jbrd::{APP_EXIF, APP_ICC, APP_XMP, EXIF_TAG, JpegData, XMP_TAG, read_jpeg_data};
use super::jpegwriter::write_jpeg;
use super::vardct::{COLOUR_TILE_BLOCKS, VarDctState};
use super::{Error, ErrorKind, container, corrupt, unsupported};
const CFL_FIXED_POINT_PRECISION: u32 = 11;
pub(crate) const DEFAULT_COLOUR_FACTOR: i32 = 84;
pub fn reconstruct_jpeg(data: &[u8]) -> Result<Vec<u8>, Error> {
    let c = container::parse(data)?;
    let Some(jbrd) = c.jbrd.as_deref() else {
        return Err(Error::new(ErrorKind::NoReconstructionData, None));
    };
    let mut j = read_jpeg_data(jbrd)?;
    let (hdr, s) = read_jpeg_compatible_frame(&c.codestream)?;
    let is_gray = j.components.len() == 1;
    let is_ycbcr = j.components.len() == 3 && s.f.do_ycbcr;
    s.fill_jpeg(&mut j, is_gray, is_ycbcr)?;
    fill_app_markers(
        &mut j,
        &hdr.icc,
        c.exif.as_deref(),
        c.xmp.as_deref().unwrap_or_default(),
    )?;
    write_jpeg(&j)
}
pub(crate) fn read_jpeg_compatible_frame(
    codestream: &[u8],
) -> Result<(CodestreamHeader, VarDctState), Error> {
    let mut br = BitReader::new(codestream);
    let hdr = read_codestream_header(&mut br)?;
    if hdr.meta.xyb_encoded {
        return Err(unsupported(
            "JPEG reconstruction requires a JPEG-compatible frame",
        ));
    }
    let (f, offsets, sizes) = read_first_frame(&mut br, &hdr)?;
    if f.modular {
        return Err(corrupt("JPEG-compatible decoding requires a VarDCT frame"));
    }
    if !hdr.meta.extra_channels.is_empty() {
        return Err(unsupported("extra channels in a JPEG-compatible frame"));
    }
    let s = read_vardct_sections(&br, codestream, &f, &offsets, &sizes, false)?;
    const LO: f32 = f32::from_bits(0x3a00_5f9b);
    const HI: f32 = f32::from_bits(0x3a00_a166);
    let den = s.raw_q_table_den;
    if !s.raw_q || den < LO || den > HI {
        return Err(unsupported("quantization table is not a JPEG table"));
    }
    Ok((hdr, s))
}
pub(crate) fn read_vardct_sections(
    br: &BitReader,
    codestream: &[u8],
    f: &FrameHeader,
    offsets: &[usize],
    sizes: &[usize],
    xyb: bool,
) -> Result<VarDctState, Error> {
    let mut readers = section_readers(br, codestream, offsets, sizes)?;
    if xyb {
        let mut end = br.byte_pos();
        for (&off, &size) in offsets.iter().zip(sizes) {
            end = end.max(br.byte_pos().saturating_add(off).saturating_add(size));
        }
        if end != codestream.len()
            && !(end.saturating_add(1) == codestream.len()
                && end & 1 == 1
                && codestream.get(end) == Some(&0))
        {
            return Err(corrupt("unexpected bytes after the XYB frame"));
        }
    }
    let single = readers.len() == 1;
    let section = |i: usize| if single { 0 } else { i };
    let mut s = VarDctState::new(f, xyb);
    let missing = || corrupt("VarDCT section beyond the table of contents");
    s.read_lf_global(readers.first_mut().ok_or_else(missing)?)?;
    for g in 0..f.num_lf_groups {
        let br = readers
            .get_mut(section(g.saturating_add(1)))
            .ok_or_else(missing)?;
        s.read_lf_group(br, g)?;
    }
    let br = readers
        .get_mut(section(f.num_lf_groups.saturating_add(1)))
        .ok_or_else(missing)?;
    s.read_hf_global(br)?;
    for p in 0..f.num_passes as usize {
        for g in 0..f.num_groups {
            let i = f
                .num_groups
                .saturating_mul(p)
                .saturating_add(g)
                .saturating_add(f.num_lf_groups)
                .saturating_add(2);
            let br = readers.get_mut(section(i)).ok_or_else(missing)?;
            s.read_pass_group(br, p, g)?;
        }
    }
    if xyb {
        for reader in readers.iter_mut() {
            reader.zero_pad_to_byte();
            reader.check()?;
            if reader.byte_pos() != reader.data_len() {
                return Err(corrupt("unexpected bytes in XYB frame section"));
            }
        }
    }
    Ok(s)
}
pub(crate) fn jpeg_order(is_gray: bool, is_ycbcr: bool) -> [usize; 3] {
    if is_gray {
        [0, 0, 0]
    } else if is_ycbcr {
        [1, 0, 2]
    } else {
        [0, 1, 2]
    }
}
impl VarDctState {
    pub(crate) fn fill_jpeg(
        &self,
        j: &mut JpegData,
        is_gray: bool,
        is_ycbcr: bool,
    ) -> Result<(), Error> {
        let f = &self.f;
        j.width = f.xsize;
        j.height = f.ysize;
        let cmap = jpeg_order(is_gray, is_ycbcr);
        let bad = || corrupt("JPEG reconstruction data does not match the frame");
        let raw = |k: usize| self.raw_q_table.get(k).copied().ok_or_else(bad);
        let mut qt_set = 0u32;
        let mut qt = [[0i32; 64]; 3];
        for (c, (t, &jc)) in qt.iter_mut().zip(&cmap).enumerate() {
            let quant_c = if is_gray { 1 } else { c };
            for (k, v) in t.iter_mut().enumerate() {
                *v = raw(quant_c
                    .saturating_mul(64)
                    .saturating_add(((k % 8) << 3) | (k >> 3)))?;
            }
            let q = j.components.get(jc).ok_or_else(bad)?.quant_idx as usize;
            qt_set |= 1u32.checked_shl(q as u32).unwrap_or(0);
            j.quant.get_mut(q).ok_or_else(bad)?.values = *t;
        }
        for i in 0..j.quant.len() {
            if qt_set & 1u32.checked_shl(i as u32).unwrap_or(0) != 0 {
                continue;
            }
            let Some(previous) = i.checked_sub(1).and_then(|p| j.quant.get(p)) else {
                return Err(corrupt("first JPEG quant table unused"));
            };
            let values = previous.values;
            if let Some(q) = j.quant.get_mut(i) {
                q.values = values;
            }
        }
        let channels: &[usize] = if is_gray { &[1] } else { &[0, 1, 2] };
        for &c in channels {
            let (Some(&jc), Some(&dc_w), Some(&dc_h)) =
                (cmap.get(c), self.dc_w.get(c), self.dc_h.get(c))
            else {
                return Err(bad());
            };
            let comp = j.components.get_mut(jc).ok_or_else(bad)?;
            comp.h_samp = 1 << f.max_h_shift.saturating_sub(f.hshift(c));
            comp.v_samp = 1 << f.max_v_shift.saturating_sub(f.vshift(c));
            comp.width_blocks = dc_w;
            comp.height_blocks = dc_h;
            comp.coeffs = vec![0; dc_w.saturating_mul(dc_h).saturating_mul(64)];
        }
        let is444 = f.max_h_shift == 0 && f.max_v_shift == 0;
        let mut dc_off = [0i32; 3];
        if !f.do_ycbcr {
            for (c, off) in dc_off.iter_mut().enumerate() {
                let quant_c = if is_gray { 1 } else { c };
                *off = 1024i32
                    .checked_div(raw(quant_c.saturating_mul(64))?)
                    .unwrap_or(0);
            }
        }
        let mut scaled_q = [[0i32; 64]; 3];
        let [_, qt_y, _] = qt;
        for (s, t) in scaled_q.iter_mut().zip(&qt) {
            for ((s, &q), &y) in s.iter_mut().zip(t).zip(&qt_y) {
                *s = (1i32 << CFL_FIXED_POINT_PRECISION)
                    .wrapping_mul(y)
                    .checked_div(q)
                    .unwrap_or(0);
            }
        }
        let round = 1i32 << CFL_FIXED_POINT_PRECISION.saturating_sub(1);
        for by in 0..f.ysize_blocks {
            for bx in 0..f.xsize_blocks {
                let tile = (by / COLOUR_TILE_BLOCKS)
                    .saturating_mul(self.tiles_x)
                    .saturating_add(bx / COLOUR_TILE_BLOCKS);
                let (Some(&ytox), Some(&ytob)) = (self.ytox_map.get(tile), self.ytob_map.get(tile))
                else {
                    return Err(bad());
                };
                let cfl = [ytox, 0, ytob];
                let mut y_block = [0i32; 64];
                for c in [1usize, 0, 2] {
                    if is_gray && c != 1 {
                        continue;
                    }
                    let (hs, vs) = (f.hshift(c), f.vshift(c));
                    let (sbx, sby) = (bx >> hs, by >> vs);
                    if sbx << hs != bx || sby << vs != by {
                        continue;
                    }
                    let (Some(&jc), Some(&dc_w), Some(coeffs), Some(dc_plane)) = (
                        cmap.get(c),
                        self.dc_w.get(c),
                        self.coeffs.get(c),
                        self.dc.get(c),
                    ) else {
                        return Err(bad());
                    };
                    let at = sby.saturating_mul(dc_w).saturating_add(sbx);
                    let src = coeffs
                        .get(at.saturating_mul(64)..)
                        .and_then(|s| s.first_chunk::<64>())
                        .ok_or_else(bad)?;
                    let mut block = [0i32; 64];
                    for (k, b) in block.iter_mut().enumerate() {
                        *b = src.get(((k % 8) << 3) | (k >> 3)).copied().unwrap_or(0);
                    }
                    let use_cfl = is444 && !is_gray && (ytox != 0 || ytob != 0);
                    if use_cfl && c == 1 {
                        y_block = block;
                    } else if use_cfl {
                        let factor = cfl.get(c).copied().unwrap_or(0);
                        let scale = factor.wrapping_mul(1 << CFL_FIXED_POINT_PRECISION)
                            / DEFAULT_COLOUR_FACTOR;
                        let ratios = scaled_q.get(c).ok_or_else(bad)?;
                        for ((b, &s), &y) in block.iter_mut().zip(ratios).zip(&y_block) {
                            let coeff_scale = s.wrapping_mul(scale).wrapping_add(round)
                                >> CFL_FIXED_POINT_PRECISION;
                            *b = b.wrapping_add(
                                y.wrapping_mul(coeff_scale).wrapping_add(round)
                                    >> CFL_FIXED_POINT_PRECISION,
                            );
                        }
                    }
                    let comp = j.components.get_mut(jc).ok_or_else(bad)?;
                    let target = sby.saturating_mul(comp.width_blocks).saturating_add(sbx);
                    let dst = comp
                        .coeffs
                        .get_mut(target.saturating_mul(64)..)
                        .and_then(|d| d.first_chunk_mut::<64>())
                        .ok_or_else(bad)?;
                    for (d, &b) in dst.iter_mut().zip(&block).skip(1) {
                        *d = b.clamp(-32768, 32767) as i16;
                    }
                    let dc = dc_plane.get(at).copied().ok_or_else(bad)?;
                    let off = dc_off.get(c).copied().unwrap_or(0);
                    dst[0] = dc.wrapping_sub(off).clamp(-2047, 2047) as i16;
                }
            }
        }
        Ok(())
    }
}
fn fill_app_markers(
    j: &mut JpegData,
    icc: &[u8],
    exif: Option<&[u8]>,
    xmp: &[u8],
) -> Result<(), Error> {
    let mut pos = 0usize;
    for (m, &kind) in j.app_data.iter_mut().zip(&j.app_marker_type) {
        if kind != APP_ICC {
            continue;
        }
        let Some(run) = m.get_mut(17..) else {
            return Err(corrupt("ICC marker too small"));
        };
        let n = run.len();
        let Some(part) = icc.get(pos..pos.saturating_add(n)) else {
            return Err(corrupt("ICC profile shorter than its APP2 markers"));
        };
        run.copy_from_slice(part);
        pos = pos.saturating_add(n);
    }
    if pos != icc.len() && pos != 0 {
        return Err(corrupt("ICC profile longer than its APP2 markers"));
    }
    for (m, &kind) in j.app_data.iter_mut().zip(&j.app_marker_type) {
        match kind {
            APP_EXIF => {
                let exif = exif.unwrap_or_default();
                let Some(payload) = exif.get(4..) else {
                    return Err(corrupt("Exif box missing for an Exif marker"));
                };
                match m.get_mut(EXIF_TAG.len().saturating_add(3)..) {
                    Some(t) if t.len() == payload.len() => t.copy_from_slice(payload),
                    _ => return Err(corrupt("Exif size mismatch")),
                }
            }
            APP_XMP => match m.get_mut(XMP_TAG.len().saturating_add(3)..) {
                Some(t) if t.len() == xmp.len() => t.copy_from_slice(xmp),
                _ => return Err(corrupt("XMP size mismatch")),
            },
            _ => {}
        }
    }
    Ok(())
}
