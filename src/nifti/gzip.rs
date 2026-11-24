use crate::deflater;
use crate::object::error::Kind;
use crate::object::inflate::inflate_raw;
use std::sync::OnceLock;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    Eof,
    UnexpectedEof,
    Other(String),
}
impl Terminal {
    pub fn text(&self) -> &str {
        match self {
            Terminal::Eof => "EOF",
            Terminal::UnexpectedEof => "unexpected EOF",
            Terminal::Other(text) => text,
        }
    }
    pub fn is_eof_kind(&self) -> bool {
        !matches!(self, Terminal::Other(_))
    }
}
pub struct ByteStream {
    inner: Inner,
}
enum Inner {
    Eager {
        data: Vec<u8>,
        pos: usize,
        terminal: Terminal,
    },
    Bzip2(Box<super::bzip2::Reader>),
}
impl ByteStream {
    pub fn new(data: Vec<u8>, terminal: Terminal) -> ByteStream {
        ByteStream {
            inner: Inner::Eager {
                data,
                pos: 0,
                terminal,
            },
        }
    }
    pub fn plain(data: &[u8]) -> ByteStream {
        ByteStream::new(data.to_vec(), Terminal::Eof)
    }
    pub(crate) fn bzip2(data: &[u8]) -> ByteStream {
        ByteStream {
            inner: Inner::Bzip2(Box::new(super::bzip2::Reader::new(data.to_vec()))),
        }
    }
    pub fn rest(&self) -> &[u8] {
        match &self.inner {
            Inner::Eager { data, pos, .. } => &data[*pos..],
            Inner::Bzip2(_) => &[],
        }
    }
    pub fn read_full(&mut self, n: usize) -> Result<Vec<u8>, Terminal> {
        match &mut self.inner {
            Inner::Eager {
                data,
                pos,
                terminal,
            } => {
                let remaining = data.len() - *pos;
                if remaining >= n {
                    let out = data[*pos..*pos + n].to_vec();
                    *pos += n;
                    return Ok(out);
                }
                *pos = data.len();
                Err(match (&*terminal, remaining > 0) {
                    (Terminal::Eof, true) => Terminal::UnexpectedEof,
                    (terminal, _) => terminal.clone(),
                })
            }
            Inner::Bzip2(reader) => {
                let mut buf = vec![0u8; n];
                let mut got = 0;
                let mut err = None;
                while got < n && err.is_none() {
                    let (count, terminal) = reader.read(&mut buf[got..]);
                    got += count;
                    err = terminal;
                }
                if got >= n {
                    return Ok(buf);
                }
                Err(match err {
                    Some(Terminal::Eof) if got > 0 => Terminal::UnexpectedEof,
                    Some(terminal) => terminal,
                    None => Terminal::UnexpectedEof,
                })
            }
        }
    }
    pub fn discard(&mut self, n: u64) -> Result<(), Terminal> {
        match &mut self.inner {
            Inner::Eager {
                data,
                pos,
                terminal,
            } => {
                if (data.len() - *pos) as u64 >= n {
                    *pos += n as usize;
                    return Ok(());
                }
                *pos = data.len();
                Err(terminal.clone())
            }
            Inner::Bzip2(reader) => {
                let size = (32u64 * 1024).min(n.max(1)) as usize;
                let mut buf = vec![0u8; size];
                let mut written = 0u64;
                let mut failed = None;
                while written < n {
                    let want = (size as u64).min(n - written) as usize;
                    let (count, terminal) = reader.read(&mut buf[..want]);
                    written += count as u64;
                    if let Some(terminal) = terminal {
                        if terminal != Terminal::Eof {
                            failed = Some(terminal);
                        }
                        break;
                    }
                }
                if written == n {
                    return Ok(());
                }
                Err(failed.unwrap_or(Terminal::Eof))
            }
        }
    }
    pub(crate) fn take_limited(&mut self, n: u64) -> ByteStream {
        match &mut self.inner {
            Inner::Eager {
                data,
                pos,
                terminal,
            } => {
                if (data.len() - *pos) as u64 >= n {
                    let n = n as usize;
                    let out = data[*pos..*pos + n].to_vec();
                    *pos += n;
                    return ByteStream::new(out, Terminal::Eof);
                }
                let out = data[*pos..].to_vec();
                *pos = data.len();
                ByteStream::new(out, terminal.clone())
            }
            Inner::Bzip2(_) => ByteStream::new(Vec::new(), Terminal::Eof),
        }
    }
}
fn crc32_table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut crc = index as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    0xEDB8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
            *slot = crc;
        }
        table
    })
}
pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let table = crc32_table();
    let mut crc = !crc;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}
pub(crate) fn adler32_update(adler: u32, data: &[u8]) -> u32 {
    let (mut a, mut b) = (adler & 0xFFFF, adler >> 16);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}
fn flate_terminal(err: &crate::object::error::Error) -> Terminal {
    if err.is_exactly(Kind::Eof) {
        Terminal::Eof
    } else if err.is_exactly(Kind::UnexpectedEof) {
        Terminal::UnexpectedEof
    } else {
        Terminal::Other(err.message().to_string())
    }
}
const GZIP_HEADER: &str = "gzip: invalid header";
fn take<'a>(rest: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], Terminal> {
    if rest.len() - *at < n {
        return Err(Terminal::UnexpectedEof);
    }
    let out = &rest[*at..*at + n];
    *at += n;
    Ok(out)
}
fn gzip_header(input: &[u8], pos: &mut usize) -> Result<(), Terminal> {
    let rest = &input[*pos..];
    if rest.len() < 10 {
        return Err(if rest.is_empty() {
            Terminal::Eof
        } else {
            Terminal::UnexpectedEof
        });
    }
    if rest[0] != 0x1f || rest[1] != 0x8b || rest[2] != 8 {
        return Err(Terminal::Other(GZIP_HEADER.to_string()));
    }
    let flags = rest[3];
    let mut digest = crc32_update(0, &rest[..10]);
    let mut at = 10usize;
    if flags & 4 != 0 {
        let size = take(rest, &mut at, 2)?;
        digest = crc32_update(digest, size);
        let size = u16::from_le_bytes([size[0], size[1]]) as usize;
        let extra = take(rest, &mut at, size)?;
        digest = crc32_update(digest, extra);
    }
    for flag in [8u8, 16] {
        if flags & flag == 0 {
            continue;
        }
        let mut count = 0usize;
        loop {
            if count >= 512 {
                return Err(Terminal::Other(GZIP_HEADER.to_string()));
            }
            let byte = take(rest, &mut at, 1)?[0];
            count += 1;
            if byte == 0 {
                digest = crc32_update(digest, &rest[at - count..at]);
                break;
            }
        }
    }
    if flags & 2 != 0 {
        let stored = take(rest, &mut at, 2)?;
        if u16::from_le_bytes([stored[0], stored[1]]) != digest as u16 {
            return Err(Terminal::Other(GZIP_HEADER.to_string()));
        }
    }
    *pos += at;
    Ok(())
}
#[doc(hidden)]
pub fn gunzip(input: &[u8]) -> Result<ByteStream, Terminal> {
    let mut pos = 0usize;
    gzip_header(input, &mut pos)?;
    let mut out = Vec::new();
    loop {
        let (data, err, consumed) = inflate_raw(&input[pos..]);
        pos += consumed;
        let terminal = flate_terminal(&err);
        let (crc, size) = (crc32_update(0, &data), data.len() as u32);
        out.extend_from_slice(&data);
        if terminal != Terminal::Eof {
            return Ok(ByteStream::new(out, terminal));
        }
        let rest = &input[pos..];
        if rest.len() < 8 {
            return Ok(ByteStream::new(out, Terminal::UnexpectedEof));
        }
        let stored_crc = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
        let stored_size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
        pos += 8;
        if stored_crc != crc || stored_size != size {
            return Ok(ByteStream::new(
                out,
                Terminal::Other("gzip: invalid checksum".to_string()),
            ));
        }
        if let Err(terminal) = gzip_header(input, &mut pos) {
            return Ok(ByteStream::new(out, terminal));
        }
    }
}
pub(crate) fn zlib_stream(input: &[u8]) -> Result<ByteStream, Terminal> {
    if input.len() < 2 {
        return Err(Terminal::UnexpectedEof);
    }
    let (cmf, flg) = (input[0], input[1]);
    let header = (cmf as u16) << 8 | flg as u16;
    if cmf & 0x0F != 8 || cmf >> 4 > 7 || !header.is_multiple_of(31) {
        return Err(Terminal::Other("zlib: invalid header".to_string()));
    }
    if flg & 0x20 != 0 {
        if input.len() < 6 {
            return Err(Terminal::UnexpectedEof);
        }
        return Err(Terminal::Other("zlib: invalid dictionary".to_string()));
    }
    let (data, err, consumed) = inflate_raw(&input[2..]);
    let terminal = flate_terminal(&err);
    if terminal != Terminal::Eof {
        return Ok(ByteStream::new(data, terminal));
    }
    let rest = &input[2 + consumed..];
    if rest.len() < 4 {
        return Ok(ByteStream::new(data, Terminal::UnexpectedEof));
    }
    let stored = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
    if stored != adler32_update(1, &data) {
        return Ok(ByteStream::new(
            data,
            Terminal::Other("zlib: invalid checksum".to_string()),
        ));
    }
    Ok(ByteStream::new(data, Terminal::Eof))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Gzip,
    Zlib,
}
pub(crate) struct DeflateWriter {
    container: Container,
    flate: deflater::Writer,
    digest: u32,
    size: u32,
}
impl DeflateWriter {
    pub fn new(container: Container) -> DeflateWriter {
        DeflateWriter {
            container,
            flate: deflater::Writer::new(deflater::DEFAULT_COMPRESSION)
                .expect("default deflate level"),
            digest: if container == Container::Zlib { 1 } else { 0 },
            size: 0,
        }
    }
    pub fn write(&mut self, data: &[u8]) {
        match self.container {
            Container::Gzip => self.digest = crc32_update(self.digest, data),
            Container::Zlib => self.digest = adler32_update(self.digest, data),
        }
        self.size = self.size.wrapping_add(data.len() as u32);
        self.flate.write(data).expect("open deflate writer");
    }
    pub fn finish(mut self) -> Vec<u8> {
        self.flate.close();
        let body = self.flate.take_output();
        let mut out = Vec::with_capacity(body.len() + 18);
        match self.container {
            Container::Gzip => {
                out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff]);
                out.extend_from_slice(&body);
                out.extend_from_slice(&self.digest.to_le_bytes());
                out.extend_from_slice(&self.size.to_le_bytes());
            }
            Container::Zlib => {
                out.extend_from_slice(&[0x78, 0x9c]);
                out.extend_from_slice(&body);
                out.extend_from_slice(&self.digest.to_be_bytes());
            }
        }
        out
    }
}
