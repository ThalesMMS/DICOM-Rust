#![forbid(unsafe_code)]
use super::error::{Error, Kind};
use super::file::{File, ReadFileOptions, read_file_with_options};
use super::obj::{Object, strings_encoded_length};
use super::types::{DataSet, Element, Tag, UNDEFINED_LENGTH, Value};
pub const ERR_INVALID_CLONE_OPTIONS: &str = "dicom: invalid clone options";
const DEFAULT_CLONE_MAX_BYTES: i64 = 2 << 30;
const DEFAULT_CLONE_MAX_ELEMENTS: i64 = 100_000;
const DEFAULT_CLONE_MAX_DEPTH: i64 = 64;
const DEFAULT_CLONE_MAX_ITEMS: i64 = 100_000;
const DEFAULT_CLONE_MAX_FRAGMENTS: i64 = 100_000;
const MAX_DEPTH_TEXT: &str = "dicom: maximum sequence nesting depth exceeded";
const MAX_ELEMENTS_TEXT: &str = "dicom: maximum element count exceeded";
const MAX_FRAGMENTS_TEXT: &str = "dicom: maximum fragment count exceeded";
const MAX_PIXEL_TEXT: &str = "dicom: maximum Pixel Data size exceeded";
const MAX_ELEMENT_BYTES_TEXT: &str = "dicom: maximum element value size exceeded";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloneFileOptions {
    pub max_element_bytes: i64,
    pub max_pixel_data_bytes: i64,
    pub max_total_bytes: i64,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_fragments: i64,
}
pub fn default_clone_file_options() -> CloneFileOptions {
    CloneFileOptions {
        max_element_bytes: DEFAULT_CLONE_MAX_BYTES,
        max_pixel_data_bytes: DEFAULT_CLONE_MAX_BYTES,
        max_total_bytes: DEFAULT_CLONE_MAX_BYTES,
        max_sequence_depth: DEFAULT_CLONE_MAX_DEPTH,
        max_elements: DEFAULT_CLONE_MAX_ELEMENTS,
        max_items: DEFAULT_CLONE_MAX_ITEMS,
        max_fragments: DEFAULT_CLONE_MAX_FRAGMENTS,
    }
}
pub(crate) fn normalize_clone_file_options(
    opts: CloneFileOptions,
) -> Result<CloneFileOptions, Error> {
    let mut o = opts;
    if o.max_element_bytes < 0
        || o.max_pixel_data_bytes < 0
        || o.max_total_bytes < 0
        || o.max_sequence_depth < 0
        || o.max_elements < 0
        || o.max_items < 0
        || o.max_fragments < 0
    {
        return Err(Error::msg(format!(
            "{ERR_INVALID_CLONE_OPTIONS}: limits must not be negative"
        )));
    }
    let d = default_clone_file_options();
    if o.max_element_bytes == 0 {
        o.max_element_bytes = d.max_element_bytes;
    }
    if o.max_pixel_data_bytes == 0 {
        o.max_pixel_data_bytes = d.max_pixel_data_bytes;
    }
    if o.max_total_bytes == 0 {
        o.max_total_bytes = d.max_total_bytes;
    }
    if o.max_sequence_depth == 0 {
        o.max_sequence_depth = d.max_sequence_depth;
    }
    if o.max_elements == 0 {
        o.max_elements = d.max_elements;
    }
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_fragments == 0 {
        o.max_fragments = d.max_fragments;
    }
    Ok(o)
}
pub fn clone_file(src: &File) -> Result<File, Error> {
    clone_file_with_options(src, CloneFileOptions::default())
}
pub fn clone_file_with_options(src: &File, opts: CloneFileOptions) -> Result<File, Error> {
    let normalized = normalize_clone_file_options(opts)?;
    validate_clone_file_structure(src, normalized)?;
    let bytes = super::writer::write_file(src)?;
    read_file_with_options(
        &bytes,
        ReadFileOptions {
            max_element_bytes: normalized.max_element_bytes,
            max_pixel_data_bytes: normalized.max_pixel_data_bytes,
            max_total_bytes: normalized.max_total_bytes,
            max_sequence_depth: normalized.max_sequence_depth,
            max_elements: normalized.max_elements,
            max_fragments: normalized.max_fragments,
            ..Default::default()
        },
    )
    .map_err(|(_, e)| e)
}
struct Budget {
    opts: CloneFileOptions,
    elements: i64,
    items: i64,
    fragments: i64,
    value_bytes: i64,
}
fn validate_clone_file_structure(src: &File, opts: CloneFileOptions) -> Result<(), Error> {
    let mut b = Budget {
        opts,
        elements: 0,
        items: 0,
        fragments: 0,
        value_bytes: 0,
    };
    if !src.preamble.is_empty() {
        b.add_value_bytes(src.preamble.len() as i64, Tag::new(0, 0))?;
    }
    b.visit_object(&src.meta, 0)?;
    b.visit_object(&src.dataset, 0)
}
impl Budget {
    fn visit_object(&mut self, obj: &Object, depth: i64) -> Result<(), Error> {
        self.visit_elements(obj.elements(), depth)
    }
    fn visit_elements(&mut self, elements: &[Element], depth: i64) -> Result<(), Error> {
        if depth > self.opts.max_sequence_depth {
            return Err(Error::msg(format!(
                "{MAX_DEPTH_TEXT}: got {depth}, limit {}",
                self.opts.max_sequence_depth
            )));
        }
        if elements.len() as i64 > self.opts.max_elements - self.elements {
            return Err(Error::msg(format!(
                "{MAX_ELEMENTS_TEXT}: got more than {} elements",
                self.opts.max_elements
            )));
        }
        self.elements += elements.len() as i64;
        for e in elements {
            self.visit_element(e, depth)?;
        }
        Ok(())
    }
    fn visit_data_set(&mut self, ds: &DataSet, depth: i64) -> Result<(), Error> {
        self.visit_elements(&ds.elements, depth)
    }
    fn visit_element(&mut self, e: &Element, depth: i64) -> Result<(), Error> {
        match &e.value {
            Value::Sequence(items) => {
                if depth >= self.opts.max_sequence_depth && !items.is_empty() {
                    return Err(Error::msg(format!(
                        "{MAX_DEPTH_TEXT}: got more than {} levels",
                        self.opts.max_sequence_depth
                    )));
                }
                if items.len() as i64 > self.opts.max_items - self.items {
                    return Err(Error::msg(format!(
                        "{MAX_ELEMENTS_TEXT}: got more than {} sequence items",
                        self.opts.max_items
                    )));
                }
                self.items += items.len() as i64;
                for item in items {
                    self.visit_data_set(item, depth + 1)?;
                }
                Ok(())
            }
            Value::Fragments {
                offset_table,
                fragments,
            } => {
                if fragments.len() as i64 > self.opts.max_fragments - self.fragments {
                    return Err(Error::msg(format!(
                        "{MAX_FRAGMENTS_TEXT}: got more than {} fragments",
                        self.opts.max_fragments
                    )));
                }
                self.fragments += fragments.len() as i64;
                let mut element_bytes = offset_table.len() as i64;
                for f in fragments {
                    let n = f.len() as i64;
                    if n > self.opts.max_pixel_data_bytes - element_bytes {
                        return Err(Error::msg(format!(
                            "{MAX_PIXEL_TEXT}: element {} exceeds {} bytes",
                            e.tag(),
                            self.opts.max_pixel_data_bytes
                        )));
                    }
                    element_bytes += n;
                }
                self.add_element_bytes(e, element_bytes)
            }
            Value::Nil => {
                if !e.header.length_set || e.header.length == UNDEFINED_LENGTH {
                    return Ok(());
                }
                self.add_element_bytes(e, e.header.length as i64)
            }
            Value::Raw(r) => self.add_element_bytes(e, r.len() as i64),
            Value::Strings(s) => self.add_element_bytes(e, strings_encoded_length(s) as i64),
            Value::BulkData(_) => Ok(()),
        }
    }
    fn add_element_bytes(&mut self, e: &Element, size: i64) -> Result<(), Error> {
        let (limit, text) = if e.tag() == Tag::PIXEL_DATA {
            (self.opts.max_pixel_data_bytes, MAX_PIXEL_TEXT)
        } else {
            (self.opts.max_element_bytes, MAX_ELEMENT_BYTES_TEXT)
        };
        if size > limit {
            return Err(Error::msg(format!(
                "{text}: element {} has {size} bytes, limit {limit}",
                e.tag()
            )));
        }
        self.add_value_bytes(size, e.tag())
    }
    fn add_value_bytes(&mut self, size: i64, tag: Tag) -> Result<(), Error> {
        if size < 0 || size > self.opts.max_total_bytes - self.value_bytes {
            return Err(Error::kind_suffix(
                Kind::MaxTotalBytes,
                &format!(
                    ": values through {tag} exceed {} bytes",
                    self.opts.max_total_bytes
                ),
            ));
        }
        self.value_bytes += size;
        Ok(())
    }
}
