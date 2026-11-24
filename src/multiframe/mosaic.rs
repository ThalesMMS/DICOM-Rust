use super::{
    Error, Kind, TAG_ACQUISITION_MATRIX, TAG_COLUMNS, TAG_IMAGE_ORIENTATION_PATIENT,
    TAG_IMAGE_POSITION_PATIENT, TAG_IN_PLANE_PHASE_ENCODING_DIRECTION, TAG_PIXEL_SPACING, TAG_ROWS,
    TAG_SLICE_THICKNESS, TAG_SPACING_BETWEEN_SLICES, cross, dot, uint16_value, unit,
};
use crate::object::geometry::spacing_values;
use crate::object::{Object, Tag};
use crate::siemens::{self, LookupOptions, trim_space};
pub const DIAGNOSTIC_MOSAIC_IMAGES_FROM_ACQUISITION_MATRIX: &str =
    "mosaic-images-from-acquisition-matrix";
pub const DIAGNOSTIC_MOSAIC_CSA_CREATOR_MISSING: &str = "mosaic-csa-creator-missing";
pub const DIAGNOSTIC_MOSAIC_SLICE_NORMAL_MISSING: &str = "mosaic-slice-normal-missing";
pub const DIAGNOSTIC_MOSAIC_SLICE_NORMAL_INCONSISTENT: &str = "mosaic-slice-normal-inconsistent";
pub const DIAGNOSTIC_MOSAIC_PROTOCOL_POSITION_MISMATCH: &str = "mosaic-protocol-position-mismatch";
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MosaicGeometry {
    pub images: i64,
    pub grid: i64,
    pub rows: i64,
    pub columns: i64,
    pub images_source: String,
    pub row: [f64; 3],
    pub column: [f64; 3],
    pub normal: [f64; 3],
    pub pixel_spacing: [f64; 2],
    pub slice_spacing: f64,
    pub inverted: bool,
    pub positions: Vec<[f64; 3]>,
    pub tile_order: Vec<i64>,
    pub protocol_checked: bool,
    pub diagnostics: Vec<&'static str>,
    pub(crate) source_rows: i64,
    pub(crate) source_columns: i64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct MosaicOptions {
    pub images_from_acquisition_matrix: bool,
    pub lookup: LookupOptions,
}
fn nonconformant(detail: impl AsRef<str>) -> Error {
    Error::wrap(Kind::Nonconformant, detail)
}
pub fn mosaic_geometry_of(dataset: &Object, opts: MosaicOptions) -> Result<MosaicGeometry, Error> {
    if !siemens::is_mosaic(dataset) {
        return Err(Error::bare(Kind::NotMosaic));
    }
    let mut g = MosaicGeometry::default();
    match (
        uint16_value(dataset, TAG_ROWS),
        uint16_value(dataset, TAG_COLUMNS),
    ) {
        (Some(rows), Some(columns)) if rows != 0 && columns != 0 => {
            g.source_rows = rows as i64;
            g.source_columns = columns as i64;
        }
        _ => return Err(nonconformant("Rows and Columns are required")),
    }
    if let Ok(header) = siemens::image_header(dataset, opts.lookup)
        && header.creator_missing
    {
        g.diagnostics.push(DIAGNOSTIC_MOSAIC_CSA_CREATOR_MISSING);
    }
    let (images, source) = match siemens::number_of_images_in_mosaic(dataset, opts.lookup) {
        Ok(found) => found,
        Err(err) => {
            if !opts.images_from_acquisition_matrix {
                return Err(nonconformant(err.to_string()));
            }
            let images = images_from_acquisition_matrix(dataset, g.source_rows, g.source_columns)?;
            g.diagnostics
                .push(DIAGNOSTIC_MOSAIC_IMAGES_FROM_ACQUISITION_MATRIX);
            (images, "acquisition-matrix")
        }
    };
    if images < 1 || images > g.source_rows * g.source_columns {
        return Err(nonconformant(format!("NumberOfImagesInMosaic {images}")));
    }
    g.images = images;
    g.images_source = source.to_string();
    g.grid = 1;
    while g.grid * g.grid < images {
        g.grid += 1;
    }
    if g.source_rows % g.grid != 0 || g.source_columns % g.grid != 0 {
        return Err(nonconformant(format!(
            "{}x{} mosaic is not a {}x{} grid",
            g.source_columns, g.source_rows, g.grid, g.grid
        )));
    }
    g.rows = g.source_rows / g.grid;
    g.columns = g.source_columns / g.grid;
    let orientation = match dataset.get_floats(TAG_IMAGE_ORIENTATION_PATIENT) {
        Ok(v) if v.len() == 6 => v,
        _ => return Err(nonconformant("Image Orientation (Patient) is required")),
    };
    let position = match dataset.get_floats(TAG_IMAGE_POSITION_PATIENT) {
        Ok(v) if v.len() == 3 => v,
        _ => return Err(nonconformant("Image Position (Patient) is required")),
    };
    match dataset.spacing_pair(TAG_PIXEL_SPACING) {
        Ok((row, column, values)) if values != spacing_values::ABSENT => {
            g.pixel_spacing = [row, column];
        }
        _ => return Err(nonconformant("Pixel Spacing is required")),
    }
    let (row, ok1) = unit([orientation[0], orientation[1], orientation[2]]);
    let (column, ok2) = unit([orientation[3], orientation[4], orientation[5]]);
    let (normal, ok3) = unit(cross(row, column));
    g.row = row;
    g.column = column;
    g.normal = normal;
    if !ok1 || !ok2 || !ok3 {
        return Err(nonconformant("degenerate Image Orientation (Patient)"));
    }
    g.slice_spacing = positive_float(dataset, TAG_SPACING_BETWEEN_SLICES);
    if g.slice_spacing == 0.0 {
        g.slice_spacing = positive_float(dataset, TAG_SLICE_THICKNESS);
    }
    if g.slice_spacing == 0.0 && images > 1 {
        return Err(nonconformant(
            "Spacing Between Slices or Slice Thickness is required",
        ));
    }
    match siemens::slice_normal_vector(dataset, opts.lookup) {
        Err(_) => g.diagnostics.push(DIAGNOSTIC_MOSAIC_SLICE_NORMAL_MISSING),
        Ok(found) => {
            let (n, ok) = unit(found);
            if !ok {
                g.diagnostics
                    .push(DIAGNOSTIC_MOSAIC_SLICE_NORMAL_INCONSISTENT);
            } else {
                let d = dot(n, g.normal);
                if d > 1.0 - 1e-4 {
                } else if d < -1.0 + 1e-4 {
                    g.inverted = true;
                } else {
                    g.diagnostics
                        .push(DIAGNOSTIC_MOSAIC_SLICE_NORMAL_INCONSISTENT);
                }
            }
        }
    }
    let mut origin = [0f64; 3];
    for i in 0..3 {
        origin[i] = position[i]
            + (g.source_columns - g.columns) as f64 / 2.0 * g.pixel_spacing[1] * g.row[i]
            + (g.source_rows - g.rows) as f64 / 2.0 * g.pixel_spacing[0] * g.column[i];
        if g.inverted {
            origin[i] -= g.normal[i] * g.slice_spacing * (images - 1) as f64;
        }
    }
    g.positions = vec![[0f64; 3]; images as usize];
    g.tile_order = vec![0; images as usize];
    for k in 0..images as usize {
        for i in 0..3 {
            g.positions[k][i] = origin[i] + k as f64 * g.slice_spacing * g.normal[i];
        }
        g.tile_order[k] = if g.inverted {
            images - 1 - k as i64
        } else {
            k as i64
        };
    }
    check_protocol(&mut g, dataset, opts.lookup);
    Ok(g)
}
fn check_protocol(g: &mut MosaicGeometry, dataset: &Object, lookup: LookupOptions) {
    let Ok(protocol) = siemens::series_protocol(dataset, lookup) else {
        return;
    };
    let slices = match protocol.slice_array() {
        Ok(slices) if slices.len() as i64 == g.images => slices,
        _ => return,
    };
    g.protocol_checked = true;
    for (k, &tile) in g.tile_order.iter().enumerate() {
        let mut distance = 0f64;
        for i in 0..3 {
            let centre = g.positions[k][i]
                + g.columns as f64 / 2.0 * g.pixel_spacing[1] * g.row[i]
                + g.rows as f64 / 2.0 * g.pixel_spacing[0] * g.column[i];
            let delta = centre - slices[tile as usize].position[i];
            distance += delta * delta;
        }
        if distance.sqrt() > 0.01 {
            g.diagnostics
                .push(DIAGNOSTIC_MOSAIC_PROTOCOL_POSITION_MISMATCH);
            return;
        }
    }
}
fn images_from_acquisition_matrix(dataset: &Object, rows: i64, columns: i64) -> Result<i64, Error> {
    let matrix = match siemens::uint16s(dataset, TAG_ACQUISITION_MATRIX) {
        Some(m) if m.len() == 4 => m,
        _ => {
            return Err(nonconformant(
                "Acquisition Matrix is required to derive the slice count",
            ));
        }
    };
    let direction = dataset
        .get_string(TAG_IN_PLANE_PHASE_ENCODING_DIRECTION)
        .unwrap_or_default();
    let (mut tile_columns, mut tile_rows) = match trim_space(&direction) {
        "COL" => (matrix[3] as i64, matrix[0] as i64),
        "ROW" => (matrix[1] as i64, matrix[2] as i64),
        _ => (0, 0),
    };
    if tile_columns <= 0 || tile_rows <= 0 {
        return Err(nonconformant("unusable Acquisition Matrix"));
    }
    if columns % tile_columns != 0 || rows % tile_rows != 0 {
        let size = tile_columns.max(tile_rows);
        if columns % size != 0 || rows % size != 0 {
            return Err(nonconformant("Acquisition Matrix does not tile the mosaic"));
        }
        tile_columns = size;
        tile_rows = size;
    }
    Ok(columns / tile_columns * (rows / tile_rows))
}
fn positive_float(dataset: &Object, tag: Tag) -> f64 {
    match dataset.get_float(tag) {
        Ok(value)
            if value.partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater)
                && !value.is_infinite() =>
        {
            value
        }
        _ => 0.0,
    }
}
