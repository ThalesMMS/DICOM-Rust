use super::error::{RegError, Sentinel};
use super::hdf5::{self, File, WriteDataset};
use super::itkfile::{
    FileTransform2D, MAX_DENSE_TRANSFORM_PARAMETERS, MAX_HDF5_TRANSFORMS,
    MAX_TRANSFORM_FILE_PARAMETERS, Record, assemble_3d, file_err, file_wrap, itk_type_name,
    itk_type_name_2d, new_itk_parameters, parameter_limit, set_itk_parameters,
};
use super::matrix::{Matrix3, finite, finite_vec};
use super::transform::{Kind, MatrixTransform, Transform, check_length};
use super::transform_2d::{MatrixTransform2D, Transform2D};
use super::{
    Affine, BSpline, Composite, DisplacementFieldTransform, Euler3D, ScaleVersor3D, Similarity3D,
    Translation3D, VersorRigid3D,
};
use crate::optimize::Context;
pub const DEFAULT_ITK_HDF5_MAX_BYTES: i64 = 256 << 20;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ItkHdf5WriteOptions {
    pub max_bytes: i64,
}
pub fn read_itk_hdf5(data: &[u8]) -> Result<Vec<Box<dyn Transform>>, RegError> {
    let (records, composite) = read_itk_hdf5_bounded(data, 3, MAX_DENSE_TRANSFORM_PARAMETERS)?;
    assemble_3d(records, composite)
}
pub(crate) fn read_itk_hdf5_bounded(
    data: &[u8],
    dimension: i64,
    mut remaining: i64,
) -> Result<(Vec<Record>, bool), RegError> {
    let mut file = File::open(data).map_err(|e| file_wrap(&e))?;
    const GROUP: &str = "/TransformGroup";
    let links = file.links(GROUP).map_err(|e| file_wrap(&e))?;
    if links.is_empty() {
        return Err(file_err("no transform"));
    }
    if links.len() > MAX_HDF5_TRANSFORMS {
        return Err(file_err(format!("{} transforms", links.len())));
    }
    let mut transforms = Vec::new();
    let mut composite = false;
    for i in 0..links.len() {
        let base = format!("{GROUP}/{i}");
        let in_transform =
            |e: &RegError| file_wrap(e).prefixed_after_sentinel(format!("transform {i}"));
        let type_name = file
            .read_string(&format!("{base}/TransformType"))
            .map_err(|e| in_transform(&e))?;
        let mut t =
            new_itk_parameters(type_name.trim(), dimension).map_err(|e| in_transform(&e))?;
        if t.kind() == Kind::COMPOSITE {
            if i != 0 {
                return Err(file_err(format!(
                    "transform {i}: a composite transform must be the first"
                )));
            }
            composite = true;
            continue;
        }
        let kind = t.kind();
        let in_kind =
            |e: &RegError| file_wrap(e).prefixed_after_sentinel(format!("transform {i} ({kind})"));
        let fixed = read_vector(
            &mut file,
            &base,
            "FixedParameters",
            MAX_TRANSFORM_FILE_PARAMETERS,
        )
        .map_err(|e| in_kind(&e))?;
        let parameters = read_vector(
            &mut file,
            &base,
            "Parameters",
            parameter_limit(kind).min(remaining),
        )
        .map_err(|e| in_kind(&e))?;
        set_itk_parameters(&mut t, &parameters, &fixed)?;
        remaining -= parameters.len() as i64;
        transforms.push(t);
    }
    if composite && transforms.is_empty() {
        return Err(file_err("empty composite transform"));
    }
    Ok((transforms, composite))
}
fn read_vector(
    file: &mut File<'_>,
    base: &str,
    name: &str,
    limit: i64,
) -> Result<Vec<f64>, RegError> {
    let mut path = format!("{base}/Transform{name}");
    if !file.exists(&path) {
        let misspelled = format!("{base}/Tranform{name}");
        if file.exists(&misspelled) {
            path = misspelled;
        }
    }
    match file.read_float64s(&path, limit) {
        Err(e) if e.is(Sentinel::Hdf5NotFound) => Err(RegError::with_text(
            Vec::new(),
            format!("no Transform{name}"),
        )),
        r => r,
    }
}
fn write_failure(e: RegError) -> RegError {
    let e = if e.is(Sentinel::Hdf5WriteBudget) {
        RegError::wrap(Sentinel::TransformFileBudget, &e)
    } else {
        e
    };
    file_wrap(&e)
}
fn write_budget(o: ItkHdf5WriteOptions) -> Result<i64, RegError> {
    if o.max_bytes < 0 {
        return Err(RegError::bare(Sentinel::InvalidParameters));
    }
    if o.max_bytes == 0 {
        return Ok(DEFAULT_ITK_HDF5_MAX_BYTES);
    }
    Ok(o.max_bytes)
}
fn string_dataset<'a>(path: String, value: &str) -> WriteDataset<'a> {
    WriteDataset {
        path,
        text: Some(value.to_string()),
        count: 0,
        float64_at: None,
    }
}
fn slice_dataset<'a>(path: String, p: Vec<f64>) -> WriteDataset<'a> {
    WriteDataset {
        path,
        text: None,
        count: p.len() as i64,
        float64_at: Some(Box::new(move |i| p[i])),
    }
}
fn root_datasets<'a>() -> Vec<WriteDataset<'a>> {
    vec![
        string_dataset("/HDFVersion".into(), "HDF5 superblock 0; in Rust"),
        string_dataset("/ITKVersion".into(), "dcm-rust; ITK 5.4 transform schema"),
    ]
}
fn params() -> RegError {
    RegError::bare(Sentinel::InvalidParameters)
}
fn dense_size(kind: Kind, count: u64, f: &[f64]) -> Result<(), RegError> {
    check_length(f, 18, "dense fixed parameters")?;
    let minimum = if kind == Kind::BSPLINE { 4.0 } else { 1.0 };
    let mut want = 3u64;
    for &size in &f[..3] {
        if size < minimum
            || size > (1u64 << 20) as f64
            || size != size.trunc()
            || want > MAX_DENSE_TRANSFORM_PARAMETERS as u64 / size as u64
        {
            return Err(RegError::new(
                Sentinel::InvalidParameters,
                "dense geometry/parameter count",
            ));
        }
        want *= size as u64;
    }
    if count != want || count > MAX_DENSE_TRANSFORM_PARAMETERS as u64 {
        return Err(RegError::new(
            Sentinel::InvalidParameters,
            format!("dense shape requires{want} parameters, got{count}"),
        ));
    }
    for &spacing in &f[6..9] {
        if spacing <= 0.0 {
            return Err(params());
        }
    }
    let m = Matrix3([
        [f[9], f[10], f[11]],
        [f[12], f[13], f[14]],
        [f[15], f[16], f[17]],
    ]);
    m.inverse()?;
    Ok(())
}
const CANONICAL_TOLERANCE: f64 = 32.0 * 2.220446049250313e-16;
fn equivalent(actual: f64, expected: f64) -> bool {
    finite(actual)
        && finite(expected)
        && (actual - expected).abs()
            <= CANONICAL_TOLERANCE * 1f64.max(actual.abs().max(expected.abs()))
}
fn noncanonical() -> RegError {
    RegError::new(
        Sentinel::InvalidParameters,
        "linear state changes geometry when native parameters are reread; use a canonical constructor/setter state or affine representation",
    )
}
fn canonical_3d(t: &dyn MatrixTransform, p: &[f64], f: &[f64]) -> Result<(), RegError> {
    let kind = t.kind();
    let mut canonical = new_itk_parameters(&format!("{kind}_double_3_3"), 3)?;
    set_itk_parameters(&mut canonical, p, f)?;
    let Record::D3(expected) = &canonical else {
        return Err(noncanonical());
    };
    let Some(expected) = expected.as_matrix_transform() else {
        return Err(noncanonical());
    };
    let (a, b) = (t.matrix(), expected.matrix());
    let mut valid = true;
    for i in 0..3 {
        for j in 0..3 {
            valid = valid && equivalent(a.0[i][j], b.0[i][j]);
        }
    }
    let (x, y) = (t.offset(), expected.offset());
    valid = valid && equivalent(x.x, y.x) && equivalent(x.y, y.y) && equivalent(x.z, y.z);
    if !valid {
        return Err(noncanonical());
    }
    Ok(())
}
fn builtin_3d(t: &dyn Transform) -> (bool, bool) {
    let any = t.as_any();
    if any.is::<BSpline>() || any.is::<DisplacementFieldTransform>() {
        return (true, true);
    }
    let linear = any.is::<Translation3D>()
        || any.is::<ScaleVersor3D>()
        || any.is::<Euler3D>()
        || any.is::<VersorRigid3D>()
        || any.is::<Similarity3D>()
        || any.is::<Affine>();
    (linear, false)
}
fn record_3d<'a>(base: &str, t: Option<&dyn Transform>) -> Result<Vec<WriteDataset<'a>>, RegError> {
    let Some(t) = t else {
        return Err(RegError::new(
            Sentinel::UnsupportedTransform,
            "HDF5 requires a built-in 3D transform",
        ));
    };
    let (valid, dense) = builtin_3d(t);
    if !valid {
        return Err(RegError::new(
            Sentinel::UnsupportedTransform,
            "HDF5 requires a built-in 3D transform",
        ));
    }
    let name = itk_type_name(t.kind())?;
    let fixed = t.fixed_parameters();
    let mut ds = vec![
        string_dataset(format!("{base}/TransformType"), &name),
        slice_dataset(format!("{base}/TransformFixedParameters"), fixed.clone()),
    ];
    if dense {
        if let Some(d) = t.as_any().downcast_ref::<DisplacementFieldTransform>()
            && d.field().vectors.len() as u64 > MAX_DENSE_TRANSFORM_PARAMETERS as u64 / 3
        {
            return Err(params());
        }
        let p = t.parameters();
        dense_size(t.kind(), p.len() as u64, &fixed)?;
        ds.push(slice_dataset(format!("{base}/TransformParameters"), p));
    } else {
        let Some(m) = t.as_matrix_transform() else {
            return Err(params());
        };
        if !m.matrix().finite() || !finite_vec(m.offset()) {
            return Err(RegError::bare(Sentinel::InvalidMatrix));
        }
        let p = t.parameters();
        if p.len() != t.number_of_parameters() || p.len() > 64 || fixed.len() > 64 {
            return Err(params());
        }
        canonical_3d(m, &p, &fixed)?;
        ds.push(slice_dataset(format!("{base}/TransformParameters"), p));
    }
    Ok(ds)
}
pub fn write_itk_hdf5(t: Option<&dyn Transform>) -> Result<Vec<u8>, RegError> {
    let mut out = Vec::new();
    write_itk_hdf5_context(
        Context::background(),
        Some(&mut out),
        t,
        ItkHdf5WriteOptions::default(),
    )?;
    Ok(out)
}
pub fn write_itk_hdf5_context(
    ctx: Context<'_>,
    w: Option<&mut Vec<u8>>,
    t: Option<&dyn Transform>,
    o: ItkHdf5WriteOptions,
) -> Result<(), RegError> {
    if ctx.err().is_err() {
        return Err(write_failure(RegError::canceled()));
    }
    let max_bytes = write_budget(o).map_err(write_failure)?;
    let Some(w) = w else {
        return Err(write_failure(params()));
    };
    let mut ds = root_datasets();
    if let Some(c) = t.and_then(|t| t.as_any().downcast_ref::<Composite>()) {
        if c.is_empty() || c.len() + 1 > MAX_HDF5_TRANSFORMS {
            return Err(write_failure(RegError::bare(
                Sentinel::UnsupportedTransform,
            )));
        }
        ds.push(string_dataset(
            "/TransformGroup/0/TransformType".into(),
            "CompositeTransform_double_3_3",
        ));
        for i in (0..c.len()).rev() {
            if ctx.err().is_err() {
                return Err(write_failure(RegError::canceled()));
            }
            let rs = record_3d(
                &format!("/TransformGroup/{}", c.len() - i),
                Some(c.transform(i)),
            )
            .map_err(write_failure)?;
            ds.extend(rs);
        }
    } else {
        ds.extend(record_3d("/TransformGroup/0", t).map_err(write_failure)?);
    }
    hdf5::write(ctx, Some(w), &ds, max_bytes).map_err(write_failure)
}
fn record_2d<'a>(base: &str, t: &FileTransform2D) -> Result<Vec<WriteDataset<'a>>, RegError> {
    let FileTransform2D::Linear(l) = t else {
        return Err(RegError::bare(Sentinel::UnsupportedTransform));
    };
    if !l.matrix().finite() || !finite(l.offset()[0]) || !finite(l.offset()[1]) {
        return Err(RegError::bare(Sentinel::InvalidMatrix));
    }
    let name = itk_type_name_2d(t)?;
    let (p, f) = (l.parameters(), l.fixed_parameters());
    if p.len() != l.number_of_parameters() || p.len() > 64 || f.len() > 64 {
        return Err(params());
    }
    let kind = l.kind();
    let mut canonical = new_itk_parameters(&format!("{kind}_double_2_2"), 2)?;
    set_itk_parameters(&mut canonical, &p, &f)?;
    let Record::D2(FileTransform2D::Linear(e)) = &canonical else {
        return Err(noncanonical());
    };
    let (a, b) = (l.matrix(), e.matrix());
    let mut valid = true;
    for i in 0..2 {
        for j in 0..2 {
            valid = valid && equivalent(a.0[i][j], b.0[i][j]);
        }
    }
    let (x, y) = (l.offset(), e.offset());
    valid = valid && equivalent(x[0], y[0]) && equivalent(x[1], y[1]);
    if !valid {
        return Err(noncanonical());
    }
    Ok(vec![
        string_dataset(format!("{base}/TransformType"), &name),
        slice_dataset(format!("{base}/TransformFixedParameters"), f),
        slice_dataset(format!("{base}/TransformParameters"), p),
    ])
}
pub fn write_itk_hdf5_2d(t: Option<&FileTransform2D>) -> Result<Vec<u8>, RegError> {
    let mut out = Vec::new();
    write_itk_hdf5_2d_context(
        Context::background(),
        Some(&mut out),
        t,
        ItkHdf5WriteOptions::default(),
    )?;
    Ok(out)
}
pub fn write_itk_hdf5_2d_context(
    ctx: Context<'_>,
    w: Option<&mut Vec<u8>>,
    t: Option<&FileTransform2D>,
    o: ItkHdf5WriteOptions,
) -> Result<(), RegError> {
    if ctx.err().is_err() {
        return Err(write_failure(RegError::canceled()));
    }
    let max_bytes = write_budget(o).map_err(write_failure)?;
    let (Some(w), Some(t)) = (w, t) else {
        return Err(write_failure(RegError::bare(
            Sentinel::UnsupportedTransform,
        )));
    };
    let mut ds = root_datasets();
    if let FileTransform2D::Composite(c) = t {
        if c.is_empty() || c.len() + 1 > MAX_HDF5_TRANSFORMS {
            return Err(write_failure(RegError::bare(
                Sentinel::UnsupportedTransform,
            )));
        }
        ds.push(string_dataset(
            "/TransformGroup/0/TransformType".into(),
            "CompositeTransform_double_2_2",
        ));
        for i in (0..c.len()).rev() {
            let rs = record_2d(
                &format!("/TransformGroup/{}", c.len() - i),
                &FileTransform2D::Linear(*c.transform(i)),
            )
            .map_err(write_failure)?;
            ds.extend(rs);
        }
    } else {
        ds.extend(record_2d("/TransformGroup/0", t).map_err(write_failure)?);
    }
    hdf5::write(ctx, Some(w), &ds, max_bytes).map_err(write_failure)
}
