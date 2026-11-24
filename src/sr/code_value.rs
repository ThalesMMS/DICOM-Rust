use super::CodedEntry;
use super::template_metadata::{
    CodeContext, code_key_for_entry, valid_code_context_shape, valid_code_key,
};
use crate::codes::{self, Code};
const MAX_SHORT_CODE_VALUE: usize = 16;
pub fn new_code(value: &str, designator: &str, meaning: &str) -> (CodedEntry, CodeContext) {
    let mut code = CodedEntry {
        coding_scheme_designator: designator.into(),
        code_meaning: meaning.into(),
        ..Default::default()
    };
    let mut context = CodeContext::default();
    if is_urn_or_url(value) {
        context.urn_code_value = value.into();
    } else if value.chars().count() > MAX_SHORT_CODE_VALUE {
        context.long_code_value = value.into();
    } else {
        code.code_value = value.into();
    }
    (code, context)
}
pub fn valid_code(code: &CodedEntry, context: &CodeContext) -> bool {
    let key = code_key_for_entry(code, context);
    if !valid_code_context_shape(context) || !valid_code_key(&key) || code.code_meaning.is_empty() {
        return false;
    }
    for v in [&key.code_value, &key.long_code_value] {
        if !v.is_empty() && is_urn_or_url(v) {
            return false;
        }
    }
    ![
        &key.code_value,
        &key.long_code_value,
        &key.coding_scheme_designator,
        &code.code_meaning,
    ]
    .iter()
    .any(|v| v.contains('\\'))
}
fn is_urn_or_url(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() > 4 && b[..4].eq_ignore_ascii_case(b"urn:") {
        return true;
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if scheme.is_empty() || rest.is_empty() {
        return false;
    }
    scheme.bytes().enumerate().all(|(i, c)| {
        c.is_ascii_alphabetic()
            || (i > 0 && (c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.'))
    })
}
pub fn from_code(c: Code<'_>) -> CodedEntry {
    CodedEntry::new(c.value, c.scheme, c.meaning)
}
pub(crate) fn from_context_group(id: i64, c: Code<'_>) -> CodedEntry {
    if let Some(group) = codes::context_group_by_id(id)
        && let Some(term) = group.member(&c)
    {
        return from_code(term.code);
    }
    from_code(c)
}
