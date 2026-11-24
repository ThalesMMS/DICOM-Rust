#![forbid(unsafe_code)]
use super::error::TlsRevocationKind;
use super::error::{Detail, Error, Kind, Result, TlsFailureClass, TlsRevocationError};
use super::tls_crl::{CertInfo, TlsCrlMode, TlsRevocationOptions, TlsRevocationSet};
use crate::textfmt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName, TrustAnchor, UnixTime,
};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::sign::CertifiedKey;
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, SignatureScheme};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::SystemTime;
use x509_parser::prelude::{FromDer, X509Certificate};
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TlsProfile(Cow<'static, str>);
impl TlsProfile {
    pub const BCP195_RFC8996: TlsProfile = TlsProfile(Cow::Borrowed("bcp195-rfc8996"));
    #[doc(hidden)]
    pub const MODIFIED_BCP195_RFC8996: TlsProfile =
        TlsProfile(Cow::Borrowed("bcp195-rfc8996-modified"));
    #[doc(hidden)]
    pub const BCP195: TlsProfile = TlsProfile(Cow::Borrowed("bcp195"));
    #[doc(hidden)]
    pub const NON_DOWNGRADING_BCP195: TlsProfile = TlsProfile(Cow::Borrowed("bcp195-nd"));
    #[doc(hidden)]
    pub const EXTENDED_BCP195: TlsProfile = TlsProfile(Cow::Borrowed("bcp195-extended"));
    pub fn new(name: impl Into<String>) -> TlsProfile {
        TlsProfile(Cow::Owned(name.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn spec(&self) -> Result<&'static ProfileSpec> {
        let name = if self.0.is_empty() {
            DEFAULT_TLS_PROFILE.as_str()
        } else {
            self.as_str()
        };
        PROFILE_SPECS
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| unknown_profile(self.as_str()))
    }
    pub fn section(&self) -> &'static str {
        self.spec().map(|s| s.section).unwrap_or("")
    }
    pub fn title(&self) -> &'static str {
        self.spec().map(|s| s.title).unwrap_or("")
    }
    pub fn cipher_suites(&self) -> Vec<u16> {
        self.spec().map(|s| s.suites.to_vec()).unwrap_or_default()
    }
    pub fn unsupported_cipher_suites(&self) -> Vec<&'static str> {
        self.spec()
            .map(|s| s.unsupported.to_vec())
            .unwrap_or_default()
    }
}
pub const DEFAULT_TLS_PROFILE: TlsProfile = TlsProfile::BCP195_RFC8996;
struct ProfileSpec {
    name: &'static str,
    section: &'static str,
    title: &'static str,
    suites: &'static [u16],
    unsupported: &'static [&'static str],
}
pub const TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256: u16 = 0xc02b;
pub const TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256: u16 = 0xc02f;
pub const TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384: u16 = 0xc02c;
pub const TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384: u16 = 0xc030;
pub const TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256: u16 = 0xcca9;
pub const TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256: u16 = 0xcca8;
pub const TLS_AES_128_GCM_SHA256: u16 = 0x1301;
pub const TLS_AES_256_GCM_SHA384: u16 = 0x1302;
pub const TLS_CHACHA20_POLY1305_SHA256: u16 = 0x1303;
pub const VERSION_TLS12: u16 = 0x0303;
pub const VERSION_TLS13: u16 = 0x0304;
const DHE_UNSUPPORTED: &[&str] = &[
    "TLS_DHE_RSA_WITH_AES_128_GCM_SHA256",
    "TLS_DHE_RSA_WITH_AES_256_GCM_SHA384",
];
static PROFILE_SPECS: [ProfileSpec; 5] = [
    ProfileSpec {
        name: "bcp195-rfc8996",
        section: "B.12",
        title: "BCP 195 RFC 8996, 9325 TLS Secure Transport Connection Profile",
        suites: &[
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        ],
        unsupported: &[],
    },
    ProfileSpec {
        name: "bcp195-rfc8996-modified",
        section: "B.13",
        title: "Modified BCP 195 RFC 8996, 9325 TLS Secure Transport Connection Profile",
        suites: &[
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        ],
        unsupported: &[
            "TLS_AES_128_CCM_SHA256",
            "TLS_AES_128_CCM_8_SHA256",
            "TLS_ECDHE_ECDSA_WITH_AES_256_CCM",
            "TLS_ECDHE_ECDSA_WITH_AES_256_CCM_8",
            "TLS_ECDHE_ECDSA_WITH_AES_128_CCM",
            "TLS_ECDHE_ECDSA_WITH_AES_128_CCM_8",
        ],
    },
    ProfileSpec {
        name: "bcp195-nd",
        section: "B.10 (retired)",
        title: "Non-Downgrading BCP 195 TLS Secure Transport Connection Profile",
        suites: &[
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        ],
        unsupported: DHE_UNSUPPORTED,
    },
    ProfileSpec {
        name: "bcp195",
        section: "B.9 (retired)",
        title: "BCP 195 TLS Secure Transport Connection Profile",
        suites: &[
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
        ],
        unsupported: DHE_UNSUPPORTED,
    },
    ProfileSpec {
        name: "bcp195-extended",
        section: "B.11 (retired)",
        title: "Extended BCP 195 TLS Profile Secure Transport Connection Profile",
        suites: &[
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
        ],
        unsupported: DHE_UNSUPPORTED,
    },
];
const PROFILE_ALIASES: [(&str, &str); 5] = [
    ("8996", "bcp195-rfc8996"),
    ("8996-mod", "bcp195-rfc8996-modified"),
    ("bcp195-ex", "bcp195-extended"),
    ("bcp195-nd", "bcp195-nd"),
    ("bcp195-8996", "bcp195-rfc8996"),
];
fn unknown_profile(name: &str) -> Error {
    Error::with_kinds(
        format!(
            "{} {}",
            Kind::UnknownTlsProfile.text(),
            textfmt::quote(name)
        ),
        vec![Kind::UnknownTlsProfile],
    )
}
pub fn tls_profiles() -> Vec<TlsProfile> {
    PROFILE_SPECS
        .iter()
        .map(|s| TlsProfile(Cow::Borrowed(s.name)))
        .collect()
}
fn trim_lower(name: &str) -> String {
    trim_unicode_space(name)
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n'
                | '\u{b}'
                | '\u{c}'
                | '\r'
                | ' '
                | '\u{85}'
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    })
}
pub fn parse_tls_profile(name: &str) -> Result<TlsProfile> {
    let name = trim_lower(name);
    if name.is_empty() {
        return Ok(DEFAULT_TLS_PROFILE);
    }
    if let Some(spec) = PROFILE_SPECS.iter().find(|s| s.name == name) {
        return Ok(TlsProfile(Cow::Borrowed(spec.name)));
    }
    if let Some((_, target)) = PROFILE_ALIASES.iter().find(|(alias, _)| *alias == name) {
        return Ok(TlsProfile(Cow::Borrowed(target)));
    }
    Err(unknown_profile(&name))
}
#[doc(hidden)]
pub fn tls_cipher_suite_name(id: u16) -> String {
    match id {
        TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 => "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256".into(),
        TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 => "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".into(),
        TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 => "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384".into(),
        TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384 => "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384".into(),
        TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 => {
            "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256".into()
        }
        TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256 => {
            "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256".into()
        }
        TLS_AES_128_GCM_SHA256 => "TLS_AES_128_GCM_SHA256".into(),
        TLS_AES_256_GCM_SHA384 => "TLS_AES_256_GCM_SHA384".into(),
        TLS_CHACHA20_POLY1305_SHA256 => "TLS_CHACHA20_POLY1305_SHA256".into(),
        other => format!("0x{other:04X}"),
    }
}
#[doc(hidden)]
pub fn tls_version_name(version: u16) -> String {
    match version {
        0x0301 => "TLS 1.0".into(),
        0x0302 => "TLS 1.1".into(),
        VERSION_TLS12 => "TLS 1.2".into(),
        VERSION_TLS13 => "TLS 1.3".into(),
        other => format!("0x{other:04X}"),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsPeerAuthentication {
    #[default]
    RequirePeerCertificate,
    VerifyPeerCertificateIfGiven,
    IgnorePeerCertificate,
    Other(i64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TlsVersion {
    Tls12,
    Tls13,
}
impl TlsVersion {
    pub fn id(self) -> u16 {
        match self {
            TlsVersion::Tls12 => VERSION_TLS12,
            TlsVersion::Tls13 => VERSION_TLS13,
        }
    }
}
#[derive(Debug)]
pub struct TlsCertificate {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}
impl Clone for TlsCertificate {
    fn clone(&self) -> Self {
        TlsCertificate {
            chain: self.chain.clone(),
            key: self.key.clone_key(),
        }
    }
}
impl TlsCertificate {
    pub fn from_pem(certificates: &[u8], key: &[u8]) -> Result<TlsCertificate> {
        let chain = certificates_from_pem(certificates)?;
        if chain.is_empty() {
            return Err(Error::other(
                "tls: failed to find any PEM data in certificate input",
            ));
        }
        let key = PrivateKeyDer::from_pem_slice(key)
            .map_err(|e| Error::other(format!("tls: failed to parse private key: {e}")))?;
        check_key_pair(&chain[0], &key)?;
        Ok(TlsCertificate { chain, key })
    }
}
fn check_key_pair(leaf: &CertificateDer<'_>, key: &PrivateKeyDer<'_>) -> Result<()> {
    const RSA: &str = "1.2.840.113549.1.1.1";
    const EC: &str = "1.2.840.10045.2.1";
    const ED25519: &str = "1.3.101.112";
    let (_, cert) = X509Certificate::from_der(leaf.as_ref())
        .map_err(|_| Error::other("x509: malformed certificate"))?;
    let cert_algorithm = cert.public_key().algorithm.algorithm.to_id_string();
    if !matches!(cert_algorithm.as_str(), RSA | EC | ED25519) {
        return Err(Error::other("tls: unknown public key algorithm"));
    }
    let key_algorithm = match key {
        PrivateKeyDer::Pkcs1(_) => Some(RSA),
        PrivateKeyDer::Sec1(_) => Some(EC),
        PrivateKeyDer::Pkcs8(k) => pkcs8_algorithm(k.secret_pkcs8_der()).and_then(|oid| {
            [
                (
                    &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01][..],
                    RSA,
                ),
                (&[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01][..], EC),
                (&[0x2b, 0x65, 0x70][..], ED25519),
            ]
            .iter()
            .find(|(der, _)| *der == oid)
            .map(|(_, name)| *name)
        }),
        _ => None,
    };
    if key_algorithm.is_some_and(|k| k != cert_algorithm) {
        return Err(Error::other(
            "tls: private key type does not match public key type",
        ));
    }
    let mismatch = || Error::other("tls: private key does not match public key");
    if cert_algorithm == RSA {
        let x509_parser::public_key::PublicKey::RSA(public) = cert
            .public_key()
            .parsed()
            .map_err(|_| Error::other("x509: malformed certificate"))?
        else {
            return Err(Error::other("x509: malformed certificate"));
        };
        if let Some((modulus, exponent)) = rsa_private_public_part(key)
            && (unsigned(modulus) != unsigned(public.modulus)
                || unsigned(exponent) != unsigned(public.exponent))
        {
            return Err(mismatch());
        }
        return Ok(());
    }
    if let Ok(signing) = rustls::crypto::ring::sign::any_supported_type(key)
        && let Some(spki) = signing.public_key()
        && spki.as_ref() != cert.public_key().raw
    {
        return Err(mismatch());
    }
    Ok(())
}
fn der_element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let length = rest[..n]
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
        (length, &rest[n..])
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}
fn pkcs8_algorithm(der: &[u8]) -> Option<&[u8]> {
    let (0x30, info, _) = der_element(der)? else {
        return None;
    };
    let (0x02, _, rest) = der_element(info)? else {
        return None;
    };
    let (0x30, algorithm, _) = der_element(rest)? else {
        return None;
    };
    let (0x06, oid, _) = der_element(algorithm)? else {
        return None;
    };
    Some(oid)
}
fn rsa_private_public_part<'a>(key: &'a PrivateKeyDer<'_>) -> Option<(&'a [u8], &'a [u8])> {
    let pkcs1 = match key {
        PrivateKeyDer::Pkcs1(k) => k.secret_pkcs1_der(),
        PrivateKeyDer::Pkcs8(k) => {
            let (0x30, info, _) = der_element(k.secret_pkcs8_der())? else {
                return None;
            };
            let (0x02, _, rest) = der_element(info)? else {
                return None;
            };
            let (0x30, _, rest) = der_element(rest)? else {
                return None;
            };
            let (0x04, private, _) = der_element(rest)? else {
                return None;
            };
            private
        }
        _ => return None,
    };
    let (0x30, body, _) = der_element(pkcs1)? else {
        return None;
    };
    let (0x02, _, rest) = der_element(body)? else {
        return None;
    };
    let (0x02, modulus, rest) = der_element(rest)? else {
        return None;
    };
    let (0x02, exponent, _) = der_element(rest)? else {
        return None;
    };
    Some((modulus, exponent))
}
fn unsigned(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}
pub fn certificates_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_slice_iter(pem)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::other(format!("x509: malformed PEM certificate: {e}")))
}
pub fn crls_from_pem(pem: &[u8]) -> Result<Vec<Vec<u8>>> {
    CertificateRevocationListDer::pem_slice_iter(pem)
        .map(|r| r.map(|crl| crl.as_ref().to_vec()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::other(format!("x509: malformed PEM CRL: {e}")))
}
#[derive(Debug, Clone, Default)]
pub struct TlsProfileOptions {
    pub profile: TlsProfile,
    pub certificates: Vec<TlsCertificate>,
    pub trusted_cas: Option<Vec<CertificateDer<'static>>>,
    pub system_roots: bool,
    pub peer_authentication: TlsPeerAuthentication,
    pub server_name: String,
    pub revocation: Option<TlsRevocationOptions>,
    pub min_version: Option<TlsVersion>,
    pub max_version: Option<TlsVersion>,
}
fn weak(detail: String) -> Error {
    Error::wrap(Kind::WeakTlsCertificate, detail)
}
pub(crate) fn check_tls_certificate_strength(der: &[u8]) -> Result<()> {
    let (_, cert) = X509Certificate::from_der(der)
        .map_err(|e| Error::other(format!("x509: malformed certificate: {e}")))?;
    check_parsed_strength(&cert)
}
fn check_parsed_strength(cert: &X509Certificate<'_>) -> Result<()> {
    use x509_parser::public_key::PublicKey;
    let spki = cert.public_key();
    let algorithm = spki.algorithm.algorithm.to_id_string();
    match spki.parsed() {
        Ok(PublicKey::RSA(rsa)) => {
            let modulus = rsa.modulus;
            let start = modulus
                .iter()
                .position(|b| *b != 0)
                .unwrap_or(modulus.len());
            let modulus = &modulus[start..];
            let bits = match modulus.first() {
                Some(first) => modulus.len() * 8 - first.leading_zeros() as usize,
                None => 0,
            };
            if bits < 2048 {
                return Err(weak(format!("{bits}-bit RSA key")));
            }
        }
        Ok(PublicKey::EC(_)) => {
            let curve = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|p| p.as_oid().ok())
                .map(|oid| oid.to_id_string())
                .unwrap_or_default();
            let bits = match curve.as_str() {
                "1.3.132.0.33" => 224,
                "1.2.840.10045.3.1.7" => 256,
                "1.3.132.0.34" => 384,
                "1.3.132.0.35" => 521,
                _ => return Err(weak("unsupported <nil> key".into())),
            };
            if bits < 256 {
                return Err(weak(format!("{bits}-bit ECC key")));
            }
        }
        _ if algorithm == "1.3.101.112" => {}
        Ok(PublicKey::DSA(_)) => return Err(weak("unsupported DSA key".into())),
        _ if algorithm == "1.3.101.110" => {
            return Err(weak("unsupported ECDH key".into()));
        }
        _ => return Err(weak("unsupported <nil> key".into())),
    }
    let name = match cert.signature_algorithm.algorithm.to_id_string().as_str() {
        "1.2.840.113549.1.1.2" => "MD2-RSA",
        "1.2.840.113549.1.1.4" => "MD5-RSA",
        "1.2.840.113549.1.1.5" | "1.3.14.3.2.29" => "SHA1-RSA",
        "1.2.840.10040.4.3" => "DSA-SHA1",
        "2.16.840.1.101.3.4.3.2" => "DSA-SHA256",
        "1.2.840.10045.4.1" => "ECDSA-SHA1",
        _ => return Ok(()),
    };
    Err(weak(format!("{name} signature")))
}
thread_local! {
    static VERIFY_ERROR: RefCell<Option<Error>> = const { RefCell::new(None) };
    static CLIENT_AUTH: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}
pub(crate) fn reset_handshake_state() {
    VERIFY_ERROR.with(|slot| slot.borrow_mut().take());
    CLIENT_AUTH.with(|c| c.set((false, false)));
}
pub(crate) fn take_verify_error() -> Option<Error> {
    VERIFY_ERROR.with(|slot| slot.borrow_mut().take())
}
pub(crate) fn client_auth_state() -> (bool, bool) {
    CLIENT_AUTH.with(|c| c.get())
}
fn x509_failure(class: TlsFailureClass, text: impl Into<String>) -> Error {
    Error::other(text).with_detail(Detail::Tls(class))
}
fn classify_webpki(e: &webpki::Error) -> Error {
    let name = format!("{e:?}");
    if name.starts_with("UnknownIssuer") {
        x509_failure(
            TlsFailureClass::UnknownCa,
            "x509: certificate signed by unknown authority",
        )
    } else if name.starts_with("CertNotValidYet") {
        x509_failure(
            TlsFailureClass::CertificateExpired,
            "x509: certificate has expired or is not yet valid",
        )
        .also(Kind::TlsCertificateNotYetValid)
    } else if name.starts_with("CertExpired") {
        x509_failure(
            TlsFailureClass::CertificateExpired,
            "x509: certificate has expired or is not yet valid",
        )
    } else if name.starts_with("CertNotValidForName") {
        x509_failure(
            TlsFailureClass::HostnameMismatch,
            "x509: certificate is not valid for the requested name",
        )
    } else if name.starts_with("RequiredEkuNotFound") {
        x509_failure(
            TlsFailureClass::CertificateUsage,
            "x509: certificate specifies an incompatible key usage",
        )
    } else {
        x509_failure(TlsFailureClass::BadCertificate, format!("x509: {name}"))
    }
}
#[derive(Debug, Clone)]
enum NameCheck {
    None,
    Name(ServerName<'static>),
    Invalid,
}
pub(crate) struct PeerPolicy {
    verifying_server: bool,
    verify_chain: bool,
    roots: Vec<CertificateDer<'static>>,
    anchors: Vec<TrustAnchor<'static>>,
    name: NameCheck,
    revocation: Option<(TlsRevocationSet, TlsCrlMode)>,
    algorithms: WebPkiSupportedAlgorithms,
    check_strength: bool,
}
impl std::fmt::Debug for PeerPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerPolicy")
            .field("verifying_server", &self.verifying_server)
            .field("verify_chain", &self.verify_chain)
            .field("roots", &self.roots.len())
            .finish_non_exhaustive()
    }
}
impl PeerPolicy {
    fn verified_chain(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<Vec<CertInfo>> {
        let bad = || {
            x509_failure(
                TlsFailureClass::BadCertificate,
                "x509: malformed certificate",
            )
        };
        self.verified_chain_der(end_entity, intermediates, now)?
            .iter()
            .map(|der| CertInfo::parse(der).ok_or_else(bad))
            .collect()
    }
    fn verified_chain_der(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<Vec<Vec<u8>>> {
        let secs = now.as_secs() as i64;
        if let Ok((_, leaf)) = X509Certificate::from_der(end_entity.as_ref()) {
            let validity = leaf.validity();
            let early = secs < validity.not_before.timestamp();
            if early || secs > validity.not_after.timestamp() {
                let err = x509_failure(
                    TlsFailureClass::CertificateExpired,
                    "x509: certificate has expired or is not yet valid",
                );
                return Err(if early {
                    err.also(Kind::TlsCertificateNotYetValid)
                } else {
                    err
                });
            }
        }
        let cert = webpki::EndEntityCert::try_from(end_entity)
            .map_err(|e| x509_failure(TlsFailureClass::BadCertificate, format!("x509: {e:?}")))?;
        match &self.name {
            NameCheck::None => {}
            NameCheck::Invalid => {
                return Err(x509_failure(
                    TlsFailureClass::HostnameMismatch,
                    "x509: certificate is not valid for the requested name",
                ));
            }
            NameCheck::Name(name) => cert
                .verify_is_valid_for_subject_name(name)
                .map_err(|e| classify_webpki(&e))?,
        }
        let usage = if self.verifying_server {
            webpki::KeyUsage::server_auth()
        } else {
            webpki::KeyUsage::client_auth()
        };
        let (current, outside) = self.anchors_by_validity(secs);
        let path = match cert.verify_for_usage(
            self.algorithms.all,
            &current,
            intermediates,
            now,
            usage,
            None,
            None,
        ) {
            Ok(path) => path,
            Err(first) => {
                if !outside.is_empty()
                    && let Ok(path) = cert.verify_for_usage(
                        self.algorithms.all,
                        &outside,
                        intermediates,
                        now,
                        usage,
                        None,
                        None,
                    )
                {
                    let err = x509_failure(
                        TlsFailureClass::CertificateExpired,
                        "x509: certificate has expired or is not yet valid",
                    );
                    return Err(if self.anchor_not_yet_valid(path.anchor(), secs) {
                        err.also(Kind::TlsCertificateNotYetValid)
                    } else {
                        err
                    });
                }
                return Err(classify_webpki(&first));
            }
        };
        let mut chain = vec![end_entity.as_ref().to_vec()];
        for intermediate in path.intermediate_certificates() {
            chain.push(intermediate.der().as_ref().to_vec());
        }
        let anchor = path.anchor();
        let root = self
            .anchors
            .iter()
            .position(|a| {
                a.subject.as_ref() == anchor.subject.as_ref()
                    && a.subject_public_key_info.as_ref() == anchor.subject_public_key_info.as_ref()
            })
            .and_then(|i| self.roots.get(i));
        if let Some(root) = root
            && root.as_ref() != end_entity.as_ref()
        {
            chain.push(root.as_ref().to_vec());
        }
        Ok(chain)
    }
    fn anchor_period(&self, index: usize) -> Option<(i64, i64)> {
        let root = self.roots.get(index)?;
        let (_, cert) = X509Certificate::from_der(root.as_ref()).ok()?;
        let validity = cert.validity();
        Some((
            validity.not_before.timestamp(),
            validity.not_after.timestamp(),
        ))
    }
    fn anchors_by_validity(
        &self,
        secs: i64,
    ) -> (Vec<TrustAnchor<'static>>, Vec<TrustAnchor<'static>>) {
        let mut current = Vec::with_capacity(self.anchors.len());
        let mut outside = Vec::new();
        for (index, anchor) in self.anchors.iter().enumerate() {
            match self.anchor_period(index) {
                Some((from, to)) if secs < from || secs > to => outside.push(anchor.clone()),
                _ => current.push(anchor.clone()),
            }
        }
        (current, outside)
    }
    fn anchor_not_yet_valid(&self, anchor: &TrustAnchor<'_>, secs: i64) -> bool {
        self.anchors
            .iter()
            .position(|a| {
                a.subject.as_ref() == anchor.subject.as_ref()
                    && a.subject_public_key_info.as_ref() == anchor.subject_public_key_info.as_ref()
            })
            .and_then(|index| self.anchor_period(index))
            .is_some_and(|(from, _)| secs < from)
    }
    fn check_revocation(&self, chain: Vec<CertInfo>, at: SystemTime) -> Result<()> {
        let Some((set, mode)) = &self.revocation else {
            return Ok(());
        };
        set.check_chains(
            &[chain],
            *mode == TlsCrlMode::Chain,
            at,
            self.algorithms.all,
        )
        .map_err(Error::from)
    }
    fn verify_server(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<()> {
        if self.check_strength {
            check_tls_certificate_strength(end_entity.as_ref())?;
        }
        if !self.verify_chain {
            return Ok(());
        }
        let at = SystemTime::now();
        let chain = self
            .verified_chain(end_entity, intermediates, now)
            .map_err(|e| e.prefixed("dicom ul: verify TLS server certificate"))?;
        self.check_revocation(chain, at)
    }
    fn verify_client(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<()> {
        let at = SystemTime::now();
        let chain = self
            .verified_chain(end_entity, intermediates, now)
            .map_err(|e| e.prefixed("tls: failed to verify certificate"))?;
        check_tls_certificate_strength(end_entity.as_ref())?;
        self.check_revocation(chain, at)
    }
    pub(crate) fn reverify(&self, peer: &[CertificateDer<'static>]) -> Result<()> {
        let Some((end_entity, intermediates)) = peer.split_first() else {
            return Ok(());
        };
        let now = UnixTime::now();
        if self.verifying_server {
            self.verify_server(end_entity, intermediates, now)
        } else {
            self.verify_client(end_entity, intermediates, now)
        }
    }
}
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) fn https_server_verifier(
    roots: Vec<CertificateDer<'static>>,
    server_name: &str,
    provider: &CryptoProvider,
) -> Arc<dyn ServerCertVerifier> {
    let mut kept = Vec::with_capacity(roots.len());
    let mut anchors = Vec::with_capacity(roots.len());
    for root in roots {
        if let Ok(anchor) = webpki::anchor_from_trusted_cert(&root) {
            anchors.push(anchor.to_owned());
            kept.push(root);
        }
    }
    let name = match ServerName::try_from(server_name.to_string()) {
        Ok(name) => NameCheck::Name(name),
        Err(_) => NameCheck::Invalid,
    };
    Arc::new(ProfileServerVerifier(Arc::new(PeerPolicy {
        verifying_server: true,
        verify_chain: true,
        roots: kept,
        anchors,
        name,
        revocation: None,
        algorithms: provider.signature_verification_algorithms,
        check_strength: false,
    })))
}
fn alert_error(err: &Error, own_chain_check: bool) -> rustls::Error {
    let certificate = match err.tls_failure() {
        Some(TlsFailureClass::UnknownCa) if own_chain_check => CertificateError::UnknownIssuer,
        Some(TlsFailureClass::CertificateExpired) if own_chain_check => CertificateError::Expired,
        _ => CertificateError::BadEncoding,
    };
    rustls::Error::InvalidCertificate(certificate)
}
#[derive(Debug)]
struct ProfileServerVerifier(Arc<PeerPolicy>);
impl ServerCertVerifier for ProfileServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        match self.0.verify_server(end_entity, intermediates, now) {
            Ok(()) => Ok(ServerCertVerified::assertion()),
            Err(e) => {
                let alert = alert_error(&e, false);
                VERIFY_ERROR.with(|slot| *slot.borrow_mut() = Some(e));
                Err(alert)
            }
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.algorithms.supported_schemes()
    }
}
#[derive(Debug)]
struct ProfileClientVerifier {
    policy: Arc<PeerPolicy>,
    mandatory: bool,
    subjects: Vec<DistinguishedName>,
}
impl ClientCertVerifier for ProfileClientVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &self.subjects
    }
    fn client_auth_mandatory(&self) -> bool {
        self.mandatory
    }
    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        match self.policy.verify_client(end_entity, intermediates, now) {
            Ok(()) => Ok(ClientCertVerified::assertion()),
            Err(e) => {
                let alert = alert_error(&e, true);
                VERIFY_ERROR.with(|slot| *slot.borrow_mut() = Some(e));
                Err(alert)
            }
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.policy.algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.policy.algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.policy.algorithms.supported_schemes()
    }
}
#[derive(Debug)]
struct ClientCertificates {
    keys: Vec<Arc<CertifiedKey>>,
    issuers: Vec<Vec<Vec<u8>>>,
}
impl rustls::client::ResolvesClientCert for ClientCertificates {
    fn resolve(
        &self,
        root_hint_subjects: &[&[u8]],
        sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        let mut chosen = None;
        for (key, issuers) in self.keys.iter().zip(&self.issuers) {
            if key.key.choose_scheme(sigschemes).is_none() {
                continue;
            }
            let acceptable = root_hint_subjects.is_empty()
                || issuers
                    .iter()
                    .any(|issuer| root_hint_subjects.contains(&issuer.as_slice()));
            if acceptable {
                chosen = Some(key.clone());
                break;
            }
        }
        CLIENT_AUTH.with(|c| c.set((true, chosen.is_some())));
        chosen
    }
    fn has_certs(&self) -> bool {
        !self.keys.is_empty()
    }
}
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) fn client_certificate_resolver(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    provider: &CryptoProvider,
) -> Result<Arc<dyn rustls::client::ResolvesClientCert>> {
    let key = provider
        .key_provider
        .load_private_key(key)
        .map_err(|e| Error::other(format!("tls: private key: {e}")))?;
    let issuers = vec![
        chain
            .iter()
            .filter_map(|der| CertInfo::parse(der.as_ref()).map(|info| info.raw_issuer))
            .collect(),
    ];
    Ok(Arc::new(ClientCertificates {
        keys: vec![Arc::new(CertifiedKey::new(chain, key))],
        issuers,
    }))
}
#[derive(Debug)]
struct ServerCertificates {
    keys: Vec<Arc<CertifiedKey>>,
    tls12_suites: Vec<u16>,
    tls12: bool,
    tls13: bool,
}
fn ecdsa_suite(id: u16) -> bool {
    matches!(
        id,
        TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
            | TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
            | TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
    )
}
impl rustls::server::ResolvesServerCert for ServerCertificates {
    fn resolve(&self, hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let offered: Vec<u16> = hello
            .cipher_suites()
            .iter()
            .map(|s| u16::from(*s))
            .collect();
        let tls13 = self.tls13 && offered.iter().any(|id| (0x1301..=0x1303).contains(id));
        for key in &self.keys {
            if key.key.choose_scheme(hello.signature_schemes()).is_none() {
                continue;
            }
            let rsa = key.key.algorithm() == rustls::SignatureAlgorithm::RSA;
            let tls12 = self.tls12
                && offered
                    .iter()
                    .any(|id| self.tls12_suites.contains(id) && ecdsa_suite(*id) != rsa);
            if tls13 || tls12 {
                return Some(key.clone());
            }
        }
        self.keys.first().cloned()
    }
}
#[derive(Clone)]
pub struct TlsClientConfig {
    pub(crate) config: Arc<rustls::ClientConfig>,
    pub(crate) server_name: String,
    pub(crate) policy: Option<Arc<PeerPolicy>>,
    description: ConfigDescription,
}
#[derive(Clone)]
pub struct TlsServerConfig {
    pub(crate) config: Arc<rustls::ServerConfig>,
    pub(crate) policy: Option<Arc<PeerPolicy>>,
    description: ConfigDescription,
}
#[derive(Debug, Clone, Default)]
struct ConfigDescription {
    min_version: u16,
    max_version: u16,
    cipher_suites: Vec<u16>,
    certificates: usize,
    client_auth: TlsPeerAuthentication,
    #[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
    certificate_ders: Vec<Vec<u8>>,
}
impl std::fmt::Debug for TlsClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsClientConfig")
            .field("server_name", &self.server_name)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for TlsServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsServerConfig")
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}
macro_rules! describe {
    ($t:ty) => {
        impl $t {
            pub fn min_version(&self) -> u16 {
                self.description.min_version
            }
            pub fn max_version(&self) -> u16 {
                self.description.max_version
            }
            pub fn cipher_suites(&self) -> &[u16] {
                &self.description.cipher_suites
            }
            pub fn certificates(&self) -> usize {
                self.description.certificates
            }
        }
    };
}
describe!(TlsClientConfig);
describe!(TlsServerConfig);
impl TlsClientConfig {
    pub fn from_rustls(config: Arc<rustls::ClientConfig>, server_name: &str) -> TlsClientConfig {
        TlsClientConfig {
            config,
            server_name: server_name.to_string(),
            policy: None,
            description: ConfigDescription::default(),
        }
    }
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
    #[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
    pub(crate) fn certificate_ders(&self) -> &[Vec<u8>] {
        &self.description.certificate_ders
    }
}
impl TlsServerConfig {
    pub fn from_rustls(config: Arc<rustls::ServerConfig>) -> TlsServerConfig {
        TlsServerConfig {
            config,
            policy: None,
            description: ConfigDescription::default(),
        }
    }
    pub fn client_auth(&self) -> TlsPeerAuthentication {
        self.description.client_auth
    }
    #[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
    pub(crate) fn rustls_config(&self) -> Arc<rustls::ServerConfig> {
        Arc::clone(&self.config)
    }
    pub fn verified_chains(&self, peer: &[CertificateDer<'_>]) -> Vec<Vec<Vec<u8>>> {
        let (Some(policy), Some((leaf, intermediates))) = (&self.policy, peer.split_first()) else {
            return Vec::new();
        };
        policy
            .verified_chain_der(leaf, intermediates, UnixTime::now())
            .map(|chain| vec![chain])
            .unwrap_or_default()
    }
}
const GO_PREFERENCE: [u16; 6] = [
    TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
    TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
];
fn provider_for(suites: &[u16]) -> CryptoProvider {
    use rustls::crypto::ring::cipher_suite as cs;
    let mut provider = rustls::crypto::ring::default_provider();
    let mut list = vec![
        cs::TLS13_AES_128_GCM_SHA256,
        cs::TLS13_AES_256_GCM_SHA384,
        cs::TLS13_CHACHA20_POLY1305_SHA256,
    ];
    for id in GO_PREFERENCE {
        if !suites.contains(&id) {
            continue;
        }
        list.push(match id {
            TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 => cs::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 => cs::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 => cs::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384 => cs::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 => {
                cs::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
            }
            _ => cs::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        });
    }
    provider.cipher_suites = list;
    provider
}
struct Common {
    provider: Arc<CryptoProvider>,
    versions: Vec<&'static rustls::SupportedProtocolVersion>,
    suites: Vec<u16>,
    min: TlsVersion,
    max: TlsVersion,
}
fn profile_common(opts: &TlsProfileOptions) -> Result<Common> {
    let spec = opts.profile.spec()?;
    for certificate in &opts.certificates {
        if let Some(leaf) = certificate.chain.first() {
            let (_, parsed) = X509Certificate::from_der(leaf.as_ref())
                .map_err(|e| Error::other(format!("dicom ul: parse TLS certificate: {e}")))?;
            check_parsed_strength(&parsed)?;
        }
    }
    let min = opts.min_version.unwrap_or(TlsVersion::Tls12);
    let max = opts.max_version.unwrap_or(TlsVersion::Tls13);
    let mut versions: Vec<&'static rustls::SupportedProtocolVersion> = Vec::new();
    if min <= TlsVersion::Tls12 && max >= TlsVersion::Tls12 {
        versions.push(&rustls::version::TLS12);
    }
    if min <= TlsVersion::Tls13 && max >= TlsVersion::Tls13 {
        versions.push(&rustls::version::TLS13);
    }
    if versions.is_empty() {
        return Err(Error::other(
            "tls: no supported versions satisfy MinVersion and MaxVersion",
        ));
    }
    Ok(Common {
        provider: Arc::new(provider_for(spec.suites)),
        versions,
        suites: spec.suites.to_vec(),
        min,
        max,
    })
}
fn compile_revocation(opts: &TlsProfileOptions) -> Result<Option<(TlsRevocationSet, TlsCrlMode)>> {
    let Some(r) = &opts.revocation else {
        return Ok(None);
    };
    if r.mode == TlsCrlMode::None && r.crls.is_empty() {
        return Ok(None);
    }
    if !matches!(r.mode, TlsCrlMode::Leaf | TlsCrlMode::Chain)
        || opts.peer_authentication == TlsPeerAuthentication::IgnorePeerCertificate
    {
        return Err(TlsRevocationError::new(TlsRevocationKind::InvalidMaterial, -1).into());
    }
    let set = TlsRevocationSet::new(&r.crls)?;
    set.preflight(SystemTime::now())?;
    Ok(Some((set, r.mode)))
}
fn certified_keys(common: &Common, opts: &TlsProfileOptions) -> Result<Vec<Arc<CertifiedKey>>> {
    opts.certificates
        .iter()
        .map(|c| {
            let key = common
                .provider
                .key_provider
                .load_private_key(c.key.clone_key())
                .map_err(|e| Error::other(format!("tls: private key: {e}")))?;
            Ok(Arc::new(CertifiedKey::new(c.chain.clone(), key)))
        })
        .collect()
}
fn anchors_of(roots: &[CertificateDer<'static>]) -> Result<Vec<TrustAnchor<'static>>> {
    roots
        .iter()
        .map(|c| {
            webpki::anchor_from_trusted_cert(c)
                .map(|a| a.to_owned())
                .map_err(|e| Error::other(format!("x509: malformed trusted certificate: {e:?}")))
        })
        .collect()
}
fn description(common: &Common, opts: &TlsProfileOptions) -> ConfigDescription {
    ConfigDescription {
        min_version: common.min.id(),
        max_version: common.max.id(),
        cipher_suites: common.suites.clone(),
        certificates: opts.certificates.len(),
        client_auth: TlsPeerAuthentication::IgnorePeerCertificate,
        certificate_ders: opts
            .certificates
            .iter()
            .flat_map(|c| c.chain.iter().map(|der| der.as_ref().to_vec()))
            .collect(),
    }
}
pub fn client_tls_config(opts: TlsProfileOptions) -> Result<TlsClientConfig> {
    let common = profile_common(&opts)?;
    let revocation = compile_revocation(&opts)?;
    let server_name = trim_unicode_space(&opts.server_name).to_string();
    let verify_chain = opts.peer_authentication != TlsPeerAuthentication::IgnorePeerCertificate;
    if verify_chain && opts.trusted_cas.is_none() && server_name.is_empty() {
        return Err(Error::other(
            "dicom ul: TLS client verification requires trusted CAs or a server name",
        ));
    }
    let roots = match &opts.trusted_cas {
        Some(roots) => roots.clone(),
        None if opts.system_roots && verify_chain => super::system_root_certificates()?,
        None => Vec::new(),
    };
    let name = if server_name.is_empty() {
        NameCheck::None
    } else {
        match ServerName::try_from(server_name.clone()) {
            Ok(n) => NameCheck::Name(n),
            Err(_) => NameCheck::Invalid,
        }
    };
    let policy = Arc::new(PeerPolicy {
        verifying_server: true,
        verify_chain,
        anchors: anchors_of(&roots)?,
        roots,
        name,
        revocation,
        algorithms: common.provider.signature_verification_algorithms,
        check_strength: true,
    });
    let keys = certified_keys(&common, &opts)?;
    let issuers = opts
        .certificates
        .iter()
        .map(|c| {
            c.chain
                .iter()
                .filter_map(|der| CertInfo::parse(der.as_ref()).map(|info| info.raw_issuer))
                .collect()
        })
        .collect();
    let mut config = rustls::ClientConfig::builder_with_provider(common.provider.clone())
        .with_protocol_versions(&common.versions)
        .map_err(|e| Error::other(format!("tls: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(ProfileServerVerifier(policy.clone())))
        .with_client_cert_resolver(Arc::new(ClientCertificates { keys, issuers }));
    config.resumption = rustls::client::Resumption::disabled();
    Ok(TlsClientConfig {
        config: Arc::new(config),
        server_name,
        policy: Some(policy),
        description: description(&common, &opts),
    })
}
pub fn server_tls_config(opts: TlsProfileOptions) -> Result<TlsServerConfig> {
    if opts.certificates.is_empty() {
        return Err(Error::other("dicom ul: TLS server requires a certificate"));
    }
    let common = profile_common(&opts)?;
    let revocation = compile_revocation(&opts)?;
    let mandatory = match opts.peer_authentication {
        TlsPeerAuthentication::RequirePeerCertificate => Some(true),
        TlsPeerAuthentication::VerifyPeerCertificateIfGiven => Some(false),
        TlsPeerAuthentication::IgnorePeerCertificate => None,
        TlsPeerAuthentication::Other(v) => {
            return Err(Error::other(format!(
                "dicom ul: unknown TLS peer authentication {v}"
            )));
        }
    };
    if mandatory.is_some() && opts.trusted_cas.is_none() {
        return Err(Error::other(
            "dicom ul: verifying TLS client certificates requires trusted CAs",
        ));
    }
    let roots = opts.trusted_cas.clone().unwrap_or_default();
    let subjects = roots
        .iter()
        .filter_map(|der| CertInfo::parse(der.as_ref()))
        .map(|info| DistinguishedName::from(info.raw_subject))
        .collect();
    let policy = Arc::new(PeerPolicy {
        verifying_server: false,
        verify_chain: true,
        anchors: anchors_of(&roots)?,
        roots,
        name: NameCheck::None,
        revocation,
        algorithms: common.provider.signature_verification_algorithms,
        check_strength: true,
    });
    let keys = certified_keys(&common, &opts)?;
    let resolver = Arc::new(ServerCertificates {
        keys,
        tls12_suites: common.suites.clone(),
        tls12: common
            .versions
            .iter()
            .any(|v| v.version == rustls::ProtocolVersion::TLSv1_2),
        tls13: common
            .versions
            .iter()
            .any(|v| v.version == rustls::ProtocolVersion::TLSv1_3),
    });
    let builder = rustls::ServerConfig::builder_with_provider(common.provider.clone())
        .with_protocol_versions(&common.versions)
        .map_err(|e| Error::other(format!("tls: {e}")))?;
    let mut config = match mandatory {
        Some(mandatory) => builder.with_client_cert_verifier(Arc::new(ProfileClientVerifier {
            policy: policy.clone(),
            mandatory,
            subjects,
        })),
        None => builder.with_no_client_auth(),
    }
    .with_cert_resolver(resolver);
    config.ignore_client_order = true;
    let mut description = description(&common, &opts);
    description.client_auth = opts.peer_authentication;
    Ok(TlsServerConfig {
        config: Arc::new(config),
        policy: mandatory.map(|_| policy),
        description,
    })
}
