#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::Object;
use super::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use crate::dcmtime::ZonedTime;
pub mod inherit_modules {
    pub const PATIENT: u16 = 1;
    pub const CLINICAL_TRIAL: u16 = 1 << 1;
    pub const STUDY: u16 = 1 << 2;
    pub const FRAME_OF_REFERENCE: u16 = 1 << 3;
    pub const EQUIPMENT: u16 = 1 << 4;
    pub const INSTANCE_CREATION: u16 = 1 << 5;
    pub const ALL: u16 =
        PATIENT | CLINICAL_TRIAL | STUDY | FRAME_OF_REFERENCE | EQUIPMENT | INSTANCE_CREATION;
    pub const WITHOUT_FRAME_OF_REFERENCE: u16 = ALL & !FRAME_OF_REFERENCE;
}
use inherit_modules as m;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Equipment {
    pub manufacturer: String,
    pub model_name: String,
    pub device_serial_number: String,
    pub software_versions: String,
}
pub fn default_equipment() -> Equipment {
    Equipment {
        manufacturer: "dcm-rust".into(),
        model_name: "dcm-rust".into(),
        device_serial_number: "dcm-rust".into(),
        software_versions: "dcm-rust".into(),
    }
}
#[derive(Clone, Debug, Default)]
pub struct InheritOptions {
    pub modules: u16,
    pub equipment: Option<Equipment>,
    pub overwrite: bool,
    pub now: Option<ZonedTime>,
}
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
#[rustfmt::skip]
const PATIENT_TAGS: &[Tag] = &[
    t(0x0008, 0x1120),
    t(0x0010, 0x0010), t(0x0010, 0x0020), t(0x0010, 0x0021), t(0x0010, 0x0022),
    t(0x0010, 0x0024), t(0x0010, 0x0026), t(0x0010, 0x0027), t(0x0010, 0x0028),
    t(0x0010, 0x0030), t(0x0010, 0x0032), t(0x0010, 0x0033), t(0x0010, 0x0034),
    t(0x0010, 0x0035), t(0x0010, 0x0040), t(0x0010, 0x0200), t(0x0010, 0x0212),
    t(0x0010, 0x0213), t(0x0010, 0x0214), t(0x0010, 0x0215), t(0x0010, 0x0216),
    t(0x0010, 0x0217), t(0x0010, 0x0218), t(0x0010, 0x0219), t(0x0010, 0x0221),
    t(0x0010, 0x0222), t(0x0010, 0x0223), t(0x0010, 0x0229),
    t(0x0010, 0x1000), t(0x0010, 0x1001), t(0x0010, 0x1002), t(0x0010, 0x1100),
    t(0x0010, 0x2160), t(0x0010, 0x2201), t(0x0010, 0x2202), t(0x0010, 0x2203),
    t(0x0010, 0x2292), t(0x0010, 0x2293), t(0x0010, 0x2294), t(0x0010, 0x2295),
    t(0x0010, 0x2296), t(0x0010, 0x2297), t(0x0010, 0x2298), t(0x0010, 0x2299),
    t(0x0010, 0x4000), t(0x0012, 0x0062), t(0x0012, 0x0063), t(0x0012, 0x0064),
    t(0x0008, 0x1080), t(0x0008, 0x1084), t(0x0010, 0x1010), t(0x0010, 0x1020),
    t(0x0010, 0x1021), t(0x0010, 0x1022), t(0x0010, 0x1023), t(0x0010, 0x1024),
    t(0x0010, 0x1030), t(0x0010, 0x2000), t(0x0010, 0x2110), t(0x0010, 0x2180),
    t(0x0010, 0x21a0), t(0x0010, 0x21b0), t(0x0010, 0x21c0), t(0x0010, 0x21d0),
    t(0x0038, 0x0010), t(0x0038, 0x0014), t(0x0038, 0x0060), t(0x0038, 0x0062),
    t(0x0038, 0x0064), t(0x0038, 0x0500), t(0x0040, 0x3001),
];
#[rustfmt::skip]
const CLINICAL_TRIAL_TAGS: &[Tag] = &[
    t(0x0012, 0x0010), t(0x0012, 0x0020), t(0x0012, 0x0021), t(0x0012, 0x0022),
    t(0x0012, 0x0023), t(0x0012, 0x0030), t(0x0012, 0x0031), t(0x0012, 0x0040),
    t(0x0012, 0x0042), t(0x0012, 0x0081), t(0x0012, 0x0082),
    t(0x0012, 0x0050), t(0x0012, 0x0051), t(0x0012, 0x0052), t(0x0012, 0x0053),
    t(0x0012, 0x0083),
];
#[rustfmt::skip]
const STUDY_TAGS: &[Tag] = &[
    t(0x0008, 0x0020), t(0x0008, 0x0030), t(0x0008, 0x0050), t(0x0008, 0x0051),
    t(0x0008, 0x0090), t(0x0008, 0x0096), t(0x0008, 0x009c), t(0x0008, 0x009d),
    t(0x0008, 0x1030), t(0x0008, 0x1032), t(0x0008, 0x1048), t(0x0008, 0x1049),
    t(0x0008, 0x1060), t(0x0008, 0x1062), t(0x0008, 0x1110), t(0x0020, 0x000d),
    t(0x0020, 0x0010), t(0x0032, 0x1033), t(0x0032, 0x1034), t(0x0032, 0x1066),
    t(0x0032, 0x1067), t(0x0040, 0x1012),
];
const FRAME_OF_REFERENCE_TAGS: &[Tag] = &[t(0x0020, 0x0052), t(0x0020, 0x1040)];
const TAG_CHARACTER_SET: Tag = t(0x0008, 0x0005);
const TAG_INSTANCE_CREATION_DATE: Tag = t(0x0008, 0x0012);
const TAG_INSTANCE_CREATION_TIME: Tag = t(0x0008, 0x0013);
const TAG_TIMEZONE_OFFSET_FROM_UTC: Tag = t(0x0008, 0x0201);
const TAG_STUDY_UID: Tag = t(0x0020, 0x000d);
const TAG_FRAME_OF_REFERENCE_UID: Tag = t(0x0020, 0x0052);
const TAG_POSITION_REFERENCE_INDICATOR: Tag = t(0x0020, 0x1040);
const TYPE2: &[(u16, &[(Tag, Vr)])] = &[
    (
        m::PATIENT,
        &[
            (t(0x0010, 0x0010), Vr::PN),
            (t(0x0010, 0x0020), Vr::LO),
            (t(0x0010, 0x0030), Vr::DA),
            (t(0x0010, 0x0040), Vr::CS),
        ],
    ),
    (
        m::STUDY,
        &[
            (t(0x0008, 0x0020), Vr::DA),
            (t(0x0008, 0x0030), Vr::TM),
            (t(0x0008, 0x0050), Vr::SH),
            (t(0x0008, 0x0090), Vr::PN),
            (t(0x0020, 0x0010), Vr::SH),
        ],
    ),
];
fn empty_raw(tag: Tag, vr: Vr) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, 0),
        value: Value::Raw(Vec::new()),
    }
}
fn strings_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
pub fn new_uid() -> String {
    format!("2.25.{}", uuid::Uuid::new_v4().as_u128())
}
pub fn trim_nul_padding(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| {
            if v.ends_with('\0') {
                v.trim_end_matches([' ', '\0']).to_string()
            } else {
                v.clone()
            }
        })
        .collect()
}
fn parse_numeric_zone(value: &str) -> Option<i64> {
    let b = value.as_bytes();
    if b.len() != 5 || !b[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let hour = i64::from(b[1] - b'0') * 10 + i64::from(b[2] - b'0');
    let minute = i64::from(b[3] - b'0') * 10 + i64::from(b[4] - b'0');
    if hour > 24 || minute > 60 {
        return None;
    }
    let offset = (hour * 60 + minute) * 60;
    match b[0] {
        b'+' => Some(offset),
        b'-' => Some(-offset),
        _ => None,
    }
}
fn local_now() -> ZonedTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    ZonedTime::from_fields(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
        i64::from(now.nanosecond()),
        i64::from(now.offset().local_minus_utc()),
    )
}
fn in_zone(t: &ZonedTime, offset: i64) -> ZonedTime {
    ZonedTime::from_fields(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second() - t.offset() + offset,
        t.nanosecond(),
        offset,
    )
}
fn pad(out: &mut String, v: i64, width: usize) {
    if v < 0 {
        out.push('-');
    }
    let digits = v.unsigned_abs().to_string();
    for _ in digits.len()..width {
        out.push('0');
    }
    out.push_str(&digits);
}
impl Object {
    pub(crate) fn detached_element_for_order(
        &self,
        element: &Element,
        big: bool,
    ) -> Result<Element, Error> {
        let mut element = element.clone();
        let vr = element.vr();
        if (vr == Vr::SQ || vr == Vr::UN && matches!(element.value, Value::Sequence(_)))
            && let Some(items) = self.get_sequence(element.tag())
        {
            let mut out = Vec::with_capacity(items.len());
            for item in &items {
                let mut ds = DataSet::default();
                for e in item.elements() {
                    ds.elements.push(item.detached_element_for_order(e, big)?);
                }
                out.push(ds);
            }
            element.value = Value::Sequence(out);
            return Ok(element);
        }
        if vr.is_string_like()
            && element.tag() != TAG_CHARACTER_SET
            && let Ok(values) = self.decode_text_values(&element)
        {
            let values = if vr == Vr::UI {
                values
            } else {
                trim_nul_padding(&values)
            };
            element.value = Value::Strings(values);
            element.header.length = 0;
            element.header.length_set = false;
            return Ok(element);
        }
        if let Value::Raw(raw) = &element.value
            && self.value_big_endian() != big
        {
            let width = vr.byte_order_width();
            if width > 1 {
                if raw.len() % width != 0 {
                    return Err(Error::msg(format!(
                        "dicom: inherited {} with VR {vr} has incomplete {width}-byte numeric unit",
                        element.tag()
                    )));
                }
                let mut reversed = raw.clone();
                for chunk in reversed.chunks_exact_mut(width) {
                    chunk.reverse();
                }
                element.value = Value::Raw(reversed);
            }
        }
        Ok(element)
    }
}
pub fn inherit_common_modules(
    dst: &mut Object,
    src: Option<&Object>,
    opts: &InheritOptions,
) -> Result<(), Error> {
    let modules = if opts.modules == 0 {
        m::ALL
    } else {
        opts.modules
    };
    let big = dst.value_big_endian();
    if let Some(src) = src
        && modules & (m::PATIENT | m::CLINICAL_TRIAL | m::STUDY) != 0
        && !dst.has(TAG_CHARACTER_SET)
        && let Some(element) = src.get(TAG_CHARACTER_SET)
    {
        let copied = src.detached_element_for_order(element, big)?;
        dst.put(copied);
    }
    for (module, tags) in [
        (m::PATIENT, PATIENT_TAGS),
        (m::CLINICAL_TRIAL, CLINICAL_TRIAL_TAGS),
        (m::STUDY, STUDY_TAGS),
        (m::FRAME_OF_REFERENCE, FRAME_OF_REFERENCE_TAGS),
    ] {
        let Some(src) = src else { continue };
        if modules & module == 0 {
            continue;
        }
        if module == m::FRAME_OF_REFERENCE && !src.has(TAG_FRAME_OF_REFERENCE_UID) {
            continue;
        }
        for &tag in tags {
            if dst.has(tag) && !opts.overwrite {
                continue;
            }
            let Some(element) = src.get(tag) else {
                continue;
            };
            let copied = src.detached_element_for_order(element, big)?;
            dst.put(copied);
        }
    }
    for &(module, attributes) in TYPE2 {
        if modules & module == 0 {
            continue;
        }
        for &(tag, vr) in attributes {
            if !dst.has(tag) {
                dst.put(empty_raw(tag, vr));
            }
        }
    }
    if modules & m::FRAME_OF_REFERENCE != 0
        && dst.has(TAG_FRAME_OF_REFERENCE_UID)
        && !dst.has(TAG_POSITION_REFERENCE_INDICATOR)
    {
        dst.put(empty_raw(TAG_POSITION_REFERENCE_INDICATOR, Vr::LO));
    }
    if modules & m::STUDY != 0 && !dst.has(TAG_STUDY_UID) {
        dst.put(strings_element(TAG_STUDY_UID, Vr::UI, vec![new_uid()]));
    }
    if modules & m::EQUIPMENT != 0 {
        let equipment = opts.equipment.clone().unwrap_or_else(default_equipment);
        for (tag, value) in [
            (t(0x0008, 0x0070), &equipment.manufacturer),
            (t(0x0008, 0x1090), &equipment.model_name),
            (t(0x0018, 0x1000), &equipment.device_serial_number),
            (t(0x0018, 0x1020), &equipment.software_versions),
        ] {
            let value = value.trim();
            if !value.is_empty() && (opts.overwrite || !dst.has(tag)) {
                dst.put(strings_element(tag, Vr::LO, vec![value.to_string()]));
            }
        }
    }
    if modules & m::INSTANCE_CREATION != 0
        && (opts.overwrite || !dst.has(TAG_INSTANCE_CREATION_DATE))
    {
        let mut now = opts.now.unwrap_or_else(local_now);
        if let Some(src) = src
            && !dst.has(TAG_TIMEZONE_OFFSET_FROM_UTC)
            && let Some(element) = src.get(TAG_TIMEZONE_OFFSET_FROM_UTC)
            && let Ok(copied) = src.detached_element_for_order(element, big)
        {
            dst.put(copied);
        }
        if let Some(offset) = dst.get_string(TAG_TIMEZONE_OFFSET_FROM_UTC)
            && let Some(zone) = parse_numeric_zone(offset.trim())
        {
            now = in_zone(&now, zone);
        }
        let mut date = String::new();
        pad(&mut date, now.year(), 4);
        pad(&mut date, now.month(), 2);
        pad(&mut date, now.day(), 2);
        let mut time = String::new();
        pad(&mut time, now.hour(), 2);
        pad(&mut time, now.minute(), 2);
        pad(&mut time, now.second(), 2);
        dst.put(strings_element(
            TAG_INSTANCE_CREATION_DATE,
            Vr::DA,
            vec![date],
        ));
        dst.put(strings_element(
            TAG_INSTANCE_CREATION_TIME,
            Vr::TM,
            vec![time],
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, Default)]
pub struct DerivedCommon {
    pub source: Option<Object>,
    pub equipment: Option<Equipment>,
    pub now: Option<ZonedTime>,
}
impl DerivedCommon {
    pub fn apply(&self, dst: &mut Object, modules: u16) -> Result<(), Error> {
        apply_derived_common(
            dst,
            self.source.as_ref(),
            self.equipment.clone(),
            self.now,
            modules,
        )
    }
}
pub(crate) fn apply_derived_common(
    dst: &mut Object,
    source: Option<&Object>,
    equipment: Option<Equipment>,
    now: Option<ZonedTime>,
    modules: u16,
) -> Result<(), Error> {
    inherit_common_modules(
        dst,
        source,
        &InheritOptions {
            modules,
            equipment,
            overwrite: false,
            now,
        },
    )
}
