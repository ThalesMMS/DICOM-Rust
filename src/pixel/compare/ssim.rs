use super::presented::{PresentedFrame, PresentedOptions, SsimOptions};
use crate::fmath;
pub(super) fn weights(s: &SsimOptions) -> Vec<f64> {
    let window = s.window as usize;
    let r = s.window / 2;
    let mut out = Vec::with_capacity(window.saturating_mul(window));
    for y in r.saturating_neg()..=r {
        for x in r.saturating_neg()..=r {
            let w = if s.kernel == "gaussian" {
                fmath::exp(
                    -0.5 * (fmath::pow(x as f64 / s.sigma, 2.0)
                        + fmath::pow(y as f64 / s.sigma, 2.0)),
                )
            } else {
                1.0
            };
            out.push(w);
        }
    }
    out
}
fn reflected(i: i64, n: i64) -> i64 {
    if n == 1 {
        return 0;
    }
    let period = n.saturating_mul(2).saturating_sub(2);
    let i = i.checked_rem_euclid(period).unwrap_or(0);
    if i >= n { period.saturating_sub(i) } else { i }
}
pub(super) fn local(
    a: &PresentedFrame,
    b: &PresentedFrame,
    o: &PresentedOptions,
    w: &[f64],
    x: usize,
    y: usize,
    c: usize,
) -> f64 {
    let radius = o.ssim.window / 2;
    let reflect = o.ssim.border == "reflect";
    let comps = a.components as usize;
    let visit = |emit: &mut dyn FnMut(f64, f64, f64)| {
        let mut weights = w.iter();
        for dy in radius.saturating_neg()..=radius {
            for dx in radius.saturating_neg()..=radius {
                let weight = weights.next().copied().unwrap_or(0.0);
                let (mut xx, mut yy) =
                    ((x as i64).saturating_add(dx), (y as i64).saturating_add(dy));
                if reflect {
                    xx = reflected(xx, a.width);
                    yy = reflected(yy, a.height);
                } else if xx < 0 || yy < 0 || xx >= a.width || yy >= a.height {
                    continue;
                }
                let p = yy.saturating_mul(a.width).saturating_add(xx) as usize;
                if !o.mask.is_empty() && !o.mask.get(p).copied().unwrap_or(false) {
                    continue;
                }
                let i = p.saturating_mul(comps).saturating_add(c);
                if let (Some(&va), Some(&vb)) = (a.values.get(i), b.values.get(i)) {
                    emit(va, vb, weight);
                }
            }
        }
    };
    let (mut sum, mut mx, mut my) = (0.0, 0.0, 0.0);
    visit(&mut |v, u, weight| {
        sum += weight;
        mx += weight * v;
        my += weight * u;
    });
    mx /= sum;
    my /= sum;
    let (mut vx, mut vy, mut cov) = (0.0, 0.0, 0.0);
    visit(&mut |v, u, weight| {
        let (dx, dy) = (v - mx, u - my);
        vx += weight * dx * dx;
        vy += weight * dy * dy;
        cov += weight * dx * dy;
    });
    vx /= sum;
    vy /= sum;
    cov /= sum;
    let mut c1 = o.ssim.k1 * o.ssim.range;
    c1 *= c1;
    let mut c2 = o.ssim.k2 * o.ssim.range;
    c2 *= c2;
    let value =
        ((2.0 * mx * my + c1) * (2.0 * cov + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2));
    fmath::max(-1.0, fmath::min(1.0, value))
}
