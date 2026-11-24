#![forbid(unsafe_code)]
use crate::net::dimse::{
    self, AssociationScpOptions, NormalizedClient, NormalizedOperationOptions, NormalizedRequest,
    NormalizedResponse, NormalizedScpOptions, NormalizedScpResult, NormalizedStatusFields,
};
use crate::net::ul::{AcceptedContext, Association, Ctx, PresentationContext};
use crate::object::{
    self, DataSet, Element, ElementHeader, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
pub const SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.33";
pub const MODALITY_PERFORMED_PROCEDURE_STEP_SOP_CLASS_UID: &str = "1.2.840.10008.3.1.2.3.3";
pub const AVAILABILITY_ONLINE: &str = "ONLINE";
pub const AVAILABILITY_NEARLINE: &str = "NEARLINE";
pub const AVAILABILITY_OFFLINE: &str = "OFFLINE";
pub const AVAILABILITY_UNAVAILABLE: &str = "UNAVAILABLE";
pub fn availabilities() -> Vec<&'static str> {
    vec![
        AVAILABILITY_ONLINE,
        AVAILABILITY_NEARLINE,
        AVAILABILITY_OFFLINE,
        AVAILABILITY_UNAVAILABLE,
    ]
}
pub fn valid_availability(value: &str) -> bool {
    availabilities().contains(&value)
}
pub const DIMSE_STATUS_SUCCESS: u16 = 0x0000;
pub const DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE: u16 = 0x0106;
pub const DIMSE_STATUS_PROCESSING_FAILURE: u16 = 0x0110;
pub const DIMSE_STATUS_DUPLICATE_SOP_INSTANCE: u16 = 0x0111;
pub const DIMSE_STATUS_INVALID_OBJECT_INSTANCE: u16 = 0x0117;
pub const DIMSE_STATUS_MISSING_ATTRIBUTE: u16 = 0x0120;
pub const DIMSE_STATUS_MISSING_ATTRIBUTE_VALUE: u16 = 0x0121;
pub const DIMSE_STATUS_RESOURCE_LIMITATION: u16 = 0x0213;
pub const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
pub const TAG_RETRIEVE_AE_TITLE: Tag = Tag::new(0x0008, 0x0054);
pub const TAG_INSTANCE_AVAILABILITY: Tag = Tag::new(0x0008, 0x0056);
pub const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
pub const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
pub const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
pub const TAG_REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE: Tag = Tag::new(0x0008, 0x1111);
pub const TAG_REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x0008, 0x1115);
pub const TAG_REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
pub const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
pub const TAG_RETRIEVE_URL: Tag = Tag::new(0x0008, 0x1190);
pub const TAG_REFERENCED_SOP_SEQUENCE: Tag = Tag::new(0x0008, 0x1199);
pub const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
pub const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub const TAG_PERFORMED_WORKITEM_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0x4019);
pub const TAG_RETRIEVE_URI: Tag = Tag::new(0x0040, 0xE010);
pub const TAG_RETRIEVE_LOCATION_UID: Tag = Tag::new(0x0040, 0xE011);
pub const TAG_STORAGE_MEDIA_FILE_SET_ID: Tag = Tag::new(0x0088, 0x0130);
pub const TAG_STORAGE_MEDIA_FILE_SET_UID: Tag = Tag::new(0x0088, 0x0140);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    InvalidDataSet,
    Duplicate,
    ResourceLimit,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidDataSet => "ian: invalid attribute list",
            Kind::Duplicate => "ian: notification already received",
            Kind::ResourceLimit => "ian: resource limit",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusError {
    pub status: u16,
    pub tags: Vec<Tag>,
    pub error_comment: String,
    pub kind: Kind,
}
impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tags: Vec<String> = self.tags.iter().map(|t| t.to_string()).collect();
        write!(
            f,
            "{} (status {:04X} {})",
            self.kind.text(),
            self.status,
            tags.join(" ")
        )
    }
}
impl std::error::Error for StatusError {}
#[derive(Debug, Clone)]
pub enum Error {
    Sentinel(Kind),
    Status(StatusError),
    Dimse(dimse::Error),
    Other(String),
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn is(&self, kind: Kind) -> bool {
        match self {
            Error::Sentinel(k) => *k == kind,
            Error::Status(s) => s.kind == kind,
            _ => false,
        }
    }
    fn status_error(status: u16, tags: Vec<Tag>, kind: Kind) -> Error {
        Error::Status(StatusError {
            status,
            tags,
            error_comment: String::new(),
            kind,
        })
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(k) => f.write_str(k.text()),
            Error::Status(s) => {
                let tags: Vec<String> = s.tags.iter().map(|t| t.to_string()).collect();
                write!(
                    f,
                    "{} (status {:04X} {})",
                    s.kind.text(),
                    s.status,
                    tags.join(" ")
                )
            }
            Error::Dimse(e) => f.write_str(e.text()),
            Error::Other(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for Error {}
impl From<dimse::Error> for Error {
    fn from(e: dimse::Error) -> Error {
        Error::Dimse(e)
    }
}
pub fn status_of(err: Option<&Error>) -> u16 {
    match err {
        None => DIMSE_STATUS_SUCCESS,
        Some(Error::Status(s)) => s.status,
        Some(e) if e.is(Kind::Duplicate) => DIMSE_STATUS_DUPLICATE_SOP_INSTANCE,
        Some(e) if e.is(Kind::ResourceLimit) => DIMSE_STATUS_RESOURCE_LIMITATION,
        Some(e) if e.is(Kind::InvalidDataSet) => DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
        Some(_) => DIMSE_STATUS_PROCESSING_FAILURE,
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notification {
    pub specific_character_set: String,
    pub study_instance_uid: String,
    pub performed_procedure_steps: Vec<ProcedureStepReference>,
    pub series: Vec<Series>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcedureStepReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub workitem_codes: Vec<Code>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Series {
    pub series_instance_uid: String,
    pub instances: Vec<Instance>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Instance {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub availability: String,
    pub retrieve_ae_title: String,
    pub retrieve_location_uid: String,
    pub retrieve_url: String,
    pub retrieve_uri: String,
    pub storage_media_file_set_id: String,
    pub storage_media_file_set_uid: String,
}
impl Notification {
    pub fn count(&self) -> usize {
        self.series.iter().map(|s| s.instances.len()).sum()
    }
}
fn text(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(if value.is_empty() {
            Vec::new()
        } else {
            vec![value.to_string()]
        }),
    }
}
fn sequence(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
        value: Value::Sequence(items),
    }
}
fn item(mut elements: Vec<Element>) -> DataSet {
    elements.sort_by_key(|e| (e.tag().group, e.tag().element));
    DataSet {
        elements,
        item_offset: None,
    }
}
fn instance_item(instance: &Instance) -> DataSet {
    let mut elements = vec![
        text(
            TAG_REFERENCED_SOP_CLASS_UID,
            Vr::UI,
            &instance.sop_class_uid,
        ),
        text(
            TAG_REFERENCED_SOP_INSTANCE_UID,
            Vr::UI,
            &instance.sop_instance_uid,
        ),
        text(TAG_INSTANCE_AVAILABILITY, Vr::CS, &instance.availability),
    ];
    if !instance.retrieve_ae_title.is_empty()
        || (instance.retrieve_location_uid.is_empty()
            && instance.retrieve_url.is_empty()
            && instance.retrieve_uri.is_empty())
    {
        elements.push(text(
            TAG_RETRIEVE_AE_TITLE,
            Vr::AE,
            &instance.retrieve_ae_title,
        ));
    }
    for (tag, vr, value) in [
        (
            TAG_RETRIEVE_LOCATION_UID,
            Vr::UI,
            &instance.retrieve_location_uid,
        ),
        (TAG_RETRIEVE_URL, Vr::UR, &instance.retrieve_url),
        (TAG_RETRIEVE_URI, Vr::UR, &instance.retrieve_uri),
        (
            TAG_STORAGE_MEDIA_FILE_SET_ID,
            Vr::SH,
            &instance.storage_media_file_set_id,
        ),
        (
            TAG_STORAGE_MEDIA_FILE_SET_UID,
            Vr::UI,
            &instance.storage_media_file_set_uid,
        ),
    ] {
        if !value.is_empty() {
            elements.push(text(tag, vr, value));
        }
    }
    item(elements)
}
fn code_items(codes: &[Code]) -> Vec<DataSet> {
    codes
        .iter()
        .map(|code| {
            item(vec![
                text(TAG_CODE_VALUE, Vr::SH, &code.value),
                text(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, &code.scheme),
                text(TAG_CODE_MEANING, Vr::LO, &code.meaning),
            ])
        })
        .collect()
}
pub fn build(n: &Notification) -> Result<Object> {
    let steps = n
        .performed_procedure_steps
        .iter()
        .map(|step| {
            item(vec![
                text(TAG_REFERENCED_SOP_CLASS_UID, Vr::UI, &step.sop_class_uid),
                text(
                    TAG_REFERENCED_SOP_INSTANCE_UID,
                    Vr::UI,
                    &step.sop_instance_uid,
                ),
                sequence(
                    TAG_PERFORMED_WORKITEM_CODE_SEQUENCE,
                    code_items(&step.workitem_codes),
                ),
            ])
        })
        .collect();
    let series = n
        .series
        .iter()
        .map(|s| {
            item(vec![
                sequence(
                    TAG_REFERENCED_SOP_SEQUENCE,
                    s.instances.iter().map(instance_item).collect(),
                ),
                text(TAG_SERIES_INSTANCE_UID, Vr::UI, &s.series_instance_uid),
            ])
        })
        .collect();
    let mut elements = vec![
        sequence(TAG_REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE, steps),
        sequence(TAG_REFERENCED_SERIES_SEQUENCE, series),
        text(TAG_STUDY_INSTANCE_UID, Vr::UI, &n.study_instance_uid),
    ];
    if !n.specific_character_set.is_empty() {
        elements.push(text(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            &n.specific_character_set,
        ));
    }
    let ds = Object::from_elements(item(elements).elements);
    validate(Some(&ds))?;
    Ok(ds)
}
fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}')
}
fn joined(ds: &Object, tag: Tag) -> String {
    let Ok(values) = ds.lookup_strings(tag) else {
        return String::new();
    };
    let values: Vec<&str> = values.iter().map(|v| trim_space(v)).collect();
    trim_space(&values.join("\\")).to_string()
}
pub fn parse(ds: Option<&Object>) -> Result<Notification> {
    validate(ds)?;
    let Some(ds) = ds else {
        return Ok(Notification::default());
    };
    let mut n = Notification {
        specific_character_set: joined(ds, TAG_SPECIFIC_CHARACTER_SET),
        study_instance_uid: joined(ds, TAG_STUDY_INSTANCE_UID),
        ..Default::default()
    };
    let steps = ds
        .get_sequence(TAG_REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE)
        .unwrap_or_default();
    for step in &steps {
        let codes = step
            .get_sequence(TAG_PERFORMED_WORKITEM_CODE_SEQUENCE)
            .unwrap_or_default();
        n.performed_procedure_steps.push(ProcedureStepReference {
            sop_class_uid: joined(step, TAG_REFERENCED_SOP_CLASS_UID),
            sop_instance_uid: joined(step, TAG_REFERENCED_SOP_INSTANCE_UID),
            workitem_codes: codes
                .iter()
                .map(|code| Code {
                    value: joined(code, TAG_CODE_VALUE),
                    scheme: joined(code, TAG_CODING_SCHEME_DESIGNATOR),
                    meaning: joined(code, TAG_CODE_MEANING),
                })
                .collect(),
        });
    }
    let series_items = ds
        .get_sequence(TAG_REFERENCED_SERIES_SEQUENCE)
        .unwrap_or_default();
    for series_item in &series_items {
        let instances = series_item
            .get_sequence(TAG_REFERENCED_SOP_SEQUENCE)
            .unwrap_or_default();
        n.series.push(Series {
            series_instance_uid: joined(series_item, TAG_SERIES_INSTANCE_UID),
            instances: instances
                .iter()
                .map(|instance| Instance {
                    sop_class_uid: joined(instance, TAG_REFERENCED_SOP_CLASS_UID),
                    sop_instance_uid: joined(instance, TAG_REFERENCED_SOP_INSTANCE_UID),
                    availability: joined(instance, TAG_INSTANCE_AVAILABILITY),
                    retrieve_ae_title: joined(instance, TAG_RETRIEVE_AE_TITLE),
                    retrieve_location_uid: joined(instance, TAG_RETRIEVE_LOCATION_UID),
                    retrieve_url: joined(instance, TAG_RETRIEVE_URL),
                    retrieve_uri: joined(instance, TAG_RETRIEVE_URI),
                    storage_media_file_set_id: joined(instance, TAG_STORAGE_MEDIA_FILE_SET_ID),
                    storage_media_file_set_uid: joined(instance, TAG_STORAGE_MEDIA_FILE_SET_UID),
                })
                .collect(),
        });
    }
    Ok(n)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudyInstance {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub instance: Instance,
}
pub fn group(instances: &[StudyInstance], steps: &[ProcedureStepReference]) -> Vec<Notification> {
    let mut notifications: Vec<Notification> = Vec::new();
    let mut study_index: HashMap<&str, usize> = HashMap::new();
    let mut series_index: HashMap<(&str, &str), usize> = HashMap::new();
    let mut seen: HashSet<(&str, &str, &str)> = HashSet::new();
    for located in instances {
        let study_uid = located.study_instance_uid.as_str();
        let series_uid = located.series_instance_uid.as_str();
        let study = *study_index.entry(study_uid).or_insert_with(|| {
            notifications.push(Notification {
                study_instance_uid: study_uid.to_string(),
                performed_procedure_steps: steps.to_vec(),
                ..Default::default()
            });
            notifications.len() - 1
        });
        let series = *series_index
            .entry((study_uid, series_uid))
            .or_insert_with(|| {
                notifications[study].series.push(Series {
                    series_instance_uid: series_uid.to_string(),
                    instances: Vec::new(),
                });
                notifications[study].series.len() - 1
            });
        if !seen.insert((
            study_uid,
            series_uid,
            located.instance.sop_instance_uid.as_str(),
        )) {
            continue;
        }
        notifications[study].series[series]
            .instances
            .push(located.instance.clone());
    }
    notifications
}
#[derive(Default)]
struct Checker {
    missing: Vec<Tag>,
    empty: Vec<Tag>,
    invalid: Vec<Tag>,
}
fn add_tag(tags: &mut Vec<Tag>, tag: Tag) {
    if !tags.contains(&tag) {
        tags.push(tag);
    }
}
impl Checker {
    fn err(self) -> Result<()> {
        for (status, tags) in [
            (DIMSE_STATUS_MISSING_ATTRIBUTE, self.missing),
            (DIMSE_STATUS_MISSING_ATTRIBUTE_VALUE, self.empty),
            (DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE, self.invalid),
        ] {
            if !tags.is_empty() {
                return Err(Error::status_error(status, tags, Kind::InvalidDataSet));
            }
        }
        Ok(())
    }
    fn values(&mut self, ds: &Object, tag: Tag) -> Option<Vec<String>> {
        let element = ds.get(tag)?;
        if matches!(element.value, Value::Sequence(_)) {
            add_tag(&mut self.invalid, tag);
            return Some(Vec::new());
        }
        let Ok(values) = ds.lookup_strings(tag) else {
            add_tag(&mut self.invalid, tag);
            return Some(Vec::new());
        };
        Some(
            values
                .iter()
                .map(|v| trim_space(v))
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect(),
        )
    }
    fn value(&mut self, ds: &Object, tag: Tag) -> Option<String> {
        self.values(ds, tag).map(|v| v.join("\\"))
    }
    fn type1(&mut self, ds: &Object, tag: Tag) -> String {
        match self.value(ds, tag) {
            None => {
                add_tag(&mut self.missing, tag);
                String::new()
            }
            Some(value) => {
                if value.is_empty() {
                    add_tag(&mut self.empty, tag);
                }
                value
            }
        }
    }
    fn uid(&mut self, ds: &Object, tag: Tag) {
        let value = self.type1(ds, tag);
        if !value.is_empty() && !object::is_valid_uid(value.as_bytes()) {
            add_tag(&mut self.invalid, tag);
        }
    }
    fn optional_uid(&mut self, ds: &Object, tag: Tag) -> bool {
        let value = self.value(ds, tag).unwrap_or_default();
        if !value.is_empty() && !object::is_valid_uid(value.as_bytes()) {
            add_tag(&mut self.invalid, tag);
        }
        !value.is_empty()
    }
    fn items(&mut self, ds: &Object, tag: Tag, required: bool) -> Vec<Object> {
        let Some(element) = ds.get(tag) else {
            add_tag(&mut self.missing, tag);
            return Vec::new();
        };
        if !matches!(element.value, Value::Sequence(_)) {
            add_tag(&mut self.invalid, tag);
            return Vec::new();
        }
        let items = ds.get_sequence(tag).unwrap_or_default();
        if required && items.is_empty() {
            add_tag(&mut self.empty, tag);
        }
        items
    }
    fn optional_sequence(&mut self, ds: &Object, tag: Tag) {
        if let Some(element) = ds.get(tag)
            && !matches!(element.value, Value::Sequence(_))
        {
            add_tag(&mut self.invalid, tag);
        }
    }
    fn instance(&mut self, ds: &Object) {
        self.uid(ds, TAG_REFERENCED_SOP_CLASS_UID);
        self.uid(ds, TAG_REFERENCED_SOP_INSTANCE_UID);
        let availability = self.type1(ds, TAG_INSTANCE_AVAILABILITY);
        if !availability.is_empty() && !valid_availability(&availability) {
            add_tag(&mut self.invalid, TAG_INSTANCE_AVAILABILITY);
        }
        let mut elsewhere = self.optional_uid(ds, TAG_RETRIEVE_LOCATION_UID);
        for tag in [TAG_RETRIEVE_URL, TAG_RETRIEVE_URI] {
            if self.value(ds, tag).is_some_and(|v| !v.is_empty()) {
                elsewhere = true;
            }
        }
        self.optional_uid(ds, TAG_STORAGE_MEDIA_FILE_SET_UID);
        let titles = self.values(ds, TAG_RETRIEVE_AE_TITLE);
        let present = titles.is_some();
        let titles = titles.unwrap_or_default();
        for title in &titles {
            if title.len() > 16 || title.contains(['\\', '\r', '\n']) {
                add_tag(&mut self.invalid, TAG_RETRIEVE_AE_TITLE);
            }
        }
        if titles.is_empty() && !elsewhere && availability != AVAILABILITY_UNAVAILABLE {
            if present {
                add_tag(&mut self.empty, TAG_RETRIEVE_AE_TITLE);
            } else {
                add_tag(&mut self.missing, TAG_RETRIEVE_AE_TITLE);
            }
        }
    }
}
pub fn validate(ds: Option<&Object>) -> Result<()> {
    let Some(ds) = ds else {
        return Err(Error::status_error(
            DIMSE_STATUS_MISSING_ATTRIBUTE,
            Vec::new(),
            Kind::InvalidDataSet,
        ));
    };
    let mut c = Checker::default();
    for step in c.items(ds, TAG_REFERENCED_PERFORMED_PROCEDURE_STEP_SEQUENCE, false) {
        c.uid(&step, TAG_REFERENCED_SOP_CLASS_UID);
        c.uid(&step, TAG_REFERENCED_SOP_INSTANCE_UID);
        c.optional_sequence(&step, TAG_PERFORMED_WORKITEM_CODE_SEQUENCE);
    }
    c.uid(ds, TAG_STUDY_INSTANCE_UID);
    for series in c.items(ds, TAG_REFERENCED_SERIES_SEQUENCE, true) {
        c.uid(&series, TAG_SERIES_INSTANCE_UID);
        for instance in c.items(&series, TAG_REFERENCED_SOP_SEQUENCE, true) {
            c.instance(&instance);
        }
    }
    c.err()
}
#[derive(Debug, Clone, Default)]
pub struct Received {
    pub sop_instance_uid: String,
    pub calling_ae_title: String,
    pub called_ae_title: String,
    pub notification: Notification,
    pub data_set: Option<Object>,
}
pub type Handler = Arc<dyn Fn(&Ctx, &Received) -> Result<()> + Send + Sync>;
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    pub max_data_set_bytes: i64,
    pub max_series: i64,
    pub max_instances: i64,
}
pub type UidGenerator = Arc<dyn Fn() -> std::result::Result<String, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServiceOptions {
    pub limits: Limits,
    pub uid_generator: Option<UidGenerator>,
    pub remembered_notifications: i64,
    pub accept_identical_replay: bool,
}
#[derive(Default)]
struct State {
    accepted: HashMap<String, [u8; 32]>,
    order: VecDeque<String>,
    inflight: HashSet<String>,
}
#[derive(Clone)]
pub struct Service {
    handler: Handler,
    limits: Limits,
    uids: UidGenerator,
    replay: bool,
    capacity: usize,
    state: Arc<Mutex<State>>,
}
impl Service {
    pub fn new(handler: Option<Handler>, options: ServiceOptions) -> Result<Service> {
        let Some(handler) = handler else {
            return Err(Error::Other("ian: nil handler".into()));
        };
        let mut limits = options.limits;
        if limits.max_data_set_bytes < 0
            || limits.max_series < 0
            || limits.max_instances < 0
            || options.remembered_notifications < 0
        {
            return Err(Error::Sentinel(Kind::ResourceLimit));
        }
        if limits.max_data_set_bytes == 0 {
            limits.max_data_set_bytes = 4 << 20;
        }
        if limits.max_series == 0 {
            limits.max_series = 1000;
        }
        if limits.max_instances == 0 {
            limits.max_instances = 100_000;
        }
        let capacity = match options.remembered_notifications {
            0 => 4096,
            n => n as usize,
        };
        Ok(Service {
            handler,
            limits,
            uids: options
                .uid_generator
                .unwrap_or_else(|| Arc::new(|| Ok(object::inherit::new_uid()))),
            replay: options.accept_identical_replay,
            capacity,
            state: Arc::new(Mutex::new(State::default())),
        })
    }
    pub fn receive(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        attributes: Option<&Object>,
    ) -> Result<Received> {
        self.receive_from(
            ctx,
            Received {
                sop_instance_uid: sop_instance_uid.to_string(),
                data_set: attributes.cloned(),
                ..Default::default()
            },
        )
    }
    fn receive_from(&self, ctx: &Ctx, mut received: Received) -> Result<Received> {
        if let Some(e) = dimse::context_err(ctx) {
            return Err(Error::Dimse(e));
        }
        let mut uid = trim_space(&received.sop_instance_uid).to_string();
        if uid.is_empty() {
            uid = (self.uids)().map_err(|e| Error::Other(e.to_string()))?;
        }
        if !object::is_valid_uid(uid.as_bytes()) {
            return Err(Error::status_error(
                DIMSE_STATUS_INVALID_OBJECT_INSTANCE,
                Vec::new(),
                Kind::InvalidDataSet,
            ));
        }
        received.sop_instance_uid = uid.clone();
        let notification = parse(received.data_set.as_ref())?;
        if notification.series.len() as i64 > self.limits.max_series
            || notification.count() as i64 > self.limits.max_instances
        {
            return Err(Error::status_error(
                DIMSE_STATUS_RESOURCE_LIMITATION,
                vec![TAG_REFERENCED_SERIES_SEQUENCE],
                Kind::ResourceLimit,
            ));
        }
        received.notification = notification;
        let digest = match &received.data_set {
            Some(ds) => data_set_digest(ds)?,
            None => [0; 32],
        };
        if self.begin(&uid, &digest)? {
            return Ok(received);
        }
        let result = (self.handler)(ctx, &received);
        self.finish(&uid, digest, result.is_ok());
        result?;
        Ok(received)
    }
    fn begin(&self, uid: &str, digest: &[u8; 32]) -> Result<bool> {
        let duplicate = || {
            Error::status_error(
                DIMSE_STATUS_DUPLICATE_SOP_INSTANCE,
                Vec::new(),
                Kind::Duplicate,
            )
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = state.accepted.get(uid) {
            if self.replay && previous == digest {
                return Ok(true);
            }
            return Err(duplicate());
        }
        if !state.inflight.insert(uid.to_string()) {
            return Err(duplicate());
        }
        Ok(false)
    }
    fn finish(&self, uid: &str, digest: [u8; 32], accepted: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.inflight.remove(uid);
        if !accepted {
            return;
        }
        state.accepted.insert(uid.to_string(), digest);
        state.order.push_back(uid.to_string());
        while state.order.len() > self.capacity {
            if let Some(oldest) = state.order.pop_front() {
                state.accepted.remove(&oldest);
            }
        }
    }
    pub fn normalized_options(&self, assoc: Option<&Association>) -> NormalizedScpOptions {
        let (calling, called) = assoc.map_or_else(Default::default, |a| {
            (a.calling_ae_title.clone(), a.called_ae_title.clone())
        });
        let service = self.clone();
        NormalizedScpOptions {
            max_data_set_bytes: self.limits.max_data_set_bytes,
            presentation_context_policy: Some(Arc::new(|pc: &AcceptedContext, class: &str| {
                if pc.abstract_syntax_uid != SOP_CLASS_UID || class != SOP_CLASS_UID {
                    return Err(dimse::normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        class,
                    ));
                }
                Ok(())
            })),
            create_handler: Some(Arc::new(
                move |ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>| {
                    let r = service.receive_from(
                        ctx,
                        Received {
                            sop_instance_uid: req.sop_instance_uid.clone(),
                            calling_ae_title: calling.clone(),
                            called_ae_title: called.clone(),
                            data_set: ds.cloned(),
                            ..Default::default()
                        },
                    );
                    let err = r.as_ref().err();
                    let mut response = NormalizedResponse {
                        status: status_of(err),
                        status_fields: status_fields(err),
                        ..Default::default()
                    };
                    if let Ok(received) = &r {
                        response.affected_sop_instance_uid = received.sop_instance_uid.clone();
                    }
                    (
                        NormalizedScpResult {
                            response,
                            data_set: None,
                        },
                        r.err().map(to_dimse),
                    )
                },
            )),
            ..Default::default()
        }
    }
    pub fn association_scp_options(&self, assoc: Option<&Association>) -> AssociationScpOptions {
        AssociationScpOptions {
            normalized_scp: Some(self.normalized_options(assoc)),
            ..Default::default()
        }
    }
}
fn to_dimse(e: Error) -> dimse::Error {
    match e {
        Error::Dimse(d) => d,
        other => dimse::Error::new(other.to_string()),
    }
}
fn data_set_digest(ds: &Object) -> Result<[u8; 32]> {
    let bytes = object::writer::write_data_set(ds, object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid)
        .map_err(|_| {
            Error::status_error(
                DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
                Vec::new(),
                Kind::InvalidDataSet,
            )
        })?;
    Ok(Sha256::digest(&bytes).into())
}
fn status_fields(err: Option<&Error>) -> Option<NormalizedStatusFields> {
    let Some(Error::Status(s)) = err else {
        return None;
    };
    if s.error_comment.is_empty() && s.tags.is_empty() {
        return None;
    }
    Some(NormalizedStatusFields {
        error_comment: s.error_comment.clone(),
        error_id: None,
        offending_elements: Vec::new(),
        attribute_identifier_list: s.tags.clone(),
    })
}
pub fn presentation_context() -> PresentationContext {
    PresentationContext {
        abstract_syntax_uid: SOP_CLASS_UID.into(),
        transfer_syntax_uids: vec![
            object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid.into(),
            object::transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid.into(),
        ],
        ..Default::default()
    }
}
#[derive(Debug, Clone, Default)]
pub struct NotifyResult {
    pub status: u16,
    pub sop_instance_uid: String,
    pub status_fields: Option<NormalizedStatusFields>,
}
pub struct Client<'a> {
    normalized: NormalizedClient<'a>,
}
impl<'a> Client<'a> {
    pub fn new(assoc: &'a Association) -> Client<'a> {
        Client {
            normalized: NormalizedClient::new(assoc),
        }
    }
    pub fn notify(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        attributes: Option<&Object>,
    ) -> (NotifyResult, Result<()>) {
        if let Err(e) = validate(attributes) {
            return (NotifyResult::default(), Err(e));
        }
        let mut options = NormalizedOperationOptions::default();
        options.operation.context = ctx.cloned();
        let (exchange, r) = self.normalized.create(
            &options,
            NormalizedRequest {
                sop_class_uid: SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                ..Default::default()
            },
            attributes,
        );
        let mut result = NotifyResult {
            sop_instance_uid: sop_instance_uid.into(),
            ..Default::default()
        };
        if let Some(rsp) = exchange.response {
            result.status = rsp.status;
            result.status_fields = rsp.status_fields;
            if !rsp.affected_sop_instance_uid.is_empty() {
                result.sop_instance_uid = rsp.affected_sop_instance_uid;
            }
        }
        (result, r.map_err(Error::Dimse))
    }
    pub fn notify_notification(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        n: &Notification,
    ) -> (NotifyResult, Result<()>) {
        match build(n) {
            Ok(attributes) => self.notify(ctx, sop_instance_uid, Some(&attributes)),
            Err(e) => (NotifyResult::default(), Err(e)),
        }
    }
}
