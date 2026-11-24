use super::Error;
use crate::object::Vr;
use crate::textfmt::quote;
#[derive(Debug, Clone, Copy)]
pub(super) struct FixedZoneTime {
    unix: i64,
    nsec: i64,
    offset: i32,
}
fn floor_div(a: i64, b: i64) -> (i64, i64) {
    let q = a.div_euclid(b);
    (q, a - q * b)
}
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
fn norm(hi: i64, lo: i64, base: i64) -> (i64, i64) {
    let (carry, lo) = floor_div(lo, base);
    (hi + carry, lo)
}
impl FixedZoneTime {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn date(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        min: i64,
        sec: i64,
        nsec: i64,
        offset: i32,
    ) -> FixedZoneTime {
        let (year, m) = norm(year, month - 1, 12);
        let (sec, nsec) = norm(sec, nsec, 1_000_000_000);
        let (min, sec) = norm(min, sec, 60);
        let (hour, min) = norm(hour, min, 60);
        let (day, hour) = norm(day, hour, 24);
        let days = days_from_civil(year, m + 1, 1) + day - 1;
        let local = days * 86_400 + hour * 3600 + min * 60 + sec;
        FixedZoneTime {
            unix: local - i64::from(offset),
            nsec,
            offset,
        }
    }
    fn fields(&self) -> (i64, i64, i64, i64, i64, i64, i64) {
        let (days, secs) = floor_div(self.unix + i64::from(self.offset), 86_400);
        let (y, m, d) = civil_from_days(days);
        (y, m, d, secs / 3600, secs % 3600 / 60, secs % 60, self.nsec)
    }
    fn key(&self) -> (i64, i64) {
        (self.unix, self.nsec)
    }
    pub(super) fn before(&self, other: &FixedZoneTime) -> bool {
        self.key() < other.key()
    }
    pub(super) fn after(&self, other: &FixedZoneTime) -> bool {
        self.key() > other.key()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Precision {
    Full,
    Fraction(i64),
    Seconds,
    Minutes,
    Hours,
    Day,
    Month,
    Year,
}
fn digits(s: &[u8]) -> Option<i64> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0, |n, &b| n * 10 + i64::from(b - b'0')))
}
fn last_day(year: i64, month: i64) -> i64 {
    FixedZoneTime::date(year, month + 1, 0, 0, 0, 0, 0, 0)
        .fields()
        .2
}
fn valid_date(year: i64, month: i64, day: i64) -> bool {
    (1..=12).contains(&month) && day >= 1 && day <= last_day(year, month)
}
fn valid_time(hours: i64, minutes: i64, seconds: Option<i64>, fraction: bool) -> bool {
    (0..=23).contains(&hours)
        && (0..=59).contains(&minutes)
        && (0..=60).contains(&seconds.unwrap_or(0))
        && (!fraction || seconds.is_some())
}
fn fraction(frac: &[u8]) -> Option<(i64, Precision)> {
    if frac.is_empty() || frac.len() > 6 {
        return None;
    }
    let value = digits(frac)?;
    let missing = 9 - frac.len() as i64;
    let precision = match missing - 3 {
        0 => Precision::Full,
        n => Precision::Fraction(n),
    };
    Some((value * 10_i64.pow(missing as u32), precision))
}
struct ParsedDate {
    time: FixedZoneTime,
    precision: Precision,
    nema: bool,
}
fn parse_date(value: &str) -> Option<ParsedDate> {
    let b = value.as_bytes();
    let all_digits = b.iter().all(u8::is_ascii_digit);
    let (parts, nema): (Vec<&[u8]>, bool) = if all_digits && matches!(b.len(), 4 | 6 | 8) {
        (b.chunks(2).collect::<Vec<_>>(), false)
    } else {
        let parts: Vec<&[u8]> = value.split('.').map(str::as_bytes).collect();
        let ok = match parts.len() {
            1..=3 => {
                parts[0].len() == 4
                    && parts[1..].iter().all(|p| p.len() == 2)
                    && parts.iter().all(|p| p.iter().all(u8::is_ascii_digit))
            }
            _ => false,
        };
        if !ok {
            return None;
        }
        let mut chunks = vec![&parts[0][..2], &parts[0][2..]];
        chunks.extend(parts[1..].iter().copied());
        (chunks, true)
    };
    let year = digits(&[parts[0], parts[1]].concat())?;
    let month = parts.get(2).map(|p| digits(p)).unwrap_or(Some(1))?;
    let day = parts.get(3).map(|p| digits(p)).unwrap_or(Some(1))?;
    let precision = match parts.len() {
        2 => Precision::Year,
        3 => Precision::Month,
        _ => Precision::Full,
    };
    if !valid_date(year, month, day) {
        return None;
    }
    Some(ParsedDate {
        time: FixedZoneTime::date(year, month, day, 0, 0, 0, 0, 0),
        precision,
        nema,
    })
}
fn split_fraction(s: &[u8]) -> (&[u8], Option<&[u8]>) {
    match s.iter().position(|&b| b == b'.') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    }
}
type Clock = (i64, i64, Option<i64>, i64, Precision);
fn parse_clock(main: &[u8], frac: Option<&[u8]>, start: Precision) -> Option<Clock> {
    if !main.iter().all(u8::is_ascii_digit) || !main.len().is_multiple_of(2) || main.len() > 6 {
        return None;
    }
    let mut precision = start;
    let mut parts = main.chunks(2);
    let hours = match parts.next() {
        Some(p) => {
            precision = Precision::Hours;
            digits(p)?
        }
        None => 0,
    };
    let minutes = match parts.next() {
        Some(p) => {
            precision = Precision::Minutes;
            digits(p)?
        }
        None => 0,
    };
    let seconds = match parts.next() {
        Some(p) => {
            precision = Precision::Seconds;
            Some(digits(p)?)
        }
        None => None,
    };
    let mut nanos = 0;
    if let Some(frac) = frac {
        let (n, p) = fraction(frac)?;
        nanos = n;
        precision = p;
    }
    Some((hours, minutes, seconds, nanos, precision))
}
fn parse_time(value: &str) -> Option<(FixedZoneTime, Precision)> {
    let (main, frac) = split_fraction(value.as_bytes());
    if main.is_empty() {
        return None;
    }
    let (h, m, s, ns, precision) = parse_clock(main, frac, Precision::Full)?;
    if !valid_time(h, m, s, frac.is_some()) {
        return None;
    }
    Some((
        FixedZoneTime::date(1, 1, 1, h, m, s.unwrap_or(0), ns, 0),
        precision,
    ))
}
struct ParsedDatetime {
    time: FixedZoneTime,
    precision: Precision,
    no_offset: bool,
}
fn parse_datetime(value: &str) -> Option<ParsedDatetime> {
    let b = value.as_bytes();
    let run = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(4..=14).contains(&run) || run % 2 != 0 {
        return None;
    }
    let (date_part, rest) = b.split_at(run);
    let (date_digits, time_digits) = date_part.split_at(run.min(8));
    let (frac, rest) = if rest.first() == Some(&b'.') {
        let n = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        (Some(&rest[1..1 + n]), &rest[1 + n..])
    } else {
        (None, rest)
    };
    let offset = match rest {
        [] => None,
        [sign @ (b'+' | b'-'), h1, h2, m1, m2] => {
            Some((*sign, digits(&[*h1, *h2])?, digits(&[*m1, *m2])?))
        }
        _ => return None,
    };
    let mut chunks = date_digits[4..].chunks(2);
    let year = digits(&date_digits[..4])?;
    let mut precision = Precision::Year;
    let month = match chunks.next() {
        Some(p) => {
            precision = Precision::Month;
            digits(p)?
        }
        None => 1,
    };
    let day = match chunks.next() {
        Some(p) => {
            precision = Precision::Day;
            digits(p)?
        }
        None => 1,
    };
    let (h, m, s, ns, precision) = parse_clock(time_digits, frac, precision)?;
    if !valid_date(year, month, day) || !valid_time(h, m, s, frac.is_some()) {
        return None;
    }
    let mut seconds = 0;
    if let Some((sign, oh, om)) = offset {
        if !(0..=59).contains(&om) {
            return None;
        }
        let valid = if sign == b'+' {
            oh < 14 || oh == 14 && om == 0
        } else {
            !(oh == 0 && om == 0) && (oh < 12 || oh == 12 && om == 0)
        };
        if !valid {
            return None;
        }
        seconds = (oh * 3600 + om * 60) * if sign == b'-' { -1 } else { 1 };
    }
    Some(ParsedDatetime {
        time: FixedZoneTime::date(year, month, day, h, m, s.unwrap_or(0), ns, seconds as i32),
        precision,
        no_offset: offset.is_none(),
    })
}
fn parse_temporal_in(
    vr: Vr,
    value: &str,
    lenient_dates: bool,
    zone: Option<i32>,
) -> Option<(FixedZoneTime, Precision)> {
    match vr {
        Vr::DA => {
            let parsed = parse_date(value)?;
            if !lenient_dates && (parsed.nema || parsed.precision != Precision::Full) {
                return None;
            }
            Some((parsed.time, parsed.precision))
        }
        Vr::TM => parse_time(value),
        Vr::DT => {
            let parsed = parse_datetime(value)?;
            if let Some(zone) = zone
                && parsed.no_offset
            {
                let (y, mo, d, h, mi, s, ns) = parsed.time.fields();
                return Some((
                    FixedZoneTime::date(y, mo, d, h, mi, s, ns, zone),
                    parsed.precision,
                ));
            }
            Some((parsed.time, parsed.precision))
        }
        _ => None,
    }
}
fn date_window(
    value: &FixedZoneTime,
    precision: Precision,
    loc: i32,
) -> (FixedZoneTime, FixedZoneTime) {
    let (year, month, day, ..) = value.fields();
    match precision {
        Precision::Year => (
            FixedZoneTime::date(year, 1, 1, 0, 0, 0, 0, loc),
            FixedZoneTime::date(year, 12, 31, 0, 0, 0, 0, loc),
        ),
        Precision::Month => {
            let start = FixedZoneTime::date(year, month, 1, 0, 0, 0, 0, loc);
            let (sy, sm, sd, sh, smi, ss, sns) = start.fields();
            (
                start,
                FixedZoneTime::date(sy, sm + 1, sd - 1, sh, smi, ss, sns, loc),
            )
        }
        _ => {
            let day = FixedZoneTime::date(year, month, day, 0, 0, 0, 0, loc);
            (day, day)
        }
    }
}
fn time_window(
    value: &FixedZoneTime,
    precision: Precision,
    loc: i32,
) -> (FixedZoneTime, FixedZoneTime) {
    let (.., h, mi, s, ns) = value.fields();
    let at = |h, mi, s, ns| FixedZoneTime::date(1, 1, 1, h, mi, s, ns, loc);
    match precision {
        Precision::Hours => (at(h, 0, 0, 0), at(h, 59, 59, 999_999_000)),
        Precision::Minutes => (at(h, mi, 0, 0), at(h, mi, 59, 999_999_000)),
        Precision::Seconds => (at(h, mi, s, 0), at(h, mi, s, 999_999_000)),
        _ => (at(h, mi, s, ns), at(h, mi, s, ns)),
    }
}
fn temporal_window(
    value: &FixedZoneTime,
    precision: Precision,
    vr: Vr,
) -> (FixedZoneTime, FixedZoneTime) {
    let loc = value.offset;
    match vr {
        Vr::DA => date_window(value, precision, loc),
        Vr::TM => time_window(value, precision, loc),
        _ => {
            let (date_start, date_end) = date_window(value, precision, loc);
            let (sy, sm, sd, ..) = date_start.fields();
            let (ey, em, ed, ..) = date_end.fields();
            if matches!(
                precision,
                Precision::Year | Precision::Month | Precision::Day
            ) {
                return (
                    FixedZoneTime::date(sy, sm, sd, 0, 0, 0, 0, loc),
                    FixedZoneTime::date(ey, em, ed, 23, 59, 59, 999_999_000, loc),
                );
            }
            let (time_start, time_end) = time_window(value, precision, loc);
            let (.., h1, m1, s1, n1) = time_start.fields();
            let (.., h2, m2, s2, n2) = time_end.fields();
            (
                FixedZoneTime::date(sy, sm, sd, h1, m1, s1, n1, loc),
                FixedZoneTime::date(ey, em, ed, h2, m2, s2, n2, loc),
            )
        }
    }
}
fn ranges_overlap(
    a_start: &FixedZoneTime,
    a_end: &FixedZoneTime,
    b_start: &FixedZoneTime,
    b_end: &FixedZoneTime,
) -> bool {
    !a_end.before(b_start) && !b_end.before(a_start)
}
pub(super) fn match_temporal_in(
    vr: Vr,
    query: &str,
    candidate: &str,
    lenient_dates: bool,
    query_zone: Option<i32>,
    candidate_zone: Option<i32>,
) -> Result<bool, Error> {
    let query = query.trim();
    let candidate = candidate.trim();
    if query.is_empty() {
        return Ok(true);
    }
    if query.contains(['*', '?']) {
        return Err(Error::InvalidQuery(format!(
            "wildcard is not allowed for {vr} matching"
        )));
    }
    let Some((candidate_time, candidate_precision)) =
        parse_temporal_in(vr, candidate, true, candidate_zone)
    else {
        return Ok(false);
    };
    let (candidate_start, candidate_end) =
        temporal_window(&candidate_time, candidate_precision, vr);
    let invalid_range = || Error::InvalidQuery(format!("invalid {vr} range"));
    if !query.contains('-') {
        let Some((query_time, query_precision)) =
            parse_temporal_in(vr, query, lenient_dates, query_zone)
        else {
            return Err(Error::InvalidQuery(format!("invalid {vr} matching value")));
        };
        let (query_start, query_end) = temporal_window(&query_time, query_precision, vr);
        return Ok(ranges_overlap(
            &candidate_start,
            &candidate_end,
            &query_start,
            &query_end,
        ));
    }
    if query.matches('-').count() != 1 {
        return Err(invalid_range());
    }
    let (from, to) = query.split_once('-').unwrap_or((query, ""));
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() && to.is_empty() {
        return Err(invalid_range());
    }
    let mut lower = None;
    let mut upper = None;
    if !from.is_empty() {
        let (t, p) =
            parse_temporal_in(vr, from, lenient_dates, query_zone).ok_or_else(invalid_range)?;
        lower = Some(temporal_window(&t, p, vr).0);
    }
    if !to.is_empty() {
        let (t, p) =
            parse_temporal_in(vr, to, lenient_dates, query_zone).ok_or_else(invalid_range)?;
        upper = Some(temporal_window(&t, p, vr).1);
    }
    if let (Some(lower), Some(upper)) = (&lower, &upper)
        && lower.after(upper)
    {
        return Err(invalid_range());
    }
    if let Some(lower) = &lower
        && candidate_end.before(lower)
    {
        return Ok(false);
    }
    if let Some(upper) = &upper
        && candidate_start.after(upper)
    {
        return Ok(false);
    }
    Ok(true)
}
fn day_bounds() -> (FixedZoneTime, FixedZoneTime) {
    (
        FixedZoneTime::date(1, 1, 1, 0, 0, 0, 0, 0),
        FixedZoneTime::date(1, 1, 1, 23, 59, 59, 999_999_999, 0),
    )
}
fn at_time_of_day(day: &FixedZoneTime, clock: &FixedZoneTime, zone: i32) -> FixedZoneTime {
    let (y, m, d, ..) = day.fields();
    let (.., h, mi, s, ns) = clock.fields();
    FixedZoneTime::date(y, m, d, h, mi, s, ns, zone)
}
pub(super) type Window = (Option<FixedZoneTime>, Option<FixedZoneTime>);
pub(super) fn combined_query_window(
    date_query: &str,
    time_query: &str,
    lenient_dates: bool,
    zone: i32,
) -> Result<Window, Error> {
    let invalid = || Error::InvalidQuery("invalid combined DA/TM matching value".to_string());
    let (mut date_lower, mut date_upper, mut time_lower, mut time_upper) =
        (date_query, date_query, time_query, time_query);
    if date_query.contains('-') {
        if date_query.matches('-').count() != 1 || time_query.matches('-').count() != 1 {
            return Err(invalid());
        }
        let (dl, du) = date_query.split_once('-').unwrap_or((date_query, ""));
        let (tl, tu) = time_query.split_once('-').unwrap_or((time_query, ""));
        (date_lower, date_upper) = (dl.trim(), du.trim());
        (time_lower, time_upper) = (tl.trim(), tu.trim());
        if date_lower.is_empty() && date_upper.is_empty() {
            return Err(invalid());
        }
    }
    let (mut time_start, mut time_end) = day_bounds();
    if !time_lower.is_empty() {
        let (value, precision) =
            parse_temporal_in(Vr::TM, time_lower, false, None).ok_or_else(invalid)?;
        time_start = time_window(&value, precision, 0).0;
    }
    if !time_upper.is_empty() {
        let (value, precision) =
            parse_temporal_in(Vr::TM, time_upper, false, None).ok_or_else(invalid)?;
        time_end = time_window(&value, precision, 0).1;
    }
    let mut lower = None;
    let mut upper = None;
    if !date_lower.is_empty() {
        let (value, precision) =
            parse_temporal_in(Vr::DA, date_lower, lenient_dates, None).ok_or_else(invalid)?;
        let day = date_window(&value, precision, 0).0;
        lower = Some(at_time_of_day(&day, &time_start, zone));
    }
    if !date_upper.is_empty() {
        let (value, precision) =
            parse_temporal_in(Vr::DA, date_upper, lenient_dates, None).ok_or_else(invalid)?;
        let day = date_window(&value, precision, 0).1;
        upper = Some(at_time_of_day(&day, &time_end, zone));
    }
    if let (Some(l), Some(u)) = (&lower, &upper)
        && l.after(u)
    {
        return Err(invalid());
    }
    Ok((lower, upper))
}
pub(super) fn candidate_date_time_window(
    date_value: &str,
    time_value: &str,
    zone: i32,
) -> Option<(FixedZoneTime, FixedZoneTime)> {
    let (value, precision) = parse_temporal_in(Vr::DA, date_value, true, None)?;
    let (first_day, last_day) = date_window(&value, precision, 0);
    let (mut time_start, mut time_end) = day_bounds();
    if !time_value.is_empty()
        && let Some((parsed, time_precision)) = parse_temporal_in(Vr::TM, time_value, true, None)
    {
        (time_start, time_end) = time_window(&parsed, time_precision, 0);
    }
    Some((
        at_time_of_day(&first_day, &time_start, zone),
        at_time_of_day(&last_day, &time_end, zone),
    ))
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimezoneOffset {
    pub name: String,
    pub seconds: i32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimezoneOffsetError(String);
impl std::fmt::Display for TimezoneOffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TimezoneOffsetError {}
pub fn parse_timezone_offset(value: &str) -> Result<TimezoneOffset, TimezoneOffsetError> {
    let value = value.trim();
    let b = value.as_bytes();
    if b.len() != 5 || (b[0] != b'+' && b[0] != b'-') {
        return Err(TimezoneOffsetError(format!(
            "dicom qrmatch: timezone offset {} is not &ZZXX",
            quote(value)
        )));
    }
    let hours = digits(&b[1..3]).filter(|&h| h <= 14);
    let Some(hours) = hours else {
        return Err(TimezoneOffsetError(format!(
            "dicom qrmatch: timezone offset {} hours",
            quote(value)
        )));
    };
    let Some(minutes) = digits(&b[3..5]).filter(|&m| m <= 59) else {
        return Err(TimezoneOffsetError(format!(
            "dicom qrmatch: timezone offset {} minutes",
            quote(value)
        )));
    };
    let mut seconds = (hours * 3600 + minutes * 60) as i32;
    if b[0] == b'-' {
        seconds = -seconds;
    }
    Ok(TimezoneOffset {
        name: value.to_string(),
        seconds,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_normalizes() {
        let t = FixedZoneTime::date(2024, 3, 0, 0, 0, 0, 0, 0);
        assert_eq!(t.fields().2, 29);
        let t = FixedZoneTime::date(1, 1, 1, 23, 59, 60, 0, 0);
        assert_eq!(t.fields(), (1, 1, 2, 0, 0, 0, 0));
        let t = FixedZoneTime::date(0, 2, 29, 0, 0, 0, 0, 0);
        assert_eq!(t.fields(), (0, 2, 29, 0, 0, 0, 0));
    }
    #[test]
    fn parses_like_dcmtime() {
        assert!(parse_date("20200101").is_some());
        assert!(parse_date("2020.06.04").unwrap().nema);
        assert!(parse_date("20200230").is_none());
        assert_eq!(parse_time("10").unwrap().1, Precision::Hours);
        assert_eq!(parse_time("100000.5").unwrap().1, Precision::Fraction(5));
        assert!(parse_time("10.5").is_none());
        assert!(parse_datetime("20260604100000-0000").is_none());
        assert!(parse_datetime("20260604100000+1400").is_some());
    }
}
