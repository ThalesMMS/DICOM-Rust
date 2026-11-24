#![forbid(unsafe_code)]
use super::deferred::{self, DeferOptions};
use super::error::Error;
use super::file::{self, File, ReadFileOptions};
use super::read_ext::{self, ReadExtras};
use super::transfer::Syntax;
use super::types::{Element, ElementHeader, Tag, UNDEFINED_LENGTH, Value};
use super::writer::{self, WriteFileOptions};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
pub const ERR_REWRITE_SYNTAX_CHANGED: &str =
    "dicom: the transfer syntax of a file opened for rewrite changed; read it whole to transcode";
pub const DEFAULT_PROBE_BYTES: usize = 1 << 20;
const COPY_BYTES: usize = 64 << 10;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RewriteOptions {
    pub read: ReadFileOptions,
    pub probe_bytes: usize,
}
#[derive(Clone, Debug)]
struct PixelSpan {
    header: ElementHeader,
    standin: Element,
    start: u64,
    end: u64,
}
#[derive(Debug)]
pub struct RewriteFile {
    pub file: File,
    path: String,
    syntax: Syntax,
    span: Option<PixelSpan>,
}
#[derive(Debug)]
pub enum RewriteError {
    Dicom(Error),
    Io(io::Error),
}
impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RewriteError::Dicom(e) => e.fmt(f),
            RewriteError::Io(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for RewriteError {}
impl From<Error> for RewriteError {
    fn from(e: Error) -> RewriteError {
        RewriteError::Dicom(e)
    }
}
impl From<io::Error> for RewriteError {
    fn from(e: io::Error) -> RewriteError {
        RewriteError::Io(e)
    }
}
fn defer_pixel_data() -> DeferOptions {
    DeferOptions {
        defer_pixel_data: true,
        ..DeferOptions::default()
    }
}
fn header_length(syntax: &Syntax, header: &ElementHeader) -> Option<u64> {
    if !syntax.explicit_vr {
        return Some(8);
    }
    header.vr.uses_long_explicit_length().then_some(12)
}
fn tag_bytes(syntax: &Syntax, tag: Tag) -> [u8; 4] {
    let (g, e) = (tag.group, tag.element);
    if syntax.big_endian {
        [(g >> 8) as u8, g as u8, (e >> 8) as u8, e as u8]
    } else {
        [g as u8, (g >> 8) as u8, e as u8, (e >> 8) as u8]
    }
}
fn length_bytes(syntax: &Syntax, length: u32) -> [u8; 4] {
    if syntax.big_endian {
        length.to_be_bytes()
    } else {
        length.to_le_bytes()
    }
}
fn read_u32(syntax: &Syntax, b: &[u8]) -> u32 {
    let b = [b[0], b[1], b[2], b[3]];
    if syntax.big_endian {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}
fn encapsulated_standin(syntax: &Syntax) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&tag_bytes(syntax, Tag::new(0xFFFE, 0xE000)));
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&tag_bytes(syntax, Tag::new(0xFFFE, 0xE0DD)));
    out.extend_from_slice(&[0; 4]);
    out
}
fn read_at(source: &mut fs::File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    source.seek(SeekFrom::Start(offset))?;
    source.read_exact(buf)
}
fn encapsulated_end(
    source: &mut fs::File,
    syntax: &Syntax,
    start: u64,
    size: u64,
) -> io::Result<Option<u64>> {
    let item = tag_bytes(syntax, Tag::new(0xFFFE, 0xE000));
    let delimiter = tag_bytes(syntax, Tag::new(0xFFFE, 0xE0DD));
    let mut at = start;
    loop {
        if at + 8 > size {
            return Ok(None);
        }
        let mut head = [0u8; 8];
        read_at(source, at, &mut head)?;
        let length = read_u32(syntax, &head[4..]);
        if head[..4] == delimiter {
            return Ok((length == 0).then_some(at + 8));
        }
        if head[..4] != item || length == UNDEFINED_LENGTH {
            return Ok(None);
        }
        at += 8 + u64::from(length);
    }
}
fn pixel_data_header(chunk: &[u8]) -> Option<(Syntax, ElementHeader, u64)> {
    let extras = ReadExtras {
        stop_before_tag: Some(Tag::PIXEL_DATA),
        ..ReadExtras::default()
    };
    let read = match read_ext::read_file_extended(chunk, ReadFileOptions::default(), &extras) {
        Ok(read) => read,
        Err((Some(read), err)) if read_ext::is_read_stopped(&err) => *read,
        Err(_) => return None,
    };
    let stop = read.stop?;
    let offset = u64::try_from(stop.offset).ok()?;
    Some((read.file.transfer_syntax, stop.header, offset))
}
fn open_streaming(path: &str, probe: usize) -> io::Result<Option<RewriteFile>> {
    let mut source = fs::File::open(path)?;
    let size = source.metadata()?.len();
    let mut want = probe.max(256) as u64;
    let (chunk, syntax, header, offset) = loop {
        let n = want.min(size) as usize;
        let mut chunk = vec![0u8; n];
        read_at(&mut source, 0, &mut chunk)?;
        if let Some((syntax, header, offset)) = pixel_data_header(&chunk) {
            break (chunk, syntax, header, offset);
        }
        if n as u64 >= size {
            return Ok(None);
        }
        want = want.saturating_mul(4);
    };
    if header.tag != Tag::PIXEL_DATA || syntax.deflated || !header.length_set {
        return Ok(None);
    }
    let Some(header_len) = header_length(&syntax, &header) else {
        return Ok(None);
    };
    let start = offset + header_len;
    if start > size || chunk.len() < start as usize {
        return Ok(None);
    }
    let encoded = &chunk[offset as usize..start as usize];
    if encoded[..4] != tag_bytes(&syntax, Tag::PIXEL_DATA)
        || read_u32(&syntax, &encoded[encoded.len() - 4..]) != header.length
    {
        return Ok(None);
    }
    let (end, standin_value) = if header.is_undefined() {
        if !syntax.encapsulated {
            return Ok(None);
        }
        match encapsulated_end(&mut source, &syntax, start, size)? {
            Some(end) => (end, encapsulated_standin(&syntax)),
            None => return Ok(None),
        }
    } else {
        let length = u64::from(header.length);
        if syntax.encapsulated || length % 2 != 0 || start + length > size {
            return Ok(None);
        }
        (start + length, Vec::new())
    };
    let mut synthetic = Vec::with_capacity(start as usize + standin_value.len());
    synthetic.extend_from_slice(&chunk[..start as usize]);
    drop(chunk);
    if !header.is_undefined() {
        let at = synthetic.len() - 4;
        synthetic[at..].fill(0);
    }
    synthetic.extend_from_slice(&standin_value);
    let mut trailer = vec![0u8; (size - end) as usize];
    read_at(&mut source, end, &mut trailer)?;
    synthetic.extend_from_slice(&trailer);
    drop(trailer);
    let Ok(mut file) = file::read_file_with_options(&synthetic, ReadFileOptions::default()) else {
        return Ok(None);
    };
    if file.transfer_syntax.uid != syntax.uid
        || file.dataset.has_duplicate_parsed_tag(Tag::PIXEL_DATA)
        || !file.dataset.parsed_pixel_data
    {
        return Ok(None);
    }
    let Some(standin) = file.dataset.get(Tag::PIXEL_DATA).cloned() else {
        return Ok(None);
    };
    let deferred = Element {
        header,
        value: Value::Nil,
    };
    if !file.dataset.set_element_in_place(deferred) {
        return Ok(None);
    }
    Ok(Some(RewriteFile {
        file,
        path: path.to_string(),
        syntax,
        span: Some(PixelSpan {
            header,
            standin,
            start,
            end,
        }),
    }))
}
#[doc(hidden)]
pub fn open_file_for_rewrite(path: &str, opts: RewriteOptions) -> Result<RewriteFile, Error> {
    let probe = if opts.probe_bytes == 0 {
        DEFAULT_PROBE_BYTES
    } else {
        opts.probe_bytes
    };
    if opts.read == ReadFileOptions::default()
        && let Ok(Some(streaming)) = open_streaming(path, probe)
    {
        return Ok(streaming);
    }
    let data = deferred::open_source_file(path)?;
    let file = deferred::read_file_deferred(&data, true, opts.read, &defer_pixel_data())
        .map_err(|(_, err)| err)?;
    Ok(RewriteFile {
        syntax: file.transfer_syntax,
        file,
        path: path.to_string(),
        span: None,
    })
}
impl RewriteFile {
    pub fn is_streaming(&self) -> bool {
        self.span.is_some()
    }
    #[doc(hidden)]
    pub fn pixel_data_range(&self) -> Option<(u64, u64)> {
        self.span.as_ref().map(|s| (s.start, s.end))
    }
    fn pixel_data_untouched(&self, span: &PixelSpan) -> bool {
        self.file.dataset.parsed_pixel_data
            && self
                .file
                .dataset
                .get(Tag::PIXEL_DATA)
                .is_some_and(|e| e.header == span.header && matches!(e.value, Value::Nil))
    }
    #[doc(hidden)]
    pub fn write_to<W: Write>(
        &self,
        w: &mut W,
        opts: &WriteFileOptions,
    ) -> Result<u64, RewriteError> {
        let span = match &self.span {
            Some(span) if self.pixel_data_untouched(span) => span,
            _ => {
                let out = writer::write_file_with_options(&self.file, opts)?;
                w.write_all(&out)?;
                return Ok(out.len() as u64);
            }
        };
        let mut file = self.file.clone();
        file.dataset.set_element_in_place(span.standin.clone());
        let out = writer::write_file_with_options(&file, opts)?;
        drop(file);
        let changed = || RewriteError::Dicom(Error::msg(ERR_REWRITE_SYNTAX_CHANGED));
        let (syntax, header, offset) = pixel_data_header(&out).ok_or_else(changed)?;
        let header_len = header_length(&syntax, &header).ok_or_else(changed)?;
        let value_at = (offset + header_len) as usize;
        if syntax.uid != self.syntax.uid
            || header.tag != Tag::PIXEL_DATA
            || header.vr != span.header.vr
            || header.is_undefined() != span.header.is_undefined()
            || out.len() < value_at
        {
            return Err(changed());
        }
        let after = if header.is_undefined() {
            let standin = encapsulated_standin(&syntax);
            if out.len() < value_at + standin.len()
                || out[value_at..value_at + standin.len()] != standin[..]
            {
                return Err(changed());
            }
            w.write_all(&out[..value_at])?;
            value_at + standin.len()
        } else {
            if header.length != 0 {
                return Err(changed());
            }
            w.write_all(&out[..value_at - 4])?;
            w.write_all(&length_bytes(&syntax, span.header.length))?;
            value_at
        };
        let mut source = fs::File::open(&self.path)?;
        source.seek(SeekFrom::Start(span.start))?;
        let mut left = span.end - span.start;
        let mut buf = vec![0u8; COPY_BYTES.min(left as usize).max(1)];
        while left > 0 {
            let n = buf.len().min(left as usize);
            source.read_exact(&mut buf[..n])?;
            w.write_all(&buf[..n])?;
            left -= n as u64;
        }
        w.write_all(&out[after..])?;
        Ok(value_at as u64 + (span.end - span.start) + (out.len() - after) as u64)
    }
    #[doc(hidden)]
    pub fn copy_pixel_data_to<W: Write>(&self, w: &mut W) -> Result<u64, RewriteError> {
        let span = match &self.span {
            Some(span) if self.pixel_data_untouched(span) => span,
            _ => {
                let mut out = Vec::new();
                let (n, err) = self.file.dataset.copy_value_to(Tag::PIXEL_DATA, &mut out);
                if let Some(err) = err {
                    return Err(RewriteError::Dicom(err));
                }
                w.write_all(&out)?;
                return Ok(n as u64);
            }
        };
        let mut source = fs::File::open(&self.path)?;
        source.seek(SeekFrom::Start(span.start))?;
        Ok(io::copy(&mut source.take(span.end - span.start), w)?)
    }
}
