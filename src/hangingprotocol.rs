#![forbid(unsafe_code)]
use crate::derivedio;
use crate::object::{Object, Tag, Value, Vr};
use crate::textfmt::{quote, to_upper};
use std::fmt;
pub const HANGING_PROTOCOL_STORAGE: &str = "1.2.840.10008.5.1.4.38.1";
const MAX_DEFINITIONS: usize = 64;
const MAX_IMAGE_SETS: usize = 64;
const MAX_SELECTORS: usize = 64;
const MAX_SCREENS: usize = 16;
const MAX_DISPLAY_SETS: usize = 128;
const MAX_IMAGE_BOXES: usize = 256;
const MAX_VALUES: usize = 32;
const MAX_TEXT_BYTES: usize = 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NilDataset,
    NotHangingProtocol,
    MissingName,
    BoundsExceeded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
fn bound_error(path: &str, got: usize, limit: usize) -> Error {
    Error {
        kind: ErrorKind::BoundsExceeded,
        message: format!(
            "dicom hanging protocol: bounded parse limit exceeded: {path} has {got} items (limit {limit})"
        ),
    }
}
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_HANGING_PROTOCOL_NAME: Tag = Tag::new(0x0072, 0x0002);
const TAG_HANGING_PROTOCOL_DESCRIPTION: Tag = Tag::new(0x0072, 0x0004);
const TAG_HANGING_PROTOCOL_LEVEL: Tag = Tag::new(0x0072, 0x0006);
const TAG_HANGING_PROTOCOL_CREATOR: Tag = Tag::new(0x0072, 0x0008);
const TAG_HANGING_PROTOCOL_DEFINITION_SEQ: Tag = Tag::new(0x0072, 0x000C);
const TAG_NUMBER_OF_PRIORS_REFERENCED: Tag = Tag::new(0x0072, 0x0014);
const TAG_IMAGE_SETS_SEQUENCE: Tag = Tag::new(0x0072, 0x0020);
const TAG_IMAGE_SET_SELECTOR_SEQUENCE: Tag = Tag::new(0x0072, 0x0022);
const TAG_IMAGE_SET_SELECTOR_USAGE_FLAG: Tag = Tag::new(0x0072, 0x0024);
const TAG_SELECTOR_ATTRIBUTE: Tag = Tag::new(0x0072, 0x0026);
const TAG_SELECTOR_VALUE_NUMBER: Tag = Tag::new(0x0072, 0x0028);
const TAG_TIME_BASED_IMAGE_SETS_SEQUENCE: Tag = Tag::new(0x0072, 0x0030);
const TAG_IMAGE_SET_NUMBER: Tag = Tag::new(0x0072, 0x0032);
const TAG_IMAGE_SET_SELECTOR_CATEGORY: Tag = Tag::new(0x0072, 0x0034);
const TAG_RELATIVE_TIME: Tag = Tag::new(0x0072, 0x0038);
const TAG_RELATIVE_TIME_UNITS: Tag = Tag::new(0x0072, 0x003A);
const TAG_ABSTRACT_PRIOR_VALUE: Tag = Tag::new(0x0072, 0x003C);
const TAG_IMAGE_SET_LABEL: Tag = Tag::new(0x0072, 0x0040);
const TAG_SELECTOR_ATTRIBUTE_VR: Tag = Tag::new(0x0072, 0x0050);
const TAG_SELECTOR_SEQUENCE_POINTER: Tag = Tag::new(0x0072, 0x0052);
const TAG_SELECTOR_ATTRIBUTE_PRIVATE_CREATOR: Tag = Tag::new(0x0072, 0x0056);
const TAG_NUMBER_OF_SCREENS: Tag = Tag::new(0x0072, 0x0100);
const TAG_NOMINAL_SCREEN_DEFINITION_SEQUENCE: Tag = Tag::new(0x0072, 0x0102);
const TAG_NUMBER_OF_VERTICAL_PIXELS: Tag = Tag::new(0x0072, 0x0104);
const TAG_NUMBER_OF_HORIZONTAL_PIXELS: Tag = Tag::new(0x0072, 0x0106);
const TAG_DISPLAY_ENVIRONMENT_SPATIAL_POSITION: Tag = Tag::new(0x0072, 0x0108);
const TAG_DISPLAY_SETS_SEQUENCE: Tag = Tag::new(0x0072, 0x0200);
const TAG_DISPLAY_SET_NUMBER: Tag = Tag::new(0x0072, 0x0202);
const TAG_DISPLAY_SET_LABEL: Tag = Tag::new(0x0072, 0x0203);
const TAG_DISPLAY_SET_PRESENTATION_GROUP: Tag = Tag::new(0x0072, 0x0204);
const TAG_PARTIAL_DATA_DISPLAY_HANDLING: Tag = Tag::new(0x0072, 0x0208);
const TAG_SYNCHRONIZED_SCROLLING_SEQUENCE: Tag = Tag::new(0x0072, 0x0210);
const TAG_DISPLAY_SET_SCROLLING_GROUP: Tag = Tag::new(0x0072, 0x0212);
const TAG_IMAGE_BOXES_SEQUENCE: Tag = Tag::new(0x0072, 0x0300);
const TAG_IMAGE_BOX_NUMBER: Tag = Tag::new(0x0072, 0x0302);
const TAG_IMAGE_BOX_LAYOUT_TYPE: Tag = Tag::new(0x0072, 0x0304);
const TAG_IMAGE_BOX_TILE_HORIZONTAL_DIMENSION: Tag = Tag::new(0x0072, 0x0306);
const TAG_IMAGE_BOX_TILE_VERTICAL_DIMENSION: Tag = Tag::new(0x0072, 0x0308);
const TAG_FILTER_OPERATIONS_SEQUENCE: Tag = Tag::new(0x0072, 0x0400);
const TAG_IMAGE_BOX_SYNCHRONIZATION_SEQUENCE: Tag = Tag::new(0x0072, 0x0430);
const TAG_SYNCHRONIZED_IMAGE_BOX_LIST: Tag = Tag::new(0x0072, 0x0432);
const TAG_TYPE_OF_SYNCHRONIZATION: Tag = Tag::new(0x0072, 0x0434);
const TAG_BLENDING_OPERATION_TYPE: Tag = Tag::new(0x0072, 0x0500);
const TAG_REFORMATTING_OPERATION_TYPE: Tag = Tag::new(0x0072, 0x0510);
const TAG_THREE_D_RENDERING_TYPE: Tag = Tag::new(0x0072, 0x0520);
const TAG_SORTING_OPERATIONS_SEQUENCE: Tag = Tag::new(0x0072, 0x0600);
const TAG_DISPLAY_SET_PATIENT_ORIENTATION: Tag = Tag::new(0x0072, 0x0700);
const TAG_VOI_TYPE: Tag = Tag::new(0x0072, 0x0702);
const TAG_PSEUDO_COLOR_TYPE: Tag = Tag::new(0x0072, 0x0704);
const TAG_SHOW_GRAYSCALE_INVERTED: Tag = Tag::new(0x0072, 0x0706);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: String,
    pub code: String,
    pub message: String,
}
fn diagnostic(path: impl Into<String>, code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        path: path.into(),
        code: code.to_string(),
        message: message.to_string(),
    }
}
fn unsupported(path: impl Into<String>, construct: &str) -> Diagnostic {
    Diagnostic {
        path: path.into(),
        code: "unsupported-construct".to_string(),
        message: format!("{construct} is not representable by the 2D hanging model"),
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    pub usage_flag: String,
    pub category: String,
    pub attribute: Tag,
    pub attribute_vr: String,
    pub value_number: i64,
    pub values: Vec<String>,
    pub sequence_pointer: Vec<Tag>,
    pub attribute_private_creator: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageSet {
    pub number: i64,
    pub label: String,
    pub prior_index: i64,
    pub is_prior: bool,
    pub relative_time: Vec<i64>,
    pub relative_time_units: String,
    pub abstract_prior: Vec<i64>,
    pub selectors: Vec<Selector>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screen {
    pub vertical_pixels: i64,
    pub horizontal_pixels: i64,
    pub spatial_position: Vec<f64>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageBox {
    pub number: i64,
    pub image_set_number: i64,
    pub layout_type: String,
    pub tile_columns: i64,
    pub tile_rows: i64,
    pub spatial_position: Vec<f64>,
    pub voi_type: String,
    pub patient_orientation: Vec<String>,
    pub show_inverted: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplaySet {
    pub number: i64,
    pub label: String,
    pub presentation_group: i64,
    pub image_set_number: i64,
    pub partial_data_handling: String,
    pub voi_type: String,
    pub patient_orientation: Vec<String>,
    pub show_inverted: bool,
    pub image_boxes: Vec<ImageBox>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncGroup {
    pub display_sets: Vec<i64>,
    pub image_boxes: Vec<i64>,
    pub r#type: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Protocol {
    pub sop_instance_uid: String,
    pub name: String,
    pub description: String,
    pub level: String,
    pub creator: String,
    pub number_of_priors: i64,
    pub number_of_screens: i64,
    pub definitions: Vec<Selector>,
    pub image_sets: Vec<ImageSet>,
    pub screens: Vec<Screen>,
    pub display_sets: Vec<DisplaySet>,
    pub sync_groups: Vec<SyncGroup>,
    pub diagnostics: Vec<Diagnostic>,
}
pub fn read(obj: Option<&Object>) -> Result<Protocol, Error> {
    let Some(obj) = obj else {
        return Err(Error {
            kind: ErrorKind::NilDataset,
            message: "dicom hanging protocol: nil dataset".to_string(),
        });
    };
    let uid = derivedio::clean_uid(obj, TAG_SOP_CLASS_UID);
    if uid != HANGING_PROTOCOL_STORAGE {
        return Err(Error {
            kind: ErrorKind::NotHangingProtocol,
            message: format!(
                "dicom hanging protocol: unsupported SOP Class UID: {}",
                quote(&uid)
            ),
        });
    }
    let mut p = Protocol {
        sop_instance_uid: derivedio::clean_uid(obj, TAG_SOP_INSTANCE_UID),
        name: clean_text(obj, TAG_HANGING_PROTOCOL_NAME),
        description: clean_text(obj, TAG_HANGING_PROTOCOL_DESCRIPTION),
        level: clean_text(obj, TAG_HANGING_PROTOCOL_LEVEL),
        creator: clean_text(obj, TAG_HANGING_PROTOCOL_CREATOR),
        number_of_priors: first_int(obj, TAG_NUMBER_OF_PRIORS_REFERENCED),
        number_of_screens: first_int(obj, TAG_NUMBER_OF_SCREENS),
        ..Protocol::default()
    };
    if p.name.is_empty() {
        return Err(Error {
            kind: ErrorKind::MissingName,
            message: "dicom hanging protocol: missing Hanging Protocol Name".to_string(),
        });
    }
    p.definitions = parse_selector_container(
        obj,
        TAG_HANGING_PROTOCOL_DEFINITION_SEQ,
        "definitions",
        MAX_DEFINITIONS,
        &mut p.diagnostics,
    )?;
    parse_image_sets(obj, &mut p)?;
    parse_screens(obj, &mut p)?;
    parse_display_sets(obj, &mut p)?;
    Ok(p)
}
fn sequence(obj: &Object, tag: Tag) -> Vec<Object> {
    obj.get_sequence(tag).unwrap_or_default()
}
fn parse_selector_container(
    obj: &Object,
    sequence_tag: Tag,
    path: &str,
    limit: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Selector>, Error> {
    let items = sequence(obj, sequence_tag);
    if items.len() > limit {
        return Err(bound_error(path, items.len(), limit));
    }
    let mut selectors = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        selectors.push(parse_selector(item, &format!("{path}[{i}]"), diagnostics));
    }
    Ok(selectors)
}
fn parse_image_sets(obj: &Object, p: &mut Protocol) -> Result<(), Error> {
    let items = sequence(obj, TAG_IMAGE_SETS_SEQUENCE);
    if items.len() > MAX_IMAGE_SETS {
        return Err(bound_error("imageSets", items.len(), MAX_IMAGE_SETS));
    }
    for (i, item) in items.iter().enumerate() {
        let path = format!("imageSets[{i}]");
        let selectors = parse_selector_container(
            item,
            TAG_IMAGE_SET_SELECTOR_SEQUENCE,
            &format!("{path}.selectors"),
            MAX_SELECTORS,
            &mut p.diagnostics,
        )?;
        let time_items = sequence(item, TAG_TIME_BASED_IMAGE_SETS_SEQUENCE);
        if time_items.len() > MAX_IMAGE_SETS
            || p.image_sets.len() + time_items.len().max(1) > MAX_IMAGE_SETS
        {
            return Err(bound_error(
                &format!("{path}.timeBased"),
                time_items.len(),
                MAX_IMAGE_SETS,
            ));
        }
        if time_items.is_empty() {
            check_image_set_value_bounds(item, &path)?;
            p.image_sets
                .push(image_set_from(item, i as i64 + 1, &selectors));
            continue;
        }
        for (j, time_item) in time_items.iter().enumerate() {
            check_image_set_value_bounds(time_item, &format!("{path}.timeBased[{j}]"))?;
            let mut set = image_set_from(time_item, p.image_sets.len() as i64 + 1, &selectors);
            if set.label.is_empty() {
                set.label = clean_text(item, TAG_IMAGE_SET_LABEL);
            }
            let number = set.number;
            p.image_sets.push(set);
            if number == 0 {
                p.diagnostics.push(diagnostic(
                    format!("{path}.timeBased[{j}]"),
                    "missing-image-set-number",
                    "time-based image set has no Image Set Number",
                ));
            }
        }
    }
    Ok(())
}
fn check_image_set_value_bounds(obj: &Object, path: &str) -> Result<(), Error> {
    for tag in [TAG_RELATIVE_TIME, TAG_ABSTRACT_PRIOR_VALUE] {
        let count = derivedio::ints(Some(obj), tag).map_or(0, |v| v.len());
        if count > MAX_VALUES {
            return Err(bound_error(&format!("{path}.{tag}"), count, MAX_VALUES));
        }
    }
    Ok(())
}
fn image_set_from(obj: &Object, fallback: i64, selectors: &[Selector]) -> ImageSet {
    let mut number = first_int(obj, TAG_IMAGE_SET_NUMBER);
    if number == 0 {
        number = fallback;
    }
    let category = to_upper(&clean_text(obj, TAG_IMAGE_SET_SELECTOR_CATEGORY));
    let selectors = selectors
        .iter()
        .cloned()
        .map(|mut selector| {
            selector.category = category.clone();
            selector
        })
        .collect();
    let mut set = ImageSet {
        number,
        label: clean_text(obj, TAG_IMAGE_SET_LABEL),
        relative_time: ints(obj, TAG_RELATIVE_TIME),
        relative_time_units: clean_text(obj, TAG_RELATIVE_TIME_UNITS),
        abstract_prior: ints(obj, TAG_ABSTRACT_PRIOR_VALUE),
        selectors,
        ..ImageSet::default()
    };
    (set.is_prior, set.prior_index) = prior_selection(&set.abstract_prior, &set.relative_time);
    set
}
fn prior_selection(abstract_prior: &[i64], relative: &[i64]) -> (bool, i64) {
    let mut best: i64 = 0;
    for &value in abstract_prior {
        if value != 0 {
            let value = value.wrapping_abs();
            if best == 0 || value < best {
                best = value;
            }
        }
    }
    if best > 0 {
        return (true, best - 1);
    }
    (relative.iter().any(|&value| value != 0), 0)
}
fn spatial_position_check(position: &[f64], path: String, diagnostics: &mut Vec<Diagnostic>) {
    if !position.is_empty() && position.len() != 4 {
        diagnostics.push(diagnostic(
            path,
            "invalid-spatial-position",
            "Display Environment Spatial Position must contain four values",
        ));
    }
}
fn parse_screens(obj: &Object, p: &mut Protocol) -> Result<(), Error> {
    let items = sequence(obj, TAG_NOMINAL_SCREEN_DEFINITION_SEQUENCE);
    if items.len() > MAX_SCREENS {
        return Err(bound_error("screens", items.len(), MAX_SCREENS));
    }
    for (i, item) in items.iter().enumerate() {
        let screen = Screen {
            vertical_pixels: first_int(item, TAG_NUMBER_OF_VERTICAL_PIXELS),
            horizontal_pixels: first_int(item, TAG_NUMBER_OF_HORIZONTAL_PIXELS),
            spatial_position: derivedio::floats(
                Some(item),
                TAG_DISPLAY_ENVIRONMENT_SPATIAL_POSITION,
            )
            .unwrap_or_default(),
        };
        spatial_position_check(
            &screen.spatial_position,
            format!("screens[{i}].spatialPosition"),
            &mut p.diagnostics,
        );
        p.screens.push(screen);
    }
    Ok(())
}
fn parse_display_sets(obj: &Object, p: &mut Protocol) -> Result<(), Error> {
    let items = sequence(obj, TAG_DISPLAY_SETS_SEQUENCE);
    if items.len() > MAX_DISPLAY_SETS {
        return Err(bound_error("displaySets", items.len(), MAX_DISPLAY_SETS));
    }
    let mut total_boxes = 0;
    for (i, item) in items.iter().enumerate() {
        let path = format!("displaySets[{i}]");
        let mut display = DisplaySet {
            number: first_int(item, TAG_DISPLAY_SET_NUMBER),
            label: clean_text(item, TAG_DISPLAY_SET_LABEL),
            presentation_group: first_int(item, TAG_DISPLAY_SET_PRESENTATION_GROUP),
            image_set_number: first_int(item, TAG_IMAGE_SET_NUMBER),
            partial_data_handling: clean_text(item, TAG_PARTIAL_DATA_DISPLAY_HANDLING),
            voi_type: clean_text(item, TAG_VOI_TYPE),
            patient_orientation: clean_texts(item, TAG_DISPLAY_SET_PATIENT_ORIENTATION),
            show_inverted: is_yes(&clean_text(item, TAG_SHOW_GRAYSCALE_INVERTED)),
            image_boxes: Vec::new(),
        };
        let boxes = sequence(item, TAG_IMAGE_BOXES_SEQUENCE);
        total_boxes += boxes.len();
        if total_boxes > MAX_IMAGE_BOXES {
            return Err(bound_error("imageBoxes", total_boxes, MAX_IMAGE_BOXES));
        }
        if item.has(TAG_FILTER_OPERATIONS_SEQUENCE) {
            p.diagnostics.push(unsupported(
                format!("{path}.filterOperations"),
                "filter operations",
            ));
        }
        for (tag, name) in [
            (TAG_BLENDING_OPERATION_TYPE, "blending operation"),
            (TAG_REFORMATTING_OPERATION_TYPE, "reformatting operation"),
            (TAG_THREE_D_RENDERING_TYPE, "3D rendering"),
            (TAG_SORTING_OPERATIONS_SEQUENCE, "sorting operation"),
            (TAG_PSEUDO_COLOR_TYPE, "pseudo-color presentation"),
        ] {
            if item.has(tag) {
                p.diagnostics.push(unsupported(path.clone(), name));
            }
        }
        for (j, box_item) in boxes.iter().enumerate() {
            let mut image_box = ImageBox {
                number: first_int(box_item, TAG_IMAGE_BOX_NUMBER),
                image_set_number: first_int(box_item, TAG_IMAGE_SET_NUMBER),
                layout_type: clean_text(box_item, TAG_IMAGE_BOX_LAYOUT_TYPE),
                tile_columns: first_int(box_item, TAG_IMAGE_BOX_TILE_HORIZONTAL_DIMENSION),
                tile_rows: first_int(box_item, TAG_IMAGE_BOX_TILE_VERTICAL_DIMENSION),
                spatial_position: derivedio::floats(
                    Some(box_item),
                    TAG_DISPLAY_ENVIRONMENT_SPATIAL_POSITION,
                )
                .unwrap_or_default(),
                voi_type: clean_text(box_item, TAG_VOI_TYPE),
                patient_orientation: clean_texts(box_item, TAG_DISPLAY_SET_PATIENT_ORIENTATION),
                show_inverted: is_yes(&clean_text(box_item, TAG_SHOW_GRAYSCALE_INVERTED)),
            };
            if image_box.image_set_number == 0 {
                image_box.image_set_number = display.image_set_number;
            }
            if image_box.voi_type.is_empty() {
                image_box.voi_type = display.voi_type.clone();
            }
            if image_box.patient_orientation.is_empty() {
                image_box.patient_orientation = display.patient_orientation.clone();
            }
            if !box_item.has(TAG_SHOW_GRAYSCALE_INVERTED) {
                image_box.show_inverted = display.show_inverted;
            }
            spatial_position_check(
                &image_box.spatial_position,
                format!("{path}.imageBoxes[{j}].spatialPosition"),
                &mut p.diagnostics,
            );
            display.image_boxes.push(image_box);
        }
        p.display_sets.push(display);
    }
    parse_sync_groups_at(obj, "protocol", p)?;
    for (i, item) in items.iter().enumerate() {
        parse_sync_groups_at(item, &format!("displaySets[{i}]"), p)?;
    }
    Ok(())
}
fn parse_sync_groups_at(obj: &Object, path: &str, p: &mut Protocol) -> Result<(), Error> {
    let scroll = sequence(obj, TAG_SYNCHRONIZED_SCROLLING_SEQUENCE);
    let boxes = sequence(obj, TAG_IMAGE_BOX_SYNCHRONIZATION_SEQUENCE);
    if scroll.len() + boxes.len() > MAX_DISPLAY_SETS {
        return Err(bound_error(
            &format!("{path}.synchronization"),
            scroll.len() + boxes.len(),
            MAX_DISPLAY_SETS,
        ));
    }
    for (i, item) in scroll.iter().enumerate() {
        let here = format!("{path}.synchronizedScrolling[{i}]");
        let count =
            derivedio::ints(Some(item), TAG_DISPLAY_SET_SCROLLING_GROUP).map_or(0, |v| v.len());
        if count > MAX_VALUES {
            return Err(bound_error(&here, count, MAX_VALUES));
        }
        let group = SyncGroup {
            display_sets: ints(item, TAG_DISPLAY_SET_SCROLLING_GROUP),
            image_boxes: Vec::new(),
            r#type: "SCROLL".to_string(),
        };
        if group.display_sets.len() < 2 {
            p.diagnostics.push(diagnostic(
                here,
                "incomplete-sync-group",
                "synchronized scrolling group must reference at least two display sets",
            ));
        }
        p.sync_groups.push(group);
    }
    for (i, item) in boxes.iter().enumerate() {
        let here = format!("{path}.imageBoxSynchronization[{i}]");
        let count =
            derivedio::ints(Some(item), TAG_SYNCHRONIZED_IMAGE_BOX_LIST).map_or(0, |v| v.len());
        if count > MAX_VALUES {
            return Err(bound_error(&here, count, MAX_VALUES));
        }
        let group = SyncGroup {
            display_sets: Vec::new(),
            image_boxes: ints(item, TAG_SYNCHRONIZED_IMAGE_BOX_LIST),
            r#type: to_upper(&clean_text(item, TAG_TYPE_OF_SYNCHRONIZATION)),
        };
        if group.image_boxes.len() < 2 || group.r#type.is_empty() {
            p.diagnostics.push(diagnostic(
                here,
                "incomplete-sync-group",
                "image-box synchronization requires a type and at least two image boxes",
            ));
        }
        p.sync_groups.push(group);
    }
    Ok(())
}
fn parse_selector(obj: &Object, path: &str, diagnostics: &mut Vec<Diagnostic>) -> Selector {
    let mut selector = Selector {
        usage_flag: to_upper(&clean_text(obj, TAG_IMAGE_SET_SELECTOR_USAGE_FLAG)),
        value_number: first_int(obj, TAG_SELECTOR_VALUE_NUMBER),
        attribute_vr: to_upper(&clean_text(obj, TAG_SELECTOR_ATTRIBUTE_VR)),
        attribute_private_creator: clean_text(obj, TAG_SELECTOR_ATTRIBUTE_PRIVATE_CREATOR),
        ..Selector::default()
    };
    if let Some(&first) = tag_values(obj, TAG_SELECTOR_ATTRIBUTE).first() {
        selector.attribute = first;
    }
    selector.sequence_pointer = tag_values(obj, TAG_SELECTOR_SEQUENCE_POINTER);
    selector.values = selector_values(obj, &selector.attribute_vr);
    if selector.attribute.is_zero() {
        diagnostics.push(diagnostic(
            path,
            "missing-selector-attribute",
            "selector has no valid Selector Attribute",
        ));
    }
    if selector.attribute_vr.is_empty() {
        diagnostics.push(diagnostic(
            path,
            "missing-selector-vr",
            "selector has no Selector Attribute VR",
        ));
    }
    if selector.values.is_empty() {
        diagnostics.push(diagnostic(
            path,
            "missing-selector-value",
            "selector has no decodable value",
        ));
    }
    if !selector.sequence_pointer.is_empty() {
        diagnostics.push(unsupported(
            format!("{path}.sequencePointer"),
            "nested selector sequence pointer",
        ));
    }
    if !selector.attribute_private_creator.is_empty() || selector.attribute.is_private() {
        diagnostics.push(unsupported(path, "private selector attribute"));
    }
    if !selector.usage_flag.is_empty() && selector.usage_flag != "MATCH" {
        diagnostics.push(unsupported(
            format!("{path}.usageFlag"),
            &format!("selector usage {}", selector.usage_flag),
        ));
    }
    if selector.value_number > selector.values.len() as i64 && !selector.values.is_empty() {
        diagnostics.push(diagnostic(
            format!("{path}.valueNumber"),
            "invalid-selector-value-number",
            "Selector Value Number exceeds available values",
        ));
    }
    if let Some(message) = selector_value_bounds_message(obj, &selector.attribute_vr) {
        diagnostics.push(diagnostic(
            format!("{path}.values"),
            "selector-value-bounds",
            &message,
        ));
    }
    selector
}
fn selector_values(obj: &Object, vr: &str) -> Vec<String> {
    let Some(tag) = selector_value_tag(vr) else {
        return Vec::new();
    };
    if vr == "AT" {
        return tag_values(obj, tag).iter().map(Tag::to_string).collect();
    }
    clean_texts(obj, tag)
}
fn selector_value_tag(vr: &str) -> Option<Tag> {
    let element = match vr {
        "AE" => 0x005E,
        "AS" => 0x005F,
        "AT" => 0x0060,
        "DA" => 0x0061,
        "CS" => 0x0062,
        "DT" => 0x0063,
        "IS" => 0x0064,
        "LO" => 0x0066,
        "PN" => 0x006A,
        "TM" => 0x006B,
        "SH" => 0x006C,
        "ST" => 0x006E,
        "UC" => 0x006F,
        "UT" => 0x0070,
        "UR" => 0x0071,
        "DS" => 0x0072,
        "UI" => 0x007F,
        _ => return None,
    };
    Some(Tag::new(0x0072, element))
}
fn selector_value_bounds_message(obj: &Object, vr: &str) -> Option<String> {
    let tag = selector_value_tag(vr)?;
    let over = |count: usize| {
        (count > MAX_VALUES)
            .then(|| format!("selector contains {count} values (limit {MAX_VALUES})"))
    };
    if vr == "AT" {
        let raw = obj.get(tag)?.raw()?;
        return if raw.len() % 4 == 0 {
            over(raw.len() / 4)
        } else {
            None
        };
    }
    let values = obj.get_strings(tag)?;
    if let Some(message) = over(values.len()) {
        return Some(message);
    }
    values
        .iter()
        .any(|value| value.len() > MAX_TEXT_BYTES)
        .then(|| format!("selector value exceeds {MAX_TEXT_BYTES} bytes"))
}
fn truncate_text(mut value: String) -> String {
    if value.len() > MAX_TEXT_BYTES {
        let mut end = MAX_TEXT_BYTES;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    value
}
fn clean_text(obj: &Object, tag: Tag) -> String {
    truncate_text(derivedio::clean_string(obj, tag))
}
fn clean_texts(obj: &Object, tag: Tag) -> Vec<String> {
    let Some(values) = obj.get_strings(tag) else {
        return Vec::new();
    };
    values
        .into_iter()
        .take(MAX_VALUES)
        .map(|value| truncate_text(value.trim_matches([' ', '\0']).to_string()))
        .filter(|value| !value.is_empty())
        .collect()
}
fn first_int(obj: &Object, tag: Tag) -> i64 {
    ints(obj, tag).first().copied().unwrap_or(0)
}
fn ints(obj: &Object, tag: Tag) -> Vec<i64> {
    let mut values = derivedio::ints(Some(obj), tag).unwrap_or_default();
    values.truncate(MAX_VALUES);
    values
}
fn tag_values(obj: &Object, tag: Tag) -> Vec<Tag> {
    let Some(elem) = obj.get(tag) else {
        return Vec::new();
    };
    if elem.vr() != Vr::AT {
        return Vec::new();
    }
    let Value::Raw(raw) = &elem.value else {
        return Vec::new();
    };
    if raw.len() % 4 != 0 {
        return Vec::new();
    }
    let big = obj.value_big_endian();
    let word = |b: &[u8]| {
        if big {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    };
    raw.as_chunks::<4>()
        .0
        .iter()
        .take(MAX_VALUES)
        .map(|b| Tag::new(word(&b[..2]), word(&b[2..])))
        .collect()
}
fn is_yes(value: &str) -> bool {
    matches!(to_upper(value.trim()).as_str(), "YES" | "Y" | "TRUE")
}
