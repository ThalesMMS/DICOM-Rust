use super::{
    Error, GraphicAnnotation, Point2D, State, TAG_ANCHOR_POINT, TAG_ANCHOR_POINT_ANNOTATION_UNITS,
    TAG_ANCHOR_POINT_VISIBILITY, TAG_GRAPHIC_LAYER, TAG_GRAPHIC_LAYER_ORDER,
    TAG_GRAPHIC_LAYER_SEQUENCE, TAG_LATERALITY, TAG_LUT_DATA, TAG_LUT_DESCRIPTOR,
    TAG_LUT_EXPLANATION, TAG_UNFORMATTED_TEXT_VALUE, flip_value, graphic_layer_name, invalid,
    normalized_annotation_units, validate_annotation_point,
};
use crate::dcmtime::ZonedTime;
use crate::deid::trim_space;
use crate::derivedio as dio;
use crate::display::{
    Lut, LutOptions, ModalityLut as DisplayModalityLut, encode_lut_data, lut_from_item,
    modality_lut_from_object,
};
use crate::object::{DataSet, Element, Object, Tag, Vr};
use crate::textfmt::{float_v, quote, to_upper};
pub const JUSTIFY_LEFT: &str = "LEFT";
pub const JUSTIFY_RIGHT: &str = "RIGHT";
pub const JUSTIFY_CENTER: &str = "CENTER";
pub const DEFAULT_CONTENT_LABEL: &str = "UNNAMED";
pub(crate) const MAX_TEXT_LENGTH: usize = 1024;
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
const TAG_SERIES_NUMBER: Tag = t(0x0020, 0x0011);
const TAG_INSTANCE_NUMBER: Tag = t(0x0020, 0x0013);
const TAG_CONTENT_LABEL: Tag = t(0x0070, 0x0080);
const TAG_CONTENT_DESCRIPTION: Tag = t(0x0070, 0x0081);
const TAG_PRESENTATION_CREATION_DATE: Tag = t(0x0070, 0x0082);
const TAG_PRESENTATION_CREATION_TIME: Tag = t(0x0070, 0x0083);
const TAG_CONTENT_CREATOR_NAME: Tag = t(0x0070, 0x0084);
const TAG_BOUNDING_BOX_ANNOTATION_UNITS: Tag = t(0x0070, 0x0003);
const TAG_BOUNDING_BOX_TOP_LEFT: Tag = t(0x0070, 0x0010);
const TAG_BOUNDING_BOX_BOTTOM_RIGHT: Tag = t(0x0070, 0x0011);
const TAG_BOUNDING_BOX_JUSTIFICATION: Tag = t(0x0070, 0x0012);
const TAG_GRAPHIC_LAYER_RECOMMENDED_GRAYSCALE: Tag = t(0x0070, 0x0066);
const TAG_GRAPHIC_LAYER_DESCRIPTION: Tag = t(0x0070, 0x0068);
const TAG_GRAPHIC_LAYER_RECOMMENDED_CIELAB: Tag = t(0x0070, 0x0401);
const TAG_PRESENTATION_LUT_SEQUENCE: Tag = t(0x2050, 0x0010);
const TAG_RESCALE_INTERCEPT: Tag = t(0x0028, 0x1052);
const TAG_RESCALE_SLOPE: Tag = t(0x0028, 0x1053);
const TAG_RESCALE_TYPE: Tag = t(0x0028, 0x1054);
const TAG_MODALITY_LUT_SEQUENCE: Tag = t(0x0028, 0x3000);
const TAG_MODALITY_LUT_TYPE: Tag = t(0x0028, 0x3004);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicLayer {
    pub name: String,
    pub order: i64,
    pub recommended_grayscale: Option<u16>,
    pub recommended_cielab: Option<[u16; 3]>,
    pub description: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoundingBox {
    pub top_left: Point2D,
    pub bottom_right: Point2D,
    pub units: String,
    pub justification: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModalityLut {
    pub slope: f64,
    pub intercept: f64,
    pub rescale_type: String,
    pub lut: Option<Lut>,
    pub lut_type: String,
    pub explanation: String,
}
impl ModalityLut {
    pub fn transform(&self) -> DisplayModalityLut {
        match &self.lut {
            Some(lut) => DisplayModalityLut {
                slope: 0.0,
                intercept: 0.0,
                lut: Some(lut.clone()),
            },
            None => DisplayModalityLut {
                slope: self.slope,
                intercept: self.intercept,
                lut: None,
            },
        }
    }
}
fn pad(v: i64, n: usize) -> String {
    format!("{v:0n$}")
}
pub(crate) fn identification_elements(
    state: &State,
    now: fn() -> ZonedTime,
) -> Result<Vec<Element>, Error> {
    let label = if state.content_label.is_empty() {
        DEFAULT_CONTENT_LABEL
    } else {
        state.content_label.as_str()
    };
    if !valid_cs(label) {
        return Err(invalid(format!(
            "Content Label {} is not a CS value",
            quote(label)
        )));
    }
    if state.instance_number < 0 || state.series_number < 0 {
        return Err(invalid("negative Series or Instance Number"));
    }
    if !state.laterality.is_empty() && state.laterality != "L" && state.laterality != "R" {
        return Err(invalid(format!(
            "Laterality {} is neither L nor R",
            quote(&state.laterality)
        )));
    }
    let instance = if state.instance_number == 0 {
        1
    } else {
        state.instance_number
    };
    let (mut date, mut clock) = (state.creation_date.clone(), state.creation_time.clone());
    if date.is_empty() || clock.is_empty() {
        let n = now();
        if date.is_empty() {
            date = format!(
                "{}{}{}",
                pad(n.year(), 4),
                pad(n.month(), 2),
                pad(n.day(), 2)
            );
        }
        if clock.is_empty() {
            clock = format!(
                "{}{}{}",
                pad(n.hour(), 2),
                pad(n.minute(), 2),
                pad(n.second(), 2)
            );
        }
    }
    let series = if state.series_number > 0 {
        dio::is(TAG_SERIES_NUMBER, &[state.series_number])
    } else {
        dio::is(TAG_SERIES_NUMBER, &[])
    };
    let mut elements = vec![
        series,
        dio::is(TAG_INSTANCE_NUMBER, &[instance]),
        dio::cs(TAG_CONTENT_LABEL, label),
        dio::lo(TAG_CONTENT_DESCRIPTION, &state.content_description),
        dio::str_elem(TAG_PRESENTATION_CREATION_DATE, Vr::DA, &date),
        dio::str_elem(TAG_PRESENTATION_CREATION_TIME, Vr::TM, &clock),
        dio::str_elem(
            TAG_CONTENT_CREATOR_NAME,
            Vr::PN,
            &state.content_creator_name,
        ),
    ];
    if !state.laterality.is_empty() {
        elements.push(dio::cs(TAG_LATERALITY, &state.laterality));
    }
    Ok(elements)
}
pub(crate) fn read_identification(obj: &Object, state: &mut State) {
    state.series_number = dio::int(obj, TAG_SERIES_NUMBER);
    state.instance_number = dio::int(obj, TAG_INSTANCE_NUMBER);
    state.laterality = dio::clean_string(obj, TAG_LATERALITY);
    state.content_label = dio::clean_string(obj, TAG_CONTENT_LABEL);
    state.content_description = dio::clean_string(obj, TAG_CONTENT_DESCRIPTION);
    state.creation_date = dio::clean_string(obj, TAG_PRESENTATION_CREATION_DATE);
    state.creation_time = dio::clean_string(obj, TAG_PRESENTATION_CREATION_TIME);
    state.content_creator_name = dio::clean_string(obj, TAG_CONTENT_CREATOR_NAME);
}
pub(crate) fn valid_cs(value: &str) -> bool {
    if value.is_empty() || value.len() > 16 || trim_space(value).is_empty() {
        return false;
    }
    value
        .chars()
        .all(|r| r.is_ascii_uppercase() || r.is_ascii_digit() || r == ' ' || r == '_')
}
pub(crate) fn resolved_graphic_layers(state: &State) -> Result<Vec<GraphicLayer>, Error> {
    let mut layers: Vec<GraphicLayer> = Vec::with_capacity(state.graphic_layers.len() + 1);
    for (i, layer) in state.graphic_layers.iter().enumerate() {
        if !valid_cs(&layer.name) {
            return Err(invalid(format!(
                "graphic layer {i} name {} is not a CS value",
                quote(&layer.name)
            )));
        }
        if layers.iter().any(|l| l.name == layer.name) {
            return Err(invalid(format!(
                "graphic layer {} is defined twice",
                quote(&layer.name)
            )));
        }
        if layer.order < 0 {
            return Err(invalid(format!(
                "graphic layer {} has a negative order",
                quote(&layer.name)
            )));
        }
        layers.push(layer.clone());
    }
    let used = |name: &str, layers: &mut Vec<GraphicLayer>| -> Result<(), Error> {
        let name = graphic_layer_name(name);
        if !valid_cs(&name) {
            return Err(invalid(format!(
                "graphic layer name {} is not a CS value",
                quote(&name)
            )));
        }
        if !layers.iter().any(|l| l.name == name) {
            layers.push(GraphicLayer {
                name,
                ..GraphicLayer::default()
            });
        }
        Ok(())
    };
    for item in &state.graphic_annotations {
        used(&item.layer_name, &mut layers)?;
    }
    for activation in &state.overlay_activations {
        used(&activation.layer, &mut layers)?;
    }
    let mut next = 0;
    for layer in &layers {
        next = next.max(layer.order);
    }
    for layer in &mut layers {
        if layer.order == 0 {
            next += 1;
            layer.order = next;
        }
    }
    layers.sort_by_key(|l| l.order);
    Ok(layers)
}
pub(crate) fn graphic_layer_sequence(layers: &[GraphicLayer]) -> Element {
    let items = layers
        .iter()
        .map(|layer| {
            let mut elements = vec![
                dio::cs(TAG_GRAPHIC_LAYER, &layer.name),
                dio::is(TAG_GRAPHIC_LAYER_ORDER, &[layer.order]),
            ];
            if let Some(g) = layer.recommended_grayscale {
                elements.push(dio::us(TAG_GRAPHIC_LAYER_RECOMMENDED_GRAYSCALE, &[g]));
            }
            if let Some(lab) = layer.recommended_cielab {
                elements.push(dio::us(TAG_GRAPHIC_LAYER_RECOMMENDED_CIELAB, &lab));
            }
            if !layer.description.is_empty() {
                elements.push(dio::lo(TAG_GRAPHIC_LAYER_DESCRIPTION, &layer.description));
            }
            dio::data_set(elements)
        })
        .collect();
    dio::seq(TAG_GRAPHIC_LAYER_SEQUENCE, items)
}
pub(crate) fn read_graphic_layers(obj: &Object) -> Vec<GraphicLayer> {
    dio::sequence(obj, TAG_GRAPHIC_LAYER_SEQUENCE)
        .iter()
        .map(|item| {
            let mut layer = GraphicLayer {
                name: dio::clean_string(item, TAG_GRAPHIC_LAYER),
                order: dio::int(item, TAG_GRAPHIC_LAYER_ORDER),
                description: dio::clean_string(item, TAG_GRAPHIC_LAYER_DESCRIPTION),
                ..GraphicLayer::default()
            };
            if let Some(values) = dio::ints(Some(item), TAG_GRAPHIC_LAYER_RECOMMENDED_GRAYSCALE)
                && let Some(&v) = values.first()
            {
                layer.recommended_grayscale = Some(v as u16);
            }
            if let Some(values) = dio::ints(Some(item), TAG_GRAPHIC_LAYER_RECOMMENDED_CIELAB)
                && values.len() == 3
            {
                layer.recommended_cielab =
                    Some([values[0] as u16, values[1] as u16, values[2] as u16]);
            }
            layer
        })
        .collect()
}
pub(crate) fn validate_text_object(item: &GraphicAnnotation) -> Result<(), Error> {
    let count = item.text.chars().count();
    if count > MAX_TEXT_LENGTH {
        return Err(invalid(format!(
            "text of {count} characters exceeds the ST limit of {MAX_TEXT_LENGTH}"
        )));
    }
    if let Some(b) = &item.bounding_box {
        for point in [b.top_left, b.bottom_right] {
            validate_annotation_point(point, &b.units).map_err(|e| e.wrap("bounding box"))?;
        }
        match normalized_justification(&b.justification).as_str() {
            JUSTIFY_LEFT | JUSTIFY_RIGHT | JUSTIFY_CENTER => {}
            _ => {
                return Err(invalid(format!(
                    "text justification {}",
                    quote(&b.justification)
                )));
            }
        }
    }
    if has_anchor(item) {
        validate_annotation_point(item.anchor, &item.anchor_units).map_err(|e| e.wrap("anchor"))?;
    }
    Ok(())
}
pub(crate) fn has_anchor(item: &GraphicAnnotation) -> bool {
    item.bounding_box.is_none() || item.anchor_visible || item.anchor != Point2D::default()
}
pub(crate) fn normalized_justification(value: &str) -> String {
    let value = to_upper(trim_space(value));
    if value.is_empty() {
        JUSTIFY_LEFT.into()
    } else {
        value
    }
}
pub(crate) fn text_object_item(item: &GraphicAnnotation) -> DataSet {
    let mut elements = super::compound::compound_link_elements(item);
    elements.push(dio::str_elem(
        TAG_UNFORMATTED_TEXT_VALUE,
        Vr::ST,
        &item.text,
    ));
    if let Some(b) = &item.bounding_box {
        elements.push(dio::cs(
            TAG_BOUNDING_BOX_ANNOTATION_UNITS,
            &normalized_annotation_units(&b.units),
        ));
        elements.push(dio::fl(
            TAG_BOUNDING_BOX_TOP_LEFT,
            &[b.top_left.x, b.top_left.y],
        ));
        elements.push(dio::fl(
            TAG_BOUNDING_BOX_BOTTOM_RIGHT,
            &[b.bottom_right.x, b.bottom_right.y],
        ));
        elements.push(dio::cs(
            TAG_BOUNDING_BOX_JUSTIFICATION,
            &normalized_justification(&b.justification),
        ));
    }
    if has_anchor(item) {
        elements.push(dio::cs(
            TAG_ANCHOR_POINT_ANNOTATION_UNITS,
            &normalized_annotation_units(&item.anchor_units),
        ));
        elements.push(dio::fl(TAG_ANCHOR_POINT, &[item.anchor.x, item.anchor.y]));
        elements.push(dio::cs(
            TAG_ANCHOR_POINT_VISIBILITY,
            flip_value(item.anchor_visible),
        ));
    }
    dio::data_set(elements)
}
pub(crate) fn read_bounding_box(item: &Object) -> Option<BoundingBox> {
    let top_left = dio::floats(Some(item), TAG_BOUNDING_BOX_TOP_LEFT).unwrap_or_default();
    let bottom_right = dio::floats(Some(item), TAG_BOUNDING_BOX_BOTTOM_RIGHT).unwrap_or_default();
    if top_left.len() < 2 || bottom_right.len() < 2 {
        return None;
    }
    Some(BoundingBox {
        top_left: Point2D::new(top_left[0], top_left[1]),
        bottom_right: Point2D::new(bottom_right[0], bottom_right[1]),
        units: normalized_annotation_units(&dio::clean_string(
            item,
            TAG_BOUNDING_BOX_ANNOTATION_UNITS,
        )),
        justification: normalized_justification(&dio::clean_string(
            item,
            TAG_BOUNDING_BOX_JUSTIFICATION,
        )),
    })
}
pub(crate) fn validate_modality_lut(m: Option<&ModalityLut>) -> Result<(), Error> {
    let Some(m) = m else {
        return Ok(());
    };
    if let Some(lut) = &m.lut {
        if lut.bits_per_entry != 8 && lut.bits_per_entry != 16 {
            return Err(invalid(format!(
                "Modality LUT has {} bits per entry, not 8 or 16",
                lut.bits_per_entry
            )));
        }
        return validate_lut("Modality LUT", lut, -(1 << 15));
    }
    if m.slope == 0.0 || !m.slope.is_finite() || !m.intercept.is_finite() {
        return Err(invalid(format!(
            "Modality LUT rescale {}/{}",
            float_v(m.slope),
            float_v(m.intercept)
        )));
    }
    Ok(())
}
fn validate_lut(name: &str, lut: &Lut, min_first_mapped: i64) -> Result<(), Error> {
    if lut.first_mapped < min_first_mapped || lut.first_mapped > (1 << 16) - 1 {
        return Err(invalid(format!(
            "{name} first mapped value {}",
            lut.first_mapped
        )));
    }
    if lut.entries.is_empty() || lut.entries.len() > 1 << 16 {
        return Err(invalid(format!("{name} has {} entries", lut.entries.len())));
    }
    let mut count = lut.entries.len() as i64;
    if count == 1 << 16 {
        count = 0;
    }
    Lut::new(
        &[count, lut.first_mapped, lut.bits_per_entry],
        lut.entries.clone(),
    )
    .map_err(|e| Error::other(format!("dicom/gsps: {name}: {e}")))?;
    Ok(())
}
pub(crate) fn modality_lut_elements(m: Option<&ModalityLut>) -> Vec<Element> {
    let Some(m) = m else {
        return Vec::new();
    };
    if let Some(lut) = &m.lut {
        let lut_type = if m.lut_type.is_empty() {
            "US"
        } else {
            m.lut_type.as_str()
        };
        let mut item = lut_item_elements(lut, &m.explanation);
        item.push(dio::lo(TAG_MODALITY_LUT_TYPE, lut_type));
        return vec![dio::seq(
            TAG_MODALITY_LUT_SEQUENCE,
            vec![dio::data_set(item)],
        )];
    }
    let rescale_type = if m.rescale_type.is_empty() {
        "US"
    } else {
        m.rescale_type.as_str()
    };
    vec![
        dio::ds(TAG_RESCALE_INTERCEPT, &[m.intercept]),
        dio::ds(TAG_RESCALE_SLOPE, &[m.slope]),
        dio::lo(TAG_RESCALE_TYPE, rescale_type),
    ]
}
pub(crate) fn read_modality_lut(obj: &Object) -> Result<Option<ModalityLut>, Error> {
    let has_slope = obj.has(TAG_RESCALE_SLOPE);
    let has_intercept = obj.has(TAG_RESCALE_INTERCEPT);
    let items = dio::sequence(obj, TAG_MODALITY_LUT_SEQUENCE);
    if !has_slope && !has_intercept && items.is_empty() {
        return Ok(None);
    }
    let (transform, err) = modality_lut_from_object(obj, LutOptions::default());
    if let Some(err) = err {
        return Err(Error::other(format!("dicom/gsps: Modality LUT: {err}")));
    }
    let mut out = ModalityLut {
        slope: transform.slope,
        intercept: transform.intercept,
        rescale_type: dio::clean_string(obj, TAG_RESCALE_TYPE),
        lut: transform.lut,
        ..ModalityLut::default()
    };
    if let Some(first) = items.first() {
        out.lut_type = dio::clean_string(first, TAG_MODALITY_LUT_TYPE);
        out.explanation = dio::clean_string(first, TAG_LUT_EXPLANATION);
    }
    Ok(Some(out))
}
pub(crate) fn validate_presentation_lut(lut: Option<&Lut>) -> Result<(), Error> {
    let Some(lut) = lut else {
        return Ok(());
    };
    if lut.first_mapped != 0 {
        return Err(invalid(format!(
            "Presentation LUT first mapped value {} is not 0",
            lut.first_mapped
        )));
    }
    if lut.bits_per_entry < 10 || lut.bits_per_entry > 16 {
        return Err(invalid(format!(
            "Presentation LUT has {} bits per entry, not 10 to 16",
            lut.bits_per_entry
        )));
    }
    validate_lut("Presentation LUT", lut, 0)
}
pub(crate) fn presentation_lut_sequence(lut: &Lut, explanation: &str) -> Element {
    dio::seq(
        TAG_PRESENTATION_LUT_SEQUENCE,
        vec![dio::data_set(lut_item_elements(lut, explanation))],
    )
}
pub(crate) fn read_presentation_lut(obj: &Object) -> Result<(Option<Lut>, String), Error> {
    let items = dio::sequence(obj, TAG_PRESENTATION_LUT_SEQUENCE);
    let Some(first) = items.first() else {
        return Ok((None, String::new()));
    };
    let lut = lut_from_item(Some(first), false, LutOptions::default())
        .map_err(|e| Error::other(format!("dicom/gsps: Presentation LUT: {e}")))?;
    Ok((Some(lut), dio::clean_string(first, TAG_LUT_EXPLANATION)))
}
pub(crate) fn lut_item_elements(lut: &Lut, explanation: &str) -> Vec<Element> {
    let mut count = lut.entries.len();
    if count == 1 << 16 {
        count = 0;
    }
    let vr = if lut.first_mapped < 0 { Vr::SS } else { Vr::US };
    let mut descriptor = Vec::with_capacity(6);
    descriptor.extend_from_slice(&(count as u16).to_le_bytes());
    descriptor.extend_from_slice(&(lut.first_mapped as u16).to_le_bytes());
    descriptor.extend_from_slice(&(lut.bits_per_entry as u16).to_le_bytes());
    let mut elements = vec![dio::raw(TAG_LUT_DESCRIPTOR, vr, descriptor)];
    if !explanation.is_empty() {
        elements.push(dio::lo(TAG_LUT_EXPLANATION, explanation));
    }
    elements.push(dio::raw(
        TAG_LUT_DATA,
        Vr::OW,
        encode_lut_data(&lut.entries, lut.bits_per_entry),
    ));
    elements
}
