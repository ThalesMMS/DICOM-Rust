use super::csa::CsaHeader;
use crate::dicomjson::sizedparse::base64_decode;
use crate::object::parser::source::MemFile;
use crate::object::parser::{Reader, ReaderOptions, TokenKind};
use crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
use crate::object::{self, Object, ReadFileOptions};
use crate::optimize::Context;
use std::fmt;
use std::rc::Rc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedKind {
    NoDataset,
    Invalid,
    Limit,
    Ambiguous,
    Canceled,
}
impl EmbeddedKind {
    pub fn text(self) -> &'static str {
        match self {
            EmbeddedKind::NoDataset => "dicom/siemens: no embedded CSA dataset",
            EmbeddedKind::Invalid => "dicom/siemens: invalid embedded CSA dataset",
            EmbeddedKind::Limit => "dicom/siemens: embedded CSA dataset exceeds limits",
            EmbeddedKind::Ambiguous => "dicom/siemens: ambiguous embedded CSA dataset",
            EmbeddedKind::Canceled => "context canceled",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedError {
    pub kind: EmbeddedKind,
    pub element_index: i64,
    pub value_index: i64,
    pub stage: &'static str,
    pub cause: Option<String>,
}
impl EmbeddedError {
    fn bare(kind: EmbeddedKind) -> EmbeddedError {
        EmbeddedError {
            kind,
            element_index: 0,
            value_index: 0,
            stage: "",
            cause: None,
        }
    }
    fn located(
        element: usize,
        value: i64,
        stage: &'static str,
        kind: EmbeddedKind,
        cause: Option<String>,
    ) -> EmbeddedError {
        EmbeddedError {
            kind,
            element_index: element as i64,
            value_index: value,
            stage,
            cause,
        }
    }
    pub fn is_located(&self) -> bool {
        !self.stage.is_empty()
    }
    pub fn is(&self, kind: EmbeddedKind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for EmbeddedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_located() {
            return write!(
                f,
                "dicom/siemens: embedded dataset element {} value {}: {}",
                self.element_index, self.value_index, self.stage
            );
        }
        f.write_str(self.kind.text())
    }
}
impl std::error::Error for EmbeddedError {}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmbeddedDatasetLimits {
    pub max_encoded_bytes: i64,
    pub max_decoded_bytes: i64,
    pub max_total_encoded_bytes: i64,
    pub max_total_decoded_bytes: i64,
    pub max_datasets: i64,
    pub max_header_elements: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
}
impl EmbeddedDatasetLimits {
    fn normalized(mut self) -> Result<EmbeddedDatasetLimits, EmbeddedError> {
        let defaults = [8 << 20, 6 << 20, 24 << 20, 16 << 20, 64, 4096, 65536, 32];
        let fields = [
            &mut self.max_encoded_bytes,
            &mut self.max_decoded_bytes,
            &mut self.max_total_encoded_bytes,
            &mut self.max_total_decoded_bytes,
            &mut self.max_datasets,
            &mut self.max_header_elements,
            &mut self.max_elements,
            &mut self.max_sequence_depth,
        ];
        for (field, default) in fields.into_iter().zip(defaults) {
            if *field < 0 {
                return Err(EmbeddedError::bare(EmbeddedKind::Limit));
            }
            if *field == 0 {
                *field = default;
            }
        }
        Ok(self)
    }
}
#[derive(Clone, Debug)]
pub struct EmbeddedDataset {
    pub name: String,
    pub element_index: usize,
    pub value_index: usize,
    pub dataset: Object,
}
pub fn is_embedded_dataset_field(name: &str) -> bool {
    matches!(
        name,
        "MRDiffusion"
            | "MR_ASL"
            | "FmriAcquisitionDescriptionSequence"
            | "FmriConditionsDataSequence"
            | "FmriResultSequence"
    )
}
struct Selection<'a> {
    name: &'a str,
    element: usize,
    value: usize,
    length: usize,
    encoded: &'a [u8],
}
fn canceled(ctx: &Context<'_>) -> Result<(), EmbeddedError> {
    ctx.err()
        .map_err(|_| EmbeddedError::bare(EmbeddedKind::Canceled))
}
pub fn embedded_dataset(
    header: &CsaHeader,
    ctx: Context<'_>,
    name: &str,
    limits: EmbeddedDatasetLimits,
) -> Result<Object, EmbeddedError> {
    let mut decoded = embedded_datasets_of(Some(header), ctx, name, true, limits)?;
    Ok(decoded.remove(0).dataset)
}
#[doc(hidden)]
pub fn embedded_datasets(
    header: &CsaHeader,
    ctx: Context<'_>,
    limits: EmbeddedDatasetLimits,
) -> Result<Vec<EmbeddedDataset>, EmbeddedError> {
    embedded_datasets_of(Some(header), ctx, "", false, limits)
}
#[doc(hidden)]
pub fn embedded_datasets_of(
    header: Option<&CsaHeader>,
    ctx: Context<'_>,
    name: &str,
    single: bool,
    limits: EmbeddedDatasetLimits,
) -> Result<Vec<EmbeddedDataset>, EmbeddedError> {
    canceled(&ctx)?;
    let limits = limits.normalized()?;
    let header = match header {
        Some(h) if !single || is_embedded_dataset_field(name) => h,
        _ => return Err(EmbeddedError::bare(EmbeddedKind::NoDataset)),
    };
    if header.elements.len() as i64 > limits.max_header_elements {
        return Err(EmbeddedError::bare(EmbeddedKind::Limit));
    }
    let mut selected: Vec<Selection<'_>> = Vec::new();
    let (mut encoded_total, mut decoded_total, mut matching) = (0i64, 0i64, 0);
    for (ei, element) in header.elements.iter().enumerate() {
        canceled(&ctx)?;
        if !is_embedded_dataset_field(&element.name) || (single && element.name != name) {
            continue;
        }
        matching += 1;
        if single && matching > 1 {
            return Err(EmbeddedError::located(
                ei,
                -1,
                "selection is ambiguous",
                EmbeddedKind::Ambiguous,
                None,
            ));
        }
        if element.values.len() as i64 > limits.max_datasets {
            return Err(EmbeddedError::located(
                ei,
                -1,
                "value count limit",
                EmbeddedKind::Limit,
                None,
            ));
        }
        for (vi, encoded) in element.values.iter().enumerate() {
            if encoded.is_empty() {
                continue;
            }
            let at = |stage, kind, cause| EmbeddedError::located(ei, vi as i64, stage, kind, cause);
            if single && !selected.is_empty() {
                return Err(at("selection is ambiguous", EmbeddedKind::Ambiguous, None));
            }
            let size = encoded.len() as i64;
            if selected.len() as i64 >= limits.max_datasets
                || size > limits.max_encoded_bytes
                || size > limits.max_total_encoded_bytes - encoded_total
            {
                return Err(at("encoded size/count limit", EmbeddedKind::Limit, None));
            }
            let Some(length) = decoded_length(encoded) else {
                return Err(at(
                    "invalid base64",
                    EmbeddedKind::Invalid,
                    Some(EmbeddedKind::Invalid.text().to_string()),
                ));
            };
            let length_i = length as i64;
            if length_i > limits.max_decoded_bytes
                || length_i > limits.max_total_decoded_bytes - decoded_total
            {
                return Err(at("decoded size limit", EmbeddedKind::Limit, None));
            }
            encoded_total += size;
            decoded_total += length_i;
            selected.push(Selection {
                name: &element.name,
                element: ei,
                value: vi,
                length,
                encoded,
            });
        }
    }
    if selected.is_empty() {
        return Err(EmbeddedError::bare(EmbeddedKind::NoDataset));
    }
    let mut out = Vec::with_capacity(selected.len());
    for selection in &selected {
        let dataset = decode(&ctx, selection, &limits)?;
        out.push(EmbeddedDataset {
            name: selection.name.to_string(),
            element_index: selection.element,
            value_index: selection.value,
            dataset,
        });
    }
    Ok(out)
}
fn decoded_length(encoded: &[u8]) -> Option<usize> {
    let (mut count, mut padding) = (0usize, 0usize);
    for &c in encoded {
        if c == b'\r' || c == b'\n' {
            continue;
        }
        if c == b'=' {
            padding += 1;
            if padding > 2 {
                return None;
            }
        } else if padding != 0 || !(c.is_ascii_alphanumeric() || c == b'+' || c == b'/') {
            return None;
        }
        count += 1;
    }
    if count == 0 || count % 4 != 0 {
        return None;
    }
    Some(count / 4 * 3 - padding)
}
const LIMIT_TEXTS: [&str; 5] = [
    "dicom: maximum element count exceeded",
    "dicom: maximum sequence nesting depth exceeded",
    "dicom: maximum element value size exceeded",
    "dicom: maximum fragment count exceeded",
    "dicom validation resource limit exceeded",
];
fn decode(
    ctx: &Context<'_>,
    s: &Selection<'_>,
    limits: &EmbeddedDatasetLimits,
) -> Result<Object, EmbeddedError> {
    canceled(ctx)?;
    let at =
        |stage, kind, cause| EmbeddedError::located(s.element, s.value as i64, stage, kind, cause);
    let decoded = match base64_decode(s.encoded, true) {
        Ok(decoded) if decoded.len() == s.length => decoded,
        Ok(_) => {
            return Err(at(
                "invalid base64",
                EmbeddedKind::Invalid,
                Some(EmbeddedKind::Invalid.text().to_string()),
            ));
        }
        Err((offset, _)) => {
            return Err(at(
                "invalid base64",
                EmbeddedKind::Invalid,
                Some(format!("illegal base64 data at input byte {offset}")),
            ));
        }
    };
    let opts = ReadFileOptions {
        max_element_bytes: limits.max_decoded_bytes,
        max_total_bytes: decoded.len() as i64 + 1,
        max_elements: limits.max_elements,
        max_sequence_depth: limits.max_sequence_depth,
        max_fragments: limits.max_elements,
        strict_reserved_bytes: true,
        ..ReadFileOptions::default()
    };
    let data: Rc<[u8]> = Rc::from(decoded);
    let rejected = |err: object::Error| {
        let text = err.to_string();
        let kind = if LIMIT_TEXTS.iter().any(|t| text.contains(t)) {
            EmbeddedKind::Limit
        } else {
            EmbeddedKind::Invalid
        };
        at("DICOM dataset rejected", kind, Some(text))
    };
    count_nodes(&data, &opts, limits.max_elements).map_err(rejected)?;
    let dataset = object::read_data_set_with_options(
        Box::new(MemFile::new(data)),
        EXPLICIT_VR_LITTLE_ENDIAN,
        opts,
    )
    .map_err(rejected)?;
    canceled(ctx)?;
    if dataset.is_empty() {
        return Err(at("DICOM dataset is empty", EmbeddedKind::Invalid, None));
    }
    Ok(dataset)
}
fn count_nodes(
    data: &Rc<[u8]>,
    opts: &ReadFileOptions,
    max_nodes: i64,
) -> Result<(), object::Error> {
    let reader_opts = ReaderOptions {
        max_element_bytes: opts.max_element_bytes,
        max_total_bytes: opts.max_total_bytes,
        max_sequence_depth: opts.max_sequence_depth,
        max_elements: opts.max_elements,
        max_fragments: opts.max_fragments,
        strict_reserved_bytes: opts.strict_reserved_bytes,
        ..ReaderOptions::default()
    };
    let mut reader = Reader::new(
        Box::new(MemFile::new(data.clone())),
        None,
        EXPLICIT_VR_LITTLE_ENDIAN,
        reader_opts,
    );
    let mut frames: Vec<bool> = Vec::new();
    let mut nodes = 0i64;
    loop {
        let tok = match reader.next() {
            Ok(tok) => tok,
            Err(err) if err.is(object::Kind::Eof) => return Ok(()),
            Err(err) => return Err(err),
        };
        let in_pixel = frames.last().copied().unwrap_or(false);
        let node = match tok.kind {
            TokenKind::StartSequence | TokenKind::StartPixelSequence => true,
            TokenKind::Element | TokenKind::StartItem => !in_pixel,
            TokenKind::EndSequence | TokenKind::EndItem => false,
        };
        if node {
            nodes += 1;
            if nodes > max_nodes {
                let (tag, vr, length) = tok.header.map_or((object::Tag::default(), None, 0), |h| {
                    let vr = (!h.vr.to_string().is_empty()).then_some(h.vr);
                    (h.tag, vr, h.length)
                });
                return Err(object::Error::parse(
                    object::error::Op::CheckElementCount,
                    tok.offset,
                    tag,
                    vr,
                    length,
                    Some(object::Error::msg(format!(
                        "dicom: maximum element count exceeded: got {nodes} element/item nodes, limit {max_nodes}"
                    ))),
                ));
            }
        }
        match tok.kind {
            TokenKind::StartSequence => frames.push(false),
            TokenKind::StartPixelSequence => frames.push(true),
            TokenKind::EndSequence => {
                frames.pop();
            }
            _ => {}
        }
    }
}
