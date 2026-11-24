#![forbid(unsafe_code)]
use super::dict::{self, Context};
use super::error::Error;
use super::file::{ReadFileOptions, read_data_set_with_options};
use super::obj::Object;
use super::parser::source::MemFile;
use super::transfer::{self, Syntax};
use super::types::{DataSet, Element, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::dictionary::reservations::{
    DEFAULT_MAX_PRIVATE_CREATORS, PrivateError, PrivateReservations, is_private_creator_tag,
    is_private_data_tag,
};
use std::fmt;
use std::rc::Rc;
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_LUT_DESCRIPTOR: Tag = Tag::new(0x0028, 0x3002);
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UnReason {
    Recovered,
    RecoveredSequence,
    RecoveredPrivateCreator,
    RecoveredLongShortVr,
    RecoveredUndefinedSequence,
    UnknownAttribute,
    DictionaryUn,
    PrivateNoCreator,
    PrivateNoCatalog,
    FileMetaGroup,
    AmbiguousVr,
    ContextMissing,
    DeferredValue,
    LengthMismatch,
    InvalidSequence,
    SequenceForScalar,
    LimitExceeded,
}
impl UnReason {
    pub fn as_str(self) -> &'static str {
        match self {
            UnReason::Recovered => "recovered",
            UnReason::RecoveredSequence => "recovered-sequence",
            UnReason::RecoveredPrivateCreator => "private-creator-encoded-as-UN",
            UnReason::RecoveredLongShortVr => "recovered-value-exceeds-16-bit-explicit-length",
            UnReason::RecoveredUndefinedSequence => "recovered-undefined-length-sequence",
            UnReason::UnknownAttribute => "unknown-attribute",
            UnReason::DictionaryUn => "dictionary-vr-is-UN",
            UnReason::PrivateNoCreator => "private-creator-unresolved",
            UnReason::PrivateNoCatalog => "private-attribute-not-in-catalog",
            UnReason::FileMetaGroup => "file-meta-group-not-recovered",
            UnReason::AmbiguousVr => "ambiguous-vr",
            UnReason::ContextMissing => "contextual-vr-without-context",
            UnReason::DeferredValue => "deferred-value",
            UnReason::LengthMismatch => "length-not-a-multiple-of-vr-size",
            UnReason::InvalidSequence => "invalid-sequence-payload",
            UnReason::SequenceForScalar => "sequence-encoding-for-non-sequence-vr",
            UnReason::LimitExceeded => "recovery-limit-exceeded",
        }
    }
    pub fn is_unsafe(self) -> bool {
        matches!(
            self,
            UnReason::AmbiguousVr
                | UnReason::ContextMissing
                | UnReason::DeferredValue
                | UnReason::LengthMismatch
                | UnReason::InvalidSequence
                | UnReason::SequenceForScalar
                | UnReason::LimitExceeded
                | UnReason::PrivateNoCreator
        )
    }
}
impl fmt::Display for UnReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnDiagnostic {
    pub path: String,
    pub tag: Tag,
    pub vr: Vr,
    pub recovered: bool,
    pub reason: UnReason,
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UnRecoveryReport {
    pub recovered: usize,
    pub kept: usize,
    pub diagnostics: Vec<UnDiagnostic>,
    pub diagnostics_truncated: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UnRecoveryOptions {
    pub max_element_bytes: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
    pub max_private_creators: i64,
    pub max_diagnostics: i64,
    pub strict: bool,
}
impl UnRecoveryOptions {
    pub fn defaults() -> UnRecoveryOptions {
        UnRecoveryOptions {
            max_element_bytes: 64 << 20,
            max_elements: 100_000,
            max_sequence_depth: 64,
            max_private_creators: DEFAULT_MAX_PRIVATE_CREATORS as i64,
            max_diagnostics: 256,
            strict: false,
        }
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnRecoveryError {
    NegativeLimits,
    Private(PrivateError),
    Unrecoverable {
        path: String,
        tag: Tag,
        reason: UnReason,
    },
}
impl fmt::Display for UnRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnRecoveryError::NegativeLimits => {
                f.write_str("dicom: UN recovery limits must not be negative")
            }
            UnRecoveryError::Private(err) => err.fmt(f),
            UnRecoveryError::Unrecoverable { path, reason, .. } => {
                write!(f, "dicom: UN element cannot be recovered: {path}: {reason}")
            }
        }
    }
}
impl std::error::Error for UnRecoveryError {}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnRecoveryFailure {
    pub error: UnRecoveryError,
    pub report: UnRecoveryReport,
}
impl fmt::Display for UnRecoveryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for UnRecoveryFailure {}
pub fn recover_un(
    o: &Object,
    opts: &UnRecoveryOptions,
) -> Result<(Object, UnRecoveryReport), UnRecoveryFailure> {
    recover_un_impl(o, opts, None)
}
pub(crate) type PrivateVrLookup<'a> = &'a dyn Fn(&str, Tag) -> Option<Vr>;
pub(crate) fn recover_un_with_private(
    o: &Object,
    opts: &UnRecoveryOptions,
    private: PrivateVrLookup<'_>,
) -> Result<(Object, UnRecoveryReport), UnRecoveryFailure> {
    recover_un_impl(o, opts, Some(private))
}
fn recover_un_impl(
    o: &Object,
    opts: &UnRecoveryOptions,
    private: Option<PrivateVrLookup<'_>>,
) -> Result<(Object, UnRecoveryReport), UnRecoveryFailure> {
    let mut opts = *opts;
    if opts.max_element_bytes < 0
        || opts.max_elements < 0
        || opts.max_sequence_depth < 0
        || opts.max_private_creators < 0
        || opts.max_diagnostics < 0
    {
        return Err(UnRecoveryFailure {
            error: UnRecoveryError::NegativeLimits,
            report: UnRecoveryReport::default(),
        });
    }
    let defaults = UnRecoveryOptions::defaults();
    if opts.max_element_bytes == 0 {
        opts.max_element_bytes = defaults.max_element_bytes;
    }
    if opts.max_elements == 0 {
        opts.max_elements = defaults.max_elements;
    }
    if opts.max_sequence_depth == 0 {
        opts.max_sequence_depth = defaults.max_sequence_depth;
    }
    if opts.max_private_creators == 0 {
        opts.max_private_creators = defaults.max_private_creators;
    }
    if opts.max_diagnostics == 0 {
        opts.max_diagnostics = defaults.max_diagnostics;
    }
    let mut r = Recoverer {
        big_endian: o.value_big_endian(),
        opts,
        budget: opts.max_elements,
        report: UnRecoveryReport::default(),
        private,
    };
    match r.data_set(o.elements().to_vec(), "", 0, &[]) {
        Ok(elements) => {
            let mut out =
                Object::from_elements_with_text_options(elements, o.text_options().clone());
            out.set_value_big_endian(o.value_big_endian());
            Ok((out, r.report))
        }
        Err(error) => Err(UnRecoveryFailure {
            error,
            report: r.report,
        }),
    }
}
#[derive(Clone, Copy)]
enum Scope {
    Absent,
    Value(u16),
    Invalid,
}
enum Change {
    Keep,
    Deferred,
    Vr(Vr),
    Data(Vr, Vec<u8>),
    Sequence(Vec<DataSet>),
}
enum Lookup {
    Vr(Vr),
    Keep(UnReason),
    Contextual(Context),
}
struct Recoverer<'a> {
    big_endian: bool,
    opts: UnRecoveryOptions,
    budget: i64,
    report: UnRecoveryReport,
    private: Option<PrivateVrLookup<'a>>,
}
fn un_path(parent: &str, tag: Tag) -> String {
    if parent.is_empty() {
        tag.to_string()
    } else {
        format!("{parent}.{tag}")
    }
}
impl Recoverer<'_> {
    fn note(
        &mut self,
        path: String,
        tag: Tag,
        vr: Vr,
        reason: UnReason,
    ) -> Result<(), UnRecoveryError> {
        let recovered = vr != Vr::UN;
        if recovered {
            self.report.recovered += 1;
        } else {
            self.report.kept += 1;
        }
        if (self.report.diagnostics.len() as i64) < self.opts.max_diagnostics {
            self.report.diagnostics.push(UnDiagnostic {
                path: path.clone(),
                tag,
                vr,
                recovered,
                reason,
            });
        } else {
            self.report.diagnostics_truncated = true;
        }
        if !recovered && self.opts.strict && reason.is_unsafe() {
            return Err(UnRecoveryError::Unrecoverable { path, tag, reason });
        }
        Ok(())
    }
    fn data_set(
        &mut self,
        mut out: Vec<Element>,
        path: &str,
        depth: i64,
        ancestors: &[Scope],
    ) -> Result<Vec<Element>, UnRecoveryError> {
        for element in out.iter_mut() {
            let tag = element.tag();
            if element.vr() != Vr::UN
                || !is_private_creator_tag(tag)
                || !matches!(element.value, Value::Raw(_))
            {
                continue;
            }
            element.header.vr = Vr::LO;
            self.note(
                un_path(path, tag),
                tag,
                Vr::LO,
                UnReason::RecoveredPrivateCreator,
            )?;
        }
        let reservations =
            PrivateReservations::from_elements(&out, self.opts.max_private_creators as usize)
                .map_err(UnRecoveryError::Private)?;
        let mut pending = vec![false; out.len()];
        for i in 0..out.len() {
            if out[i].vr() != Vr::UN || is_private_creator_tag(out[i].tag()) {
                continue;
            }
            let change = self.primitive(i, &out, path, depth, &reservations, None)?;
            pending[i] = matches!(change, Change::Deferred);
            apply(&mut out[i], change);
        }
        let mut scope = Vec::with_capacity(ancestors.len() + 1);
        scope.push(pixel_representation(&out, self.big_endian));
        scope.extend_from_slice(ancestors);
        for i in 0..out.len() {
            if !pending[i] {
                continue;
            }
            let change = self.primitive(i, &out, path, depth, &reservations, Some(&scope))?;
            apply(&mut out[i], change);
        }
        for element in out.iter_mut() {
            let tag = element.tag();
            let Value::Sequence(items) = &mut element.value else {
                continue;
            };
            let element_path = un_path(path, tag);
            if depth + 1 > self.opts.max_sequence_depth {
                self.note(element_path, tag, Vr::UN, UnReason::LimitExceeded)?;
                continue;
            }
            for (j, item) in items.iter_mut().enumerate() {
                let elements = std::mem::take(&mut item.elements);
                item.elements =
                    self.data_set(elements, &format!("{element_path}[{j}]"), depth + 1, &scope)?;
            }
        }
        Ok(out)
    }
    fn primitive(
        &mut self,
        index: usize,
        siblings: &[Element],
        path: &str,
        depth: i64,
        reservations: &PrivateReservations,
        scope: Option<&[Scope]>,
    ) -> Result<Change, UnRecoveryError> {
        let element = &siblings[index];
        let tag = element.tag();
        let element_path = un_path(path, tag);
        if tag.group == 0x0002 {
            return self.keep(element_path, tag, UnReason::FileMetaGroup);
        }
        let vr = match lookup(tag, reservations, self.private) {
            Lookup::Keep(reason) => return self.keep(element_path, tag, reason),
            Lookup::Vr(vr) => vr,
            Lookup::Contextual(context) => {
                let Some(scope) = scope else {
                    return Ok(Change::Deferred);
                };
                match resolve(element, context, siblings, scope, self.big_endian) {
                    Some(vr) => vr,
                    None => return self.keep(element_path, tag, UnReason::ContextMissing),
                }
            }
        };
        match &element.value {
            Value::Nil => self.keep(element_path, tag, UnReason::DeferredValue),
            Value::Sequence(_) => {
                if vr != Vr::SQ {
                    return self.keep(element_path, tag, UnReason::SequenceForScalar);
                }
                self.note(
                    element_path,
                    tag,
                    Vr::SQ,
                    UnReason::RecoveredUndefinedSequence,
                )?;
                Ok(Change::Vr(Vr::SQ))
            }
            Value::Raw(value) => {
                if vr == Vr::SQ {
                    return match self.sequence(tag, value, depth) {
                        Err(reason) => self.keep(element_path, tag, reason),
                        Ok(items) => {
                            self.note(element_path, tag, Vr::SQ, UnReason::RecoveredSequence)?;
                            Ok(Change::Sequence(items))
                        }
                    };
                }
                if value.len() % vr_unit_size(vr) != 0 {
                    return self.keep(element_path, tag, UnReason::LengthMismatch);
                }
                let mut change = Change::Vr(vr);
                let width = swap_width(tag, vr, siblings, self.big_endian);
                if width > 1 && self.big_endian {
                    if value.len() % width != 0 {
                        return self.keep(element_path, tag, UnReason::LengthMismatch);
                    }
                    let mut data = value.clone();
                    for unit in data.chunks_exact_mut(width) {
                        unit.reverse();
                    }
                    change = Change::Data(vr, data);
                }
                let reason = if !vr.uses_long_explicit_length() && value.len() > 0xFFFF {
                    UnReason::RecoveredLongShortVr
                } else {
                    UnReason::Recovered
                };
                self.note(element_path, tag, vr, reason)?;
                Ok(change)
            }
            _ => self.keep(element_path, tag, UnReason::AmbiguousVr),
        }
    }
    fn keep(
        &mut self,
        path: String,
        tag: Tag,
        reason: UnReason,
    ) -> Result<Change, UnRecoveryError> {
        self.note(path, tag, Vr::UN, reason)?;
        Ok(Change::Keep)
    }
    fn sequence(&mut self, tag: Tag, payload: &[u8], depth: i64) -> Result<Vec<DataSet>, UnReason> {
        if self.budget <= 0 {
            return Err(UnReason::LimitExceeded);
        }
        let syntax: Syntax = if self.big_endian {
            transfer::lookup("1.2.840.10008.1.2.2").unwrap_or(transfer::EXPLICIT_VR_LITTLE_ENDIAN)
        } else {
            transfer::EXPLICIT_VR_LITTLE_ENDIAN
        };
        let remaining_depth = self.opts.max_sequence_depth - depth;
        if remaining_depth < 2 {
            return Err(UnReason::LimitExceeded);
        }
        let u16_bytes = |v: u16| {
            if syntax.big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut stream = Vec::with_capacity(payload.len() + 20);
        stream.extend_from_slice(&u16_bytes(tag.group));
        stream.extend_from_slice(&u16_bytes(tag.element));
        stream.extend_from_slice(b"UN\0\0");
        stream.extend_from_slice(&[0xFF; 4]);
        stream.extend_from_slice(payload);
        stream.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0, 0, 0, 0, 0]);
        let payload_len = payload.len() as i64;
        let element_limit = self.opts.max_element_bytes.min(payload_len);
        let parsed = read_data_set_with_options(
            Box::new(MemFile::new(Rc::from(stream))),
            syntax,
            ReadFileOptions {
                max_element_bytes: element_limit,
                max_elements: self.budget,
                max_sequence_depth: remaining_depth,
                ..Default::default()
            },
        );
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(err) => {
                if is_limit(&err, "dicom: maximum sequence nesting depth exceeded")
                    || is_limit(&err, "dicom: maximum element count exceeded")
                    || is_limit(&err, "dicom: maximum element value size exceeded")
                        && self.opts.max_element_bytes < payload_len
                {
                    return Err(UnReason::LimitExceeded);
                }
                return Err(UnReason::InvalidSequence);
            }
        };
        let [element] = parsed.elements() else {
            return Err(UnReason::InvalidSequence);
        };
        let Value::Sequence(items) = &element.value else {
            return Err(UnReason::InvalidSequence);
        };
        self.budget -= count_elements(items);
        Ok(items.clone())
    }
}
fn apply(element: &mut Element, change: Change) {
    match change {
        Change::Keep | Change::Deferred => {}
        Change::Vr(vr) => element.header.vr = vr,
        Change::Data(vr, data) => {
            element.header.vr = vr;
            element.value = Value::Raw(data);
        }
        Change::Sequence(items) => {
            element.header.vr = Vr::SQ;
            element.value = Value::Sequence(items);
        }
    }
}
fn is_limit(err: &Error, sentinel: &str) -> bool {
    err.message().contains(sentinel)
}
fn lookup(
    tag: Tag,
    reservations: &PrivateReservations,
    private: Option<PrivateVrLookup<'_>>,
) -> Lookup {
    if is_private_data_tag(tag) {
        let Ok(creator) = reservations.creator(tag) else {
            return Lookup::Keep(UnReason::PrivateNoCreator);
        };
        match private.and_then(|p| p(&creator, tag)) {
            Some(Vr::UN) => return Lookup::Keep(UnReason::DictionaryUn),
            Some(vr) => return Lookup::Vr(vr),
            None => {}
        }
        return match dict::by_tag(tag) {
            None => Lookup::Keep(UnReason::PrivateNoCatalog),
            Some(entry) if entry.vr == Vr::UN => Lookup::Keep(UnReason::DictionaryUn),
            Some(entry) => Lookup::Vr(entry.vr),
        };
    }
    let Some(entry) = dict::by_tag(tag) else {
        return Lookup::Keep(UnReason::UnknownAttribute);
    };
    match entry.context {
        None => {}
        Some(Context::Up) => return Lookup::Vr(Vr::UL),
        Some(context) => return Lookup::Contextual(context),
    }
    match dict::vr_values(entry) {
        [Vr::UN] => Lookup::Keep(UnReason::DictionaryUn),
        [vr] => Lookup::Vr(*vr),
        _ => Lookup::Keep(UnReason::AmbiguousVr),
    }
}
fn resolve(
    element: &Element,
    context: Context,
    siblings: &[Element],
    scope: &[Scope],
    big_endian: bool,
) -> Option<Vr> {
    match context {
        Context::Xs => {
            for pr in scope {
                match pr {
                    Scope::Invalid => return None,
                    Scope::Absent => continue,
                    Scope::Value(1) => return Some(Vr::SS),
                    Scope::Value(_) => return Some(Vr::US),
                }
            }
            None
        }
        Context::Ox | Context::Px => {
            if element.tag() != Tag::PIXEL_DATA {
                return Some(Vr::OW);
            }
            if matches!(element.value, Value::Fragments { .. })
                || element.header.length == UNDEFINED_LENGTH && element.header.length_set
            {
                return Some(Vr::OB);
            }
            let bits = uint16_value(siblings, TAG_BITS_ALLOCATED, big_endian).ok()??;
            Some(if bits <= 8 { Vr::OB } else { Vr::OW })
        }
        Context::Lt => {
            let entries = uint16_value(siblings, TAG_LUT_DESCRIPTOR, big_endian).ok()??;
            Some(if entries == 1 { Vr::US } else { Vr::OW })
        }
        Context::Up => None,
    }
}
fn pixel_representation(elements: &[Element], big_endian: bool) -> Scope {
    match uint16_value(elements, TAG_PIXEL_REPRESENTATION, big_endian) {
        Err(()) => Scope::Invalid,
        Ok(None) => Scope::Absent,
        Ok(Some(v)) if v > 1 => Scope::Invalid,
        Ok(Some(v)) => Scope::Value(v),
    }
}
fn uint16_value(elements: &[Element], tag: Tag, big_endian: bool) -> Result<Option<u16>, ()> {
    let Some(element) = elements.iter().find(|e| e.tag() == tag) else {
        return Ok(None);
    };
    match &element.value {
        Value::Raw(raw) if raw.len() >= 2 && raw.len() % 2 == 0 => {
            let b = [raw[0], raw[1]];
            Ok(Some(if big_endian {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            }))
        }
        Value::Nil => Ok(None),
        _ => Err(()),
    }
}
fn count_elements(items: &[DataSet]) -> i64 {
    let mut n = 0;
    for item in items {
        for element in &item.elements {
            n += 1;
            if let Value::Sequence(nested) = &element.value {
                n += count_elements(nested);
            }
        }
    }
    n
}
fn swap_width(tag: Tag, vr: Vr, siblings: &[Element], big_endian: bool) -> usize {
    let width = vr.byte_order_width();
    if tag != Tag::PIXEL_DATA || vr != Vr::OW {
        return width;
    }
    match uint16_value(siblings, TAG_BITS_ALLOCATED, big_endian) {
        Ok(Some(bits)) if bits == 32 || bits == 64 => usize::from(bits / 8),
        _ => width,
    }
}
fn vr_unit_size(vr: Vr) -> usize {
    if vr == Vr::AT {
        4
    } else {
        vr.byte_order_width()
    }
}
