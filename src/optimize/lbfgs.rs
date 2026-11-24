use super::{
    DEFAULT_VALUE_TOLERANCE, Error, GradientFunction, OptimizeResult, StopReason, axpy, default_to,
    default_to_int, dot, max_abs, options,
};
use crate::fmath;
pub(super) const WOLFE_DECREASE: f64 = 1e-4;
pub(super) const WOLFE_CURVATURE: f64 = 0.9;
#[derive(Debug, Clone, Default)]
pub struct Lbfgs {
    pub memory: i64,
    pub maximum_iterations: i64,
    pub maximum_evaluations: i64,
    pub gradient_tolerance: f64,
    pub value_tolerance: f64,
    pub initial_step_length: f64,
    pub maximum_line_search_evaluations: i64,
}
impl Lbfgs {
    pub fn minimize(
        &self,
        f: &mut dyn GradientFunction,
        x0: &[f64],
    ) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        if n == 0 {
            return Err(options("no parameters"));
        }
        if self.memory < 0
            || self.gradient_tolerance < 0.0
            || self.value_tolerance < 0.0
            || self.initial_step_length < 0.0
            || self.maximum_line_search_evaluations < 0
            || self.maximum_evaluations < 0
        {
            return Err(options("negative L-BFGS option"));
        }
        let memory = default_to_int(self.memory, 5) as usize;
        let mut max_iterations = self.maximum_iterations;
        if max_iterations == 0 {
            max_iterations = 100;
        }
        let gradient_tolerance = default_to(self.gradient_tolerance, 1e-5);
        let value_tolerance = default_to(self.value_tolerance, DEFAULT_VALUE_TOLERANCE);
        let line_evaluations = default_to_int(self.maximum_line_search_evaluations, 20);
        let mut search = LineSearch {
            f,
            n,
            max_evaluations: line_evaluations,
            budget: self.maximum_evaluations,
            evaluations: 0,
        };
        let mut x = x0.to_vec();
        let (mut value, mut gradient) = search.evaluate(&x)?;
        if value.is_infinite() && value > 0.0 || value.is_nan() {
            return Err(options("the starting point has no finite value"));
        }
        let mut result = OptimizeResult {
            x: x.clone(),
            value,
            iterations: 0,
            evaluations: 0,
            stop: StopReason::MaximumIterations,
        };
        let mut s_history: Vec<Vec<f64>> = Vec::new();
        let mut y_history: Vec<Vec<f64>> = Vec::new();
        let mut rho: Vec<f64> = Vec::new();
        let mut direction = vec![0.0; n];
        let mut alpha = vec![0.0; memory];
        let mut iteration: i64 = 0;
        while max_iterations < 0 || iteration < max_iterations {
            if max_abs(&gradient) <= gradient_tolerance {
                result.stop = StopReason::GradientMagnitude;
                break;
            }
            direction.copy_from_slice(&gradient);
            for i in (0..s_history.len()).rev() {
                alpha[i] = rho[i] * dot(&s_history[i], &direction);
                axpy(-alpha[i], &y_history[i], &mut direction);
            }
            if let Some(k) = s_history.len().checked_sub(1) {
                let gamma = dot(&s_history[k], &y_history[k]) / dot(&y_history[k], &y_history[k]);
                for d in direction.iter_mut() {
                    *d *= gamma;
                }
            }
            for i in 0..s_history.len() {
                let beta = rho[i] * dot(&y_history[i], &direction);
                axpy(alpha[i] - beta, &s_history[i], &mut direction);
            }
            for d in direction.iter_mut() {
                *d = -*d;
            }
            let mut slope = dot(&gradient, &direction);
            if !(slope < 0.0) {
                s_history.clear();
                y_history.clear();
                rho.clear();
                for i in 0..n {
                    direction[i] = -gradient[i];
                }
                slope = -dot(&gradient, &gradient);
            }
            let mut step = 1.0;
            if s_history.is_empty() {
                if self.initial_step_length > 0.0 {
                    step = self.initial_step_length / max_abs(&direction);
                } else {
                    step = 1.0 / f64::sqrt(-slope);
                }
            }
            let found = search.run(&x, value, slope, &direction, step)?;
            let Some(best) = found else {
                result.stop = if search.exhausted() {
                    StopReason::MaximumEvaluations
                } else {
                    StopReason::LineSearch
                };
                break;
            };
            let next_gradient = best.gradient;
            let mut s = vec![0.0; n];
            let mut y = vec![0.0; n];
            for i in 0..n {
                s[i] = best.x[i] - x[i];
                y[i] = next_gradient[i] - gradient[i];
            }
            let previous = value;
            x = best.x;
            value = best.value;
            gradient = next_gradient;
            result.x = x.clone();
            result.value = value;
            result.iterations = iteration + 1;
            let sy = dot(&s, &y);
            if sy > 1e-10 * f64::sqrt(dot(&s, &s) * dot(&y, &y)) {
                if s_history.len() == memory {
                    s_history.remove(0);
                    y_history.remove(0);
                    rho.remove(0);
                }
                s_history.push(s);
                y_history.push(y);
                rho.push(1.0 / sy);
            }
            if previous - value
                <= value_tolerance * fmath::max(fmath::max(previous.abs(), value.abs()), 1.0)
            {
                result.stop = StopReason::ValueTolerance;
                break;
            }
            if search.exhausted() {
                result.stop = StopReason::MaximumEvaluations;
                break;
            }
            iteration += 1;
        }
        result.evaluations = search.evaluations;
        Ok(result)
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct Trial {
    pub(super) step: f64,
    pub(super) value: f64,
    pub(super) slope: f64,
    x: Vec<f64>,
    gradient: Vec<f64>,
}
impl Trial {
    pub(super) fn scalar(step: f64, value: f64, slope: f64) -> Self {
        Trial {
            step,
            value,
            slope,
            ..Trial::default()
        }
    }
}
struct LineSearch<'f> {
    f: &'f mut dyn GradientFunction,
    n: usize,
    max_evaluations: i64,
    budget: i64,
    evaluations: i64,
}
impl LineSearch<'_> {
    fn exhausted(&self) -> bool {
        self.budget > 0 && self.evaluations >= self.budget
    }
    fn evaluate(&mut self, x: &[f64]) -> Result<(f64, Vec<f64>), Error> {
        self.evaluations += 1;
        let (value, gradient) = self.f.value_and_gradient(x)?;
        if value.is_nan() || value == f64::INFINITY {
            return Ok((f64::INFINITY, Vec::new()));
        }
        if gradient.len() != self.n {
            return Err(options(format!(
                "{} gradient components for {} parameters",
                gradient.len(),
                self.n
            )));
        }
        Ok((value, gradient))
    }
    fn at(&mut self, x: &[f64], direction: &[f64], alpha: f64) -> Result<Trial, Error> {
        let point: Vec<f64> = (0..self.n).map(|i| x[i] + alpha * direction[i]).collect();
        let (value, gradient) = self.evaluate(&point)?;
        let slope = if gradient.is_empty() {
            0.0
        } else {
            dot(&gradient, direction)
        };
        Ok(Trial {
            step: alpha,
            value,
            slope,
            x: point,
            gradient,
        })
    }
    fn run(
        &mut self,
        x: &[f64],
        f0: f64,
        slope0: f64,
        direction: &[f64],
        step: f64,
    ) -> Result<Option<Trial>, Error> {
        let w = Wolfe { f0, slope0 };
        let mut best: Option<Trial> = None;
        let mut previous = Trial::scalar(0.0, f0, slope0);
        let mut used = 0i64;
        let mut alpha = step;
        loop {
            if used >= self.max_evaluations || self.exhausted() {
                return Ok(best);
            }
            let current = self.at(x, direction, alpha)?;
            used += 1;
            w.keep(&current, &mut best);
            if !w.sufficient(&current) || (used > 1 && current.value >= previous.value) {
                return self.zoom(x, direction, previous, current, used, &w, best);
            }
            if w.curvature(&current) {
                return Ok(Some(current));
            }
            if current.slope >= 0.0 {
                return self.zoom(x, direction, current, previous, used, &w, best);
            }
            previous = current;
            alpha *= 2.0;
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn zoom(
        &mut self,
        x: &[f64],
        direction: &[f64],
        mut lo: Trial,
        mut hi: Trial,
        mut used: i64,
        w: &Wolfe,
        mut best: Option<Trial>,
    ) -> Result<Option<Trial>, Error> {
        while used < self.max_evaluations && !self.exhausted() {
            let alpha = interpolate_step(&lo, &hi);
            let current = self.at(x, direction, alpha)?;
            used += 1;
            w.keep(&current, &mut best);
            if !w.sufficient(&current) || current.value >= lo.value {
                hi = current;
                continue;
            }
            if w.curvature(&current) {
                return Ok(Some(current));
            }
            if current.slope * (hi.step - lo.step) >= 0.0 {
                hi = lo;
            }
            lo = current;
            if (hi.step - lo.step).abs() <= 1e-12 * fmath::max(lo.step.abs(), 1e-300) {
                break;
            }
        }
        Ok(best)
    }
}
struct Wolfe {
    f0: f64,
    slope0: f64,
}
impl Wolfe {
    fn sufficient(&self, t: &Trial) -> bool {
        t.value != f64::INFINITY && t.value <= self.f0 + WOLFE_DECREASE * t.step * self.slope0
    }
    fn curvature(&self, t: &Trial) -> bool {
        t.slope.abs() <= -WOLFE_CURVATURE * self.slope0
    }
    fn keep(&self, t: &Trial, best: &mut Option<Trial>) {
        if self.sufficient(t)
            && t.value < self.f0
            && best.as_ref().is_none_or(|b| t.value < b.value)
        {
            *best = Some(t.clone());
        }
    }
}
pub(super) fn interpolate_step(lo: &Trial, hi: &Trial) -> f64 {
    let a = fmath::min(lo.step, hi.step);
    let b = fmath::max(lo.step, hi.step);
    let margin = 0.1 * (b - a);
    let mid = (lo.step + hi.step) / 2.0;
    if hi.value == f64::INFINITY || lo.value == f64::INFINITY {
        return mid;
    }
    let d1 = lo.slope + hi.slope - 3.0 * (lo.value - hi.value) / (lo.step - hi.step);
    let radicand = d1 * d1 - lo.slope * hi.slope;
    if radicand < 0.0 {
        return mid;
    }
    let d2 = f64::sqrt(radicand).copysign(hi.step - lo.step);
    let alpha =
        hi.step - (hi.step - lo.step) * (hi.slope + d2 - d1) / (hi.slope - lo.slope + 2.0 * d2);
    if alpha.is_nan() || alpha.is_infinite() {
        return mid;
    }
    fmath::min(fmath::max(alpha, a + margin), b - margin)
}
