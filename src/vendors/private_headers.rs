use super::blobs::{elscint_protocol_of, ge_protocol_data_block_of, vepro_data_of};
use super::{ByteString, Error, Kind, Limits, headers, text_lines};
use crate::dictionary::reservations::private_tag;
use crate::object::{Object, Tag};
use crate::siemens::{self, CsaFormat, CsaHeader, CsaLimits, LookupOptions, MedComLimits};
pub const SIEMENS_CSA_IMAGE: &str = "siemens-csa-image";
pub const SIEMENS_CSA_SERIES: &str = "siemens-csa-series";
pub const SIEMENS_MR_PROTOCOL: &str = "siemens-mr-protocol";
pub const SIEMENS_MEDCOM_HISTORY: &str = "siemens-medcom-history";
pub const GE_PROTOCOL_DATA_BLOCK_HEADER: &str = "ge-protocol-data-block";
pub const ELSCINT_PROTOCOL_HEADER: &str = "elscint-protocol";
pub const VEPRO_HEADER: &str = "vepro-data";
pub const PHILIPS_SDS_HEADER: &str = "philips-sds";
pub const TOSHIBA_CT3_HEADER: &str = "toshiba-ct3";
pub const TOSHIBA_MEC_MR3_HEADER: &str = "toshiba-mec-mr3";
pub const PMTF_HEADER: &str = "pmtf";
pub const CANON_PMTF_HEADER: &str = "canon-pmtf";
pub const TOSHIBA_PMTF_HEADER: &str = "toshiba-pmtf";
pub const CANON_PAS_HEADER: &str = "canon-pas";
const BACKSLASH: &[u8] = &[0x5C];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateHeaderInfo {
    pub kind: &'static str,
    pub title: &'static str,
    pub may_identify: bool,
    group: u16,
    creators: &'static [&'static str],
    offset: u8,
}
const fn info(
    kind: &'static str,
    title: &'static str,
    may_identify: bool,
    group: u16,
    creators: &'static [&'static str],
    offset: u8,
) -> PrivateHeaderInfo {
    PrivateHeaderInfo {
        kind,
        title,
        may_identify,
        group,
        creators,
        offset,
    }
}
const CSA: &[&str] = &["SIEMENS CSA HEADER"];
const TOSHIBA_MR3: &[&str] = &["TOSHIBA_MEC_MR3"];
const PRIVATE_HEADER_KINDS: &[PrivateHeaderInfo] = &[
    info(
        SIEMENS_CSA_IMAGE,
        "Siemens CSA Image Header",
        false,
        0x0029,
        CSA,
        0x10,
    ),
    info(
        SIEMENS_CSA_SERIES,
        "Siemens CSA Series Header",
        false,
        0x0029,
        CSA,
        0x20,
    ),
    info(
        SIEMENS_MR_PROTOCOL,
        "Siemens MR Protocol (ASCCONV)",
        false,
        0x0029,
        CSA,
        0x20,
    ),
    info(
        SIEMENS_MEDCOM_HISTORY,
        "Siemens MedCom History",
        true,
        0x0029,
        &["SIEMENS MEDCOM HEADER"],
        0x20,
    ),
    info(
        GE_PROTOCOL_DATA_BLOCK_HEADER,
        "GE Protocol Data Block",
        false,
        0x0025,
        &["GEMS_SERS_01"],
        0x1B,
    ),
    info(
        ELSCINT_PROTOCOL_HEADER,
        "ELSCINT1 Protocol Information",
        false,
        0x01F7,
        &["ELSCINT1"],
        0x26,
    ),
    info(
        VEPRO_HEADER,
        "VEPRO Data",
        true,
        0x0055,
        &["VEPRO VIF 3.0 DATA", "VEPRO VIM 5.0 DATA"],
        0x20,
    ),
    info(
        PHILIPS_SDS_HEADER,
        "Philips SDS",
        true,
        0x2005,
        &["Philips MR Imaging DD 002"],
        0x32,
    ),
    info(
        TOSHIBA_CT3_HEADER,
        "Toshiba CT Private Data 2",
        true,
        0x7005,
        &["TOSHIBA_MEC_CT3"],
        0x10,
    ),
    info(
        TOSHIBA_MEC_MR3_HEADER,
        "Toshiba MEC MR3 Original Data",
        true,
        0x700d,
        TOSHIBA_MR3,
        0x08,
    ),
    info(
        PMTF_HEADER,
        "PMTF Information Data",
        true,
        0x0029,
        &["PMTF INFORMATION DATA"],
        1,
    ),
    info(
        CANON_PMTF_HEADER,
        "Canon MEC MR3 PMTF",
        true,
        0x0029,
        &["CANON_MEC_MR3"],
        1,
    ),
    info(
        TOSHIBA_PMTF_HEADER,
        "Toshiba MEC MR3 PMTF",
        true,
        0x0029,
        TOSHIBA_MR3,
        1,
    ),
    info(
        TOSHIBA_PMTF_HEADER,
        "Toshiba MEC MR3 PMTF",
        true,
        0x0029,
        TOSHIBA_MR3,
        2,
    ),
    info(
        CANON_PAS_HEADER,
        "Canon PAS Reproduct Information (XML)",
        true,
        0x700d,
        &["CANON_MEC_MR3^10"],
        0x19,
    ),
];
pub fn private_header_kinds() -> &'static [PrivateHeaderInfo] {
    PRIVATE_HEADER_KINDS
}
pub fn private_header_info_of(kind: &str) -> Option<PrivateHeaderInfo> {
    PRIVATE_HEADER_KINDS
        .iter()
        .find(|i| i.kind == kind)
        .copied()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateHeaderLocation {
    pub info: PrivateHeaderInfo,
    pub tag: Tag,
}
pub fn detect_private_headers(dataset: &Object) -> Vec<PrivateHeaderLocation> {
    let mut out = Vec::new();
    for info in PRIVATE_HEADER_KINDS {
        for creator in info.creators {
            match private_tag(dataset, info.group, creator, info.offset) {
                Ok(tag) if dataset.has(tag) => {
                    out.push(PrivateHeaderLocation { info: *info, tag });
                    break;
                }
                _ => {}
            }
        }
    }
    out
}
#[derive(Clone, Debug, Default)]
pub struct PrivateHeaderOptions {
    pub kinds: Vec<String>,
    pub limits: Limits,
    pub max_lines: i64,
}
#[derive(Clone, Debug)]
pub struct PrivateHeader {
    pub location: PrivateHeaderLocation,
    pub lines: Vec<ByteString>,
    pub truncated: bool,
    pub err: Option<Error>,
}
pub fn decode_private_headers(dataset: &Object, opts: &PrivateHeaderOptions) -> Vec<PrivateHeader> {
    let max_lines = if opts.max_lines <= 0 {
        20000
    } else {
        opts.max_lines as usize
    };
    let mut out = Vec::new();
    for location in detect_private_headers(dataset) {
        if !opts.kinds.is_empty() && !opts.kinds.iter().any(|k| k == location.info.kind) {
            continue;
        }
        let mut header = PrivateHeader {
            location,
            lines: Vec::new(),
            truncated: false,
            err: None,
        };
        match render_private_header(dataset, &location, opts.limits) {
            Err(err)
                if location.info.kind == SIEMENS_MR_PROTOCOL
                    && err.is(Kind::Siemens(siemens::Kind::NoAscconv)) =>
            {
                continue;
            }
            Err(err) => header.err = Some(err),
            Ok(mut lines) => {
                if lines.len() > max_lines {
                    lines.truncate(max_lines);
                    header.truncated = true;
                }
                header.lines = lines;
            }
        }
        out.push(header);
    }
    out
}
fn line(parts: &[&[u8]]) -> ByteString {
    parts.concat()
}
fn render_private_header(
    dataset: &Object,
    location: &PrivateHeaderLocation,
    limits: Limits,
) -> Result<Vec<ByteString>, Error> {
    let limits = limits.normalized();
    let siemens_options = LookupOptions {
        allow_missing_creator: false,
        limits: CsaLimits {
            max_bytes: limits.max_bytes,
            ..CsaLimits::default()
        },
    };
    match location.info.kind {
        SIEMENS_CSA_IMAGE => {
            let header = siemens::image_header(dataset, siemens_options)?;
            Ok(csa_lines(&header.header))
        }
        SIEMENS_CSA_SERIES => {
            let header = siemens::series_header(dataset, siemens_options)?;
            Ok(csa_lines(&header.header))
        }
        SIEMENS_MR_PROTOCOL => {
            let protocol = siemens::series_protocol(dataset, siemens_options)?;
            Ok(protocol
                .entries()
                .iter()
                .map(|e| format!("{} = {}", e.key, e.value).into_bytes())
                .collect())
        }
        SIEMENS_MEDCOM_HISTORY => {
            let history = siemens::medcom_history_of(
                dataset,
                MedComLimits {
                    max_bytes: limits.max_bytes,
                    max_lines: limits.max_entries,
                },
            )?;
            let mut lines = vec![format!("Version: {}", history.version).into_bytes()];
            for section in &history.sections {
                lines.push(section.name.clone().into_bytes());
                for record in &section.records {
                    lines.push(format!("  {}", record.join(" | ")).into_bytes());
                }
            }
            Ok(lines)
        }
        GE_PROTOCOL_DATA_BLOCK_HEADER => {
            let block = ge_protocol_data_block_of(dataset, limits)?;
            let mut lines = Vec::new();
            if block.truncated {
                lines
                    .push(b"(gzip stream ends before its trailer; checksum not verified)".to_vec());
            }
            if block.xml || block.entries.is_empty() {
                lines.extend(text_lines(&block.text));
                return Ok(lines);
            }
            for entry in &block.entries {
                lines.push(line(&[&entry.key, b" = ", &entry.value]));
            }
            Ok(lines)
        }
        ELSCINT_PROTOCOL_HEADER => {
            let protocol = elscint_protocol_of(dataset, limits)?;
            if protocol.groups.is_empty() {
                return Ok(text_lines(&protocol.text));
            }
            let mut lines = Vec::new();
            for group in &protocol.groups {
                for (i, item) in group.items.iter().enumerate() {
                    let mut name = group.name.clone();
                    if group.multiple {
                        name.extend_from_slice(format!("[{i}]").as_bytes());
                    }
                    if item.is_empty() {
                        lines.push(line(&[&name, b": (empty)"]));
                    }
                    for entry in item {
                        let values = entry.values.join(BACKSLASH);
                        lines.push(line(&[&name, b": ", &entry.key, b" = ", &values]));
                    }
                }
            }
            Ok(lines)
        }
        VEPRO_HEADER => {
            let data = vepro_data_of(dataset, limits)?;
            let mut lines = vec![
                line(&[b"ID: ", &data.id]),
                line(&[b"Version: ", &data.version]),
                line(&[b"User Name: ", &data.user_name]),
            ];
            for (i, address) in data.user_address.iter().enumerate() {
                if !address.is_empty() {
                    let label = format!("User Address {}: ", i + 1);
                    lines.push(line(&[label.as_bytes(), address]));
                }
            }
            lines.push(line(&[b"Recording Date: ", &data.recording_date]));
            lines.push(line(&[b"Recording Time: ", &data.recording_time]));
            lines.push(line(&[b"Recording Place: ", &data.recording_place]));
            lines.push(line(&[b"Recording Source: ", &data.recording_source]));
            for (i, field) in data.data_fields.iter().enumerate() {
                if !field.is_empty() {
                    let label = format!("Data Field {}: ", i + 1);
                    lines.push(line(&[label.as_bytes(), field]));
                }
            }
            lines.push(line(&[b"Study Instance UID: ", &data.study_instance_uid]));
            lines.push(line(&[b"Series UID: ", &data.series_uid]));
            lines.push(line(&[b"Modality: ", &data.modality]));
            Ok(lines)
        }
        kind => Ok(
            headers::render_private_header(dataset, kind, location.tag, limits)?
                .into_iter()
                .map(String::into_bytes)
                .collect(),
        ),
    }
}
fn csa_lines(header: &CsaHeader) -> Vec<ByteString> {
    let format = header.format.map_or("", CsaFormat::as_str);
    let mut lines = vec![format!("Format: {format}").into_bytes()];
    if header.format == Some(CsaFormat::Zeroed) {
        lines.push(b"(all bytes zero)".to_vec());
        return lines;
    }
    if let Some(dataset) = &header.dataset {
        for element in dataset.elements() {
            let value = match dataset.get_strings(element.tag()) {
                Some(values) => format!("[{}]", values.join("\u{5C}")),
                None => format!("({} bytes)", element.header.length),
            };
            lines.push(format!("{} {} {value}", element.tag(), element.vr()).into_bytes());
        }
        return lines;
    }
    if !header.text.is_empty() {
        lines.extend(text_lines(&header.text));
        return lines;
    }
    for element in &header.elements {
        let values = element.strings().join(BACKSLASH);
        let head = format!("{} {} {} [", element.name, element.vr, element.vm);
        lines.push(line(&[head.as_bytes(), &values, b"]"]));
    }
    lines
}
