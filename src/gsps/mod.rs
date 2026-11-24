#![forbid(unsafe_code)]
mod compound;
mod from_image;
mod glyphs;
mod model;
mod overlay;
mod render;
use crate::deid::trim_space;
use crate::derivation::DerivedCommon;
use crate::derivedio as dio;
use crate::display::{
    Lut, ModalityLut as DisplayModalityLut, Overlay, Shutter, VoiFunction, VoiLut,
    shutter_from_object, validate_shutter,
};
use crate::object::element_setters::ss_element;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, File, Object, Tag, Vr};
use crate::textfmt::{float_v, quote, to_upper};
pub use compound::{
    COMPOUND_TYPE_ARROW, COMPOUND_TYPE_AXIS, COMPOUND_TYPE_CROSSHAIR, COMPOUND_TYPE_CUTLINE,
    COMPOUND_TYPE_ELLIPSE, COMPOUND_TYPE_INFINITE_LINE, COMPOUND_TYPE_MULTILINE,
    COMPOUND_TYPE_RANGE_LINE, COMPOUND_TYPE_RECTANGLE, COMPOUND_TYPE_RULER, CompoundFillStyle,
    CompoundGraphic, CompoundLineStyle, CompoundTextStyle, CompoundTick, GraphicGroup,
    capture_annotation_state,
};
use compound::{
    MAX_WRITTEN_COMPOUND_POINTS, compound_graphic_item, compound_index_by_id,
    compound_link_elements, graphic_group_sequence, read_compound_graphics, read_compound_link,
    read_graphic_groups, validate_compound_graphic, validate_compound_links,
};
pub use from_image::{FromImageOptions, OVERLAY_LAYER, VoiSelection, from_image};
pub use model::{
    BoundingBox, DEFAULT_CONTENT_LABEL, GraphicLayer, JUSTIFY_CENTER, JUSTIFY_LEFT, JUSTIFY_RIGHT,
    ModalityLut,
};
use model::{
    graphic_layer_sequence, identification_elements, lut_item_elements, modality_lut_elements,
    presentation_lut_sequence, read_bounding_box, read_graphic_layers, read_identification,
    read_modality_lut, read_presentation_lut, resolved_graphic_layers, text_object_item, valid_cs,
    validate_modality_lut, validate_presentation_lut, validate_text_object,
};
pub use overlay::{OverlayActivation, active_overlays};
use overlay::{overlay_activation_elements, read_overlay_activations, read_state_overlays};
pub use render::{
    BasicFont, Font, Raster, Rect, RenderOptions, basic_font, render, render_annotation_overlay,
};
pub const GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.1";
pub const PRESENTATION_LUT_IDENTITY: &str = "IDENTITY";
pub const PRESENTATION_LUT_INVERSE: &str = "INVERSE";
pub const GRAPHIC_TYPE_POINT: &str = "POINT";
pub const GRAPHIC_TYPE_POLYLINE: &str = "POLYLINE";
pub const GRAPHIC_TYPE_CIRCLE: &str = "CIRCLE";
pub const GRAPHIC_TYPE_ELLIPSE: &str = "ELLIPSE";
pub const GRAPHIC_TYPE_INTERPOLATED: &str = "INTERPOLATED";
pub const ANNOTATION_UNITS_PIXEL: &str = "PIXEL";
pub const ANNOTATION_UNITS_DISPLAY: &str = "DISPLAY";
pub const PRESENTATION_SIZE_SCALE_TO_FIT: &str = "SCALE TO FIT";
pub const PRESENTATION_SIZE_TRUE_SIZE: &str = "TRUE SIZE";
pub const PRESENTATION_SIZE_MAGNIFY: &str = "MAGNIFY";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    UnsupportedSopClass,
    UnsupportedUnits,
    InvalidObject,
    MissingReference,
    Other,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/gsps: unsupported SOP class",
            Kind::UnsupportedUnits => "dicom/gsps: unsupported annotation units",
            Kind::InvalidObject => "dicom/gsps: invalid object",
            Kind::MissingReference => "dicom/gsps: missing reference",
            Kind::Other => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    pub message: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
    pub(crate) fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            message: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    pub(crate) fn other(message: impl std::fmt::Display) -> Error {
        Error {
            kind: Kind::Other,
            message: message.to_string(),
        }
    }
    pub(crate) fn wrap(self, prefix: &str) -> Error {
        Error {
            kind: self.kind,
            message: format!("{prefix}: {}", self.message),
        }
    }
}
pub(crate) fn invalid(detail: impl AsRef<str>) -> Error {
    Error::new(Kind::InvalidObject, detail)
}
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
pub(crate) const TAG_REFERENCED_SERIES_SEQUENCE: Tag = t(0x0008, 0x1115);
pub(crate) const TAG_REFERENCED_IMAGE_SEQUENCE: Tag = t(0x0008, 0x1140);
const TAG_DISPLAYED_AREA_SELECTION_SEQ: Tag = t(0x0070, 0x005A);
const TAG_DISPLAYED_AREA_TOP_LEFT: Tag = t(0x0070, 0x0052);
const TAG_DISPLAYED_AREA_BOTTOM_RIGHT: Tag = t(0x0070, 0x0053);
const TAG_PRESENTATION_SIZE_MODE: Tag = t(0x0070, 0x0100);
const TAG_PRESENTATION_PIXEL_SPACING: Tag = t(0x0070, 0x0101);
const TAG_PRESENTATION_PIXEL_ASPECT_RATIO: Tag = t(0x0070, 0x0102);
const TAG_PRESENTATION_PIXEL_MAGNIFY: Tag = t(0x0070, 0x0103);
const TAG_IMAGE_ROTATION: Tag = t(0x0070, 0x0042);
const TAG_IMAGE_HORIZONTAL_FLIP: Tag = t(0x0070, 0x0041);
const TAG_SHUTTER_SHAPE: Tag = t(0x0018, 0x1600);
const TAG_SHUTTER_LEFT: Tag = t(0x0018, 0x1602);
const TAG_SHUTTER_RIGHT: Tag = t(0x0018, 0x1604);
const TAG_SHUTTER_UPPER: Tag = t(0x0018, 0x1606);
const TAG_SHUTTER_LOWER: Tag = t(0x0018, 0x1608);
const TAG_SHUTTER_CIRCLE_CENTER: Tag = t(0x0018, 0x1610);
const TAG_SHUTTER_CIRCLE_RADIUS: Tag = t(0x0018, 0x1612);
const TAG_SHUTTER_POLYGON_VERTICES: Tag = t(0x0018, 0x1620);
const TAG_SHUTTER_PRESENTATION_VALUE: Tag = t(0x0018, 0x1622);
const TAG_SHUTTER_OVERLAY_GROUP: Tag = t(0x0018, 0x1623);
const TAG_SHUTTER_PRESENTATION_COLOR: Tag = t(0x0018, 0x1624);
const TAG_SOFTCOPY_VOI_LUT_SEQUENCE: Tag = t(0x0028, 0x3110);
const TAG_WINDOW_CENTER: Tag = t(0x0028, 0x1050);
const TAG_WINDOW_WIDTH: Tag = t(0x0028, 0x1051);
const TAG_WINDOW_CENTER_WIDTH_EXPLANATION: Tag = t(0x0028, 0x1055);
const TAG_VOI_LUT_FUNCTION: Tag = t(0x0028, 0x1056);
const TAG_VOI_LUT_SEQUENCE: Tag = t(0x0028, 0x3010);
pub(crate) const TAG_LUT_DESCRIPTOR: Tag = t(0x0028, 0x3002);
pub(crate) const TAG_LUT_EXPLANATION: Tag = t(0x0028, 0x3003);
pub(crate) const TAG_LUT_DATA: Tag = t(0x0028, 0x3006);
pub(crate) const TAG_PRESENTATION_LUT_SHAPE: Tag = t(0x2050, 0x0020);
const TAG_GRAPHIC_ANNOTATION_SEQUENCE: Tag = t(0x0070, 0x0001);
pub(crate) const TAG_GRAPHIC_LAYER: Tag = t(0x0070, 0x0002);
pub(crate) const TAG_ANCHOR_POINT_ANNOTATION_UNITS: Tag = t(0x0070, 0x0004);
const TAG_GRAPHIC_ANNOTATION_UNITS: Tag = t(0x0070, 0x0005);
const TAG_TEXT_OBJECT_SEQUENCE: Tag = t(0x0070, 0x0008);
pub(crate) const TAG_UNFORMATTED_TEXT_VALUE: Tag = t(0x0070, 0x0006);
pub(crate) const TAG_ANCHOR_POINT: Tag = t(0x0070, 0x0014);
pub(crate) const TAG_ANCHOR_POINT_VISIBILITY: Tag = t(0x0070, 0x0015);
const TAG_GRAPHIC_OBJECT_SEQUENCE: Tag = t(0x0070, 0x0009);
pub(crate) const TAG_GRAPHIC_DIMENSIONS: Tag = t(0x0070, 0x0020);
pub(crate) const TAG_GRAPHIC_DATA: Tag = t(0x0070, 0x0022);
const TAG_GRAPHIC_TYPE: Tag = t(0x0070, 0x0023);
pub(crate) const TAG_GRAPHIC_FILLED: Tag = t(0x0070, 0x0024);
pub(crate) const TAG_NUMBER_OF_GRAPHIC_POINTS: Tag = t(0x0070, 0x0021);
pub(crate) const TAG_GRAPHIC_LAYER_SEQUENCE: Tag = t(0x0070, 0x0060);
pub(crate) const TAG_GRAPHIC_LAYER_ORDER: Tag = t(0x0070, 0x0062);
pub(crate) const TAG_LATERALITY: Tag = t(0x0020, 0x0060);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencedImage {
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayedArea {
    pub defined: bool,
    pub top_left_x: i64,
    pub top_left_y: i64,
    pub bottom_right_x: i64,
    pub bottom_right_y: i64,
    pub presentation_size_mode: String,
    pub pixel_spacing: Vec<f64>,
    pub pixel_aspect_ratio: Vec<i64>,
    pub magnification_ratio: f64,
    pub referenced_images: Vec<ReferencedImage>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpatialTransform {
    pub rotation_degrees: i64,
    pub flip_horizontal: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoftcopyVoi {
    pub window_center: f64,
    pub window_width: f64,
    pub explanation: String,
    pub function: VoiFunction,
    pub lut: Option<Lut>,
    pub referenced_images: Vec<ReferencedImage>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}
impl Point2D {
    pub const fn new(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphicAnnotation {
    pub layer_name: String,
    pub group_index: i64,
    pub text: String,
    pub anchor: Point2D,
    pub anchor_units: String,
    pub graphic_type: String,
    pub graphic_units: String,
    pub polyline: Vec<Point2D>,
    pub bounding_box: Option<BoundingBox>,
    pub anchor_visible: bool,
    pub filled: bool,
    pub compound: Option<CompoundGraphic>,
    pub compound_graphic_id: u32,
    pub has_compound_graphic_id: bool,
    pub graphic_group_id: u32,
    pub has_graphic_group_id: bool,
    pub referenced_images: Vec<ReferencedImage>,
    pub(crate) malformed_compound_link: bool,
}
impl GraphicAnnotation {
    pub(crate) fn compound_graphic_id_present(&self) -> bool {
        self.has_compound_graphic_id || self.compound_graphic_id != 0
    }
    pub(crate) fn graphic_group_id_present(&self) -> bool {
        self.has_graphic_group_id || self.graphic_group_id != 0
    }
    pub(crate) fn same_graphic_group(&self, other: &GraphicAnnotation) -> bool {
        self.graphic_group_id_present() == other.graphic_group_id_present()
            && self.graphic_group_id == other.graphic_group_id
    }
    #[doc(hidden)]
    pub fn malformed_compound_link(&self) -> bool {
        self.malformed_compound_link
    }
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub common: DerivedCommon,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: i64,
    pub laterality: String,
    pub instance_number: i64,
    pub content_label: String,
    pub content_description: String,
    pub content_creator_name: String,
    pub creation_date: String,
    pub creation_time: String,
    pub referenced_images: Vec<ReferencedImage>,
    pub displayed_area: DisplayedArea,
    pub displayed_areas: Vec<DisplayedArea>,
    pub shutter: Option<Shutter>,
    pub spatial_transform: SpatialTransform,
    pub softcopy_voi: SoftcopyVoi,
    pub softcopy_vois: Vec<SoftcopyVoi>,
    pub presentation_lut_shape: String,
    pub presentation_lut: Option<Lut>,
    pub presentation_lut_explanation: String,
    pub modality_lut: Option<ModalityLut>,
    pub graphic_layers: Vec<GraphicLayer>,
    pub graphic_groups: Vec<GraphicGroup>,
    pub graphic_annotations: Vec<GraphicAnnotation>,
    pub overlay_activations: Vec<OverlayActivation>,
    pub overlays: Vec<Overlay>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageState {
    pub sop_instance_uid: String,
    pub frame_number: i64,
    pub rows: i64,
    pub columns: i64,
}
#[derive(Clone, Debug, Default)]
pub struct AppliedState {
    pub displayed_area: DisplayedArea,
    pub shutter: Option<Shutter>,
    pub rotation_degrees: i64,
    pub flip_horizontal: bool,
    pub window_center: f64,
    pub window_width: f64,
    pub voi: VoiLut,
    pub presentation_lut: String,
    pub presentation_lut_table: Option<Lut>,
    pub modality_lut: Option<DisplayModalityLut>,
    pub graphic_layers: Vec<GraphicLayer>,
    pub graphic_annotations: Vec<GraphicAnnotation>,
}
pub(crate) fn local_now() -> crate::dcmtime::ZonedTime {
    crate::imagebuild::local_now()
}
pub fn write(state: Option<&State>) -> Result<File, Error> {
    let Some(state) = state else {
        return Err(invalid("state is nil"));
    };
    let sop_class_uid = if state.sop_class_uid.is_empty() {
        GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE
    } else {
        state.sop_class_uid.as_str()
    };
    if sop_class_uid != GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class_uid));
    }
    validate_referenced_series(&state.referenced_images)?;
    validate_displayed_areas(state)?;
    validate_shutter(state.shutter.as_ref()).map_err(Error::other)?;
    validate_softcopy_vois(state)?;
    validate_graphic_annotations(&state.graphic_annotations)?;
    validate_compound_links(&state.graphic_annotations)?;
    match state.spatial_transform.rotation_degrees {
        0 | 90 | 180 | 270 => {}
        r => {
            return Err(invalid(format!(
                "image rotation {r} is not 0, 90, 180 or 270"
            )));
        }
    }
    validate_modality_lut(state.modality_lut.as_ref())?;
    validate_presentation_lut(state.presentation_lut.as_ref())?;
    let layers = resolved_graphic_layers(state)?;
    let identification = identification_elements(state, local_now)?;
    let mut elements = vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, sop_class_uid),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &state.sop_instance_uid),
        dio::cs(dio::TAG_MODALITY, "PR"),
        dio::ui(dio::TAG_STUDY_INSTANCE_UID, &state.study_instance_uid),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, &state.series_instance_uid),
        referenced_series_sequence(&state.referenced_images),
        displayed_area_sequence(state),
        dio::us(
            TAG_IMAGE_ROTATION,
            &[state.spatial_transform.rotation_degrees as u16],
        ),
        dio::cs(
            TAG_IMAGE_HORIZONTAL_FLIP,
            flip_value(state.spatial_transform.flip_horizontal),
        ),
    ];
    elements.extend(identification);
    elements.extend(modality_lut_elements(state.modality_lut.as_ref()));
    elements.extend(shutter_elements(state.shutter.as_ref()));
    if let Some(voi) = softcopy_voi_sequence(state) {
        elements.push(voi);
    }
    match &state.presentation_lut {
        Some(lut) => elements.push(presentation_lut_sequence(
            lut,
            &state.presentation_lut_explanation,
        )),
        None => elements.push(dio::cs(
            TAG_PRESENTATION_LUT_SHAPE,
            presentation_lut_shape(&state.presentation_lut_shape),
        )),
    }
    elements.push(graphic_layer_sequence(&layers));
    if let Some(groups) = graphic_group_sequence(state)? {
        elements.push(groups);
    }
    if !state.graphic_annotations.is_empty() {
        elements.push(graphic_annotation_sequence(&state.graphic_annotations));
    }
    elements.extend(overlay_activation_elements(state)?);
    let mut dataset = dio::object(elements);
    state
        .common
        .apply(&mut dataset, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)
        .map_err(Error::other)?;
    dio::file(sop_class_uid, &state.sop_instance_uid, Some(dataset)).map_err(Error::other)
}
pub fn read(obj: Option<&Object>) -> Result<State, Error> {
    let Some(obj) = obj else {
        return Err(invalid("dataset is nil"));
    };
    let sop_class_uid = dio::clean_uid(obj, dio::TAG_SOP_CLASS_UID);
    if sop_class_uid != GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, &sop_class_uid));
    }
    let vois = read_softcopy_vois(obj)?;
    let displayed_areas = read_displayed_areas(obj)?;
    let shutter = shutter_from_object(obj).map_err(|e| Error::other(e).wrap("dicom/gsps"))?;
    let modality = read_modality_lut(obj)?;
    let (plut, plut_explanation) = read_presentation_lut(obj)?;
    let mut state = State {
        sop_class_uid,
        sop_instance_uid: dio::clean_uid(obj, dio::TAG_SOP_INSTANCE_UID),
        study_instance_uid: dio::clean_uid(obj, dio::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: dio::clean_uid(obj, dio::TAG_SERIES_INSTANCE_UID),
        referenced_images: read_references(obj),
        displayed_areas: displayed_areas.clone(),
        overlays: read_state_overlays(obj, shutter.as_ref()),
        shutter,
        spatial_transform: read_spatial_transform(obj),
        softcopy_vois: vois.clone(),
        presentation_lut_shape: dio::clean_string(obj, TAG_PRESENTATION_LUT_SHAPE),
        presentation_lut: plut,
        presentation_lut_explanation: plut_explanation,
        modality_lut: modality,
        graphic_layers: read_graphic_layers(obj),
        graphic_groups: read_graphic_groups(obj),
        graphic_annotations: read_graphic_annotations(obj),
        overlay_activations: read_overlay_activations(obj),
        ..State::default()
    };
    read_identification(obj, &mut state);
    if let Some(first) = displayed_areas.first() {
        state.displayed_area = first.clone();
    }
    if let Some(first) = vois.first() {
        state.softcopy_voi = vois
            .iter()
            .find(|v| v.referenced_images.is_empty())
            .unwrap_or(first)
            .clone();
    }
    Ok(state)
}
pub fn apply(state: Option<&State>, image: &ImageState) -> Result<AppliedState, Error> {
    let Some(state) = state else {
        return Err(invalid("state is nil"));
    };
    if !image.sop_instance_uid.is_empty()
        && !state.referenced_images.is_empty()
        && !references_apply(&state.referenced_images, image)
    {
        return Err(Error::new(
            Kind::MissingReference,
            format!("image {}", image.sop_instance_uid),
        ));
    }
    let displayed_area = select_displayed_area(state, image);
    let annotations = apply_graphic_annotations(state, &displayed_area, image)?;
    let softcopy_voi = select_softcopy_voi(state, image);
    let voi = VoiLut {
        center: softcopy_voi.window_center,
        width: softcopy_voi.window_width,
        function: softcopy_voi.function,
        lut: softcopy_voi.lut.clone(),
        explanation: String::new(),
    };
    let layers = resolved_graphic_layers(state)?;
    let modality = state.modality_lut.as_ref().map(ModalityLut::transform);
    Ok(AppliedState {
        presentation_lut_table: state.presentation_lut.clone(),
        modality_lut: modality,
        graphic_layers: layers,
        displayed_area,
        shutter: state.shutter.clone(),
        rotation_degrees: state.spatial_transform.rotation_degrees,
        flip_horizontal: state.spatial_transform.flip_horizontal,
        window_center: softcopy_voi.window_center,
        window_width: softcopy_voi.window_width,
        voi,
        presentation_lut: presentation_lut_shape(&state.presentation_lut_shape).to_string(),
        graphic_annotations: annotations,
    })
}
pub(crate) fn select_softcopy_voi(state: &State, image: &ImageState) -> SoftcopyVoi {
    let items = &state.softcopy_vois;
    if items.is_empty() {
        return state.softcopy_voi.clone();
    }
    let mut default_index = None;
    for (i, item) in items.iter().enumerate() {
        if item.referenced_images.is_empty() {
            default_index.get_or_insert(i);
            continue;
        }
        if references_apply(&item.referenced_images, image) {
            return item.clone();
        }
    }
    default_index.map(|i| items[i].clone()).unwrap_or_default()
}
pub(crate) fn select_displayed_area(state: &State, image: &ImageState) -> DisplayedArea {
    let areas = configured_displayed_areas(state);
    let mut default_index = None;
    for (i, area) in areas.iter().enumerate() {
        if area.referenced_images.is_empty() {
            default_index.get_or_insert(i);
            continue;
        }
        if references_apply(&area.referenced_images, image) {
            return area.clone();
        }
    }
    default_index.map(|i| areas[i].clone()).unwrap_or_default()
}
pub(crate) fn references_apply(refs: &[ReferencedImage], image: &ImageState) -> bool {
    for r in refs {
        if r.sop_instance_uid != image.sop_instance_uid {
            continue;
        }
        if r.frames.is_empty() {
            return true;
        }
        if image.frame_number <= 0 {
            continue;
        }
        if r.frames.contains(&image.frame_number) {
            return true;
        }
    }
    false
}
fn apply_graphic_annotations(
    state: &State,
    displayed_area: &DisplayedArea,
    image: &ImageState,
) -> Result<Vec<GraphicAnnotation>, Error> {
    let selected = compound::select_graphic_annotations(state, image);
    let mut annotations = Vec::with_capacity(selected.len());
    let transform = state.spatial_transform;
    for (i, source) in selected.into_iter().enumerate() {
        let mut annotation = source.clone();
        if let Some(b) = &source.bounding_box {
            let mut converted = b.clone();
            let wrap = |e: Error| e.wrap(&format!("annotation {i} bounding box"));
            converted.top_left = annotation_point_in_pixel_units(
                b.top_left,
                &b.units,
                displayed_area,
                image,
                transform,
            )
            .map_err(wrap)?;
            converted.bottom_right = annotation_point_in_pixel_units(
                b.bottom_right,
                &b.units,
                displayed_area,
                image,
                transform,
            )
            .map_err(wrap)?;
            converted.units = ANNOTATION_UNITS_PIXEL.into();
            annotation.bounding_box = Some(converted);
        }
        if !source.text.is_empty() && model::has_anchor(&source) {
            annotation.anchor = annotation_point_in_pixel_units(
                source.anchor,
                &source.anchor_units,
                displayed_area,
                image,
                transform,
            )
            .map_err(|e| e.wrap(&format!("annotation {i} anchor")))?;
            annotation.anchor_units = ANNOTATION_UNITS_PIXEL.into();
        }
        if !source.polyline.is_empty() {
            for (point_index, point) in source.polyline.iter().enumerate() {
                annotation.polyline[point_index] = annotation_point_in_pixel_units(
                    *point,
                    &source.graphic_units,
                    displayed_area,
                    image,
                    transform,
                )
                .map_err(|e| e.wrap(&format!("annotation {i} graphic point {point_index}")))?;
            }
            annotation.graphic_units = ANNOTATION_UNITS_PIXEL.into();
        }
        annotations.push(annotation);
    }
    Ok(annotations)
}
pub(crate) fn annotation_point_in_pixel_units(
    point: Point2D,
    units: &str,
    area: &DisplayedArea,
    image: &ImageState,
    transform: SpatialTransform,
) -> Result<Point2D, Error> {
    match normalized_annotation_units(units).as_str() {
        ANNOTATION_UNITS_PIXEL => Ok(point),
        ANNOTATION_UNITS_DISPLAY => {
            if !unit_interval(point.x) || !unit_interval(point.y) {
                return Err(invalid(format!(
                    "DISPLAY point {}\\{} is outside 0..1",
                    float_v(point.x),
                    float_v(point.y)
                )));
            }
            let (left, top, right, bottom) = displayed_area_pixel_edges(area, image)?;
            let g = SpatialGeometry::new(image.columns, image.rows, transform);
            let a = g.rotate(Point2D::new(left, top));
            let b = g.rotate(Point2D::new(right, bottom));
            let shown = Point2D::new(
                gmin(a.x, b.x) + point.x * (b.x - a.x).abs(),
                gmin(a.y, b.y) + point.y * (b.y - a.y).abs(),
            );
            Ok(g.unrotate(shown))
        }
        _ => Err(Error::new(Kind::UnsupportedUnits, quote(units))),
    }
}
fn gmin(a: f64, b: f64) -> f64 {
    crate::fmath::min(a, b)
}
pub(crate) fn displayed_area_pixel_edges(
    area: &DisplayedArea,
    image: &ImageState,
) -> Result<(f64, f64, f64, f64), Error> {
    if !displayed_area_defined(area) {
        if image.columns <= 0 || image.rows <= 0 {
            return Err(invalid("DISPLAY units require positive image dimensions"));
        }
        return Ok((0.0, 0.0, image.columns as f64, image.rows as f64));
    }
    let (l, t, r, b) = area.image_region();
    Ok(((l - 1) as f64, (t - 1) as f64, r as f64, b as f64))
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpatialGeometry {
    pub width: i64,
    pub height: i64,
    pub rotation: i64,
    pub flip: bool,
    pub rotated_width: i64,
}
impl SpatialGeometry {
    pub(crate) fn new(width: i64, height: i64, transform: SpatialTransform) -> SpatialGeometry {
        let rotated_width = if transform.rotation_degrees == 90 || transform.rotation_degrees == 270
        {
            height
        } else {
            width
        };
        SpatialGeometry {
            width,
            height,
            rotation: transform.rotation_degrees,
            flip: transform.flip_horizontal,
            rotated_width,
        }
    }
    pub(crate) fn rotate(&self, mut p: Point2D) -> Point2D {
        let (w, h) = (self.width as f64, self.height as f64);
        p = match self.rotation {
            90 => Point2D::new(h - p.y, p.x),
            180 => Point2D::new(w - p.x, h - p.y),
            270 => Point2D::new(p.y, w - p.x),
            _ => p,
        };
        if self.flip {
            p.x = self.rotated_width as f64 - p.x;
        }
        p
    }
    pub(crate) fn unrotate(&self, mut p: Point2D) -> Point2D {
        let (w, h) = (self.width as f64, self.height as f64);
        if self.flip {
            p.x = self.rotated_width as f64 - p.x;
        }
        match self.rotation {
            90 => Point2D::new(p.y, h - p.x),
            180 => Point2D::new(w - p.x, h - p.y),
            270 => Point2D::new(w - p.y, p.x),
            _ => p,
        }
    }
}
impl DisplayedArea {
    pub fn image_region(&self) -> (i64, i64, i64, i64) {
        (
            self.top_left_x.min(self.bottom_right_x),
            self.top_left_y.min(self.bottom_right_y),
            self.top_left_x.max(self.bottom_right_x),
            self.top_left_y.max(self.bottom_right_y),
        )
    }
}
impl SpatialTransform {
    pub fn corners(&self, left: i64, top: i64, right: i64, bottom: i64) -> (i64, i64, i64, i64) {
        let (left, right) = (left.min(right), left.max(right));
        let (top, bottom) = (top.min(bottom), top.max(bottom));
        let place = |x: i64, y: i64| -> (i64, i64) {
            let (mut x, y) = match self.rotation_degrees {
                90 => (y.wrapping_neg(), x),
                180 => (x.wrapping_neg(), y.wrapping_neg()),
                270 => (y, x.wrapping_neg()),
                _ => (x, y),
            };
            if self.flip_horizontal {
                x = x.wrapping_neg();
            }
            (x, y)
        };
        let corners = [(left, top), (right, top), (left, bottom), (right, bottom)];
        let mut best = 0;
        for (i, c) in corners.iter().enumerate() {
            let (x, y) = place(c.0, c.1);
            let (bx, by) = place(corners[best].0, corners[best].1);
            if x.wrapping_add(y) < bx.wrapping_add(by) {
                best = i;
            }
        }
        let opposite = corners[3 - best];
        (corners[best].0, corners[best].1, opposite.0, opposite.1)
    }
}
pub(crate) fn unit_interval(value: f64) -> bool {
    (0.0..=1.0).contains(&value)
}
fn referenced_series_sequence(refs: &[ReferencedImage]) -> Element {
    let mut groups: Vec<(String, Vec<DataSet>)> = Vec::new();
    for r in refs {
        let mut elements = vec![
            dio::ui(dio::TAG_REF_SOP_CLASS_UID, &r.sop_class_uid),
            dio::ui(dio::TAG_REF_SOP_INSTANCE_UID, &r.sop_instance_uid),
        ];
        if !r.frames.is_empty() {
            elements.push(dio::is(dio::TAG_REF_FRAME_NUMBER, &r.frames));
        }
        let ds = dio::data_set(elements);
        match groups.iter_mut().find(|g| g.0 == r.series_instance_uid) {
            Some(g) => g.1.push(ds),
            None => groups.push((r.series_instance_uid.clone(), vec![ds])),
        }
    }
    let items = groups
        .into_iter()
        .map(|(uid, images)| {
            dio::data_set(vec![
                dio::ui(dio::TAG_SERIES_INSTANCE_UID, &uid),
                dio::seq(TAG_REFERENCED_IMAGE_SEQUENCE, images),
            ])
        })
        .collect();
    dio::seq(TAG_REFERENCED_SERIES_SEQUENCE, items)
}
fn validate_referenced_series(refs: &[ReferencedImage]) -> Result<(), Error> {
    for r in refs {
        if trim_space(&r.series_instance_uid).is_empty()
            || trim_space(&r.sop_class_uid).is_empty()
            || trim_space(&r.sop_instance_uid).is_empty()
        {
            return Err(Error::new(
                Kind::MissingReference,
                "referenced image requires series, SOP class, and SOP instance UIDs",
            ));
        }
    }
    Ok(())
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn validate_displayed_area(area: &DisplayedArea) -> Result<(), Error> {
    if !displayed_area_defined(area) {
        return Err(invalid("Displayed Area is required"));
    }
    for value in [
        area.top_left_x,
        area.top_left_y,
        area.bottom_right_x,
        area.bottom_right_y,
    ] {
        if !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value) {
            return Err(invalid(format!(
                "Displayed Area coordinate {value} is outside SL"
            )));
        }
    }
    let mode = normalized_presentation_size_mode(&area.presentation_size_mode);
    match mode.as_str() {
        PRESENTATION_SIZE_SCALE_TO_FIT => {}
        PRESENTATION_SIZE_TRUE_SIZE => {
            if area.pixel_spacing.len() != 2 {
                return Err(invalid(
                    "TRUE SIZE Displayed Area requires Presentation Pixel Spacing",
                ));
            }
        }
        PRESENTATION_SIZE_MAGNIFY => {
            if area.magnification_ratio <= 0.0 || !finite(area.magnification_ratio) {
                return Err(invalid(
                    "MAGNIFY Displayed Area requires a positive magnification ratio",
                ));
            }
        }
        _ => {
            return Err(invalid(format!(
                "unsupported Presentation Size Mode {}",
                quote(&area.presentation_size_mode)
            )));
        }
    }
    if area.magnification_ratio != 0.0
        && (area.magnification_ratio < 0.0 || !finite(area.magnification_ratio))
    {
        return Err(invalid(format!(
            "invalid magnification ratio {}",
            float_v(area.magnification_ratio)
        )));
    }
    if !area.pixel_spacing.is_empty() && area.pixel_spacing.len() != 2 {
        return Err(invalid("Presentation Pixel Spacing requires two values"));
    }
    for &value in &area.pixel_spacing {
        if value <= 0.0 || !finite(value) {
            return Err(invalid(format!(
                "Presentation Pixel Spacing contains invalid value {}",
                float_v(value)
            )));
        }
    }
    if !area.pixel_aspect_ratio.is_empty() && area.pixel_aspect_ratio.len() != 2 {
        return Err(invalid(
            "Presentation Pixel Aspect Ratio requires two values",
        ));
    }
    if !area.pixel_spacing.is_empty() && !area.pixel_aspect_ratio.is_empty() {
        return Err(invalid(
            "Displayed Area cannot encode both Presentation Pixel Spacing and Aspect Ratio",
        ));
    }
    for &value in &area.pixel_aspect_ratio {
        if value <= 0 {
            return Err(invalid(format!(
                "Presentation Pixel Aspect Ratio contains invalid value {value}"
            )));
        }
    }
    for r in &area.referenced_images {
        if trim_space(&r.sop_class_uid).is_empty() || trim_space(&r.sop_instance_uid).is_empty() {
            return Err(Error::new(
                Kind::MissingReference,
                "Displayed Area reference requires SOP class and instance UIDs",
            ));
        }
    }
    Ok(())
}
fn validate_displayed_areas(state: &State) -> Result<(), Error> {
    let areas = configured_displayed_areas(state);
    if areas.is_empty() {
        return Err(invalid("Displayed Area is required"));
    }
    for (i, area) in areas.iter().enumerate() {
        validate_displayed_area(area)
            .map_err(|e| e.wrap(&format!("dicom/gsps: Displayed Area item {i}")))?;
    }
    Ok(())
}
pub(crate) fn configured_displayed_areas(state: &State) -> Vec<DisplayedArea> {
    if !state.displayed_areas.is_empty() {
        return state.displayed_areas.clone();
    }
    if displayed_area_defined(&state.displayed_area) {
        return vec![state.displayed_area.clone()];
    }
    Vec::new()
}
fn normalized_presentation_size_mode(mode: &str) -> String {
    let mode = to_upper(trim_space(mode));
    if mode.is_empty() {
        PRESENTATION_SIZE_SCALE_TO_FIT.to_string()
    } else {
        mode
    }
}
fn shutter_elements(shutter: Option<&Shutter>) -> Vec<Element> {
    let Some(shutter) = shutter else {
        return Vec::new();
    };
    let mut shapes: Vec<String> = Vec::new();
    let mut elements = Vec::new();
    if let Some(rect) = &shutter.rectangle {
        shapes.push("RECTANGULAR".into());
        elements.push(dio::is(TAG_SHUTTER_LEFT, &[rect.left]));
        elements.push(dio::is(TAG_SHUTTER_RIGHT, &[rect.right]));
        elements.push(dio::is(TAG_SHUTTER_UPPER, &[rect.upper]));
        elements.push(dio::is(TAG_SHUTTER_LOWER, &[rect.lower]));
    }
    if let Some(circle) = &shutter.circle {
        shapes.push("CIRCULAR".into());
        elements.push(dio::is(
            TAG_SHUTTER_CIRCLE_CENTER,
            &[circle.center_row, circle.center_col],
        ));
        elements.push(dio::is(TAG_SHUTTER_CIRCLE_RADIUS, &[circle.radius]));
    }
    if let Some(polygon) = &shutter.polygon {
        shapes.push("POLYGONAL".into());
        let values: Vec<i64> = polygon
            .vertices
            .iter()
            .flat_map(|p| [p.row, p.col])
            .collect();
        elements.push(dio::is(TAG_SHUTTER_POLYGON_VERTICES, &values));
    }
    if let Some(bitmap) = &shutter.bitmap {
        shapes.push("BITMAP".into());
        let group = bitmap.group;
        let overlay = &bitmap.overlay;
        elements.push(dio::us(TAG_SHUTTER_OVERLAY_GROUP, &[group]));
        elements.push(dio::us(Tag::new(group, 0x0010), &[overlay.rows as u16]));
        elements.push(dio::us(Tag::new(group, 0x0011), &[overlay.columns as u16]));
        elements.push(dio::cs(Tag::new(group, 0x0040), "G"));
        elements.push(signed_shorts(
            Tag::new(group, 0x0050),
            &[overlay.origin_row as i16, overlay.origin_col as i16],
        ));
        elements.push(dio::us(Tag::new(group, 0x0100), &[1]));
        elements.push(dio::us(Tag::new(group, 0x0102), &[0]));
        elements.push(dio::raw(
            Tag::new(group, 0x3000),
            Vr::OW,
            overlay.data.clone(),
        ));
    }
    let mut pvalue = u16::from(shutter.presentation_value).wrapping_mul(257);
    if shutter.presentation_p_value_defined {
        pvalue = shutter.presentation_p_value;
    }
    let mut head = vec![
        dio::strings(TAG_SHUTTER_SHAPE, Vr::CS, &shapes),
        dio::us(TAG_SHUTTER_PRESENTATION_VALUE, &[pvalue]),
    ];
    if shutter.presentation_color_cielab_defined {
        head.push(dio::us(
            TAG_SHUTTER_PRESENTATION_COLOR,
            &shutter.presentation_color_cielab,
        ));
    }
    head.extend(elements);
    head
}
pub(crate) fn signed_shorts(tag: Tag, values: &[i16]) -> Element {
    ss_element(tag, false, values)
}
fn validate_graphic_annotations(items: &[GraphicAnnotation]) -> Result<(), Error> {
    let mut group_refs: Vec<((i64, String), Vec<ReferencedImage>)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        for r in &item.referenced_images {
            if trim_space(&r.sop_class_uid).is_empty() || trim_space(&r.sop_instance_uid).is_empty()
            {
                return Err(Error::new(
                    Kind::MissingReference,
                    format!("annotation {i} reference requires SOP class and instance UIDs"),
                ));
            }
        }
        if item.group_index > 0 {
            let key = (item.group_index, graphic_layer_name(&item.layer_name));
            match group_refs.iter_mut().find(|g| g.0 == key) {
                Some(g) => {
                    if !referenced_images_equal(&g.1, &item.referenced_images) {
                        return Err(invalid(format!(
                            "annotation group {} on layer {} has inconsistent references",
                            item.group_index,
                            quote(&item.layer_name)
                        )));
                    }
                    g.1 = item.referenced_images.clone();
                }
                None => group_refs.push((key, item.referenced_images.clone())),
            }
        }
        if !valid_cs(&graphic_layer_name(&item.layer_name)) {
            return Err(invalid(format!(
                "annotation {i} layer {} is not a CS value",
                quote(&item.layer_name)
            )));
        }
        if let Some(c) = &item.compound {
            if !item.text.is_empty()
                || item.bounding_box.is_some()
                || item.anchor_visible
                || !item.polyline.is_empty()
                || item.filled
            {
                return Err(invalid(format!(
                    "annotation {i} mixes Compound Graphic and simple object attributes"
                )));
            }
            if c.points.len() > MAX_WRITTEN_COMPOUND_POINTS {
                return Err(invalid(format!(
                    "annotation {i} Compound Graphic exceeds Explicit VR Graphic Data length"
                )));
            }
            validate_compound_graphic(Some(c))
                .map_err(|e| e.wrap(&format!("dicom/gsps: annotation {i}")))?;
            continue;
        }
        if !item.text.is_empty() {
            validate_text_object(item)
                .map_err(|e| e.wrap(&format!("dicom/gsps: annotation {i} text")))?;
        } else if item.bounding_box.is_some() || item.anchor_visible {
            return Err(invalid(format!(
                "annotation {i} has a bounding box or visible anchor without text"
            )));
        }
        if item.filled
            && !graphic_needs_filled_attribute(
                &normalized_graphic_type_or_polyline(&item.graphic_type),
                &item.polyline,
            )
        {
            return Err(invalid(format!(
                "annotation {i} is filled but not a closed graphic"
            )));
        }
        if item.polyline.len() > (1 << 16) - 1 {
            return Err(invalid(format!(
                "annotation {i} has {} graphic points, maximum is 65535",
                item.polyline.len()
            )));
        }
        for (point_index, point) in item.polyline.iter().enumerate() {
            validate_annotation_point(*point, &item.graphic_units).map_err(|e| {
                e.wrap(&format!(
                    "dicom/gsps: annotation {i} graphic point {point_index}"
                ))
            })?;
        }
    }
    Ok(())
}
pub(crate) fn referenced_images_equal(left: &[ReferencedImage], right: &[ReferencedImage]) -> bool {
    left == right
}
pub(crate) fn validate_annotation_point(point: Point2D, units: &str) -> Result<(), Error> {
    let units = normalized_annotation_units(units);
    if units != ANNOTATION_UNITS_PIXEL && units != ANNOTATION_UNITS_DISPLAY {
        return Err(Error::new(Kind::UnsupportedUnits, quote(&units)));
    }
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(invalid("annotation point contains a non-finite value"));
    }
    if units == ANNOTATION_UNITS_DISPLAY && (!unit_interval(point.x) || !unit_interval(point.y)) {
        return Err(invalid(format!(
            "DISPLAY point {}\\{} is outside 0..1",
            float_v(point.x),
            float_v(point.y)
        )));
    }
    Ok(())
}
fn validate_softcopy_vois(state: &State) -> Result<(), Error> {
    let items = configured_softcopy_vois(state);
    for (i, item) in items.iter().enumerate() {
        for r in &item.referenced_images {
            if trim_space(&r.sop_class_uid).is_empty() || trim_space(&r.sop_instance_uid).is_empty()
            {
                return Err(Error::new(
                    Kind::MissingReference,
                    format!("Softcopy VOI item {i} reference requires SOP class and instance UIDs"),
                ));
            }
        }
        let Some(lut) = &item.lut else {
            if !item.window_center.is_finite() || !item.window_width.is_finite() {
                return Err(invalid(format!(
                    "Softcopy VOI item {i} window contains a non-finite value"
                )));
            }
            if item.function == VoiFunction::Linear && item.window_width < 1.0 {
                return Err(invalid(format!(
                    "Softcopy VOI item {i} LINEAR window width {} is below 1",
                    float_v(item.window_width)
                )));
            }
            if item.function != VoiFunction::Linear && item.window_width <= 0.0 {
                return Err(invalid(format!(
                    "Softcopy VOI item {i} window width {} is not positive",
                    float_v(item.window_width)
                )));
            }
            continue;
        };
        if lut.bits_per_entry != 8 && lut.bits_per_entry != 16 {
            return Err(invalid(format!(
                "Softcopy VOI item {i} LUT has {} bits per entry, not 8 or 16",
                lut.bits_per_entry
            )));
        }
        if lut.first_mapped < -(1 << 15) || lut.first_mapped > (1 << 16) - 1 {
            return Err(invalid(format!(
                "Softcopy VOI item {i} LUT first mapped value {} is outside SS/US",
                lut.first_mapped
            )));
        }
        if lut.entries.len() > 1 << 16 {
            return Err(invalid(format!(
                "Softcopy VOI item {i} LUT has {} entries, maximum is 65536",
                lut.entries.len()
            )));
        }
        let mut count = lut.entries.len() as i64;
        if count == 1 << 16 {
            count = 0;
        }
        Lut::new(
            &[count, lut.first_mapped, lut.bits_per_entry],
            lut.entries.clone(),
        )
        .map_err(|e| Error::other(format!("dicom/gsps: Softcopy VOI item {i}: {e}")))?;
    }
    Ok(())
}
pub(crate) fn displayed_area_defined(area: &DisplayedArea) -> bool {
    area.defined
        || area.top_left_x != 0
        || area.top_left_y != 0
        || area.bottom_right_x != 0
        || area.bottom_right_y != 0
}
fn configured_softcopy_vois(state: &State) -> Vec<SoftcopyVoi> {
    if !state.softcopy_vois.is_empty() {
        return state.softcopy_vois.clone();
    }
    if softcopy_voi_defined(&state.softcopy_voi) {
        return vec![state.softcopy_voi.clone()];
    }
    Vec::new()
}
pub(crate) fn softcopy_voi_defined(voi: &SoftcopyVoi) -> bool {
    voi.window_center != 0.0
        || voi.window_width != 0.0
        || !voi.explanation.is_empty()
        || voi.function != VoiFunction::Linear
        || voi.lut.is_some()
        || !voi.referenced_images.is_empty()
}
fn displayed_area_sequence(state: &State) -> Element {
    let areas = configured_displayed_areas(state);
    let mut items = Vec::with_capacity(areas.len());
    for area in &areas {
        let mut elements = vec![
            dio::sl(
                TAG_DISPLAYED_AREA_TOP_LEFT,
                &[area.top_left_x as i32, area.top_left_y as i32],
            ),
            dio::sl(
                TAG_DISPLAYED_AREA_BOTTOM_RIGHT,
                &[area.bottom_right_x as i32, area.bottom_right_y as i32],
            ),
            dio::cs(
                TAG_PRESENTATION_SIZE_MODE,
                &normalized_presentation_size_mode(&area.presentation_size_mode),
            ),
        ];
        if !area.referenced_images.is_empty() {
            elements.push(voi_referenced_image_sequence(&area.referenced_images));
        }
        if area.pixel_spacing.len() == 2 {
            elements.push(dio::ds(TAG_PRESENTATION_PIXEL_SPACING, &area.pixel_spacing));
        }
        if area.pixel_aspect_ratio.len() == 2 {
            elements.push(dio::is(
                TAG_PRESENTATION_PIXEL_ASPECT_RATIO,
                &area.pixel_aspect_ratio,
            ));
        } else if area.pixel_spacing.len() != 2 {
            elements.push(dio::is(TAG_PRESENTATION_PIXEL_ASPECT_RATIO, &[1, 1]));
        }
        if area.magnification_ratio != 0.0 {
            elements.push(dio::ds(
                TAG_PRESENTATION_PIXEL_MAGNIFY,
                &[area.magnification_ratio],
            ));
        }
        items.push(dio::data_set(elements));
    }
    dio::seq(TAG_DISPLAYED_AREA_SELECTION_SEQ, items)
}
fn softcopy_voi_sequence(state: &State) -> Option<Element> {
    let items = configured_softcopy_vois(state);
    if items.is_empty() {
        return None;
    }
    let mut sets = Vec::with_capacity(items.len());
    for voi in &items {
        let mut elements = Vec::new();
        if !voi.referenced_images.is_empty() {
            elements.push(voi_referenced_image_sequence(&voi.referenced_images));
        }
        match &voi.lut {
            Some(lut) => elements.push(dio::seq(
                TAG_VOI_LUT_SEQUENCE,
                vec![dio::data_set(lut_item_elements(lut, &voi.explanation))],
            )),
            None => {
                elements.push(dio::ds(TAG_WINDOW_CENTER, &[voi.window_center]));
                elements.push(dio::ds(TAG_WINDOW_WIDTH, &[voi.window_width]));
                elements.push(dio::lo(
                    TAG_WINDOW_CENTER_WIDTH_EXPLANATION,
                    &voi.explanation,
                ));
                elements.push(dio::cs(
                    TAG_VOI_LUT_FUNCTION,
                    voi_function_value(voi.function),
                ));
            }
        }
        sets.push(dio::data_set(elements));
    }
    Some(dio::seq(TAG_SOFTCOPY_VOI_LUT_SEQUENCE, sets))
}
fn voi_referenced_image_sequence(refs: &[ReferencedImage]) -> Element {
    let items = refs
        .iter()
        .map(|r| {
            let mut elements = vec![
                dio::ui(dio::TAG_REF_SOP_CLASS_UID, &r.sop_class_uid),
                dio::ui(dio::TAG_REF_SOP_INSTANCE_UID, &r.sop_instance_uid),
            ];
            if !r.frames.is_empty() {
                elements.push(dio::is(dio::TAG_REF_FRAME_NUMBER, &r.frames));
            }
            dio::data_set(elements)
        })
        .collect();
    dio::seq(TAG_REFERENCED_IMAGE_SEQUENCE, items)
}
fn voi_function_value(function: VoiFunction) -> &'static str {
    match function {
        VoiFunction::LinearExact => "LINEAR_EXACT",
        VoiFunction::Sigmoid => "SIGMOID",
        VoiFunction::Linear => "LINEAR",
    }
}
fn graphic_annotation_sequence(items: &[GraphicAnnotation]) -> Element {
    struct Group<'a> {
        layer: String,
        items: Vec<&'a GraphicAnnotation>,
        refs: Vec<ReferencedImage>,
    }
    let mut groups: Vec<Group<'_>> = Vec::with_capacity(items.len());
    let mut group_indexes: Vec<((i64, String, bool), usize)> = Vec::new();
    let compound_indexes = compound_index_by_id(items);
    for item in items {
        let layer = graphic_layer_name(&item.layer_name);
        let key = if item.group_index > 0 {
            (item.group_index, layer.clone(), false)
        } else if item.compound_graphic_id_present() {
            let index = compound_indexes
                .get(&item.compound_graphic_id)
                .map_or(0, |&i| i as i64);
            (index, layer.clone(), true)
        } else {
            groups.push(Group {
                layer,
                items: vec![item],
                refs: item.referenced_images.clone(),
            });
            continue;
        };
        if let Some((_, gi)) = group_indexes.iter().find(|(k, _)| *k == key) {
            groups[*gi].items.push(item);
            continue;
        }
        group_indexes.push((key, groups.len()));
        groups.push(Group {
            layer,
            items: vec![item],
            refs: item.referenced_images.clone(),
        });
    }
    let mut sets = Vec::with_capacity(groups.len());
    for group in &groups {
        let mut elements = vec![dio::cs(TAG_GRAPHIC_LAYER, &group.layer)];
        if !group.refs.is_empty() {
            elements.push(voi_referenced_image_sequence(&group.refs));
        }
        let mut texts = Vec::new();
        let mut graphics = Vec::new();
        let mut compounds = Vec::new();
        for item in &group.items {
            if item.compound.is_some() {
                compounds.push(compound_graphic_item(item));
                continue;
            }
            if !item.text.is_empty() {
                texts.push(text_object_item(item));
            }
            if !item.polyline.is_empty() {
                let graphic_type = normalized_graphic_type_or_polyline(&item.graphic_type);
                let mut graphic_elements = vec![
                    dio::cs(
                        TAG_GRAPHIC_ANNOTATION_UNITS,
                        &normalized_annotation_units(&item.graphic_units),
                    ),
                    dio::us(TAG_GRAPHIC_DIMENSIONS, &[2]),
                    dio::us(TAG_NUMBER_OF_GRAPHIC_POINTS, &[item.polyline.len() as u16]),
                    dio::fl(TAG_GRAPHIC_DATA, &flatten_points(&item.polyline)),
                    dio::cs(TAG_GRAPHIC_TYPE, &graphic_type),
                ];
                if graphic_needs_filled_attribute(&graphic_type, &item.polyline) {
                    graphic_elements.push(dio::cs(TAG_GRAPHIC_FILLED, flip_value(item.filled)));
                }
                graphic_elements.extend(compound_link_elements(item));
                graphics.push(dio::data_set(graphic_elements));
            }
        }
        if !texts.is_empty() {
            elements.push(dio::seq(TAG_TEXT_OBJECT_SEQUENCE, texts));
        }
        if !graphics.is_empty() {
            elements.push(dio::seq(TAG_GRAPHIC_OBJECT_SEQUENCE, graphics));
        }
        if !compounds.is_empty() {
            elements.push(dio::seq(compound::TAG_COMPOUND_GRAPHIC_SEQUENCE, compounds));
        }
        sets.push(dio::data_set(elements));
    }
    dio::seq(TAG_GRAPHIC_ANNOTATION_SEQUENCE, sets)
}
fn ref_frames(obj: &Object) -> Vec<i64> {
    obj.get_ints(dio::TAG_REF_FRAME_NUMBER).unwrap_or_default()
}
fn read_references(obj: &Object) -> Vec<ReferencedImage> {
    let mut out = Vec::new();
    for series in dio::sequence(obj, TAG_REFERENCED_SERIES_SEQUENCE) {
        let series_uid = dio::clean_uid(&series, dio::TAG_SERIES_INSTANCE_UID);
        for r in dio::sequence(&series, TAG_REFERENCED_IMAGE_SEQUENCE) {
            out.push(ReferencedImage {
                series_instance_uid: series_uid.clone(),
                sop_class_uid: dio::clean_uid(&r, dio::TAG_REF_SOP_CLASS_UID),
                sop_instance_uid: dio::clean_uid(&r, dio::TAG_REF_SOP_INSTANCE_UID),
                frames: ref_frames(&r),
            });
        }
    }
    out
}
pub(crate) fn read_direct_image_references(obj: &Object) -> Vec<ReferencedImage> {
    dio::sequence(obj, TAG_REFERENCED_IMAGE_SEQUENCE)
        .iter()
        .map(|r| ReferencedImage {
            series_instance_uid: String::new(),
            sop_class_uid: dio::clean_uid(r, dio::TAG_REF_SOP_CLASS_UID),
            sop_instance_uid: dio::clean_uid(r, dio::TAG_REF_SOP_INSTANCE_UID),
            frames: ref_frames(r),
        })
        .collect()
}
fn read_displayed_areas(obj: &Object) -> Result<Vec<DisplayedArea>, Error> {
    let items = dio::sequence(obj, TAG_DISPLAYED_AREA_SELECTION_SEQ);
    let mut areas = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let tl = dio::ints(Some(item), TAG_DISPLAYED_AREA_TOP_LEFT).unwrap_or_default();
        let br = dio::ints(Some(item), TAG_DISPLAYED_AREA_BOTTOM_RIGHT).unwrap_or_default();
        if tl.len() != 2 || br.len() != 2 {
            return Err(invalid(format!(
                "Displayed Area item {i} requires two corners"
            )));
        }
        let mut area = DisplayedArea {
            defined: true,
            top_left_x: tl[0],
            top_left_y: tl[1],
            bottom_right_x: br[0],
            bottom_right_y: br[1],
            presentation_size_mode: dio::clean_string(item, TAG_PRESENTATION_SIZE_MODE),
            pixel_spacing: dio::floats(Some(item), TAG_PRESENTATION_PIXEL_SPACING)
                .unwrap_or_default(),
            referenced_images: read_direct_image_references(item),
            ..DisplayedArea::default()
        };
        area.pixel_aspect_ratio =
            dio::ints(Some(item), TAG_PRESENTATION_PIXEL_ASPECT_RATIO).unwrap_or_default();
        if let Some(values) = dio::floats(Some(item), TAG_PRESENTATION_PIXEL_MAGNIFY)
            && let Some(&v) = values.first()
        {
            area.magnification_ratio = v;
        }
        validate_displayed_area(&area)
            .map_err(|e| e.wrap(&format!("dicom/gsps: Displayed Area item {i}")))?;
        areas.push(area);
    }
    Ok(areas)
}
fn read_spatial_transform(obj: &Object) -> SpatialTransform {
    SpatialTransform {
        rotation_degrees: dio::int(obj, TAG_IMAGE_ROTATION),
        flip_horizontal: dio::clean_string(obj, TAG_IMAGE_HORIZONTAL_FLIP) == "Y",
    }
}
fn read_softcopy_vois(obj: &Object) -> Result<Vec<SoftcopyVoi>, Error> {
    let items = dio::sequence(obj, TAG_SOFTCOPY_VOI_LUT_SEQUENCE);
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let (transform, err) =
            crate::display::voi_from_object(item, crate::display::LutOptions::default());
        if let Some(err) = err {
            return Err(Error::other(format!(
                "dicom/gsps: Softcopy VOI item {i}: {err}"
            )));
        }
        let mut explanation = dio::clean_string(item, TAG_WINDOW_CENTER_WIDTH_EXPLANATION);
        if transform.lut.is_some()
            && let Some(lut_item) = dio::sequence(item, TAG_VOI_LUT_SEQUENCE).first()
        {
            explanation = dio::clean_string(lut_item, TAG_LUT_EXPLANATION);
        }
        out.push(SoftcopyVoi {
            window_center: transform.center,
            window_width: transform.width,
            explanation,
            function: transform.function,
            lut: transform.lut,
            referenced_images: read_direct_image_references(item),
        });
    }
    Ok(out)
}
fn read_graphic_annotations(obj: &Object) -> Vec<GraphicAnnotation> {
    let mut out = Vec::new();
    for (group_index, item) in dio::sequence(obj, TAG_GRAPHIC_ANNOTATION_SEQUENCE)
        .iter()
        .enumerate()
    {
        let layer = dio::clean_string(item, TAG_GRAPHIC_LAYER);
        let references = read_direct_image_references(item);
        let texts = read_text_objects(item, &layer);
        let graphics = read_graphic_objects(item, &layer);
        let compounds = read_compound_graphics(item, &layer);
        let empty = texts.is_empty() && graphics.is_empty() && compounds.is_empty();
        for mut annotation in texts.into_iter().chain(graphics).chain(compounds) {
            annotation.group_index = group_index as i64 + 1;
            annotation.referenced_images = references.clone();
            out.push(annotation);
        }
        if empty {
            out.push(GraphicAnnotation {
                layer_name: layer,
                group_index: group_index as i64 + 1,
                referenced_images: references.clone(),
                ..GraphicAnnotation::default()
            });
        }
    }
    out
}
fn read_text_objects(item: &Object, layer: &str) -> Vec<GraphicAnnotation> {
    let mut out = Vec::new();
    for text_item in dio::sequence(item, TAG_TEXT_OBJECT_SEQUENCE) {
        let mut annotation = GraphicAnnotation {
            layer_name: layer.into(),
            text: dio::clean_string(&text_item, TAG_UNFORMATTED_TEXT_VALUE),
            anchor_units: normalized_annotation_units(&dio::clean_string(
                &text_item,
                TAG_ANCHOR_POINT_ANNOTATION_UNITS,
            )),
            bounding_box: read_bounding_box(&text_item),
            anchor_visible: dio::clean_string(&text_item, TAG_ANCHOR_POINT_VISIBILITY) == "Y",
            ..GraphicAnnotation::default()
        };
        annotation.malformed_compound_link = !read_compound_link(&text_item, &mut annotation);
        let anchor = dio::floats(Some(&text_item), TAG_ANCHOR_POINT).unwrap_or_default();
        if anchor.len() >= 2 {
            annotation.anchor = Point2D::new(anchor[0], anchor[1]);
        }
        out.push(annotation);
    }
    out
}
fn read_graphic_objects(item: &Object, layer: &str) -> Vec<GraphicAnnotation> {
    let mut out = Vec::new();
    for graphic_item in dio::sequence(item, TAG_GRAPHIC_OBJECT_SEQUENCE) {
        let mut annotation = GraphicAnnotation {
            layer_name: layer.into(),
            graphic_type: normalized_graphic_type_or_polyline(&dio::clean_string(
                &graphic_item,
                TAG_GRAPHIC_TYPE,
            )),
            graphic_units: normalized_annotation_units(&dio::clean_string(
                &graphic_item,
                TAG_GRAPHIC_ANNOTATION_UNITS,
            )),
            filled: dio::clean_string(&graphic_item, TAG_GRAPHIC_FILLED) == "Y",
            ..GraphicAnnotation::default()
        };
        annotation.malformed_compound_link = !read_compound_link(&graphic_item, &mut annotation);
        let values = dio::floats(Some(&graphic_item), TAG_GRAPHIC_DATA).unwrap_or_default();
        annotation.polyline = values
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| Point2D::new(p[0], p[1]))
            .collect();
        out.push(annotation);
    }
    out
}
fn normalized_graphic_type(value: &str) -> String {
    to_upper(trim_space(value))
}
pub(crate) fn normalized_graphic_type_or_polyline(value: &str) -> String {
    let value = normalized_graphic_type(value);
    if value.is_empty() {
        GRAPHIC_TYPE_POLYLINE.into()
    } else {
        value
    }
}
pub(crate) fn normalized_annotation_units(value: &str) -> String {
    let value = to_upper(trim_space(value));
    if value.is_empty() {
        ANNOTATION_UNITS_PIXEL.into()
    } else {
        value
    }
}
pub(crate) fn flatten_points(points: &[Point2D]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.x, p.y]).collect()
}
pub(crate) fn graphic_layer_name(value: &str) -> String {
    if value.is_empty() {
        "ANNOTATIONS".into()
    } else {
        value.into()
    }
}
pub(crate) fn graphic_needs_filled_attribute(graphic_type: &str, points: &[Point2D]) -> bool {
    match graphic_type {
        GRAPHIC_TYPE_CIRCLE | GRAPHIC_TYPE_ELLIPSE => !points.is_empty(),
        GRAPHIC_TYPE_POLYLINE | GRAPHIC_TYPE_INTERPOLATED => closed_polyline(points),
        _ => false,
    }
}
pub(crate) fn closed_polyline(points: &[Point2D]) -> bool {
    if points.len() < 2 {
        return false;
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    first.x == last.x && first.y == last.y
}
pub(crate) fn flip_value(enabled: bool) -> &'static str {
    if enabled { "Y" } else { "N" }
}
fn presentation_lut_shape(value: &str) -> &str {
    if value.is_empty() {
        PRESENTATION_LUT_IDENTITY
    } else {
        value
    }
}
