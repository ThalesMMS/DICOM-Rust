#![forbid(unsafe_code)]
use super::error::{Error, Result};
use super::find::{
    CFindExtendedNegotiation, c_find_extended_negotiation_handler, is_query_retrieve_find_sop_class,
};
use super::retrieve::{
    CRetrieveExtendedNegotiation, is_query_retrieve_retrieve_sop_class,
    parse_c_retrieve_extended_negotiation,
};
use super::storage_catalog::default_storage_sop_class_uids;
use crate::net::ul::{self, Association};
use std::sync::Arc;
pub type StorageLevelOfSupport = u8;
pub const STORAGE_LEVEL_0_SCP: StorageLevelOfSupport = 0;
pub const STORAGE_LEVEL_1_SCP: StorageLevelOfSupport = 1;
pub const STORAGE_LEVEL_2_SCP: StorageLevelOfSupport = 2;
pub const STORAGE_LEVEL_SCU_ONLY: StorageLevelOfSupport = 3;
pub type StorageDigitalSignatureSupport = u8;
pub const STORAGE_SIGNATURE_UNSPECIFIED: StorageDigitalSignatureSupport = 0;
pub const STORAGE_SIGNATURE_LEVEL_1: StorageDigitalSignatureSupport = 1;
pub const STORAGE_SIGNATURE_LEVEL_2: StorageDigitalSignatureSupport = 2;
pub const STORAGE_SIGNATURE_LEVEL_3: StorageDigitalSignatureSupport = 3;
pub type StorageElementCoercion = u8;
pub const STORAGE_DOES_NOT_COERCE: StorageElementCoercion = 0;
pub const STORAGE_MAY_COERCE: StorageElementCoercion = 1;
pub const STORAGE_COERCION_SCU_ONLY: StorageElementCoercion = 2;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageExtendedNegotiation {
    pub level_of_support: StorageLevelOfSupport,
    pub digital_signature_support: StorageDigitalSignatureSupport,
    pub element_coercion: StorageElementCoercion,
}
pub fn storage_scu_extended_negotiation() -> StorageExtendedNegotiation {
    StorageExtendedNegotiation {
        level_of_support: STORAGE_LEVEL_SCU_ONLY,
        digital_signature_support: STORAGE_SIGNATURE_UNSPECIFIED,
        element_coercion: STORAGE_COERCION_SCU_ONLY,
    }
}
pub fn parse_storage_extended_negotiation(data: &[u8]) -> Result<StorageExtendedNegotiation> {
    if data.is_empty() {
        return Err(Error::new(
            "dicom dimse: empty Storage extended negotiation",
        ));
    }
    let mut n = storage_scu_extended_negotiation();
    n.level_of_support = data[0];
    if data.len() > 2 {
        n.digital_signature_support = data[2];
    }
    if data.len() > 4 {
        n.element_coercion = data[4];
    }
    n.validate()?;
    Ok(n)
}
impl StorageExtendedNegotiation {
    fn validate(&self) -> Result<()> {
        if self.level_of_support > STORAGE_LEVEL_SCU_ONLY {
            return Err(Error::new(format!(
                "dicom dimse: Storage level of support {}",
                self.level_of_support
            )));
        }
        if self.digital_signature_support > STORAGE_SIGNATURE_LEVEL_3 {
            return Err(Error::new(format!(
                "dicom dimse: Storage digital signature support {}",
                self.digital_signature_support
            )));
        }
        if self.element_coercion > STORAGE_COERCION_SCU_ONLY {
            return Err(Error::new(format!(
                "dicom dimse: Storage element coercion {}",
                self.element_coercion
            )));
        }
        Ok(())
    }
    pub fn bytes(&self) -> Vec<u8> {
        vec![
            self.level_of_support,
            0,
            self.digital_signature_support,
            0,
            self.element_coercion,
            0,
        ]
    }
    pub fn item(&self, sop_class_uid: &str) -> ul::SopClassExtendedNegotiationItem {
        ul::SopClassExtendedNegotiationItem {
            sop_class_uid: sop_class_uid.to_string(),
            data: self.bytes(),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct ExtendedNegotiationPolicy {
    pub find: Option<CFindExtendedNegotiation>,
    pub retrieve: Option<CRetrieveExtendedNegotiation>,
    pub storage: Option<StorageExtendedNegotiation>,
    pub storage_sop_class_uids: Vec<String>,
}
impl ExtendedNegotiationPolicy {
    pub fn handler(&self) -> ul::ExtendedNegotiationHandler {
        let policy = self.clone();
        let find_handler = policy.find.map(c_find_extended_negotiation_handler);
        let mut storage_classes = policy.storage_sop_class_uids.clone();
        storage_classes.extend(default_storage_sop_class_uids());
        Arc::new(move |item: &ul::SopClassExtendedNegotiationItem| {
            let ignore = ul::ExtendedNegotiationResult {
                action: ul::EXTENDED_NEGOTIATION_ACTION_IGNORE,
                data: Vec::new(),
            };
            if is_query_retrieve_find_sop_class(&item.sop_class_uid) {
                return match &find_handler {
                    Some(handler) => handler(item),
                    None => Ok(ignore),
                };
            }
            if is_query_retrieve_retrieve_sop_class(&item.sop_class_uid) {
                let Some(supported) = policy.retrieve else {
                    return Ok(ignore);
                };
                let Ok(requested) = parse_c_retrieve_extended_negotiation(&item.data) else {
                    return Ok(ignore);
                };
                let answer = [
                    u8::from(requested.relational_retrieval && supported.relational_retrieval),
                    u8::from(
                        requested.enhanced_multi_frame_image_conversion
                            && supported.enhanced_multi_frame_image_conversion,
                    ),
                ];
                let n = item.data.len().min(2);
                return Ok(ul::ExtendedNegotiationResult {
                    action: ul::EXTENDED_NEGOTIATION_ACTION_ACCEPT,
                    data: answer[..n].to_vec(),
                });
            }
            if storage_classes.contains(&item.sop_class_uid) {
                let Some(storage) = policy.storage else {
                    return Ok(ignore);
                };
                if parse_storage_extended_negotiation(&item.data).is_err() {
                    return Ok(ignore);
                }
                return Ok(ul::ExtendedNegotiationResult {
                    action: ul::EXTENDED_NEGOTIATION_ACTION_ACCEPT,
                    data: storage.bytes(),
                });
            }
            Ok(ignore)
        })
    }
}
#[doc(hidden)]
pub fn storage_extended_negotiation_of(
    accepted: &[ul::SopClassExtendedNegotiationItem],
    sop_class_uid: &str,
) -> Option<StorageExtendedNegotiation> {
    let item = accepted.iter().find(|i| i.sop_class_uid == sop_class_uid)?;
    parse_storage_extended_negotiation(&item.data).ok()
}
pub fn accepted_storage_extended_negotiation(
    assoc: Option<&Association>,
    sop_class_uid: &str,
) -> Option<StorageExtendedNegotiation> {
    storage_extended_negotiation_of(&assoc?.accepted_extended_negotiation, sop_class_uid)
}
