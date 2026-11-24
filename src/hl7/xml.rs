use super::error::{Error, Sentinel};
use super::parse::{
    DEFAULT_MAX_FIELDS, DEFAULT_MAX_REPETITIONS, DEFAULT_MAX_SEGMENTS, read_delimiters,
    valid_segment_name,
};
use super::{Delimiters, FieldExt, Message, Segment};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmlOptions {
    pub max_bytes: i64,
    pub max_nodes: i64,
    pub max_segments: i64,
    pub max_fields: i64,
    pub max_repetitions: i64,
}
impl XmlOptions {
    fn invalid(&self) -> bool {
        self.max_bytes < 0
            || self.max_nodes < 0
            || self.max_segments < 0
            || self.max_fields < 0
            || self.max_repetitions < 0
    }
    fn defaults(&self) -> XmlOptions {
        let pick = |v: i64, d: i64| if v <= 0 { d } else { v };
        XmlOptions {
            max_bytes: pick(self.max_bytes, 8 << 20),
            max_nodes: pick(self.max_nodes, 100000),
            max_segments: pick(self.max_segments, DEFAULT_MAX_SEGMENTS),
            max_fields: pick(self.max_fields, DEFAULT_MAX_FIELDS),
            max_repetitions: pick(self.max_repetitions, DEFAULT_MAX_REPETITIONS),
        }
    }
}
fn xml_err() -> Error {
    Error::Sentinel(Sentinel::Xml)
}
fn limit_err() -> Error {
    Error::Sentinel(Sentinel::Limit)
}
fn xml_delimiters(field: &[u8], encoding: &[u8]) -> Result<Delimiters, Error> {
    if field.len() != 1 {
        return Err(xml_err());
    }
    let mut raw = b"MSH".to_vec();
    raw.extend_from_slice(field);
    raw.extend_from_slice(encoding);
    raw.extend_from_slice(field);
    read_delimiters(&raw).map_err(|_| xml_err())
}
fn validate_xml_message(m: Option<&Message>, o: &XmlOptions) -> Result<(), Error> {
    let Some(m) = m else {
        return Err(xml_err());
    };
    if m.segments.first().is_none_or(|s| s.name != "MSH") {
        return Err(xml_err());
    }
    let d = xml_delimiters(
        &[m.delimiters.field],
        m.delimiters.encoding_characters().as_bytes(),
    );
    match d {
        Ok(d) if d == m.delimiters => {}
        _ => return Err(xml_err()),
    }
    let d = m.delimiters;
    if m.segments.len() as i64 > o.max_segments {
        return Err(limit_err());
    }
    let mut nodes = o.max_nodes;
    let mut text_bytes = o.max_bytes;
    let mut charge = |n: i64| {
        if n > nodes {
            return false;
        }
        nodes -= n;
        true
    };
    if !charge(2) {
        return Err(limit_err());
    }
    for (i, s) in m.segments.iter().enumerate() {
        if !valid_segment_name(&s.name) || (i > 0 && s.name == "MSH") {
            return Err(xml_err());
        }
        if s.fields.len() as i64 > o.max_fields || !charge(2) {
            return Err(limit_err());
        }
        for f in &s.fields {
            if f.len() as i64 > o.max_repetitions || !charge(2) {
                return Err(limit_err());
            }
            for r in f {
                if !charge(2) {
                    return Err(limit_err());
                }
                for c in r {
                    if !charge(2) {
                        return Err(limit_err());
                    }
                    for sub in c {
                        if !charge(3) || sub.len() as i64 > text_bytes {
                            return Err(limit_err());
                        }
                        text_bytes -= sub.len() as i64;
                        for r in sub.chars() {
                            let v = u32::from(r);
                            if v != 9
                                && v != 10
                                && v != 13
                                && (v < 0x20 || v == 0xfffe || v == 0xffff)
                            {
                                return Err(xml_err());
                            }
                        }
                    }
                }
            }
        }
    }
    let msh = &m.segments[0];
    if msh.fields.len() < 2
        || msh.field(1).string().as_bytes() != [d.field]
        || msh.field(2).string() != d.encoding_characters()
    {
        return Err(xml_err());
    }
    for n in [1, 2] {
        let f = msh.field(n);
        if f.len() != 1 || f[0].len() != 1 || f[0][0].len() != 1 {
            return Err(xml_err());
        }
    }
    Ok(())
}
struct XmlWriter {
    buf: Vec<u8>,
    err: Option<Error>,
    out: Vec<u8>,
    remaining: i64,
    tags: Vec<String>,
}
impl XmlWriter {
    fn sink(&mut self, p: &[u8]) -> Result<usize, Error> {
        if p.len() as i64 > self.remaining {
            return Err(limit_err());
        }
        self.remaining -= p.len() as i64;
        self.out.extend_from_slice(p);
        Ok(p.len())
    }
    fn flush(&mut self) -> Result<(), Error> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.buf);
        if let Err(e) = self.sink(&data) {
            self.buf = data;
            self.err = Some(e.clone());
            return Err(e);
        }
        Ok(())
    }
    fn write(&mut self, mut p: &[u8]) {
        while p.len() > 4096 - self.buf.len() && self.err.is_none() {
            let n;
            if self.buf.is_empty() {
                match self.sink(p) {
                    Ok(k) => n = k,
                    Err(e) => {
                        self.err = Some(e);
                        n = 0;
                    }
                }
            } else {
                n = 4096 - self.buf.len();
                self.buf.extend_from_slice(&p[..n]);
                let _ = self.flush();
            }
            p = &p[n..];
        }
        if self.err.is_some() {
            return;
        }
        self.buf.extend_from_slice(p);
    }
    fn write_byte(&mut self, b: u8) {
        if self.err.is_some() {
            return;
        }
        if self.buf.len() >= 4096 && self.flush().is_err() {
            return;
        }
        self.buf.push(b);
    }
    fn cached_error(&self) -> Result<(), Error> {
        match &self.err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn escape_string(&mut self, s: &str, escape_newline: bool) {
        let mut last = 0;
        for (i, r) in s.char_indices() {
            let esc: &[u8] = match r {
                '"' => b"&#34;",
                '\'' => b"&#39;",
                '&' => b"&amp;",
                '<' => b"&lt;",
                '>' => b"&gt;",
                '\t' => b"&#x9;",
                '\n' => {
                    if !escape_newline {
                        continue;
                    }
                    b"&#xA;"
                }
                '\r' => b"&#xD;",
                _ => {
                    if !in_character_range(r) {
                        "\u{FFFD}".as_bytes()
                    } else {
                        continue;
                    }
                }
            };
            self.write(&s.as_bytes()[last..i]);
            self.write(esc);
            last = i + r.len_utf8();
        }
        self.write(&s.as_bytes()[last..]);
    }
    fn start(&mut self, name: &str, attrs: &[(&str, &str)]) -> Result<(), Error> {
        self.tags.push(name.to_owned());
        self.write_byte(b'<');
        self.write(name.as_bytes());
        for (k, v) in attrs {
            self.write_byte(b' ');
            self.write(k.as_bytes());
            self.write(b"=\"");
            self.escape_string(v, true);
            self.write_byte(b'"');
        }
        self.write_byte(b'>');
        self.cached_error()
    }
    fn end(&mut self, name: &str) -> Result<(), Error> {
        self.tags.pop();
        self.write(b"</");
        self.write(name.as_bytes());
        self.write_byte(b'>');
        self.cached_error()
    }
    fn char_data(&mut self, s: &str) -> Result<(), Error> {
        self.escape_string(s, false);
        self.cached_error()
    }
}
fn in_character_range(r: char) -> bool {
    let r = u32::from(r);
    r == 0x09
        || r == 0x0A
        || r == 0x0D
        || (0x20..=0xD7FF).contains(&r)
        || (0xE000..=0xFFFD).contains(&r)
        || (0x10000..=0x10FFFF).contains(&r)
}
pub fn marshal_xml_message(m: Option<&Message>, options: &XmlOptions) -> Result<Vec<u8>, Error> {
    if options.invalid() {
        return Err(limit_err());
    }
    let o = options.defaults();
    validate_xml_message(m, &o)?;
    let m = m.expect("validated");
    let mut w = XmlWriter {
        buf: Vec::with_capacity(4096),
        err: None,
        out: Vec::new(),
        remaining: o.max_bytes,
        tags: Vec::new(),
    };
    let field = (m.delimiters.field as char).to_string();
    let encoding = m.delimiters.encoding_characters();
    w.start(
        "hl7-message",
        &[("version", "1"), ("field", &field), ("encoding", &encoding)],
    )?;
    for s in &m.segments {
        w.start("segment", &[("name", &s.name)])?;
        for f in &s.fields {
            w.start("field", &[])?;
            for r in f {
                w.start("repetition", &[])?;
                for c in r {
                    w.start("component", &[])?;
                    for sub in c {
                        w.start("subcomponent", &[])?;
                        if w.char_data(sub).is_err() {
                            return Err(xml_err());
                        }
                        w.end("subcomponent")?;
                    }
                    w.end("component")?;
                }
                w.end("repetition")?;
            }
            w.end("field")?;
        }
        w.end("segment")?;
    }
    w.end("hl7-message")?;
    if let Err(e) = w.flush() {
        if e.is(Sentinel::Limit) {
            return Err(limit_err());
        }
        return Err(xml_err());
    }
    Ok(w.out)
}
enum Token {
    Start {
        space: String,
        local: String,
        attrs: Vec<(String, String, String)>,
    },
    End {
        space: String,
    },
    CharData(Vec<u8>),
    ProcInst(String),
    Other,
}
struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    next_byte: Option<u8>,
    failed: bool,
    need_close: Option<(String, String)>,
    stack: Vec<(String, String)>,
    eof: bool,
}
type Tok<T> = Result<T, ()>;
fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b':' || c == b'.' || c == b'-'
}
fn is_name(s: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(s) else {
        return false;
    };
    let mut chars = s.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    let first = |c: char| {
        c.is_ascii_alphabetic() || c == '_' || c == ':' || (!c.is_ascii() && c.is_alphabetic())
    };
    if !first(c) {
        return false;
    }
    chars.all(|c| {
        first(c)
            || c.is_ascii_digit()
            || c == '.'
            || c == '-'
            || (!c.is_ascii() && c.is_alphanumeric())
    })
}
impl<'a> Decoder<'a> {
    fn getc(&mut self) -> Option<u8> {
        if self.failed {
            return None;
        }
        if let Some(b) = self.next_byte.take() {
            return Some(b);
        }
        if self.pos >= self.data.len() {
            self.eof = true;
            return None;
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Some(b)
    }
    fn ungetc(&mut self, b: u8) {
        self.next_byte = Some(b);
    }
    fn mustgetc(&mut self) -> Tok<u8> {
        match self.getc() {
            Some(b) => Ok(b),
            None => {
                self.failed = true;
                Err(())
            }
        }
    }
    fn fail<T>(&mut self) -> Tok<T> {
        self.failed = true;
        Err(())
    }
    fn space(&mut self) {
        while let Some(b) = self.getc() {
            if !matches!(b, b' ' | b'\r' | b'\n' | b'\t') {
                self.ungetc(b);
                return;
            }
        }
    }
    fn read_name(&mut self, buf: &mut Vec<u8>) -> Tok<bool> {
        let b = self.mustgetc()?;
        if b < 0x80 && !is_name_byte(b) {
            self.ungetc(b);
            return Ok(false);
        }
        buf.push(b);
        loop {
            let b = self.mustgetc()?;
            if b < 0x80 && !is_name_byte(b) {
                self.ungetc(b);
                break;
            }
            buf.push(b);
        }
        Ok(true)
    }
    fn name(&mut self) -> Tok<Option<String>> {
        let mut buf = Vec::new();
        if !self.read_name(&mut buf)? {
            return Ok(None);
        }
        if !is_name(&buf) {
            return self.fail();
        }
        Ok(Some(String::from_utf8(buf).expect("checked")))
    }
    fn nsname(&mut self) -> Tok<Option<(String, String)>> {
        let Some(s) = self.name()? else {
            return Ok(None);
        };
        if s.matches(':').count() > 1 {
            return Ok(None);
        }
        match s.split_once(':') {
            Some((space, local)) if !space.is_empty() && !local.is_empty() => {
                Ok(Some((space.to_owned(), local.to_owned())))
            }
            _ => Ok(Some((String::new(), s))),
        }
    }
    fn text(&mut self, quote: i32, cdata: bool) -> Tok<Vec<u8>> {
        let (mut b0, mut b1) = (0u8, 0u8);
        let mut trunc = 0;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let Some(b) = self.getc() else {
                if cdata || self.failed {
                    return self.fail();
                }
                break;
            };
            if quote < 0 && b0 == b']' && b1 == b']' && b == b'>' {
                if cdata {
                    trunc = 2;
                    break;
                }
                return self.fail();
            }
            if b == b'<' && !cdata {
                if quote >= 0 {
                    return self.fail();
                }
                self.ungetc(b'<');
                break;
            }
            if quote >= 0 && i32::from(b) == quote {
                break;
            }
            if b == b'&' && !cdata {
                let before = buf.len();
                buf.push(b'&');
                let mut b = self.mustgetc()?;
                let mut text: Option<String> = None;
                if b == b'#' {
                    buf.push(b);
                    b = self.mustgetc()?;
                    let mut base = 10;
                    if b == b'x' {
                        base = 16;
                        buf.push(b);
                        b = self.mustgetc()?;
                    }
                    let start = buf.len();
                    while b.is_ascii_digit() || (base == 16 && b.is_ascii_hexdigit()) {
                        buf.push(b);
                        b = self.mustgetc()?;
                    }
                    if b != b';' {
                        self.ungetc(b);
                    } else {
                        let s = String::from_utf8(buf[start..].to_vec()).expect("ASCII");
                        buf.push(b';');
                        if let Ok(n) = u64::from_str_radix(&s, base)
                            && n <= 0x10FFFF
                            && !s.is_empty()
                        {
                            text = Some(char::from_u32(n as u32).unwrap_or('\u{FFFD}').to_string());
                        }
                    }
                } else {
                    self.ungetc(b);
                    let mut name = Vec::new();
                    self.read_name(&mut name)?;
                    buf.extend_from_slice(&name);
                    let b = self.mustgetc()?;
                    if b != b';' {
                        self.ungetc(b);
                    } else {
                        buf.push(b';');
                        if is_name(&name) {
                            text = match name.as_slice() {
                                b"lt" => Some("<".into()),
                                b"gt" => Some(">".into()),
                                b"amp" => Some("&".into()),
                                b"apos" => Some("'".into()),
                                b"quot" => Some("\"".into()),
                                _ => None,
                            };
                        }
                    }
                }
                match text {
                    Some(t) => {
                        buf.truncate(before);
                        buf.extend_from_slice(t.as_bytes());
                        b0 = 0;
                        b1 = 0;
                        continue;
                    }
                    None => return self.fail(),
                }
            }
            if b == b'\r' {
                buf.push(b'\n');
            } else if b1 == b'\r' && b == b'\n' {
            } else {
                buf.push(b);
            }
            b0 = b1;
            b1 = b;
        }
        buf.truncate(buf.len() - trunc);
        let Ok(s) = std::str::from_utf8(&buf) else {
            return self.fail();
        };
        if !s.chars().all(in_character_range) {
            return self.fail();
        }
        Ok(buf)
    }
    fn raw_token(&mut self) -> Tok<Option<Token>> {
        if self.failed {
            return Err(());
        }
        if let Some((space, _local)) = self.need_close.take() {
            self.stack.pop();
            return Ok(Some(Token::End { space }));
        }
        let Some(b) = self.getc() else {
            return Ok(None);
        };
        if b != b'<' {
            self.ungetc(b);
            return Ok(Some(Token::CharData(self.text(-1, false)?)));
        }
        let b = self.mustgetc()?;
        match b {
            b'/' => {
                let Some((space, local)) = self.nsname()? else {
                    return self.fail();
                };
                self.space();
                if self.mustgetc()? != b'>' {
                    return self.fail();
                }
                let top = self.stack.pop();
                match top {
                    Some((s, l)) if l == local && s == space => {}
                    _ => return self.fail(),
                }
                return Ok(Some(Token::End { space }));
            }
            b'?' => {
                let Some(target) = self.name()? else {
                    return self.fail();
                };
                self.space();
                let mut data = Vec::new();
                let mut b0 = 0u8;
                loop {
                    let b = self.mustgetc()?;
                    data.push(b);
                    if b0 == b'?' && b == b'>' {
                        break;
                    }
                    b0 = b;
                }
                data.truncate(data.len() - 2);
                if target == "xml" {
                    let content = String::from_utf8_lossy(&data).into_owned();
                    let ver = proc_inst("version", &content);
                    if !ver.is_empty() && ver != "1.0" {
                        return self.fail();
                    }
                    let enc = proc_inst("encoding", &content);
                    if !enc.is_empty() && !enc.eq_ignore_ascii_case("utf-8") {
                        return self.fail();
                    }
                }
                return Ok(Some(Token::ProcInst(target)));
            }
            b'!' => {
                let b = self.mustgetc()?;
                match b {
                    b'-' => {
                        if self.mustgetc()? != b'-' {
                            return self.fail();
                        }
                        let (mut b0, mut b1) = (0u8, 0u8);
                        loop {
                            let b = self.mustgetc()?;
                            if b0 == b'-' && b1 == b'-' {
                                if b != b'>' {
                                    return self.fail();
                                }
                                break;
                            }
                            b0 = b1;
                            b1 = b;
                        }
                        return Ok(Some(Token::Other));
                    }
                    b'[' => {
                        for &want in b"CDATA[" {
                            if self.mustgetc()? != want {
                                return self.fail();
                            }
                        }
                        return Ok(Some(Token::CharData(self.text(-1, true)?)));
                    }
                    _ => {}
                }
                let mut inquote = 0u8;
                let mut depth = 0i64;
                let _ = b;
                loop {
                    let b = self.mustgetc()?;
                    if inquote == 0 && b == b'>' && depth == 0 {
                        break;
                    }
                    if b == inquote {
                        inquote = 0;
                    } else if inquote != 0 {
                    } else if b == b'\'' || b == b'"' {
                        inquote = b;
                    } else if b == b'>' {
                        depth -= 1;
                    } else if b == b'<' {
                        depth += 1;
                    }
                }
                return Ok(Some(Token::Other));
            }
            _ => {}
        }
        self.ungetc(b);
        let Some((space, local)) = self.nsname()? else {
            return self.fail();
        };
        let mut attrs = Vec::new();
        let mut empty = false;
        loop {
            self.space();
            let b = self.mustgetc()?;
            if b == b'/' {
                empty = true;
                if self.mustgetc()? != b'>' {
                    return self.fail();
                }
                break;
            }
            if b == b'>' {
                break;
            }
            self.ungetc(b);
            let Some((aspace, alocal)) = self.nsname()? else {
                return self.fail();
            };
            self.space();
            if self.mustgetc()? != b'=' {
                return self.fail();
            }
            self.space();
            let q = self.mustgetc()?;
            if q != b'"' && q != b'\'' {
                return self.fail();
            }
            let value = self.text(i32::from(q), false)?;
            attrs.push((
                aspace,
                alocal,
                String::from_utf8(value).expect("checked UTF-8"),
            ));
        }
        self.stack.push((space.clone(), local.clone()));
        if empty {
            self.need_close = Some((space.clone(), local.clone()));
        }
        Ok(Some(Token::Start {
            space,
            local,
            attrs,
        }))
    }
}
fn proc_inst(param: &str, s: &str) -> String {
    let param = format!("{param}=");
    let lenp = param.len();
    let sb = s.as_bytes();
    let mut i = 0;
    let mut sep = 0u8;
    while i < sb.len() {
        let sub = &s[i..];
        let Some(k) = sub.find(&param) else {
            return String::new();
        };
        if lenp + k >= sub.len() {
            return String::new();
        }
        let c = sub.as_bytes()[lenp + k];
        i += lenp + k + 1;
        if c == b'\'' || c == b'"' {
            sep = c;
            break;
        }
    }
    if sep == 0 {
        return String::new();
    }
    match sb[i..].iter().position(|&c| c == sep) {
        Some(j) => String::from_utf8_lossy(&sb[i..i + j]).into_owned(),
        None => String::new(),
    }
}
fn xml_attrs(
    space: &str,
    attrs: &[(String, String, String)],
    keys: &[&str],
) -> Result<std::collections::HashMap<String, String>, Error> {
    if !space.is_empty() {
        return Err(xml_err());
    }
    let mut out = std::collections::HashMap::new();
    for (aspace, local, value) in attrs {
        if !aspace.is_empty() {
            return Err(xml_err());
        }
        if !keys.contains(&local.as_str()) {
            return Err(xml_err());
        }
        if out.contains_key(local) {
            return Err(xml_err());
        }
        out.insert(local.clone(), value.clone());
    }
    if out.len() != keys.len() {
        return Err(xml_err());
    }
    Ok(out)
}
pub fn parse_xml_message(data: &[u8], options: &XmlOptions) -> Result<Message, Error> {
    if options.invalid() {
        return Err(limit_err());
    }
    let o = options.defaults();
    if data.len() as i64 > o.max_bytes {
        return Err(limit_err());
    }
    let mut decoder = Decoder {
        data,
        pos: 0,
        next_byte: None,
        failed: false,
        need_close: None,
        stack: Vec::new(),
        eof: false,
    };
    let mut remaining = o.max_nodes;
    let mut stack: Vec<String> = Vec::new();
    let mut m: Option<Message> = None;
    let mut text: Vec<u8> = Vec::new();
    let (mut finished, mut seen_token) = (false, false);
    loop {
        let token = match decoder.raw_token() {
            Err(()) => return Err(xml_err()),
            Ok(None) => {
                if !finished || !stack.is_empty() || !decoder.stack.is_empty() {
                    return Err(xml_err());
                }
                break;
            }
            Ok(Some(t)) => t,
        };
        if remaining == 0 {
            return Err(limit_err());
        }
        remaining -= 1;
        match token {
            Token::ProcInst(target) => {
                if seen_token || target != "xml" {
                    return Err(xml_err());
                }
                seen_token = true;
            }
            Token::Start {
                space,
                local,
                attrs,
            } => {
                seen_token = true;
                if finished || !space.is_empty() {
                    return Err(xml_err());
                }
                let parent = stack.last().map(String::as_str).unwrap_or("");
                match local.as_str() {
                    "hl7-message" => {
                        if !parent.is_empty() || m.is_some() {
                            return Err(xml_err());
                        }
                        let a = xml_attrs(&space, &attrs, &["version", "field", "encoding"])
                            .map_err(|_| xml_err())?;
                        if a["version"] != "1" {
                            return Err(xml_err());
                        }
                        let d = xml_delimiters(a["field"].as_bytes(), a["encoding"].as_bytes())?;
                        m = Some(Message {
                            delimiters: d,
                            segments: Vec::new(),
                        });
                    }
                    "segment" => {
                        if parent != "hl7-message" {
                            return Err(xml_err());
                        }
                        let a = xml_attrs(&space, &attrs, &["name"]).map_err(|_| xml_err())?;
                        if !valid_segment_name(&a["name"]) {
                            return Err(xml_err());
                        }
                        let msg = m.as_mut().expect("root");
                        if msg.segments.len() as i64 >= o.max_segments {
                            return Err(limit_err());
                        }
                        msg.segments.push(Segment::new(&a["name"]));
                    }
                    "field" => {
                        if parent != "segment" {
                            return Err(xml_err());
                        }
                        xml_attrs(&space, &attrs, &[])?;
                        let s = m
                            .as_mut()
                            .expect("root")
                            .segments
                            .last_mut()
                            .expect("segment");
                        if s.fields.len() as i64 >= o.max_fields {
                            return Err(limit_err());
                        }
                        s.fields.push(Vec::new());
                    }
                    "repetition" => {
                        if parent != "field" {
                            return Err(xml_err());
                        }
                        xml_attrs(&space, &attrs, &[])?;
                        let s = m
                            .as_mut()
                            .expect("root")
                            .segments
                            .last_mut()
                            .expect("segment");
                        let f = s.fields.last_mut().expect("field");
                        if f.len() as i64 >= o.max_repetitions {
                            return Err(limit_err());
                        }
                        f.push(Vec::new());
                    }
                    "component" => {
                        if parent != "repetition" {
                            return Err(xml_err());
                        }
                        xml_attrs(&space, &attrs, &[])?;
                        let s = m
                            .as_mut()
                            .expect("root")
                            .segments
                            .last_mut()
                            .expect("segment");
                        let r = s
                            .fields
                            .last_mut()
                            .and_then(|f| f.last_mut())
                            .expect("repetition");
                        r.push(Vec::new());
                    }
                    "subcomponent" => {
                        if parent != "component" {
                            return Err(xml_err());
                        }
                        xml_attrs(&space, &attrs, &[])?;
                        text.clear();
                    }
                    _ => return Err(xml_err()),
                }
                stack.push(local);
            }
            Token::End { space } => {
                let Some(top) = stack.last() else {
                    return Err(xml_err());
                };
                if !space.is_empty() {
                    return Err(xml_err());
                }
                if top == "subcomponent" {
                    let s = m
                        .as_mut()
                        .expect("root")
                        .segments
                        .last_mut()
                        .expect("segment");
                    let c = s
                        .fields
                        .last_mut()
                        .and_then(|f| f.last_mut())
                        .and_then(|r| r.last_mut())
                        .expect("component");
                    c.push(String::from_utf8(std::mem::take(&mut text)).expect("UTF-8"));
                }
                let top = stack.pop().expect("non-empty");
                if top == "hl7-message" {
                    finished = true;
                }
            }
            Token::CharData(t) => {
                if stack.last().is_some_and(|s| s == "subcomponent") {
                    text.extend_from_slice(&t);
                } else if !String::from_utf8_lossy(&t).trim().is_empty() {
                    return Err(xml_err());
                }
            }
            Token::Other => return Err(xml_err()),
        }
    }
    validate_xml_message(m.as_ref(), &o)?;
    Ok(m.expect("validated"))
}
