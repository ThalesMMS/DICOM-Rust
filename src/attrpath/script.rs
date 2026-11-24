use super::edit::{self, Options};
use super::path::{Path, parse};
use super::{Error, Kind};
use crate::object::Object;
use crate::textfmt::quote;
use std::fmt;
pub const MAX_SCRIPT_BYTES: usize = 1 << 20;
pub const MAX_SCRIPT_EDITS: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Insert,
    Modify,
    Remove,
    Create,
}
impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Insert => "insert",
            Operation::Modify => "modify",
            Operation::Remove => "remove",
            Operation::Create => "create",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub op: Operation,
    pub path: Path,
    pub value: String,
    pub if_present: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Script(pub Vec<Edit>);
pub fn parse_script(text: &str) -> Result<Script, Error> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(Error::new(
            Kind::Limit,
            format!("script longer than {MAX_SCRIPT_BYTES} bytes"),
        ));
    }
    let mut script = Vec::new();
    for (number, line) in text.split('\n').enumerate() {
        let line = line.trim_end_matches('\r');
        let trimmed = trim_unicode_space(line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if script.len() == MAX_SCRIPT_EDITS {
            return Err(Error::new(
                Kind::Limit,
                format!("more than {MAX_SCRIPT_EDITS} edits"),
            ));
        }
        let edit = parse_edit(line.trim_start_matches([' ', '\t']))
            .map_err(|e| e.prefixed(format!("line {}", number + 1)))?;
        script.push(edit);
    }
    Ok(Script(script))
}
pub(crate) fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
fn parse_edit(line: &str) -> Result<Edit, Error> {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    let (op, if_present) = match word {
        "insert" => (Operation::Insert, false),
        "modify" => (Operation::Modify, false),
        "remove" => (Operation::Remove, false),
        "create" => (Operation::Create, false),
        "modify-if-present" => (Operation::Modify, true),
        "remove-if-present" => (Operation::Remove, true),
        _ => {
            return Err(Error::new(
                Kind::Syntax,
                format!("unknown operation {}", quote(word)),
            ));
        }
    };
    let rest = rest.trim_start_matches([' ', '\t']);
    let (path_text, value, has_value) = cut_outside_quotes(rest, b'=');
    let path_text = trim_unicode_space(path_text);
    if op == Operation::Remove && has_value {
        return Err(Error::new(Kind::Syntax, "remove takes no value"));
    }
    if op == Operation::Modify && !has_value {
        return Err(Error::new(Kind::Syntax, "modify needs path=value"));
    }
    let path = parse(path_text)?;
    Ok(Edit {
        op,
        path,
        value: value.to_string(),
        if_present,
    })
}
fn cut_outside_quotes(s: &str, sep: u8) -> (&str, &str, bool) {
    let mut quoted = false;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if b == b'"' {
            quoted = !quoted;
        } else if b == sep && !quoted {
            return (&s[..i], &s[i + 1..], true);
        }
    }
    (s, "", false)
}
impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for edit in &self.0 {
            f.write_str(edit.op.as_str())?;
            if edit.if_present {
                f.write_str("-if-present")?;
            }
            write!(f, " {}", edit.path)?;
            if edit.op != Operation::Remove
                && (!edit.value.is_empty() || edit.op == Operation::Modify)
            {
                write!(f, "={}", edit.value)?;
            }
            f.write_str("\n")?;
        }
        Ok(())
    }
}
impl Script {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn apply(&self, dataset: &mut Object, opts: &Options) -> (usize, Option<Error>) {
        let mut total = 0;
        for (i, edit) in self.0.iter().enumerate() {
            let result = match edit.op {
                Operation::Insert => edit::insert(dataset, &edit.path, &edit.value, opts),
                Operation::Modify => edit::modify(dataset, &edit.path, &edit.value, opts),
                Operation::Remove => edit::remove(dataset, &edit.path),
                Operation::Create => edit::create(dataset, &edit.path, opts),
            };
            match result {
                Ok(n) => total += n,
                Err(e) if edit.if_present && e.is(Kind::NotFound) => {}
                Err(e) => {
                    let prefix = format!("edit {} ({} {})", i + 1, edit.op.as_str(), edit.path);
                    return (total, Some(e.prefixed(prefix)));
                }
            }
        }
        (total, None)
    }
}
