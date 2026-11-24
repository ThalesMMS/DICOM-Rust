use super::engine::Error;
use super::{
    CODE_HOOK_ACTION, CODE_HOOK_DIAGNOSTIC, CODE_HOOK_ERROR, CODE_HOOK_PANIC, Change, ChangeKind,
    Code, Finding, Path, Severity,
};
use crate::object::{DataSet, Element, ElementHeader, Tag, Vr};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HookPoint {
    ElementHeaderRead,
    #[default]
    AfterElement,
    ItemComplete,
    SequenceComplete,
    DataSetComplete,
    PreValidation,
    PostValidation,
    PreSerialization,
    PostWrite,
    Unknown,
}
impl HookPoint {
    pub const ALL: [HookPoint; 9] = [
        HookPoint::ElementHeaderRead,
        HookPoint::AfterElement,
        HookPoint::ItemComplete,
        HookPoint::SequenceComplete,
        HookPoint::DataSetComplete,
        HookPoint::PreValidation,
        HookPoint::PostValidation,
        HookPoint::PreSerialization,
        HookPoint::PostWrite,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            HookPoint::ElementHeaderRead => "element_header_read",
            HookPoint::AfterElement => "element_decoded",
            HookPoint::ItemComplete => "item_complete",
            HookPoint::SequenceComplete => "sequence_complete",
            HookPoint::DataSetComplete => "dataset_complete",
            HookPoint::PreValidation => "pre_validation",
            HookPoint::PostValidation => "post_validation",
            HookPoint::PreSerialization => "pre_serialization",
            HookPoint::PostWrite => "post_write",
            HookPoint::Unknown => "",
        }
    }
    pub fn parse(s: &str) -> Option<HookPoint> {
        HookPoint::ALL.into_iter().find(|p| p.as_str() == s)
    }
}
#[derive(Clone, Debug, Default)]
pub struct HookEvent {
    pub point: HookPoint,
    pub path: Path,
    pub header: Option<ElementHeader>,
    pub element: Option<Element>,
    pub data_set: Option<DataSet>,
    pub offset: i64,
    pub offset_set: bool,
    pub bytes_written: i64,
    pub write_complete: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleFinding {
    pub path: Path,
    pub tag: Tag,
    pub vr: Option<Vr>,
    pub severity: Option<Severity>,
    pub code: Option<Code>,
}
#[derive(Clone, Debug, Default)]
pub struct HookDecision {
    pub element: Option<Element>,
    pub filter: bool,
    pub skip_value: bool,
    pub defer_value: bool,
    pub findings: Vec<RuleFinding>,
}
pub type Hook = Arc<dyn Fn(&HookEvent) -> Result<HookDecision, crate::BoxError> + Send + Sync>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookFailurePolicy {
    #[default]
    Reject,
    Finding,
}
#[derive(Clone, Default)]
pub struct HookRegistration {
    pub name: String,
    pub points: Vec<HookPoint>,
    pub hook: Option<Hook>,
    pub concurrent_safe: bool,
    pub timeout: Duration,
    pub on_failure: HookFailurePolicy,
}
struct Registered {
    registration: HookRegistration,
    gate: Option<Mutex<()>>,
}
#[derive(Default)]
pub struct HookChain {
    hooks: RwLock<Vec<Arc<Registered>>>,
}
#[derive(Clone, Debug, Default)]
pub struct HookResult {
    pub element: Option<Element>,
    pub filter: bool,
    pub skip_value: bool,
    pub defer_value: bool,
    pub findings: Vec<Finding>,
    pub changes: Vec<Change>,
}
pub(crate) fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
pub(crate) trait ChangeKindCode {
    fn code_message(self) -> (Code, &'static str);
}
impl HookChain {
    pub fn new(registrations: Vec<HookRegistration>) -> Result<HookChain, Error> {
        let chain = HookChain::default();
        for r in registrations {
            chain.add(r)?;
        }
        Ok(chain)
    }
    pub fn add(&self, mut registration: HookRegistration) -> Result<(), Error> {
        registration.name = registration.name.trim().to_string();
        if !valid_name(&registration.name)
            || registration.hook.is_none()
            || registration.points.is_empty()
        {
            return Err(Error::InvalidPolicy("invalid hook registration".into()));
        }
        if registration.points.contains(&HookPoint::Unknown) {
            return Err(Error::InvalidPolicy("unknown hook point \"\"".into()));
        }
        let gate = (!registration.concurrent_safe).then(|| Mutex::new(()));
        let mut hooks = self.hooks.write().unwrap();
        if hooks
            .iter()
            .any(|h| h.registration.name == registration.name)
        {
            return Err(Error::InvalidPolicy("duplicate hook name".into()));
        }
        hooks.push(Arc::new(Registered { registration, gate }));
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.hooks.read().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn has_point(&self, point: HookPoint) -> bool {
        self.hooks
            .read()
            .unwrap()
            .iter()
            .any(|h| h.registration.points.contains(&point))
    }
    pub fn run(&self, event: &HookEvent) -> (HookResult, Option<Error>) {
        self.run_inner(event, false)
    }
    pub(crate) fn run_inner(
        &self,
        event: &HookEvent,
        stop_after_finding: bool,
    ) -> (HookResult, Option<Error>) {
        self.run_with_admission(event, stop_after_finding, None)
    }
    pub(crate) fn snapshot(&self) -> HookChain {
        HookChain {
            hooks: RwLock::new(self.hooks.read().unwrap().clone()),
        }
    }
    #[allow(clippy::type_complexity)]
    pub(crate) fn run_with_admission(
        &self,
        event: &HookEvent,
        stop_after_finding: bool,
        mut admit: Option<&mut dyn FnMut(&HookEvent) -> Result<(), Error>>,
    ) -> (HookResult, Option<Error>) {
        let hooks: Vec<Arc<Registered>> = self.hooks.read().unwrap().clone();
        let path_event = || HookEvent {
            path: event.path.clone(),
            ..HookEvent::default()
        };
        let element_event = |element: &Element| HookEvent {
            element: Some(element.clone()),
            ..HookEvent::default()
        };
        if let Some(admit) = admit.as_mut() {
            if let Err(e) = admit(event) {
                return (HookResult::default(), Some(e));
            }
            if let Some(element) = &event.element
                && let Err(e) = admit(&element_event(element))
            {
                return (HookResult::default(), Some(e));
            }
        }
        let mut working = event.clone();
        let mut result = HookResult {
            element: working.element.clone(),
            ..HookResult::default()
        };
        for registered in &hooks {
            if !registered.registration.points.contains(&event.point) {
                continue;
            }
            if let Some(admit) = admit.as_mut()
                && let Err(e) = admit(&working)
            {
                return (result, Some(e));
            }
            let outcome = {
                let _guard = registered.gate.as_ref().map(|g| g.lock());
                invoke(registered, &working)
            };
            let decision = match outcome {
                Err(code) => {
                    if let Some(e) = hook_failure(&mut result, registered, event, code) {
                        return (result, Some(e));
                    }
                    if stop_after_finding && !result.findings.is_empty() {
                        break;
                    }
                    continue;
                }
                Ok(d) => d,
            };
            if !valid_decision(event.point, &decision)
                || (decision.skip_value && result.defer_value)
                || (decision.defer_value && result.skip_value)
            {
                if let Some(e) = hook_failure(&mut result, registered, event, CODE_HOOK_ACTION) {
                    return (result, Some(e));
                }
                if stop_after_finding && !result.findings.is_empty() {
                    break;
                }
                continue;
            }
            let name = &registered.registration.name;
            for rf in &decision.findings {
                if let Some(admit) = admit.as_mut()
                    && let Err(e) = admit(&path_event())
                {
                    return (result, Some(e));
                }
                let (tag, vr) = if let Some(e) = &event.element {
                    (e.tag(), Some(e.vr()))
                } else if let Some(h) = &event.header {
                    (h.tag, Some(h.vr))
                } else {
                    (rf.tag, rf.vr)
                };
                let mut f = Finding::new(
                    event.path.clone(),
                    tag,
                    vr,
                    CODE_HOOK_DIAGNOSTIC,
                    rf.severity.unwrap_or(Severity::Warning),
                    "hook reported a validation finding",
                );
                f.offset = event.offset;
                f.offset_set = event.offset_set;
                f.hook = name.clone();
                result.findings.push(f);
            }
            if let Some(replacement) = &decision.element {
                if let Some(admit) = admit.as_mut()
                    && let Err(e) = admit(&element_event(replacement))
                {
                    return (result, Some(e));
                }
                working.element = Some(replacement.clone());
                result.element = Some(replacement.clone());
                if let Some(admit) = admit.as_mut()
                    && let Err(e) = admit(&path_event())
                {
                    return (result, Some(e));
                }
                result.changes.push(Change {
                    path: event.path.clone(),
                    tag: replacement.tag(),
                    hook: name.clone(),
                    kind: ChangeKind::Transformed,
                });
            }
            let tag = event_tag(event);
            for (flag, kind) in [
                (decision.filter, ChangeKind::Filtered),
                (decision.skip_value, ChangeKind::Skipped),
                (decision.defer_value, ChangeKind::Deferred),
            ] {
                if flag {
                    if let Some(admit) = admit.as_mut()
                        && let Err(e) = admit(&path_event())
                    {
                        return (result, Some(e));
                    }
                    match kind {
                        ChangeKind::Filtered => result.filter = true,
                        ChangeKind::Skipped => result.skip_value = true,
                        _ => result.defer_value = true,
                    }
                    result.changes.push(Change {
                        path: event.path.clone(),
                        tag,
                        hook: name.clone(),
                        kind,
                    });
                }
            }
            if stop_after_finding && !decision.findings.is_empty() {
                break;
            }
        }
        (result, None)
    }
}
fn invoke(registered: &Registered, event: &HookEvent) -> Result<HookDecision, Code> {
    let hook = registered
        .registration
        .hook
        .as_ref()
        .expect("validated hook");
    let event = event.clone();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(&event))) {
        Err(_) => Err(CODE_HOOK_PANIC),
        Ok(Err(_)) => Err(CODE_HOOK_ERROR),
        Ok(Ok(d)) => Ok(d),
    }
}
fn hook_failure(
    result: &mut HookResult,
    registered: &Registered,
    event: &HookEvent,
    code: Code,
) -> Option<Error> {
    let name = &registered.registration.name;
    if registered.registration.on_failure == HookFailurePolicy::Reject {
        return Some(Error::Hook {
            hook: name.clone(),
            code,
            action: code == CODE_HOOK_ACTION,
        });
    }
    let vr = if let Some(e) = &event.element {
        Some(e.vr())
    } else {
        event.header.map(|h| h.vr)
    };
    let mut f = Finding::new(
        event.path.clone(),
        event_tag(event),
        vr,
        code,
        Severity::Warning,
        "hook failed without applying its decision",
    );
    f.offset = event.offset;
    f.offset_set = event.offset_set;
    f.hook = name.clone();
    result.findings.push(f);
    None
}
fn valid_decision(point: HookPoint, d: &HookDecision) -> bool {
    if d.skip_value && d.defer_value {
        return false;
    }
    if d.element.is_some() && d.filter {
        return false;
    }
    match point {
        HookPoint::ElementHeaderRead => d.element.is_none() && !d.filter,
        HookPoint::AfterElement | HookPoint::SequenceComplete | HookPoint::PreSerialization => {
            !d.skip_value && !d.defer_value
        }
        _ => d.element.is_none() && !d.filter && !d.skip_value && !d.defer_value,
    }
}
pub(crate) fn event_tag(event: &HookEvent) -> Tag {
    if let Some(e) = &event.element {
        return e.tag();
    }
    if let Some(h) = &event.header {
        return h.tag;
    }
    event.path.last().map(|s| s.tag).unwrap_or_default()
}
