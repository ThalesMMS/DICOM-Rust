use super::{Error, Limits, ctx_err};
use crate::net::ul::Ctx;
use crate::object::parser::source::MemFile;
use crate::object::read_ext::ReadExtras;
use crate::object::validation_read::read_data_set_with_validation;
use crate::object::{Object, ReadFileOptions, Tag, UNDEFINED_LENGTH, Vr, read_part10_header_mem};
use crate::validation::{
    HookChain, HookDecision, HookPoint, HookRegistration, Mode, ValidationOptions,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
pub(super) struct Part10 {
    pub meta: Object,
    pub dataset: Object,
    pub transfer_syntax_uid: String,
}
const PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
const DOUBLE_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
pub(super) fn read_part10(ctx: &Ctx, data: &[u8], limits: Limits) -> Result<Part10, Error> {
    ctx_err(ctx)?;
    let opts = ReadFileOptions {
        max_total_bytes: limits.total_bytes,
        max_element_bytes: limits.metadata_bytes,
        max_pixel_data_bytes: limits.total_bytes,
        max_sequence_depth: 24,
        max_elements: 100_000,
        max_fragments: 100_000,
        skip_pixel_data: true,
        ..ReadFileOptions::default()
    };
    let header = read_part10_header_mem(&MemFile::new(std::rc::Rc::from(data)), &opts)
        .map_err(|e| Error::Other(e.to_string()))?;
    let remaining = limits.metadata_bytes - header.data_set_offset;
    if remaining <= 0 {
        return Err(Error::Limit);
    }
    let budget = Arc::new(AtomicI64::new(remaining));
    let hook_budget = Arc::clone(&budget);
    let hooks = HookChain::new(vec![HookRegistration {
        name: "xdsi-metadata-budget".into(),
        points: vec![HookPoint::ElementHeaderRead],
        hook: Some(Arc::new(move |event| {
            let mut cost = 12_i64;
            if let Some(h) = &event.header
                && h.vr != Vr::SQ
                && h.length != UNDEFINED_LENGTH
                && h.tag != PIXEL_DATA
                && h.tag != FLOAT_PIXEL_DATA
                && h.tag != DOUBLE_PIXEL_DATA
            {
                cost += i64::from(h.length);
            }
            if hook_budget.fetch_sub(cost, Ordering::Relaxed) - cost < 0 {
                return Err(Error::Limit.to_string().into());
            }
            Ok(HookDecision::default())
        })),
        ..HookRegistration::default()
    }])
    .map_err(|e| Error::Other(e.to_string()))?;
    let opts = ReadFileOptions {
        max_total_bytes: limits.total_bytes - header.data_set_offset,
        ..opts
    };
    let offset = usize::try_from(header.data_set_offset)
        .unwrap_or(data.len())
        .min(data.len());
    let (dataset, _, err) = read_data_set_with_validation(
        &data[offset..],
        true,
        header.transfer_syntax,
        opts,
        &ReadExtras::default(),
        None,
        ValidationOptions {
            mode: Mode::Strict,
            max_findings: 32,
            max_elements: 100_000,
            max_depth: 12,
            hooks: Some(Arc::new(hooks)),
            ..ValidationOptions::default()
        },
    );
    if let Some(err) = err {
        return Err(Error::Other(err.to_string()));
    }
    let Some(dataset) = dataset else {
        return Err(Error::Other("dicom: no data set".into()));
    };
    Ok(Part10 {
        meta: header.meta,
        dataset,
        transfer_syntax_uid: header.transfer_syntax.uid.to_string(),
    })
}
