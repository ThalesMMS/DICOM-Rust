#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::error::{Error, ErrorKind};
use super::fileset::{FileId, FileRecord, FileSet, Options, validate_authored_file_id};
use super::keys::directory_keys_equal;
use super::pathrules;
use super::snapshot::{
    DirectorySnapshot, encoded_selection_matches_record, read_directory_snapshot,
};
use super::{RECORD_TYPE_MRDR, fileset::exact_file_id_spelling};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileSetUpdateMode {
    #[default]
    Unset,
    Append,
    Refresh,
    Other(u8),
}
#[derive(Clone, Debug, Default)]
pub struct FileSetUpdateOptions {
    pub mode: FileSetUpdateMode,
    pub source_options: Options,
    pub legacy_mrdr: bool,
    pub max_bytes: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileSetUpdateReport {
    pub new_files: i64,
    pub unchanged_files: i64,
    pub refreshed_files: i64,
    pub logical_references: i64,
    pub mrdr_records: i64,
}
pub fn parse_file_set(path: &str) -> Result<FileSet, Error> {
    parse_file_set_with_options(path, Options::default())
}
fn check_dicomdir_path(path: &str) -> Result<String, Error> {
    let absolute = match pathrules::abs(path) {
        Ok(a) if pathrules::base(&a) == "DICOMDIR" => a,
        _ => return Err(ErrorKind::InvalidRecord.into()),
    };
    if exact_file_id_spelling(
        &pathrules::dir(&absolute),
        &FileId(vec!["DICOMDIR".to_string()]),
    )
    .is_err()
    {
        return Err(ErrorKind::InvalidRecord.into());
    }
    Ok(absolute)
}
pub fn parse_file_set_with_options(path: &str, options: Options) -> Result<FileSet, Error> {
    let absolute = check_dicomdir_path(path)?;
    let snapshot = read_directory_snapshot(&absolute, &options, options.legacy_mrdr, 64 << 20)?;
    let legacy = options.legacy_mrdr;
    prepare_imported_file_set(
        snapshot,
        &[],
        &FileSetUpdateOptions {
            mode: FileSetUpdateMode::Append,
            source_options: options,
            legacy_mrdr: legacy,
            max_bytes: 0,
        },
    )
    .map(|(set, _)| set)
}
pub fn prepare_file_set_update(
    path: &str,
    ids: &[FileId],
    options: &FileSetUpdateOptions,
) -> Result<(FileSet, FileSetUpdateReport), Error> {
    if !matches!(
        options.mode,
        FileSetUpdateMode::Append | FileSetUpdateMode::Refresh
    ) || options.max_bytes < 0
    {
        return Err(ErrorKind::InvalidOptions.into());
    }
    let mut options = options.clone();
    if options.max_bytes == 0 {
        options.max_bytes = 64 << 20;
    }
    let absolute = check_dicomdir_path(path)?;
    let snapshot = read_directory_snapshot(
        &absolute,
        &options.source_options,
        options.legacy_mrdr,
        options.max_bytes,
    )?;
    prepare_imported_file_set(snapshot, ids, &options)
}
pub(crate) fn prepare_imported_file_set(
    snapshot: DirectorySnapshot,
    ids: &[FileId],
    options: &FileSetUpdateOptions,
) -> Result<(FileSet, FileSetUpdateReport), Error> {
    let mut source = options.source_options.clone();
    if !source.file_set_uid.is_empty() && source.file_set_uid != snapshot.uid
        || !source.file_set_id.is_empty() && source.file_set_id != snapshot.id
        || !source.descriptor_file_id.0.is_empty()
            && source.descriptor_file_id != snapshot.descriptor
        || !source.descriptor_character_set.is_empty()
            && source.descriptor_character_set != snapshot.descriptor_character_set
    {
        return Err(ErrorKind::InvalidOptions.into());
    }
    source.file_set_uid = snapshot.uid.clone();
    source.file_set_id = snapshot.id.clone();
    source.descriptor_file_id = snapshot.descriptor.clone();
    source.descriptor_character_set = snapshot.descriptor_character_set.clone();
    source.legacy_mrdr = options.legacy_mrdr;
    let mut set = FileSet::new(&pathrules::dir(&snapshot.path), source)?;
    if snapshot.key_bytes > set.options.limits.max_total_record_key_bytes {
        return Err(ErrorKind::ResourceLimit.into());
    }
    let snapshot = Rc::new(snapshot);
    set.directory = Some(snapshot.clone());
    if ids.len() as i64 > set.options.limits.max_files {
        return Err(ErrorKind::ResourceLimit.into());
    }
    let mut selected: HashSet<String> = HashSet::new();
    for id in ids {
        validate_authored_file_id(id)?;
        if !selected.insert(id.to_string()) {
            return Err(ErrorKind::InvalidFileId.into());
        }
    }
    let mut report = FileSetUpdateReport {
        logical_references: snapshot.references.len() as i64,
        mrdr_records: snapshot
            .graph
            .flat
            .iter()
            .filter(|&&n| snapshot.graph[n].typ.as_str() == RECORD_TYPE_MRDR)
            .count() as i64,
        ..Default::default()
    };
    let refresh = options.mode == FileSetUpdateMode::Refresh;
    let mut physical: HashMap<String, FileRecord> = HashMap::new();
    for reference in &snapshot.references {
        let key = reference.file_id.to_string();
        let chosen = selected.contains(&key);
        let added = match physical.get(&key) {
            Some(a) => a.clone(),
            None => {
                set.add_inner(&reference.file_id, true, Some(&reference.selection))?;
                let Some(last) = set.records.last() else {
                    return Err(ErrorKind::InvalidRecord.into());
                };
                let added = last.record.clone();
                physical.insert(key.clone(), added.clone());
                if chosen && refresh {
                    report.refreshed_files = report.refreshed_files.saturating_add(1);
                } else {
                    report.unchanged_files = report.unchanged_files.saturating_add(1);
                }
                added
            }
        };
        if added.sop_class_uid != reference.sop_class_uid
            || added.sop_instance_uid != reference.sop_instance_uid
            || added.record_type != reference.record_type
        {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if (!chosen || !refresh)
            && (added.transfer_syntax_uid != reference.transfer_syntax_uid
                || !encoded_selection_matches_record(&reference.selection, &added)
                || !directory_keys_equal(&reference.selection.keys, &added.keys)
                || reference.related_general_sop_class_uids != added.related_general_sop_class_uids)
        {
            return Err(ErrorKind::SourceChanged.into());
        }
    }
    for id in ids {
        if physical.contains_key(&id.to_string()) {
            continue;
        }
        set.add(id)?;
        report.new_files = report.new_files.saturating_add(1);
        report.logical_references = report.logical_references.saturating_add(1);
    }
    if snapshot.key_bytes
        > set
            .options
            .limits
            .max_total_record_key_bytes
            .saturating_sub(set.key_bytes)
    {
        return Err(ErrorKind::ResourceLimit.into());
    }
    let (_, err) = set.validate();
    if let Some(e) = err {
        return Err(e);
    }
    let records = set.records.clone();
    set.revalidate_sources(&records)?;
    if !snapshot.matches() {
        return Err(ErrorKind::SourceChanged.into());
    }
    Ok((set, report))
}
