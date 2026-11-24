#![forbid(unsafe_code)]
use super::common_ext::validate_common_extended_negotiation_item;
use super::error::{Error, Kind, Result};
use super::pdu::*;
use std::io::Write;
pub fn write_pdu<W: Write + ?Sized>(w: &mut W, pdu: &Pdu) -> Result<()> {
    let encoded = encode_pdu(pdu)?;
    w.write_all(&encoded)
        .map_err(|e| super::conn::io_error(&e, "write", None).prefixed("dicom ul: write PDU"))
}
#[doc(hidden)]
pub fn encode_pdu(pdu: &Pdu) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; PDU_HEADER_SIZE as usize];
    let pdu_type = write_pdu_body(&mut buf, pdu)?;
    let body_length = buf.len() - PDU_HEADER_SIZE as usize;
    if body_length as u64 > u32::MAX as u64 {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!("PDU body length {body_length} exceeds uint32"),
        ));
    }
    buf[0] = pdu_type;
    buf[2..6].copy_from_slice(&(body_length as u32).to_be_bytes());
    Ok(buf)
}
fn write_pdu_body(buf: &mut Vec<u8>, pdu: &Pdu) -> Result<u8> {
    match pdu {
        Pdu::AssociateRq(rq) => write_association_rq(buf, rq).map(|_| PDU_ASSOCIATE_RQ),
        Pdu::AssociateAc(ac) => write_association_ac(buf, ac).map(|_| PDU_ASSOCIATE_AC),
        Pdu::AssociateRj(rj) => {
            if !valid_reject(rj.result, rj.source, rj.reason) {
                return Err(Error::wrap(
                    Kind::InvalidPduField,
                    format!(
                        "invalid A-ASSOCIATE-RJ result/source/reason {}/{}/{}",
                        rj.result, rj.source, rj.reason
                    ),
                ));
            }
            buf.extend_from_slice(&[0, rj.result, rj.source, rj.reason]);
            Ok(PDU_ASSOCIATE_RJ)
        }
        Pdu::PData(p) => write_pdata_tf(buf, p).map(|_| PDU_DATA_TF),
        Pdu::ReleaseRq => {
            buf.extend_from_slice(&[0, 0, 0, 0]);
            Ok(PDU_RELEASE_RQ)
        }
        Pdu::ReleaseRp => {
            buf.extend_from_slice(&[0, 0, 0, 0]);
            Ok(PDU_RELEASE_RP)
        }
        Pdu::Abort(a) => {
            if !valid_abort(a.source, a.reason) {
                return Err(Error::wrap(
                    Kind::InvalidPduField,
                    format!("invalid A-ABORT source/reason {}/{}", a.source, a.reason),
                ));
            }
            buf.extend_from_slice(&[0, 0, a.source, a.reason]);
            Ok(PDU_ABORT)
        }
        Pdu::Unknown(u) => {
            buf.extend_from_slice(&u.data);
            Ok(u.pdu_type)
        }
    }
}
fn write_association_rq(buf: &mut Vec<u8>, rq: &AssociationRq) -> Result<()> {
    if rq.presentation_contexts.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidPdu,
            "A-ASSOCIATE-RQ requires at least one presentation context",
        ));
    }
    validate_presentation_context_ids_proposed(
        &rq.presentation_contexts,
        valid_presentation_context_id,
    )?;
    let Some(user_info) = &rq.user_info else {
        return Err(Error::wrap(
            Kind::InvalidPdu,
            "A-ASSOCIATE-RQ requires a User Information item",
        ));
    };
    write_association_fixed(
        buf,
        rq.protocol_version,
        &rq.called_ae_title,
        &rq.calling_ae_title,
    )?;
    write_application_context(buf, &rq.application_context_name)?;
    for pc in &rq.presentation_contexts {
        write_presentation_context_proposed(buf, pc)?;
    }
    write_user_information(buf, user_info)
}
fn write_association_ac(buf: &mut Vec<u8>, ac: &AssociationAc) -> Result<()> {
    if ac.presentation_contexts.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidPdu,
            "A-ASSOCIATE-AC requires at least one presentation context",
        ));
    }
    validate_presentation_context_ids_result(&ac.presentation_contexts)?;
    let Some(user_info) = &ac.user_info else {
        return Err(Error::wrap(
            Kind::InvalidPdu,
            "A-ASSOCIATE-AC requires a User Information item",
        ));
    };
    write_association_fixed(
        buf,
        ac.protocol_version,
        &ac.called_ae_title,
        &ac.calling_ae_title,
    )?;
    write_application_context(buf, &ac.application_context_name)?;
    for pc in &ac.presentation_contexts {
        write_presentation_context_result(buf, pc)?;
    }
    write_user_information(buf, user_info)
}
pub(crate) fn validate_presentation_context_ids_proposed(
    contexts: &[PresentationContextProposed],
    valid: fn(u8) -> bool,
) -> Result<()> {
    if contexts.len() > MAX_PRESENTATION_CONTEXTS {
        return Err(Error::wrap(
            Kind::InvalidPcid,
            format!(
                "presentation context count {} exceeds {MAX_PRESENTATION_CONTEXTS}",
                contexts.len()
            ),
        ));
    }
    let mut seen = [false; 256];
    for c in contexts {
        if !valid(c.id) || seen[c.id as usize] {
            return Err(Error::wrap(
                Kind::InvalidPcid,
                format!("duplicate or invalid presentation context ID {}", c.id),
            ));
        }
        seen[c.id as usize] = true;
    }
    Ok(())
}
pub(crate) fn validate_presentation_context_ids_result(
    contexts: &[PresentationContextResult],
) -> Result<()> {
    if contexts.len() > MAX_PRESENTATION_CONTEXTS {
        return Err(Error::wrap(
            Kind::InvalidPcid,
            format!(
                "presentation context count {} exceeds {MAX_PRESENTATION_CONTEXTS}",
                contexts.len()
            ),
        ));
    }
    let mut seen = [false; 256];
    for c in contexts {
        if !valid_peer_presentation_context_id(c.id) || seen[c.id as usize] {
            return Err(Error::wrap(
                Kind::InvalidPcid,
                format!("duplicate or invalid presentation context ID {}", c.id),
            ));
        }
        seen[c.id as usize] = true;
    }
    Ok(())
}
fn write_association_fixed(
    buf: &mut Vec<u8>,
    protocol_version: u16,
    called: &str,
    calling: &str,
) -> Result<()> {
    buf.extend_from_slice(&protocol_version.to_be_bytes());
    buf.extend_from_slice(&[0, 0]);
    write_ae_title(buf, called).map_err(|e| e.prefixed("called AE title"))?;
    write_ae_title(buf, calling).map_err(|e| e.prefixed("calling AE title"))?;
    buf.extend_from_slice(&[0u8; 32]);
    Ok(())
}
fn write_application_context(buf: &mut Vec<u8>, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "application context name",
        ));
    }
    write_item(buf, ITEM_APPLICATION_CONTEXT, name.as_bytes())
}
fn write_presentation_context_proposed(
    buf: &mut Vec<u8>,
    pc: &PresentationContextProposed,
) -> Result<()> {
    if !valid_presentation_context_id(pc.id) {
        return Err(Error::wrap(Kind::InvalidPcid, pc.id));
    }
    if pc.abstract_syntax_uid.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "abstract syntax"));
    }
    if pc.transfer_syntax_uids.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
    }
    let mut item = vec![pc.id, 0, 0, 0];
    write_item(
        &mut item,
        ITEM_ABSTRACT_SYNTAX,
        pc.abstract_syntax_uid.as_bytes(),
    )?;
    for ts in &pc.transfer_syntax_uids {
        if ts.is_empty() {
            return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
        }
        write_item(&mut item, ITEM_TRANSFER_SYNTAX, ts.as_bytes())?;
    }
    write_item(buf, ITEM_PRESENTATION_CONTEXT_RQ, &item)
}
fn write_presentation_context_result(
    buf: &mut Vec<u8>,
    pc: &PresentationContextResult,
) -> Result<()> {
    if !valid_peer_presentation_context_id(pc.id) {
        return Err(Error::wrap(Kind::InvalidPcid, pc.id));
    }
    if pc.result > PRESENTATION_CONTEXT_TRANSFER_SYNTAXES_NOT_SUPPORTED {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!("presentation context result {}", pc.result),
        ));
    }
    if pc.transfer_syntax_uid.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
    }
    let mut item = vec![pc.id, 0, pc.result, 0];
    write_item(
        &mut item,
        ITEM_TRANSFER_SYNTAX,
        pc.transfer_syntax_uid.as_bytes(),
    )?;
    write_item(buf, ITEM_PRESENTATION_CONTEXT_AC, &item)
}
fn write_user_information(buf: &mut Vec<u8>, items: &[UserItem]) -> Result<()> {
    let mut item = Vec::new();
    let mut async_seen = false;
    for user_item in items {
        if matches!(user_item, UserItem::AsynchronousOperations(_)) {
            if async_seen {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    "duplicate asynchronous operations window",
                ));
            }
            async_seen = true;
        }
        write_user_item(&mut item, user_item)?;
    }
    write_item(buf, ITEM_USER_INFORMATION, &item)
}
fn write_user_item(buf: &mut Vec<u8>, item: &UserItem) -> Result<()> {
    match item {
        UserItem::MaxLength(v) => write_item(buf, SUB_ITEM_MAX_LENGTH, &v.to_be_bytes()),
        UserItem::ImplementationClassUid(uid) => {
            write_item(buf, SUB_ITEM_IMPLEMENTATION_CLASS_UID, uid.as_bytes())
        }
        UserItem::ImplementationVersionName(name) => {
            if name.len() > 16 {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    format!(
                        "implementation version name length {} exceeds 16",
                        name.len()
                    ),
                ));
            }
            write_item(buf, SUB_ITEM_IMPLEMENTATION_VERSION_NAME, name.as_bytes())
        }
        UserItem::AsynchronousOperations(w) => {
            let mut data = w.maximum_invoked.to_be_bytes().to_vec();
            data.extend_from_slice(&w.maximum_performed.to_be_bytes());
            write_item(buf, SUB_ITEM_ASYNCHRONOUS_OPERATIONS, &data)
        }
        UserItem::ExtendedNegotiation(e) => {
            if e.sop_class_uid.len() > u16::MAX as usize {
                return Err(Error::wrap(
                    Kind::LengthOverflow,
                    format!(
                        "SOP class UID length {} exceeds uint16",
                        e.sop_class_uid.len()
                    ),
                ));
            }
            let mut data = (e.sop_class_uid.len() as u16).to_be_bytes().to_vec();
            data.extend_from_slice(e.sop_class_uid.as_bytes());
            data.extend_from_slice(&e.data);
            write_item(buf, SUB_ITEM_SOP_CLASS_EXTENDED_NEGOTIATION, &data)
        }
        UserItem::CommonExtendedNegotiation(c) => write_common_extended_negotiation(buf, c),
        UserItem::RoleSelection(r) => {
            if r.sop_class_uid.is_empty() {
                return Err(Error::wrap(Kind::MissingPduField, "SOP class UID"));
            }
            if r.sop_class_uid.len() > u16::MAX as usize {
                return Err(Error::wrap(
                    Kind::LengthOverflow,
                    format!(
                        "SOP class UID length {} exceeds uint16",
                        r.sop_class_uid.len()
                    ),
                ));
            }
            let mut data = (r.sop_class_uid.len() as u16).to_be_bytes().to_vec();
            data.extend_from_slice(r.sop_class_uid.as_bytes());
            data.push(r.scu_role as u8);
            data.push(r.scp_role as u8);
            write_item(buf, SUB_ITEM_ROLE_SELECTION, &data)
        }
        UserItem::UserIdentity(u) => {
            let mut data = vec![u.identity_type, u.positive_response_requested as u8];
            write_u16_length_bytes(&mut data, &u.primary_field)?;
            write_u16_length_bytes(&mut data, &u.secondary_field)?;
            write_item(buf, SUB_ITEM_USER_IDENTITY, &data)
        }
        UserItem::UserIdentityResponse(r) => {
            let mut data = Vec::new();
            write_u16_length_bytes(&mut data, r)?;
            write_item(buf, SUB_ITEM_USER_IDENTITY_RESPONSE, &data)
        }
        UserItem::Unknown(u) => write_item(buf, u.item_type, &u.data),
    }
}
fn write_common_extended_negotiation(
    buf: &mut Vec<u8>,
    item: &SopClassCommonExtendedNegotiationItem,
) -> Result<()> {
    validate_common_extended_negotiation_item(item)?;
    let mut related = Vec::new();
    for uid in &item.related_general_sop_class_uids {
        write_u16_length_bytes(&mut related, uid.as_bytes())?;
    }
    if related.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!(
                "related general SOP class identification length {} exceeds uint16",
                related.len()
            ),
        ));
    }
    let mut data = Vec::new();
    write_u16_length_bytes(&mut data, item.sop_class_uid.as_bytes())?;
    write_u16_length_bytes(&mut data, item.service_class_uid.as_bytes())?;
    write_u16_length_bytes(&mut data, &related)?;
    write_item(buf, SUB_ITEM_SOP_CLASS_COMMON_EXTENDED_NEGOTIATION, &data)
}
fn write_pdata_tf(buf: &mut Vec<u8>, pdata: &PDataTf) -> Result<()> {
    if pdata.values.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "P-DATA-TF without values",
        ));
    }
    for value in &pdata.values {
        if !valid_peer_presentation_context_id(value.presentation_context_id) {
            return Err(Error::wrap(
                Kind::InvalidPcid,
                value.presentation_context_id,
            ));
        }
        if value.data.len() as u64 > u32::MAX as u64 - 2 {
            return Err(Error::wrap(
                Kind::LengthOverflow,
                format!("P-DATA value length {} exceeds uint32", value.data.len()),
            ));
        }
        buf.extend_from_slice(&(value.data.len() as u32 + 2).to_be_bytes());
        buf.push(value.presentation_context_id);
        let mut flags = 0u8;
        if value.is_command {
            flags |= 0x01;
        }
        if value.is_last {
            flags |= 0x02;
        }
        buf.push(flags);
        buf.extend_from_slice(&value.data);
    }
    Ok(())
}
fn write_item(buf: &mut Vec<u8>, item_type: u8, data: &[u8]) -> Result<()> {
    if data.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!(
                "item 0x{item_type:02x} length {} exceeds uint16",
                data.len()
            ),
        ));
    }
    buf.push(item_type);
    buf.push(0);
    buf.extend_from_slice(&(data.len() as u16).to_be_bytes());
    buf.extend_from_slice(data);
    Ok(())
}
fn write_u16_length_bytes(buf: &mut Vec<u8>, data: &[u8]) -> Result<()> {
    if data.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!("length {} exceeds uint16", data.len()),
        ));
    }
    buf.extend_from_slice(&(data.len() as u16).to_be_bytes());
    buf.extend_from_slice(data);
    Ok(())
}
fn write_ae_title(buf: &mut Vec<u8>, title: &str) -> Result<()> {
    if title.len() > 16 {
        return Err(Error::wrap(
            Kind::InvalidAeTitle,
            format!("length {} exceeds 16", title.len()),
        ));
    }
    buf.extend_from_slice(title.as_bytes());
    buf.resize(buf.len() + 16 - title.len(), b' ');
    Ok(())
}
