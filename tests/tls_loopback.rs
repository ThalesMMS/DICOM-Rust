#![cfg(feature = "tls")]
#[path = "support/build.rs"]
mod build;
use build::*;
use dicom_rust::net::dimse as d;
use dicom_rust::net::ul::{self, Ctx};
use dicom_rust::object::Tag;
use std::sync::{Arc, Mutex};
use std::thread;
const VERIFY: &str = "1.2.840.10008.1.1";
fn pki(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/pki")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
fn profile(own: bool, trust: bool) -> ul::TlsProfileOptions {
    ul::TlsProfileOptions {
        certificates: if own {
            vec![
                ul::TlsCertificate::from_pem(&pki("leaf-ec.pem"), &pki("leaf-ec.key.pem"))
                    .expect("certificate"),
            ]
        } else {
            Vec::new()
        },
        trusted_cas: trust.then(|| ul::certificates_from_pem(&pki("ca.pem")).expect("authority")),
        peer_authentication: ul::TlsPeerAuthentication::RequirePeerCertificate,
        ..Default::default()
    }
}
fn dial(own: bool, trust: bool) -> ul::DialOptions {
    let mut p = profile(own, trust);
    p.server_name = "localhost".into();
    ul::DialOptions {
        called_ae_title: "SCP".into(),
        calling_ae_title: "SCU".into(),
        contexts: vec![
            ul::PresentationContext::new(VERIFY, &[IMPLICIT_LE]),
            ul::PresentationContext::new(SECONDARY_CAPTURE, &[EXPLICIT_LE]),
        ],
        tls_config: Some(ul::client_tls_config(p).expect("client TLS")),
        negotiation_timeout: ul::Nanos(20_000_000_000),
        ..Default::default()
    }
}
fn accept() -> ul::AcceptOptions {
    ul::AcceptOptions {
        ae_title: "SCP".into(),
        supported_abstract_syntaxes: vec![VERIFY.into(), SECONDARY_CAPTURE.into()],
        supported_transfer_syntaxes: vec![IMPLICIT_LE.into(), EXPLICIT_LE.into()],
        tls_config: Some(ul::server_tls_config(profile(true, true)).expect("server TLS")),
        ..Default::default()
    }
}
#[test]
fn echo_and_store_over_tls() {
    let stored = Arc::new(Mutex::new(Vec::new()));
    let seen = stored.clone();
    let opts = d::AssociationScpOptions {
        storage: d::StorageScpOptions {
            store_handler: Some(Arc::new(move |_: &Ctx, req: &d::CStoreRequestContext| {
                let ds = req.data_set.as_ref().expect("data set");
                seen.lock().unwrap().push(
                    ds.get(Tag::PIXEL_DATA)
                        .and_then(|e| e.raw())
                        .map(<[u8]>::to_vec),
                );
                (0, None)
            })),
            ..Default::default()
        },
        ..Default::default()
    };
    let listener = ul::listen(ul::ListenOptions::default()).expect("listen");
    let addr = listener.local_addr().expect("addr").to_string();
    let server = thread::spawn(move || {
        let assoc = listener.accept_association(accept()).expect("accept");
        assert!(
            assoc.tls_connection_state().is_some(),
            "server side over TLS"
        );
        let r = d::serve_association(None, &assoc, &opts).map_err(|e| e.text().to_string());
        assoc.close();
        r
    });
    let assoc = ul::dial(&addr, dial(true, true)).expect("dial");
    assert!(
        assoc.tls_connection_state().is_some(),
        "client side over TLS"
    );
    let pc = d::accepted_verification_context(&assoc).expect("verification context");
    assert_eq!(
        d::send_c_echo(None, &assoc, pc.id, 1).expect("echo").status,
        0
    );
    let ds = image(8, 8, 1, 8, 8);
    let want = ds.get_raw(Tag::PIXEL_DATA);
    let mut client = d::StoreClient::new(&assoc);
    let r = client
        .store_with_options(None, &ds, Default::default())
        .map_err(|(_, e)| e.text().to_string())
        .expect("store");
    assert_eq!(r.response.expect("response").status, 0);
    assert_eq!(
        d::send_c_echo(None, &assoc, pc.id, 2).expect("echo").status,
        0
    );
    assoc.release(None).expect("release");
    assoc.close();
    assert_eq!(server.join().expect("server thread"), Ok(()));
    assert_eq!(*stored.lock().unwrap(), vec![want]);
}
fn refused(own: bool, trust: bool) {
    let listener = ul::listen(ul::ListenOptions::default()).expect("listen");
    let addr = listener.local_addr().expect("addr").to_string();
    let server = thread::spawn(move || {
        listener
            .accept_association(accept())
            .map(|a| a.close())
            .is_err()
    });
    let client = ul::dial(&addr, dial(own, trust));
    let client_failed = match client {
        Err(_) => true,
        Ok(a) => {
            let failed = d::accepted_verification_context(&a)
                .map(|pc| d::send_c_echo(None, &a, pc.id, 1).is_err())
                .unwrap_or(true);
            a.close();
            failed
        }
    };
    assert!(
        client_failed,
        "own={own} trust={trust}: the client went through"
    );
    assert!(
        server.join().expect("server thread"),
        "own={own} trust={trust}: the server accepted"
    );
}
#[test]
fn an_untrusted_server_is_refused() {
    refused(true, false);
}
#[test]
fn a_client_without_certificate_is_refused() {
    refused(false, true);
}
