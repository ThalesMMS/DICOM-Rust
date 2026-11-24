use super::super::mimeheader::{self, parse_media_type};
use super::super::response::{
    frame_media_type_matches_transfer_syntax, web_service_transfer_syntax_allowed,
};
use super::ServerError;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaPreference {
    pub media_type: String,
    pub transfer_syntax_uid: String,
    pub quality: f64,
    pub multipart: bool,
}
const EXPLICIT_VR_LE: &str = "1.2.840.10008.1.2.1";
pub(crate) const DICOM_JSON: &str = "application/dicom+json";
pub(crate) const DICOM_XML: &str = "application/dicom+xml";
pub(crate) fn default_frame_syntax(media: &str) -> &'static str {
    match media {
        "application/octet-stream" => EXPLICIT_VR_LE,
        "application/x-deflate" => "1.2.840.10008.1.2.8.1",
        "image/jpeg" => "1.2.840.10008.1.2.4.70",
        "image/dicom-rle" => "1.2.840.10008.1.2.5",
        "image/jls" => "1.2.840.10008.1.2.4.80",
        "image/jp2" => "1.2.840.10008.1.2.4.90",
        "image/jpx" => "1.2.840.10008.1.2.4.92",
        "image/jphc" => "1.2.840.10008.1.2.4.201",
        "image/jxl" => "1.2.840.10008.1.2.4.110",
        _ => "",
    }
}
pub(crate) fn accepts_dicom_json(header: &str) -> bool {
    if header.trim().is_empty() {
        return true;
    }
    matches!(best_matching_media_preference(&parse_accept_header(header), DICOM_JSON, "", false), Some(p) if p.quality > 0.0)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataRepresentation {
    Json,
    Xml,
}
pub(crate) fn select_metadata_representation(header: &str) -> Option<MetadataRepresentation> {
    let prefs = parse_metadata_accept_header(header);
    let json = best_matching_media_preference(&prefs, DICOM_JSON, EXPLICIT_VR_LE, false);
    let xml = best_matching_media_preference(&prefs, DICOM_XML, EXPLICIT_VR_LE, true);
    let json = json.filter(|p| p.quality > 0.0);
    let xml = xml.filter(|p| p.quality > 0.0);
    let spec = |p: &Option<MediaPreference>, media: &str| {
        p.as_ref()
            .and_then(|p| media_preference_specificity(p, media, EXPLICIT_VR_LE))
            .unwrap_or(0)
    };
    let json_spec = spec(&json, DICOM_JSON);
    let xml_spec = spec(&xml, DICOM_XML);
    match (&json, &xml) {
        (Some(j), Some(x)) if x.quality > j.quality => Some(MetadataRepresentation::Xml),
        (Some(j), Some(x)) if x.quality == j.quality && xml_spec > json_spec => {
            Some(MetadataRepresentation::Xml)
        }
        (Some(_), _) => Some(MetadataRepresentation::Json),
        (None, Some(_)) => Some(MetadataRepresentation::Xml),
        (None, None) => None,
    }
}
pub(crate) fn parameters_supported(params: &[(String, String)], allowed: &[&str]) -> bool {
    params
        .iter()
        .all(|(name, _)| allowed.contains(&mimeheader::to_lower(name).as_str()))
}
fn param<'a>(params: &'a [(String, String)], name: &str) -> &'a str {
    params
        .iter()
        .find(|(k, _)| k == name)
        .map_or("", |(_, v)| v.as_str())
}
fn quality_of(params: &[(String, String)]) -> Option<f64> {
    let raw = param(params, "q");
    if raw.is_empty() {
        return Some(1.0);
    }
    parse_accept_q_value(raw)
}
fn sort_by_quality(result: &mut [MediaPreference]) {
    result.sort_by(|a, b| {
        b.quality
            .partial_cmp(&a.quality)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
pub(crate) fn parse_metadata_accept_header(header: &str) -> Vec<MediaPreference> {
    let mut result = Vec::new();
    for raw in split_accept_values(header) {
        let Ok(mt) = parse_media_type(raw.trim()) else {
            continue;
        };
        let params = &mt.params;
        let Some(quality) = quality_of(params) else {
            continue;
        };
        let mut media_type = mt.media_type.clone();
        let multipart_related = media_type.eq_ignore_ascii_case("multipart/related");
        if multipart_related {
            if !parameters_supported(
                params,
                &[
                    "boundary",
                    "charset",
                    "q",
                    "start",
                    "start-info",
                    "transfer-syntax",
                    "type",
                ],
            ) || param(params, "type").is_empty()
            {
                continue;
            }
            media_type = param(params, "type").trim().to_string();
        } else if !parameters_supported(params, &["charset", "q", "transfer-syntax"]) {
            continue;
        }
        let charset = param(params, "charset").trim();
        if !charset.is_empty() && !charset.eq_ignore_ascii_case("utf-8") {
            continue;
        }
        result.push(MediaPreference {
            media_type: mimeheader::to_lower(&media_type),
            transfer_syntax_uid: param(params, "transfer-syntax").trim().to_string(),
            quality,
            multipart: multipart_related,
        });
    }
    sort_by_quality(&mut result);
    result
}
pub(crate) fn parse_media_preferences(
    header: &str,
    default_media_type: &str,
) -> Vec<MediaPreference> {
    if header.trim().is_empty() {
        return vec![MediaPreference {
            media_type: default_media_type.to_string(),
            quality: 1.0,
            ..MediaPreference::default()
        }];
    }
    parse_accept_header(header)
}
pub(crate) fn parse_accept_header(header: &str) -> Vec<MediaPreference> {
    let mut result = Vec::new();
    for raw in split_accept_values(header) {
        let Ok(mt) = parse_media_type(raw.trim()) else {
            continue;
        };
        let params = &mt.params;
        let Some(quality) = quality_of(params) else {
            continue;
        };
        let mut media_type = mt.media_type.clone();
        let multipart_related = media_type.eq_ignore_ascii_case("multipart/related");
        if multipart_related && !param(params, "type").is_empty() {
            media_type = param(params, "type").trim_matches('"').to_string();
        }
        result.push(MediaPreference {
            media_type: mimeheader::to_lower(&media_type),
            transfer_syntax_uid: param(params, "transfer-syntax").trim().to_string(),
            quality,
            multipart: multipart_related,
        });
    }
    sort_by_quality(&mut result);
    result
}
pub(crate) fn parse_accept_q_value(raw: &str) -> Option<f64> {
    if raw == "0" || raw == "1" {
        return raw.parse().ok();
    }
    let b = raw.as_bytes();
    if b.len() < 2 || b.len() > 5 || b[1] != b'.' || (b[0] != b'0' && b[0] != b'1') {
        return None;
    }
    for &c in &b[2..] {
        if !c.is_ascii_digit() || (b[0] == b'1' && c != b'0') {
            return None;
        }
    }
    raw.parse().ok()
}
pub(crate) fn split_accept_values(header: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, c) in header.bytes().enumerate() {
        if c == b'"' {
            quoted = !quoted;
        }
        if c == b',' && !quoted {
            values.push(&header[start..index]);
            start = index + 1;
        }
    }
    values.push(&header[start..]);
    values
}
#[doc(hidden)]
pub fn media_preference_accepts(prefs: &[MediaPreference], content_type: &str, ts: &str) -> bool {
    matching_media_preference(prefs, content_type, ts, false).is_some()
}
pub(crate) fn matching_media_preference(
    prefs: &[MediaPreference],
    content_type: &str,
    transfer_syntax_uid: &str,
    require_multipart: bool,
) -> Option<MediaPreference> {
    let mt = parse_media_type(content_type).ok()?;
    let ts = if transfer_syntax_uid.is_empty() {
        mt.param("transfer-syntax").to_string()
    } else {
        transfer_syntax_uid.to_string()
    };
    let media_type = mimeheader::to_lower(mt.media_type.trim());
    if require_multipart {
        return best_matching_media_preference(prefs, &media_type, &ts, true)
            .filter(|p| p.quality > 0.0);
    }
    let direct = best_matching_media_preference(prefs, &media_type, &ts, false);
    let multipart = best_matching_media_preference(prefs, &media_type, &ts, true);
    match direct {
        Some(d) if d.quality > 0.0 => match multipart {
            Some(m) if m.quality > d.quality => Some(m),
            _ => Some(d),
        },
        _ => multipart.filter(|m| m.quality > 0.0),
    }
}
pub(crate) fn best_matching_media_preference(
    prefs: &[MediaPreference],
    media_type: &str,
    transfer_syntax_uid: &str,
    multipart: bool,
) -> Option<MediaPreference> {
    let mut best_specificity = -1i32;
    let mut best: Option<MediaPreference> = None;
    for p in prefs {
        if p.multipart != multipart && !(multipart && !p.multipart && p.media_type == "*/*") {
            continue;
        }
        let Some(specificity) = media_preference_specificity(p, media_type, transfer_syntax_uid)
        else {
            continue;
        };
        let best_quality = best.as_ref().map_or(0.0, |b| b.quality);
        if specificity > best_specificity
            || (specificity == best_specificity && p.quality > best_quality)
        {
            let mut chosen = p.clone();
            chosen.multipart = multipart;
            best = Some(chosen);
            best_specificity = specificity;
        }
    }
    best
}
pub(crate) fn media_preference_specificity(
    p: &MediaPreference,
    media_type: &str,
    ts: &str,
) -> Option<i32> {
    let mut specificity = if p.media_type == media_type {
        200
    } else if p.media_type == "*/*" {
        0
    } else if p.media_type.ends_with("/*")
        && media_type.starts_with(p.media_type.trim_end_matches('*'))
        && media_type.starts_with(&p.media_type[..p.media_type.len() - 1])
    {
        100
    } else {
        return None;
    };
    if !p.transfer_syntax_uid.is_empty() {
        if p.transfer_syntax_uid == "*" {
            specificity += 1;
        } else {
            if p.transfer_syntax_uid != ts {
                return None;
            }
            specificity += 2;
        }
    }
    Some(specificity)
}
pub(crate) fn normalized_dicom_part_content_type(
    content_type: &str,
    ts: &str,
) -> Result<String, ServerError> {
    let trimmed = content_type.trim();
    let media = match parse_media_type(trimmed) {
        Ok(mt) => mt.media_type,
        Err(e) if !trimmed.is_empty() => return Err(ServerError::Other(e)),
        Err(_) => String::new(),
    };
    let media = if media.is_empty() {
        "application/dicom".to_string()
    } else {
        media
    };
    if !media.eq_ignore_ascii_case("application/dicom") {
        return Err(ServerError::Unsupported);
    }
    Ok(media_type_with_transfer_syntax("application/dicom", ts))
}
pub(crate) fn normalized_frame_content_type(content_type: &str) -> Result<String, ServerError> {
    if content_type.trim().is_empty() {
        return Ok("application/octet-stream".into());
    }
    match parse_media_type(content_type) {
        Ok(mt) if mt.params.is_empty() && is_frame_media_type(&mt.media_type) => {
            Ok(mimeheader::to_lower(&mt.media_type))
        }
        _ => Err(ServerError::Unsupported),
    }
}
pub(crate) fn is_frame_media_type(media_type: &str) -> bool {
    matches!(
        mimeheader::to_lower(media_type).as_str(),
        "application/octet-stream"
            | "application/x-deflate"
            | "image/jpeg"
            | "image/jls"
            | "image/jp2"
            | "image/jpx"
            | "image/jphc"
            | "image/jxl"
            | "image/dicom-rle"
            | "video/mpeg"
            | "video/mp4"
            | "video/h265"
    )
}
pub(crate) fn frame_media_matches_syntax(media_type: &str, ts: &str) -> bool {
    frame_media_type_matches_transfer_syntax(media_type, ts)
}
pub(crate) fn web_service_syntax_allowed(ts: &str) -> bool {
    web_service_transfer_syntax_allowed(ts)
}
pub(crate) fn normalized_bulk_content_type(content_type: &str) -> Result<String, ServerError> {
    if content_type.trim().is_empty() {
        return Ok("application/octet-stream".into());
    }
    match parse_media_type(content_type) {
        Ok(mt)
            if mt.params.is_empty()
                && mt
                    .media_type
                    .eq_ignore_ascii_case("application/octet-stream") =>
        {
            Ok("application/octet-stream".into())
        }
        _ => Err(ServerError::Unsupported),
    }
}
pub(crate) fn media_type_with_transfer_syntax(media_type: &str, ts: &str) -> String {
    let ts = ts.trim();
    if ts.is_empty() {
        return media_type.to_string();
    }
    format_media_type(media_type, &[("transfer-syntax", ts)])
}
pub(crate) fn prepared_preferences_supported(
    prefs: &[MediaPreference],
    frames: bool,
    multipart_required: bool,
) -> bool {
    for p in prefs {
        if p.quality <= 0.0 || (multipart_required && !p.multipart && p.media_type != "*/*") {
            continue;
        }
        if !p.transfer_syntax_uid.is_empty()
            && p.transfer_syntax_uid != "*"
            && !web_service_transfer_syntax_allowed(&p.transfer_syntax_uid)
        {
            continue;
        }
        let m = p.media_type.as_str();
        if !frames && (m == "application/dicom" || m == "application/*" || m == "*/*") {
            return true;
        }
        if frames && (is_frame_media_type(m) || m == "image/*" || m == "video/*" || m == "*/*") {
            return true;
        }
    }
    false
}
fn is_tspecial(c: char) -> bool {
    "()<>@,;:\\\"/[]?=".contains(c)
}
fn is_token_char(c: char) -> bool {
    (c as u32) > 0x20 && (c as u32) < 0x7f && !is_tspecial(c)
}
fn is_token(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_token_char)
}
pub(crate) fn format_media_type(t: &str, params: &[(&str, &str)]) -> String {
    let mut b = String::new();
    match t.split_once('/') {
        None => {
            if !is_token(t) {
                return String::new();
            }
            b.push_str(&mimeheader::to_lower(t));
        }
        Some((major, sub)) => {
            if !is_token(major) || !is_token(sub) {
                return String::new();
            }
            b.push_str(&mimeheader::to_lower(major));
            b.push('/');
            b.push_str(&mimeheader::to_lower(sub));
        }
    }
    let mut sorted: Vec<&(&str, &str)> = params.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (attribute, value) in sorted {
        b.push_str("; ");
        if !is_token(attribute) {
            return String::new();
        }
        b.push_str(&mimeheader::to_lower(attribute));
        let needs_encoding = value.bytes().any(|c| c < 0x20 && c != b'\t' || c >= 0x7f);
        if needs_encoding {
            b.push_str("*=utf-8''");
            for c in value.bytes() {
                if c <= b' '
                    || c >= 0x7f
                    || c == b'*'
                    || c == b'\''
                    || c == b'%'
                    || is_tspecial(c as char)
                {
                    b.push_str(&format!("%{c:02X}"));
                } else {
                    b.push(c as char);
                }
            }
            continue;
        }
        b.push('=');
        if is_token(value) {
            b.push_str(value);
        } else {
            b.push('"');
            for c in value.chars() {
                if c == '"' || c == '\\' {
                    b.push('\\');
                }
                b.push(c);
            }
            b.push('"');
        }
    }
    b
}
