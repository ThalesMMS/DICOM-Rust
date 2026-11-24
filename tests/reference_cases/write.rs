#[allow(unused_imports)]
use super::support::*;
use dicom_rust::object::writer::{self, FileMetaAeTitles, WriteDataSetOptions, WriteFileOptions};
use dicom_rust::object::{
    self, Element, ElementHeader, File, Object, ReadResult, Syntax, Tag, UNDEFINED_LENGTH, Value,
    Vr, transfer,
};
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
fn render(out: &[u8]) -> String {
    let sum = Sha256::digest(out);
    let mut s = format!("len={} sha256={}\n", out.len(), hex::encode(sum));
    if out.len() <= 4096 {
        s.push_str(&hex::encode(out));
        s.push('\n');
    }
    s
}
fn parse_tag(s: &str) -> Result<Tag, String> {
    let bad = || format!("objwrite: bad tag {}", quote(s));
    if s.len() != 8 {
        return Err(bad());
    }
    let v = u32::from_str_radix(s, 16).map_err(|_| bad())?;
    Ok(Tag::new((v >> 16) as u16, v as u16))
}
fn vr(s: &str) -> Result<Vr, String> {
    Vr::parse(s.as_bytes()).ok_or_else(|| format!("objwrite: bad VR {}", quote(s)))
}
fn apply(o: &mut Object, ops: &str) -> Result<(), String> {
    for op in ops.split(';') {
        let fields: Vec<&str> = op.trim().splitn(4, ' ').collect();
        if fields.len() < 2 {
            continue;
        }
        let tag = parse_tag(fields[1])?;
        match fields[0] {
            "R" => {
                o.remove(tag);
            }
            "P" => {
                let raw = match fields.get(3) {
                    Some(h) => hex::decode(h).map_err(|e| e.to_string())?,
                    None => Vec::new(),
                };
                o.put(Element {
                    header: ElementHeader::new(tag, vr(fields[2])?, raw.len() as u32),
                    value: Value::Raw(raw),
                });
            }
            "S" => {
                let values = fields
                    .get(3)
                    .map(|v| v.split('|').map(str::to_string).collect())
                    .unwrap_or_default();
                o.put(Element {
                    header: ElementHeader {
                        tag,
                        vr: vr(fields[2])?,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Strings(values),
                });
            }
            "Q" | "N" => {
                let items = if fields[0] == "N" {
                    vec![object::DataSet::default()]
                } else {
                    Vec::new()
                };
                o.put(Element {
                    header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
                    value: Value::Sequence(items),
                });
            }
            _ => {}
        }
    }
    Ok(())
}
fn options(seq: i64, group: i64, mode: i64, ae: &str, keep: bool) -> WriteFileOptions {
    let mut opts = WriteFileOptions {
        sequence_length: seq as u8,
        group_length: group as u8,
        meta_mode: mode,
        keep_partial_byte_order_units: keep,
        ..WriteFileOptions::default()
    };
    if !ae.is_empty() {
        let mut parts: Vec<String> = ae.split('|').map(str::to_string).collect();
        parts.resize(3, String::new());
        opts.ae_titles = FileMetaAeTitles {
            source: parts[0].clone(),
            sending: parts[1].clone(),
            receiving: parts[2].clone(),
        };
    }
    opts
}
fn set_syntax(f: &mut File, ts: &str) -> Result<(), String> {
    if ts.is_empty() {
        return Ok(());
    }
    if let Some(syntax) = transfer::lookup(ts) {
        f.transfer_syntax = syntax;
        return Ok(());
    }
    if transfer::normalize_uid(ts).is_empty() {
        f.transfer_syntax = Syntax {
            uid: "",
            name: "",
            explicit_vr: false,
            big_endian: false,
            supported: false,
            encapsulated: false,
            deflated: false,
        };
        return Ok(());
    }
    Err(writer::writable_syntax(ts)
        .err()
        .map_or_else(String::new, |e| e.to_string()))
}
fn read(result: ReadResult) -> Result<File, String> {
    result.map_err(|(_, e)| e.to_string())
}
fn write_file(
    result: ReadResult,
    ts: &str,
    ops: &str,
    meta_ops: &str,
    seq: i64,
    group: i64,
    mode: i64,
    ae: &str,
    keep: bool,
) -> Result<String, String> {
    let mut f = read(result)?;
    let syntax = set_syntax(&mut f, ts);
    apply(&mut f.dataset, ops)?;
    apply(&mut f.meta, meta_ops)?;
    syntax?;
    let out = writer::write_file_with_options(&f, &options(seq, group, mode, ae, keep))
        .map_err(|e| e.to_string())?;
    Ok(render(&out))
}
pub fn write(
    path: String,
    ts: String,
    ops: String,
    meta_ops: String,
    seq_len: i64,
    group_len: i64,
    meta_mode: i64,
    ae_titles: String,
    keep_partial: bool,
) -> Result<String, String> {
    write_file(
        object::open_file(&path),
        &ts,
        &ops,
        &meta_ops,
        seq_len,
        group_len,
        meta_mode,
        &ae_titles,
        keep_partial,
    )
}
fn bytes(data: Vec<i64>) -> Vec<u8> {
    data.into_iter().map(|b| b as u8).collect()
}
pub fn write_bytes(
    data: Vec<i64>,
    ts: String,
    ops: String,
    meta_ops: String,
    seq_len: i64,
    group_len: i64,
    meta_mode: i64,
    ae_titles: String,
    keep_partial: bool,
) -> Result<String, String> {
    write_file(
        object::read_file(&bytes(data)),
        &ts,
        &ops,
        &meta_ops,
        seq_len,
        group_len,
        meta_mode,
        &ae_titles,
        keep_partial,
    )
}
pub fn write_data_set_bytes(
    data: Vec<i64>,
    ts: String,
    ops: String,
    seq_len: i64,
    group_len: i64,
) -> Result<String, String> {
    let mut f = read(object::read_file(&bytes(data)))?;
    let uid = if ts.is_empty() {
        f.transfer_syntax.uid.to_string()
    } else {
        ts
    };
    apply(&mut f.dataset, &ops)?;
    let out = writer::write_data_set_with_options(
        &f.dataset,
        &uid,
        WriteDataSetOptions {
            sequence_length: seq_len as u8,
            group_length: group_len as u8,
            ..WriteDataSetOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(render(&out))
}
pub fn rebuild_meta(
    data: Vec<i64>,
    ts: String,
    ops: String,
    meta_mode: i64,
    ae_titles: String,
) -> Result<String, String> {
    let mut f = read(object::read_file(&bytes(data)))?;
    let syntax = set_syntax(&mut f, &ts);
    apply(&mut f.dataset, &ops)?;
    syntax?;
    f.rebuild_file_meta_with_options(&options(0, 0, meta_mode, &ae_titles, false))
        .map_err(|e| e.to_string())?;
    let mut b = format!("syntax {}\n", f.transfer_syntax.uid);
    for e in f.meta.elements() {
        let s = f.meta.get_string(e.tag()).unwrap_or_default();
        b.push_str(&format!(
            "{} {} {} {}\n",
            e.tag(),
            e.vr(),
            quote(&s),
            hex::encode(e.raw().unwrap_or_default())
        ));
    }
    Ok(b)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Write" => {
            let out = write(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
                arg(args, 7)?,
                arg(args, 8)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "WriteBytes" => {
            let out = write_bytes(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
                arg(args, 7)?,
                arg(args, 8)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "WriteDataSetBytes" => {
            let out = write_data_set_bytes(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "RebuildMeta" => {
            let out = rebuild_meta(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
