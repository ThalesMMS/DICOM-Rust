use super::{
    CT_IMAGE_STORAGE_UID, ENHANCED_CT_IMAGE_STORAGE_UID, ENHANCED_MR_IMAGE_STORAGE_UID,
    ENHANCED_PET_IMAGE_STORAGE_UID, Error, Kind, LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID,
    LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID,
    LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID, Limits, MR_IMAGE_STORAGE_UID,
    PET_IMAGE_STORAGE_UID, normalize_limits, uint16_value,
};
use crate::dcmtime::{self, ZonedTime};
use crate::derivation::{self, Derivation, Source};
use crate::dictionary::reservations::{
    PrivateReservations, is_private_creator_tag, is_private_data_tag, reserve_private_creator,
};
use crate::object::{
    DataSet, Element, ElementHeader, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use crate::pixel::transcode::{self, TranscodeLimits, TranscodeOptions};
use crate::validation::iod::{self as iodrules, modules as iodmodules};
const CONVERSION_DESCRIPTION: &str = "Classic Image created from Enhanced Image";
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
pub(crate) const T_SOP_CLASS: Tag = tag(0x0008, 0x0016);
pub(crate) const T_SOP_INSTANCE: Tag = tag(0x0008, 0x0018);
const T_MODALITY: Tag = tag(0x0008, 0x0060);
pub(crate) const T_IMAGE_TYPE: Tag = tag(0x0008, 0x0008);
const T_FRAME_TYPE: Tag = tag(0x0008, 0x9007);
const T_MANUFACTURER: Tag = tag(0x0008, 0x0070);
const T_INSTITUTION: Tag = tag(0x0008, 0x0080);
const T_CODE_VALUE: Tag = tag(0x0008, 0x0100);
const T_CODING_SCHEME: Tag = tag(0x0008, 0x0102);
const T_CODE_MEANING: Tag = tag(0x0008, 0x0104);
const T_STATION: Tag = tag(0x0008, 0x1010);
const T_MODEL: Tag = tag(0x0008, 0x1090);
const T_REF_SOP_CLASS: Tag = tag(0x0008, 0x1150);
const T_REF_SOP_INSTANCE: Tag = tag(0x0008, 0x1155);
const T_REF_FRAME: Tag = tag(0x0008, 0x1160);
const T_DERIVATION_DESCRIPTION: Tag = tag(0x0008, 0x2111);
const T_SERIAL: Tag = tag(0x0018, 0x1000);
const T_SOFTWARE: Tag = tag(0x0018, 0x1020);
const T_CONTRIBUTING_EQUIPMENT: Tag = tag(0x0018, 0xA001);
const T_CONTRIBUTION_DATETIME: Tag = tag(0x0018, 0xA002);
const T_CONTRIBUTION_DESCRIPTION: Tag = tag(0x0018, 0xA003);
const T_STUDY_UID: Tag = tag(0x0020, 0x000D);
const T_SERIES_UID: Tag = tag(0x0020, 0x000E);
const T_INSTANCE_NUMBER: Tag = tag(0x0020, 0x0013);
const T_FRAME_OF_REFERENCE: Tag = tag(0x0020, 0x0052);
const T_IPP: Tag = tag(0x0020, 0x0032);
const T_IOP: Tag = tag(0x0020, 0x0037);
const T_SPP: Tag = tag(0x0028, 0x0002);
const T_PHOTOMETRIC: Tag = tag(0x0028, 0x0004);
const T_NUMBER_OF_FRAMES: Tag = tag(0x0028, 0x0008);
const T_FRAME_INCREMENT_POINTER: Tag = tag(0x0028, 0x0009);
const T_ROWS: Tag = tag(0x0028, 0x0010);
const T_COLUMNS: Tag = tag(0x0028, 0x0011);
const T_PIXEL_SPACING: Tag = tag(0x0028, 0x0030);
const T_BITS_ALLOCATED: Tag = tag(0x0028, 0x0100);
const T_BITS_STORED: Tag = tag(0x0028, 0x0101);
const T_HIGH_BIT: Tag = tag(0x0028, 0x0102);
const T_PIXEL_REPRESENTATION: Tag = tag(0x0028, 0x0103);
const T_REPRESENTATIVE_FRAME: Tag = tag(0x0028, 0x6010);
const T_STEREO_PAIRS: Tag = tag(0x0022, 0x0028);
const T_PURPOSE_OF_REFERENCE: Tag = tag(0x0040, 0xA170);
const T_CONVERSION_SOURCE: Tag = tag(0x0020, 0x9172);
const T_DIMENSION_ORGANIZATION: Tag = tag(0x0020, 0x9221);
const T_DIMENSION_INDEX: Tag = tag(0x0020, 0x9222);
const T_DIMENSION_ORGANIZATION_TYPE: Tag = tag(0x0020, 0x9311);
const T_CONCATENATION_UID: Tag = tag(0x0020, 0x9161);
const T_IN_CONCATENATION_NUMBER: Tag = tag(0x0020, 0x9162);
const T_IN_CONCATENATION_TOTAL: Tag = tag(0x0020, 0x9163);
const T_CONCATENATION_OFFSET: Tag = tag(0x0020, 0x9228);
const T_CONCATENATION_SOURCE: Tag = tag(0x0020, 0x0242);
const T_SHARED: Tag = tag(0x5200, 0x9229);
const T_PER_FRAME: Tag = tag(0x5200, 0x9230);
const T_PIXEL_DATA: Tag = tag(0x7FE0, 0x0010);
const T_FLOAT_PIXEL_DATA: Tag = tag(0x7FE0, 0x0008);
const T_DOUBLE_PIXEL_DATA: Tag = tag(0x7FE0, 0x0009);
const T_SCANNING_SEQUENCE: Tag = tag(0x0018, 0x0020);
const T_SEQUENCE_VARIANT: Tag = tag(0x0018, 0x0021);
const T_SCAN_OPTIONS: Tag = tag(0x0018, 0x0022);
const T_MR_ACQUISITION_TYPE: Tag = tag(0x0018, 0x0023);
const T_ECHO_TIME: Tag = tag(0x0018, 0x0081);
const T_ECHO_TRAIN_LENGTH: Tag = tag(0x0018, 0x0091);
const T_KVP: Tag = tag(0x0018, 0x0060);
const T_ACQUISITION_NUMBER: Tag = tag(0x0020, 0x0012);
const T_RESCALE_INTERCEPT: Tag = tag(0x0028, 0x1052);
const T_RESCALE_SLOPE: Tag = tag(0x0028, 0x1053);
const T_PIXEL_MEASURES: Tag = tag(0x0028, 0x9110);
const T_PLANE_POSITION: Tag = tag(0x0020, 0x9113);
const T_PLANE_ORIENTATION: Tag = tag(0x0020, 0x9116);
const T_PIXEL_VALUE_TRANSFORMATION: Tag = tag(0x0028, 0x9145);
const T_CT_FRAME_TYPE: Tag = tag(0x0018, 0x9329);
const T_MR_FRAME_TYPE: Tag = tag(0x0018, 0x9226);
const T_PET_FRAME_TYPE: Tag = tag(0x0018, 0x9751);
const T_UNASSIGNED_SHARED: Tag = tag(0x0020, 0x9170);
const T_UNASSIGNED_PER_FRAME: Tag = tag(0x0020, 0x9171);
const T_FRAME_CONTENT: Tag = tag(0x0020, 0x9111);
const T_SERIES_DATE: Tag = tag(0x0008, 0x0021);
const T_SERIES_TIME: Tag = tag(0x0008, 0x0031);
const T_CONTENT_DATE: Tag = tag(0x0008, 0x0023);
const T_ACQUISITION_DATETIME: Tag = tag(0x0008, 0x002A);
const T_FRAME_REFERENCE_DATETIME: Tag = tag(0x0018, 0x9151);
const T_FRAME_ACQUISITION_DATETIME: Tag = tag(0x0018, 0x9074);
const T_FRAME_ACQUISITION_DURATION: Tag = tag(0x0018, 0x9220);
const T_ACTUAL_FRAME_DURATION: Tag = tag(0x0018, 0x1242);
const T_DIMENSION_INDEX_VALUES: Tag = tag(0x0020, 0x9157);
const T_TEMPORAL_POSITION_INDEX: Tag = tag(0x0020, 0x9128);
const T_STACK_ID: Tag = tag(0x0020, 0x9056);
const T_IN_STACK_POSITION: Tag = tag(0x0020, 0x9057);
const T_ACQUISITION_DURATION: Tag = tag(0x0018, 0x9073);
const T_CORRECTED_IMAGE: Tag = tag(0x0028, 0x0051);
const T_SERIES_TYPE: Tag = tag(0x0054, 0x1000);
const T_UNITS: Tag = tag(0x0054, 0x1001);
const T_COUNTS_SOURCE: Tag = tag(0x0054, 0x1002);
const T_DECAY_CORRECTION: Tag = tag(0x0054, 0x1102);
const T_DECAY_CORRECTED: Tag = tag(0x0018, 0x9758);
const T_DECAY_CORRECTION_DATETIME: Tag = tag(0x0018, 0x9701);
const T_NUMBER_OF_SLICES: Tag = tag(0x0054, 0x0081);
const T_NUMBER_OF_TIME_SLICES: Tag = tag(0x0054, 0x0101);
const T_FRAME_REFERENCE_TIME: Tag = tag(0x0054, 0x1300);
const T_IMAGE_INDEX: Tag = tag(0x0054, 0x1330);
const T_DECAY_FACTOR: Tag = tag(0x0054, 0x1321);
const T_CARDIAC_SYNC: Tag = tag(0x0018, 0x9118);
const T_RESPIRATORY_SYNC: Tag = tag(0x0020, 0x9253);
const T_PIXEL_PRESENTATION: Tag = tag(0x0008, 0x9205);
const T_VOLUMETRIC_PROPERTIES: Tag = tag(0x0008, 0x9206);
const T_VOLUME_BASED_CALCULATION: Tag = tag(0x0008, 0x9207);
const T_COMPLEX_IMAGE_COMPONENT: Tag = tag(0x0008, 0x9208);
const T_ACQUISITION_CONTRAST: Tag = tag(0x0008, 0x9209);
const T_FRAME_LATERALITY: Tag = tag(0x0020, 0x9072);
const T_FRAME_ACQUISITION_NUMBER: Tag = tag(0x0020, 0x9156);
const T_CONTENT_QUALIFICATION: Tag = tag(0x0018, 0x9004);
const T_PRESENTATION_LUT_SHAPE: Tag = tag(0x2050, 0x0020);
const T_ACQUISITION_CONTEXT: Tag = tag(0x0040, 0x0555);
const T_RESCALE_TYPE: Tag = tag(0x0028, 0x1054);
const MULTI_FRAME_ONLY: [Tag; 18] = [
    T_NUMBER_OF_FRAMES,
    T_FRAME_INCREMENT_POINTER,
    T_REPRESENTATIVE_FRAME,
    T_STEREO_PAIRS,
    T_SHARED,
    T_PER_FRAME,
    T_CONVERSION_SOURCE,
    T_DIMENSION_ORGANIZATION,
    T_DIMENSION_INDEX,
    T_DIMENSION_ORGANIZATION_TYPE,
    T_CONCATENATION_UID,
    T_IN_CONCATENATION_NUMBER,
    T_IN_CONCATENATION_TOTAL,
    T_CONCATENATION_OFFSET,
    T_CONCATENATION_SOURCE,
    T_PIXEL_DATA,
    T_FLOAT_PIXEL_DATA,
    T_DOUBLE_PIXEL_DATA,
];
const ENHANCED_ONLY: [Tag; 19] = [
    T_PIXEL_PRESENTATION,
    T_VOLUMETRIC_PROPERTIES,
    T_VOLUME_BASED_CALCULATION,
    T_COMPLEX_IMAGE_COMPONENT,
    T_ACQUISITION_CONTRAST,
    T_FRAME_LATERALITY,
    T_FRAME_ACQUISITION_NUMBER,
    T_FRAME_REFERENCE_DATETIME,
    T_FRAME_ACQUISITION_DATETIME,
    T_FRAME_ACQUISITION_DURATION,
    T_DIMENSION_INDEX_VALUES,
    T_TEMPORAL_POSITION_INDEX,
    T_STACK_ID,
    T_IN_STACK_POSITION,
    T_CONTENT_QUALIFICATION,
    T_PRESENTATION_LUT_SHAPE,
    T_ACQUISITION_CONTEXT,
    T_ACQUISITION_DURATION,
    T_RESCALE_TYPE,
];
const PET_CORRECTIONS: [(Tag, &str); 11] = [
    (tag(0x0018, 0x9758), "DECY"),
    (tag(0x0018, 0x9759), "ATTN"),
    (tag(0x0018, 0x9760), "SCAT"),
    (tag(0x0018, 0x9761), "DTIM"),
    (tag(0x0018, 0x9762), "MOTN"),
    (tag(0x0018, 0x9763), "PMOT"),
    (tag(0x0018, 0x9764), "CLN"),
    (tag(0x0018, 0x9765), "RAN"),
    (tag(0x0018, 0x9766), "RADL"),
    (tag(0x0018, 0x9767), "DCAL"),
    (tag(0x0018, 0x9768), "NORM"),
];
const PET_UNITS: [&str; 19] = [
    "CNTS",
    "NONE",
    "CM2",
    "CM2ML",
    "PCNT",
    "CPS",
    "BQML",
    "MGMINML",
    "UMOLMINML",
    "MLMING",
    "MLG",
    "1CM",
    "UMOLML",
    "PROPCNTS",
    "PROPCPS",
    "MLMINML",
    "MLML",
    "GML",
    "STDDEV",
];
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributingEquipment {
    pub manufacturer: String,
    pub institution_name: String,
    pub station_name: String,
    pub manufacturer_model_name: String,
    pub device_serial_number: String,
    pub software_versions: Vec<String>,
}
pub type UidForFrame<'a> = &'a dyn Fn(i64) -> Result<String, Error>;
pub struct ClassicOptions<'a> {
    pub frame_indices: Option<Vec<i64>>,
    pub series_instance_uid: String,
    pub sop_instance_uid_for_frame: Option<UidForFrame<'a>>,
    pub decoder: Option<transcode::DecoderRegistry>,
    pub limits: Limits,
    pub equipment: ContributingEquipment,
    pub now: Option<ZonedTime>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassicReport {
    pub source_frames: i64,
    pub converted_frames: i64,
    pub native_bytes: i64,
    pub output_bytes: i64,
    pub target_sop_class_uid: String,
    pub transfer_syntax_uid: String,
}
pub(crate) fn conversion_error(stage: &str, frame: i64, err: Error) -> Error {
    let text = if frame >= 0 {
        format!("dicom/multiframe: conversion failed during {stage} for frame {frame}")
    } else {
        format!("dicom/multiframe: conversion failed during {stage}")
    };
    Error::conversion(text, err)
}
pub(crate) fn transcode_error(err: transcode::Error) -> Error {
    let inner = Error::with_kinds(err.to_string(), Vec::new());
    if err.is_resource_limit() {
        return Error::join(vec![Error::bare(Kind::ResourceLimit), inner]);
    }
    inner
}
pub(crate) fn transcode_limits(l: &Limits) -> TranscodeLimits {
    TranscodeLimits {
        max_frames: l.max_frames,
        max_pixels: l.max_pixels,
        max_input_bytes: l.max_input_bytes,
        max_output_bytes: l.max_output_bytes,
        max_fragments: l.max_fragments,
        max_elements: l.max_elements,
        max_depth: l.max_depth,
        max_expansion_ratio: l.max_expansion_ratio,
        max_duration_nanos: l.max_duration,
    }
}
pub(crate) fn clean_text(v: &str) -> &str {
    v.trim_matches([' ', '\0'])
}
pub(crate) fn str_elem(tag: Tag, vr: Vr, value: &str) -> Element {
    crate::derivedio::str_elem(tag, vr, value)
}
pub(crate) fn strings_elem(tag: Tag, vr: Vr, values: &[String]) -> Element {
    crate::derivedio::strings(tag, vr, values)
}
pub(crate) fn ui(tag: Tag, value: &str) -> Element {
    str_elem(tag, Vr::UI, value)
}
pub(crate) fn seq(tag: Tag, items: Vec<DataSet>) -> Element {
    crate::derivedio::seq(tag, items)
}
pub(crate) fn data_set(elements: Vec<Element>) -> DataSet {
    DataSet {
        elements,
        item_offset: None,
    }
}
fn code_item(value: &str, scheme: &str, meaning: &str) -> DataSet {
    data_set(vec![
        str_elem(T_CODE_VALUE, Vr::SH, value),
        str_elem(T_CODING_SCHEME, Vr::SH, scheme),
        str_elem(T_CODE_MEANING, Vr::LO, meaning),
    ])
}
pub(crate) fn required_uid(obj: &Object, t: Tag) -> Result<String, Error> {
    let value = obj.get_string(t);
    let v = crate::siemens::trim_space(value.as_deref().unwrap_or("")).to_string();
    if value.is_none() || !crate::object::is_valid_uid(v.as_bytes()) {
        return Err(Error::wrap(
            Kind::Nonconformant,
            format!("required UID {t} is missing or invalid"),
        ));
    }
    Ok(v)
}
fn valid_uid(v: &str) -> bool {
    crate::object::is_valid_uid(v.as_bytes())
}
pub(crate) struct Budget {
    pub max_elements: i64,
    pub max_depth: i64,
    pub elements: i64,
}
impl Budget {
    pub(crate) fn clone_element(&mut self, e: &Element, depth: i64) -> Result<Element, Error> {
        if depth > self.max_depth || self.elements >= self.max_elements {
            return Err(Error::wrap(
                Kind::ResourceLimit,
                "elements or sequence depth",
            ));
        }
        self.elements += 1;
        let value = match &e.value {
            Value::Nil => {
                return Err(Error::wrap(Kind::InvalidSource, "deferred element value"));
            }
            Value::Sequence(items) => {
                if depth >= self.max_depth {
                    return Err(Error::wrap(Kind::ResourceLimit, "sequence depth"));
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let mut elements = Vec::with_capacity(item.elements.len());
                    for e in &item.elements {
                        elements.push(self.clone_element(e, depth + 1)?);
                    }
                    out.push(DataSet {
                        elements,
                        item_offset: item.item_offset,
                    });
                }
                Value::Sequence(out)
            }
            other => other.clone(),
        };
        Ok(Element {
            header: e.header,
            value,
        })
    }
}
fn elements_equal(a: &Element, b: &Element) -> bool {
    a.tag() == b.tag() && a.vr() == b.vr() && a.value == b.value
}
pub(crate) fn validate_equipment(e: &ContributingEquipment) -> Result<(), Error> {
    let fields = [
        ("Manufacturer", &e.manufacturer, 64, true),
        ("InstitutionName", &e.institution_name, 64, false),
        ("StationName", &e.station_name, 16, false),
        (
            "ManufacturerModelName",
            &e.manufacturer_model_name,
            64,
            false,
        ),
        ("DeviceSerialNumber", &e.device_serial_number, 64, false),
    ];
    for (name, value, max, required) in fields {
        let v = crate::siemens::trim_space(value);
        if required && v.is_empty() {
            return Err(Error::wrap(
                Kind::InvalidOptions,
                format!("Equipment.{name} is required"),
            ));
        }
        if v.len() > max || v.contains('\\') {
            return Err(Error::wrap(
                Kind::InvalidOptions,
                format!("Equipment.{name} is not a valid single DICOM value"),
            ));
        }
    }
    for version in &e.software_versions {
        let v = crate::siemens::trim_space(version);
        if v.len() > 64 || v.contains('\\') {
            return Err(Error::wrap(
                Kind::InvalidOptions,
                "Equipment.SoftwareVersions contains an invalid DICOM value",
            ));
        }
    }
    Ok(())
}
fn conversion_class(obj: &Object) -> Result<(String, &'static str, &'static str), Error> {
    let source = required_uid(obj, T_SOP_CLASS)?;
    let (target, modality) = match source.as_str() {
        ENHANCED_CT_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID => {
            (CT_IMAGE_STORAGE_UID, "CT")
        }
        ENHANCED_MR_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID => {
            (MR_IMAGE_STORAGE_UID, "MR")
        }
        ENHANCED_PET_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID => {
            (PET_IMAGE_STORAGE_UID, "PT")
        }
        _ => return Err(Error::bare(Kind::UnsupportedSopClass)),
    };
    Ok((source, target, modality))
}
fn selected_frames(indices: &Option<Vec<i64>>, frames: i64, max: i64) -> Result<Vec<i64>, Error> {
    let Some(indices) = indices else {
        if frames > max {
            return Err(Error::wrap(
                Kind::ResourceLimit,
                "frame count exceeds limit",
            ));
        }
        return Ok((0..frames).collect());
    };
    if indices.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidOptions,
            "FrameIndices must not be empty",
        ));
    }
    if indices.len() as i64 > max {
        return Err(Error::wrap(
            Kind::ResourceLimit,
            "selected frame count exceeds limit",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for &i in indices {
        if i < 0 || i >= frames {
            return Err(Error::wrap(
                Kind::InvalidOptions,
                "frame index is out of range",
            ));
        }
        if !seen.insert(i) {
            return Err(Error::wrap(Kind::InvalidOptions, "duplicate frame index"));
        }
    }
    Ok(indices.clone())
}
pub fn convert_to_classic(
    src: &File,
    opts: &ClassicOptions<'_>,
) -> Result<(Vec<File>, ClassicReport), Error> {
    let limits = normalize_limits(opts.limits).map_err(|e| conversion_error("options", -1, e))?;
    let source_err = |e| conversion_error("source", -1, e);
    let options_err = |e| conversion_error("options", -1, e);
    let (source_class, target_class, modality) =
        conversion_class(&src.dataset).map_err(source_err)?;
    let source_sop = required_uid(&src.dataset, T_SOP_INSTANCE).map_err(source_err)?;
    let source_series = required_uid(&src.dataset, T_SERIES_UID).map_err(source_err)?;
    let series = crate::siemens::trim_space(&opts.series_instance_uid).to_string();
    if !valid_uid(&series) || series == source_series {
        return Err(options_err(Error::wrap(
            Kind::InvalidOptions,
            "SeriesInstanceUID must be valid and differ from the source",
        )));
    }
    let Some(uid_for_frame) = opts.sop_instance_uid_for_frame else {
        return Err(options_err(Error::wrap(
            Kind::InvalidOptions,
            "SOPInstanceUIDForFrame is required",
        )));
    };
    validate_equipment(&opts.equipment).map_err(options_err)?;
    let (transcoded, transcode_report) = transcode::transcode_file(
        src,
        "1.2.840.10008.1.2.1",
        &TranscodeOptions {
            decoder: opts.decoder,
            limits: transcode_limits(&limits),
            ..TranscodeOptions::default()
        },
    )
    .map_err(|e| conversion_error("transcode", -1, transcode_error(e)))?;
    let frames =
        crate::pixel::pixeldata::extract_native_frames(&transcoded.dataset).map_err(|e| {
            conversion_error(
                "pixel_data",
                -1,
                Error::with_kinds(e.to_string(), Vec::new()),
            )
        })?;
    let frame_count = frames.metadata.number_of_frames;
    if frame_count < 1 {
        return Err(source_err(Error::wrap(
            Kind::Nonconformant,
            "NumberOfFrames must be positive",
        )));
    }
    let selected = selected_frames(&opts.frame_indices, frame_count, limits.max_frames)
        .map_err(|e| conversion_error("selection", -1, e))?;
    let (shared, per_frame) = functional_groups(&transcoded.dataset, frame_count)
        .map_err(|e| conversion_error("functional_groups", -1, e))?;
    let mut uids = std::collections::HashSet::new();
    let mut frame_uids = Vec::with_capacity(selected.len());
    for &frame in &selected {
        let uid = uid_for_frame(frame).map_err(|e| conversion_error("uid", frame, e))?;
        let uid = crate::siemens::trim_space(&uid).to_string();
        if !valid_uid(&uid) || uid == source_sop {
            return Err(conversion_error(
                "uid",
                frame,
                Error::wrap(
                    Kind::InvalidOptions,
                    "generated SOP Instance UID is invalid or equals the source",
                ),
            ));
        }
        if !uids.insert(uid.clone()) {
            return Err(conversion_error(
                "uid",
                frame,
                Error::wrap(Kind::InvalidOptions, "duplicate generated SOP Instance UID"),
            ));
        }
        frame_uids.push(uid);
    }
    let now = opts.now.unwrap_or_else(now_local);
    let mut budget = Budget {
        max_elements: limits.max_elements,
        max_depth: limits.max_depth,
        elements: 0,
    };
    let mut outputs = Vec::with_capacity(selected.len());
    let mut output_bytes = 0i64;
    let ctx = FrameContext {
        source_class: &source_class,
        source_sop: &source_sop,
        target_class,
        modality,
        series: &series,
        equipment: &opts.equipment,
        now,
    };
    for (i, &frame) in selected.iter().enumerate() {
        let dataset = build_frame_dataset(
            &mut budget,
            &transcoded.dataset,
            &shared,
            &per_frame[frame as usize],
            frame,
            &frames.data[frame as usize],
            &frame_uids[i],
            &ctx,
        )
        .map_err(|e| conversion_error("frame", frame, e))?;
        let mut file = File {
            preamble: Vec::new(),
            meta: Object::new(),
            dataset,
            transfer_syntax: crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            partial_read: None,
            incomplete_pixel_data: None,
        };
        file.rebuild_file_meta().map_err(|e| {
            conversion_error(
                "file_meta",
                frame,
                Error::with_kinds(e.to_string(), Vec::new()),
            )
        })?;
        let written = crate::object::writer::write_file(&file).map_err(|e| {
            conversion_error(
                "write_validation",
                frame,
                Error::with_kinds(e.to_string(), Vec::new()),
            )
        })?;
        if written.len() as i64 > limits.max_output_bytes - output_bytes {
            return Err(conversion_error(
                "output",
                frame,
                Error::wrap(Kind::ResourceLimit, "aggregate output bytes"),
            ));
        }
        output_bytes += written.len() as i64;
        outputs.push(file);
    }
    let report = ClassicReport {
        source_frames: frame_count,
        converted_frames: outputs.len() as i64,
        native_bytes: transcode_report.native_bytes,
        output_bytes,
        target_sop_class_uid: target_class.to_string(),
        transfer_syntax_uid: "1.2.840.10008.1.2.1".to_string(),
    };
    Ok((outputs, report))
}
pub(crate) fn now_local() -> ZonedTime {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ZonedTime::from_fields(1970, 1, 1, 0, 0, unix, 0, 0)
}
struct FrameContext<'a> {
    source_class: &'a str,
    source_sop: &'a str,
    target_class: &'static str,
    modality: &'static str,
    series: &'a str,
    equipment: &'a ContributingEquipment,
    now: ZonedTime,
}
fn functional_groups(obj: &Object, frames: i64) -> Result<(Object, Vec<Object>), Error> {
    let shared = obj.get_sequence(T_SHARED).unwrap_or_default();
    if !obj.has(T_SHARED) || shared.len() != 1 {
        return Err(Error::wrap(
            Kind::Nonconformant,
            "SharedFunctionalGroupsSequence must contain exactly one item",
        ));
    }
    let per_frame = obj.get_sequence(T_PER_FRAME);
    let Some(per_frame) = per_frame.filter(|p| p.len() as i64 == frames) else {
        return Err(Error::wrap(
            Kind::Nonconformant,
            "PerFrameFunctionalGroupsSequence item count must equal NumberOfFrames",
        ));
    };
    if let Some(c) = obj.functional_group_conflicts().first() {
        return Err(Error::wrap(
            Kind::Nonconformant,
            format!(
                "functional group macro {} appears in Shared and PerFrame (frame {})",
                c.macro_tag,
                c.frames.first().copied().unwrap_or(0) + 1
            ),
        ));
    }
    let shared = shared.into_iter().next().unwrap_or_default();
    Ok((shared, per_frame))
}
fn validate_required_groups(source: &Object, frame: i64, target: &str) -> Result<(), Error> {
    let mut required = vec![T_PIXEL_MEASURES, T_PLANE_POSITION, T_PLANE_ORIENTATION];
    match target {
        CT_IMAGE_STORAGE_UID => required.extend([T_CT_FRAME_TYPE, T_PIXEL_VALUE_TRANSFORMATION]),
        MR_IMAGE_STORAGE_UID => required.push(T_MR_FRAME_TYPE),
        PET_IMAGE_STORAGE_UID => required.extend([T_PET_FRAME_TYPE, T_PIXEL_VALUE_TRANSFORMATION]),
        _ => {}
    }
    for t in required {
        if source.functional_group_item(frame, t).0.is_none() {
            return Err(Error::wrap(
                Kind::Nonconformant,
                format!("required functional group macro {t} is missing"),
            ));
        }
    }
    Ok(())
}
type Leaves = std::collections::HashMap<Tag, Element>;
fn flatten(
    budget: &mut Budget,
    target: &mut Object,
    item: &Object,
    forbidden: Option<&Leaves>,
    unassigned: bool,
) -> Result<Leaves, Error> {
    let mut leaves = Leaves::new();
    for m in item.elements() {
        let is_unassigned = m.tag() == T_UNASSIGNED_SHARED || m.tag() == T_UNASSIGNED_PER_FRAME;
        if is_unassigned != unassigned || m.tag() == T_CONVERSION_SOURCE {
            continue;
        }
        if m.vr() != Vr::SQ {
            return Err(Error::wrap(
                Kind::Nonconformant,
                format!("functional group macro {} is not SQ", m.tag()),
            ));
        }
        let items = match &m.value {
            Value::Sequence(items) if items.len() == 1 => items,
            _ => {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!(
                        "functional group macro {} must contain exactly one item",
                        m.tag()
                    ),
                ));
            }
        };
        let elements = &items[0].elements;
        let creators = if unassigned {
            Some(
                PrivateReservations::from_elements(elements, elements.len()).map_err(|e| {
                    Error::wrap(
                        Kind::Nonconformant,
                        format!("private creators in {}: {e}", m.tag()),
                    )
                })?,
            )
        } else {
            None
        };
        for leaf in elements {
            let mut mapped = leaf.clone();
            if unassigned && is_private_creator_tag(leaf.tag()) {
                continue;
            }
            if let Some(creators) = &creators
                && is_private_data_tag(leaf.tag())
            {
                let creator = creators.creator(leaf.tag()).map_err(|e| {
                    Error::wrap(
                        Kind::Nonconformant,
                        format!("private element {}: {e}", leaf.tag()),
                    )
                })?;
                let block =
                    reserve_private_creator(target, leaf.tag().group, &creator).map_err(|e| {
                        Error::wrap(
                            Kind::Nonconformant,
                            format!("reserve private creator for {}: {e}", leaf.tag()),
                        )
                    })?;
                mapped.header.tag = Tag::new(
                    leaf.tag().group,
                    u16::from(block) << 8 | leaf.tag().element & 0x00ff,
                );
            }
            if leaf.tag() == T_FRAME_TYPE {
                mapped.header.tag = T_IMAGE_TYPE;
            }
            let clone = budget.clone_element(&mapped, 2)?;
            if let Some(previous) = leaves.get(&clone.tag())
                && !elements_equal(previous, &clone)
            {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!("conflicting functional group values for {}", clone.tag()),
                ));
            }
            if let Some(previous) = forbidden.and_then(|f| f.get(&clone.tag()))
                && !elements_equal(previous, &clone)
            {
                return Err(Error::wrap(
                    Kind::Nonconformant,
                    format!("conflicting Shared and PerFrame values for {}", clone.tag()),
                ));
            }
            leaves.insert(clone.tag(), clone.clone());
            target.put(clone);
        }
    }
    Ok(leaves)
}
#[allow(clippy::too_many_arguments)]
fn build_frame_dataset(
    budget: &mut Budget,
    source: &Object,
    shared: &Object,
    per_frame: &Object,
    frame: i64,
    pixels: &[u8],
    sop_uid: &str,
    c: &FrameContext<'_>,
) -> Result<Object, Error> {
    let mut elements = Vec::with_capacity(source.len() + 32);
    for e in source.elements() {
        if MULTI_FRAME_ONLY.contains(&e.tag()) {
            continue;
        }
        elements.push(budget.clone_element(e, 1)?);
    }
    let mut ds = Object::from_elements(elements);
    ds.set_value_big_endian(false);
    let shared_state = flatten(budget, &mut ds, shared, None, false)?;
    flatten(budget, &mut ds, per_frame, Some(&shared_state), false)?;
    for item in [shared, per_frame] {
        flatten(budget, &mut ds, item, None, true)?;
    }
    validate_required_groups(source, frame, c.target_class)?;
    ds.put(ui(T_SOP_CLASS, c.target_class));
    ds.put(ui(T_SOP_INSTANCE, sop_uid));
    ds.put(ui(T_SERIES_UID, c.series));
    ds.put(str_elem(T_MODALITY, Vr::CS, c.modality));
    ds.put(str_elem(
        T_INSTANCE_NUMBER,
        Vr::IS,
        &(frame + 1).to_string(),
    ));
    let vr = match uint16_value(&ds, T_BITS_ALLOCATED) {
        Some(b) if b > 8 => Vr::OW,
        _ => Vr::OB,
    };
    let mut pixel_copy = pixels.to_vec();
    if !pixel_copy.len().is_multiple_of(2) {
        pixel_copy.push(0);
    }
    ds.put(Element {
        header: ElementHeader::new(T_PIXEL_DATA, vr, pixel_copy.len() as u32),
        value: Value::Raw(pixel_copy),
    });
    ds.put(seq(
        T_CONVERSION_SOURCE,
        vec![data_set(vec![
            ui(T_REF_SOP_CLASS, c.source_class),
            ui(T_REF_SOP_INSTANCE, c.source_sop),
            str_elem(T_REF_FRAME, Vr::IS, &(frame + 1).to_string()),
        ])],
    ));
    append_source_reference(
        &mut ds,
        Source {
            sop_class_uid: c.source_class.to_string(),
            sop_instance_uid: c.source_sop.to_string(),
            frames: vec![frame + 1],
            purpose: None,
        },
    )?;
    if !ds.has(T_DERIVATION_DESCRIPTION) {
        ds.put(str_elem(
            T_DERIVATION_DESCRIPTION,
            Vr::ST,
            CONVERSION_DESCRIPTION,
        ));
    }
    append_contributing_equipment(
        &mut ds,
        c.equipment,
        &c.now,
        CONVERSION_DESCRIPTION,
        "109106",
        "Enhanced Multi-frame Conversion Equipment",
    )?;
    let frames_text = source.get_string(T_NUMBER_OF_FRAMES).unwrap_or_default();
    let count = clean_text(&frames_text).parse::<i64>().unwrap_or(0);
    finish_classic(&mut ds, source, frame, count, c.target_class)?;
    validate_classic(&ds, c.target_class, c.modality)?;
    Ok(ds)
}
pub(crate) fn append_source_reference(ds: &mut Object, mut source: Source) -> Result<(), Error> {
    source.purpose = Some(derivation::codes::SourceImageForProcessing());
    derivation::apply(
        Some(ds),
        &Derivation {
            sources: vec![source],
            ..Derivation::default()
        },
    )
    .map_err(|e| Error::wrap(Kind::Nonconformant, format!("SourceImageSequence: {e}")))
}
pub(crate) fn format_dt(t: &ZonedTime) -> String {
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}{}",
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        format_offset(t)
    )
}
pub(crate) fn format_offset(t: &ZonedTime) -> String {
    let zone = t.offset() / 60;
    let (sign, zone) = if zone < 0 { ('-', -zone) } else { ('+', zone) };
    format!("{sign}{:02}{:02}", zone / 60, zone % 60)
}
pub(crate) fn append_contributing_equipment(
    ds: &mut Object,
    e: &ContributingEquipment,
    now: &ZonedTime,
    description: &str,
    purpose_code: &str,
    purpose_meaning: &str,
) -> Result<(), Error> {
    let mut item = vec![
        seq(
            T_PURPOSE_OF_REFERENCE,
            vec![code_item(purpose_code, "DCM", purpose_meaning)],
        ),
        str_elem(
            T_MANUFACTURER,
            Vr::LO,
            crate::siemens::trim_space(&e.manufacturer),
        ),
        str_elem(T_CONTRIBUTION_DATETIME, Vr::DT, &format_dt(now)),
        str_elem(T_CONTRIBUTION_DESCRIPTION, Vr::ST, description),
    ];
    for (t, vr, value) in [
        (T_INSTITUTION, Vr::LO, &e.institution_name),
        (T_STATION, Vr::SH, &e.station_name),
        (T_MODEL, Vr::LO, &e.manufacturer_model_name),
        (T_SERIAL, Vr::LO, &e.device_serial_number),
    ] {
        let v = crate::siemens::trim_space(value);
        if !v.is_empty() {
            item.push(str_elem(t, vr, v));
        }
    }
    let versions: Vec<String> = e
        .software_versions
        .iter()
        .map(|v| crate::siemens::trim_space(v).to_string())
        .filter(|v| !v.is_empty())
        .collect();
    if !versions.is_empty() {
        item.push(strings_elem(T_SOFTWARE, Vr::LO, &versions));
    }
    let mut items = Vec::new();
    if let Some(existing) = ds.get(T_CONTRIBUTING_EQUIPMENT) {
        let Value::Sequence(seq_items) = &existing.value else {
            return Err(Error::wrap(
                Kind::Nonconformant,
                "ContributingEquipmentSequence is malformed",
            ));
        };
        items.extend(seq_items.iter().cloned());
    }
    item.sort_by_key(|e| e.tag());
    items.push(data_set(item));
    ds.put(seq(T_CONTRIBUTING_EQUIPMENT, items));
    Ok(())
}
fn classic_iod(target: &str) -> &'static str {
    match target {
        CT_IMAGE_STORAGE_UID => "CT Image",
        MR_IMAGE_STORAGE_UID => "MR Image",
        PET_IMAGE_STORAGE_UID => "Positron Emission Tomography Image",
        _ => "",
    }
}
pub(crate) fn top_level_tags(iod: &str) -> std::collections::HashSet<Tag> {
    let mut tags = std::collections::HashSet::new();
    for usage in iodmodules::modules(iod).unwrap_or_default() {
        for a in iodmodules::attributes(usage.module).unwrap_or_default() {
            tags.insert(Tag::new((a.tag >> 16) as u16, a.tag as u16));
        }
    }
    tags
}
fn finish_classic(
    ds: &mut Object,
    source: &Object,
    frame: i64,
    frames: i64,
    target: &str,
) -> Result<(), Error> {
    if target == PET_IMAGE_STORAGE_UID {
        finish_pet(ds, source, frame, frames)?;
    }
    let iod = classic_iod(target);
    let top = top_level_tags(iod);
    let drop = ENHANCED_ONLY
        .iter()
        .copied()
        .chain(PET_CORRECTIONS.iter().map(|c| c.0));
    for t in drop {
        if !top.contains(&t) {
            ds.remove(t);
        }
    }
    emit_type2(ds, iod);
    Ok(())
}
pub(crate) fn string_of(ds: &Object, t: Tag) -> String {
    clean_text(&ds.get_string(t).unwrap_or_default()).to_string()
}
fn has_text(ds: &Object, t: Tag) -> bool {
    ds.get_string(t).is_some_and(|v| !clean_text(&v).is_empty())
}
fn put_if_absent(ds: &mut Object, t: Tag, vr: Vr, value: &str) {
    if !has_text(ds, t) {
        ds.put(str_elem(t, vr, value));
    }
}
fn put_us16(ds: &mut Object, t: Tag, value: i64) {
    ds.set_us(t, &[value.clamp(0, i64::from(u16::MAX)) as u16]);
}
fn parse_dicom_datetime(value: &str) -> Option<ZonedTime> {
    let value = crate::siemens::trim_space(value);
    if value.len() < 8 {
        return None;
    }
    dcmtime::parse_datetime(value).ok().map(|d| d.time)
}
fn enhanced_date_time(ds: &Object, offset: &str, tags: &[Tag]) -> Option<ZonedTime> {
    for &t in tags {
        let mut value = string_of(ds, t);
        if !value.contains(['+', '-']) {
            value.push_str(offset);
        }
        if let Some(t) = parse_dicom_datetime(&value) {
            return Some(t);
        }
    }
    None
}
fn in_zone(t: &ZonedTime, offset: i64) -> ZonedTime {
    ZonedTime::from_fields(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second() + offset - t.offset(),
        t.nanosecond(),
        offset,
    )
}
fn format_date(t: &ZonedTime) -> String {
    format!("{:04}{:02}{:02}", t.year(), t.month(), t.day())
}
fn format_time_frac(t: &ZonedTime) -> String {
    let mut s = format!("{:02}{:02}{:02}", t.hour(), t.minute(), t.second());
    let micros = t.nanosecond() / 1000;
    if micros > 0 {
        let frac = format!("{micros:06}");
        s.push('.');
        s.push_str(frac.trim_end_matches('0'));
    }
    s
}
fn temporal_positions(source: &Object) -> i64 {
    let mut seen = std::collections::HashSet::new();
    for item in source.get_sequence(T_PER_FRAME).unwrap_or_default() {
        for content in item.get_sequence(T_FRAME_CONTENT).unwrap_or_default() {
            if let Some(e) = content.get(T_TEMPORAL_POSITION_INDEX) {
                if let Some(raw) = e.raw() {
                    seen.insert(raw.to_vec());
                } else if let Some(v) = content.get_string(T_TEMPORAL_POSITION_INDEX) {
                    seen.insert(clean_text(&v).as_bytes().to_vec());
                }
            }
        }
    }
    (seen.len() as i64).max(1)
}
fn number_value(ds: &Object, t: Tag) -> Option<f64> {
    let e = ds.get(t)?;
    if let Some(raw) = e.raw() {
        if e.vr() == Vr::FD && raw.len() == 8 {
            let mut b = [0u8; 8];
            b.copy_from_slice(raw);
            return Some(f64::from_le_bytes(b));
        }
        if e.vr() == Vr::FL && raw.len() == 4 {
            return Some(f64::from(f32::from_le_bytes([
                raw[0], raw[1], raw[2], raw[3],
            ])));
        }
    }
    match ds.get_floats(t) {
        Ok(v) if v.len() == 1 => Some(v[0]),
        _ => None,
    }
}
fn finish_pet(ds: &mut Object, source: &Object, frame: i64, frames: i64) -> Result<(), Error> {
    let mut offset = string_of(ds, Tag::new(0x0008, 0x0201));
    if !has_text(ds, T_SERIES_DATE) || !has_text(ds, T_SERIES_TIME) {
        let mut series = enhanced_date_time(
            ds,
            &offset,
            &[
                T_ACQUISITION_DATETIME,
                T_FRAME_ACQUISITION_DATETIME,
                T_FRAME_REFERENCE_DATETIME,
            ],
        );
        if series.is_none() {
            series = enhanced_date_time(ds, &offset, &[T_CONTENT_DATE]);
        }
        if let Some(mut st) = series.filter(|t| !t.is_zero()) {
            if offset.is_empty() {
                offset = format_offset(&st);
            } else if let Some(local) =
                parse_dicom_datetime(&format!("{}{offset}", format_date(&st)))
            {
                st = in_zone(&st, local.offset());
            }
            put_if_absent(ds, T_SERIES_DATE, Vr::DA, &format_date(&st));
            put_if_absent(ds, T_SERIES_TIME, Vr::TM, &format_time_frac(&st));
        }
    }
    let reference = enhanced_date_time(
        ds,
        &offset,
        &[
            T_FRAME_REFERENCE_DATETIME,
            T_FRAME_ACQUISITION_DATETIME,
            T_ACQUISITION_DATETIME,
        ],
    );
    let mut units = string_of(ds, T_RESCALE_TYPE);
    if !PET_UNITS.contains(&units.as_str()) {
        units = "NONE".into();
    }
    put_if_absent(ds, T_UNITS, Vr::CS, &units);
    put_if_absent(ds, T_COUNTS_SOURCE, Vr::CS, "EMISSION");
    let temporal = temporal_positions(source);
    if !has_text(ds, T_SERIES_TYPE) {
        let kind = if source.has(T_CARDIAC_SYNC) || source.has(T_RESPIRATORY_SYNC) {
            "GATED"
        } else if temporal > 1 {
            "DYNAMIC"
        } else {
            "STATIC"
        };
        ds.put(strings_elem(
            T_SERIES_TYPE,
            Vr::CS,
            &[kind.to_string(), "IMAGE".to_string()],
        ));
    }
    if ds
        .get_strings(T_SERIES_TYPE)
        .and_then(|v| v.first().cloned())
        .is_some_and(|v| clean_text(&v) == "DYNAMIC")
    {
        put_us16(ds, T_NUMBER_OF_TIME_SLICES, temporal);
    }
    if !ds.has(T_NUMBER_OF_SLICES) {
        put_us16(ds, T_NUMBER_OF_SLICES, (frames / temporal.max(1)).max(1));
    }
    if !ds.has(T_CORRECTED_IMAGE) {
        let terms: Vec<String> = PET_CORRECTIONS
            .iter()
            .filter(|(flag, _)| string_of(ds, *flag) == "YES")
            .map(|(_, term)| term.to_string())
            .collect();
        if !terms.is_empty() {
            ds.put(strings_elem(T_CORRECTED_IMAGE, Vr::CS, &terms));
        }
    }
    if string_of(ds, T_DECAY_CORRECTED) == "YES"
        && (!has_text(ds, T_DECAY_CORRECTION) || string_of(ds, T_DECAY_CORRECTION) == "NONE")
    {
        let mut decay_value = string_of(ds, T_DECAY_CORRECTION_DATETIME);
        if !decay_value.contains(['+', '-']) {
            decay_value.push_str(&offset);
        }
        let decay = parse_dicom_datetime(&decay_value);
        let start = parse_dicom_datetime(&format!(
            "{}{}{offset}",
            string_of(ds, T_SERIES_DATE),
            string_of(ds, T_SERIES_TIME)
        ));
        let (Some(decay), Some(start)) = (decay, start) else {
            return Err(decay_error());
        };
        if !has_text(ds, T_SERIES_DATE) || !has_text(ds, T_SERIES_TIME) {
            return Err(decay_error());
        }
        let correction = if decay.sub_nanoseconds(&start) == 0 {
            "START"
        } else {
            "ADMIN"
        };
        ds.put(str_elem(T_DECAY_CORRECTION, Vr::CS, correction));
    } else {
        put_if_absent(ds, T_DECAY_CORRECTION, Vr::CS, "NONE");
    }
    if !has_text(ds, T_FRAME_REFERENCE_TIME) {
        let mut elapsed = 0.0f64;
        let (date, time) = (string_of(ds, T_SERIES_DATE), string_of(ds, T_SERIES_TIME));
        if let Some(reference) = reference.filter(|r| !r.is_zero())
            && !date.is_empty()
            && let Some(start) = parse_dicom_datetime(&format!("{date}{time}{offset}"))
        {
            elapsed = (reference.sub_nanoseconds(&start) / 1_000_000) as f64;
        }
        if ds
            .set_ds(T_FRAME_REFERENCE_TIME, &[crate::fmath::max(elapsed, 0.0)])
            .is_err()
        {
            ds.put(str_elem(T_FRAME_REFERENCE_TIME, Vr::DS, "0"));
        }
    }
    if !ds.has(T_IMAGE_INDEX) {
        put_us16(ds, T_IMAGE_INDEX, frame + 1);
    }
    if let Some(acquired) = enhanced_date_time(ds, &offset, &[T_FRAME_ACQUISITION_DATETIME]) {
        put_if_absent(
            ds,
            Tag::new(0x0008, 0x0022),
            Vr::DA,
            &format_date(&acquired),
        );
        let tm = format!(
            "{:02}{:02}{:02}",
            acquired.hour(),
            acquired.minute(),
            acquired.second()
        );
        put_if_absent(ds, Tag::new(0x0008, 0x0032), Vr::TM, &tm);
    }
    if let Some(ms) = number_value(ds, T_FRAME_ACQUISITION_DURATION) {
        put_if_absent(
            ds,
            T_ACTUAL_FRAME_DURATION,
            Vr::IS,
            &crate::fmath::f64_to_i64(ms.round()).to_string(),
        );
    }
    if string_of(ds, T_DECAY_CORRECTION) != "NONE" && !has_text(ds, T_DECAY_FACTOR) {
        return Err(Error::wrap(
            Kind::Nonconformant,
            "Decay Factor is required for decay-corrected PET",
        ));
    }
    Ok(())
}
fn decay_error() -> Error {
    Error::wrap(
        Kind::Nonconformant,
        "cannot derive Decay Correction from Decay Correction DateTime and series start",
    )
}
pub(crate) fn emit_type2(ds: &mut Object, iod: &str) {
    for usage in iodmodules::modules(iod).unwrap_or_default() {
        if usage.usage != "M" {
            continue;
        }
        for a in iodmodules::attributes(usage.module).unwrap_or_default() {
            if a.typ == "2" {
                put_empty(ds, Tag::new((a.tag >> 16) as u16, a.tag as u16));
            }
        }
    }
    let (report, err) = iodrules::validate(
        ds,
        &iodrules::Options {
            iod: iod.to_string(),
            ..iodrules::Options::default()
        },
    );
    if err.is_some() {
        return;
    }
    for f in report.errors() {
        if f.kind == iodrules::MISSING_TYPE2C && f.path == f.tag.to_string() {
            put_empty(ds, f.tag);
        }
    }
}
pub(crate) fn put_empty(ds: &mut Object, t: Tag) {
    if ds.has(t) {
        return;
    }
    let vr = crate::object::dict::lookup_vr(t);
    if vr == Vr::SQ {
        ds.put(Element {
            header: ElementHeader::new(t, Vr::SQ, UNDEFINED_LENGTH),
            value: Value::Sequence(Vec::new()),
        });
    } else {
        ds.put(Element {
            header: ElementHeader::new(t, vr, 0),
            value: Value::Raw(Vec::new()),
        });
    }
}
fn vr_of(ds: &Object, t: Tag) -> Option<Vr> {
    ds.get(t).map(Element::vr)
}
fn validate_classic(ds: &Object, target: &str, modality: &str) -> Result<(), Error> {
    let nc = |d: String| Err(Error::wrap(Kind::Nonconformant, d));
    match required_uid(ds, T_SOP_CLASS) {
        Ok(got) if got == target => {}
        _ => return nc("target SOP Class UID".into()),
    }
    for t in [
        T_SOP_INSTANCE,
        T_STUDY_UID,
        T_SERIES_UID,
        T_FRAME_OF_REFERENCE,
    ] {
        required_uid(ds, t)?;
        if vr_of(ds, t) != Some(Vr::UI) {
            return nc(format!("UID attribute {t} must use UI"));
        }
    }
    let m = ds.get_string(T_MODALITY);
    if m.as_deref().map(crate::siemens::trim_space) != Some(modality)
        || vr_of(ds, T_MODALITY) != Some(Vr::CS)
    {
        return nc(format!("Modality must be {modality}"));
    }
    match ds.get_strings(T_IMAGE_TYPE) {
        Some(v)
            if v.len() >= 2
                && !crate::siemens::trim_space(&v[0]).is_empty()
                && vr_of(ds, T_IMAGE_TYPE) == Some(Vr::CS) => {}
        _ => return nc("ImageType must have at least two Values".into()),
    }
    for t in [
        T_ROWS,
        T_COLUMNS,
        T_SPP,
        T_BITS_ALLOCATED,
        T_BITS_STORED,
        T_HIGH_BIT,
        T_PIXEL_REPRESENTATION,
    ] {
        if uint16_value(ds, t).is_none() || vr_of(ds, t) != Some(Vr::US) {
            return nc(format!("required pixel attribute {t}"));
        }
    }
    let v = |t| uint16_value(ds, t).unwrap_or(0);
    let (rows, columns, ba, bs, hb, spp, pr) = (
        v(T_ROWS),
        v(T_COLUMNS),
        v(T_BITS_ALLOCATED),
        v(T_BITS_STORED),
        v(T_HIGH_BIT),
        v(T_SPP),
        v(T_PIXEL_REPRESENTATION),
    );
    if rows == 0
        || columns == 0
        || ba != 16
        || !(12..=16).contains(&bs)
        || hb != bs.wrapping_sub(1)
        || spp != 1
        || pr > 1
    {
        return nc("classic CT/MR integer pixel characteristics".into());
    }
    let p = ds.get_string(T_PHOTOMETRIC);
    let pt = p.as_deref().map(crate::siemens::trim_space).unwrap_or("");
    if p.is_none()
        || vr_of(ds, T_PHOTOMETRIC) != Some(Vr::CS)
        || pt != "MONOCHROME1" && pt != "MONOCHROME2"
    {
        return nc("classic CT/MR PhotometricInterpretation".into());
    }
    for (t, vm) in [(T_PIXEL_SPACING, 2), (T_IPP, 3), (T_IOP, 6)] {
        let values = ds.get_floats(t);
        let ok = matches!(&values, Ok(v) if v.len() == vm) && vr_of(ds, t) == Some(Vr::DS);
        if !ok {
            return nc(format!("required frame geometry {t}"));
        }
        if t == T_IOP {
            let values = values.unwrap_or_default();
            match crate::object::geometry::image_orientation_deviation(&values) {
                Some(d) if d <= crate::object::geometry::IMAGE_ORIENTATION_TOLERANCE => {}
                _ => return nc("Image Orientation (Patient) is not orthonormal".into()),
            }
        }
    }
    if ds.has(T_NUMBER_OF_FRAMES) || ds.has(T_SHARED) || ds.has(T_PER_FRAME) {
        return nc("enhanced multi-frame attributes remain".into());
    }
    if !ds.has(T_CONVERSION_SOURCE) || !ds.has(T_CONTRIBUTING_EQUIPMENT) {
        return nc("conversion provenance is missing".into());
    }
    match target {
        CT_IMAGE_STORAGE_UID => {
            for t in [T_RESCALE_INTERCEPT, T_RESCALE_SLOPE] {
                let ok = matches!(ds.get_floats(t), Ok(v) if v.len() == 1)
                    && vr_of(ds, t) == Some(Vr::DS);
                if !ok {
                    return nc(format!("required CT attribute {t}"));
                }
            }
            for (t, vr) in [(T_KVP, Vr::DS), (T_ACQUISITION_NUMBER, Vr::IS)] {
                if !ds.has(t) || vr_of(ds, t) != Some(vr) {
                    return nc(format!("required CT Type 2 attribute {t}"));
                }
            }
        }
        PET_IMAGE_STORAGE_UID => {
            for t in [T_RESCALE_INTERCEPT, T_RESCALE_SLOPE, T_FRAME_REFERENCE_TIME] {
                if !matches!(ds.get_floats(t), Ok(v) if v.len() == 1) {
                    return nc(format!("required PET attribute {t}"));
                }
            }
            for t in [
                T_UNITS,
                T_COUNTS_SOURCE,
                T_SERIES_TYPE,
                T_DECAY_CORRECTION,
                T_SERIES_DATE,
                T_SERIES_TIME,
            ] {
                let ok = ds
                    .get_strings(t)
                    .is_some_and(|v| !v.is_empty() && !clean_text(&v[0]).is_empty());
                if !ok {
                    return nc(format!("required PET attribute {t}"));
                }
            }
            for t in [T_NUMBER_OF_SLICES, T_IMAGE_INDEX] {
                if uint16_value(ds, t).is_none() {
                    return nc(format!("required PET attribute {t}"));
                }
            }
        }
        MR_IMAGE_STORAGE_UID => {
            for t in [T_SCANNING_SEQUENCE, T_SEQUENCE_VARIANT] {
                let ok = ds.get_strings(t).is_some_and(|v| {
                    !v.is_empty() && !crate::siemens::trim_space(&v[0]).is_empty()
                }) && vr_of(ds, t) == Some(Vr::CS);
                if !ok {
                    return nc(format!("required MR attribute {t}"));
                }
            }
            for (t, vr) in [
                (T_SCAN_OPTIONS, Vr::CS),
                (T_MR_ACQUISITION_TYPE, Vr::CS),
                (T_ECHO_TIME, Vr::DS),
                (T_ECHO_TRAIN_LENGTH, Vr::IS),
            ] {
                if !ds.has(t) || vr_of(ds, t) != Some(vr) {
                    return nc(format!("required MR Type 2 attribute {t}"));
                }
            }
        }
        _ => {}
    }
    Ok(())
}
