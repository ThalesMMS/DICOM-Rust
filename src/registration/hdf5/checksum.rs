use super::le_uint;
pub(crate) fn metadata_checksum(data: &[u8]) -> u32 {
    let mut a = 0xdead_beefu32.wrapping_add(data.len() as u32);
    let (mut b, mut c) = (a, a);
    let mut data = data;
    let word = |d: &[u8], at: usize| u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]);
    while data.len() > 12 {
        a = a.wrapping_add(word(data, 0));
        b = b.wrapping_add(word(data, 4));
        c = c.wrapping_add(word(data, 8));
        a = a.wrapping_sub(c) ^ c.rotate_left(4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a) ^ a.rotate_left(6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b) ^ b.rotate_left(8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c) ^ c.rotate_left(16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a) ^ a.rotate_left(19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b) ^ b.rotate_left(4);
        b = b.wrapping_add(a);
        data = &data[12..];
    }
    if data.is_empty() {
        return c;
    }
    a = a.wrapping_add(le_uint(&data[..data.len().min(4)]) as u32);
    if data.len() > 4 {
        b = b.wrapping_add(le_uint(&data[4..data.len().min(8)]) as u32);
    }
    if data.len() > 8 {
        c = c.wrapping_add(le_uint(&data[8..]) as u32);
    }
    c = (c ^ b).wrapping_sub(b.rotate_left(14));
    a = (a ^ c).wrapping_sub(c.rotate_left(11));
    b = (b ^ a).wrapping_sub(a.rotate_left(25));
    c = (c ^ b).wrapping_sub(b.rotate_left(16));
    a = (a ^ c).wrapping_sub(c.rotate_left(4));
    b = (b ^ a).wrapping_sub(a.rotate_left(14));
    (c ^ b).wrapping_sub(b.rotate_left(24))
}
