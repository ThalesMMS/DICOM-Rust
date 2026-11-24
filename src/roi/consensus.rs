#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::{Cancellation, Error, ctx_err};
pub const ERR_CONSENSUS_INPUT: &str = "roi: invalid consensus input";
pub const ERR_UNDECIDED_LABEL: &str = "roi: no label available for undecided voxels";
pub const DEFAULT_STAPLE_ITERATIONS: i64 = 1000;
pub const STAPLE_SEED_ONLY: i64 = -1;
pub const DEFAULT_STAPLE_TERMINATION: f64 = 1e-5;
pub const MAX_CONSENSUS_VOXELS: u64 = 1 << 31;
pub const MAX_CONSENSUS_LABELS: i64 = 1024;
fn check_raters<T>(raters: &[Vec<T>]) -> Result<usize, Error> {
    if raters.len() < 2 {
        return Err(Error::wrap(
            ERR_CONSENSUS_INPUT,
            format_args!("{} raters, want at least 2", raters.len()),
        ));
    }
    let n = raters[0].len();
    if n == 0 {
        return Err(Error::wrap(ERR_CONSENSUS_INPUT, "empty image"));
    }
    for (i, r) in raters.iter().enumerate() {
        if r.len() != n {
            return Err(Error::wrap(
                ERR_CONSENSUS_INPUT,
                format_args!("rater {} has {} voxels, want {}", i, r.len(), n),
            ));
        }
    }
    if (n as u64).wrapping_mul(raters.len() as u64) > MAX_CONSENSUS_VOXELS {
        return Err(Error::wrap(
            ERR_CONSENSUS_INPUT,
            format_args!("{} raters × {} voxels exceed the budget", raters.len(), n),
        ));
    }
    Ok(n)
}
fn max_label(raters: &[Vec<u16>]) -> i64 {
    let mut m = 0i64;
    for r in raters {
        for &v in r {
            m = m.max(i64::from(v));
        }
    }
    m
}
pub fn label_voting(raters: &[Vec<u16>], undecided: i64) -> Result<Vec<i64>, Error> {
    let n = check_raters(raters)?;
    let total = (max_label(raters) + 1) as usize;
    let undecided = if undecided < 0 {
        total as i64
    } else {
        undecided
    };
    let mut votes = vec![0i64; total];
    let mut out = vec![0i64; n];
    for (voxel, slot) in out.iter_mut().enumerate() {
        votes.fill(0);
        for r in raters {
            votes[r[voxel] as usize] += 1;
        }
        let (mut winner, mut best) = (0i64, votes[0]);
        for (label, &count) in votes.iter().enumerate().skip(1) {
            if count > best {
                winner = label as i64;
                best = count;
            } else if count == best {
                winner = undecided;
            }
        }
        *slot = winner;
    }
    Ok(out)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MultiLabelStapleOptions {
    pub max_iterations: i64,
    pub termination: f64,
    pub undecided_label: i64,
    pub has_undecided_label: bool,
    pub priors: Option<Vec<f64>>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MultiLabelStapleResult {
    pub labels: Vec<i64>,
    pub undecided_label: i64,
    pub confusion_matrices: Vec<Vec<Vec<f64>>>,
    pub priors: Vec<f64>,
    pub iterations: i64,
    pub converged: bool,
}
fn builtin_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}
pub fn multi_label_staple(
    ctx: Option<&dyn Cancellation>,
    raters: &[Vec<u16>],
    options: &MultiLabelStapleOptions,
) -> Result<MultiLabelStapleResult, Error> {
    let n = check_raters(raters)?;
    let total_labels = max_label(raters) + 1;
    if total_labels > MAX_CONSENSUS_LABELS {
        return Err(Error::wrap(
            ERR_CONSENSUS_INPUT,
            format_args!("{total_labels} labels exceed {MAX_CONSENSUS_LABELS}"),
        ));
    }
    let total = total_labels as usize;
    let mut result = MultiLabelStapleResult {
        undecided_label: total_labels,
        ..MultiLabelStapleResult::default()
    };
    if options.has_undecided_label {
        result.undecided_label = options.undecided_label;
    } else if total_labels > i64::from(u16::MAX) {
        return Err(Error::sentinel(ERR_UNDECIDED_LABEL));
    }
    let max_iterations = match options.max_iterations {
        0 => DEFAULT_STAPLE_ITERATIONS,
        m if m < 0 => 0,
        m => m,
    };
    let mut termination = options.termination;
    if termination <= 0.0 {
        termination = DEFAULT_STAPLE_TERMINATION;
    }
    let voting = label_voting(raters, total_labels)?;
    let new_matrix = || vec![vec![0.0f64; total]; total + 1];
    let mut matrices: Vec<Vec<Vec<f64>>> = Vec::with_capacity(raters.len());
    let mut updated: Vec<Vec<Vec<f64>>> = Vec::with_capacity(raters.len());
    for rater in raters {
        let mut matrix = new_matrix();
        for (voxel, &vote) in voting.iter().enumerate() {
            if vote != total_labels {
                matrix[rater[voxel] as usize][vote as usize] += 1.0;
            }
        }
        for c in 0..total {
            let mut sum = 0.0f64;
            for row in &matrix {
                sum += row[c];
            }
            if sum > 0.0 {
                for row in matrix.iter_mut() {
                    row[c] /= sum;
                }
            }
        }
        matrices.push(matrix);
        updated.push(new_matrix());
    }
    let priors: Vec<f64> = match &options.priors {
        None => {
            let mut counts = vec![0.0f64; total];
            for rater in raters {
                for &v in rater {
                    counts[v as usize] += 1.0;
                }
            }
            let all = (n * raters.len()) as f64;
            counts.iter().map(|count| count / all).collect()
        }
        Some(priors) => {
            if priors.len() < total {
                return Err(Error::wrap(
                    ERR_CONSENSUS_INPUT,
                    format_args!("{} priors for {} labels", priors.len(), total),
                ));
            }
            priors[..total].to_vec()
        }
    };
    result.priors = priors.clone();
    let mut weights = vec![0.0f64; total];
    let estep = |weights: &mut [f64], matrices: &[Vec<Vec<f64>>], voxel: usize| {
        weights.copy_from_slice(&priors);
        for (k, rater) in raters.iter().enumerate() {
            let row = &matrices[k][rater[voxel] as usize];
            for (w, r) in weights.iter_mut().zip(row) {
                *w *= r;
            }
        }
    };
    let mut iteration = 0i64;
    while iteration < max_iterations {
        for matrix in updated.iter_mut() {
            for row in matrix.iter_mut() {
                row.fill(0.0);
            }
        }
        for voxel in 0..n {
            if voxel & 0xffff == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(err);
            }
            estep(&mut weights, &matrices, voxel);
            let mut sum = 0.0f64;
            for &w in &weights {
                sum += w;
            }
            if sum != 0.0 {
                for w in weights.iter_mut() {
                    *w /= sum;
                }
            }
            for (k, rater) in raters.iter().enumerate() {
                let row = &mut updated[k][rater[voxel] as usize];
                for (slot, &w) in row.iter_mut().zip(&weights) {
                    *slot += w;
                }
            }
        }
        let mut maximum_update = 0.0f64;
        for k in 0..updated.len() {
            for c in 0..total {
                let mut sum = 0.0f64;
                for row in &updated[k] {
                    sum += row[c];
                }
                if sum != 0.0 {
                    for row in updated[k].iter_mut() {
                        row[c] /= sum;
                    }
                }
            }
            for j in 0..updated[k].len() {
                for c in 0..total {
                    maximum_update =
                        builtin_max(maximum_update, (updated[k][j][c] - matrices[k][j][c]).abs());
                    matrices[k][j][c] = updated[k][j][c];
                }
            }
        }
        if maximum_update < termination {
            result.converged = true;
            break;
        }
        iteration += 1;
    }
    result.iterations = iteration;
    result.labels = vec![0i64; n];
    for voxel in 0..n {
        estep(&mut weights, &matrices, voxel);
        let (mut winner, mut best) = (result.undecided_label, 0.0f64);
        for (c, &w) in weights.iter().enumerate() {
            if w > best {
                winner = c as i64;
                best = w;
            } else if !(w < best) {
                winner = result.undecided_label;
            }
        }
        result.labels[voxel] = winner;
    }
    result.confusion_matrices = matrices;
    Ok(result)
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BinaryStapleOptions {
    pub confidence_weight: f64,
    pub max_iterations: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BinaryStapleResult {
    pub probability: Vec<f64>,
    pub sensitivity: Vec<f64>,
    pub specificity: Vec<f64>,
    pub prior: f64,
    pub iterations: i64,
    pub converged: bool,
}
impl BinaryStapleResult {
    pub fn mask(&self, threshold: f64) -> Vec<bool> {
        self.probability.iter().map(|&p| p >= threshold).collect()
    }
}
pub fn binary_staple(
    ctx: Option<&dyn Cancellation>,
    raters: &[Vec<bool>],
    options: BinaryStapleOptions,
) -> Result<BinaryStapleResult, Error> {
    let n = check_raters(raters)?;
    let mut weight = options.confidence_weight;
    if weight == 0.0 {
        weight = 1.0;
    }
    let mut max_iterations = options.max_iterations;
    if max_iterations <= 0 {
        max_iterations = DEFAULT_STAPLE_ITERATIONS;
    }
    const MIN_SQUARED_CHANGE: f64 = 1e-14;
    let k = raters.len();
    let mut w = vec![0.0f64; n];
    let mut prior = 0.0f64;
    for (voxel, value) in w.iter_mut().enumerate() {
        for r in raters {
            if r[voxel] {
                *value += 1.0;
            }
        }
        *value /= k as f64;
        prior += *value;
    }
    prior = prior / n as f64 * weight;
    let (mut p, mut q) = (vec![1.0f64; k], vec![1.0f64; k]);
    let (mut last_p, mut last_q) = (vec![-10.0f64; k], vec![-10.0f64; k]);
    let mut converged_flag = false;
    let mut iteration = 0i64;
    while iteration < max_iterations {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        for (i, r) in raters.iter().enumerate() {
            let (mut p_num, mut p_den, mut q_num, mut q_den) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for (voxel, &v) in r.iter().enumerate() {
                if v {
                    p_num += w[voxel];
                } else {
                    q_num += 1.0 - w[voxel];
                }
                p_den += w[voxel];
                q_den += 1.0 - w[voxel];
            }
            if p_den != 0.0 {
                p[i] = p_num / p_den;
            }
            if q_den != 0.0 {
                q[i] = q_num / q_den;
            }
        }
        for (voxel, value) in w.iter_mut().enumerate() {
            let (mut alpha, mut beta) = (1.0f64, 1.0f64);
            for (i, r) in raters.iter().enumerate() {
                if r[voxel] {
                    alpha *= p[i];
                    beta *= 1.0 - q[i];
                } else {
                    alpha *= 1.0 - p[i];
                    beta *= q[i];
                }
            }
            let denominator = prior * alpha + (1.0 - prior) * beta;
            *value = if denominator > 0.0 {
                prior * alpha / denominator
            } else {
                prior
            };
        }
        let mut converged = iteration != 0;
        for i in 0..k {
            if (p[i] - last_p[i]) * (p[i] - last_p[i]) > MIN_SQUARED_CHANGE
                || (q[i] - last_q[i]) * (q[i] - last_q[i]) > MIN_SQUARED_CHANGE
            {
                converged = false;
            }
            last_p[i] = p[i];
            last_q[i] = q[i];
        }
        if converged {
            converged_flag = true;
            iteration += 1;
            break;
        }
        iteration += 1;
    }
    Ok(BinaryStapleResult {
        probability: w,
        sensitivity: p,
        specificity: q,
        prior,
        iterations: iteration,
        converged: converged_flag,
    })
}
