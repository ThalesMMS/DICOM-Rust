use super::{Error, Kind};
use crate::dictionary::private_creator_id;
use crate::object::{Tag, dict};
use crate::textfmt::quote;
use std::fmt;
pub const MAX_PATH_LENGTH: usize = 4096;
pub const MAX_SEGMENTS: usize = 64;
pub const MAX_ITEM_INDEX: usize = 1 << 20;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub tag: Tag,
    pub creator: String,
    pub keyword: String,
    pub has_item: bool,
    pub item: usize,
    pub any_item: bool,
}
impl Segment {
    pub fn private(&self) -> bool {
        !self.creator.is_empty()
    }
}
impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.private() {
            write!(
                f,
                "({:04X},{},{:02X})",
                self.tag.group,
                quote(&self.creator),
                self.tag.element & 0xff
            )?;
        } else if !self.keyword.is_empty() {
            f.write_str(&self.keyword)?;
        } else {
            write!(f, "({:04X},{:04X})", self.tag.group, self.tag.element)?;
        }
        if self.any_item {
            f.write_str("[*]")
        } else if self.has_item {
            write!(f, "[{}]", self.item)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Path(pub Vec<Segment>);
impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, s) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{s}")?;
        }
        Ok(())
    }
}
impl Path {
    pub fn selects_item(&self) -> bool {
        self.0.last().is_some_and(|s| s.has_item)
    }
    pub fn wildcard(&self) -> bool {
        self.0.iter().any(|s| s.any_item)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
pub fn parse(text: &str) -> Result<Path, Error> {
    if text.len() > MAX_PATH_LENGTH {
        return Err(Error::new(
            Kind::Limit,
            format!("path longer than {MAX_PATH_LENGTH} bytes"),
        ));
    }
    let mut p = Parser {
        text: text.as_bytes(),
        pos: 0,
    };
    let mut path = Vec::new();
    loop {
        if path.len() == MAX_SEGMENTS {
            return Err(Error::new(
                Kind::Limit,
                format!("more than {MAX_SEGMENTS} segments"),
            ));
        }
        path.push(p.segment()?);
        if p.done() {
            break;
        }
        if !p.consume(b'.') {
            return Err(p.error("expected '.'"));
        }
    }
    for (i, s) in path[..path.len() - 1].iter().enumerate() {
        if !s.has_item {
            return Err(Error::new(
                Kind::Syntax,
                format!("segment {} ({s}) needs an item index", i + 1),
            ));
        }
    }
    Ok(Path(path))
}
struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn done(&self) -> bool {
        self.pos >= self.text.len()
    }
    fn peek(&self) -> u8 {
        if self.done() { 0 } else { self.text[self.pos] }
    }
    fn consume(&mut self, c: u8) -> bool {
        if !self.done() && self.peek() == c {
            self.pos += 1;
            return true;
        }
        false
    }
    fn error(&self, msg: impl fmt::Display) -> Error {
        Error::new(Kind::Syntax, format!("{msg} at offset {}", self.pos))
    }
    fn segment(&mut self) -> Result<Segment, Error> {
        let mut s = if self.peek() == b'(' {
            self.tag()?
        } else {
            self.keyword()?
        };
        if self.consume(b'[') {
            s.has_item = true;
            if self.consume(b'*') {
                s.any_item = true;
            } else {
                let start = self.pos;
                while !self.done() && self.peek().is_ascii_digit() {
                    self.pos += 1;
                }
                let digits = &self.text[start..self.pos];
                if digits.is_empty() || (digits.len() > 1 && digits[0] == b'0') {
                    return Err(self.error("item index must be a non-negative decimal number"));
                }
                let n = std::str::from_utf8(digits)
                    .ok()
                    .and_then(|d| d.parse::<i64>().ok());
                match n {
                    Some(n) if n as usize <= MAX_ITEM_INDEX => s.item = n as usize,
                    _ => {
                        return Err(Error::new(
                            Kind::Limit,
                            format!("item index above {MAX_ITEM_INDEX}"),
                        ));
                    }
                }
            }
            if !self.consume(b']') {
                return Err(self.error("expected ']'"));
            }
        }
        Ok(s)
    }
    fn hex(&mut self, digits: usize) -> Result<u16, Error> {
        let msg = format!("expected {digits} hexadecimal digits");
        if self.pos + digits > self.text.len() {
            return Err(self.error(msg));
        }
        let field = &self.text[self.pos..self.pos + digits];
        if !field.iter().all(u8::is_ascii_hexdigit) {
            return Err(self.error(msg));
        }
        let value = u32::from_str_radix(std::str::from_utf8(field).unwrap_or("x"), 16)
            .map_err(|_| self.error(&msg))?;
        if value > 0xFFFF {
            return Err(self.error(msg));
        }
        self.pos += digits;
        Ok(value as u16)
    }
    fn tag(&mut self) -> Result<Segment, Error> {
        self.consume(b'(');
        let group = self.hex(4)?;
        if !self.consume(b',') {
            return Err(self.error("expected ','"));
        }
        let mut s = Segment::default();
        if self.consume(b'"') {
            let rest = &self.text[self.pos..];
            let Some(end) = rest.iter().position(|&b| b == b'"') else {
                return Err(self.error("unterminated private creator"));
            };
            let creator = String::from_utf8_lossy(&rest[..end]).into_owned();
            self.pos += end + 1;
            let id = match private_creator_id(&creator) {
                Ok(id) if !id.is_empty() => id.to_string(),
                _ => return Err(self.error("invalid private creator")),
            };
            if group % 2 == 0 || group <= 0x0007 || group == 0xFFFF {
                return Err(self.error(format!("group {group:04X} cannot hold private attributes")));
            }
            if !self.consume(b',') {
                return Err(self.error("expected ','"));
            }
            let offset = self.hex(2)?;
            s.tag = Tag::new(group, offset);
            s.creator = id;
        } else {
            let element = self.hex(4)?;
            s.tag = Tag::new(group, element);
        }
        if !self.consume(b')') {
            return Err(self.error("expected ')'"));
        }
        if s.tag.group == 0xFFFE {
            return Err(self.error("item and delimitation tags cannot be addressed"));
        }
        Ok(s)
    }
    fn keyword(&mut self) -> Result<Segment, Error> {
        let start = self.pos;
        while !self.done() {
            let c = self.peek();
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.pos += 1;
                continue;
            }
            break;
        }
        let word = String::from_utf8_lossy(&self.text[start..self.pos]).into_owned();
        if word.is_empty() {
            self.pos = start;
            return Err(self.error("expected a keyword or tag"));
        }
        match dict::by_keyword(&word) {
            Some((tag, keyword)) => Ok(Segment {
                tag,
                keyword: keyword.to_string(),
                ..Default::default()
            }),
            None => {
                self.pos = start;
                Err(self.error(format!("unknown keyword {}", quote(&word))))
            }
        }
    }
}
