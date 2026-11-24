use super::bitio::{BitReader, bits_n, bits_off, div_ceil, unpack_signed, val};
use super::entropy::{SymbolReader, read_entropy_code, read_permutation};
use super::{
    Error, Image, MAX_IMAGE_PIXELS, container, corrupt, icc, limit, modular, truncated, unsupported,
};
pub(crate) const COLOUR_SPACE_RGB: u32 = 0;
pub(crate) const COLOUR_SPACE_GREY: u32 = 1;
pub(crate) const COLOUR_SPACE_XYB: u32 = 2;
pub(crate) const MAX_XYB_DIMENSION: usize = 2048;
pub(crate) const MAX_XYB_CODESTREAM: usize = 32 << 20;
#[derive(Clone, Copy, Default)]
pub(crate) struct BitDepth {
    pub float_sample: bool,
    pub bits_per_sample: u32,
    pub exp_bits: u32,
}
fn read_bit_depth(br: &mut BitReader) -> BitDepth {
    let mut d = BitDepth {
        float_sample: br.bool(),
        ..Default::default()
    };
    if !d.float_sample {
        d.bits_per_sample = br.u32(val(8), val(10), val(12), bits_off(6, 1));
    } else {
        d.bits_per_sample = br.u32(val(32), val(16), val(24), bits_off(6, 1));
        d.exp_bits = br.u(4).saturating_add(1);
    }
    d
}
#[derive(Clone, Copy)]
pub(crate) struct ExtraChannelInfo {
    pub kind: u32,
    pub depth: BitDepth,
    pub dim_shift: u32,
}
fn read_extra_channel_info(br: &mut BitReader) -> ExtraChannelInfo {
    let mut e = ExtraChannelInfo {
        kind: 0,
        depth: BitDepth {
            bits_per_sample: 8,
            ..Default::default()
        },
        dim_shift: 0,
    };
    if br.bool() {
        return e;
    }
    e.kind = br.enum_value();
    e.depth = read_bit_depth(br);
    e.dim_shift = br.u32(val(0), val(3), val(4), bits_off(3, 1));
    let name_len = br.u32(val(0), bits_n(4), bits_off(5, 16), bits_off(10, 48));
    br.skip_large(8 * u64::from(name_len));
    match e.kind {
        0 => {
            br.bool();
        }
        2 => {
            for _ in 0..4 {
                br.f16();
            }
        }
        5 => {
            br.u32(val(1), bits_n(2), bits_off(4, 3), bits_off(8, 19));
        }
        _ => {}
    }
    e
}
#[derive(Clone, Copy)]
pub(crate) struct ColourEncoding {
    pub want_icc: bool,
    pub colour_space: u32,
    pub is_srgb: bool,
}
fn read_custom_xy(br: &mut BitReader) {
    for _ in 0..2 {
        br.u32(
            bits_n(19),
            bits_off(19, 524288),
            bits_off(20, 1048576),
            bits_off(21, 2097152),
        );
    }
}
fn read_colour_encoding(br: &mut BitReader) -> ColourEncoding {
    let mut c = ColourEncoding {
        want_icc: false,
        colour_space: COLOUR_SPACE_RGB,
        is_srgb: true,
    };
    if br.bool() {
        return c;
    }
    c.want_icc = br.bool();
    c.colour_space = br.enum_value();
    c.is_srgb = false;
    if c.want_icc {
        return c;
    }
    let (mut white_point, mut primaries, mut transfer) = (0, 0, 0);
    if c.colour_space != COLOUR_SPACE_XYB {
        white_point = br.enum_value();
        if white_point == 2 {
            read_custom_xy(br);
        }
    }
    if c.colour_space != COLOUR_SPACE_XYB && c.colour_space != COLOUR_SPACE_GREY {
        primaries = br.enum_value();
        if primaries == 2 {
            read_custom_xy(br);
            read_custom_xy(br);
            read_custom_xy(br);
        }
    }
    if c.colour_space != COLOUR_SPACE_XYB {
        if br.bool() {
            br.u(24);
        } else {
            transfer = br.enum_value();
        }
    }
    c.is_srgb =
        c.colour_space == COLOUR_SPACE_RGB && white_point == 1 && primaries == 1 && transfer == 13;
    br.enum_value();
    c
}
#[derive(Clone)]
pub(crate) struct ImageMetadata {
    pub width: usize,
    pub height: usize,
    pub orientation: u32,
    pub have_animation: bool,
    pub have_timecodes: bool,
    pub have_preview: bool,
    pub depth: BitDepth,
    pub modular_16bit: bool,
    pub extra_channels: Vec<ExtraChannelInfo>,
    pub xyb_encoded: bool,
    pub custom_opsin: bool,
    pub custom_tone: bool,
    pub colour: ColourEncoding,
    pub upsampling_mask: u32,
}
fn read_size_header(br: &mut BitReader) -> (u64, u64) {
    let small = br.bool();
    let h = if small {
        u64::from(br.u(5)).saturating_add(1).saturating_mul(8)
    } else {
        u64::from(br.u32(
            bits_off(9, 1),
            bits_off(13, 1),
            bits_off(18, 1),
            bits_off(30, 1),
        ))
    };
    let ratio = br.u(3);
    let w = if ratio == 0 {
        if small {
            u64::from(br.u(5)).saturating_add(1).saturating_mul(8)
        } else {
            u64::from(br.u32(
                bits_off(9, 1),
                bits_off(13, 1),
                bits_off(18, 1),
                bits_off(30, 1),
            ))
        }
    } else {
        fixed_ratio_width(h, ratio)
    };
    (w, h)
}
fn fixed_ratio_width(h: u64, ratio: u32) -> u64 {
    const NUM: [u64; 8] = [0, 1, 12, 4, 3, 16, 5, 2];
    const DEN: [u64; 8] = [1, 1, 10, 3, 2, 9, 4, 1];
    let num = NUM.get(ratio as usize).copied().unwrap_or(0);
    let den = DEN.get(ratio as usize).copied().unwrap_or(1);
    h.saturating_mul(num).checked_div(den).unwrap_or(0)
}
fn read_preview_header(br: &mut BitReader) {
    let div8 = br.bool();
    let dim = |br: &mut BitReader| {
        if div8 {
            br.u32(val(16), val(32), bits_off(5, 1), bits_off(9, 33));
        } else {
            br.u32(
                bits_off(6, 1),
                bits_off(8, 65),
                bits_off(10, 321),
                bits_off(12, 1345),
            );
        }
    };
    dim(br);
    if br.u(3) == 0 {
        dim(br);
    }
}
fn read_image_metadata(br: &mut BitReader) -> ImageMetadata {
    let mut m = ImageMetadata {
        width: 0,
        height: 0,
        orientation: 1,
        have_animation: false,
        have_timecodes: false,
        have_preview: false,
        depth: BitDepth {
            bits_per_sample: 8,
            ..Default::default()
        },
        modular_16bit: true,
        extra_channels: Vec::new(),
        xyb_encoded: true,
        custom_opsin: false,
        custom_tone: false,
        colour: ColourEncoding {
            want_icc: false,
            colour_space: COLOUR_SPACE_RGB,
            is_srgb: true,
        },
        upsampling_mask: 0,
    };
    if !br.bool() {
        let extra_fields = br.bool();
        if extra_fields {
            m.orientation = br.u(3).saturating_add(1);
            if br.bool() {
                read_size_header(br);
            }
            m.have_preview = br.bool();
            if m.have_preview {
                read_preview_header(br);
            }
            m.have_animation = br.bool();
            if m.have_animation {
                br.u32(val(100), val(1000), bits_off(10, 1), bits_off(30, 1));
                br.u32(val(1), val(1001), bits_off(8, 1), bits_off(10, 1));
                br.u32(val(0), bits_n(3), bits_n(16), bits_n(32));
                m.have_timecodes = br.bool();
            }
        }
        m.depth = read_bit_depth(br);
        m.modular_16bit = br.bool();
        let n = br.u32(val(0), val(1), bits_off(4, 2), bits_off(12, 1));
        if n > 256 {
            br.fail(unsupported(format!("{n} extra channels")));
            return m;
        }
        m.extra_channels = (0..n).map(|_| read_extra_channel_info(br)).collect();
        m.xyb_encoded = br.bool();
        m.colour = read_colour_encoding(br);
        if extra_fields {
            m.custom_tone = !br.bool();
            if m.custom_tone {
                br.f16();
                br.f16();
                br.bool();
                br.f16();
            }
        }
        br.extensions();
    }
    if !br.bool() {
        if m.xyb_encoded {
            m.custom_opsin = !br.bool();
            if m.custom_opsin {
                for _ in 0..16 {
                    br.f16();
                }
            }
        }
        m.upsampling_mask = br.u(3);
        for (i, c) in [15, 55, 210].into_iter().enumerate() {
            if m.upsampling_mask & (1 << i) != 0 {
                for _ in 0..c {
                    br.f16();
                }
            }
        }
    }
    m
}
pub(crate) struct CodestreamHeader {
    pub meta: ImageMetadata,
    pub icc: Vec<u8>,
}
fn icc_context(i: usize, b1: u8, b2: u8) -> usize {
    if i <= 128 {
        return 0;
    }
    let kind1 = match b1 {
        b'a'..=b'z' | b'A'..=b'Z' => 0,
        b'0'..=b'9' | b'.' | b',' => 1,
        0 => 2,
        1 => 3,
        2..=15 => 4,
        255 => 6,
        241..=254 => 5,
        _ => 7,
    };
    let kind2 = match b2 {
        b'a'..=b'z' | b'A'..=b'Z' => 0,
        b'0'..=b'9' | b'.' | b',' => 1,
        0..=15 => 2,
        241..=255 => 3,
        _ => 4,
    };
    (kind2 as usize)
        .saturating_mul(8)
        .saturating_add(kind1)
        .saturating_add(1)
}
fn read_encoded_icc(br: &mut BitReader) -> Result<Vec<u8>, Error> {
    let size = br.u64();
    if size > icc::MAX_ICC_BYTES {
        return Err(limit());
    }
    let code = read_entropy_code(br, 41)?;
    let mut sr = SymbolReader::new(&code, br, 0);
    let mut out = Vec::with_capacity(size as usize);
    let (mut b1, mut b2) = (0u8, 0u8);
    for i in 0..size as usize {
        let v = sr.read_uint(br, icc_context(i, b1, b2));
        if v > 255 {
            return Err(corrupt("ICC byte out of range"));
        }
        out.push(v as u8);
        (b2, b1) = (b1, v as u8);
        br.check()?;
        if out.len() == icc::PREAMBLE_BYTES && size as usize > icc::PREAMBLE_BYTES {
            icc::check_preamble(&out, size)?;
        }
    }
    if !sr.final_state_ok() {
        return Err(corrupt("ICC ANS final state"));
    }
    Ok(out)
}
pub(crate) fn read_codestream_header(br: &mut BitReader) -> Result<CodestreamHeader, Error> {
    if br.u(16) != 0x0AFF {
        return Err(corrupt("missing codestream signature"));
    }
    let (w, h) = read_size_header(br);
    let mut m = read_image_metadata(br);
    br.check()?;
    if w == 0 || h == 0 || w.saturating_mul(h) > MAX_IMAGE_PIXELS {
        return Err(limit());
    }
    m.width = w as usize;
    m.height = h as usize;
    let mut hdr = CodestreamHeader {
        meta: m,
        icc: Vec::new(),
    };
    if hdr.meta.colour.want_icc {
        if hdr.meta.xyb_encoded {
            return Err(unsupported("ICC output profile in XYB codestream"));
        }
        let enc = read_encoded_icc(br)?;
        hdr.icc = icc::unpredict(&enc)?;
    }
    if hdr.meta.have_preview {
        return Err(unsupported("preview frame"));
    }
    br.zero_pad_to_byte();
    br.check()?;
    Ok(hdr)
}
pub(crate) const FRAME_REGULAR: u32 = 0;
pub(crate) const FRAME_LF: u32 = 1;
pub(crate) const FRAME_REFERENCE_ONLY: u32 = 2;
pub(crate) const FRAME_SKIP_PROGRESSIVE: u32 = 3;
pub(crate) const FLAG_NOISE: u64 = 1;
pub(crate) const FLAG_PATCHES: u64 = 2;
pub(crate) const FLAG_SPLINES: u64 = 16;
pub(crate) const FLAG_USE_LF: u64 = 32;
pub(crate) const FLAG_SKIP_LF_SM: u64 = 128;
pub(crate) const CHROMA_H_SHIFT: [usize; 4] = [0, 1, 1, 0];
pub(crate) const CHROMA_V_SHIFT: [usize; 4] = [0, 1, 0, 1];
#[derive(Clone, Default)]
pub(crate) struct FrameHeader {
    pub frame_type: u32,
    pub modular: bool,
    pub flags: u64,
    pub do_ycbcr: bool,
    pub jpeg_upsampling: [u32; 3],
    pub upsampling: u32,
    pub xyb_quant_scale: [u32; 2],
    pub ec_upsampling: Vec<u32>,
    pub group_size_shift: u32,
    pub num_passes: u32,
    pub pass_shift: Vec<u32>,
    pub have_crop: bool,
    pub x0: i32,
    pub y0: i32,
    pub width: usize,
    pub height: usize,
    pub is_last: bool,
    pub blend_mode: u32,
    pub gab: bool,
    pub epf_iters: u32,
    pub group_dim: usize,
    pub xsize: usize,
    pub ysize: usize,
    pub xsize_blocks: usize,
    pub ysize_blocks: usize,
    pub x_groups: usize,
    pub y_groups: usize,
    pub x_lf_groups: usize,
    pub y_lf_groups: usize,
    pub num_groups: usize,
    pub num_lf_groups: usize,
    pub max_h_shift: usize,
    pub max_v_shift: usize,
}
impl FrameHeader {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn hshift(&self, c: usize) -> usize {
        self.max_h_shift - CHROMA_H_SHIFT[self.jpeg_upsampling[c] as usize]
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn vshift(&self, c: usize) -> usize {
        self.max_v_shift - CHROMA_V_SHIFT[self.jpeg_upsampling[c] as usize]
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn compute_dims(&mut self) {
        for c in 0..3 {
            self.max_h_shift = self
                .max_h_shift
                .max(CHROMA_H_SHIFT[self.jpeg_upsampling[c] as usize]);
            self.max_v_shift = self
                .max_v_shift
                .max(CHROMA_V_SHIFT[self.jpeg_upsampling[c] as usize]);
        }
        self.group_dim = (256 >> 1) << self.group_size_shift;
        self.xsize = self.width;
        self.ysize = self.height;
        self.xsize_blocks = div_ceil(self.xsize, 8 << self.max_h_shift) << self.max_h_shift;
        self.ysize_blocks = div_ceil(self.ysize, 8 << self.max_v_shift) << self.max_v_shift;
        self.x_groups = div_ceil(self.xsize, self.group_dim);
        self.y_groups = div_ceil(self.ysize, self.group_dim);
        self.x_lf_groups = div_ceil(self.xsize_blocks, self.group_dim);
        self.y_lf_groups = div_ceil(self.ysize_blocks, self.group_dim);
        self.num_groups = self.x_groups * self.y_groups;
        self.num_lf_groups = self.x_lf_groups * self.y_lf_groups;
    }
}
fn read_blending_info(br: &mut BitReader, num_extra: usize, full_frame: bool) -> u32 {
    let mode = br.u32(val(0), val(1), val(2), bits_off(2, 3));
    if num_extra > 0 && (mode == 2 || mode == 3) {
        br.u32(val(0), val(1), val(2), bits_off(3, 3));
    }
    if num_extra > 0 && (mode == 2 || mode == 3 || mode == 4) {
        br.bool();
    }
    if mode != 0 || !full_frame {
        br.u32(val(0), val(1), val(2), val(3));
    }
    mode
}
fn crop_dim(br: &mut BitReader) -> u32 {
    br.u32(
        bits_n(8),
        bits_off(11, 256),
        bits_off(14, 2304),
        bits_off(30, 18688),
    )
}
pub(crate) fn read_frame_header(
    br: &mut BitReader,
    m: &ImageMetadata,
) -> Result<FrameHeader, Error> {
    let num_extra = m.extra_channels.len();
    let mut f = FrameHeader {
        upsampling: 1,
        xyb_quant_scale: [3, 2],
        group_size_shift: 1,
        num_passes: 1,
        is_last: true,
        gab: true,
        epf_iters: 1,
        ec_upsampling: vec![1; num_extra],
        width: m.width,
        height: m.height,
        ..Default::default()
    };
    let all_default = br.bool();
    if !all_default {
        f.frame_type = br.u(2);
        f.modular = br.u(1) == 1;
        f.flags = br.u64();
        if !m.xyb_encoded {
            f.do_ycbcr = br.bool();
        }
        if f.flags & FLAG_USE_LF == 0 {
            if f.do_ycbcr {
                for u in f.jpeg_upsampling.iter_mut() {
                    *u = br.u(2);
                }
            }
            f.upsampling = br.u32(val(1), val(2), val(4), val(8));
            for u in f.ec_upsampling.iter_mut() {
                *u = br.u32(val(1), val(2), val(4), val(8));
            }
        }
        if f.modular {
            f.group_size_shift = br.u(2);
        }
        if m.xyb_encoded && !f.modular {
            f.xyb_quant_scale[0] = br.u(3);
            f.xyb_quant_scale[1] = br.u(3);
        }
        if f.frame_type != FRAME_REFERENCE_ONLY {
            f.num_passes = br.u32(val(1), val(2), val(3), bits_off(3, 4));
            if f.num_passes != 1 {
                let num_ds = br.u32(val(0), val(1), val(2), bits_off(1, 3));
                if num_ds > f.num_passes.saturating_sub(1) {
                    return Err(corrupt("too many downsampling passes"));
                }
                f.pass_shift = vec![0; f.num_passes as usize];
                let shifted = (f.num_passes as usize).saturating_sub(1);
                for shift in f.pass_shift.iter_mut().take(shifted) {
                    *shift = br.u(2);
                }
                for _ in 0..num_ds {
                    br.u32(val(1), val(2), val(4), val(8));
                }
                for _ in 0..num_ds {
                    br.u32(val(0), val(1), val(2), bits_n(3));
                }
            }
        }
        if f.frame_type == FRAME_LF {
            br.u(2);
        } else {
            f.have_crop = br.bool();
        }
        if f.have_crop {
            if f.frame_type != FRAME_REFERENCE_ONLY {
                f.x0 = unpack_signed(crop_dim(br));
                f.y0 = unpack_signed(crop_dim(br));
            }
            f.width = crop_dim(br) as usize;
            f.height = crop_dim(br) as usize;
        }
        let full_frame = !f.have_crop
            || (f.x0 <= 0
                && f.y0 <= 0
                && i64::from(f.x0).saturating_add(f.width as i64) >= m.width as i64
                && i64::from(f.y0).saturating_add(f.height as i64) >= m.height as i64);
        let normal = f.frame_type == FRAME_REGULAR || f.frame_type == FRAME_SKIP_PROGRESSIVE;
        if normal {
            f.blend_mode = read_blending_info(br, num_extra, full_frame);
            for _ in 0..num_extra {
                read_blending_info(br, num_extra, full_frame);
            }
            if m.have_animation {
                br.u32(val(0), val(1), bits_n(8), bits_n(32));
            }
            if m.have_timecodes {
                br.u(32);
            }
            f.is_last = br.bool();
        } else {
            f.is_last = false;
        }
        if f.frame_type != FRAME_LF && !f.is_last {
            br.u(2);
        }
        if f.frame_type == FRAME_REFERENCE_ONLY
            || (!f.is_last && full_frame && f.frame_type != FRAME_LF)
        {
            br.bool();
        }
        let name_len = br.u32(val(0), bits_n(4), bits_off(5, 16), bits_off(10, 48));
        br.skip_large(8 * u64::from(name_len));
        if !br.bool() {
            f.gab = br.bool();
            if f.gab && br.bool() {
                for _ in 0..6 {
                    br.f16();
                }
            }
            f.epf_iters = br.u(2);
            if f.epf_iters > 0 {
                if !f.modular && br.bool() {
                    for _ in 0..8 {
                        br.f16();
                    }
                }
                if br.bool() {
                    for _ in 0..5 {
                        br.f16();
                    }
                }
                if br.bool() {
                    if !f.modular {
                        br.f16();
                    }
                    br.f16();
                    br.f16();
                    br.f16();
                }
                if f.modular {
                    br.f16();
                }
            }
            br.extensions();
        }
        br.extensions();
    }
    br.check()?;
    if f.upsampling != 1 {
        return Err(unsupported(format!("frame upsampling {}", f.upsampling)));
    }
    f.compute_dims();
    Ok(f)
}
pub(crate) fn read_toc(
    br: &mut BitReader,
    f: &FrameHeader,
) -> Result<(Vec<usize>, Vec<usize>), Error> {
    let mut n = 1usize;
    if f.num_groups != 1 || f.num_passes != 1 {
        n = f
            .num_groups
            .saturating_mul(f.num_passes as usize)
            .saturating_add(f.num_lf_groups)
            .saturating_add(2);
    }
    if n > 1 << 22 {
        return Err(limit());
    }
    let mut perm = None;
    if br.bool() {
        let code = read_entropy_code(br, 8)?;
        let mut sr = SymbolReader::new(&code, br, 0);
        perm = Some(read_permutation(br, &mut sr, n, 0)?);
        if !sr.final_state_ok() {
            return Err(corrupt("TOC permutation final state"));
        }
    }
    br.zero_pad_to_byte();
    let sizes: Vec<usize> = (0..n)
        .map(|_| {
            br.u32(
                bits_n(10),
                bits_off(14, 1024),
                bits_off(22, 17408),
                bits_off(30, 4211712),
            ) as usize
        })
        .collect();
    br.zero_pad_to_byte();
    br.check()?;
    let mut offsets = Vec::with_capacity(n);
    let mut total = 0usize;
    for &s in &sizes {
        offsets.push(total);
        total = total.saturating_add(s);
    }
    if let Some(perm) = perm {
        let bad = || corrupt("TOC permutation");
        let po = perm
            .iter()
            .map(|&p| offsets.get(p).copied().ok_or_else(bad))
            .collect::<Result<_, _>>()?;
        let ps = perm
            .iter()
            .map(|&p| sizes.get(p).copied().ok_or_else(bad))
            .collect::<Result<_, _>>()?;
        return Ok((po, ps));
    }
    Ok((offsets, sizes))
}
pub(crate) fn read_first_frame(
    br: &mut BitReader,
    hdr: &CodestreamHeader,
) -> Result<(FrameHeader, Vec<usize>, Vec<usize>), Error> {
    let f = read_frame_header(br, &hdr.meta)?;
    if f.frame_type != FRAME_REGULAR || !f.is_last {
        return Err(unsupported("multi-frame or non-regular frame"));
    }
    if f.have_crop
        && (f.x0 != 0 || f.y0 != 0 || f.width != hdr.meta.width || f.height != hdr.meta.height)
    {
        return Err(unsupported("cropped frame"));
    }
    let (offsets, sizes) = read_toc(br, &f)?;
    Ok((f, offsets, sizes))
}
pub(crate) fn section_readers<'a>(
    br: &BitReader,
    data: &'a [u8],
    offsets: &[usize],
    sizes: &[usize],
) -> Result<Vec<BitReader<'a>>, Error> {
    let rest = data.get(br.byte_pos()..).ok_or_else(truncated)?;
    offsets
        .iter()
        .zip(sizes)
        .map(|(&offset, &size)| {
            let section = rest
                .get(offset..offset.saturating_add(size))
                .ok_or_else(truncated)?;
            let mut r = BitReader::new(section);
            r.guarded = br.guarded;
            Ok(r)
        })
        .collect()
}
pub(crate) fn decode(data: &[u8], allow_xyb: bool) -> Result<Image, Error> {
    let c = container::parse(data)?;
    let cs: &[u8] = &c.codestream;
    let mut br = BitReader::new(cs);
    let hdr = read_codestream_header(&mut br)?;
    if hdr.meta.xyb_encoded && !allow_xyb {
        return Err(unsupported(
            "lossy XYB requires the JPEG XL Lossy transfer syntax",
        ));
    }
    if hdr.meta.xyb_encoded
        && (hdr.meta.width > MAX_XYB_DIMENSION
            || hdr.meta.height > MAX_XYB_DIMENSION
            || cs.len() > MAX_XYB_CODESTREAM)
    {
        return Err(limit());
    }
    let (f, offsets, sizes) = read_first_frame(&mut br, &hdr)?;
    if !f.modular {
        if hdr.meta.xyb_encoded {
            return super::xyb::decode_xyb(&hdr, &mut br, cs, &f, &offsets, &sizes);
        }
        return super::render::decode_jpeg_compatible(cs);
    }
    if f.num_passes != 1 {
        return Err(unsupported("progressive modular passes"));
    }
    if f.gab || f.epf_iters != 0 {
        return Err(unsupported("restoration filters on a modular frame"));
    }
    let mut readers = section_readers(&br, cs, &offsets, &sizes)?;
    let img = modular::decode_modular_frame(&hdr, &f, &mut readers)?;
    let nc = if hdr.meta.colour.colour_space == COLOUR_SPACE_GREY {
        1
    } else {
        3
    };
    let n = f.xsize.saturating_mul(f.ysize);
    let mut out = Image {
        width: f.xsize,
        height: f.ysize,
        channels: nc,
        bits_per_sample: hdr.meta.depth.bits_per_sample,
        samples: vec![0; n.saturating_mul(nc)],
    };
    for c in 0..nc {
        let ch = img.channels.get(c).ok_or_else(|| corrupt("channel size"))?;
        if ch.w != f.xsize || ch.h != f.ysize {
            return Err(corrupt("channel size"));
        }
        for (px, &v) in out.samples.chunks_exact_mut(nc).zip(&ch.data) {
            if let Some(slot) = px.get_mut(c) {
                *slot = v;
            }
        }
    }
    Ok(out)
}
