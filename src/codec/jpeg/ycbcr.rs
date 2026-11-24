#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::stdjpeg::image::{Ratio, YCbCr};
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(super) fn ycbcr_to_rgb_frame(img: &YCbCr, rows: usize, columns: usize) -> Option<Vec<u8>> {
    if img.width != columns || img.height != rows {
        return None;
    }
    let (h_factor, v_factor) = match img.ratio {
        Ratio::R444 => (1, 1),
        Ratio::R422 => (2, 1),
        Ratio::R420 => (2, 2),
        _ => return None,
    };
    let mut out = vec![0u8; rows * columns * 3];
    let mut cb_row = vec![0u8; columns];
    let mut cr_row = vec![0u8; columns];
    let chroma_width = columns.div_ceil(h_factor);
    let chroma_height = rows.div_ceil(v_factor);
    let row = |plane: &[u8], r: usize| -> Vec<u8> {
        plane[r * img.c_stride..r * img.c_stride + chroma_width].to_vec()
    };
    for y in 0..rows {
        if v_factor == 1 {
            upsample_row(&mut cb_row, &row(&img.cb, y), h_factor);
            upsample_row(&mut cr_row, &row(&img.cr, y), h_factor);
        } else {
            let cy = y / 2;
            let mut near = cy as isize - 1;
            if y % 2 == 1 {
                near = cy as isize + 1;
            }
            let near = near.clamp(0, chroma_height as isize - 1) as usize;
            upsample_2d(&mut cb_row, &row(&img.cb, cy), &row(&img.cb, near));
            upsample_2d(&mut cr_row, &row(&img.cr, cy), &row(&img.cr, near));
        }
        let luma = &img.y[y * img.y_stride..y * img.y_stride + columns];
        let o = &mut out[y * columns * 3..(y + 1) * columns * 3];
        for x in 0..columns {
            let (r, g, b) = ycbcr_to_rgb(luma[x], cb_row[x], cr_row[x]);
            o[3 * x] = r;
            o[3 * x + 1] = g;
            o[3 * x + 2] = b;
        }
    }
    Some(out)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn upsample_row(dst: &mut [u8], src: &[u8], factor: usize) {
    if factor == 1 {
        let n = dst.len().min(src.len());
        dst[..n].copy_from_slice(&src[..n]);
        return;
    }
    let n = src.len();
    for i in 0..n {
        let c = src[i] as i32 * 3;
        let mut left = src[i] as i32;
        let mut right = src[i] as i32;
        if i > 0 {
            left = src[i - 1] as i32;
        }
        if i + 1 < n {
            right = src[i + 1] as i32;
        }
        if 2 * i < dst.len() {
            if i == 0 {
                dst[0] = src[0];
            } else {
                dst[2 * i] = ((c + left + 1) >> 2) as u8;
            }
        }
        if 2 * i + 1 < dst.len() {
            if i == n - 1 {
                dst[2 * i + 1] = src[i];
            } else {
                dst[2 * i + 1] = ((c + right + 2) >> 2) as u8;
            }
        }
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn upsample_2d(dst: &mut [u8], cur: &[u8], near: &[u8]) {
    let n = cur.len();
    let col = |i: usize| cur[i] as i32 * 3 + near[i] as i32;
    for i in 0..n {
        let this = col(i);
        let mut last = this;
        let mut next = this;
        if i > 0 {
            last = col(i - 1);
        }
        if i + 1 < n {
            next = col(i + 1);
        }
        if 2 * i < dst.len() {
            if i == 0 {
                dst[0] = ((this * 4 + 8) >> 4) as u8;
            } else {
                dst[2 * i] = ((this * 3 + last + 8) >> 4) as u8;
            }
        }
        if 2 * i + 1 < dst.len() {
            if i == n - 1 {
                dst[2 * i + 1] = ((this * 4 + 7) >> 4) as u8;
            } else {
                dst[2 * i + 1] = ((this * 3 + next + 7) >> 4) as u8;
            }
        }
    }
}
const FIX_SHIFT: i32 = 16;
const FIX_HALF: i32 = 1 << (FIX_SHIFT - 1);
const CR_TO_R: i32 = 91881;
const CB_TO_G: i32 = -22554;
const CR_TO_G: i32 = -46802;
const CB_TO_B: i32 = 116130;
#[allow(clippy::arithmetic_side_effects)]
fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let yy = y as i32;
    let cbv = cb as i32 - 128;
    let crv = cr as i32 - 128;
    let r = yy + ((CR_TO_R * crv + FIX_HALF) >> FIX_SHIFT);
    let g = yy + ((CB_TO_G * cbv + CR_TO_G * crv + FIX_HALF) >> FIX_SHIFT);
    let b = yy + ((CB_TO_B * cbv + FIX_HALF) >> FIX_SHIFT);
    (clamp8(r), clamp8(g), clamp8(b))
}
fn clamp8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}
