use super::super::mimeheader::trim_space;
use super::super::websocket::{
    CLOSE_GOING_AWAY, CLOSE_NORMAL, Conn, OP_TEXT, WsError, accept_key, header_contains_token,
};
use super::ServerError;
use super::http::{Request, ResponseWriter, http_error};
use super::ups::UPS_EVENT_SOP_CLASS_UID;
use crate::dicomjson::jsonwire::{self, JAny, RawElement};
use crate::dicomjson::{Dataset, element_string};
use crate::net::ul::Ctx;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
const DEFAULT_MAX_CONNECTIONS: i64 = 256;
const DEFAULT_MAX_PER_AE_TITLE: i64 = 8;
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_MAX_INCOMING_MESSAGES: i64 = 4 << 10;
pub const ERR_EVENT_CHANNEL_CLOSED: &str = "dicomweb: event channel hub closed";
pub type CheckOriginFn = Arc<dyn Fn(&Request) -> bool + Send + Sync>;
pub(crate) type OnConnectFn = Arc<dyn Fn(&str) + Send + Sync>;
#[derive(Clone, Default)]
pub struct WorkitemEventHubOptions {
    pub max_connections: i64,
    pub max_connections_per_ae_title: i64,
    pub write_timeout_nanos: i64,
    pub ping_interval_nanos: i64,
    pub max_incoming_message_bytes: i64,
    pub check_origin: Option<CheckOriginFn>,
    pub on_connect: Option<OnConnectFn>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkitemEvent {
    pub sop_instance_uid: String,
    pub event_type_id: u16,
    pub information: Dataset,
}
struct Channel {
    id: u64,
    conn: Arc<Conn>,
    ae_title: String,
    write_mu: Mutex<()>,
}
#[derive(Default)]
struct HubState {
    closed: bool,
    total: i64,
    channels: HashMap<String, Vec<Arc<Channel>>>,
    message_id: u16,
    next_id: u64,
    threads: Vec<JoinHandle<()>>,
}
pub struct WorkitemEventHub {
    options: WorkitemEventHubOptions,
    write_timeout: Duration,
    ping_interval: Option<Duration>,
    state: Mutex<HubState>,
}
#[doc(hidden)]
pub fn same_origin_or_absent(r: &Request) -> bool {
    let origin = r.header.get("Origin");
    if origin.is_empty() {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    rest.eq_ignore_ascii_case(&r.host)
}
pub fn accept(
    w: &mut dyn ResponseWriter,
    r: &Request,
    max_message_bytes: i64,
    check_origin: Option<&CheckOriginFn>,
) -> Result<Arc<Conn>, WsError> {
    fn refuse(w: &mut dyn ResponseWriter, status: u16) -> Result<Arc<Conn>, WsError> {
        if status == 426 {
            w.header().set("Sec-WebSocket-Version", "13");
            w.header().set("Upgrade", "websocket");
        }
        http_error(
            w,
            super::super::types::status_text(i64::from(status)),
            status,
        );
        Err(WsError::Handshake)
    }
    let old = r.proto == "HTTP/1.0" || r.proto == "HTTP/0.9";
    if r.method != "GET" || old {
        return refuse(w, 400);
    }
    if !header_contains_token(&r.header, "Connection", "upgrade")
        || !header_contains_token(&r.header, "Upgrade", "websocket")
    {
        return refuse(w, 426);
    }
    if r.header.get("Sec-WebSocket-Version") != "13" {
        return refuse(w, 426);
    }
    let key = trim_space(r.header.get("Sec-WebSocket-Key")).to_string();
    match crate::dicomjson::sizedparse::base64_decode(key.as_bytes(), false) {
        Ok(decoded) if decoded.len() == 16 => {}
        _ => return refuse(w, 400),
    }
    let allowed = match check_origin {
        Some(check) => check(r),
        None => same_origin_or_absent(r),
    };
    if !allowed {
        return refuse(w, 403);
    }
    let Some(hijacked) = w.hijack() else {
        return refuse(w, 500);
    };
    let mut stream = hijacked.stream;
    let _ = stream.set_read_timeout(None);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(&key)
    );
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    use std::io::Write as _;
    let failed = |stream: &std::net::TcpStream, e: std::io::Error| {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        WsError::Io {
            text: e.to_string(),
            timeout: false,
        }
    };
    #[cfg(feature = "tls")]
    if let Some(mut session) = hijacked.tls {
        let written = session
            .writer()
            .write_all(response.as_bytes())
            .and_then(|()| {
                while session.wants_write() {
                    session.write_tls(&mut stream)?;
                }
                Ok(())
            });
        if let Err(e) = written {
            return Err(failed(&stream, e));
        }
        let _ = stream.set_write_timeout(None);
        return Conn::new_tls(
            stream,
            rustls::Connection::Server(*session),
            hijacked.buffered,
            false,
            max_message_bytes,
        )
        .map_err(|e| WsError::Io {
            text: e.to_string(),
            timeout: false,
        });
    }
    if let Err(e) = stream.write_all(response.as_bytes()) {
        return Err(failed(&stream, e));
    }
    let _ = stream.set_write_timeout(None);
    Conn::new(stream, hijacked.buffered, false, max_message_bytes).map_err(|e| WsError::Io {
        text: e.to_string(),
        timeout: false,
    })
}
impl WorkitemEventHub {
    pub fn new(options: WorkitemEventHubOptions) -> Arc<WorkitemEventHub> {
        let mut options = options;
        if options.max_connections <= 0 {
            options.max_connections = DEFAULT_MAX_CONNECTIONS;
        }
        if options.max_connections_per_ae_title <= 0 {
            options.max_connections_per_ae_title = DEFAULT_MAX_PER_AE_TITLE;
        }
        let write_timeout = if options.write_timeout_nanos <= 0 {
            DEFAULT_WRITE_TIMEOUT
        } else {
            Duration::from_nanos(options.write_timeout_nanos as u64)
        };
        let ping_interval = match options.ping_interval_nanos {
            0 => Some(DEFAULT_PING_INTERVAL),
            n if n < 0 => None,
            n => Some(Duration::from_nanos(n as u64)),
        };
        if options.max_incoming_message_bytes <= 0 {
            options.max_incoming_message_bytes = DEFAULT_MAX_INCOMING_MESSAGES;
        }
        Arc::new(WorkitemEventHub {
            options,
            write_timeout,
            ping_interval,
            state: Mutex::new(HubState::default()),
        })
    }
    pub fn connections(&self, ae_title: &str) -> usize {
        self.state
            .lock()
            .map(|s| s.channels.get(ae_title).map_or(0, Vec::len))
            .unwrap_or(0)
    }
    fn full(&self, state: &HubState, ae_title: &str) -> bool {
        state.total >= self.options.max_connections
            || state.channels.get(ae_title).map_or(0, Vec::len) as i64
                >= self.options.max_connections_per_ae_title
    }
    fn admit(&self, ae_title: &str) -> Result<(), ServerError> {
        let Ok(state) = self.state.lock() else {
            return Err(ServerError::Backend);
        };
        if state.closed {
            return Err(ServerError::status(
                503,
                "unavailable",
                ServerError::ResourceLimit,
            ));
        }
        if self.full(&state, ae_title) {
            return Err(ServerError::status(503, "busy", ServerError::ResourceLimit));
        }
        Ok(())
    }
    pub(crate) fn accept(
        self: &Arc<Self>,
        w: &mut dyn ResponseWriter,
        r: &Request,
        ae_title: &str,
    ) -> Result<(), Option<ServerError>> {
        self.admit(ae_title).map_err(Some)?;
        let conn = match accept(
            w,
            r,
            self.options.max_incoming_message_bytes,
            self.options.check_origin.as_ref(),
        ) {
            Ok(conn) => conn,
            Err(WsError::Handshake) => return Err(None),
            Err(e) => return Err(Some(ServerError::Other(e.to_string()))),
        };
        let channel = {
            let Ok(mut state) = self.state.lock() else {
                return Ok(());
            };
            if state.closed || self.full(&state, ae_title) {
                drop(state);
                conn.close(1013, "try again later");
                return Ok(());
            }
            state.next_id += 1;
            let channel = Arc::new(Channel {
                id: state.next_id,
                conn,
                ae_title: ae_title.to_string(),
                write_mu: Mutex::new(()),
            });
            state
                .channels
                .entry(ae_title.to_string())
                .or_default()
                .push(channel.clone());
            state.total += 1;
            let hub = self.clone();
            let running = channel.clone();
            state
                .threads
                .push(std::thread::spawn(move || hub.run(running)));
            channel
        };
        let _ = &channel;
        if let Some(callback) = &self.options.on_connect {
            let callback = callback.clone();
            let ae = ae_title.to_string();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(&ae)));
        }
        Ok(())
    }
    fn run(&self, channel: Arc<Channel>) {
        let pinger = self.ping_interval.map(|interval| {
            let channel = channel.clone();
            let write_timeout = self.write_timeout;
            std::thread::spawn(move || {
                loop {
                    if channel.conn.wait_closed(interval) {
                        return;
                    }
                    let failed = {
                        let _guard = channel.write_mu.lock();
                        channel
                            .conn
                            .set_write_deadline(Some(Instant::now() + write_timeout));
                        channel.conn.write_ping(&[]).is_err()
                    };
                    if failed {
                        channel.conn.close(CLOSE_GOING_AWAY, "");
                        return;
                    }
                }
            })
        });
        while channel.conn.read_message().is_ok() {}
        channel.conn.close(CLOSE_NORMAL, "");
        self.remove(&channel);
        if let Some(pinger) = pinger {
            let _ = pinger.join();
        }
    }
    fn remove(&self, channel: &Channel) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(set) = state.channels.get_mut(&channel.ae_title) else {
            return;
        };
        let before = set.len();
        set.retain(|c| c.id != channel.id);
        if set.len() == before {
            return;
        }
        if set.is_empty() {
            state.channels.remove(&channel.ae_title);
        }
        state.total -= 1;
    }
    pub fn publish(
        &self,
        ctx: &Ctx,
        ae_title: &str,
        event: &WorkitemEvent,
    ) -> Result<usize, ServerError> {
        let (targets, message_id) = {
            let Ok(mut state) = self.state.lock() else {
                return Err(ServerError::Other(ERR_EVENT_CHANNEL_CLOSED.into()));
            };
            if state.closed {
                return Err(ServerError::Other(ERR_EVENT_CHANNEL_CLOSED.into()));
            }
            let targets: Vec<Arc<Channel>> =
                state.channels.get(ae_title).cloned().unwrap_or_default();
            state.message_id = state.message_id.wrapping_add(1);
            if state.message_id == 0 {
                state.message_id = 1;
            }
            (targets, state.message_id)
        };
        if targets.is_empty() {
            return Ok(0);
        }
        let message = encode_workitem_event(event, message_id)?;
        let mut delivered = 0;
        let mut last: Option<ServerError> = None;
        for channel in targets {
            super::ctx_err(ctx)?;
            let mut deadline = Instant::now() + self.write_timeout;
            if let Some(d) = ctx.deadline()
                && d < deadline
            {
                deadline = d;
            }
            let result = {
                let _guard = channel.write_mu.lock();
                channel.conn.set_write_deadline(Some(deadline));
                channel.conn.write_message(OP_TEXT, &message)
            };
            match result {
                Ok(()) => delivered += 1,
                Err(e) => {
                    last = Some(ServerError::Other(e.to_string()));
                    channel.conn.close(CLOSE_GOING_AWAY, "");
                }
            }
        }
        if delivered > 0 {
            return Ok(delivered);
        }
        match last {
            Some(e) => Err(e),
            None => Ok(0),
        }
    }
    pub fn close(&self) {
        let (all, threads) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.closed = true;
            let all: Vec<Arc<Channel>> = state.channels.values().flatten().cloned().collect();
            (all, std::mem::take(&mut state.threads))
        };
        for channel in all {
            channel.conn.close(CLOSE_GOING_AWAY, "server shutdown");
        }
        for thread in threads {
            let _ = thread.join();
        }
    }
}
pub fn encode_workitem_event(
    event: &WorkitemEvent,
    message_id: u16,
) -> Result<Vec<u8>, ServerError> {
    if event.event_type_id == 0 || event.sop_instance_uid.is_empty() {
        return Err(ServerError::InvalidRequest);
    }
    let mut message: Dataset = event.information.clone();
    let mut set = |tag: &str, vr: &str, value: JAny| {
        message.retain(|(k, _)| k != tag);
        message.push((
            tag.to_string(),
            RawElement {
                vr: vr.to_string(),
                value: vec![value],
                ..RawElement::default()
            },
        ));
    };
    set("00000002", "UI", JAny::Str(UPS_EVENT_SOP_CLASS_UID.into()));
    set("00000110", "US", JAny::Number(message_id.to_string()));
    set("00001000", "UI", JAny::Str(event.sop_instance_uid.clone()));
    set(
        "00001002",
        "US",
        JAny::Number(event.event_type_id.to_string()),
    );
    Ok(crate::dicomjson::dataset_stream::marshal_dataset(&message).into_bytes())
}
fn unmarshal_dataset(raw: &[u8]) -> Option<Dataset> {
    let members = jsonwire::as_raw_map(raw).ok()?;
    let mut dataset: Dataset = Vec::with_capacity(members.len());
    for (key, value) in members {
        let element = jsonwire::as_element(value).ok()?;
        dataset.retain(|(k, _)| *k != key);
        dataset.push((key, element));
    }
    Some(dataset)
}
fn whole_json(data: &[u8]) -> Option<&[u8]> {
    let mut dec = jsonwire::Decoder::new(data);
    let value = dec.decode_raw().ok()?;
    match dec.decode_raw() {
        Err(jsonwire::JsonError::Eof) => Some(value),
        _ => None,
    }
}
pub fn decode_workitem_event(data: &[u8]) -> Result<WorkitemEvent, ServerError> {
    let value = whole_json(data).ok_or(ServerError::InvalidRequest)?;
    let dataset = match value.first() {
        Some(b'{') | Some(b'n') => unmarshal_dataset(value),
        _ => None,
    };
    let dataset = match dataset {
        Some(d) => d,
        None => {
            if value.first() != Some(&b'[') {
                return Err(ServerError::InvalidRequest);
            }
            let items = jsonwire::as_raw_array(value).map_err(|_| ServerError::InvalidRequest)?;
            if items.len() != 1 {
                return Err(ServerError::InvalidRequest);
            }
            let item = items[0];
            if !matches!(item.first(), Some(b'{') | Some(b'n')) {
                return Err(ServerError::InvalidRequest);
            }
            unmarshal_dataset(item).ok_or(ServerError::InvalidRequest)?
        }
    };
    let uid = element_string(&dataset, "00001000");
    let event_type = element_string(&dataset, "00001002");
    let type_id = crate::dicomjson::sizedparse::parse_uint(&event_type, 16)
        .map_err(|_| ServerError::InvalidRequest)?;
    if uid.is_empty() || type_id == 0 {
        return Err(ServerError::InvalidRequest);
    }
    let information: Dataset = dataset
        .into_iter()
        .filter(|(tag, _)| !(tag.len() == 8 && tag.starts_with("0000")))
        .collect();
    Ok(WorkitemEvent {
        sop_instance_uid: uid,
        event_type_id: type_id as u16,
        information,
    })
}
