#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::fmt::Write as _;
pub mod stream;
#[derive(Clone, Debug, PartialEq)]
pub enum JVal {
    Null,
    Bool(bool),
    Str(String),
    F64(f64),
    I64(i64),
    U64(u64),
    Obj(Vec<(String, JVal)>),
    Arr(Vec<JVal>),
}
const HEX: &[u8; 16] = b"0123456789abcdef";
pub fn append_string(dst: &mut String, s: &str) {
    dst.push('"');
    for c in s.chars() {
        match c {
            '\\' => dst.push_str("\\\\"),
            '"' => dst.push_str("\\\""),
            '\u{8}' => dst.push_str("\\b"),
            '\u{c}' => dst.push_str("\\f"),
            '\n' => dst.push_str("\\n"),
            '\r' => dst.push_str("\\r"),
            '\t' => dst.push_str("\\t"),
            '<' | '>' | '&' => push_u00(dst, c as u8),
            c if (c as u32) < 0x20 => push_u00(dst, c as u8),
            '\u{2028}' => dst.push_str("\\u2028"),
            '\u{2029}' => dst.push_str("\\u2029"),
            c => dst.push(c),
        }
    }
    dst.push('"');
}
fn push_u00(dst: &mut String, b: u8) {
    dst.push_str("\\u00");
    for nibble in [b >> 4, b & 15] {
        dst.push(char::from(
            HEX.get(usize::from(nibble)).copied().unwrap_or(b'0'),
        ));
    }
}
pub fn format_float(f: f64) -> String {
    let abs = f.abs();
    if abs != 0.0 && !(1e-6..1e21).contains(&abs) {
        exp_text(f)
    } else {
        format!("{f}")
    }
}
fn exp_text(f: f64) -> String {
    let s = format!("{f:e}");
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < 0 {
        format!("{mant}e-{}", exp.unsigned_abs())
    } else {
        format!("{mant}e+{exp:02}")
    }
}
pub fn write_compact(v: &JVal, out: &mut String) {
    match v {
        JVal::Null => out.push_str("null"),
        JVal::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JVal::Str(s) => append_string(out, s),
        JVal::F64(f) => out.push_str(&format_float(*f)),
        JVal::I64(n) => {
            let _ = write!(out, "{n}");
        }
        JVal::U64(n) => {
            let _ = write!(out, "{n}");
        }
        JVal::Obj(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                append_string(out, k);
                out.push(':');
                write_compact(v, out);
            }
            out.push('}');
        }
        JVal::Arr(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(v, out);
            }
            out.push(']');
        }
    }
}
fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}
pub fn write_pretty(v: &JVal, out: &mut String, depth: usize) {
    match v {
        JVal::Obj(entries) if !entries.is_empty() => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth.saturating_add(1));
                append_string(out, k);
                out.push_str(": ");
                write_pretty(v, out, depth.saturating_add(1));
            }
            newline(out, depth);
            out.push('}');
        }
        JVal::Arr(items) if !items.is_empty() => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth.saturating_add(1));
                write_pretty(v, out, depth.saturating_add(1));
            }
            newline(out, depth);
            out.push(']');
        }
        other => write_compact(other, out),
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonError {
    Syntax(String),
    Eof,
    UnexpectedEof,
    Other(String),
}
impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonError::Syntax(m) | JsonError::Other(m) => f.write_str(m),
            JsonError::Eof => f.write_str("EOF"),
            JsonError::UnexpectedEof => f.write_str("unexpected EOF"),
        }
    }
}
impl std::error::Error for JsonError {}
fn quote_char(c: u8) -> String {
    match c {
        b'\'' => "'\\''".to_string(),
        b'"' => "'\"'".to_string(),
        _ => {
            let q = crate::textfmt::quote(&char::from(c).to_string());
            format!(
                "'{}'",
                q.get(1..q.len().saturating_sub(1)).unwrap_or_default()
            )
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Continue,
    BeginLiteral,
    BeginObject,
    ObjectKey,
    ObjectValue,
    EndObject,
    BeginArray,
    ArrayValue,
    EndArray,
    SkipSpace,
    End,
    Error,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum St {
    BeginValue,
    BeginValueOrEmpty,
    BeginStringOrEmpty,
    BeginString,
    EndValue,
    EndTop,
    InString,
    InStringEsc,
    EscU(u8),
    Neg,
    One,
    Zero,
    Dot,
    Dot0,
    E,
    ESign,
    E0,
    Lit(&'static [u8], u8),
    Error,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parse {
    ObjectKey,
    ObjectValue,
    ArrayValue,
}
const MAX_NESTING_DEPTH: usize = 10000;
struct Scanner {
    step: St,
    end_top: bool,
    parse: Vec<Parse>,
    err: Option<String>,
}
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}
impl Scanner {
    fn new() -> Scanner {
        Scanner {
            step: St::BeginValue,
            end_top: false,
            parse: Vec::new(),
            err: None,
        }
    }
    fn error(&mut self, c: u8, context: &str) -> Op {
        self.step = St::Error;
        self.err = Some(format!("invalid character {} {context}", quote_char(c)));
        Op::Error
    }
    fn push(&mut self, c: u8, p: Parse, ok: Op) -> Op {
        self.parse.push(p);
        if self.parse.len() <= MAX_NESTING_DEPTH {
            ok
        } else {
            self.error(c, "exceeded max depth")
        }
    }
    fn pop(&mut self) {
        self.parse.pop();
        if self.parse.is_empty() {
            self.step = St::EndTop;
            self.end_top = true;
        } else {
            self.step = St::EndValue;
        }
    }
    fn step(&mut self, c: u8) -> Op {
        match self.step {
            St::BeginValueOrEmpty => {
                if is_space(c) {
                    return Op::SkipSpace;
                }
                if c == b']' {
                    return self.end_value(c);
                }
                self.begin_value(c)
            }
            St::BeginValue => self.begin_value(c),
            St::BeginStringOrEmpty => {
                if is_space(c) {
                    return Op::SkipSpace;
                }
                if c == b'}' {
                    if let Some(last) = self.parse.last_mut() {
                        *last = Parse::ObjectValue;
                    }
                    return self.end_value(c);
                }
                self.begin_string(c)
            }
            St::BeginString => self.begin_string(c),
            St::EndValue => self.end_value(c),
            St::EndTop => self.end_top(c),
            St::InString => {
                if c == b'"' {
                    self.step = St::EndValue;
                    Op::Continue
                } else if c == b'\\' {
                    self.step = St::InStringEsc;
                    Op::Continue
                } else if c < 0x20 {
                    self.error(c, "in string literal")
                } else {
                    Op::Continue
                }
            }
            St::InStringEsc => match c {
                b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => {
                    self.step = St::InString;
                    Op::Continue
                }
                b'u' => {
                    self.step = St::EscU(0);
                    Op::Continue
                }
                _ => self.error(c, "in string escape code"),
            },
            St::EscU(n) => {
                if c.is_ascii_hexdigit() {
                    self.step = if n == 3 {
                        St::InString
                    } else {
                        St::EscU(n.saturating_add(1))
                    };
                    Op::Continue
                } else {
                    self.error(c, "in \\u hexadecimal character escape")
                }
            }
            St::Neg => {
                if c == b'0' {
                    self.step = St::Zero;
                    Op::Continue
                } else if (b'1'..=b'9').contains(&c) {
                    self.step = St::One;
                    Op::Continue
                } else {
                    self.error(c, "in numeric literal")
                }
            }
            St::One => {
                if c.is_ascii_digit() {
                    Op::Continue
                } else {
                    self.zero(c)
                }
            }
            St::Zero => self.zero(c),
            St::Dot => {
                if c.is_ascii_digit() {
                    self.step = St::Dot0;
                    Op::Continue
                } else {
                    self.error(c, "after decimal point in numeric literal")
                }
            }
            St::Dot0 => {
                if c.is_ascii_digit() {
                    Op::Continue
                } else if c == b'e' || c == b'E' {
                    self.step = St::E;
                    Op::Continue
                } else {
                    self.end_value(c)
                }
            }
            St::E => {
                if c == b'+' || c == b'-' {
                    self.step = St::ESign;
                    Op::Continue
                } else {
                    self.e_sign(c)
                }
            }
            St::ESign => self.e_sign(c),
            St::E0 => {
                if c.is_ascii_digit() {
                    Op::Continue
                } else {
                    self.end_value(c)
                }
            }
            St::Lit(word, i) => {
                let want = word.get(usize::from(i)).copied().unwrap_or_default();
                if c == want {
                    if usize::from(i).saturating_add(1) >= word.len() {
                        self.step = St::EndValue;
                    } else {
                        self.step = St::Lit(word, i.saturating_add(1));
                    }
                    Op::Continue
                } else {
                    let name = std::str::from_utf8(word).unwrap_or("");
                    let ctx = format!("in literal {name} (expecting '{}')", want as char);
                    self.error(c, &ctx)
                }
            }
            St::Error => Op::Error,
        }
    }
    fn zero(&mut self, c: u8) -> Op {
        if c == b'.' {
            self.step = St::Dot;
            Op::Continue
        } else if c == b'e' || c == b'E' {
            self.step = St::E;
            Op::Continue
        } else {
            self.end_value(c)
        }
    }
    fn e_sign(&mut self, c: u8) -> Op {
        if c.is_ascii_digit() {
            self.step = St::E0;
            Op::Continue
        } else {
            self.error(c, "in exponent of numeric literal")
        }
    }
    fn begin_value(&mut self, c: u8) -> Op {
        if is_space(c) {
            return Op::SkipSpace;
        }
        match c {
            b'{' => {
                self.step = St::BeginStringOrEmpty;
                self.push(c, Parse::ObjectKey, Op::BeginObject)
            }
            b'[' => {
                self.step = St::BeginValueOrEmpty;
                self.push(c, Parse::ArrayValue, Op::BeginArray)
            }
            b'"' => {
                self.step = St::InString;
                Op::BeginLiteral
            }
            b'-' => {
                self.step = St::Neg;
                Op::BeginLiteral
            }
            b'0' => {
                self.step = St::Zero;
                Op::BeginLiteral
            }
            b't' => {
                self.step = St::Lit(b"true", 1);
                Op::BeginLiteral
            }
            b'f' => {
                self.step = St::Lit(b"false", 1);
                Op::BeginLiteral
            }
            b'n' => {
                self.step = St::Lit(b"null", 1);
                Op::BeginLiteral
            }
            b'1'..=b'9' => {
                self.step = St::One;
                Op::BeginLiteral
            }
            _ => self.error(c, "looking for beginning of value"),
        }
    }
    fn begin_string(&mut self, c: u8) -> Op {
        if is_space(c) {
            return Op::SkipSpace;
        }
        if c == b'"' {
            self.step = St::InString;
            return Op::BeginLiteral;
        }
        self.error(c, "looking for beginning of object key string")
    }
    fn end_value(&mut self, c: u8) -> Op {
        let Some(&ps) = self.parse.last() else {
            self.step = St::EndTop;
            self.end_top = true;
            return self.end_top(c);
        };
        if is_space(c) {
            self.step = St::EndValue;
            return Op::SkipSpace;
        }
        match ps {
            Parse::ObjectKey => {
                if c == b':' {
                    if let Some(top) = self.parse.last_mut() {
                        *top = Parse::ObjectValue;
                    }
                    self.step = St::BeginValue;
                    return Op::ObjectKey;
                }
                self.error(c, "after object key")
            }
            Parse::ObjectValue => {
                if c == b',' {
                    if let Some(top) = self.parse.last_mut() {
                        *top = Parse::ObjectKey;
                    }
                    self.step = St::BeginString;
                    return Op::ObjectValue;
                }
                if c == b'}' {
                    self.pop();
                    return Op::EndObject;
                }
                self.error(c, "after object key:value pair")
            }
            Parse::ArrayValue => {
                if c == b',' {
                    self.step = St::BeginValue;
                    return Op::ArrayValue;
                }
                if c == b']' {
                    self.pop();
                    return Op::EndArray;
                }
                self.error(c, "after array element")
            }
        }
    }
    fn end_top(&mut self, c: u8) -> Op {
        if !is_space(c) {
            self.error(c, "after top-level value");
        }
        Op::End
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokState {
    TopValue,
    ArrayStart,
    ArrayValue,
    ArrayComma,
    ObjectStart,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Delim(u8),
    Str(String),
    Scalar(Vec<u8>),
}
pub struct Decoder<'a> {
    buf: &'a [u8],
    scanp: usize,
    state: TokState,
    stack: Vec<TokState>,
    err: Option<JsonError>,
    tail: Option<JsonError>,
    use_number: bool,
}
impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Decoder<'a> {
        Decoder {
            buf,
            scanp: 0,
            state: TokState::TopValue,
            stack: Vec::new(),
            err: None,
            tail: None,
            use_number: false,
        }
    }
    pub fn with_tail(buf: &'a [u8], tail: Option<JsonError>, use_number: bool) -> Decoder<'a> {
        Decoder {
            tail,
            use_number,
            ..Decoder::new(buf)
        }
    }
    fn end_error(&self) -> JsonError {
        self.tail.clone().unwrap_or(JsonError::Eof)
    }
    pub fn input_offset(&self) -> usize {
        self.scanp
    }
    fn peek(&mut self) -> Result<u8, JsonError> {
        let rest = self.buf.get(self.scanp..).unwrap_or_default();
        if let Some(skip) = rest.iter().position(|&c| !is_space(c))
            && let Some(&c) = rest.get(skip)
        {
            self.scanp = self.scanp.saturating_add(skip);
            return Ok(c);
        }
        Err(self.end_error())
    }
    fn read_value(&mut self) -> Result<usize, JsonError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let mut scan = Scanner::new();
        let rest = self.buf.get(self.scanp..).unwrap_or_default();
        for (i, &c) in rest.iter().enumerate() {
            match scan.step(c) {
                Op::End => return Ok(i),
                Op::EndObject | Op::EndArray => {
                    if scan.end_value(b' ') == Op::End {
                        return Ok(i.saturating_add(1));
                    }
                }
                Op::Error => {
                    let e = JsonError::Syntax(scan.err.unwrap_or_default());
                    self.err = Some(e.clone());
                    return Err(e);
                }
                _ => {}
            }
        }
        if let Some(t) = &self.tail {
            let e = t.clone();
            self.err = Some(e.clone());
            return Err(e);
        }
        if scan.step(b' ') == Op::End {
            return Ok(rest.len());
        }
        let e = if rest.iter().any(|&c| !is_space(c)) {
            JsonError::UnexpectedEof
        } else {
            JsonError::Eof
        };
        self.err = Some(e.clone());
        Err(e)
    }
    fn prepare_for_decode(&mut self) -> Result<(), JsonError> {
        match self.state {
            TokState::ArrayComma => {
                if self.peek()? != b',' {
                    return Err(JsonError::Syntax(
                        "expected comma after array element".into(),
                    ));
                }
                self.scanp = self.scanp.saturating_add(1);
                self.state = TokState::ArrayValue;
            }
            TokState::ObjectColon => {
                if self.peek()? != b':' {
                    return Err(JsonError::Syntax("expected colon after object key".into()));
                }
                self.scanp = self.scanp.saturating_add(1);
                self.state = TokState::ObjectValue;
            }
            _ => {}
        }
        Ok(())
    }
    fn value_allowed(&self) -> bool {
        matches!(
            self.state,
            TokState::TopValue
                | TokState::ArrayStart
                | TokState::ArrayValue
                | TokState::ObjectValue
        )
    }
    fn value_end(&mut self) {
        match self.state {
            TokState::ArrayStart | TokState::ArrayValue => self.state = TokState::ArrayComma,
            TokState::ObjectValue => self.state = TokState::ObjectComma,
            _ => {}
        }
    }
    pub fn decode_raw(&mut self) -> Result<&'a [u8], JsonError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        self.prepare_for_decode()?;
        if !self.value_allowed() {
            return Err(JsonError::Syntax("not at beginning of value".into()));
        }
        let n = self.read_value()?;
        let end = self.scanp.saturating_add(n);
        let raw = self.buf.get(self.scanp..end).unwrap_or_default();
        self.scanp = end;
        self.value_end();
        let start = raw.iter().position(|&c| !is_space(c)).unwrap_or(raw.len());
        Ok(raw.get(start..).unwrap_or_default())
    }
    fn token_error(&self, c: u8) -> JsonError {
        let context = match self.state {
            TokState::TopValue
            | TokState::ArrayStart
            | TokState::ArrayValue
            | TokState::ObjectValue => " looking for beginning of value",
            TokState::ArrayComma => " after array element",
            TokState::ObjectKey => " looking for beginning of object key string",
            TokState::ObjectColon => " after object key",
            TokState::ObjectComma => " after object key:value pair",
            TokState::ObjectStart => "",
        };
        JsonError::Syntax(format!("invalid character {}{context}", quote_char(c)))
    }
    pub fn token(&mut self) -> Result<Token, JsonError> {
        loop {
            let c = self.peek()?;
            match c {
                b'[' | b'{' => {
                    if !self.value_allowed() {
                        return Err(self.token_error(c));
                    }
                    self.scanp = self.scanp.saturating_add(1);
                    self.stack.push(self.state);
                    self.state = if c == b'[' {
                        TokState::ArrayStart
                    } else {
                        TokState::ObjectStart
                    };
                    return Ok(Token::Delim(c));
                }
                b']' | b'}' => {
                    let ok = if c == b']' {
                        matches!(self.state, TokState::ArrayStart | TokState::ArrayComma)
                    } else {
                        matches!(self.state, TokState::ObjectStart | TokState::ObjectComma)
                    };
                    if !ok {
                        return Err(self.token_error(c));
                    }
                    self.scanp = self.scanp.saturating_add(1);
                    self.state = self.stack.pop().unwrap_or(TokState::TopValue);
                    self.value_end();
                    return Ok(Token::Delim(c));
                }
                b':' => {
                    if self.state != TokState::ObjectColon {
                        return Err(self.token_error(c));
                    }
                    self.scanp = self.scanp.saturating_add(1);
                    self.state = TokState::ObjectValue;
                }
                b',' => {
                    if self.state == TokState::ArrayComma {
                        self.scanp = self.scanp.saturating_add(1);
                        self.state = TokState::ArrayValue;
                    } else if self.state == TokState::ObjectComma {
                        self.scanp = self.scanp.saturating_add(1);
                        self.state = TokState::ObjectKey;
                    } else {
                        return Err(self.token_error(c));
                    }
                }
                _ => {
                    if c == b'"'
                        && matches!(self.state, TokState::ObjectStart | TokState::ObjectKey)
                    {
                        let old = self.state;
                        self.state = TokState::TopValue;
                        let raw = self.decode_raw();
                        self.state = old;
                        let raw = raw?;
                        self.state = TokState::ObjectColon;
                        return Ok(Token::Str(unquote(raw)));
                    }
                    if !self.value_allowed() {
                        return Err(self.token_error(c));
                    }
                    let raw = self.decode_raw()?;
                    if raw.first() == Some(&b'"') {
                        return Ok(Token::Str(unquote(raw)));
                    }
                    if !self.use_number && matches!(raw.first(), Some(b'-' | b'0'..=b'9')) {
                        let s = String::from_utf8_lossy(raw);
                        if crate::numtext::parse_float(&s).is_err() {
                            return Err(JsonError::Other(format!(
                                "json: cannot unmarshal number {s} into value of type float64"
                            )));
                        }
                    }
                    return Ok(Token::Scalar(raw.to_vec()));
                }
            }
        }
    }
    pub fn more(&mut self) -> bool {
        matches!(self.peek(), Ok(c) if c != b']' && c != b'}')
    }
}
pub fn ensure_eof(dec: &mut Decoder) -> Result<(), JsonError> {
    match dec.token() {
        Err(JsonError::Eof) => Ok(()),
        Ok(_) => Err(JsonError::Other("unexpected trailing data".into())),
        Err(e) => Err(e),
    }
}
fn getu4(s: &[u8]) -> Option<u32> {
    let [b'\\', b'u', hex @ ..] = s else {
        return None;
    };
    let h = std::str::from_utf8(hex.get(..4)?).ok()?;
    u32::from_str_radix(h, 16).ok()
}
pub fn unquote(lit: &[u8]) -> String {
    let s = lit
        .get(1..lit.len().saturating_sub(1).max(1))
        .unwrap_or_default();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut r = 0usize;
    while let Some(&c) = s.get(r) {
        if c == b'\\'
            && let Some(&e) = s.get(r.saturating_add(1))
        {
            let r2 = r.saturating_add(2);
            match e {
                b'"' | b'\\' | b'/' | b'\'' => {
                    out.push(e);
                    r = r2;
                }
                b'b' => {
                    out.push(8);
                    r = r2;
                }
                b'f' => {
                    out.push(12);
                    r = r2;
                }
                b'n' => {
                    out.push(b'\n');
                    r = r2;
                }
                b'r' => {
                    out.push(b'\r');
                    r = r2;
                }
                b't' => {
                    out.push(b'\t');
                    r = r2;
                }
                b'u' => {
                    let Some(rr) = getu4(s.get(r..).unwrap_or_default()) else {
                        break;
                    };
                    r = r.saturating_add(6);
                    let mut ch = char::from_u32(rr);
                    if (0xD800..0xE000).contains(&rr) {
                        ch = None;
                        if let Some(rr1) = getu4(s.get(r..).unwrap_or_default())
                            && (0xD800..0xDC00).contains(&rr)
                            && (0xDC00..0xE000).contains(&rr1)
                        {
                            let pair = (rr.wrapping_sub(0xD800) << 10)
                                .wrapping_add(rr1.wrapping_sub(0xDC00))
                                .wrapping_add(0x10000);
                            ch = char::from_u32(pair);
                            r = r.saturating_add(6);
                        }
                    }
                    let ch = ch.unwrap_or('\u{FFFD}');
                    let mut b = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                _ => {
                    out.push(c);
                    r = r.saturating_add(1);
                }
            }
        } else {
            out.push(c);
            r = r.saturating_add(1);
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}
pub fn kind(raw: &[u8]) -> &'static str {
    match raw.first() {
        Some(b'"') => "string",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        Some(b'{') => "object",
        Some(b'[') => "array",
        _ => "number",
    }
}
fn type_error(value: &str, typ: &str) -> JsonError {
    JsonError::Other(format!(
        "json: cannot unmarshal {value} into value of type {typ}"
    ))
}
fn members(raw: &[u8]) -> Vec<(String, &[u8])> {
    let mut out = Vec::new();
    let mut i = 1;
    loop {
        i = skip_ws(raw, i);
        if raw.get(i).is_none_or(|&c| c == b'}') {
            return out;
        }
        let kend = value_end(raw, i);
        let key = unquote(raw.get(i..kend).unwrap_or_default());
        i = skip_ws(raw, kend).saturating_add(1);
        i = skip_ws(raw, i);
        let vend = value_end(raw, i);
        out.push((key, raw.get(i..vend).unwrap_or_default()));
        i = skip_ws(raw, vend);
        if raw.get(i) == Some(&b',') {
            i = i.saturating_add(1);
        }
    }
}
fn elements(raw: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 1;
    loop {
        i = skip_ws(raw, i);
        if raw.get(i).is_none_or(|&c| c == b']') {
            return out;
        }
        let end = value_end(raw, i);
        out.push(raw.get(i..end).unwrap_or_default());
        i = skip_ws(raw, end);
        if raw.get(i) == Some(&b',') {
            i = i.saturating_add(1);
        }
    }
}
fn skip_ws(raw: &[u8], mut i: usize) -> usize {
    while raw.get(i).is_some_and(|&c| is_space(c)) {
        i = i.saturating_add(1);
    }
    i
}
fn value_end(raw: &[u8], i: usize) -> usize {
    let at = |j: usize| raw.get(j).copied();
    match at(i) {
        Some(b'"') => {
            let mut j = i.saturating_add(1);
            while let Some(c) = at(j) {
                match c {
                    b'\\' => j = j.saturating_add(2),
                    b'"' => return j.saturating_add(1),
                    _ => j = j.saturating_add(1),
                }
            }
            raw.len()
        }
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut j = i;
            while let Some(c) = at(j) {
                match c {
                    b'"' => {
                        j = value_end(raw, j);
                        continue;
                    }
                    b'{' | b'[' => depth = depth.saturating_add(1),
                    b'}' | b']' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return j.saturating_add(1);
                        }
                    }
                    _ => {}
                }
                j = j.saturating_add(1);
            }
            raw.len()
        }
        _ => {
            let mut j = i;
            while at(j).is_some_and(|c| !matches!(c, b',' | b'}' | b']') && !is_space(c)) {
                j = j.saturating_add(1);
            }
            j
        }
    }
}
pub fn as_raw_map(raw: &[u8]) -> Result<Vec<(String, &[u8])>, JsonError> {
    match raw.first() {
        Some(b'{') => {
            let mut out: Vec<(String, &[u8])> = Vec::new();
            for (k, v) in members(raw) {
                if let Some(slot) = out.iter_mut().find(|(key, _)| *key == k) {
                    slot.1 = v;
                } else {
                    out.push((k, v));
                }
            }
            Ok(out)
        }
        Some(b'n') => Ok(Vec::new()),
        _ => Err(type_error(kind(raw), "JSON object")),
    }
}
pub fn as_raw_array(raw: &[u8]) -> Result<Vec<&[u8]>, JsonError> {
    match raw.first() {
        Some(b'[') => Ok(elements(raw)),
        Some(b'n') => Ok(Vec::new()),
        _ => Err(type_error(kind(raw), "JSON array")),
    }
}
pub fn as_string(raw: &[u8]) -> Result<String, JsonError> {
    match raw.first() {
        Some(b'"') => Ok(unquote(raw)),
        Some(b'n') => Ok(String::new()),
        _ => Err(type_error(kind(raw), "string")),
    }
}
fn fold_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{17F}' => 'S',
            '\u{212A}' => 'K',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}
pub fn as_string_struct(
    raw: &[u8],
    struct_name: &str,
    type_name: &str,
    fields: &[&str],
) -> Result<Vec<String>, JsonError> {
    let mut out = vec![String::new(); fields.len()];
    match raw.first() {
        Some(b'{') => {}
        Some(b'n') => return Ok(out),
        _ => return Err(type_error(kind(raw), type_name)),
    }
    let mut saved: Option<JsonError> = None;
    for (key, value) in members(raw) {
        let index = fields.iter().position(|f| *f == key).or_else(|| {
            let folded = fold_name(&key);
            fields.iter().position(|f| fold_name(f) == folded)
        });
        let Some(index) = index else {
            continue;
        };
        match value.first() {
            Some(b'"') => {
                if let Some(slot) = out.get_mut(index) {
                    *slot = unquote(value);
                }
            }
            Some(b'n') => {}
            _ => {
                if saved.is_none() {
                    saved = Some(JsonError::Other(format!(
                        "json: cannot unmarshal {} into struct field {struct_name}.{} of type string",
                        kind(value),
                        fields.get(index).copied().unwrap_or_default()
                    )));
                }
            }
        }
    }
    match saved {
        Some(e) => Err(e),
        None => Ok(out),
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum JAny {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Arr(Vec<JAny>),
    Obj(Vec<(String, JAny)>),
}
pub fn parse_any(raw: &[u8]) -> JAny {
    match raw.first() {
        Some(b'"') => JAny::Str(unquote(raw)),
        Some(b't') => JAny::Bool(true),
        Some(b'f') => JAny::Bool(false),
        Some(b'n') | None => JAny::Null,
        Some(b'[') => JAny::Arr(elements(raw).into_iter().map(parse_any).collect()),
        Some(b'{') => {
            let mut out: Vec<(String, JAny)> = Vec::new();
            for (k, v) in members(raw) {
                let v = parse_any(v);
                if let Some(slot) = out.iter_mut().find(|(key, _)| *key == k) {
                    slot.1 = v;
                } else {
                    out.push((k, v));
                }
            }
            out.sort_by(|a, b| a.0.cmp(&b.0));
            JAny::Obj(out)
        }
        _ => JAny::Number(String::from_utf8_lossy(raw).into_owned()),
    }
}
pub fn sprint_text(v: &JAny) -> String {
    match v {
        JAny::Null => "<nil>".into(),
        JAny::Bool(b) => b.to_string(),
        JAny::Number(n) | JAny::Str(n) => n.clone(),
        JAny::Arr(items) => format!(
            "[{}]",
            items.iter().map(sprint_text).collect::<Vec<_>>().join(" ")
        ),
        JAny::Obj(entries) => format!(
            "map[{}]",
            entries
                .iter()
                .map(|(k, v)| format!("{k}:{}", sprint_text(v)))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawElement {
    pub vr: String,
    pub value: Vec<JAny>,
    pub inline_binary: String,
    pub bulk_data_uri: String,
}
pub fn as_element(raw: &[u8]) -> Result<RawElement, JsonError> {
    let mut out = RawElement::default();
    match raw.first() {
        Some(b'{') => {}
        Some(b'n') => return Ok(out),
        _ => return Err(type_error(kind(raw), "Element")),
    }
    let fields = ["vr", "Value", "InlineBinary", "BulkDataURI"];
    let mut saved: Option<JsonError> = None;
    for (key, value) in members(raw) {
        let index = fields.iter().position(|f| *f == key).or_else(|| {
            let folded = fold_name(&key);
            fields.iter().position(|f| fold_name(f) == folded)
        });
        let Some(index) = index else {
            continue;
        };
        let field_error = |typ: &str| {
            JsonError::Other(format!(
                "json: cannot unmarshal {} into struct field Element.{} of type {typ}",
                kind(value),
                fields.get(index).copied().unwrap_or_default()
            ))
        };
        if index == 1 {
            match value.first() {
                Some(b'[') => out.value = elements(value).into_iter().map(parse_any).collect(),
                Some(b'n') => out.value = Vec::new(),
                _ => {
                    saved.get_or_insert_with(|| field_error("[]interface {}"));
                }
            }
            continue;
        }
        match value.first() {
            Some(b'"') => {
                let s = unquote(value);
                match index {
                    0 => out.vr = s,
                    2 => out.inline_binary = s,
                    _ => out.bulk_data_uri = s,
                }
            }
            Some(b'n') => {}
            _ => {
                saved.get_or_insert_with(|| field_error("string"));
            }
        }
    }
    match saved {
        Some(e) => Err(e),
        None => Ok(out),
    }
}
