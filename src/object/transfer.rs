#![forbid(unsafe_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Syntax {
    pub uid: &'static str,
    pub name: &'static str,
    pub explicit_vr: bool,
    pub big_endian: bool,
    pub supported: bool,
    pub encapsulated: bool,
    pub deflated: bool,
}
impl Syntax {
    pub fn is_little_endian(&self) -> bool {
        !self.big_endian
    }
}
pub const IMPLICIT_VR_LITTLE_ENDIAN: Syntax = Syntax {
    uid: "1.2.840.10008.1.2",
    name: "Implicit VR Little Endian",
    explicit_vr: false,
    big_endian: false,
    supported: true,
    encapsulated: false,
    deflated: false,
};
pub const EXPLICIT_VR_LITTLE_ENDIAN: Syntax = Syntax {
    uid: "1.2.840.10008.1.2.1",
    name: "Explicit VR Little Endian",
    explicit_vr: true,
    big_endian: false,
    supported: true,
    encapsulated: false,
    deflated: false,
};
pub fn normalize_uid(uid: &str) -> &str {
    uid.trim_end_matches([' ', '\0'])
}
pub fn lookup(uid: &str) -> Option<Syntax> {
    let uid = normalize_uid(uid);
    super::transfer_tables::REGISTRY
        .iter()
        .find(|s| s.uid == uid)
        .copied()
}
pub fn is_legacy(uid: &str) -> bool {
    matches!(
        normalize_uid(uid),
        "ACR-NEMA-Implicit-VR-Big-Endian" | "1.2.840.113619.5.2"
    )
}
pub const IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA: Syntax = Syntax {
    uid: "ACR-NEMA-Implicit-VR-Big-Endian",
    name: "ACR-NEMA Implicit VR Big Endian",
    explicit_vr: false,
    big_endian: true,
    supported: true,
    encapsulated: false,
    deflated: false,
};
pub const PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA: Syntax = Syntax {
    uid: "1.2.840.113619.5.2",
    name: "Private GE Implicit VR Little Endian with Big Endian Pixel Data",
    explicit_vr: false,
    big_endian: false,
    supported: true,
    encapsulated: false,
    deflated: false,
};
#[doc(hidden)]
pub fn legacy_syntax(uid: &str) -> Option<Syntax> {
    match normalize_uid(uid) {
        "ACR-NEMA-Implicit-VR-Big-Endian" => Some(IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA),
        "1.2.840.113619.5.2" => Some(PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA),
        _ => None,
    }
}
