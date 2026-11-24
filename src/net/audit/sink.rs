#![forbid(unsafe_code)]
use super::{
    ACTION_CREATE, Common, Ctx, Error, Message, OUTCOME_MINOR_FAILURE, OUTCOME_SUCCESS,
    Participant, QueryRequest, Source, SyslogLogger, Time, Transfer, begin_transferring,
    instances_transferred, query, security_alert, split_host_port, type_node_authentication,
};
use std::sync::Arc;
pub type EventKind = &'static str;
pub const ASSOCIATION_ACCEPTED: EventKind = "association.accepted";
pub const ASSOCIATION_REJECTED: EventKind = "association.rejected";
pub const ASSOCIATION_ABORTED: EventKind = "association.aborted";
pub const COMMAND_RECEIVED: EventKind = "command.received";
pub const RESPONSE_SENT: EventKind = "response.sent";
pub const OPERATION_SUCCEEDED: EventKind = "operation.succeeded";
pub const OPERATION_FAILED: EventKind = "operation.failed";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub time: Time,
    pub kind: String,
    pub service: String,
    pub local_ae_title: String,
    pub remote_ae_title: String,
    pub remote_addr: String,
    pub sop_class_uid: String,
    pub command: String,
    pub status: u16,
    pub status_set: bool,
    pub error_class: String,
}
pub trait Sink: Send + Sync {
    fn emit_audit_event(&self, ctx: &Ctx, event: Event);
}
impl<F: Fn(&Ctx, Event) + Send + Sync> Sink for F {
    fn emit_audit_event(&self, ctx: &Ctx, event: Event) {
        self(ctx, event)
    }
}
pub fn emit(ctx: &Ctx, sink: Option<&dyn Sink>, mut event: Event) {
    let Some(sink) = sink else { return };
    if event.time.is_zero() {
        event.time = Time::now_utc();
    }
    sink.emit_audit_event(ctx, event);
}
#[derive(Clone, Default)]
pub struct AtnaSink {
    pub logger: Option<Arc<SyslogLogger>>,
    pub source: Source,
    pub on_error: Option<Arc<dyn Fn(Error) + Send + Sync>>,
}
impl Sink for AtnaSink {
    fn emit_audit_event(&self, _ctx: &Ctx, event: Event) {
        let (Some(message), Some(logger)) = (self.message(&event), &self.logger) else {
            return;
        };
        if let Err(e) = logger.log(&message)
            && let Some(on_error) = &self.on_error
        {
            on_error(e);
        }
    }
}
impl AtnaSink {
    pub fn message(&self, event: &Event) -> Option<Message> {
        let mut common = Common {
            time: event.time,
            source: self.source.clone(),
            outcome: OUTCOME_SUCCESS.to_string(),
            outcome_description: String::new(),
        };
        let kind = event.kind.as_str();
        if kind == OPERATION_FAILED || kind == ASSOCIATION_REJECTED || kind == ASSOCIATION_ABORTED {
            common.outcome = OUTCOME_MINOR_FAILURE.to_string();
            common.outcome_description = event.error_class.clone();
        }
        let mut local = Participant {
            user_id: event.local_ae_title.clone(),
            alternative_user_id: event.local_ae_title.clone(),
            ..Default::default()
        };
        let mut remote = Participant {
            user_id: event.remote_ae_title.clone(),
            alternative_user_id: event.remote_ae_title.clone(),
            host: remote_host(&event.remote_addr).to_string(),
            ..Default::default()
        };
        if local.user_id.is_empty() {
            local.user_id = self.source.id.clone();
        }
        if remote.user_id.is_empty() {
            remote.user_id = remote.host.clone();
        }
        if kind == ASSOCIATION_REJECTED || kind == ASSOCIATION_ABORTED {
            let subject = if remote.host.is_empty() {
                &remote.user_id
            } else {
                &remote.host
            };
            return (!local.user_id.is_empty())
                .then(|| security_alert(&common, type_node_authentication(), &local, subject));
        }
        if kind != OPERATION_SUCCEEDED && kind != OPERATION_FAILED {
            return None;
        }
        if local.user_id.is_empty() || remote.user_id.is_empty() {
            return None;
        }
        remote.requestor = true;
        let service = super::trim_unicode_space(&event.service).to_uppercase();
        match service.as_str() {
            "C-STORE" => Some(instances_transferred(
                &Transfer {
                    common,
                    source: remote,
                    destination: local,
                    ..Default::default()
                },
                ACTION_CREATE,
            )),
            "C-FIND" => (!event.sop_class_uid.is_empty()).then(|| {
                query(&QueryRequest {
                    common,
                    source: remote,
                    destination: local,
                    sop_class_uid: event.sop_class_uid.clone(),
                    ..Default::default()
                })
            }),
            "C-MOVE" | "C-GET" => Some(begin_transferring(&Transfer {
                common,
                source: local,
                destination: remote,
                ..Default::default()
            })),
            _ => None,
        }
    }
}
fn remote_host(address: &str) -> &str {
    match split_host_port(address) {
        Some((host, _)) => host,
        None => address,
    }
}
