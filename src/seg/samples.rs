use super::overlap::overlaps;
use super::{
    Document, Error, FRACTIONAL_TYPE_OCCUPANCY, FRACTIONAL_TYPE_PROBABILITY, Frame, Kind,
    LABEL_MAP_SEGMENTATION_STORAGE, SEGMENTATION_TYPE_FRACTIONAL, SEGMENTS_OVERLAP_NO,
    SEGMENTS_OVERLAP_YES, TAG_MAXIMUM_FRACTIONAL_VALUE, TAG_SEGMENTATION_FRACTIONAL_TYPE, invalid,
};
use crate::derivedio as dio;
use crate::fmath;
use crate::object::Object;
use crate::roi::RasterMask;
use crate::textfmt::{float_v, quote};
pub(crate) fn read_sample_layout(obj: &Object, doc: &mut Document) -> Result<(), Error> {
    let mut bits_allocated = dio::int(obj, dio::TAG_BITS_ALLOCATED);
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        if bits_allocated == 0 {
            bits_allocated = 16;
        }
        if bits_allocated != 8 && bits_allocated != 16 {
            return Err(Error::wrap(
                Kind::UnsupportedPixelData,
                format!("label map with Bits Allocated {bits_allocated}"),
            ));
        }
        doc.label_map_bits_allocated = bits_allocated;
    } else if doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
        if bits_allocated != 0 && bits_allocated != 8 {
            return Err(Error::wrap(
                Kind::UnsupportedPixelData,
                format!("FRACTIONAL segmentation with Bits Allocated {bits_allocated}"),
            ));
        }
        doc.fractional_type = dio::clean_string(obj, TAG_SEGMENTATION_FRACTIONAL_TYPE);
        if doc.fractional_type != FRACTIONAL_TYPE_PROBABILITY
            && doc.fractional_type != FRACTIONAL_TYPE_OCCUPANCY
        {
            return Err(invalid(format!(
                "Segmentation Fractional Type {}",
                quote(&doc.fractional_type)
            )));
        }
        doc.max_fractional_value = dio::int(obj, TAG_MAXIMUM_FRACTIONAL_VALUE);
        if doc.max_fractional_value < 1 || doc.max_fractional_value > 255 {
            return Err(invalid(format!(
                "Maximum Fractional Value {} is outside 1-255",
                doc.max_fractional_value
            )));
        }
    } else if bits_allocated != 0 && bits_allocated != 1 {
        return Err(Error::wrap(
            Kind::UnsupportedPixelData,
            format!("BINARY segmentation with Bits Allocated {bits_allocated}"),
        ));
    }
    Ok(())
}
pub const DEFAULT_FRACTIONAL_THRESHOLD: f64 = 0.5;
pub(crate) fn normalize_fractional_threshold(threshold: f64) -> Result<f64, Error> {
    if threshold == 0.0 {
        return Ok(DEFAULT_FRACTIONAL_THRESHOLD);
    }
    if !(threshold > 0.0 && threshold <= 1.0) {
        return Err(invalid(format!(
            "fractional threshold {} is outside (0, 1]",
            float_v(threshold)
        )));
    }
    Ok(threshold)
}
pub(crate) fn cutoff_value(max_value: i64, threshold: f64) -> i64 {
    fmath::f64_to_i64((threshold * max_value as f64 - 1e-9).ceil()).max(1)
}
pub fn fractional_cutoff(max_value: i64, threshold: f64) -> Result<i64, Error> {
    let threshold = normalize_fractional_threshold(threshold)?;
    if !(1..=255).contains(&max_value) {
        return Err(invalid(format!(
            "Maximum Fractional Value {max_value} is outside 1-255"
        )));
    }
    Ok(cutoff_value(max_value, threshold))
}
pub(crate) fn sample_mask(
    columns: i64,
    rows: i64,
    samples: &[u8],
    inside: impl Fn(u8) -> bool,
) -> RasterMask {
    let mut mask = RasterMask::new(columns, rows);
    for y in 0..rows {
        let row = y * columns;
        let mut run_start = -1;
        for x in 0..=columns {
            let index = (row + x) as usize;
            let is_inside = x < columns && index < samples.len() && inside(samples[index]);
            if is_inside && run_start < 0 {
                run_start = x;
            } else if !is_inside && run_start >= 0 {
                mask.set_run(y, run_start, x);
                run_start = -1;
            }
        }
    }
    mask
}
pub(crate) fn fractional_mask(
    doc: &Document,
    frame: &Frame,
    threshold: f64,
) -> Result<RasterMask, Error> {
    let cut = fractional_cutoff(doc.max_fractional_value, threshold)?;
    if doc.columns <= 0 {
        return Err(invalid("fractional frame has non-positive columns"));
    }
    Ok(sample_mask(doc.columns, doc.rows, &frame.fractional, |v| {
        i64::from(v) >= cut
    }))
}
pub(crate) fn segments_overlap_value(
    doc: &Document,
    sop_class_uid: &str,
) -> Result<&'static str, Error> {
    if sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        return Ok(SEGMENTS_OVERLAP_NO);
    }
    let report = overlaps(doc)?;
    Ok(if report.overlaps.is_empty() {
        SEGMENTS_OVERLAP_NO
    } else {
        SEGMENTS_OVERLAP_YES
    })
}
