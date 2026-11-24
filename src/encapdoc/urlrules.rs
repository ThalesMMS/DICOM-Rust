use super::payload::lower_text;
use super::{Error, Kind, Result};
fn bad() -> Error {
    Error::wrap(Kind::InvalidDocument, "invalid relative model URI")
}
fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => c - b'A' + 10,
    }
}
fn unescape(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            if i + 2 >= s.len() || !is_hex(s[i + 1]) || !is_hex(s[i + 2]) {
                return None;
            }
            out.push(unhex(s[i + 1]) << 4 | unhex(s[i + 2]));
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    Some(out)
}
fn relative_path(raw: &str) -> Option<Vec<u8>> {
    let (u, frag) = match raw.split_once('#') {
        Some((u, f)) => (u, f),
        None => (raw, ""),
    };
    if !frag.is_empty() {
        return None;
    }
    let b = u.as_bytes();
    if b.iter().any(|&c| c < 0x20 || c == 0x7f) {
        return None;
    }
    if u == "*" {
        return Some(b"*".to_vec());
    }
    let mut rest = u;
    for (i, &c) in b.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            continue;
        }
        if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' {
            if i == 0 {
                break;
            }
            continue;
        }
        if c == b':' {
            return None;
        }
        break;
    }
    if rest.ends_with('?') && rest.matches('?').count() == 1 {
        rest = &rest[..rest.len() - 1];
    } else if let Some((path, query)) = rest.split_once('?') {
        if !query.is_empty() {
            return None;
        }
        rest = path;
    }
    if !rest.starts_with('/') {
        let segment = rest.split('/').next().unwrap_or_default();
        if segment.contains(':') {
            return None;
        }
    }
    if rest.starts_with("//") && !rest.starts_with("///") {
        return None;
    }
    unescape(rest.as_bytes())
}
fn has_space(s: &[u8]) -> bool {
    String::from_utf8_lossy(s).chars().any(char::is_whitespace)
}
fn clean(p: &[u8]) -> Vec<u8> {
    if p.is_empty() {
        return b".".to_vec();
    }
    let rooted = p[0] == b'/';
    let n = p.len();
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let (mut r, mut dotdot) = (0, 0);
    if rooted {
        out.push(b'/');
        r = 1;
        dotdot = 1;
    }
    while r < n {
        if p[r] == b'/' || p[r] == b'.' && (r + 1 == n || p[r + 1] == b'/') {
            r += 1;
        } else if p[r] == b'.' && p[r + 1] == b'.' && (r + 2 == n || p[r + 2] == b'/') {
            r += 2;
            if out.len() > dotdot {
                let mut w = out.len() - 1;
                while w > dotdot && out[w] != b'/' {
                    w -= 1;
                }
                out.truncate(w);
            } else if !rooted {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(b"..");
                dotdot = out.len();
            }
        } else {
            if rooted && out.len() != 1 || !rooted && !out.is_empty() {
                out.push(b'/');
            }
            while r < n && p[r] != b'/' {
                out.push(p[r]);
                r += 1;
            }
        }
    }
    if out.is_empty() {
        return b".".to_vec();
    }
    out
}
fn ext(p: &[u8]) -> &[u8] {
    let mut i = p.len();
    while i > 0 && p[i - 1] != b'/' {
        i -= 1;
        if p[i] == b'.' {
            return &p[i..];
        }
    }
    &[]
}
pub fn model_relative_uri(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_whitespace)
        || value.contains(['\\', '\0'])
    {
        return Err(bad());
    }
    let Some(decoded) = relative_path(value) else {
        return Err(bad());
    };
    if decoded.is_empty()
        || decoded[0] == b'/'
        || decoded.contains(&b'\\')
        || decoded.contains(&0)
        || has_space(&decoded)
    {
        return Err(bad());
    }
    if decoded.split(|&c| c == b'/').any(|part| part == b"..") {
        return Err(bad());
    }
    let extension = lower_text(&String::from_utf8_lossy(ext(&decoded)));
    if matches!(
        extension.as_str(),
        ".exe" | ".dll" | ".com" | ".bat" | ".cmd" | ".ps1" | ".vbs" | ".js" | ".msi" | ".scr"
    ) {
        return Err(bad());
    }
    let cleaned = clean(&decoded);
    if cleaned == b"." {
        return Err(bad());
    }
    Ok(String::from_utf8_lossy(&cleaned).into_owned())
}
