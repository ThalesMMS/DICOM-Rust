#![forbid(unsafe_code)]
use crate::object::{Element, Tag, Vr, is_valid_uid};
pub const ERR_VALUE_NOT_REPRESENTABLE: &str = "dicom: value is not representable in its VR";
pub const BITS_ALLOCATED_TAG: Tag = Tag::new(0x0028, 0x0100);
fn number_matches_ds(s: &[u8]) -> bool {
    let mut i = 0;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    if digits(&mut i) > 0 {
        if i < s.len() && s[i] == b'.' {
            i += 1;
            digits(&mut i);
        }
    } else {
        if i >= s.len() || s[i] != b'.' {
            return false;
        }
        i += 1;
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == s.len()
}
fn number_matches_is(s: &[u8]) -> bool {
    let s = s
        .strip_prefix(b"+")
        .or_else(|| s.strip_prefix(b"-"))
        .unwrap_or(s);
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}
pub fn validate_number_string(vr: Vr, value: &str) -> Result<(), String> {
    let max = if vr == Vr::IS { 12 } else { 16 };
    if value.len() > max {
        return Err(format!(
            "{vr} value is {} bytes, want at most {max}",
            value.len()
        ));
    }
    let trimmed = value.trim_matches(' ');
    let ok = if vr == Vr::IS {
        number_matches_is(trimmed.as_bytes())
    } else {
        number_matches_ds(trimmed.as_bytes())
    };
    if !ok {
        return Err(format!(
            "{}-byte value does not match {vr} syntax",
            value.len()
        ));
    }
    if vr == Vr::IS
        && !matches!(trimmed.parse::<i64>(), Ok(n) if i64::from(i32::MIN) <= n && n <= i64::from(i32::MAX))
    {
        return Err("IS value is outside the signed 32-bit range".to_string());
    }
    Ok(())
}
pub fn validate_string(vr: Vr, values: &[String]) -> Result<(), String> {
    let fail = |m: String| Err(format!("{ERR_VALUE_NOT_REPRESENTABLE}: {m}"));
    if !vr.is_string_like() {
        return fail(format!("text value is incompatible with VR {vr}"));
    }
    if !vr.uses_text_value_delimiter() && values.len() > 1 {
        return fail(format!("VR {vr} holds one value, got {}", values.len()));
    }
    if vr == Vr::UI {
        let mut index = 0;
        for component in values {
            for uid in component.split('\\') {
                let uid = uid.trim_end_matches(['\0', ' ']);
                if !uid.is_empty() && !is_valid_uid(uid.as_bytes()) {
                    return fail(format!(
                        "UI value {index} ({} bytes) is not a valid UID",
                        uid.len()
                    ));
                }
                index += 1;
            }
        }
        return Ok(());
    }
    if vr != Vr::IS && vr != Vr::DS {
        return Ok(());
    }
    let mut index = 0;
    for component in values {
        for number in component.split('\\') {
            if !number.trim_matches(' ').is_empty()
                && let Err(e) = validate_number_string(vr, number)
            {
                return fail(format!("{vr} value {index}: {e}"));
            }
            index += 1;
        }
    }
    Ok(())
}
pub fn little_endian_unit_width(tag: Tag, vr: Vr, bits_allocated: i64) -> usize {
    match vr {
        Vr::OW => {
            if tag == Tag::PIXEL_DATA && (bits_allocated == 32 || bits_allocated == 64) {
                (bits_allocated / 8) as usize
            } else {
                vr.byte_order_width()
            }
        }
        Vr::OF | Vr::OD | Vr::OL | Vr::OV => vr.byte_order_width(),
        _ => 1,
    }
}
pub fn reverse_units_in_place(data: &mut [u8], width: usize) {
    if width > 1 {
        for chunk in data.chunks_exact_mut(width) {
            chunk.reverse();
        }
    }
}
pub fn reverse_fixed_width(data: &[u8], width: usize) -> Vec<u8> {
    let mut out = data.to_vec();
    reverse_units_in_place(&mut out, width);
    out
}
pub fn bits_allocated(elem: &Element, big_endian: bool) -> i64 {
    match elem.raw() {
        Some(raw) if raw.len() >= 2 => {
            let v = [raw[0], raw[1]];
            i64::from(if big_endian {
                u16::from_be_bytes(v)
            } else {
                u16::from_le_bytes(v)
            })
        }
        _ => 0,
    }
}
