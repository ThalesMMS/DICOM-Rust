use super::errors::{OidcError, Sentinel};
use super::json::{self, Fields};
use super::jwk::{Curve, JsonWebKey, KeySet, PublicKey};
use super::{Clock, b64_decode, b64_encode, trim_space};
use crate::net::dicomweb::server::http::Request;
use crate::net::dicomweb::server::{Authorizer, Operation, ServerError};
use crate::net::ul::{Ctx, Kind, Nanos};
use num_bigint::BigUint;
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::sync::Arc;
use std::time::SystemTime;
pub const MAX_TOKEN_BYTES: usize = 64 << 10;
pub const DEFAULT_CLOCK_SKEW: Nanos = Nanos(60_000_000_000);
const ALLOWED_ALGORITHMS: &[&str] = &[
    "RS256", "RS384", "RS512", "PS256", "PS384", "PS512", "ES256", "ES384", "ES512", "EdDSA",
];
pub fn supported_algorithms(advertised: &[String]) -> Option<Vec<String>> {
    let mut out: Option<Vec<String>> = None;
    for algorithm in advertised {
        let algorithm = trim_space(algorithm);
        if ALLOWED_ALGORITHMS.contains(&algorithm)
            && !out
                .as_ref()
                .is_some_and(|o| o.iter().any(|a| a == algorithm))
        {
            out.get_or_insert_with(Vec::new).push(algorithm.to_string());
        }
    }
    out
}
#[derive(Clone, Default)]
pub struct VerifierConfig {
    pub issuer: String,
    pub audience: String,
    pub skip_audience_check: bool,
    pub algorithms: Vec<String>,
    pub clock_skew: Nanos,
    pub now: Option<Clock>,
}
#[derive(Clone)]
pub struct Verifier {
    keys: Arc<dyn KeySet>,
    issuer: String,
    audience: String,
    skip_audience_check: bool,
    algorithms: Vec<String>,
    clock_skew: i64,
    now: Clock,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdToken {
    pub issuer: String,
    pub subject: String,
    pub audience: Vec<String>,
    pub authorized_party: String,
    pub expiry: Option<i64>,
    pub issued_at: Option<i64>,
    pub not_before: Option<i64>,
    pub nonce: String,
    pub access_token_hash: String,
    pub algorithm: String,
    pub key_id: String,
    payload: Vec<u8>,
}
impl IdToken {
    pub fn claims(&self) -> &[u8] {
        &self.payload
    }
    pub fn string(&self) -> String {
        format!("OIDC token (alg={}, claims redacted)", self.algorithm)
    }
    pub fn verify_access_token(&self, access_token: &str) -> Result<(), OidcError> {
        if self.access_token_hash.is_empty() {
            return Ok(());
        }
        let Some(sum) = digest(&self.algorithm, access_token.as_bytes()) else {
            return Err(OidcError::Sentinel(Sentinel::AccessTokenHashMismatch));
        };
        let want = b64_encode(&sum[..sum.len() / 2]);
        if want.as_bytes() != self.access_token_hash.as_bytes() {
            return Err(OidcError::Sentinel(Sentinel::AccessTokenHashMismatch));
        }
        Ok(())
    }
}
fn digest(algorithm: &str, data: &[u8]) -> Option<Vec<u8>> {
    match algorithm {
        "RS256" | "PS256" | "ES256" => Some(Sha256::digest(data).to_vec()),
        "RS384" | "PS384" | "ES384" => Some(Sha384::digest(data).to_vec()),
        "RS512" | "PS512" | "ES512" | "EdDSA" => Some(Sha512::digest(data).to_vec()),
        _ => None,
    }
}
fn hash_bytes(len: usize, data: &[u8]) -> Vec<u8> {
    match len {
        32 => Sha256::digest(data).to_vec(),
        48 => Sha384::digest(data).to_vec(),
        _ => Sha512::digest(data).to_vec(),
    }
}
fn numeric_date(raw: Option<&[u8]>) -> Option<Option<i64>> {
    let Some(raw) = raw else {
        return Some(None);
    };
    let number = match raw.first() {
        Some(b'"') => {
            let s = json::as_string(raw)?;
            if !json::is_valid_number(&s) {
                return None;
            }
            s
        }
        Some(b'-' | b'0'..=b'9') => String::from_utf8_lossy(raw).into_owned(),
        Some(b'n') => String::new(),
        _ => return None,
    };
    let seconds: f64 = crate::numtext::parse_float(&number).ok()?;
    if !(0.0..=(1u64 << 40) as f64).contains(&seconds) {
        return None;
    }
    Some(Some(seconds as i64))
}
fn audience(raw: Option<&[u8]>) -> Option<Vec<String>> {
    let Some(raw) = raw else {
        return Some(Vec::new());
    };
    if let Some(single) = json::as_string(raw) {
        return Some(vec![single]);
    }
    json::as_strings(raw).map(|v| v.unwrap_or_default())
}
pub(crate) fn key_matches_algorithm(key: &PublicKey, algorithm: &str) -> bool {
    match key {
        PublicKey::Rsa { .. } => algorithm.starts_with("RS") || algorithm.starts_with("PS"),
        PublicKey::Ec { curve, .. } => matches!(
            (algorithm, curve),
            ("ES256", Curve::P256) | ("ES384", Curve::P384) | ("ES512", Curve::P521)
        ),
        PublicKey::Ed25519(_) => algorithm == "EdDSA",
    }
}
fn digest_info_prefix(len: usize) -> &'static [u8] {
    match len {
        32 => &[
            0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x01, 0x05, 0x00, 0x04, 0x20,
        ],
        48 => &[
            0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x02, 0x05, 0x00, 0x04, 0x30,
        ],
        _ => &[
            0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
            0x03, 0x05, 0x00, 0x04, 0x40,
        ],
    }
}
fn rsa_open(n: &BigUint, e: u32, signature: &[u8], size: usize) -> Option<Vec<u8>> {
    let k = (n.bits() as usize).div_ceil(8);
    if signature.len() != k {
        return None;
    }
    let s = BigUint::from_bytes_be(signature);
    if &s >= n {
        return None;
    }
    let m = s.modpow(&BigUint::from(e), n).to_bytes_be();
    if m.len() > size {
        return None;
    }
    let mut out = vec![0u8; size - m.len()];
    out.extend_from_slice(&m);
    Some(out)
}
fn verify_pkcs1v15(n: &BigUint, e: u32, digest: &[u8], signature: &[u8]) -> bool {
    let k = (n.bits() as usize).div_ceil(8);
    let prefix = digest_info_prefix(digest.len());
    let t_len = prefix.len() + digest.len();
    if k < t_len + 11 {
        return false;
    }
    let Some(em) = rsa_open(n, e, signature, k) else {
        return false;
    };
    let mut want = vec![0x00, 0x01];
    want.resize(k - t_len - 1, 0xff);
    want.push(0x00);
    want.extend_from_slice(prefix);
    want.extend_from_slice(digest);
    em == want
}
fn mgf1(seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 64);
    let mut counter: u32 = 0;
    while out.len() < len {
        let mut input = seed.to_vec();
        input.extend_from_slice(&counter.to_be_bytes());
        out.extend_from_slice(&hash_bytes(seed.len(), &input));
        counter += 1;
    }
    out.truncate(len);
    out
}
fn verify_pss(n: &BigUint, e: u32, digest: &[u8], signature: &[u8]) -> bool {
    let k = (n.bits() as usize).div_ceil(8);
    if signature.len() != k {
        return false;
    }
    let em_bits = n.bits() as usize - 1;
    let em_len = em_bits.div_ceil(8);
    let Some(em) = rsa_open(n, e, signature, em_len) else {
        return false;
    };
    let h_len = digest.len();
    let s_len = h_len;
    if em_len < h_len + s_len + 2 || em[em_len - 1] != 0xbc {
        return false;
    }
    let mut db = em[..em_len - h_len - 1].to_vec();
    let h = &em[em_len - h_len - 1..em_len - 1];
    let bit_mask: u8 = 0xff >> (8 * em_len - em_bits);
    if db[0] & !bit_mask != 0 {
        return false;
    }
    for (d, m) in db.iter_mut().zip(mgf1(h, em_len - h_len - 1)) {
        *d ^= m;
    }
    db[0] &= bit_mask;
    let ps_len = em_len - h_len - s_len - 2;
    if db[..ps_len].iter().any(|&b| b != 0) || db[ps_len] != 0x01 {
        return false;
    }
    let salt = &db[db.len() - s_len..];
    let mut prime = vec![0u8; 8];
    prime.extend_from_slice(digest);
    prime.extend_from_slice(salt);
    hash_bytes(h_len, &prime) == h
}
fn verify_ecdsa(curve: Curve, x: &[u8], y: &[u8], digest: &[u8], signature: &[u8]) -> bool {
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    let size = curve.bit_size().div_ceil(8);
    if signature.len() != 2 * size {
        return false;
    }
    let mut point = vec![4u8];
    point.extend_from_slice(x);
    point.extend_from_slice(y);
    match curve {
        Curve::P256 => {
            let (Ok(key), Ok(sig)) = (
                p256::ecdsa::VerifyingKey::from_sec1_bytes(&point),
                p256::ecdsa::Signature::from_slice(signature),
            ) else {
                return false;
            };
            key.verify_prehash(digest, &sig).is_ok()
        }
        Curve::P384 => {
            let (Ok(key), Ok(sig)) = (
                p384::ecdsa::VerifyingKey::from_sec1_bytes(&point),
                p384::ecdsa::Signature::from_slice(signature),
            ) else {
                return false;
            };
            key.verify_prehash(digest, &sig).is_ok()
        }
        Curve::P521 => {
            let (Ok(key), Ok(sig)) = (
                p521::ecdsa::VerifyingKey::from_sec1_bytes(&point),
                p521::ecdsa::Signature::from_slice(signature),
            ) else {
                return false;
            };
            key.verify_prehash(digest, &sig).is_ok()
        }
    }
}
#[cfg(feature = "tls")]
fn verify_ed25519(public: &[u8], input: &[u8], signature: &[u8]) -> bool {
    signature.len() == 64
        && webpki::ring::ED25519
            .verify_signature(public, input, signature)
            .is_ok()
}
#[cfg(not(feature = "tls"))]
fn verify_ed25519(_public: &[u8], _input: &[u8], _signature: &[u8]) -> bool {
    false
}
fn verify_signature(key: &PublicKey, algorithm: &str, input: &[u8], signature: &[u8]) -> bool {
    if algorithm == "EdDSA" {
        return match key {
            PublicKey::Ed25519(public) => verify_ed25519(public, input, signature),
            _ => false,
        };
    }
    let Some(digest) = digest(algorithm, input) else {
        return false;
    };
    match key {
        PublicKey::Rsa { n, e } => {
            if algorithm.starts_with("PS") {
                verify_pss(n, *e, &digest, signature)
            } else {
                verify_pkcs1v15(n, *e, &digest, signature)
            }
        }
        PublicKey::Ec { curve, x, y } => verify_ecdsa(*curve, x, y, &digest, signature),
        PublicKey::Ed25519(_) => false,
    }
}
fn ctx_err(ctx: &Ctx) -> Option<OidcError> {
    match ctx.err() {
        Some(Kind::Canceled) => Some(OidcError::Canceled),
        Some(_) => Some(OidcError::DeadlineExceeded),
        None => None,
    }
}
const CLAIM_FIELDS: &[&str] = &[
    "iss", "sub", "aud", "azp", "exp", "iat", "nbf", "nonce", "at_hash",
];
impl Verifier {
    pub fn new(
        keys: Option<Arc<dyn KeySet>>,
        config: VerifierConfig,
    ) -> Result<Verifier, OidcError> {
        let not_configured = || OidcError::Sentinel(Sentinel::NotConfigured);
        let Some(keys) = keys else {
            return Err(not_configured());
        };
        if trim_space(&config.issuer).is_empty()
            || (!config.skip_audience_check && trim_space(&config.audience).is_empty())
        {
            return Err(not_configured());
        }
        let algorithms = if config.algorithms.is_empty() {
            vec!["RS256".to_string()]
        } else {
            supported_algorithms(&config.algorithms).ok_or_else(not_configured)?
        };
        let clock_skew = match config.clock_skew.0 {
            0 => DEFAULT_CLOCK_SKEW.0,
            s if s < 0 => 0,
            s => s,
        };
        Ok(Verifier {
            keys,
            issuer: config.issuer,
            audience: config.audience,
            skip_audience_check: config.skip_audience_check,
            algorithms,
            clock_skew,
            now: config.now.unwrap_or_else(|| Arc::new(SystemTime::now)),
        })
    }
    pub fn verify(&self, ctx: &Ctx, raw: &str) -> Result<IdToken, OidcError> {
        let fail = |s: Sentinel| Err(OidcError::Sentinel(s));
        let raw = trim_space(raw);
        if raw.is_empty() || raw.len() > MAX_TOKEN_BYTES {
            return fail(Sentinel::MalformedToken);
        }
        let parts: Vec<&str> = raw.split('.').collect();
        if parts.len() != 3 {
            return fail(Sentinel::MalformedToken);
        }
        let (Some(header_bytes), Some(payload), Some(signature)) = (
            b64_decode(parts[0]),
            b64_decode(parts[1]),
            b64_decode(parts[2]),
        ) else {
            return fail(Sentinel::MalformedToken);
        };
        if signature.is_empty() {
            return fail(Sentinel::MalformedToken);
        }
        let header = json::strict_value(&header_bytes)
            .and_then(|raw| Fields::new(raw, &["alg", "kid", "typ", "crit"]))
            .and_then(|f| Some((f.string(0)?, f.string(1)?, f.string(2)?, f.raw(3).is_some())));
        let Some((algorithm, key_id, _typ, critical)) = header else {
            return fail(Sentinel::MalformedToken);
        };
        if critical {
            return fail(Sentinel::UnsupportedAlgorithm);
        }
        if !self.algorithms.contains(&algorithm) {
            return fail(Sentinel::UnsupportedAlgorithm);
        }
        let keys: Vec<JsonWebKey> = match self.keys.keys(ctx, &key_id) {
            Ok(keys) => keys,
            Err(e) => {
                if let Some(c) = ctx_err(ctx) {
                    return Err(c);
                }
                if e.is(Sentinel::InvalidToken) {
                    return Err(e);
                }
                return fail(Sentinel::KeyNotFound);
            }
        };
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let mut verified = false;
        let mut compatible = false;
        for key in &keys {
            if !key.algorithm.is_empty() && key.algorithm != algorithm {
                continue;
            }
            if !key_matches_algorithm(&key.key, &algorithm) {
                continue;
            }
            compatible = true;
            if verify_signature(&key.key, &algorithm, signing_input.as_bytes(), &signature) {
                verified = true;
                break;
            }
        }
        if !compatible {
            return fail(Sentinel::KeyNotFound);
        }
        if !verified {
            return fail(Sentinel::InvalidSignature);
        }
        let claims = json::unmarshal_value(&payload)
            .and_then(|raw| Fields::new(raw, CLAIM_FIELDS))
            .and_then(|f| {
                Some((
                    f.string(0)?,
                    f.string(1)?,
                    audience(f.raw(2))?,
                    f.string(3)?,
                    numeric_date(f.raw(4))?,
                    numeric_date(f.raw(5))?,
                    numeric_date(f.raw(6))?,
                    f.string(7)?,
                    f.string(8)?,
                ))
            });
        let Some((issuer, subject, aud, azp, exp, iat, nbf, nonce, at_hash)) = claims else {
            return fail(Sentinel::MalformedToken);
        };
        if issuer != self.issuer {
            return fail(Sentinel::IssuerMismatch);
        }
        if !self.skip_audience_check {
            if !aud.contains(&self.audience) {
                return fail(Sentinel::AudienceMismatch);
            }
            if !azp.is_empty() && azp != self.audience {
                return fail(Sentinel::AudienceMismatch);
            }
        }
        let now = super::unix_nanos((self.now)());
        let skew = i128::from(self.clock_skew);
        let at = |seconds: i64| i128::from(seconds) * 1_000_000_000;
        let Some(expiry) = exp else {
            return fail(Sentinel::MalformedToken);
        };
        if now >= at(expiry) + skew {
            return fail(Sentinel::TokenExpired);
        }
        if let Some(nbf) = nbf
            && now + skew < at(nbf)
        {
            return fail(Sentinel::TokenNotYetValid);
        }
        if let Some(iat) = iat
            && now + skew < at(iat)
        {
            return fail(Sentinel::TokenNotYetValid);
        }
        if trim_space(&subject).is_empty() {
            return fail(Sentinel::MalformedToken);
        }
        Ok(IdToken {
            issuer,
            subject,
            audience: aud,
            authorized_party: azp,
            expiry: Some(expiry),
            issued_at: iat,
            not_before: nbf,
            nonce,
            access_token_hash: at_hash,
            algorithm,
            key_id,
            payload,
        })
    }
    pub fn authorizer(&self, policy: Option<AccessPolicy>) -> Authorizer {
        let verifier = self.clone();
        Arc::new(move |ctx: &Ctx, request: &Request, operation: Operation| {
            let Some(raw) = bearer_token(Some(request)) else {
                return Err(ServerError::Unauthorized);
            };
            let token = match verifier.verify(ctx, &raw) {
                Ok(token) => token,
                Err(_) => {
                    return match ctx.err() {
                        Some(Kind::Canceled) => Err(ServerError::Canceled),
                        Some(_) => Err(ServerError::DeadlineExceeded),
                        None => Err(ServerError::Unauthorized),
                    };
                }
            };
            match &policy {
                None => Ok(()),
                Some(policy) => {
                    let token = Arc::new(token);
                    let ctx = ctx.with_value(VerifiedToken(token.clone()));
                    policy(&ctx, &token, request, operation)
                }
            }
        })
    }
}
pub type AccessPolicy =
    Arc<dyn Fn(&Ctx, &Arc<IdToken>, &Request, Operation) -> Result<(), ServerError> + Send + Sync>;
#[derive(Clone)]
struct VerifiedToken(Arc<IdToken>);
pub fn token_from_context(ctx: &Ctx) -> Option<Arc<IdToken>> {
    ctx.value::<VerifiedToken>().map(|v| v.0.clone())
}
pub fn bearer_token(request: Option<&Request>) -> Option<String> {
    let request = request?;
    let values = request.header.values("Authorization");
    if values.len() != 1 {
        return None;
    }
    let value = trim_space(&values[0]);
    let (scheme, token) = value.split_once(' ')?;
    if !super::equal_fold(scheme, "Bearer") {
        return None;
    }
    let token = trim_space(token);
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}
