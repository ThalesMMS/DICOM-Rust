use crate::dicomjson::jsonwire::{self, Decoder};
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}
pub(crate) fn strict_value(data: &[u8]) -> Option<&[u8]> {
    let mut decoder = Decoder::new(data);
    let raw = decoder.decode_raw().ok()?;
    let end = decoder.input_offset();
    if data[end..].iter().all(|&c| is_space(c)) {
        Some(raw)
    } else {
        None
    }
}
pub(crate) fn unmarshal_value(data: &[u8]) -> Option<&[u8]> {
    strict_value(data)
}
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{17F}' => 'S',
            '\u{212A}' => 'K',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}
pub(crate) struct Fields<'a> {
    values: Vec<Option<&'a [u8]>>,
}
impl<'a> Fields<'a> {
    pub(crate) fn new(raw: &'a [u8], names: &[&str]) -> Option<Fields<'a>> {
        match raw.first() {
            Some(b'n') => Some(Fields {
                values: vec![None; names.len()],
            }),
            Some(b'{') => Some(Self::assign(raw, names)),
            _ => None,
        }
    }
    pub(crate) fn new_lenient(raw: &'a [u8], names: &[&str]) -> Option<Fields<'a>> {
        match raw.first() {
            Some(b'{') => Some(Self::assign(raw, names)),
            _ => None,
        }
    }
    fn assign(raw: &'a [u8], names: &[&str]) -> Fields<'a> {
        let mut values = vec![None; names.len()];
        let members = jsonwire::as_raw_map(raw).unwrap_or_default();
        for (key, value) in members {
            let index = names.iter().position(|n| *n == key).or_else(|| {
                let folded = fold(&key);
                names.iter().position(|n| fold(n) == folded)
            });
            if let Some(i) = index {
                values[i] = Some(value);
            }
        }
        Fields { values }
    }
    pub(crate) fn raw(&self, i: usize) -> Option<&'a [u8]> {
        self.values[i]
    }
    pub(crate) fn string(&self, i: usize) -> Option<String> {
        match self.values[i] {
            None => Some(String::new()),
            Some(raw) => as_string(raw),
        }
    }
    pub(crate) fn string_lenient(&self, i: usize) -> String {
        self.string(i).unwrap_or_default()
    }
    pub(crate) fn strings(&self, i: usize) -> Option<Option<Vec<String>>> {
        match self.values[i] {
            None => Some(None),
            Some(raw) => as_strings(raw),
        }
    }
    pub(crate) fn int(&self, i: usize) -> Option<i64> {
        match self.values[i] {
            None => Some(0),
            Some(raw) => match raw.first() {
                Some(b'n') => Some(0),
                Some(b'-' | b'0'..=b'9') => std::str::from_utf8(raw).ok()?.parse::<i64>().ok(),
                _ => None,
            },
        }
    }
}
pub(crate) fn as_string(raw: &[u8]) -> Option<String> {
    match raw.first() {
        Some(b'"') => Some(jsonwire::unquote(raw)),
        Some(b'n') => Some(String::new()),
        _ => None,
    }
}
pub(crate) fn as_strings(raw: &[u8]) -> Option<Option<Vec<String>>> {
    match raw.first() {
        Some(b'n') => Some(None),
        Some(b'[') => {
            let mut out = Vec::new();
            for element in jsonwire::as_raw_array(raw).ok()? {
                out.push(as_string(element)?);
            }
            Some(Some(out))
        }
        _ => None,
    }
}
pub(crate) fn is_valid_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if b.is_empty() {
        return false;
    }
    if b[i] == b'-' {
        i += 1;
        if i == b.len() {
            return false;
        }
    }
    match b[i] {
        b'0' => i += 1,
        b'1'..=b'9' => {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        _ => return false,
    }
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i += 2;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i + 1 < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if b[i] == b'+' || b[i] == b'-' {
            i += 1;
            if i == b.len() {
                return false;
            }
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    i == b.len()
}
