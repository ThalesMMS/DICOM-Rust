use num_bigint::BigUint;
pub(super) const TAG_INTEGER: u8 = 0x02;
pub(super) const TAG_BIT_STRING: u8 = 0x03;
pub(super) const TAG_OID: u8 = 0x06;
pub(super) const TAG_SEQUENCE: u8 = 0x30;
#[derive(Clone, Copy, Debug)]
pub(super) struct Tlv<'a> {
    pub tag: u8,
    pub body: &'a [u8],
    pub full: &'a [u8],
}
pub(super) fn read(input: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1F == 0x1F {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (digits, rest) = rest.split_at(count);
        if digits[0] == 0 {
            return None;
        }
        let length = digits.iter().fold(0usize, |n, &d| n << 8 | usize::from(d));
        if length < 0x80 {
            return None;
        }
        (length, rest)
    };
    if rest.len() < length {
        return None;
    }
    let (body, rest) = rest.split_at(length);
    let full = &input[..input.len() - rest.len()];
    Some((Tlv { tag, body, full }, rest))
}
pub(super) fn children(mut body: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !body.is_empty() {
        let (tlv, rest) = read(body)?;
        out.push(tlv);
        body = rest;
    }
    Some(out)
}
pub(super) fn sequence<'a>(tlv: &Tlv<'a>) -> Option<Vec<Tlv<'a>>> {
    if tlv.tag != TAG_SEQUENCE {
        return None;
    }
    children(tlv.body)
}
pub(super) fn unsigned(tlv: &Tlv<'_>) -> Option<BigUint> {
    if tlv.tag != TAG_INTEGER {
        return None;
    }
    match tlv.body {
        [] => None,
        [first, ..] if first & 0x80 != 0 => None,
        [0, second, ..] if second & 0x80 == 0 => None,
        body => Some(BigUint::from_bytes_be(body)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_lengths_strictly() {
        let (tlv, rest) = read(&[0x04, 0x02, 1, 2, 9]).unwrap();
        assert_eq!(
            (tlv.tag, tlv.body, tlv.full.len(), rest),
            (4, &[1u8, 2][..], 4, &[9u8][..])
        );
        assert!(read(&[0x04, 0x81, 0x02, 1, 2]).is_none());
        assert!(read(&[0x04, 0x80, 1, 2]).is_none());
        assert!(read(&[0x04, 0x03, 1, 2]).is_none());
        let mut long = vec![0x04, 0x81, 0x80];
        long.extend_from_slice(&[7; 128]);
        assert_eq!(read(&long).unwrap().0.body.len(), 128);
    }
    #[test]
    fn integers() {
        let int = |body: &'static [u8]| {
            let full: &'static [u8] = body;
            unsigned(&Tlv {
                tag: TAG_INTEGER,
                body,
                full,
            })
        };
        assert_eq!(int(&[0x00, 0x80]), Some(BigUint::from(128u32)));
        assert_eq!(int(&[0x7F]), Some(BigUint::from(127u32)));
        assert_eq!(int(&[0x80]), None);
        assert_eq!(int(&[0x00, 0x7F]), None);
        assert_eq!(int(&[]), None);
    }
}
