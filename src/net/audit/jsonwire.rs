#![forbid(unsafe_code)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    String(String, &'a [u8]),
    Array(Vec<Value<'a>>),
    Object(Vec<(String, Value<'a>, &'a [u8])>),
}
const MAX_DEPTH: usize = 64;
struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}
impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }
    fn literal(&mut self, text: &[u8]) -> Option<()> {
        if self.b[self.i..].starts_with(text) {
            self.i += text.len();
            Some(())
        } else {
            None
        }
    }
    fn value(&mut self, depth: usize) -> Option<Value<'a>> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match *self.b.get(self.i)? {
            b'n' => self.literal(b"null").map(|()| Value::Null),
            b't' => self.literal(b"true").map(|()| Value::Bool(true)),
            b'f' => self.literal(b"false").map(|()| Value::Bool(false)),
            b'"' => {
                let (text, raw) = self.string()?;
                Some(Value::String(text, raw))
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Some(Value::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b']' => {
                            self.i += 1;
                            return Some(Value::Array(items));
                        }
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Some(Value::Object(members));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return None;
                    }
                    let (name, _) = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return None;
                    }
                    self.i += 1;
                    self.ws();
                    let start = self.i;
                    let value = self.value(depth + 1)?;
                    members.push((name, value, &self.b[start..self.i]));
                    self.ws();
                    match self.b.get(self.i)? {
                        b',' => self.i += 1,
                        b'}' => {
                            self.i += 1;
                            return Some(Value::Object(members));
                        }
                        _ => return None,
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }
    fn number(&mut self) -> Option<Value<'a>> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.b.get(self.i)? {
            b'0' => self.i += 1,
            b'1'..=b'9' => {
                while self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            }
            _ => return None,
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
                return None;
            }
            while self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
                return None;
            }
            while self.b.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()
            .map(Value::Number)
    }
    fn hex4(&mut self) -> Option<u32> {
        let part = self.b.get(self.i..self.i + 4)?;
        let text = std::str::from_utf8(part).ok()?;
        if !text.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.i += 4;
        u32::from_str_radix(text, 16).ok()
    }
    fn string(&mut self) -> Option<(String, &'a [u8])> {
        self.i += 1;
        let start = self.i;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i)?;
            match c {
                b'"' => {
                    let raw = &self.b[start..self.i];
                    self.i += 1;
                    return Some((String::from_utf8_lossy(&out).into_owned(), raw));
                }
                b'\\' => {
                    self.i += 1;
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut r = self.hex4()?;
                            if (0xd800..0xdc00).contains(&r) {
                                let save = self.i;
                                let mut paired = false;
                                if self.b.get(self.i) == Some(&b'\\')
                                    && self.b.get(self.i + 1) == Some(&b'u')
                                {
                                    self.i += 2;
                                    let low = self.hex4()?;
                                    if (0xdc00..0xe000).contains(&low) {
                                        r = 0x10000 + ((r - 0xd800) << 10) + (low - 0xdc00);
                                        paired = true;
                                    }
                                }
                                if !paired {
                                    self.i = save;
                                    r = 0xfffd;
                                }
                            } else if (0xdc00..0xe000).contains(&r) {
                                r = 0xfffd;
                            }
                            let ch = char::from_u32(r).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return None,
                    }
                }
                c if c < 0x20 => return None,
                c => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }
}
pub(crate) fn parse(data: &[u8]) -> Option<Value<'_>> {
    let mut p = Parser { b: data, i: 0 };
    let value = p.value(0)?;
    p.ws();
    (p.i == data.len()).then_some(value)
}
pub(crate) fn field<'n>(names: &[&'n str], name: &str) -> Option<&'n str> {
    names
        .iter()
        .find(|n| **n == name)
        .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(name)))
        .copied()
}
pub(crate) fn as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(raw) if raw.bytes().all(|c| c.is_ascii_digit()) => raw.parse().ok(),
        _ => None,
    }
}
pub(crate) fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(raw)
            if raw
                .strip_prefix('-')
                .unwrap_or(raw)
                .bytes()
                .all(|c| c.is_ascii_digit()) =>
        {
            raw.parse().ok()
        }
        _ => None,
    }
}
pub(crate) fn push_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_keeps_raw() {
        let text = " {\"a\": [1, -2.5e3, \"x\\u00e9\\ud83d\\ude00\"], \"State\" : {\"k\":null} } ";
        let v = parse(text.as_bytes()).unwrap();
        let Value::Object(members) = v else { panic!() };
        assert_eq!(members[1].2, br#"{"k":null}"#);
        let Value::Array(items) = &members[0].1 else {
            panic!()
        };
        assert_eq!(as_u64(&items[0]), Some(1));
        assert_eq!(as_u64(&items[1]), None);
        assert!(matches!(&items[2], Value::String(s, _) if s == "x\u{e9}\u{1f600}"));
        assert!(parse(b"{} x").is_none());
        assert!(parse(b"01").is_none());
        assert!(parse(b"\"\x01\"").is_none());
    }
}
