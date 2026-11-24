pub fn trim_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
        )
}
fn is_tspecial(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')'
            | b'<'
            | b'>'
            | b'@'
            | b','
            | b';'
            | b':'
            | b'\\'
            | b'"'
            | b'/'
            | b'['
            | b']'
            | b'?'
            | b'='
    )
}
fn consume_token(v: &[u8]) -> (&[u8], &[u8]) {
    let end = v.iter().position(|&c| !is_token_char(c)).unwrap_or(v.len());
    v.split_at(end)
}
fn consume_value(v: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    if v.is_empty() {
        return None;
    }
    if v[0] != b'"' {
        let (token, rest) = consume_token(v);
        if token.is_empty() {
            return None;
        }
        return Some((token.to_vec(), rest));
    }
    let mut buffer = Vec::new();
    let mut i = 1;
    while i < v.len() {
        let r = v[i];
        if r == b'"' {
            return Some((buffer, &v[i + 1..]));
        }
        if r == b'\\' && i + 1 < v.len() && is_tspecial(v[i + 1]) {
            buffer.push(v[i + 1]);
            i += 2;
            continue;
        }
        if r == b'\r' || r == b'\n' {
            return None;
        }
        buffer.push(r);
        i += 1;
    }
    None
}
pub fn to_lower(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\u{130}' {
                return 'i';
            }
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        })
        .collect()
}
fn trim_left_space(v: &[u8]) -> &[u8] {
    match std::str::from_utf8(v) {
        Ok(s) => s.trim_start_matches(char::is_whitespace).as_bytes(),
        Err(_) => {
            let n = v
                .iter()
                .position(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r'))
                .unwrap_or(v.len());
            &v[n..]
        }
    }
}
fn consume_media_param(v: &[u8]) -> Option<(String, Vec<u8>, &[u8])> {
    let rest = trim_left_space(v);
    let rest = rest.strip_prefix(b";")?;
    let rest = trim_left_space(rest);
    let (param, rest) = consume_token(rest);
    if param.is_empty() {
        return None;
    }
    let param = String::from_utf8_lossy(param).to_ascii_lowercase();
    let rest = trim_left_space(rest);
    let rest = rest.strip_prefix(b"=")?;
    let rest = trim_left_space(rest);
    let (value, rest) = consume_value(rest)?;
    Some((param, value, rest))
}
fn percent_hex_unescape(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'%' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        if i + 2 >= s.len() || !s[i + 1].is_ascii_hexdigit() || !s[i + 2].is_ascii_hexdigit() {
            return None;
        }
        let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
        out.push((hex(s[i + 1]) << 4) | hex(s[i + 2]));
        i += 3;
    }
    Some(out)
}
fn decode_2231_enc(v: &[u8]) -> Option<Vec<u8>> {
    let first = v.iter().position(|&c| c == b'\'')?;
    let charset = String::from_utf8_lossy(&v[..first]).to_lowercase();
    let rest = &v[first + 1..];
    let second = rest.iter().position(|&c| c == b'\'')?;
    if charset != "us-ascii" && charset != "utf-8" {
        return None;
    }
    percent_hex_unescape(&rest[second + 1..])
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaType {
    pub media_type: String,
    pub params: Vec<(String, String)>,
}
impl MediaType {
    pub fn param(&self, name: &str) -> &str {
        self.params
            .iter()
            .find(|(k, _)| k == name)
            .map_or("", |(_, v)| v.as_str())
    }
}
type RawParams = Vec<(String, Vec<u8>)>;
pub fn parse_media_type(v: &str) -> Result<MediaType, String> {
    let base = v.split(';').next().unwrap_or("");
    let media_type = trim_space(&to_lower(base)).to_string();
    {
        let (typ, rest) = consume_token(media_type.as_bytes());
        if typ.is_empty() {
            return Err("mime: no media type".into());
        }
        if !rest.is_empty() {
            let Some(rest) = rest.strip_prefix(b"/") else {
                return Err("mime: expected slash after first token".into());
            };
            let (subtype, rest) = consume_token(rest);
            if subtype.is_empty() {
                return Err("mime: expected token after slash".into());
            }
            if !rest.is_empty() {
                return Err("mime: unexpected content after media subtype".into());
            }
        }
    }
    let mut params: RawParams = Vec::new();
    let mut continuation: Vec<(String, RawParams)> = Vec::new();
    let mut rest = &v.as_bytes()[base.len()..];
    while !rest.is_empty() {
        rest = trim_left_space(rest);
        if rest.is_empty() {
            break;
        }
        let Some((key, value, after)) = consume_media_param(rest) else {
            if trim_space(&String::from_utf8_lossy(rest)) == ";" {
                break;
            }
            return Err("mime: invalid media parameter".into());
        };
        let map = match key.split_once('*') {
            Some((base_name, _)) => {
                let index = match continuation.iter().position(|(k, _)| k == base_name) {
                    Some(index) => index,
                    None => {
                        continuation.push((base_name.to_string(), Vec::new()));
                        continuation.len() - 1
                    }
                };
                &mut continuation[index].1
            }
            None => &mut params,
        };
        match map.iter_mut().find(|(k, _)| *k == key) {
            Some((_, existing)) => {
                if *existing != value {
                    return Err("mime: duplicate parameter name".into());
                }
            }
            None => map.push((key, value)),
        }
        rest = after;
    }
    let mut set = |key: &str, value: Vec<u8>| match params.iter_mut().find(|(k, _)| k == key) {
        Some((_, slot)) => *slot = value,
        None => params.push((key.to_string(), value)),
    };
    for (key, pieces) in &continuation {
        let piece = |name: &str| pieces.iter().find(|(k, _)| k == name).map(|(_, v)| v);
        if let Some(v) = piece(&format!("{key}*")) {
            if let Some(decoded) = decode_2231_enc(v) {
                set(key, decoded);
            }
            continue;
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut valid = false;
        let mut n = 0;
        loop {
            let simple = format!("{key}*{n}");
            if let Some(v) = piece(&simple) {
                valid = true;
                buf.extend_from_slice(v);
                n += 1;
                continue;
            }
            let Some(v) = piece(&format!("{simple}*")) else {
                break;
            };
            valid = true;
            if n == 0 {
                if let Some(decoded) = decode_2231_enc(v) {
                    buf.extend_from_slice(&decoded);
                }
            } else if let Some(decoded) = percent_hex_unescape(v) {
                buf.extend_from_slice(&decoded);
            }
            n += 1;
        }
        if valid {
            set(key, buf);
        }
    }
    Ok(MediaType {
        media_type,
        params: params
            .into_iter()
            .map(|(k, v)| (k, String::from_utf8_lossy(&v).into_owned()))
            .collect(),
    })
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header(pub Vec<(String, Vec<String>)>);
pub fn canonical_header_key(key: &str) -> String {
    canonical_key_bytes(key.as_bytes()).map_or_else(|| key.to_string(), |(canonical, _)| canonical)
}
fn canonical_key_bytes(a: &[u8]) -> Option<(String, bool)> {
    if a.is_empty() {
        return None;
    }
    let mut no_canon = false;
    for &c in a {
        if is_token_char(c) {
            continue;
        }
        if c == b' ' {
            no_canon = true;
            continue;
        }
        return None;
    }
    if no_canon {
        return Some((String::from_utf8_lossy(a).into_owned(), false));
    }
    let mut out = Vec::with_capacity(a.len());
    let mut upper = true;
    for &c in a {
        let c = if upper {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        };
        out.push(c);
        upper = c == b'-';
    }
    Some((String::from_utf8_lossy(&out).into_owned(), true))
}
impl Header {
    pub fn new() -> Header {
        Header::default()
    }
    pub fn values(&self, key: &str) -> &[String] {
        let key = canonical_header_key(key);
        self.0
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(&[], |(_, v)| v.as_slice())
    }
    pub fn get(&self, key: &str) -> &str {
        self.values(key).first().map_or("", String::as_str)
    }
    pub fn add(&mut self, key: &str, value: &str) {
        let key = canonical_header_key(key);
        self.add_raw(key, value.to_string());
    }
    pub fn set(&mut self, key: &str, value: &str) {
        let key = canonical_header_key(key);
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => *values = vec![value.to_string()],
            None => self.0.push((key, vec![value.to_string()])),
        }
    }
    pub fn del(&mut self, key: &str) {
        let key = canonical_header_key(key);
        self.0.retain(|(k, _)| *k != key);
    }
    fn add_raw(&mut self, key: String, value: String) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, values)) => values.push(value),
            None => self.0.push((key, vec![value])),
        }
    }
}
fn trim_lws(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t'))
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|c| !matches!(c, b' ' | b'\t'))
        .map_or(start, |i| i + 1);
    &line[start..end]
}
fn read_line<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos >= data.len() {
        return None;
    }
    let rest = &data[*pos..];
    match rest.iter().position(|&c| c == b'\n') {
        Some(i) => {
            *pos += i + 1;
            let line = &rest[..i];
            Some(line.strip_suffix(b"\r").unwrap_or(line))
        }
        None => {
            *pos = data.len();
            Some(rest)
        }
    }
}
pub fn read_mime_header(data: &[u8]) -> Result<Header, String> {
    let mut header = Header::new();
    if matches!(data.first(), Some(b' ' | b'\t')) {
        return Err("malformed MIME header initial line".into());
    }
    let mut pos = 0;
    loop {
        let Some(line) = read_line(data, &mut pos) else {
            return Err("EOF".into());
        };
        if line.is_empty() {
            return Ok(header);
        }
        if !line.contains(&b':') {
            return Err("malformed MIME header: missing colon".into());
        }
        let mut kv: Vec<u8> = trim_lws(line).to_vec();
        while matches!(data.get(pos), Some(b' ' | b'\t')) {
            while matches!(data.get(pos), Some(b' ' | b'\t')) {
                pos += 1;
            }
            kv.push(b' ');
            match read_line(data, &mut pos) {
                Some(line) => kv.extend_from_slice(trim_lws(line)),
                None => break,
            }
        }
        let colon = kv.iter().position(|&c| c == b':').unwrap_or(kv.len());
        if colon == kv.len() {
            return Err("malformed MIME header line".into());
        }
        let Some((key, _)) = canonical_key_bytes(&kv[..colon]) else {
            return Err("malformed MIME header line".into());
        };
        let value = &kv[colon + 1..];
        if value.iter().any(|&c| (c < 0x20 && c != b'\t') || c == 0x7f) {
            return Err("malformed MIME header line".into());
        }
        let start = value
            .iter()
            .position(|c| !matches!(c, b' ' | b'\t'))
            .unwrap_or(value.len());
        header.add_raw(key, String::from_utf8_lossy(&value[start..]).into_owned());
    }
}
