use super::charset::{
    Encoder, Encoding, RUNE_NOT_SUPPORTED, Set, encode_character_set, encoding_for,
};
use super::error::{Error, ParseError, Sentinel};
use super::parse::{raw_character_set, valid_segment_name};
use super::{Component, DEFAULT_DELIMITERS, Delimiters, Field, FieldExt, Message};
pub(super) trait Er7Writer {
    fn write_byte(&mut self, b: u8) -> Result<(), Error>;
    fn write_bytes(&mut self, s: &[u8]) -> Result<(), Error>;
}
impl Er7Writer for Vec<u8> {
    fn write_byte(&mut self, b: u8) -> Result<(), Error> {
        self.push(b);
        Ok(())
    }
    fn write_bytes(&mut self, s: &[u8]) -> Result<(), Error> {
        self.extend_from_slice(s);
        Ok(())
    }
}
impl Message {
    pub fn to_er7(&self) -> String {
        self.encode().unwrap_or_default()
    }
    fn encode(&self) -> Result<String, Error> {
        let mut b = Vec::new();
        self.write_er7(&mut b)?;
        Ok(String::from_utf8(b).expect("ER7 of UTF-8 values"))
    }
    pub fn marshal(&self) -> Result<Vec<u8>, Error> {
        let text = self.encode()?;
        let charset = self.header().character_set.trim().to_owned();
        let encoded = encode_character_set(&charset, &text)?;
        self.check_encoded_character_set(&encoded, &charset)?;
        Ok(encoded)
    }
    fn check_encoded_character_set(&self, encoded: &[u8], charset: &str) -> Result<(), Error> {
        let mut d = self.delimiters;
        if d.field == 0 || d.component == 0 || d.repetition == 0 {
            d = DEFAULT_DELIMITERS;
        }
        match raw_character_set(encoded, &d) {
            Ok(raw) if raw == charset => Ok(()),
            _ => Err(Error::wrap(
                Sentinel::Charset,
                "a character before MSH-18 encodes with a delimiter byte",
            )),
        }
    }
    pub(super) fn write_er7(&self, b: &mut dyn Er7Writer) -> Result<(), Error> {
        if self.segments.first().is_none_or(|s| s.name != "MSH") {
            return Err(Error::wrap(
                Sentinel::Syntax,
                "the first segment must be MSH",
            ));
        }
        let mut d = self.delimiters;
        if d.field == 0 || d.component == 0 || d.repetition == 0 {
            d = DEFAULT_DELIMITERS;
        }
        for (index, segment) in self.segments.iter().enumerate() {
            if !valid_segment_name(&segment.name) {
                let mut e = ParseError::new(Error::wrap(Sentinel::Syntax, "invalid segment ID"));
                e.segment = index + 1;
                return Err(e.into());
            }
            if index > 0 && segment.name == "MSH" {
                return Err(ParseError::at(
                    index + 1,
                    "MSH",
                    0,
                    Error::text("hl7: MSH after the first segment"),
                )
                .into());
            }
            b.write_bytes(segment.name.as_bytes())?;
            let mut fields: &[Field] = &segment.fields;
            if segment.name == "MSH" {
                b.write_byte(d.field)?;
                b.write_bytes(d.encoding_characters().as_bytes())?;
                fields = if fields.len() > 2 { &fields[2..] } else { &[] };
            }
            while fields.last().is_some_and(|f| f.is_empty_value()) {
                fields = &fields[..fields.len() - 1];
            }
            for field in fields {
                b.write_byte(d.field)?;
                write_field(b, field, &d)?;
            }
            b.write_byte(b'\r')?;
        }
        Ok(())
    }
    pub fn marshal_with_limit(&self, max_bytes: i64) -> Result<Vec<u8>, Error> {
        if max_bytes <= 0 {
            return Err(Error::Sentinel(Sentinel::Limit));
        }
        if self.segments.first().is_none_or(|s| s.name != "MSH") {
            return Err(Error::wrap(
                Sentinel::Syntax,
                "the first segment must be MSH",
            ));
        }
        let charset = self.header().character_set.trim().to_owned();
        if charset.len() > 32 {
            return Err(Error::wrap(Sentinel::Charset, "unsupported MSH-18 value"));
        }
        let set = encoding_for(&charset)?;
        let encoder = match set {
            Set::Encoded(e) => Some(TransformWriter::new(e)),
            _ => None,
        };
        let mut writer = BoundedWriter {
            buffer: BufWriter {
                buf: Vec::with_capacity(4096),
                err: None,
                sink: Sink {
                    output: Output {
                        data: Vec::new(),
                        max: max_bytes,
                    },
                    encoder,
                },
            },
            ascii: set == Set::Ascii,
        };
        if let Err(err) = self.write_er7(&mut writer) {
            return Err(bounded_encode_error(err));
        }
        if let Err(err) = writer.buffer.flush() {
            return Err(bounded_encode_error(err));
        }
        let sink = &mut writer.buffer.sink;
        if let Some(enc) = sink.encoder.as_mut()
            && let Err(err) = enc.close(&mut sink.output)
        {
            return Err(bounded_encode_error(err));
        }
        let data = std::mem::take(&mut writer.buffer.sink.output.data);
        self.check_encoded_character_set(&data, &charset)?;
        Ok(data)
    }
}
fn bounded_encode_error(err: Error) -> Error {
    if err.is(Sentinel::Limit) || err.is(Sentinel::Charset) || err.as_parse().is_some() {
        return err;
    }
    Error::Wrapped {
        text: format!(
            "{}: a character cannot be represented in the declared set: {err}",
            Sentinel::Charset.text()
        ),
        wrapped: vec![Error::Sentinel(Sentinel::Charset), err],
    }
}
fn write_field(b: &mut dyn Er7Writer, field: &Field, d: &Delimiters) -> Result<(), Error> {
    let mut repetitions: &[Vec<Component>] = field;
    while repetitions
        .last()
        .is_some_and(|r| std::slice::from_ref(r).is_empty_value())
    {
        repetitions = &repetitions[..repetitions.len() - 1];
    }
    for (i, repetition) in repetitions.iter().enumerate() {
        if i > 0 {
            b.write_byte(d.repetition)?;
        }
        let mut comps: &[Component] = repetition;
        while comps.last().is_some_and(|c| c.iter().all(String::is_empty)) {
            comps = &comps[..comps.len() - 1];
        }
        for (j, component) in comps.iter().enumerate() {
            if j > 0 {
                b.write_byte(d.component)?;
            }
            let mut subs: &[String] = component;
            while subs.last().is_some_and(String::is_empty) {
                subs = &subs[..subs.len() - 1];
            }
            for (k, sub) in subs.iter().enumerate() {
                if k > 0 && d.subcomponent != 0 {
                    b.write_byte(d.subcomponent)?;
                }
                write_escaped(b, sub.as_bytes(), d)?;
            }
        }
    }
    Ok(())
}
fn write_escaped(b: &mut dyn Er7Writer, value: &[u8], d: &Delimiters) -> Result<(), Error> {
    if d.escape == 0 {
        return b.write_bytes(value);
    }
    let mut raw_start = 0;
    for i in 0..value.len() {
        if i - raw_start >= 2048 {
            b.write_bytes(&value[raw_start..i])?;
            raw_start = i;
        }
        let c = value[i];
        let hex;
        let code: &[u8] = if c == d.escape {
            b"E"
        } else if c == d.field {
            b"F"
        } else if c == d.component {
            b"S"
        } else if c == d.repetition {
            b"R"
        } else if d.subcomponent != 0 && c == d.subcomponent {
            b"T"
        } else if d.truncation != 0 && c == d.truncation {
            b"P"
        } else if c == b'\n' {
            b".br"
        } else if c < 0x20 || c == 0x7F {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            hex = [b'X', HEX[(c >> 4) as usize], HEX[(c & 15) as usize]];
            &hex
        } else {
            continue;
        };
        b.write_bytes(&value[raw_start..i])?;
        b.write_byte(d.escape)?;
        b.write_bytes(code)?;
        b.write_byte(d.escape)?;
        raw_start = i + 1;
    }
    b.write_bytes(&value[raw_start..])
}
struct Output {
    data: Vec<u8>,
    max: i64,
}
impl Output {
    fn write(&mut self, p: &[u8]) -> Result<(), Error> {
        if p.len() as i64 > self.max - self.data.len() as i64 {
            return Err(Error::Sentinel(Sentinel::Limit));
        }
        self.data.extend_from_slice(p);
        Ok(())
    }
}
struct TransformWriter {
    encoder: Encoder,
    pending: Vec<u8>,
}
impl TransformWriter {
    fn new(e: Encoding) -> TransformWriter {
        TransformWriter {
            encoder: Encoder::new(e),
            pending: Vec::new(),
        }
    }
    fn write(&mut self, data: &[u8], out: &mut Output) -> Result<(), Error> {
        let mut src = std::mem::take(&mut self.pending);
        src.extend_from_slice(data);
        let valid = match std::str::from_utf8(&src) {
            Ok(_) => src.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = std::str::from_utf8(&src[..valid]).expect("valid prefix");
        let mut encoded = Vec::new();
        let result = self.encoder.encode(text, &mut encoded);
        out.write(&encoded)?;
        if result.is_err() {
            return Err(Error::text(RUNE_NOT_SUPPORTED));
        }
        self.pending = src[valid..].to_vec();
        Ok(())
    }
    fn close(&mut self, out: &mut Output) -> Result<(), Error> {
        let mut encoded = Vec::new();
        self.encoder.close(&mut encoded);
        out.write(&encoded)
    }
}
struct Sink {
    output: Output,
    encoder: Option<TransformWriter>,
}
impl Sink {
    fn write(&mut self, p: &[u8]) -> Result<(), Error> {
        match self.encoder.as_mut() {
            Some(enc) => enc.write(p, &mut self.output),
            None => self.output.write(p),
        }
    }
}
struct BufWriter {
    buf: Vec<u8>,
    err: Option<Error>,
    sink: Sink,
}
const BUF_SIZE: usize = 4096;
impl BufWriter {
    fn available(&self) -> usize {
        BUF_SIZE - self.buf.len()
    }
    fn flush(&mut self) -> Result<(), Error> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.buf.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.buf);
        if let Err(e) = self.sink.write(&data) {
            self.buf = data;
            self.err = Some(e.clone());
            return Err(e);
        }
        Ok(())
    }
    fn write_byte(&mut self, b: u8) -> Result<(), Error> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.available() == 0 {
            self.flush()?;
        }
        self.buf.push(b);
        Ok(())
    }
    fn write_bytes(&mut self, mut s: &[u8]) -> Result<(), Error> {
        while s.len() > self.available() && self.err.is_none() {
            let n = self.available();
            self.buf.extend_from_slice(&s[..n]);
            s = &s[n..];
            let _ = self.flush();
        }
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        self.buf.extend_from_slice(s);
        Ok(())
    }
}
struct BoundedWriter {
    buffer: BufWriter,
    ascii: bool,
}
fn not_ascii() -> Error {
    Error::wrap(
        Sentinel::Charset,
        "a character is not ASCII; declare MSH-18",
    )
}
impl Er7Writer for BoundedWriter {
    fn write_byte(&mut self, b: u8) -> Result<(), Error> {
        if self.ascii && b > 0x7F {
            return Err(not_ascii());
        }
        self.buffer.write_byte(b)
    }
    fn write_bytes(&mut self, mut s: &[u8]) -> Result<(), Error> {
        while !s.is_empty() {
            let n = s.len().min(2048);
            let chunk = &s[..n];
            if self.ascii && chunk.iter().any(|&c| c > 0x7F) {
                return Err(not_ascii());
            }
            self.buffer.write_bytes(chunk)?;
            s = &s[n..];
        }
        Ok(())
    }
}
