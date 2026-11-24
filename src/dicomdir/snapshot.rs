#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{
    required_string_with_vr, required_uid, string_with_vr, strings_with_vr, uint16_value,
    uint32_value, valid_uid,
};
use super::error::{Error, ErrorKind};
use super::fileset::{
    FileId, FileRecord, Limits, Options, SourceSnapshot, new_file_id, normalize_file_set_limits,
};
use super::graph::{
    Graph, Node, TAG_ACCESSION_NUMBER, TAG_INSTANCE_NUMBER, TAG_MODALITY, TAG_PATIENT_ID,
    TAG_PATIENT_NAME, TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE,
    TAG_REFERENCED_SOP_CLASS_UID_IN_FILE, TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE,
    TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE, TAG_SERIES_INSTANCE_UID, TAG_SERIES_NUMBER,
    TAG_STUDY_DATE, TAG_STUDY_DESCRIPTION, TAG_STUDY_ID, TAG_STUDY_INSTANCE_UID, TAG_STUDY_TIME,
    link_siblings, same_series_keys, same_study_keys,
};
use super::keys::{
    KeyBudget, MetaError, TAG_MEDIA_STORAGE_SOP_CLASS_UID, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID,
    TAG_SOP_CLASS_UID, TAG_SOP_INSTANCE_UID, directory_key_rules, directory_keys_equal,
    own_directory_data_set, own_directory_element, read_handle, read_selected_directory_metadata,
    validate_directory_keys,
};
use super::pathrules;
use super::platform::{self, FileStamp};
use super::record_types::{
    RecordType, record_type_for_sop_class, root_file_record_type, standard_file_record_type,
};
use super::{
    RECORD_TYPE_MRDR, Reference, TAG_DIRECTORY_RECORD_SEQUENCE, TAG_DIRECTORY_RECORD_TYPE,
    TAG_MRDR_DIRECTORY_RECORD_OFFSET, TAG_NUMBER_OF_REFERENCES,
    TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD, TAG_OFFSET_LOWER_LEVEL_RECORD,
    TAG_OFFSET_NEXT_DIRECTORY_RECORD, TAG_RECORD_IN_USE_FLAG, TAG_REFERENCED_FILE_ID,
    TAG_SPECIFIC_CHARACTER_SET, open_referenced_file,
};
use crate::object::text::normalize_uid;
use crate::object::{DataSet, Object, Tag, Vr};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
pub(crate) const MEDIA_STORAGE_DIRECTORY_STORAGE_UID: &str = "1.2.840.10008.1.3.10";
pub(crate) const TAG_FILE_SET_ID: Tag = Tag::new(0x0004, 0x1130);
pub(crate) const TAG_FILE_SET_DESCRIPTOR_FILE_ID: Tag = Tag::new(0x0004, 0x1141);
pub(crate) const TAG_DESCRIPTOR_CHARACTER_SET: Tag = Tag::new(0x0004, 0x1142);
pub(crate) const TAG_OFFSET_LAST_ROOT_DIRECTORY_RECORD: Tag = Tag::new(0x0004, 0x1202);
pub(crate) const TAG_FILE_SET_CONSISTENCY_FLAG: Tag = Tag::new(0x0004, 0x1212);
#[derive(Clone, Debug, Default)]
pub(crate) struct EncodedReference {
    pub file_id: FileId,
    pub record_type: RecordType,
    pub selection: FileRecord,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub transfer_syntax_uid: String,
    pub related_general_sop_class_uids: Vec<String>,
    pub offset: u32,
    #[allow(dead_code)]
    pub mrdr_offset: u32,
}
#[derive(Debug)]
pub(crate) struct DirectorySnapshot {
    pub path: String,
    pub stamp: FileStamp,
    pub hash: [u8; 32],
    pub graph: Graph,
    pub references: Vec<EncodedReference>,
    pub uid: String,
    pub id: String,
    pub descriptor: FileId,
    pub descriptor_character_set: String,
    pub key_bytes: i64,
    #[allow(dead_code)]
    pub legacy: bool,
}
fn capture_directory_record_keys(
    src: &Object,
    typ: &str,
    limits: &Limits,
) -> Result<(DataSet, i64), Error> {
    let mut limits = *limits;
    normalize_file_set_limits(&mut limits)?;
    let mut b = KeyBudget::new(limits);
    let mut keys = DataSet::default();
    for rule in directory_key_rules(typ) {
        if !src.has(rule.tag) {
            continue;
        }
        keys.elements
            .push(own_directory_element(src, rule.tag, &mut b, 0)?);
    }
    Ok((keys, b.bytes))
}
fn meta_error(e: MetaError) -> Error {
    match e {
        MetaError::Dir(e) => e,
        MetaError::Object(e) => {
            if super::keys::is_parser_resource_limit(&e) {
                ErrorKind::ResourceLimit.into()
            } else {
                Error::other(e.to_string())
            }
        }
    }
}
fn open_snapshot_file(abs: &str) -> Result<std::fs::File, Error> {
    let reference = Reference {
        file_id: vec![pathrules::base(abs)],
        typ: String::new(),
    };
    open_referenced_file(pathrules::dir(abs), &reference)
        .map(|(f, _)| f)
        .map_err(|_| ErrorKind::InvalidRecord.into())
}
pub(crate) fn read_directory_snapshot(
    path: &str,
    options: &Options,
    legacy: bool,
    max_bytes: i64,
) -> Result<DirectorySnapshot, Error> {
    let mut limits = options.limits;
    normalize_file_set_limits(&mut limits)?;
    let abs = pathrules::abs(path).map_err(|_| Error::from(ErrorKind::InvalidRecord))?;
    let handle = open_snapshot_file(&abs)?;
    let stamp = match platform::stamp(&handle) {
        Ok(s) if s.regular && s.size <= max_bytes => s,
        _ => return Err(ErrorKind::ResourceLimit.into()),
    };
    let data = read_handle(handle).map_err(|_| Error::from(ErrorKind::InvalidRecord))?;
    if data.len() as i64 > max_bytes {
        return Err(ErrorKind::ResourceLimit.into());
    }
    let hash: [u8; 32] = Sha256::digest(&data).into();
    let (header, dataset) =
        read_selected_directory_metadata(data, "", &limits, 0, true).map_err(meta_error)?;
    let (class, class_ok) = required_uid(&header.meta, TAG_MEDIA_STORAGE_SOP_CLASS_UID);
    let (instance, instance_ok) = required_uid(&header.meta, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID);
    if !class_ok || !instance_ok || class != MEDIA_STORAGE_DIRECTORY_STORAGE_UID {
        return Err(ErrorKind::InvalidRecord.into());
    }
    if dataset.has(TAG_SOP_CLASS_UID)
        || dataset.has(TAG_SOP_INSTANCE_UID)
        || dataset.has(Tag::PIXEL_DATA)
    {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let Some(id) = string_with_vr(&dataset, TAG_FILE_SET_ID, Vr::CS) else {
        return Err(ErrorKind::InvalidRecord.into());
    };
    validate_directory_file_set_uid(&dataset, &instance)?;
    let references = validate_directory_data_set(&dataset, legacy, Some(&limits))?;
    let mut snapshot = DirectorySnapshot {
        path: abs.clone(),
        stamp,
        hash,
        graph: Graph::default(),
        key_bytes: references.iter().map(|r| r.selection.key_bytes).sum(),
        references,
        uid: instance,
        id,
        descriptor: FileId::default(),
        descriptor_character_set: String::new(),
        legacy,
    };
    if dataset.has(TAG_FILE_SET_DESCRIPTOR_FILE_ID) {
        let Some(ids) = strings_with_vr(&dataset, TAG_FILE_SET_DESCRIPTOR_FILE_ID, Vr::CS) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        if !ids.is_empty() {
            snapshot.descriptor = new_file_id(&ids)?;
        }
    }
    if dataset.has(TAG_DESCRIPTOR_CHARACTER_SET) {
        match required_string_with_vr(&dataset, TAG_DESCRIPTOR_CHARACTER_SET, Vr::CS) {
            Some(cs) if !snapshot.descriptor.0.is_empty() => snapshot.descriptor_character_set = cs,
            _ => return Err(ErrorKind::InvalidRecord.into()),
        }
    }
    let items = dataset
        .get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE)
        .unwrap_or_default();
    let mut by_offset: HashMap<u32, usize> = HashMap::new();
    let mut raw_by_offset: HashMap<u32, usize> = HashMap::new();
    let ref_by_offset: HashMap<u32, usize> = snapshot
        .references
        .iter()
        .enumerate()
        .map(|(i, r)| (r.offset, i))
        .collect();
    let g = &mut snapshot.graph;
    for (i, item) in items.iter().enumerate() {
        let offset = item.item_offset().unwrap_or(0) as u32;
        let typ = item
            .get_string(TAG_DIRECTORY_RECORD_TYPE)
            .unwrap_or_default();
        raw_by_offset.insert(offset, i);
        let flag = uint16_value(item, TAG_RECORD_IN_USE_FLAG).unwrap_or(0);
        if flag == 0 {
            let n = g.push(Node {
                typ: RecordType::from(typ),
                offset,
                inactive: true,
                ..Default::default()
            });
            by_offset.insert(offset, n);
            continue;
        }
        let remaining = limits
            .max_total_record_key_bytes
            .saturating_sub(snapshot.key_bytes);
        if remaining <= 0 {
            return Err(ErrorKind::ResourceLimit.into());
        }
        let mut own_limits = limits;
        if remaining < own_limits.max_record_key_bytes {
            own_limits.max_record_key_bytes = remaining;
        }
        let mut b = KeyBudget::new(own_limits);
        let owned = own_directory_data_set(item, &mut b, 0)?;
        if b.bytes
            > limits
                .max_total_record_key_bytes
                .saturating_sub(snapshot.key_bytes)
        {
            return Err(ErrorKind::ResourceLimit.into());
        }
        snapshot.key_bytes = snapshot.key_bytes.saturating_add(b.bytes);
        let mut node = Node {
            typ: RecordType::from(typ.clone()),
            imported: owned,
            offset,
            ..Default::default()
        };
        if let Some(reference) = ref_by_offset
            .get(&offset)
            .and_then(|&r| snapshot.references.get(r))
        {
            node.record = reference.selection.clone();
        }
        if typ == RECORD_TYPE_MRDR {
            node.record.file_id =
                FileId(item.get_strings(TAG_REFERENCED_FILE_ID).unwrap_or_default());
        }
        let n = g.push(node);
        by_offset.insert(offset, n);
        g.flat.push(n);
    }
    fn group(
        g: &mut Graph,
        items: &[Object],
        by_offset: &HashMap<u32, usize>,
        raw_by_offset: &HashMap<u32, usize>,
        mut offset: u32,
    ) -> Vec<usize> {
        let mut nodes = Vec::new();
        while offset != 0 {
            let (Some(&node), Some(&raw)) = (by_offset.get(&offset), raw_by_offset.get(&offset))
            else {
                break;
            };
            let Some(raw) = items.get(raw) else {
                break;
            };
            if g[node].inactive {
                offset = uint32_value(raw, TAG_OFFSET_NEXT_DIRECTORY_RECORD).unwrap_or(0);
                continue;
            }
            nodes.push(node);
            let lower = uint32_value(raw, TAG_OFFSET_LOWER_LEVEL_RECORD).unwrap_or(0);
            let children = group(g, items, by_offset, raw_by_offset, lower);
            g[node].children = children;
            if let Some(target) = uint32_value(raw, TAG_MRDR_DIRECTORY_RECORD_OFFSET) {
                g[node].mrdr = by_offset.get(&target).copied();
            }
            let next = uint32_value(raw, TAG_OFFSET_NEXT_DIRECTORY_RECORD).unwrap_or(0);
            g[node].next = by_offset.get(&next).copied();
            offset = next;
        }
        nodes
    }
    let first = uint32_value(&dataset, TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD).unwrap_or(0);
    let roots = group(g, &items, &by_offset, &raw_by_offset, first);
    g.roots = roots;
    for &n in &g.flat.clone() {
        let raw = Object::from_data_set(g[n].imported.clone());
        let node = &mut g[n];
        let get = |tag| raw.get_string(tag).unwrap_or_default();
        match node.typ.as_str() {
            "PATIENT" => {
                node.record.patient_id = get(TAG_PATIENT_ID);
                node.record.patient_name = get(TAG_PATIENT_NAME);
            }
            "STUDY" => {
                node.record.study_instance_uid =
                    raw.get_uid(TAG_STUDY_INSTANCE_UID).unwrap_or_default();
                node.record.study_date = get(TAG_STUDY_DATE);
                node.record.study_time = get(TAG_STUDY_TIME);
                node.record.study_description = get(TAG_STUDY_DESCRIPTION);
                node.record.study_id = get(TAG_STUDY_ID);
                node.record.accession_number = get(TAG_ACCESSION_NUMBER);
            }
            "SERIES" => {
                node.record.series_instance_uid =
                    raw.get_uid(TAG_SERIES_INSTANCE_UID).unwrap_or_default();
                node.record.modality = get(TAG_MODALITY);
                node.record.series_number = get(TAG_SERIES_NUMBER);
            }
            _ => {}
        }
    }
    let check = open_snapshot_file(&abs).map_err(|_| Error::from(ErrorKind::SourceChanged))?;
    match platform::stamp(&check) {
        Ok(current)
            if current.identity == stamp.identity
                && current.size == stamp.size
                && current.modified == stamp.modified => {}
        _ => return Err(ErrorKind::SourceChanged.into()),
    }
    Ok(snapshot)
}
impl DirectorySnapshot {
    pub fn matches(&self) -> bool {
        let Ok(handle) = open_snapshot_file(&self.path) else {
            return false;
        };
        match platform::stamp(&handle) {
            Ok(s)
                if s.identity == self.stamp.identity
                    && s.size == self.stamp.size
                    && s.modified == self.stamp.modified => {}
            _ => return false,
        }
        let Ok(data) = read_handle(handle) else {
            return false;
        };
        if data.len() as i64 > self.stamp.size {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(&data).into();
        digest == self.hash
    }
    pub(crate) fn has_mrdr(&self) -> bool {
        self.graph
            .flat
            .iter()
            .any(|&n| self.graph[n].typ.as_str() == RECORD_TYPE_MRDR)
    }
    fn standard_aliases(&self) -> HashMap<String, Vec<usize>> {
        let mut aliases: HashMap<String, Vec<usize>> = HashMap::new();
        for &n in &self.graph.flat {
            let node = &self.graph[n];
            if standard_file_record_type(node.typ.as_str()) {
                aliases
                    .entry(node.record.file_id.to_string())
                    .or_default()
                    .push(n);
            }
        }
        aliases
    }
}
pub(crate) fn directory_entity_record(r: &FileRecord) -> FileRecord {
    let mut r = r.clone();
    r.keys = DataSet::default();
    r.key_bytes = 0;
    r.file_id = FileId::default();
    r.related_general_sop_class_uids = Vec::new();
    r.invented_keys = Vec::new();
    r
}
pub(crate) fn merge_imported_directory(
    s: &DirectorySnapshot,
    fresh: &Graph,
) -> Result<Graph, Error> {
    let snap = &s.graph;
    let mut current: HashMap<String, FileRecord> = HashMap::new();
    fn collect(g: &Graph, nodes: &[usize], current: &mut HashMap<String, FileRecord>) {
        for &n in nodes {
            let node = &g[n];
            if standard_file_record_type(node.typ.as_str()) {
                current.insert(node.record.file_id.to_string(), node.record.clone());
            }
            collect(g, &node.children, current);
        }
    }
    collect(fresh, &fresh.roots, &mut current);
    let aliases = s.standard_aliases();
    struct Merge<'a> {
        snap: &'a Graph,
        out: Graph,
        assigned: HashSet<String>,
        physical: HashMap<usize, usize>,
        current: HashMap<String, FileRecord>,
    }
    impl Merge<'_> {
        fn clone_leaf(&mut self, prior: Option<usize>, r: &FileRecord) -> usize {
            let mut n = Node::new(r.record_type.clone(), r.clone());
            if let Some(p) = prior {
                let pn = &self.snap[p];
                n.imported = pn.imported.clone();
                if let Some(m) = pn.mrdr {
                    let target = match self.physical.get(&m) {
                        Some(&t) => t,
                        None => {
                            let mn = &self.snap[m];
                            let t = self.out.push(Node {
                                typ: RecordType::from(RECORD_TYPE_MRDR),
                                record: FileRecord {
                                    file_id: mn.record.file_id.clone(),
                                    ..Default::default()
                                },
                                imported: mn.imported.clone(),
                                ..Default::default()
                            });
                            self.physical.insert(m, t);
                            t
                        }
                    };
                    n.mrdr = Some(target);
                    self.out[target].references = self.out[target].references.saturating_add(1);
                }
            }
            self.out.push(n)
        }
        fn retain(
            &mut self,
            prior: usize,
            mut patient: String,
            mut study: String,
            mut series: String,
        ) -> Option<usize> {
            let pn = &self.snap[prior];
            if standard_file_record_type(pn.typ.as_str()) {
                let r = self.current.get(&pn.record.file_id.to_string())?.clone();
                if r.patient_id != patient
                    || r.study_instance_uid != study
                    || r.series_instance_uid != series
                {
                    return None;
                }
                self.assigned.insert(r.file_id.to_string());
                return Some(self.clone_leaf(Some(prior), &r));
            }
            let mut n = Node {
                typ: pn.typ.clone(),
                record: directory_entity_record(&pn.record),
                imported: pn.imported.clone(),
                ..Default::default()
            };
            match n.typ.as_str() {
                "PATIENT" => patient = n.record.patient_id.clone(),
                "STUDY" => study = n.record.study_instance_uid.clone(),
                "SERIES" => series = n.record.series_instance_uid.clone(),
                _ => {}
            }
            let prior_children = pn.children.clone();
            for child in &prior_children {
                if let Some(kept) =
                    self.retain(*child, patient.clone(), study.clone(), series.clone())
                {
                    n.children.push(kept);
                }
            }
            if !prior_children.is_empty() && n.children.is_empty() {
                return None;
            }
            let idx = self.out.push(n);
            for leaf in self.out.leaf_nodes(idx) {
                let leaf_node = &self.out[leaf];
                if standard_file_record_type(leaf_node.typ.as_str()) {
                    let record = directory_entity_record(&leaf_node.record);
                    self.out[idx].record = record;
                    break;
                }
            }
            Some(idx)
        }
        fn find(&self, nodes: &[usize], typ: &str, key: &str) -> Option<usize> {
            nodes.iter().copied().find(|&n| {
                let node = &self.out[n];
                node.typ.as_str() == typ
                    && match typ {
                        "PATIENT" => node.record.patient_id == key,
                        "STUDY" => node.record.study_instance_uid == key,
                        "SERIES" => node.record.series_instance_uid == key,
                        _ => false,
                    }
            })
        }
    }
    let mut m = Merge {
        snap,
        out: Graph::default(),
        assigned: HashSet::new(),
        physical: HashMap::new(),
        current,
    };
    for &prior in &snap.roots {
        if let Some(kept) = m.retain(prior, String::new(), String::new(), String::new()) {
            m.out.roots.push(kept);
        }
    }
    fn add(
        m: &mut Merge,
        fresh: &Graph,
        nodes: &[usize],
        aliases: &HashMap<String, Vec<usize>>,
    ) -> Result<(), Error> {
        for &leaf in nodes {
            let fnode = &fresh[leaf];
            if !standard_file_record_type(fnode.typ.as_str()) {
                add(m, fresh, &fnode.children, aliases)?;
                continue;
            }
            let r = fnode.record.clone();
            let key = r.file_id.to_string();
            if m.assigned.contains(&key) {
                continue;
            }
            let parent: Option<usize> = if root_file_record_type(r.record_type.as_str()) {
                None
            } else {
                let roots = m.out.roots.clone();
                let patient = match m.find(&roots, "PATIENT", &r.patient_id) {
                    None => {
                        let p = m
                            .out
                            .push(Node::new(RecordType::PATIENT, directory_entity_record(&r)));
                        m.out.roots.push(p);
                        p
                    }
                    Some(p) => {
                        if m.out[p].record.patient_name != r.patient_name {
                            return Err(ErrorKind::InvalidRecord.into());
                        }
                        p
                    }
                };
                let children = m.out[patient].children.clone();
                let study = match m.find(&children, "STUDY", &r.study_instance_uid) {
                    None => {
                        let s = m
                            .out
                            .push(Node::new(RecordType::STUDY, directory_entity_record(&r)));
                        m.out[patient].children.push(s);
                        s
                    }
                    Some(s) => {
                        if !same_study_keys(&m.out[s].record, &r) {
                            return Err(ErrorKind::InvalidRecord.into());
                        }
                        s
                    }
                };
                let children = m.out[study].children.clone();
                let series = match m.find(&children, "SERIES", &r.series_instance_uid) {
                    None => {
                        let s = m
                            .out
                            .push(Node::new(RecordType::SERIES, directory_entity_record(&r)));
                        m.out[study].children.push(s);
                        s
                    }
                    Some(s) => {
                        if !same_series_keys(&m.out[s].record, &r) {
                            return Err(ErrorKind::InvalidRecord.into());
                        }
                        s
                    }
                };
                Some(series)
            };
            let priors: Vec<Option<usize>> = match aliases.get(&key) {
                Some(list) if !list.is_empty() => list.iter().map(|&p| Some(p)).collect(),
                _ => vec![None],
            };
            for prior in priors {
                let n = m.clone_leaf(prior, &r);
                match parent {
                    None => m.out.roots.push(n),
                    Some(p) => m.out[p].children.push(n),
                }
            }
            m.assigned.insert(key);
        }
        Ok(())
    }
    add(&mut m, fresh, &fresh.roots, &aliases)?;
    let mut out = m.out;
    let physical = m.physical;
    out.flat = out.preorder();
    for n in out.nodes.iter_mut() {
        n.next = None;
    }
    link_siblings(&mut out);
    for &prior in &snap.flat {
        if let Some(&n) = physical.get(&prior) {
            out.flat.push(n);
        }
    }
    let mut check = EntityKeyValidator::default();
    for &n in &out.flat {
        let node = &out[n];
        if node.typ.as_str() != RECORD_TYPE_MRDR {
            check.admit(node.typ.as_str(), &node.record)?;
        }
    }
    Ok(out)
}
pub(crate) fn planned_directory_record_count(
    snapshot: Option<&DirectorySnapshot>,
    records: &[SourceSnapshot],
    maximum: i64,
) -> Result<i64, Error> {
    let mut current: HashMap<String, &FileRecord> = HashMap::new();
    for stored in records {
        current.insert(stored.record.file_id.to_string(), &stored.record);
    }
    struct Plan<'a> {
        count: i64,
        maximum: i64,
        entities: HashSet<String>,
        assigned: HashSet<String>,
        targets: HashSet<usize>,
        current: HashMap<String, &'a FileRecord>,
    }
    impl Plan<'_> {
        fn charge(&mut self, n: i64) -> Result<(), Error> {
            if n < 0 || n > self.maximum.saturating_sub(self.count) {
                return Err(ErrorKind::ResourceLimit.into());
            }
            self.count = self.count.saturating_add(n);
            Ok(())
        }
    }
    let path_key = |typ: &str, p: &str, s: &str, r: &str| format!("{typ}:{p}/{s}/{r}");
    let mut plan = Plan {
        count: 0,
        maximum,
        entities: HashSet::new(),
        assigned: HashSet::new(),
        targets: HashSet::new(),
        current,
    };
    fn retain(
        plan: &mut Plan,
        g: &Graph,
        n: usize,
        mut p: String,
        mut s: String,
        mut r: String,
    ) -> Result<bool, Error> {
        let node = &g[n];
        if standard_file_record_type(node.typ.as_str()) {
            let Some(v) = plan.current.get(&node.record.file_id.to_string()) else {
                return Ok(false);
            };
            if v.patient_id != p || v.study_instance_uid != s || v.series_instance_uid != r {
                return Ok(false);
            }
            let key = v.file_id.to_string();
            plan.charge(1)?;
            plan.assigned.insert(key);
            if let Some(m) = node.mrdr {
                plan.targets.insert(m);
            }
            return Ok(true);
        }
        match node.typ.as_str() {
            "PATIENT" => p = node.record.patient_id.clone(),
            "STUDY" => s = node.record.study_instance_uid.clone(),
            "SERIES" => r = node.record.series_instance_uid.clone(),
            _ => {}
        }
        let mut alive = node.children.is_empty();
        for &child in &node.children {
            let kept = retain(plan, g, child, p.clone(), s.clone(), r.clone())?;
            alive = alive || kept;
        }
        if alive {
            plan.charge(1)?;
            plan.entities.insert(format!("{}:{p}/{s}/{r}", node.typ));
        }
        Ok(alive)
    }
    let mut aliases: HashMap<String, Vec<usize>> = HashMap::new();
    if let Some(snap) = snapshot {
        aliases = snap.standard_aliases();
        for &root in &snap.graph.roots {
            retain(
                &mut plan,
                &snap.graph,
                root,
                String::new(),
                String::new(),
                String::new(),
            )?;
        }
    }
    for stored in records {
        let v = &stored.record;
        let key = v.file_id.to_string();
        if plan.assigned.contains(&key) {
            continue;
        }
        if !root_file_record_type(v.record_type.as_str()) {
            for (typ, p, s, r) in [
                ("PATIENT", v.patient_id.as_str(), "", ""),
                (
                    "STUDY",
                    v.patient_id.as_str(),
                    v.study_instance_uid.as_str(),
                    "",
                ),
                (
                    "SERIES",
                    v.patient_id.as_str(),
                    v.study_instance_uid.as_str(),
                    v.series_instance_uid.as_str(),
                ),
            ] {
                let k = path_key(typ, p, s, r);
                if !plan.entities.contains(&k) {
                    plan.charge(1)?;
                    plan.entities.insert(k);
                }
            }
        }
        let list = aliases.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let n = if list.is_empty() {
            1
        } else {
            list.len() as i64
        };
        plan.charge(n)?;
        if let Some(snap) = snapshot {
            for &prior in list {
                if let Some(m) = snap.graph[prior].mrdr {
                    plan.targets.insert(m);
                }
            }
        }
    }
    let targets = plan.targets.len() as i64;
    plan.charge(targets)?;
    Ok(plan.count)
}
pub(crate) fn planned_directory_key_bytes(
    snapshot: Option<&DirectorySnapshot>,
    records: &[SourceSnapshot],
    maximum: i64,
) -> Result<(), Error> {
    planned_directory_key_usage(snapshot, records, maximum).map(|_| ())
}
pub(crate) fn planned_directory_key_usage(
    snapshot: Option<&DirectorySnapshot>,
    records: &[SourceSnapshot],
    maximum: i64,
) -> Result<i64, Error> {
    let mut used = 0i64;
    let mut aliases: HashMap<String, i64> = HashMap::new();
    if let Some(s) = snapshot {
        used = s.key_bytes;
        for &n in &s.graph.flat {
            let node = &s.graph[n];
            if standard_file_record_type(node.typ.as_str()) {
                let count = aliases.entry(node.record.file_id.to_string()).or_default();
                *count = count.saturating_add(1);
            }
        }
    }
    if used > maximum {
        return Err(ErrorKind::ResourceLimit.into());
    }
    for stored in records {
        let mut n = aliases
            .get(&stored.record.file_id.to_string())
            .copied()
            .unwrap_or(0);
        if n == 0 {
            n = 1;
        }
        let bytes = stored.record.key_bytes;
        if bytes < 0
            || bytes > 0 && n > maximum.saturating_sub(used).checked_div(bytes).unwrap_or(0)
        {
            return Err(ErrorKind::ResourceLimit.into());
        }
        used = used.saturating_add(bytes.saturating_mul(n));
    }
    Ok(used)
}
pub(crate) fn alias_counts(snapshot: Option<&DirectorySnapshot>) -> HashMap<String, i64> {
    let mut aliases: HashMap<String, i64> = HashMap::new();
    if let Some(s) = snapshot {
        for &n in &s.graph.flat {
            let node = &s.graph[n];
            if standard_file_record_type(node.typ.as_str()) {
                let count = aliases.entry(node.record.file_id.to_string()).or_default();
                *count = count.saturating_add(1);
            }
        }
    }
    aliases
}
#[derive(Default)]
pub(crate) struct EntityKeyValidator {
    patients: HashMap<String, FileRecord>,
    studies: HashMap<String, FileRecord>,
    series: HashMap<String, FileRecord>,
    roles: HashMap<String, &'static str>,
}
impl EntityKeyValidator {
    pub fn admit(&mut self, typ: &str, r: &FileRecord) -> Result<(), Error> {
        let (id, role): (&str, &'static str) = match typ {
            "PATIENT" => (&r.patient_id, ""),
            "STUDY" => (&r.study_instance_uid, "STUDY"),
            "SERIES" => (&r.series_instance_uid, "SERIES"),
            _ => (&r.sop_instance_uid, "IMAGE"),
        };
        if id.is_empty() {
            return Ok(());
        }
        if !role.is_empty() {
            if let Some(&old) = self.roles.get(id)
                && old != role
            {
                return Err(ErrorKind::InvalidRecord.into());
            }
            self.roles.insert(id.to_string(), role);
        }
        let records = match typ {
            "PATIENT" => &mut self.patients,
            "STUDY" => &mut self.studies,
            "SERIES" => &mut self.series,
            _ => return Ok(()),
        };
        if let Some(old) = records.get(id) {
            let ok = match typ {
                "PATIENT" => old.patient_name == r.patient_name,
                "STUDY" => old.patient_id == r.patient_id && same_study_keys(old, r),
                _ => {
                    old.patient_id == r.patient_id
                        && old.study_instance_uid == r.study_instance_uid
                        && same_series_keys(old, r)
                }
            };
            if !ok {
                return Err(ErrorKind::InvalidRecord.into());
            }
        }
        records.insert(id.to_string(), r.clone());
        Ok(())
    }
}
fn inspect_legacy_mrdr(
    items: &[Object],
    by_offset: &[(u32, usize)],
    legacy: bool,
) -> Result<HashMap<u32, usize>, Error> {
    let mut out = HashMap::new();
    for &(offset, i) in by_offset {
        let Some(item) = items.get(i) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        let typ = item
            .get_string(TAG_DIRECTORY_RECORD_TYPE)
            .unwrap_or_default();
        if typ != RECORD_TYPE_MRDR {
            if item.has(TAG_MRDR_DIRECTORY_RECORD_OFFSET) && !legacy {
                return Err(ErrorKind::LegacyMrdrRequiresOptIn.into());
            }
            continue;
        }
        if !legacy {
            return Err(ErrorKind::LegacyMrdrRequiresOptIn.into());
        }
        let next = uint32_value(item, TAG_OFFSET_NEXT_DIRECTORY_RECORD);
        let lower = uint32_value(item, TAG_OFFSET_LOWER_LEVEL_RECORD);
        let flag = uint16_value(item, TAG_RECORD_IN_USE_FLAG);
        let count = uint32_value(item, TAG_NUMBER_OF_REFERENCES);
        let (Some(next), Some(lower), Some(flag), Some(count)) = (next, lower, flag, count) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        if next != 0 || lower != 0 || (flag == 0) != (count == 0) {
            return Err(ErrorKind::InvalidRecord.into());
        }
        let ids = strings_with_vr(item, TAG_REFERENCED_FILE_ID, Vr::CS);
        if flag != 0 && ids.is_none() {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if flag != 0 {
            new_file_id(&ids.unwrap_or_default())?;
        }
        for tag in [
            TAG_REFERENCED_SOP_CLASS_UID_IN_FILE,
            TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE,
            TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
            TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE,
            TAG_MRDR_DIRECTORY_RECORD_OFFSET,
        ] {
            if item.has(tag) {
                return Err(ErrorKind::InvalidRecord.into());
            }
        }
        out.insert(offset, i);
    }
    Ok(out)
}
fn logical_mrdr_target(
    item: &Object,
    items: &[Object],
    mrdrs: &HashMap<u32, usize>,
    legacy: bool,
) -> Result<Option<u32>, Error> {
    if !item.has(TAG_MRDR_DIRECTORY_RECORD_OFFSET) {
        return Ok(None);
    }
    if !legacy {
        return Err(ErrorKind::LegacyMrdrRequiresOptIn.into());
    }
    let offset = match uint32_value(item, TAG_MRDR_DIRECTORY_RECORD_OFFSET) {
        Some(o) if o != 0 && mrdrs.contains_key(&o) && !item.has(TAG_REFERENCED_FILE_ID) => o,
        _ => return Err(ErrorKind::InvalidRecord.into()),
    };
    let item = mrdrs.get(&offset).and_then(|&i| items.get(i));
    match item.and_then(|item| uint16_value(item, TAG_RECORD_IN_USE_FLAG)) {
        Some(flag) if flag != 0 => Ok(Some(offset)),
        _ => Err(ErrorKind::InvalidRecord.into()),
    }
}
struct TraversalFrame {
    offset: u32,
    parent_type: String,
    root: bool,
    selection: FileRecord,
}
pub(crate) fn validate_directory_data_set(
    dataset: &Object,
    legacy: bool,
    admission: Option<&Limits>,
) -> Result<Vec<EncodedReference>, Error> {
    let mut limits = admission.copied().unwrap_or_default();
    normalize_file_set_limits(&mut limits)?;
    let invalid = || Error::from(ErrorKind::InvalidRecord);
    let mut key_bytes = 0i64;
    let mut entity_keys = EntityKeyValidator::default();
    let first =
        uint32_value(dataset, TAG_OFFSET_FIRST_ROOT_DIRECTORY_RECORD).ok_or_else(invalid)?;
    let last = uint32_value(dataset, TAG_OFFSET_LAST_ROOT_DIRECTORY_RECORD).ok_or_else(invalid)?;
    match uint16_value(dataset, TAG_FILE_SET_CONSISTENCY_FLAG) {
        Some(0) => {}
        _ => return Err(invalid()),
    }
    let items = dataset
        .get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE)
        .ok_or_else(invalid)?;
    if items.is_empty() {
        if first != 0 || last != 0 {
            return Err(invalid());
        }
        return Ok(Vec::new());
    }
    if first == 0 || last == 0 {
        return Err(invalid());
    }
    let mut by_offset: HashMap<u32, usize> = HashMap::new();
    let mut ordered: Vec<(u32, usize)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let offset = match item.item_offset() {
            Some(o)
                if o > 0 && o <= i64::from(u32::MAX) && !by_offset.contains_key(&(o as u32)) =>
            {
                o as u32
            }
            _ => return Err(invalid()),
        };
        by_offset.insert(offset, i);
        ordered.push((offset, i));
    }
    let mrdrs = inspect_legacy_mrdr(&items, &ordered, legacy)?;
    let mut root_last = 0u32;
    let mut seen: HashSet<u32> = HashSet::new();
    let mut offset = first;
    while offset != 0 {
        if !seen.insert(offset) {
            return Err(invalid());
        }
        let Some(&i) = by_offset.get(&offset) else {
            return Err(invalid());
        };
        root_last = offset;
        let item = items.get(i).ok_or_else(invalid)?;
        offset = uint32_value(item, TAG_OFFSET_NEXT_DIRECTORY_RECORD).ok_or_else(invalid)?;
    }
    if root_last != last {
        return Err(invalid());
    }
    let mut visited: HashSet<u32> = HashSet::new();
    let mut pending = vec![TraversalFrame {
        offset: first,
        parent_type: String::new(),
        root: true,
        selection: FileRecord::default(),
    }];
    let mut references = Vec::new();
    let mut file_ids: HashSet<String> = HashSet::new();
    let mut sop_instances: HashSet<String> = HashSet::new();
    let mut file_targets: HashMap<String, u32> = HashMap::new();
    let mut file_selections: HashMap<String, FileRecord> = HashMap::new();
    let mut mrdr_counts: HashMap<u32, u32> = HashMap::new();
    while let Some(frame) = pending.pop() {
        if frame.offset == 0 {
            continue;
        }
        if visited.contains(&frame.offset) {
            return Err(invalid());
        }
        let Some(&index) = by_offset.get(&frame.offset) else {
            return Err(invalid());
        };
        let item = items.get(index).ok_or_else(invalid)?;
        visited.insert(frame.offset);
        if let Some(cs) = item.get(TAG_SPECIFIC_CHARACTER_SET)
            && cs.vr() != Vr::CS
        {
            return Err(invalid());
        }
        let type_value =
            string_with_vr(item, TAG_DIRECTORY_RECORD_TYPE, Vr::CS).ok_or_else(invalid)?;
        let record_type = RecordType::from(type_value.clone());
        if !valid_hierarchy_child(&frame.parent_type, &type_value, frame.root) {
            return Err(invalid());
        }
        let next = uint32_value(item, TAG_OFFSET_NEXT_DIRECTORY_RECORD);
        let lower = uint32_value(item, TAG_OFFSET_LOWER_LEVEL_RECORD);
        let in_use = uint16_value(item, TAG_RECORD_IN_USE_FLAG);
        let (Some(next), Some(lower), Some(in_use)) = (next, lower, in_use) else {
            return Err(invalid());
        };
        if in_use == 0 && (!legacy || lower != 0 || !standard_file_record_type(&type_value)) {
            return Err(invalid());
        }
        if next != 0 {
            pending.push(TraversalFrame {
                offset: next,
                parent_type: frame.parent_type.clone(),
                root: frame.root,
                selection: frame.selection.clone(),
            });
        }
        if in_use == 0 {
            continue;
        }
        let mut child = TraversalFrame {
            offset: lower,
            parent_type: type_value.clone(),
            root: false,
            selection: frame.selection.clone(),
        };
        let entity = matches!(type_value.as_str(), "PATIENT" | "STUDY" | "SERIES");
        match type_value.as_str() {
            "PATIENT" => {
                let id = required_string_with_vr(item, TAG_PATIENT_ID, Vr::LO);
                let name = string_with_vr(item, TAG_PATIENT_NAME, Vr::PN);
                let (Some(id), Some(name)) = (id, name) else {
                    return Err(invalid());
                };
                child.selection.patient_name = name;
                child.selection.patient_id = id;
            }
            "STUDY" => {
                let (uid, ok) = required_uid(item, TAG_STUDY_INSTANCE_UID);
                if !ok || !has_required_study_keys(item) {
                    return Err(invalid());
                }
                let get = |tag| item.get_string(tag).unwrap_or_default();
                child.selection.study_date = get(TAG_STUDY_DATE);
                child.selection.study_time = get(TAG_STUDY_TIME);
                child.selection.study_description = get(TAG_STUDY_DESCRIPTION);
                child.selection.study_instance_uid = uid;
                child.selection.study_id = get(TAG_STUDY_ID);
                child.selection.accession_number = get(TAG_ACCESSION_NUMBER);
            }
            "SERIES" => {
                let (uid, ok) = required_uid(item, TAG_SERIES_INSTANCE_UID);
                if !ok || !has_required_series_keys(item) {
                    return Err(invalid());
                }
                child.selection.modality = item.get_string(TAG_MODALITY).unwrap_or_default();
                child.selection.series_instance_uid = uid;
                child.selection.series_number =
                    item.get_string(TAG_SERIES_NUMBER).unwrap_or_default();
            }
            _ => {
                child.selection.instance_number =
                    item.get_string(TAG_INSTANCE_NUMBER).unwrap_or_default();
            }
        }
        let child_offset = child.offset;
        let selection_snapshot = child.selection.clone();
        if entity {
            entity_keys.admit(&type_value, &selection_snapshot)?;
        }
        let mut file_id_values = strings_with_vr(item, TAG_REFERENCED_FILE_ID, Vr::CS);
        let mrdr_offset = logical_mrdr_target(item, &items, &mrdrs, legacy)?;
        if let Some(target) = mrdr_offset {
            if file_id_values.is_some() || !standard_file_record_type(&type_value) {
                return Err(invalid());
            }
            let mrdr = mrdrs.get(&target).and_then(|&i| items.get(i));
            file_id_values = Some(
                mrdr.ok_or_else(invalid)?
                    .get_strings(TAG_REFERENCED_FILE_ID)
                    .unwrap_or_default(),
            );
            let count = mrdr_counts.entry(target).or_default();
            *count = count.saturating_add(1);
        }
        let Some(file_id_values) = file_id_values else {
            if !entity {
                return Err(invalid());
            }
            if child_offset != 0 {
                pending.push(child);
            }
            continue;
        };
        if entity {
            return Err(invalid());
        }
        if child_offset != 0 {
            pending.push(TraversalFrame {
                offset: child_offset,
                parent_type: child.parent_type.clone(),
                root: false,
                selection: child.selection.clone(),
            });
        }
        let file_id = new_file_id(&file_id_values)?;
        let (class_uid, class_ok) = required_uid(item, TAG_REFERENCED_SOP_CLASS_UID_IN_FILE);
        let (instance_uid, instance_ok) =
            required_uid(item, TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE);
        let (syntax_uid, syntax_ok) =
            required_uid(item, TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE);
        if !class_ok || !instance_ok || !syntax_ok {
            return Err(invalid());
        }
        match record_type_for_sop_class(&class_uid) {
            Some(mapped) if mapped == record_type => {}
            _ => return Err(ErrorKind::UnsupportedSopClass.into()),
        }
        let mut selection = child.selection;
        selection.record_type = record_type.clone();
        selection.sop_class_uid = class_uid.clone();
        selection.sop_instance_uid = instance_uid.clone();
        selection.transfer_syntax_uid = syntax_uid.clone();
        selection.file_id = file_id.clone();
        entity_keys.admit(&type_value, &selection)?;
        let remaining = limits.max_total_record_key_bytes.saturating_sub(key_bytes);
        if remaining <= 0 {
            return Err(ErrorKind::ResourceLimit.into());
        }
        let mut key_limits = limits;
        if remaining < key_limits.max_record_key_bytes {
            key_limits.max_record_key_bytes = remaining;
        }
        let (keys, owned_bytes) = capture_directory_record_keys(item, &type_value, &key_limits)?;
        selection.keys = keys;
        selection.key_bytes = owned_bytes;
        key_bytes = key_bytes.saturating_add(owned_bytes);
        validate_directory_keys(&type_value, &selection.keys, &class_uid, Some(&limits))?;
        let related = strings_with_vr(
            item,
            TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE,
            Vr::UI,
        );
        let mut related_uids = Vec::new();
        if let Some(values) = related {
            if values.is_empty() {
                return Err(invalid());
            }
            for v in values {
                let n = normalize_uid(&v).to_string();
                if !valid_uid(&n) {
                    return Err(invalid());
                }
                related_uids.push(n);
            }
        }
        selection.related_general_sop_class_uids = related_uids.clone();
        let key = file_id.to_string();
        if let Some(prior) = file_selections.get(&key)
            && (prior.sop_class_uid != class_uid
                || prior.sop_instance_uid != instance_uid
                || prior.transfer_syntax_uid != syntax_uid
                || !encoded_selection_matches_record(prior, &selection)
                || !directory_keys_equal(&prior.keys, &selection.keys)
                || prior.related_general_sop_class_uids != related_uids)
        {
            return Err(invalid());
        }
        file_selections.insert(key.clone(), selection.clone());
        let target = mrdr_offset.unwrap_or(0);
        if (file_ids.contains(&key) || sop_instances.contains(&instance_uid))
            && (mrdr_offset.is_none() || file_targets.get(&key).copied().unwrap_or(0) != target)
        {
            return Err(invalid());
        }
        file_ids.insert(key.clone());
        file_targets.insert(key, target);
        sop_instances.insert(instance_uid.clone());
        references.push(EncodedReference {
            file_id,
            record_type,
            selection,
            sop_class_uid: class_uid,
            sop_instance_uid: instance_uid,
            transfer_syntax_uid: syntax_uid,
            related_general_sop_class_uids: related_uids,
            offset: frame.offset,
            mrdr_offset: target,
        });
    }
    for (&offset, &i) in &mrdrs {
        let count = items
            .get(i)
            .and_then(|item| uint32_value(item, TAG_NUMBER_OF_REFERENCES));
        if count != Some(mrdr_counts.get(&offset).copied().unwrap_or(0))
            || visited.contains(&offset)
        {
            return Err(invalid());
        }
        visited.insert(offset);
    }
    if visited.len() != items.len() {
        return Err(invalid());
    }
    Ok(references)
}
fn has_required_study_keys(item: &Object) -> bool {
    required_string_with_vr(item, TAG_STUDY_DATE, Vr::DA).is_some()
        && required_string_with_vr(item, TAG_STUDY_TIME, Vr::TM).is_some()
        && required_string_with_vr(item, TAG_STUDY_ID, Vr::SH).is_some()
        && string_with_vr(item, TAG_STUDY_DESCRIPTION, Vr::LO).is_some()
        && string_with_vr(item, TAG_ACCESSION_NUMBER, Vr::SH).is_some()
}
fn has_required_series_keys(item: &Object) -> bool {
    required_string_with_vr(item, TAG_MODALITY, Vr::CS).is_some()
        && required_string_with_vr(item, TAG_SERIES_NUMBER, Vr::IS).is_some()
}
fn valid_hierarchy_child(parent: &str, child: &str, root: bool) -> bool {
    if root {
        return child == "PATIENT" || root_file_record_type(child);
    }
    match parent {
        "PATIENT" => child == "STUDY",
        "STUDY" => child == "SERIES",
        "SERIES" => standard_file_record_type(child) && !root_file_record_type(child),
        _ => false,
    }
}
pub(crate) fn validate_directory_file_set_uid(dataset: &Object, uid: &str) -> Result<(), Error> {
    if !valid_uid(uid) {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let Some(items) = dataset.get_sequence(TAG_DIRECTORY_RECORD_SEQUENCE) else {
        return Err(ErrorKind::InvalidRecord.into());
    };
    for item in &items {
        let Some(flag) = uint16_value(item, TAG_RECORD_IN_USE_FLAG) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        if flag == 0 {
            continue;
        }
        for tag in [
            TAG_STUDY_INSTANCE_UID,
            TAG_SERIES_INSTANCE_UID,
            TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE,
        ] {
            if item.get_uid(tag).as_deref() == Some(uid) {
                return Err(ErrorKind::InvalidRecord.into());
            }
        }
    }
    Ok(())
}
pub(crate) fn encoded_selection_matches_record(encoded: &FileRecord, record: &FileRecord) -> bool {
    encoded.patient_name == record.patient_name
        && encoded.patient_id == record.patient_id
        && encoded.study_date == record.study_date
        && encoded.study_time == record.study_time
        && encoded.study_description == record.study_description
        && encoded.study_instance_uid == record.study_instance_uid
        && encoded.study_id == record.study_id
        && encoded.accession_number == record.accession_number
        && encoded.modality == record.modality
        && encoded.series_instance_uid == record.series_instance_uid
        && encoded.series_number == record.series_number
        && encoded.instance_number == record.instance_number
}
