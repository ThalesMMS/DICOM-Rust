use super::pixeldata::{extract_metadata, native_packed_byte_count, total_size};
use crate::object::{DataSet, Element, Object, Tag, Value};
use crate::validation::{CODE_PIXEL_METADATA, DataSetContext, DataSetRule, RuleFinding};
use std::sync::Arc;
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
pub fn metadata_validation_rule() -> DataSetRule {
    Arc::new(metadata_findings)
}
fn finding(tag: Tag) -> RuleFinding {
    RuleFinding {
        tag,
        code: Some(CODE_PIXEL_METADATA),
        ..RuleFinding::default()
    }
}
fn find_element(ds: &DataSet, tag: Tag) -> Option<&Element> {
    ds.elements.iter().rev().find(|e| e.tag() == tag)
}
#[doc(hidden)]
pub fn metadata_findings(ctx: &DataSetContext) -> Vec<RuleFinding> {
    let Some(pixel) = find_element(&ctx.data_set, TAG_PIXEL_DATA) else {
        return Vec::new();
    };
    let mut obj = Object::from_data_set(ctx.data_set.clone());
    obj.set_value_big_endian(ctx.big_endian);
    let Ok(m) = extract_metadata(&obj) else {
        return vec![finding(TAG_PIXEL_DATA)];
    };
    let mut findings = Vec::new();
    let mut check = |bad: bool, tag: Tag| {
        if bad {
            findings.push(finding(tag));
        }
    };
    check(m.rows == 0, TAG_ROWS);
    check(m.columns == 0, TAG_COLUMNS);
    check(m.samples_per_pixel == 0, TAG_SAMPLES_PER_PIXEL);
    check(
        m.bits_allocated != 1
            && (m.bits_allocated == 0 || m.bits_allocated > 64 || m.bits_allocated % 8 != 0),
        TAG_BITS_ALLOCATED,
    );
    check(
        m.bits_stored == 0 || m.bits_stored > m.bits_allocated,
        TAG_BITS_STORED,
    );
    check(
        m.bits_stored > 0
            && (m.high_bit < m.bits_stored.saturating_sub(1) || m.high_bit >= m.bits_allocated),
        TAG_HIGH_BIT,
    );
    check(m.pixel_representation > 1, TAG_PIXEL_REPRESENTATION);
    check(
        m.samples_per_pixel > 1 && (!m.planar_configuration_present || m.planar_configuration > 1),
        TAG_PLANAR_CONFIGURATION,
    );
    if let Value::Raw(raw) = &pixel.value {
        let expected = if m.bits_allocated == 1 {
            native_packed_byte_count(&m)
        } else {
            total_size(&m)
        };
        let actual = raw.len() as i64;
        check(
            expected <= 0 || actual < expected || actual.saturating_sub(expected) > 1,
            TAG_PIXEL_DATA,
        );
    }
    findings
}
