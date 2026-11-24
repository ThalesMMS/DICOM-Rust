#[allow(unused_imports)]
use super::support::*;
use dicom_rust::charset::{
    SpecificCharacterSet, read_default_repertoire_replacing as read_default,
};
use dicom_rust::textfmt::quote;
use serde_json::json;
fn parse(codes: &str) -> Result<SpecificCharacterSet, String> {
    let result = if codes.is_empty() {
        SpecificCharacterSet::parse::<&str>(&[])
    } else {
        SpecificCharacterSet::parse(&codes.split('\\').collect::<Vec<_>>())
    };
    result.map_err(|e| e.to_string())
}
fn bytes(text: &[i64]) -> Vec<u8> {
    text.iter().map(|&b| b as u8).collect()
}
fn unknown_mode(mode: &str) -> String {
    format!("harness: unknown mode {}", quote(mode))
}
pub fn names(codes: String) -> Result<String, String> {
    Ok(parse(&codes)?.name())
}
pub fn decode(codes: String, mode: String, text: Vec<i64>) -> Result<String, String> {
    let cs = parse(&codes)?;
    let text = bytes(&text);
    let result = match mode.as_str() {
        "multi" => cs.decode(&text),
        "single" => cs.decode_single_value(&text),
        "pn" => cs.decode_person_name(&text),
        _ => return Err(unknown_mode(&mode)),
    };
    result.map_err(|e| e.to_string())
}
pub fn encode(codes: String, mode: String, text: String) -> Result<Option<Vec<i64>>, String> {
    let cs = parse(&codes)?;
    let result = match mode.as_str() {
        "multi" => cs.encode(&text),
        "single" => cs.encode_single_value(&text),
        "pn" => cs.encode_person_name(&text),
        _ => return Err(unknown_mode(&mode)),
    };
    let raw = result.map_err(|e| e.to_string())?;
    if mode == "pn" && text.is_empty() && !cs.uses_code_extensions() {
        return Ok(None);
    }
    Ok(Some(raw.into_iter().map(i64::from).collect()))
}
pub fn decode_replacing(
    codes: String,
    mode: String,
    text: Vec<i64>,
) -> Result<serde_json::Value, String> {
    let cs = parse(&codes)?;
    let text = bytes(&text);
    let (decoded, replaced) = match mode.as_str() {
        "multi" => cs.decode_replacing(&text),
        "single" => cs.decode_single_value_replacing(&text),
        "pn" => cs.decode_person_name_replacing(&text),
        _ => return Err(unknown_mode(&mode)),
    };
    Ok(json!({"text": decoded, "replaced": replaced}))
}
pub fn read_default_repertoire_replacing(text: Vec<i64>) -> Result<serde_json::Value, String> {
    let (decoded, replaced) = read_default(&bytes(&text));
    Ok(json!({"text": decoded, "replaced": replaced}))
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Names" => {
            let out = names(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Decode" => {
            let out = decode(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Encode" => {
            let out = encode(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DecodeReplacing" => {
            let out = decode_replacing(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ReadDefaultRepertoireReplacing" => {
            let out = read_default_repertoire_replacing(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
