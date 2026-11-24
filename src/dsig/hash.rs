use md5::Md5;
use sha2::{Digest, Sha256, Sha384, Sha512};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacAlgorithm {
    Ripemd160,
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}
impl MacAlgorithm {
    pub fn parse(term: &str) -> Option<MacAlgorithm> {
        Some(match term {
            "RIPEMD160" => MacAlgorithm::Ripemd160,
            "MD5" => MacAlgorithm::Md5,
            "SHA1" => MacAlgorithm::Sha1,
            "SHA256" => MacAlgorithm::Sha256,
            "SHA384" => MacAlgorithm::Sha384,
            "SHA512" => MacAlgorithm::Sha512,
            _ => return None,
        })
    }
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            MacAlgorithm::Ripemd160 => ripemd160(data).to_vec(),
            MacAlgorithm::Md5 => Md5::digest(data).to_vec(),
            MacAlgorithm::Sha1 => sha1(data).to_vec(),
            MacAlgorithm::Sha256 => Sha256::digest(data).to_vec(),
            MacAlgorithm::Sha384 => Sha384::digest(data).to_vec(),
            MacAlgorithm::Sha512 => Sha512::digest(data).to_vec(),
        }
    }
    fn oid(self) -> &'static [u8] {
        match self {
            MacAlgorithm::Ripemd160 => &[0x2B, 0x24, 0x03, 0x02, 0x01],
            MacAlgorithm::Md5 => &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x02, 0x05],
            MacAlgorithm::Sha1 => &[0x2B, 0x0E, 0x03, 0x02, 0x1A],
            MacAlgorithm::Sha256 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01],
            MacAlgorithm::Sha384 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02],
            MacAlgorithm::Sha512 => &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03],
        }
    }
    pub(crate) fn digest_info(self, mac: &[u8], null_parameters: bool) -> Vec<u8> {
        let oid = self.oid();
        let mut algorithm = vec![0x06, oid.len() as u8];
        algorithm.extend_from_slice(oid);
        if null_parameters {
            algorithm.extend_from_slice(&[0x05, 0x00]);
        }
        let mut body = vec![0x30, algorithm.len() as u8];
        body.extend_from_slice(&algorithm);
        body.extend_from_slice(&[0x04, mac.len() as u8]);
        body.extend_from_slice(mac);
        let mut out = vec![0x30, body.len() as u8];
        out.extend_from_slice(&body);
        out
    }
}
fn padded(data: &[u8], big_endian_length: bool) -> Vec<u8> {
    let bits = (data.len() as u64).wrapping_mul(8);
    let mut out = data.to_vec();
    out.push(0x80);
    while out.len() % 64 != 56 {
        out.push(0);
    }
    if big_endian_length {
        out.extend_from_slice(&bits.to_be_bytes());
    } else {
        out.extend_from_slice(&bits.to_le_bytes());
    }
    out
}
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    for block in padded(data, true).as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i / 20 {
                0 => ((b & c) | (!b & d), 0x5A827999),
                1 => (b ^ c ^ d, 0x6ED9EBA1),
                2 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}
const RMD_R: [[usize; 16]; 5] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5, 2, 14, 11, 8],
    [3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12],
    [1, 9, 11, 10, 0, 8, 12, 4, 13, 3, 7, 15, 14, 5, 6, 2],
    [4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13],
];
const RMD_RP: [[usize; 16]; 5] = [
    [5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12],
    [6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12, 4, 9, 1, 2],
    [15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13],
    [8, 6, 4, 1, 3, 11, 15, 0, 5, 12, 2, 13, 9, 7, 10, 14],
    [12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11],
];
const RMD_S: [[u32; 16]; 5] = [
    [11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8],
    [7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15, 9, 11, 7, 13, 12],
    [11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5],
    [11, 12, 14, 15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12],
    [9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11, 8, 5, 6],
];
const RMD_SP: [[u32; 16]; 5] = [
    [8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6],
    [9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12, 7, 6, 15, 13, 11],
    [9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5],
    [15, 5, 8, 11, 14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8],
    [8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13, 11, 11],
];
const RMD_K: [u32; 5] = [0, 0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC, 0xA953FD4E];
const RMD_KP: [u32; 5] = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x7A6D76E9, 0];
fn rmd_f(round: usize, x: u32, y: u32, z: u32) -> u32 {
    match round {
        0 => x ^ y ^ z,
        1 => (x & y) | (!x & z),
        2 => (x | !y) ^ z,
        3 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}
pub(crate) fn ripemd160(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    for block in padded(data, false).as_chunks::<64>().0 {
        let mut x = [0u32; 16];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            x[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }
        let mut left = h;
        let mut right = h;
        for round in 0..5 {
            for step in 0..16 {
                let [a, b, c, d, e] = left;
                let t = a
                    .wrapping_add(rmd_f(round, b, c, d))
                    .wrapping_add(x[RMD_R[round][step]])
                    .wrapping_add(RMD_K[round])
                    .rotate_left(RMD_S[round][step])
                    .wrapping_add(e);
                left = [e, t, b, c.rotate_left(10), d];
                let [a, b, c, d, e] = right;
                let t = a
                    .wrapping_add(rmd_f(4 - round, b, c, d))
                    .wrapping_add(x[RMD_RP[round][step]])
                    .wrapping_add(RMD_KP[round])
                    .rotate_left(RMD_SP[round][step])
                    .wrapping_add(e);
                right = [e, t, b, c.rotate_left(10), d];
            }
        }
        let t = h[1].wrapping_add(left[2]).wrapping_add(right[3]);
        h[1] = h[2].wrapping_add(left[3]).wrapping_add(right[4]);
        h[2] = h[3].wrapping_add(left[4]).wrapping_add(right[0]);
        h[3] = h[4].wrapping_add(left[0]).wrapping_add(right[1]);
        h[4] = h[0].wrapping_add(left[1]).wrapping_add(right[2]);
        h[0] = t;
    }
    let mut out = [0u8; 20];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    #[test]
    fn sha1_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(&sha1(&[b'a'; 1000])),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }
    #[test]
    fn ripemd160_vectors() {
        assert_eq!(
            hex(&ripemd160(b"")),
            "9c1185a5c5e9fc54612808977ee8f548b2258d31"
        );
        assert_eq!(
            hex(&ripemd160(b"abc")),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        assert_eq!(
            hex(&ripemd160(b"abcdefghijklmnopqrstuvwxyz")),
            "f71c27109c692c1b56bbdceb5b9d2865b3708dbc"
        );
    }
    #[test]
    fn digest_info_shape() {
        let info = MacAlgorithm::Sha1.digest_info(&[0xAB; 20], true);
        assert_eq!(hex(&info[..15]), "3021300906052b0e03021a05000414");
        assert_eq!(MacAlgorithm::Sha1.digest_info(&[0; 20], false).len(), 33);
    }
}
