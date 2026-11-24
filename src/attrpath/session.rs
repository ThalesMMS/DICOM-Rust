use super::edit::{Editor, Mode, Options, element_from_string, first_error};
use super::path::{MAX_ITEM_INDEX, MAX_PATH_LENGTH, MAX_SEGMENTS, Path, Segment, parse};
use super::script::{Edit, Operation};
use super::{Error, Kind};
use crate::charset::SpecificCharacterSet;
use crate::dictionary;
use crate::object::text::{split_text_multiplicity, trim_text_value, trim_text_value_bytes};
use crate::object::{Element, Object, Tag, UNDEFINED_LENGTH, Value, Vr, WalkOptions};
use crate::{validation, valueencode};
use std::cmp::Ordering;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditLimits {
    pub max_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_visits: i64,
    pub max_matches: i64,
    pub max_path_bytes: i64,
    pub max_touches: i64,
    pub max_created_items: i64,
    pub max_value_bytes: i64,
}
impl EditLimits {
    pub fn defaults() -> EditLimits {
        EditLimits {
            max_depth: 64,
            max_elements: 100_000,
            max_items: 100_000,
            max_visits: 2_000_000,
            max_matches: 8192,
            max_path_bytes: 8 << 20,
            max_touches: 100_000,
            max_created_items: 8192,
            max_value_bytes: 128 << 20,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct SelectedMatch {
    pub path: Path,
    pub element: Option<Element>,
    pub item: Option<Object>,
    pub parent: Object,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Budget {
    limits: EditLimits,
    visits: i64,
    touches: i64,
    created: i64,
    path_bytes: i64,
    elements: i64,
    items: i64,
    value_bytes: i64,
    charset_admission: bool,
    pub(crate) matches: Vec<SelectedMatch>,
}
fn limit() -> Error {
    Error::bare(Kind::Limit)
}
fn invalid() -> Error {
    Error::bare(Kind::Value)
}
impl Budget {
    pub(crate) fn visit(&mut self) -> Result<(), Error> {
        if self.visits >= self.limits.max_visits {
            return Err(limit());
        }
        self.visits += 1;
        Ok(())
    }
    pub(crate) fn reserve_model(&mut self, elements: i64, items: i64) -> Result<(), Error> {
        let l = &self.limits;
        if elements < 0
            || items < 0
            || elements > l.max_elements - self.elements
            || items > l.max_items - self.items
            || items > l.max_created_items - self.created
        {
            return Err(limit());
        }
        self.elements += elements;
        self.items += items;
        self.created += items;
        Ok(())
    }
    pub(crate) fn touch(&mut self) -> Result<(), Error> {
        if self.touches >= self.limits.max_touches {
            return Err(limit());
        }
        self.touches += 1;
        Ok(())
    }
    pub(crate) fn reserve_bytes(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 || n > self.limits.max_value_bytes - self.value_bytes {
            return Err(limit());
        }
        self.value_bytes += n;
        Ok(())
    }
    pub(crate) fn reserve_text(&mut self, vr: Vr, value: &str) -> Result<(), Error> {
        let mut n = value.len() as i64;
        if !vr.is_string_like() && !value.is_empty() {
            let width: i64 = match vr {
                Vr::US | Vr::SS => 2,
                Vr::UL | Vr::SL | Vr::FL | Vr::AT => 4,
                _ => 8,
            };
            let count = value.matches('\\').count() as i64 + 1;
            if count > i64::MAX / width {
                return Err(limit());
            }
            n = count * width;
        }
        self.reserve_bytes(n)
    }
    pub(crate) fn add_match(&mut self, m: SelectedMatch) -> Result<(), Error> {
        if self.matches.len() as i64 >= self.limits.max_matches {
            return Err(limit());
        }
        self.reserve_path(&m.path)?;
        self.matches.push(m);
        Ok(())
    }
    pub(crate) fn reserve_path(&mut self, p: &Path) -> Result<(), Error> {
        let mut n = 0i64;
        for seg in &p.0 {
            let cost = 64 + seg.creator.len() as i64 + seg.keyword.len() as i64;
            if cost > self.limits.max_path_bytes - n {
                return Err(limit());
            }
            n += cost;
        }
        if n > self.limits.max_path_bytes - self.path_bytes {
            return Err(limit());
        }
        self.path_bytes += n;
        Ok(())
    }
    pub(crate) fn reserve_condition_text_value(
        &mut self,
        vr: Vr,
        value: &Value,
    ) -> Result<(), Error> {
        let charge =
            |this: &mut Budget, data: &[u8], raw: bool, factor: i64| -> Result<(), Error> {
                let n = data.len() as i64;
                if n > i64::MAX / factor {
                    return Err(limit());
                }
                this.reserve_bytes(n * factor)?;
                if n > 0 && raw {
                    this.reserve_bytes(16)?;
                }
                if vr == Vr::PN {
                    this.reserve_bytes(80)?;
                }
                for &b in data {
                    let mut cost = 0;
                    if b == b'\\' && vr.uses_text_value_delimiter() {
                        cost += 16;
                    }
                    if b == b'=' && vr == Vr::PN {
                        cost += 80;
                    }
                    if cost != 0 {
                        this.reserve_bytes(cost)?;
                    }
                }
                Ok(())
            };
        match value {
            Value::Raw(raw) => charge(self, raw, true, 52),
            Value::Strings(values) => {
                self.reserve_bytes(values.len() as i64 * 16)?;
                for text in values {
                    charge(self, text.as_bytes(), false, 13)?;
                }
                Ok(())
            }
            _ => Err(invalid()),
        }
    }
}
pub(crate) fn validate_session_path(p: &Path) -> Result<(), Error> {
    if p.is_empty() || p.len() > MAX_SEGMENTS {
        return Err(Error::bare(Kind::Syntax));
    }
    let mut n = 0usize;
    for (i, seg) in p.0.iter().enumerate() {
        if seg.creator.len() > MAX_PATH_LENGTH - n {
            return Err(limit());
        }
        n += seg.creator.len();
        if seg.keyword.len() > MAX_PATH_LENGTH - n {
            return Err(limit());
        }
        n += seg.keyword.len();
        if 32 > MAX_PATH_LENGTH - n {
            return Err(limit());
        }
        n += 32;
        if seg.private() && (seg.tag.group % 2 == 0 || seg.tag.element > 255) {
            return Err(Error::bare(Kind::Syntax));
        }
        if (seg.has_item && seg.item > MAX_ITEM_INDEX)
            || (seg.any_item && !seg.has_item)
            || (i < p.len() - 1 && !seg.has_item)
        {
            return Err(Error::bare(Kind::Syntax));
        }
    }
    Ok(())
}
fn session_error(err: Error) -> Error {
    match err.kind {
        Kind::Limit
        | Kind::NotFound
        | Kind::NotSequence
        | Kind::UnknownVr
        | Kind::Syntax
        | Kind::Value => Error {
            kind: err.kind,
            text: format!(
                "dicom/attrpath: bounded operation failed: {}",
                err.kind.text()
            ),
        },
        _ => invalid(),
    }
}
pub struct EditSession<'a> {
    dataset: &'a mut Object,
    opts: Options,
    budget: Budget,
}
impl<'a> EditSession<'a> {
    pub fn new(
        dataset: &'a mut Object,
        opts: Options,
        limits: EditLimits,
    ) -> Result<EditSession<'a>, Error> {
        let d = EditLimits::defaults();
        let mut l = limits;
        for (field, default) in [
            (&mut l.max_depth, d.max_depth),
            (&mut l.max_elements, d.max_elements),
            (&mut l.max_items, d.max_items),
            (&mut l.max_visits, d.max_visits),
            (&mut l.max_matches, d.max_matches),
            (&mut l.max_path_bytes, d.max_path_bytes),
            (&mut l.max_touches, d.max_touches),
            (&mut l.max_created_items, d.max_created_items),
        ] {
            if *field < 0 {
                return Err(limit());
            }
            if *field == 0 {
                *field = default;
            }
        }
        if l.max_value_bytes < 0 {
            return Err(limit());
        }
        if l.max_value_bytes == 0 {
            l.max_value_bytes = d.max_value_bytes;
        }
        let mut s = EditSession {
            dataset,
            opts,
            budget: Budget {
                limits: l,
                ..Default::default()
            },
        };
        s.admit()?;
        Ok(s)
    }
    pub fn dataset(&self) -> &Object {
        self.dataset
    }
    fn admit(&mut self) -> Result<(), Error> {
        if self.dataset.len() as i64 > self.budget.limits.max_elements {
            return Err(limit());
        }
        let mut n = 0i64;
        let mut items = 0i64;
        let elements = self.dataset.elements().to_vec();
        scan(&mut self.budget, &elements, 0, &mut n, &mut items)?;
        self.budget.elements = n;
        self.budget.items = items;
        self.budget.charset_admission = true;
        Ok(())
    }
    fn begin(&mut self) -> Result<(), Error> {
        self.budget.path_bytes = 0;
        self.budget.matches.clear();
        self.admit()
    }
    fn select_path(&mut self, p: &Path) -> Result<Vec<SelectedMatch>, Error> {
        self.budget.matches.clear();
        let mut work = self.dataset.clone();
        let mut e = Editor::new(Mode::Find, &self.opts, "");
        e.session = Some(&mut self.budget);
        let result = e.run(&mut work, p);
        drop(e);
        result.map_err(session_error)?;
        Ok(std::mem::take(&mut self.budget.matches))
    }
    pub fn apply(&mut self, edit: &Edit) -> Result<usize, Error> {
        self.begin()?;
        self.apply_edit(edit)
    }
    fn apply_edit(&mut self, edit: &Edit) -> Result<usize, Error> {
        let mode = match edit.op {
            Operation::Insert => Mode::Insert,
            Operation::Modify => Mode::Modify,
            Operation::Remove => Mode::Remove,
            Operation::Create => Mode::Create,
        };
        let mut e = Editor::new(mode, &self.opts, &edit.value);
        e.session = Some(&mut self.budget);
        match e.run(self.dataset, &edit.path) {
            Ok(n) => Ok(n),
            Err(err) if edit.if_present && err.is(Kind::NotFound) => Ok(0),
            Err(err) => Err(session_error(err)),
        }
    }
    pub fn modify_all(&mut self, tag: &Segment, value: &str) -> Result<usize, Error> {
        self.all(tag, value, false)
    }
    pub fn remove_all(&mut self, tag: &Segment) -> Result<usize, Error> {
        self.all(tag, "", true)
    }
    fn all(&mut self, tag: &Segment, value: &str, remove: bool) -> Result<usize, Error> {
        if tag.has_item || tag.any_item {
            return Err(Error::bare(Kind::Syntax));
        }
        validate_session_path(&Path(vec![tag.clone()]))?;
        self.begin()?;
        let opts = WalkOptions {
            max_depth: self.budget.limits.max_depth,
            max_elements: self.budget.limits.max_elements,
        };
        let mut events: Vec<(Vec<crate::object::WalkPathStep>, Element)> = Vec::new();
        let walked = self.dataset.walk_path_with(opts, |path, el| {
            events.push((path.to_vec(), el.clone()));
            Ok(())
        });
        let mut selected: Vec<Path> = Vec::new();
        for (path, el) in events {
            self.budget.visit().map_err(session_error)?;
            if el.tag().group != tag.tag.group || (!tag.private() && el.tag() != tag.tag) {
                continue;
            }
            let max = self.budget.limits.max_path_bytes;
            if path.len() as i64 > max / 64 || path.len() as i64 * 64 > max - self.budget.path_bytes
            {
                return Err(session_error(limit()));
            }
            self.budget.path_bytes += path.len() as i64 * 64;
            let p = Path(
                path.iter()
                    .map(|step| Segment {
                        tag: step.tag,
                        has_item: step.item_index >= 0,
                        item: step.item_index.max(0) as usize,
                        ..Default::default()
                    })
                    .collect(),
            );
            if tag.private() {
                let parent = if p.len() > 1 {
                    let matches = match self.select_path(&Path(p.0[..p.len() - 1].to_vec())) {
                        Err(err) if err.is(Kind::NotFound) => continue,
                        Err(err) => return Err(session_error(err)),
                        Ok(m) => m,
                    };
                    if matches.len() != 1 || matches[0].item.is_none() {
                        return Err(session_error(invalid()));
                    }
                    matches[0].item.clone().unwrap_or_default()
                } else {
                    self.dataset.clone()
                };
                match dictionary::reservations::private_tag(
                    &parent,
                    tag.tag.group,
                    &tag.creator,
                    tag.tag.element as u8,
                ) {
                    Ok(actual) if actual == el.tag() => {}
                    _ => continue,
                }
            }
            if selected.iter().any(|ancestor| path_prefix(ancestor, &p)) {
                continue;
            }
            if !remove && el.vr() == Vr::SQ {
                return Err(session_error(invalid()));
            }
            if selected.len() as i64 >= self.budget.limits.max_matches {
                return Err(session_error(limit()));
            }
            self.budget.matches.clear();
            self.budget
                .add_match(SelectedMatch {
                    path: p.clone(),
                    element: None,
                    item: None,
                    parent: Object::new(),
                })
                .map_err(session_error)?;
            self.budget.matches.clear();
            selected.push(p);
        }
        if walked.is_err() {
            return Err(invalid());
        }
        if selected.is_empty() {
            return Err(Error::bare(Kind::NotFound));
        }
        let mut total = 0;
        for p in selected {
            let op = if remove {
                Operation::Remove
            } else {
                Operation::Modify
            };
            total += self.apply_edit(&Edit {
                op,
                path: p,
                value: value.to_string(),
                if_present: false,
            })?;
        }
        Ok(total)
    }
    pub fn evaluate(&mut self, c: &Condition) -> Result<bool, Error> {
        self.begin()?;
        if c.path.is_empty() {
            return Err(Error::new(Kind::Syntax, "invalid condition"));
        }
        let mut result = self.condition_path(c, &c.path, 0)?;
        if c.op == Op::Absent {
            result = !result;
        }
        Ok(result)
    }
    fn condition_path(&mut self, c: &Condition, p: &Path, start: usize) -> Result<bool, Error> {
        for i in start..p.len().saturating_sub(1) {
            if !p.0[i].any_item {
                continue;
            }
            let branches = match self.select_path(&Path(p.0[..=i].to_vec())) {
                Err(err) if err.is(Kind::NotFound) => return Ok(false),
                Err(err) => return Err(err),
                Ok(b) => b,
            };
            let mut result = c.op.compares() && !branches.is_empty();
            for branch in branches {
                self.budget.reserve_path(p)?;
                let mut concrete = p.clone();
                concrete.0[..branch.path.len()].clone_from_slice(&branch.path.0);
                let matched = self.condition_path(c, &concrete, i + 1)?;
                if !c.op.compares() {
                    if matched {
                        return Ok(true);
                    }
                } else {
                    result = result && matched;
                }
            }
            return Ok(result);
        }
        let last = p.0[p.len() - 1].clone();
        let mut selection = p.clone();
        if last.has_item && !last.any_item {
            self.budget.reserve_path(p)?;
            let n = selection.len() - 1;
            selection.0[n].has_item = false;
            selection.0[n].item = 0;
        }
        let matches = match self.select_path(&selection) {
            Err(err) if err.is(Kind::NotFound) => return Ok(false),
            Err(err) if last.any_item && err.is(Kind::NotSequence) => {
                return Err(Error::new(
                    Kind::Value,
                    "wildcard VM selection is unsupported",
                ));
            }
            Err(err) => return Err(err),
            Ok(m) => m,
        };
        let mut result = c.op.compares() && !matches.is_empty();
        for m in matches {
            let matched = self.condition_match(c, &last, &m)?;
            if !c.op.compares() {
                if matched {
                    return Ok(true);
                }
            } else {
                result = result && matched;
            }
        }
        Ok(result)
    }
    fn condition_match(
        &mut self,
        c: &Condition,
        last: &Segment,
        m: &SelectedMatch,
    ) -> Result<bool, Error> {
        let existence = !c.op.compares();
        if existence && !last.has_item {
            return Ok(true);
        }
        if m.item.is_some() {
            if existence {
                return Ok(true);
            }
            return Err(Error::new(Kind::Value, "condition cannot compare an item"));
        }
        let element = m.element.clone().unwrap_or_else(|| Element {
            header: crate::object::ElementHeader {
                tag: Tag::default(),
                vr: Vr::UN,
                length: 0,
                length_set: false,
            },
            value: Value::Nil,
        });
        if let Value::Sequence(items) = &element.value {
            if existence && last.has_item {
                return Ok(last.item < items.len());
            }
            return Err(Error::new(
                Kind::Value,
                "condition cannot compare a sequence",
            ));
        }
        if existence {
            let count = self.condition_vm(&element, &m.parent)?;
            return Ok(last.item < count);
        }
        let lhs = self.condition_vector(&element, &m.parent)?;
        if last.has_item && last.item >= lhs.count {
            return Ok(false);
        }
        let rhs = self.condition_rhs(&element, &m.parent, &c.value)?;
        let (start, count) = if last.has_item {
            (last.item, 1)
        } else {
            (0, lhs.count)
        };
        let mut comparison = Ordering::Equal;
        let mut i = 0;
        while i < count && i < rhs.count {
            self.budget.visit()?;
            comparison = lhs.compare(start + i, &rhs, i);
            if comparison != Ordering::Equal {
                break;
            }
            i += 1;
        }
        if comparison == Ordering::Equal {
            comparison = count.cmp(&rhs.count);
        }
        Ok(match c.op {
            Op::Equal => comparison == Ordering::Equal,
            Op::NotEqual => comparison != Ordering::Equal,
            Op::Less => comparison == Ordering::Less,
            Op::LessEqual => comparison != Ordering::Greater,
            Op::Greater => comparison == Ordering::Greater,
            _ => comparison != Ordering::Less,
        })
    }
    fn condition_vm(&mut self, element: &Element, parent: &Object) -> Result<usize, Error> {
        let vr = element.vr();
        if vr.is_string_like() || condition_width(vr) != 0 {
            return Ok(self.condition_vector(element, parent)?.count);
        }
        let h = &element.header;
        Ok(match &element.value {
            Value::Nil => usize::from(!(h.length_set && h.length == 0)),
            Value::Raw(raw) => usize::from(!raw.is_empty()),
            Value::Strings(values) => {
                let n: usize =
                    values.iter().map(String::len).sum::<usize>() + values.len().saturating_sub(1);
                usize::from(n != 0)
            }
            _ => 1,
        })
    }
    fn condition_vector(&mut self, element: &Element, parent: &Object) -> Result<Vector, Error> {
        let mut v = Vector {
            vr: element.vr(),
            raw: Vec::new(),
            big_endian: parent.value_big_endian(),
            text: Vec::new(),
            count: 0,
        };
        let h = &element.header;
        if element.tag() == Tag::PIXEL_DATA
            || matches!(element.value, Value::Nil)
            || (h.length_set && h.length == UNDEFINED_LENGTH)
        {
            return Err(Error::new(
                Kind::Value,
                "condition requires a materialized primitive",
            ));
        }
        if v.vr.is_string_like() {
            v.text = self.condition_text(element, parent)?;
            v.count = v.text.len();
            return Ok(v);
        }
        let width = condition_width(v.vr);
        if width == 0 {
            return Err(Error::new(Kind::Value, "condition VR is unsupported"));
        }
        match &element.value {
            Value::Raw(raw) => {
                if raw.len() % width != 0 {
                    return Err(Error::new(
                        Kind::Value,
                        "condition primitive has invalid width",
                    ));
                }
                v.count = raw.len() / width;
                v.raw = raw.clone();
            }
            _ => {
                return Err(Error::new(
                    Kind::Value,
                    "condition primitive has invalid type",
                ));
            }
        }
        if matches!(v.vr, Vr::FL | Vr::FD) {
            for i in 0..v.count {
                self.budget.visit()?;
                if !v.real(i).is_finite() {
                    return Err(Error::new(
                        Kind::Value,
                        "condition floating value is not finite",
                    ));
                }
            }
        }
        Ok(v)
    }
    fn condition_rhs(
        &mut self,
        element: &Element,
        parent: &Object,
        text: &str,
    ) -> Result<Vector, Error> {
        let vr = element.vr();
        let mut count: i64 = 1;
        if vr.uses_text_value_delimiter() {
            count += text.matches('\\').count() as i64;
        }
        if count > (i64::MAX - text.len() as i64) / 80 {
            return Err(limit());
        }
        self.budget.reserve_bytes(text.len() as i64 + count * 80)?;
        self.budget.reserve_text(vr, text)?;
        if vr.is_string_like() {
            self.budget
                .reserve_condition_text_value(vr, &Value::Strings(vec![text.to_string()]))?;
        }
        let operand = || Error::new(Kind::Value, "invalid condition operand");
        if vr == Vr::AT && !condition_at_operand(text) {
            return Err(operand());
        }
        let mut text = text.to_string();
        if vr == Vr::UI {
            let parts: Vec<String> = split_text_multiplicity(vr, &text)
                .into_iter()
                .map(|part| match dictionary::uid::by_keyword(&part) {
                    Some(entry) => entry.uid.to_string(),
                    None => part,
                })
                .collect();
            text = parts.join("\\");
        }
        let rhs = element_from_string(element.tag(), vr, &text, parent.value_big_endian())
            .map_err(|_| operand())?;
        let report = validation::validate_element(
            &rhs,
            validation::Options {
                dictionary: false,
                max_findings: 0,
            },
        );
        if first_error(&report).is_some() {
            return Err(operand());
        }
        self.condition_vector(&rhs, parent)
    }
    fn condition_text(&mut self, element: &Element, parent: &Object) -> Result<Vec<String>, Error> {
        self.budget
            .reserve_condition_text_value(element.vr(), &element.value)?;
        let vr = element.vr();
        let charset = if vr.uses_specific_character_set() {
            parent
                .character_set()
                .map_err(|_| Error::new(Kind::Value, "condition character set is unresolved"))?
        } else {
            SpecificCharacterSet::default_repertoire()
        };
        let encoding = || Error::new(Kind::Value, "invalid condition text encoding");
        let mut values: Vec<String> = match &element.value {
            Value::Raw(raw) => {
                let raw = trim_text_value_bytes(vr, raw);
                let text = if vr == Vr::PN {
                    charset.decode_person_name(raw)
                } else if !vr.uses_text_value_delimiter() {
                    charset.decode_single_value(raw)
                } else {
                    charset.decode(raw)
                }
                .map_err(|_| encoding())?;
                split_text_multiplicity(vr, &text)
            }
            Value::Strings(values) => values
                .iter()
                .map(|v| trim_text_value(vr, v).to_string())
                .collect(),
            _ => {
                return Err(Error::new(
                    Kind::Value,
                    "condition text is not materialized",
                ));
            }
        };
        for value in values.iter_mut() {
            if vr == Vr::DS || vr == Vr::IS {
                *value = value.trim_matches(' ').to_string();
            }
            let ok = if vr == Vr::PN {
                charset.encode_person_name(value).is_ok()
            } else if !vr.uses_text_value_delimiter() {
                charset.encode_single_value(value).is_ok()
            } else {
                charset.encode(value).is_ok()
            };
            if !ok {
                return Err(encoding());
            }
        }
        if valueencode::validate_string(vr, &values).is_err() {
            return Err(Error::new(Kind::Value, "invalid condition text value"));
        }
        if values.len() == 1 && values[0].is_empty() {
            values.clear();
        }
        Ok(values)
    }
}
fn scan(
    b: &mut Budget,
    elements: &[Element],
    depth: i64,
    n: &mut i64,
    items: &mut i64,
) -> Result<(), Error> {
    for el in elements {
        b.visit()?;
        if *n >= b.limits.max_elements {
            return Err(limit());
        }
        *n += 1;
        if !b.charset_admission && el.tag() == Tag::new(0x0008, 0x0005) {
            b.reserve_condition_text_value(Vr::CS, &el.value)?;
        }
        match &el.value {
            Value::Sequence(seq) => {
                if !seq.is_empty() && depth >= b.limits.max_depth {
                    return Err(limit());
                }
                if seq.len() as i64 > b.limits.max_items - *items {
                    return Err(limit());
                }
                *items += seq.len() as i64;
                for item in seq {
                    b.visit()?;
                    scan(b, &item.elements, depth + 1, n, items)?;
                }
            }
            Value::Nil if el.header.length > 0 => return Err(invalid()),
            _ => {}
        }
    }
    Ok(())
}
fn path_prefix(a: &Path, b: &Path) -> bool {
    if a.len() >= b.len() {
        return false;
    }
    a.0.iter()
        .zip(b.0.iter())
        .all(|(x, y)| x.tag == y.tag && (!x.has_item || (y.has_item && x.item == y.item)))
}
fn condition_width(vr: Vr) -> usize {
    match vr {
        Vr::US | Vr::SS => 2,
        Vr::UL | Vr::SL | Vr::FL | Vr::AT => 4,
        Vr::UV | Vr::SV | Vr::FD => 8,
        _ => 0,
    }
}
fn condition_at_operand(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    for part in text.split('\\') {
        let mut part = super::script::trim_unicode_space(part).to_string();
        let b = part.as_bytes();
        if b.len() == 11 && b[0] == b'(' && b[5] == b',' && b[10] == b')' {
            part = format!("{}{}", &part[1..5], &part[6..10]);
        }
        if part.len() != 8 || !part.bytes().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    true
}
struct Vector {
    vr: Vr,
    raw: Vec<u8>,
    big_endian: bool,
    text: Vec<String>,
    count: usize,
}
impl Vector {
    fn bytes<const N: usize>(&self, at: usize) -> [u8; N] {
        let mut b = [0u8; N];
        b.copy_from_slice(&self.raw[at..at + N]);
        if self.big_endian {
            b.reverse();
        }
        b
    }
    fn u16_at(&self, at: usize) -> u16 {
        u16::from_le_bytes(self.bytes::<2>(at))
    }
    fn unsigned(&self, i: usize) -> u64 {
        match self.vr {
            Vr::US => self.u16_at(i * 2) as u64,
            Vr::UL => u32::from_le_bytes(self.bytes::<4>(i * 4)) as u64,
            Vr::UV => u64::from_le_bytes(self.bytes::<8>(i * 8)),
            Vr::AT => (self.u16_at(i * 4) as u64) << 16 | self.u16_at(i * 4 + 2) as u64,
            _ => 0,
        }
    }
    fn signed(&self, i: usize) -> i64 {
        match self.vr {
            Vr::SS => self.u16_at(i * 2) as i16 as i64,
            Vr::SL => i32::from_le_bytes(self.bytes::<4>(i * 4)) as i64,
            Vr::SV => i64::from_le_bytes(self.bytes::<8>(i * 8)),
            _ => 0,
        }
    }
    fn real(&self, i: usize) -> f64 {
        if self.vr == Vr::FL {
            return f32::from_le_bytes(self.bytes::<4>(i * 4)) as f64;
        }
        f64::from_le_bytes(self.bytes::<8>(i * 8))
    }
    fn compare(&self, i: usize, other: &Vector, j: usize) -> Ordering {
        match self.vr {
            Vr::SS | Vr::SL | Vr::SV => self.signed(i).cmp(&other.signed(j)),
            Vr::FL | Vr::FD => self
                .real(i)
                .partial_cmp(&other.real(j))
                .unwrap_or(Ordering::Equal),
            Vr::US | Vr::UL | Vr::UV | Vr::AT => self.unsigned(i).cmp(&other.unsigned(j)),
            _ => self.text[i].as_bytes().cmp(other.text[j].as_bytes()),
        }
    }
}
pub const MAX_CONDITION_LENGTH: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Op {
    Exists,
    Absent,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
impl Op {
    fn compares(self) -> bool {
        self >= Op::Equal
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    path: Path,
    op: Op,
    value: String,
}
pub fn parse_condition(text: &str) -> Result<Condition, Error> {
    if text.len() > MAX_CONDITION_LENGTH {
        return Err(Error::new(Kind::Limit, "condition exceeds length limit"));
    }
    let text = text.trim_start_matches([' ', '\t', '\r', '\n']);
    let b = text.as_bytes();
    let mut op = Op::Exists;
    let (mut path_text, mut value) = (text, "");
    let mut quoted = false;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            quoted = !quoted;
            i += 1;
            continue;
        }
        if quoted {
            i += 1;
            continue;
        }
        let mut width = 1;
        match b[i] {
            b'=' => op = Op::Equal,
            b'!' => {
                if i == 0 && (b.len() == 1 || b[1] != b'=') {
                    op = Op::Absent;
                    i += 1;
                    continue;
                }
                if i + 1 >= b.len() || b[i + 1] != b'=' {
                    return Err(Error::new(Kind::Syntax, "invalid condition operator"));
                }
                op = Op::NotEqual;
                width = 2;
            }
            b'<' => {
                op = Op::Less;
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    op = Op::LessEqual;
                    width = 2;
                }
            }
            b'>' => {
                op = Op::Greater;
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    op = Op::GreaterEqual;
                    width = 2;
                }
            }
            _ => {
                i += 1;
                continue;
            }
        }
        if text.starts_with('!') {
            return Err(Error::new(
                Kind::Syntax,
                "negated comparison is unsupported",
            ));
        }
        path_text = &text[..i];
        value = &text[i + width..];
        break;
    }
    if op == Op::Absent {
        path_text = path_text.strip_prefix('!').unwrap_or(path_text);
    }
    match parse(super::script::trim_unicode_space(path_text)) {
        Ok(path) => Ok(Condition {
            path,
            op,
            value: value.to_string(),
        }),
        Err(err) if err.is(Kind::Limit) => {
            Err(Error::new(Kind::Limit, "condition path exceeds limit"))
        }
        Err(_) => Err(Error::new(Kind::Syntax, "invalid condition path")),
    }
}
