use super::binary_antialias::{ERR_BINARY_ANTI_ALIAS, anti_alias_finite};
use super::distance3d::{VolumeMask, ctx_check, squared_distance_1d_context};
use super::{Cancellation, Error};
#[allow(clippy::type_complexity)]
pub(crate) fn anti_alias_geometry(
    ctx: Option<&dyn Cancellation>,
    m: &VolumeMask,
) -> Result<([i64; 3], [f64; 3], [f64; 3], usize), Error> {
    let invalid = || Error::sentinel(ERR_BINARY_ANTI_ALIAS);
    let size = [m.columns, m.rows, m.slices];
    let axes: [&[f64]; 3] = [&m.x, &m.y, &m.z];
    let mut product = 1u64;
    for (a, &d) in size.iter().enumerate() {
        if d < 1 || d as u64 > i64::MAX as u64 / product || axes[a].len() as i64 != d {
            return Err(invalid());
        }
        product *= d as u64;
    }
    if size[0] < 2 || size[1] < 2 || m.voxels.len() as u64 != product {
        return Err(invalid());
    }
    let n = product as usize;
    let mut h = [0.0f64; 3];
    let mut origin = [0.0f64; 3];
    for (a, axis) in axes.iter().enumerate() {
        origin[a] = axis[0];
        h[a] = 1.0;
        if axis.len() > 1 {
            h[a] = axis[1] - axis[0];
        }
        if !anti_alias_finite(origin[a]) || !anti_alias_finite(h[a]) || h[a] <= 0.0 {
            return Err(invalid());
        }
        for (i, &v) in axis.iter().enumerate() {
            if i & 4095 == 0 {
                ctx_check(ctx)?;
            }
            let expected = origin[a] + i as f64 * h[a];
            if !anti_alias_finite(v)
                || !anti_alias_finite(expected)
                || i > 0 && v <= axis[i - 1]
                || ((v - origin[a]) - i as f64 * h[a]).abs() > 1e-6 * h[a]
            {
                return Err(Error::wrap(
                    ERR_BINARY_ANTI_ALIAS,
                    "regular finite axes required",
                ));
            }
        }
        let length = (axis.len() - 1) as f64 * h[a];
        if !anti_alias_finite(length)
            || length > f64::from(f32::MAX) / 16.0
            || !anti_alias_finite(length * length)
            || !anti_alias_finite(h[a] * h[a])
            || h[a] * h[a] == 0.0
        {
            return Err(invalid());
        }
    }
    Ok((size, h, origin, n))
}
pub(crate) fn anti_alias_initial(
    ctx: Option<&dyn Cancellation>,
    m: &VolumeMask,
    size: [usize; 3],
    h: [f64; 3],
    hmin: f64,
) -> Result<Vec<f32>, Error> {
    let n = m.voxels.len();
    let mut d = vec![0.0f64; n];
    let mut phi = vec![0.0f32; n];
    let max_axis = size[0].max(size[1]).max(size[2]);
    let mut f = vec![0.0f64; max_axis];
    let mut out = vec![0.0f64; max_axis];
    let mut coords = vec![0.0f64; max_axis];
    let mut hull = vec![0usize; max_axis];
    let mut bounds = vec![0.0f64; max_axis + 1];
    for target in [false, true] {
        for (i, &v) in m.voxels.iter().enumerate() {
            if i & 4095 == 0 {
                ctx_check(ctx)?;
            }
            d[i] = if v == target { 0.0 } else { f64::INFINITY };
        }
        for (axis, &length) in size.iter().enumerate() {
            for (k, coord) in coords.iter_mut().enumerate().take(length) {
                *coord = k as f64 * h[axis];
            }
            let lines = n / length;
            let stride = [1, size[0], size[0] * size[1]][axis];
            for line in 0..lines {
                if line & 63 == 0 {
                    ctx_check(ctx)?;
                }
                let base = match axis {
                    1 => (line / size[0]) * size[0] * size[1] + line % size[0],
                    2 => line,
                    _ => line * length,
                };
                for k in 0..length {
                    if k & 4095 == 0 {
                        ctx_check(ctx)?;
                    }
                    f[k] = d[base + k * stride];
                }
                squared_distance_1d_context(
                    ctx,
                    &mut out[..length],
                    &f[..length],
                    &coords[..length],
                    &mut hull[..length],
                    &mut bounds[..length + 1],
                )?;
                for k in 0..length {
                    if k & 4095 == 0 {
                        ctx_check(ctx)?;
                    }
                    d[base + k * stride] = out[k];
                }
            }
        }
        for (i, &v) in m.voxels.iter().enumerate() {
            if i & 4095 == 0 {
                ctx_check(ctx)?;
            }
            if v != target {
                let mut value = d[i].sqrt() - 0.5 * hmin;
                if target {
                    value = -value;
                }
                if !anti_alias_finite(value) || (value as f32).is_infinite() || value == 0.0 {
                    return Err(Error::sentinel(ERR_BINARY_ANTI_ALIAS));
                }
                phi[i] = value as f32;
            }
        }
    }
    ctx_check(ctx)?;
    Ok(phi)
}
