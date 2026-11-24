#![forbid(unsafe_code)]
use crate::dictionary::is_private_group;
use crate::dictionary::reservations::{
    DEFAULT_MAX_PRIVATE_CREATORS, PrivateReservations, is_private_data_tag, private_tag,
};
use crate::dictionary::uid::{self, Type};
use crate::object::dict::lookup_vr;
use crate::object::transfer::{EXPLICIT_VR_LITTLE_ENDIAN, Syntax};
use crate::object::un_recovery::{UnReason, UnRecoveryOptions, recover_un_with_private};
use crate::object::{
    DataSet, Element, ElementHeader, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
    element_length, is_valid_uid,
};
use crate::validation::iod::modules as iodmodules;
use std::collections::{HashMap, HashSet};
pub const CREATOR: &str = "PAPYRUS 3.0";
pub const SECONDARY_CAPTURE_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.7";
const PAPYRUS_GROUP: u16 = 0x0041;
const OFFSET_NUMBER_IMAGES: u8 = 0x15;
const OFFSET_IMAGE_SEQUENCE: u8 = 0x50;
const DEFAULT_MAX_IMAGES: i64 = 10_000;
pub const ERR_NOT_PAPYRUS: &str = "papyrus: not a Papyrus 3.0 file";
pub const ERR_INVALID_IMAGE: &str = "papyrus: invalid image";
pub const ERR_INVALID_OPTIONS: &str = "papyrus: invalid options";
pub const ERR_RESOURCE_LIMIT: &str = "papyrus: resource limit exceeded";
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_CONVERSION_TYPE: Tag = Tag::new(0x0008, 0x0064);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    NotPapyrus,
    InvalidImage,
    InvalidOptions,
    ResourceLimit,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub text: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn sentinel(kind: ErrorKind) -> &'static str {
        match kind {
            ErrorKind::NotPapyrus => ERR_NOT_PAPYRUS,
            ErrorKind::InvalidImage => ERR_INVALID_IMAGE,
            ErrorKind::InvalidOptions => ERR_INVALID_OPTIONS,
            ErrorKind::ResourceLimit => ERR_RESOURCE_LIMIT,
        }
    }
    fn bare(kind: ErrorKind) -> Error {
        Error {
            kind,
            text: Error::sentinel(kind).to_string(),
        }
    }
    fn detail(kind: ErrorKind, detail: impl std::fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", Error::sentinel(kind)),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub fallback_sop_class_uid: String,
    pub max_images: i64,
    pub un_recovery: UnRecoveryOptions,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UidChange {
    pub tag: Tag,
    pub old: String,
    pub new: String,
    pub reason: &'static str,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageReport {
    pub sop_class_uid: String,
    pub source_sop_class_uid: String,
    pub sop_instance_uid: String,
    pub inherited: Vec<Tag>,
    pub added: Vec<Tag>,
    pub removed: Vec<Tag>,
    pub uids: Vec<UidChange>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub declared_images: i64,
    pub images: Vec<ImageReport>,
    pub transfer_syntax_uid: String,
    pub warnings: Vec<String>,
}
pub fn is_papyrus(file: Option<&File>) -> bool {
    let Some(file) = file else {
        return false;
    };
    private_tag(&file.dataset, PAPYRUS_GROUP, CREATOR, OFFSET_IMAGE_SEQUENCE)
        .is_ok_and(|tag| file.dataset.has(tag))
}
pub fn convert(file: Option<&File>, opts: &Options) -> Result<(Vec<File>, Report), Error> {
    convert_with_uids(file, opts, &mut crate::object::inherit::new_uid)
}
pub(crate) fn convert_with_uids(
    file: Option<&File>,
    opts: &Options,
    new_uid: &mut dyn FnMut() -> String,
) -> Result<(Vec<File>, Report), Error> {
    let mut fallback = opts
        .fallback_sop_class_uid
        .trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}');
    if fallback.is_empty() {
        fallback = SECONDARY_CAPTURE_IMAGE_STORAGE_UID;
    }
    if !is_image_storage(fallback) {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            format_args!(
                "{} is not a current image storage SOP Class",
                crate::textfmt::quote(fallback)
            ),
        ));
    }
    if opts.max_images < 0 {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            "negative MaxImages",
        ));
    }
    let max_images = if opts.max_images == 0 {
        DEFAULT_MAX_IMAGES
    } else {
        opts.max_images
    };
    let Some(file) = file.filter(|f| is_papyrus(Some(*f))) else {
        return Err(Error::bare(ErrorKind::NotPapyrus));
    };
    let mut report = Report {
        declared_images: -1,
        ..Default::default()
    };
    if let Ok(tag) = private_tag(&file.dataset, PAPYRUS_GROUP, CREATOR, OFFSET_NUMBER_IMAGES)
        && let Some(n) = declared_images(&file.dataset, tag)
    {
        report.declared_images = n;
    }
    let items = image_items(file, &opts.un_recovery)?;
    if items.is_empty() {
        return Err(Error::detail(ErrorKind::NotPapyrus, "empty Image Sequence"));
    }
    if items.len() as i64 > max_images {
        return Err(Error::detail(
            ErrorKind::ResourceLimit,
            format_args!("{} images, limit {max_images}", items.len()),
        ));
    }
    if report.declared_images >= 0 && report.declared_images != items.len() as i64 {
        report.warnings.push(format!(
            "Number of Images is {}, the Image Sequence has {} items",
            report.declared_images,
            items.len()
        ));
    }
    let syntax = output_syntax(file.transfer_syntax);
    report.transfer_syntax_uid = syntax.uid.to_string();
    let shared = shared_attributes(&file.dataset)?;
    let mut fixer = UidFixer {
        replaced: HashMap::new(),
        generated: HashMap::new(),
        instances: HashSet::new(),
        new_uid,
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let (mut image, image_report) = convert_image(item, &shared, fallback, &mut fixer)
            .map_err(|e| Error {
                kind: ErrorKind::InvalidImage,
                text: format!("{ERR_INVALID_IMAGE} {}: {e}", i + 1),
            })?;
        image.set_value_big_endian(syntax.big_endian);
        let mut result = File {
            preamble: Vec::new(),
            meta: Object::new(),
            dataset: image,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        };
        result.rebuild_file_meta().map_err(|e| Error {
            kind: ErrorKind::InvalidImage,
            text: format!("{ERR_INVALID_IMAGE} {}: file meta: {e}", i + 1),
        })?;
        out.push(result);
        report.images.push(image_report);
    }
    Ok((out, report))
}
fn declared_images(dataset: &Object, tag: Tag) -> Option<i64> {
    if let Ok(n) = dataset.get_int(tag) {
        return Some(n);
    }
    match dataset.get_raw(tag).as_deref() {
        Some([lo, hi]) => Some(i64::from(*lo) | i64::from(*hi) << 8),
        _ => None,
    }
}
fn catalog(creator: &str, tag: Tag) -> Option<Vr> {
    if creator != CREATOR || tag.group != PAPYRUS_GROUP {
        return None;
    }
    match tag.element as u8 {
        0x10 | OFFSET_IMAGE_SEQUENCE => Some(Vr::SQ),
        OFFSET_NUMBER_IMAGES => Some(Vr::US),
        _ => None,
    }
}
fn image_items(file: &File, limits: &UnRecoveryOptions) -> Result<Vec<DataSet>, Error> {
    let not_papyrus = |detail: String| Error::detail(ErrorKind::NotPapyrus, detail);
    let Ok(tag) = private_tag(&file.dataset, PAPYRUS_GROUP, CREATOR, OFFSET_IMAGE_SEQUENCE) else {
        return Err(Error::bare(ErrorKind::NotPapyrus));
    };
    let Some(element) = file.dataset.get(tag) else {
        return Err(Error::bare(ErrorKind::NotPapyrus));
    };
    if let Value::Sequence(items) = &element.value {
        return Ok(items.clone());
    }
    if element.vr() != Vr::UN {
        return Err(not_papyrus(format!(
            "Image Sequence has VR {}",
            element.vr().as_str()
        )));
    }
    let Some(raw) = file.dataset.get_raw(tag) else {
        return Err(not_papyrus("unreadable Image Sequence".to_string()));
    };
    let creator = Element {
        header: ElementHeader {
            tag: Tag::new(PAPYRUS_GROUP, tag.element >> 8),
            vr: Vr::LO,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![CREATOR.to_string()]),
    };
    let unknown = Element {
        header: ElementHeader::new(tag, Vr::UN, raw.len() as u32),
        value: Value::Raw(raw),
    };
    let mut holder = Object::from_elements(vec![creator, unknown]);
    holder.set_value_big_endian(file.dataset.value_big_endian());
    let mut limits = *limits;
    limits.strict = false;
    let (recovered, un_report) = recover_un_with_private(&holder, &limits, &catalog)
        .map_err(|e| not_papyrus(format!("Image Sequence: {e}")))?;
    let Some(sequence) = recovered.get_sequence(tag) else {
        let path = tag.to_string();
        let reason = un_report
            .diagnostics
            .iter()
            .rfind(|d| d.path == path)
            .map(|d| d.reason);
        let text = reason.map_or("not a sequence", UnReason::as_str);
        if reason == Some(UnReason::LimitExceeded) {
            return Err(Error::detail(
                ErrorKind::ResourceLimit,
                format_args!("Image Sequence: {text}"),
            ));
        }
        return Err(not_papyrus(format!("Image Sequence: {text}")));
    };
    Ok(sequence.iter().map(Object::to_data_set).collect())
}
fn output_syntax(source: Syntax) -> Syntax {
    if source.encapsulated || !source.is_little_endian() {
        source
    } else {
        EXPLICIT_VR_LITTLE_ENDIAN
    }
}
fn shared_attributes(dataset: &Object) -> Result<Vec<Element>, Error> {
    let mut shared = Vec::new();
    for element in dataset.elements() {
        let tag = element.tag();
        if tag.group == 0x0002
            || tag.element == 0
            || is_private_group(tag.group)
            || tag == TAG_PIXEL_DATA
        {
            continue;
        }
        let mut element = element.clone();
        if element.value == Value::Nil {
            if element_length(&element) > 0 {
                return Err(Error::detail(
                    ErrorKind::InvalidImage,
                    format_args!("unreadable {tag}"),
                ));
            }
            element.value = Value::Raw(Vec::new());
        }
        shared.push(element);
    }
    Ok(shared)
}
fn ui_element(tag: Tag, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::UI,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn convert_image(
    item: &DataSet,
    shared: &[Element],
    fallback: &str,
    fixer: &mut UidFixer<'_>,
) -> Result<(Object, ImageReport), String> {
    let mut report = ImageReport::default();
    let reservations =
        PrivateReservations::from_elements(&item.elements, DEFAULT_MAX_PRIVATE_CREATORS)
            .map_err(|e| e.to_string())?;
    let mut used: HashSet<Tag> = HashSet::new();
    let mut kept: Vec<&Element> = Vec::new();
    for element in &item.elements {
        let tag = element.tag();
        if tag.element == 0 || tag.group == 0x0002 {
            report.removed.push(tag);
            continue;
        }
        if is_private_data_tag(tag) {
            match reservations.creator(tag) {
                Ok(creator) if creator != CREATOR => {
                    used.insert(Tag::new(tag.group, tag.element >> 8));
                }
                _ => {
                    report.removed.push(tag);
                    continue;
                }
            }
        }
        kept.push(element);
    }
    let mut image = Object::new();
    for element in kept {
        let tag = element.tag();
        if is_private_group(tag.group) && !is_private_data_tag(tag) && !used.contains(&tag) {
            report.removed.push(tag);
            continue;
        }
        image.put(element.clone());
    }
    if !image.has(TAG_PIXEL_DATA) {
        return Err("no Pixel Data".to_string());
    }
    for element in shared {
        if image.has(element.tag()) {
            continue;
        }
        image.put(element.clone());
        report.inherited.push(element.tag());
    }
    let declared = image.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
    let declared = uid::normalize_uid(&declared).to_string();
    report.sop_class_uid = declared.clone();
    if !is_image_storage(&declared) {
        report.sop_class_uid = fallback.to_string();
        report.source_sop_class_uid = declared;
        image.put(ui_element(TAG_SOP_CLASS_UID, fallback));
    }
    if report.sop_class_uid == SECONDARY_CAPTURE_IMAGE_STORAGE_UID
        && !image.has(TAG_CONVERSION_TYPE)
    {
        image.put(Element {
            header: ElementHeader {
                tag: TAG_CONVERSION_TYPE,
                vr: Vr::CS,
                length: 0,
                length_set: false,
            },
            value: Value::Strings(vec!["WSD".to_string()]),
        });
        report.added.push(TAG_CONVERSION_TYPE);
    }
    let added = emit_type2(&mut image, &report.sop_class_uid);
    report.added.extend(added);
    for tag in [
        TAG_STUDY_INSTANCE_UID,
        TAG_SERIES_INSTANCE_UID,
        TAG_FRAME_OF_REFERENCE_UID,
        TAG_SOP_INSTANCE_UID,
    ] {
        if let Some(change) = fixer.fix(&mut image, tag) {
            report.uids.push(change);
        }
    }
    report.sop_instance_uid = image.get_uid(TAG_SOP_INSTANCE_UID).unwrap_or_default();
    Ok((image, report))
}
fn emit_type2(image: &mut Object, class: &str) -> Vec<Tag> {
    let name = uid::by_uid(class).map_or("", |e| e.name);
    let name = name.strip_suffix(" Storage").unwrap_or(name);
    let mut added = Vec::new();
    for usage in iodmodules::modules(name).unwrap_or_default() {
        if usage.usage != "M" {
            continue;
        }
        for attribute in iodmodules::attributes(usage.module).unwrap_or_default() {
            let tag = Tag::new((attribute.tag >> 16) as u16, attribute.tag as u16);
            if attribute.typ != "2" || image.has(tag) {
                continue;
            }
            let vr = lookup_vr(tag);
            if vr == Vr::SQ {
                image.put(Element {
                    header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
                    value: Value::Sequence(Vec::new()),
                });
            } else {
                image.put(Element {
                    header: ElementHeader::new(tag, vr, 0),
                    value: Value::Raw(Vec::new()),
                });
            }
            added.push(tag);
        }
    }
    added
}
fn is_image_storage(class: &str) -> bool {
    uid::by_uid(class).is_some_and(|e| {
        e.typ == Type::SOP_CLASS && !e.retired && e.name.ends_with("Image Storage")
    })
}
struct UidFixer<'a> {
    replaced: HashMap<Tag, HashMap<String, String>>,
    generated: HashMap<Tag, String>,
    instances: HashSet<String>,
    new_uid: &'a mut dyn FnMut() -> String,
}
impl UidFixer<'_> {
    fn fix(&mut self, image: &mut Object, tag: Tag) -> Option<UidChange> {
        let found = image.get_uid(tag);
        let value = uid::normalize_uid(found.as_deref().unwrap_or_default()).to_string();
        let reason = if found.is_none() || value.is_empty() {
            if tag == TAG_FRAME_OF_REFERENCE_UID {
                return None;
            }
            "missing"
        } else if !is_valid_uid(value.as_bytes()) {
            "invalid"
        } else if value.starts_with("1.2.840.10008.") {
            "reserved"
        } else if tag == TAG_SOP_INSTANCE_UID && self.instances.contains(&value) {
            "duplicate"
        } else {
            if tag == TAG_SOP_INSTANCE_UID {
                self.instances.insert(value);
            }
            return None;
        };
        let new = if tag == TAG_SOP_INSTANCE_UID {
            let new = (self.new_uid)();
            self.instances.insert(new.clone());
            new
        } else if reason == "missing" {
            match self.generated.get(&tag) {
                Some(existing) if !existing.is_empty() => existing.clone(),
                _ => {
                    let new = (self.new_uid)();
                    self.generated.insert(tag, new.clone());
                    new
                }
            }
        } else {
            let by_value = self.replaced.entry(tag).or_default();
            match by_value.get(&value) {
                Some(existing) if !existing.is_empty() => existing.clone(),
                _ => {
                    let new = (self.new_uid)();
                    by_value.insert(value.clone(), new.clone());
                    new
                }
            }
        };
        image.put(ui_element(tag, &new));
        Some(UidChange {
            tag,
            old: value,
            new,
            reason,
        })
    }
}
