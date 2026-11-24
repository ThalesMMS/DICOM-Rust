use super::Message;
use super::error::{Error, ParseError, Sentinel, StreamError};
use super::parse::{ParseOptions, parse_with_options};
use std::io::Read;
pub trait Cancel {
    fn err(&self) -> Option<Error>;
}
impl Cancel for () {
    fn err(&self) -> Option<Error> {
        None
    }
}
pub(super) enum PeekErr {
    Eof,
    Err(Error),
}
pub(super) struct Er7Input<R: Read> {
    reader: R,
    buf: Vec<u8>,
    r: usize,
    w: usize,
    rd_err: Option<Option<Error>>,
    pub(super) offset: i64,
    eof: bool,
}
const BUF: usize = 4096;
impl<R: Read> Er7Input<R> {
    pub(super) fn new(reader: R) -> Er7Input<R> {
        Er7Input {
            reader,
            buf: vec![0; BUF],
            r: 0,
            w: 0,
            rd_err: None,
            offset: 0,
            eof: false,
        }
    }
    fn buffered(&self) -> usize {
        self.w - self.r
    }
    fn fill(&mut self) {
        if self.r > 0 {
            self.buf.copy_within(self.r..self.w, 0);
            self.w -= self.r;
            self.r = 0;
        }
        for _ in 0..100 {
            match self.reader.read(&mut self.buf[self.w..]) {
                Ok(0) => {
                    self.rd_err = Some(None);
                    return;
                }
                Ok(n) => {
                    self.w += n;
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.rd_err = Some(Some(Error::text(e.to_string())));
                    return;
                }
            }
        }
        self.rd_err = Some(Some(Error::text(
            "multiple Read calls return no data or error",
        )));
    }
    fn bufio_peek(&mut self, n: usize) -> (usize, Option<Option<Error>>) {
        while self.buffered() < n && self.buffered() < BUF && self.rd_err.is_none() {
            self.fill();
        }
        let avail = self.buffered();
        if avail < n {
            let err = self
                .rd_err
                .take()
                .unwrap_or(Some(Error::text("bufio: buffer full")));
            return (avail, Some(err));
        }
        (n, None)
    }
    pub(super) fn peek(&mut self, cancel: &dyn Cancel, count: usize) -> (Vec<u8>, Option<PeekErr>) {
        if let Some(e) = cancel.err() {
            return (Vec::new(), Some(PeekErr::Err(e)));
        }
        if self.eof && self.buffered() < count {
            let data = self.buf[self.r..self.w].to_vec();
            return (data, Some(PeekErr::Eof));
        }
        let (n, err) = self.bufio_peek(count);
        if let Some(e) = cancel.err() {
            return (Vec::new(), Some(PeekErr::Err(e)));
        }
        let data = self.buf[self.r..self.r + n].to_vec();
        match err {
            None => (data, None),
            Some(None) => {
                self.eof = true;
                (data, Some(PeekErr::Eof))
            }
            Some(Some(_)) => (
                Vec::new(),
                Some(PeekErr::Err(Error::Sentinel(Sentinel::StreamRead))),
            ),
        }
    }
    pub(super) fn peek_name(&mut self, cancel: &dyn Cancel) -> (String, Option<PeekErr>) {
        let (prefix, err) = self.peek(cancel, 3);
        let name: String = prefix.iter().map(|&b| b as char).collect();
        match err {
            Some(PeekErr::Eof) if !prefix.is_empty() => (name, None),
            e => (name, e),
        }
    }
    pub(super) fn read_segment(
        &mut self,
        cancel: &dyn Cancel,
        mut dst: Vec<u8>,
        max_bytes: i64,
    ) -> (Vec<u8>, Option<PeekErr>) {
        let start = dst.len();
        loop {
            let (_, err) = self.peek(cancel, 1);
            if let Some(err) = err {
                if matches!(err, PeekErr::Eof) && dst.len() != start {
                    return (dst, None);
                }
                return (dst, Some(err));
            }
            let mut chunk_len = self.buffered();
            let chunk = &self.buf[self.r..self.r + chunk_len];
            let terminator = chunk.iter().position(|&c| c == b'\r' || c == b'\n');
            if let Some(t) = terminator {
                chunk_len = t + 1;
            }
            if chunk_len as i64 > max_bytes - dst.len() as i64 {
                return (
                    dst,
                    Some(PeekErr::Err(Error::wrap(
                        Sentinel::Limit,
                        &format!("more than {max_bytes} bytes"),
                    ))),
                );
            }
            dst.extend_from_slice(&self.buf[self.r..self.r + chunk_len]);
            self.r += chunk_len;
            self.offset += chunk_len as i64;
            if terminator.is_some() {
                return (dst, None);
            }
        }
    }
}
fn is_eof(e: &Option<PeekErr>) -> bool {
    matches!(e, Some(PeekErr::Eof))
}
fn peek_error(e: PeekErr) -> Error {
    match e {
        PeekErr::Eof => Error::text("EOF"),
        PeekErr::Err(e) => e,
    }
}
pub fn parse_stream<R: Read>(
    cancel: &dyn Cancel,
    input: R,
    options: &ParseOptions,
    consume: &mut dyn FnMut(Message) -> Result<(), Error>,
) -> Result<(), Error> {
    let options = options.clone().with_defaults();
    let mut wire = Er7Input::new(input);
    let mut data: Vec<u8> = Vec::new();
    let mut start: i64 = 0;
    let mut index: i64 = 1;
    let fail = |index: i64, start: i64, err: Error| -> Error {
        Error::Stream(Box::new(StreamError {
            message: index,
            offset: start,
            err,
        }))
    };
    loop {
        let (name, err) = wire.peek_name(cancel);
        let eof = is_eof(&err);
        if let Some(e) = err
            && !eof
        {
            return Err(fail(index, start, peek_error(e)));
        }
        let mut next_message = false;
        if !data.is_empty() && name == "MSH" {
            let (prefix, err) = wire.peek(cancel, 4);
            if let Some(e) = err
                && !matches!(e, PeekErr::Eof)
            {
                return Err(fail(index, start, peek_error(e)));
            }
            if prefix.len() == 4 {
                let separator = prefix[3];
                next_message =
                    (0x21..=0x7e).contains(&separator) && !separator.is_ascii_alphanumeric();
            }
        }
        if !data.is_empty() && (next_message || eof) {
            let message = match parse_with_options(&data, &options) {
                Ok(m) => m,
                Err(e) => return Err(fail(index, start, e)),
            };
            if let Some(e) = cancel.err() {
                return Err(fail(index, start, e));
            }
            consume(message)?;
            data.clear();
            index += 1;
            start = wire.offset;
        }
        if eof {
            return match cancel.err() {
                Some(e) => Err(e),
                None => Ok(()),
            };
        }
        if data.is_empty() && name != "MSH" {
            return Err(fail(
                index,
                start,
                ParseError::new(Error::wrap(
                    Sentinel::Syntax,
                    "the message does not start with MSH",
                ))
                .into(),
            ));
        }
        let (d, err) = wire.read_segment(cancel, std::mem::take(&mut data), options.max_bytes);
        data = d;
        if let Some(e) = err {
            return Err(fail(index, start, peek_error(e)));
        }
    }
}
