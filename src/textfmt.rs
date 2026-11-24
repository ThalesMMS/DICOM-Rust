#![forbid(unsafe_code)]
use unicode_general_category::{GeneralCategory as G, get_general_category};
pub fn is_print(r: char) -> bool {
    let c = r as u32;
    if c <= 0xFF {
        return (0x20..=0x7E).contains(&c) || ((0xA1..=0xFF).contains(&c) && c != 0xAD);
    }
    matches!(
        get_general_category(r),
        G::UppercaseLetter
            | G::LowercaseLetter
            | G::TitlecaseLetter
            | G::ModifierLetter
            | G::OtherLetter
            | G::NonspacingMark
            | G::SpacingMark
            | G::EnclosingMark
            | G::DecimalNumber
            | G::LetterNumber
            | G::OtherNumber
            | G::ConnectorPunctuation
            | G::DashPunctuation
            | G::OpenPunctuation
            | G::ClosePunctuation
            | G::InitialPunctuation
            | G::FinalPunctuation
            | G::OtherPunctuation
            | G::MathSymbol
            | G::CurrencySymbol
            | G::ModifierSymbol
            | G::OtherSymbol
    )
}
fn push_escaped(out: &mut String, r: char, quote: char) {
    if r == quote || r == '\\' {
        out.push('\\');
        out.push(r);
        return;
    }
    if is_print(r) {
        out.push(r);
        return;
    }
    match r {
        '\u{7}' => out.push_str("\\a"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{b}' => out.push_str("\\v"),
        _ => {
            let c = r as u32;
            if c < 0x20 || c == 0x7F {
                out.push_str(&format!("\\x{c:02x}"));
            } else if c < 0x10000 {
                out.push_str(&format!("\\u{c:04x}"));
            } else {
                out.push_str(&format!("\\U{c:08x}"));
            }
        }
    }
}
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for r in s.chars() {
        push_escaped(&mut out, r, '"');
    }
    out.push('"');
    out
}
pub fn decode_rune(s: &[u8]) -> (Option<char>, usize) {
    let width = match s.first() {
        None => return (None, 0),
        Some(&b) if b < 0x80 => 1,
        Some(&b) if b & 0xE0 == 0xC0 => 2,
        Some(&b) if b & 0xF0 == 0xE0 => 3,
        Some(&b) if b & 0xF8 == 0xF0 => 4,
        Some(_) => return (None, 1),
    };
    if s.len() < width {
        return (None, 1);
    }
    match std::str::from_utf8(&s[..width]) {
        Ok(text) => (text.chars().next(), width),
        Err(_) => (None, 1),
    }
}
pub fn runes(s: &[u8]) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (r, width) = decode_rune(&s[i..]);
        out.push(r.unwrap_or('\u{FFFD}'));
        i += width;
    }
    out
}
pub fn quote_rune(r: char) -> String {
    let mut out = String::with_capacity(6);
    out.push('\'');
    push_escaped(&mut out, r, '\'');
    out.push('\'');
    out
}
pub fn spaced_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn to_upper(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut up = c.to_uppercase();
            match (up.next(), up.next()) {
                (Some(u), None) => u,
                _ => c,
            }
        })
        .collect()
}
pub fn float_v(x: f64) -> String {
    crate::numtext::format_float(x, 'g', -1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floats_like_go_v() {
        assert_eq!(float_v(1.1), "1.1");
        assert_eq!(float_v(1e-5), "1e-05");
        assert_eq!(float_v(1e20), "1e+20");
        assert_eq!(float_v(123456.0), "123456");
        assert_eq!(float_v(1e6), "1e+06");
        assert_eq!(float_v(1e21), "1e+21");
        assert_eq!(float_v(0.0001), "0.0001");
        assert_eq!(float_v(-2.5e-300), "-2.5e-300");
    }
    #[test]
    fn quotes_like_go() {
        assert_eq!(quote("a\"b\\c\n\u{feff}é"), "\"a\\\"b\\\\c\\n\\ufeffé\"");
        assert_eq!(quote_rune('\''), "'\\''");
        assert_eq!(quote_rune('\u{1}'), "'\\x01'");
        assert_eq!(quote_rune('😀'), "'😀'");
        assert_eq!(spaced_hex(&[0x1b, 0x24, 0x51]), "1B 24 51");
    }
}
