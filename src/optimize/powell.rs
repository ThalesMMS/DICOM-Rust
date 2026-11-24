use super::{
    Counter, Error, Function, OptimizeResult, StopReason, default_to, default_to_int, options,
    scales_for, sq,
};
use crate::fmath;
#[derive(Debug, Clone, Default)]
pub struct Powell {
    pub step_length: f64,
    pub step_tolerance: f64,
    pub value_tolerance: f64,
    pub maximum_iterations: i64,
    pub maximum_line_iterations: i64,
    pub scales: Option<Vec<f64>>,
}
impl Powell {
    pub fn minimize(&self, f: &mut dyn Function, x0: &[f64]) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        if n == 0 {
            return Err(options("no parameters"));
        }
        let scales = scales_for(self.scales.as_deref(), n)?;
        let step = default_to(self.step_length, 1.0);
        let step_tolerance = default_to(self.step_tolerance, 1e-6);
        let value_tolerance = default_to(self.value_tolerance, 1e-6);
        let max_iterations = default_to_int(self.maximum_iterations, 100);
        let max_line = default_to_int(self.maximum_line_iterations, 100);
        let mut c = Counter { f, count: 0 };
        let to_x = |u: &[f64]| -> Vec<f64> { (0..n).map(|i| u[i] / scales[i]).collect() };
        let mut value = |u: &[f64]| -> Result<f64, Error> { c.value(&to_x(u)) };
        let mut u: Vec<f64> = (0..n).map(|i| x0[i] * scales[i]).collect();
        let mut current = value(&u)?;
        let mut directions: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                let mut d = vec![0.0; n];
                d[i] = 1.0;
                d
            })
            .collect();
        let mut stop = StopReason::MaximumIterations;
        let mut iterations = 0;
        for iteration in 0..max_iterations {
            iterations = iteration + 1;
            let start = u.clone();
            let start_value = current;
            let mut largest_decrease = 0.0;
            let mut largest_index = 0;
            for (i, direction) in directions.iter().enumerate() {
                let before = current;
                (u, current) = line_minimize(
                    &mut value,
                    u,
                    current,
                    direction,
                    step,
                    step_tolerance,
                    max_line,
                )?;
                let decrease = before - current;
                if decrease > largest_decrease {
                    largest_decrease = decrease;
                    largest_index = i;
                }
            }
            if 2.0 * (start_value - current)
                <= value_tolerance * (start_value.abs() + current.abs()) + 1e-20
            {
                stop = StopReason::ValueTolerance;
                break;
            }
            let displacement: Vec<f64> = (0..n).map(|i| u[i] - start[i]).collect();
            let extrapolated: Vec<f64> = (0..n).map(|i| 2.0 * u[i] - start[i]).collect();
            let extrapolated_value = value(&extrapolated)?;
            if extrapolated_value < start_value {
                let mut t = 2.0
                    * (start_value - 2.0 * current + extrapolated_value)
                    * sq(start_value - current - largest_decrease);
                t -= largest_decrease * sq(start_value - extrapolated_value);
                if t < 0.0 {
                    (u, current) = line_minimize(
                        &mut value,
                        u,
                        current,
                        &displacement,
                        step,
                        step_tolerance,
                        max_line,
                    )?;
                    directions[largest_index] = directions[n - 1].clone();
                    directions[n - 1] = displacement;
                }
            }
        }
        drop(value);
        Ok(OptimizeResult {
            x: to_x(&u),
            value: current,
            iterations,
            evaluations: c.count,
            stop,
        })
    }
}
fn line_minimize(
    value: &mut dyn FnMut(&[f64]) -> Result<f64, Error>,
    u: Vec<f64>,
    current: f64,
    direction: &[f64],
    step: f64,
    tolerance: f64,
    max_iterations: i64,
) -> Result<(Vec<f64>, f64), Error> {
    let mut at = |t: f64| -> Result<f64, Error> {
        let point: Vec<f64> = (0..u.len()).map(|i| u[i] + t * direction[i]).collect();
        value(&point)
    };
    let (a, b, c, fb) = bracket(&mut at, current, step)?;
    let (t, ft) = brent(&mut at, a, b, c, fb, tolerance, max_iterations)?;
    if ft >= current {
        return Ok((u, current));
    }
    let moved = (0..u.len()).map(|i| u[i] + t * direction[i]).collect();
    Ok((moved, ft))
}
const GOLDEN: f64 = 1.618033988749895;
fn bracket(
    f: &mut dyn FnMut(f64) -> Result<f64, Error>,
    f0: f64,
    step: f64,
) -> Result<(f64, f64, f64, f64), Error> {
    let (mut a, mut fa) = (0.0, f0);
    let mut b = step;
    let mut fb = f(b)?;
    if fb > fa {
        (a, b, fa, fb) = (b, a, fb, fa);
    }
    let mut c = b + GOLDEN * (b - a);
    let mut fc = f(c)?;
    let mut i = 0;
    while fc < fb && i < 100 {
        (a, b, fa, fb) = (b, c, fb, fc);
        c = b + GOLDEN * (b - a);
        fc = f(c)?;
        i += 1;
    }
    let _ = fa;
    Ok((a, b, c, fb))
}
fn brent(
    f: &mut dyn FnMut(f64) -> Result<f64, Error>,
    a: f64,
    b: f64,
    c: f64,
    fb: f64,
    tolerance: f64,
    max_iterations: i64,
) -> Result<(f64, f64), Error> {
    const CGOLD: f64 = 0.3819660112501051;
    let mut low = fmath::min(a, c);
    let mut high = fmath::max(a, c);
    let (mut x, mut w, mut v) = (b, b, b);
    let (mut fx, mut fw, mut fv) = (fb, fb, fb);
    let mut d = 0.0f64;
    let mut e = 0.0f64;
    for _ in 0..max_iterations.max(0) {
        let middle = (low + high) / 2.0;
        let tol1 = tolerance * x.abs() + 1e-12;
        let tol2 = 2.0 * tol1;
        if (x - middle).abs() <= tol2 - (high - low) / 2.0 {
            break;
        }
        let mut parabolic = false;
        if e.abs() > tol1 {
            let r = (x - w) * (fx - fv);
            let mut q = (x - v) * (fx - fw);
            let mut p = (x - v) * q - (x - w) * r;
            q = 2.0 * (q - r);
            if q > 0.0 {
                p = -p;
            }
            q = q.abs();
            if p.abs() < (q * e / 2.0).abs() && p > q * (low - x) && p < q * (high - x) {
                e = d;
                d = p / q;
                let u = x + d;
                if u - low < tol2 || high - u < tol2 {
                    d = tol1.copysign(middle - x);
                }
                parabolic = true;
            }
        }
        if !parabolic {
            e = if x >= middle { low - x } else { high - x };
            d = CGOLD * e;
        }
        let mut u = x + d;
        if d.abs() < tol1 {
            u = x + tol1.copysign(d);
        }
        let fu = f(u)?;
        if fu <= fx {
            if u >= x {
                low = x;
            } else {
                high = x;
            }
            (v, w, x) = (w, x, u);
            (fv, fw, fx) = (fw, fx, fu);
        } else {
            if u < x {
                low = u;
            } else {
                high = u;
            }
            if fu <= fw || w == x {
                (v, w, fv, fw) = (w, u, fw, fu);
            } else if fu <= fv || v == x || v == w {
                v = u;
                fv = fu;
            }
        }
    }
    Ok((x, fx))
}
