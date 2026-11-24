use super::Error;
use super::imaging_study::{
    ImagingStudy, Members, decode_imaging_study, decode_list, decode_string, field, object_members,
};
use crate::dicomjson::jsonwire::{Decoder, JsonError, Token};
use crate::net::ul::Ctx;
const IMAGING_STUDY_TYPE: &str = "ImagingStudy";
fn context(ctx: &Ctx) -> Result<(), Error> {
    ctx.err().map_or(Ok(()), |kind| Err(Error::Context(kind)))
}
#[derive(Default)]
struct Link {
    relation: String,
    url: String,
}
fn decode_link(m: &Members) -> Result<Link, ()> {
    Ok(Link {
        relation: decode_string(m, "relation")?,
        url: decode_string(m, "url")?,
    })
}
#[derive(Default)]
struct Entry<'a> {
    mode: Option<String>,
    resource: Option<&'a [u8]>,
}
fn decode_entry<'a>(m: &Members<'a>) -> Result<Entry<'a>, ()> {
    let mode = match field(m, "search")
        .map(object_members)
        .transpose()?
        .flatten()
    {
        Some(search) => Some(decode_string(&search, "mode")?),
        None => None,
    };
    Ok(Entry {
        mode,
        resource: field(m, "resource"),
    })
}
pub(super) fn decode_page(ctx: &Ctx, raw: &[u8]) -> Result<(Vec<ImagingStudy>, String), Error> {
    check_json(ctx, raw)?;
    let members = match object_members(raw) {
        Ok(Some(members)) => members,
        Ok(None) => Members::new(),
        Err(()) => return Err(Error::Response),
    };
    let text = |name| decode_string(&members, name).map_err(|_| Error::Response);
    let resource_type = text("resourceType")?;
    let typ = text("type")?;
    let links = decode_list(field(&members, "link"), decode_link).map_err(|_| Error::Response)?;
    let entries = entry_list(field(&members, "entry")).map_err(|_| Error::Response)?;
    if resource_type != "Bundle" || typ != "searchset" {
        return Err(Error::Response);
    }
    let mut next = String::new();
    for link in links {
        if link.relation != "next" {
            continue;
        }
        if !next.is_empty() || link.url.is_empty() {
            return Err(Error::Response);
        }
        next = link.url;
    }
    let mut studies = Vec::new();
    for entry in entries {
        context(ctx)?;
        match entry.mode.as_deref() {
            Some("include" | "outcome") => continue,
            None | Some("" | "match") => {}
            Some(_) => return Err(Error::Response),
        }
        let study = decode_imaging_study(entry.resource).map_err(|_| Error::Response)?;
        if study.resource_type != IMAGING_STUDY_TYPE
            || !valid_date_time(&study.started)
            || study.series.iter().any(|s| !valid_date_time(&s.started))
        {
            return Err(Error::Response);
        }
        studies.push(study);
    }
    Ok((studies, next))
}
fn entry_list(raw: Option<&[u8]>) -> Result<Vec<Entry<'_>>, ()> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    match raw.first() {
        Some(b'[') => crate::dicomjson::jsonwire::as_raw_array(raw)
            .map_err(|_| ())?
            .into_iter()
            .map(|item| match object_members(item)? {
                Some(m) => decode_entry(&m),
                None => Ok(Entry::default()),
            })
            .collect(),
        Some(b'n') => Ok(Vec::new()),
        _ => Err(()),
    }
}
fn valid_date_time(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let b = value.as_bytes();
    if b.len() < 4 || &b[..4] == b"0000" {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let num =
        |r: std::ops::Range<usize>| b[r].iter().fold(0u32, |n, &c| n * 10 + u32::from(c - b'0'));
    let date_ok = |len: usize| -> bool {
        if !digits(0..4) {
            return false;
        }
        if len == 4 {
            return true;
        }
        if b[4] != b'-' || !digits(5..7) {
            return false;
        }
        let month = num(5..7);
        if !(1..=12).contains(&month) {
            return false;
        }
        if len == 7 {
            return true;
        }
        if b[7] != b'-' || !digits(8..10) {
            return false;
        }
        let day = num(8..10);
        day >= 1 && day <= days_in(month, num(0..4))
    };
    match b.len() {
        4 | 7 | 10 => date_ok(b.len()),
        _ => {
            if !precise_date_time(b) {
                return false;
            }
            date_ok(10) && num(11..13) < 24 && num(14..16) < 60 && {
                let second = num(17..19);
                second <= 60
            }
        }
    }
}
fn days_in(month: u32, year: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
fn precise_date_time(b: &[u8]) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd";
    if b.len() < shape.len() {
        return false;
    }
    for (i, &s) in shape.iter().enumerate() {
        let ok = if s == b'd' {
            b[i].is_ascii_digit()
        } else {
            b[i] == s
        };
        if !ok {
            return false;
        }
    }
    let mut rest = &b[shape.len()..];
    if let Some(fraction) = rest.strip_prefix(b".") {
        let n = fraction.iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return false;
        }
        rest = &fraction[n..];
    }
    match rest {
        b"Z" => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            let hour_ok =
                (*h1 == b'0' && h2.is_ascii_digit()) || (*h1 == b'1' && (b'0'..=b'3').contains(h2));
            (hour_ok && (b'0'..=b'5').contains(m1) && m2.is_ascii_digit())
                || (*h1 == b'1' && *h2 == b'4' && *m1 == b'0' && *m2 == b'0')
        }
        _ => false,
    }
}
fn check_json(ctx: &Ctx, raw: &[u8]) -> Result<(), Error> {
    if std::str::from_utf8(raw).is_err() {
        return Err(Error::Response);
    }
    let mut decoder = Decoder::with_tail(raw, None, true);
    let mut remaining: usize = 500_000;
    scan_json_value(ctx, &mut decoder, 0, &mut remaining)?;
    match decoder.token() {
        Err(JsonError::Eof) => Ok(()),
        _ => Err(Error::Response),
    }
}
fn scan_json_value(
    ctx: &Ctx,
    decoder: &mut Decoder,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), Error> {
    context(ctx)?;
    if depth > 64 || *remaining == 0 {
        return Err(Error::Limit);
    }
    *remaining -= 1;
    let delim = match decoder.token() {
        Err(_) => return Err(Error::Response),
        Ok(Token::Delim(delim)) => delim,
        Ok(_) => return Ok(()),
    };
    match delim {
        b'{' => {
            let mut keys: std::collections::HashSet<String> = std::collections::HashSet::new();
            while decoder.more() {
                let key = match decoder.token() {
                    Ok(Token::Str(key)) if key.len() <= 256 && key.is_ascii() => key,
                    _ => return Err(Error::Response),
                };
                let key = key.to_ascii_uppercase();
                if !keys.insert(key) {
                    return Err(Error::Response);
                }
                scan_json_value(ctx, decoder, depth + 1, remaining)?;
            }
        }
        b'[' => {
            while decoder.more() {
                scan_json_value(ctx, decoder, depth + 1, remaining)?;
            }
        }
        _ => return Err(Error::Response),
    }
    decoder.token().map(|_| ()).map_err(|_| Error::Response)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_times() {
        for ok in [
            "",
            "2026",
            "2024-02-29",
            "2026-11-16T23:59:60Z",
            "2026-11-16T10:15:30.5+14:00",
        ] {
            assert!(valid_date_time(ok), "{ok}");
        }
        for bad in [
            "0000",
            "2026-02-29",
            "2026-11-16T24:00:00Z",
            "2026-11-16T10:15:30+14:01",
            "2026-11-16T10:15:30",
        ] {
            assert!(!valid_date_time(bad), "{bad}");
        }
    }
}
