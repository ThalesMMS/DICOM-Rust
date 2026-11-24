use crate::support::{self, Outcome, Report};
use dicom_rust::codec::jpegxl;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;
const PROFILE: &str = "outside the decoder's profile (one frame; modular without delta     palette and without restoration filters, or JPEG-compatible VarDCT, or 8-bit sRGB XYB with     DCT8 only; grey or RGB, extra channels dropped): refused";
const JPEG_RENDER: &str = "a recompressed JPEG is returned as 8-bit integers, as a JPEG decoder     renders it (integer inverse DCT, repeated chroma samples, rounding): above the level 10     bound, where rounding alone is 0.0011 RMS against 0.00001; the level 5 case of the same     file is within its bound and the reconstructed JPEG is exact";
const ORIENTATION: &str = "a recompressed JPEG is returned on the JPEG's own grid: the     orientation of the JPEG XL image header is not applied (500x606 for 606x500)";
const DECODED: &[(&str, Option<&str>)] = &[
    ("alpha_nonpremultiplied", None),
    ("alpha_triangles", None),
    ("bench_oriented_brg", Some(ORIENTATION)),
    ("bench_oriented_brg_5", Some(ORIENTATION)),
    ("cafe", Some(JPEG_RENDER)),
    ("cafe_5", None),
    ("grayscale_jpeg", Some(JPEG_RENDER)),
    ("grayscale_jpeg_5", None),
    ("lz77_flower", None),
];
fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn field(json: &str, name: &str) -> Option<String> {
    let pattern = format!(r#""{}"\s*:\s*"?([^",\s}}]+)"?"#, regex::escape(name));
    Regex::new(&pattern)
        .ok()?
        .captures(json)
        .map(|c| c[1].to_string())
}
fn read_npy(data: &[u8]) -> Result<(Vec<usize>, Vec<f32>), String> {
    if data.len() < 10 || &data[..6] != b"\x93NUMPY" {
        return Err("not a NumPy file".into());
    }
    let (header_len, start) = if data[6] == 1 {
        (usize::from(u16::from_le_bytes([data[8], data[9]])), 10)
    } else {
        if data.len() < 12 {
            return Err("short NumPy header".into());
        }
        (
            u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize,
            12,
        )
    };
    let header = String::from_utf8_lossy(data.get(start..start + header_len).ok_or("short")?);
    if !header.contains("'<f4'") || header.contains("True") {
        return Err(format!("unsupported NumPy layout: {header}"));
    }
    let shape = Regex::new(r"'shape':\s*\(([^)]*)\)")
        .map_err(|e| e.to_string())?
        .captures(&header)
        .ok_or("no shape")?[1]
        .split(',')
        .filter_map(|n| n.trim().parse().ok())
        .collect::<Vec<usize>>();
    let values = data[start + header_len..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect::<Vec<f32>>();
    if values.len() != shape.iter().product::<usize>() {
        return Err("NumPy body does not match its shape".into());
    }
    Ok((shape, values))
}
fn pixels(image: &jpegxl::Image, objects: &Path, json: &str) -> Outcome {
    let Some(sha) = field(json, "reference_image.npy") else {
        return Outcome::Differs("the descriptor names no reference image".into());
    };
    let path = objects.join(&sha);
    if !path.is_file() {
        return Outcome::Differs(format!("reference image {sha} was not fetched"));
    }
    let data = support::read(&path);
    if sha256(&data) != sha {
        return Outcome::Differs("reference image does not match its SHA-256".into());
    }
    let (shape, want) = match read_npy(&data) {
        Ok(r) => r,
        Err(e) => return Outcome::Differs(e),
    };
    let got_shape = [1, image.height, image.width, image.channels];
    if shape.len() != 4 || shape[..3] != got_shape[..3] || shape[3] < image.channels {
        return Outcome::Differs(format!("shape {got_shape:?}, reference {shape:?}"));
    }
    let reference_channels = shape[3];
    let bound = |name: &str| field(json, name).and_then(|v| v.parse::<f64>().ok());
    let (Some(rms_bound), Some(peak_bound)) = (bound("rms_error"), bound("peak_error")) else {
        return Outcome::Differs("the descriptor has no error bounds".into());
    };
    let scale = ((1u64 << image.bits_per_sample) - 1) as f64;
    let clamped = field(json, "reconstructed.jpg").is_some();
    let mut peak = 0f64;
    let mut squares = vec![0f64; image.channels];
    for (i, g) in image.samples.iter().enumerate() {
        let (pixel, channel) = (i / image.channels, i % image.channels);
        let w = f64::from(want[pixel * reference_channels + channel]);
        let w = if clamped { w.clamp(0.0, 1.0) } else { w };
        let d = (f64::from(*g) / scale - w).abs();
        peak = peak.max(d);
        squares[channel] += d * d;
    }
    let pixels = (image.width * image.height).max(1) as f64;
    let rms = squares
        .iter()
        .map(|s| (s / pixels).sqrt())
        .fold(0f64, f64::max);
    let detail = format!("peak {peak:.6}/{peak_bound} rms {rms:.6}/{rms_bound}");
    if peak <= peak_bound && rms <= rms_bound {
        Outcome::Pass(detail)
    } else {
        Outcome::Differs(detail)
    }
}
#[test]
#[ignore = "needs the conformance cache: run scripts/ci.sh conformance"]
fn jpeg_xl_conformance() {
    let Some(dir) = support::set_dir("jxl-conformance") else {
        return;
    };
    let objects = dir.join("objects");
    let cases = dir.join("testcases");
    let mut names: Vec<String> = std::fs::read_dir(&cases)
        .expect("testcases")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("test.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut report = Report::new("jxl-conformance (JPEG XL)");
    for name in &names {
        let case = cases.join(name);
        let json = String::from_utf8_lossy(&support::read(&case.join("test.json"))).into_owned();
        let input = support::read(&case.join("input.jxl"));
        let decoded = DECODED.iter().find(|(n, _)| n == name);
        if let Some(sha) = field(&json, "reconstructed.jpg") {
            let outcome = match jpegxl::reconstruct_jpeg(&input) {
                Err(e) => Outcome::Refused(e.to_string()),
                Ok(jpeg) if sha256(&jpeg) == sha => {
                    Outcome::Pass(format!("{} bytes, SHA-256 as the reference", jpeg.len()))
                }
                Ok(jpeg) => Outcome::Differs(format!("{} bytes, another SHA-256", jpeg.len())),
            };
            report.case(&format!("{name}: reconstructed JPEG"), outcome, None);
        }
        let outcome = match jpegxl::decode(&input) {
            Err(e) => Outcome::Refused(e.to_string()),
            Ok(image) => pixels(&image, &objects, &json),
        };
        let reason = match decoded {
            Some((_, reason)) => *reason,
            None => Some(PROFILE),
        };
        report.case(name, outcome, reason);
    }
    report.finish();
}
