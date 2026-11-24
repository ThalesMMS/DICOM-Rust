#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{
    raw_element, sequence_element, string_values, text_element, text_values_element,
};
use super::error::{Error, ErrorKind};
use super::fileset::{
    Diagnostic, DiagnosticCode, FileRecord, FileSet, SourceSnapshot, ValidationReport,
};
use super::graph::RecordExtras;
use super::keys::{KeyBudget, own_directory_element, read_handle};
use super::snapshot::{alias_counts, planned_directory_key_usage};
use super::{Reference, open_referenced_file};
use crate::iconimage::{self, DecompressLimits, GenerateOptions};
use crate::object::text::normalize_uid;
use crate::object::{DataSet, Element, File, Object, Tag, Vr, read_file};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Profile(pub Cow<'static, str>);
impl Profile {
    pub const GENERAL_PURPOSE: Profile = Profile(Cow::Borrowed("STD-GEN-CD"));
    pub const GENERAL_DVD_JPEG: Profile = Profile(Cow::Borrowed("STD-GEN-DVD-JPEG"));
    #[doc(hidden)]
    pub const GENERAL_DVD_J2K: Profile = Profile(Cow::Borrowed("STD-GEN-DVD-J2K"));
    pub const GENERAL_USB_JPEG: Profile = Profile(Cow::Borrowed("STD-GEN-USB-JPEG"));
    #[doc(hidden)]
    pub const GENERAL_USB_J2K: Profile = Profile(Cow::Borrowed("STD-GEN-USB-J2K"));
    pub const XA1K_CD: Profile = Profile(Cow::Borrowed("STD-XA1K-CD"));
    #[doc(hidden)]
    pub const XA1K_DVD: Profile = Profile(Cow::Borrowed("STD-XA1K-DVD"));
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl From<&str> for Profile {
    fn from(s: &str) -> Profile {
        Profile(Cow::Owned(s.to_string()))
    }
}
impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Clone, Debug, Default)]
pub struct ProfileError {
    pub profile: Profile,
    pub diagnostics: Vec<Diagnostic>,
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} ({} finding(s))",
            ErrorKind::ProfileViolation.text(),
            self.profile,
            self.diagnostics.len()
        )
    }
}
impl std::error::Error for ProfileError {}
const SOP_CLASS_XA: &str = "1.2.840.10008.5.1.4.1.1.12.1";
const SOP_CLASS_SC: &str = "1.2.840.10008.5.1.4.1.1.7";
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const JPEG_LOSSLESS_SV1: &str = "1.2.840.10008.1.2.4.70";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
const JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
const JPEG2000_LOSSLESS: &str = "1.2.840.10008.1.2.4.90";
const JPEG2000: &str = "1.2.840.10008.1.2.4.91";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syntaxes {
    ExplicitOnly,
    Jpeg,
    J2k,
    Xa { dvd: bool },
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProfileRules {
    syntaxes: Syntaxes,
    xa_classes: bool,
    pub icon_size: i64,
    general_keys: bool,
    xa_keys: bool,
    compression_keys: bool,
    xa_images: bool,
}
impl ProfileRules {
    fn transfer_syntax(&self, sop_class: &str, syntax: &str) -> bool {
        match self.syntaxes {
            Syntaxes::ExplicitOnly => syntax == EXPLICIT_LE,
            Syntaxes::Jpeg => matches!(
                syntax,
                EXPLICIT_LE | JPEG_LOSSLESS_SV1 | JPEG_BASELINE | JPEG_EXTENDED
            ),
            Syntaxes::J2k => matches!(syntax, EXPLICIT_LE | JPEG2000_LOSSLESS | JPEG2000),
            Syntaxes::Xa { dvd } => {
                if sop_class == SOP_CLASS_SC {
                    return syntax == EXPLICIT_LE;
                }
                syntax == JPEG_LOSSLESS_SV1
                    || dvd && (syntax == JPEG_BASELINE || syntax == JPEG_EXTENDED)
            }
        }
    }
}
pub(crate) fn profile_rules_for(profile: &Profile) -> Result<ProfileRules, Error> {
    let general = |syntaxes, compression| ProfileRules {
        syntaxes,
        xa_classes: false,
        icon_size: 0,
        general_keys: true,
        xa_keys: false,
        compression_keys: compression,
        xa_images: false,
    };
    match profile.as_str() {
        "STD-GEN-CD" => Ok(general(Syntaxes::ExplicitOnly, false)),
        "STD-GEN-DVD-JPEG" | "STD-GEN-USB-JPEG" => Ok(general(Syntaxes::Jpeg, true)),
        "STD-GEN-DVD-J2K" | "STD-GEN-USB-J2K" => Ok(general(Syntaxes::J2k, true)),
        "STD-XA1K-CD" | "STD-XA1K-DVD" => Ok(ProfileRules {
            syntaxes: Syntaxes::Xa {
                dvd: profile.as_str() == "STD-XA1K-DVD",
            },
            xa_classes: true,
            icon_size: 128,
            general_keys: false,
            xa_keys: true,
            compression_keys: false,
            xa_images: true,
        }),
        other => Err(Error::unknown_profile(other)),
    }
}
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_INSTITUTION_NAME: Tag = Tag::new(0x0008, 0x0080);
const TAG_INSTITUTION_ADDRESS: Tag = Tag::new(0x0008, 0x0081);
const TAG_REFERENCED_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x1140);
const TAG_PERFORMING_PHYSICIAN_NAME: Tag = Tag::new(0x0008, 0x1050);
const TAG_PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
const TAG_PATIENT_SEX: Tag = Tag::new(0x0010, 0x0040);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_LOSSY_COMPRESSION_RATIO: Tag = Tag::new(0x0028, 0x2112);
const TAG_CALIBRATION_IMAGE: Tag = Tag::new(0x0050, 0x0004);
pub(crate) const TAG_ICON_IMAGE_SEQUENCE: Tag = Tag::new(0x0088, 0x0200);
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_ACQUISITION_DATE_TIME: Tag = Tag::new(0x0008, 0x002A);
const TAG_ACQUISITION_TIME_SYNCHRONIZED: Tag = Tag::new(0x0018, 0x1800);
const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
const TAG_SYNCHRONIZATION_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0200);
const TAG_PLANE_POSITION_SEQUENCE: Tag = Tag::new(0x0020, 0x9113);
const TAG_PLANE_ORIENTATION_SEQUENCE: Tag = Tag::new(0x0020, 0x9116);
const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_PIXEL_MEASURES_SEQUENCE: Tag = Tag::new(0x0028, 0x9110);
type Finding = (DiagnosticCode, String);
pub(crate) type ExtrasResult = (HashMap<String, RecordExtras>, Vec<Diagnostic>, i64);
fn inspect_profile(
    rules: &ProfileRules,
    record: &FileRecord,
    dataset: &Object,
) -> (RecordExtras, Vec<Finding>) {
    let mut extras = RecordExtras::default();
    let mut findings: Vec<Finding> = Vec::new();
    let mut fail = |code: DiagnosticCode, message: &str| findings.push((code, message.to_string()));
    let syntax = normalize_uid(&record.transfer_syntax_uid);
    if rules.xa_classes
        && record.sop_class_uid != SOP_CLASS_XA
        && record.sop_class_uid != SOP_CLASS_SC
    {
        fail(
            DiagnosticCode::PROFILE_SOP_CLASS,
            "SOP class is not part of the profile",
        );
    } else if !rules.transfer_syntax(&record.sop_class_uid, syntax) {
        fail(
            DiagnosticCode::PROFILE_TRANSFER_SYNTAX,
            "transfer syntax is not allowed for this SOP class by the profile",
        );
    }
    if rules.general_keys {
        for (name, value) in [
            ("Patient ID", &record.patient_id),
            ("Study Date", &record.study_date),
            ("Study Time", &record.study_time),
            ("Study ID", &record.study_id),
            ("Series Number", &record.series_number),
            ("Instance Number", &record.instance_number),
        ] {
            if value.trim().is_empty() {
                fail(
                    DiagnosticCode::PROFILE_MISSING_KEY,
                    &format!("{name} is empty but Type 1 in the profile"),
                );
            }
        }
    }
    if rules.xa_images {
        for f in xa_image_findings(record, dataset) {
            findings.push(f);
        }
    }
    let mut fail = |code: DiagnosticCode, message: &str| findings.push((code, message.to_string()));
    if let Some(e) = dataset.get(TAG_IMAGE_TYPE) {
        extras.image.push(e.clone());
    } else if rules.xa_keys && record.sop_class_uid == SOP_CLASS_XA {
        fail(
            DiagnosticCode::PROFILE_MISSING_KEY,
            "Image Type is required for X-Ray Angiographic images",
        );
    }
    if let Some(e) = dataset.get(TAG_REFERENCED_IMAGE_SEQUENCE) {
        extras.image.push(e.clone());
    }
    if rules.xa_keys {
        extras
            .patient
            .push(copied_or_empty(dataset, TAG_PATIENT_BIRTH_DATE, Vr::DA));
        extras
            .patient
            .push(copied_or_empty(dataset, TAG_PATIENT_SEX, Vr::CS));
        extras
            .series
            .push(copied_or_empty(dataset, TAG_INSTITUTION_NAME, Vr::LO));
        extras
            .series
            .push(copied_or_empty(dataset, TAG_INSTITUTION_ADDRESS, Vr::ST));
        extras.series.push(copied_or_empty(
            dataset,
            TAG_PERFORMING_PHYSICIAN_NAME,
            Vr::PN,
        ));
        extras
            .image
            .push(copied_or_empty(dataset, TAG_CALIBRATION_IMAGE, Vr::CS));
        if let Some(e) = dataset.get(TAG_LOSSY_COMPRESSION_RATIO) {
            extras.image.push(e.clone());
        }
    }
    if rules.compression_keys {
        for tag in [TAG_PATIENT_BIRTH_DATE, TAG_PATIENT_SEX] {
            if let Some(e) = non_empty(dataset, tag) {
                extras.patient.push(e);
            }
        }
        for tag in [
            TAG_INSTITUTION_NAME,
            TAG_INSTITUTION_ADDRESS,
            TAG_PERFORMING_PHYSICIAN_NAME,
        ] {
            if let Some(e) = non_empty(dataset, tag) {
                extras.series.push(e);
            }
        }
        for tag in [TAG_CALIBRATION_IMAGE, TAG_LOSSY_COMPRESSION_RATIO] {
            if let Some(e) = non_empty(dataset, tag) {
                extras.image.push(e);
            }
        }
        for tag in [TAG_ROWS, TAG_COLUMNS] {
            match dataset.get(tag) {
                Some(e) => extras.image.push(e.clone()),
                None => fail(
                    DiagnosticCode::PROFILE_MISSING_KEY,
                    "Rows and Columns are Type 1 in the profile",
                ),
            }
        }
        for tag in [
            TAG_FRAME_OF_REFERENCE_UID,
            TAG_SYNCHRONIZATION_FRAME_OF_REFERENCE_UID,
            TAG_NUMBER_OF_FRAMES,
            TAG_ACQUISITION_TIME_SYNCHRONIZED,
            TAG_ACQUISITION_DATE_TIME,
        ] {
            if let Some(e) = dataset.get(tag) {
                extras.image.push(e.clone());
            }
        }
        for (group, tag) in [
            (TAG_PLANE_POSITION_SEQUENCE, TAG_IMAGE_POSITION_PATIENT),
            (
                TAG_PLANE_ORIENTATION_SEQUENCE,
                TAG_IMAGE_ORIENTATION_PATIENT,
            ),
            (TAG_PIXEL_MEASURES_SEQUENCE, TAG_PIXEL_SPACING),
        ] {
            if let Some(e) = top_level_or_shared(dataset, group, tag) {
                extras.image.push(e);
            }
        }
    }
    (extras, findings)
}
fn non_empty(dataset: &Object, tag: Tag) -> Option<Element> {
    let e = dataset.get(tag)?;
    let ok = match e.raw() {
        Some(raw) => !String::from_utf8_lossy(raw)
            .trim_matches([' ', '\0'])
            .is_empty(),
        None => !string_values(e).join("").trim().is_empty(),
    };
    ok.then(|| e.clone())
}
fn top_level_or_shared(dataset: &Object, group: Tag, tag: Tag) -> Option<Element> {
    if let Some(e) = dataset.get(tag) {
        return Some(e.clone());
    }
    let (item, _, _) = dataset.functional_group_item(-1, group);
    item?.get(tag).cloned()
}
fn us_value(dataset: &Object, tag: Tag) -> Option<i64> {
    let e = dataset.get(tag)?;
    if e.vr() != Vr::US {
        return None;
    }
    let raw = e.raw()?;
    let &[b0, b1, ..] = raw else {
        return None;
    };
    let b = [b0, b1];
    Some(i64::from(if dataset.value_big_endian() {
        u16::from_be_bytes(b)
    } else {
        u16::from_le_bytes(b)
    }))
}
fn xa_image_findings(record: &FileRecord, dataset: &Object) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut fail =
        |message: &str| findings.push((DiagnosticCode::PROFILE_IMAGE, message.to_string()));
    let rows = us_value(dataset, TAG_ROWS);
    let columns = us_value(dataset, TAG_COLUMNS);
    match (rows, columns) {
        (Some(r), Some(c)) if (1..=1024).contains(&r) && (1..=1024).contains(&c) => {}
        _ => fail("Rows and Columns must be 1 to 1024"),
    }
    let stored = us_value(dataset, TAG_BITS_STORED).unwrap_or(0);
    match record.sop_class_uid.as_str() {
        SOP_CLASS_XA => {
            if dataset.get_string(TAG_MODALITY).unwrap_or_default().trim() != "XA" {
                fail("Modality must be XA");
            }
            if stored != 8 && stored != 10 && stored != 12 {
                fail("X-Ray Angiographic Bits Stored must be 8, 10 or 12");
            }
        }
        SOP_CLASS_SC => {
            let samples = us_value(dataset, TAG_SAMPLES_PER_PIXEL).unwrap_or(0);
            let allocated = us_value(dataset, TAG_BITS_ALLOCATED).unwrap_or(0);
            let high = us_value(dataset, TAG_HIGH_BIT).unwrap_or(0);
            let representation = us_value(dataset, TAG_PIXEL_REPRESENTATION).unwrap_or(0);
            let photometric = dataset.get_string(TAG_PHOTOMETRIC).unwrap_or_default();
            if samples != 1
                || photometric.trim() != "MONOCHROME2"
                || allocated != 8
                || stored != 8
                || high != 7
                || representation != 0
            {
                fail("Secondary Capture must be 8-bit unsigned MONOCHROME2");
            }
            if dataset.elements().iter().any(|e| {
                let g = e.tag().group;
                (0x6000..=0x60FF).contains(&g) && g % 2 == 0
            }) {
                fail("Secondary Capture must not carry overlay groups");
            }
        }
        _ => {}
    }
    findings
}
fn copied_or_empty(dataset: &Object, tag: Tag, vr: Vr) -> Element {
    match dataset.get(tag) {
        Some(e) => e.clone(),
        None => raw_element(tag, vr, Vec::new()),
    }
}
fn render_icon(file: &File, size: i64) -> Result<Element, String> {
    iconimage::generate(
        file,
        &GenerateOptions {
            width: size,
            height: size,
            monochrome: true,
            ignore_padding: true,
            big_endian: Some(false),
            limits: DecompressLimits::defaults(),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())
}
fn icon_sequence(pixels: Vec<u8>, size: i64) -> Element {
    let us = |tag: Tag, value: i64| raw_element(tag, Vr::US, (value as u16).to_le_bytes().to_vec());
    let item = DataSet {
        elements: vec![
            us(TAG_SAMPLES_PER_PIXEL, 1),
            text_element(TAG_PHOTOMETRIC, Vr::CS, "MONOCHROME2"),
            us(TAG_ROWS, size),
            us(TAG_COLUMNS, size),
            text_values_element(
                Tag::new(0x0028, 0x0034),
                Vr::IS,
                vec!["1".into(), "1".into()],
            ),
            us(TAG_BITS_ALLOCATED, 8),
            us(TAG_BITS_STORED, 8),
            us(TAG_HIGH_BIT, 7),
            us(TAG_PIXEL_REPRESENTATION, 0),
            raw_element(Tag::PIXEL_DATA, Vr::OB, pixels),
        ],
        item_offset: None,
    };
    sequence_element(TAG_ICON_IMAGE_SEQUENCE, vec![item])
}
impl FileSet {
    fn read_referenced_data_set(&self, record: &FileRecord) -> Result<File, ()> {
        let reference = Reference {
            file_id: record.file_id.0.clone(),
            typ: String::new(),
        };
        let (handle, _) = open_referenced_file(&self.root, &reference).map_err(|_| ())?;
        let data = read_handle(handle).map_err(|_| ())?;
        read_file(&data).map_err(|_| ())
    }
    pub fn validate_profile(&self, profile: &Profile) -> Result<ValidationReport, Error> {
        let rules = profile_rules_for(profile)?;
        let mut report = ValidationReport {
            valid: true,
            statistics: self.statistics(),
            diagnostics: Vec::new(),
        };
        for (i, record) in self.records().iter().enumerate() {
            let Ok(file) = self.read_referenced_data_set(record) else {
                report.valid = false;
                report.diagnostics.push(Diagnostic {
                    code: DiagnosticCode::SOURCE_CHANGED,
                    source_index: i,
                    message: "referenced file could not be read".to_string(),
                });
                continue;
            };
            let (_, findings) = inspect_profile(&rules, record, &file.dataset);
            for (code, message) in findings {
                report.valid = false;
                report.diagnostics.push(Diagnostic {
                    code,
                    source_index: i,
                    message,
                });
            }
        }
        Ok(report)
    }
    pub(crate) fn record_extras(
        &self,
        records: &[SourceSnapshot],
        options: &super::WriteOptions,
    ) -> Result<ExtrasResult, Error> {
        let rules = if options.profile.is_empty() {
            None
        } else {
            Some(profile_rules_for(&options.profile)?)
        };
        let (mut icons, mut size) = (options.icons, options.icon_size);
        if let Some(r) = &rules
            && r.icon_size != 0
        {
            icons = true;
            size = r.icon_size;
        }
        if size == 0 {
            size = 64;
        }
        let mut out = HashMap::new();
        if rules.is_none() && !icons {
            return Ok((out, Vec::new(), 0));
        }
        let mut findings: Vec<Diagnostic> = Vec::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut count = 0i64;
        let imported = self.directory.as_deref();
        let max_total = self.options.limits.max_total_record_key_bytes;
        let mut used = planned_directory_key_usage(imported, records, max_total)?;
        let aliases = alias_counts(imported);
        for (i, stored) in records.iter().enumerate() {
            let typ = stored.record.record_type.as_str();
            let supports_icon = typ == "IMAGE" || typ == "RT DOSE";
            let Ok(file) = self.read_referenced_data_set(&stored.record) else {
                return Err(Error::source_validation(i));
            };
            let mut extra = RecordExtras::default();
            if let Some(rules) = &rules {
                let (e, found) = inspect_profile(rules, &stored.record, &file.dataset);
                extra = e;
                for (code, message) in found {
                    findings.push(Diagnostic {
                        code,
                        source_index: i,
                        message,
                    });
                }
            }
            let key = stored.record.file_id.to_string();
            let n = aliases.get(&key).copied().filter(|&n| n != 0).unwrap_or(1);
            let remaining = max_total.saturating_sub(used).checked_div(n).unwrap_or(0);
            if remaining <= 0 {
                return Err(ErrorKind::ResourceLimit.into());
            }
            let mut key_limits = self.options.limits;
            if remaining < key_limits.max_record_key_bytes {
                key_limits.max_record_key_bytes = remaining;
            }
            let mut budget = KeyBudget::new(key_limits);
            for list in [&mut extra.patient, &mut extra.series, &mut extra.image] {
                for e in list.iter_mut() {
                    if file.dataset.has(e.tag()) {
                        *e = own_directory_element(&file.dataset, e.tag(), &mut budget, 0)?;
                    }
                }
            }
            if icons && supports_icon && findings.is_empty() {
                budget.charge(size.saturating_mul(size).saturating_add(512), 0)?;
                let icon = match render_icon(&file, size) {
                    Ok(icon) => icon,
                    Err(cause) => {
                        if !options.blank_icon_on_failure {
                            return Err(Error::icon(i, &cause));
                        }
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::ICON_UNAVAILABLE,
                            source_index: i,
                            message: "icon replaced by a black image".to_string(),
                        });
                        icon_sequence(vec![0; size.saturating_mul(size) as usize], size)
                    }
                };
                extra.image.push(icon);
                count = count.saturating_add(1);
            }
            used = used.saturating_add(budget.bytes.saturating_mul(n));
            out.insert(key, extra);
        }
        if !findings.is_empty() {
            return Err(Error::profile(ProfileError {
                profile: options.profile.clone(),
                diagnostics: findings,
            }));
        }
        Ok((out, diagnostics, count))
    }
}
