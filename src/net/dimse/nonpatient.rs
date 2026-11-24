#![forbid(unsafe_code)]
use super::error::{Error, Kind, Result};
use super::find_extras::{
    StreamingCFindHandler, StreamingCFindLimits, StreamingCFindRequest, StreamingCFindRoute,
    StreamingCFindYield,
};
use super::get::{CGetHandler, CGetSubOperation, new_c_get_scp_error};
use super::mwl::string_values;
use super::mwl_scp::preflight_modality_worklist_response;
use super::retrieve::{CMoveHandler, CMoveRequestContext, new_c_move_scp_error};
use super::retrieve_extras::{CMoveBatch, CMoveBatchHandler};
use crate::net::ul::{self, Ctx};
use crate::object::{self, DataSet, Element, ElementHeader, Object, Tag, Value, Vr};
use crate::qrmatch;
use std::collections::BTreeMap;
use std::sync::Arc;
const STATUS_UNABLE_TO_PROCESS: u16 = 0xC000;
const STATUS_PENDING: u16 = 0xFF00;
pub const HANGING_PROTOCOL_STORAGE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.38.1";
pub const HANGING_PROTOCOL_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.38.2";
pub const HANGING_PROTOCOL_MOVE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.38.3";
pub const HANGING_PROTOCOL_GET_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.38.4";
pub const COLOR_PALETTE_STORAGE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.39.1";
pub const COLOR_PALETTE_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.39.2";
pub const COLOR_PALETTE_MOVE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.39.3";
pub const COLOR_PALETTE_GET_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.39.4";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonPatientModel {
    Invalid,
    HangingProtocol,
    ColorPalette,
}
fn identifier_error() -> Error {
    Error::sentinel(Kind::NonPatientIdentifier)
}
fn provider_error() -> Error {
    Error::sentinel(Kind::NonPatientProvider)
}
fn np(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
#[derive(Clone)]
struct Key {
    vr: Vr,
    matching: bool,
    wildcard: bool,
    children: BTreeMap<Tag, Key>,
}
fn key(vr: Vr, matching: bool, wildcard: bool) -> Key {
    Key {
        vr,
        matching,
        wildcard,
        children: BTreeMap::new(),
    }
}
fn sq(children: BTreeMap<Tag, Key>, matching: bool) -> Key {
    Key {
        vr: Vr::SQ,
        matching,
        wildcard: false,
        children,
    }
}
impl NonPatientModel {
    pub fn sop_classes(self) -> Result<(&'static str, &'static str, &'static str, &'static str)> {
        match self {
            NonPatientModel::HangingProtocol => Ok((
                HANGING_PROTOCOL_STORAGE_SOP_CLASS_UID,
                HANGING_PROTOCOL_FIND_SOP_CLASS_UID,
                HANGING_PROTOCOL_MOVE_SOP_CLASS_UID,
                HANGING_PROTOCOL_GET_SOP_CLASS_UID,
            )),
            NonPatientModel::ColorPalette => Ok((
                COLOR_PALETTE_STORAGE_SOP_CLASS_UID,
                COLOR_PALETTE_FIND_SOP_CLASS_UID,
                COLOR_PALETTE_MOVE_SOP_CLASS_UID,
                COLOR_PALETTE_GET_SOP_CLASS_UID,
            )),
            NonPatientModel::Invalid => Err(identifier_error()),
        }
    }
    pub fn presentation_contexts(self) -> Result<Vec<ul::PresentationContext>> {
        let (storage, find, mv, get) = self.sop_classes()?;
        Ok([find, mv, get, storage]
            .iter()
            .map(|uid| {
                ul::PresentationContext::new(
                    uid,
                    &[ul::EXPLICIT_VR_LITTLE_ENDIAN, ul::IMPLICIT_VR_LITTLE_ENDIAN],
                )
            })
            .collect())
    }
    fn keys(self) -> BTreeMap<Tag, Key> {
        let code: BTreeMap<Tag, Key> = [
            (np(8, 0x100), key(Vr::SH, true, false)),
            (np(8, 0x102), key(Vr::SH, true, false)),
            (np(8, 0x103), key(Vr::SH, true, false)),
            (np(8, 0x104), key(Vr::LO, false, false)),
        ]
        .into_iter()
        .collect();
        let mut keys: BTreeMap<Tag, Key> = [
            (np(8, 5), key(Vr::CS, false, false)),
            (np(8, 0x16), key(Vr::UI, true, false)),
            (np(8, 0x18), key(Vr::UI, true, false)),
        ]
        .into_iter()
        .collect();
        if self == NonPatientModel::ColorPalette {
            keys.insert(np(0x70, 0x80), key(Vr::CS, true, true));
            keys.insert(np(0x70, 0x81), key(Vr::LO, false, false));
            keys.insert(np(0x70, 0x84), key(Vr::PN, false, false));
            return keys;
        }
        let selector: BTreeMap<Tag, Key> = [
            (np(8, 0x60), key(Vr::CS, true, false)),
            (np(0x20, 0x60), key(Vr::CS, true, false)),
            (np(8, 0x2218), sq(code.clone(), true)),
            (np(8, 0x1032), sq(code.clone(), true)),
            (np(0x40, 0x100a), sq(code.clone(), true)),
        ]
        .into_iter()
        .collect();
        let screen: BTreeMap<Tag, Key> = [
            (np(0x72, 0x104), key(Vr::US, false, false)),
            (np(0x72, 0x106), key(Vr::US, false, false)),
            (np(0x72, 0x108), key(Vr::FD, false, false)),
            (np(0x72, 0x10a), key(Vr::US, false, false)),
            (np(0x72, 0x10c), key(Vr::US, false, false)),
            (np(0x72, 0x10e), key(Vr::US, false, false)),
        ]
        .into_iter()
        .collect();
        for (tag, value) in [
            (np(0x72, 2), key(Vr::SH, true, true)),
            (np(0x72, 4), key(Vr::LO, false, false)),
            (np(0x72, 6), key(Vr::CS, true, false)),
            (np(0x72, 8), key(Vr::LO, false, false)),
            (np(0x72, 10), key(Vr::DT, false, false)),
            (np(0x72, 0x10), key(Vr::LO, true, true)),
            (np(0x72, 0x14), key(Vr::US, true, false)),
            (np(0x72, 0x100), key(Vr::US, true, false)),
            (np(0x72, 0x0e), sq(code.clone(), true)),
            (np(0x72, 0x0c), sq(selector, true)),
            (np(0x72, 0x102), sq(screen, false)),
        ] {
            keys.insert(tag, value);
        }
        keys
    }
    pub fn validate_find_identifier(self, identifier: Option<&Object>) -> Result<()> {
        self.sop_classes()?;
        let Some(identifier) = identifier else {
            return Err(identifier_error());
        };
        preflight_streaming(identifier, 1 << 20, 256, 8)?;
        let charset = identifier.get_string(np(8, 5)).unwrap_or_default();
        if !charset.is_empty() && charset != "ISO_IR 6" && charset != "ISO_IR 192" {
            return Err(identifier_error());
        }
        validate_keys(identifier, &self.keys(), charset == "ISO_IR 192", false)
    }
    pub fn retrieve_uids(self, identifier: Option<&Object>) -> Result<Vec<String>> {
        self.sop_classes()?;
        let Some(identifier) = identifier.filter(|i| i.elements().len() == 1) else {
            return Err(identifier_error());
        };
        let Some(e) = identifier.get(np(8, 0x18)).filter(|e| e.vr() == Vr::UI) else {
            return Err(identifier_error());
        };
        let values = string_values(e);
        if values.is_empty() || values.len() > 128 {
            return Err(identifier_error());
        }
        let mut seen: Vec<&str> = Vec::new();
        for v in &values {
            if !object::is_valid_uid(v.as_bytes()) || seen.contains(&v.as_str()) {
                return Err(identifier_error());
            }
            seen.push(v);
        }
        Ok(values)
    }
    #[doc(hidden)]
    pub fn match_candidate(
        self,
        query: Option<&Object>,
        candidate: Option<&Object>,
    ) -> Result<Option<Object>> {
        self.validate_find_identifier(query)?;
        let query = query.ok_or_else(identifier_error)?;
        let (storage, ..) = self.sop_classes()?;
        let Some(candidate) = candidate else {
            return Err(provider_error());
        };
        preflight_streaming(candidate, 1 << 20, 1024, 8)?;
        let class = candidate.get_string(np(8, 0x16)).unwrap_or_default();
        let instance = candidate.get_string(np(8, 0x18)).unwrap_or_default();
        if class != storage || !object::is_valid_uid(instance.as_bytes()) {
            return Err(provider_error());
        }
        let charset = candidate.get_string(np(8, 5)).unwrap_or_default();
        if !charset.is_empty() && charset != "ISO_IR 6" && charset != "ISO_IR 192" {
            return Err(provider_error());
        }
        if !qr_match(query, candidate)? {
            return Ok(None);
        }
        let mut projection = project(query, candidate)?;
        for tag in [np(8, 0x16), np(8, 0x18)] {
            if let Some(e) = candidate.get(tag) {
                projection.put(e.clone());
            }
        }
        Ok(Some(projection))
    }
    pub fn find_route(
        self,
        catalog: Option<NonPatientCatalog>,
        limits: StreamingCFindLimits,
    ) -> Result<StreamingCFindRoute> {
        let find = match self.sop_classes() {
            Ok((_, find, _, _)) if catalog.is_some() => find,
            _ => return Err(provider_error()),
        };
        let limits = limits.normalized()?;
        Ok(StreamingCFindRoute {
            sop_class_uid: find.to_string(),
            response_sop_class_uid: String::new(),
            handler: Some(Arc::new(NonPatientFind {
                model: self,
                catalog: catalog.unwrap(),
            })),
            limits,
        })
    }
}
pub type NonPatientCatalog =
    Arc<dyn Fn(&Ctx, &mut dyn FnMut(Option<&Object>) -> Result<()>) -> Result<()> + Send + Sync>;
struct NonPatientFind {
    model: NonPatientModel,
    catalog: NonPatientCatalog,
}
impl StreamingCFindHandler for NonPatientFind {
    fn find(
        &self,
        ctx: &Ctx,
        request: &StreamingCFindRequest,
        yield_: &mut StreamingCFindYield<'_>,
    ) -> Result<()> {
        if let Err(e) = self
            .model
            .validate_find_identifier(Some(&request.identifier))
        {
            if e.is(Kind::StreamingCFindResourceLimit) {
                return Err(e);
            }
            return Err(Error::wrap_err(Kind::StreamingCFindIdentifier, e));
        }
        let mut count = 0;
        let r = (self.catalog)(ctx, &mut |candidate: Option<&Object>| {
            if let Some(e) = super::controls::context_err(ctx) {
                return Err(e);
            }
            count += 1;
            if count > 10000 {
                return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
            }
            match self
                .model
                .match_candidate(Some(&request.identifier), candidate)?
            {
                None => Ok(()),
                Some(p) => yield_(STATUS_PENDING, Some(&p)),
            }
        });
        r.map_err(|e| Error::wrap_err(Kind::StreamingCFindProvider, e))
    }
}
fn preflight_streaming(
    value: &Object,
    max_bytes: i64,
    max_elements: i64,
    max_depth: i64,
) -> Result<()> {
    match preflight_modality_worklist_response(
        value,
        &object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        max_elements,
        max_bytes,
        max_depth,
    ) {
        Ok(()) => Ok(()),
        Err(e) if e.is(Kind::ModalityWorklistResourceLimit) => {
            Err(Error::sentinel(Kind::StreamingCFindResourceLimit))
        }
        Err(_) => Err(Error::sentinel(Kind::StreamingCFindProvider)),
    }
}
fn validate_keys(
    identifier: &Object,
    keys: &BTreeMap<Tag, Key>,
    unicode: bool,
    return_only: bool,
) -> Result<()> {
    for e in identifier.elements() {
        let Some(k) = keys.get(&e.tag()).filter(|k| k.vr == e.vr()) else {
            return Err(identifier_error());
        };
        if k.vr == Vr::SQ {
            let Value::Sequence(items) = &e.value else {
                return Err(identifier_error());
            };
            if items.len() > 1 {
                return Err(identifier_error());
            }
            for item in items {
                let nested = Object::from_elements(item.elements.clone());
                validate_keys(&nested, &k.children, unicode, return_only || !k.matching)?;
            }
            continue;
        }
        let values = values_of(identifier, e)?;
        if values.len() > 1 {
            return Err(identifier_error());
        }
        for value in values {
            if value.is_empty() {
                continue;
            }
            if value.len() > 1024
                || value.contains(['\\', '\0', '\r', '\n', '\t'])
                || (!unicode && !value.chars().all(|c| (' '..='~').contains(&c)))
            {
                return Err(identifier_error());
            }
            if e.tag() == np(8, 5) {
                continue;
            }
            if return_only || !k.matching || (!k.wildcard && value.contains(['*', '?'])) {
                return Err(identifier_error());
            }
            if k.vr == Vr::UI && !object::is_valid_uid(value.as_bytes()) {
                return Err(identifier_error());
            }
        }
    }
    Ok(())
}
fn values_of(owner: &Object, e: &Element) -> Result<Vec<String>> {
    if e.vr() == Vr::US {
        return match &e.value {
            Value::Raw(r) if r.is_empty() => Ok(Vec::new()),
            Value::Raw(r) if r.len() == 2 => {
                let v = if owner.value_big_endian() {
                    u16::from_be_bytes([r[0], r[1]])
                } else {
                    u16::from_le_bytes([r[0], r[1]])
                };
                Ok(vec![v.to_string()])
            }
            _ => Err(identifier_error()),
        };
    }
    if e.vr() == Vr::FD {
        return match &e.value {
            Value::Raw(r) if r.is_empty() => Ok(Vec::new()),
            _ => Err(identifier_error()),
        };
    }
    match &e.value {
        Value::Strings(_) | Value::Raw(_) => Ok(string_values(e)),
        _ => Err(identifier_error()),
    }
}
fn matching_elements(source: &Object) -> Vec<Element> {
    let mut out = Vec::new();
    for e in source.elements() {
        let mut e = e.clone();
        if e.vr() == Vr::US
            && let Ok(values) = values_of(source, &e)
        {
            e.value = Value::Strings(values);
            e.header.length_set = false;
            e.header.length = 0;
        }
        if let Value::Sequence(items) = &e.value {
            let items = items
                .iter()
                .map(|item| {
                    let mut nested = Object::from_elements(item.elements.clone());
                    nested.set_value_big_endian(source.value_big_endian());
                    DataSet {
                        elements: matching_elements(&nested),
                        item_offset: None,
                    }
                })
                .collect();
            e.value = Value::Sequence(items);
        }
        out.push(e);
    }
    out
}
fn qr_match(query: &Object, candidate: &Object) -> Result<bool> {
    let q = qrmatch::elements_from(&matching_elements(query));
    let c = qrmatch::elements_from(&matching_elements(candidate));
    qrmatch::match_identifier(Some(&q), Some(&c), &qrmatch::Options::default())
        .map(|r| r.matched)
        .map_err(|e| Error::new(e.to_string()))
}
fn project(query: &Object, candidate: &Object) -> Result<Object> {
    let mut result = Object::new();
    result.set_value_big_endian(candidate.value_big_endian());
    for requested in query.elements() {
        let mut e = candidate.get(requested.tag()).cloned().unwrap_or(Element {
            header: ElementHeader {
                tag: requested.tag(),
                vr: requested.vr(),
                length: 0,
                length_set: false,
            },
            value: if requested.vr() == Vr::SQ {
                Value::Sequence(Vec::new())
            } else {
                Value::Raw(Vec::new())
            },
        });
        if let Value::Sequence(q) = &requested.value
            && q.len() == 1
            && let Value::Sequence(c) = &e.value
        {
            let query_item = Object::from_elements(q[0].elements.clone());
            let mut items = Vec::new();
            for ci in c {
                let candidate_item = Object::from_elements(ci.elements.clone());
                if !qr_match(&query_item, &candidate_item)? {
                    continue;
                }
                let p = project(&query_item, &candidate_item)?;
                items.push(DataSet {
                    elements: p.elements().to_vec(),
                    item_offset: None,
                });
            }
            e.value = Value::Sequence(items);
        }
        result.put(e);
    }
    if let Some(cs) = candidate.get(np(8, 5)) {
        result.put(cs.clone());
    }
    Ok(result)
}
pub(crate) fn non_patient_retrieve_model(sop: &str) -> Option<NonPatientModel> {
    match sop {
        HANGING_PROTOCOL_MOVE_SOP_CLASS_UID | HANGING_PROTOCOL_GET_SOP_CLASS_UID => {
            Some(NonPatientModel::HangingProtocol)
        }
        COLOR_PALETTE_MOVE_SOP_CLASS_UID | COLOR_PALETTE_GET_SOP_CLASS_UID => {
            Some(NonPatientModel::ColorPalette)
        }
        _ => None,
    }
}
fn selected(
    storage: &str,
    uids: &[String],
    seen: &mut Vec<String>,
    class: &str,
    instance: &str,
) -> bool {
    if class != storage || seen.iter().any(|s| s == instance) {
        return false;
    }
    if uids.iter().any(|u| u == instance) {
        seen.push(instance.to_string());
        return true;
    }
    false
}
pub(crate) fn non_patient_c_get_handler(model: NonPatientModel, base: CGetHandler) -> CGetHandler {
    Arc::new(move |ctx, request| {
        let (storage, ..) = model.sop_classes()?;
        let uids = model
            .retrieve_uids(Some(&request.identifier))
            .map_err(|e| new_c_get_scp_error(0xA900, "Invalid non-patient Identifier", Some(e)))?;
        let ops = base(ctx, request)?;
        if ops.len() > uids.len() {
            return Err(new_c_get_scp_error(
                STATUS_UNABLE_TO_PROCESS,
                "Invalid catalog result",
                Some(provider_error()),
            ));
        }
        let mut seen = Vec::new();
        let mut out: Vec<CGetSubOperation> = Vec::with_capacity(ops.len());
        for op in ops {
            let Some(load) = op.load_data_set.clone().filter(|_| {
                selected(
                    storage,
                    &uids,
                    &mut seen,
                    &op.affected_sop_class_uid,
                    &op.affected_sop_instance_uid,
                )
            }) else {
                return Err(new_c_get_scp_error(
                    STATUS_UNABLE_TO_PROCESS,
                    "Invalid catalog identity",
                    Some(provider_error()),
                ));
            };
            let instance = op.affected_sop_instance_uid.clone();
            let mut op = op;
            op.load_data_set = Some(Arc::new(move |ctx: &Ctx| {
                let data = load(ctx)?;
                let class = data.get_string(np(8, 0x16)).unwrap_or_default();
                let uid = data.get_string(np(8, 0x18)).unwrap_or_default();
                if class != storage || uid != instance {
                    return Err(provider_error());
                }
                Ok(data)
            }));
            out.push(op);
        }
        Ok(out)
    })
}
pub(crate) fn non_patient_c_move_handler(
    model: NonPatientModel,
    base: CMoveHandler,
) -> CMoveHandler {
    Arc::new(move |ctx, request: &CMoveRequestContext| {
        let uids = model
            .retrieve_uids(Some(&request.identifier))
            .map_err(|e| new_c_move_scp_error(0xA900, "Invalid non-patient Identifier", Some(e)))?;
        let ops = base(ctx, request)?;
        let (storage, ..) = model.sop_classes()?;
        if ops.len() > uids.len() {
            return Err(new_c_move_scp_error(
                STATUS_UNABLE_TO_PROCESS,
                "Invalid catalog result",
                Some(provider_error()),
            ));
        }
        let mut seen = Vec::new();
        for op in &ops {
            if !selected(
                storage,
                &uids,
                &mut seen,
                &op.affected_sop_class_uid,
                &op.affected_sop_instance_uid,
            ) || op.store.is_none()
            {
                return Err(new_c_move_scp_error(
                    STATUS_UNABLE_TO_PROCESS,
                    "Invalid catalog identity",
                    Some(provider_error()),
                ));
            }
        }
        Ok(ops)
    })
}
pub(crate) fn non_patient_c_move_batch_handler(
    model: NonPatientModel,
    base: CMoveBatchHandler,
) -> CMoveBatchHandler {
    Arc::new(
        move |ctx, request: &CMoveRequestContext| -> Result<CMoveBatch> {
            let uids = model
                .retrieve_uids(Some(&request.identifier))
                .map_err(|e| {
                    new_c_move_scp_error(0xA900, "Invalid non-patient Identifier", Some(e))
                })?;
            let batch = base(ctx, request)?;
            let (storage, ..) = model.sop_classes()?;
            if batch.sub_operations.len() > uids.len() {
                return Err(new_c_move_scp_error(
                    STATUS_UNABLE_TO_PROCESS,
                    "Invalid catalog result",
                    Some(provider_error()),
                ));
            }
            let mut seen = Vec::new();
            for op in &batch.sub_operations {
                if !selected(
                    storage,
                    &uids,
                    &mut seen,
                    &op.affected_sop_class_uid,
                    &op.affected_sop_instance_uid,
                ) {
                    return Err(new_c_move_scp_error(
                        STATUS_UNABLE_TO_PROCESS,
                        "Invalid catalog identity",
                        Some(provider_error()),
                    ));
                }
            }
            Ok(batch)
        },
    )
}
