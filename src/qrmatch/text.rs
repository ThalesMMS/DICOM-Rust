use super::tables::SIMPLE_FOLD;
use crate::object::Vr;
pub(super) fn lossy_text(mut bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    loop {
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&bytes[..valid]).unwrap_or_default());
                out.push('\u{FFFD}');
                bytes = &bytes[valid + 1..];
            }
        }
    }
}
fn simple_fold(r: char) -> char {
    match SIMPLE_FOLD.binary_search_by_key(&(r as u32), |&(from, _)| from) {
        Ok(i) => char::from_u32(SIMPLE_FOLD[i].1).unwrap_or(r),
        Err(_) => r,
    }
}
pub(super) fn equal_fold(s: &str, t: &str) -> bool {
    let mut a = s.chars();
    let mut b = t.chars();
    loop {
        let (r1, r2) = match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(r1), Some(r2)) => (r1, r2),
            _ => return false,
        };
        if r1 == r2 {
            continue;
        }
        let (r1, r2) = if r2 < r1 { (r2, r1) } else { (r1, r2) };
        if r2.is_ascii() {
            if r1.is_ascii_uppercase() && r2 as u32 == r1 as u32 + 0x20 {
                continue;
            }
            return false;
        }
        let mut r = simple_fold(r1);
        while r != r1 && r < r2 {
            r = simple_fold(r);
        }
        if r != r2 {
            return false;
        }
    }
}
fn space_padded(vr: Vr) -> bool {
    use Vr::*;
    matches!(
        vr,
        AE | AS | CS | DA | DS | DT | IS | LO | LT | PN | SH | ST | TM | UC | UR | UT
    )
}
pub(super) fn trim_text_value(vr: Vr, s: &str) -> &str {
    if vr != Vr::UI && space_padded(vr) {
        s.trim_end_matches(' ')
    } else {
        s.trim_end_matches([' ', '\0'])
    }
}
pub(super) fn trim_text_value_bytes(vr: Vr, data: &[u8]) -> &[u8] {
    let pad: &[u8] = if vr != Vr::UI && space_padded(vr) {
        b" "
    } else {
        b" \0"
    };
    let mut end = data.len();
    while end > 0 && pad.contains(&data[end - 1]) {
        end -= 1;
    }
    &data[..end]
}
pub(super) fn split_text_multiplicity(vr: Vr, s: &str) -> Vec<String> {
    let s = trim_text_value(vr, s);
    if s.is_empty() {
        return Vec::new();
    }
    if !uses_text_value_delimiter(vr) {
        return vec![s.to_string()];
    }
    s.split('\\')
        .map(|part| trim_text_value(vr, part).to_string())
        .collect()
}
pub(super) fn is_string_like(vr: Vr) -> bool {
    use Vr::*;
    matches!(
        vr,
        AE | AS | CS | DA | DS | DT | IS | LO | LT | PN | SH | ST | TM | UC | UI | UR | UT
    )
}
pub(super) fn uses_specific_character_set(vr: Vr) -> bool {
    use Vr::*;
    matches!(vr, LO | LT | PN | SH | ST | UC | UT)
}
pub(super) fn uses_text_value_delimiter(vr: Vr) -> bool {
    !matches!(vr, Vr::LT | Vr::ST | Vr::UT | Vr::UR)
}
pub(super) fn value_rune_len(values: &[String]) -> usize {
    values.iter().map(|v| v.chars().count()).sum()
}
pub(super) fn all_values_empty(values: &[String]) -> bool {
    values.iter().all(|v| v.trim().is_empty())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossy_text_replaces_each_invalid_byte() {
        assert_eq!(lossy_text(b"a\xe9\xff\\b"), "a\u{FFFD}\u{FFFD}\\b");
        assert_eq!(lossy_text(b"\xe2\x82"), "\u{FFFD}\u{FFFD}");
    }
    #[test]
    fn equal_fold_like_go() {
        assert!(equal_fold("Doe^John", "DOE^JOHN"));
        assert!(equal_fold("\u{212A}elvin", "kelvin"));
        assert!(equal_fold("\u{17F}mith", "SMITH"));
        assert!(!equal_fold("ab", "a"));
    }
}
