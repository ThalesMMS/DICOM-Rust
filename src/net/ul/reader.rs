#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::aetitle::normalize_ae_title;
use super::common_ext::validate_common_extended_negotiation_item;
use super::error::{Error, Kind, Result};
use super::pdu::*;
use super::writer::{
    validate_presentation_context_ids_proposed, validate_presentation_context_ids_result,
};
pub(crate) const PDU_READ_GROWTH: usize = 64 << 10;
pub(crate) fn resolve_max_association_pdu(configured: u32) -> u32 {
    if configured == 0 {
        DEFAULT_MAX_ASSOCIATION_PDU
    } else {
        configured
    }
}
pub trait ByteRead {
    fn read_avail(&mut self, buf: &mut [u8]) -> Result<usize>;
}
impl ByteRead for &[u8] {
    fn read_avail(&mut self, buf: &mut [u8]) -> Result<usize> {
        let Some((head, tail)) = self.split_at_checked(buf.len().min(self.len())) else {
            return Ok(0);
        };
        if let Some(dst) = buf.get_mut(..head.len()) {
            dst.copy_from_slice(head);
        }
        *self = tail;
        Ok(head.len())
    }
}
pub(crate) fn read_full<R: ByteRead + ?Sized>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    let mut n = 0;
    while n < buf.len() {
        match r.read_avail(buf.get_mut(n..).unwrap_or_default()) {
            Ok(0) => {
                return Err(if n == 0 {
                    Error::eof()
                } else {
                    Error::unexpected_eof()
                });
            }
            Ok(k) => n = n.saturating_add(k).min(buf.len()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub fn read_pdu<R: ByteRead + ?Sized>(r: &mut R, max_pdu_length: u32) -> Result<Pdu> {
    read_pdu_with_association_limit(r, max_pdu_length, DEFAULT_MAX_ASSOCIATION_PDU)
}
pub fn read_pdu_with_association_limit<R: ByteRead + ?Sized>(
    r: &mut R,
    max_pdu_length: u32,
    max_association_pdu: u32,
) -> Result<Pdu> {
    if max_pdu_length < MINIMUM_PDU_SIZE {
        return Err(Error::wrap(
            Kind::InvalidPduSize,
            format!("max PDU length {max_pdu_length} is smaller than {MINIMUM_PDU_SIZE}"),
        ));
    }
    let mut header = [0u8; PDU_HEADER_SIZE as usize];
    read_full(r, &mut header).map_err(|e| e.prefixed("dicom ul: read PDU header"))?;
    let [pdu_type, _, l0, l1, l2, l3] = header;
    let length = u32::from_be_bytes([l0, l1, l2, l3]);
    validate_read_pdu_length(
        pdu_type,
        length,
        max_pdu_length,
        resolve_max_association_pdu(max_association_pdu),
    )?;
    let data =
        read_pdu_body(r, length as usize).map_err(|e| e.prefixed("dicom ul: read PDU body"))?;
    parse_pdu_body(pdu_type, &data)
}
fn read_pdu_body<R: ByteRead + ?Sized>(r: &mut R, length: usize) -> Result<Vec<u8>> {
    let mut data: Vec<u8> = Vec::with_capacity(length.min(PDU_READ_GROWTH));
    let mut cap = length.min(PDU_READ_GROWTH);
    while data.len() < length {
        if data.len() == cap {
            cap = length.min(data.len().saturating_add(data.len().max(PDU_READ_GROWTH)));
            data.reserve(cap.saturating_sub(data.len()));
        }
        let start = data.len();
        data.resize(cap, 0);
        let mut n = start;
        let mut err = None;
        while n < cap {
            match r.read_avail(data.get_mut(n..cap).unwrap_or_default()) {
                Ok(0) => {
                    err = Some(if n == start {
                        Error::eof()
                    } else {
                        Error::unexpected_eof()
                    });
                    break;
                }
                Ok(k) => n = n.saturating_add(k).min(cap),
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        data.truncate(n);
        if let Some(mut e) = err {
            if e.is(Kind::Eof) && !data.is_empty() {
                e = Error::unexpected_eof();
            }
            return Err(e);
        }
    }
    Ok(data)
}
#[doc(hidden)]
pub fn parse_pdu_body(pdu_type: u8, data: &[u8]) -> Result<Pdu> {
    match pdu_type {
        PDU_ASSOCIATE_RQ => read_association_rq(data).map(Pdu::AssociateRq),
        PDU_ASSOCIATE_AC => read_association_ac(data).map(Pdu::AssociateAc),
        PDU_ASSOCIATE_RJ => read_association_rj(data).map(Pdu::AssociateRj),
        PDU_DATA_TF => read_pdata_tf(data).map(Pdu::PData),
        PDU_RELEASE_RQ => {
            if data.len() != 4 {
                return Err(Error::wrap(
                    Kind::InvalidPduField,
                    format!("A-RELEASE-RQ length {}", data.len()),
                ));
            }
            Ok(Pdu::ReleaseRq)
        }
        PDU_RELEASE_RP => {
            if data.len() != 4 {
                return Err(Error::wrap(
                    Kind::InvalidPduField,
                    format!("A-RELEASE-RP length {}", data.len()),
                ));
            }
            Ok(Pdu::ReleaseRp)
        }
        PDU_ABORT => read_abort_rq(data).map(Pdu::Abort),
        other => Ok(Pdu::Unknown(UnknownPdu {
            pdu_type: other,
            data: data.to_vec(),
        })),
    }
}
pub(crate) fn validate_read_pdu_length(
    pdu_type: u8,
    pdu_length: u32,
    max_pdu_length: u32,
    max_association_pdu: u32,
) -> Result<()> {
    let limit = if pdu_type == PDU_ASSOCIATE_RQ || pdu_type == PDU_ASSOCIATE_AC {
        max_association_pdu
    } else {
        max_pdu_length
    };
    if pdu_length > limit {
        return Err(Error::wrap(
            Kind::PduTooLarge,
            format!("length {pdu_length} exceeds max {limit}"),
        ));
    }
    Ok(())
}
pub(crate) fn lossy_text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}
fn trimmed(data: &[u8]) -> String {
    lossy_text(data).trim().to_string()
}
struct Fixed {
    protocol_version: u16,
    called: String,
    calling: String,
}
fn read_association_fixed(data: &[u8]) -> Result<(Fixed, &[u8])> {
    let Some((head, variable)) = data.split_first_chunk::<68>() else {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!("association fixed fields need 68 bytes, got {}", data.len()),
        ));
    };
    let fixed = Fixed {
        protocol_version: u16::from_be_bytes([head[0], head[1]]),
        called: normalize_ae_title(&lossy_text(&head[4..20])).to_string(),
        calling: normalize_ae_title(&lossy_text(&head[20..36])).to_string(),
    };
    Ok((fixed, variable))
}
fn read_association_rq(data: &[u8]) -> Result<AssociationRq> {
    let (fixed, variable) = read_association_fixed(data)?;
    let items = read_variable_items(variable, false)?;
    let contexts = match items.contexts {
        Contexts::Proposed(c) => c,
        Contexts::Results(_) => {
            return Err(Error::wrap(
                Kind::InvalidPduItem,
                "presentation context request item type",
            ));
        }
    };
    Ok(AssociationRq {
        protocol_version: fixed.protocol_version,
        called_ae_title: fixed.called,
        calling_ae_title: fixed.calling,
        application_context_name: items.app_context,
        presentation_contexts: contexts,
        user_info: items.user_info,
    })
}
fn read_association_ac(data: &[u8]) -> Result<AssociationAc> {
    let (fixed, variable) = read_association_fixed(data)?;
    let items = read_variable_items(variable, true)?;
    let contexts = match items.contexts {
        Contexts::Results(c) => c,
        Contexts::Proposed(_) => {
            return Err(Error::wrap(
                Kind::InvalidPduItem,
                "presentation context accept item type",
            ));
        }
    };
    Ok(AssociationAc {
        protocol_version: fixed.protocol_version,
        called_ae_title: fixed.called,
        calling_ae_title: fixed.calling,
        application_context_name: items.app_context,
        presentation_contexts: contexts,
        user_info: items.user_info,
    })
}
fn read_association_rj(data: &[u8]) -> Result<AssociationRj> {
    let &[_, result, source, reason] = data else {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!("A-ASSOCIATE-RJ length {}", data.len()),
        ));
    };
    let pdu = AssociationRj {
        result,
        source,
        reason,
    };
    if !valid_reject(pdu.result, pdu.source, pdu.reason) {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!(
                "invalid A-ASSOCIATE-RJ result/source/reason {}/{}/{}",
                pdu.result, pdu.source, pdu.reason
            ),
        ));
    }
    Ok(pdu)
}
enum Contexts {
    Proposed(Vec<PresentationContextProposed>),
    Results(Vec<PresentationContextResult>),
}
struct VariableItems {
    app_context: String,
    contexts: Contexts,
    user_info: Option<Vec<UserItem>>,
}
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
}
impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Cursor { data }
    }
    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }
    pub(crate) fn take(&mut self, n: usize) -> &'a [u8] {
        let n = n.min(self.data.len());
        let (head, tail) = self.data.split_at(n);
        self.data = tail;
        head
    }
    pub(crate) fn u16(&mut self) -> u16 {
        match self.take(2) {
            &[a, b] => u16::from_be_bytes([a, b]),
            _ => 0,
        }
    }
}
fn read_item<'a>(r: &mut Cursor<'a>) -> Result<(u8, &'a [u8])> {
    let available = r.len();
    let &[item_type, _, l0, l1] = r.take(4) else {
        return Err(Error::wrap(
            Kind::InvalidPduItem,
            format!("item header needs 4 bytes, got {available}"),
        ));
    };
    let length = u16::from_be_bytes([l0, l1]) as usize;
    if length > r.len() {
        return Err(Error::wrap(
            Kind::InvalidPduItem,
            format!(
                "item type 0x{item_type:02x} length {length} exceeds remaining {}",
                r.len()
            ),
        ));
    }
    Ok((item_type, r.take(length)))
}
fn read_variable_items(data: &[u8], for_ac: bool) -> Result<VariableItems> {
    let mut proposed = Vec::new();
    let mut results = Vec::new();
    let mut app_context = String::new();
    let mut have_app_context = false;
    let mut user_info = None;
    let mut r = Cursor::new(data);
    while r.len() > 0 {
        let (item_type, item) = read_item(&mut r)?;
        match item_type {
            ITEM_APPLICATION_CONTEXT => {
                if have_app_context {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "duplicate application-context",
                    ));
                }
                app_context = trimmed(item);
                if app_context.is_empty() {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "empty application-context",
                    ));
                }
                have_app_context = true;
            }
            ITEM_PRESENTATION_CONTEXT_RQ => {
                if for_ac {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "request presentation context in A-ASSOCIATE-AC",
                    ));
                }
                proposed.push(read_presentation_context_proposed(item)?);
            }
            ITEM_PRESENTATION_CONTEXT_AC => {
                if !for_ac {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "result presentation context in A-ASSOCIATE-RQ",
                    ));
                }
                results.push(read_presentation_context_result(item)?);
            }
            ITEM_USER_INFORMATION => {
                if user_info.is_some() {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "duplicate user-information",
                    ));
                }
                let items = read_user_information(item)?;
                if items.is_empty() {
                    return Err(Error::wrap(Kind::InvalidPduItem, "empty user-information"));
                }
                user_info = Some(items);
            }
            _ => {}
        }
    }
    if !have_app_context {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "application context name",
        ));
    }
    if for_ac {
        validate_presentation_context_ids_result(&results)?;
        return Ok(VariableItems {
            app_context,
            contexts: Contexts::Results(results),
            user_info,
        });
    }
    validate_presentation_context_ids_proposed(&proposed, valid_peer_presentation_context_id)?;
    Ok(VariableItems {
        app_context,
        contexts: Contexts::Proposed(proposed),
        user_info,
    })
}
fn read_presentation_context_proposed(data: &[u8]) -> Result<PresentationContextProposed> {
    let Some((&[id, _, _, _], sub_items)) = data.split_first_chunk::<4>() else {
        return Err(Error::wrap(
            Kind::InvalidPduItem,
            "presentation context request fixed fields",
        ));
    };
    let mut pc = PresentationContextProposed {
        id,
        ..Default::default()
    };
    if !valid_peer_presentation_context_id(pc.id) {
        return Err(Error::wrap(Kind::InvalidPcid, pc.id));
    }
    let mut r = Cursor::new(sub_items);
    while r.len() > 0 {
        let (item_type, item) = read_item(&mut r)?;
        match item_type {
            ITEM_ABSTRACT_SYNTAX => {
                if !pc.abstract_syntax_uid.is_empty() {
                    return Err(Error::wrap(
                        Kind::InvalidPduItem,
                        "multiple abstract syntax sub-items",
                    ));
                }
                pc.abstract_syntax_uid = trimmed(item);
            }
            ITEM_TRANSFER_SYNTAX => pc.transfer_syntax_uids.push(trimmed(item)),
            _ => {}
        }
    }
    if pc.abstract_syntax_uid.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "abstract syntax"));
    }
    if pc.transfer_syntax_uids.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
    }
    Ok(pc)
}
fn read_presentation_context_result(data: &[u8]) -> Result<PresentationContextResult> {
    let Some((&[id, _, result, _], sub_items)) = data.split_first_chunk::<4>() else {
        return Err(Error::wrap(
            Kind::InvalidPduItem,
            "presentation context accept fixed fields",
        ));
    };
    let mut pc = PresentationContextResult {
        id,
        result,
        transfer_syntax_uid: String::new(),
    };
    if !valid_peer_presentation_context_id(pc.id) {
        return Err(Error::wrap(Kind::InvalidPcid, pc.id));
    }
    if pc.result > PRESENTATION_CONTEXT_TRANSFER_SYNTAXES_NOT_SUPPORTED {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!("presentation context result {}", pc.result),
        ));
    }
    let mut r = Cursor::new(sub_items);
    while r.len() > 0 {
        let (item_type, item) = read_item(&mut r)?;
        if item_type == ITEM_TRANSFER_SYNTAX {
            if !pc.transfer_syntax_uid.is_empty() {
                return Err(Error::wrap(
                    Kind::InvalidPduItem,
                    "multiple accepted transfer syntaxes",
                ));
            }
            pc.transfer_syntax_uid = trimmed(item);
        }
    }
    if pc.result == PRESENTATION_CONTEXT_ACCEPTANCE && pc.transfer_syntax_uid.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
    }
    Ok(pc)
}
fn single_user_information_item(item_type: u8) -> bool {
    matches!(
        item_type,
        SUB_ITEM_MAX_LENGTH
            | SUB_ITEM_IMPLEMENTATION_CLASS_UID
            | SUB_ITEM_ASYNCHRONOUS_OPERATIONS
            | SUB_ITEM_IMPLEMENTATION_VERSION_NAME
            | SUB_ITEM_USER_IDENTITY
            | SUB_ITEM_USER_IDENTITY_RESPONSE
    )
}
fn read_user_information(data: &[u8]) -> Result<Vec<UserItem>> {
    let mut r = Cursor::new(data);
    let mut items = Vec::new();
    let mut seen = [false; 256];
    while r.len() > 0 {
        let (item_type, item) = read_item(&mut r)?;
        if single_user_information_item(item_type)
            && let Some(seen) = seen.get_mut(usize::from(item_type))
        {
            if *seen {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    format!("duplicate user information sub-item 0x{item_type:02X}"),
                ));
            }
            *seen = true;
        }
        let parsed = match item_type {
            SUB_ITEM_MAX_LENGTH => {
                let &[a, b, c, d] = item else {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!("maximum length item length {}", item.len()),
                    ));
                };
                UserItem::MaxLength(u32::from_be_bytes([a, b, c, d]))
            }
            SUB_ITEM_IMPLEMENTATION_CLASS_UID => UserItem::ImplementationClassUid(trimmed(item)),
            SUB_ITEM_IMPLEMENTATION_VERSION_NAME => {
                UserItem::ImplementationVersionName(trimmed(item))
            }
            SUB_ITEM_ASYNCHRONOUS_OPERATIONS => {
                let &[a, b, c, d] = item else {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!("asynchronous operations window item length {}", item.len()),
                    ));
                };
                UserItem::AsynchronousOperations(AsynchronousOperationsWindow {
                    maximum_invoked: u16::from_be_bytes([a, b]),
                    maximum_performed: u16::from_be_bytes([c, d]),
                })
            }
            SUB_ITEM_SOP_CLASS_EXTENDED_NEGOTIATION => {
                let Some((&[h0, h1], rest)) = item.split_first_chunk::<2>() else {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!("SOP class extended negotiation length {}", item.len()),
                    ));
                };
                let sop_len = u16::from_be_bytes([h0, h1]) as usize;
                let Some((uid, data)) = rest.split_at_checked(sop_len) else {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!(
                            "SOP class UID length {sop_len} exceeds item length {}",
                            item.len()
                        ),
                    ));
                };
                UserItem::ExtendedNegotiation(SopClassExtendedNegotiationItem {
                    sop_class_uid: trimmed(uid),
                    data: data.to_vec(),
                })
            }
            SUB_ITEM_SOP_CLASS_COMMON_EXTENDED_NEGOTIATION => {
                UserItem::CommonExtendedNegotiation(read_common_extended_negotiation(item)?)
            }
            SUB_ITEM_ROLE_SELECTION => UserItem::RoleSelection(read_role_selection(item)?),
            SUB_ITEM_USER_IDENTITY => UserItem::UserIdentity(read_user_identity(item)?),
            SUB_ITEM_USER_IDENTITY_RESPONSE => {
                let Some((&[h0, h1], response)) = item.split_first_chunk::<2>() else {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!("user identity response item length {}", item.len()),
                    ));
                };
                let response_len = u16::from_be_bytes([h0, h1]) as usize;
                if response_len != response.len() {
                    return Err(Error::wrap(
                        Kind::InvalidUserItem,
                        format!(
                            "server response length {response_len} does not match remaining {}",
                            response.len()
                        ),
                    ));
                }
                UserItem::UserIdentityResponse(response.to_vec())
            }
            other => UserItem::Unknown(UnknownUserItem {
                item_type: other,
                data: item.to_vec(),
            }),
        };
        items.push(parsed);
    }
    Ok(items)
}
fn read_role_selection(data: &[u8]) -> Result<RoleSelectionItem> {
    let Some((&[h0, h1], rest)) = data.split_first_chunk::<2>().filter(|_| data.len() >= 4) else {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!("role selection item length {}", data.len()),
        ));
    };
    let sop_len = u16::from_be_bytes([h0, h1]) as usize;
    let Some((uid, roles)) = rest
        .split_at_checked(sop_len)
        .filter(|(_, roles)| roles.len() >= 2)
    else {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "role selection SOP class UID length {sop_len} exceeds item length {}",
                data.len()
            ),
        ));
    };
    let &[scu, scp] = roles else {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "role selection item length {} does not match SOP class UID length {sop_len}",
                data.len()
            ),
        ));
    };
    if scu > 1 || scp > 1 {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!("role selection role values {scu}/{scp}"),
        ));
    }
    Ok(RoleSelectionItem {
        sop_class_uid: trimmed(uid),
        scu_role: scu == 1,
        scp_role: scp == 1,
    })
}
fn read_user_identity(data: &[u8]) -> Result<UserIdentityItem> {
    let Some((&[identity_type, positive, p0, p1], rest)) =
        data.split_first_chunk::<4>().filter(|_| data.len() >= 6)
    else {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!("user identity item length {}", data.len()),
        ));
    };
    let primary_len = u16::from_be_bytes([p0, p1]) as usize;
    let Some((primary, (&[s0, s1], secondary))) = rest
        .split_at_checked(primary_len)
        .and_then(|(primary, after)| Some((primary, after.split_first_chunk::<2>()?)))
    else {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "primary field length {primary_len} exceeds item length {}",
                data.len()
            ),
        ));
    };
    let secondary_len = u16::from_be_bytes([s0, s1]) as usize;
    if secondary_len != secondary.len() {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "secondary field length {secondary_len} does not match remaining {}",
                secondary.len()
            ),
        ));
    }
    Ok(UserIdentityItem {
        identity_type,
        positive_response_requested: positive != 0,
        primary_field: primary.to_vec(),
        secondary_field: secondary.to_vec(),
    })
}
fn read_pdata_tf(data: &[u8]) -> Result<PDataTf> {
    if data.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            "P-DATA-TF must contain at least one P-DATA value",
        ));
    }
    let mut r = Cursor::new(data);
    let mut values = Vec::new();
    while r.len() > 0 {
        let available = r.len();
        let short = || {
            Error::wrap(
                Kind::InvalidPduField,
                format!("P-DATA value needs at least 6 bytes, got {available}"),
            )
        };
        if available < 6 {
            return Err(short());
        }
        let &[b0, b1, b2, b3] = r.take(4) else {
            return Err(short());
        };
        let item_length = u32::from_be_bytes([b0, b1, b2, b3]);
        if item_length < 2 {
            return Err(Error::wrap(
                Kind::InvalidPduField,
                format!("P-DATA value item length {item_length}"),
            ));
        }
        if item_length as usize > r.len() {
            return Err(Error::wrap(
                Kind::InvalidPduField,
                format!(
                    "P-DATA value item length {item_length} exceeds remaining {}",
                    r.len()
                ),
            ));
        }
        let &[pc_id, flags] = r.take(2) else {
            return Err(short());
        };
        if !valid_peer_presentation_context_id(pc_id) {
            return Err(Error::wrap(Kind::InvalidPcid, pc_id));
        }
        values.push(PDataValue {
            presentation_context_id: pc_id,
            is_command: flags & 0x01 != 0,
            is_last: flags & 0x02 != 0,
            data: r.take((item_length as usize).saturating_sub(2)).to_vec(),
        });
    }
    Ok(PDataTf { values })
}
fn read_abort_rq(data: &[u8]) -> Result<AbortRq> {
    let &[_, _, source, reason] = data else {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!("A-ABORT length {}", data.len()),
        ));
    };
    let pdu = AbortRq { source, reason };
    if !valid_abort(pdu.source, pdu.reason) {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!(
                "invalid A-ABORT source/reason {}/{}",
                pdu.source, pdu.reason
            ),
        ));
    }
    Ok(pdu)
}
fn read_common_u16(r: &mut Cursor<'_>, field: &str) -> Result<u16> {
    if r.len() < 2 {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!("SOP class common extended negotiation {field} truncated"),
        ));
    }
    Ok(r.u16())
}
fn read_u16_length_uid(r: &mut Cursor<'_>, field: &str) -> Result<String> {
    let length = read_common_u16(r, &format!("{field} length"))? as usize;
    if length > r.len() {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "SOP class common extended negotiation {field} length {length} exceeds remaining {}",
                r.len()
            ),
        ));
    }
    let value = trimmed(r.take(length));
    Ok(value.trim_end_matches('\0').to_string())
}
fn read_common_extended_negotiation(data: &[u8]) -> Result<SopClassCommonExtendedNegotiationItem> {
    let mut r = Cursor::new(data);
    let sop_class_uid = read_u16_length_uid(&mut r, "SOP class UID")?;
    let service_class_uid = read_u16_length_uid(&mut r, "service class UID")?;
    let related_length =
        read_common_u16(&mut r, "related general SOP class identification length")? as usize;
    if related_length > r.len() {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "related general SOP class identification length {related_length} exceeds remaining {}",
                r.len()
            ),
        ));
    }
    let mut related = Cursor::new(r.take(related_length));
    let mut related_uids = Vec::new();
    while related.len() > 0 {
        related_uids.push(read_u16_length_uid(
            &mut related,
            "related general SOP class UID",
        )?);
    }
    let item = SopClassCommonExtendedNegotiationItem {
        sop_class_uid,
        service_class_uid,
        related_general_sop_class_uids: related_uids,
    };
    validate_common_extended_negotiation_item(&item)?;
    Ok(item)
}
