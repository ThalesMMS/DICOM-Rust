use super::read::{ReadOptions, ReadResult, read_document_with_options};
use super::reference::{append_path, identifier_string};
use super::template_metadata::element_index;
use super::{ContentItem, DataSet, Element, Kind, SrError, tags as tg, value};
use crate::object::{ElementHeader, Object, Tag, Value, Vr};
use std::cmp::Ordering;
use std::collections::HashMap;
pub const NUMERIC_CODE_SHAPE: &str = "numeric-shape";
pub const NUMERIC_CODE_DS: &str = "decimal-string";
pub const NUMERIC_CODE_FINITE: &str = "finite-profile";
pub const NUMERIC_CODE_COHERENCE: &str = "representation-coherence";
pub const NUMERIC_CODE_ASSOCIATION: &str = "numeric-association";
pub const NUMERIC_CODE_LIMIT: &str = "numeric-resource-limit";
const TAG_FLOATING_POINT_VALUE: Tag = Tag::new(0x0040, 0xA161);
const TAG_RATIONAL_NUMERATOR_VALUE: Tag = Tag::new(0x0040, 0xA162);
const TAG_RATIONAL_DENOMINATOR_VALUE: Tag = Tag::new(0x0040, 0xA163);
pub(crate) fn numeric_error(code: &str, path: &str) -> SrError {
    let mut msg = format!("{}: {code}", Kind::NumericRepresentation.text());
    if super::image_library::valid_numeric_path(path) {
        msg.push_str(" at ");
        msg.push_str(path);
    }
    let mut e = SrError::new(Kind::NumericRepresentation, "");
    e.msg = msg;
    e.code = Some(code.into());
    if code == NUMERIC_CODE_LIMIT {
        e.kinds.push(Kind::ResourceLimitExceeded);
    }
    e
}
fn numeric_error_at(err: SrError, path: &str) -> SrError {
    if err.is(Kind::NumericRepresentation) {
        return numeric_error(err.code.as_deref().unwrap_or(NUMERIC_CODE_SHAPE), path);
    }
    numeric_error(NUMERIC_CODE_SHAPE, path)
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumericRepresentation {
    pub decimal_string: String,
    pub has_decimal_string: bool,
    pub floating_point: f64,
    pub has_floating_point: bool,
    pub rational_numerator: i32,
    pub rational_denominator: u32,
    pub has_rational: bool,
}
pub const NUMERIC_SOURCE_DECIMAL: &str = "DS";
pub const NUMERIC_SOURCE_FLOATING_POINT: &str = "FD";
pub const NUMERIC_SOURCE_RATIONAL: &str = "RATIONAL";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NumericProvenance {
    pub source: &'static str,
    pub exact: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumericRepresentationEntry {
    pub path: String,
    pub representation: NumericRepresentation,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumericRepresentationMetadata {
    pub entries: Vec<NumericRepresentationEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big(Vec<u32>);
impl Big {
    fn from_u64(v: u64) -> Big {
        let mut b = Big(vec![v as u32, (v >> 32) as u32]);
        b.trim();
        b
    }
    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }
    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
    fn mul_small(&self, m: u32) -> Big {
        let mut out = Vec::with_capacity(self.0.len() + 1);
        let mut carry = 0u64;
        for &l in &self.0 {
            let v = u64::from(l) * u64::from(m) + carry;
            out.push(v as u32);
            carry = v >> 32;
        }
        out.push(carry as u32);
        let mut b = Big(out);
        b.trim();
        b
    }
    fn mul(&self, o: &Big) -> Big {
        let mut out = vec![0u32; self.0.len() + o.0.len() + 1];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in o.0.iter().enumerate() {
                let v = u64::from(out[i + j]) + u64::from(a) * u64::from(b) + carry;
                out[i + j] = v as u32;
                carry = v >> 32;
            }
            let mut k = i + o.0.len();
            while carry > 0 {
                let v = u64::from(out[k]) + carry;
                out[k] = v as u32;
                carry = v >> 32;
                k += 1;
            }
        }
        let mut b = Big(out);
        b.trim();
        b
    }
    fn pow10(n: u32) -> Big {
        let mut b = Big::from_u64(1);
        for _ in 0..n {
            b = b.mul_small(10);
        }
        b
    }
    fn shl(&self, bits: u32) -> Big {
        let mut b = self.clone();
        for _ in 0..bits / 16 {
            b = b.mul_small(1 << 16);
        }
        b.mul_small(1 << (bits % 16))
    }
    fn add(&self, o: &Big) -> Big {
        let n = self.0.len().max(o.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry = 0u64;
        for i in 0..n {
            let v = u64::from(*self.0.get(i).unwrap_or(&0))
                + u64::from(*o.0.get(i).unwrap_or(&0))
                + carry;
            out.push(v as u32);
            carry = v >> 32;
        }
        out.push(carry as u32);
        let mut b = Big(out);
        b.trim();
        b
    }
    fn sub(&self, o: &Big) -> Big {
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow = 0i64;
        for (i, &limb) in self.0.iter().enumerate() {
            let mut v = i64::from(limb) - i64::from(*o.0.get(i).unwrap_or(&0)) - borrow;
            borrow = 0;
            if v < 0 {
                v += 1 << 32;
                borrow = 1;
            }
            out.push(v as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }
    fn cmp(&self, o: &Big) -> Ordering {
        self.0
            .len()
            .cmp(&o.0.len())
            .then_with(|| self.0.iter().rev().cmp(o.0.iter().rev()))
    }
}
#[derive(Clone, Debug)]
struct Int {
    neg: bool,
    mag: Big,
}
impl Int {
    fn mul(&self, o: &Big) -> Int {
        Int {
            neg: self.neg,
            mag: self.mag.mul(o),
        }
    }
    fn abs_diff(&self, o: &Int) -> Big {
        let (a, b) = (
            if self.mag.is_zero() { false } else { self.neg },
            if o.mag.is_zero() { false } else { o.neg },
        );
        if a != b {
            return self.mag.add(&o.mag);
        }
        match self.mag.cmp(&o.mag) {
            Ordering::Less => o.mag.sub(&self.mag),
            _ => self.mag.sub(&o.mag),
        }
    }
    fn eq_value(&self, o: &Int) -> bool {
        self.mag == o.mag && (self.mag.is_zero() || self.neg == o.neg)
    }
}
#[derive(Clone, Debug)]
struct Rat {
    num: Int,
    den: Big,
}
impl Rat {
    fn from_f64(v: f64) -> Rat {
        let bits = v.to_bits();
        let neg = bits >> 63 == 1;
        let exp = ((bits >> 52) & 0x7FF) as i32;
        let frac = bits & ((1 << 52) - 1);
        let (m, e) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1 << 52), exp - 1075)
        };
        let m = Big::from_u64(m);
        if e >= 0 {
            Rat {
                num: Int {
                    neg,
                    mag: m.shl(e as u32),
                },
                den: Big::from_u64(1),
            }
        } else {
            Rat {
                num: Int { neg, mag: m },
                den: Big::from_u64(1).shl((-e) as u32),
            }
        }
    }
    fn eq(&self, o: &Rat) -> bool {
        self.num.mul(&o.den).eq_value(&o.num.mul(&self.den))
    }
}
fn numeric_decimal(original: &str) -> Result<(Int, i64, f64), SrError> {
    let fail = |code| Err(numeric_error(code, ""));
    if original.is_empty() || original.len() > 16 {
        return fail(NUMERIC_CODE_DS);
    }
    let s = original.trim_matches(' ');
    if s.is_empty() {
        return fail(NUMERIC_CODE_DS);
    }
    let b = s.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if b[0] == b'+' || b[0] == b'-' {
        negative = b[0] == b'-';
        i += 1;
        if i == b.len() {
            return fail(NUMERIC_CODE_DS);
        }
    }
    let mut digits = String::new();
    let mut fractional = 0i64;
    let mut dot = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            digits.push(c as char);
            if dot {
                fractional += 1;
            }
            i += 1;
            continue;
        }
        if c == b'.' && !dot {
            dot = true;
            i += 1;
            continue;
        }
        break;
    }
    if digits.is_empty() {
        return fail(NUMERIC_CODE_DS);
    }
    let mut exponent = 0i64;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        let mut sign = 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            if b[i] == b'-' {
                sign = -1;
            }
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            exponent = exponent * 10 + i64::from(b[i] - b'0');
            if exponent > 1024 {
                return fail(NUMERIC_CODE_LIMIT);
            }
            i += 1;
        }
        if i == start {
            return fail(NUMERIC_CODE_DS);
        }
        exponent *= sign;
    }
    if i != b.len() {
        return fail(NUMERIC_CODE_DS);
    }
    let mut mag = Big::from_u64(0);
    for d in digits.bytes() {
        mag = mag.mul_small(10).add(&Big::from_u64(u64::from(d - b'0')));
    }
    let power = exponent - fractional;
    let scalar = match crate::numtext::parse_float(s) {
        Ok(v) if v.is_finite() => v,
        _ => return fail(NUMERIC_CODE_FINITE),
    };
    Ok((Int { neg: negative, mag }, power, scalar))
}
fn within_cell(m: &Int, power: i64, value: &Rat) -> bool {
    let q = Big::pow10(power.unsigned_abs() as u32);
    let two_den = value.den.mul_small(2);
    if power >= 0 {
        let lhs = value
            .num
            .mul(&Big::from_u64(2))
            .abs_diff(&m.mul(&two_den.mul(&q)));
        lhs.cmp(&value.den.mul(&q)) != Ordering::Greater
    } else {
        let lhs = value.num.mul(&q.mul_small(2)).abs_diff(&m.mul(&two_den));
        lhs.cmp(&value.den) != Ordering::Greater
    }
}
fn decimal_rat(m: &Int, power: i64) -> Rat {
    let q = Big::pow10(power.unsigned_abs() as u32);
    if power >= 0 {
        Rat {
            num: m.mul(&q),
            den: Big::from_u64(1),
        }
    } else {
        Rat {
            num: m.clone(),
            den: q,
        }
    }
}
impl NumericRepresentation {
    pub fn resolve(&self) -> Result<(f64, NumericProvenance), SrError> {
        let fail = |code| Err(numeric_error(code, ""));
        if !self.has_decimal_string
            || (!self.has_floating_point && self.floating_point.to_bits() != 0)
            || (!self.has_rational
                && (self.rational_numerator != 0 || self.rational_denominator != 0))
        {
            return fail(NUMERIC_CODE_SHAPE);
        }
        let (m, power, mut scalar) = numeric_decimal(&self.decimal_string)?;
        let mut exact = decimal_rat(&m, power);
        let mut source = NUMERIC_SOURCE_DECIMAL;
        if self.has_rational {
            if self.rational_denominator == 0 {
                return fail(NUMERIC_CODE_SHAPE);
            }
            let n = i64::from(self.rational_numerator);
            exact = Rat {
                num: Int {
                    neg: n < 0,
                    mag: Big::from_u64(n.unsigned_abs()),
                },
                den: Big::from_u64(u64::from(self.rational_denominator)),
            };
            scalar = f64::from(self.rational_numerator) / f64::from(self.rational_denominator);
            source = NUMERIC_SOURCE_RATIONAL;
            if !within_cell(&m, power, &exact) {
                return fail(NUMERIC_CODE_COHERENCE);
            }
        }
        if self.has_floating_point {
            if !self.floating_point.is_finite() {
                return fail(NUMERIC_CODE_FINITE);
            }
            if self.has_rational && scalar != self.floating_point {
                return fail(NUMERIC_CODE_COHERENCE);
            }
            exact = Rat::from_f64(self.floating_point);
            if !within_cell(&m, power, &exact) {
                return fail(NUMERIC_CODE_COHERENCE);
            }
            scalar = self.floating_point;
            source = NUMERIC_SOURCE_FLOATING_POINT;
        }
        let exact_match = Rat::from_f64(scalar).eq(&exact);
        Ok((
            scalar,
            NumericProvenance {
                source,
                exact: exact_match,
            },
        ))
    }
}
pub(crate) fn index_numeric_representations(
    content: &[ContentItem],
    metadata: &NumericRepresentationMetadata,
    max_items: i64,
    max_depth: i64,
) -> Result<HashMap<String, NumericRepresentation>, SrError> {
    let mut index = HashMap::new();
    for entry in &metadata.entries {
        if !super::image_library::valid_numeric_path(&entry.path) {
            return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, ""));
        }
        if index.contains_key(&entry.path) {
            return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &entry.path));
        }
        entry
            .representation
            .resolve()
            .map_err(|e| numeric_error_at(e, &entry.path))?;
        index.insert(entry.path.clone(), entry.representation.clone());
    }
    if index.is_empty() {
        return Ok(index);
    }
    let mut count = 0i64;
    let mut found = 0usize;
    let mut stack: Vec<(&[ContentItem], Vec<u32>, i64, usize)> = Vec::new();
    if !content.is_empty() {
        stack.push((content, vec![1], 1, 0));
    }
    while let Some(frame) = stack.last_mut() {
        if frame.3 >= frame.0.len() {
            stack.pop();
            continue;
        }
        let i = frame.3;
        frame.3 += 1;
        let item = &frame.0[i];
        let path = append_path(&frame.1, i as u32 + 1);
        let depth = frame.2;
        count += 1;
        if count > max_items || depth > max_depth {
            return Err(numeric_error(NUMERIC_CODE_LIMIT, ""));
        }
        let key = identifier_string(&path);
        if let Some(r) = index.get(&key) {
            let v = r.resolve().map(|x| x.0).unwrap_or(0.0);
            match &item.measurement {
                Some(m) if item.value_type == value::NUM && m.value.to_bits() == v.to_bits() => {}
                _ => return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &key)),
            }
            found += 1;
        }
        if !item.children.is_empty() {
            stack.push((&item.children, path, depth + 1, 0));
        }
    }
    if found != index.len() {
        return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, ""));
    }
    Ok(index)
}
pub(crate) fn resident_string(e: &Element, vr: Vr) -> Result<String, SrError> {
    if e.vr() != vr {
        return Err(numeric_error(NUMERIC_CODE_SHAPE, ""));
    }
    match &e.value {
        Value::Raw(v) => Ok(String::from_utf8_lossy(v).into_owned()),
        Value::Strings(v) if v.len() == 1 => Ok(v[0].clone()),
        _ => Err(numeric_error(NUMERIC_CODE_SHAPE, "")),
    }
}
fn read_numeric_macro(item: &Object) -> Result<Option<NumericRepresentation>, SrError> {
    let fail = || Err(numeric_error(NUMERIC_CODE_SHAPE, ""));
    let Some(element) = item.get(tg::MEASURED_VALUE_SEQ) else {
        return fail();
    };
    let Value::Sequence(seq) = &element.value else {
        return fail();
    };
    if element.vr() != Vr::SQ || seq.len() > 1 {
        return fail();
    }
    if seq.is_empty() {
        return Ok(None);
    }
    let items = item
        .get_sequence(tg::MEASURED_VALUE_SEQ)
        .unwrap_or_default();
    let m = &items[0];
    let Some(ds) = m.get(tg::NUMERIC_VALUE) else {
        return fail();
    };
    let ds = resident_string(ds, Vr::DS)?;
    let Some(units) = m.get(tg::MEASUREMENT_UNITS_CODE) else {
        return fail();
    };
    match &units.value {
        Value::Sequence(u) if units.vr() == Vr::SQ && u.len() == 1 => {}
        _ => return fail(),
    }
    let big = m.value_big_endian();
    let mut r = NumericRepresentation {
        decimal_string: ds,
        has_decimal_string: true,
        ..Default::default()
    };
    if let Some(fd) = m.get(TAG_FLOATING_POINT_VALUE) {
        match fd.raw() {
            Some(v) if fd.vr() == Vr::FD && v.len() == 8 => {
                let b: [u8; 8] = v.try_into().unwrap_or_default();
                r.floating_point = f64::from_bits(if big {
                    u64::from_be_bytes(b)
                } else {
                    u64::from_le_bytes(b)
                });
            }
            _ => return fail(),
        }
        r.has_floating_point = true;
    }
    let (n, d) = (
        m.get(TAG_RATIONAL_NUMERATOR_VALUE),
        m.get(TAG_RATIONAL_DENOMINATOR_VALUE),
    );
    if n.is_some() != d.is_some() {
        return fail();
    }
    if let (Some(n), Some(d)) = (n, d) {
        if n.vr() != Vr::SL || d.vr() != Vr::UL {
            return fail();
        }
        let word = |e: &Element| -> Option<u32> {
            let v = e.raw()?;
            let b: [u8; 4] = v.try_into().ok()?;
            Some(if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            })
        };
        let Some(nv) = word(n) else { return fail() };
        let Some(dv) = word(d) else { return fail() };
        r.rational_numerator = nv as i32;
        r.rational_denominator = dv;
        r.has_rational = true;
    }
    r.resolve()?;
    Ok(Some(r))
}
pub(crate) fn read_numeric_metadata(
    obj: &Object,
    max_items: i64,
    max_depth: i64,
) -> Result<NumericRepresentationMetadata, SrError> {
    let mut metadata = NumericRepresentationMetadata::default();
    let mut count = 0i64;
    fn visit(
        parent: &Object,
        path: &[u32],
        depth: i64,
        max_items: i64,
        max_depth: i64,
        count: &mut i64,
        metadata: &mut NumericRepresentationMetadata,
    ) -> Result<(), SrError> {
        let Some(element) = parent.get(tg::CONTENT_SEQUENCE) else {
            return Ok(());
        };
        let Value::Sequence(seq) = &element.value else {
            return Err(numeric_error(NUMERIC_CODE_SHAPE, ""));
        };
        if element.vr() != Vr::SQ {
            return Err(numeric_error(NUMERIC_CODE_SHAPE, ""));
        }
        if !seq.is_empty()
            && (depth > 64 || depth > max_depth || seq.len() as i64 > max_items - *count)
        {
            return Err(numeric_error(NUMERIC_CODE_LIMIT, ""));
        }
        let items = parent
            .get_sequence(tg::CONTENT_SEQUENCE)
            .unwrap_or_default();
        for (i, item) in items.iter().enumerate() {
            *count += 1;
            let item_path = append_path(path, i as u32 + 1);
            let key = identifier_string(&item_path);
            if let Some(vt) = item.get(tg::VALUE_TYPE) {
                let vt = resident_string(vt, Vr::CS).map_err(|e| numeric_error_at(e, &key))?;
                if vt.trim_matches(' ') == value::NUM
                    && let Some(r) =
                        read_numeric_macro(item).map_err(|e| numeric_error_at(e, &key))?
                {
                    metadata.entries.push(NumericRepresentationEntry {
                        path: key.clone(),
                        representation: r,
                    });
                }
            }
            visit(
                item,
                &item_path,
                depth + 1,
                max_items,
                max_depth,
                count,
                metadata,
            )?;
        }
        Ok(())
    }
    visit(
        obj,
        &[1],
        1,
        max_items,
        max_depth,
        &mut count,
        &mut metadata,
    )?;
    Ok(metadata)
}
pub(crate) fn read_document_numeric_representations(
    obj: Option<&Object>,
    read_options: ReadOptions,
) -> (ReadResult, NumericRepresentationMetadata, Option<SrError>) {
    let Some(obj) = obj else {
        return (
            ReadResult::default(),
            Default::default(),
            Some(SrError::new(Kind::NoDataset, "")),
        );
    };
    let options = super::read::normalize_read_options(read_options);
    if options.max_depth < 1
        || options.max_items < 1
        || options.max_references < 1
        || options.max_path_components < 1
        || options.max_findings < 1
    {
        return (
            ReadResult::default(),
            Default::default(),
            Some(numeric_error(NUMERIC_CODE_LIMIT, "")),
        );
    }
    let numeric = match read_numeric_metadata(obj, options.max_items, options.max_depth) {
        Ok(n) => n,
        Err(e) => return (ReadResult::default(), Default::default(), Some(e)),
    };
    let mut decode = read_options;
    decode.resolve_references = false;
    decode.validate_relationships = false;
    let (mut result, err) = read_document_with_options(Some(obj), decode);
    if let Some(e) = err {
        return (result, Default::default(), Some(e));
    }
    let by_path: HashMap<&str, &NumericRepresentation> = numeric
        .entries
        .iter()
        .map(|e| (e.path.as_str(), &e.representation))
        .collect();
    if let Some(doc) = &mut result.document
        && let Err(e) = apply_numeric(&mut doc.content, &[1], &by_path)
    {
        return (result, Default::default(), Some(e));
    }
    if options.resolve_references
        && let Some(doc) = &result.document
    {
        let mut ro = super::reference::default_reference_options();
        ro.mode = options.reference_mode;
        ro.max_depth = options.max_depth;
        ro.max_items = options.max_items;
        ro.max_references = options.max_references;
        ro.max_path_components = options.max_path_components;
        ro.max_findings = options.max_findings;
        let (index, report, err) = super::reference::resolve_references(Some(doc), ro);
        result.references = index;
        result.report = report;
        if let Some(e) = err {
            return (result, Default::default(), Some(e));
        }
    }
    if options.validate_relationships
        && let Some(doc) = &result.document
    {
        let mut mode = options.relationship_mode;
        if mode.0 == 0 {
            mode = super::reference::ValidationMode::WARN;
        }
        let (report, err) = super::relationship::validate_relationships(
            Some(doc),
            super::relationship::RelationshipOptions {
                mode,
                max_depth: options.max_depth,
                max_items: options.max_items,
                max_findings: options.max_findings,
            },
        );
        result.report.findings.extend(report.findings);
        result.report.truncated = result.report.truncated || report.truncated;
        if let Some(e) = err
            && !e.is(Kind::UnconstrainedSopClass)
        {
            return (result, Default::default(), Some(e));
        }
    }
    (result, numeric, None)
}
pub(crate) fn apply_numeric(
    items: &mut [ContentItem],
    parent: &[u32],
    by_path: &HashMap<&str, &NumericRepresentation>,
) -> Result<(), SrError> {
    let mut order: Vec<Vec<usize>> = Vec::new();
    fn walk_order(items: &[ContentItem], prefix: Vec<usize>, out: &mut Vec<Vec<usize>>) {
        let mut stack: Vec<(&[ContentItem], Vec<usize>, usize)> = vec![(items, prefix, 0)];
        while let Some(frame) = stack.last_mut() {
            if frame.2 >= frame.0.len() {
                stack.pop();
                continue;
            }
            let i = frame.2;
            frame.2 += 1;
            let mut p = frame.1.clone();
            p.push(i);
            out.push(p.clone());
            let item = &frame.0[i];
            if !item.children.is_empty() {
                stack.push((&item.children, p, 0));
            }
        }
    }
    walk_order(items, Vec::new(), &mut order);
    for index_path in order {
        let mut item = &mut items[index_path[0]];
        for &i in &index_path[1..] {
            item = &mut item.children[i];
        }
        let mut path = parent.to_vec();
        path.extend(index_path.iter().map(|i| *i as u32 + 1));
        let key = identifier_string(&path);
        if let Some(r) = by_path.get(key.as_str()) {
            let Some(m) = &mut item.measurement else {
                return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &key));
            };
            m.value = r.resolve().map(|x| x.0).unwrap_or(0.0);
        }
    }
    Ok(())
}
pub(crate) fn restore_numeric_representations(
    elements: &mut [Element],
    items: &[ContentItem],
    parent: &[u32],
    numeric: &HashMap<String, NumericRepresentation>,
) -> Result<(), SrError> {
    if numeric.is_empty() || items.is_empty() {
        return Ok(());
    }
    let Some(seq_index) = element_index(elements, tg::CONTENT_SEQUENCE) else {
        return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, ""));
    };
    let Value::Sequence(seq) = &mut elements[seq_index].value else {
        return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, ""));
    };
    if seq.len() != items.len() {
        return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, ""));
    }
    for (i, item) in items.iter().enumerate() {
        let path = append_path(parent, i as u32 + 1);
        let key = identifier_string(&path);
        let data_set: &mut DataSet = &mut seq[i];
        if let Some(r) = numeric.get(&key) {
            let Some(index) = element_index(&data_set.elements, tg::MEASURED_VALUE_SEQ) else {
                return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &key));
            };
            let Value::Sequence(measured) = &mut data_set.elements[index].value else {
                return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &key));
            };
            if measured.len() != 1 {
                return Err(numeric_error(NUMERIC_CODE_ASSOCIATION, &key));
            }
            let target = &mut measured[0].elements;
            let raw = r.decimal_string.as_bytes().to_vec();
            super::template_metadata::replace_or_append_element(
                target,
                Element {
                    header: ElementHeader::new(tg::NUMERIC_VALUE, Vr::DS, raw.len() as u32),
                    value: Value::Raw(raw),
                },
            );
            let typed = |tag, vr, raw: Vec<u8>| Element {
                header: ElementHeader {
                    tag,
                    vr,
                    length: 0,
                    length_set: false,
                },
                value: Value::Raw(raw),
            };
            if r.has_floating_point {
                super::template_metadata::replace_or_append_element(
                    target,
                    typed(
                        TAG_FLOATING_POINT_VALUE,
                        Vr::FD,
                        r.floating_point.to_bits().to_le_bytes().to_vec(),
                    ),
                );
            }
            if r.has_rational {
                super::template_metadata::replace_or_append_element(
                    target,
                    typed(
                        TAG_RATIONAL_NUMERATOR_VALUE,
                        Vr::SL,
                        r.rational_numerator.to_le_bytes().to_vec(),
                    ),
                );
                super::template_metadata::replace_or_append_element(
                    target,
                    typed(
                        TAG_RATIONAL_DENOMINATOR_VALUE,
                        Vr::UL,
                        r.rational_denominator.to_le_bytes().to_vec(),
                    ),
                );
            }
            target.sort_by_key(|e| e.tag().key());
        }
        restore_numeric_representations(&mut data_set.elements, &item.children, &path, numeric)?;
    }
    Ok(())
}
