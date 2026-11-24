use super::error::{RegError, Sentinel};
use super::matrix::finite_slice;
use super::method::LevelMinimizer;
use crate::optimize::{
    self, Context, Exhaustive, Function, GradientFunction, MultiStart, OptimizeResult,
    SearchFunction, SearchLimits, StopReason,
};
#[derive(Debug, Clone, Default)]
pub struct CoarseSearchOptions {
    pub starts: Vec<Vec<f64>>,
    pub grid: Option<Exhaustive>,
    pub limits: SearchLimits,
}
fn options_error() -> RegError {
    RegError::bare(Sentinel::OptimizeOptions)
}
fn budget() -> RegError {
    RegError::bare(Sentinel::OptimizeSearchBudget)
}
struct Up<'a>(&'a mut dyn GradientFunction);
impl Function for Up<'_> {
    fn value(&mut self, x: &[f64]) -> Result<f64, optimize::Error> {
        self.0.value(x)
    }
    fn as_gradient(&mut self) -> Option<&mut dyn GradientFunction> {
        Some(&mut *self.0)
    }
}
pub fn coarse_search_minimizer<'a>(
    ctx: Context<'a>,
    opts: &CoarseSearchOptions,
    refine: Option<LevelMinimizer<'a>>,
) -> Result<LevelMinimizer<'a>, RegError> {
    ctx.err().map_err(|_| RegError::canceled())?;
    let l = opts.limits;
    let max_starts = if l.max_starts == 0 {
        4096
    } else {
        l.max_starts
    };
    let max_params = if l.max_parameters == 0 {
        4096
    } else {
        l.max_parameters
    };
    let max_bytes = if l.max_workspace_bytes == 0 {
        64 << 20
    } else {
        l.max_workspace_bytes
    };
    if max_starts < 0 || max_params < 0 || max_bytes < 0 || l.max_evaluations < 0 {
        return Err(options_error());
    }
    if max_bytes < 8192 || opts.starts.len() as i64 > max_starts {
        return Err(budget());
    }
    let mut elements = 0i64;
    let mut n = 0usize;
    for x in &opts.starts {
        if n == 0 {
            n = x.len();
        }
        if x.is_empty() || x.len() != n || x.len() as i64 > max_params || !finite_slice(x) {
            return Err(RegError::new(Sentinel::OptimizeOptions, "coarse starts"));
        }
        if elements > (max_bytes - 8192) / 8 - x.len() as i64 {
            return Err(budget());
        }
        elements += x.len() as i64;
    }
    if opts.starts.len() as i64 > (max_bytes - 8192 - 8 * elements) / 24 {
        return Err(budget());
    }
    let mut hook_bytes = 8192 + 8 * elements + 24 * opts.starts.len() as i64;
    if let Some(g) = &opts.grid {
        let gl = g.limits;
        if gl.max_parameters < 0
            || gl.max_starts < 0
            || gl.max_evaluations < 0
            || gl.max_workspace_bytes < 0
        {
            return Err(options_error());
        }
        let (steps, sizes) = (g.steps.len() as i64, g.step_sizes.len() as i64);
        if steps > max_params
            || sizes > max_params
            || steps > (max_bytes - hook_bytes) / 8
            || sizes > (max_bytes - hook_bytes) / 8 - steps
        {
            return Err(budget());
        }
        hook_bytes += 8 * (steps + sizes);
    }
    let mut search_limits = l;
    search_limits.max_workspace_bytes = max_bytes - hook_bytes;
    if opts.grid.is_some() {
        search_limits.max_workspace_bytes /= 2;
    }
    if search_limits.max_workspace_bytes < 8192 {
        return Err(budget());
    }
    let starts = opts.starts.clone();
    let mut grid = opts.grid.clone();
    if let Some(g) = grid.as_mut() {
        if g.limits.max_workspace_bytes < 0 {
            return Err(options_error());
        }
        if g.limits.max_workspace_bytes == 0
            || g.limits.max_workspace_bytes > search_limits.max_workspace_bytes
        {
            g.limits.max_workspace_bytes = search_limits.max_workspace_bytes;
        }
    }
    let mut refine = refine;
    Ok(Box::new(
        move |level: i64,
              objective: &mut dyn GradientFunction,
              start: &[f64],
              scales: &[f64]|
              -> Result<OptimizeResult, optimize::Error> {
            ctx.err()?;
            if level < 0 {
                return Err(options_error().into());
            }
            if level != 0
                && let Some(r) = refine.as_mut()
            {
                let run = r(level, objective, start, scales);
                ctx.err()?;
                return run;
            }
            let points = if level != 0 || starts.is_empty() {
                vec![start.to_vec()]
            } else {
                starts.clone()
            };
            let mut multi = MultiStart {
                starts: points,
                local: None,
                limits: search_limits,
            };
            let refine_ref = &mut refine;
            if level == 0 && (grid.is_some() || refine_ref.is_some()) {
                let grid = &grid;
                multi.local = Some(Box::new(
                    move |sf: &mut SearchFunction<'_>,
                          x: &[f64]|
                          -> Result<OptimizeResult, optimize::Error> {
                        let mut run = OptimizeResult {
                            x: x.to_vec(),
                            value: 0.0,
                            iterations: 0,
                            evaluations: 0,
                            stop: StopReason::MaximumIterations,
                        };
                        if let Some(g) = grid {
                            let local_ctx = sf.context();
                            run = g.minimize_context(local_ctx, sf, x)?;
                        }
                        if let Some(r) = refine_ref.as_mut() {
                            return r(level, sf, &run.x, scales);
                        }
                        Ok(run)
                    },
                ));
            }
            multi.minimize_context(ctx, &mut Up(objective))
        },
    ))
}
