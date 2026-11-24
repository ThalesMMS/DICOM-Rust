#![forbid(unsafe_code)]
use crate::derivation::DerivedCommon;
use crate::derivedio::{self as d, Identification};
use crate::display::VoiFunction;
use crate::fmath;
use crate::numtext::format_float;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, File, Object, Tag, Value, Vr};
use crate::render::Vec3;
use crate::textfmt::{quote, to_upper};
use std::fmt;
mod apply;
mod standard;
pub use apply::{AppliedState, apply};
pub const DEFAULT_CONTENT_LABEL: &str = "VOLUMETRIC_PS";
pub const GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE: &str =
    "1.2.840.10008.5.1.4.1.1.11.6";
pub const COMPOSITING_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE: &str =
    "1.2.840.10008.5.1.4.1.1.11.7";
pub const VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE: &str =
    "1.2.840.10008.5.1.4.1.1.11.9";
pub const SEGMENTED_VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE: &str =
    "1.2.840.10008.5.1.4.1.1.11.10";
pub const MULTIPLE_VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE: &str =
    "1.2.840.10008.5.1.4.1.1.11.11";
const PRIVATE_CREATOR: &str = "THALESMMS_VPS";
const DEFAULT_VR_PRESET_NAME: &str = "Bones and skin 1";
pub const CROP_BOUNDING_BOX: &str = "BOUNDING_BOX";
pub const MPR_THICKNESS_THIN: &str = "THIN";
pub const MPR_THICKNESS_SLAB: &str = "SLAB";
pub const PROJECTION_ORTHOGRAPHIC: &str = "ORTHOGRAPHIC";
pub const PROJECTION_PERSPECTIVE: &str = "PERSPECTIVE";
pub const RENDERING_MAXIMUM_IP: &str = "MAXIMUM_IP";
pub const RENDERING_MINIMUM_IP: &str = "MINIMUM_IP";
pub const RENDERING_AVERAGE_IP: &str = "AVERAGE_IP";
pub const RENDERING_VOLUME_RENDERED: &str = "VOLUME_RENDERED";
pub const PIXEL_PRESENTATION_MONOCHROME: &str = "MONOCHROME";
pub const PIXEL_PRESENTATION_TRUE_COLOR: &str = "TRUE_COLOR";
pub const PRESENTATION_LUT_IDENTITY: &str = "IDENTITY";
pub const PRESENTATION_LUT_INVERSE: &str = "INVERSE";
pub const RGB_TRANSFER_EQUAL: &str = "EQUAL_RGB";
pub const RGB_TRANSFER_TABLE: &str = "TABLE";
pub const ALPHA_TRANSFER_NONE: &str = "NONE";
pub const ALPHA_TRANSFER_IDENTITY: &str = "IDENTITY";
pub const ALPHA_TRANSFER_TABLE: &str = "TABLE";
pub const SHADING_SINGLE_SIDED: &str = "SINGLESIDED";
pub const SHADING_DOUBLE_SIDED: &str = "DOUBLESIDED";
const TAG_VPS_INPUT_SEQUENCE: Tag = Tag::new(0x0070, 0x1201);
const TAG_PRESENTATION_INPUT_TYPE: Tag = Tag::new(0x0070, 0x1202);
const TAG_CROP: Tag = Tag::new(0x0070, 0x1204);
const TAG_VPS_INPUT_NUMBER: Tag = Tag::new(0x0070, 0x1207);
const TAG_VPS_INPUT_SET_UID: Tag = Tag::new(0x0070, 0x1209);
const TAG_VPS_INPUT_SET_SEQUENCE: Tag = Tag::new(0x0070, 0x120A);
const TAG_GLOBAL_CROP: Tag = Tag::new(0x0070, 0x120B);
const TAG_REFERENCED_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x1140);
const TAG_PRIVATE_CREATOR: Tag = Tag::new(0x0071, 0x0010);
const TAG_RENDER_PRESET_NAME: Tag = Tag::new(0x0071, 0x1001);
const TAG_CAMERA_TARGET: Tag = Tag::new(0x0071, 0x1002);
const TAG_CAMERA_ANGLES: Tag = Tag::new(0x0071, 0x1003);
const TAG_CAMERA_DISTANCE_FOV: Tag = Tag::new(0x0071, 0x1004);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    UnsupportedSopClass,
    UnsupportedPayload,
    InvalidObject,
    MissingReference,
    Other,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/vps: unsupported SOP class",
            Kind::UnsupportedPayload => "dicom/vps: unsupported standard payload",
            Kind::InvalidObject => "dicom/vps: invalid object",
            Kind::MissingReference => "dicom/vps: missing reference",
            Kind::Other => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            text: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    fn invalid(detail: impl AsRef<str>) -> Error {
        Error::new(Kind::InvalidObject, detail)
    }
    fn unsupported(detail: impl AsRef<str>) -> Error {
        Error::new(Kind::UnsupportedPayload, detail)
    }
    fn other(e: impl fmt::Display) -> Error {
        Error {
            kind: Kind::Other,
            text: e.to_string(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencedInstance {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub number: i64,
    pub input_set_uid: String,
    pub referenced_instances: Vec<ReferencedInstance>,
    pub crop_specification_numbers: Vec<i64>,
    pub voi: Option<Voi>,
    pub rendering_method: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Voi {
    pub window_center: f64,
    pub window_width: f64,
    pub function: VoiFunction,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CroppingSpecification {
    pub number: i64,
    pub method: String,
    pub bounding_box: [f64; 6],
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MprGeometry {
    pub thickness_type: String,
    pub slab_thickness: f64,
    pub top_left: Vec3,
    pub width_direction: Vec3,
    pub width: f64,
    pub height_direction: Vec3,
    pub height: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeRenderGeometry {
    pub projection: String,
    pub position: Vec3,
    pub look_at: Vec3,
    pub up: Vec3,
    pub field_of_view: [f64; 6],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassificationComponent {
    pub input_numbers: Vec<i64>,
    pub rgb_transfer_function: String,
    pub alpha_transfer_function: String,
    pub palette_descriptor: [u16; 3],
    pub red_palette: Vec<u16>,
    pub green_palette: Vec<u16>,
    pub blue_palette: Vec<u16>,
    pub alpha_palette: Vec<u16>,
    pub description: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Display {
    pub pixel_presentation: String,
    pub presentation_lut_shape: String,
    pub input_set_uid: String,
    pub classification: Vec<ClassificationComponent>,
    pub icc_profile: Vec<u8>,
    pub color_space: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shading {
    pub style: String,
    pub ambient: f64,
    pub light_direction: Vec3,
    pub diffuse: f64,
    pub specular: f64,
    pub shininess: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub target: Vec3,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub distance: f64,
    pub fov_y: f64,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub common: DerivedCommon,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub inputs: Vec<Input>,
    pub global_crop_specification_numbers: Vec<i64>,
    pub cropping_specifications: Vec<CroppingSpecification>,
    pub mpr_geometry: Option<MprGeometry>,
    pub volume_render_geometry: Option<VolumeRenderGeometry>,
    pub rendering_method: String,
    pub display: Option<Display>,
    pub shading: Option<Shading>,
    pub render_preset_name: String,
    pub camera: Camera,
    pub identification: Identification,
    pub creation_date: String,
    pub creation_time: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub legacy_interleaved_bounding_box: bool,
    pub legacy_missing_alpha_descriptor: bool,
    pub legacy_sixteen_bit_alpha: bool,
}
pub fn srgb_icc_profile() -> Vec<u8> {
    crate::icc::iccprofile::srgb()
}
pub fn write(state: Option<&State>) -> Result<File, Error> {
    let Some(state) = state else {
        return Err(Error::invalid("state is nil"));
    };
    if !supported_sop_class(&state.sop_class_uid) {
        return Err(Error::new(Kind::UnsupportedSopClass, &state.sop_class_uid));
    }
    if standard::has_standard_state(state) {
        return standard::write_standard(state);
    }
    for (i, input) in state.inputs.iter().enumerate() {
        if input.number <= 0 || input.number > (1 << 16) - 1 {
            return Err(Error::invalid(format!(
                "input {i} number {} is outside US",
                input.number
            )));
        }
    }
    let mut elements = vec![
        d::ui(d::TAG_SOP_CLASS_UID, &state.sop_class_uid),
        d::ui(d::TAG_SOP_INSTANCE_UID, &state.sop_instance_uid),
        d::cs(d::TAG_MODALITY, "PR"),
        d::ui(d::TAG_STUDY_INSTANCE_UID, &state.study_instance_uid),
        d::ui(d::TAG_SERIES_INSTANCE_UID, &state.series_instance_uid),
    ];
    if !state.frame_of_reference_uid.is_empty() {
        elements.push(d::ui(
            d::TAG_FRAME_OF_REFERENCE_UID,
            &state.frame_of_reference_uid,
        ));
    }
    let c = &state.camera;
    elements.extend([
        input_set_sequence(&state.inputs),
        input_sequence(&state.inputs),
        d::cs(TAG_GLOBAL_CROP, "NO"),
        d::lo(TAG_PRIVATE_CREATOR, PRIVATE_CREATOR),
        d::lo(
            TAG_RENDER_PRESET_NAME,
            preset_name(&state.render_preset_name),
        ),
        d::ds(TAG_CAMERA_TARGET, &[c.target.x, c.target.y, c.target.z]),
        d::ds(TAG_CAMERA_ANGLES, &[c.yaw, c.pitch, c.roll]),
        d::ds(TAG_CAMERA_DISTANCE_FOV, &[c.distance, c.fov_y]),
    ]);
    finish(state, elements)
}
fn finish(state: &State, elements: Vec<Element>) -> Result<File, Error> {
    let mut dataset = d::object(elements);
    state
        .common
        .apply(&mut dataset, inherit_modules::ALL)
        .map_err(Error::other)?;
    d::file(&state.sop_class_uid, &state.sop_instance_uid, Some(dataset)).map_err(Error::other)
}
pub fn read(obj: Option<&Object>) -> Result<State, Error> {
    read_with_options(obj, ReadOptions::default())
}
pub fn read_with_options(obj: Option<&Object>, options: ReadOptions) -> Result<State, Error> {
    let Some(obj) = obj else {
        return Err(Error::invalid("dataset is nil"));
    };
    let sop_class_uid = d::clean_uid(obj, d::TAG_SOP_CLASS_UID);
    if !supported_sop_class(&sop_class_uid) {
        return Err(Error::new(Kind::UnsupportedSopClass, &sop_class_uid));
    }
    let creator = d::clean_string(obj, TAG_PRIVATE_CREATOR);
    let has_renderer_payload = has_private_renderer_payload(obj);
    if creator == PRIVATE_CREATOR && !has_renderer_payload {
        return Err(Error::invalid(
            "private creator is present without renderer payload",
        ));
    }
    let legacy_private_payload = creator.is_empty() && has_renderer_payload;
    if standard::has_standard_geometry(obj, &sop_class_uid) {
        return standard::read_standard(obj, &sop_class_uid, options);
    }
    if creator != PRIVATE_CREATOR && !legacy_private_payload {
        return Err(Error::unsupported(format!(
            "SOP class {sop_class_uid} requires standard VPS geometry, crop, VOI, and display modules"
        )));
    }
    Ok(State {
        sop_instance_uid: d::clean_uid(obj, d::TAG_SOP_INSTANCE_UID),
        study_instance_uid: d::clean_uid(obj, d::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: d::clean_uid(obj, d::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: d::clean_uid(obj, d::TAG_FRAME_OF_REFERENCE_UID),
        inputs: read_inputs(obj),
        render_preset_name: d::clean_string(obj, TAG_RENDER_PRESET_NAME),
        camera: read_camera(obj),
        sop_class_uid,
        ..Default::default()
    })
}
pub fn validate(state: Option<&State>) -> Result<(), Error> {
    let Some(state) = state else {
        return Err(Error::invalid("state is nil"));
    };
    if !supported_sop_class(&state.sop_class_uid) {
        return Err(Error::new(Kind::UnsupportedSopClass, &state.sop_class_uid));
    }
    if standard::has_standard_state(state) {
        return standard::validate_standard_state(state);
    }
    Ok(())
}
fn has_private_renderer_payload(obj: &Object) -> bool {
    obj.has(TAG_RENDER_PRESET_NAME)
        || obj.has(TAG_CAMERA_TARGET)
        || obj.has(TAG_CAMERA_ANGLES)
        || obj.has(TAG_CAMERA_DISTANCE_FOV)
}
fn ref_item(r: &ReferencedInstance) -> DataSet {
    d::data_set(vec![
        d::ui(d::TAG_REF_SOP_CLASS_UID, &r.sop_class_uid),
        d::ui(d::TAG_REF_SOP_INSTANCE_UID, &r.sop_instance_uid),
    ])
}
fn input_set_sequence(inputs: &[Input]) -> Element {
    let mut sets: Vec<(&str, Vec<&ReferencedInstance>)> = Vec::new();
    for input in inputs {
        let index = match sets.iter().position(|s| s.0 == input.input_set_uid) {
            Some(i) => i,
            None => {
                sets.push((&input.input_set_uid, Vec::new()));
                sets.len() - 1
            }
        };
        for r in &input.referenced_instances {
            if !sets[index].1.contains(&r) {
                sets[index].1.push(r);
            }
        }
    }
    let items = sets
        .iter()
        .map(|(uid, refs)| {
            d::data_set(vec![
                d::ui(TAG_VPS_INPUT_SET_UID, uid),
                d::cs(TAG_PRESENTATION_INPUT_TYPE, "VOLUME"),
                d::seq(
                    TAG_REFERENCED_IMAGE_SEQUENCE,
                    refs.iter().map(|r| ref_item(r)).collect(),
                ),
            ])
        })
        .collect();
    d::seq(TAG_VPS_INPUT_SET_SEQUENCE, items)
}
fn input_sequence(inputs: &[Input]) -> Element {
    let items = inputs
        .iter()
        .map(|input| {
            d::data_set(vec![
                d::us(TAG_VPS_INPUT_NUMBER, &[input.number as u16]),
                d::ui(TAG_VPS_INPUT_SET_UID, &input.input_set_uid),
                d::cs(TAG_CROP, "NO"),
            ])
        })
        .collect();
    d::seq(TAG_VPS_INPUT_SEQUENCE, items)
}
fn read_refs(owner: &Object, out: &mut Vec<ReferencedInstance>) {
    for r in d::sequence(owner, TAG_REFERENCED_IMAGE_SEQUENCE) {
        out.push(ReferencedInstance {
            sop_class_uid: d::clean_uid(&r, d::TAG_REF_SOP_CLASS_UID),
            sop_instance_uid: d::clean_uid(&r, d::TAG_REF_SOP_INSTANCE_UID),
        });
    }
}
fn read_inputs(obj: &Object) -> Vec<Input> {
    let mut input_sets: Vec<(String, Vec<ReferencedInstance>)> = Vec::new();
    for set in d::sequence(obj, TAG_VPS_INPUT_SET_SEQUENCE) {
        let uid = d::clean_uid(&set, TAG_VPS_INPUT_SET_UID);
        let index = match input_sets.iter().position(|s| s.0 == uid) {
            Some(i) => i,
            None => {
                input_sets.push((uid, Vec::new()));
                input_sets.len() - 1
            }
        };
        read_refs(&set, &mut input_sets[index].1);
    }
    d::sequence(obj, TAG_VPS_INPUT_SEQUENCE)
        .iter()
        .map(|item| {
            let mut input = Input {
                number: d::int(item, TAG_VPS_INPUT_NUMBER),
                input_set_uid: d::clean_uid(item, TAG_VPS_INPUT_SET_UID),
                ..Default::default()
            };
            if let Some(set) = input_sets.iter().find(|s| s.0 == input.input_set_uid) {
                input.referenced_instances.extend(set.1.iter().cloned());
            }
            if input.referenced_instances.is_empty() {
                for input_set in d::sequence(item, TAG_VPS_INPUT_SET_SEQUENCE) {
                    read_refs(&input_set, &mut input.referenced_instances);
                }
            }
            input
        })
        .collect()
}
fn read_camera(obj: &Object) -> Camera {
    let target = d::floats(Some(obj), TAG_CAMERA_TARGET).unwrap_or_default();
    let angles = d::floats(Some(obj), TAG_CAMERA_ANGLES).unwrap_or_default();
    let distance_fov = d::floats(Some(obj), TAG_CAMERA_DISTANCE_FOV).unwrap_or_default();
    let mut camera = Camera::default();
    if target.len() >= 3 {
        camera.target = Vec3::new(target[0], target[1], target[2]);
    }
    if angles.len() >= 3 {
        camera.yaw = angles[0];
        camera.pitch = angles[1];
        camera.roll = angles[2];
    }
    if distance_fov.len() >= 2 {
        camera.distance = distance_fov[0];
        camera.fov_y = distance_fov[1];
    }
    camera
}
fn supported_sop_class(uid: &str) -> bool {
    matches!(
        uid,
        GRAYSCALE_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE
            | COMPOSITING_PLANAR_MPR_VOLUMETRIC_PRESENTATION_STATE_STORAGE
            | VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE
            | SEGMENTED_VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE
            | MULTIPLE_VOLUME_RENDERING_VOLUMETRIC_PRESENTATION_STATE_STORAGE
    )
}
fn preset_name(value: &str) -> &str {
    if value.is_empty() {
        DEFAULT_VR_PRESET_NAME
    } else {
        value
    }
}
fn clean_cs(obj: &Object, tag: Tag) -> String {
    to_upper(crate::siemens::trim_space(&d::clean_string(obj, tag)))
}
fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
fn unit_vec(v: Vec3) -> bool {
    finite_vec(v) && (v.length() - 1.0).abs() <= 1e-4
}
fn g(v: f64) -> String {
    format_float(v, 'g', -1)
}
fn raw_bytes(e: &Element) -> Option<&[u8]> {
    match &e.value {
        Value::Raw(raw) => Some(raw),
        _ => None,
    }
}
fn us_ints(tag: Tag, values: &[i64]) -> Element {
    let out: Vec<u16> = values.iter().map(|&v| v as u16).collect();
    d::us(tag, &out)
}
fn word_element(tag: Tag, values: &[u16]) -> Element {
    d::raw(tag, Vr::OW, d::uint16_bytes(values))
}
