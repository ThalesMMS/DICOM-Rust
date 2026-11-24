use super::tables;
pub(super) type Charmap = [u16; 256];
pub(super) fn charmap_decode(map: &Charmap, b: u8) -> Option<char> {
    match map[usize::from(b)] {
        0 if b != 0 => None,
        c => char::from_u32(u32::from(c)),
    }
}
pub(super) fn charmap_encode(map: &Charmap, r: char) -> Option<u8> {
    let c = u32::from(r);
    if c == 0 {
        return Some(0);
    }
    (0..=255u8).find(|&b| u32::from(map[usize::from(b)]) == c)
}
fn search(table: &[(u32, u32)], r: char) -> Option<u32> {
    table
        .binary_search_by_key(&u32::from(r), |&(k, _)| k)
        .ok()
        .map(|i| table[i].1)
}
fn cell(table: &[u16; 94 * 94], first: u8, second: u8) -> Option<char> {
    if !(0xA1..=0xFE).contains(&first) || !(0xA1..=0xFE).contains(&second) {
        return None;
    }
    let c = table[usize::from(first - 0xA1) * 94 + usize::from(second - 0xA1)];
    if c == 0 {
        None
    } else {
        char::from_u32(u32::from(c))
    }
}
pub(super) fn gbk_decode(c0: u8, c1: u8) -> Option<char> {
    if !(0x81..=0xFE).contains(&c0) {
        return None;
    }
    let trail = match c1 {
        0x40..=0x7E => c1 - 0x40,
        0x80..=0xFE => c1 - 0x41,
        _ => return None,
    };
    let c = tables::GBK_DECODE[usize::from(c0 - 0x81) * 190 + usize::from(trail)];
    if c == 0 {
        None
    } else {
        char::from_u32(u32::from(c))
    }
}
pub(super) fn gbk_encode(r: char) -> Option<[u8; 2]> {
    search(&tables::GBK_ENCODE, r).map(|code| [(code >> 8) as u8, code as u8])
}
pub(super) fn gb18030_decode_pointer(pointer: u32) -> Option<char> {
    if pointer >= 189_000 {
        let r = pointer - 189_000;
        return if r < 0x10_0000 {
            char::from_u32(r + 0x10000)
        } else {
            None
        };
    }
    match tables::GB18030_FOUR_DECODE.get(pointer as usize) {
        Some(&c) if c != 0 => char::from_u32(u32::from(c)),
        _ => None,
    }
}
pub(super) fn gb18030_encode_four(r: char) -> Option<[u8; 4]> {
    let c = u32::from(r);
    let pointer = if c >= 0x10000 {
        c - 0x10000 + 189_000
    } else {
        let spans = &tables::GB18030_SPANS;
        let index = spans.partition_point(|&(rune, _, _)| rune <= c);
        let &(rune, start, length) = spans.get(index.checked_sub(1)?)?;
        if c - rune >= length {
            return None;
        }
        start + (c - rune)
    };
    let b3 = pointer % 10;
    let b2 = (pointer / 10) % 126;
    let b1 = (pointer / 1260) % 10;
    let b0 = pointer / 12600;
    Some([
        (b0 + 0x81) as u8,
        (b1 + 0x30) as u8,
        (b2 + 0x81) as u8,
        (b3 + 0x30) as u8,
    ])
}
pub(super) fn jis0208_decode(first: u8, second: u8) -> Option<char> {
    cell(&tables::JIS0208_DECODE, first, second)
}
pub(super) fn jis0212_decode(first: u8, second: u8) -> Option<char> {
    cell(&tables::JIS0212_DECODE, first, second)
}
pub(super) fn ksx1001_decode(first: u8, second: u8) -> Option<char> {
    cell(&tables::KSX1001_DECODE, first, second)
}
pub(super) fn eucjp_encode(r: char) -> Option<Result<[u8; 2], [u8; 2]>> {
    search(&tables::EUCJP_ENCODE, r).map(|code| {
        let pair = [(code >> 8) as u8, code as u8];
        if code >> 16 == 0x8F {
            Err(pair)
        } else {
            Ok(pair)
        }
    })
}
pub(super) fn ksx1001_encode(r: char) -> Option<[u8; 2]> {
    search(&tables::KSX1001_ENCODE, r).map(|code| [(code >> 8) as u8, code as u8])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gb18030_four_byte_round_trips() {
        for r in ['\u{80}', '\u{FFFF}', '😀', '\u{10FFFF}'] {
            if let Some(b) = gb18030_encode_four(r) {
                let p = ((u32::from(b[0]) - 0x81) * 10 + (u32::from(b[1]) - 0x30)) * 1260
                    + (u32::from(b[2]) - 0x81) * 10
                    + (u32::from(b[3]) - 0x30);
                assert_eq!(gb18030_decode_pointer(p), Some(r));
            }
        }
        assert_eq!(
            gb18030_encode_four('\u{80}'),
            Some([0x81, 0x30, 0x81, 0x30])
        );
    }
}
