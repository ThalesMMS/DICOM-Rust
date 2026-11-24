use super::{
    DocKind, ENCAPSULATED_CDA_STORAGE, ENCAPSULATED_MTL_STORAGE, ENCAPSULATED_OBJ_STORAGE,
    ENCAPSULATED_PDF_STORAGE, ENCAPSULATED_STL_STORAGE, Error, KIND_CDA, KIND_MTL, KIND_OBJ,
    KIND_PDF, KIND_STL, Kind, Result, binary_stl_length,
};
use crate::dicomxml::xmlwire::Tok;
use crate::numtext::{parse_float, parse_int};
pub(super) const CDA_NAMESPACE: &str = "urn:hl7-org:v3";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PayloadLimits {
    pub max_bytes: i64,
    pub max_depth: i64,
    pub max_records: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PayloadInfo {
    pub sop_class_uid: String,
    pub mime_type: String,
    pub hl7_instance_identifier: String,
}
fn invalid() -> Error {
    Error::sentinel(Kind::InvalidDocument)
}
fn too_large() -> Error {
    Error::sentinel(Kind::TooLarge)
}
fn trim_unicode_space(b: &[u8]) -> &[u8] {
    use crate::textfmt::decode_rune;
    let mut start = 0;
    while start < b.len() {
        match decode_rune(&b[start..]) {
            (Some(c), n) if c.is_whitespace() => start += n,
            _ => break,
        }
    }
    let mut end = b.len();
    'outer: while end > start {
        for k in (end.saturating_sub(4).max(start)..end).rev() {
            if b[k] & 0xC0 != 0x80 {
                if let (Some(c), n) = decode_rune(&b[k..end])
                    && k + n == end
                    && c.is_whitespace()
                {
                    end = k;
                    continue 'outer;
                }
                break 'outer;
            }
        }
        break;
    }
    &b[start..end]
}
pub(super) fn lower_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{130}' => 'i',
            _ => {
                let mut l = c.to_lowercase();
                match (l.next(), l.next()) {
                    (Some(x), None) => x,
                    _ => c,
                }
            }
        })
        .collect()
}
pub fn inspect_payload(mime_type: &str, data: &[u8], limits: PayloadLimits) -> Result<PayloadInfo> {
    let mut l = limits;
    if l.max_bytes < 0
        || l.max_bytes > 1 << 30
        || l.max_depth < 0
        || l.max_depth > 256
        || l.max_records < 0
        || l.max_records > 1_000_000
    {
        return Err(invalid());
    }
    if l.max_bytes == 0 {
        l.max_bytes = 256 << 20;
    }
    if l.max_depth == 0 {
        l.max_depth = 64;
    }
    if l.max_records == 0 {
        l.max_records = 100_000;
    }
    if data.len() as i64 > l.max_bytes {
        return Err(too_large());
    }
    if data.is_empty() {
        return Err(invalid());
    }
    let k: DocKind = match lower_text(mime_type).as_str() {
        "application/pdf" => KIND_PDF,
        "text/xml" => KIND_CDA,
        "model/stl" => KIND_STL,
        "model/obj" => KIND_OBJ,
        "model/mtl" => KIND_MTL,
        _ => return Err(Error::sentinel(Kind::UnsupportedSopClass)),
    };
    let mut info = PayloadInfo {
        sop_class_uid: k.sop_class_uid.to_string(),
        mime_type: k.mime_type.to_string(),
        ..PayloadInfo::default()
    };
    match k.sop_class_uid {
        ENCAPSULATED_PDF_STORAGE => {
            let tail = trim_unicode_space(data);
            if data.len() < 8
                || !data.starts_with(b"%PDF-")
                || (data[5] != b'1' && data[5] != b'2')
                || data[6] != b'.'
                || data[7] < b'0'
                || data[7] > b'7'
                || (data[5] == b'2' && data[7] != b'0')
                || !tail.ends_with(b"%%EOF")
            {
                return Err(invalid());
            }
        }
        ENCAPSULATED_CDA_STORAGE => {
            info.hl7_instance_identifier = inspect_cda(data, &l)?;
        }
        ENCAPSULATED_STL_STORAGE => match binary_stl_length(data) {
            Some(n) if n == data.len() as i64 => {
                let count = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as i64;
                if count < 1 {
                    return Err(invalid());
                }
                if count > l.max_records {
                    return Err(too_large());
                }
                for i in 0..count as usize {
                    let mut at = 84 + i * 50;
                    while at < 84 + i * 50 + 48 {
                        let v = f32::from_le_bytes([
                            data[at],
                            data[at + 1],
                            data[at + 2],
                            data[at + 3],
                        ]);
                        if !v.is_finite() {
                            return Err(invalid());
                        }
                        at += 4;
                    }
                }
            }
            _ => inspect_model_text(data, k.sop_class_uid, &l)?,
        },
        _ => inspect_model_text(data, k.sop_class_uid, &l)?,
    }
    Ok(info)
}
pub(super) fn xml_decoder(data: &[u8]) -> crate::dicomxml::xmlwire::Decoder {
    crate::dicomxml::xmlwire::Decoder::new(data.to_vec(), None, no_charset_reader)
}
const NIL_READER: &str = "\u{0}nil CharsetReader";
fn no_charset_reader(_: &str, _: &[u8]) -> std::result::Result<Vec<u8>, String> {
    Err(NIL_READER.to_string())
}
pub(super) fn xml_error_text(err: &crate::dicomxml::xmlwire::XmlError) -> String {
    let text = err.to_string();
    if let Some(rest) = text.strip_prefix("xml: opening charset ")
        && let Some(quoted) = rest.strip_suffix(&format!(": {NIL_READER}"))
    {
        return format!("xml: encoding {quoted} declared but no charset reader is set");
    }
    text
}
pub(super) fn cda_identifier_attributes(
    attrs: &[crate::dicomxml::xmlwire::Attr],
) -> Result<(String, String)> {
    let (mut root, mut extension) = (String::new(), String::new());
    let (mut root_seen, mut extension_seen) = (false, false);
    for a in attrs {
        if !a.name.space.is_empty() {
            continue;
        }
        match a.name.local.as_str() {
            "root" => {
                if root_seen {
                    return Err(invalid());
                }
                root = a.value.clone();
                root_seen = true;
            }
            "extension" => {
                if extension_seen {
                    return Err(invalid());
                }
                extension = a.value.clone();
                extension_seen = true;
            }
            _ => {}
        }
    }
    Ok((root, extension))
}
fn inspect_cda(data: &[u8], l: &PayloadLimits) -> Result<String> {
    if std::str::from_utf8(data).is_err() || data.contains(&0) {
        return Err(invalid());
    }
    let mut d = xml_decoder(data);
    let (mut depth, mut records, mut roots, mut ids) = (0i64, 0i64, 0, 0);
    let mut identifier = String::new();
    loop {
        let token = match d.token() {
            Ok(t) => t,
            Err(crate::dicomxml::xmlwire::XmlError::Eof) => break,
            Err(_) => return Err(invalid()),
        };
        records += 1;
        if records > l.max_records {
            return Err(too_large());
        }
        match token {
            Tok::Start(name, attrs) => {
                depth += 1;
                if depth > l.max_depth {
                    return Err(too_large());
                }
                if depth == 1 {
                    roots += 1;
                    if roots != 1 || name.local != "ClinicalDocument" || name.space != CDA_NAMESPACE
                    {
                        return Err(invalid());
                    }
                }
                if depth == 2 && name.local == "id" && name.space == CDA_NAMESPACE {
                    ids += 1;
                    if ids > 1 {
                        return Err(invalid());
                    }
                    let (root, extension) = cda_identifier_attributes(&attrs)?;
                    if !payload_oid_or_uuid(&root) || extension.contains(['^', '\r', '\n', '\0']) {
                        return Err(invalid());
                    }
                    identifier = root;
                    if !extension.is_empty() {
                        identifier.push('^');
                        identifier.push_str(&extension);
                    }
                    if identifier.len() > 1024 {
                        return Err(invalid());
                    }
                }
            }
            Tok::End(_) => depth -= 1,
            Tok::Directive(_) => return Err(invalid()),
            Tok::CharData(v) if depth == 0 && !String::from_utf8_lossy(&v).trim().is_empty() => {
                return Err(invalid());
            }
            _ => {}
        }
    }
    if roots != 1 || depth != 0 || ids != 1 || identifier.is_empty() {
        return Err(Error::sentinel(Kind::MissingHl7InstanceIdentifier));
    }
    Ok(identifier)
}
pub(crate) fn payload_oid_or_uuid(s: &str) -> bool {
    if s.is_empty() || s.trim() != s {
        return false;
    }
    let b = s.as_bytes();
    if s.len() == 36 && b[8] == b'-' {
        for (i, c) in s.char_indices() {
            if i == 8 || i == 13 || i == 18 || i == 23 {
                if c != '-' {
                    return false;
                }
            } else if !c.is_ascii_hexdigit() {
                return false;
            }
        }
        return true;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || s.len() > 64 {
        return false;
    }
    for p in &parts {
        if p.is_empty() || (p.len() > 1 && p.as_bytes()[0] == b'0') {
            return false;
        }
        if !p.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    if parts[0] != "0" && parts[0] != "1" && parts[0] != "2" {
        return false;
    }
    if parts[0] != "2" {
        match parts[1].parse::<u8>() {
            Ok(second) if second <= 39 => {}
            _ => return false,
        }
    }
    true
}
fn payload_numbers(fields: &[&str]) -> bool {
    fields
        .iter()
        .all(|s| matches!(parse_float(s), Ok(v) if v.is_finite()))
}
fn scan_lines(data: &[u8]) -> (Vec<&[u8]>, bool) {
    const MAX: usize = 65536;
    let mut lines = Vec::new();
    let mut start = 0;
    while start < data.len() {
        match data[start..].iter().position(|&b| b == b'\n') {
            Some(i) => {
                if i + 1 > MAX {
                    return (lines, true);
                }
                let mut line = &data[start..start + i];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                lines.push(line);
                start += i + 1;
            }
            None => {
                let rest = &data[start..];
                if rest.len() >= MAX {
                    return (lines, true);
                }
                let mut line = rest;
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                lines.push(line);
                break;
            }
        }
    }
    (lines, false)
}
fn inspect_model_text(data: &[u8], sop: &str, l: &PayloadLimits) -> Result<()> {
    if std::str::from_utf8(data).is_err() || data.contains(&0) {
        return Err(invalid());
    }
    let (lines, too_long) = scan_lines(data);
    let mut records = 0i64;
    let (mut vertices, mut faces, mut materials, mut textures, mut normals) =
        (0i64, 0, 0, 0i64, 0i64);
    let mut state = 0;
    let (mut started, mut ended) = (false, false);
    for raw in lines {
        records += 1;
        if records > l.max_records {
            return Err(too_large());
        }
        let text = std::str::from_utf8(raw).unwrap_or_default();
        let line = text.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        match sop {
            ENCAPSULATED_STL_STORAGE => {
                if !started && f[0] == "solid" {
                    started = true;
                } else if started
                    && !ended
                    && state == 0
                    && f.len() == 5
                    && f[0] == "facet"
                    && f[1] == "normal"
                    && payload_numbers(&f[2..])
                {
                    state = 1;
                } else if state == 1 && line == "outer loop" {
                    state = 2;
                    vertices = 0;
                } else if state == 2 && f.len() == 4 && f[0] == "vertex" && payload_numbers(&f[1..])
                {
                    vertices += 1;
                    if vertices > 3 {
                        return Err(invalid());
                    }
                } else if state == 2 && vertices == 3 && line == "endloop" {
                    state = 3;
                } else if state == 3 && line == "endfacet" {
                    state = 0;
                    faces += 1;
                } else if started && !ended && state == 0 && f[0] == "endsolid" {
                    ended = true;
                } else {
                    return Err(invalid());
                }
            }
            ENCAPSULATED_OBJ_STORAGE => match f[0] {
                "v" => {
                    if (f.len() != 4 && f.len() != 5 && f.len() != 7) || !payload_numbers(&f[1..]) {
                        return Err(invalid());
                    }
                    vertices += 1;
                }
                "vn" => {
                    if f.len() != 4 || !payload_numbers(&f[1..]) {
                        return Err(invalid());
                    }
                    normals += 1;
                }
                "vt" => {
                    if f.len() < 2 || f.len() > 4 || !payload_numbers(&f[1..]) {
                        return Err(invalid());
                    }
                    textures += 1;
                }
                "f" | "l" | "p" => {
                    if f.len() < 2 || (f[0] == "f" && f.len() < 4) {
                        return Err(invalid());
                    }
                    for index in &f[1..] {
                        let components: Vec<&str> = index.split('/').collect();
                        if components.len() > 3
                            || components[0].is_empty()
                            || (f[0] == "p" && components.len() != 1)
                            || (f[0] == "l" && components.len() > 2)
                        {
                            return Err(invalid());
                        }
                        for (i, value) in components.iter().enumerate() {
                            if value.is_empty() {
                                if i != 1 || components.len() != 3 {
                                    return Err(invalid());
                                }
                                continue;
                            }
                            let count = [vertices, textures, normals][i];
                            match parse_int(value) {
                                Ok(n) if n != 0 && n <= count && n >= -count => {}
                                _ => return Err(invalid()),
                            }
                        }
                    }
                    faces += 1;
                }
                "o" | "g" | "s" | "usemtl" | "mtllib" => {
                    if f.len() < 2 {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            },
            ENCAPSULATED_MTL_STORAGE => match f[0] {
                "newmtl" => {
                    if f.len() < 2 {
                        return Err(invalid());
                    }
                    materials += 1;
                }
                "Ka" | "Kd" | "Ks" | "Ke" | "Tf" => {
                    if materials == 0 || f.len() != 4 || !payload_numbers(&f[1..]) {
                        return Err(invalid());
                    }
                }
                "Ns" | "Ni" | "d" | "Tr" | "illum" => {
                    if materials == 0 || f.len() != 2 || !payload_numbers(&f[1..]) {
                        return Err(invalid());
                    }
                }
                "map_Ka" | "map_Kd" | "map_Ks" | "map_Ns" | "map_d" | "bump" | "map_bump"
                | "disp" | "decal" | "norm" => {
                    if materials == 0 || f.len() < 2 {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            },
            _ => {}
        }
    }
    if too_long {
        return Err(invalid());
    }
    match sop {
        ENCAPSULATED_STL_STORAGE if !started || !ended || state != 0 || faces < 1 => Err(invalid()),
        ENCAPSULATED_OBJ_STORAGE if vertices < 1 || faces < 1 => Err(invalid()),
        ENCAPSULATED_MTL_STORAGE if materials < 1 => Err(invalid()),
        _ => Ok(()),
    }
}
