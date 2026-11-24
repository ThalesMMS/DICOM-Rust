use super::der::{self, Tlv};
use num_bigint::BigInt;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Asn1Error;
impl fmt::Display for Asn1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("asn1: structure error")
    }
}
pub(super) type Result<T> = std::result::Result<T, Asn1Error>;
pub(super) const BOOLEAN: u8 = 0x01;
pub(super) const INTEGER: u8 = 0x02;
pub(super) const BIT_STRING: u8 = 0x03;
pub(super) const OCTET_STRING: u8 = 0x04;
pub(super) const OID: u8 = 0x06;
pub(super) const SEQUENCE: u8 = 0x30;
pub(super) const SET: u8 = 0x31;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Oid(pub Vec<u32>);
impl Oid {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(super) fn is(&self, arcs: &[u32]) -> bool {
        self.0 == arcs
    }
    pub(super) fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let mut push = |mut value: u64| {
            let mut chunk = vec![(value & 0x7F) as u8];
            value >>= 7;
            while value > 0 {
                chunk.push(0x80 | (value & 0x7F) as u8);
                value >>= 7;
            }
            chunk.reverse();
            body.extend_from_slice(&chunk);
        };
        if let [first, second, rest @ ..] = self.0.as_slice() {
            push(u64::from(*first) * 40 + u64::from(*second));
            for arc in rest {
                push(u64::from(*arc));
            }
        }
        element(OID, &body)
    }
}
impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, arc) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{arc}")?;
        }
        Ok(())
    }
}
pub(super) fn element(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    if body.len() < 0x80 {
        out.push(body.len() as u8);
    } else {
        let digits: Vec<u8> = body
            .len()
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        out.push(0x80 | digits.len() as u8);
        out.extend_from_slice(&digits);
    }
    out.extend_from_slice(body);
    out
}
pub(super) fn integer(value: &BigInt) -> Vec<u8> {
    element(INTEGER, &value.to_signed_bytes_be())
}
pub(super) struct Fields<'a> {
    rest: &'a [u8],
}
impl<'a> Fields<'a> {
    pub fn new(body: &'a [u8]) -> Fields<'a> {
        Fields { rest: body }
    }
    fn peek(&self) -> Result<Option<Tlv<'a>>> {
        if self.rest.is_empty() {
            return Ok(None);
        }
        der::read(self.rest)
            .map(|(tlv, _)| Some(tlv))
            .ok_or(Asn1Error)
    }
    fn advance(&mut self, tlv: &Tlv<'a>) {
        self.rest = &self.rest[tlv.full.len()..];
    }
    pub fn any(&mut self) -> Result<Tlv<'a>> {
        let tlv = self.peek()?.ok_or(Asn1Error)?;
        self.advance(&tlv);
        Ok(tlv)
    }
    pub(crate) fn optional_any(&mut self) -> Result<Option<Tlv<'a>>> {
        let tlv = self.peek()?;
        if let Some(tlv) = &tlv {
            self.advance(tlv);
        }
        Ok(tlv)
    }
    pub fn required(&mut self, tag: u8) -> Result<Tlv<'a>> {
        self.optional(tag)?.ok_or(Asn1Error)
    }
    pub fn optional(&mut self, tag: u8) -> Result<Option<Tlv<'a>>> {
        self.optional_if(|t| t == tag)
    }
    fn optional_if(&mut self, matches: impl Fn(u8) -> bool) -> Result<Option<Tlv<'a>>> {
        match self.peek()? {
            Some(tlv) if matches(tlv.tag) => {
                self.advance(&tlv);
                Ok(Some(tlv))
            }
            _ => Ok(None),
        }
    }
    pub(crate) fn optional_context(&mut self, number: u8) -> Result<Option<Tlv<'a>>> {
        self.optional_if(|t| t & 0xDF == 0x80 | number)
    }
    pub(crate) fn optional_explicit(&mut self, number: u8) -> Result<Option<Tlv<'a>>> {
        let Some(tlv) = self.peek()? else {
            return Ok(None);
        };
        if tlv.body.is_empty() && tlv.full.len() == self.rest.len() {
            return Err(Asn1Error);
        }
        if tlv.tag & 0xDF == 0x80 | number && (tlv.body.is_empty() || tlv.tag & 0x20 != 0) {
            self.advance(&tlv);
            return Ok(Some(tlv));
        }
        Ok(None)
    }
    pub fn int(&mut self) -> Result<i64> {
        int64(self.required(INTEGER)?.body)
    }
    pub(crate) fn big_int(&mut self) -> Result<BigInt> {
        big_int(self.required(INTEGER)?.body)
    }
    pub(crate) fn optional_big_int(&mut self) -> Result<Option<BigInt>> {
        self.optional(INTEGER)?.map(|t| big_int(t.body)).transpose()
    }
    #[doc(hidden)]
    pub fn oid(&mut self) -> Result<Oid> {
        oid(self.required(OID)?.body)
    }
    pub(crate) fn optional_oid(&mut self) -> Result<Option<Oid>> {
        self.optional(OID)?.map(|t| oid(t.body)).transpose()
    }
    pub(crate) fn octets(&mut self) -> Result<&'a [u8]> {
        Ok(self.required(OCTET_STRING)?.body)
    }
    pub(crate) fn optional_bool(&mut self) -> Result<bool> {
        match self.optional(BOOLEAN)? {
            None => Ok(false),
            Some(tlv) => match tlv.body {
                [0x00] => Ok(false),
                [0xFF] => Ok(true),
                _ => Err(Asn1Error),
            },
        }
    }
}
pub(super) fn single(data: &[u8]) -> Result<Option<Tlv<'_>>> {
    let (tlv, rest) = der::read(data).ok_or(Asn1Error)?;
    Ok(rest.is_empty().then_some(tlv))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unmarshal {
    Decoding(Asn1Error),
    TrailingData,
}
impl fmt::Display for Unmarshal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unmarshal::Decoding(err) => err.fmt(f),
            Unmarshal::TrailingData => f.write_str("trailing data"),
        }
    }
}
pub(super) fn unmarshal<'a, T>(
    data: &'a [u8],
    decode: impl FnOnce(Tlv<'a>) -> Result<T>,
) -> std::result::Result<T, Unmarshal> {
    let (tlv, rest) = der::read(data).ok_or(Unmarshal::Decoding(Asn1Error))?;
    let value = decode(tlv).map_err(Unmarshal::Decoding)?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(Unmarshal::TrailingData)
    }
}
pub(super) fn sequence_fields<'a>(tlv: Tlv<'a>) -> Result<Fields<'a>> {
    if tlv.tag != SEQUENCE {
        return Err(Asn1Error);
    }
    Ok(Fields::new(tlv.body))
}
pub(super) fn elements(body: &[u8]) -> Result<Vec<Tlv<'_>>> {
    der::children(body).ok_or(Asn1Error)
}
pub(super) fn sequences(body: &[u8]) -> Result<Vec<Tlv<'_>>> {
    let list = elements(body)?;
    if list.iter().any(|t| t.tag != SEQUENCE) {
        return Err(Asn1Error);
    }
    Ok(list)
}
fn minimal(body: &[u8]) -> Result<()> {
    match body {
        [] => Err(Asn1Error),
        [0x00, second, ..] if second & 0x80 == 0 => Err(Asn1Error),
        [0xFF, second, ..] if second & 0x80 != 0 => Err(Asn1Error),
        _ => Ok(()),
    }
}
pub(super) fn int64(body: &[u8]) -> Result<i64> {
    minimal(body)?;
    if body.len() > 8 {
        return Err(Asn1Error);
    }
    let mut value: i64 = if body[0] & 0x80 != 0 { -1 } else { 0 };
    for &b in body {
        value = value << 8 | i64::from(b);
    }
    Ok(value)
}
pub(super) fn big_int(body: &[u8]) -> Result<BigInt> {
    minimal(body)?;
    Ok(BigInt::from_signed_bytes_be(body))
}
pub(super) fn oid(body: &[u8]) -> Result<Oid> {
    if body.is_empty() {
        return Err(Asn1Error);
    }
    let mut arcs = Vec::new();
    let mut value: u64 = 0;
    let mut start = true;
    for (i, &b) in body.iter().enumerate() {
        if start && b == 0x80 {
            return Err(Asn1Error);
        }
        value = value << 7 | u64::from(b & 0x7F);
        if value > i32::MAX as u64 {
            return Err(Asn1Error);
        }
        start = b & 0x80 == 0;
        if !start {
            if i == body.len() - 1 {
                return Err(Asn1Error);
            }
            continue;
        }
        if arcs.is_empty() {
            if value < 80 {
                arcs.extend([(value / 40) as u32, (value % 40) as u32]);
            } else {
                arcs.extend([2, (value - 80) as u32]);
            }
        } else {
            arcs.push(value as u32);
        }
        value = 0;
    }
    Ok(Oid(arcs))
}
pub(super) fn bit_string(body: &[u8]) -> Result<()> {
    let Some((&padding, bits)) = body.split_first() else {
        return Err(Asn1Error);
    };
    if padding > 7
        || bits.is_empty() && padding != 0
        || bits
            .last()
            .is_some_and(|last| last & ((1u8 << padding) - 1) != 0)
    {
        return Err(Asn1Error);
    }
    Ok(())
}
pub(super) fn algorithm(tlv: &Tlv<'_>) -> Result<Oid> {
    let mut fields = Fields::new(tlv.body);
    let oid = fields.oid()?;
    fields.optional_any()?;
    Ok(oid)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn object_identifiers() {
        let sha256 = Oid(vec![2, 16, 840, 1, 101, 3, 4, 2, 1]);
        let der = sha256.encode();
        assert_eq!(
            der,
            [
                0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01
            ]
        );
        assert_eq!(oid(&der[2..]), Ok(sha256.clone()));
        assert_eq!(sha256.to_string(), "2.16.840.1.101.3.4.2.1");
        assert_eq!(oid(&[0x88, 0x37]), Ok(Oid(vec![2, 999])));
        assert_eq!(oid(&[]), Err(Asn1Error));
        assert_eq!(oid(&[0x2A, 0x80, 0x01]), Err(Asn1Error));
        assert_eq!(oid(&[0x2A, 0x86]), Err(Asn1Error));
    }
    #[test]
    fn integers() {
        assert_eq!(int64(&[0x7F]), Ok(127));
        assert_eq!(int64(&[0xFF]), Ok(-1));
        assert_eq!(int64(&[0x00, 0x80]), Ok(128));
        assert_eq!(int64(&[0x00, 0x7F]), Err(Asn1Error));
        assert_eq!(int64(&[0xFF, 0x80]), Err(Asn1Error));
        assert_eq!(int64(&[]), Err(Asn1Error));
        assert_eq!(int64(&[1; 9]), Err(Asn1Error));
        assert_eq!(
            integer(&BigInt::from(-5)),
            [0x02, 0x01, 0xFB],
            "two's complement"
        );
        assert_eq!(integer(&BigInt::from(128)), [0x02, 0x02, 0x00, 0x80]);
    }
    #[test]
    fn optional_fields() {
        let data = [0x02, 0x01, 0x05, 0xA1, 0x00, 0x04, 0x01, 0x09];
        let mut f = Fields::new(&data);
        assert_eq!(f.optional_oid(), Ok(None));
        assert_eq!(f.int(), Ok(5));
        assert!(f.optional_context(0).unwrap().is_none());
        assert!(f.optional_context(1).unwrap().is_some());
        assert_eq!(f.octets(), Ok(&[9u8][..]));
        assert_eq!(f.optional_bool(), Ok(false));
        let mut f = Fields::new(&[0xA1, 0x00]);
        assert!(f.optional_explicit(0).is_err());
    }
}
