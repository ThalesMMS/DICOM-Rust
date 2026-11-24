#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{text_element, text_values_element, ui_element, ul_element, us_element};
use super::error::{Error, ErrorKind};
use super::fileset::{FileRecord, SourceSnapshot, sorted_file_records};
use super::keys::directory_key_rules;
use super::record_types::{RecordType, root_file_record_type};
use super::{
    RECORD_TYPE_MRDR, TAG_DIRECTORY_RECORD_TYPE, TAG_MRDR_DIRECTORY_RECORD_OFFSET,
    TAG_NUMBER_OF_REFERENCES, TAG_OFFSET_LOWER_LEVEL_RECORD, TAG_OFFSET_NEXT_DIRECTORY_RECORD,
    TAG_RECORD_IN_USE_FLAG, TAG_REFERENCED_FILE_ID, TAG_SPECIFIC_CHARACTER_SET,
};
use crate::object::{DataSet, Element, Tag, Vr};
use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) const TAG_REFERENCED_SOP_CLASS_UID_IN_FILE: Tag = Tag::new(0x0004, 0x1510);
pub(crate) const TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE: Tag = Tag::new(0x0004, 0x1511);
pub(crate) const TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE: Tag = Tag::new(0x0004, 0x1512);
pub(crate) const TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE: Tag =
    Tag::new(0x0004, 0x151A);
pub(crate) const TAG_PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
pub(crate) const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
pub(crate) const TAG_STUDY_DATE: Tag = Tag::new(0x0008, 0x0020);
pub(crate) const TAG_STUDY_TIME: Tag = Tag::new(0x0008, 0x0030);
pub(crate) const TAG_STUDY_DESCRIPTION: Tag = Tag::new(0x0008, 0x1030);
pub(crate) const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
pub(crate) const TAG_STUDY_ID: Tag = Tag::new(0x0020, 0x0010);
pub(crate) const TAG_ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
pub(crate) const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
pub(crate) const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub(crate) const TAG_SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
pub(crate) const TAG_INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
#[derive(Clone, Debug, Default)]
pub(crate) struct RecordExtras {
    pub patient: Vec<Element>,
    pub series: Vec<Element>,
    pub image: Vec<Element>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Node {
    pub typ: RecordType,
    pub record: FileRecord,
    pub extras: RecordExtras,
    pub children: Vec<usize>,
    pub next: Option<usize>,
    pub offset: u32,
    pub imported: DataSet,
    pub mrdr: Option<usize>,
    pub references: u32,
    pub inactive: bool,
}
impl Node {
    pub fn new(typ: RecordType, record: FileRecord) -> Node {
        Node {
            typ,
            record,
            ..Default::default()
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Graph {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
    pub flat: Vec<usize>,
}
impl std::ops::Index<usize> for Graph {
    type Output = Node;
    #[allow(clippy::indexing_slicing)]
    fn index(&self, n: usize) -> &Node {
        &self.nodes[n]
    }
}
impl std::ops::IndexMut<usize> for Graph {
    #[allow(clippy::indexing_slicing)]
    fn index_mut(&mut self, n: usize) -> &mut Node {
        &mut self.nodes[n]
    }
}
impl Graph {
    pub fn push(&mut self, node: Node) -> usize {
        let n = self.nodes.len();
        self.nodes.push(node);
        n
    }
    pub fn offset(&self, node: Option<usize>) -> u32 {
        node.map_or(0, |n| self[n].offset)
    }
    pub(crate) fn first_child_offset(&self, node: usize) -> u32 {
        self.offset(self[node].children.first().copied())
    }
    pub(crate) fn leaf_nodes(&self, node: usize) -> Vec<usize> {
        if self[node].children.is_empty() {
            return vec![node];
        }
        let mut out = Vec::new();
        for &child in &self[node].children {
            out.extend(self.leaf_nodes(child));
        }
        out
    }
    pub(crate) fn record_item(&self, node: usize) -> DataSet {
        let n = &self[node];
        let mrdr = n.mrdr.map(|m| self[m].offset);
        directory_record_data_set(n, self.offset(n.next), self.first_child_offset(node), mrdr)
    }
    pub(crate) fn preorder(&self) -> Vec<usize> {
        let mut flat = Vec::new();
        fn visit(g: &Graph, nodes: &[usize], flat: &mut Vec<usize>) {
            for &n in nodes {
                flat.push(n);
                visit(g, &g[n].children, flat);
            }
        }
        visit(self, &self.roots, &mut flat);
        flat
    }
}
pub(crate) fn same_study_keys(a: &FileRecord, b: &FileRecord) -> bool {
    a.study_date == b.study_date
        && a.study_time == b.study_time
        && a.study_description == b.study_description
        && a.study_id == b.study_id
        && a.accession_number == b.accession_number
}
pub(crate) fn same_series_keys(a: &FileRecord, b: &FileRecord) -> bool {
    a.modality == b.modality && a.series_number == b.series_number
}
pub(crate) fn build_directory_hierarchy(records: &[SourceSnapshot]) -> Result<Graph, Error> {
    let records = sorted_file_records(records);
    let mut g = Graph::default();
    let mut patients: HashMap<String, usize> = HashMap::new();
    let mut studies: HashMap<String, usize> = HashMap::new();
    let mut series: HashMap<String, usize> = HashMap::new();
    for stored in &records {
        let r = &stored.record;
        if root_file_record_type(r.record_type.as_str()) {
            let n = g.push(Node::new(r.record_type.clone(), r.clone()));
            g.roots.push(n);
            continue;
        }
        let patient = match patients.get(&r.patient_id) {
            Some(&p) => {
                if g[p].record.patient_name != r.patient_name {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                p
            }
            None => {
                let p = g.push(Node::new(RecordType::PATIENT, r.clone()));
                patients.insert(r.patient_id.clone(), p);
                g.roots.push(p);
                p
            }
        };
        let study = match studies.get(&r.study_instance_uid) {
            Some(&s) => {
                let sr = &g[s].record;
                if sr.patient_id != r.patient_id || !same_study_keys(sr, r) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                s
            }
            None => {
                let s = g.push(Node::new(RecordType::STUDY, r.clone()));
                studies.insert(r.study_instance_uid.clone(), s);
                g[patient].children.push(s);
                s
            }
        };
        let series_node = match series.get(&r.series_instance_uid) {
            Some(&s) => {
                let sr = &g[s].record;
                if sr.study_instance_uid != r.study_instance_uid || !same_series_keys(sr, r) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                s
            }
            None => {
                let s = g.push(Node::new(RecordType::SERIES, r.clone()));
                series.insert(r.series_instance_uid.clone(), s);
                g[study].children.push(s);
                s
            }
        };
        let leaf = g.push(Node::new(r.record_type.clone(), r.clone()));
        g[series_node].children.push(leaf);
    }
    g.flat = g.preorder();
    link_siblings(&mut g);
    Ok(g)
}
pub(crate) fn link_siblings(g: &mut Graph) {
    let mut groups: Vec<Vec<usize>> = vec![g.roots.clone()];
    for &n in &g.flat {
        if !g[n].children.is_empty() {
            groups.push(g[n].children.clone());
        }
    }
    for group in groups {
        for w in group.windows(2) {
            if let &[a, b] = w {
                g[a].next = Some(b);
            }
        }
    }
}
pub(crate) fn directory_record_data_set(
    node: &Node,
    next: u32,
    lower: u32,
    mrdr: Option<u32>,
) -> DataSet {
    let r = &node.record;
    let mut elements = vec![
        ul_element(TAG_OFFSET_NEXT_DIRECTORY_RECORD, next),
        us_element(TAG_RECORD_IN_USE_FLAG, 0xFFFF),
        ul_element(TAG_OFFSET_LOWER_LEVEL_RECORD, lower),
        text_element(TAG_DIRECTORY_RECORD_TYPE, Vr::CS, node.typ.as_str()),
    ];
    if !node.imported.elements.is_empty() {
        elements.push(text_element(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            "ISO_IR 192",
        ));
    } else if !r.specific_character_set.is_empty() {
        elements.push(text_values_element(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            r.specific_character_set
                .split('\\')
                .map(str::to_string)
                .collect(),
        ));
    }
    match node.typ.as_str() {
        RECORD_TYPE_MRDR => {
            elements.push(text_values_element(
                TAG_REFERENCED_FILE_ID,
                Vr::CS,
                r.file_id.0.clone(),
            ));
            elements.push(ul_element(TAG_NUMBER_OF_REFERENCES, node.references));
        }
        "PATIENT" => {
            elements.push(text_element(TAG_PATIENT_NAME, Vr::PN, &r.patient_name));
            elements.push(text_element(TAG_PATIENT_ID, Vr::LO, &r.patient_id));
            elements.extend(node.extras.patient.iter().cloned());
        }
        "STUDY" => {
            elements.push(text_element(TAG_STUDY_DATE, Vr::DA, &r.study_date));
            elements.push(text_element(TAG_STUDY_TIME, Vr::TM, &r.study_time));
            elements.push(text_element(
                TAG_STUDY_DESCRIPTION,
                Vr::LO,
                &r.study_description,
            ));
            elements.push(ui_element(TAG_STUDY_INSTANCE_UID, &r.study_instance_uid));
            elements.push(text_element(TAG_STUDY_ID, Vr::SH, &r.study_id));
            elements.push(text_element(
                TAG_ACCESSION_NUMBER,
                Vr::SH,
                &r.accession_number,
            ));
        }
        "SERIES" => {
            elements.push(text_element(TAG_MODALITY, Vr::CS, &r.modality));
            elements.push(ui_element(TAG_SERIES_INSTANCE_UID, &r.series_instance_uid));
            elements.push(text_element(TAG_SERIES_NUMBER, Vr::IS, &r.series_number));
            elements.extend(node.extras.series.iter().cloned());
        }
        _ => {
            match mrdr {
                None => elements.push(text_values_element(
                    TAG_REFERENCED_FILE_ID,
                    Vr::CS,
                    r.file_id.0.clone(),
                )),
                Some(offset) => elements.push(ul_element(TAG_MRDR_DIRECTORY_RECORD_OFFSET, offset)),
            }
            elements.push(ui_element(
                TAG_REFERENCED_SOP_CLASS_UID_IN_FILE,
                &r.sop_class_uid,
            ));
            elements.push(ui_element(
                TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE,
                &r.sop_instance_uid,
            ));
            elements.push(ui_element(
                TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
                &r.transfer_syntax_uid,
            ));
            if !r.instance_number.is_empty() && r.keys.elements.is_empty() {
                elements.push(text_element(
                    TAG_INSTANCE_NUMBER,
                    Vr::IS,
                    &r.instance_number,
                ));
            }
            if !r.related_general_sop_class_uids.is_empty() {
                elements.push(text_values_element(
                    TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE,
                    Vr::UI,
                    r.related_general_sop_class_uids.clone(),
                ));
            }
            elements.extend(node.extras.image.iter().cloned());
            elements.extend(r.keys.elements.iter().cloned());
        }
    }
    let mut reserved: HashSet<Tag> = [
        TAG_REFERENCED_FILE_ID,
        TAG_REFERENCED_SOP_CLASS_UID_IN_FILE,
        TAG_REFERENCED_SOP_INSTANCE_UID_IN_FILE,
        TAG_REFERENCED_TRANSFER_SYNTAX_UID_IN_FILE,
        TAG_REFERENCED_RELATED_GENERAL_SOP_CLASS_UID_IN_FILE,
        TAG_MRDR_DIRECTORY_RECORD_OFFSET,
        TAG_NUMBER_OF_REFERENCES,
    ]
    .into_iter()
    .collect();
    for rule in directory_key_rules(node.typ.as_str()) {
        reserved.insert(rule.tag);
    }
    let mut merged: BTreeMap<Tag, Element> = BTreeMap::new();
    for e in &node.imported.elements {
        if !reserved.contains(&e.tag()) && !e.tag().is_group_length() {
            merged.insert(e.tag(), e.clone());
        }
    }
    for e in elements {
        merged.insert(e.tag(), e);
    }
    DataSet {
        elements: merged.into_values().collect(),
        item_offset: None,
    }
}
