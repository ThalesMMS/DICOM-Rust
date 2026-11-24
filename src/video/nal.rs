use super::{Error, Kind, malformed};
use std::borrow::Cow;
fn find_start_code(data: &[u8]) -> Option<usize> {
    data.windows(3).position(|w| w == [0, 0, 1])
}
pub(crate) fn split_annex_b(data: &[u8], max_units: i64) -> Result<Vec<&[u8]>, Error> {
    let Some(first) = find_start_code(data) else {
        return Err(malformed("no Annex B start code"));
    };
    if data[..first].iter().any(|&b| b != 0) {
        return Err(malformed("data before the first start code"));
    }
    let mut units: Vec<&[u8]> = Vec::new();
    let mut pos = first + 3;
    while pos <= data.len() {
        let next = find_start_code(&data[pos..]);
        let end = next.map_or(data.len(), |n| pos + n);
        let trimmed = data[pos..end]
            .iter()
            .rposition(|&b| b != 0)
            .map_or(0, |i| i + 1);
        if trimmed > 0 {
            if units.len() as i64 >= max_units {
                return Err(Error::new(
                    Kind::TooManyFragments,
                    format!("more than {max_units} NAL units"),
                ));
            }
            units.push(&data[pos..pos + trimmed]);
        }
        if next.is_none() {
            break;
        }
        pos = end + 3;
    }
    if units.is_empty() {
        return Err(malformed("no NAL units"));
    }
    Ok(units)
}
pub(crate) fn rbsp(nal: &[u8]) -> Cow<'_, [u8]> {
    if !nal.windows(3).any(|w| w == [0, 0, 3]) {
        return Cow::Borrowed(nal);
    }
    let mut out = Vec::with_capacity(nal.len());
    let mut zeros = 0;
    for &b in nal {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        if b == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(b);
    }
    Cow::Owned(out)
}
pub(crate) struct BitReader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub err: Option<Error>,
}
impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader {
            data,
            pos: 0,
            err: None,
        }
    }
    fn truncated(&mut self) {
        if self.err.is_none() {
            self.err = Some(malformed("truncated header"));
        }
    }
    pub fn u(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            if self.pos >= 8 * self.data.len() {
                self.truncated();
                return 0;
            }
            v = v << 1 | u32::from(self.data[self.pos >> 3] >> (7 - (self.pos & 7))) & 1;
            self.pos += 1;
        }
        v
    }
    pub fn flag(&mut self) -> bool {
        self.u(1) == 1
    }
    pub fn skip(&mut self, n: usize) {
        if self.pos + n > 8 * self.data.len() {
            self.pos = 8 * self.data.len();
            self.truncated();
            return;
        }
        self.pos += n;
    }
    pub fn ue(&mut self) -> u32 {
        let mut zeros = 0u32;
        while self.u(1) == 0 {
            if self.err.is_some() {
                return 0;
            }
            zeros += 1;
            if zeros > 31 {
                self.err = Some(malformed("Exp-Golomb code too long"));
                return 0;
            }
        }
        ((1u32 << zeros) - 1).wrapping_add(self.u(zeros))
    }
    pub fn se(&mut self) -> i32 {
        let v = self.ue();
        if v & 1 == 1 {
            (v.wrapping_add(1) / 2) as i32
        } else {
            -((v / 2) as i32)
        }
    }
    pub(crate) fn ue_max(&mut self, limit: u32, field: &str) -> u32 {
        let v = self.ue();
        if v > limit && self.err.is_none() {
            self.err = Some(malformed(format!("{field} {v} exceeds {limit}")));
        }
        v
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exp_golomb() {
        let data = [0b1010_0110, 0b0100_0010, 0b1000_0000];
        let mut r = BitReader::new(&data);
        assert_eq!([r.ue(), r.ue(), r.ue(), r.ue(), r.ue()], [0, 1, 2, 3, 4]);
        assert!(r.err.is_none());
        let mut s = BitReader::new(&[0b0100_1100]);
        assert_eq!((s.se(), s.se()), (1, -1));
        let mut t = BitReader::new(&[0]);
        t.ue();
        assert!(t.err.is_some());
    }
    #[test]
    fn removes_emulation_prevention() {
        assert_eq!(&*rbsp(&[1, 0, 0, 3, 1, 0, 0, 3]), &[1, 0, 0, 1, 0, 0]);
        assert_eq!(&*rbsp(&[0, 0, 1]), &[0, 0, 1]);
    }
}
