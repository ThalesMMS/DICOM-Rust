use super::measurement_report::{concept, matches_concept};
use super::read::Decoder;
use super::{ContentItem, ImageReference, Kind, RELATIONSHIP_HAS_ACQ_CONTEXT, SrError, tags as tg};
use crate::object::{Element, Object, Value};
pub fn image_library_error(code: &str, path: &str) -> SrError {
    let mut msg = format!("{}: {code}", Kind::ImageLibrary.text());
    if valid_numeric_path(path) {
        msg.push_str(" at ");
        msg.push_str(path);
    }
    let mut e = SrError::new(Kind::ImageLibrary, "");
    e.msg = msg;
    e.code = Some(code.into());
    if code == "resource-limit" {
        e.kinds.push(Kind::ResourceLimitExceeded);
    }
    e
}
pub(crate) fn valid_numeric_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 2048 {
        return false;
    }
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 || parts.len() > 65 || parts[0] != "1" {
        return false;
    }
    parts[1..].iter().all(|p| {
        p.parse::<u32>()
            .ok()
            .is_some_and(|n| n != 0 && n.to_string() == *p)
    })
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageLibraryEntry {
    pub image: ImageReference,
    pub descriptors: Vec<ContentItem>,
    pub header: ContentItem,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageLibraryGroup {
    pub order: Vec<i64>,
    pub descriptors: Vec<ContentItem>,
    pub entries: Vec<ImageLibraryEntry>,
    pub header: ContentItem,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLibraryOptions {
    pub max_items: i64,
    pub max_depth: i64,
    pub max_value_bytes: i64,
}
pub fn default_image_library_options() -> ImageLibraryOptions {
    ImageLibraryOptions {
        max_items: 10000,
        max_depth: 32,
        max_value_bytes: 16 << 20,
    }
}
pub(crate) fn normalize(mut o: ImageLibraryOptions) -> Result<ImageLibraryOptions, SrError> {
    let d = default_image_library_options();
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_depth == 0 {
        o.max_depth = d.max_depth;
    }
    if o.max_value_bytes == 0 {
        o.max_value_bytes = d.max_value_bytes;
    }
    if o.max_items < 1
        || o.max_items > 1_000_000
        || o.max_depth < 1
        || o.max_depth > 64
        || o.max_value_bytes < 1
    {
        return Err(image_library_error("resource-limit", ""));
    }
    Ok(o)
}
struct Budget {
    items: i64,
    bytes: i64,
    max_items: i64,
    max_value_bytes: i64,
}
impl Budget {
    fn resident(&mut self, e: &Element) -> Result<(), SrError> {
        self.items += 1;
        if self.items > self.max_items {
            return Err(image_library_error("resource-limit", ""));
        }
        let mut n: i64 = 0;
        match &e.value {
            Value::Raw(v) => n = v.len() as i64,
            Value::Strings(v) => {
                for s in v {
                    if s.len() as i64 > self.max_value_bytes - self.bytes - n {
                        return Err(image_library_error("resource-limit", ""));
                    }
                    n += s.len() as i64;
                }
            }
            Value::Sequence(items) => {
                if items.len() as i64 > self.max_items - self.items {
                    return Err(image_library_error("resource-limit", ""));
                }
            }
            _ => return Err(image_library_error("nonresident-or-custom-value", "")),
        }
        if n > self.max_value_bytes - self.bytes {
            return Err(image_library_error("resource-limit", ""));
        }
        self.bytes += n;
        Ok(())
    }
}
pub(crate) fn preflight_library_object(
    obj: &Object,
    o: &ImageLibraryOptions,
) -> Result<(), SrError> {
    let mut b = Budget {
        items: 0,
        bytes: 0,
        max_items: o.max_items * 64,
        max_value_bytes: o.max_value_bytes,
    };
    fn visit(
        elements: &[Element],
        depth: i64,
        o: &ImageLibraryOptions,
        b: &mut Budget,
    ) -> Result<(), SrError> {
        if depth > o.max_depth * 4 + 4 {
            return Err(image_library_error("resource-limit", ""));
        }
        if elements.len() as i64 > b.max_items - b.items {
            return Err(image_library_error("resource-limit", ""));
        }
        for e in elements {
            b.resident(e)?;
            if let Value::Sequence(items) = &e.value {
                for item in items {
                    visit(&item.elements, depth + 1, o, b)?;
                }
            }
        }
        Ok(())
    }
    if obj.len() as i64 > b.max_items {
        return Err(image_library_error("resource-limit", ""));
    }
    visit(obj.elements(), 0, o, &mut b)
}
pub(crate) fn structured_image_library_needed(obj: &Object) -> Result<bool, SrError> {
    let o = default_image_library_options();
    preflight_library_object(obj, &o)?;
    let mut rich = false;
    let mut groups = 0;
    let mut dec = Decoder::default();
    fn visit(
        node: &Object,
        depth: i64,
        o: &ImageLibraryOptions,
        dec: &mut Decoder,
        rich: &mut bool,
        groups: &mut i64,
    ) -> Result<(), SrError> {
        if depth > o.max_depth {
            return Err(image_library_error("resource-limit", ""));
        }
        if dec.string(Some(node), tg::RELATIONSHIP_TYPE) == RELATIONSHIP_HAS_ACQ_CONTEXT {
            *rich = true;
        }
        for tag in [tg::CONCEPT_NAME_CODE_SEQ, tg::CONCEPT_CODE_SEQ] {
            if let Some(codes) = node.get_sequence(tag) {
                for code in &codes {
                    for el in code.elements() {
                        if el.tag() != tg::CODE_VALUE
                            && el.tag() != tg::CODING_SCHEME
                            && el.tag() != tg::CODE_MEANING
                        {
                            *rich = true;
                        }
                    }
                }
            }
        }
        if let Some(ids) = node.get_sequence(tg::CONTENT_TEMPLATE_SEQUENCE)
            && let Some(first) = ids.first()
            && (depth > 0
                || dec.string(Some(first), tg::TEMPLATE_IDENTIFIER) != "1600"
                || first.elements().len() > 2)
        {
            *rich = true;
        }
        let (children, _) = dec.sequence(Some(node), tg::CONTENT_SEQUENCE);
        for child in &children {
            if depth == 0 {
                let c = super::read::decode_code(dec, Some(child), tg::CONCEPT_NAME_CODE_SEQ).0;
                if matches_concept(&c, &concept::image_library_group()) {
                    *groups += 1;
                }
            }
            visit(child, depth + 1, o, dec, rich, groups)?;
        }
        match dec.err.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    visit(obj, 0, &o, &mut dec, &mut rich, &mut groups)?;
    Ok(rich || groups > 1)
}
