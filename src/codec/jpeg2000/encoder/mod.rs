#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tier1;
mod tier2;
mod transform;
use super::htj2k;
use super::j2kpart2::{self, Profile};
use super::{
    Error, ErrorKind, UID_HTJ2K, UID_HTJ2K_LOSSLESS, UID_HTJ2K_LOSSLESS_RPCL, UID_LOSSLESS,
    UID_LOSSY,
};
use super::{UID_PART2, UID_PART2_LOSSLESS};
use crate::codec::common::Metadata;
use crate::fmath;
use crate::textfmt;
use tier1::{BlockCoder, ORIENT_HH, ORIENT_HL, ORIENT_LH, ORIENT_LL};
use tier2::{CodedBlock, PacketBand, bit_length, write_packet};
use transform::{
    forward_dwt53, forward_dwt97, forward_ict, forward_rct, level_size, synthesis_norm,
};
pub const METHOD_PART1: &str = "ISO_15444_1";
pub const METHOD_HTJ2K: &str = "ISO_15444_15";
const MAX_STEP_SIZE: f64 = 65536.0;
const MAX_FRAME_SIZE: i64 = 1 << 31;
const CODE_BLOCK_EXPONENT: i64 = 6;
const CODE_BLOCK_SIZE: usize = 1 << CODE_BLOCK_EXPONENT;
const MAX_AUTO_LEVELS: i64 = 5;
const MIN_GUARD_BITS: i64 = 2;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub transfer_syntax_uid: String,
    pub color_transform: bool,
    pub levels: i64,
    pub step_size: f64,
    pub allow_lossy: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Coding {
    ht: bool,
    reversible: bool,
    mct: bool,
    rpcl: bool,
    levels: i64,
    step: f64,
    annex_j: Option<Profile>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncoderCapabilities {
    pub transfer_syntax_uid: String,
    pub bits_allocated: Vec<u16>,
    pub pixel_representations: Vec<u16>,
    pub samples_per_pixel: Vec<u16>,
    pub photometric_interpretations: Vec<String>,
    pub output_photometric_interpretations: Vec<String>,
    pub output_planar_configurations: Vec<u16>,
    pub lossless: bool,
    pub lossy_method: String,
    pub supports_multi_frame: bool,
    pub backend: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub photometric_interpretation: String,
    pub planar_configuration: Option<u16>,
}
fn unsupported(field: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedEncoderMetadata,
        format!("{}: {field}", ErrorKind::UnsupportedEncoderMetadata.text()),
    )
}
#[derive(Debug, Clone, PartialEq)]
pub struct Encoder {
    uid: String,
    coding: Coding,
}
struct Image {
    width: usize,
    height: usize,
    depth: i64,
    signed: bool,
    planes: Vec<Vec<i32>>,
}
impl Encoder {
    pub fn new(opts: &Options) -> Result<Encoder, Error> {
        let uid = opts
            .transfer_syntax_uid
            .trim_end_matches([' ', '\0'])
            .to_string();
        let mut c = Coding {
            mct: opts.color_transform,
            ..Coding::default()
        };
        match uid.as_str() {
            UID_LOSSLESS => c.reversible = true,
            UID_LOSSY => {}
            UID_HTJ2K_LOSSLESS => {
                c.reversible = true;
                c.ht = true;
            }
            UID_HTJ2K_LOSSLESS_RPCL => {
                c.reversible = true;
                c.ht = true;
                c.rpcl = true;
            }
            UID_HTJ2K => c.ht = true,
            _ => return Err(unsupported("TransferSyntaxUID")),
        }
        if opts.levels < 0 || opts.levels > 32 {
            return Err(unsupported("Levels"));
        }
        c.levels = opts.levels;
        if c.reversible {
            if opts.step_size != 0.0 {
                return Err(unsupported("StepSize"));
            }
        } else {
            if !opts.allow_lossy {
                return Err(Error::bare(ErrorKind::TranscodeLossyDisallowed));
            }
            if opts.step_size.is_nan() || opts.step_size <= 0.0 || opts.step_size > MAX_STEP_SIZE {
                return Err(unsupported("StepSize"));
            }
            c.step = opts.step_size;
        }
        Ok(Encoder { uid, coding: c })
    }
    pub fn capabilities(&self) -> EncoderCapabilities {
        let mut caps = EncoderCapabilities {
            transfer_syntax_uid: self.uid.clone(),
            bits_allocated: vec![8, 16],
            pixel_representations: vec![0, 1],
            samples_per_pixel: vec![1, 3],
            photometric_interpretations: vec![
                "MONOCHROME1".into(),
                "MONOCHROME2".into(),
                "RGB".into(),
            ],
            lossless: true,
            supports_multi_frame: true,
            backend: "pure-rust".into(),
            ..EncoderCapabilities::default()
        };
        if self.coding.reversible {
            caps.photometric_interpretations
                .push("PALETTE COLOR".into());
        } else {
            caps.lossless = false;
            caps.lossy_method = if self.coding.ht {
                METHOD_HTJ2K
            } else {
                METHOD_PART1
            }
            .into();
        }
        if self.coding.mct {
            caps.output_photometric_interpretations = vec![self.color_photometric().into()];
            caps.output_planar_configurations = vec![0];
        }
        caps
    }
    fn color_photometric(&self) -> &'static str {
        if self.coding.reversible {
            "YBR_RCT"
        } else {
            "YBR_ICT"
        }
    }
    pub fn encode_frame(&self, frame: &[u8], m: &Metadata) -> Result<EncodedFrame, Error> {
        let img = self.unpack(frame, m)?;
        let data = encode_image(&img, &self.coding)?;
        let mut encoded = EncodedFrame {
            data,
            ..EncodedFrame::default()
        };
        if self.coding.mct && m.samples_per_pixel == 3 {
            encoded.photometric_interpretation = self.color_photometric().into();
            encoded.planar_configuration = Some(0);
        }
        Ok(encoded)
    }
    fn unpack(&self, frame: &[u8], m: &Metadata) -> Result<Image, Error> {
        if m.rows == 0 {
            return Err(unsupported("Rows"));
        }
        if m.columns == 0 {
            return Err(unsupported("Columns"));
        }
        if m.bits_allocated != 8 && m.bits_allocated != 16 {
            return Err(unsupported("BitsAllocated"));
        }
        if m.bits_stored < 1 || m.bits_stored > m.bits_allocated {
            return Err(unsupported("BitsStored"));
        }
        if m.high_bit != m.bits_stored - 1 {
            return Err(unsupported("HighBit"));
        }
        if m.pixel_representation > 1 {
            return Err(unsupported("PixelRepresentation"));
        }
        if m.samples_per_pixel != 1 && m.samples_per_pixel != 3 {
            return Err(unsupported("SamplesPerPixel"));
        }
        if m.number_of_frames < 1 {
            return Err(unsupported("NumberOfFrames"));
        }
        let photometric = textfmt::to_upper(m.photometric_interpretation.trim());
        if m.samples_per_pixel == 1 {
            let palette = photometric == "PALETTE COLOR"
                && self.coding.reversible
                && m.pixel_representation == 0;
            if !palette && photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
                return Err(unsupported("PhotometricInterpretation"));
            }
            if m.planar_configuration_present && m.planar_configuration != 0 {
                return Err(unsupported("PlanarConfiguration"));
            }
        } else {
            if photometric != "RGB" {
                return Err(unsupported("PhotometricInterpretation"));
            }
            if m.pixel_representation != 0 {
                return Err(unsupported("PixelRepresentation"));
            }
            if !m.planar_configuration_present || m.planar_configuration != 0 {
                return Err(unsupported("PlanarConfiguration"));
            }
        }
        let pixels = m.rows as usize * m.columns as usize;
        let components = m.samples_per_pixel as usize;
        let width = (m.bits_allocated / 8) as usize;
        let size = (pixels * components * width) as i64;
        if size > MAX_FRAME_SIZE {
            return Err(unsupported("frame size"));
        }
        if frame.len() as i64 != size {
            return Err(Error::new(
                ErrorKind::PixelDataSizeMismatch,
                format!(
                    "{}: native frame length",
                    ErrorKind::PixelDataSizeMismatch.text()
                ),
            ));
        }
        let mut img = Image {
            width: m.columns as usize,
            height: m.rows as usize,
            depth: m.bits_stored as i64,
            signed: m.pixel_representation == 1,
            planes: vec![vec![0; pixels]; components],
        };
        let mask = ((1u64 << m.bits_stored) - 1) as u32;
        let sign_bit = 1u32 << (m.bits_stored - 1);
        for i in 0..pixels {
            for c in 0..components {
                let o = (i * components + c) * width;
                let mut raw = frame[o] as u32;
                if width == 2 {
                    raw |= (frame[o + 1] as u32) << 8;
                }
                raw &= mask;
                let mut v = raw as i32;
                if img.signed && raw & sign_bit != 0 {
                    v = v.wrapping_sub((mask as i32).wrapping_add(1));
                }
                img.planes[c][i] = v;
            }
        }
        Ok(img)
    }
}
#[derive(Debug, Clone, Default)]
struct Subband {
    orientation: i64,
    level: i64,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    gain: i64,
    coeffs: Vec<i32>,
    exponent: i64,
    mantissa: i64,
    max_mag: i32,
}
fn resolutions_of(w: usize, h: usize, levels: i64) -> Vec<Vec<Subband>> {
    let mut res = Vec::with_capacity(levels as usize + 1);
    res.push(vec![Subband {
        orientation: ORIENT_LL,
        level: levels,
        w: level_size(w, levels),
        h: level_size(h, levels),
        ..Subband::default()
    }]);
    for r in 1..=levels {
        let n = levels - r + 1;
        let (cw, ch) = (level_size(w, n - 1), level_size(h, n - 1));
        let (lw, lh) = (level_size(w, n), level_size(h, n));
        res.push(vec![
            Subband {
                orientation: ORIENT_HL,
                level: n,
                x0: lw,
                w: cw - lw,
                h: lh,
                gain: 1,
                ..Subband::default()
            },
            Subband {
                orientation: ORIENT_LH,
                level: n,
                y0: lh,
                w: lw,
                h: ch - lh,
                gain: 1,
                ..Subband::default()
            },
            Subband {
                orientation: ORIENT_HH,
                level: n,
                x0: lw,
                y0: lh,
                w: cw - lw,
                h: ch - lh,
                gain: 2,
                ..Subband::default()
            },
        ]);
    }
    res
}
fn auto_levels(w: usize, h: usize) -> i64 {
    let mut levels = 0;
    let mut side = w.min(h);
    while side >= 2 && levels < MAX_AUTO_LEVELS {
        levels += 1;
        side /= 2;
    }
    levels
}
fn encode_image(img: &Image, c: &Coding) -> Result<Vec<u8>, Error> {
    let (w, h) = (img.width, img.height);
    let mut levels = auto_levels(w, h);
    if c.levels > 0 {
        let mut limit = 0;
        let mut side = w.min(h);
        while side >= 2 && limit < 32 {
            limit += 1;
            side /= 2;
        }
        levels = c.levels.min(limit);
    }
    let components = img.planes.len();
    let mct = c.mct && components == 3;
    let mut resolutions: Vec<Vec<Vec<Subband>>> = Vec::with_capacity(components);
    if c.reversible {
        let mut planes: Vec<Vec<i32>> = img.planes.clone();
        if !img.signed {
            let shift = 1i32 << (img.depth - 1);
            for p in planes.iter_mut() {
                for v in p.iter_mut() {
                    *v = v.wrapping_sub(shift);
                }
            }
        }
        if mct {
            let (a, rest) = planes.split_at_mut(1);
            let (b, cc) = rest.split_at_mut(1);
            forward_rct(&mut a[0], &mut b[0], &mut cc[0]);
        }
        for mut plane in planes {
            forward_dwt53(&mut plane, w, h, levels);
            let mut res = cut_bands(&plane, w, h, levels);
            for r in res.iter_mut() {
                for band in r.iter_mut() {
                    band.exponent = img.depth + band.gain;
                    if mct {
                        band.exponent += 1;
                    }
                }
            }
            resolutions.push(res);
        }
    } else {
        let mut planes: Vec<Vec<f64>> = Vec::with_capacity(components);
        for p in &img.planes {
            let shift = if img.signed {
                0.0
            } else {
                (1i32 << (img.depth - 1)) as f64
            };
            planes.push(p.iter().map(|&v| v as f64 - shift).collect());
        }
        if mct {
            let (a, rest) = planes.split_at_mut(1);
            let (b, cc) = rest.split_at_mut(1);
            forward_ict(&mut a[0], &mut b[0], &mut cc[0]);
        }
        for mut plane in planes {
            forward_dwt97(&mut plane, w, h, levels);
            resolutions.push(quantize_bands(&plane, w, h, levels, img.depth, c.step));
        }
    }
    let mut guard = MIN_GUARD_BITS;
    for comp in &resolutions {
        for res in comp {
            for band in res {
                let need = bit_length(band.max_mag as i64) - band.exponent + 1;
                if need > guard {
                    guard = need;
                }
            }
        }
    }
    if guard > 7 {
        return Err(Error::new(
            ErrorKind::Other,
            "jpeg2000: coefficient range exceeds the codestream's bit-planes".into(),
        ));
    }
    let mut magb = 0;
    for comp in &resolutions {
        for res in comp {
            for band in res {
                magb = magb.max(guard + band.exponent - 1);
            }
        }
    }
    let mut coder = BlockCoder::default();
    let mut scratch = vec![0i32; CODE_BLOCK_SIZE * CODE_BLOCK_SIZE];
    let nres = resolutions[0].len();
    let mut packets: Vec<Vec<Vec<PacketBand>>> = Vec::with_capacity(nres);
    for r in 0..nres {
        let mut per_comp = Vec::with_capacity(components);
        for comp in resolutions.iter() {
            let mut bands = Vec::with_capacity(comp[r].len());
            for band in &comp[r] {
                let mut pb = PacketBand {
                    mb: guard + band.exponent - 1,
                    ..PacketBand::default()
                };
                if band.w != 0 && band.h != 0 {
                    let cols = band.w.div_ceil(CODE_BLOCK_SIZE);
                    let rows = band.h.div_ceil(CODE_BLOCK_SIZE);
                    pb.cols = cols as i64;
                    pb.rows = rows as i64;
                    for i in 0..cols * rows {
                        let (x0, y0) = (i % cols * CODE_BLOCK_SIZE, i / cols * CODE_BLOCK_SIZE);
                        pb.blocks
                            .push(code_block(&mut coder, &mut scratch, band, x0, y0, c.ht)?);
                    }
                }
                bands.push(pb);
            }
            per_comp.push(bands);
        }
        packets.push(per_comp);
    }
    let mut body = Vec::new();
    for r in &packets {
        for comp in r {
            write_packet(&mut body, comp, c.ht);
        }
    }
    Ok(assemble(
        img,
        c,
        levels,
        mct,
        guard,
        magb,
        &resolutions[0],
        &body,
    ))
}
fn cut_bands(plane: &[i32], w: usize, h: usize, levels: i64) -> Vec<Vec<Subband>> {
    let _ = h;
    let mut res = resolutions_of(w, h, levels);
    for r in res.iter_mut() {
        for band in r.iter_mut() {
            band.coeffs = vec![0; band.w * band.h];
            for y in 0..band.h {
                for x in 0..band.w {
                    let v = plane[(band.y0 + y) * w + band.x0 + x];
                    band.coeffs[y * band.w + x] = v;
                    band.max_mag = band.max_mag.max(v.wrapping_abs());
                }
            }
        }
    }
    res
}
fn quantize_bands(
    plane: &[f64],
    w: usize,
    h: usize,
    levels: i64,
    depth: i64,
    step: f64,
) -> Vec<Vec<Subband>> {
    let mut res = resolutions_of(w, h, levels);
    for r in res.iter_mut() {
        for band in r.iter_mut() {
            let horizontal = band.orientation == ORIENT_HL || band.orientation == ORIENT_HH;
            let vertical = band.orientation == ORIENT_LH || band.orientation == ORIENT_HH;
            let mut norm = 1.0;
            if band.level > 0 {
                norm =
                    synthesis_norm(band.level, horizontal) * synthesis_norm(band.level, vertical);
            }
            (band.exponent, band.mantissa) = step_parameters(step / norm, depth);
            let delta = fmath::ldexp(1.0 + band.mantissa as f64 / 2048.0, depth - band.exponent);
            band.coeffs = vec![0; band.w * band.h];
            for y in 0..band.h {
                for x in 0..band.w {
                    let v = plane[(band.y0 + y) * w + band.x0 + x];
                    let mut q = (v.abs() / delta).floor();
                    if q > htj2k::MAX_MAGNITUDE as f64 {
                        q = htj2k::MAX_MAGNITUDE as f64;
                    }
                    let mut m = fmath::f64_to_i32(q);
                    band.max_mag = band.max_mag.max(m);
                    if v < 0.0 {
                        m = m.wrapping_neg();
                    }
                    band.coeffs[y * band.w + x] = m;
                }
            }
        }
    }
    res
}
fn log2_frexp(x: f64) -> f64 {
    let (frac, exp) = fmath::frexp(x);
    if frac == 0.5 {
        return (exp - 1) as f64;
    }
    fmath::log(frac) * std::f64::consts::LOG2_E + exp as f64
}
fn step_parameters(delta: f64, depth: i64) -> (i64, i64) {
    let e = fmath::f64_to_i64(log2_frexp(delta).floor());
    let mut mu = fmath::f64_to_i64(((delta / fmath::ldexp(1.0, e) - 1.0) * 2048.0).round());
    let mut e = e;
    if mu >= 2048 {
        mu = 0;
        e += 1;
    }
    let epsilon = depth - e;
    if epsilon < 0 {
        return (0, 2047);
    }
    if epsilon > 31 {
        return (31, 0);
    }
    (epsilon, mu)
}
fn code_block(
    coder: &mut BlockCoder,
    scratch: &mut [i32],
    band: &Subband,
    x0: usize,
    y0: usize,
    ht: bool,
) -> Result<CodedBlock, Error> {
    let bw = CODE_BLOCK_SIZE.min(band.w - x0);
    let bh = CODE_BLOCK_SIZE.min(band.h - y0);
    let samples = &mut scratch[..bw * bh];
    let mut nonzero = false;
    for y in 0..bh {
        let start = (y0 + y) * band.w + x0;
        let row = &band.coeffs[start..start + bw];
        samples[y * bw..(y + 1) * bw].copy_from_slice(row);
        nonzero = nonzero || row.iter().any(|&v| v != 0);
    }
    if !nonzero {
        return Ok(CodedBlock::default());
    }
    if ht {
        return match htj2k::encode_cleanup(samples, bw as i64, bh as i64) {
            Ok(data) => Ok(CodedBlock {
                data,
                passes: 1,
                planes: 0,
            }),
            Err(e) => Err(Error::new(
                ErrorKind::Other,
                format!("jpeg2000: HT code-block: {e}"),
            )),
        };
    }
    let (data, passes, planes) = coder.encode_block(samples, bw, bh, band.orientation);
    Ok(CodedBlock {
        data,
        passes,
        planes,
    })
}
#[allow(clippy::too_many_arguments)]
fn assemble(
    img: &Image,
    c: &Coding,
    levels: i64,
    mct: bool,
    guard: i64,
    magb: i64,
    bands: &[Vec<Subband>],
    body: &[u8],
) -> Vec<u8> {
    let components = img.planes.len();
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + 256);
    let be16 = |out: &mut Vec<u8>, v: i64| out.extend_from_slice(&(v as u16).to_be_bytes());
    let be32 = |out: &mut Vec<u8>, v: i64| out.extend_from_slice(&(v as u32).to_be_bytes());
    be16(&mut out, 0xFF4F);
    be16(&mut out, 0xFF51);
    be16(&mut out, 38 + 3 * components as i64);
    let mut rsiz = 0;
    if c.ht {
        rsiz = 0x4000;
    }
    if c.annex_j.is_some() {
        rsiz = j2kpart2::CAPABILITIES as i64;
    }
    be16(&mut out, rsiz);
    be32(&mut out, img.width as i64);
    be32(&mut out, img.height as i64);
    be32(&mut out, 0);
    be32(&mut out, 0);
    be32(&mut out, img.width as i64);
    be32(&mut out, img.height as i64);
    be32(&mut out, 0);
    be32(&mut out, 0);
    be16(&mut out, components as i64);
    for _ in 0..components {
        let mut ssiz = img.depth - 1;
        if img.signed {
            ssiz |= 0x80;
        }
        out.extend_from_slice(&[ssiz as u8, 1, 1]);
    }
    if c.ht {
        be16(&mut out, 0xFF50);
        be16(&mut out, 8);
        be32(&mut out, 0x0002_0000);
        let mut ccap = magb_parameter(magb);
        if !c.reversible {
            ccap |= 0x0020;
        }
        be16(&mut out, ccap);
    }
    be16(&mut out, 0xFF52);
    be16(&mut out, 12);
    out.push(0);
    out.push(if c.rpcl { 2 } else { 0 });
    be16(&mut out, 1);
    out.push(if c.annex_j.is_some() {
        2
    } else if mct {
        1
    } else {
        0
    });
    let style = if c.ht { 0x40 } else { 0 };
    let transform = u8::from(c.reversible);
    out.extend_from_slice(&[
        levels as u8,
        (CODE_BLOCK_EXPONENT - 2) as u8,
        (CODE_BLOCK_EXPONENT - 2) as u8,
        style,
        transform,
    ]);
    be16(&mut out, 0xFF5C);
    let n: usize = bands.iter().map(Vec::len).sum();
    if c.reversible {
        be16(&mut out, 3 + n as i64);
        out.push((guard << 5) as u8);
        for res in bands {
            for band in res {
                out.push((band.exponent << 3) as u8);
            }
        }
    } else {
        be16(&mut out, 3 + 2 * n as i64);
        out.push(((guard << 5) | 2) as u8);
        for res in bands {
            for band in res {
                be16(&mut out, (band.exponent << 11) | band.mantissa);
            }
        }
    }
    if let Some(profile) = &c.annex_j {
        out.extend_from_slice(&profile.markers());
    }
    let psot = 12 + 2 + body.len() as i64;
    if c.rpcl {
        be16(&mut out, 0xFF55);
        be16(&mut out, 9);
        out.extend_from_slice(&[0, 0x50, 0]);
        be32(&mut out, psot);
    }
    be16(&mut out, 0xFF90);
    be16(&mut out, 10);
    be16(&mut out, 0);
    be32(&mut out, psot);
    out.extend_from_slice(&[0, 1]);
    be16(&mut out, 0xFF93);
    out.extend_from_slice(body);
    be16(&mut out, 0xFFD9);
    out
}
fn magb_parameter(b: i64) -> i64 {
    if b <= 8 {
        0
    } else if b < 28 {
        b - 8
    } else if b < 48 {
        13 + (b >> 2)
    } else {
        31
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Part2Encoder {
    encoder: Encoder,
    uid: String,
}
impl Part2Encoder {
    pub fn new(opts: &Options) -> Result<Part2Encoder, Error> {
        let uid = opts
            .transfer_syntax_uid
            .trim_end_matches([' ', '\0'])
            .to_string();
        if opts.color_transform {
            return Err(unsupported("ColorTransform"));
        }
        let mut opts = opts.clone();
        match uid.as_str() {
            UID_PART2_LOSSLESS => opts.transfer_syntax_uid = UID_LOSSLESS.into(),
            UID_PART2 => opts.transfer_syntax_uid = UID_LOSSY.into(),
            _ => return Err(unsupported("TransferSyntaxUID")),
        }
        let encoder = Encoder::new(&opts)?;
        Ok(Part2Encoder { encoder, uid })
    }
    pub fn capabilities(&self) -> EncoderCapabilities {
        let mut caps = self.encoder.capabilities();
        caps.transfer_syntax_uid = self.uid.clone();
        caps.samples_per_pixel = vec![1];
        caps.photometric_interpretations = vec!["MONOCHROME1".into(), "MONOCHROME2".into()];
        if !caps.lossless {
            caps.bits_allocated = vec![8];
        }
        caps
    }
    pub fn encode_frame_collection(
        &self,
        frames: &[&[u8]],
        metadata: &Metadata,
    ) -> Result<Vec<u8>, Error> {
        if metadata.samples_per_pixel != 1 {
            return Err(unsupported("SamplesPerPixel"));
        }
        let pi = textfmt::to_upper(metadata.photometric_interpretation.trim());
        if pi != "MONOCHROME1" && pi != "MONOCHROME2" {
            return Err(unsupported("PhotometricInterpretation"));
        }
        if metadata.planar_configuration_present {
            return Err(unsupported("PlanarConfiguration"));
        }
        if frames.len() as i64 != metadata.number_of_frames
            || frames.len() < 2
            || frames.len() as i64 > j2kpart2::MAX_FRAMES
        {
            return Err(unsupported("NumberOfFrames"));
        }
        let profile = Profile {
            frames: frames.len() as i64,
            bits: metadata.bits_stored as i64,
            signed: metadata.pixel_representation == 1,
            reversible: self.encoder.coding.reversible,
        };
        if !profile.valid() || (!profile.reversible && metadata.bits_allocated != 8) {
            return Err(unsupported("BitsStored"));
        }
        let samples = metadata.rows as i64 * metadata.columns as i64 * frames.len() as i64;
        if samples > j2kpart2::MAX_SAMPLES {
            return Err(Error::new(
                ErrorKind::TranscodeResourceLimit,
                format!(
                    "{}: Annex J collection samples",
                    ErrorKind::TranscodeResourceLimit.text()
                ),
            ));
        }
        let frame_size = frame_size(metadata);
        for frame in frames {
            if frame.len() as i64 != frame_size {
                return Err(Error::bare(ErrorKind::PixelDataSizeMismatch));
            }
        }
        let mut img = Image {
            width: metadata.columns as usize,
            height: metadata.rows as usize,
            depth: profile.coded_bits(),
            signed: true,
            planes: Vec::with_capacity(frames.len()),
        };
        for frame in frames {
            let unpacked = self.encoder.unpack(frame, metadata)?;
            img.planes
                .push(unpacked.planes.into_iter().next().unwrap_or_default());
        }
        for i in (1..img.planes.len()).rev() {
            let (before, after) = img.planes.split_at_mut(i);
            for (v, p) in after[0].iter_mut().zip(before[i - 1].iter()) {
                *v = v.wrapping_sub(*p);
            }
        }
        let shift = profile.shift();
        for v in img.planes[0].iter_mut() {
            *v = v.wrapping_sub(shift);
        }
        let mut coding = self.encoder.coding;
        coding.annex_j = Some(profile);
        encode_image(&img, &coding)
    }
}
pub(super) fn frame_size(m: &Metadata) -> i64 {
    let photometric = textfmt::to_upper(m.photometric_interpretation.trim());
    let bytes = (m.bits_allocated as i64 + 7) / 8;
    if (photometric == "YBR_FULL_422" || photometric == "YBR_PARTIAL_422") && m.bits_allocated >= 8
    {
        return m.rows as i64 * m.columns as i64 * 2 * bytes;
    }
    let samples = m.rows as i64 * m.columns as i64 * m.samples_per_pixel as i64;
    if m.bits_allocated < 8 {
        return (samples * m.bits_allocated as i64 + 7) / 8;
    }
    samples * bytes
}
