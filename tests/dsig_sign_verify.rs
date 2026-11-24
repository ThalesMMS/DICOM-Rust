#![cfg(feature = "dsig")]
#[path = "support/build.rs"]
mod build;
use build::*;
use chrono::{DateTime, Utc};
use dicom_rust::dsig::{self, CertPool, SignOptions, SigningKey, TrustOptions, X509Certificate};
use dicom_rust::object::{self, File, Tag, Value, Vr, writer};
fn pki(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/pki")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
fn options(signer: &str, tags: Option<Vec<Tag>>) -> SignOptions {
    SignOptions {
        certificate: dsig::parse_certificate(&pki(&format!("{signer}.der"))).expect("certificate"),
        key: SigningKey::from_pkcs8_der(&pki(&format!("{signer}.key.der"))).expect("key"),
        mac_algorithm: String::new(),
        mac_transfer_syntax_uid: String::new(),
        tags,
        purpose: None,
        profile: dsig::Profile::None,
        profile_options: Default::default(),
        time: DateTime::<Utc>::from_timestamp(1_735_000_000, 0).unwrap(),
        uid: "1.2.826.0.1.3680043.10.4.1".into(),
        item: String::new(),
    }
}
fn signed(signer: &str, tags: Option<Vec<Tag>>) -> File {
    let mut f = file(rich_data_set(), EXPLICIT_LE);
    let sig = dsig::sign(&mut f.dataset, &options(signer, tags)).expect("sign");
    assert_eq!(sig.uid, "1.2.826.0.1.3680043.10.4.1");
    assert!(!sig.signature.is_empty());
    let bytes = writer::write_file(&f).expect("write");
    object::read_file(&bytes).map_err(|(_, e)| e).expect("read")
}
fn trust() -> TrustOptions {
    let mut roots = CertPool::new();
    roots.add(X509Certificate::parse(&pki("ca.der")).expect("authority"));
    TrustOptions {
        roots: Some(roots),
        ..Default::default()
    }
}
#[test]
fn signatures_verify_after_a_file_round_trip() {
    for signer in ["signer-rsa", "leaf-ec"] {
        let f = signed(signer, None);
        let list = dsig::signatures(f.dataset.elements());
        assert_eq!(list.len(), 1, "{signer}");
        let sig = &list[0];
        assert_eq!(sig.mac_algorithm, "SHA256", "{signer}");
        assert_eq!(sig.mac_transfer_syntax_uid, EXPLICIT_LE, "{signer}");
        assert!(
            sig.data_elements_signed.contains(&Tag::new(0x0010, 0x0010)),
            "{signer}"
        );
        dsig::verify_signature(sig).unwrap_or_else(|e| panic!("{signer}: {e}"));
        let results = dsig::verify(&f.dataset);
        assert_eq!(results.len(), 1);
        assert!(results[0].valid(), "{signer}: {:?}", results[0].err);
        let trusted = dsig::verify_with_trust(&f.dataset, &trust());
        assert!(trusted[0].trusted(), "{signer}: {:?}", trusted[0].trust_err);
        let untrusted = dsig::verify_with_trust(&f.dataset, &TrustOptions::default());
        assert!(untrusted[0].valid() && !untrusted[0].trusted(), "{signer}");
    }
}
#[test]
fn a_changed_signed_value_breaks_the_signature() {
    for signer in ["signer-rsa", "leaf-ec"] {
        let mut f = signed(signer, None);
        f.dataset.put(text(0x0010, 0x0010, Vr::PN, "Changed^Name"));
        let results = dsig::verify(&f.dataset);
        assert!(!results[0].valid(), "{signer}: still valid after a change");
        assert!(dsig::verify_signature(&dsig::signatures(f.dataset.elements())[0]).is_err());
    }
}
#[test]
fn a_changed_signature_value_is_invalid() {
    let f = signed("signer-rsa", None);
    let mut sig = dsig::signatures(f.dataset.elements()).remove(0);
    let last = sig.signature.len() - 1;
    sig.signature[last] ^= 1;
    assert!(dsig::verify_signature(&sig).is_err());
}
#[test]
fn a_subset_signature_ignores_other_elements() {
    let tags = vec![
        Tag::new(0x0008, 0x0016),
        Tag::new(0x0008, 0x0018),
        Tag::new(0x0010, 0x0020),
    ];
    let mut f = signed("leaf-ec", Some(tags.clone()));
    let sig = &dsig::signatures(f.dataset.elements())[0];
    assert_eq!(sig.data_elements_signed, tags);
    f.dataset
        .put(text(0x0008, 0x1030, Vr::LO, "Another description"));
    assert!(dsig::verify(&f.dataset)[0].valid());
    f.dataset.put(text(0x0010, 0x0020, Vr::LO, "SYN-CHANGED"));
    assert!(!dsig::verify(&f.dataset)[0].valid());
}
#[test]
fn removal_leaves_no_signature() {
    let mut f = signed("signer-rsa", None);
    dsig::sign(&mut f.dataset, &{
        let mut o = options("leaf-ec", None);
        o.uid = "1.2.826.0.1.3680043.10.4.2".into();
        o
    })
    .expect("second signature");
    assert_eq!(dsig::signatures(f.dataset.elements()).len(), 2);
    let first = dsig::signatures(f.dataset.elements()).remove(0);
    dsig::remove_signature(&mut f.dataset, &first).expect("remove one");
    assert_eq!(dsig::signatures(f.dataset.elements()).len(), 1);
    assert_eq!(
        dsig::remove_all_signatures(&mut f.dataset).expect("remove all"),
        1
    );
    assert!(dsig::signatures(f.dataset.elements()).is_empty());
    assert!(
        !f.dataset.has(Tag::new(0xFFFA, 0xFFFA)),
        "Digital Signatures Sequence"
    );
    assert!(matches!(
        f.dataset.get(Tag::new(0x4FFE, 0x0001)).map(|e| &e.value),
        None | Some(Value::Sequence(_))
    ));
}
#[test]
fn bad_keys_and_certificates_are_refused() {
    assert!(SigningKey::from_pkcs8_der(b"not a key").is_none());
    assert!(dsig::parse_certificate(b"not a certificate").is_none());
    assert!(
        X509Certificate::parse(&pki("ca.pem")).is_none(),
        "PEM is not DER"
    );
    let mut f = file(rich_data_set(), EXPLICIT_LE);
    let mut o = options("signer-rsa", None);
    o.key = SigningKey::from_pkcs8_der(&pki("leaf-ec.key.der")).unwrap();
    assert!(dsig::sign(&mut f.dataset, &o).is_err());
}
