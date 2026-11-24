#[allow(unused_imports)]
use super::support::*;
use dicom_rust::charset::SpecificCharacterSet;
use dicom_rust::object::charset_conversion::{CharsetConversionOptions, CharsetConversionPolicy};
use dicom_rust::object::charset_encode::{
    CharsetEncodeOptions, CharsetEncodeReport, UnrepresentablePolicy,
    convert_text_to_character_set, lookup_string_replacing, lookup_strings_replacing,
};
use dicom_rust::object::parser::source::MemFile;
use dicom_rust::object::{
    self, CharsetFallbackCases, Element, ElementHeader, Object, ReadFileOptions, Tag, TextOptions,
    Value, Vr, transfer,
};
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
use std::rc::Rc;
const MAX_INPUT: usize = 2 << 20;
const FALLBACKS: [&str; 7] = [
    "",
    "ISO_IR 100",
    "ISO_IR 192",
    "ISO 2022 IR 87",
    "ISO_IR 144",
    "GB18030",
    "ISO 2022 IR 6\\ISO 2022 IR 87",
];
fn payload(data: &[u8]) -> String {
    if data.len() <= 96 {
        return hex::encode(data);
    }
    let sum = Sha256::digest(data);
    format!("len={} sha256={}", data.len(), hex::encode(&sum[..8]))
}
const GO_UNASSIGNED: [(char, &str); 3] = [
    ('\u{0897}', "\\u0897"),
    ('\u{105C9}', "\\U000105c9"),
    ('\u{11F5A}', "\\U00011f5a"),
];
fn reference_quote(s: &str) -> String {
    let mut q = quote(s);
    for (r, escaped) in GO_UNASSIGNED {
        q = q.replace(r, escaped);
    }
    q
}
fn quote_list(values: &[String]) -> String {
    let parts: Vec<String> = values.iter().map(|v| reference_quote(v)).collect();
    format!("[{}]", parts.join(" "))
}
fn dump(b: &mut String, elements: &[Element], indent: &str) {
    let mut sorted: Vec<&Element> = elements.iter().collect();
    sorted.sort_by_key(|e| e.header.tag);
    for e in sorted {
        let length = if e.header.length_set {
            e.header.length.to_string()
        } else {
            "unset".to_string()
        };
        b.push_str(&format!(
            "{indent}{} {} {length} {}",
            e.tag(),
            e.vr(),
            e.header.length
        ));
        match &e.value {
            Value::Nil => b.push_str(" nil\n"),
            Value::Raw(raw) => b.push_str(&format!(" raw {}\n", payload(raw))),
            Value::Strings(values) => b.push_str(&format!(" strings {}\n", quote_list(values))),
            Value::Sequence(items) => {
                b.push_str(&format!(" items {}\n", items.len()));
                for (i, item) in items.iter().enumerate() {
                    b.push_str(&format!("{indent}  item {i}\n"));
                    dump(b, &item.elements, &format!("{indent}    "));
                }
            }
            Value::Fragments { .. } => b.push_str(" kind 3\n"),
            Value::BulkData(_) => b.push_str(" kind 4\n"),
        }
    }
}
fn read(data: Vec<i64>, syntax_uid: &str, part10: bool) -> Result<Object, String> {
    if data.len() > MAX_INPUT {
        return Err("harness: input too large".to_string());
    }
    let data: Vec<u8> = data.into_iter().map(|b| b as u8).collect();
    if part10 {
        let opts = ReadFileOptions {
            skip_pixel_data: true,
            ..Default::default()
        };
        return match object::read_file_with_options(&data, opts) {
            Ok(f) => Ok(f.dataset),
            Err((_, err)) => Err(format!("read error {err}\n")),
        };
    }
    let Some(syntax) = transfer::lookup(syntax_uid) else {
        return Err("harness: unknown syntax".to_string());
    };
    let source = Box::new(MemFile::new(Rc::from(data.as_slice())));
    object::read_data_set_with_options(source, syntax, ReadFileOptions::default())
        .map_err(|err| format!("read error {err}\n"))
}
fn text_options(
    allow_unsupported: bool,
    fallback: i64,
    undeclared: bool,
    undecodable: bool,
) -> Result<TextOptions, String> {
    let mut text = TextOptions {
        allow_unsupported_charset_fallback: allow_unsupported,
        ..TextOptions::default()
    };
    if fallback > 0 && (fallback as usize) < FALLBACKS.len() {
        let codes: Vec<&str> = FALLBACKS[fallback as usize].split('\\').collect();
        match SpecificCharacterSet::parse(&codes) {
            Ok(cs) => text.fallback_character_set = cs,
            Err(err) => return Err(format!("harness: {err}")),
        }
    }
    if undeclared {
        text.charset_fallback = text.charset_fallback | CharsetFallbackCases::UNDECLARED;
    }
    if undecodable {
        text.charset_fallback = text.charset_fallback | CharsetFallbackCases::UNDECODABLE;
    }
    Ok(text)
}
fn authored_elements(spec: &str) -> Vec<Element> {
    let mut out = Vec::new();
    for line in spec.split('\n') {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() < 3 || parts[0].len() != 8 || parts[1].len() != 2 {
            continue;
        }
        let Ok(key) = u32::from_str_radix(parts[0], 16) else {
            continue;
        };
        let Some(vr) = Vr::parse(parts[1].as_bytes()) else {
            continue;
        };
        out.push(Element {
            header: ElementHeader {
                tag: Tag::new((key >> 16) as u16, key as u16),
                vr,
                length: 0,
                length_set: false,
            },
            value: Value::Strings(parts[2].split('\\').map(str::to_string).collect()),
        });
    }
    out
}
pub fn encode_charset(
    data: Vec<i64>,
    syntax_uid: String,
    part10: bool,
    target: String,
    unrep: i64,
    policy: i64,
    max_diagnostics: i64,
    allow_unsupported: bool,
    fallback: i64,
    undeclared: bool,
    undecodable: bool,
    authored: String,
) -> Result<String, String> {
    let mut obj = match read(data, &syntax_uid, part10) {
        Ok(obj) => obj,
        Err(msg) => return Ok(msg),
    };
    for e in authored_elements(&authored) {
        obj.put(e);
    }
    match text_options(allow_unsupported, fallback, undeclared, undecodable) {
        Ok(text) => obj.set_text_options(text),
        Err(msg) => return Ok(msg),
    }
    let opts = CharsetEncodeOptions {
        unrepresentable: match unrep {
            1 => UnrepresentablePolicy::Transliterate,
            2 => UnrepresentablePolicy::Discard,
            _ => UnrepresentablePolicy::Fail,
        },
        decode: CharsetConversionOptions {
            policy: if policy != 0 {
                CharsetConversionPolicy::KeepScope
            } else {
                CharsetConversionPolicy::Atomic
            },
            max_diagnostics,
        },
    };
    let mut b = String::new();
    let (report, err): (CharsetEncodeReport, _) =
        match convert_text_to_character_set(&obj, &target, opts) {
            Ok((out, report)) => {
                dump(&mut b, out.elements(), "");
                for tag in [
                    Tag::new(0x0010, 0x0010),
                    Tag::new(0x0008, 0x0080),
                    Tag::new(0x0010, 0x4000),
                    Tag::new(0x0008, 0x0005),
                ] {
                    match out.lookup_strings(tag) {
                        Ok(values) => b.push_str(&format!("text {tag} {}\n", quote_list(&values))),
                        Err(err) if out.has(tag) => {
                            b.push_str(&format!("text {tag} error {err}\n"))
                        }
                        Err(_) => {}
                    }
                }
                (report, None)
            }
            Err(failure) => {
                let failure = *failure;
                (failure.report, Some(failure.error))
            }
        };
    b.push_str(&format!(
        "report converted={} fallback={} kept={} truncated={} encoded={} transliterated={} discarded={}\n",
        report.converted_values,
        report.fallback_values,
        report.kept_scopes,
        report.diagnostics_truncated,
        report.encoded_values,
        report.transliterated_characters,
        report.discarded_characters
    ));
    for d in &report.diagnostics {
        b.push_str(&format!(
            "  {} {} {}\n",
            d.path,
            d.reason,
            quote(&d.declared)
        ));
    }
    b.push_str("original-unchanged true\n");
    if let Some(err) = err {
        b.push_str(&format!("error {err}\n"));
    }
    Ok(b)
}
pub fn lookup_replacing(
    data: Vec<i64>,
    syntax_uid: String,
    part10: bool,
    tag: i64,
    seq_tag: i64,
    index: i64,
    allow_unsupported: bool,
    fallback: i64,
    undeclared: bool,
    undecodable: bool,
) -> Result<String, String> {
    let mut obj = match read(data, &syntax_uid, part10) {
        Ok(obj) => obj,
        Err(msg) => return Ok(msg),
    };
    match text_options(allow_unsupported, fallback, undeclared, undecodable) {
        Ok(text) => obj.set_text_options(text),
        Err(msg) => return Ok(msg),
    }
    if seq_tag != 0 {
        let seq = seq_tag as u32;
        let items = obj.get_sequence(Tag::new((seq >> 16) as u16, seq as u16));
        match items {
            Some(items) if index >= 0 && (index as usize) < items.len() => {
                obj = items.into_iter().nth(index as usize).unwrap_or_default();
            }
            _ => return Ok("no item".to_string()),
        }
    }
    let tag = tag as u32;
    let t = Tag::new((tag >> 16) as u16, tag as u16);
    let mut b = String::new();
    match lookup_strings_replacing(&obj, t) {
        Err(err) => b.push_str(&format!("strings error {err} replaced=false\n")),
        Ok((values, replaced)) => b.push_str(&format!(
            "strings {} nil={} replaced={replaced}\n",
            quote_list(&values),
            values.is_empty()
        )),
    }
    match lookup_string_replacing(&obj, t) {
        Err(err) => b.push_str(&format!("string error {err} replaced=false\n")),
        Ok((value, replaced)) => {
            b.push_str(&format!("string {} replaced={replaced}\n", quote(&value)))
        }
    }
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "EncodeCharset" => {
            let out = encode_charset(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
                arg(args, 7)?,
                arg(args, 8)?,
                arg(args, 9)?,
                arg(args, 10)?,
                arg(args, 11)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "LookupReplacing" => {
            let out = lookup_replacing(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
                arg(args, 7)?,
                arg(args, 8)?,
                arg(args, 9)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
