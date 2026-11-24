use super::{Error, Options};
use crate::numtext::{parse_float, parse_int};
pub(crate) const CLASS_UNIVERSAL: u8 = 0x00;
pub(crate) const CLASS_APPLICATION: u8 = 0x40;
pub(crate) const CLASS_CONTEXT: u8 = 0x80;
const TAG_BOOLEAN: u64 = 1;
const TAG_INTEGER: u64 = 2;
const TAG_OCTET_STRING: u64 = 4;
const TAG_REAL: u64 = 9;
const TAG_ENUMERATED: u64 = 10;
const TAG_UTF8: u64 = 12;
const TAG_SEQUENCE: u64 = 16;
const TAG_SET: u64 = 17;
const TAG_PRINTABLE: u64 = 19;
const TAG_IA5: u64 = 22;
const TAG_GENERALIZED_TIME: u64 = 24;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    None,
    Int(i64),
    Bool(bool),
    Str(Vec<u8>),
}
#[derive(Debug, Clone)]
pub(crate) struct Packet {
    pub class: u8,
    pub constructed: bool,
    pub tag: u64,
    pub value: Value,
    pub data: Vec<u8>,
    pub children: Vec<Packet>,
}
impl Packet {
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = encode_identifier(self.class, self.constructed, self.tag);
        out.extend(encode_length(self.data.len()));
        out.extend_from_slice(&self.data);
        out
    }
    pub fn int(&self) -> Option<i64> {
        match self.value {
            Value::Int(v) => Some(v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&[u8]> {
        match &self.value {
            Value::Str(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_octet_string(&self) -> bool {
        self.class == CLASS_UNIVERSAL
            && !self.constructed
            && self.tag == TAG_OCTET_STRING
            && matches!(self.value, Value::Str(_))
    }
}
fn encode_identifier(class: u8, constructed: bool, tag: u64) -> Vec<u8> {
    let first = class | if constructed { 0x20 } else { 0 };
    if tag < 31 {
        return vec![first | tag as u8];
    }
    let mut groups = Vec::new();
    let mut t = tag;
    while t != 0 {
        let mut b = (t & 0x7f) as u8;
        t >>= 7;
        if !groups.is_empty() {
            b |= 0x80;
        }
        groups.push(b);
    }
    groups.reverse();
    let mut out = vec![first | 0x1f];
    out.extend(groups);
    out
}
fn encode_unsigned(v: u64) -> Vec<u8> {
    let mut n = 1;
    let mut x = v;
    while x > 255 {
        n += 1;
        x >>= 8;
    }
    (0..n).rev().map(|i| (v >> (i * 8)) as u8).collect()
}
fn encode_length(len: usize) -> Vec<u8> {
    let bytes = encode_unsigned(len as u64);
    if len > 127 || bytes.len() > 1 {
        let mut out = vec![0x80 | bytes.len() as u8];
        out.extend(bytes);
        return out;
    }
    bytes
}
fn encode_integer(v: i64) -> Vec<u8> {
    let mut n = 1;
    let mut i = v;
    while i > 127 {
        n += 1;
        i >>= 8;
    }
    while i < -128 {
        n += 1;
        i >>= 8;
    }
    (0..n).rev().map(|k| (v >> (k * 8)) as u8).collect()
}
pub(crate) fn tlv(identifier: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![identifier];
    out.extend(encode_length(content.len()));
    out.extend_from_slice(content);
    out
}
pub(crate) fn integer(identifier: u8, v: i64) -> Vec<u8> {
    tlv(identifier, &encode_integer(v))
}
pub(crate) fn boolean(v: bool) -> Vec<u8> {
    tlv(0x01, &encode_integer(i64::from(v)))
}
fn parse_int64(b: &[u8]) -> Result<i64, ()> {
    if b.len() > 8 {
        return Err(());
    }
    if b.is_empty() {
        return Ok(0);
    }
    let mut v: i64 = 0;
    for &x in b {
        v = (v << 8) | i64::from(x);
    }
    let shift = 64 - 8 * b.len() as u32;
    Ok(if shift == 0 { v } else { (v << shift) >> shift })
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, ()> {
        let b = *self.data.get(self.pos).ok_or(())?;
        self.pos += 1;
        Ok(b)
    }
}
fn read_packet(r: &mut Reader) -> Result<(Packet, usize), ()> {
    let mut read = 0usize;
    let b = r.byte()?;
    read += 1;
    let class = b & 0xc0;
    let constructed = b & 0x20 != 0;
    let mut tag = u64::from(b & 0x1f);
    if tag == 0x1f {
        tag = 0;
        let mut count = 0;
        loop {
            let b = r.byte()?;
            count += 1;
            read += 1;
            tag = (tag << 7) | u64::from(b & 0x7f);
            if count == 1 && tag == 0 {
                return Err(());
            }
            if count > 9 {
                return Err(());
            }
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let l = r.byte()?;
    read += 1;
    let length: i64 = match l {
        0xff => return Err(()),
        0x80 => -1,
        l if l & 0x80 == 0 => i64::from(l),
        l => {
            let n = l & 0x7f;
            if n > 8 {
                return Err(());
            }
            let mut v: i64 = 0;
            for _ in 0..n {
                v = (v << 8) | i64::from(r.byte()?);
                read += 1;
            }
            v
        }
    };
    if length == -1 && !constructed {
        return Err(());
    }
    if length < -1 {
        return Err(());
    }
    let mut p = Packet {
        class,
        constructed,
        tag,
        value: Value::None,
        data: Vec::new(),
        children: Vec::new(),
    };
    if constructed {
        let mut content = 0i64;
        loop {
            if length != -1 {
                if content == length {
                    break;
                }
                if content > length {
                    return Err(());
                }
            }
            let (child, n) = read_packet(r)?;
            content += n as i64;
            read += n;
            let eoc = child.tag == 0
                && child.class == CLASS_UNIVERSAL
                && !child.constructed
                && child.data.is_empty()
                && child.children.is_empty();
            if eoc {
                if length == -1 {
                    break;
                }
                return Err(());
            }
            p.data.extend(child.bytes());
            p.children.push(child);
        }
        return Ok((p, read));
    }
    if length > i64::from(i32::MAX) {
        return Err(());
    }
    let length = length as usize;
    if r.data.len() - r.pos < length {
        return Err(());
    }
    let content = r.data[r.pos..r.pos + length].to_vec();
    r.pos += length;
    read += length;
    let mut failed = false;
    if class == CLASS_UNIVERSAL {
        p.value = match tag {
            TAG_BOOLEAN => Value::Bool(parse_int64(&content).unwrap_or(0) != 0),
            TAG_INTEGER | TAG_ENUMERATED => Value::Int(parse_int64(&content).unwrap_or(0)),
            TAG_OCTET_STRING => Value::Str(content.clone()),
            TAG_REAL => {
                failed = parse_real(&content).is_err();
                Value::None
            }
            TAG_UTF8 => match std::str::from_utf8(&content) {
                Ok(_) => Value::Str(content.clone()),
                Err(_) => {
                    failed = true;
                    Value::None
                }
            },
            TAG_PRINTABLE => {
                failed = !crate::textfmt::runes(&content)
                    .iter()
                    .all(|&c| c.is_ascii_alphanumeric() || "'()+,-./:=? ".contains(c));
                Value::None
            }
            TAG_IA5 => {
                failed = crate::textfmt::runes(&content)
                    .iter()
                    .any(|&c| c as u32 >= 0x7f);
                Value::None
            }
            TAG_GENERALIZED_TIME => {
                failed = !generalized_time_ok(&content);
                Value::None
            }
            _ => Value::None,
        };
    }
    p.data = content;
    if failed {
        return Err(());
    }
    Ok((p, read))
}
pub(crate) fn decode_packet(data: &[u8]) -> Result<Packet, ()> {
    read_packet(&mut Reader { data, pos: 0 }).map(|(p, _)| p)
}
fn parse_real(v: &[u8]) -> Result<(), ()> {
    if v.is_empty() {
        return Ok(());
    }
    let val = if v[0] & 0x80 == 0x80 {
        parse_binary_float(v)?
    } else if v[0] & 0xc0 == 0x40 {
        if v.len() != 1 {
            return Err(());
        }
        match v[0] {
            0x40 => f64::INFINITY,
            0x41 => f64::NEG_INFINITY,
            0x42 => f64::NAN,
            0x43 => -0.0,
            _ => return Err(()),
        }
    } else if v[0] & 0xc0 == 0 {
        let text = std::str::from_utf8(&v[1..]).map_err(|_| ())?;
        let text = text.trim_start_matches(' ');
        let val = match v[0] & 0x3f {
            0x01 => parse_int(text).map_err(|_| ())? as f64,
            0x02 | 0x03 => parse_float(&text.replace(',', ".")).map_err(|_| ())?,
            _ => return Err(()),
        };
        if val == 0.0 && val.is_sign_negative() {
            return Err(());
        }
        val
    } else {
        return Err(());
    };
    if val == 0.0 && !val.is_sign_negative() {
        return Err(());
    }
    Ok(())
}
fn parse_binary_float(v: &[u8]) -> Result<f64, ()> {
    let info = v[0];
    let mut v = &v[1..];
    let base: f64 = match info & 0x30 {
        0x00 => 2.0,
        0x10 => 8.0,
        0x20 => 16.0,
        _ => return Err(()),
    };
    let scale = u32::from((info & 0x0c) >> 2);
    let exp_len = match info & 0x03 {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => {
            if v.len() < 2 {
                return Err(());
            }
            let n = v[0] as usize;
            if n > 8 {
                return Err(());
            }
            v = &v[1..];
            n
        }
    };
    if exp_len > v.len() {
        return Err(());
    }
    let exponent = parse_int64(&v[..exp_len])?;
    let v = &v[exp_len..];
    if v.len() > 8 {
        return Err(());
    }
    let mut mantissa = parse_int64(v)?.wrapping_shl(scale);
    if info & 0x40 == 0x40 {
        mantissa = mantissa.wrapping_neg();
    }
    Ok(mantissa as f64 * crate::fmath::pow(base, exponent as f64))
}
fn generalized_time_ok(v: &[u8]) -> bool {
    let mut s = v.to_vec();
    let Some(mut tz) = s.iter().position(|c| b"Z+-".contains(c)) else {
        return false;
    };
    let dot = s.iter().position(|c| *c == b'.' || *c == b',');
    let digits = match dot {
        None => match tz {
            10 | 12 | 14 => tz,
            _ => return false,
        },
        Some(d @ (10 | 12)) => {
            if tz < d {
                return false;
            }
            s[d] = b'.';
            let Ok(fraction) = std::str::from_utf8(&s[d..tz]) else {
                return false;
            };
            if parse_float(fraction).is_err() {
                return false;
            }
            s.drain(d..tz);
            tz = d;
            d
        }
        Some(14) => {
            if tz < 14 {
                return false;
            }
            s[14] = b'.';
            14
        }
        Some(_) => return false,
    };
    let zone = match s.len() - tz {
        1 => {
            if s[s.len() - 1] != b'Z' {
                return false;
            }
            false
        }
        3 => {
            s.extend_from_slice(b"00");
            true
        }
        5 => true,
        _ => return false,
    };
    time_parse(&s, digits, zone)
}
fn getnum(v: &[u8], fixed: bool) -> Option<(i64, usize)> {
    let d = |i: usize| v.get(i).is_some_and(u8::is_ascii_digit);
    if !d(0) {
        return None;
    }
    if !d(1) {
        if fixed {
            return None;
        }
        return Some((i64::from(v[0] - b'0'), 1));
    }
    Some((i64::from(v[0] - b'0') * 10 + i64::from(v[1] - b'0'), 2))
}
fn time_parse(v: &[u8], digits: usize, zone: bool) -> bool {
    let mut v = v;
    if v.len() < 4 {
        return false;
    }
    let year_text = &v[..4];
    let (neg, body) = match year_text[0] {
        b'-' => (true, &year_text[1..]),
        b'+' => (false, &year_text[1..]),
        _ => (false, year_text),
    };
    if !body.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let mut year: i64 = body.iter().fold(0, |a, c| a * 10 + i64::from(c - b'0'));
    if neg {
        year = -year;
    }
    v = &v[4..];
    let Some((month, n)) = getnum(v, true) else {
        return false;
    };
    v = &v[n..];
    if !(1..=12).contains(&month) {
        return false;
    }
    let Some((day, n)) = getnum(v, true) else {
        return false;
    };
    v = &v[n..];
    let Some((hour, n)) = getnum(v, false) else {
        return false;
    };
    v = &v[n..];
    if hour >= 24 {
        return false;
    }
    if digits >= 12 {
        let Some((minute, n)) = getnum(v, true) else {
            return false;
        };
        v = &v[n..];
        if minute >= 60 {
            return false;
        }
    }
    if digits >= 14 {
        let Some((second, n)) = getnum(v, true) else {
            return false;
        };
        v = &v[n..];
        if second >= 60 {
            return false;
        }
        if v.len() >= 2 && (v[0] == b'.' || v[0] == b',') && v[1].is_ascii_digit() {
            let mut n = 2;
            while n < v.len() && v[n].is_ascii_digit() {
                n += 1;
            }
            v = &v[n..];
        }
    }
    if zone {
        if v.first() == Some(&b'Z') {
            v = &v[1..];
        } else {
            if v.len() < 5 {
                return false;
            }
            let (Some((hr, _)), Some((mm, _))) = (getnum(&v[1..3], true), getnum(&v[3..5], true))
            else {
                return false;
            };
            if hr > 24 || mm > 60 || (v[0] != b'+' && v[0] != b'-') {
                return false;
            }
            v = &v[5..];
        }
    } else {
        if v.first() != Some(&b'Z') {
            return false;
        }
        v = &v[1..];
    }
    if !v.is_empty() {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}
pub(crate) fn validate_ber(data: &[u8], o: &Options) -> Result<(), Error> {
    let mut stack = vec![(0usize, data.len(), 0i64)];
    let mut nodes = 0i64;
    while let Some((start, end, depth)) = stack.pop() {
        let mut at = start;
        while at < end {
            if end - at < 2 || data[at] & 0x1f == 0x1f {
                return Err(Error::Protocol);
            }
            let tag = data[at];
            let mut size = 2;
            let mut length = data[at + 1] as usize;
            if data[at + 1] & 0x80 != 0 {
                let n = (data[at + 1] & 0x7f) as usize;
                if n == 0 || n > 4 || end - at < 2 + n {
                    return Err(Error::Protocol);
                }
                size += n;
                let value = data[at + 2..at + size]
                    .iter()
                    .fold(0u64, |v, b| (v << 8) | u64::from(*b));
                if value > (end - at - size) as u64 {
                    return Err(Error::Protocol);
                }
                length = value as usize;
            }
            if length > end - at - size {
                return Err(Error::Protocol);
            }
            nodes += 1;
            if nodes > o.max_ber_nodes {
                return Err(Error::Limit);
            }
            if tag & 0x20 != 0 {
                if depth >= o.max_ber_depth {
                    return Err(Error::Limit);
                }
                if at + size + length < end {
                    stack.push((at + size + length, end, depth));
                }
                stack.push((at + size, at + size + length, depth + 1));
                break;
            }
            at += size + length;
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameError {
    Limit,
    Protocol,
    Io,
}
pub(crate) fn read_frame(
    read_exact: &mut dyn FnMut(&mut [u8]) -> Result<(), FrameError>,
    o: &Options,
    remaining: i64,
) -> Result<Vec<u8>, FrameError> {
    let mut header = [0u8; 6];
    read_exact(&mut header[..2])?;
    if header[0] != 0x30 {
        return Err(FrameError::Protocol);
    }
    let mut header_size = 2usize;
    let mut length = i64::from(header[1]);
    if header[1] & 0x80 != 0 {
        let n = (header[1] & 0x7f) as usize;
        if n == 0 || n > 4 {
            return Err(FrameError::Protocol);
        }
        read_exact(&mut header[2..2 + n])?;
        header_size += n;
        let value = header[2..header_size]
            .iter()
            .fold(0u64, |v, b| (v << 8) | u64::from(*b));
        if value > o.max_packet_bytes as u64 {
            return Err(FrameError::Limit);
        }
        length = value as i64;
    }
    let hs = header_size as i64;
    if length > o.max_packet_bytes - hs || hs > remaining || length > remaining - hs {
        return Err(FrameError::Limit);
    }
    let mut frame = vec![0u8; header_size + length as usize];
    frame[..header_size].copy_from_slice(&header[..header_size]);
    read_exact(&mut frame[header_size..])?;
    validate_ber(&frame, o).map_err(|e| match e {
        Error::Limit => FrameError::Limit,
        _ => FrameError::Protocol,
    })?;
    Ok(frame)
}
pub(crate) fn validate_envelope(frame: &[u8], o: &Options) -> Result<u64, Error> {
    let p = decode_packet(frame).map_err(|_| Error::Protocol)?;
    if p.children.len() < 2 || p.children.len() > 3 {
        return Err(Error::Protocol);
    }
    let id = &p.children[0];
    let id_ok = id.class == CLASS_UNIVERSAL && !id.constructed && id.tag == TAG_INTEGER;
    match id.int() {
        Some(v) if id_ok && (1..=2147483647).contains(&v) => {}
        _ => return Err(Error::Protocol),
    }
    let op = &p.children[1];
    if op.class != CLASS_APPLICATION || !op.constructed {
        return Err(Error::Protocol);
    }
    match op.tag {
        1 | 5 | 24 => {
            if op.children.len() < 3
                || op.children[0].tag != TAG_ENUMERATED
                || !op.children[1].is_octet_string()
                || !op.children[2].is_octet_string()
            {
                return Err(Error::Protocol);
            }
            match op.children[0].int() {
                Some(code) if code >= 0 => {}
                _ => return Err(Error::Protocol),
            }
        }
        4 => {
            if op.children.len() != 2
                || !op.children[0].is_octet_string()
                || op.children[1].tag != TAG_SEQUENCE
            {
                return Err(Error::Protocol);
            }
            for attr in &op.children[1].children {
                if attr.tag != TAG_SEQUENCE
                    || attr.children.len() != 2
                    || !attr.children[0].is_octet_string()
                    || attr.children[1].tag != TAG_SET
                {
                    return Err(Error::Protocol);
                }
                if !attr.children[1]
                    .children
                    .iter()
                    .all(Packet::is_octet_string)
                {
                    return Err(Error::Protocol);
                }
            }
        }
        19 => {
            if !op.children.iter().all(Packet::is_octet_string) {
                return Err(Error::Protocol);
            }
        }
        _ => return Err(Error::Protocol),
    }
    if p.children.len() == 3 {
        let controls = &p.children[2];
        if controls.class != CLASS_CONTEXT
            || !controls.constructed
            || controls.tag != 0
            || controls.children.len() > 1
        {
            return Err(Error::Protocol);
        }
        for control in &controls.children {
            if control.class != CLASS_UNIVERSAL
                || !control.constructed
                || control.tag != TAG_SEQUENCE
                || control.children.len() < 2
                || control.children.len() > 3
                || !control.children[0].is_octet_string()
            {
                return Err(Error::Protocol);
            }
            if control.children[0].str() != Some(PAGING_OID.as_bytes()) {
                return Err(Error::Protocol);
            }
            if control.children.len() == 3 {
                let critical = &control.children[1];
                if !matches!(critical.value, Value::Bool(_))
                    || critical.class != CLASS_UNIVERSAL
                    || critical.constructed
                    || critical.tag != TAG_BOOLEAN
                {
                    return Err(Error::Protocol);
                }
            }
            let value = &control.children[control.children.len() - 1];
            if !value.is_octet_string() {
                return Err(Error::Protocol);
            }
            let encoded = &value.data;
            validate_ber(encoded, o)?;
            let paging = decode_packet(encoded).map_err(|_| Error::Protocol)?;
            if paging.tag != TAG_SEQUENCE
                || paging.children.len() != 2
                || paging.children[0].tag != TAG_INTEGER
                || !paging.children[1].is_octet_string()
                || paging.children[1].data.len() > 4096
            {
                return Err(Error::Protocol);
            }
            match paging.children[0].int() {
                Some(count)
                    if (0..=4294967295).contains(&count)
                        && paging.class == CLASS_UNIVERSAL
                        && paging.constructed
                        && paging.bytes().len() == encoded.len() => {}
                _ => return Err(Error::Protocol),
            }
        }
    }
    Ok(op.tag)
}
pub(crate) const PAGING_OID: &str = "1.2.840.113556.1.4.319";
