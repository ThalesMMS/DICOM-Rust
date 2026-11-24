#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::jpeg2000::Metadata;
use dicom_rust::codec::jpeg2000::encoder::{Encoder, EncoderCapabilities, Options, Part2Encoder};
use dicom_rust::codec::jpeg2000::j2kcore::Rect;
use dicom_rust::codec::jpeg2000::{
    self, CoreOptions, PartialRequest, Registration, SignednessPolicy,
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
fn extracted(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    planar: i64,
    frames: i64,
) -> Result<Metadata, String> {
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
    if frames <= 0 || frames > i32::MAX as i64 {
        return Err("dicom: invalid pixel data metadata: invalid NumberOfFrames".into());
    }
    Ok(metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        planar,
        frames,
    ))
}
fn core_options(signedness: i64, allow_jp2: bool) -> CoreOptions {
    CoreOptions {
        signedness_mismatch: if signedness == 1 {
            SignednessPolicy::ReinterpretUnsignedAsSigned
        } else {
            SignednessPolicy::Reject
        },
        allow_jp2_wrapper: allow_jp2,
        ..CoreOptions::default()
    }
}
fn frames_result(frames: &dicom_rust::codec::jpeg2000::DecodedFrames) -> Value {
    let data: Vec<u8> = frames.data.concat();
    json!({
        "data": ints(&data),
        "rows": frames.rows,
        "columns": frames.columns,
        "frames": frames.data.len(),
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
    syntax: String,
    registration: i64,
    signedness: i64,
    allow_jp2: bool,
    copies: i64,
    split: i64,
) -> Result<Value, String> {
    let copies = if (1..=8).contains(&copies) { copies } else { 1 };
    let codestream = bytes(&codestream);
    let md = extracted(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        planar,
        copies,
    );
    let mut fragments: Vec<&[u8]> = Vec::new();
    for _ in 0..copies {
        if split > 0 && (split as usize) < codestream.len() {
            fragments.push(&codestream[..split as usize]);
            fragments.push(&codestream[split as usize..]);
        } else {
            fragments.push(&codestream);
        }
    }
    let registration = match registration {
        1 => Registration::Core,
        2 => Registration::CoreHtj2k,
        _ => Registration::Default,
    };
    let options = if registration == Registration::Default {
        CoreOptions::default()
    } else {
        core_options(signedness, allow_jp2)
    };
    let known = match registration {
        Registration::Default => jpeg2000::transfer_syntax_uids(),
        Registration::Core => vec![jpeg2000::UID_LOSSLESS, jpeg2000::UID_LOSSY],
        Registration::CoreHtj2k => vec![
            jpeg2000::UID_HTJ2K_LOSSLESS,
            jpeg2000::UID_HTJ2K_LOSSLESS_RPCL,
            jpeg2000::UID_HTJ2K,
        ],
    };
    if !known.contains(&syntax.trim_end_matches([' ', '\0'])) {
        return Err("dicom: no pixel data codec registered for transfer syntax".into());
    }
    let md = md?;
    let frames = jpeg2000::decode_frames(&fragments, &md, &syntax, registration, options)
        .map_err(|e| e.to_string())?;
    Ok(frames_result(&frames))
}
pub fn decode_partial(
    codestream: Vec<i64>,
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    reduce: i64,
    layers: i64,
    signedness: i64,
) -> Result<Value, String> {
    let photometric = if photometric.is_empty() {
        "MONOCHROME2".to_string()
    } else {
        photometric
    };
    let md = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        -1,
        1,
    );
    let region = if x0 == 0 && y0 == 0 && x1 == 0 && y1 == 0 {
        Rect::default()
    } else {
        Rect::new(x0, y0, x1, y1)
    };
    let request = PartialRequest {
        region,
        reduction_level: reduce,
        quality_layers: layers,
    };
    let frames = jpeg2000::decode_partial(
        &bytes(&codestream),
        &md,
        request,
        core_options(signedness, false),
    )
    .map_err(|e| e.to_string())?;
    Ok(frames_result(&frames))
}
pub fn partial_capabilities(
    codestream: Vec<i64>,
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
) -> Result<Value, String> {
    let photometric = if photometric.is_empty() {
        "MONOCHROME2".to_string()
    } else {
        photometric
    };
    let md = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        -1,
        1,
    );
    let caps =
        jpeg2000::partial_capabilities(&bytes(&codestream), &md).map_err(|e| e.to_string())?;
    Ok(json!({
        "region": caps.region,
        "reductionLevels": caps.reduction_levels,
        "qualityLayers": caps.quality_layers,
    }))
}
pub fn decode_reduced(codestream: Vec<i64>, reduce: i64, allow_jp2: bool) -> Result<Value, String> {
    let img =
        jpeg2000::decode_core_reduced(&bytes(&codestream), reduce, core_options(0, allow_jp2))
            .map_err(|e| e.to_string())?;
    let samples: Vec<i32> = img.components.concat();
    Ok(json!({
        "columns": img.columns,
        "rows": img.rows,
        "samples": samples,
        "depths": img.depths,
        "signed": img.signed,
    }))
}
pub fn decode_part2(
    codestream: Vec<i64>,
    rows: i64,
    columns: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    frames: i64,
    syntax: String,
) -> Result<Value, String> {
    let uid = syntax.trim_end_matches([' ', '\0']);
    if uid != jpeg2000::UID_PART2_LOSSLESS && uid != jpeg2000::UID_PART2 {
        return Err("dicom: no pixel data codec registered for transfer syntax".into());
    }
    let md = extracted(
        rows,
        columns,
        1,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        -1,
        frames,
    )?;
    let codestream = bytes(&codestream);
    let out = jpeg2000::decode_part2(&[&codestream], &[], false, &md, &syntax)
        .map_err(|e| e.to_string())?;
    Ok(frames_result(&out))
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
    syntax: String,
    color_transform: bool,
    levels: i64,
    step_size: f64,
    allow_lossy: bool,
) -> Result<Value, String> {
    let encoder = Encoder::new(&Options {
        transfer_syntax_uid: syntax,
        color_transform,
        levels,
        step_size,
        allow_lossy,
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
    Ok(json!({
        "data": ints(&frame.data),
        "photometric": frame.photometric_interpretation,
        "planar": frame.planar_configuration.map_or(-1, i64::from),
    }))
}
fn flatten(c: EncoderCapabilities, error: &str) -> Value {
    fn list<T: serde::Serialize>(v: Vec<T>) -> Value {
        if v.is_empty() { Value::Null } else { json!(v) }
    }
    json!({
        "transferSyntaxUID": c.transfer_syntax_uid,
        "bitsAllocated": list(c.bits_allocated),
        "pixelRepresentations": list(c.pixel_representations),
        "samplesPerPixel": list(c.samples_per_pixel),
        "photometricInterpretations": list(c.photometric_interpretations),
        "outputPhotometric": list(c.output_photometric_interpretations),
        "outputPlanar": list(c.output_planar_configurations),
        "lossless": c.lossless,
        "lossyMethod": c.lossy_method,
        "backend": c.backend,
        "error": error,
    })
}
pub fn encoder_caps(
    syntax: String,
    part2: bool,
    color_transform: bool,
    levels: i64,
    step_size: f64,
    allow_lossy: bool,
) -> Result<Value, String> {
    let opts = Options {
        transfer_syntax_uid: syntax,
        color_transform,
        levels,
        step_size,
        allow_lossy,
    };
    let caps = if part2 {
        Part2Encoder::new(&opts).map(|e| e.capabilities())
    } else {
        Encoder::new(&opts).map(|e| e.capabilities())
    };
    Ok(match caps {
        Ok(c) => flatten(c, ""),
        Err(e) => flatten(EncoderCapabilities::default(), &e.to_string()),
    })
}
pub fn encode_part2(
    pixels: Vec<i64>,
    rows: i64,
    columns: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_representation: i64,
    photometric: String,
    planar: i64,
    number_of_frames: i64,
    count: i64,
    syntax: String,
    levels: i64,
    step_size: f64,
    allow_lossy: bool,
) -> Result<Vec<i64>, String> {
    let encoder = Part2Encoder::new(&Options {
        transfer_syntax_uid: syntax,
        color_transform: false,
        levels,
        step_size,
        allow_lossy,
    })
    .map_err(|e| e.to_string())?;
    let pixels = bytes(&pixels);
    let mut frames: Vec<&[u8]> = Vec::new();
    if count > 0 && count <= 64 {
        let count = count as usize;
        let size = pixels.len() / count;
        for i in 0..count {
            let end = if i == count - 1 {
                pixels.len()
            } else {
                (i + 1) * size
            };
            frames.push(&pixels[i * size..end]);
        }
    }
    let md = metadata(
        rows,
        columns,
        1,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_representation,
        photometric,
        planar,
        number_of_frames,
    );
    let data = encoder
        .encode_frame_collection(&frames, &md)
        .map_err(|e| e.to_string())?;
    Ok(ints(&data))
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
                arg(args, 13)?,
                arg(args, 14)?,
                arg(args, 15)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DecodePartial" => {
            let out = decode_partial(
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
        "PartialCapabilities" => {
            let out = partial_capabilities(
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
        "DecodeReduced" => {
            let out = decode_reduced(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DecodePart2" => {
            let out = decode_part2(
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
                num(args, 14)?,
                arg(args, 15)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncoderCaps" => {
            let out = encoder_caps(
                arg(args, 0)?,
                arg(args, 1)?,
                arg(args, 2)?,
                arg(args, 3)?,
                num(args, 4)?,
                arg(args, 5)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncodePart2" => {
            let out = encode_part2(
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
                num(args, 13)?,
                arg(args, 14)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
