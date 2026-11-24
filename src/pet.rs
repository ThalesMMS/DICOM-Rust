#![forbid(unsafe_code)]
use crate::dcmtime::{PrecisionLevel, ZonedTime};
use crate::fmath;
use crate::numtext::parse_int;
use crate::object::{Object, Tag};
use crate::textfmt::to_upper;
const PET_UNITS_BQML: &str = "BQML";
const PET_UNITS_GML: &str = "GML";
const PET_SUV_TYPE_BW: &str = "BW";
const PET_DECAY_CORRECTION_ADMIN: &str = "ADMIN";
const PET_DECAY_CORRECTION_START: &str = "START";
const PET_CORRECTED_IMAGE_DECAY: &str = "DECY";
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_ACQUISITION_DATE: Tag = Tag::new(0x0008, 0x0022);
const TAG_ACQUISITION_DATE_TIME: Tag = Tag::new(0x0008, 0x002A);
const TAG_ACQUISITION_TIME: Tag = Tag::new(0x0008, 0x0032);
const TAG_TIMEZONE_OFFSET_FROM_UTC: Tag = Tag::new(0x0008, 0x0201);
const TAG_PATIENT_WEIGHT: Tag = Tag::new(0x0010, 0x1030);
const TAG_RADIOPHARMACEUTICAL_START_TIME: Tag = Tag::new(0x0018, 0x1072);
const TAG_RADIONUCLIDE_TOTAL_DOSE: Tag = Tag::new(0x0018, 0x1074);
const TAG_RADIONUCLIDE_HALF_LIFE: Tag = Tag::new(0x0018, 0x1075);
const TAG_RADIOPHARMACEUTICAL_START_DATE_TIME: Tag = Tag::new(0x0018, 0x1078);
const TAG_CORRECTED_IMAGE: Tag = Tag::new(0x0028, 0x0051);
const TAG_RADIOPHARMACEUTICAL_INFORMATION_SEQUENCE: Tag = Tag::new(0x0054, 0x0016);
const TAG_UNITS: Tag = Tag::new(0x0054, 0x1001);
const TAG_SUV_TYPE: Tag = Tag::new(0x0054, 0x1006);
const TAG_DECAY_CORRECTION: Tag = Tag::new(0x0054, 0x1102);
#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub units: String,
    pub suv_type: String,
    pub decay_correction: String,
    pub corrected_image: Vec<String>,
    pub patient_weight_kg: f64,
    pub total_dose_bq: f64,
    pub half_life_seconds: f64,
    pub administration_time: ZonedTime,
    pub administration_time_ok: bool,
    pub acquisition_time: ZonedTime,
    pub acquisition_time_ok: bool,
}
impl Default for Metadata {
    fn default() -> Metadata {
        Metadata {
            units: String::new(),
            suv_type: String::new(),
            decay_correction: String::new(),
            corrected_image: Vec::new(),
            patient_weight_kg: 0.0,
            total_dose_bq: 0.0,
            half_life_seconds: 0.0,
            administration_time: ZonedTime::zero(),
            administration_time_ok: false,
            acquisition_time: ZonedTime::zero(),
            acquisition_time_ok: false,
        }
    }
}
impl Metadata {
    pub fn suv_bw(&self, rescaled_value: f64) -> Option<f64> {
        if !finite_non_negative(rescaled_value) {
            return None;
        }
        let units = normalize_pet_term(&self.units);
        if units == PET_UNITS_GML {
            let suv_type = normalize_pet_term(&self.suv_type);
            if !suv_type.is_empty() && suv_type != PET_SUV_TYPE_BW {
                return None;
            }
            return Some(rescaled_value);
        }
        if units != PET_UNITS_BQML
            || !finite_positive(self.patient_weight_kg)
            || !finite_positive(self.total_dose_bq)
            || !contains_pet_term(&self.corrected_image, PET_CORRECTED_IMAGE_DECAY)
        {
            return None;
        }
        let mut dose_at_reference = self.total_dose_bq;
        match normalize_pet_term(&self.decay_correction).as_str() {
            PET_DECAY_CORRECTION_ADMIN => {}
            PET_DECAY_CORRECTION_START => {
                if !finite_positive(self.half_life_seconds)
                    || !self.administration_time_ok
                    || !self.acquisition_time_ok
                {
                    return None;
                }
                let elapsed = duration_seconds(
                    self.acquisition_time
                        .sub_nanoseconds(&self.administration_time),
                );
                if elapsed < 0.0 || !elapsed.is_finite() {
                    return None;
                }
                dose_at_reference *=
                    fmath::exp(-std::f64::consts::LN_2 * elapsed / self.half_life_seconds);
            }
            _ => return None,
        }
        if !finite_positive(dose_at_reference) {
            return None;
        }
        let suv = rescaled_value * (self.patient_weight_kg * 1000.0) / dose_at_reference;
        if !finite_non_negative(suv) {
            return None;
        }
        Some(suv)
    }
}
fn duration_seconds(nanoseconds: i64) -> f64 {
    let sec = nanoseconds / 1_000_000_000;
    let nsec = nanoseconds % 1_000_000_000;
    sec as f64 + nsec as f64 / 1e9
}
pub fn parse_classic_metadata(dataset: &Object) -> Option<Metadata> {
    let modality = dataset_term(dataset, TAG_MODALITY);
    if modality != "PT" && modality != "PET" {
        return None;
    }
    let mut meta = Metadata {
        units: dataset_term(dataset, TAG_UNITS),
        suv_type: dataset_term(dataset, TAG_SUV_TYPE),
        decay_correction: dataset_term(dataset, TAG_DECAY_CORRECTION),
        ..Metadata::default()
    };
    if let Some(values) = dataset.get_strings(TAG_CORRECTED_IMAGE) {
        meta.corrected_image = values
            .iter()
            .map(|v| normalize_pet_term(v))
            .filter(|v| !v.is_empty())
            .collect();
    }
    meta.patient_weight_kg = single_finite_float(dataset, TAG_PATIENT_WEIGHT);
    let (offset, timezone_known) = dicom_timezone(dataset);
    let acquisition_offset_known =
        match dicom_acquisition_date_time(dataset, offset, timezone_known) {
            Some((value, offset_known)) => {
                meta.acquisition_time = value;
                meta.acquisition_time_ok = true;
                offset_known
            }
            None => timezone_known,
        };
    let Some(items) = dataset.get_sequence(TAG_RADIOPHARMACEUTICAL_INFORMATION_SEQUENCE) else {
        return Some(meta);
    };
    let [isotope] = items.as_slice() else {
        return Some(meta);
    };
    meta.total_dose_bq = single_finite_float(isotope, TAG_RADIONUCLIDE_TOTAL_DOSE);
    meta.half_life_seconds = single_finite_float(isotope, TAG_RADIONUCLIDE_HALF_LIFE);
    let mut administration_offset_known = false;
    if let Some((value, offset_known)) = dicom_date_time(
        isotope,
        TAG_RADIOPHARMACEUTICAL_START_DATE_TIME,
        offset,
        timezone_known,
    ) {
        meta.administration_time = value;
        meta.administration_time_ok = true;
        administration_offset_known = offset_known;
    } else if meta.acquisition_time_ok {
        if let Some(value) = dicom_time_on_date(
            isotope,
            TAG_RADIOPHARMACEUTICAL_START_TIME,
            &meta.acquisition_time,
            offset,
        ) {
            meta.administration_time = value;
            meta.administration_time_ok = true;
        }
        administration_offset_known = timezone_known;
    }
    if meta.administration_time_ok
        && meta.acquisition_time_ok
        && administration_offset_known != acquisition_offset_known
    {
        meta.administration_time_ok = false;
    }
    Some(meta)
}
fn dicom_acquisition_date_time(
    obj: &Object,
    offset: i64,
    timezone_known: bool,
) -> Option<(ZonedTime, bool)> {
    if let Some(found) = dicom_date_time(obj, TAG_ACQUISITION_DATE_TIME, offset, timezone_known) {
        return Some(found);
    }
    dicom_date_and_time(obj, TAG_ACQUISITION_DATE, TAG_ACQUISITION_TIME, offset)
        .map(|value| (value, timezone_known))
}
fn dicom_date_time(
    obj: &Object,
    tag: Tag,
    offset: i64,
    timezone_known: bool,
) -> Option<(ZonedTime, bool)> {
    let value = obj.lookup_date_time(tag).ok()?;
    if value.precision > PrecisionLevel::MINUTES {
        return None;
    }
    let mut parsed = value.time;
    if value.no_offset {
        parsed = ZonedTime::from_fields(
            parsed.year(),
            parsed.month(),
            parsed.day(),
            parsed.hour(),
            parsed.minute(),
            parsed.second(),
            parsed.nanosecond(),
            offset,
        );
    }
    Some((parsed, !value.no_offset || timezone_known))
}
fn dicom_date_and_time(
    obj: &Object,
    date_tag: Tag,
    time_tag: Tag,
    offset: i64,
) -> Option<ZonedTime> {
    let date = obj.lookup_date(date_tag).ok()?;
    if date.precision > PrecisionLevel::DAY {
        return None;
    }
    let clock = obj.lookup_time(time_tag).ok()?;
    if clock.precision > PrecisionLevel::MINUTES {
        return None;
    }
    Some(ZonedTime::from_fields(
        date.time.year(),
        date.time.month(),
        date.time.day(),
        clock.time.hour(),
        clock.time.minute(),
        clock.time.second(),
        clock.time.nanosecond(),
        offset,
    ))
}
fn dicom_time_on_date(
    obj: &Object,
    time_tag: Tag,
    date: &ZonedTime,
    offset: i64,
) -> Option<ZonedTime> {
    if date.is_zero() {
        return None;
    }
    let clock = obj.lookup_time(time_tag).ok()?;
    if clock.precision > PrecisionLevel::MINUTES {
        return None;
    }
    Some(ZonedTime::from_fields(
        date.year(),
        date.month(),
        date.day(),
        clock.time.hour(),
        clock.time.minute(),
        clock.time.second(),
        clock.time.nanosecond(),
        offset,
    ))
}
fn dicom_timezone(obj: &Object) -> (i64, bool) {
    let Some(raw) = obj.get_string(TAG_TIMEZONE_OFFSET_FROM_UTC) else {
        return (0, false);
    };
    let raw = raw.trim().as_bytes();
    if raw.len() != 5 || (raw[0] != b'+' && raw[0] != b'-') {
        return (0, false);
    }
    let field = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&raw[range])
            .ok()
            .and_then(|s| parse_int(s).ok())
    };
    let (Some(hours), Some(minutes)) = (field(1..3), field(3..5)) else {
        return (0, false);
    };
    if hours > 14 || minutes > 59 || (hours == 14 && minutes != 0) {
        return (0, false);
    }
    let offset = hours * 3600 + minutes * 60;
    (if raw[0] == b'-' { -offset } else { offset }, true)
}
fn single_finite_float(obj: &Object, tag: Tag) -> f64 {
    match obj.get_floats(tag) {
        Ok(values) if values.len() == 1 && values[0].is_finite() => values[0],
        _ => 0.0,
    }
}
fn dataset_term(obj: &Object, tag: Tag) -> String {
    let value = obj.get_string(tag).unwrap_or_default();
    normalize_pet_term(value.trim_end_matches('\0'))
}
fn normalize_pet_term(value: &str) -> String {
    to_upper(value.trim())
}
fn contains_pet_term(values: &[String], want: &str) -> bool {
    let want = normalize_pet_term(want);
    values.iter().any(|v| normalize_pet_term(v) == want)
}
fn finite_positive(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn finite_non_negative(value: f64) -> bool {
    value >= 0.0 && value.is_finite()
}
