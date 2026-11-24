use super::{ChainError, Sentinel};
pub type DataBinClass = u8;
pub const CLASS_PRECINCT: DataBinClass = 0;
pub const CLASS_EXTENDED_PRECINCT: DataBinClass = 1;
pub const CLASS_TILE_HEADER: DataBinClass = 2;
pub const CLASS_TILE: DataBinClass = 4;
pub const CLASS_EXTENDED_TILE: DataBinClass = 5;
pub const CLASS_MAIN_HEADER: DataBinClass = 6;
pub const CLASS_METADATA: DataBinClass = 8;
const MAX_SUPPORTED_DATA_BIN_CODE: u64 = 8;
pub type EorReason = u8;
pub const EOR_IMAGE_DONE: EorReason = 1;
pub const EOR_WINDOW_DONE: EorReason = 2;
pub const EOR_WINDOW_CHANGE: EorReason = 3;
pub const EOR_BYTE_LIMIT_REACHED: EorReason = 4;
pub const EOR_QUALITY_LIMIT_REACHED: EorReason = 5;
pub const EOR_SESSION_LIMIT_REACHED: EorReason = 6;
pub const EOR_RESPONSE_LIMIT_REACHED: EorReason = 7;
pub const EOR_NON_SPECIFIED: EorReason = 0xff;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub class: DataBinClass,
    pub stream: u64,
    pub id: u64,
    pub offset: u64,
    pub data: Vec<u8>,
    pub complete: bool,
    pub aux: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stream {
    pub messages: Vec<Message>,
    pub eor: EorReason,
    pub has_eor: bool,
}
const MAX_VBAS_BYTES: usize = 9;
pub(crate) fn append_vbas(out: &mut Vec<u8>, value: u64) {
    let mut n = 1;
    let mut v = value >> 7;
    while v != 0 {
        n += 1;
        v >>= 7;
    }
    for i in (0..n).rev() {
        let mut b = (value >> (7 * i)) as u8 & 0x7f;
        if i != 0 {
            b |= 0x80;
        }
        out.push(b);
    }
}
pub(crate) fn append_message_header(out: &mut Vec<u8>, m: &Message, length: usize) {
    let mut groups = 0u32;
    let mut v = m.id >> 4;
    while v != 0 {
        groups += 1;
        v >>= 7;
    }
    let mut first = (3u8 << 5) | ((m.id >> (7 * groups)) as u8 & 0x0f);
    if m.complete {
        first |= 0x10;
    }
    if groups > 0 {
        first |= 0x80;
    }
    out.push(first);
    for i in (0..groups).rev() {
        let mut b = (m.id >> (7 * i)) as u8 & 0x7f;
        if i != 0 {
            b |= 0x80;
        }
        out.push(b);
    }
    append_vbas(out, u64::from(m.class));
    append_vbas(out, m.stream);
    append_vbas(out, m.offset);
    append_vbas(out, length as u64);
    if m.class == CLASS_EXTENDED_PRECINCT || m.class == CLASS_EXTENDED_TILE {
        append_vbas(out, m.aux);
    }
}
pub fn append_message(out: &mut Vec<u8>, m: &Message) {
    append_message_header(out, m, m.data.len());
    out.extend_from_slice(&m.data);
}
pub fn append_eor(out: &mut Vec<u8>, reason: EorReason) {
    out.extend_from_slice(&[0x00, reason, 0x00]);
}
fn bad(detail: &str) -> ChainError {
    ChainError::wrap(Sentinel::MalformedStream, detail)
}
struct Reader<'a> {
    body: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn vbas(&mut self) -> Result<u64, ChainError> {
        let mut value = 0u64;
        let mut i = 0;
        loop {
            if self.at >= self.body.len() || i == MAX_VBAS_BYTES {
                return Err(bad("truncated or oversized VBAS"));
            }
            let b = self.body[self.at];
            self.at += 1;
            value = (value << 7) | u64::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(value);
            }
            i += 1;
        }
    }
    fn class(&mut self) -> Result<DataBinClass, ChainError> {
        let value = self.vbas()?;
        if value > MAX_SUPPORTED_DATA_BIN_CODE || value == 3 || value == 7 {
            return Err(bad("unknown data-bin class"));
        }
        Ok(value as u8)
    }
}
pub fn parse_stream(body: &[u8]) -> Result<Stream, ChainError> {
    let mut stream = Stream::default();
    let mut class: DataBinClass = 0;
    let mut csn = 0u64;
    let mut have_previous = false;
    let mut r = Reader { body, at: 0 };
    while r.at < body.len() {
        if stream.has_eor {
            return Err(bad("data after EOR"));
        }
        let first = body[r.at];
        if first == 0 {
            if r.at + 2 > body.len() {
                return Err(bad("truncated EOR"));
            }
            stream.eor = body[r.at + 1];
            r.at += 2;
            let length = r.vbas()?;
            if length > (body.len() - r.at) as u64 {
                return Err(bad("truncated EOR body"));
            }
            r.at += length as usize;
            stream.has_eor = true;
            continue;
        }
        r.at += 1;
        let indicator = (first >> 5) & 3;
        if indicator == 0 {
            return Err(bad("prohibited Bin-ID indicator"));
        }
        let mut m = Message {
            complete: first & 0x10 != 0,
            id: u64::from(first & 0x0f),
            ..Message::default()
        };
        let mut b = first;
        while b & 0x80 != 0 {
            if r.at >= body.len() || m.id >> (63 - 7) != 0 {
                return Err(bad("truncated or oversized Bin-ID"));
            }
            b = body[r.at];
            r.at += 1;
            m.id = (m.id << 7) | u64::from(b & 0x7f);
        }
        match indicator {
            1 => {
                if !have_previous {
                    return Err(bad("message omits class before any class was sent"));
                }
            }
            2 => {
                if !have_previous {
                    return Err(bad("message omits CSn before any CSn was sent"));
                }
                class = r.class()?;
            }
            _ => {
                class = r.class()?;
                csn = r.vbas()?;
            }
        }
        have_previous = true;
        m.class = class;
        m.stream = csn;
        let offset = r.vbas()?;
        let length = r.vbas()?;
        if class == CLASS_EXTENDED_PRECINCT || class == CLASS_EXTENDED_TILE {
            m.aux = r.vbas()?;
        }
        if length > (body.len() - r.at) as u64 {
            return Err(bad("truncated message body"));
        }
        m.offset = offset;
        let length = length as usize;
        m.data = body[r.at..r.at + length].to_vec();
        r.at += length;
        stream.messages.push(m);
    }
    Ok(stream)
}
