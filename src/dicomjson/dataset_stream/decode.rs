#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::super::jsonwire::stream::StreamDecoder;
use super::super::jsonwire::{self, JAny, JsonError, RawElement, Token};
use super::super::{
    Budget, Dataset, ERR_DUPLICATE_DATASET_TAG, ERR_EXPECTED_DATASET_ARRAY,
    ERR_EXPECTED_DATASET_OBJECT, ERR_MAX_ELEMENTS, ERR_MAX_JSON_BYTES,
    ERR_PIXEL_DATA_ENCODING_MISMATCH, Limits, base64_decode_padded, base64_decoded_length,
    index_path, join_path, parse_encapsulated_pixel_data, parse_json_tag, sizedparse,
    trim_unicode_space,
};
use super::{DatasetEncodeError, View, field, represented_attribute, validate_represented_values};
use crate::object::transfer::Syntax;
use crate::object::{Tag, Vr};
use std::io::Read;
#[derive(Clone, Copy, Debug, Default)]
pub struct ReaderOptions {
    pub transfer_syntax: Option<Syntax>,
    pub limits: Limits,
}
struct LimitReader<R> {
    inner: R,
    maximum: i64,
    read: i64,
    exceeded: bool,
}
impl<R: Read> Read for LimitReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.exceeded {
            return Err(std::io::Error::other(ERR_MAX_JSON_BYTES));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let mut want = buf.len();
        let mut remaining = 0;
        if self.maximum > 0 {
            remaining = self.maximum.saturating_sub(self.read);
            if remaining < want as i64 {
                want = remaining.saturating_add(1) as usize;
            }
        }
        let want = want.min(buf.len());
        let n = self
            .inner
            .read(buf.get_mut(..want).unwrap_or_default())?
            .min(want);
        if self.maximum > 0 && n as i64 > remaining {
            self.exceeded = true;
        }
        self.read = self.read.saturating_add(n as i64);
        Ok(n)
    }
}
type Decoder<R> = StreamDecoder<LimitReader<R>>;
fn reader<R: Read>(src: R, limits: Limits) -> Result<(Decoder<R>, Budget), String> {
    let budget = Budget::new(limits)?;
    let dec = StreamDecoder::new(
        LimitReader {
            inner: src,
            maximum: limits.max_json_bytes,
            read: 0,
            exceeded: false,
        },
        true,
    );
    Ok((dec, budget))
}
fn check_offset<R: Read>(dec: &Decoder<R>, budget: &Budget) -> Result<(), String> {
    let max = budget.limits.max_json_bytes;
    if max > 0 && dec.input_offset() as i64 > max {
        return Err(ERR_MAX_JSON_BYTES.to_string());
    }
    Ok(())
}
fn text(e: JsonError) -> String {
    e.to_string()
}
fn decode_object<R: Read>(
    dec: &mut Decoder<R>,
    budget: &Budget,
    path: &str,
    check: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let token = dec.token().map_err(text)?;
    if token != Token::Delim(b'{') {
        return Err(format!("{ERR_EXPECTED_DATASET_OBJECT} at {path}"));
    }
    let mut dataset: Vec<(String, Vec<u8>)> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    while dec.more() {
        let token = dec.token().map_err(text)?;
        let Token::Str(key) = token else {
            return Err(format!("{ERR_EXPECTED_DATASET_OBJECT} at {path}"));
        };
        let tag = crate::textfmt::to_upper(trim_unicode_space(&key));
        if tags.contains(&tag) {
            let mut path = path.to_string();
            if tag.len() == 8 && u32::from_str_radix(&tag, 16).is_ok() && !tag.starts_with('+') {
                path = join_path(&path, &tag);
            }
            return Err(format!("{ERR_DUPLICATE_DATASET_TAG} at {path}"));
        }
        let max = budget.limits.max_elements;
        if max > 0 && dataset.len() as i64 >= max.saturating_sub(budget.elements) {
            return Err(format!("{ERR_MAX_ELEMENTS}: got more than {max}"));
        }
        let value = dec.decode_raw().map_err(text)?;
        check(&value)?;
        dataset.push((key, value));
        tags.push(tag);
    }
    dec.token().map_err(text)?;
    Ok(dataset)
}
type RawMembers = Vec<(String, Vec<u8>)>;
fn decode_array<R: Read>(
    dec: &mut Decoder<R>,
    budget: &mut Budget,
    check: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    handle: &mut dyn FnMut(&mut Budget, RawMembers) -> Result<(), String>,
) -> Result<(), String> {
    let token = dec.token().map_err(text)?;
    if token != Token::Delim(b'[') {
        return Err(ERR_EXPECTED_DATASET_ARRAY.to_string());
    }
    let mut index = 0;
    while dec.more() {
        let raw = decode_object(dec, budget, &index_path("", index), check)?;
        check_offset(dec, budget)?;
        handle(budget, raw)?;
        index = index.saturating_add(1);
    }
    let token = dec.token().map_err(text)?;
    if token != Token::Delim(b']') {
        return Err("dicomjson: invalid dataset array".to_string());
    }
    dec.ensure_eof().map_err(text)
}
pub(crate) fn decode_raw_datasets_reader<R: Read>(
    src: R,
    opts: &ReaderOptions,
    handle: &mut dyn FnMut(Dataset) -> Result<(), String>,
) -> Result<(), String> {
    let (mut dec, mut budget) = reader(src, opts.limits)?;
    decode_array(
        &mut dec,
        &mut budget,
        &mut |value| jsonwire::as_element(value).map(|_| ()).map_err(text),
        &mut |budget, raw| {
            let mut dataset: Dataset = Vec::with_capacity(raw.len());
            for (key, value) in raw {
                dataset.push((key, jsonwire::as_element(&value).map_err(text)?));
            }
            budget.enter_data_set(0, dataset.len() as i64)?;
            handle(dataset)
        },
    )
}
pub(crate) fn decode_datasets_reader<R: Read>(
    src: R,
    opts: &ReaderOptions,
    handle: &mut dyn FnMut(Dataset) -> Result<(), String>,
) -> Result<(), String> {
    let (mut dec, mut budget) = reader(src, opts.limits)?;
    let syntax = opts.transfer_syntax;
    decode_array(
        &mut dec,
        &mut budget,
        &mut |_| Ok(()),
        &mut |budget, raw| {
            let mut generic: Vec<(String, JAny)> = raw
                .into_iter()
                .map(|(key, value)| (key, jsonwire::parse_any(&value)))
                .collect();
            generic.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            validate_dataset(budget, syntax, Some(&generic), 0)?;
            let mut dataset: Dataset = Vec::with_capacity(generic.len());
            for (key, value) in &generic {
                let view = represented_attribute::<String>(value, 0).map_err(flatten)?;
                dataset.push((key.clone(), view.element));
            }
            handle(dataset)
        },
    )
}
fn flatten(e: DatasetEncodeError<String>) -> String {
    match e {
        DatasetEncodeError::Encode(s) | DatasetEncodeError::Validate(s) => s,
    }
}
fn validate_dataset(
    budget: &mut Budget,
    syntax: Option<Syntax>,
    generic: Option<&[(String, JAny)]>,
    depth: i64,
) -> Result<(), String> {
    budget.enter_data_set(depth, generic.map_or(0, <[_]>::len) as i64)?;
    let Some(entries) = generic else {
        return Ok(());
    };
    for (key, value) in entries {
        let tag = parse_json_tag(key, key)?;
        let view = represented_attribute::<String>(value, 0).map_err(flatten)?;
        validate_element(budget, syntax, tag, &view, depth)?;
    }
    Ok(())
}
fn validate_element(
    budget: &mut Budget,
    syntax: Option<Syntax>,
    tag: Tag,
    view: &View<'_>,
    depth: i64,
) -> Result<(), String> {
    let e: &RawElement = &view.element;
    let Some(vr) = Vr::parse(e.vr.as_bytes()) else {
        return Err(format!(
            "dicom: invalid VR {}",
            crate::textfmt::quote(&e.vr)
        ));
    };
    let fields = view.fields.unwrap_or(&[]);
    let has_value = field(fields, "Value").is_some();
    let has_inline = field(fields, "InlineBinary").is_some();
    let has_bulk = field(fields, "BulkDataURI").is_some();
    if [has_value, has_inline, has_bulk]
        .iter()
        .filter(|b| **b)
        .count()
        > 1
    {
        return Err("dicomjson: conflicting element representations".into());
    }
    if vr == Vr::SQ && (has_inline || has_bulk) {
        return Err("dicomjson: binary representation is invalid for SQ".into());
    }
    let encapsulated = syntax.is_some_and(|s| s.encapsulated);
    if has_inline {
        budget.add_inline_binary(base64_decoded_length(&e.inline_binary) as i64)?;
        let invalid = |e: B64Err| format!("dicomjson: invalid InlineBinary: {}", e.text());
        let mut stream = B64Stream::new(e.inline_binary.as_bytes());
        let mut prefix: Vec<u8> = Vec::new();
        let mut err = None;
        while prefix.len() < 4 && err.is_none() {
            let (bytes, e) = stream.read(4usize.saturating_sub(prefix.len()));
            prefix.extend_from_slice(&bytes);
            err = e;
        }
        if prefix.len() >= 4 {
            err = None;
        }
        if let Some(e) = err
            && !matches!(e, B64Err::Eof | B64Err::UnexpectedEof)
        {
            return Err(invalid(e));
        }
        if tag == Tag::PIXEL_DATA
            && !encapsulated
            && prefix.len() == 4
            && (prefix[..] == [0xfe, 0xff, 0x00, 0xe0] || prefix[..] == [0xff, 0xfe, 0xe0, 0x00])
        {
            return Err(ERR_PIXEL_DATA_ENCODING_MISMATCH.into());
        }
        loop {
            match stream.read(8192).1 {
                None => {}
                Some(B64Err::Eof) => break,
                Some(e) => return Err(invalid(e)),
            }
        }
        if let (true, Some(syntax)) = (tag == Tag::PIXEL_DATA && encapsulated, syntax) {
            let Some(data) = base64_decode_padded(&e.inline_binary) else {
                return Err("dicomjson: invalid InlineBinary".into());
            };
            parse_encapsulated_pixel_data(&data, syntax, budget)?;
        }
    }
    if !has_value {
        return Ok(());
    }
    if vr != Vr::SQ {
        return validate_represented_values::<String>(vr, &e.value).map_err(flatten);
    }
    budget.add_sequence_items(e.value.len() as i64)?;
    for item in &e.value {
        match item {
            JAny::Obj(entries) => {
                validate_dataset(budget, syntax, Some(entries), depth.saturating_add(1))?
            }
            JAny::Null => validate_dataset(budget, syntax, None, depth.saturating_add(1))?,
            _ => return Err("dicomjson: invalid sequence item".into()),
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum B64Err {
    Eof,
    UnexpectedEof,
    Corrupt(usize),
}
impl B64Err {
    fn text(self) -> String {
        match self {
            B64Err::Eof => "EOF".into(),
            B64Err::UnexpectedEof => "unexpected EOF".into(),
            B64Err::Corrupt(at) => format!("illegal base64 data at input byte {at}"),
        }
    }
}
struct B64Stream<'a> {
    src: &'a [u8],
    pos: usize,
    buf: [u8; 1024],
    nbuf: usize,
    out: Vec<u8>,
    err: Option<B64Err>,
    read_eof: bool,
}
impl<'a> B64Stream<'a> {
    fn new(src: &'a [u8]) -> B64Stream<'a> {
        B64Stream {
            src,
            pos: 0,
            buf: [0; 1024],
            nbuf: 0,
            out: Vec::new(),
            err: None,
            read_eof: false,
        }
    }
    fn fill(&mut self, lo: usize, hi: usize) -> usize {
        loop {
            let src = self.src.get(self.pos..).unwrap_or_default();
            let n = hi.saturating_sub(lo).min(src.len());
            if n == 0 {
                return 0;
            }
            let dst = self.buf.get_mut(lo..).unwrap_or_default();
            let mut offset = 0usize;
            for &b in src.get(..n).unwrap_or_default() {
                if b != b'\r' && b != b'\n' {
                    if let Some(d) = dst.get_mut(offset) {
                        *d = b;
                    }
                    offset = offset.saturating_add(1);
                }
            }
            self.pos = self.pos.saturating_add(n);
            if offset > 0 {
                return offset;
            }
        }
    }
    fn read(&mut self, plen: usize) -> (Vec<u8>, Option<B64Err>) {
        if !self.out.is_empty() {
            let n = plen.min(self.out.len());
            return (self.out.drain(..n).collect(), None);
        }
        if let Some(e) = self.err {
            return (Vec::new(), Some(e));
        }
        while self.nbuf < 4 && !self.read_eof {
            let nn = (plen / 3).saturating_mul(4).clamp(4, self.buf.len());
            let got = self.fill(self.nbuf, nn);
            if got == 0 {
                self.read_eof = true;
            }
            self.nbuf = self.nbuf.saturating_add(got);
        }
        if self.nbuf < 4 {
            let e = if self.nbuf > 0 {
                B64Err::UnexpectedEof
            } else {
                B64Err::Eof
            };
            self.err = Some(e);
            return (Vec::new(), Some(e));
        }
        let nr = self.nbuf & !3;
        let nw = (self.nbuf / 4).saturating_mul(3);
        let (decoded, err) =
            match sizedparse::base64_decode(self.buf.get(..nr).unwrap_or_default(), false) {
                Ok(d) => (d, None),
                Err((at, d)) => (d, Some(B64Err::Corrupt(at))),
            };
        self.err = err;
        let ret = if nw > plen {
            self.out = decoded;
            let n = plen.min(self.out.len());
            self.out.drain(..n).collect()
        } else {
            decoded
        };
        self.buf.copy_within(nr..self.nbuf, 0);
        self.nbuf = self.nbuf.saturating_sub(nr);
        (ret, err)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trickle(Vec<u8>, usize);
    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1 >= self.0.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.0[self.1];
            self.1 += 1;
            Ok(1)
        }
    }
    #[test]
    fn datasets_arrive_one_by_one() {
        let body = br#"[{"00100010":{"vr":"PN","Value":[{"Alphabetic":"A"}]}},{"00200013":{"vr":"IS","Value":[3]}}]"#;
        let mut seen = Vec::new();
        decode_datasets_reader(
            Trickle(body.to_vec(), 0),
            &ReaderOptions::default(),
            &mut |d| {
                seen.push(d[0].0.clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, vec!["00100010", "00200013"]);
    }
    #[test]
    fn stream_base64_accepts_inner_padding() {
        let mut s = B64Stream::new(b"QQ==QUJD");
        let mut got = Vec::new();
        while got.len() < 4 {
            let (b, e) = s.read(4 - got.len());
            got.extend(b);
            assert!(e.is_none());
        }
        assert_eq!(got, b"AABC");
    }
    #[test]
    fn represented_validation_errors() {
        let body = br#"[{"00100020":{"vr":"LO","Value":["a\\b"]}}]"#;
        let err = decode_datasets_reader(&body[..], &ReaderOptions::default(), &mut |_| Ok(()))
            .unwrap_err();
        assert_eq!(err, "dicomjson: text value delimiter");
    }
}
