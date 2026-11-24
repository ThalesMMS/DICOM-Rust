use super::charset::decode_character_set;
use super::error::{BatchError, Error, Sentinel};
use super::parse::{
    ParseOptions, parse_segment_fields, parse_with_options, read_header_delimiters, split_segment,
};
use super::stream::{Cancel, Er7Input, PeekErr};
use super::{Delimiters, FieldExt, Message, Segment};
use std::io::Read;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchOptions {
    pub max_bytes: i64,
    pub max_batches: i64,
    pub max_messages: i64,
    pub message_options: ParseOptions,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchFile {
    pub header: Option<Segment>,
    pub trailer: Option<Segment>,
    pub batches: Vec<Batch>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    pub header: Segment,
    pub trailer: Segment,
    pub messages: Vec<Message>,
}
fn batch_err(offset: i64, batch: usize, message: usize, err: Error) -> Error {
    Error::Batch(Box::new(BatchError {
        offset,
        batch,
        message,
        err,
    }))
}
fn envelope(detail: &str) -> Error {
    Error::wrap(Sentinel::BatchEnvelope, detail)
}
fn peek_err(e: PeekErr) -> Error {
    match e {
        PeekErr::Eof => Error::text("EOF"),
        PeekErr::Err(e) => e,
    }
}
pub fn parse_batch<R: Read>(
    cancel: &dyn Cancel,
    input: R,
    options: &BatchOptions,
) -> Result<BatchFile, Error> {
    let mut options = options.clone();
    if options.max_bytes < 0 || options.max_batches < 0 || options.max_messages < 0 {
        return Err(batch_err(
            0,
            0,
            0,
            Error::wrap(Sentinel::Limit, "negative batch limit"),
        ));
    }
    if options.max_bytes == 0 {
        options.max_bytes = 32 << 20;
    }
    if options.max_batches == 0 {
        options.max_batches = 1000;
    }
    if options.max_messages == 0 {
        options.max_messages = 10000;
    }
    options.message_options = options.message_options.clone().with_defaults();
    let mut parser = BatchParser {
        cancel,
        input: Er7Input::new(input),
        options,
        messages: 0,
    };
    parser.parse()
}
struct BatchParser<'a, R: Read> {
    cancel: &'a dyn Cancel,
    input: Er7Input<R>,
    options: BatchOptions,
    messages: i64,
}
impl<R: Read> BatchParser<'_, R> {
    fn parse(&mut self) -> Result<BatchFile, Error> {
        let mut result = BatchFile::default();
        let mut name = self
            .next_envelope_name()
            .map_err(|e| batch_err(self.input.offset, 0, 0, e))?;
        let mut file_delimiters = Delimiters::default();
        if name == "FHS" {
            let offset = self.input.offset;
            let header = self
                .read_envelope("FHS", &mut file_delimiters)
                .map_err(|e| batch_err(offset, 0, 0, e))?;
            result.header = Some(header);
            name = self
                .next_envelope_name()
                .map_err(|e| batch_err(self.input.offset, 0, 0, e))?;
        }
        while name == "BHS" {
            let number = result.batches.len() + 1;
            if number as i64 > self.options.max_batches {
                return Err(batch_err(
                    self.input.offset,
                    number,
                    0,
                    Error::wrap(Sentinel::Limit, "number of batches"),
                ));
            }
            let batch = self.read_batch(number)?;
            result.batches.push(batch);
            name = self
                .next_envelope_name()
                .map_err(|e| batch_err(self.input.offset, 0, 0, e))?;
        }
        if result.batches.is_empty() {
            return Err(batch_err(self.input.offset, 0, 0, envelope("missing BHS")));
        }
        if result.header.is_some() {
            let offset = self.input.offset;
            if name != "FTS" {
                return Err(batch_err(offset, 0, 0, envelope("expected FTS")));
            }
            let trailer = self
                .read_envelope("FTS", &mut file_delimiters)
                .and_then(|t| validate_batch_count(&t, result.batches.len()).map(|()| t))
                .map_err(|e| batch_err(offset, 0, 0, e))?;
            result.trailer = Some(trailer);
            name = self
                .next_envelope_name()
                .map_err(|e| batch_err(self.input.offset, 0, 0, e))?;
        }
        if !name.is_empty() {
            return Err(batch_err(
                self.input.offset,
                0,
                0,
                envelope("data after the last trailer"),
            ));
        }
        Ok(result)
    }
    fn read_batch(&mut self, number: usize) -> Result<Batch, Error> {
        let mut batch = Batch::default();
        let mut delimiters = Delimiters::default();
        let offset = self.input.offset;
        batch.header = self
            .read_envelope("BHS", &mut delimiters)
            .map_err(|e| batch_err(offset, number, 0, e))?;
        loop {
            let name = self.next_envelope_name();
            let offset = self.input.offset;
            let name = name.map_err(|e| batch_err(offset, number, 0, e))?;
            match name.as_str() {
                "MSH" => {
                    if self.messages >= self.options.max_messages {
                        return Err(batch_err(
                            offset,
                            number,
                            batch.messages.len() + 1,
                            Error::wrap(Sentinel::Limit, "number of messages"),
                        ));
                    }
                    let message = self
                        .read_message()
                        .map_err(|e| batch_err(offset, number, batch.messages.len() + 1, e))?;
                    batch.messages.push(message);
                    self.messages += 1;
                }
                "BTS" => {
                    let trailer = self
                        .read_envelope("BTS", &mut delimiters)
                        .and_then(|t| validate_batch_count(&t, batch.messages.len()).map(|()| t))
                        .map_err(|e| batch_err(offset, number, 0, e))?;
                    batch.trailer = trailer;
                    return Ok(batch);
                }
                _ => {
                    return Err(batch_err(
                        offset,
                        number,
                        0,
                        envelope("expected MSH or BTS"),
                    ));
                }
            }
        }
    }
    fn next_envelope_name(&mut self) -> Result<String, Error> {
        loop {
            let name = match self.peek_name() {
                Ok(Some(name)) => name,
                Ok(None) => return Ok(String::new()),
                Err(e) => return Err(e),
            };
            let first = name.as_bytes().first().copied();
            if first.is_some_and(|c| c != b'\r' && c != b'\n') {
                return Ok(name);
            }
            self.read_segment(Vec::new(), 1)?;
        }
    }
    fn peek_name(&mut self) -> Result<Option<String>, Error> {
        let (name, err) = self.input.peek_name(self.cancel);
        match err {
            Some(PeekErr::Eof) => Ok(None),
            Some(PeekErr::Err(e)) => Err(e),
            None => {
                if self.input.offset >= self.options.max_bytes {
                    return Err(Error::wrap(Sentinel::Limit, "batch file bytes"));
                }
                Ok(Some(name))
            }
        }
    }
    fn read_segment(&mut self, dst: Vec<u8>, maximum: i64) -> Result<Vec<u8>, Error> {
        let remaining = self.options.max_bytes - self.input.offset;
        let limit = maximum.min(dst.len() as i64 + remaining);
        let (d, err) = self.input.read_segment(self.cancel, dst, limit);
        match err {
            None => Ok(d),
            Some(e) => Err(peek_err(e)),
        }
    }
    fn read_message(&mut self) -> Result<Message, Error> {
        let options = self.options.message_options.clone();
        let mut raw = Vec::new();
        loop {
            raw = self.read_segment(raw, options.max_bytes)?;
            let (name, err) = self.input.peek_name(self.cancel);
            match err {
                Some(PeekErr::Eof) => break,
                Some(PeekErr::Err(e)) => return Err(e),
                None => {}
            }
            if matches!(name.as_str(), "MSH" | "FHS" | "BHS" | "BTS" | "FTS") {
                break;
            }
        }
        parse_with_options(&raw, &options)
    }
    fn read_envelope(&mut self, name: &str, delimiters: &mut Delimiters) -> Result<Segment, Error> {
        let options = ParseOptions {
            default_character_set: self.options.message_options.default_character_set.clone(),
            ..ParseOptions::default()
        }
        .with_defaults();
        let data = self.read_segment(Vec::new(), options.max_bytes)?;
        let header = name == "FHS" || name == "BHS";
        if header {
            *delimiters = read_header_delimiters(&data, name)
                .map_err(|e| Error::join2(Error::Sentinel(Sentinel::BatchEnvelope), e))?;
        }
        let text = decode_character_set(&options.default_character_set, &data)?;
        let line = text.trim_end_matches(['\r', '\n']);
        let parts: Vec<&str> =
            split_segment(line.as_bytes(), delimiters, header, options.max_fields)
                .into_iter()
                .map(|p| std::str::from_utf8(p).expect("UTF-8 split at ASCII"))
                .collect();
        if parts[0] != name {
            return Err(envelope(&format!("invalid {name} field separator")));
        }
        parse_segment_fields(
            &parts,
            delimiters,
            &options.default_character_set,
            header,
            &options,
        )
        .map_err(Error::from)
    }
}
fn validate_batch_count(trailer: &Segment, actual: usize) -> Result<(), Error> {
    let count = trailer.field(1);
    if count.is_empty() {
        return Ok(());
    }
    if count.len() != 1 || count[0].len() != 1 || count[0][0].len() != 1 {
        return Err(Error::wrap(
            Sentinel::BatchCount,
            &format!("expected a single value in {}-1", trailer.name),
        ));
    }
    let value = count.string();
    let ok = !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|n| n == actual as u64);
    if !ok {
        return Err(Error::wrap(
            Sentinel::BatchCount,
            &format!("{}-1 must equal the observed count", trailer.name),
        ));
    }
    Ok(())
}
