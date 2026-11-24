use super::client::{Client, Token, TokenExpectations};
use super::errors::{self, ERROR_LOGIN, OidcError};
use super::json::Fields;
use super::{
    FLOW_AUTHORIZATION_CODE, FLOW_DEVICE_CODE, Metadata, b64_encode, safe_provider_code, trim_space,
};
use crate::net::dicomweb::server::http::{
    Handler, HttpConfig, Request, ResponseWriter, ServeHandle, http_error, serve,
};
use crate::net::dicomweb::urlrules::{self, Values};
use crate::net::ul::{Ctx, Kind};
use sha2::{Digest, Sha256};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
const POLL: Duration = Duration::from_millis(5);
const MAX_DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(30);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginPrompt {
    pub flow: String,
    pub authorization_url: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub user_code: String,
    pub expires_at: Option<std::time::SystemTime>,
}
impl LoginPrompt {
    pub fn string(&self) -> String {
        let flow = if self.flow.is_empty() {
            "unknown"
        } else {
            &self.flow
        };
        format!("OIDC login prompt (flow={flow}, interaction values redacted)")
    }
    pub fn marshal_json(&self) -> String {
        let mut out = String::from("{");
        if !self.flow.is_empty() {
            out.push_str("\"flow\":");
            crate::dicomjson::jsonwire::append_string(&mut out, &self.flow);
            out.push(',');
        }
        let expiry = self.expires_at.map(super::unix_nanos);
        out.push_str(&format!(
            "\"expiresAt\":\"{}\"}}",
            super::client::rfc3339_nano(expiry)
        ));
        out
    }
}
type WaitFn = Box<dyn FnOnce(&Ctx) -> Result<Token, OidcError> + Send>;
pub struct LoginSession {
    pub prompt: LoginPrompt,
    wait: Mutex<Option<WaitFn>>,
    result: Mutex<Option<Result<Token, OidcError>>>,
    cancel: Arc<dyn Fn() + Send + Sync>,
}
impl LoginSession {
    pub fn string(&self) -> String {
        format!(
            "OIDC login session (flow={}, secrets redacted)",
            self.prompt.flow
        )
    }
    pub fn wait(&self, ctx: &Ctx) -> Result<Token, OidcError> {
        let Ok(mut result) = self.result.lock() else {
            return Err(errors::new_error(ERROR_LOGIN, "wait", 0, "", None));
        };
        if let Some(r) = result.as_ref() {
            return r.clone();
        }
        let wait = self.wait.lock().ok().and_then(|mut w| w.take());
        let Some(wait) = wait else {
            return Err(errors::new_error(ERROR_LOGIN, "wait", 0, "", None));
        };
        let r = wait(ctx);
        self.close();
        *result = Some(r.clone());
        r
    }
    pub fn close(&self) {
        (self.cancel)();
    }
}
impl Drop for LoginSession {
    fn drop(&mut self) {
        self.close();
    }
}
pub fn random_url_token(size: usize) -> Result<String, OidcError> {
    use rand_core::{OsRng, RngCore};
    let mut value = vec![0u8; size];
    OsRng
        .try_fill_bytes(&mut value)
        .map_err(|e| OidcError::Other(e.to_string()))?;
    Ok(b64_encode(&value))
}
pub fn pkce_challenge(verifier: &str) -> String {
    b64_encode(&Sha256::digest(verifier.as_bytes()))
}
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '"' => out.push_str("&#34;"),
            c => out.push(c),
        }
    }
    out
}
fn ctx_err(ctx: &Ctx) -> Option<OidcError> {
    match ctx.err() {
        Some(Kind::Canceled) => Some(OidcError::Canceled),
        Some(_) => Some(OidcError::DeadlineExceeded),
        None => None,
    }
}
struct Callback {
    result: Mutex<Option<Result<String, OidcError>>>,
}
impl Client {
    pub fn begin_login(self: &Arc<Self>, ctx: &Ctx) -> Result<LoginSession, OidcError> {
        let metadata = self.discover(ctx)?;
        if self.config.flow == FLOW_DEVICE_CODE {
            return self.begin_device_login(ctx, metadata);
        }
        self.begin_authorization_code_login(metadata)
    }
    fn begin_authorization_code_login(
        self: &Arc<Self>,
        metadata: Metadata,
    ) -> Result<LoginSession, OidcError> {
        let secrets = (
            random_url_token(32),
            random_url_token(64),
            random_url_token(32),
        );
        let (Ok(state), Ok(verifier), Ok(nonce)) = secrets else {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "generate login secrets",
                0,
                "",
                None,
            ));
        };
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "listen for callback",
                0,
                "",
                None,
            ));
        };
        let Ok(address) = listener.local_addr() else {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "listen for callback",
                0,
                "",
                None,
            ));
        };
        let callback_host = address.to_string();
        let redirect_uri = format!("http://{callback_host}{}", self.config.callback_path);
        let callback = Arc::new(Callback {
            result: Mutex::new(None),
        });
        let closed = Arc::new(AtomicBool::new(false));
        let application = html_escape(&self.config.application_name);
        let handler: Handler = {
            let callback = callback.clone();
            let path = self.config.callback_path.clone();
            let host = callback_host.clone();
            let state = state.clone();
            Arc::new(move |w: &mut dyn ResponseWriter, r: &mut Request| {
                if r.url.path != path {
                    http_error(w, "404 page not found", 404);
                    return;
                }
                if r.method != "GET" || r.host != host {
                    http_error(w, "Invalid OAuth callback target.", 400);
                    return;
                }
                let (query, _) = urlrules::parse_query(&r.url.raw_query);
                if query.get("state") != state {
                    http_error(
                        w,
                        "Invalid OAuth state. Return to the application and retry.",
                        400,
                    );
                    return;
                }
                let code = trim_space(query.get("code")).to_string();
                let provider_code = safe_provider_code(query.get("error"));
                let result = if !provider_code.is_empty() {
                    Err(errors::new_error(
                        ERROR_LOGIN,
                        "authorization callback",
                        0,
                        &provider_code,
                        None,
                    ))
                } else if !query.get("error").is_empty() || code.is_empty() {
                    Err(errors::new_error(
                        ERROR_LOGIN,
                        "authorization callback",
                        0,
                        "",
                        None,
                    ))
                } else {
                    Ok(code)
                };
                let message = if result.is_err() {
                    format!(
                        "Authentication failed. You may close this window and return to {application} to retry."
                    )
                } else {
                    format!(
                        "Authentication completed. You may close this window and return to {application}."
                    )
                };
                let h = w.header();
                h.set("Content-Type", "text/html; charset=utf-8");
                h.set("Cache-Control", "no-store");
                h.set("Referrer-Policy", "no-referrer");
                let _ = w.write(
                    format!("<!doctype html><title>Sign-in</title><p>{message}</p>").as_bytes(),
                );
                if let Ok(mut slot) = callback.result.lock()
                    && slot.is_none()
                {
                    *slot = Some(result);
                }
            })
        };
        let handle = Arc::new(ServeHandle::new());
        let config = HttpConfig {
            read_header_timeout: Duration::from_secs(5),
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(5),
            max_header_bytes: 0,
        };
        let server = {
            let handle = handle.clone();
            std::thread::spawn(move || {
                let _ = serve(listener, handler, config, &handle);
            })
        };
        let server = Arc::new(Mutex::new(Some(server)));
        let cancel: Arc<dyn Fn() + Send + Sync> = {
            let closed = closed.clone();
            let handle = handle.clone();
            Arc::new(move || {
                if !closed.swap(true, Ordering::SeqCst) {
                    handle.stop();
                    let server = server.lock().ok().and_then(|mut s| s.take());
                    if let Some(server) = server
                        && server.thread().id() != std::thread::current().id()
                    {
                        let _ = server.join();
                    }
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while handle.active() > 0 && Instant::now() < deadline {
                        std::thread::sleep(POLL);
                    }
                    handle.close_connections();
                }
            })
        };
        let Ok(mut authorization_url) = urlrules::parse(&metadata.authorization_endpoint) else {
            cancel();
            return Err(errors::new_error(
                ERROR_LOGIN,
                "build authorization request",
                0,
                "",
                None,
            ));
        };
        let (mut query, _) = urlrules::parse_query(&authorization_url.raw_query);
        query.set("response_type", "code");
        query.set("client_id", &self.config.client_id);
        query.set("redirect_uri", &redirect_uri);
        query.set("scope", &self.scope);
        query.set("state", &state);
        query.set("nonce", &nonce);
        query.set("code_challenge", &pkce_challenge(&verifier));
        query.set("code_challenge_method", "S256");
        if !self.config.audience.is_empty() {
            query.set("audience", &self.config.audience);
        }
        if self.config.scopes.iter().any(|s| s == "offline_access") {
            query.set("prompt", "consent");
        }
        authorization_url.raw_query = query.encode();
        let client = self.clone();
        let wait: WaitFn = {
            let closed = closed.clone();
            Box::new(move |ctx: &Ctx| {
                loop {
                    if let Some(e) = ctx_err(ctx) {
                        return Err(e);
                    }
                    if closed.load(Ordering::SeqCst) {
                        return Err(OidcError::Canceled);
                    }
                    let result = callback.result.lock().ok().and_then(|mut s| s.take());
                    if let Some(result) = result {
                        let code = result?;
                        let mut form = Values::new();
                        form.set("grant_type", "authorization_code");
                        form.set("client_id", &client.config.client_id);
                        form.set("code", &code);
                        form.set("redirect_uri", &redirect_uri);
                        form.set("code_verifier", &verifier);
                        return client.exchange(
                            ctx,
                            &metadata,
                            &form,
                            &TokenExpectations {
                                kind: ERROR_LOGIN.into(),
                                nonce: nonce.clone(),
                                ..TokenExpectations::default()
                            },
                        );
                    }
                    std::thread::sleep(POLL);
                }
            })
        };
        Ok(LoginSession {
            prompt: LoginPrompt {
                flow: FLOW_AUTHORIZATION_CODE.into(),
                authorization_url: authorization_url.string(),
                ..LoginPrompt::default()
            },
            wait: Mutex::new(Some(wait)),
            result: Mutex::new(None),
            cancel,
        })
    }
    fn begin_device_login(
        self: &Arc<Self>,
        ctx: &Ctx,
        metadata: Metadata,
    ) -> Result<LoginSession, OidcError> {
        let mut form = Values::new();
        form.set("client_id", &self.config.client_id);
        form.set("scope", &self.scope);
        if !self.config.audience.is_empty() {
            form.set("audience", &self.config.audience);
        }
        let (body, status, code) = self
            .http
            .post_form(ctx, &metadata.device_authorization_endpoint, &form)
            .map_err(|f| errors::context_error(ERROR_LOGIN, "start device login", f))?;
        if !(200..300).contains(&status) {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "start device login",
                status,
                &code,
                None,
            ));
        }
        let device = super::json::unmarshal_value(&body)
            .and_then(|raw| {
                Fields::new(
                    raw,
                    &[
                        "device_code",
                        "user_code",
                        "verification_uri",
                        "verification_uri_complete",
                        "expires_in",
                        "interval",
                    ],
                )
            })
            .and_then(|f| {
                Some((
                    f.string(0)?,
                    f.string(1)?,
                    f.string(2)?,
                    f.string(3)?,
                    f.int(4)?,
                    f.int(5)?,
                ))
            });
        let Some((device_code, user_code, verification_uri, complete, expires_in, interval)) =
            device.filter(|d| {
                !trim_space(&d.0).is_empty() && !trim_space(&d.1).is_empty() && d.4 > 0
            })
        else {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "decode device login",
                status,
                "",
                None,
            ));
        };
        if super::validate_secure_endpoint(&verification_uri).is_err()
            || (!complete.is_empty() && super::validate_secure_endpoint(&complete).is_err())
        {
            return Err(errors::new_error(
                ERROR_LOGIN,
                "validate device login",
                status,
                "",
                None,
            ));
        }
        let interval = if interval <= 0 {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(interval as u64)
        };
        let lifetime = Duration::from_secs(expires_in as u64);
        let expires_at = Instant::now() + lifetime;
        let closed = Arc::new(AtomicBool::new(false));
        let cancel: Arc<dyn Fn() + Send + Sync> = {
            let closed = closed.clone();
            Arc::new(move || closed.store(true, Ordering::SeqCst))
        };
        let client = self.clone();
        let wait: WaitFn = Box::new(move |ctx: &Ctx| {
            client.poll_device_token(ctx, &closed, &metadata, &device_code, expires_at, interval)
        });
        Ok(LoginSession {
            prompt: LoginPrompt {
                flow: FLOW_DEVICE_CODE.into(),
                verification_uri,
                verification_uri_complete: complete,
                user_code,
                expires_at: Some(std::time::SystemTime::now() + lifetime),
                ..LoginPrompt::default()
            },
            wait: Mutex::new(Some(wait)),
            result: Mutex::new(None),
            cancel,
        })
    }
    fn poll_device_token(
        &self,
        ctx: &Ctx,
        closed: &AtomicBool,
        metadata: &Metadata,
        device_code: &str,
        expires_at: Instant,
        mut interval: Duration,
    ) -> Result<Token, OidcError> {
        let expired = || {
            Err(errors::new_error(
                ERROR_LOGIN,
                "poll device login",
                0,
                "expired_token",
                None,
            ))
        };
        let mut delay = interval.min(expires_at.saturating_duration_since(Instant::now()));
        loop {
            let fire = Instant::now() + delay;
            loop {
                if let Some(e) = ctx_err(ctx) {
                    return Err(e);
                }
                if closed.load(Ordering::SeqCst) {
                    return Err(OidcError::Canceled);
                }
                let now = Instant::now();
                if now >= fire {
                    break;
                }
                std::thread::sleep(POLL.min(fire - now));
            }
            if Instant::now() >= expires_at {
                return expired();
            }
            let mut form = Values::new();
            form.set("grant_type", "urn:ietf:params:oauth:grant-type:device_code");
            form.set("device_code", device_code);
            form.set("client_id", &self.config.client_id);
            match self.http.post_form(ctx, &metadata.token_endpoint, &form) {
                Err(_) => {
                    if let Some(e) = ctx_err(ctx) {
                        return Err(e);
                    }
                    interval = (interval * 2).min(MAX_DEVICE_POLL_INTERVAL);
                }
                Ok((body, status, _)) if (200..300).contains(&status) => {
                    return self.decode_token_response(
                        ctx,
                        metadata,
                        &body,
                        &TokenExpectations {
                            kind: ERROR_LOGIN.into(),
                            ..TokenExpectations::default()
                        },
                    );
                }
                Ok((_, _, code)) if code == "authorization_pending" => {}
                Ok((_, _, code)) if code == "slow_down" => interval += Duration::from_secs(5),
                Ok((_, status, code)) => {
                    return Err(errors::new_error(
                        ERROR_LOGIN,
                        "poll device login",
                        status,
                        &code,
                        None,
                    ));
                }
            }
            delay = interval.min(expires_at.saturating_duration_since(Instant::now()));
            if delay.is_zero() {
                return expired();
            }
        }
    }
}
