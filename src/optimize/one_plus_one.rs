use super::{
    Counter, Error, Function, OptimizeResult, StopReason, default_to, default_to_int, options,
    scales_for,
};
use crate::fmath::{self, rand::Rand};
use crate::textfmt::float_v;
#[derive(Debug, Clone, Default)]
pub struct OnePlusOne {
    pub initial_radius: f64,
    pub growth_factor: f64,
    pub shrink_factor: f64,
    pub epsilon: f64,
    pub maximum_iterations: i64,
    pub scales: Option<Vec<f64>>,
    pub seed: i64,
}
impl OnePlusOne {
    pub fn minimize(&self, f: &mut dyn Function, x0: &[f64]) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        if n == 0 {
            return Err(options("no parameters"));
        }
        let scales = scales_for(self.scales.as_deref(), n)?;
        let growth = default_to(self.growth_factor, 1.05);
        let shrink = default_to(self.shrink_factor, fmath::pow(growth, -0.25));
        if growth <= 1.0 || shrink >= 1.0 {
            return Err(options(format!(
                "growth {} must exceed 1 and shrink {} stay below it",
                float_v(growth),
                float_v(shrink)
            )));
        }
        let epsilon = default_to(self.epsilon, 1.5e-4);
        let max_iterations = default_to_int(self.maximum_iterations, 100);
        let radius = default_to(self.initial_radius, 1.01);
        let mut random = Rand::new(self.seed);
        let mut c = Counter { f, count: 0 };
        let mut parent = x0.to_vec();
        let mut parent_value = c.value(&parent)?;
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            a[i * n + i] = radius;
        }
        let mut normal = vec![0.0; n];
        let mut step = vec![0.0; n];
        let mut child = vec![0.0; n];
        let mut stop = StopReason::MaximumIterations;
        let mut iterations = 0;
        for iteration in 0..max_iterations {
            iterations = iteration + 1;
            let mut norm2 = 0.0;
            for v in normal.iter_mut() {
                *v = random.norm_float64();
                norm2 += *v * *v;
            }
            for r in 0..n {
                let mut sum = 0.0;
                for k in 0..n {
                    sum += a[r * n + k] * normal[k];
                }
                step[r] = sum;
                child[r] = parent[r] + sum / scales[r];
            }
            let child_value = c.value(&child)?;
            let mut adjust = shrink;
            if child_value < parent_value {
                std::mem::swap(&mut parent, &mut child);
                parent_value = child_value;
                adjust = growth;
            }
            let mut frobenius = 0.0;
            for r in 0..n {
                for k in 0..n {
                    let v = a[r * n + k] / scales[r];
                    frobenius += v * v;
                }
            }
            if f64::sqrt(frobenius) <= epsilon {
                stop = StopReason::Radius;
                break;
            }
            if norm2 > 0.0 {
                let alpha = (adjust - 1.0) / norm2;
                for r in 0..n {
                    for k in 0..n {
                        a[r * n + k] += alpha * step[r] * normal[k];
                    }
                }
            }
        }
        Ok(OptimizeResult {
            x: parent,
            value: parent_value,
            iterations,
            evaluations: c.count,
            stop,
        })
    }
}
