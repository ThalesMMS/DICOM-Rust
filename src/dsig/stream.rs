use super::{
    Error, Kind, TAG_CERTIFICATE_OF_SIGNER, TAG_CERTIFIED_TIMESTAMP, TAG_CERTIFIED_TIMESTAMP_TYPE,
    TAG_MAC_PARAMETERS_SEQUENCE, TAG_SIGNATURE,
};
use crate::object::transfer::{self, IMPLICIT_VR_LITTLE_ENDIAN};
use crate::object::writer::try_encode_element_explicit_le;
use crate::object::{Element, Syntax, Tag, Value, Vr};
use crate::textfmt::quote;
const TAG_ITEM: Tag = Tag {
    group: 0xFFFE,
    element: 0xE000,
};
const TAG_ITEM_DELIMITATION: Tag = Tag {
    group: 0xFFFE,
    element: 0xE00D,
};
const TAG_SEQUENCE_DELIMITATION: Tag = Tag {
    group: 0xFFFE,
    element: 0xE0DD,
};
const TAG_LENGTH_TO_END: Tag = Tag {
    group: 0x0008,
    element: 0x0001,
};
const TAG_DATA_SET_TRAILING_PADDING: Tag = Tag {
    group: 0xFFFC,
    element: 0xFFFC,
};
pub(super) fn mac_transfer_syntax(uid: &str) -> Result<Syntax, Error> {
    match transfer::lookup(uid) {
        Some(syntax)
            if syntax.is_little_endian()
                && !syntax.deflated
                && (syntax.explicit_vr || syntax.uid == IMPLICIT_VR_LITTLE_ENDIAN.uid) =>
        {
            Ok(syntax)
        }
        _ => Err(Error::new(
            Kind::Unsupported,
            format!("MAC Calculation Transfer Syntax {}", quote(uid)),
        )),
    }
}
pub(super) fn sorted_elements(elements: &[Element]) -> Vec<&Element> {
    let mut out: Vec<&Element> = elements.iter().collect();
    out.sort_by_key(|e| (e.tag().group, e.tag().element));
    out
}
pub(super) fn excluded(element: &Element) -> bool {
    let tag = element.tag();
    tag.element == 0
        || tag == TAG_LENGTH_TO_END
        || tag.group < 0x0008
        || tag.group == 0xFFFA
        || tag == TAG_MAC_PARAMETERS_SEQUENCE
        || tag == TAG_DATA_SET_TRAILING_PADDING
        || tag == TAG_ITEM_DELIMITATION
        || contains_un(element)
}
fn contains_un(element: &Element) -> bool {
    if element.vr() == Vr::UN {
        return true;
    }
    match &element.value {
        Value::Sequence(items) => items
            .iter()
            .any(|item| item.elements.iter().any(contains_un)),
        _ => false,
    }
}
struct StreamWriter {
    out: Vec<u8>,
    syntax: Syntax,
}
impl StreamWriter {
    fn tag(&mut self, tag: Tag) {
        self.out.extend_from_slice(&tag.group.to_le_bytes());
        self.out.extend_from_slice(&tag.element.to_le_bytes());
    }
    fn header(&mut self, tag: Tag, vr: Vr) {
        self.tag(tag);
        if self.syntax.explicit_vr {
            self.out.extend_from_slice(vr.as_str().as_bytes());
            self.out.extend_from_slice(&[0, 0]);
        }
    }
    fn element(&mut self, element: &Element) -> Result<(), Error> {
        match &element.value {
            Value::Sequence(items) => {
                self.header(element.tag(), Vr::SQ);
                for item in items {
                    self.tag(TAG_ITEM);
                    for nested in sorted_elements(&item.elements) {
                        if !excluded(nested) {
                            self.element(nested)?;
                        }
                    }
                }
                self.tag(TAG_SEQUENCE_DELIMITATION);
            }
            Value::Fragments {
                offset_table,
                fragments,
            } => {
                if !self.syntax.encapsulated {
                    return Err(Error::bare(Kind::EncapsulatedMacSyntax));
                }
                self.header(element.tag(), element.vr());
                self.tag(TAG_ITEM);
                self.out.extend_from_slice(offset_table);
                for fragment in fragments {
                    self.tag(TAG_ITEM);
                    self.out.extend_from_slice(fragment);
                }
                self.tag(TAG_SEQUENCE_DELIMITATION);
            }
            _ => {
                let encoded = try_encode_element_explicit_le(element)
                    .map_err(|e| Error::other(e.to_string()))?;
                if self.syntax.explicit_vr {
                    self.out.extend_from_slice(&encoded);
                } else {
                    let header = if element.vr().uses_long_explicit_length() {
                        12
                    } else {
                        8
                    };
                    let value = encoded.get(header..).unwrap_or_default();
                    self.out.extend_from_slice(&encoded[..4.min(encoded.len())]);
                    self.out
                        .extend_from_slice(&(value.len() as u32).to_le_bytes());
                    self.out.extend_from_slice(value);
                }
            }
        }
        Ok(())
    }
}
pub(super) fn signed_stream(
    data_set: &[Element],
    signed: &[Tag],
    signature_item: &[Element],
    syntax: Syntax,
) -> Result<Vec<u8>, Error> {
    let mut stream = StreamWriter {
        out: Vec::new(),
        syntax,
    };
    let encode = |stream: &mut StreamWriter, element: &Element| {
        stream
            .element(element)
            .map_err(|e| e.prefixed(format!("dicom dsig: encode {}", element.tag())))
    };
    for element in sorted_elements(data_set) {
        if signed.contains(&element.tag()) && !excluded(element) {
            encode(&mut stream, element)?;
        }
    }
    for element in sorted_elements(signature_item) {
        let tag = element.tag();
        if tag == TAG_CERTIFICATE_OF_SIGNER
            || tag == TAG_SIGNATURE
            || tag == TAG_CERTIFIED_TIMESTAMP_TYPE
            || tag == TAG_CERTIFIED_TIMESTAMP
        {
            continue;
        }
        encode(&mut stream, element)?;
    }
    Ok(stream.out)
}
