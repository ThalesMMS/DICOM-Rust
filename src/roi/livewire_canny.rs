#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::edge_levelset::{ERR_CANNY_DOMAIN, ERR_CANNY_NO_EDGES};
use super::grayscale_reconstruction::max32;
use super::levelset::finite_non_negative;
use super::livewire::{ERR_LIVEWIRE, LivewireCostMap, LivewireOptions, LivewireValue};
use super::{Cancellation, ERR_CONTEXT_CANCELED, ERR_CONTEXT_DEADLINE_EXCEEDED, Error, ctx_err};
use crate::imagefilter::{
    CannyLimits, CannyOptions, CannyParameters, ErrorKind, FilterError, Volume, canny,
    canny_working_bytes,
};
use crate::render::parallel;
pub const ERR_CANNY_RESOURCE_LIMIT: &str = "imagefilter: Canny resource limit exceeded";
#[doc(hidden)]
pub const ERR_FILTER_INVALID_VOLUME: &str = "imagefilter: invalid volume";
pub(crate) fn filter_error(e: FilterError) -> Error {
    let sentinel = match e.kind() {
        ErrorKind::CannyResourceLimit => ERR_CANNY_RESOURCE_LIMIT,
        ErrorKind::InvalidVolume => ERR_FILTER_INVALID_VOLUME,
        ErrorKind::Canceled => ERR_CONTEXT_CANCELED,
        ErrorKind::DeadlineExceeded => ERR_CONTEXT_DEADLINE_EXCEEDED,
        _ => "",
    };
    Error {
        sentinel,
        cause: "",
        message: e.to_string(),
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CannyLivewireOptions {
    pub canny: CannyParameters,
    pub path: LivewireOptions,
    pub limits: CannyLimits,
    pub workers: i64,
}
fn poll(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
pub fn new_canny_livewire_cost_map(
    ctx: Option<&dyn Cancellation>,
    columns: i64,
    rows: i64,
    value: Option<LivewireValue<'_>>,
    o: CannyLivewireOptions,
) -> Result<LivewireCostMap, Error> {
    poll(ctx)?;
    let Some(value) = value.filter(|_| columns >= 2 && rows >= 2 && o.workers >= 0) else {
        return Err(Error::wrap(ERR_LIVEWIRE, "Canny source/workers"));
    };
    let l = o.limits.normalize().map_err(filter_error)?;
    if columns > l.max_voxels / rows {
        return Err(Error::wrap(ERR_CANNY_RESOURCE_LIMIT, "livewire source"));
    }
    let p = o.canny.normalize(2).map_err(filter_error)?;
    let mut path = o.path;
    if !finite_non_negative(path.image_weight)
        || !finite_non_negative(path.curvature_weight)
        || path.length_weight.is_nan()
        || path.length_weight.is_infinite()
        || !(path.col_spacing > 0.0 && path.col_spacing <= f64::MAX)
        || !(path.row_spacing > 0.0 && path.row_spacing <= f64::MAX)
    {
        return Err(Error::wrap(
            ERR_LIVEWIRE,
            "finite Canny weights/spacing required",
        ));
    }
    if path.image_weight == 0.0 {
        path.image_weight = 1.0;
    }
    if path.length_weight == 0.0 {
        path.length_weight = 0.1;
    }
    let n = columns * rows;
    let workers = parallel::workers(o.workers).min(parallel::default_workers());
    let bytes = canny_working_bytes([columns, rows, 1], workers).map_err(filter_error)?;
    if bytes + n * 12 > l.max_bytes {
        return Err(Error::wrap(
            ERR_CANNY_RESOURCE_LIMIT,
            "livewire source/filter/cost",
        ));
    }
    let mut src = Volume {
        dims: [columns, rows, 1],
        spacing_mm: [path.col_spacing, path.row_spacing, 0.0],
        data: vec![0.0f32; n as usize],
    };
    for i in 0..n {
        if i % 4096 == 0 {
            poll(ctx)?;
        }
        let Some(v) = value(i % columns, i / columns) else {
            poll(ctx)?;
            return Err(Error::sentinel(ERR_CANNY_DOMAIN));
        };
        if v.is_nan() || v.is_infinite() || v.abs() > f32::MAX as f64 {
            return Err(Error::wrap(ERR_LIVEWIRE, "non-finite Canny value"));
        }
        src.data[i as usize] = v as f32;
    }
    let r = canny(
        ctx,
        &src,
        CannyOptions {
            parameters: p,
            limits: l,
            workers,
        },
    )
    .map_err(filter_error)?;
    if r.edge_count == 0 {
        return Err(Error::sentinel(ERR_CANNY_NO_EDGES));
    }
    let mut maximum = 0.0f32;
    for (i, &v) in r.strength.data.iter().enumerate() {
        if i % 4096 == 0 {
            poll(ctx)?;
        }
        maximum = max32(maximum, v);
    }
    let mut cost = vec![0.0f64; n as usize];
    for (i, &v) in r.strength.data.iter().enumerate() {
        if i % 4096 == 0 {
            poll(ctx)?;
        }
        cost[i] = 1.0 - v as f64 / maximum as f64;
    }
    poll(ctx)?;
    Ok(LivewireCostMap {
        columns,
        rows,
        cost,
        opts: path,
        max_bytes: l.max_bytes,
    })
}
impl LivewireCostMap {
    pub(crate) fn canny_tree_admission(&self, states: i64, directions: i64) -> Result<i64, Error> {
        if states > 16 << 20 || states > i64::from(i32::MAX) || states < 1 {
            return Err(Error::wrap(ERR_CANNY_RESOURCE_LIMIT, "livewire states"));
        }
        let pushes = states * 8 + directions;
        let bytes = self.cost.len() as i64 * 8 + states * (13 + 48) + pushes * 24 + 4096;
        if bytes > self.max_bytes {
            return Err(Error::wrap(
                ERR_CANNY_RESOURCE_LIMIT,
                format!("livewire tree reserves {bytes} bytes"),
            ));
        }
        Ok(pushes)
    }
}
