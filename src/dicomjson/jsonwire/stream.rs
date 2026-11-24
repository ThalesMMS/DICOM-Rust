#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::{JsonError, Op, Scanner, TokState, Token, is_space, unquote};
use std::io::{ErrorKind, Read};
const READ_SIZE: usize = 4096;
pub struct StreamDecoder<R> {
    src: R,
    buf: Vec<u8>,
    scanp: usize,
    scanned: u64,
    state: TokState,
    stack: Vec<TokState>,
    err: Option<JsonError>,
    src_err: Option<JsonError>,
    use_number: bool,
    check: Option<Box<dyn FnMut() -> Result<(), String>>>,
}
impl<R: Read> StreamDecoder<R> {
    pub fn new(src: R, use_number: bool) -> StreamDecoder<R> {
        StreamDecoder {
            src,
            buf: Vec::new(),
            scanp: 0,
            scanned: 0,
            state: TokState::TopValue,
            stack: Vec::new(),
            err: None,
            src_err: None,
            use_number,
            check: None,
        }
    }
    pub fn set_check(&mut self, check: Box<dyn FnMut() -> Result<(), String>>) {
        self.check = Some(check);
    }
    pub fn input_offset(&self) -> u64 {
        self.scanned.saturating_add(self.scanp as u64)
    }
    pub fn into_inner(self) -> R {
        self.src
    }
    fn refill(&mut self) -> Result<(), JsonError> {
        if let Some(e) = &self.src_err {
            return Err(e.clone());
        }
        if self.scanp > 0 {
            self.scanned = self.scanned.saturating_add(self.scanp as u64);
            self.buf.drain(..self.scanp);
            self.scanp = 0;
        }
        if let Some(check) = self.check.as_mut()
            && let Err(text) = check()
        {
            let e = JsonError::Other(text);
            self.src_err = Some(e.clone());
            return Err(e);
        }
        let start = self.buf.len();
        self.buf.resize(start.saturating_add(READ_SIZE), 0);
        loop {
            match self.src.read(self.buf.get_mut(start..).unwrap_or_default()) {
                Ok(0) => {
                    self.buf.truncate(start);
                    self.src_err = Some(JsonError::Eof);
                    return Err(JsonError::Eof);
                }
                Ok(n) => {
                    self.buf.truncate(start.saturating_add(n.min(READ_SIZE)));
                    return Ok(());
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.buf.truncate(start);
                    let e = if e.kind() == ErrorKind::UnexpectedEof {
                        JsonError::UnexpectedEof
                    } else {
                        JsonError::Other(e.to_string())
                    };
                    self.src_err = Some(e.clone());
                    return Err(e);
                }
            }
        }
    }
    fn peek(&mut self) -> Result<u8, JsonError> {
        let mut err: Option<JsonError> = None;
        loop {
            let rest = self.buf.get(self.scanp..).unwrap_or_default();
            if let Some(skip) = rest.iter().position(|&c| !is_space(c))
                && let Some(&c) = rest.get(skip)
            {
                self.scanp = self.scanp.saturating_add(skip);
                return Ok(c);
            }
            if let Some(e) = err {
                return Err(e);
            }
            err = self.refill().err();
        }
    }
    fn read_value(&mut self) -> Result<usize, JsonError> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let mut scan = Scanner::new();
        let mut k = 0usize;
        let mut err: Option<JsonError> = None;
        loop {
            let rest = self.buf.get(self.scanp..).unwrap_or_default();
            for &c in rest.get(k..).unwrap_or_default() {
                match scan.step(c) {
                    Op::End => return Ok(k),
                    Op::EndObject | Op::EndArray => {
                        if scan.end_value(b' ') == Op::End {
                            return Ok(k.saturating_add(1));
                        }
                    }
                    Op::Error => {
                        let e = JsonError::Syntax(scan.err.take().unwrap_or_default());
                        self.err = Some(e.clone());
                        return Err(e);
                    }
                    _ => {}
                }
                k = k.saturating_add(1);
            }
            if let Some(mut e) = err {
                if e == JsonError::Eof {
                    if scan.step(b' ') == Op::End {
                        return Ok(rest.len());
                    }
                    if rest.iter().any(|&c| !is_space(c)) {
                        e = JsonError::UnexpectedEof;
                    }
                }
                self.err = Some(e.clone());
                return Err(e);
            }
            err = self.refill().err();
        }
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
    pub fn decode_raw(&mut self) -> Result<Vec<u8>, JsonError> {
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
        let start = raw.iter().position(|&c| !is_space(c)).unwrap_or(raw.len());
        let out = raw.get(start..).unwrap_or_default().to_vec();
        self.scanp = end;
        self.value_end();
        Ok(out)
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
        JsonError::Syntax(format!(
            "invalid character {}{context}",
            super::quote_char(c)
        ))
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
                        return Ok(Token::Str(unquote(&raw)));
                    }
                    if !self.value_allowed() {
                        return Err(self.token_error(c));
                    }
                    let raw = self.decode_raw()?;
                    if raw.first() == Some(&b'"') {
                        return Ok(Token::Str(unquote(&raw)));
                    }
                    if !self.use_number && matches!(raw.first(), Some(b'-' | b'0'..=b'9')) {
                        let s = String::from_utf8_lossy(&raw);
                        if crate::numtext::parse_float(&s).is_err() {
                            return Err(JsonError::Other(format!(
                                "json: cannot unmarshal number {s} into value of type float64"
                            )));
                        }
                    }
                    return Ok(Token::Scalar(raw));
                }
            }
        }
    }
    pub fn more(&mut self) -> bool {
        matches!(self.peek(), Ok(c) if c != b']' && c != b'}')
    }
    pub fn ensure_eof(&mut self) -> Result<(), JsonError> {
        match self.token() {
            Err(JsonError::Eof) => Ok(()),
            Ok(_) => Err(JsonError::Other("unexpected trailing data".into())),
            Err(e) => Err(e),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trickle(Vec<u8>, usize, Option<&'static str>);
    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1 < self.0.len() {
                buf[0] = self.0[self.1];
                self.1 += 1;
                return Ok(1);
            }
            match self.2 {
                Some(text) => Err(std::io::Error::other(text)),
                None => Ok(0),
            }
        }
    }
    #[test]
    fn tokens_match_the_buffer_decoder() {
        let text = br#" [ {"a": [1, 2.5e3, "x\ty"], "b": null}, true ] "#;
        let mut whole = super::super::Decoder::with_tail(text, None, true);
        let mut stream = StreamDecoder::new(Trickle(text.to_vec(), 0, None), true);
        loop {
            let a = whole.token();
            let b = stream.token();
            assert_eq!(a, b);
            if a.is_err() {
                break;
            }
        }
    }
    #[test]
    fn source_error_after_bytes() {
        let mut stream = StreamDecoder::new(Trickle(b"[{\"a\":".to_vec(), 0, Some("cut")), true);
        assert_eq!(stream.token(), Ok(Token::Delim(b'[')));
        assert_eq!(stream.token(), Ok(Token::Delim(b'{')));
        assert_eq!(stream.token(), Ok(Token::Str("a".into())));
        assert_eq!(stream.decode_raw(), Err(JsonError::Other("cut".into())));
    }
}
