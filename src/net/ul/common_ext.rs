#![forbid(unsafe_code)]
use super::error::{Error, Kind, Result};
use super::negotiation::AcceptOptions;
use super::pdu::{SopClassCommonExtendedNegotiationItem, UserItem};
use super::rules::presentation_context_rule;
use crate::textfmt::quote;
use std::collections::HashSet;
use std::sync::Arc;
pub type CommonExtendedNegotiationHandler =
    Arc<dyn Fn(&SopClassCommonExtendedNegotiationItem, &str) -> bool + Send + Sync>;
pub fn accept_related_general_sop_classes() -> CommonExtendedNegotiationHandler {
    Arc::new(|_, _| true)
}
pub(crate) fn validate_common_extended_negotiation_item(
    item: &SopClassCommonExtendedNegotiationItem,
) -> Result<()> {
    if item.sop_class_uid.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "common extended negotiation SOP class UID",
        ));
    }
    if item.service_class_uid.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            format!(
                "common extended negotiation service class UID for SOP class {}",
                quote(&item.sop_class_uid)
            ),
        ));
    }
    let all = [&item.sop_class_uid, &item.service_class_uid]
        .into_iter()
        .chain(item.related_general_sop_class_uids.iter());
    for uid in all {
        if uid.is_empty() {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "empty related general SOP class UID for SOP class {}",
                    quote(&item.sop_class_uid)
                ),
            ));
        }
        if uid.len() > u16::MAX as usize {
            return Err(Error::wrap(
                Kind::LengthOverflow,
                format!(
                    "common extended negotiation UID length {} exceeds uint16",
                    uid.len()
                ),
            ));
        }
    }
    Ok(())
}
pub(crate) fn common_extended_negotiation_from_user_info(
    items: &[UserItem],
) -> Result<Vec<SopClassCommonExtendedNegotiationItem>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let UserItem::CommonExtendedNegotiation(common) = item else {
            continue;
        };
        validate_common_extended_negotiation_item(common)?;
        if !seen.insert(common.sop_class_uid.clone()) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "duplicate common extended negotiation for SOP class {}",
                    quote(&common.sop_class_uid)
                ),
            ));
        }
        out.push(common.clone());
    }
    Ok(out)
}
pub(crate) fn related_general_sop_class_for(
    abstract_syntax_uid: &str,
    common: &[SopClassCommonExtendedNegotiationItem],
    opts: &AcceptOptions,
) -> Option<String> {
    let handler = opts.common_extended_negotiation_handler.as_ref()?;
    for item in common {
        if item.sop_class_uid != abstract_syntax_uid {
            continue;
        }
        for general in &item.related_general_sop_class_uids {
            let ruled = presentation_context_rule(opts, general).is_some();
            if !ruled
                && !opts
                    .supported_abstract_syntaxes
                    .iter()
                    .any(|s| s == general)
            {
                continue;
            }
            if handler(item, general) {
                return Some(general.clone());
            }
        }
        return None;
    }
    None
}
