use super::lbfgs::{Trial, WOLFE_CURVATURE, WOLFE_DECREASE, interpolate_step};
use super::{
    Context, DEFAULT_VALUE_TOLERANCE, Error, GradientFunction, OptimizeResult, StopReason,
    default_to, default_to_int, max_abs, options,
};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LbfgsbLimits {
    pub max_parameters: i64,
    pub max_workspace_bytes: i64,
}
#[derive(Debug, Clone, Default)]
pub struct Lbfgsb {
    pub lower_bounds: Option<Vec<f64>>,
    pub upper_bounds: Option<Vec<f64>>,
    pub memory: i64,
    pub maximum_iterations: i64,
    pub maximum_evaluations: i64,
    pub maximum_line_search_evaluations: i64,
    pub projected_gradient_tolerance: f64,
    pub value_tolerance: f64,
    pub initial_step_length: f64,
    pub limits: LbfgsbLimits,
}
#[derive(Debug, Clone, Copy, Default)]
struct Config {
    n: usize,
    memory: usize,
    iterations: i64,
    evaluations: i64,
    line_evaluations: i64,
    gradient_tolerance: f64,
    value_tolerance: f64,
    initial_step: f64,
    bytes: i64,
}
impl Lbfgsb {
    pub fn workspace_bytes(&self, n: i64) -> Result<i64, Error> {
        self.resolve(&Context::background(), n).map(|c| c.bytes)
    }
    fn resolve(&self, ctx: &Context<'_>, n: i64) -> Result<Config, Error> {
        if n <= 0
            || self.memory < 0
            || self.memory > 64
            || self.maximum_iterations < 0
            || self.maximum_evaluations < 0
            || self.maximum_line_search_evaluations < 0
            || self.limits.max_parameters < 0
            || self.limits.max_workspace_bytes < 0
        {
            return Err(options("L-BFGS-B dimensions or counts"));
        }
        for value in [
            self.projected_gradient_tolerance,
            self.value_tolerance,
            self.initial_step_length,
        ] {
            if value.is_nan() || value.is_infinite() || value < 0.0 {
                return Err(options("L-BFGS-B scalar"));
            }
        }
        let max_n = default_to_int(self.limits.max_parameters, 1 << 20);
        if n > max_n {
            return Err(Error::LbfgsbResourceLimit);
        }
        let len_ok = |b: &Option<Vec<f64>>| b.as_ref().is_none_or(|v| v.len() as i64 == n);
        if !len_ok(&self.lower_bounds) || !len_ok(&self.upper_bounds) {
            return Err(options("L-BFGS-B bound length"));
        }
        if self.lower_bounds.is_some() || self.upper_bounds.is_some() {
            for i in 0..n as usize {
                ctx.tick(i)?;
                let lo = self
                    .lower_bounds
                    .as_ref()
                    .map_or(f64::NEG_INFINITY, |v| v[i]);
                let hi = self.upper_bounds.as_ref().map_or(f64::INFINITY, |v| v[i]);
                if lo.is_nan()
                    || hi.is_nan()
                    || lo == f64::INFINITY
                    || hi == f64::NEG_INFINITY
                    || lo > hi
                {
                    return Err(options("L-BFGS-B bounds"));
                }
            }
        }
        let mut c = Config {
            n: n as usize,
            memory: default_to_int(self.memory, 5) as usize,
            iterations: default_to_int(self.maximum_iterations, 100),
            evaluations: default_to_int(self.maximum_evaluations, 2000),
            line_evaluations: default_to_int(self.maximum_line_search_evaluations, 20),
            gradient_tolerance: default_to(self.projected_gradient_tolerance, 1e-5),
            value_tolerance: default_to(self.value_tolerance, DEFAULT_VALUE_TOLERANCE),
            initial_step: self.initial_step_length,
            bytes: 0,
        };
        let m = c.memory as i64;
        let factor = 16 * m + 192;
        let tail = 256 * m * m + 512 * m + 8192;
        if n > (i64::MAX - tail) / factor {
            return Err(Error::LbfgsbResourceLimit);
        }
        c.bytes = factor * n + tail;
        let mut maximum = self.limits.max_workspace_bytes;
        if maximum == 0 {
            maximum = 512 << 20;
        }
        if c.bytes > maximum || n as u64 > (u64::MAX >> 1) / (2 * m as u64 + 16) {
            return Err(Error::LbfgsbResourceLimit);
        }
        Ok(c)
    }
    pub fn minimize(
        &self,
        f: &mut dyn GradientFunction,
        x0: &[f64],
    ) -> Result<OptimizeResult, Error> {
        self.minimize_context(Context::background(), f, x0)
    }
    pub fn minimize_context(
        &self,
        ctx: Context<'_>,
        f: &mut dyn GradientFunction,
        x0: &[f64],
    ) -> Result<OptimizeResult, Error> {
        ctx.err()?;
        let c = self.resolve(&ctx, x0.len() as i64)?;
        for (i, v) in x0.iter().enumerate() {
            ctx.tick(i)?;
            if !v.is_finite() {
                return Err(options("non-finite L-BFGS-B start"));
            }
        }
        let n = c.n;
        let mut model = Model::new(ctx, n, c.memory);
        let mut x = vec![0.0; n];
        let mut g = vec![0.0; n];
        let mut lo = vec![0.0; n];
        let mut hi = vec![0.0; n];
        let mut xc = vec![0.0; n];
        let mut d = vec![0.0; n];
        let mut z = vec![0.0; n];
        let mut bz = vec![0.0; n];
        let mut step = vec![0.0; n];
        let mut change = vec![0.0; n];
        for (i, &v) in x0.iter().enumerate() {
            ctx.tick(i)?;
            lo[i] = self
                .lower_bounds
                .as_ref()
                .map_or(f64::NEG_INFINITY, |b| b[i]);
            hi[i] = self.upper_bounds.as_ref().map_or(f64::INFINITY, |b| b[i]);
            x[i] = clamp(v, lo[i], hi[i]);
        }
        let mut search = Search {
            ctx,
            f,
            config: c,
            evaluations: 0,
            lo: lo.clone(),
            hi: hi.clone(),
            trial_x: vec![0.0; n],
            trial_g: vec![0.0; n],
            best_x: vec![0.0; n],
            best_g: vec![0.0; n],
        };
        let mut value = search.evaluate_into(&x, &mut g)?;
        if value == f64::INFINITY {
            return Err(options("L-BFGS-B start has no finite value"));
        }
        let mut active = vec![false; n];
        let mut breaks: Vec<Breakpoint> = Vec::with_capacity(n);
        let mut result = OptimizeResult {
            x: Vec::new(),
            value,
            iterations: 0,
            evaluations: 0,
            stop: StopReason::MaximumIterations,
        };
        for iteration in 0..c.iterations {
            let residual = projected_norm(&ctx, &x, &g, &lo, &hi)?;
            if residual <= c.gradient_tolerance {
                result.stop = StopReason::ProjectedGradient;
                break;
            }
            if search.exhausted() {
                result.stop = StopReason::MaximumEvaluations;
                break;
            }
            let mut attempt = |model: &mut Model<'_>| -> Result<(), Error> {
                model.build()?;
                model.cauchy(&x, &g, &lo, &hi, &mut xc, &mut d, &mut active, &mut breaks)?;
                model.subspace(&x, &g, &lo, &hi, &mut xc, &mut z, &mut bz, &active)
            };
            let mut err = attempt(&mut model);
            if matches!(err, Err(Error::LbfgsbModel)) {
                model.reset();
                err = attempt(&mut model);
            }
            err?;
            for i in 0..n {
                ctx.tick(i)?;
                d[i] = xc[i] - x[i];
            }
            let slope = dot_checked(&ctx, &g, &d)?;
            if !(slope < 0.0) {
                result.stop = StopReason::LineSearch;
                break;
            }
            let mut initial = 1.0;
            if model.count == 0 {
                if c.initial_step > 0.0 {
                    initial = fmath::min(1.0, c.initial_step / max_abs(&d));
                } else {
                    let norm = dot_checked(&ctx, &d, &d)?;
                    initial = fmath::min(1.0, 1.0 / f64::sqrt(norm));
                }
            }
            let (next, ok) = search.run(&x, value, slope, &d, initial)?;
            if !ok {
                result.stop = if search.exhausted() {
                    StopReason::MaximumEvaluations
                } else {
                    StopReason::LineSearch
                };
                break;
            }
            for i in 0..n {
                ctx.tick(i)?;
                step[i] = search.best_x[i] - x[i];
                change[i] = search.best_g[i] - g[i];
            }
            model.add(&step, &change)?;
            let previous = value;
            value = next;
            x.copy_from_slice(&search.best_x);
            g.copy_from_slice(&search.best_g);
            result.iterations = iteration + 1;
            result.value = value;
            let residual = projected_norm(&ctx, &x, &g, &lo, &hi)?;
            if residual <= c.gradient_tolerance {
                result.stop = StopReason::ProjectedGradient;
                break;
            }
            if previous - value
                <= c.value_tolerance * fmath::max(fmath::max(previous.abs(), value.abs()), 1.0)
            {
                result.stop = StopReason::ValueTolerance;
                break;
            }
            if search.exhausted() {
                result.stop = StopReason::MaximumEvaluations;
                break;
            }
        }
        ctx.err()?;
        result.x = x;
        result.evaluations = search.evaluations;
        Ok(result)
    }
}
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    fmath::max(lo, fmath::min(hi, x))
}
fn projected_norm(
    ctx: &Context<'_>,
    x: &[f64],
    g: &[f64],
    lo: &[f64],
    hi: &[f64],
) -> Result<f64, Error> {
    let mut out = 0.0;
    for i in 0..x.len() {
        ctx.tick(i)?;
        let mut v = 0.0;
        if g[i] > 0.0 {
            v = fmath::min(g[i], x[i] - lo[i]);
        } else if g[i] < 0.0 {
            v = fmath::min(-g[i], hi[i] - x[i]);
        }
        out = fmath::max(out, v);
    }
    Ok(out)
}
fn dot_checked(ctx: &Context<'_>, a: &[f64], b: &[f64]) -> Result<f64, Error> {
    let mut v = 0.0;
    for i in 0..a.len() {
        ctx.tick(i)?;
        v += a[i] * b[i];
    }
    if v.is_nan() || v.is_infinite() {
        return Err(Error::LbfgsbModel);
    }
    Ok(v)
}
struct Search<'a, 'f> {
    ctx: Context<'a>,
    f: &'f mut dyn GradientFunction,
    config: Config,
    evaluations: i64,
    lo: Vec<f64>,
    hi: Vec<f64>,
    trial_x: Vec<f64>,
    trial_g: Vec<f64>,
    best_x: Vec<f64>,
    best_g: Vec<f64>,
}
impl Search<'_, '_> {
    fn exhausted(&self) -> bool {
        self.evaluations >= self.config.evaluations
    }
    fn evaluate_into(&mut self, x: &[f64], g: &mut [f64]) -> Result<f64, Error> {
        self.ctx.err()?;
        if self.exhausted() {
            return Err(options("exhausted L-BFGS-B evaluator"));
        }
        self.evaluations += 1;
        let evaluated = self.f.value_and_gradient(x);
        self.ctx.err()?;
        let (value, gradient) = evaluated?;
        if value.is_nan() || value == f64::NEG_INFINITY {
            return Err(options("non-finite L-BFGS-B value"));
        }
        if value == f64::INFINITY {
            return Ok(value);
        }
        if gradient.len() != g.len() {
            return Err(options("L-BFGS-B gradient length"));
        }
        for (i, &v) in gradient.iter().enumerate() {
            self.ctx.tick(i)?;
            if !v.is_finite() {
                return Err(options("non-finite L-BFGS-B gradient"));
            }
            g[i] = v;
        }
        Ok(value)
    }
    fn at(&mut self, x: &[f64], d: &[f64], step: f64) -> Result<Trial, Error> {
        let mut t = Trial::scalar(step, f64::INFINITY, 0.0);
        for i in 0..x.len() {
            self.ctx.tick(i)?;
            self.trial_x[i] = clamp(x[i] + step * d[i], self.lo[i], self.hi[i]);
            if !self.trial_x[i].is_finite() {
                return Ok(t);
            }
        }
        let tx = std::mem::take(&mut self.trial_x);
        let mut tg = std::mem::take(&mut self.trial_g);
        let v = self.evaluate_into(&tx, &mut tg);
        self.trial_x = tx;
        self.trial_g = tg;
        t.value = v?;
        if t.value != f64::INFINITY {
            t.slope = dot_checked(&self.ctx, &self.trial_g, d)?;
        }
        Ok(t)
    }
    fn accept(&mut self) {
        self.best_x.copy_from_slice(&self.trial_x);
        self.best_g.copy_from_slice(&self.trial_g);
    }
    fn run(
        &mut self,
        x: &[f64],
        f0: f64,
        slope0: f64,
        d: &[f64],
        initial: f64,
    ) -> Result<(f64, bool), Error> {
        let mut maximum = f64::INFINITY;
        for (i, &di) in d.iter().enumerate() {
            self.ctx.tick(i)?;
            if di > 0.0 {
                maximum = fmath::min(maximum, (self.hi[i] - x[i]) / di);
            } else if di < 0.0 {
                maximum = fmath::min(maximum, (self.lo[i] - x[i]) / di);
            }
        }
        let mut alpha = fmath::min(initial, maximum);
        if !(alpha > 0.0) || alpha.is_nan() {
            return Ok((0.0, false));
        }
        let mut best_value = f64::INFINITY;
        let mut have_best = false;
        let sufficient = |t: &Trial| {
            t.value != f64::INFINITY && t.value <= f0 + WOLFE_DECREASE * t.step * slope0
        };
        let good = |t: &Trial| {
            sufficient(t) && (t.slope.abs() <= -WOLFE_CURVATURE * slope0 || t.step == maximum)
        };
        let mut keep = |s: &mut Self, t: &Trial| {
            if sufficient(t) && t.value < f0 && t.value < best_value {
                s.accept();
                best_value = t.value;
                have_best = true;
            }
        };
        let mut previous = Trial::scalar(0.0, f0, slope0);
        let mut low = Trial::default();
        let mut high = Trial::default();
        let mut zoom = false;
        let mut used = 0i64;
        while used < self.config.line_evaluations && !self.exhausted() {
            self.ctx.err()?;
            let t = self.at(x, d, alpha)?;
            used += 1;
            keep(self, &t);
            if !sufficient(&t) || (used > 1 && t.value >= previous.value) {
                (low, high, zoom) = (previous, t, true);
                break;
            }
            if good(&t) {
                self.accept();
                return Ok((t.value, true));
            }
            if t.slope >= 0.0 {
                (low, high, zoom) = (t, previous, true);
                break;
            }
            previous = t;
            if alpha == maximum {
                break;
            }
            let next = fmath::min(2.0 * alpha, maximum);
            if !(next > alpha) || next.is_infinite() {
                break;
            }
            alpha = next;
        }
        while zoom && used < self.config.line_evaluations && !self.exhausted() {
            self.ctx.err()?;
            alpha = interpolate_step(&low, &high);
            if !(alpha > 0.0) || alpha == low.step || alpha == high.step {
                break;
            }
            let t = self.at(x, d, alpha)?;
            used += 1;
            keep(self, &t);
            if !sufficient(&t) || t.value >= low.value {
                high = t;
                continue;
            }
            if good(&t) {
                self.accept();
                return Ok((t.value, true));
            }
            if t.slope * (high.step - low.step) >= 0.0 {
                high = low;
            }
            low = t;
            if (high.step - low.step).abs() <= 1e-14 * fmath::max(low.step.abs(), 1e-300) {
                break;
            }
        }
        Ok((best_value, have_best))
    }
}
#[derive(Debug, Clone, Copy)]
struct Breakpoint {
    time: f64,
    index: usize,
}
struct Model<'a> {
    ctx: Context<'a>,
    m: usize,
    count: usize,
    head: usize,
    theta: f64,
    s: Vec<Vec<f64>>,
    y: Vec<Vec<f64>>,
    matrix: Vec<f64>,
    factor: Lu,
    reduced_factor: Lu,
}
impl<'a> Model<'a> {
    fn new(ctx: Context<'a>, n: usize, m: usize) -> Self {
        Model {
            ctx,
            m,
            count: 0,
            head: 0,
            theta: 1.0,
            s: vec![vec![0.0; n]; m],
            y: vec![vec![0.0; n]; m],
            matrix: vec![0.0; 4 * m * m],
            factor: Lu::new(2 * m),
            reduced_factor: Lu::new(2 * m),
        }
    }
    fn slot(&self, i: usize) -> usize {
        (self.head + i) % self.m
    }
    fn reset(&mut self) {
        self.count = 0;
        self.head = 0;
        self.theta = 1.0;
        self.factor.n = 0;
        self.reduced_factor.n = 0;
    }
    fn add(&mut self, s: &[f64], y: &[f64]) -> Result<(), Error> {
        let ctx = self.ctx;
        let checked = |a: &[f64], b: &[f64]| -> Result<Option<f64>, Error> {
            match dot_checked(&ctx, a, b) {
                Ok(v) => Ok(Some(v)),
                Err(_) => ctx.err().map(|()| None),
            }
        };
        let Some(sy) = checked(s, y)? else {
            return Ok(());
        };
        let Some(ss) = checked(s, s)? else {
            return Ok(());
        };
        let Some(yy) = checked(y, y)? else {
            return Ok(());
        };
        if !(sy > 1e-10 * f64::sqrt(ss) * f64::sqrt(yy)) || !(yy > 0.0) || (yy / sy).is_infinite() {
            return Ok(());
        }
        let mut at = (self.head + self.count) % self.m;
        if self.count == self.m {
            at = self.head;
            self.head = (self.head + 1) % self.m;
        } else {
            self.count += 1;
        }
        self.s[at].copy_from_slice(s);
        self.y[at].copy_from_slice(y);
        Ok(())
    }
    fn build(&mut self) -> Result<(), Error> {
        self.theta = 1.0;
        if self.count == 0 {
            return self.ctx.err();
        }
        let ctx = self.ctx;
        let last = self.slot(self.count - 1);
        let sy = dot_checked(&ctx, &self.s[last], &self.y[last])?;
        let yy = dot_checked(&ctx, &self.y[last], &self.y[last])?;
        self.theta = yy / sy;
        if !(self.theta > 0.0) || self.theta.is_infinite() {
            return Err(Error::LbfgsbModel);
        }
        let count = self.count;
        let k = 2 * count;
        let mut a = std::mem::take(&mut self.matrix);
        a[..k * k].fill(0.0);
        let filled = self.fill(&mut a, count);
        let r = filled.and_then(|()| self.factor.factor(&ctx, &a[..k * k], k));
        self.matrix = a;
        r
    }
    fn fill(&self, a: &mut [f64], count: usize) -> Result<(), Error> {
        let ctx = self.ctx;
        let k = 2 * count;
        for i in 0..count {
            let si = self.slot(i);
            let diagonal = dot_checked(&ctx, &self.s[si], &self.y[si])?;
            a[i * k + i] = -diagonal;
            for j in 0..count {
                let sj = self.slot(j);
                if i > j {
                    let v = dot_checked(&ctx, &self.s[si], &self.y[sj])?;
                    a[(count + i) * k + j] = v;
                    a[j * k + count + i] = v;
                }
                let v = dot_checked(&ctx, &self.s[si], &self.s[sj])?;
                a[(count + i) * k + count + j] = self.theta * v;
            }
        }
        Ok(())
    }
    fn row(&self, index: usize, out: &mut [f64]) {
        for j in 0..self.count {
            let p = self.slot(j);
            out[j] = self.y[p][index];
            out[self.count + j] = self.theta * self.s[p][index];
        }
    }
    fn transpose(&self, v: &[f64], out: &mut [f64]) -> Result<(), Error> {
        for j in 0..self.count {
            let p = self.slot(j);
            out[j] = dot_checked(&self.ctx, &self.y[p], v)?;
            out[self.count + j] = self.theta * dot_checked(&self.ctx, &self.s[p], v)?;
        }
        Ok(())
    }
    fn product(&self, v: &[f64], out: &mut [f64]) -> Result<(), Error> {
        let k = 2 * self.count;
        let mut wt = vec![0.0; k];
        let mut sol = vec![0.0; k];
        let mut row = vec![0.0; k];
        self.transpose(v, &mut wt)?;
        self.factor.solve(&self.ctx, &wt, &mut sol)?;
        for i in 0..v.len() {
            self.ctx.tick(i)?;
            self.row(i, &mut row);
            let mut correction = 0.0;
            for j in 0..k {
                correction += row[j] * sol[j];
            }
            out[i] = self.theta * v[i] - correction;
            if !out[i].is_finite() {
                return Err(Error::LbfgsbModel);
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn cauchy(
        &self,
        x: &[f64],
        g: &[f64],
        lo: &[f64],
        hi: &[f64],
        xc: &mut [f64],
        d: &mut [f64],
        active: &mut [bool],
        breaks: &mut Vec<Breakpoint>,
    ) -> Result<(), Error> {
        let ctx = self.ctx;
        breaks.clear();
        xc.copy_from_slice(x);
        let mut gd = 0.0;
        let mut dd = 0.0;
        let mut moving = 0usize;
        for i in 0..x.len() {
            ctx.tick(i)?;
            active[i] =
                lo[i] == hi[i] || x[i] == lo[i] && g[i] >= 0.0 || x[i] == hi[i] && g[i] <= 0.0;
            d[i] = -g[i];
            if active[i] {
                d[i] = 0.0;
            }
            if d[i] != 0.0 {
                moving += 1;
                let bound = if d[i] < 0.0 { lo[i] } else { hi[i] };
                let t = (bound - x[i]) / d[i];
                if t != f64::INFINITY {
                    if t.is_nan() || t < 0.0 {
                        return Err(Error::LbfgsbModel);
                    }
                    breaks.push(Breakpoint { time: t, index: i });
                }
            }
            gd += g[i] * d[i];
            dd += d[i] * d[i];
        }
        if gd.is_infinite() || dd.is_infinite() {
            return Err(Error::LbfgsbModel);
        }
        breaks.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.index.cmp(&b.index)));
        ctx.err()?;
        let k = 2 * self.count;
        let mut p = vec![0.0; k];
        let mut c = vec![0.0; k];
        let mut sp = vec![0.0; k];
        let mut sc = vec![0.0; k];
        let mut row = vec![0.0; k];
        self.transpose(d, &mut p)?;
        let mut time = 0.0;
        let mut zd = 0.0;
        let mut at = 0usize;
        while dd > 0.0 {
            ctx.err()?;
            self.factor.solve(&ctx, &p, &mut sp)?;
            self.factor.solve(&ctx, &c, &mut sc)?;
            let mut first = gd + self.theta * zd;
            let mut second = self.theta * dd;
            for j in 0..k {
                first -= p[j] * sc[j];
                second -= p[j] * sp[j];
            }
            if !first.is_finite() || !(second > 0.0) || second.is_infinite() {
                return Err(Error::LbfgsbModel);
            }
            if first >= 0.0 {
                break;
            }
            let mut delta = -first / second;
            let next = breaks.get(at).map_or(f64::INFINITY, |b| b.time);
            if delta <= next - time {
                time += delta;
                break;
            }
            delta = next - time;
            for j in 0..k {
                c[j] += delta * p[j];
            }
            zd += delta * dd;
            time = next;
            while at < breaks.len() && breaks[at].time == time {
                if at & 1023 == 0 {
                    ctx.err()?;
                }
                let i = breaks[at].index;
                let di = d[i];
                let bound = if di < 0.0 { lo[i] } else { hi[i] };
                let zi = bound - x[i];
                self.row(i, &mut row);
                let correction = zi - time * di;
                for j in 0..k {
                    c[j] += correction * row[j];
                    p[j] -= di * row[j];
                }
                zd -= time * di * di;
                gd -= g[i] * di;
                dd -= di * di;
                xc[i] = bound;
                d[i] = 0.0;
                active[i] = true;
                moving -= 1;
                at += 1;
            }
            if moving == 0 {
                dd = 0.0;
            } else if dd < 0.0 {
                return Err(Error::LbfgsbModel);
            }
        }
        for i in 0..x.len() {
            ctx.tick(i)?;
            if !active[i] {
                xc[i] = clamp(x[i] + time * d[i], lo[i], hi[i]);
                active[i] = xc[i] == lo[i] || xc[i] == hi[i];
            }
            if !xc[i].is_finite() {
                return Err(Error::LbfgsbModel);
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn subspace(
        &mut self,
        x: &[f64],
        g: &[f64],
        lo: &[f64],
        hi: &[f64],
        xc: &mut [f64],
        z: &mut [f64],
        bz: &mut [f64],
        active: &[bool],
    ) -> Result<(), Error> {
        let ctx = self.ctx;
        for i in 0..x.len() {
            ctx.tick(i)?;
            z[i] = xc[i] - x[i];
        }
        self.product(z, bz)?;
        let k = 2 * self.count;
        let mut a = self.matrix[..k * k].to_vec();
        let mut rhs = vec![0.0; k];
        let mut sol = vec![0.0; k];
        let mut row = vec![0.0; k];
        for i in 0..x.len() {
            ctx.tick(i)?;
            z[i] = 0.0;
            if active[i] {
                continue;
            }
            let r = g[i] + bz[i];
            bz[i] = r;
            self.row(i, &mut row);
            for j in 0..k {
                rhs[j] += row[j] * r;
                for l in 0..k {
                    a[j * k + l] -= row[j] * row[l] / self.theta;
                }
            }
        }
        if k > 0 {
            self.reduced_factor.factor(&ctx, &a, k)?;
            self.reduced_factor.solve(&ctx, &rhs, &mut sol)?;
        }
        let mut alpha = 1.0;
        for i in 0..x.len() {
            ctx.tick(i)?;
            if active[i] {
                continue;
            }
            self.row(i, &mut row);
            let mut correction = 0.0;
            for j in 0..k {
                correction += row[j] * sol[j];
            }
            z[i] = -(bz[i] + correction / self.theta) / self.theta;
            if !z[i].is_finite() {
                return Err(Error::LbfgsbModel);
            }
            if z[i] > 0.0 {
                alpha = fmath::min(alpha, (hi[i] - xc[i]) / z[i]);
            } else if z[i] < 0.0 {
                alpha = fmath::min(alpha, (lo[i] - xc[i]) / z[i]);
            }
        }
        if !(alpha >= 0.0) {
            return Err(Error::LbfgsbModel);
        }
        for i in 0..xc.len() {
            ctx.tick(i)?;
            xc[i] = clamp(xc[i] + alpha * z[i], lo[i], hi[i]);
        }
        Ok(())
    }
}
struct Lu {
    data: Vec<f64>,
    pivot: Vec<usize>,
    n: usize,
}
impl Lu {
    fn new(n: usize) -> Self {
        Lu {
            data: vec![0.0; n * n],
            pivot: vec![0; n],
            n: 0,
        }
    }
    fn factor(&mut self, ctx: &Context<'_>, a: &[f64], n: usize) -> Result<(), Error> {
        self.n = n;
        let v = &mut self.data[..n * n];
        v.copy_from_slice(a);
        let mut scale = 0.0;
        for &x in v.iter() {
            if !x.is_finite() {
                return Err(Error::LbfgsbModel);
            }
            scale = fmath::max(scale, x.abs());
        }
        if n > 0 && !(scale > 0.0) {
            return Err(Error::LbfgsbModel);
        }
        for k in 0..n {
            ctx.err()?;
            let mut pivot = k;
            for i in k + 1..n {
                if v[i * n + k].abs() > v[pivot * n + k].abs() {
                    pivot = i;
                }
            }
            if v[pivot * n + k].abs() <= 64.0 * 2.220446049250313e-16 * scale {
                return Err(Error::LbfgsbModel);
            }
            self.pivot[k] = pivot;
            if pivot != k {
                for j in 0..n {
                    v.swap(k * n + j, pivot * n + j);
                }
            }
            for i in k + 1..n {
                v[i * n + k] /= v[k * n + k];
                for j in k + 1..n {
                    v[i * n + j] -= v[i * n + k] * v[k * n + j];
                }
            }
        }
        Ok(())
    }
    fn solve(&self, ctx: &Context<'_>, rhs: &[f64], out: &mut [f64]) -> Result<(), Error> {
        let n = self.n;
        if rhs.len() != n || out.len() != n {
            return Err(Error::LbfgsbModel);
        }
        out.copy_from_slice(rhs);
        for k in 0..n {
            out.swap(k, self.pivot[k]);
        }
        for i in 0..n {
            ctx.err()?;
            for j in 0..i {
                out[i] -= self.data[i * n + j] * out[j];
            }
        }
        for i in (0..n).rev() {
            for j in i + 1..n {
                out[i] -= self.data[i * n + j] * out[j];
            }
            out[i] /= self.data[i * n + i];
            if !out[i].is_finite() {
                return Err(Error::LbfgsbModel);
            }
        }
        Ok(())
    }
}
