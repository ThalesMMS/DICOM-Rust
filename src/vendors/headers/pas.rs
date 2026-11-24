use super::{Budget, private_value};
use crate::dicomxml::xmlwire::{Decoder, Tok, XmlError};
use crate::object::Object;
use crate::vendors::{Error, Kind, Limits, trim_unicode_space};
fn no_charset_reader(_label: &str, _rest: &[u8]) -> Result<Vec<u8>, String> {
    Err("xml: encoding declared but no charset reader is set".to_string())
}
pub fn decode_canon_pas(raw: &[u8], l: Limits) -> Result<String, Error> {
    let mut b = Budget::new(l);
    b.take(0, raw.len() as i64, 0)?;
    let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
    let Ok(text) = std::str::from_utf8(raw) else {
        return Err(Error::new(Kind::Invalid, "PAS requires UTF-8"));
    };
    let body = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    let mut d = Decoder::new(body.to_vec(), None, no_charset_reader);
    let (mut depth, mut roots) = (0i64, 0i64);
    let mut declaration = false;
    loop {
        let tok = match d.token() {
            Ok(tok) => tok,
            Err(XmlError::Eof) => break,
            Err(_) => return Err(Error::new(Kind::Invalid, "PAS XML")),
        };
        b.take(1, 0, 0)?;
        match tok {
            Tok::Start(_, attrs) => {
                b.take(attrs.len() as i64, 0, 0)?;
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if depth > 64 {
                    return Err(Error::bare(Kind::Limit));
                }
            }
            Tok::End(_) => depth -= 1,
            Tok::ProcInst(target, _) => {
                if target.eq_ignore_ascii_case("xml") {
                    if target != "xml" || declaration || depth != 0 || roots != 0 {
                        return Err(Error::bare(Kind::Invalid));
                    }
                    declaration = true;
                }
            }
            Tok::Directive(_) => {
                return Err(Error::new(Kind::UnsupportedHeader, "PAS DTD/directive"));
            }
            Tok::CharData(data) => {
                if depth == 0 && !trim_unicode_space(&data).is_empty() {
                    return Err(Error::bare(Kind::Invalid));
                }
            }
            Tok::Comment(_) => {}
        }
    }
    if depth != 0 || roots != 1 {
        return Err(Error::bare(Kind::Invalid));
    }
    b.take(0, 0, raw.len() as i64)?;
    Ok(text.to_string())
}
pub fn canon_pas_of(o: &Object, l: Limits) -> Result<String, Error> {
    let l = l.normalized();
    let raw = private_value(o, 0x700d, "CANON_MEC_MR3^10", 0x19, l)?;
    decode_canon_pas(&raw, l)
}
