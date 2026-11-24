#![forbid(unsafe_code)]
use std::borrow::Cow;
use std::fmt;
mod audio;
mod index;
mod layout;
mod parse;
pub use audio::{MAX_AUDIO_RANGE_FRAMES, is_audio_storage_sop_class};
pub use layout::{
    AmplitudeScale, GridLine, GridLineKind, GridText, GridTextKind, LayoutOptions, RasterLayout,
    RasterLine, RasterPoint, TraceLayout, amplitude_scale_from_envelope,
    annotation_applies_to_channels, build_raster_layout, channel_unit, cursor_bounds, cursor_step,
    group_display_bounds, group_sample_bounds, raster_cursor_line,
};
pub use parse::{
    is_storage_sop_class, is_waveform_sop_class, open, open_with_source_factory,
    supported_storage_sop_class_uids,
};
pub type Duration = i64;
pub const SECOND: Duration = 1_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    NotWaveform,
    Unsupported,
    Closed,
    AudioRange,
    Other,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawFallback {
    pub sop_class_uid: String,
    pub group_index: i64,
    pub encoding: String,
    pub raw_bytes: i64,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    text: String,
    pub fallback: Option<RawFallback>,
}
impl Error {
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: ErrorKind::Other,
            text: text.into(),
            fallback: None,
        }
    }
    pub(crate) fn not_waveform(detail: &str) -> Error {
        Error {
            kind: ErrorKind::NotWaveform,
            text: format!("waveform: not a waveform storage SOP class: {detail}"),
            fallback: None,
        }
    }
    pub(crate) fn closed() -> Error {
        Error {
            kind: ErrorKind::Closed,
            text: "waveform: recording is closed".to_string(),
            fallback: None,
        }
    }
    pub(crate) fn audio_range() -> Error {
        Error {
            kind: ErrorKind::AudioRange,
            text: "waveform: invalid audio PCM range".to_string(),
            fallback: None,
        }
    }
    pub(crate) fn unsupported(fallback: RawFallback) -> Error {
        let text = if fallback.reason.is_empty() {
            "waveform: unsupported waveform".to_string()
        } else {
            format!("waveform: unsupported waveform: {}", fallback.reason)
        };
        Error {
            kind: ErrorKind::Unsupported,
            text,
            fallback: Some(fallback),
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.text = format!("{prefix}: {}", self.text);
        self
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub max_index_entries: i64,
    pub max_envelope_width: i64,
    pub max_groups: i64,
    pub max_channels_per_group: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CalibrationStatus {
    #[default]
    MissingSensitivity,
    MissingUnits,
    MissingParameters,
    Complete,
}
impl CalibrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CalibrationStatus::MissingSensitivity => "missing sensitivity",
            CalibrationStatus::MissingUnits => "missing units",
            CalibrationStatus::MissingParameters => "missing correction factor or baseline",
            CalibrationStatus::Complete => "complete",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodedConcept {
    pub value: String,
    pub scheme: String,
    pub version: String,
    pub meaning: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Calibration {
    pub status: CalibrationStatus,
    pub units: CodedConcept,
    pub sensitivity: f64,
    pub correction_factor: f64,
    pub baseline: f64,
    pub has_sensitivity: bool,
    pub has_correction_factor: bool,
    pub has_baseline: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelInfo {
    pub index: i64,
    pub number: i64,
    pub label: String,
    pub status: Vec<String>,
    pub source: CodedConcept,
    pub calibration: Calibration,
    pub time_skew: Duration,
    pub sample_skew: f64,
    pub channel_offset: Duration,
    pub bits_stored: i64,
    pub low_frequency: f64,
    pub high_frequency: f64,
    pub notch_frequency: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupInfo {
    pub index: i64,
    pub label: String,
    pub uid: String,
    pub originality: String,
    pub time_offset: Duration,
    pub sampling_frequency_hz: f64,
    pub sample_count: i64,
    pub duration: Duration,
    pub bits_allocated: i64,
    pub sample_interpretation: String,
    pub channels: Vec<ChannelInfo>,
    pub supported: bool,
    pub fallback_reason: String,
    pub raw_data_bytes: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelReference {
    pub group_number: i64,
    pub channel_number: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotation {
    pub annotation_group_number: i64,
    pub temporal_range_type: String,
    pub channels: Vec<ChannelReference>,
    pub referenced_sample_positions: Vec<u64>,
    pub referenced_time_offsets: Vec<Duration>,
    pub referenced_date_times: Vec<String>,
    pub concept_name: CodedConcept,
    pub concept_value: CodedConcept,
    pub numeric_values: Vec<f64>,
    pub numeric_units: CodedConcept,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub sample_index: i64,
    pub time: Duration,
    pub raw: f64,
    pub value: f64,
    pub valid: bool,
    pub calibrated: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvelopeBucket {
    pub start_sample: i64,
    pub end_sample: i64,
    pub min: Sample,
    pub max: Sample,
    pub valid: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelEnvelope {
    pub channel: ChannelInfo,
    pub buckets: Vec<EnvelopeBucket>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct NormalizedOptions {
    pub max_index_entries: i64,
    pub max_width: i64,
    pub max_groups: i64,
    pub max_channels: i64,
}
pub trait SampleSource: Send + Sync {
    fn read_at(&self, buf: &mut [u8], offset: i64) -> std::io::Result<usize>;
    fn size(&self) -> i64;
    fn close(&self) -> Result<(), crate::BoxError> {
        Ok(())
    }
}
pub type SourceFactory<'a> =
    dyn Fn(i64, &crate::object::Element) -> Result<Box<dyn SampleSource>, crate::BoxError> + 'a;
pub(crate) enum Source {
    Bytes(Vec<u8>),
    External(Box<dyn SampleSource>),
}
pub(crate) type ReadFailure = Option<String>;
impl Source {
    pub(crate) fn size(&self) -> i64 {
        match self {
            Source::Bytes(b) => b.len() as i64,
            Source::External(s) => s.size(),
        }
    }
    pub(crate) fn read_at(&self, offset: i64, len: usize) -> Result<Cow<'_, [u8]>, ReadFailure> {
        match self {
            Source::Bytes(b) => {
                if offset < 0 {
                    return Err(Some("read at a negative offset".into()));
                }
                let at = offset as usize;
                match at.checked_add(len).and_then(|end| b.get(at..end)) {
                    Some(bytes) => Ok(Cow::Borrowed(bytes)),
                    None => Err(Some("EOF".into())),
                }
            }
            Source::External(s) => {
                let mut buf = vec![0u8; len];
                match s.read_at(&mut buf, offset) {
                    Ok(n) if n == len => Ok(Cow::Owned(buf)),
                    Ok(_) => Err(None),
                    Err(e) => Err(Some(e.to_string())),
                }
            }
        }
    }
    pub(crate) fn close(&mut self) -> Result<(), String> {
        match self {
            Source::Bytes(_) => Ok(()),
            Source::External(s) => s.close().map_err(|e| e.to_string()),
        }
    }
}
pub(crate) struct Group {
    pub info: GroupInfo,
    pub source: Option<Source>,
    pub decoder: parse::SampleDecoder,
    pub padding_bits: Vec<u64>,
    pub has_padding: bool,
    pub index: Option<index::MultiIndex>,
    pub index_err: Option<Error>,
    pub index_budget: i64,
}
pub struct Recording {
    pub(crate) closed: bool,
    pub(crate) sop_class_uid: String,
    pub(crate) groups: Vec<Group>,
    pub(crate) annotations: Vec<Annotation>,
    pub(crate) options: NormalizedOptions,
}
impl Recording {
    pub fn sop_class_uid(&self) -> &str {
        &self.sop_class_uid
    }
    pub fn groups(&self) -> Vec<GroupInfo> {
        self.groups.iter().map(|g| g.info.clone()).collect()
    }
    pub fn annotations(&self) -> Vec<Annotation> {
        self.annotations.clone()
    }
    pub fn build_index(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::closed());
        }
        for group in &mut self.groups {
            if group.info.supported {
                group.ensure_index()?;
            }
        }
        Ok(())
    }
    pub fn close(&mut self) {
        let _ = self.try_close();
    }
    pub fn try_close(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut errs = Vec::new();
        for group in &mut self.groups {
            if let Some(mut source) = group.source.take()
                && let Err(e) = source.close()
            {
                errs.push(e);
            }
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(Error::other(errs.join("\n")))
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(mut source) = self.source.take() {
            let _ = source.close();
        }
    }
}
