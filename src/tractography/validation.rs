use super::budget::{Budget, operation};
use super::codec::{MANAGED, T_CHARSET, metadata_tag};
use super::urlrules::parses_with_scheme;
use super::{
    Code, Context, DICOM_IMAGES, ImageReference, Limits, OTHER_SOURCE, Result, Results, STORAGE,
    UNKNOWN_ORIGIN, URN_CODE, check, invalid, invalid_reference, limited,
};
use crate::codes;
use crate::dcmtime;
use crate::dicomjson::trim_unicode_space;
use crate::dictionary::uid::{StorageCategory, lookup_storage_sop_class};
use crate::object::{Object, is_valid_uid};
use crate::validation::iod;
use crate::validation::{Mode, ValidationOptions, validate_data_set_with};
use std::collections::{HashMap, HashSet};
pub fn validate(ctx: &Context, r: Option<&Results>, l: Limits) -> Result<()> {
    let mut b = operation(ctx, l)?;
    b.model(r)?;
    let Some(r) = r else {
        return Err(invalid("nil Results"));
    };
    if let Some(metadata) = &r.metadata {
        let m = b.facade(metadata)?;
        metadata_valid(&b, &m)?;
    }
    model_valid(&b, r, false)
}
fn valid_uid(uid: &str) -> bool {
    is_valid_uid(uid.as_bytes())
}
pub(crate) fn text_ok(s: &str, max: i64, long: bool) -> bool {
    if s.chars().count() as i64 > max {
        return false;
    }
    s.chars().all(|c| {
        let code = c as u32;
        !(code == 0
            || code < 32 && !(long && matches!(c, '\n' | '\r' | '\t' | '\u{c}'))
            || c == '\\' && !long)
    })
}
pub(crate) fn required_text(s: &str, max: i64, long: bool) -> bool {
    !trim_unicode_space(s).is_empty() && text_ok(s, max, long)
}
fn cs(s: &str) -> bool {
    s.len() <= 16
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' || c == '_')
}
fn valid_code(c: &Code, depth: i64) -> Result<()> {
    let default_text = Limits::defaults().max_text_bytes;
    if depth > 1
        || c.kind > URN_CODE
        || !required_text(&c.value, default_text, false)
        || !required_text(&c.meaning, 64, false)
    {
        return Err(invalid("code"));
    }
    let runes = c.value.chars().count();
    let urn_like = c.value.contains("://") || c.value.starts_with("urn:");
    match c.kind {
        super::SHORT_CODE => {
            if runes > 16 || urn_like {
                return Err(invalid("short code form"));
            }
        }
        super::LONG_CODE => {
            if runes <= 16 || urn_like {
                return Err(invalid("long code form"));
            }
        }
        _ => {
            if !parses_with_scheme(&c.value) {
                return Err(invalid("URN code form"));
            }
        }
    }
    if c.kind != URN_CODE && !required_text(&c.scheme, 16, false)
        || !text_ok(&c.scheme, 16, false)
        || !text_ok(&c.scheme_version, 16, false)
        || c.scheme.is_empty() && !c.scheme_version.is_empty()
    {
        return Err(invalid("coding scheme"));
    }
    if let Some(x) = &c.context {
        if !cs(&x.identifier)
            || !cs(&x.mapping_resource)
            || !cs(&x.extension_flag)
            || !text_ok(&x.mapping_resource_name, 64, false)
        {
            return Err(invalid("code context text"));
        }
        if !x.identifier.is_empty() && (x.mapping_resource.is_empty() || x.version.is_empty()) {
            return Err(invalid("code context definition"));
        }
        if !x.extension_flag.is_empty() && x.extension_flag != "N" && x.extension_flag != "Y" {
            return Err(invalid("extension flag"));
        }
        if x.extension_flag == "Y"
            && (x.local_version.is_empty() || x.extension_creator_uid.is_empty())
        {
            return Err(invalid("extension identity"));
        }
        for u in [&x.uid, &x.mapping_resource_uid, &x.extension_creator_uid] {
            if !u.is_empty() && !valid_uid(u) {
                return Err(invalid("code context UID"));
            }
        }
        for s in [&x.version, &x.local_version] {
            if !s.is_empty() {
                if dcmtime::parse_datetime(s).is_err() {
                    return Err(invalid("code context DT"));
                }
                if x.mapping_resource == "DCMR" && !s.ends_with("+0000") {
                    return Err(invalid("DCMR context version UTC"));
                }
            }
        }
    }
    let equivalent = c.equivalent.as_deref().unwrap_or_default();
    if depth == 1 && !equivalent.is_empty() {
        return Err(invalid("nested equivalent code"));
    }
    for eq in equivalent {
        valid_code(eq, depth + 1)?;
    }
    Ok(())
}
fn code_in(c: &Code, cid: i64) -> Result<()> {
    valid_code(c, 0)?;
    if cid == 82 && c.scheme != "UCUM" {
        return Err(invalid("CID 82 UCUM scheme"));
    }
    let Some(group) = codes::context_group_by_id(cid) else {
        return Err(invalid("missing context group"));
    };
    let (member, decided) = group.contains(&codes::Code {
        value: &c.value,
        scheme: &c.scheme,
        meaning: &c.meaning,
    });
    if decided && !member && !group.extensible {
        return Err(invalid("non-extensible context group"));
    }
    Ok(())
}
fn tm_syntax(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    if n != 2 && n != 4 && n != 6 && !((8..=13).contains(&n) && b[6] == b'.') {
        return false;
    }
    s.char_indices()
        .all(|(i, c)| (i == 6 && c == '.') || c.is_ascii_digit())
}
type Roles<'a> = HashMap<&'a str, &'static str>;
pub(crate) fn model_valid(b: &Budget<'_>, r: &Results, writing: bool) -> Result<()> {
    if r.origin > OTHER_SOURCE
        || writing && r.origin == UNKNOWN_ORIGIN
        || r.origin == DICOM_IMAGES && r.references.is_empty()
        || r.origin == OTHER_SOURCE && !r.references.is_empty()
    {
        return Err(invalid("source origin"));
    }
    let mut roles: Roles<'_> = HashMap::from([(STORAGE, "class")]);
    for (uid, role) in [
        (&r.sop_instance_uid, "instance"),
        (&r.study_instance_uid, "study"),
        (&r.series_instance_uid, "series"),
        (&r.frame_of_reference_uid, "frame"),
    ] {
        if !valid_uid(uid) || roles.contains_key(uid.as_str()) {
            return Err(invalid("result UID roles"));
        }
        roles.insert(uid, role);
    }
    if r.content_date.len() != 8 {
        return Err(invalid("content DA"));
    }
    match dcmtime::parse_date(&r.content_date) {
        Ok(d) if !d.is_nema => {}
        _ => return Err(invalid("content DA")),
    }
    if !tm_syntax(&r.content_time) || dcmtime::parse_time(&r.content_time).is_err() {
        return Err(invalid("content TM"));
    }
    if !cs(&r.label)
        || trim_unicode_space(&r.label).is_empty()
        || !text_ok(&r.description, 64, false)
    {
        return Err(invalid("content text"));
    }
    if r.track_sets.is_empty() {
        return Err(invalid("no track sets"));
    }
    for (i, s) in r.track_sets.iter().enumerate() {
        check(b.ctx)?;
        if s.number != (i + 1) as u32
            || !required_text(&s.label, 64, false)
            || !text_ok(&s.description, b.l.max_text_bytes, true)
        {
            return Err(invalid("track set identity"));
        }
        valid_code(&s.anatomy, 0)?;
        for m in s.anatomy_modifiers.iter().flatten() {
            code_in(m, 244)?;
        }
        code_in(&s.model, 7261)?;
        if let Some(acquisition) = &s.acquisition {
            code_in(acquisition, 7260)?;
        }
        if s.algorithms.is_empty() {
            return Err(invalid("algorithms"));
        }
        for a in &s.algorithms {
            code_in(&a.family, 7262)?;
            if let Some(name_code) = &a.name_code {
                valid_code(name_code, 0)?;
            }
            if !required_text(&a.name, 64, false)
                || !required_text(&a.version, 64, false)
                || !text_ok(&a.parameters, 10240, true)
                || !text_ok(&a.source, 64, false)
            {
                return Err(invalid("algorithm text"));
            }
        }
        if s.tracks.is_empty() {
            return Err(invalid("no tracks"));
        }
        for t in &s.tracks {
            if t.points.len() < 2 {
                return Err(invalid("track point count"));
            }
            if t.point_colors
                .as_ref()
                .is_some_and(|c| c.len() != t.points.len())
            {
                return Err(invalid("point color count"));
            }
            if s.color.is_none()
                && t.color.is_none()
                && t.point_colors.as_ref().is_none_or(Vec::is_empty)
            {
                return Err(invalid("missing color"));
            }
        }
        if s.anatomy_modifiers.as_ref().is_some_and(Vec::is_empty)
            || s.measurements.as_ref().is_some_and(Vec::is_empty)
            || s.track_statistics.as_ref().is_some_and(Vec::is_empty)
            || s.statistics.as_ref().is_some_and(Vec::is_empty)
        {
            return Err(invalid("empty optional sequence"));
        }
        for m in s.measurements.iter().flatten() {
            code_in(&m.quantity, 7263)?;
            code_in(&m.units, 82)?;
            if m.tracks.len() != s.tracks.len() {
                return Err(invalid("measurement track count"));
            }
            for (j, v) in m.tracks.iter().enumerate() {
                if v.values.is_empty() {
                    return Err(invalid("empty values"));
                }
                let points = s.tracks[j].points.len();
                match &v.point_indices {
                    None => {
                        if v.values.len() != points {
                            return Err(invalid("all-point values"));
                        }
                    }
                    Some(indices) => {
                        if v.values.len() != indices.len() || indices.len() > points {
                            return Err(invalid("subset values"));
                        }
                        for (k, &n) in indices.iter().enumerate() {
                            if k % 1024 == 0 {
                                check(b.ctx)?;
                            }
                            if n == 0 || u64::from(n) > points as u64 {
                                return Err(invalid("point index"));
                            }
                        }
                    }
                }
            }
        }
        for v in s.track_statistics.iter().flatten() {
            for (c, cid) in [(&v.quantity, 7263), (&v.modifier, 7464), (&v.units, 82)] {
                code_in(c, cid)?;
            }
            if v.values.len() != s.tracks.len() {
                return Err(invalid("track statistic count"));
            }
        }
        for v in s.statistics.iter().flatten() {
            for (c, cid) in [(&v.quantity, 7263), (&v.modifier, 7464), (&v.units, 82)] {
                code_in(c, cid)?;
            }
        }
    }
    canonical_references(b.ctx, r, Some(roles)).map(|_| ())
}
fn image_class(u: &str) -> bool {
    lookup_storage_sop_class(u).is_some_and(|s| {
        s.category == StorageCategory::Image
            || s.category == StorageCategory::Segmentation
            || s.name == "Surface Segmentation Storage"
            || s.name == "Parametric Map Storage"
    })
}
pub(crate) fn canonical_references<'a>(
    ctx: &Context,
    r: &'a Results,
    roles: Option<Roles<'a>>,
) -> Result<Vec<ImageReference>> {
    let mut roles = roles.unwrap_or_else(|| {
        let mut m: Roles<'a> = HashMap::new();
        m.insert(STORAGE, "class");
        m.insert(&r.sop_instance_uid, "instance");
        m.insert(&r.study_instance_uid, "study");
        m.insert(&r.series_instance_uid, "series");
        m.insert(&r.frame_of_reference_uid, "frame");
        m
    });
    let mut by_instance: HashMap<&str, &ImageReference> = HashMap::new();
    let mut series_study: HashMap<&str, &str> = HashMap::new();
    let mut out = Vec::with_capacity(r.references.len());
    for v in &r.references {
        check(ctx)?;
        if !image_class(&v.sop_class_uid) || v.sop_instance_uid == r.sop_instance_uid {
            return Err(invalid_reference());
        }
        for (u, role) in [
            (&v.sop_class_uid, "class"),
            (&v.sop_instance_uid, "instance"),
            (&v.study_instance_uid, "study"),
            (&v.series_instance_uid, "series"),
        ] {
            if !valid_uid(u) || roles.get(u.as_str()).is_some_and(|known| *known != role) {
                return Err(invalid_reference());
            }
            roles.insert(u, role);
        }
        if let Some(known) = series_study.get(v.series_instance_uid.as_str())
            && !known.is_empty()
            && *known != v.study_instance_uid
        {
            return Err(invalid_reference());
        }
        series_study.insert(&v.series_instance_uid, &v.study_instance_uid);
        if v.frames.as_ref().is_some_and(Vec::is_empty)
            || v.segments.as_ref().is_some_and(Vec::is_empty)
        {
            return Err(invalid_reference());
        }
        let mut frames = HashSet::new();
        for (i, &f) in v.frames.iter().flatten().enumerate() {
            if i % 1024 == 0 {
                check(ctx)?;
            }
            if f > 1
                && matches!(
                    v.sop_class_uid.as_str(),
                    "1.2.840.10008.5.1.4.1.1.2"
                        | "1.2.840.10008.5.1.4.1.1.4"
                        | "1.2.840.10008.5.1.4.1.1.7"
                )
            {
                return Err(invalid_reference());
            }
            if f <= 0 || !frames.insert(f) {
                return Err(invalid_reference());
            }
        }
        let segment_list = v.segments.as_deref().unwrap_or_default();
        if !segment_list.is_empty() {
            let name = lookup_storage_sop_class(&v.sop_class_uid).map_or("", |c| c.name);
            if !name.contains("Segmentation") {
                return Err(invalid_reference());
            }
            let mut segments = HashSet::new();
            for (i, &s) in segment_list.iter().enumerate() {
                if i % 1024 == 0 {
                    check(ctx)?;
                }
                if s == 0 || !segments.insert(s) {
                    return Err(invalid_reference());
                }
            }
        }
        if let Some(old) = by_instance.get(v.sop_instance_uid.as_str()) {
            if *old != v {
                return Err(invalid_reference());
            }
            continue;
        }
        by_instance.insert(&v.sop_instance_uid, v);
        out.push(v.clone());
    }
    if let Some(p) = &r.performed_procedure_step
        && (p.sop_class_uid != "1.2.840.10008.3.1.2.3.3"
            || !valid_uid(&p.sop_instance_uid)
            || roles.contains_key(p.sop_instance_uid.as_str())
            || roles
                .get(p.sop_class_uid.as_str())
                .is_some_and(|role| *role != "class"))
    {
        return Err(invalid_reference());
    }
    Ok(out)
}
pub(crate) fn metadata_valid(b: &Budget<'_>, m: &Object) -> Result<()> {
    for e in m.elements() {
        if e.tag() == T_CHARSET {
            continue;
        }
        if MANAGED.contains(&e.tag()) {
            return Err(invalid("metadata overrides modeled attribute"));
        }
        if !metadata_tag(e.tag()) {
            return Err(invalid("metadata outside IOD"));
        }
    }
    let (result, err) = validate_data_set_with(
        &m.to_data_set(),
        &ValidationOptions {
            mode: Mode::Strict,
            dictionary: true,
            max_depth: b.l.max_sequence_depth,
            max_elements: b.l.max_elements,
            max_findings: 1024,
            stop_first: true,
            big_endian: m.value_big_endian(),
            ..ValidationOptions::default()
        },
    );
    check(b.ctx)?;
    if err.is_some() || result.report.has_errors() {
        return Err(invalid("metadata encoding"));
    }
    Ok(())
}
pub(crate) fn check_iod(o: &Object, b: &Budget<'_>) -> Result<()> {
    let (report, err) = iod::validate(
        o,
        &iod::Options {
            check_values: true,
            max_findings: b.l.max_elements * 4 + 128,
            ..iod::Options::default()
        },
    );
    if err.is_some() {
        return Err(invalid("IOD"));
    }
    for f in &report.findings {
        if [
            iod::MISSING_TYPE1,
            iod::EMPTY_TYPE1,
            iod::MISSING_TYPE2,
            iod::MISSING_TYPE1C,
            iod::EMPTY_TYPE1C,
            iod::MISSING_TYPE2C,
        ]
        .contains(&f.kind)
        {
            return Err(invalid("present IOD module attributes"));
        }
    }
    if !report.errors().is_empty() {
        return Err(invalid("mandatory IOD attributes"));
    }
    if report.truncated > 0 {
        return Err(limited("IOD findings"));
    }
    check(b.ctx)
}
