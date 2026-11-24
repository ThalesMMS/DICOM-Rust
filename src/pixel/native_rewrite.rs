use crate::derivation::{self, Derivation};
use crate::display::{self, Lut, LutOptions};
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::element_setters::format_ds;
use crate::object::{Element, Object, Tag, Vr};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{PixelError, PixelErrorKind, extract_native_frames, frame_size};
use crate::pixel::transcode::{raw_element, strings_element};
use crate::textfmt;
use std::fmt;
const ERR_NATIVE_REWRITE: &str = "dicom: native pixel data rewrite unsupported";
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_SMALLEST: Tag = Tag::new(0x0028, 0x0106);
const TAG_LARGEST: Tag = Tag::new(0x0028, 0x0107);
const TAG_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0120);
const TAG_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0121);
const TAG_PIXEL_INTENSITY_SIGN: Tag = Tag::new(0x0028, 0x1041);
const TAG_WINDOW_CENTER: Tag = Tag::new(0x0028, 0x1050);
const TAG_RESCALE_INTERCEPT: Tag = Tag::new(0x0028, 0x1052);
const TAG_RESCALE_SLOPE: Tag = Tag::new(0x0028, 0x1053);
const TAG_VOI_LUT_FUNCTION: Tag = Tag::new(0x0028, 0x1056);
const TAG_PALETTE_COLOR_LUT_UID: Tag = Tag::new(0x0028, 0x1199);
const TAG_MODALITY_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3000);
const TAG_VOI_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3010);
const TAG_PRESENTATION_LUT_SHAPE: Tag = Tag::new(0x2050, 0x0020);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
const PALETTE_DESCRIPTORS: [Tag; 3] = [
    Tag::new(0x0028, 0x1101),
    Tag::new(0x0028, 0x1102),
    Tag::new(0x0028, 0x1103),
];
const PALETTE_DATA: [Tag; 3] = [
    Tag::new(0x0028, 0x1201),
    Tag::new(0x0028, 0x1202),
    Tag::new(0x0028, 0x1203),
];
const SEGMENTED_PALETTE: [Tag; 3] = [
    Tag::new(0x0028, 0x1221),
    Tag::new(0x0028, 0x1222),
    Tag::new(0x0028, 0x1223),
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeRewrite {
    pub palette_to_rgb: bool,
    pub linearize_segmented_palette: bool,
    pub ybr_full_to_rgb: bool,
    pub monochrome1_to_monochrome2: bool,
    pub planar_configuration: Option<u16>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeRewriteReport {
    pub frames: i64,
    pub steps: Vec<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RewriteError {
    Unsupported(String),
    Pixel(PixelError),
    Other(String),
    Context(CtxKind),
}
impl RewriteError {
    pub fn is_unsupported(&self) -> bool {
        matches!(self, RewriteError::Unsupported(_))
    }
}
impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewriteError::Unsupported(detail) => write!(f, "{ERR_NATIVE_REWRITE}: {detail}"),
            RewriteError::Pixel(e) => e.fmt(f),
            RewriteError::Other(text) => f.write_str(text),
            RewriteError::Context(CtxKind::Canceled) => f.write_str("context canceled"),
            RewriteError::Context(_) => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for RewriteError {}
fn unsupported(detail: impl Into<String>) -> RewriteError {
    RewriteError::Unsupported(detail.into())
}
fn context(ctx: &Ctx) -> Result<(), RewriteError> {
    ctx.err().map_or(Ok(()), |k| Err(RewriteError::Context(k)))
}
pub fn rewrite_native(
    ctx: &Ctx,
    dataset: &Object,
    rewrite: &NativeRewrite,
) -> Result<(Object, NativeRewriteReport), RewriteError> {
    let native = extract_native_frames(dataset).map_err(RewriteError::Pixel)?;
    let m = native.metadata;
    if (m.bits_allocated != 8 && m.bits_allocated != 16)
        || m.bits_stored == 0
        || m.bits_stored > m.bits_allocated
        || m.high_bit != m.bits_stored.wrapping_sub(1)
    {
        return Err(unsupported(format!(
            "Bits Allocated {}, Bits Stored {}, High Bit {}",
            m.bits_allocated, m.bits_stored, m.high_bit
        )));
    }
    let big_endian = dataset.value_big_endian();
    let mut out = Object::from_elements(dataset.elements().to_vec());
    out.set_value_big_endian(big_endian);
    let mut r = Rewriter {
        ctx,
        source: dataset,
        out,
        m,
        frames: native.data,
        big_endian,
        steps: Vec::new(),
        derived: Vec::new(),
    };
    let photometric = r.m.photometric_interpretation.trim().to_uppercase();
    let palette = photometric == "PALETTE COLOR";
    type Step = fn(&mut Rewriter<'_>, Option<u16>) -> Result<(), RewriteError>;
    let steps: [(bool, Step); 5] = [
        (rewrite.palette_to_rgb && palette, |r, _| r.palette_to_rgb()),
        (
            rewrite.linearize_segmented_palette && !rewrite.palette_to_rgb && palette,
            |r, _| r.linearize_palette(),
        ),
        (
            rewrite.ybr_full_to_rgb && photometric == "YBR_FULL",
            |r, _| r.ybr_full_to_rgb(),
        ),
        (
            rewrite.monochrome1_to_monochrome2 && photometric == "MONOCHROME1",
            |r, _| r.monochrome1_to_monochrome2(),
        ),
        (rewrite.planar_configuration.is_some(), |r, target| {
            r.planar(target.unwrap_or_default())
        }),
    ];
    for (on, apply) in steps {
        if !on {
            continue;
        }
        context(ctx)?;
        apply(&mut r, rewrite.planar_configuration)?;
    }
    let frames = r.frames.len() as i64;
    if r.steps.is_empty() {
        return Ok((
            r.out,
            NativeRewriteReport {
                frames,
                steps: Vec::new(),
            },
        ));
    }
    let mut raw: Vec<u8> = r.frames.concat();
    if raw.len() % 2 == 1 {
        raw.push(0);
    }
    let vr = if r.m.bits_allocated > 8 {
        Vr::OW
    } else {
        Vr::OB
    };
    r.out.put(raw_element(TAG_PIXEL_DATA, vr, raw));
    r.out.put(strings_element(
        TAG_PHOTOMETRIC,
        Vr::CS,
        vec![r.m.photometric_interpretation.clone()],
    ));
    for (tag, value) in [
        (TAG_SAMPLES_PER_PIXEL, r.m.samples_per_pixel),
        (TAG_BITS_ALLOCATED, r.m.bits_allocated),
        (TAG_BITS_STORED, r.m.bits_stored),
        (TAG_HIGH_BIT, r.m.high_bit),
        (TAG_PIXEL_REPRESENTATION, r.m.pixel_representation),
    ] {
        let element = r.us(tag, value);
        r.out.put(element);
    }
    if r.m.samples_per_pixel > 1 {
        let element = r.us(TAG_PLANAR_CONFIGURATION, r.m.planar_configuration);
        r.out.put(element);
    } else {
        r.out.remove(TAG_PLANAR_CONFIGURATION);
    }
    if !r.derived.is_empty() {
        let d = Derivation {
            description: r.derived.join("; "),
            ..Derivation::default()
        };
        derivation::apply(Some(&mut r.out), &d).map_err(|e| RewriteError::Other(e.to_string()))?;
    }
    Ok((
        r.out,
        NativeRewriteReport {
            frames,
            steps: r.steps,
        },
    ))
}
struct Rewriter<'a> {
    ctx: &'a Ctx,
    source: &'a Object,
    out: Object,
    m: Metadata,
    frames: Vec<Vec<u8>>,
    big_endian: bool,
    steps: Vec<String>,
    derived: Vec<String>,
}
impl Rewriter<'_> {
    fn bytes16(&self, value: u16) -> [u8; 2] {
        if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }
    fn us(&self, tag: Tag, value: u16) -> Element {
        raw_element(tag, Vr::US, self.bytes16(value).to_vec())
    }
    fn record(&mut self, step: &str, derived: bool) {
        self.steps.push(step.to_string());
        if derived {
            self.derived.push(step.to_string());
        }
    }
    fn sample(&self, frame: &[u8], i: usize) -> i64 {
        let mut v = if self.m.bits_allocated == 8 {
            frame.get(i).copied().map_or(0, u32::from)
        } else {
            let pair = frame.as_chunks::<2>().0.get(i).copied().unwrap_or_default();
            u32::from(if self.big_endian {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            })
        };
        let bits = u32::from(self.m.bits_stored);
        v &= 1u32
            .checked_shl(bits)
            .map_or(u32::MAX, |b| b.wrapping_sub(1));
        if self.m.pixel_representation == 1 && v & 1u32.wrapping_shl(bits.wrapping_sub(1)) != 0 {
            return i64::from(v).wrapping_sub(1i64.wrapping_shl(bits));
        }
        i64::from(v)
    }
    fn pixels(&self) -> usize {
        usize::from(self.m.rows).saturating_mul(usize::from(self.m.columns))
    }
    fn palette(&self) -> Result<[Lut; 3], RewriteError> {
        let palette = display::palette_from_object(self.source, LutOptions::default())
            .map_err(|e| RewriteError::Other(e.to_string()))?;
        match palette {
            Some(display::PaletteColorLut {
                red: Some(r),
                green: Some(g),
                blue: Some(b),
            }) => Ok([r, g, b]),
            _ => Err(unsupported("PALETTE COLOR without palette LUTs")),
        }
    }
    fn palette_to_rgb(&mut self) -> Result<(), RewriteError> {
        if self.m.samples_per_pixel != 1 {
            return Err(unsupported(format!(
                "PALETTE COLOR with {} samples per pixel",
                self.m.samples_per_pixel
            )));
        }
        let luts = self.palette()?;
        let bits = luts[0].bits_per_entry;
        if luts[1].bits_per_entry != bits || luts[2].bits_per_entry != bits {
            return Err(unsupported("palette LUTs of different depths"));
        }
        let allocated: usize = if bits <= 8 { 8 } else { 16 };
        let pixels = self.pixels();
        for f in 0..self.frames.len() {
            let mut out = vec![0u8; pixels.saturating_mul(3).saturating_mul(allocated) / 8];
            let Some(frame) = self.frames.get(f) else {
                break;
            };
            for i in 0..pixels {
                let index = self.sample(frame, i);
                for (c, lut) in luts.iter().enumerate() {
                    put_sample(
                        &mut out,
                        i.saturating_mul(3).saturating_add(c),
                        allocated,
                        self.big_endian,
                        i64::from(lut.lookup(index)),
                    );
                }
            }
            if let Some(frame) = self.frames.get_mut(f) {
                *frame = out;
            }
        }
        self.m.samples_per_pixel = 3;
        self.m.photometric_interpretation = "RGB".into();
        self.m.planar_configuration = 0;
        self.m.bits_allocated = allocated as u16;
        self.m.bits_stored = bits as u16;
        self.m.high_bit = bits.saturating_sub(1) as u16;
        self.m.pixel_representation = 0;
        for tag in PALETTE_DESCRIPTORS
            .into_iter()
            .chain(PALETTE_DATA)
            .chain(SEGMENTED_PALETTE)
            .chain([
                TAG_SMALLEST,
                TAG_LARGEST,
                TAG_PIXEL_PADDING_VALUE,
                TAG_PIXEL_PADDING_RANGE_LIMIT,
                TAG_PALETTE_COLOR_LUT_UID,
            ])
        {
            self.out.remove(tag);
        }
        self.record("PALETTE COLOR to RGB", true);
        Ok(())
    }
    fn linearize_palette(&mut self) -> Result<(), RewriteError> {
        if !SEGMENTED_PALETTE.iter().any(|&t| self.source.has(t)) {
            return Ok(());
        }
        let luts = self.palette()?;
        let tags = PALETTE_DESCRIPTORS
            .into_iter()
            .zip(PALETTE_DATA)
            .zip(SEGMENTED_PALETTE);
        for (lut, ((descriptor_tag, data_tag), segmented_tag)) in luts.iter().zip(tags) {
            let entries = lut.entries.len();
            if !(1..=1 << 16).contains(&entries) {
                return Err(unsupported(format!("palette of {entries} entries")));
            }
            let count = if entries == 1 << 16 {
                0
            } else {
                entries as u16
            };
            let mut descriptor = Vec::with_capacity(6);
            for v in [count, lut.first_mapped as u16, lut.bits_per_entry as u16] {
                descriptor.extend_from_slice(&self.bytes16(v));
            }
            let vr = if self.m.pixel_representation == 1 {
                Vr::SS
            } else {
                Vr::US
            };
            self.out.put(raw_element(descriptor_tag, vr, descriptor));
            let mut data = display::encode_lut_data(&lut.entries, lut.bits_per_entry);
            if self.big_endian {
                for pair in data.as_chunks_mut::<2>().0 {
                    pair.swap(0, 1);
                }
            }
            self.out.put(raw_element(data_tag, Vr::OW, data));
            self.out.remove(segmented_tag);
        }
        self.record("segmented palette linearized", false);
        Ok(())
    }
    fn ybr_full_to_rgb(&mut self) -> Result<(), RewriteError> {
        if self.m.samples_per_pixel != 3 || self.m.pixel_representation != 0 {
            return Err(unsupported("YBR_FULL needs 3 unsigned samples per pixel"));
        }
        let bits = u32::from(self.m.bits_stored);
        let max_value = (1i64 << bits).saturating_sub(1) as f64;
        let half = (1i64 << bits.saturating_sub(1)) as f64;
        let pixels = self.pixels();
        let planar = self.m.planar_configuration == 1;
        let index = |i: usize, c: usize| {
            if planar {
                c.saturating_mul(pixels).saturating_add(i)
            } else {
                i.saturating_mul(3).saturating_add(c)
            }
        };
        let clamp = |v: f64| crate::fmath::max(0.0, crate::fmath::min(max_value, v)).round() as i64;
        let allocated = usize::from(self.m.bits_allocated);
        let big_endian = self.big_endian;
        for f in 0..self.frames.len() {
            for i in 0..pixels {
                let Some(frame) = self.frames.get(f) else {
                    break;
                };
                let y = self.sample(frame, index(i, 0)) as f64;
                let cb = self.sample(frame, index(i, 1)) as f64 - half;
                let cr = self.sample(frame, index(i, 2)) as f64 - half;
                let rgb = [
                    clamp(y + 1.402 * cr),
                    clamp(y - 0.344136 * cb - 0.714136 * cr),
                    clamp(y + 1.772 * cb),
                ];
                let Some(frame) = self.frames.get_mut(f) else {
                    break;
                };
                for (c, v) in rgb.into_iter().enumerate() {
                    put_sample(frame, index(i, c), allocated, big_endian, v);
                }
            }
        }
        self.m.photometric_interpretation = "RGB".into();
        self.record("YBR_FULL to RGB", true);
        Ok(())
    }
    fn monochrome1_to_monochrome2(&mut self) -> Result<(), RewriteError> {
        for tag in [
            TAG_MODALITY_LUT_SEQUENCE,
            TAG_VOI_LUT_SEQUENCE,
            TAG_SHARED_FUNCTIONAL_GROUPS,
        ] {
            if self.source.has(tag) {
                return Err(unsupported(format!("MONOCHROME1 with {tag}")));
            }
        }
        if self.m.samples_per_pixel != 1 {
            return Err(unsupported(format!(
                "MONOCHROME1 with {} samples per pixel",
                self.m.samples_per_pixel
            )));
        }
        let sum: i64 = if self.m.pixel_representation == 1 {
            -1
        } else {
            (1i64 << self.m.bits_stored).saturating_sub(1)
        };
        let n = self
            .pixels()
            .saturating_mul(usize::from(self.m.samples_per_pixel));
        let allocated = usize::from(self.m.bits_allocated);
        let big_endian = self.big_endian;
        for f in 0..self.frames.len() {
            for i in 0..n {
                let Some(frame) = self.frames.get(f) else {
                    break;
                };
                let v = sum.saturating_sub(self.sample(frame, i));
                if let Some(frame) = self.frames.get_mut(f) {
                    put_sample(frame, i, allocated, big_endian, v);
                }
            }
        }
        let source = self.source;
        let invert = |tag: Tag| match crate::derivedio::lookup_ints(Some(source), tag) {
            Ok(values) => match values.as_slice() {
                &[v] => Some(sum.saturating_sub(v)),
                _ => None,
            },
            _ => None,
        };
        let vr = if self.m.pixel_representation == 1 {
            Vr::SS
        } else {
            Vr::US
        };
        let smallest = invert(TAG_LARGEST);
        let largest = invert(TAG_SMALLEST);
        self.out.remove(TAG_SMALLEST);
        self.out.remove(TAG_LARGEST);
        let mut puts = Vec::new();
        if let Some(v) = smallest {
            puts.push((TAG_SMALLEST, v));
        }
        if let Some(v) = largest {
            puts.push((TAG_LARGEST, v));
        }
        for tag in [TAG_PIXEL_PADDING_VALUE, TAG_PIXEL_PADDING_RANGE_LIMIT] {
            if let Some(v) = invert(tag) {
                puts.push((tag, v));
            }
        }
        for (tag, v) in puts {
            let element = raw_element(tag, vr, self.bytes16(v as u16).to_vec());
            self.out.put(element);
        }
        if let Ok(v) = crate::derivedio::lookup_ints(Some(source), TAG_PIXEL_INTENSITY_SIGN)
            && let &[sign @ (1 | -1)] = v.as_slice()
        {
            let element = raw_element(
                TAG_PIXEL_INTENSITY_SIGN,
                Vr::SS,
                self.bytes16(sign.wrapping_neg() as i16 as u16).to_vec(),
            );
            self.out.put(element);
        }
        if source
            .get_string(TAG_PRESENTATION_LUT_SHAPE)
            .is_some_and(|shape| shape.trim() == "INVERSE")
        {
            self.out.put(strings_element(
                TAG_PRESENTATION_LUT_SHAPE,
                Vr::CS,
                vec!["IDENTITY".into()],
            ));
        }
        self.invert_windows(sum)?;
        self.m.photometric_interpretation = "MONOCHROME2".into();
        self.record("MONOCHROME1 inverted to MONOCHROME2", true);
        Ok(())
    }
    fn invert_windows(&mut self, sum: i64) -> Result<(), RewriteError> {
        let Some(centers) = self.source.get_strings(TAG_WINDOW_CENTER) else {
            return Ok(());
        };
        if centers.is_empty() {
            return Ok(());
        }
        let source = self.source;
        let number = |tag: Tag, fallback: f64| -> Result<f64, String> {
            match source.get_string(tag) {
                Some(v) if !v.trim().is_empty() => {
                    crate::numtext::parse_float(v.trim()).map_err(|e| e.to_string())
                }
                _ => Ok(fallback),
            }
        };
        let slope = number(TAG_RESCALE_SLOPE, 1.0)
            .map_err(|e| unsupported(format!("Rescale Slope: {e}")))?;
        let intercept = number(TAG_RESCALE_INTERCEPT, 0.0)
            .map_err(|e| unsupported(format!("Rescale Intercept: {e}")))?;
        let mut offset = 1.0;
        if let Some(function) = source.get_string(TAG_VOI_LUT_FUNCTION)
            && matches!(
                function.trim().to_uppercase().as_str(),
                "LINEAR_EXACT" | "SIGMOID"
            )
        {
            offset = 0.0;
        }
        let k = slope * sum as f64 + 2.0 * intercept;
        let mut out = Vec::with_capacity(centers.len());
        for c in &centers {
            let center = crate::numtext::parse_float(c.trim())
                .map_err(|_| unsupported(format!("Window Center {}", textfmt::quote(c))))?;
            let text = format_ds(k - center + offset)
                .map_err(|e| unsupported(format!("Window Center: {e}")))?;
            out.push(text);
        }
        self.out
            .put(strings_element(TAG_WINDOW_CENTER, Vr::DS, out));
        Ok(())
    }
    fn planar(&mut self, target: u16) -> Result<(), RewriteError> {
        if target > 1 {
            return Err(unsupported(format!("Planar Configuration {target}")));
        }
        if self.m.samples_per_pixel == 1 || self.m.planar_configuration == target {
            return Ok(());
        }
        for frame in self.frames.iter_mut() {
            if let Some(out) = reorder_native_frame(self.ctx, frame, &self.m, target)? {
                *frame = out;
            }
        }
        self.m.planar_configuration = target;
        self.record(&format!("Planar Configuration {target}"), false);
        Ok(())
    }
}
fn put_sample(frame: &mut [u8], i: usize, bits_allocated: usize, big_endian: bool, v: i64) {
    if bits_allocated == 8 {
        if let Some(b) = frame.get_mut(i) {
            *b = v as u8;
        }
        return;
    }
    let bytes = if big_endian {
        (v as u16).to_be_bytes()
    } else {
        (v as u16).to_le_bytes()
    };
    if let Some(pair) = frame.as_chunks_mut::<2>().0.get_mut(i) {
        *pair = bytes;
    }
}
pub(crate) fn reorder_native_frame(
    ctx: &Ctx,
    frame: &[u8],
    m: &Metadata,
    target: u16,
) -> Result<Option<Vec<u8>>, RewriteError> {
    context(ctx)?;
    if target > 1 || m.planar_configuration > 1 || (m.bits_allocated != 8 && m.bits_allocated != 16)
    {
        return Err(unsupported("planar sample layout"));
    }
    if frame.len() as i64 != frame_size(m) {
        return Err(RewriteError::Pixel(PixelError::new(
            PixelErrorKind::PixelDataSizeMismatch,
            "dicom: pixel data size does not match metadata",
        )));
    }
    if m.samples_per_pixel == 1 || m.planar_configuration == target {
        return Ok(None);
    }
    let samples = usize::from(m.samples_per_pixel);
    let pixels = usize::from(m.rows).saturating_mul(usize::from(m.columns));
    let width = usize::from(m.bits_allocated) / 8;
    let mut out = vec![0u8; frame.len()];
    for i in 0..pixels {
        if i % 65536 == 0 {
            context(ctx)?;
        }
        for c in 0..samples {
            let interleaved = samples
                .saturating_mul(i)
                .saturating_add(c)
                .saturating_mul(width);
            let planar = c
                .saturating_mul(pixels)
                .saturating_add(i)
                .saturating_mul(width);
            let (to, from) = if target == 1 {
                (planar, interleaved)
            } else {
                (interleaved, planar)
            };
            let (Some(dst), Some(src)) = (
                out.get_mut(to..to.saturating_add(width)),
                frame.get(from..from.saturating_add(width)),
            ) else {
                continue;
            };
            dst.copy_from_slice(src);
        }
    }
    Ok(Some(out))
}
