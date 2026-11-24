use super::*;
use crate::net::dimse::{
    NormalizedClient, NormalizedEventReportOptions, NormalizedEventReportSubscription,
    NormalizedHandler, NormalizedOperationOptions, NormalizedRequest, NormalizedResponse,
    NormalizedResult, NormalizedScpResult, NormalizedStatusClass, NormalizedStatusFields,
};
use crate::net::ul::{AcceptedContext, Association, PresentationContext};
use crate::object::{is_valid_uid, transfer};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
pub const STEP_CREATE_FILM_SESSION: &str = "Film Session N-CREATE";
pub const STEP_CREATE_FILM_BOX: &str = "Film Box N-CREATE";
pub const STEP_SET_IMAGE_BOX: &str = "Image Box N-SET";
pub const STEP_PRINT_FILM_BOX: &str = "Film Box N-ACTION";
pub const STEP_DELETE_FILM_SESSION: &str = "Film Session N-DELETE";
pub const STEP_CREATE_PRESENTATION_LUT: &str = "Presentation LUT N-CREATE";
pub const STEP_DELETE_PRESENTATION_LUT: &str = "Presentation LUT N-DELETE";
pub const STEP_SET_ANNOTATION_BOX: &str = "Annotation Box N-SET";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
fn syntaxes() -> Vec<String> {
    vec![
        transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid.into(),
        transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid.into(),
    ]
}
pub fn presentation_contexts(color: bool) -> Vec<PresentationContext> {
    let meta = if color {
        COLOR_META_SOP_CLASS_UID
    } else {
        GRAYSCALE_META_SOP_CLASS_UID
    };
    vec![PresentationContext {
        abstract_syntax_uid: meta.into(),
        transfer_syntax_uids: syntaxes(),
        ..Default::default()
    }]
}
pub fn native_presentation_contexts(
    color: bool,
    lut: bool,
    annotations: bool,
) -> Vec<PresentationContext> {
    let mut out = presentation_contexts(color);
    for (enabled, uid) in [
        (lut, PRESENTATION_LUT_SOP_CLASS_UID),
        (annotations, ANNOTATION_BOX_SOP_CLASS_UID),
    ] {
        if enabled {
            out.push(PresentationContext {
                abstract_syntax_uid: uid.into(),
                transfer_syntax_uids: syntaxes(),
                ..Default::default()
            });
        }
    }
    out
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageBoxReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub position: i64,
}
#[derive(Clone, Default)]
pub struct PrinterEventReportOptions {
    pub handler: Option<NormalizedHandler>,
    pub max_events: i64,
    pub max_data_set_bytes: i64,
}
#[derive(Clone, Default)]
pub struct ClientOptions {
    pub printer_events: Option<PrinterEventReportOptions>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub code: u16,
    pub fields: Option<NormalizedStatusFields>,
}
pub struct Client<'a> {
    normalized: NormalizedClient<'a>,
    meta: String,
    events: Option<NormalizedEventReportOptions>,
    native_contexts: HashSet<String>,
}
fn status_of_result(r: &NormalizedResult) -> Status {
    match &r.response {
        Some(rsp) => Status {
            code: rsp.status,
            fields: rsp.status_fields.clone(),
        },
        None => Status::default(),
    }
}
fn is_warning(err: &Error) -> bool {
    err.normalized_status()
        .is_some_and(|s| dimse::classify_normalized_status(s) == NormalizedStatusClass::Warning)
}
impl<'a> Client<'a> {
    pub fn new(assoc: &'a Association, color: bool) -> Client<'a> {
        Client::with_options(assoc, color, ClientOptions::default())
    }
    pub fn with_options(assoc: &'a Association, color: bool, options: ClientOptions) -> Client<'a> {
        let meta = if color {
            COLOR_META_SOP_CLASS_UID
        } else {
            GRAYSCALE_META_SOP_CLASS_UID
        };
        let native_contexts = assoc
            .accepted_contexts
            .iter()
            .map(|pc| pc.abstract_syntax_uid.clone())
            .collect();
        let events = options.printer_events.map(|configured| {
            let meta_policy = meta.to_string();
            let handler = configured.handler.map(|h| {
                let wrapped: NormalizedHandler = Arc::new(
                    move |ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>| {
                        if req.sop_instance_uid != PRINTER_SOP_INSTANCE_UID {
                            return (
                                NormalizedScpResult {
                                    response: NormalizedResponse {
                                        status: dimse::STATUS_NO_SUCH_SOP_INSTANCE,
                                        ..Default::default()
                                    },
                                    data_set: None,
                                },
                                None,
                            );
                        }
                        h(ctx, req, ds)
                    },
                );
                wrapped
            });
            NormalizedEventReportOptions {
                handler,
                max_events: configured.max_events,
                max_data_set_bytes: configured.max_data_set_bytes,
                subscriptions: vec![NormalizedEventReportSubscription {
                    sop_class_uid: PRINTER_SOP_CLASS_UID.into(),
                    event_type_ids: vec![1, 2, 3],
                }],
                presentation_context_policy: Some(Arc::new(
                    move |pc: &AcceptedContext, class: &str| {
                        if class == PRINTER_SOP_CLASS_UID
                            && (pc.abstract_syntax_uid == meta_policy
                                || pc.abstract_syntax_uid == PRINTER_SOP_CLASS_UID)
                        {
                            return Ok(());
                        }
                        Err(dimse::normalized_presentation_context_error(
                            pc.id,
                            &pc.abstract_syntax_uid,
                            class,
                        ))
                    },
                )),
            }
        });
        Client {
            normalized: NormalizedClient::new(assoc),
            meta: meta.into(),
            events,
            native_contexts,
        }
    }
    fn options(&self, ctx: Option<&Ctx>) -> NormalizedOperationOptions {
        let mut o = NormalizedOperationOptions::default();
        o.operation.context = ctx.cloned();
        o.presentation_context_abstract_syntax_uid = self.meta.clone();
        o.event_reports = self.events.clone();
        o
    }
    fn is_color(&self) -> bool {
        self.meta == COLOR_META_SOP_CLASS_UID
    }
    pub fn create_film_session(
        &self,
        ctx: Option<&Ctx>,
        uid: &str,
        p: &FilmSessionPresentation,
    ) -> (String, Status, Result<()>) {
        let ds = build_film_session(p);
        let (r, e) = self.normalized.create(
            &self.options(ctx),
            NormalizedRequest {
                sop_class_uid: FILM_SESSION_SOP_CLASS_UID.into(),
                sop_instance_uid: uid.into(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                ..Default::default()
            },
            Some(&ds),
        );
        let mut uid = uid.to_string();
        if let Some(rsp) = &r.response
            && !rsp.affected_sop_instance_uid.is_empty()
        {
            uid = rsp.affected_sop_instance_uid.clone();
        }
        (uid, status_of_result(&r), e.map_err(Error::Dimse))
    }
    pub fn create_film_box(
        &self,
        ctx: Option<&Ctx>,
        film_session_uid: &str,
        columns: i64,
        rows: i64,
        p: &FilmBoxPresentation,
    ) -> (String, Vec<ImageBoxReference>, Status, Result<()>) {
        if !box_count_within(columns, rows, MAX_IMAGE_BOXES_CEILING) {
            return (
                String::new(),
                Vec::new(),
                Status::default(),
                Err(Error::Other(
                    "printmgmt: Film Box columns and rows must be positive and within MaxImageBoxesCeiling image boxes"
                        .into(),
                )),
            );
        }
        let (uid, refs, status, err) =
            self.create_film_box_references(ctx, film_session_uid, columns, rows, p, false);
        (uid, refs.image_boxes, status, err)
    }
    fn create_film_box_references(
        &self,
        ctx: Option<&Ctx>,
        session: &str,
        columns: i64,
        rows: i64,
        p: &FilmBoxPresentation,
        annotations: bool,
    ) -> (String, FilmBoxReferences, Status, Result<()>) {
        let ds = build_film_box(session, columns, rows, p);
        let (r, e) = self.normalized.create(
            &self.options(ctx),
            NormalizedRequest {
                sop_class_uid: FILM_BOX_SOP_CLASS_UID.into(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                ..Default::default()
            },
            Some(&ds),
        );
        let status = status_of_result(&r);
        let uid = r
            .response
            .as_ref()
            .map(|rsp| rsp.affected_sop_instance_uid.clone())
            .unwrap_or_default();
        let err = e.err().map(Error::Dimse);
        if let Some(e) = &err
            && !is_warning(e)
        {
            return (uid, FilmBoxReferences::default(), status, Err(e.clone()));
        }
        match film_box_references(
            r.data_set.as_ref(),
            columns * rows,
            self.is_color(),
            annotations,
            &[session, &uid, &p.presentation_lut_uid],
        ) {
            Err(pe) => (uid, FilmBoxReferences::default(), status, Err(pe)),
            Ok(refs) => (
                uid,
                refs,
                status,
                match err {
                    Some(e) => Err(e),
                    None => Ok(()),
                },
            ),
        }
    }
    pub fn set_image_box(
        &self,
        ctx: Option<&Ctx>,
        b: &ImageBoxReference,
        image: Option<&Image>,
        polarity: &str,
    ) -> (Status, Result<()>) {
        match build_image_box(b.position, image, polarity) {
            Err(e) => (Status::default(), Err(e)),
            Ok(ds) => self.set(ctx, &b.sop_class_uid, &b.sop_instance_uid, Some(&ds)),
        }
    }
    pub fn set_image_box_with_presentation(
        &self,
        ctx: Option<&Ctx>,
        b: &ImageBoxReference,
        image: Option<&Image>,
        polarity: &str,
        p: &ImageBoxPresentation,
    ) -> (Status, Result<()>) {
        match build_image_box_with_presentation(b.position, image, polarity, p) {
            Err(e) => (Status::default(), Err(e)),
            Ok(ds) => self.set(ctx, &b.sop_class_uid, &b.sop_instance_uid, Some(&ds)),
        }
    }
    pub fn set_film_box(
        &self,
        ctx: Option<&Ctx>,
        uid: &str,
        modifications: Option<&Object>,
    ) -> (Status, Result<()>) {
        self.set(ctx, FILM_BOX_SOP_CLASS_UID, uid, modifications)
    }
    fn set(
        &self,
        ctx: Option<&Ctx>,
        class: &str,
        uid: &str,
        modifications: Option<&Object>,
    ) -> (Status, Result<()>) {
        self.set_with(self.options(ctx), class, uid, modifications)
    }
    fn set_with(
        &self,
        options: NormalizedOperationOptions,
        class: &str,
        uid: &str,
        modifications: Option<&Object>,
    ) -> (Status, Result<()>) {
        let (r, e) = self.normalized.set(
            &options,
            NormalizedRequest {
                sop_class_uid: class.into(),
                sop_instance_uid: uid.into(),
                ..Default::default()
            },
            modifications,
        );
        (status_of_result(&r), e.map_err(Error::Dimse))
    }
    pub fn print_film_box(&self, ctx: Option<&Ctx>, uid: &str) -> (Status, Result<()>) {
        self.print(ctx, FILM_BOX_SOP_CLASS_UID, uid)
    }
    pub fn print_film_session(&self, ctx: Option<&Ctx>, uid: &str) -> (Status, Result<()>) {
        self.print(ctx, FILM_SESSION_SOP_CLASS_UID, uid)
    }
    fn print(&self, ctx: Option<&Ctx>, class: &str, uid: &str) -> (Status, Result<()>) {
        let (r, e) = self.normalized.action(
            &self.options(ctx),
            NormalizedRequest {
                sop_class_uid: class.into(),
                sop_instance_uid: uid.into(),
                type_id: PRINT_ACTION_TYPE_ID,
                command_data_set_type: dimse::NO_DATA_SET,
                ..Default::default()
            },
            None,
        );
        (status_of_result(&r), e.map_err(Error::Dimse))
    }
    pub fn delete_film_session(&self, ctx: Option<&Ctx>, uid: &str) -> (Status, Result<()>) {
        self.delete_with(self.options(ctx), FILM_SESSION_SOP_CLASS_UID, uid)
    }
    pub fn delete_film_box(&self, ctx: Option<&Ctx>, uid: &str) -> (Status, Result<()>) {
        self.delete_with(self.options(ctx), FILM_BOX_SOP_CLASS_UID, uid)
    }
    fn delete_with(
        &self,
        options: NormalizedOperationOptions,
        class: &str,
        uid: &str,
    ) -> (Status, Result<()>) {
        let (r, e) = self.normalized.delete(
            &options,
            NormalizedRequest {
                sop_class_uid: class.into(),
                sop_instance_uid: uid.into(),
                ..Default::default()
            },
        );
        (status_of_result(&r), e.map_err(Error::Dimse))
    }
    pub fn printer_status(&self, ctx: Option<&Ctx>) -> Result<PrinterStatus> {
        let (r, e) = self.normalized.get(
            &self.options(ctx),
            NormalizedRequest {
                sop_class_uid: PRINTER_SOP_CLASS_UID.into(),
                sop_instance_uid: PRINTER_SOP_INSTANCE_UID.into(),
                ..Default::default()
            },
        );
        e.map_err(Error::Dimse)?;
        let Some(ds) = r.data_set else {
            return Ok(PrinterStatus::default());
        };
        let get = |t: Tag| ds.get_string(t).unwrap_or_default();
        Ok(PrinterStatus {
            status: get(TAG_PRINTER_STATUS),
            info: get(TAG_PRINTER_STATUS_INFO),
            name: get(TAG_PRINTER_NAME),
            manufacturer: get(TAG_MANUFACTURER),
            manufacturer_model_name: get(TAG_MANUFACTURER_MODEL_NAME),
        })
    }
    fn native_options(&self, ctx: Option<&Ctx>, class: &str) -> Result<NormalizedOperationOptions> {
        if !self.native_contexts.contains(class) {
            return Err(Kind::NativeUnsupported.into());
        }
        let mut o = self.options(ctx);
        o.presentation_context_abstract_syntax_uid = class.into();
        Ok(o)
    }
    pub fn create_presentation_lut(
        &self,
        ctx: Option<&Ctx>,
        uid: &str,
        p: &PresentationLut,
    ) -> (String, Status, Result<()>) {
        let ds = match build_presentation_lut(p) {
            Ok(d) => d,
            Err(e) => return (String::new(), Status::default(), Err(e)),
        };
        let options = match self.native_options(ctx, PRESENTATION_LUT_SOP_CLASS_UID) {
            Ok(o) => o,
            Err(e) => return (String::new(), Status::default(), Err(e)),
        };
        let (r, e) = self.normalized.create(
            &options,
            NormalizedRequest {
                sop_class_uid: PRESENTATION_LUT_SOP_CLASS_UID.into(),
                sop_instance_uid: uid.into(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                ..Default::default()
            },
            Some(&ds),
        );
        let mut uid = uid.to_string();
        if let Some(rsp) = &r.response {
            uid = rsp.affected_sop_instance_uid.clone();
        }
        (uid, status_of_result(&r), e.map_err(Error::Dimse))
    }
    pub fn delete_presentation_lut(&self, ctx: Option<&Ctx>, uid: &str) -> (Status, Result<()>) {
        match self.native_options(ctx, PRESENTATION_LUT_SOP_CLASS_UID) {
            Err(e) => (Status::default(), Err(e)),
            Ok(o) => self.delete_with(o, PRESENTATION_LUT_SOP_CLASS_UID, uid),
        }
    }
    pub fn create_film_box_with_native(
        &self,
        ctx: Option<&Ctx>,
        session: &str,
        columns: i64,
        rows: i64,
        p: &FilmBoxPresentation,
    ) -> (String, FilmBoxReferences, Status, Result<()>) {
        if !box_count_within(columns, rows, MAX_IMAGE_BOXES_CEILING) {
            return (
                String::new(),
                FilmBoxReferences::default(),
                Status::default(),
                Err(Kind::InvalidFilm.into()),
            );
        }
        if (!p.presentation_lut_uid.is_empty()
            && !self
                .native_contexts
                .contains(PRESENTATION_LUT_SOP_CLASS_UID))
            || (!p.annotation_display_format_id.is_empty()
                && !self.native_contexts.contains(ANNOTATION_BOX_SOP_CLASS_UID))
        {
            return (
                String::new(),
                FilmBoxReferences::default(),
                Status::default(),
                Err(Kind::NativeUnsupported.into()),
            );
        }
        let annotations = self.native_contexts.contains(ANNOTATION_BOX_SOP_CLASS_UID);
        self.create_film_box_references(ctx, session, columns, rows, p, annotations)
    }
    pub fn set_annotation_box(
        &self,
        ctx: Option<&Ctx>,
        b: &AnnotationBoxReference,
        text: &str,
        charset: &str,
    ) -> (Status, Result<()>) {
        let ds = match build_annotation_box(b.position, text, charset) {
            Ok(d) => d,
            Err(e) => return (Status::default(), Err(e)),
        };
        match self.native_options(ctx, ANNOTATION_BOX_SOP_CLASS_UID) {
            Err(e) => (Status::default(), Err(e)),
            Ok(o) => self.set_with(
                o,
                ANNOTATION_BOX_SOP_CLASS_UID,
                &b.sop_instance_uid,
                Some(&ds),
            ),
        }
    }
    pub fn set_image_box_with_lut(
        &self,
        ctx: Option<&Ctx>,
        b: &ImageBoxReference,
        image: Option<&Image>,
        polarity: &str,
        p: &ImageBoxPresentation,
        lut_uid: &str,
    ) -> (Status, Result<()>) {
        if !lut_uid.is_empty()
            && !self
                .native_contexts
                .contains(PRESENTATION_LUT_SOP_CLASS_UID)
        {
            return (Status::default(), Err(Kind::NativeUnsupported.into()));
        }
        let mut ds = match build_image_box_with_presentation(b.position, image, polarity, p) {
            Ok(d) => d,
            Err(e) => return (Status::default(), Err(e)),
        };
        if !lut_uid.is_empty() {
            if !is_valid_uid(lut_uid.as_bytes()) {
                return (
                    Status::default(),
                    Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT])),
                );
            }
            ds.put(presentation_lut_reference(lut_uid));
        }
        self.set(ctx, &b.sop_class_uid, &b.sop_instance_uid, Some(&ds))
    }
    fn validate_native_film(&self, film: &Film) -> Result<()> {
        if !self.native_contexts.contains(&self.meta) {
            return Err(Kind::NativeUnsupported.into());
        }
        let n = &film.native;
        if let Some(lut) = &n.presentation_lut {
            if !self
                .native_contexts
                .contains(PRESENTATION_LUT_SOP_CLASS_UID)
                || self.is_color()
            {
                return Err(Kind::NativeUnsupported.into());
            }
            lut.validate()?;
            film.presentation.native_print_density()?;
            for image in &film.images {
                lut.validate_image(image.as_ref())?;
            }
        }
        if !n.annotation_display_format_id.is_empty() || !n.annotations.is_empty() {
            if !self.native_contexts.contains(ANNOTATION_BOX_SOP_CLASS_UID) {
                return Err(Kind::NativeUnsupported.into());
            }
            if n.annotation_display_format_id.is_empty()
                || !valid_native_cs(&n.annotation_display_format_id)
                || n.annotations.len() as i64 > MAX_NATIVE_ANNOTATION_BOXES
            {
                return Err(invalid(&[TAG_ANNOTATION_DISPLAY_FORMAT_ID]));
            }
            if !supported_annotation_charset(&film.session.specific_character_set) {
                return Err(invalid(&[TAG_SPECIFIC_CHARACTER_SET]));
            }
            let mut seen = HashSet::new();
            for a in &n.annotations {
                if !seen.insert(a.position) {
                    return Err(invalid(&[TAG_ANNOTATION_POSITION]));
                }
                build_annotation_box(a.position, &a.text, &film.session.specific_character_set)?;
            }
        }
        if !film.presentation.presentation_lut_uid.is_empty()
            || !film.presentation.annotation_display_format_id.is_empty()
        {
            return Err(Error::Wrapped(
                Kind::InvalidFilm,
                "native object references are owned by PrintFilm".into(),
            ));
        }
        Ok(())
    }
    fn print_native_film(&self, ctx: &Ctx, film: &Film) -> (FilmResult, Result<()>) {
        let mut result = FilmResult::default();
        if let Err(e) = self.validate_native_film(film) {
            return (
                result,
                Err(print_error(STEP_CREATE_FILM_BOX, 0, None, Some(e))),
            );
        }
        let mut film = film.clone();
        let mut lut_uid = String::new();
        let mut session_uid = String::new();
        let outcome =
            self.print_native_steps(ctx, &mut film, &mut result, &mut lut_uid, &mut session_uid);
        let cleanup = ctx.without_cancel().with_timeout(CLEANUP_TIMEOUT);
        if !session_uid.is_empty() {
            let (status, err) = self.delete_film_session(Some(&cleanup), &session_uid);
            if let Err(e) = result.check(STEP_DELETE_FILM_SESSION, 0, &status, err) {
                result.cleanup_err = join(result.cleanup_err.take(), e);
            }
        }
        if !lut_uid.is_empty() {
            let (status, err) = self.delete_presentation_lut(Some(&cleanup), &lut_uid);
            if let Err(e) = result.check(STEP_DELETE_PRESENTATION_LUT, 0, &status, err) {
                result.cleanup_err = join(result.cleanup_err.take(), e);
            }
        }
        (result, outcome)
    }
    fn print_native_steps(
        &self,
        ctx: &Ctx,
        film: &mut Film,
        result: &mut FilmResult,
        lut_uid: &mut String,
        session_uid: &mut String,
    ) -> Result<()> {
        let check =
            |result: &mut FilmResult, step: &str, pos: i64, status: &Status, err: Result<()>| {
                if native_ignored(status) {
                    return Err(print_error(
                        step,
                        pos,
                        Some(status.code),
                        Some(Kind::NativeUnsupported.into()),
                    ));
                }
                result.check(step, pos, status, err)
            };
        if let Some(lut) = film.native.presentation_lut.clone() {
            let (uid, status, err) = self.create_presentation_lut(Some(ctx), "", &lut);
            *lut_uid = uid.clone();
            check(result, STEP_CREATE_PRESENTATION_LUT, 0, &status, err)?;
            if !is_valid_uid(uid.as_bytes()) {
                return Err(print_error(
                    STEP_CREATE_PRESENTATION_LUT,
                    0,
                    None,
                    Some(invalid(&[TAG_REFERENCED_SOP_INSTANCE_UID])),
                ));
            }
            film.presentation.presentation_lut_uid = uid;
        }
        let (uid, status, err) = self.create_film_session(Some(ctx), "", &film.session);
        *session_uid = uid.clone();
        result.film_session_uid = uid.clone();
        check(result, STEP_CREATE_FILM_SESSION, 0, &status, err)?;
        if !is_valid_uid(uid.as_bytes()) {
            return Err(print_error(
                STEP_CREATE_FILM_SESSION,
                0,
                None,
                Some(invalid(&[TAG_REFERENCED_SOP_INSTANCE_UID])),
            ));
        }
        film.presentation.annotation_display_format_id =
            film.native.annotation_display_format_id.clone();
        let (box_uid, refs, status, err) = self.create_film_box_with_native(
            Some(ctx),
            &uid,
            film.columns,
            film.rows,
            &film.presentation,
        );
        check(result, STEP_CREATE_FILM_BOX, 0, &status, err)?;
        if !is_valid_uid(box_uid.as_bytes()) {
            return Err(print_error(
                STEP_CREATE_FILM_BOX,
                0,
                None,
                Some(invalid(&[TAG_REFERENCED_SOP_INSTANCE_UID])),
            ));
        }
        result.film_box_uid = box_uid.clone();
        result.image_boxes = refs.image_boxes.len() as i64;
        for a in &film.native.annotations {
            if a.position > refs.annotation_boxes.len() as i64 {
                return Err(print_error(
                    STEP_SET_ANNOTATION_BOX,
                    a.position,
                    None,
                    Some(invalid(&[TAG_ANNOTATION_POSITION])),
                ));
            }
            let (status, err) = self.set_annotation_box(
                Some(ctx),
                &refs.annotation_boxes[(a.position - 1) as usize],
                &a.text,
                &film.session.specific_character_set,
            );
            check(result, STEP_SET_ANNOTATION_BOX, a.position, &status, err)?;
        }
        for (i, image) in film.images.iter().enumerate() {
            let Some(image) = image else { continue };
            let p = film.image_presentations.get(i).cloned().unwrap_or_default();
            let (status, err) = self.set_image_box_with_presentation(
                Some(ctx),
                &refs.image_boxes[i],
                Some(image),
                &film.polarity,
                &p,
            );
            check(result, STEP_SET_IMAGE_BOX, i as i64 + 1, &status, err)?;
            result.images_set += 1;
        }
        let (status, err) = self.print_film_box(Some(ctx), &box_uid);
        if let Err(mut e) = result.check(STEP_PRINT_FILM_BOX, 0, &status, err) {
            if let Error::Print(p) = &mut e
                && !p.has_status
            {
                p.outcome_unknown = true;
            }
            return Err(e);
        }
        result.printed = true;
        Ok(())
    }
    pub fn print_film(&self, ctx: &Ctx, film: &Film) -> (FilmResult, Result<()>) {
        let mut result = FilmResult::default();
        if let Err(e) = self.validate_film(film) {
            return (result, Err(e));
        }
        if film.native.specified() {
            return self.print_native_film(ctx, film);
        }
        let (session_uid, status, err) = self.create_film_session(Some(ctx), "", &film.session);
        if let Err(e) = result.check(STEP_CREATE_FILM_SESSION, 0, &status, err) {
            return (result, Err(e));
        }
        if session_uid.is_empty() {
            return (
                result,
                Err(print_error(
                    STEP_CREATE_FILM_SESSION,
                    0,
                    None,
                    Some(Error::Other(
                        "the printer assigned no Film Session UID".into(),
                    )),
                )),
            );
        }
        result.film_session_uid = session_uid.clone();
        let fail = |result: FilmResult, e: Error| -> (FilmResult, Result<()>) {
            let cleanup = ctx.without_cancel().with_timeout(CLEANUP_TIMEOUT);
            let _ = self.delete_film_session(Some(&cleanup), &session_uid);
            (result, Err(e))
        };
        let (box_uid, boxes, status, err) = self.create_film_box(
            Some(ctx),
            &session_uid,
            film.columns,
            film.rows,
            &film.presentation,
        );
        if let Err(e) = result.check(STEP_CREATE_FILM_BOX, 0, &status, err) {
            return fail(result, e);
        }
        if box_uid.is_empty() {
            return fail(
                result,
                print_error(
                    STEP_CREATE_FILM_BOX,
                    0,
                    None,
                    Some(Error::Other("the printer assigned no Film Box UID".into())),
                ),
            );
        }
        result.film_box_uid = box_uid.clone();
        result.image_boxes = boxes.len() as i64;
        for (i, image) in film.images.iter().enumerate() {
            let Some(image) = image else { continue };
            let p = film.image_presentations.get(i).cloned().unwrap_or_default();
            let (status, err) = self.set_image_box_with_presentation(
                Some(ctx),
                &boxes[i],
                Some(image),
                &film.polarity,
                &p,
            );
            if film.require_image_presentation
                && p.specified()
                && matches!(status.code, 0x0001 | STATUS_ATTRIBUTE_LIST_ERROR | 0x0116)
            {
                return fail(
                    result,
                    print_error(
                        STEP_SET_IMAGE_BOX,
                        boxes[i].position,
                        Some(status.code),
                        Some(Kind::PhysicalUnsupported.into()),
                    ),
                );
            }
            if let Err(e) = result.check(STEP_SET_IMAGE_BOX, boxes[i].position, &status, err) {
                return fail(result, e);
            }
            result.images_set += 1;
        }
        let (status, err) = self.print_film_box(Some(ctx), &box_uid);
        if let Err(mut e) = result.check(STEP_PRINT_FILM_BOX, 0, &status, err) {
            if let Error::Print(p) = &mut e
                && !p.has_status
            {
                p.outcome_unknown = true;
            }
            return fail(result, e);
        }
        result.printed = true;
        let (status, err) = self.delete_film_session(Some(ctx), &session_uid);
        if let Err(e) = result.check(STEP_DELETE_FILM_SESSION, 0, &status, err) {
            result.cleanup_err = Some(e);
        }
        (result, Ok(()))
    }
    fn validate_film(&self, film: &Film) -> Result<()> {
        let invalid_film = |reason: String| {
            print_error(
                STEP_CREATE_FILM_BOX,
                0,
                None,
                Some(Error::Wrapped(Kind::InvalidFilm, reason)),
            )
        };
        if !box_count_within(film.columns, film.rows, MAX_IMAGE_BOXES_CEILING) {
            return Err(invalid_film(
                "columns and rows must be positive and within MaxImageBoxesCeiling image boxes"
                    .into(),
            ));
        }
        if film.images.len() as i64 > film.columns * film.rows {
            return Err(invalid_film(format!(
                "{} images do not fit {} image boxes",
                film.images.len(),
                film.columns * film.rows
            )));
        }
        if !film.polarity.is_empty() && film.polarity != "NORMAL" && film.polarity != "REVERSE" {
            return Err(invalid_film("polarity must be NORMAL or REVERSE".into()));
        }
        if film.image_presentations.len() > film.images.len() {
            return Err(invalid_film("more image presentations than images".into()));
        }
        for (i, p) in film.image_presentations.iter().enumerate() {
            if p.validate().is_err() {
                return Err(invalid_film("invalid image presentation".into()));
            }
            if film.images[i].is_none() && p.specified() {
                return Err(invalid_film("presentation for an empty image".into()));
            }
        }
        let color = self.is_color();
        for (i, image) in film.images.iter().enumerate() {
            let Some(image) = image else { continue };
            if image.color != color {
                return Err(invalid_film(if color {
                    format!("image {} is grayscale on a color print session", i + 1)
                } else {
                    format!("image {} is color on a grayscale print session", i + 1)
                }));
            }
            if build_image_box(i as i64 + 1, Some(image), &film.polarity).is_err() {
                return Err(invalid_film(format!(
                    "image {} does not meet the preformatted image constraints",
                    i + 1
                )));
            }
        }
        Ok(())
    }
}
fn native_ignored(status: &Status) -> bool {
    matches!(status.code, 0x0001 | STATUS_ATTRIBUTE_LIST_ERROR | 0x0116)
}
fn join(prior: Option<Error>, e: Error) -> Option<Error> {
    match prior {
        None => Some(Error::Join(vec![e])),
        Some(Error::Join(mut list)) => {
            list.push(e);
            Some(Error::Join(list))
        }
        Some(other) => Some(Error::Join(vec![other, e])),
    }
}
fn print_error(step: &str, position: i64, status: Option<u16>, err: Option<Error>) -> Error {
    Error::Print(Box::new(PrintError {
        step: step.into(),
        position,
        status: status.unwrap_or(0),
        has_status: status.is_some(),
        outcome_unknown: false,
        err: err.map(Box::new),
    }))
}
fn film_box_references(
    ds: Option<&Object>,
    count: i64,
    color: bool,
    annotations: bool,
    assigned: &[&str],
) -> Result<FilmBoxReferences> {
    let mut out = FilmBoxReferences::default();
    let Some(ds) = ds else {
        return Err(Error::Other(
            "printmgmt: Film Box response omitted object references".into(),
        ));
    };
    let mut seen: HashMap<String, bool> = assigned
        .iter()
        .map(|u| (crate::object::text::normalize_uid(u).to_string(), true))
        .collect();
    let mut read = |tag: Tag, max: i64, class: &str| -> Result<Vec<String>> {
        let Some(e) = ds.get(tag) else {
            return Err(invalid(&[tag]));
        };
        match &e.value {
            Value::Sequence(items) if e.vr() == Vr::SQ && items.len() as i64 <= max => {}
            _ => return Err(invalid(&[tag])),
        }
        let items = ds.get_sequence(tag).unwrap_or_default();
        let mut uids = Vec::with_capacity(items.len());
        for item in &items {
            match native_uid(item, TAG_REFERENCED_SOP_CLASS_UID) {
                Ok(c) if c == class => {}
                _ => return Err(invalid(&[tag])),
            }
            let uid = match native_uid(item, TAG_REFERENCED_SOP_INSTANCE_UID) {
                Ok(u) if !seen.contains_key(&u) => u,
                _ => return Err(invalid(&[tag])),
            };
            seen.insert(uid.clone(), true);
            uids.push(uid);
        }
        Ok(uids)
    };
    let class = if color {
        COLOR_IMAGE_BOX_UID
    } else {
        GRAYSCALE_IMAGE_BOX_UID
    };
    match read(TAG_REFERENCED_IMAGE_BOX_SEQUENCE, count, class) {
        Ok(images) if images.len() as i64 == count => {
            for (i, uid) in images.into_iter().enumerate() {
                out.image_boxes.push(ImageBoxReference {
                    sop_class_uid: class.into(),
                    sop_instance_uid: uid,
                    position: i as i64 + 1,
                });
            }
        }
        _ => return Err(invalid(&[TAG_REFERENCED_IMAGE_BOX_SEQUENCE])),
    }
    if annotations {
        let uids = read(
            TAG_REFERENCED_ANNOTATION_BOX_SEQUENCE,
            MAX_NATIVE_ANNOTATION_BOXES,
            ANNOTATION_BOX_SOP_CLASS_UID,
        )?;
        for (i, uid) in uids.into_iter().enumerate() {
            out.annotation_boxes.push(AnnotationBoxReference {
                sop_instance_uid: uid,
                position: i as i64 + 1,
            });
        }
    }
    Ok(out)
}
pub fn image_from_gray(width: usize, height: usize, stride: usize, pix: &[u8]) -> Image {
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        data.extend_from_slice(&pix[y * stride..y * stride + width]);
    }
    Image {
        rows: height as i64,
        columns: width as i64,
        bits: 8,
        photometric: "MONOCHROME2".into(),
        pixel_data: data,
        ..Default::default()
    }
}
pub fn image_from_rgba(width: usize, height: usize, stride: usize, pix: &[u8]) -> Image {
    let n = width * height;
    let mut data = vec![0u8; 3 * n];
    let mut i = 0;
    for y in 0..height {
        let row = &pix[y * stride..];
        for x in 0..width {
            data[i] = row[4 * x];
            data[n + i] = row[4 * x + 1];
            data[2 * n + i] = row[4 * x + 2];
            i += 1;
        }
    }
    Image {
        color: true,
        rows: height as i64,
        columns: width as i64,
        bits: 8,
        photometric: "RGB".into(),
        pixel_data: data,
        ..Default::default()
    }
}
#[derive(Debug, Clone, Default)]
pub struct Film {
    pub session: FilmSessionPresentation,
    pub columns: i64,
    pub rows: i64,
    pub presentation: FilmBoxPresentation,
    pub polarity: String,
    pub images: Vec<Option<Image>>,
    pub image_presentations: Vec<ImageBoxPresentation>,
    pub require_image_presentation: bool,
    pub native: NativeFilmOptions,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepStatus {
    pub step: String,
    pub position: i64,
    pub status: u16,
}
impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let step = if self.position > 0 {
            format!("{} (position {})", self.step, self.position)
        } else {
            self.step.clone()
        };
        write!(
            f,
            "{step}: status {:04X}H ({})",
            self.status,
            status_text(self.status)
        )
    }
}
#[derive(Debug, Clone, Default)]
pub struct FilmResult {
    pub film_session_uid: String,
    pub film_box_uid: String,
    pub image_boxes: i64,
    pub images_set: i64,
    pub printed: bool,
    pub warnings: Vec<StepStatus>,
    pub cleanup_err: Option<Error>,
}
impl FilmResult {
    fn check(&mut self, step: &str, position: i64, status: &Status, err: Result<()>) -> Result<()> {
        let Err(err) = err else {
            return Ok(());
        };
        if let Some(s) = err.normalized_status() {
            if dimse::classify_normalized_status(s) == NormalizedStatusClass::Warning {
                self.warnings.push(StepStatus {
                    step: step.into(),
                    position,
                    status: s,
                });
                return Ok(());
            }
            return Err(print_error(step, position, Some(s), Some(err)));
        }
        if status.code != STATUS_SUCCESS {
            return Err(print_error(step, position, Some(status.code), Some(err)));
        }
        Err(print_error(step, position, None, Some(err)))
    }
}
pub fn status_text(status: u16) -> &'static str {
    match status {
        STATUS_SUCCESS => "success",
        0x0001 => "requested optional attributes are not supported",
        STATUS_ATTRIBUTE_LIST_ERROR => {
            "attribute list error: unsupported optional attributes were ignored"
        }
        0x0116 => "attribute value out of range",
        STATUS_MEMORY_ALLOCATION_NOT_SUPPORT => "memory allocation not supported",
        STATUS_EMPTY_SESSION_PAGE => "film session printed with no image boxes filled",
        STATUS_EMPTY_FILM_BOX_PAGE => "film box printed with no image boxes filled",
        0xB604 => "image size larger than the image box; image demagnified",
        0xB605 => "requested Min or Max Density outside the printer's range",
        0xB609 => "image size larger than the image box; image cropped",
        0xB60A => {
            "image size or combined print image size larger than the image box; image decimated"
        }
        STATUS_NO_FILM_BOX_IN_SESSION => "film session has no film box",
        STATUS_SESSION_QUEUE_FULL | STATUS_FILM_BOX_QUEUE_FULL => {
            "unable to create a print job: print queue is full"
        }
        0xC603 => "image size larger than the image box",
        0xC605 => "insufficient memory in the printer to store the image",
        0xC613 => "combined print image size larger than the image box",
        0xC616 => "more than one Film Box in the film session",
        STATUS_NO_SUCH_ATTRIBUTE => "no such attribute",
        STATUS_INVALID_ATTRIBUTE_VALUE => "invalid attribute value",
        STATUS_PROCESSING_FAILURE => "processing failure",
        STATUS_NO_SUCH_SOP_INSTANCE => "no such SOP instance",
        STATUS_MISSING_ATTRIBUTE => "missing attribute",
        STATUS_MISSING_ATTRIBUTE_VALUE => "missing attribute value",
        0x0111 => "duplicate SOP instance",
        0x0117 => "invalid object instance",
        0x0118 => "no such SOP class",
        0x0119 => "class-instance conflict",
        0x0122 => "SOP class not supported",
        0x0123 => "no such action",
        0x0124 => "not authorized",
        0x0210 => "duplicate invocation",
        0x0211 => "unrecognized operation",
        0x0212 => "mistyped argument",
        STATUS_RESOURCE_LIMITATION => "resource limitation",
        _ if dimse::classify_normalized_status(status) == NormalizedStatusClass::Warning => {
            "warning"
        }
        _ => "failure",
    }
}
