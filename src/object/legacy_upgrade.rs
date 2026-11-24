#![forbid(unsafe_code)]
use super::error::Error;
use super::file::File;
use super::obj::Object;
use super::transfer::{self, IMPLICIT_VR_LITTLE_ENDIAN};
use super::types::{Element, ElementHeader, Tag, Value, Vr};
use super::writer::write_data_set;
use crate::deid::uid::name_uid_bytes;
use crate::textfmt;
use sha2::{Digest, Sha256};
pub const ERR_LEGACY_SOP_CLASS: &str = "dicom: cannot infer the SOP Class of a legacy data set";
pub const LEGACY_DIAGNOSTIC_LIBIDO_DIMENSIONS: &str = "libido-dimensions-swapped";
pub const LEGACY_DIAGNOSTIC_LIBIDO_RGB24: &str = "libido-rgb24-normalized";
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LegacySopClassSource {
    #[default]
    Unset,
    Declared,
    Modality,
    SecondaryCapture,
}
impl LegacySopClassSource {
    pub fn as_str(self) -> &'static str {
        match self {
            LegacySopClassSource::Unset => "",
            LegacySopClassSource::Declared => "declared",
            LegacySopClassSource::Modality => "modality",
            LegacySopClassSource::SecondaryCapture => "secondary-capture",
        }
    }
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LegacyUpgradeReport {
    pub sop_class_uid: String,
    pub sop_class_source: LegacySopClassSource,
    pub modality: String,
    pub generated: Vec<Tag>,
    pub removed: Vec<Tag>,
    pub defaulted: Vec<Tag>,
    pub corrected: Vec<Tag>,
    pub diagnostics: Vec<String>,
}
impl LegacyUpgradeReport {
    pub fn changed(&self) -> bool {
        self.sop_class_source != LegacySopClassSource::Declared
            || !self.generated.is_empty()
            || !self.removed.is_empty()
            || !self.defaulted.is_empty()
            || !self.corrected.is_empty()
    }
}
const TAG_RECOGNITION_CODE: Tag = Tag::new(0x0008, 0x0010);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_SAMPLES: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_NUMBER_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_PLANAR: Tag = Tag::new(0x0028, 0x0006);
const STUDY_IDENTITY: [Tag; 6] = [
    Tag::new(0x0010, 0x0020),
    Tag::new(0x0010, 0x0010),
    Tag::new(0x0008, 0x0020),
    Tag::new(0x0008, 0x0030),
    Tag::new(0x0020, 0x0010),
    Tag::new(0x0008, 0x0050),
];
const SERIES_IDENTITY: [Tag; 4] = [
    Tag::new(0x0020, 0x0011),
    Tag::new(0x0008, 0x0021),
    Tag::new(0x0008, 0x0031),
    Tag::new(0x0008, 0x103E),
];
const SECONDARY_CAPTURE: &str = "1.2.840.10008.5.1.4.1.1.7";
pub const DEFAULT_LEGACY_UID_NAMESPACE: &str = "dcm-rust legacy upgrade";
fn modality_classes(modality: &str) -> Option<(&'static str, &'static str)> {
    Some(match modality {
        "CR" => ("1.2.840.10008.5.1.4.1.1.1", ""),
        "DX" => ("1.2.840.10008.5.1.4.1.1.1.1", ""),
        "MG" => ("1.2.840.10008.5.1.4.1.1.1.2", ""),
        "IO" => ("1.2.840.10008.5.1.4.1.1.1.3", ""),
        "CT" => ("1.2.840.10008.5.1.4.1.1.2", ""),
        "MR" => ("1.2.840.10008.5.1.4.1.1.4", ""),
        "US" => ("1.2.840.10008.5.1.4.1.1.6.1", "1.2.840.10008.5.1.4.1.1.3.1"),
        "XA" => ("1.2.840.10008.5.1.4.1.1.12.1", ""),
        "RF" => ("1.2.840.10008.5.1.4.1.1.12.2", ""),
        "NM" => ("1.2.840.10008.5.1.4.1.1.20", ""),
        "PT" => ("1.2.840.10008.5.1.4.1.1.128", ""),
        "RTIMAGE" => ("1.2.840.10008.5.1.4.1.1.481.1", ""),
        _ => return None,
    })
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(is_space_char)
}
fn is_space_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{000B}'
            | '\u{000C}'
            | '\r'
            | ' '
            | '\u{0085}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}
fn trim_ws_bytes(mut b: &[u8]) -> &[u8] {
    while let Some((c, n)) = first_char(b) {
        if !is_space_char(c) {
            break;
        }
        b = &b[n..];
    }
    while let Some((c, n)) = last_char(b) {
        if !is_space_char(c) {
            break;
        }
        b = &b[..b.len() - n];
    }
    b
}
fn first_char(b: &[u8]) -> Option<(char, usize)> {
    for n in 1..=b.len().min(4) {
        if let Ok(s) = std::str::from_utf8(&b[..n]) {
            return s.chars().next().map(|c| (c, n));
        }
    }
    None
}
fn last_char(b: &[u8]) -> Option<(char, usize)> {
    for n in 1..=b.len().min(4) {
        if let Ok(s) = std::str::from_utf8(&b[b.len() - n..]) {
            return s.chars().next().map(|c| (c, n));
        }
    }
    None
}
fn new_string_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn us_element(tag: Tag, value: u16, big_endian: bool) -> Element {
    let data = if big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    Element {
        header: ElementHeader::new(tag, Vr::US, 2),
        value: Value::Raw(data.to_vec()),
    }
}
fn legacy_us(dataset: &Object, tag: Tag) -> Option<u16> {
    let element = dataset.get(tag)?;
    if element.vr() != Vr::US {
        return None;
    }
    let values = dataset.get_ints(tag).ok()?;
    if values.len() != 1 || values[0] < 0 || values[0] > i64::from(u16::MAX) {
        return None;
    }
    Some(values[0] as u16)
}
pub fn upgrade_legacy(file: &File) -> Result<(File, LegacyUpgradeReport), Error> {
    upgrade_legacy_in_namespace(file, DEFAULT_LEGACY_UID_NAMESPACE)
}
pub fn upgrade_legacy_in_namespace(
    file: &File,
    namespace: &str,
) -> Result<(File, LegacyUpgradeReport), Error> {
    let source = &file.dataset;
    let mut dataset = Object::new();
    dataset.set_text_options(source.text_options().clone());
    dataset.set_value_big_endian(source.value_big_endian());
    let mut report = LegacyUpgradeReport::default();
    for element in source.elements() {
        let tag = element.tag();
        if tag == TAG_RECOGNITION_CODE
            || tag.element == 0x0000 && tag.group != 0x0000 && tag.group != 0x0002
        {
            report.removed.push(tag);
            continue;
        }
        dataset.put(element.clone());
    }
    dataset.value_provider = source.value_provider.clone();
    let has_pixels = dataset.get(Tag::PIXEL_DATA).is_some();
    if has_pixels {
        if !file.transfer_syntax.encapsulated {
            normalize_libido(source, &mut dataset, &mut report);
        }
        if !dataset.has(TAG_SAMPLES) {
            dataset.put(us_element(TAG_SAMPLES, 1, dataset.value_big_endian()));
            report.defaulted.push(TAG_SAMPLES);
        }
        if !dataset.has(TAG_PHOTOMETRIC) && dataset.get_int(TAG_SAMPLES).is_ok_and(|n| n == 1) {
            dataset.put(new_string_element(TAG_PHOTOMETRIC, Vr::CS, "MONOCHROME2"));
            report.defaulted.push(TAG_PHOTOMETRIC);
        }
    }
    let sop_class = dataset.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
    let sop_class = trim_unicode_space(&sop_class);
    if !sop_class.is_empty() {
        report.sop_class_uid = sop_class.to_string();
        report.sop_class_source = LegacySopClassSource::Declared;
    } else if !has_pixels {
        return Err(Error::msg(format!(
            "{ERR_LEGACY_SOP_CLASS}: no SOP Class UID and no Pixel Data"
        )));
    } else {
        let modality = dataset.get_string(TAG_MODALITY).unwrap_or_default();
        let modality = textfmt::to_upper(trim_unicode_space(&modality));
        match modality_classes(&modality) {
            Some((single, multi_frame)) => {
                report.sop_class_uid = single.to_string();
                report.sop_class_source = LegacySopClassSource::Modality;
                if dataset.get_int(TAG_NUMBER_FRAMES).is_ok_and(|n| n > 1)
                    && !multi_frame.is_empty()
                {
                    report.sop_class_uid = multi_frame.to_string();
                }
            }
            None => {
                report.sop_class_uid = SECONDARY_CAPTURE.to_string();
                report.sop_class_source = LegacySopClassSource::SecondaryCapture;
            }
        }
        report.modality = modality;
        dataset.put(new_string_element(
            TAG_SOP_CLASS_UID,
            Vr::UI,
            &report.sop_class_uid,
        ));
    }
    assign_legacy_uids(&mut dataset, &mut report, namespace)?;
    let mut syntax = file.transfer_syntax;
    if syntax.uid.is_empty() || transfer::is_legacy(syntax.uid) {
        syntax = IMPLICIT_VR_LITTLE_ENDIAN;
    }
    Ok((
        File {
            preamble: vec![0; 128],
            meta: Object::new(),
            dataset,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        },
        report,
    ))
}
fn normalize_libido(source: &Object, dataset: &mut Object, report: &mut LegacyUpgradeReport) {
    let class = source.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
    let code = source.get_string(TAG_RECOGNITION_CODE).unwrap_or_default();
    if !trim_unicode_space(&class).is_empty()
        || code != "ACRNEMA_LIBIDO_1.0" && code != "ACRNEMA_LIBIDO_1.1"
    {
        return;
    }
    let Some(pixel) = source.get(Tag::PIXEL_DATA) else {
        return;
    };
    if !matches!(pixel.value, Value::Nil | Value::Raw(_)) {
        return;
    }
    let (Some(rows), Some(columns)) = (legacy_us(source, TAG_ROWS), legacy_us(source, TAG_COLUMNS))
    else {
        return;
    };
    if rows == 0 || columns == 0 {
        return;
    }
    let big_endian = dataset.value_big_endian();
    let put = |dataset: &mut Object, report: &mut LegacyUpgradeReport, tag: Tag, value: u16| {
        dataset.put(us_element(tag, value, big_endian));
        report.corrected.push(tag);
    };
    if rows != columns {
        put(dataset, report, TAG_ROWS, columns);
        put(dataset, report, TAG_COLUMNS, rows);
        report
            .diagnostics
            .push(LEGACY_DIAGNOSTIC_LIBIDO_DIMENSIONS.to_string());
    }
    for (tag, want) in [
        (TAG_BITS_ALLOCATED, 24),
        (TAG_BITS_STORED, 24),
        (TAG_HIGH_BIT, 23),
        (TAG_REPRESENTATION, 0),
    ] {
        if legacy_us(source, tag) != Some(want) {
            return;
        }
    }
    if source.has(TAG_SAMPLES) && legacy_us(source, TAG_SAMPLES) != Some(1) {
        return;
    }
    let photometric = source.get_string(TAG_PHOTOMETRIC).unwrap_or_default();
    if !trim_unicode_space(&photometric).is_empty() {
        return;
    }
    if source.has(TAG_PLANAR) && legacy_us(source, TAG_PLANAR) != Some(0) {
        return;
    }
    let mut frames = 1i64;
    let mut frames_from_planes = false;
    if source.has(TAG_NUMBER_FRAMES) {
        match source.get_int(TAG_NUMBER_FRAMES) {
            Ok(n) if n > 0 && n <= i64::from(i32::MAX) => frames = n,
            _ => return,
        }
    } else if legacy_us(source, Tag::new(0x0028, 0x0005)) == Some(3)
        && let Some(planes) = legacy_us(source, Tag::new(0x0028, 0x0012))
        && planes > 1
    {
        frames = i64::from(planes);
        frames_from_planes = true;
    }
    let frame_bytes = i64::from(rows) * i64::from(columns) * 3;
    let length = match &pixel.value {
        Value::Raw(raw) => Some(raw.len() as i64),
        _ if pixel.header.length_set && pixel.header.length != super::UNDEFINED_LENGTH => {
            Some(i64::from(pixel.header.length))
        }
        _ => None,
    };
    let Some(length) = length else { return };
    if frames > i64::MAX / frame_bytes || length != frame_bytes * frames {
        return;
    }
    if frames_from_planes {
        dataset.put(new_string_element(
            TAG_NUMBER_FRAMES,
            Vr::IS,
            &frames.to_string(),
        ));
        report.defaulted.push(TAG_NUMBER_FRAMES);
        report
            .diagnostics
            .insert(0, "number-of-frames-from-planes".to_string());
    }
    put(dataset, report, TAG_SAMPLES, 3);
    dataset.put(new_string_element(TAG_PHOTOMETRIC, Vr::CS, "RGB"));
    report.corrected.push(TAG_PHOTOMETRIC);
    put(dataset, report, TAG_PLANAR, 0);
    put(dataset, report, TAG_BITS_ALLOCATED, 8);
    put(dataset, report, TAG_BITS_STORED, 8);
    put(dataset, report, TAG_HIGH_BIT, 7);
    report
        .diagnostics
        .push(LEGACY_DIAGNOSTIC_LIBIDO_RGB24.to_string());
}
fn assign_legacy_uids(
    dataset: &mut Object,
    report: &mut LegacyUpgradeReport,
    namespace: &str,
) -> Result<(), Error> {
    let missing = |dataset: &Object, tag: Tag| {
        trim_unicode_space(&dataset.get_uid(tag).unwrap_or_default()).is_empty()
    };
    let need_instance = missing(dataset, TAG_SOP_INSTANCE_UID);
    let need_study = missing(dataset, TAG_STUDY_INSTANCE_UID);
    let need_series = missing(dataset, TAG_SERIES_INSTANCE_UID);
    if !need_instance && !need_study && !need_series {
        return Ok(());
    }
    let encoded = write_data_set(dataset, IMPLICIT_VR_LITTLE_ENDIAN.uid)
        .map_err(|e| Error::wrap("dicom: digest legacy data set", e))?;
    let instance: String = Sha256::digest(&encoded)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if need_instance {
        let uid = name_uid_bytes(&[
            namespace.as_bytes(),
            b"instance".as_slice(),
            instance.as_bytes(),
        ]);
        dataset.put(new_string_element(TAG_SOP_INSTANCE_UID, Vr::UI, &uid));
        report.generated.push(TAG_SOP_INSTANCE_UID);
    }
    if need_study {
        let uid = group_uid(dataset, namespace, "study", &instance, &STUDY_IDENTITY, &[]);
        dataset.put(new_string_element(TAG_STUDY_INSTANCE_UID, Vr::UI, &uid));
        report.generated.push(TAG_STUDY_INSTANCE_UID);
    }
    if need_series {
        let study = dataset.get_uid(TAG_STUDY_INSTANCE_UID).unwrap_or_default();
        let modality = dataset.get_string(TAG_MODALITY).unwrap_or_default();
        let extra = [
            trim_unicode_space(&study).as_bytes().to_vec(),
            trim_unicode_space(&modality).as_bytes().to_vec(),
        ];
        let uid = group_uid(
            dataset,
            namespace,
            "series",
            &instance,
            &SERIES_IDENTITY,
            &extra,
        );
        dataset.put(new_string_element(TAG_SERIES_INSTANCE_UID, Vr::UI, &uid));
        report.generated.push(TAG_SERIES_INSTANCE_UID);
    }
    Ok(())
}
fn group_uid(
    dataset: &Object,
    namespace: &str,
    kind: &str,
    instance: &str,
    identity: &[Tag],
    extra: &[Vec<u8>],
) -> String {
    let mut parts: Vec<Vec<u8>> = vec![namespace.as_bytes().to_vec(), kind.as_bytes().to_vec()];
    parts.extend(extra.iter().cloned());
    let mut identified = false;
    for tag in identity {
        let mut value: Vec<u8> = Vec::new();
        if let Some(element) = dataset.get(*tag) {
            match &element.value {
                Value::Raw(raw) => {
                    let end = raw
                        .iter()
                        .rposition(|b| *b != b' ' && *b != 0)
                        .map_or(0, |i| i + 1);
                    value = raw[..end].to_vec();
                }
                Value::Strings(values) => value = values.join("\\").into_bytes(),
                _ => {}
            }
        }
        let value = trim_ws_bytes(&value).to_vec();
        identified = identified || !value.is_empty();
        parts.push(value);
    }
    if !identified {
        parts.push(b"instance".to_vec());
        parts.push(instance.as_bytes().to_vec());
    }
    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
    name_uid_bytes(&refs)
}
#[cfg(test)]
mod namespace_tests {
    use super::*;
    fn legacy_ct() -> File {
        let mut dataset = Object::new();
        dataset.put(new_string_element(TAG_MODALITY, Vr::CS, "CT"));
        dataset.put(new_string_element(Tag::new(0x0010, 0x0020), Vr::LO, "P1"));
        dataset.put(Element {
            header: ElementHeader::new(Tag::PIXEL_DATA, Vr::OW, 4),
            value: Value::Raw(vec![1, 0, 2, 0]),
        });
        File {
            preamble: Vec::new(),
            meta: Object::new(),
            dataset,
            transfer_syntax: IMPLICIT_VR_LITTLE_ENDIAN,
            partial_read: None,
            incomplete_pixel_data: None,
        }
    }
    fn uids(file: &File) -> Vec<String> {
        [
            TAG_SOP_INSTANCE_UID,
            TAG_STUDY_INSTANCE_UID,
            TAG_SERIES_INSTANCE_UID,
        ]
        .iter()
        .map(|tag| file.dataset.get_uid(*tag).unwrap_or_default())
        .collect()
    }
    #[test]
    fn the_namespace_names_every_generated_uid() {
        let file = legacy_ct();
        let (plain, _) = upgrade_legacy(&file).unwrap();
        let (default, _) =
            upgrade_legacy_in_namespace(&file, DEFAULT_LEGACY_UID_NAMESPACE).unwrap();
        let (other, _) = upgrade_legacy_in_namespace(&file, "another namespace").unwrap();
        let (again, _) = upgrade_legacy_in_namespace(&file, "another namespace").unwrap();
        assert!(uids(&plain).iter().all(|uid| !uid.is_empty()));
        assert_eq!(uids(&plain), uids(&default));
        assert_eq!(uids(&other), uids(&again));
        for (a, b) in uids(&plain).iter().zip(uids(&other).iter()) {
            assert_ne!(a, b);
        }
    }
}
