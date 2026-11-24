#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod elem;
mod error;
mod fileset;
mod graph;
mod keys;
mod keys_sequences;
pub(crate) mod pathrules;
mod platform;
mod profile;
mod record_types;
mod snapshot;
mod update;
mod write_limit;
mod writer;
use crate::object::{Object, Tag};
use elem::{record_in_use, uint32_value};
pub use error::{Error, ErrorKind};
pub use fileset::{
    Diagnostic, DiagnosticCode, EntryPolicy, FileId, FileRecord, FileSet, Limits, Options, Query,
    ScanOptions, ScanReport, Statistics, ValidationReport, new_file_id, parse_file_id,
};
pub use profile::{Profile, ProfileError};
pub use record_types::RecordType;
use record_types::standard_file_record_type;
use std::collections::{HashMap, HashSet};
use std::fs::File;
pub use update::{
    FileSetUpdateMode, FileSetUpdateOptions, FileSetUpdateReport, parse_file_set,
    parse_file_set_with_options, prepare_file_set_update,
};
pub use writer::{PublicationGuard, WriteOptions, WriteReport, commit_dicomdir, write_dicomdir};
pub(crate) const TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD: Tag = Tag::new(0x0004, 0x1200);
pub(crate) const TAG_DIRECTORY_RECORD_SEQUENCE: Tag = Tag::new(0x0004, 0x1220);
pub(crate) const TAG_OFFSET_NEXT_DIRECTORY_RECORD: Tag = Tag::new(0x0004, 0x1400);
pub(crate) const TAG_RECORD_IN_USE_FLAG: Tag = Tag::new(0x0004, 0x1410);
pub(crate) const TAG_OFFSET_LOWER_LEVEL_RECORD: Tag = Tag::new(0x0004, 0x1420);
pub(crate) const TAG_DIRECTORY_RECORD_TYPE: Tag = Tag::new(0x0004, 0x1430);
pub(crate) const TAG_REFERENCED_FILE_ID: Tag = Tag::new(0x0004, 0x1500);
pub(crate) const TAG_MRDR_DIRECTORY_RECORD_OFFSET: Tag = Tag::new(0x0004, 0x1504);
pub(crate) const TAG_NUMBER_OF_REFERENCES: Tag = Tag::new(0x0004, 0x1600);
pub(crate) const RECORD_TYPE_MRDR: &str = "MRDR";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reference {
    pub file_id: Vec<String>,
    pub typ: String,
}
pub fn references(obj: &Object) -> Vec<Reference> {
    let Some(records) = obj.get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE) else {
        return Vec::new();
    };
    if !obj.has(TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD) {
        return flat_references(&records);
    }
    let Some(root_offset) = uint32_value(obj, TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD) else {
        return Vec::new();
    };
    if root_offset == 0 {
        return Vec::new();
    }
    let mut by_offset: HashMap<u32, usize> = HashMap::new();
    let mut ambiguous: HashSet<u32> = HashSet::new();
    for (i, record) in records.iter().enumerate() {
        let Some(offset) = record.item_offset() else {
            continue;
        };
        if !(0..=i64::from(u32::MAX)).contains(&offset) {
            continue;
        }
        let encoded = offset as u32;
        if by_offset.contains_key(&encoded) {
            ambiguous.insert(encoded);
            by_offset.remove(&encoded);
            continue;
        }
        if !ambiguous.contains(&encoded) {
            by_offset.insert(encoded, i);
        }
    }
    if by_offset.is_empty() {
        return flat_references(&records);
    }
    let mut refs = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let mut pending = vec![root_offset];
    while let Some(offset) = pending.pop() {
        if offset == 0 || !visited.insert(offset) {
            continue;
        }
        let Some(&index) = by_offset.get(&offset) else {
            continue;
        };
        let Some(record) = records.get(index) else {
            continue;
        };
        if let Some(next) = uint32_value(record, TAG_OFFSET_NEXT_DIRECTORY_RECORD)
            && next != 0
        {
            pending.push(next);
        }
        if let Some(lower) = uint32_value(record, TAG_OFFSET_LOWER_LEVEL_RECORD)
            && lower != 0
        {
            pending.push(lower);
        }
        if !record_in_use(record) {
            continue;
        }
        if let Some(r) = reference_from_record(record) {
            refs.push(r);
        } else if let Some(target) = uint32_value(record, TAG_MRDR_DIRECTORY_RECORD_OFFSET)
            && target != 0
            && let Some(physical) = by_offset.get(&target).and_then(|&i| records.get(i))
            && record_in_use(physical)
            && physical
                .get_string(TAG_DIRECTORY_RECORD_TYPE)
                .unwrap_or_default()
                == RECORD_TYPE_MRDR
            && let Some(r) = reference_from_record_mrdr(record, physical)
        {
            refs.push(r);
        }
    }
    refs
}
fn flat_references(records: &[Object]) -> Vec<Reference> {
    records
        .iter()
        .filter(|r| record_in_use(r))
        .filter_map(reference_from_record)
        .collect()
}
fn reference_from_record(record: &Object) -> Option<Reference> {
    let file_id = referenced_file_id(record)?;
    let typ = record
        .get_string(TAG_DIRECTORY_RECORD_TYPE)
        .unwrap_or_default();
    if typ == RECORD_TYPE_MRDR {
        return None;
    }
    Some(Reference { file_id, typ })
}
pub(crate) fn referenced_file_id(record: &Object) -> Option<Vec<String>> {
    let values = record.get_strings(TAG_REFERENCED_FILE_ID)?;
    let first = values.first()?.trim();
    if first.is_empty() || first.starts_with('\\') {
        return None;
    }
    Some(values)
}
fn reference_from_record_mrdr(logical: &Object, physical: &Object) -> Option<Reference> {
    if logical.has(TAG_REFERENCED_FILE_ID) {
        return None;
    }
    let file_id = referenced_file_id(physical)?;
    let typ = logical
        .get_string(TAG_DIRECTORY_RECORD_TYPE)
        .unwrap_or_default();
    if !standard_file_record_type(&typ) {
        return None;
    }
    Some(Reference { file_id, typ })
}
pub(crate) fn trim_space(s: &str) -> &str {
    s.trim()
}
pub fn resolve(root: &str, reference: &Reference) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in &reference.file_id {
        if !safe_file_id_component(component) {
            return String::new();
        }
        for part in component.split('\\') {
            let trimmed = trim_space(part);
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut all: Vec<&str> = vec![root];
    all.extend(parts.iter().map(String::as_str));
    let joined = pathrules::join(&all);
    let Ok(root_abs) = pathrules::abs(root) else {
        return String::new();
    };
    let Ok(path_abs) = pathrules::abs(&joined) else {
        return String::new();
    };
    let rel = match pathrules::rel(&root_abs, &path_abs) {
        Ok(r) => r,
        Err(_) => return String::new(),
    };
    if rel == ".." || rel.starts_with(&format!("..{}", pathrules::sep_str())) {
        return String::new();
    }
    match has_redirect_component(&root_abs, &rel) {
        Ok(false) => pathrules::clean(&joined),
        _ => String::new(),
    }
}
fn has_redirect_component(root_abs: &str, relative: &str) -> std::io::Result<bool> {
    let mut current = root_abs.to_string();
    for part in relative.split(pathrules::sep_str()) {
        current = pathrules::join(&[&current, part]);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Ok(true);
                }
            }
            Err(e) if pathrules::is_not_exist(&e) => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}
fn safe_file_id_component(component: &str) -> bool {
    let component = trim_space(component);
    if component.is_empty() {
        return true;
    }
    let mut has_part = false;
    for part in component.split('\\') {
        let part = trim_space(part);
        if part.is_empty() {
            continue;
        }
        has_part = true;
        if part == "."
            || part == ".."
            || is_windows_reserved_file_name(part)
            || part.contains(['/', ':', '\0'])
            || pathrules::is_abs(part)
            || !pathrules::volume_name(part).is_empty()
        {
            return false;
        }
    }
    if !has_part {
        return true;
    }
    !component.starts_with('\\')
        && !component.contains('/')
        && !pathrules::is_abs(component)
        && pathrules::volume_name(component).is_empty()
}
pub(crate) fn is_windows_reserved_file_name(part: &str) -> bool {
    let mut name = part.trim_end_matches([' ', '.']);
    if let Some(ext) = name.find('.') {
        name = name[..ext].trim_end_matches([' ', '.']);
    }
    let name = crate::textfmt::to_upper(name);
    match name.as_str() {
        "NUL" | "CON" | "PRN" | "AUX" => return true,
        _ => {}
    }
    let b = name.as_bytes();
    matches!(b, [b'C', b'O', b'M', d] | [b'L', b'P', b'T', d] if (b'1'..=b'9').contains(d))
}
pub fn referenced_paths(dicomdir_path: &str) -> Result<Vec<String>, Error> {
    let path = pathrules::abs(dicomdir_path).map_err(|e| Error::other(e.to_string()))?;
    let file = crate::object::open_file(&path).map_err(|(_, e)| Error::other(e.to_string()))?;
    let root = pathrules::dir(&path);
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for r in references(&file.dataset) {
        let p = resolve(&root, &r);
        if p.is_empty() || !seen.insert(p.clone()) {
            continue;
        }
        paths.push(p);
    }
    Ok(paths)
}
pub fn open_referenced_file(
    root: impl AsRef<std::path::Path>,
    reference: &Reference,
) -> Result<(File, String), Error> {
    let root = root.as_ref().to_string_lossy();
    let root: &str = &root;
    let root_abs = pathrules::abs(root).map_err(|e| Error::other(e.to_string()))?;
    match std::fs::symlink_metadata(&root_abs) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        _ => return Err(Error::other("dicomdir: invalid media root")),
    }
    let resolved = resolve(&root_abs, reference);
    if resolved.is_empty() {
        return Err(Error::other("dicomdir: invalid referenced file ID"));
    }
    let relative = match pathrules::rel(&root_abs, &resolved) {
        Ok(r) => r,
        Err(e) => return Err(Error::other(e)),
    };
    if relative == "."
        || relative == ".."
        || relative.starts_with(&format!("..{}", pathrules::sep_str()))
    {
        return Err(Error::other(
            "dicomdir: referenced file is outside the media root",
        ));
    }
    match platform::open_relative_file(&root_abs, &relative) {
        Ok(f) => Ok((f, resolved)),
        Err(e) => Err(Error::open(&relative, e)),
    }
}
pub(crate) const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
