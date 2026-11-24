use super::{Context, Error, Function, GradientFunction, OptimizeResult, StopReason, options};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchLimits {
    pub max_parameters: i64,
    pub max_evaluations: i64,
    pub max_starts: i64,
    pub max_workspace_bytes: i64,
}
impl SearchLimits {
    fn normalized(mut self) -> Result<Self, Error> {
        if self.max_parameters < 0
            || self.max_evaluations < 0
            || self.max_starts < 0
            || self.max_workspace_bytes < 0
        {
            return Err(options("negative search limit"));
        }
        if self.max_parameters == 0 {
            self.max_parameters = 4096;
        }
        if self.max_evaluations == 0 {
            self.max_evaluations = 100000;
        }
        if self.max_starts == 0 {
            self.max_starts = 4096;
        }
        if self.max_workspace_bytes == 0 {
            self.max_workspace_bytes = 64 << 20;
        }
        Ok(self)
    }
}
fn search_admission(l: SearchLimits, n: i64, starts: i64) -> Result<SearchLimits, Error> {
    let l = l.normalized()?;
    if n < 1 || starts < 1 {
        return Err(options("empty search"));
    }
    if n > l.max_parameters || starts > l.max_starts {
        return Err(Error::SearchBudget);
    }
    const ALLOWANCE: i64 = 8192;
    if n > (i64::MAX - ALLOWANCE) / 96 {
        return Err(Error::SearchBudget);
    }
    let mut bytes = ALLOWANCE + 96 * n;
    if n > (i64::MAX - 24) / 8 {
        return Err(Error::SearchBudget);
    }
    let per_start = 8 * n + 24;
    if starts > (i64::MAX - bytes) / per_start {
        return Err(Error::SearchBudget);
    }
    bytes += per_start * starts;
    if bytes > l.max_workspace_bytes {
        return Err(Error::SearchBudget);
    }
    Ok(l)
}
fn search_finite(x: &[f64]) -> bool {
    x.iter().all(|v| v.is_finite())
}
pub struct SearchFunction<'f> {
    ctx: Context<'f>,
    f: &'f mut dyn Function,
    n: usize,
    calls: i64,
    maximum: i64,
}
impl<'f> SearchFunction<'f> {
    fn new(
        ctx: Context<'f>,
        f: &'f mut dyn Function,
        n: usize,
        maximum: i64,
    ) -> Result<Self, Error> {
        ctx.err()?;
        Ok(SearchFunction {
            ctx,
            f,
            n,
            calls: 0,
            maximum,
        })
    }
    pub fn context(&self) -> Context<'f> {
        self.ctx
    }
    fn before(&mut self, x: &[f64]) -> Result<(), Error> {
        self.ctx.err()?;
        if x.len() != self.n || !search_finite(x) {
            return Err(options("invalid search parameters"));
        }
        if self.calls >= self.maximum {
            return Err(Error::SearchBudget);
        }
        self.calls += 1;
        Ok(())
    }
}
impl Function for SearchFunction<'_> {
    fn value(&mut self, x: &[f64]) -> Result<f64, Error> {
        self.before(x)?;
        let v = self.f.value(x);
        self.ctx.err()?;
        let v = v?;
        if v.is_nan() || v == f64::NEG_INFINITY {
            return Err(options("invalid search value"));
        }
        Ok(v)
    }
    fn as_gradient(&mut self) -> Option<&mut dyn GradientFunction> {
        Some(self)
    }
}
impl GradientFunction for SearchFunction<'_> {
    fn value_and_gradient(&mut self, x: &[f64]) -> Result<(f64, Vec<f64>), Error> {
        if self.f.as_gradient().is_none() {
            return Err(options("function has no gradient"));
        }
        self.before(x)?;
        let Some(g) = self.f.as_gradient() else {
            return Err(options("function has no gradient"));
        };
        let evaluated = g.value_and_gradient(x);
        self.ctx.err()?;
        let (v, gradient) = evaluated?;
        if v.is_nan() || v == f64::NEG_INFINITY {
            return Err(options("invalid search value"));
        }
        if v == f64::INFINITY {
            return Ok((v, Vec::new()));
        }
        if gradient.len() != self.n || !search_finite(&gradient) {
            return Err(options("invalid search gradient"));
        }
        Ok((v, gradient))
    }
}
#[derive(Debug, Clone, Default)]
pub struct Exhaustive {
    pub steps: Vec<i64>,
    pub step_sizes: Vec<f64>,
    pub limits: SearchLimits,
}
impl Exhaustive {
    pub fn minimize(&self, f: &mut dyn Function, x0: &[f64]) -> Result<OptimizeResult, Error> {
        self.minimize_context(Context::background(), f, x0)
    }
    pub fn minimize_context(
        &self,
        ctx: Context<'_>,
        f: &mut dyn Function,
        x0: &[f64],
    ) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        let l = search_admission(self.limits, n as i64, 1)?;
        if self.steps.len() != n || self.step_sizes.len() != n || !search_finite(x0) {
            return Err(options("exhaustive dimensions"));
        }
        let mut count: i64 = 1;
        for (i, &s) in self.steps.iter().enumerate() {
            let step = self.step_sizes[i];
            if !(0..=(i64::MAX - 1) / 2).contains(&s)
                || !(step > 0.0)
                || step.is_infinite()
                || step.is_nan()
            {
                return Err(options("exhaustive axis"));
            }
            let width = 2 * s + 1;
            if count > l.max_evaluations / width {
                return Err(Error::SearchBudget);
            }
            count *= width;
            let offset = s as f64 * step;
            if offset.is_infinite() || !search_finite(&[x0[i] - offset, x0[i] + offset]) {
                return Err(options("exhaustive endpoint overflow"));
            }
        }
        let mut c = SearchFunction::new(ctx, f, n, l.max_evaluations)?;
        let start = x0.to_vec();
        let steps = self.steps.clone();
        let sizes = self.step_sizes.clone();
        let mut trial = vec![0.0; n];
        let mut best = vec![0.0; n];
        let mut value = f64::INFINITY;
        for ordinal in 0..count {
            let mut at = ordinal;
            for (i, &s) in steps.iter().enumerate() {
                let width = 2 * s + 1;
                trial[i] = start[i] + (at % width - s) as f64 * sizes[i];
                at /= width;
            }
            let v = c.value(&trial)?;
            if v < value {
                value = v;
                best.copy_from_slice(&trial);
            }
        }
        if value == f64::INFINITY {
            return Err(Error::NoFiniteCandidate);
        }
        Ok(OptimizeResult {
            x: best,
            value,
            iterations: count,
            evaluations: c.calls,
            stop: StopReason::ExhaustiveComplete,
        })
    }
}
pub type LocalMinimizer<'l> =
    Box<dyn FnMut(&mut SearchFunction<'_>, &[f64]) -> Result<OptimizeResult, Error> + 'l>;
#[derive(Default)]
pub struct MultiStart<'l> {
    pub starts: Vec<Vec<f64>>,
    pub local: Option<LocalMinimizer<'l>>,
    pub limits: SearchLimits,
}
impl MultiStart<'_> {
    pub fn minimize(&mut self, f: &mut dyn Function) -> Result<OptimizeResult, Error> {
        self.minimize_context(Context::background(), f)
    }
    pub fn minimize_context(
        &mut self,
        ctx: Context<'_>,
        f: &mut dyn Function,
    ) -> Result<OptimizeResult, Error> {
        if self.starts.is_empty() {
            return Err(options("no starts"));
        }
        let n = self.starts[0].len();
        let l = search_admission(self.limits, n as i64, self.starts.len() as i64)?;
        if self.starts.len() as i64 > l.max_evaluations {
            return Err(Error::SearchBudget);
        }
        for x in &self.starts {
            if x.len() != n || !search_finite(x) {
                return Err(options("inconsistent starts"));
            }
        }
        let mut c = SearchFunction::new(ctx, f, n, l.max_evaluations)?;
        let mut starts = Vec::with_capacity(self.starts.len());
        for x in &self.starts {
            c.ctx.err()?;
            starts.push(x.clone());
        }
        let mut best = vec![0.0; n];
        let mut value = f64::INFINITY;
        for start in &starts {
            let v = c.value(start)?;
            if v < value {
                value = v;
                best.copy_from_slice(start);
            }
            let Some(local) = self.local.as_mut() else {
                continue;
            };
            if v == f64::INFINITY {
                continue;
            }
            let run = local(&mut c, start);
            c.ctx.err()?;
            let run = run?;
            let v = c.value(&run.x)?;
            if v < value {
                value = v;
                best.copy_from_slice(&run.x);
            }
        }
        if value == f64::INFINITY {
            return Err(Error::NoFiniteCandidate);
        }
        Ok(OptimizeResult {
            x: best,
            value,
            iterations: starts.len() as i64,
            evaluations: c.calls,
            stop: StopReason::MultiStartComplete,
        })
    }
}
