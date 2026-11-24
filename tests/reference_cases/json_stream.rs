#[allow(unused_imports)]
use super::support::*;
use dicom_rust::dicomjson;
use dicom_rust::dicomjson::encode_stream::{self, Produced};
use dicom_rust::object::{
    self, DataSet, Element, ElementHeader, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use dicom_rust::textfmt::quote;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
const F_PRETTY: i64 = 1;
const F_OMIT_GL: i64 = 2;
const F_LE: i64 = 4;
const F_BE: i64 = 8;
const F_SOURCE_BO: i64 = 16;
const F_PIXEL_STREAM: i64 = 32;
const F_PIXEL_DECLINE: i64 = 256;
fn reference_lossy(b: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        match std::str::from_utf8(&b[i..]) {
            Ok(s) => {
                out.push_str(s);
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&b[i..i + valid]).unwrap_or_default());
                out.push('\u{FFFD}');
                i += valid + 1;
            }
        }
    }
    out
}
fn render(out: &[u8]) -> String {
    if out.len() <= 12288 {
        return reference_lossy(out);
    }
    let sum = Sha256::digest(out);
    format!(
        "len={} sha256={}\n{}",
        out.len(),
        hex::encode(sum),
        reference_lossy(&out[..2048])
    )
}
fn digest(data: &[u8]) -> String {
    let sum = Sha256::digest(data);
    format!("{}/{}", data.len(), hex::encode(&sum[..4]))
}
fn parse_tag(s: &str) -> Result<Tag, String> {
    let bad = || format!("dicomjsonx: bad tag {}", quote(s));
    if s.len() != 8 {
        return Err(bad());
    }
    let v = u32::from_str_radix(s, 16).map_err(|_| bad())?;
    Ok(Tag::new((v >> 16) as u16, v as u16))
}
fn vr(s: &str) -> Result<Vr, String> {
    Vr::parse(s.as_bytes()).ok_or_else(|| format!("dicomjsonx: bad VR {}", quote(s)))
}
fn header(tag: Tag, vr: Vr, length: u32, set: bool) -> ElementHeader {
    ElementHeader {
        tag,
        vr,
        length,
        length_set: set,
    }
}
fn apply(o: &mut Object, ops: &str) -> Result<(), String> {
    for op in ops.split(';') {
        let fields: Vec<&str> = op.trim().splitn(4, ' ').collect();
        if fields.len() < 2 {
            continue;
        }
        let tag = parse_tag(fields[1])?;
        let arg = fields.get(3).copied();
        match fields[0] {
            "R" => {
                o.remove(tag);
            }
            "P" => {
                let raw = match arg {
                    Some(h) => hex::decode(h).map_err(|e| e.to_string())?,
                    None => Vec::new(),
                };
                o.put(Element {
                    header: header(tag, vr(fields[2])?, raw.len() as u32, true),
                    value: Value::Raw(raw),
                });
            }
            "S" => {
                let values = arg
                    .map(|v| v.split('|').map(str::to_string).collect())
                    .unwrap_or_default();
                o.put(Element {
                    header: header(tag, vr(fields[2])?, 0, false),
                    value: Value::Strings(values),
                });
            }
            "Q" | "N" => {
                let items = if fields[0] == "N" {
                    vec![DataSet::default()]
                } else {
                    Vec::new()
                };
                o.put(Element {
                    header: header(tag, Vr::SQ, UNDEFINED_LENGTH, true),
                    value: Value::Sequence(items),
                });
            }
            "F" => {
                let mut offset_table = Vec::new();
                let mut fragments = Vec::new();
                for (i, p) in arg.unwrap_or("").split('|').enumerate() {
                    let b = hex::decode(p).map_err(|e| e.to_string())?;
                    if i == 0 {
                        offset_table = b;
                    } else {
                        fragments.push(b);
                    }
                }
                o.put(Element {
                    header: header(tag, vr(fields[2])?, UNDEFINED_LENGTH, true),
                    value: Value::Fragments {
                        offset_table,
                        fragments,
                    },
                });
            }
            "B" => o.put(Element {
                header: header(tag, vr(fields[2])?, 0, false),
                value: Value::BulkData(arg.unwrap_or("").to_string()),
            }),
            _ => {}
        }
    }
    Ok(())
}
fn ints(s: &str, n: usize) -> Vec<i64> {
    let mut out = vec![0; n];
    if s.is_empty() {
        return out;
    }
    for (i, p) in s.split(',').enumerate() {
        if i < n {
            out[i] = p.parse().unwrap_or(0);
        }
    }
    out
}
fn order(flags: i64, le: i64, be: i64) -> Option<bool> {
    if flags & be != 0 {
        Some(true)
    } else if flags & le != 0 {
        Some(false)
    } else {
        None
    }
}
fn open(path: &str, data: Option<Vec<i64>>) -> Result<File, String> {
    let result = match data {
        Some(d) => object::read_file(&d.into_iter().map(|b| b as u8).collect::<Vec<u8>>()),
        None if path.is_empty() => object::read_file(&[]),
        None => object::open_file(path),
    };
    result.map_err(|(_, e)| e.to_string())
}
fn bulk_func<'a>(
    bulk_min: i64,
    log: &'a RefCell<Vec<String>>,
) -> Option<Box<dicomjson::BulkDataUriFunc<'a>>> {
    if bulk_min < 0 {
        return None;
    }
    Some(Box::new(move |tag: Tag, vr: Vr, data: &[u8]| {
        let hex_tag = format!("{:04X}{:04X}", tag.group, tag.element);
        log.borrow_mut()
            .push(format!("bulk {hex_tag} {vr} {}", digest(data)));
        if data.len() as i64 >= bulk_min {
            format!("bulk:{hex_tag}/{vr}/{}", digest(data))
        } else {
            String::new()
        }
    }))
}
fn json_limits(limits: &str) -> dicomjson::Limits {
    let l = ints(limits, 6);
    dicomjson::Limits {
        max_json_bytes: l[0],
        max_inline_binary_bytes: l[1],
        max_total_binary_bytes: l[2],
        max_sequence_depth: l[3],
        max_elements: l[4],
        max_sequence_items: l[5],
    }
}
fn encode_options<'a>(
    flags: i64,
    bulk_min: i64,
    limits: &str,
    log: &'a RefCell<Vec<String>>,
) -> dicomjson::Options<'a> {
    let mut opts = dicomjson::Options {
        pretty: flags & F_PRETTY != 0,
        omit_group_length: flags & F_OMIT_GL != 0,
        byte_order: order(flags, F_LE, F_BE),
        source_byte_order_inline_binary: flags & F_SOURCE_BO != 0,
        bulk_data_uri_func: bulk_func(bulk_min, log),
        limits: json_limits(limits),
        ..dicomjson::Options::default()
    };
    if flags & F_PIXEL_STREAM != 0 {
        let decline = flags & F_PIXEL_DECLINE != 0;
        opts.pixel_data_bulk_data_uri_func = Some(Box::new(move |tag: Tag, vr: Vr, b: &[u8]| {
            log.borrow_mut().push(format!(
                "pixel {:04X}{:04X} {vr} {}",
                tag.group,
                tag.element,
                digest(b)
            ));
            Ok(if decline {
                String::new()
            } else {
                format!("pixel:{}", digest(b))
            })
        }));
    }
    opts
}
pub fn json_encode(
    data: Vec<i64>,
    ops: String,
    flags: i64,
    bulk_min: i64,
    limits: String,
) -> Result<String, String> {
    let mut f = open("", Some(data))?;
    apply(&mut f.dataset, &ops)?;
    let log = RefCell::new(Vec::new());
    let mut opts = encode_options(flags, bulk_min, &limits, &log);
    let obj = (flags & 512 == 0).then_some(&f.dataset);
    let out = encode_stream::encode_context(obj, &mut opts).map_err(|e| e.to_string())?;
    drop(opts);
    Ok(render(&out) + "\n" + &log.into_inner().join("\n"))
}
#[allow(clippy::too_many_arguments)]
pub fn json_encode_array(
    data: Vec<i64>,
    ops: String,
    flags: i64,
    bulk_min: i64,
    limits: String,
    count: i64,
    nil_at: i64,
    fail_at: i64,
) -> Result<String, String> {
    let mut f = open("", Some(data))?;
    apply(&mut f.dataset, &ops)?;
    let log = RefCell::new(Vec::new());
    let mut opts = encode_options(flags, bulk_min, &limits, &log);
    let mut i = 0i64;
    let dataset = f.dataset.clone();
    let mut producer = || {
        let n = i;
        i += 1;
        if n == fail_at {
            Produced::Err("harness: producer failed".into())
        } else if n >= count {
            Produced::Eof
        } else if n == nil_at {
            Produced::Object(None)
        } else {
            Produced::Object(Some(Box::new(dataset.clone())))
        }
    };
    let next: Option<&mut dyn FnMut() -> Produced> = if count >= 0 {
        Some(&mut producer)
    } else {
        None
    };
    let out = encode_stream::encode_array_context(next, &mut opts).map_err(|e| e.to_string())?;
    drop(opts);
    Ok(render(&out) + "\n" + &log.into_inner().join("\n"))
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "JSONEncode" => {
            let out = json_encode(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "JSONEncodeArray" => {
            let out = json_encode_array(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                arg(args, 4)?,
                arg(args, 5)?,
                arg(args, 6)?,
                arg(args, 7)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
