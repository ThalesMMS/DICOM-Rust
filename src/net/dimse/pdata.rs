#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::error::{Error, Kind, Result};
use crate::net::ul::{self, Association, Ctx, Pdu};
use crate::object::parser::source::{ByteRead, Shared};
use crate::object::{self, Object, ReadFileOptions, Syntax};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
pub const MAX_COMMAND_SET_BYTES: usize = 4 << 20;
pub(crate) const MIN_REJECTED_DATA_SET_DRAIN_BYTES: i64 = 1 << 30;
pub struct PDataWriter<'a> {
    assoc: Option<&'a Association>,
    ctx: Option<Ctx>,
    pc_id: u8,
    is_command: bool,
    max_pdu: i64,
    pub(crate) buf: Vec<u8>,
    pub(crate) written: i64,
    finished: bool,
    pub(crate) command_prefix: Option<Vec<u8>>,
    pub(crate) command_sent: Option<Box<dyn FnOnce() + 'a>>,
    pub(crate) first_context: Option<Ctx>,
}
impl<'a> PDataWriter<'a> {
    pub fn new(
        ctx: Option<Ctx>,
        assoc: Option<&'a Association>,
        pc_id: u8,
        is_command: bool,
        max_pdu: i64,
    ) -> PDataWriter<'a> {
        PDataWriter {
            assoc,
            ctx,
            pc_id,
            is_command,
            max_pdu,
            buf: Vec::new(),
            written: 0,
            finished: false,
            command_prefix: None,
            command_sent: None,
            first_context: None,
        }
    }
    pub fn write(&mut self, mut p: &[u8]) -> Result<usize> {
        if self.finished {
            return Err(Error::new("dicom dimse: write after P-DATA finish"));
        }
        let mut written = 0usize;
        while !p.is_empty() {
            let payload_max = self.max_payload();
            let mut space = (payload_max as i64).saturating_sub(self.buf.len() as i64);
            if space <= 0 {
                self.flush(false)?;
                space = self.max_payload() as i64;
            }
            let (chunk, rest) = p.split_at((space as usize).min(p.len()));
            self.buf.extend_from_slice(chunk);
            p = rest;
            written = written.saturating_add(chunk.len());
            self.written = self.written.saturating_add(chunk.len() as i64);
        }
        Ok(written)
    }
    pub(crate) fn finish_data_set(&mut self, syntax: &Syntax) -> Result<()> {
        if !self.finished && syntax.deflated && self.written % 2 != 0 {
            self.write(&[0])?;
        }
        self.finish()
    }
    pub fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let r = self.flush(true);
        self.discard();
        r
    }
    pub fn discard(&mut self) {
        self.buf = Vec::new();
        self.command_prefix = None;
        self.command_sent = None;
        self.first_context = None;
        self.finished = true;
    }
    fn flush(&mut self, is_last: bool) -> Result<()> {
        let Some(assoc) = self.assoc else {
            return Err(Error::new("dicom dimse: nil association"));
        };
        if !is_last && self.buf.is_empty() {
            return Ok(());
        }
        let value = ul::PDataValue {
            presentation_context_id: self.pc_id,
            is_command: self.is_command,
            is_last,
            data: std::mem::take(&mut self.buf),
        };
        let mut ctx = self.ctx.clone();
        let values = if let Some(prefix) = self.command_prefix.take() {
            ctx = self.first_context.clone();
            vec![
                ul::PDataValue {
                    presentation_context_id: self.pc_id,
                    is_command: true,
                    is_last: true,
                    data: prefix,
                },
                value,
            ]
        } else {
            vec![value]
        };
        let had_prefix = values.len() == 2;
        let pdu = Pdu::PData(ul::PDataTf { values });
        let r = match &ctx {
            Some(c) => assoc.send(Some(c), &pdu),
            None => assoc.write_pdu(&pdu),
        };
        if had_prefix {
            if r.is_ok()
                && let Some(sent) = self.command_sent.take()
            {
                sent();
            }
            self.command_sent = None;
            self.first_context = None;
        }
        r.map_err(Error::from)
    }
    fn max_payload(&self) -> usize {
        let mut max_pdu = self.max_pdu;
        if max_pdu <= 0 {
            max_pdu = (ul::DEFAULT_MAX_PDU + ul::PDU_HEADER_SIZE) as i64;
        }
        let mut payload_max =
            max_pdu.saturating_sub(i64::from(ul::PDU_HEADER_SIZE + ul::PDV_HEADER_SIZE));
        if let Some(prefix) = &self.command_prefix {
            payload_max = payload_max
                .saturating_sub(i64::from(ul::PDV_HEADER_SIZE).saturating_add(prefix.len() as i64));
        }
        if payload_max < 1 {
            return 1;
        }
        if payload_max > 1 {
            payload_max &= !1;
        }
        payload_max as usize
    }
}
pub(crate) fn peer_max_pdu_with_header(assoc: &Association) -> i64 {
    i64::from(assoc.send_pdu_limit()).saturating_add(i64::from(ul::PDU_HEADER_SIZE))
}
pub struct PDataReader<'a> {
    assoc: Option<&'a Association>,
    ctx: Option<Ctx>,
    pc_id: u8,
    expected_type: Option<bool>,
    fragments: VecDeque<Vec<u8>>,
    offset: usize,
    last_received: bool,
    pub(crate) received_bytes: usize,
    last_err: Option<Error>,
}
impl<'a> PDataReader<'a> {
    pub fn new(assoc: Option<&'a Association>, pc_id: u8) -> PDataReader<'a> {
        PDataReader {
            assoc,
            ctx: None,
            pc_id,
            expected_type: None,
            fragments: VecDeque::new(),
            offset: 0,
            last_received: false,
            received_bytes: 0,
            last_err: None,
        }
    }
    pub fn typed(
        ctx: Option<Ctx>,
        assoc: Option<&'a Association>,
        pc_id: u8,
        is_command: bool,
    ) -> PDataReader<'a> {
        let mut r = PDataReader::new(assoc, pc_id);
        r.ctx = ctx;
        r.expected_type = Some(is_command);
        r
    }
    pub fn with_prefix(
        assoc: Option<&'a Association>,
        pc_id: u8,
        prefix: Vec<u8>,
        last: bool,
    ) -> PDataReader<'a> {
        let mut r = PDataReader::typed(None, assoc, pc_id, false);
        r.received_bytes = prefix.len();
        r.append_fragment(prefix);
        r.last_received = last;
        r
    }
    pub(crate) fn received_bytes(&self) -> usize {
        self.received_bytes
    }
    fn buffered_len(&self) -> usize {
        self.fragments
            .iter()
            .map(Vec::len)
            .fold(0usize, usize::saturating_add)
            .saturating_sub(self.offset)
    }
    fn append_fragment(&mut self, fragment: Vec<u8>) {
        if !fragment.is_empty() {
            self.fragments.push_back(fragment);
        }
    }
    pub fn read_bytes(&mut self, p: &mut [u8]) -> Result<usize> {
        if p.is_empty() {
            return Ok(0);
        }
        while self.buffered_len() == 0 && !self.last_received {
            if let Err(err) = self.read_next_pdata() {
                if err.is_ul(ul::Kind::Eof) {
                    return Err(Error::bare_with(
                        format!(
                            "dicom dimse: association ended before the last P-DATA fragment: unexpected EOF ({})",
                            err.text()
                        ),
                        vec![super::error::Cause::Ul(ul::Kind::UnexpectedEof)],
                    ));
                }
                return Err(err);
            }
        }
        if self.buffered_len() == 0 {
            return Ok(0);
        }
        let mut n = 0;
        while n < p.len() {
            let Some(front) = self.fragments.front() else {
                break;
            };
            let avail = front.get(self.offset..).unwrap_or_default();
            let take =
                object::parser::source::copy_prefix(p.get_mut(n..).unwrap_or_default(), avail);
            n = n.saturating_add(take);
            self.offset = self.offset.saturating_add(take);
            if self.offset == front.len() {
                self.fragments.pop_front();
                self.offset = 0;
            }
        }
        Ok(n)
    }
    pub fn read_to_end(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        loop {
            let n = self.read_bytes(&mut buf)?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(buf.get(..n).unwrap_or_default());
        }
    }
    fn read_next_pdata(&mut self) -> Result<()> {
        let values = receive_pdata_values(self.ctx.as_ref(), self.assoc)?;
        self.consume_pdata_values(values)?;
        if !self.last_received {
            let base = match (&self.ctx, self.assoc) {
                (Some(c), _) => c.clone(),
                (None, Some(a)) => a.context.clone(),
                (None, None) => Ctx::background(),
            };
            self.ctx = Some(base.with_active_transfer());
        }
        Ok(())
    }
    fn consume_pdata_values(&mut self, values: Vec<ul::PDataValue>) -> Result<()> {
        let mut values = values.into_iter();
        while let Some(value) = values.next() {
            if self.last_received {
                let mut rest = vec![value];
                rest.extend(values);
                return store_carryover(self.ctx.as_ref(), self.assoc, rest);
            }
            if value.presentation_context_id != self.pc_id {
                return Err(Error::wrap(
                    Kind::PresentationContextMismatch,
                    format!("got {}, want {}", value.presentation_context_id, self.pc_id),
                ));
            }
            if let Some(expected) = self.expected_type {
                if value.is_command != expected {
                    return Err(Error::new(format!(
                        "dicom dimse: P-DATA command flag {}, want {expected}",
                        value.is_command
                    )));
                }
                if expected {
                    check_command_set_size(self.received_bytes, value.data.len())?;
                }
            }
            self.received_bytes = self.received_bytes.saturating_add(value.data.len());
            let last = value.is_last;
            self.append_fragment(value.data);
            if last {
                self.last_received = true;
                return store_carryover(self.ctx.as_ref(), self.assoc, values.collect());
            }
        }
        Ok(())
    }
}
impl ByteRead for PDataReader<'_> {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<object::Error>) {
        match self.read_bytes(p) {
            Ok(0) if !p.is_empty() => (0, Some(object::Error::eof())),
            Ok(n) => (n, None),
            Err(err) => {
                let obj = if err.is_ul(ul::Kind::UnexpectedEof) {
                    object::Error::relabel(err.text(), object::Error::unexpected_eof())
                } else {
                    object::Error::msg(err.text())
                };
                self.last_err = Some(err);
                (0, Some(obj))
            }
        }
    }
}
pub(crate) fn check_command_set_size(current: usize, additional: usize) -> Result<()> {
    if current > MAX_COMMAND_SET_BYTES || additional > MAX_COMMAND_SET_BYTES.saturating_sub(current)
    {
        return Err(Error::wrap(
            Kind::CommandSetTooLarge,
            format!(
                "{current} existing byte(s) plus {additional} incoming byte(s) exceeds {MAX_COMMAND_SET_BYTES}"
            ),
        ));
    }
    Ok(())
}
pub(crate) fn append_command_set_fragment(buf: &mut Vec<u8>, fragment: &[u8]) -> Result<()> {
    check_command_set_size(buf.len(), fragment.len())?;
    buf.extend_from_slice(fragment);
    Ok(())
}
pub(crate) fn receive_pdata_values(
    ctx: Option<&Ctx>,
    assoc: Option<&Association>,
) -> Result<Vec<ul::PDataValue>> {
    let Some(assoc) = assoc else {
        return Err(Error::new("dicom dimse: nil association"));
    };
    let values = assoc.take_pdata_carryover(ctx)?;
    if !values.is_empty() {
        return Ok(values);
    }
    let pdu = match ctx {
        Some(c) => assoc.receive(Some(c))?,
        None => assoc.read_pdu()?,
    };
    match pdu {
        Pdu::PData(p) => Ok(p.values),
        other => Err(ul::unexpected_pdu_error(&other, "waiting for P-DATA-TF").into()),
    }
}
pub(crate) fn store_carryover(
    ctx: Option<&Ctx>,
    assoc: Option<&Association>,
    values: Vec<ul::PDataValue>,
) -> Result<()> {
    match assoc {
        None => Ok(()),
        Some(a) => a.store_pdata_carryover(ctx, &values).map_err(Error::from),
    }
}
pub(crate) fn drain_pdata_reader_with_budget(
    reader: &mut PDataReader<'_>,
    budget: i64,
) -> Result<()> {
    let mut left = budget.saturating_add(1);
    let mut buf = vec![0u8; 32 << 10];
    while left > 0 {
        let want = (left as usize).min(buf.len());
        let n = reader.read_bytes(buf.get_mut(..want).unwrap_or_default())?;
        if n == 0 {
            return Ok(());
        }
        left = left.saturating_sub(n as i64);
    }
    Err(Error::sentinel(Kind::RejectedDataSetDrainLimit))
}
pub(crate) fn drain_pdata_reader(reader: &mut PDataReader<'_>) -> Result<()> {
    let budget = (reader.received_bytes as i64).max(MIN_REJECTED_DATA_SET_DRAIN_BYTES);
    drain_pdata_reader_with_budget(reader, budget)
}
#[allow(unsafe_code)]
pub fn read_pdata_data_set(
    reader: &mut PDataReader<'_>,
    syntax: Syntax,
    opts: ReadFileOptions,
) -> Result<Object> {
    let taken = std::mem::replace(reader, PDataReader::new(None, 0));
    let shared = Rc::new(RefCell::new(taken));
    let result = {
        let source: Box<dyn ByteRead + '_> = Box::new(Shared(shared.clone()));
        let source: Box<dyn ByteRead + 'static> = unsafe { std::mem::transmute(source) };
        object::read_data_set_with_options(source, syntax, opts)
    };
    let mut restored = match Rc::try_unwrap(shared) {
        Ok(cell) => cell.into_inner(),
        Err(_) => unreachable!("the parser released its source"),
    };
    let transport = restored.last_err.take();
    *reader = restored;
    match result {
        Ok(obj) => {
            if syntax.deflated
                && let Err(err) = drain_pdata_reader(reader)
            {
                return Err(err.prefixed("dicom dimse: read past the end of the deflated data set"));
            }
            Ok(obj)
        }
        Err(err) => {
            let mut e = Error::from(err);
            if let Some(t) = transport {
                for c in t.causes() {
                    e = e.also_cause(*c);
                }
            }
            Err(e)
        }
    }
}
const _: () = {
    const fn send<T: Send>() {}
    send::<Object>();
    send::<object::Error>();
};
pub const INTERLEAVED_WIRE_LIMIT_TEXT: &str =
    "dicom dimse: interleaved event wire dataset limit exceeded";
struct WireLimited<R> {
    inner: R,
    remaining: i64,
}
impl<R: ByteRead> ByteRead for WireLimited<R> {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<object::Error>) {
        if p.is_empty() {
            return (0, None);
        }
        let mut len = p.len();
        if self.remaining < (len as i64).saturating_sub(1) {
            len = self.remaining.saturating_add(1) as usize;
        }
        let (n, err) = self.inner.read(p.get_mut(..len).unwrap_or_default());
        if n as i64 > self.remaining {
            self.remaining = 0;
            return (0, Some(object::Error::msg(INTERLEAVED_WIRE_LIMIT_TEXT)));
        }
        self.remaining = self.remaining.saturating_sub(n as i64);
        (n, err)
    }
}
#[allow(unsafe_code)]
pub(crate) fn read_pdata_data_set_wire_limited(
    reader: &mut PDataReader<'_>,
    syntax: Syntax,
    opts: ReadFileOptions,
    limit: i64,
) -> Result<Object> {
    let taken = std::mem::replace(reader, PDataReader::new(None, 0));
    let shared = Rc::new(RefCell::new(taken));
    let limited = Rc::new(RefCell::new(WireLimited {
        inner: Shared(shared.clone()),
        remaining: limit,
    }));
    let result = {
        let source: Box<dyn ByteRead + '_> = Box::new(Shared(limited.clone()));
        let source: Box<dyn ByteRead + 'static> = unsafe { std::mem::transmute(source) };
        object::read_data_set_with_options(source, syntax, opts)
    };
    let drained = match &result {
        Ok(_) => {
            let mut buf = vec![0u8; 32 << 10];
            let mut out = Ok(());
            loop {
                let (n, err) = limited.borrow_mut().read(&mut buf);
                match err {
                    Some(e) if e.is(object::Kind::Eof) => break,
                    Some(e) => {
                        out = Err(e);
                        break;
                    }
                    None if n == 0 => break,
                    None => {}
                }
            }
            out
        }
        Err(_) => Ok(()),
    };
    drop(limited);
    let mut restored = match Rc::try_unwrap(shared) {
        Ok(cell) => cell.into_inner(),
        Err(_) => unreachable!("the parser released its source"),
    };
    let transport = restored.last_err.take();
    *reader = restored;
    let attach = |err: object::Error| {
        let mut e = Error::from(err);
        if let Some(t) = &transport {
            for c in t.causes() {
                e = e.also_cause(*c);
            }
        }
        e
    };
    match result {
        Ok(obj) => match drained {
            Ok(()) => Ok(obj),
            Err(e) => Err(attach(e)),
        },
        Err(err) => Err(attach(err)),
    }
}
#[cfg(test)]
mod tests;
