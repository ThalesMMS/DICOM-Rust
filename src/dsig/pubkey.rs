use super::der;
use num_bigint::BigUint;
fn hex(text: &str) -> BigUint {
    BigUint::parse_bytes(text.as_bytes(), 16).unwrap_or_default()
}
pub(super) fn rsa_verify(n: &BigUint, e: &BigUint, info: &[u8], signature: &[u8]) -> bool {
    let bits = n.bits();
    let exponent_ok = e.bits() <= 31 && e.bit(0) && *e >= BigUint::from(2u32);
    if bits < 1024 || !n.bit(0) || !exponent_ok {
        return false;
    }
    let k = bits.div_ceil(8) as usize;
    if signature.len() != k || k < info.len() + 11 {
        return false;
    }
    let s = BigUint::from_bytes_be(signature);
    if s >= *n {
        return false;
    }
    let em = s.modpow(e, n).to_bytes_be();
    let mut expected = vec![0xFFu8; k];
    expected[0] = 0;
    expected[1] = 1;
    expected[k - info.len() - 1] = 0;
    expected[k - info.len()..].copy_from_slice(info);
    em.len() < k
        && expected[k - em.len()..] == em[..]
        && expected[..k - em.len()].iter().all(|&b| b == 0)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    P224,
    P256,
    P384,
    P521,
}
struct Params {
    p: BigUint,
    b: BigUint,
    gx: BigUint,
    gy: BigUint,
    n: BigUint,
}
impl Curve {
    pub fn name(self) -> &'static str {
        match self {
            Curve::P224 => "P-224",
            Curve::P256 => "P-256",
            Curve::P384 => "P-384",
            Curve::P521 => "P-521",
        }
    }
    pub(super) fn byte_len(self) -> usize {
        match self {
            Curve::P224 => 28,
            Curve::P256 => 32,
            Curve::P384 => 48,
            Curve::P521 => 66,
        }
    }
    fn params(self) -> Params {
        let [p, b, gx, gy, n] = match self {
            Curve::P224 => [
                "ffffffffffffffffffffffffffffffff000000000000000000000001",
                "b4050a850c04b3abf54132565044b0b7d7bfd8ba270b39432355ffb4",
                "b70e0cbd6bb4bf7f321390b94a03c1d356c21122343280d6115c1d21",
                "bd376388b5f723fb4c22dfe6cd4375a05a07476444d5819985007e34",
                "ffffffffffffffffffffffffffff16a2e0b8f03e13dd29455c5c2a3d",
            ],
            Curve::P256 => [
                "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
                "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
                "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
                "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
                "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
            ],
            Curve::P384 => [
                "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff",
                "b3312fa7e23ee7e4988e056be3f82d19181d9c6efe8141120314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef",
                "aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7",
                "3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f",
                "ffffffffffffffffffffffffffffffffffffffffffffffffc7634d81f4372ddf581a0db248b0a77aecec196accc52973",
            ],
            Curve::P521 => [
                "1ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "51953eb9618e1c9a1f929a21a0b68540eea2da725b99b315f3b8b489918ef109e156193951ec7e937b1652c0bd3bb1bf073573df883d2c34f1ef451fd46b503f00",
                "c6858e06b70404e9cd9e3ecb662395b4429c648139053fb521f828af606b4d3dbaa14b5e77efe75928fe1dc127a2ffa8de3348b3c1856a429bf97e7e31c2e5bd66",
                "11839296a789a3bc0045c8a5fb42c7d1bd998f54449579b446817afbd17273e662c97ee72995ef42640c550b9013fad0761353c7086a272c24088be94769fd16650",
                "1fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffa51868783bf2f966b7fcc0148f709a5d03bb5c9b8899c47aebb6fb71e91386409",
            ],
        }
        .map(hex);
        Params { p, b, gx, gy, n }
    }
    pub(super) fn on_curve(self, x: &BigUint, y: &BigUint) -> bool {
        let c = self.params();
        if *x >= c.p || *y >= c.p {
            return false;
        }
        let three_x = (x * 3u32) % &c.p;
        let right = (x * x % &c.p * x + &c.b + &c.p - three_x) % &c.p;
        y * y % &c.p == right
    }
}
#[derive(Clone)]
struct Point {
    x: BigUint,
    y: BigUint,
    z: BigUint,
}
struct Field<'a> {
    p: &'a BigUint,
}
impl Field<'_> {
    fn mul(&self, a: &BigUint, b: &BigUint) -> BigUint {
        a * b % self.p
    }
    fn sub(&self, a: &BigUint, b: &BigUint) -> BigUint {
        (a + self.p - b % self.p) % self.p
    }
    fn infinity(&self) -> Point {
        Point {
            x: BigUint::from(1u32),
            y: BigUint::from(1u32),
            z: BigUint::default(),
        }
    }
    fn double(&self, q: &Point) -> Point {
        if q.z.bits() == 0 || q.y.bits() == 0 {
            return self.infinity();
        }
        let zz = self.mul(&q.z, &q.z);
        let m = self.mul(&self.sub(&q.x, &zz), &(&q.x + &zz)) * 3u32 % self.p;
        let yy = self.mul(&q.y, &q.y);
        let s = self.mul(&(&q.x * 4u32), &yy);
        let x = self.sub(&self.mul(&m, &m), &(&s * 2u32));
        let y = self.sub(
            &self.mul(&m, &self.sub(&s, &x)),
            &(self.mul(&yy, &yy) * 8u32),
        );
        let z = self.mul(&(&q.y * 2u32), &q.z);
        Point { x, y, z }
    }
    fn add(&self, a: &Point, b: &Point) -> Point {
        if a.z.bits() == 0 {
            return b.clone();
        }
        if b.z.bits() == 0 {
            return a.clone();
        }
        let z1z1 = self.mul(&a.z, &a.z);
        let z2z2 = self.mul(&b.z, &b.z);
        let u1 = self.mul(&a.x, &z2z2);
        let u2 = self.mul(&b.x, &z1z1);
        let s1 = self.mul(&self.mul(&a.y, &b.z), &z2z2);
        let s2 = self.mul(&self.mul(&b.y, &a.z), &z1z1);
        if u1 == u2 {
            return if s1 == s2 {
                self.double(a)
            } else {
                self.infinity()
            };
        }
        let h = self.sub(&u2, &u1);
        let r = self.sub(&s2, &s1);
        let hh = self.mul(&h, &h);
        let hhh = self.mul(&hh, &h);
        let v = self.mul(&u1, &hh);
        let x = self.sub(&self.sub(&self.mul(&r, &r), &hhh), &(&v * 2u32));
        let y = self.sub(&self.mul(&r, &self.sub(&v, &x)), &self.mul(&s1, &hhh));
        let z = self.mul(&self.mul(&a.z, &b.z), &h);
        Point { x, y, z }
    }
    fn scalar_mul(&self, q: &Point, k: &BigUint) -> Point {
        let mut out = self.infinity();
        for i in (0..k.bits()).rev() {
            out = self.double(&out);
            if k.bit(i) {
                out = self.add(&out, q);
            }
        }
        out
    }
    fn affine_x(&self, q: &Point) -> Option<BigUint> {
        if q.z.bits() == 0 {
            return None;
        }
        let inverse = q.z.modinv(self.p)?;
        Some(self.mul(&q.x, &self.mul(&inverse, &inverse)))
    }
}
fn digest_to_scalar(digest: &[u8], n: &BigUint) -> BigUint {
    let size = n.bits().div_ceil(8) as usize;
    let mut e = BigUint::from_bytes_be(&digest[..digest.len().min(size)]);
    if digest.len() >= size {
        e >>= size as u64 * 8 - n.bits();
    }
    e % n
}
pub(super) fn ecdsa_verify(
    curve: Curve,
    qx: &BigUint,
    qy: &BigUint,
    digest: &[u8],
    signature: &[u8],
) -> bool {
    let Some((outer, [])) = der::read(signature) else {
        return false;
    };
    let Some(parts) = der::sequence(&outer) else {
        return false;
    };
    let [r, s] = parts.as_slice() else {
        return false;
    };
    let (Some(r), Some(s)) = (der::unsigned(r), der::unsigned(s)) else {
        return false;
    };
    let c = curve.params();
    if r.bits() == 0 || s.bits() == 0 || r >= c.n || s >= c.n {
        return false;
    }
    if !curve.on_curve(qx, qy) {
        return false;
    }
    let Some(w) = s.modinv(&c.n) else {
        return false;
    };
    let e = digest_to_scalar(digest, &c.n);
    let field = Field { p: &c.p };
    let one = BigUint::from(1u32);
    let g = Point {
        x: c.gx.clone(),
        y: c.gy.clone(),
        z: one.clone(),
    };
    let q = Point {
        x: qx.clone(),
        y: qy.clone(),
        z: one,
    };
    let p1 = field.scalar_mul(&g, &(e * &w % &c.n));
    let p2 = field.scalar_mul(&q, &(&r * &w % &c.n));
    match field.affine_x(&field.add(&p1, &p2)) {
        Some(x) => x % &c.n == r,
        None => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generators_are_on_their_curves() {
        for curve in [Curve::P224, Curve::P256, Curve::P384, Curve::P521] {
            let c = curve.params();
            assert!(curve.on_curve(&c.gx, &c.gy), "{}", curve.name());
            assert!(!curve.on_curve(&c.gx, &(&c.gy + 1u32)));
            let field = Field { p: &c.p };
            let g = Point {
                x: c.gx.clone(),
                y: c.gy.clone(),
                z: BigUint::from(1u32),
            };
            assert!(field.affine_x(&field.scalar_mul(&g, &c.n)).is_none());
            let twice = field.affine_x(&field.double(&g));
            assert_eq!(twice, field.affine_x(&field.add(&g, &g)));
            assert!(twice.is_some());
        }
    }
    #[test]
    fn digest_truncation() {
        let n = Curve::P521.params().n;
        assert_eq!(
            digest_to_scalar(&[0xFF; 64], &n),
            BigUint::from_bytes_be(&[0xFF; 64])
        );
        let n = Curve::P224.params().n;
        assert_eq!(
            digest_to_scalar(&[0x12; 32], &n),
            BigUint::from_bytes_be(&[0x12; 28]) % &n
        );
    }
}
