#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::error::{Error, ErrorKind};
const BUFFER: i64 = 4096;
const UNDEFINED: u32 = 0xFFFF_FFFF;
struct Piece {
    size: i64,
    wrap: String,
}
pub(super) fn limit_error(encoded: &[u8], max: i64) -> Error {
    let limit = ErrorKind::ResourceLimit;
    match failing_wrap(encoded, max) {
        Some(wrap) if !wrap.is_empty() => Error::wrapped(format!("{wrap}{}", limit.text()), limit),
        _ => limit.into(),
    }
}
fn failing_wrap(encoded: &[u8], max: i64) -> Option<String> {
    let pieces = encoder_writes(pieces(encoded)?);
    let mut buffered: i64 = 0;
    let mut written: i64 = 0;
    for piece in pieces {
        let mut left = piece.size;
        while left > BUFFER.saturating_sub(buffered) {
            let chunk = if buffered == 0 {
                let all = left;
                left = 0;
                all
            } else {
                left = left.saturating_sub(BUFFER.saturating_sub(buffered));
                buffered = 0;
                BUFFER
            };
            if chunk > max.saturating_sub(written) {
                return Some(piece.wrap);
            }
            written = written.saturating_add(chunk);
        }
        buffered = buffered.saturating_add(left);
    }
    None
}
fn encoder_writes(pieces: Vec<Piece>) -> Vec<Piece> {
    let mut out = Vec::with_capacity(pieces.len().saturating_add(pieces.len() / 4));
    for piece in pieces {
        let innermost = piece.wrap.rfind("dicom: ").map_or("", |i| &piece.wrap[i..]);
        if innermost.starts_with("dicom: write tag for") {
            for half in ["group", "element"] {
                out.push(Piece {
                    size: 2,
                    wrap: format!(
                        "{}dicom: write tag {half}: dicom: write uint16: ",
                        piece.wrap
                    ),
                });
            }
        } else if innermost.starts_with("dicom: write length for") {
            let width = if piece.size == 2 { "uint16" } else { "uint32" };
            out.push(Piece {
                size: piece.size,
                wrap: format!("{}dicom: write {width}: ", piece.wrap),
            });
        } else {
            out.push(piece);
        }
    }
    out
}
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn length_text(length: u32) -> String {
    if length == UNDEFINED {
        "UNDEFINED".to_string()
    } else {
        length.to_string()
    }
}
fn wrap(op: &str, group: u16, element: u16, vr: &str, length: u32, outer: &str) -> String {
    let mut text = format!("{outer}dicom: {op} for tag ({group:04X},{element:04X}) with VR {vr}");
    if length != 0 || op == "write length" || op == "write value" {
        text.push_str(&format!(" with length {}", length_text(length)));
    }
    text.push_str(": ");
    text
}
fn long_length(vr: &[u8]) -> bool {
    matches!(
        vr,
        b"OB"
            | b"OD"
            | b"OF"
            | b"OL"
            | b"OV"
            | b"OW"
            | b"SQ"
            | b"SV"
            | b"UC"
            | b"UN"
            | b"UR"
            | b"UT"
            | b"UV"
    )
}
fn pieces(encoded: &[u8]) -> Option<Vec<Piece>> {
    let bare = |size: usize| Piece {
        size: size as i64,
        wrap: String::new(),
    };
    if encoded.get(128..132)? != b"DICM" || encoded.get(132..136)? != [2, 0, 0, 0] {
        return None;
    }
    let meta_end = 144usize.checked_add(u32_at(encoded, 140)? as usize)?;
    if meta_end > encoded.len() {
        return None;
    }
    let mut out = vec![bare(128), bare(4), bare(meta_end.saturating_sub(132))];
    let end = data_set(encoded, meta_end, encoded.len(), false, "", &mut out)?;
    (end == encoded.len()).then_some(out)
}
fn data_set(
    b: &[u8],
    mut at: usize,
    end: usize,
    delimited: bool,
    outer: &str,
    out: &mut Vec<Piece>,
) -> Option<usize> {
    let mut push = |size: usize, wrap: String| {
        out.push(Piece {
            size: size as i64,
            wrap,
        });
    };
    loop {
        if at == end && !delimited {
            return Some(at);
        }
        let group = u16_at(b, at)?;
        let element = u16_at(b, at.checked_add(2)?)?;
        if delimited && (group, element) == (0xFFFE, 0xE00D) {
            return Some(at);
        }
        let vr_bytes = b.get(at.checked_add(4)?..at.checked_add(6)?)?;
        let vr = std::str::from_utf8(vr_bytes).ok()?;
        let long = long_length(vr_bytes);
        let (length, value_at) = if long {
            (u32_at(b, at.checked_add(8)?)?, at.checked_add(12)?)
        } else {
            (
                u32::from(u16_at(b, at.checked_add(6)?)?),
                at.checked_add(8)?,
            )
        };
        let piece = |op: &str| wrap(op, group, element, vr, length, outer);
        push(4, piece("write tag"));
        push(2, piece("write VR"));
        if long {
            push(2, piece("write reserved bytes"));
            push(4, piece("write length"));
        } else {
            push(2, piece("write length"));
        }
        if vr_bytes != b"SQ" {
            if length == UNDEFINED {
                return None;
            }
            let next = value_at.checked_add(length as usize)?;
            if next > end {
                return None;
            }
            push(length as usize, piece("write value"));
            at = next;
            continue;
        }
        at = sequence(b, value_at, end, length, outer, &mut push)?;
    }
}
fn sequence(
    b: &[u8],
    mut at: usize,
    end: usize,
    length: u32,
    outer: &str,
    push: &mut dyn FnMut(usize, String),
) -> Option<usize> {
    let undefined = length == UNDEFINED;
    let stop = if undefined {
        end
    } else {
        let stop = at.checked_add(length as usize)?;
        if stop > end {
            return None;
        }
        stop
    };
    loop {
        if !undefined && at == stop {
            return Some(at);
        }
        let group = u16_at(b, at)?;
        let element = u16_at(b, at.checked_add(2)?)?;
        let item_length = u32_at(b, at.checked_add(4)?)?;
        if undefined && (group, element) == (0xFFFE, 0xE0DD) {
            push(4, wrap("write tag", group, element, "UN", 0, outer));
            push(4, wrap("write length", group, element, "UN", 0, outer));
            return at.checked_add(8);
        }
        if (group, element) != (0xFFFE, 0xE000) {
            return None;
        }
        push(
            4,
            wrap("write tag", group, element, "UN", item_length, outer),
        );
        push(
            4,
            wrap("write length", group, element, "UN", item_length, outer),
        );
        at = at.checked_add(8)?;
        let mut pieces = Vec::new();
        if item_length == UNDEFINED {
            let inner = wrap("write value", 0xFFFE, 0xE000, "UN", UNDEFINED, outer);
            at = data_set(b, at, stop, true, &inner, &mut pieces)?;
            pieces.push(Piece {
                size: 4,
                wrap: wrap("write tag", 0xFFFE, 0xE00D, "UN", 0, &inner),
            });
            pieces.push(Piece {
                size: 4,
                wrap: wrap("write length", 0xFFFE, 0xE00D, "UN", 0, &inner),
            });
            at = at.checked_add(8)?;
        } else {
            let item_end = at.checked_add(item_length as usize)?;
            if item_end > stop {
                return None;
            }
            at = data_set(b, at, item_end, false, outer, &mut pieces)?;
        }
        for piece in pieces {
            push(piece.size as usize, piece.wrap);
        }
    }
}
