#[derive(Debug, Clone)]
pub(super) struct Json {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}
#[derive(Debug, Clone)]
pub(super) enum Kind {
    Null,
    Bool,
    Number,
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}
pub(super) fn parse(s: &[u8]) -> Option<Json> {
    let mut p = Parser { s, i: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != s.len() {
        return None;
    }
    Some(v)
}
impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn lit(&mut self, word: &[u8]) -> Option<()> {
        if self.s[self.i..].starts_with(word) {
            self.i += word.len();
            Some(())
        } else {
            None
        }
    }
    fn value(&mut self) -> Option<Json> {
        let start = self.i;
        let kind = match self.peek()? {
            b'n' => {
                self.lit(b"null")?;
                Kind::Null
            }
            b't' => {
                self.lit(b"true")?;
                Kind::Bool
            }
            b'f' => {
                self.lit(b"false")?;
                Kind::Bool
            }
            b'"' => Kind::Str(self.string()?),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.peek()? == b']' {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        items.push(self.value()?);
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b']' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Kind::Array(items)
            }
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.peek()? == b'}' {
                    self.i += 1;
                } else {
                    loop {
                        self.ws();
                        if self.peek()? != b'"' {
                            return None;
                        }
                        let key = self.string()?;
                        self.ws();
                        if self.peek()? != b':' {
                            return None;
                        }
                        self.i += 1;
                        self.ws();
                        let v = self.value()?;
                        members.push((key, v));
                        self.ws();
                        match self.peek()? {
                            b',' => self.i += 1,
                            b'}' => {
                                self.i += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Kind::Object(members)
            }
            b'-' | b'0'..=b'9' => {
                self.number()?;
                Kind::Number
            }
            _ => return None,
        };
        Some(Json {
            kind,
            start,
            end: self.i,
        })
    }
    fn digits(&mut self) -> usize {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.i - start
    }
    fn number(&mut self) -> Option<()> {
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        Some(())
    }
    fn hex4(&mut self) -> Option<u32> {
        let h = self.s.get(self.i..self.i + 4)?;
        let v = u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?;
        if !h.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.i += 4;
        Some(v)
    }
    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            match c {
                b'"' => {
                    self.i += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let r = self.hex4()?;
                            if (0xD800..0xDC00).contains(&r) {
                                let save = self.i;
                                if self.s.get(self.i..self.i + 2) == Some(b"\\u")
                                    && let Some(lo) = {
                                        self.i += 2;
                                        self.hex4()
                                    }
                                    && (0xDC00..0xE000).contains(&lo)
                                {
                                    let v = 0x10000 + ((r - 0xD800) << 10) + (lo - 0xDC00);
                                    out.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                                } else {
                                    self.i = save;
                                    out.push('\u{FFFD}');
                                }
                            } else {
                                out.push(char::from_u32(r).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return None,
                    }
                }
                0..0x20 => return None,
                _ => {
                    let len = match c {
                        0x00..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let ch = std::str::from_utf8(self.s.get(self.i..self.i + len)?).ok()?;
                    out.push_str(ch);
                    self.i += len;
                }
            }
        }
    }
}
