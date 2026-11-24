#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{
    text_element, text_values_element, ui_element, uint32_value, ul_element, us_element,
};
use super::error::{Error, ErrorKind};
use super::fileset::{
    Diagnostic, DiagnosticCode, FileRecord, FileSet, Limits, SourceSnapshot, snapshot_file_records,
    validate_exact_file_id_spellings,
};
use super::graph::{Graph, build_directory_hierarchy};
use super::keys::{
    TAG_MEDIA_STORAGE_SOP_CLASS_UID, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID, TAG_SOP_CLASS_UID,
    TAG_SOP_INSTANCE_UID, TAG_TRANSFER_SYNTAX_UID, directory_keys_equal, read_handle,
};
use super::pathrules;
use super::platform::{self, FileIdentity};
use super::profile::{Profile, TAG_ICON_IMAGE_SEQUENCE, profile_rules_for};
use super::record_types::standard_file_record_type;
use super::snapshot::{
    MEDIA_STORAGE_DIRECTORY_STORAGE_UID, TAG_DESCRIPTOR_CHARACTER_SET,
    TAG_FILE_SET_CONSISTENCY_FLAG, TAG_FILE_SET_DESCRIPTOR_FILE_ID, TAG_FILE_SET_ID,
    TAG_OFFSET_LAST_ROOT_DIRECTORY_RECORD, encoded_selection_matches_record,
    merge_imported_directory, planned_directory_key_bytes, planned_directory_record_count,
    validate_directory_data_set, validate_directory_file_set_uid,
};
use super::update::{FileSetUpdateMode, FileSetUpdateOptions, prepare_imported_file_set};
use super::{
    RECORD_TYPE_MRDR, Reference, TAG_DIRECTORY_RECORD_SEQUENCE, TAG_MRDR_DIRECTORY_RECORD_OFFSET,
    TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD, TAG_OFFSET_LOWER_LEVEL_RECORD,
    TAG_OFFSET_NEXT_DIRECTORY_RECORD, open_referenced_file,
};
use crate::object::writer::{WriteFileOptions, write_file_with_options};
use crate::object::{DataSet, Element, File, Object, Value, Vr, read_file};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
pub type PublicationGuard = Arc<dyn Fn(&str) -> Result<(), Error> + Send + Sync>;
#[derive(Clone, Default)]
pub struct WriteOptions {
    pub publication_guard: Option<PublicationGuard>,
    pub legacy_mrdr: bool,
    pub max_bytes: i64,
    pub profile: Profile,
    pub icons: bool,
    pub icon_size: i64,
    pub blank_icon_on_failure: bool,
}
impl std::fmt::Debug for WriteOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteOptions")
            .field("publication_guard", &self.publication_guard.is_some())
            .field("legacy_mrdr", &self.legacy_mrdr)
            .field("max_bytes", &self.max_bytes)
            .field("profile", &self.profile)
            .field("icons", &self.icons)
            .field("icon_size", &self.icon_size)
            .field("blank_icon_on_failure", &self.blank_icon_on_failure)
            .finish()
    }
}
#[derive(Clone, Debug, Default)]
pub struct WriteReport {
    pub bytes_written: i64,
    pub records: i64,
    pub logical_references: i64,
    pub mrdr_records: i64,
    pub sha256: [u8; 32],
    pub icons: i64,
    pub diagnostics: Vec<Diagnostic>,
    pub(crate) revision: u64,
    pub(crate) extra_elements: i64,
}
fn normalize_write_options(options: &WriteOptions) -> Result<i64, Error> {
    if options.max_bytes < 0 || options.icon_size != 0 && !(1..=128).contains(&options.icon_size) {
        return Err(ErrorKind::InvalidOptions.into());
    }
    if !options.profile.is_empty() {
        profile_rules_for(&options.profile)?;
    }
    Ok(if options.max_bytes == 0 {
        64 << 20
    } else {
        options.max_bytes
    })
}
fn count_directory_elements(elements: &[Element]) -> i64 {
    let mut count = elements.len() as i64;
    for e in elements {
        if let Value::Sequence(items) = &e.value {
            for item in items {
                count = count.saturating_add(count_directory_elements(&item.elements));
            }
        }
    }
    count
}
fn merge_level_keys(g: &Graph, leaves: &[usize], patient: bool) -> Vec<Element> {
    let mut order = Vec::new();
    let mut chosen: HashMap<crate::object::Tag, Element> = HashMap::new();
    for &leaf in leaves {
        let extras = &g[leaf].extras;
        let level = if patient {
            &extras.patient
        } else {
            &extras.series
        };
        for e in level {
            let tag = e.tag();
            match chosen.get(&tag) {
                None => {
                    order.push(tag);
                    chosen.insert(tag, e.clone());
                }
                Some(current) => {
                    if current.raw().is_some_and(<[u8]>::is_empty) {
                        chosen.insert(tag, e.clone());
                    }
                }
            }
        }
    }
    order
        .into_iter()
        .filter_map(|t| chosen.remove(&t))
        .collect()
}
pub fn write_dicomdir(
    destination: &mut dyn Write,
    file_set: &FileSet,
    options: &WriteOptions,
) -> Result<WriteReport, Error> {
    let max_bytes = normalize_write_options(options)?;
    let records = snapshot_file_records(&file_set.records);
    let imported = file_set.directory.as_deref();
    if let Some(d) = imported
        && !d.matches()
    {
        return Err(ErrorKind::SourceChanged.into());
    }
    if let Some(d) = imported
        && d.has_mrdr()
        && !options.legacy_mrdr
    {
        return Err(ErrorKind::LegacyMrdrRequiresOptIn.into());
    }
    file_set.validate_descriptor()?;
    let limits = file_set.options.limits;
    planned_directory_key_bytes(imported, &records, limits.max_total_record_key_bytes)?;
    planned_directory_record_count(imported, &records, limits.max_directory_records)?;
    file_set
        .validate_records_snapshot(&records)
        .map_err(|pair| pair.1)?;
    file_set.revalidate_sources(&records)?;
    let (extras, mut diagnostics, _) = file_set.record_extras(&records, options)?;
    let mut graph = build_directory_hierarchy(&records)?;
    if let Some(d) = imported {
        graph = merge_imported_directory(d, &graph)?;
    }
    if graph.flat.len() as i64 > limits.max_directory_records {
        return Err(ErrorKind::ResourceLimit.into());
    }
    for &n in &graph.flat.clone() {
        if graph[n].children.is_empty() {
            let key = graph[n].record.file_id.to_string();
            graph[n].extras = extras.get(&key).cloned().unwrap_or_default();
        }
    }
    let mut extra_elements = 0i64;
    let mut icons = 0i64;
    for &n in &graph.flat.clone() {
        match graph[n].typ.as_str() {
            "PATIENT" => {
                let leaves = graph.leaf_nodes(n);
                let merged = merge_level_keys(&graph, &leaves, true);
                extra_elements = extra_elements.saturating_add(count_directory_elements(&merged));
                graph[n].extras.patient = merged;
            }
            "SERIES" => {
                let leaves = graph.leaf_nodes(n);
                let merged = merge_level_keys(&graph, &leaves, false);
                extra_elements = extra_elements.saturating_add(count_directory_elements(&merged));
                graph[n].extras.series = merged;
            }
            "STUDY" => {}
            _ => {
                let node = &graph[n];
                extra_elements = extra_elements
                    .saturating_add(count_directory_elements(&node.extras.image))
                    .saturating_add(count_directory_elements(&node.record.keys.elements));
            }
        }
        let node = &graph[n];
        extra_elements =
            extra_elements.saturating_add(count_directory_elements(&node.imported.elements));
        let mrdr = node.mrdr.map(|_| 0);
        if super::graph::directory_record_data_set(node, 0, 0, mrdr)
            .elements
            .iter()
            .any(|e| e.tag() == TAG_ICON_IMAGE_SEQUENCE)
        {
            icons = icons.saturating_add(1);
        }
    }
    let encoded = encode_directory_two_pass(
        &mut graph,
        &file_set.uid,
        &file_set.id,
        &file_set.descriptor.0,
        &file_set.descriptor_character_set,
        max_bytes,
        &limits,
    )?;
    file_set.revalidate_sources(&records)?;
    if let Some(d) = imported
        && !d.matches()
    {
        return Err(ErrorKind::SourceChanged.into());
    }
    match destination.write_all(&encoded) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::WriteZero => {
            return Err(Error::join(ErrorKind::DicomdirWrite, ErrorKind::ShortWrite));
        }
        Err(_) => return Err(Error::redacted(ErrorKind::DicomdirWrite)),
    }
    let mut logical = 0i64;
    let mut mrdr = 0i64;
    for &n in &graph.flat {
        let t = graph[n].typ.as_str();
        if t == RECORD_TYPE_MRDR {
            mrdr = mrdr.saturating_add(1);
        } else if standard_file_record_type(t) {
            logical = logical.saturating_add(1);
        }
    }
    if mrdr > 0 {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::UNSUPPORTED,
            source_index: 0,
            message: "retired MRDR compatibility graph emitted".to_string(),
        });
    }
    Ok(WriteReport {
        bytes_written: encoded.len() as i64,
        records: graph.flat.len() as i64,
        logical_references: logical,
        mrdr_records: mrdr,
        sha256: Sha256::digest(&encoded).into(),
        icons,
        diagnostics,
        revision: file_set.revision,
        extra_elements,
    })
}
fn encode_directory_two_pass(
    graph: &mut Graph,
    uid: &str,
    id: &str,
    descriptor: &[String],
    descriptor_character_set: &str,
    max_bytes: i64,
    admission: &Limits,
) -> Result<Vec<u8>, Error> {
    let pass_one = encode_directory(
        graph,
        uid,
        id,
        descriptor,
        descriptor_character_set,
        max_bytes,
    )?;
    let offsets = encoded_directory_offsets(&pass_one, graph.flat.len())?;
    for (&n, &offset) in graph.flat.clone().iter().zip(&offsets) {
        graph[n].offset = offset;
    }
    let pass_two = encode_directory(
        graph,
        uid,
        id,
        descriptor,
        descriptor_character_set,
        max_bytes,
    )?;
    match encoded_directory_offsets(&pass_two, graph.flat.len()) {
        Ok(second) if second == offsets => {}
        _ => return Err(Error::redacted(ErrorKind::DicomdirWrite)),
    }
    validate_encoded_directory(&pass_two, graph, admission)?;
    Ok(pass_two)
}
fn encode_directory(
    graph: &Graph,
    uid: &str,
    id: &str,
    descriptor: &[String],
    descriptor_character_set: &str,
    max_bytes: i64,
) -> Result<Vec<u8>, Error> {
    let mut elements = vec![
        text_element(TAG_FILE_SET_ID, Vr::CS, id),
        ul_element(
            TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD,
            graph.offset(graph.roots.first().copied()),
        ),
        ul_element(
            TAG_OFFSET_LAST_ROOT_DIRECTORY_RECORD,
            graph.offset(graph.roots.last().copied()),
        ),
        us_element(TAG_FILE_SET_CONSISTENCY_FLAG, 0),
    ];
    if !descriptor.is_empty() {
        elements.push(text_values_element(
            TAG_FILE_SET_DESCRIPTOR_FILE_ID,
            Vr::CS,
            descriptor.to_vec(),
        ));
        if !descriptor_character_set.is_empty() {
            elements.push(text_element(
                TAG_DESCRIPTOR_CHARACTER_SET,
                Vr::CS,
                descriptor_character_set,
            ));
        }
    }
    let items: Vec<DataSet> = graph.flat.iter().map(|&n| graph.record_item(n)).collect();
    elements.push(super::elem::sequence_element(
        TAG_DIRECTORY_RECORD_SEQUENCE,
        items,
    ));
    let file = File {
        preamble: Vec::new(),
        meta: Object::from_elements(vec![
            ui_element(
                TAG_MEDIA_STORAGE_SOP_CLASS_UID,
                MEDIA_STORAGE_DIRECTORY_STORAGE_UID,
            ),
            ui_element(TAG_MEDIA_STORAGE_SOP_INSTANCE_UID, uid),
            ui_element(
                TAG_TRANSFER_SYNTAX_UID,
                crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid,
            ),
        ]),
        dataset: Object::from_elements(elements),
        transfer_syntax: crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    let encoded = write_file_with_options(
        &file,
        &WriteFileOptions {
            omit_reconciled_data_set_sop_uids: true,
            ..Default::default()
        },
    )
    .map_err(|_| Error::redacted(ErrorKind::DicomdirWrite))?;
    if encoded.len() as i64 > max_bytes {
        return Err(super::write_limit::limit_error(&encoded, max_bytes));
    }
    Ok(encoded)
}
fn encoded_directory_offsets(encoded: &[u8], want: usize) -> Result<Vec<u32>, Error> {
    let fail = || Error::redacted(ErrorKind::DicomdirWrite);
    let file = read_file(encoded).map_err(|_| fail())?;
    let items = file
        .dataset
        .get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE)
        .ok_or_else(fail)?;
    if items.len() != want {
        return Err(fail());
    }
    let mut offsets = Vec::with_capacity(items.len());
    let mut seen = std::collections::HashSet::new();
    for item in &items {
        match item.item_offset() {
            Some(o) if o > 0 && o <= i64::from(u32::MAX) && seen.insert(o as u32) => {
                offsets.push(o as u32)
            }
            _ => return Err(fail()),
        }
    }
    Ok(offsets)
}
fn validate_encoded_directory(
    encoded: &[u8],
    graph: &Graph,
    admission: &Limits,
) -> Result<(), Error> {
    let fail = || Error::redacted(ErrorKind::DicomdirWrite);
    let file = read_file(encoded).map_err(|_| fail())?;
    if file.transfer_syntax.uid != crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid
        || file.dataset.has(TAG_SOP_CLASS_UID)
        || file.dataset.has(TAG_SOP_INSTANCE_UID)
    {
        return Err(fail());
    }
    let meta_class = file
        .meta
        .get_uid(TAG_MEDIA_STORAGE_SOP_CLASS_UID)
        .unwrap_or_default();
    let meta_instance = file
        .meta
        .get_uid(TAG_MEDIA_STORAGE_SOP_INSTANCE_UID)
        .unwrap_or_default();
    if validate_directory_file_set_uid(&file.dataset, &meta_instance).is_err() {
        return Err(fail());
    }
    if meta_class != MEDIA_STORAGE_DIRECTORY_STORAGE_UID {
        return Err(fail());
    }
    let first = uint32_value(&file.dataset, TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD);
    let last = uint32_value(&file.dataset, TAG_OFFSET_LAST_ROOT_DIRECTORY_RECORD);
    if first != Some(graph.offset(graph.roots.first().copied()))
        || last != Some(graph.offset(graph.roots.last().copied()))
    {
        return Err(fail());
    }
    let items = file
        .dataset
        .get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE)
        .ok_or_else(fail)?;
    if items.len() != graph.flat.len() {
        return Err(fail());
    }
    let mut by_offset: HashMap<u32, usize> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let Some(offset) = item.item_offset() else {
            return Err(fail());
        };
        by_offset.insert(offset as u32, i);
    }
    let legacy = graph
        .flat
        .iter()
        .any(|&n| graph[n].typ.as_str() == RECORD_TYPE_MRDR);
    let references =
        validate_directory_data_set(&file.dataset, legacy, Some(admission)).map_err(|_| fail())?;
    for &n in &graph.flat {
        let node = &graph[n];
        let Some(&i) = by_offset.get(&node.offset) else {
            return Err(ErrorKind::DicomdirWrite.into());
        };
        let Some(item) = items.get(i) else {
            return Err(ErrorKind::DicomdirWrite.into());
        };
        let next = uint32_value(item, TAG_OFFSET_NEXT_DIRECTORY_RECORD);
        let lower = uint32_value(item, TAG_OFFSET_LOWER_LEVEL_RECORD);
        if next != Some(graph.offset(node.next)) || lower != Some(graph.first_child_offset(n)) {
            return Err(ErrorKind::DicomdirWrite.into());
        }
        if node.mrdr.is_some()
            && uint32_value(item, TAG_MRDR_DIRECTORY_RECORD_OFFSET) != Some(graph.offset(node.mrdr))
        {
            return Err(ErrorKind::DicomdirWrite.into());
        }
    }
    let expected: Vec<&FileRecord> = graph
        .flat
        .iter()
        .map(|&n| &graph[n])
        .filter(|n| standard_file_record_type(n.typ.as_str()))
        .map(|n| &n.record)
        .collect();
    if references.len() != expected.len() {
        return Err(fail());
    }
    for (got, want) in references.iter().zip(expected) {
        if got.file_id != want.file_id
            || got.record_type != want.record_type
            || !encoded_selection_matches_record(&got.selection, want)
            || got.sop_class_uid != want.sop_class_uid
            || got.sop_instance_uid != want.sop_instance_uid
            || got.transfer_syntax_uid != want.transfer_syntax_uid
            || got.related_general_sop_class_uids != want.related_general_sop_class_uids
            || !directory_keys_equal(&got.selection.keys, &want.keys)
        {
            return Err(fail());
        }
    }
    Ok(())
}
fn same_referenced_identity(a: &FileRecord, b: &FileRecord) -> bool {
    a.record_type == b.record_type
        && a.specific_character_set == b.specific_character_set
        && encoded_selection_matches_record(a, b)
        && a.sop_class_uid == b.sop_class_uid
        && a.sop_instance_uid == b.sop_instance_uid
        && a.transfer_syntax_uid == b.transfer_syntax_uid
        && a.related_general_sop_class_uids == b.related_general_sop_class_uids
}
impl FileSet {
    pub(crate) fn revalidate_sources(&self, records: &[SourceSnapshot]) -> Result<(), Error> {
        if !self.root_identity_matches() {
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        let ids: Vec<_> = records.iter().map(|s| s.record.file_id.clone()).collect();
        let (source, err) = validate_exact_file_id_spellings(&self.root, &ids);
        if err.is_some() {
            if source >= 0 {
                return Err(Error::source_validation(source as usize));
            }
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        for (i, stored) in records.iter().enumerate() {
            let reference = Reference {
                file_id: stored.record.file_id.0.clone(),
                typ: String::new(),
            };
            let Ok((file, _)) = open_referenced_file(&self.root, &reference) else {
                return Err(Error::source_validation(i));
            };
            match platform::stamp(&file) {
                Ok(s)
                    if s.identity == stored.stamp.identity
                        && s.size == stored.stamp.size
                        && s.modified == stored.stamp.modified => {}
                _ => return Err(Error::source_validation(i)),
            }
            let Ok(data) = read_handle(file) else {
                return Err(Error::source_validation(i));
            };
            let (result, err) = crate::index::read_bytes("", &data, self.metadata_index_options());
            drop(data);
            if err.is_some() {
                return Err(Error::source_validation(i));
            }
            let current = super::fileset::file_record_from_index(
                &stored.record.file_id,
                &result,
                self.options.invent_missing_keys || !stored.record.invented_keys.is_empty(),
            );
            let current = current.and_then(|mut c| {
                super::fileset::adopt_invented_keys(&mut c, &stored.record);
                self.read_directory_keys(&mut c).map(|()| c)
            });
            match current {
                Ok(c)
                    if same_referenced_identity(&stored.record, &c)
                        && directory_keys_equal(&stored.record.keys, &c.keys) => {}
                _ => return Err(Error::source_validation(i)),
            }
        }
        Ok(())
    }
}
fn publications() -> &'static (Mutex<Vec<FileIdentity>>, Condvar) {
    static LOCK: OnceLock<(Mutex<Vec<FileIdentity>>, Condvar)> = OnceLock::new();
    LOCK.get_or_init(|| (Mutex::new(Vec::new()), Condvar::new()))
}
struct PublicationLock(FileIdentity);
impl PublicationLock {
    fn acquire(root: FileIdentity) -> PublicationLock {
        let (lock, changed) = publications();
        let mut active = lock.lock().unwrap_or_else(|e| e.into_inner());
        while active.contains(&root) {
            active = changed.wait(active).unwrap_or_else(|e| e.into_inner());
        }
        active.push(root);
        PublicationLock(root)
    }
}
impl Drop for PublicationLock {
    fn drop(&mut self) {
        let (lock, changed) = publications();
        let mut active = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = active.iter().position(|a| *a == self.0) {
            active.remove(i);
        }
        changed.notify_all();
    }
}
fn directory_read_options(
    max_bytes: i64,
    max_records: i64,
    extra_elements: i64,
) -> Result<i64, Error> {
    const PER_RECORD: i64 = 16;
    if max_bytes <= 0
        || max_records < 0
        || !(0..=i64::MAX.saturating_sub(32)).contains(&extra_elements)
        || max_records > i64::MAX.saturating_sub(32).saturating_sub(extra_elements) / PER_RECORD
    {
        return Err(ErrorKind::ResourceLimit.into());
    }
    Ok(max_bytes)
}
struct TempGuard<'a> {
    path: String,
    file_set: &'a FileSet,
}
impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if self.file_set.root_identity_matches() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn create_temp(root: &str) -> std::io::Result<(std::fs::File, String)> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ u64::from(std::process::id());
    for _ in 0..10_000 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let name = format!(".DICOMDIR.tmp-{}", (seed >> 32) as u32);
        let path = pathrules::join(&[root, &name]);
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("dicomdir: no temporary name"))
}
pub fn commit_dicomdir(file_set: &FileSet, options: &WriteOptions) -> (WriteReport, Option<Error>) {
    match commit(file_set, options) {
        Ok(report) => (report, None),
        Err(pair) => {
            let (report, e) = *pair;
            (report.unwrap_or_default(), Some(e))
        }
    }
}
type CommitResult = Result<WriteReport, Box<(Option<WriteReport>, Error)>>;
fn commit(file_set: &FileSet, options: &WriteOptions) -> CommitResult {
    let fail = |e: Error| Box::new((None, e));
    let max_bytes = normalize_write_options(options).map_err(fail)?;
    if !file_set.root_identity_matches() {
        return Err(fail(Error::redacted(ErrorKind::SourceChanged)));
    }
    let imported = file_set.directory.as_deref();
    if let Some(d) = imported
        && !d.matches()
    {
        return Err(fail(ErrorKind::SourceChanged.into()));
    }
    let destination = pathrules::join(&[&file_set.root, "DICOMDIR"]);
    let previous: Option<FileIdentity> = match std::fs::symlink_metadata(&destination) {
        Ok(meta) => {
            if !meta.file_type().is_file() {
                return Err(fail(Error::redacted(ErrorKind::Transaction)));
            }
            match platform::path_identity(&destination) {
                Ok(id) => Some(id),
                Err(_) => return Err(fail(Error::redacted(ErrorKind::Transaction))),
            }
        }
        Err(e) if pathrules::is_not_exist(&e) => None,
        Err(_) => return Err(fail(Error::redacted(ErrorKind::Transaction))),
    };
    let (mut temp, temp_path) =
        create_temp(&file_set.root).map_err(|_| fail(Error::redacted(ErrorKind::Transaction)))?;
    let _guard = TempGuard {
        path: temp_path.clone(),
        file_set,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if temp
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .is_err()
        {
            return Err(fail(Error::redacted(ErrorKind::Transaction)));
        }
    }
    let report = write_dicomdir(&mut temp, file_set, options).map_err(fail)?;
    temp.sync_all()
        .map_err(|e| fail(Error::other(e.to_string())))?;
    drop(temp);
    if file_set.revision != report.revision {
        return Err(fail(Error::redacted(ErrorKind::SourceChanged)));
    }
    let records = snapshot_file_records(&file_set.records);
    let read_max =
        directory_read_options(max_bytes, report.records, report.extra_elements).map_err(fail)?;
    if let Err(e) = parse_staged_file_set(&temp_path, read_max, &file_set.options) {
        if e.is(ErrorKind::ResourceLimit) {
            return Err(fail(e));
        }
        return Err(fail(Error::redacted(ErrorKind::Transaction)));
    }
    file_set.revalidate_sources(&records).map_err(fail)?;
    let root_identity = file_set.root_identity;
    let _lock = PublicationLock::acquire(root_identity);
    if let Some(guard) = &options.publication_guard {
        guard(&destination).map_err(fail)?;
    }
    if !file_set.root_identity_matches() {
        return Err(fail(ErrorKind::SourceChanged.into()));
    }
    if let Some(d) = imported
        && !d.matches()
    {
        return Err(fail(ErrorKind::SourceChanged.into()));
    }
    if !same_destination_snapshot(&destination, previous) {
        return Err(fail(Error::redacted(ErrorKind::Transaction)));
    }
    if platform::replace_file_atomically(&temp_path, &destination).is_err() {
        return Err(fail(Error::redacted(ErrorKind::Transaction)));
    }
    if platform::sync_directory(&file_set.root).is_err() {
        return Err(Box::new((
            Some(report),
            Error::redacted(ErrorKind::Transaction),
        )));
    }
    Ok(report)
}
fn same_destination_snapshot(path: &str, previous: Option<FileIdentity>) -> bool {
    let meta = std::fs::symlink_metadata(path);
    match previous {
        None => matches!(&meta, Err(e) if pathrules::is_not_exist(e)),
        Some(id) => {
            meta.is_ok_and(|m| m.file_type().is_file())
                && platform::path_identity(path).is_ok_and(|current| current == id)
        }
    }
}
fn parse_staged_file_set(
    path: &str,
    max_bytes: i64,
    source: &super::fileset::Options,
) -> Result<FileSet, Error> {
    let snapshot =
        super::snapshot::read_directory_snapshot(path, source, source.legacy_mrdr, max_bytes)?;
    prepare_imported_file_set(
        snapshot,
        &[],
        &FileSetUpdateOptions {
            mode: FileSetUpdateMode::Append,
            source_options: source.clone(),
            legacy_mrdr: source.legacy_mrdr,
            max_bytes,
        },
    )
    .map(|(set, _)| set)
}
