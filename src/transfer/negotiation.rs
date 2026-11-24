use super::*;
use std::collections::HashSet;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NativeStorePreference {
    #[default]
    SourceFirst = 0,
    ExplicitLittleEndianFirst = 1,
    ImplicitLittleEndianFirst = 2,
}
const PRESERVABLE_COMPRESSED_RECEIVE_SYNTAXES: [Syntax; 19] = [
    ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
    JPEG_BASELINE,
    JPEG_EXTENDED,
    JPEG_LOSSLESS_NON_HIERARCHICAL,
    JPEG_LOSSLESS_SV1,
    JPEG_LS_LOSSLESS,
    JPEG_LS_NEAR_LOSSLESS,
    RLE_LOSSLESS,
    DEFLATED_IMAGE_FRAME_COMPRESSION,
    JPEG_2000_LOSSLESS_ONLY,
    JPEG_2000,
    JPEG_2000_PART2_LOSSLESS,
    JPEG_2000_PART2,
    HTJ2K_LOSSLESS,
    HTJ2K_LOSSLESS_RPCL,
    HTJ2K,
    JPEG_XL_LOSSLESS,
    JPEG_XL_JPEG_RECOMPRESSION,
    JPEG_XL,
];
const EXPLICIT_LE_UID: &str = "1.2.840.10008.1.2.1";
const IMPLICIT_LE_UID: &str = "1.2.840.10008.1.2";
pub fn proposed_store_transfer_syntax_uids(
    source_uid: &str,
    pref: NativeStorePreference,
) -> Vec<String> {
    if !is_native_store_transfer_syntax(source_uid) {
        return normalized_unique_uids([source_uid]);
    }
    match pref {
        NativeStorePreference::ExplicitLittleEndianFirst => {
            normalized_unique_uids([EXPLICIT_LE_UID, source_uid, IMPLICIT_LE_UID])
        }
        NativeStorePreference::ImplicitLittleEndianFirst => {
            normalized_unique_uids([IMPLICIT_LE_UID, source_uid, EXPLICIT_LE_UID])
        }
        NativeStorePreference::SourceFirst => {
            normalized_unique_uids([source_uid, EXPLICIT_LE_UID, IMPLICIT_LE_UID])
        }
    }
}
pub fn receive_transfer_syntax_uids(native_order: &[Syntax]) -> Vec<String> {
    normalized_unique_uids(
        native_order
            .iter()
            .chain(PRESERVABLE_COMPRESSED_RECEIVE_SYNTAXES.iter())
            .map(|s| s.uid.as_ref()),
    )
}
pub fn preservable_compressed_receive_syntaxes() -> Vec<Syntax> {
    PRESERVABLE_COMPRESSED_RECEIVE_SYNTAXES.to_vec()
}
fn is_native_store_transfer_syntax(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        EXPLICIT_LE_UID | IMPLICIT_LE_UID | "1.2.840.10008.1.2.1.99"
    )
}
fn normalized_unique_uids<'a>(uids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for uid in uids {
        let uid = normalize_uid(uid);
        if uid.is_empty() || !seen.insert(uid) {
            continue;
        }
        out.push(uid.to_string());
    }
    out
}
pub fn filter_transfer_syntax_uids<P: AsRef<str>, A: AsRef<str>>(
    proposals: &[P],
    allowlist: &[A],
) -> Vec<String> {
    proposals
        .iter()
        .map(|p| p.as_ref())
        .filter(|p| {
            allowlist.is_empty()
                || allowlist
                    .iter()
                    .any(|a| normalize_uid(p) == normalize_uid(a.as_ref()))
        })
        .map(str::to_string)
        .collect()
}
