use super::mimeheader::{self, trim_space};
use super::response::{
    frame_media_type_matches_transfer_syntax, web_service_transfer_syntax_allowed,
};
use super::server::ServerError;
use crate::object::{Element, Object, Tag, Vr};
pub(crate) const EXPLICIT_VR_LITTLE_ENDIAN_UID: &str = "1.2.840.10008.1.2.1";
const MPEG2_MPML: &str = "1.2.840.10008.1.2.4.100";
const MPEG2_MPHL: &str = "1.2.840.10008.1.2.4.101";
const MPEG4_HP41: &str = "1.2.840.10008.1.2.4.102";
pub(crate) const VIDEO_SOP_CLASSES: [&str; 3] = [
    "1.2.840.10008.5.1.4.1.1.77.1.1.1",
    "1.2.840.10008.5.1.4.1.1.77.1.2.1",
    "1.2.840.10008.5.1.4.1.1.77.1.4.1",
];
pub(crate) const STORE_DOCUMENT_TAG: Tag = Tag {
    group: 0x0042,
    element: 0x0011,
};
pub(crate) const STORE_DOCUMENT_MIME_TAG: Tag = Tag {
    group: 0x0042,
    element: 0x0012,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WholeProfile {
    pub media: &'static str,
    pub sop: &'static str,
    pub modality: &'static str,
    pub video: bool,
}
const DOCUMENT_PROFILES: [WholeProfile; 5] = [
    WholeProfile {
        media: "application/pdf",
        sop: "1.2.840.10008.5.1.4.1.1.104.1",
        modality: "DOC",
        video: false,
    },
    WholeProfile {
        media: "text/xml",
        sop: "1.2.840.10008.5.1.4.1.1.104.2",
        modality: "DOC",
        video: false,
    },
    WholeProfile {
        media: "model/stl",
        sop: "1.2.840.10008.5.1.4.1.1.104.3",
        modality: "M3D",
        video: false,
    },
    WholeProfile {
        media: "model/obj",
        sop: "1.2.840.10008.5.1.4.1.1.104.4",
        modality: "M3D",
        video: false,
    },
    WholeProfile {
        media: "model/mtl",
        sop: "1.2.840.10008.5.1.4.1.1.104.5",
        modality: "M3D",
        video: false,
    },
];
pub(crate) fn store_whole_profile(media: &str) -> Option<WholeProfile> {
    if let Some(p) = DOCUMENT_PROFILES.iter().find(|p| p.media == media) {
        return Some(*p);
    }
    match media {
        "video/mp4" => Some(WholeProfile {
            media: "video/mp4",
            sop: "",
            modality: "",
            video: true,
        }),
        "video/mpeg" => Some(WholeProfile {
            media: "video/mpeg",
            sop: "",
            modality: "",
            video: true,
        }),
        _ => None,
    }
}
fn store_whole_part_parameters(media: &str, params: &[(String, String)]) -> bool {
    let Some(p) = store_whole_profile(media) else {
        return false;
    };
    params.iter().all(|(name, v)| match name.as_str() {
        "transfer-syntax" => {
            if p.video {
                !(media == "video/mp4" && v != MPEG4_HP41
                    || media == "video/mpeg" && v != MPEG2_MPML && v != MPEG2_MPHL)
            } else {
                v == EXPLICIT_VR_LITTLE_ENDIAN_UID
            }
        }
        "charset" => media == "text/xml" && v.eq_ignore_ascii_case("utf-8"),
        _ => false,
    })
}
pub(crate) fn valid_bulk_part_parameters(params: &[(String, String)]) -> bool {
    params.iter().all(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-syntax")
            && crate::object::transfer::normalize_uid(trim_space(value))
                == EXPLICIT_VR_LITTLE_ENDIAN_UID
    })
}
pub(crate) fn store_bulk_part_parameters(media: &str, params: &[(String, String)]) -> bool {
    if media == "application/octet-stream" {
        return valid_bulk_part_parameters(params);
    }
    if media != "image/jpeg" && media != "image/jls" {
        return store_whole_part_parameters(media, params);
    }
    params.iter().all(|(k, v)| {
        k == "transfer-syntax"
            && web_service_transfer_syntax_allowed(v)
            && frame_media_type_matches_transfer_syntax(media, v)
    })
}
pub(crate) fn compressed_store_content_type(
    text: &str,
) -> Result<(String, Vec<(String, String)>), ServerError> {
    match mimeheader::parse_media_type(text) {
        Ok(parsed) => {
            let media = mimeheader::to_lower(&parsed.media_type);
            if store_bulk_part_parameters(&media, &parsed.params) {
                Ok((media, parsed.params))
            } else {
                Err(ServerError::Unsupported)
            }
        }
        Err(_) => Err(ServerError::Unsupported),
    }
}
pub(crate) fn store_metadata_frame_count(obj: &Object) -> Result<i64, ServerError> {
    let tag = Tag {
        group: 0x0028,
        element: 0x0008,
    };
    if obj.get(tag).is_none() {
        return Ok(1);
    }
    match obj.get_int(tag) {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(ServerError::InvalidRequest),
    }
}
pub(crate) fn store_single_string(obj: &Object, tag: Tag) -> Option<String> {
    let values = obj.get_strings(tag)?;
    if values.len() != 1 {
        return None;
    }
    Some(trim_space(&values[0]).to_string())
}
pub(crate) fn store_required_string(obj: &Object, tag: Tag) -> Result<String, ServerError> {
    match store_single_string(obj, tag) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(ServerError::InvalidRequest),
    }
}
pub(crate) fn validate_store_whole_metadata(
    obj: &Object,
    element: &Element,
    p: &WholeProfile,
) -> Result<(), ServerError> {
    let sop = store_required_string(
        obj,
        Tag {
            group: 0x0008,
            element: 0x0016,
        },
    )?;
    if element.vr() != Vr::OB {
        return Err(ServerError::InvalidRequest);
    }
    if p.video {
        if element.tag() != Tag::PIXEL_DATA || !VIDEO_SOP_CLASSES.contains(&sop.as_str()) {
            return Err(ServerError::InvalidRequest);
        }
        return Ok(());
    }
    if element.tag() != STORE_DOCUMENT_TAG || sop != p.sop {
        return Err(ServerError::InvalidRequest);
    }
    let text = store_required_string(obj, STORE_DOCUMENT_MIME_TAG)?;
    match mimeheader::parse_media_type(&text) {
        Ok(parsed)
            if mimeheader::to_lower(&parsed.media_type) == p.media && parsed.params.is_empty() =>
        {
            Ok(())
        }
        _ => Err(ServerError::InvalidRequest),
    }
}
