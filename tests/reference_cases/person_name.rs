#[allow(unused_imports)]
use super::support::*;
use dicom_rust::personname::{
    self as pn, GroupInfo, GroupTrailingNullLevel, Info, InfoTrailingNullLevel,
};
use dicom_rust::textfmt::quote;
use serde_json::{Map, Value, json};
fn level(v: i64) -> u64 {
    v as u64
}
fn put_group(out: &mut Map<String, Value>, prefix: &str, g: &GroupInfo) -> Result<(), String> {
    let fields = [
        ("FamilyName", json!(g.family_name)),
        ("GivenName", json!(g.given_name)),
        ("MiddleName", json!(g.middle_name)),
        ("NamePrefix", json!(g.name_prefix)),
        ("NameSuffix", json!(g.name_suffix)),
        ("TrailingNullLevel", json!(g.trailing_null_level.0)),
        ("LevelName", json!(g.trailing_null_level.to_string())),
        ("IsEmpty", json!(g.is_empty())),
        ("Display", json!(g.display())),
        ("Dcm", json!(g.dcm().map_err(|e| e.to_string())?)),
    ];
    for (name, value) in fields {
        out.insert(format!("{prefix}{name}"), value);
    }
    Ok(())
}
fn info_json(i: &Info) -> Result<Value, String> {
    let mut out = Map::new();
    put_group(&mut out, "a", &i.alphabetic)?;
    put_group(&mut out, "i", &i.ideographic)?;
    put_group(&mut out, "p", &i.phonetic)?;
    out.insert("trailingNullLevel".into(), json!(i.trailing_null_level.0));
    out.insert("levelName".into(), json!(i.trailing_null_level.to_string()));
    out.insert("isEmpty".into(), json!(i.is_empty()));
    out.insert("display".into(), json!(i.display()));
    out.insert("dcm".into(), json!(i.dcm().map_err(|e| e.to_string())?));
    Ok(Value::Object(out))
}
fn parse_info(value: &str) -> Result<Info, String> {
    Info::parse(value).map_err(|e| e.to_string())
}
pub fn parse(value: String) -> Result<Value, String> {
    info_json(&parse_info(&value)?)
}
pub fn parse_error_kinds(value: String) -> Result<Value, String> {
    let err = Info::parse(&value).err();
    let is = |f: fn(&pn::PersonNameError) -> bool| err.as_ref().is_some_and(f);
    Ok(json!({
        "ok": err.is_none(),
        "parse": is(pn::PersonNameError::is_parse_error),
        "groupCount": is(pn::PersonNameError::is_group_count),
        "segmentCount": is(pn::PersonNameError::is_group_segment_count),
        "nullSepLevel": is(pn::PersonNameError::is_null_sep_level_invalid),
    }))
}
pub fn display(value: String) -> Result<String, String> {
    Ok(pn::display(&value))
}
pub fn reformat(value: String, mode: String) -> Result<Value, String> {
    let info = parse_info(&value)?;
    let info = match mode.as_str() {
        "asis" => info,
        "with" => info.with_trailing_nulls(),
        "without" => info.without_trailing_nulls(),
        "withoutEmpty" => info.without_empty_groups(),
        _ => return Err(format!("harness: unknown mode {}", quote(&mode))),
    };
    info_json(&info)
}
pub fn format_dcm(
    value: String,
    info_level: i64,
    alphabetic: i64,
    ideographic: i64,
    phonetic: i64,
) -> Result<String, String> {
    parse_info(&value)?
        .with_format(
            InfoTrailingNullLevel(level(info_level)),
            GroupTrailingNullLevel(level(alphabetic)),
            GroupTrailingNullLevel(level(ideographic)),
            GroupTrailingNullLevel(level(phonetic)),
        )
        .dcm()
        .map_err(|e| e.to_string())
}
pub fn group_dcm(
    family: String,
    given: String,
    middle: String,
    prefix: String,
    suffix: String,
    level_: i64,
) -> Result<String, String> {
    GroupInfo {
        family_name: family,
        given_name: given,
        middle_name: middle,
        name_prefix: prefix,
        name_suffix: suffix,
        trailing_null_level: GroupTrailingNullLevel(level(level_)),
    }
    .dcm()
    .map_err(|e| e.to_string())
}
pub fn group_level_name(level_: i64) -> Result<String, String> {
    Ok(GroupTrailingNullLevel(level(level_)).to_string())
}
pub fn info_level_name(level_: i64) -> Result<String, String> {
    Ok(InfoTrailingNullLevel(level(level_)).to_string())
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Parse" => {
            let out = parse(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ParseErrorKinds" => {
            let out = parse_error_kinds(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Display" => {
            let out = display(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Reformat" => {
            let out = reformat(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "FormatDCM" => {
            let out = format_dcm(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "GroupDCM" => {
            let out = group_dcm(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "GroupLevelName" => {
            let out = group_level_name(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "InfoLevelName" => {
            let out = info_level_name(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
