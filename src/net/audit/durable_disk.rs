#![forbid(unsafe_code)]
use super::durable::{Spool, destination_fingerprint, hex, valid_destination_id, valid_hex_digest};
use super::jsonwire::{self, Value};
use super::{Error, ErrorKind, Result, Time, ctx_err, privatefs, timetext};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
const SPOOL_VERSION: i64 = 1;
const RECORD_HEADER_LIMIT: u64 = 8192;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DestinationState {
    pub(crate) fingerprint: String,
    pub(crate) cursor: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RecordMeta {
    pub(crate) sequence: u64,
    pub(crate) source: u64,
    pub(crate) created: Time,
    pub(crate) created_utc: bool,
    pub(crate) bytes: i64,
    pub(crate) digest: String,
    pub(crate) targets: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DiskState {
    pub(crate) version: i64,
    pub(crate) next: u64,
    pub(crate) source_identity: String,
    pub(crate) source_cursor: u64,
    pub(crate) source_paused: bool,
    pub(crate) destinations: BTreeMap<String, DestinationState>,
    pub(crate) no_destinations: bool,
    pub(crate) records: Vec<RecordMeta>,
    pub(crate) expired: u64,
    pub(crate) suppressed: u64,
    pub(crate) sent: u64,
    pub(crate) failed: u64,
    pub(crate) rejected: u64,
    pub(crate) source_retirements: u64,
}
fn encode_meta(out: &mut String, r: &RecordMeta) {
    out.push_str(&format!(
        "{{\"Sequence\":{},\"Source\":{},\"Created\":\"{}\",\"Bytes\":{},\"Digest\":",
        r.sequence,
        r.source,
        timetext::format_rfc3339_nano(&r.created),
        r.bytes
    ));
    jsonwire::push_string(out, &r.digest);
    out.push_str(",\"Targets\":[");
    for (i, t) in r.targets.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        jsonwire::push_string(out, t);
    }
    out.push_str("]}");
}
fn encode_state(s: &DiskState) -> String {
    let mut out = format!(
        "{{\"Version\":{},\"Next\":{},\"SourceIdentity\":",
        s.version, s.next
    );
    jsonwire::push_string(&mut out, &s.source_identity);
    out.push_str(&format!(
        ",\"SourceCursor\":{},\"SourcePaused\":{},\"Destinations\":",
        s.source_cursor, s.source_paused
    ));
    if s.no_destinations {
        out.push_str("null");
    } else {
        out.push('{');
        for (i, (id, d)) in s.destinations.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            jsonwire::push_string(&mut out, id);
            out.push_str(":{\"Fingerprint\":");
            jsonwire::push_string(&mut out, &d.fingerprint);
            out.push_str(&format!(",\"Cursor\":{}}}", d.cursor));
        }
        out.push('}');
    }
    out.push_str(",\"Records\":");
    if s.records.is_empty() {
        out.push_str("null");
    } else {
        out.push('[');
        for (i, r) in s.records.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_meta(&mut out, r);
        }
        out.push(']');
    }
    out.push_str(&format!(
        ",\"Expired\":{},\"Suppressed\":{},\"Sent\":{},\"Failed\":{},\"Rejected\":{},\"SourceRetirements\":{}}}",
        s.expired, s.suppressed, s.sent, s.failed, s.rejected, s.source_retirements
    ));
    out
}
fn decode_string(v: &Value) -> Option<Option<String>> {
    match v {
        Value::Null => Some(None),
        Value::String(s, _) => Some(Some(s.clone())),
        _ => None,
    }
}
fn members<'v, 'a>(
    v: &'v Value<'a>,
    names: &[&'static str],
) -> Option<Vec<(&'static str, &'v Value<'a>)>> {
    match v {
        Value::Null => Some(Vec::new()),
        Value::Object(members) => {
            let mut out = Vec::new();
            for (name, value, _) in members {
                let field = jsonwire::field(names, name)?;
                if !matches!(value, Value::Null) {
                    out.push((field, value));
                }
            }
            Some(out)
        }
        _ => None,
    }
}
fn decode_meta(v: &Value) -> Option<RecordMeta> {
    let mut r = RecordMeta::default();
    let names = [
        "Sequence", "Source", "Created", "Bytes", "Digest", "Targets",
    ];
    for (field, value) in members(v, &names)? {
        match field {
            "Sequence" => r.sequence = jsonwire::as_u64(value)?,
            "Source" => r.source = jsonwire::as_u64(value)?,
            "Created" => {
                let Value::String(_, raw) = value else {
                    return None;
                };
                let text = std::str::from_utf8(raw).ok()?;
                r.created = timetext::parse_rfc3339_json(text)?;
                r.created_utc = text.ends_with('Z');
            }
            "Bytes" => r.bytes = jsonwire::as_i64(value)?,
            "Digest" => r.digest = decode_string(value)??,
            _ => {
                let Value::Array(items) = value else {
                    return None;
                };
                r.targets = items
                    .iter()
                    .map(|t| decode_string(t).map(Option::unwrap_or_default))
                    .collect::<Option<Vec<_>>>()?;
            }
        }
    }
    Some(r)
}
fn decode_state(v: &Value) -> Option<DiskState> {
    let mut s = DiskState {
        no_destinations: true,
        ..Default::default()
    };
    let names = [
        "Version",
        "Next",
        "SourceIdentity",
        "SourceCursor",
        "SourcePaused",
        "Destinations",
        "Records",
        "Expired",
        "Suppressed",
        "Sent",
        "Failed",
        "Rejected",
        "SourceRetirements",
    ];
    for (field, value) in members(v, &names)? {
        match field {
            "Version" => s.version = jsonwire::as_i64(value)?,
            "Next" => s.next = jsonwire::as_u64(value)?,
            "SourceIdentity" => s.source_identity = decode_string(value)??,
            "SourceCursor" => s.source_cursor = jsonwire::as_u64(value)?,
            "SourcePaused" => {
                let Value::Bool(b) = value else { return None };
                s.source_paused = *b;
            }
            "Destinations" => {
                let Value::Object(entries) = value else {
                    return None;
                };
                s.no_destinations = false;
                for (id, entry, _) in entries {
                    let mut d = DestinationState::default();
                    for (field, value) in members(entry, &["Fingerprint", "Cursor"])? {
                        if field == "Fingerprint" {
                            d.fingerprint = decode_string(value)??;
                        } else {
                            d.cursor = jsonwire::as_u64(value)?;
                        }
                    }
                    s.destinations.insert(id.clone(), d);
                }
            }
            "Records" => {
                let Value::Array(items) = value else {
                    return None;
                };
                s.records = items.iter().map(decode_meta).collect::<Option<Vec<_>>>()?;
            }
            "Expired" => s.expired = jsonwire::as_u64(value)?,
            "Suppressed" => s.suppressed = jsonwire::as_u64(value)?,
            "Sent" => s.sent = jsonwire::as_u64(value)?,
            "Failed" => s.failed = jsonwire::as_u64(value)?,
            "Rejected" => s.rejected = jsonwire::as_u64(value)?,
            _ => s.source_retirements = jsonwire::as_u64(value)?,
        }
    }
    Some(s)
}
fn decode_envelope(data: &[u8]) -> Option<DiskState> {
    let envelope = jsonwire::parse(data)?;
    let mut digest = String::new();
    let mut state: &[u8] = &[];
    if let Value::Object(entries) = &envelope {
        for (name, value, raw) in entries {
            match jsonwire::field(&["Digest", "State"], name)? {
                "Digest" => {
                    if let Some(text) = decode_string(value)? {
                        digest = text;
                    }
                }
                _ => state = raw,
            }
        }
    } else if !matches!(envelope, Value::Null) {
        return None;
    }
    if hex(&Sha256::digest(state)) != digest {
        return None;
    }
    decode_state(&jsonwire::parse(state)?)
}
fn valid_record_meta(r: &RecordMeta, maximum: i64) -> bool {
    if r.sequence == 0
        || r.created.is_zero()
        || r.bytes < 1
        || r.bytes > maximum
        || r.digest.len() != 64
        || r.targets.is_empty()
        || r.targets.len() > 16
        || !valid_hex_digest(&r.digest)
    {
        return false;
    }
    r.targets
        .iter()
        .enumerate()
        .all(|(i, id)| valid_destination_id(id) && (i == 0 || r.targets[i - 1] < *id))
}
fn private_regular(path: &Path, maximum: u64) -> std::result::Result<(), Option<()>> {
    let info = match fs::symlink_metadata(path) {
        Ok(info) => info,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(None),
        Err(_) => return Err(Some(())),
    };
    if !info.file_type().is_file() || privatefs::check(path).is_err() || info.len() > maximum {
        return Err(Some(()));
    }
    Ok(())
}
pub(crate) enum RecordError {
    Context(Error),
    Corrupt,
}
impl Spool<'_> {
    fn directory(&self) -> &Path {
        Path::new(&self.options.directory)
    }
    fn record_path(&self, sequence: u64) -> PathBuf {
        self.directory().join(format!("{sequence:020}.msg"))
    }
    fn state_limit(&self) -> u64 {
        self.options.max_messages as u64 * 2048 + (1 << 20)
    }
    fn recovery_err(&self) -> Option<Error> {
        ctx_err(self.recovery)
    }
    pub(crate) fn open_disk(&mut self, lock: &mut Option<File>) -> Result<()> {
        let durable = |code: &str| Error::spool(code, ErrorKind::DurableSpool);
        let corrupt = |code: &str| Error::spool(code, ErrorKind::SpoolCorrupt);
        let directory = self.directory().to_path_buf();
        if privatefs::create_directory(&directory).is_err() {
            return Err(durable("directory"));
        }
        match fs::symlink_metadata(&directory) {
            Ok(info) if info.is_dir() && privatefs::check(&directory).is_ok() => {}
            _ => return Err(durable("directory_permissions")),
        }
        let lock_path = directory.join(".lock");
        match fs::symlink_metadata(&lock_path) {
            Ok(info) => {
                if !info.file_type().is_file() || privatefs::check(&lock_path).is_err() {
                    return Err(durable("lock_permissions"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(durable("lock")),
        }
        match privatefs::lock_spool(&lock_path) {
            Ok(f) => *lock = Some(f),
            Err(_) => return Err(durable("lock")),
        }
        if privatefs::secure(&lock_path).is_err() {
            return Err(durable("lock_permissions"));
        }
        let o = self.options;
        let path = directory.join("state.json");
        match private_regular(&path, self.state_limit()) {
            Err(None) => {
                *self.state = DiskState {
                    version: SPOOL_VERSION,
                    next: 1,
                    source_identity: o.source_identity.clone(),
                    source_cursor: o.initial_source_cursor,
                    ..Default::default()
                };
            }
            Err(Some(())) => return Err(corrupt("state_integrity")),
            Ok(()) => {
                let Ok(data) = fs::read(&path) else {
                    return Err(durable("state_read"));
                };
                match decode_envelope(&data) {
                    Some(state) => *self.state = state,
                    None => return Err(corrupt("state_integrity")),
                }
            }
        }
        self.validate_state()?;
        if self.state.source_identity != o.source_identity {
            return Err(durable("source_identity"));
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            return Err(durable("directory_read"));
        };
        let known: HashMap<u64, RecordMeta> = self
            .state
            .records
            .iter()
            .map(|r| (r.sequence, r.clone()))
            .collect();
        let mut seen: HashSet<u64> = HashSet::new();
        let mut orphan: Vec<RecordMeta> = Vec::new();
        let mut garbage: Vec<String> = Vec::new();
        let mut count = 0i64;
        for entry in entries {
            if let Some(e) = self.recovery_err() {
                return Err(e);
            }
            let Ok(entry) = entry else {
                return Err(durable("directory_read"));
            };
            count += 1;
            if count > 2 * o.max_messages + 32 {
                return Err(corrupt("directory_entry_limit"));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".lock" || name == "state.json" {
                continue;
            }
            if name.starts_with(".tmp-") {
                let limit = o.max_record_bytes as u64 + self.state_limit() + RECORD_HEADER_LIMIT;
                if private_regular(&directory.join(&name), limit).is_err() {
                    return Err(corrupt("temporary_integrity"));
                }
                garbage.push(name);
                continue;
            }
            if name.len() != 24 || !name.ends_with(".msg") {
                return Err(corrupt("directory_entry"));
            }
            let digits = &name.as_bytes()[..20];
            let seq = if digits.iter().all(u8::is_ascii_digit) {
                std::str::from_utf8(digits)
                    .ok()
                    .and_then(|d| d.parse::<u64>().ok())
            } else {
                None
            };
            let seq = match seq {
                Some(seq) if seq != 0 && seen.insert(seq) => seq,
                _ => return Err(corrupt("record_name")),
            };
            if seq < self.state.next {
                if let Some(expected) = known.get(&seq) {
                    match self.read_record_meta(seq) {
                        Err(RecordError::Context(e)) => return Err(e),
                        Ok(got) if got == *expected => {}
                        _ => return Err(corrupt("record_integrity")),
                    }
                } else {
                    garbage.push(name);
                }
                continue;
            }
            match self.read_record_meta(seq) {
                Err(RecordError::Context(e)) => return Err(e),
                Err(RecordError::Corrupt) => return Err(corrupt("orphan_integrity")),
                Ok(got) => orphan.push(got),
            }
        }
        if known.keys().any(|seq| !seen.contains(seq)) {
            return Err(corrupt("missing_record"));
        }
        orphan.sort_by_key(|r| r.sequence);
        for r in orphan {
            if r.sequence != self.state.next || self.state.records.len() as i64 >= o.max_messages {
                return Err(corrupt("orphan_sequence"));
            }
            if r.targets
                .iter()
                .any(|id| !self.state.destinations.contains_key(id))
            {
                return Err(corrupt("orphan_destination"));
            }
            if r.source > 0 {
                if r.source <= self.state.source_cursor {
                    return Err(corrupt("orphan_source"));
                }
                self.state.source_cursor = r.source;
            }
            self.state.records.push(r);
            self.state.next += 1;
        }
        self.validate_state()?;
        if self.state.source_paused && o.resume_source {
            self.state.source_paused = false;
            if o.initial_source_cursor > self.state.source_cursor {
                self.state.source_cursor = o.initial_source_cursor;
                self.state.source_retirements += 1;
            }
        }
        self.expire()?;
        let stale: Vec<String> = self
            .state
            .destinations
            .keys()
            .filter(|id| !self.has_pending(id) && !o.destinations.iter().any(|d| d.id == **id))
            .cloned()
            .collect();
        for id in stale {
            self.state.destinations.remove(&id);
        }
        for d in &o.destinations {
            let fingerprint = destination_fingerprint(d);
            let old = self.state.destinations.get(&d.id);
            let changed = old.is_none_or(|old| old.fingerprint != fingerprint);
            if old.is_some() && changed && self.has_pending(&d.id) {
                return Err(Error::spool(
                    "pending_destination_changed",
                    ErrorKind::SyslogConfig,
                ));
            }
            if changed {
                self.state.destinations.insert(
                    d.id.clone(),
                    DestinationState {
                        fingerprint,
                        cursor: self.state.next - 1,
                    },
                );
            }
        }
        if self.state.destinations.len() > 256 {
            return Err(Error::spool(
                "destination_history_limit",
                ErrorKind::SpoolFull,
            ));
        }
        self.commit(self.state.clone())?;
        for name in &garbage {
            if fs::remove_file(directory.join(name)).is_err() {
                return Err(durable("temporary_cleanup"));
            }
        }
        if !garbage.is_empty() && self.sync_directory().is_err() {
            return Err(durable("directory_sync"));
        }
        self.expire()
    }
    fn validate_state(&self) -> Result<()> {
        let corrupt = |code: &str| Error::spool(code, ErrorKind::SpoolCorrupt);
        let s = &*self.state;
        let o = self.options;
        if s.version != SPOOL_VERSION
            || s.next == 0
            || s.source_identity.len() > 256
            || s.records.len() as i64 > o.max_messages
            || s.destinations.len() > 256
            || s.no_destinations
        {
            return Err(corrupt("state_limits"));
        }
        let mut size = 0i64;
        let mut prior = 0u64;
        for r in &s.records {
            if r.sequence <= prior
                || r.sequence >= s.next
                || r.source > s.source_cursor
                || !valid_record_meta(r, o.max_record_bytes)
            {
                return Err(corrupt("state_records"));
            }
            prior = r.sequence;
            size += r.bytes;
            if size > o.max_bytes {
                return Err(Error::spool("state_byte_limit", ErrorKind::SpoolFull));
            }
            if r.targets.iter().any(|id| !s.destinations.contains_key(id)) {
                return Err(corrupt("state_destination"));
            }
        }
        for (id, d) in &s.destinations {
            if !valid_destination_id(id) || d.fingerprint.len() != 64 || d.cursor >= s.next {
                return Err(corrupt("state_destination"));
            }
        }
        Ok(())
    }
    pub(crate) fn commit(&mut self, next: DiskState) -> Result<()> {
        let state = encode_state(&next);
        let mut envelope = String::from("{\"Digest\":");
        jsonwire::push_string(&mut envelope, &hex(&Sha256::digest(state.as_bytes())));
        envelope.push_str(",\"State\":");
        envelope.push_str(&state);
        envelope.push('}');
        if envelope.len() as u64 > self.state_limit()
            || self
                .atomic_file("state.json", envelope.as_bytes(), "state")
                .is_err()
        {
            return Err(self.disk_failure());
        }
        *self.state = next;
        Ok(())
    }
    fn publish(&self, target: &Path, parts: &[&[u8]], stage: &str) -> std::io::Result<()> {
        let (mut f, temp) = privatefs::create_temp(self.directory())?;
        let written = (|| {
            privatefs::secure(&temp)?;
            for part in parts {
                f.write_all(part)?;
            }
            self.fault(&format!("{stage}_sync"))?;
            f.sync_all()?;
            Ok(())
        })();
        drop(f);
        let published = written
            .and_then(|()| self.fault(&format!("{stage}_rename")))
            .and_then(|()| privatefs::replace(&temp, target));
        if let Err(e) = published {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        self.sync_directory()
    }
    fn atomic_file(&self, name: &str, data: &[u8], stage: &str) -> std::io::Result<()> {
        self.publish(&self.directory().join(name), &[data], stage)
    }
    fn sync_directory(&self) -> std::io::Result<()> {
        self.fault("dir_sync")?;
        privatefs::sync_directory(self.directory())
    }
    pub(crate) fn write_record(&self, meta: &RecordMeta, record: &[u8]) -> std::io::Result<()> {
        let mut header = String::new();
        encode_meta(&mut header, meta);
        if header.len() as u64 > RECORD_HEADER_LIMIT {
            return Err(std::io::Error::other("header"));
        }
        let prefix = (header.len() as u64).to_be_bytes();
        self.publish(
            &self.record_path(meta.sequence),
            &[&prefix, header.as_bytes(), record],
            "record",
        )
    }
    fn open_record(&self, seq: u64) -> Option<(File, RecordMeta)> {
        let path = self.record_path(seq);
        let limit = self.options.max_record_bytes as u64 + RECORD_HEADER_LIMIT + 8;
        private_regular(&path, limit).ok()?;
        let mut f = File::open(&path).ok()?;
        let mut prefix = [0u8; 8];
        f.read_exact(&mut prefix).ok()?;
        let size = u64::from_be_bytes(prefix);
        if size == 0 || size > RECORD_HEADER_LIMIT {
            return None;
        }
        let mut header = vec![0u8; size as usize];
        f.read_exact(&mut header).ok()?;
        let meta = decode_meta(&jsonwire::parse(&header)?)?;
        if meta.sequence != seq || !valid_record_meta(&meta, self.options.max_record_bytes) {
            return None;
        }
        if f.metadata().ok()?.len() != 8 + size + meta.bytes as u64 {
            return None;
        }
        Some((f, meta))
    }
    fn read_record_meta(&self, seq: u64) -> std::result::Result<RecordMeta, RecordError> {
        let (mut f, meta) = self.open_record(seq).ok_or(RecordError::Corrupt)?;
        let mut h = Sha256::new();
        let mut buffer = vec![0u8; 64 << 10];
        let mut n = 0i64;
        loop {
            if let Some(e) = self.recovery_err() {
                return Err(RecordError::Context(e));
            }
            match f.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    h.update(&buffer[..read]);
                    n += read as i64;
                }
                Err(_) => return Err(RecordError::Corrupt),
            }
        }
        if n != meta.bytes || hex(&h.finalize()) != meta.digest {
            return Err(RecordError::Corrupt);
        }
        Ok(meta)
    }
    pub(crate) fn read_record(&self, expected: &RecordMeta) -> std::result::Result<Vec<u8>, ()> {
        let (mut f, meta) = self.open_record(expected.sequence).ok_or(())?;
        if meta != *expected {
            return Err(());
        }
        let mut record = vec![0u8; meta.bytes as usize];
        f.read_exact(&mut record).map_err(|_| ())?;
        if hex(&Sha256::digest(&record)) != meta.digest {
            return Err(());
        }
        Ok(record)
    }
    fn has_pending(&self, id: &str) -> bool {
        let cursor = self.state.cursor(id);
        self.state
            .records
            .iter()
            .any(|r| cursor < r.sequence && r.targets.iter().any(|t| t == id))
    }
    pub(crate) fn expire(&mut self) -> Result<()> {
        let mut next = self.state.clone();
        let mut retired: Vec<u64> = Vec::new();
        next.records.clear();
        let cutoff = self.now().nanos_since_zero() - i128::from(self.options.max_age.0);
        for r in &self.state.records {
            let done = r
                .targets
                .iter()
                .all(|id| self.state.cursor(id) >= r.sequence);
            if r.created.nanos_since_zero() < cutoff {
                next.expired += 1;
                retired.push(r.sequence);
            } else if done {
                retired.push(r.sequence);
            } else {
                next.records.push(r.clone());
            }
        }
        if retired.is_empty() {
            return Ok(());
        }
        self.commit(next)?;
        for sequence in retired {
            if self.fault("gc").is_err() {
                return Err(self.disk_failure());
            }
            match fs::remove_file(self.record_path(sequence)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(self.disk_failure()),
            }
        }
        if self.sync_directory().is_err() {
            return Err(self.disk_failure());
        }
        Ok(())
    }
}
