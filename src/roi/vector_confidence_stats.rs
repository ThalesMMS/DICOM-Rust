#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::Error;
use super::vector_confidence::{
    ERR_VECTOR_CONFIDENCE_COVARIANCE, ERR_VECTOR_CONFIDENCE_NUMERIC, Ledger,
    VECTOR_COVARIANCE_POPULATION, VECTOR_COVARIANCE_RIDGE, VectorConfidenceParams, finite,
};
use crate::fmath;
#[derive(Debug, Default)]
pub(crate) struct VectorConfidenceModel {
    pub c: usize,
    pub samples: i64,
    pub mean: Vec<f64>,
    pub m2: Vec<f64>,
    pub cov: Vec<f64>,
    pub applied: Vec<f64>,
    pub lower: Vec<f64>,
    pub delta: Vec<f64>,
    pub values: Vec<f64>,
    pub solve: Vec<f64>,
}
impl VectorConfidenceModel {
    pub(crate) fn new(c: usize) -> VectorConfidenceModel {
        VectorConfidenceModel {
            c,
            samples: 0,
            mean: vec![0.0; c],
            m2: vec![0.0; c * c],
            cov: vec![0.0; c * c],
            applied: vec![0.0; c * c],
            lower: vec![0.0; c * c],
            delta: vec![0.0; c],
            values: vec![0.0; c],
            solve: vec![0.0; c],
        }
    }
    pub(crate) fn reset(&mut self, g: &mut Ledger<'_>) -> Result<(), Error> {
        g.charge((self.c + self.c * self.c) as u64)?;
        self.mean.fill(0.0);
        self.m2.fill(0.0);
        self.samples = 0;
        Ok(())
    }
    pub(crate) fn add_values(&mut self, g: &mut Ledger<'_>) -> Result<(), Error> {
        let c = self.c;
        g.charge((4 * c + 3 * c * (c + 1) / 2) as u64)?;
        self.samples += 1;
        let n = self.samples as f64;
        let factor = (n - 1.0) / n;
        for i in 0..c {
            self.delta[i] = self.values[i] - self.mean[i];
            self.mean[i] += self.delta[i] / n;
        }
        for i in 0..c {
            for j in i..c {
                self.m2[i * c + j] += self.delta[i] * self.delta[j] * factor;
                if !finite(self.m2[i * c + j]) {
                    return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn covariance(
        &mut self,
        g: &mut Ledger<'_>,
        params: &VectorConfidenceParams,
    ) -> Result<(), Error> {
        let c = self.c;
        if self.samples < 2 {
            return Err(Error::wrap(
                ERR_VECTOR_CONFIDENCE_COVARIANCE,
                "fewer than two samples",
            ));
        }
        g.charge((c * c) as u64)?;
        let mut denominator = (self.samples - 1) as f64;
        if params.covariance_normalization == VECTOR_COVARIANCE_POPULATION {
            denominator = self.samples as f64;
        }
        for i in 0..c {
            for j in i..c {
                let v = self.m2[i * c + j] / denominator;
                if !finite(v) {
                    return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
                }
                self.cov[i * c + j] = v;
                self.cov[j * c + i] = v;
            }
        }
        Ok(())
    }
    pub(crate) fn factor(
        &mut self,
        g: &mut Ledger<'_>,
        params: &VectorConfidenceParams,
    ) -> Result<(), Error> {
        let c = self.c;
        g.charge((2 * c * c + 2 * c * c * c) as u64)?;
        self.applied.copy_from_slice(&self.cov);
        self.lower.fill(0.0);
        let mut max_diagonal = 0.0f64;
        for i in 0..c {
            if params.covariance_policy == VECTOR_COVARIANCE_RIDGE {
                self.applied[i * c + i] += params.ridge;
            }
            let v = self.applied[i * c + i];
            if !finite(v) {
                return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
            }
            max_diagonal = fmath::max(max_diagonal, v);
        }
        if max_diagonal <= 0.0 {
            return Err(Error::wrap(
                ERR_VECTOR_CONFIDENCE_COVARIANCE,
                "non-positive covariance scale",
            ));
        }
        for i in 0..c {
            for j in 0..=i {
                let mut v = self.applied[i * c + j];
                for k in 0..j {
                    v -= self.lower[i * c + k] * self.lower[j * c + k];
                }
                if !finite(v) {
                    return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
                }
                if i == j {
                    if !(v > 0.0) || !(v / max_diagonal > params.min_pivot_ratio) {
                        return Err(Error::wrap(
                            ERR_VECTOR_CONFIDENCE_COVARIANCE,
                            format_args!("pivot {i} fails relative admission"),
                        ));
                    }
                    self.lower[i * c + j] = v.sqrt();
                } else {
                    self.lower[i * c + j] = v / self.lower[j * c + j];
                    if !finite(self.lower[i * c + j]) {
                        return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn distance(&mut self, g: &mut Ledger<'_>, value: &[f64]) -> Result<f64, Error> {
        mahalanobis(self.c, &self.mean, &self.lower, &mut self.solve, value, g)
    }
    pub(crate) fn distance_of_values(&mut self, g: &mut Ledger<'_>) -> Result<f64, Error> {
        mahalanobis(
            self.c,
            &self.mean,
            &self.lower,
            &mut self.solve,
            &self.values,
            g,
        )
    }
}
fn mahalanobis(
    c: usize,
    mean: &[f64],
    lower: &[f64],
    solve: &mut [f64],
    value: &[f64],
    g: &mut Ledger<'_>,
) -> Result<f64, Error> {
    g.charge((c * c + 3 * c) as u64)?;
    let mut distance = 0.0f64;
    for i in 0..c {
        let mut v = value[i] - mean[i];
        for j in 0..i {
            v -= lower[i * c + j] * solve[j];
        }
        solve[i] = v / lower[i * c + i];
        if !finite(solve[i]) {
            return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
        }
        distance = fmath::hypot(distance, solve[i]);
    }
    if !finite(distance) {
        return Err(Error::sentinel(ERR_VECTOR_CONFIDENCE_NUMERIC));
    }
    Ok(distance)
}
