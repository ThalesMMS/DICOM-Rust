use super::charset::decode_character_set;
use super::error::{Error, ParseError, Sentinel};
use super::{Delimiters, Field, Message, Segment, text};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseOptions {
    pub max_bytes: i64,
    pub max_segments: i64,
    pub max_fields: i64,
    pub max_repetitions: i64,
    pub default_character_set: String,
}
pub(super) const DEFAULT_MAX_SEGMENTS: i64 = 10000;
pub(super) const DEFAULT_MAX_FIELDS: i64 = 1000;
pub(super) const DEFAULT_MAX_REPETITIONS: i64 = 1000;
impl ParseOptions {
    pub(super) fn with_defaults(mut self) -> ParseOptions {
        if self.max_bytes <= 0 {
            self.max_bytes = 4 << 20;
        }
        if self.max_segments <= 0 {
            self.max_segments = DEFAULT_MAX_SEGMENTS;
        }
        if self.max_fields <= 0 {
            self.max_fields = DEFAULT_MAX_FIELDS;
        }
        if self.max_repetitions <= 0 {
            self.max_repetitions = DEFAULT_MAX_REPETITIONS;
        }
        self
    }
}
pub fn parse(data: &[u8]) -> Result<Message, Error> {
    parse_with_options(data, &ParseOptions::default())
}
fn limit(detail: &str) -> Error {
    Error::wrap(Sentinel::Limit, detail)
}
fn syntax(detail: &str) -> Error {
    Error::wrap(Sentinel::Syntax, detail)
}
pub fn parse_with_options(data: &[u8], options: &ParseOptions) -> Result<Message, Error> {
    let options = options.clone().with_defaults();
    if data.len() as i64 > options.max_bytes {
        return Err(ParseError::new(limit(&format!(
            "{} bytes, limit {}",
            data.len(),
            options.max_bytes
        )))
        .into());
    }
    let delimiters = read_delimiters(data)?;
    let mut charset = raw_character_set(data, &delimiters)?;
    let declared = charset.clone();
    if charset.is_empty() {
        charset = options.default_character_set.clone();
    }
    let text = decode_character_set(&charset, data)
        .map_err(|err| Error::from(ParseError::at(1, "MSH", 18, err)))?;
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    let mut message = Message {
        delimiters,
        segments: Vec::new(),
    };
    for line in text.split('\r') {
        if line.is_empty() {
            continue;
        }
        if message.segments.len() as i64 >= options.max_segments {
            let mut e = ParseError::new(limit(&format!(
                "more than {} segments",
                options.max_segments
            )));
            e.segment = message.segments.len() + 1;
            return Err(e.into());
        }
        match parse_segment(
            line,
            &delimiters,
            &charset,
            message.segments.is_empty(),
            &options,
        ) {
            Ok(segment) => message.segments.push(segment),
            Err(mut e) => {
                e.segment = message.segments.len() + 1;
                return Err(e.into());
            }
        }
    }
    if message.segments.is_empty() {
        return Err(ParseError::new(syntax("no segments")).into());
    }
    if message.header().character_set.trim() != declared {
        return Err(ParseError::at(
            1,
            "MSH",
            18,
            Error::wrap(Sentinel::Charset, "MSH-18 reads differently once decoded"),
        )
        .into());
    }
    Ok(message)
}
pub(super) fn read_delimiters(data: &[u8]) -> Result<Delimiters, Error> {
    read_header_delimiters(data, "MSH")
}
pub(super) fn read_header_delimiters(data: &[u8], header: &str) -> Result<Delimiters, Error> {
    if data.len() < 6 || !data.starts_with(header.as_bytes()) {
        return Err(
            ParseError::new(syntax(&format!("the message does not start with {header}"))).into(),
        );
    }
    let mut d = Delimiters {
        field: data[3],
        ..Delimiters::default()
    };
    let rest = &data[4..];
    let end = rest
        .iter()
        .position(|&c| c == d.field || c == b'\r' || c == b'\n')
        .unwrap_or(data.len() - 4);
    let encoding = &rest[..end];
    if encoding.len() < 2 || encoding.len() > 5 {
        return Err(ParseError::at(
            1,
            header,
            2,
            syntax(&format!("{} encoding characters", encoding.len())),
        )
        .into());
    }
    d.component = encoding[0];
    d.repetition = encoding[1];
    if encoding.len() > 2 {
        d.escape = encoding[2];
    }
    if encoding.len() > 3 {
        d.subcomponent = encoding[3];
    }
    if encoding.len() > 4 {
        d.truncation = encoding[4];
    }
    let mut seen = [false; 256];
    for &c in std::iter::once(&d.field).chain(encoding.iter()) {
        if seen[c as usize] || !(0x21..=0x7E).contains(&c) || c.is_ascii_alphanumeric() {
            return Err(
                ParseError::at(1, header, 2, syntax("invalid or repeated delimiter")).into(),
            );
        }
        seen[c as usize] = true;
    }
    Ok(d)
}
pub(super) fn split_n(s: &[u8], sep: u8, n: i64) -> Vec<&[u8]> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &c) in s.iter().enumerate() {
        if n > 0 && out.len() as i64 == n - 1 {
            break;
        }
        if c == sep {
            out.push(&s[start..i]);
            start = i + 1;
        }
    }
    out.push(&s[start..]);
    out
}
pub(super) fn raw_character_set(data: &[u8], d: &Delimiters) -> Result<String, Error> {
    let line = match data.iter().position(|&c| c == b'\r' || c == b'\n') {
        Some(end) => &data[..end],
        None => data,
    };
    const CHARSET_FIELD: i64 = 18;
    let split = split_segment(line, d, true, CHARSET_FIELD);
    let declared = character_set_field(&split, d);
    if line.iter().any(|&c| c >= 0x80 || c == 0x1b) {
        let plain_parts = split_n(line, d.field, CHARSET_FIELD + 1);
        if let Ok(plain) = character_set_field(&plain_parts, d)
            && !plain.is_empty()
            && declared.as_deref().ok() != Some(plain.as_str())
            && let Ok(decoded) = decode_character_set(&plain, line)
            && let Ok(actual) = character_set_field(
                &split_segment(decoded.as_bytes(), d, true, CHARSET_FIELD),
                d,
            )
            && actual == plain
        {
            return Ok(plain);
        }
    }
    declared
}
fn character_set_field(fields: &[&[u8]], d: &Delimiters) -> Result<String, Error> {
    if fields.len() < 18 {
        return Ok(String::new());
    }
    let mut value = fields[17];
    if let Some(i) = value.iter().position(|&c| c == d.repetition) {
        value = &value[..i];
    }
    if let Some(i) = value.iter().position(|&c| c == d.component) {
        value = &value[..i];
    }
    for &c in value {
        if !(0x20..=0x7E).contains(&c)
            || (d.escape != 0 && c == d.escape)
            || (d.subcomponent != 0 && c == d.subcomponent)
        {
            return Err(ParseError::at(
                1,
                "MSH",
                18,
                Error::wrap(Sentinel::Charset, "MSH-18 is not a printable code"),
            )
            .into());
        }
    }
    let s: String = value.iter().map(|&c| c as char).collect();
    Ok(s.trim().to_owned())
}
pub(super) fn valid_segment_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() != 3 || !b[0].is_ascii_uppercase() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}
fn as_str(b: &[u8]) -> &str {
    std::str::from_utf8(b).expect("ER7 splits UTF-8 text at ASCII delimiters")
}
fn parse_segment(
    line: &str,
    d: &Delimiters,
    charset: &str,
    first: bool,
    options: &ParseOptions,
) -> Result<Segment, ParseError> {
    let parts: Vec<&str> = split_segment(line.as_bytes(), d, first, options.max_fields)
        .into_iter()
        .map(as_str)
        .collect();
    let name = parts[0];
    if !valid_segment_name(name) {
        return Err(ParseError::new(syntax("invalid segment ID")));
    }
    if first != (name == "MSH") {
        let mut e = if first {
            ParseError::new(syntax("the first segment is not MSH"))
        } else {
            ParseError::new(syntax("MSH after the first segment"))
        };
        e.name = name.to_owned();
        return Err(e);
    }
    parse_segment_fields(&parts, d, charset, first, options)
}
pub(super) fn split_segment<'a>(
    line: &'a [u8],
    d: &Delimiters,
    header: bool,
    max_fields: i64,
) -> Vec<&'a [u8]> {
    let n = max_fields.min(line.len() as i64) + 1;
    if !header {
        return split_outside_escapes(line, d.field, d.escape, n + 1);
    }
    let mut parts = split_n(line, d.field, 3);
    if parts.len() == 3 {
        let rest = parts.pop().expect("three parts");
        parts.extend(split_outside_escapes(rest, d.field, d.escape, n - 2));
    }
    parts
}
pub(super) fn split_outside_escapes(text: &[u8], separator: u8, escape: u8, n: i64) -> Vec<&[u8]> {
    if n == 0 || n == 1 || escape == 0 || !text.contains(&escape) {
        return split_n(text, separator, n);
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == escape {
            if let Some(end) = text[i + 1..].iter().position(|&x| x == escape) {
                i += end + 1;
            }
        } else if c == separator {
            parts.push(&text[start..i]);
            start = i + 1;
            if parts.len() as i64 + 1 == n {
                parts.push(&text[start..]);
                return parts;
            }
        }
        i += 1;
    }
    parts.push(&text[start..]);
    parts
}
pub(super) fn parse_segment_fields(
    parts: &[&str],
    d: &Delimiters,
    charset: &str,
    header: bool,
    options: &ParseOptions,
) -> Result<Segment, ParseError> {
    let name = parts[0];
    let mut segment = Segment::new(name);
    let mut values = &parts[1..];
    if header {
        if parts.len() < 2 {
            return Err(ParseError::at(
                0,
                name,
                2,
                Error::Sentinel(Sentinel::Syntax),
            ));
        }
        segment.fields.push(text(&(d.field as char).to_string()));
        segment.fields.push(text(parts[1]));
        values = &parts[2..];
    }
    if (segment.fields.len() + values.len()) as i64 > options.max_fields {
        let mut e = ParseError::new(limit(&format!("more than {} fields", options.max_fields)));
        e.name = name.to_owned();
        return Err(e);
    }
    for raw in values {
        match parse_field(raw, d, charset, options) {
            Ok(field) => segment.fields.push(field),
            Err(err) => {
                return Err(ParseError::at(0, name, segment.fields.len() + 1, err));
            }
        }
    }
    Ok(segment)
}
fn parse_field(
    raw: &str,
    d: &Delimiters,
    charset: &str,
    options: &ParseOptions,
) -> Result<Field, Error> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let repetitions = split_outside_escapes(
        raw.as_bytes(),
        d.repetition,
        d.escape,
        options.max_repetitions.min(raw.len() as i64) + 1,
    );
    if repetitions.len() as i64 > options.max_repetitions {
        return Err(limit(&format!(
            "more than {} repetitions",
            options.max_repetitions
        )));
    }
    let mut field = Vec::with_capacity(repetitions.len());
    for repetition in repetitions {
        let comps = split_outside_escapes(repetition, d.component, d.escape, -1);
        let mut rep = Vec::with_capacity(comps.len());
        for component in comps {
            let subs = if d.subcomponent != 0 {
                split_outside_escapes(component, d.subcomponent, d.escape, -1)
            } else {
                vec![component]
            };
            rep.push(
                subs.into_iter()
                    .map(|s| unescape(as_str(s), d, charset))
                    .collect(),
            );
        }
        field.push(rep);
    }
    Ok(field)
}
pub(super) fn unescape(value: &str, d: &Delimiters, charset: &str) -> String {
    let bytes = value.as_bytes();
    if d.escape == 0 || !bytes.contains(&d.escape) {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    let mut run = 0;
    while i < bytes.len() {
        if bytes[i] != d.escape {
            i += 1;
            continue;
        }
        out.push_str(&value[run..i]);
        let Some(end) = bytes[i + 1..].iter().position(|&c| c == d.escape) else {
            out.push_str(&value[i..]);
            return out;
        };
        let sequence = &value[i + 1..i + 1 + end];
        let raw = &value[i..i + 2 + end];
        i += 2 + end;
        run = i;
        match sequence {
            "F" => out.push(d.field as char),
            "S" => out.push(d.component as char),
            "T" if d.subcomponent != 0 => out.push(d.subcomponent as char),
            "R" => out.push(d.repetition as char),
            "E" => out.push(d.escape as char),
            "P" if d.truncation != 0 => out.push(d.truncation as char),
            "H" | "N" => {}
            ".br" => out.push('\n'),
            _ if sequence.len() > 1 && sequence.starts_with('X') && sequence.len() % 2 == 1 => {
                match decode_hex(&sequence[1..], charset) {
                    Some(decoded) => out.push_str(&decoded),
                    None => out.push_str(raw),
                }
            }
            _ => out.push_str(raw),
        }
    }
    out.push_str(&value[run..]);
    out
}
fn decode_hex(hex: &str, charset: &str) -> Option<String> {
    let b = hex.as_bytes();
    let mut out = Vec::with_capacity(b.len() / 2);
    let mut i = 0;
    while i + 1 < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi << 4 | lo) as u8);
        i += 2;
    }
    decode_character_set(charset, &out).ok()
}
