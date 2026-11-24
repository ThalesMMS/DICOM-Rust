#[allow(unused_imports)]
use super::support::*;
use dicom_rust::object;
use dicom_rust::pixel::extract::render_frame;
use dicom_rust::pixel::frame::FrameImage;
use dicom_rust::pixel::pixeldata::PixelError;
use dicom_rust::pixel::render_single::render_single_frame;
use sha2::{Digest, Sha256};
const MAX_INPUT: usize = 4 << 20;
fn digest(img: &FrameImage) -> String {
    let (kind, width, height, pixels) = match img {
        FrameImage::Gray(g) => ("gray", g.width, g.height, &g.pixels),
        FrameImage::Rgba(r) => ("rgba", r.width, r.height, &r.pixels),
    };
    let sum = Sha256::digest(pixels);
    format!("{kind} {width}x{height} {}", hex::encode(&sum[..12]))
}
fn result(r: Result<FrameImage, PixelError>) -> String {
    match r {
        Ok(img) => digest(&img),
        Err(err) => format!("error {err}"),
    }
}
pub fn frame_image(data: Vec<i64>, first: i64, last: i64) -> Result<String, String> {
    if data.len() > MAX_INPUT || last.wrapping_sub(first) > 64 || last < first {
        return Err("harness: bad arguments".to_string());
    }
    let data: Vec<u8> = data.iter().map(|&v| v as u8).collect();
    let file = match object::read_file(&data) {
        Ok(file) => file,
        Err((_, err)) => return Ok(format!("read-error {err}\n")),
    };
    let mut out = String::new();
    for index in first..=last {
        out.push_str(&format!(
            "{index} single {} | whole {}\n",
            result(render_single_frame(&file, index)),
            result(render_frame(&file, index))
        ));
    }
    Ok(out)
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "FrameImage" => {
            let out = frame_image(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
