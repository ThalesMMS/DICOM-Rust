#![forbid(unsafe_code)]
use super::command::*;
use super::controls::{OperationOptions, context_cause};
use super::error::{Detail, Error, Kind, Result};
use super::find::*;
use super::get::*;
use super::message::{
    accepted_context_for_sop_class, normalized_transfer_syntax_uids,
    transfer_syntax_for_accepted_context,
};
use super::retrieve::*;
use super::retrieve_extras::{
    CMoveBatch, CMoveBatchHandler, CMoveBatchYield, CMoveSubOperationDescriptor, read_single_value,
    read_uid_list,
};
use super::storage_catalog::default_c_get_storage_sop_class_uids;
use super::store::{CStoreOptions, STATUS_C_STORE_OUT_OF_RESOURCES, StoreClient};
use crate::net::ul::{self, Association, Ctx, DialOptions, Nanos, PresentationContext};
use crate::object::{
    self, Element, ElementHeader, File, Object, Syntax, Tag, Value, Vr, is_valid_uid,
};
use crate::transfer::{NativeStorePreference, proposed_store_transfer_syntax_uids};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub const STATUS_RETRIEVE_OUT_OF_RESOURCES_MATCHES: u16 = 0xA701;
pub const STATUS_RETRIEVE_OUT_OF_RESOURCES_SUB_OPERATIONS: u16 = 0xA702;
const MAX_SUB_OPERATIONS: i64 = (1 << 16) - 1;
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
const TAG_QUERY_RETRIEVE_LEVEL: Tag = Tag::new(0x0008, 0x0052);
const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub fn store_transfer_syntax_uids(source_uid: &str, allowed: &[String]) -> Vec<String> {
    let candidates =
        proposed_store_transfer_syntax_uids(source_uid, NativeStorePreference::SourceFirst);
    if allowed.is_empty() {
        return candidates;
    }
    let permitted: HashSet<&str> = allowed
        .iter()
        .map(|u| object::transfer::normalize_uid(u))
        .collect();
    candidates
        .into_iter()
        .filter(|u| permitted.contains(u.as_str()))
        .collect()
}
fn accepted_transfer_syntax_uids(assoc: &Association, sop_class_uid: &str) -> Vec<String> {
    let syntaxes: Vec<String> = assoc
        .accepted_contexts
        .iter()
        .filter(|pc| pc.abstract_syntax_uid == sop_class_uid)
        .map(|pc| pc.transfer_syntax_uid.clone())
        .collect();
    normalized_transfer_syntax_uids(&syntaxes)
}
fn after_ctx(ctx: &Ctx, f: impl FnOnce() + Send + 'static) {
    if !ctx.can_end() {
        return;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        while ctx.err().is_none() {
            std::thread::sleep(Duration::from_millis(2));
        }
        f();
    });
}
#[derive(Debug, Clone, Default)]
pub struct RetrieveSourceRequest {
    pub sop_class_uid: String,
    pub query_retrieve_level: String,
    pub identifier: Option<Object>,
    pub priority: u16,
    pub calling_ae_title: String,
    pub move_destination: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetrieveInstance {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub transfer_syntax_uid: String,
}
pub trait RetrieveSet: Send + Sync {
    fn instances(&self) -> Vec<RetrieveInstance>;
    fn fetch(
        &self,
        ctx: &Ctx,
        instance: &RetrieveInstance,
        transfer_syntax_uids: &[String],
    ) -> Result<Option<Arc<File>>>;
    fn close(&self) -> Result<()>;
}
pub trait RetrieveSource: Send + Sync {
    fn locate(
        &self,
        ctx: &Ctx,
        request: &RetrieveSourceRequest,
    ) -> Result<Option<Arc<dyn RetrieveSet>>>;
}
pub(crate) type RetrieveLocateFunc =
    Arc<dyn Fn(&Ctx, &RetrieveSourceRequest) -> Result<Vec<RetrieveInstance>> + Send + Sync>;
pub(crate) type RetrieveFetchFunc =
    Arc<dyn Fn(&Ctx, &RetrieveInstance, &[String]) -> Result<Option<Arc<File>>> + Send + Sync>;
#[derive(Clone, Default)]
pub struct RetrieveSourceFuncs {
    pub locate_func: Option<RetrieveLocateFunc>,
    pub fetch_func: Option<RetrieveFetchFunc>,
}
struct FuncRetrieveSet {
    instances: Vec<RetrieveInstance>,
    fetch: RetrieveFetchFunc,
}
impl RetrieveSet for FuncRetrieveSet {
    fn instances(&self) -> Vec<RetrieveInstance> {
        self.instances.clone()
    }
    fn fetch(
        &self,
        ctx: &Ctx,
        instance: &RetrieveInstance,
        syntaxes: &[String],
    ) -> Result<Option<Arc<File>>> {
        (self.fetch)(ctx, instance, syntaxes)
    }
    fn close(&self) -> Result<()> {
        Ok(())
    }
}
impl RetrieveSource for RetrieveSourceFuncs {
    fn locate(
        &self,
        ctx: &Ctx,
        request: &RetrieveSourceRequest,
    ) -> Result<Option<Arc<dyn RetrieveSet>>> {
        let (Some(locate), Some(fetch)) = (&self.locate_func, &self.fetch_func) else {
            return Err(Error::new(
                "dicom dimse: retrieve source functions are not set",
            ));
        };
        let instances = locate(ctx, request)?;
        Ok(Some(Arc::new(FuncRetrieveSet {
            instances,
            fetch: fetch.clone(),
        })))
    }
}
#[derive(Clone, Default)]
pub struct MoveDestinationAddress {
    pub address: String,
    #[cfg(feature = "tls")]
    pub tls_config: Option<ul::TlsClientConfig>,
}
impl std::fmt::Debug for MoveDestinationAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveDestinationAddress")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Default)]
pub struct MoveDestinationRequest {
    pub calling_ae_title: String,
    pub called_ae_title: String,
    pub move_destination: String,
}
pub trait MoveDestinationResolver: Send + Sync {
    fn resolve_move_destination(
        &self,
        ctx: &Ctx,
        request: &MoveDestinationRequest,
    ) -> Result<MoveDestinationAddress>;
}
#[derive(Debug, Clone, Default)]
pub struct MoveDestinationPolicy {
    pub destinations: HashMap<String, MoveDestinationAddress>,
    pub callers: HashMap<String, Vec<String>>,
    pub caller_only: bool,
}
impl MoveDestinationResolver for MoveDestinationPolicy {
    fn resolve_move_destination(
        &self,
        _ctx: &Ctx,
        request: &MoveDestinationRequest,
    ) -> Result<MoveDestinationAddress> {
        let name = request.move_destination.trim();
        let caller = request.calling_ae_title.trim();
        let destination = match self.destinations.get(name) {
            Some(d) if !d.address.trim().is_empty() => d,
            _ => return Err(Error::sentinel(Kind::MoveDestinationUnknown)),
        };
        if self.caller_only && name != caller {
            return Err(Error::sentinel(Kind::MoveDestinationRefused));
        }
        if let Some(allowed) = self.callers.get(name) {
            if allowed.iter().any(|c| c.trim() == caller) {
                return Ok(destination.clone());
            }
            return Err(Error::sentinel(Kind::MoveDestinationRefused));
        }
        Ok(destination.clone())
    }
}
#[derive(Clone, Default)]
pub struct RetrieveProxyOptions {
    pub source: Option<Arc<dyn RetrieveSource>>,
    pub destinations: Option<Arc<dyn MoveDestinationResolver>>,
    pub ae_title: String,
    pub destination_dial_options: DialOptions,
    pub destination_transfer_syntax_uids: Vec<String>,
    pub max_instances: i64,
}
pub struct RetrieveProxy {
    options: RetrieveProxyOptions,
    source: Arc<dyn RetrieveSource>,
}
pub fn new_retrieve_proxy(mut options: RetrieveProxyOptions) -> Result<Arc<RetrieveProxy>> {
    let Some(source) = options.source.clone() else {
        return Err(Error::new("dicom dimse: retrieve proxy source is required"));
    };
    if options.max_instances <= 0 || options.max_instances > MAX_SUB_OPERATIONS {
        options.max_instances = MAX_SUB_OPERATIONS;
    }
    if options.destination_transfer_syntax_uids.is_empty() {
        options.destination_transfer_syntax_uids = vec![EXPLICIT_LE.into(), IMPLICIT_LE.into()];
    }
    options.destination_transfer_syntax_uids =
        normalized_transfer_syntax_uids(&options.destination_transfer_syntax_uids);
    if options.ae_title.trim().is_empty() {
        options.ae_title = "DCMRUST_PROXY".into();
    }
    Ok(Arc::new(RetrieveProxy { options, source }))
}
impl RetrieveProxy {
    pub fn handler(self: &Arc<Self>, assoc: Arc<Association>) -> Arc<RetrieveProxyHandler> {
        Arc::new(RetrieveProxyHandler {
            proxy: self.clone(),
            assoc,
        })
    }
}
pub struct RetrieveProxyHandler {
    proxy: Arc<RetrieveProxy>,
    assoc: Arc<Association>,
}
enum ProxyError {
    Status(u16, &'static str, Error),
    Other(Error),
}
impl ProxyError {
    fn matches(err: Error) -> ProxyError {
        ProxyError::Status(
            STATUS_RETRIEVE_OUT_OF_RESOURCES_MATCHES,
            "Unable to calculate number of matches",
            err,
        )
    }
    fn into_move(self) -> Error {
        match self {
            ProxyError::Status(status, comment, err) => {
                new_c_move_scp_error(status, comment, Some(err))
            }
            ProxyError::Other(e) => e,
        }
    }
    fn into_get(self) -> Error {
        match self {
            ProxyError::Status(status, comment, err) => {
                new_c_get_scp_error(status, comment, Some(err))
            }
            ProxyError::Other(e) => e,
        }
    }
}
fn join(first: Error, second: Error) -> Error {
    Error::join([Some(first), Some(second)]).unwrap()
}
struct ProxyRetrieveSet {
    set: Arc<dyn RetrieveSet>,
    instances: Vec<RetrieveInstance>,
    closed: Mutex<bool>,
}
impl ProxyRetrieveSet {
    fn close(&self) {
        let mut closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        if !*closed {
            *closed = true;
            let _ = self.set.close();
        }
    }
    fn fetch(
        &self,
        ctx: &Ctx,
        instance: &RetrieveInstance,
        syntaxes: &[String],
    ) -> Result<Arc<File>> {
        let source_instance = || Error::sentinel(Kind::RetrieveSourceInstance);
        let file = match self.set.fetch(ctx, instance, syntaxes) {
            Ok(f) => f,
            Err(err) => {
                if let Some(cause) = context_cause(ctx) {
                    return Err(join(cause, source_instance()));
                }
                let mut hidden = err.relabel(Kind::RetrieveSourceInstance.text());
                hidden = hidden.also(Kind::RetrieveSourceInstance);
                return Err(hidden);
            }
        };
        let Some(file) = file else {
            return Err(source_instance());
        };
        let class = file.dataset.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
        let found = file
            .dataset
            .get_uid(TAG_SOP_INSTANCE_UID)
            .unwrap_or_default();
        if (!instance.sop_class_uid.is_empty() && class != instance.sop_class_uid)
            || !is_valid_uid(class.as_bytes())
            || found != instance.sop_instance_uid
        {
            return Err(Error::wrap(
                Kind::RetrieveSourceInstance,
                "fetched data set is a different instance",
            ));
        }
        Ok(file)
    }
}
impl RetrieveProxyHandler {
    fn calling_ae_title(&self) -> String {
        self.assoc.calling_ae_title.trim().to_string()
    }
    fn called_ae_title(&self) -> String {
        self.assoc.called_ae_title.trim().to_string()
    }
    fn locate(
        &self,
        ctx: &Ctx,
        request: &RetrieveSourceRequest,
    ) -> std::result::Result<Arc<ProxyRetrieveSet>, ProxyError> {
        let set = match self.proxy.source.locate(ctx, request) {
            Ok(s) => s,
            Err(err) => {
                if let Some(cause) = context_cause(ctx) {
                    return Err(ProxyError::Other(join(cause, err)));
                }
                return Err(ProxyError::matches(err));
            }
        };
        let Some(set) = set else {
            return Err(ProxyError::matches(Error::new(
                "dicom dimse: retrieve source returned no set",
            )));
        };
        let instances = set.instances();
        let wrapped = Arc::new(ProxyRetrieveSet {
            set,
            instances: instances.clone(),
            closed: Mutex::new(false),
        });
        let watched = wrapped.clone();
        after_ctx(ctx, move || watched.close());
        let max = self.proxy.options.max_instances;
        if instances.len() as i64 > max {
            wrapped.close();
            return Err(ProxyError::Status(
                STATUS_RETRIEVE_OUT_OF_RESOURCES_SUB_OPERATIONS,
                "Too many sub-operations",
                Error::new(format!(
                    "dicom dimse: retrieve source located {} instances, limit {max}",
                    instances.len()
                )),
            ));
        }
        for (i, instance) in instances.iter().enumerate() {
            if (!instance.sop_class_uid.is_empty()
                && !is_valid_uid(instance.sop_class_uid.as_bytes()))
                || !is_valid_uid(instance.sop_instance_uid.as_bytes())
            {
                wrapped.close();
                return Err(ProxyError::matches(Error::new(format!(
                    "dicom dimse: retrieve source instance {i} has an invalid SOP Class or Instance UID"
                ))));
            }
        }
        Ok(wrapped)
    }
    fn resolve_destination(
        &self,
        ctx: &Ctx,
        request: &CMoveRequestContext,
    ) -> Result<MoveDestinationAddress> {
        let unknown = |err: Error| {
            new_c_move_scp_error(
                STATUS_C_MOVE_MOVE_DESTINATION_UNKNOWN,
                "Move Destination unknown",
                Some(err),
            )
        };
        let Some(resolver) = &self.proxy.options.destinations else {
            return Err(unknown(Error::sentinel(Kind::MoveDestinationUnknown)));
        };
        let resolved = resolver.resolve_move_destination(
            ctx,
            &MoveDestinationRequest {
                calling_ae_title: self.calling_ae_title(),
                called_ae_title: self.called_ae_title(),
                move_destination: request.request.move_destination.trim().to_string(),
            },
        );
        let resolved = match resolved {
            Ok(d) if d.address.trim().is_empty() => {
                Err(Error::sentinel(Kind::MoveDestinationUnknown))
            }
            other => other,
        };
        match resolved {
            Ok(d) => Ok(d),
            Err(err) => match context_cause(ctx) {
                Some(cause) => Err(join(cause, err)),
                None => Err(unknown(err)),
            },
        }
    }
    fn prepare_move(&self, ctx: &Ctx, request: &CMoveRequestContext) -> Result<Arc<MoveForwarder>> {
        let destination = self.resolve_destination(ctx, request)?;
        let set = self
            .locate(
                ctx,
                &RetrieveSourceRequest {
                    sop_class_uid: request.request.affected_sop_class_uid.clone(),
                    query_retrieve_level: request.query_retrieve_level.clone(),
                    identifier: Some(request.identifier.clone()),
                    priority: request.request.priority,
                    calling_ae_title: self.calling_ae_title(),
                    move_destination: request.request.move_destination.trim().to_string(),
                },
            )
            .map_err(ProxyError::into_move)?;
        let forwarder = Arc::new(MoveForwarder {
            proxy: self.proxy.clone(),
            set,
            destination,
            destination_ae: request.request.move_destination.trim().to_string(),
            originator_ae: self.calling_ae_title(),
            originator_message_id: request.request.message_id,
            priority: request.request.priority,
            state: Mutex::new(ForwardState::default()),
        });
        let watched = forwarder.clone();
        after_ctx(ctx, move || watched.release());
        Ok(forwarder)
    }
    pub fn prepare_c_move_batch(
        &self,
        ctx: &Ctx,
        request: &CMoveRequestContext,
    ) -> Result<CMoveBatch> {
        let forwarder = self.prepare_move(ctx, request)?;
        let sub_operations = forwarder
            .set
            .instances
            .iter()
            .map(|i| CMoveSubOperationDescriptor {
                affected_sop_class_uid: i.sop_class_uid.clone(),
                affected_sop_instance_uid: i.sop_instance_uid.clone(),
            })
            .collect();
        let run = move |ctx: &Ctx, yield_: &mut CMoveBatchYield<'_>| -> Result<()> {
            let r = (|| {
                if forwarder.set.instances.is_empty() {
                    return Ok(());
                }
                if let Err(err) = forwarder.connect(ctx) {
                    if let Some(cause) = context_cause(ctx) {
                        return Err(join(cause, err));
                    }
                    return Err(new_c_move_scp_error(
                        STATUS_RETRIEVE_OUT_OF_RESOURCES_SUB_OPERATIONS,
                        "Unable to perform sub-operations",
                        Some(err),
                    ));
                }
                for instance in &forwarder.set.instances {
                    yield_(forwarder.forward(ctx, instance))?;
                }
                Ok(())
            })();
            forwarder.release();
            r
        };
        Ok(CMoveBatch {
            sub_operations,
            run: Some(Arc::new(run)),
        })
    }
    pub fn move_(
        &self,
        ctx: &Ctx,
        request: &CMoveRequestContext,
    ) -> Result<Vec<CMoveSubOperation>> {
        let forwarder = self.prepare_move(ctx, request)?;
        Ok(forwarder
            .set
            .instances
            .iter()
            .map(|instance| {
                let forwarder = forwarder.clone();
                let located = instance.clone();
                CMoveSubOperation {
                    affected_sop_class_uid: instance.sop_class_uid.clone(),
                    affected_sop_instance_uid: instance.sop_instance_uid.clone(),
                    store: Some(Arc::new(move |ctx: &Ctx| {
                        if let Err(err) = forwarder.connect(ctx) {
                            return CMoveSubOperationResult {
                                status: STATUS_C_STORE_OUT_OF_RESOURCES,
                                err: Some(err),
                            };
                        }
                        forwarder.forward(ctx, &located)
                    })),
                }
            })
            .collect())
    }
    pub fn get(&self, ctx: &Ctx, request: &CGetRequestContext) -> Result<Vec<CGetSubOperation>> {
        let set = self
            .locate(
                ctx,
                &RetrieveSourceRequest {
                    sop_class_uid: request.request.affected_sop_class_uid.clone(),
                    query_retrieve_level: request.query_retrieve_level.clone(),
                    identifier: Some(request.identifier.clone()),
                    priority: request.request.priority,
                    calling_ae_title: self.calling_ae_title(),
                    move_destination: String::new(),
                },
            )
            .map_err(ProxyError::into_get)?;
        Ok(set
            .instances
            .iter()
            .map(|instance| {
                let set = set.clone();
                let assoc = self.assoc.clone();
                let located = instance.clone();
                let load: LoadFile = Arc::new(move |ctx: &Ctx| {
                    let accepted = accepted_transfer_syntax_uids(&assoc, &located.sop_class_uid);
                    match set.fetch(ctx, &located, &accepted) {
                        Ok(file) => (Some(file), None),
                        Err(e) => (None, Some(e)),
                    }
                });
                CGetSubOperation {
                    affected_sop_class_uid: instance.sop_class_uid.clone(),
                    affected_sop_instance_uid: instance.sop_instance_uid.clone(),
                    load_file: Some(load),
                    ..Default::default()
                }
            })
            .collect())
    }
    pub fn c_move_batch_handler(self: &Arc<Self>) -> CMoveBatchHandler {
        let handler = self.clone();
        Arc::new(move |ctx: &Ctx, request: &CMoveRequestContext| {
            handler.prepare_c_move_batch(ctx, request)
        })
    }
    pub fn c_move_handler(self: &Arc<Self>) -> CMoveHandler {
        let handler = self.clone();
        Arc::new(move |ctx: &Ctx, request: &CMoveRequestContext| handler.move_(ctx, request))
    }
    pub fn c_get_handler(self: &Arc<Self>) -> CGetHandler {
        let handler = self.clone();
        Arc::new(move |ctx: &Ctx, request: &CGetRequestContext| handler.get(ctx, request))
    }
}
#[derive(Default)]
struct ForwardState {
    assoc: Option<Arc<Association>>,
    dial_err: Option<Error>,
    broken: Option<Error>,
    message_id: u16,
    released: bool,
}
struct MoveForwarder {
    proxy: Arc<RetrieveProxy>,
    set: Arc<ProxyRetrieveSet>,
    destination: MoveDestinationAddress,
    destination_ae: String,
    originator_ae: String,
    originator_message_id: u16,
    priority: u16,
    state: Mutex<ForwardState>,
}
fn release_timeout_or_default(options: &DialOptions) -> Duration {
    options
        .release_timeout
        .positive()
        .unwrap_or(Duration::from_secs(5))
}
impl MoveForwarder {
    fn state(&self) -> std::sync::MutexGuard<'_, ForwardState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn connect(&self, ctx: &Ctx) -> Result<()> {
        let mut st = self.state();
        if st.assoc.is_some() {
            return Ok(());
        }
        if let Some(e) = &st.dial_err {
            return Err(e.clone());
        }
        if st.released {
            let e = Error::new("dicom dimse: C-MOVE forwarding already finished");
            st.dial_err = Some(e.clone());
            return Err(e);
        }
        let contexts = match destination_contexts(
            &self.set.instances,
            &self.proxy.options.destination_transfer_syntax_uids,
        ) {
            Ok(c) => c,
            Err(e) => {
                st.dial_err = Some(e.clone());
                return Err(e);
            }
        };
        let mut options = self.proxy.options.destination_dial_options.clone();
        options.calling_ae_title = self.proxy.options.ae_title.clone();
        options.called_ae_title = self.destination_ae.clone();
        options.contexts = contexts;
        options.role_selections = Vec::new();
        options.extended_negotiation = Vec::new();
        options.common_extended_negotiation = Vec::new();
        options.asynchronous_operations_window = None;
        #[cfg(feature = "tls")]
        if self.destination.tls_config.is_some() {
            options.tls_config = self.destination.tls_config.clone();
        }
        match ul::dial_context(ctx, self.destination.address.trim(), options) {
            Ok(assoc) => {
                st.assoc = Some(Arc::new(assoc));
                Ok(())
            }
            Err(e) => {
                let e = Error::from(e).prefixed("dicom dimse: Move Destination association failed");
                st.dial_err = Some(e.clone());
                Err(e)
            }
        }
    }
    fn forward(&self, ctx: &Ctx, instance: &RetrieveInstance) -> CMoveSubOperationResult {
        let failed = |err: Error| CMoveSubOperationResult {
            status: STATUS_C_STORE_OUT_OF_RESOURCES,
            err: Some(err),
        };
        let (assoc, broken) = {
            let st = self.state();
            (st.assoc.clone(), st.broken.clone())
        };
        let Some(assoc) = assoc else {
            return failed(Error::new("dicom dimse: no Move Destination association"));
        };
        if let Some(broken) = broken {
            return failed(broken);
        }
        let not_accepted = || {
            Error::wrap(
                Kind::StorePresentationContextRejected,
                "SOP Class not accepted by the Move Destination",
            )
        };
        let mut class = instance.sop_class_uid.clone();
        let mut accepted = accepted_transfer_syntax_uids(&assoc, &class);
        if !class.is_empty() && accepted.is_empty() {
            return failed(not_accepted());
        }
        let file = match self.set.fetch(ctx, instance, &accepted) {
            Ok(f) => f,
            Err(e) => return failed(e),
        };
        if class.is_empty() {
            class = file.dataset.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
            accepted = accepted_transfer_syntax_uids(&assoc, &class);
            if accepted.is_empty() {
                return failed(not_accepted());
            }
        }
        let syntaxes = store_transfer_syntax_uids(file.transfer_syntax.uid, &accepted);
        if syntaxes.is_empty() {
            return failed(Error::wrap(
                Kind::StoreTransferSyntax,
                "no accepted context for the fetched transfer syntax",
            ));
        }
        let message_id = {
            let mut st = self.state();
            st.message_id = st.message_id.wrapping_add(1);
            if st.message_id == 0 {
                st.message_id = 1;
            }
            st.message_id
        };
        let stored = StoreClient::new(&assoc).store_with_options(
            Some(ctx),
            &file.dataset,
            CStoreOptions {
                affected_sop_class_uid: class,
                affected_sop_instance_uid: instance.sop_instance_uid.clone(),
                transfer_syntax_uids: syntaxes,
                message_id,
                priority: self.priority,
                move_originator_ae_title: self.originator_ae.clone(),
                move_originator_message_id: Some(self.originator_message_id),
            },
        );
        let (result, err) = match stored {
            Ok(r) => (r, None),
            Err((r, e)) => (r, Some(e)),
        };
        if let Some(response) = &result.response {
            return CMoveSubOperationResult {
                status: response.status,
                err: None,
            };
        }
        let err = err.unwrap_or_else(|| Error::new("dicom dimse: C-STORE returned no response"));
        if let Some(cause) = context_cause(ctx) {
            return failed(join(cause, err));
        }
        let missing = matches!(err.detail(), Detail::MissingPresentationContext(..));
        if !missing && !err.is(Kind::StoreTransferSyntax) {
            self.state().broken = Some(
                err.clone()
                    .prefixed("dicom dimse: Move Destination association failed"),
            );
            let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
        }
        failed(err)
    }
    fn release(&self) {
        let (assoc, broken) = {
            let mut st = self.state();
            st.released = true;
            (st.assoc.take(), st.broken.is_some())
        };
        if let Some(assoc) = assoc {
            if !broken {
                let ctx = Ctx::background().with_timeout(release_timeout_or_default(
                    &self.proxy.options.destination_dial_options,
                ));
                if assoc.release(Some(&ctx)).is_err() {
                    let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
                }
            }
            assoc.close();
        }
        self.set.close();
    }
}
fn destination_contexts(
    instances: &[RetrieveInstance],
    syntaxes: &[String],
) -> Result<Vec<PresentationContext>> {
    let mut classes: Vec<String> = Vec::new();
    let mut per_class: HashMap<String, Vec<String>> = HashMap::new();
    let mut unknown = false;
    for instance in instances {
        if instance.sop_class_uid.is_empty() {
            unknown = true;
            continue;
        }
        if !per_class.contains_key(&instance.sop_class_uid) {
            classes.push(instance.sop_class_uid.clone());
            per_class.insert(instance.sop_class_uid.clone(), Vec::new());
        }
        let hint = object::transfer::normalize_uid(&instance.transfer_syntax_uid);
        if !hint.is_empty()
            && let Some(hints) = per_class.get_mut(&instance.sop_class_uid)
        {
            hints.push(hint.to_string());
        }
    }
    if unknown {
        for class in default_c_get_storage_sop_class_uids() {
            if classes.len() >= 128 {
                break;
            }
            if let std::collections::hash_map::Entry::Vacant(slot) = per_class.entry(class.clone())
            {
                classes.push(class);
                slot.insert(Vec::new());
            }
        }
    }
    let mut total = 0;
    for class in &classes {
        if let Some(list) = per_class.get_mut(class) {
            list.extend(syntaxes.iter().cloned());
            *list = normalized_transfer_syntax_uids(list);
            total += list.len();
        }
    }
    if classes.len() > 128 {
        return Err(Error::new(format!(
            "dicom dimse: {} SOP Classes exceed the 128 presentation contexts of one association",
            classes.len()
        )));
    }
    let mut contexts = Vec::new();
    let mut id: u8 = 1;
    for class in &classes {
        let list = per_class.get(class).cloned().unwrap_or_default();
        if total <= 128 {
            for syntax in list {
                contexts.push(PresentationContext {
                    id,
                    abstract_syntax_uid: class.clone(),
                    transfer_syntax_uids: vec![syntax],
                });
                id = id.wrapping_add(2);
            }
            continue;
        }
        contexts.push(PresentationContext {
            id,
            abstract_syntax_uid: class.clone(),
            transfer_syntax_uids: list,
        });
        id = id.wrapping_add(2);
    }
    Ok(contexts)
}
#[derive(Clone, Default)]
pub struct DimseRetrieveSourceOptions {
    pub address: String,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub dial_options: DialOptions,
    pub transfer_syntax_uids: Vec<String>,
    pub max_instances: i64,
    pub response_timeout: Nanos,
    pub require_exact_inventory: bool,
}
pub struct DimseRetrieveSource {
    options: DimseRetrieveSourceOptions,
}
pub fn new_dimse_retrieve_source(
    mut options: DimseRetrieveSourceOptions,
) -> Result<Arc<DimseRetrieveSource>> {
    if options.address.trim().is_empty() {
        return Err(Error::new(
            "dicom dimse: DIMSE retrieve source address is required",
        ));
    }
    if options.max_instances <= 0 || options.max_instances > MAX_SUB_OPERATIONS {
        options.max_instances = MAX_SUB_OPERATIONS;
    }
    if options.transfer_syntax_uids.is_empty() {
        options.transfer_syntax_uids = vec![EXPLICIT_LE.into(), IMPLICIT_LE.into()];
    }
    options.transfer_syntax_uids = normalized_transfer_syntax_uids(&options.transfer_syntax_uids);
    Ok(Arc::new(DimseRetrieveSource { options }))
}
#[derive(Debug, Clone, Copy)]
struct DimseRetrieveModel {
    patient_root: bool,
    find: &'static str,
    get: &'static str,
}
fn dimse_retrieve_model_for(sop_class_uid: &str) -> Result<DimseRetrieveModel> {
    match sop_class_uid {
        PATIENT_ROOT_MOVE_SOP_CLASS_UID
        | PATIENT_ROOT_GET_SOP_CLASS_UID
        | PATIENT_ROOT_FIND_SOP_CLASS_UID => Ok(DimseRetrieveModel {
            patient_root: true,
            find: PATIENT_ROOT_FIND_SOP_CLASS_UID,
            get: PATIENT_ROOT_GET_SOP_CLASS_UID,
        }),
        STUDY_ROOT_MOVE_SOP_CLASS_UID
        | STUDY_ROOT_GET_SOP_CLASS_UID
        | STUDY_ROOT_FIND_SOP_CLASS_UID => Ok(DimseRetrieveModel {
            patient_root: false,
            find: STUDY_ROOT_FIND_SOP_CLASS_UID,
            get: STUDY_ROOT_GET_SOP_CLASS_UID,
        }),
        other => Err(Error::new(format!(
            "dicom dimse: unsupported Query/Retrieve model {}",
            crate::textfmt::quote(other)
        ))),
    }
}
fn string_element(tag: Tag, vr: Vr, values: &[&str]) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values.iter().map(|v| v.to_string()).collect()),
    }
}
fn ui_element(tag: Tag, values: &[&str]) -> Element {
    string_element(tag, Vr::UI, values)
}
fn retrieve_keys(level: &str, patient_root: bool, patient_id: &str, keys: Vec<Element>) -> Object {
    let mut elements = vec![string_element(TAG_QUERY_RETRIEVE_LEVEL, Vr::CS, &[level])];
    if patient_root {
        elements.push(string_element(TAG_PATIENT_ID, Vr::LO, &[patient_id]));
    }
    elements.extend(keys);
    Object::from_elements(elements)
}
fn identifier_strings(identifier: &Object, tag: Tag) -> Vec<String> {
    identifier
        .get_strings(tag)
        .unwrap_or_default()
        .iter()
        .map(|v| v.trim_end_matches('\0').trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}
fn single_identifier_value(identifier: &Object, tag: Tag, name: &str) -> Result<String> {
    let mut values = identifier_strings(identifier, tag);
    if values.len() != 1 {
        return Err(Error::new(format!(
            "dicom dimse: retrieve identifier needs one {name}"
        )));
    }
    Ok(values.remove(0))
}
fn uids_of(results: &[Object], tag: Tag) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for result in results {
        let uid = result.get_uid(tag).unwrap_or_default();
        let uid = uid.trim();
        if !uid.is_empty() && !out.iter().any(|u| u == uid) {
            out.push(uid.to_string());
        }
    }
    out
}
#[derive(Debug, Clone)]
struct DimseLocatedInstance {
    instance: RetrieveInstance,
    patient_id: String,
}
struct DimseLocator<'a> {
    source: &'a DimseRetrieveSource,
    model: DimseRetrieveModel,
    assoc: &'a Association,
    priority: u16,
    message_id: u16,
    matches: i64,
}
fn valid_retrieve_inventory_match(identifier: &Object, level: &str, requested: &[Element]) -> bool {
    let (returned, ok) = read_single_value(identifier, TAG_QUERY_RETRIEVE_LEVEL, Vr::CS);
    if !ok || returned != level {
        return false;
    }
    for key in requested {
        let Ok(value) = read_uid_list(identifier, key.tag(), false) else {
            return false;
        };
        let wanted: Vec<String> = match &key.value {
            Value::Strings(values) => values.clone(),
            _ => Vec::new(),
        };
        if !wanted.is_empty() && !value.first().is_some_and(|v| wanted.contains(v)) {
            return false;
        }
    }
    true
}
impl DimseLocator<'_> {
    fn find(
        &mut self,
        ctx: &Ctx,
        level: &str,
        patient_id: &str,
        keys: Vec<Element>,
    ) -> Result<Vec<Object>> {
        let client = FindClient::for_sop_class(self.assoc, self.model.find)?;
        self.message_id = self.message_id.wrapping_add(1);
        let cancel = ul::CancelToken::new();
        let find_ctx = ctx.with_cancel(&cancel);
        let identifier = retrieve_keys(level, self.model.patient_root, patient_id, keys.clone());
        let mut out: Vec<Object> = Vec::new();
        let mut refusal: Option<Error> = None;
        let exact = self.source.options.require_exact_inventory;
        let max = self.source.options.max_instances;
        let mut matches = self.matches;
        let found = client.find_with_options(
            OperationOptions {
                context: Some(find_ctx),
                response_timeout: self.source.options.response_timeout,
                ..Default::default()
            },
            CFindRequest {
                affected_sop_class_uid: self.model.find.to_string(),
                message_id: self.message_id,
                priority: self.priority,
            },
            Some(&identifier),
            &mut |result: FindResult| {
                if refusal.is_some()
                    || categorize_c_find_status(result.response.status)
                        != CFindStatusCategory::Pending
                {
                    return;
                }
                let Some(found) = result.identifier else {
                    return;
                };
                if exact && !valid_retrieve_inventory_match(&found, level, &keys) {
                    cancel.cancel();
                    refusal = Some(Error::new(
                        "dicom dimse: inconsistent C-FIND retrieve inventory",
                    ));
                    return;
                }
                matches += 1;
                if matches > max {
                    cancel.cancel();
                    refusal = Some(Error::new(format!(
                        "dicom dimse: upstream matches exceed {max}"
                    )));
                    return;
                }
                out.push(found);
            },
        );
        self.matches = matches;
        if let Some(e) = refusal {
            return Err(e);
        }
        found.map_err(|e| e.prefixed("dicom dimse: upstream C-FIND failed"))?;
        Ok(out)
    }
    fn walk(
        &mut self,
        ctx: &Ctx,
        level: &str,
        identifier: &Object,
    ) -> Result<Vec<DimseLocatedInstance>> {
        let mut patient_id = String::new();
        if self.model.patient_root {
            patient_id = single_identifier_value(identifier, TAG_PATIENT_ID, "Patient ID")?;
        }
        match level {
            QUERY_RETRIEVE_LEVEL_PATIENT => {
                if !self.model.patient_root {
                    return Err(Error::new(
                        "dicom dimse: PATIENT level needs the Patient Root model",
                    ));
                }
                let studies = self.find(
                    ctx,
                    QUERY_RETRIEVE_LEVEL_STUDY,
                    &patient_id,
                    vec![ui_element(TAG_STUDY_INSTANCE_UID, &[])],
                )?;
                let studies = uids_of(&studies, TAG_STUDY_INSTANCE_UID);
                self.studies(ctx, &patient_id, &studies)
            }
            QUERY_RETRIEVE_LEVEL_STUDY => {
                let studies = identifier_strings(identifier, TAG_STUDY_INSTANCE_UID);
                if studies.is_empty() {
                    return Err(Error::new(
                        "dicom dimse: retrieve identifier needs Study Instance UID",
                    ));
                }
                self.studies(ctx, &patient_id, &studies)
            }
            QUERY_RETRIEVE_LEVEL_SERIES => {
                let study = single_identifier_value(
                    identifier,
                    TAG_STUDY_INSTANCE_UID,
                    "Study Instance UID",
                )?;
                let series = identifier_strings(identifier, TAG_SERIES_INSTANCE_UID);
                if series.is_empty() {
                    return Err(Error::new(
                        "dicom dimse: retrieve identifier needs Series Instance UID",
                    ));
                }
                self.series(ctx, &patient_id, &study, &series, &[])
            }
            QUERY_RETRIEVE_LEVEL_IMAGE => {
                let study = single_identifier_value(
                    identifier,
                    TAG_STUDY_INSTANCE_UID,
                    "Study Instance UID",
                )?;
                let series = single_identifier_value(
                    identifier,
                    TAG_SERIES_INSTANCE_UID,
                    "Series Instance UID",
                )?;
                let instances = identifier_strings(identifier, TAG_SOP_INSTANCE_UID);
                if instances.is_empty() {
                    return Err(Error::new(
                        "dicom dimse: retrieve identifier needs SOP Instance UID",
                    ));
                }
                self.series(ctx, &patient_id, &study, &[series], &instances)
            }
            other => Err(Error::new(format!(
                "dicom dimse: unsupported retrieve level {}",
                crate::textfmt::quote(other)
            ))),
        }
    }
    fn studies(
        &mut self,
        ctx: &Ctx,
        patient_id: &str,
        studies: &[String],
    ) -> Result<Vec<DimseLocatedInstance>> {
        let mut out = Vec::new();
        for study in studies {
            let series = self.find(
                ctx,
                QUERY_RETRIEVE_LEVEL_SERIES,
                patient_id,
                vec![
                    ui_element(TAG_STUDY_INSTANCE_UID, &[study]),
                    ui_element(TAG_SERIES_INSTANCE_UID, &[]),
                ],
            )?;
            let series = uids_of(&series, TAG_SERIES_INSTANCE_UID);
            out.extend(self.series(ctx, patient_id, study, &series, &[])?);
        }
        Ok(out)
    }
    fn series(
        &mut self,
        ctx: &Ctx,
        patient_id: &str,
        study: &str,
        series: &[String],
        sop_instance_uids: &[String],
    ) -> Result<Vec<DimseLocatedInstance>> {
        let mut out = Vec::new();
        let wanted: Vec<&str> = sop_instance_uids.iter().map(String::as_str).collect();
        for uid in series {
            let results = self.find(
                ctx,
                QUERY_RETRIEVE_LEVEL_IMAGE,
                patient_id,
                vec![
                    ui_element(TAG_STUDY_INSTANCE_UID, &[study]),
                    ui_element(TAG_SERIES_INSTANCE_UID, &[uid]),
                    ui_element(TAG_SOP_INSTANCE_UID, &wanted),
                    ui_element(TAG_SOP_CLASS_UID, &[]),
                ],
            )?;
            for result in &results {
                let class = result.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
                let instance = result.get_uid(TAG_SOP_INSTANCE_UID).unwrap_or_default();
                if !is_valid_uid(instance.as_bytes())
                    || (!class.is_empty() && !is_valid_uid(class.as_bytes()))
                {
                    return Err(Error::new(
                        "dicom dimse: upstream C-FIND response lacks a valid SOP Instance UID",
                    ));
                }
                out.push(DimseLocatedInstance {
                    instance: RetrieveInstance {
                        sop_class_uid: class,
                        sop_instance_uid: instance,
                        study_instance_uid: study.to_string(),
                        series_instance_uid: uid.clone(),
                        transfer_syntax_uid: String::new(),
                    },
                    patient_id: patient_id.to_string(),
                });
            }
        }
        Ok(out)
    }
}
impl DimseRetrieveSource {
    fn dial(
        &self,
        ctx: &Ctx,
        contexts: Vec<PresentationContext>,
        roles: Vec<ul::RoleSelectionItem>,
    ) -> Result<Association> {
        let mut options = self.options.dial_options.clone();
        options.called_ae_title = self.options.called_ae_title.clone();
        options.calling_ae_title = self.options.calling_ae_title.clone();
        options.contexts = contexts;
        options.role_selections = roles;
        options.extended_negotiation = Vec::new();
        options.common_extended_negotiation = Vec::new();
        options.asynchronous_operations_window = None;
        ul::dial_context(ctx, self.options.address.trim(), options)
            .map_err(|e| Error::from(e).prefixed("dicom dimse: upstream association failed"))
    }
    fn locate_set(
        self: &Arc<Self>,
        ctx: &Ctx,
        request: &RetrieveSourceRequest,
    ) -> Result<Arc<dyn RetrieveSet>> {
        let model = dimse_retrieve_model_for(&request.sop_class_uid)?;
        let Some(identifier) = &request.identifier else {
            return Err(Error::new("dicom dimse: retrieve identifier is required"));
        };
        let assoc = self.dial(
            ctx,
            vec![PresentationContext {
                id: 1,
                abstract_syntax_uid: model.find.to_string(),
                transfer_syntax_uids: vec![EXPLICIT_LE.into(), IMPLICIT_LE.into()],
            }],
            Vec::new(),
        )?;
        let mut walker = DimseLocator {
            source: self,
            model,
            assoc: &assoc,
            priority: request.priority,
            message_id: 0,
            matches: 0,
        };
        let level = request.query_retrieve_level.trim().to_uppercase();
        let instances = match walker.walk(ctx, &level, identifier) {
            Ok(i) => i,
            Err(e) => {
                let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
                assoc.close();
                return Err(e);
            }
        };
        let release_parent = if self.options.require_exact_inventory {
            ctx.clone()
        } else {
            Ctx::background()
        };
        let release_ctx =
            release_parent.with_timeout(release_timeout_or_default(&self.options.dial_options));
        if assoc.release(Some(&release_ctx)).is_err() {
            let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
        }
        assoc.close();
        let patient_ids = instances
            .iter()
            .map(|i| (i.instance.sop_instance_uid.clone(), i.patient_id.clone()))
            .collect();
        Ok(Arc::new(DimseRetrieveSet {
            source: self.clone(),
            model,
            priority: request.priority,
            instances: instances.into_iter().map(|i| i.instance).collect(),
            patient_ids,
            state: Mutex::new(DimseSetState::default()),
        }))
    }
}
impl RetrieveSource for Arc<DimseRetrieveSource> {
    fn locate(
        &self,
        ctx: &Ctx,
        request: &RetrieveSourceRequest,
    ) -> Result<Option<Arc<dyn RetrieveSet>>> {
        self.locate_set(ctx, request).map(Some)
    }
}
#[derive(Default)]
struct DimseSetState {
    assoc: Option<Arc<Association>>,
    message_id: u16,
    closed: bool,
}
struct DimseRetrieveSet {
    source: Arc<DimseRetrieveSource>,
    model: DimseRetrieveModel,
    priority: u16,
    instances: Vec<RetrieveInstance>,
    patient_ids: HashMap<String, String>,
    state: Mutex<DimseSetState>,
}
impl DimseRetrieveSet {
    fn get_contexts(&self) -> Result<(Vec<PresentationContext>, Vec<ul::RoleSelectionItem>)> {
        let mut contexts = vec![PresentationContext {
            id: 1,
            abstract_syntax_uid: self.model.get.to_string(),
            transfer_syntax_uids: vec![EXPLICIT_LE.into(), IMPLICIT_LE.into()],
        }];
        let mut classes: Vec<String> = Vec::new();
        let mut unknown = false;
        for instance in &self.instances {
            if instance.sop_class_uid.is_empty() {
                unknown = true;
            } else if !classes.contains(&instance.sop_class_uid) {
                classes.push(instance.sop_class_uid.clone());
            }
        }
        if classes.len() > 127 {
            return Err(Error::new(format!(
                "dicom dimse: {} SOP Classes exceed one C-GET association",
                classes.len()
            )));
        }
        if unknown {
            for class in default_c_get_storage_sop_class_uids() {
                if classes.len() >= 127 {
                    break;
                }
                if !classes.contains(&class) {
                    classes.push(class);
                }
            }
        }
        let syntaxes = &self.source.options.transfer_syntax_uids;
        let per_syntax = classes.len() * syntaxes.len() <= 127;
        let mut id: u8 = 3;
        let mut roles = Vec::with_capacity(classes.len());
        for class in &classes {
            roles.push(ul::RoleSelectionItem {
                sop_class_uid: class.clone(),
                scu_role: false,
                scp_role: true,
            });
            if !per_syntax {
                contexts.push(PresentationContext {
                    id,
                    abstract_syntax_uid: class.clone(),
                    transfer_syntax_uids: syntaxes.clone(),
                });
                id = id.wrapping_add(2);
                continue;
            }
            for syntax in syntaxes {
                contexts.push(PresentationContext {
                    id,
                    abstract_syntax_uid: class.clone(),
                    transfer_syntax_uids: vec![syntax.clone()],
                });
                id = id.wrapping_add(2);
            }
        }
        Ok((contexts, roles))
    }
}
impl RetrieveSet for DimseRetrieveSet {
    fn instances(&self) -> Vec<RetrieveInstance> {
        self.instances.clone()
    }
    fn fetch(
        &self,
        ctx: &Ctx,
        instance: &RetrieveInstance,
        _syntaxes: &[String],
    ) -> Result<Option<Arc<File>>> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.closed {
            return Err(Error::new("dicom dimse: retrieve set is closed"));
        }
        let assoc = match &st.assoc {
            Some(a) => a.clone(),
            None => {
                let (contexts, roles) = self.get_contexts()?;
                let assoc = Arc::new(self.source.dial(ctx, contexts, roles)?);
                st.assoc = Some(assoc.clone());
                assoc
            }
        };
        let pc = accepted_context_for_sop_class(&assoc, self.model.get)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        st.message_id = st.message_id.wrapping_add(1);
        if st.message_id == 0 {
            st.message_id = 1;
        }
        let received: Arc<Mutex<Option<File>>> = Arc::new(Mutex::new(None));
        let slot = received.clone();
        let wanted = instance.sop_instance_uid.clone();
        let handler: CGetStoreHandler =
            Arc::new(move |_: &Ctx, store: &CGetStoreRequestContext| {
                let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                if slot.is_some() || store.request.affected_sop_instance_uid != wanted {
                    return (
                        super::store::STATUS_C_STORE_CANNOT_UNDERSTAND,
                        Some(Error::new("dicom dimse: unexpected instance from upstream")),
                    );
                }
                *slot = Some(File {
                    preamble: Vec::new(),
                    meta: Object::new(),
                    dataset: store.data_set.clone(),
                    transfer_syntax: store.data_set_syntax,
                    partial_read: None,
                    incomplete_pixel_data: None,
                });
                (STATUS_SUCCESS, None)
            });
        let patient_id = self
            .patient_ids
            .get(&instance.sop_instance_uid)
            .cloned()
            .unwrap_or_default();
        let identifier = retrieve_keys(
            QUERY_RETRIEVE_LEVEL_IMAGE,
            self.model.patient_root,
            &patient_id,
            vec![
                ui_element(TAG_STUDY_INSTANCE_UID, &[&instance.study_instance_uid]),
                ui_element(TAG_SERIES_INSTANCE_UID, &[&instance.series_instance_uid]),
                ui_element(TAG_SOP_INSTANCE_UID, &[&instance.sop_instance_uid]),
            ],
        );
        let response = send_c_get_with_progress(
            Some(ctx),
            &assoc,
            pc.id,
            &CGetRequest {
                affected_sop_class_uid: self.model.get.to_string(),
                message_id: st.message_id,
                priority: self.priority,
            },
            Some(&identifier),
            &syntax,
            crate::net::dimse::CGetHandlers {
                store: Some(&handler),
                on_progress: &mut |_| {},
            },
        );
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
                assoc.close();
                st.assoc = None;
                return Err(e.prefixed("dicom dimse: upstream C-GET failed"));
            }
        };
        let file = received.lock().unwrap_or_else(|e| e.into_inner()).take();
        match file {
            Some(file) => Ok(Some(Arc::new(file))),
            None => Err(Error::new(format!(
                "dicom dimse: upstream C-GET ended with status 0x{:04X} without the instance",
                response.status
            ))),
        }
    }
    fn close(&self) -> Result<()> {
        let assoc = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.closed = true;
            st.assoc.take()
        };
        let Some(assoc) = assoc else {
            return Ok(());
        };
        let ctx = Ctx::background().with_timeout(release_timeout_or_default(
            &self.source.options.dial_options,
        ));
        let released = assoc.release(Some(&ctx)).map_err(Error::from);
        if released.is_err() {
            let _ = assoc.abort(ul::ABORT_REASON_NOT_SPECIFIED);
        }
        assoc.close();
        released
    }
}
#[derive(Clone, Copy)]
pub struct RetrieveClient<'a> {
    pub assoc: Option<&'a Association>,
    pub presentation_ctx_id: u8,
    pub identifier_syntax: Option<Syntax>,
}
impl<'a> RetrieveClient<'a> {
    pub fn new(
        assoc: Option<&'a Association>,
        pc_id: u8,
        identifier_syntax: Option<Syntax>,
    ) -> RetrieveClient<'a> {
        RetrieveClient {
            assoc,
            presentation_ctx_id: pc_id,
            identifier_syntax,
        }
    }
    pub fn for_sop_class(
        assoc: Option<&'a Association>,
        sop_class_uid: &str,
    ) -> Result<RetrieveClient<'a>> {
        let Some(assoc) = assoc else {
            return Err(Error::new("dicom dimse: nil association"));
        };
        let pc = accepted_context_for_sop_class(assoc, sop_class_uid)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        Ok(RetrieveClient::new(Some(assoc), pc.id, Some(syntax)))
    }
    fn validate(&self) -> Result<(&'a Association, Syntax)> {
        let Some(assoc) = self.assoc else {
            return Err(Error::new(
                "dicom dimse: retrieve client association is nil",
            ));
        };
        if self.presentation_ctx_id == 0 {
            return Err(Error::new(
                "dicom dimse: retrieve client presentation context ID is required",
            ));
        }
        let Some(syntax) = self.identifier_syntax else {
            return Err(Error::new(
                "dicom dimse: retrieve client identifier transfer syntax is required",
            ));
        };
        Ok((assoc, syntax))
    }
    pub fn move_with_progress(
        &self,
        ctx: Option<&Ctx>,
        req: &CMoveRequest,
        identifier: Option<&Object>,
        on_progress: &mut dyn FnMut(&CMoveProgress),
    ) -> Result<CMoveResponse> {
        let (assoc, syntax) = self.validate()?;
        send_c_move_with_options(
            OperationOptions {
                context: ctx.cloned(),
                ..Default::default()
            },
            assoc,
            self.presentation_ctx_id,
            req,
            identifier,
            &syntax,
            on_progress,
        )
    }
    pub fn move_(
        &self,
        ctx: Option<&Ctx>,
        req: &CMoveRequest,
        identifier: Option<&Object>,
    ) -> Result<CMoveResponse> {
        self.move_with_progress(ctx, req, identifier, &mut |_| {})
    }
    pub fn get_with_progress(
        &self,
        ctx: Option<&Ctx>,
        req: &CGetRequest,
        identifier: Option<&Object>,
        store_handler: Option<&CGetStoreHandler>,
        on_progress: &mut dyn FnMut(&CGetProgress),
    ) -> Result<CGetResponse> {
        let (assoc, syntax) = self.validate()?;
        send_c_get_with_progress(
            ctx,
            assoc,
            self.presentation_ctx_id,
            req,
            identifier,
            &syntax,
            crate::net::dimse::CGetHandlers {
                store: store_handler,
                on_progress,
            },
        )
    }
    pub fn get(
        &self,
        ctx: Option<&Ctx>,
        req: &CGetRequest,
        identifier: Option<&Object>,
        store_handler: Option<&CGetStoreHandler>,
    ) -> Result<CGetResponse> {
        self.get_with_progress(ctx, req, identifier, store_handler, &mut |_| {})
    }
}
