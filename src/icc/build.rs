use super::{D50, HEADER_SIZE, pad4};
use crate::fmath;
use std::borrow::Cow;
#[derive(Debug, Clone, PartialEq)]
pub struct MatrixTrc {
    pub description: Cow<'static, str>,
    pub colorants: [[f64; 3]; 3],
    pub gamma: f64,
}
pub const ADOBE_RGB_1998: MatrixTrc = MatrixTrc {
    description: Cow::Borrowed("Adobe RGB (1998) compatible"),
    colorants: [
        [0.6097412, 0.2052765, 0.1491852],
        [0.3111115, 0.6256714, 0.0632172],
        [0.0194702, 0.0608673, 0.7445679],
    ],
    gamma: 563.0 / 256.0,
};
pub(super) fn fixed(v: f64) -> u32 {
    fmath::f64_to_i32((v * 65536.0).round()) as u32
}
pub(super) fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut out = vec![0u8; 20];
    out[0..4].copy_from_slice(b"XYZ ");
    for (i, v) in [x, y, z].into_iter().enumerate() {
        out[8 + 4 * i..12 + 4 * i].copy_from_slice(&fixed(v).to_be_bytes());
    }
    out
}
fn put32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_be_bytes());
}
impl MatrixTrc {
    pub fn build(&self) -> Vec<u8> {
        let mut curve = vec![0u8; 16];
        curve[0..4].copy_from_slice(b"para");
        put32(&mut curve, 12, fixed(self.gamma));
        let text = self.description.as_bytes();
        let mut desc = vec![0u8; 28 + 2 * text.len()];
        desc[0..4].copy_from_slice(b"mluc");
        put32(&mut desc, 8, 1);
        put32(&mut desc, 12, 12);
        desc[16..20].copy_from_slice(b"enUS");
        put32(&mut desc, 20, (2 * text.len()) as u32);
        put32(&mut desc, 24, 28);
        for (i, &c) in text.iter().enumerate() {
            desc[29 + 2 * i] = c;
        }
        let c = &self.colorants;
        let tags: [(&[u8; 4], Vec<u8>); 8] = [
            (b"desc", desc),
            (b"wtpt", xyz(D50[0], D50[1], D50[2])),
            (b"rXYZ", xyz(c[0][0], c[1][0], c[2][0])),
            (b"gXYZ", xyz(c[0][1], c[1][1], c[2][1])),
            (b"bXYZ", xyz(c[0][2], c[1][2], c[2][2])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ];
        let table_end = HEADER_SIZE as usize + 4 + 12 * tags.len();
        let size = table_end + tags.iter().map(|(_, v)| pad4(v.len())).sum::<usize>();
        let mut out = vec![0u8; size];
        put32(&mut out, 0, size as u32);
        put32(&mut out, 8, 0x04300000);
        out[12..16].copy_from_slice(b"mntr");
        out[16..20].copy_from_slice(b"RGB ");
        out[20..24].copy_from_slice(b"XYZ ");
        out[36..40].copy_from_slice(b"acsp");
        for (i, &v) in D50.iter().enumerate() {
            put32(&mut out, 68 + 4 * i, fixed(v));
        }
        put32(&mut out, 128, tags.len() as u32);
        let mut at = table_end;
        for (i, (signature, value)) in tags.iter().enumerate() {
            let entry = 132 + 12 * i;
            out[entry..entry + 4].copy_from_slice(*signature);
            put32(&mut out, entry + 4, at as u32);
            put32(&mut out, entry + 8, value.len() as u32);
            out[at..at + value.len()].copy_from_slice(value);
            at += pad4(value.len());
        }
        out
    }
}
