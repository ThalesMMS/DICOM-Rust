use super::condition::parse_condition_full;
use super::external::ConditionState;
use super::functional_groups::{
    FG_PER_FRAME, FG_SHARED, default_options, fg_integer, fg_primitive_within, fg_raw_sequence,
};
use super::modules::RuleSource;
use crate::object::{DataSet, Element, Object, Tag, Value, Vr};
pub(super) const SEG_FRAME_OF_REFERENCE_RULE: &str = "table_A.51-1/row-9/index-8";
type Eval = fn(&Object, &Object) -> ConditionState;
fn priority(rule: &str) -> Option<Eval> {
    Some(match rule {
        "table_C.8-3/row-53/index-52" | "table_C.8-3/row-62/index-61" => ct_energy_weighting,
        "table_C.8-114/row-5/index-4" | "table_C.8-114/row-6/index-5" => enhanced_ct_acquisition,
        "table_C.8-4/row-11/index-10" => mr_repetition_time,
        "table_C.8-4/row-14/index-13" => mr_inversion_time,
        "table_C.8-4/row-15/index-14" => mr_trigger_time,
        "table_C.8.20-2/row-13/index-12" | "table_C.8.20-2/row-14/index-13" => seg_fractional,
        "table_C.8.20-2/row-18/index-17" => seg_algorithm_name,
        "table_A.38-1/row-20/index-19" | "table_A.36-1/row-21/index-20" => supplemental_palette,
        SEG_FRAME_OF_REFERENCE_RULE => seg_frame_of_reference,
        "table_A.51-1/row-23/index-22" => seg_common_instance_reference,
        _ => return None,
    })
}
pub(super) fn has_priority(rule: &str) -> bool {
    priority(rule).is_some()
}
pub(super) fn dataset_condition(
    text: &str,
    source: &RuleSource,
    item: &Object,
    root: &Object,
) -> (ConditionState, bool) {
    if let Some(evaluate) = priority(source.rule) {
        return (evaluate(item, root), false);
    }
    let (c, ok) = parse_condition_full(text);
    if ok {
        return (ConditionState::from_bool(c.eval(item, root)), c.forbidden);
    }
    (ConditionState::Unknown, false)
}
pub(super) fn condition_strings(element: Option<&Element>, vr: Vr) -> Vec<String> {
    match element {
        Some(e) if e.vr() == vr && fg_primitive_within(e, 256) => super::super::string_values(e),
        _ => Vec::new(),
    }
}
fn condition_cs(obj: &Object, group: u16, element: u16) -> Vec<String> {
    condition_strings(obj.get(Tag::new(group, element)), Vr::CS)
}
fn condition_single(values: &[String]) -> String {
    if values.len() != 1 {
        return String::new();
    }
    values[0].trim().to_string()
}
fn ct_energy_weighting(_: &Object, root: &Object) -> ConditionState {
    let tag = Tag::new(0x0008, 0x9215);
    if !root.has(tag) {
        return ConditionState::False;
    }
    let defaults = default_options();
    let Some(items) = fg_raw_sequence(root, tag) else {
        return ConditionState::Unknown;
    };
    if items.is_empty() || items.len() as i64 > defaults.max_items {
        return ConditionState::Unknown;
    }
    let mut state = ConditionState::False;
    let mut remaining = defaults.max_elements;
    for item in items {
        remaining -= item.elements.len() as i64;
        if remaining < 0 {
            return ConditionState::Unknown;
        }
        let (mut code, mut scheme) = (String::new(), String::new());
        let (mut code_count, mut scheme_count) = (0, 0);
        for e in &item.elements {
            if e.tag() == Tag::new(0x0008, 0x0100) {
                code = condition_single(&condition_strings(Some(e), Vr::SH));
                code_count += 1;
            } else if e.tag() == Tag::new(0x0008, 0x0102) {
                scheme = condition_single(&condition_strings(Some(e), Vr::SH));
                scheme_count += 1;
            }
        }
        if code_count != 1 || scheme_count != 1 {
            state = ConditionState::Unknown;
            continue;
        }
        if code == "113097" && scheme == "DCM" {
            return ConditionState::True;
        }
        if code.is_empty() || scheme.is_empty() {
            state = ConditionState::Unknown;
        }
    }
    state
}
fn enhanced_ct_acquisition(_: &Object, root: &Object) -> ConditionState {
    let image_type = condition_cs(root, 0x0008, 0x0008);
    let uid = condition_single(&condition_strings(
        root.get(Tag::new(0x0008, 0x0016)),
        Vr::UI,
    ));
    if uid == "1.2.840.10008.5.1.4.1.1.2.2" {
        return ConditionState::False;
    }
    if image_type.is_empty() {
        return ConditionState::Unknown;
    }
    match image_type[0].trim() {
        "DERIVED" => return ConditionState::False,
        "ORIGINAL" | "MIXED" => {
            if matches!(
                uid.as_str(),
                "1.2.840.10008.5.1.4.1.1.2.1"
                    | "1.2.840.10008.5.1.4.1.1.2.4"
                    | "1.2.840.10008.5.1.4.1.1.2.5"
            ) {
                return ConditionState::True;
            }
        }
        _ => {}
    }
    ConditionState::Unknown
}
const SCANNING_SEQUENCE_TERMS: [&str; 5] = ["SE", "IR", "GR", "EP", "RM"];
fn mr_repetition_time(_: &Object, root: &Object) -> ConditionState {
    let variant = match_mr_codes(
        &condition_cs(root, 0x0018, 0x0021),
        &["SK"],
        &["MTC", "SS", "TRSS", "SP", "MP", "OSP", "NONE"],
    );
    if variant == ConditionState::True {
        return ConditionState::True;
    }
    let scanning = condition_cs(root, 0x0018, 0x0020);
    let includes_ep = match_mr_codes(&scanning, &["EP"], &SCANNING_SEQUENCE_TERMS);
    if includes_ep == ConditionState::False {
        ConditionState::True
    } else if scanning.len() == 1
        && includes_ep == ConditionState::True
        && variant == ConditionState::False
    {
        ConditionState::False
    } else {
        ConditionState::Unknown
    }
}
fn mr_inversion_time(_: &Object, root: &Object) -> ConditionState {
    match_mr_codes(
        &condition_cs(root, 0x0018, 0x0020),
        &["IR"],
        &SCANNING_SEQUENCE_TERMS,
    )
}
fn mr_trigger_time(_: &Object, root: &Object) -> ConditionState {
    match_mr_codes(
        &condition_cs(root, 0x0018, 0x0022),
        &["CG", "PPG"],
        &["PER", "RG", "FC", "PFF", "PFP", "SP", "FS"],
    )
}
fn match_mr_codes(values: &[String], required: &[&str], other: &[&str]) -> ConditionState {
    if values.is_empty() {
        return ConditionState::Unknown;
    }
    let mut state = ConditionState::False;
    for value in values {
        let value = value.trim();
        if required.contains(&value) {
            return ConditionState::True;
        }
        if !other.contains(&value) {
            state = ConditionState::Unknown;
        }
    }
    state
}
fn supplemental_palette(_: &Object, root: &Object) -> ConditionState {
    match condition_single(&condition_cs(root, 0x0008, 0x9205)).as_str() {
        "COLOR" | "MIXED" => ConditionState::True,
        "MONOCHROME" | "TRUE_COLOR" => ConditionState::False,
        _ => ConditionState::Unknown,
    }
}
fn seg_fractional(_: &Object, root: &Object) -> ConditionState {
    match condition_single(&condition_cs(root, 0x0062, 0x0001)).as_str() {
        "FRACTIONAL" => ConditionState::True,
        "BINARY" | "LABELMAP" => ConditionState::False,
        _ => ConditionState::Unknown,
    }
}
fn seg_algorithm_name(item: &Object, _: &Object) -> ConditionState {
    match condition_single(&condition_cs(item, 0x0062, 0x0008)).as_str() {
        "AUTOMATIC" | "SEMIAUTOMATIC" => ConditionState::True,
        "MANUAL" => ConditionState::False,
        _ => ConditionState::Unknown,
    }
}
fn seg_frame_of_reference(_: &Object, root: &Object) -> ConditionState {
    let (seen, all) = seg_derivation_groups(root);
    if all == ConditionState::True {
        ConditionState::False
    } else if seen == ConditionState::False {
        ConditionState::True
    } else {
        ConditionState::Unknown
    }
}
fn seg_common_instance_reference(_: &Object, root: &Object) -> ConditionState {
    seg_derivation_groups(root).0
}
fn seg_derivation_groups(root: &Object) -> (ConditionState, ConditionState) {
    use ConditionState::*;
    let defaults = default_options();
    let frames = fg_integer(root, Tag::new(0x0028, 0x0008));
    let shared = fg_raw_sequence(root, FG_SHARED);
    let per_frame = fg_raw_sequence(root, FG_PER_FRAME);
    let Some(frames) = frames else {
        return (Unknown, Unknown);
    };
    let Some(shared) = shared else {
        return (Unknown, Unknown);
    };
    if frames <= 0
        || shared.len() != 1
        || frames > defaults.max_frames
        || root.has(FG_PER_FRAME) && per_frame.as_ref().is_none_or(|p| p.len() as i64 != frames)
    {
        return (Unknown, Unknown);
    }
    let mut remaining = defaults.max_elements - shared[0].elements.len() as i64;
    if remaining < 0 {
        return (Unknown, Unknown);
    }
    let in_shared = seg_derivation_group(&shared[0]);
    if in_shared == Unknown {
        return (Unknown, Unknown);
    }
    let mut present = 0;
    for frame in per_frame.unwrap_or_default() {
        remaining -= frame.elements.len() as i64;
        if remaining < 0 {
            return (Unknown, Unknown);
        }
        let state = seg_derivation_group(&frame);
        if state == Unknown || (state == True && in_shared == True) {
            return (Unknown, Unknown);
        }
        if state == True {
            present += 1;
        }
    }
    if in_shared == True || present == frames {
        return (True, True);
    }
    if present != 0 {
        return (True, False);
    }
    (False, False)
}
fn seg_derivation_group(group: &DataSet) -> ConditionState {
    let mut state = ConditionState::False;
    for e in &group.elements {
        if e.tag() != Tag::new(0x0008, 0x9124) {
            continue;
        }
        let valid = matches!(e.value, Value::Sequence(_));
        if e.vr() != Vr::SQ || !valid || state == ConditionState::True {
            return ConditionState::Unknown;
        }
        state = ConditionState::True;
    }
    state
}
