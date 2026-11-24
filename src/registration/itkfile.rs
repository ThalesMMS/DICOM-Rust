use super::error::{RegError, Sentinel};
use super::matrix::{Matrix3, finite_slice};
use super::transform::{Error, Kind, Transform, check_length};
use super::transform_2d::{Composite2D, Linear2D, Transform2D};
use super::{
    Affine, BSpline, Composite, DisplacementField, DisplacementFieldTransform, Euler3D,
    ScaleVersor3D, Similarity3D, Translation3D, VersorRigid3D,
};
use crate::numtext::{format_float, parse_float};
use crate::optimize::Context;
use crate::render::Vec3;
use crate::textfmt::quote;
pub const ITK_TEXT_HEADER: &str = "#Insight Transform File V1.0";
const UTF8_BOM: &str = "\u{FEFF}";
pub(crate) const MAX_TRANSFORM_FILE_PARAMETERS: i64 = 64;
pub(crate) const MAX_DENSE_TRANSFORM_PARAMETERS: i64 = 1 << 24;
const MAX_TEXT_LINE: usize = 1 << 29;
pub(crate) const MAX_HDF5_TRANSFORMS: usize = 1024;
const MATLAB_BIG_ENDIAN: i32 = 1000;
const MATLAB_SINGLE: i32 = 10;
pub(crate) fn file_err(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::TransformFile, detail)
}
pub(crate) fn file_wrap(e: &RegError) -> RegError {
    RegError::wrap(Sentinel::TransformFile, e)
}
fn unsupported(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::UnsupportedTransform, detail)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransformFileReadOptions {
    pub max_parameters: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum FileTransform2D {
    Linear(Linear2D),
    Composite(Composite2D),
}
impl FileTransform2D {
    pub fn transform(&self) -> &dyn Transform2D {
        match self {
            FileTransform2D::Linear(t) => t,
            FileTransform2D::Composite(t) => t,
        }
    }
    fn transform_mut(&mut self) -> &mut dyn Transform2D {
        match self {
            FileTransform2D::Linear(t) => t,
            FileTransform2D::Composite(t) => t,
        }
    }
}
pub(crate) enum Record {
    D3(Box<dyn Transform>),
    D2(FileTransform2D),
}
impl Record {
    pub fn kind(&self) -> Kind {
        match self {
            Record::D3(t) => t.kind(),
            Record::D2(t) => t.transform().kind(),
        }
    }
    pub fn parameters(&self) -> Vec<f64> {
        match self {
            Record::D3(t) => t.parameters(),
            Record::D2(t) => t.transform().parameters(),
        }
    }
    fn set_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        match self {
            Record::D3(t) => t.set_parameters(p),
            Record::D2(t) => t.transform_mut().set_parameters(p),
        }
    }
    fn set_fixed_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        match self {
            Record::D3(t) => t.set_fixed_parameters(p),
            Record::D2(t) => t.transform_mut().set_fixed_parameters(p),
        }
    }
}
pub(crate) fn parameter_limit(kind: Kind) -> i64 {
    if kind == Kind::BSPLINE || kind == Kind::DISPLACEMENT_FIELD {
        MAX_DENSE_TRANSFORM_PARAMETERS
    } else {
        MAX_TRANSFORM_FILE_PARAMETERS
    }
}
pub fn read_transform_file(data: &[u8]) -> Result<Box<dyn Transform>, RegError> {
    read_transform_file_context(
        Context::background(),
        data,
        TransformFileReadOptions::default(),
    )
}
fn has_hash_prefix(data: &[u8]) -> bool {
    let bom = UTF8_BOM.as_bytes();
    let mut i = 0;
    while i < data.len() {
        if matches!(data[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        } else if data[i..].starts_with(bom) {
            i += bom.len();
        } else {
            break;
        }
    }
    data[i..].starts_with(b"#")
}
pub fn read_transform_file_context(
    ctx: Context<'_>,
    data: &[u8],
    options: TransformFileReadOptions,
) -> Result<Box<dyn Transform>, RegError> {
    let canceled = || ctx.err().is_err();
    if canceled() {
        return Err(RegError::canceled());
    }
    let mut limit = options.max_parameters;
    if limit == 0 {
        limit = MAX_DENSE_TRANSFORM_PARAMETERS;
    }
    if !(1..=MAX_DENSE_TRANSFORM_PARAMETERS).contains(&limit) {
        return Err(file_err("invalid total parameter budget"));
    }
    if let Some(body) = super::mincxfm::minc_transform_body(data) {
        if data.len() > super::mincxfm::MAX_MINC_FILE_BYTES {
            return Err(file_err(format!(
                "MINC XFM exceeds {} encoded bytes",
                super::mincxfm::MAX_MINC_FILE_BYTES
            )));
        }
        let t = super::mincxfm::read_minc_transform(body, limit);
        if canceled() {
            return Err(RegError::canceled());
        }
        return t;
    }
    let r: Result<(Vec<Record>, bool), RegError> = if super::hdf5::is_hdf5(data) {
        super::itkhdf5::read_itk_hdf5_bounded(data, 3, limit)
    } else if has_hash_prefix(data) {
        read_itk_text_bounded(data, 3, limit)
    } else {
        match read_itk_matlab_records(data, 3) {
            Ok(records) => {
                let mut err = None;
                for t in &records {
                    limit -= t.parameters().len() as i64;
                    if limit < 0 {
                        err = Some(file_err("total parameter budget"));
                        break;
                    }
                }
                match err {
                    Some(e) => Err(e),
                    None => Ok((records, false)),
                }
            }
            Err(e) => Err(e),
        }
    };
    if canceled() {
        return Err(RegError::canceled());
    }
    let (records, composite) = r?;
    let mut transforms = assemble_3d(records, composite)?;
    if transforms.len() != 1 {
        return Err(file_err(format!(
            "{} transforms without a composite; the file must hold one transform",
            transforms.len()
        )));
    }
    Ok(transforms.remove(0))
}
pub fn read_itk_text(data: &[u8]) -> Result<Vec<Box<dyn Transform>>, RegError> {
    let (records, composite) = read_itk_text_bounded(data, 3, MAX_DENSE_TRANSFORM_PARAMETERS)?;
    assemble_3d(records, composite)
}
fn scan_lines(data: &[u8], limit: usize) -> Vec<Result<Vec<u8>, ()>> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < data.len() {
        match data[start..].iter().position(|&b| b == b'\n') {
            Some(n) => {
                if n + 1 > limit {
                    out.push(Err(()));
                    return out;
                }
                let mut line = &data[start..start + n];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                out.push(Ok(line.to_vec()));
                start += n + 1;
            }
            None => {
                let n = data.len() - start;
                if n >= limit {
                    out.push(Err(()));
                    return out;
                }
                let mut line = &data[start..];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                out.push(Ok(line.to_vec()));
                start = data.len();
            }
        }
    }
    out
}
#[derive(Default)]
struct TextState {
    transforms: Vec<Record>,
    current: Option<Record>,
    parameters: Vec<f64>,
    fixed: Vec<f64>,
    have_parameters: bool,
    have_fixed: bool,
}
impl TextState {
    fn finish(&mut self) -> Result<(), RegError> {
        let Some(mut t) = self.current.take() else {
            return Ok(());
        };
        if t.kind() == Kind::COMPOSITE {
            return Ok(());
        }
        if !self.have_parameters || !self.have_fixed {
            return Err(file_err(format!(
                "{} needs Parameters and FixedParameters",
                t.kind()
            )));
        }
        set_itk_parameters(&mut t, &self.parameters, &self.fixed)?;
        self.transforms.push(t);
        self.parameters = Vec::new();
        self.fixed = Vec::new();
        self.have_parameters = false;
        self.have_fixed = false;
        Ok(())
    }
}
pub(crate) fn read_itk_text_bounded(
    data: &[u8],
    dimension: i64,
    mut remaining: i64,
) -> Result<(Vec<Record>, bool), RegError> {
    let line_limit = if dimension == 2 {
        64 * 1024
    } else {
        MAX_TEXT_LINE
    };
    let mut st = TextState::default();
    let mut composite = false;
    let mut line_number = 0i64;
    let mut records_read = 0usize;
    let mut saw_header = false;
    let not_itk = || file_err("not an ITK text transform file");
    for scanned in scan_lines(data, line_limit) {
        let Ok(raw) = scanned else {
            return Err(file_err("line too long"));
        };
        line_number += 1;
        let text = String::from_utf8_lossy(&raw).into_owned();
        let text = text.strip_prefix(UTF8_BOM).unwrap_or(&text);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if line_number == 1 || !saw_header {
                if !line.starts_with("#Insight Transform File") {
                    return Err(not_itk());
                }
                if line != ITK_TEXT_HEADER {
                    return Err(file_err(format!("unsupported version {}", quote(line))));
                }
                saw_header = true;
            }
            continue;
        }
        if !saw_header {
            return Err(not_itk());
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(file_err(format!(
                "line {line_number}: tags must be delimited by ':'"
            )));
        };
        let (name, value) = (name.trim(), value.trim());
        match name {
            "Transform" => {
                if records_read >= MAX_HDF5_TRANSFORMS {
                    return Err(file_err("transform count"));
                }
                records_read += 1;
                st.finish()?;
                let t = new_itk_parameters(value, dimension).map_err(|e| {
                    file_wrap(&e).prefixed_after_sentinel(format!("line {line_number}"))
                })?;
                if t.kind() == Kind::COMPOSITE {
                    if !st.transforms.is_empty() || composite {
                        return Err(file_err(format!(
                            "line {line_number}: a composite transform must open the file"
                        )));
                    }
                    composite = true;
                }
                st.current = Some(t);
            }
            "Parameters" | "FixedParameters" => {
                let Some(t) = &st.current else {
                    return Err(file_err(format!(
                        "line {line_number}: {name} before a Transform"
                    )));
                };
                if t.kind() == Kind::COMPOSITE {
                    return Err(file_err(format!(
                        "line {line_number}: a composite transform has no parameters of its own"
                    )));
                }
                let mut limit = MAX_TRANSFORM_FILE_PARAMETERS;
                if name == "Parameters" {
                    limit = parameter_limit(t.kind()).min(remaining);
                }
                let values = parse_itk_vector(value, limit)
                    .map_err(|e| file_err(format!("line {line_number}: {e}")))?;
                if name == "Parameters" {
                    remaining -= values.len() as i64;
                    st.parameters = values;
                    st.have_parameters = true;
                } else {
                    st.fixed = values;
                    st.have_fixed = true;
                }
            }
            "ComponentTransformFile" => {
                return Err(file_err(format!(
                    "line {line_number}: components in other files are not read"
                )));
            }
            _ => {
                return Err(file_err(format!(
                    "line {line_number}: unknown tag {}",
                    quote(name)
                )));
            }
        }
    }
    if !saw_header {
        return Err(not_itk());
    }
    st.finish()?;
    let transforms = st.transforms;
    if transforms.is_empty() {
        if composite {
            return Err(file_err("empty composite transform"));
        }
        return Err(file_err("no transform"));
    }
    if dimension == 2 && transforms.len() + usize::from(composite) > MAX_HDF5_TRANSFORMS {
        return Err(file_err("2D transform count"));
    }
    Ok((transforms, composite))
}
impl RegError {
    pub(crate) fn prefixed_after_sentinel(self, prefix: String) -> RegError {
        let head = Sentinel::TransformFile.text();
        let text = self.to_string();
        let rest = text
            .strip_prefix(head)
            .and_then(|r| r.strip_prefix(": "))
            .unwrap_or(&text);
        RegError::with_text(
            self.sentinels().to_vec(),
            format!("{head}: {prefix}: {rest}"),
        )
    }
}
fn parse_itk_vector(value: &str, limit: i64) -> Result<Vec<f64>, String> {
    let mut values = Vec::new();
    for field in value.split_whitespace() {
        if values.len() as i64 >= limit {
            return Err(format!("more than {limit} values"));
        }
        match parse_float(field) {
            Ok(v) if v.is_finite() => values.push(v),
            _ => return Err(format!("value {} is not a finite number", quote(field))),
        }
    }
    Ok(values)
}
pub(crate) fn new_itk_transform(type_name: &str) -> Result<Box<dyn Transform>, RegError> {
    let parts: Vec<&str> = type_name.split('_').collect();
    if parts.len() != 4 || (parts[1] != "double" && parts[1] != "float") {
        return Err(unsupported(format!("transform type {}", quote(type_name))));
    }
    if parts[2] != "3" || parts[3] != "3" {
        return Err(unsupported(format!("{type_name} is not three-dimensional")));
    }
    let t: Box<dyn Transform> = match parts[0] {
        "TranslationTransform" => Box::new(Translation3D::new()),
        "ScaleVersor3DTransform" => Box::new(ScaleVersor3D::new()),
        "ScaleSkewVersor3DTransform" => Box::new(ScaleVersor3D::new_skew()),
        "Euler3DTransform" => Box::new(Euler3D::new()),
        "VersorRigid3DTransform" => Box::new(VersorRigid3D::new()),
        "Similarity3DTransform" => Box::new(Similarity3D::new()),
        "AffineTransform" | "MatrixOffsetTransformBase" => Box::new(Affine::new()),
        "CompositeTransform" => Box::new(Composite::new(Vec::new())),
        "BSplineTransform" => Box::new(
            BSpline::new(
                [4, 4, 4],
                Vec3::ZERO,
                Vec3::new(1.0, 1.0, 1.0),
                Matrix3::identity(),
            )
            .map_err(RegError::from)?,
        ),
        "DisplacementFieldTransform" => Box::new(
            DisplacementFieldTransform::new(Some(&DisplacementField {
                size: [1, 1, 1],
                origin: Vec3::ZERO,
                spacing: Vec3::new(1.0, 1.0, 1.0),
                direction: Matrix3::identity(),
                vectors: vec![Vec3::ZERO],
            }))
            .map_err(RegError::from)?,
        ),
        other => {
            return Err(unsupported(format!(
                "{other} (only supported built-in three-dimensional transforms are read)"
            )));
        }
    };
    Ok(t)
}
pub(crate) fn new_itk_transform_2d(name: &str) -> Result<FileTransform2D, RegError> {
    let p: Vec<&str> = name.split('_').collect();
    if p.len() != 4 || (p[1] != "double" && p[1] != "float") || p[2] != "2" || p[3] != "2" {
        return Err(unsupported(format!("{name} is not a supported 2D type")));
    }
    let t = match p[0] {
        "TranslationTransform" => FileTransform2D::Linear(Linear2D::translation_2d()),
        "Euler2DTransform" => FileTransform2D::Linear(Linear2D::euler_2d()),
        "Similarity2DTransform" => FileTransform2D::Linear(Linear2D::similarity_2d()),
        "AffineTransform" | "MatrixOffsetTransformBase" => {
            FileTransform2D::Linear(Linear2D::affine_2d())
        }
        "CompositeTransform" => {
            FileTransform2D::Composite(Composite2D::new(&[]).map_err(RegError::from)?)
        }
        _ => return Err(unsupported(name)),
    };
    Ok(t)
}
pub(crate) fn new_itk_parameters(name: &str, dimension: i64) -> Result<Record, RegError> {
    if dimension == 2 {
        return new_itk_transform_2d(name).map(Record::D2);
    }
    new_itk_transform(name).map(Record::D3)
}
pub(crate) fn itk_type_name(kind: Kind) -> Result<String, RegError> {
    match kind {
        Kind::TRANSLATION
        | Kind::SCALE_VERSOR_3D
        | Kind::SCALE_SKEW_VERSOR_3D
        | Kind::EULER_3D
        | Kind::VERSOR_RIGID_3D
        | Kind::SIMILARITY_3D
        | Kind::AFFINE
        | Kind::COMPOSITE
        | Kind::BSPLINE
        | Kind::DISPLACEMENT_FIELD => Ok(format!("{kind}_double_3_3")),
        _ => Err(unsupported(kind.to_string())),
    }
}
fn format_itk_vector(values: &[f64]) -> String {
    let fields: Vec<String> = values.iter().map(|&v| format_float(v, 'g', -1)).collect();
    fields.join(" ")
}
pub fn write_itk_text(t: Option<&dyn Transform>) -> Result<String, RegError> {
    let Some(t) = t else {
        return Err(file_err("no transform"));
    };
    let mut b = format!("{ITK_TEXT_HEADER}\n");
    let mut components: Vec<&dyn Transform> = vec![t];
    if let Some(c) = t.as_any().downcast_ref::<Composite>() {
        if c.is_empty() {
            return Err(file_err("empty composite transform"));
        }
        for i in (0..c.len()).rev() {
            if c.transform(i).kind() == Kind::COMPOSITE {
                return Err(file_err("nested composite transform"));
            }
            components.push(c.transform(i));
        }
    }
    for (i, component) in components.iter().enumerate() {
        let name = itk_type_name(component.kind()).map_err(|e| file_wrap(&e))?;
        b.push_str(&format!("#Transform {i}\nTransform: {name}\n"));
        if component.kind() == Kind::COMPOSITE {
            continue;
        }
        b.push_str(&format!(
            "Parameters: {}\nFixedParameters: {}\n",
            format_itk_vector(&component.parameters()),
            format_itk_vector(&component.fixed_parameters())
        ));
    }
    Ok(b)
}
pub fn read_itk_matlab(data: &[u8]) -> Result<Vec<Box<dyn Transform>>, RegError> {
    let records = read_itk_matlab_records(data, 3)?;
    assemble_3d(records, false)
}
pub(crate) fn read_itk_matlab_records(
    data: &[u8],
    dimension: i64,
) -> Result<Vec<Record>, RegError> {
    let mut r = data;
    let mut transforms = Vec::new();
    while let Some((name, parameters)) = read_matlab_vector(&mut r)? {
        let Some((_, fixed)) = read_matlab_vector(&mut r)? else {
            return Err(file_err(format!("{name} has no fixed parameters")));
        };
        let mut t = new_itk_parameters(&name, dimension).map_err(|e| file_wrap(&e))?;
        if t.kind() == Kind::COMPOSITE {
            return Err(file_err(
                "composite transforms are not read from MATLAB files",
            ));
        }
        set_itk_parameters(&mut t, &parameters, &fixed)?;
        if transforms.len() >= MAX_HDF5_TRANSFORMS {
            return Err(file_err("transform count"));
        }
        transforms.push(t);
    }
    if transforms.is_empty() {
        return Err(file_err("no transform"));
    }
    Ok(transforms)
}
fn read_full<'d>(r: &mut &'d [u8], n: usize) -> Option<&'d [u8]> {
    if r.len() < n {
        *r = &r[r.len()..];
        return None;
    }
    let (head, rest) = r.split_at(n);
    *r = rest;
    Some(head)
}
fn read_matlab_vector(r: &mut &[u8]) -> Result<Option<(String, Vec<f64>)>, RegError> {
    if r.is_empty() {
        return Ok(None);
    }
    let Some(header) = read_full(r, 20) else {
        return Err(file_err("truncated MATLAB header"));
    };
    let field = |at: usize, big: bool| {
        let b = [header[at], header[at + 1], header[at + 2], header[at + 3]];
        if big {
            i32::from_be_bytes(b)
        } else {
            i32::from_le_bytes(b)
        }
    };
    let mut big = false;
    let mut kind = field(0, false);
    if !(0..MATLAB_BIG_ENDIAN).contains(&kind) {
        big = true;
        kind = field(0, true);
        if !(MATLAB_BIG_ENDIAN..2 * MATLAB_BIG_ENDIAN).contains(&kind) {
            return Err(file_err(format!("not a MATLAB Level 4 file (type {kind})")));
        }
    }
    let precision = kind % 100;
    if kind % 1000 >= 100 || (precision != 0 && precision != MATLAB_SINGLE) {
        return Err(file_err(format!(
            "MATLAB type {kind} is not a double or single numeric matrix"
        )));
    }
    let rows = field(4, big);
    let cols = field(8, big);
    let imaginary = field(12, big);
    let name_length = field(16, big);
    if cols != 1 || rows < 0 || i64::from(rows) > MAX_TRANSFORM_FILE_PARAMETERS {
        return Err(file_err(format!(
            "a {rows} × {cols} matrix, only parameter vectors are read"
        )));
    }
    if imaginary != 0 {
        return Err(file_err("complex parameters"));
    }
    if !(1..=256).contains(&name_length) {
        return Err(file_err(format!("variable name of {name_length} bytes")));
    }
    let Some(name_bytes) = read_full(r, name_length as usize) else {
        return Err(file_err("truncated MATLAB variable name"));
    };
    let mut end = name_bytes.len();
    while end > 0 && name_bytes[end - 1] == 0 {
        end -= 1;
    }
    let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
    let size = if precision == MATLAB_SINGLE { 4 } else { 8 };
    let Some(data) = read_full(r, rows as usize * size) else {
        return Err(file_err(format!("truncated MATLAB data for {name}")));
    };
    let mut values = Vec::with_capacity(rows as usize);
    for i in 0..rows as usize {
        let v = if size == 8 {
            let b: [u8; 8] = data[8 * i..8 * i + 8].try_into().unwrap_or_default();
            f64::from_bits(if big {
                u64::from_be_bytes(b)
            } else {
                u64::from_le_bytes(b)
            })
        } else {
            let b: [u8; 4] = data[4 * i..4 * i + 4].try_into().unwrap_or_default();
            f64::from(f32::from_bits(if big {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            }))
        };
        if !v.is_finite() {
            return Err(file_err(format!("non-finite value in {name}")));
        }
        values.push(v);
    }
    Ok(Some((name, values)))
}
fn matlab_vector(b: &mut Vec<u8>, name: &str, values: &[f64]) {
    for v in [0i32, values.len() as i32, 1, 0, name.len() as i32 + 1] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(name.as_bytes());
    b.push(0);
    for v in values {
        b.extend_from_slice(&v.to_le_bytes());
    }
}
pub fn write_itk_matlab(t: Option<&dyn Transform>) -> Result<Vec<u8>, RegError> {
    let Some(t) = t else {
        return Err(file_err("no transform"));
    };
    if t.kind() == Kind::COMPOSITE {
        return Err(file_err(
            "composite transforms are not written to MATLAB files",
        ));
    }
    if t.kind() == Kind::BSPLINE || t.kind() == Kind::DISPLACEMENT_FIELD {
        return Err(file_err(format!(
            "{} transforms are written to text files only",
            t.kind()
        )));
    }
    let name = itk_type_name(t.kind()).map_err(|e| file_wrap(&e))?;
    let mut b = Vec::new();
    matlab_vector(&mut b, &name, &t.parameters());
    matlab_vector(&mut b, "fixed", &t.fixed_parameters());
    Ok(b)
}
pub(crate) fn assemble_3d(
    records: Vec<Record>,
    composite: bool,
) -> Result<Vec<Box<dyn Transform>>, RegError> {
    let mut ts: Vec<Box<dyn Transform>> = Vec::with_capacity(records.len());
    for r in records {
        match r {
            Record::D3(t) => ts.push(t),
            Record::D2(_) => return Err(unsupported("two-dimensional record")),
        }
    }
    if !composite {
        return Ok(ts);
    }
    ts.reverse();
    Ok(vec![Box::new(Composite::new(ts))])
}
pub(crate) fn assemble_2d(
    records: Vec<Record>,
    composite: bool,
) -> Result<Vec<FileTransform2D>, RegError> {
    let mut ts = Vec::with_capacity(records.len());
    for r in records {
        match r {
            Record::D2(t) => ts.push(t),
            Record::D3(_) => return Err(unsupported("three-dimensional record")),
        }
    }
    if !composite {
        return Ok(ts);
    }
    ts.reverse();
    let mut linear = Vec::with_capacity(ts.len());
    for t in ts {
        match t {
            FileTransform2D::Linear(l) => linear.push(l),
            FileTransform2D::Composite(_) => {
                return Err(file_wrap(&unsupported("nested 2D composite")));
            }
        }
    }
    let c = Composite2D::new(&linear).map_err(|e| file_wrap(&RegError::from(e)))?;
    Ok(vec![FileTransform2D::Composite(c)])
}
pub(crate) fn set_itk_parameters(t: &mut Record, p: &[f64], f: &[f64]) -> Result<(), RegError> {
    let kind = t.kind();
    if kind == Kind::BSPLINE || kind == Kind::DISPLACEMENT_FIELD {
        check_length(f, 18, "dense fixed parameters").map_err(|e| file_wrap(&RegError::from(e)))?;
        let minimum = if kind == Kind::BSPLINE { 4.0 } else { 1.0 };
        let mut count = 3u64;
        for &size in &f[..3] {
            if size < minimum
                || size > (1u64 << 20) as f64
                || size != size.trunc()
                || count > MAX_DENSE_TRANSFORM_PARAMETERS as u64 / size as u64
            {
                return Err(file_err("dense fixed geometry exceeds parameter limit"));
            }
            count *= size as u64;
        }
        if count != p.len() as u64 {
            return Err(file_err(format!(
                "dense geometry requires {count} parameters, got{}",
                p.len()
            )));
        }
    }
    t.set_fixed_parameters(f).map_err(|e| {
        file_wrap(&RegError::from(e)).prefixed_after_sentinel(format!("{kind} fixed parameters"))
    })?;
    t.set_parameters(p).map_err(|e| {
        file_wrap(&RegError::from(e)).prefixed_after_sentinel(format!("{kind} parameters"))
    })?;
    Ok(())
}
pub(crate) fn itk_type_name_2d(t: &FileTransform2D) -> Result<String, RegError> {
    let kind = t.transform().kind();
    match kind {
        Kind::TRANSLATION
        | Kind::EULER_2D
        | Kind::SIMILARITY_2D
        | Kind::AFFINE
        | Kind::COMPOSITE => Ok(format!("{kind}_double_2_2")),
        _ => Err(RegError::bare(Sentinel::UnsupportedTransform)),
    }
}
pub fn read_transform_file_2d(data: &[u8]) -> Result<FileTransform2D, RegError> {
    let mut ts = if super::hdf5::is_hdf5(data) {
        read_itk_hdf5_2d(data)?
    } else if has_hash_prefix(data) {
        read_itk_text_2d(data)?
    } else {
        read_itk_matlab_2d(data)?
    };
    if ts.len() != 1 {
        return Err(file_err("file must hold one 2D transform or composite"));
    }
    Ok(ts.remove(0))
}
pub fn read_itk_text_2d(data: &[u8]) -> Result<Vec<FileTransform2D>, RegError> {
    let (rs, c) = read_itk_text_bounded(data, 2, MAX_DENSE_TRANSFORM_PARAMETERS)?;
    assemble_2d(rs, c)
}
pub fn read_itk_matlab_2d(data: &[u8]) -> Result<Vec<FileTransform2D>, RegError> {
    let rs = read_itk_matlab_records(data, 2)?;
    assemble_2d(rs, false)
}
pub fn read_itk_hdf5_2d(data: &[u8]) -> Result<Vec<FileTransform2D>, RegError> {
    let (rs, c) = super::itkhdf5::read_itk_hdf5_bounded(data, 2, MAX_DENSE_TRANSFORM_PARAMETERS)?;
    assemble_2d(rs, c)
}
pub fn write_itk_text_2d(t: Option<&FileTransform2D>) -> Result<String, RegError> {
    let Some(t) = t else {
        return Err(file_err("nil transform/writer"));
    };
    let mut ts: Vec<FileTransform2D> = vec![t.clone()];
    if let FileTransform2D::Composite(c) = t {
        if c.is_empty() || c.len() + 1 > MAX_HDF5_TRANSFORMS {
            return Err(file_err("empty/oversized 2D composite"));
        }
        for i in (0..c.len()).rev() {
            ts.push(FileTransform2D::Linear(*c.transform(i)));
        }
    }
    let mut b = format!("{ITK_TEXT_HEADER}\n");
    for (i, t) in ts.iter().enumerate() {
        let name = itk_type_name_2d(t).map_err(|e| file_wrap(&e))?;
        b.push_str(&format!("#Transform {i}\nTransform: {name}\n"));
        let tr = t.transform();
        if tr.kind() == Kind::COMPOSITE {
            if i != 0 {
                return Err(file_err("nested composite"));
            }
            continue;
        }
        if !finite_slice(&tr.parameters()) || !finite_slice(&tr.fixed_parameters()) {
            return Err(file_err("non-finite parameters"));
        }
        b.push_str(&format!(
            "Parameters: {}\nFixedParameters: {}\n",
            format_itk_vector(&tr.parameters()),
            format_itk_vector(&tr.fixed_parameters())
        ));
    }
    Ok(b)
}
pub fn write_itk_matlab_2d(t: Option<&FileTransform2D>) -> Result<Vec<u8>, RegError> {
    let Some(FileTransform2D::Linear(l)) = t else {
        return Err(file_err("MATLAB requires one 2D linear transform"));
    };
    let name = itk_type_name_2d(&FileTransform2D::Linear(*l)).map_err(|e| file_wrap(&e))?;
    let mut b = Vec::new();
    for (vname, values) in [
        (name.as_str(), l.parameters()),
        ("fixed", l.fixed_parameters()),
    ] {
        if values.len() > 64 || !finite_slice(&values) {
            return Err(file_err("invalid vector"));
        }
        matlab_vector(&mut b, vname, &values);
    }
    Ok(b)
}
