#![forbid(unsafe_code)]
use super::conn::Ctx;
use super::error::{Error, Kind};
use super::negotiation::{
    UserIdentityHandler, UserIdentityRequest, user_identity_request_from_user_info,
};
use super::pdu::*;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
#[derive(Debug, Clone, Default)]
pub struct AdmissionRequest {
    pub remote_addr: Option<SocketAddr>,
    pub local_addr: Option<SocketAddr>,
    pub calling_ae_title: String,
    pub called_ae_title: String,
    pub implementation_class_uid: String,
    pub implementation_version_name: String,
    pub user_identity: Option<UserIdentityRequest>,
    #[cfg(feature = "tls")]
    pub tls: Option<super::tls_conn::TlsConnectionState>,
}
pub type AdmissionHandler =
    Arc<dyn Fn(&Ctx, &AdmissionRequest) -> Option<AssociationRj> + Send + Sync>;
#[derive(Clone, Default)]
pub struct AdmissionDecision {
    pub reject: Option<AssociationRj>,
    pub user_identity: Option<UserIdentityHandler>,
}
pub type AdmissionDecider = Arc<dyn Fn(&Ctx, &AdmissionRequest) -> AdmissionDecision + Send + Sync>;
pub(crate) type AdmitFn<T> = Arc<dyn Fn(&Ctx, &AdmissionRequest) -> T + Send + Sync>;
pub(crate) const ADMISSION_REJECT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
#[derive(Debug, Clone, Default)]
pub struct RejectedAssociation {
    pub remote_addr: Option<SocketAddr>,
    pub calling_ae_title: String,
    pub called_ae_title: String,
    pub rejection: AssociationRj,
}
pub fn reject_calling_ae_title() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_PERMANENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
        reason: ASSOCIATE_RJ_REASON_CALLING_AE_TITLE_NOT_RECOGNIZED,
    }
}
pub fn reject_called_ae_title() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_PERMANENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
        reason: ASSOCIATE_RJ_REASON_CALLED_AE_TITLE_NOT_RECOGNIZED,
    }
}
pub fn reject_permanent() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_PERMANENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
        reason: ASSOCIATE_RJ_REASON_NO_REASON_GIVEN,
    }
}
pub fn reject_transient() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_TRANSIENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
        reason: ASSOCIATE_RJ_REASON_NO_REASON_GIVEN,
    }
}
pub fn reject_local_limit_exceeded() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_TRANSIENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_PRESENTATION,
        reason: ASSOCIATE_RJ_REASON_LOCAL_LIMIT_EXCEEDED,
    }
}
pub(crate) fn admission_request(
    remote: Option<SocketAddr>,
    local: Option<SocketAddr>,
    rq: &AssociationRq,
) -> AdmissionRequest {
    let mut req = AdmissionRequest {
        remote_addr: remote,
        local_addr: local,
        calling_ae_title: rq.calling_ae_title.clone(),
        called_ae_title: rq.called_ae_title.clone(),
        ..Default::default()
    };
    let items = rq.user_info.as_deref().unwrap_or(&[]);
    for item in items {
        match item {
            UserItem::ImplementationClassUid(uid) => req.implementation_class_uid = uid.clone(),
            UserItem::ImplementationVersionName(n) => req.implementation_version_name = n.clone(),
            _ => {}
        }
    }
    if let Ok(Some(identity)) = user_identity_request_from_user_info(items) {
        req.user_identity = Some(identity);
    }
    req
}
pub(crate) fn admit<T: Send + 'static>(
    ctx: &Ctx,
    handler: AdmitFn<T>,
    req: AdmissionRequest,
    reject_of: fn(&T) -> Option<AssociationRj>,
) -> (Option<AssociationRj>, Option<T>, Option<Error>) {
    let (tx, rx) = mpsc::channel();
    let thread_ctx = ctx.clone();
    std::thread::spawn(move || {
        let out = handler(&thread_ctx, &req);
        let _ = tx.send(out);
    });
    let received = match ctx.deadline() {
        Some(d) => loop {
            match rx.recv_timeout(
                Duration::from_millis(20).min(
                    d.saturating_duration_since(std::time::Instant::now())
                        .max(Duration::from_millis(1)),
                ),
            ) {
                Ok(v) => break Ok(v),
                Err(mpsc::RecvTimeoutError::Disconnected) => break Err(true),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if ctx.err().is_some() {
                        break Err(false);
                    }
                }
            }
        },
        None => loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(v) => break Ok(v),
                Err(mpsc::RecvTimeoutError::Disconnected) => break Err(true),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if ctx.err().is_some() {
                        break Err(false);
                    }
                }
            }
        },
    };
    match received {
        Err(true) => (
            Some(reject_permanent()),
            None,
            Some(Error::sentinel(Kind::AdmissionHandler)),
        ),
        Err(false) => {
            let ctx_err = super::conn::ended_context_error(ctx).map(|e| match ctx.err() {
                Some(Kind::DeadlineExceeded) => Error::sentinel(Kind::DeadlineExceeded),
                _ => e,
            });
            (
                Some(reject_transient()),
                None,
                Error::join([Some(Error::sentinel(Kind::AdmissionHandler)), ctx_err]),
            )
        }
        Ok(value) => {
            let Some(mut rj) = reject_of(&value) else {
                return (None, Some(value), None);
            };
            if rj.result != ASSOCIATE_RJ_RESULT_PERMANENT
                && rj.result != ASSOCIATE_RJ_RESULT_TRANSIENT
            {
                rj.result = ASSOCIATE_RJ_RESULT_PERMANENT;
            }
            if !valid_reject(rj.result, rj.source, rj.reason) {
                rj.source = ASSOCIATE_RJ_SOURCE_SERVICE_USER;
                rj.reason = ASSOCIATE_RJ_REASON_NO_REASON_GIVEN;
            }
            (Some(rj), Some(value), None)
        }
    }
}
