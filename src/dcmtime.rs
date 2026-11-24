#![forbid(unsafe_code)]
use std::fmt;
mod builder;
pub use builder::ZonedTimeBuilder;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    ParseDA,
    ParseDT,
    ParseTM,
    ParseFullDA,
    ParseValidTM,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::ParseDA => {
                "error parsing dicom DA (date) value -- expected format is 'YYYYMMDD'. \
                 for more details on proper DA value formatting, see here: \
                 https://dicom.nema.org/medical/dicom/current/output/html/part05.html#table_6.2-1"
            }
            Error::ParseDT => {
                "error parsing dicom DT (datetime) value -- expected format is \
                 'YYYYMMDDHHMMSS.FFFFFF&ZZXX'. \
                 for more details on proper DT value formatting, see here: \
                 https://dicom.nema.org/medical/dicom/current/output/html/part05.html#table_6.2-1"
            }
            Error::ParseTM => {
                "error parsing dicom TM (time) value, but expected format is 'HHMMSS.FFFFFF'. \
                 for more details on proper TM value formatting, see here: \
                 https://dicom.nema.org/medical/dicom/current/output/html/part05.html#table_6.2-1"
            }
            Error::ParseFullDA => "date must be a full-precision DICOM DA value",
            Error::ParseValidTM => "time must be a valid DICOM TM value",
        })
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PrecisionLevel(pub i64);
impl PrecisionLevel {
    pub const FULL: Self = Self(0);
    pub const MS5: Self = Self(1);
    pub const MS4: Self = Self(2);
    pub const MS3: Self = Self(3);
    pub const MS2: Self = Self(4);
    pub const MS1: Self = Self(5);
    pub const SECONDS: Self = Self(6);
    pub const MINUTES: Self = Self(7);
    pub const HOURS: Self = Self(8);
    pub const DAY: Self = Self(9);
    pub const MONTH: Self = Self(10);
    pub const YEAR: Self = Self(11);
    pub fn is_renderable(self) -> bool {
        6i64.wrapping_sub(self.0) <= 6
    }
}
impl fmt::Display for PrecisionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            0 => "FULL",
            11 => "YEAR",
            10 => "MONTH",
            9 => "DAY",
            8 => "HOURS",
            7 => "MINUTES",
            6 => "SECONDS",
            5 => "MS1",
            4 => "MS2",
            3 => "MS3",
            2 => "MS4",
            1 => "MS5",
            _ => "!{PRECISION}",
        })
    }
}
fn is_included(check: PrecisionLevel, limit: PrecisionLevel) -> bool {
    check >= limit
}
const SECONDS_PER_DAY: i64 = 86_400;
const ABSOLUTE_YEARS: u64 = 292_277_022_400;
const MARCH_THRU_DECEMBER: u32 = 306;
const INTERNAL_TO_ABSOLUTE: i64 =
    (ABSOLUTE_YEARS as i64 * 365 + ABSOLUTE_YEARS as i64 / 400 * 97 + MARCH_THRU_DECEMBER as i64)
        * SECONDS_PER_DAY;
const UNIX_TO_INTERNAL: i64 = (1969 * 365 + 1969 / 4 - 1969 / 100 + 1969 / 400) * SECONDS_PER_DAY;
const ABSOLUTE_TO_UNIX: i64 = -INTERNAL_TO_ABSOLUTE - UNIX_TO_INTERNAL;
const NSEC_MASK: u64 = (1 << 30) - 1;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ZonedTime {
    sec: i64,
    nsec: i32,
    offset: i64,
}
fn norm(hi: i64, lo: i64, base: i64) -> (i64, i64) {
    let (mut hi, mut lo) = (hi, lo);
    if lo < 0 {
        let n = lo.wrapping_neg().wrapping_sub(1) / base + 1;
        hi = hi.wrapping_sub(n);
        lo = lo.wrapping_add(n.wrapping_mul(base));
    }
    if lo >= base {
        let n = lo / base;
        hi = hi.wrapping_add(n);
        lo = lo.wrapping_sub(n.wrapping_mul(base));
    }
    (hi, lo)
}
fn date_to_abs_days(year: i64, month: i64, day: i64) -> u64 {
    let mut amonth = month as u32;
    let jan_feb = u32::from(amonth < 3);
    amonth = amonth.wrapping_add(12 * jan_feb);
    let y = (year as u64)
        .wrapping_sub(u64::from(jan_feb))
        .wrapping_add(ABSOLUTE_YEARS);
    let ayday = 979u32.wrapping_mul(amonth).wrapping_sub(2919) >> 5;
    let century = y / 100;
    let cyear = (y % 100) as u32;
    let cday = 1461 * cyear / 4;
    let century_days = 146_097u64.wrapping_mul(century) / 4;
    century_days.wrapping_add(
        i64::from(cday.wrapping_add(ayday))
            .wrapping_add(day)
            .wrapping_sub(1) as u64,
    )
}
fn abs_days_date(days: u64) -> (i64, i64, i64) {
    let d = 4u64.wrapping_mul(days).wrapping_add(3);
    let century = d / 146_097;
    let cd = ((d % 146_097) as u32) | 3;
    let product = 2_939_745u64 * u64::from(cd);
    let cyear = (product >> 32) as i64;
    let ayday = (product as u32) / 2_939_745 / 4;
    let md = 2141u32.wrapping_mul(ayday).wrapping_add(197_913);
    let amonth = i64::from(md >> 16);
    let mday = 1 + i64::from((md & 0xFFFF) / 2141);
    let jan_feb = i64::from(ayday >= MARCH_THRU_DECEMBER);
    let year = (century.wrapping_mul(100).wrapping_sub(ABSOLUTE_YEARS) as i64)
        .wrapping_add(cyear)
        .wrapping_add(jan_feb);
    (year, amonth - jan_feb * 12, mday)
}
pub fn local_offset_at(unix_seconds: i64) -> i64 {
    use chrono::{Offset, TimeZone};
    match chrono::Local.timestamp_opt(unix_seconds, 0) {
        chrono::LocalResult::Single(t) => i64::from(t.offset().fix().local_minus_utc()),
        _ => i64::from(chrono::Local::now().offset().fix().local_minus_utc()),
    }
}
impl ZonedTime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_fields(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        nanosecond: i64,
        offset: i64,
    ) -> Self {
        let (year, m) = norm(year, month.wrapping_sub(1), 12);
        let month = m.wrapping_add(1);
        let (second, nanosecond) = norm(second, nanosecond, 1_000_000_000);
        let (minute, second) = norm(minute, second, 60);
        let (hour, minute) = norm(hour, minute, 60);
        let (day, hour) = norm(day, hour, 24);
        let clock = hour
            .wrapping_mul(3600)
            .wrapping_add(minute.wrapping_mul(60))
            .wrapping_add(second);
        let unix = (date_to_abs_days(year, month, day) as i64)
            .wrapping_mul(SECONDS_PER_DAY)
            .wrapping_add(clock)
            .wrapping_add(ABSOLUTE_TO_UNIX)
            .wrapping_sub(offset);
        ZonedTime {
            sec: unix.wrapping_add(UNIX_TO_INTERNAL),
            nsec: nanosecond as i32,
            offset,
        }
    }
    fn abs(&self) -> u64 {
        self.sec
            .wrapping_sub(UNIX_TO_INTERNAL)
            .wrapping_add(self.offset)
            .wrapping_add(UNIX_TO_INTERNAL + INTERNAL_TO_ABSOLUTE) as u64
    }
    pub fn date(&self) -> (i64, i64, i64) {
        abs_days_date(self.abs() / SECONDS_PER_DAY as u64)
    }
    pub fn year(&self) -> i64 {
        self.date().0
    }
    pub fn month(&self) -> i64 {
        self.date().1
    }
    pub fn day(&self) -> i64 {
        self.date().2
    }
    pub fn hour(&self) -> i64 {
        (self.abs() % SECONDS_PER_DAY as u64) as i64 / 3600
    }
    pub fn minute(&self) -> i64 {
        (self.abs() % 3600) as i64 / 60
    }
    pub fn second(&self) -> i64 {
        (self.abs() % 60) as i64
    }
    pub fn nanosecond(&self) -> i64 {
        ((self.nsec as i64 as u64) & NSEC_MASK) as i64
    }
    pub fn offset(&self) -> i64 {
        self.offset
    }
    pub fn add_seconds(&self, seconds: i64) -> Self {
        ZonedTime {
            sec: self.sec.wrapping_add(seconds),
            ..*self
        }
    }
    pub fn unix(&self) -> i64 {
        self.sec.wrapping_sub(UNIX_TO_INTERNAL)
    }
    pub fn from_unix_utc(seconds: i64, nanoseconds: i64) -> Self {
        ZonedTime {
            sec: seconds.wrapping_add(UNIX_TO_INTERNAL),
            nsec: nanoseconds.clamp(0, 999_999_999) as i32,
            offset: 0,
        }
    }
    pub fn from_unix_local(seconds: i64, nanoseconds: i64) -> Self {
        ZonedTime {
            offset: local_offset_at(seconds),
            ..ZonedTime::from_unix_utc(seconds, nanoseconds)
        }
    }
    pub fn now_local() -> Self {
        let now = chrono::Utc::now();
        ZonedTime::from_unix_local(now.timestamp(), i64::from(now.timestamp_subsec_nanos()))
    }
    pub fn now_utc() -> Self {
        let now = chrono::Utc::now();
        ZonedTime::from_unix_utc(now.timestamp(), i64::from(now.timestamp_subsec_nanos()))
    }
    pub fn new_local(
        year: i64,
        month: i64,
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        nanosecond: i64,
    ) -> Self {
        let wall = ZonedTime::from_fields(year, month, day, hour, minute, second, nanosecond, 0);
        let mut unix = wall.unix();
        let guess = local_offset_at(unix);
        if guess != 0 {
            unix = unix.wrapping_sub(local_offset_at(unix.wrapping_sub(guess)));
        }
        ZonedTime::from_unix_local(unix, wall.nanosecond())
    }
    pub fn add_date_local(&self, years: i64, months: i64, days: i64) -> Self {
        let (year, month, day) = self.date();
        ZonedTime::new_local(
            year.wrapping_add(years),
            month.wrapping_add(months),
            day.wrapping_add(days),
            self.hour(),
            self.minute(),
            self.second(),
            self.nanosecond(),
        )
    }
    pub const fn zero() -> Self {
        ZonedTime {
            sec: 0,
            nsec: 0,
            offset: 0,
        }
    }
    pub fn is_zero(&self) -> bool {
        self.sec == 0 && self.nanosecond() == 0
    }
    pub fn nanos_since_zero(&self) -> i128 {
        i128::from(self.sec) * 1_000_000_000 + i128::from(self.nanosecond())
    }
    pub fn sub_nanoseconds(&self, earlier: &ZonedTime) -> i64 {
        let d = (i128::from(self.sec) - i128::from(earlier.sec)) * 1_000_000_000
            + i128::from(self.nanosecond() - earlier.nanosecond());
        d.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }
    fn write_offset(&self, out: &mut String, colon: bool) {
        let mut zone = self.offset / 60;
        if zone < 0 {
            out.push('-');
            zone = -zone;
        } else {
            out.push('+');
        }
        out.push_str(&format!("{:02}", zone / 60));
        if colon {
            out.push(':');
        }
        out.push_str(&format!("{:02}", zone % 60));
    }
}
fn write_zero_padded(out: &mut String, value: i64, width: i64) {
    if width <= 0 {
        out.push_str(&value.to_string());
        return;
    }
    let mut value = value;
    let mut width = width;
    if value < 0 {
        out.push('-');
        value = value.wrapping_neg();
        width -= 1;
    }
    if width <= 0 {
        out.push_str(&value.to_string());
        return;
    }
    let mut limit = 1i64;
    for _ in 0..width {
        limit = limit.wrapping_mul(10);
    }
    if value >= limit {
        out.push_str(&value.to_string());
        return;
    }
    let mut buf = vec![0u8; width as usize];
    for slot in buf.iter_mut().rev() {
        *slot = (48 + value % 10) as u8;
        value /= 10;
    }
    out.extend(buf.into_iter().map(char::from));
}
fn write_truncated_microseconds(out: &mut String, nanoseconds: i64, precision: PrecisionLevel) {
    let digits = 6i64.wrapping_sub(precision.0);
    if digits <= 0 {
        return;
    }
    let mut microseconds = nanoseconds / 1000;
    let mut divisor = 100_000i64;
    for _ in 0..digits {
        let digit = microseconds
            .checked_div(divisor)
            .expect("runtime error: integer divide by zero");
        out.push(char::from((48 + digit) as u8));
        microseconds %= divisor;
        divisor /= 10;
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub time: ZonedTime,
    pub precision: PrecisionLevel,
    pub is_nema: bool,
}
impl Date {
    pub fn dcm(&self) -> String {
        let (year, month, day) = self.time.date();
        let mut out = String::with_capacity(10);
        write_zero_padded(&mut out, year, 4);
        if !is_included(PrecisionLevel::MONTH, self.precision) {
            return out;
        }
        if self.is_nema {
            out.push('.');
        }
        write_zero_padded(&mut out, month, 2);
        if !is_included(PrecisionLevel::DAY, self.precision) {
            return out;
        }
        if self.is_nema {
            out.push('.');
        }
        write_zero_padded(&mut out, day, 2);
        out
    }
}
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.time.date();
        let mut out = String::with_capacity(10);
        write_zero_padded(&mut out, year, 4);
        if is_included(PrecisionLevel::MONTH, self.precision) {
            out.push('-');
            write_zero_padded(&mut out, month, 2);
            if is_included(PrecisionLevel::DAY, self.precision) {
                out.push('-');
                write_zero_padded(&mut out, day, 2);
            }
        }
        f.write_str(&out)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub time: ZonedTime,
    pub precision: PrecisionLevel,
}
impl Time {
    fn render(&self, sep: Option<char>) -> String {
        let mut out = String::with_capacity(15);
        write_zero_padded(&mut out, self.time.hour(), 2);
        if !is_included(PrecisionLevel::MINUTES, self.precision) {
            return out;
        }
        out.extend(sep);
        write_zero_padded(&mut out, self.time.minute(), 2);
        if !is_included(PrecisionLevel::SECONDS, self.precision) {
            return out;
        }
        out.extend(sep);
        write_zero_padded(&mut out, self.time.second(), 2);
        if !is_included(PrecisionLevel::MS1, self.precision) {
            return out;
        }
        out.push('.');
        write_truncated_microseconds(&mut out, self.time.nanosecond(), self.precision);
        out
    }
    pub fn dcm(&self) -> String {
        self.render(None)
    }
}
impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Some(':')))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Datetime {
    pub time: ZonedTime,
    pub precision: PrecisionLevel,
    pub no_offset: bool,
}
impl Datetime {
    fn date(&self) -> Date {
        Date {
            time: self.time,
            precision: self.precision,
            is_nema: false,
        }
    }
    fn clock(&self) -> Time {
        Time {
            time: self.time,
            precision: self.precision,
        }
    }
    pub fn dcm(&self) -> String {
        let mut out = self.date().dcm();
        if is_included(PrecisionLevel::HOURS, self.precision) {
            out.push_str(&self.clock().dcm());
        }
        if !self.no_offset {
            self.time.write_offset(&mut out, false);
        }
        out
    }
}
impl fmt::Display for Datetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = self.date().to_string();
        if is_included(PrecisionLevel::HOURS, self.precision) {
            out.push(' ');
            out.push_str(&self.clock().to_string());
        }
        if !self.no_offset {
            out.push(' ');
            self.time.write_offset(&mut out, true);
        }
        f.write_str(&out)
    }
}
type Part = Option<i64>;
fn digits_value(s: &[u8]) -> i64 {
    s.iter().fold(0, |acc, &b| acc * 10 + i64::from(b - b'0'))
}
fn digit_run(s: &[u8]) -> usize {
    s.iter().take_while(|b| b.is_ascii_digit()).count()
}
fn groups(run: &[u8], first: usize, count: usize) -> Option<Vec<Part>> {
    if run.len() < first
        || !(run.len() - first).is_multiple_of(2)
        || (run.len() - first) / 2 > count
    {
        return None;
    }
    let mut out = vec![Some(digits_value(&run[..first]))];
    let mut rest = &run[first..];
    for _ in 0..count {
        if rest.is_empty() {
            out.push(None);
        } else {
            out.push(Some(digits_value(&rest[..2])));
            rest = &rest[2..];
        }
    }
    Some(out)
}
type Fraction = (i64, PrecisionLevel);
fn fraction(s: &[u8]) -> Option<(Option<Fraction>, &[u8])> {
    let Some(rest) = s.strip_prefix(b".") else {
        return Some((None, s));
    };
    let n = digit_run(rest);
    if !(1..=6).contains(&n) {
        return None;
    }
    let missing = 9 - n as i64;
    let nanos = digits_value(&rest[..n]) * 10i64.pow(missing as u32);
    Some((Some((nanos, PrecisionLevel(missing - 3))), &rest[n..]))
}
fn valid_date(year: i64, month: i64, day: i64) -> bool {
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    day <= ZonedTime::from_fields(year, month + 1, 0, 0, 0, 0, 0, 0).day()
}
fn valid_time(hours: Part, minutes: Part, seconds: Part, has_fraction: bool) -> bool {
    (0..=23).contains(&hours.unwrap_or(0))
        && (0..=59).contains(&minutes.unwrap_or(0))
        && (0..=60).contains(&seconds.unwrap_or(0))
        && (!has_fraction || seconds.is_some())
}
fn valid_offset(sign: u8, hours: i64, minutes: i64) -> bool {
    if !(0..=59).contains(&minutes) {
        return false;
    }
    match sign {
        b'+' => hours < 14 || hours == 14 && minutes == 0,
        _ => {
            if hours == 0 && minutes == 0 {
                return false;
            }
            hours < 12 || hours == 12 && minutes == 0
        }
    }
}
fn precision_of(parts: &[(Part, PrecisionLevel)], start: PrecisionLevel) -> PrecisionLevel {
    parts
        .iter()
        .rev()
        .find(|(part, _)| part.is_some())
        .map(|&(_, level)| level)
        .unwrap_or(start)
}
pub fn parse_date(value: &str) -> Result<Date, Error> {
    let s = value.as_bytes();
    let (parts, is_nema) = if digit_run(s) == s.len() {
        (groups(s, 4, 2), false)
    } else {
        (parse_nema(s), true)
    };
    let parts = parts.ok_or(Error::ParseDA)?;
    let (year, month, day) = (parts[0], parts[1], parts[2]);
    let precision = precision_of(
        &[
            (year, PrecisionLevel::YEAR),
            (month, PrecisionLevel::MONTH),
            (day, PrecisionLevel::FULL),
        ],
        PrecisionLevel::FULL,
    );
    let (year, month, day) = (year.unwrap_or(0), month.unwrap_or(1), day.unwrap_or(1));
    if !valid_date(year, month, day) {
        return Err(Error::ParseDA);
    }
    Ok(Date {
        time: ZonedTime::from_fields(year, month, day, 0, 0, 0, 0, 0),
        precision,
        is_nema,
    })
}
fn parse_nema(s: &[u8]) -> Option<Vec<Part>> {
    if s.len() < 4 || digit_run(&s[..4]) != 4 {
        return None;
    }
    let mut out = vec![Some(digits_value(&s[..4]))];
    let mut rest = &s[4..];
    for _ in 0..2 {
        match rest {
            [b'.', a, b, tail @ ..] if a.is_ascii_digit() && b.is_ascii_digit() => {
                out.push(Some(digits_value(&[*a, *b])));
                rest = tail;
            }
            _ => out.push(None),
        }
    }
    rest.is_empty().then_some(out)
}
pub fn parse_time(value: &str) -> Result<Time, Error> {
    let s = value.as_bytes();
    let n = digit_run(s);
    let parts = groups(&s[..n], 2, 2).ok_or(Error::ParseTM)?;
    let (frac, rest) = fraction(&s[n..]).ok_or(Error::ParseTM)?;
    if !rest.is_empty() {
        return Err(Error::ParseTM);
    }
    let (hours, minutes, seconds) = (parts[0], parts[1], parts[2]);
    let mut precision = precision_of(
        &[
            (hours, PrecisionLevel::HOURS),
            (minutes, PrecisionLevel::MINUTES),
            (seconds, PrecisionLevel::SECONDS),
        ],
        PrecisionLevel::FULL,
    );
    if let Some((_, level)) = frac {
        precision = level;
    }
    if !valid_time(hours, minutes, seconds, frac.is_some()) {
        return Err(Error::ParseTM);
    }
    let time = ZonedTime::from_fields(
        1,
        1,
        1,
        hours.unwrap_or(0),
        minutes.unwrap_or(0),
        seconds.unwrap_or(0),
        frac.map_or(0, |(nanos, _)| nanos),
        0,
    );
    Ok(Time { time, precision })
}
pub fn parse_datetime(value: &str) -> Result<Datetime, Error> {
    let s = value.as_bytes();
    let n = digit_run(s);
    let parts = groups(&s[..n], 4, 5).ok_or(Error::ParseDT)?;
    let (frac, rest) = fraction(&s[n..]).ok_or(Error::ParseDT)?;
    let zone = match rest {
        [] => None,
        [sign @ (b'+' | b'-'), digits @ ..] if digits.len() == 4 && digit_run(digits) == 4 => {
            Some((
                *sign,
                digits_value(&digits[..2]),
                digits_value(&digits[2..]),
            ))
        }
        _ => return Err(Error::ParseDT),
    };
    let [year, month, day, hours, minutes, seconds] = parts[..] else {
        return Err(Error::ParseDT);
    };
    let mut precision = precision_of(
        &[
            (year, PrecisionLevel::YEAR),
            (month, PrecisionLevel::MONTH),
            (day, PrecisionLevel::DAY),
            (hours, PrecisionLevel::HOURS),
            (minutes, PrecisionLevel::MINUTES),
            (seconds, PrecisionLevel::SECONDS),
        ],
        PrecisionLevel::FULL,
    );
    if let Some((_, level)) = frac {
        precision = level;
    }
    let (year, month, day) = (year.unwrap_or(0), month.unwrap_or(1), day.unwrap_or(1));
    if !valid_date(year, month, day) || !valid_time(hours, minutes, seconds, frac.is_some()) {
        return Err(Error::ParseDT);
    }
    let mut offset = 0;
    if let Some((sign, oh, om)) = zone {
        if !valid_offset(sign, oh, om) {
            return Err(Error::ParseDT);
        }
        offset = (oh * 3600 + om * 60) * if sign == b'-' { -1 } else { 1 };
    }
    let time = ZonedTime::from_fields(
        year,
        month,
        day,
        hours.unwrap_or(0),
        minutes.unwrap_or(0),
        seconds.unwrap_or(0),
        frac.map_or(0, |(nanos, _)| nanos),
        offset,
    );
    Ok(Datetime {
        time,
        precision,
        no_offset: zone.is_none(),
    })
}
fn clean(value: &str) -> String {
    value.replace('\0', "").trim().to_owned()
}
pub fn parse_da(value: &str) -> Result<ZonedTime, Error> {
    match parse_date(&clean(value)) {
        Ok(date) if !date.is_nema && matches!(date.precision.0, 0 | 9) => Ok(date.time),
        _ => Err(Error::ParseFullDA),
    }
}
pub fn parse_tm(value: &str) -> Result<ZonedTime, Error> {
    parse_time(&clean(value))
        .map(|tm| tm.time)
        .map_err(|_| Error::ParseValidTM)
}
pub fn format_date(value: &str) -> String {
    let value = clean(value);
    match parse_date(&value) {
        Ok(date) if !value.is_empty() => date.to_string(),
        _ => value,
    }
}
pub fn format_time(value: &str) -> String {
    let value = clean(value);
    match parse_time(&value) {
        Ok(tm) if !value.is_empty() => tm.to_string(),
        _ => value,
    }
}
#[cfg(test)]
mod local_tests {
    use super::*;
    #[test]
    fn local_times_agree_with_the_system_zone() {
        let now = ZonedTime::now_local();
        assert_eq!(now.offset(), local_offset_at(now.unix()));
        assert!((ZonedTime::now_utc().unix() - now.unix()).abs() <= 2);
        assert_eq!(ZonedTime::now_utc().offset(), 0);
        let t = ZonedTime::new_local(2026, 7, 15, 12, 34, 56, 789);
        assert_eq!(t.date(), (2026, 7, 15));
        assert_eq!(
            (t.hour(), t.minute(), t.second(), t.nanosecond()),
            (12, 34, 56, 789)
        );
        assert_eq!(t.offset(), local_offset_at(t.unix()));
        let rolled = ZonedTime::new_local(2026, 13, 1, 12, 0, 0, 0);
        assert_eq!(rolled.date(), (2027, 1, 1));
        let yesterday = t.add_date_local(0, 0, -1);
        assert_eq!(yesterday.date(), (2026, 7, 14));
        assert_eq!((yesterday.hour(), yesterday.minute()), (12, 34));
        assert_eq!(ZonedTime::from_unix_utc(0, 0).date(), (1970, 1, 1));
        assert_eq!(ZonedTime::from_unix_utc(86_400, 5).unix(), 86_400);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_renders() {
        let dt = parse_datetime("10100203040506.456789-0102").unwrap();
        assert_eq!(dt.dcm(), "10100203040506.456789-0102");
        assert_eq!(dt.to_string(), "1010-02-03 04:05:06.456789 -01:02");
        assert_eq!(parse_date("2020.03").unwrap().dcm(), "2020.03");
        assert_eq!(parse_time("1345").unwrap().to_string(), "13:45");
        assert_eq!(parse_time("235960").unwrap().time.hour(), 0);
        assert_eq!(parse_date("20230229"), Err(Error::ParseDA));
        assert_eq!(format_date(" 20260712\0 "), "2026-07-12");
        assert_eq!(PrecisionLevel::MS3.to_string(), "MS3");
    }
    #[test]
    fn normalizes_like_go() {
        let t = ZonedTime::from_fields(2020, 2, 30, 24, 60, 60, 1_000_000_000, 0);
        assert_eq!(t.date(), (2020, 3, 2));
        assert_eq!((t.hour(), t.minute(), t.second()), (1, 1, 1));
        let t = ZonedTime::from_fields(-5, 3, 4, 1, 2, 3, 4, 0);
        assert_eq!(
            Date {
                time: t,
                precision: PrecisionLevel::FULL,
                is_nema: false
            }
            .dcm(),
            "-0050304"
        );
    }
}
