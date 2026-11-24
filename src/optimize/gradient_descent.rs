use super::{Error, GradientFunction, OptimizeResult, StopReason, default_to, options, scales_for};
use crate::fmath;
use crate::textfmt::float_v;
pub trait StepScaleEstimator {
    fn step_scale(&self, step: &[f64]) -> Result<f64, Error>;
}
#[derive(Default)]
pub struct RegularStepGradientDescent {
    pub learning_rate: f64,
    pub relaxation_factor: f64,
    pub minimum_step_length: f64,
    pub gradient_magnitude_tolerance: f64,
    pub maximum_iterations: i64,
    pub scales: Option<Vec<f64>>,
    pub step_scale: Option<Box<dyn StepScaleEstimator>>,
    pub maximum_step_size: f64,
    pub estimate_learning_rate_each_iteration: bool,
    pub convergence_window: i64,
    pub minimum_convergence_value: f64,
}
impl RegularStepGradientDescent {
    pub fn minimize(
        &self,
        f: &mut dyn GradientFunction,
        x0: &[f64],
    ) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        if n == 0 {
            return Err(options("no parameters"));
        }
        let scales = scales_for(self.scales.as_deref(), n)?;
        let mut relaxation_factor = self.relaxation_factor;
        if relaxation_factor == 0.0 {
            relaxation_factor = 0.5;
        }
        if relaxation_factor < 0.0 || relaxation_factor >= 1.0 {
            return Err(options(format!(
                "relaxation factor {} outside [0, 1)",
                float_v(relaxation_factor)
            )));
        }
        if self.gradient_magnitude_tolerance < 0.0 {
            return Err(options("negative gradient magnitude tolerance"));
        }
        let mut learning_rate = default_to(self.learning_rate, 1.0);
        let minimum_step = default_to(self.minimum_step_length, 1e-4);
        let gradient_tolerance = default_to(self.gradient_magnitude_tolerance, 1e-4);
        let mut max_iterations = self.maximum_iterations;
        if max_iterations == 0 {
            max_iterations = 100;
        }
        let mut monitor = (self.convergence_window > 0)
            .then(|| WindowConvergenceMonitor::new(self.convergence_window));
        let minimum_convergence = default_to(self.minimum_convergence_value, 1e-8);
        let mut x = x0.to_vec();
        let mut previous = vec![0.0; n];
        let mut relaxation = 1.0;
        let mut result = OptimizeResult {
            x: Vec::new(),
            value: 0.0,
            iterations: 0,
            evaluations: 0,
            stop: StopReason::MaximumIterations,
        };
        let mut evaluations = 0;
        let mut iteration: i64 = 0;
        while max_iterations < 0 || iteration < max_iterations {
            let evaluated = f.value_and_gradient(&x);
            evaluations += 1;
            let (value, gradient) = evaluated?;
            if gradient.len() != n {
                return Err(options(format!(
                    "{} gradient components for {n} parameters",
                    gradient.len()
                )));
            }
            result.value = value;
            result.iterations = iteration;
            if let Some(m) = monitor.as_mut() {
                m.add(value);
                if m.value() <= minimum_convergence {
                    result.stop = StopReason::ConvergenceWindow;
                    break;
                }
            }
            let mut scaled = vec![0.0; n];
            let mut magnitude = 0.0;
            let mut direction = 0.0;
            for i in 0..n {
                scaled[i] = gradient[i] / scales[i];
                magnitude += scaled[i] * scaled[i];
                direction += scaled[i] * previous[i];
            }
            magnitude = f64::sqrt(magnitude);
            if magnitude < gradient_tolerance {
                result.stop = StopReason::GradientMagnitude;
                break;
            }
            if direction < 0.0 {
                relaxation *= relaxation_factor;
            }
            let mut step = relaxation * learning_rate;
            if step < minimum_step {
                result.stop = StopReason::MinimumStepLength;
                break;
            }
            if let Some(estimator) = &self.step_scale
                && self.maximum_step_size > 0.0
                && (iteration == 0 || self.estimate_learning_rate_each_iteration)
            {
                let scale = estimator.step_scale(&scaled)?;
                learning_rate = 1.0;
                if scale > 0.0 {
                    learning_rate = self.maximum_step_size / scale;
                }
                learning_rate *= magnitude;
                step = relaxation * learning_rate;
            }
            for i in 0..n {
                x[i] -= step * scaled[i] / magnitude;
            }
            previous = scaled;
            result.iterations = iteration + 1;
            iteration += 1;
        }
        result.x = x;
        result.evaluations = evaluations;
        Ok(result)
    }
}
#[derive(Debug, Clone)]
pub struct WindowConvergenceMonitor {
    window: i64,
    values: Vec<f64>,
    total: f64,
}
impl WindowConvergenceMonitor {
    pub fn new(window: i64) -> Self {
        WindowConvergenceMonitor {
            window: window.max(2),
            values: Vec::new(),
            total: 0.0,
        }
    }
    pub fn add(&mut self, value: f64) {
        self.values.push(value);
        if self.values.len() as i64 > self.window {
            self.values.remove(0);
        }
        self.total += value.abs();
    }
    pub fn value(&self) -> f64 {
        if (self.values.len() as i64) < self.window || self.total == 0.0 {
            return f64::INFINITY;
        }
        let mut numerator = [0.0f64; 2];
        let mut denominator = [0.0f64; 2];
        for (n, &v) in self.values.iter().enumerate() {
            let mut t = n as f64 / (self.window - 1) as f64;
            t = fmath::min(t, 0.9999);
            let basis = [1.0 - t, t];
            let w2 = basis[0] * basis[0] + basis[1] * basis[1];
            let z = v / self.total;
            for k in 0..2 {
                numerator[k] += basis[k] * basis[k] * basis[k] * z / w2;
                denominator[k] += basis[k] * basis[k];
            }
        }
        let mut control = [0.0f64; 2];
        for k in 0..2 {
            if denominator[k] > 1e-300 {
                control[k] = numerator[k] / denominator[k];
            }
        }
        control[0] - control[1]
    }
}
