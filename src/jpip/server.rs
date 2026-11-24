use super::codestream::MAX_COMPONENTS;
use super::databins::DataBinKey;
use super::index::{CodestreamIndex, IndexLimits, index_codestream};
use super::message::EorReason;
use super::plan::{ResponsePlan, ViewWindow};
use super::target::{Target, TargetProvider, target_id};
use super::{ChainError, Sentinel};
use crate::net::dicomweb::server::http::{Request, ResponseWriter, http_error};
use crate::net::dicomweb::types::status_text;
use crate::net::dicomweb::urlrules;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
pub type EventSink = Arc<dyn Fn(&ServerEvent) + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i128 + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServerConfig {
    pub provider: Option<Arc<dyn TargetProvider>>,
    pub max_sessions: usize,
    pub session_idle_timeout: i64,
    pub max_concurrent_requests: usize,
    pub max_response_bytes: usize,
    pub max_query_bytes: usize,
    pub max_cache_bytes: i64,
    pub index: IndexLimits,
    pub on_event: Option<EventSink>,
    pub now: Option<Clock>,
}
const DEFAULT_MAX_SESSIONS: usize = 64;
const DEFAULT_SESSION_IDLE_TIMEOUT: i64 = 5 * 60 * 1_000_000_000;
const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 16;
const DEFAULT_MAX_SERVER_RESPONSE: usize = 16 << 20;
const DEFAULT_MAX_QUERY_BYTES: usize = 4 << 10;
const DEFAULT_MAX_CACHE_BYTES: i64 = 512 << 20;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerEventKind {
    #[default]
    Request,
    SessionOpened,
    SessionClosed,
    SessionExpired,
}
impl ServerEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerEventKind::Request => "request",
            ServerEventKind::SessionOpened => "session-opened",
            ServerEventKind::SessionClosed => "session-closed",
            ServerEventKind::SessionExpired => "session-expired",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerEvent {
    pub kind: ServerEventKind,
    pub status: u16,
    pub content_type: String,
    pub reason: String,
    pub bytes: usize,
    pub messages: usize,
    pub eor: EorReason,
    pub sessions: usize,
    pub duration: i64,
}
struct SessionState {
    tid: String,
    model: HashMap<DataBinKey, i64>,
    last_used: i128,
}
struct Session {
    id: String,
    target: String,
    state: Mutex<SessionState>,
    closed: AtomicBool,
}
struct Indexes {
    indexes: HashMap<usize, Arc<CodestreamIndex>>,
    errs: HashMap<usize, ChainError>,
}
struct CachedTarget {
    name: String,
    tid: String,
    target: Target,
    indexes: Mutex<Indexes>,
}
struct State {
    closed: bool,
    sessions: HashMap<String, Arc<Session>>,
    lru: Vec<(Arc<CachedTarget>, i64)>,
    cached: i64,
}
pub struct Server {
    config: ServerConfig,
    provider: Arc<dyn TargetProvider>,
    now: Clock,
    slots: AtomicUsize,
    state: Mutex<State>,
}
struct RequestError {
    status: u16,
    reason: &'static str,
}
fn fail(status: u16, reason: &'static str) -> RequestError {
    RequestError { status, reason }
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn system_now() -> i128 {
    chrono::Utc::now()
        .timestamp_nanos_opt()
        .map_or(0, i128::from)
}
impl Server {
    pub fn new(mut config: ServerConfig) -> Result<Server, ChainError> {
        let Some(provider) = config.provider.clone() else {
            return Err(ChainError::plain("jpip: server needs a target provider"));
        };
        if config.max_sessions == 0 {
            config.max_sessions = DEFAULT_MAX_SESSIONS;
        }
        if config.session_idle_timeout <= 0 {
            config.session_idle_timeout = DEFAULT_SESSION_IDLE_TIMEOUT;
        }
        if config.max_concurrent_requests == 0 {
            config.max_concurrent_requests = DEFAULT_MAX_CONCURRENT_REQUESTS;
        }
        if config.max_response_bytes == 0 {
            config.max_response_bytes = DEFAULT_MAX_SERVER_RESPONSE;
        }
        if config.max_response_bytes < 64 {
            return Err(ChainError::plain(
                "jpip: server response limit is too small",
            ));
        }
        if config.max_query_bytes == 0 {
            config.max_query_bytes = DEFAULT_MAX_QUERY_BYTES;
        }
        if config.max_cache_bytes <= 0 {
            config.max_cache_bytes = DEFAULT_MAX_CACHE_BYTES;
        }
        config.index = config.index.normalized();
        let now = config.now.clone().unwrap_or_else(|| Arc::new(system_now));
        Ok(Server {
            config,
            provider,
            now,
            slots: AtomicUsize::new(0),
            state: Mutex::new(State {
                closed: false,
                sessions: HashMap::new(),
                lru: Vec::new(),
                cached: 0,
            }),
        })
    }
    pub fn close(&self) {
        let mut s = lock(&self.state);
        s.closed = true;
        for (_, sess) in s.sessions.drain() {
            sess.closed.store(true, Ordering::SeqCst);
        }
        s.lru.clear();
        s.cached = 0;
    }
    pub fn sessions(&self) -> usize {
        lock(&self.state).sessions.len()
    }
    fn emit(&self, event: &ServerEvent) {
        if let Some(f) = &self.config.on_event {
            f(event);
        }
    }
    fn acquire_slot(&self) -> bool {
        let max = self.config.max_concurrent_requests;
        let mut n = self.slots.load(Ordering::SeqCst);
        loop {
            if n >= max {
                return false;
            }
            match self
                .slots
                .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(current) => n = current,
            }
        }
    }
    pub fn serve_http(&self, w: &mut dyn ResponseWriter, r: &mut Request) {
        let started = (self.now)();
        let mut event = ServerEvent::default();
        w.header().set("Cache-Control", "no-store");
        let result = self.serve_http_inner(w, r, &mut event);
        if let Err(re) = result {
            event.status = re.status;
            event.reason = re.reason.into();
            if re.status == 503 {
                w.header().set("Retry-After", "1");
            }
            http_error(w, status_text(i64::from(re.status)), re.status);
        }
        event.duration = ((self.now)() - started) as i64;
        self.emit(&event);
    }
    fn serve_http_inner(
        &self,
        w: &mut dyn ResponseWriter,
        r: &mut Request,
        event: &mut ServerEvent,
    ) -> Result<(), RequestError> {
        if r.method != "GET" && r.method != "HEAD" {
            w.header().set("Allow", "GET, HEAD");
            return Err(fail(405, "method"));
        }
        if r.url.raw_query.len() > self.config.max_query_bytes {
            return Err(fail(414, "query-too-long"));
        }
        if !self.acquire_slot() {
            return Err(fail(503, "busy"));
        }
        struct Release<'a>(&'a AtomicUsize);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _release = Release(&self.slots);
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return Err(fail(400, "query"));
        }
        self.serve(w, r, &query, event)
    }
    fn serve(
        &self,
        w: &mut dyn ResponseWriter,
        r: &mut Request,
        query: &urlrules::Values,
        event: &mut ServerEvent,
    ) -> Result<(), RequestError> {
        self.expire_sessions();
        let mut sess: Option<Arc<Session>> = None;
        let cid = query.get("cid");
        if !cid.is_empty() {
            sess = lock(&self.state).sessions.get(cid).cloned();
            if sess.is_none() {
                return Err(fail(404, "unknown-channel"));
            }
        }
        let mut target_name = query.get("target").to_string();
        if let Some(s) = &sess {
            if !target_name.is_empty() && target_name != s.target {
                return Err(fail(400, "channel-target-mismatch"));
            }
            target_name = s.target.clone();
        }
        if target_name.is_empty() {
            return Err(fail(400, "missing-target"));
        }
        let mut new_channel = false;
        let cnew = query.get("cnew");
        if !cnew.is_empty() {
            if !list_contains(cnew, "http") {
                return Err(fail(501, "transport"));
            }
            new_channel = true;
        }
        let mut stream = 1usize;
        let raw = query.get("stream");
        if !raw.is_empty() {
            match atoi(raw) {
                Some(n) if n >= 1 => stream = n as usize,
                _ => return Err(fail(400, "stream")),
            }
        }
        let window = parse_view_window(query)?;
        let entry = self.resolve(r, &target_name)?;
        if stream > entry.target.codestreams.len() {
            return Err(fail(404, "stream"));
        }
        let index = match entry.index(stream - 1, self.config.index) {
            Ok(index) => index,
            Err(e) if e.is(Sentinel::IndexLimit) => return Err(fail(507, "index-limit")),
            Err(_) => return Err(fail(422, "codestream")),
        };
        self.touch(&entry);
        let types = query.get("type");
        if types.is_empty() && sess.is_none() && !new_channel {
            self.serve_raw(w, r, &entry, &index, event);
            return Ok(());
        }
        let Some(kind) = choose_stream_type(types, &index) else {
            return Err(fail(415, "type"));
        };
        if new_channel {
            let opened = self.open_session(&target_name, &entry.tid)?;
            w.header().set(
                "JPIP-cnew",
                &format!("cid={},path={},transport=http", opened.id, r.url.path),
            );
            sess = Some(opened);
            event.sessions = self.sessions();
        }
        let closing = query.get("cclose").to_string();
        let result = self.serve_window(
            w,
            r,
            query,
            event,
            sess.as_deref(),
            &entry,
            &index,
            stream,
            kind,
            &window,
        );
        if !closing.is_empty() {
            self.close_sessions(&closing);
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn serve_window(
        &self,
        w: &mut dyn ResponseWriter,
        r: &Request,
        query: &urlrules::Values,
        event: &mut ServerEvent,
        sess: Option<&Session>,
        entry: &CachedTarget,
        index: &CodestreamIndex,
        stream: usize,
        kind: &str,
        window: &ViewWindow,
    ) -> Result<(), RequestError> {
        let mut own_model = HashMap::new();
        let mut guard = None;
        if let Some(sess) = sess {
            let mut state = lock(&sess.state);
            if sess.closed.load(Ordering::SeqCst) {
                return Err(fail(404, "unknown-channel"));
            }
            state.last_used = (self.now)();
            let tid = query.get("tid");
            if state.tid != entry.tid || !tid.is_empty() && tid != "0" && tid != entry.tid {
                state.model = HashMap::new();
                state.tid = entry.tid.clone();
            }
            guard = Some(state);
        }
        let model = match guard.as_mut() {
            Some(state) => &mut state.model,
            None => &mut own_model,
        };
        let plan = ResponsePlan::new(index, stream as u64, kind, window);
        let body = plan.write(model, window.length, self.config.max_response_bytes);
        let content_type = format!("image/{kind}");
        let h = w.header();
        h.set("Content-Type", &content_type);
        h.set("JPIP-tid", &entry.tid);
        if window.explicit_size {
            h.set("JPIP-fsiz", &format!("{},{}", plan.fsiz_x, plan.fsiz_y));
        }
        if window.length < 0 || window.length > self.config.max_response_bytes as i64 {
            h.set("JPIP-len", &self.config.max_response_bytes.to_string());
        }
        h.set("Content-Length", &body.data.len().to_string());
        w.write_header(200);
        if r.method != "HEAD" {
            let _ = w.write(&body.data);
        }
        event.status = 200;
        event.content_type = content_type;
        event.bytes = body.data.len();
        event.messages = body.messages;
        event.eor = body.eor;
        drop(guard);
        Ok(())
    }
    fn serve_raw(
        &self,
        w: &mut dyn ResponseWriter,
        r: &Request,
        entry: &CachedTarget,
        index: &CodestreamIndex,
        event: &mut ServerEvent,
    ) {
        let content_type = if index.htj2k() {
            "image/jphc"
        } else {
            "image/jp2"
        };
        w.header().set("Content-Type", content_type);
        w.header().set("JPIP-tid", &entry.tid);
        let mut rec = StatusRecorder {
            inner: w,
            status: 0,
            bytes: 0,
        };
        serve_content(&mut rec, r, index.data());
        event.status = rec.status;
        event.content_type = content_type.into();
        event.bytes = rec.bytes;
    }
    fn resolve(&self, r: &Request, name: &str) -> Result<Arc<CachedTarget>, RequestError> {
        {
            let mut s = lock(&self.state);
            if s.closed {
                return Err(fail(503, "closed"));
            }
            if let Some(entry) = s.move_to_front(name) {
                return Ok(entry);
            }
        }
        let target = match self.provider.target(r.context(), name) {
            Ok(t) => t,
            Err(e) => {
                if e.is(Sentinel::TargetNotFound) {
                    return Err(fail(404, "not-found"));
                }
                if r.context().err().is_some() {
                    return Err(fail(503, "canceled"));
                }
                return Err(fail(500, "provider"));
            }
        };
        if target.codestreams.is_empty() {
            return Err(fail(404, "empty-target"));
        }
        let mut bytes = 0i64;
        for codestream in &target.codestreams {
            if codestream.len() > self.config.index.max_codestream_bytes {
                return Err(fail(507, "index-limit"));
            }
            bytes += codestream.len() as i64;
        }
        let entry = Arc::new(CachedTarget {
            name: name.into(),
            tid: target_id(&target),
            target,
            indexes: Mutex::new(Indexes {
                indexes: HashMap::new(),
                errs: HashMap::new(),
            }),
        });
        let mut s = lock(&self.state);
        if let Some(existing) = s.move_to_front(name) {
            return Ok(existing);
        }
        s.lru.insert(0, (entry.clone(), bytes));
        s.cached += bytes;
        s.evict(self.config.max_cache_bytes);
        Ok(entry)
    }
    fn touch(&self, entry: &Arc<CachedTarget>) {
        let mut size: i64 = entry
            .target
            .codestreams
            .iter()
            .map(|c| c.len() as i64)
            .sum();
        for index in lock(&entry.indexes).indexes.values() {
            size += index.packet_count as i64 * 24;
        }
        let mut s = lock(&self.state);
        let max = self.config.max_cache_bytes;
        if let Some(pos) = s.lru.iter().position(|(e, _)| e.name == entry.name)
            && Arc::ptr_eq(&s.lru[pos].0, entry)
        {
            let old = s.lru[pos].1;
            s.cached += size - old;
            s.lru[pos].1 = size;
            s.evict(max);
        }
    }
    fn open_session(&self, target: &str, tid: &str) -> Result<Arc<Session>, RequestError> {
        let id: String = uuid::Uuid::new_v4()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sess = Arc::new(Session {
            id: id.clone(),
            target: target.into(),
            state: Mutex::new(SessionState {
                tid: tid.into(),
                model: HashMap::new(),
                last_used: (self.now)(),
            }),
            closed: AtomicBool::new(false),
        });
        let count = {
            let mut s = lock(&self.state);
            if s.closed {
                return Err(fail(503, "closed"));
            }
            if s.sessions.len() >= self.config.max_sessions {
                return Err(fail(503, "session-limit"));
            }
            s.sessions.insert(id, sess.clone());
            s.sessions.len()
        };
        self.emit(&ServerEvent {
            kind: ServerEventKind::SessionOpened,
            sessions: count,
            ..ServerEvent::default()
        });
        Ok(sess)
    }
    fn close_sessions(&self, ids: &str) {
        let (closed, count) = {
            let mut s = lock(&self.state);
            let mut closed = 0;
            for id in ids.split(',') {
                let id = id.trim();
                if id == "*" {
                    for (_, sess) in s.sessions.drain() {
                        sess.closed.store(true, Ordering::SeqCst);
                        closed += 1;
                    }
                    continue;
                }
                if let Some(sess) = s.sessions.remove(id) {
                    sess.closed.store(true, Ordering::SeqCst);
                    closed += 1;
                }
            }
            (closed, s.sessions.len())
        };
        for _ in 0..closed {
            self.emit(&ServerEvent {
                kind: ServerEventKind::SessionClosed,
                sessions: count,
                ..ServerEvent::default()
            });
        }
    }
    fn expire_sessions(&self) {
        let now = (self.now)();
        let (expired, count) = {
            let mut s = lock(&self.state);
            let mut expired = 0;
            let timeout = i128::from(self.config.session_idle_timeout);
            s.sessions.retain(|_, sess| {
                let Ok(state) = sess.state.try_lock() else {
                    return true;
                };
                if now - state.last_used > timeout {
                    sess.closed.store(true, Ordering::SeqCst);
                    expired += 1;
                    return false;
                }
                true
            });
            (expired, s.sessions.len())
        };
        for _ in 0..expired {
            self.emit(&ServerEvent {
                kind: ServerEventKind::SessionExpired,
                sessions: count,
                ..ServerEvent::default()
            });
        }
    }
}
impl State {
    fn move_to_front(&mut self, name: &str) -> Option<Arc<CachedTarget>> {
        let pos = self.lru.iter().position(|(e, _)| e.name == name)?;
        let item = self.lru.remove(pos);
        let entry = item.0.clone();
        self.lru.insert(0, item);
        Some(entry)
    }
    fn evict(&mut self, max: i64) {
        while self.cached > max && self.lru.len() > 1 {
            if let Some((_, bytes)) = self.lru.pop() {
                self.cached -= bytes;
            }
        }
    }
}
impl CachedTarget {
    fn index(&self, i: usize, limits: IndexLimits) -> Result<Arc<CodestreamIndex>, ChainError> {
        let mut x = lock(&self.indexes);
        if let Some(index) = x.indexes.get(&i) {
            return Ok(index.clone());
        }
        if let Some(e) = x.errs.get(&i) {
            return Err(e.clone());
        }
        match index_codestream(&self.target.codestreams[i], limits) {
            Ok(index) => {
                let index = Arc::new(index);
                x.indexes.insert(i, index.clone());
                Ok(index)
            }
            Err(e) => {
                x.errs.insert(i, e.clone());
                Err(e)
            }
        }
    }
}
struct StatusRecorder<'a> {
    inner: &'a mut dyn ResponseWriter,
    status: u16,
    bytes: usize,
}
impl ResponseWriter for StatusRecorder<'_> {
    fn header(&mut self) -> &mut crate::net::dicomweb::mimeheader::Header {
        self.inner.header()
    }
    fn write_header(&mut self, status: u16) {
        self.status = status;
        self.inner.write_header(status);
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, crate::net::dicomweb::server::ServerError> {
        if self.status == 0 {
            self.status = 200;
        }
        let n = self.inner.write(data)?;
        self.bytes += n;
        Ok(n)
    }
}
fn list_contains(list: &str, want: &str) -> bool {
    list.split(',')
        .any(|item| item.trim().eq_ignore_ascii_case(want))
}
fn choose_stream_type(types: &str, index: &CodestreamIndex) -> Option<&'static str> {
    if types.is_empty() {
        return Some(if index.precincts() {
            "jpp-stream"
        } else {
            "jpt-stream"
        });
    }
    for item in types.split(',') {
        let mut item = item.trim().to_lowercase();
        if let Some(rest) = item.strip_prefix("image/") {
            item = rest.to_string();
        }
        if let Some(semi) = item.find(';') {
            item = item[..semi].trim().to_string();
        }
        match item.as_str() {
            "jpp-stream" if index.precincts() => return Some("jpp-stream"),
            "jpt-stream" => return Some("jpt-stream"),
            _ => {}
        }
    }
    None
}
fn atoi(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
fn parse_pair(raw: &str) -> Option<(i64, i64, String)> {
    let parts: Vec<&str> = raw.split(',').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let a = atoi(parts[0].trim())?;
    let b = atoi(parts[1].trim())?;
    if a < 0 || b < 0 || a > 1 << 32 || b > 1 << 32 {
        return None;
    }
    let extra = if parts.len() == 3 {
        parts[2].trim().to_string()
    } else {
        String::new()
    };
    Some((a, b, extra))
}
fn parse_view_window(query: &urlrules::Values) -> Result<ViewWindow, RequestError> {
    let mut w = ViewWindow::default();
    let raw = query.get("fsiz");
    if !raw.is_empty() {
        let Some((fx, fy, round)) = parse_pair(raw).filter(|(fx, fy, _)| *fx != 0 && *fy != 0)
        else {
            return Err(fail(400, "fsiz"));
        };
        match round.as_str() {
            "" => {}
            "round-down" | "round-up" | "closest" => w.round = round,
            _ => return Err(fail(400, "fsiz")),
        }
        w.explicit_size = true;
        w.fx = fx;
        w.fy = fy;
    }
    let raw = query.get("roff");
    if !raw.is_empty() {
        let Some((x, y, _)) = parse_pair(raw).filter(|(_, _, extra)| extra.is_empty()) else {
            return Err(fail(400, "roff"));
        };
        w.ox = x;
        w.oy = y;
        w.has_offset = true;
    }
    let raw = query.get("rsiz");
    if !raw.is_empty() {
        let Some((x, y, _)) =
            parse_pair(raw).filter(|(x, y, extra)| extra.is_empty() && *x != 0 && *y != 0)
        else {
            return Err(fail(400, "rsiz"));
        };
        w.sx = x;
        w.sy = y;
        w.has_region = true;
    }
    let raw = query.get("comps");
    if !raw.is_empty() {
        let Some(comps) = parse_component_ranges(raw) else {
            return Err(fail(400, "comps"));
        };
        w.comps = Some(comps);
    }
    let raw = query.get("layers");
    if !raw.is_empty() {
        match atoi(raw) {
            Some(n) if n >= 0 => w.layers = n,
            _ => return Err(fail(400, "layers")),
        }
    }
    let raw = query.get("len");
    if !raw.is_empty() {
        match atoi(raw) {
            Some(n) if n >= 0 => w.length = n,
            _ => return Err(fail(400, "len")),
        }
    }
    Ok(w)
}
fn parse_component_ranges(raw: &str) -> Option<Vec<bool>> {
    let mut mask: Vec<bool> = Vec::new();
    for item in raw.split(',') {
        let item = item.trim();
        let (from, to, ranged) = match item.split_once('-') {
            Some((f, t)) => (f, t, true),
            None => (item, "", false),
        };
        let a = atoi(from)?;
        let mut b = a;
        if ranged {
            b = if to.is_empty() {
                MAX_COMPONENTS as i64 - 1
            } else {
                atoi(to)?
            };
        }
        if a < 0 || b < a || b >= MAX_COMPONENTS as i64 {
            return None;
        }
        let (a, b) = (a as usize, b as usize);
        if mask.len() <= b {
            mask.resize(b + 1, false);
        }
        for m in &mut mask[a..=b] {
            *m = true;
        }
    }
    Some(mask)
}
fn trim_ascii_space(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r'])
}
fn scan_etag(s: &str) -> (&str, &str) {
    let s = trim_ascii_space(s);
    let start = if s.starts_with("W/") { 2 } else { 0 };
    let b = s.as_bytes();
    if s.len() - start < 2 || b[start] != b'"' {
        return ("", "");
    }
    for (i, &c) in b.iter().enumerate().skip(start + 1) {
        match c {
            0x21 | 0x23..=0x7e | 0x80..=0xff => {}
            b'"' => return (&s[..i + 1], &s[i + 1..]),
            _ => return ("", ""),
        }
    }
    ("", "")
}
#[derive(PartialEq, Eq)]
enum Cond {
    None,
    True,
    False,
}
fn check_if_match(r: &Request) -> Cond {
    let mut im = r.header.get("If-Match");
    if im.is_empty() {
        return Cond::None;
    }
    loop {
        im = trim_ascii_space(im);
        if im.is_empty() {
            break;
        }
        if let Some(rest) = im.strip_prefix(',') {
            im = rest;
            continue;
        }
        if im.starts_with('*') {
            return Cond::True;
        }
        let (etag, remain) = scan_etag(im);
        if etag.is_empty() {
            break;
        }
        im = remain;
    }
    Cond::False
}
fn check_if_none_match(r: &Request) -> Cond {
    let mut buf = r.header.get("If-None-Match");
    if buf.is_empty() {
        return Cond::None;
    }
    loop {
        buf = trim_ascii_space(buf);
        if buf.is_empty() {
            break;
        }
        if let Some(rest) = buf.strip_prefix(',') {
            buf = rest;
            continue;
        }
        if buf.starts_with('*') {
            return Cond::False;
        }
        let (etag, remain) = scan_etag(buf);
        if etag.is_empty() {
            break;
        }
        if etag.strip_prefix("W/").unwrap_or(etag).is_empty() {
            return Cond::False;
        }
        buf = remain;
    }
    Cond::True
}
struct HttpRange {
    start: i64,
    length: i64,
}
impl HttpRange {
    fn content_range(&self, size: i64) -> String {
        format!(
            "bytes {}-{}/{}",
            self.start,
            self.start + self.length - 1,
            size
        )
    }
}
enum RangeError {
    Invalid,
    NoOverlap,
}
fn parse_range(s: &str, size: i64) -> Result<Vec<HttpRange>, RangeError> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let Some(spec) = s.strip_prefix("bytes=") else {
        return Err(RangeError::Invalid);
    };
    let mut ranges = Vec::new();
    let mut no_overlap = false;
    for ra in spec.split(',') {
        let ra = trim_ascii_space(ra);
        if ra.is_empty() {
            continue;
        }
        let Some((start, end)) = ra.split_once('-') else {
            return Err(RangeError::Invalid);
        };
        let (start, end) = (trim_ascii_space(start), trim_ascii_space(end));
        let mut r = HttpRange {
            start: 0,
            length: 0,
        };
        if start.is_empty() {
            if end.is_empty() || end.starts_with('-') {
                return Err(RangeError::Invalid);
            }
            let mut i = match atoi(end) {
                Some(i) if i >= 0 => i,
                _ => return Err(RangeError::Invalid),
            };
            if i > size {
                i = size;
            }
            r.start = size - i;
            r.length = size - r.start;
        } else {
            let i = match atoi(start) {
                Some(i) if i >= 0 => i,
                _ => return Err(RangeError::Invalid),
            };
            if i >= size {
                no_overlap = true;
                continue;
            }
            r.start = i;
            if end.is_empty() {
                r.length = size - r.start;
            } else {
                let mut i = match atoi(end) {
                    Some(i) if r.start <= i => i,
                    _ => return Err(RangeError::Invalid),
                };
                if i >= size {
                    i = size - 1;
                }
                r.length = i - r.start + 1;
            }
        }
        ranges.push(r);
    }
    if no_overlap && ranges.is_empty() {
        return Err(RangeError::NoOverlap);
    }
    Ok(ranges)
}
fn serve_error(w: &mut dyn ResponseWriter, text: &str, code: u16) {
    for k in ["Cache-Control", "Content-Encoding", "Etag", "Last-Modified"] {
        w.header().del(k);
    }
    http_error(w, text, code);
}
fn multipart_part_header(
    boundary: &str,
    first: bool,
    content_range: &str,
    content_type: &str,
) -> String {
    let mut b = String::new();
    if first {
        b.push_str(&format!("--{boundary}\r\n"));
    } else {
        b.push_str(&format!("\r\n--{boundary}\r\n"));
    }
    b.push_str(&format!(
        "Content-Range: {content_range}\r\nContent-Type: {content_type}\r\n\r\n"
    ));
    b
}
fn serve_content(w: &mut dyn ResponseWriter, r: &Request, content: &[u8]) {
    if check_if_match(r) == Cond::False {
        w.write_header(412);
        return;
    }
    if check_if_none_match(r) == Cond::False {
        let h = w.header();
        h.del("Content-Type");
        h.del("Content-Length");
        h.del("Content-Encoding");
        w.write_header(304);
        return;
    }
    let mut range_header = r.header.get("Range").to_string();
    if !range_header.is_empty() && !r.header.get("If-Range").is_empty() {
        range_header.clear();
    }
    let ctype = w
        .header()
        .values("Content-Type")
        .first()
        .cloned()
        .unwrap_or_default();
    let size = content.len() as i64;
    let mut ranges = match parse_range(&range_header, size) {
        Ok(ranges) => ranges,
        Err(RangeError::NoOverlap) if size == 0 => Vec::new(),
        Err(RangeError::NoOverlap) => {
            w.header().set("Content-Range", &format!("bytes */{size}"));
            serve_error(w, "invalid range: failed to overlap", 416);
            return;
        }
        Err(RangeError::Invalid) => {
            serve_error(w, "invalid range", 416);
            return;
        }
    };
    if ranges.iter().map(|r| r.length).sum::<i64>() > size {
        ranges.clear();
    }
    let mut code = 200;
    let mut send: Vec<u8>;
    match ranges.len() {
        0 => send = content.to_vec(),
        1 => {
            let ra = &ranges[0];
            send = content[ra.start as usize..(ra.start + ra.length) as usize].to_vec();
            code = 206;
            w.header().set("Content-Range", &ra.content_range(size));
        }
        _ => {
            code = 206;
            let boundary: String = uuid::Uuid::new_v4()
                .as_bytes()
                .iter()
                .chain(uuid::Uuid::new_v4().as_bytes()[..14].iter())
                .map(|b| format!("{b:02x}"))
                .collect();
            send = Vec::new();
            for (i, ra) in ranges.iter().enumerate() {
                send.extend_from_slice(
                    multipart_part_header(&boundary, i == 0, &ra.content_range(size), &ctype)
                        .as_bytes(),
                );
                send.extend_from_slice(
                    &content[ra.start as usize..(ra.start + ra.length) as usize],
                );
            }
            send.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
            w.header().set(
                "Content-Type",
                &format!("multipart/byteranges; boundary={boundary}"),
            );
        }
    }
    w.header().set("Accept-Ranges", "bytes");
    if !ranges.is_empty() || w.header().get("Content-Encoding").is_empty() {
        w.header().set("Content-Length", &send.len().to_string());
    }
    w.write_header(code);
    if r.method != "HEAD" {
        let _ = w.write(&send);
    }
}
