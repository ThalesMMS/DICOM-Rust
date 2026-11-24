#![forbid(unsafe_code)]
use super::{
    ACTION_READ, CallbackError, ClinicalPolicy, ClinicalTransfer, Common, Ctx, Error, Message,
    OUTCOME_SUCCESS, Participant, PatientIdentity, QueryRequest, Result, SopClass, StudyIdentity,
    Time, build_instances_transferred, build_query,
};
use crate::net::audit::clinical::clinical_uid;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
pub type ClinicalEmit =
    Arc<dyn Fn(&Ctx, Message) -> std::result::Result<(), CallbackError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct AssociationAuditOptions {
    pub policy: ClinicalPolicy,
    pub common: Common,
    pub source: Participant,
    pub destination: Participant,
    pub requestor: Option<Participant>,
    pub emit: Option<ClinicalEmit>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstanceTransfer {
    pub time: Time,
    pub outcome: String,
    pub action: String,
    pub patient: PatientIdentity,
    pub study: StudyIdentity,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub encrypted: Option<bool>,
    pub anonymized: Option<bool>,
}
type AggregateKey = [String; 7];
struct AggregateGroup {
    input: InstanceTransfer,
    instances: HashMap<String, String>,
    classes: BTreeMap<String, i64>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationAuditStats {
    pub recorded: i64,
    pub duplicates: i64,
    pub rejected: i64,
    pub emitted: i64,
    pub finished: bool,
}
#[derive(Default)]
struct State {
    groups: BTreeMap<AggregateKey, AggregateGroup>,
    study_patients: HashMap<String, PatientIdentity>,
    identity_bytes: i64,
    instances: i64,
    closing: bool,
    pending: Option<Vec<Message>>,
    next: usize,
    stats: AssociationAuditStats,
}
pub struct AssociationAudit {
    opts: AssociationAuditOptions,
    emit: ClinicalEmit,
    state: Mutex<State>,
    gate: Mutex<bool>,
    gate_free: Condvar,
}
fn flag_key(v: Option<bool>) -> String {
    match v {
        None => String::new(),
        Some(true) => "true".to_string(),
        Some(false) => "false".to_string(),
    }
}
fn after(a: &Time, b: &Time) -> bool {
    a.nanos_since_zero() > b.nanos_since_zero()
}
struct GateGuard<'a>(&'a AssociationAudit);
impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        *self.0.gate.lock().unwrap_or_else(|e| e.into_inner()) = false;
        self.0.gate_free.notify_one();
    }
}
impl AssociationAudit {
    pub fn new(id: &str, opts: AssociationAuditOptions) -> Result<AssociationAudit> {
        let policy = opts.policy.normalize()?;
        if super::trim_unicode_space(id).is_empty() || id.len() > 256 {
            return Err(Error::clinical("required", "Association.ID"));
        }
        let Some(emit) = opts.emit.clone() else {
            return Err(Error::clinical("required", "Emit"));
        };
        Ok(AssociationAudit {
            opts: AssociationAuditOptions { policy, ..opts },
            emit,
            state: Mutex::new(State::default()),
            gate: Mutex::new(false),
            gate_free: Condvar::new(),
        })
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn enter(&self, ctx: &Ctx) -> Option<GateGuard<'_>> {
        self.enter_with(ctx, false)
    }
    fn enter_with(&self, ctx: &Ctx, prefer_gate: bool) -> Option<GateGuard<'_>> {
        let mut busy = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if prefer_gate && !*busy {
            *busy = true;
            return Some(GateGuard(self));
        }
        loop {
            if ctx.err().is_some() {
                return None;
            }
            if !*busy {
                *busy = true;
                return Some(GateGuard(self));
            }
            busy = self
                .gate_free
                .wait_timeout(busy, Duration::from_millis(5))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
    fn transfer(&self, time: Time, outcome: &str, patient: &PatientIdentity) -> ClinicalTransfer {
        let mut common = self.opts.common.clone();
        common.time = time;
        common.outcome = outcome.to_string();
        ClinicalTransfer {
            common,
            source: self.opts.source.clone(),
            destination: self.opts.destination.clone(),
            requestor: self.opts.requestor.clone(),
            patient: patient.clone(),
            studies: Vec::new(),
        }
    }
    pub fn record(&self, r: &InstanceTransfer) -> Result<()> {
        let mut r = r.clone();
        let mut state = self.lock();
        let a = &mut *state;
        let limits = &self.opts.policy.limits;
        macro_rules! reject {
            ($e:expr) => {{
                a.stats.rejected += 1;
                return Err($e);
            }};
        }
        if a.closing {
            reject!(Error::clinical("finished", "Association"));
        }
        if !self.opts.policy.include_identities {
            reject!(Error::clinical("opt_in", "Identity"));
        }
        if !clinical_uid(&r.sop_class_uid) || !clinical_uid(&r.sop_instance_uid) {
            reject!(Error::clinical("uid", "Instance"));
        }
        if r.action.is_empty() {
            r.action = ACTION_READ.to_string();
        }
        if r.outcome.is_empty() {
            r.outcome = OUTCOME_SUCCESS.to_string();
        }
        if r.time.is_zero() {
            r.time = Time::now_local();
        }
        r.time = Time::from_unix_utc(r.time.unix(), r.time.nanosecond());
        r.study.sop_classes = vec![SopClass {
            uid: r.sop_class_uid.clone(),
            number_of_instances: 1,
            instances: Vec::new(),
        }];
        r.study.encrypted = r.encrypted;
        r.study.anonymized = r.anonymized;
        let mut envelope = self.transfer(r.time, &r.outcome, &r.patient);
        envelope.studies = vec![r.study.clone()];
        if let Err(e) = build_instances_transferred(&self.opts.policy, &envelope, &r.action) {
            reject!(e);
        }
        if a.study_patients
            .get(&r.study.uid)
            .is_some_and(|previous| *previous != r.patient)
        {
            reject!(Error::clinical("identity_conflict", "Study.Patient"));
        }
        let key: AggregateKey = [
            r.patient.id.clone(),
            r.patient.issuer.clone(),
            r.study.uid.clone(),
            r.action.clone(),
            r.outcome.clone(),
            flag_key(r.encrypted),
            flag_key(r.anonymized),
        ];
        if let Some(g) = a.groups.get_mut(&key) {
            if g.input.study.name != r.study.name || g.input.study.query != r.study.query {
                reject!(Error::clinical("identity_conflict", "Study"));
            }
            if let Some(previous) = g.instances.get(&r.sop_instance_uid) {
                if *previous != r.sop_class_uid {
                    reject!(Error::clinical("identity_conflict", "Instance"));
                }
                a.stats.duplicates += 1;
                if after(&r.time, &g.input.time) {
                    g.input.time = r.time;
                }
                return Ok(());
            }
        }
        if a.instances >= limits.max_instances {
            reject!(Error::clinical("limit", "Instances"));
        }
        let mut added = (r.sop_instance_uid.len() + r.sop_class_uid.len()) as i64;
        match a.groups.get(&key) {
            None => {
                if a.groups.len() as i64 >= limits.max_groups {
                    reject!(Error::clinical("limit", "Groups"));
                }
                added += (r.patient.id.len()
                    + r.patient.name.len()
                    + r.patient.issuer.len()
                    + r.study.uid.len()
                    + r.study.name.len()
                    + r.study.query.len()) as i64;
                added += r
                    .study
                    .accessions
                    .iter()
                    .map(|v| v.len() as i64)
                    .sum::<i64>();
            }
            Some(g) => {
                if !g.classes.contains_key(&r.sop_class_uid)
                    && g.classes.len() as i64 >= limits.max_classes
                {
                    reject!(Error::clinical("limit", "Classes"));
                }
            }
        }
        if added > limits.max_identity_bytes - a.identity_bytes {
            reject!(Error::clinical("limit", "Identity"));
        }
        if !a.groups.contains_key(&key) {
            a.study_patients
                .insert(r.study.uid.clone(), r.patient.clone());
        }
        let g = a.groups.entry(key).or_insert_with(|| AggregateGroup {
            input: r.clone(),
            instances: HashMap::new(),
            classes: BTreeMap::new(),
        });
        g.instances
            .insert(r.sop_instance_uid.clone(), r.sop_class_uid.clone());
        *g.classes.entry(r.sop_class_uid.clone()).or_insert(0) += 1;
        if after(&r.time, &g.input.time) {
            g.input.time = r.time;
        }
        a.identity_bytes += added;
        a.instances += 1;
        a.stats.recorded += 1;
        Ok(())
    }
    pub fn stats(&self) -> AssociationAuditStats {
        self.lock().stats
    }
    fn invoke(&self, ctx: &Ctx, m: Message) -> Result<()> {
        let emit = self.emit.clone();
        let ctx = ctx.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || emit(&ctx, m)));
        match result {
            Ok(Ok(())) => Ok(()),
            _ => Err(Error::clinical("sink", "Emit")),
        }
    }
    pub fn finish(&self, ctx: &Ctx) -> Result<()> {
        let Some(_gate) = self.enter(ctx) else {
            return Err(Error::clinical("canceled", "Finish"));
        };
        {
            let mut state = self.lock();
            let a = &mut *state;
            if a.stats.finished {
                return Ok(());
            }
            a.closing = true;
            if a.pending.is_none() {
                let mut messages = Vec::with_capacity(a.groups.len());
                let mut keys: Vec<&AggregateKey> = a.groups.keys().collect();
                keys.sort_by_key(|k| k.join("\0"));
                for k in keys {
                    let g = &a.groups[k];
                    let mut study = g.input.study.clone();
                    study.sop_classes = g
                        .classes
                        .iter()
                        .map(|(uid, n)| SopClass {
                            uid: uid.clone(),
                            number_of_instances: *n,
                            instances: Vec::new(),
                        })
                        .collect();
                    let mut envelope =
                        self.transfer(g.input.time, &g.input.outcome, &g.input.patient);
                    envelope.studies = vec![study];
                    messages.push(build_instances_transferred(
                        &self.opts.policy,
                        &envelope,
                        &g.input.action,
                    )?);
                }
                a.pending = Some(messages);
            }
        }
        loop {
            let m = {
                let mut state = self.lock();
                let a = &mut *state;
                let pending = a.pending.as_ref().map_or(0, Vec::len);
                if a.next == pending {
                    a.stats.finished = true;
                    a.groups.clear();
                    a.study_patients.clear();
                    a.pending = None;
                    a.identity_bytes = 0;
                    a.instances = 0;
                    return Ok(());
                }
                a.pending.as_ref().map(|p| p[a.next].clone())
            };
            if ctx.err().is_some() {
                return Err(Error::clinical("canceled", "Finish"));
            }
            if let Some(m) = m {
                self.invoke(ctx, m)?;
            }
            let mut state = self.lock();
            state.next += 1;
            state.stats.emitted += 1;
        }
    }
    pub fn query(&self, ctx: &Ctx, q: &QueryRequest) -> Result<()> {
        let Some(_gate) = self.enter_with(ctx, true) else {
            return Err(Error::clinical("canceled", "Query"));
        };
        if self.lock().closing {
            return Err(Error::clinical("finished", "Association"));
        }
        let mut q = q.clone();
        q.source = self.opts.source.clone();
        q.destination = self.opts.destination.clone();
        q.common.source = self.opts.common.source.clone();
        let m = build_query(&self.opts.policy, &q)?;
        self.invoke(ctx, m)
    }
}
