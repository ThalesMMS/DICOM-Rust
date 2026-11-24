use super::engine::{
    DEFAULT_MAX_DEPTH, DEFAULT_MAX_ELEMENTS, Error, Mode, ReportBuilder, Session,
    ValidationOptions, ValidationResult, check_clone_limits, validate_options,
};
use super::hooks::{HookEvent, HookPoint, HookResult};
use super::operation::Operation;
use super::{CODE_DUPLICATE_ELEMENT, CODE_ELEMENT_ORDER, Change, Finding, Path, Report, Severity};
use crate::charset::SpecificCharacterSet;
use crate::object::{DataSet, Element, Tag, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) mod size {
    pub const ELEMENT: i64 = 48;
    pub const ELEMENT_HEADER: i64 = 32;
    pub const DATA_SET: i64 = 40;
    pub const TAG: i64 = 4;
    pub const PATH_STEP: i64 = 16;
    pub const FINDING: i64 = 176;
    pub const CHANGE: i64 = 64;
    pub const HOOK_EVENT: i64 = 96;
    pub const WRITE_CONTEXT: i64 = 80;
    pub const RULE_REGISTRATION: i64 = 32;
    pub const VR: i64 = 16;
    pub const STRING: i64 = 16;
    pub const BYTES: i64 = 24;
}
const MAX_INT: i64 = i64::MAX;
pub const DEFAULT_MAX_WRITE_COPIED_VALUE_BYTES: i64 = 64 << 20;
pub const DEFAULT_MAX_WRITE_WORK_BYTES: i64 = 64 << 20;
pub const DEFAULT_MAX_WRITE_SCOPE_NODES: i64 = 1_000_000;
pub const DEFAULT_MAX_WRITE_HOOK_REGISTRATIONS: i64 = 4096;
pub const DEFAULT_MAX_WRITE_RULE_REGISTRATIONS: i64 = 1024;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteLimits {
    pub max_copied_value_bytes: i64,
    pub max_work_bytes: i64,
    pub max_scope_nodes: i64,
    pub max_hook_registrations: i64,
    pub max_rule_registrations: i64,
}
fn limit() -> Error {
    Error::Limit(String::new())
}
fn policy() -> Error {
    Error::InvalidPolicy(String::new())
}
fn normalize_write_limits(mut l: WriteLimits) -> Result<WriteLimits, Error> {
    if l.max_copied_value_bytes < 0
        || l.max_work_bytes < 0
        || l.max_scope_nodes < 0
        || l.max_hook_registrations < 0
        || l.max_rule_registrations < 0
    {
        return Err(policy());
    }
    if l.max_copied_value_bytes == 0 {
        l.max_copied_value_bytes = DEFAULT_MAX_WRITE_COPIED_VALUE_BYTES;
    }
    if l.max_work_bytes == 0 {
        l.max_work_bytes = DEFAULT_MAX_WRITE_WORK_BYTES;
    }
    if l.max_scope_nodes == 0 {
        l.max_scope_nodes = DEFAULT_MAX_WRITE_SCOPE_NODES;
    }
    if l.max_hook_registrations == 0 {
        l.max_hook_registrations = DEFAULT_MAX_WRITE_HOOK_REGISTRATIONS;
    }
    if l.max_rule_registrations == 0 {
        l.max_rule_registrations = DEFAULT_MAX_WRITE_RULE_REGISTRATIONS;
    }
    Ok(l)
}
pub(crate) fn write_add(a: i64, b: i64) -> Result<i64, Error> {
    if a < 0 || b < 0 || a > MAX_INT - b {
        return Err(limit());
    }
    Ok(a + b)
}
pub(crate) fn write_product(n: i64, width: i64) -> Result<i64, Error> {
    if n < 0 {
        return Err(limit());
    }
    if width == 0 {
        return Ok(0);
    }
    if n as u64 > (MAX_INT as u64) / width as u64 {
        return Err(limit());
    }
    Ok(n * width)
}
pub(crate) fn write_scale(n: i64, multiplier: i64) -> Result<i64, Error> {
    if n < 0 || multiplier < 0 || multiplier > 0 && n > MAX_INT / multiplier {
        return Err(limit());
    }
    Ok(n * multiplier)
}
#[derive(Debug)]
pub(crate) struct WriteBudget {
    limits: WriteLimits,
    copied: i64,
    work: i64,
    nodes: i64,
    pub(crate) max_depth: i64,
    pub(crate) max_elements: i64,
}
pub(crate) type SharedBudget = Rc<RefCell<WriteBudget>>;
#[derive(Default)]
struct Footprint {
    copied: i64,
    work: i64,
    scratch: i64,
    path_work: i64,
    nodes: i64,
}
impl WriteBudget {
    pub(crate) fn reserve(&mut self, copied: i64, work: i64, nodes: i64) -> Result<(), Error> {
        if copied < 0
            || work < 0
            || nodes < 0
            || copied > self.limits.max_copied_value_bytes - self.copied
            || work > self.limits.max_work_bytes - self.work
            || nodes > self.limits.max_scope_nodes - self.nodes
        {
            return Err(limit());
        }
        self.copied += copied;
        self.work += work;
        self.nodes += nodes;
        Ok(())
    }
    fn scan(&self, ds: &DataSet) -> Result<Footprint, Error> {
        let mut f = Footprint::default();
        let mut visited = 0i64;
        self.scan_walk(ds, 0, &mut f, &mut visited)?;
        Ok(f)
    }
    fn scan_walk(
        &self,
        ds: &DataSet,
        depth: i64,
        f: &mut Footprint,
        visited: &mut i64,
    ) -> Result<(), Error> {
        let add = |target: &mut i64, n: i64| -> Result<(), Error> {
            *target = write_add(*target, n)?;
            Ok(())
        };
        if depth > self.max_depth {
            return Err(limit());
        }
        let n = ds.elements.len() as i64;
        if n > self.max_elements - *visited || n > self.limits.max_scope_nodes - f.nodes {
            return Err(limit());
        }
        *visited += n;
        f.nodes += n;
        add(&mut f.work, write_product(n, size::ELEMENT)?)?;
        for el in &ds.elements {
            let p = write_product(depth + 1, size::PATH_STEP * 16)?;
            add(&mut f.path_work, p)?;
            let mut payload = 0i64;
            let mut structure = 0i64;
            let mut text = 0i64;
            let mut components = 0i64;
            match &el.value {
                Value::Nil | Value::BulkData(_) => {}
                Value::Raw(raw) => {
                    payload = raw.len() as i64;
                    if el.vr().is_string_like() {
                        text = raw.len() as i64;
                        components = raw.len() as i64 + 1;
                    }
                }
                Value::Strings(values) => {
                    structure = write_product(values.len() as i64, size::STRING)?;
                    components = values.len() as i64;
                    if structure > self.limits.max_work_bytes
                        || components > self.limits.max_work_bytes / 32
                    {
                        return Err(limit());
                    }
                    for s in values {
                        add(&mut text, s.len() as i64)?;
                    }
                }
                Value::Sequence(items) => {
                    let n = items.len() as i64;
                    if n > self.max_elements - *visited || n > self.limits.max_scope_nodes - f.nodes
                    {
                        return Err(limit());
                    }
                    *visited += n;
                    f.nodes += n;
                    structure = write_product(n, size::DATA_SET)?;
                    for item in items {
                        self.scan_walk(item, depth + 1, f, visited)?;
                    }
                }
                Value::Fragments {
                    offset_table,
                    fragments,
                } => {
                    let n = write_product(fragments.len() as i64, size::BYTES)?;
                    if n > self.limits.max_work_bytes {
                        return Err(limit());
                    }
                    structure = n;
                    payload = offset_table.len() as i64;
                    for fragment in fragments {
                        add(&mut payload, fragment.len() as i64)?;
                    }
                }
            }
            add(&mut f.copied, payload)?;
            add(&mut f.work, structure)?;
            if text > 0 || components > 0 {
                if text > MAX_INT / 32 || components > MAX_INT / 32 {
                    return Err(limit());
                }
                let scratch = write_add(text * 32, components * 32)?;
                add(&mut f.scratch, scratch)?;
                if f.scratch > self.limits.max_work_bytes {
                    return Err(limit());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn admit_copy(&mut self, ds: &DataSet, multiplier: i64) -> Result<(), Error> {
        let f = self.scan(ds)?;
        let m = multiplier.max(1);
        if multiplier < 0 || f.copied > MAX_INT / m || f.work > MAX_INT / m {
            return Err(limit());
        }
        self.reserve(f.copied * multiplier, f.work * multiplier, 0)
    }
    fn admit_traversal(&mut self, ds: &DataSet) -> Result<(), Error> {
        let f = self.scan(ds)?;
        let per = write_product(f.nodes, 128)?;
        let mut work = write_add(f.work, per)?;
        work = write_add(work, f.path_work)?;
        work = write_add(work, f.scratch)?;
        self.reserve(0, work, f.nodes)
    }
    pub(crate) fn admit_event(&mut self, event: &HookEvent) -> Result<(), Error> {
        if let Some(element) = &event.element {
            self.admit_copy(&single(element.clone()), 1)?;
        }
        if let Some(ds) = &event.data_set {
            self.admit_copy(ds, 1)?;
        }
        let bytes = write_product(event.path.len() as i64, size::PATH_STEP)?;
        let mut total = write_scale(bytes, 16)?;
        total = write_add(
            total,
            size::HOOK_EVENT + size::ELEMENT_HEADER + size::FINDING * 4,
        )?;
        self.reserve(0, total, 0)
    }
    pub(crate) fn admit_report(&mut self, r: &Report) -> Result<(), Error> {
        let mut bytes = write_product((r.findings.len() + r.changes.len()) as i64, size::FINDING)?;
        let path_bytes = |path: &Path| -> Result<i64, Error> {
            write_scale(write_product(path.len() as i64, size::PATH_STEP)?, 3)
        };
        for f in &r.findings {
            bytes = write_add(bytes, path_bytes(&f.path)?)?;
            let n = write_scale(write_product(f.expected_vr.len() as i64, size::VR)?, 3)?;
            bytes = write_add(bytes, n)?;
        }
        for c in &r.changes {
            bytes = write_add(bytes, path_bytes(&c.path)?)?;
        }
        bytes = write_scale(bytes, 12)?;
        self.reserve(0, bytes, 0)
    }
    pub(crate) fn admit_finding(&mut self, f: &Finding, count: i64) -> Result<(), Error> {
        let bytes = write_product(count, size::FINDING)?;
        let paths = write_product(f.path.len() as i64, size::PATH_STEP)?;
        let expected = write_product(f.expected_vr.len() as i64, size::VR)?;
        let total = write_add(write_add(bytes, paths)?, expected)?;
        self.reserve(0, total, 0)
    }
    pub(crate) fn admit_change(&mut self, c: &Change, count: i64) -> Result<(), Error> {
        let bytes = write_product(count, size::CHANGE)?;
        let paths = write_product(c.path.len() as i64, size::PATH_STEP)?;
        self.reserve(0, write_add(bytes, paths)?, 0)
    }
}
fn single(element: Element) -> DataSet {
    DataSet {
        elements: vec![element],
        item_offset: None,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteError {
    Validation(Error),
    Scope,
}
impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Validation(e) => e.fmt(f),
            WriteError::Scope => f.write_str(
                "dicom serialization change invalidates an opaque or character-set validation scope",
            ),
        }
    }
}
impl std::error::Error for WriteError {}
impl From<Error> for WriteError {
    fn from(e: Error) -> Self {
        WriteError::Validation(e)
    }
}
static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);
pub struct WriteOperation {
    op: Operation,
    limits: WriteLimits,
    budget: Option<SharedBudget>,
    config_work: i64,
    location_work: i64,
    location_nodes: i64,
    epoch: u64,
    id: u64,
}
pub struct WriteContext {
    elements: Vec<Element>,
    filtered: Vec<bool>,
    root: bool,
    processed: usize,
    owner: u64,
    epoch: u64,
}
impl WriteContext {
    pub fn mark(&mut self, index: usize) {
        if index >= self.processed {
            self.processed = index + 1;
        }
    }
}
fn depth_elements(opts: &ValidationOptions) -> (i64, i64) {
    let depth = if opts.max_depth == 0 {
        DEFAULT_MAX_DEPTH
    } else {
        opts.max_depth
    };
    let elements = if opts.max_elements == 0 {
        DEFAULT_MAX_ELEMENTS
    } else {
        opts.max_elements
    };
    (depth, elements)
}
impl WriteOperation {
    pub fn new(mut opts: ValidationOptions, limits: WriteLimits) -> Result<WriteOperation, Error> {
        let limits = normalize_write_limits(limits)?;
        if opts.data_set_rules.len() as i64 > limits.max_rule_registrations {
            return Err(limit());
        }
        let (depth, elements) = depth_elements(&opts);
        if depth < 0 || elements < 0 || depth > MAX_INT / 128 {
            return Err(policy());
        }
        let mut b = WriteBudget {
            limits,
            copied: 0,
            work: 0,
            nodes: 0,
            max_depth: depth,
            max_elements: elements,
        };
        let rule_bytes = write_product(opts.data_set_rules.len() as i64, size::RULE_REGISTRATION)?;
        let uid_bytes = write_product(opts.required_uids.len() as i64, size::TAG)?;
        let map_bytes = write_product(opts.data_set_rules.len() as i64, 64)?;
        let config = write_add(write_add(rule_bytes, uid_bytes)?, map_bytes)?;
        b.reserve(0, config, opts.required_uids.len() as i64)?;
        if let Some(chain) = &opts.hooks {
            let snapshot = chain.snapshot();
            if snapshot.len() as i64 > limits.max_hook_registrations {
                return Err(limit());
            }
            b.reserve(0, snapshot.len() as i64 * 8, 0)?;
            opts.hooks = Some(Arc::new(snapshot));
        }
        validate_options(&opts)?;
        let op = Operation::new(opts)?;
        Ok(WriteOperation {
            op,
            limits,
            budget: None,
            config_work: b.work,
            location_work: 0,
            location_nodes: 0,
            epoch: 0,
            id: NEXT_OPERATION.fetch_add(1, Ordering::Relaxed),
        })
    }
    fn opts(&self) -> &ValidationOptions {
        &self.op.opts
    }
    fn new_budget(&self) -> WriteBudget {
        let (max_depth, max_elements) = depth_elements(self.opts());
        WriteBudget {
            limits: self.limits,
            copied: 0,
            work: 0,
            nodes: 0,
            max_depth,
            max_elements,
        }
    }
    pub fn begin_write(&mut self) -> Result<(), Error> {
        if self.budget.is_some() {
            return Err(policy());
        }
        let mut b = self.new_budget();
        b.reserve(
            0,
            self.config_work + self.location_work,
            self.location_nodes,
        )?;
        self.epoch += 1;
        self.budget = Some(Rc::new(RefCell::new(b)));
        Ok(())
    }
    pub fn end_write(&mut self) {
        self.budget = None;
    }
    pub fn active(&self) -> bool {
        self.budget.is_some()
    }
    pub fn has_hooks(&self, point: HookPoint) -> bool {
        self.op.has_hooks(point)
    }
    pub fn report(&self) -> Report {
        self.op.report()
    }
    fn hook_count(&self) -> i64 {
        self.op.hooks.as_ref().map_or(0, |h| h.len() as i64)
    }
    fn active_budget(&self) -> Result<SharedBudget, Error> {
        self.budget.clone().ok_or_else(policy)
    }
    pub fn handle_write(&mut self, mut event: HookEvent) -> (HookResult, Option<Error>) {
        let budget = match self.active_budget() {
            Ok(b) => b,
            Err(e) => return (HookResult::default(), Some(e)),
        };
        if !self.has_hooks(event.point) {
            event.element = None;
            event.data_set = None;
        }
        let r = (|| -> Result<i64, Error> {
            let bytes = write_product(event.path.len() as i64, size::PATH_STEP)?;
            let location = write_add(write_scale(bytes, 16)?, 128)?;
            if self.opts().data_set_rules.len() as i64 > MAX_INT / 128 {
                return Err(limit());
            }
            let overhead = write_add(
                location,
                self.hook_count() * 8 + self.opts().data_set_rules.len() as i64 * 128 + 256,
            )?;
            budget.borrow_mut().reserve(0, overhead, 1)?;
            Ok(location)
        })();
        let location = match r {
            Ok(l) => l,
            Err(e) => return (HookResult::default(), Some(e)),
        };
        if event.offset_set && !event.path.is_empty() {
            self.location_work += location;
            self.location_nodes += 1;
        }
        let report = self.op.report.clone();
        if let Err(e) = budget.borrow_mut().admit_report(&report) {
            return (HookResult::default(), Some(e));
        }
        self.op.handle_with_budget(&event, Some(&budget))
    }
    fn add_report(&mut self, budget: &SharedBudget, report: Report) -> Result<(), Error> {
        {
            let mut b = budget.borrow_mut();
            b.reserve(0, self.opts().data_set_rules.len() as i64 * 64 + 256, 0)?;
            b.admit_report(&report)?;
            b.admit_report(&self.op.report)?;
        }
        self.op.add_report(report);
        Ok(())
    }
    pub fn validate_element(&mut self, element: &Element) -> Result<(), Error> {
        self.validate_element_in(element, &Some(SpecificCharacterSet::default_repertoire()))
    }
    pub fn validate_changed_element(
        &mut self,
        element: &Element,
        charset: &Option<SpecificCharacterSet>,
    ) -> Result<(), Error> {
        self.validate_element_in(element, charset)
    }
    fn validate_element_in(
        &mut self,
        element: &Element,
        charset: &Option<SpecificCharacterSet>,
    ) -> Result<(), Error> {
        let budget = self.active_budget()?;
        let mut opts = self.opts().clone();
        opts.hooks = None;
        opts.required_uids = Vec::new();
        let (result, err) = validate_write_view(&single(element.clone()), &opts, &budget, charset);
        self.add_report(&budget, result.report)?;
        match err {
            Some(e) if e.is_validation_failed() => Err(Error::Failed(self.op.report())),
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn stopped_result(&self, ds: DataSet) -> (ValidationResult, Option<Error>) {
        let result = ValidationResult {
            data_set: ds,
            report: self.op.report(),
        };
        if self.opts().mode == Mode::Strict && result.report.has_errors() {
            let report = result.report.clone();
            return (result, Some(Error::Failed(report)));
        }
        (result, None)
    }
    pub fn validate_data_set(&mut self, ds: DataSet) -> (ValidationResult, Option<Error>) {
        let budget = match self.active_budget() {
            Ok(b) => b,
            Err(e) => return (ValidationResult::default(), Some(e)),
        };
        if self.op.stopped {
            return self.stopped_result(ds);
        }
        let pre = HookEvent {
            point: HookPoint::PreValidation,
            data_set: Some(ds.clone()),
            ..HookEvent::default()
        };
        if let (_, Some(e)) = self.handle_write(pre) {
            return (
                ValidationResult {
                    data_set: ds,
                    report: self.op.report(),
                },
                Some(e),
            );
        }
        if self.op.stopped {
            return self.stopped_result(ds);
        }
        let mut opts = self.opts().clone();
        opts.hooks = None;
        let (mut result, rule_err) = validate_write_view(
            &ds,
            &opts,
            &budget,
            &Some(SpecificCharacterSet::default_repertoire()),
        );
        if let Err(e) = self.add_report(&budget, result.report.clone()) {
            return (ValidationResult::default(), Some(e));
        }
        if let Some(e) = rule_err
            && !e.is_validation_failed()
        {
            return (
                ValidationResult {
                    data_set: result.data_set,
                    report: self.op.report(),
                },
                Some(e),
            );
        }
        if self.op.stopped {
            return self.stopped_result(result.data_set);
        }
        for point in [HookPoint::DataSetComplete, HookPoint::PostValidation] {
            let event = HookEvent {
                point,
                data_set: Some(result.data_set.clone()),
                ..HookEvent::default()
            };
            if let (_, Some(e)) = self.handle_write(event) {
                return (
                    ValidationResult {
                        data_set: result.data_set,
                        report: self.op.report(),
                    },
                    Some(e),
                );
            }
        }
        result.report = self.op.report();
        if self.opts().mode == Mode::Strict && result.report.has_errors() {
            let report = result.report.clone();
            return (result, Some(Error::Failed(report)));
        }
        (result, None)
    }
    pub fn new_context(&mut self, ds: &DataSet, root: bool) -> Result<WriteContext, Error> {
        let budget = self.active_budget()?;
        let n = ds.elements.len() as i64;
        let bytes = write_add(write_product(n, size::ELEMENT + 1)?, size::WRITE_CONTEXT)?;
        budget.borrow_mut().reserve(0, bytes, n + 1)?;
        Ok(WriteContext {
            elements: ds.elements.clone(),
            filtered: vec![false; ds.elements.len()],
            root,
            processed: 0,
            owner: self.id,
            epoch: self.epoch,
        })
    }
    pub fn apply_change(
        &mut self,
        c: Option<&mut WriteContext>,
        change: ElementChange<'_>,
    ) -> Result<(), WriteError> {
        let ElementChange {
            index,
            original,
            current,
            filtered,
            prior_proof,
            path,
        } = change;
        let budget = self.active_budget()?;
        if let Some(c) = &c
            && (c.owner != self.id || c.epoch != self.epoch)
        {
            return Err(WriteError::Scope);
        }
        {
            let mut b = budget.borrow_mut();
            let footprint = b.scan(&single(current.clone()))?;
            b.reserve(0, footprint.scratch, 0)?;
        }
        if prior_proof
            && (self.opts().resolve_vr.is_some() || !self.opts().data_set_rules.is_empty())
        {
            return Err(WriteError::Scope);
        }
        let Some(c) = c else {
            return Ok(());
        };
        if index < 0 || index as usize >= c.elements.len() {
            return Err(WriteError::Scope);
        }
        let index = index as usize;
        let charset_tag = Tag::new(0x0008, 0x0005);
        if (original.tag() == charset_tag || current.tag() == charset_tag)
            && (prior_proof || c.processed > 0)
        {
            return Err(WriteError::Scope);
        }
        c.elements[index] = current.clone();
        c.filtered[index] = filtered;
        let mut report = Report::default();
        if !filtered && original.tag() != current.tag() {
            let duplicate = c
                .elements
                .iter()
                .enumerate()
                .any(|(i, el)| i != index && !c.filtered[i] && el.tag() == current.tag());
            if duplicate {
                report.findings.push(Finding::new(
                    path.clone(),
                    current.tag(),
                    Some(current.vr()),
                    CODE_DUPLICATE_ELEMENT,
                    Severity::Error,
                    "duplicate element in the same data set",
                ));
            }
            let previous = (0..index).rev().find(|&i| !c.filtered[i]);
            let next = (index + 1..c.elements.len()).find(|&i| !c.filtered[i]);
            if previous.is_some_and(|p| current.tag() < c.elements[p].tag())
                || next.is_some_and(|n| c.elements[n].tag() < current.tag())
            {
                report.findings.push(Finding::new(
                    path.clone(),
                    current.tag(),
                    Some(current.vr()),
                    CODE_ELEMENT_ORDER,
                    Severity::Warning,
                    "element is encoded out of tag order",
                ));
            }
        }
        if c.root {
            let affected = self
                .opts()
                .required_uids
                .iter()
                .any(|&tag| original.tag() == tag || current.tag() == tag);
            if affected {
                let bytes = write_product(c.elements.len() as i64, size::ELEMENT)?;
                budget.borrow_mut().reserve(0, bytes + 256, 0)?;
                let elements: Vec<Element> = c
                    .elements
                    .iter()
                    .zip(&c.filtered)
                    .filter(|(_, f)| !**f)
                    .map(|(e, _)| e.clone())
                    .collect();
                let mut opts = self.opts().clone();
                opts.data_set_rules = Vec::new();
                let mut builder = ReportBuilder::new(&opts)?;
                builder.write_budget = Some(budget.clone());
                let mut session = Session::new(&opts, builder, false);
                session.validate_required_uids(&DataSet {
                    elements,
                    item_offset: None,
                });
                if let Some(e) = session.builder.limit_err.take() {
                    return Err(e.into());
                }
                let mut required = session.builder.report;
                let mut findings = std::mem::take(&mut report.findings);
                findings.append(&mut required.findings);
                required.findings = findings;
                report = required;
            }
        }
        self.add_report(&budget, report.clone())?;
        if self.opts().mode == Mode::Strict && report.has_errors() {
            return Err(Error::Failed(self.op.report()).into());
        }
        Ok(())
    }
}
pub(crate) fn validate_write_view(
    ds: &DataSet,
    opts: &ValidationOptions,
    budget: &SharedBudget,
    inherited: &Option<SpecificCharacterSet>,
) -> (ValidationResult, Option<Error>) {
    let fail = |e| (ValidationResult::default(), Some(e));
    if let Err(e) = budget
        .borrow_mut()
        .reserve(0, opts.data_set_rules.len() as i64 * 64 + 256, 0)
    {
        return fail(e);
    }
    let mut builder = match ReportBuilder::new(opts) {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    builder.write_budget = Some(budget.clone());
    if let Err(e) = budget.borrow_mut().admit_traversal(ds) {
        return fail(e);
    }
    let borrowed =
        opts.resolve_vr.is_none() && opts.data_set_rules.is_empty() && opts.hooks.is_none();
    let (max_depth, max_elements) = {
        let b = budget.borrow();
        (b.max_depth, b.max_elements)
    };
    let mut working = ds.clone();
    if !borrowed {
        if let Err(e) = budget.borrow_mut().admit_copy(ds, 1) {
            return fail(e);
        }
        if let Err(e) = check_clone_limits(ds, max_depth, max_elements) {
            return fail(e);
        }
    }
    let mut session = Session::new(opts, builder, borrowed);
    session.write_budget = Some(budget.clone());
    let mut err = None;
    match session.process_elements(&working, &[], 0, inherited) {
        Ok(elements) => {
            if !borrowed {
                working.elements = elements;
            }
        }
        Err(e) => {
            if !borrowed {
                working.elements = Vec::new();
            }
            err = Some(e);
        }
    }
    if err.is_none() && !session.builder.should_stop(opts) {
        session.validate_required_uids(&working);
    }
    if let Some(e) = session.builder.limit_err.clone() {
        err = Some(e);
    }
    let report = session.builder.report;
    if let Err(e) = budget.borrow_mut().admit_report(&report) {
        return fail(e);
    }
    let result = ValidationResult {
        data_set: working,
        report,
    };
    if err.is_some() {
        return (result, err);
    }
    if opts.mode == Mode::Strict && result.report.has_errors() {
        let report = result.report.clone();
        return (result, Some(Error::Failed(report)));
    }
    (result, None)
}
#[derive(Debug, Clone, Copy)]
pub struct ElementChange<'a> {
    pub index: i64,
    pub original: &'a Element,
    pub current: &'a Element,
    pub filtered: bool,
    pub prior_proof: bool,
    pub path: &'a Path,
}
