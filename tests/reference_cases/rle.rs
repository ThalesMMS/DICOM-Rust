#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::rle::{self, Frames, Metadata, Recovery, Rect};
use serde_json::{Value, json};
fn bytes(values: &[i64]) -> Vec<u8> {
    values.iter().map(|&v| v as u8).collect()
}
fn ints(values: &[u8]) -> Vec<i64> {
    values.iter().map(|&v| i64::from(v)).collect()
}
fn diagnostics(list: &[&str]) -> Value {
    if list.is_empty() {
        Value::Null
    } else {
        json!(list)
    }
}
fn metadata(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
) -> Metadata {
    Metadata {
        rows: rows as u16,
        columns: columns as u16,
        samples_per_pixel: samples as u16,
        bits_allocated: bits_allocated as u16,
        bits_stored: bits_stored as u16,
        high_bit: high_bit as u16,
        pixel_representation: pixel_rep as u16,
        planar_configuration: planar as u16,
        planar_configuration_present: planar_present,
        number_of_frames: frames,
        photometric_interpretation: photometric,
    }
}
fn frames_json(f: Frames) -> Value {
    let lengths: Vec<usize> = f.data.iter().map(Vec::len).collect();
    json!({
        "rows": f.rows,
        "columns": f.columns,
        "data": ints(&f.data.concat()),
        "frame_lengths": lengths,
        "diagnostics": diagnostics(&f.diagnostics),
    })
}
fn recovery(overrun: bool, reversed: bool) -> Recovery {
    Recovery {
        segment_overrun: overrun,
        reversed_byte_segments: reversed,
    }
}
fn too_large(rows: i64, columns: i64, overrun: bool) -> Result<(), String> {
    if overrun && i64::from(rows as u16) * i64::from(columns as u16) > 1 << 20 {
        return Err("harness: tolerant decode of a frame over 1 Mi pixels".into());
    }
    Ok(())
}
pub fn decode(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
    offset_table: Vec<i64>,
    fragments: Vec<i64>,
    fragment_lengths: Vec<i64>,
    overrun: bool,
    reversed: bool,
) -> Result<Value, String> {
    too_large(rows, columns, overrun)?;
    let photometric = photometric.trim_matches([' ', '\0']).to_string();
    if photometric.is_empty() {
        return Err(
            "dicom: missing required pixel data metadata: PhotometricInterpretation (0028,0004)"
                .into(),
        );
    }
    if frames <= 0 || frames > i64::from(i32::MAX) {
        return Err("dicom: invalid pixel data metadata: invalid NumberOfFrames".into());
    }
    let m = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_rep,
        planar,
        planar_present,
        frames,
        photometric,
    );
    let blob = bytes(&fragments);
    let mut pieces: Vec<&[u8]> = Vec::with_capacity(fragment_lengths.len());
    let mut at = 0usize;
    for &n in &fragment_lengths {
        let n = n.clamp(0, (blob.len() - at) as i64) as usize;
        pieces.push(&blob[at..at + n]);
        at += n;
    }
    let f = rle::decode(
        &pieces,
        &bytes(&offset_table),
        &m,
        recovery(overrun, reversed),
    )
    .map_err(|e| e.to_string())?;
    Ok(frames_json(f))
}
pub fn decode_region(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
    codestream: Vec<i64>,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    overrun: bool,
    reversed: bool,
) -> Result<Value, String> {
    too_large(rows, columns, overrun)?;
    let m = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_rep,
        planar,
        planar_present,
        frames,
        photometric,
    );
    let f = rle::decode_region(
        &bytes(&codestream),
        &m,
        Rect::new(x0, y0, x1, y1),
        recovery(overrun, reversed),
    )
    .map_err(|e| e.to_string())?;
    Ok(frames_json(f))
}
pub fn buffer_metadata(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    whole_frame: bool,
) -> Result<Value, String> {
    let m = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_rep,
        planar,
        planar_present,
        frames,
        photometric,
    );
    let out = rle::buffer_metadata(&m, Rect::new(x0, y0, x1, y1), whole_frame)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "rows": out.rows,
        "columns": out.columns,
        "samples_per_pixel": out.samples_per_pixel,
        "bits_allocated": out.bits_allocated,
        "bits_stored": out.bits_stored,
        "high_bit": out.high_bit,
        "pixel_representation": out.pixel_representation,
        "planar_configuration": out.planar_configuration,
        "planar_configuration_present": out.planar_configuration_present,
        "number_of_frames": out.number_of_frames,
        "photometric_interpretation": out.photometric_interpretation,
    }))
}
pub fn decode_frame_into(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
    codestream: Vec<i64>,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    whole_frame: bool,
    dst_len: i64,
    overrun: bool,
    reversed: bool,
) -> Result<Value, String> {
    let m = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_rep,
        planar,
        planar_present,
        frames,
        photometric,
    );
    let mut dst = vec![0xA5u8; dst_len.clamp(0, 1 << 22) as usize];
    let (n, diags, err) = match rle::decode_frame_into(
        &bytes(&codestream),
        &m,
        Rect::new(x0, y0, x1, y1),
        whole_frame,
        &mut dst,
        recovery(overrun, reversed),
    ) {
        Ok((n, diags)) => (n, diagnostics(&diags), String::new()),
        Err(e) => (e.written, Value::Null, e.error.to_string()),
    };
    Ok(json!({"n": n, "dst": ints(&dst), "diagnostics": diags, "err": err}))
}
pub fn encode_frame(
    rows: i64,
    columns: i64,
    samples: i64,
    bits_allocated: i64,
    bits_stored: i64,
    high_bit: i64,
    pixel_rep: i64,
    planar: i64,
    planar_present: bool,
    frames: i64,
    photometric: String,
    pixels: Vec<i64>,
) -> Result<Vec<i64>, String> {
    let m = metadata(
        rows,
        columns,
        samples,
        bits_allocated,
        bits_stored,
        high_bit,
        pixel_rep,
        planar,
        planar_present,
        frames,
        photometric,
    );
    let encoded = rle::encode_frame(&bytes(&pixels), &m).map_err(|e| e.to_string())?;
    Ok(ints(&encoded))
}
pub fn capabilities() -> Result<Value, String> {
    let c = rle::capabilities();
    Ok(json!({
        "transfer_syntax_uid": c.transfer_syntax_uid,
        "bits_allocated": c.bits_allocated,
        "pixel_representations": c.pixel_representations,
        "samples_per_pixel": c.samples_per_pixel,
        "photometric_interpretations": c.photometric_interpretations,
        "lossless": c.lossless,
        "supports_multi_frame": c.supports_multi_frame,
        "backend": c.backend,
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
                arg(args, 13)?,
                arg(args, 14)?,
                arg(args, 15)?,
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
                arg(args, 16)?,
                arg(args, 17)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "BufferMetadata" => {
            let out = buffer_metadata(
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
        "DecodeFrameInto" => {
            let out = decode_frame_into(
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
                arg(args, 16)?,
                arg(args, 17)?,
                arg(args, 18)?,
                arg(args, 19)?,
            )?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncodeFrame" => {
            let out = encode_frame(
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
        "Capabilities" => {
            let out = capabilities()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
