fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    Host,
    Zone,
    UserPassword,
    Fragment,
}
fn host_should_escape(c: u8) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    !matches!(
        c,
        b'!' | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'"'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
    )
}
fn unescape_ok(s: &[u8], mode: Mode) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            if i + 2 >= s.len() || !is_hex(s[i + 1]) || !is_hex(s[i + 2]) {
                return false;
            }
            let escape = &s[i..i + 3];
            if mode == Mode::Host && unhex(s[i + 1]) < 8 && escape != b"%25" {
                return false;
            }
            if mode == Mode::Zone {
                let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                if escape != b"%25" && v != b' ' && host_should_escape(v) {
                    return false;
                }
            }
            i += 3;
        } else {
            if (mode == Mode::Host || mode == Mode::Zone) && s[i] < 0x80 && host_should_escape(s[i])
            {
                return false;
            }
            i += 1;
        }
    }
    true
}
fn valid_optional_port(port: &[u8]) -> bool {
    match port.split_first() {
        None => true,
        Some((b':', rest)) => rest.iter().all(u8::is_ascii_digit),
        Some(_) => false,
    }
}
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
fn host_ok(host: &[u8]) -> bool {
    if host.first() == Some(&b'[') {
        let Some(i) = host.iter().rposition(|&c| c == b']') else {
            return false;
        };
        if !valid_optional_port(&host[i + 1..]) {
            return false;
        }
        if let Some(zone) = find(&host[..i], b"%25") {
            return unescape_ok(&host[..zone], Mode::Host)
                && unescape_ok(&host[zone..i], Mode::Zone)
                && unescape_ok(&host[i..], Mode::Host);
        }
    } else if let Some(i) = host.iter().rposition(|&c| c == b':')
        && !valid_optional_port(&host[i..])
    {
        return false;
    }
    unescape_ok(host, Mode::Host)
}
fn valid_userinfo(s: &[u8]) -> bool {
    s.iter().all(|&c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'-' | b'.'
                    | b'_'
                    | b':'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
                    | b'@'
            )
    })
}
fn authority_ok(authority: &[u8]) -> bool {
    let Some(i) = authority.iter().rposition(|&c| c == b'@') else {
        return host_ok(authority);
    };
    if !host_ok(&authority[i + 1..]) {
        return false;
    }
    let userinfo = &authority[..i];
    if !valid_userinfo(userinfo) {
        return false;
    }
    match userinfo.iter().position(|&c| c == b':') {
        None => unescape_ok(userinfo, Mode::UserPassword),
        Some(colon) => {
            unescape_ok(&userinfo[..colon], Mode::UserPassword)
                && unescape_ok(&userinfo[colon + 1..], Mode::UserPassword)
        }
    }
}
pub(crate) fn parses_with_scheme(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let (u, fragment) = match bytes.iter().position(|&c| c == b'#') {
        Some(i) => (&bytes[..i], &bytes[i + 1..]),
        None => (bytes, &bytes[bytes.len()..]),
    };
    if u.iter().any(|&c| c < 0x20 || c == 0x7f) {
        return false;
    }
    let mut scheme_end = None;
    for (i, &c) in u.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            continue;
        }
        if c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.') {
            if i == 0 {
                return false;
            }
            continue;
        }
        if c == b':' && i > 0 {
            scheme_end = Some(i);
        }
        break;
    }
    let Some(scheme_end) = scheme_end else {
        return false;
    };
    let mut rest = &u[scheme_end + 1..];
    let questions = rest.iter().filter(|&&c| c == b'?').count();
    if rest.last() == Some(&b'?') && questions == 1 {
        rest = &rest[..rest.len() - 1];
    } else if let Some(i) = rest.iter().position(|&c| c == b'?') {
        rest = &rest[..i];
    }
    if rest.first() == Some(&b'/') {
        if rest.starts_with(b"//") {
            let after = &rest[2..];
            let (authority, path) = match after.iter().position(|&c| c == b'/') {
                Some(i) => (&after[..i], &after[i..]),
                None => (after, &after[after.len()..]),
            };
            if !authority_ok(authority) {
                return false;
            }
            rest = path;
        }
        if !unescape_ok(rest, Mode::Path) {
            return false;
        }
    }
    unescape_ok(fragment, Mode::Fragment)
}
#[cfg(test)]
mod tests {
    use super::parses_with_scheme;
    #[test]
    fn scheme_and_escapes() {
        assert!(parses_with_scheme("urn:oid:1.2.3"));
        assert!(parses_with_scheme("http://example.org/a%20b"));
        assert!(!parses_with_scheme("no scheme"));
        assert!(!parses_with_scheme(":x"));
        assert!(!parses_with_scheme("1a:x"));
        assert!(!parses_with_scheme("http://host:port/"));
        assert!(!parses_with_scheme("http://example.org/%zz"));
        assert!(!parses_with_scheme("urn:x#%"));
    }
}
