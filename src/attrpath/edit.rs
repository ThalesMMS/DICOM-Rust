use super::path::{Path, Segment};
use super::session::{Budget, SelectedMatch, validate_session_path};
use super::{Error, Kind};
use crate::dictionary::reservations::{private_tag, reserve_private_creator};
use crate::numtext;
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr, dict};
use crate::textfmt::quote;
use crate::validation;
pub const DEFAULT_MAX_CREATED_ITEMS: usize = 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub vr: Option<Vr>,
    pub max_created_items: usize,
    pub skip_value_validation: bool,
    pub private_catalog: bool,
}
#[derive(Clone, Debug)]
pub struct Match {
    pub path: Path,
    pub element: Option<Element>,
    pub item: Option<Object>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Find,
    Create,
    Insert,
    Modify,
    Remove,
}
impl Mode {
    fn creates(self) -> bool {
        matches!(self, Mode::Create | Mode::Insert)
    }
}
pub fn find(dataset: &Object, p: &Path) -> Result<Vec<Match>, Error> {
    let mut work = dataset.clone();
    let mut e = Editor::new(Mode::Find, &Options::default(), "");
    e.run(&mut work, p)?;
    Ok(e.matches)
}
pub fn create(dataset: &mut Object, p: &Path, opts: &Options) -> Result<usize, Error> {
    Editor::new(Mode::Create, opts, "").run(dataset, p)
}
pub fn insert(dataset: &mut Object, p: &Path, value: &str, opts: &Options) -> Result<usize, Error> {
    Editor::new(Mode::Insert, opts, value).run(dataset, p)
}
pub fn modify(dataset: &mut Object, p: &Path, value: &str, opts: &Options) -> Result<usize, Error> {
    Editor::new(Mode::Modify, opts, value).run(dataset, p)
}
pub fn remove(dataset: &mut Object, p: &Path) -> Result<usize, Error> {
    Editor::new(Mode::Remove, &Options::default(), "").run(dataset, p)
}
pub(crate) struct Editor<'s> {
    mode: Mode,
    opts: Options,
    value: String,
    created: usize,
    count: usize,
    matches: Vec<Match>,
    pub(crate) session: Option<&'s mut Budget>,
}
fn sequence_header(element: &Element) -> ElementHeader {
    let mut header = element.header;
    header.length = 0;
    header.length_set = false;
    header
}
impl<'s> Editor<'s> {
    pub(crate) fn new(mode: Mode, opts: &Options, value: &str) -> Editor<'s> {
        Editor {
            mode,
            opts: opts.clone(),
            value: value.to_string(),
            created: 0,
            count: 0,
            matches: Vec::new(),
            session: None,
        }
    }
    pub(crate) fn run(&mut self, dataset: &mut Object, p: &Path) -> Result<usize, Error> {
        if self.session.is_some() {
            validate_session_path(p)?;
        }
        if p.is_empty() {
            return Err(Error::new(Kind::NotFound, "empty path or data set"));
        }
        if self.opts.max_created_items == 0 {
            self.opts.max_created_items = DEFAULT_MAX_CREATED_ITEMS;
        }
        if p.selects_item()
            && (self.mode == Mode::Modify || (self.mode == Mode::Insert && !self.value.is_empty()))
        {
            return Err(Error::new(Kind::Value, "an item takes no value"));
        }
        self.apply(dataset, p, 0, &[])?;
        if self.count == 0 {
            return Err(Error::new(Kind::NotFound, p));
        }
        Ok(self.count)
    }
    fn resolve(&mut self, dataset: &mut Object, s: &Segment) -> Result<(Tag, bool), Error> {
        if !s.private() {
            return Ok((s.tag, false));
        }
        let offset = s.tag.element as u8;
        if let Ok(tag) = private_tag(dataset, s.tag.group, &s.creator, offset) {
            return Ok((tag, false));
        }
        if !self.mode.creates() {
            return Err(Error::new(
                Kind::NotFound,
                format!(
                    "private creator {} in group {:04X}",
                    quote(&s.creator),
                    s.tag.group
                ),
            ));
        }
        if let Some(b) = self.session.as_deref_mut() {
            b.reserve_model(1, 0)?;
            b.reserve_bytes(s.creator.len() as i64)?;
        }
        let block = reserve_private_creator(dataset, s.tag.group, &s.creator)?;
        Ok((
            Tag::new(s.tag.group, (block as u16) << 8 | offset as u16),
            true,
        ))
    }
    fn sess(&mut self) -> Option<&mut Budget> {
        self.session.as_deref_mut()
    }
    fn apply(
        &mut self,
        dataset: &mut Object,
        p: &Path,
        i: usize,
        prefix: &[Segment],
    ) -> Result<bool, Error> {
        if let Some(b) = self.sess() {
            b.visit()?;
        }
        let s = &p.0[i];
        let (tag, mut changed) = self.resolve(dataset, s)?;
        let mut concrete: Vec<Segment> = prefix.to_vec();
        concrete.push(s.clone());
        let last = i == p.len() - 1;
        if !s.has_item {
            let leaf_changed = self.leaf(dataset, tag, s, &Path(concrete))?;
            return Ok(changed || leaf_changed);
        }
        let existing = dataset.get(tag).cloned();
        let exists = existing.is_some();
        let element = match existing {
            Some(element) => {
                if !matches!(element.value, Value::Sequence(_)) {
                    return Err(Error::new(Kind::NotSequence, Path(concrete)));
                }
                element
            }
            None => {
                if !self.mode.creates() || s.any_item {
                    return Err(Error::new(Kind::NotFound, Path(concrete)));
                }
                if let Some(b) = self.sess() {
                    b.reserve_model(1, 0)?;
                }
                changed = true;
                Element {
                    header: ElementHeader {
                        tag,
                        vr: Vr::SQ,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Sequence(Vec::new()),
                }
            }
        };
        let mut items: Vec<Object> = if exists {
            dataset.get_sequence(tag).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (first, end) = if s.any_item {
            if items.is_empty() {
                return Err(Error::new(
                    Kind::NotFound,
                    format!("{} has no items", Path(concrete)),
                ));
            }
            (0, items.len())
        } else if s.item >= items.len() {
            if !self.mode.creates() {
                return Err(Error::new(Kind::NotFound, Path(concrete)));
            }
            let add = s.item + 1 - items.len();
            if self.created + add > self.opts.max_created_items {
                return Err(Error::new(
                    Kind::Limit,
                    format!("more than {} new items", self.opts.max_created_items),
                ));
            }
            if let Some(b) = self.sess() {
                b.reserve_model(0, add as i64)?;
            }
            self.created += add;
            for _ in 0..add {
                let mut item = Object::new();
                item.set_value_big_endian(dataset.value_big_endian());
                items.push(item);
            }
            changed = true;
            (s.item, s.item + 1)
        } else {
            (s.item, s.item + 1)
        };
        if self.session.is_some() && changed && self.mode != Mode::Find {
            let seq: Vec<DataSet> = items.iter().map(Object::to_data_set).collect();
            dataset.put(Element {
                header: sequence_header(&element),
                value: Value::Sequence(seq),
            });
            items = dataset.get_sequence(tag).unwrap_or_default();
        }
        let mut removed = vec![false; items.len()];
        let mut found = false;
        let mut first_err: Option<Error> = None;
        for index in first..end {
            if let Some(b) = self.sess() {
                b.visit()?;
            }
            let mut item_path: Vec<Segment> = prefix.to_vec();
            let mut seg = s.clone();
            seg.any_item = false;
            seg.item = index;
            item_path.push(seg);
            if last {
                found = true;
                self.count += 1;
                match self.mode {
                    Mode::Find => {
                        if let Some(b) = self.session.as_deref_mut() {
                            b.add_match(SelectedMatch {
                                path: Path(item_path),
                                element: None,
                                item: Some(items[index].clone()),
                                parent: dataset.clone(),
                            })?;
                        } else {
                            self.matches.push(Match {
                                path: Path(item_path),
                                element: None,
                                item: Some(Object::from_elements(items[index].elements().to_vec())),
                            });
                        }
                    }
                    Mode::Remove => {
                        if let Some(b) = self.sess() {
                            b.touch()?;
                        }
                        removed[index] = true;
                        changed = true;
                    }
                    _ => {}
                }
                continue;
            }
            match self.apply(&mut items[index], p, i + 1, &item_path) {
                Ok(item_changed) => {
                    found = true;
                    changed = changed || item_changed;
                }
                Err(err) if s.any_item && err.is(Kind::NotFound) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                }
                Err(err) => return Err(err),
            }
        }
        if !found && let Some(err) = first_err {
            return Err(err);
        }
        if changed && self.mode != Mode::Find {
            let sequence: Vec<DataSet> = items
                .iter()
                .enumerate()
                .filter(|(index, _)| !removed[*index])
                .map(|(_, item)| item.to_data_set())
                .collect();
            dataset.put(Element {
                header: sequence_header(&element),
                value: Value::Sequence(sequence),
            });
        }
        Ok(changed)
    }
    fn leaf(
        &mut self,
        dataset: &mut Object,
        tag: Tag,
        s: &Segment,
        path: &Path,
    ) -> Result<bool, Error> {
        let existing = dataset.get(tag).cloned();
        match self.mode {
            Mode::Find => {
                let Some(existing) = existing else {
                    return Err(Error::new(Kind::NotFound, path));
                };
                self.count += 1;
                if let Some(b) = self.session.as_deref_mut() {
                    b.add_match(SelectedMatch {
                        path: path.clone(),
                        element: Some(existing),
                        item: None,
                        parent: dataset.clone(),
                    })?;
                } else {
                    self.matches.push(Match {
                        path: path.clone(),
                        element: Some(existing),
                        item: None,
                    });
                }
                return Ok(false);
            }
            Mode::Remove => {
                if existing.is_none() {
                    return Err(Error::new(Kind::NotFound, path));
                }
                if let Some(b) = self.sess() {
                    b.touch()?;
                }
                dataset.remove(tag);
                self.count += 1;
                return Ok(true);
            }
            Mode::Create => {
                self.count += 1;
                if existing.is_some() {
                    return Ok(false);
                }
                if let Some(b) = self.sess() {
                    b.reserve_model(1, 0)?;
                    b.touch()?;
                }
                let element = self.element(dataset, tag, s, None, "")?;
                dataset.put(element);
                return Ok(true);
            }
            Mode::Modify => {
                if existing.is_none() {
                    return Err(Error::new(Kind::NotFound, path));
                }
            }
            Mode::Insert => {}
        }
        if let Some(b) = self.sess() {
            if existing.is_none() {
                b.reserve_model(1, 0)?;
            }
            b.touch()?;
        }
        let value = self.value.clone();
        let element = self.element(dataset, tag, s, existing.as_ref(), &value)?;
        dataset.put(element);
        self.count += 1;
        Ok(true)
    }
    fn element(
        &mut self,
        dataset: &Object,
        tag: Tag,
        s: &Segment,
        existing: Option<&Element>,
        value: &str,
    ) -> Result<Element, Error> {
        let mut vr = self.opts.vr;
        if vr.is_none()
            && let Some(e) = existing
            && e.header.vr != Vr::UN
        {
            vr = Some(e.header.vr);
        }
        if vr.is_none()
            && self.opts.private_catalog
            && s.private()
            && let Some(entry) = crate::dictionary::curated::Catalog::new().by_private(
                &s.creator,
                tag.group,
                tag.element as u8,
            )
        {
            vr = Some(Vr::parse(entry.vr.as_bytes()).unwrap_or(Vr::UN));
        }
        if vr.is_none() {
            vr = dict::by_tag(tag).map(|e| e.vr);
        }
        let vr = match vr {
            Some(vr) if vr != Vr::UN => vr,
            _ => {
                return Err(Error::new(
                    Kind::UnknownVr,
                    format!("{tag} (set Options.VR)"),
                ));
            }
        };
        if let Some(b) = self.sess() {
            b.reserve_text(vr, value)?;
            if tag == Tag::new(0x0008, 0x0005) {
                b.reserve_condition_text_value(Vr::CS, &Value::Strings(vec![value.to_string()]))?;
            }
        }
        let element = element_from_string(tag, vr, value, dataset.value_big_endian())?;
        if !self.opts.skip_value_validation && vr != Vr::SQ && !value.is_empty() {
            let report = validation::validate_element(&element, validation::Options::default());
            if let Some(code) = first_error(&report) {
                return Err(Error::new(Kind::Value, format!("{tag}: {code}")));
            }
        }
        Ok(element)
    }
}
pub(crate) fn first_error(report: &validation::Report) -> Option<validation::Code> {
    report
        .findings
        .iter()
        .find(|f| {
            f.severity == validation::Severity::Error
                && f.code != validation::CODE_VALUE_MULTIPLICITY
        })
        .map(|f| f.code)
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
fn parse_unsigned(s: &str, bits: u32) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u128 = s.parse().ok()?;
    if bits < 64 && v >= 1u128 << bits || v > u64::MAX as u128 {
        return None;
    }
    Some(v as u64)
}
fn parse_signed(s: &str, bits: u32) -> Option<i64> {
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'+') => (false, &s[1..]),
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i128 = digits.parse().ok()?;
    let v = if neg { -v } else { v };
    let max = (1i128 << (bits - 1)) - 1;
    let min = -(1i128 << (bits - 1));
    if v < min || v > max {
        return None;
    }
    Some(v as i64)
}
fn parse_float32(s: &str) -> Option<f32> {
    let (f64v, err) = numtext::parse_float_value(s);
    if err.is_some() {
        return None;
    }
    let decimal: String = s.chars().filter(|&c| c != '_').collect();
    let lower = decimal.to_ascii_lowercase();
    let f = if lower.contains("0x") || lower.contains("inf") || lower.contains("nan") {
        f64v as f32
    } else {
        decimal.parse::<f32>().ok()?
    };
    Some(f)
}
fn put_u(out: &mut Vec<u8>, v: u64, width: usize, big_endian: bool) {
    let bytes = v.to_le_bytes();
    if big_endian {
        out.extend(bytes[..width].iter().rev());
    } else {
        out.extend(&bytes[..width]);
    }
}
pub fn element_from_string(
    tag: Tag,
    vr: Vr,
    text: &str,
    big_endian: bool,
) -> Result<Element, Error> {
    let header = ElementHeader {
        tag,
        vr,
        length: 0,
        length_set: false,
    };
    if vr.is_string_like() {
        let values = if text.is_empty() {
            Vec::new()
        } else if vr.uses_text_value_delimiter() {
            text.split('\\').map(str::to_string).collect()
        } else {
            vec![text.to_string()]
        };
        return Ok(Element {
            header,
            value: Value::Strings(values),
        });
    }
    let parts: Vec<&str> = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\\').collect()
    };
    let fail = |i: usize| Error::new(Kind::Value, format!("{tag} value {} for VR {vr}", i + 1));
    let mut raw = Vec::new();
    match vr {
        Vr::SQ => {
            if !text.is_empty() {
                return Err(Error::new(Kind::Value, "a sequence takes no value"));
            }
            return Ok(Element {
                header,
                value: Value::Sequence(Vec::new()),
            });
        }
        Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::UV | Vr::SV => {
            let bits = match vr {
                Vr::US | Vr::SS => 16,
                Vr::UL | Vr::SL => 32,
                _ => 64,
            };
            let signed = matches!(vr, Vr::SS | Vr::SL | Vr::SV);
            for (i, part) in parts.iter().enumerate() {
                let part = trim_unicode_space(part);
                let v = if signed {
                    parse_signed(part, bits).map(|v| v as u64)
                } else {
                    parse_unsigned(part, bits)
                };
                let v = v.ok_or_else(|| fail(i))?;
                put_u(&mut raw, v, (bits / 8) as usize, big_endian);
            }
        }
        Vr::FL | Vr::FD => {
            for (i, part) in parts.iter().enumerate() {
                let part = trim_unicode_space(part);
                if vr == Vr::FD {
                    let v = numtext::parse_float(part).map_err(|_| fail(i))?;
                    if !v.is_finite() {
                        return Err(fail(i));
                    }
                    put_u(&mut raw, v.to_bits(), 8, big_endian);
                } else {
                    let v = parse_float32(part).ok_or_else(|| fail(i))?;
                    if !v.is_finite() {
                        return Err(fail(i));
                    }
                    put_u(&mut raw, v.to_bits() as u64, 4, big_endian);
                }
            }
        }
        Vr::AT => {
            for (i, part) in parts.iter().enumerate() {
                let mut part = trim_unicode_space(part).to_string();
                if part.starts_with('(') && part.ends_with(')') && part.len() >= 2 {
                    part = part[1..part.len() - 1].replace(',', "");
                }
                if part.len() != 8 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(fail(i));
                }
                let n = u32::from_str_radix(&part, 16).map_err(|_| fail(i))?;
                put_u(&mut raw, (n >> 16) as u64, 2, big_endian);
                put_u(&mut raw, (n & 0xFFFF) as u64, 2, big_endian);
            }
        }
        _ => {
            if !text.is_empty() {
                return Err(Error::new(
                    Kind::Value,
                    format!("VR {vr} takes binary data, not text"),
                ));
            }
        }
    }
    Ok(Element {
        header,
        value: Value::Raw(raw),
    })
}
