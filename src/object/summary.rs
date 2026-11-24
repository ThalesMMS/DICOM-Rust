#![forbid(unsafe_code)]
use super::dict_names::STD_NAMES;
use super::obj::{Object, element_length};
use super::text::raw_string_values;
use super::types::{Element, Length, Tag, Value};
pub const DEFAULT_SUMMARY_MAX_VALUE_LENGTH: i64 = 160;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SummaryOptions {
    pub source: String,
    pub max_value_length: i64,
    pub skip_private: bool,
    pub skip_pixel_data: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementSummary {
    pub source: String,
    pub tag: Tag,
    pub vr: super::types::Vr,
    pub keyword: String,
    pub name: String,
    pub length: Length,
    pub value: Vec<u8>,
    pub private: bool,
}
pub fn standard_names(tag: Tag) -> Option<(&'static str, &'static str)> {
    let key = tag.key();
    let i = STD_NAMES.binary_search_by_key(&key, |e| e.0).ok()?;
    Some((STD_NAMES[i].1, STD_NAMES[i].2))
}
impl Object {
    pub fn summarize_elements(&self, opts: &SummaryOptions) -> Vec<ElementSummary> {
        let max = if opts.max_value_length == 0 {
            DEFAULT_SUMMARY_MAX_VALUE_LENGTH
        } else {
            opts.max_value_length
        };
        let mut rows = Vec::new();
        for elem in self.sorted_elements() {
            let tag = elem.tag();
            if opts.skip_private && tag.is_private() {
                continue;
            }
            if opts.skip_pixel_data && tag == Tag::PIXEL_DATA {
                continue;
            }
            let (keyword, name) = standard_names(tag).unwrap_or(("", ""));
            rows.push(ElementSummary {
                source: opts.source.clone(),
                tag,
                vr: elem.vr(),
                keyword: keyword.to_string(),
                name: name.to_string(),
                length: element_length(&elem),
                value: truncate_summary_value(&self.summary_element_value(&elem), max),
                private: tag.is_private(),
            });
        }
        rows
    }
    fn summary_element_value(&self, elem: &Element) -> Vec<u8> {
        match &elem.value {
            Value::Nil => {
                return if elem.tag() == Tag::PIXEL_DATA {
                    b"<pixel data skipped>".to_vec()
                } else {
                    b"<value skipped>".to_vec()
                };
            }
            Value::Sequence(items) => return format!("{} item(s)", items.len()).into_bytes(),
            Value::Fragments { fragments, .. } => {
                return format!("{} fragment(s)", fragments.len()).into_bytes();
            }
            Value::BulkData(uri) => {
                return if uri.is_empty() {
                    b"<bulk data>".to_vec()
                } else {
                    uri.clone().into_bytes()
                };
            }
            Value::Raw(_) | Value::Strings(_) => {}
        }
        if elem.vr().is_string_like() {
            let values: Vec<Vec<u8>> = match self.lookup_strings(elem.tag()) {
                Ok(values) => values.into_iter().map(String::into_bytes).collect(),
                Err(_) => raw_string_values(elem.vr(), elem.raw().unwrap_or_default()),
            };
            return values.join(&b'\\');
        }
        match elem.raw() {
            Some(raw) => format!("<{} bytes>", raw.len()).into_bytes(),
            None => b"<value>".to_vec(),
        }
    }
}
fn truncate_summary_value(value: &[u8], max: i64) -> Vec<u8> {
    if max < 0 {
        return value.to_vec();
    }
    if max == 0 {
        return if value.is_empty() {
            Vec::new()
        } else {
            b"...".to_vec()
        };
    }
    let runes = crate::textfmt::runes(value);
    if runes.len() as i64 <= max {
        return value.to_vec();
    }
    let mut out: String = runes[..max as usize].iter().collect();
    out.push_str("...");
    out.into_bytes()
}
