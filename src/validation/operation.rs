use super::engine::{
    Error, Mode, ReportBuilder, Session, ValidationOptions, ValidationResult, merge_reports,
    report_from_hook_result, validate_data_set_with, validate_options,
};
use super::hooks::{HookChain, HookEvent, HookPoint, HookResult, event_tag};
use super::{Finding, Path, Report, path_string};
use crate::charset::SpecificCharacterSet;
use crate::object::DataSet;
use std::collections::HashMap;
use std::sync::Arc;
pub struct Operation {
    pub(super) opts: ValidationOptions,
    pub(super) hooks: Option<Arc<HookChain>>,
    pub(super) report: Report,
    locations: HashMap<String, Vec<i64>>,
    pub(super) stopped: bool,
}
impl Operation {
    pub fn new(opts: ValidationOptions) -> Result<Operation, Error> {
        validate_options(&opts)?;
        let hooks = opts.hooks.clone();
        Ok(Operation {
            opts,
            hooks,
            report: Report::default(),
            locations: HashMap::new(),
            stopped: false,
        })
    }
    fn failed(&self) -> Error {
        Error::Failed(self.report.clone())
    }
    pub fn handle(&mut self, event: &HookEvent) -> (HookResult, Option<Error>) {
        self.handle_with_budget(event, None)
    }
    #[allow(clippy::type_complexity)]
    pub(super) fn handle_with_budget(
        &mut self,
        event: &HookEvent,
        budget: Option<&super::write::SharedBudget>,
    ) -> (HookResult, Option<Error>) {
        if self.stopped {
            if self.opts.mode == Mode::Strict && self.report.has_errors() {
                return (HookResult::default(), Some(self.failed()));
            }
            if let (Some(budget), Some(element)) = (budget, &event.element) {
                let admitted = budget.borrow_mut().admit_event(&HookEvent {
                    element: Some(element.clone()),
                    ..HookEvent::default()
                });
                if let Err(e) = admitted {
                    return (HookResult::default(), Some(e));
                }
            }
            return (
                HookResult {
                    element: event.element.clone(),
                    ..HookResult::default()
                },
                None,
            );
        }
        if event.offset_set && !event.path.is_empty() {
            self.record_location(&event.path, event.offset);
        }
        let mut admit_fn = |e: &HookEvent| -> Result<(), Error> {
            match budget {
                Some(b) => b.borrow_mut().admit_event(e),
                None => Ok(()),
            }
        };
        let admit: Option<&mut dyn FnMut(&HookEvent) -> Result<(), Error>> =
            budget.map(|_| &mut admit_fn as &mut dyn FnMut(&HookEvent) -> Result<(), Error>);
        let (result, err) = match &self.hooks {
            Some(h) => h.run_with_admission(event, self.opts.stop_first, admit),
            None => (
                HookResult {
                    element: event.element.clone(),
                    ..HookResult::default()
                },
                None,
            ),
        };
        let transformed_tag = event.offset_set
            && !event.path.is_empty()
            && result
                .element
                .as_ref()
                .is_some_and(|e| e.tag() != event_tag(event));
        if event.offset_set && !event.path.is_empty() && (result.filter || transformed_tag) {
            self.remove_location(&event.path, event.offset);
        }
        if transformed_tag {
            let mut path = event.path.clone();
            path.last_mut().unwrap().tag = result.element.as_ref().unwrap().tag();
            self.record_location(&path, event.offset);
        }
        if let Some(budget) = budget {
            let admitted = budget.borrow_mut().admit_report(&Report {
                findings: result.findings.clone(),
                changes: result.changes.clone(),
                ..Report::default()
            });
            if let Err(e) = admitted {
                return (result, Some(e));
            }
        }
        let report = match report_from_hook_result(&self.opts, &result) {
            Ok(r) => r,
            Err(e) => return (result, Some(e)),
        };
        self.merge(report);
        self.stop_if_requested();
        if self.stopped && self.opts.mode == Mode::Strict && self.report.has_errors() {
            return (result, Some(self.failed()));
        }
        (result, err)
    }
    pub fn has_hooks(&self, point: HookPoint) -> bool {
        self.hooks.as_ref().is_some_and(|h| h.has_point(point))
    }
    fn stopped_result(&self, ds: DataSet) -> (ValidationResult, Option<Error>) {
        let result = ValidationResult {
            data_set: ds,
            report: self.report.clone(),
        };
        if self.opts.mode == Mode::Strict && result.report.has_errors() {
            return (result, Some(self.failed()));
        }
        (result, None)
    }
    pub fn validate_parsed_data_set(&mut self, ds: DataSet) -> (ValidationResult, Option<Error>) {
        if self.stopped {
            return self.stopped_result(ds);
        }
        let pre = HookEvent {
            point: HookPoint::PreValidation,
            data_set: Some(ds.clone()),
            ..HookEvent::default()
        };
        if let (_, Some(e)) = self.handle(&pre) {
            return (
                ValidationResult {
                    data_set: ds,
                    report: self.report.clone(),
                },
                Some(e),
            );
        }
        if self.stopped {
            return self.stopped_result(ds);
        }
        let mut rule_opts = self.opts.clone();
        rule_opts.hooks = None;
        let (mut result, rule_err) = validate_data_set_with(&ds, &rule_opts);
        self.annotate_offsets(&mut result.report);
        self.merge(result.report.clone());
        self.stop_if_requested();
        if let Some(e) = rule_err
            && !e.is_validation_failed()
        {
            return (
                ValidationResult {
                    data_set: result.data_set,
                    report: self.report.clone(),
                },
                Some(e),
            );
        }
        if self.stopped {
            return self.stopped_result(result.data_set);
        }
        for point in [HookPoint::DataSetComplete, HookPoint::PostValidation] {
            let event = HookEvent {
                point,
                data_set: Some(result.data_set.clone()),
                ..HookEvent::default()
            };
            if let (_, Some(e)) = self.handle(&event) {
                return (
                    ValidationResult {
                        data_set: result.data_set,
                        report: self.report.clone(),
                    },
                    Some(e),
                );
            }
        }
        result.report = self.report.clone();
        if self.opts.mode == Mode::Strict && result.report.has_errors() {
            let report = result.report.clone();
            return (result, Some(Error::Failed(report)));
        }
        (result, None)
    }
    pub fn diagnose_parsed_prefix(&mut self, ds: &DataSet) {
        if self.stopped || ds.elements.is_empty() {
            return;
        }
        let opts = ValidationOptions {
            mode: self.opts.mode,
            stop_first: self.opts.stop_first,
            max_findings: self.opts.max_findings,
            max_depth: self.opts.max_depth,
            max_elements: self.opts.max_elements,
            big_endian: self.opts.big_endian,
            ..ValidationOptions::default()
        };
        let Ok(builder) = ReportBuilder::new(&opts) else {
            return;
        };
        let mut session = Session::new(&opts, builder, true);
        let _ = session.process_elements(
            ds,
            &[],
            0,
            &Some(SpecificCharacterSet::default_repertoire()),
        );
        let report = session.builder.report;
        self.add_report(report);
    }
    pub fn report(&self) -> Report {
        self.report.clone()
    }
    pub fn add_finding(&mut self, finding: Finding) {
        let report = Report {
            findings: vec![finding],
            ..Report::default()
        };
        if let Ok(r) = merge_reports(&self.opts, &[report]) {
            self.merge(r);
            self.stop_if_requested();
        }
    }
    pub fn add_report(&mut self, mut report: Report) {
        self.annotate_offsets(&mut report);
        self.merge(report);
        self.stop_if_requested();
    }
    fn stop_if_requested(&mut self) {
        if self.opts.stop_first && self.report.has_findings() {
            self.stopped = true;
        }
    }
    fn merge(&mut self, report: Report) {
        let current = std::mem::take(&mut self.report);
        match merge_reports(&self.opts, &[current.clone(), report]) {
            Ok(m) => self.report = m,
            Err(_) => self.report = current,
        }
    }
    fn annotate_offsets(&self, report: &mut Report) {
        for f in &mut report.findings {
            if f.offset_set {
                continue;
            }
            if let Some(offsets) = self.locations.get(&path_string(&f.path))
                && f.occurrence < offsets.len()
            {
                f.offset = offsets[f.occurrence];
                f.offset_set = true;
            }
        }
    }
    fn record_location(&mut self, path: &Path, offset: i64) {
        let offsets = self.locations.entry(path_string(path)).or_default();
        if offsets.last() != Some(&offset) {
            offsets.push(offset);
        }
    }
    fn remove_location(&mut self, path: &Path, offset: i64) {
        let key = path_string(path);
        if let Some(offsets) = self.locations.get_mut(&key)
            && let Some(i) = offsets.iter().rposition(|&o| o == offset)
        {
            offsets.remove(i);
            if offsets.is_empty() {
                self.locations.remove(&key);
            }
        }
    }
}
