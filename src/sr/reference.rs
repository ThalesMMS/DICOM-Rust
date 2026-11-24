use super::error::{Kind, ReferenceErrorInfo, SrError};
use super::relationship::lookup_relationship_iod;
use super::{
    CodedEntry, ContentItem, Document, EXTENSIBLE_SR_STORAGE, RELATIONSHIP_CONTAINS, value,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
pub type ContentItemIdentifier = Vec<u32>;
pub fn identifier_string(identifier: &[u32]) -> String {
    identifier
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}
fn identifier_valid(identifier: &[u32], max_components: i64) -> bool {
    !identifier.is_empty()
        && identifier[0] == 1
        && identifier.len() as i64 <= max_components
        && identifier.iter().all(|&c| c != 0)
}
fn ancestor_of(identifier: &[u32], other: &[u32]) -> bool {
    identifier.len() < other.len() && other[..identifier.len()] == *identifier
}
pub(crate) fn append_path(parent: &[u32], child: u32) -> ContentItemIdentifier {
    let mut out = parent.to_vec();
    out.push(child);
    out
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValidationMode(pub u8);
impl ValidationMode {
    pub const STRICT: ValidationMode = ValidationMode(1);
    pub const WARN: ValidationMode = ValidationMode(2);
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    #[default]
    Empty,
    Error,
    Warning,
}
impl DiagnosticSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticSeverity::Empty => "",
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticFinding {
    pub path: ContentItemIdentifier,
    pub target: ContentItemIdentifier,
    pub rule_id: String,
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub sop_class_uid: String,
    pub source_value_type: String,
    pub relationship_type: String,
    pub target_value_type: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub findings: Vec<DiagnosticFinding>,
    pub truncated: bool,
}
impl ValidationReport {
    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == DiagnosticSeverity::Error)
    }
}
pub const REFERENCE_CODE_INVALID_PATH: &str = "sr.reference.invalid_path";
pub const REFERENCE_CODE_DANGLING: &str = "sr.reference.dangling";
pub const REFERENCE_CODE_SELF: &str = "sr.reference.self";
pub const REFERENCE_CODE_ANCESTOR: &str = "sr.reference.ancestor";
pub const REFERENCE_CODE_CYCLE: &str = "sr.reference.cycle";
pub const REFERENCE_CODE_FORBIDDEN_PROFILE: &str = "sr.reference.forbidden_profile";
pub const REFERENCE_CODE_FORBIDDEN_RELATION: &str = "sr.reference.forbidden_relationship";
pub const REFERENCE_CODE_INCOMPATIBLE: &str = "sr.reference.incompatible_target";
pub const REFERENCE_CODE_BY_VALUE_MACRO: &str = "sr.reference.by_value_macro";
pub const REFERENCE_CODE_MISSING_RELATION: &str = "sr.reference.missing_relationship";
pub const REFERENCE_CODE_RESOURCE_LIMIT: &str = "sr.reference.resource_limit";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReferenceOptions {
    pub mode: ValidationMode,
    pub max_depth: i64,
    pub max_items: i64,
    pub max_references: i64,
    pub max_path_components: i64,
    pub max_findings: i64,
}
pub fn default_reference_options() -> ReferenceOptions {
    ReferenceOptions {
        mode: ValidationMode::STRICT,
        max_depth: 64,
        max_items: 100_000,
        max_references: 100_000,
        max_path_components: 65,
        max_findings: 1_024,
    }
}
fn normalize_reference_options(mut o: ReferenceOptions) -> ReferenceOptions {
    let d = default_reference_options();
    if o.mode.0 == 0 {
        o.mode = d.mode;
    }
    if o.max_depth == 0 {
        o.max_depth = d.max_depth;
    }
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_references == 0 {
        o.max_references = d.max_references;
    }
    if o.max_path_components == 0 {
        o.max_path_components = d.max_path_components;
    }
    if o.max_findings == 0 {
        o.max_findings = d.max_findings;
    }
    o
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceEdge {
    pub source: ContentItemIdentifier,
    pub slot: ContentItemIdentifier,
    pub target: ContentItemIdentifier,
    pub relationship_type: String,
}
#[derive(Clone, Debug)]
pub struct ReferenceIndex {
    fingerprint: [u8; 32],
    options: ReferenceOptions,
    edges: Vec<ReferenceEdge>,
    by_source: HashMap<Vec<u32>, Vec<usize>>,
    by_target: HashMap<Vec<u32>, Vec<usize>>,
    by_slot: HashMap<Vec<u32>, usize>,
}
struct WalkEntry<'a> {
    item: &'a ContentItem,
    parent: ContentItemIdentifier,
    path: ContentItemIdentifier,
    depth: i64,
}
struct WalkFrame<'a> {
    items: &'a [ContentItem],
    parent: ContentItemIdentifier,
    next: usize,
    depth: i64,
}
struct Walker<'a> {
    frames: Vec<WalkFrame<'a>>,
}
impl<'a> Walker<'a> {
    fn new(items: &'a [ContentItem], parent: ContentItemIdentifier, depth: i64) -> Walker<'a> {
        let mut w = Walker { frames: Vec::new() };
        w.push(items, parent, depth);
        w
    }
    fn push(&mut self, items: &'a [ContentItem], parent: ContentItemIdentifier, depth: i64) {
        if items.is_empty() {
            return;
        }
        self.frames.push(WalkFrame {
            items,
            parent,
            next: 0,
            depth,
        });
    }
    fn next(&mut self) -> Option<WalkEntry<'a>> {
        while let Some(frame) = self.frames.last_mut() {
            if frame.next >= frame.items.len() {
                self.frames.pop();
                continue;
            }
            let index = frame.next;
            frame.next += 1;
            let path = append_path(&frame.parent, index as u32 + 1);
            return Some(WalkEntry {
                item: &frame.items[index],
                parent: frame.parent.clone(),
                path,
                depth: frame.depth,
            });
        }
        None
    }
}
pub fn resolve_references(
    document: Option<&Document>,
    options: ReferenceOptions,
) -> (Option<ReferenceIndex>, ValidationReport, Option<SrError>) {
    let options = normalize_reference_options(options);
    if options.mode != ValidationMode::STRICT && options.mode != ValidationMode::WARN {
        return (
            None,
            ValidationReport::default(),
            Some(SrError::new(
                Kind::ReferenceResolution,
                ": invalid validation mode",
            )),
        );
    }
    if options.max_depth < 1
        || options.max_items < 1
        || options.max_references < 1
        || options.max_path_components < 1
        || options.max_findings < 1
    {
        return (
            None,
            ValidationReport::default(),
            Some(SrError::new(
                Kind::ReferenceResolution,
                ": invalid resource limits",
            )),
        );
    }
    let Some(document) = document else {
        return (
            None,
            ValidationReport::default(),
            Some(SrError::new(Kind::ReferenceResolution, ": nil document")),
        );
    };
    let mut edges: Vec<ReferenceEdge> = Vec::new();
    let mut report = ValidationReport::default();
    let root: Vec<u32> = vec![1];
    let mut nodes: HashMap<Vec<u32>, String> = HashMap::new();
    nodes.insert(root.clone(), value::CONTAINER.into());
    let mut node_order: Vec<Vec<u32>> = vec![root.clone()];
    let mut walker = Walker::new(&document.content, root.clone(), 1);
    let mut item_count = 0i64;
    let mut reference_count = 0i64;
    let mut resource_limited = false;
    let mut invalid_slots: std::collections::HashSet<Vec<u32>> = Default::default();
    while let Some(entry) = walker.next() {
        item_count += 1;
        if item_count > options.max_items || entry.depth > options.max_depth {
            add_reference_resource_finding(&mut report, &options, &entry.path);
            resource_limited = true;
            break;
        }
        let item = entry.item;
        if item.encoding.empty_reference_macro_present
            || !item.referenced_content_item_identifier.is_empty()
        {
            reference_count += 1;
            if reference_count > options.max_references {
                add_reference_resource_finding(&mut report, &options, &entry.path);
                resource_limited = true;
                break;
            }
            let edge = ReferenceEdge {
                source: entry.parent.clone(),
                slot: entry.path.clone(),
                target: item.referenced_content_item_identifier.clone(),
                relationship_type: item.relationship_type.clone(),
            };
            if let Err(e) = validate_encoded_reference_item(item, options.max_path_components) {
                let code = e
                    .reference
                    .as_ref()
                    .map(|r| r.code.clone())
                    .filter(|c| !c.is_empty())
                    .unwrap_or_else(|| REFERENCE_CODE_BY_VALUE_MACRO.into());
                add_reference_finding(&mut report, &options, &code, &edge.slot, &edge.target);
                invalid_slots.insert(edge.slot.clone());
            }
            if !invalid_slots.contains(&edge.slot)
                && !identifier_valid(&edge.target, options.max_path_components)
            {
                add_reference_finding(
                    &mut report,
                    &options,
                    REFERENCE_CODE_INVALID_PATH,
                    &edge.slot,
                    &edge.target,
                );
                invalid_slots.insert(edge.slot.clone());
            }
            edges.push(edge);
            continue;
        }
        nodes.insert(entry.path.clone(), item.value_type.clone());
        node_order.push(entry.path.clone());
        walker.push(&item.children, entry.path, entry.depth + 1);
    }
    if resource_limited {
        return (
            None,
            report,
            Some(SrError::new(Kind::ResourceLimitExceeded, "").wrapped(Kind::ReferenceResolution)),
        );
    }
    let mut cycle_edges = Vec::new();
    for edge in &edges {
        if invalid_slots.contains(&edge.slot) {
            continue;
        }
        let source = nodes.get(&edge.source).cloned().unwrap_or_default();
        let Some(target) = nodes.get(&edge.target).cloned() else {
            add_reference_finding(
                &mut report,
                &options,
                REFERENCE_CODE_DANGLING,
                &edge.slot,
                &edge.target,
            );
            continue;
        };
        if edge.target == edge.source {
            add_reference_finding(
                &mut report,
                &options,
                REFERENCE_CODE_SELF,
                &edge.slot,
                &edge.target,
            );
            continue;
        }
        if ancestor_of(&edge.target, &edge.source) {
            add_reference_finding(
                &mut report,
                &options,
                REFERENCE_CODE_ANCESTOR,
                &edge.slot,
                &edge.target,
            );
            continue;
        }
        let code = if profile_forbids_by_reference(&document.sop_class_uid) {
            REFERENCE_CODE_FORBIDDEN_PROFILE
        } else if reference_relationship_forbidden(&document.sop_class_uid, &edge.relationship_type)
        {
            REFERENCE_CODE_FORBIDDEN_RELATION
        } else if !reference_relationship_allowed(
            &document.sop_class_uid,
            &source,
            &edge.relationship_type,
            &target,
        ) {
            REFERENCE_CODE_INCOMPATIBLE
        } else {
            ""
        };
        if !code.is_empty() {
            add_reference_relationship_finding(
                &mut report,
                &options,
                code,
                edge,
                &document.sop_class_uid,
                &source,
                &target,
            );
            continue;
        }
        cycle_edges.push(edge.clone());
    }
    detect_reference_cycles(&cycle_edges, &nodes, &node_order, &mut report, &options);
    let mut index = ReferenceIndex {
        fingerprint: [0; 32],
        options,
        edges,
        by_source: HashMap::new(),
        by_target: HashMap::new(),
        by_slot: HashMap::new(),
    };
    for (i, edge) in index.edges.iter().enumerate() {
        index
            .by_source
            .entry(edge.source.clone())
            .or_default()
            .push(i);
        index
            .by_target
            .entry(edge.target.clone())
            .or_default()
            .push(i);
        index.by_slot.insert(edge.slot.clone(), i);
    }
    match fingerprint_document(document, &options) {
        Some(f) => index.fingerprint = f,
        None => {
            add_reference_resource_finding(&mut report, &options, &root);
            return (
                None,
                report,
                Some(
                    SrError::new(Kind::ResourceLimitExceeded, "")
                        .wrapped(Kind::ReferenceResolution),
                ),
            );
        }
    }
    if options.mode == ValidationMode::STRICT && report.has_errors() {
        let finding = report.findings[0].clone();
        let source = index
            .by_slot
            .get(&finding.path)
            .map(|&i| index.edges[i].source.clone())
            .unwrap_or_default();
        let err = SrError::reference(ReferenceErrorInfo {
            code: finding.code.clone(),
            source,
            slot: finding.path.clone(),
            target: finding.target.clone(),
        });
        return (Some(index), report, Some(err));
    }
    (Some(index), report, None)
}
fn reference_relationship_forbidden(sop_class_uid: &str, relationship: &str) -> bool {
    let Some(iod) = lookup_relationship_iod(sop_class_uid) else {
        return false;
    };
    if iod.sop_class_uid == EXTENSIBLE_SR_STORAGE && relationship == RELATIONSHIP_CONTAINS {
        return true;
    }
    !iod.allows_by_reference(relationship)
}
fn add_reference_relationship_finding(
    report: &mut ValidationReport,
    options: &ReferenceOptions,
    code: &str,
    edge: &ReferenceEdge,
    sop_class_uid: &str,
    source: &str,
    target: &str,
) {
    let before = report.findings.len();
    add_reference_finding(report, options, code, &edge.slot, &edge.target);
    if report.findings.len() == before {
        return;
    }
    let finding = report.findings.last_mut().expect("finding appended");
    if let Some(iod) = lookup_relationship_iod(sop_class_uid) {
        finding.rule_id = iod.rule_id();
        finding.sop_class_uid = iod.sop_class_uid.into();
    }
    finding.source_value_type = source.into();
    finding.relationship_type = edge.relationship_type.clone();
    finding.target_value_type = target.into();
}
fn add_path_finding(
    report: &mut ValidationReport,
    options: &ReferenceOptions,
    mut finding: DiagnosticFinding,
) {
    if report.findings.len() as i64 >= options.max_findings {
        report.truncated = true;
        return;
    }
    finding.severity = if options.mode == ValidationMode::WARN {
        DiagnosticSeverity::Warning
    } else {
        DiagnosticSeverity::Error
    };
    report.findings.push(finding);
}
fn add_reference_finding(
    report: &mut ValidationReport,
    options: &ReferenceOptions,
    code: &str,
    slot: &[u32],
    target: &[u32],
) {
    add_path_finding(
        report,
        options,
        DiagnosticFinding {
            path: slot.to_vec(),
            target: target.to_vec(),
            rule_id: "PS3.3.C.17.3.4".into(),
            code: code.into(),
            severity: DiagnosticSeverity::Error,
            message: reference_message(code).into(),
            ..Default::default()
        },
    );
}
fn add_reference_resource_finding(
    report: &mut ValidationReport,
    options: &ReferenceOptions,
    path: &[u32],
) {
    let before = report.findings.len();
    add_reference_finding(report, options, REFERENCE_CODE_RESOURCE_LIMIT, path, &[]);
    if report.findings.len() > before {
        report
            .findings
            .last_mut()
            .expect("finding appended")
            .severity = DiagnosticSeverity::Error;
    }
}
fn reference_message(code: &str) -> &'static str {
    match code {
        REFERENCE_CODE_INVALID_PATH => "referenced content item identifier is invalid",
        REFERENCE_CODE_DANGLING => "referenced content item does not exist",
        REFERENCE_CODE_SELF => "content item references itself",
        REFERENCE_CODE_ANCESTOR => "content item references an ancestor",
        REFERENCE_CODE_CYCLE => "content item references form a cycle",
        REFERENCE_CODE_FORBIDDEN_PROFILE => "SOP class forbids by-reference relationships",
        REFERENCE_CODE_FORBIDDEN_RELATION => "relationship type cannot be conveyed by-reference",
        REFERENCE_CODE_INCOMPATIBLE => "source and target value types are incompatible",
        REFERENCE_CODE_BY_VALUE_MACRO => "by-reference item also carries by-value content",
        REFERENCE_CODE_MISSING_RELATION => "by-reference item has no relationship type",
        _ => "reference validation resource limit exceeded",
    }
}
fn profile_forbids_by_reference(sop_class_uid: &str) -> bool {
    lookup_relationship_iod(sop_class_uid).is_some_and(|i| i.by_reference == "none")
}
fn reference_relationship_allowed(
    sop_class_uid: &str,
    source: &str,
    relationship: &str,
    target: &str,
) -> bool {
    if relationship.is_empty() {
        return false;
    }
    match lookup_relationship_iod(sop_class_uid) {
        None => true,
        Some(iod) => iod.allows(source, relationship, target),
    }
}
pub(super) fn has_image_reference(r: &super::ImageReference) -> bool {
    !r.sop_class_uid.is_empty() || !r.sop_instance_uid.is_empty() || !r.frames.is_empty()
}
pub(super) fn validate_encoded_reference_item(
    item: &ContentItem,
    max_path_components: i64,
) -> Result<(), SrError> {
    let identifier = &item.referenced_content_item_identifier;
    let fail = |code: &str| {
        Err(SrError::reference(ReferenceErrorInfo {
            code: code.into(),
            target: identifier.clone(),
            ..Default::default()
        }))
    };
    if identifier.is_empty() || !identifier_valid(identifier, max_path_components) {
        return fail(REFERENCE_CODE_INVALID_PATH);
    }
    if item.relationship_type.is_empty() {
        return fail(REFERENCE_CODE_MISSING_RELATION);
    }
    let w = &item.waveform;
    let t = &item.temporal;
    if item.encoding.by_value_macro_present
        || !item.value_type.is_empty()
        || !item.concept_name.empty()
        || !item.text.is_empty()
        || !item.code.empty()
        || item.measurement.is_some()
        || !item.numeric_value_qualifier.empty()
        || !item.date_time.is_empty()
        || !item.date.is_empty()
        || !item.time.is_empty()
        || !item.uid.is_empty()
        || !item.person_name.is_empty()
        || has_image_reference(&item.image)
        || !item.composite.sop_class_uid.is_empty()
        || !item.composite.sop_instance_uid.is_empty()
        || !w.composite.sop_class_uid.is_empty()
        || !w.composite.sop_instance_uid.is_empty()
        || !w.channels.is_empty()
        || !item.spatial.graphic_type.is_empty()
        || !item.spatial.coordinates.is_empty()
        || !item.spatial3d.graphic_type.is_empty()
        || !item.spatial3d.coordinates.is_empty()
        || !t.range_type.is_empty()
        || !t.sample_positions.is_empty()
        || !t.time_offsets.is_empty()
        || !t.date_times.is_empty()
        || !item.value_elements.is_empty()
        || !item.continuity_of_content.is_empty()
        || !item.children.is_empty()
    {
        return fail(REFERENCE_CODE_BY_VALUE_MACRO);
    }
    Ok(())
}
fn detect_reference_cycles(
    edges: &[ReferenceEdge],
    nodes: &HashMap<Vec<u32>, String>,
    node_order: &[Vec<u32>],
    report: &mut ValidationReport,
    options: &ReferenceOptions,
) {
    let mut adjacency: HashMap<&[u32], Vec<usize>> = HashMap::new();
    for (i, edge) in edges.iter().enumerate() {
        if !nodes.contains_key(&edge.source) || !nodes.contains_key(&edge.target) {
            continue;
        }
        adjacency.entry(&edge.source).or_default().push(i);
    }
    let mut colors: HashMap<&[u32], u8> = HashMap::new();
    for start in node_order {
        if colors.get(start.as_slice()).copied().unwrap_or(0) != 0 {
            continue;
        }
        colors.insert(start, 1);
        let mut stack: Vec<(&[u32], usize)> = vec![(start, 0)];
        while let Some(frame) = stack.last_mut() {
            let node = frame.0;
            let list = adjacency.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if frame.1 >= list.len() {
                colors.insert(node, 2);
                stack.pop();
                continue;
            }
            let edge = &edges[list[frame.1]];
            frame.1 += 1;
            let target: &[u32] = &edge.target;
            match colors.get(target).copied().unwrap_or(0) {
                0 => {
                    colors.insert(target, 1);
                    stack.push((target, 0));
                }
                1 => {
                    add_reference_finding(
                        report,
                        options,
                        REFERENCE_CODE_CYCLE,
                        &edge.slot,
                        &edge.target,
                    );
                    return;
                }
                _ => {}
            }
        }
    }
}
impl ReferenceIndex {
    fn check_current(&self, document: &Document) -> Result<(), SrError> {
        match fingerprint_document(document, &self.options) {
            Some(f) if f == self.fingerprint => Ok(()),
            _ => Err(SrError::new(Kind::StaleReferenceIndex, "")),
        }
    }
    pub fn edges(&self, document: &Document) -> Result<Vec<ReferenceEdge>, SrError> {
        self.check_current(document)?;
        Ok(self.edges.clone())
    }
    fn edges_for(&self, list: Option<&Vec<usize>>) -> Vec<ReferenceEdge> {
        list.map(|l| l.iter().map(|&i| self.edges[i].clone()).collect())
            .unwrap_or_default()
    }
    pub fn targets_from(
        &self,
        document: &Document,
        source: &[u32],
    ) -> Result<Vec<ReferenceEdge>, SrError> {
        self.check_current(document)?;
        Ok(self.edges_for(self.by_source.get(source)))
    }
    pub fn references_to(
        &self,
        document: &Document,
        target: &[u32],
    ) -> Result<Vec<ReferenceEdge>, SrError> {
        self.check_current(document)?;
        Ok(self.edges_for(self.by_target.get(target)))
    }
    pub fn target_for(
        &self,
        document: &Document,
        slot: &[u32],
    ) -> Result<ContentItemIdentifier, SrError> {
        self.check_current(document)?;
        match self.by_slot.get(slot) {
            Some(&i) => Ok(self.edges[i].target.clone()),
            None => Err(SrError::reference(ReferenceErrorInfo {
                code: REFERENCE_CODE_DANGLING.into(),
                slot: slot.to_vec(),
                ..Default::default()
            })),
        }
    }
}
fn fingerprint_document(document: &Document, options: &ReferenceOptions) -> Option<[u8; 32]> {
    let mut h = Sha256::new();
    hash_string(&mut h, &document.sop_class_uid);
    let mut walker = Walker::new(&document.content, vec![1], 1);
    let mut count = 0i64;
    while let Some(entry) = walker.next() {
        count += 1;
        if count > options.max_items || entry.depth > options.max_depth {
            return None;
        }
        hash_identifier(&mut h, &entry.path);
        hash_content_item(&mut h, entry.item);
        walker.push(&entry.item.children, entry.path, entry.depth + 1);
    }
    Some(h.finalize().into())
}
fn hash_content_item(h: &mut Sha256, item: &ContentItem) {
    hash_string(h, &item.value_type);
    hash_string(h, &item.relationship_type);
    hash_code(h, &item.concept_name);
    hash_string(h, &item.text);
    hash_code(h, &item.code);
    if let Some(m) = &item.measurement {
        hash_u64(h, m.value.to_bits());
        hash_code(h, &m.units);
    }
    hash_code(h, &item.numeric_value_qualifier);
    for s in [
        &item.date_time,
        &item.date,
        &item.time,
        &item.uid,
        &item.person_name,
        &item.image.sop_class_uid,
        &item.image.sop_instance_uid,
        &item.composite.sop_class_uid,
        &item.composite.sop_instance_uid,
        &item.continuity_of_content,
    ] {
        hash_string(h, s);
    }
    hash_identifier(h, &item.referenced_content_item_identifier);
    hash_u64(h, u64::from(item.encoding.by_value_macro_present));
    hash_u64(h, u64::from(item.encoding.empty_reference_macro_present));
    hash_u64(h, item.children.len() as u64);
    for e in &item.value_elements {
        hash_u64(h, u64::from(e.tag().key()));
        hash_string(h, e.vr().as_str());
        if let Some(raw) = e.raw() {
            hash_bytes(h, raw);
        }
    }
}
fn hash_code(h: &mut Sha256, c: &CodedEntry) {
    hash_string(h, &c.code_value);
    hash_string(h, &c.coding_scheme_designator);
    hash_string(h, &c.code_meaning);
}
fn hash_identifier(h: &mut Sha256, identifier: &[u32]) {
    hash_u64(h, identifier.len() as u64);
    for c in identifier {
        h.update(c.to_be_bytes());
    }
}
fn hash_string(h: &mut Sha256, s: &str) {
    hash_bytes(h, s.as_bytes());
}
fn hash_bytes(h: &mut Sha256, b: &[u8]) {
    hash_u64(h, b.len() as u64);
    h.update(b);
}
fn hash_u64(h: &mut Sha256, v: u64) {
    h.update(v.to_be_bytes());
}
