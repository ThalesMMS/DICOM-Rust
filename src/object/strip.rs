#![forbid(unsafe_code)]
use super::deferred::ValueProvider;
use super::dict_retired::STD_RETIRED;
use super::error::Error;
use super::obj::Object;
use super::parser::bulk_data_vr;
use super::types::{DataSet, Element, UNDEFINED_LENGTH, Value, Vr};
pub const ERR_STRIP_ATTRIBUTES_OPTIONS: &str =
    "dicom: invalid attribute stripping options or source representation";
pub const ERR_STRIP_ATTRIBUTES_LIMIT: &str = "dicom: attribute stripping resource limit";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StripAttributesOptions {
    pub remove_private: bool,
    pub remove_retired: bool,
    pub max_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StripAttributesReport {
    pub visited_elements: i64,
    pub visited_items: i64,
    pub removed_private: i64,
    pub removed_retired: i64,
}
pub fn is_retired(tag: super::types::Tag) -> bool {
    STD_RETIRED.binary_search(&tag.key()).is_ok()
}
fn options_error() -> Error {
    Error::msg(ERR_STRIP_ATTRIBUTES_OPTIONS)
}
fn limit(name: &str) -> Error {
    Error::msg(format!("{ERR_STRIP_ATTRIBUTES_LIMIT}: {name}"))
}
struct State<'a> {
    opts: StripAttributesOptions,
    report: StripAttributesReport,
    root: &'a Object,
    provider: Option<&'a ValueProvider>,
    charged_elements: i64,
    charged_items: i64,
}
impl State<'_> {
    fn prepare(
        &mut self,
        elements: &[Element],
        depth: i64,
        nested: bool,
        discard: bool,
        ancestors: &[Option<i64>],
    ) -> Result<Option<Vec<Element>>, Error> {
        if elements.len() as i64 > self.opts.max_elements - self.charged_elements {
            return Err(limit("elements"));
        }
        self.charged_elements += elements.len() as i64;
        let mut out = Vec::with_capacity(elements.len());
        let mut changed = false;
        for elem in elements {
            if self.report.visited_elements >= self.opts.max_elements {
                return Err(limit("elements"));
            }
            self.report.visited_elements += 1;
            let tag = elem.tag();
            let mut remove = false;
            if self.opts.remove_private && tag.is_private() {
                self.report.removed_private += 1;
                remove = true;
            } else if self.opts.remove_retired && !tag.is_private() && is_retired(tag) {
                self.report.removed_retired += 1;
                remove = true;
            }
            let mut elem = elem.clone();
            if let Value::Sequence(items) = &elem.value {
                if elem.vr() != Vr::SQ {
                    return Err(options_error());
                }
                if !items.is_empty() && depth >= self.opts.max_depth {
                    return Err(limit("depth"));
                }
                if items.len() as i64 > self.opts.max_items - self.charged_items {
                    return Err(limit("items"));
                }
                self.charged_items += items.len() as i64;
                let mut new_items = Vec::with_capacity(items.len());
                let mut sequence_changed = false;
                for item in items {
                    self.report.visited_items += 1;
                    let mut path = ancestors.to_vec();
                    path.push(item.item_offset);
                    let kept =
                        self.prepare(&item.elements, depth + 1, true, discard || remove, &path)?;
                    match kept {
                        Some(kept) => {
                            sequence_changed = true;
                            new_items.push(DataSet {
                                elements: kept,
                                item_offset: item.item_offset,
                            });
                        }
                        None => new_items.push(item.clone()),
                    }
                }
                if sequence_changed {
                    elem.value = Value::Sequence(new_items);
                    changed = true;
                }
            } else if elem.vr() == Vr::SQ {
                return Err(options_error());
            } else if !discard && !remove && matches!(elem.value, Value::Nil) {
                self.check_nil(&elem, nested, ancestors)?;
            }
            if remove {
                changed = true;
            } else {
                out.push(elem);
            }
        }
        Ok(changed.then_some(out))
    }
    fn check_nil(
        &self,
        elem: &Element,
        nested: bool,
        ancestors: &[Option<i64>],
    ) -> Result<(), Error> {
        if !elem.header.length_set {
            return Err(options_error());
        }
        let length = elem.header.length;
        let requires_origin =
            length == UNDEFINED_LENGTH || length > 0 || !nested && self.provider.is_some();
        if !requires_origin {
            return Ok(());
        }
        let Some(provider) = self.provider else {
            return Err(options_error());
        };
        if !nested && self.root.ambiguous_deferred.contains(&elem.tag()) {
            return Err(options_error());
        }
        if !nested {
            if !provider.authenticated_root_primitive(elem) {
                return Err(options_error());
            }
            return Ok(());
        }
        if !bulk_data_vr(elem.vr()) {
            return Err(options_error());
        }
        let Some(Some(_)) = ancestors.last() else {
            return Err(options_error());
        };
        for ancestor in ancestors {
            match ancestor {
                Some(offset) if provider.item_bound(*offset) => {}
                _ => return Err(options_error()),
            }
        }
        Ok(())
    }
}
pub fn strip_attributes(
    obj: &mut Object,
    opts: StripAttributesOptions,
) -> Result<StripAttributesReport, Error> {
    let mut opts = opts;
    if opts.max_depth < 0 || opts.max_elements < 0 || opts.max_items < 0 {
        return Err(options_error());
    }
    if opts.max_depth == 0 {
        opts.max_depth = 64;
    }
    if opts.max_elements == 0 {
        opts.max_elements = 100_000;
    }
    if opts.max_items == 0 {
        opts.max_items = 100_000;
    }
    if !opts.remove_private && !opts.remove_retired {
        return Ok(StripAttributesReport::default());
    }
    if obj.elements().len() as i64 > opts.max_elements {
        return Err(limit("elements"));
    }
    let provider = obj.value_provider.clone();
    let (elements, report) = {
        let mut state = State {
            opts,
            report: StripAttributesReport::default(),
            root: obj,
            provider: provider.as_deref(),
            charged_elements: 0,
            charged_items: 0,
        };
        let elements = state.prepare(obj.elements(), 0, false, false, &[])?;
        (elements, state.report)
    };
    if let Some(elements) = elements {
        obj.replace_elements_retaining_state(elements);
    }
    Ok(report)
}
