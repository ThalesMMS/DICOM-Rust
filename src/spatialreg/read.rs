use super::affine::{
    AffineObject, AffineRegistration, MatrixError, SPATIAL_REGISTRATION_STORAGE, TRANSFORM_AFFINE,
    TRANSFORM_RIGID, TRANSFORM_RIGID_SCALE, inverse_affine, multiply_affine, validate_matrix_type,
};
use super::{
    DEFORMABLE_SPATIAL_REGISTRATION_STORAGE, Error, Grid, IDENTITY, Kind, LIMIT_GRID_BYTES,
    LIMIT_GRID_VOXELS, LIMIT_REGISTRATIONS, Matrix, Object, Options, Registration, all_finite,
    checked_product64, finite_positive, undefined_vector,
};
use crate::derivedio as dio;
use crate::dicomjson::trim_unicode_space;
use crate::object::element_length;
use crate::object::{Object as DataObject, Tag, Vr};
use crate::render::affine::GeometryAffine;
use crate::render::vec3::Vec3;
use crate::textfmt::{quote, to_upper};
pub(crate) const TAG_REGISTRATION_SEQUENCE: Tag = Tag::new(0x0070, 0x0308);
pub(crate) const TAG_MATRIX_REGISTRATION_SEQUENCE: Tag = Tag::new(0x0070, 0x0309);
pub(crate) const TAG_MATRIX_SEQUENCE: Tag = Tag::new(0x0070, 0x030A);
pub(crate) const TAG_REFERENCED_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x1140);
pub(crate) const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
pub(crate) const TAG_DEFORMABLE_REGISTRATION_SEQUENCE: Tag = Tag::new(0x0064, 0x0002);
pub(crate) const TAG_SOURCE_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0064, 0x0003);
pub(crate) const TAG_DEFORMABLE_REGISTRATION_GRID_SEQUENCE: Tag = Tag::new(0x0064, 0x0005);
pub(crate) const TAG_GRID_DIMENSIONS: Tag = Tag::new(0x0064, 0x0007);
pub(crate) const TAG_GRID_RESOLUTION: Tag = Tag::new(0x0064, 0x0008);
pub(crate) const TAG_VECTOR_GRID_DATA: Tag = Tag::new(0x0064, 0x0009);
pub(crate) const TAG_PRE_DEFORMATION_MATRIX_SEQUENCE: Tag = Tag::new(0x0064, 0x000F);
pub(crate) const TAG_POST_DEFORMATION_MATRIX_SEQUENCE: Tag = Tag::new(0x0064, 0x0010);
pub(crate) const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
pub(crate) const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
pub(crate) const TAG_FRAME_TRANSFORMATION_MATRIX: Tag = Tag::new(0x3006, 0x00C6);
pub(crate) const TAG_FRAME_TRANSFORMATION_MATRIX_TYPE: Tag = Tag::new(0x0070, 0x030C);
fn nil_dataset() -> Error {
    Error::new(Kind::InvalidObject, "dataset is nil")
}
fn append_unique_string(values: &mut Vec<String>, value: &str) {
    let value = trim_unicode_space(value);
    if value.is_empty() || values.iter().any(|v| v == value) {
        return;
    }
    values.push(value.to_string());
}
fn affine_from(values: &[f64]) -> GeometryAffine {
    let mut out = [0f64; 16];
    out.copy_from_slice(&values[..16]);
    GeometryAffine(out)
}
pub fn read_affine_dataset(dataset: Option<&DataObject>) -> Result<AffineObject, Error> {
    let Some(dataset) = dataset else {
        return Err(nil_dataset());
    };
    let sop_class = dio::clean_uid(dataset, dio::TAG_SOP_CLASS_UID);
    if sop_class != SPATIAL_REGISTRATION_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class));
    }
    let mut out = AffineObject {
        sop_instance_uid: dio::clean_uid(dataset, dio::TAG_SOP_INSTANCE_UID),
        registered_frame_of_reference_uid: dio::clean_uid(dataset, dio::TAG_FRAME_OF_REFERENCE_UID),
        registrations: Vec::new(),
    };
    if out.sop_instance_uid.is_empty() || out.registered_frame_of_reference_uid.is_empty() {
        return Err(Error::new(
            Kind::InvalidObject,
            "missing SOP Instance UID or registered Frame of Reference UID",
        ));
    }
    let items = dataset
        .get_sequence(TAG_REGISTRATION_SEQUENCE)
        .unwrap_or_default();
    if items.is_empty() {
        return Err(Error::new(
            Kind::InvalidObject,
            "missing Registration Sequence",
        ));
    }
    for (index, item) in items.iter().enumerate() {
        let registration = read_affine_registration(item)
            .map_err(|e| e.wrap(Kind::InvalidObject, format!("registration item {index}")))?;
        out.registrations.push(registration);
    }
    Ok(out)
}
fn referenced_uids(item: &DataObject) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    if let Some(refs) = item.get_sequence(TAG_REFERENCED_IMAGE_SEQUENCE) {
        for reference in &refs {
            let uid = dio::clean_uid(reference, TAG_REFERENCED_SOP_INSTANCE_UID);
            if uid.is_empty() {
                return Err(Error::plain(
                    "Referenced Image Sequence contains a missing SOP Instance UID",
                ));
            }
            append_unique_string(&mut out, &uid);
        }
    }
    Ok(out)
}
fn read_affine_registration(item: &DataObject) -> Result<AffineRegistration, Error> {
    let mut out = AffineRegistration {
        source_frame_of_reference_uid: dio::clean_uid(item, dio::TAG_FRAME_OF_REFERENCE_UID),
        referenced_sop_instance_uids: referenced_uids(item)?,
        ..AffineRegistration::default()
    };
    if out.source_frame_of_reference_uid.is_empty() && out.referenced_sop_instance_uids.is_empty() {
        return Err(Error::plain(
            "missing source Frame of Reference UID and Referenced Image Sequence",
        ));
    }
    let matrix_registration_items = item
        .get_sequence(TAG_MATRIX_REGISTRATION_SEQUENCE)
        .unwrap_or_default();
    if matrix_registration_items.len() != 1 {
        return Err(Error::plain(
            "Matrix Registration Sequence must contain one item",
        ));
    }
    let matrix_items = matrix_registration_items[0]
        .get_sequence(TAG_MATRIX_SEQUENCE)
        .unwrap_or_default();
    if matrix_items.is_empty() {
        return Err(Error::plain(
            "Matrix Sequence must contain at least one item",
        ));
    }
    let mut combined = IDENTITY;
    for (index, matrix_item) in matrix_items.iter().enumerate() {
        let matrix = read_affine_matrix(matrix_item, index)?;
        combined = multiply_affine(&matrix.values, &combined);
        out.matrices.push(matrix);
    }
    let kind = most_general_type(&out.matrices);
    match inverse_affine(&combined) {
        Err((condition, err)) => Err(MatrixError {
            index: matrix_items.len() - 1,
            r#type: kind.to_string(),
            condition,
            kind: err,
        }
        .into_error()),
        Ok((inverse, _)) => {
            out.source_to_registered = combined;
            out.registered_to_source = inverse;
            out.r#type = kind.to_string();
            Ok(out)
        }
    }
}
fn matrix_type_name(item: &DataObject) -> String {
    to_upper(trim_unicode_space(&dio::clean_string(
        item,
        TAG_FRAME_TRANSFORMATION_MATRIX_TYPE,
    )))
}
fn read_affine_matrix(item: &DataObject, index: usize) -> Result<Matrix, Error> {
    let type_name = matrix_type_name(item);
    let fail = |condition: f64, kind: Kind| {
        MatrixError {
            index,
            r#type: type_name.clone(),
            condition,
            kind,
        }
        .into_error()
    };
    if type_name != TRANSFORM_RIGID
        && type_name != TRANSFORM_RIGID_SCALE
        && type_name != TRANSFORM_AFFINE
    {
        return Err(fail(0.0, Kind::UnsupportedMatrix));
    }
    let values = match dio::lookup_floats(Some(item), TAG_FRAME_TRANSFORMATION_MATRIX) {
        Ok(values) if values.len() == 16 => values,
        _ => return Err(fail(0.0, Kind::InvalidMatrix)),
    };
    let affine = affine_from(&values);
    if let Err(kind) = validate_matrix_type(&affine, &type_name) {
        let condition = match inverse_affine(&affine) {
            Err((value, _)) => value,
            Ok(_) => 0.0,
        };
        return Err(fail(condition, kind));
    }
    Ok(Matrix {
        r#type: type_name,
        values: affine,
    })
}
fn most_general_type(matrices: &[Matrix]) -> &'static str {
    let mut kind = TRANSFORM_RIGID;
    for matrix in matrices {
        match matrix.r#type.as_str() {
            TRANSFORM_AFFINE => return TRANSFORM_AFFINE,
            TRANSFORM_RIGID_SCALE => kind = TRANSFORM_RIGID_SCALE,
            _ => {}
        }
    }
    kind
}
pub fn read_dataset(dataset: Option<&DataObject>) -> Result<Object, Error> {
    read_dataset_with_options(dataset, Options::default())
}
pub fn read_dataset_with_options(
    dataset: Option<&DataObject>,
    opts: Options,
) -> Result<Object, Error> {
    let Some(dataset) = dataset else {
        return Err(nil_dataset());
    };
    if opts.max_registrations < 0 {
        return Err(Error::new(
            Kind::InvalidObject,
            "MaxRegistrations must not be negative",
        ));
    }
    let opts = opts.normalized();
    let sop_class = dio::clean_uid(dataset, dio::TAG_SOP_CLASS_UID);
    if sop_class != DEFORMABLE_SPATIAL_REGISTRATION_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class));
    }
    let mut out = Object {
        sop_instance_uid: dio::clean_uid(dataset, dio::TAG_SOP_INSTANCE_UID),
        registered_frame_of_reference_uid: dio::clean_uid(dataset, dio::TAG_FRAME_OF_REFERENCE_UID),
        registrations: Vec::new(),
    };
    if out.sop_instance_uid.is_empty() || out.registered_frame_of_reference_uid.is_empty() {
        return Err(Error::new(
            Kind::InvalidObject,
            "missing SOP Instance UID or registered Frame of Reference UID",
        ));
    }
    let items = dataset
        .get_sequence(TAG_DEFORMABLE_REGISTRATION_SEQUENCE)
        .unwrap_or_default();
    if items.is_empty() {
        return Err(Error::new(
            Kind::InvalidObject,
            "missing Deformable Registration Sequence",
        ));
    }
    if items.len() as i64 > opts.max_registrations {
        return Err(Error::limit_exceeded(
            LIMIT_REGISTRATIONS,
            items.len() as u64,
            opts.max_registrations as u64,
        ));
    }
    let mut has_grid = false;
    for (index, item) in items.iter().enumerate() {
        let registration = read_registration(item, &opts)
            .map_err(|e| e.wrap(Kind::InvalidObject, format!("registration item {index}")))?;
        has_grid = has_grid || registration.grid.is_some();
        out.registrations.push(registration);
    }
    if !has_grid {
        return Err(Error::new(
            Kind::InvalidObject,
            "no registration item contains a deformation grid",
        ));
    }
    Ok(out)
}
fn read_registration(item: &DataObject, opts: &Options) -> Result<Registration, Error> {
    let mut out = Registration {
        source_frame_of_reference_uid: dio::clean_uid(item, TAG_SOURCE_FRAME_OF_REFERENCE_UID),
        referenced_sop_instance_uids: Vec::new(),
        pre: Matrix::identity(),
        post: Matrix::identity(),
        grid: None,
    };
    if out.source_frame_of_reference_uid.is_empty() {
        return Err(Error::plain("missing Source Frame of Reference UID"));
    }
    out.referenced_sop_instance_uids = referenced_uids(item)?;
    out.pre = read_matrix(item, TAG_PRE_DEFORMATION_MATRIX_SEQUENCE)
        .map_err(|e| e.prefixed("pre deformation matrix"))?;
    out.post = read_matrix(item, TAG_POST_DEFORMATION_MATRIX_SEQUENCE)
        .map_err(|e| e.prefixed("post deformation matrix"))?;
    let Some(grid_items) = item.get_sequence(TAG_DEFORMABLE_REGISTRATION_GRID_SEQUENCE) else {
        return Ok(out);
    };
    if grid_items.is_empty() {
        return Ok(out);
    }
    if grid_items.len() > 1 {
        return Err(Error::plain(format!(
            "Deformable Registration Grid Sequence has {} items, want 1",
            grid_items.len()
        )));
    }
    out.grid = Some(read_grid(&grid_items[0], opts)?);
    Ok(out)
}
fn read_matrix(parent: &DataObject, tag: Tag) -> Result<Matrix, Error> {
    let Some(items) = parent.get_sequence(tag) else {
        return Ok(Matrix::identity());
    };
    if items.len() != 1 {
        return Err(Error::plain(format!(
            "sequence has {} items, want 1",
            items.len()
        )));
    }
    let item = &items[0];
    let type_name = matrix_type_name(item);
    match type_name.as_str() {
        "RIGID" | "RIGID_SCALE" | "AFFINE" => {}
        _ => {
            return Err(Error::plain(format!(
                "unsupported matrix type {}",
                quote(&type_name)
            )));
        }
    }
    let values = match dio::lookup_floats(Some(item), TAG_FRAME_TRANSFORMATION_MATRIX) {
        Ok(values) if values.len() == 16 => values,
        _ => {
            return Err(Error::plain(
                "Frame of Reference Transformation Matrix must contain 16 DS values",
            ));
        }
    };
    let affine = affine_from(&values);
    if !affine.finite() {
        return Err(Error::plain(
            "matrix is non-finite or has an invalid homogeneous row",
        ));
    }
    Ok(Matrix {
        r#type: type_name,
        values: affine,
    })
}
fn read_grid(item: &DataObject, opts: &Options) -> Result<Grid, Error> {
    let dimension_values = match dio::lookup_ints(Some(item), TAG_GRID_DIMENSIONS) {
        Ok(values) if values.len() == 3 => values,
        _ => return Err(Error::plain("Grid Dimensions must contain three UL values")),
    };
    let mut dimensions = [0u32; 3];
    for (index, &value) in dimension_values.iter().enumerate() {
        if value <= 0 || value > i64::from(u32::MAX) {
            return Err(Error::plain(format!(
                "Grid Dimensions[{index}] is invalid: {value}"
            )));
        }
        dimensions[index] = value as u32;
    }
    let Some(voxel_count) = checked_product64(&dimensions.map(u64::from)) else {
        return Err(Error::plain(
            "Grid Dimensions multiplication overflows uint64",
        ));
    };
    if voxel_count > opts.max_grid_voxels {
        return Err(Error::limit_exceeded(
            LIMIT_GRID_VOXELS,
            voxel_count,
            opts.max_grid_voxels,
        ));
    }
    let Some(encoded_bytes) = checked_product64(&[voxel_count, 3, 4]) else {
        return Err(Error::plain(
            "Vector Grid Data byte length overflows uint64",
        ));
    };
    if encoded_bytes > opts.max_grid_bytes {
        return Err(Error::limit_exceeded(
            LIMIT_GRID_BYTES,
            encoded_bytes,
            opts.max_grid_bytes,
        ));
    }
    if voxel_count > i64::MAX as u64 {
        return Err(Error::limit_exceeded(
            LIMIT_GRID_VOXELS,
            voxel_count,
            i64::MAX as u64,
        ));
    }
    let position = dio::lookup_floats(Some(item), TAG_IMAGE_POSITION_PATIENT);
    let orientation = dio::lookup_floats(Some(item), TAG_IMAGE_ORIENTATION_PATIENT);
    let resolution = dio::lookup_floats(Some(item), TAG_GRID_RESOLUTION);
    let (position, orientation, resolution) = match (position, orientation, resolution) {
        (Ok(p), Ok(o), Ok(r)) if p.len() == 3 && o.len() == 6 && r.len() == 3 => (p, o, r),
        _ => return Err(Error::plain("incomplete deformation grid geometry")),
    };
    if !all_finite(&position)
        || !all_finite(&orientation)
        || !finite_positive(resolution[0])
        || !finite_positive(resolution[1])
        || !finite_positive(resolution[2])
    {
        return Err(Error::plain(
            "non-finite or non-positive deformation grid geometry",
        ));
    }
    let row = Vec3::new(orientation[0], orientation[1], orientation[2]);
    let column = Vec3::new(orientation[3], orientation[4], orientation[5]);
    if (row.length() - 1.0).abs() > 1e-4
        || (column.length() - 1.0).abs() > 1e-4
        || row.dot(column).abs() > 1e-4
    {
        return Err(Error::plain(
            "Image Orientation Patient must contain orthonormal directions",
        ));
    }
    let row = row.normalize();
    let column = column.normalize();
    let normal = row.cross(column).normalize();
    if row.is_zero() || column.is_zero() || normal.is_zero() {
        return Err(Error::plain("Image Orientation Patient is degenerate"));
    }
    let element = match item.get(TAG_VECTOR_GRID_DATA) {
        Some(element) if element.vr() == Vr::OF => element,
        _ => return Err(Error::plain("missing Vector Grid Data with VR OF")),
    };
    let Some(raw) = element.raw() else {
        return Err(Error::plain(
            "Vector Grid Data is not materialized raw OF data",
        ));
    };
    if raw.len() as u64 != encoded_bytes || u64::from(element_length(element)) != encoded_bytes {
        return Err(Error::plain(format!(
            "Vector Grid Data has {} bytes, want {encoded_bytes}",
            raw.len()
        )));
    }
    let big_endian = item.value_big_endian();
    let mut vectors = Vec::with_capacity(voxel_count as usize);
    for (index, chunk) in raw.as_chunks::<12>().0.iter().enumerate() {
        let read = |o: usize| {
            let bytes = [chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]];
            f32::from_bits(if big_endian {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            })
        };
        let value = [read(0), read(4), read(8)];
        let all_finite = value[0].is_finite() && value[1].is_finite() && value[2].is_finite();
        if !undefined_vector(value) && !all_finite {
            return Err(Error::plain(format!(
                "Vector Grid Data vector {index} is partially undefined or non-finite"
            )));
        }
        vectors.push(value);
    }
    Ok(Grid {
        origin: Vec3::new(position[0], position[1], position[2]),
        row_direction: row,
        column_direction: column,
        normal_direction: normal,
        spacing: Vec3::new(resolution[0], resolution[1], resolution[2]),
        dimensions,
        vectors: vectors.into(),
    })
}
