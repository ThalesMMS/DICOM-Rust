#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::num::Wrapping;
type W = Wrapping<i32>;
pub(crate) type Block = [i32; 64];
const COS1: u64 = 1130768441178740757;
const SIN1: u64 = 224923827593068887;
const COS3: u64 = 958619196450722178;
const SIN3: u64 = 640528868967736374;
const SQRT2: u64 = 1630477228166597777;
const SQRT2_COS6: u64 = 623956622067911264;
const SQRT2_SIN6: u64 = 1506364539328854985;
const SQRT2INV: u64 = 815238614083298888;
const SQRT2INV_COS6: u64 = 311978311033955632;
const SQRT2INV_SIN6: u64 = 753182269664427492;
#[allow(clippy::arithmetic_side_effects)]
const fn c(x: u64, bits: u32) -> W {
    Wrapping(((x + (1 << (59 - bits))) >> (60 - bits)) as i32)
}
#[inline]
fn dct_box(x0: W, x1: W, kcos: W, ksin: W) -> (W, W) {
    let ksum = kcos * (x0 + x1);
    (ksum + (ksin - kcos) * x1, ksum - (kcos + ksin) * x0)
}
#[inline]
fn w(v: i32) -> W {
    Wrapping(v)
}
pub(crate) fn fdct(b: &mut Block) {
    fdct_cols(b);
    fdct_rows(b);
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn fdct_cols(b: &mut Block) {
    for i in 0..8 {
        let mut x0 = w(b[i]);
        let mut x1 = w(b[8 + i]);
        let mut x2 = w(b[16 + i]);
        let mut x3 = w(b[24 + i]);
        let mut x4 = w(b[32 + i]);
        let mut x5 = w(b[40 + i]);
        let mut x6 = w(b[48 + i]);
        let mut x7 = w(b[56 + i]);
        (x0, x7) = (x0 + x7, x0 - x7);
        (x1, x6) = (x1 + x6, x1 - x6);
        (x2, x5) = (x2 + x5, x2 - x5);
        (x3, x4) = (x3 + x4, x3 - x4);
        (x4, x7) = dct_box(x4, x7, c(COS3, 18), c(SIN3, 18));
        (x5, x6) = dct_box(x5, x6, c(COS1, 18), c(SIN1, 18));
        (x0, x3) = (x0 + x3, x0 - x3);
        (x1, x2) = (x1 + x2, x1 - x2);
        (x2, x3) = dct_box(x2, x3, c(SQRT2_COS6, 18), c(SQRT2_SIN6, 18));
        (x0, x1) = (x0 + x1, x0 - x1);
        b[i] = ((x0 - w(128 * 8)) << 18).0;
        b[32 + i] = (x1 << 18).0;
        b[16 + i] = x2.0;
        b[48 + i] = x3.0;
        (x4, x6) = (x4 + x6, x4 - x6);
        (x7, x5) = (x7 + x5, x7 - x5);
        x5 = (x5 >> 12) * c(SQRT2, 12);
        x6 = (x6 >> 12) * c(SQRT2, 12);
        (x7, x4) = (x7 + x4, x7 - x4);
        b[8 + i] = x7.0;
        b[24 + i] = x5.0;
        b[40 + i] = x6.0;
        b[56 + i] = x4.0;
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn fdct_rows(b: &mut Block) {
    for i in 0..8 {
        let x = &mut b[8 * i..8 * i + 8];
        let mut x0 = w(x[0]);
        let mut x1 = w(x[1]);
        let mut x2 = w(x[2]);
        let mut x3 = w(x[3]);
        let mut x4 = w(x[4]);
        let mut x5 = w(x[5]);
        let mut x6 = w(x[6]);
        let mut x7 = w(x[7]);
        (x0, x7) = (x0 + x7, x0 - x7);
        (x1, x6) = (x1 + x6, x1 - x6);
        (x2, x5) = (x2 + x5, x2 - x5);
        (x3, x4) = (x3 + x4, x3 - x4);
        (x4, x7) = dct_box(x4 >> 14, x7 >> 14, c(COS3, 14), c(SIN3, 14));
        (x5, x6) = dct_box(x5 >> 14, x6 >> 14, c(COS1, 14), c(SIN1, 14));
        (x0, x3) = (x0 + x3, x0 - x3);
        (x1, x2) = (x1 + x2, x1 - x2);
        (x2, x3) = dct_box(x2 >> 14, x3 >> 14, c(SQRT2_COS6, 14), c(SQRT2_SIN6, 14));
        (x0, x1) = (x0 + x1, x0 - x1);
        (x4, x6) = (x4 + x6, x4 - x6);
        (x7, x5) = (x7 + x5, x7 - x5);
        x5 = (x5 >> 14) * c(SQRT2, 14);
        x6 = (x6 >> 14) * c(SQRT2, 14);
        (x7, x4) = (x7 + x4, x7 - x4);
        let half = w(1 << 17);
        x0 = (x0 + half) >> 18;
        x1 = (x1 + half) >> 18;
        x2 = (x2 + half) >> 18;
        x3 = (x3 + half) >> 18;
        x4 = (x4 + half) >> 18;
        x5 = (x5 + half) >> 18;
        x6 = (x6 + half) >> 18;
        x7 = (x7 + half) >> 18;
        x[0] = x0.0;
        x[1] = x7.0;
        x[2] = x2.0;
        x[3] = x5.0;
        x[4] = x1.0;
        x[5] = x6.0;
        x[6] = x3.0;
        x[7] = x4.0;
    }
}
pub(crate) fn idct(b: &mut Block) {
    idct_rows(b);
    idct_cols(b);
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn idct_rows(b: &mut Block) {
    for i in 0..8 {
        let x = &mut b[8 * i..8 * i + 8];
        let mut x0 = w(x[0]);
        let mut x7 = w(x[1]);
        let mut x2 = w(x[2]);
        let mut x5 = w(x[3]);
        let mut x1 = w(x[4]);
        let mut x6 = w(x[5]);
        let mut x3 = w(x[6]);
        let mut x4 = w(x[7]);
        x0 <<= 17;
        x1 <<= 17;
        (x0, x1) = (x0 + x1, x0 - x1);
        (x2, x3) = dct_box(x2, x3, c(SQRT2INV_COS6, 18), -c(SQRT2INV_SIN6, 18));
        (x1, x2) = (x1 + x2, x1 - x2);
        (x0, x3) = (x0 + x3, x0 - x3);
        x4 <<= 7;
        x7 <<= 7;
        (x7, x4) = (x7 + x4, x7 - x4);
        x6 *= c(SQRT2INV, 8);
        x5 *= c(SQRT2INV, 8);
        (x7, x5) = (x7 + x5, x7 - x5);
        (x4, x6) = (x4 + x6, x4 - x6);
        (x4, x7) = dct_box(x4 >> 2, x7 >> 2, c(COS3, 12), -c(SIN3, 12));
        (x5, x6) = dct_box(x5 >> 2, x6 >> 2, c(COS1, 12), -c(SIN1, 12));
        (x0, x7) = (x0 + x7, x0 - x7);
        (x1, x6) = (x1 + x6, x1 - x6);
        (x2, x5) = (x2 + x5, x2 - x5);
        (x3, x4) = (x3 + x4, x3 - x4);
        x[0] = x0.0;
        x[1] = x1.0;
        x[2] = x2.0;
        x[3] = x3.0;
        x[4] = x4.0;
        x[5] = x5.0;
        x[6] = x6.0;
        x[7] = x7.0;
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn idct_cols(b: &mut Block) {
    for i in 0..8 {
        let mut x0 = w(b[i]);
        let mut x7 = w(b[8 + i]);
        let mut x2 = w(b[16 + i]);
        let mut x5 = w(b[24 + i]);
        let mut x1 = w(b[32 + i]);
        let mut x6 = w(b[40 + i]);
        let mut x3 = w(b[48 + i]);
        let mut x4 = w(b[56 + i]);
        x0 += w(1 << 19);
        (x0, x1) = ((x0 + x1) >> 2, (x0 - x1) >> 2);
        (x2, x3) = dct_box(
            x2 >> 13,
            x3 >> 13,
            c(SQRT2INV_COS6, 12),
            -c(SQRT2INV_SIN6, 12),
        );
        (x1, x2) = (x1 + x2, x1 - x2);
        (x0, x3) = (x0 + x3, x0 - x3);
        (x7, x4) = (x7 + x4, x7 - x4);
        x5 = (x5 >> 13) * c(SQRT2INV, 14);
        x6 = (x6 >> 13) * c(SQRT2INV, 14);
        (x7, x5) = (x7 + x5, x7 - x5);
        (x4, x6) = (x4 + x6, x4 - x6);
        (x4, x7) = dct_box(x4 >> 14, x7 >> 14, c(COS3, 12), -c(SIN3, 12));
        (x5, x6) = dct_box(x5 >> 14, x6 >> 14, c(COS1, 12), -c(SIN1, 12));
        (x0, x7) = (x0 + x7, x0 - x7);
        (x1, x6) = (x1 + x6, x1 - x6);
        (x2, x5) = (x2 + x5, x2 - x5);
        (x3, x4) = (x3 + x4, x3 - x4);
        b[i] = (x0 >> 18).0;
        b[8 + i] = (x1 >> 18).0;
        b[16 + i] = (x2 >> 18).0;
        b[24 + i] = (x3 >> 18).0;
        b[32 + i] = (x4 >> 18).0;
        b[40 + i] = (x5 >> 18).0;
        b[48 + i] = (x6 >> 18).0;
        b[56 + i] = (x7 >> 18).0;
    }
}
