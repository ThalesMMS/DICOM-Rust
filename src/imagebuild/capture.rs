use super::{
    Error, SECONDARY_CAPTURE_IMAGE_STORAGE, TAG_CONTENT_DATE, TAG_CONTENT_TIME,
    TAG_CONVERSION_TYPE, TAG_IMAGE_TYPE, TAG_INSTANCE_NUMBER, TAG_MODALITY,
    TAG_PATIENT_ORIENTATION, TAG_SERIES_DESCRIPTION, TAG_SERIES_NUMBER, TAG_STUDY_INSTANCE_UID,
    empty, invalid, local_now,
};
use crate::dcmtime::ZonedTime;
use crate::derivation::{self, Derivation, DerivedCommon};
use crate::derivedio as dio;
use crate::object::element_setters::{date_element, time_element};
use crate::object::inherit::{inherit_modules, new_uid};
use crate::object::writer;
use crate::object::{Element, ElementHeader, File, Tag, Value, Vr, is_valid_uid};
pub const MULTI_FRAME_TRUE_COLOR_SECONDARY_CAPTURE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.7.4";
const MAX_RGB_CINE_FRAMES: i64 = 10000;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureKind {
    #[default]
    Nrgba,
    Rgba,
    NilNrgba,
    NilRgba,
    Other,
}
#[derive(Clone, Debug, Default)]
pub struct CaptureImage {
    pub kind: CaptureKind,
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
    pub stride: i64,
    pub pix: Vec<u8>,
}
#[derive(Clone, Debug, Default)]
pub struct SecondaryCaptureRgbOptions {
    pub common: DerivedCommon,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: i64,
    pub instance_number: i64,
    pub series_description: String,
    pub derivation: Derivation,
    pub content_time: Option<ZonedTime>,
    pub background: Option<[u8; 4]>,
    pub burned_in_annotation: String,
    pub conversion_type: String,
    pub device_manufacturer: String,
    pub device_model_name: String,
    pub device_software_versions: String,
    pub max_pixels: i64,
}
struct Layout<'a> {
    width: usize,
    height: usize,
    stride: usize,
    pix: &'a [u8],
    premultiplied: bool,
    rgb_bytes: i64,
}
fn capture_layout(frame: &CaptureImage, max_pixels: i64) -> Result<Layout<'_>, Error> {
    let premultiplied = match frame.kind {
        CaptureKind::Nrgba => false,
        CaptureKind::Rgba => true,
        CaptureKind::NilNrgba => return Err(invalid("nil NRGBA frame")),
        CaptureKind::NilRgba => return Err(invalid("nil RGBA frame")),
        CaptureKind::Other => return Err(invalid("RGB capture requires NRGBA or RGBA")),
    };
    let width = (frame.max_x as u64).wrapping_sub(frame.min_x as u64);
    let height = (frame.max_y as u64).wrapping_sub(frame.min_y as u64);
    if frame.max_x <= frame.min_x
        || frame.max_y <= frame.min_y
        || width > u16::MAX as u64
        || height > u16::MAX as u64
    {
        return Err(invalid("RGB capture dimensions"));
    }
    let rgb_bytes = rgb_capture_length(width as i64, height as i64, max_pixels)?;
    let row_bytes = width as i64 * 4;
    let len = frame.pix.len() as i64;
    if frame.stride < row_bytes
        || len < row_bytes
        || height - 1 > ((len - row_bytes) / frame.stride) as u64
    {
        return Err(invalid("RGB capture stride or buffer"));
    }
    Ok(Layout {
        width: width as usize,
        height: height as usize,
        stride: frame.stride as usize,
        pix: &frame.pix,
        premultiplied,
        rgb_bytes,
    })
}
fn rgb_capture_length(width: i64, height: i64, max_pixels: i64) -> Result<i64, Error> {
    if width <= 0 || height <= 0 || width > u16::MAX as i64 || height > u16::MAX as i64 {
        return Err(invalid("RGB capture dimensions"));
    }
    let max_pixels = if max_pixels == 0 {
        16 << 20
    } else {
        max_pixels
    };
    let count = width * height;
    let byte_count = count * 3;
    let padded = byte_count + byte_count % 2;
    if count > max_pixels || padded > u32::MAX as i64 - 1 {
        return Err(invalid("RGB capture pixel limit"));
    }
    Ok(byte_count)
}
impl Layout<'_> {
    fn encode(&self, background: [u8; 4], pixels: &mut [u8]) -> Result<(), Error> {
        let row_bytes = self.width * 4;
        let mut out = 0;
        for y in 0..self.height {
            let row = &self.pix[y * self.stride..y * self.stride + row_bytes];
            for x in (0..row_bytes).step_by(4) {
                let (r, g, b, a) = (row[x], row[x + 1], row[x + 2], row[x + 3]);
                if self.premultiplied && (r > a || g > a || b > a) {
                    return Err(invalid("invalid premultiplied RGB pixel"));
                }
                let (alpha, inverse) = (u32::from(a), u32::from(255 - a));
                let c = [r, g, b];
                for k in 0..3 {
                    let bg = u32::from(background[k]);
                    pixels[out + k] = if self.premultiplied {
                        (u32::from(c[k]) + bg * inverse / 255) as u8
                    } else {
                        ((u32::from(c[k]) * alpha + bg * inverse) / 255) as u8
                    };
                }
                out += 3;
            }
        }
        Ok(())
    }
}
pub fn build_secondary_capture_rgb(
    frame: &CaptureImage,
    opts: &SecondaryCaptureRgbOptions,
) -> Result<File, Error> {
    let layout = capture_layout(frame, opts.max_pixels)?;
    let mut file = build_rgb_capture_file(
        layout.width as i64,
        layout.height as i64,
        SECONDARY_CAPTURE_IMAGE_STORAGE,
        opts,
    )?;
    let size = layout.rgb_bytes;
    let mut pixels = vec![0u8; (size + size % 2) as usize];
    layout.encode(opts.background.unwrap_or_default(), &mut pixels)?;
    file.dataset.put(dio::raw(Tag::PIXEL_DATA, Vr::OB, pixels));
    Ok(file)
}
fn lo_ok(value: &str) -> bool {
    value.chars().count() <= 64 && !value.contains(['\\', '\0'])
}
fn build_rgb_capture_file(
    width: i64,
    height: i64,
    sop_class: &str,
    opts: &SecondaryCaptureRgbOptions,
) -> Result<File, Error> {
    let background = match opts.background {
        Some(b) if b[3] == 255 => b,
        _ => {
            return Err(invalid("an explicit opaque RGB background is required"));
        }
    };
    let _ = background;
    if opts.burned_in_annotation != "YES" && opts.burned_in_annotation != "NO" {
        return Err(invalid(
            "explicit Burned In Annotation YES or NO is required",
        ));
    }
    let conversion = if opts.conversion_type.is_empty() {
        "WSD"
    } else {
        opts.conversion_type.as_str()
    };
    if !matches!(
        conversion,
        "DV" | "DI" | "DF" | "WSD" | "SD" | "SI" | "DRW" | "SYN"
    ) {
        return Err(invalid("SC conversion type"));
    }
    for value in [
        &opts.series_description,
        &opts.device_manufacturer,
        &opts.device_model_name,
        &opts.device_software_versions,
    ] {
        if !lo_ok(value) {
            return Err(invalid("SC equipment or description LO"));
        }
    }
    if opts.series_number < 0
        || opts.series_number > i32::MAX as i64
        || opts.instance_number < 0
        || opts.instance_number > i32::MAX as i64
        || opts.derivation.sources.len() > 10000
        || opts.derivation.codes.len() > 10000
    {
        return Err(invalid("SC numbering or derivation limit"));
    }
    let mut referenced_frames = 0usize;
    for source in &opts.derivation.sources {
        if !is_valid_uid(source.sop_class_uid.as_bytes())
            || !is_valid_uid(source.sop_instance_uid.as_bytes())
            || source.frames.len() > 10000 - referenced_frames
        {
            return Err(invalid("SC source reference"));
        }
        referenced_frames += source.frames.len();
    }
    let mut sop_uid = opts.sop_instance_uid.clone();
    let mut series_uid = opts.series_instance_uid.clone();
    for uid in [&mut sop_uid, &mut series_uid] {
        if uid.is_empty() {
            *uid = new_uid();
        } else if !is_valid_uid(uid.as_bytes()) {
            return Err(invalid("SC UID"));
        }
    }
    if !opts.study_instance_uid.is_empty() && !is_valid_uid(opts.study_instance_uid.as_bytes()) {
        return Err(invalid("SC study UID"));
    }
    let now = opts.content_time.unwrap_or_else(local_now);
    let mut dataset = dio::object(vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, sop_class),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &sop_uid),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &series_uid),
        dio::cs(TAG_MODALITY, "OT"),
        dio::strings(
            TAG_IMAGE_TYPE,
            Vr::CS,
            &["DERIVED".to_string(), "SECONDARY".to_string()],
        ),
        dio::cs(TAG_CONVERSION_TYPE, conversion),
        dio::is(TAG_SERIES_NUMBER, &[opts.series_number]),
        dio::is(TAG_INSTANCE_NUMBER, &[opts.instance_number]),
        dio::lo(TAG_SERIES_DESCRIPTION, &opts.series_description),
        empty(TAG_PATIENT_ORIENTATION, Vr::CS),
        date_element(TAG_CONTENT_DATE, now),
        time_element(TAG_CONTENT_TIME, now),
        dio::us(dio::TAG_ROWS, &[height as u16]),
        dio::us(dio::TAG_COLUMNS, &[width as u16]),
        dio::us(dio::TAG_SAMPLES_PER_PIXEL, &[3]),
        dio::cs(dio::TAG_PHOTOMETRIC_INTERPRETATION, "RGB"),
        dio::us(Tag::new(0x0028, 0x0006), &[0]),
        dio::us(dio::TAG_BITS_ALLOCATED, &[8]),
        dio::us(dio::TAG_BITS_STORED, &[8]),
        dio::us(dio::TAG_HIGH_BIT, &[7]),
        dio::us(dio::TAG_PIXEL_REPRESENTATION, &[0]),
        dio::cs(Tag::new(0x0028, 0x0301), &opts.burned_in_annotation),
        dio::cs(Tag::new(0x0028, 0x2110), "00"),
    ]);
    if !opts.study_instance_uid.is_empty() {
        dataset.put(dio::ui(TAG_STUDY_INSTANCE_UID, &opts.study_instance_uid));
    }
    for (tag, value) in [
        (Tag::new(0x0018, 0x1016), &opts.device_manufacturer),
        (Tag::new(0x0018, 0x1018), &opts.device_model_name),
        (Tag::new(0x0018, 0x1019), &opts.device_software_versions),
    ] {
        if !value.is_empty() {
            dataset.put(dio::lo(tag, value));
        }
    }
    let mut common = opts.common.clone();
    common.now = Some(now);
    common.apply(&mut dataset, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)?;
    derivation::apply(Some(&mut dataset), &opts.derivation)?;
    Ok(dio::file(sop_class, &sop_uid, Some(dataset))?)
}
#[derive(Clone, Debug, Default)]
pub struct SecondaryCaptureRgbCineOptions {
    pub rgb: SecondaryCaptureRgbOptions,
    pub width: i64,
    pub height: i64,
    pub frame_count: i64,
    pub frame_time_ms: f64,
    pub image_type_detail: String,
}
pub type FrameProducer<'a> = Box<dyn FnMut(i64) -> Result<CaptureImage, Error> + 'a>;
pub struct RgbCineSource<'a> {
    frame_at: FrameProducer<'a>,
    width: i64,
    height: i64,
    count: i64,
    length: i64,
    background: [u8; 4],
}
pub fn build_secondary_capture_rgb_cine<'a>(
    opts: &SecondaryCaptureRgbCineOptions,
    frame_at: Option<FrameProducer<'a>>,
) -> Result<(File, RgbCineSource<'a>), Error> {
    let Some(frame_at) =
        frame_at.filter(|_| opts.frame_count > 0 && opts.frame_count <= MAX_RGB_CINE_FRAMES)
    else {
        return Err(invalid("RGB cine frame count or producer"));
    };
    let length = rgb_capture_length(opts.width, opts.height, opts.rgb.max_pixels)?;
    if length > (u32::MAX as i64 - 1) / opts.frame_count {
        return Err(invalid("RGB cine Pixel Data exceeds 32-bit length"));
    }
    if opts.frame_time_ms.is_nan() || opts.frame_time_ms <= 0.0 || opts.frame_time_ms.is_infinite()
    {
        return Err(invalid(
            "RGB cine frame interval must be finite and positive",
        ));
    }
    let detail = &opts.image_type_detail;
    if detail.len() > 16
        || detail
            .chars()
            .any(|r| !(r.is_ascii_uppercase() || r.is_ascii_digit() || r == '_' || r == ' '))
    {
        return Err(invalid("RGB cine Image Type detail"));
    }
    let mut file = build_rgb_capture_file(
        opts.width,
        opts.height,
        MULTI_FRAME_TRUE_COLOR_SECONDARY_CAPTURE_STORAGE,
        &opts.rgb,
    )?;
    file.dataset
        .set_is(dio::TAG_NUMBER_OF_FRAMES, &[opts.frame_count])?;
    let frame_time = Tag::new(0x0018, 0x1063);
    file.dataset.set_ds(frame_time, &[opts.frame_time_ms])?;
    file.dataset.set_at(Tag::new(0x0028, 0x0009), &[frame_time]);
    if !detail.is_empty() {
        file.dataset.put(dio::strings(
            TAG_IMAGE_TYPE,
            Vr::CS,
            &["DERIVED".into(), "SECONDARY".into(), detail.clone()],
        ));
    }
    Ok((
        file,
        RgbCineSource {
            frame_at,
            width: opts.width,
            height: opts.height,
            count: opts.frame_count,
            length,
            background: opts.rgb.background.unwrap_or_default(),
        },
    ))
}
const ERR_FRAME_SOURCE: &str = "dicom: invalid frame source";
impl RgbCineSource<'_> {
    pub fn frames(&self) -> i64 {
        self.count
    }
    fn write_frame(&mut self, i: i64, out: &mut Vec<u8>) -> Result<(), Error> {
        let frame = (self.frame_at)(i)?;
        let layout = capture_layout(&frame, self.length / 3)
            .map_err(|e| e.wrap(&format!("RGB cine frame {}", i + 1)))?;
        if layout.width as i64 != self.width || layout.height as i64 != self.height {
            return Err(Error {
                message: format!(
                    "{ERR_FRAME_SOURCE}: RGB cine frame {} dimensions do not match",
                    i + 1
                ),
                invalid: false,
            });
        }
        let start = out.len();
        out.resize(start + self.length as usize, 0);
        layout.encode(self.background, &mut out[start..])
    }
    pub fn write_file(&mut self, file: &File) -> Result<Vec<u8>, Error> {
        if declared_frame_count(file) != self.count {
            return Err(Error::other(format!(
                "{ERR_FRAME_SOURCE}: {} frames for Number of Frames {}",
                self.count,
                declared_frame_count(file)
            )));
        }
        let total = self.length * self.count;
        let vr = native_pixel_vr(file);
        let mut pixels = Vec::with_capacity((total + total % 2) as usize);
        for i in 0..self.count {
            self.write_frame(i, &mut pixels).map_err(|e| Error {
                message: format!(
                    "dicom: write value for tag {} with VR {} with length {}: {}",
                    Tag::PIXEL_DATA,
                    vr.as_str(),
                    total + total % 2,
                    e.message
                ),
                invalid: e.invalid,
            })?;
        }
        if total % 2 == 1 {
            pixels.push(0);
        }
        write_with_pixels(file, vr, pixels)
    }
}
fn declared_frame_count(file: &File) -> i64 {
    match file.dataset.get_string(dio::TAG_NUMBER_OF_FRAMES) {
        None => 1,
        Some(v) => {
            let v = crate::deid::trim_space(&v).to_string();
            if v.is_empty() {
                return 1;
            }
            crate::numtext::parse_int(&v).unwrap_or(-1)
        }
    }
}
pub(crate) fn native_pixel_vr(file: &File) -> Vr {
    match file.dataset.get(Tag::PIXEL_DATA) {
        Some(p) if p.vr() == Vr::OW => Vr::OW,
        Some(p) if p.vr() == Vr::OB => Vr::OB,
        _ => match file.dataset.get_ints(dio::TAG_BITS_ALLOCATED) {
            Ok(bits) if bits.len() == 1 && bits[0] > 8 => Vr::OW,
            _ => Vr::OB,
        },
    }
}
pub(crate) fn write_with_pixels(file: &File, vr: Vr, pixels: Vec<u8>) -> Result<Vec<u8>, Error> {
    let mut framed = file.clone();
    for tag in [
        Tag::PIXEL_DATA,
        Tag::new(0x7FE0, 0x0001),
        Tag::new(0x7FE0, 0x0002),
        Tag::new(0x7FE0, 0x0003),
    ] {
        framed.dataset.remove(tag);
    }
    framed.dataset.put(Element {
        header: ElementHeader {
            tag: Tag::PIXEL_DATA,
            vr,
            length: pixels.len() as u32,
            length_set: true,
        },
        value: Value::Raw(pixels),
    });
    writer::write_file(&framed).map_err(Error::from)
}
