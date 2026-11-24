#[allow(unused_imports)]
use super::support::*;
use dicom_rust::textfmt::quote;
use dicom_rust::transfer::{self as t, MemoryRegistry, NativeStorePreference, Syntax};
use serde::Deserialize;
use std::borrow::Cow;
const HARNESS: &str = "harness: input rejected";
fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}
fn dump(s: &Syntax) -> String {
    format!(
        "{} {} ex={} le={} sup={} enc={} def={} media={} creq={} cav={} req={} has={}",
        quote(&s.uid),
        quote(&s.name),
        b(s.explicit_vr),
        b(s.is_little_endian()),
        b(s.supported),
        b(s.encapsulated),
        b(s.deflated),
        b(s.media_payload),
        b(s.codec_required),
        b(s.codec_available),
        b(s.requires_codec()),
        b(s.has_codec())
    )
}
fn dump_found(s: Option<Syntax>) -> String {
    format!("{} {}", b(s.is_some()), dump(&s.unwrap_or_default()))
}
fn dump_all(list: &[Syntax]) -> String {
    let lines: Vec<String> = list.iter().map(dump).collect();
    format!("{}\n{}", list.len(), lines.join("\n"))
}
fn list(uids: &[String]) -> String {
    let quoted: Vec<String> = uids.iter().map(|u| quote(u)).collect();
    format!("{} [{}]", uids.len(), quoted.join(" "))
}
fn from_flags(uid: &str, name: &str, flags: i64) -> Syntax {
    Syntax {
        uid: Cow::Owned(uid.to_string()),
        name: Cow::Owned(name.to_string()),
        explicit_vr: flags & 1 != 0,
        big_endian: (flags >> 7) & 3 == 2,
        supported: flags & 2 != 0,
        encapsulated: flags & 4 != 0,
        deflated: flags & 8 != 0,
        media_payload: flags & 16 != 0,
        codec_required: flags & 32 != 0,
        codec_available: flags & 64 != 0,
    }
}
pub fn syntax_flags(uid: String, name: String, flags: i64) -> Result<String, String> {
    Ok(dump(&from_flags(&uid, &name, flags)))
}
fn named_list() -> Vec<(&'static str, Syntax)> {
    vec![
        ("ImplicitVRLittleEndian", t::IMPLICIT_VR_LITTLE_ENDIAN),
        ("ExplicitVRLittleEndian", t::EXPLICIT_VR_LITTLE_ENDIAN),
        (
            "DeflatedExplicitVRLittleEndian",
            t::DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
        ),
        ("ExplicitVRBigEndian", t::EXPLICIT_VR_BIG_ENDIAN),
        (
            "EncapsulatedUncompressedExplicitVRLittleEndian",
            t::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
        ),
        ("JPEGBaseline", t::JPEG_BASELINE),
        ("JPEGExtended", t::JPEG_EXTENDED),
        ("JPEGSpectralSelection", t::JPEG_SPECTRAL_SELECTION),
        ("JPEGFullProgression", t::JPEG_FULL_PROGRESSION),
        (
            "JPEGLosslessNonHierarchical",
            t::JPEG_LOSSLESS_NON_HIERARCHICAL,
        ),
        ("JPEGLosslessSV1", t::JPEG_LOSSLESS_SV1),
        ("JPEGLSLossless", t::JPEG_LS_LOSSLESS),
        ("JPEGLSNearLossless", t::JPEG_LS_NEAR_LOSSLESS),
        ("JPEG2000LosslessOnly", t::JPEG_2000_LOSSLESS_ONLY),
        ("JPEG2000", t::JPEG_2000),
        ("JPEG2000Part2Lossless", t::JPEG_2000_PART2_LOSSLESS),
        ("JPEG2000Part2", t::JPEG_2000_PART2),
        ("RLELossless", t::RLE_LOSSLESS),
        ("JPIPReferenced", t::JPIP_REFERENCED),
        ("JPIPReferencedDeflate", t::JPIP_REFERENCED_DEFLATE),
        ("MPEG2MPML", t::MPEG2_MPML),
        ("MPEG2MPMLF", t::MPEG2_MPML_F),
        ("MPEG2MPHL", t::MPEG2_MPHL),
        ("MPEG2MPHLF", t::MPEG2_MPHL_F),
        ("MPEG4HP41", t::MPEG4_HP41),
        ("MPEG4HP41F", t::MPEG4_HP41_F),
        ("MPEG4HP41BD", t::MPEG4_HP41_BD),
        ("MPEG4HP41BDF", t::MPEG4_HP41_BD_F),
        ("MPEG4HP422D", t::MPEG4_HP42_2D),
        ("MPEG4HP422DF", t::MPEG4_HP42_2D_F),
        ("MPEG4HP423D", t::MPEG4_HP42_3D),
        ("MPEG4HP423DF", t::MPEG4_HP42_3D_F),
        ("MPEG4HP42STEREO", t::MPEG4_HP42_STEREO),
        ("MPEG4HP42STEREOF", t::MPEG4_HP42_STEREO_F),
        ("HEVCMP51", t::HEVC_MP51),
        ("HEVCM10P51", t::HEVC_M10P51),
        ("JPEGXLLossless", t::JPEG_XL_LOSSLESS),
        ("JPEGXLJPEGRecompression", t::JPEG_XL_JPEG_RECOMPRESSION),
        ("JPEGXL", t::JPEG_XL),
        ("HTJ2KLossless", t::HTJ2K_LOSSLESS),
        ("HTJ2KLosslessRPCL", t::HTJ2K_LOSSLESS_RPCL),
        ("HTJ2K", t::HTJ2K),
        ("JPIPHTJ2KReferenced", t::JPIP_HTJ2K_REFERENCED),
        (
            "JPIPHTJ2KReferencedDeflate",
            t::JPIP_HTJ2K_REFERENCED_DEFLATE,
        ),
        ("RFC2557MIMEEncapsulation", t::RFC_2557_MIME_ENCAPSULATION),
        ("XMLEncoding", t::XML_ENCODING),
        (
            "SMPTEST211020UncompressedProgressiveActiveVideo",
            t::SMPTE_ST_2110_20_UNCOMPRESSED_PROGRESSIVE_ACTIVE_VIDEO,
        ),
        (
            "SMPTEST211020UncompressedInterlacedActiveVideo",
            t::SMPTE_ST_2110_20_UNCOMPRESSED_INTERLACED_ACTIVE_VIDEO,
        ),
        (
            "SMPTEST211030PCMDigitalAudio",
            t::SMPTE_ST_2110_30_PCM_DIGITAL_AUDIO,
        ),
        (
            "DeflatedImageFrameCompression",
            t::DEFLATED_IMAGE_FRAME_COMPRESSION,
        ),
        (
            "ImplicitVRBigEndianACRNEMA",
            t::IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA,
        ),
        (
            "PrivateGEImplicitVRBigEndianPixelData",
            t::PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA,
        ),
    ]
}
pub fn named_count() -> Result<i64, String> {
    Ok(named_list().len() as i64)
}
pub fn named(index: i64) -> Result<String, String> {
    let all = named_list();
    let (name, syntax) = usize::try_from(index)
        .ok()
        .and_then(|i| all.get(i))
        .ok_or(HARNESS)?;
    Ok(format!("{name} {}", dump(syntax)))
}
pub fn error_texts() -> Result<String, String> {
    Ok(format!(
        "{}\n{}",
        t::Error::UnknownTransferSyntax,
        t::Error::UnsupportedTransferSyntax
    ))
}
pub fn normalize_uid(uid: String) -> Result<String, String> {
    Ok(t::normalize_uid(&uid).to_string())
}
pub fn default_get(uid: String) -> Result<String, String> {
    Ok(dump_found(t::default_registry().get(&uid)))
}
pub fn default_all() -> Result<String, String> {
    Ok(dump_all(&t::default_registry().all()))
}
pub fn legacy_syntax(uid: String) -> Result<String, String> {
    Ok(dump_found(t::legacy_syntax(&uid)))
}
pub fn classify(uid: String) -> Result<String, String> {
    Ok(format!(
        "video={} jpegls={} j2k={} jxl={}",
        b(t::is_video_transfer_syntax(&uid)),
        b(t::is_jpeg_ls_transfer_syntax(&uid)),
        b(t::is_jpeg_2000_transfer_syntax(&uid)),
        b(t::is_jpeg_xl_transfer_syntax(&uid))
    ))
}
pub fn proposed_store(source_uid: String, pref: i64) -> Result<String, String> {
    let pref = match pref {
        1 => NativeStorePreference::ExplicitLittleEndianFirst,
        2 => NativeStorePreference::ImplicitLittleEndianFirst,
        _ => NativeStorePreference::SourceFirst,
    };
    Ok(list(&t::proposed_store_transfer_syntax_uids(
        &source_uid,
        pref,
    )))
}
pub fn preference_values() -> Result<String, String> {
    Ok(format!(
        "{} {} {}",
        NativeStorePreference::SourceFirst as i64,
        NativeStorePreference::ExplicitLittleEndianFirst as i64,
        NativeStorePreference::ImplicitLittleEndianFirst as i64
    ))
}
pub fn receive_ui_ds(native: Vec<String>) -> Result<String, String> {
    if native.len() > 256 {
        return Ok("harness: too many".to_string());
    }
    let order: Vec<Syntax> = native
        .into_iter()
        .map(|uid| Syntax {
            uid: Cow::Owned(uid),
            ..Syntax::default()
        })
        .collect();
    Ok(list(&t::receive_transfer_syntax_uids(&order)))
}
pub fn preservable() -> Result<String, String> {
    let mut first = t::preservable_compressed_receive_syntaxes();
    if let Some(s) = first.first_mut() {
        s.uid = Cow::Borrowed("mutated");
    }
    Ok(dump_all(&t::preservable_compressed_receive_syntaxes()))
}
pub fn filter(proposals: Vec<String>, allowlist: Vec<String>) -> Result<String, String> {
    if proposals.len() > 256 || allowlist.len() > 256 {
        return Ok("harness: too many".to_string());
    }
    let out = t::filter_transfer_syntax_uids(&proposals, &allowlist);
    if !allowlist.is_empty() && out.is_empty() {
        return Ok("nil".to_string());
    }
    Ok(list(&out))
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct RegOp {
    op: String,
    uid: String,
    name: String,
    flags: i64,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct RegScript {
    kind: String,
    initial: Option<Vec<RegOp>>,
    ops: Option<Vec<RegOp>>,
}
pub fn registry_script(spec: String) -> Result<String, String> {
    if !spec.trim_start().starts_with('{') {
        return Err(HARNESS.to_string());
    }
    let script: RegScript = serde_json::from_str(&spec).map_err(|_| HARNESS.to_string())?;
    let initial = script.initial.unwrap_or_default();
    let ops = script.ops.unwrap_or_default();
    if initial.len() > 256 || ops.len() > 256 {
        return Err(HARNESS.to_string());
    }
    let registry: Option<MemoryRegistry> = match script.kind.as_str() {
        "new" => Some(MemoryRegistry::new(
            initial
                .iter()
                .map(|op| from_flags(&op.uid, &op.name, op.flags)),
        )),
        "zero" => Some(MemoryRegistry::default()),
        "nil" => None,
        _ => return Err(HARNESS.to_string()),
    };
    let mut out = Vec::new();
    for op in &ops {
        match op.op.as_str() {
            "register" => {
                if let Some(r) = &registry {
                    r.register(from_flags(&op.uid, &op.name, op.flags));
                }
                out.push("register".to_string());
            }
            "get" => {
                let found = registry.as_ref().and_then(|r| r.get(&op.uid));
                out.push(format!("get {}", dump_found(found)));
            }
            "all" => match &registry {
                Some(r) => {
                    let mut all = r.all();
                    let text = dump_all(&all);
                    if let Some(s) = all.first_mut() {
                        s.name = Cow::Borrowed("mutated");
                    }
                    out.push(format!("all {text}"));
                }
                None => out.push("all nil".to_string()),
            },
            _ => return Err(HARNESS.to_string()),
        }
    }
    Ok(out.join("\n"))
}
pub fn call(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    match name {
        "SyntaxFlags" => {
            let out = syntax_flags(arg(args, 0)?, arg(args, 1)?, arg(args, 2)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "NamedCount" => {
            let out = named_count()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Named" => {
            let out = named(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ErrorTexts" => {
            let out = error_texts()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "NormalizeUID" => {
            let out = normalize_uid(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DefaultGet" => {
            let out = default_get(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "DefaultAll" => {
            let out = default_all()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "LegacySyntax" => {
            let out = legacy_syntax(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Classify" => {
            let out = classify(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ProposedStore" => {
            let out = proposed_store(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "PreferenceValues" => {
            let out = preference_values()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "ReceiveUIDs" => {
            let out = receive_ui_ds(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Preservable" => {
            let out = preservable()?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "Filter" => {
            let out = filter(arg(args, 0)?, arg(args, 1)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        "RegistryScript" => {
            let out = registry_script(arg(args, 0)?)?;
            Ok(serde_json::to_value(out).map_err(|e| e.to_string())?)
        }
        _ => Err(format!("no function `{name}`")),
    }
}
