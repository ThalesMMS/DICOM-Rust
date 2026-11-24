use super::{Error, Kind, direction_frame, numbers, read};
use crate::fmath;
use crate::numeric::linalg::symmetric_eigen3;
use crate::object::{Object, Tag, Vr, is_valid_uid};
use crate::render::Vec3;
pub type Tensor3 = [f64; 6];
pub const TENSOR_LPS: &str = "patient-lps";
pub const TENSOR_IMAGE_AXES: &str = "image";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TensorGeometry {
    pub size: [i64; 3],
    pub origin: Vec3,
    pub spacing: Vec3,
    pub direction: [[f64; 3]; 3],
    pub frame_of_reference_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TensorEncoding {
    pub b_value: f64,
    pub gradient: [f64; 3],
    pub has_gradient: bool,
    pub b_matrix: Tensor3,
    pub has_b_matrix: bool,
    pub axes: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TensorSource {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frame_number: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TensorVolume {
    pub geometry: TensorGeometry,
    pub encoding: TensorEncoding,
    pub signals: Vec<f32>,
    pub sources: Vec<TensorSource>,
}
pub const TENSOR_REJECT_NEGATIVE: u8 = 0;
pub const TENSOR_CLAMP_NEGATIVE: u8 = 1;
pub const TENSOR_VALID: u8 = 0;
pub const TENSOR_MASKED: u8 = 1;
pub const TENSOR_LOW_SIGNAL: u8 = 2;
pub const TENSOR_NONFINITE: u8 = 3;
pub const TENSOR_NEGATIVE: u8 = 4;
pub const TENSOR_NUMERICAL: u8 = 5;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TensorOptions {
    pub mask: Option<Vec<bool>>,
    pub noise_floor: f64,
    pub min_signal: f64,
    pub negative_tolerance: f64,
    pub max_diffusivity: f64,
    pub negative_policy: u8,
    pub workers: i64,
    pub max_volumes: u64,
    pub max_voxels: u64,
    pub max_bytes: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TensorVoxel {
    pub tensor: Tensor3,
    pub eigenvalues: [f64; 3],
    pub fa: f64,
    pub md: f64,
    pub direction: Vec3,
    pub rgb: [u8; 3],
    pub status: u8,
    pub direction_defined: bool,
    pub clamped: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TensorResult {
    pub voxels: Vec<TensorVoxel>,
    pub counts: [i64; 6],
    pub duplicate_encodings: i64,
    pub clamped: i64,
    pub design_diagonal_ratio: f64,
    geometry: TensorGeometry,
    encodings: Vec<Tensor3>,
    sources: Vec<Vec<TensorSource>>,
    options: TensorOptions,
    logical_bytes: u64,
}
impl TensorResult {
    pub fn geometry(&self) -> &TensorGeometry {
        &self.geometry
    }
    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
    pub fn encodings(&self) -> &[Tensor3] {
        &self.encodings
    }
    pub fn sources(&self) -> &[Vec<TensorSource>] {
        &self.sources
    }
    pub fn options(&self) -> &TensorOptions {
        &self.options
    }
}
const TENSOR_VOXEL_SIZE: u64 = 120;
fn finite(x: f64) -> bool {
    !x.is_nan() && !x.is_infinite()
}
pub(super) fn tensor_error(detail: &str) -> Error {
    Error::new(Kind::Tensor, detail)
}
fn matrix(t: &Tensor3) -> [[f64; 3]; 3] {
    [[t[0], t[3], t[4]], [t[3], t[1], t[5]], [t[4], t[5], t[2]]]
}
fn pack(a: &[[f64; 3]; 3]) -> Tensor3 {
    [a[0][0], a[1][1], a[2][2], a[0][1], a[0][2], a[1][2]]
}
fn rotate(t: &Tensor3, r: &[[f64; 3]; 3]) -> Tensor3 {
    let a = matrix(t);
    let mut b = [[0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                for l in 0..3 {
                    b[i][j] += r[i][k] * a[k][l] * r[j][l];
                }
            }
        }
    }
    pack(&b)
}
#[allow(clippy::needless_range_loop)]
pub(super) fn tensor_grid(g: &TensorGeometry) -> Result<usize, Error> {
    let mut n = 1u64;
    for &d in &g.size {
        if d < 1 || d as u64 > (i64::MAX as u64) / n {
            return Err(tensor_error("size overflow"));
        }
        n *= d as u64;
    }
    if !is_valid_uid(g.frame_of_reference_uid.as_bytes()) {
        return Err(tensor_error("frame UID required"));
    }
    let (o, s) = (g.origin, g.spacing);
    if [o.x, o.y, o.z, s.x, s.y, s.z].iter().any(|x| !finite(*x)) {
        return Err(tensor_error("nonfinite grid"));
    }
    if s.x <= 0.0 || s.y <= 0.0 || s.z <= 0.0 {
        return Err(tensor_error("spacing"));
    }
    let r = &g.direction;
    for i in 0..3 {
        for j in 0..3 {
            let mut dot = 0.0;
            for k in 0..3 {
                if !finite(r[k][i]) {
                    return Err(tensor_error("direction"));
                }
                dot += r[k][i] * r[k][j];
            }
            let want = if i == j { 1.0 } else { 0.0 };
            if (dot - want).abs() > 1e-8 {
                return Err(tensor_error("nonorthonormal direction"));
            }
        }
    }
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    if (det - 1.0).abs() > 1e-8 {
        return Err(tensor_error("left-handed direction"));
    }
    let sp = [s.x, s.y, s.z];
    for corner in 0..8 {
        let mut point = [o.x, o.y, o.z];
        for axis in 0..3 {
            if corner & (1 << axis) != 0 {
                for (row, p) in point.iter_mut().enumerate() {
                    *p += g.direction[row][axis] * sp[axis] * (g.size[axis] - 1) as f64;
                }
            }
        }
        if point.iter().any(|x| !finite(*x)) {
            return Err(tensor_error("physical grid overflow"));
        }
    }
    Ok(n as usize)
}
fn same_grid(a: &TensorGeometry, b: &TensorGeometry) -> bool {
    a.size == b.size
        && a.frame_of_reference_uid == b.frame_of_reference_uid
        && a.origin.x == b.origin.x
        && a.origin.y == b.origin.y
        && a.origin.z == b.origin.z
        && a.spacing.x == b.spacing.x
        && a.spacing.y == b.spacing.y
        && a.spacing.z == b.spacing.z
        && a.direction == b.direction
}
fn encoding_lps(e: &TensorEncoding, g: &TensorGeometry) -> Result<Tensor3, Error> {
    if !finite(e.b_value) || e.b_value < 0.0 || e.b_value > 1e7 {
        return Err(tensor_error("b-value"));
    }
    if e.axes != TENSOR_LPS && e.axes != TENSOR_IMAGE_AXES {
        return Err(tensor_error("unknown encoding coordinate frame"));
    }
    let mut from_gradient = [0f64; 6];
    if e.has_gradient {
        let mut norm = 0.0;
        for &v in &e.gradient {
            if !finite(v) {
                return Err(tensor_error("nonfinite gradient"));
            }
            norm += v * v;
        }
        if e.b_value > 0.0 && (norm - 1.0).abs() > 1e-5 {
            return Err(tensor_error("gradient must be unit length"));
        }
        if e.b_value == 0.0 && norm > 1e-10 {
            return Err(tensor_error("b0 has nonzero direction"));
        }
        let [x, y, z] = e.gradient;
        let b = e.b_value;
        from_gradient = [
            b * x * x,
            b * y * y,
            b * z * z,
            b * x * y,
            b * x * z,
            b * y * z,
        ];
    }
    let mut b = from_gradient;
    if e.has_b_matrix {
        b = e.b_matrix;
        if e.b_value == 0.0 && b.iter().any(|v| *v != 0.0) {
            return Err(tensor_error("nonzero b-matrix for b0"));
        }
        if b.iter().any(|v| !finite(*v)) {
            return Err(tensor_error("nonfinite b-matrix"));
        }
        let scale = fmath::max(e.b_value, 1.0);
        let consistent = match symmetric_eigen3(matrix(&b)) {
            Ok((values, _)) => {
                !(values[0] < -1e-8 * scale
                    || (b[0] + b[1] + b[2] - e.b_value).abs() > 1e-6 * scale)
            }
            Err(_) => false,
        };
        if !consistent {
            return Err(tensor_error("b-matrix PSD or trace inconsistent"));
        }
        if e.has_gradient {
            for (i, v) in b.iter().enumerate() {
                if (v - from_gradient[i]).abs() > 1e-6 * scale {
                    return Err(tensor_error("b-matrix inconsistent with gradient"));
                }
            }
        }
    }
    if e.b_value > 0.0 && !e.has_gradient && !e.has_b_matrix {
        return Err(tensor_error("missing encoding"));
    }
    if e.axes == TENSOR_IMAGE_AXES {
        b = rotate(&b, &g.direction);
    }
    Ok(b)
}
pub(super) fn budget_add(total: &mut u64, more: u64, limit: u64) -> bool {
    if *total > limit || more > limit - *total {
        return false;
    }
    *total += more;
    true
}
fn parallel_budget() -> i64 {
    let procs = std::thread::available_parallelism().map_or(1, |n| n.get() as i64);
    (procs - 1).max(0)
}
pub fn fit_tensor(
    volumes: &[TensorVolume],
    options: &TensorOptions,
) -> Result<TensorResult, Error> {
    let mut o = options.clone();
    if o.max_volumes == 0 {
        o.max_volumes = 256;
    }
    if o.max_voxels == 0 {
        o.max_voxels = 1 << 24;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 512 << 20;
    }
    if o.max_diffusivity == 0.0 {
        o.max_diffusivity = 0.1;
    }
    if o.negative_tolerance == 0.0 {
        o.negative_tolerance = 1e-10;
    }
    if volumes.len() < 7
        || volumes.len() as u64 > o.max_volumes
        || volumes.len() > 4096
        || o.workers < 0
        || !finite(o.noise_floor)
        || o.noise_floor < 0.0
        || !finite(o.min_signal)
        || o.min_signal < 0.0
        || !finite(o.negative_tolerance)
        || o.negative_tolerance < 0.0
        || !finite(o.max_diffusivity)
        || o.max_diffusivity <= 0.0
        || o.negative_policy > TENSOR_CLAMP_NEGATIVE
    {
        return Err(tensor_error("options or channel count"));
    }
    let n = tensor_grid(&volumes[0].geometry)?;
    if n as u64 > o.max_voxels || o.mask.as_ref().is_some_and(|m| m.len() != n) {
        return Err(tensor_error("voxel/mask admission"));
    }
    let mut workers = 1 + parallel_budget().min(63);
    if o.workers > 0 {
        workers = workers.min(o.workers);
    }
    workers = workers.min(((n as i64 + 127) / 128).max(1));
    let bytes_per_voxel = volumes.len() as u64 * 4 + TENSOR_VOXEL_SIZE + 1;
    if n as u64 > u64::MAX / bytes_per_voxel {
        return Err(tensor_error("byte overflow"));
    }
    let mut budget = n as u64 * bytes_per_voxel;
    if !budget_add(
        &mut budget,
        volumes.len() as u64 * 256 + workers as u64 * 4096,
        o.max_bytes,
    ) {
        return Err(tensor_error("byte budget"));
    }
    for v in volumes {
        if !same_grid(&v.geometry, &volumes[0].geometry)
            || v.signals.len() != n
            || (!v.sources.is_empty() && v.sources.len() as i64 != v.geometry.size[2])
        {
            return Err(tensor_error("incompatible volume/source geometry"));
        }
        for s in &v.sources {
            if s.study_instance_uid.len() > 64
                || s.series_instance_uid.len() > 64
                || s.sop_class_uid.len() > 64
                || s.sop_instance_uid.len() > 64
                || s.frame_number < 0
            {
                return Err(tensor_error("source metadata"));
            }
            if !budget_add(&mut budget, 384, o.max_bytes) {
                return Err(tensor_error("source byte budget"));
            }
        }
    }
    if budget > o.max_bytes {
        return Err(tensor_error("byte budget"));
    }
    let mut rows = vec![[0f64; 6]; volumes.len()];
    let mut b0 = Vec::new();
    let mut weighted: Vec<usize> = Vec::new();
    let mut max_b = 0.0f64;
    let mut duplicates = 0i64;
    for (i, v) in volumes.iter().enumerate() {
        let b = encoding_lps(&v.encoding, &v.geometry)
            .map_err(|e| Error::with_text(e.kind, format!("volume{i}: {e}")))?;
        rows[i] = b;
        if v.encoding.b_value == 0.0 {
            b0.push(i);
        } else {
            for &previous in &weighted {
                let limit =
                    1e-8 * fmath::max(v.encoding.b_value, volumes[previous].encoding.b_value);
                if b.iter()
                    .zip(&rows[previous])
                    .all(|(x, y)| (x - y).abs() <= limit)
                {
                    duplicates += 1;
                    break;
                }
            }
            weighted.push(i);
            max_b = fmath::max(max_b, v.encoding.b_value);
        }
    }
    if b0.is_empty() {
        return Err(tensor_error("b=0 reference missing"));
    }
    if weighted.len() < 6 {
        return Err(Error::bare(Kind::TensorRank));
    }
    let mut q: Vec<[f64; 6]> = weighted
        .iter()
        .map(|&j| {
            let mut row = [0f64; 6];
            for c in 0..6 {
                row[c] = rows[j][c] / max_b;
                if c >= 3 {
                    row[c] *= 2.0;
                }
            }
            row
        })
        .collect();
    let mut r = [[0f64; 6]; 6];
    let (mut small, mut large) = (f64::INFINITY, 0.0f64);
    for col in 0..6 {
        for _pass in 0..2 {
            for prev in 0..col {
                let mut dot = 0.0;
                for row in &q {
                    dot += row[col] * row[prev];
                }
                r[prev][col] += dot;
                for row in q.iter_mut() {
                    row[col] -= dot * row[prev];
                }
            }
        }
        let mut norm = 0.0;
        for row in &q {
            norm += row[col] * row[col];
        }
        norm = norm.sqrt();
        if !finite(norm) || norm <= 1e-10 * (q.len() as f64).sqrt() {
            return Err(Error::bare(Kind::TensorRank));
        }
        r[col][col] = norm;
        small = fmath::min(small, norm);
        large = fmath::max(large, norm);
        for row in q.iter_mut() {
            row[col] /= norm;
        }
    }
    let mut result = TensorResult {
        geometry: volumes[0].geometry.clone(),
        encodings: rows.clone(),
        sources: volumes.iter().map(|v| v.sources.clone()).collect(),
        options: TensorOptions {
            mask: None,
            ..o.clone()
        },
        logical_bytes: budget,
        duplicate_encodings: duplicates,
        design_diagonal_ratio: large / small,
        voxels: vec![TensorVoxel::default(); n],
        ..TensorResult::default()
    };
    for voxel in 0..n {
        let mut out = TensorVoxel::default();
        if o.mask.as_ref().is_some_and(|m| !m[voxel]) {
            out.status = TENSOR_MASKED;
        } else {
            let mut status = TENSOR_VALID;
            for v in volumes {
                let s = f64::from(v.signals[voxel]);
                if !finite(s) {
                    status = TENSOR_NONFINITE;
                    break;
                }
                if s - o.noise_floor <= o.min_signal {
                    status = TENSOR_LOW_SIGNAL;
                    break;
                }
            }
            if status == TENSOR_VALID {
                let mut s0 = 0.0;
                for &i in &b0 {
                    s0 += f64::from(volumes[i].signals[voxel]) - o.noise_floor;
                }
                s0 /= b0.len() as f64;
                let mut rhs = [0f64; 6];
                for (row, &i) in weighted.iter().enumerate() {
                    let y = fmath::log(s0)
                        - fmath::log(f64::from(volumes[i].signals[voxel]) - o.noise_floor);
                    for c in 0..6 {
                        rhs[c] += q[row][c] * y;
                    }
                }
                let mut d = [0f64; 6];
                for col in (0..6).rev() {
                    let mut v = rhs[col];
                    for j in col + 1..6 {
                        v -= r[col][j] * d[j];
                    }
                    d[col] = v / r[col][col];
                }
                for c in d.iter_mut() {
                    *c /= max_b;
                }
                out = tensor_quantities(d, &o);
                status = out.status;
            }
            out.status = status;
        }
        result.voxels[voxel] = out;
        result.counts[out.status as usize] += 1;
        if out.clamped {
            result.clamped += 1;
        }
    }
    Ok(result)
}
fn tensor_quantities(mut d: Tensor3, o: &TensorOptions) -> TensorVoxel {
    let mut out = TensorVoxel {
        status: TENSOR_NUMERICAL,
        ..TensorVoxel::default()
    };
    let mut scale = 0.0f64;
    for &v in &d {
        if !finite(v) {
            return out;
        }
        scale = fmath::max(scale, v.abs());
    }
    if scale == 0.0 {
        out.tensor = d;
        out.status = TENSOR_VALID;
        return out;
    }
    let mut normalized = d;
    for v in normalized.iter_mut() {
        *v /= scale;
    }
    let Ok((mut w, v)) = symmetric_eigen3(matrix(&normalized)) else {
        return out;
    };
    for x in w.iter_mut() {
        *x *= scale;
        if !finite(*x) || x.abs() > o.max_diffusivity {
            return out;
        }
    }
    if w[0] < -o.negative_tolerance && o.negative_policy == TENSOR_REJECT_NEGATIVE {
        out.status = TENSOR_NEGATIVE;
        return out;
    }
    for x in w.iter_mut() {
        if *x < 0.0 {
            *x = 0.0;
            out.clamped = true;
        }
    }
    if out.clamped {
        let mut m = [[0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    m[i][j] += v[i][k] * w[k] * v[j][k];
                }
            }
        }
        d = pack(&m);
    }
    out.tensor = d;
    out.eigenvalues = [w[2], w[1], w[0]];
    out.md = w[0] / 3.0 + w[1] / 3.0 + w[2] / 3.0;
    if w[2] > 0.0 {
        let (a, b, c) = (w[0] / w[2], w[1] / w[2], 1.0);
        let mean = (a + b + c) / 3.0;
        out.fa = fmath::min(
            1.0,
            (1.5 * ((a - mean) * (a - mean) + (b - mean) * (b - mean) + (c - mean) * (c - mean))
                / (a * a + b * b + c * c))
                .sqrt(),
        );
    }
    if !finite(out.md) || !finite(out.fa) {
        return TensorVoxel {
            status: TENSOR_NUMERICAL,
            ..TensorVoxel::default()
        };
    }
    out.status = TENSOR_VALID;
    if w[2] > 0.0 && w[2] - w[1] > 1e-8 * w[2] {
        out.direction_defined = true;
        let mut components = [v[0][2], v[1][2], v[2][2]];
        let mut pivot = 0;
        for i in 1..3 {
            if components[i].abs() > components[pivot].abs() {
                pivot = i;
            }
        }
        if components[pivot] < 0.0 {
            for c in components.iter_mut() {
                *c = -*c;
            }
        }
        out.direction = Vec3::new(components[0], components[1], components[2]);
        for (i, x) in components.iter().enumerate() {
            out.rgb[i] = fmath::f64_to_u8((255.0 * fmath::min(1.0, out.fa) * x.abs()).round());
        }
    }
    out
}
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_MR_DIFFUSION_SEQUENCE: Tag = Tag::new(0x0018, 0x9117);
const TAG_B_MATRIX_SEQUENCE: Tag = Tag::new(0x0018, 0x9601);
fn frame_admission(ds: Option<&Object>, index: i64) -> Result<(), Error> {
    let Some(ds) = ds else {
        return Err(tensor_error("DICOM source/frame"));
    };
    if index < 0 {
        return Err(tensor_error("DICOM source/frame"));
    }
    let mut n = 1i64;
    if let Some(text) = ds.get_string(TAG_NUMBER_OF_FRAMES) {
        match crate::numtext::parse_int(&text) {
            Ok(v) if v >= 1 => n = v,
            _ => return Err(tensor_error("NumberOfFrames")),
        }
    }
    if index >= n {
        return Err(tensor_error("frame outside source"));
    }
    Ok(())
}
pub fn read_tensor_encoding(
    ds: Option<&Object>,
    frame_index: i64,
) -> Result<TensorEncoding, Error> {
    frame_admission(ds, frame_index)?;
    let Some(ds) = ds else {
        return Err(tensor_error("DICOM source/frame"));
    };
    let f = read(Some(ds), frame_index)?;
    let mut e = TensorEncoding {
        b_value: f.b_value,
        gradient: f.direction,
        has_gradient: f.has_direction,
        axes: TENSOR_LPS.into(),
        ..TensorEncoding::default()
    };
    if f.direction_frame != direction_frame::PATIENT
        && f.direction_frame != direction_frame::PHILIPS_RL_AP_FH
    {
        return Err(tensor_error(
            "vendor gradient axes need explicit conversion",
        ));
    }
    let (item, _, err) = ds.functional_group_item(frame_index, TAG_MR_DIFFUSION_SEQUENCE);
    if let Some(err) = err {
        return Err(Error::with_text(Kind::Other, err.to_string()));
    }
    let item = item.unwrap_or_else(|| ds.clone());
    if let Some(seq) = item.get_sequence(TAG_B_MATRIX_SEQUENCE) {
        if seq.len() != 1 {
            return Err(tensor_error("invalid b-matrix sequence"));
        }
        let tags: [u16; 6] = [0x9602, 0x9605, 0x9607, 0x9603, 0x9604, 0x9606];
        for (i, &tag) in tags.iter().enumerate() {
            match numbers(&seq[0], Tag::new(0x0018, tag), Vr::FD) {
                Ok(v) if v.len() == 1 => e.b_matrix[i] = v[0],
                _ => return Err(tensor_error("invalid b-matrix coefficient")),
            }
        }
        e.has_b_matrix = true;
    }
    encoding_lps(&e, &TensorGeometry::default())?;
    Ok(e)
}
pub fn read_tensor_source(ds: Option<&Object>, frame_index: i64) -> Result<TensorSource, Error> {
    frame_admission(ds, frame_index)?;
    let Some(ds) = ds else {
        return Err(tensor_error("DICOM source/frame"));
    };
    let read_uid = |tag: Tag| -> Result<String, Error> {
        match ds.get_string(tag) {
            Some(v) if is_valid_uid(v.as_bytes()) => Ok(v),
            _ => Err(tensor_error("source identity UID")),
        }
    };
    let mut s = TensorSource {
        study_instance_uid: read_uid(Tag::new(0x0020, 0x000d))?,
        series_instance_uid: read_uid(Tag::new(0x0020, 0x000e))?,
        sop_class_uid: read_uid(Tag::new(0x0008, 0x0016))?,
        sop_instance_uid: read_uid(Tag::new(0x0008, 0x0018))?,
        frame_number: 0,
    };
    if ds.has(TAG_NUMBER_OF_FRAMES) {
        s.frame_number = frame_index + 1;
    }
    Ok(s)
}
