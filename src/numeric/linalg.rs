use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidMatrix;
impl std::fmt::Display for InvalidMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("linalg: invalid symmetric matrix")
    }
}
impl std::error::Error for InvalidMatrix {}
const MAX_JACOBI_SWEEPS: usize = 64;
pub fn symmetric_eigen(a: &[Vec<f64>]) -> Result<(Vec<f64>, Vec<Vec<f64>>), InvalidMatrix> {
    let n = a.len();
    let mut scale = 0.0f64;
    for row in a {
        if row.len() != n {
            return Err(InvalidMatrix);
        }
        for &v in row {
            if !v.is_finite() {
                return Err(InvalidMatrix);
            }
            scale = fmath::max(scale, v.abs());
        }
    }
    let mut m = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            if (a[i][j] - a[j][i]).abs() > 1e-9 * scale {
                return Err(InvalidMatrix);
            }
            m[i][j] = (a[i][j] + a[j][i]) / 2.0;
        }
    }
    let mut v = vec![vec![0.0; n]; n];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    jacobi(&mut m, &mut v);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        if m[i][i] < m[j][j] {
            std::cmp::Ordering::Less
        } else if m[j][j] < m[i][i] {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let values = order.iter().map(|&c| m[c][c]).collect();
    let mut vectors = vec![vec![0.0; n]; n];
    for (k, &col) in order.iter().enumerate() {
        for i in 0..n {
            vectors[i][k] = v[i][col];
        }
    }
    Ok((values, vectors))
}
pub fn symmetric_eigen3(a: [[f64; 3]; 3]) -> Result<([f64; 3], [[f64; 3]; 3]), InvalidMatrix> {
    let rows: Vec<Vec<f64>> = a.iter().map(|r| r.to_vec()).collect();
    let (w, v) = symmetric_eigen(&rows)?;
    let mut values = [0.0; 3];
    let mut vectors = [[0.0; 3]; 3];
    values.copy_from_slice(&w);
    for (dst, src) in vectors.iter_mut().zip(&v) {
        dst.copy_from_slice(src);
    }
    Ok((values, vectors))
}
pub fn symmetric_eigen4(a: [[f64; 4]; 4]) -> Result<([f64; 4], [[f64; 4]; 4]), InvalidMatrix> {
    let rows: Vec<Vec<f64>> = a.iter().map(|r| r.to_vec()).collect();
    let (w, v) = symmetric_eigen(&rows)?;
    let mut values = [0.0; 4];
    let mut vectors = [[0.0; 4]; 4];
    values.copy_from_slice(&w);
    for (dst, src) in vectors.iter_mut().zip(&v) {
        dst.copy_from_slice(src);
    }
    Ok((values, vectors))
}
#[allow(clippy::needless_range_loop)]
fn jacobi(m: &mut [Vec<f64>], v: &mut [Vec<f64>]) {
    let n = m.len();
    for sweep in 0..MAX_JACOBI_SWEEPS {
        let (mut off, mut diag) = (0.0, 0.0);
        for p in 0..n {
            diag += m[p][p] * m[p][p];
            for q in p + 1..n {
                off += m[p][q] * m[p][q];
            }
        }
        if off == 0.0 || off <= 1e-32 * diag {
            return;
        }
        for p in 0..n.saturating_sub(1) {
            for q in p + 1..n {
                let apq = m[p][q];
                if apq == 0.0 {
                    continue;
                }
                let (app, aqq) = (m[p][p], m[q][q]);
                let g = 100.0 * apq.abs();
                if sweep > 3 && app.abs() + g == app.abs() && aqq.abs() + g == aqq.abs() {
                    m[p][q] = 0.0;
                    m[q][p] = 0.0;
                    continue;
                }
                let theta = (aqq - app) / (2.0 * apq);
                let t = if theta.abs() > 1e150 {
                    1.0 / (2.0 * theta)
                } else {
                    let t = 1.0 / (theta.abs() + (theta * theta + 1.0).sqrt());
                    if theta < 0.0 { -t } else { t }
                };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in m.iter_mut() {
                    let (mkp, mkq) = (row[p], row[q]);
                    row[p] = c * mkp - s * mkq;
                    row[q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
                m[p][q] = 0.0;
                m[q][p] = 0.0;
                for row in v.iter_mut() {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
    }
}
pub fn symmetric_eigenvalues3(
    a00: f64,
    a01: f64,
    a02: f64,
    a11: f64,
    a12: f64,
    a22: f64,
) -> [f64; 3] {
    if a01 == 0.0 && a02 == 0.0 && a12 == 0.0 {
        return sort3(a00, a11, a22);
    }
    let mx = fmath::max;
    let scale = mx(
        mx(mx(a00.abs(), a01.abs()), mx(a02.abs(), a11.abs())),
        mx(a12.abs(), a22.abs()),
    );
    if scale.is_nan() || scale <= 0.0 || scale.is_infinite() {
        return [f64::NAN; 3];
    }
    let inv = 1.0 / scale;
    let (a00, a01, a02, a11, a12, a22) = (
        a00 * inv,
        a01 * inv,
        a02 * inv,
        a11 * inv,
        a12 * inv,
        a22 * inv,
    );
    let q = (a00 + a11 + a22) / 3.0;
    let (b00, b11, b22) = (a00 - q, a11 - q, a22 - q);
    let p2 = b00 * b00 + b11 * b11 + b22 * b22 + 2.0 * (a01 * a01 + a02 * a02 + a12 * a12);
    let p = (p2 / 6.0).sqrt();
    if p == 0.0 {
        return [q * scale; 3];
    }
    let det = b00 * (b11 * b22 - a12 * a12) - a01 * (a01 * b22 - a12 * a02)
        + a02 * (a01 * a12 - b11 * a02);
    let r = det / (2.0 * p * p * p);
    let r = fmath::max(-1.0, fmath::min(1.0, r));
    let phi = fmath::acos(r) / 3.0;
    let largest = q + 2.0 * p * fmath::cos(phi);
    const TWO_PI_OVER_3: f64 = 2.094_395_102_393_195_7;
    let smallest = q + 2.0 * p * fmath::cos(phi + TWO_PI_OVER_3);
    let middle = 3.0 * q - largest - smallest;
    sort3(smallest * scale, middle * scale, largest * scale)
}
pub fn symmetric_eigenvalues2(a00: f64, a01: f64, a11: f64) -> [f64; 2] {
    if a01 == 0.0 {
        return if a00 <= a11 { [a00, a11] } else { [a11, a00] };
    }
    let mean = (a00 + a11) / 2.0;
    let radius = fmath::hypot((a00 - a11) / 2.0, a01);
    let big = if mean >= 0.0 {
        mean + radius
    } else {
        mean - radius
    };
    let small = if big != 0.0 {
        (a00 * a11 - a01 * a01) / big
    } else {
        0.0
    };
    if small <= big {
        [small, big]
    } else {
        [big, small]
    }
}
fn sort3(mut a: f64, mut b: f64, mut c: f64) -> [f64; 3] {
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    if b > c {
        std::mem::swap(&mut b, &mut c);
    }
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    [a, b, c]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_values() {
        let (values, _) = symmetric_eigen(&[
            vec![3.0, 1.0, 1.0],
            vec![1.0, 3.0, 1.0],
            vec![1.0, 1.0, 3.0],
        ])
        .unwrap();
        assert!((values[0] - 2.0).abs() < 1e-12 && (values[2] - 5.0).abs() < 1e-12);
        assert_eq!(symmetric_eigenvalues2(3.0, 0.0, -1.0), [-1.0, 3.0]);
        assert!(symmetric_eigen(&[vec![1.0, 2.0], vec![3.0, 1.0]]).is_err());
    }
}
