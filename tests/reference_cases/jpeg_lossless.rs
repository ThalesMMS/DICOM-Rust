#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::jpeglossless::{self, DecodeOptions, Encoder, Metadata};
use serde_json::json;
fn bytes(values: &[i64]) -> Vec<u8> {
    values.iter().map(|&v| v as u8).collect()
}
fn ints(values: &[u8]) -> Vec<i64> {
    values.iter().map(|&v| v as i64).collect()
}
fn metadata(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    planar: i64,
    number_of_frames: i64,
) -> Metadata {
    Metadata {
        rows: rows as u16,
        columns: columns as u16,
        samples_per_pixel: samples as u16,
        bits_allocated: bits_allocated as u16,
        bits_stored: bits_stored as u16,
        high_bit: high_bit as u16,
        pixel_representation: pixel_representation as u16,
        planar_configuration: if planar >= 0 { planar as u16 } else { 0 },
        planar_configuration_present: planar >= 0,
        number_of_frames,
        photometric_interpretation: photometric,
    }
}
pub fn decode(
    codestream: Vec<i64>,
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    planar: i64,
    sv1: bool,
    quirks: bool,
    predictor6: bool,
) -> Result<serde_json::Value, String> {
    let photometric = if photometric.is_empty() {
        "MONOCHROME2".to_string()
    } else {
        photometric
    };
    let photometric = photometric.trim_matches([' ', '\0']).to_string();
    if photometric.is_empty() {
        return Err(
            "dicom: missing required pixel data metadata: PhotometricInterpretation ((0028,0004))"
                .into(),
        );
    }
    let md = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        planar,
        1,
    );
    let codestream = bytes(&codestream);
    let options = DecodeOptions {
        selection_value: if sv1 { 1 } else { 0 },
        encoder_quirks: quirks,
        predictor6_overflow: predictor6,
    };
    let frames =
        jpeglossless::decode_frames(&[&codestream], &md, options).map_err(|e| e.to_string())?;
    let diagnostics = if frames.diagnostics.is_empty() {
        serde_json::Value::Null
    } else {
        json!(frames.diagnostics)
    };
    Ok(json!({
        "data": ints(&frames.data[0]),
        "rows": frames.rows,
        "columns": frames.columns,
        "photometric": frames.photometric_interpretation,
        "diagnostics": diagnostics,
    }))
}
fn encoder(selection_value: i64) -> Result<Encoder, String> {
    if selection_value == 0 {
        Ok(Encoder::sv1())
    } else {
        Encoder::process14(selection_value).map_err(|e| e.to_string())
    }
}
pub fn encode(
    pixels: Vec<i64>,
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    planar: i64,
    number_of_frames: i64,
    selection_value: i64,
) -> Result<Vec<i64>, String> {
    let encoder = encoder(selection_value)?;
    let md = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        planar,
        number_of_frames,
    );
    let out = encoder
        .encode_frame(&bytes(&pixels), &md)
        .map_err(|e| e.to_string())?;
    Ok(ints(&out))
}
pub fn encoder_transfer_syntax(selection_value: i64) -> Result<String, String> {
    Ok(encoder(selection_value)
        .map(|e| e.transfer_syntax().to_string())
        .unwrap_or_default())
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Decode" => {
            let out = decode(
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
                arg(args, 12)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Encode" => {
            let out = encode(
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
        "EncoderTransferSyntax" => {
            let out = encoder_transfer_syntax(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
