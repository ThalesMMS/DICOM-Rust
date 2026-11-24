use super::brachy::read_brachy;
use super::ion::read_ion;
use super::read::{Reader, join};
use super::tags::*;
use super::validate::validate_plan;
use super::*;
use crate::object::transfer::{self, Syntax};
use crate::object::{DataSet, Element, File, Object, Tag, Value, Vr};
use crate::validation::iod;
use crate::validation::{ValidationOptions, validate_data_set_with};
use std::collections::HashSet;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_metadata_bytes: i64,
    pub max_value_bytes: i64,
    pub max_model_bytes: i64,
    pub max_pixel_bytes: i64,
    pub max_elements: i64,
    pub max_depth: i64,
    pub max_beams: i64,
    pub max_entities: i64,
    pub max_control_points: i64,
    pub max_spots: i64,
    pub max_problems: i64,
}
pub fn default_limits() -> Limits {
    Limits {
        max_metadata_bytes: 64 << 20,
        max_value_bytes: 16 << 20,
        max_model_bytes: 128 << 20,
        max_pixel_bytes: 64 << 20,
        max_elements: 100000,
        max_depth: 32,
        max_beams: 1024,
        max_entities: 10000,
        max_control_points: 100000,
        max_spots: 1000000,
        max_problems: 256,
    }
}
fn limit_error(detail: &str) -> Error {
    Error {
        kind: ErrorKind::ResourceLimit,
        text: format!("{}: {detail}", ErrorKind::ResourceLimit.text()),
        validation: false,
        problems: Vec::new(),
    }
}
impl Limits {
    pub fn normalized(&self) -> Result<Limits, Error> {
        let d = default_limits();
        let mut l = *self;
        for (p, dv) in [
            (&mut l.max_metadata_bytes, d.max_metadata_bytes),
            (&mut l.max_value_bytes, d.max_value_bytes),
            (&mut l.max_model_bytes, d.max_model_bytes),
            (&mut l.max_pixel_bytes, d.max_pixel_bytes),
        ] {
            if *p < 0 || *p > i64::MAX / 16 {
                return Err(limit_error("negative limit"));
            }
            if *p == 0 {
                *p = dv;
            }
        }
        for (p, dv) in [
            (&mut l.max_elements, d.max_elements),
            (&mut l.max_depth, d.max_depth),
            (&mut l.max_beams, d.max_beams),
            (&mut l.max_entities, d.max_entities),
            (&mut l.max_control_points, d.max_control_points),
            (&mut l.max_spots, d.max_spots),
            (&mut l.max_problems, d.max_problems),
        ] {
            if *p < 0 || *p > i64::from(i32::MAX) {
                return Err(limit_error("negative limit"));
            }
            if *p == 0 {
                *p = dv;
            }
        }
        if l.max_depth > 128 {
            return Err(limit_error("depth exceeds 128"));
        }
        Ok(l)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub limits: Limits,
    pub strict_iod: bool,
}
pub(crate) fn bulk_tag(t: Tag) -> bool {
    t.group == 0x7fe0 || t == ft(0x54001010)
}
pub(crate) struct Admission {
    pub limits: Limits,
    pub bytes: i64,
    pub pixels: i64,
    pub model: i64,
    pub elements: i64,
    items: i64,
    beams: i64,
    entities: i64,
    points: i64,
    spots: i64,
    inherited_bytes: i64,
    shifter_settings: i64,
}
pub(crate) fn preflight(obj: &Object, l: &Limits, pixels: bool) -> Result<Admission, Error> {
    let limits = l.normalized()?;
    preflight_prepared(obj, limits, pixels)
}
pub(crate) fn preflight_prepared(
    obj: &Object,
    limits: Limits,
    pixels: bool,
) -> Result<Admission, Error> {
    let mut b = Admission {
        limits,
        bytes: 0,
        pixels: 0,
        model: 0,
        elements: 0,
        items: 0,
        beams: 0,
        entities: 0,
        points: 0,
        spots: 0,
        inherited_bytes: 0,
        shifter_settings: 0,
    };
    b.reserve(limits.max_problems, 8192)?;
    if obj.len() as i64 > limits.max_elements {
        return Err(path_error(ErrorKind::ResourceLimit, "", "element count"));
    }
    b.reserve(obj.len() as i64, 64)?;
    b.visit(obj.elements(), "", 0, pixels)?;
    b.reserve(b.inherited_bytes * 6, b.points)?;
    b.reserve(b.shifter_settings * 256, b.points)?;
    Ok(b)
}
fn count_backslashes(raw: &[u8]) -> i64 {
    raw.iter().filter(|&&c| c == b'\\').count() as i64
}
impl Admission {
    pub fn reserve(&mut self, n: i64, m: i64) -> Result<(), Error> {
        if n < 0 || m < 0 || m != 0 && n > (self.limits.max_model_bytes - self.model) / m {
            return Err(path_error(ErrorKind::ResourceLimit, "", "model allowance"));
        }
        self.model += n * m;
        Ok(())
    }
    fn visit(&mut self, es: &[Element], path: &str, depth: i64, pixels: bool) -> Result<(), Error> {
        if depth > self.limits.max_depth
            || es.len() as i64 > self.limits.max_elements - self.elements
        {
            return Err(path_error(
                ErrorKind::ResourceLimit,
                path,
                "depth or element count",
            ));
        }
        self.reserve(es.len() as i64, 576)?;
        self.elements += es.len() as i64;
        let mut seen: HashSet<Tag> = HashSet::with_capacity(es.len());
        for e in es {
            let tag = e.tag();
            let p = join(path, tag);
            if !seen.insert(tag) {
                return Err(path_error(
                    ErrorKind::InvalidObject,
                    &p,
                    "duplicate attribute",
                ));
            }
            if bulk_tag(tag) {
                if !pixels {
                    continue;
                }
                if tag != ft(0x7fe00010) {
                    return Err(path_error(
                        ErrorKind::UnsupportedFeature,
                        &p,
                        "non-integer Pixel Data",
                    ));
                }
                let Value::Raw(raw) = &e.value else {
                    return Err(path_error(
                        ErrorKind::Unmaterialized,
                        &p,
                        "native Pixel Data is required",
                    ));
                };
                let n = raw.len() as i64;
                if n > self.limits.max_pixel_bytes - self.pixels {
                    return Err(path_error(ErrorKind::ResourceLimit, &p, "pixel bytes"));
                }
                self.reserve(n, 1)?;
                self.pixels += n;
                continue;
            }
            if let Value::Sequence(items) = &e.value {
                if e.vr() != Vr::SQ {
                    return Err(path_error(ErrorKind::InvalidObject, &p, "sequence VR"));
                }
                let n = items.len() as i64;
                if tag == ft(0x300a0360) && n > self.shifter_settings {
                    self.shifter_settings = n;
                }
                if n > self.limits.max_elements - self.items {
                    return Err(path_error(ErrorKind::ResourceLimit, &p, "item count"));
                }
                self.reserve(n, 192)?;
                self.items += n;
                match tag.key() {
                    0x300a00b0 | 0x300a03a2 | 0x30080020 | 0x30080021 => self.beams += n,
                    0x300a0314 | 0x300a0210 | 0x300a0230 | 0x300a0280 | 0x30080100 | 0x30080110
                    | 0x30080130 | 0x300800f2 | 0x30080220 | 0x30080240 | 0x30080171 => {
                        self.entities += n
                    }
                    0x300a0111 | 0x300a03a8 | 0x300a02d0 | 0x30080040 | 0x30080041 | 0x30080160
                    | 0x30080173 => {
                        self.reserve(n, 1024)?;
                        self.points += n;
                    }
                    _ => {}
                }
                if self.beams > self.limits.max_beams
                    || self.entities > self.limits.max_entities
                    || self.points > self.limits.max_control_points
                {
                    return Err(path_error(
                        ErrorKind::ResourceLimit,
                        &p,
                        "family item count",
                    ));
                }
                for (i, it) in items.iter().enumerate() {
                    self.visit(&it.elements, &format!("{p}[{i}]"), depth + 1, pixels)?;
                }
                continue;
            }
            if e.vr() == Vr::SQ {
                return Err(path_error(
                    ErrorKind::InvalidObject,
                    &p,
                    "SQ requires a sequence value",
                ));
            }
            let n = match &e.value {
                Value::Nil => {
                    if e.header.length_set && e.header.length == 0 {
                        continue;
                    }
                    return Err(path_error(
                        ErrorKind::Unmaterialized,
                        &p,
                        "nil metadata value",
                    ));
                }
                Value::Raw(raw) => raw.len() as i64,
                Value::Strings(values) => values.iter().map(|s| s.len() as i64 + 1).sum(),
                _ => {
                    return Err(path_error(
                        ErrorKind::Unmaterialized,
                        &p,
                        "source-backed or unsupported metadata value",
                    ));
                }
            };
            if n > self.limits.max_value_bytes || n > self.limits.max_metadata_bytes - self.bytes {
                return Err(path_error(ErrorKind::ResourceLimit, &p, "metadata bytes"));
            }
            self.reserve(n, 4)?;
            match &e.value {
                Value::Strings(values) => self.reserve(values.len() as i64, 48)?,
                Value::Raw(raw)
                    if matches!(
                        e.vr(),
                        Vr::DS
                            | Vr::IS
                            | Vr::CS
                            | Vr::LO
                            | Vr::SH
                            | Vr::UI
                            | Vr::PN
                            | Vr::DA
                            | Vr::TM
                            | Vr::DT
                    ) =>
                {
                    self.reserve(count_backslashes(raw) + 1, 48)?
                }
                _ => {}
            }
            if tag == tagLeafJawPositions {
                let count = match &e.value {
                    Value::Strings(values) => values.len() as i64,
                    Value::Raw(raw) => match e.vr() {
                        Vr::FL => raw.len() as i64 / 4,
                        Vr::FD => raw.len() as i64 / 8,
                        _ => count_backslashes(raw) + 1,
                    },
                    _ => 0,
                };
                if count * 8 > self.inherited_bytes {
                    self.inherited_bytes = count * 8;
                }
            }
            self.bytes += n;
            if n == 0 && e.header.length_set && e.header.length != 0 {
                return Err(path_error(
                    ErrorKind::Unmaterialized,
                    &p,
                    "nonzero backing with empty value",
                ));
            }
            if tag == ft(0x300a0394) {
                if n % 8 != 0 {
                    return Err(path_error(ErrorKind::InvalidObject, &p, "spot map length"));
                }
                let spots = n / 8;
                if spots > self.limits.max_spots - self.spots {
                    return Err(path_error(ErrorKind::ResourceLimit, &p, "spot count"));
                }
                self.reserve(spots, 24)?;
                self.spots += spots;
            }
        }
        Ok(())
    }
}
fn strip_bulk(input: &[Element]) -> Vec<Element> {
    let mut out = Vec::with_capacity(input.len());
    for e in input {
        if bulk_tag(e.tag()) {
            continue;
        }
        let mut e = e.clone();
        if let Value::Sequence(items) = &e.value {
            e.value = Value::Sequence(
                items
                    .iter()
                    .map(|it| DataSet {
                        elements: strip_bulk(&it.elements),
                        item_offset: None,
                    })
                    .collect(),
            );
        }
        out.push(e);
    }
    out
}
pub(crate) fn fresh_facade(obj: &Object, pixels: bool) -> Object {
    let elements = if pixels {
        obj.elements().to_vec()
    } else {
        strip_bulk(obj.elements())
    };
    let mut out = Object::from_elements(elements);
    out.set_value_big_endian(obj.value_big_endian());
    out
}
pub(crate) fn strict_object(obj: &Object, l: &Limits) -> Result<(), Error> {
    let (result, err) = validate_data_set_with(
        &DataSet {
            elements: obj.elements().to_vec(),
            item_offset: None,
        },
        &ValidationOptions {
            max_elements: l.max_elements,
            max_depth: l.max_depth,
            max_findings: l.max_problems,
            dictionary: true,
            big_endian: obj.value_big_endian(),
            ..ValidationOptions::default()
        },
    );
    if err.is_some() || result.report.has_errors() || result.report.truncated {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "VR/VM validation failed",
        ));
    }
    let (report, err) = iod::validate(
        obj,
        &iod::Options {
            check_values: true,
            max_findings: l.max_problems,
            ..iod::Options::default()
        },
    );
    if err.is_some() || !report.errors().is_empty() || report.truncated != 0 {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "IOD validation failed",
        ));
    }
    Ok(())
}
pub(crate) fn native_syntax(s: &Syntax) -> bool {
    s.uid == transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid
        || s.uid == transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid
        || s.uid == EXPLICIT_VR_BIG_ENDIAN_UID
}
pub(crate) const EXPLICIT_VR_BIG_ENDIAN_UID: &str = "1.2.840.10008.1.2.2";
pub fn read(file: &File) -> Result<Plan, Error> {
    read_with_options(file, ReadOptions::default())
}
pub fn read_dataset(obj: &Object) -> Result<Plan, Error> {
    read_dataset_with_options(obj, ReadOptions::default())
}
pub fn read_with_options(file: &File, opts: ReadOptions) -> Result<Plan, Error> {
    let adjusted = file_read_options(file, opts)?;
    let mut plan = read_dataset_with_options(&file.dataset, adjusted)?;
    let effective = opts.limits.normalized()?;
    plan.model_bytes += effective.max_model_bytes - adjusted.limits.max_model_bytes;
    Ok(plan)
}
pub fn read_dataset_with_options(obj: &Object, opts: ReadOptions) -> Result<Plan, Error> {
    let b = preflight(obj, &opts.limits, false)?;
    let obj = fresh_facade(obj, false);
    if opts.strict_iod {
        strict_object(&obj, &b.limits)?;
    }
    let sop = obj.get_uid(tagSOPClassUID).unwrap_or_default();
    if sop != RT_PLAN_STORAGE && sop != RT_ION_PLAN_STORAGE {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    let mut r = Reader::new(b.limits.max_problems);
    let mut plan = r.read_plan(&obj);
    plan.sop_class_uid = sop.clone();
    if sop == RT_ION_PLAN_STORAGE {
        read_ion(&mut r, &obj, &mut plan);
    }
    read_brachy(&mut r, &obj, &mut plan);
    validate_plan(&mut r, &plan);
    if !r.errors.is_empty() {
        return Err(Error::validation(r.errors));
    }
    plan.model_bytes = b.model;
    plan.warnings = r.warnings;
    Ok(plan)
}
pub(crate) fn file_read_options(f: &File, mut opts: ReadOptions) -> Result<ReadOptions, Error> {
    let l = opts.limits.normalized()?;
    let mut remaining = l;
    if native_syntax(&f.transfer_syntax)
        && f.dataset.value_big_endian() != f.transfer_syntax.big_endian
    {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "(0002,0010)",
            "file syntax and dataset byte order differ",
        ));
    }
    let a = preflight_prepared(&f.meta, l, false)?;
    validate_file_meta_groups(&f.meta)?;
    remaining.max_metadata_bytes -= a.bytes;
    remaining.max_elements -= a.elements;
    remaining.max_model_bytes -= a.model;
    if remaining.max_metadata_bytes <= 0
        || remaining.max_elements <= 0
        || remaining.max_model_bytes <= 0
    {
        return Err(Error::bare(ErrorKind::ResourceLimit));
    }
    preflight_prepared(&f.dataset, remaining, false)?;
    let meta = fresh_facade(&f.meta, false);
    let ds = fresh_facade(&f.dataset, false);
    for (m, d) in [
        (ft(0x00020002), tagSOPClassUID),
        (ft(0x00020003), tagSOPInstanceUID),
    ] {
        let mv = meta.get_uid(m).unwrap_or_default();
        let dv = ds.get_uid(d).unwrap_or_default();
        if mv.is_empty() || dv.is_empty() || mv != dv {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &m.to_string(),
                "file meta and dataset identity differ",
            ));
        }
    }
    let ts = meta.get_uid(ft(0x00020010)).unwrap_or_default();
    if !f.transfer_syntax.uid.is_empty() && ts != f.transfer_syntax.uid {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "(0002,0010)",
            "file transfer syntax identity differs",
        ));
    }
    opts.limits = remaining;
    Ok(opts)
}
pub(crate) fn validate_file_meta_groups(meta: &Object) -> Result<(), Error> {
    for e in meta.elements() {
        if e.tag().group != 0x0002 {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &e.tag().to_string(),
                "file meta contains a dataset attribute",
            ));
        }
    }
    Ok(())
}
pub(crate) fn inspection_limits(options: Option<Limits>) -> Result<Limits, Error> {
    match options {
        Some(l) => l.normalized(),
        None => Ok(default_limits()),
    }
}
