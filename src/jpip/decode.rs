use super::client::{Client, Registry, Request, Response};
use super::databins::{DataBinSet, Reconstruction};
use super::message::{EOR_IMAGE_DONE, EOR_WINDOW_DONE, parse_stream};
use super::{ChainError, ClientError, Error, ErrorKind, Sentinel};
use crate::net::ul::Ctx;
use crate::object::transfer;
use crate::object::{Element, ElementHeader, Object, Tag, Value, Vr};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{PixelData, decode_frames, extract_metadata};
use crate::textfmt;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodedFrame {
    pub metadata: Metadata,
    pub data: Vec<u8>,
    pub content_type: String,
    pub cache_hit: bool,
    pub partial: bool,
}
const UID_JPEG2000: &str = "1.2.840.10008.1.2.4.91";
const UID_HTJ2K: &str = "1.2.840.10008.1.2.4.203";
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
impl Client {
    pub fn decode_frame(
        &self,
        ctx: &Ctx,
        request: &Request,
        dataset: Option<&Object>,
    ) -> Result<DecodedFrame, ClientError> {
        let invalid = || {
            Error::sentinel(
                ErrorKind::InvalidRequest,
                "decode",
                Sentinel::InvalidRequest,
            )
        };
        let Some(dataset) = dataset else {
            return Err(invalid().into());
        };
        if request.range.is_some() {
            return Err(invalid().into());
        }
        let mut response = self.retrieve(ctx, request)?;
        let mut partial = false;
        match response.content_type.as_str() {
            "image/jpp-stream" | "image/jpt-stream" => {
                let rebuilt = rebuild_stream(&response, request.frame as u64 + 1).map_err(|e| {
                    Error::new(
                        ErrorKind::IncrementalStream,
                        "decode",
                        ChainError::join(ChainError::sentinel(Sentinel::IncrementalStream), e),
                    )
                    .content_type(&response.content_type)
                })?;
                response.data = rebuilt.codestream;
                partial = !rebuilt.complete;
            }
            _ => {
                if !content_type_matches_reference(&response.content_type, request.reference.htj2k)
                {
                    return Err(Error::new(
                        ErrorKind::Decode,
                        "decode",
                        ChainError::join(
                            ChainError::sentinel(Sentinel::Decode),
                            ChainError::sentinel(Sentinel::UnsupportedContentType),
                        ),
                    )
                    .content_type(&response.content_type)
                    .into());
                }
            }
        }
        let mut decoder_dataset = dataset.clone();
        decoder_dataset.put(Element {
            header: ElementHeader {
                tag: TAG_NUMBER_OF_FRAMES,
                vr: Vr::IS,
                length: 2,
                length_set: true,
            },
            value: Value::Raw(b"1 ".to_vec()),
        });
        let uid = if request.reference.htj2k {
            UID_HTJ2K
        } else {
            UID_JPEG2000
        };
        let decode_error = |text: String| {
            Error::new(
                ErrorKind::Decode,
                "decode",
                ChainError::join_text(Sentinel::Decode, text),
            )
            .content_type(&response.content_type)
        };
        let frames = match self.registry {
            Registry::Empty => return Err(decode_error(codec_not_found(uid)).into()),
            Registry::Default => {
                let fragments = [std::mem::take(&mut response.data)];
                let pixel = PixelData::Encapsulated {
                    offset_table: &[],
                    fragments: &fragments,
                };
                decode_frames(uid, &pixel, &decoder_dataset)
                    .map_err(|e| decode_error(e.to_string()))?
            }
        };
        if frames.data.len() != 1 || frames.data[0].is_empty() {
            return Err(Error::sentinel(
                ErrorKind::CorruptResponse,
                "decode",
                Sentinel::CorruptResponse,
            )
            .content_type(&response.content_type)
            .into());
        }
        let mut metadata = extract_metadata(&decoder_dataset).map_err(|e| {
            Error::new(
                ErrorKind::Decode,
                "decode metadata",
                ChainError::join_text(Sentinel::Decode, e),
            )
        })?;
        metadata.number_of_frames = 1;
        let data = frames.data.into_iter().next().unwrap_or_default();
        Ok(DecodedFrame {
            metadata,
            data,
            content_type: response.content_type,
            cache_hit: response.cache_hit,
            partial,
        })
    }
}
fn codec_not_found(uid: &str) -> String {
    let mut text = format!(
        "dicom: no pixel data codec registered for transfer syntax: transfer syntax {}",
        textfmt::quote(uid)
    );
    if let Some(name) = transfer::lookup(uid)
        .map(|s| s.name)
        .filter(|n| !n.is_empty())
    {
        text.push_str(&format!(" ({name})"));
    }
    text.push_str(
        "; JPEG 2000 / HTJ2K transfer syntax is recognized, but there is no default decoder adapter; metadata and encapsulated Pixel Data can be preserved, or decode with a registry that has the JPEG 2000 codecs (DecoderRegistry::Builtin)",
    );
    text.push_str("; registered codecs: none");
    text
}
fn rebuild_stream(response: &Response, stream: u64) -> Result<Reconstruction, ChainError> {
    let parsed = parse_stream(&response.data)?;
    if !parsed.has_eor || parsed.eor != EOR_IMAGE_DONE && parsed.eor != EOR_WINDOW_DONE {
        return Err(ChainError::plain(
            "dicom: JPIP response ended before its window was done",
        ));
    }
    let mut set = DataBinSet::new(response.data.len() as i64 + 1);
    set.add_stream(&parsed)?;
    set.codestream(stream)
}
fn content_type_matches_reference(content_type: &str, htj2k: bool) -> bool {
    if htj2k {
        content_type == "image/jph" || content_type == "image/jphc"
    } else {
        content_type == "image/jp2"
    }
}
