struct SplitUrl {
    scheme: String,
    opaque: String,
    host_set: bool,
    path_set: bool,
}
fn split_url(raw: &str) -> Option<SplitUrl> {
    let (u, frag) = match raw.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (raw, None),
    };
    let url = split_url_inner(u)?;
    if let Some(f) = frag {
        url_unescape_check(f, false)?;
    }
    Some(url)
}
fn split_url_inner(raw: &str) -> Option<SplitUrl> {
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    let mut url = SplitUrl {
        scheme: String::new(),
        opaque: String::new(),
        host_set: false,
        path_set: false,
    };
    if raw == "*" {
        url.path_set = true;
        return Some(url);
    }
    let (scheme, rest) = url_scheme(raw)?;
    url.scheme = scheme.to_ascii_lowercase();
    let rest = if rest.ends_with('?') && rest.matches('?').count() == 1 {
        &rest[..rest.len() - 1]
    } else {
        rest.split_once('?').map_or(rest, |(a, _)| a)
    };
    if !rest.starts_with('/') {
        if !url.scheme.is_empty() {
            url.opaque = rest.to_string();
            return Some(url);
        }
        let segment = rest.split_once('/').map_or(rest, |(a, _)| a);
        if segment.contains(':') {
            return None;
        }
    }
    let mut path = rest;
    if (!url.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
        let authority_rest = &rest[2..];
        let (authority, p) = match authority_rest.find('/') {
            Some(i) => (&authority_rest[..i], &authority_rest[i..]),
            None => (authority_rest, ""),
        };
        let host = url_authority(authority)?;
        url.host_set = !host.is_empty();
        path = p;
    }
    url_unescape_check(path, false)?;
    url.path_set = !path.is_empty();
    Some(url)
}
fn url_scheme(raw: &str) -> Option<(&str, &str)> {
    for (i, c) in raw.bytes().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Some(("", raw));
                }
            }
            b':' => {
                if i == 0 {
                    return None;
                }
                return Some((&raw[..i], &raw[i + 1..]));
            }
            _ => return Some(("", raw)),
        }
    }
    Some(("", raw))
}
fn url_authority(authority: &str) -> Option<&str> {
    let (userinfo, host) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    url_host(host)?;
    if let Some(userinfo) = userinfo {
        let valid = userinfo.bytes().all(|c| {
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
        });
        if !valid {
            return None;
        }
        url_unescape_check(userinfo, false)?;
    }
    Some(host)
}
fn url_host(host: &str) -> Option<()> {
    let valid_port = |port: &str| {
        port.is_empty()
            || port
                .strip_prefix(':')
                .is_some_and(|p| p.bytes().all(|c| c.is_ascii_digit()))
    };
    if host.starts_with('[') {
        let i = host.rfind(']')?;
        if !valid_port(&host[i + 1..]) {
            return None;
        }
    } else if let Some(i) = host.rfind(':')
        && !valid_port(&host[i..])
    {
        return None;
    }
    url_unescape_check(host, true)
}
fn url_unescape_check(s: &str, host: bool) -> Option<()> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                if i + 2 >= b.len()
                    || !b[i + 1].is_ascii_hexdigit()
                    || !b[i + 2].is_ascii_hexdigit()
                {
                    return None;
                }
                if host && unhex(b[i + 1]) < 8 && &s[i..i + 3] != "%25" {
                    return None;
                }
                i += 3;
            }
            c if host && c < 0x80 && should_escape_host(c) => return None,
            _ => i += 1,
        }
    }
    Some(())
}
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}
fn should_escape_host(c: u8) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    !matches!(
        c,
        b'-' | b'_'
            | b'.'
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
            | b':'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'"'
    )
}
pub(crate) fn absolute_code_uri(value: &str) -> bool {
    match split_url(value) {
        Some(u) => !u.scheme.is_empty() && (!u.opaque.is_empty() || u.host_set || u.path_set),
        None => false,
    }
}
