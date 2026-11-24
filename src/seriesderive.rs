#![forbid(unsafe_code)]
use crate::derivation::{self, Derivation, Source, codes};
use crate::derivedio as dio;
use crate::display::StoredSampleReader;
use crate::fmath;
use crate::iconimage::pixel_padding_from_object;
use crate::object::clone::{
    CloneFileOptions, clone_file, clone_file_with_options, default_clone_file_options,
};
use crate::object::element_setters::{format_ds, us_element};
use crate::object::geometry::image_orientation_deviation;
use crate::object::{
    self, Element, ElementHeader, File, Kind, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
    element_length, is_valid_uid,
};
use crate::pixel::pixeldata::{extract_metadata, extract_native_frames};
use crate::validation::iod;
use crate::validation::{Mode, ValidationOptions, validate_file};
pub const ERR_MISSING_SERIES_INSTANCE_UID: &str =
    "dicom/seriesderive: missing derived Series Instance UID";
pub const ERR_MISSING_SOP_INSTANCE_UID: &str =
    "dicom/seriesderive: missing derived SOP Instance UID";
pub const ERR_GEOMETRIC_PROFILE: &str = "dicom/seriesderive: unsupported geometric image profile";
pub const ERR_GEOMETRIC_OPTIONS: &str = "dicom/seriesderive: invalid geometric options";
pub const ERR_GEOMETRIC_LIMIT: &str = "dicom/seriesderive: geometric resource limit exceeded";
pub const ERR_GEOMETRIC_VALIDATION: &str = "dicom/seriesderive: geometric image validation failed";
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_DERIVATION_DESCRIPTION: Tag = Tag::new(0x0008, 0x2111);
const TAG_SOURCE_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x2112);
const TAG_SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
const TAG_POSITION: Tag = Tag::new(0x0020, 0x0032);
const TAG_ORIENTATION: Tag = Tag::new(0x0020, 0x0037);
const TAG_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const ORIENTATION_TOLERANCE: f64 = 1e-6;
const CT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2";
const MR_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.4";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    MissingSeriesInstanceUid,
    MissingSopInstanceUid,
    GeometricProfile,
    GeometricOptions,
    GeometricLimit,
    GeometricValidation,
    Object,
    Derivation,
    Generator,
}
#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub text: String,
    pub object: Option<object::Error>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn bare(kind: ErrorKind) -> Error {
        let text = match kind {
            ErrorKind::MissingSeriesInstanceUid => ERR_MISSING_SERIES_INSTANCE_UID,
            ErrorKind::MissingSopInstanceUid => ERR_MISSING_SOP_INSTANCE_UID,
            ErrorKind::GeometricProfile => ERR_GEOMETRIC_PROFILE,
            ErrorKind::GeometricOptions => ERR_GEOMETRIC_OPTIONS,
            ErrorKind::GeometricLimit => ERR_GEOMETRIC_LIMIT,
            ErrorKind::GeometricValidation => ERR_GEOMETRIC_VALIDATION,
            ErrorKind::Object | ErrorKind::Derivation | ErrorKind::Generator => "",
        };
        Error {
            kind,
            text: text.to_string(),
            object: None,
        }
    }
    fn detail(kind: ErrorKind, detail: &str) -> Error {
        let mut e = Error::bare(kind);
        e.text = format!("{}: {detail}", e.text);
        e
    }
    fn object(err: object::Error) -> Error {
        Error {
            kind: ErrorKind::Object,
            text: err.to_string(),
            object: Some(err),
        }
    }
    fn object_wrapped(prefix: &str, err: object::Error) -> Error {
        Error {
            kind: ErrorKind::Object,
            text: format!("{prefix}: {err}"),
            object: Some(err),
        }
    }
    fn derivation(text: String) -> Error {
        Error {
            kind: ErrorKind::Derivation,
            text,
            object: None,
        }
    }
}
fn profile_err(detail: &str) -> Error {
    Error::detail(ErrorKind::GeometricProfile, detail)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub series_description: String,
    pub derivation_description: String,
    pub series_number: String,
}
fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}')
}
pub fn clone_to_series(src: Option<&File>, opts: &Options) -> Result<File, Error> {
    let Some(src) = src else {
        return Err(Error::object(object::Error::sentinel(Kind::NilFile)));
    };
    let series_uid = trim_space(&opts.series_instance_uid);
    if series_uid.is_empty() {
        return Err(Error::bare(ErrorKind::MissingSeriesInstanceUid));
    }
    let sop_uid = trim_space(&opts.sop_instance_uid);
    if sop_uid.is_empty() {
        return Err(Error::bare(ErrorKind::MissingSopInstanceUid));
    }
    let source_class_uid = match src.dataset.get_string(dio::TAG_SOP_CLASS_UID) {
        Some(v) if !trim_space(&v).is_empty() => v,
        _ => {
            return Err(Error::object(object::Error::sentinel(
                Kind::MissingSopClassUid,
            )));
        }
    };
    let source_sop_uid = match src.dataset.get_string(dio::TAG_SOP_INSTANCE_UID) {
        Some(v) if !trim_space(&v).is_empty() => v,
        _ => {
            return Err(Error::object(object::Error::sentinel(
                Kind::MissingSopInstanceUid,
            )));
        }
    };
    let mut clone = clone_file(src)
        .map_err(|e| Error::object_wrapped("dicom/seriesderive: clone source", e))?;
    let ds = &mut clone.dataset;
    ds.put(dio::ui(dio::TAG_SERIES_INSTANCE_UID, series_uid));
    ds.put(dio::ui(dio::TAG_SOP_INSTANCE_UID, sop_uid));
    let image_type = derived_image_type(ds);
    ds.put(dio::strings(TAG_IMAGE_TYPE, Vr::CS, &image_type));
    ds.remove(TAG_SOURCE_IMAGE_SEQUENCE);
    if !trim_space(&opts.derivation_description).is_empty() {
        ds.remove(TAG_DERIVATION_DESCRIPTION);
    }
    derivation::apply(
        Some(ds),
        &Derivation {
            description: opts.derivation_description.clone(),
            codes: Vec::new(),
            sources: vec![Source {
                sop_class_uid: source_class_uid,
                sop_instance_uid: source_sop_uid,
                frames: Vec::new(),
                purpose: None,
            }],
        },
    )
    .map_err(|e| Error::derivation(format!("dicom/seriesderive: {e}")))?;
    let value = trim_space(&opts.series_description);
    if !value.is_empty() {
        ds.put(dio::lo(dio::TAG_SERIES_DESCR, value));
    }
    let value = trim_space(&opts.series_number);
    if !value.is_empty() {
        ds.put(dio::str_elem(TAG_SERIES_NUMBER, Vr::IS, value));
    }
    clone
        .rebuild_file_meta()
        .map_err(|e| Error::object_wrapped("dicom/seriesderive: rebuild file meta", e))?;
    Ok(clone)
}
fn derived_image_type(dataset: &Object) -> Vec<String> {
    let values = dataset.get_strings(TAG_IMAGE_TYPE).unwrap_or_default();
    let mut out = vec!["DERIVED".to_string(), "SECONDARY".to_string()];
    if values.len() > 2 {
        out.extend_from_slice(&values[2..]);
    }
    out
}
pub type Interpolation = u8;
pub const NEAREST: Interpolation = 0;
pub const BILINEAR: Interpolation = 1;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CropRegion {
    pub column: i64,
    pub row: i64,
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeometricLimits {
    pub max_input_pixels: i64,
    pub max_output_pixels: i64,
    pub clone: CloneFileOptions,
}
pub fn default_geometric_limits() -> GeometricLimits {
    let mut c = default_clone_file_options();
    c.max_element_bytes = 256 << 20;
    c.max_pixel_data_bytes = 128 << 20;
    c.max_total_bytes = 256 << 20;
    GeometricLimits {
        max_input_pixels: 64 << 20,
        max_output_pixels: 64 << 20,
        clone: c,
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeometricOptions {
    pub crop: Option<CropRegion>,
    pub rows: i64,
    pub columns: i64,
    pub interpolation: Interpolation,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub rotate_clockwise: i64,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub series_description: String,
    pub derivation_description: String,
    pub limits: GeometricLimits,
}
pub type UidGenerator<'a> = dyn FnMut() -> Result<String, crate::BoxError> + 'a;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GeometricGeometry {
    pub position: [f64; 3],
    pub orientation: [f64; 6],
    pub spacing: [f64; 2],
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometricReport {
    pub source_rows: i64,
    pub source_columns: i64,
    pub rows: i64,
    pub columns: i64,
    pub crop: CropRegion,
    pub interpolation: Interpolation,
    pub source: GeometricGeometry,
    pub output: GeometricGeometry,
    pub source_from_output: [[f64; 3]; 2],
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub input_iod: iod::Report,
    pub output_iod: iod::Report,
}
pub fn transform_image(
    src: Option<&File>,
    opts: &GeometricOptions,
    generator: Option<&mut UidGenerator<'_>>,
) -> Result<(File, GeometricReport), Error> {
    let limits = geometric_limits(opts.limits)?;
    geometric_options(opts)?;
    let profile = geometric_profile(src, &limits)?;
    let Some(src) = src else {
        return Err(Error::object(object::Error::sentinel(Kind::NilFile)));
    };
    let grid = geometric_grid(&profile, opts, &limits)?;
    let input_iod = validate_geometric_file(src, &limits)?;
    let mut work = clone_file_with_options(src, limits.clone).map_err(|e| Error {
        kind: ErrorKind::GeometricLimit,
        text: format!("{ERR_GEOMETRIC_LIMIT}: source clone: {e}"),
        object: Some(e),
    })?;
    validate_geometric_file(&work, &limits)?;
    geometric_profile(Some(&work), &limits)?;
    let frames = match extract_native_frames(&work.dataset) {
        Ok(f)
            if f.data.len() == 1
                && f.data[0].len() as i64 == profile.rows * profile.columns * 2 =>
        {
            f
        }
        _ => return Err(profile_err("native frame layout")),
    };
    let big_endian = work.dataset.value_big_endian();
    let md = &frames.metadata;
    let bits_stored = i64::from(md.bits_stored);
    let reader = StoredSampleReader::new(
        i64::from(md.bits_allocated),
        bits_stored,
        i64::from(md.high_bit),
        md.pixel_representation != 0,
        big_endian,
    )
    .map_err(|_| profile_err("pixel padding"))?;
    let padding = match pixel_padding_from_object(&work.dataset, &reader, bits_stored) {
        Ok(p) if !(work.dataset.has(Tag::new(0x0028, 0x0121)) && p.is_none()) => p,
        _ => return Err(profile_err("pixel padding")),
    };
    if padding.is_some() && opts.interpolation == BILINEAR {
        return Err(profile_err("bilinear interpolation with pixel padding"));
    }
    let (series_uid, sop_uid) = geometric_uids(&work.dataset, &profile, opts, generator)?;
    let pixels = resample_geometric(
        &frames.data[0],
        &reader,
        bits_stored,
        big_endian,
        &grid,
        opts.interpolation,
    );
    let dataset = &mut work.dataset;
    dataset.put(us_element(dio::TAG_ROWS, big_endian, &[grid.rows as u16]));
    dataset.put(us_element(
        dio::TAG_COLUMNS,
        big_endian,
        &[grid.columns as u16],
    ));
    dataset.put(Element {
        header: ElementHeader {
            tag: TAG_PIXEL_DATA,
            vr: Vr::OW,
            length: 0,
            length_set: false,
        },
        value: Value::Raw(pixels),
    });
    dataset.put(dio::ds(TAG_POSITION, &grid.geometry.position));
    dataset.put(dio::ds(TAG_ORIENTATION, &grid.geometry.orientation));
    dataset.put(dio::ds(TAG_SPACING, &grid.geometry.spacing));
    dataset.put(dio::ui(dio::TAG_SERIES_INSTANCE_UID, &series_uid));
    dataset.put(dio::ui(dio::TAG_SOP_INSTANCE_UID, &sop_uid));
    let image_type = derived_image_type(dataset);
    dataset.put(dio::strings(TAG_IMAGE_TYPE, Vr::CS, &image_type));
    clear_geometric_grid_metadata(dataset);
    if !opts.series_description.is_empty() {
        dataset.put(dio::lo(dio::TAG_SERIES_DESCR, &opts.series_description));
    }
    let mut description = format!(
        "Geometric derivation: crop={},{},{},{}; resize={}x{}; interpolation={}; flip={},{}; clockwise={}; pixel-edge FOV",
        grid.crop.column,
        grid.crop.row,
        grid.crop.width,
        grid.crop.height,
        grid.resize_columns,
        grid.resize_rows,
        if opts.interpolation == BILINEAR {
            "bilinear"
        } else {
            "nearest"
        },
        opts.flip_horizontal,
        opts.flip_vertical,
        opts.rotate_clockwise
    );
    if !opts.derivation_description.is_empty() {
        description.push_str("; ");
        description.push_str(&opts.derivation_description);
    }
    derivation::apply(
        Some(dataset),
        &Derivation {
            description,
            codes: vec![codes::SpatialResampling()],
            sources: vec![Source {
                sop_class_uid: profile.class_uid.clone(),
                sop_instance_uid: profile.sop_uid.clone(),
                frames: Vec::new(),
                purpose: Some(codes::SourceImageForProcessing()),
            }],
        },
    )
    .map_err(|e| Error::derivation(e.to_string()))?;
    work.rebuild_file_meta().map_err(Error::object)?;
    let out = clone_file_with_options(&work, limits.clone).map_err(|e| Error {
        kind: ErrorKind::GeometricLimit,
        text: format!("{ERR_GEOMETRIC_LIMIT}: output clone: {e}"),
        object: Some(e),
    })?;
    let output_iod = validate_geometric_file(&out, &limits)?;
    let mut output_limits = limits;
    output_limits.max_input_pixels = limits.max_output_pixels;
    let output_profile = geometric_profile(Some(&out), &output_limits)?;
    let report = GeometricReport {
        source_rows: profile.rows,
        source_columns: profile.columns,
        rows: grid.rows,
        columns: grid.columns,
        crop: grid.crop,
        interpolation: opts.interpolation,
        source: profile.geometry,
        output: output_profile.geometry,
        source_from_output: grid.inverse,
        series_instance_uid: series_uid,
        sop_instance_uid: sop_uid,
        input_iod,
        output_iod,
    };
    Ok((out, report))
}
fn clear_geometric_grid_metadata(dataset: &mut Object) {
    for (group, element) in [
        (0x0088, 0x0200),
        (0x0018, 0x1050),
        (0x0028, 0x0031),
        (0x0028, 0x0032),
        (0x0028, 0x0034),
        (0x0028, 0x0106),
        (0x0028, 0x0107),
        (0x0028, 0x0108),
        (0x0028, 0x0109),
        (0x0028, 0x0110),
        (0x0028, 0x0111),
        (0x0028, 0x9099),
    ] {
        dataset.remove(Tag::new(group, element));
    }
    let patient_orientation = Tag::new(0x0020, 0x0020);
    if dataset.has(patient_orientation) {
        dataset.put(dio::strings(patient_orientation, Vr::CS, &[]));
    }
}
fn geometric_options(opts: &GeometricOptions) -> Result<(), Error> {
    if opts.interpolation > BILINEAR
        || opts.rows < 0
        || opts.columns < 0
        || !matches!(opts.rotate_clockwise, 0 | 90 | 180 | 270)
    {
        return Err(Error::bare(ErrorKind::GeometricOptions));
    }
    if opts.series_description.len() > 64
        || opts
            .series_description
            .contains(['\\', '\0', '\r', '\n', '\u{c}'])
        || opts.derivation_description.len() > 768
        || opts.derivation_description.contains('\0')
    {
        return Err(Error::detail(ErrorKind::GeometricOptions, "descriptions"));
    }
    for uid in [&opts.series_instance_uid, &opts.sop_instance_uid] {
        if !uid.is_empty() && !is_valid_uid(uid.as_bytes()) {
            return Err(Error::detail(
                ErrorKind::GeometricOptions,
                "output UID syntax",
            ));
        }
    }
    Ok(())
}
struct Grid {
    crop: CropRegion,
    rows: i64,
    columns: i64,
    resize_rows: i64,
    resize_columns: i64,
    source_columns: i64,
    inverse: [[f64; 3]; 2],
    geometry: GeometricGeometry,
    flip_horizontal: bool,
    flip_vertical: bool,
    rotate_clockwise: i64,
}
fn compose(inverse: &mut [[f64; 3]; 2], a: [[f64; 3]; 2]) {
    let before = *inverse;
    for i in 0..2 {
        inverse[i][0] = before[i][0] * a[0][0] + before[i][1] * a[1][0];
        inverse[i][1] = before[i][0] * a[0][1] + before[i][1] * a[1][1];
        inverse[i][2] = before[i][0] * a[0][2] + before[i][1] * a[1][2] + before[i][2];
    }
}
fn geometric_grid(
    source: &Profile,
    opts: &GeometricOptions,
    limits: &GeometricLimits,
) -> Result<Grid, Error> {
    let crop = opts.crop.unwrap_or(CropRegion {
        column: 0,
        row: 0,
        width: source.columns,
        height: source.rows,
    });
    if crop.column < 0
        || crop.row < 0
        || crop.width <= 0
        || crop.height <= 0
        || crop.column > source.columns
        || crop.width > source.columns - crop.column
        || crop.row > source.rows
        || crop.height > source.rows - crop.row
    {
        return Err(Error::detail(
            ErrorKind::GeometricOptions,
            "crop must lie inside the source",
        ));
    }
    let limit = || Error::bare(ErrorKind::GeometricLimit);
    let profile = || Error::bare(ErrorKind::GeometricProfile);
    let (mut rows, mut cols) = (opts.rows, opts.columns);
    if rows > 65535 || cols > 65535 {
        return Err(limit());
    }
    let physical_rows = crop.height as f64 * source.geometry.spacing[0];
    let physical_cols = crop.width as f64 * source.geometry.spacing[1];
    if !physical_rows.is_finite()
        || !physical_cols.is_finite()
        || physical_rows <= 0.0
        || physical_cols <= 0.0
    {
        return Err(profile());
    }
    if rows == 0 && cols == 0 {
        rows = crop.height;
        cols = crop.width;
    } else if rows == 0 {
        let n = (cols as f64 * physical_rows / physical_cols).round();
        if !n.is_finite() || n > 65535.0 {
            return Err(limit());
        }
        rows = fmath::f64_to_i64(fmath::max(1.0, n));
    } else if cols == 0 {
        let n = (rows as f64 * physical_cols / physical_rows).round();
        if !n.is_finite() || n > 65535.0 {
            return Err(limit());
        }
        cols = fmath::f64_to_i64(fmath::max(1.0, n));
    }
    let count = rows * cols;
    if rows <= 0
        || cols <= 0
        || rows > 65535
        || cols > 65535
        || count > limits.max_output_pixels
        || count * 2 > limits.clone.max_pixel_data_bytes
        || count * 2 > limits.clone.max_total_bytes
    {
        return Err(limit());
    }
    let (resize_rows, resize_columns) = (rows, cols);
    let sx = crop.width as f64 / cols as f64;
    let sy = crop.height as f64 / rows as f64;
    let mut inverse = [
        [sx, 0.0, crop.column as f64 + sx * 0.5 - 0.5],
        [0.0, sy, crop.row as f64 + sy * 0.5 - 0.5],
    ];
    if opts.flip_horizontal {
        compose(
            &mut inverse,
            [[-1.0, 0.0, (cols - 1) as f64], [0.0, 1.0, 0.0]],
        );
    }
    if opts.flip_vertical {
        compose(
            &mut inverse,
            [[1.0, 0.0, 0.0], [0.0, -1.0, (rows - 1) as f64]],
        );
    }
    match opts.rotate_clockwise {
        90 => {
            compose(
                &mut inverse,
                [[0.0, 1.0, 0.0], [-1.0, 0.0, (rows - 1) as f64]],
            );
            std::mem::swap(&mut rows, &mut cols);
        }
        180 => compose(
            &mut inverse,
            [
                [-1.0, 0.0, (cols - 1) as f64],
                [0.0, -1.0, (rows - 1) as f64],
            ],
        ),
        270 => {
            compose(
                &mut inverse,
                [[0.0, -1.0, (cols - 1) as f64], [1.0, 0.0, 0.0]],
            );
            std::mem::swap(&mut rows, &mut cols);
        }
        _ => {}
    }
    let mut geometry = GeometricGeometry::default();
    let mut col_step = [0.0f64; 3];
    let mut row_step = [0.0f64; 3];
    for axis in 0..3 {
        let x = source.geometry.orientation[axis] * source.geometry.spacing[1];
        let y = source.geometry.orientation[axis + 3] * source.geometry.spacing[0];
        geometry.position[axis] =
            source.geometry.position[axis] + x * inverse[0][2] + y * inverse[1][2];
        col_step[axis] = x * inverse[0][0] + y * inverse[1][0];
        row_step[axis] = x * inverse[0][1] + y * inverse[1][1];
    }
    geometry.spacing = [
        fmath::hypot(fmath::hypot(row_step[0], row_step[1]), row_step[2]),
        fmath::hypot(fmath::hypot(col_step[0], col_step[1]), col_step[2]),
    ];
    if geometry.spacing[0] <= 0.0 || geometry.spacing[1] <= 0.0 {
        return Err(profile());
    }
    for axis in 0..3 {
        geometry.orientation[axis] = col_step[axis] / geometry.spacing[1];
        geometry.orientation[axis + 3] = row_step[axis] / geometry.spacing[0];
    }
    match image_orientation_deviation(&geometry.orientation) {
        Some(d) if d <= ORIENTATION_TOLERANCE => {}
        _ => return Err(profile()),
    }
    let values = geometry
        .position
        .iter()
        .chain(&geometry.orientation)
        .chain(&geometry.spacing);
    for value in values {
        if !value.is_finite() || format_ds(*value).is_err() {
            return Err(profile());
        }
    }
    Ok(Grid {
        crop,
        rows,
        columns: cols,
        resize_rows,
        resize_columns,
        source_columns: source.columns,
        inverse,
        geometry,
        flip_horizontal: opts.flip_horizontal,
        flip_vertical: opts.flip_vertical,
        rotate_clockwise: opts.rotate_clockwise,
    })
}
fn resample_geometric(
    src: &[u8],
    reader: &StoredSampleReader,
    bits_stored: i64,
    big_endian: bool,
    grid: &Grid,
    method: Interpolation,
) -> Vec<u8> {
    let (lo, hi) = reader.range();
    let stride = grid.source_columns;
    let mut out = vec![0u8; (grid.rows * grid.columns * 2) as usize];
    let sample = |x: i64, y: i64| reader.value(&src[((y * stride + x) * 2) as usize..]);
    let (dx, dy) = (2 * grid.resize_columns, 2 * grid.resize_rows);
    let denominator = dx * dy;
    let mask = ((1u32 << bits_stored as u32) - 1) as u16;
    let crop = grid.crop;
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            let (mut x, mut y) = match grid.rotate_clockwise {
                90 => (row, grid.resize_rows - 1 - column),
                180 => (grid.resize_columns - 1 - column, grid.resize_rows - 1 - row),
                270 => (grid.resize_columns - 1 - row, column),
                _ => (column, row),
            };
            if grid.flip_vertical {
                y = grid.resize_rows - 1 - y;
            }
            if grid.flip_horizontal {
                x = grid.resize_columns - 1 - x;
            }
            let mut stored = if method == NEAREST {
                let sx = (((2 * x + 1) * crop.width) / dx).min(crop.width - 1) + crop.column;
                let sy = (((2 * y + 1) * crop.height) / dy).min(crop.height - 1) + crop.row;
                sample(sx, sy)
            } else {
                let nx = ((2 * x + 1) * crop.width - grid.resize_columns)
                    .min((crop.width - 1) * dx)
                    .max(0);
                let ny = ((2 * y + 1) * crop.height - grid.resize_rows)
                    .min((crop.height - 1) * dy)
                    .max(0);
                let (x0, y0) = (nx / dx + crop.column, ny / dy + crop.row);
                let x1 = (x0 + 1).min(crop.column + crop.width - 1);
                let y1 = (y0 + 1).min(crop.row + crop.height - 1);
                let (fx, fy) = (nx % dx, ny % dy);
                let sum = (dx - fx) * (dy - fy) * sample(x0, y0)
                    + fx * (dy - fy) * sample(x1, y0)
                    + (dx - fx) * fy * sample(x0, y1)
                    + fx * fy * sample(x1, y1);
                let mut stored = sum / denominator;
                let remainder = sum % denominator;
                if remainder * 2 >= denominator {
                    stored += 1;
                } else if -remainder * 2 >= denominator {
                    stored -= 1;
                }
                stored
            };
            stored = stored.min(hi).max(lo);
            let word = (stored as u16) & mask;
            let at = ((row * grid.columns + column) * 2) as usize;
            let bytes = if big_endian {
                word.to_be_bytes()
            } else {
                word.to_le_bytes()
            };
            out[at..at + 2].copy_from_slice(&bytes);
        }
    }
    out
}
#[derive(Clone, Debug, Default)]
struct Profile {
    rows: i64,
    columns: i64,
    class_uid: String,
    sop_uid: String,
    series_uid: String,
    study_uid: String,
    frame_uid: String,
    geometry: GeometricGeometry,
}
fn geometric_limits(mut l: GeometricLimits) -> Result<GeometricLimits, Error> {
    let d = default_geometric_limits();
    let c = &mut l.clone;
    if l.max_input_pixels < 0
        || l.max_output_pixels < 0
        || c.max_element_bytes < 0
        || c.max_pixel_data_bytes < 0
        || c.max_total_bytes < 0
        || c.max_sequence_depth < 0
        || c.max_elements < 0
        || c.max_items < 0
        || c.max_fragments < 0
    {
        return Err(Error::detail(
            ErrorKind::GeometricOptions,
            "negative limits",
        ));
    }
    fn fill(value: &mut i64, default: i64) {
        if *value == 0 {
            *value = default;
        }
    }
    fill(&mut l.max_input_pixels, d.max_input_pixels);
    fill(&mut l.max_output_pixels, d.max_output_pixels);
    fill(&mut c.max_element_bytes, d.clone.max_element_bytes);
    fill(&mut c.max_pixel_data_bytes, d.clone.max_pixel_data_bytes);
    fill(&mut c.max_total_bytes, d.clone.max_total_bytes);
    fill(&mut c.max_sequence_depth, d.clone.max_sequence_depth);
    fill(&mut c.max_elements, d.clone.max_elements);
    fill(&mut c.max_items, d.clone.max_items);
    fill(&mut c.max_fragments, d.clone.max_fragments);
    if c.max_sequence_depth > 256 {
        return Err(Error::detail(
            ErrorKind::GeometricOptions,
            "maximum sequence depth is 256",
        ));
    }
    Ok(l)
}
struct Structure<'a> {
    limits: &'a GeometricLimits,
    remaining_elements: i64,
    remaining_items: i64,
    remaining_bytes: i64,
}
impl Structure<'_> {
    fn visit(&mut self, elements: &[Element], depth: i64) -> Result<(), Error> {
        let limit = || Error::bare(ErrorKind::GeometricLimit);
        if depth > self.limits.clone.max_sequence_depth
            || elements.len() as i64 > self.remaining_elements
        {
            return Err(limit());
        }
        self.remaining_elements -= elements.len() as i64;
        for e in elements {
            let size = match &e.value {
                Value::Sequence(items) => {
                    if items.len() as i64 > self.remaining_items {
                        return Err(limit());
                    }
                    self.remaining_items -= items.len() as i64;
                    for item in items {
                        self.visit(&item.elements, depth + 1)?;
                    }
                    continue;
                }
                Value::Strings(values) => {
                    let mut size = values.len().saturating_sub(1) as i64;
                    for s in values {
                        if s.len() as i64 > self.remaining_bytes - size {
                            return Err(limit());
                        }
                        size += s.len() as i64;
                    }
                    size + size % 2
                }
                Value::Raw(raw) => {
                    let size = raw.len() as i64;
                    if size >= i64::from(UNDEFINED_LENGTH) {
                        return Err(limit());
                    }
                    size
                }
                Value::Nil => {
                    if !e.header.length_set || e.header.length == UNDEFINED_LENGTH {
                        return Err(Error::bare(ErrorKind::GeometricProfile));
                    }
                    i64::from(e.header.length)
                }
                _ => return Err(profile_err("unresolved or unsupported value kind")),
            };
            let max = if e.tag() == TAG_PIXEL_DATA {
                self.limits.clone.max_pixel_data_bytes
            } else {
                self.limits.clone.max_element_bytes
            };
            if size > max || size > self.remaining_bytes {
                return Err(limit());
            }
            self.remaining_bytes -= size;
        }
        Ok(())
    }
}
fn geometric_structure(src: &File, limits: &GeometricLimits) -> Result<(), Error> {
    let mut s = Structure {
        limits,
        remaining_elements: limits.clone.max_elements,
        remaining_items: limits.clone.max_items,
        remaining_bytes: limits.clone.max_total_bytes,
    };
    for obj in [&src.meta, &src.dataset] {
        if obj.len() as i64 > s.remaining_elements {
            return Err(Error::bare(ErrorKind::GeometricLimit));
        }
        s.visit(obj.elements(), 0)?;
    }
    Ok(())
}
fn dependent_pixel_layout(t: Tag) -> bool {
    t.group == 0x5200
        || (0x6000..=0x60FF).contains(&t.group)
        || t == Tag::new(0x7FE0, 0x0008)
        || t == Tag::new(0x7FE0, 0x0009)
        || t == Tag::new(0x7FE0, 0x0001)
        || t == Tag::new(0x7FE0, 0x0002)
        || t == Tag::new(0x0028, 0x0009)
        || t == Tag::new(0x0028, 0x7FE0)
        || t.group == 0x0028
            && ((0x1101..=0x1114).contains(&t.element)
                || (0x1201..=0x1204).contains(&t.element)
                || (0x1221..=0x1224).contains(&t.element)
                || t.element == 0x140B)
}
fn geometric_profile(src: Option<&File>, limits: &GeometricLimits) -> Result<Profile, Error> {
    let Some(src) = src else {
        return Err(Error::object(object::Error::sentinel(Kind::NilFile)));
    };
    let mut p = Profile::default();
    if !matches!(
        src.transfer_syntax.uid,
        "1.2.840.10008.1.2" | "1.2.840.10008.1.2.1" | "1.2.840.10008.1.2.2"
    ) {
        return Err(profile_err("transfer syntax"));
    }
    if src.transfer_syntax.encapsulated || src.transfer_syntax.deflated {
        return Err(Error::bare(ErrorKind::GeometricProfile));
    }
    geometric_structure(src, limits)?;
    let o = &src.dataset;
    if o.elements().iter().any(|e| dependent_pixel_layout(e.tag())) {
        return Err(profile_err("dependent pixel layout"));
    }
    let uid_tags = [
        dio::TAG_SOP_CLASS_UID,
        dio::TAG_SOP_INSTANCE_UID,
        dio::TAG_SERIES_INSTANCE_UID,
        dio::TAG_STUDY_INSTANCE_UID,
        dio::TAG_FRAME_OF_REFERENCE_UID,
    ];
    let mut uids: Vec<String> = Vec::with_capacity(uid_tags.len());
    for tag in uid_tags {
        if !geometric_primitive(o, tag, Vr::UI, 66, 1) {
            return Err(profile_err("source UID"));
        }
        match o.get_uids(tag).as_deref() {
            Some([uid]) if is_valid_uid(uid.as_bytes()) => uids.push(uid.clone()),
            _ => return Err(profile_err("source UID")),
        }
    }
    for (i, uid) in uids.iter().enumerate() {
        if uids[..i].contains(uid) {
            return Err(profile_err("source UID entity collision"));
        }
    }
    p.frame_uid = uids.pop().unwrap_or_default();
    p.study_uid = uids.pop().unwrap_or_default();
    p.series_uid = uids.pop().unwrap_or_default();
    p.sop_uid = uids.pop().unwrap_or_default();
    p.class_uid = uids.pop().unwrap_or_default();
    if p.class_uid != CT_IMAGE_STORAGE && p.class_uid != MR_IMAGE_STORAGE {
        return Err(profile_err("SOP class"));
    }
    for tag in [
        dio::TAG_ROWS,
        dio::TAG_COLUMNS,
        dio::TAG_SAMPLES_PER_PIXEL,
        dio::TAG_BITS_ALLOCATED,
        dio::TAG_BITS_STORED,
        dio::TAG_HIGH_BIT,
        dio::TAG_PIXEL_REPRESENTATION,
    ] {
        if !geometric_primitive(o, tag, Vr::US, 2, 1) {
            return Err(profile_err("pixel metadata"));
        }
    }
    if !geometric_primitive(o, dio::TAG_PHOTOMETRIC_INTERPRETATION, Vr::CS, 16, 1) {
        return Err(Error::bare(ErrorKind::GeometricProfile));
    }
    if o.has(dio::TAG_NUMBER_OF_FRAMES)
        && !geometric_primitive(o, dio::TAG_NUMBER_OF_FRAMES, Vr::IS, 12, 1)
    {
        return Err(Error::bare(ErrorKind::GeometricProfile));
    }
    let m = match extract_metadata(o) {
        Ok(m)
            if m.rows != 0
                && m.columns != 0
                && m.number_of_frames == 1
                && m.samples_per_pixel == 1
                && m.bits_allocated == 16
                && m.bits_stored != 0
                && m.bits_stored <= 16
                && m.high_bit == m.bits_stored - 1
                && m.pixel_representation <= 1
                && !m.planar_configuration_present
                && (m.photometric_interpretation == "MONOCHROME1"
                    || m.photometric_interpretation == "MONOCHROME2")
                && !(p.class_uid == CT_IMAGE_STORAGE && m.bits_stored < 12) =>
        {
            m
        }
        _ => return Err(profile_err("single-frame monochrome format")),
    };
    p.rows = i64::from(m.rows);
    p.columns = i64::from(m.columns);
    let count = p.rows * p.columns;
    if count > limits.max_input_pixels
        || count * 2 > limits.clone.max_pixel_data_bytes
        || count * 2 > limits.clone.max_total_bytes
    {
        return Err(Error::bare(ErrorKind::GeometricLimit));
    }
    match o.get(TAG_PIXEL_DATA) {
        Some(e) if element_length(e) != UNDEFINED_LENGTH => {
            if let Some(raw) = e.raw() {
                if raw.len() as i64 != count * 2 {
                    return Err(profile_err("Pixel Data length"));
                }
            } else if e.value != Value::Nil
                || !e.header.length_set
                || i64::from(element_length(e)) != count * 2
            {
                return Err(profile_err("native Pixel Data"));
            }
        }
        _ => return Err(profile_err("native Pixel Data")),
    }
    let mut position = [0.0; 3];
    let mut orientation = [0.0; 6];
    let mut spacing = [0.0; 2];
    let items: [(Tag, &mut [f64]); 3] = [
        (TAG_POSITION, &mut position),
        (TAG_ORIENTATION, &mut orientation),
        (TAG_SPACING, &mut spacing),
    ];
    for (tag, dst) in items {
        let count = dst.len();
        if !geometric_primitive(o, tag, Vr::DS, count as i64 * 17, count) {
            return Err(profile_err("Image Plane"));
        }
        match o.get_floats(tag) {
            Ok(values) if values.len() == count => {
                if values.iter().any(|v| !v.is_finite()) {
                    return Err(profile_err("finite Image Plane"));
                }
                dst.copy_from_slice(&values);
            }
            _ => return Err(profile_err("Image Plane VM")),
        }
    }
    p.geometry = GeometricGeometry {
        position,
        orientation,
        spacing,
    };
    match image_orientation_deviation(&orientation) {
        Some(d) if d <= ORIENTATION_TOLERANCE && spacing[0] > 0.0 && spacing[1] > 0.0 => {}
        _ => return Err(profile_err("Image Plane directions/spacing")),
    }
    Ok(p)
}
fn geometric_primitive(o: &Object, tag: Tag, vr: Vr, max_bytes: i64, max_vm: usize) -> bool {
    let Some(e) = o.get(tag) else {
        return false;
    };
    if e.vr() != vr {
        return false;
    }
    let n = match &e.value {
        Value::Strings(values) => {
            if values.len() > max_vm {
                return false;
            }
            let total = values.iter().map(String::len).sum::<usize>() + values.len().max(1) - 1;
            (total + total % 2) as u32
        }
        Value::Raw(raw) => raw.len() as u32,
        _ => return false,
    };
    n != UNDEFINED_LENGTH && i64::from(n) <= max_bytes
}
fn validate_geometric_file(file: &File, limits: &GeometricLimits) -> Result<iod::Report, Error> {
    let opts = ValidationOptions {
        mode: Mode::Strict,
        dictionary: true,
        big_endian: file.dataset.value_big_endian(),
        max_depth: limits.clone.max_sequence_depth,
        max_elements: limits.clone.max_elements,
        required_uids: vec![
            dio::TAG_SOP_CLASS_UID,
            dio::TAG_SOP_INSTANCE_UID,
            dio::TAG_STUDY_INSTANCE_UID,
            dio::TAG_SERIES_INSTANCE_UID,
            dio::TAG_FRAME_OF_REFERENCE_UID,
        ],
        ..Default::default()
    };
    let (r, err) = validate_file(
        &file.meta.to_data_set(),
        &file.dataset.to_data_set(),
        file.transfer_syntax.uid,
        &opts,
    );
    if err.is_some() || r.truncated || r.has_errors() {
        return Err(Error::detail(
            ErrorKind::GeometricValidation,
            "strict VR/VM or Part 10 identity",
        ));
    }
    let (i, err) = iod::validate(
        &file.dataset,
        &iod::Options {
            check_values: true,
            ..Default::default()
        },
    );
    if err.is_some() || i.truncated != 0 || !i.errors().is_empty() {
        return Err(Error::detail(ErrorKind::GeometricValidation, "CT/MR IOD"));
    }
    Ok(i)
}
fn geometric_uids(
    dataset: &Object,
    profile: &Profile,
    opts: &GeometricOptions,
    mut generator: Option<&mut UidGenerator<'_>>,
) -> Result<(String, String), Error> {
    let mut generate = || -> Result<String, Error> {
        let Some(g) = generator.as_mut() else {
            return Ok(object::inherit::new_uid());
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(g)) {
            Ok(Ok(uid)) => Ok(uid),
            Ok(Err(e)) => Err(Error {
                kind: ErrorKind::Generator,
                text: e.to_string(),
                object: None,
            }),
            Err(_) => Err(Error::detail(
                ErrorKind::GeometricOptions,
                "UID generator panic",
            )),
        }
    };
    let mut series = opts.series_instance_uid.clone();
    let mut sop = opts.sop_instance_uid.clone();
    if series.is_empty() {
        series = generate()?;
    }
    if sop.is_empty() {
        sop = generate()?;
    }
    if !is_valid_uid(series.as_bytes()) || !is_valid_uid(sop.as_bytes()) || series == sop {
        return Err(Error::detail(
            ErrorKind::GeometricOptions,
            "distinct valid output UIDs required",
        ));
    }
    let mut used = vec![
        profile.class_uid.clone(),
        profile.sop_uid.clone(),
        profile.series_uid.clone(),
        profile.study_uid.clone(),
        profile.frame_uid.clone(),
    ];
    if let Some(refs) = dataset.get_sequence(TAG_SOURCE_IMAGE_SEQUENCE) {
        used.extend(
            refs.iter()
                .filter_map(|r| r.get_uid(dio::TAG_REF_SOP_INSTANCE_UID)),
        );
    }
    if used.iter().any(|uid| *uid == series || *uid == sop) {
        return Err(Error::detail(
            ErrorKind::GeometricOptions,
            "output UID reuses a source entity",
        ));
    }
    Ok((series, sop))
}
