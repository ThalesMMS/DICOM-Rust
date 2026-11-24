use super::{ChainError, Sentinel};
use crate::object::{Object, Tag, Value, Vr};
use crate::pixel::encapsulated::{self, Format, Limits, Recovery, TableAttribute, TableValue};
use crate::pixel::pixeldata::{PixelData, extract_metadata, extract_view};
use crate::transfer::is_jpeg_2000_transfer_syntax;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub codestreams: Vec<Vec<u8>>,
}
pub trait TargetProvider: Send + Sync {
    fn target(&self, ctx: &crate::net::ul::Ctx, name: &str) -> Result<Target, ChainError>;
}
#[derive(Clone, Debug, Default)]
pub struct StaticTargets(pub HashMap<String, Target>);
impl TargetProvider for StaticTargets {
    fn target(&self, _ctx: &crate::net::ul::Ctx, name: &str) -> Result<Target, ChainError> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| ChainError::sentinel(Sentinel::TargetNotFound))
    }
}
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let elem = obj.get(tag)?;
    let value = match &elem.value {
        Value::Raw(raw) => TableValue::Raw(raw),
        _ => TableValue::Unsupported("core.Value"),
    };
    Some(TableAttribute {
        vr_ov: elem.vr() == Vr::OV,
        value,
    })
}
fn unsupported(detail: &str) -> ChainError {
    ChainError::wrap(Sentinel::UnsupportedTarget, detail)
}
pub fn target_from_object(transfer_syntax_uid: &str, obj: &Object) -> Result<Target, ChainError> {
    if !is_jpeg_2000_transfer_syntax(transfer_syntax_uid) {
        return Err(unsupported("transfer syntax is not JPEG 2000"));
    }
    let pixel =
        extract_view(obj).map_err(|e| ChainError::join_text(Sentinel::UnsupportedTarget, e))?;
    let PixelData::Encapsulated {
        offset_table,
        fragments,
    } = pixel
    else {
        return Err(unsupported("pixel data is not encapsulated"));
    };
    let metadata =
        extract_metadata(obj).map_err(|e| ChainError::join_text(Sentinel::UnsupportedTarget, e))?;
    let frames = metadata.number_of_frames.max(1);
    let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
    let plan = encapsulated::from_fragments(
        offset_table,
        &refs,
        crate::pixel::encapsulated::EotAttributes {
            extended: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE),
            lengths: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
        },
        frames,
        Format::JPEG2000,
        Limits::default(),
        Recovery::default(),
    )
    .map_err(|e| ChainError::join_text(Sentinel::UnsupportedTarget, e))?;
    let mut target = Target {
        codestreams: Vec::with_capacity(plan.len()),
    };
    for i in 0..plan.len() {
        let view = plan
            .frame(i)
            .map_err(|e| ChainError::join_text(Sentinel::UnsupportedTarget, e))?;
        let mut data = view.data.into_owned();
        if view.padded {
            data.pop();
        }
        target.codestreams.push(data);
    }
    Ok(target)
}
pub(crate) fn target_id(target: &Target) -> String {
    let mut h = Sha256::new();
    for codestream in &target.codestreams {
        h.update((codestream.len() as u64).to_be_bytes());
        h.update(codestream);
    }
    let sum = h.finalize();
    sum[..16].iter().map(|b| format!("{b:02x}")).collect()
}
