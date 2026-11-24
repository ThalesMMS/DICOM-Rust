use super::cubes::CubesExtractor;
use super::field::{AffineFrame, Field, snapshot_field};
use super::voxel::VoxelExtractor;
use super::{BudgetError, Mesh, MeshError, VolumeMeshError, dims_v, mesh_bytes};
use crate::render::parallel::Cancellation;
use crate::render::volume::Volume;
use crate::textfmt;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Method(pub i64);
impl Method {
    pub const MARCHING_CUBES: Method = Method(0);
    pub const VOXEL_FACES: Method = Method(1);
}
impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("marching-cubes"),
            1 => f.write_str("voxel-faces"),
            n => write!(f, "Method({n})"),
        }
    }
}
pub const DEFAULT_MAX_TRIANGLES: i64 = 25_000_000;
pub const DEFAULT_MAX_BYTES: i64 = 1 << 30;
pub const DEFAULT_MAX_WORKERS: i64 = 8;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Options {
    pub method: Method,
    pub iso: f64,
    pub open_boundary: bool,
    pub skip_normals: bool,
    pub workers: i64,
    pub max_triangles: i64,
    pub max_bytes: i64,
}
pub fn extract_volume(
    ctx: Option<&dyn Cancellation>,
    volume: &Arc<Volume>,
    opts: Options,
) -> Result<Mesh, VolumeMeshError> {
    let lease = volume.acquire_store_snapshot_context(ctx)?;
    let result = (|| {
        let field = snapshot_field(lease.snapshot()?)?;
        extract(&field, opts).map_err(|e| match field.take_render_error() {
            Some(render) => VolumeMeshError::Render(render),
            None => VolumeMeshError::Mesh(e),
        })
    })();
    lease.release();
    result
}
pub fn extract(field: &dyn Field, opts: Options) -> Result<Mesh, MeshError> {
    if opts.iso.is_nan() || opts.iso.is_infinite() {
        return Err(MeshError::invalid(format!(
            "iso value {}",
            textfmt::float_v(opts.iso)
        )));
    }
    let dims = field.dims();
    validate_field_dimensions(dims)?;
    let frame = AffineFrame::new(field.index_to_patient())?;
    match opts.method {
        Method::MARCHING_CUBES => CubesExtractor::new(field, frame, opts).run(),
        Method::VOXEL_FACES => VoxelExtractor::new(field, frame, opts).run(),
        other => Err(MeshError::invalid(format!("method {other}"))),
    }
}
pub(crate) fn validate_field_dimensions(dims: [i64; 3]) -> Result<(), MeshError> {
    for n in dims {
        if n <= 0 || n > i64::from(i32::MAX) - 2 {
            return Err(MeshError::invalid(format!("dimensions {}", dims_v(dims))));
        }
    }
    if (dims[0] + 2) * (dims[1] + 2) > i64::from(i32::MAX) {
        return Err(MeshError::invalid(format!(
            "slice of {}x{} samples",
            dims[0], dims[1]
        )));
    }
    Ok(())
}
pub(crate) fn budget(
    opts: &Options,
    vertices: i64,
    triangles: i64,
    normals: bool,
) -> Result<(), MeshError> {
    check_output_budget(
        opts,
        vertices,
        triangles,
        mesh_bytes(vertices, triangles, normals),
    )
}
pub(crate) fn check_output_budget(
    opts: &Options,
    vertices: i64,
    triangles: i64,
    bytes: i64,
) -> Result<(), MeshError> {
    let (mut max_triangles, mut max_bytes) = (opts.max_triangles, opts.max_bytes);
    if max_triangles == 0 {
        max_triangles = DEFAULT_MAX_TRIANGLES;
    }
    if max_bytes == 0 {
        max_bytes = DEFAULT_MAX_BYTES;
    }
    if vertices > i64::from(u32::MAX)
        || (max_triangles > 0 && triangles > max_triangles)
        || (max_bytes > 0 && bytes > max_bytes)
    {
        return Err(BudgetError {
            vertices,
            triangles,
            bytes,
            max_triangles,
            max_bytes,
        }
        .into());
    }
    Ok(())
}
pub(crate) fn prefix(counts: &mut [i64]) -> i64 {
    let mut total = 0;
    for c in counts.iter_mut() {
        let n = *c;
        *c = total;
        total += n;
    }
    total
}
pub(crate) fn finite32(v: f32) -> bool {
    v.is_finite()
}
