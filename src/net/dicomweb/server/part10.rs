use super::ServerError;
use crate::object::Tag;
use crate::object::parser::{BulkDataPathStep, MAX_BULK_DATA_PATH_STEPS};
pub fn format_bulk_data_path(path: &[BulkDataPathStep]) -> Result<String, ServerError> {
    if path.is_empty() || path.len() > MAX_BULK_DATA_PATH_STEPS {
        return Err(ServerError::InvalidRequest);
    }
    let mut segments = Vec::with_capacity(path.len());
    for (i, step) in path.iter().enumerate() {
        let tag = format!("{:04X}{:04X}", step.tag.group, step.tag.element);
        let last = i == path.len() - 1;
        if last && step.item_index == -1 {
            segments.push(tag);
        } else if !last && step.item_index >= 0 {
            segments.push(format!("{tag}.{}", step.item_index));
        } else {
            return Err(ServerError::InvalidRequest);
        }
    }
    Ok(segments.join("/"))
}
pub fn parse_bulk_data_path(text: &str) -> Result<Vec<BulkDataPathStep>, ServerError> {
    let segments: Vec<&str> = text.split('/').collect();
    if text.is_empty() || segments.len() > MAX_BULK_DATA_PATH_STEPS {
        return Err(ServerError::InvalidRequest);
    }
    let mut path = Vec::with_capacity(segments.len());
    for (i, segment) in segments.iter().enumerate() {
        let (tag_text, index_text, nested) = match segment.split_once('.') {
            Some((t, n)) => (t, n, true),
            None => (*segment, "", false),
        };
        if nested == (i == segments.len() - 1)
            || tag_text.len() != 8
            || tag_text.to_uppercase() != tag_text
        {
            return Err(ServerError::InvalidRequest);
        }
        let value = crate::dicomjson::sizedparse::parse_uint_base(tag_text, 16, 32)
            .map_err(|_| ServerError::InvalidRequest)?;
        let mut step = BulkDataPathStep {
            tag: Tag::new((value >> 16) as u16, value as u16),
            item_index: -1,
        };
        if nested {
            let index = crate::dicomjson::sizedparse::parse_int(index_text, 64)
                .map_err(|_| ServerError::InvalidRequest)?;
            if index < 0 || index.to_string() != index_text {
                return Err(ServerError::InvalidRequest);
            }
            step.item_index = index;
        }
        if step.tag.group == 0xFFFE || step.tag.group == 0x0002 {
            return Err(ServerError::InvalidRequest);
        }
        path.push(step);
    }
    Ok(path)
}
