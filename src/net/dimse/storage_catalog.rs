#![forbid(unsafe_code)]
use crate::dictionary::uid;
use crate::net::ul::{PresentationContext, SopClassCommonExtendedNegotiationItem};
use std::collections::HashSet;
pub const STORAGE_SERVICE_CLASS_UID: &str = "1.2.840.10008.4.2";
const LIBRARY_STORAGE_SOP_CLASS_UIDS: [&str; 42] = [
    "1.2.840.10008.5.1.4.1.1.1",
    "1.2.840.10008.5.1.4.1.1.2",
    "1.2.840.10008.5.1.4.1.1.2.1",
    "1.2.840.10008.5.1.4.1.1.4",
    "1.2.840.10008.5.1.4.1.1.4.1",
    "1.2.840.10008.5.1.4.1.1.6.1",
    "1.2.840.10008.5.1.4.1.1.7",
    "1.2.840.10008.5.1.4.1.1.7.1",
    "1.2.840.10008.5.1.4.1.1.7.2",
    "1.2.840.10008.5.1.4.1.1.7.3",
    "1.2.840.10008.5.1.4.1.1.7.4",
    "1.2.840.10008.5.1.4.1.1.12.1",
    "1.2.840.10008.5.1.4.1.1.12.2",
    "1.2.840.10008.5.1.4.1.1.20",
    "1.2.840.10008.5.1.4.1.1.128",
    "1.2.840.10008.5.1.4.1.1.1.1",
    "1.2.840.10008.5.1.4.1.1.1.1.1",
    "1.2.840.10008.5.1.4.1.1.66.4",
    "1.2.840.10008.5.1.4.1.1.66.7",
    "1.2.840.10008.5.1.4.1.1.88.22",
    "1.2.840.10008.5.1.4.1.1.88.33",
    "1.2.840.10008.5.1.4.1.1.88.34",
    "1.2.840.10008.5.1.4.1.1.88.59",
    "1.2.840.10008.5.1.4.1.1.481.2",
    "1.2.840.10008.5.1.4.1.1.30",
    "1.2.840.10008.5.1.4.1.1.104.1",
    "1.2.840.10008.5.1.4.1.1.104.2",
    "1.2.840.10008.5.1.4.1.1.104.3",
    "1.2.840.10008.5.1.4.1.1.104.4",
    "1.2.840.10008.5.1.4.1.1.104.5",
    "1.2.840.10008.5.1.4.1.1.77.1.6",
    "1.2.840.10008.5.1.4.1.1.77.1.9",
    "1.2.840.10008.5.1.4.1.1.91.1",
    "1.2.840.10008.5.1.4.1.1.11.1",
    "1.2.840.10008.5.1.4.1.1.481.3",
    "1.2.840.10008.5.1.4.1.1.66.1",
    "1.2.840.10008.5.1.4.1.1.66.3",
    "1.2.840.10008.5.1.4.1.1.11.6",
    "1.2.840.10008.5.1.4.1.1.11.7",
    "1.2.840.10008.5.1.4.1.1.11.9",
    "1.2.840.10008.5.1.4.1.1.11.10",
    "1.2.840.10008.5.1.4.1.1.11.11",
];
fn library_storage_sop_class_uids() -> Vec<&'static str> {
    let mut out = LIBRARY_STORAGE_SOP_CLASS_UIDS.to_vec();
    out.extend(crate::waveform::supported_storage_sop_class_uids());
    out
}
pub fn default_storage_sop_class_uids() -> Vec<String> {
    let mut out: Vec<String> = uid::storage_sop_class_uids()
        .into_iter()
        .map(String::from)
        .collect();
    for sop in library_storage_sop_class_uids() {
        if !uid::is_storage(sop) {
            out.push(sop.to_string());
        }
    }
    out
}
pub fn default_c_get_storage_sop_class_uids() -> Vec<String> {
    uid::storage_scu_subset(&library_storage_sop_class_uids())
}
pub fn related_general_storage_sop_classes(
    sop_class_uid: &str,
    related: &[&str],
) -> SopClassCommonExtendedNegotiationItem {
    SopClassCommonExtendedNegotiationItem {
        sop_class_uid: sop_class_uid.to_string(),
        service_class_uid: STORAGE_SERVICE_CLASS_UID.to_string(),
        related_general_sop_class_uids: related.iter().map(|s| s.to_string()).collect(),
    }
}
pub(crate) fn common_extended_negotiation_for(
    items: &[SopClassCommonExtendedNegotiationItem],
    contexts: &[PresentationContext],
) -> Vec<SopClassCommonExtendedNegotiationItem> {
    let proposed: HashSet<&str> = contexts
        .iter()
        .map(|c| c.abstract_syntax_uid.as_str())
        .collect();
    items
        .iter()
        .filter(|i| proposed.contains(i.sop_class_uid.as_str()))
        .cloned()
        .collect()
}
