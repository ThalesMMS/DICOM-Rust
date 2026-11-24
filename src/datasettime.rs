#![forbid(unsafe_code)]
use crate::dcmtime::{self, PrecisionLevel, ZonedTime};
use crate::object::text::{trim_text_value, trim_text_value_bytes};
use crate::object::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use std::collections::{HashMap, HashSet};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    InvalidOffset,
    InvalidValue,
    Ambiguous,
    MissingOffset,
    Unsupported,
    LimitExceeded,
}
impl ErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::InvalidOffset => "dicom datasettime: invalid UTC offset",
            ErrorKind::InvalidValue => "dicom datasettime: invalid temporal value",
            ErrorKind::Ambiguous => "dicom datasettime: ambiguous or unrepresentable conversion",
            ErrorKind::MissingOffset => "dicom datasettime: source UTC offset is undefined",
            ErrorKind::Unsupported => "dicom datasettime: unavailable temporal or sequence value",
            ErrorKind::LimitExceeded => "dicom datasettime: structural copy limit exceeded",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}
impl Error {
    fn new(kind: ErrorKind) -> Error {
        Error {
            kind,
            message: kind.as_str().to_string(),
        }
    }
    fn detail(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.as_str()),
        }
    }
    fn prefixed(self, prefix: impl fmt::Display) -> Error {
        Error {
            kind: self.kind,
            message: format!("{prefix}: {}", self.message),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pair {
    pub date: Tag,
    pub time: Tag,
}
const fn pair(group: u16, date: u16, clock: u16) -> Pair {
    Pair {
        date: Tag::new(group, date),
        time: Tag::new(group, clock),
    }
}
const STANDARD_PAIRS: [Pair; 46] = [
    pair(0x0008, 0x0012, 0x0013),
    pair(0x0008, 0x0020, 0x0030),
    pair(0x0008, 0x0021, 0x0031),
    pair(0x0008, 0x0022, 0x0032),
    pair(0x0008, 0x0023, 0x0033),
    pair(0x0008, 0x0024, 0x0034),
    pair(0x0008, 0x0025, 0x0035),
    pair(0x0010, 0x0030, 0x0032),
    pair(0x0014, 0x0102, 0x0103),
    pair(0x0014, 0x3076, 0x3077),
    pair(0x0014, 0x407E, 0x407C),
    pair(0x0018, 0x1012, 0x1014),
    pair(0x0018, 0x1200, 0x1201),
    pair(0x0018, 0x700C, 0x700E),
    pair(0x0020, 0x3403, 0x3405),
    pair(0x0032, 0x0032, 0x0033),
    pair(0x0032, 0x0034, 0x0035),
    pair(0x0032, 0x1000, 0x1001),
    pair(0x0032, 0x1010, 0x1011),
    pair(0x0032, 0x1040, 0x1041),
    pair(0x0032, 0x1050, 0x1051),
    pair(0x0038, 0x001A, 0x001B),
    pair(0x0038, 0x001C, 0x001D),
    pair(0x0038, 0x0020, 0x0021),
    pair(0x0038, 0x0030, 0x0032),
    pair(0x0040, 0x0002, 0x0003),
    pair(0x0040, 0x0004, 0x0005),
    pair(0x0040, 0x0244, 0x0245),
    pair(0x0040, 0x0250, 0x0251),
    pair(0x0040, 0x2004, 0x2005),
    pair(0x0040, 0xA023, 0xA024),
    pair(0x0040, 0xA110, 0xA112),
    pair(0x0040, 0xA192, 0xA193),
    pair(0x0070, 0x0082, 0x0083),
    pair(0x2100, 0x0040, 0x0050),
    pair(0x3006, 0x0008, 0x0009),
    pair(0x3008, 0x0024, 0x0025),
    pair(0x3008, 0x0162, 0x0164),
    pair(0x3008, 0x0166, 0x0168),
    pair(0x3008, 0x0250, 0x0251),
    pair(0x300A, 0x0006, 0x0007),
    pair(0x300A, 0x022C, 0x022E),
    pair(0x300E, 0x0004, 0x0005),
    pair(0x4008, 0x0100, 0x0101),
    pair(0x4008, 0x0108, 0x0109),
    pair(0x4008, 0x0112, 0x0113),
];
pub fn standard_pairs() -> Vec<Pair> {
    STANDARD_PAIRS.to_vec()
}
const TIMEZONE_TAG: Tag = Tag::new(0x0008, 0x0201);
const CONTEXT_VERSION_TAG: Tag = Tag::new(0x0008, 0x0106);
const MAPPING_RESOURCE_TAG: Tag = Tag::new(0x0008, 0x0105);
const MAPPING_RESOURCE_UID_TAG: Tag = Tag::new(0x0008, 0x0118);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnpairedDatePolicy(pub u8);
impl UnpairedDatePolicy {
    pub const REJECT_AMBIGUOUS: UnpairedDatePolicy = UnpairedDatePolicy(0);
    pub const PRESERVE_CALENDAR_DATE: UnpairedDatePolicy = UnpairedDatePolicy(1);
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub source_offset: String,
    pub unpaired_dates: UnpairedDatePolicy,
    pub pairs: Vec<Pair>,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservedDate {
    pub item_path: String,
    pub tag: Tag,
    pub component: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub preserved_dates: Vec<PreservedDate>,
}
#[derive(Clone, Debug)]
struct Offset {
    text: String,
    seconds: i64,
}
pub fn convert(src: &DataSet, target_offset: &str, opts: &Options) -> Result<DataSet, Error> {
    convert_with_report(src, target_offset, opts).map(|(out, _)| out)
}
pub fn convert_with_report(
    src: &DataSet,
    target_offset: &str,
    opts: &Options,
) -> Result<(DataSet, Report), Error> {
    let target = parse_offset(target_offset)?;
    let fallback = if opts.source_offset.is_empty() {
        None
    } else {
        Some(parse_offset(&opts.source_offset)?)
    };
    let mut depth = opts.max_sequence_depth;
    if depth == 0 {
        depth = 64;
    }
    if depth < 0 {
        return Err(Error::detail(
            ErrorKind::LimitExceeded,
            "negative sequence depth",
        ));
    }
    let (mut elements, mut items) = (opts.max_elements, opts.max_items);
    if elements == 0 {
        elements = 100_000;
    }
    if items == 0 {
        items = 100_000;
    }
    if elements < 0 || items < 0 {
        return Err(Error::detail(
            ErrorKind::LimitExceeded,
            "negative elements/items budget",
        ));
    }
    if opts.pairs.len() as u64 > elements as u64 {
        return Err(Error::detail(
            ErrorKind::LimitExceeded,
            "additional pair count",
        ));
    }
    if opts.unpaired_dates > UnpairedDatePolicy::PRESERVE_CALENDAR_DATE {
        return Err(Error::detail(
            ErrorKind::Ambiguous,
            "unknown unpaired date policy",
        ));
    }
    let mut pairs = standard_pairs();
    pairs.extend_from_slice(&opts.pairs);
    let mut seen = HashSet::with_capacity(pairs.len() * 2);
    for p in &pairs {
        if p.date == p.time
            || seen.contains(&p.date)
            || seen.contains(&p.time)
            || p.date == TIMEZONE_TAG
            || p.time == TIMEZONE_TAG
        {
            return Err(Error::detail(
                ErrorKind::Ambiguous,
                format_args!("overlapping DA/TM pair {}/{}", p.date, p.time),
            ));
        }
        seen.insert(p.date);
        seen.insert(p.time);
    }
    let mut c = Converter {
        target,
        pairs,
        pair_tags: seen,
        max_depth: depth,
        unpaired_dates: opts.unpaired_dates,
        report: Report::default(),
        elements_remaining: elements,
        items_remaining: items,
    };
    let out = c.convert(src, fallback.as_ref(), 0, "dataset")?;
    Ok((out, c.report))
}
fn parse_offset(s: &str) -> Result<Offset, Error> {
    let s = s.trim_end_matches(' ');
    let b = s.as_bytes();
    if b.len() != 5 || (b[0] != b'+' && b[0] != b'-') {
        return Err(Error::detail(ErrorKind::InvalidOffset, "expected &ZZXX"));
    }
    if !b[1..].iter().all(u8::is_ascii_digit) {
        return Err(Error::new(ErrorKind::InvalidOffset));
    }
    let digit = |i: usize| i64::from(b[i] - b'0');
    let hours = digit(1) * 10 + digit(2);
    let minutes = digit(3) * 10 + digit(4);
    if minutes > 59 {
        return Err(Error::new(ErrorKind::InvalidOffset));
    }
    let mut n = hours * 3600 + minutes * 60;
    if b[0] == b'-' {
        if n == 0 || n > 12 * 3600 {
            return Err(Error::new(ErrorKind::InvalidOffset));
        }
        n = -n;
    } else if n > 14 * 3600 {
        return Err(Error::new(ErrorKind::InvalidOffset));
    }
    Ok(Offset {
        text: s.to_string(),
        seconds: n,
    })
}
struct Converter {
    target: Offset,
    pairs: Vec<Pair>,
    pair_tags: HashSet<Tag>,
    max_depth: i64,
    unpaired_dates: UnpairedDatePolicy,
    report: Report,
    elements_remaining: i64,
    items_remaining: i64,
}
fn fail(path: &str, tag: Tag, err: Error) -> Error {
    err.prefixed(format_args!("{path}/{tag}"))
}
impl Converter {
    fn convert(
        &mut self,
        src: &DataSet,
        inherited: Option<&Offset>,
        depth: i64,
        path: &str,
    ) -> Result<DataSet, Error> {
        if depth > self.max_depth {
            return Err(Error::detail(
                ErrorKind::LimitExceeded,
                format_args!("{path} exceeds sequence depth"),
            ));
        }
        let count = src.elements.len() as i64;
        if count > self.elements_remaining {
            return Err(Error::detail(
                ErrorKind::LimitExceeded,
                format_args!("{path} element visits"),
            ));
        }
        self.elements_remaining -= count;
        if depth == 0 && !src.elements.iter().any(|e| e.tag() == TIMEZONE_TAG) {
            if self.elements_remaining == 0 {
                return Err(Error::detail(
                    ErrorKind::LimitExceeded,
                    "root offset element",
                ));
            }
            self.elements_remaining -= 1;
        }
        let mut indices: HashMap<Tag, usize> = HashMap::with_capacity(src.elements.len());
        for (i, e) in src.elements.iter().enumerate() {
            if let Some(&old) = indices.get(&e.tag()) {
                let previous = &src.elements[old];
                if temporal(e.vr())
                    || temporal(previous.vr())
                    || self.pair_tags.contains(&e.tag())
                    || e.tag() == TIMEZONE_TAG
                    || e.tag() == MAPPING_RESOURCE_TAG
                    || e.tag() == MAPPING_RESOURCE_UID_TAG
                {
                    return Err(fail(
                        path,
                        e.tag(),
                        Error::detail(ErrorKind::Ambiguous, "duplicate context/temporal tag"),
                    ));
                }
            }
            indices.insert(e.tag(), i);
        }
        let mut source: Option<Offset> = inherited.cloned();
        let zone_index = indices.get(&TIMEZONE_TAG).copied();
        if let Some(zone_index) = zone_index {
            let e = &src.elements[zone_index];
            if e.vr() != Vr::SH {
                return Err(fail(
                    path,
                    TIMEZONE_TAG,
                    Error::new(ErrorKind::InvalidValue),
                ));
            }
            let values = text_values(e).map_err(|err| fail(path, TIMEZONE_TAG, err))?;
            if values.len() > 1 {
                return Err(fail(
                    path,
                    TIMEZONE_TAG,
                    Error::new(ErrorKind::InvalidOffset),
                ));
            }
            if values.len() == 1 && !values[0].is_empty() {
                let value =
                    parse_offset(&values[0]).map_err(|err| fail(path, TIMEZONE_TAG, err))?;
                source = Some(value);
            }
        }
        let source = source.as_ref();
        let mut out = DataSet {
            elements: src.elements.clone(),
            item_offset: src.item_offset,
        };
        let mut handled = vec![false; src.elements.len()];
        for pi in 0..self.pairs.len() {
            let p = self.pairs[pi];
            let (Some(&di), Some(&ti)) = (indices.get(&p.date), indices.get(&p.time)) else {
                continue;
            };
            let (d, t) = (&src.elements[di], &src.elements[ti]);
            if d.vr() != Vr::DA || t.vr() != Vr::TM {
                return Err(fail(
                    path,
                    p.date,
                    Error::detail(ErrorKind::InvalidValue, "DA/TM pair VR"),
                ));
            }
            let dates = text_values(d).map_err(|err| fail(path, p.date, err))?;
            let clocks = text_values(t).map_err(|err| fail(path, p.time, err))?;
            if empty_values(&dates) && empty_values(&clocks) {
                handled[di] = true;
                handled[ti] = true;
                continue;
            }
            if self.unpaired_dates == UnpairedDatePolicy::PRESERVE_CALENDAR_DATE
                && empty_values(&clocks)
            {
                for (i, date) in dates.iter().enumerate() {
                    if !date.is_empty() {
                        self.preserve_date(date, source, path, p.date, i)
                            .map_err(|err| fail(path, p.date, err))?;
                    }
                }
                handled[di] = true;
                handled[ti] = true;
                continue;
            }
            if dates.len() != clocks.len() {
                return Err(fail(
                    path,
                    p.date,
                    Error::detail(ErrorKind::Ambiguous, "pair multiplicity differs"),
                ));
            }
            let (mut new_dates, mut new_clocks) = (dates.clone(), clocks.clone());
            for i in 0..dates.len() {
                if dates[i].is_empty() && clocks[i].is_empty() {
                    continue;
                }
                if dates[i].is_empty() || clocks[i].is_empty() {
                    if !dates[i].is_empty()
                        && self.unpaired_dates == UnpairedDatePolicy::PRESERVE_CALENDAR_DATE
                    {
                        self.preserve_date(&dates[i], source, path, p.date, i)
                            .map_err(|err| fail(path, p.date, err))?;
                        continue;
                    }
                    return Err(fail(
                        path,
                        p.date,
                        Error::detail(
                            ErrorKind::Ambiguous,
                            format_args!("incomplete pair component {i}"),
                        ),
                    ));
                }
                let (date, clock) =
                    self.shift_pair(&dates[i], &clocks[i], source)
                        .map_err(|err| {
                            fail(path, p.date, err.prefixed(format_args!("component {i}")))
                        })?;
                new_dates[i] = date;
                new_clocks[i] = clock;
            }
            replace_text(&mut out.elements[di], &dates, new_dates);
            replace_text(&mut out.elements[ti], &clocks, new_clocks);
            handled[di] = true;
            handled[ti] = true;
        }
        for (i, e) in src.elements.iter().enumerate() {
            if handled[i] {
                continue;
            }
            let items = match &e.value {
                Value::Sequence(items) => Some(items),
                _ => None,
            };
            if items.is_some() || e.vr() == Vr::SQ {
                if e.vr() != Vr::SQ && e.vr() != Vr::UN {
                    return Err(fail(path, e.tag(), Error::new(ErrorKind::InvalidValue)));
                }
                let Some(items) = items else {
                    return Err(fail(path, e.tag(), Error::new(ErrorKind::Unsupported)));
                };
                if items.len() as i64 > self.items_remaining {
                    return Err(fail(
                        path,
                        e.tag(),
                        Error::detail(ErrorKind::LimitExceeded, "sequence item copies"),
                    ));
                }
                self.items_remaining -= items.len() as i64;
                let mut new_items = Vec::with_capacity(items.len());
                for (j, item) in items.iter().enumerate() {
                    let item_path = format!("{path}/{}[{j}]", e.tag());
                    new_items.push(self.convert(item, source, depth + 1, &item_path)?);
                }
                let target = &mut out.elements[i];
                target.value = Value::Sequence(new_items);
                target.header.length = 0;
                target.header.length_set = false;
                continue;
            }
            if !temporal(e.vr()) {
                continue;
            }
            let values = text_values(e).map_err(|err| fail(path, e.tag(), err))?;
            let mut converted = values.clone();
            for (j, value) in values.iter().enumerate() {
                if value.is_empty() {
                    continue;
                }
                if e.tag() == CONTEXT_VERSION_TAG && e.vr() == Vr::DT {
                    let exempt = context_version_exempt(src, &indices, value)
                        .map_err(|err| fail(path, e.tag(), err))?;
                    if exempt {
                        continue;
                    }
                }
                let result = match e.vr() {
                    Vr::DT => self.shift_dt(value, source).map(|v| converted[j] = v),
                    Vr::TM => self.shift_clock(value, source).map(|v| converted[j] = v),
                    _ => self.preserve_date(value, source, path, e.tag(), j),
                };
                result.map_err(|err| {
                    fail(path, e.tag(), err.prefixed(format_args!("component {j}")))
                })?;
            }
            replace_text(&mut out.elements[i], &values, converted);
        }
        if let Some(zone_index) = zone_index {
            changed_text(
                &mut out.elements[zone_index],
                vec![self.target.text.clone()],
            );
        } else if depth == 0 {
            out.elements.push(Element {
                header: ElementHeader {
                    tag: TIMEZONE_TAG,
                    vr: Vr::SH,
                    length: 0,
                    length_set: false,
                },
                value: Value::Strings(vec![self.target.text.clone()]),
            });
        }
        Ok(out)
    }
    fn preserve_date(
        &mut self,
        value: &str,
        source: Option<&Offset>,
        path: &str,
        tag: Tag,
        component: usize,
    ) -> Result<(), Error> {
        if dcmtime::parse_date(value).is_err() || value.len() != 8 {
            return Err(Error::new(ErrorKind::InvalidValue));
        }
        if source.is_some_and(|s| s.seconds == self.target.seconds) {
            return Ok(());
        }
        if self.unpaired_dates == UnpairedDatePolicy::PRESERVE_CALENDAR_DATE {
            self.report.preserved_dates.push(PreservedDate {
                item_path: path.to_string(),
                tag,
                component,
            });
            return Ok(());
        }
        if source.is_none() {
            return Err(Error::new(ErrorKind::MissingOffset));
        }
        Err(Error::detail(ErrorKind::Ambiguous, "DA without paired TM"))
    }
    fn delta(&self, source: Option<&Offset>) -> Result<i64, Error> {
        match source {
            Some(source) => Ok(self.target.seconds - source.seconds),
            None => Err(Error::new(ErrorKind::MissingOffset)),
        }
    }
    fn shift_pair(
        &self,
        date: &str,
        clock: &str,
        source: Option<&Offset>,
    ) -> Result<(String, String), Error> {
        let invalid = || Error::new(ErrorKind::InvalidValue);
        let da = dcmtime::parse_date(date).map_err(|_| invalid())?;
        if date.len() != 8 {
            return Err(invalid());
        }
        let tm = dcmtime::parse_time(clock).map_err(|_| invalid())?;
        let delta = self.delta(source)?;
        shift_allowed(tm.precision, delta, leap_second(clock, false))?;
        if delta == 0 {
            return Ok((date.to_string(), clock.to_string()));
        }
        let value = ZonedTime::from_fields(
            da.time.year(),
            da.time.month(),
            da.time.day(),
            tm.time.hour(),
            tm.time.minute(),
            tm.time.second(),
            tm.time.nanosecond(),
            0,
        )
        .add_seconds(delta);
        if value.year() < 0 || value.year() > 9999 {
            return Err(Error::detail(
                ErrorKind::Ambiguous,
                "date outside four-digit year",
            ));
        }
        let new_date = dcmtime::Date {
            time: value,
            precision: PrecisionLevel::FULL,
            is_nema: false,
        };
        let new_clock = dcmtime::Time {
            time: value,
            precision: tm.precision,
        };
        Ok((new_date.dcm(), new_clock.dcm()))
    }
    fn shift_clock(&self, value: &str, source: Option<&Offset>) -> Result<String, Error> {
        let mut tm = dcmtime::parse_time(value).map_err(|_| Error::new(ErrorKind::InvalidValue))?;
        let delta = self.delta(source)?;
        shift_allowed(tm.precision, delta, leap_second(value, false))?;
        if delta == 0 {
            return Ok(value.to_string());
        }
        tm.time = tm.time.add_seconds(delta);
        Ok(tm.dcm())
    }
    fn shift_dt(&self, value: &str, source: Option<&Offset>) -> Result<String, Error> {
        let mut dt =
            dcmtime::parse_datetime(value).map_err(|_| Error::new(ErrorKind::InvalidValue))?;
        if !dt.no_offset {
            return Ok(value.to_string());
        }
        let delta = self.delta(source)?;
        shift_allowed(dt.precision, delta, leap_second(value, true))?;
        if delta == 0 {
            return Ok(value.to_string());
        }
        dt.time = dt.time.add_seconds(delta);
        if dt.time.year() < 0 || dt.time.year() > 9999 {
            return Err(Error::detail(
                ErrorKind::Ambiguous,
                "date outside four-digit year",
            ));
        }
        Ok(dt.dcm())
    }
}
fn temporal(vr: Vr) -> bool {
    matches!(vr, Vr::DA | Vr::TM | Vr::DT)
}
fn text_values(e: &Element) -> Result<Vec<String>, Error> {
    let vr = e.vr();
    match &e.value {
        Value::Raw(raw) => {
            let parts: Vec<&[u8]> = raw.split(|&b| b == b'\\').collect();
            if parts.len() == 1 && trim_text_value_bytes(vr, parts[0]).is_empty() {
                return Ok(Vec::new());
            }
            Ok(parts
                .iter()
                .map(|p| bytes_text(trim_text_value_bytes(vr, p)))
                .collect())
        }
        Value::Strings(values) => Ok(values
            .iter()
            .map(|v| trim_text_value(vr, v).to_string())
            .collect()),
        _ => Err(Error::new(ErrorKind::Unsupported)),
    }
}
fn bytes_text(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    for chunk in raw.utf8_chunks() {
        out.push_str(chunk.valid());
        out.extend(chunk.invalid().iter().map(|_| '\u{7f}'));
    }
    out
}
fn empty_values(values: &[String]) -> bool {
    values.iter().all(String::is_empty)
}
fn replace_text(e: &mut Element, before: &[String], after: Vec<String>) {
    if before != after.as_slice() {
        changed_text(e, after);
    }
}
fn changed_text(e: &mut Element, values: Vec<String>) {
    e.value = Value::Strings(values);
    e.header.length = 0;
    e.header.length_set = false;
}
fn shift_allowed(precision: PrecisionLevel, delta: i64, leap: bool) -> Result<(), Error> {
    if delta == 0 {
        return Ok(());
    }
    if leap {
        return Err(Error::detail(
            ErrorKind::Ambiguous,
            "leap-second arithmetic",
        ));
    }
    if precision >= PrecisionLevel::DAY {
        return Err(Error::detail(ErrorKind::Ambiguous, "date-only precision"));
    }
    if precision == PrecisionLevel::HOURS && delta % 3600 != 0 {
        return Err(Error::detail(
            ErrorKind::Ambiguous,
            "hour precision with fractional-hour shift",
        ));
    }
    Ok(())
}
fn leap_second(value: &str, dt: bool) -> bool {
    let start = if dt { 12 } else { 4 };
    value.as_bytes().get(start..start + 2) == Some(b"60")
}
fn context_version_exempt(
    ds: &DataSet,
    indices: &HashMap<Tag, usize>,
    value: &str,
) -> Result<bool, Error> {
    let day_len = if value.len() == 13 && value.ends_with("+0000") {
        8
    } else {
        value.len()
    };
    if day_len != 8 {
        return Ok(false);
    }
    if dcmtime::parse_datetime(value).is_err() {
        return Err(Error::new(ErrorKind::InvalidValue));
    }
    let (mut resource, mut uid) = (String::new(), String::new());
    for (tag, vr) in [
        (MAPPING_RESOURCE_TAG, Vr::CS),
        (MAPPING_RESOURCE_UID_TAG, Vr::UI),
    ] {
        let Some(&i) = indices.get(&tag) else {
            continue;
        };
        if ds.elements[i].vr() != vr {
            return Err(Error::new(ErrorKind::InvalidValue));
        }
        let values = text_values(&ds.elements[i])?;
        if values.len() > 1 {
            return Err(Error::new(ErrorKind::Ambiguous));
        }
        if let Some(first) = values.into_iter().next() {
            if tag == MAPPING_RESOURCE_TAG {
                resource = first.trim_matches(' ').to_string();
            } else {
                uid = first;
            }
        }
    }
    const DCMR_UID: &str = "1.2.840.10008.8.1.1";
    if resource == "DCMR" && !uid.is_empty() && uid != DCMR_UID
        || uid == DCMR_UID && !resource.is_empty() && resource != "DCMR"
    {
        return Err(Error::detail(
            ErrorKind::Ambiguous,
            "conflicting mapping resource",
        ));
    }
    Ok(resource == "DCMR" || uid == DCMR_UID)
}
