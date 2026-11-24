use super::client::{AccessToken, BearerTokenSource, TokenError};
use crate::net::ul::{Ctx, Kind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
const REFRESH_SKEW_NANOS: i128 = 30_000_000_000;
const REFRESH_FAILURE_BACKOFF: Duration = Duration::from_secs(1);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(20);
pub type TokenRefreshFn = Arc<dyn Fn(&Ctx) -> Result<AccessToken, TokenError> + Send + Sync>;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenStatus {
    pub authenticated: bool,
    pub expiry: Option<i128>,
    pub refreshing: bool,
}
struct Call {
    result: Mutex<Option<Result<AccessToken, TokenError>>>,
    done: Condvar,
}
struct State {
    token: Option<AccessToken>,
    refresh: Option<TokenRefreshFn>,
    in_flight: Option<Arc<Call>>,
    refresh_failure: Option<TokenError>,
    retry_after: Option<Instant>,
    generation: u64,
}
pub struct TokenManager {
    state: Arc<Mutex<State>>,
}
fn now_nanos() -> i128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}
fn valid_at(token: Option<&AccessToken>, skew: i128) -> bool {
    let Some(token) = token else {
        return false;
    };
    if token.value().is_empty() {
        return false;
    }
    match token.expiry {
        None => true,
        Some(expiry) => now_nanos() + skew < expiry,
    }
}
fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn ctx_error(ctx: &Ctx) -> Option<TokenError> {
    ctx.err().map(|kind| match kind {
        Kind::Canceled => TokenError::Canceled,
        _ => TokenError::DeadlineExceeded,
    })
}
fn constant_time_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
impl TokenManager {
    pub fn new(initial: Option<AccessToken>, refresh: Option<TokenRefreshFn>) -> TokenManager {
        TokenManager {
            state: Arc::new(Mutex::new(State {
                token: initial,
                refresh,
                in_flight: None,
                refresh_failure: None,
                retry_after: None,
                generation: 0,
            })),
        }
    }
    fn wait(ctx: &Ctx, call: &Call) -> Result<AccessToken, TokenError> {
        let mut result = call
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(r) = result.clone() {
                return r;
            }
            if let Some(e) = ctx_error(ctx) {
                return Err(e);
            }
            result = call
                .done
                .wait_timeout(result, Duration::from_millis(5))
                .map(|(g, _)| g)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
    pub fn token(&self, ctx: &Ctx) -> Result<AccessToken, TokenError> {
        let mut state = lock(&self.state);
        if valid_at(state.token.as_ref(), REFRESH_SKEW_NANOS) {
            return state.token.clone().ok_or(TokenError::Unavailable);
        }
        if let Some(call) = state.in_flight.clone() {
            drop(state);
            return Self::wait(ctx, &call);
        }
        let Some(refresh) = state.refresh.clone() else {
            return Err(TokenError::Unavailable);
        };
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        if let (Some(err), Some(after)) = (state.refresh_failure, state.retry_after)
            && Instant::now() < after
        {
            return Err(err);
        }
        state.refresh_failure = None;
        state.retry_after = None;
        let call = Arc::new(Call {
            result: Mutex::new(None),
            done: Condvar::new(),
        });
        state.in_flight = Some(call.clone());
        let generation = state.generation;
        drop(state);
        let shared = self.state.clone();
        let worker_call = call.clone();
        let refresh_ctx = ctx.without_cancel().with_timeout(REFRESH_TIMEOUT);
        std::thread::spawn(move || {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| refresh(&refresh_ctx)))
                    .unwrap_or(Err(TokenError::Unavailable));
            let mut result = match outcome {
                Ok(token) if valid_at(Some(&token), REFRESH_SKEW_NANOS) => Ok(token),
                Ok(_) => Err(TokenError::Unavailable),
                Err(e) => Err(e),
            };
            let mut state = lock(&shared);
            if state.generation != generation {
                result = Err(TokenError::Unavailable);
            } else {
                match &result {
                    Ok(token) => {
                        state.token = Some(token.clone());
                        state.refresh_failure = None;
                        state.retry_after = None;
                    }
                    Err(e) => {
                        state.refresh_failure = Some(*e);
                        state.retry_after = Some(Instant::now() + REFRESH_FAILURE_BACKOFF);
                    }
                }
            }
            *worker_call
                .result
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
            state.in_flight = None;
            worker_call.done.notify_all();
        });
        Self::wait(ctx, &call)
    }
    pub fn invalidate(&self, challenged: &AccessToken) {
        if challenged.value().is_empty() {
            return;
        }
        let mut state = lock(&self.state);
        let current = state
            .token
            .as_ref()
            .map_or("", AccessToken::value)
            .to_string();
        if constant_time_equal(&current, challenged.value()) {
            state.token = None;
            state.refresh_failure = None;
            state.retry_after = None;
            state.generation += 1;
        }
    }
    pub fn logout(&self) {
        let mut state = lock(&self.state);
        state.token = None;
        state.refresh = None;
        state.refresh_failure = None;
        state.retry_after = None;
        state.generation += 1;
    }
    pub fn status(&self) -> TokenStatus {
        let state = lock(&self.state);
        TokenStatus {
            authenticated: valid_at(state.token.as_ref(), 0),
            expiry: state.token.as_ref().and_then(|t| t.expiry),
            refreshing: state.in_flight.is_some(),
        }
    }
    pub fn string(&self) -> String {
        let status = self.status();
        let expiry = match status.expiry {
            None => "none".to_string(),
            Some(nanos) => {
                let seconds = nanos.div_euclid(1_000_000_000) as i64;
                chrono::DateTime::from_timestamp(seconds, 0).map_or_else(
                    || "none".to_string(),
                    |t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                )
            }
        };
        format!(
            "DICOMweb token manager (authenticated={}, refreshing={}, expiry={expiry})",
            status.authenticated, status.refreshing
        )
    }
}
impl BearerTokenSource for TokenManager {
    fn token(&self, ctx: &Ctx) -> Result<AccessToken, TokenError> {
        TokenManager::token(self, ctx)
    }
    fn can_invalidate(&self) -> bool {
        true
    }
    fn invalidate(&self, challenged: &AccessToken) {
        TokenManager::invalidate(self, challenged);
    }
}
