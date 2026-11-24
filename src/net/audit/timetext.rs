#![forbid(unsafe_code)]
use super::Time;
pub(crate) fn format_rfc3339_nano(t: &Time) -> String {
    let (y, mo, d) = t.date();
    let mut s = format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year_digits(y),
        mo,
        d,
        t.hour(),
        t.minute(),
        t.second()
    );
    let nanos = t.nanosecond();
    if nanos != 0 {
        let frac = format!("{nanos:09}");
        s.push('.');
        s.push_str(frac.trim_end_matches('0'));
    }
    push_zone(&mut s, t.offset());
    s
}
pub(crate) fn format_syslog_utc(t: &Time) -> String {
    let u = Time::from_unix_utc(t.unix(), t.nanosecond());
    let (y, mo, d) = u.date();
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        year_digits(y),
        mo,
        d,
        u.hour(),
        u.minute(),
        u.second(),
        u.nanosecond() / 1000
    )
}
fn year_digits(y: i64) -> String {
    if y < 0 {
        format!("-{:04}", -y)
    } else {
        format!("{y:04}")
    }
}
fn push_zone(s: &mut String, offset: i64) {
    let minutes = offset / 60;
    if minutes == 0 {
        s.push('Z');
        return;
    }
    let (sign, m) = if minutes < 0 {
        ('-', -minutes)
    } else {
        ('+', minutes)
    };
    s.push_str(&format!("{sign}{:02}:{:02}", m / 60, m % 60));
}
fn days_in(month: i64, year: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
fn digits(b: &[u8], from: usize, n: usize) -> Option<i64> {
    let part = b.get(from..from + n)?;
    let mut v = 0i64;
    for c in part {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + i64::from(c - b'0');
    }
    Some(v)
}
pub(crate) fn parse_rfc3339(value: &str) -> Option<Time> {
    parse(value, false)
}
pub(crate) fn parse_rfc3339_json(value: &str) -> Option<Time> {
    parse(value, false)
}
fn parse(value: &str, strict: bool) -> Option<Time> {
    let b = value.as_bytes();
    let year = digits(b, 0, 4)?;
    if b.get(4) != Some(&b'-') {
        return None;
    }
    let month = digits(b, 5, 2)?;
    if b.get(7) != Some(&b'-') {
        return None;
    }
    let day = digits(b, 8, 2)?;
    if b.get(10) != Some(&b'T') {
        return None;
    }
    let mut i = 11;
    let hour = if b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        let h = digits(b, i, 2)?;
        i += 2;
        h
    } else {
        if strict {
            return None;
        }
        let h = digits(b, i, 1)?;
        i += 1;
        h
    };
    if b.get(i) != Some(&b':') {
        return None;
    }
    let minute = digits(b, i + 1, 2)?;
    if b.get(i + 3) != Some(&b':') {
        return None;
    }
    let second = digits(b, i + 4, 2)?;
    i += 6;
    let mut nanos = 0i64;
    if let (Some(sep), Some(d)) = (b.get(i), b.get(i + 1))
        && (*sep == b'.' || (*sep == b',' && !strict))
        && d.is_ascii_digit()
    {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let mut scale = 100_000_000i64;
        for c in &b[start..i.min(start + 9)] {
            nanos += i64::from(c - b'0') * scale;
            scale /= 10;
        }
    }
    let offset;
    if b.get(i) == Some(&b'Z') {
        offset = 0;
        i += 1;
    } else {
        let sign = match b.get(i)? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let zh = digits(b, i + 1, 2)?;
        if b.get(i + 3) != Some(&b':') {
            return None;
        }
        let zm = digits(b, i + 4, 2)?;
        let (max_h, max_m) = if strict { (23, 59) } else { (24, 60) };
        if zh > max_h || zm > max_m {
            return None;
        }
        offset = sign * (zh * 3600 + zm * 60);
        i += 6;
    }
    if i != b.len() {
        return None;
    }
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in(month, year)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }
    Some(Time::from_fields(
        year, month, day, hour, minute, second, nanos, offset,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        for s in [
            "2026-01-02T03:04:05Z",
            "2026-01-02T03:04:05.5Z",
            "2026-01-02T03:04:05.123456789+05:30",
            "2024-02-29T23:59:59-08:00",
        ] {
            let t = parse_rfc3339(s).unwrap();
            assert_eq!(format_rfc3339_nano(&t), s);
        }
        assert!(parse_rfc3339("2025-02-29T00:00:00Z").is_none());
        assert!(parse_rfc3339("2026-01-02T3:04:05Z").is_some());
        assert!(parse_rfc3339_json("2026-01-02T3:04:05Z").is_some());
        assert!(parse_rfc3339("2026-01-02T03:04:05,5Z").is_some());
        assert!(parse_rfc3339("2026-01-02T03:04:05").is_none());
    }
}
