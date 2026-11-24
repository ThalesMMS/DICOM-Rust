use crate::nifti::gzip::{Terminal, crc32_update};
use crate::object::error::Kind;
use crate::object::inflate::inflate_raw;
const GZIP_HEADER: &str = "gzip: invalid header";
pub(crate) struct Gunzipped {
    pub name: String,
    pub data: Vec<u8>,
    pub terminal: Terminal,
    pub attached: bool,
}
const WINDOW: usize = 1 << 15;
fn take<'a>(rest: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], Terminal> {
    if rest.len() - *at < n {
        return Err(Terminal::UnexpectedEof);
    }
    let out = &rest[*at..*at + n];
    *at += n;
    Ok(out)
}
fn header_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}
fn gzip_header(input: &[u8], pos: &mut usize) -> Result<String, Terminal> {
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
    let mut name = String::new();
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
                if flag == 8 {
                    name = header_string(&rest[at - count..at - 1]);
                }
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
    Ok(name)
}
pub(crate) fn gunzip(input: &[u8], multistream: bool) -> Result<Gunzipped, Terminal> {
    let mut pos = 0usize;
    let name = gzip_header(input, &mut pos)?;
    let mut data = Vec::new();
    let done = |data, terminal, last_member: usize| {
        Ok(Gunzipped {
            name: name.clone(),
            data,
            terminal,
            attached: !last_member.is_multiple_of(WINDOW),
        })
    };
    loop {
        let (member, err, consumed) = inflate_raw(&input[pos..]);
        pos += consumed;
        let terminal = if err.is_exactly(Kind::Eof) {
            Terminal::Eof
        } else if err.is_exactly(Kind::UnexpectedEof) {
            Terminal::UnexpectedEof
        } else {
            Terminal::Other(err.message().to_string())
        };
        let (crc, size) = (crc32_update(0, &member), member.len() as u32);
        data.extend_from_slice(&member);
        if terminal != Terminal::Eof {
            return done(data, terminal, member.len());
        }
        let rest = &input[pos..];
        if rest.len() < 8 {
            return done(data, Terminal::UnexpectedEof, member.len());
        }
        let stored_crc = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
        let stored_size = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
        pos += 8;
        if stored_crc != crc || stored_size != size {
            return done(
                data,
                Terminal::Other("gzip: invalid checksum".to_string()),
                member.len(),
            );
        }
        if !multistream {
            return done(data, Terminal::Eof, member.len());
        }
        if let Err(terminal) = gzip_header(input, &mut pos) {
            return done(data, terminal, member.len());
        }
    }
}
