use super::{Error, corrupt};
const ICC_HEADER_SIZE: usize = 128;
const TAG_STRINGS: [&[u8; 4]; 17] = [
    b"cprt", b"wtpt", b"bkpt", b"rXYZ", b"gXYZ", b"bXYZ", b"kXYZ", b"rTRC", b"gTRC", b"bTRC",
    b"kTRC", b"chad", b"desc", b"chrm", b"dmnd", b"dmdd", b"lumi",
];
const TYPE_STRINGS: [&[u8; 4]; 8] = [
    b"XYZ ", b"desc", b"text", b"mluc", b"para", b"curv", b"sf32", b"gbd ",
];
fn var_int(b: &[u8], pos: &mut usize) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 10 {
        let Some(&byte) = b.get(pos.saturating_add(i)) else {
            break;
        };
        v |= u64::from(byte & 127) << i.saturating_mul(7);
        if byte & 128 == 0 {
            break;
        }
        i = i.saturating_add(1);
    }
    *pos = pos.saturating_add(i).saturating_add(1);
    v
}
fn shuffle(data: &mut [u8], width: usize) {
    let size = data.len();
    let height = size.div_ceil(width);
    let mut out = vec![0u8; size];
    let (mut s, mut j) = (0usize, 0usize);
    for o in out.iter_mut() {
        *o = data.get(j).copied().unwrap_or(0);
        j = j.saturating_add(height);
        if j >= size {
            s = s.saturating_add(1);
            j = s;
        }
    }
    data.copy_from_slice(&out);
}
fn predict(p1: u64, p2: u64, p3: u64, order: usize) -> u64 {
    match order {
        0 => p1,
        1 => p1.wrapping_mul(2).wrapping_sub(p2),
        2 => p1
            .wrapping_mul(3)
            .wrapping_sub(p2.wrapping_mul(3))
            .wrapping_add(p3),
        _ => 0,
    }
}
fn linear_predict(
    data: &[u8],
    start: usize,
    i: usize,
    stride: usize,
    width: usize,
    order: usize,
) -> u8 {
    let pos = start.saturating_add(i);
    let back = |p: usize, k: usize| p.checked_sub(stride.checked_mul(k)?);
    match width {
        1 => {
            let get = |k| {
                back(pos, k)
                    .and_then(|q| data.get(q))
                    .map_or(0, |&b| u64::from(b))
            };
            predict(get(1), get(2), get(3), order) as u8
        }
        2 => {
            let p = start.saturating_add(i & !1);
            let get = |k| {
                back(p, k)
                    .and_then(|q| data.get(q..)?.first_chunk::<2>())
                    .map_or(0, |&b| u64::from(u16::from_be_bytes(b)))
            };
            let pred = predict(get(1), get(2), get(3), order) as u16;
            if i & 1 == 1 {
                pred as u8
            } else {
                (pred >> 8) as u8
            }
        }
        _ => {
            let p = start.saturating_add(i & !3);
            let get = |k| {
                back(p, k)
                    .filter(|q| q.saturating_add(4) <= pos)
                    .and_then(|q| data.get(q..)?.first_chunk::<4>())
                    .map_or(0, |&b| u64::from(u32::from_be_bytes(b)))
            };
            let pred = predict(get(1), get(2), get(3), order) as u32;
            (pred >> 3usize.saturating_sub(i & 3).saturating_mul(8)) as u8
        }
    }
}
pub(crate) const MAX_ICC_BYTES: u64 = 1 << 22;
pub(crate) const PREAMBLE_BYTES: usize = 22;
pub(crate) fn check_preamble(preamble: &[u8], enc_len: u64) -> Result<(), Error> {
    let mut pos = 0usize;
    let osize = var_int(preamble, &mut pos);
    let csize = var_int(preamble, &mut pos);
    if osize > 1 << 28 || csize > enc_len || (pos as u64).saturating_add(csize) > enc_len {
        return Err(corrupt("ICC stream"));
    }
    if osize > MAX_ICC_BYTES {
        return Err(super::limit());
    }
    Ok(())
}
pub(crate) fn unpredict(enc: &[u8]) -> Result<Vec<u8>, Error> {
    let bad = || corrupt("ICC stream");
    let mut pos = 0usize;
    if pos >= enc.len() {
        return Err(bad());
    }
    let osize = var_int(enc, &mut pos);
    if pos >= enc.len() {
        return Err(bad());
    }
    let csize = var_int(enc, &mut pos);
    if osize > 1 << 28
        || csize > enc.len() as u64
        || (pos as u64).saturating_add(csize) > enc.len() as u64
    {
        return Err(bad());
    }
    let mut cpos = pos;
    let cend = cpos.saturating_add(csize as usize);
    pos = cend;
    let mut out: Vec<u8> = Vec::new();
    let mut header = [0u8; ICC_HEADER_SIZE];
    header[8] = 4;
    header[12..24].copy_from_slice(b"mntrRGB XYZ ");
    header[36..40].copy_from_slice(b"acsp");
    header[68..80].copy_from_slice(&[0, 0, 246, 214, 0, 1, 0, 0, 0, 0, 211, 45]);
    header[..4].copy_from_slice(&(osize as u32).to_be_bytes());
    for i in 0..=ICC_HEADER_SIZE {
        if out.len() as u64 == osize {
            if cpos != cend || pos != enc.len() {
                return Err(bad());
            }
            return Ok(out);
        }
        if i == ICC_HEADER_SIZE {
            break;
        }
        match i {
            8 => {
                if let Some(v) = out.get(4..8) {
                    header[80..84].copy_from_slice(v);
                }
            }
            41 => match out.get(40) {
                Some(b'A') => header[41..44].copy_from_slice(b"PPL"),
                Some(b'M') => header[41..44].copy_from_slice(b"SFT"),
                _ => {}
            },
            42 => match out.get(40..42) {
                Some(b"SG") => header[42..44].copy_from_slice(b"I "),
                Some(b"SU") => header[42..44].copy_from_slice(b"NW"),
                _ => {}
            },
            _ => {}
        }
        let (Some(&e), Some(&h)) = (enc.get(pos), header.get(i)) else {
            return Err(bad());
        };
        out.push(e.wrapping_add(h));
        pos = pos.saturating_add(1);
    }
    if cpos >= cend {
        return Err(bad());
    }
    let append_u32 = |out: &mut Vec<u8>, v: u64| out.extend_from_slice(&(v as u32).to_be_bytes());
    let mut num_tags = var_int(enc, &mut cpos);
    if num_tags != 0 {
        num_tags = num_tags.saturating_sub(1);
        if num_tags > 1 << 20 {
            return Err(bad());
        }
        append_u32(&mut out, num_tags);
        let mut prev_start = num_tags
            .saturating_mul(12)
            .saturating_add(ICC_HEADER_SIZE as u64);
        let mut prev_size = 0u64;
        loop {
            if out.len() as u64 > osize || cpos > cend {
                return Err(bad());
            }
            if cpos == cend {
                break;
            }
            let Some(&cmd) = enc.get(cpos) else {
                return Err(bad());
            };
            cpos = cpos.saturating_add(1);
            let code = cmd & 63;
            let tag: [u8; 4] = match code {
                0 => break,
                1 => {
                    let Some(&t) = enc.get(pos..).and_then(|r| r.first_chunk::<4>()) else {
                        return Err(bad());
                    };
                    pos = pos.saturating_add(4);
                    t
                }
                2 => *b"rTRC",
                3 => *b"rXYZ",
                _ => match TAG_STRINGS.get(usize::from(code.saturating_sub(4))) {
                    Some(&&t) => t,
                    None => return Err(bad()),
                },
            };
            out.extend_from_slice(&tag);
            let mut size = prev_size;
            if matches!(
                &tag,
                b"rXYZ" | b"gXYZ" | b"bXYZ" | b"kXYZ" | b"wtpt" | b"bkpt" | b"lumi"
            ) {
                size = 20;
            }
            let mut start = prev_start.wrapping_add(prev_size);
            if cmd & 64 != 0 {
                if cpos >= cend {
                    return Err(bad());
                }
                start = var_int(enc, &mut cpos);
            }
            if cmd & 128 != 0 {
                if cpos >= cend {
                    return Err(bad());
                }
                size = var_int(enc, &mut cpos);
            }
            if start > u64::from(u32::MAX) || size > u64::from(u32::MAX) {
                return Err(bad());
            }
            append_u32(&mut out, start);
            append_u32(&mut out, size);
            prev_start = start;
            prev_size = size;
            if code == 2 {
                out.extend_from_slice(b"gTRC");
                append_u32(&mut out, start);
                append_u32(&mut out, size);
                out.extend_from_slice(b"bTRC");
                append_u32(&mut out, start);
                append_u32(&mut out, size);
            }
            if code == 3 {
                out.extend_from_slice(b"gXYZ");
                append_u32(&mut out, start.saturating_add(size));
                append_u32(&mut out, size);
                out.extend_from_slice(b"bXYZ");
                append_u32(&mut out, start.saturating_add(size.saturating_mul(2)));
                append_u32(&mut out, size);
            }
        }
    }
    loop {
        if out.len() as u64 > osize || cpos > cend {
            return Err(bad());
        }
        if cpos == cend {
            break;
        }
        let Some(&cmd) = enc.get(cpos) else {
            return Err(bad());
        };
        cpos = cpos.saturating_add(1);
        let take = |pos: usize, n: u64| enc.get(pos..pos.saturating_add(usize::try_from(n).ok()?));
        match cmd {
            1..=3 => {
                if cpos >= cend {
                    return Err(bad());
                }
                let n = var_int(enc, &mut cpos);
                let Some(bytes) = take(pos, n) else {
                    return Err(bad());
                };
                let mut buf = bytes.to_vec();
                if cmd != 1 {
                    shuffle(&mut buf, if cmd == 2 { 2 } else { 4 });
                }
                out.extend_from_slice(&buf);
                pos = pos.saturating_add(buf.len());
            }
            4 => {
                if cpos.saturating_add(2) > cend {
                    return Err(bad());
                }
                let Some(&flags) = enc.get(cpos) else {
                    return Err(bad());
                };
                cpos = cpos.saturating_add(1);
                let width = usize::from(flags & 3).saturating_add(1);
                let order = usize::from(flags & 12) >> 2;
                if width == 3 || order == 3 {
                    return Err(bad());
                }
                let mut stride = width as u64;
                if flags & 16 != 0 {
                    if cpos >= cend {
                        return Err(bad());
                    }
                    stride = var_int(enc, &mut cpos);
                    if stride < width as u64 {
                        return Err(bad());
                    }
                }
                if out.is_empty() || (out.len().saturating_sub(1) as u64 >> 2) < stride {
                    return Err(bad());
                }
                if cpos >= cend {
                    return Err(bad());
                }
                let n = var_int(enc, &mut cpos);
                let Some(bytes) = take(pos, n) else {
                    return Err(bad());
                };
                let n = bytes.len();
                let mut buf = bytes.to_vec();
                if width > 1 {
                    shuffle(&mut buf, width);
                }
                let start = out.len();
                for (i, &b) in buf.iter().enumerate() {
                    let p = linear_predict(&out, start, i, stride as usize, width, order);
                    out.push(p.wrapping_add(b));
                }
                pos = pos.saturating_add(n);
            }
            10 => {
                out.extend_from_slice(b"XYZ \0\0\0\0");
                let Some(xyz) = take(pos, 12) else {
                    return Err(bad());
                };
                out.extend_from_slice(xyz);
                pos = pos.saturating_add(12);
            }
            16..=23 => {
                let Some(t) = TYPE_STRINGS.get(usize::from(cmd.saturating_sub(16))) else {
                    return Err(bad());
                };
                out.extend_from_slice(*t);
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
            _ => return Err(bad()),
        }
    }
    if pos != enc.len() || out.len() as u64 != osize {
        return Err(bad());
    }
    Ok(out)
}
#[cfg(test)]
mod preamble_tests {
    use super::*;
    #[test]
    fn check_preamble_refuses_only_what_unpredict_refuses() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let len = PREAMBLE_BYTES + 1 + (next() % 64) as usize;
            let mut enc: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if next() & 1 == 0 {
                enc[0] &= 0x7F;
                enc[1] &= 0x3F;
            }
            match check_preamble(&enc[..PREAMBLE_BYTES], len as u64) {
                Err(e) if !e.is_limit() => {
                    assert_eq!(unpredict(&enc).unwrap_err(), e, "{enc:02x?}");
                }
                _ => {}
            }
        }
    }
    #[test]
    fn a_profile_over_the_ceiling_is_a_limit() {
        let mut enc = vec![0x81, 0x80, 0x80, 0x02, 0x00];
        enc.resize(PREAMBLE_BYTES + 1, 0);
        assert!(
            check_preamble(&enc[..PREAMBLE_BYTES], enc.len() as u64)
                .unwrap_err()
                .is_limit()
        );
        enc[0] = 0x80;
        assert!(check_preamble(&enc[..PREAMBLE_BYTES], enc.len() as u64).is_ok());
    }
}
