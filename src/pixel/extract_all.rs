use super::pixeldata::PixelData;
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::{Element, Object, Tag, Value};
use std::fmt;
pub const PIXEL_DATA_PATH_NO_ITEM: i64 = -1;
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_ICON_IMAGE_SEQUENCE: Tag = Tag::new(0x0088, 0x0200);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractAllOptions {
    pub max_matches: i64,
    pub max_bytes: i64,
    pub max_sequence_depth: i64,
    pub max_fragments: i64,
}
pub fn default_extract_all_options() -> ExtractAllOptions {
    ExtractAllOptions {
        max_matches: 10_000,
        max_bytes: 2 << 30,
        max_sequence_depth: 64,
        max_fragments: 100_000,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractAllLimit {
    Matches,
    Bytes,
    Depth,
    Fragments,
}
impl ExtractAllLimit {
    pub fn as_str(self) -> &'static str {
        match self {
            ExtractAllLimit::Matches => "matches",
            ExtractAllLimit::Bytes => "bytes",
            ExtractAllLimit::Depth => "depth",
            ExtractAllLimit::Fragments => "fragments",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelDataContext {
    TopLevel,
    IconImageSequence,
    Sequence,
}
impl PixelDataContext {
    pub fn as_str(self) -> &'static str {
        match self {
            PixelDataContext::TopLevel => "top-level",
            PixelDataContext::IconImageSequence => "icon-image-sequence",
            PixelDataContext::Sequence => "sequence",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelDataPathStep {
    pub tag: Tag,
    pub item_index: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PixelDataPath(pub Vec<PixelDataPathStep>);
impl fmt::Display for PixelDataPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, step) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{}", step.tag)?;
            if step.item_index >= 0 {
                write!(f, "[{}]", step.item_index)?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedPixelData<'a> {
    pub context: PixelDataContext,
    pub path: PixelDataPath,
    pub data: PixelData<'a>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractAllError {
    InvalidOptions(&'static str),
    Limit {
        limit: ExtractAllLimit,
        maximum: i64,
    },
    Context(CtxKind),
    UnsupportedValue(String),
}
impl ExtractAllError {
    pub fn is_limit(&self) -> bool {
        matches!(self, ExtractAllError::Limit { .. })
    }
}
impl fmt::Display for ExtractAllError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractAllError::InvalidOptions(detail) => {
                write!(f, "dicom: invalid ExtractAll options: {detail}")
            }
            ExtractAllError::Limit { limit, maximum } => write!(
                f,
                "dicom: ExtractAll resource limit exceeded: {} maximum {maximum}",
                limit.as_str()
            ),
            ExtractAllError::Context(kind) => write!(
                f,
                "dicom: extract all Pixel Data: {}",
                if *kind == CtxKind::Canceled {
                    "context canceled"
                } else {
                    "context deadline exceeded"
                }
            ),
            ExtractAllError::UnsupportedValue(path) => write!(
                f,
                "dicom: extract Pixel Data at {path}: dicom: pixel data is not a supported value type"
            ),
        }
    }
}
impl std::error::Error for ExtractAllError {}
pub fn extract_all(obj: &Object) -> Result<Vec<ExtractedPixelData<'_>>, ExtractAllError> {
    extract_all_with_options(&Ctx::background(), obj, ExtractAllOptions::default())
}
pub fn extract_all_with_options<'a>(
    ctx: &Ctx,
    obj: &'a Object,
    opts: ExtractAllOptions,
) -> Result<Vec<ExtractedPixelData<'a>>, ExtractAllError> {
    let opts = normalize(opts)?;
    let mut state = State {
        ctx,
        opts,
        path: Vec::new(),
        out: Vec::new(),
        bytes: 0,
        fragments: 0,
    };
    state.context()?;
    state.elements(obj.elements(), 0)?;
    Ok(state.out)
}
fn normalize(mut o: ExtractAllOptions) -> Result<ExtractAllOptions, ExtractAllError> {
    if o.max_matches < 0 || o.max_bytes < 0 || o.max_sequence_depth < 0 || o.max_fragments < 0 {
        return Err(ExtractAllError::InvalidOptions(
            "limits must not be negative",
        ));
    }
    let d = default_extract_all_options();
    for (value, default) in [
        (&mut o.max_matches, d.max_matches),
        (&mut o.max_bytes, d.max_bytes),
        (&mut o.max_sequence_depth, d.max_sequence_depth),
        (&mut o.max_fragments, d.max_fragments),
    ] {
        if *value == 0 {
            *value = default;
        }
    }
    Ok(o)
}
struct State<'c, 'a> {
    ctx: &'c Ctx,
    opts: ExtractAllOptions,
    path: Vec<PixelDataPathStep>,
    out: Vec<ExtractedPixelData<'a>>,
    bytes: i64,
    fragments: i64,
}
impl<'a> State<'_, 'a> {
    fn context(&self) -> Result<(), ExtractAllError> {
        self.ctx
            .err()
            .map_or(Ok(()), |kind| Err(ExtractAllError::Context(kind)))
    }
    fn pixel_path(&self, tag: Tag) -> PixelDataPath {
        let mut steps = self.path.clone();
        steps.push(PixelDataPathStep {
            tag,
            item_index: PIXEL_DATA_PATH_NO_ITEM,
        });
        PixelDataPath(steps)
    }
    fn elements(&mut self, elements: &'a [Element], depth: i64) -> Result<(), ExtractAllError> {
        for element in elements {
            self.context()?;
            let tag = element.tag();
            if tag == TAG_PIXEL_DATA {
                if self.out.len() as i64 >= self.opts.max_matches {
                    return Err(ExtractAllError::Limit {
                        limit: ExtractAllLimit::Matches,
                        maximum: self.opts.max_matches,
                    });
                }
                let (data, payload, fragments) = match &element.value {
                    Value::Raw(raw) => (PixelData::Native(raw), raw.len() as i64, 0),
                    Value::Fragments {
                        offset_table,
                        fragments,
                    } => {
                        if fragments.len() as i64
                            > self.opts.max_fragments.saturating_sub(self.fragments)
                        {
                            return Err(ExtractAllError::Limit {
                                limit: ExtractAllLimit::Fragments,
                                maximum: self.opts.max_fragments,
                            });
                        }
                        let total = fragments.iter().fold(offset_table.len() as i64, |t, f| {
                            t.saturating_add(f.len() as i64)
                        });
                        (
                            PixelData::Encapsulated {
                                offset_table,
                                fragments,
                            },
                            total,
                            fragments.len() as i64,
                        )
                    }
                    _ => {
                        return Err(ExtractAllError::UnsupportedValue(
                            self.pixel_path(tag).to_string(),
                        ));
                    }
                };
                if payload > self.opts.max_bytes.saturating_sub(self.bytes) {
                    return Err(ExtractAllError::Limit {
                        limit: ExtractAllLimit::Bytes,
                        maximum: self.opts.max_bytes,
                    });
                }
                let path = self.pixel_path(tag);
                let in_icon = path.0.split_last().is_some_and(|(_, outer)| {
                    outer.iter().any(|step| step.tag == TAG_ICON_IMAGE_SEQUENCE)
                });
                let context = if path.0.len() <= 1 {
                    PixelDataContext::TopLevel
                } else if in_icon {
                    PixelDataContext::IconImageSequence
                } else {
                    PixelDataContext::Sequence
                };
                self.out.push(ExtractedPixelData {
                    context,
                    path,
                    data,
                });
                self.bytes = self.bytes.saturating_add(payload);
                self.fragments = self.fragments.saturating_add(fragments);
                continue;
            }
            let Value::Sequence(items) = &element.value else {
                continue;
            };
            if !items.is_empty() && depth >= self.opts.max_sequence_depth {
                return Err(ExtractAllError::Limit {
                    limit: ExtractAllLimit::Depth,
                    maximum: self.opts.max_sequence_depth,
                });
            }
            for (i, item) in items.iter().enumerate() {
                self.context()?;
                self.path.push(PixelDataPathStep {
                    tag,
                    item_index: i as i64,
                });
                let result = self.elements(&item.elements, depth.saturating_add(1));
                self.path.pop();
                result?;
            }
        }
        Ok(())
    }
}
