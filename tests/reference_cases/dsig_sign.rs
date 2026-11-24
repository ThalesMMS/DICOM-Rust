#[allow(unused_imports)]
use super::support::*;
use chrono::DateTime;
use dicom_rust::dsig::{self, Code, Kind, Profile, ProfileOptions, SignOptions, SigningKey};
use dicom_rust::object::{self, File, Tag};
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
const MAX_INPUT: usize = 1 << 20;
fn kind_of(err: &dsig::Error) -> String {
    let kinds: Vec<&str> = [
        (Kind::SignatureInvalid, "invalid"),
        (Kind::MalformedSignature, "malformed"),
        (Kind::Unsupported, "unsupported"),
        (Kind::EncapsulatedMacSyntax, "encapsulated"),
        (Kind::ItemNotFound, "item"),
        (Kind::ProfileViolation, "violation"),
        (Kind::ProfileIncomplete, "incomplete"),
        (Kind::ProfileLimit, "limit"),
    ]
    .into_iter()
    .filter(|(kind, _)| err.is(*kind))
    .map(|(_, name)| name)
    .collect();
    format!("[{}]", kinds.join(" "))
}
fn bytes(data: &[i64]) -> Vec<u8> {
    data.iter().map(|&b| b as u8).collect()
}
fn read(data: &[i64]) -> Result<File, String> {
    if data.len() > MAX_INPUT {
        return Err("harness: input too large".to_string());
    }
    object::read_file(&bytes(data)).map_err(|(_, e)| format!("harness: {e}"))
}
fn state(b: &mut String, f: &File, with_file: bool, random_uid: &str) {
    let list = dsig::signatures(f.dataset.elements());
    let with_file =
        with_file && (random_uid.is_empty() || list.iter().all(|s| s.uid != random_uid.trim()));
    b.push_str(&format!("signatures={}\n", list.len()));
    for sig in &list {
        let verify = match dsig::verify_signature(sig) {
            Ok(()) => "ok".to_string(),
            Err(err) => format!("{} {err}", kind_of(&err)),
        };
        let tags: Vec<String> = sig
            .data_elements_signed
            .iter()
            .map(ToString::to_string)
            .collect();
        let purposes: Vec<String> = sig
            .purposes
            .iter()
            .map(|p| format!("{{{} {} {}}}", p.value, p.scheme, p.meaning))
            .collect();
        b.push_str(&format!(
            "  path={} macid={} uid={} dt={} mac={} syntax={} tags=[{}] purposes=[{}] verify={verify}\n",
            quote(&sig.path),
            sig.mac_id_number,
            quote(&sig.uid),
            quote(&sig.date_time),
            quote(&sig.mac_algorithm),
            quote(&sig.mac_transfer_syntax_uid),
            tags.join(" "),
            purposes.join(" "),
        ));
    }
    if !with_file {
        return;
    }
    match object::writer::write_file(f) {
        Err(err) => b.push_str(&format!("file: {err}\n")),
        Ok(out) => b.push_str(&format!(
            "file={}:{}\n",
            out.len(),
            hex::encode(Sha256::digest(&out))
        )),
    }
}
fn bad_opts() -> String {
    "harness: opts".to_string()
}
pub fn sign(data: Vec<i64>, cert: Vec<i64>, key: Vec<i64>, opts: String) -> Result<String, String> {
    let mut f = read(&data)?;
    let lines: Vec<&str> = opts.split('\n').collect();
    if lines.len() != 13 {
        return Err(bad_opts());
    }
    let mut numbers = [0i64; 6];
    for (slot, at) in numbers.iter_mut().zip([4, 6, 7, 8, 10, 11]) {
        *slot = lines[at].parse().map_err(|_| bad_opts())?;
    }
    if !(0..=253_402_300_799).contains(&numbers[4])
        || !(0..=999_999).contains(&numbers[5])
        || lines[12].is_empty()
    {
        return Err(bad_opts());
    }
    let certificate =
        dsig::parse_certificate(&bytes(&cert)).ok_or_else(|| "harness: certificate".to_string())?;
    let key = SigningKey::from_pkcs8_der(&bytes(&key)).ok_or_else(|| "harness: key".to_string())?;
    let deterministic = matches!(key, SigningKey::Rsa(_));
    let tags = match lines[2] {
        "" => None,
        "-" => Some(Vec::new()),
        list => {
            let mut tags = Vec::new();
            for text in list.split(',') {
                let v = u32::from_str_radix(text, 16).map_err(|_| bad_opts())?;
                if text.len() != 8 {
                    return Err(bad_opts());
                }
                tags.push(Tag {
                    group: (v >> 16) as u16,
                    element: v as u16,
                });
            }
            Some(tags)
        }
    };
    let purpose = if lines[3].is_empty() {
        None
    } else {
        let parts: Vec<&str> = lines[3].splitn(3, '|').collect();
        if parts.len() != 3 {
            return Err(bad_opts());
        }
        Some(Code {
            value: parts[0].to_string(),
            scheme: parts[1].to_string(),
            meaning: parts[2].to_string(),
        })
    };
    let so = SignOptions {
        certificate,
        key,
        mac_algorithm: lines[0].to_string(),
        mac_transfer_syntax_uid: lines[1].to_string(),
        tags,
        purpose,
        profile: Profile::from_number(numbers[0]),
        profile_options: ProfileOptions {
            human_verified_paths: match lines[5] {
                "" => None,
                "-" => Some(Vec::new()),
                paths => Some(paths.split('|').map(str::to_string).collect()),
            },
            max_elements: numbers[1],
            max_depth: numbers[2],
            max_issues: numbers[3],
        },
        time: DateTime::from_timestamp(numbers[4], (numbers[5] * 1000) as u32)
            .ok_or_else(bad_opts)?,
        uid: lines[12].to_string(),
        item: lines[9].to_string(),
    };
    let mut b = String::new();
    let random_uid = if deterministic { "" } else { lines[12] };
    let sig = match dsig::sign(&mut f.dataset, &so) {
        Ok(sig) => sig,
        Err(err) => {
            b.push_str(&format!(
                "sign={} {}\n",
                kind_of(&err),
                quote(&err.to_string())
            ));
            let found = !err.to_string().contains("after signing");
            state(&mut b, &f, deterministic || found, random_uid);
            return Ok(b);
        }
    };
    let signature = if deterministic {
        format!(
            "{}:{}",
            sig.signature.len(),
            hex::encode(Sha256::digest(&sig.signature))
        )
    } else {
        "random".to_string()
    };
    b.push_str(&format!(
        "sign=ok path={} macid={} uid={} certtype={} cert={} signature={signature}\n",
        quote(&sig.path),
        sig.mac_id_number,
        quote(&sig.uid),
        quote(&sig.certificate_type),
        sig.certificate_of_signer.len(),
    ));
    state(&mut b, &f, deterministic, "");
    match dsig::remove_signature(&mut f.dataset, &sig) {
        Ok(()) => b.push_str("remove=ok\n"),
        Err(err) => b.push_str(&format!("remove={}\n", quote(&err.to_string()))),
    }
    state(&mut b, &f, true, random_uid);
    Ok(b)
}
pub fn remove(data: Vec<i64>, path: String, uid: String) -> Result<String, String> {
    let mut f = read(&data)?;
    let mut b = String::new();
    let mut sig = dsig::Signature::default();
    sig.path = path;
    sig.uid = uid;
    match dsig::remove_signature(&mut f.dataset, &sig) {
        Ok(()) => b.push_str("remove=ok\n"),
        Err(err) => b.push_str(&format!(
            "remove={} {}\n",
            kind_of(&err),
            quote(&err.to_string())
        )),
    }
    state(&mut b, &f, true, "");
    Ok(b)
}
pub fn remove_all(data: Vec<i64>) -> Result<String, String> {
    let mut f = read(&data)?;
    let mut b = String::new();
    match dsig::remove_all_signatures(&mut f.dataset) {
        Ok(n) => b.push_str(&format!("removeall={n}\n")),
        Err(err) => b.push_str(&format!(
            "removeall={} {}\n",
            kind_of(&err),
            quote(&err.to_string())
        )),
    }
    state(&mut b, &f, true, "");
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Sign" => {
            let out = sign(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Remove" => {
            let out = remove(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "RemoveAll" => {
            let out = remove_all(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
