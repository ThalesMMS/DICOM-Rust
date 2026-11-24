use super::manifest::{DICOM_MEDIA_TYPE, Manifest, patient_id_from, single_string};
use super::metadata::{PublicationMetadata, publication_body};
use super::read::read_part10;
use super::wire::{
    ACTION_PUBLISH, ACTION_PUBLISH_RESPONSE, ACTION_RETRIEVE, ACTION_RETRIEVE_RESPONSE, Attachment,
    NS_RAD, NS_RS, NS_XDS, NS_XOP, Node, STATUS_FAILURE, STATUS_PARTIAL, STATUS_SUCCESS, element,
    encode_mtom, envelope, new_message_id, read_mtom, response_body,
};
use super::{
    Error, InstanceRef, Limits, Operation, PatientId, ctx_err, normalize_limits, trim_unicode_space,
};
use crate::net::dicomweb::http::{Body, HttpRequest, RedirectCheck, TransportError, send};
use crate::net::dicomweb::multipart::{ByteRead, ReadErr, SliceReader};
use crate::net::dicomweb::urlrules;
use crate::net::dicomweb::{HttpsIdentity, HttpsOptions};
use crate::net::ul::Ctx;
use crate::object::is_valid_uid;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
pub type MappingFn = Arc<dyn Fn(&Ctx, &PatientId) -> Result<PatientId, Error> + Send + Sync>;
pub type AuthorizeFn =
    Arc<dyn Fn(&Ctx, Operation, &[InstanceRef]) -> Result<(), Error> + Send + Sync>;
pub type Release = Box<dyn FnOnce() + Send>;
pub type HoldFn =
    Arc<dyn Fn(&Ctx, &[InstanceRef]) -> (Option<Release>, Option<Error>) + Send + Sync>;
#[derive(Clone, Debug, Default)]
pub struct HttpOptions {
    pub timeout: Duration,
    pub https: HttpsOptions,
    pub identity: Option<HttpsIdentity>,
}
#[derive(Clone, Default)]
pub struct ClientConfig {
    pub http: Option<HttpOptions>,
    pub repository_endpoint: String,
    pub imaging_endpoint: String,
    pub imaging_source_uid: String,
    pub allow_http: bool,
    pub limits: Limits,
    pub mapping: Option<MappingFn>,
    pub authorize: Option<AuthorizeFn>,
    pub hold_instances: Option<HoldFn>,
}
pub struct Client {
    http: HttpOptions,
    repository: urlrules::Url,
    imaging: urlrules::Url,
    repository_text: String,
    imaging_text: String,
    source_uid: String,
    limits: Limits,
    mapping: Option<MappingFn>,
    authorize: AuthorizeFn,
    hold: Option<HoldFn>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistryError {
    pub code: String,
    pub severity: String,
    pub location: String,
    pub context: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicationResult {
    pub status: String,
    pub entry_uuid: String,
    pub submission_entry_uuid: String,
    pub errors: Vec<RegistryError>,
}
impl PublicationResult {
    pub fn successful(&self) -> bool {
        self.status == STATUS_SUCCESS
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceResult {
    pub reference: InstanceRef,
    pub data: Option<Vec<u8>>,
    pub sha256: [u8; 32],
    pub transfer_syntax_uid: String,
    pub errors: Vec<RegistryError>,
    pub err: Option<Error>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetrievalResult {
    pub status: String,
    pub instances: Vec<InstanceResult>,
    pub errors: Vec<RegistryError>,
}
fn endpoint(raw: &str, allow_http: bool) -> Option<urlrules::Url> {
    let u = urlrules::parse(raw).ok()?;
    let scheme_ok = u.scheme == "https" || (allow_http && u.scheme == "http");
    (!u.host.is_empty()
        && u.user.is_none()
        && u.fragment.is_empty()
        && u.raw_query.is_empty()
        && scheme_ok)
        .then_some(u)
}
impl Client {
    pub fn new(config: ClientConfig) -> Result<Client, Error> {
        let limits = normalize_limits(config.limits)?;
        let (Some(http), Some(authorize)) = (config.http, config.authorize) else {
            return Err(Error::Mapping);
        };
        if !is_valid_uid(config.imaging_source_uid.as_bytes()) {
            return Err(Error::Mapping);
        }
        let repository = endpoint(&config.repository_endpoint, config.allow_http);
        let imaging = endpoint(&config.imaging_endpoint, config.allow_http);
        let (Some(repository), Some(imaging)) = (repository, imaging) else {
            return Err(Error::Mapping);
        };
        let mut http = http;
        if http.timeout.is_zero() {
            http.timeout = Duration::from_secs(30);
        }
        Ok(Client {
            http,
            repository,
            imaging,
            repository_text: config.repository_endpoint,
            imaging_text: config.imaging_endpoint,
            source_uid: config.imaging_source_uid,
            limits,
            mapping: config.mapping,
            authorize,
            hold: config.hold_instances,
        })
    }
    fn authorized(&self, ctx: &Ctx, op: Operation, refs: &[InstanceRef]) -> Result<(), Error> {
        (self.authorize)(ctx, op, refs).map_err(|e| Error::Authorization(Some(Box::new(e))))
    }
    pub fn publish(
        &self,
        ctx: &Ctx,
        manifest: Option<&Manifest>,
        metadata: &PublicationMetadata,
    ) -> Result<PublicationResult, Error> {
        ctx_err(ctx)?;
        metadata.validate(manifest)?;
        let Some(manifest) = manifest else {
            return Err(Error::Mapping);
        };
        let Some(mapping) = self
            .mapping
            .as_ref()
            .filter(|_| manifest.source_uid == self.source_uid)
        else {
            return Err(Error::Mapping);
        };
        let mapped = mapping(ctx, &manifest.patient)?;
        if !mapped.valid() || mapped != metadata.patient {
            return Err(Error::Mapping);
        }
        self.authorized(ctx, Operation::Publish, &manifest.refs)?;
        let Some(hold) = self.hold.as_ref() else {
            return Err(Error::Availability);
        };
        let (release, err) = hold(ctx, &manifest.refs);
        let held = ReleaseOnDrop(release);
        if let Some(err) = err {
            return Err(err);
        }
        if held.0.is_none() {
            return Err(Error::Availability);
        }
        let entry_id = new_message_id();
        let set_id = new_message_id();
        let msg = new_message_id();
        let body = publication_body(manifest, metadata, &entry_id, &set_id);
        let (root, attachments) = self.exchange(
            ctx,
            &self.repository,
            &self.repository_text,
            ACTION_PUBLISH,
            ACTION_PUBLISH_RESPONSE,
            &msg,
            body,
            &[Attachment {
                id: "manifest@xdsi".into(),
                media_type: DICOM_MEDIA_TYPE.into(),
                data: manifest.data.clone(),
            }],
            1,
        )?;
        if !attachments.is_empty() || !root.name.is(NS_RS, "RegistryResponse") {
            return Err(Error::Protocol);
        }
        let (status, errors) = match registry_response(&root) {
            Ok((status, _)) if status == STATUS_PARTIAL => return Err(Error::Protocol),
            Ok(r) => r,
            Err(_) => return Err(Error::Protocol),
        };
        self.authorized(ctx, Operation::Publish, &manifest.refs)?;
        ctx_err(ctx)?;
        Ok(PublicationResult {
            status,
            entry_uuid: entry_id,
            submission_entry_uuid: set_id,
            errors,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn exchange(
        &self,
        ctx: &Ctx,
        url: &urlrules::Url,
        endpoint: &str,
        action: &str,
        response_action: &str,
        msg: &str,
        body: Node,
        attachments: &[Attachment],
        max_parts: usize,
    ) -> Result<(Node, Vec<Attachment>), Error> {
        let (wire, content_type) =
            encode_mtom(&envelope(action, endpoint, msg, body), action, attachments)?;
        if wire.len() as i64 > self.limits.total_bytes {
            return Err(Error::Limit);
        }
        let exchange_ctx = ctx.with_timeout(self.http.timeout);
        let mut reader = SliceReader::new(&wire);
        let request = HttpRequest {
            method: "POST",
            url,
            headers: vec![
                ("Content-Type".into(), content_type),
                (
                    "Accept".into(),
                    "multipart/related; type=\"application/xop+xml\"".into(),
                ),
            ],
            body: Some(&mut reader as &mut dyn ByteRead),
            https: (url.scheme == "https").then_some(&self.http.https),
            identity: self.http.identity.as_ref(),
        };
        let mut response =
            send(request, RedirectCheck::NoFollow, &exchange_ctx).map_err(transport_error)?;
        if response.status_code != 200 {
            return Err(Error::PeerStatus(response.status_code));
        }
        if response.content_length > self.limits.total_bytes {
            return Err(Error::Limit);
        }
        let mut limited = LimitedBody {
            body: &mut response.body,
            n: self.limits.total_bytes + 1,
        };
        let content_type = response.header.get("Content-Type").to_string();
        let (root, parts) = read_mtom(&content_type, &mut limited, self.limits, max_parts)?;
        let mut sink = vec![0u8; 32 << 10];
        loop {
            match limited.read(&mut sink) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => return Err(Error::Other(e.to_string())),
            }
        }
        if limited.n == 0 {
            return Err(Error::Limit);
        }
        let payload = response_body(root, response_action, msg)?;
        Ok((payload, parts))
    }
    pub fn retrieve(
        &self,
        ctx: &Ctx,
        manifest: Option<&Manifest>,
        syntaxes: &[String],
    ) -> Result<RetrievalResult, Error> {
        ctx_err(ctx)?;
        let Some(manifest) = manifest.filter(|m| m.source_uid == self.source_uid) else {
            return Err(Error::Mapping);
        };
        if syntaxes.is_empty() || syntaxes.len() > 32 {
            return Err(Error::Manifest);
        }
        for (i, syntax) in syntaxes.iter().enumerate() {
            if !is_valid_uid(syntax.as_bytes()) || syntaxes[..i].contains(syntax) {
                return Err(Error::Manifest);
            }
        }
        self.authorized(ctx, Operation::Retrieve, &manifest.refs)?;
        let body = retrieve_body(manifest, syntaxes);
        let msg = new_message_id();
        let (root, parts) = self.exchange(
            ctx,
            &self.imaging,
            &self.imaging_text,
            ACTION_RETRIEVE,
            ACTION_RETRIEVE_RESPONSE,
            &msg,
            body,
            &[],
            manifest.refs.len() + 1,
        )?;
        if !root.name.is(NS_XDS, "RetrieveDocumentSetResponse") {
            return Err(Error::Protocol);
        }
        let registry = root.one(NS_RS, "RegistryResponse")?;
        let (status, registry_errors) = registry_response(registry)?;
        if root.children.iter().any(|c| {
            !c.name.is(NS_RS, "RegistryResponse") && !c.name.is(NS_XDS, "DocumentResponse")
        }) {
            return Err(Error::Protocol);
        }
        let mut result = RetrievalResult {
            status: status.clone(),
            errors: registry_errors.clone(),
            instances: manifest
                .refs
                .iter()
                .map(|r| InstanceResult {
                    reference: r.clone(),
                    ..InstanceResult::default()
                })
                .collect(),
        };
        let index: HashMap<&str, usize> = manifest
            .refs
            .iter()
            .enumerate()
            .map(|(i, r)| (r.sop_instance_uid.as_str(), i))
            .collect();
        for entry in &registry_errors {
            if entry.location.is_empty() && status == STATUS_FAILURE {
                continue;
            }
            let Some(&i) = index.get(entry.location.as_str()) else {
                return Err(Error::Protocol);
            };
            result.instances[i].errors.push(entry.clone());
        }
        let mut parts: Vec<Option<Attachment>> = parts.into_iter().map(Some).collect();
        let mut seen: HashMap<String, bool> = HashMap::with_capacity(index.len());
        for node in root.children_named(NS_XDS, "DocumentResponse") {
            let id = text_child(node, NS_XDS, "DocumentUniqueId")?;
            let i = match index.get(id.as_str()) {
                Some(&i) if !seen.contains_key(&id) => i,
                _ => return Err(Error::Protocol),
            };
            seen.insert(id, true);
            match text_child(node, NS_XDS, "RepositoryUniqueId") {
                Ok(repository)
                    if repository == self.source_uid
                        && node
                            .children_named(NS_XDS, "HomeCommunityId")
                            .next()
                            .is_none() => {}
                _ => return Err(Error::Protocol),
            }
            match text_child(node, NS_XDS, "mimeType") {
                Ok(media) if media == DICOM_MEDIA_TYPE => {}
                _ => return Err(Error::Protocol),
            }
            let document = node.one(NS_XDS, "Document")?;
            let include = match document.one(NS_XOP, "Include") {
                Ok(include)
                    if document.children.len() == 1
                        && trim_unicode_space(&document.text).is_empty()
                        && include.children.is_empty()
                        && trim_unicode_space(&include.text).is_empty() =>
                {
                    include
                }
                _ => return Err(Error::Protocol),
            };
            let Some(cid) = include.attr("href").strip_prefix("cid:") else {
                return Err(Error::Protocol);
            };
            let slot = parts
                .iter_mut()
                .find(|p| p.as_ref().is_some_and(|p| p.id == cid));
            let part = match slot.and_then(Option::take) {
                Some(part) if part.media_type == DICOM_MEDIA_TYPE => part,
                _ => return Err(Error::Protocol),
            };
            let out = &mut result.instances[i];
            if has_registry_error(&out.errors) {
                return Err(Error::Protocol);
            }
            match validate_instance(
                ctx,
                &part.data,
                &out.reference,
                &manifest.patient,
                syntaxes,
                self.limits,
            ) {
                Ok(syntax) => {
                    out.sha256 = Sha256::digest(&part.data).into();
                    out.data = Some(part.data);
                    out.transfer_syntax_uid = syntax;
                }
                Err(err) => out.err = Some(err),
            }
        }
        if parts.iter().any(Option::is_some) {
            return Err(Error::Protocol);
        }
        let received = seen.len();
        if (status == STATUS_SUCCESS && received != index.len())
            || (status == STATUS_PARTIAL && (received == 0 || received == index.len()))
            || (status == STATUS_FAILURE && received != 0)
        {
            return Err(Error::Protocol);
        }
        for out in &mut result.instances {
            if !seen.contains_key(&out.reference.sop_instance_uid) {
                if !has_registry_error(&out.errors) && status != STATUS_FAILURE {
                    return Err(Error::Protocol);
                }
                out.err = Some(Error::Availability);
            }
        }
        self.authorized(ctx, Operation::Retrieve, &manifest.refs)?;
        ctx_err(ctx)?;
        Ok(result)
    }
}
struct ReleaseOnDrop(Option<Release>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            release();
        }
    }
}
fn transport_error(e: TransportError) -> Error {
    match e {
        TransportError::Timeout => Error::Transport {
            timeout: true,
            canceled: false,
        },
        TransportError::Canceled => Error::Transport {
            timeout: false,
            canceled: true,
        },
        _ => Error::Transport {
            timeout: false,
            canceled: false,
        },
    }
}
struct LimitedBody<'a> {
    body: &'a mut Body,
    n: i64,
}
impl Read for LimitedBody<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.n <= 0 || buf.is_empty() {
            return Ok(0);
        }
        let want = buf.len().min(usize::try_from(self.n).unwrap_or(usize::MAX));
        let (n, err) = ByteRead::read(self.body, &mut buf[..want]);
        self.n -= n as i64;
        if n > 0 {
            return Ok(n);
        }
        match err {
            None | Some(ReadErr::Eof) => Ok(0),
            Some(ReadErr::UnexpectedEof) => Err(std::io::ErrorKind::UnexpectedEof.into()),
            Some(e) => Err(std::io::Error::other(e.to_string())),
        }
    }
}
fn registry_response(node: &Node) -> Result<(String, Vec<RegistryError>), Error> {
    if !node.name.is(NS_RS, "RegistryResponse") {
        return Err(Error::Protocol);
    }
    let status = node.attr("status");
    if status != STATUS_SUCCESS && status != STATUS_PARTIAL && status != STATUS_FAILURE {
        return Err(Error::Protocol);
    }
    let mut errors = Vec::new();
    let mut lists = node.children_named(NS_RS, "RegistryErrorList");
    let list = lists.next();
    if lists.next().is_some() {
        return Err(Error::Protocol);
    }
    if let Some(list) = list {
        for child in &list.children {
            if !child.name.is(NS_RS, "RegistryError") {
                return Err(Error::Protocol);
            }
            let (code, severity) = (child.attr("errorCode"), child.attr("severity"));
            if code.is_empty()
                || (severity != "urn:oasis:names:tc:ebxml-regrep:ErrorSeverityType:Error"
                    && severity != "urn:oasis:names:tc:ebxml-regrep:ErrorSeverityType:Warning")
            {
                return Err(Error::Protocol);
            }
            if status == STATUS_SUCCESS && severity.ends_with(":Error") {
                return Err(Error::Protocol);
            }
            errors.push(RegistryError {
                code: code.into(),
                severity: severity.into(),
                location: child.attr("location").into(),
                context: child.attr("codeContext").into(),
            });
        }
    }
    if status != STATUS_SUCCESS && !has_registry_error(&errors) {
        return Err(Error::Protocol);
    }
    Ok((status.to_string(), errors))
}
fn text_child(node: &Node, ns: &str, local: &str) -> Result<String, Error> {
    match node.one(ns, local) {
        Ok(child) if child.children.is_empty() => Ok(trim_unicode_space(&child.text).to_string()),
        _ => Err(Error::Protocol),
    }
}
fn has_registry_error(errors: &[RegistryError]) -> bool {
    errors.iter().any(|e| e.severity.ends_with(":Error"))
}
fn retrieve_body(manifest: &Manifest, syntaxes: &[String]) -> Node {
    let mut study = element(
        NS_RAD,
        "StudyRequest",
        "",
        &[("studyInstanceUID", &manifest.study_uid)],
    );
    let mut series_index: HashMap<&str, usize> = HashMap::new();
    for r in &manifest.refs {
        let i = *series_index
            .entry(r.series_uid.as_str())
            .or_insert_with(|| {
                study.children.push(element(
                    NS_RAD,
                    "SeriesRequest",
                    "",
                    &[("seriesInstanceUID", &r.series_uid)],
                ));
                study.children.len() - 1
            });
        study.children[i]
            .children
            .push(element(NS_XDS, "DocumentRequest", "", &[]).with(vec![
                element(NS_XDS, "RepositoryUniqueId", &manifest.source_uid, &[]),
                element(NS_XDS, "DocumentUniqueId", &r.sop_instance_uid, &[]),
            ]));
    }
    let list = element(NS_RAD, "TransferSyntaxUIDList", "", &[]).with(
        syntaxes
            .iter()
            .map(|s| element(NS_RAD, "TransferSyntaxUID", s, &[]))
            .collect(),
    );
    element(NS_RAD, "RetrieveImagingDocumentSetRequest", "", &[]).with(vec![study, list])
}
const TAG_STUDY: crate::object::Tag = crate::object::Tag::new(0x0020, 0x000D);
const TAG_SERIES: crate::object::Tag = crate::object::Tag::new(0x0020, 0x000E);
const TAG_SOP_CLASS: crate::object::Tag = crate::object::Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE: crate::object::Tag = crate::object::Tag::new(0x0008, 0x0018);
const TAG_MEDIA_CLASS: crate::object::Tag = crate::object::Tag::new(0x0002, 0x0002);
const TAG_MEDIA_INSTANCE: crate::object::Tag = crate::object::Tag::new(0x0002, 0x0003);
fn validate_instance(
    ctx: &Ctx,
    data: &[u8],
    r: &InstanceRef,
    patient: &PatientId,
    syntaxes: &[String],
    limits: Limits,
) -> Result<String, Error> {
    let file = match read_part10(ctx, data, limits) {
        Ok(file) => file,
        Err(_) => {
            ctx_err(ctx)?;
            return Err(Error::Identity);
        }
    };
    let ds = &file.dataset;
    let ok = patient_id_from(ds).is_some_and(|p| p == *patient)
        && single_string(ds, TAG_STUDY, &r.study_uid)
        && single_string(ds, TAG_SERIES, &r.series_uid)
        && single_string(ds, TAG_SOP_CLASS, &r.sop_class_uid)
        && single_string(ds, TAG_SOP_INSTANCE, &r.sop_instance_uid)
        && single_string(&file.meta, TAG_MEDIA_CLASS, &r.sop_class_uid)
        && single_string(&file.meta, TAG_MEDIA_INSTANCE, &r.sop_instance_uid)
        && syntaxes.contains(&file.transfer_syntax_uid);
    if !ok {
        return Err(Error::Identity);
    }
    Ok(file.transfer_syntax_uid)
}
