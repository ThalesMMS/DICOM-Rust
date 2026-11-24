use super::{Mesh, MeshError, face_normals};
use crate::fmath;
use crate::textfmt::float_v;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowFunction(pub i64);
impl WindowFunction {
    #[doc(hidden)]
    pub const NUTTALL: WindowFunction = WindowFunction(0);
    pub const BLACKMAN: WindowFunction = WindowFunction(1);
    pub const HANNING: WindowFunction = WindowFunction(2);
    pub const HAMMING: WindowFunction = WindowFunction(3);
}
pub const DEFAULT_FEATURE_ANGLE: f64 = 45.0;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SmoothOptions {
    pub iterations: i64,
    pub pass_band: f64,
    pub window: WindowFunction,
    pub fix_boundary: bool,
    pub preserve_feature_edges: bool,
    pub feature_angle: f64,
    pub workers: i64,
}
pub fn smooth_windowed_sinc(m: &Mesh, opts: &SmoothOptions) -> Result<Mesh, MeshError> {
    m.validate()?;
    let degree = if opts.iterations == 0 {
        20
    } else {
        opts.iterations
    };
    let pass_band = if opts.pass_band == 0.0 {
        0.1
    } else {
        opts.pass_band
    };
    if degree < 1 || !(pass_band > 0.0 && pass_band < 2.0) {
        return Err(MeshError::invalid(format!(
            "smoothing degree {degree}, pass band {}",
            float_v(pass_band)
        )));
    }
    let mut feature_angle = -1.0;
    if opts.preserve_feature_edges {
        feature_angle = opts.feature_angle;
        if feature_angle == 0.0 {
            feature_angle = DEFAULT_FEATURE_ANGLE;
        }
        if !(feature_angle > 0.0 && feature_angle < 180.0) {
            return Err(MeshError::invalid(format!(
                "feature angle {}",
                float_v(opts.feature_angle)
            )));
        }
    }
    let (neighbours, start) = smoothing_adjacency(m, opts.fix_boundary, feature_angle);
    let degree = degree as usize;
    let coefficients = sinc_coefficients(degree, pass_band, opts.window);
    let n = m.vertices.len();
    let umbrella = |x: &[[f64; 3]], i: usize| -> [f64; 3] {
        let list = &neighbours[start[i] as usize..start[i + 1] as usize];
        if list.is_empty() {
            return x[i];
        }
        let mut mean = [0.0f64; 3];
        for j in list {
            let p = x[*j as usize];
            mean[0] += p[0];
            mean[1] += p[1];
            mean[2] += p[2];
        }
        let scale = 1.0 / list.len() as f64;
        [mean[0] * scale, mean[1] * scale, mean[2] * scale]
    };
    let mut previous: Vec<[f64; 3]> = m
        .vertices
        .iter()
        .map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
        .collect();
    let mut sum: Vec<[f64; 3]> = previous
        .iter()
        .map(|p| {
            [
                coefficients[0] * p[0],
                coefficients[0] * p[1],
                coefficients[0] * p[2],
            ]
        })
        .collect();
    let mut current = vec![[0.0f64; 3]; n];
    let mut next = vec![[0.0f64; 3]; n];
    for i in 0..n {
        current[i] = umbrella(&previous, i);
        for axis in 0..3 {
            sum[i][axis] += coefficients[1] * current[i][axis];
        }
    }
    for c in coefficients.iter().skip(2) {
        for i in 0..n {
            let w = umbrella(&current, i);
            for axis in 0..3 {
                next[i][axis] = 2.0 * w[axis] - previous[i][axis];
                sum[i][axis] += c * next[i][axis];
            }
        }
        std::mem::swap(&mut previous, &mut current);
        std::mem::swap(&mut current, &mut next);
    }
    let mut out = Mesh {
        vertices: sum
            .iter()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect(),
        normals: None,
        triangles: m.triangles.clone(),
    };
    if m.normals.is_some() {
        face_normals(&mut out, None);
    }
    Ok(out)
}
fn sinc_coefficients(degree: usize, pass_band: f64, window: WindowFunction) -> Vec<f64> {
    let response = |c: &[f64], k: f64| -> f64 {
        let x = 1.0 - k;
        let (mut previous, mut current) = (1.0, x);
        let mut value = c[0] + c[1] * x;
        for cn in c.iter().skip(2) {
            let following = 2.0 * x * current - previous;
            previous = current;
            current = following;
            value += cn * current;
        }
        value
    };
    let gap = |sigma: f64| -> f64 {
        response(
            &windowed_sinc(degree, fmath::min(pass_band + sigma, 2.0), window),
            pass_band,
        ) - 1.0
    };
    let mut sigma = 0.0;
    if degree > 1 && gap(0.0) < 0.0 {
        const STEPS: i32 = 400;
        let (mut low, mut high) = (0.0, -1.0);
        for i in 1..=STEPS {
            let s = (2.0 - pass_band) * f64::from(i) / f64::from(STEPS);
            if gap(s) >= 0.0 {
                high = s;
                break;
            }
            low = s;
        }
        if high > 0.0 {
            for _ in 0..60 {
                let mid = (low + high) / 2.0;
                if gap(mid) >= 0.0 {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            sigma = high;
        }
    }
    windowed_sinc(degree, fmath::min(pass_band + sigma, 2.0), window)
}
fn windowed_sinc(degree: usize, pass_band: f64, window: WindowFunction) -> Vec<f64> {
    let theta = fmath::acos(1.0 - pass_band);
    let mut coefficients = vec![0.0; degree + 1];
    let mut total = 0.0;
    for (n, slot) in coefficients.iter_mut().enumerate() {
        let nf = n as f64;
        let mut c = theta / PI;
        if n > 0 {
            c = 2.0 * fmath::sin(nf * theta) / (nf * PI);
        }
        let x = PI * nf / (degree + 1) as f64;
        let w = match window {
            WindowFunction::BLACKMAN => 0.42 + 0.5 * fmath::cos(x) + 0.08 * fmath::cos(2.0 * x),
            WindowFunction::HANNING => 0.5 + 0.5 * fmath::cos(x),
            WindowFunction::HAMMING => 0.54 + 0.46 * fmath::cos(x),
            _ => {
                0.355768
                    + 0.487396 * fmath::cos(x)
                    + 0.144232 * fmath::cos(2.0 * x)
                    + 0.012604 * fmath::cos(3.0 * x)
            }
        };
        *slot = c * w;
        total += *slot;
    }
    for c in coefficients.iter_mut() {
        *c /= total;
    }
    coefficients
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshEdge {
    pub a: u32,
    pub b: u32,
    pub uses: i32,
    pub feature: bool,
}
pub(crate) fn sorted_edges(m: &Mesh, feature_angle: f64) -> Vec<MeshEdge> {
    let mut uses: Vec<(u32, u32, u32)> = Vec::with_capacity(3 * m.triangles.len());
    for (i, t) in m.triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            uses.push((a.min(b), a.max(b), i as u32));
        }
    }
    uses.sort_unstable();
    let cosine = fmath::cos(feature_angle * PI / 180.0);
    let mut edges = Vec::with_capacity(uses.len() / 2 + 1);
    let mut i = 0;
    while i < uses.len() {
        let mut j = i + 1;
        while j < uses.len() && uses[j].0 == uses[i].0 && uses[j].1 == uses[i].1 {
            j += 1;
        }
        let mut e = MeshEdge {
            a: uses[i].0,
            b: uses[i].1,
            uses: (j - i) as i32,
            feature: false,
        };
        if feature_angle > 0.0 && e.uses == 2 {
            let n0 = m.triangle_normal(m.triangles[uses[i].2 as usize]);
            let n1 = m.triangle_normal(m.triangles[uses[i + 1].2 as usize]);
            if n0 != [0.0; 3] && n1 != [0.0; 3] {
                let d = f64::from(n0[0]) * f64::from(n1[0])
                    + f64::from(n0[1]) * f64::from(n1[1])
                    + f64::from(n0[2]) * f64::from(n1[2]);
                e.feature = d < cosine;
            }
        }
        edges.push(e);
        i = j;
    }
    edges
}
pub(crate) fn smoothing_adjacency(
    m: &Mesh,
    fix_boundary: bool,
    feature_angle: f64,
) -> (Vec<u32>, Vec<i32>) {
    let edges = sorted_edges(m, feature_angle);
    let n = m.vertices.len();
    let mut boundary = vec![false; n];
    let mut fixed = vec![false; n];
    let use_features = feature_angle > 0.0;
    let mut feature = vec![false; if use_features { n } else { 0 }];
    for e in &edges {
        let (a, b) = (e.a as usize, e.b as usize);
        if e.uses == 1 {
            boundary[a] = true;
            boundary[b] = true;
        } else if e.uses > 2 {
            fixed[a] = true;
            fixed[b] = true;
        } else if e.feature {
            feature[a] = true;
            feature[b] = true;
        }
    }
    let keep = |fixed: &[bool], v: u32, e: &MeshEdge| -> bool {
        let v = v as usize;
        if fixed[v] {
            false
        } else if boundary[v] {
            e.uses == 1
        } else if use_features && feature[v] {
            e.feature
        } else {
            true
        }
    };
    let mut counts = vec![0i32; n];
    for e in &edges {
        if keep(&fixed, e.a, e) {
            counts[e.a as usize] += 1;
        }
        if keep(&fixed, e.b, e) {
            counts[e.b as usize] += 1;
        }
    }
    for v in 0..n {
        if (boundary[v] && (fix_boundary || counts[v] != 2))
            || (!boundary[v] && use_features && feature[v] && counts[v] != 2)
        {
            fixed[v] = true;
            counts[v] = 0;
        }
    }
    let mut start = vec![0i32; n + 1];
    for (v, c) in counts.iter().enumerate() {
        start[v + 1] = start[v] + c;
    }
    let mut neighbours = vec![0u32; start[n] as usize];
    let mut fill: Vec<i32> = start[..n].to_vec();
    for e in &edges {
        if keep(&fixed, e.a, e) {
            neighbours[fill[e.a as usize] as usize] = e.b;
            fill[e.a as usize] += 1;
        }
        if keep(&fixed, e.b, e) {
            neighbours[fill[e.b as usize] as usize] = e.a;
            fill[e.b as usize] += 1;
        }
    }
    (neighbours, start)
}
