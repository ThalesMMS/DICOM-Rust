#![forbid(unsafe_code)]
use crate::deid::trim_space;
use crate::derivedio::{self as dio, TAG_SOP_CLASS_UID, TAG_SOP_INSTANCE_UID};
use crate::object::text::{raw_string_values, trim_text_value};
use crate::object::{DataSet, Element, Object, Tag, Value, Vr};
const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
const TAG_REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
const TAG_REFERENCED_FRAME_NUMBER: Tag = Tag::new(0x0008, 0x1160);
const TAG_DERIVATION_DESCRIPTION: Tag = Tag::new(0x0008, 0x2111);
const TAG_SOURCE_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x2112);
const TAG_DERIVATION_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x9124);
const TAG_DERIVATION_CODE_SEQUENCE: Tag = Tag::new(0x0008, 0x9215);
const TAG_PURPOSE_OF_REFERENCE: Tag = Tag::new(0x0040, 0xa170);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
const MAX_DERIVATION_DESCRIPTION_LENGTH: usize = 1024;
pub const ERR_INVALID: &str = "dicom/derivation: invalid derivation";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
fn invalid(detail: impl AsRef<str>) -> Error {
    Error(format!("{ERR_INVALID}: {}", detail.as_ref()))
}
pub use crate::object::inherit::DerivedCommon;
pub(crate) fn string_values(e: &Element) -> Vec<String> {
    match &e.value {
        Value::Raw(raw) => raw_string_values(e.vr(), raw)
            .into_iter()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .collect(),
        Value::Strings(values) => values
            .iter()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
const fn code(value: &'static str, meaning: &'static str) -> (&'static str, &'static str) {
    (value, meaning)
}
macro_rules! dcm_codes {
    ($($name:ident = code($value:literal, $meaning:literal);)*) => {
        $(
            #[doc = concat!("The DCM code ", $value, ", \"", $meaning, "\".")]
            #[allow(non_snake_case)]
            pub fn $name() -> Code {
                let (value, meaning) = code($value, $meaning);
                Code { value: value.into(), scheme: "DCM".into(), meaning: meaning.into() }
            }
        )*
    };
}
pub mod codes {
    use super::{Code, code};
    dcm_codes! {
        LossyCompression = code("113040", "Lossy Compression");
        Segmentation = code("113076", "Segmentation");
        MultiplanarReformatting = code("113072", "Multiplanar reformatting");
        CurvedMultiplanarReformatting = code("113073", "Curved multiplanar reformatting");
        MaximumIntensityProjection = code("113078", "Maximum intensity projection");
        MinimumIntensityProjection = code("113079", "Minimum intensity projection");
        SpatialResampling = code("113085", "Spatial resampling");
        UncompressedPredecessor = code("121320", "Uncompressed predecessor");
        SourceImageForProcessing = code("121322", "Source image for image processing operation");
        MaskImageForProcessing = code("121321", "Mask image for image processing operation");
        PixelByPixelSubtraction = code("113062", "Pixel by pixel subtraction");
        SpatialRegistration = code("112353", "Spatial Registration");
        SourceDeformableSpatialRegistration = code("125028", "Source Deformable Spatial Registration");
    }
}
impl Code {
    pub fn new(value: &str, scheme: &str, meaning: &str) -> Code {
        Code {
            value: value.into(),
            scheme: scheme.into(),
            meaning: meaning.into(),
        }
    }
    fn validate(&self) -> Result<(), Error> {
        if trim_space(&self.value).is_empty()
            || trim_space(&self.scheme).is_empty()
            || trim_space(&self.meaning).is_empty()
        {
            return Err(invalid("code needs a value, scheme and meaning"));
        }
        Ok(())
    }
    pub fn item(&self) -> DataSet {
        dio::data_set(vec![
            dio::sh(TAG_CODE_VALUE, trim_space(&self.value)),
            dio::sh(TAG_CODING_SCHEME_DESIGNATOR, trim_space(&self.scheme)),
            dio::lo(TAG_CODE_MEANING, trim_space(&self.meaning)),
        ])
    }
    fn equal(&self, item: &DataSet) -> bool {
        let text = |tag: Tag| -> String {
            for e in &item.elements {
                if e.tag() == tag {
                    let values = string_values(e);
                    if values.len() == 1 {
                        return trim_space(&values[0]).to_string();
                    }
                }
            }
            String::new()
        };
        text(TAG_CODE_VALUE) == trim_space(&self.value)
            && text(TAG_CODING_SCHEME_DESIGNATOR) == trim_space(&self.scheme)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
    pub purpose: Option<Code>,
}
pub fn source_of(obj: Option<&Object>, purpose: Option<Code>) -> Result<Source, Error> {
    let Some(obj) = obj else {
        return Err(invalid("nil source"));
    };
    let source = Source {
        sop_class_uid: dio::clean_uid(obj, TAG_SOP_CLASS_UID),
        sop_instance_uid: dio::clean_uid(obj, TAG_SOP_INSTANCE_UID),
        frames: Vec::new(),
        purpose,
    };
    if source.sop_class_uid.is_empty() || source.sop_instance_uid.is_empty() {
        return Err(invalid("source has no SOP Class or SOP Instance UID"));
    }
    Ok(source)
}
impl Source {
    pub fn with_frame_indices(&self, frame_count: i64, indices: &[i64]) -> Result<Source, Error> {
        if frame_count < 1 || frame_count > i32::MAX as i64 || indices.is_empty() {
            return Err(invalid(format!(
                "frame count {frame_count} and {} selected frames",
                indices.len()
            )));
        }
        for &index in indices {
            if index < 0 || index >= frame_count {
                return Err(invalid(format!(
                    "frame index {index} is outside the {frame_count}-frame source"
                )));
            }
        }
        let mut s = self.clone();
        s.frames = Vec::new();
        if frame_count == 1 {
            return Ok(s);
        }
        if indices.len() == 1 {
            s.frames = vec![indices[0] + 1];
            return Ok(s);
        }
        for &index in indices {
            if !s.frames.contains(&(index + 1)) {
                s.frames.push(index + 1);
            }
        }
        Ok(s)
    }
    pub fn item(&self) -> Result<DataSet, Error> {
        let class = trim_space(&self.sop_class_uid);
        let instance = trim_space(&self.sop_instance_uid);
        if class.is_empty() || instance.is_empty() {
            return Err(invalid(
                "source reference needs SOP Class and SOP Instance UIDs",
            ));
        }
        let mut elements = vec![
            dio::ui(TAG_REFERENCED_SOP_CLASS_UID, class),
            dio::ui(TAG_REFERENCED_SOP_INSTANCE_UID, instance),
        ];
        if !self.frames.is_empty() {
            for &frame in &self.frames {
                if frame < 1 {
                    return Err(invalid(format!("frame number {frame}")));
                }
            }
            elements.push(dio::is(TAG_REFERENCED_FRAME_NUMBER, &self.frames));
        }
        if let Some(purpose) = &self.purpose {
            purpose.validate()?;
            elements.push(dio::seq(TAG_PURPOSE_OF_REFERENCE, vec![purpose.item()]));
        }
        Ok(dio::data_set(elements))
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Derivation {
    pub description: String,
    pub codes: Vec<Code>,
    pub sources: Vec<Source>,
}
impl Derivation {
    fn source_items(&self) -> Result<Vec<DataSet>, Error> {
        self.sources.iter().map(Source::item).collect()
    }
    pub fn item(&self) -> Result<DataSet, Error> {
        if self.codes.is_empty() {
            return Err(invalid("a derivation image item needs a derivation code"));
        }
        if self.sources.iter().any(|s| s.purpose.is_none()) {
            return Err(invalid(
                "a derivation image source needs a purpose of reference",
            ));
        }
        let mut elements = record(&[], self)?;
        if !has_tag(&elements, TAG_SOURCE_IMAGE_SEQUENCE) {
            elements.push(dio::seq(TAG_SOURCE_IMAGE_SEQUENCE, Vec::new()));
        }
        Ok(dio::data_set(elements))
    }
}
pub fn apply(obj: Option<&mut Object>, d: &Derivation) -> Result<(), Error> {
    let Some(obj) = obj else {
        return Err(invalid("nil object"));
    };
    let elements = record(obj.elements(), d)?;
    for e in elements {
        obj.put(e);
    }
    Ok(())
}
pub fn apply_functional_groups(obj: Option<&mut Object>, d: &Derivation) -> Result<(), Error> {
    let Some(obj) = obj else {
        return Err(invalid("nil object"));
    };
    let shared_items = match obj.get(TAG_SHARED_FUNCTIONAL_GROUPS).map(|e| &e.value) {
        Some(Value::Sequence(items)) if items.len() == 1 => items.clone(),
        _ => return Err(invalid("no Shared Functional Groups Sequence item")),
    };
    if let Some(Value::Sequence(frames)) =
        obj.get(TAG_PER_FRAME_FUNCTIONAL_GROUPS).map(|e| &e.value)
        && !frames.is_empty()
        && has_tag(&frames[0].elements, TAG_DERIVATION_IMAGE_SEQUENCE)
    {
        let frames = frames.clone();
        let mut items = Vec::with_capacity(frames.len());
        for (i, frame) in frames.iter().enumerate() {
            let mut step = d.clone();
            for s in &mut step.sources {
                if s.frames.is_empty() {
                    s.frames = vec![i as i64 + 1];
                }
            }
            let item = step.item()?;
            items.push(append_item(frame, TAG_DERIVATION_IMAGE_SEQUENCE, item));
        }
        obj.put(dio::seq(TAG_PER_FRAME_FUNCTIONAL_GROUPS, items));
        return Ok(());
    }
    let item = d.item()?;
    obj.put(dio::seq(
        TAG_SHARED_FUNCTIONAL_GROUPS,
        vec![append_item(
            &shared_items[0],
            TAG_DERIVATION_IMAGE_SEQUENCE,
            item,
        )],
    ));
    Ok(())
}
pub fn apply_to(obj: Option<&mut Object>, d: &Derivation) -> Result<(), Error> {
    match obj {
        Some(o) if o.has(TAG_SHARED_FUNCTIONAL_GROUPS) => apply_functional_groups(Some(o), d),
        other => apply(other, d),
    }
}
fn record(existing: &[Element], d: &Derivation) -> Result<Vec<Element>, Error> {
    let mut out = Vec::new();
    let description = trim_space(&d.description);
    if !description.is_empty() {
        let mut description = description.to_string();
        let previous = element_text(existing, TAG_DERIVATION_DESCRIPTION);
        if !previous.is_empty() {
            description = format!("{previous}; {description}");
        }
        out.push(dio::str_elem(
            TAG_DERIVATION_DESCRIPTION,
            Vr::ST,
            &truncate(&description, MAX_DERIVATION_DESCRIPTION_LENGTH),
        ));
    }
    if !d.codes.is_empty() {
        let mut codes = sequence_items(existing, TAG_DERIVATION_CODE_SEQUENCE)?;
        let mut added = false;
        for c in &d.codes {
            c.validate()?;
            if !codes.iter().any(|item| c.equal(item)) {
                codes.push(c.item());
                added = true;
            }
        }
        if added {
            out.push(dio::seq(TAG_DERIVATION_CODE_SEQUENCE, codes));
        }
    }
    if !d.sources.is_empty() {
        let mut sources = sequence_items(existing, TAG_SOURCE_IMAGE_SEQUENCE)?;
        sources.extend(d.source_items()?);
        out.push(dio::seq(TAG_SOURCE_IMAGE_SEQUENCE, sources));
    }
    Ok(out)
}
fn sequence_items(elements: &[Element], tag: Tag) -> Result<Vec<DataSet>, Error> {
    for e in elements {
        if e.tag() != tag {
            continue;
        }
        return match &e.value {
            Value::Sequence(items) => Ok(items.clone()),
            _ => Err(invalid(format!("{tag} is not a sequence"))),
        };
    }
    Ok(Vec::new())
}
fn append_item(ds: &DataSet, tag: Tag, item: DataSet) -> DataSet {
    let mut elements = Vec::with_capacity(ds.elements.len() + 1);
    let mut items = Vec::new();
    for e in &ds.elements {
        if e.tag() == tag {
            if let Value::Sequence(s) = &e.value {
                items.extend(s.iter().cloned());
            }
            continue;
        }
        elements.push(e.clone());
    }
    items.push(item);
    elements.push(dio::seq(tag, items));
    dio::data_set(elements)
}
fn has_tag(elements: &[Element], tag: Tag) -> bool {
    elements.iter().any(|e| e.tag() == tag)
}
fn element_text(elements: &[Element], tag: Tag) -> String {
    for e in elements {
        if e.tag() == tag {
            return trim_space(&string_values(e).join("\\")).to_string();
        }
    }
    String::new()
}
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    trim_space(&cut).to_string()
}
pub fn lossy_description(method: &str, ratio: f64) -> String {
    format!(
        "Lossy compression {}, compression ratio {}",
        trim_space(method),
        crate::numtext::format_float(ratio, 'g', 4)
    )
}
