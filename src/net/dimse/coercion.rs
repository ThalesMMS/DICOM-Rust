#![forbid(unsafe_code)]
use super::controls::context_err;
use super::error::{Error, Kind, Result};
use super::store::{
    STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH, scp_status_of, storage_data_set_error_status,
};
use crate::net::ul::Ctx;
use crate::object::clone::{CloneFileOptions, clone_file_with_options, default_clone_file_options};
use crate::object::parser::source::MemFile;
use crate::object::{
    self, DataSet, Element, ElementHeader, File, Object, ReadFileOptions, Syntax, Tag, Value, Vr,
};
use sha2::{Digest, Sha256};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
pub type AttributeCoercion =
    Arc<dyn Fn(&mut Object) -> std::result::Result<Vec<Element>, Error> + Send + Sync>;
pub(crate) type DataSetEncoder<'a> = dyn Fn(&Ctx, &Syntax) -> Result<Vec<u8>> + 'a;
#[derive(Debug)]
pub struct StoreCoercionBudget {
    maximum: i64,
    used: Mutex<i64>,
}
impl StoreCoercionBudget {
    pub fn new(maximum: i64) -> Result<Arc<StoreCoercionBudget>> {
        if maximum <= 0 {
            return Err(Error::sentinel(Kind::StoreInvalidOptions));
        }
        Ok(Arc::new(StoreCoercionBudget {
            maximum,
            used: Mutex::new(0),
        }))
    }
    pub fn reserve(&self, maximum: i64) -> Result<i64> {
        let mut used = self.used.lock().unwrap();
        let available = self.maximum - *used;
        if available <= 0 {
            return Err(Error::sentinel(Kind::StoreResourceLimit));
        }
        let take = maximum.min(available);
        *used += take;
        Ok(take)
    }
    pub fn release(&self, bytes: i64) {
        *self.used.lock().unwrap() -= bytes;
    }
    pub fn used(&self) -> i64 {
        *self.used.lock().unwrap()
    }
}
#[derive(Clone, Default)]
pub struct StoreCoercionOptions {
    pub coercion: Option<AttributeCoercion>,
    pub clone_options: CloneFileOptions,
    pub budget: Option<Arc<StoreCoercionBudget>>,
    pub(crate) original_class: String,
    pub(crate) original_instance: String,
}
pub(crate) fn normalize_store_coercion_options(
    opts: StoreCoercionOptions,
) -> (StoreCoercionOptions, Option<Error>) {
    if opts.coercion.is_none() {
        return (opts, Some(Error::sentinel(Kind::StoreInvalidOptions)));
    }
    let c = opts.clone_options;
    if c.max_element_bytes < 0
        || c.max_pixel_data_bytes < 0
        || c.max_total_bytes < 0
        || c.max_sequence_depth < 0
        || c.max_elements < 0
        || c.max_items < 0
        || c.max_fragments < 0
    {
        return (opts, Some(Error::sentinel(Kind::StoreInvalidOptions)));
    }
    let d = default_clone_file_options();
    let mut o = opts;
    let c = &mut o.clone_options;
    for (v, dv) in [
        (&mut c.max_element_bytes, d.max_element_bytes),
        (&mut c.max_pixel_data_bytes, d.max_pixel_data_bytes),
        (&mut c.max_total_bytes, d.max_total_bytes),
        (&mut c.max_sequence_depth, d.max_sequence_depth),
        (&mut c.max_elements, d.max_elements),
        (&mut c.max_items, d.max_items),
        (&mut c.max_fragments, d.max_fragments),
    ] {
        if *v == 0 {
            *v = dv;
        }
    }
    (o, None)
}
fn coercion_read_options(opts: &StoreCoercionOptions) -> ReadFileOptions {
    let c = opts.clone_options;
    ReadFileOptions {
        max_element_bytes: c.max_element_bytes,
        max_pixel_data_bytes: c.max_pixel_data_bytes,
        max_total_bytes: c.max_total_bytes,
        max_sequence_depth: c.max_sequence_depth,
        max_elements: c.max_elements,
        max_fragments: c.max_fragments,
        ..Default::default()
    }
}
pub fn store_coercion_error(cause: Error) -> Error {
    let limit =
        storage_data_set_error_status(&cause) == super::store::STATUS_C_STORE_OUT_OF_RESOURCES;
    let mut e = cause
        .relabel(Kind::StoreCoercion.text())
        .also(Kind::StoreCoercion);
    if limit {
        e = e.also(Kind::ParserResourceLimit);
    }
    e
}
fn read_coercion_copy(
    ctx: &Ctx,
    write: &DataSetEncoder<'_>,
    syntax: Syntax,
    opts: &StoreCoercionOptions,
) -> Result<Object> {
    let written = match catch_unwind(AssertUnwindSafe(|| write(ctx, &syntax))) {
        Ok(r) => r,
        Err(_) => Err(Error::sentinel(Kind::StoreInvalidSource)),
    };
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    let bytes = written?;
    object::read_data_set_with_options(
        Box::new(MemFile::new(std::rc::Rc::from(bytes.as_slice()))),
        syntax,
        coercion_read_options(opts),
    )
    .map_err(Error::from)
}
fn invoke_store_coercion(
    coercion: &AttributeCoercion,
    dataset: &mut Object,
) -> Result<Vec<Element>> {
    match catch_unwind(AssertUnwindSafe(|| coercion(dataset))) {
        Ok(r) => r,
        Err(_) => Err(Error::sentinel(Kind::StoreCoercion)),
    }
}
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub(crate) fn coerced_store_identity(dataset: &Object) -> Result<(String, String)> {
    let class = dataset.get_uid(TAG_SOP_CLASS_UID);
    let instance = dataset.get_uid(TAG_SOP_INSTANCE_UID);
    let (Some(class), Some(instance)) = (class, instance) else {
        return Err(Error::sentinel(Kind::StoreCoercionIdentity));
    };
    if !object::is_valid_uid(class.as_bytes()) || !object::is_valid_uid(instance.as_bytes()) {
        return Err(Error::sentinel(Kind::StoreCoercionIdentity));
    }
    if dataset
        .elements()
        .iter()
        .any(|e| e.tag().group == 0x0000 || e.tag().group == 0x0002)
    {
        return Err(Error::sentinel(Kind::StoreCoercionIdentity));
    }
    Ok((class, instance))
}
fn ui(tag: Tag, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::UI,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn coercion_file(dataset: Object, syntax: Syntax) -> Result<File> {
    let (class, instance) = coerced_store_identity(&dataset)?;
    Ok(File {
        preamble: Vec::new(),
        meta: Object::from_elements(vec![
            ui(Tag::new(0x0002, 0x0002), &class),
            ui(Tag::new(0x0002, 0x0003), &instance),
            ui(Tag::new(0x0002, 0x0010), syntax.uid),
        ]),
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
fn protected_pixel_elements(elements: &[Element]) -> Vec<Element> {
    let mut out = Vec::new();
    for e in elements {
        let g = e.tag().group;
        if g == 0x0028 || g == 0x7FE0 || g & 0xFF00 == 0x6000 {
            out.push(e.clone());
            continue;
        }
        if let Value::Sequence(items) = &e.value {
            let mut found = false;
            let protected: Vec<DataSet> = items
                .iter()
                .map(|item| {
                    let els = protected_pixel_elements(&item.elements);
                    found = found || !els.is_empty();
                    DataSet {
                        elements: els,
                        item_offset: None,
                    }
                })
                .collect();
            if found {
                out.push(Element {
                    header: e.header,
                    value: Value::Sequence(protected),
                });
            }
        }
    }
    out
}
fn store_pixel_fingerprint(dataset: &Object, syntax: Syntax) -> Result<[u8; 32]> {
    let mut subset = Object::from_elements(protected_pixel_elements(dataset.elements()));
    subset.set_value_big_endian(dataset.value_big_endian());
    let bytes = object::writer::write_data_set(&subset, syntax.uid)?;
    Ok(Sha256::digest(&bytes).into())
}
fn validate_coercion_items(elements: &[Element], remaining: &mut i64) -> Result<()> {
    for e in elements {
        if let Value::Sequence(items) = &e.value {
            if items.len() as i64 > *remaining {
                return Err(Error::bare_with(
                    "dicom: maximum element count exceeded",
                    vec![super::error::Cause::Dimse(Kind::ParserResourceLimit)],
                ));
            }
            *remaining -= items.len() as i64;
            for item in items {
                validate_coercion_items(&item.elements, remaining)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn prepare_store_coercion(
    ctx: &Ctx,
    write: &DataSetEncoder<'_>,
    syntax: Syntax,
    opts: &StoreCoercionOptions,
) -> Result<(File, bool)> {
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    let mut working = read_coercion_copy(ctx, write, syntax, opts).map_err(store_coercion_error)?;
    let mut remaining = opts.clone_options.max_items;
    validate_coercion_items(working.elements(), &mut remaining).map_err(store_coercion_error)?;
    if !opts.original_class.is_empty() {
        let same = matches!(coerced_store_identity(&working),
            Ok((c, i)) if c == opts.original_class && i == opts.original_instance);
        if !same {
            return Err(store_coercion_error(Error::sentinel(
                Kind::StoreSourceChanged,
            )));
        }
    }
    let before = store_pixel_fingerprint(&working, syntax).map_err(store_coercion_error)?;
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    let Some(coercion) = &opts.coercion else {
        return Err(store_coercion_error(Error::sentinel(
            Kind::StoreInvalidOptions,
        )));
    };
    let modified = invoke_store_coercion(coercion, &mut working).map_err(store_coercion_error)?;
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    let file = coercion_file(working, syntax).map_err(store_coercion_error)?;
    let file = clone_file_with_options(&file, opts.clone_options)
        .map_err(|e| store_coercion_error(Error::from(e)))?;
    coerced_store_identity(&file.dataset).map_err(store_coercion_error)?;
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    file.dataset
        .check_pixel_data_encoding(&syntax)
        .map_err(|e| store_coercion_error(Error::from(e)))?;
    let after = store_pixel_fingerprint(&file.dataset, syntax).map_err(store_coercion_error)?;
    if before != after {
        return Err(store_coercion_error(Error::sentinel(
            Kind::StoreCoercionPixels,
        )));
    }
    Ok((file, !modified.is_empty()))
}
pub(crate) fn storage_coercion_error_status(err: &Error) -> u16 {
    if let Some((status, _)) = scp_status_of(err, "C-STORE")
        && (status & 0xFF00 == 0xA700 || status & 0xFF00 == 0xA900 || status & 0xF000 == 0xC000)
    {
        return status;
    }
    if err.is(Kind::StoreCoercionIdentity) {
        return STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH;
    }
    storage_data_set_error_status(err)
}
