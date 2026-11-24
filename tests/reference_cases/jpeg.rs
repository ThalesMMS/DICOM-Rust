#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::jpeg::{
    self, ColorConversion, DecodeOptions, EncodedFrame, Encoder, EncoderCapabilities,
    EncoderOptions, ExtendedEncoder, ExtendedEncoderOptions, Metadata, Subsampling, Syntax,
};
use serde_json::{Value, json};
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
    syntax: String,
    color_policy: i64,
    scan_quirks: bool,
) -> Result<Value, String> {
    let syntax = if syntax.is_empty() {
        Syntax::Auto
    } else {
        Syntax::from_uid(&syntax)
            .ok_or("dicom: no pixel data codec registered for transfer syntax")?
    };
    let color = match color_policy {
        1 => ColorConversion::Photometric,
        2 => ColorConversion::Always,
        3 => ColorConversion::Never,
        _ => ColorConversion::Guess,
    };
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
        syntax,
        color,
        sequential_scan_quirks: scan_quirks,
    };
    let frames = jpeg::decode_frames(&[&codestream], &md, options).map_err(|e| e.to_string())?;
    let diagnostics = if frames.diagnostics.is_empty() {
        Value::Null
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
fn encode_result(frame: EncodedFrame) -> Value {
    json!({
        "data": ints(&frame.data),
        "photometric": frame.photometric_interpretation,
        "planar": frame.planar_configuration.map_or(-1, |p| p as i64),
        "profile": frame.compatibility_profile,
    })
}
pub fn encode_baseline(
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
    quality: i64,
    subsampling: i64,
    optimize_huffman: bool,
) -> Result<Value, String> {
    let encoder = Encoder::with_options(EncoderOptions {
        quality,
        subsampling: Subsampling(subsampling as u8),
        optimize_huffman,
    })
    .map_err(|e| e.to_string())?;
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
    let frame = encoder
        .encode_frame(&bytes(&pixels), &md)
        .map_err(|e| e.to_string())?;
    Ok(encode_result(frame))
}
pub fn encode_extended(
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
    quality: i64,
    allow_color: bool,
    allow_legacy: bool,
    max_working_bytes: i64,
    max_encoded_frame_bytes: i64,
) -> Result<Value, String> {
    let encoder = ExtendedEncoder::with_options(ExtendedEncoderOptions {
        quality,
        allow_color_compatibility: allow_color,
        allow_legacy_reduced_precision: allow_legacy,
        max_working_bytes,
        max_encoded_frame_bytes,
    })
    .map_err(|e| e.to_string())?;
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
    let frame = encoder
        .encode_frame(&bytes(&pixels), &md)
        .map_err(|e| e.to_string())?;
    Ok(encode_result(frame))
}
fn list<T: serde::Serialize>(values: &[T]) -> Value {
    if values.is_empty() {
        Value::Null
    } else {
        json!(values)
    }
}
fn flatten(c: &EncoderCapabilities) -> Value {
    json!({
        "transferSyntaxUID": c.transfer_syntax_uid,
        "bitsAllocated": list(&c.bits_allocated),
        "samplesPerPixel": list(&c.samples_per_pixel),
        "photometricInterpretations": list(&c.photometric_interpretations),
        "outputPhotometric": list(&c.output_photometric_interpretations),
        "backend": c.backend,
        "compatibilityProfiles": list(&c.compatibility_profiles),
    })
}
fn empty_capabilities() -> Value {
    json!({
        "transferSyntaxUID": "",
        "bitsAllocated": null,
        "samplesPerPixel": null,
        "photometricInterpretations": null,
        "outputPhotometric": null,
        "backend": "",
        "compatibilityProfiles": null,
    })
}
pub fn baseline_capabilities(
    quality: i64,
    subsampling: i64,
    optimize_huffman: bool,
) -> Result<Value, String> {
    Ok(Encoder::with_options(EncoderOptions {
        quality,
        subsampling: Subsampling(subsampling as u8),
        optimize_huffman,
    })
    .map_or_else(|_| empty_capabilities(), |e| flatten(&e.capabilities())))
}
pub fn extended_capabilities(allow_color: bool, allow_legacy: bool) -> Result<Value, String> {
    Ok(ExtendedEncoder::with_options(ExtendedEncoderOptions {
        quality: jpeg::DEFAULT_QUALITY,
        allow_color_compatibility: allow_color,
        allow_legacy_reduced_precision: allow_legacy,
        ..Default::default()
    })
    .map_or_else(|_| empty_capabilities(), |e| flatten(&e.capabilities())))
}
pub fn transfer_syntax_ui_ds() -> Result<Vec<String>, String> {
    Ok(jpeg::transfer_syntax_uids()
        .into_iter()
        .map(str::to_string)
        .collect())
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
        "EncodeBaseline" => {
            let out = encode_baseline(
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
                arg(args, 13)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncodeExtended" => {
            let out = encode_extended(
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
                arg(args, 13)?,
                arg(args, 14)?,
                arg(args, 15)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "BaselineCapabilities" => {
            let out = baseline_capabilities(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ExtendedCapabilities" => {
            let out = extended_capabilities(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "TransferSyntaxUIDs" => {
            let out = transfer_syntax_ui_ds()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
