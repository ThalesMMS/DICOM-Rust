use super::pubkey::Curve;
use num_bigint::BigUint;
type Check<T = ()> = Result<T, &'static str>;
const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const BOOLEAN: u8 = 0x01;
const INTEGER: u8 = 0x02;
const BIT_STRING: u8 = 0x03;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
const OID_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
const OID_DSA: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x38, 0x04, 0x01];
const OID_EC: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];
const OID_P224: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x21];
const OID_P256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x22];
const OID_P521: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x23];
const OID_AIA: &[u8] = &[0x2B, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x01];
#[derive(Clone, Copy)]
struct S<'a>(&'a [u8]);
impl<'a> S<'a> {
    fn empty(&self) -> bool {
        self.0.is_empty()
    }
    fn peek(&self, tag: u8) -> bool {
        self.0.first() == Some(&tag)
    }
    fn any(&mut self) -> Option<(u8, &'a [u8], &'a [u8])> {
        let (tlv, rest) = super::der::read(self.0)?;
        self.0 = rest;
        Some((tlv.tag, tlv.body, tlv.full))
    }
    fn read(&mut self, tag: u8) -> Option<&'a [u8]> {
        let mut copy = *self;
        let (t, body, _) = copy.any()?;
        if t != tag {
            return None;
        }
        *self = copy;
        Some(body)
    }
    fn element(&mut self, tag: u8) -> Option<&'a [u8]> {
        let mut copy = *self;
        let (t, _, full) = copy.any()?;
        if t != tag {
            return None;
        }
        *self = copy;
        Some(full)
    }
    fn optional(&mut self, tag: u8) -> Option<Option<&'a [u8]>> {
        if !self.peek(tag) {
            return Some(None);
        }
        self.read(tag).map(Some)
    }
    fn big_integer(&mut self) -> Option<(bool, &'a [u8])> {
        let body = self.read(INTEGER)?;
        valid_integer(body).then(|| (body[0] & 0x80 != 0, body))
    }
    fn int64(&mut self, tag: u8) -> Option<i64> {
        let body = self.read(tag)?;
        if !valid_integer(body) || body.len() > 8 {
            return None;
        }
        let mut v: i64 = if body[0] & 0x80 != 0 { -1 } else { 0 };
        for &b in body {
            v = (v << 8) | i64::from(b);
        }
        Some(v)
    }
    fn uint64(&mut self) -> Option<u64> {
        let body = self.read(INTEGER)?;
        if !valid_integer(body) || body[0] & 0x80 != 0 {
            return None;
        }
        if body.len() > 9 || body.len() == 9 && body[0] != 0 {
            return None;
        }
        Some(body.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
    }
    fn oid(&mut self) -> Option<&'a [u8]> {
        let body = self.read(OID)?;
        valid_oid(body).then_some(body)
    }
    fn boolean(&mut self) -> Option<bool> {
        match self.read(BOOLEAN)? {
            [0x00] => Some(false),
            [0xFF] => Some(true),
            _ => None,
        }
    }
    fn bit_string(&mut self) -> Option<(u8, &'a [u8])> {
        let body = self.read(BIT_STRING)?;
        let (&padding, bytes) = body.split_first()?;
        if padding > 7
            || bytes.is_empty() && padding != 0
            || !bytes.is_empty() && bytes[bytes.len() - 1] & ((1u8 << padding) - 1) != 0
        {
            return None;
        }
        Some((padding, bytes))
    }
}
fn valid_integer(b: &[u8]) -> bool {
    match b {
        [] => false,
        [_] => true,
        [0x00, second, ..] if second & 0x80 == 0 => false,
        [0xFF, second, ..] if second & 0x80 != 0 => false,
        _ => true,
    }
}
fn valid_oid(mut b: &[u8]) -> bool {
    if b.is_empty() {
        return false;
    }
    while !b.is_empty() {
        let mut ret: u32 = 0;
        let mut i = 0;
        loop {
            if i == 5 || ret >= 1 << (31 - 7) {
                return false;
            }
            let Some((&c, rest)) = b.split_first() else {
                return false;
            };
            b = rest;
            if i == 0 && c == 0x80 {
                return false;
            }
            ret = (ret << 7) | u32::from(c & 0x7F);
            if c & 0x80 == 0 {
                break;
            }
            i += 1;
        }
    }
    true
}
fn valid_policy_oid(der: &[u8]) -> bool {
    if der.is_empty() || der[der.len() - 1] & 0x80 != 0 {
        return false;
    }
    let mut start = 0;
    for (i, &v) in der.iter().enumerate() {
        if i == start && v == 0x80 {
            return false;
        }
        if v & 0x80 == 0 {
            start = i + 1;
        }
    }
    true
}
fn right_align(padding: u8, bytes: &[u8]) -> Vec<u8> {
    if padding == 0 || bytes.is_empty() {
        return bytes.to_vec();
    }
    let shift = 8 - padding;
    let mut out = vec![0u8; bytes.len()];
    out[0] = bytes[0] >> padding;
    for i in 1..bytes.len() {
        out[i] = bytes[i - 1] << shift | bytes[i] >> padding;
    }
    out
}
fn is_printable(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || (b'\''..=b')').contains(&b)
        || (b'+'..=b'/').contains(&b)
        || matches!(b, b' ' | b':' | b'=' | b'?' | b'*' | b'&')
}
fn is_ia5(b: &[u8]) -> bool {
    b.iter().all(|&c| c < 0x80)
}
fn valid_string(tag: u8, value: &[u8]) -> bool {
    match tag {
        0x14 => true,
        0x13 => value.iter().all(|&b| is_printable(b)),
        0x0C => std::str::from_utf8(value).is_ok(),
        0x1E => {
            if !value.len().is_multiple_of(2) {
                return false;
            }
            let mut v = value;
            if v.len() >= 2 && v[v.len() - 1] == 0 && v[v.len() - 2] == 0 {
                v = &v[..v.len() - 2];
            }
            v.as_chunks::<2>().0.iter().all(|p| {
                let point = u16::from(p[0]) << 8 | u16::from(p[1]);
                !(point == 0xFFFE
                    || point == 0xFFFF
                    || (0xFDD0..=0xFDEF).contains(&point)
                    || (0xD800..=0xDFFF).contains(&point))
            })
        }
        0x16 => is_ia5(value),
        0x12 => value.iter().all(|&b| b.is_ascii_digit() || b == b' '),
        _ => false,
    }
}
fn name(raw: &[u8]) -> Check {
    let mut raw = S(raw);
    let Some(body) = raw.read(SEQUENCE) else {
        return Err("x509: invalid RDNSequence");
    };
    let mut rdns = S(body);
    while !rdns.empty() {
        let Some(set) = rdns.read(SET) else {
            return Err("x509: invalid RDNSequence");
        };
        let mut set = S(set);
        while !set.empty() {
            let Some(atav) = set.read(SEQUENCE) else {
                return Err("x509: invalid RDNSequence: invalid attribute");
            };
            let mut atav = S(atav);
            atav.oid()
                .ok_or("x509: invalid RDNSequence: invalid attribute type")?;
            let (tag, value, _) = atav
                .any()
                .ok_or("x509: invalid RDNSequence: invalid attribute value")?;
            if !valid_string(tag, value) {
                return Err("x509: invalid RDNSequence: invalid attribute value");
            }
        }
    }
    Ok(())
}
fn algorithm(der: &[u8]) -> Check<(&[u8], &[u8])> {
    let mut der = S(der);
    let oid = der.oid().ok_or("x509: malformed OID")?;
    if der.empty() {
        return Ok((oid, &[]));
    }
    let (_, _, params) = der.any().ok_or("x509: malformed parameters")?;
    Ok((oid, params))
}
fn days_in(month: u32, year: i64) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
fn two(b: &[u8]) -> Option<u32> {
    match b {
        [a, c] if a.is_ascii_digit() && c.is_ascii_digit() => {
            Some(u32::from(a - b'0') * 10 + u32::from(c - b'0'))
        }
        _ => None,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FixedZoneTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub offset_seconds: i32,
}
fn zone_and_fields(fields: &[(u32, u32, u32)], zone: &[u8]) -> Option<i32> {
    if fields.iter().any(|&(v, lo, hi)| v < lo || v > hi) {
        return None;
    }
    match zone {
        [b'Z'] => Some(0),
        [sign, h1, h2, m1, m2] if *sign == b'+' || *sign == b'-' => {
            let (hr, mm) = (two(&[*h1, *h2])?, two(&[*m1, *m2])?);
            if hr > 24 || mm >= 60 || hr * 60 + mm == 0 {
                return None;
            }
            let offset = (hr * 3600 + mm * 60) as i32;
            Some(if *sign == b'-' { -offset } else { offset })
        }
        _ => None,
    }
}
pub(super) fn parse_utc_time(t: &[u8]) -> Option<FixedZoneTime> {
    let date = |t: &[u8], with_seconds: bool| -> Option<FixedZoneTime> {
        let n = if with_seconds { 12 } else { 10 };
        if t.len() < n {
            return None;
        }
        let year = two(&t[0..2])?;
        let (month, day, hour, minute) = (
            two(&t[2..4])?,
            two(&t[4..6])?,
            two(&t[6..8])?,
            two(&t[8..10])?,
        );
        let second = if with_seconds { two(&t[10..12])? } else { 0 };
        let parsed_year = i64::from(if year >= 69 { 1900 + year } else { 2000 + year });
        if !(1..=12).contains(&month) {
            return None;
        }
        let offset_seconds = zone_and_fields(
            &[
                (day, 1, days_in(month, parsed_year)),
                (hour, 0, 23),
                (minute, 0, 59),
                (second, 0, 59),
            ],
            &t[n..],
        )?;
        Some(FixedZoneTime {
            year: if parsed_year >= 2050 {
                parsed_year - 100
            } else {
                parsed_year
            },
            month,
            day,
            hour,
            minute,
            second,
            offset_seconds,
        })
    };
    date(t, true).or_else(|| date(t, false))
}
fn valid_utc_time(t: &[u8]) -> bool {
    parse_utc_time(t).is_some()
}
pub(super) fn parse_generalized_time(t: &[u8]) -> Option<FixedZoneTime> {
    if t.len() < 14 || !t[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let year: i64 = t[..4].iter().fold(0, |v, &d| v * 10 + i64::from(d - b'0'));
    let (month, day, hour, minute, second) = (
        two(&t[4..6])?,
        two(&t[6..8])?,
        two(&t[8..10])?,
        two(&t[10..12])?,
        two(&t[12..14])?,
    );
    if !(1..=12).contains(&month) {
        return None;
    }
    let rest = &t[14..];
    let offset_seconds = zone_and_fields(
        &[
            (day, 1, days_in(month, year)),
            (hour, 0, 23),
            (minute, 0, 59),
            (second, 0, 59),
        ],
        rest,
    )?;
    Some(FixedZoneTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        offset_seconds,
    })
}
fn valid_generalized_time(t: &[u8]) -> bool {
    parse_generalized_time(t).is_some()
}
fn time<'a>(der: &mut S<'a>) -> Check<(u8, &'a [u8])> {
    if der.peek(UTC_TIME) {
        let t = der.read(UTC_TIME).ok_or("x509: malformed UTCTime")?;
        return valid_utc_time(t)
            .then_some((UTC_TIME, t))
            .ok_or("x509: malformed UTCTime");
    }
    if der.peek(GENERALIZED_TIME) {
        let t = der
            .read(GENERALIZED_TIME)
            .ok_or("x509: malformed GeneralizedTime")?;
        return valid_generalized_time(t)
            .then_some((GENERALIZED_TIME, t))
            .ok_or("x509: malformed GeneralizedTime");
    }
    Err("x509: unsupported time format")
}
fn positive(neg: bool, body: &[u8]) -> bool {
    !neg && body.iter().any(|&b| b != 0)
}
fn public_key(oid: &[u8], params: &[u8], data: &[u8]) -> Check {
    match oid {
        OID_RSA => {
            if params != [0x05, 0x00] {
                return Err("x509: RSA key missing NULL parameters");
            }
            let mut der = S(data);
            let body = der.read(SEQUENCE).ok_or("x509: invalid RSA public key")?;
            let mut der = S(body);
            let (n_neg, n) = der.big_integer().ok_or("x509: invalid RSA modulus")?;
            let e = der
                .int64(INTEGER)
                .ok_or("x509: invalid RSA public exponent")?;
            if !positive(n_neg, n) {
                return Err("x509: RSA modulus is not a positive number");
            }
            if e <= 0 {
                return Err("x509: RSA public exponent is not a positive number");
            }
            Ok(())
        }
        OID_EC => {
            let mut p = S(params);
            let named = p.oid().ok_or("x509: invalid ECDSA parameters")?;
            let curve = match named {
                OID_P224 => Curve::P224,
                OID_P256 => Curve::P256,
                OID_P384 => Curve::P384,
                OID_P521 => Curve::P521,
                _ => return Err("x509: unsupported elliptic curve"),
            };
            let size = curve.byte_len();
            match data.split_first() {
                Some((&4, xy)) if xy.len() == 2 * size => {
                    let x = BigUint::from_bytes_be(&xy[..size]);
                    let y = BigUint::from_bytes_be(&xy[size..]);
                    curve
                        .on_curve(&x, &y)
                        .then_some(())
                        .ok_or("invalid public key")
                }
                _ => Err("invalid public key"),
            }
        }
        OID_ED25519 => {
            if !params.is_empty() {
                return Err("x509: Ed25519 key encoded with illegal parameters");
            }
            (data.len() == 32)
                .then_some(())
                .ok_or("x509: wrong Ed25519 public key size")
        }
        OID_DSA => {
            let mut der = S(data);
            let (y_neg, y) = der.big_integer().ok_or("x509: invalid DSA public key")?;
            let mut p = S(params);
            let bad = "x509: invalid DSA parameters";
            let body = p.read(SEQUENCE).ok_or(bad)?;
            let mut p = S(body);
            let pp = p.big_integer().ok_or(bad)?;
            let q = p.big_integer().ok_or(bad)?;
            let g = p.big_integer().ok_or(bad)?;
            if !positive(y_neg, y)
                || !positive(pp.0, pp.1)
                || !positive(q.0, q.1)
                || !positive(g.0, g.1)
            {
                return Err("x509: zero or negative DSA parameter");
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn domain_name_valid(s: &[u8], constraint: bool) -> bool {
    if s.is_empty() {
        return true;
    }
    if s[s.len() - 1] == b'.' {
        return false;
    }
    let s = if constraint && s[0] == b'.' {
        &s[1..]
    } else {
        s
    };
    let mut last_dot: isize = -1;
    for i in 0..=s.len() {
        if i < s.len() && !(33..=126).contains(&s[i]) {
            return false;
        }
        if i == s.len() || s[i] == b'.' {
            let label = i as isize - (last_dot + 1);
            if label == 0 {
                return false;
            }
            last_dot = i as isize;
        }
    }
    true
}
fn reverse_labels_valid(domain: &[u8]) -> bool {
    if domain.is_empty() {
        return true;
    }
    if domain[domain.len() - 1] == b'.' {
        return false;
    }
    domain
        .split(|&b| b == b'.')
        .all(|label| !label.is_empty() && label.iter().all(|c| (33..=126).contains(c)))
}
fn rfc2821_mailbox(input: &[u8]) -> bool {
    if input.is_empty() {
        return false;
    }
    let mut local = Vec::new();
    let mut i = 0;
    if input[0] == b'"' {
        i = 1;
        loop {
            let Some(&c) = input.get(i) else {
                return false;
            };
            i += 1;
            if c == b'"' {
                break;
            }
            if c == b'\\' {
                let Some(&n) = input.get(i) else {
                    return false;
                };
                if n == 11 || n == 12 || (1..=9).contains(&n) || (14..=127).contains(&n) {
                    local.push(n);
                    i += 1;
                } else {
                    return false;
                }
                continue;
            }
            if c == 11
                || c == 12
                || c == 32
                || c == 33
                || c == 127
                || (1..=8).contains(&c)
                || (14..=31).contains(&c)
                || (35..=91).contains(&c)
                || (93..=126).contains(&c)
            {
                local.push(c);
            } else {
                return false;
            }
        }
    } else {
        while let Some(&c) = input.get(i) {
            if c == b'\\' {
                i += 1;
                let Some(&n) = input.get(i) else {
                    return false;
                };
                local.push(n);
                i += 1;
                continue;
            }
            if c.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&c) {
                local.push(c);
                i += 1;
            } else {
                break;
            }
        }
        if local.is_empty()
            || local[0] == b'.'
            || local[local.len() - 1] == b'.'
            || local.windows(2).any(|w| w == b"..")
        {
            return false;
        }
    }
    if input.get(i) != Some(&b'@') {
        return false;
    }
    reverse_labels_valid(&input[i + 1..])
}
fn name_constraints(value: &[u8]) -> Check {
    let invalid = "x509: invalid NameConstraints extension";
    let mut outer = S(value);
    let top = outer.read(SEQUENCE).ok_or(invalid)?;
    if !outer.empty() {
        return Err(invalid);
    }
    let mut top = S(top);
    let permitted = top.optional(0xA0).ok_or(invalid)?;
    let excluded = top.optional(0xA1).ok_or(invalid)?;
    if !top.empty() {
        return Err(invalid);
    }
    if permitted.is_none() && excluded.is_none()
        || permitted.unwrap_or_default().is_empty() && excluded.unwrap_or_default().is_empty()
    {
        return Err("x509: empty name constraints extension");
    }
    for subtrees in [permitted, excluded].into_iter().flatten() {
        let mut subtrees = S(subtrees);
        while !subtrees.empty() {
            let seq = subtrees.read(SEQUENCE).ok_or(invalid)?;
            let (tag, value, _) = S(seq).any().ok_or(invalid)?;
            match tag {
                0x82 => {
                    if !is_ia5(value) {
                        return Err("x509: invalid constraint value");
                    }
                    if !domain_name_valid(value, true) {
                        return Err("x509: failed to parse dnsName constraint");
                    }
                }
                0x87 => {
                    let mask = match value.len() {
                        8 => &value[4..],
                        32 => &value[16..],
                        _ => return Err("x509: IP constraint contained value of length"),
                    };
                    let mut seen_zero = false;
                    for &b in mask {
                        if seen_zero {
                            if b != 0 {
                                return Err("x509: IP constraint contained invalid mask");
                            }
                            continue;
                        }
                        match b {
                            0x00 | 0x80 | 0xC0 | 0xE0 | 0xF0 | 0xF8 | 0xFC | 0xFE => {
                                seen_zero = true
                            }
                            0xFF => {}
                            _ => return Err("x509: IP constraint contained invalid mask"),
                        }
                    }
                }
                0x81 => {
                    if !is_ia5(value) {
                        return Err("x509: invalid constraint value");
                    }
                    let ok = if value.contains(&b'@') {
                        rfc2821_mailbox(value)
                    } else {
                        domain_name_valid(value, true)
                    };
                    if !ok {
                        return Err("x509: failed to parse rfc822Name constraint");
                    }
                }
                0x86 => {
                    if !is_ia5(value) {
                        return Err("x509: invalid constraint value");
                    }
                    let text = std::str::from_utf8(value).unwrap_or_default();
                    if text.parse::<std::net::IpAddr>().is_ok() {
                        return Err("x509: failed to parse URI constraint: cannot be IP address");
                    }
                    if !domain_name_valid(value, true) {
                        return Err("x509: failed to parse URI constraint");
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn subject_alt_names(value: &[u8]) -> Check {
    let mut der = S(value);
    let body = der
        .read(SEQUENCE)
        .ok_or("x509: invalid subject alternative names")?;
    let mut names = S(body);
    while !names.empty() {
        let (tag, data, _) = names
            .any()
            .ok_or("x509: invalid subject alternative name")?;
        match tag ^ 0x80 {
            1 if !is_ia5(data) => return Err("x509: SAN rfc822Name is malformed"),
            2 if !is_ia5(data) => return Err("x509: SAN dNSName is malformed"),
            6 => {
                if !is_ia5(data) {
                    return Err("x509: SAN uniformResourceIdentifier is malformed");
                }
                let text = std::str::from_utf8(data).unwrap_or_default();
                let uri = crate::net::dicomweb::urlrules::parse(text)
                    .map_err(|_| "x509: cannot parse URI")?;
                if !uri.host.is_empty() && !domain_name_valid(uri.host.as_bytes(), false) {
                    return Err("x509: cannot parse URI: invalid domain");
                }
            }
            7 if data.len() != 4 && data.len() != 16 => {
                return Err("x509: cannot parse IP address");
            }
            _ => {}
        }
    }
    Ok(())
}
fn process_extension(id: &[u8], critical: bool, value: &[u8]) -> Check {
    if id.len() == 3 && id[0] == 0x55 && id[1] == 0x1D {
        let mut val = S(value);
        match id[2] {
            15 => {
                val.bit_string().ok_or("x509: invalid key usage")?;
            }
            19 => {
                let bad = "x509: invalid basic constraints";
                let mut der = S(val.read(SEQUENCE).ok_or(bad)?);
                if der.peek(BOOLEAN) {
                    der.boolean().ok_or(bad)?;
                }
                if der.peek(INTEGER) {
                    let mpl = der.uint64().ok_or(bad)?;
                    if mpl > i64::MAX as u64 {
                        return Err(bad);
                    }
                }
            }
            17 => subject_alt_names(value)?,
            30 => name_constraints(value)?,
            31 => {
                let mut points = S(val
                    .read(SEQUENCE)
                    .ok_or("x509: invalid CRL distribution points")?);
                let bad = "x509: invalid CRL distribution point";
                while !points.empty() {
                    let mut dp = S(points.read(SEQUENCE).ok_or(bad)?);
                    let Some(name) = dp.optional(0xA0).ok_or(bad)? else {
                        continue;
                    };
                    let mut name = S(name);
                    let mut full = S(name.read(0xA0).ok_or(bad)?);
                    while !full.empty() {
                        if !full.peek(0x86) {
                            break;
                        }
                        full.read(0x86).ok_or(bad)?;
                    }
                }
            }
            35 => {
                if critical {
                    return Err("x509: authority key identifier incorrectly marked critical");
                }
                let bad = "x509: invalid authority key identifier";
                let mut akid = S(val.read(SEQUENCE).ok_or(bad)?);
                if akid.peek(0x80) {
                    akid.read(0x80).ok_or(bad)?;
                }
            }
            36 => {
                let bad = "x509: invalid policy constraints extension";
                let mut pc = S(val.read(SEQUENCE).ok_or(bad)?);
                for tag in [0x80, 0x81] {
                    if pc.peek(tag) {
                        pc.int64(tag).ok_or(bad)?;
                    }
                }
            }
            37 => {
                let bad = "x509: invalid extended key usages";
                let mut usages = S(val.read(SEQUENCE).ok_or(bad)?);
                while !usages.empty() {
                    usages.oid().ok_or(bad)?;
                }
            }
            14 => {
                if critical {
                    return Err("x509: subject key identifier incorrectly marked critical");
                }
                val.read(OCTET_STRING)
                    .ok_or("x509: invalid subject key identifier")?;
            }
            32 => {
                let bad = "x509: invalid certificate policies";
                let mut policies = S(val.read(SEQUENCE).ok_or(bad)?);
                let mut seen: Vec<&[u8]> = Vec::new();
                while !policies.empty() {
                    let mut cp = S(policies.read(SEQUENCE).ok_or(bad)?);
                    let oid = cp.read(OID).ok_or(bad)?;
                    if seen.contains(&oid) {
                        return Err(bad);
                    }
                    seen.push(oid);
                    if !valid_policy_oid(oid) {
                        return Err(bad);
                    }
                }
            }
            33 => {
                let bad = "x509: invalid policy mappings extension";
                let mut maps = S(val.read(SEQUENCE).ok_or(bad)?);
                while !maps.empty() {
                    let mut s = S(maps.read(SEQUENCE).ok_or(bad)?);
                    s.read(OID).ok_or(bad)?;
                    s.read(OID).ok_or(bad)?;
                }
            }
            54 => {
                val.int64(INTEGER)
                    .ok_or("x509: invalid inhibit any policy extension")?;
            }
            _ => {}
        }
    } else if id == OID_AIA {
        if critical {
            return Err("x509: authority info access incorrectly marked critical");
        }
        let bad = "x509: invalid authority info access";
        let mut val = S(value);
        let mut list = S(val.read(SEQUENCE).ok_or(bad)?);
        while !list.empty() {
            let mut aia = S(list.read(SEQUENCE).ok_or(bad)?);
            aia.oid().ok_or(bad)?;
            if !aia.peek(0x86) {
                continue;
            }
            aia.read(0x86).ok_or(bad)?;
        }
    }
    Ok(())
}
pub(super) struct CertFields<'a> {
    pub raw: &'a [u8],
    pub serial: &'a [u8],
    pub issuer: &'a [u8],
    pub validity: [(u8, &'a [u8]); 2],
    pub key_algorithm: (&'a [u8], &'a [u8]),
    pub key: Vec<u8>,
    pub extensions: Vec<(&'a [u8], bool, &'a [u8])>,
}
pub(super) fn parse_cert_fields(der: &[u8]) -> Check<CertFields<'_>> {
    let mut input = S(der);
    let raw = input
        .element(SEQUENCE)
        .ok_or("x509: malformed certificate")?;
    let mut input = S(S(raw).read(SEQUENCE).ok_or("x509: malformed certificate")?);
    let tbs = input
        .element(SEQUENCE)
        .ok_or("x509: malformed tbs certificate")?;
    let mut tbs = S(S(tbs)
        .read(SEQUENCE)
        .ok_or("x509: malformed tbs certificate")?);
    let mut version: i64 = 0;
    if tbs.peek(0xA0) {
        let explicit = tbs.read(0xA0).ok_or("x509: malformed version")?;
        let mut inner = S(explicit);
        version = inner.int64(INTEGER).ok_or("x509: malformed version")?;
        if !inner.empty() {
            return Err("x509: malformed version");
        }
    }
    if version < 0 {
        return Err("x509: malformed version");
    }
    let version = version + 1;
    if version > 3 {
        return Err("x509: invalid version");
    }
    let (negative, serial) = tbs.big_integer().ok_or("x509: malformed serial number")?;
    if negative {
        return Err("x509: negative serial number");
    }
    let sig_ai = tbs
        .read(SEQUENCE)
        .ok_or("x509: malformed signature algorithm identifier")?;
    let outer_ai = input
        .read(SEQUENCE)
        .ok_or("x509: malformed algorithm identifier")?;
    if outer_ai != sig_ai {
        return Err("x509: inner and outer signature algorithm identifiers don't match");
    }
    algorithm(sig_ai)?;
    let issuer = tbs.element(SEQUENCE).ok_or("x509: malformed issuer")?;
    name(issuer)?;
    let mut validity = S(tbs.read(SEQUENCE).ok_or("x509: malformed validity")?);
    let validity = [time(&mut validity)?, time(&mut validity)?];
    let subject = tbs.element(SEQUENCE).ok_or("x509: malformed issuer")?;
    name(subject)?;
    let spki = tbs.element(SEQUENCE).ok_or("x509: malformed spki")?;
    let mut spki = S(S(spki).read(SEQUENCE).ok_or("x509: malformed spki")?);
    let pk_ai = spki
        .read(SEQUENCE)
        .ok_or("x509: malformed public key algorithm identifier")?;
    let (pk_oid, pk_params) = algorithm(pk_ai)?;
    let (padding, key) = spki
        .bit_string()
        .ok_or("x509: malformed subjectPublicKey")?;
    let key = right_align(padding, key);
    if [OID_RSA, OID_DSA, OID_EC, OID_ED25519].contains(&pk_oid) {
        public_key(pk_oid, pk_params, &key)?;
    }
    let mut parsed_extensions = Vec::new();
    if version > 1 {
        let skip = |tbs: &mut S<'_>, tag: u8| -> bool { !tbs.peek(tag) || tbs.any().is_some() };
        if !skip(&mut tbs, 0x81) {
            return Err("x509: malformed issuerUniqueID");
        }
        if !skip(&mut tbs, 0x82) {
            return Err("x509: malformed subjectUniqueID");
        }
        if version == 3 {
            let present = tbs.optional(0xA3).ok_or("x509: malformed extensions")?;
            if let Some(extensions) = present {
                let mut list = S(S(extensions)
                    .read(SEQUENCE)
                    .ok_or("x509: malformed extensions")?);
                let mut parsed: Vec<(&[u8], bool, &[u8])> = Vec::new();
                while !list.empty() {
                    let mut ext = S(list.read(SEQUENCE).ok_or("x509: malformed extension")?);
                    let id = ext.oid().ok_or("x509: malformed extension OID field")?;
                    let mut critical = false;
                    if ext.peek(BOOLEAN) {
                        critical = ext
                            .boolean()
                            .ok_or("x509: malformed extension critical field")?;
                    }
                    let value = ext
                        .read(OCTET_STRING)
                        .ok_or("x509: malformed extension value field")?;
                    if parsed.iter().any(|(seen, _, _)| *seen == id) {
                        return Err("x509: certificate contains duplicate extension");
                    }
                    parsed.push((id, critical, value));
                }
                for &(id, critical, value) in &parsed {
                    process_extension(id, critical, value)?;
                }
                parsed_extensions = parsed;
            }
        }
    }
    input.bit_string().ok_or("x509: malformed signature")?;
    Ok(CertFields {
        raw,
        serial,
        issuer,
        validity,
        key_algorithm: (pk_oid, pk_params),
        key,
        extensions: parsed_extensions,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn times_round_trip_as_go() {
        assert!(valid_utc_time(b"250101120000Z"));
        assert!(valid_utc_time(b"2501011200Z"));
        assert!(!valid_utc_time(b"250101120000+0000"));
        assert!(valid_utc_time(b"250101120000+0130"));
        assert!(!valid_utc_time(b"250230120000Z"));
        assert!(valid_utc_time(b"240229120000Z"));
        assert!(!valid_utc_time(b"250101126000Z"));
        assert!(valid_generalized_time(b"20250101120000Z"));
        assert!(!valid_generalized_time(b"20250101120000.5Z"));
        assert_eq!(
            parse_utc_time(b"500101120000-0130").map(|t| (t.year, t.offset_seconds)),
            Some((1950, -5400))
        );
        assert!(!valid_generalized_time(b"20250101120000.50Z"));
        assert!(!valid_generalized_time(b"20250101120000,5Z"));
        assert!(!valid_generalized_time(b"2025010112000Z"));
    }
    #[test]
    fn oids_and_integers() {
        assert!(valid_oid(&[0x2A, 0x86, 0x48]));
        assert!(!valid_oid(&[0x2A, 0x86]));
        assert!(!valid_oid(&[0x80, 0x01]));
        assert!(!valid_integer(&[0x00, 0x01]));
        assert!(valid_integer(&[0x00, 0x80]));
    }
}
