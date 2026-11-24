use super::*;
use crate::net::dimse::{
    AssociationScpOptions, NormalizedRequest, NormalizedResponse, NormalizedScpOptions,
    NormalizedScpResult, NormalizedStatusFields,
};
use crate::net::ul::{AcceptedContext, Association, Nanos};
use crate::object::{DataSet, is_valid_uid};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub const MAX_IMAGE_BOXES_CEILING: i64 = 4096;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_film_boxes: i64,
    pub max_image_boxes: i64,
    pub max_dimension: i64,
    pub max_image_bytes: i64,
    pub max_session_bytes: i64,
    pub max_copies: i64,
    pub print_timeout: Nanos,
}
pub type Clock = Arc<dyn Fn() -> Time + Send + Sync>;
pub type UidGenerator = Arc<dyn Fn() -> std::result::Result<String, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServiceOptions {
    pub limits: Limits,
    pub physical: Option<PhysicalServiceOptions>,
    pub native: Option<NativeServiceOptions>,
    pub clock: Option<Clock>,
    pub uid_generator: Option<UidGenerator>,
}
#[derive(Clone)]
pub struct Service {
    backend: Arc<dyn Backend>,
    limits: Limits,
    clock: Clock,
    uids: UidGenerator,
    physical: Option<PhysicalServiceOptions>,
    native: Option<NativeServiceOptions>,
}
impl Service {
    pub fn new(backend: Option<Arc<dyn Backend>>, options: ServiceOptions) -> Result<Service> {
        let Some(backend) = backend else {
            return Err(Kind::InvalidAttributes.into());
        };
        let native = normalize_native_options(backend.as_ref(), options.native.as_ref())?;
        let mut physical = None;
        if let Some(p) = &options.physical {
            if backend.honors_physical_image_box_plans() != Some(true)
                || p.area.is_none()
                || p.capabilities.validate().is_err()
            {
                return Err(Kind::PhysicalUnsupported.into());
            }
            if !matches!(p.default_behavior.as_str(), "" | DECIMATE | CROP | FAIL) {
                return Err(Kind::InvalidAttributes.into());
            }
            physical = Some(p.clone());
        }
        let mut l = options.limits;
        if l.max_film_boxes < 0
            || l.max_image_boxes < 0
            || l.max_dimension < 0
            || l.max_image_bytes < 0
            || l.max_session_bytes < 0
            || l.max_copies < 0
            || l.print_timeout.0 < 0
            || l.max_image_boxes > MAX_IMAGE_BOXES_CEILING
        {
            return Err(Kind::InvalidAttributes.into());
        }
        if l.max_film_boxes == 0 {
            l.max_film_boxes = 16;
        }
        if l.max_image_boxes == 0 {
            l.max_image_boxes = 64;
        }
        if l.max_dimension == 0 {
            l.max_dimension = 8192;
        }
        if l.max_image_bytes == 0 {
            l.max_image_bytes = 64 << 20;
        }
        if l.max_session_bytes == 0 {
            l.max_session_bytes = 512 << 20;
        }
        if l.max_copies == 0 {
            l.max_copies = 10;
        }
        if l.print_timeout.0 == 0 {
            l.print_timeout = Nanos::from_secs(30);
        }
        Ok(Service {
            backend,
            limits: l,
            clock: options
                .clock
                .unwrap_or_else(|| Arc::new(dimse::StorageCommitmentTime::now)),
            uids: options
                .uid_generator
                .unwrap_or_else(|| Arc::new(|| Ok(crate::deid::uid::new_uid()))),
            physical,
            native,
        })
    }
    pub fn supported_abstract_syntaxes(&self) -> Vec<String> {
        let mut out = vec![
            GRAYSCALE_META_SOP_CLASS_UID.to_string(),
            COLOR_META_SOP_CLASS_UID.to_string(),
            PRINTER_SOP_CLASS_UID.to_string(),
        ];
        if let Some(n) = &self.native {
            if n.presentation_lut {
                out.push(PRESENTATION_LUT_SOP_CLASS_UID.into());
            }
            if !n.annotation_formats.is_empty() {
                out.push(ANNOTATION_BOX_SOP_CLASS_UID.into());
            }
        }
        out
    }
    pub fn bind(&self, assoc: Option<&Association>) -> Binding {
        let mut native_contexts = HashSet::new();
        let mut calling = String::new();
        if let Some(a) = assoc {
            calling = a.calling_ae_title.trim().to_string();
            for pc in &a.accepted_contexts {
                if pc.abstract_syntax_uid == PRESENTATION_LUT_SOP_CLASS_UID
                    || pc.abstract_syntax_uid == ANNOTATION_BOX_SOP_CLASS_UID
                {
                    native_contexts.insert(pc.abstract_syntax_uid.clone());
                }
            }
        }
        Binding {
            inner: Arc::new(Mutex::new(BindingState {
                service: self.clone(),
                calling,
                session: None,
                closed: false,
                native_contexts,
                luts: None,
            })),
        }
    }
    fn new_uid(&self) -> Result<String> {
        (self.uids)().map_err(|e| Error::Other(e.to_string()))
    }
}
struct FilmSession {
    uid: String,
    presentation: FilmSessionPresentation,
    boxes: Vec<FilmBox>,
}
struct BindingState {
    service: Service,
    calling: String,
    session: Option<FilmSession>,
    closed: bool,
    native_contexts: HashSet<String>,
    luts: Option<HashMap<String, PresentationLutInstance>>,
}
#[derive(Clone)]
pub struct Binding {
    inner: Arc<Mutex<BindingState>>,
}
fn meta_context(ctx: &Ctx) -> String {
    dimse::normalized_request_info_from_context(Some(ctx))
        .map(|i| i.presentation_context.abstract_syntax_uid)
        .unwrap_or_default()
}
fn image_box_class(meta: &str) -> &'static str {
    if meta == COLOR_META_SOP_CLASS_UID {
        COLOR_IMAGE_BOX_UID
    } else {
        GRAYSCALE_IMAGE_BOX_UID
    }
}
fn allowed_command(abstract_syntax: &str, command: &str) -> bool {
    match abstract_syntax {
        GRAYSCALE_META_SOP_CLASS_UID => matches!(
            command,
            FILM_SESSION_SOP_CLASS_UID
                | FILM_BOX_SOP_CLASS_UID
                | GRAYSCALE_IMAGE_BOX_UID
                | PRINTER_SOP_CLASS_UID
        ),
        COLOR_META_SOP_CLASS_UID => matches!(
            command,
            FILM_SESSION_SOP_CLASS_UID
                | FILM_BOX_SOP_CLASS_UID
                | COLOR_IMAGE_BOX_UID
                | PRINTER_SOP_CLASS_UID
        ),
        PRINTER_SOP_CLASS_UID => command == PRINTER_SOP_CLASS_UID,
        PRESENTATION_LUT_SOP_CLASS_UID | ANNOTATION_BOX_SOP_CLASS_UID => command == abstract_syntax,
        _ => false,
    }
}
fn state_error() -> Error {
    Kind::State.into()
}
pub(crate) fn status_of(err: Option<&Error>) -> u16 {
    match err {
        None => STATUS_SUCCESS,
        Some(e) => match e.status_error() {
            Some(s) => s.status,
            None if e.is(Kind::UnknownBox) => STATUS_NO_SUCH_SOP_INSTANCE,
            None => STATUS_PROCESSING_FAILURE,
        },
    }
}
fn fields_of(err: Option<&Error>) -> Option<NormalizedStatusFields> {
    let s = err?.status_error()?;
    if s.error_comment.is_empty() && s.tags.is_empty() {
        return None;
    }
    Some(NormalizedStatusFields {
        error_comment: s.error_comment.clone(),
        attribute_identifier_list: s.tags.clone(),
        ..Default::default()
    })
}
fn warning(status: u16, tags: Vec<Tag>) -> (u16, Option<NormalizedStatusFields>) {
    (
        status,
        Some(NormalizedStatusFields {
            attribute_identifier_list: tags,
            ..Default::default()
        }),
    )
}
type HandlerOut = (NormalizedScpResult, Option<dimse::Error>);
fn to_dimse(e: Error) -> dimse::Error {
    match e {
        Error::Dimse(d) => d,
        other => dimse::Error::new(other.to_string()),
    }
}
fn error_reply(err: Error, type_id: Option<u16>) -> HandlerOut {
    (
        NormalizedScpResult {
            response: NormalizedResponse {
                status: status_of(Some(&err)),
                status_fields: fields_of(Some(&err)),
                type_id,
                ..Default::default()
            },
            data_set: None,
        },
        Some(to_dimse(err)),
    )
}
fn ok_reply(
    status: u16,
    fields: Option<NormalizedStatusFields>,
    uid: String,
    data_set: Option<Object>,
) -> HandlerOut {
    (
        NormalizedScpResult {
            response: NormalizedResponse {
                status,
                status_fields: fields,
                affected_sop_instance_uid: uid,
                ..Default::default()
            },
            data_set,
        },
        None,
    )
}
impl Binding {
    pub fn close(&self) {
        let mut b = self.lock();
        b.closed = true;
        b.session = None;
        b.luts = None;
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, BindingState> {
        self.inner.lock().unwrap()
    }
    pub fn normalized_options(&self) -> NormalizedScpOptions {
        let max = self.lock().service.limits.max_image_bytes + (1 << 20);
        let (c, s, a, d, g) = (
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
            self.clone(),
        );
        NormalizedScpOptions {
            max_data_set_bytes: max,
            presentation_context_policy: Some(Arc::new(|pc: &AcceptedContext, command: &str| {
                if !allowed_command(&pc.abstract_syntax_uid, command) {
                    return Err(dimse::normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        command,
                    ));
                }
                Ok(())
            })),
            create_handler: Some(Arc::new(move |ctx, req, ds| c.create(ctx, req, ds))),
            set_handler: Some(Arc::new(move |ctx, req, ds| s.set(ctx, req, ds))),
            action_handler: Some(Arc::new(move |ctx, req, _| a.action(ctx, req))),
            delete_handler: Some(Arc::new(move |_, req, _| d.delete(req))),
            get_handler: Some(Arc::new(move |ctx, req, _| g.get(ctx, req))),
            ..Default::default()
        }
    }
    pub fn association_scp_options(&self) -> AssociationScpOptions {
        AssociationScpOptions {
            normalized_scp: Some(self.normalized_options()),
            ..Default::default()
        }
    }
    fn create(&self, ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>) -> HandlerOut {
        let mut b = self.lock();
        let r = match req.sop_class_uid.as_str() {
            FILM_SESSION_SOP_CLASS_UID => b.create_session(req, ds),
            FILM_BOX_SOP_CLASS_UID => b.create_film_box(ctx, req, ds),
            PRESENTATION_LUT_SOP_CLASS_UID => b.create_presentation_lut(req, ds),
            _ => Err(failure(
                dimse::STATUS_UNRECOGNIZED_OPERATION,
                "",
                state_error(),
                &[],
            )),
        };
        match r {
            Ok(out) => out,
            Err(e) => error_reply(e, None),
        }
    }
    fn set(&self, ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>) -> HandlerOut {
        let mut b = self.lock();
        match b.set_locked(ctx, req, ds) {
            Ok((status, fields)) => ok_reply(status, fields, String::new(), None),
            Err(e) => error_reply(e, None),
        }
    }
    fn action(&self, ctx: &Ctx, req: &NormalizedRequest) -> HandlerOut {
        let mut b = self.lock();
        let id = req.type_id;
        match b.action_locked(ctx, req) {
            Ok(status) => {
                let (mut out, e) = ok_reply(status, None, String::new(), None);
                out.response.type_id = Some(id);
                (out, e)
            }
            Err(e) => error_reply(e, Some(id)),
        }
    }
    fn delete(&self, req: &NormalizedRequest) -> HandlerOut {
        let mut b = self.lock();
        match b.delete_locked(req) {
            Ok(()) => ok_reply(STATUS_SUCCESS, None, String::new(), None),
            Err(e) => error_reply(e, None),
        }
    }
    fn get(&self, ctx: &Ctx, req: &NormalizedRequest) -> HandlerOut {
        let service = self.lock().service.clone();
        if req.sop_class_uid != PRINTER_SOP_CLASS_UID
            || req.sop_instance_uid != PRINTER_SOP_INSTANCE_UID
        {
            let err = if req.sop_class_uid != PRINTER_SOP_CLASS_UID {
                failure(dimse::STATUS_UNRECOGNIZED_OPERATION, "", state_error(), &[])
            } else {
                Kind::UnknownBox.into()
            };
            let status = status_of(Some(&err));
            return (
                NormalizedScpResult {
                    response: NormalizedResponse {
                        status,
                        ..Default::default()
                    },
                    data_set: None,
                },
                Some(to_dimse(err)),
            );
        }
        let status = match service.backend.printer_status(ctx) {
            Ok(s) => s,
            Err(e) => {
                return (
                    NormalizedScpResult {
                        response: NormalizedResponse {
                            status: STATUS_PROCESSING_FAILURE,
                            ..Default::default()
                        },
                        data_set: None,
                    },
                    Some(to_dimse(e)),
                );
            }
        };
        let all = [
            (
                TAG_MANUFACTURER,
                text(TAG_MANUFACTURER, Vr::LO, &status.manufacturer),
            ),
            (
                TAG_MANUFACTURER_MODEL_NAME,
                text(
                    TAG_MANUFACTURER_MODEL_NAME,
                    Vr::LO,
                    &status.manufacturer_model_name,
                ),
            ),
            (
                TAG_PRINTER_STATUS,
                text(TAG_PRINTER_STATUS, Vr::CS, &status.status),
            ),
            (
                TAG_PRINTER_STATUS_INFO,
                text(TAG_PRINTER_STATUS_INFO, Vr::CS, &status.info),
            ),
            (
                TAG_PRINTER_NAME,
                text(TAG_PRINTER_NAME, Vr::LO, &status.name),
            ),
        ];
        let mut elements = Vec::new();
        let mut missing = false;
        if req.attribute_identifier_list.is_empty() {
            elements = all.iter().map(|(_, e)| e.clone()).collect();
        } else {
            for tag in &req.attribute_identifier_list {
                match all.iter().find(|(t, _)| t == tag) {
                    Some((_, e)) => elements.push(e.clone()),
                    None => missing = true,
                }
            }
        }
        let code = if missing {
            STATUS_ATTRIBUTE_LIST_ERROR
        } else {
            STATUS_SUCCESS
        };
        ok_reply(
            code,
            None,
            String::new(),
            Some(Object::from_elements(elements)),
        )
    }
}
impl BindingState {
    fn new_uid(&self, requested: &str) -> Result<String> {
        let uid = requested.trim();
        if !uid.is_empty() {
            if !is_valid_uid(uid.as_bytes()) {
                return Err(failure(0x0117, "", Kind::InvalidAttributes.into(), &[]));
            }
            return Ok(uid.to_string());
        }
        self.service.new_uid()
    }
    fn check_native_uid(&self, uid: &str) -> Result<()> {
        if self.service.native.is_some() {
            if !is_valid_uid(uid.as_bytes()) {
                return Err(failure(0x0117, "", Kind::InvalidAttributes.into(), &[]));
            }
            if self.native_uid_used(uid) {
                return Err(failure(0x0111, "", state_error(), &[]));
            }
        }
        Ok(())
    }
    fn create_session(
        &mut self,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<HandlerOut> {
        if self.closed || self.session.is_some() {
            return Err(failure(
                STATUS_PROCESSING_FAILURE,
                "Only one Film Session per association",
                state_error(),
                &[],
            ));
        }
        let defaults = FilmSessionPresentation {
            number_of_copies: 1,
            print_priority: "MED".into(),
            medium_type: "PAPER".into(),
            film_destination: "MAGAZINE".into(),
            ..Default::default()
        };
        let (mut presentation, memory, ignored) =
            parse_film_session(ds, &defaults, self.service.limits.max_copies)?;
        presentation.specific_character_set = capture_native_charset(ds, "");
        let uid = self.new_uid(&req.sop_instance_uid)?;
        self.check_native_uid(&uid)?;
        self.session = Some(FilmSession {
            uid: uid.clone(),
            presentation,
            boxes: Vec::new(),
        });
        let (status, fields) = if memory {
            warning(
                STATUS_MEMORY_ALLOCATION_NOT_SUPPORT,
                vec![TAG_MEMORY_ALLOCATION],
            )
        } else if !ignored.is_empty() {
            warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored)
        } else {
            (STATUS_SUCCESS, None)
        };
        Ok(ok_reply(status, fields, uid, None))
    }
    fn create_film_box(
        &mut self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<HandlerOut> {
        let meta = meta_context(ctx);
        if meta != GRAYSCALE_META_SOP_CLASS_UID && meta != COLOR_META_SOP_CLASS_UID {
            return Err(failure(
                dimse::STATUS_UNRECOGNIZED_OPERATION,
                "",
                state_error(),
                &[],
            ));
        }
        let Some(session) = &self.session else {
            return Err(failure(
                STATUS_PROCESSING_FAILURE,
                "No Film Session",
                state_error(),
                &[],
            ));
        };
        let limits = self.service.limits;
        if session.boxes.len() as i64 >= limits.max_film_boxes {
            return Err(failure(
                STATUS_RESOURCE_LIMITATION,
                "",
                Kind::InvalidAttributes.into(),
                &[],
            ));
        }
        let Some(attributes) = ds else {
            return Err(failure(
                STATUS_MISSING_ATTRIBUTE,
                "",
                Kind::InvalidAttributes.into(),
                &[
                    TAG_IMAGE_DISPLAY_FORMAT,
                    TAG_REFERENCED_FILM_SESSION_SEQUENCE,
                ],
            ));
        };
        let session_uid = session.uid.clone();
        let mut r = AttributeReader::new(attributes);
        let (format, ok) = r.text(TAG_IMAGE_DISPLAY_FORMAT)?;
        if !ok {
            return Err(failure(
                STATUS_MISSING_ATTRIBUTE,
                "",
                Kind::InvalidAttributes.into(),
                &[TAG_IMAGE_DISPLAY_FORMAT],
            ));
        }
        let (columns, rows) = parse_display_format(&format, limits.max_image_boxes)?;
        r.mark(TAG_REFERENCED_FILM_SESSION_SEQUENCE);
        let Some(references) = attributes.get_sequence(TAG_REFERENCED_FILM_SESSION_SEQUENCE) else {
            return Err(failure(
                STATUS_MISSING_ATTRIBUTE,
                "",
                Kind::InvalidAttributes.into(),
                &[TAG_REFERENCED_FILM_SESSION_SEQUENCE],
            ));
        };
        if references.len() != 1 {
            return Err(invalid(&[TAG_REFERENCED_FILM_SESSION_SEQUENCE]));
        }
        let class = references[0]
            .get_string(TAG_REFERENCED_SOP_CLASS_UID)
            .unwrap_or_default();
        let instance = references[0]
            .get_string(TAG_REFERENCED_SOP_INSTANCE_UID)
            .unwrap_or_default();
        if class.trim_end_matches(['\0', ' ']) != FILM_SESSION_SOP_CLASS_UID
            || instance.trim_end_matches(['\0', ' ']) != session_uid
        {
            return Err(invalid(&[TAG_REFERENCED_FILM_SESSION_SEQUENCE]));
        }
        let color = meta == COLOR_META_SOP_CLASS_UID;
        let defaults = FilmBoxPresentation {
            film_orientation: "PORTRAIT".into(),
            film_size_id: "8INX10IN".into(),
            magnification_type: "REPLICATE".into(),
            trim: "NO".into(),
            ..Default::default()
        };
        let presentation =
            parse_film_box_presentation_native(&mut r, &defaults, color, self.supports_lut())?;
        let uid = self.new_uid(&req.sop_instance_uid)?;
        let mut film = FilmBox {
            sop_instance_uid: uid.clone(),
            color,
            columns,
            rows,
            presentation,
            ..Default::default()
        };
        self.check_native_uid(&uid)?;
        self.configure_native_film(&mut r, &mut film, true)?;
        let class = image_box_class(&meta);
        let mut items = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(uid.clone());
        for a in &film.annotation_boxes {
            seen.insert(a.sop_instance_uid.clone());
        }
        for position in 1..=columns * rows {
            let box_uid = self.service.new_uid()?;
            if self.service.native.is_some() {
                if !is_valid_uid(box_uid.as_bytes()) {
                    return Err(failure(0x0117, "", Kind::InvalidAttributes.into(), &[]));
                }
                if seen.contains(&box_uid) || self.native_uid_used(&box_uid) {
                    return Err(failure(0x0111, "", state_error(), &[]));
                }
                seen.insert(box_uid.clone());
            }
            film.image_boxes.push(ImageBox {
                sop_class_uid: class.into(),
                sop_instance_uid: box_uid.clone(),
                position,
                ..Default::default()
            });
            items.push(reference_item(class, &box_uid));
        }
        let ignored = r.ignored();
        let annotation_refs: Vec<DataSet> = film
            .annotation_boxes
            .iter()
            .map(|a| reference_item(ANNOTATION_BOX_SOP_CLASS_UID, &a.sop_instance_uid))
            .collect();
        self.session.as_mut().unwrap().boxes.push(film);
        let mut elements = attributes.elements().to_vec();
        elements.push(sequence_of(TAG_REFERENCED_IMAGE_BOX_SEQUENCE, items));
        let mut reply = Object::from_elements(elements);
        if self.supports_annotation() {
            reply.put(sequence_of(
                TAG_REFERENCED_ANNOTATION_BOX_SEQUENCE,
                annotation_refs,
            ));
        }
        let (status, fields) = if ignored.is_empty() {
            (STATUS_SUCCESS, None)
        } else {
            warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored)
        };
        Ok(ok_reply(status, fields, uid, Some(reply)))
    }
    fn last_box(&self, uid: &str) -> Result<usize> {
        let Some(session) = &self.session else {
            return Err(Kind::UnknownBox.into());
        };
        let Some(last) = session.boxes.last() else {
            return Err(Kind::UnknownBox.into());
        };
        if last.sop_instance_uid == uid {
            return Ok(session.boxes.len() - 1);
        }
        if session.boxes.iter().any(|b| b.sop_instance_uid == uid) {
            return Err(failure(
                STATUS_PROCESSING_FAILURE,
                "Only the last created Film Box can be addressed",
                state_error(),
                &[],
            ));
        }
        Err(Kind::UnknownBox.into())
    }
    fn set_locked(
        &mut self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<(u16, Option<NormalizedStatusFields>)> {
        if req.sop_class_uid == PRESENTATION_LUT_SOP_CLASS_UID {
            return Err(failure(
                dimse::STATUS_UNRECOGNIZED_OPERATION,
                "",
                state_error(),
                &[],
            ));
        }
        if self.session.is_none() {
            return Err(Kind::UnknownBox.into());
        }
        match req.sop_class_uid.as_str() {
            FILM_SESSION_SOP_CLASS_UID => {
                let session = self.session.as_ref().unwrap();
                if req.sop_instance_uid != session.uid {
                    return Err(Kind::UnknownBox.into());
                }
                let (mut presentation, memory, ignored) =
                    parse_film_session(ds, &session.presentation, self.service.limits.max_copies)?;
                presentation.specific_character_set =
                    capture_native_charset(ds, &session.presentation.specific_character_set);
                if presentation.specific_character_set
                    != session.presentation.specific_character_set
                    && session.boxes.iter().any(|b| !b.annotation_boxes.is_empty())
                {
                    return Err(invalid(&[TAG_SPECIFIC_CHARACTER_SET]));
                }
                self.session.as_mut().unwrap().presentation = presentation;
                if memory {
                    return Ok(warning(
                        STATUS_MEMORY_ALLOCATION_NOT_SUPPORT,
                        vec![TAG_MEMORY_ALLOCATION],
                    ));
                }
                if !ignored.is_empty() {
                    return Ok(warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored));
                }
                Ok((STATUS_SUCCESS, None))
            }
            FILM_BOX_SOP_CLASS_UID => {
                let index = self.last_box(&req.sop_instance_uid)?;
                let empty = Object::new();
                let modifications = ds.unwrap_or(&empty);
                let mut r = AttributeReader::new(modifications);
                let film = self.session.as_ref().unwrap().boxes[index].clone();
                let presentation = parse_film_box_presentation_native(
                    &mut r,
                    &film.presentation,
                    film.color,
                    self.supports_lut(),
                )?;
                let mut candidate = film.clone();
                candidate.presentation = presentation;
                self.configure_native_film(&mut r, &mut candidate, false)?;
                let presentation = candidate.presentation;
                let mut plans = None;
                if self.service.physical.is_some() {
                    let mut list = vec![PhysicalPlan::default(); film.image_boxes.len()];
                    for (i, image_box) in film.image_boxes.iter().enumerate() {
                        if let Some(e) = dimse::context_err(ctx) {
                            return Err(Error::Dimse(e));
                        }
                        if !image_box.presentation.specified() {
                            continue;
                        }
                        let (plan, _) = self.resolve_physical_plan(
                            ctx,
                            &presentation,
                            film.columns,
                            film.rows,
                            image_box.position,
                            image_box.image.as_ref(),
                            &image_box.presentation,
                        )?;
                        list[i] = plan;
                    }
                    plans = Some(list);
                }
                let target = &mut self.session.as_mut().unwrap().boxes[index];
                target.presentation = presentation;
                if let Some(plans) = plans {
                    for (i, plan) in plans.into_iter().enumerate() {
                        target.image_boxes[i].plan = plan;
                    }
                }
                let ignored = r.ignored();
                if !ignored.is_empty() {
                    return Ok(warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored));
                }
                Ok((STATUS_SUCCESS, None))
            }
            GRAYSCALE_IMAGE_BOX_UID | COLOR_IMAGE_BOX_UID => self.set_image_box(ctx, req, ds),
            ANNOTATION_BOX_SOP_CLASS_UID => self.set_annotation_box(req, ds),
            _ => Err(failure(
                dimse::STATUS_UNRECOGNIZED_OPERATION,
                "",
                state_error(),
                &[],
            )),
        }
    }
    fn set_image_box(
        &mut self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<(u16, Option<NormalizedStatusFields>)> {
        let session = self.session.as_ref().unwrap();
        let Some(last) = session.boxes.last() else {
            return Err(Kind::UnknownBox.into());
        };
        let Some(target_index) = last
            .image_boxes
            .iter()
            .rposition(|b| b.sop_instance_uid == req.sop_instance_uid)
        else {
            let earlier = &session.boxes[..session.boxes.len() - 1];
            if earlier.iter().any(|f| {
                f.image_boxes
                    .iter()
                    .any(|i| i.sop_instance_uid == req.sop_instance_uid)
            }) {
                return Err(failure(
                    STATUS_PROCESSING_FAILURE,
                    "Only Image Boxes of the last created Film Box can be set",
                    state_error(),
                    &[],
                ));
            }
            return Err(Kind::UnknownBox.into());
        };
        let target = last.image_boxes[target_index].clone();
        let last = last.clone();
        if req.sop_class_uid != target.sop_class_uid
            || image_box_class(&meta_context(ctx)) != target.sop_class_uid
        {
            return Err(failure(0x0119, "", state_error(), &[]));
        }
        let Some(modifications) = ds else {
            return Err(failure(
                STATUS_INVALID_ATTRIBUTE_VALUE,
                "",
                Kind::InvalidAttributes.into(),
                &[TAG_IMAGE_BOX_POSITION],
            ));
        };
        let mut r = AttributeReader::new(modifications);
        match r.number(TAG_IMAGE_BOX_POSITION) {
            Ok((position, true)) if position == target.position => {}
            _ => return Err(invalid(&[TAG_IMAGE_BOX_POSITION])),
        }
        let polarity = r.enumerated(TAG_POLARITY, &["NORMAL", "REVERSE"])?;
        let sequence_tag = if last.color {
            TAG_BASIC_COLOR_IMAGE_SEQUENCE
        } else {
            TAG_BASIC_GRAYSCALE_IMAGE_SEQUENCE
        };
        r.mark(sequence_tag);
        let reference = self.image_lut_reference(&mut r, &target.presentation_lut_uid)?;
        let presentation = {
            let mut fresh = AttributeReader::new(modifications);
            parse_image_presentation(&mut fresh, &target.presentation)?
        };
        if self.service.physical.is_some() {
            for t in [
                TAG_REQUESTED_IMAGE_SIZE,
                TAG_REQUESTED_DECIMATE_CROP,
                TAG_MAGNIFICATION_TYPE,
            ] {
                r.mark(t);
            }
        }
        let Some(element) = modifications.get(sequence_tag) else {
            return Err(failure(
                STATUS_INVALID_ATTRIBUTE_VALUE,
                "",
                Kind::InvalidAttributes.into(),
                &[sequence_tag],
            ));
        };
        let mut image = None;
        match &element.value {
            Value::Sequence(items) if items.len() <= 1 => {
                if items.len() == 1 {
                    let items = modifications.get_sequence(sequence_tag).unwrap_or_default();
                    let parsed = parse_image(
                        &items[0],
                        last.color,
                        self.service.limits.max_dimension,
                        self.service.limits.max_image_bytes,
                    )?;
                    if self.session_bytes() - image_bytes(target.image.as_ref())
                        + image_bytes(Some(&parsed))
                        > self.service.limits.max_session_bytes
                    {
                        return Err(failure(
                            STATUS_INSUFFICIENT_PRINTER_MEMORY,
                            "",
                            Kind::InvalidAttributes.into(),
                            &[sequence_tag],
                        ));
                    }
                    image = Some(parsed);
                }
            }
            _ => return Err(invalid(&[sequence_tag])),
        }
        let mut plan = PhysicalPlan::default();
        let mut physical_warning = 0;
        if presentation.specified() && self.service.physical.is_some() {
            (plan, physical_warning) = self.resolve_physical_plan(
                ctx,
                &last.presentation,
                last.columns,
                last.rows,
                target.position,
                image.as_ref(),
                &presentation,
            )?;
        }
        self.validate_lut_image(&last.presentation, &reference, image.as_ref())?;
        let session = self.session.as_mut().unwrap();
        let slot = &mut session.boxes.last_mut().unwrap().image_boxes[target_index];
        slot.image = image;
        slot.presentation_lut_uid = reference;
        slot.presentation = presentation;
        slot.plan = plan;
        if !polarity.is_empty() {
            slot.polarity = polarity;
        }
        let ignored = r.ignored();
        if !ignored.is_empty() {
            return Ok(warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored));
        }
        if physical_warning != 0 {
            return Ok((physical_warning, None));
        }
        Ok((STATUS_SUCCESS, None))
    }
    #[allow(clippy::too_many_arguments)]
    fn resolve_physical_plan(
        &self,
        ctx: &Ctx,
        film: &FilmBoxPresentation,
        columns: i64,
        rows: i64,
        position: i64,
        image: Option<&Image>,
        p: &ImageBoxPresentation,
    ) -> Result<(PhysicalPlan, u16)> {
        let profile = self.service.physical.as_ref().unwrap();
        if p.requested_width_mm != 0.0 && profile.capabilities.requested_size != Support::Supported
        {
            return Err(invalid(&[TAG_REQUESTED_IMAGE_SIZE]));
        }
        if !p.decimate_crop_behavior.is_empty()
            && profile.capabilities.behavior(&p.decimate_crop_behavior) != Support::Supported
        {
            return Err(invalid(&[TAG_REQUESTED_DECIMATE_CROP]));
        }
        let Some(image) = image else {
            return Ok((PhysicalPlan::default(), 0));
        };
        if let Some(e) = dimse::context_err(ctx) {
            return Err(Error::Dimse(e));
        }
        let area = profile.area.as_ref().unwrap();
        let area = area(ctx, film, columns, rows, position).map_err(|_| {
            failure(
                STATUS_PROCESSING_FAILURE,
                "",
                Kind::PhysicalGeometry.into(),
                &[TAG_REQUESTED_IMAGE_SIZE, TAG_REQUESTED_DECIMATE_CROP],
            )
        })?;
        let plan = plan_image_box(
            image.rows,
            image.columns,
            image.pixel_aspect_ratio,
            p,
            &area,
            &film.magnification_type,
            &profile.default_behavior,
        )?;
        if !plan.outcome.is_empty()
            && profile.capabilities.behavior(&plan.outcome) != Support::Supported
        {
            return Err(invalid(&[TAG_REQUESTED_DECIMATE_CROP]));
        }
        let status = match plan.outcome.as_str() {
            CROP => 0xB609,
            DECIMATE => 0xB60A,
            _ => 0,
        };
        Ok((plan, status))
    }
    fn session_bytes(&self) -> i64 {
        self.session.as_ref().map_or(0, |s| {
            s.boxes
                .iter()
                .flat_map(|b| b.image_boxes.iter())
                .map(|i| image_bytes(i.image.as_ref()))
                .sum()
        })
    }
    fn action_locked(&mut self, ctx: &Ctx, req: &NormalizedRequest) -> Result<u16> {
        if req.type_id != PRINT_ACTION_TYPE_ID {
            return Err(failure(
                dimse::STATUS_NO_SUCH_ACTION_TYPE,
                "",
                state_error(),
                &[],
            ));
        }
        let Some(session) = &self.session else {
            return Err(Kind::UnknownBox.into());
        };
        let (indices, whole): (Vec<usize>, bool) = match req.sop_class_uid.as_str() {
            FILM_SESSION_SOP_CLASS_UID => {
                if req.sop_instance_uid != session.uid {
                    return Err(Kind::UnknownBox.into());
                }
                if session.boxes.is_empty() {
                    return Err(failure(
                        STATUS_NO_FILM_BOX_IN_SESSION,
                        "",
                        state_error(),
                        &[],
                    ));
                }
                ((0..session.boxes.len()).collect(), true)
            }
            FILM_BOX_SOP_CLASS_UID => (vec![self.last_box(&req.sop_instance_uid)?], false),
            _ => {
                return Err(failure(
                    dimse::STATUS_UNRECOGNIZED_OPERATION,
                    "",
                    state_error(),
                    &[],
                ));
            }
        };
        let job_id = self.service.new_uid()?;
        let session = self.session.as_ref().unwrap();
        let mut job = Job {
            id: job_id,
            film_session_uid: session.uid.clone(),
            calling_ae_title: self.calling.clone(),
            session: session.presentation.clone(),
            whole_session: whole,
            copies: session.presentation.number_of_copies,
            requested_at: (self.service.clock)(),
            ..Default::default()
        };
        let boxes: Vec<FilmBox> = indices.iter().map(|i| session.boxes[*i].clone()).collect();
        for film in &boxes {
            for image in &film.image_boxes {
                self.validate_lut_image(
                    &film.presentation,
                    &image.presentation_lut_uid,
                    image.image.as_ref(),
                )?;
            }
        }
        job.presentation_luts = self.job_luts(&boxes);
        for film in boxes {
            for image in &film.image_boxes {
                if image.image.is_none() {
                    job.empty_image_boxes += 1;
                } else {
                    job.image_boxes_printed += 1;
                }
            }
            job.film_boxes.push(film);
        }
        let timeout = self.service.limits.print_timeout;
        let print_ctx = ctx.with_timeout(Duration::from_nanos(timeout.0.max(0) as u64));
        if let Err(e) = self.service.backend.print(&print_ctx, &job) {
            return Err(if e.is(Kind::QueueFull) && whole {
                failure(STATUS_SESSION_QUEUE_FULL, "", e, &[])
            } else if e.is(Kind::QueueFull) {
                failure(STATUS_FILM_BOX_QUEUE_FULL, "", e, &[])
            } else {
                failure(STATUS_PROCESSING_FAILURE, "Print backend failure", e, &[])
            });
        }
        let session = self.session.as_mut().unwrap();
        for i in indices {
            session.boxes[i].printed = true;
        }
        if job.image_boxes_printed == 0 {
            return Ok(if whole {
                STATUS_EMPTY_SESSION_PAGE
            } else {
                STATUS_EMPTY_FILM_BOX_PAGE
            });
        }
        Ok(STATUS_SUCCESS)
    }
    fn delete_locked(&mut self, req: &NormalizedRequest) -> Result<()> {
        if req.sop_class_uid == PRESENTATION_LUT_SOP_CLASS_UID {
            return self.delete_presentation_lut(&req.sop_instance_uid);
        }
        let Some(session) = &self.session else {
            return Err(Kind::UnknownBox.into());
        };
        match req.sop_class_uid.as_str() {
            FILM_SESSION_SOP_CLASS_UID => {
                if req.sop_instance_uid != session.uid {
                    return Err(Kind::UnknownBox.into());
                }
                self.session = None;
                Ok(())
            }
            FILM_BOX_SOP_CLASS_UID => {
                self.last_box(&req.sop_instance_uid)?;
                self.session.as_mut().unwrap().boxes.pop();
                Ok(())
            }
            _ => Err(failure(
                dimse::STATUS_UNRECOGNIZED_OPERATION,
                "",
                state_error(),
                &[],
            )),
        }
    }
    fn supports_lut(&self) -> bool {
        self.service
            .native
            .as_ref()
            .is_some_and(|n| n.presentation_lut)
            && self
                .native_contexts
                .contains(PRESENTATION_LUT_SOP_CLASS_UID)
    }
    fn supports_annotation(&self) -> bool {
        self.service
            .native
            .as_ref()
            .is_some_and(|n| !n.annotation_formats.is_empty())
            && self.native_contexts.contains(ANNOTATION_BOX_SOP_CLASS_UID)
    }
    fn native_uid_used(&self, uid: &str) -> bool {
        if self.luts.as_ref().is_some_and(|l| l.contains_key(uid)) {
            return true;
        }
        let Some(session) = &self.session else {
            return false;
        };
        if session.uid == uid {
            return true;
        }
        session.boxes.iter().any(|f| {
            f.sop_instance_uid == uid
                || f.image_boxes.iter().any(|i| i.sop_instance_uid == uid)
                || f.annotation_boxes.iter().any(|a| a.sop_instance_uid == uid)
        })
    }
    fn create_presentation_lut(
        &mut self,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<HandlerOut> {
        if self.closed {
            return Err(failure(STATUS_PROCESSING_FAILURE, "", state_error(), &[]));
        }
        if !self.supports_lut() {
            return Err(failure(0x0122, "", Kind::NativeUnsupported.into(), &[]));
        }
        let limits = self.service.native.as_ref().unwrap().limits;
        let count = self.luts.as_ref().map_or(0, |l| l.len()) as i64;
        if count >= limits.max_presentation_luts {
            return Err(failure(
                STATUS_RESOURCE_LIMITATION,
                "",
                Kind::InvalidAttributes.into(),
                &[],
            ));
        }
        let used: i64 = self.luts.as_ref().map_or(0, |l| {
            l.values().map(|p| 2 * p.lut.entries.len() as i64).sum()
        });
        let p = parse_presentation_lut_budget(ds, limits.max_lut_bytes - used)?;
        let uid = self.new_uid(&req.sop_instance_uid)?;
        if !is_valid_uid(uid.as_bytes()) {
            return Err(failure(0x0117, "", Kind::InvalidAttributes.into(), &[]));
        }
        if self.native_uid_used(&uid) {
            return Err(failure(0x0111, "", state_error(), &[]));
        }
        self.luts.get_or_insert_with(HashMap::new).insert(
            uid.clone(),
            PresentationLutInstance {
                sop_instance_uid: uid.clone(),
                lut: p.clone(),
            },
        );
        let reply = build_presentation_lut(&p).ok();
        let ds = ds.unwrap();
        let mut r = AttributeReader::new(ds);
        r.mark(TAG_PRESENTATION_LUT_SHAPE);
        r.mark(TAG_PRESENTATION_LUT_SEQUENCE);
        let mut ignored = r.ignored();
        if let Some(items) = ds.get_sequence(TAG_PRESENTATION_LUT_SEQUENCE)
            && items.len() == 1
        {
            let mut nested = AttributeReader::new(&items[0]);
            nested.mark(TAG_LUT_DESCRIPTOR);
            nested.mark(TAG_LUT_DATA);
            nested.mark(TAG_LUT_EXPLANATION);
            ignored.extend(nested.ignored());
        }
        let (status, fields) = if ignored.is_empty() {
            (STATUS_SUCCESS, None)
        } else {
            warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored)
        };
        Ok(ok_reply(status, fields, uid, reply))
    }
    fn delete_presentation_lut(&mut self, uid: &str) -> Result<()> {
        if !self.supports_lut() {
            return Err(failure(0x0122, "", Kind::NativeUnsupported.into(), &[]));
        }
        if !self.luts.as_ref().is_some_and(|l| l.contains_key(uid)) {
            return Err(Kind::UnknownBox.into());
        }
        if let Some(session) = &self.session {
            for f in &session.boxes {
                if f.presentation.presentation_lut_uid == uid
                    || f.image_boxes.iter().any(|i| i.presentation_lut_uid == uid)
                {
                    return Err(failure(
                        STATUS_PROCESSING_FAILURE,
                        "",
                        state_error(),
                        &[TAG_REFERENCED_PRESENTATION_LUT],
                    ));
                }
            }
        }
        if let Some(l) = self.luts.as_mut() {
            l.remove(uid);
        }
        Ok(())
    }
    fn image_lut_reference(&self, r: &mut AttributeReader<'_>, prior: &str) -> Result<String> {
        if r.has(TAG_REFERENCED_PRESENTATION_LUT) && !self.supports_lut() {
            return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT]));
        }
        read_lut_reference(r, prior)
    }
    fn validate_lut_image(
        &self,
        film: &FilmBoxPresentation,
        image_ref: &str,
        image: Option<&Image>,
    ) -> Result<()> {
        let reference = if image_ref.is_empty() {
            film.presentation_lut_uid.as_str()
        } else {
            image_ref
        };
        if reference.is_empty() {
            return Ok(());
        }
        if !self.supports_lut() {
            return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT]));
        }
        let Some(p) = self.luts.as_ref().and_then(|l| l.get(reference)) else {
            return Err(invalid(&[
                TAG_REFERENCED_PRESENTATION_LUT,
                TAG_REFERENCED_SOP_INSTANCE_UID,
            ]));
        };
        film.native_print_density()?;
        p.lut.validate_image(image)
    }
    fn configure_native_film(
        &self,
        r: &mut AttributeReader<'_>,
        film: &mut FilmBox,
        create: bool,
    ) -> Result<()> {
        let mut p = film.presentation.clone();
        if self.supports_lut() {
            for tag in [
                TAG_MIN_DENSITY,
                TAG_MAX_DENSITY,
                TAG_ILLUMINATION,
                TAG_REFLECTED_AMBIENT_LIGHT,
            ] {
                if r.has(tag) {
                    native_us_values(r.ds, tag, 1)?;
                }
            }
        }
        let session_charset = self
            .session
            .as_ref()
            .map(|s| s.presentation.specific_character_set.clone())
            .unwrap_or_default();
        if self.supports_annotation() && r.has(TAG_SPECIFIC_CHARACTER_SET) {
            match presentation_text(r.ds, TAG_SPECIFIC_CHARACTER_SET, Vr::CS) {
                Ok((cs, _)) if cs == session_charset => {}
                _ => return Err(invalid(&[TAG_SPECIFIC_CHARACTER_SET])),
            }
            r.mark(TAG_SPECIFIC_CHARACTER_SET);
        }
        p.presentation_lut_uid = self.image_lut_reference(r, &p.presentation_lut_uid)?;
        if !p.presentation_lut_uid.is_empty() {
            if film.color {
                return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT]));
            }
            self.validate_lut_image(&p, "", None)?;
        }
        for i in &film.image_boxes {
            self.validate_lut_image(&p, &i.presentation_lut_uid, i.image.as_ref())?;
        }
        if r.has(TAG_ANNOTATION_DISPLAY_FORMAT_ID) && self.supports_annotation() {
            if !create {
                return Err(invalid(&[TAG_ANNOTATION_DISPLAY_FORMAT_ID]));
            }
            let (value, present) =
                presentation_text(r.ds, TAG_ANNOTATION_DISPLAY_FORMAT_ID, Vr::CS)?;
            if present {
                p.annotation_display_format_id = value;
                r.mark(TAG_ANNOTATION_DISPLAY_FORMAT_ID);
            }
        }
        if create && !p.annotation_display_format_id.is_empty() {
            if !self.supports_annotation() {
                return Err(invalid(&[TAG_ANNOTATION_DISPLAY_FORMAT_ID]));
            }
            if !supported_annotation_charset(&session_charset) {
                return Err(invalid(&[TAG_SPECIFIC_CHARACTER_SET]));
            }
            let formats = &self.service.native.as_ref().unwrap().annotation_formats;
            let slots = formats
                .iter()
                .filter(|f| f.id == p.annotation_display_format_id)
                .map(|f| f.slots)
                .next_back()
                .unwrap_or(0);
            if slots == 0 {
                return Err(invalid(&[TAG_ANNOTATION_DISPLAY_FORMAT_ID]));
            }
            film.annotation_boxes = Vec::with_capacity(slots as usize);
            let mut seen = HashSet::new();
            seen.insert(film.sop_instance_uid.clone());
            for _ in 1..=slots {
                let uid = self.service.new_uid()?;
                if !is_valid_uid(uid.as_bytes()) {
                    return Err(failure(0x0117, "", Kind::InvalidAttributes.into(), &[]));
                }
                if seen.contains(&uid) || self.native_uid_used(&uid) {
                    return Err(failure(0x0111, "", state_error(), &[]));
                }
                seen.insert(uid.clone());
                film.annotation_boxes.push(AnnotationBox {
                    sop_instance_uid: uid,
                    ..Default::default()
                });
            }
        }
        film.presentation = p;
        Ok(())
    }
    fn set_annotation_box(
        &mut self,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> Result<(u16, Option<NormalizedStatusFields>)> {
        if !self.supports_annotation() {
            return Err(failure(0x0122, "", Kind::NativeUnsupported.into(), &[]));
        }
        let Some(ds) = ds else {
            return Err(Kind::UnknownBox.into());
        };
        let Some(session) = self.session.as_ref().filter(|s| !s.boxes.is_empty()) else {
            return Err(Kind::UnknownBox.into());
        };
        let last = session.boxes.last().unwrap();
        let Some(target) = last
            .annotation_boxes
            .iter()
            .rposition(|a| a.sop_instance_uid == req.sop_instance_uid)
        else {
            let earlier = &session.boxes[..session.boxes.len() - 1];
            if earlier.iter().any(|f| {
                f.annotation_boxes
                    .iter()
                    .any(|a| a.sop_instance_uid == req.sop_instance_uid)
            }) {
                return Err(failure(STATUS_PROCESSING_FAILURE, "", state_error(), &[]));
            }
            return Err(Kind::UnknownBox.into());
        };
        let position = native_us_values(ds, TAG_ANNOTATION_POSITION, 1)?[0] as i64;
        if position < 1 || position > last.annotation_boxes.len() as i64 {
            return Err(invalid(&[TAG_ANNOTATION_POSITION]));
        }
        let target_uid = last.annotation_boxes[target].sop_instance_uid.clone();
        if last
            .annotation_boxes
            .iter()
            .any(|o| o.sop_instance_uid != target_uid && o.position == position)
        {
            return Err(invalid(&[TAG_ANNOTATION_POSITION]));
        }
        let (value, present) = native_text(
            ds,
            TAG_ANNOTATION_TEXT,
            &session.presentation.specific_character_set,
        )?;
        let slot = &mut self
            .session
            .as_mut()
            .unwrap()
            .boxes
            .last_mut()
            .unwrap()
            .annotation_boxes[target];
        if present {
            slot.text = value;
        }
        slot.position = position;
        let mut r = AttributeReader::new(ds);
        r.mark(TAG_ANNOTATION_POSITION);
        r.mark(TAG_ANNOTATION_TEXT);
        let ignored = r.ignored();
        if !ignored.is_empty() {
            return Ok(warning(STATUS_ATTRIBUTE_LIST_ERROR, ignored));
        }
        Ok((STATUS_SUCCESS, None))
    }
    fn job_luts(&self, boxes: &[FilmBox]) -> Vec<PresentationLutInstance> {
        let mut refs = BTreeSet::new();
        for f in boxes {
            if !f.presentation.presentation_lut_uid.is_empty() {
                refs.insert(f.presentation.presentation_lut_uid.clone());
            }
            for i in &f.image_boxes {
                if !i.presentation_lut_uid.is_empty() {
                    refs.insert(i.presentation_lut_uid.clone());
                }
            }
        }
        refs.iter()
            .map(|uid| {
                self.luts
                    .as_ref()
                    .and_then(|l| l.get(uid))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }
}
fn image_bytes(image: Option<&Image>) -> i64 {
    image.map_or(0, |i| i.pixel_data.len() as i64)
}
pub(crate) fn capture_native_charset(ds: Option<&Object>, prior: &str) -> String {
    let Some(ds) = ds else {
        return prior.to_string();
    };
    match presentation_text(ds, TAG_SPECIFIC_CHARACTER_SET, Vr::CS) {
        Err(_) => "UNSUPPORTED".into(),
        Ok((value, true)) => value,
        Ok((_, false)) => prior.to_string(),
    }
}
pub(crate) fn native_uid(ds: &Object, tag: Tag) -> Result<String> {
    let Some(e) = ds.get(tag).filter(|e| e.vr() == Vr::UI) else {
        return Err(invalid(&[tag]));
    };
    let mut s = match &e.value {
        Value::Raw(raw) => {
            if raw.len() > 64 {
                return Err(invalid(&[tag]));
            }
            String::from_utf8_lossy(raw).into_owned()
        }
        Value::Strings(v) => {
            if v.len() != 1 || v[0].len() > 64 {
                return Err(invalid(&[tag]));
            }
            v[0].clone()
        }
        _ => return Err(invalid(&[tag])),
    };
    if s.ends_with('\0') {
        s.pop();
    }
    if !is_valid_uid(s.as_bytes()) {
        return Err(invalid(&[tag]));
    }
    Ok(s)
}
fn read_lut_reference(r: &mut AttributeReader<'_>, prior: &str) -> Result<String> {
    let Some(e) = r.ds.get(TAG_REFERENCED_PRESENTATION_LUT) else {
        return Ok(prior.to_string());
    };
    r.mark(TAG_REFERENCED_PRESENTATION_LUT);
    let count = match &e.value {
        Value::Sequence(items) if e.vr() == Vr::SQ && items.len() <= 1 => items.len(),
        _ => return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT])),
    };
    if count == 0 {
        return Ok(String::new());
    }
    let items =
        r.ds.get_sequence(TAG_REFERENCED_PRESENTATION_LUT)
            .unwrap_or_default();
    match native_uid(&items[0], TAG_REFERENCED_SOP_CLASS_UID) {
        Ok(c) if c == PRESENTATION_LUT_SOP_CLASS_UID => {}
        _ => {
            return Err(invalid(&[
                TAG_REFERENCED_PRESENTATION_LUT,
                TAG_REFERENCED_SOP_CLASS_UID,
            ]));
        }
    }
    native_uid(&items[0], TAG_REFERENCED_SOP_INSTANCE_UID).map_err(|_| {
        invalid(&[
            TAG_REFERENCED_PRESENTATION_LUT,
            TAG_REFERENCED_SOP_INSTANCE_UID,
        ])
    })
}
