#[allow(unused_imports)]
use super::support::*;
use dicom_rust::dictionary::dic::{DicOptions, parse_dic};
use dicom_rust::object::private::{
    PrivateDictionaryOptions, read_data_set_with_private_dictionary,
    read_file_with_private_dictionary,
};
use dicom_rust::object::{self, Element, Kind, Object, ReadFileOptions, Value, transfer};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const MAX_INPUT: usize = 4 << 20;
const HARNESS: &str = "harness: input rejected";
const F_SKIP_PIXEL_DATA: i64 = 1;
const F_ALLOW_PARTIAL_READ: i64 = 2;
const F_REJECT: i64 = 4;
const F_NO_CATALOG: i64 = 8;
const F_END: i64 = 16;
fn rejected() -> String {
    HARNESS.to_string()
}
fn payload(data: &[u8]) -> String {
    if data.len() <= 16 {
        return hex::encode(data);
    }
    let sum = Sha256::digest(data);
    format!("len={} sha256={}", data.len(), hex::encode(&sum[..6]))
}
fn dump(b: &mut String, elements: &[Element], indent: &str) {
    for e in elements {
        let (tag, vr) = (e.tag(), e.vr().as_str());
        match &e.value {
            Value::Sequence(items) => {
                b.push_str(&format!("{indent}{tag} {vr} items={}\n", items.len()));
                for (i, item) in items.iter().enumerate() {
                    b.push_str(&format!("{indent} item {i}\n"));
                    dump(b, &item.elements, &format!("{indent}  "));
                }
            }
            Value::Fragments { fragments, .. } => {
                b.push_str(&format!(
                    "{indent}{tag} {vr} fragments={}\n",
                    fragments.len()
                ));
            }
            Value::Raw(raw) => b.push_str(&format!("{indent}{tag} {vr} {}\n", payload(raw))),
            Value::Nil => b.push_str(&format!("{indent}{tag} {vr} nil\n")),
            _ => b.push_str(&format!("{indent}{tag} {vr} other\n")),
        }
    }
}
pub fn read(
    data: Vec<i64>,
    syntax_uid: String,
    dic: String,
    max_creators: i64,
    max_diagnostics: i64,
    flags: i64,
) -> Result<String, String> {
    let limit = 1i64 << 30;
    if data.len() > MAX_INPUT
        || !(0..F_END).contains(&flags)
        || dic.len() > 1 << 16
        || !(-limit..=limit).contains(&max_creators)
        || !(-limit..=limit).contains(&max_diagnostics)
    {
        return Err(rejected());
    }
    let opts = ReadFileOptions {
        skip_pixel_data: flags & F_SKIP_PIXEL_DATA != 0,
        allow_partial_read: flags & F_ALLOW_PARTIAL_READ != 0,
        ..ReadFileOptions::default()
    };
    let mut private = None;
    let mut b = String::new();
    if flags & F_NO_CATALOG == 0 {
        let options = DicOptions {
            tolerant: true,
            ..DicOptions::default()
        };
        let (d, report) = match parse_dic(dic.as_bytes(), &options) {
            Ok(v) => v,
            Err(err) => return Ok(format!("dic error {err}\n")),
        };
        b.push_str(&format!(
            "dic entries={} skipped={} private={}\n",
            d.len(),
            report.skipped.len(),
            d.has_private_definitions()
        ));
        private = Some(PrivateDictionaryOptions {
            catalog: Arc::new(d),
            max_private_creators: max_creators,
            max_private_diagnostics: max_diagnostics,
            reject_invalid_private_creators: flags & F_REJECT != 0,
        });
    }
    let data: Vec<u8> = data.iter().map(|b| *b as u8).collect();
    let root: Object;
    if syntax_uid.is_empty() {
        let result = match &private {
            Some(private) => read_file_with_private_dictionary(&data, opts, private),
            None => object::read_file_with_options(&data, opts),
        };
        let f = match result {
            Ok(f) => f,
            Err((f, err)) => {
                b.push_str(&format!("error {err}\n"));
                match f {
                    Some(f) if err.is(Kind::PartialRead) => *f,
                    _ => return Ok(b),
                }
            }
        };
        b.push_str(&format!("transfer-syntax {}\n", f.transfer_syntax.uid));
        root = f.dataset;
    } else {
        let Some(syntax) = transfer::lookup(&syntax_uid) else {
            return Err(rejected());
        };
        let result = match &private {
            Some(private) => read_data_set_with_private_dictionary(&data, syntax, opts, private),
            None => object::read_data_set_with_options(
                Box::new(object::parser::source::MemFile::new(std::rc::Rc::from(
                    data.as_slice(),
                ))),
                syntax,
                opts,
            ),
        };
        match result {
            Ok(obj) => root = obj,
            Err(err) => {
                b.push_str(&format!("error {err}\n"));
                return Ok(b);
            }
        }
    }
    dump(&mut b, root.elements(), "");
    let (found, truncated) = root.parsed_private_diagnostics();
    for d in found {
        let item = match d.item_offset {
            Some(offset) => offset.to_string(),
            None => "-".to_string(),
        };
        b.push_str(&format!(
            "diagnostic {} offset={} item={item} {}\n",
            d.tag, d.offset, d.err
        ));
    }
    b.push_str(&format!("truncated {truncated}\n"));
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Read" => {
            let out = read(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
