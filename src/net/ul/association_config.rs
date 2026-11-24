#![forbid(unsafe_code)]
use super::called_ae::AssociationProfile;
use super::conn::Ctx;
use super::error::Kind;
use super::negotiation::{
    EXTENDED_NEGOTIATION_ACTION_ACCEPT, EXTENDED_NEGOTIATION_ACTION_IGNORE,
    ExtendedNegotiationHandler, ExtendedNegotiationResult,
};
use super::pdu::{
    MAX_PRESENTATION_CONTEXTS, PresentationContext, RoleSelectionItem,
    SopClassExtendedNegotiationItem,
};
use super::rules::{PresentationContextRule, RequestorRoles};
use crate::dictionary::uid as uiddict;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Read;
use std::sync::Arc;
const TEXT_INVALID: &str = "dicom ul: invalid association configuration";
const TEXT_LIMIT: &str = "dicom ul: association configuration resource limit";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssociationConfigError {
    Invalid,
    InvalidLine(usize),
    Limit,
    Io(std::io::ErrorKind),
    Context(Kind),
}
impl AssociationConfigError {
    pub fn is_invalid(&self) -> bool {
        matches!(
            self,
            AssociationConfigError::Invalid
                | AssociationConfigError::InvalidLine(_)
                | AssociationConfigError::Io(_)
        )
    }
    pub fn is_limit(&self) -> bool {
        *self == AssociationConfigError::Limit
    }
}
impl fmt::Display for AssociationConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssociationConfigError::Invalid => f.write_str(TEXT_INVALID),
            AssociationConfigError::InvalidLine(n) => write!(f, "{TEXT_INVALID}: line {n}"),
            AssociationConfigError::Limit => f.write_str(TEXT_LIMIT),
            AssociationConfigError::Io(_) => write!(f, "{TEXT_INVALID}: I/O failure"),
            AssociationConfigError::Context(Kind::Canceled) => f.write_str("context canceled"),
            AssociationConfigError::Context(_) => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for AssociationConfigError {}
type Result<T> = std::result::Result<T, AssociationConfigError>;
use AssociationConfigError as E;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationConfigLimits {
    pub max_bytes: i64,
    pub max_line_bytes: i64,
    pub max_value_bytes: i64,
    pub max_name_bytes: i64,
    pub max_sections: i64,
    pub max_entries: i64,
    pub max_profiles: i64,
    pub max_context_rules: i64,
    pub max_transfer_syntaxes: i64,
    pub max_role_items: i64,
    pub max_extended_items: i64,
    pub max_extended_item_bytes: i64,
    pub max_extended_bytes: i64,
    pub max_compiled_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalByteOrder {
    LittleEndian,
    BigEndian,
    Other,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationConfigOptions {
    pub limits: AssociationConfigLimits,
    pub local_byte_order: Option<LocalByteOrder>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssociationConfigMode {
    Requestor,
    Acceptor,
}
#[derive(Debug, Clone)]
struct ConfigContext {
    sop: String,
    syntax_list: String,
}
#[derive(Debug, Clone, Default)]
struct ConfigProfile {
    contexts: String,
    roles: String,
    extended: String,
}
#[derive(Debug, Default)]
struct Section {
    entries: HashMap<String, String>,
}
#[derive(Debug, Clone)]
pub struct AssociationConfig {
    limits: AssociationConfigLimits,
    syntaxes: HashMap<String, Vec<String>>,
    contexts: HashMap<String, Vec<ConfigContext>>,
    roles: HashMap<String, Vec<RoleSelectionItem>>,
    extended: HashMap<String, Vec<SopClassExtendedNegotiationItem>>,
    profiles: HashMap<String, ConfigProfile>,
}
impl AssociationProfile {
    pub fn extended_negotiation_handler(&self) -> ExtendedNegotiationHandler {
        let items: HashMap<String, Vec<u8>> = self
            .extended_negotiation
            .iter()
            .map(|i| (i.sop_class_uid.clone(), i.data.clone()))
            .collect();
        Arc::new(move |request: &SopClassExtendedNegotiationItem| {
            Ok(match items.get(&request.sop_class_uid) {
                None => ExtendedNegotiationResult {
                    action: EXTENDED_NEGOTIATION_ACTION_IGNORE,
                    data: Vec::new(),
                },
                Some(data) => ExtendedNegotiationResult {
                    action: EXTENDED_NEGOTIATION_ACTION_ACCEPT,
                    data: data.clone(),
                },
            })
        })
    }
}
const MAX_INT: i64 = i64::MAX;
fn normalized(mut o: AssociationConfigOptions) -> Result<AssociationConfigOptions> {
    let l = &mut o.limits;
    if l.max_bytes < 0 || l.max_bytes == i64::MAX {
        return Err(E::Limit);
    }
    if l.max_bytes == 0 {
        l.max_bytes = 1 << 20;
    }
    let pairs: [(&mut i64, i64); 13] = [
        (&mut l.max_line_bytes, 8192),
        (&mut l.max_value_bytes, 8192),
        (&mut l.max_name_bytes, 128),
        (&mut l.max_sections, 1024),
        (&mut l.max_entries, 16384),
        (&mut l.max_profiles, 256),
        (&mut l.max_context_rules, 1024),
        (&mut l.max_transfer_syntaxes, 256),
        (&mut l.max_role_items, 1024),
        (&mut l.max_extended_items, 1024),
        (&mut l.max_extended_item_bytes, 1024),
        (&mut l.max_extended_bytes, 64 << 10),
        (&mut l.max_compiled_bytes, 8 << 20),
    ];
    for (value, default) in pairs {
        if *value < 0 || *value > MAX_INT - 2 {
            return Err(E::Limit);
        }
        if *value == 0 {
            *value = default;
        }
    }
    if l.max_extended_item_bytes > i64::from(u16::MAX) - 66 {
        return Err(E::Limit);
    }
    if o.local_byte_order == Some(LocalByteOrder::Other) {
        return Err(E::Invalid);
    }
    Ok(o)
}
fn config_name(value: &str, limit: i64) -> Result<String> {
    if value.len() as i64 > limit {
        return Err(E::Limit);
    }
    let mut out = String::new();
    for mut c in value.bytes() {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
            continue;
        }
        c = c.to_ascii_uppercase();
        if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_' || c == b'-') {
            return Err(E::Invalid);
        }
        out.push(c as char);
    }
    if out.is_empty() {
        return Err(E::Invalid);
    }
    Ok(out)
}
fn ctx_err(ctx: Option<&Ctx>) -> Result<()> {
    match ctx.and_then(Ctx::err) {
        Some(k) => Err(E::Context(k)),
        None => Ok(()),
    }
}
struct Source<'a> {
    reader: &'a mut dyn Read,
    ctx: Option<&'a Ctx>,
    left: i64,
}
enum ReadEnd {
    Eof,
    Ctx(Kind),
    Io(std::io::ErrorKind),
    TooLong,
    NoProgress,
}
impl Source<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadEnd>) {
        if self.left <= 0 {
            return (0, Some(ReadEnd::Eof));
        }
        let take = buf.len().min(self.left as usize);
        if let Some(k) = self.ctx.and_then(Ctx::err) {
            return (0, Some(ReadEnd::Ctx(k)));
        }
        loop {
            match self.reader.read(&mut buf[..take]) {
                Ok(0) if take > 0 => return (0, Some(ReadEnd::Eof)),
                Ok(n) => {
                    self.left -= n as i64;
                    return (n, None);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return (0, Some(ReadEnd::Io(e.kind()))),
            }
        }
    }
}
struct Scanner<'a> {
    src: Source<'a>,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    max: usize,
    err: Option<ReadEnd>,
    done: bool,
}
fn split_line(data: &[u8], at_eof: bool) -> Option<(usize, Vec<u8>)> {
    for (i, &c) in data.iter().enumerate() {
        if c == b'\n' {
            return Some((i + 1, data[..i].to_vec()));
        }
        if c == b'\r' {
            if i + 1 == data.len() && !at_eof {
                return None;
            }
            let mut advance = i + 1;
            if advance < data.len() && data[advance] == b'\n' {
                advance += 1;
            }
            return Some((advance, data[..i].to_vec()));
        }
    }
    if at_eof && !data.is_empty() {
        return Some((data.len(), data.to_vec()));
    }
    None
}
impl Scanner<'_> {
    fn scan(&mut self) -> Option<Vec<u8>> {
        if self.done {
            return None;
        }
        loop {
            if (self.end > self.start || self.err.is_some())
                && let Some((advance, token)) =
                    split_line(&self.buf[self.start..self.end], self.err.is_some())
            {
                self.start += advance;
                return Some(token);
            }
            if self.err.is_some() {
                self.start = 0;
                self.end = 0;
                self.done = true;
                return None;
            }
            if self.start > 0 && (self.end == self.buf.len() || self.start > self.buf.len() / 2) {
                self.buf.copy_within(self.start..self.end, 0);
                self.end -= self.start;
                self.start = 0;
            }
            if self.end == self.buf.len() {
                if self.buf.len() >= self.max {
                    self.err = Some(ReadEnd::TooLong);
                    self.done = true;
                    return None;
                }
                let size = if self.buf.is_empty() {
                    4096
                } else {
                    self.buf.len() * 2
                }
                .min(self.max);
                let mut grown = vec![0u8; size];
                grown[..self.end - self.start].copy_from_slice(&self.buf[self.start..self.end]);
                self.buf = grown;
                self.end -= self.start;
                self.start = 0;
            }
            let mut empty = 0;
            loop {
                let (n, end) = self.src.read(&mut self.buf[self.end..]);
                self.end += n;
                if let Some(e) = end {
                    self.err = Some(e);
                    break;
                }
                if n > 0 {
                    break;
                }
                empty += 1;
                if empty > 100 {
                    self.err = Some(ReadEnd::NoProgress);
                    break;
                }
            }
        }
    }
}
const KNOWN_GROUPS: [&str; 5] = [
    "TRANSFERSYNTAXES",
    "PRESENTATIONCONTEXTS",
    "PROFILES",
    "SCPSCUROLESELECTION",
    "EXTENDEDNEGOTIATION",
];
type Groups = HashMap<String, Vec<(String, Section)>>;
fn find<'a>(sections: &'a [(String, Section)], name: &str) -> Option<&'a Section> {
    sections.iter().find(|(n, _)| n == name).map(|(_, s)| s)
}
pub fn parse_association_config(
    ctx: Option<&Ctx>,
    reader: &mut dyn Read,
    options: AssociationConfigOptions,
) -> Result<AssociationConfig> {
    ctx_err(ctx)?;
    let options = normalized(options)?;
    let l = options.limits;
    let max = (l.max_line_bytes + 2) as usize;
    let mut scanner = Scanner {
        src: Source {
            reader,
            ctx,
            left: l.max_bytes + 1,
        },
        buf: vec![0u8; 1024.min(max)],
        start: 0,
        end: 0,
        max,
        err: None,
        done: false,
    };
    let mut groups: Groups = HashMap::new();
    let mut group = String::new();
    let mut section: Option<(String, String)> = None;
    let mut previous_key = String::new();
    let (mut section_count, mut entry_count, mut line_number) = (0i64, 0i64, 0usize);
    while let Some(raw) = scanner.scan() {
        line_number += 1;
        ctx_err(ctx)?;
        if scanner.src.left == 0 {
            return Err(E::Limit);
        }
        if raw.len() as i64 > l.max_line_bytes {
            return Err(E::Limit);
        }
        if raw
            .iter()
            .any(|&c| c == 0 || (c < 32 && c != b'\t') || c == 127 || c >= 128)
        {
            return Err(E::InvalidLine(line_number));
        }
        let line = String::from_utf8(raw).unwrap_or_default();
        let trimmed = line.trim_matches([' ', '\t']);
        if line.is_empty() || line.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') {
            previous_key.clear();
            if let Some(inner) = trimmed.strip_prefix("[[") {
                let Some(inner) = inner.strip_suffix("]]") else {
                    return Err(E::Invalid);
                };
                group = config_name(inner, l.max_name_bytes)?;
                if !KNOWN_GROUPS.contains(&group.as_str()) || groups.contains_key(&group) {
                    return Err(E::Invalid);
                }
                groups.insert(group.clone(), Vec::new());
                section = None;
            } else {
                if !trimmed.ends_with(']') || !groups.contains_key(&group) {
                    return Err(E::Invalid);
                }
                let name = config_name(&trimmed[1..trimmed.len() - 1], l.max_name_bytes)?;
                let sections = groups.entry(group.clone()).or_default();
                if find(sections, &name).is_some() {
                    return Err(E::Invalid);
                }
                if section_count >= l.max_sections
                    || (group == "PROFILES" && sections.len() as i64 >= l.max_profiles)
                {
                    return Err(E::Limit);
                }
                section_count += 1;
                sections.push((name.clone(), Section::default()));
                section = Some((group.clone(), name));
            }
            continue;
        }
        let Some((g, name)) = &section else {
            return Err(E::Invalid);
        };
        let Some(current) = groups
            .get_mut(g)
            .and_then(|s| s.iter_mut().find(|(n, _)| n == name))
            .map(|(_, s)| s)
        else {
            return Err(E::Invalid);
        };
        if (line.starts_with(' ') || line.starts_with('\t')) && !previous_key.is_empty() {
            let value = current
                .entries
                .get(&previous_key)
                .cloned()
                .unwrap_or_default();
            if value.len() as i64 >= l.max_value_bytes
                || trimmed.len() as i64 > l.max_value_bytes - value.len() as i64 - 1
            {
                return Err(E::Limit);
            }
            current
                .entries
                .insert(previous_key.clone(), format!("{value}\n{trimmed}"));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(E::Invalid);
        };
        let key = config_name(key, l.max_name_bytes)?;
        let value = value.trim_matches([' ', '\t']);
        if value.len() as i64 > l.max_value_bytes {
            return Err(E::Limit);
        }
        if current.entries.contains_key(&key) {
            return Err(E::Invalid);
        }
        if entry_count >= l.max_entries {
            return Err(E::Limit);
        }
        entry_count += 1;
        current.entries.insert(key.clone(), value.to_string());
        previous_key = key;
    }
    match scanner.err {
        Some(ReadEnd::Ctx(k)) => return Err(E::Context(k)),
        Some(ReadEnd::TooLong) => return Err(E::Limit),
        Some(ReadEnd::Io(kind)) => return Err(E::Io(kind)),
        Some(ReadEnd::NoProgress) => return Err(E::Io(std::io::ErrorKind::Other)),
        _ => {}
    }
    if scanner.src.left == 0 {
        return Err(E::Limit);
    }
    ctx_err(ctx)?;
    compile(ctx, &groups, &options)
}
pub fn read_association_config_file(
    ctx: Option<&Ctx>,
    path: impl AsRef<std::path::Path>,
    options: AssociationConfigOptions,
) -> Result<AssociationConfig> {
    ctx_err(ctx)?;
    let mut file = std::fs::File::open(path).map_err(|e| E::Io(e.kind()))?;
    parse_association_config(ctx, &mut file, options)
}
fn numbered(section: &Section, prefix: &str, maximum: i64) -> Result<Vec<String>> {
    if section.entries.len() as i64 > maximum {
        return Err(E::Limit);
    }
    (1..=section.entries.len())
        .map(|i| {
            section
                .entries
                .get(&format!("{prefix}{i}"))
                .cloned()
                .ok_or(E::Invalid)
        })
        .collect()
}
fn pair(value: &str) -> Result<(String, String)> {
    let Some((a, b)) = value.split_once('\\') else {
        return Err(E::Invalid);
    };
    let (a, b) = (a.trim(), b.trim());
    if b.contains('\\') || a.is_empty() || b.is_empty() {
        return Err(E::Invalid);
    }
    Ok((a.to_string(), b.to_string()))
}
const ALIASES: &[(&str, &str)] = &[
    ("BigEndianExplicit", "1.2.840.10008.1.2.2"),
    ("CTImageStorageForProcessing", "1.2.840.10008.5.1.4.1.1.2.3"),
    (
        "DICONDE_EddyCurrentImageStorage",
        "1.2.840.10008.5.1.4.1.1.601.1",
    ),
    (
        "DICONDE_EddyCurrentMultiframeImageStorage",
        "1.2.840.10008.5.1.4.1.1.601.2",
    ),
    (
        "DICONDE_ThermographyImageStorage",
        "1.2.840.10008.5.1.4.1.1.601.3",
    ),
    (
        "DICONDE_ThermographyMultiFrameImageStorage",
        "1.2.840.10008.5.1.4.1.1.601.4",
    ),
    (
        "DICONDE_UltrasoundWaveformStorage",
        "1.2.840.10008.5.1.4.1.1.601.5",
    ),
    ("DICOS_2DAITStorage", "1.2.840.10008.5.1.4.1.1.501.4"),
    ("DICOS_3DAITStorage", "1.2.840.10008.5.1.4.1.1.501.5"),
    ("DICOS_CTImageStorage", "1.2.840.10008.5.1.4.1.1.501.1"),
    (
        "DICOS_DigitalXRayImageStorageForPresentation",
        "1.2.840.10008.5.1.4.1.1.501.2.1",
    ),
    (
        "DICOS_DigitalXRayImageStorageForProcessing",
        "1.2.840.10008.5.1.4.1.1.501.2.2",
    ),
    (
        "DICOS_QuadrupoleResonanceStorage",
        "1.2.840.10008.5.1.4.1.1.501.6",
    ),
    (
        "DICOS_ThreatDetectionReportStorage",
        "1.2.840.10008.5.1.4.1.1.501.3",
    ),
    (
        "DRAFT_RTBeamsDeliveryInstructionStorage",
        "1.2.840.10008.5.1.4.34.1",
    ),
    ("DRAFT_SRAudioStorage", "1.2.840.10008.5.1.4.1.1.88.2"),
    (
        "DRAFT_SRComprehensiveStorage",
        "1.2.840.10008.5.1.4.1.1.88.4",
    ),
    ("DRAFT_SRDetailStorage", "1.2.840.10008.5.1.4.1.1.88.3"),
    ("DRAFT_SRTextStorage", "1.2.840.10008.5.1.4.1.1.88.1"),
    ("DRAFT_WaveformStorage", "1.2.840.10008.5.1.4.1.1.9.1"),
    ("DeflatedLittleEndianExplicit", "1.2.840.10008.1.2.1.99"),
    (
        "EncapsulatedUncompressedLittleEndianExplicit",
        "1.2.840.10008.1.2.1.98",
    ),
    (
        "EnhancedCTImageStorageForProcessing",
        "1.2.840.10008.5.1.4.1.1.2.4",
    ),
    (
        "FragmentableMPEG2MainProfile/HighLevel",
        "1.2.840.10008.1.2.4.101.1",
    ),
    (
        "FragmentableMPEG2MainProfile/MainLevel",
        "1.2.840.10008.1.2.4.100.1",
    ),
    (
        "FragmentableMPEG4BDcompatibleHighProfile/Level4.1",
        "1.2.840.10008.1.2.4.103.1",
    ),
    (
        "FragmentableMPEG4HighProfile/Level4.1",
        "1.2.840.10008.1.2.4.102.1",
    ),
    (
        "FragmentableMPEG4HighProfile/Level4.2For2DVideo",
        "1.2.840.10008.1.2.4.104.1",
    ),
    (
        "FragmentableMPEG4HighProfile/Level4.2For3DVideo",
        "1.2.840.10008.1.2.4.105.1",
    ),
    (
        "FragmentableMPEG4StereoHighProfile/Level4.2",
        "1.2.840.10008.1.2.4.106.1",
    ),
    (
        "General32BitECGWaveformStorage",
        "1.2.840.10008.5.1.4.1.1.9.1.4",
    ),
    ("HEVCMain10Profile/Level5.1", "1.2.840.10008.1.2.4.108"),
    ("HEVCMainProfile/Level5.1", "1.2.840.10008.1.2.4.107"),
    (
        "HighThroughputJPEG2000ImageCompression",
        "1.2.840.10008.1.2.4.203",
    ),
    (
        "HighThroughputJPEG2000ImageCompressionLosslessOnly",
        "1.2.840.10008.1.2.4.201",
    ),
    (
        "HighThroughputJPEG2000withRPCLOptionsImageCompressionLosslessOnly",
        "1.2.840.10008.1.2.4.202",
    ),
    ("JPEG2000LosslessOnly", "1.2.840.10008.1.2.4.90"),
    ("JPEG2000Multicomponent", "1.2.840.10008.1.2.4.93"),
    (
        "JPEG2000MulticomponentLosslessOnly",
        "1.2.840.10008.1.2.4.92",
    ),
    ("JPEGBaseline", "1.2.840.10008.1.2.4.50"),
    (
        "JPEGExtended:Hierarchical:Process16+18",
        "1.2.840.10008.1.2.4.59",
    ),
    (
        "JPEGExtended:Hierarchical:Process17+19",
        "1.2.840.10008.1.2.4.60",
    ),
    ("JPEGExtended:Process2+4", "1.2.840.10008.1.2.4.51"),
    ("JPEGExtended:Process3+5", "1.2.840.10008.1.2.4.52"),
    (
        "JPEGFullProgression:Hierarchical:Process24+26",
        "1.2.840.10008.1.2.4.63",
    ),
    (
        "JPEGFullProgression:Hierarchical:Process25+27",
        "1.2.840.10008.1.2.4.64",
    ),
    (
        "JPEGFullProgression:Non-hierarchical:Process10+12",
        "1.2.840.10008.1.2.4.55",
    ),
    (
        "JPEGFullProgression:Non-hierarchical:Process11+13",
        "1.2.840.10008.1.2.4.56",
    ),
    ("JPEGLSLossy", "1.2.840.10008.1.2.4.81"),
    (
        "JPEGLossless:Hierarchical:Process28",
        "1.2.840.10008.1.2.4.65",
    ),
    (
        "JPEGLossless:Hierarchical:Process29",
        "1.2.840.10008.1.2.4.66",
    ),
    (
        "JPEGLossless:Non-hierarchical-1stOrderPrediction",
        "1.2.840.10008.1.2.4.70",
    ),
    (
        "JPEGLossless:Non-hierarchical:Process14",
        "1.2.840.10008.1.2.4.57",
    ),
    (
        "JPEGLossless:Non-hierarchical:Process15",
        "1.2.840.10008.1.2.4.58",
    ),
    (
        "JPEGSpectralSelection:Hierarchical:Process20+22",
        "1.2.840.10008.1.2.4.61",
    ),
    (
        "JPEGSpectralSelection:Hierarchical:Process21+23",
        "1.2.840.10008.1.2.4.62",
    ),
    (
        "JPEGSpectralSelection:Non-hierarchical:Process6+8",
        "1.2.840.10008.1.2.4.53",
    ),
    (
        "JPEGSpectralSelection:Non-hierarchical:Process7+9",
        "1.2.840.10008.1.2.4.54",
    ),
    (
        "LegacyConvertedEnhancedCTImageStorageForProcessing",
        "1.2.840.10008.5.1.4.1.1.2.5",
    ),
    ("LittleEndianExplicit", "1.2.840.10008.1.2.1"),
    ("LittleEndianImplicit", "1.2.840.10008.1.2"),
    ("MPEG2MainProfile@HighLevel", "1.2.840.10008.1.2.4.101"),
    ("MPEG2MainProfile@MainLevel", "1.2.840.10008.1.2.4.100"),
    (
        "MPEG4BDcompatibleHighProfile/Level4.1",
        "1.2.840.10008.1.2.4.103",
    ),
    ("MPEG4HighProfile/Level4.1", "1.2.840.10008.1.2.4.102"),
    (
        "MPEG4HighProfile/Level4.2For2DVideo",
        "1.2.840.10008.1.2.4.104",
    ),
    (
        "MPEG4HighProfile/Level4.2For3DVideo",
        "1.2.840.10008.1.2.4.105",
    ),
    ("MPEG4StereoHighProfile/Level4.2", "1.2.840.10008.1.2.4.106"),
    (
        "MultiframeGrayscaleByteSecondaryCaptureImageStorage",
        "1.2.840.10008.5.1.4.1.1.7.2",
    ),
    (
        "MultiframeGrayscaleWordSecondaryCaptureImageStorage",
        "1.2.840.10008.5.1.4.1.1.7.3",
    ),
    (
        "MultiframeSingleBitSecondaryCaptureImageStorage",
        "1.2.840.10008.5.1.4.1.1.7.1",
    ),
    (
        "MultiframeTrueColorSecondaryCaptureImageStorage",
        "1.2.840.10008.5.1.4.1.1.7.4",
    ),
    (
        "RETIRED_HardcopyColorImageStorage",
        "1.2.840.10008.5.1.1.30",
    ),
    (
        "RETIRED_HardcopyGrayscaleImageStorage",
        "1.2.840.10008.5.1.1.29",
    ),
    (
        "RETIRED_NuclearMedicineImageStorage",
        "1.2.840.10008.5.1.4.1.1.5",
    ),
    (
        "RETIRED_StandaloneCurveStorage",
        "1.2.840.10008.5.1.4.1.1.9",
    ),
    (
        "RETIRED_StandaloneModalityLUTStorage",
        "1.2.840.10008.5.1.4.1.1.10",
    ),
    (
        "RETIRED_StandaloneOverlayStorage",
        "1.2.840.10008.5.1.4.1.1.8",
    ),
    (
        "RETIRED_StandalonePETCurveStorage",
        "1.2.840.10008.5.1.4.1.1.129",
    ),
    (
        "RETIRED_StandaloneVOILUTStorage",
        "1.2.840.10008.5.1.4.1.1.11",
    ),
    ("RETIRED_StoredPrintStorage", "1.2.840.10008.5.1.1.27"),
    (
        "RETIRED_UltrasoundImageStorage",
        "1.2.840.10008.5.1.4.1.1.6",
    ),
    (
        "RETIRED_UltrasoundMultiframeImageStorage",
        "1.2.840.10008.5.1.4.1.1.3",
    ),
    ("RETIRED_VLImageStorage", "1.2.840.10008.5.1.4.1.1.77.1"),
    (
        "RETIRED_VLMultiframeImageStorage",
        "1.2.840.10008.5.1.4.1.1.77.2",
    ),
    (
        "RETIRED_XRayAngiographicBiPlaneImageStorage",
        "1.2.840.10008.5.1.4.1.1.12.3",
    ),
    (
        "UltrasoundMultiframeImageStorage",
        "1.2.840.10008.5.1.4.1.1.3.1",
    ),
    ("VerificationSOPClass", "1.2.840.10008.1.1"),
];
fn config_uid(value: &str, order: Option<LocalByteOrder>, transfer_syntax: bool) -> Result<String> {
    let value = value.strip_prefix('=').unwrap_or(value);
    if value == "LocalEndianExplicit" || value == "OppositeEndianExplicit" {
        if !transfer_syntax {
            return Err(E::Invalid);
        }
        let mut little = match order {
            Some(LocalByteOrder::BigEndian) => false,
            Some(_) => true,
            None => cfg!(target_endian = "little"),
        };
        if value == "OppositeEndianExplicit" {
            little = !little;
        }
        return Ok(if little {
            "1.2.840.10008.1.2.1"
        } else {
            "1.2.840.10008.1.2.2"
        }
        .to_string());
    }
    let mut uid = value.to_string();
    if let Some((_, number)) = ALIASES.iter().find(|(k, _)| *k == value) {
        uid = number.to_string();
    } else if let Some(e) = uiddict::by_keyword(value).filter(|e| e.keyword == value) {
        let ok = if transfer_syntax {
            e.typ == uiddict::Type::TRANSFER_SYNTAX
        } else {
            e.typ == uiddict::Type::SOP_CLASS || e.typ == uiddict::Type::META_SOP_CLASS
        };
        if !ok {
            return Err(E::Invalid);
        }
        uid = e.uid.to_string();
    }
    if !crate::object::is_valid_uid(uid.as_bytes()) {
        return Err(E::Invalid);
    }
    Ok(uid)
}
fn sections<'a>(groups: &'a Groups, name: &str) -> &'a [(String, Section)] {
    groups.get(name).map_or(&[], Vec::as_slice)
}
fn compile(
    ctx: Option<&Ctx>,
    groups: &Groups,
    options: &AssociationConfigOptions,
) -> Result<AssociationConfig> {
    for required in ["TRANSFERSYNTAXES", "PRESENTATIONCONTEXTS", "PROFILES"] {
        if sections(groups, required).is_empty() {
            return Err(E::Invalid);
        }
    }
    let l = options.limits;
    let order = options.local_byte_order;
    let mut c = AssociationConfig {
        limits: l,
        syntaxes: HashMap::new(),
        contexts: HashMap::new(),
        roles: HashMap::new(),
        extended: HashMap::new(),
        profiles: HashMap::new(),
    };
    for (name, section) in sections(groups, "TRANSFERSYNTAXES") {
        ctx_err(ctx)?;
        let values = numbered(section, "TRANSFERSYNTAX", l.max_transfer_syntaxes)?;
        if values.is_empty() {
            return Err(E::Invalid);
        }
        let mut seen = HashSet::new();
        for value in values {
            ctx_err(ctx)?;
            let uid = config_uid(&value, order, true)?;
            if !seen.insert(uid.clone()) {
                return Err(E::Invalid);
            }
            c.syntaxes.entry(name.clone()).or_default().push(uid);
        }
    }
    for (name, section) in sections(groups, "PRESENTATIONCONTEXTS") {
        ctx_err(ctx)?;
        let values = numbered(section, "PRESENTATIONCONTEXT", l.max_context_rules)?;
        if values.is_empty() {
            return Err(E::Invalid);
        }
        for value in values {
            ctx_err(ctx)?;
            let (a, b) = pair(&value)?;
            let uid = config_uid(&a, order, false)?;
            let label = config_name(&b, l.max_name_bytes)?;
            if !c.syntaxes.contains_key(&label) {
                return Err(E::Invalid);
            }
            c.contexts
                .entry(name.clone())
                .or_default()
                .push(ConfigContext {
                    sop: uid,
                    syntax_list: label,
                });
        }
    }
    for (name, section) in sections(groups, "SCPSCUROLESELECTION") {
        ctx_err(ctx)?;
        let values = numbered(section, "ROLE", l.max_role_items)?;
        let mut items = Vec::with_capacity(values.len());
        let mut seen = HashSet::new();
        for value in values {
            ctx_err(ctx)?;
            let (a, b) = pair(&value)?;
            let uid = config_uid(&a, order, false)?;
            if !seen.insert(uid.clone()) {
                return Err(E::Invalid);
            }
            let role = b.to_ascii_uppercase();
            if role != "SCU" && role != "SCP" && role != "BOTH" {
                return Err(E::Invalid);
            }
            items.push(RoleSelectionItem {
                sop_class_uid: uid,
                scu_role: role == "SCU" || role == "BOTH",
                scp_role: role == "SCP" || role == "BOTH",
            });
        }
        c.roles.insert(name.clone(), items);
    }
    let mut total = 0i64;
    for (name, section) in sections(groups, "EXTENDEDNEGOTIATION") {
        ctx_err(ctx)?;
        let values = numbered(section, "EXTENDEDNEGOTIATION", l.max_extended_items)?;
        let mut items = Vec::with_capacity(values.len());
        let mut seen = HashSet::new();
        for value in values {
            ctx_err(ctx)?;
            let (a, b) = pair(&value)?;
            let uid = config_uid(&a, order, false)?;
            if seen.contains(&uid) || b.len() % 2 != 0 {
                return Err(E::Invalid);
            }
            seen.insert(uid.clone());
            let n = (b.len() / 2) as i64;
            if n > l.max_extended_item_bytes || n > l.max_extended_bytes - total {
                return Err(E::Limit);
            }
            let data = decode_hex(&b).ok_or(E::Invalid)?;
            total += n;
            items.push(SopClassExtendedNegotiationItem {
                sop_class_uid: uid,
                data,
            });
        }
        c.extended.insert(name.clone(), items);
    }
    for (name, section) in sections(groups, "PROFILES") {
        ctx_err(ctx)?;
        if section.entries.keys().any(|k| {
            k != "PRESENTATIONCONTEXTS" && k != "SCPSCUROLESELECTION" && k != "EXTENDEDNEGOTIATION"
        }) {
            return Err(E::Invalid);
        }
        let get = |k: &str| section.entries.get(k).cloned();
        let mut profile = ConfigProfile {
            contexts: config_name(
                &get("PRESENTATIONCONTEXTS").unwrap_or_default(),
                l.max_name_bytes,
            )?,
            ..ConfigProfile::default()
        };
        let Some(contexts) = c.contexts.get(&profile.contexts) else {
            return Err(E::Invalid);
        };
        if let Some(v) = get("SCPSCUROLESELECTION") {
            profile.roles = config_name(&v, l.max_name_bytes)?;
        }
        if let Some(v) = get("EXTENDEDNEGOTIATION") {
            profile.extended = config_name(&v, l.max_name_bytes)?;
        }
        if !profile.roles.is_empty() && !c.roles.contains_key(&profile.roles) {
            return Err(E::Invalid);
        }
        if !profile.extended.is_empty() && !c.extended.contains_key(&profile.extended) {
            return Err(E::Invalid);
        }
        let present: HashSet<&str> = contexts.iter().map(|pc| pc.sop.as_str()).collect();
        let roles = c.roles.get(&profile.roles).map_or(&[][..], Vec::as_slice);
        if roles
            .iter()
            .any(|r| !present.contains(r.sop_class_uid.as_str()))
        {
            return Err(E::Invalid);
        }
        let extended = c
            .extended
            .get(&profile.extended)
            .map_or(&[][..], Vec::as_slice);
        if extended
            .iter()
            .any(|i| !present.contains(i.sop_class_uid.as_str()))
        {
            return Err(E::Invalid);
        }
        c.profiles.insert(name.clone(), profile);
    }
    Ok(c)
}
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    s.as_bytes()
        .chunks(2)
        .map(|p| {
            let h = (p[0] as char).to_digit(16)?;
            let l = (p[1] as char).to_digit(16)?;
            Some((h << 4 | l) as u8)
        })
        .collect()
}
impl AssociationConfig {
    pub fn profile(&self, name: &str, mode: AssociationConfigMode) -> Result<AssociationProfile> {
        let name = config_name(name, self.limits.max_name_bytes)?;
        let Some(definition) = self.profiles.get(&name) else {
            return Err(E::Invalid);
        };
        let contexts = self
            .contexts
            .get(&definition.contexts)
            .map_or(&[][..], Vec::as_slice);
        let requestor = mode == AssociationConfigMode::Requestor;
        if requestor && contexts.len() > MAX_PRESENTATION_CONTEXTS {
            return Err(E::Limit);
        }
        let mut remaining = self.limits.max_compiled_bytes;
        let mut consume = |n: i64| -> Result<()> {
            if n < 0 || n > remaining {
                return Err(E::Limit);
            }
            remaining -= n;
            Ok(())
        };
        let syntaxes = |pc: &ConfigContext| {
            self.syntaxes
                .get(&pc.syntax_list)
                .cloned()
                .unwrap_or_default()
        };
        for pc in contexts {
            consume(128 + pc.sop.len() as i64)?;
            let mut wire = 8 + pc.sop.len() as i64;
            for ts in &syntaxes(pc) {
                consume(16 + ts.len() as i64)?;
                wire += 4 + ts.len() as i64;
            }
            if requestor && wire > i64::from(u16::MAX) {
                return Err(E::Limit);
            }
        }
        let roles = self
            .roles
            .get(&definition.roles)
            .map_or(&[][..], Vec::as_slice);
        let extended = self
            .extended
            .get(&definition.extended)
            .map_or(&[][..], Vec::as_slice);
        for role in roles {
            consume(64 + role.sop_class_uid.len() as i64)?;
        }
        for item in extended {
            consume(64 + item.sop_class_uid.len() as i64 + item.data.len() as i64)?;
        }
        let mut role_by_sop: HashMap<&str, &RoleSelectionItem> = HashMap::new();
        if !requestor {
            consume(64)?;
            for _ in contexts {
                consume(64)?;
            }
            if !roles.is_empty() {
                consume(64)?;
                for _ in roles {
                    consume(64)?;
                }
            }
            let mut seen = HashSet::new();
            for pc in contexts {
                if !seen.insert(pc.sop.as_str()) {
                    return Err(E::Invalid);
                }
            }
            for role in roles {
                role_by_sop.insert(role.sop_class_uid.as_str(), role);
            }
        }
        let mut out = AssociationProfile {
            role_selections: roles.to_vec(),
            extended_negotiation: extended.to_vec(),
            ..AssociationProfile::default()
        };
        for (i, pc) in contexts.iter().enumerate() {
            if requestor {
                out.contexts.push(PresentationContext {
                    id: (2 * i + 1) as u8,
                    abstract_syntax_uid: pc.sop.clone(),
                    transfer_syntax_uids: syntaxes(pc),
                });
            } else {
                out.rules.push(PresentationContextRule {
                    abstract_syntax_uid: pc.sop.clone(),
                    transfer_syntaxes: syntaxes(pc),
                    roles: role_by_sop.get(pc.sop.as_str()).map(|r| RequestorRoles {
                        scu: r.scu_role,
                        scp: r.scp_role,
                    }),
                });
            }
        }
        Ok(out)
    }
}
