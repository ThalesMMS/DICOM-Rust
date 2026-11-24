use crate::object::error::{Error, Kind};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
pub trait ByteRead {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>);
    fn read_at(&self, _p: &mut [u8], _off: i64) -> Option<(usize, Option<Error>)> {
        None
    }
    fn seek_remaining(&self) -> Option<i64> {
        None
    }
    fn seek_forward(&mut self, _n: i64) -> bool {
        false
    }
}
pub fn read_full(r: &mut dyn ByteRead, buf: &mut [u8]) -> (usize, Option<Error>) {
    let mut n = 0;
    let mut err = None;
    while n < buf.len() && err.is_none() {
        let (nn, e) = r.read(buf.get_mut(n..).unwrap_or_default());
        n = n.saturating_add(nn).min(buf.len());
        err = e;
    }
    if n >= buf.len() {
        err = None;
    } else if n > 0 && err.as_ref().is_some_and(|e| e.is_exactly(Kind::Eof)) {
        err = Some(Error::unexpected_eof());
    }
    (n, err)
}
pub(crate) fn discard_n(r: &mut dyn ByteRead, n: i64) -> Option<Error> {
    let mut left = n;
    let mut buf = vec![0u8; n.clamp(0, 32 << 10) as usize];
    while left > 0 {
        let want = left.min(buf.len() as i64) as usize;
        let (got, err) = r.read(buf.get_mut(..want).unwrap_or_default());
        left = left.saturating_sub(got as i64);
        if left == 0 {
            return None;
        }
        if let Some(err) = err {
            return Some(err);
        }
    }
    None
}
#[derive(Clone)]
pub struct MemFile {
    data: Rc<[u8]>,
    pos: Rc<Cell<usize>>,
}
impl MemFile {
    pub fn new(data: Rc<[u8]>) -> MemFile {
        MemFile {
            data,
            pos: Rc::new(Cell::new(0)),
        }
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn position(&self) -> usize {
        self.pos.get()
    }
    pub fn seek(&self, pos: usize) {
        self.pos.set(pos);
    }
    pub fn remaining(&self) -> &[u8] {
        self.data.get(self.pos.get()..).unwrap_or_default()
    }
}
pub(crate) fn copy_prefix(dst: &mut [u8], src: &[u8]) -> usize {
    let n = dst.len().min(src.len());
    if let (Some(dst), Some(src)) = (dst.get_mut(..n), src.get(..n)) {
        dst.copy_from_slice(src);
    }
    n
}
impl ByteRead for MemFile {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        let pos = self.pos.get();
        let rest = match self.data.get(pos..) {
            Some(rest) if !rest.is_empty() => rest,
            _ => return (0, Some(Error::eof())),
        };
        let n = copy_prefix(p, rest);
        self.pos.set(pos.saturating_add(n));
        (n, None)
    }
    fn read_at(&self, p: &mut [u8], off: i64) -> Option<(usize, Option<Error>)> {
        if off < 0 {
            return Some((0, Some(Error::msg("read at a negative offset"))));
        }
        let rest = match usize::try_from(off)
            .ok()
            .and_then(|off| self.data.get(off..))
        {
            Some(rest) if !rest.is_empty() => rest,
            _ => return Some((0, Some(Error::eof()))),
        };
        let n = copy_prefix(p, rest);
        Some((
            n,
            if n < p.len() {
                Some(Error::eof())
            } else {
                None
            },
        ))
    }
    fn seek_remaining(&self) -> Option<i64> {
        Some(self.data.len().saturating_sub(self.pos.get()) as i64)
    }
    fn seek_forward(&mut self, n: i64) -> bool {
        self.pos
            .set((self.pos.get() as i64).saturating_add(n).max(0) as usize);
        true
    }
}
pub(crate) struct Limited {
    pub r: Box<dyn ByteRead>,
    pub n: i64,
}
impl ByteRead for Limited {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if self.n <= 0 {
            return (0, Some(Error::eof()));
        }
        let len = (p.len() as i64).min(self.n) as usize;
        let (n, err) = self.r.read(p.get_mut(..len).unwrap_or_default());
        self.n = self.n.saturating_sub(n as i64);
        (n, err)
    }
}
pub struct Shared<T: ByteRead>(pub Rc<RefCell<T>>);
impl<T: ByteRead> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}
impl<T: ByteRead> ByteRead for Shared<T> {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        self.0.borrow_mut().read(p)
    }
    fn read_at(&self, p: &mut [u8], off: i64) -> Option<(usize, Option<Error>)> {
        self.0.borrow().read_at(p, off)
    }
}
pub(crate) struct Counter {
    pub record_errors: bool,
    pub last_error: Option<Error>,
    pub first_failure: Option<Error>,
    pub r: Box<dyn ByteRead>,
    pub pos: i64,
    pub max_total_bytes: i64,
    pub fence: i64,
    pub fence_hit: bool,
    pub pending: Vec<u8>,
}
impl Counter {
    pub fn new(
        r: Box<dyn ByteRead>,
        pos: i64,
        max_total_bytes: i64,
        record_errors: bool,
    ) -> Counter {
        Counter {
            record_errors,
            last_error: None,
            first_failure: None,
            r,
            pos,
            max_total_bytes,
            fence: 0,
            fence_hit: false,
            pending: Vec::new(),
        }
    }
    pub fn unread(&mut self, b: &[u8]) {
        let mut pending = b.to_vec();
        pending.extend_from_slice(&self.pending);
        self.pending = pending;
        self.pos = self.pos.saturating_sub(b.len() as i64);
    }
    fn read_inner(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if !self.pending.is_empty() && !p.is_empty() {
            let mut n = p.len().min(self.pending.len());
            if self.fence > 0 && self.pos.saturating_add(n as i64) > self.fence {
                n = self.fence.saturating_sub(self.pos).max(0) as usize;
                if n == 0 {
                    self.fence_hit = true;
                    return (0, Some(Error::unexpected_eof()));
                }
            }
            let n = copy_prefix(p.get_mut(..n).unwrap_or_default(), &self.pending);
            self.pending.drain(..n);
            self.pos = self.pos.saturating_add(n as i64);
            return (n, None);
        }
        let mut p = p;
        if self.fence > 0 {
            let remaining = self.fence.saturating_sub(self.pos);
            if remaining <= 0 {
                self.fence_hit = true;
                return (0, Some(Error::unexpected_eof()));
            }
            if p.len() as i64 > remaining {
                p = p.get_mut(..remaining as usize).unwrap_or_default();
            }
        }
        if self.max_total_bytes > 0 {
            let remaining = self.max_total_bytes.saturating_sub(self.pos);
            if remaining <= 0 {
                return (0, Some(Error::sentinel(Kind::MaxTotalBytes)));
            }
            if p.len() as i64 > remaining {
                let p = p.get_mut(..remaining as usize).unwrap_or_default();
                let (n, err) = self.r.read(p);
                self.pos = self.pos.saturating_add(n as i64);
                if err.is_some() {
                    return (n, err);
                }
                if n as i64 == remaining {
                    return (n, Some(Error::sentinel(Kind::MaxTotalBytes)));
                }
                return (n, None);
            }
        }
        let (n, err) = self.r.read(p);
        self.pos = self.pos.saturating_add(n as i64);
        (n, err)
    }
}
impl ByteRead for Counter {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        let (n, err) = self.read_inner(p);
        if self.record_errors
            && let Some(e) = &err
        {
            self.last_error = Some(e.clone());
            if self.first_failure.is_none() && !terminal_eof_cause(e) {
                self.first_failure = Some(e.clone());
            }
        }
        (n, err)
    }
}
pub(crate) fn terminal_eof_cause(err: &Error) -> bool {
    err.is(Kind::Eof) || err.is(Kind::UnexpectedEof)
}
