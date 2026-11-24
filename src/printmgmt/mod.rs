#![forbid(unsafe_code)]
mod attributes;
mod client;
mod native;
mod physical;
mod scp;
use crate::display::{DEFAULT_PRINT_DENSITY, PrintDensity};
use crate::net::dimse;
use crate::net::ul::Ctx;
use crate::object::{Element, Object, Tag, Value, Vr};
pub use attributes::*;
pub use client::*;
pub use native::*;
pub use physical::*;
pub use scp::*;
use std::fmt;
use std::sync::Mutex;
pub type Time = dimse::StorageCommitmentTime;
pub const GRAYSCALE_META_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.9";
pub const COLOR_META_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.18";
pub const FILM_SESSION_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.1";
pub const FILM_BOX_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.2";
pub const GRAYSCALE_IMAGE_BOX_UID: &str = "1.2.840.10008.5.1.1.4";
pub const COLOR_IMAGE_BOX_UID: &str = "1.2.840.10008.5.1.1.4.1";
pub const PRINTER_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.16";
pub const PRINTER_SOP_INSTANCE_UID: &str = "1.2.840.10008.5.1.1.17";
pub const PRINT_ACTION_TYPE_ID: u16 = 1;
pub const STATUS_SUCCESS: u16 = 0x0000;
pub const STATUS_NO_SUCH_ATTRIBUTE: u16 = 0x0105;
pub const STATUS_INVALID_ATTRIBUTE_VALUE: u16 = 0x0106;
pub const STATUS_ATTRIBUTE_LIST_ERROR: u16 = 0x0107;
pub const STATUS_PROCESSING_FAILURE: u16 = 0x0110;
pub const STATUS_NO_SUCH_SOP_INSTANCE: u16 = 0x0112;
pub const STATUS_MISSING_ATTRIBUTE: u16 = 0x0120;
pub const STATUS_MISSING_ATTRIBUTE_VALUE: u16 = 0x0121;
pub const STATUS_RESOURCE_LIMITATION: u16 = 0x0213;
pub const STATUS_MEMORY_ALLOCATION_NOT_SUPPORT: u16 = 0xB600;
pub const STATUS_EMPTY_SESSION_PAGE: u16 = 0xB602;
pub const STATUS_EMPTY_FILM_BOX_PAGE: u16 = 0xB603;
pub const STATUS_NO_FILM_BOX_IN_SESSION: u16 = 0xC600;
pub const STATUS_SESSION_QUEUE_FULL: u16 = 0xC601;
pub const STATUS_FILM_BOX_QUEUE_FULL: u16 = 0xC602;
pub const STATUS_INSUFFICIENT_PRINTER_MEMORY: u16 = 0xC605;
pub const PRINTER_NORMAL: &str = "NORMAL";
pub const PRINTER_WARNING: &str = "WARNING";
pub const PRINTER_FAILURE: &str = "FAILURE";
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
pub const TAG_MANUFACTURER: Tag = tag(0x0008, 0x0070);
pub const TAG_REFERENCED_SOP_CLASS_UID: Tag = tag(0x0008, 0x1150);
pub const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x1155);
pub const TAG_MANUFACTURER_MODEL_NAME: Tag = tag(0x0008, 0x1090);
pub const TAG_SAMPLES_PER_PIXEL: Tag = tag(0x0028, 0x0002);
pub const TAG_PHOTOMETRIC_INTERPRETATION: Tag = tag(0x0028, 0x0004);
pub const TAG_PLANAR_CONFIGURATION: Tag = tag(0x0028, 0x0006);
pub const TAG_ROWS: Tag = tag(0x0028, 0x0010);
pub const TAG_COLUMNS: Tag = tag(0x0028, 0x0011);
pub const TAG_PIXEL_ASPECT_RATIO: Tag = tag(0x0028, 0x0034);
pub const TAG_BITS_ALLOCATED: Tag = tag(0x0028, 0x0100);
pub const TAG_BITS_STORED: Tag = tag(0x0028, 0x0101);
pub const TAG_HIGH_BIT: Tag = tag(0x0028, 0x0102);
pub const TAG_PIXEL_REPRESENTATION: Tag = tag(0x0028, 0x0103);
pub const TAG_ICC_PROFILE: Tag = tag(0x0028, 0x2000);
pub const TAG_NUMBER_OF_COPIES: Tag = tag(0x2000, 0x0010);
pub const TAG_PRINT_PRIORITY: Tag = tag(0x2000, 0x0020);
pub const TAG_MEDIUM_TYPE: Tag = tag(0x2000, 0x0030);
pub const TAG_FILM_DESTINATION: Tag = tag(0x2000, 0x0040);
pub const TAG_FILM_SESSION_LABEL: Tag = tag(0x2000, 0x0050);
pub const TAG_MEMORY_ALLOCATION: Tag = tag(0x2000, 0x0060);
pub const TAG_IMAGE_DISPLAY_FORMAT: Tag = tag(0x2010, 0x0010);
pub const TAG_ANNOTATION_DISPLAY_FORMAT_ID: Tag = tag(0x2010, 0x0030);
pub const TAG_FILM_ORIENTATION: Tag = tag(0x2010, 0x0040);
pub const TAG_FILM_SIZE_ID: Tag = tag(0x2010, 0x0050);
pub const TAG_MAGNIFICATION_TYPE: Tag = tag(0x2010, 0x0060);
pub const TAG_SMOOTHING_TYPE: Tag = tag(0x2010, 0x0080);
pub const TAG_BORDER_DENSITY: Tag = tag(0x2010, 0x0100);
pub const TAG_EMPTY_IMAGE_DENSITY: Tag = tag(0x2010, 0x0110);
pub const TAG_MIN_DENSITY: Tag = tag(0x2010, 0x0120);
pub const TAG_MAX_DENSITY: Tag = tag(0x2010, 0x0130);
pub const TAG_TRIM: Tag = tag(0x2010, 0x0140);
pub const TAG_CONFIGURATION_INFORMATION: Tag = tag(0x2010, 0x0150);
pub const TAG_ILLUMINATION: Tag = tag(0x2010, 0x015E);
pub const TAG_REFLECTED_AMBIENT_LIGHT: Tag = tag(0x2010, 0x0160);
pub const TAG_REFERENCED_FILM_SESSION_SEQUENCE: Tag = tag(0x2010, 0x0500);
pub const TAG_REFERENCED_IMAGE_BOX_SEQUENCE: Tag = tag(0x2010, 0x0510);
pub const TAG_IMAGE_BOX_POSITION: Tag = tag(0x2020, 0x0010);
pub const TAG_POLARITY: Tag = tag(0x2020, 0x0020);
pub const TAG_REQUESTED_IMAGE_SIZE: Tag = tag(0x2020, 0x0030);
pub const TAG_REQUESTED_DECIMATE_CROP: Tag = tag(0x2020, 0x0040);
pub const TAG_REQUESTED_RESOLUTION_ID: Tag = tag(0x2020, 0x0050);
pub const TAG_BASIC_GRAYSCALE_IMAGE_SEQUENCE: Tag = tag(0x2020, 0x0110);
pub const TAG_BASIC_COLOR_IMAGE_SEQUENCE: Tag = tag(0x2020, 0x0111);
pub const TAG_REFERENCED_PRESENTATION_LUT: Tag = tag(0x2050, 0x0500);
pub const TAG_OWNER_ID: Tag = tag(0x2100, 0x0160);
pub const TAG_PRINTER_STATUS: Tag = tag(0x2110, 0x0010);
pub const TAG_PRINTER_STATUS_INFO: Tag = tag(0x2110, 0x0020);
pub const TAG_PRINTER_NAME: Tag = tag(0x2110, 0x0030);
pub const TAG_PROPOSED_STUDY_SEQUENCE: Tag = tag(0x2130, 0x00A0);
pub const TAG_ORIGINAL_IMAGE_SEQUENCE: Tag = tag(0x2130, 0x00C0);
pub const TAG_PIXEL_DATA: Tag = Tag::PIXEL_DATA;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    InvalidAttributes,
    QueueFull,
    NativeUnsupported,
    PhysicalGeometry,
    PhysicalUnsupported,
    InvalidFilm,
    State,
    UnknownBox,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidAttributes => "printmgmt: invalid attribute list",
            Kind::QueueFull => "printmgmt: print queue is full",
            Kind::NativeUnsupported => {
                "printmgmt: requested native print objects are unavailable or unsupported"
            }
            Kind::PhysicalGeometry => {
                "printmgmt: physical image geometry is unavailable or invalid"
            }
            Kind::PhysicalUnsupported => {
                "printmgmt: requested physical presentation is not supported"
            }
            Kind::InvalidFilm => "printmgmt: invalid film",
            Kind::State => "printmgmt: operation not allowed in the current state",
            Kind::UnknownBox => "printmgmt: unknown instance",
        }
    }
}
#[derive(Debug, Clone)]
pub struct StatusError {
    pub status: u16,
    pub tags: Vec<Tag>,
    pub error_comment: String,
    pub err: Box<Error>,
}
impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tags: Vec<String> = self.tags.iter().map(|t| t.to_string()).collect();
        write!(
            f,
            "{} (status {:04X} {})",
            self.err,
            self.status,
            tags.join(" ")
        )
    }
}
impl std::error::Error for StatusError {}
#[derive(Debug, Clone)]
pub struct PrintError {
    pub step: String,
    pub position: i64,
    pub status: u16,
    pub has_status: bool,
    pub outcome_unknown: bool,
    pub err: Option<Box<Error>>,
}
impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let step = if self.position > 0 {
            format!("{} (position {})", self.step, self.position)
        } else {
            self.step.clone()
        };
        let inner = self
            .err
            .as_ref()
            .map_or("<nil>".to_string(), |e| e.to_string());
        if self.has_status {
            write!(
                f,
                "printmgmt: {step} failed with status {:04X}H ({})",
                self.status,
                status_text(self.status)
            )
        } else if self.outcome_unknown {
            write!(
                f,
                "printmgmt: {step} response not received; the film may have been printed: {inner}"
            )
        } else {
            write!(f, "printmgmt: {step} failed: {inner}")
        }
    }
}
impl std::error::Error for PrintError {}
#[derive(Debug, Clone)]
pub enum Error {
    Sentinel(Kind),
    Wrapped(Kind, String),
    Status(StatusError),
    Print(Box<PrintError>),
    Dimse(dimse::Error),
    Join(Vec<Error>),
    Other(String),
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn is(&self, kind: Kind) -> bool {
        match self {
            Error::Sentinel(k) | Error::Wrapped(k, _) => *k == kind,
            Error::Status(s) => s.err.is(kind),
            Error::Print(p) => p.err.as_ref().is_some_and(|e| e.is(kind)),
            Error::Join(list) => list.iter().any(|e| e.is(kind)),
            Error::Dimse(_) | Error::Other(_) => false,
        }
    }
    pub fn status_error(&self) -> Option<&StatusError> {
        match self {
            Error::Status(s) => Some(s),
            Error::Print(p) => p.err.as_ref().and_then(|e| e.status_error()),
            Error::Join(list) => list.iter().find_map(|e| e.status_error()),
            _ => None,
        }
    }
    pub fn print_error(&self) -> Option<&PrintError> {
        match self {
            Error::Print(p) => Some(p),
            Error::Status(s) => s.err.print_error(),
            Error::Join(list) => list.iter().find_map(|e| e.print_error()),
            _ => None,
        }
    }
    pub fn dimse_error(&self) -> Option<&dimse::Error> {
        match self {
            Error::Dimse(e) => Some(e),
            Error::Status(s) => s.err.dimse_error(),
            Error::Print(p) => p.err.as_ref().and_then(|e| e.dimse_error()),
            Error::Join(list) => list.iter().find_map(|e| e.dimse_error()),
            _ => None,
        }
    }
    pub fn normalized_status(&self) -> Option<u16> {
        let e = self.dimse_error()?;
        if !e.is(dimse::Kind::NormalizedStatus) {
            return None;
        }
        match e.detail() {
            dimse::Detail::Status(s) => Some(*s),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(k) => f.write_str(k.text()),
            Error::Wrapped(k, m) => write!(f, "{}: {m}", k.text()),
            Error::Status(s) => {
                let tags: Vec<String> = s.tags.iter().map(|t| t.to_string()).collect();
                write!(f, "{} (status {:04X} {})", s.err, s.status, tags.join(" "))
            }
            Error::Print(p) => {
                let step = if p.position > 0 {
                    format!("{} (position {})", p.step, p.position)
                } else {
                    p.step.clone()
                };
                let inner = p
                    .err
                    .as_ref()
                    .map_or("<nil>".to_string(), |e| e.to_string());
                if p.has_status {
                    write!(
                        f,
                        "printmgmt: {step} failed with status {:04X}H ({})",
                        p.status,
                        status_text(p.status)
                    )
                } else if p.outcome_unknown {
                    write!(
                        f,
                        "printmgmt: {step} response not received; the film may have been printed: {inner}"
                    )
                } else {
                    write!(f, "printmgmt: {step} failed: {inner}")
                }
            }
            Error::Dimse(e) => f.write_str(e.text()),
            Error::Join(list) => {
                let texts: Vec<String> = list.iter().map(|e| e.to_string()).collect();
                f.write_str(&texts.join("\n"))
            }
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
impl From<Kind> for Error {
    fn from(k: Kind) -> Error {
        Error::Sentinel(k)
    }
}
pub(crate) fn invalid(tags: &[Tag]) -> Error {
    failure(
        STATUS_INVALID_ATTRIBUTE_VALUE,
        "",
        Kind::InvalidAttributes.into(),
        tags,
    )
}
pub(crate) fn failure(status: u16, comment: &str, err: Error, tags: &[Tag]) -> Error {
    Error::Status(StatusError {
        status,
        tags: tags.to_vec(),
        error_comment: comment.to_string(),
        err: Box::new(err),
    })
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    pub color: bool,
    pub rows: i64,
    pub columns: i64,
    pub bits: i64,
    pub photometric: String,
    pub pixel_aspect_ratio: [i64; 2],
    pub pixel_data: Vec<u8>,
}
#[derive(Debug, Clone, Default)]
pub struct ImageBox {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub position: i64,
    pub polarity: String,
    pub image: Option<Image>,
    pub presentation: ImageBoxPresentation,
    pub plan: PhysicalPlan,
    pub presentation_lut_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilmBoxPresentation {
    pub annotation_display_format_id: String,
    pub presentation_lut_uid: String,
    pub film_orientation: String,
    pub film_size_id: String,
    pub magnification_type: String,
    pub smoothing_type: String,
    pub border_density: String,
    pub empty_image_density: String,
    pub trim: String,
    pub min_density: i64,
    pub max_density: i64,
    pub illumination: i64,
    pub reflected_ambient_light: i64,
    pub min_density_set: bool,
    pub max_density_set: bool,
    pub illumination_set: bool,
    pub reflected_ambient_light_set: bool,
    pub configuration: String,
}
impl FilmBoxPresentation {
    pub fn print_density(&self) -> PrintDensity {
        let mut out = DEFAULT_PRINT_DENSITY;
        if self.min_density > 0 {
            out.min_density = self.min_density as f64 / 100.0;
        }
        if self.max_density > 0 {
            out.max_density = self.max_density as f64 / 100.0;
        }
        if self.illumination > 0 {
            out.illumination = self.illumination as f64;
        }
        if self.reflected_ambient_light > 0 {
            out.reflected_ambient_light = self.reflected_ambient_light as f64;
        }
        if out.min_density >= out.max_density {
            out.min_density = DEFAULT_PRINT_DENSITY.min_density;
            out.max_density = DEFAULT_PRINT_DENSITY.max_density;
        }
        out
    }
}
#[derive(Debug, Clone, Default)]
pub struct FilmBox {
    pub sop_instance_uid: String,
    pub color: bool,
    pub columns: i64,
    pub rows: i64,
    pub presentation: FilmBoxPresentation,
    pub image_boxes: Vec<ImageBox>,
    pub annotation_boxes: Vec<AnnotationBox>,
    pub printed: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilmSessionPresentation {
    pub specific_character_set: String,
    pub number_of_copies: i64,
    pub print_priority: String,
    pub medium_type: String,
    pub film_destination: String,
    pub label: String,
    pub owner_id: String,
}
#[derive(Debug, Clone, Default)]
pub struct Job {
    pub id: String,
    pub film_session_uid: String,
    pub calling_ae_title: String,
    pub session: FilmSessionPresentation,
    pub film_boxes: Vec<FilmBox>,
    pub whole_session: bool,
    pub copies: i64,
    pub requested_at: Time,
    pub empty_image_boxes: i64,
    pub image_boxes_printed: i64,
    pub presentation_luts: Vec<PresentationLutInstance>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrinterStatus {
    pub status: String,
    pub info: String,
    pub name: String,
    pub manufacturer: String,
    pub manufacturer_model_name: String,
}
pub trait Backend: Send + Sync {
    fn print(&self, ctx: &Ctx, job: &Job) -> Result<()>;
    fn printer_status(&self, ctx: &Ctx) -> Result<PrinterStatus>;
    fn native_capabilities(&self) -> Option<(bool, bool)> {
        None
    }
    fn honors_physical_image_box_plans(&self) -> Option<bool> {
        None
    }
}
fn ctx_err(ctx: &Ctx) -> Result<()> {
    match dimse::context_err(ctx) {
        Some(e) => Err(Error::Dimse(e)),
        None => Ok(()),
    }
}
struct MemoryState {
    jobs: Vec<Job>,
    status: PrinterStatus,
}
pub struct MemoryBackend {
    max: usize,
    state: Mutex<MemoryState>,
}
impl MemoryBackend {
    pub fn new(max_jobs: i64) -> MemoryBackend {
        let max = if max_jobs <= 0 { 64 } else { max_jobs as usize };
        MemoryBackend {
            max,
            state: Mutex::new(MemoryState {
                jobs: Vec::new(),
                status: PrinterStatus {
                    status: PRINTER_NORMAL.into(),
                    info: "NORMAL".into(),
                    name: "MEMORY".into(),
                    manufacturer: "dcm-rust".into(),
                    manufacturer_model_name: "dcm-rust memory backend".into(),
                },
            }),
        }
    }
    pub fn set_printer_status(&self, status: PrinterStatus) {
        self.state.lock().unwrap().status = status;
    }
    pub fn jobs(&self) -> Vec<Job> {
        self.state.lock().unwrap().jobs.clone()
    }
}
impl Backend for MemoryBackend {
    fn print(&self, ctx: &Ctx, job: &Job) -> Result<()> {
        ctx_err(ctx)?;
        let mut st = self.state.lock().unwrap();
        if st.jobs.len() >= self.max {
            return Err(Kind::QueueFull.into());
        }
        st.jobs.push(job.clone());
        Ok(())
    }
    fn printer_status(&self, ctx: &Ctx) -> Result<PrinterStatus> {
        ctx_err(ctx)?;
        Ok(self.state.lock().unwrap().status.clone())
    }
}
pub(crate) fn supported_annotation_charset(cs: &str) -> bool {
    cs.is_empty() || cs == "ISO_IR 6" || cs == "ISO_IR 192"
}
pub(crate) fn valid_native_text(value: &str, charset: &str) -> bool {
    if !supported_annotation_charset(charset) || value.len() > 256 || value.chars().count() > 64 {
        return false;
    }
    value
        .chars()
        .all(|c| !c.is_control() && c != '\\' && (charset == "ISO_IR 192" || c as u32 <= 0x7e))
}
pub(crate) fn native_text(ds: &Object, tag: Tag, charset: &str) -> Result<(String, bool)> {
    let Some(e) = ds.get(tag) else {
        return Ok((String::new(), false));
    };
    if e.vr() != Vr::LO {
        return Err(invalid(&[tag]));
    }
    let s = match &e.value {
        Value::Raw(raw) => {
            if raw.len() > 256 {
                return Err(invalid(&[tag]));
            }
            match String::from_utf8(raw.clone()) {
                Ok(s) => s,
                Err(_) => return Err(invalid(&[tag])),
            }
        }
        Value::Strings(v) => {
            if v.len() != 1 || v[0].len() > 256 {
                return Err(invalid(&[tag]));
            }
            v[0].clone()
        }
        Value::Nil => {
            if !e.header.length_set || e.header.length != 0 {
                return Err(invalid(&[tag]));
            }
            String::new()
        }
        _ => return Err(invalid(&[tag])),
    };
    if !valid_native_text(&s, charset) {
        return Err(invalid(&[tag]));
    }
    Ok((s.trim_end_matches(' ').to_string(), true))
}
pub fn build_annotation_box(position: i64, text_value: &str, charset: &str) -> Result<Object> {
    if !(1..=MAX_NATIVE_ANNOTATION_BOXES).contains(&position)
        || !valid_native_text(text_value, charset)
        || (text_value.len() % 2 == 1 && text_value.chars().count() == 64)
    {
        return Err(invalid(&[TAG_ANNOTATION_POSITION, TAG_ANNOTATION_TEXT]));
    }
    Ok(Object::from_elements(vec![
        us(TAG_ANNOTATION_POSITION, position),
        raw_element(TAG_ANNOTATION_TEXT, Vr::LO, text_value.as_bytes().to_vec()),
    ]))
}
pub(crate) fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: crate::object::ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
