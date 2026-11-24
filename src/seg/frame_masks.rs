use super::samples::fractional_mask;
use super::{
    Document, Error, Frame, LABEL_MAP_SEGMENTATION_STORAGE, RoiOptions,
    SEGMENTATION_TYPE_FRACTIONAL, SEGMENTATION_TYPE_LABEL_MAP, invalid,
};
use crate::roi::RasterMask;
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMask {
    pub segment_number: i64,
    pub mask: RasterMask,
}
#[derive(Debug)]
pub struct FrameMaskExtractor<'a> {
    doc: &'a Document,
    threshold: f64,
    pixels: i64,
    labels: HashSet<u16>,
}
impl<'a> FrameMaskExtractor<'a> {
    pub fn new(doc: &'a Document, opts: RoiOptions) -> Result<FrameMaskExtractor<'a>, Error> {
        if doc.columns <= 0 || doc.rows <= 0 || doc.columns > i64::MAX / doc.rows {
            return Err(invalid(format!(
                "Rows={} Columns={}",
                doc.rows, doc.columns
            )));
        }
        Ok(FrameMaskExtractor {
            doc,
            threshold: opts.fractional_threshold,
            pixels: doc.rows * doc.columns,
            labels: doc
                .segments
                .iter()
                .filter(|s| s.number > 0 && s.number <= 0xFFFF)
                .map(|s| s.number as u16)
                .collect(),
        })
    }
    pub fn masks(&self, frame: &Frame) -> Result<Vec<FrameMask>, Error> {
        let doc = self.doc;
        if !frame.label_map.is_empty()
            || doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE
            || doc.segmentation_type == SEGMENTATION_TYPE_LABEL_MAP
        {
            if frame.label_map.len() as i64 != self.pixels {
                return Err(invalid(format!(
                    "label-map frame has {} samples, want {}",
                    frame.label_map.len(),
                    self.pixels
                )));
            }
            return Ok(self.label_map_masks(&frame.label_map));
        }
        if !frame.fractional.is_empty() || doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
            if frame.fractional.is_empty() {
                return Ok(Vec::new());
            }
            if frame.fractional.len() as i64 != self.pixels {
                return Err(invalid(format!(
                    "fractional frame has {} samples, want {}",
                    frame.fractional.len(),
                    self.pixels
                )));
            }
            if frame.segment_number <= 0 {
                return Err(invalid(format!(
                    "fractional frame has segment number {}",
                    frame.segment_number
                )));
            }
            let mask = fractional_mask(doc, frame, self.threshold)?;
            if !mask.is_empty() {
                return Ok(vec![FrameMask {
                    segment_number: frame.segment_number,
                    mask,
                }]);
            }
            return Ok(Vec::new());
        }
        if let Some(mask) = &frame.mask {
            if mask.columns != doc.columns || mask.rows != doc.rows {
                return Err(invalid(format!(
                    "binary frame mask is {}x{}, want {}x{}",
                    mask.columns, mask.rows, doc.columns, doc.rows
                )));
            }
            if !mask.is_empty() {
                return Ok(vec![FrameMask {
                    segment_number: frame.segment_number,
                    mask: mask.clone(),
                }]);
            }
        }
        Ok(Vec::new())
    }
    fn label_map_masks(&self, samples: &[u16]) -> Vec<FrameMask> {
        let mut index: HashMap<u16, usize> = HashMap::new();
        let mut out: Vec<FrameMask> = Vec::new();
        let (columns, rows) = (self.doc.columns, self.doc.rows);
        for y in 0..rows {
            let row = &samples[(y * columns) as usize..((y + 1) * columns) as usize];
            let mut x = 0usize;
            while x < row.len() {
                let label = row[x];
                let mut end = x + 1;
                while end < row.len() && row[end] == label {
                    end += 1;
                }
                if self.labels.contains(&label) {
                    let slot = *index.entry(label).or_insert_with(|| {
                        out.push(FrameMask {
                            segment_number: i64::from(label),
                            mask: RasterMask::new(columns, rows),
                        });
                        out.len() - 1
                    });
                    out[slot].mask.set_run(y, x as i64, end as i64);
                }
                x = end;
            }
        }
        out
    }
}
