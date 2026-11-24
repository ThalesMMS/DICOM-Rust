#[allow(unused_imports)]
use super::support::*;
use dicom_rust::dsig::{self, Kind, PublicKey};
use dicom_rust::object;
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
const MAX_INPUT: usize = 1 << 20;
fn digest(data: &[u8]) -> String {
    let sum = Sha256::digest(data);
    format!("{}:{}", data.len(), hex::encode(&sum[..8]))
}
pub fn verify(data: Vec<i64>) -> Result<String, String> {
    if data.len() > MAX_INPUT {
        return Err("harness: input too large".to_string());
    }
    let data: Vec<u8> = data.iter().map(|&b| b as u8).collect();
    let f = object::read_file(&data).map_err(|(_, e)| format!("harness: {e}"))?;
    let list = dsig::signatures(f.dataset.elements());
    let mut b = format!("signatures={}\n", list.len());
    for sig in &list {
        let key = match sig.certificate.as_ref().map(|c| &c.public_key) {
            None => "none".to_string(),
            Some(PublicKey::Rsa { n, e }) => format!("rsa/{}/{e}", n.bits()),
            Some(PublicKey::Ecdsa { curve, .. }) => format!("ecdsa/{}", curve.name()),
            Some(other) => other.type_label().to_string(),
        };
        b.push_str(&format!(
            "sig path={} macid={} uid={} dt={} certtype={} cert={} key={key} signature={} tstype={} ts={}\n",
            quote(&sig.path),
            sig.mac_id_number,
            quote(&sig.uid),
            quote(&sig.date_time),
            quote(&sig.certificate_type),
            digest(&sig.certificate_of_signer),
            digest(&sig.signature),
            quote(&sig.certified_timestamp_type),
            digest(&sig.certified_timestamp)
        ));
        let tags: Vec<String> = sig
            .data_elements_signed
            .iter()
            .map(ToString::to_string)
            .collect();
        b.push_str(&format!(
            "  mac={} syntax={} tags=[{}] purposes={}",
            quote(&sig.mac_algorithm),
            quote(&sig.mac_transfer_syntax_uid),
            tags.join(" "),
            sig.purposes.len()
        ));
        for p in &sig.purposes {
            b.push_str(&format!(
                " ({},{},{})",
                quote(&p.value),
                quote(&p.scheme),
                quote(&p.meaning)
            ));
        }
        match dsig::verify_signature(sig) {
            Ok(()) => b.push_str("\n  verify=ok\n"),
            Err(err) => {
                let kind = match err.kind {
                    Kind::SignatureInvalid => "invalid",
                    Kind::MalformedSignature => "malformed",
                    Kind::Unsupported => "unsupported",
                    Kind::EncapsulatedMacSyntax => "encapsulated",
                    _ => "other",
                };
                b.push_str(&format!("\n  verify={kind}: {err}\n"));
            }
        }
    }
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Verify" => {
            let out = verify(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
