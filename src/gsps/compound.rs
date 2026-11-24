use super::model::{MAX_TEXT_LENGTH, valid_cs};
use super::{
    ANNOTATION_UNITS_DISPLAY, ANNOTATION_UNITS_PIXEL, DisplayedArea, Error, GraphicAnnotation,
    ImageState, Point2D, ReferencedImage, State, TAG_GRAPHIC_DATA, TAG_GRAPHIC_DIMENSIONS,
    TAG_GRAPHIC_FILLED, TAG_NUMBER_OF_GRAPHIC_POINTS, flatten_points, flip_value,
    graphic_layer_name, invalid, normalized_annotation_units, references_apply, unit_interval,
};
use crate::deid::trim_space;
use crate::derivedio as dio;
use crate::object::{DataSet, Element, Object, Tag, Value, Vr, element_length};
use std::collections::HashMap;
pub const COMPOUND_TYPE_MULTILINE: &str = "MULTILINE";
pub const COMPOUND_TYPE_INFINITE_LINE: &str = "INFINITELINE";
pub const COMPOUND_TYPE_CUTLINE: &str = "CUTLINE";
pub const COMPOUND_TYPE_RANGE_LINE: &str = "RANGELINE";
pub const COMPOUND_TYPE_RULER: &str = "RULER";
pub const COMPOUND_TYPE_AXIS: &str = "AXIS";
pub const COMPOUND_TYPE_CROSSHAIR: &str = "CROSSHAIR";
pub const COMPOUND_TYPE_ARROW: &str = "ARROW";
pub const COMPOUND_TYPE_RECTANGLE: &str = "RECTANGLE";
pub const COMPOUND_TYPE_ELLIPSE: &str = "ELLIPSE";
pub(crate) const MAX_WRITTEN_COMPOUND_POINTS: usize = ((1 << 16) - 2) / 8;
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
pub(crate) const TAG_COMPOUND_GRAPHIC_SEQUENCE: Tag = t(0x0070, 0x0209);
const TAG_COMPOUND_GRAPHIC_INSTANCE_ID: Tag = t(0x0070, 0x0226);
const TAG_COMPOUND_ROTATION_ANGLE: Tag = t(0x0070, 0x0230);
const TAG_GRAPHIC_GROUP_SEQUENCE: Tag = t(0x0070, 0x0234);
const TAG_GRAPHIC_GROUP_LABEL: Tag = t(0x0070, 0x0207);
const TAG_GRAPHIC_GROUP_DESCRIPTION: Tag = t(0x0070, 0x0208);
const TAG_COMPOUND_GAP_LENGTH: Tag = t(0x0070, 0x0261);
const TAG_DIAMETER_OF_VISIBILITY: Tag = t(0x0070, 0x0262);
const TAG_COMPOUND_ROTATION_POINT: Tag = t(0x0070, 0x0273);
const TAG_TICK_ALIGNMENT: Tag = t(0x0070, 0x0274);
const TAG_SHOW_TICK_LABEL: Tag = t(0x0070, 0x0278);
const TAG_TICK_LABEL_ALIGNMENT: Tag = t(0x0070, 0x0279);
const TAG_COMPOUND_GRAPHIC_UNITS: Tag = t(0x0070, 0x0282);
const TAG_MAJOR_TICKS_SEQUENCE: Tag = t(0x0070, 0x0287);
const TAG_TICK_POSITION: Tag = t(0x0070, 0x0288);
const TAG_TICK_LABEL: Tag = t(0x0070, 0x0289);
const TAG_COMPOUND_GRAPHIC_TYPE: Tag = t(0x0070, 0x0294);
const TAG_GRAPHIC_GROUP_ID: Tag = t(0x0070, 0x0295);
const TAG_FONT_NAME: Tag = t(0x0070, 0x0227);
const TAG_FONT_NAME_TYPE: Tag = t(0x0070, 0x0228);
const TAG_CSS_FONT: Tag = t(0x0070, 0x0229);
const TAG_TEXT_STYLE: Tag = t(0x0070, 0x0231);
const TAG_LINE_STYLE: Tag = t(0x0070, 0x0232);
const TAG_FILL_STYLE: Tag = t(0x0070, 0x0233);
const TAG_TEXT_COLOR: Tag = t(0x0070, 0x0241);
const TAG_HORIZONTAL: Tag = t(0x0070, 0x0242);
const TAG_VERTICAL: Tag = t(0x0070, 0x0243);
const TAG_SHADOW_STYLE: Tag = t(0x0070, 0x0244);
const TAG_SHADOW_OFFSET_X: Tag = t(0x0070, 0x0245);
const TAG_SHADOW_OFFSET_Y: Tag = t(0x0070, 0x0246);
const TAG_SHADOW_COLOR: Tag = t(0x0070, 0x0247);
const TAG_UNDERLINED: Tag = t(0x0070, 0x0248);
const TAG_BOLD: Tag = t(0x0070, 0x0249);
const TAG_ITALIC: Tag = t(0x0070, 0x0250);
const TAG_PATTERN_ON: Tag = t(0x0070, 0x0251);
const TAG_PATTERN_OFF: Tag = t(0x0070, 0x0252);
const TAG_LINE_THICKNESS: Tag = t(0x0070, 0x0253);
const TAG_LINE_DASHING: Tag = t(0x0070, 0x0254);
const TAG_LINE_PATTERN: Tag = t(0x0070, 0x0255);
const TAG_FILL_PATTERN: Tag = t(0x0070, 0x0256);
const TAG_FILL_MODE: Tag = t(0x0070, 0x0257);
const TAG_SHADOW_OPACITY: Tag = t(0x0070, 0x0258);
const TAG_ON_OPACITY: Tag = t(0x0070, 0x0284);
const TAG_OFF_OPACITY: Tag = t(0x0070, 0x0285);
const FILL_PATTERN_BYTES: usize = 128;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundGraphic {
    pub compound_type: String,
    pub units: String,
    pub points: Vec<Point2D>,
    pub filled: Option<bool>,
    pub rotation_angle: Option<f64>,
    pub rotation_point: Option<Point2D>,
    pub gap_length: Option<f64>,
    pub diameter_of_visibility: Option<f64>,
    pub tick_alignment: String,
    pub tick_label_alignment: String,
    pub show_tick_label: Option<bool>,
    pub major_ticks: Vec<CompoundTick>,
    pub line_style: Option<CompoundLineStyle>,
    pub fill_style: Option<CompoundFillStyle>,
    pub text_style: Option<CompoundTextStyle>,
    pub(crate) malformed: bool,
}
impl CompoundGraphic {
    pub fn malformed(&self) -> bool {
        self.malformed
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundTick {
    pub position: f64,
    pub label: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicGroup {
    pub id: u32,
    pub label: String,
    pub description: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundLineStyle {
    pub on_color: Option<[u16; 3]>,
    pub off_color: Option<[u16; 3]>,
    pub shadow_color: Option<[u16; 3]>,
    pub on_opacity: Option<f64>,
    pub off_opacity: Option<f64>,
    pub thickness: Option<f64>,
    pub shadow_offset_x: Option<f64>,
    pub shadow_offset_y: Option<f64>,
    pub shadow_opacity: Option<f64>,
    pub dashing: String,
    pub shadow: String,
    pub pattern: Option<u32>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundFillStyle {
    pub on_color: Option<[u16; 3]>,
    pub off_color: Option<[u16; 3]>,
    pub on_opacity: Option<f64>,
    pub off_opacity: Option<f64>,
    pub mode: String,
    pub pattern: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompoundTextStyle {
    pub font_name: String,
    pub font_name_type: String,
    pub css_font_name: String,
    pub horizontal_alignment: String,
    pub vertical_alignment: String,
    pub shadow: String,
    pub color: Option<[u16; 3]>,
    pub shadow_color: Option<[u16; 3]>,
    pub shadow_offset_x: Option<f64>,
    pub shadow_offset_y: Option<f64>,
    pub shadow_opacity: Option<f64>,
    pub underlined: Option<bool>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}
fn standard_compound_type(value: &str) -> bool {
    matches!(
        value,
        COMPOUND_TYPE_MULTILINE
            | COMPOUND_TYPE_INFINITE_LINE
            | COMPOUND_TYPE_CUTLINE
            | COMPOUND_TYPE_RANGE_LINE
            | COMPOUND_TYPE_RULER
            | COMPOUND_TYPE_AXIS
            | COMPOUND_TYPE_CROSSHAIR
            | COMPOUND_TYPE_ARROW
            | COMPOUND_TYPE_RECTANGLE
            | COMPOUND_TYPE_ELLIPSE
    )
}
pub(crate) fn read_compound_graphics(item: &Object, layer: &str) -> Vec<GraphicAnnotation> {
    let mut out = Vec::new();
    for obj in dio::sequence(item, TAG_COMPOUND_GRAPHIC_SEQUENCE) {
        let mut annotation = GraphicAnnotation {
            layer_name: layer.into(),
            ..GraphicAnnotation::default()
        };
        let link_ok = read_compound_link(&obj, &mut annotation);
        let mut c = CompoundGraphic {
            malformed: !link_ok || !annotation.compound_graphic_id_present(),
            ..CompoundGraphic::default()
        };
        let mut ok;
        (c.compound_type, ok) = read_compound_string(&obj, TAG_COMPOUND_GRAPHIC_TYPE, Vr::CS);
        c.malformed |= !ok;
        (c.units, ok) = read_compound_string(&obj, TAG_COMPOUND_GRAPHIC_UNITS, Vr::CS);
        c.malformed |= !ok;
        (c.points, ok) = read_compound_points(&obj);
        c.malformed |= !ok;
        (c.filled, ok) = read_compound_bool(&obj, TAG_GRAPHIC_FILLED);
        c.malformed |= !ok;
        (c.rotation_angle, ok) = read_compound_float(&obj, TAG_COMPOUND_ROTATION_ANGLE, Vr::FD);
        c.malformed |= !ok;
        (c.rotation_point, ok) = read_compound_rotation_point(&obj);
        c.malformed |= !ok;
        (c.gap_length, ok) = read_compound_float(&obj, TAG_COMPOUND_GAP_LENGTH, Vr::FL);
        c.malformed |= !ok;
        (c.diameter_of_visibility, ok) =
            read_compound_float(&obj, TAG_DIAMETER_OF_VISIBILITY, Vr::FL);
        c.malformed |= !ok;
        (c.tick_alignment, ok) = read_compound_string(&obj, TAG_TICK_ALIGNMENT, Vr::CS);
        c.malformed |= !ok;
        (c.tick_label_alignment, ok) = read_compound_string(&obj, TAG_TICK_LABEL_ALIGNMENT, Vr::CS);
        c.malformed |= !ok;
        (c.show_tick_label, ok) = read_compound_bool(&obj, TAG_SHOW_TICK_LABEL);
        c.malformed |= !ok;
        (c.major_ticks, ok) = read_compound_ticks(&obj);
        c.malformed |= !ok;
        if !read_compound_styles(&obj, &mut c) {
            c.malformed = true;
        }
        annotation.compound = Some(c);
        out.push(annotation);
    }
    out
}
fn read_compound_rotation_point(obj: &Object) -> (Option<Point2D>, bool) {
    let Some(e) = obj.get(TAG_COMPOUND_ROTATION_POINT) else {
        return (None, true);
    };
    if e.vr() != Vr::FL || element_length(e) != 8 {
        return (None, false);
    }
    match dio::floats(Some(obj), TAG_COMPOUND_ROTATION_POINT) {
        Some(xy) if xy.len() == 2 => (Some(Point2D::new(xy[0], xy[1])), true),
        _ => (None, false),
    }
}
fn read_compound_points(obj: &Object) -> (Vec<Point2D>, bool) {
    let (dimensions, dimensions_ok) = read_compound_int(obj, TAG_GRAPHIC_DIMENSIONS, Vr::US);
    let (count, count_ok) = read_compound_int(obj, TAG_NUMBER_OF_GRAPHIC_POINTS, Vr::US);
    let e = obj.get(TAG_GRAPHIC_DATA);
    let (Some(count), Some(e)) = (count, e) else {
        return (Vec::new(), false);
    };
    if !dimensions_ok
        || dimensions != Some(2)
        || !count_ok
        || count <= 0
        || e.vr() != Vr::FL
        || i64::from(element_length(e)) != count * 8
    {
        return (Vec::new(), false);
    }
    let raw = dio::floats(Some(obj), TAG_GRAPHIC_DATA).unwrap_or_default();
    if raw.len() as i64 != count * 2 {
        return (Vec::new(), false);
    }
    (
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|p| Point2D::new(p[0], p[1]))
            .collect(),
        true,
    )
}
fn read_compound_ticks(obj: &Object) -> (Vec<CompoundTick>, bool) {
    let Some(e) = obj.get(TAG_MAJOR_TICKS_SEQUENCE) else {
        return (Vec::new(), true);
    };
    let items = dio::sequence(obj, TAG_MAJOR_TICKS_SEQUENCE);
    if e.vr() != Vr::SQ || items.is_empty() {
        return (Vec::new(), false);
    }
    let mut ticks = Vec::new();
    let mut ok = true;
    for tick in &items {
        let (position, position_ok) = read_compound_float(tick, TAG_TICK_POSITION, Vr::FL);
        let (label, label_ok) = read_compound_string(tick, TAG_TICK_LABEL, Vr::SH);
        match position {
            Some(position) if position_ok && label_ok && !label.is_empty() => {
                ticks.push(CompoundTick { position, label })
            }
            _ => ok = false,
        }
    }
    (ticks, ok)
}
pub(crate) fn read_compound_link(obj: &Object, a: &mut GraphicAnnotation) -> bool {
    let (id, id_ok) = read_compound_u32(obj, TAG_COMPOUND_GRAPHIC_INSTANCE_ID);
    let (group, group_ok) = read_compound_u32(obj, TAG_GRAPHIC_GROUP_ID);
    if let Some(id) = id {
        a.compound_graphic_id = id;
        a.has_compound_graphic_id = true;
    }
    if let Some(group) = group {
        a.graphic_group_id = group;
        a.has_graphic_group_id = true;
    }
    id_ok && group_ok
}
fn read_compound_string(obj: &Object, tag: Tag, vr: Vr) -> (String, bool) {
    let Some(e) = obj.get(tag) else {
        return (String::new(), true);
    };
    let max_bytes: u32 = match vr {
        Vr::CS => 16,
        Vr::SH => 16 * 8,
        Vr::LO => 64 * 8,
        Vr::ST => (MAX_TEXT_LENGTH * 8) as u32,
        _ => 0,
    };
    if e.vr() != vr || element_length(e) > max_bytes {
        return (String::new(), false);
    }
    match obj.lookup_strings(tag) {
        Ok(values) if values.len() == 1 => {
            if vr == Vr::CS {
                (trim_space(&values[0]).to_string(), true)
            } else {
                (values[0].clone(), true)
            }
        }
        _ => (String::new(), false),
    }
}
fn read_compound_int(obj: &Object, tag: Tag, vr: Vr) -> (Option<i64>, bool) {
    let Some(e) = obj.get(tag) else {
        return (None, true);
    };
    let width = if vr == Vr::US { 2 } else { 4 };
    if e.vr() != vr || element_length(e) != width {
        return (None, false);
    }
    match obj.get_ints(tag) {
        Ok(values) if values.len() == 1 => (Some(values[0]), true),
        _ => (None, false),
    }
}
fn read_compound_u32(obj: &Object, tag: Tag) -> (Option<u32>, bool) {
    let (value, ok) = read_compound_int(obj, tag, Vr::UL);
    match value {
        None => (None, ok),
        Some(v) if v < 0 || v > u32::MAX as i64 => (None, false),
        Some(v) => (Some(v as u32), true),
    }
}
fn read_compound_float(obj: &Object, tag: Tag, vr: Vr) -> (Option<f64>, bool) {
    let Some(e) = obj.get(tag) else {
        return (None, true);
    };
    let width = if vr == Vr::FD { 8 } else { 4 };
    if e.vr() != vr || element_length(e) != width {
        return (None, false);
    }
    match obj.get_floats(tag) {
        Ok(values) if values.len() == 1 => (Some(values[0]), true),
        _ => (None, false),
    }
}
fn read_compound_bool(obj: &Object, tag: Tag) -> (Option<bool>, bool) {
    if !obj.has(tag) {
        return (None, true);
    }
    let (value, ok) = read_compound_string(obj, tag, Vr::CS);
    if !ok || (value != "Y" && value != "N") {
        return (None, false);
    }
    (Some(value == "Y"), true)
}
pub(crate) fn compound_link_elements(a: &GraphicAnnotation) -> Vec<Element> {
    let mut elements = Vec::new();
    if a.compound_graphic_id_present() {
        elements.push(dio::ul(
            TAG_COMPOUND_GRAPHIC_INSTANCE_ID,
            &[a.compound_graphic_id],
        ));
    }
    if a.graphic_group_id_present() {
        elements.push(dio::ul(TAG_GRAPHIC_GROUP_ID, &[a.graphic_group_id]));
    }
    elements
}
pub(crate) fn compound_graphic_item(a: &GraphicAnnotation) -> DataSet {
    let c = a.compound.as_ref().expect("compound annotation");
    let mut elements = compound_link_elements(a);
    elements.extend([
        dio::cs(TAG_COMPOUND_GRAPHIC_TYPE, &c.compound_type),
        dio::cs(TAG_COMPOUND_GRAPHIC_UNITS, &c.units),
        dio::us(TAG_GRAPHIC_DIMENSIONS, &[2]),
        dio::us(TAG_NUMBER_OF_GRAPHIC_POINTS, &[c.points.len() as u16]),
        dio::fl(TAG_GRAPHIC_DATA, &flatten_points(&c.points)),
    ]);
    if let Some(angle) = c.rotation_angle {
        elements.push(dio::fd(TAG_COMPOUND_ROTATION_ANGLE, &[angle]));
    }
    if let Some(p) = c.rotation_point {
        elements.push(dio::fl(TAG_COMPOUND_ROTATION_POINT, &[p.x, p.y]));
    }
    for (tag, value) in [
        (TAG_COMPOUND_GAP_LENGTH, c.gap_length),
        (TAG_DIAMETER_OF_VISIBILITY, c.diameter_of_visibility),
    ] {
        if let Some(v) = value {
            elements.push(dio::fl(tag, &[v]));
        }
    }
    if let Some(filled) = c.filled {
        elements.push(dio::cs(TAG_GRAPHIC_FILLED, flip_value(filled)));
    }
    if let Some(show) = c.show_tick_label {
        elements.push(dio::cs(TAG_SHOW_TICK_LABEL, flip_value(show)));
    }
    if !c.tick_alignment.is_empty() {
        elements.push(dio::cs(TAG_TICK_ALIGNMENT, &c.tick_alignment));
    }
    if !c.tick_label_alignment.is_empty() {
        elements.push(dio::cs(TAG_TICK_LABEL_ALIGNMENT, &c.tick_label_alignment));
    }
    if !c.major_ticks.is_empty() {
        let ticks = c
            .major_ticks
            .iter()
            .map(|tick| {
                dio::data_set(vec![
                    dio::fl(TAG_TICK_POSITION, &[tick.position]),
                    dio::str_elem(TAG_TICK_LABEL, Vr::SH, &tick.label),
                ])
            })
            .collect();
        elements.push(dio::seq(TAG_MAJOR_TICKS_SEQUENCE, ticks));
    }
    elements.extend(compound_style_elements(c));
    dio::data_set(elements)
}
fn float32_eq(a: f64, b: f64) -> bool {
    (a as f32) == (b as f32)
}
pub(crate) fn validate_compound_graphic(c: Option<&CompoundGraphic>) -> Result<(), Error> {
    let incomplete = || invalid("incomplete Compound Graphic");
    let Some(c) = c else {
        return Err(incomplete());
    };
    if c.malformed
        || !valid_cs(&c.compound_type)
        || (c.units != ANNOTATION_UNITS_PIXEL && c.units != ANNOTATION_UNITS_DISPLAY)
        || c.points.is_empty()
    {
        return Err(incomplete());
    }
    for p in &c.points {
        if !valid_compound_point(*p, &c.units) {
            return Err(invalid("invalid Compound Graphic point"));
        }
    }
    match c.compound_type.as_str() {
        COMPOUND_TYPE_MULTILINE => {
            if c.points.len() % 2 != 0 {
                return Err(invalid("MULTILINE requires pairs of points"));
            }
        }
        COMPOUND_TYPE_CROSSHAIR => {
            if c.points.len() != 1 {
                return Err(invalid("CROSSHAIR requires one point"));
            }
        }
        COMPOUND_TYPE_RECTANGLE
        | COMPOUND_TYPE_ELLIPSE
        | COMPOUND_TYPE_INFINITE_LINE
        | COMPOUND_TYPE_CUTLINE
        | COMPOUND_TYPE_RANGE_LINE
        | COMPOUND_TYPE_RULER
        | COMPOUND_TYPE_AXIS
        | COMPOUND_TYPE_ARROW => {
            if c.points.len() != 2 {
                return Err(invalid("Compound Graphic requires two points"));
            }
            if float32_eq(c.points[0].x, c.points[1].x) && float32_eq(c.points[0].y, c.points[1].y)
            {
                return Err(invalid("Compound Graphic endpoints must differ"));
            }
        }
        _ => {}
    }
    if c.compound_type == COMPOUND_TYPE_RECTANGLE || c.compound_type == COMPOUND_TYPE_ELLIPSE {
        if c.filled.is_none() || (c.filled == Some(true) && c.fill_style.is_none()) {
            return Err(invalid(
                "rectangle or ellipse is missing its fill attributes",
            ));
        }
        if c.points[1].x <= c.points[0].x || c.points[1].y <= c.points[0].y {
            return Err(invalid("invalid Compound Graphic bounding rectangle"));
        }
    }
    let gap_line =
        c.compound_type == COMPOUND_TYPE_CUTLINE || c.compound_type == COMPOUND_TYPE_INFINITE_LINE;
    if let Some(angle) = c.rotation_angle
        && (!angle.is_finite() || !(0.0..=360.0).contains(&angle))
    {
        return Err(invalid("invalid Compound Graphic rotation"));
    }
    if ((c.rotation_angle.is_some() || gap_line) && c.rotation_point.is_none())
        || c.rotation_point
            .is_some_and(|p| !valid_compound_point(p, &c.units))
    {
        return Err(invalid(
            "missing or invalid Compound Graphic rotation point",
        ));
    }
    if ((gap_line || c.compound_type == COMPOUND_TYPE_CROSSHAIR) && c.gap_length.is_none())
        || c.gap_length
            .is_some_and(|g| !valid_compound_fl(g) || g < 0.0)
    {
        return Err(invalid("missing or invalid Compound Graphic gap"));
    }
    if (c.compound_type == COMPOUND_TYPE_CROSSHAIR && c.diameter_of_visibility.is_none())
        || c.diameter_of_visibility
            .is_some_and(|d| !valid_compound_fl(d) || d <= 0.0)
    {
        return Err(invalid("missing or invalid CROSSHAIR diameter"));
    }
    validate_compound_ticks(c)?;
    validate_compound_styles(c)
}
fn valid_compound_point(p: Point2D, units: &str) -> bool {
    valid_compound_fl(p.x)
        && valid_compound_fl(p.y)
        && p.x >= 0.0
        && p.y >= 0.0
        && (units != ANNOTATION_UNITS_DISPLAY || (p.x <= 1.0 && p.y <= 1.0))
}
fn valid_compound_fl(value: f64) -> bool {
    value.is_finite() && value.abs() <= f32::MAX as f64
}
fn validate_compound_ticks(c: &CompoundGraphic) -> Result<(), Error> {
    let ty = c.compound_type.as_str();
    if (ty == COMPOUND_TYPE_AXIS || ty == COMPOUND_TYPE_CROSSHAIR || ty == COMPOUND_TYPE_RULER)
        && (c.show_tick_label.is_none()
            || !matches!(c.tick_alignment.as_str(), "TOP" | "BOTTOM" | "CENTER")
            || !matches!(c.tick_label_alignment.as_str(), "TOP" | "BOTTOM")
            || (ty == COMPOUND_TYPE_CROSSHAIR && c.tick_alignment != "CENTER"))
    {
        return Err(invalid(
            "invalid Compound Graphic tick alignment or visibility",
        ));
    }
    if ty == COMPOUND_TYPE_AXIS && c.major_ticks.len() < 2 {
        return Err(invalid("AXIS requires at least two major ticks"));
    }
    for tick in &c.major_ticks {
        if !unit_interval(tick.position)
            || trim_space(&tick.label).is_empty()
            || tick.label.chars().count() > 16
            || tick.label.contains('\\')
        {
            return Err(invalid("invalid Compound Graphic major tick"));
        }
    }
    Ok(())
}
pub(crate) fn compound_index_by_id(items: &[GraphicAnnotation]) -> HashMap<u32, usize> {
    let mut indexes = HashMap::new();
    for (i, a) in items.iter().enumerate() {
        if a.compound.is_some() {
            indexes.insert(a.compound_graphic_id, i);
        }
    }
    indexes
}
pub(crate) fn validate_compound_links(items: &[GraphicAnnotation]) -> Result<(), Error> {
    let mut indexes: HashMap<u32, usize> = HashMap::new();
    for (i, a) in items.iter().enumerate() {
        if a.malformed_compound_link {
            return Err(invalid(format!(
                "annotation {i} has invalid graphic linkage"
            )));
        }
        if a.compound.is_none() {
            continue;
        }
        if !a.compound_graphic_id_present() {
            return Err(invalid("Compound Graphic requires its instance ID"));
        }
        if indexes.contains_key(&a.compound_graphic_id) {
            return Err(invalid(format!(
                "duplicate Compound Graphic instance ID {}",
                a.compound_graphic_id
            )));
        }
        indexes.insert(a.compound_graphic_id, i);
    }
    let mut alternatives: HashMap<u32, bool> = HashMap::new();
    for (i, a) in items.iter().enumerate() {
        if a.compound.is_some() || !a.compound_graphic_id_present() {
            continue;
        }
        let Some(&compound_index) = indexes.get(&a.compound_graphic_id) else {
            return Err(invalid(format!(
                "annotation {i} has no corresponding Compound Graphic"
            )));
        };
        let compound = &items[compound_index];
        if graphic_layer_name(&a.layer_name) != graphic_layer_name(&compound.layer_name)
            || a.group_index.max(0) != compound.group_index.max(0)
            || !a.same_graphic_group(compound)
            || a.referenced_images != compound.referenced_images
        {
            return Err(invalid(format!(
                "annotation {i} alternative does not share its compound's scope"
            )));
        }
        if !a.text.is_empty() || !a.polyline.is_empty() {
            alternatives.insert(a.compound_graphic_id, true);
        }
    }
    for a in items {
        if a.compound.is_some() && !alternatives.contains_key(&a.compound_graphic_id) {
            return Err(invalid(format!(
                "Compound Graphic {} requires a text or graphic alternative",
                a.compound_graphic_id
            )));
        }
    }
    Ok(())
}
pub(crate) fn read_graphic_groups(obj: &Object) -> Vec<GraphicGroup> {
    let mut groups = Vec::new();
    for item in dio::sequence(obj, TAG_GRAPHIC_GROUP_SEQUENCE) {
        let (id, ok) = read_compound_u32(&item, TAG_GRAPHIC_GROUP_ID);
        let (label, label_ok) = read_compound_string(&item, TAG_GRAPHIC_GROUP_LABEL, Vr::LO);
        let (description, description_ok) =
            read_compound_string(&item, TAG_GRAPHIC_GROUP_DESCRIPTION, Vr::ST);
        if let Some(id) = id
            && ok
            && label_ok
            && !label.is_empty()
            && description_ok
        {
            groups.push(GraphicGroup {
                id,
                label,
                description,
            });
        }
    }
    groups
}
pub(crate) fn graphic_group_sequence(state: &State) -> Result<Option<Element>, Error> {
    let mut groups: HashMap<u32, bool> = HashMap::new();
    let mut sets = Vec::new();
    for group in &state.graphic_groups {
        if trim_space(&group.label).is_empty()
            || group.label.chars().count() > 64
            || group.label.contains('\\')
            || group.description.chars().count() > MAX_TEXT_LENGTH
        {
            return Err(invalid("invalid Graphic Group label or description"));
        }
        if groups.contains_key(&group.id) {
            return Err(invalid(format!("duplicate Graphic Group ID {}", group.id)));
        }
        groups.insert(group.id, true);
        let mut attrs = vec![
            dio::ul(TAG_GRAPHIC_GROUP_ID, &[group.id]),
            dio::lo(TAG_GRAPHIC_GROUP_LABEL, &group.label),
        ];
        if !group.description.is_empty() {
            attrs.push(dio::str_elem(
                TAG_GRAPHIC_GROUP_DESCRIPTION,
                Vr::ST,
                &group.description,
            ));
        }
        sets.push(dio::data_set(attrs));
    }
    for a in &state.graphic_annotations {
        if a.graphic_group_id_present() && !groups.contains_key(&a.graphic_group_id) {
            return Err(invalid(format!(
                "undefined Graphic Group ID {}",
                a.graphic_group_id
            )));
        }
    }
    if sets.is_empty() {
        return Ok(None);
    }
    Ok(Some(dio::seq(TAG_GRAPHIC_GROUP_SEQUENCE, sets)))
}
struct StyleReader {
    item: Option<Object>,
    ok: bool,
}
fn read_style_item(obj: &Object, tag: Tag) -> (StyleReader, bool) {
    let Some(element) = obj.get(tag) else {
        return (
            StyleReader {
                item: None,
                ok: false,
            },
            true,
        );
    };
    let valid = matches!(&element.value, Value::Sequence(items) if items.len() == 1);
    if element.vr() != Vr::SQ || !valid {
        return (
            StyleReader {
                item: None,
                ok: false,
            },
            false,
        );
    }
    let item = obj
        .get_sequence(tag)
        .and_then(|items| items.into_iter().next());
    (StyleReader { item, ok: true }, true)
}
impl StyleReader {
    fn obj(&self) -> &Object {
        self.item.as_ref().expect("style item")
    }
    fn text(&mut self, tag: Tag, vr: Vr) -> String {
        let max_bytes = if vr == Vr::CS { 16 } else { 512 };
        let item = self.item.as_ref().expect("style item");
        if let Some(e) = item.get(tag) {
            if element_length(e) > max_bytes {
                self.ok = false;
                return String::new();
            }
            if tag == TAG_FONT_NAME && element_length(e) == 0 && e.vr() == vr {
                return String::new();
            }
        }
        let (value, ok) = read_compound_string(item, tag, vr);
        self.ok = self.ok && ok;
        value
    }
    fn color(&mut self, tag: Tag) -> Option<[u16; 3]> {
        let item = self.obj();
        let e = item.get(tag)?;
        if e.vr() != Vr::US || element_length(e) != 6 {
            self.ok = false;
            return None;
        }
        match item.get_ints(tag) {
            Ok(v) if v.len() == 3 => Some([v[0] as u16, v[1] as u16, v[2] as u16]),
            _ => {
                self.ok = false;
                None
            }
        }
    }
    fn float(&mut self, tag: Tag) -> Option<f64> {
        let (value, ok) = read_compound_float(self.obj(), tag, Vr::FL);
        self.ok = self.ok && ok;
        value
    }
    fn word(&mut self, tag: Tag) -> Option<u32> {
        let (value, ok) = read_compound_u32(self.obj(), tag);
        self.ok = self.ok && ok;
        value
    }
    fn yn(&mut self, tag: Tag) -> Option<bool> {
        let (value, ok) = read_compound_bool(self.obj(), tag);
        self.ok = self.ok && ok;
        value
    }
    fn pattern(&mut self) -> Vec<u8> {
        let item = self.obj();
        let Some(e) = item.get(TAG_FILL_PATTERN) else {
            return Vec::new();
        };
        match e.raw() {
            Some(raw) if e.vr() == Vr::OB && raw.len() == FILL_PATTERN_BYTES => raw.to_vec(),
            _ => {
                self.ok = false;
                Vec::new()
            }
        }
    }
}
fn read_compound_styles(item: &Object, c: &mut CompoundGraphic) -> bool {
    let (mut line, mut line_ok) = read_style_item(item, TAG_LINE_STYLE);
    if line.item.is_some() {
        c.line_style = Some(CompoundLineStyle {
            on_color: line.color(TAG_PATTERN_ON),
            off_color: line.color(TAG_PATTERN_OFF),
            on_opacity: line.float(TAG_ON_OPACITY),
            off_opacity: line.float(TAG_OFF_OPACITY),
            thickness: line.float(TAG_LINE_THICKNESS),
            dashing: line.text(TAG_LINE_DASHING, Vr::CS),
            pattern: line.word(TAG_LINE_PATTERN),
            shadow: line.text(TAG_SHADOW_STYLE, Vr::CS),
            shadow_color: line.color(TAG_SHADOW_COLOR),
            shadow_opacity: line.float(TAG_SHADOW_OPACITY),
            shadow_offset_x: line.float(TAG_SHADOW_OFFSET_X),
            shadow_offset_y: line.float(TAG_SHADOW_OFFSET_Y),
        });
        line_ok = line.ok;
    }
    let (mut fill, mut fill_ok) = read_style_item(item, TAG_FILL_STYLE);
    if fill.item.is_some() {
        c.fill_style = Some(CompoundFillStyle {
            on_color: fill.color(TAG_PATTERN_ON),
            off_color: fill.color(TAG_PATTERN_OFF),
            on_opacity: fill.float(TAG_ON_OPACITY),
            off_opacity: fill.float(TAG_OFF_OPACITY),
            mode: fill.text(TAG_FILL_MODE, Vr::CS),
            pattern: fill.pattern(),
        });
        fill_ok = fill.ok;
    }
    let (mut text, mut text_ok) = read_style_item(item, TAG_TEXT_STYLE);
    if text.item.is_some() {
        c.text_style = Some(CompoundTextStyle {
            font_name: text.text(TAG_FONT_NAME, Vr::LO),
            font_name_type: text.text(TAG_FONT_NAME_TYPE, Vr::CS),
            css_font_name: text.text(TAG_CSS_FONT, Vr::LO),
            color: text.color(TAG_TEXT_COLOR),
            horizontal_alignment: text.text(TAG_HORIZONTAL, Vr::CS),
            vertical_alignment: text.text(TAG_VERTICAL, Vr::CS),
            shadow: text.text(TAG_SHADOW_STYLE, Vr::CS),
            shadow_color: text.color(TAG_SHADOW_COLOR),
            shadow_offset_x: text.float(TAG_SHADOW_OFFSET_X),
            shadow_offset_y: text.float(TAG_SHADOW_OFFSET_Y),
            shadow_opacity: text.float(TAG_SHADOW_OPACITY),
            underlined: text.yn(TAG_UNDERLINED),
            bold: text.yn(TAG_BOLD),
            italic: text.yn(TAG_ITALIC),
        });
        text_ok = text.ok;
    }
    line_ok && fill_ok && text_ok && validate_compound_styles(c).is_ok()
}
fn validate_compound_styles(c: &CompoundGraphic) -> Result<(), Error> {
    if let Some(line) = &c.line_style {
        match line.thickness {
            Some(th) if valid_style_float(Some(th)) && th > 0.0 && (th as f32) != 0.0 => {}
            _ => return Err(invalid("line style requires a positive FL thickness")),
        }
        match line.dashing.as_str() {
            "" | "SOLID" => {}
            "DASHED" => {
                if line.pattern.is_none() {
                    return Err(invalid("dashed line style requires a line pattern"));
                }
            }
            _ => return Err(invalid("unknown line dashing style")),
        }
        if !valid_style_opacity(line.on_opacity)
            || !valid_style_opacity(line.off_opacity)
            || !valid_style_shadow(
                &line.shadow,
                line.shadow_color,
                line.shadow_offset_x,
                line.shadow_offset_y,
                line.shadow_opacity,
            )
        {
            return Err(invalid("invalid line style opacity or shadow"));
        }
    }
    if let Some(fill) = &c.fill_style {
        match fill.mode.as_str() {
            "" | "SOLID" => {}
            "STIPPELED" => {
                if fill.pattern.len() != FILL_PATTERN_BYTES {
                    return Err(invalid("stippled fill requires a 128-byte fill pattern"));
                }
            }
            _ => return Err(invalid("unknown fill mode")),
        }
        if (!fill.pattern.is_empty() && fill.pattern.len() != FILL_PATTERN_BYTES)
            || !valid_style_opacity(fill.on_opacity)
            || !valid_style_opacity(fill.off_opacity)
        {
            return Err(invalid("invalid fill pattern or opacity"));
        }
    }
    if let Some(text) = &c.text_style {
        if !valid_style_font_name(&text.font_name)
            || !valid_style_font_name(&text.css_font_name)
            || (!text.font_name.is_empty() && text.font_name_type.is_empty())
            || (!text.font_name_type.is_empty() && !valid_cs(&text.font_name_type))
        {
            return Err(invalid("invalid text style font specification"));
        }
        if !matches!(
            text.horizontal_alignment.as_str(),
            "" | "LEFT" | "CENTER" | "RIGHT"
        ) {
            return Err(invalid("invalid text style horizontal alignment"));
        }
        if !matches!(
            text.vertical_alignment.as_str(),
            "" | "TOP" | "CENTER" | "BOTTOM"
        ) {
            return Err(invalid("invalid text style vertical alignment"));
        }
        if !valid_style_shadow(
            &text.shadow,
            text.shadow_color,
            text.shadow_offset_x,
            text.shadow_offset_y,
            text.shadow_opacity,
        ) {
            return Err(invalid("invalid text style shadow"));
        }
    }
    Ok(())
}
fn valid_style_font_name(value: &str) -> bool {
    value.chars().count() <= 64 && !value.chars().any(|r| r == '\\' || r < ' ' || r == '\u{7f}')
}
fn valid_style_float(value: Option<f64>) -> bool {
    value.is_none_or(|v| !v.is_nan() && v.abs() <= f32::MAX as f64)
}
fn valid_style_opacity(value: Option<f64>) -> bool {
    value.is_none_or(unit_interval)
}
fn valid_style_shadow(
    style: &str,
    color: Option<[u16; 3]>,
    x: Option<f64>,
    y: Option<f64>,
    opacity: Option<f64>,
) -> bool {
    match style {
        "" | "OFF" => {}
        "NORMAL" | "OUTLINED" => {
            if color.is_none() || x.is_none() || y.is_none() || opacity.is_none() {
                return false;
            }
        }
        _ => return false,
    }
    valid_style_float(x) && valid_style_float(y) && valid_style_opacity(opacity)
}
#[derive(Default)]
struct StyleWriter(Vec<Element>);
impl StyleWriter {
    fn float(&mut self, tag: Tag, value: Option<f64>, fallback: f64) {
        self.0.push(dio::fl(tag, &[value.unwrap_or(fallback)]));
    }
    fn optional_float(&mut self, tag: Tag, value: Option<f64>) {
        if value.is_some() {
            self.float(tag, value, 0.0);
        }
    }
    fn code(&mut self, tag: Tag, value: &str, fallback: &str) {
        let value = if value.is_empty() { fallback } else { value };
        if !value.is_empty() {
            self.0.push(dio::cs(tag, value));
        }
    }
    fn yn(&mut self, tag: Tag, value: Option<bool>) {
        self.code(tag, flip_value(value == Some(true)), "");
    }
    fn color(&mut self, tag: Tag, value: Option<[u16; 3]>) {
        if let Some(v) = value {
            self.0.push(dio::us(tag, &v));
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn shadow(
        &mut self,
        style: &str,
        color: Option<[u16; 3]>,
        x: Option<f64>,
        y: Option<f64>,
        opacity: Option<f64>,
        include_defaults: bool,
    ) {
        self.code(TAG_SHADOW_STYLE, style, "OFF");
        let mut color = color;
        if include_defaults {
            self.float(TAG_SHADOW_OFFSET_X, x, 0.0);
            self.float(TAG_SHADOW_OFFSET_Y, y, 0.0);
            self.float(TAG_SHADOW_OPACITY, opacity, 0.0);
            if color.is_none() {
                color = Some([0, 0x8080, 0x8080]);
            }
        } else {
            self.optional_float(TAG_SHADOW_OFFSET_X, x);
            self.optional_float(TAG_SHADOW_OFFSET_Y, y);
            self.optional_float(TAG_SHADOW_OPACITY, opacity);
        }
        self.color(TAG_SHADOW_COLOR, color);
    }
    fn sequence(self, tag: Tag) -> Element {
        dio::seq(tag, vec![dio::data_set(self.0)])
    }
}
fn default_style_color(value: Option<[u16; 3]>) -> Option<[u16; 3]> {
    Some(value.unwrap_or([0xffff, 0x8080, 0x8080]))
}
fn compound_style_elements(c: &CompoundGraphic) -> Vec<Element> {
    let mut out = Vec::new();
    if let Some(s) = &c.line_style {
        let mut w = StyleWriter::default();
        w.color(TAG_PATTERN_ON, default_style_color(s.on_color));
        w.color(TAG_PATTERN_OFF, s.off_color);
        w.float(TAG_ON_OPACITY, s.on_opacity, 1.0);
        w.optional_float(TAG_OFF_OPACITY, s.off_opacity);
        w.float(TAG_LINE_THICKNESS, s.thickness, 0.0);
        w.code(TAG_LINE_DASHING, &s.dashing, "SOLID");
        if let Some(p) = s.pattern {
            w.0.push(dio::ul(TAG_LINE_PATTERN, &[p]));
        }
        w.shadow(
            &s.shadow,
            s.shadow_color,
            s.shadow_offset_x,
            s.shadow_offset_y,
            s.shadow_opacity,
            true,
        );
        out.push(w.sequence(TAG_LINE_STYLE));
    }
    if let Some(s) = &c.fill_style {
        let mut w = StyleWriter::default();
        w.color(TAG_PATTERN_ON, default_style_color(s.on_color));
        w.color(TAG_PATTERN_OFF, s.off_color);
        w.float(TAG_ON_OPACITY, s.on_opacity, 1.0);
        w.float(TAG_OFF_OPACITY, s.off_opacity, 0.0);
        w.code(TAG_FILL_MODE, &s.mode, "SOLID");
        if !s.pattern.is_empty() {
            w.0.push(dio::raw(TAG_FILL_PATTERN, Vr::OB, s.pattern.clone()));
        }
        out.push(w.sequence(TAG_FILL_STYLE));
    }
    if let Some(s) = &c.text_style {
        let mut w = StyleWriter::default();
        if !s.font_name.is_empty() {
            w.0.push(dio::lo(TAG_FONT_NAME, &s.font_name));
        }
        w.code(TAG_FONT_NAME_TYPE, &s.font_name_type, "");
        let css = if s.css_font_name.is_empty() {
            "sans-serif"
        } else {
            s.css_font_name.as_str()
        };
        w.0.push(dio::lo(TAG_CSS_FONT, css));
        w.color(TAG_TEXT_COLOR, default_style_color(s.color));
        w.code(TAG_HORIZONTAL, &s.horizontal_alignment, "");
        w.code(TAG_VERTICAL, &s.vertical_alignment, "");
        w.shadow(
            &s.shadow,
            s.shadow_color,
            s.shadow_offset_x,
            s.shadow_offset_y,
            s.shadow_opacity,
            false,
        );
        w.yn(TAG_UNDERLINED, s.underlined);
        w.yn(TAG_BOLD, s.bold);
        w.yn(TAG_ITALIC, s.italic);
        out.push(w.sequence(TAG_TEXT_STYLE));
    }
    out
}
pub(crate) fn select_graphic_annotations(
    state: &State,
    target: &ImageState,
) -> Vec<GraphicAnnotation> {
    let mut counts: HashMap<u32, i64> = HashMap::new();
    let mut applicable: Vec<&GraphicAnnotation> = Vec::new();
    for item in &state.graphic_annotations {
        if item.compound.is_some() && item.compound_graphic_id_present() {
            *counts.entry(item.compound_graphic_id).or_default() += 1;
        }
        if item.referenced_images.is_empty() || references_apply(&item.referenced_images, target) {
            applicable.push(item);
        }
    }
    let mut primary: HashMap<u32, usize> = HashMap::new();
    for (i, item) in applicable.iter().enumerate() {
        if let Some(c) = &item.compound
            && !item.malformed_compound_link
            && item.compound_graphic_id_present()
            && counts.get(&item.compound_graphic_id) == Some(&1)
            && compound_fits_target(c, target)
        {
            primary.insert(item.compound_graphic_id, i);
        }
    }
    for item in &applicable {
        if item.compound.is_some() || !item.compound_graphic_id_present() {
            continue;
        }
        if let Some(&ci) = primary.get(&item.compound_graphic_id)
            && (item.malformed_compound_link || !same_compound_scope(item, applicable[ci]))
        {
            primary.remove(&item.compound_graphic_id);
        }
    }
    let mut selected = Vec::new();
    for (i, item) in applicable.iter().enumerate() {
        let primary_index = primary.get(&item.compound_graphic_id).copied();
        if item.compound.is_some() {
            if primary_index == Some(i) {
                selected.push((*item).clone());
            }
            continue;
        }
        if let Some(ci) = primary_index
            && !item.malformed_compound_link
            && item.compound_graphic_id_present()
            && same_compound_scope(item, applicable[ci])
        {
            continue;
        }
        selected.push((*item).clone());
    }
    selected
}
fn same_compound_scope(a: &GraphicAnnotation, b: &GraphicAnnotation) -> bool {
    a.group_index == b.group_index
        && a.same_graphic_group(b)
        && graphic_layer_name(&a.layer_name) == graphic_layer_name(&b.layer_name)
        && a.referenced_images == b.referenced_images
}
fn compound_fits_target(graphic: &CompoundGraphic, target: &ImageState) -> bool {
    if validate_compound_graphic(Some(graphic)).is_err()
        || !standard_compound_type(&graphic.compound_type)
        || target.rows <= 0
        || target.columns <= 0
    {
        return false;
    }
    if normalized_annotation_units(&graphic.units) == ANNOTATION_UNITS_PIXEL {
        let inside = |p: &Point2D| {
            p.x >= 0.0 && p.x <= target.columns as f64 && p.y >= 0.0 && p.y <= target.rows as f64
        };
        if !graphic.points.iter().all(inside) {
            return false;
        }
        if let Some(p) = &graphic.rotation_point
            && !inside(p)
        {
            return false;
        }
    }
    true
}
pub fn capture_annotation_state(state: Option<&State>) -> Result<State, Error> {
    let Some(state) = state else {
        return Err(invalid("annotation state is nil"));
    };
    const LIMIT: usize = 1 << 20;
    let mut remaining = LIMIT;
    let mut reserve = |n: usize| -> bool {
        if n > remaining {
            return false;
        }
        remaining -= n;
        true
    };
    fn references(reserve: &mut dyn FnMut(usize) -> bool, refs: &[ReferencedImage]) -> bool {
        if !reserve(refs.len()) {
            return false;
        }
        refs.iter().all(|r| reserve(r.frames.len()))
    }
    fn area(reserve: &mut dyn FnMut(usize) -> bool, a: &DisplayedArea) -> bool {
        reserve(a.pixel_spacing.len()) && reserve(a.pixel_aspect_ratio.len())
    }
    let over = || invalid(format!("annotation snapshot exceeds {LIMIT} entries"));
    if !reserve(state.graphic_layers.len())
        || !reserve(state.graphic_annotations.len())
        || !reserve(state.displayed_areas.len())
        || !references(&mut reserve, &state.referenced_images)
        || !references(&mut reserve, &state.displayed_area.referenced_images)
    {
        return Err(over());
    }
    for item in &state.graphic_annotations {
        if !reserve(item.polyline.len()) || !references(&mut reserve, &item.referenced_images) {
            return Err(over());
        }
        if let Some(g) = &item.compound
            && (!reserve(g.points.len())
                || !reserve(g.major_ticks.len())
                || g.fill_style
                    .as_ref()
                    .is_some_and(|f| !reserve(f.pattern.len())))
        {
            return Err(over());
        }
    }
    if !area(&mut reserve, &state.displayed_area) {
        return Err(over());
    }
    for a in &state.displayed_areas {
        if !references(&mut reserve, &a.referenced_images) || !area(&mut reserve, a) {
            return Err(over());
        }
    }
    Ok(State {
        sop_class_uid: state.sop_class_uid.clone(),
        sop_instance_uid: state.sop_instance_uid.clone(),
        spatial_transform: state.spatial_transform,
        referenced_images: state.referenced_images.clone(),
        displayed_area: state.displayed_area.clone(),
        displayed_areas: state.displayed_areas.clone(),
        graphic_layers: state.graphic_layers.clone(),
        graphic_annotations: state.graphic_annotations.clone(),
        ..State::default()
    })
}
