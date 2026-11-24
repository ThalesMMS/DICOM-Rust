use super::{
    Error, Frame, Kind, Map, Mapping, PayloadKind, ReadOptions, TAG_CODE_MEANING, TAG_CODE_VALUE,
    TAG_CODING_SCHEME_DESIGNATOR, TAG_CODING_SCHEME_VERSION, TAG_DOUBLE_RWV_FIRST_MAPPED,
    TAG_DOUBLE_RWV_LAST_MAPPED, TAG_MEASUREMENT_UNITS_CODE_SEQ, TAG_QUANTITY_DEFINITION_SEQ,
    TAG_REAL_WORLD_VALUE_MAPPING_SEQ, TAG_RWV_FIRST_MAPPED, TAG_RWV_INTERCEPT, TAG_RWV_LAST_MAPPED,
    TAG_RWV_LUT_DATA, TAG_RWV_SLOPE, TAG_SHARED_FUNCTIONAL_GROUPS, finite, read_code,
    read_quantity, trim_unicode_space,
};
use crate::derivation::string_values;
use crate::derivedio as dio;
use crate::fmath;
use crate::object::{Object, Tag, Vr, element_length};
const MAX_MAPPINGS_PER_FRAME: usize = 256;
const MAX_MAPPING_LUT_VALUES: i64 = 4 << 20;
pub(crate) const TAG_LUT_LABEL: Tag = Tag::new(0x0040, 0x9210);
#[derive(Clone, Default)]
struct FrameMappings {
    mappings: Vec<Mapping>,
    quantity: Vec<Object>,
}
pub(super) fn read_frame_mappings(
    obj: &Object,
    per_frame: &[Object],
    m: &mut Map,
    options: &ReadOptions,
) -> Result<(), Error> {
    let shared_groups = dio::sequence(obj, TAG_SHARED_FUNCTIONAL_GROUPS);
    if shared_groups.len() > 1 {
        return Err(Error::wrap(
            Kind::InvalidObject,
            "ambiguous functional groups",
        ));
    }
    let shared = shared_groups.first();
    let signed =
        m.payload != PayloadKind::Integer || dio::int(obj, dio::TAG_PIXEL_REPRESENTATION) == 1;
    let label = options.mapping_label.trim_end_matches(' ');
    let mut remaining = MAX_MAPPING_LUT_VALUES;
    let mut shared_mappings: Option<FrameMappings> = None;
    let mut top_mappings: Option<FrameMappings> = None;
    let mut first = FrameMappings::default();
    for index in 0..m.frames.len() {
        let group = per_frame.get(index);
        let frame_err = |e: Error| e.prefixed(format!("frame {}", index + 1));
        let shared_has = shared.is_some_and(|s| s.has(TAG_REAL_WORLD_VALUE_MAPPING_SEQ));
        let resolved =
            if let Some(group) = group.filter(|g| g.has(TAG_REAL_WORLD_VALUE_MAPPING_SEQ)) {
                if shared_has {
                    return Err(Error::sentinel(Kind::FunctionalGroupConflict)
                        .under(Kind::InvalidObject)
                        .prefixed(format!("frame {}", index + 1)));
                }
                decode_frame_mappings(group, m.payload, signed, label, &mut remaining)
                    .map_err(frame_err)?
            } else if let Some(shared) = shared.filter(|_| shared_has) {
                if shared_mappings.is_none() {
                    shared_mappings = Some(
                        decode_frame_mappings(shared, m.payload, signed, label, &mut remaining)
                            .map_err(frame_err)?,
                    );
                }
                shared_mappings.clone().unwrap_or_default()
            } else {
                if top_mappings.is_none() {
                    top_mappings = Some(
                        decode_frame_mappings(obj, m.payload, signed, label, &mut remaining)
                            .map_err(frame_err)?,
                    );
                }
                top_mappings.clone().unwrap_or_default()
            };
        let mut primary = resolved.mappings[0].clone();
        if index == 0 {
            m.units = primary.units.clone();
            m.quantity = primary.quantity.clone();
            first = resolved.clone();
        } else if !same_units(&primary, &first.mappings[0])
            || !same_quantity(&resolved.quantity, &first.quantity)
        {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                "units or quantity vary across frames",
            ));
        }
        primary.units = m.units.clone();
        primary.quantity = m.quantity.clone();
        let frame: &mut Frame = &mut m.frames[index];
        frame.mapping = primary;
        frame.additional_mappings = resolved.mappings[1..].to_vec();
    }
    Ok(())
}
fn decode_frame_mappings(
    container: &Object,
    payload: PayloadKind,
    signed: bool,
    label: &str,
    remaining: &mut i64,
) -> Result<FrameMappings, Error> {
    let mut items = dio::sequence(container, TAG_REAL_WORLD_VALUE_MAPPING_SEQ);
    if items.is_empty() {
        return Err(Error::sentinel(Kind::MissingMapping));
    }
    if items.len() > MAX_MAPPINGS_PER_FRAME {
        return Err(Error::wrap(Kind::MemoryLimit, "mapping item count"));
    }
    if !label.is_empty() {
        let mut selected: Option<usize> = None;
        for (i, item) in items.iter().enumerate() {
            let Some(item_label) = mapping_label(item) else {
                return Err(Error::wrap(Kind::MissingMapping, "invalid LUT Label"));
            };
            if item_label == label {
                if selected.is_some() {
                    return Err(Error::wrap(
                        Kind::AmbiguousMapping,
                        "duplicate selected LUT Label",
                    ));
                }
                selected = Some(i);
            }
        }
        let Some(selected) = selected else {
            return Err(Error::wrap(
                Kind::MissingMapping,
                "selected LUT Label absent",
            ));
        };
        items = vec![items.swap_remove(selected)];
    }
    let mut out = FrameMappings::default();
    for (index, item) in items.iter().enumerate() {
        let mapping = decode_mapping_item(item, payload, signed, remaining)
            .map_err(|e| e.prefixed(format!("mapping item {}", index + 1)))?;
        let quantity = dio::sequence(item, TAG_QUANTITY_DEFINITION_SEQ);
        if index == 0 {
            out.quantity = quantity;
        } else if !same_units(&out.mappings[0], &mapping)
            || !same_quantity(&out.quantity, &quantity)
        {
            return Err(Error::wrap(
                Kind::AmbiguousMapping,
                "incompatible units or quantity",
            ));
        }
        out.mappings.push(mapping);
    }
    if out.mappings.len() > 1 {
        sort_by_first_value(&mut out.mappings);
        for i in 0..out.mappings.len() {
            if !out.mappings[i].has_range
                || (i > 0 && out.mappings[i - 1].last_value >= out.mappings[i].first_value)
            {
                return Err(Error::wrap(
                    Kind::AmbiguousMapping,
                    "intersecting or unspecified mapping domains",
                ));
            }
        }
    }
    Ok(out)
}
fn sort_by_first_value(mappings: &mut [Mapping]) {
    mappings.sort_by(|a, b| {
        a.first_value
            .partial_cmp(&b.first_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
fn decode_mapping_item(
    item: &Object,
    payload: PayloadKind,
    signed: bool,
    remaining: &mut i64,
) -> Result<Mapping, Error> {
    let units = dio::sequence(item, TAG_MEASUREMENT_UNITS_CODE_SEQ);
    if units.len() != 1 {
        return Err(Error::wrap(
            Kind::MissingMapping,
            "expected one measurement units item",
        ));
    }
    let mut mapping = Mapping {
        units: read_code(Some(&units[0])),
        quantity: read_quantity(item),
        units_version: dio::clean_string(&units[0], TAG_CODING_SCHEME_VERSION),
        ..Mapping::default()
    };
    if !valid_units_code(&units[0])
        || mapping.units.value.is_empty()
        || mapping.units.scheme.is_empty()
    {
        return Err(Error::wrap(
            Kind::MissingMapping,
            "incomplete measurement units code",
        ));
    }
    if item.has(TAG_QUANTITY_DEFINITION_SEQ)
        && item.get_sequence(TAG_QUANTITY_DEFINITION_SEQ).is_none()
    {
        return Err(Error::wrap(
            Kind::MissingMapping,
            "malformed quantity definition",
        ));
    }
    let Some(label) = mapping_label(item) else {
        return Err(Error::wrap(Kind::MissingMapping, "invalid LUT Label"));
    };
    mapping.label = label;
    let Some(range) = mapping_range(item, payload, signed) else {
        return Err(Error::wrap(
            Kind::MissingMapping,
            "invalid stored-value domain",
        ));
    };
    if let Some((first, last)) = range {
        mapping.first_value = first;
        mapping.last_value = last;
        mapping.has_range = true;
    }
    if let Some(lut) = item.get(TAG_RWV_LUT_DATA) {
        if payload != PayloadKind::Integer
            || !mapping.has_range
            || item.has(TAG_RWV_SLOPE)
            || item.has(TAG_RWV_INTERCEPT)
            || lut.vr() != Vr::FD
        {
            return Err(Error::wrap(Kind::MissingMapping, "invalid lookup table"));
        }
        let count = fmath::f64_to_i64(mapping.last_value - mapping.first_value) + 1;
        if count > *remaining {
            return Err(Error::wrap(Kind::MemoryLimit, "mapping lookup values"));
        }
        if element_length(lut) != (count * 8) as u32 {
            return Err(Error::wrap(
                Kind::MissingMapping,
                "lookup table length differs from domain",
            ));
        }
        mapping.lut = item.get_floats(TAG_RWV_LUT_DATA).unwrap_or_default();
        if mapping.lut.len() as i64 != count {
            return Err(Error::wrap(Kind::MissingMapping, "unreadable lookup table"));
        }
        *remaining -= count;
    } else {
        let slope = mapping_fd(item, TAG_RWV_SLOPE);
        let intercept = mapping_fd(item, TAG_RWV_INTERCEPT);
        let (Some(slope), Some(intercept)) = (slope, intercept) else {
            return Err(Error::wrap(
                Kind::MissingMapping,
                "invalid linear coefficients",
            ));
        };
        mapping.slope = slope;
        mapping.intercept = intercept;
    }
    Ok(mapping)
}
fn valid_units_code(item: &Object) -> bool {
    for tag in [
        TAG_CODE_VALUE,
        TAG_CODING_SCHEME_DESIGNATOR,
        TAG_CODING_SCHEME_VERSION,
    ] {
        let Some(elem) = item.get(tag) else {
            if tag == TAG_CODING_SCHEME_VERSION {
                continue;
            }
            return false;
        };
        if elem.vr() != Vr::SH || element_length(elem) > 16 {
            return false;
        }
        match item.lookup_strings(tag) {
            Ok(values) if values.len() == 1 && !trim_unicode_space(&values[0]).is_empty() => {}
            _ => return false,
        }
    }
    true
}
fn mapping_label(item: &Object) -> Option<String> {
    let Some(elem) = item.get(TAG_LUT_LABEL) else {
        return Some(String::new());
    };
    if elem.vr() != Vr::SH || element_length(elem) > 16 {
        return None;
    }
    match item.lookup_strings(TAG_LUT_LABEL) {
        Ok(values) if values.len() == 1 => Some(values[0].trim_end_matches(' ').to_string()),
        _ => None,
    }
}
#[allow(clippy::option_option)]
fn mapping_range(item: &Object, payload: PayloadKind, signed: bool) -> Option<Option<(f64, f64)>> {
    let has_float = item.has(TAG_DOUBLE_RWV_FIRST_MAPPED) || item.has(TAG_DOUBLE_RWV_LAST_MAPPED);
    let has_integer = item.has(TAG_RWV_FIRST_MAPPED) || item.has(TAG_RWV_LAST_MAPPED);
    if !has_float && !has_integer {
        return Some(None);
    }
    let (mut integer_first, mut integer_last) = (0.0, 0.0);
    if has_integer {
        integer_first = mapping_integer(item, TAG_RWV_FIRST_MAPPED, signed)?;
        integer_last = mapping_integer(item, TAG_RWV_LAST_MAPPED, signed)?;
        if integer_first > integer_last {
            return None;
        }
    }
    if !has_float {
        return Some(Some((integer_first, integer_last)));
    }
    if payload == PayloadKind::Integer {
        return None;
    }
    let first = mapping_fd(item, TAG_DOUBLE_RWV_FIRST_MAPPED)?;
    let last = mapping_fd(item, TAG_DOUBLE_RWV_LAST_MAPPED)?;
    if first > last || (has_integer && (first != integer_first || last != integer_last)) {
        return None;
    }
    Some(Some((first, last)))
}
fn mapping_integer(item: &Object, tag: Tag, signed: bool) -> Option<f64> {
    let elem = item.get(tag)?;
    if (elem.vr() != Vr::SS && elem.vr() != Vr::US) || element_length(elem) != 2 {
        return None;
    }
    let values = item.get_ints(tag).ok()?;
    if values.len() != 1 {
        return None;
    }
    if signed {
        return Some(f64::from(values[0] as i16));
    }
    (values[0] >= 0).then_some(values[0] as f64)
}
fn mapping_fd(item: &Object, tag: Tag) -> Option<f64> {
    let elem = item.get(tag)?;
    if elem.vr() != Vr::FD || element_length(elem) != 8 {
        return None;
    }
    let values = item.get_floats(tag).ok()?;
    (values.len() == 1 && finite(values[0])).then(|| values[0])
}
fn same_units(a: &Mapping, b: &Mapping) -> bool {
    a.units.value == b.units.value
        && a.units.scheme == b.units.scheme
        && a.units_version == b.units_version
}
fn same_quantity(a: &[Object], b: &[Object]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_quantity_item(x, y))
}
fn same_quantity_item(a: &Object, b: &Object) -> bool {
    let (left, right) = (a.elements(), b.elements());
    if left.len() != right.len() {
        return false;
    }
    for (elem, other) in left.iter().zip(right) {
        if elem.tag() != other.tag() || elem.vr() != other.vr() {
            return false;
        }
        if elem.tag() == TAG_CODE_MEANING {
            continue;
        }
        match elem.vr() {
            Vr::SQ => {
                if !same_quantity(&dio::sequence(a, elem.tag()), &dio::sequence(b, elem.tag())) {
                    return false;
                }
            }
            Vr::SH | Vr::CS | Vr::LO | Vr::UI | Vr::UC | Vr::UR => {
                if string_values(elem) != string_values(other) {
                    return false;
                }
            }
            _ => {
                if elem.value != other.value {
                    return false;
                }
            }
        }
    }
    true
}
impl Frame {
    pub(crate) fn apply_mapping(&self, stored: f64) -> Result<f64, Error> {
        if self.additional_mappings.is_empty() || stored.is_nan() {
            return self.mapping.apply(stored);
        }
        if stored.is_infinite() {
            return Ok(stored);
        }
        if stored <= self.mapping.last_value {
            return self.mapping.apply(stored);
        }
        let index = self
            .additional_mappings
            .partition_point(|m| stored > m.last_value);
        if index == self.additional_mappings.len() {
            return Err(Error::sentinel(Kind::OutsideMapping));
        }
        self.additional_mappings[index].apply(stored)
    }
}
