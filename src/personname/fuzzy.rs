#[rustfmt::skip]
mod tables;
use tables::{FOLDS, WORD_RANGES};
pub const NAME_SOUNDEX: &str = "soundex";
pub const NAME_METAPHONE: &str = "metaphone";
pub const NAME_KOELNER_PHONETIK: &str = "koelner-phonetik";
pub fn algorithms() -> Vec<String> {
    vec![
        NAME_SOUNDEX.to_string(),
        NAME_METAPHONE.to_string(),
        NAME_KOELNER_PHONETIK.to_string(),
    ]
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzyAlgorithm {
    Soundex { length: i64 },
    Metaphone,
    KoelnerPhonetik,
}
impl FuzzyAlgorithm {
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            NAME_SOUNDEX => Some(FuzzyAlgorithm::Soundex { length: 0 }),
            NAME_METAPHONE => Some(FuzzyAlgorithm::Metaphone),
            NAME_KOELNER_PHONETIK => Some(FuzzyAlgorithm::KoelnerPhonetik),
            _ => None,
        }
    }
    pub fn name(&self) -> String {
        match self {
            FuzzyAlgorithm::Soundex { length } => {
                let length = soundex_length(*length);
                if length == 4 {
                    NAME_SOUNDEX.to_string()
                } else {
                    format!("{NAME_SOUNDEX}-{length}")
                }
            }
            FuzzyAlgorithm::Metaphone => NAME_METAPHONE.to_string(),
            FuzzyAlgorithm::KoelnerPhonetik => NAME_KOELNER_PHONETIK.to_string(),
        }
    }
    pub fn encode(&self, word: &str) -> String {
        match self {
            FuzzyAlgorithm::Soundex { length } => soundex(word, soundex_length(*length)),
            FuzzyAlgorithm::Metaphone => metaphone(word),
            FuzzyAlgorithm::KoelnerPhonetik => koelner(word),
        }
    }
}
fn soundex_length(length: i64) -> i64 {
    if length <= 0 { 4 } else { length }
}
fn is_word_char(r: char) -> bool {
    let c = r as u32;
    WORD_RANGES
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}
pub fn words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for r in value.chars() {
        if r == '\'' || r == '\u{2019}' {
            continue;
        }
        if is_word_char(r) {
            current.push(r);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}
pub fn keys(encoder: FuzzyAlgorithm, value: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for word in words(value) {
        let key = encoder.encode(&word);
        if key.is_empty() || keys.contains(&key) {
            continue;
        }
        keys.push(key);
    }
    keys
}
pub fn matches(encoder: FuzzyAlgorithm, query: &str, candidate: &str) -> bool {
    let query_keys = keys(encoder, query);
    if query_keys.is_empty() {
        return false;
    }
    let candidate_keys = keys(encoder, candidate);
    query_keys.iter().all(|k| candidate_keys.contains(k))
}
fn fold_letters(word: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(word.len());
    for r in word.chars() {
        if r.is_ascii() {
            if r.is_ascii_alphabetic() {
                out.push(r.to_ascii_uppercase() as u8);
            }
            continue;
        }
        if let Ok(i) = FOLDS.binary_search_by_key(&(r as u32), |&(c, _)| c) {
            out.extend_from_slice(FOLDS[i].1.as_bytes());
        }
    }
    out
}
fn letter_at(word: &[u8], index: isize) -> u8 {
    if index < 0 || index as usize >= word.len() {
        0
    } else {
        word[index as usize]
    }
}
fn one_of(letter: u8, set: &str) -> bool {
    letter != 0 && set.as_bytes().contains(&letter)
}
fn soundex_digit(letter: u8) -> u8 {
    match letter {
        b'B' | b'F' | b'P' | b'V' => b'1',
        b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => b'2',
        b'D' | b'T' => b'3',
        b'L' => b'4',
        b'M' | b'N' => b'5',
        b'R' => b'6',
        _ => 0,
    }
}
fn soundex(word: &str, length: i64) -> String {
    let length = usize::try_from(length).unwrap_or(usize::MAX);
    let letters = fold_letters(word);
    if letters.is_empty() {
        return String::new();
    }
    let mut code = vec![letters[0]];
    let mut last = soundex_digit(letters[0]);
    for &letter in &letters[1..] {
        if code.len() == length {
            break;
        }
        if letter == b'H' || letter == b'W' {
            continue;
        }
        let digit = soundex_digit(letter);
        if digit == 0 {
            last = 0;
            continue;
        }
        if digit != last {
            code.push(digit);
        }
        last = digit;
    }
    while code.len() < length {
        code.push(b'0');
    }
    String::from_utf8_lossy(&code).into_owned()
}
fn is_metaphone_vowel(letter: u8) -> bool {
    one_of(letter, "AEIOU")
}
fn metaphone(word: &str) -> String {
    let folded = fold_letters(word);
    let mut letters: Vec<u8> = Vec::with_capacity(folded.len());
    for (i, &letter) in folded.iter().enumerate() {
        if i > 0 && letter == folded[i - 1] && letter != b'C' {
            continue;
        }
        letters.push(letter);
    }
    if letters.is_empty() {
        return String::new();
    }
    if letters.len() > 1
        && ["AE", "GN", "KN", "PN", "WR"]
            .iter()
            .any(|p| letters.starts_with(p.as_bytes()))
    {
        letters.remove(0);
    } else if letters[0] == b'X' {
        letters[0] = b'S';
    } else if letters.starts_with(b"WH") {
        letters.remove(1);
    }
    let mut code = Vec::with_capacity(letters.len() + 2);
    let last = letters.len() as isize - 1;
    for (i, &letter) in letters.iter().enumerate() {
        let i = i as isize;
        let previous = letter_at(&letters, i - 1);
        let next = letter_at(&letters, i + 1);
        let after_next = letter_at(&letters, i + 2);
        match letter {
            b'A' | b'E' | b'I' | b'O' | b'U' => {
                if i == 0 {
                    code.push(letter);
                }
            }
            b'B' => {
                if !(previous == b'M' && i == last) {
                    code.push(b'B');
                }
            }
            b'C' => {
                if next == b'I' && after_next == b'A' {
                    code.push(b'X');
                } else if next == b'H' && previous == b'S' {
                    code.push(b'K');
                } else if next == b'H' {
                    code.push(b'X');
                } else if one_of(next, "IEY") && previous == b'S' {
                } else if one_of(next, "IEY") {
                    code.push(b'S');
                } else {
                    code.push(b'K');
                }
            }
            b'D' => {
                if next == b'G' && one_of(after_next, "EIY") {
                    code.push(b'J');
                } else {
                    code.push(b'T');
                }
            }
            b'G' => {
                let silent = (next == b'H' && i + 2 <= last && !is_metaphone_vowel(after_next))
                    || (next == b'N'
                        && (i + 1 == last
                            || (i + 3 == last
                                && after_next == b'E'
                                && letter_at(&letters, i + 3) == b'D')))
                    || (previous == b'D' && one_of(next, "EIY"));
                if silent {
                } else if one_of(next, "EIY") {
                    code.push(b'J');
                } else {
                    code.push(b'K');
                }
            }
            b'H' => {
                if one_of(previous, "CGPST")
                    || (is_metaphone_vowel(previous) && !is_metaphone_vowel(next))
                {
                } else {
                    code.push(b'H');
                }
            }
            b'K' => {
                if previous != b'C' {
                    code.push(b'K');
                }
            }
            b'P' => code.push(if next == b'H' { b'F' } else { b'P' }),
            b'Q' => code.push(b'K'),
            b'S' => {
                if next == b'H' || (next == b'I' && one_of(after_next, "OA")) {
                    code.push(b'X');
                } else {
                    code.push(b'S');
                }
            }
            b'T' => {
                if next == b'I' && one_of(after_next, "OA") {
                    code.push(b'X');
                } else if next == b'H' {
                    code.push(b'0');
                } else if next == b'C' && after_next == b'H' {
                } else {
                    code.push(b'T');
                }
            }
            b'V' => code.push(b'F'),
            b'W' | b'Y' => {
                if is_metaphone_vowel(next) {
                    code.push(letter);
                }
            }
            b'X' => code.extend_from_slice(b"KS"),
            b'Z' => code.push(b'S'),
            _ => code.push(letter),
        }
    }
    String::from_utf8_lossy(&code).into_owned()
}
fn koelner(word: &str) -> String {
    let letters = fold_letters(word);
    if letters.is_empty() {
        return String::new();
    }
    let mut raw = Vec::with_capacity(letters.len() + 2);
    for (i, &letter) in letters.iter().enumerate() {
        let i = i as isize;
        let previous = letter_at(&letters, i - 1);
        let next = letter_at(&letters, i + 1);
        match letter {
            b'A' | b'E' | b'I' | b'J' | b'O' | b'U' | b'Y' => raw.push(b'0'),
            b'H' => {}
            b'B' => raw.push(b'1'),
            b'P' => raw.push(if next == b'H' { b'3' } else { b'1' }),
            b'D' | b'T' => raw.push(if one_of(next, "CSZ") { b'8' } else { b'2' }),
            b'F' | b'V' | b'W' => raw.push(b'3'),
            b'G' | b'K' | b'Q' => raw.push(b'4'),
            b'C' => {
                let digit = if i == 0 && one_of(next, "AHKLOQRUX") {
                    b'4'
                } else if i == 0 || one_of(previous, "SZ") {
                    b'8'
                } else if one_of(next, "AHKOQUX") {
                    b'4'
                } else {
                    b'8'
                };
                raw.push(digit);
            }
            b'X' => {
                if one_of(previous, "CKQ") {
                    raw.push(b'8');
                } else {
                    raw.extend_from_slice(b"48");
                }
            }
            b'L' => raw.push(b'5'),
            b'M' | b'N' => raw.push(b'6'),
            b'R' => raw.push(b'7'),
            b'S' | b'Z' => raw.push(b'8'),
            _ => {}
        }
    }
    if raw.is_empty() {
        return String::new();
    }
    let mut code = Vec::with_capacity(raw.len());
    for (i, &digit) in raw.iter().enumerate() {
        if i > 0 && digit == raw[i - 1] {
            continue;
        }
        if digit == b'0' && !code.is_empty() {
            continue;
        }
        code.push(digit);
    }
    String::from_utf8_lossy(&code).into_owned()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoders() {
        let s = FuzzyAlgorithm::Soundex { length: 0 };
        assert_eq!(s.encode("Robert"), "R163");
        assert_eq!(s.encode("Tymczak"), "T522");
        assert_eq!(s.encode("Müller"), "M460");
        assert_eq!(FuzzyAlgorithm::Metaphone.encode("Thompson"), "0MPSN");
        assert_eq!(
            FuzzyAlgorithm::KoelnerPhonetik.encode("Müller-Lüdenscheidt"),
            "65752682"
        );
        assert!(matches(s, "SMYTH^JON", "Smith^John^Q"));
    }
}
