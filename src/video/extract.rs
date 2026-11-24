use super::{Error, Kind};
use crate::object::transfer;
use crate::object::{File, Object, Tag, Value};
use crate::textfmt::quote;
use crate::transfer::is_video_transfer_syntax;
use std::io::Write;
pub const DEFAULT_MAX_BYTES: i64 = 1 << 30;
const PROBE_BYTES: usize = 4096;
const TAG_RECOMMENDED_DISPLAY_FRAME_RATE: Tag = Tag::new(0x0008, 0x2144);
const TAG_CINE_RATE: Tag = Tag::new(0x0018, 0x0040);
const TAG_FRAME_TIME: Tag = Tag::new(0x0018, 0x1063);
const TAG_FRAME_TIME_VECTOR: Tag = Tag::new(0x0018, 0x1065);
const TAG_ACTUAL_FRAME_DURATION: Tag = Tag::new(0x0018, 0x1242);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
pub const CODEC_MPEG2: &str = "MPEG-2";
pub const CODEC_H264: &str = "H.264/AVC";
pub const CODEC_HEVC: &str = "H.265/HEVC";
pub const CONTAINER_MPEG_TRANSPORT: &str = "MPEG-TS";
pub const CONTAINER_MPEG_ELEMENTARY: &str = "MPEG-ES";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: i64,
    pub max_fragments: i64,
}
impl Limits {
    pub(crate) fn normalized(mut self) -> Limits {
        if self.max_bytes <= 0 {
            self.max_bytes = DEFAULT_MAX_BYTES;
        }
        if self.max_fragments <= 0 {
            self.max_fragments = super::DEFAULT_MAX_FRAGMENTS as i64;
        }
        self
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub transfer_syntax_uid: String,
    pub transfer_syntax_name: String,
    pub codec: &'static str,
    pub container: &'static str,
    pub extension: &'static str,
    pub mime_type: &'static str,
    pub fragment_count: i64,
    pub size: i64,
    pub rows: i64,
    pub columns: i64,
    pub number_of_frames: i64,
    pub frame_rate: f64,
    pub frame_time_millis: f64,
}
fn fragments_error(detail: impl std::fmt::Display) -> Error {
    Error::new(Kind::MalformedFragments, detail)
}
pub fn inspect(file: &File, limits: Limits) -> (Info, Result<(), Error>) {
    copy(file, &mut std::io::sink(), limits)
}
pub fn metadata(file: &File) -> Result<Info, Error> {
    base_info(file)
}
pub fn copy(file: &File, dst: &mut dyn Write, limits: Limits) -> (Info, Result<(), Error>) {
    let mut info = match base_info(file) {
        Ok(info) => info,
        Err(err) => return (Info::default(), Err(err)),
    };
    let limits = limits.normalized();
    let Some(elem) = file.dataset.get(Tag::PIXEL_DATA) else {
        return (info, Err(Error::bare(Kind::PixelDataUnavailable)));
    };
    let mut probe = PrefixWriter {
        dst,
        prefix: Vec::new(),
    };
    let result = match &elem.value {
        Value::Fragments {
            offset_table,
            fragments,
        } => copy_materialized(offset_table, fragments, &mut probe, limits, &mut info),
        Value::Nil => copy_deferred(&file.dataset, &mut probe, limits, &mut info),
        other => {
            let name = match other {
                Value::Sequence(_) => "core.SequenceValue",
                Value::Strings(_) => "core.StringValue",
                Value::BulkData(_) => "core.BulkDataValue",
                _ => "core.RawValue",
            };
            Err(fragments_error(format!("Pixel Data value has type {name}")))
        }
    };
    if let Err(err) = result {
        return (info, Err(err));
    }
    match detect_container(info.codec, &probe.prefix) {
        Ok((container, extension, mime_type)) => {
            (info.container, info.extension, info.mime_type) = (container, extension, mime_type);
            (info, Ok(()))
        }
        Err(err) => (info, Err(err)),
    }
}
fn base_info(file: &File) -> Result<Info, Error> {
    let uid = transfer::normalize_uid(file.transfer_syntax.uid);
    if !is_video_transfer_syntax(uid) {
        return Err(Error::new(Kind::NotVideoTransferSyntax, quote(uid)));
    }
    let mut info = Info {
        transfer_syntax_uid: uid.to_string(),
        transfer_syntax_name: file.transfer_syntax.name.trim().to_string(),
        codec: codec_for_syntax(uid),
        ..Info::default()
    };
    if info.transfer_syntax_name.is_empty()
        && let Some(syntax) = transfer::lookup(uid)
    {
        info.transfer_syntax_name = syntax.name.to_string();
    }
    let positive = |tag: Tag| file.get_int(tag).ok().filter(|v| *v > 0);
    info.rows = positive(TAG_ROWS).unwrap_or(0);
    info.columns = positive(TAG_COLUMNS).unwrap_or(0);
    info.number_of_frames = positive(TAG_NUMBER_OF_FRAMES).unwrap_or(0);
    (info.frame_rate, info.frame_time_millis) = timing(file);
    Ok(info)
}
fn timing(file: &File) -> (f64, f64) {
    let (mut rate, mut frame_time) = (0.0f64, 0.0f64);
    if let Ok(value) = file.get_float(TAG_FRAME_TIME)
        && value > 0.0
        && value.is_finite()
    {
        frame_time = value;
        rate = 1000.0 / value;
    }
    let positive = |tag: Tag| file.get_int(tag).ok().filter(|v| *v > 0);
    if let Some(value) = positive(TAG_RECOMMENDED_DISPLAY_FRAME_RATE) {
        rate = value as f64;
        if frame_time <= 0.0 {
            frame_time = 1000.0 / rate;
        }
    } else if rate <= 0.0
        && let Some(value) = positive(TAG_CINE_RATE)
    {
        rate = value as f64;
        frame_time = 1000.0 / rate;
    }
    if frame_time <= 0.0
        && let Ok(vector) = file.get_floats(TAG_FRAME_TIME_VECTOR)
        && vector.len() > 1
    {
        let mut total = 0.0f64;
        let mut valid = vector[0] == 0.0;
        for &increment in &vector[1..] {
            if increment <= 0.0 || !increment.is_finite() {
                valid = false;
                break;
            }
            total += increment;
        }
        if valid {
            frame_time = total / (vector.len() - 1) as f64;
            rate = 1000.0 / frame_time;
        }
    }
    if frame_time <= 0.0
        && let Some(value) = positive(TAG_ACTUAL_FRAME_DURATION)
    {
        frame_time = value as f64;
        rate = 1000.0 / frame_time;
    }
    (rate, frame_time)
}
fn write_once(dst: &mut dyn Write, data: &[u8]) -> (usize, Option<std::io::Error>) {
    match dst.write(data) {
        Ok(n) => (n.min(data.len()), None),
        Err(err) => (0, Some(err)),
    }
}
fn copy_materialized(
    offset_table: &[u8],
    fragments: &[Vec<u8>],
    dst: &mut dyn Write,
    limits: Limits,
    info: &mut Info,
) -> Result<(), Error> {
    if !offset_table.is_empty() {
        return Err(fragments_error(
            "Basic Offset Table must be empty for video",
        ));
    }
    validate_fragment_count(
        &info.transfer_syntax_uid,
        fragments.len() as i64,
        limits.max_fragments,
    )?;
    for fragment in fragments {
        if fragment.is_empty() {
            return Err(fragments_error("empty media fragment"));
        }
        if fragment.len() as i64 > limits.max_bytes - info.size {
            return Err(too_large(limits));
        }
        let (n, err) = write_once(dst, fragment);
        info.size += n as i64;
        info.fragment_count += 1;
        if let Some(err) = err {
            return Err(Error::other(format!("dicom: write video fragment: {err}")));
        }
        if n != fragment.len() {
            return Err(Error::other("short write"));
        }
    }
    Ok(())
}
fn too_large(limits: Limits) -> Error {
    Error::new(
        Kind::MediaTooLarge,
        format!("limit {} bytes", limits.max_bytes),
    )
}
fn copy_deferred(
    dataset: &Object,
    dst: &mut dyn Write,
    limits: Limits,
    info: &mut Info,
) -> Result<(), Error> {
    let mut encoded = Vec::new();
    let (_, provider_err) = dataset.copy_value_to(Tag::PIXEL_DATA, &mut encoded);
    let provider_err = provider_err.map(|err| err.to_string());
    let mut source = Replay {
        data: &encoded,
        at: 0,
        failure: provider_err.as_deref(),
    };
    copy_encoded_fragments(&mut source, dst, limits, info)?;
    let unread = source.at < encoded.len();
    let pending =
        provider_err.or_else(|| unread.then(|| "io: read/write on closed pipe".to_string()));
    match pending {
        Some(err) => Err(Error::other(format!(
            "dicom: replay deferred video Pixel Data: {err}"
        ))),
        None => Ok(()),
    }
}
struct Replay<'a> {
    data: &'a [u8],
    at: usize,
    failure: Option<&'a str>,
}
impl<'a> Replay<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let end = self.at.saturating_add(n).min(self.data.len());
        let out = &self.data[self.at..end];
        self.at = end;
        out
    }
    fn short(&self, truncated: &str) -> Error {
        match self.failure {
            Some(err) => Error::other(err),
            None => fragments_error(truncated),
        }
    }
}
fn copy_encoded_fragments(
    src: &mut Replay<'_>,
    dst: &mut dyn Write,
    limits: Limits,
    info: &mut Info,
) -> Result<(), Error> {
    let mut item_index = 0;
    loop {
        let header = src.take(8);
        if header.len() < 8 {
            return Err(src.short("truncated fragment header"));
        }
        let group = u16::from_le_bytes([header[0], header[1]]);
        let element = u16::from_le_bytes([header[2], header[3]]);
        let length = i64::from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]));
        if group == 0xfffe && element == 0xe0dd {
            if length != 0 {
                return Err(fragments_error("non-zero sequence delimiter length"));
            }
            return validate_fragment_count(
                &info.transfer_syntax_uid,
                info.fragment_count,
                limits.max_fragments,
            );
        }
        if group != 0xfffe || element != 0xe000 {
            return Err(fragments_error(format!(
                "unexpected item tag ({group:04X},{element:04X})"
            )));
        }
        if item_index == 0 {
            if length != 0 {
                return Err(fragments_error(
                    "Basic Offset Table must be empty for video",
                ));
            }
            item_index += 1;
            continue;
        }
        if length == 0 {
            return Err(fragments_error("empty media fragment"));
        }
        if info.fragment_count >= limits.max_fragments {
            return Err(Error::new(
                Kind::TooManyFragments,
                format!("limit {}", limits.max_fragments),
            ));
        }
        if length > limits.max_bytes - info.size {
            return Err(too_large(limits));
        }
        let payload = src.take(length as usize);
        let (n, err) = write_once(dst, payload);
        info.size += n as i64;
        info.fragment_count += 1;
        if let Some(err) = err {
            return Err(Error::other(err.to_string()));
        }
        if n != payload.len() {
            return Err(Error::other("short write"));
        }
        if (payload.len() as i64) < length {
            return Err(src.short("truncated media fragment"));
        }
        item_index += 1;
    }
}
fn validate_fragment_count(uid: &str, count: i64, max: i64) -> Result<(), Error> {
    if count <= 0 {
        return Err(fragments_error("no media fragments"));
    }
    if count > max {
        return Err(Error::new(
            Kind::TooManyFragments,
            format!("got {count}, limit {max}"),
        ));
    }
    if !is_fragmentable(uid) && count != 1 {
        return Err(fragments_error(format!(
            "non-fragmentable transfer syntax {} has {count} fragments",
            quote(uid)
        )));
    }
    Ok(())
}
fn is_fragmentable(uid: &str) -> bool {
    matches!(
        transfer::normalize_uid(uid),
        "1.2.840.10008.1.2.4.100.1"
            | "1.2.840.10008.1.2.4.101.1"
            | "1.2.840.10008.1.2.4.102.1"
            | "1.2.840.10008.1.2.4.103.1"
            | "1.2.840.10008.1.2.4.104.1"
            | "1.2.840.10008.1.2.4.105.1"
            | "1.2.840.10008.1.2.4.106.1"
    )
}
fn codec_for_syntax(uid: &str) -> &'static str {
    match transfer::normalize_uid(uid) {
        "1.2.840.10008.1.2.4.100"
        | "1.2.840.10008.1.2.4.100.1"
        | "1.2.840.10008.1.2.4.101"
        | "1.2.840.10008.1.2.4.101.1" => CODEC_MPEG2,
        "1.2.840.10008.1.2.4.107" | "1.2.840.10008.1.2.4.108" => CODEC_HEVC,
        _ => CODEC_H264,
    }
}
fn detect_container(
    codec: &'static str,
    data: &[u8],
) -> Result<(&'static str, &'static str, &'static str), Error> {
    let start_code = data.len() >= 4 && data[..3] == [0, 0, 1];
    let container = if data.len() >= 12 && &data[4..8] == b"ftyp" {
        super::CONTAINER_MP4
    } else if is_mpeg_transport_stream(data) {
        CONTAINER_MPEG_TRANSPORT
    } else if start_code && (data[3] == 0xba || (0xbd..=0xef).contains(&data[3])) {
        super::CONTAINER_MPEG_PROGRAM
    } else if start_code && data[3] == 0xb3 {
        CONTAINER_MPEG_ELEMENTARY
    } else {
        return Err(Error::new(
            Kind::UnsupportedContainer,
            format!("unrecognized {codec} stream signature"),
        ));
    };
    if codec != CODEC_MPEG2
        && container != super::CONTAINER_MP4
        && container != CONTAINER_MPEG_TRANSPORT
    {
        return Err(Error::new(
            Kind::UnsupportedContainer,
            format!("{codec} requires MP4 or MPEG-TS, got {container}"),
        ));
    }
    Ok(match container {
        super::CONTAINER_MP4 => (container, ".mp4", "video/mp4"),
        CONTAINER_MPEG_TRANSPORT => (container, ".ts", "video/mp2t"),
        CONTAINER_MPEG_ELEMENTARY => (container, ".m2v", "video/mpeg"),
        _ => (container, ".mpg", "video/mpeg"),
    })
}
fn is_mpeg_transport_stream(data: &[u8]) -> bool {
    data.len() >= 188
        && data[0] == 0x47
        && data.iter().step_by(188).take(4).all(|&sync| sync == 0x47)
}
struct PrefixWriter<'a> {
    dst: &'a mut dyn Write,
    prefix: Vec<u8>,
}
impl Write for PrefixWriter<'_> {
    fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
        let room = PROBE_BYTES - self.prefix.len();
        self.prefix.extend_from_slice(&p[..p.len().min(room)]);
        self.dst.write(p)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.dst.flush()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn containers() {
        let mut ts = vec![0u8; 188 * 3];
        ts[0] = 0x47;
        ts[188] = 0x47;
        assert!(!is_mpeg_transport_stream(&ts));
        ts[376] = 0x47;
        assert!(is_mpeg_transport_stream(&ts));
        assert_eq!(detect_container(CODEC_H264, &ts).map(|c| c.1), Ok(".ts"));
        assert_eq!(
            detect_container(CODEC_MPEG2, &[0, 0, 1, 0xb3]).map(|c| c.1),
            Ok(".m2v")
        );
        assert!(detect_container(CODEC_HEVC, &[0, 0, 1, 0xba]).is_err());
        assert!(is_fragmentable("1.2.840.10008.1.2.4.102.1"));
        assert!(!is_fragmentable("1.2.840.10008.1.2.4.107"));
    }
}
