#[allow(unused_imports)]
use super::support::*;
use dicom_rust::deflater;
use dicom_rust::object::writer::{self, PaddingOptions, WriteDataSetOptions, WriteFileOptions};
use dicom_rust::object::{self, File, ReadResult, Syntax, transfer};
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
fn generate(kind: i64, size: i64, seed: i64) -> Vec<u8> {
    let size = if !(0..=1 << 22).contains(&size) {
        0
    } else {
        size as usize
    };
    let mut out = vec![0u8; size];
    let mut x = (seed as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(1);
    let mut next = || {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        x >> 33
    };
    let phrase = b"the quick brown fox jumps over the lazy dog; DICOM ";
    let period = 32000 + seed % 768;
    for i in 0..out.len() {
        out[i] = match kind {
            0 => next() as u8,
            1 => b'a' + (next() % 6) as u8,
            2 => {
                let mut v = phrase[i % phrase.len()];
                if next() % 97 == 0 {
                    v = next() as u8;
                }
                v
            }
            3 => {
                if next() % 61 == 0 {
                    next() as u8
                } else {
                    0
                }
            }
            5 => {
                if i as i64 >= period {
                    out[i - period as usize]
                } else {
                    next() as u8
                }
            }
            _ => {
                let mut v = ((i / 2) as u16).wrapping_mul(3);
                if next() % 13 == 0 {
                    v = v.wrapping_add((next() % 5) as u16);
                }
                (v >> (8 * (i % 2))) as u8
            }
        };
    }
    out
}
fn compress(data: &[u8], level: i64, chunks: &[i64]) -> Result<String, String> {
    let level = i32::try_from(level).map_err(|_| {
        format!("flate: invalid compression level {level}: want value in range [-2, 9]")
    })?;
    let mut ends = Vec::new();
    let mut pos = 0usize;
    let mut i = 0usize;
    while pos < data.len() {
        let mut n = data.len() - pos;
        if !chunks.is_empty() {
            let c = chunks[i % chunks.len()];
            if c > 0 && (c as u64) < n as u64 {
                n = c as usize;
            }
        }
        pos += n;
        ends.push(pos);
        i += 1;
    }
    let out = deflater::compress_chunks(data, &ends, level)?;
    Ok(render(&out))
}
pub fn deflate(
    kind: i64,
    size: i64,
    seed: i64,
    level: i64,
    chunks: Vec<i64>,
) -> Result<String, String> {
    compress(&generate(kind, size, seed), level, &chunks)
}
fn bytes(data: Vec<i64>) -> Vec<u8> {
    data.into_iter().map(|b| b as u8).collect()
}
pub fn deflate_bytes(data: Vec<i64>, level: i64, chunks: Vec<i64>) -> Result<String, String> {
    compress(&bytes(data), level, &chunks)
}
fn level_opt(level: i64) -> Option<i64> {
    (level != -100).then_some(level)
}
fn read(result: ReadResult) -> Result<File, String> {
    result.map_err(|(_, e)| e.to_string())
}
const EMPTY_SYNTAX: Syntax = Syntax {
    uid: "",
    name: "",
    explicit_vr: false,
    big_endian: false,
    supported: false,
    encapsulated: false,
    deflated: false,
};
fn padding(policy: i64, alignment: i64, item_alignment: i64, max_added: i64) -> PaddingOptions {
    PaddingOptions {
        policy: policy as u8,
        alignment: alignment as u32,
        item_alignment: item_alignment as u32,
        max_added_bytes: max_added,
    }
}
fn write_file_result(
    result: ReadResult,
    ts: &str,
    level: i64,
    seq: i64,
    group: i64,
    padding: PaddingOptions,
) -> Result<String, String> {
    let mut f = read(result)?;
    if !ts.is_empty() {
        match transfer::lookup(ts) {
            Some(syntax) => f.transfer_syntax = syntax,
            None if transfer::normalize_uid(ts).is_empty() => f.transfer_syntax = EMPTY_SYNTAX,
            None => {
                return Err(writer::writable_syntax(ts)
                    .err()
                    .map_or_else(String::new, |e| e.to_string()));
            }
        }
    }
    let opts = WriteFileOptions {
        sequence_length: seq as u8,
        group_length: group as u8,
        deflate_level: level_opt(level),
        padding,
        ..WriteFileOptions::default()
    };
    let out = writer::write_file_with_options(&f, &opts).map_err(|e| e.to_string())?;
    Ok(render(&out))
}
pub fn write_file(
    path: String,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
) -> Result<String, String> {
    write_file_result(
        object::open_file(&path),
        &ts,
        level,
        seq_len,
        group_len,
        PaddingOptions::default(),
    )
}
pub fn write_bytes(
    data: Vec<i64>,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
) -> Result<String, String> {
    write_file_result(
        object::read_file(&bytes(data)),
        &ts,
        level,
        seq_len,
        group_len,
        PaddingOptions::default(),
    )
}
pub fn write_data_set(
    data: Vec<i64>,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
) -> Result<String, String> {
    write_data_set_with(
        data,
        ts,
        level,
        seq_len,
        group_len,
        PaddingOptions::default(),
    )
}
fn write_data_set_with(
    data: Vec<i64>,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
    padding: PaddingOptions,
) -> Result<String, String> {
    let f = read(object::read_file(&bytes(data)))?;
    let uid = if ts.is_empty() {
        f.transfer_syntax.uid.to_string()
    } else {
        ts
    };
    let out = writer::write_data_set_with_options(
        &f.dataset,
        &uid,
        WriteDataSetOptions {
            sequence_length: seq_len as u8,
            group_length: group_len as u8,
            deflate_level: level_opt(level),
            padding,
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(render(&out))
}
pub fn validate(ts: String, level: i64) -> Result<(), String> {
    writer::validate_serialization_options(&ts, level_opt(level), &PaddingOptions::default())
        .map_err(|e| e.to_string())
}
pub fn write_padded(
    data: Vec<i64>,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
    policy: i64,
    alignment: i64,
    item_alignment: i64,
    max_added: i64,
) -> Result<String, String> {
    write_file_result(
        object::read_file(&bytes(data)),
        &ts,
        level,
        seq_len,
        group_len,
        padding(policy, alignment, item_alignment, max_added),
    )
}
pub fn write_data_set_padded(
    data: Vec<i64>,
    ts: String,
    level: i64,
    seq_len: i64,
    group_len: i64,
    policy: i64,
    alignment: i64,
    item_alignment: i64,
    max_added: i64,
) -> Result<String, String> {
    write_data_set_with(
        data,
        ts,
        level,
        seq_len,
        group_len,
        padding(policy, alignment, item_alignment, max_added),
    )
}
pub fn validate_padding(
    ts: String,
    level: i64,
    policy: i64,
    alignment: i64,
    item_alignment: i64,
    max_added: i64,
) -> Result<(), String> {
    writer::validate_serialization_options(
        &ts,
        level_opt(level),
        &padding(policy, alignment, item_alignment, max_added),
    )
    .map_err(|e| e.to_string())
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Deflate" => {
            let out = deflate(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DeflateBytes" => {
            let out = deflate_bytes(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "WriteFile" => {
            let out = write_file(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
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
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "WriteDataSet" => {
            let out = write_data_set(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Validate" => {
            let () = validate(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::Value::Null)
        }
        "WritePadded" => {
            let out = write_padded(
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
        "WriteDataSetPadded" => {
            let out = write_data_set_padded(
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
        "ValidatePadding" => {
            let () = validate_padding(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
            )?;
            Ok(serde_json::Value::Null)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
