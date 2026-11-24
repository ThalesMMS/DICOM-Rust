#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::jpegls::{
    self, Codec, DecodeOptions, DecodedFrames, Encoder, EncoderOptions, Metadata, Region,
};
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
fn codec(near: bool) -> Codec {
    if near {
        Codec::near_lossless()
    } else {
        Codec::lossless()
    }
}
fn frames_json(frames: DecodedFrames) -> serde_json::Value {
    let diagnostics = if frames.diagnostics.is_empty() {
        serde_json::Value::Null
    } else {
        json!(frames.diagnostics)
    };
    json!({
        "data": ints(frames.data.first().map(Vec::as_slice).unwrap_or(&[])),
        "rows": frames.rows,
        "columns": frames.columns,
        "photometric": frames.photometric_interpretation,
        "diagnostics": diagnostics,
    })
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
    near: bool,
    jai: bool,
    hp: bool,
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
    let options = DecodeOptions {
        jai_thresholds: jai,
        hp_color_transforms: hp,
    };
    let codestream = bytes(&codestream);
    let frames = codec(near)
        .decode_frames(&[&codestream], &md, options)
        .map_err(|e| e.to_string())?;
    Ok(frames_json(frames))
}
pub fn decode_region(
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
    near: bool,
    jai: bool,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
) -> Result<serde_json::Value, String> {
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
    let options = DecodeOptions {
        jai_thresholds: jai,
        hp_color_transforms: false,
    };
    let frames = codec(near)
        .decode_frame_region(
            &bytes(&codestream),
            &md,
            Region::new(x0, y0, x1, y1),
            options,
        )
        .map_err(|e| e.to_string())?;
    Ok(frames_json(frames))
}
fn encoder(near: i64, allow_lossy: bool, interleave: i64) -> Result<Encoder, String> {
    if near == 0 && !allow_lossy && interleave == 0 {
        return Ok(Encoder::new());
    }
    Encoder::with_options(EncoderOptions {
        near,
        allow_lossy,
        interleave,
    })
    .map_err(|e| e.to_string())
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
    near: i64,
    allow_lossy: bool,
    interleave: i64,
) -> Result<Vec<i64>, String> {
    let encoder = encoder(near, allow_lossy, interleave)?;
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
pub fn encoder_capabilities(
    near: i64,
    allow_lossy: bool,
    interleave: i64,
) -> Result<serde_json::Value, String> {
    let c = encoder(near, allow_lossy, interleave)?.capabilities();
    Ok(json!({
        "transfer_syntax_uid": c.transfer_syntax_uid,
        "bits_allocated": c.bits_allocated,
        "pixel_representations": c.pixel_representations,
        "samples_per_pixel": c.samples_per_pixel,
        "photometric_interpretations": c.photometric_interpretations,
        "lossless": c.lossless,
        "lossy_method": c.lossy_method,
        "supports_multi_frame": c.supports_multi_frame,
        "backend": c.backend,
    }))
}
pub fn inspect(data: Vec<i64>, max_bytes: i64, hp: bool) -> Result<serde_json::Value, String> {
    let options = DecodeOptions {
        jai_thresholds: false,
        hp_color_transforms: hp,
    };
    let info =
        jpegls::inspect_codestream(&bytes(&data), max_bytes, options).map_err(|e| e.to_string())?;
    Ok(json!({
        "rows": info.rows,
        "columns": info.columns,
        "components": info.components,
        "precision": info.precision,
        "near": info.near,
        "interleave": info.interleave,
        "max_value": info.max_value,
        "hp_color_transform": info.hp_color_transform,
    }))
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
        "DecodeRegion" => {
            let out = decode_region(
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
                arg(args, 12)?,
                arg(args, 13)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncoderCapabilities" => {
            let out = encoder_capabilities(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Inspect" => {
            let out = inspect(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
