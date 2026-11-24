#![forbid(unsafe_code)]
#![allow(clippy::excessive_precision, clippy::approx_constant)]
pub mod rand;
pub fn max(a: f64, b: f64) -> f64 {
    if a == f64::INFINITY || b == f64::INFINITY {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}
pub fn min(a: f64, b: f64) -> f64 {
    if a == f64::NEG_INFINITY || b == f64::NEG_INFINITY {
        return f64::NEG_INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}
#[allow(clippy::excessive_precision)]
pub fn f64_to_i64(x: f64) -> i64 {
    if x.is_nan() || !(-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&x) {
        i64::MIN
    } else {
        x as i64
    }
}
pub fn f64_to_i32(x: f64) -> i32 {
    if x.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&x) {
        i32::MIN
    } else {
        x as i32
    }
}
pub fn f64_to_u8(x: f64) -> u8 {
    f64_to_i32(x) as u8
}
pub fn next_up(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == 0.0 {
        return f64::from_bits(1);
    }
    let bits = x.to_bits();
    f64::from_bits(if x > 0.0 { bits + 1 } else { bits - 1 })
}
#[allow(clippy::excessive_precision, clippy::approx_constant)]
pub fn exp(x: f64) -> f64 {
    const LOG2E: f64 = 1.442_695_040_888_963_4;
    const LN2U: f64 = 0.693_147_180_559_662_956_511_601_805_686_950_683_593_75;
    const LN2L: f64 = 0.282_352_905_630_315_771_225_884_481_750_134_360_255_254_120_68e-12;
    const OVERFLOW: f64 = 7.097_827_128_933_84e2;
    const C: [f64; 9] = [
        0.5,
        1.0,
        2.0,
        1.666_666_666_666_666_666_7e-1,
        4.166_666_666_666_666_666_7e-2,
        8.333_333_333_333_333_333_3e-3,
        1.388_888_888_888_888_888_9e-3,
        1.984_126_984_126_984_127e-4,
        2.480_158_730_158_730_158_7e-5,
    ];
    if !x.is_finite() {
        return if x == f64::NEG_INFINITY { 0.0 } else { x };
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    let scaled = x * LOG2E;
    let exponent: i32 = if (-2_147_483_648.0..2_147_483_648.0).contains(&scaled.round_ties_even()) {
        scaled.round_ties_even() as i32
    } else {
        i32::MIN
    };
    let e = f64::from(exponent);
    let mut x0 = (-LN2U).mul_add(e, x);
    x0 = (-LN2L).mul_add(e, x0);
    x0 *= 0.0625;
    let mut x1 = C[8];
    for c in [C[7], C[6], C[5], C[4], C[3], C[0], C[1]] {
        x1 = x1.mul_add(x0, c);
    }
    x0 *= x1;
    for _ in 0..3 {
        x1 = x0 + C[2];
        x0 *= x1;
    }
    x1 = x0 + C[2];
    x0 = x0.mul_add(x1, C[1]);
    let mut biased = i64::from(exponent) + 0x3FF;
    if biased <= 0 {
        if biased < -52 {
            return 0.0;
        }
        biased += 0x3FE;
        x0 *= f64::from_bits((biased as u64) << 52);
        biased = 1;
    } else if biased >= 0x7FF {
        return f64::INFINITY;
    }
    x0 * f64::from_bits((biased as u64) << 52)
}
const SIN: [f64; 6] = [
    1.589_623_015_765_465_680_60e-10,
    -2.505_074_776_285_780_728_66e-8,
    2.755_731_362_138_572_452_13e-6,
    -1.984_126_982_958_953_859_96e-4,
    8.333_333_333_322_118_588_78e-3,
    -1.666_666_666_666_663_072_95e-1,
];
const COS: [f64; 6] = [
    -1.135_853_652_138_768_173_00e-11,
    2.087_570_084_197_473_167_78e-9,
    -2.755_731_417_929_673_881_12e-7,
    2.480_158_728_885_170_453_48e-5,
    -1.388_888_888_887_305_641_16e-3,
    4.166_666_666_666_659_292_18e-2,
];
const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;
const M_PI4: [u64; 20] = [
    0x0000000000000001,
    0x45f306dc9c882a53,
    0xf84eafa3ea69bb81,
    0xb6c52b3278872083,
    0xfca2c757bd778ac3,
    0x6e48dc74849ba5c0,
    0x0c925dd413a32439,
    0xfc3bd63962534e7d,
    0xd1046bea5d768909,
    0xd338e04d68befc82,
    0x7323ac7306a673e9,
    0x3908bf177bf25076,
    0x3ff12fffbc0b301f,
    0xde5e2316b414da3e,
    0xda6cfd9e4f96136e,
    0x9e8c7ecd3cbfd45a,
    0xea4f758fd7cbe2f6,
    0x7a0e73ef14a525d4,
    0xd7f6bf623f1aba10,
    0xac06608df8f6d757,
];
fn trig_reduce(x: f64) -> (u64, f64) {
    const PI4: f64 = std::f64::consts::PI / 4.0;
    if x < PI4 {
        return (0, x);
    }
    let mut ix = x.to_bits();
    let exp = ((ix >> 52) & 0x7FF) as i64 - 1023 - 52;
    ix &= !(0x7FFu64 << 52);
    ix |= 1 << 52;
    let digit = ((exp + 61) / 64) as usize;
    let bitshift = ((exp + 61) % 64) as u32;
    let shl = |v: u64, s: u32| if s >= 64 { 0 } else { v << s };
    let shr = |v: u64, s: u32| if s >= 64 { 0 } else { v >> s };
    let z0 = shl(M_PI4[digit], bitshift) | shr(M_PI4[digit + 1], 64 - bitshift);
    let z1 = shl(M_PI4[digit + 1], bitshift) | shr(M_PI4[digit + 2], 64 - bitshift);
    let z2 = shl(M_PI4[digit + 2], bitshift) | shr(M_PI4[digit + 3], 64 - bitshift);
    let z2hi = ((u128::from(z2) * u128::from(ix)) >> 64) as u64;
    let z1full = u128::from(z1) * u128::from(ix);
    let (z1hi, z1lo) = ((z1full >> 64) as u64, z1full as u64);
    let z0lo = z0.wrapping_mul(ix);
    let (lo, carry) = z1lo.overflowing_add(z2hi);
    let mut hi = z0lo.wrapping_add(z1hi).wrapping_add(u64::from(carry));
    let mut j = hi >> 61;
    hi = (hi << 3) | (lo >> 61);
    let lz = hi.leading_zeros();
    let e = (1023 - (i64::from(lz) + 1)) as u64;
    hi = shl(hi, lz + 1) | shr(lo, 64 - (lz + 1));
    hi >>= 64 - 52;
    hi |= e << 52;
    let mut z = f64::from_bits(hi);
    if j & 1 == 1 {
        j += 1;
        j &= 7;
        z -= 1.0;
    }
    (j, z * PI4)
}
#[allow(clippy::excessive_precision)]
fn octant(x: f64) -> (u64, f64) {
    const PI4A: f64 = 7.853_981_256_484_985_351_56e-1;
    const PI4B: f64 = 3.774_894_707_930_798_176_68e-8;
    const PI4C: f64 = 2.695_151_429_079_059_526_45e-15;
    if x >= REDUCE_THRESHOLD {
        return trig_reduce(x);
    }
    let mut j = (x * (4.0 / std::f64::consts::PI)) as u64;
    let mut y = j as f64;
    if j & 1 == 1 {
        j += 1;
        y += 1.0;
    }
    j &= 7;
    (j, ((x - y * PI4A) - y * PI4B) - y * PI4C)
}
fn sin_poly(z: f64, zz: f64) -> f64 {
    z + z
        * zz
        * (((((SIN[0] * zz + SIN[1]) * zz + SIN[2]) * zz + SIN[3]) * zz + SIN[4]) * zz + SIN[5])
}
fn cos_poly(zz: f64) -> f64 {
    1.0 - 0.5 * zz
        + zz * zz
            * (((((COS[0] * zz + COS[1]) * zz + COS[2]) * zz + COS[3]) * zz + COS[4]) * zz + COS[5])
}
pub fn sin(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = x < 0.0;
    let (mut j, z) = octant(x.abs());
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        cos_poly(zz)
    } else {
        sin_poly(z, zz)
    };
    if sign { -y } else { y }
}
pub fn sincos(x: f64) -> (f64, f64) {
    (sin(x), cos(x))
}
pub fn cos(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let (mut j, z) = octant(x.abs());
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        sin_poly(z, zz)
    } else {
        cos_poly(zz)
    };
    if sign { -y } else { y }
}
#[allow(clippy::excessive_precision)]
fn xatan(x: f64) -> f64 {
    const P: [f64; 5] = [
        -8.750_608_600_031_904_122_785e-01,
        -1.615_753_718_733_365_076_637e+01,
        -7.500_855_792_314_704_667_340e+01,
        -1.228_866_684_490_136_173_410e+02,
        -6.485_021_904_942_025_371_773e+01,
    ];
    const Q: [f64; 5] = [
        2.485_846_490_142_306_297_962e+01,
        1.650_270_098_316_988_542_046e+02,
        4.328_810_604_912_902_668_951e+02,
        4.853_903_996_359_136_964_868e+02,
        1.945_506_571_482_613_964_425e+02,
    ];
    let z = x * x;
    let z = z * ((((P[0] * z + P[1]) * z + P[2]) * z + P[3]) * z + P[4])
        / (((((z + Q[0]) * z + Q[1]) * z + Q[2]) * z + Q[3]) * z + Q[4]);
    x * z + x
}
#[allow(clippy::excessive_precision)]
fn satan(x: f64) -> f64 {
    use std::f64::consts::PI;
    const MOREBITS: f64 = 6.123_233_995_736_765_886_130e-17;
    const TAN3PIO8: f64 = 2.414_213_562_373_095_048_80;
    if x <= 0.66 {
        return xatan(x);
    }
    if x > TAN3PIO8 {
        return PI / 2.0 - xatan(1.0 / x) + MOREBITS;
    }
    PI / 4.0 + xatan((x - 1.0) / (x + 1.0)) + 0.5 * MOREBITS
}
pub fn atan(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 { satan(x) } else { -satan(-x) }
}
pub fn asin(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    let sign = x < 0.0;
    let x = x.abs();
    if x > 1.0 {
        return f64::NAN;
    }
    let temp = (1.0 - x * x).sqrt();
    let temp = if x > 0.7 {
        std::f64::consts::PI / 2.0 - satan(temp / x)
    } else {
        satan(x / temp)
    };
    if sign { -temp } else { temp }
}
pub fn acos(x: f64) -> f64 {
    std::f64::consts::PI / 2.0 - asin(x)
}
pub fn hypot(p: f64, q: f64) -> f64 {
    let (p, q) = (p.abs(), q.abs());
    if p.is_infinite() || q.is_infinite() {
        return f64::INFINITY;
    }
    if p.is_nan() || q.is_nan() {
        return f64::NAN;
    }
    if p == 0.0 && q == 0.0 {
        return 0.0;
    }
    let (big, small) = if p > q { (p, q) } else { (q, p) };
    let ratio = small / big;
    big * (ratio * ratio + 1.0).sqrt()
}
pub fn log(x: f64) -> f64 {
    const HSQRT2: f64 = 7.071_067_811_865_475_244_01e-01;
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const L1: f64 = 6.666_666_666_666_735_130e-01;
    const L2: f64 = 3.999_999_999_940_941_908e-01;
    const L3: f64 = 2.857_142_874_366_239_149e-01;
    const L4: f64 = 2.222_219_843_214_978_396e-01;
    const L5: f64 = 1.818_357_216_161_805_012e-01;
    const L6: f64 = 1.531_383_769_920_937_332e-01;
    const L7: f64 = 1.479_819_860_511_658_591e-01;
    let bits = x.to_bits();
    if bits & !(1 << 63) == 0 {
        return f64::NEG_INFINITY;
    }
    if (bits as i64) < 0 {
        return f64::from_bits(0x7FF8_0000_0000_0001);
    }
    if bits >= 0x7FF0_0000_0000_0000 {
        return x;
    }
    let mut f1 = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | 0x3FE0_0000_0000_0000);
    let mut k = f64::from(((bits >> 52) & 0x7FF) as i32 - 0x3FE);
    if f1 < HSQRT2 {
        k -= 1.0;
        f1 *= 2.0;
    }
    let f = f1 - 1.0;
    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    let t1 = s2 * (((L7 * s4 + L5) * s4 + L3) * s4 + L1);
    let t2 = s4 * ((L6 * s4 + L4) * s4 + L2);
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;
    k * LN2_HI - ((hfsq - (s * (hfsq + r) + k * LN2_LO)) - f)
}
pub fn log10(x: f64) -> f64 {
    log(x) * std::f64::consts::LOG10_E
}
const SMALLEST_NORMAL: f64 = 2.225_073_858_507_201_4e-308;
fn normalize(x: f64) -> (f64, i64) {
    if x.abs() < SMALLEST_NORMAL {
        return (x * (1u64 << 52) as f64, -52);
    }
    (x, 0)
}
pub fn frexp(f: f64) -> (f64, i64) {
    if f == 0.0 || !f.is_finite() {
        return (f, 0);
    }
    let (f, mut exp) = normalize(f);
    let mut x = f.to_bits();
    exp += ((x >> 52) & 0x7FF) as i64 - 1023 + 1;
    x &= !(0x7FF << 52);
    x |= 1022 << 52;
    (f64::from_bits(x), exp)
}
pub fn ldexp(frac: f64, exp: i64) -> f64 {
    if frac == 0.0 || !frac.is_finite() {
        return frac;
    }
    let (frac, e) = normalize(frac);
    let mut exp = exp.wrapping_add(e);
    let mut x = frac.to_bits();
    exp = exp.wrapping_add(((x >> 52) & 0x7FF) as i64 - 1023);
    if exp < -1075 {
        return 0f64.copysign(frac);
    }
    if exp > 1023 {
        return if frac < 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let mut m = 1.0;
    if exp < -1022 {
        exp += 53;
        m = 1.0 / (1u64 << 53) as f64;
    }
    x &= !(0x7FF << 52);
    x |= ((exp + 1023) as u64) << 52;
    m * f64::from_bits(x)
}
pub fn log2(x: f64) -> f64 {
    let (frac, exp) = frexp(x);
    if frac == 0.5 {
        return (exp - 1) as f64;
    }
    log(frac) * 1.4426950408889634 + exp as f64
}
pub fn modf(f: f64) -> (f64, f64) {
    let integer = f.trunc();
    (integer, (f - integer).copysign(f))
}
fn is_odd_int(x: f64) -> bool {
    if x.abs() >= (1u64 << 53) as f64 {
        return false;
    }
    let (xi, xf) = modf(x);
    xf == 0.0 && f64_to_i64(xi) & 1 == 1
}
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if y == 1.0 {
        return x;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        if y < 0.0 {
            if x.is_sign_negative() && is_odd_int(y) {
                return f64::NEG_INFINITY;
            }
            return f64::INFINITY;
        }
        if y > 0.0 {
            if x.is_sign_negative() && is_odd_int(y) {
                return x;
            }
            return 0.0;
        }
    } else if y.is_infinite() {
        if x == -1.0 {
            return 1.0;
        }
        if (x.abs() < 1.0) == (y == f64::INFINITY) {
            return 0.0;
        }
        return f64::INFINITY;
    } else if x.is_infinite() {
        if x == f64::NEG_INFINITY {
            return pow(1.0 / x, -y);
        }
        if y < 0.0 {
            return 0.0;
        }
        if y > 0.0 {
            return f64::INFINITY;
        }
    } else if y == 0.5 {
        return x.sqrt();
    } else if y == -0.5 {
        return 1.0 / x.sqrt();
    }
    let (mut yi, mut yf) = modf(y.abs());
    if yf != 0.0 && x < 0.0 {
        return f64::NAN;
    }
    if yi >= 9.223_372_036_854_775_808e18 {
        if x == -1.0 {
            return 1.0;
        }
        if (x.abs() < 1.0) == (y > 0.0) {
            return 0.0;
        }
        return f64::INFINITY;
    }
    let mut a1 = 1.0;
    let mut ae: i64 = 0;
    if yf != 0.0 {
        if yf > 0.5 {
            yf -= 1.0;
            yi += 1.0;
        }
        a1 = exp(yf * log(x));
    }
    let (mut x1, mut xe) = frexp(x);
    let mut i = yi as i64;
    while i != 0 {
        if !(-(1 << 12)..=1 << 12).contains(&xe) {
            ae += xe;
            break;
        }
        if i & 1 == 1 {
            a1 *= x1;
            ae += xe;
        }
        x1 *= x1;
        xe <<= 1;
        if x1 < 0.5 {
            x1 += x1;
            xe -= 1;
        }
        i >>= 1;
    }
    if y < 0.0 {
        a1 = 1.0 / a1;
        ae = -ae;
    }
    ldexp(a1, ae)
}
pub fn cbrt(x: f64) -> f64 {
    const B1: u64 = 715_094_163;
    const B2: u64 = 696_219_795;
    const C: f64 = 5.428_571_428_571_428_159_06e-01;
    const D: f64 = -7.053_061_224_489_796_110_50e-01;
    const E: f64 = 1.414_285_714_285_714_368_19e+00;
    const F: f64 = 1.607_142_857_142_857_206_30e+00;
    const G: f64 = 3.571_428_571_428_571_507_87e-01;
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let sign = x < 0.0;
    let x = x.abs();
    let mut t = f64::from_bits(x.to_bits() / 3 + (B1 << 32));
    if x < SMALLEST_NORMAL {
        t = (1u64 << 54) as f64;
        t *= x;
        t = f64::from_bits(t.to_bits() / 3 + (B2 << 32));
    }
    let mut r = t * t / x;
    let mut s = C + r * t;
    t *= G + F / (s + E + D / s);
    t = f64::from_bits((t.to_bits() & (0xF_FFFF_FFFC << 28)) + (1 << 30));
    s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t += t * r;
    if sign { -t } else { t }
}
pub fn tan(x: f64) -> f64 {
    const P: [f64; 3] = [
        -1.30936939181383777646e4,
        1.15351664838587416140e6,
        -1.79565251976484877988e7,
    ];
    const Q: [f64; 5] = [
        1.0,
        1.36812963470692954678e4,
        -1.32089234440210967447e6,
        2.50083801823357915839e7,
        -5.38695755929454629881e7,
    ];
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let sign = x < 0.0;
    let (j, z) = octant(x.abs());
    let zz = z * z;
    let mut y = if zz > 1e-14 {
        z + z
            * (zz * (((P[0] * zz) + P[1]) * zz + P[2])
                / ((((zz + Q[1]) * zz + Q[2]) * zz + Q[3]) * zz + Q[4]))
    } else {
        z
    };
    if j & 2 == 2 {
        y = -1.0 / y;
    }
    if sign { -y } else { y }
}
pub fn atan2(y: f64, x: f64) -> f64 {
    use std::f64::consts::PI;
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return f64::copysign(0.0, y);
        }
        return f64::copysign(PI, y);
    }
    if x == 0.0 {
        return f64::copysign(PI / 2.0, y);
    }
    if x.is_infinite() {
        if x == f64::INFINITY {
            if y.is_infinite() {
                return f64::copysign(PI / 4.0, y);
            }
            return f64::copysign(0.0, y);
        }
        if y.is_infinite() {
            return f64::copysign(3.0 * PI / 4.0, y);
        }
        return f64::copysign(PI, y);
    }
    if y.is_infinite() {
        return f64::copysign(PI / 2.0, y);
    }
    let q = atan(y / x);
    if x < 0.0 {
        if q <= 0.0 {
            return q + PI;
        }
        return q - PI;
    }
    q
}
pub fn erf(x: f64) -> f64 {
    const ERX: f64 = f64::from_bits(0x3feb0ac160000000);
    const EFX: f64 = f64::from_bits(0x3fc06eba8214db69);
    const EFX8: f64 = f64::from_bits(0x3ff06eba8214db69);
    const PP0: f64 = f64::from_bits(0x3fc06eba8214db68);
    const PP1: f64 = f64::from_bits(0xbfd4cd7d691cb913);
    const PP2: f64 = f64::from_bits(0xbf9d2a51dbd7194f);
    const PP3: f64 = f64::from_bits(0xbf77a291236668e4);
    const PP4: f64 = f64::from_bits(0xbef8ead6120016ac);
    const QQ1: f64 = f64::from_bits(0x3fd97779cddadc09);
    const QQ2: f64 = f64::from_bits(0x3fb0a54c5536ceba);
    const QQ3: f64 = f64::from_bits(0x3f74d022c4d36b0f);
    const QQ4: f64 = f64::from_bits(0x3f215dc9221c1a10);
    const QQ5: f64 = f64::from_bits(0xbed09c4342a26120);
    const PA0: f64 = f64::from_bits(0xbf6359b8bef77538);
    const PA1: f64 = f64::from_bits(0x3fda8d00ad92b34d);
    const PA2: f64 = f64::from_bits(0xbfd7d240fbb8c3f1);
    const PA3: f64 = f64::from_bits(0x3fd45fca805120e4);
    const PA4: f64 = f64::from_bits(0xbfbc63983d3e28ec);
    const PA5: f64 = f64::from_bits(0x3fa22a36599795eb);
    const PA6: f64 = f64::from_bits(0xbf61bf380a96073f);
    const QA1: f64 = f64::from_bits(0x3fbb3e6618eee323);
    const QA2: f64 = f64::from_bits(0x3fe14af092eb6f33);
    const QA3: f64 = f64::from_bits(0x3fb2635cd99fe9a7);
    const QA4: f64 = f64::from_bits(0x3fc02660e763351f);
    const QA5: f64 = f64::from_bits(0x3f8bedc26b51dd1c);
    const QA6: f64 = f64::from_bits(0x3f888b545735151d);
    const RA0: f64 = f64::from_bits(0xbf843412600d6435);
    const RA1: f64 = f64::from_bits(0xbfe63416e4ba7360);
    const RA2: f64 = f64::from_bits(0xc0251e0441b0e726);
    const RA3: f64 = f64::from_bits(0xc04f300ae4cba38d);
    const RA4: f64 = f64::from_bits(0xc0644cb184282266);
    const RA5: f64 = f64::from_bits(0xc067135cebccabb2);
    const RA6: f64 = f64::from_bits(0xc054526557e4d2f2);
    const RA7: f64 = f64::from_bits(0xc023a0efc69ac25c);
    const SA1: f64 = f64::from_bits(0x4033a6b9bd707687);
    const SA2: f64 = f64::from_bits(0x4061350c526ae721);
    const SA3: f64 = f64::from_bits(0x407b290dd58a1a71);
    const SA4: f64 = f64::from_bits(0x40842b1921ec2868);
    const SA5: f64 = f64::from_bits(0x407ad02157700314);
    const SA6: f64 = f64::from_bits(0x405b28a3ee48ae2c);
    const SA7: f64 = f64::from_bits(0x401a47ef8e484a93);
    const SA8: f64 = f64::from_bits(0xbfaeeff2ee749a62);
    const RB0: f64 = f64::from_bits(0xbf84341239e86f4a);
    const RB1: f64 = f64::from_bits(0xbfe993ba70c285de);
    const RB2: f64 = f64::from_bits(0xc031c209555f995a);
    const RB3: f64 = f64::from_bits(0xc064145d43c5ed98);
    const RB4: f64 = f64::from_bits(0xc083ec881375f228);
    const RB5: f64 = f64::from_bits(0xc09004616a2e5992);
    const RB6: f64 = f64::from_bits(0xc07e384e9bdc383f);
    const SB1: f64 = f64::from_bits(0x403e568b261d5190);
    const SB2: f64 = f64::from_bits(0x40745cae221b9f0a);
    const SB3: f64 = f64::from_bits(0x409802eb189d5118);
    const SB4: f64 = f64::from_bits(0x40a8ffb7688c246a);
    const SB5: f64 = f64::from_bits(0x40a3f219cedf3be6);
    const SB6: f64 = f64::from_bits(0x407da874e79fe763);
    const SB7: f64 = f64::from_bits(0xc03670e242712d62);
    const VERY_TINY: f64 = f64::from_bits(0x0080000000000000);
    const SMALL: f64 = 1.0 / (1u64 << 28) as f64;
    if x.is_nan() {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return 1.0;
    }
    if x == f64::NEG_INFINITY {
        return -1.0;
    }
    let mut x = x;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if x < 0.84375 {
        let temp = if x < SMALL {
            if x < VERY_TINY {
                0.125 * (8.0 * x + EFX8 * x)
            } else {
                x + EFX * x
            }
        } else {
            let z = x * x;
            let r = PP0 + z * (PP1 + z * (PP2 + z * (PP3 + z * PP4)));
            let s = 1.0 + z * (QQ1 + z * (QQ2 + z * (QQ3 + z * (QQ4 + z * QQ5))));
            let y = r / s;
            x + x * y
        };
        return if sign { -temp } else { temp };
    }
    if x < 1.25 {
        let s = x - 1.0;
        let p = PA0 + s * (PA1 + s * (PA2 + s * (PA3 + s * (PA4 + s * (PA5 + s * PA6)))));
        let q = 1.0 + s * (QA1 + s * (QA2 + s * (QA3 + s * (QA4 + s * (QA5 + s * QA6)))));
        return if sign { -ERX - p / q } else { ERX + p / q };
    }
    if x >= 6.0 {
        return if sign { -1.0 } else { 1.0 };
    }
    let s = 1.0 / (x * x);
    let (r, big_s);
    if x < f64::from_bits(0x4006db6db6db6db7) {
        r = RA0 + s * (RA1 + s * (RA2 + s * (RA3 + s * (RA4 + s * (RA5 + s * (RA6 + s * RA7))))));
        big_s = 1.0
            + s * (SA1
                + s * (SA2 + s * (SA3 + s * (SA4 + s * (SA5 + s * (SA6 + s * (SA7 + s * SA8)))))));
    } else {
        r = RB0 + s * (RB1 + s * (RB2 + s * (RB3 + s * (RB4 + s * (RB5 + s * RB6)))));
        big_s =
            1.0 + s * (SB1 + s * (SB2 + s * (SB3 + s * (SB4 + s * (SB5 + s * (SB6 + s * SB7))))));
    }
    let z = f64::from_bits(x.to_bits() & 0xffff_ffff_0000_0000);
    let e = exp(-z * z - 0.5625) * exp((z - x) * (z + x) + r / big_s);
    if sign { e / x - 1.0 } else { 1.0 - e / x }
}
#[allow(clippy::excessive_precision)]
pub fn log1p(x: f64) -> f64 {
    const SQRT2_M1: f64 = 4.142135623730950488017e-01;
    const SQRT2_HALF_M1: f64 = -2.928932188134524755992e-01;
    const SMALL: f64 = 1.0 / (1u64 << 29) as f64;
    const TINY: f64 = 1.0 / (1u64 << 54) as f64;
    const TWO53: f64 = (1u64 << 53) as f64;
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const LP1: f64 = 6.666666666666735130e-01;
    const LP2: f64 = 3.999999999940941908e-01;
    const LP3: f64 = 2.857142874366239149e-01;
    const LP4: f64 = 2.222219843214978396e-01;
    const LP5: f64 = 1.818357216161805012e-01;
    const LP6: f64 = 1.531383769920937332e-01;
    const LP7: f64 = 1.479819860511658591e-01;
    if x < -1.0 || x.is_nan() {
        return f64::NAN;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    let absx = x.abs();
    let mut f = 0.0;
    let mut iu: u64 = 0;
    let mut k: i64 = 1;
    if absx < SQRT2_M1 {
        if absx < SMALL {
            if absx < TINY {
                return x;
            }
            return x - x * x * 0.5;
        }
        if x > SQRT2_HALF_M1 {
            k = 0;
            f = x;
            iu = 1;
        }
    }
    let mut c = 0.0;
    if k != 0 {
        let mut u;
        if absx < TWO53 {
            u = 1.0 + x;
            iu = u.to_bits();
            k = (iu >> 52) as i64 - 1023;
            if k > 0 {
                c = 1.0 - (u - x);
            } else {
                c = x - (u - 1.0);
            }
            c /= u;
        } else {
            u = x;
            iu = u.to_bits();
            k = (iu >> 52) as i64 - 1023;
            c = 0.0;
        }
        iu &= 0x000f_ffff_ffff_ffff;
        if iu < 0x0006_a09e_667f_3bcd {
            u = f64::from_bits(iu | 0x3ff0_0000_0000_0000);
        } else {
            k += 1;
            u = f64::from_bits(iu | 0x3fe0_0000_0000_0000);
            iu = (0x0010_0000_0000_0000 - iu) >> 2;
        }
        f = u - 1.0;
    }
    let hfsq = 0.5 * f * f;
    let kf = k as f64;
    if iu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c += kf * LN2_LO;
            return kf * LN2_HI + c;
        }
        let r = hfsq * (1.0 - 0.66666666666666666 * f);
        if k == 0 {
            return f - r;
        }
        return kf * LN2_HI - ((r - (kf * LN2_LO + c)) - f);
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let r = z * (LP1 + z * (LP2 + z * (LP3 + z * (LP4 + z * (LP5 + z * (LP6 + z * LP7))))));
    if k == 0 {
        return f - (hfsq - s * (hfsq + r));
    }
    kf * LN2_HI - ((hfsq - (s * (hfsq + r) + (kf * LN2_LO + c))) - f)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_results() {
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(1.0, f64::NAN).is_nan());
        assert_eq!(f64_to_i64(f64::NAN), i64::MIN);
        assert_eq!(f64_to_i64(1e300), i64::MIN);
        assert_eq!(f64_to_i64(-3.9), -3);
        assert_eq!(f64_to_u8(f64::NAN), 0);
        assert_eq!(next_up(1.0), 1.0 + f64::EPSILON);
    }
}
