use super::{
    ApplicationEntity, Budget, Cause, Connection, Device, Limits, Options, Result,
    TransferCapability, field_error, io_field_error, normalized, validate_normalized,
};
use crate::net::ul::{MAX_AE_TITLE_LENGTH, normalize_ae_title};
type Scan<T> = std::result::Result<T, Cause>;
struct Field {
    name: &'static str,
    kind: i32,
    required: bool,
}
const fn field(name: &'static str, kind: i32, required: bool) -> Field {
    Field {
        name,
        kind,
        required,
    }
}
const SCHEMAS: [&[Field]; 4] = [
    &[
        field("schemaVersion", -3, true),
        field("deviceName", -1, true),
        field("description", -1, false),
        field("installed", -2, false),
        field("connections", 10, true),
        field("applicationEntities", 11, true),
    ],
    &[
        field("id", -1, true),
        field("hostname", -1, true),
        field("port", -3, true),
        field("installed", -2, false),
        field("tlsRequired", -2, true),
    ],
    &[
        field("aeTitle", -1, true),
        field("connectionIDs", 13, true),
        field("associationInitiator", -2, true),
        field("associationAcceptor", -2, true),
        field("installed", -2, false),
        field("transferCapabilities", 12, true),
    ],
    &[
        field("sopClassUID", -1, true),
        field("role", -1, true),
        field("transferSyntaxUIDs", 14, true),
    ],
];
enum Value<'a> {
    Scalar(&'a [u8]),
    Array(Vec<Value<'a>>),
    Object(Object<'a>),
}
#[derive(Default)]
struct Object<'a> {
    fields: [Option<Box<Value<'a>>>; 6],
}
fn hex4(raw: &[u8]) -> Option<u32> {
    if raw.len() < 4 {
        return None;
    }
    let mut n = 0u32;
    for &c in &raw[..4] {
        n = n << 4 | (c as char).to_digit(16)?;
    }
    Some(n)
}
fn decode_string(raw: &[u8]) -> Option<String> {
    if raw.len() < 2 || raw[0] != b'"' || raw[raw.len() - 1] != b'"' {
        return None;
    }
    let body = &raw[1..raw.len() - 1];
    let mut out: Vec<u8> = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let c = body[i];
        if c == b'"' || c < 0x20 {
            return None;
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let escape = *body.get(i)?;
        i += 1;
        let simple = match escape {
            b'"' => b'"',
            b'\\' => b'\\',
            b'/' => b'/',
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'u' => {
                let mut rune = hex4(&body[i..])?;
                i += 4;
                if (0xD800..0xDC00).contains(&rune) {
                    let low = if body.get(i) == Some(&b'\\') && body.get(i + 1) == Some(&b'u') {
                        hex4(&body[i + 2..])
                    } else {
                        None
                    };
                    match low {
                        Some(low) if (0xDC00..0xE000).contains(&low) => {
                            rune = 0x10000 + ((rune - 0xD800) << 10) + (low - 0xDC00);
                            i += 6;
                        }
                        _ => rune = 0xFFFD,
                    }
                } else if (0xDC00..0xE000).contains(&rune) {
                    rune = 0xFFFD;
                }
                let ch = char::from_u32(rune).unwrap_or('\u{FFFD}');
                out.extend_from_slice(ch.encode_utf8(&mut [0u8; 4]).as_bytes());
                continue;
            }
            _ => return None,
        };
        out.push(simple);
    }
    String::from_utf8(out).ok()
}
fn integer_ok(raw: &[u8]) -> bool {
    if raw == b"null" {
        return true;
    }
    let digits = raw.strip_prefix(b"-").unwrap_or(raw);
    if digits.is_empty()
        || !digits.iter().all(u8::is_ascii_digit)
        || (digits.len() > 1 && digits[0] == b'0')
    {
        return false;
    }
    std::str::from_utf8(raw).is_ok_and(|text| text.parse::<i64>().is_ok())
}
fn atoi(raw: &[u8]) -> i64 {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
        .unwrap_or(0)
}
fn decoded_string_bytes(raw: &[u8]) -> i64 {
    decode_string(raw).map_or(0, |text| text.len() as i64)
}
struct Scanner<'a> {
    raw: &'a [u8],
    at: usize,
    l: Limits,
    b: Budget,
    caps: i64,
    syntaxes: i64,
}
impl<'a> Scanner<'a> {
    fn ws(&mut self) {
        while self.at < self.raw.len() && matches!(self.raw[self.at], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.at += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.ws();
        if self.at < self.raw.len() && self.raw[self.at] == c {
            self.at += 1;
            return true;
        }
        false
    }
    fn scalar(&mut self, kind: i32) -> Scan<&'a [u8]> {
        self.ws();
        let start = self.at;
        if start >= self.raw.len() {
            return Err(Cause::Invalid);
        }
        let max = self.l.max_scalar_bytes;
        if kind == -1 {
            if self.raw[self.at] != b'"' {
                return Err(Cause::Invalid);
            }
            self.at += 1;
            while self.at < self.raw.len() {
                let c = self.raw[self.at];
                self.at += 1;
                if (self.at - start) as i64 > max {
                    return Err(Cause::ResourceLimit);
                }
                if c == b'"' {
                    break;
                }
                if c == b'\\' {
                    if self.at >= self.raw.len() {
                        return Err(Cause::Invalid);
                    }
                    self.at += 1;
                }
            }
            if self.raw[self.at - 1] != b'"' {
                return Err(Cause::Invalid);
            }
        } else {
            while self.at < self.raw.len() {
                let c = self.raw[self.at];
                if matches!(c, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r') {
                    break;
                }
                self.at += 1;
                if (self.at - start) as i64 > max {
                    return Err(Cause::ResourceLimit);
                }
            }
        }
        let raw = &self.raw[start..self.at];
        self.b.add(raw.len() as i64)?;
        let valid = match kind {
            -1 => raw.len() >= 2 && decode_string(raw).is_some(),
            -2 => raw == b"true" || raw == b"false",
            _ => integer_ok(raw),
        };
        if !valid {
            return Err(Cause::Invalid);
        }
        Ok(raw)
    }
    fn object(&mut self, kind: usize, depth: i64) -> Scan<Object<'a>> {
        if depth > self.l.max_depth {
            return Err(Cause::ResourceLimit);
        }
        self.b.add(256)?;
        if !self.take(b'{') {
            return Err(Cause::Invalid);
        }
        let schema = SCHEMAS[kind];
        let mut out = Object::default();
        if !self.take(b'}') {
            loop {
                let key = self.scalar(-1)?;
                if key.len() > 128 {
                    return Err(Cause::ResourceLimit);
                }
                let name = decode_string(key).unwrap_or_default();
                let index = schema
                    .iter()
                    .position(|f| f.name == name && !name.is_empty());
                let Some(index) = index.filter(|&i| out.fields[i].is_none()) else {
                    return Err(Cause::Invalid);
                };
                if !self.take(b':') {
                    return Err(Cause::Invalid);
                }
                let f = &schema[index];
                let value = if f.kind < 0 {
                    let value = self.scalar(f.kind)?;
                    if f.kind == -1 {
                        let mut limit = self.l.max_name_bytes;
                        match kind {
                            0 if f.name == "description" => limit = self.l.max_description_bytes,
                            1 if f.name == "hostname" => limit = self.l.max_hostname_bytes,
                            2 => limit = MAX_AE_TITLE_LENGTH as i64,
                            3 => {
                                limit = 64;
                                if f.name == "role" {
                                    if decoded_string_bytes(value) != 3 {
                                        return Err(Cause::Invalid);
                                    }
                                    limit = 3;
                                }
                            }
                            _ => {}
                        }
                        if decoded_string_bytes(value) > limit {
                            return Err(Cause::ResourceLimit);
                        }
                    }
                    Value::Scalar(value)
                } else {
                    Value::Array(self.array(f.kind, depth + 1)?)
                };
                out.fields[index] = Some(Box::new(value));
                if self.take(b'}') {
                    break;
                }
                if !self.take(b',') {
                    return Err(Cause::Invalid);
                }
            }
        }
        for (i, f) in schema.iter().enumerate() {
            if f.required && out.fields[i].is_none() {
                return Err(Cause::Invalid);
            }
        }
        Ok(out)
    }
    fn array(&mut self, kind: i32, depth: i64) -> Scan<Vec<Value<'a>>> {
        if depth > self.l.max_depth {
            return Err(Cause::ResourceLimit);
        }
        if !self.take(b'[') {
            return Err(Cause::Invalid);
        }
        let (child, max) = match kind {
            10 => (Some(1), self.l.max_connections),
            11 => (Some(2), self.l.max_aes),
            12 => (Some(3), self.l.max_capabilities_per_ae),
            14 => (None, self.l.max_transfer_syntaxes_per_capability),
            _ => (None, self.l.max_connections),
        };
        let mut out = Vec::new();
        if self.take(b']') {
            return Ok(out);
        }
        let mut count = 0i64;
        loop {
            count += 1;
            if count > max {
                return Err(Cause::ResourceLimit);
            }
            if kind == 12 {
                self.caps += 1;
                if self.caps > self.l.max_capabilities {
                    return Err(Cause::ResourceLimit);
                }
            }
            if kind == 14 {
                self.syntaxes += 1;
                if self.syntaxes > self.l.max_transfer_syntaxes {
                    return Err(Cause::ResourceLimit);
                }
            }
            self.b.add(80)?;
            match child {
                None => {
                    let value = self.scalar(-1)?;
                    let limit = if kind == 14 {
                        64
                    } else {
                        self.l.max_name_bytes
                    };
                    if decoded_string_bytes(value) > limit {
                        return Err(Cause::ResourceLimit);
                    }
                    out.push(Value::Scalar(value));
                }
                Some(child) => out.push(Value::Object(self.object(child, depth + 1)?)),
            }
            if self.take(b']') {
                return Ok(out);
            }
            if !self.take(b',') {
                return Err(Cause::Invalid);
            }
        }
    }
}
impl<'a> Object<'a> {
    fn scalar(&self, index: usize) -> Option<&'a [u8]> {
        match self.fields[index].as_deref() {
            Some(Value::Scalar(raw)) => Some(raw),
            _ => None,
        }
    }
    fn text(&self, index: usize) -> String {
        self.scalar(index)
            .and_then(decode_string)
            .unwrap_or_default()
    }
    fn integer(&self, index: usize) -> i64 {
        self.scalar(index).map_or(0, atoi)
    }
    fn boolean(&self, index: usize) -> bool {
        self.scalar(index) == Some(b"true")
    }
    fn optional_bool(&self, index: usize) -> Option<bool> {
        self.scalar(index).map(|raw| raw == b"true")
    }
    fn array(&self, index: usize) -> &[Value<'a>] {
        match self.fields[index].as_deref() {
            Some(Value::Array(items)) => items,
            _ => &[],
        }
    }
    fn strings(&self, index: usize) -> Vec<String> {
        self.array(index)
            .iter()
            .map(|value| match value {
                Value::Scalar(raw) => decode_string(raw).unwrap_or_default(),
                _ => String::new(),
            })
            .collect()
    }
    fn objects(&self, index: usize) -> impl Iterator<Item = &Object<'a>> {
        self.array(index).iter().filter_map(|value| match value {
            Value::Object(object) => Some(object),
            _ => None,
        })
    }
}
fn decode_device(root: &Object) -> Device {
    Device {
        schema_version: root.integer(0),
        device_name: root.text(1),
        description: root.text(2),
        installed: root.optional_bool(3),
        connections: root
            .objects(4)
            .map(|c| Connection {
                id: c.text(0),
                hostname: c.text(1),
                port: c.integer(2),
                installed: c.optional_bool(3),
                tls_required: c.boolean(4),
            })
            .collect(),
        application_entities: root
            .objects(5)
            .map(|a| ApplicationEntity {
                ae_title: a.text(0),
                connection_ids: a.strings(1),
                association_initiator: a.boolean(2),
                association_acceptor: a.boolean(3),
                installed: a.optional_bool(4),
                transfer_capabilities: a
                    .objects(5)
                    .map(|t| TransferCapability {
                        sop_class_uid: t.text(0),
                        role: t.text(1),
                        transfer_syntax_uids: t.strings(2),
                    })
                    .collect(),
            })
            .collect(),
    }
}
pub fn parse(raw: &[u8], options: Options) -> Result<Device> {
    const FIELD: &str = "json";
    let bare = |cause| field_error(FIELD, cause);
    let o = normalized(options).map_err(|cause| field_error("limits", cause))?;
    if raw.len() as i64 > o.limits.max_json_bytes {
        return Err(bare(Cause::ResourceLimit));
    }
    if std::str::from_utf8(raw).is_err() {
        return Err(bare(Cause::Invalid));
    }
    let mut s = Scanner {
        raw,
        at: 0,
        l: o.limits,
        b: Budget::new(o.limits.max_compiled_bytes),
        caps: 0,
        syntaxes: 0,
    };
    let root = s.object(0, 1).map_err(bare)?;
    s.ws();
    if s.at != raw.len() {
        return Err(bare(Cause::Invalid));
    }
    let mut d = decode_device(&root);
    validate_normalized(&d, &o)?;
    for ae in &mut d.application_entities {
        ae.ae_title = normalize_ae_title(&ae.ae_title).to_string();
    }
    Ok(d)
}
pub fn read_file(path: impl AsRef<std::path::Path>, options: Options) -> Result<Device> {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    const FIELD: &str = "configurationFile";
    let o = normalized(options).map_err(|cause| field_error(FIELD, cause))?;
    let raw = std::fs::read(path).map_err(|err| io_field_error(FIELD, err.kind()))?;
    parse(&raw, o)
}
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
fn json_string_size(s: &str) -> i64 {
    let mut n = 2i64;
    for r in s.chars() {
        n += match r {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            r if (r as u32) < 32 || r == '\u{2028}' || r == '\u{2029}' => 6,
            r => r.len_utf8() as i64,
        };
    }
    n
}
fn bool_size(v: bool) -> i64 {
    if v { 4 } else { 5 }
}
fn installed_size(v: Option<bool>) -> i64 {
    v.map_or(0, |v| ",\"installed\":".len() as i64 + bool_size(v))
}
fn list_size(values: &[String]) -> i64 {
    let mut n = 2i64;
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            n += 1;
        }
        n += json_string_size(v);
    }
    n
}
fn lit(text: &str) -> i64 {
    text.len() as i64
}
fn json_device_size(d: &Device) -> i64 {
    let mut n = lit("{\"schemaVersion\":1,\"deviceName\":")
        + json_string_size(&d.device_name)
        + installed_size(d.installed);
    if !d.description.is_empty() {
        n += lit(",\"description\":") + json_string_size(&d.description);
    }
    n += lit(",\"connections\":[");
    for (i, c) in d.connections.iter().enumerate() {
        if i > 0 {
            n += 1;
        }
        n += lit("{\"id\":")
            + json_string_size(&c.id)
            + lit(",\"hostname\":")
            + json_string_size(&c.hostname)
            + lit(",\"port\":")
            + lit(&c.port.to_string())
            + installed_size(c.installed)
            + lit(",\"tlsRequired\":")
            + bool_size(c.tls_required)
            + 1;
    }
    n += lit("],\"applicationEntities\":[");
    for (i, a) in d.application_entities.iter().enumerate() {
        if i > 0 {
            n += 1;
        }
        n += lit("{\"aeTitle\":")
            + json_string_size(normalize_ae_title(&a.ae_title))
            + lit(",\"connectionIDs\":")
            + list_size(&a.connection_ids)
            + lit(",\"associationInitiator\":")
            + bool_size(a.association_initiator)
            + lit(",\"associationAcceptor\":")
            + bool_size(a.association_acceptor)
            + installed_size(a.installed)
            + lit(",\"transferCapabilities\":[");
        for (j, t) in a.transfer_capabilities.iter().enumerate() {
            if j > 0 {
                n += 1;
            }
            n += lit("{\"sopClassUID\":")
                + json_string_size(&t.sop_class_uid)
                + lit(",\"role\":")
                + json_string_size(&t.role)
                + lit(",\"transferSyntaxUIDs\":")
                + list_size(&t.transfer_syntax_uids)
                + 1;
        }
        n += 2;
    }
    n + 2
}
fn serialization_admission(d: &Device, l: &Limits) -> Scan<()> {
    let depth = if d
        .application_entities
        .iter()
        .any(|a| !a.transfer_capabilities.is_empty())
    {
        6
    } else {
        4
    };
    if depth > l.max_depth {
        return Err(Cause::ResourceLimit);
    }
    let mut b = Budget::new(l.max_compiled_bytes);
    let max_scalar = l.max_scalar_bytes;
    fn scalar(b: &mut Budget, max: i64, n: i64) -> Scan<()> {
        if n > max {
            return Err(Cause::ResourceLimit);
        }
        b.add(n)
    }
    let text = |b: &mut Budget, s: &str| scalar(b, max_scalar, json_string_size(s));
    let object = |b: &mut Budget, kind: usize, optional: &dyn Fn(&str) -> bool| -> Scan<()> {
        b.add(256)?;
        for f in SCHEMAS[kind] {
            if f.required || optional(f.name) {
                text(b, f.name)?;
            }
        }
        Ok(())
    };
    object(&mut b, 0, &|k| {
        k == "description" && !d.description.is_empty() || k == "installed" && d.installed.is_some()
    })?;
    scalar(&mut b, max_scalar, 1)?;
    text(&mut b, &d.device_name)?;
    if !d.description.is_empty() {
        text(&mut b, &d.description)?;
    }
    if let Some(installed) = d.installed {
        scalar(&mut b, max_scalar, bool_size(installed))?;
    }
    for c in &d.connections {
        b.add(80)?;
        object(&mut b, 1, &|k| k == "installed" && c.installed.is_some())?;
        text(&mut b, &c.id)?;
        text(&mut b, &c.hostname)?;
        scalar(&mut b, max_scalar, lit(&c.port.to_string()))?;
        scalar(&mut b, max_scalar, bool_size(c.tls_required))?;
        if let Some(installed) = c.installed {
            scalar(&mut b, max_scalar, bool_size(installed))?;
        }
    }
    for a in &d.application_entities {
        b.add(80)?;
        object(&mut b, 2, &|k| k == "installed" && a.installed.is_some())?;
        text(&mut b, normalize_ae_title(&a.ae_title))?;
        scalar(&mut b, max_scalar, bool_size(a.association_initiator))?;
        scalar(&mut b, max_scalar, bool_size(a.association_acceptor))?;
        if let Some(installed) = a.installed {
            scalar(&mut b, max_scalar, bool_size(installed))?;
        }
        for id in &a.connection_ids {
            b.add(80)?;
            text(&mut b, id)?;
        }
        for t in &a.transfer_capabilities {
            b.add(80)?;
            object(&mut b, 3, &|_| false)?;
            text(&mut b, &t.sop_class_uid)?;
            text(&mut b, &t.role)?;
            for ts in &t.transfer_syntax_uids {
                b.add(80)?;
                text(&mut b, ts)?;
            }
        }
    }
    Ok(())
}
fn json_list(out: &mut String, values: &[String]) {
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(out, v);
    }
    out.push(']');
}
fn json_installed(out: &mut String, v: Option<bool>) {
    if let Some(v) = v {
        out.push_str(&format!(",\"installed\":{v}"));
    }
}
pub fn marshal(d: &Device, options: Options) -> Result<Vec<u8>> {
    const FIELD: &str = "serialization";
    let bare = |cause| field_error(FIELD, cause);
    let o = normalized(options).map_err(bare)?;
    validate_normalized(d, &o)?;
    serialization_admission(d, &o.limits).map_err(bare)?;
    if json_device_size(d) > o.limits.max_json_bytes {
        return Err(bare(Cause::ResourceLimit));
    }
    let mut out = String::new();
    out.push_str(&format!("{{\"schemaVersion\":{}", d.schema_version));
    out.push_str(",\"deviceName\":");
    json_string(&mut out, &d.device_name);
    if !d.description.is_empty() {
        out.push_str(",\"description\":");
        json_string(&mut out, &d.description);
    }
    json_installed(&mut out, d.installed);
    out.push_str(",\"connections\":[");
    for (i, c) in d.connections.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        json_string(&mut out, &c.id);
        out.push_str(",\"hostname\":");
        json_string(&mut out, &c.hostname);
        out.push_str(&format!(",\"port\":{}", c.port));
        json_installed(&mut out, c.installed);
        out.push_str(&format!(",\"tlsRequired\":{}}}", c.tls_required));
    }
    out.push_str("],\"applicationEntities\":[");
    for (i, a) in d.application_entities.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"aeTitle\":");
        json_string(&mut out, normalize_ae_title(&a.ae_title));
        out.push_str(",\"connectionIDs\":");
        json_list(&mut out, &a.connection_ids);
        out.push_str(&format!(
            ",\"associationInitiator\":{},\"associationAcceptor\":{}",
            a.association_initiator, a.association_acceptor
        ));
        json_installed(&mut out, a.installed);
        out.push_str(",\"transferCapabilities\":[");
        for (j, t) in a.transfer_capabilities.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str("{\"sopClassUID\":");
            json_string(&mut out, &t.sop_class_uid);
            out.push_str(",\"role\":");
            json_string(&mut out, &t.role);
            out.push_str(",\"transferSyntaxUIDs\":");
            json_list(&mut out, &t.transfer_syntax_uids);
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    Ok(out.into_bytes())
}
