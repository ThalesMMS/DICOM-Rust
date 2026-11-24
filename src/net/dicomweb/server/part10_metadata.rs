use super::ServerError;
use super::wado::{BulkDataPart, PartReader, ReadAt};
use crate::dicomjson::{self, Dataset, Limits, Options};
use crate::object::deferred::{BulkDataReference, DeferOptions, read_file_deferred};
use crate::object::parser::{
    BulkDataDescriptor, BulkDataPathStep, BulkDataRule, MAX_BULK_DATA_PATH_STEPS,
};
use crate::object::{
    DataSet, Element, Error as ObjectError, Object, ReadFileOptions, Tag, Value, Vr,
};
use std::io::{self, Read};
use std::sync::Arc;
pub const DEFAULT_PART10_BULK_DATA_THRESHOLD: i64 = 1024;
const DEFAULT_ELEMENTS: i64 = 1 << 20;
const DEFAULT_ELEMENT_BYTES: i64 = 64 << 20;
const DEFAULT_DATASET_BYTES: i64 = 64 << 20;
const DEFAULT_FRAGMENTS: i64 = 1 << 20;
const UID_PRIVATE_GE: &str = "1.2.840.113619.5.2";
pub(crate) type BulkDataUriFn<'a> = dyn Fn(&[BulkDataPathStep]) -> Result<String, ServerError> + 'a;
#[derive(Default)]
pub struct Part10MetadataOptions<'a> {
    pub bulk_data_threshold: i64,
    pub bulk_data_uri: Option<Box<BulkDataUriFn<'a>>>,
    pub max_elements: i64,
    pub max_element_bytes: i64,
    pub max_fragments: i64,
    pub max_dataset_bytes: i64,
}
const BULK_VRS: [Vr; 15] = [
    Vr::OB,
    Vr::OD,
    Vr::OF,
    Vr::OL,
    Vr::OV,
    Vr::OW,
    Vr::UN,
    Vr::FL,
    Vr::FD,
    Vr::SL,
    Vr::SS,
    Vr::SV,
    Vr::UL,
    Vr::US,
    Vr::UV,
];
impl Part10MetadataOptions<'_> {
    fn read_options(&self) -> Result<(ReadFileOptions, DeferOptions), ServerError> {
        if self.bulk_data_threshold < 0
            || self.max_elements < 0
            || self.max_element_bytes < 0
            || self.max_fragments < 0
            || self.max_dataset_bytes < 0
        {
            return Err(ServerError::InvalidServerOptions(
                "invalid Part 10 metadata options".into(),
            ));
        }
        let threshold = if self.bulk_data_threshold == 0 {
            DEFAULT_PART10_BULK_DATA_THRESHOLD
        } else {
            self.bulk_data_threshold
        };
        let mut rules: Vec<BulkDataRule> = BULK_VRS
            .iter()
            .map(|vr| BulkDataRule {
                vr: vr.as_str().to_string(),
                threshold,
                path: Vec::new(),
            })
            .collect();
        for element in [0x0008u16, 0x0009, 0x0010] {
            rules.push(BulkDataRule {
                vr: String::new(),
                threshold: 0,
                path: vec![BulkDataPathStep {
                    tag: Tag::new(0x7FE0, element),
                    item_index: -1,
                }],
            });
        }
        let pick = |v: i64, d: i64| if v == 0 { d } else { v };
        Ok((
            ReadFileOptions {
                max_elements: pick(self.max_elements, DEFAULT_ELEMENTS),
                max_element_bytes: pick(self.max_element_bytes, DEFAULT_ELEMENT_BYTES),
                max_fragments: pick(self.max_fragments, DEFAULT_FRAGMENTS),
                ..ReadFileOptions::default()
            },
            DeferOptions {
                defer_pixel_data: true,
                bulk_data_descriptor: Some(BulkDataDescriptor { rules }),
                ..DeferOptions::default()
            },
        ))
    }
}
fn read_error(err: &ObjectError) -> ServerError {
    let text = err.to_string();
    if [
        "dicom: maximum element count exceeded",
        "dicom: maximum element value size exceeded",
        "dicom: maximum fragment count exceeded",
        "dicom: maximum sequence nesting depth exceeded",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        ServerError::ResourceLimit
    } else {
        ServerError::Backend
    }
}
fn read_deferred(
    source: &[u8],
    opts: &Part10MetadataOptions<'_>,
) -> Result<crate::object::File, ServerError> {
    let (read, defer) = opts.read_options()?;
    let mem = crate::object::parser::source::MemFile::new(std::rc::Rc::from(source));
    let header = crate::object::read_part10_header_mem(&mem, &ReadFileOptions::default())
        .map_err(|_| ServerError::Backend)?;
    if header.transfer_syntax.deflated {
        return Err(ServerError::Unsupported);
    }
    read_file_deferred(source, true, read, &defer).map_err(|(_, e)| read_error(&e))
}
fn replace_deferred(
    root: &Object,
    elements: &mut [Element],
    prefix: &mut Vec<BulkDataPathStep>,
    uri: &BulkDataUriFn<'_>,
) -> Result<(), ServerError> {
    for e in elements.iter_mut() {
        let tag = e.tag();
        match &mut e.value {
            Value::Nil => {
                prefix.push(BulkDataPathStep {
                    tag,
                    item_index: -1,
                });
                let reference = root.deferred_bulk_data_reference(prefix);
                let path = prefix.clone();
                prefix.pop();
                if let Ok(Some(_)) = reference {
                    let text = uri(&path).map_err(|_| ServerError::Backend)?;
                    if text.is_empty() {
                        return Err(ServerError::Backend);
                    }
                    e.value = Value::BulkData(text);
                }
            }
            Value::Sequence(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    prefix.push(BulkDataPathStep {
                        tag,
                        item_index: index as i64,
                    });
                    let DataSet { elements, .. } = item;
                    let result = replace_deferred(root, elements, prefix, uri);
                    prefix.pop();
                    result?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn part10_metadata(
    source: &[u8],
    opts: &Part10MetadataOptions<'_>,
) -> Result<Dataset, ServerError> {
    let Some(uri) = opts.bulk_data_uri.as_deref() else {
        return Err(ServerError::InvalidServerOptions(
            "BulkDataURI is required".into(),
        ));
    };
    let file = read_deferred(source, opts)?;
    let maximum = if opts.max_dataset_bytes == 0 {
        DEFAULT_DATASET_BYTES
    } else {
        opts.max_dataset_bytes
    };
    let mut dataset = file.dataset.clone();
    let mut elements = dataset.elements().to_vec();
    replace_deferred(&file.dataset, &mut elements, &mut Vec::new(), uri)?;
    let big = dataset.value_big_endian();
    dataset = Object::from_elements(elements);
    dataset.set_value_big_endian(big);
    let mut options = Options {
        omit_group_length: true,
        limits: Limits {
            max_json_bytes: maximum,
            max_inline_binary_bytes: maximum,
            max_total_binary_bytes: maximum,
            ..Limits::default()
        },
        max_materialized_value_bytes: 1,
        ..Options::default()
    };
    let encoded = dicomjson::marshal_text(Some(&dataset), &mut options).map_err(|e| {
        if [
            dicomjson::ERR_MAX_JSON_BYTES,
            dicomjson::ERR_MAX_INLINE_BINARY_BYTES,
            dicomjson::ERR_MAX_TOTAL_BINARY_BYTES,
        ]
        .iter()
        .any(|s| e.contains(s))
        {
            ServerError::ResourceLimit
        } else {
            ServerError::Backend
        }
    })?;
    crate::net::dicomweb::dataset_from_json(&encoded).map_err(|_| ServerError::Backend)
}
fn little_endian_width(root: &Object, r: &BulkDataReference) -> usize {
    let reference = &r.reference;
    if reference.transfer_syntax_uid == UID_PRIVATE_GE {
        return r.normalization_width;
    }
    if !reference.big_endian {
        return 0;
    }
    if reference.tag == Tag::PIXEL_DATA && reference.vr == Vr::OW {
        let mut item: Option<Object> = Some(root.clone());
        for step in &reference.path[..reference.path.len() - 1] {
            let next = item
                .as_ref()
                .and_then(|o| o.get_sequence(step.tag))
                .and_then(|items| items.get(step.item_index as usize).cloned());
            if next.is_none() {
                return reference.vr.byte_order_width();
            }
            item = next;
        }
        if let Some(bits) = item.and_then(|o| o.get_int(Tag::new(0x0028, 0x0100)).ok())
            && (bits == 32 || bits == 64)
        {
            return (bits / 8) as usize;
        }
    }
    reference.vr.byte_order_width()
}
struct ValueReader {
    data: Arc<[u8]>,
    start: usize,
    end: usize,
    width: usize,
}
impl Read for ValueReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.start >= self.end {
            return Ok(0);
        }
        let mut n = buf.len().min(self.end - self.start);
        if self.width > 1 {
            n -= n % self.width;
            if n == 0 {
                n = self.width.min(self.end - self.start);
            }
        }
        let n = n.min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.start..self.start + n]);
        if self.width > 1 {
            for unit in buf[..n].chunks_exact_mut(self.width) {
                unit.reverse();
            }
        }
        self.start += n;
        Ok(n)
    }
}
impl PartReader for ValueReader {}
struct ValueReaderAt {
    data: Arc<[u8]>,
    start: usize,
    end: usize,
}
impl ReadAt for ValueReaderAt {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let at = self.start.saturating_add(offset as usize);
        if at >= self.end {
            return Ok(0);
        }
        let n = buf.len().min(self.end - at);
        buf[..n].copy_from_slice(&self.data[at..at + n]);
        Ok(n)
    }
}
pub fn part10_bulk_data(
    source: Arc<[u8]>,
    path: &[BulkDataPathStep],
    opts: &Part10MetadataOptions<'_>,
) -> Result<BulkDataPart, ServerError> {
    if path.is_empty() || path.len() > MAX_BULK_DATA_PATH_STEPS {
        return Err(ServerError::NotFound);
    }
    let file = read_deferred(&source, opts)?;
    let reference = match file.dataset.deferred_bulk_data_reference(path) {
        Ok(Some(r)) => r,
        _ => return Err(ServerError::NotFound),
    };
    if reference.reference.undefined_length {
        return Err(ServerError::Unsupported);
    }
    let (offset, length) = (reference.reference.offset, reference.reference.length);
    if offset < 0 || length < 0 {
        return Err(ServerError::Backend);
    }
    let start = (offset as usize).min(source.len());
    let end = (offset.saturating_add(length) as usize).min(source.len());
    let width = little_endian_width(&file.dataset, &reference);
    if width <= 1 {
        return Ok(BulkDataPart {
            content_type: "application/octet-stream".into(),
            reader: Some(Box::new(ValueReader {
                data: source.clone(),
                start,
                end,
                width: 0,
            })),
            size: length,
            reader_at: Some(Box::new(ValueReaderAt {
                data: source,
                start,
                end,
            })),
        });
    }
    if length % width as i64 != 0 {
        return Err(ServerError::Backend);
    }
    Ok(BulkDataPart {
        content_type: "application/octet-stream".into(),
        reader: Some(Box::new(ValueReader {
            data: source,
            start,
            end,
            width,
        })),
        size: length,
        reader_at: None,
    })
}
