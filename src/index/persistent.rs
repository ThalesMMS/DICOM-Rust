use super::{
    DEFAULT_MAX_DIAGNOSTICS, Diagnostic, DiagnosticCode, FileMeta, Geometry, Instance,
    MissingField, Patient, Record, Series, Severity, SourceInfo, Study,
};
use crate::dicomjson::jsonwire::{self, Decoder, JsonError, Token};
use crate::dicomjson::sizedparse;
use crate::net::ul::Ctx;
use crate::object::{Tag, is_valid_uid};
use crate::recordstore;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
pub const ERR_PERSISTENT_INDEX: &str =
    "dicom index: persistent metadata is inconsistent or unavailable";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistentError {
    Inconsistent,
    ResourceLimit,
    Canceled,
    DeadlineExceeded,
}
impl std::fmt::Display for PersistentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PersistentError::Inconsistent => ERR_PERSISTENT_INDEX,
            PersistentError::ResourceLimit => super::Sentinel::ResourceLimit.text(),
            PersistentError::Canceled => "context canceled",
            PersistentError::DeadlineExceeded => "context deadline exceeded",
        })
    }
}
impl std::error::Error for PersistentError {}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersistentRecord {
    pub record: Record,
    pub attributes: BTreeMap<String, String>,
    pub file_bytes: i64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PersistentStoreOptions {
    pub max_records: i64,
    pub max_record_bytes: i64,
    pub max_total_bytes: i64,
}
#[derive(Debug)]
pub struct PersistentStore {
    store: recordstore::Store,
    max_record_bytes: i64,
}
fn persistent_error(err: recordstore::Error) -> PersistentError {
    match err {
        recordstore::Error::Canceled => PersistentError::Canceled,
        recordstore::Error::DeadlineExceeded => PersistentError::DeadlineExceeded,
        recordstore::Error::Limit => PersistentError::ResourceLimit,
        _ => PersistentError::Inconsistent,
    }
}
#[doc(hidden)]
pub fn persistent_key(uid: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(uid.as_bytes()).into();
    let mut s = String::with_capacity(69);
    for b in digest {
        s.push(char::from(b"0123456789abcdef"[(b >> 4) as usize]));
        s.push(char::from(b"0123456789abcdef"[(b & 15) as usize]));
    }
    s.push_str(".json");
    s
}
impl PersistentStore {
    pub fn open(
        ctx: Option<&Ctx>,
        dir: impl AsRef<std::path::Path>,
        mut opts: PersistentStoreOptions,
    ) -> Result<PersistentStore, PersistentError> {
        if opts.max_records == 0 {
            opts.max_records = 10_000;
        }
        if opts.max_record_bytes == 0 {
            opts.max_record_bytes = 256 << 10;
        }
        if opts.max_total_bytes == 0 {
            opts.max_total_bytes = 256 << 20;
        }
        let store = recordstore::Store::open(
            ctx,
            dir,
            recordstore::Options {
                max_records: opts.max_records,
                max_record_bytes: opts.max_record_bytes,
                max_total_bytes: opts.max_total_bytes,
            },
        )
        .map_err(persistent_error)?;
        let p = PersistentStore {
            store,
            max_record_bytes: opts.max_record_bytes,
        };
        if let Err(e) = p.records(ctx) {
            let _ = p.store.close();
            return Err(e);
        }
        Ok(p)
    }
    pub fn insert(
        &self,
        ctx: Option<&Ctx>,
        value: &PersistentRecord,
    ) -> Result<bool, PersistentError> {
        if !valid_persistent(value) {
            return Err(PersistentError::Inconsistent);
        }
        persistent_admission(value, self.max_record_bytes)?;
        let data = marshal_envelope(value).ok_or(PersistentError::Inconsistent)?;
        self.store
            .create(
                ctx,
                &persistent_key(&value.record.instance.sop_instance_uid),
                data.as_bytes(),
            )
            .map_err(persistent_error)
    }
    pub fn records(&self, ctx: Option<&Ctx>) -> Result<Vec<PersistentRecord>, PersistentError> {
        let keys = self.store.keys(ctx).map_err(persistent_error)?;
        let mut rows = Vec::with_capacity(keys.len());
        for key in keys {
            let data = self.store.read(ctx, &key).map_err(persistent_error)?;
            rows.push(decode_persistent(&key, &data)?);
        }
        Ok(rows)
    }
    pub fn close(&self) -> Result<(), PersistentError> {
        self.store.close().map_err(persistent_error)
    }
}
fn valid_persistent(r: &PersistentRecord) -> bool {
    let v = &r.record;
    if r.file_bytes < 1 || r.sha256 == [0; 32] || !v.selected.is_empty() || r.attributes.len() > 128
    {
        return false;
    }
    for uid in [
        &v.instance.sop_class_uid,
        &v.instance.sop_instance_uid,
        &v.study.instance_uid,
        &v.series.instance_uid,
        &v.file_meta.transfer_syntax_uid,
    ] {
        if !is_valid_uid(uid.as_bytes()) {
            return false;
        }
    }
    if v.file_meta.media_storage_sop_class_uid != v.instance.sop_class_uid
        || v.file_meta.media_storage_sop_instance_uid != v.instance.sop_instance_uid
        || v.source.size != r.file_bytes
    {
        return false;
    }
    r.attributes.iter().all(|(key, value)| {
        key.len() == 8
            && key
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
            && value.len() <= 1024
    })
}
enum J {
    Bool(bool),
    Str(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Obj(Vec<(String, J)>),
    Arr(Vec<J>),
}
struct Fields(Vec<(String, J)>);
impl Fields {
    fn new() -> Fields {
        Fields(Vec::new())
    }
    fn put(mut self, name: &str, v: J) -> Fields {
        self.0.push((name.to_string(), v));
        self
    }
    fn str(self, name: &str, s: &str) -> Fields {
        if s.is_empty() {
            self
        } else {
            self.put(name, J::Str(s.to_string()))
        }
    }
    fn int(self, name: &str, n: i64) -> Fields {
        if n == 0 {
            self
        } else {
            self.put(name, J::I64(n))
        }
    }
    fn floats(self, name: &str, v: &Option<Vec<f64>>) -> Fields {
        match v {
            Some(v) if !v.is_empty() => {
                self.put(name, J::Arr(v.iter().map(|&f| J::F64(f)).collect()))
            }
            _ => self,
        }
    }
    fn obj(self) -> J {
        J::Obj(self.0)
    }
}
fn tag_json(t: Tag) -> J {
    Fields::new()
        .put("Group", J::U64(u64::from(t.group)))
        .put("Element", J::U64(u64::from(t.element)))
        .obj()
}
fn record_json(r: &Record) -> J {
    let mut f = Fields::new()
        .put(
            "source",
            Fields::new()
                .str("origin", &r.source.origin)
                .put("size", J::I64(r.source.size))
                .obj(),
        )
        .put(
            "file_meta",
            Fields::new()
                .str("transfer_syntax_uid", &r.file_meta.transfer_syntax_uid)
                .str("transfer_syntax_name", &r.file_meta.transfer_syntax_name)
                .str(
                    "media_storage_sop_class_uid",
                    &r.file_meta.media_storage_sop_class_uid,
                )
                .str(
                    "media_storage_sop_instance_uid",
                    &r.file_meta.media_storage_sop_instance_uid,
                )
                .obj(),
        );
    if let Some(p) = &r.patient {
        f = f.put(
            "patient",
            Fields::new()
                .str("name", &p.name)
                .str("id", &p.id)
                .str("birth_date", &p.birth_date)
                .str("sex", &p.sex)
                .obj(),
        );
    }
    f = f
        .put(
            "study",
            Fields::new()
                .str("instance_uid", &r.study.instance_uid)
                .str("accession_number", &r.study.accession_number)
                .str("date", &r.study.date)
                .str("time", &r.study.time)
                .str("description", &r.study.description)
                .obj(),
        )
        .put(
            "series",
            Fields::new()
                .str("instance_uid", &r.series.instance_uid)
                .str("modality", &r.series.modality)
                .str("number", &r.series.number)
                .str("date", &r.series.date)
                .str("time", &r.series.time)
                .str("description", &r.series.description)
                .obj(),
        )
        .put(
            "instance",
            Fields::new()
                .str("sop_class_uid", &r.instance.sop_class_uid)
                .str("sop_instance_uid", &r.instance.sop_instance_uid)
                .str("number", &r.instance.number)
                .int("number_of_frames", r.instance.number_of_frames)
                .obj(),
        );
    if let Some(g) = &r.geometry {
        let mut gf = Fields::new()
            .int("rows", g.rows)
            .int("columns", g.columns)
            .floats("pixel_spacing", &g.pixel_spacing)
            .floats("image_position_patient", &g.image_position_patient)
            .floats("image_orientation_patient", &g.image_orientation_patient);
        if g.slice_thickness != 0.0 {
            gf = gf.put("slice_thickness", J::F64(g.slice_thickness));
        }
        f = f.put("geometry", gf.obj());
    }
    if !r.missing.is_empty() {
        let items = r
            .missing
            .iter()
            .map(|m| {
                Fields::new()
                    .put("field", J::Str(m.field.clone()))
                    .put("tag", tag_json(m.tag))
                    .obj()
            })
            .collect();
        f = f.put("missing", J::Arr(items));
    }
    if !r.diagnostics.is_empty() {
        let items = r
            .diagnostics
            .iter()
            .map(|d| {
                let mut df = Fields::new()
                    .put("code", J::Str(d.code.as_str().to_string()))
                    .put("severity", J::Str(d.severity.as_str().to_string()))
                    .str("field", &d.field)
                    .put("tag", tag_json(d.tag))
                    .int("offset", d.offset);
                if d.offset_set {
                    df = df.put("offset_set", J::Bool(true));
                }
                df.put("message", J::Str(d.message.clone())).obj()
            })
            .collect();
        f = f.put("diagnostics", J::Arr(items));
    }
    f.obj()
}
fn envelope_json(value: &PersistentRecord) -> J {
    let mut v = Fields::new().put("record", record_json(&value.record));
    if !value.attributes.is_empty() {
        v = v.put(
            "attributes",
            J::Obj(
                value
                    .attributes
                    .iter()
                    .map(|(k, s)| (k.clone(), J::Str(s.clone())))
                    .collect(),
            ),
        );
    }
    v = v.put("file_bytes", J::I64(value.file_bytes)).put(
        "sha256",
        J::Arr(value.sha256.iter().map(|&b| J::U64(u64::from(b))).collect()),
    );
    Fields::new()
        .put("version", J::I64(1))
        .put("value", v.obj())
        .obj()
}
fn write_json(v: &J, out: &mut String) -> Option<()> {
    match v {
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Str(s) => jsonwire::append_string(out, s),
        J::I64(n) => out.push_str(&n.to_string()),
        J::U64(n) => out.push_str(&n.to_string()),
        J::F64(f) => {
            if !f.is_finite() {
                return None;
            }
            out.push_str(&jsonwire::format_float(*f));
        }
        J::Obj(fields) => {
            out.push('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                jsonwire::append_string(out, k);
                out.push(':');
                write_json(v, out)?;
            }
            out.push('}');
        }
        J::Arr(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(v, out)?;
            }
            out.push(']');
        }
    }
    Some(())
}
fn marshal_envelope(value: &PersistentRecord) -> Option<String> {
    let mut out = String::new();
    write_json(&envelope_json(value), &mut out)?;
    Some(out)
}
fn persistent_admission(value: &PersistentRecord, limit: i64) -> Result<(), PersistentError> {
    let r = &value.record;
    if r.missing.len() as i64 > DEFAULT_MAX_DIAGNOSTICS
        || r.diagnostics.len() as i64 > DEFAULT_MAX_DIAGNOSTICS
    {
        return Err(PersistentError::ResourceLimit);
    }
    if let Some(g) = &r.geometry {
        let len = |v: &Option<Vec<f64>>| v.as_ref().map_or(0, Vec::len);
        if len(&g.pixel_spacing) > 64
            || len(&g.image_position_patient) > 64
            || len(&g.image_orientation_patient) > 64
        {
            return Err(PersistentError::ResourceLimit);
        }
    }
    let mut budget = limit;
    json_admission(&envelope_json(value), &mut budget)
}
fn consume(budget: &mut i64, n: i64) -> Result<(), PersistentError> {
    if n > *budget {
        return Err(PersistentError::ResourceLimit);
    }
    *budget -= n;
    Ok(())
}
fn string_json_size(s: &str) -> i64 {
    let mut n = 2i64;
    for c in s.chars() {
        n += match c {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => 6,
            c if (c as u32) < 32 => 6,
            c => c.len_utf8() as i64,
        };
    }
    n
}
fn json_admission(v: &J, budget: &mut i64) -> Result<(), PersistentError> {
    if *budget < 0 {
        return Err(PersistentError::ResourceLimit);
    }
    match v {
        J::Str(s) => {
            if s.len() as i64 > *budget {
                return Err(PersistentError::ResourceLimit);
            }
            consume(budget, string_json_size(s))
        }
        J::Bool(b) => consume(budget, if *b { 4 } else { 5 }),
        J::I64(n) => consume(budget, n.to_string().len() as i64),
        J::U64(n) => consume(budget, n.to_string().len() as i64),
        J::F64(f) => {
            if !f.is_finite() {
                return Err(PersistentError::Inconsistent);
            }
            consume(budget, jsonwire::format_float(*f).len() as i64)
        }
        J::Obj(fields) => {
            consume(budget, 2)?;
            for (i, (name, child)) in fields.iter().enumerate() {
                let mut n = string_json_size(name);
                if i > 0 {
                    n += 1;
                }
                consume(budget, n + 1)?;
                json_admission(child, budget)?;
            }
            Ok(())
        }
        J::Arr(items) => {
            consume(budget, 2)?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    consume(budget, 1)?;
                }
                json_admission(item, budget)?;
            }
            Ok(())
        }
    }
}
enum Node {
    Obj(Vec<(String, Node)>),
    Arr(Vec<Node>),
    Str(String),
    Lit(Vec<u8>),
}
fn strict_persistent_json(data: &[u8]) -> Option<Node> {
    std::str::from_utf8(data).ok()?;
    let mut d = Decoder::new(data);
    let mut tokens = 0usize;
    let v = strict_value(&mut d, 0, &mut tokens)?;
    match d.token() {
        Err(JsonError::Eof) => Some(v),
        _ => None,
    }
}
fn strict_value(d: &mut Decoder, depth: usize, tokens: &mut usize) -> Option<Node> {
    if depth > 64 || *tokens > 100_000 {
        return None;
    }
    let token = d.token();
    *tokens += 1;
    match token.ok()? {
        Token::Delim(b'{') => {
            let mut seen = HashSet::new();
            let mut members = Vec::new();
            while d.more() {
                let key = d.token();
                *tokens += 1;
                let Ok(Token::Str(name)) = key else {
                    return None;
                };
                if !name.is_ascii() {
                    return None;
                }
                let folded = name.to_ascii_lowercase();
                if seen.contains(&folded) || seen.len() >= 128 {
                    return None;
                }
                seen.insert(folded);
                let v = strict_value(d, depth + 1, tokens)?;
                members.push((name, v));
            }
            match d.token() {
                Ok(Token::Delim(b'}')) => Some(Node::Obj(members)),
                _ => None,
            }
        }
        Token::Delim(b'[') => {
            let mut items = Vec::new();
            while d.more() {
                items.push(strict_value(d, depth + 1, tokens)?);
            }
            match d.token() {
                Ok(Token::Delim(b']')) => Some(Node::Arr(items)),
                _ => None,
            }
        }
        Token::Delim(_) => None,
        Token::Str(s) => Some(Node::Str(s)),
        Token::Scalar(raw) => Some(Node::Lit(raw)),
    }
}
fn is_null(n: &Node) -> bool {
    matches!(n, Node::Lit(raw) if raw == b"null")
}
fn number(n: &Node) -> Option<&str> {
    match n {
        Node::Lit(raw) if matches!(raw.first(), Some(b'-' | b'0'..=b'9')) => {
            std::str::from_utf8(raw).ok()
        }
        _ => None,
    }
}
fn de_string(n: &Node) -> Option<String> {
    match n {
        Node::Str(s) => Some(s.clone()),
        _ if is_null(n) => Some(String::new()),
        _ => None,
    }
}
fn de_int(n: &Node) -> Option<i64> {
    if is_null(n) {
        return Some(0);
    }
    sizedparse::parse_int(number(n)?, 64).ok()
}
fn de_uint(n: &Node, bits: u32) -> Option<u64> {
    if is_null(n) {
        return Some(0);
    }
    sizedparse::parse_uint(number(n)?, bits).ok()
}
fn de_float(n: &Node) -> Option<f64> {
    if is_null(n) {
        return Some(0.0);
    }
    crate::numtext::parse_float(number(n)?)
        .ok()
        .filter(|f| f.is_finite())
}
fn de_bool(n: &Node) -> Option<bool> {
    match n {
        Node::Lit(raw) if raw == b"true" => Some(true),
        Node::Lit(raw) if raw == b"false" || raw == b"null" => Some(false),
        _ => None,
    }
}
fn de_struct<'a, const N: usize>(n: &'a Node, names: [&str; N]) -> Option<[Option<&'a Node>; N]> {
    let mut out = [None; N];
    match n {
        Node::Obj(members) => {
            for (key, value) in members {
                let i = names.iter().position(|f| f.eq_ignore_ascii_case(key))?;
                out[i] = Some(value);
            }
            Some(out)
        }
        _ if is_null(n) => Some(out),
        _ => None,
    }
}
fn field<T: Default>(n: Option<&Node>, f: impl Fn(&Node) -> Option<T>) -> Option<T> {
    match n {
        None => Some(T::default()),
        Some(n) => f(n),
    }
}
fn de_slice<T>(n: &Node, f: impl Fn(&Node) -> Option<T>) -> Option<Option<Vec<T>>> {
    match n {
        Node::Arr(items) => items.iter().map(f).collect::<Option<Vec<T>>>().map(Some),
        _ if is_null(n) => Some(None),
        _ => None,
    }
}
fn de_tag(n: &Node) -> Option<Tag> {
    let [g, e] = de_struct(n, ["Group", "Element"])?;
    Some(Tag::new(
        field(g, |n| de_uint(n, 16))? as u16,
        field(e, |n| de_uint(n, 16))? as u16,
    ))
}
fn de_missing(n: &Node) -> Option<MissingField> {
    let [f, t] = de_struct(n, ["field", "tag"])?;
    Some(MissingField {
        field: field(f, de_string)?,
        tag: field(t, de_tag)?,
    })
}
fn de_diagnostic(n: &Node) -> Option<Diagnostic> {
    let [code, severity, fld, tag, offset, offset_set, message] = de_struct(
        n,
        [
            "code",
            "severity",
            "field",
            "tag",
            "offset",
            "offset_set",
            "message",
        ],
    )?;
    let code = match field(code, de_string)?.as_str() {
        "raw_dataset" => DiagnosticCode::RawDataSet,
        "missing_field" => DiagnosticCode::MissingField,
        "invalid_value" => DiagnosticCode::InvalidValue,
        "resource_limit" => DiagnosticCode::Limit,
        _ => return None,
    };
    let severity = match field(severity, de_string)?.as_str() {
        "info" => Severity::Info,
        "warning" => Severity::Warning,
        "error" => Severity::Error,
        _ => return None,
    };
    Some(Diagnostic {
        code,
        severity,
        field: field(fld, de_string)?,
        tag: field(tag, de_tag)?,
        offset: field(offset, de_int)?,
        offset_set: field(offset_set, de_bool)?,
        message: field(message, de_string)?,
    })
}
fn de_patient(n: &Node) -> Option<Patient> {
    let [name, id, birth, sex] = de_struct(n, ["name", "id", "birth_date", "sex"])?;
    Some(Patient {
        name: field(name, de_string)?,
        id: field(id, de_string)?,
        birth_date: field(birth, de_string)?,
        sex: field(sex, de_string)?,
    })
}
fn de_geometry(n: &Node) -> Option<Geometry> {
    let [rows, columns, ps, ipp, iop, st] = de_struct(
        n,
        [
            "rows",
            "columns",
            "pixel_spacing",
            "image_position_patient",
            "image_orientation_patient",
            "slice_thickness",
        ],
    )?;
    let floats = |n: Option<&Node>| match n {
        None => Some(None),
        Some(n) => de_slice(n, de_float),
    };
    Some(Geometry {
        rows: field(rows, de_int)?,
        columns: field(columns, de_int)?,
        pixel_spacing: floats(ps)?,
        image_position_patient: floats(ipp)?,
        image_orientation_patient: floats(iop)?,
        slice_thickness: field(st, de_float)?,
    })
}
fn de_pointer<T>(n: Option<&Node>, f: impl Fn(&Node) -> Option<T>) -> Option<Option<T>> {
    match n {
        None => Some(None),
        Some(n) if is_null(n) => Some(None),
        Some(n) => f(n).map(Some),
    }
}
fn de_record(n: &Node) -> Option<Record> {
    let [
        source,
        file_meta,
        patient,
        study,
        series,
        instance,
        geometry,
        missing,
        diagnostics,
    ] = de_struct(
        n,
        [
            "source",
            "file_meta",
            "patient",
            "study",
            "series",
            "instance",
            "geometry",
            "missing",
            "diagnostics",
        ],
    )?;
    let source = match source {
        None => SourceInfo::default(),
        Some(n) => {
            let [origin, size] = de_struct(n, ["origin", "size"])?;
            SourceInfo {
                origin: field(origin, de_string)?,
                size: field(size, de_int)?,
            }
        }
    };
    let file_meta = match file_meta {
        None => FileMeta::default(),
        Some(n) => {
            let [ts, tsn, msc, msi] = de_struct(
                n,
                [
                    "transfer_syntax_uid",
                    "transfer_syntax_name",
                    "media_storage_sop_class_uid",
                    "media_storage_sop_instance_uid",
                ],
            )?;
            FileMeta {
                transfer_syntax_uid: field(ts, de_string)?,
                transfer_syntax_name: field(tsn, de_string)?,
                media_storage_sop_class_uid: field(msc, de_string)?,
                media_storage_sop_instance_uid: field(msi, de_string)?,
            }
        }
    };
    let study = match study {
        None => Study::default(),
        Some(n) => {
            let [uid, acc, date, time, desc] = de_struct(
                n,
                [
                    "instance_uid",
                    "accession_number",
                    "date",
                    "time",
                    "description",
                ],
            )?;
            Study {
                instance_uid: field(uid, de_string)?,
                accession_number: field(acc, de_string)?,
                date: field(date, de_string)?,
                time: field(time, de_string)?,
                description: field(desc, de_string)?,
            }
        }
    };
    let series = match series {
        None => Series::default(),
        Some(n) => {
            let [uid, modality, number, date, time, desc] = de_struct(
                n,
                [
                    "instance_uid",
                    "modality",
                    "number",
                    "date",
                    "time",
                    "description",
                ],
            )?;
            Series {
                instance_uid: field(uid, de_string)?,
                modality: field(modality, de_string)?,
                number: field(number, de_string)?,
                date: field(date, de_string)?,
                time: field(time, de_string)?,
                description: field(desc, de_string)?,
            }
        }
    };
    let instance = match instance {
        None => Instance::default(),
        Some(n) => {
            let [class, uid, number, frames] = de_struct(
                n,
                [
                    "sop_class_uid",
                    "sop_instance_uid",
                    "number",
                    "number_of_frames",
                ],
            )?;
            Instance {
                sop_class_uid: field(class, de_string)?,
                sop_instance_uid: field(uid, de_string)?,
                number: field(number, de_string)?,
                number_of_frames: field(frames, de_int)?,
            }
        }
    };
    let missing = match missing {
        None => Vec::new(),
        Some(n) => de_slice(n, de_missing)?.unwrap_or_default(),
    };
    let diagnostics = match diagnostics {
        None => Vec::new(),
        Some(n) => de_slice(n, de_diagnostic)?.unwrap_or_default(),
    };
    Some(Record {
        source,
        file_meta,
        patient: de_pointer(patient, de_patient)?,
        study,
        series,
        instance,
        geometry: de_pointer(geometry, de_geometry)?,
        selected: BTreeMap::new(),
        missing,
        diagnostics,
    })
}
fn de_digest(n: &Node) -> Option<[u8; 32]> {
    match n {
        Node::Arr(items) if items.len() == 32 => {
            let mut out = [0u8; 32];
            for (o, item) in out.iter_mut().zip(items) {
                *o = de_uint(item, 8)? as u8;
            }
            Some(out)
        }
        _ => None,
    }
}
fn de_persistent(n: &Node) -> Option<PersistentRecord> {
    let [record, attributes, file_bytes, sha] =
        de_struct(n, ["record", "attributes", "file_bytes", "sha256"])?;
    let attributes = match attributes {
        None => BTreeMap::new(),
        Some(Node::Obj(members)) => {
            let mut m = BTreeMap::new();
            for (k, v) in members {
                m.insert(k.clone(), de_string(v)?);
            }
            m
        }
        Some(n) if is_null(n) => BTreeMap::new(),
        Some(_) => return None,
    };
    Some(PersistentRecord {
        record: field(record, de_record)?,
        attributes,
        file_bytes: field(file_bytes, de_int)?,
        sha256: de_digest(sha?)?,
    })
}
fn decode_persistent(key: &str, data: &[u8]) -> Result<PersistentRecord, PersistentError> {
    let bad = PersistentError::Inconsistent;
    let node = strict_persistent_json(data).ok_or(bad)?;
    let [version, value] = de_struct(&node, ["version", "value"]).ok_or(bad)?;
    let version = field(version, de_int).ok_or(bad)?;
    let value = value.ok_or(bad)?;
    if is_null(value) {
        return Err(bad);
    }
    let value = de_persistent(value).ok_or(bad)?;
    if version != 1
        || !valid_persistent(&value)
        || persistent_key(&value.record.instance.sop_instance_uid) != key
    {
        return Err(bad);
    }
    if persistent_admission(&value, data.len() as i64).is_err() {
        return Err(bad);
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PersistentRecord {
        let mut r = PersistentRecord {
            file_bytes: 400,
            sha256: Sha256::digest(b"synthetic bytes").into(),
            ..Default::default()
        };
        r.record.source.size = 400;
        r.record.file_meta.transfer_syntax_uid = "1.2.840.10008.1.2.1".into();
        r.record.file_meta.media_storage_sop_class_uid = "1.2.840.10008.5.1.4.1.1.7".into();
        r.record.file_meta.media_storage_sop_instance_uid = "2.25.1357".into();
        r.record.patient = Some(Patient {
            id: "SYNTHETIC".into(),
            ..Default::default()
        });
        r.record.study.instance_uid = "2.25.13571".into();
        r.record.series.instance_uid = "2.25.13572".into();
        r.record.instance.sop_class_uid = "1.2.840.10008.5.1.4.1.1.7".into();
        r.record.instance.sop_instance_uid = "2.25.1357".into();
        r.attributes.insert("00200010".into(), "TEST".into());
        r
    }
    #[test]
    fn round_trip_and_ambiguity() {
        let row = fixture();
        let data = marshal_envelope(&row).unwrap();
        let key = persistent_key("2.25.1357");
        assert_eq!(decode_persistent(&key, data.as_bytes()), Ok(row.clone()));
        let len = data.len() as i64;
        assert_eq!(persistent_admission(&row, len), Ok(()));
        assert_eq!(
            persistent_admission(&row, len - 1),
            Err(PersistentError::ResourceLimit)
        );
        for bad in [
            data.replacen(r#""version":1"#, r#""version":1,"version":1"#, 1),
            format!("{data}{{}}"),
            data.replacen(r#""version":1"#, r#""version":2"#, 1),
            data.replacen(r#""version":1"#, r#""version":1,"Version":1"#, 1),
            data.replacen(r#""version":1"#, r#""version":0,"verſion":1"#, 1),
        ] {
            assert!(decode_persistent(&key, bad.as_bytes()).is_err(), "{bad}");
        }
        let upper = data.replacen(r#""version":1"#, r#""VERSION":1"#, 1);
        assert!(decode_persistent(&key, upper.as_bytes()).is_ok());
    }
}
