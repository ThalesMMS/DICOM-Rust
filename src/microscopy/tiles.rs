use super::concatenation::{SlideTileMember, implicit_tile_count, validate_slide_span};
use super::{
    Error, Kind, Level, TAG_COLUMN_POSITION_TOTAL_MATRIX, TAG_DIMENSION_ORGANIZATION_TYPE,
    TAG_IMAGE_TYPE, TAG_NUMBER_OF_OPTICAL_PATHS, TAG_OPTICAL_PATH_IDENTIFICATION_SEQUENCE,
    TAG_OPTICAL_PATH_IDENTIFIER, TAG_PER_FRAME_FUNCTIONAL_GROUPS, TAG_PIXEL_MEASURES_SEQUENCE,
    TAG_PLANE_POSITION_SLIDE_SEQUENCE, TAG_ROW_POSITION_TOTAL_MATRIX, TAG_SHARED_FUNCTIONAL_GROUPS,
    TAG_SPACING_BETWEEN_SLICES, TAG_TOTAL_PIXEL_MATRIX_FOCAL_PLANES, TAG_Z_OFFSET_SLIDE, Tile,
    ceil_div, finite_positive,
};
use crate::derivedio;
use crate::object::{Object, Tag};
use std::collections::HashMap;
const MAX_SLIDE_FOCAL_PLANES: i64 = 4096;
pub(super) fn place_slide_tiles(root: &Object, level: &mut Level) -> Result<(), Error> {
    let full = derivedio::clean_string(root, TAG_DIMENSION_ORGANIZATION_TYPE) == "TILED_FULL";
    let per_frame = derivedio::sequence(root, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    if root.get(TAG_PER_FRAME_FUNCTIONAL_GROUPS).is_some()
        && per_frame.len() as i64 != level.frame_count
    {
        return Err(Error::invalid(format!(
            "{} per-frame groups for {} frames",
            per_frame.len(),
            level.frame_count
        )));
    }
    if !full && per_frame.is_empty() {
        return Err(Error::invalid(
            "frame positions are required outside TILED_FULL",
        ));
    }
    let shared = slide_macro(Some(root), TAG_SHARED_FUNCTIONAL_GROUPS)?;
    let shared_path = slide_path(shared.as_ref(), level)?;
    let ancillary = is_single_frame_slide_flavor(root);
    if ancillary && (level.frame_count != 1 || !level.concatenation_uid().is_empty()) {
        return Err(Error::invalid(
            "ancillary slide image requires one standalone frame",
        ));
    }
    if !full {
        return read_sparse_tiles(level, &per_frame, shared_path);
    }
    if ancillary {
        return place_tiled_full_ancillary(level, shared.as_ref(), &per_frame, shared_path);
    }
    place_tiled_full(root, level, shared.as_ref(), &per_frame, &shared_path)
}
fn place_tiled_full(
    root: &Object,
    level: &mut Level,
    shared: Option<&Object>,
    per_frame: &[Object],
    shared_path: &str,
) -> Result<(), Error> {
    let paths = level.optical_path_ids.len() as i64;
    if root.get(TAG_NUMBER_OF_OPTICAL_PATHS).is_some() {
        let declared = derivedio::ints(Some(root), TAG_NUMBER_OF_OPTICAL_PATHS);
        if declared.as_deref() != Some(&[paths]) {
            return Err(Error::invalid(
                "optical path count disagrees with the path sequence",
            ));
        }
    }
    let plane_count = slide_count(
        root,
        TAG_TOTAL_PIXEL_MATRIX_FOCAL_PLANES,
        MAX_SLIDE_FOCAL_PLANES,
    )?;
    let expected = implicit_tile_count(
        level.matrix_width,
        level.matrix_height,
        level.tile_width,
        level.tile_height,
        paths,
        plane_count,
    )?;
    let end = level.frame_offset + level.frame_count;
    if level.concatenation_uid().is_empty() {
        if end != expected {
            return Err(Error::invalid(format!(
                "{end} frames for {expected} TILED_FULL tile positions"
            )));
        }
    } else {
        if end > expected {
            return Err(Error::invalid(
                "concatenation frames exceed the TILED_FULL layout",
            ));
        }
        let member = SlideTileMember {
            source: level.source.clone(),
            number: level.concatenation_number(),
            offset: level.frame_offset,
            frames: level.frame_count,
        };
        validate_slide_span(&member, level.concatenation_total(), expected)?;
    }
    if !shared_path.is_empty() {
        let frames_per_path = expected / paths;
        let at = |frame: i64| &level.optical_path_ids[(frame / frames_per_path) as usize];
        if at(level.frame_offset) != shared_path || at(end - 1) != shared_path {
            return Err(Error::invalid(
                "shared optical path disagrees with the TILED_FULL frame span",
            ));
        }
    }
    let spacing = if plane_count > 1 {
        slide_focal_spacing(shared, per_frame)?
    } else {
        0.0
    };
    let mut offsets: Vec<f64> = Vec::with_capacity(plane_count as usize);
    for plane in 0..plane_count {
        let z = level.origin.z + plane as f64 * spacing;
        if z.is_infinite() || offsets.last().is_some_and(|&previous| z <= previous) {
            return Err(Error::invalid("unrepresentable focal plane position"));
        }
        offsets.push(z);
    }
    level.focal_plane_offsets = offsets;
    validate_tiled_full_groups(level, shared, per_frame)?;
    level.implicit_tiling = true;
    Ok(())
}
fn place_tiled_full_ancillary(
    level: &mut Level,
    shared: Option<&Object>,
    frames: &[Object],
    mut path: String,
) -> Result<(), Error> {
    let mut position = slide_macro(shared, TAG_PLANE_POSITION_SLIDE_SEQUENCE)?;
    if let [frame] = frames {
        if let Some(explicit) = slide_macro(Some(frame), TAG_PLANE_POSITION_SLIDE_SEQUENCE)? {
            position = Some(explicit);
        }
        let explicit_path = slide_path(Some(frame), level)?;
        if !explicit_path.is_empty() {
            path = explicit_path;
        }
    }
    if path.is_empty() {
        path = level.optical_path_ids[0].clone();
    }
    let (mut column, mut row, mut z) = (0, 0, level.origin.z);
    if let Some(position) = &position {
        (column, row, z) = slide_tile_position(position, z, level)?;
    }
    level.tiles = vec![Tile {
        source: level.source.clone(),
        frame_number: 1,
        column,
        row,
        width: level.tile_width.min(level.matrix_width - column),
        height: level.tile_height.min(level.matrix_height - row),
        optical_path: path,
        focal_plane: 0,
        z_offset_millimeter: z,
    }];
    level.focal_plane_offsets = vec![z];
    Ok(())
}
fn validate_tiled_full_groups(
    level: &Level,
    shared: Option<&Object>,
    per_frame: &[Object],
) -> Result<(), Error> {
    let shared_position = slide_macro(shared, TAG_PLANE_POSITION_SLIDE_SEQUENCE)?;
    if shared_position.is_none() && per_frame.is_empty() {
        return Ok(());
    }
    let across = ceil_div(level.matrix_width, level.tile_width);
    let tiles_per_plane = across * ceil_div(level.matrix_height, level.tile_height);
    let planes = level.focal_plane_offsets.len() as i64;
    for index in 0..level.frame_count {
        let ordinal = level.frame_offset + index;
        let plane = ordinal / tiles_per_plane % planes;
        let column = ordinal % tiles_per_plane % across * level.tile_width;
        let row = ordinal % tiles_per_plane / across * level.tile_height;
        let path = &level.optical_path_ids[(ordinal / (tiles_per_plane * planes)) as usize];
        let mut explicit = None;
        if !per_frame.is_empty() {
            let frame = &per_frame[index as usize];
            explicit = slide_macro(Some(frame), TAG_PLANE_POSITION_SLIDE_SEQUENCE)?;
            let explicit_path = slide_path(Some(frame), level)?;
            if !explicit_path.is_empty() && explicit_path != *path {
                return Err(Error::invalid(format!(
                    "frame {} optical path disagrees with TILED_FULL order",
                    index + 1
                )));
            }
        }
        let Some(position) = explicit.as_ref().or(shared_position.as_ref()) else {
            continue;
        };
        let z = level.focal_plane_offsets[plane as usize];
        let (got_column, got_row, got_z) = slide_tile_position(position, z, level)
            .map_err(|e| e.prefixed(format!("frame {}", index + 1)))?;
        if got_column != column || got_row != row || (got_z - z).abs() > 1e-9 {
            return Err(Error::invalid(format!(
                "frame {} position disagrees with TILED_FULL order",
                index + 1
            )));
        }
    }
    Ok(())
}
fn read_sparse_tiles(
    level: &mut Level,
    per_frame: &[Object],
    mut shared_path: String,
) -> Result<(), Error> {
    if shared_path.is_empty() && level.optical_path_ids.len() == 1 {
        shared_path = level.optical_path_ids[0].clone();
    }
    let mut planes: HashMap<u64, f64> = HashMap::new();
    let mut tiles = Vec::with_capacity(per_frame.len());
    for (index, frame) in per_frame.iter().enumerate() {
        let number = index as i64 + 1;
        let position = slide_macro(Some(frame), TAG_PLANE_POSITION_SLIDE_SEQUENCE)
            .map_err(|e| e.prefixed(format!("frame {number}")))?;
        let Some(position) = position else {
            return Err(Error::invalid(format!(
                "frame {number} has no explicit tile position"
            )));
        };
        let (column, row, z) = slide_tile_position(&position, level.origin.z, level)
            .map_err(|e| e.prefixed(format!("frame {number}")))?;
        let mut path = slide_path(Some(frame), level)?;
        if path.is_empty() {
            path = shared_path.clone();
        }
        if path.is_empty() {
            return Err(Error::path(&level.source, "", Kind::MissingOpticalPath));
        }
        planes.entry((z + 0.0).to_bits()).or_insert(z);
        if planes.len() as i64 > MAX_SLIDE_FOCAL_PLANES {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        tiles.push(Tile {
            source: level.source.clone(),
            frame_number: number,
            row,
            column,
            width: level.tile_width.min(level.matrix_width - column),
            height: level.tile_height.min(level.matrix_height - row),
            optical_path: path,
            focal_plane: 0,
            z_offset_millimeter: z,
        });
    }
    let mut offsets: Vec<f64> = planes.into_values().collect();
    offsets.sort_by(f64::total_cmp);
    for tile in &mut tiles {
        tile.focal_plane =
            offsets.partition_point(|&offset| offset < tile.z_offset_millimeter) as i64;
    }
    level.tiles = tiles;
    level.focal_plane_offsets = offsets;
    Ok(())
}
fn slide_tile_position(
    position: &Object,
    fallback_z: f64,
    level: &Level,
) -> Result<(i64, i64, f64), Error> {
    let one = |t: Tag, limit: i64| match derivedio::ints(Some(position), t).as_deref() {
        Some([value]) if *value > 0 && *value <= limit => Some(*value),
        _ => None,
    };
    let (Some(column), Some(row)) = (
        one(TAG_COLUMN_POSITION_TOTAL_MATRIX, level.matrix_width),
        one(TAG_ROW_POSITION_TOTAL_MATRIX, level.matrix_height),
    ) else {
        return Err(Error::invalid("invalid one-based tile position"));
    };
    let z = slide_z(position, fallback_z)?;
    Ok((column - 1, row - 1, z))
}
pub(super) fn slide_z(root: &Object, fallback: f64) -> Result<f64, Error> {
    if root.get(TAG_Z_OFFSET_SLIDE).is_none() {
        return Ok(fallback);
    }
    match derivedio::floats(Some(root), TAG_Z_OFFSET_SLIDE).as_deref() {
        Some([value]) if value.is_finite() => Ok(value / 1000.0),
        _ => Err(Error::invalid("invalid slide Z offset")),
    }
}
fn slide_focal_spacing(shared: Option<&Object>, per_frame: &[Object]) -> Result<f64, Error> {
    if let Some(measures) = slide_macro(shared, TAG_PIXEL_MEASURES_SEQUENCE)? {
        if per_frame
            .iter()
            .any(|frame| frame.get(TAG_PIXEL_MEASURES_SEQUENCE).is_some())
        {
            return Err(Error::invalid(
                "Pixel Measures occur in both shared and per-frame groups",
            ));
        }
        return spacing_between_slide_planes(Some(&measures));
    }
    let mut spacing = 0.0;
    for frame in per_frame {
        let measures = slide_macro(Some(frame), TAG_PIXEL_MEASURES_SEQUENCE)?;
        let current = spacing_between_slide_planes(measures.as_ref())?;
        if spacing > 0.0 && spacing != current {
            return Err(Error::invalid("inconsistent focal plane spacing"));
        }
        spacing = current;
    }
    if spacing <= 0.0 {
        return Err(Error::invalid(
            "missing Pixel Measures for TILED_FULL focal planes",
        ));
    }
    Ok(spacing)
}
fn spacing_between_slide_planes(measures: Option<&Object>) -> Result<f64, Error> {
    match derivedio::floats(measures, TAG_SPACING_BETWEEN_SLICES).as_deref() {
        Some([value]) if finite_positive(*value) => Ok(*value),
        _ => Err(Error::invalid(
            "invalid Spacing Between Slices for TILED_FULL focal planes",
        )),
    }
}
fn slide_path(group: Option<&Object>, level: &Level) -> Result<String, Error> {
    let Some(path) = slide_macro(group, TAG_OPTICAL_PATH_IDENTIFICATION_SEQUENCE)? else {
        return Ok(String::new());
    };
    let id = derivedio::clean_string(&path, TAG_OPTICAL_PATH_IDENTIFIER);
    if id.is_empty() {
        return Err(Error::path(&level.source, "", Kind::MissingOpticalPath));
    }
    if !level.optical_path_ids.contains(&id) {
        return Err(Error::path(&level.source, &id, Kind::UnknownOpticalPath));
    }
    Ok(id)
}
fn slide_macro(group: Option<&Object>, tag: Tag) -> Result<Option<Object>, Error> {
    let Some(group) = group else {
        return Ok(None);
    };
    if group.get(tag).is_none() {
        return Ok(None);
    }
    let mut items = derivedio::sequence(group, tag);
    if items.len() != 1 {
        return Err(Error::invalid(format!("expected one item for {tag}")));
    }
    Ok(items.pop())
}
pub(super) fn slide_count(root: &Object, tag: Tag, maximum: i64) -> Result<i64, Error> {
    if root.get(tag).is_none() {
        return Ok(1);
    }
    let value = match derivedio::ints(Some(root), tag).as_deref() {
        Some([value]) if *value > 0 => *value,
        _ => {
            return Err(Error::invalid(format!("expected positive count for {tag}")));
        }
    };
    if value > maximum {
        if tag == TAG_TOTAL_PIXEL_MATRIX_FOCAL_PLANES {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        return Err(Error::invalid("frame count exceeds limits"));
    }
    Ok(value)
}
fn is_single_frame_slide_flavor(root: &Object) -> bool {
    let values = root.get_strings(TAG_IMAGE_TYPE).unwrap_or_default();
    matches!(
        values.get(2).map(|v| v.trim()),
        Some("THUMBNAIL" | "LABEL" | "OVERVIEW" | "LOCALIZER")
    )
}
