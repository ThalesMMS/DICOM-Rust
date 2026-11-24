use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
pub fn arg<T: serde::de::DeserializeOwned>(args: &[Value], i: usize) -> Result<T, String> {
    serde_json::from_value(args.get(i).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("argument {i}: {e}"))
}
#[allow(dead_code)]
pub fn num(args: &[Value], i: usize) -> Result<f64, String> {
    match args.get(i) {
        Some(Value::Object(o)) => match o.get("$num").and_then(Value::as_str) {
            Some("NaN") => Ok(f64::NAN),
            Some("Infinity") => Ok(f64::INFINITY),
            Some("-Infinity") => Ok(f64::NEG_INFINITY),
            Some("-0") => Ok(-0.0),
            _ => Err(format!("argument {i}: not a number")),
        },
        _ => arg(args, i),
    }
}
#[allow(dead_code)]
pub fn num_vec(args: &[Value], i: usize) -> Result<Vec<f64>, String> {
    match args.get(i) {
        Some(Value::Array(items)) => (0..items.len()).map(|j| num(items, j)).collect(),
        _ => arg(args, i),
    }
}
#[allow(dead_code)]
pub fn opt_num(args: &[Value], i: usize) -> Result<Option<f64>, String> {
    match args.get(i) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(args, i).map(Some),
    }
}
#[allow(dead_code)]
pub fn out_num(x: f64) -> Value {
    if x.is_nan() {
        json!({"$num": "NaN"})
    } else if x.is_infinite() {
        json!({"$num": if x > 0.0 { "Infinity" } else { "-Infinity" }})
    } else if x == 0.0 && x.is_sign_negative() {
        json!({"$num": "-0"})
    } else {
        json!(x)
    }
}
fn expand(v: Value) -> Value {
    match v {
        Value::Array(items) => Value::Array(items.into_iter().map(expand).collect()),
        Value::Object(o) => {
            if o.len() == 1
                && let Some(Value::String(h)) = o.get("$hex")
            {
                let bytes = hex::decode(h).expect("a $hex value is hex");
                return Value::Array(bytes.into_iter().map(Value::from).collect());
            }
            Value::Object(o.into_iter().map(|(k, v)| (k, expand(v))).collect())
        }
        other => other,
    }
}
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.chunks(4) {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = n << 6 | u32::from(value(c)?);
        }
        n <<= 6 * pad as u32;
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    Some(out)
}
fn byte_array(items: &[Value]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect()
}
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(s), Value::Array(items)) | (Value::Array(items), Value::String(s)) => {
            match (base64(s), byte_array(items)) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            }
        }
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => x == y,
            _ => match (x.as_u64(), y.as_u64()) {
                (Some(x), Some(y)) => x == y,
                _ => x.as_f64() == y.as_f64(),
            },
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}
fn shorten(v: &str) -> String {
    if v.chars().count() <= 600 {
        return v.to_string();
    }
    let head: String = v.chars().take(600).collect();
    format!("{head}...")
}
pub fn run(area: &str, call: fn(&str, &[Value]) -> Result<Value, String>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("reference_cases")
        .join(format!("{area}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (mut checked, mut failed) = (0, Vec::new());
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let case: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("{}:{}: {e}", path.display(), n + 1));
        let name = case["fn"].as_str().expect("fn").to_string();
        let args = match expand(case["args"].clone()) {
            Value::Array(a) => a,
            other => panic!("{}:{}: args {other}", path.display(), n + 1),
        };
        let got = catch_unwind(AssertUnwindSafe(|| call(&name, &args)))
            .unwrap_or_else(|_| Err("panic".into()));
        let ok = match (case.get("ok"), case.get("error"), &got) {
            (Some(want), _, Ok(got)) => same(&expand(want.clone()), got),
            (None, Some(Value::String(want)), Err(got)) => want == got,
            _ => false,
        };
        checked += 1;
        if !ok {
            let want = case
                .get("ok")
                .map(|v| format!("ok {v}"))
                .or_else(|| case.get("error").map(|v| format!("error {v}")))
                .unwrap_or_default();
            let got = match &got {
                Ok(v) => format!("ok {v}"),
                Err(e) => format!("error {e:?}"),
            };
            failed.push(format!(
                "line {} {name}:\n  want {}\n  got  {}",
                n + 1,
                shorten(&want),
                shorten(&got)
            ));
        }
    }
    assert!(checked > 0, "{}: no cases", path.display());
    assert!(
        failed.is_empty(),
        "{area}: {} of {checked} cases differ\n{}",
        failed.len(),
        failed.join("\n")
    );
}
