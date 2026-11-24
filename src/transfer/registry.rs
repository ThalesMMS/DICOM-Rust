use super::*;
use crate::dictionary::uid as uiddict;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{LazyLock, RwLock};
pub trait Registry: Send + Sync {
    fn get(&self, uid: &str) -> Option<Syntax>;
    fn register(&self, syntax: Syntax);
    fn all(&self) -> Vec<Syntax>;
}
#[derive(Debug, Default)]
pub struct MemoryRegistry {
    by_uid: RwLock<BTreeMap<String, Syntax>>,
}
impl MemoryRegistry {
    pub fn new(syntaxes: impl IntoIterator<Item = Syntax>) -> MemoryRegistry {
        let r = MemoryRegistry::default();
        for syntax in syntaxes {
            r.register(syntax);
        }
        r
    }
    pub fn get(&self, uid: &str) -> Option<Syntax> {
        let map = self.by_uid.read().unwrap_or_else(|e| e.into_inner());
        map.get(normalize_uid(uid)).cloned()
    }
    pub fn register(&self, mut syntax: Syntax) {
        let normalized = normalize_uid(&syntax.uid);
        if normalized.is_empty() {
            return;
        }
        if normalized.len() != syntax.uid.len() {
            syntax.uid = Cow::Owned(normalized.to_string());
        }
        let mut map = self.by_uid.write().unwrap_or_else(|e| e.into_inner());
        map.insert(syntax.uid.to_string(), syntax);
    }
    pub fn all(&self) -> Vec<Syntax> {
        let map = self.by_uid.read().unwrap_or_else(|e| e.into_inner());
        map.values().cloned().collect()
    }
}
impl Registry for MemoryRegistry {
    fn get(&self, uid: &str) -> Option<Syntax> {
        MemoryRegistry::get(self, uid)
    }
    fn register(&self, syntax: Syntax) {
        MemoryRegistry::register(self, syntax)
    }
    fn all(&self) -> Vec<Syntax> {
        MemoryRegistry::all(self)
    }
}
static DEFAULT_REGISTRY: LazyLock<MemoryRegistry> = LazyLock::new(new_default_registry);
pub fn default_registry() -> &'static MemoryRegistry {
    &DEFAULT_REGISTRY
}
const REGISTERED: [Syntax; 50] = [
    IMPLICIT_VR_LITTLE_ENDIAN,
    EXPLICIT_VR_LITTLE_ENDIAN,
    DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
    EXPLICIT_VR_BIG_ENDIAN,
    ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
    JPEG_BASELINE,
    JPEG_EXTENDED,
    JPEG_SPECTRAL_SELECTION,
    JPEG_FULL_PROGRESSION,
    JPEG_LOSSLESS_NON_HIERARCHICAL,
    JPEG_LOSSLESS_SV1,
    JPEG_LS_LOSSLESS,
    JPEG_LS_NEAR_LOSSLESS,
    JPEG_2000_LOSSLESS_ONLY,
    JPEG_2000,
    JPEG_2000_PART2_LOSSLESS,
    JPEG_2000_PART2,
    RLE_LOSSLESS,
    JPIP_REFERENCED,
    JPIP_REFERENCED_DEFLATE,
    MPEG2_MPML,
    MPEG2_MPML_F,
    MPEG2_MPHL,
    MPEG2_MPHL_F,
    MPEG4_HP41,
    MPEG4_HP41_F,
    MPEG4_HP41_BD,
    MPEG4_HP41_BD_F,
    MPEG4_HP42_2D,
    MPEG4_HP42_2D_F,
    MPEG4_HP42_3D,
    MPEG4_HP42_3D_F,
    MPEG4_HP42_STEREO,
    MPEG4_HP42_STEREO_F,
    HEVC_MP51,
    HEVC_M10P51,
    JPEG_XL_LOSSLESS,
    JPEG_XL_JPEG_RECOMPRESSION,
    JPEG_XL,
    HTJ2K_LOSSLESS,
    HTJ2K_LOSSLESS_RPCL,
    HTJ2K,
    JPIP_HTJ2K_REFERENCED,
    JPIP_HTJ2K_REFERENCED_DEFLATE,
    RFC_2557_MIME_ENCAPSULATION,
    XML_ENCODING,
    SMPTE_ST_2110_20_UNCOMPRESSED_PROGRESSIVE_ACTIVE_VIDEO,
    SMPTE_ST_2110_20_UNCOMPRESSED_INTERLACED_ACTIVE_VIDEO,
    SMPTE_ST_2110_30_PCM_DIGITAL_AUDIO,
    DEFLATED_IMAGE_FRAME_COMPRESSION,
];
fn new_default_registry() -> MemoryRegistry {
    let r = MemoryRegistry::default();
    for entry in uiddict::entries_by_type(uiddict::Type::TRANSFER_SYNTAX) {
        r.register(known_transfer_syntax(entry));
    }
    for syntax in REGISTERED {
        r.register(syntax);
    }
    r
}
fn known_transfer_syntax(entry: &uiddict::Entry) -> Syntax {
    const KNOWN: [Syntax; 23] = [
        IMPLICIT_VR_LITTLE_ENDIAN,
        EXPLICIT_VR_LITTLE_ENDIAN,
        DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN,
        EXPLICIT_VR_BIG_ENDIAN,
        ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
        JPEG_BASELINE,
        JPEG_EXTENDED,
        JPEG_SPECTRAL_SELECTION,
        JPEG_FULL_PROGRESSION,
        JPEG_LOSSLESS_NON_HIERARCHICAL,
        JPEG_LOSSLESS_SV1,
        JPEG_LS_LOSSLESS,
        JPEG_LS_NEAR_LOSSLESS,
        JPEG_2000_LOSSLESS_ONLY,
        JPEG_2000,
        JPEG_2000_PART2_LOSSLESS,
        JPEG_2000_PART2,
        RLE_LOSSLESS,
        JPIP_REFERENCED,
        JPIP_REFERENCED_DEFLATE,
        JPIP_HTJ2K_REFERENCED,
        JPIP_HTJ2K_REFERENCED_DEFLATE,
        DEFLATED_IMAGE_FRAME_COMPRESSION,
    ];
    if let Some(known) = KNOWN.iter().find(|s| s.uid == entry.uid) {
        return known.clone();
    }
    if is_jpeg_xl_transfer_syntax(entry.uid) {
        return JPEG_XL.renamed(entry.uid, entry.name);
    }
    if is_video_transfer_syntax(entry.uid) {
        return MPEG2_MPML.renamed(entry.uid, entry.name);
    }
    if is_retired_jpeg_transfer_syntax_uid(entry.uid) {
        return JPEG_2000.renamed(entry.uid, entry.name);
    }
    let mut syntax = XML_ENCODING.renamed(entry.uid, entry.name);
    match entry.uid {
        "1.2.840.10008.1.20" => {
            syntax.explicit_vr = false;
            return syntax;
        }
        "1.2.840.10008.1.2.4.95" | "1.2.840.10008.1.2.4.205" => {
            syntax.deflated = true;
            return syntax;
        }
        _ => {}
    }
    syntax.encapsulated = is_encapsulated_transfer_syntax_uid(entry.uid);
    syntax
}
fn is_retired_jpeg_transfer_syntax_uid(uid: &str) -> bool {
    let Some(suffix) = uid.strip_prefix("1.2.840.10008.1.2.4.") else {
        return false;
    };
    match suffix.parse::<i64>() {
        Ok(n) => (52..=66).contains(&n) && n != 57 && n.to_string() == suffix,
        Err(_) => false,
    }
}
fn is_encapsulated_transfer_syntax_uid(uid: &str) -> bool {
    if uid == "1.2.840.10008.1.2.1.98" {
        return true;
    }
    if uid.starts_with("1.2.840.10008.1.2.4.") {
        return uid != "1.2.840.10008.1.2.4.95" && uid != "1.2.840.10008.1.2.4.205";
    }
    uid == "1.2.840.10008.1.2.5"
        || uid.starts_with("1.2.840.10008.1.2.7.")
        || uid == "1.2.840.10008.1.2.8.1"
}
