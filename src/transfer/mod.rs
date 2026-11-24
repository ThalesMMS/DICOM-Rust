#![forbid(unsafe_code)]
use std::borrow::Cow;
use std::fmt;
mod negotiation;
mod registry;
pub use crate::object::transfer::normalize_uid;
pub use negotiation::{
    NativeStorePreference, filter_transfer_syntax_uids, preservable_compressed_receive_syntaxes,
    proposed_store_transfer_syntax_uids, receive_transfer_syntax_uids,
};
pub use registry::{MemoryRegistry, Registry, default_registry};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    UnknownTransferSyntax,
    UnsupportedTransferSyntax,
}
impl Error {
    pub const fn as_str(self) -> &'static str {
        match self {
            Error::UnknownTransferSyntax => "dicom: unknown transfer syntax",
            Error::UnsupportedTransferSyntax => "dicom: unsupported transfer syntax",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Syntax {
    pub uid: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub explicit_vr: bool,
    pub big_endian: bool,
    pub supported: bool,
    pub encapsulated: bool,
    pub deflated: bool,
    pub media_payload: bool,
    pub codec_required: bool,
    pub codec_available: bool,
}
impl Syntax {
    pub fn is_little_endian(&self) -> bool {
        !self.big_endian
    }
    pub fn requires_codec(&self) -> bool {
        if self.media_payload {
            return false;
        }
        self.deflated
            || self.codec_required
            || (self.encapsulated && (!self.supported || self.codec_available))
    }
    pub fn has_codec(&self) -> bool {
        self.codec_available
    }
    fn renamed(&self, uid: &'static str, name: &'static str) -> Syntax {
        Syntax {
            uid: Cow::Borrowed(uid),
            name: Cow::Borrowed(name),
            ..self.clone()
        }
    }
}
const fn base(uid: &'static str, name: &'static str) -> Syntax {
    Syntax {
        uid: Cow::Borrowed(uid),
        name: Cow::Borrowed(name),
        explicit_vr: true,
        big_endian: false,
        supported: false,
        encapsulated: false,
        deflated: false,
        media_payload: false,
        codec_required: false,
        codec_available: false,
    }
}
const fn native(
    uid: &'static str,
    name: &'static str,
    explicit_vr: bool,
    big_endian: bool,
) -> Syntax {
    let mut s = base(uid, name);
    s.explicit_vr = explicit_vr;
    s.big_endian = big_endian;
    s.supported = true;
    s.codec_available = true;
    s
}
const fn supported_encapsulated(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = base(uid, name);
    s.supported = true;
    s.encapsulated = true;
    s.codec_available = true;
    s
}
const fn fragment_only(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = base(uid, name);
    s.supported = true;
    s.encapsulated = true;
    s
}
const fn video_media(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = fragment_only(uid, name);
    s.media_payload = true;
    s
}
const fn optional_still_image_codec(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = fragment_only(uid, name);
    s.codec_required = true;
    s
}
const fn jpip_referenced(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = base(uid, name);
    s.supported = true;
    s
}
const fn jpip_referenced_deflated(uid: &'static str, name: &'static str) -> Syntax {
    let mut s = jpip_referenced(uid, name);
    s.deflated = true;
    s
}
const fn unsupported_explicit_le(uid: &'static str, name: &'static str) -> Syntax {
    base(uid, name)
}
pub const IMPLICIT_VR_LITTLE_ENDIAN: Syntax = native(
    "1.2.840.10008.1.2",
    "Implicit VR Little Endian",
    false,
    false,
);
pub const EXPLICIT_VR_LITTLE_ENDIAN: Syntax = native(
    "1.2.840.10008.1.2.1",
    "Explicit VR Little Endian",
    true,
    false,
);
pub const DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN: Syntax = {
    let mut s = native(
        "1.2.840.10008.1.2.1.99",
        "Deflated Explicit VR Little Endian",
        true,
        false,
    );
    s.deflated = true;
    s
};
pub const EXPLICIT_VR_BIG_ENDIAN: Syntax =
    native("1.2.840.10008.1.2.2", "Explicit VR Big Endian", true, true);
pub const ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN: Syntax = fragment_only(
    "1.2.840.10008.1.2.1.98",
    "Encapsulated Uncompressed Explicit VR Little Endian",
);
pub const JPEG_BASELINE: Syntax =
    supported_encapsulated("1.2.840.10008.1.2.4.50", "JPEG Baseline (Process 1)");
pub const JPEG_EXTENDED: Syntax =
    supported_encapsulated("1.2.840.10008.1.2.4.51", "JPEG Extended (Process 2 & 4)");
pub const JPEG_SPECTRAL_SELECTION: Syntax = supported_encapsulated(
    "1.2.840.10008.1.2.4.53",
    "JPEG Spectral Selection, Non-Hierarchical (Process 6 & 8) (Retired)",
);
pub const JPEG_FULL_PROGRESSION: Syntax = supported_encapsulated(
    "1.2.840.10008.1.2.4.55",
    "JPEG Full Progression, Non-Hierarchical (Process 10 & 12) (Retired)",
);
pub const JPEG_LOSSLESS_NON_HIERARCHICAL: Syntax = supported_encapsulated(
    "1.2.840.10008.1.2.4.57",
    "JPEG Lossless, Non-Hierarchical (Process 14)",
);
pub const JPEG_LOSSLESS_SV1: Syntax = supported_encapsulated(
    "1.2.840.10008.1.2.4.70",
    "JPEG Lossless, Non-Hierarchical, First-Order Prediction",
);
pub const JPEG_LS_LOSSLESS: Syntax = supported_encapsulated(
    "1.2.840.10008.1.2.4.80",
    "JPEG-LS Lossless Image Compression",
);
pub const JPEG_LS_NEAR_LOSSLESS: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.81",
    "JPEG-LS Lossy (Near-Lossless) Image Compression",
);
pub const JPEG_2000_LOSSLESS_ONLY: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.90",
    "JPEG 2000 Image Compression (Lossless Only)",
);
pub const JPEG_2000: Syntax =
    optional_still_image_codec("1.2.840.10008.1.2.4.91", "JPEG 2000 Image Compression");
pub const JPEG_2000_PART2_LOSSLESS: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.92",
    "JPEG 2000 Part 2 Multi-component Image Compression (Lossless Only)",
);
pub const JPEG_2000_PART2: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.93",
    "JPEG 2000 Part 2 Multi-component Image Compression",
);
pub const RLE_LOSSLESS: Syntax = supported_encapsulated("1.2.840.10008.1.2.5", "RLE Lossless");
pub const JPIP_REFERENCED: Syntax = jpip_referenced("1.2.840.10008.1.2.4.94", "JPIP Referenced");
pub const JPIP_REFERENCED_DEFLATE: Syntax =
    jpip_referenced_deflated("1.2.840.10008.1.2.4.95", "JPIP Referenced Deflate");
pub const MPEG2_MPML: Syntax =
    video_media("1.2.840.10008.1.2.4.100", "MPEG2 Main Profile / Main Level");
pub const MPEG2_MPML_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.100.1",
    "Fragmentable MPEG2 Main Profile / Main Level",
);
pub const MPEG2_MPHL: Syntax =
    video_media("1.2.840.10008.1.2.4.101", "MPEG2 Main Profile / High Level");
pub const MPEG2_MPHL_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.101.1",
    "Fragmentable MPEG2 Main Profile / High Level",
);
pub const MPEG4_HP41: Syntax = video_media(
    "1.2.840.10008.1.2.4.102",
    "MPEG-4 AVC/H.264 High Profile / Level 4.1",
);
pub const MPEG4_HP41_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.102.1",
    "Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.1",
);
pub const MPEG4_HP41_BD: Syntax = video_media(
    "1.2.840.10008.1.2.4.103",
    "MPEG-4 AVC/H.264 BD-compatible High Profile / Level 4.1",
);
pub const MPEG4_HP41_BD_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.103.1",
    "Fragmentable MPEG-4 AVC/H.264 BD-compatible High Profile / Level 4.1",
);
pub const MPEG4_HP42_2D: Syntax = video_media(
    "1.2.840.10008.1.2.4.104",
    "MPEG-4 AVC/H.264 High Profile / Level 4.2 For 2D Video",
);
pub const MPEG4_HP42_2D_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.104.1",
    "Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.2 For 2D Video",
);
pub const MPEG4_HP42_3D: Syntax = video_media(
    "1.2.840.10008.1.2.4.105",
    "MPEG-4 AVC/H.264 High Profile / Level 4.2 For 3D Video",
);
pub const MPEG4_HP42_3D_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.105.1",
    "Fragmentable MPEG-4 AVC/H.264 High Profile / Level 4.2 For 3D Video",
);
pub const MPEG4_HP42_STEREO: Syntax = video_media(
    "1.2.840.10008.1.2.4.106",
    "MPEG-4 AVC/H.264 Stereo High Profile / Level 4.2",
);
pub const MPEG4_HP42_STEREO_F: Syntax = video_media(
    "1.2.840.10008.1.2.4.106.1",
    "Fragmentable MPEG-4 AVC/H.264 Stereo High Profile / Level 4.2",
);
pub const HEVC_MP51: Syntax = video_media(
    "1.2.840.10008.1.2.4.107",
    "HEVC/H.265 Main Profile / Level 5.1",
);
pub const HEVC_M10P51: Syntax = video_media(
    "1.2.840.10008.1.2.4.108",
    "HEVC/H.265 Main 10 Profile / Level 5.1",
);
pub const JPEG_XL_LOSSLESS: Syntax =
    optional_still_image_codec("1.2.840.10008.1.2.4.110", "JPEG XL Lossless");
pub const JPEG_XL_JPEG_RECOMPRESSION: Syntax =
    optional_still_image_codec("1.2.840.10008.1.2.4.111", "JPEG XL JPEG Recompression");
pub const JPEG_XL: Syntax = optional_still_image_codec("1.2.840.10008.1.2.4.112", "JPEG XL");
pub const HTJ2K_LOSSLESS: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.201",
    "High-Throughput JPEG 2000 Image Compression (Lossless Only)",
);
pub const HTJ2K_LOSSLESS_RPCL: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.202",
    "High-Throughput JPEG 2000 with RPCL Options Image Compression (Lossless Only)",
);
pub const HTJ2K: Syntax = optional_still_image_codec(
    "1.2.840.10008.1.2.4.203",
    "High-Throughput JPEG 2000 Image Compression",
);
pub const JPIP_HTJ2K_REFERENCED: Syntax =
    jpip_referenced("1.2.840.10008.1.2.4.204", "JPIP HTJ2K Referenced");
pub const JPIP_HTJ2K_REFERENCED_DEFLATE: Syntax =
    jpip_referenced_deflated("1.2.840.10008.1.2.4.205", "JPIP HTJ2K Referenced Deflate");
pub const RFC_2557_MIME_ENCAPSULATION: Syntax = unsupported_explicit_le(
    "1.2.840.10008.1.2.6.1",
    "RFC 2557 MIME encapsulation (Retired)",
);
pub const XML_ENCODING: Syntax =
    unsupported_explicit_le("1.2.840.10008.1.2.6.2", "XML Encoding (Retired)");
pub const SMPTE_ST_2110_20_UNCOMPRESSED_PROGRESSIVE_ACTIVE_VIDEO: Syntax = unsupported_explicit_le(
    "1.2.840.10008.1.2.7.1",
    "SMPTE ST 2110-20 Uncompressed Progressive Active Video",
);
pub const SMPTE_ST_2110_20_UNCOMPRESSED_INTERLACED_ACTIVE_VIDEO: Syntax = unsupported_explicit_le(
    "1.2.840.10008.1.2.7.2",
    "SMPTE ST 2110-20 Uncompressed Interlaced Active Video",
);
pub const SMPTE_ST_2110_30_PCM_DIGITAL_AUDIO: Syntax = unsupported_explicit_le(
    "1.2.840.10008.1.2.7.3",
    "SMPTE ST 2110-30 PCM Digital Audio",
);
pub const DEFLATED_IMAGE_FRAME_COMPRESSION: Syntax =
    supported_encapsulated("1.2.840.10008.1.2.8.1", "Deflated Image Frame Compression");
pub const IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA: Syntax = native(
    "ACR-NEMA-Implicit-VR-Big-Endian",
    "ACR-NEMA Implicit VR Big Endian",
    false,
    true,
);
pub const PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA: Syntax = native(
    "1.2.840.113619.5.2",
    "Private GE Implicit VR Little Endian with Big Endian Pixel Data",
    false,
    false,
);
pub fn legacy_syntax(uid: &str) -> Option<Syntax> {
    match normalize_uid(uid) {
        "ACR-NEMA-Implicit-VR-Big-Endian" => Some(IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA),
        "1.2.840.113619.5.2" => Some(PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA),
        _ => None,
    }
}
pub fn is_video_transfer_syntax(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        "1.2.840.10008.1.2.4.100"
            | "1.2.840.10008.1.2.4.100.1"
            | "1.2.840.10008.1.2.4.101"
            | "1.2.840.10008.1.2.4.101.1"
            | "1.2.840.10008.1.2.4.102"
            | "1.2.840.10008.1.2.4.102.1"
            | "1.2.840.10008.1.2.4.103"
            | "1.2.840.10008.1.2.4.103.1"
            | "1.2.840.10008.1.2.4.104"
            | "1.2.840.10008.1.2.4.104.1"
            | "1.2.840.10008.1.2.4.105"
            | "1.2.840.10008.1.2.4.105.1"
            | "1.2.840.10008.1.2.4.106"
            | "1.2.840.10008.1.2.4.106.1"
            | "1.2.840.10008.1.2.4.107"
            | "1.2.840.10008.1.2.4.108"
    )
}
pub fn is_jpeg_ls_transfer_syntax(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81"
    )
}
pub fn is_jpeg_2000_transfer_syntax(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        "1.2.840.10008.1.2.4.90"
            | "1.2.840.10008.1.2.4.91"
            | "1.2.840.10008.1.2.4.92"
            | "1.2.840.10008.1.2.4.93"
            | "1.2.840.10008.1.2.4.201"
            | "1.2.840.10008.1.2.4.202"
            | "1.2.840.10008.1.2.4.203"
    )
}
pub fn is_jpeg_xl_transfer_syntax(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        "1.2.840.10008.1.2.4.110" | "1.2.840.10008.1.2.4.111" | "1.2.840.10008.1.2.4.112"
    )
}
