#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::TAG_SPECIFIC_CHARACTER_SET;
use super::elem::{
    non_ascii, required_uid, string_with_vr, text_element, valid_uid, value_element,
};
use super::error::{Error, ErrorKind};
use super::fileset::FileRecord;
use super::keys::{KeyBudget, KeyRule, key_tag, non_empty_directory_key, own_directory_element};
use crate::dcmtime::{self, Datetime, PrecisionLevel, ZonedTime};
use crate::dictionary::uid;
use crate::object::{DataSet, Element, Object, Tag, Value, Vr};
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub(crate) fn directory_key_condition(
    src: &Object,
    r: &FileRecord,
    rule: &KeyRule,
) -> (bool, bool) {
    if rule.kind == "1" {
        return (true, true);
    }
    if rule.kind == "2" {
        return (false, true);
    }
    let typ = r.record_type.as_str();
    let mut required = src.has(rule.tag);
    if typ == "SR DOCUMENT" && rule.tag == key_tag(0x0040, 0xA030) {
        required = src.get_string(key_tag(0x0040, 0xA493)).unwrap_or_default() == "VERIFIED";
    } else if (typ == "SR DOCUMENT" || typ == "KEY OBJECT DOC")
        && rule.tag == key_tag(0x0040, 0xA730)
    {
        required = !title_modifier_items(src).is_empty();
    } else if typ == "ENCAP DOC" && rule.tag == key_tag(0x0040, 0xE001) {
        required = r.sop_class_uid == "1.2.840.10008.5.1.4.1.1.104.2";
    } else if rule.tag == key_tag(0x0008, 0x1115) {
        required = presentation_series_required(&r.sop_class_uid)
            || typ == "WF PRESENTATION"
            || src.has(rule.tag);
    }
    (required && rule.kind == "1C", required)
}
pub(crate) fn presentation_series_required(sop: &str) -> bool {
    let keyword = uid::by_uid(sop).map(|e| e.keyword).unwrap_or("");
    matches!(
        keyword,
        "GrayscaleSoftcopyPresentationStateStorage"
            | "ColorSoftcopyPresentationStateStorage"
            | "PseudoColorSoftcopyPresentationStateStorage"
            | "BlendingSoftcopyPresentationStateStorage"
            | "XAXRFGrayscaleSoftcopyPresentationStateStorage"
            | "VariableModalityLUTSoftcopyPresentationStateStorage"
    )
}
fn title_modifier_items(src: &Object) -> Vec<Object> {
    src.get_sequence(key_tag(0x0040, 0xA730))
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            item.get_string(key_tag(0x0040, 0xA010)).unwrap_or_default() == "HAS CONCEPT MOD"
        })
        .collect()
}
pub(crate) fn title_modifier_directory_key(
    src: &Object,
    b: &mut KeyBudget,
) -> Result<Element, Error> {
    let items = title_modifier_items(src);
    b.charge(
        (items.len() as i64).saturating_mul(16).saturating_add(16),
        0,
    )?;
    let mut owned: Vec<DataSet> = Vec::with_capacity(items.len());
    for item in &items {
        let value_type = item.get_string(key_tag(0x0040, 0xA040)).unwrap_or_default();
        if value_type != "TEXT" && value_type != "CODE" {
            return Err(ErrorKind::InvalidRecord.into());
        }
        let mut tags = vec![
            key_tag(0x0040, 0xA010),
            key_tag(0x0040, 0xA040),
            key_tag(0x0040, 0xA043),
        ];
        tags.push(if value_type == "TEXT" {
            key_tag(0x0040, 0xA160)
        } else {
            key_tag(0x0040, 0xA168)
        });
        let mut ds = DataSet::default();
        for tag in tags {
            ds.elements.push(own_directory_element(item, tag, b, 1)?);
        }
        let needs_utf8 = ds
            .elements
            .iter()
            .any(|e| matches!(&e.value, Value::Strings(v) if non_ascii(v)));
        if needs_utf8 {
            b.charge(26, 1)?;
            ds.elements.push(text_element(
                TAG_SPECIFIC_CHARACTER_SET,
                Vr::CS,
                "ISO_IR 192",
            ));
        }
        owned.push(ds);
    }
    Ok(value_element(
        key_tag(0x0040, 0xA730),
        Vr::SQ,
        Value::Sequence(owned),
    ))
}
#[allow(clippy::arithmetic_side_effects)]
fn instant(t: &ZonedTime) -> i128 {
    let (y, m, d) = t.date();
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = i128::from(era) * 146_097 + i128::from(doe);
    let secs = days * 86_400 + i128::from(t.hour() * 3600 + t.minute() * 60 + t.second())
        - i128::from(t.offset());
    secs * 1_000_000_000 + i128::from(t.nanosecond())
}
#[allow(clippy::arithmetic_side_effects)]
fn add(t: &ZonedTime, years: i64, months: i64, days: i64, nanos: i64) -> ZonedTime {
    ZonedTime::from_fields(
        t.year() + years,
        t.month() + months,
        t.day() + days,
        t.hour(),
        t.minute(),
        t.second(),
        t.nanosecond() + nanos,
        t.offset(),
    )
}
fn directory_datetime_end(dt: &Datetime) -> ZonedTime {
    let t = &dt.time;
    match dt.precision {
        PrecisionLevel::YEAR => add(t, 1, 0, 0, 0),
        PrecisionLevel::MONTH => add(t, 0, 1, 0, 0),
        PrecisionLevel::DAY => add(t, 0, 0, 1, 0),
        PrecisionLevel::HOURS => add(t, 0, 0, 0, 3_600_000_000_000),
        PrecisionLevel::MINUTES => add(t, 0, 0, 0, 60_000_000_000),
        PrecisionLevel::SECONDS => add(t, 0, 0, 0, 1_000_000_000),
        PrecisionLevel::MS1 => add(t, 0, 0, 0, 100_000_000),
        PrecisionLevel::MS2 => add(t, 0, 0, 0, 10_000_000),
        PrecisionLevel::MS3 => add(t, 0, 0, 0, 1_000_000),
        PrecisionLevel::MS4 => add(t, 0, 0, 0, 100_000),
        PrecisionLevel::MS5 => add(t, 0, 0, 0, 10_000),
        _ => add(t, 0, 0, 0, 1_000),
    }
}
pub(crate) fn verification_directory_key(
    src: &Object,
    b: &mut KeyBudget,
) -> Result<Element, Error> {
    let items = src
        .get_sequence(key_tag(0x0040, 0xA073))
        .unwrap_or_default();
    if items.is_empty() {
        return Err(ErrorKind::MissingRequiredKey.into());
    }
    let zone = src.get_string(key_tag(0x0008, 0x0201)).unwrap_or_default();
    let mut latest: Option<Datetime> = None;
    let mut latest_text = String::new();
    let mut offset_mode: Option<bool> = None;
    for item in &items {
        let mut value = match item.get_string(key_tag(0x0040, 0xA030)) {
            Some(v) if !v.is_empty() => v,
            _ => return Err(ErrorKind::MissingRequiredKey.into()),
        };
        let Ok(mut parsed) = dcmtime::parse_datetime(&value) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        if parsed.no_offset && !zone.is_empty() {
            value.push_str(&zone);
            match dcmtime::parse_datetime(&value) {
                Ok(p) => parsed = p,
                Err(_) => return Err(ErrorKind::InvalidRecord.into()),
            }
        }
        if offset_mode.is_some_and(|m| m != parsed.no_offset) {
            return Err(ErrorKind::InvalidRecord.into());
        }
        offset_mode = Some(parsed.no_offset);
        let Some(current) = latest.as_ref().filter(|_| !latest_text.is_empty()) else {
            latest = Some(parsed);
            latest_text = value;
            continue;
        };
        let end = instant(&directory_datetime_end(&parsed));
        let latest_end = instant(&directory_datetime_end(current));
        let start = instant(&parsed.time);
        let latest_start = instant(&current.time);
        if start >= latest_end {
            latest = Some(parsed);
            latest_text = value;
        } else if latest_start >= end {
        } else if start != latest_start || end != latest_end {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    b.charge((latest_text.len() as i64).saturating_add(16), 0)?;
    Ok(text_element(key_tag(0x0040, 0xA030), Vr::DT, &latest_text))
}
fn require_directory_key(obj: &Object, tag: Tag, vr: Vr) -> Result<(), Error> {
    match obj.get(tag) {
        Some(e) if e.vr() == vr && non_empty_directory_key(e) => Ok(()),
        _ => Err(ErrorKind::MissingRequiredKey.into()),
    }
}
pub(crate) fn directory_sequence(
    obj: &Object,
    tag: Tag,
    min: usize,
    max: i64,
) -> Result<Vec<Object>, Error> {
    match obj.get(tag) {
        Some(e) if e.vr() == Vr::SQ => {}
        _ => return Err(ErrorKind::InvalidRecord.into()),
    }
    match obj.get_sequence(tag) {
        Some(items) if items.len() >= min && (max < 0 || items.len() as i64 <= max) => Ok(items),
        _ => Err(ErrorKind::InvalidRecord.into()),
    }
}
fn validate_code_directory_sequence(
    obj: &Object,
    tag: Tag,
    min: usize,
    max: i64,
) -> Result<(), Error> {
    for item in &directory_sequence(obj, tag, min, max)? {
        validate_code_directory_item(item)?;
    }
    Ok(())
}
fn is_absolute_uri(value: &str) -> bool {
    if value.bytes().any(|c| c < 0x20 || c == 0x7F) {
        return false;
    }
    let Some(colon) = value.find(':') else {
        return false;
    };
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic()
        || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return false;
    }
    url_parse_ok(value)
}
fn url_parse_ok(value: &str) -> bool {
    let rest = match value.find('#') {
        Some(i) => {
            let (before, fragment) = value.split_at(i);
            if !valid_escapes(fragment.get(1..).unwrap_or_default()) {
                return false;
            }
            before
        }
        None => value,
    };
    let mut rest = match rest.split_once(':') {
        Some((_, after)) => after,
        None => rest.get(1..).unwrap_or_default(),
    };
    if let Some((before, _)) = rest.split_once('?') {
        rest = before;
    }
    if let Some(after) = rest.strip_prefix("//") {
        let (authority, path) = after.split_at(after.find('/').unwrap_or(after.len()));
        let host = match authority.rsplit_once('@') {
            Some((_, host)) => host,
            None => authority,
        };
        if !valid_host(host) {
            return false;
        }
        return valid_escapes(path);
    }
    valid_escapes(rest)
}
fn valid_escapes(s: &str) -> bool {
    let mut b = s.as_bytes();
    while let Some((&c, rest)) = b.split_first() {
        b = rest;
        if c == b'%' {
            match b {
                [h, l, tail @ ..] if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => b = tail,
                _ => return false,
            }
        }
    }
    true
}
fn valid_host(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[') {
        return inner.contains(']');
    }
    let host = match host.rsplit_once(':') {
        Some((name, port)) => {
            if !port.bytes().all(|c| c.is_ascii_digit()) {
                return false;
            }
            name
        }
        None => host,
    };
    host.bytes().all(|c| {
        c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:%\"<>[]^`{|}".contains(&c) || c >= 0x80
    }) && valid_escapes(host)
}
fn validate_code_directory_item(item: &Object) -> Result<(), Error> {
    require_directory_key(item, key_tag(0x0008, 0x0104), Vr::LO)?;
    let value_tags = [
        key_tag(0x0008, 0x0100),
        key_tag(0x0008, 0x0119),
        key_tag(0x0008, 0x0120),
    ];
    let mut count = 0u32;
    for tag in value_tags {
        if let Some(e) = item.get(tag) {
            if !non_empty_directory_key(e) {
                return Err(ErrorKind::InvalidRecord.into());
            }
            count = count.saturating_add(1);
        }
    }
    if count != 1 {
        return Err(ErrorKind::InvalidRecord.into());
    }
    for tag in value_tags {
        if !item.has(tag) {
            continue;
        }
        let value = item.get_string(tag).unwrap_or_default();
        let uri = is_absolute_uri(&value);
        if tag == key_tag(0x0008, 0x0120) {
            if !uri {
                return Err(ErrorKind::InvalidRecord.into());
            }
        } else if uri || tag == key_tag(0x0008, 0x0119) && value.chars().count() <= 16 {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    if item.has(key_tag(0x0008, 0x0100)) || item.has(key_tag(0x0008, 0x0119)) {
        require_directory_key(item, key_tag(0x0008, 0x0102), Vr::SH)?;
    } else if item.has(key_tag(0x0008, 0x0103)) && !item.has(key_tag(0x0008, 0x0102)) {
        return Err(ErrorKind::InvalidRecord.into());
    }
    if item.has(key_tag(0x0008, 0x010F)) {
        for tag in [key_tag(0x0008, 0x0105), key_tag(0x0008, 0x0106)] {
            match item.get(tag) {
                Some(e) if non_empty_directory_key(e) => {}
                _ => return Err(ErrorKind::MissingRequiredKey.into()),
            }
        }
    }
    let extension = item.get_string(key_tag(0x0008, 0x010B)).unwrap_or_default();
    if !extension.is_empty() && extension != "Y" && extension != "N" {
        return Err(ErrorKind::InvalidRecord.into());
    }
    if extension == "Y" {
        for tag in [key_tag(0x0008, 0x0107), key_tag(0x0008, 0x010D)] {
            match item.get(tag) {
                Some(e) if non_empty_directory_key(e) => {}
                _ => return Err(ErrorKind::MissingRequiredKey.into()),
            }
        }
    }
    if item.has(key_tag(0x0008, 0x0121)) {
        validate_code_directory_sequence(item, key_tag(0x0008, 0x0121), 1, -1)?;
    }
    Ok(())
}
fn validate_sop_directory_sequence(obj: &Object, tag: Tag, class: &str) -> Result<(), Error> {
    for item in &directory_sequence(obj, tag, 1, -1)? {
        let (c, ok) = required_uid(item, key_tag(0x0008, 0x1150));
        if !ok || !class.is_empty() && c != class {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if !required_uid(item, key_tag(0x0008, 0x1155)).1 {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    Ok(())
}
fn validate_presentation_directory_series(obj: &Object, waveform: bool) -> Result<(), Error> {
    for item in &directory_sequence(obj, key_tag(0x0008, 0x1115), 1, -1)? {
        if !required_uid(item, TAG_SERIES_INSTANCE_UID).1 {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if !waveform {
            validate_sop_directory_sequence(item, key_tag(0x0008, 0x1140), "")?;
            continue;
        }
        let instances = item.has(key_tag(0x0008, 0x114A));
        let waves = item.has(key_tag(0x0008, 0x113A));
        if !instances && !waves {
            return Err(ErrorKind::MissingRequiredKey.into());
        }
        if instances {
            validate_sop_directory_sequence(
                item,
                key_tag(0x0008, 0x114A),
                "1.2.840.10008.5.1.4.1.1.88.77",
            )?;
        }
        if waves {
            validate_sop_directory_sequence(item, key_tag(0x0008, 0x113A), "")?;
            let referenced = item
                .get_sequence(key_tag(0x0008, 0x113A))
                .unwrap_or_default();
            let mut class = String::new();
            for wave in &referenced {
                let current = wave.get_uid(key_tag(0x0008, 0x1150)).unwrap_or_default();
                if !class.is_empty() && class != current {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                class = current;
            }
        }
    }
    Ok(())
}
pub(crate) fn validate_directory_key_sequences(
    obj: &Object,
    typ: &str,
    sop: &str,
) -> Result<(), Error> {
    let concept = key_tag(0x0040, 0xA043);
    if obj.has(concept) {
        let min = if typ == "ENCAP DOC" { 0 } else { 1 };
        validate_code_directory_sequence(obj, concept, min, 1)?;
    }
    match typ {
        "SR DOCUMENT" | "KEY OBJECT DOC" => {
            if typ == "SR DOCUMENT" {
                let completion = obj.get_string(key_tag(0x0040, 0xA491)).unwrap_or_default();
                let verification = obj.get_string(key_tag(0x0040, 0xA493)).unwrap_or_default();
                if completion != "COMPLETE" && completion != "PARTIAL"
                    || verification != "VERIFIED" && verification != "UNVERIFIED"
                {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                if verification == "VERIFIED" {
                    if completion != "COMPLETE" {
                        return Err(ErrorKind::InvalidRecord.into());
                    }
                    require_directory_key(obj, key_tag(0x0040, 0xA030), Vr::DT)?;
                    let value = obj.get_string(key_tag(0x0040, 0xA030)).unwrap_or_default();
                    if dcmtime::parse_datetime(&value).is_err() {
                        return Err(ErrorKind::InvalidRecord.into());
                    }
                }
            }
            if obj.has(key_tag(0x0040, 0xA730)) {
                for item in &directory_sequence(obj, key_tag(0x0040, 0xA730), 1, -1)? {
                    let rel = item.get_string(key_tag(0x0040, 0xA010)).unwrap_or_default();
                    let value_type = item.get_string(key_tag(0x0040, 0xA040)).unwrap_or_default();
                    if rel != "HAS CONCEPT MOD" || item.has(key_tag(0x0040, 0xA730)) {
                        return Err(ErrorKind::InvalidRecord.into());
                    }
                    validate_code_directory_sequence(item, concept, 1, 1)?;
                    match value_type.as_str() {
                        "TEXT" => require_directory_key(item, key_tag(0x0040, 0xA160), Vr::UT)?,
                        "CODE" => {
                            validate_code_directory_sequence(item, key_tag(0x0040, 0xA168), 1, 1)?
                        }
                        _ => return Err(ErrorKind::InvalidRecord.into()),
                    }
                }
            }
        }
        "PRESENTATION" | "WF PRESENTATION" => {
            let wf = typ == "WF PRESENTATION";
            if presentation_series_required(sop) || wf || obj.has(key_tag(0x0008, 0x1115)) {
                validate_presentation_directory_series(obj, wf)?;
            }
            if obj.has(key_tag(0x0070, 0x0402)) {
                for item in &directory_sequence(obj, key_tag(0x0070, 0x0402), 2, 2)? {
                    if !required_uid(item, TAG_STUDY_INSTANCE_UID).1 {
                        return Err(ErrorKind::InvalidRecord.into());
                    }
                    directory_sequence(item, key_tag(0x0008, 0x1115), 1, 1)?;
                    validate_presentation_directory_series(item, false)?;
                }
            }
        }
        "SPECTROSCOPY" => {
            if obj.has(key_tag(0x0008, 0x9092)) {
                return validate_hierarchical_directory_references(obj, key_tag(0x0008, 0x9092));
            }
        }
        "ENCAP DOC" => {
            if sop == "1.2.840.10008.5.1.4.1.1.104.2" {
                require_directory_key(obj, key_tag(0x0040, 0xE001), Vr::ST)?;
            }
        }
        "HANGING PROTOCOL" => return validate_hanging_directory_keys(obj),
        "IMPLANT ASSY" => {
            return validate_code_directory_sequence(obj, key_tag(0x0076, 0x0020), 1, -1);
        }
        "INVENTORY" => return validate_inventory_scope(obj),
        _ => {}
    }
    Ok(())
}
fn validate_hanging_directory_keys(obj: &Object) -> Result<(), Error> {
    for item in &directory_sequence(obj, key_tag(0x0072, 0x000C), 1, -1)? {
        let modality = item.has(TAG_MODALITY);
        let region = item.has(key_tag(0x0008, 0x2218));
        if !modality && !region {
            return Err(ErrorKind::MissingRequiredKey.into());
        }
        if modality {
            require_directory_key(item, TAG_MODALITY, Vr::CS)?;
        }
        if region {
            validate_code_directory_sequence(item, key_tag(0x0008, 0x2218), 1, -1)?;
            if string_with_vr(item, key_tag(0x0020, 0x0060), Vr::CS).is_none() {
                return Err(ErrorKind::MissingRequiredKey.into());
            }
        }
        for tag in [key_tag(0x0008, 0x1032), key_tag(0x0040, 0x100A)] {
            validate_code_directory_sequence(item, tag, 0, -1)?;
        }
    }
    validate_code_directory_sequence(obj, key_tag(0x0072, 0x000E), 0, 1)
}
fn validate_inventory_scope(obj: &Object) -> Result<(), Error> {
    let items = directory_sequence(obj, key_tag(0x0008, 0x0400), 0, 1)?;
    let Some(item) = items.first() else {
        return Ok(());
    };
    let mechanisms = item
        .get_strings(key_tag(0x0008, 0x040F))
        .unwrap_or_default();
    let has = |v: &str| mechanisms.iter().any(|m| m == v);
    for m in &mechanisms {
        if !matches!(
            m.as_str(),
            "RELATIONAL"
                | "COMBINED_DT"
                | "FUZZY_SEMANTIC"
                | "TIMEZONE_ADJUST"
                | "EMPTY_VALUE"
                | "MULTIPLE_VALUE"
        ) {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    if has("RELATIONAL") {
        match item.get_string(key_tag(0x0008, 0x0052)) {
            Some(level) if level == "STUDY" || level == "SERIES" || level == "IMAGE" => {}
            _ => return Err(ErrorKind::InvalidRecord.into()),
        }
    }
    if has("TIMEZONE_ADJUST") {
        let zone = item.get_string(key_tag(0x0008, 0x0201)).unwrap_or_default();
        let zb = zone.as_bytes();
        if zb.len() != 5 || !matches!(zb.first(), Some(b'+' | b'-')) {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if dcmtime::parse_datetime(&format!("20000101000000{zone}")).is_err() {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    for tag in [
        key_tag(0x0008, 0x0410),
        key_tag(0x0008, 0x0411),
        key_tag(0x0008, 0x0412),
        key_tag(0x0008, 0x0413),
    ] {
        if !item.has(tag) {
            continue;
        }
        let (mut min, mut max) = (1usize, 1i64);
        if tag == key_tag(0x0008, 0x0410) {
            min = 2;
            max = 2;
        }
        if tag == key_tag(0x0008, 0x0411) {
            max = -1;
        }
        let entries = directory_sequence(item, tag, min, max)?;
        for entry in &entries {
            let mut count = 0u32;
            for e in entry.elements() {
                if e.tag() == TAG_SPECIFIC_CHARACTER_SET {
                    continue;
                }
                count = count.saturating_add(1);
                if tag == key_tag(0x0008, 0x0410) && !matches!(e.vr(), Vr::DA | Vr::DT | Vr::TM) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                if tag == key_tag(0x0008, 0x0411) {
                    let values = entry.get_strings(e.tag());
                    let ok = match &values {
                        Some(v) => match v.as_slice() {
                            [only] => valid_uid(crate::object::text::normalize_uid(only)),
                            _ => false,
                        },
                        None => false,
                    };
                    if e.vr() != Vr::UI || !ok {
                        return Err(ErrorKind::InvalidRecord.into());
                    }
                }
                if tag == key_tag(0x0008, 0x0412) && non_empty_directory_key(e) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                if tag != key_tag(0x0008, 0x0412)
                    && tag != key_tag(0x0008, 0x0410)
                    && !non_empty_directory_key(e)
                {
                    return Err(ErrorKind::InvalidRecord.into());
                }
            }
            if count == 0 || tag == key_tag(0x0008, 0x0411) && count != 1 {
                return Err(ErrorKind::InvalidRecord.into());
            }
            if tag == key_tag(0x0008, 0x0413) {
                for e in entry.elements() {
                    if e.vr().is_string_like() {
                        let values = entry.get_strings(e.tag()).unwrap_or_default();
                        if values.iter().any(|v| v == "*") {
                            return Err(ErrorKind::InvalidRecord.into());
                        }
                    }
                }
            }
        }
        if tag == key_tag(0x0008, 0x0412) && !has("EMPTY_VALUE") {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if tag == key_tag(0x0008, 0x0410) {
            let [a, b] = entries.as_slice() else {
                return Err(ErrorKind::InvalidRecord.into());
            };
            for e in a.elements() {
                if e.tag() == TAG_SPECIFIC_CHARACTER_SET {
                    continue;
                }
                match b.get(e.tag()) {
                    Some(other)
                        if other.vr() == e.vr()
                            && (non_empty_directory_key(e) || non_empty_directory_key(other)) => {}
                    _ => return Err(ErrorKind::InvalidRecord.into()),
                }
            }
            if a.elements().len() != b.elements().len() {
                return Err(ErrorKind::InvalidRecord.into());
            }
        }
    }
    Ok(())
}
pub(crate) fn normalize_directory_key_sequences(
    typ: &str,
    mut keys: DataSet,
    b: &mut KeyBudget,
) -> Result<DataSet, Error> {
    if typ != "HANGING PROTOCOL" {
        return Ok(keys);
    }
    for e in keys.elements.iter_mut() {
        if e.tag() != key_tag(0x0072, 0x000C) {
            continue;
        }
        let Value::Sequence(items) = &mut e.value else {
            continue;
        };
        for item in items.iter_mut() {
            let view = Object::from_data_set(item.clone());
            for tag in [key_tag(0x0008, 0x1032), key_tag(0x0040, 0x100A)] {
                if !view.has(tag) {
                    b.charge(16, 1)?;
                    item.elements
                        .push(value_element(tag, Vr::SQ, Value::Sequence(Vec::new())));
                }
            }
            if view.has(key_tag(0x0008, 0x2218)) && !view.has(key_tag(0x0020, 0x0060)) {
                b.charge(16, 1)?;
                item.elements
                    .push(text_element(key_tag(0x0020, 0x0060), Vr::CS, ""));
            }
        }
    }
    Ok(keys)
}
fn validate_hierarchical_directory_references(obj: &Object, tag: Tag) -> Result<(), Error> {
    for study in &directory_sequence(obj, tag, 1, -1)? {
        if !required_uid(study, TAG_STUDY_INSTANCE_UID).1 {
            return Err(ErrorKind::InvalidRecord.into());
        }
        for item in &directory_sequence(study, key_tag(0x0008, 0x1115), 1, -1)? {
            if !required_uid(item, TAG_SERIES_INSTANCE_UID).1 {
                return Err(ErrorKind::InvalidRecord.into());
            }
            validate_sop_directory_sequence(item, key_tag(0x0008, 0x1199), "")?;
            for reference in &item
                .get_sequence(key_tag(0x0008, 0x1199))
                .unwrap_or_default()
            {
                if reference.has(key_tag(0x0040, 0xA170)) {
                    validate_code_directory_sequence(reference, key_tag(0x0040, 0xA170), 1, -1)?;
                }
            }
        }
    }
    Ok(())
}
