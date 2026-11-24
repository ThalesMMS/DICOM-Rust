#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::jpegxl::{self, Error, Image, LosslessOptions};
use serde::Deserialize;
use sha2::{Digest, Sha256};
#[derive(Deserialize, Default)]
#[serde(default)]
struct Script {
    op: String,
    hex: String,
    w: i64,
    h: i64,
    c: i64,
    bits: i64,
    opt: i64,
    pattern: String,
    seed: u64,
    samples: Option<Vec<i32>>,
    flips: Vec<u64>,
    cut: i64,
}
fn invalid_script() -> String {
    "harness: invalid script".to_string()
}
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn describe(err: &Error) -> String {
    format!(
        "err[invalid={},unsupported={},limit={},norecon={},notrev={}] {err}",
        err.is_invalid_codestream(),
        err.is_unsupported(),
        err.is_limit(),
        err.is_no_reconstruction_data(),
        err.is_not_reversible()
    )
}
fn sample_sum(s: &[i32]) -> String {
    let mut h = Sha256::new();
    for v in s {
        h.update(v.to_le_bytes());
    }
    let head: Vec<String> = s.iter().take(8).map(|v| v.to_string()).collect();
    format!("sha={} head=[{}]", hex(&h.finalize()), head.join(" "))
}
fn decode_summary(data: &[u8]) -> String {
    match jpegxl::decode(data) {
        Ok(img) => format!(
            "ok {}x{}x{} bits={} n={} {}",
            img.width,
            img.height,
            img.channels,
            img.bits_per_sample,
            img.samples.len(),
            sample_sum(&img.samples)
        ),
        Err(e) => describe(&e),
    }
}
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}
fn tri(v: i64, period: i64) -> i64 {
    let v = v % (2 * period);
    if v > period { 2 * period - v } else { v }
}
fn pattern(name: &str, seed: u64, w: usize, h: usize, c: usize, bits: u32) -> Vec<i32> {
    let maxv = (1i64 << bits) - 1;
    let mut out = vec![0i32; w * h * c];
    for y in 0..h {
        for x in 0..w {
            for k in 0..c {
                let i = (y * w + x) * c + k;
                let r = mix(seed ^ (i as u64).wrapping_mul(0x100_0000_01B3));
                let (xi, yi, ki) = (x as i64, y as i64, k as i64);
                let v = match name {
                    "noise" => (r % (maxv as u64 + 1)) as i64,
                    "flat" => (seed % (maxv as u64 + 1)) as i64,
                    "max" => maxv,
                    "checker" => {
                        if (xi / 3 + yi / 3 + ki) % 2 == 0 {
                            maxv
                        } else {
                            0
                        }
                    }
                    "smooth" => {
                        let base =
                            tri(xi * 7 + ki * 13, 61) * tri(yi * 5 + ki * 3, 47) * maxv / (61 * 47);
                        let v = base + (r % 9) as i64 - 4;
                        if (xi / 13 + yi / 11) % 5 == 0 {
                            maxv - v
                        } else {
                            v
                        }
                    }
                    _ => (xi * 3 + yi * 5 + ki * 11) * (maxv / 255 + 1) % (maxv + 1),
                };
                out[i] = v.clamp(0, maxv) as i32;
            }
        }
    }
    out
}
fn mutate(data: &[u8], flips: &[u64], cut: i64) -> Vec<u8> {
    let mut out = data.to_vec();
    if !out.is_empty() {
        for f in flips {
            let bit = f % (8 * out.len() as u64);
            out[(bit / 8) as usize] ^= 1 << (bit % 8);
        }
    }
    if cut > 0 && (cut as usize) < out.len() {
        out.truncate(cut as usize);
    }
    out
}
fn encode_summary(enc: &[u8]) -> String {
    format!(
        "len={} sha={} head={}",
        enc.len(),
        hex(&Sha256::digest(enc)),
        hex(&enc[..enc.len().min(32)])
    )
}
fn encode(s: &Script) -> Result<String, String> {
    if s.w < 0
        || s.h < 0
        || s.c < 0
        || s.bits < 0
        || s.opt < 0
        || (s.samples.is_none() && (s.w > 4096 || s.h > 4096 || s.w * s.h * s.c.max(1) > 1 << 22))
    {
        return Err(invalid_script());
    }
    let (w, h, c) = (s.w as usize, s.h as usize, s.c as usize);
    let samples = match &s.samples {
        Some(v) => v.clone(),
        None if w > 0 && h > 0 && c > 0 && (1..=31).contains(&s.bits) => {
            pattern(&s.pattern, s.seed, w, h, c, s.bits as u32)
        }
        None => Vec::new(),
    };
    let img = Image {
        width: w,
        height: h,
        channels: c,
        bits_per_sample: u32::try_from(s.bits).map_err(|_| invalid_script())?,
        samples,
    };
    let opts = LosslessOptions {
        bits_per_sample: u32::try_from(s.opt).map_err(|_| invalid_script())?,
    };
    let enc = match jpegxl::encode_lossless(&img, opts) {
        Ok(e) => e,
        Err(e) => return Ok(format!("encode {}", describe(&e))),
    };
    if s.op == "encmut" {
        return Ok(format!(
            "encode {} | mutated {}",
            encode_summary(&enc),
            decode_summary(&mutate(&enc, &s.flips, s.cut))
        ));
    }
    let got = decode_summary(&enc);
    let same = match jpegxl::decode(&enc) {
        Ok(back)
            if back.width == img.width
                && back.height == img.height
                && back.channels == img.channels
                && back.samples == img.samples =>
        {
            "exact"
        }
        _ => "differs",
    };
    Ok(format!(
        "encode {} | decode {got} | roundtrip {same}",
        encode_summary(&enc)
    ))
}
pub fn run(input: String) -> Result<String, String> {
    let s: Script = serde_json::from_str(&input).map_err(|_| invalid_script())?;
    match s.op.as_str() {
        "decode" => {
            let data = decode_hex(&s.hex).ok_or_else(invalid_script)?;
            Ok(decode_summary(&data))
        }
        "encode" | "encmut" => encode(&s),
        _ => Err(invalid_script()),
    }
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Run" => {
            let out = run(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
