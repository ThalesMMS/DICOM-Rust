#[allow(unused_imports)]
use super::support::*;
use dicom_rust::codec::{jpeg, jpeg2000, jpeglossless, jpegls};
use dicom_rust::object::{self, File, Object, Tag, Vr};
use dicom_rust::pixel::transcode::{
    self as tc, DecoderRegistry, DecompressLimits, DecompressOptions, EncoderRegistry,
    FrameEncoder, PlanarOutput, TranscodeOptions, TranscodeReport, UidPolicy,
};
use sha2::{Digest, Sha256};
const HARNESS: &str = "harness: input rejected";
const FIXED_UID: &str = "1.2.826.0.1.3680043.10.9999";
const TAG_SOP_INSTANCE: Tag = Tag::new(0x0008, 0x0018);
const TAG_MEDIA_SOP: Tag = Tag::new(0x0002, 0x0003);
fn bytes(data: &[i64]) -> Vec<u8> {
    data.iter().map(|b| *b as u8).collect()
}
fn rejected() -> String {
    HARNESS.to_string()
}
fn parse_int(s: &str) -> Result<i64, String> {
    s.parse::<i64>().map_err(|_| rejected())
}
fn encoder(spec: &str) -> Result<FrameEncoder, String> {
    let parts: Vec<&str> = spec.split(':').collect();
    let arg = |i: usize, def: i64| -> Result<i64, String> {
        match parts.get(i) {
            Some(p) if !p.is_empty() => parse_int(p),
            _ => Ok(def),
        }
    };
    let has = |flag: &str| parts[1..].contains(&flag);
    match parts[0] {
        "rle" => Ok(FrameEncoder::Rle),
        "jpeg" => {
            let q = arg(1, 90)?;
            let s = arg(2, 0)?;
            if !(0..=255).contains(&s) {
                return Err(rejected());
            }
            jpeg::Encoder::with_options(jpeg::EncoderOptions {
                quality: q,
                subsampling: jpeg::Subsampling(s as u8),
                optimize_huffman: has("opt"),
            })
            .map(FrameEncoder::JpegBaseline)
            .map_err(|_| rejected())
        }
        "jpegx" => {
            let q = arg(1, 90)?;
            jpeg::ExtendedEncoder::with_options(jpeg::ExtendedEncoderOptions {
                quality: q,
                allow_color_compatibility: has("color"),
                allow_legacy_reduced_precision: has("reduced"),
                ..Default::default()
            })
            .map(FrameEncoder::JpegExtended)
            .map_err(|_| rejected())
        }
        "jl" => jpeglossless::Encoder::process14(arg(1, 1)?)
            .map(FrameEncoder::JpegLossless)
            .map_err(|_| rejected()),
        "jlsv1" => Ok(FrameEncoder::JpegLossless(jpeglossless::Encoder::sv1())),
        "jls" => {
            let near = arg(1, 0)?;
            let ilv = arg(2, 0)?;
            jpegls::Encoder::with_options(jpegls::EncoderOptions {
                near,
                allow_lossy: near > 0,
                interleave: ilv,
            })
            .map(FrameEncoder::JpegLs)
            .map_err(|_| rejected())
        }
        "defl" => {
            let level = arg(1, -1)?;
            if !(-2..=9).contains(&level) {
                return Err(rejected());
            }
            Ok(FrameEncoder::Deflate(level as i32))
        }
        "j2k" => {
            if parts.len() < 2 {
                return Err(rejected());
            }
            let levels = arg(3, 0)?;
            let step = match parts.get(4) {
                Some(p) if !p.is_empty() => p.parse::<f64>().map_err(|_| rejected())?,
                _ => 0.0,
            };
            jpeg2000::encoder::Encoder::new(&jpeg2000::encoder::Options {
                transfer_syntax_uid: parts[1].to_string(),
                color_transform: parts.len() > 2 && parts[2] == "mct",
                levels,
                step_size: step,
                allow_lossy: step > 0.0,
            })
            .map(FrameEncoder::Jpeg2000)
            .map_err(|_| rejected())
        }
        _ => Err(rejected()),
    }
}
fn parse_options(spec: &str) -> Result<TranscodeOptions, String> {
    let mut o = TranscodeOptions {
        decoder: Some(DecoderRegistry::Builtin),
        ..TranscodeOptions::default()
    };
    if spec.is_empty() {
        return Ok(o);
    }
    for kv in spec.split(';') {
        let Some((key, value)) = kv.split_once('=') else {
            return Err(rejected());
        };
        let num = || value.parse::<i64>().map_err(|_| rejected());
        match key {
            "dec" => {
                if value == "none" {
                    o.decoder = None;
                }
            }
            "enc" => {
                let mut reg = EncoderRegistry::new();
                if !value.is_empty() {
                    for s in value.split(',') {
                        let e = encoder(s)?;
                        reg.register_own(e).map_err(|e| e.to_string())?;
                    }
                }
                o.encoders = Some(reg);
            }
            "lossy" => o.allow_lossy = value == "1",
            "force" => o.force_reencode = value == "1",
            "sc" => o.secondary_capture = value == "1",
            "icons" => o.compress_icons = value == "1",
            "ovl" => o.extract_embedded_overlays = value == "1",
            "uid" => {
                o.uid_policy = match value {
                    "always" => UidPolicy::Always,
                    "preserve" => UidPolicy::Preserve,
                    _ => return Err(rejected()),
                }
            }
            "root" => o.uid_root = value.to_string(),
            "planar" => {
                o.planar_output = match value {
                    "iod" => PlanarOutput::Iod,
                    "inter" => PlanarOutput::Interleaved,
                    "planar" => PlanarOutput::Planar,
                    _ => return Err(rejected()),
                }
            }
            "frag" => o.max_fragment_bytes = num()?,
            "ot" => {
                let n = num()
                    .ok()
                    .filter(|n| (0..=3).contains(n))
                    .ok_or_else(rejected)?;
                o.offset_table = n as u8;
            }
            "frames" => o.limits.max_frames = num()?,
            "pixels" => {
                let n = num()?;
                if n < 0 {
                    return Err(rejected());
                }
                o.limits.max_pixels = n as u64;
            }
            "in" => o.limits.max_input_bytes = num()?,
            "out" => o.limits.max_output_bytes = num()?,
            "frags" => o.limits.max_fragments = num()?,
            "elems" => o.limits.max_elements = num()?,
            "depth" => o.limits.max_depth = num()?,
            "exp" => o.limits.max_expansion_ratio = num()?,
            _ => return Err(rejected()),
        }
    }
    Ok(o)
}
fn quote_list(values: &[String]) -> String {
    let parts: Vec<String> = values
        .iter()
        .map(|v| dicom_rust::textfmt::quote(v))
        .collect();
    format!("[{}]", parts.join(","))
}
fn report_text(r: &TranscodeReport) -> String {
    let mut s = format!(
        "frames={} native={} encoded={} preserved={} lossy={} identity={} overlays={} diags={} profiles={}",
        r.frames,
        r.native_bytes,
        r.encoded_bytes,
        r.pixel_data_preserved,
        r.lossy,
        r.identity_changed,
        r.embedded_overlays_extracted,
        quote_list(&r.decode_diagnostics),
        quote_list(&r.encode_compatibility_profiles),
    );
    for icon in &r.icons {
        s.push_str(&format!(
            "\nicon {} {} incomplete={} native={} encoded={} diags={} profiles={}",
            icon.path,
            icon.action.as_str(),
            icon.metadata_incomplete,
            icon.native_bytes,
            icon.encoded_bytes,
            quote_list(&icon.decode_diagnostics),
            quote_list(&icon.encode_compatibility_profiles),
        ));
    }
    s
}
fn uid_element(tag: Tag, value: &str) -> object::Element {
    dicom_rust::derivedio::str_elem(tag, Vr::UI, value)
}
fn uid_state(ds: &mut Object, original: &str, root: &str) -> &'static str {
    let uid = ds.get_string(TAG_SOP_INSTANCE).unwrap_or_default();
    if uid == original {
        return "same";
    }
    let valid = object::is_valid_uid(uid.as_bytes());
    let state = if root.is_empty() && uid.starts_with("2.25.") && valid {
        "new-uuid"
    } else if !root.is_empty()
        && uid.starts_with(&format!("{root}."))
        && uid.len() == root.len() + 1 + (64 - root.len() - 1).min(39)
        && valid
    {
        "new-root"
    } else {
        "bad"
    };
    ds.put(uid_element(TAG_SOP_INSTANCE, FIXED_UID));
    state
}
fn element_list(ds: &Object) -> String {
    let mut parts: Vec<String> = ds
        .elements()
        .iter()
        .map(|e| {
            format!(
                "{:04x}{:04x}:{}",
                e.tag().group,
                e.tag().element,
                e.vr().as_str()
            )
        })
        .collect();
    parts.sort();
    parts.join(" ")
}
fn file_text(mut f: File, original: &str, root: &str) -> Result<String, String> {
    let state = uid_state(&mut f.dataset, original, root);
    if state != "same"
        && let Some(v) = f.meta.get_string(TAG_MEDIA_SOP)
        && v != original
    {
        f.meta.put(uid_element(TAG_MEDIA_SOP, FIXED_UID));
    }
    let buf = object::writer::write_file(&f).map_err(|e| format!("write: {e}"))?;
    let sum = Sha256::digest(&buf);
    Ok(format!(
        "uid={state} ts={} len={} sha={}\n{}",
        f.transfer_syntax.uid,
        buf.len(),
        hex::encode(&sum[..12]),
        element_list(&f.dataset)
    ))
}
fn read(data: &[i64]) -> Result<(File, String), String> {
    let f = object::read_file(&bytes(data)).map_err(|_| rejected())?;
    let original = f.dataset.get_string(TAG_SOP_INSTANCE).unwrap_or_default();
    Ok((f, original))
}
pub fn transcode(data: Vec<i64>, target: String, options: String) -> Result<String, String> {
    let (f, original) = read(&data)?;
    let o = parse_options(&options)?;
    let (out, report) = tc::transcode_file(&f, &target, &o).map_err(|e| e.to_string())?;
    let text = file_text(out, &original, &o.uid_root)?;
    Ok(format!("{}\n{text}", report_text(&report)))
}
pub fn transcode_data_set(
    data: Vec<i64>,
    source: String,
    target: String,
    options: String,
) -> Result<String, String> {
    let (f, original) = read(&data)?;
    let o = parse_options(&options)?;
    let (ds, report) =
        tc::transcode_data_set(&f.dataset, &source, &target, &o).map_err(|e| e.to_string())?;
    let Some(syntax) = object::transfer::lookup(&target) else {
        return Err(rejected());
    };
    let file = File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset: ds,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    let text = file_text(file, &original, &o.uid_root)?;
    Ok(format!("{}\n{text}", report_text(&report)))
}
pub fn decompress(
    data: Vec<i64>,
    target: String,
    max_frames: i64,
    max_native: i64,
) -> Result<String, String> {
    let (f, original) = read(&data)?;
    let out = tc::decompress_file(
        &f,
        &DecompressOptions {
            registry: Some(DecoderRegistry::Builtin),
            target_transfer_syntax: target,
            limits: DecompressLimits {
                max_frames,
                max_native_bytes: max_native,
                ..DecompressLimits::default()
            },
        },
    )
    .map_err(|e| e.to_string())?;
    file_text(out, &original, "")
}
pub fn limits(options: String) -> Result<String, String> {
    let o = parse_options(&options)?;
    let l = tc::resolve_limits(o.limits).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} {} {} {} {} {} {} {}",
        l.max_frames,
        l.max_pixels,
        l.max_input_bytes,
        l.max_output_bytes,
        l.max_fragments,
        l.max_elements,
        l.max_depth,
        l.max_expansion_ratio
    ))
}
fn u16_list(v: &[u16]) -> String {
    let parts: Vec<String> = v.iter().map(u16::to_string).collect();
    format!("[{}]", parts.join(" "))
}
pub fn encoder_caps(specs: String) -> Result<String, String> {
    let mut reg = EncoderRegistry::new();
    let mut lines = Vec::new();
    for s in specs.split(',') {
        let e = encoder(s)?;
        let c = e.capabilities();
        reg.register(&c.transfer_syntax_uid, e)
            .map_err(|e| e.to_string())?;
        lines.push(format!(
            "{} ba={} pr={} spp={} pi={} out={} planar={} lossless={} method={} multi={} backend={} profiles={}",
            c.transfer_syntax_uid,
            u16_list(&c.bits_allocated),
            u16_list(&c.pixel_representations),
            u16_list(&c.samples_per_pixel),
            quote_list(&c.photometric_interpretations),
            quote_list(&c.output_photometric_interpretations),
            u16_list(&c.output_planar_configurations),
            c.lossless,
            dicom_rust::textfmt::quote(&c.lossy_method),
            c.supports_multi_frame,
            dicom_rust::textfmt::quote(&c.backend),
            quote_list(&c.compatibility_profiles),
        ));
    }
    Ok(format!(
        "{}\nregistered={}",
        lines.join("\n"),
        reg.registered_uids().join(",")
    ))
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "Transcode" => {
            let out = transcode(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "TranscodeDataSet" => {
            let out =
                transcode_data_set(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Decompress" => {
            let out = decompress(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?, arg(args, 3)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Limits" => {
            let out = limits(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "EncoderCaps" => {
            let out = encoder_caps(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
