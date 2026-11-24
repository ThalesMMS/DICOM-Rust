use super::import::{ImportLimits, SourceInfo};
use super::{CONTAINER_MPEG_PROGRAM, Error, Kind, import_error, import_media, too_large};
use crate::object::transfer;
const MPEG2_MPML: &str = "1.2.840.10008.1.2.4.100";
const MPEG2_MPHL: &str = "1.2.840.10008.1.2.4.101";
fn be16(b: &[u8]) -> usize {
    usize::from(b[0]) << 8 | usize::from(b[1])
}
pub(crate) fn inspect_mpeg(b: &[u8], limits: ImportLimits) -> Result<SourceInfo, Error> {
    let mut es: Vec<u8> = Vec::new();
    let mut packets = 0;
    let mut at = 0usize;
    while at < b.len() {
        packets += 1;
        if packets > 4096 {
            return Err(Error::bare(Kind::TooManyFragments));
        }
        if at + 4 > b.len() || b[at..at + 3] != [0, 0, 1] {
            return Err(import_error("MPEG packet framing"));
        }
        let code = b[at + 3];
        if code == 0xb9 {
            at += 4;
            if at != b.len() {
                return Err(import_media());
            }
            break;
        }
        if code == 0xba {
            if at + 14 > b.len()
                || b[at + 4] & 0xc0 != 0x40
                || b[at + 4] & 4 == 0
                || b[at + 6] & 4 == 0
                || b[at + 8] & 4 == 0
                || b[at + 9] & 1 == 0
                || b[at + 12] & 3 != 3
                || b[at + 13] & 0xf8 != 0xf8
            {
                return Err(import_error("MPEG2 pack header"));
            }
            let n = 14 + usize::from(b[at + 13] & 7);
            if n > b.len() - at {
                return Err(import_media());
            }
            at += n;
            continue;
        }
        if at + 6 > b.len() {
            return Err(import_media());
        }
        let size = be16(&b[at + 4..]);
        let mut end = at + 6 + size;
        if size == 0 && code == 0xe0 {
            end = next_system_start(b, at + 6);
        }
        if end > b.len() || end < at + 6 {
            return Err(import_media());
        }
        match code {
            0xbb | 0xbe => {}
            0xe0 => {
                let mut start = at + 6;
                if start + 3 > end || b[start] & 0xc0 != 0x80 {
                    return Err(import_error("MPEG2 PES header"));
                }
                start += 3 + usize::from(b[start + 2]);
                if start > end {
                    return Err(import_media());
                }
                if (end - start) as i64 > limits.max_bytes - es.len() as i64 {
                    return Err(too_large());
                }
                es.extend_from_slice(&b[start..end]);
            }
            _ => {
                return Err(import_error("multiple or unsupported ProgramStream tracks"));
            }
        }
        at = end;
    }
    if es.is_empty() {
        return Err(import_media());
    }
    let (mut columns, mut rows, mut frame_rate) = (0i64, 0i64, 0.0f64);
    let (mut sequence, mut extension) = (false, false);
    let mut level = 0u8;
    let mut pictures = 0i64;
    let mut base_rate = 0.0f64;
    let mut aspect = 0u8;
    let mut at = 0usize;
    while at + 4 <= es.len() {
        let Some(position) = import_start_code(&es, at) else {
            break;
        };
        at = position;
        let code = es[at + 3];
        let end = import_start_code(&es, at + 4).unwrap_or(es.len());
        let v = &es[at + 4..end];
        match code {
            0xb3 => {
                if v.len() < 8 {
                    return Err(import_media());
                }
                let new_columns = i64::from(v[0]) << 4 | i64::from(v[1] >> 4);
                let new_rows = i64::from(v[1] & 15) << 8 | i64::from(v[2]);
                let rate = match mpeg_rate(v[3] & 15) {
                    Some(rate) if new_columns >= 1 && new_rows >= 1 => rate,
                    _ => return Err(import_media()),
                };
                if sequence && (columns != new_columns || rows != new_rows) {
                    return Err(import_error("changing MPEG geometry"));
                }
                if !sequence {
                    (columns, rows, frame_rate) = (new_columns, new_rows, rate);
                }
                if sequence && aspect != v[3] >> 4 {
                    return Err(import_error("changing MPEG aspect"));
                }
                aspect = v[3] >> 4;
                sequence = true;
                base_rate = rate;
            }
            0xb5 => {
                if v.is_empty() {
                    return Err(import_media());
                }
                if v[0] >> 4 == 1 {
                    if !sequence || v.len() < 6 {
                        return Err(import_media());
                    }
                    let profile_level = (v[0] & 15) << 4 | v[1] >> 4;
                    let profile = (profile_level >> 4) & 7;
                    let new_level = profile_level & 15;
                    let chroma = (v[1] >> 1) & 3;
                    let progressive = v[1] & 8 != 0;
                    if profile_level & 128 != 0
                        || profile != 4
                        || (new_level != 8 && new_level != 4)
                        || chroma != 1
                        || !progressive
                        || v[1] & 1 != 0
                        || v[2] & 0xe0 != 0
                    {
                        return Err(import_error("MPEG2 profile/chroma/field geometry"));
                    }
                    let rate =
                        base_rate * (f64::from(((v[5] >> 5) & 3) + 1) / f64::from((v[5] & 31) + 1));
                    if extension && (level != new_level || frame_rate != rate) {
                        return Err(import_error("changing MPEG timing/profile"));
                    }
                    frame_rate = rate;
                    level = new_level;
                    extension = true;
                }
            }
            0x00 => {
                if !extension || v.len() < 2 {
                    return Err(import_media());
                }
                let picture_type = (v[1] >> 3) & 7;
                if !(1..=3).contains(&picture_type) {
                    return Err(import_media());
                }
                pictures += 1;
                if pictures > limits.max_frames {
                    return Err(too_large());
                }
            }
            _ => {}
        }
        at = end;
    }
    if !sequence || !extension || pictures == 0 {
        return Err(import_error("missing MPEG2 sequence/pictures"));
    }
    let ntsc = 30000.0 / 1001.0;
    if level == 8 {
        let rows_cap = if frame_rate == 25.0 { 576 } else { 480 };
        if columns > 720
            || rows > rows_cap
            || (frame_rate != 25.0 && frame_rate != 30.0 && frame_rate != ntsc)
        {
            return Err(import_error("DICOM MPEG2 Main Level dimensions/timing"));
        }
    } else {
        let hd = (rows == 720 && columns == 1280) || (rows == 1080 && columns == 1920);
        let timing = frame_rate == 25.0
            || frame_rate == 30.0
            || frame_rate == ntsc
            || (rows == 720
                && (frame_rate == 50.0 || frame_rate == 60.0 || frame_rate == 60000.0 / 1001.0));
        if aspect != 3 || !hd || !timing {
            return Err(import_error(
                "DICOM MPEG2 High Level dimensions/aspect/timing",
            ));
        }
    }
    let uid = if level == 8 { MPEG2_MPML } else { MPEG2_MPHL };
    Ok(SourceInfo {
        rows,
        columns,
        frames: pictures,
        frame_rate,
        transfer_syntax: transfer::lookup(uid).ok_or_else(import_media)?,
        container: CONTAINER_MPEG_PROGRAM,
        compression_method: "ISO_13818_2",
    })
}
fn next_system_start(b: &[u8], start: usize) -> usize {
    let mut at = start;
    while at + 4 <= b.len() {
        let Some(n) = import_start_code(b, at) else {
            return b.len();
        };
        if b[n + 3] >= 0xb9 {
            return n;
        }
        at = n + 4;
    }
    b.len()
}
fn import_start_code(b: &[u8], start: usize) -> Option<usize> {
    if start + 4 > b.len() {
        return None;
    }
    let n = start + b[start..].windows(3).position(|w| w == [0, 0, 1])?;
    (n + 4 <= b.len()).then_some(n)
}
fn mpeg_rate(code: u8) -> Option<f64> {
    Some(match code {
        1 => 24000.0 / 1001.0,
        2 => 24.0,
        3 => 25.0,
        4 => 30000.0 / 1001.0,
        5 => 30.0,
        6 => 50.0,
        7 => 60000.0 / 1001.0,
        8 => 60.0,
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn start_codes() {
        assert_eq!(import_start_code(&[9, 0, 0, 1, 0xb3], 0), Some(1));
        assert_eq!(import_start_code(&[9, 9, 0, 0, 1], 0), None);
        assert_eq!(import_start_code(&[0, 0, 1], 0), None);
        assert_eq!(next_system_start(&[0, 0, 1, 0x01, 0, 0, 1, 0xba], 0), 4);
        assert_eq!(mpeg_rate(3), Some(25.0));
        assert_eq!(mpeg_rate(9), None);
    }
}
