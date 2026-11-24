use super::source::{FilesSource, Source, SourceReadOptions};
use super::{
    Cause, Error, ErrorCode, ExportSentinel as S, GEOMETRY_RESAMPLE_CUBIC_BSPLINE,
    GEOMETRY_RESAMPLE_LINEAR, Options, Report, SCALING_APPLY_FLOAT32, SCALING_APPLY_FLOAT64,
    SCALING_PRESERVE_UNIFORM, SINGLE_SLICE_IMAGE_GRID, SINGLE_SLICE_REJECT, UnmeasuredVoxels,
    WARNING_NO_PATIENT_GEOMETRY, WARNING_PIXEL_SPACING_DEFAULT, WARNING_SLICE_SPACING_DEFAULT,
    WARNING_SLICE_SPACING_FROM_THICKNESS, bare, checked_add, checked_mul, export_error,
    normalized_options,
};
use crate::dynamic::{self, FrameMetadata, TimePoint};
use crate::fmath;
use crate::nifti::{
    DATATYPE_FLOAT32, DATATYPE_FLOAT64, DATATYPE_INT8, DATATYPE_INT16, DATATYPE_INT32,
    DATATYPE_UINT8, DATATYPE_UINT16, DATATYPE_UINT32, finite, finite_positive,
};
use crate::object::geometry::{
    FrameGeometry, IMAGE_ORIENTATION_TOLERANCE, SliceSpacing, image_orientation_deviation,
    slice_spacing_source,
};
use crate::object::{File, Object, Tag};
use crate::parametricmap::{self, PARAMETRIC_MAP_STORAGE};
use crate::pixel::Metadata;
use crate::pixel::pixeldata;
use crate::render::stablesort::slice_stable;
use crate::render::volume::{RegularGridGeometry, plan_regular_grid_geometry};
use crate::render::{
    GeometryAffine, GeometryDisposition, GeometryIssue, SliceGeometry, Vec3, VolumeGeometry,
    build_volume_geometry,
};
use std::io::Write;
pub(super) const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub(super) const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_ANATOMICAL_ORIENTATION_TYPE: Tag = Tag::new(0x0010, 0x2210);
const TAG_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
const TAG_DOUBLE_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_RESCALE_INTERCEPT: Tag = Tag::new(0x0028, 0x1052);
const TAG_RESCALE_SLOPE: Tag = Tag::new(0x0028, 0x1053);
const TAG_MODALITY_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3000);
const TAG_PIXEL_VALUE_TRANSFORMATION: Tag = Tag::new(0x0028, 0x9145);
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LinearTransform {
    pub slope: f64,
    pub intercept: f64,
}
impl Default for LinearTransform {
    fn default() -> Self {
        LinearTransform {
            slope: 1.0,
            intercept: 0.0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct ImageGridInfo {
    pub used: bool,
    pub semantic: &'static str,
    pub default_spacing: bool,
}
#[derive(Debug, Clone)]
pub(super) struct FramePlan {
    pub source_index: i64,
    pub frame_index: i64,
    pub ordinal: i64,
    pub geometry: SliceGeometry,
    pub temporal: FrameMetadata,
    pub metadata: Metadata,
    pub rescale: LinearTransform,
    pub parametric: bool,
    pub spool_offset: i64,
    pub spool_length: i64,
    pub slice_spacing: SliceSpacing,
    pub grid: ImageGridInfo,
}
#[derive(Debug, Clone)]
pub(super) struct TimePointPlan {
    pub frames: Vec<usize>,
    pub geometry: VolumeGeometry,
    pub regular_grid: RegularGridGeometry,
    pub timing: TimePoint,
}
#[derive(Debug, Clone)]
pub(super) struct ExportPlan {
    pub sources: i64,
    pub source_encapsulated: Vec<bool>,
    pub series_uid: String,
    pub frame_of_reference_uid: String,
    pub frames: Vec<FramePlan>,
    pub time_points: Vec<TimePointPlan>,
    pub dimensions: [i64; 4],
    pub datatype: i16,
    pub bitpix: i16,
    pub spool_datatype: i16,
    pub spool_bitpix: i16,
    pub spacing: [f64; 4],
    pub affine_lps: GeometryAffine,
    pub header_slope: f64,
    pub header_intercept: f64,
    pub scaling_policy: u8,
    pub parametric: bool,
    pub unit_code: String,
    pub unit_scheme: String,
    pub quantity_code: String,
    pub quantity_scheme: String,
    pub reordered: bool,
    pub resampled: bool,
    pub single_slice: bool,
    pub slice_spacing_source: &'static str,
    pub image_grid: ImageGridInfo,
    pub warnings: Vec<String>,
    pub unmeasured: UnmeasuredVoxels,
}
pub struct Plan<'s> {
    pub(super) source: &'s dyn Source,
    pub(super) options: Options,
    pub(super) value: ExportPlan,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlanLayout {
    pub dimensions: [i64; 4],
    pub datatype: i16,
    pub index_to_lps: GeometryAffine,
    pub spacing: [f64; 4],
    pub scaling_slope: f64,
    pub scaling_intercept: f64,
    pub scaling_policy: u8,
    pub no_spatial_transform: bool,
    pub resampled: bool,
}
impl PlanLayout {
    pub fn identity_scaling(&self) -> bool {
        self.scaling_slope == 1.0 && self.scaling_intercept == 0.0
    }
}
pub fn plan_volume<'s>(source: &'s dyn Source, options: &Options) -> Result<Plan<'s>, Error> {
    let normalized = normalized_options(options.clone())?;
    let value = build_plan(source, &normalized)?;
    Ok(Plan {
        source,
        options: normalized,
        value,
    })
}
pub fn write(
    destination: &mut dyn Write,
    source: &dyn Source,
    options: &Options,
) -> Result<Report, Error> {
    plan_volume(source, options)?.write(destination)
}
pub fn write_files(
    destination: &mut dyn Write,
    files: &[Option<&File>],
    options: &Options,
) -> Result<Report, Error> {
    let source = FilesSource::new(files.to_vec());
    write(destination, &source, options)
}
impl Plan<'_> {
    pub fn layout(&self) -> PlanLayout {
        let v = &self.value;
        PlanLayout {
            dimensions: v.dimensions,
            datatype: v.datatype,
            index_to_lps: v.affine_lps,
            spacing: v.spacing,
            scaling_slope: v.header_slope,
            scaling_intercept: v.header_intercept,
            scaling_policy: v.scaling_policy,
            no_spatial_transform: v.image_grid.used,
            resampled: v.resampled,
        }
    }
}
fn trim_text(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
fn uid(file: &File, tag: Tag) -> String {
    file.dataset.get_uid(tag).unwrap_or_default()
}
fn build_plan(source: &dyn Source, options: &Options) -> Result<ExportPlan, Error> {
    let count = source.len();
    if count <= 0 {
        return Err(bare(ErrorCode::Source, -1, -1, S::InvalidSource));
    }
    if count > options.limits.max_instances {
        return Err(bare(ErrorCode::Limit, -1, -1, S::LimitExceeded));
    }
    let mut plan = ExportPlan {
        sources: count,
        source_encapsulated: vec![false; count as usize],
        series_uid: String::new(),
        frame_of_reference_uid: String::new(),
        frames: Vec::new(),
        time_points: Vec::new(),
        dimensions: [0; 4],
        datatype: 0,
        bitpix: 0,
        spool_datatype: 0,
        spool_bitpix: 0,
        spacing: [1.0; 4],
        affine_lps: GeometryAffine::default(),
        header_slope: 1.0,
        header_intercept: 0.0,
        scaling_policy: SCALING_PRESERVE_UNIFORM,
        parametric: false,
        unit_code: String::new(),
        unit_scheme: String::new(),
        quantity_code: String::new(),
        quantity_scheme: String::new(),
        reordered: false,
        resampled: false,
        single_slice: false,
        slice_spacing_source: slice_spacing_source::UNKNOWN,
        image_grid: ImageGridInfo::default(),
        warnings: Vec::new(),
        unmeasured: UnmeasuredVoxels::default(),
    };
    let (mut series_uid, mut frame_of_reference_uid) = (String::new(), String::new());
    for source_index in 0..count {
        let opened = source
            .open(
                source_index,
                &SourceReadOptions {
                    defer_pixel_data: true,
                    max_total_bytes: 0,
                },
            )
            .map_err(|e| {
                let mut cause = Cause::text(e.text.clone());
                if e.limit {
                    cause.sentinels.push(S::LimitExceeded);
                }
                export_error(
                    ErrorCode::Source,
                    source_index,
                    -1,
                    Some(S::InvalidSource),
                    Some(cause),
                )
            })?;
        let file = opened.file();
        plan.source_encapsulated[source_index as usize] = file.transfer_syntax.encapsulated;
        let current_series = uid(file, TAG_SERIES_INSTANCE_UID);
        let current_for = uid(file, TAG_FRAME_OF_REFERENCE_UID);
        let grid_may_lack_for = options.single_slice == SINGLE_SLICE_IMAGE_GRID && count == 1;
        if trim_text(&current_series).is_empty()
            || (trim_text(&current_for).is_empty() && !grid_may_lack_for)
        {
            return Err(bare(
                ErrorCode::Identity,
                source_index,
                -1,
                S::MixedIdentity,
            ));
        }
        if source_index == 0 {
            (series_uid, frame_of_reference_uid) = (current_series, current_for);
        } else if current_series != series_uid || current_for != frame_of_reference_uid {
            return Err(bare(
                ErrorCode::Identity,
                source_index,
                -1,
                S::MixedIdentity,
            ));
        }
        if let Some(orientation) = file.dataset.get_string(TAG_ANATOMICAL_ORIENTATION_TYPE) {
            let orientation = crate::textfmt::to_upper(trim_text(&orientation));
            if !orientation.is_empty() && orientation != "BIPED" {
                return Err(bare(
                    ErrorCode::Quadruped,
                    source_index,
                    -1,
                    S::InvalidGeometry,
                ));
            }
        }
        let sop_class = uid(file, TAG_SOP_CLASS_UID);
        if sop_class == PARAMETRIC_MAP_STORAGE {
            plan_parametric_source(&mut plan, file, source_index, options)?;
        } else {
            plan_image_source(&mut plan, file, source_index, options)?;
        }
    }
    if plan.frames.is_empty() {
        return Err(bare(ErrorCode::Source, -1, -1, S::InvalidSource));
    }
    plan.series_uid = series_uid;
    plan.frame_of_reference_uid = frame_of_reference_uid;
    if plan.frames.len() as i64 > options.limits.max_frames {
        return Err(bare(ErrorCode::Limit, -1, -1, S::LimitExceeded));
    }
    plan_time_and_geometry(&mut plan, options)?;
    choose_encoding(&mut plan, options)?;
    Ok(plan)
}
fn plan_image_source(
    plan: &mut ExportPlan,
    file: &File,
    source_index: i64,
    options: &Options,
) -> Result<(), Error> {
    let ds = &file.dataset;
    if ds.get(TAG_FLOAT_PIXEL_DATA).is_some()
        || ds.get(TAG_DOUBLE_FLOAT_PIXEL_DATA).is_some()
        || plan.parametric
    {
        return Err(bare(
            ErrorCode::Pixels,
            source_index,
            -1,
            S::UnsupportedPixels,
        ));
    }
    let pixels = |cause: Option<Cause>| {
        export_error(
            ErrorCode::Pixels,
            source_index,
            -1,
            Some(S::UnsupportedPixels),
            cause,
        )
    };
    let metadata =
        pixeldata::extract_metadata(ds).map_err(|e| pixels(Some(Cause::text(e.to_string()))))?;
    if !valid_pixel_metadata(&metadata) {
        return Err(pixels(None));
    }
    if metadata.number_of_frames > options.limits.max_frames - plan.frames.len() as i64 {
        return Err(bare(ErrorCode::Limit, source_index, -1, S::LimitExceeded));
    }
    let temporal = match dynamic::read_frame_metadata(file) {
        Ok(t) if t.len() as i64 == metadata.number_of_frames => t,
        Ok(_) => {
            return Err(bare(
                ErrorCode::Temporal,
                source_index,
                -1,
                S::UnsupportedTemporal,
            ));
        }
        Err(e) => {
            return Err(export_error(
                ErrorCode::Temporal,
                source_index,
                -1,
                Some(S::UnsupportedTemporal),
                Some(Cause::text(e.into_message())),
            ));
        }
    };
    let allow_image_grid = options.single_slice == SINGLE_SLICE_IMAGE_GRID
        && plan.sources == 1
        && metadata.number_of_frames == 1;
    for frame_index in 0..metadata.number_of_frames {
        let frame_geometry = ds.frame_geometry_at(frame_index);
        let (geometry, grid) = plan_frame_geometry(
            ds,
            &frame_geometry,
            frame_index,
            i64::from(metadata.rows),
            i64::from(metadata.columns),
            allow_image_grid,
        )
        .ok_or_else(|| {
            bare(
                ErrorCode::Geometry,
                source_index,
                frame_index,
                S::InvalidGeometry,
            )
        })?;
        let rescale = frame_linear_transform(ds, frame_index).ok_or_else(|| {
            bare(
                ErrorCode::UnsupportedModality,
                source_index,
                frame_index,
                S::UnsupportedScaling,
            )
        })?;
        let mut frame_temporal = temporal[frame_index as usize].clone();
        frame_temporal.frame_index = plan.frames.len() as i64;
        let ordinal = plan.frames.len() as i64;
        plan.frames.push(FramePlan {
            source_index,
            frame_index,
            ordinal,
            geometry,
            temporal: frame_temporal,
            metadata: metadata.clone(),
            rescale,
            parametric: false,
            spool_offset: 0,
            spool_length: 0,
            slice_spacing: frame_geometry.slice_spacing(),
            grid,
        });
    }
    Ok(())
}
pub(super) fn plan_frame_geometry(
    obj: &Object,
    input: &FrameGeometry,
    frame_index: i64,
    rows: i64,
    columns: i64,
    allow_image_grid: bool,
) -> Option<(SliceGeometry, ImageGridInfo)> {
    let has_position = input
        .image_position_patient
        .as_ref()
        .is_some_and(|v| !v.is_empty());
    let has_orientation = input
        .image_orientation_patient
        .as_ref()
        .is_some_and(|v| !v.is_empty());
    if !allow_image_grid || has_position || has_orientation {
        return strict_slice_geometry(input, rows, columns).map(|g| (g, ImageGridInfo::default()));
    }
    if rows <= 0 || columns <= 0 {
        return None;
    }
    let mut grid = ImageGridInfo {
        used: true,
        ..ImageGridInfo::default()
    };
    let (mut row_spacing, mut column_spacing) = (1.0, 1.0);
    let resolution = obj.pixel_spacing_at(frame_index);
    if resolution.valid()
        && finite_positive(resolution.row_spacing)
        && finite_positive(resolution.column_spacing)
    {
        (row_spacing, column_spacing, grid.semantic) = (
            resolution.row_spacing,
            resolution.column_spacing,
            resolution.semantic,
        );
    } else {
        grid.default_spacing = true;
    }
    Some((
        SliceGeometry {
            row_dir: Vec3::new(1.0, 0.0, 0.0),
            col_dir: Vec3::new(0.0, 1.0, 0.0),
            normal: Vec3::new(0.0, 0.0, 1.0),
            row_spacing,
            col_spacing: column_spacing,
            rows,
            columns,
            ..SliceGeometry::default()
        },
        grid,
    ))
}
fn plan_parametric_source(
    plan: &mut ExportPlan,
    file: &File,
    source_index: i64,
    options: &Options,
) -> Result<(), Error> {
    let pixels = |cause: Option<Cause>| {
        export_error(
            ErrorCode::Pixels,
            source_index,
            -1,
            Some(S::UnsupportedPixels),
            cause,
        )
    };
    if !plan.frames.is_empty() && !plan.parametric {
        return Err(pixels(None));
    }
    let pm =
        parametricmap::read_metadata(file).map_err(|e| pixels(Some(Cause::text(e.to_string()))))?;
    let payload = parametric_payload_bytes(&file.dataset);
    let working = payload.and_then(|p| parametric_working_set_bytes(p, pm.rows, pm.columns));
    match working {
        Some(w) if w <= options.limits.max_in_memory_source_bytes => {}
        _ => return Err(bare(ErrorCode::Limit, source_index, -1, S::LimitExceeded)),
    }
    if pm.number_of_frames > options.limits.max_frames - plan.frames.len() as i64 {
        return Err(bare(ErrorCode::Limit, source_index, -1, S::LimitExceeded));
    }
    let temporal = match dynamic::read_frame_metadata(file) {
        Ok(t) if t.len() as i64 == pm.number_of_frames => t,
        Ok(_) => {
            return Err(bare(
                ErrorCode::Temporal,
                source_index,
                -1,
                S::UnsupportedTemporal,
            ));
        }
        Err(e) => {
            return Err(export_error(
                ErrorCode::Temporal,
                source_index,
                -1,
                Some(S::UnsupportedTemporal),
                Some(Cause::text(e.into_message())),
            ));
        }
    };
    let unit_code = trim_text(&pm.units.value).to_string();
    let unit_scheme = trim_text(&pm.units.scheme).to_string();
    let quantity_code = trim_text(&pm.quantity.value).to_string();
    let quantity_scheme = trim_text(&pm.quantity.scheme).to_string();
    if plan.parametric
        && (unit_code != plan.unit_code
            || unit_scheme != plan.unit_scheme
            || quantity_code != plan.quantity_code
            || quantity_scheme != plan.quantity_scheme)
    {
        return Err(pixels(None));
    }
    plan.parametric = true;
    plan.unit_code = unit_code;
    plan.unit_scheme = unit_scheme;
    plan.quantity_code = quantity_code;
    plan.quantity_scheme = quantity_scheme;
    for frame_index in 0..pm.number_of_frames {
        let mut frame_temporal = temporal[frame_index as usize].clone();
        frame_temporal.frame_index = plan.frames.len() as i64;
        let ordinal = plan.frames.len() as i64;
        plan.frames.push(FramePlan {
            source_index,
            frame_index,
            ordinal,
            geometry: pm.frames[frame_index as usize].geometry,
            temporal: frame_temporal,
            metadata: Metadata {
                rows: pm.rows as u16,
                columns: pm.columns as u16,
                samples_per_pixel: 1,
                bits_allocated: 64,
                bits_stored: 64,
                high_bit: 63,
                number_of_frames: pm.number_of_frames,
                photometric_interpretation: "MONOCHROME2".into(),
                ..Metadata::default()
            },
            rescale: LinearTransform::default(),
            parametric: true,
            spool_offset: 0,
            spool_length: 0,
            slice_spacing: SliceSpacing {
                value: 0.0,
                source: slice_spacing_source::UNKNOWN,
                negative: false,
            },
            grid: ImageGridInfo::default(),
        });
    }
    Ok(())
}
pub(super) fn parametric_payload_bytes(obj: &Object) -> Option<u64> {
    for tag in [
        TAG_FLOAT_PIXEL_DATA,
        TAG_DOUBLE_FLOAT_PIXEL_DATA,
        TAG_PIXEL_DATA,
    ] {
        if let Some(element) = obj.get(tag) {
            return Some(u64::from(crate::object::element_length(element)));
        }
    }
    None
}
pub(super) fn parametric_working_set_bytes(payload: u64, rows: i64, columns: i64) -> Option<u64> {
    if rows <= 0 || columns <= 0 {
        return None;
    }
    let frame_voxels = checked_mul(rows as u64, columns as u64)?;
    let frame_buffers = checked_mul(frame_voxels, 16)?;
    checked_add(payload, frame_buffers)
}
fn valid_pixel_metadata(m: &Metadata) -> bool {
    if m.rows == 0 || m.columns == 0 || m.number_of_frames <= 0 || m.samples_per_pixel != 1 {
        return false;
    }
    let photometric =
        crate::textfmt::to_upper(trim_text(&m.photometric_interpretation).trim_matches('\0'));
    if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
        return false;
    }
    if !matches!(m.bits_allocated, 8 | 16 | 32) {
        return false;
    }
    !(m.pixel_representation > 1
        || m.bits_stored == 0
        || m.bits_stored > m.bits_allocated
        || m.high_bit >= m.bits_allocated
        || m.high_bit + 1 < m.bits_stored)
}
fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
pub(crate) fn strict_slice_geometry(
    input: &FrameGeometry,
    rows: i64,
    columns: i64,
) -> Option<SliceGeometry> {
    let ipp = input.image_position_patient.as_deref().unwrap_or(&[]);
    let iop = input.image_orientation_patient.as_deref().unwrap_or(&[]);
    let ps = input.pixel_spacing.as_deref().unwrap_or(&[]);
    if rows <= 0 || columns <= 0 || ipp.len() != 3 || iop.len() != 6 || ps.len() != 2 {
        return None;
    }
    let mut row = Vec3::new(iop[0], iop[1], iop[2]);
    let mut column = Vec3::new(iop[3], iop[4], iop[5]);
    let origin = Vec3::new(ipp[0], ipp[1], ipp[2]);
    let deviation = image_orientation_deviation(iop);
    match deviation {
        Some(d)
            if finite_vec(origin)
                && d <= IMAGE_ORIENTATION_TOLERANCE
                && finite_positive(ps[0])
                && finite_positive(ps[1]) => {}
        _ => return None,
    }
    row = row.normalize();
    column = column.sub(row.scale(column.dot(row))).normalize();
    let normal = row.cross(column).normalize();
    if normal == Vec3::default() {
        return None;
    }
    Some(SliceGeometry {
        origin,
        row_dir: row,
        col_dir: column,
        normal,
        row_spacing: ps[0],
        col_spacing: ps[1],
        rows,
        columns,
    })
}
pub(super) fn frame_linear_transform(obj: &Object, frame_index: i64) -> Option<LinearTransform> {
    let mut transform = overlay_linear_transform(LinearTransform::default(), obj)?;
    if let (Some(item), _, _) =
        obj.functional_group_item(frame_index, TAG_PIXEL_VALUE_TRANSFORMATION)
    {
        transform = overlay_linear_transform(transform, &item)?;
    }
    if !finite(transform.slope) || transform.slope == 0.0 || !finite(transform.intercept) {
        return None;
    }
    Some(transform)
}
fn overlay_linear_transform(mut base: LinearTransform, obj: &Object) -> Option<LinearTransform> {
    if obj
        .get_sequence(TAG_MODALITY_LUT_SEQUENCE)
        .is_some_and(|items| !items.is_empty())
    {
        return None;
    }
    if obj.get(TAG_RESCALE_SLOPE).is_some() {
        let value = obj.get_float(TAG_RESCALE_SLOPE).ok()?;
        if !finite(value) || value == 0.0 {
            return None;
        }
        base.slope = value;
    }
    if obj.get(TAG_RESCALE_INTERCEPT).is_some() {
        let value = obj.get_float(TAG_RESCALE_INTERCEPT).ok()?;
        if !finite(value) {
            return None;
        }
        base.intercept = value;
    }
    Some(base)
}
fn geometry_export_error(frame: Option<&FramePlan>, issue: GeometryIssue) -> Error {
    let (source_index, frame_index) = frame.map_or((-1, -1), |f| (f.source_index, f.frame_index));
    let mut err = bare(
        ErrorCode::Geometry,
        source_index,
        frame_index,
        S::InvalidGeometry,
    );
    if let Error::Export(e) = &mut err {
        e.geometry_issue = issue;
    }
    err
}
fn resamples(policy: u8) -> bool {
    policy == GEOMETRY_RESAMPLE_LINEAR || policy == GEOMETRY_RESAMPLE_CUBIC_BSPLINE
}
fn plan_time_and_geometry(plan: &mut ExportPlan, options: &Options) -> Result<(), Error> {
    let mut metadata = Vec::with_capacity(plan.frames.len());
    let mut declared_temporal_positions = 0i64;
    for (index, frame) in plan.frames.iter().enumerate() {
        let mut m = frame.temporal.clone();
        m.frame_index = index as i64;
        if m.has_number_of_temporal_positions {
            let declared = m.number_of_temporal_positions;
            if declared <= 0
                || (declared_temporal_positions != 0 && declared != declared_temporal_positions)
            {
                return Err(bare(
                    ErrorCode::Temporal,
                    frame.source_index,
                    frame.frame_index,
                    S::UnsupportedTemporal,
                ));
            }
            declared_temporal_positions = declared;
        }
        metadata.push(m);
    }
    let timeline = dynamic::build(&metadata);
    let points = timeline.points();
    if points.is_empty() || timeline.multiple_stacks {
        return Err(bare(
            ErrorCode::MultipleStacks,
            -1,
            -1,
            S::UnsupportedTemporal,
        ));
    }
    if timeline.used_occurrence_fallback
        || (declared_temporal_positions != 0 && points.len() as i64 != declared_temporal_positions)
    {
        return Err(bare(ErrorCode::Temporal, -1, -1, S::UnsupportedTemporal));
    }
    if timeline.dynamic {
        for frame in &metadata {
            if !frame.has_temporal_position
                && !frame.has_offset
                && !frame.has_trigger
                && !frame.has_phase
            {
                return Err(bare(ErrorCode::Temporal, -1, -1, S::UnsupportedTemporal));
            }
        }
    }
    if options.single_slice != SINGLE_SLICE_REJECT
        && plan.frames.len() == 1
        && points.len() == 1
        && points[0].stacks.len() == 1
        && points[0].stacks[0].frame_indices.len() == 1
    {
        return plan_single_slice(plan, points[0].clone(), options);
    }
    if trim_text(&plan.frame_of_reference_uid).is_empty() {
        return Err(bare(ErrorCode::Identity, -1, -1, S::MixedIdentity));
    }
    plan.slice_spacing_source = slice_spacing_source::FROM_POSITIONS;
    let mut output_ordinal = 0i64;
    for point in points {
        if point.stacks.len() != 1 || point.stacks[0].frame_indices.len() < 2 {
            let code = if point.stacks.len() == 1 {
                ErrorCode::SingleSlice
            } else {
                ErrorCode::MultipleStacks
            };
            return Err(bare(code, -1, -1, S::InvalidGeometry));
        }
        let mut frames = Vec::with_capacity(point.stacks[0].frame_indices.len());
        for &frame_index in &point.stacks[0].frame_indices {
            if frame_index < 0 || frame_index as usize >= plan.frames.len() {
                return Err(bare(ErrorCode::Temporal, -1, -1, S::UnsupportedTemporal));
            }
            frames.push(frame_index as usize);
        }
        let ref_normal = plan.frames[frames[0]].geometry.normal;
        let all = &plan.frames;
        slice_stable(&mut frames, |&a, &b| {
            all[a].geometry.position_along(ref_normal) < all[b].geometry.position_along(ref_normal)
        });
        for &frame in &frames {
            if plan.frames[frame].ordinal != output_ordinal {
                plan.reordered = true;
            }
            output_ordinal += 1;
        }
        let geometries: Vec<SliceGeometry> =
            frames.iter().map(|&f| plan.frames[f].geometry).collect();
        let geometry = build_volume_geometry(&geometries, options.tolerances);
        if geometry.disposition == GeometryDisposition::Unsupported
            || (geometry.requires_resampling && !resamples(options.geometry))
        {
            return Err(geometry_export_error(
                Some(&plan.frames[frames[0]]),
                geometry.primary_issue,
            ));
        }
        if geometry.requires_resampling {
            plan.resampled = true;
        }
        plan.time_points.push(TimePointPlan {
            frames,
            geometry,
            regular_grid: RegularGridGeometry::default(),
            timing: point.clone(),
        });
    }
    if plan.resampled {
        for index in 0..plan.time_points.len() {
            match plan_regular_grid_geometry(&plan.time_points[index].geometry) {
                Ok(grid) => plan.time_points[index].regular_grid = grid,
                Err(e) => {
                    let first = &plan.frames[plan.time_points[index].frames[0]];
                    return Err(export_error(
                        ErrorCode::Geometry,
                        first.source_index,
                        first.frame_index,
                        Some(S::InvalidGeometry),
                        Some(Cause::text(e.to_string())),
                    ));
                }
            }
        }
    }
    let first = &plan.time_points[0];
    if plan.resampled {
        let g = &first.regular_grid;
        plan.dimensions = [
            g.dimensions[0],
            g.dimensions[1],
            g.dimensions[2],
            plan.time_points.len() as i64,
        ];
        plan.affine_lps = g.voxel_to_patient_affine;
        plan.spacing = [g.col_spacing, g.row_spacing, g.slice_spacing, 1.0];
    } else {
        let s0 = first.geometry.slices()[0];
        plan.dimensions = [
            s0.columns,
            s0.rows,
            first.frames.len() as i64,
            plan.time_points.len() as i64,
        ];
        plan.affine_lps = first.geometry.voxel_to_patient_affine;
        plan.spacing = [
            s0.col_spacing,
            s0.row_spacing,
            affine_column_norm(&plan.affine_lps, 2),
            1.0,
        ];
    }
    for time_index in 1..plan.time_points.len() {
        let point = &plan.time_points[time_index];
        let mismatch = if plan.resampled {
            point.regular_grid.dimensions
                != [plan.dimensions[0], plan.dimensions[1], plan.dimensions[2]]
                || !affine_close(
                    &point.regular_grid.voxel_to_patient_affine,
                    &plan.affine_lps,
                    1e-5,
                )
        } else {
            let s0 = point.geometry.slices()[0];
            s0.columns != plan.dimensions[0]
                || s0.rows != plan.dimensions[1]
                || point.frames.len() as i64 != plan.dimensions[2]
                || !affine_close(
                    &point.geometry.voxel_to_patient_affine,
                    &plan.affine_lps,
                    1e-5,
                )
        };
        if mismatch {
            return Err(bare(ErrorCode::Temporal, -1, -1, S::UnsupportedTemporal));
        }
    }
    plan.spacing[3] = temporal_spacing(&plan.time_points, options.temporal_spacing_seconds)?;
    validate_plan_dimensions(plan, options)
}
fn plan_single_slice(
    plan: &mut ExportPlan,
    point: TimePoint,
    options: &Options,
) -> Result<(), Error> {
    let index = point.stacks[0].frame_indices[0];
    if index < 0 || index as usize >= plan.frames.len() {
        return Err(bare(ErrorCode::Temporal, -1, -1, S::UnsupportedTemporal));
    }
    let frame = plan.frames[index as usize].clone();
    let (geometry, spacing) = (frame.geometry, frame.slice_spacing);
    if !finite_positive(spacing.value) || !fits_float32_spacing(spacing.value) {
        return Err(bare(
            ErrorCode::Geometry,
            frame.source_index,
            frame.frame_index,
            S::InvalidGeometry,
        ));
    }
    if spacing.source == slice_spacing_source::FROM_SLICE_THICKNESS {
        plan.warnings
            .push(WARNING_SLICE_SPACING_FROM_THICKNESS.into());
    } else if spacing.source == slice_spacing_source::DEFAULT {
        plan.warnings.push(WARNING_SLICE_SPACING_DEFAULT.into());
    }
    if frame.grid.used {
        plan.image_grid = frame.grid;
        plan.warnings.push(WARNING_NO_PATIENT_GEOMETRY.into());
        if frame.grid.default_spacing {
            plan.warnings.push(WARNING_PIXEL_SPACING_DEFAULT.into());
        }
    }
    let x = geometry.row_dir.scale(geometry.col_spacing);
    let y = geometry.col_dir.scale(geometry.row_spacing);
    let z = geometry.normal.scale(spacing.value);
    let o = geometry.origin;
    plan.affine_lps = GeometryAffine([
        x.x, y.x, z.x, o.x, x.y, y.y, z.y, o.y, x.z, y.z, z.z, o.z, 0.0, 0.0, 0.0, 1.0,
    ]);
    plan.dimensions = [geometry.columns, geometry.rows, 1, 1];
    plan.spacing = [
        geometry.col_spacing,
        geometry.row_spacing,
        spacing.value,
        1.0,
    ];
    plan.time_points = vec![TimePointPlan {
        frames: vec![index as usize],
        geometry: VolumeGeometry::default(),
        regular_grid: RegularGridGeometry::default(),
        timing: point,
    }];
    plan.single_slice = true;
    plan.slice_spacing_source = spacing.source;
    if !frame.grid.used && trim_text(&plan.frame_of_reference_uid).is_empty() {
        return Err(bare(
            ErrorCode::Identity,
            frame.source_index,
            -1,
            S::MixedIdentity,
        ));
    }
    validate_plan_dimensions(plan, options)
}
fn fits_float32_spacing(value: f64) -> bool {
    let converted = value as f32;
    converted > 0.0 && !converted.is_infinite()
}
pub(super) fn duration_seconds(d: i64) -> f64 {
    let sec = d / 1_000_000_000;
    let nsec = d % 1_000_000_000;
    sec as f64 + nsec as f64 / 1e9
}
fn abs_duration(d: i64) -> i64 {
    if d < 0 { d.wrapping_neg() } else { d }
}
fn temporal_spacing(points: &[TimePointPlan], over: f64) -> Result<f64, Error> {
    if points.len() <= 1 {
        return Ok(1.0);
    }
    let irregular = || bare(ErrorCode::IrregularTime, -1, -1, S::UnsupportedTemporal);
    let missing = || bare(ErrorCode::MissingTiming, -1, -1, S::UnsupportedTemporal);
    if points.iter().all(|p| p.timing.has_offset) {
        let first = points[1]
            .timing
            .offset
            .wrapping_sub(points[0].timing.offset);
        if first <= 0 {
            return Err(irregular());
        }
        for index in 2..points.len() {
            let delta = points[index]
                .timing
                .offset
                .wrapping_sub(points[index - 1].timing.offset);
            if delta <= 0 || abs_duration(delta.wrapping_sub(first)) > 1_000 {
                return Err(irregular());
            }
        }
        let spacing = duration_seconds(first);
        if over != 0.0 && (!finite_positive(over) || (over - spacing).abs() > 1e-6) {
            return Err(irregular());
        }
        return Ok(spacing);
    }
    if points
        .iter()
        .all(|p| p.timing.has_duration && p.timing.duration > 0)
    {
        let first = points[0].timing.duration;
        for point in &points[1..] {
            if abs_duration(point.timing.duration.wrapping_sub(first)) > 1_000 {
                return Err(irregular());
            }
        }
        let spacing = duration_seconds(first);
        if over != 0.0 && (!finite_positive(over) || (over - spacing).abs() > 1e-6) {
            return Err(irregular());
        }
        return Ok(spacing);
    }
    if over != 0.0 {
        if !finite_positive(over) {
            return Err(missing());
        }
        return Ok(over);
    }
    Err(missing())
}
fn validate_plan_dimensions(plan: &ExportPlan, options: &Options) -> Result<(), Error> {
    let mut voxels = 1u64;
    for &dimension in &plan.dimensions {
        if dimension <= 0 || dimension > i64::from(i16::MAX) {
            return Err(bare(ErrorCode::NIfTI2, -1, -1, S::NIfTI2Required));
        }
        match checked_mul(voxels, dimension as u64) {
            Some(v) if v <= options.limits.max_voxels => voxels = v,
            _ => return Err(bare(ErrorCode::Limit, -1, -1, S::LimitExceeded)),
        }
    }
    if plan.resampled && voxels > options.limits.max_resample_voxels {
        return Err(bare(ErrorCode::Limit, -1, -1, S::LimitExceeded));
    }
    Ok(())
}
fn choose_encoding(plan: &mut ExportPlan, options: &Options) -> Result<(), Error> {
    if plan.parametric {
        if plan.resampled {
            return Err(bare(ErrorCode::Geometry, -1, -1, S::InvalidGeometry));
        }
        (plan.datatype, plan.bitpix) = (DATATYPE_FLOAT64, 64);
        (plan.spool_datatype, plan.spool_bitpix) = (plan.datatype, plan.bitpix);
        (plan.header_slope, plan.header_intercept) = (1.0, 0.0);
        plan.scaling_policy = SCALING_APPLY_FLOAT64;
        return validate_output_size(plan, options);
    }
    let first = plan.frames[0].clone();
    for frame in &plan.frames[1..] {
        let (a, b) = (&frame.metadata, &first.metadata);
        if a.rows != b.rows
            || a.columns != b.columns
            || a.samples_per_pixel != b.samples_per_pixel
            || a.bits_allocated != b.bits_allocated
            || a.pixel_representation != b.pixel_representation
        {
            return Err(bare(
                ErrorCode::Pixels,
                frame.source_index,
                frame.frame_index,
                S::UnsupportedPixels,
            ));
        }
    }
    let Some((stored_datatype, stored_bitpix)) = integer_datatype(&first.metadata) else {
        return Err(bare(
            ErrorCode::Pixels,
            first.source_index,
            first.frame_index,
            S::UnsupportedPixels,
        ));
    };
    (plan.spool_datatype, plan.spool_bitpix) = (stored_datatype, stored_bitpix);
    if plan.resampled {
        (plan.datatype, plan.bitpix) = (DATATYPE_FLOAT32, 32);
        (plan.header_slope, plan.header_intercept) = (1.0, 0.0);
        plan.scaling_policy = SCALING_APPLY_FLOAT32;
    } else if options.scaling == SCALING_APPLY_FLOAT32 {
        (plan.datatype, plan.bitpix) = (DATATYPE_FLOAT32, 32);
        (plan.spool_datatype, plan.spool_bitpix) = (plan.datatype, plan.bitpix);
        (plan.header_slope, plan.header_intercept) = (1.0, 0.0);
        plan.scaling_policy = SCALING_APPLY_FLOAT32;
    } else if options.scaling == SCALING_APPLY_FLOAT64 {
        (plan.datatype, plan.bitpix) = (DATATYPE_FLOAT64, 64);
        (plan.spool_datatype, plan.spool_bitpix) = (plan.datatype, plan.bitpix);
        (plan.header_slope, plan.header_intercept) = (1.0, 0.0);
        plan.scaling_policy = SCALING_APPLY_FLOAT64;
    } else {
        (plan.datatype, plan.bitpix) = (stored_datatype, stored_bitpix);
        (plan.header_slope, plan.header_intercept) = (first.rescale.slope, first.rescale.intercept);
        plan.scaling_policy = SCALING_PRESERVE_UNIFORM;
        for frame in &plan.frames[1..] {
            if frame.rescale != first.rescale {
                return Err(bare(
                    ErrorCode::Scaling,
                    frame.source_index,
                    frame.frame_index,
                    S::UnsupportedScaling,
                ));
            }
        }
        if !float32_representable(plan.header_slope)
            || !float32_representable(plan.header_intercept)
        {
            return Err(bare(ErrorCode::Scaling, -1, -1, S::UnsupportedScaling));
        }
    }
    validate_output_size(plan, options)
}
fn validate_output_size(plan: &ExportPlan, options: &Options) -> Result<(), Error> {
    let mut voxels = 1u64;
    for &dimension in &plan.dimensions {
        voxels = checked_mul(voxels, dimension as u64)
            .ok_or_else(|| bare(ErrorCode::Limit, -1, -1, S::LimitExceeded))?;
    }
    match checked_mul(voxels, (plan.bitpix / 8) as u64) {
        Some(bytes) if bytes <= options.limits.max_uncompressed_bytes => Ok(()),
        _ => Err(bare(ErrorCode::Limit, -1, -1, S::LimitExceeded)),
    }
}
pub(super) fn integer_datatype(m: &Metadata) -> Option<(i16, i16)> {
    let signed = m.pixel_representation == 1;
    match m.bits_allocated {
        8 => Some(if signed {
            (DATATYPE_INT8, 8)
        } else {
            (DATATYPE_UINT8, 8)
        }),
        16 => Some(if signed {
            (DATATYPE_INT16, 16)
        } else {
            (DATATYPE_UINT16, 16)
        }),
        32 => Some(if signed {
            (DATATYPE_INT32, 32)
        } else {
            (DATATYPE_UINT32, 32)
        }),
        _ => None,
    }
}
fn affine_column_norm(affine: &GeometryAffine, column: usize) -> f64 {
    let a = &affine.0;
    (a[column] * a[column] + a[4 + column] * a[4 + column] + a[8 + column] * a[8 + column]).sqrt()
}
pub(super) fn affine_close(a: &GeometryAffine, b: &GeometryAffine, tolerance: f64) -> bool {
    for index in 0..16 {
        let (x, y) = (a.0[index], b.0[index]);
        if (x - y).abs() > tolerance * fmath::max(1.0, fmath::max(x.abs(), y.abs())) {
            return false;
        }
    }
    true
}
fn float32_representable(value: f64) -> bool {
    let converted = value as f32;
    if converted.is_nan() || converted.is_infinite() {
        return false;
    }
    let delta = (value - f64::from(converted)).abs();
    delta <= 1e-7 * fmath::max(1.0, value.abs())
}
