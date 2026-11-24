use super::MeshError;
use super::field::grid_affine;
use super::surface_nets::DenseLabelField;
use crate::fmath;
use crate::render::GeometryAffine;
use crate::roi::RasterMask;
use crate::roi::resample_grid::Grid;
use crate::seg::Resampled;
use std::collections::BTreeMap;
pub fn label_field_from_masks(
    grid: &Grid,
    masks: &BTreeMap<u32, Vec<Option<RasterMask>>>,
) -> Result<(DenseLabelField, i64), MeshError> {
    let affine = label_grid_affine(grid)?;
    let count = grid.voxel_count();
    if count > i64::from(i32::MAX) {
        return Err(MeshError::invalid(format!("{count} voxels")));
    }
    let mut field = DenseLabelField {
        size: [grid.columns, grid.rows, grid.slices],
        affine,
        labels: vec![0; count as usize],
    };
    let columns = grid.columns as usize;
    let plane = columns * grid.rows as usize;
    let mut overlaps = 0i64;
    for (&label, slices) in masks.range(1..) {
        for (z, mask) in slices.iter().enumerate() {
            let Some(mask) = mask else {
                continue;
            };
            if z as i64 >= grid.slices {
                continue;
            }
            if mask.columns != grid.columns || mask.rows != grid.rows {
                return Err(MeshError::invalid(format!(
                    "label {label} slice {z} mask is {}x{} on a {}x{} grid",
                    mask.columns, mask.rows, grid.columns, grid.rows
                )));
            }
            let base = &mut field.labels[z * plane..(z + 1) * plane];
            mask.for_each_run(|y, run| {
                let row = y as usize * columns;
                for v in &mut base[row + run.start as usize..row + run.end as usize] {
                    if *v != 0 {
                        overlaps += 1;
                    }
                    *v = label;
                }
            });
        }
    }
    Ok((field, overlaps))
}
pub fn label_field_from_seg(resampled: &Resampled) -> Result<(DenseLabelField, i64), MeshError> {
    let mut masks = BTreeMap::new();
    for segment in &resampled.segments {
        if segment.number <= 0 {
            continue;
        }
        masks.insert(
            segment.number as u32,
            segment.masks.clone().unwrap_or_default(),
        );
    }
    label_field_from_masks(&resampled.grid, &masks)
}
fn label_grid_affine(grid: &Grid) -> Result<GeometryAffine, MeshError> {
    grid.validate().map_err(MeshError::invalid)?;
    let (mut origin, mut spacing) = (grid.origin, grid.slice_spacing);
    let offsets = &grid.slice_offsets;
    if let Some(&first) = offsets.first() {
        origin = origin.add(grid.slice_dir.scale(first));
        if offsets.len() > 1 {
            spacing = (offsets[offsets.len() - 1] - first) / (offsets.len() - 1) as f64;
            for (z, &offset) in offsets.iter().enumerate() {
                let expected = first + z as f64 * spacing;
                if (offset - expected).abs() > 1e-3 * fmath::max(spacing, 1e-9) {
                    return Err(MeshError::invalid(
                        "irregular slice offsets; resample onto a regular grid first",
                    ));
                }
            }
        }
    }
    if spacing.is_nan() || spacing <= 0.0 {
        spacing = fmath::min(grid.col_spacing, grid.row_spacing);
    }
    Ok(grid_affine(
        origin,
        [grid.row_dir, grid.col_dir, grid.slice_dir],
        [grid.col_spacing, grid.row_spacing, spacing],
    ))
}
