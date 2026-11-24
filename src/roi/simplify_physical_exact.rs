use super::simplify_physical::Point3D;
use crate::fmath;
use num_bigint::{BigInt, BigUint, Sign};
use std::cmp::Ordering;
#[derive(Clone, Debug)]
struct Dyadic {
    m: BigInt,
    e: i64,
}
impl Dyadic {
    fn from_f64(v: f64) -> Dyadic {
        let bits = v.to_bits();
        let negative = bits >> 63 != 0;
        let exponent = ((bits >> 52) & 0x7ff) as i64;
        let fraction = bits & ((1u64 << 52) - 1);
        let (mantissa, e) = if exponent == 0 {
            (fraction, -1074)
        } else {
            (fraction | (1u64 << 52), exponent - 1075)
        };
        if mantissa == 0 {
            return Dyadic {
                m: BigInt::from(0),
                e: 0,
            };
        }
        let shift = mantissa.trailing_zeros();
        let m = BigInt::from(mantissa >> shift);
        Dyadic {
            m: if negative { -m } else { m },
            e: e + i64::from(shift),
        }
    }
    fn aligned(&self, other: &Dyadic) -> (BigInt, BigInt, i64) {
        match self.e.cmp(&other.e) {
            Ordering::Equal => (self.m.clone(), other.m.clone(), self.e),
            Ordering::Greater => (
                &self.m << (self.e - other.e) as u64,
                other.m.clone(),
                other.e,
            ),
            Ordering::Less => (
                self.m.clone(),
                &other.m << (other.e - self.e) as u64,
                self.e,
            ),
        }
    }
    fn add(&self, other: &Dyadic) -> Dyadic {
        let (a, b, e) = self.aligned(other);
        Dyadic { m: a + b, e }
    }
    fn sub(&self, other: &Dyadic) -> Dyadic {
        let (a, b, e) = self.aligned(other);
        Dyadic { m: a - b, e }
    }
    fn mul(&self, other: &Dyadic) -> Dyadic {
        Dyadic {
            m: &self.m * &other.m,
            e: self.e + other.e,
        }
    }
    fn sign(&self) -> i32 {
        match self.m.sign() {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        }
    }
    fn cmp(&self, other: &Dyadic) -> Ordering {
        let (a, b, _) = self.aligned(other);
        a.cmp(&b)
    }
}
type RatVec = [Dyadic; 3];
fn rat_point(p: Point3D) -> RatVec {
    [
        Dyadic::from_f64(p.x),
        Dyadic::from_f64(p.y),
        Dyadic::from_f64(p.z),
    ]
}
fn rat_sub(a: &RatVec, b: &RatVec) -> RatVec {
    [a[0].sub(&b[0]), a[1].sub(&b[1]), a[2].sub(&b[2])]
}
fn rat_dot(a: &RatVec, b: &RatVec) -> Dyadic {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}
fn rat_cross(a: &RatVec, b: &RatVec) -> RatVec {
    let component = |i: usize| {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        a[j].mul(&b[k]).sub(&a[k].mul(&b[j]))
    };
    [component(0), component(1), component(2)]
}
pub(super) fn polyline_independent(a: Point3D, b: Point3D) -> bool {
    let c = rat_cross(&rat_point(a), &rat_point(b));
    c.iter().any(|v| v.sign() != 0)
}
pub(super) fn polyline_within(p: Point3D, a: Point3D, b: Point3D, tolerance: f64) -> bool {
    let (pv, av, bv) = (rat_point(p), rat_point(a), rat_point(b));
    let (d, v) = (rat_sub(&bv, &av), rat_sub(&pv, &av));
    let (l2, dot) = (rat_dot(&d, &d), rat_dot(&v, &d));
    let t = Dyadic::from_f64(tolerance);
    let t2 = t.mul(&t);
    if l2.sign() == 0 || dot.sign() <= 0 {
        return rat_dot(&v, &v).cmp(&t2) != Ordering::Greater;
    }
    if dot.cmp(&l2) != Ordering::Less {
        let w = rat_sub(&pv, &bv);
        return rat_dot(&w, &w).cmp(&t2) != Ordering::Greater;
    }
    let v2 = rat_dot(&v, &v);
    let left = v2.mul(&l2).sub(&dot.mul(&dot));
    let right = t2.mul(&l2);
    left.cmp(&right) != Ordering::Greater
}
pub(super) fn polyline_segments_intersect(a: Point3D, b: Point3D, c: Point3D, d: Point3D) -> bool {
    for axis in 0..3 {
        let (mut av, mut bv, mut cv, mut dv) = (
            polyline_coord(a, axis),
            polyline_coord(b, axis),
            polyline_coord(c, axis),
            polyline_coord(d, axis),
        );
        if av > bv {
            (av, bv) = (bv, av);
        }
        if cv > dv {
            (cv, dv) = (dv, cv);
        }
        if bv < cv || dv < av {
            return false;
        }
    }
    let (av, bv, cv, dv) = (rat_point(a), rat_point(b), rat_point(c), rat_point(d));
    let (ab, ac, ad) = (rat_sub(&bv, &av), rat_sub(&cv, &av), rat_sub(&dv, &av));
    let mut normal = rat_cross(&ab, &ac);
    let volume = rat_dot(&normal, &ad);
    if volume.sign() != 0 {
        return false;
    }
    let mut axis = normal.iter().position(|v| v.sign() != 0);
    if axis.is_none() {
        normal = rat_cross(&ab, &ad);
        axis = normal.iter().position(|v| v.sign() != 0);
    }
    let Some(axis) = axis else {
        return true;
    };
    let (x, y) = ((axis + 1) % 3, (axis + 2) % 3);
    let s1 = polyline_orient(&av, &bv, &cv, x, y);
    let s2 = polyline_orient(&av, &bv, &dv, x, y);
    let s3 = polyline_orient(&cv, &dv, &av, x, y);
    let s4 = polyline_orient(&cv, &dv, &bv, x, y);
    if s1 == 0 && polyline_on_box(c, a, b) {
        return true;
    }
    if s2 == 0 && polyline_on_box(d, a, b) {
        return true;
    }
    if s3 == 0 && polyline_on_box(a, c, d) {
        return true;
    }
    if s4 == 0 && polyline_on_box(b, c, d) {
        return true;
    }
    s1 * s2 < 0 && s3 * s4 < 0
}
fn polyline_orient(a: &RatVec, b: &RatVec, c: &RatVec, x: usize, y: usize) -> i32 {
    let bx = b[x].sub(&a[x]);
    let by = b[y].sub(&a[y]);
    let cx = c[x].sub(&a[x]);
    let cy = c[y].sub(&a[y]);
    bx.mul(&cy).sub(&by.mul(&cx)).sign()
}
fn polyline_coord(p: Point3D, axis: usize) -> f64 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}
fn polyline_on_box(p: Point3D, a: Point3D, b: Point3D) -> bool {
    for axis in 0..3 {
        let (v, mut x, mut y) = (
            polyline_coord(p, axis),
            polyline_coord(a, axis),
            polyline_coord(b, axis),
        );
        if x > y {
            (x, y) = (y, x);
        }
        if v < x || v > y {
            return false;
        }
    }
    true
}
pub(super) fn polyline_exact_segment_distance(p: Point3D, a: Point3D, b: Point3D) -> f64 {
    let (pv, av, bv) = (rat_point(p), rat_point(a), rat_point(b));
    let (d, v) = (rat_sub(&bv, &av), rat_sub(&pv, &av));
    let (l2, dot) = (rat_dot(&d, &d), rat_dot(&v, &d));
    let one = Dyadic {
        m: BigInt::from(1),
        e: 0,
    };
    let (numerator, denominator) = if l2.sign() == 0 || dot.sign() <= 0 {
        (rat_dot(&v, &v), one)
    } else if dot.cmp(&l2) != Ordering::Less {
        let w = rat_sub(&pv, &bv);
        (rat_dot(&w, &w), one)
    } else {
        let v2 = rat_dot(&v, &v);
        (v2.mul(&l2).sub(&dot.mul(&dot)), l2)
    };
    if numerator.sign() <= 0 {
        return 0.0;
    }
    let n = numerator.m.magnitude().clone();
    let dm = denominator.m.magnitude().clone();
    let x = BigFloat::quotient(&n, &dm, numerator.e - denominator.e, 128);
    x.sqrt_128().to_f64()
}
#[derive(Clone, Debug)]
struct BigFloat {
    m: BigUint,
    e: i64,
}
impl BigFloat {
    fn rounded(m: BigUint, e: i64, sticky: bool, prec: u64) -> BigFloat {
        let bits = m.bits();
        if bits <= prec {
            return BigFloat { m, e };
        }
        let drop = bits - prec;
        let half = m.bit(drop - 1);
        let rest = sticky || m.trailing_zeros().is_some_and(|z| z < drop - 1);
        let mut kept = &m >> drop;
        if half && (rest || kept.bit(0)) {
            kept += 1u32;
        }
        BigFloat {
            m: kept,
            e: e + drop as i64,
        }
    }
    fn quotient(n: &BigUint, d: &BigUint, e: i64, prec: u64) -> BigFloat {
        let want = prec + 2 + d.bits();
        let shift = want.saturating_sub(n.bits());
        let scaled = n << shift;
        let q = &scaled / d;
        let sticky = &q * d != scaled;
        BigFloat::rounded(q, e - shift as i64, sticky, prec)
    }
    fn from_f64(v: f64) -> BigFloat {
        let d = Dyadic::from_f64(v);
        BigFloat {
            m: d.m.magnitude().clone(),
            e: d.e,
        }
    }
    fn mul(&self, other: &BigFloat, prec: u64) -> BigFloat {
        BigFloat::rounded(&self.m * &other.m, self.e + other.e, false, prec)
    }
    fn three_minus(&self, prec: u64) -> BigFloat {
        let three = BigUint::from(3u32);
        let (a, b, e) = if self.e >= 0 {
            (three, &self.m << self.e as u64, 0)
        } else {
            (three << (-self.e) as u64, self.m.clone(), self.e)
        };
        BigFloat::rounded(a - b, e, false, prec)
    }
    fn sqrt_128(&self) -> BigFloat {
        let bits = self.m.bits() as i64;
        let b = self.e + bits;
        let z = BigFloat {
            m: self.m.clone(),
            e: -bits + b % 2,
        };
        let mut t = BigFloat::from_f64(1.0 / z.to_f64().sqrt());
        let mut prec = 53u64;
        while prec < 128 + 32 {
            prec *= 2;
            let u = t.mul(&t, prec);
            let u = z.mul(&u, prec);
            let v = u.three_minus(prec);
            let mut u = t.mul(&v, prec);
            u.e -= 1;
            t = u;
        }
        let mut root = z.mul(&t, 128);
        root.e += b / 2;
        root
    }
    fn to_f64(&self) -> f64 {
        let bits = self.m.bits() as i64;
        if bits == 0 {
            return 0.0;
        }
        let top = bits - 1 + self.e;
        if top > 1023 {
            return f64::INFINITY;
        }
        let quantum = (top - 52).max(-1074);
        let scaled = if self.e >= quantum {
            &self.m << (self.e - quantum) as u64
        } else {
            let drop = (quantum - self.e) as u64;
            if drop > self.m.bits() {
                return 0.0;
            }
            let half = self.m.bit(drop - 1);
            let rest = self.m.trailing_zeros().is_some_and(|z| z < drop - 1);
            let mut kept = &self.m >> drop;
            if half && (rest || kept.bit(0)) {
                kept += 1u32;
            }
            kept
        };
        let mantissa = scaled.iter_u64_digits().next().unwrap_or(0);
        fmath::ldexp(mantissa as f64, quantum)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dyadic_round_trip() {
        for v in [0.0, -0.0, 1.0, -2.5, 5e-324, 1e9, 0.1, f64::MIN_POSITIVE] {
            let d = Dyadic::from_f64(v);
            let f = BigFloat {
                m: d.m.magnitude().clone(),
                e: d.e,
            };
            assert_eq!(f.to_f64(), v.abs());
        }
    }
    #[test]
    fn exact_distances() {
        let p = |x, y, z| Point3D { x, y, z };
        assert_eq!(
            polyline_exact_segment_distance(p(0.0, 3.0, 4.0), p(0.0, 0.0, 0.0), p(0.0, 0.0, 0.0)),
            5.0
        );
        assert_eq!(
            polyline_exact_segment_distance(p(1.0, 2.0, 0.0), p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0)),
            2.0
        );
        assert_eq!(
            polyline_exact_segment_distance(p(1.0, 1.0, 0.0), p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0)),
            1.0
        );
        let d =
            polyline_exact_segment_distance(p(0.0, 1.0, 0.0), p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0));
        assert_eq!(d, 0.5f64.sqrt());
        assert_eq!(
            polyline_exact_segment_distance(
                p(5e-324, 0.0, 0.0),
                p(0.0, 0.0, 0.0),
                p(0.0, 0.0, 0.0)
            ),
            5e-324
        );
    }
}
