use super::error::{Error, Sentinel};
use super::tables::*;
#[derive(Clone, Copy)]
pub(super) struct Table {
    pub offset: usize,
    pub len: usize,
    pub width: usize,
}
static BLOB: &[u8] = include_bytes!("tables.bin");
impl Table {
    pub(super) fn get(self, i: usize) -> u32 {
        let at = self.offset + i * self.width;
        if self.width == 2 {
            u32::from(u16::from_le_bytes([BLOB[at], BLOB[at + 1]]))
        } else {
            u32::from_le_bytes([BLOB[at], BLOB[at + 1], BLOB[at + 2], BLOB[at + 3]])
        }
    }
}
pub const CHARACTER_SETS: [&str; 16] = [
    "ASCII",
    "8859/1",
    "8859/2",
    "8859/3",
    "8859/4",
    "8859/5",
    "8859/6",
    "8859/7",
    "8859/8",
    "8859/9",
    "8859/15",
    "UNICODE UTF-8",
    "ISO IR87",
    "GB 18030-2000",
    "KS X 1001",
    "BIG-5",
];
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Encoding {
    Charmap(Table, Table),
    Iso2022Jp,
    Gb18030,
    EucKr,
    Big5,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Set {
    Ascii,
    Utf8,
    Encoded(Encoding),
}
impl PartialEq for Table {
    fn eq(&self, other: &Table) -> bool {
        self.offset == other.offset
    }
}
impl Eq for Table {}
impl std::fmt::Debug for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Table@{}", self.offset)
    }
}
pub(super) fn encoding_for(charset: &str) -> Result<Set, Error> {
    let cm = |d, e| Ok(Set::Encoded(Encoding::Charmap(d, e)));
    match crate::textfmt::to_upper(charset.trim()).as_str() {
        "" | "ASCII" => Ok(Set::Ascii),
        "UNICODE UTF-8" | "UTF-8" => Ok(Set::Utf8),
        "8859/1" => cm(ISO8859_1_DECODE, ISO8859_1_ENCODE),
        "8859/2" => cm(ISO8859_2_DECODE, ISO8859_2_ENCODE),
        "8859/3" => cm(ISO8859_3_DECODE, ISO8859_3_ENCODE),
        "8859/4" => cm(ISO8859_4_DECODE, ISO8859_4_ENCODE),
        "8859/5" => cm(ISO8859_5_DECODE, ISO8859_5_ENCODE),
        "8859/6" => cm(ISO8859_6_DECODE, ISO8859_6_ENCODE),
        "8859/7" => cm(ISO8859_7_DECODE, ISO8859_7_ENCODE),
        "8859/8" => cm(ISO8859_8_DECODE, ISO8859_8_ENCODE),
        "8859/9" => cm(ISO8859_9_DECODE, ISO8859_9_ENCODE),
        "8859/15" => cm(ISO8859_15_DECODE, ISO8859_15_ENCODE),
        "ISO IR87" => Ok(Set::Encoded(Encoding::Iso2022Jp)),
        "GB 18030-2000" => Ok(Set::Encoded(Encoding::Gb18030)),
        "KS X 1001" => Ok(Set::Encoded(Encoding::EucKr)),
        "BIG-5" => Ok(Set::Encoded(Encoding::Big5)),
        _ => Err(Error::wrap(Sentinel::Charset, "unsupported MSH-18 value")),
    }
}
pub(super) fn decode_character_set(charset: &str, data: &[u8]) -> Result<String, Error> {
    match encoding_for(charset)? {
        Set::Utf8 => match std::str::from_utf8(data) {
            Ok(s) => Ok(s.to_owned()),
            Err(_) => Err(Error::wrap(Sentinel::Charset, "invalid UTF-8")),
        },
        Set::Ascii => {
            if let Some(i) = data.iter().position(|&c| c > 0x7F) {
                return Err(Error::wrap(
                    Sentinel::Charset,
                    &format!("byte {i} is not ASCII"),
                ));
            }
            Ok(data.iter().map(|&b| b as char).collect())
        }
        Set::Encoded(e) => Ok(e.decode(data)),
    }
}
pub(super) fn encode_character_set(charset: &str, text: &str) -> Result<Vec<u8>, Error> {
    match encoding_for(charset)? {
        Set::Utf8 => Ok(text.as_bytes().to_vec()),
        Set::Ascii => {
            if text.bytes().any(|c| c > 0x7F) {
                return Err(Error::wrap(
                    Sentinel::Charset,
                    "a character is not ASCII; declare MSH-18",
                ));
            }
            Ok(text.as_bytes().to_vec())
        }
        Set::Encoded(e) => {
            let mut enc = Encoder::new(e);
            let mut out = Vec::new();
            match enc.encode(text, &mut out) {
                Ok(()) => {
                    enc.close(&mut out);
                    Ok(out)
                }
                Err(_) => Err(Error::wrap(
                    Sentinel::Charset,
                    "a character cannot be represented in the declared set",
                )),
            }
        }
    }
}
const FFFD: char = '\u{FFFD}';
fn ch(v: u32) -> char {
    char::from_u32(v).unwrap_or(FFFD)
}
impl Encoding {
    pub(super) fn decode(self, src: &[u8]) -> String {
        match self {
            Encoding::Charmap(decode, _) => {
                src.iter().map(|&b| ch(decode.get(b as usize))).collect()
            }
            Encoding::Iso2022Jp => decode_iso2022jp(src),
            Encoding::Gb18030 => decode_gb18030(src),
            Encoding::EucKr => decode_euckr(src),
            Encoding::Big5 => decode_big5(src),
        }
    }
}
fn decode_iso2022jp(src: &[u8]) -> String {
    #[derive(PartialEq)]
    enum St {
        Ascii,
        Katakana,
        Jis0208,
        Jis0212,
    }
    let mut state = St::Ascii;
    let mut out = String::with_capacity(src.len());
    let mut n = 0;
    while n < src.len() {
        let c0 = src[n];
        let (r, size): (char, usize);
        if c0 >= 0x80 {
            (r, size) = (FFFD, 1);
        } else if c0 == 0x1b {
            if n + 2 >= src.len() {
                (r, size) = (FFFD, 1);
            } else {
                let c1 = src[n + 1];
                let c2 = src[n + 2];
                if c1 == b'$' && (c2 == b'@' || c2 == b'B') {
                    state = St::Jis0208;
                    n += 3;
                    continue;
                } else if c1 == b'$' && c2 == b'(' {
                    if n + 3 >= src.len() {
                        (r, size) = (FFFD, 1);
                    } else if src[n + 3] == b'D' {
                        state = St::Jis0212;
                        n += 4;
                        continue;
                    } else {
                        (r, size) = (FFFD, 1);
                    }
                } else if c1 == b'(' && (c2 == b'B' || c2 == b'J') {
                    state = St::Ascii;
                    n += 3;
                    continue;
                } else if c1 == b'(' && c2 == b'I' {
                    state = St::Katakana;
                    n += 3;
                    continue;
                } else {
                    (r, size) = (FFFD, 1);
                }
            }
        } else {
            match state {
                St::Ascii => (r, size) = (c0 as char, 1),
                St::Katakana => {
                    if !(0x21..0x60).contains(&c0) {
                        (r, size) = (FFFD, 1);
                    } else {
                        (r, size) = (ch(u32::from(c0) + (0xff61 - 0x21)), 1);
                    }
                }
                _ => {
                    if c0 == 0x0a {
                        state = St::Ascii;
                        (r, size) = ('\n', 1);
                    } else if n + 1 >= src.len() {
                        (r, size) = (FFFD, 1);
                    } else {
                        let c1 = src[n + 1];
                        let i = usize::from(c0.wrapping_sub(0x21)) * 94
                            + usize::from(c1.wrapping_sub(0x21));
                        let v = if state == St::Jis0208 && i < JP_JIS0208DECODE.len {
                            JP_JIS0208DECODE.get(i)
                        } else if state == St::Jis0212 && i < JP_JIS0212DECODE.len {
                            JP_JIS0212DECODE.get(i)
                        } else {
                            0xFFFD
                        };
                        (r, size) = (if v == 0 { FFFD } else { ch(v) }, 2);
                    }
                }
            }
        }
        out.push(r);
        n += size;
    }
    out
}
fn decode_euckr(src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut n = 0;
    while n < src.len() {
        let c0 = src[n];
        let (r, size): (char, usize);
        if c0 < 0x80 {
            (r, size) = (c0 as char, 1);
        } else if (0x81..0xff).contains(&c0) {
            if n + 1 >= src.len() {
                (r, size) = (FFFD, 1);
            } else {
                let c1 = src[n + 1];
                let mut idx: Option<u32> = None;
                if c0 < 0xc7 {
                    let base = 178 * (u32::from(c0) - 0x81);
                    if (0x41..0x5b).contains(&c1) {
                        idx = Some(base + u32::from(c1) - 0x41);
                    } else if (0x61..0x7b).contains(&c1) {
                        idx = Some(base + u32::from(c1) - (0x61 - 26));
                    } else if (0x81..0xff).contains(&c1) {
                        idx = Some(base + u32::from(c1) - (0x81 - 52));
                    }
                } else if (0xa1..0xff).contains(&c1) {
                    idx = Some(
                        178 * (0xc7 - 0x81) + (u32::from(c0) - 0xc7) * 94 + (u32::from(c1) - 0xa1),
                    );
                }
                let mut decoded = None;
                if let Some(i) = idx
                    && (i as usize) < KR_DECODE.len
                {
                    let v = KR_DECODE.get(i as usize);
                    if v != 0 {
                        decoded = Some(ch(v));
                    }
                }
                match decoded {
                    Some(c) => (r, size) = (c, 2),
                    None => (r, size) = (FFFD, if c1 < 0x80 { 1 } else { 2 }),
                }
            }
        } else {
            (r, size) = (FFFD, 1);
        }
        out.push(r);
        n += size;
    }
    out
}
fn gb18030_range_decode(r: u32) -> u32 {
    let count = GB_GB18030.len / 2;
    let (mut i, mut j) = (0usize, count);
    while i < j {
        let h = i + (j - i) / 2;
        if r >= GB_GB18030.get(h * 2) {
            i = h + 1;
        } else {
            j = h;
        }
    }
    let k = i - 1;
    r + GB_GB18030.get(k * 2 + 1) - GB_GB18030.get(k * 2)
}
fn gb18030_range_encode(r: u32) -> u32 {
    let count = GB_GB18030.len / 2;
    let (mut i, mut j) = (0usize, count);
    while i < j {
        let h = i + (j - i) / 2;
        if r >= GB_GB18030.get(h * 2 + 1) {
            i = h + 1;
        } else {
            j = h;
        }
    }
    let k = i - 1;
    r + GB_GB18030.get(k * 2) - GB_GB18030.get(k * 2 + 1)
}
fn decode_gb18030(src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut n = 0;
    while n < src.len() {
        let c0 = src[n];
        let (r, size): (char, usize);
        if c0 < 0x80 {
            (r, size) = (c0 as char, 1);
        } else if c0 == 0x80 {
            (r, size) = ('€', 1);
        } else if c0 < 0xff {
            if n + 1 >= src.len() {
                (r, size) = (FFFD, 1);
            } else {
                let c1 = src[n + 1];
                let c1v: Option<u8> = if (0x40..0x7f).contains(&c1) {
                    Some(c1 - 0x40)
                } else if (0x80..0xff).contains(&c1) {
                    Some(c1 - 0x41)
                } else {
                    None
                };
                if let Some(c1v) = c1v {
                    let i = usize::from(c0 - 0x81) * 190 + usize::from(c1v);
                    let mut v = 0xFFFD;
                    if i < GB_DECODE.len {
                        v = GB_DECODE.get(i);
                        if v == 0 {
                            v = 0xFFFD;
                        }
                    }
                    (r, size) = (ch(v), 2);
                } else if (0x30..0x40).contains(&c1) {
                    if n + 3 >= src.len() {
                        (r, size) = (FFFD, 1);
                    } else {
                        let c2 = src[n + 2];
                        let c3 = src[n + 3];
                        if !(0x81..0xff).contains(&c2) || !(0x30..0x3a).contains(&c3) {
                            (r, size) = (FFFD, 1);
                        } else {
                            let v = ((u32::from(c0 - 0x81) * 10 + u32::from(c1 - 0x30)) * 126
                                + u32::from(c2 - 0x81))
                                * 10
                                + u32::from(c3 - 0x30);
                            if v < 39420 {
                                (r, size) = (ch(gb18030_range_decode(v)), 4);
                            } else {
                                let w = i64::from(v) - 189000;
                                if (0..0x100000).contains(&w) {
                                    (r, size) = (ch(w as u32 + 0x10000), 4);
                                } else {
                                    (r, size) = (FFFD, 1);
                                }
                            }
                        }
                    }
                } else {
                    (r, size) = (FFFD, 1);
                }
            }
        } else {
            (r, size) = (FFFD, 1);
        }
        out.push(r);
        n += size;
    }
    out
}
fn decode_big5(src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut n = 0;
    while n < src.len() {
        let c0 = src[n];
        let size: usize;
        if c0 < 0x80 {
            out.push(c0 as char);
            size = 1;
        } else if (0x81..0xff).contains(&c0) {
            if n + 1 >= src.len() {
                out.push(FFFD);
                size = 1;
            } else {
                let c1 = src[n + 1];
                let c1v = if (0x40..0x7f).contains(&c1) {
                    Some(c1 - 0x40)
                } else if (0xa1..0xff).contains(&c1) {
                    Some(c1 - 0x62)
                } else {
                    None
                };
                match c1v {
                    None if c1 < 0x40 => {
                        out.push(FFFD);
                        size = 1;
                    }
                    None => {
                        out.push(FFFD);
                        size = 2;
                    }
                    Some(c1v) => {
                        size = 2;
                        let i = usize::from(c0 - 0x81) * 157 + usize::from(c1v);
                        if i < BIG5_DECODE.len {
                            match i {
                                1133 => out.push_str("\u{00CA}\u{0304}"),
                                1135 => out.push_str("\u{00CA}\u{030C}"),
                                1164 => out.push_str("\u{00EA}\u{0304}"),
                                1166 => out.push_str("\u{00EA}\u{030C}"),
                                _ => {
                                    let v = BIG5_DECODE.get(i);
                                    out.push(if v == 0 { FFFD } else { ch(v) });
                                }
                            }
                        } else {
                            out.push(FFFD);
                        }
                    }
                }
            }
        } else {
            out.push(FFFD);
            size = 1;
        }
        n += size;
    }
    out
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unsupported;
pub(super) const RUNE_NOT_SUPPORTED: &str = "encoding: rune not supported by encoding.";
pub(super) struct Encoder {
    encoding: Encoding,
    jp_state: u8,
}
fn lookup(table: Table, low: u32, high: u32, r: u32) -> Option<u32> {
    if low <= r && r < high {
        Some(table.get((r - low) as usize))
    } else {
        None
    }
}
impl Encoder {
    pub(super) fn new(encoding: Encoding) -> Encoder {
        Encoder {
            encoding,
            jp_state: 0,
        }
    }
    pub(super) fn encode(&mut self, text: &str, out: &mut Vec<u8>) -> Result<(), Unsupported> {
        for c in text.chars() {
            self.encode_rune(c, out)?;
        }
        Ok(())
    }
    pub(super) fn close(&mut self, out: &mut Vec<u8>) {
        if self.encoding == Encoding::Iso2022Jp && self.jp_state != 0 {
            self.jp_state = 0;
            out.extend_from_slice(b"\x1b(B");
        }
    }
    fn encode_rune(&mut self, c: char, out: &mut Vec<u8>) -> Result<(), Unsupported> {
        let r = u32::from(c);
        match self.encoding {
            Encoding::Charmap(decode, encode) => {
                if r < 0x80 {
                    out.push(r as u8);
                    return Ok(());
                }
                for b in 0..256 {
                    if decode.get(b) == r && r != 0xFFFD {
                        let e = encode.get(b);
                        if e < 0x100 {
                            out.push(e as u8);
                            return Ok(());
                        }
                    }
                }
                Err(Unsupported)
            }
            Encoding::EucKr => {
                if r < 0x80 {
                    out.push(r as u8);
                    return Ok(());
                }
                let tables = [
                    (KR_ENCODE0, KR_ENCODE0LOW, KR_ENCODE0HIGH),
                    (KR_ENCODE1, KR_ENCODE1LOW, KR_ENCODE1HIGH),
                    (KR_ENCODE2, KR_ENCODE2LOW, KR_ENCODE2HIGH),
                    (KR_ENCODE3, KR_ENCODE3LOW, KR_ENCODE3HIGH),
                    (KR_ENCODE4, KR_ENCODE4LOW, KR_ENCODE4HIGH),
                    (KR_ENCODE5, KR_ENCODE5LOW, KR_ENCODE5HIGH),
                    (KR_ENCODE6, KR_ENCODE6LOW, KR_ENCODE6HIGH),
                ];
                for (t, lo, hi) in tables {
                    if let Some(v) = lookup(t, lo, hi, r) {
                        if v != 0 {
                            out.push((v >> 8) as u8);
                            out.push(v as u8);
                            return Ok(());
                        }
                        break;
                    }
                }
                Err(Unsupported)
            }
            Encoding::Big5 => {
                if r < 0x80 {
                    out.push(r as u8);
                    return Ok(());
                }
                let tables = [
                    (BIG5_ENCODE0, BIG5_ENCODE0LOW, BIG5_ENCODE0HIGH),
                    (BIG5_ENCODE1, BIG5_ENCODE1LOW, BIG5_ENCODE1HIGH),
                    (BIG5_ENCODE2, BIG5_ENCODE2LOW, BIG5_ENCODE2HIGH),
                    (BIG5_ENCODE3, BIG5_ENCODE3LOW, BIG5_ENCODE3HIGH),
                    (BIG5_ENCODE4, BIG5_ENCODE4LOW, BIG5_ENCODE4HIGH),
                    (BIG5_ENCODE5, BIG5_ENCODE5LOW, BIG5_ENCODE5HIGH),
                    (BIG5_ENCODE6, BIG5_ENCODE6LOW, BIG5_ENCODE6HIGH),
                    (BIG5_ENCODE7, BIG5_ENCODE7LOW, BIG5_ENCODE7HIGH),
                ];
                for (t, lo, hi) in tables {
                    if let Some(v) = lookup(t, lo, hi, r) {
                        if v != 0 {
                            out.push((v >> 8) as u8);
                            out.push(v as u8);
                            return Ok(());
                        }
                        break;
                    }
                }
                Err(Unsupported)
            }
            Encoding::Gb18030 => {
                if r < 0x80 {
                    out.push(r as u8);
                    return Ok(());
                }
                let tables = [
                    (GB_ENCODE0, GB_ENCODE0LOW, GB_ENCODE0HIGH),
                    (GB_ENCODE1, GB_ENCODE1LOW, GB_ENCODE1HIGH),
                    (GB_ENCODE2, GB_ENCODE2LOW, GB_ENCODE2HIGH),
                    (GB_ENCODE3, GB_ENCODE3LOW, GB_ENCODE3HIGH),
                    (GB_ENCODE4, GB_ENCODE4LOW, GB_ENCODE4HIGH),
                ];
                for (t, lo, hi) in tables {
                    if let Some(v) = lookup(t, lo, hi, r) {
                        if v != 0 {
                            out.push((v >> 8) as u8);
                            out.push(v as u8);
                            return Ok(());
                        }
                        break;
                    }
                }
                let mut v = if r < 0x10000 {
                    gb18030_range_encode(r)
                } else {
                    r + 189000 - 0x10000
                };
                let mut four = [0u8; 4];
                four[3] = (v % 10 + 0x30) as u8;
                v /= 10;
                four[2] = (v % 126 + 0x81) as u8;
                v /= 126;
                four[1] = (v % 10 + 0x30) as u8;
                v /= 10;
                four[0] = (v + 0x81) as u8;
                out.extend_from_slice(&four);
                Ok(())
            }
            Encoding::Iso2022Jp => {
                if r < 0x80 {
                    if self.jp_state != 0 {
                        self.jp_state = 0;
                        out.extend_from_slice(b"\x1b(B");
                    }
                    out.push(r as u8);
                    return Ok(());
                }
                let tables = [
                    (JP_ENCODE0, JP_ENCODE0LOW, JP_ENCODE0HIGH),
                    (JP_ENCODE1, JP_ENCODE1LOW, JP_ENCODE1HIGH),
                    (JP_ENCODE2, JP_ENCODE2LOW, JP_ENCODE2HIGH),
                    (JP_ENCODE3, JP_ENCODE3LOW, JP_ENCODE3HIGH),
                    (JP_ENCODE4, JP_ENCODE4LOW, JP_ENCODE4HIGH),
                    (JP_ENCODE5, JP_ENCODE5LOW, JP_ENCODE5HIGH),
                ];
                for (k, (t, lo, hi)) in tables.into_iter().enumerate() {
                    if lo <= r && r < hi {
                        if k == 5 && (0xff61..0xffa0).contains(&r) {
                            if self.jp_state != 1 {
                                self.jp_state = 1;
                                out.extend_from_slice(b"\x1b(I");
                            }
                            out.push((r - (0xff61 - 0x21)) as u8);
                            return Ok(());
                        }
                        let v = t.get((r - lo) as usize);
                        if v >> JP_TABLESHIFT == JP_JIS0208 {
                            if self.jp_state != 2 {
                                self.jp_state = 2;
                                out.extend_from_slice(b"\x1b$B");
                            }
                            out.push((0x21 + ((v >> JP_CODESHIFT) & JP_CODEMASK)) as u8);
                            out.push((0x21 + (v & JP_CODEMASK)) as u8);
                            return Ok(());
                        }
                        break;
                    }
                }
                if self.jp_state != 0 {
                    self.jp_state = 0;
                    out.extend_from_slice(b"\x1b(B");
                }
                Err(Unsupported)
            }
        }
    }
}
