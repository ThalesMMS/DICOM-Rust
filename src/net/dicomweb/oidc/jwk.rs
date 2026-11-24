use super::errors::{OidcError, Sentinel};
use super::json::{self, Fields};
use super::{Clock, HttpClient, b64_decode, contains_fold, trim_space, validate_secure_endpoint};
use crate::net::ul::{Ctx, Kind, Nanos};
use num_bigint::BigUint;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};
pub const MIN_RSA_KEY_BITS: u64 = 2048;
pub const MAX_JWKS_KEYS: usize = 100;
const DEFAULT_KEY_SET_MAX_AGE: i64 = 3_600_000_000_000;
const DEFAULT_KEY_SET_MIN_REFRESH_WAIT: i64 = 5_000_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    P256,
    P384,
    P521,
}
impl Curve {
    pub fn name(self) -> &'static str {
        match self {
            Curve::P256 => "P-256",
            Curve::P384 => "P-384",
            Curve::P521 => "P-521",
        }
    }
    pub(crate) fn bit_size(self) -> usize {
        match self {
            Curve::P256 => 256,
            Curve::P384 => 384,
            Curve::P521 => 521,
        }
    }
    fn byte_size(self) -> usize {
        self.bit_size().div_ceil(8)
    }
    fn from_name(name: &str) -> Option<Curve> {
        match name {
            "P-256" => Some(Curve::P256),
            "P-384" => Some(Curve::P384),
            "P-521" => Some(Curve::P521),
            _ => None,
        }
    }
    fn on_curve(self, point: &[u8]) -> bool {
        match self {
            Curve::P256 => p256::PublicKey::from_sec1_bytes(point).is_ok(),
            Curve::P384 => p384::PublicKey::from_sec1_bytes(point).is_ok(),
            Curve::P521 => p521::PublicKey::from_sec1_bytes(point).is_ok(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicKey {
    Rsa {
        n: BigUint,
        e: u32,
    },
    Ec {
        curve: Curve,
        x: Vec<u8>,
        y: Vec<u8>,
    },
    Ed25519(Vec<u8>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonWebKey {
    pub key_id: String,
    pub algorithm: String,
    pub r#use: String,
    pub key: PublicKey,
}
const JWK_FIELDS: &[&str] = &[
    "kty", "kid", "alg", "use", "key_ops", "crv", "n", "e", "x", "y", "d",
];
pub fn parse_jwks(data: &[u8]) -> Result<Vec<JsonWebKey>, OidcError> {
    let not_found = || OidcError::Sentinel(Sentinel::KeyNotFound);
    let raw = json::strict_value(data).ok_or_else(not_found)?;
    let document = Fields::new(raw, &["keys"]).ok_or_else(not_found)?;
    let entries = match document.raw(0) {
        None => Vec::new(),
        Some(raw) => match raw.first() {
            Some(b'n') => Vec::new(),
            Some(b'[') => crate::dicomjson::jsonwire::as_raw_array(raw).map_err(|_| not_found())?,
            _ => return Err(not_found()),
        },
    };
    let mut keys = Vec::new();
    for raw in entries {
        if keys.len() >= MAX_JWKS_KEYS {
            break;
        }
        if let Some(key) = parse_jwk(raw) {
            keys.push(key);
        }
    }
    if keys.is_empty() {
        return Err(not_found());
    }
    Ok(keys)
}
fn parse_jwk(raw: &[u8]) -> Option<JsonWebKey> {
    let f = Fields::new(raw, JWK_FIELDS)?;
    let text: Vec<String> = [0, 1, 2, 3, 5, 6, 7, 8, 9, 10]
        .iter()
        .map(|&i| f.string(i))
        .collect::<Option<_>>()?;
    let key_ops = f.strings(4)?.unwrap_or_default();
    let [kty, kid, alg, usage, crv, n, e, x, y, d] = &text[..] else {
        return None;
    };
    if !d.is_empty() {
        return None;
    }
    if !usage.is_empty() && usage != "sig" {
        return None;
    }
    if !key_ops.is_empty() && !contains_fold(&key_ops, "verify") {
        return None;
    }
    let key = match kty.as_str() {
        "RSA" => {
            let n = decode_big(n)?;
            let e = decode_big(e)?;
            let e: u32 = u32::try_from(&e).ok()?;
            if !(3..=0x7fff_ffff).contains(&e) || e.is_multiple_of(2) {
                return None;
            }
            if n.bits() < MIN_RSA_KEY_BITS {
                return None;
            }
            PublicKey::Rsa { n, e }
        }
        "EC" => {
            let curve = Curve::from_name(crv)?;
            let x = decode_fixed(x, curve.byte_size())?;
            let y = decode_fixed(y, curve.byte_size())?;
            let mut point = vec![4u8];
            point.extend_from_slice(&x);
            point.extend_from_slice(&y);
            if !curve.on_curve(&point) {
                return None;
            }
            PublicKey::Ec { curve, x, y }
        }
        "OKP" => {
            if crv != "Ed25519" {
                return None;
            }
            PublicKey::Ed25519(decode_fixed(x, 32)?)
        }
        _ => return None,
    };
    if !alg.is_empty() && !super::jwt::key_matches_algorithm(&key, alg) {
        return None;
    }
    Some(JsonWebKey {
        key_id: kid.clone(),
        algorithm: alg.clone(),
        r#use: usage.clone(),
        key,
    })
}
fn decode_big(value: &str) -> Option<BigUint> {
    let data = b64_decode(value)?;
    if data.is_empty() {
        return None;
    }
    Some(BigUint::from_bytes_be(&data))
}
fn decode_fixed(value: &str, size: usize) -> Option<Vec<u8>> {
    b64_decode(value).filter(|d| d.len() == size)
}
pub trait KeySet: Send + Sync {
    fn keys(&self, ctx: &Ctx, key_id: &str) -> Result<Vec<JsonWebKey>, OidcError>;
}
fn matching_keys(keys: &[JsonWebKey], key_id: &str) -> Vec<JsonWebKey> {
    keys.iter()
        .filter(|k| key_id.is_empty() || k.key_id == key_id)
        .cloned()
        .collect()
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticKeySet(pub Vec<JsonWebKey>);
impl KeySet for StaticKeySet {
    fn keys(&self, _ctx: &Ctx, key_id: &str) -> Result<Vec<JsonWebKey>, OidcError> {
        let matches = matching_keys(&self.0, key_id);
        if matches.is_empty() {
            return Err(OidcError::Sentinel(Sentinel::KeyNotFound));
        }
        Ok(matches)
    }
}
#[derive(Clone, Default)]
pub struct RemoteKeySetOptions {
    pub max_age: Nanos,
    pub min_refresh_interval: Nanos,
    pub now: Option<Clock>,
}
impl std::fmt::Debug for RemoteKeySetOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteKeySetOptions")
            .field("max_age", &self.max_age)
            .field("min_refresh_interval", &self.min_refresh_interval)
            .field("now", &self.now.is_some())
            .finish()
    }
}
struct Fetch {
    result: Mutex<Option<Result<Vec<JsonWebKey>, OidcError>>>,
    done: Condvar,
}
#[derive(Default)]
struct RemoteState {
    keys: Vec<JsonWebKey>,
    fetched_at: Option<SystemTime>,
    in_flight: Option<Arc<Fetch>>,
}
pub struct RemoteKeySet {
    uri: String,
    http: HttpClient,
    max_age: i64,
    min_refresh: i64,
    now: Clock,
    state: Arc<Mutex<RemoteState>>,
}
fn since_nanos(now: SystemTime, then: SystemTime) -> i64 {
    match now.duration_since(then) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}
impl RemoteKeySet {
    pub fn new(
        http: Option<&HttpClient>,
        jwks_uri: &str,
        options: RemoteKeySetOptions,
    ) -> Result<RemoteKeySet, OidcError> {
        if validate_secure_endpoint(jwks_uri).is_err() {
            return Err(OidcError::Sentinel(Sentinel::NotConfigured));
        }
        let max_age = if options.max_age.0 <= 0 {
            DEFAULT_KEY_SET_MAX_AGE
        } else {
            options.max_age.0
        };
        let min_refresh = if options.min_refresh_interval.0 == 0 {
            DEFAULT_KEY_SET_MIN_REFRESH_WAIT
        } else {
            options.min_refresh_interval.0
        };
        Ok(RemoteKeySet {
            uri: trim_space(jwks_uri).to_string(),
            http: http.cloned().unwrap_or_else(HttpClient::default_client),
            max_age,
            min_refresh,
            now: options.now.unwrap_or_else(|| Arc::new(SystemTime::now)),
            state: Arc::new(Mutex::new(RemoteState::default())),
        })
    }
    fn download(http: &HttpClient, uri: &str) -> Result<Vec<JsonWebKey>, OidcError> {
        let ctx = Ctx::background().with_timeout(Duration::from_nanos(
            super::DEFAULT_HTTP_TIMEOUT.as_nanos() as u64,
        ));
        let data = http
            .get_raw(&ctx, uri, "application/jwk-set+json, application/json")
            .map_err(|_| OidcError::Sentinel(Sentinel::KeyNotFound))?;
        parse_jwks(&data)
    }
}
impl KeySet for RemoteKeySet {
    fn keys(&self, ctx: &Ctx, key_id: &str) -> Result<Vec<JsonWebKey>, OidcError> {
        let call = {
            let Ok(mut state) = self.state.lock() else {
                return Err(OidcError::Sentinel(Sentinel::KeyNotFound));
            };
            let now = (self.now)();
            if let Some(at) = state.fetched_at
                && since_nanos(now, at) < self.max_age
            {
                let matches = matching_keys(&state.keys, key_id);
                if !matches.is_empty() {
                    return Ok(matches);
                }
                if self.min_refresh > 0 && since_nanos(now, at) < self.min_refresh {
                    return Err(OidcError::Sentinel(Sentinel::KeyNotFound));
                }
            }
            match &state.in_flight {
                Some(call) => call.clone(),
                None => {
                    let call = Arc::new(Fetch {
                        result: Mutex::new(None),
                        done: Condvar::new(),
                    });
                    state.in_flight = Some(call.clone());
                    let (http, uri, now, shared, fetch) = (
                        self.http.clone(),
                        self.uri.clone(),
                        self.now.clone(),
                        self.state.clone(),
                        call.clone(),
                    );
                    std::thread::spawn(move || {
                        let result = RemoteKeySet::download(&http, &uri);
                        if let Ok(mut state) = shared.lock() {
                            if let Ok(keys) = &result {
                                state.keys = keys.clone();
                                state.fetched_at = Some(now());
                            }
                            state.in_flight = None;
                        }
                        if let Ok(mut slot) = fetch.result.lock() {
                            *slot = Some(result);
                        }
                        fetch.done.notify_all();
                    });
                    call
                }
            }
        };
        let Ok(mut slot) = call.result.lock() else {
            return Err(OidcError::Sentinel(Sentinel::KeyNotFound));
        };
        loop {
            if let Some(result) = slot.as_ref() {
                let keys = result.clone()?;
                let matches = matching_keys(&keys, key_id);
                if matches.is_empty() {
                    return Err(OidcError::Sentinel(Sentinel::KeyNotFound));
                }
                return Ok(matches);
            }
            match ctx.err() {
                Some(Kind::Canceled) => return Err(OidcError::Canceled),
                Some(_) => return Err(OidcError::DeadlineExceeded),
                None => {}
            }
            slot = match call.done.wait_timeout(slot, Duration::from_millis(5)) {
                Ok((guard, _)) => guard,
                Err(_) => return Err(OidcError::Sentinel(Sentinel::KeyNotFound)),
            };
        }
    }
}
