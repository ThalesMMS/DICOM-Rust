use super::{
    Counter, Error, Function, OptimizeResult, StopReason, default_to, default_to_int, options,
    scales_for,
};
use crate::fmath::{self, rand::Rand};
#[derive(Debug, Clone, Default)]
pub struct NelderMead {
    pub initial_simplex_delta: Option<Vec<f64>>,
    pub parameters_tolerance: f64,
    pub function_tolerance: f64,
    pub maximum_iterations: i64,
    pub scales: Option<Vec<f64>>,
    pub restarts: bool,
    pub seed: i64,
}
#[derive(Debug, Clone)]
struct Vertex {
    x: Vec<f64>,
    value: f64,
}
struct SearchResult {
    vertex: Vertex,
    converged: bool,
}
impl NelderMead {
    pub fn minimize(&self, f: &mut dyn Function, x0: &[f64]) -> Result<OptimizeResult, Error> {
        let n = x0.len();
        if n == 0 {
            return Err(options("no parameters"));
        }
        if let Some(delta) = &self.initial_simplex_delta
            && delta.len() != n
        {
            return Err(options(format!(
                "{} simplex deltas for {n} parameters",
                delta.len()
            )));
        }
        if self.parameters_tolerance < 0.0 || self.function_tolerance < 0.0 {
            return Err(options("negative tolerance"));
        }
        let scales = scales_for(self.scales.as_deref(), n)?;
        let x_tolerance = default_to(self.parameters_tolerance, 1e-8);
        let f_tolerance = default_to(self.function_tolerance, 1e-4);
        let max_iterations = default_to_int(self.maximum_iterations, 500);
        let mut scaled = super::Func(|u: &[f64]| -> Result<f64, Error> {
            let x: Vec<f64> = (0..n).map(|i| u[i] / scales[i]).collect();
            f.value(&x)
        });
        let mut c = Counter {
            f: &mut scaled,
            count: 0,
        };
        let mut u = vec![0.0; n];
        let mut delta = vec![0.0; n];
        for i in 0..n {
            u[i] = x0[i] * scales[i];
            delta[i] = match &self.initial_simplex_delta {
                Some(d) => d[i],
                None if u[i].abs() > 0.00025 => 0.05 * u[i],
                None => 0.00025,
            };
        }
        let mut best = search(&mut c, &u, &delta, x_tolerance, f_tolerance, max_iterations)?;
        if self.restarts {
            let mut random = Rand::new(self.seed);
            let mut round = 1i64;
            while c.count < max_iterations {
                let mut sign = 1.0;
                if random.intn(2) == Some(0) {
                    sign = -1.0;
                }
                for d in delta.iter_mut() {
                    *d *= sign / fmath::pow(2.0, round as f64);
                }
                let budget = max_iterations - c.count;
                let next = search(
                    &mut c,
                    &best.vertex.x,
                    &delta,
                    x_tolerance,
                    f_tolerance,
                    budget,
                )?;
                let mut converged = (best.vertex.value - next.vertex.value).abs() < f_tolerance;
                for i in 0..next.vertex.x.len() {
                    converged =
                        converged && (best.vertex.x[i] - next.vertex.x[i]).abs() < x_tolerance;
                }
                if next.vertex.value < best.vertex.value {
                    best = next;
                }
                if converged {
                    break;
                }
                round += 1;
            }
        }
        let count = c.count;
        let x = (0..n).map(|i| best.vertex.x[i] / scales[i]).collect();
        Ok(OptimizeResult {
            x,
            value: best.vertex.value,
            iterations: count,
            evaluations: count,
            stop: if best.converged {
                StopReason::ParameterTolerance
            } else {
                StopReason::MaximumIterations
            },
        })
    }
}
fn search(
    c: &mut Counter<'_>,
    u: &[f64],
    delta: &[f64],
    x_tolerance: f64,
    f_tolerance: f64,
    budget: i64,
) -> Result<SearchResult, Error> {
    let n = u.len();
    let start = c.count;
    let evaluate = |c: &mut Counter<'_>, x: Vec<f64>| -> Result<Vertex, Error> {
        let value = c.value(&x)?;
        Ok(Vertex { x, value })
    };
    let mut simplex = Vec::with_capacity(n + 1);
    simplex.push(evaluate(c, u.to_vec())?);
    for i in 0..n {
        let mut x = u.to_vec();
        x[i] += delta[i];
        simplex.push(evaluate(c, x)?);
    }
    let point = |centroid: &[f64], toward: &[f64], t: f64| -> Vec<f64> {
        (0..n)
            .map(|i| centroid[i] + t * (toward[i] - centroid[i]))
            .collect()
    };
    let by_value = |a: &Vertex, b: &Vertex| a.value < b.value;
    stable_sort_by_less(&mut simplex, by_value);
    while c.count - start < budget {
        if diameter(&simplex) < x_tolerance && simplex[n].value - simplex[0].value < f_tolerance {
            return Ok(SearchResult {
                vertex: simplex.swap_remove(0),
                converged: true,
            });
        }
        let mut centroid = vec![0.0; n];
        for v in &simplex[..n] {
            for i in 0..n {
                centroid[i] += v.x[i] / n as f64;
            }
        }
        let worst = simplex[n].clone();
        let reflected = evaluate(c, point(&centroid, &worst.x, -1.0))?;
        if reflected.value < simplex[0].value {
            let expanded = evaluate(c, point(&centroid, &worst.x, -2.0))?;
            simplex[n] = if expanded.value < reflected.value {
                expanded
            } else {
                reflected
            };
        } else if reflected.value < simplex[n - 1].value {
            simplex[n] = reflected;
        } else {
            let toward = if reflected.value < worst.value {
                reflected
            } else {
                worst
            };
            let contracted = evaluate(c, point(&centroid, &toward.x, 0.5))?;
            if contracted.value < toward.value {
                simplex[n] = contracted;
            } else {
                for i in 1..=n {
                    let x = point(&simplex[0].x, &simplex[i].x, 0.5);
                    simplex[i] = evaluate(c, x)?;
                }
            }
        }
        stable_sort_by_less(&mut simplex, by_value);
    }
    Ok(SearchResult {
        vertex: simplex.swap_remove(0),
        converged: false,
    })
}
fn diameter(simplex: &[Vertex]) -> f64 {
    let mut out = 0.0;
    for i in 0..simplex.len().saturating_sub(1) {
        for k in 0..simplex[i].x.len() {
            out = fmath::max(out, (simplex[i].x[k] - simplex[i + 1].x[k]).abs());
        }
    }
    out
}
fn stable_sort_by_less<T>(v: &mut [T], less: impl Fn(&T, &T) -> bool) {
    let less = |v: &[T], i: usize, j: usize| less(&v[i], &v[j]);
    let n = v.len();
    let mut block = 20;
    let (mut a, mut b) = (0, block);
    while b <= n {
        insertion_sort(v, a, b, &less);
        a = b;
        b += block;
    }
    insertion_sort(v, a, n, &less);
    while block < n {
        a = 0;
        b = 2 * block;
        while b <= n {
            sym_merge(v, a, a + block, b, &less);
            a = b;
            b += 2 * block;
        }
        let m = a + block;
        if m < n {
            sym_merge(v, a, m, n, &less);
        }
        block *= 2;
    }
}
fn insertion_sort<T>(v: &mut [T], a: usize, b: usize, less: &impl Fn(&[T], usize, usize) -> bool) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && less(v, j, j - 1) {
            v.swap(j, j - 1);
            j -= 1;
        }
    }
}
fn sym_merge<T>(
    v: &mut [T],
    a: usize,
    m: usize,
    b: usize,
    less: &impl Fn(&[T], usize, usize) -> bool,
) {
    if m - a == 1 {
        let (mut i, mut j) = (m, b);
        while i < j {
            let h = (i + j) / 2;
            if less(v, h, a) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        for k in a..i - 1 {
            v.swap(k, k + 1);
        }
        return;
    }
    if b - m == 1 {
        let (mut i, mut j) = (a, m);
        while i < j {
            let h = (i + j) / 2;
            if !less(v, m, h) {
                i = h + 1;
            } else {
                j = h;
            }
        }
        let mut k = m;
        while k > i {
            v.swap(k, k - 1);
            k -= 1;
        }
        return;
    }
    let mid = (a + b) / 2;
    let n = mid + m;
    let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
    let p = n - 1;
    while start < r {
        let c = (start + r) / 2;
        if !less(v, p - c, c) {
            start = c + 1;
        } else {
            r = c;
        }
    }
    let end = n - start;
    if start < m && m < end {
        rotate(v, start, m, end);
    }
    if a < start && start < mid {
        sym_merge(v, a, start, mid, less);
    }
    if mid < end && end < b {
        sym_merge(v, mid, end, b, less);
    }
}
fn rotate<T>(v: &mut [T], a: usize, m: usize, b: usize) {
    let mut i = m - a;
    let mut j = b - m;
    while i != j {
        if i > j {
            swap_range(v, m - i, m, j);
            i -= j;
        } else {
            swap_range(v, m - i, m + j - i, i);
            j -= i;
        }
    }
    swap_range(v, m - i, m, i);
}
fn swap_range<T>(v: &mut [T], a: usize, b: usize, n: usize) {
    for i in 0..n {
        v.swap(a + i, b + i);
    }
}
