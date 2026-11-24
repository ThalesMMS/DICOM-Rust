use super::Error;
const SIG: u8 = 1;
const NEW: u8 = 2;
const NEG: u8 = 4;
const GROUP: usize = 4;
struct ForwardBits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    bits: u32,
    unstuff: bool,
}
impl ForwardBits<'_> {
    #[allow(clippy::arithmetic_side_effects)]
    fn bit(&mut self) -> u32 {
        if self.bits == 0 {
            let b = match self.data.get(self.pos) {
                Some(&b) => {
                    self.pos += 1;
                    b
                }
                None => 0,
            };
            if self.unstuff {
                self.acc = (b & 0x7F) as u32;
                self.bits = 7;
            } else {
                self.acc = b as u32;
                self.bits = 8;
            }
            self.unstuff = b == 0xFF;
        }
        let v = self.acc & 1;
        self.acc >>= 1;
        self.bits -= 1;
        v
    }
}
struct ReverseBits<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u32,
    bits: u32,
    unstuff: bool,
}
impl ReverseBits<'_> {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn bit(&mut self) -> u32 {
        if self.bits == 0 {
            let b = if self.pos > 0 {
                self.pos -= 1;
                self.data[self.pos]
            } else {
                0
            };
            if self.unstuff && b & 0x7F == 0x7F {
                self.acc = (b & 0x7F) as u32;
                self.bits = 7;
            } else {
                self.acc = b as u32;
                self.bits = 8;
            }
            self.unstuff = b > 0x8F;
        }
        let v = self.acc & 1;
        self.acc >>= 1;
        self.bits -= 1;
        v
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn refine(
    segment: &[u8],
    width: usize,
    height: usize,
    values: &mut [i32],
    passes: u32,
    causal: bool,
    plane: u32,
) -> Result<(), Error> {
    if plane == 0 || plane > 30 || !(1..=2).contains(&passes) || values.len() != width * height {
        return Err(Error::malformed(format_args!(
            "{passes} refinement passes below bit-plane {plane}"
        )));
    }
    let stride = width + 2;
    let mut state = vec![0u8; stride * (height + 2)];
    for y in 0..height {
        let row = &values[y * width..][..width];
        let st = &mut state[(y + 1) * stride + 1..][..width];
        for (s, &v) in st.iter_mut().zip(row) {
            if v != 0 {
                *s = SIG;
            }
        }
    }
    let mut spp = ForwardBits {
        data: segment,
        pos: 0,
        acc: 0,
        bits: 0,
        unstuff: false,
    };
    let mut fresh = [0usize; 4 * GROUP];
    for y0 in (0..height).step_by(4) {
        let y1 = (y0 + 4).min(height);
        for gx in (0..width).step_by(GROUP) {
            let gx1 = (gx + GROUP).min(width);
            let mut count = 0;
            for x in gx..gx1 {
                for y in y0..y1 {
                    let at = (y + 1) * stride + x + 1;
                    if state[at] != 0 {
                        continue;
                    }
                    let above = &state[at - stride - 1..][..3];
                    let level = &state[at - 1..][..3];
                    let mut near = above.iter().chain(level).any(|&s| s != 0);
                    if !near && !(causal && y % 4 == 3) {
                        near = state[at + stride - 1..][..3].iter().any(|&s| s != 0);
                    }
                    if near && spp.bit() == 1 {
                        state[at] = NEW;
                        fresh[count] = at;
                        count += 1;
                    }
                }
            }
            for &at in &fresh[..count] {
                if spp.bit() == 1 {
                    state[at] |= NEG;
                }
            }
        }
    }
    let refined = passes == 2;
    if refined {
        let mut mrp = ReverseBits {
            data: segment,
            pos: segment.len(),
            acc: 0,
            bits: 0,
            unstuff: true,
        };
        for y0 in (0..height).step_by(4) {
            let y1 = (y0 + 4).min(height);
            for x in 0..width {
                for y in y0..y1 {
                    let k = y * width + x;
                    if state[(y + 1) * stride + x + 1] & SIG == 0 {
                        continue;
                    }
                    let v = values[k];
                    let m = (v.unsigned_abs() << 1) | mrp.bit();
                    values[k] = if v < 0 { -(m as i32) } else { m as i32 };
                }
            }
        }
    }
    let limit = 1u64 << 30;
    for y in 0..height {
        for x in 0..width {
            let k = y * width + x;
            let s = state[(y + 1) * stride + x + 1];
            let (m, q, negative) = if s & SIG != 0 {
                let q = if refined { plane - 1 } else { plane };
                (values[k].unsigned_abs() as u64, q, values[k] < 0)
            } else if s & NEW != 0 {
                (1, plane - 1, s & NEG != 0)
            } else {
                continue;
            };
            let halves = (2 * m + 1) << q;
            if halves >= limit {
                return Err(Error::malformed("refined value past what an int32 holds"));
            }
            values[k] = if negative {
                -(halves as i32)
            } else {
                halves as i32
            };
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forward_bits_unstuff_after_ff_and_read_zeros_past_the_end() {
        let mut r = ForwardBits {
            data: &[0xFF, 0x7F],
            pos: 0,
            acc: 0,
            bits: 0,
            unstuff: false,
        };
        let bits: Vec<u32> = (0..18).map(|_| r.bit()).collect();
        assert_eq!(bits[..8], [1; 8]);
        assert_eq!(bits[8..15], [1; 7]);
        assert_eq!(bits[15..], [0; 3]);
    }
    #[test]
    fn reverse_bits_unstuff_a_7f_byte_after_one_above_8f() {
        let mut r = ReverseBits {
            data: &[0xFF, 0x90],
            pos: 2,
            acc: 0,
            bits: 0,
            unstuff: false,
        };
        let bits: Vec<u32> = (0..17).map(|_| r.bit()).collect();
        assert_eq!(bits[..8], [0, 0, 0, 0, 1, 0, 0, 1]);
        assert_eq!(bits[8..15], [1; 7]);
        assert_eq!(bits[15..], [0; 2]);
    }
    #[test]
    fn sigprop_and_magref_refine_by_one_plane() {
        let mut values = vec![0i32; 16];
        values[0] = 1;
        refine(&[0x21, 0x01], 4, 4, &mut values, 2, false, 1).unwrap();
        let mut want = vec![0i32; 16];
        want[0] = 7;
        want[4] = -3;
        assert_eq!(values, want);
    }
    #[test]
    fn sigprop_alone_keeps_cleanup_samples_at_their_plane() {
        let mut values = vec![0i32; 16];
        values[0] = -1;
        refine(&[0x01], 4, 4, &mut values, 1, false, 2).unwrap();
        assert_eq!(values[0], -12);
        assert_eq!(values[4], 6);
        assert!(
            values
                .iter()
                .enumerate()
                .all(|(k, &v)| k == 0 || k == 4 || v == 0)
        );
    }
    #[test]
    fn causal_mode_ignores_the_stripe_below() {
        let mut values = vec![0i32; 32];
        values[16] = 1;
        let mut causal = values.clone();
        refine(&[0x01], 4, 8, &mut values, 1, false, 1).unwrap();
        refine(&[0x01], 4, 8, &mut causal, 1, true, 1).unwrap();
        assert_eq!(values[12], 3);
        assert_eq!(causal[12], 0);
    }
    #[test]
    fn refuses_refinement_below_plane_zero() {
        let mut values = vec![1i32; 4];
        assert!(refine(&[], 2, 2, &mut values, 2, false, 0).is_err());
    }
}
