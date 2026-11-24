#![forbid(unsafe_code)]
use super::dict;
use super::error::Error;
use super::obj::{Object, TAG_SPECIFIC_CHARACTER_SET, decode_text_values_with_default};
use super::text::{CharsetFallbackCases, TextOptions, trim_text_value_bytes};
use super::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use crate::charset::SpecificCharacterSet;
use std::collections::HashSet;
pub const ERR_CHARSET_CONVERSION: &str = "dicom: text cannot be converted to UTF-8";
const UTF8_DECLARATION: &str = "ISO_IR 192";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CharsetConversionPolicy {
    #[default]
    Atomic,
    KeepScope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharsetReason {
    FallbackUndeclared,
    FallbackUndecodable,
    FallbackUnsupported,
    Undecodable,
    Unsupported,
    ScopeKept,
    ScopePinned,
    DeferredText,
}
impl CharsetReason {
    pub fn as_str(self) -> &'static str {
        match self {
            CharsetReason::FallbackUndeclared => "fallback-undeclared",
            CharsetReason::FallbackUndecodable => "fallback-undecodable",
            CharsetReason::FallbackUnsupported => "fallback-unsupported-declaration",
            CharsetReason::Undecodable => "undecodable-text",
            CharsetReason::Unsupported => "unsupported-declaration",
            CharsetReason::ScopeKept => "scope-kept",
            CharsetReason::ScopePinned => "scope-kept-declaration-pinned",
            CharsetReason::DeferredText => "deferred-text-value",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharsetDiagnostic {
    pub path: String,
    pub reason: CharsetReason,
    pub declared: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharsetConversionReport {
    pub converted_values: i64,
    pub fallback_values: i64,
    pub kept_scopes: i64,
    pub diagnostics: Vec<CharsetDiagnostic>,
    pub diagnostics_truncated: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharsetConversionOptions {
    pub policy: CharsetConversionPolicy,
    pub max_diagnostics: i64,
}
#[derive(Clone, Debug)]
pub struct CharsetConversionFailure {
    pub error: Error,
    pub report: CharsetConversionReport,
}
impl std::fmt::Display for CharsetConversionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for CharsetConversionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}
fn conversion_error(path: &str, reason: CharsetReason, declared: &str) -> Error {
    Error::msg(format!(
        "{ERR_CHARSET_CONVERSION}: {path}: {} (declared {})",
        reason.as_str(),
        crate::textfmt::quote(declared)
    ))
}
#[derive(Clone)]
struct Scope {
    charset: SpecificCharacterSet,
    err: bool,
    declared: bool,
    terms: Vec<String>,
}
pub(super) fn sanitize_terms(terms: &[String]) -> String {
    let joined = terms.join("\\");
    let mut b = String::new();
    for r in joined.chars() {
        if b.len() >= 64 {
            break;
        }
        if r.is_ascii_alphanumeric() || matches!(r, ' ' | '_' | '-' | '\\') {
            b.push(r);
        } else {
            b.push('?');
        }
    }
    b
}
pub(super) fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{a0}')
}
fn scope_of(elements: &[Element], parent: &Scope) -> (Scope, bool) {
    let Some(element) = elements
        .iter()
        .find(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
    else {
        return (parent.clone(), false);
    };
    let Ok(values) = decode_text_values_with_default(element) else {
        return (
            Scope {
                charset: SpecificCharacterSet::default(),
                err: true,
                declared: true,
                terms: Vec::new(),
            },
            true,
        );
    };
    let declared = values.iter().any(|v| !trim_unicode_space(v).is_empty());
    let (charset, err) = match SpecificCharacterSet::parse(&values) {
        Ok(charset) => (charset, false),
        Err(_) => (SpecificCharacterSet::default(), true),
    };
    (
        Scope {
            charset,
            err,
            declared,
            terms: values,
        },
        true,
    )
}
pub(super) fn un_path(parent: &str, tag: Tag) -> String {
    if parent.is_empty() {
        tag.to_string()
    } else {
        format!("{parent}.{tag}")
    }
}
fn decode_whole_text(vr: Vr, raw: &[u8], charset: &SpecificCharacterSet) -> Option<String> {
    let trimmed = trim_text_value_bytes(vr, raw);
    if vr == Vr::PN {
        charset.decode_person_name(trimmed).ok()
    } else if !vr.uses_text_value_delimiter() {
        charset.decode_single_value(trimmed).ok()
    } else {
        charset.decode(trimmed).ok()
    }
}
pub(super) fn pad_to_even(vr: Vr, mut data: Vec<u8>) -> Vec<u8> {
    if data.len() % 2 == 1 {
        data.push(vr.padding_byte());
    }
    data
}
pub(super) fn set_declaration(mut elements: Vec<Element>, terms: &[String]) -> Vec<Element> {
    let raw = pad_to_even(Vr::CS, terms.join("\\").into_bytes());
    let element = Element {
        header: ElementHeader {
            tag: TAG_SPECIFIC_CHARACTER_SET,
            vr: Vr::CS,
            length: raw.len() as u32,
            length_set: false,
        },
        value: Value::Raw(raw),
    };
    match elements
        .iter_mut()
        .find(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
    {
        Some(existing) => *existing = element,
        None => elements.push(element),
    }
    elements
}
struct Converter {
    text: TextOptions,
    opts: CharsetConversionOptions,
    report: CharsetConversionReport,
    kept_scopes: HashSet<String>,
}
type Converted = Result<Vec<Element>, Error>;
impl Converter {
    fn note(&mut self, path: &str, reason: CharsetReason, scope: &Scope) {
        if (self.report.diagnostics.len() as i64) < self.opts.max_diagnostics {
            self.report.diagnostics.push(CharsetDiagnostic {
                path: path.to_string(),
                reason,
                declared: sanitize_terms(&scope.terms),
            });
        } else {
            self.report.diagnostics_truncated = true;
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn keep(
        &mut self,
        elements: &[Element],
        scope_path: &str,
        path: &str,
        reason: CharsetReason,
        scope: &Scope,
        own: bool,
        parent: &Scope,
        parent_converted: bool,
    ) -> Converted {
        if self.opts.policy == CharsetConversionPolicy::Atomic {
            return Err(conversion_error(
                path,
                reason,
                &sanitize_terms(&scope.terms),
            ));
        }
        self.note(path, reason, scope);
        self.report.kept_scopes += 1;
        self.kept_scopes.insert(scope_path.to_string());
        if own || !parent_converted {
            return Ok(elements.to_vec());
        }
        self.note(path, CharsetReason::ScopePinned, parent);
        Ok(set_declaration(elements.to_vec(), &parent.terms))
    }
    fn data_set(
        &mut self,
        elements: &[Element],
        path: &str,
        parent: &Scope,
        parent_converted: bool,
        top: bool,
    ) -> Converted {
        let (scope, own) = scope_of(elements, parent);
        let mut out = elements.to_vec();
        let mut fallback = None;
        let mut charset = scope.charset.clone();
        if scope.err {
            if !self.text.allow_unsupported_charset_fallback {
                return self.keep(
                    elements,
                    path,
                    path,
                    CharsetReason::Unsupported,
                    &scope,
                    own,
                    parent,
                    parent_converted,
                );
            }
            charset = self.text.fallback_character_set.clone();
            fallback = Some(CharsetReason::FallbackUnsupported);
        }
        let mut converted = 0;
        let mut fallbacks = Vec::new();
        for element in &mut out {
            let vr = element.vr();
            if !vr.uses_specific_character_set() || dict::is_private_creator(element.tag()) {
                continue;
            }
            let element_path = un_path(path, element.tag());
            let raw = match &element.value {
                Value::Raw(raw) => raw,
                Value::Nil => {
                    return self.keep(
                        elements,
                        path,
                        &element_path,
                        CharsetReason::DeferredText,
                        &scope,
                        own,
                        parent,
                        parent_converted,
                    );
                }
                _ => continue,
            };
            let mut text = decode_whole_text(vr, raw, &charset);
            let mut reason = fallback;
            if text.is_none() && fallback.is_none() {
                let cases = self.text.charset_fallback;
                let authorized = scope.declared
                    && cases.contains(CharsetFallbackCases::UNDECODABLE)
                    || !scope.declared && cases.contains(CharsetFallbackCases::UNDECLARED);
                if authorized {
                    text = decode_whole_text(vr, raw, &self.text.fallback_character_set);
                    reason = Some(if scope.declared {
                        CharsetReason::FallbackUndecodable
                    } else {
                        CharsetReason::FallbackUndeclared
                    });
                }
            }
            let Some(text) = text else {
                return self.keep(
                    elements,
                    path,
                    &element_path,
                    CharsetReason::Undecodable,
                    &scope,
                    own,
                    parent,
                    parent_converted,
                );
            };
            if let Some(reason) = reason {
                fallbacks.push((element_path, reason));
            }
            let value = pad_to_even(vr, text.into_bytes());
            element.header.length = value.len() as u32;
            element.value = Value::Raw(value);
            converted += 1;
        }
        for element in &mut out {
            let element_path = un_path(path, element.tag());
            let Value::Sequence(items) = &element.value else {
                continue;
            };
            let mut converted_items = Vec::with_capacity(items.len());
            for (j, item) in items.iter().enumerate() {
                let item_path = format!("{element_path}[{j}]");
                let converted_item =
                    self.data_set(&item.elements, &item_path, &scope, true, false)?;
                converted_items.push(DataSet {
                    elements: converted_item,
                    item_offset: item.item_offset,
                });
            }
            element.value = Value::Sequence(converted_items);
        }
        self.report.fallback_values += fallbacks.len() as i64;
        for (path, reason) in fallbacks {
            self.note(&path, reason, &scope);
        }
        self.report.converted_values += converted;
        if own || top || !parent_converted {
            out = set_declaration(out, &[UTF8_DECLARATION.to_string()]);
        }
        Ok(out)
    }
}
fn verify_scope(
    elements: &[Element],
    path: &str,
    parent: &Scope,
    kept_scopes: &HashSet<String>,
) -> Result<(), Error> {
    if kept_scopes.contains(path) {
        return Ok(());
    }
    let (scope, _) = scope_of(elements, parent);
    if scope.err {
        return Err(conversion_error(
            path,
            CharsetReason::Unsupported,
            &sanitize_terms(&scope.terms),
        ));
    }
    let utf8 = scope.terms.len() == 1 && trim_unicode_space(&scope.terms[0]) == UTF8_DECLARATION;
    for element in elements {
        let element_path = un_path(path, element.tag());
        match &element.value {
            Value::Sequence(items) => {
                for (j, item) in items.iter().enumerate() {
                    let item_path = format!("{element_path}[{j}]");
                    verify_scope(&item.elements, &item_path, &scope, kept_scopes)?;
                }
            }
            Value::Raw(raw) => {
                let vr = element.vr();
                if !utf8
                    || !vr.uses_specific_character_set()
                    || dict::is_private_creator(element.tag())
                {
                    continue;
                }
                if decode_whole_text(vr, raw, &scope.charset).is_none() {
                    return Err(conversion_error(
                        &element_path,
                        CharsetReason::Undecodable,
                        UTF8_DECLARATION,
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn convert_text_to_utf8(
    o: &Object,
    opts: CharsetConversionOptions,
) -> Result<(Object, CharsetConversionReport), Box<CharsetConversionFailure>> {
    let mut opts = opts;
    if opts.max_diagnostics <= 0 {
        opts.max_diagnostics = 256;
    }
    let mut c = Converter {
        text: o.text_options().clone(),
        opts,
        report: CharsetConversionReport::default(),
        kept_scopes: HashSet::new(),
    };
    let root = match o.inherited_charset() {
        Some((charset, err, declared)) => Scope {
            charset,
            err: err.is_some(),
            declared,
            terms: Vec::new(),
        },
        None => Scope {
            charset: SpecificCharacterSet::default(),
            err: false,
            declared: false,
            terms: Vec::new(),
        },
    };
    let elements = match c.data_set(o.elements(), "", &root, false, true) {
        Ok(elements) => elements,
        Err(error) => {
            return Err(Box::new(CharsetConversionFailure {
                error,
                report: c.report,
            }));
        }
    };
    if let Err(error) = verify_scope(&elements, "", &root, &c.kept_scopes) {
        return Err(Box::new(CharsetConversionFailure {
            error,
            report: c.report,
        }));
    }
    let mut out = Object::from_elements_with_text_options(elements, TextOptions::default());
    out.set_value_big_endian(o.value_big_endian());
    out.value_provider = o.value_provider.clone();
    Ok((out, c.report))
}
