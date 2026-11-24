#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::collections::HashMap;
pub const XML_URL: &str = "http://www.w3.org/XML/1998/namespace";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    pub space: String,
    pub local: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    pub name: Name,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Start(Name, Vec<Attr>),
    End(Name),
    CharData(Vec<u8>),
    Comment(Vec<u8>),
    ProcInst(String, Vec<u8>),
    Directive(Vec<u8>),
}
impl Tok {
    pub fn type_name(&self) -> &'static str {
        match self {
            Tok::Start(..) => "start element",
            Tok::End(_) => "end element",
            Tok::CharData(_) => "character data",
            Tok::Comment(_) => "comment",
            Tok::ProcInst(..) => "processing instruction",
            Tok::Directive(_) => "directive",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XmlError {
    Eof,
    Msg(String),
}
impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmlError::Eof => f.write_str("EOF"),
            XmlError::Msg(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for XmlError {}
pub type CharsetReader = fn(&str, &[u8]) -> Result<Vec<u8>, String>;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Start,
    Ns,
}
struct Stack {
    kind: Kind,
    name: Name,
    ok: bool,
}
pub struct Decoder {
    data: Vec<u8>,
    pos: usize,
    tail_error: Option<String>,
    charset_reader: CharsetReader,
    next_byte: Option<u8>,
    buf: Vec<u8>,
    stk: Vec<Stack>,
    ns: HashMap<String, String>,
    err: Option<XmlError>,
    line: usize,
    need_close: bool,
    to_close: Name,
    max_decoded_bytes: usize,
}
fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-')
}
fn is_name_first(c: char) -> bool {
    c == ':' || c == '_' || c.is_alphabetic()
}
fn is_name_rest(c: char) -> bool {
    is_name_first(c) || c == '.' || c == '-' || c == '\u{b7}' || c.is_alphanumeric()
}
fn is_name(s: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(s) else {
        return false;
    };
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_name_first(c) => chars.all(is_name_rest),
        _ => false,
    }
}
pub fn is_in_character_range(r: u32) -> bool {
    r == 0x09
        || r == 0x0A
        || r == 0x0D
        || (0x20..=0xD7FF).contains(&r)
        || (0xE000..=0xFFFD).contains(&r)
        || (0x10000..=0x10FFFF).contains(&r)
}
pub fn u_plus(r: u32) -> String {
    format!("U+{r:04X}")
}
fn proc_inst(param: &str, s: &str) -> String {
    let param = format!("{param}=");
    let (needle, bytes) = (param.as_bytes(), s.as_bytes());
    let lenp = needle.len();
    let mut i = 0;
    let mut sep = 0u8;
    while i < bytes.len() {
        let sub = bytes.get(i..).unwrap_or_default();
        let Some(k) = sub.windows(lenp).position(|w| w == needle) else {
            return String::new();
        };
        let at = lenp.saturating_add(k);
        let Some(&c) = sub.get(at) else {
            return String::new();
        };
        i = i.saturating_add(at).saturating_add(1);
        if c == b'\'' || c == b'"' {
            sep = c;
            break;
        }
    }
    if sep == 0 {
        return String::new();
    }
    match bytes
        .get(i..)
        .unwrap_or_default()
        .iter()
        .position(|&b| b == sep)
    {
        Some(j) => s
            .get(i..i.saturating_add(j))
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    }
}
impl Decoder {
    pub fn new(
        data: Vec<u8>,
        tail_error: Option<String>,
        charset_reader: CharsetReader,
    ) -> Decoder {
        Decoder {
            data,
            pos: 0,
            tail_error,
            charset_reader,
            next_byte: None,
            buf: Vec::new(),
            stk: Vec::new(),
            ns: HashMap::new(),
            err: None,
            line: 1,
            need_close: false,
            to_close: Name::default(),
            max_decoded_bytes: usize::MAX,
        }
    }
    pub(crate) fn set_max_decoded_bytes(&mut self, max: usize) {
        self.max_decoded_bytes = max;
    }
    fn syntax_error(&self, msg: &str) -> XmlError {
        XmlError::Msg(format!("XML syntax error on line {}: {msg}", self.line))
    }
    fn getc(&mut self) -> Option<u8> {
        if self.err.is_some() {
            return None;
        }
        let b = if let Some(b) = self.next_byte.take() {
            b
        } else {
            let Some(&b) = self.data.get(self.pos) else {
                self.err = Some(match &self.tail_error {
                    Some(m) => XmlError::Msg(m.clone()),
                    None => XmlError::Eof,
                });
                return None;
            };
            self.pos = self.pos.saturating_add(1);
            b
        };
        if b == b'\n' {
            self.line = self.line.saturating_add(1);
        }
        Some(b)
    }
    fn mustgetc(&mut self) -> Option<u8> {
        let b = self.getc();
        if b.is_none() && self.err == Some(XmlError::Eof) {
            self.err = Some(self.syntax_error("unexpected EOF"));
        }
        b
    }
    fn ungetc(&mut self, b: u8) {
        if b == b'\n' {
            self.line = self.line.saturating_sub(1);
        }
        self.next_byte = Some(b);
    }
    fn fail<T>(&self) -> Result<T, XmlError> {
        Err(self.err.clone().unwrap_or(XmlError::Eof))
    }
    fn set_err<T>(&mut self, msg: &str) -> Result<T, XmlError> {
        let e = self.syntax_error(msg);
        self.err = Some(e.clone());
        Err(e)
    }
    pub fn token(&mut self) -> Result<Tok, XmlError> {
        let t = match self.raw_token() {
            Ok(t) => t,
            Err(XmlError::Eof) if !self.stk.is_empty() => {
                return Err(self.syntax_error("unexpected EOF"));
            }
            Err(e) => return Err(e),
        };
        match t {
            Tok::Start(mut name, mut attrs) => {
                for a in &attrs {
                    if a.name.space == "xmlns" {
                        let old = self.ns.get(&a.name.local).cloned();
                        self.push_ns(&a.name.local, old);
                        self.ns.insert(a.name.local.clone(), a.value.clone());
                    }
                    if a.name.space.is_empty() && a.name.local == "xmlns" {
                        let old = self.ns.get("").cloned();
                        self.push_ns("", old);
                        self.ns.insert(String::new(), a.value.clone());
                    }
                }
                self.stk.push(Stack {
                    kind: Kind::Start,
                    name: name.clone(),
                    ok: false,
                });
                self.translate(&mut name, true);
                for a in &mut attrs {
                    let mut n = std::mem::take(&mut a.name);
                    self.translate(&mut n, false);
                    a.name = n;
                }
                Ok(Tok::Start(name, attrs))
            }
            Tok::End(mut name) => {
                self.pop_element(&mut name)?;
                Ok(Tok::End(name))
            }
            other => Ok(other),
        }
    }
    fn push_ns(&mut self, local: &str, old: Option<String>) {
        self.stk.push(Stack {
            kind: Kind::Ns,
            name: Name {
                space: old.clone().unwrap_or_default(),
                local: local.to_string(),
            },
            ok: old.is_some(),
        });
    }
    fn translate(&self, n: &mut Name, is_element_name: bool) {
        if n.space == "xmlns" {
            return;
        }
        if n.space.is_empty() && !is_element_name {
            return;
        }
        if n.space == "xml" {
            n.space = XML_URL.to_string();
        } else if n.space.is_empty() && n.local == "xmlns" {
            return;
        }
        if let Some(v) = self.ns.get(&n.space) {
            n.space = v.clone();
        }
    }
    fn pop_element(&mut self, t: &mut Name) -> Result<(), XmlError> {
        let s = self.stk.pop();
        match s {
            Some(s) if s.kind == Kind::Start => {
                if s.name.local != t.local {
                    let msg = format!("element <{}> closed by </{}>", s.name.local, t.local);
                    return self.set_err(&msg);
                }
                if s.name.space != t.space {
                    let ns = if t.space.is_empty() {
                        "\"\"".to_string()
                    } else {
                        t.space.clone()
                    };
                    let msg = format!(
                        "element <{}> in space {} closed by </{}> in space {ns}",
                        s.name.local, s.name.space, t.local
                    );
                    return self.set_err(&msg);
                }
            }
            _ => {
                let msg = format!("unexpected end element </{}>", t.local);
                return self.set_err(&msg);
            }
        }
        self.translate(t, true);
        while let Some(top) = self.stk.last() {
            if top.kind == Kind::Start {
                break;
            }
            let Some(s) = self.stk.pop() else { break };
            if s.ok {
                self.ns.insert(s.name.local, s.name.space);
            } else {
                self.ns.remove(&s.name.local);
            }
        }
        Ok(())
    }
    fn raw_token(&mut self) -> Result<Tok, XmlError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.need_close {
            self.need_close = false;
            return Ok(Tok::End(std::mem::take(&mut self.to_close)));
        }
        let Some(b) = self.getc() else {
            return self.fail();
        };
        if b != b'<' {
            self.ungetc(b);
            return match self.text(-1, false) {
                Some(data) => Ok(Tok::CharData(data)),
                None => self.fail(),
            };
        }
        let Some(b) = self.mustgetc() else {
            return self.fail();
        };
        match b {
            b'/' => {
                let Some(name) = self.nsname() else {
                    if self.err.is_none() {
                        return self.set_err("expected element name after </");
                    }
                    return self.fail();
                };
                self.space();
                let Some(b) = self.mustgetc() else {
                    return self.fail();
                };
                if b != b'>' {
                    let msg = format!("invalid characters between </{} and >", name.local);
                    return self.set_err(&msg);
                }
                Ok(Tok::End(name))
            }
            b'?' => self.proc_inst_token(),
            b'!' => self.bang_token(),
            _ => {
                self.ungetc(b);
                self.start_token()
            }
        }
    }
    fn proc_inst_token(&mut self) -> Result<Tok, XmlError> {
        let Some(target) = self.name() else {
            if self.err.is_none() {
                return self.set_err("expected target name after <?");
            }
            return self.fail();
        };
        self.space();
        self.buf.clear();
        let mut b0 = 0u8;
        loop {
            let Some(b) = self.mustgetc() else {
                return self.fail();
            };
            self.buf.push(b);
            if b0 == b'?' && b == b'>' {
                break;
            }
            b0 = b;
        }
        let data = self
            .buf
            .get(..self.buf.len().saturating_sub(2))
            .unwrap_or_default()
            .to_vec();
        if target == "xml" {
            let content = String::from_utf8_lossy(&data).into_owned();
            let ver = proc_inst("version", &content);
            if !ver.is_empty() && ver != "1.0" {
                let e = XmlError::Msg(format!(
                    "xml: unsupported version {}; only version 1.0 is supported",
                    crate::textfmt::quote(&ver)
                ));
                self.err = Some(e.clone());
                return Err(e);
            }
            let enc = proc_inst("encoding", &content);
            if !enc.is_empty() && !enc.eq_ignore_ascii_case("utf-8") {
                let mut rest = Vec::new();
                if let Some(b) = self.next_byte.take() {
                    rest.push(b);
                }
                rest.extend_from_slice(self.data.get(self.pos..).unwrap_or_default());
                let max = self.max_decoded_bytes;
                let too_long = |len: usize| {
                    XmlError::Msg(format!(
                        "{}: the text decoded from charset {} is {len} bytes, limit {max}",
                        super::ERR_MAX_XML_BYTES,
                        crate::textfmt::quote(&enc),
                    ))
                };
                if rest.len() > max {
                    let e = too_long(rest.len());
                    self.err = Some(e.clone());
                    return Err(e);
                }
                match (self.charset_reader)(&enc, &rest) {
                    Ok(decoded) if decoded.len() > max => {
                        let e = too_long(decoded.len());
                        self.err = Some(e.clone());
                        return Err(e);
                    }
                    Ok(decoded) => {
                        self.data = decoded;
                        self.pos = 0;
                    }
                    Err(err) => {
                        let e = XmlError::Msg(format!(
                            "xml: opening charset {}: {err}",
                            crate::textfmt::quote(&enc)
                        ));
                        self.err = Some(e.clone());
                        return Err(e);
                    }
                }
            }
        }
        Ok(Tok::ProcInst(target, data))
    }
    fn bang_token(&mut self) -> Result<Tok, XmlError> {
        let Some(b) = self.mustgetc() else {
            return self.fail();
        };
        match b {
            b'-' => {
                let Some(b) = self.mustgetc() else {
                    return self.fail();
                };
                if b != b'-' {
                    return self.set_err("invalid sequence <!- not part of <!--");
                }
                self.buf.clear();
                let (mut b0, mut b1) = (0u8, 0u8);
                loop {
                    let Some(b) = self.mustgetc() else {
                        return self.fail();
                    };
                    self.buf.push(b);
                    if b0 == b'-' && b1 == b'-' {
                        if b != b'>' {
                            return self.set_err("invalid sequence \"--\" not allowed in comments");
                        }
                        break;
                    }
                    b0 = b1;
                    b1 = b;
                }
                let n = self.buf.len().saturating_sub(3);
                return Ok(Tok::Comment(self.buf.get(..n).unwrap_or_default().to_vec()));
            }
            b'[' => {
                for want in b"CDATA[" {
                    let Some(b) = self.mustgetc() else {
                        return self.fail();
                    };
                    if b != *want {
                        return self.set_err("invalid <![ sequence");
                    }
                }
                return match self.text(-1, true) {
                    Some(data) => Ok(Tok::CharData(data)),
                    None => self.fail(),
                };
            }
            _ => {}
        }
        self.buf.clear();
        self.buf.push(b);
        let mut inquote = 0u8;
        let mut depth = 0i32;
        loop {
            let Some(mut b) = self.mustgetc() else {
                return self.fail();
            };
            if inquote == 0 && b == b'>' && depth == 0 {
                break;
            }
            'handle: loop {
                self.buf.push(b);
                if b == inquote {
                    inquote = 0;
                } else if inquote != 0 {
                } else if b == b'\'' || b == b'"' {
                    inquote = b;
                } else if b == b'>' {
                    depth = depth.saturating_sub(1);
                } else if b == b'<' {
                    let s = b"!--";
                    for (i, &want) in s.iter().enumerate() {
                        let Some(c) = self.mustgetc() else {
                            return self.fail();
                        };
                        b = c;
                        if b != want {
                            self.buf.extend_from_slice(s.get(..i).unwrap_or_default());
                            depth = depth.saturating_add(1);
                            continue 'handle;
                        }
                    }
                    self.buf.pop();
                    let (mut b0, mut b1) = (0u8, 0u8);
                    loop {
                        let Some(c) = self.mustgetc() else {
                            return self.fail();
                        };
                        if b0 == b'-' && b1 == b'-' && c == b'>' {
                            break;
                        }
                        b0 = b1;
                        b1 = c;
                    }
                    self.buf.push(b' ');
                }
                break;
            }
        }
        Ok(Tok::Directive(self.buf.clone()))
    }
    fn start_token(&mut self) -> Result<Tok, XmlError> {
        let Some(name) = self.nsname() else {
            if self.err.is_none() {
                return self.set_err("expected element name after <");
            }
            return self.fail();
        };
        let mut attrs = Vec::new();
        let mut empty = false;
        loop {
            self.space();
            let Some(b) = self.mustgetc() else {
                return self.fail();
            };
            if b == b'/' {
                empty = true;
                let Some(b) = self.mustgetc() else {
                    return self.fail();
                };
                if b != b'>' {
                    return self.set_err("expected /> in element");
                }
                break;
            }
            if b == b'>' {
                break;
            }
            self.ungetc(b);
            let Some(an) = self.nsname() else {
                if self.err.is_none() {
                    return self.set_err("expected attribute name in element");
                }
                return self.fail();
            };
            self.space();
            let Some(b) = self.mustgetc() else {
                return self.fail();
            };
            if b != b'=' {
                return self.set_err("attribute name without = in element");
            }
            self.space();
            let Some(q) = self.mustgetc() else {
                return self.fail();
            };
            if q != b'"' && q != b'\'' {
                return self.set_err("unquoted or missing attribute value in element");
            }
            let Some(data) = self.text(i32::from(q), false) else {
                return self.fail();
            };
            attrs.push(Attr {
                name: an,
                value: String::from_utf8_lossy(&data).into_owned(),
            });
        }
        if empty {
            self.need_close = true;
            self.to_close = name.clone();
        }
        Ok(Tok::Start(name, attrs))
    }
    fn space(&mut self) {
        while let Some(b) = self.getc() {
            if !matches!(b, b' ' | b'\r' | b'\n' | b'\t') {
                self.ungetc(b);
                return;
            }
        }
    }
    fn read_name(&mut self) -> bool {
        let Some(b) = self.mustgetc() else {
            return false;
        };
        if b < 0x80 && !is_name_byte(b) {
            self.ungetc(b);
            return false;
        }
        self.buf.push(b);
        loop {
            let Some(b) = self.mustgetc() else {
                return false;
            };
            if b < 0x80 && !is_name_byte(b) {
                self.ungetc(b);
                return true;
            }
            self.buf.push(b);
        }
    }
    fn name(&mut self) -> Option<String> {
        self.buf.clear();
        if !self.read_name() {
            return None;
        }
        if !is_name(&self.buf) {
            let msg = format!("invalid XML name: {}", String::from_utf8_lossy(&self.buf));
            self.err = Some(self.syntax_error(&msg));
            return None;
        }
        Some(String::from_utf8_lossy(&self.buf).into_owned())
    }
    fn nsname(&mut self) -> Option<Name> {
        let s = self.name()?;
        if s.matches(':').count() > 1 {
            return None;
        }
        match s.split_once(':') {
            Some((space, local)) if !space.is_empty() && !local.is_empty() => Some(Name {
                space: space.to_string(),
                local: local.to_string(),
            }),
            _ => Some(Name {
                space: String::new(),
                local: s,
            }),
        }
    }
    fn text(&mut self, quote: i32, cdata: bool) -> Option<Vec<u8>> {
        let (mut b0, mut b1) = (0u8, 0u8);
        let mut trunc = 0;
        self.buf.clear();
        loop {
            let Some(b) = self.getc() else {
                if cdata {
                    if self.err == Some(XmlError::Eof) {
                        self.err = Some(self.syntax_error("unexpected EOF in CDATA section"));
                    }
                    return None;
                }
                break;
            };
            if quote < 0 && b0 == b']' && b1 == b']' && b == b'>' {
                if cdata {
                    trunc = 2;
                    break;
                }
                self.err = Some(self.syntax_error("unescaped ]]> not in CDATA section"));
                return None;
            }
            if b == b'<' && !cdata {
                if quote >= 0 {
                    self.err = Some(self.syntax_error("unescaped < inside quoted string"));
                    return None;
                }
                self.ungetc(b'<');
                break;
            }
            if quote >= 0 && i32::from(b) == quote {
                break;
            }
            if b == b'&' && !cdata {
                let before = self.buf.len();
                self.buf.push(b'&');
                let mut text: Option<String> = None;
                let mut b = self.mustgetc()?;
                if b == b'#' {
                    self.buf.push(b);
                    b = self.mustgetc()?;
                    let mut base = 10;
                    if b == b'x' {
                        base = 16;
                        self.buf.push(b);
                        b = self.mustgetc()?;
                    }
                    let start = self.buf.len();
                    while b.is_ascii_digit() || base == 16 && b.is_ascii_hexdigit() {
                        self.buf.push(b);
                        b = self.mustgetc()?;
                    }
                    if b != b';' {
                        self.ungetc(b);
                    } else {
                        let s = String::from_utf8_lossy(self.buf.get(start..).unwrap_or_default())
                            .into_owned();
                        self.buf.push(b';');
                        if let Ok(n) = u64::from_str_radix(&s, base)
                            && n <= 0x10FFFF
                        {
                            let c = char::from_u32(n as u32).unwrap_or('\u{FFFD}');
                            text = Some(c.to_string());
                        }
                    }
                } else {
                    self.ungetc(b);
                    if !self.read_name() && self.err.is_some() {
                        return None;
                    }
                    let b = self.mustgetc()?;
                    if b != b';' {
                        self.ungetc(b);
                    } else {
                        let name = self
                            .buf
                            .get(before.saturating_add(1)..)
                            .unwrap_or_default()
                            .to_vec();
                        self.buf.push(b';');
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
                if let Some(t) = text {
                    self.buf.truncate(before);
                    self.buf.extend_from_slice(t.as_bytes());
                    b0 = 0;
                    b1 = 0;
                    continue;
                }
                let mut ent = String::from_utf8_lossy(self.buf.get(before..).unwrap_or_default())
                    .into_owned();
                if !ent.ends_with(';') {
                    ent.push_str(" (no semicolon)");
                }
                self.err = Some(self.syntax_error(&format!("invalid character entity {ent}")));
                return None;
            }
            if b == b'\r' {
                self.buf.push(b'\n');
            } else if b1 == b'\r' && b == b'\n' {
            } else {
                self.buf.push(b);
            }
            b0 = b1;
            b1 = b;
        }
        let data = self
            .buf
            .get(..self.buf.len().saturating_sub(trunc))
            .unwrap_or_default()
            .to_vec();
        let mut i = 0usize;
        while let Some(rest) = data.get(i..)
            && !rest.is_empty()
        {
            let (r, size) = crate::textfmt::decode_rune(rest);
            let Some(r) = r else {
                self.err = Some(self.syntax_error("invalid UTF-8"));
                return None;
            };
            i = i.saturating_add(size);
            if !is_in_character_range(r as u32) {
                let msg = format!("illegal character code {}", u_plus(r as u32));
                self.err = Some(self.syntax_error(&msg));
                return None;
            }
        }
        Some(data)
    }
}
struct BufWriter<'w> {
    buf: Vec<u8>,
    out: Vec<u8>,
    sink: Option<&'w mut dyn std::io::Write>,
    remaining: i64,
    err: Option<String>,
    limit_error: String,
}
const BUF_SIZE: usize = 4096;
impl BufWriter<'_> {
    fn lw_write(&mut self, p: &[u8]) -> Result<(), String> {
        if p.len() as i64 > self.remaining {
            return Err(self.limit_error.clone());
        }
        if let Some(sink) = self.sink.as_mut() {
            let n = sink.write(p).map_err(|e| e.to_string())?;
            self.remaining = self.remaining.saturating_sub(n as i64);
            if n != p.len() {
                return Err("short write".to_string());
            }
            return Ok(());
        }
        self.out.extend_from_slice(p);
        self.remaining = self.remaining.saturating_sub(p.len() as i64);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        let buf = std::mem::take(&mut self.buf);
        match self.lw_write(&buf) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.buf = buf;
                self.err = Some(e.clone());
                Err(e)
            }
        }
    }
    fn write(&mut self, mut p: &[u8]) {
        while p.len() > BUF_SIZE.saturating_sub(self.buf.len()) && self.err.is_none() {
            if self.buf.is_empty() {
                if let Err(e) = self.lw_write(p) {
                    self.err = Some(e);
                } else {
                    p = &[];
                }
            } else {
                let n = BUF_SIZE.saturating_sub(self.buf.len());
                let (head, tail) = p.split_at_checked(n).unwrap_or((p, &[]));
                self.buf.extend_from_slice(head);
                p = tail;
                let _ = self.flush();
            }
        }
        if self.err.is_some() {
            return;
        }
        self.buf.extend_from_slice(p);
    }
}
pub struct Printer<'w> {
    w: BufWriter<'w>,
    pretty: bool,
    depth: usize,
    indented_in: bool,
    put_newline: bool,
}
fn escape_of(r: Option<char>, newline: bool) -> Option<&'static str> {
    match r {
        None => Some("\u{FFFD}"),
        Some('"') => Some("&#34;"),
        Some('\'') => Some("&#39;"),
        Some('&') => Some("&amp;"),
        Some('<') => Some("&lt;"),
        Some('>') => Some("&gt;"),
        Some('\t') => Some("&#x9;"),
        Some('\n') => {
            if newline {
                Some("&#xA;")
            } else {
                None
            }
        }
        Some('\r') => Some("&#xD;"),
        Some(c) if !is_in_character_range(c as u32) => Some("\u{FFFD}"),
        _ => None,
    }
}
impl<'w> Printer<'w> {
    pub fn new(pretty: bool, remaining: i64, limit_error: String) -> Printer<'w> {
        Printer {
            w: BufWriter {
                buf: Vec::with_capacity(BUF_SIZE),
                out: Vec::new(),
                sink: None,
                remaining,
                err: None,
                limit_error,
            },
            pretty,
            depth: 0,
            indented_in: false,
            put_newline: false,
        }
    }
    pub fn with_sink(
        pretty: bool,
        remaining: i64,
        limit_error: String,
        sink: &'w mut dyn std::io::Write,
    ) -> Printer<'w> {
        Printer {
            w: BufWriter {
                buf: Vec::with_capacity(BUF_SIZE),
                out: Vec::new(),
                sink: Some(sink),
                remaining,
                err: None,
                limit_error,
            },
            pretty,
            depth: 0,
            indented_in: false,
            put_newline: false,
        }
    }
    pub fn write_direct(&mut self, p: &[u8]) -> Result<(), String> {
        self.w.lw_write(p)
    }
    fn cached(&self) -> Result<(), String> {
        match &self.w.err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn write_indent(&mut self, delta: i32) {
        if !self.pretty {
            return;
        }
        if delta < 0 {
            self.depth = self.depth.saturating_sub(1);
            if self.indented_in {
                self.indented_in = false;
                return;
            }
            self.indented_in = false;
        }
        if self.put_newline {
            self.w.write(b"\n");
        } else {
            self.put_newline = true;
        }
        for _ in 0..self.depth {
            self.w.write(b"  ");
        }
        if delta > 0 {
            self.depth = self.depth.saturating_add(1);
            self.indented_in = true;
        }
    }
    fn escape(&mut self, s: &[u8], newline: bool) {
        let mut last = 0;
        let mut i = 0usize;
        while let Some(rest) = s.get(i..)
            && !rest.is_empty()
        {
            let (r, width) = crate::textfmt::decode_rune(rest);
            let width = width.max(1);
            i = i.saturating_add(width);
            let r = match r {
                Some('\u{FFFD}') if width == 1 => None,
                other => other,
            };
            if let Some(esc) = escape_of(r, newline) {
                self.w
                    .write(s.get(last..i.saturating_sub(width)).unwrap_or_default());
                self.w.write(esc.as_bytes());
                last = i;
            }
        }
        self.w.write(s.get(last..).unwrap_or_default());
    }
    pub fn start(&mut self, local: &str, attrs: &[(&str, &str, &str)]) -> Result<(), String> {
        self.write_indent(1);
        self.w.write(b"<");
        self.w.write(local.as_bytes());
        for (prefix, name, value) in attrs {
            self.w.write(b" ");
            if !prefix.is_empty() {
                self.w.write(prefix.as_bytes());
                self.w.write(b":");
            }
            self.w.write(name.as_bytes());
            self.w.write(b"=\"");
            self.escape(value.as_bytes(), true);
            self.w.write(b"\"");
        }
        self.w.write(b">");
        self.cached()
    }
    pub fn end(&mut self, local: &str) -> Result<(), String> {
        self.write_indent(-1);
        self.w.write(b"</");
        self.w.write(local.as_bytes());
        self.w.write(b">");
        self.cached()
    }
    pub fn chardata(&mut self, s: &[u8]) -> Result<(), String> {
        self.escape(s, false);
        self.cached()
    }
    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        self.w.flush()?;
        Ok(self.w.out)
    }
}
