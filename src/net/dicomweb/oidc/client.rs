use super::errors::{
    self, ERROR_CONFIGURATION, ERROR_DISCOVERY, ERROR_LOGIN, ERROR_REFRESH, ERROR_REVOCATION,
};
use super::errors::{OidcError, Sentinel};
use super::json::{self, Fields};
use super::jwk::{KeySet, RemoteKeySet, RemoteKeySetOptions};
use super::jwt::{IdToken, Verifier, VerifierConfig, supported_algorithms};
use super::{
    Clock, FLOW_AUTHORIZATION_CODE, FLOW_DEVICE_CODE, Fail, HttpClient, Metadata, discover_with,
    normalize_issuer, trim_space, unix_nanos,
};
use crate::net::dicomweb::client::AccessToken;
use crate::net::dicomweb::urlrules::Values;
use crate::net::ul::{Ctx, Kind, Nanos};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
#[derive(Clone, Default)]
pub struct Config {
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub audience: String,
    pub flow: String,
    pub http: Option<HttpClient>,
    pub fetch_user_info: bool,
    pub application_name: String,
    pub callback_path: String,
    pub key_set_options: RemoteKeySetOptions,
    pub clock_skew: Nanos,
    pub now: Option<Clock>,
}
pub struct Client {
    pub(super) config: Config,
    pub(super) scope: String,
    pub(super) http: HttpClient,
    state: Mutex<(Option<Metadata>, Option<Verifier>)>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserInfo {
    pub subject: String,
    pub email: String,
    pub preferred_username: String,
    pub name: String,
}
impl UserInfo {
    pub fn account(&self) -> String {
        [
            &self.email,
            &self.preferred_username,
            &self.name,
            &self.subject,
        ]
        .into_iter()
        .map(|v| trim_space(v))
        .find(|v| !v.is_empty())
        .unwrap_or("")
        .to_string()
    }
    fn decode(raw: &[u8]) -> Option<UserInfo> {
        let f = Fields::new(raw, &["sub", "email", "preferred_username", "name"])?;
        Some(UserInfo {
            subject: f.string(0)?,
            email: f.string(1)?,
            preferred_username: f.string(2)?,
            name: f.string(3)?,
        })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Token {
    pub access_token: AccessToken,
    pub id_token: Option<IdToken>,
    pub subject: String,
    pub user_info: Option<UserInfo>,
    pub(super) refresh_token: String,
}
impl Token {
    pub fn new(access_token: AccessToken, refresh_token: &str, subject: &str) -> Token {
        Token {
            access_token,
            refresh_token: trim_space(refresh_token).to_string(),
            subject: subject.to_string(),
            ..Token::default()
        }
    }
    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }
    pub fn string(&self) -> String {
        if self.refresh_token.is_empty() {
            "OIDC token (access token redacted, no refresh token)".into()
        } else {
            "OIDC token (access and refresh tokens redacted)".into()
        }
    }
    pub fn marshal_json(&self) -> String {
        format!(
            "{{\"expiry\":\"{}\"}}",
            rfc3339_nano(self.access_token.expiry)
        )
    }
}
pub(crate) fn rfc3339_nano(nanos: Option<i128>) -> String {
    use chrono::{DateTime, SecondsFormat, Utc};
    match nanos {
        None => "0001-01-01T00:00:00Z".into(),
        Some(n) => {
            let secs = n.div_euclid(1_000_000_000) as i64;
            let sub = n.rem_euclid(1_000_000_000) as u32;
            DateTime::<Utc>::from_timestamp(secs, sub)
                .map(|t| t.to_rfc3339_opts(SecondsFormat::AutoSi, true))
                .unwrap_or_default()
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenExpectations {
    pub kind: String,
    pub nonce: String,
    pub subject: String,
    pub previous_refresh_token: String,
}
pub(super) enum UserInfoError {
    Invalid,
    Other(OidcError),
}
fn ctx_err(ctx: &Ctx) -> Option<OidcError> {
    match ctx.err() {
        Some(Kind::Canceled) => Some(OidcError::Canceled),
        Some(_) => Some(OidcError::DeadlineExceeded),
        None => None,
    }
}
impl Client {
    pub fn new(mut config: Config) -> Result<Client, OidcError> {
        let configuration = |op: &str| Err(errors::new_error(ERROR_CONFIGURATION, op, 0, "", None));
        let Ok(issuer) = normalize_issuer(&config.issuer) else {
            return configuration("validate issuer");
        };
        config.issuer = issuer;
        config.client_id = trim_space(&config.client_id).to_string();
        if config.client_id.is_empty() {
            return configuration("validate client");
        }
        let mut scopes: Vec<String> = Vec::new();
        for scope in &config.scopes {
            for field in scope
                .split(|c: char| c.is_whitespace())
                .filter(|f| !f.is_empty())
            {
                if !scopes.iter().any(|s| s == field) {
                    scopes.push(field.to_string());
                }
            }
        }
        if scopes.is_empty() {
            scopes.push("openid".into());
        }
        if !scopes.iter().any(|s| s == "openid") {
            return configuration("validate scopes");
        }
        config.scopes = scopes;
        match config.flow.as_str() {
            "" => config.flow = FLOW_AUTHORIZATION_CODE.into(),
            FLOW_AUTHORIZATION_CODE | FLOW_DEVICE_CODE => {}
            _ => return configuration("validate flow"),
        }
        config.audience = trim_space(&config.audience).to_string();
        if trim_space(&config.application_name).is_empty() {
            config.application_name = "the application".into();
        }
        if config.callback_path.is_empty() {
            config.callback_path = "/oauth2/callback".into();
        }
        if !config.callback_path.starts_with('/') {
            return configuration("validate callback path");
        }
        let http = config
            .http
            .clone()
            .unwrap_or_else(HttpClient::default_client);
        Ok(Client {
            scope: config.scopes.join(" "),
            http,
            config,
            state: Mutex::new((None, None)),
        })
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn discover(&self, ctx: &Ctx) -> Result<Metadata, OidcError> {
        if let Ok(state) = self.state.lock()
            && let Some(m) = &state.0
        {
            return Ok(m.clone());
        }
        let metadata = discover_with(ctx, &self.http, &self.config.issuer, &self.config.flow)?;
        let Ok(mut state) = self.state.lock() else {
            return Ok(metadata);
        };
        Ok(state.0.get_or_insert(metadata).clone())
    }
    pub fn id_token_verifier(&self, metadata: &Metadata) -> Result<Verifier, OidcError> {
        let Ok(mut state) = self.state.lock() else {
            return Err(errors::new_error(
                ERROR_DISCOVERY,
                "configure key set",
                0,
                "",
                None,
            ));
        };
        if let Some(v) = &state.1 {
            return Ok(v.clone());
        }
        let Ok(keys) = RemoteKeySet::new(
            Some(&self.http),
            &metadata.jwks_uri,
            self.config.key_set_options.clone(),
        ) else {
            return Err(errors::new_error(
                ERROR_DISCOVERY,
                "configure key set",
                0,
                "",
                None,
            ));
        };
        let keys: Arc<dyn KeySet> = Arc::new(keys);
        let algorithms = metadata
            .id_token_signing_algs
            .as_deref()
            .and_then(supported_algorithms)
            .unwrap_or_default();
        let Ok(verifier) = Verifier::new(
            Some(keys),
            VerifierConfig {
                issuer: metadata.issuer.clone(),
                audience: self.config.client_id.clone(),
                skip_audience_check: false,
                algorithms,
                clock_skew: self.config.clock_skew,
                now: self.config.now.clone(),
            },
        ) else {
            return Err(errors::new_error(
                ERROR_DISCOVERY,
                "configure verifier",
                0,
                "",
                None,
            ));
        };
        state.1 = Some(verifier.clone());
        Ok(verifier)
    }
    pub(super) fn now_nanos(&self) -> i128 {
        unix_nanos(match &self.config.now {
            Some(now) => now(),
            None => SystemTime::now(),
        })
    }
    pub fn decode_token_response(
        &self,
        ctx: &Ctx,
        metadata: &Metadata,
        body: &[u8],
        expect: &TokenExpectations,
    ) -> Result<Token, OidcError> {
        let kind = if expect.kind == ERROR_REFRESH {
            ERROR_REFRESH
        } else {
            ERROR_LOGIN
        };
        let response = json::unmarshal_value(body)
            .and_then(|raw| {
                Fields::new(
                    raw,
                    &[
                        "access_token",
                        "token_type",
                        "expires_in",
                        "refresh_token",
                        "id_token",
                    ],
                )
            })
            .and_then(|f| {
                Some((
                    f.string(0)?,
                    f.string(1)?,
                    f.int(2)?,
                    f.string(3)?,
                    f.string(4)?,
                ))
            });
        let decode_error = || Err(errors::new_error(kind, "decode token", 0, "", None));
        let Some((access, token_type, expires_in, refresh, raw_id_token)) = response else {
            return decode_error();
        };
        if trim_space(&access).is_empty() || !super::equal_fold(trim_space(&token_type), "Bearer") {
            return decode_error();
        }
        let expiry =
            (expires_in > 0).then(|| self.now_nanos() + i128::from(expires_in) * 1_000_000_000);
        let Ok(access_token) = AccessToken::new(&access, expiry) else {
            return decode_error();
        };
        let mut subject = trim_space(&expect.subject).to_string();
        let mut token = Token {
            access_token: access_token.clone(),
            ..Token::default()
        };
        let raw_id_token = trim_space(&raw_id_token);
        if raw_id_token.is_empty() {
            if kind == ERROR_LOGIN || subject.is_empty() {
                return Err(errors::new_error_sentinel(
                    kind,
                    "validate ID token",
                    Sentinel::MalformedToken,
                ));
            }
        } else {
            let verifier = self.id_token_verifier(metadata)?;
            let id_token = match verifier.verify(ctx, raw_id_token) {
                Ok(t) => t,
                Err(e) => {
                    if let Some(c) = ctx_err(ctx) {
                        return Err(c);
                    }
                    return Err(errors::new_error(
                        kind,
                        "validate ID token",
                        0,
                        "",
                        Some(&e),
                    ));
                }
            };
            if !expect.nonce.is_empty() && expect.nonce != id_token.nonce {
                return Err(errors::new_error_sentinel(
                    kind,
                    "validate ID token",
                    Sentinel::NonceMismatch,
                ));
            }
            if let Err(e) = id_token.verify_access_token(&access) {
                return Err(errors::new_error(
                    kind,
                    "validate ID token",
                    0,
                    "",
                    Some(&e),
                ));
            }
            if !subject.is_empty() && subject != id_token.subject {
                return Err(errors::new_error_sentinel(
                    kind,
                    "validate ID token subject",
                    Sentinel::SubjectMismatch,
                ));
            }
            subject = id_token.subject.clone();
            token.id_token = Some(id_token);
        }
        token.subject = subject.clone();
        token.refresh_token = trim_space(&refresh).to_string();
        if token.refresh_token.is_empty() {
            token.refresh_token = trim_space(&expect.previous_refresh_token).to_string();
        }
        if self.config.fetch_user_info && !metadata.userinfo_endpoint.is_empty() {
            match self.user_info_at(ctx, metadata, &access_token) {
                Ok(info) => {
                    if subject != trim_space(&info.subject) {
                        return Err(errors::new_error_sentinel(
                            kind,
                            "validate UserInfo subject",
                            Sentinel::SubjectMismatch,
                        ));
                    }
                    token.user_info = Some(info);
                }
                Err(UserInfoError::Invalid) => {
                    return Err(errors::new_error_sentinel(
                        kind,
                        "validate UserInfo subject",
                        Sentinel::SubjectMismatch,
                    ));
                }
                Err(UserInfoError::Other(_)) => {
                    if let Some(c) = ctx_err(ctx) {
                        return Err(c);
                    }
                }
            }
        }
        Ok(token)
    }
    pub fn user_info(&self, ctx: &Ctx, access_token: &AccessToken) -> Result<UserInfo, OidcError> {
        let metadata = self.discover(ctx)?;
        if metadata.userinfo_endpoint.is_empty() {
            return Err(errors::new_error(ERROR_DISCOVERY, "UserInfo", 0, "", None));
        }
        match self.user_info_at(ctx, &metadata, access_token) {
            Ok(info) => Ok(info),
            Err(UserInfoError::Other(e)) => Err(e),
            Err(UserInfoError::Invalid) => Err(errors::new_error(
                ERROR_LOGIN,
                "decode UserInfo",
                0,
                "",
                None,
            )),
        }
    }
    fn user_info_at(
        &self,
        ctx: &Ctx,
        metadata: &Metadata,
        access_token: &AccessToken,
    ) -> Result<UserInfo, UserInfoError> {
        let (status, result) = self.http.get_json(
            ctx,
            &metadata.userinfo_endpoint,
            access_token.value(),
            UserInfo::decode,
        );
        match result {
            Err(fail) => {
                if (200..300).contains(&status) {
                    return Err(UserInfoError::Invalid);
                }
                Err(UserInfoError::Other(errors::context_error(
                    ERROR_LOGIN,
                    "UserInfo",
                    fail,
                )))
            }
            Ok(None) => Err(UserInfoError::Other(errors::new_error(
                ERROR_LOGIN,
                "UserInfo",
                status,
                "",
                None,
            ))),
            Ok(Some(info)) => {
                if trim_space(&info.subject).is_empty() {
                    return Err(UserInfoError::Invalid);
                }
                Ok(info)
            }
        }
    }
    pub(super) fn exchange(
        &self,
        ctx: &Ctx,
        metadata: &Metadata,
        form: &Values,
        expect: &TokenExpectations,
    ) -> Result<Token, OidcError> {
        let (body, status, code) = self
            .http
            .post_form(ctx, &metadata.token_endpoint, form)
            .map_err(|f| errors::context_error(&expect.kind, "exchange token", f))?;
        if !(200..300).contains(&status) {
            return Err(errors::new_error(
                &expect.kind,
                "exchange token",
                status,
                &code,
                None,
            ));
        }
        self.decode_token_response(ctx, metadata, &body, expect)
    }
    pub fn refresh(
        &self,
        ctx: &Ctx,
        refresh_token: &str,
        subject: &str,
    ) -> Result<Token, OidcError> {
        let refresh_token = trim_space(refresh_token);
        if refresh_token.is_empty() {
            return Err(errors::new_error_sentinel(
                ERROR_REFRESH,
                "refresh",
                Sentinel::NoRefreshToken,
            ));
        }
        let metadata = self.discover(ctx)?;
        let mut form = Values::new();
        form.set("grant_type", "refresh_token");
        form.set("refresh_token", refresh_token);
        form.set("client_id", &self.config.client_id);
        self.exchange(
            ctx,
            &metadata,
            &form,
            &TokenExpectations {
                kind: ERROR_REFRESH.into(),
                subject: subject.to_string(),
                previous_refresh_token: refresh_token.to_string(),
                ..TokenExpectations::default()
            },
        )
    }
    pub fn revoke(&self, ctx: &Ctx, refresh_token: &str) -> Result<(), OidcError> {
        let refresh_token = trim_space(refresh_token);
        if refresh_token.is_empty() {
            return Ok(());
        }
        let metadata = self.discover(ctx)?;
        if metadata.revocation_endpoint.is_empty() {
            return Ok(());
        }
        let mut form = Values::new();
        form.set("token", refresh_token);
        form.set("token_type_hint", "refresh_token");
        form.set("client_id", &self.config.client_id);
        let (_, status, code) = self
            .http
            .post_form(ctx, &metadata.revocation_endpoint, &form)
            .map_err(|f: Fail| errors::context_error(ERROR_REVOCATION, "revoke token", f))?;
        if !(200..300).contains(&status) {
            return Err(errors::new_error(
                ERROR_REVOCATION,
                "revoke token",
                status,
                &code,
                None,
            ));
        }
        Ok(())
    }
    pub fn token_refresh_func(
        self: &Arc<Self>,
        store: Option<Arc<dyn RefreshTokenStore>>,
    ) -> OidcRefreshFn {
        let client = self.clone();
        Arc::new(move |ctx: &Ctx| {
            let Some(store) = &store else {
                return Err(OidcError::Sentinel(Sentinel::NoRefreshToken));
            };
            let (refresh_token, subject) = store.load_refresh_token(ctx)?;
            let token = client.refresh(ctx, &refresh_token, &subject)?;
            if token.refresh_token != refresh_token || token.subject != subject {
                store.save_refresh_token(ctx, &token.refresh_token, &token.subject)?;
            }
            Ok(token.access_token)
        })
    }
    #[doc(hidden)]
    pub fn token_refresh_fn(
        self: &Arc<Self>,
        store: Option<Arc<dyn RefreshTokenStore>>,
    ) -> crate::net::dicomweb::TokenRefreshFn {
        use crate::net::dicomweb::client::TokenError;
        let refresh = self.token_refresh_func(store);
        Arc::new(move |ctx: &Ctx| {
            refresh(ctx).map_err(|e| match e {
                OidcError::Canceled => TokenError::Canceled,
                OidcError::DeadlineExceeded => TokenError::DeadlineExceeded,
                e if e.is(Sentinel::NoRefreshToken) => TokenError::SignInRequired,
                _ => TokenError::Unavailable,
            })
        })
    }
}
pub type OidcRefreshFn = Arc<dyn Fn(&Ctx) -> Result<AccessToken, OidcError> + Send + Sync>;
pub trait RefreshTokenStore: Send + Sync {
    fn load_refresh_token(&self, ctx: &Ctx) -> Result<(String, String), OidcError>;
    fn save_refresh_token(
        &self,
        ctx: &Ctx,
        refresh_token: &str,
        subject: &str,
    ) -> Result<(), OidcError>;
}
#[derive(Debug, Default)]
pub struct MemoryRefreshTokenStore {
    state: Mutex<(String, String)>,
}
impl MemoryRefreshTokenStore {
    pub fn new(token: &Token) -> MemoryRefreshTokenStore {
        MemoryRefreshTokenStore {
            state: Mutex::new((token.refresh_token.clone(), token.subject.clone())),
        }
    }
    pub fn string(&self) -> String {
        "OIDC memory refresh-token store (redacted)".into()
    }
}
impl RefreshTokenStore for MemoryRefreshTokenStore {
    fn load_refresh_token(&self, _ctx: &Ctx) -> Result<(String, String), OidcError> {
        let state = self
            .state
            .lock()
            .map_err(|_| OidcError::Sentinel(Sentinel::NoRefreshToken))?;
        if state.0.is_empty() {
            return Err(OidcError::Sentinel(Sentinel::NoRefreshToken));
        }
        Ok(state.clone())
    }
    fn save_refresh_token(
        &self,
        _ctx: &Ctx,
        refresh_token: &str,
        subject: &str,
    ) -> Result<(), OidcError> {
        if let Ok(mut state) = self.state.lock() {
            *state = (refresh_token.to_string(), subject.to_string());
        }
        Ok(())
    }
}
