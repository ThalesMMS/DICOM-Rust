#![forbid(unsafe_code)]
use super::error::{Error, Result};
use rustls::pki_types::CertificateDer;
pub fn system_root_certificates() -> Result<Vec<CertificateDer<'static>>> {
    let loaded = rustls_native_certs::load_native_certs();
    if loaded.certs.is_empty()
        && let Some(first) = loaded.errors.first()
    {
        return Err(Error::other(format!(
            "dicom ul: load system trust roots: {first}"
        )));
    }
    Ok(loaded.certs)
}
#[doc(hidden)]
pub fn system_root_store() -> Result<(rustls::RootCertStore, usize)> {
    let mut store = rustls::RootCertStore::empty();
    let (added, _ignored) = store.add_parsable_certificates(system_root_certificates()?);
    Ok((store, added))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_configurations_with_the_system_roots() {
        let certs = system_root_certificates().expect("the system store opens");
        let (store, added) = system_root_store().expect("the system store opens");
        assert!(added <= certs.len());
        assert_eq!(store.len(), added);
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_root_certificates(store)
            .with_no_client_auth();
        assert!(config.alpn_protocols.is_empty());
    }
    #[test]
    fn upper_layer_client_opts_in() {
        use super::super::tls_profile::{TlsProfileOptions, client_tls_config};
        let refused = client_tls_config(TlsProfileOptions {
            system_roots: true,
            ..Default::default()
        });
        assert!(refused.is_err());
        let config = client_tls_config(TlsProfileOptions {
            server_name: "pacs.example.org".into(),
            system_roots: true,
            ..Default::default()
        });
        assert!(
            config.is_ok(),
            "{:?}",
            config.err().map(|e| e.text().to_string())
        );
        let explicit = client_tls_config(TlsProfileOptions {
            server_name: "pacs.example.org".into(),
            trusted_cas: Some(Vec::new()),
            system_roots: true,
            ..Default::default()
        });
        assert!(explicit.is_ok());
    }
}
