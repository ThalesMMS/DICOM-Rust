#![forbid(unsafe_code)]
use super::error::Error;
use super::file::File;
use super::obj::Object;
use super::types::{DataSet, Element, Tag, Value};
use super::writer::ElementStream;
use crate::charset::SpecificCharacterSet;
use crate::validation::write::{WriteError, WriteLimits, WriteOperation};
use crate::validation::{
    self as v, ChangeKind, HookEvent, HookPoint, NO_ITEM, Path, PathStep, Report,
    ValidationOptions, ValidationResult,
};
use std::fmt;
pub const ERR_PARTIAL_FILE: &str = "dicom: file is the result of a partial read";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileValidationError {
    PartialFile,
    Validation(v::Error),
}
impl fmt::Display for FileValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileValidationError::PartialFile => f.write_str(ERR_PARTIAL_FILE),
            FileValidationError::Validation(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for FileValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FileValidationError::Validation(e) => std::error::Error::source(e),
            FileValidationError::PartialFile => None,
        }
    }
}
fn element_data_set(elements: Vec<Element>) -> DataSet {
    DataSet {
        elements,
        item_offset: None,
    }
}
pub fn validate_data_set(
    obj: &Object,
    mut opts: ValidationOptions,
) -> (ValidationResult, Option<v::Error>) {
    opts.dictionary = true;
    opts.big_endian = obj.value_big_endian();
    v::validate_data_set_with(&element_data_set(obj.elements().to_vec()), &opts)
}
pub fn validate_file(
    file: &File,
    mut opts: ValidationOptions,
) -> (Report, Option<FileValidationError>) {
    if file.incomplete_pixel_data.is_some() || file.partial_read.is_some() {
        return (Report::default(), Some(FileValidationError::PartialFile));
    }
    opts.dictionary = true;
    opts.big_endian = file.dataset.value_big_endian();
    let meta = element_data_set(file.meta.elements().to_vec());
    let ds = element_data_set(file.dataset.elements().to_vec());
    let (report, err) = v::validate_file(&meta, &ds, file.transfer_syntax.uid, &opts);
    (report, err.map(FileValidationError::Validation))
}
fn selected(obj: &Object, tags: &[Tag]) -> Vec<Element> {
    tags.iter().filter_map(|&t| obj.get(t).cloned()).collect()
}
pub fn validate_part10_identity(file: &File) -> Report {
    let meta = selected(
        &file.meta,
        &[
            Tag::new(0x0002, 0x0002),
            Tag::new(0x0002, 0x0003),
            Tag::new(0x0002, 0x0010),
        ],
    );
    let ds = selected(
        &file.dataset,
        &[Tag::new(0x0008, 0x0016), Tag::new(0x0008, 0x0018)],
    );
    v::validate_file_meta_consistency(
        &element_data_set(meta),
        &element_data_set(ds),
        file.transfer_syntax.uid,
    )
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SopClassFinding {
    pub sop_class_uid: String,
    pub tag: Tag,
    pub keyword: String,
    pub message: String,
}
pub struct SopClassValidationContext<'a> {
    pub sop_class_uid: String,
    pub object: Option<&'a Object>,
}
pub trait SopClassValidationHook {
    fn validate_sop_class(&self, ctx: &SopClassValidationContext<'_>) -> Vec<SopClassFinding>;
}
impl<F> SopClassValidationHook for F
where
    F: Fn(&SopClassValidationContext<'_>) -> Vec<SopClassFinding>,
{
    fn validate_sop_class(&self, ctx: &SopClassValidationContext<'_>) -> Vec<SopClassFinding> {
        self(ctx)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequiredAttribute {
    pub tag: Tag,
    pub keyword: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequiredAttributeRule {
    pub sop_class_uid: String,
    pub attributes: Vec<RequiredAttribute>,
}
impl SopClassValidationHook for RequiredAttributeRule {
    fn validate_sop_class(&self, ctx: &SopClassValidationContext<'_>) -> Vec<SopClassFinding> {
        if !self.sop_class_uid.is_empty() && self.sop_class_uid != ctx.sop_class_uid {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for attr in &self.attributes {
            if ctx.object.is_some_and(|o| o.has(attr.tag)) {
                continue;
            }
            let label = if attr.keyword.is_empty() {
                attr.tag.to_string()
            } else {
                attr.keyword.clone()
            };
            findings.push(SopClassFinding {
                sop_class_uid: ctx.sop_class_uid.clone(),
                tag: attr.tag,
                keyword: attr.keyword.clone(),
                message: format!("missing required attribute {label} ({})", attr.tag),
            });
        }
        findings
    }
}
#[derive(Default)]
pub struct SopClassValidationOptions<'a> {
    pub sop_class_uid: String,
    pub hooks: Vec<Option<&'a dyn SopClassValidationHook>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SopClassValidationError {
    pub findings: Vec<SopClassFinding>,
}
fn write_finding(b: &mut String, f: &SopClassFinding) {
    if !f.sop_class_uid.is_empty() {
        b.push_str(&format!("SOP Class {}: ", f.sop_class_uid));
    }
    if !f.message.is_empty() {
        b.push_str(&f.message);
    } else if !f.keyword.is_empty() {
        b.push_str(&format!("{} ({})", f.keyword, f.tag));
    } else {
        b.push_str(&f.tag.to_string());
    }
}
impl fmt::Display for SopClassValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = String::new();
        match self.findings.len() {
            0 => return f.write_str("dicom: SOP Class validation failed"),
            1 => {
                b.push_str("dicom: SOP Class validation failed: ");
                write_finding(&mut b, &self.findings[0]);
            }
            n => {
                b.push_str(&format!(
                    "dicom: SOP Class validation failed with {n} findings: "
                ));
                for (i, finding) in self.findings.iter().enumerate() {
                    if i > 0 {
                        b.push_str("; ");
                    }
                    write_finding(&mut b, finding);
                }
            }
        }
        f.write_str(&b)
    }
}
impl std::error::Error for SopClassValidationError {}
pub fn validate_sop_class(
    obj: Option<&Object>,
    opts: &SopClassValidationOptions<'_>,
) -> Result<(), SopClassValidationError> {
    let mut uid = opts.sop_class_uid.clone();
    if uid.is_empty()
        && let Some(o) = obj
    {
        uid = o.get_string(Tag::new(0x0008, 0x0016)).unwrap_or_default();
    }
    let ctx = SopClassValidationContext {
        sop_class_uid: uid.clone(),
        object: obj,
    };
    let mut findings = Vec::new();
    for hook in opts.hooks.iter().flatten() {
        for mut f in hook.validate_sop_class(&ctx) {
            if f.sop_class_uid.is_empty() {
                f.sop_class_uid = uid.clone();
            }
            findings.push(f);
        }
    }
    if findings.is_empty() {
        return Ok(());
    }
    Err(SopClassValidationError { findings })
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteValidationResult {
    pub report: Report,
    pub bytes_written: i64,
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidatedWriteError {
    Object(Error),
    Validation(v::Error),
    Scope,
    HookAction(String),
    Committed {
        bytes_written: i64,
        complete: bool,
        err: Box<ValidatedWriteError>,
    },
    Item(Box<ValidatedWriteError>),
    Joined(Vec<ValidatedWriteError>),
    Write {
        result: Box<WriteValidationResult>,
        err: Box<ValidatedWriteError>,
    },
}
impl From<WriteError> for ValidatedWriteError {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::Validation(e) => ValidatedWriteError::Validation(e),
            WriteError::Scope => ValidatedWriteError::Scope,
        }
    }
}
impl From<v::Error> for ValidatedWriteError {
    fn from(e: v::Error) -> Self {
        ValidatedWriteError::Validation(e)
    }
}
impl fmt::Display for ValidatedWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidatedWriteError::Object(e) => e.fmt(f),
            ValidatedWriteError::Validation(e) => e.fmt(f),
            ValidatedWriteError::Scope => WriteError::Scope.fmt(f),
            ValidatedWriteError::HookAction(d) => write!(
                f,
                "dicom validation hook action is invalid for this lifecycle point: {d}"
            ),
            ValidatedWriteError::Committed {
                bytes_written,
                complete,
                err,
            } => write!(
                f,
                "dicom validation write failed after {bytes_written} committed bytes (complete={complete}): {err}"
            ),
            ValidatedWriteError::Item(err) => {
                let text = ElementStream::item_error(Error::msg("\u{0}")).to_string();
                let prefix = text.strip_suffix('\u{0}').unwrap_or(&text);
                write!(f, "{prefix}{err}")
            }
            ValidatedWriteError::Joined(list) => {
                let texts: Vec<String> = list.iter().map(|e| e.to_string()).collect();
                f.write_str(&texts.join("\n"))
            }
            ValidatedWriteError::Write { result, err } => write!(
                f,
                "dicom validated data set write failed after {} committed bytes (complete={}): {err}",
                result.bytes_written, result.complete
            ),
        }
    }
}
impl std::error::Error for ValidatedWriteError {}
impl ValidatedWriteError {
    fn any(&self, test: &dyn Fn(&ValidatedWriteError) -> bool) -> bool {
        if test(self) {
            return true;
        }
        match self {
            ValidatedWriteError::Committed { err, .. }
            | ValidatedWriteError::Item(err)
            | ValidatedWriteError::Write { err, .. } => err.any(test),
            ValidatedWriteError::Joined(list) => list.iter().any(|e| e.any(test)),
            _ => false,
        }
    }
    pub fn is_validation(&self, test: &dyn Fn(&v::Error) -> bool) -> bool {
        self.any(&|e| matches!(e, ValidatedWriteError::Validation(v) if test(v)))
    }
    pub fn is_hook_action(&self) -> bool {
        self.any(&|e| {
            matches!(e, ValidatedWriteError::HookAction(_))
                || matches!(
                    e,
                    ValidatedWriteError::Validation(v::Error::Hook { action: true, .. })
                )
        })
    }
    pub fn is_scope(&self) -> bool {
        self.any(&|e| matches!(e, ValidatedWriteError::Scope))
    }
}
#[derive(Clone, Copy)]
struct Level {
    proof: bool,
    context_proof: bool,
}
enum Leaf<'a> {
    Object,
    Validated,
    Deferred(Option<DeferredValueWriter<'a>>),
}
pub type DeferredValueWriter<'a> =
    &'a mut dyn FnMut(&Element, &mut Vec<u8>) -> (i64, Option<Error>);
struct ValidatedWriter<'a, 'b> {
    op: &'a mut WriteOperation,
    stream: &'a mut ElementStream,
    has_pre_hooks: bool,
    leaf: Leaf<'b>,
    current: Option<Element>,
}
fn join(errs: Vec<ValidatedWriteError>) -> Option<ValidatedWriteError> {
    match errs.len() {
        0 => None,
        1 => errs.into_iter().next(),
        _ => Some(ValidatedWriteError::Joined(errs)),
    }
}
struct Prepared {
    element: Element,
    path: Path,
    offset: i64,
    filtered: bool,
}
impl ValidatedWriter<'_, '_> {
    fn prepare(
        &mut self,
        element: &Element,
        mut path: Path,
        validate_rules: bool,
        level: &mut Level,
        context: &mut v::write::WriteContext,
        ordinal: usize,
    ) -> Result<Prepared, ValidatedWriteError> {
        let original = element.clone();
        let mut element = element.clone();
        let start = self.stream.position();
        let event = HookEvent {
            point: HookPoint::PreSerialization,
            path: path.clone(),
            header: Some(element.header),
            element: self.has_pre_hooks.then(|| element.clone()),
            offset: start,
            offset_set: true,
            ..HookEvent::default()
        };
        let (pre, err) = self.op.handle_write(event);
        if let Some(e) = err {
            return Err(e.into());
        }
        if let Some(replacement) = &pre.element {
            element = replacement.clone();
            if matches!(original.value, Value::Nil)
                && matches!(element.value, Value::Nil)
                && element.header != original.header
            {
                return Err(ValidatedWriteError::HookAction(
                    "deferred element replacement without a value must preserve its header".into(),
                ));
            }
            if let Some(last) = path.last_mut() {
                last.tag = element.tag();
            }
        }
        let changed = pre
            .changes
            .iter()
            .any(|c| matches!(c.kind, ChangeKind::Transformed | ChangeKind::Filtered));
        if changed {
            self.op.apply_change(
                Some(context),
                crate::validation::write::ElementChange {
                    index: ordinal as i64,
                    original: &original,
                    current: &element,
                    filtered: pre.filter,
                    prior_proof: level.proof || level.context_proof,
                    path: &path,
                },
            )?;
            level.proof = false;
        }
        context.mark(ordinal);
        let prepared = Prepared {
            element,
            path,
            offset: start,
            filtered: pre.filter,
        };
        if prepared.filtered {
            return Ok(prepared);
        }
        if validate_rules || changed {
            if changed {
                let charset: Option<SpecificCharacterSet> = Some(self.stream.charset());
                self.op
                    .validate_changed_element(&prepared.element, &charset)?;
            } else {
                self.op.validate_element(&prepared.element)?;
            }
            if matches!(prepared.element.value, Value::Sequence(_)) {
                level.context_proof = true;
            }
        }
        Ok(prepared)
    }
    #[allow(clippy::too_many_arguments)]
    fn write_element(
        &mut self,
        element: &Element,
        path: Path,
        validate_rules: bool,
        level: &mut Level,
        context: &mut v::write::WriteContext,
        ordinal: usize,
        top_level: bool,
    ) -> Result<(), ValidatedWriteError> {
        let prepared = self.prepare(element, path, validate_rules, level, context, ordinal)?;
        if prepared.filtered {
            return self.finish_post_write(&prepared, None);
        }
        let write_err = self
            .write_validated(&prepared.element, &prepared.path, level, top_level)
            .err();
        self.finish_post_write(&prepared, write_err)
    }
    fn finish_post_write(
        &mut self,
        prepared: &Prepared,
        write_err: Option<ValidatedWriteError>,
    ) -> Result<(), ValidatedWriteError> {
        let delta = self.stream.position() - prepared.offset;
        let event = HookEvent {
            point: HookPoint::PostWrite,
            path: prepared.path.clone(),
            header: Some(prepared.element.header),
            offset: prepared.offset,
            offset_set: true,
            bytes_written: delta,
            write_complete: write_err.is_none(),
            ..HookEvent::default()
        };
        let (_, hook_err) = self.op.handle_write(event);
        if write_err.is_none() && hook_err.is_none() {
            return Ok(());
        }
        let complete = write_err.is_none();
        let mut errs = Vec::new();
        errs.extend(write_err);
        errs.extend(hook_err.map(ValidatedWriteError::Validation));
        Err(ValidatedWriteError::Committed {
            bytes_written: delta,
            complete,
            err: Box::new(join(errs).expect("an error")),
        })
    }
    fn write_validated(
        &mut self,
        el: &Element,
        path: &Path,
        level: &Level,
        top_level: bool,
    ) -> Result<(), ValidatedWriteError> {
        if top_level && matches!(el.value, Value::Nil) && !matches!(self.leaf, Leaf::Validated) {
            let current = self.current.clone().unwrap_or_else(|| el.clone());
            let r = match &mut self.leaf {
                Leaf::Deferred(None) => self.stream.write_deferred_leaf(el, None),
                Leaf::Deferred(Some(copy)) => self
                    .stream
                    .write_deferred_leaf(el, Some(&mut |w: &mut Vec<u8>| copy(&current, w))),
                Leaf::Object | Leaf::Validated => self.stream.write_leaf(el, true),
            };
            return r.map_err(ValidatedWriteError::Object);
        }
        if !ElementStream::is_sequence(el) {
            return self
                .stream
                .write_leaf(el, false)
                .map_err(ValidatedWriteError::Object);
        }
        let items = self
            .stream
            .begin_sequence(el)
            .map_err(ValidatedWriteError::Object)?;
        for (index, item) in items.iter().enumerate() {
            let mut item_path = path.clone();
            if let Some(last) = item_path.last_mut() {
                last.item_index = index as i64;
            }
            self.write_item(item, &item_path, level)?;
        }
        self.stream.end_sequence();
        Ok(())
    }
    fn write_item(
        &mut self,
        item: &DataSet,
        path: &Path,
        parent: &Level,
    ) -> Result<(), ValidatedWriteError> {
        self.stream
            .begin_item(item)
            .map_err(ValidatedWriteError::Object)?;
        let mut context = match self.op.new_context(item, false) {
            Ok(c) => c,
            Err(e) => {
                self.stream.abandon();
                return Err(e.into());
            }
        };
        self.stream.item_header();
        let mut level = *parent;
        for (index, el) in item.elements.iter().enumerate() {
            level.proof = parent.proof;
            let mut element_path = path.clone();
            element_path.push(PathStep {
                tag: el.tag(),
                item_index: NO_ITEM,
            });
            let validate_rules = !level.proof;
            if let Err(e) = self.write_element(
                el,
                element_path,
                validate_rules,
                &mut level,
                &mut context,
                index,
                false,
            ) {
                self.stream.abandon();
                return Err(ValidatedWriteError::Item(Box::new(e)));
            }
        }
        self.stream.end_item();
        Ok(())
    }
}
pub fn write_data_set_with_validation(
    obj: &Object,
    uid: &str,
    mut opts: ValidationOptions,
    limits: WriteLimits,
) -> (Vec<u8>, WriteValidationResult, Option<ValidatedWriteError>) {
    if has_deferred_sequence_value(obj) {
        return (
            Vec::new(),
            WriteValidationResult::default(),
            Some(ValidatedWriteError::Object(Error::msg(
                super::deferred::ERR_DEFERRED_SEQUENCE_VALUE_WRITE,
            ))),
        );
    }
    let (mut stream, elements) = match ElementStream::new(obj, uid) {
        Ok(s) => s,
        Err(e) => {
            return (
                Vec::new(),
                WriteValidationResult::default(),
                Some(ValidatedWriteError::Object(e)),
            );
        }
    };
    opts.dictionary = true;
    let syntax_big = super::writer::writable_syntax(uid).is_ok_and(|s| s.big_endian);
    opts.big_endian = if obj.value_big_endian() != syntax_big {
        syntax_big
    } else {
        obj.value_big_endian()
    };
    opts.transfer_syntax_uid = uid.to_string();
    let mut op = match WriteOperation::new(opts, limits) {
        Ok(op) => op,
        Err(e) => {
            return (
                Vec::new(),
                WriteValidationResult::default(),
                Some(ValidatedWriteError::Validation(e)),
            );
        }
    };
    let has_pre_hooks = op.has_hooks(HookPoint::PreSerialization);
    let mut writer = ValidatedWriter {
        op: &mut op,
        stream: &mut stream,
        has_pre_hooks,
        leaf: Leaf::Object,
        current: None,
    };
    let write_err = write_data_set(&mut writer, &element_data_set(elements));
    writer.op.end_write();
    let deflated = writer.stream.deflated();
    let logically_complete = match &write_err {
        None => true,
        Some(ValidatedWriteError::Committed { complete, .. }) => *complete,
        Some(_) => false,
    };
    let mut result = WriteValidationResult {
        report: writer.op.report(),
        ..WriteValidationResult::default()
    };
    let (bytes, close_err) = if deflated && !logically_complete && writer.stream.position() == 0 {
        (Vec::new(), None)
    } else {
        match writer.stream.finish() {
            Ok(b) => (b, None),
            Err(e) => (Vec::new(), Some(e)),
        }
    };
    result.bytes_written = bytes.len() as i64;
    let close_failed = close_err.clone();
    let mut errs = Vec::new();
    errs.extend(write_err);
    errs.extend(close_err.map(ValidatedWriteError::Object));
    let Some(err) = join(errs) else {
        result.complete = true;
        return (bytes, result, None);
    };
    result.complete = logically_complete && close_failed.is_none();
    let wrapped = ValidatedWriteError::Write {
        result: Box::new(result.clone()),
        err: Box::new(err),
    };
    (bytes, result, Some(wrapped))
}
fn write_data_set(
    w: &mut ValidatedWriter<'_, '_>,
    data_set: &DataSet,
) -> Option<ValidatedWriteError> {
    if let Err(e) = w.op.begin_write() {
        return Some(e.into());
    }
    let (result, err) = w.op.validate_data_set(data_set.clone());
    if let Some(e) = err {
        return Some(e.into());
    }
    let start = w.stream.position();
    let mut context = match w.op.new_context(&result.data_set, true) {
        Ok(c) => c,
        Err(e) => return Some(e.into()),
    };
    let mut level = Level {
        proof: true,
        context_proof: false,
    };
    let count = result.data_set.elements.len();
    for (index, element) in result.data_set.elements.iter().enumerate() {
        level.proof = true;
        w.current = Some(element.clone());
        let path = vec![PathStep {
            tag: element.tag(),
            item_index: NO_ITEM,
        }];
        if let Err(e) = w.write_element(element, path, false, &mut level, &mut context, index, true)
        {
            let complete = match &e {
                ValidatedWriteError::Committed { complete, .. } => index == count - 1 && *complete,
                _ => false,
            };
            return Some(ValidatedWriteError::Committed {
                bytes_written: w.stream.position() - start,
                complete,
                err: Box::new(e),
            });
        }
    }
    None
}
fn has_deferred_sequence_value(obj: &Object) -> bool {
    fn nested_nil(elements: &[Element], nested: bool) -> bool {
        elements.iter().any(|e| {
            if nested
                && matches!(e.value, Value::Nil)
                && e.vr() != super::types::Vr::SQ
                && e.header.length_set
                && e.header.length != super::UNDEFINED_LENGTH
                && e.header.length > 0
            {
                return true;
            }
            match &e.value {
                Value::Sequence(items) => items.iter().any(|i| nested_nil(&i.elements, true)),
                _ => false,
            }
        })
    }
    obj.has_value_provider() && nested_nil(obj.elements(), false)
}
pub struct ValidationWriter {
    op: WriteOperation,
    stream: ElementStream,
    has_pre_hooks: bool,
}
impl ValidationWriter {
    pub fn new(
        syntax: &super::transfer::Syntax,
        mut opts: ValidationOptions,
        limits: WriteLimits,
    ) -> Result<ValidationWriter, v::Error> {
        opts.big_endian = syntax.big_endian;
        opts.transfer_syntax_uid = syntax.uid.to_string();
        let op = WriteOperation::new(opts, limits)?;
        let has_pre_hooks = op.has_hooks(HookPoint::PreSerialization);
        Ok(ValidationWriter {
            op,
            stream: ElementStream::for_syntax(syntax),
            has_pre_hooks,
        })
    }
    fn write_one(&mut self, element: &Element, leaf: Leaf<'_>) -> Result<(), ValidatedWriteError> {
        self.op.begin_write()?;
        let result = self.write_one_active(element, leaf);
        self.op.end_write();
        result
    }
    fn write_one_active(
        &mut self,
        element: &Element,
        leaf: Leaf<'_>,
    ) -> Result<(), ValidatedWriteError> {
        let mut context = self.op.new_context(
            &DataSet {
                elements: vec![element.clone()],
                item_offset: None,
            },
            false,
        )?;
        let mut level = Level {
            proof: false,
            context_proof: false,
        };
        let path = vec![PathStep {
            tag: element.tag(),
            item_index: NO_ITEM,
        }];
        let mut writer = ValidatedWriter {
            op: &mut self.op,
            stream: &mut self.stream,
            has_pre_hooks: self.has_pre_hooks,
            leaf,
            current: Some(element.clone()),
        };
        writer.write_element(element, path, true, &mut level, &mut context, 0, true)
    }
    pub fn write_element(&mut self, element: &Element) -> Result<(), ValidatedWriteError> {
        self.write_one(element, Leaf::Validated)
    }
    pub fn write_deferred_element(
        &mut self,
        element: &Element,
        copy: Option<DeferredValueWriter<'_>>,
    ) -> Result<(), ValidatedWriteError> {
        self.write_one(element, Leaf::Deferred(copy))
    }
    pub fn write_data_set(&mut self, data_set: &DataSet) -> Result<(), ValidatedWriteError> {
        self.write_data_set_with_deferred(data_set, None)
    }
    pub fn write_data_set_with_deferred(
        &mut self,
        data_set: &DataSet,
        provider: Option<DeferredValueWriter<'_>>,
    ) -> Result<(), ValidatedWriteError> {
        let mut writer = ValidatedWriter {
            op: &mut self.op,
            stream: &mut self.stream,
            has_pre_hooks: self.has_pre_hooks,
            leaf: Leaf::Deferred(provider),
            current: None,
        };
        let err = write_data_set(&mut writer, data_set);
        self.op.end_write();
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    pub fn validation_report(&self) -> Report {
        self.op.report()
    }
    pub fn bytes_written(&self) -> i64 {
        self.stream.position()
    }
    pub fn bytes(&self) -> &[u8] {
        &self.stream.out.data
    }
}
