use super::bodyparts::body_part_region;
use super::classic::{
    ContributingEquipment, append_contributing_equipment, clean_text, conversion_error, data_set,
    emit_type2, required_uid, seq, str_elem, strings_elem, transcode_error, transcode_limits, ui,
};
use super::{
    CT_IMAGE_STORAGE_UID, Error, Kind, LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID,
    LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID,
    LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID, Limits, MR_IMAGE_STORAGE_UID,
    PET_IMAGE_STORAGE_UID, normalize_limits, uint16_value,
};
use crate::dcmtime::ZonedTime;
use crate::object::{DataSet, Element, ElementHeader, File, Object, Tag, Value, Vr};
use crate::pixel::transcode::{self, DecoderRegistry, TranscodeOptions};
use crate::validation::iod::modules as iodmodules;
use std::collections::{HashMap, HashSet};
const LEGACY_DESCRIPTION: &str = "Legacy Converted Enhanced Image created from Classic Images";
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
const T_SOP_CLASS: Tag = tag(0x0008, 0x0016);
const T_SOP_INSTANCE: Tag = tag(0x0008, 0x0018);
const T_SPECIFIC_CHARSET: Tag = tag(0x0008, 0x0005);
const T_IMAGE_TYPE: Tag = tag(0x0008, 0x0008);
const T_CONTENT_DATE: Tag = tag(0x0008, 0x0023);
const T_CONTENT_TIME: Tag = tag(0x0008, 0x0033);
const T_MODALITY: Tag = tag(0x0008, 0x0060);
const T_REFERENCED_IMAGE: Tag = tag(0x0008, 0x1140);
const T_REF_SOP_CLASS: Tag = tag(0x0008, 0x1150);
const T_REF_SOP_INSTANCE: Tag = tag(0x0008, 0x1155);
const T_DERIVATION_DESCRIPTION: Tag = tag(0x0008, 0x2111);
const T_SOURCE_IMAGE: Tag = tag(0x0008, 0x2112);
const T_ANATOMIC_REGION: Tag = tag(0x0008, 0x2218);
const T_IRRADIATION_EVENT_UID: Tag = tag(0x0008, 0x3010);
const T_FRAME_TYPE: Tag = tag(0x0008, 0x9007);
const T_DERIVATION_IMAGE: Tag = tag(0x0008, 0x9124);
const T_PIXEL_PRESENTATION: Tag = tag(0x0008, 0x9205);
const T_VOLUMETRIC_PROPERTIES: Tag = tag(0x0008, 0x9206);
const T_VOLUME_BASED_CALCULATION: Tag = tag(0x0008, 0x9207);
const T_COMPLEX_IMAGE_COMPONENT: Tag = tag(0x0008, 0x9208);
const T_ACQUISITION_CONTRAST: Tag = tag(0x0008, 0x9209);
const T_DERIVATION_CODE: Tag = tag(0x0008, 0x9215);
const T_BODY_PART: Tag = tag(0x0018, 0x0015);
const T_SLICE_THICKNESS: Tag = tag(0x0018, 0x0050);
const T_SPACING_BETWEEN_SLICES: Tag = tag(0x0018, 0x0088);
const T_CONTENT_QUALIFICATION: Tag = tag(0x0018, 0x9004);
const T_CT_FRAME_TYPE: Tag = tag(0x0018, 0x9329);
const T_MR_FRAME_TYPE: Tag = tag(0x0018, 0x9226);
const T_IRRADIATION_EVENT_ID: Tag = tag(0x0018, 0x9477);
const T_PET_FRAME_TYPE: Tag = tag(0x0018, 0x9751);
const T_STUDY_UID: Tag = tag(0x0020, 0x000D);
const T_SERIES_UID: Tag = tag(0x0020, 0x000E);
const T_SERIES_NUMBER: Tag = tag(0x0020, 0x0011);
const T_ACQUISITION_NUMBER: Tag = tag(0x0020, 0x0012);
const T_INSTANCE_NUMBER: Tag = tag(0x0020, 0x0013);
const T_IPP: Tag = tag(0x0020, 0x0032);
const T_IOP: Tag = tag(0x0020, 0x0037);
const T_FRAME_OF_REFERENCE: Tag = tag(0x0020, 0x0052);
const T_LATERALITY: Tag = tag(0x0020, 0x0060);
const T_IMAGE_LATERALITY: Tag = tag(0x0020, 0x0062);
const T_POSITION_REFERENCE: Tag = tag(0x0020, 0x1040);
const T_FRAME_ANATOMY: Tag = tag(0x0020, 0x9071);
const T_FRAME_LATERALITY: Tag = tag(0x0020, 0x9072);
const T_FRAME_CONTENT: Tag = tag(0x0020, 0x9111);
const T_PLANE_POSITION: Tag = tag(0x0020, 0x9113);
const T_PLANE_ORIENTATION: Tag = tag(0x0020, 0x9116);
const T_FRAME_ACQUISITION_NUMBER: Tag = tag(0x0020, 0x9156);
const T_UNASSIGNED_SHARED: Tag = tag(0x0020, 0x9170);
const T_UNASSIGNED_PER_FRAME: Tag = tag(0x0020, 0x9171);
const T_CONVERSION_SOURCE: Tag = tag(0x0020, 0x9172);
const T_SPP: Tag = tag(0x0028, 0x0002);
const T_PHOTOMETRIC: Tag = tag(0x0028, 0x0004);
const T_PLANAR: Tag = tag(0x0028, 0x0006);
const T_NUMBER_OF_FRAMES: Tag = tag(0x0028, 0x0008);
const T_ROWS: Tag = tag(0x0028, 0x0010);
const T_COLUMNS: Tag = tag(0x0028, 0x0011);
const T_PIXEL_SPACING: Tag = tag(0x0028, 0x0030);
const T_BITS_ALLOCATED: Tag = tag(0x0028, 0x0100);
const T_BITS_STORED: Tag = tag(0x0028, 0x0101);
const T_HIGH_BIT: Tag = tag(0x0028, 0x0102);
const T_PIXEL_REPRESENTATION: Tag = tag(0x0028, 0x0103);
const T_BURNED_IN: Tag = tag(0x0028, 0x0301);
const T_WINDOW_CENTER: Tag = tag(0x0028, 0x1050);
const T_WINDOW_WIDTH: Tag = tag(0x0028, 0x1051);
const T_RESCALE_INTERCEPT: Tag = tag(0x0028, 0x1052);
const T_RESCALE_SLOPE: Tag = tag(0x0028, 0x1053);
const T_RESCALE_TYPE: Tag = tag(0x0028, 0x1054);
const T_WINDOW_EXPLANATION: Tag = tag(0x0028, 0x1055);
const T_VOI_LUT_FUNCTION: Tag = tag(0x0028, 0x1056);
const T_LOSSY: Tag = tag(0x0028, 0x2110);
const T_PIXEL_MEASURES: Tag = tag(0x0028, 0x9110);
const T_FRAME_VOI_LUT: Tag = tag(0x0028, 0x9132);
const T_PIXEL_VALUE_TRANSFORMATION: Tag = tag(0x0028, 0x9145);
const T_ACQUISITION_CONTEXT: Tag = tag(0x0040, 0x0555);
const T_PURPOSE_OF_REFERENCE: Tag = tag(0x0040, 0xA170);
const T_UNITS: Tag = tag(0x0054, 0x1001);
const T_PRESENTATION_LUT_SHAPE: Tag = tag(0x2050, 0x0020);
const T_SHARED: Tag = tag(0x5200, 0x9229);
const T_PER_FRAME: Tag = tag(0x5200, 0x9230);
const T_PIXEL_DATA: Tag = tag(0x7FE0, 0x0010);
const T_CODE_VALUE: Tag = tag(0x0008, 0x0100);
const T_CODING_SCHEME: Tag = tag(0x0008, 0x0102);
const T_CODE_MEANING: Tag = tag(0x0008, 0x0104);
const ASSIGNED: [Tag; 32] = [
    T_PIXEL_SPACING,
    T_SLICE_THICKNESS,
    T_SPACING_BETWEEN_SLICES,
    T_IPP,
    T_IOP,
    T_WINDOW_CENTER,
    T_WINDOW_WIDTH,
    T_WINDOW_EXPLANATION,
    T_VOI_LUT_FUNCTION,
    T_RESCALE_INTERCEPT,
    T_RESCALE_SLOPE,
    T_RESCALE_TYPE,
    T_REFERENCED_IMAGE,
    T_SOURCE_IMAGE,
    T_DERIVATION_DESCRIPTION,
    T_DERIVATION_CODE,
    T_ANATOMIC_REGION,
    T_IRRADIATION_EVENT_UID,
    T_SOP_CLASS,
    T_SOP_INSTANCE,
    T_PIXEL_DATA,
    T_NUMBER_OF_FRAMES,
    T_ROWS,
    T_COLUMNS,
    T_SPP,
    T_PHOTOMETRIC,
    T_BITS_ALLOCATED,
    T_BITS_STORED,
    T_HIGH_BIT,
    T_PIXEL_REPRESENTATION,
    T_PLANAR,
    T_PLANAR,
];
const REPLACED: [Tag; 8] = [
    T_SERIES_UID,
    T_SERIES_NUMBER,
    T_INSTANCE_NUMBER,
    T_CONTENT_DATE,
    T_CONTENT_TIME,
    T_SPECIFIC_CHARSET,
    T_MODALITY,
    T_IMAGE_TYPE,
];
#[derive(Clone, Copy)]
struct Target {
    target: &'static str,
    modality: &'static str,
    iod: &'static str,
    frame_type: Tag,
    value_transform_required: bool,
    frame_voi_required: bool,
}
fn legacy_target(class: &str) -> Option<Target> {
    Some(match class {
        CT_IMAGE_STORAGE_UID => Target {
            target: LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID,
            modality: "CT",
            iod: "Legacy Converted Enhanced CT Image",
            frame_type: T_CT_FRAME_TYPE,
            value_transform_required: true,
            frame_voi_required: true,
        },
        MR_IMAGE_STORAGE_UID => Target {
            target: LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID,
            modality: "MR",
            iod: "Legacy Converted Enhanced MR Image",
            frame_type: T_MR_FRAME_TYPE,
            value_transform_required: false,
            frame_voi_required: false,
        },
        PET_IMAGE_STORAGE_UID => Target {
            target: LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID,
            modality: "PT",
            iod: "Legacy Converted Enhanced PET Image",
            frame_type: T_PET_FRAME_TYPE,
            value_transform_required: true,
            frame_voi_required: true,
        },
        _ => return None,
    })
}
#[derive(Clone, Debug, Default)]
pub struct LegacyOptions {
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub decoder: Option<DecoderRegistry>,
    pub limits: Limits,
    pub equipment: ContributingEquipment,
    pub now: Option<ZonedTime>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LegacyReport {
    pub frames: i64,
    pub target_sop_class_uid: String,
    pub shared_converted: i64,
    pub per_frame_converted: i64,
    pub ordered_by: String,
    pub dropped_private_elements: i64,
}
struct Frame {
    dataset: Object,
    pixels: Vec<u8>,
    class_uid: String,
    sop_uid: String,
    order: f64,
    instance: i64,
}
fn optional_uid(uid: &str) -> Result<String, Error> {
    let uid = clean_text(uid);
    if uid.is_empty() {
        return Ok(crate::deid::uid::new_uid());
    }
    if !crate::object::is_valid_uid(uid.as_bytes()) {
        return Err(Error::wrap(Kind::InvalidOptions, "invalid UID"));
    }
    Ok(uid.to_string())
}
pub fn convert_to_legacy_enhanced(
    sources: &[Option<&File>],
    opts: &LegacyOptions,
) -> Result<(File, LegacyReport), Error> {
    let limits = normalize_limits(opts.limits).map_err(|e| conversion_error("options", -1, e))?;
    if sources.is_empty() {
        return Err(conversion_error(
            "source",
            -1,
            Error::wrap(Kind::InvalidSource, "no classic images"),
        ));
    }
    if sources.len() as i64 > limits.max_frames {
        return Err(conversion_error(
            "source",
            -1,
            Error::wrap(
                Kind::ResourceLimit,
                format!("{} images, limit {}", sources.len(), limits.max_frames),
            ),
        ));
    }
    super::classic::validate_equipment(&opts.equipment)
        .map_err(|e| conversion_error("options", -1, e))?;
    let sop_uid =
        optional_uid(&opts.sop_instance_uid).map_err(|e| conversion_error("options", -1, e))?;
    let series_uid =
        optional_uid(&opts.series_instance_uid).map_err(|e| conversion_error("options", -1, e))?;
    let (mut frames, target) = decode_frames(sources, opts.decoder, &limits)?;
    let mut report = LegacyReport {
        frames: frames.len() as i64,
        target_sop_class_uid: target.target.to_string(),
        ..LegacyReport::default()
    };
    report.ordered_by = order_frames(&mut frames).to_string();
    let pixel =
        pixel_module(&frames, &target).map_err(|e| conversion_error("pixel_data", -1, e))?;
    let Some(top_level) = iod_top_level_types(target.iod) else {
        return Err(conversion_error(
            "source",
            -1,
            Error::wrap(
                Kind::UnsupportedSopClass,
                format!("no module table for {}", target.iod),
            ),
        ));
    };
    let now = opts.now.unwrap_or_else(super::classic::now_local);
    let build = Build {
        frames: &frames,
        target,
        top_level,
    };
    let dataset = build.dataset(
        &mut report,
        pixel,
        &sop_uid,
        &series_uid,
        clean_text(&opts.series_number),
        &opts.equipment,
        &now,
    )?;
    let mut file = File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    file.rebuild_file_meta()
        .map_err(|e| conversion_error("file_meta", -1, Error::with_text(e.to_string())))?;
    let written = crate::object::writer::write_file(&file)
        .map_err(|e| conversion_error("write_validation", -1, Error::with_text(e.to_string())))?;
    if written.len() as i64 > limits.max_output_bytes {
        return Err(conversion_error(
            "output",
            -1,
            Error::wrap(Kind::ResourceLimit, "aggregate output bytes"),
        ));
    }
    Ok((file, report))
}
fn decode_frames(
    sources: &[Option<&File>],
    decoder: Option<DecoderRegistry>,
    limits: &Limits,
) -> Result<(Vec<Frame>, Target), Error> {
    let mut frames = Vec::with_capacity(sources.len());
    let mut target = None;
    let mut first_class = String::new();
    let mut identity = [String::new(), String::new(), String::new()];
    let mut charset = None;
    let mut seen = HashSet::new();
    for (i, src) in sources.iter().enumerate() {
        let i = i as i64;
        let Some(src) = src else {
            return Err(conversion_error(
                "source",
                i,
                Error::wrap(Kind::InvalidSource, "file and dataset are required"),
            ));
        };
        let class = required_uid(&src.dataset, T_SOP_CLASS)
            .map_err(|e| conversion_error("source", i, e))?;
        if i == 0 {
            let Some(t) = legacy_target(&class) else {
                return Err(conversion_error(
                    "source",
                    i,
                    Error::wrap(
                        Kind::UnsupportedSopClass,
                        format!("{class} is not a classic CT, MR or PET image"),
                    ),
                ));
            };
            target = Some(t);
            first_class = class.clone();
        } else if class != first_class {
            return Err(conversion_error(
                "source",
                i,
                Error::wrap(Kind::InvalidSource, "images of different SOP Classes"),
            ));
        }
        let sop = required_uid(&src.dataset, T_SOP_INSTANCE)
            .map_err(|e| conversion_error("source", i, e))?;
        if !seen.insert(sop.clone()) {
            return Err(conversion_error(
                "source",
                i,
                Error::wrap(Kind::InvalidSource, "duplicate SOP Instance UID"),
            ));
        }
        for (k, (t, name)) in [
            (T_SERIES_UID, "Series"),
            (T_STUDY_UID, "Study"),
            (T_FRAME_OF_REFERENCE, "Frame of Reference"),
        ]
        .into_iter()
        .enumerate()
        {
            let value = clean_text(&src.dataset.get_string(t).unwrap_or_default()).to_string();
            if i == 0 {
                identity[k] = value;
            } else if value != identity[k] {
                return Err(conversion_error(
                    "source",
                    i,
                    Error::wrap(Kind::InvalidSource, format!("images of different {name}")),
                ));
            }
        }
        let cs = src
            .dataset
            .get_string(T_SPECIFIC_CHARSET)
            .unwrap_or_default();
        match &charset {
            None => charset = Some(cs),
            Some(first) if *first != cs => {
                return Err(conversion_error(
                    "source",
                    i,
                    Error::wrap(
                        Kind::InvalidSource,
                        "images with different Specific Character Sets",
                    ),
                ));
            }
            _ => {}
        }
        let (native, _) = transcode::transcode_file(
            src,
            "1.2.840.10008.1.2.1",
            &TranscodeOptions {
                decoder,
                limits: transcode_limits(limits),
                ..TranscodeOptions::default()
            },
        )
        .map_err(|e| conversion_error("transcode", i, transcode_error(e)))?;
        let extracted = crate::pixel::pixeldata::extract_native_frames(&native.dataset)
            .map_err(|e| conversion_error("pixel_data", i, Error::with_text(e.to_string())))?;
        if extracted.data.len() != 1 {
            return Err(conversion_error(
                "source",
                i,
                Error::wrap(
                    Kind::InvalidSource,
                    format!(
                        "a classic image has one frame, not {}",
                        extracted.data.len()
                    ),
                ),
            ));
        }
        let instance = src
            .dataset
            .get_string(T_INSTANCE_NUMBER)
            .unwrap_or_default();
        let instance = parse_decimal_int(clean_text(&instance));
        let pixels = extracted.data.into_iter().next().unwrap_or_default();
        frames.push(Frame {
            dataset: native.dataset,
            pixels,
            class_uid: class,
            sop_uid: sop,
            order: 0.0,
            instance,
        });
    }
    if identity[2].is_empty() {
        return Err(conversion_error(
            "source",
            -1,
            Error::wrap(Kind::Nonconformant, "the images have no Frame of Reference"),
        ));
    }
    match target {
        Some(t) => Ok((frames, t)),
        None => Err(Error::bare(Kind::InvalidSource)),
    }
}
fn parse_decimal_int(s: &str) -> i64 {
    crate::numtext::parse_int(s).unwrap_or(0)
}
fn order_frames(frames: &mut [Frame]) -> &'static str {
    let mut orientation: Vec<f64> = Vec::new();
    let mut by_position = true;
    for (i, f) in frames.iter_mut().enumerate() {
        let iop = f.dataset.get_floats(T_IOP);
        let ipp = f.dataset.get_floats(T_IPP);
        let (Ok(iop), Ok(ipp)) = (iop, ipp) else {
            by_position = false;
            break;
        };
        if iop.len() != 6 || ipp.len() != 3 {
            by_position = false;
            break;
        }
        if i == 0 {
            orientation = iop.clone();
        } else {
            for k in 0..6 {
                if (iop[k] - orientation[k]).abs() > 1e-4 {
                    by_position = false;
                }
            }
        }
        let normal = [
            iop[1] * iop[5] - iop[2] * iop[4],
            iop[2] * iop[3] - iop[0] * iop[5],
            iop[0] * iop[4] - iop[1] * iop[3],
        ];
        f.order = ipp[0] * normal[0] + ipp[1] * normal[1] + ipp[2] * normal[2];
    }
    if by_position {
        frames.sort_by(|a, b| {
            let less = |a: &Frame, b: &Frame| {
                if a.order != b.order {
                    a.order < b.order
                } else {
                    a.instance < b.instance
                }
            };
            if less(a, b) {
                std::cmp::Ordering::Less
            } else if less(b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        return "position";
    }
    frames.sort_by_key(|f| f.instance);
    "instance-number"
}
struct Pixel {
    rows: u16,
    columns: u16,
    bits_allocated: u16,
    bits_stored: u16,
    pixel_representation: u16,
    data: Vec<u8>,
}
fn pixel_module(frames: &[Frame], target: &Target) -> Result<Pixel, Error> {
    let first = &frames[0].dataset;
    let get = |t| uint16_value(first, t).unwrap_or(0);
    let mut p = Pixel {
        rows: get(T_ROWS),
        columns: get(T_COLUMNS),
        bits_allocated: get(T_BITS_ALLOCATED),
        bits_stored: get(T_BITS_STORED),
        pixel_representation: get(T_PIXEL_REPRESENTATION),
        data: Vec::new(),
    };
    for (i, f) in frames.iter().enumerate() {
        for t in [
            T_ROWS,
            T_COLUMNS,
            T_SPP,
            T_BITS_ALLOCATED,
            T_BITS_STORED,
            T_HIGH_BIT,
            T_PIXEL_REPRESENTATION,
        ] {
            let a = uint16_value(first, t).unwrap_or(0);
            match uint16_value(&f.dataset, t) {
                Some(b) if a == b => {}
                _ => {
                    return Err(Error::wrap(
                        Kind::InvalidSource,
                        format!("image {i} differs in {t}"),
                    ));
                }
            }
        }
        let photometric = f.dataset.get_string(T_PHOTOMETRIC).unwrap_or_default();
        if clean_text(&photometric) != "MONOCHROME2" {
            return Err(Error::wrap(
                Kind::UnsupportedSopClass,
                format!(
                    "Photometric Interpretation {} (only MONOCHROME2 is converted)",
                    crate::textfmt::quote(clean_text(&photometric))
                ),
            ));
        }
    }
    if uint16_value(first, T_SPP).unwrap_or(0) != 1 || p.rows == 0 || p.columns == 0 {
        return Err(Error::wrap(
            Kind::Nonconformant,
            "one sample per pixel is required",
        ));
    }
    let mut widen = p.bits_stored;
    match target.modality {
        "CT" => {
            if p.bits_allocated != 16 {
                return Err(Error::wrap(
                    Kind::UnsupportedSopClass,
                    "Enhanced CT needs Bits Allocated 16",
                ));
            }
            if p.bits_stored < 12 {
                widen = 12;
            } else if p.bits_stored > 12 {
                widen = 16;
            }
        }
        "PT" => {
            if p.bits_allocated != 16 {
                return Err(Error::wrap(
                    Kind::UnsupportedSopClass,
                    "Enhanced PET needs Bits Allocated 16",
                ));
            }
            widen = 16;
        }
        _ => {
            if p.bits_allocated != 8 && p.bits_allocated != 16 {
                return Err(Error::wrap(
                    Kind::UnsupportedSopClass,
                    "Enhanced MR needs Bits Allocated 8 or 16",
                ));
            }
        }
    }
    let frame_size =
        usize::from(p.rows) * usize::from(p.columns) * usize::from(p.bits_allocated / 8);
    let mut data = Vec::with_capacity(frame_size * frames.len() + 1);
    for (i, f) in frames.iter().enumerate() {
        if f.pixels.len() < frame_size {
            return Err(Error::wrap(
                Kind::InvalidSource,
                format!(
                    "image {i} has {} bytes of Pixel Data, want {frame_size}",
                    f.pixels.len()
                ),
            ));
        }
        let frame = &f.pixels[..frame_size];
        if p.bits_allocated == 16 {
            data.extend(canonical_samples(
                frame,
                p.bits_stored,
                p.pixel_representation == 1,
            ));
        } else {
            data.extend_from_slice(frame);
        }
    }
    p.data = data;
    p.bits_stored = widen;
    Ok(p)
}
fn canonical_samples(frame: &[u8], bits_stored: u16, signed: bool) -> Vec<u8> {
    let mut out = vec![0u8; frame.len()];
    let shift = 16u32.wrapping_sub(u32::from(bits_stored));
    let mut i = 0;
    while i + 1 < frame.len() {
        let v = u16::from_le_bytes([frame[i], frame[i + 1]]);
        let v = if signed {
            (v.wrapping_shl(shift) as i16).wrapping_shr(shift) as u16
        } else {
            v.wrapping_shl(shift).wrapping_shr(shift)
        };
        out[i..i + 2].copy_from_slice(&v.to_le_bytes());
        i += 2;
    }
    out
}
struct Build<'a> {
    frames: &'a [Frame],
    target: Target,
    top_level: HashMap<Tag, &'static str>,
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    tag: Tag,
    creator: String,
}
#[derive(Clone)]
struct Converted {
    element: Element,
    creator: String,
}
fn us(t: Tag, v: u16) -> Element {
    Element {
        header: ElementHeader::new(t, Vr::US, 2),
        value: Value::Raw(v.to_le_bytes().to_vec()),
    }
}
fn code_item(value: &str, scheme: &str, meaning: &str) -> DataSet {
    data_set(vec![
        str_elem(T_CODE_VALUE, Vr::SH, value),
        str_elem(T_CODING_SCHEME, Vr::SH, scheme),
        str_elem(T_CODE_MEANING, Vr::LO, meaning),
    ])
}
fn sort_elements(mut elements: Vec<Element>) -> Vec<Element> {
    elements.sort_by_key(|e| e.tag());
    elements
}
impl Build<'_> {
    #[allow(clippy::too_many_arguments)]
    fn dataset(
        &self,
        report: &mut LegacyReport,
        pixel: Pixel,
        sop_uid: &str,
        series_uid: &str,
        series_number: &str,
        equipment: &ContributingEquipment,
        now: &ZonedTime,
    ) -> Result<Object, Error> {
        let n = self.frames.len();
        let mut values: Vec<HashMap<Key, (Element, String)>> = Vec::with_capacity(n);
        let mut order: Vec<Key> = Vec::new();
        let mut known: HashSet<Key> = HashSet::new();
        for f in self.frames {
            let mut map = HashMap::new();
            let creators = private_creators_of(&f.dataset);
            for e in f.dataset.elements() {
                let t = e.tag();
                if t.group == 0x0002 || t.element == 0x0000 || ASSIGNED.contains(&t) {
                    continue;
                }
                let mut element = e.clone();
                let mut key = Key {
                    tag: t,
                    creator: String::new(),
                };
                if t.is_private() {
                    if (0x0010..=0x00ff).contains(&t.element) {
                        continue;
                    }
                    let Some(creator) = creators.get(&Tag::new(t.group, t.element >> 8)) else {
                        report.dropped_private_elements += 1;
                        continue;
                    };
                    key = Key {
                        tag: Tag::new(t.group, t.element & 0x00ff),
                        creator: creator.clone(),
                    };
                    element.header.tag = key.tag;
                }
                if known.insert(key.clone()) {
                    order.push(key.clone());
                }
                let value = canonical_value(&f.dataset, e);
                map.insert(key, (repad_text(element), value));
            }
            values.push(map);
        }
        let mut top = Vec::new();
        let mut shared = Vec::new();
        let mut per_frame: Vec<Vec<Converted>> = vec![Vec::new(); n];
        for key in &order {
            let mut any: Option<Element> = None;
            let mut same = true;
            let first_value = values[0].get(key).map(|o| o.1.as_str()).unwrap_or("");
            for (i, map) in values.iter().enumerate() {
                let o = map.get(key);
                if let Some(o) = o
                    && any.is_none()
                {
                    any = Some(o.0.clone());
                }
                if i > 0 && first_value != o.map(|o| o.1.as_str()).unwrap_or("") {
                    same = false;
                }
            }
            let any = any.unwrap_or_else(|| Element {
                header: ElementHeader::new(Tag::new(0, 0), Vr::UN, 0),
                value: Value::Nil,
            });
            let iod_type = self.top_level.get(&key.tag);
            let empty = first_value.is_empty();
            if same
                && iod_type.is_some()
                && key.creator.is_empty()
                && !REPLACED.contains(&key.tag)
                && (!empty || iod_type == Some(&"2"))
            {
                top.push(any);
            } else if same {
                shared.push(Converted {
                    element: any,
                    creator: key.creator.clone(),
                });
            } else {
                for (i, map) in values.iter().enumerate() {
                    if let Some(o) = map.get(key) {
                        per_frame[i].push(Converted {
                            element: o.0.clone(),
                            creator: key.creator.clone(),
                        });
                    }
                }
            }
        }
        report.shared_converted = shared.len() as i64;
        report.per_frame_converted = per_frame.iter().map(|p| p.len() as i64).sum();
        let mut ds = Object::from_elements(top);
        let first = &self.frames[0].dataset;
        if let Some(cs) = first.get(T_SPECIFIC_CHARSET) {
            ds.put(cs.clone());
        }
        ds.put(ui(T_SOP_CLASS, self.target.target));
        ds.put(ui(T_SOP_INSTANCE, sop_uid));
        ds.put(ui(T_SERIES_UID, series_uid));
        ds.put(str_elem(T_MODALITY, Vr::CS, self.target.modality));
        let series_number = if series_number.is_empty() {
            shared_string(self.frames, T_SERIES_NUMBER)
        } else {
            series_number.to_string()
        };
        ds.put(str_elem(T_SERIES_NUMBER, Vr::IS, &series_number));
        ds.put(str_elem(T_INSTANCE_NUMBER, Vr::IS, "1"));
        let (date, time) = earliest_content(self.frames, now);
        ds.put(str_elem(T_CONTENT_DATE, Vr::DA, &date));
        ds.put(str_elem(T_CONTENT_TIME, Vr::TM, &time));
        ds.put(str_elem(T_NUMBER_OF_FRAMES, Vr::IS, &n.to_string()));
        ds.put(us(T_ROWS, pixel.rows));
        ds.put(us(T_COLUMNS, pixel.columns));
        ds.put(us(T_SPP, 1));
        ds.put(str_elem(T_PHOTOMETRIC, Vr::CS, "MONOCHROME2"));
        ds.put(us(T_BITS_ALLOCATED, pixel.bits_allocated));
        ds.put(us(T_BITS_STORED, pixel.bits_stored));
        ds.put(us(T_HIGH_BIT, pixel.bits_stored.wrapping_sub(1)));
        ds.put(us(T_PIXEL_REPRESENTATION, pixel.pixel_representation));
        let vr = if pixel.bits_allocated == 8 {
            Vr::OB
        } else {
            Vr::OW
        };
        let mut data = pixel.data;
        if data.len() % 2 == 1 {
            data.push(0);
        }
        ds.put(Element {
            header: ElementHeader::new(T_PIXEL_DATA, vr, data.len() as u32),
            value: Value::Raw(data),
        });
        let frame_types: Vec<Vec<String>> = self
            .frames
            .iter()
            .map(|f| frame_type_values(&f.dataset))
            .collect();
        ds.put(strings_elem(
            T_IMAGE_TYPE,
            Vr::CS,
            &combined_image_type(&frame_types),
        ));
        for e in self.frame_type_description() {
            if !ds.has(e.tag()) {
                ds.put(e);
            }
        }
        ds.put(str_elem(T_PRESENTATION_LUT_SHAPE, Vr::CS, "IDENTITY"));
        if !ds.has(T_BURNED_IN) {
            ds.put(str_elem(T_BURNED_IN, Vr::CS, "NO"));
        }
        if !ds.has(T_LOSSY) {
            let lossy = if self
                .frames
                .iter()
                .any(|f| clean_text(&f.dataset.get_string(T_LOSSY).unwrap_or_default()) == "01")
            {
                "01"
            } else {
                "00"
            };
            ds.put(str_elem(T_LOSSY, Vr::CS, lossy));
        }
        if self.target.modality == "PT" && !ds.has(T_CONTENT_QUALIFICATION) {
            ds.put(str_elem(T_CONTENT_QUALIFICATION, Vr::CS, "PRODUCT"));
        }
        if !ds.has(T_ACQUISITION_CONTEXT) {
            ds.put(seq(T_ACQUISITION_CONTEXT, Vec::new()));
        }
        if !ds.has(T_POSITION_REFERENCE) {
            ds.put(str_elem(T_POSITION_REFERENCE, Vr::LO, ""));
        }
        let macros = self.functional_groups(&frame_types)?;
        let mut shared_item = Vec::new();
        let mut per_frame_items: Vec<Vec<Element>> = vec![Vec::new(); n];
        for m in macros {
            if m.shared {
                shared_item.extend(m.items.into_iter().take(1));
                continue;
            }
            for (i, item) in m.items.into_iter().enumerate() {
                per_frame_items[i].push(item);
            }
        }
        if !shared.is_empty() {
            shared_item.push(seq(
                T_UNASSIGNED_SHARED,
                vec![data_set(with_private_creators(&shared))],
            ));
        }
        let mut items = Vec::with_capacity(n);
        for (i, f) in self.frames.iter().enumerate() {
            let mut elements = std::mem::take(&mut per_frame_items[i]);
            elements.push(seq(
                T_CONVERSION_SOURCE,
                vec![data_set(vec![
                    ui(T_REF_SOP_CLASS, &f.class_uid),
                    ui(T_REF_SOP_INSTANCE, &f.sop_uid),
                ])],
            ));
            if !per_frame[i].is_empty() {
                elements.push(seq(
                    T_UNASSIGNED_PER_FRAME,
                    vec![data_set(with_private_creators(&per_frame[i]))],
                ));
            }
            items.push(data_set(sort_elements(elements)));
        }
        ds.put(seq(T_SHARED, vec![data_set(sort_elements(shared_item))]));
        ds.put(seq(T_PER_FRAME, items));
        append_contributing_equipment(
            &mut ds,
            equipment,
            now,
            LEGACY_DESCRIPTION,
            "109106",
            "Enhanced Multi-frame Conversion Equipment",
        )?;
        emit_type2(&mut ds, self.target.iod);
        Ok(ds)
    }
    fn frame_type_description(&self) -> Vec<Element> {
        let mut out = vec![
            str_elem(T_PIXEL_PRESENTATION, Vr::CS, "MONOCHROME"),
            str_elem(T_VOLUMETRIC_PROPERTIES, Vr::CS, "VOLUME"),
            str_elem(T_VOLUME_BASED_CALCULATION, Vr::CS, "NONE"),
        ];
        if self.target.modality == "MR" {
            out.push(str_elem(T_COMPLEX_IMAGE_COMPONENT, Vr::CS, "MAGNITUDE"));
            out.push(str_elem(T_ACQUISITION_CONTRAST, Vr::CS, "UNKNOWN"));
        }
        out
    }
    fn functional_groups(&self, frame_types: &[Vec<String>]) -> Result<Vec<Macro>, Error> {
        let n = self.frames.len();
        let mut macros = Vec::new();
        let fg = |e: Error| conversion_error("functional_groups", -1, e);
        let mut add =
            |t: Tag, force_per_frame: bool, build: &MacroBuilder<'_>| -> Result<(), Error> {
                let mut items = Vec::with_capacity(n);
                let mut present = 0;
                for (i, f) in self.frames.iter().enumerate() {
                    match build(i, &f.dataset)? {
                        Some(elements) => {
                            present += 1;
                            items.push(Some(seq(t, vec![data_set(sort_elements(elements))])));
                        }
                        None => items.push(None),
                    }
                }
                if present == 0 {
                    return Ok(());
                }
                if present != n {
                    return Err(Error::wrap(
                        Kind::Nonconformant,
                        format!("{t} is missing for some images"),
                    ));
                }
                let items: Vec<Element> = items.into_iter().flatten().collect();
                if !force_per_frame && items.iter().all(|i| elements_equal(i, &items[0])) {
                    macros.push(Macro {
                        shared: true,
                        items: items.into_iter().take(1).collect(),
                    });
                } else {
                    macros.push(Macro {
                        shared: false,
                        items,
                    });
                }
                Ok(())
            };
        let copy_of = |ds: &Object, tags: &[Tag]| -> Vec<Element> {
            tags.iter()
                .filter_map(|t| ds.get(*t).map(|e| repad_text(e.clone())))
                .collect()
        };
        add(T_PIXEL_MEASURES, false, &|i, ds| {
            if !ds.has(T_PIXEL_SPACING) {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!("image {i} has no Pixel Spacing"),
                ));
            }
            Ok(Some(copy_of(
                ds,
                &[T_PIXEL_SPACING, T_SLICE_THICKNESS, T_SPACING_BETWEEN_SLICES],
            )))
        })
        .map_err(fg)?;
        add(T_PLANE_POSITION, true, &|i, ds| {
            if !ds.has(T_IPP) {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!("image {i} has no Image Position (Patient)"),
                ));
            }
            Ok(Some(copy_of(ds, &[T_IPP])))
        })
        .map_err(fg)?;
        add(T_PLANE_ORIENTATION, false, &|i, ds| {
            if !ds.has(T_IOP) {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!("image {i} has no Image Orientation (Patient)"),
                ));
            }
            Ok(Some(copy_of(ds, &[T_IOP])))
        })
        .map_err(fg)?;
        let voi_required = self.target.frame_voi_required;
        add(T_FRAME_VOI_LUT, false, &|i, ds| {
            let elements = copy_of(
                ds,
                &[
                    T_WINDOW_CENTER,
                    T_WINDOW_WIDTH,
                    T_WINDOW_EXPLANATION,
                    T_VOI_LUT_FUNCTION,
                ],
            );
            if ds.has(T_WINDOW_CENTER) && ds.has(T_WINDOW_WIDTH) {
                return Ok(Some(elements));
            }
            if !voi_required {
                return Ok(None);
            }
            let (center, width) = default_window(&self.frames[i]);
            Ok(Some(vec![
                str_elem(T_WINDOW_CENTER, Vr::DS, &center),
                str_elem(T_WINDOW_WIDTH, Vr::DS, &width),
            ]))
        })
        .map_err(fg)?;
        let modality = self.target.modality;
        let transform_required = self.target.value_transform_required;
        add(T_PIXEL_VALUE_TRANSFORMATION, false, &|_, ds| {
            if !ds.has(T_RESCALE_INTERCEPT) && !transform_required {
                return Ok(None);
            }
            let text = |t| clean_text(&ds.get_string(t).unwrap_or_default()).to_string();
            let mut intercept = text(T_RESCALE_INTERCEPT);
            if intercept.is_empty() {
                intercept = "0".into();
            }
            let mut slope = text(T_RESCALE_SLOPE);
            if slope.is_empty() {
                slope = "1".into();
            }
            let mut rescale_type = text(T_RESCALE_TYPE);
            if rescale_type.is_empty() {
                if modality == "CT" {
                    rescale_type = "HU".into();
                } else if modality == "PT" {
                    rescale_type = text(T_UNITS);
                }
            }
            if rescale_type.is_empty() {
                rescale_type = "US".into();
            }
            Ok(Some(vec![
                str_elem(T_RESCALE_INTERCEPT, Vr::DS, &intercept),
                str_elem(T_RESCALE_SLOPE, Vr::DS, &slope),
                str_elem(T_RESCALE_TYPE, Vr::LO, &rescale_type),
            ]))
        })
        .map_err(fg)?;
        let description = self.frame_type_description();
        add(self.target.frame_type, false, &|i, _| {
            let mut out = vec![strings_elem(T_FRAME_TYPE, Vr::CS, &frame_types[i])];
            out.extend(description.iter().cloned());
            Ok(Some(out))
        })
        .map_err(fg)?;
        add(T_FRAME_CONTENT, true, &|_, ds| {
            let mut out = Vec::new();
            if let Some(v) = ds.get_string(T_ACQUISITION_NUMBER)
                && let Ok(number) = crate::numtext::parse_int(clean_text(&v))
                && number >= 0
            {
                out.push(us(
                    T_FRAME_ACQUISITION_NUMBER,
                    number.min(i64::from(u16::MAX)) as u16,
                ));
            }
            Ok(Some(out))
        })
        .map_err(fg)?;
        add(T_REFERENCED_IMAGE, false, &|_, ds| {
            let Some(e) = ds.get(T_REFERENCED_IMAGE) else {
                return Ok(None);
            };
            Ok(Some(vec![with_purpose(
                e.clone(),
                code_item("121311", "DCM", "Localizer"),
            )]))
        })
        .map_err(fg)?;
        add(T_DERIVATION_IMAGE, false, &|_, ds| {
            let Some(sources) = ds.get(T_SOURCE_IMAGE) else {
                return Ok(None);
            };
            let mut item = vec![with_purpose(
                sources.clone(),
                code_item(
                    "121322",
                    "DCM",
                    "Source image for image processing operation",
                ),
            )];
            match ds.get(T_DERIVATION_CODE) {
                Some(codes) => item.push(codes.clone()),
                None => item.push(seq(
                    T_DERIVATION_CODE,
                    vec![code_item(
                        "113130",
                        "DCM",
                        "Predecessor containing group of imaging subjects",
                    )],
                )),
            }
            if let Some(d) = ds.get(T_DERIVATION_DESCRIPTION) {
                item.push(d.clone());
            }
            Ok(Some(vec![seq(
                T_DERIVATION_IMAGE,
                vec![data_set(sort_elements(item))],
            )]))
        })
        .map_err(fg)?;
        add(T_FRAME_ANATOMY, false, &|_, ds| {
            let region = match ds.get(T_ANATOMIC_REGION) {
                Some(r) => r.clone(),
                None => {
                    let part = ds.get_string(T_BODY_PART).unwrap_or_default();
                    let Some((scheme, value, meaning)) = body_part_region(clean_text(&part)) else {
                        return Ok(None);
                    };
                    seq(T_ANATOMIC_REGION, vec![code_item(value, scheme, meaning)])
                }
            };
            let mut laterality = "U".to_string();
            for t in [T_IMAGE_LATERALITY, T_LATERALITY] {
                if let Some(v) = ds.get_string(t)
                    && !clean_text(&v).is_empty()
                {
                    laterality = clean_text(&v).to_string();
                    break;
                }
            }
            Ok(Some(vec![
                region,
                str_elem(T_FRAME_LATERALITY, Vr::CS, &laterality),
            ]))
        })
        .map_err(fg)?;
        add(T_IRRADIATION_EVENT_ID, false, &|_, ds| {
            Ok(Some(copy_of(ds, &[T_IRRADIATION_EVENT_UID])))
        })
        .map_err(fg)?;
        for m in macros.iter_mut() {
            for item in m.items.iter_mut() {
                *item = flatten_self_nested(item.clone());
            }
        }
        Ok(macros)
    }
}
struct Macro {
    shared: bool,
    items: Vec<Element>,
}
fn elements_equal(a: &Element, b: &Element) -> bool {
    a.tag() == b.tag() && a.vr() == b.vr() && a.value == b.value
}
fn flatten_self_nested(e: Element) -> Element {
    if let Value::Sequence(items) = &e.value
        && items.len() == 1
        && items[0].elements.len() == 1
        && items[0].elements[0].tag() == e.tag()
    {
        return items[0].elements[0].clone();
    }
    e
}
fn frame_type_values(ds: &Object) -> Vec<String> {
    let values = ds.get_strings(T_IMAGE_TYPE).unwrap_or_default();
    let mut out: Vec<String> = ["ORIGINAL", "PRIMARY", "OTHER", "NONE"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for (i, v) in values.iter().take(4).enumerate() {
        let v = clean_text(v);
        if !v.is_empty() {
            out[i] = v.to_string();
        }
    }
    out[1] = "PRIMARY".into();
    out
}
fn combined_image_type(frame_types: &[Vec<String>]) -> Vec<String> {
    let mut out = frame_types[0].clone();
    for values in &frame_types[1..] {
        for i in 0..out.len() {
            if values[i] != out[i] {
                out[i] = "MIXED".into();
            }
        }
    }
    out
}
fn default_window(f: &Frame) -> (String, String) {
    let mut slope = 1.0f64;
    let mut intercept = 0.0f64;
    if let Ok(v) = f.dataset.get_floats(T_RESCALE_SLOPE)
        && v.len() == 1
        && v[0] != 0.0
    {
        slope = v[0];
    }
    if let Ok(v) = f.dataset.get_floats(T_RESCALE_INTERCEPT)
        && v.len() == 1
    {
        intercept = v[0];
    }
    let bits = uint16_value(&f.dataset, T_BITS_ALLOCATED).unwrap_or(0);
    let signed = uint16_value(&f.dataset, T_PIXEL_REPRESENTATION).unwrap_or(0);
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    let step = usize::from(bits / 8);
    if step > 0 {
        let mut i = 0;
        while i + step <= f.pixels.len() {
            let v = if step == 1 {
                f64::from(f.pixels[i])
            } else if signed == 1 {
                f64::from(i16::from_le_bytes([f.pixels[i], f.pixels[i + 1]]))
            } else {
                f64::from(u16::from_le_bytes([f.pixels[i], f.pixels[i + 1]]))
            };
            low = crate::fmath::min(low, v);
            high = crate::fmath::max(high, v);
            i += step;
        }
    }
    low = low * slope + intercept;
    high = high * slope + intercept;
    if low > high {
        std::mem::swap(&mut low, &mut high);
    }
    let width = crate::fmath::max(high - low, 1.0);
    let center = crate::object::element_setters::format_ds(low + width / 2.0);
    let width_ds = crate::object::element_setters::format_ds(width);
    match (center, width_ds) {
        (Ok(c), Ok(w)) => (c, w),
        _ => ("0".into(), "1".into()),
    }
}
fn with_purpose(mut e: Element, purpose: DataSet) -> Element {
    let Value::Sequence(items) = &e.value else {
        return e;
    };
    let items = items
        .iter()
        .map(|item| {
            let mut elements = item.elements.clone();
            if !elements.iter().any(|e| e.tag() == T_PURPOSE_OF_REFERENCE) {
                elements.push(seq(T_PURPOSE_OF_REFERENCE, vec![purpose.clone()]));
            }
            data_set(sort_elements(elements))
        })
        .collect();
    e.value = Value::Sequence(items);
    e.header.length = 0;
    e.header.length_set = false;
    e
}
fn private_creators_of(ds: &Object) -> HashMap<Tag, String> {
    let mut out = HashMap::new();
    for e in ds.elements() {
        let t = e.tag();
        if t.is_private()
            && (0x0010..=0x00ff).contains(&t.element)
            && let Some(v) = ds.get_string(t)
            && !clean_text(&v).is_empty()
        {
            out.insert(t, v.trim_end_matches([' ', '\0']).to_string());
        }
    }
    out
}
fn with_private_creators(attributes: &[Converted]) -> Vec<Element> {
    let mut out = Vec::new();
    let mut blocks: HashMap<u16, Vec<String>> = HashMap::new();
    for a in attributes {
        let mut e = a.element.clone();
        if a.creator.is_empty() {
            out.push(e);
            continue;
        }
        let group = e.tag().group;
        let creators = blocks.entry(group).or_default();
        let block = match creators.iter().position(|c| *c == a.creator) {
            Some(p) => 0x10 + p as u16,
            None => {
                let b = 0x10 + creators.len() as u16;
                creators.push(a.creator.clone());
                out.push(str_elem(Tag::new(group, b), Vr::LO, &a.creator));
                b
            }
        };
        e.header.tag = Tag::new(group, block << 8 | e.tag().element & 0x00ff);
        out.push(e);
    }
    sort_elements(out)
}
fn canonical_value(ds: &Object, e: &Element) -> String {
    if e.vr() == Vr::SQ {
        let Some(items) = ds.get_sequence(e.tag()) else {
            return String::new();
        };
        let mut out = Vec::new();
        for item in items {
            out.extend_from_slice(b"\x00item\x00");
            if let Ok(bytes) = crate::object::writer::write_data_set(&item, "1.2.840.10008.1.2.1") {
                out.extend(bytes);
            }
        }
        return String::from_utf8_lossy(&out).into_owned();
    }
    if e.vr().is_string_like()
        && let Ok(values) = ds.lookup_strings(e.tag())
    {
        let values: Vec<&str> = values.iter().map(|v| clean_text(v)).collect();
        let joined = values.join("\\");
        if joined.trim_matches('\\').is_empty() {
            return String::new();
        }
        return joined;
    }
    if let Some(raw) = e.raw() {
        return String::from_utf8_lossy(raw).into_owned();
    }
    format!("{:?}", e.value)
}
fn shared_string(frames: &[Frame], t: Tag) -> String {
    let first = clean_text(&frames[0].dataset.get_string(t).unwrap_or_default()).to_string();
    for f in &frames[1..] {
        if clean_text(&f.dataset.get_string(t).unwrap_or_default()) != first {
            return String::new();
        }
    }
    first
}
fn repad_text(e: Element) -> Element {
    let Value::Raw(raw) = &e.value else {
        return e;
    };
    if !e.vr().is_string_like() || e.vr() == Vr::UI || raw.last() != Some(&0) {
        return e;
    }
    let mut trimmed = raw.clone();
    while trimmed.last().is_some_and(|b| *b == 0 || *b == b' ') {
        trimmed.pop();
    }
    if trimmed.len() % 2 == 1 {
        trimmed.push(b' ');
    }
    Element {
        header: ElementHeader::new(e.tag(), e.vr(), trimmed.len() as u32),
        value: Value::Raw(trimmed),
    }
}
fn earliest_content(frames: &[Frame], now: &ZonedTime) -> (String, String) {
    let mut best = String::new();
    for f in frames {
        let date =
            clean_text(&f.dataset.get_string(T_CONTENT_DATE).unwrap_or_default()).to_string();
        let clock =
            clean_text(&f.dataset.get_string(T_CONTENT_TIME).unwrap_or_default()).to_string();
        if date.is_empty() || clock.is_empty() {
            return (
                format!("{:04}{:02}{:02}", now.year(), now.month(), now.day()),
                format!("{:02}{:02}{:02}", now.hour(), now.minute(), now.second()),
            );
        }
        let candidate = format!("{date} {clock}");
        if best.is_empty() || candidate < best {
            best = candidate;
        }
    }
    let (date, time) = best.split_once(' ').unwrap_or((best.as_str(), ""));
    (date.to_string(), time.to_string())
}
fn iod_top_level_types(iod: &str) -> Option<HashMap<Tag, &'static str>> {
    let modules = iodmodules::modules(iod)?;
    let mut types: HashMap<Tag, &'static str> = HashMap::new();
    for usage in modules {
        for a in iodmodules::attributes(usage.module).unwrap_or_default() {
            let t = Tag::new((a.tag >> 16) as u16, a.tag as u16);
            match types.get(&t) {
                Some(previous) if !(a.typ == "2" && *previous != "2") => {}
                _ => {
                    types.insert(t, a.typ);
                }
            }
        }
    }
    Some(types)
}
type MacroBuilder<'a> = dyn Fn(usize, &Object) -> Result<Option<Vec<Element>>, Error> + 'a;
