use super::reference::{
    ContentItemIdentifier, DiagnosticFinding, DiagnosticSeverity, REFERENCE_CODE_DANGLING,
    REFERENCE_CODE_INCOMPATIBLE, REFERENCE_CODE_INVALID_PATH, ValidationMode, ValidationReport,
    append_path, identifier_string,
};
use super::template_metadata::{
    CodeContext, CodeKey, ContentItemCodeContexts, MAX_REGISTRY_STRING, TEMPLATE_CODE_CARDINALITY,
    TEMPLATE_CODE_CODE_CONTEXT, TEMPLATE_CODE_CONDITION_FAILED,
    TEMPLATE_CODE_CONDITION_UNSUPPORTED, TEMPLATE_CODE_CONTEXT_GROUP, TEMPLATE_CODE_ORDER,
    TEMPLATE_CODE_REFERENCE_MODE, TEMPLATE_CODE_RESOURCE_LIMIT,
    TEMPLATE_CODE_TEMPLATE_IDENTIFICATION, TEMPLATE_CODE_TEMPLATE_NOT_FOUND,
    TEMPLATE_CODE_UNKNOWN_CONTENT, TemplateIdentification, TemplateValidationMetadata,
    code_key_for_entry, template_validation_error, valid_code_context_shape, valid_code_key,
    valid_equivalents, valid_template_identification,
};
use super::{ContentItem, Document, Kind, SrError, value};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
pub const TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION: &str =
    "sr.template_registry.invalid_definition";
pub const TEMPLATE_REGISTRY_CODE_DUPLICATE: &str = "sr.template_registry.duplicate";
pub const TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE: &str = "sr.template_registry.missing_include";
pub const TEMPLATE_REGISTRY_CODE_INCLUDE_CYCLE: &str = "sr.template_registry.include_cycle";
pub const TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT: &str = "sr.template_registry.resource_limit";
pub(crate) const MAX_REGISTRY_DEFINITIONS: usize = 4096;
const MAX_REGISTRY_ROWS: usize = 100_000;
const MAX_REGISTRY_CODES: usize = 250_000;
const MAX_REGISTRY_INCLUDES: usize = 100_000;
const MAX_TEMPLATE_INCLUDE_DEPTH: usize = 32;
const MAX_TEMPLATE_ROW_DEPTH: usize = 64;
const MAX_RULE_ID_LENGTH: usize = 256;
pub(crate) fn registry_error(code: &str) -> SrError {
    SrError::coded(Kind::InvalidTemplateRegistry, code)
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TemplateKey {
    pub mapping_resource: String,
    pub identifier: String,
    pub version: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextGroupKey {
    pub mapping_resource: String,
    pub identifier: String,
    pub version: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefinitionProvenance {
    pub source: String,
    pub checksum: String,
}
pub mod reference_mode {
    pub const BY_VALUE: u8 = 1;
    pub const BY_REFERENCE: u8 = 2;
    pub const EITHER: u8 = 3;
}
pub mod requirement {
    pub const REQUIRED: u8 = 1;
    pub const CONDITIONAL: u8 = 2;
    #[doc(hidden)]
    pub const OPTIONAL: u8 = 3;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cardinality {
    pub min: i64,
    pub max: i64,
}
#[derive(Clone, Debug, Default)]
pub struct TemplateConditionContext {
    pub sop_class_uid: String,
    pub parent_path: String,
    pub parent_value_type: String,
    pub child_count: i64,
    pub rule_id: String,
    pub row_counts: Arc<HashMap<String, i64>>,
}
pub type TemplateCondition =
    Arc<dyn Fn(&TemplateConditionContext) -> Result<bool, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct TemplateRow {
    pub rule_id: String,
    pub relationship_types: Vec<String>,
    pub reference_mode: u8,
    pub value_types: Vec<String>,
    pub concept_names: Vec<CodeKey>,
    pub context_group: Option<ContextGroupKey>,
    pub cardinality: Cardinality,
    pub requirement: u8,
    pub condition_id: String,
    pub condition: Option<TemplateCondition>,
    pub children: Vec<TemplateRow>,
    pub include: Option<TemplateKey>,
    pub include_fills_relationship: bool,
    pub allow_recursive_include: bool,
}
impl std::fmt::Debug for TemplateRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateRow")
            .field("rule_id", &self.rule_id)
            .field("relationship_types", &self.relationship_types)
            .field("reference_mode", &self.reference_mode)
            .field("value_types", &self.value_types)
            .field("concept_names", &self.concept_names)
            .field("context_group", &self.context_group)
            .field("cardinality", &self.cardinality)
            .field("requirement", &self.requirement)
            .field("condition_id", &self.condition_id)
            .field("condition", &self.condition.is_some())
            .field("children", &self.children)
            .field("include", &self.include)
            .finish()
    }
}
#[derive(Clone, Debug, Default)]
pub struct TemplateDefinition {
    pub key: TemplateKey,
    pub extensible: bool,
    pub order_significant: bool,
    pub rows: Vec<TemplateRow>,
    pub provenance: DefinitionProvenance,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextGroupDefinition {
    pub key: ContextGroupKey,
    pub extensible: bool,
    pub codes: Vec<CodeKey>,
    pub includes: Vec<ContextGroupKey>,
    pub provenance: DefinitionProvenance,
}
#[derive(Debug, Default)]
pub struct TemplateRegistry {
    templates: HashMap<TemplateKey, TemplateDefinition>,
    context_groups: HashMap<ContextGroupKey, ContextGroupDefinition>,
    context_group_members: HashMap<ContextGroupKey, HashSet<CodeKey>>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TemplateValidationOptions {
    pub mode: ValidationMode,
    pub max_depth: i64,
    pub max_items: i64,
    pub max_steps: i64,
    pub max_findings: i64,
}
pub fn default_template_validation_options() -> TemplateValidationOptions {
    TemplateValidationOptions {
        mode: ValidationMode::STRICT,
        max_depth: 64,
        max_items: 100_000,
        max_steps: 1_000_000,
        max_findings: 1_024,
    }
}
fn normalize_options(mut o: TemplateValidationOptions) -> TemplateValidationOptions {
    let d = default_template_validation_options();
    if o.mode.0 == 0 {
        o.mode = d.mode;
    }
    if o.max_depth == 0 {
        o.max_depth = d.max_depth;
    }
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_steps == 0 {
        o.max_steps = d.max_steps;
    }
    if o.max_findings == 0 {
        o.max_findings = d.max_findings;
    }
    o
}
#[derive(Clone, Debug)]
pub struct TemplateValidator {
    registry: Arc<TemplateRegistry>,
    options: TemplateValidationOptions,
}
pub fn new_template_validator(
    registry: Option<Arc<TemplateRegistry>>,
    options: TemplateValidationOptions,
) -> Result<TemplateValidator, SrError> {
    let Some(registry) = registry else {
        return Err(template_validation_error(TEMPLATE_CODE_TEMPLATE_NOT_FOUND));
    };
    let options = normalize_options(options);
    if options.mode != ValidationMode::STRICT && options.mode != ValidationMode::WARN {
        return Err(template_validation_error(TEMPLATE_CODE_RESOURCE_LIMIT));
    }
    if options.max_depth < 1
        || options.max_items < 1
        || options.max_steps < 1
        || options.max_findings < 1
    {
        return Err(template_validation_error(TEMPLATE_CODE_RESOURCE_LIMIT));
    }
    Ok(TemplateValidator { registry, options })
}
struct Frame<'a> {
    path: ContentItemIdentifier,
    value_type: String,
    children: &'a [ContentItem],
    rows: Vec<TemplateRow>,
    order_significant: bool,
    scope: String,
    child_scopes: Vec<String>,
}
#[derive(Clone, Copy)]
struct Lookups<'a> {
    targets: &'a HashMap<Vec<u32>, &'a ContentItem>,
    code_contexts: &'a HashMap<String, ContentItemCodeContexts>,
}
fn limit() -> SrError {
    SrError::new(Kind::ResourceLimitExceeded, "")
}
impl TemplateValidator {
    pub fn validate(
        &self,
        document: Option<&Document>,
        key: &TemplateKey,
    ) -> (ValidationReport, Option<SrError>) {
        self.validate_with_metadata(document, key, &TemplateValidationMetadata::default())
    }
    pub fn validate_with_metadata(
        &self,
        document: Option<&Document>,
        key: &TemplateKey,
        metadata: &TemplateValidationMetadata,
    ) -> (ValidationReport, Option<SrError>) {
        let o = self.options;
        let mut report = ValidationReport::default();
        let root: Vec<u32> = vec![1];
        let Some(document) = document else {
            add_finding(&mut report, &o, TEMPLATE_CODE_TEMPLATE_NOT_FOUND, &root, "");
            return self.finish(report);
        };
        let Some(definition) = self.registry.templates.get(key) else {
            add_finding(&mut report, &o, TEMPLATE_CODE_TEMPLATE_NOT_FOUND, &root, "");
            return self.finish(report);
        };
        if !super::template_metadata::metadata_item_count(metadata, o.max_items.max(0) as usize) {
            add_finding(&mut report, &o, TEMPLATE_CODE_RESOURCE_LIMIT, &root, "");
            return self.finish(report);
        }
        let mut code_contexts: HashMap<String, ContentItemCodeContexts> = HashMap::new();
        for c in &metadata.code_contexts {
            if c.path.is_empty()
                || c.path.len() > MAX_REGISTRY_STRING
                || !valid_code_context_shape(&c.concept_name)
                || !valid_code_context_shape(&c.value)
                || !valid_code_context_shape(&c.measurement_units)
                || !valid_code_context_shape(&c.numeric_value_qualifier)
                || !valid_equivalents(&c.concept_name_equivalent_codes)
                || !valid_equivalents(&c.value_equivalent_codes)
                || !valid_equivalents(&c.measurement_units_equivalent_codes)
                || !valid_equivalents(&c.numeric_value_qualifier_equivalents)
                || code_contexts.contains_key(&c.path)
            {
                add_finding(&mut report, &o, TEMPLATE_CODE_CODE_CONTEXT, &root, "");
                continue;
            }
            code_contexts.insert(c.path.clone(), c.clone());
        }
        let mut identifications: HashMap<String, TemplateIdentification> = HashMap::new();
        for t in &metadata.template_identifications {
            if !valid_template_identification(t) || identifications.contains_key(&t.path) {
                add_finding(
                    &mut report,
                    &o,
                    TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
                    &root,
                    "",
                );
                continue;
            }
            identifications.insert(t.path.clone(), t.clone());
        }
        if let Some(t) = identifications.get("1")
            && (t.mapping_resource != key.mapping_resource
                || t.identifier != key.identifier
                || (!t.version.is_empty() && t.version != key.version))
        {
            add_finding(
                &mut report,
                &o,
                TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
                &root,
                "",
            );
        }
        let root_item = ContentItem {
            value_type: value::CONTAINER.into(),
            concept_name: document.title.clone(),
            children: document.content.clone(),
            ..Default::default()
        };
        let Some(targets) = index_targets(document, &root_item, &o) else {
            add_finding(&mut report, &o, TEMPLATE_CODE_RESOURCE_LIMIT, &root, "");
            return self.finish(report);
        };
        let lookups = Lookups {
            targets: &targets,
            code_contexts: &code_contexts,
        };
        let mut stack = vec![Frame {
            path: root.clone(),
            value_type: value::CONTAINER.into(),
            children: &document.content,
            rows: definition.rows.clone(),
            order_significant: definition.order_significant,
            scope: String::new(),
            child_scopes: Vec::new(),
        }];
        let mut items = 0i64;
        let mut steps = 0i64;
        while let Some(mut frame) = stack.pop() {
            if frame.path.len() as i64 - 1 > o.max_depth {
                add_finding(
                    &mut report,
                    &o,
                    TEMPLATE_CODE_RESOURCE_LIMIT,
                    &frame.path,
                    "",
                );
                break;
            }
            match self.registry.resolve_template_level(
                &frame.rows,
                &frame.scope,
                &mut steps,
                o.max_steps,
            ) {
                Ok((rows, scopes)) => {
                    frame.rows = rows;
                    frame.child_scopes = scopes;
                }
                Err(_) => {
                    add_finding(
                        &mut report,
                        &o,
                        TEMPLATE_CODE_RESOURCE_LIMIT,
                        &frame.path,
                        "",
                    );
                    return self.finish(report);
                }
            }
            let mut counts = vec![0i64; frame.rows.len()];
            let mut last_row: i64 = -1;
            for (child_index, child) in frame.children.iter().enumerate() {
                if report.truncated {
                    return self.finish(report);
                }
                items += 1;
                if items > o.max_items {
                    add_finding(
                        &mut report,
                        &o,
                        TEMPLATE_CODE_RESOURCE_LIMIT,
                        &frame.path,
                        "",
                    );
                    break;
                }
                let child_path = append_path(&frame.path, child_index as u32 + 1);
                let child_key = identifier_string(&child_path);
                let child_contexts = code_contexts.get(&child_key).cloned().unwrap_or_default();
                let mut matched: i64 = -1;
                let mut matched_score: i64 = -(1 << 30);
                let mut mismatch_code = String::new();
                let mut mismatch_rule = String::new();
                let start = if frame.order_significant && last_row >= 0 {
                    last_row as usize
                } else {
                    0
                };
                let (mut ties, mut best_static) = (0, 0i64);
                for row_index in start..frame.rows.len() {
                    steps += 1;
                    if steps > o.max_steps {
                        add_finding(
                            &mut report,
                            &o,
                            TEMPLATE_CODE_RESOURCE_LIMIT,
                            &child_path,
                            "",
                        );
                        break;
                    }
                    let row = &frame.rows[row_index];
                    let (matches, reason) =
                        row_matches(row, child, &child_contexts, &self.registry, lookups, &o);
                    if !reason.is_empty() && mismatch_code.is_empty() {
                        mismatch_code = reason.to_string();
                        mismatch_rule = row.rule_id.clone();
                    }
                    if matches {
                        if frame.order_significant {
                            matched = row_index as i64;
                            break;
                        }
                        let score = row_selection_score(row, counts[row_index]);
                        if matched < 0 || score > matched_score {
                            matched = row_index as i64;
                            matched_score = score;
                        }
                        let st = row_static_score(row);
                        if ties == 0 || st > best_static {
                            best_static = st;
                            ties = 1;
                        } else if st == best_static {
                            ties += 1;
                        }
                    }
                }
                if steps > o.max_steps {
                    break;
                }
                if ties > 1 && !child.children.is_empty() {
                    match self.best_fitting_row(
                        document,
                        &frame,
                        &counts,
                        best_static,
                        child,
                        &child_path,
                        lookups,
                        &mut steps,
                    ) {
                        Ok(m) => matched = m,
                        Err(_) => {
                            add_finding(
                                &mut report,
                                &o,
                                TEMPLATE_CODE_RESOURCE_LIMIT,
                                &child_path,
                                "",
                            );
                            return self.finish(report);
                        }
                    }
                    if steps > o.max_steps {
                        add_finding(
                            &mut report,
                            &o,
                            TEMPLATE_CODE_RESOURCE_LIMIT,
                            &child_path,
                            "",
                        );
                        break;
                    }
                }
                if matched < 0 && frame.order_significant && start > 0 {
                    for (row_index, row) in frame.rows[..start].iter().enumerate() {
                        steps += 1;
                        if steps > o.max_steps {
                            add_finding(
                                &mut report,
                                &o,
                                TEMPLATE_CODE_RESOURCE_LIMIT,
                                &child_path,
                                "",
                            );
                            break;
                        }
                        let (matches, _) =
                            row_matches(row, child, &child_contexts, &self.registry, lookups, &o);
                        if matches {
                            matched = row_index as i64;
                            break;
                        }
                    }
                }
                if matched < 0 {
                    if !mismatch_code.is_empty() {
                        add_finding(&mut report, &o, &mismatch_code, &child_path, &mismatch_rule);
                    } else {
                        add_finding_severity(
                            &mut report,
                            &o,
                            TEMPLATE_CODE_UNKNOWN_CONTENT,
                            &child_path,
                            "",
                            definition.extensible,
                        );
                    }
                    if !child.children.is_empty() {
                        stack.push(Frame {
                            path: child_path,
                            value_type: child.value_type.clone(),
                            children: &child.children,
                            rows: Vec::new(),
                            order_significant: false,
                            scope: String::new(),
                            child_scopes: Vec::new(),
                        });
                    }
                    continue;
                }
                let m = matched as usize;
                if frame.order_significant && matched < last_row {
                    add_finding(
                        &mut report,
                        &o,
                        TEMPLATE_CODE_ORDER,
                        &child_path,
                        &frame.rows[m].rule_id,
                    );
                }
                if matched > last_row {
                    last_row = matched;
                }
                counts[m] += 1;
                if child.referenced_content_item_identifier.is_empty()
                    && (!child.children.is_empty() || !frame.rows[m].children.is_empty())
                {
                    stack.push(Frame {
                        path: child_path,
                        value_type: child.value_type.clone(),
                        children: &child.children,
                        rows: frame.rows[m].children.clone(),
                        order_significant: frame.order_significant,
                        scope: frame.child_scopes[m].clone(),
                        child_scopes: Vec::new(),
                    });
                }
            }
            let mut row_counts: Option<Arc<HashMap<String, i64>>> = None;
            for (row_index, row) in frame.rows.iter().enumerate() {
                if report.truncated {
                    return self.finish(report);
                }
                steps += 1;
                if steps > o.max_steps {
                    add_finding(
                        &mut report,
                        &o,
                        TEMPLATE_CODE_RESOURCE_LIMIT,
                        &frame.path,
                        &row.rule_id,
                    );
                    break;
                }
                let mut required = row.requirement == requirement::REQUIRED;
                if row.requirement == requirement::CONDITIONAL {
                    match &row.condition {
                        None => add_finding(
                            &mut report,
                            &o,
                            TEMPLATE_CODE_CONDITION_UNSUPPORTED,
                            &frame.path,
                            &row.rule_id,
                        ),
                        Some(condition) => {
                            let rc = row_counts
                                .get_or_insert_with(|| {
                                    Arc::new(template_row_counts(&frame.rows, &counts))
                                })
                                .clone();
                            let input = TemplateConditionContext {
                                sop_class_uid: document.sop_class_uid.clone(),
                                parent_path: identifier_string(&frame.path),
                                parent_value_type: frame.value_type.clone(),
                                child_count: frame.children.len() as i64,
                                rule_id: row.rule_id.clone(),
                                row_counts: rc,
                            };
                            match call_condition(condition, &input) {
                                Err(_) => add_finding(
                                    &mut report,
                                    &o,
                                    TEMPLATE_CODE_CONDITION_FAILED,
                                    &frame.path,
                                    &row.rule_id,
                                ),
                                Ok(active) => required = active,
                            }
                        }
                    }
                }
                let mut minimum = row.cardinality.min;
                if required && minimum < 1 {
                    minimum = 1;
                }
                if !required && counts[row_index] == 0 {
                    continue;
                }
                if counts[row_index] < minimum
                    || (row.cardinality.max > 0 && counts[row_index] > row.cardinality.max)
                {
                    add_finding(
                        &mut report,
                        &o,
                        TEMPLATE_CODE_CARDINALITY,
                        &frame.path,
                        &row.rule_id,
                    );
                }
            }
            if steps > o.max_steps || items > o.max_items {
                break;
            }
        }
        self.finish(report)
    }
    fn finish(&self, report: ValidationReport) -> (ValidationReport, Option<SrError>) {
        if report
            .findings
            .iter()
            .any(|f| f.code == TEMPLATE_CODE_RESOURCE_LIMIT)
        {
            return (
                report,
                Some(template_validation_error(TEMPLATE_CODE_RESOURCE_LIMIT)),
            );
        }
        if self.options.mode == ValidationMode::STRICT && report.has_errors() {
            let code = report.findings[0].code.clone();
            return (report, Some(template_validation_error(&code)));
        }
        (report, None)
    }
    #[allow(clippy::too_many_arguments)]
    fn best_fitting_row(
        &self,
        document: &Document,
        frame: &Frame<'_>,
        counts: &[i64],
        best: i64,
        child: &ContentItem,
        child_path: &[u32],
        lookups: Lookups<'_>,
        steps: &mut i64,
    ) -> Result<i64, SrError> {
        let (mut chosen, mut chosen_full, mut chosen_fit, mut chosen_score) =
            (-1i64, false, 0i64, 0i64);
        let contexts = lookups
            .code_contexts
            .get(&identifier_string(child_path))
            .cloned()
            .unwrap_or_default();
        for (row_index, row) in frame.rows.iter().enumerate() {
            *steps += 1;
            if *steps > self.options.max_steps {
                break;
            }
            let (matches, _) = row_matches(
                row,
                child,
                &contexts,
                &self.registry,
                lookups,
                &self.options,
            );
            if !matches || row_static_score(row) != best {
                continue;
            }
            let full = row.cardinality.max > 0 && counts[row_index] >= row.cardinality.max;
            let fit = self.child_rows_fit(
                document,
                row,
                &frame.child_scopes[row_index],
                child,
                child_path,
                lookups,
                steps,
            )?;
            let score = row_selection_score(row, counts[row_index]);
            let better = chosen < 0
                || (!full && chosen_full)
                || (full == chosen_full
                    && (fit > chosen_fit || (fit == chosen_fit && score > chosen_score)));
            if better {
                chosen = row_index as i64;
                chosen_full = full;
                chosen_fit = fit;
                chosen_score = score;
            }
        }
        Ok(chosen)
    }
    #[allow(clippy::too_many_arguments)]
    fn child_rows_fit(
        &self,
        document: &Document,
        row: &TemplateRow,
        scope: &str,
        item: &ContentItem,
        item_path: &[u32],
        lookups: Lookups<'_>,
        steps: &mut i64,
    ) -> Result<i64, SrError> {
        if !item.referenced_content_item_identifier.is_empty() {
            return Ok(0);
        }
        let (rows, _) = self.registry.resolve_template_level(
            &row.children,
            scope,
            steps,
            self.options.max_steps,
        )?;
        let mut counts = vec![0i64; rows.len()];
        let (mut matched, mut unmatched) = (0i64, 0i64);
        for (child_index, child) in item.children.iter().enumerate() {
            let child_path = append_path(item_path, child_index as u32 + 1);
            let contexts = lookups
                .code_contexts
                .get(&identifier_string(&child_path))
                .cloned()
                .unwrap_or_default();
            let (mut chosen, mut chosen_score) = (-1i64, 0i64);
            for (row_index, candidate) in rows.iter().enumerate() {
                *steps += 1;
                if *steps > self.options.max_steps {
                    return Err(limit());
                }
                let (matches, _) = row_matches(
                    candidate,
                    child,
                    &contexts,
                    &self.registry,
                    lookups,
                    &self.options,
                );
                if !matches {
                    continue;
                }
                let score = row_selection_score(candidate, counts[row_index]);
                if chosen < 0 || score > chosen_score {
                    chosen = row_index as i64;
                    chosen_score = score;
                }
            }
            if chosen < 0 {
                unmatched += 1;
                continue;
            }
            counts[chosen as usize] += 1;
            matched += 1;
        }
        let mut missing = 0i64;
        let mut row_counts: Option<Arc<HashMap<String, i64>>> = None;
        for (row_index, candidate) in rows.iter().enumerate() {
            if *steps >= self.options.max_steps {
                return Err(limit());
            }
            *steps += 1;
            let mut required = candidate.requirement == requirement::REQUIRED;
            if candidate.requirement == requirement::CONDITIONAL
                && let Some(condition) = &candidate.condition
            {
                let rc = row_counts
                    .get_or_insert_with(|| Arc::new(template_row_counts(&rows, &counts)))
                    .clone();
                let input = TemplateConditionContext {
                    sop_class_uid: document.sop_class_uid.clone(),
                    parent_path: identifier_string(item_path),
                    parent_value_type: item.value_type.clone(),
                    child_count: item.children.len() as i64,
                    rule_id: candidate.rule_id.clone(),
                    row_counts: rc,
                };
                required = matches!(call_condition(condition, &input), Ok(true));
            }
            if required && counts[row_index] < candidate.cardinality.min.max(1) {
                missing += 1;
            }
        }
        Ok(matched - unmatched - 2 * missing)
    }
}
fn template_row_counts(rows: &[TemplateRow], counts: &[i64]) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        *out.entry(row.rule_id.clone()).or_insert(0) += counts[i];
    }
    out
}
fn call_condition(
    condition: &TemplateCondition,
    input: &TemplateConditionContext,
) -> Result<bool, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| condition(input))) {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => Err("dicom/sr: template condition failed".into()),
    }
}
fn index_targets<'a>(
    document: &'a Document,
    root_item: &'a ContentItem,
    o: &TemplateValidationOptions,
) -> Option<HashMap<Vec<u32>, &'a ContentItem>> {
    let mut targets: HashMap<Vec<u32>, &ContentItem> = HashMap::new();
    targets.insert(vec![1], root_item);
    let mut stack: Vec<(&'a [ContentItem], Vec<u32>, i64, usize)> = Vec::new();
    if !document.content.is_empty() {
        stack.push((&document.content, vec![1], 1, 0));
    }
    let mut count = 0i64;
    while let Some(frame) = stack.last_mut() {
        if frame.3 >= frame.0.len() {
            stack.pop();
            continue;
        }
        let index = frame.3;
        frame.3 += 1;
        let item = &frame.0[index];
        let path = append_path(&frame.1, index as u32 + 1);
        let depth = frame.2;
        count += 1;
        if count > o.max_items || depth > o.max_depth {
            return None;
        }
        if item.referenced_content_item_identifier.is_empty() {
            targets.insert(path.clone(), item);
        }
        if !item.children.is_empty() {
            stack.push((&item.children, path, depth + 1, 0));
        }
    }
    Some(targets)
}
fn add_finding(
    report: &mut ValidationReport,
    o: &TemplateValidationOptions,
    code: &str,
    path: &[u32],
    rule_id: &str,
) {
    add_finding_severity(report, o, code, path, rule_id, false);
}
fn add_finding_severity(
    report: &mut ValidationReport,
    o: &TemplateValidationOptions,
    code: &str,
    path: &[u32],
    rule_id: &str,
    allowed_extension: bool,
) {
    if report.findings.len() as i64 >= o.max_findings {
        report.truncated = true;
        if let Some(last) = report.findings.last_mut() {
            *last = DiagnosticFinding {
                path: path.to_vec(),
                code: TEMPLATE_CODE_RESOURCE_LIMIT.into(),
                severity: DiagnosticSeverity::Error,
                message: finding_message(TEMPLATE_CODE_RESOURCE_LIMIT).into(),
                ..Default::default()
            };
        }
        return;
    }
    let severity = if code != TEMPLATE_CODE_RESOURCE_LIMIT
        && (o.mode == ValidationMode::WARN || allowed_extension)
    {
        DiagnosticSeverity::Warning
    } else {
        DiagnosticSeverity::Error
    };
    report.findings.push(DiagnosticFinding {
        path: path.to_vec(),
        rule_id: rule_id.into(),
        code: code.into(),
        severity,
        message: finding_message(code).into(),
        ..Default::default()
    });
}
fn finding_message(code: &str) -> &'static str {
    match code {
        TEMPLATE_CODE_TEMPLATE_NOT_FOUND => "template is not registered",
        TEMPLATE_CODE_CARDINALITY => "template row cardinality is not satisfied",
        TEMPLATE_CODE_UNKNOWN_CONTENT => "content item is not described by the template",
        TEMPLATE_CODE_ORDER => "content item order does not satisfy the template",
        TEMPLATE_CODE_CONDITION_UNSUPPORTED => "template condition is not available",
        TEMPLATE_CODE_CONDITION_FAILED => "template condition evaluation failed",
        TEMPLATE_CODE_REFERENCE_MODE => {
            "content item relationship mode does not satisfy the template"
        }
        TEMPLATE_CODE_CONTEXT_GROUP => "coded value does not satisfy the context group",
        TEMPLATE_CODE_CODE_CONTEXT => "encoded code context metadata is invalid",
        TEMPLATE_CODE_TEMPLATE_IDENTIFICATION => {
            "encoded template identification does not match validation template"
        }
        _ => "template validation resource limit exceeded",
    }
}
fn valid_registry_part(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_REGISTRY_STRING
}
fn valid_template_key(k: &TemplateKey) -> bool {
    valid_registry_part(&k.mapping_resource)
        && valid_registry_part(&k.identifier)
        && valid_registry_part(&k.version)
}
fn valid_context_group_key(k: &ContextGroupKey) -> bool {
    valid_registry_part(&k.mapping_resource)
        && valid_registry_part(&k.identifier)
        && valid_registry_part(&k.version)
}
fn valid_provenance(p: &DefinitionProvenance) -> bool {
    p.source.len() <= MAX_REGISTRY_STRING
        && p.checksum.len() <= MAX_REGISTRY_STRING
        && (p.source.is_empty() == p.checksum.is_empty())
}
fn valid_rule_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_RULE_ID_LENGTH
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}
fn validate_template_rows(rows: &[TemplateRow], total: &mut usize) -> Result<(), SrError> {
    let mut seen = HashSet::new();
    if rows.len() > MAX_REGISTRY_ROWS - *total {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
    }
    let mut stack: Vec<(&TemplateRow, usize)> = rows.iter().map(|r| (r, 1)).collect();
    while let Some((row, depth)) = stack.pop() {
        *total += 1;
        if *total > MAX_REGISTRY_ROWS || depth > MAX_TEMPLATE_ROW_DEPTH {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
        }
        if !valid_rule_id(&row.rule_id) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if !seen.insert(row.rule_id.as_str()) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_DUPLICATE));
        }
        if row.include.is_some() && !row.children.is_empty() {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        let c = row.cardinality;
        if row.reference_mode > reference_mode::EITHER
            || row.requirement > requirement::OPTIONAL
            || c.min < 0
            || c.max < 0
            || (c.max > 0 && c.max < c.min)
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if row.relationship_types.len() > 64
            || row.value_types.len() > 64
            || row.concept_names.len() > 4096
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
        }
        if row
            .relationship_types
            .iter()
            .any(|r| r.is_empty() || r.len() > 64)
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if row.requirement == requirement::CONDITIONAL && !valid_registry_part(&row.condition_id) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if row.concept_names.iter().any(|k| !valid_code_key(k)) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if row.children.len() as i64 > MAX_REGISTRY_ROWS as i64 - *total as i64 - stack.len() as i64
            || (depth >= MAX_TEMPLATE_ROW_DEPTH && !row.children.is_empty())
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
        }
        for child in &row.children {
            stack.push((child, depth + 1));
        }
    }
    Ok(())
}
pub fn new_template_registry(
    templates: &[TemplateDefinition],
    context_groups: &[ContextGroupDefinition],
) -> Result<TemplateRegistry, SrError> {
    if templates.len() + context_groups.len() > MAX_REGISTRY_DEFINITIONS {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
    }
    let mut registry = TemplateRegistry::default();
    let mut raw_rows = 0usize;
    for d in templates {
        if !valid_template_key(&d.key) || !valid_provenance(&d.provenance) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if registry.templates.contains_key(&d.key) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_DUPLICATE));
        }
        validate_template_rows(&d.rows, &mut raw_rows)?;
        registry.templates.insert(d.key.clone(), d.clone());
    }
    let mut raw_codes = 0usize;
    let mut raw_includes = 0usize;
    for d in context_groups {
        if !valid_context_group_key(&d.key)
            || !valid_provenance(&d.provenance)
            || d.codes.len() > MAX_REGISTRY_CODES
            || d.includes.len() > MAX_REGISTRY_INCLUDES
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
        }
        if registry.context_groups.contains_key(&d.key) {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_DUPLICATE));
        }
        if d.codes.len() > MAX_REGISTRY_CODES - raw_codes
            || d.includes.len() > MAX_REGISTRY_INCLUDES - raw_includes
        {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
        }
        raw_includes += d.includes.len();
        let mut seen = HashSet::new();
        for code in &d.codes {
            raw_codes += 1;
            if raw_codes > MAX_REGISTRY_CODES {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
            }
            if !valid_code_key(code) {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
            }
            if !seen.insert(code) {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_DUPLICATE));
            }
        }
        registry.context_groups.insert(d.key.clone(), d.clone());
    }
    validate_template_include_graph(&registry.templates)?;
    validate_context_group_includes(&registry.context_groups)?;
    validate_template_context_groups(&registry.templates, &registry.context_groups)?;
    let mut total = 0usize;
    let keys: Vec<ContextGroupKey> = registry.context_groups.keys().cloned().collect();
    for key in keys {
        let members = registry.expand_context_group(&key, &mut total)?;
        registry.context_group_members.insert(key, members);
    }
    Ok(registry)
}
impl TemplateRegistry {
    pub fn lookup_template(&self, key: &TemplateKey) -> Option<TemplateDefinition> {
        self.templates.get(key).cloned()
    }
    pub fn lookup_context_group(&self, key: &ContextGroupKey) -> Option<ContextGroupDefinition> {
        self.context_groups.get(key).cloned()
    }
    fn definitions(&self) -> (Vec<TemplateDefinition>, Vec<ContextGroupDefinition>) {
        (
            self.templates.values().cloned().collect(),
            self.context_groups.values().cloned().collect(),
        )
    }
    pub fn with_template(&self, d: TemplateDefinition) -> Result<TemplateRegistry, SrError> {
        let (mut t, g) = self.definitions();
        t.push(d);
        new_template_registry(&t, &g)
    }
    pub fn with_context_group(
        &self,
        d: ContextGroupDefinition,
    ) -> Result<TemplateRegistry, SrError> {
        let (t, mut g) = self.definitions();
        g.push(d);
        new_template_registry(&t, &g)
    }
    fn expand_context_group(
        &self,
        root: &ContextGroupKey,
        total: &mut usize,
    ) -> Result<HashSet<CodeKey>, SrError> {
        let mut members = HashSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root.clone()];
        let mut steps = 0;
        while let Some(key) = stack.pop() {
            if !seen.insert(key.clone()) {
                continue;
            }
            steps += 1;
            if steps > MAX_REGISTRY_DEFINITIONS {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
            }
            let Some(d) = self.context_groups.get(&key) else {
                continue;
            };
            for code in &d.codes {
                if members.insert(code.clone()) {
                    *total += 1;
                    if *total > MAX_REGISTRY_CODES {
                        return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
                    }
                }
            }
            stack.extend(d.includes.iter().cloned());
        }
        Ok(members)
    }
    fn context_group_contains(
        &self,
        key: &ContextGroupKey,
        code: &CodeKey,
        ctx: &CodeContext,
    ) -> (bool, &'static str) {
        let (Some(definition), Some(members)) = (
            self.context_groups.get(key),
            self.context_group_members.get(key),
        ) else {
            return (false, TEMPLATE_CODE_CODE_CONTEXT);
        };
        if !valid_code_key(code) || !valid_encoded_code_context(key, ctx) {
            return (false, TEMPLATE_CODE_CODE_CONTEXT);
        }
        if members.contains(code) {
            if ctx.extension_flag == "Y" {
                return (false, TEMPLATE_CODE_CODE_CONTEXT);
            }
            return (true, "");
        }
        if !definition.extensible {
            return (false, TEMPLATE_CODE_CONTEXT_GROUP);
        }
        if ctx.extension_flag != "Y"
            || ctx.context_group_local_version.is_empty()
            || ctx.extension_creator_uid.is_empty()
        {
            return (false, TEMPLATE_CODE_CODE_CONTEXT);
        }
        (true, "")
    }
    fn resolve_template_level(
        &self,
        rows: &[TemplateRow],
        scope: &str,
        steps: &mut i64,
        limit_steps: i64,
    ) -> Result<(Vec<TemplateRow>, Vec<String>), SrError> {
        struct Level<'a> {
            rows: &'a [TemplateRow],
            next: usize,
            scope: String,
            constraints: Vec<&'a TemplateRow>,
        }
        let mut stack = vec![Level {
            rows,
            next: 0,
            scope: scope.into(),
            constraints: Vec::new(),
        }];
        let mut out = Vec::new();
        let mut scopes = Vec::new();
        let mut seen = HashSet::new();
        while !stack.is_empty() {
            let depth = stack.len();
            let frame = stack.last_mut().expect("non-empty");
            if frame.next == frame.rows.len() {
                stack.pop();
                continue;
            }
            if *steps >= limit_steps {
                return Err(limit());
            }
            *steps += 1;
            let row = &frame.rows[frame.next];
            frame.next += 1;
            if let Some(include) = &row.include {
                if depth > MAX_TEMPLATE_INCLUDE_DEPTH
                    || !include_scope_fits(&frame.scope, &row.rule_id)
                {
                    return Err(limit());
                }
                let Some(included) = self.templates.get(include) else {
                    return Err(limit());
                };
                let mut constraints = frame.constraints.clone();
                constraints.push(row);
                let scope = scoped_rule_id(&frame.scope, &row.rule_id);
                stack.push(Level {
                    rows: &included.rows,
                    next: 0,
                    scope,
                    constraints,
                });
                continue;
            }
            if out.len() >= MAX_REGISTRY_ROWS || !include_scope_fits(&frame.scope, &row.rule_id) {
                return Err(limit());
            }
            let mut resolved = row.clone();
            resolved.rule_id = scoped_rule_id(&frame.scope, &row.rule_id);
            if seen.contains(&resolved.rule_id) {
                return Err(limit());
            }
            for include in frame.constraints.iter().rev() {
                resolved = constrain_row(include, resolved)?;
            }
            seen.insert(resolved.rule_id.clone());
            out.push(resolved);
            scopes.push(frame.scope.clone());
        }
        Ok((out, scopes))
    }
}
fn valid_encoded_code_context(key: &ContextGroupKey, c: &CodeContext) -> bool {
    if !c.long_code_value.is_empty() && !c.urn_code_value.is_empty() {
        return false;
    }
    let group = !c.context_identifier.is_empty()
        || !c.mapping_resource.is_empty()
        || !c.context_group_version.is_empty()
        || !c.context_group_local_version.is_empty()
        || !c.extension_flag.is_empty()
        || !c.extension_creator_uid.is_empty();
    if group {
        if c.context_identifier.is_empty()
            || c.mapping_resource.is_empty()
            || c.context_group_version.is_empty()
        {
            return false;
        }
        if c.context_identifier != key.identifier
            || c.mapping_resource != key.mapping_resource
            || c.context_group_version != key.version
        {
            return false;
        }
    }
    if !c.extension_flag.is_empty() && c.extension_flag != "Y" && c.extension_flag != "N" {
        return false;
    }
    if c.extension_flag == "Y" {
        return !c.context_group_local_version.is_empty() && !c.extension_creator_uid.is_empty();
    }
    c.context_group_local_version.is_empty() && c.extension_creator_uid.is_empty()
}
fn row_matches(
    row: &TemplateRow,
    item: &ContentItem,
    contexts: &ContentItemCodeContexts,
    registry: &TemplateRegistry,
    lookups: Lookups<'_>,
    o: &TemplateValidationOptions,
) -> (bool, &'static str) {
    let by_reference = !item.referenced_content_item_identifier.is_empty();
    if !row.relationship_types.is_empty()
        && !row.relationship_types.contains(&item.relationship_type)
    {
        return (false, "");
    }
    if by_reference {
        if row.reference_mode == reference_mode::BY_VALUE {
            return (false, TEMPLATE_CODE_REFERENCE_MODE);
        }
        let target_path = &item.referenced_content_item_identifier;
        let valid = target_path[0] == 1
            && target_path.len() as i64 <= o.max_depth + 1
            && target_path.iter().all(|&c| c != 0);
        if !valid {
            return (false, REFERENCE_CODE_INVALID_PATH);
        }
        let Some(target) = lookups.targets.get(target_path) else {
            return (false, REFERENCE_CODE_DANGLING);
        };
        let tc = lookups
            .code_contexts
            .get(&identifier_string(target_path))
            .cloned()
            .unwrap_or_default();
        let (matches, mut reason) = value_matches(row, target, &tc, registry);
        if !matches && reason.is_empty() {
            reason = REFERENCE_CODE_INCOMPATIBLE;
        }
        return (matches, reason);
    }
    if row.reference_mode == reference_mode::BY_REFERENCE {
        return (false, TEMPLATE_CODE_REFERENCE_MODE);
    }
    value_matches(row, item, contexts, registry)
}
fn value_matches(
    row: &TemplateRow,
    item: &ContentItem,
    contexts: &ContentItemCodeContexts,
    registry: &TemplateRegistry,
) -> (bool, &'static str) {
    if !row.value_types.is_empty() && !row.value_types.contains(&item.value_type) {
        return (false, "");
    }
    if !row.concept_names.is_empty() {
        let concept = code_key_for_entry(&item.concept_name, &contexts.concept_name);
        if !row.concept_names.contains(&concept) {
            return (false, "");
        }
    }
    if let Some(group) = &row.context_group {
        let (mut code, mut ctx) = (CodeKey::default(), CodeContext::default());
        match item.value_type.as_str() {
            value::CODE => {
                ctx = contexts.value.clone();
                code = code_key_for_entry(&item.code, &ctx);
            }
            value::NUM => {
                if let Some(m) = &item.measurement {
                    ctx = contexts.measurement_units.clone();
                    code = code_key_for_entry(&m.units, &ctx);
                }
            }
            _ => {}
        }
        let (matches, reason) = registry.context_group_contains(group, &code, &ctx);
        if !matches {
            return (false, reason);
        }
    }
    (true, "")
}
fn row_selection_score(row: &TemplateRow, current: i64) -> i64 {
    let mut score = row_static_score(row);
    if current < row.cardinality.min {
        score += 2_000;
    }
    if row.cardinality.max > 0 && current >= row.cardinality.max {
        score -= 1_000_000;
    }
    score
}
fn row_static_score(row: &TemplateRow) -> i64 {
    let mut score = 0i64;
    match row.requirement {
        requirement::REQUIRED => score += 10_000,
        requirement::CONDITIONAL => score += 5_000,
        _ => {}
    }
    if !row.concept_names.is_empty() {
        score += 1_000 - row.concept_names.len().min(999) as i64;
    }
    if row.context_group.is_some() {
        score += 500;
    }
    if !row.value_types.is_empty() {
        score += 200 - row.value_types.len().min(199) as i64;
    }
    if !row.relationship_types.is_empty() {
        score += 100 - row.relationship_types.len().min(99) as i64;
    }
    if row.reference_mode != 0 && row.reference_mode != reference_mode::EITHER {
        score += 50;
    }
    score
}
fn scoped_rule_id(scope: &str, rule_id: &str) -> String {
    if scope.is_empty() {
        return rule_id.into();
    }
    format!("{scope}.{rule_id}")
}
fn include_scope_fits(scope: &str, rule_id: &str) -> bool {
    if scope.is_empty() {
        return rule_id.len() <= MAX_RULE_ID_LENGTH;
    }
    scope.len() + 1 + rule_id.len() <= MAX_RULE_ID_LENGTH
}
fn constrain_row(include: &TemplateRow, mut row: TemplateRow) -> Result<TemplateRow, SrError> {
    if !include.relationship_types.is_empty() {
        if !include.include_fills_relationship {
            row.relationship_types = include.relationship_types.clone();
        } else if row.relationship_types.is_empty() {
            row.relationship_types = include.relationship_types.clone();
            if include.reference_mode != 0 {
                row.reference_mode = include.reference_mode;
            }
        }
    }
    row.cardinality = compose_cardinality(include.cardinality, row.cardinality)?;
    if include.requirement == requirement::CONDITIONAL
        && row.requirement == requirement::CONDITIONAL
        && include.condition_id.len() + row.condition_id.len() + 1 > MAX_REGISTRY_STRING
    {
        return Err(limit());
    }
    apply_include_requirement(include, &mut row);
    Ok(row)
}
fn compose_cardinality(include: Cardinality, row: Cardinality) -> Result<Cardinality, SrError> {
    if include == Cardinality::default() {
        return Ok(row);
    }
    let mul = |a: i64, b: i64| -> Option<i64> {
        if a == 0 || b == 0 {
            return Some(0);
        }
        if a > i64::MAX / b {
            return None;
        }
        Some(a * b)
    };
    let min = mul(include.min, row.min)
        .ok_or_else(|| registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT))?;
    let mut max = 0;
    if include.max > 0 && row.max > 0 {
        max = mul(include.max, row.max)
            .ok_or_else(|| registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT))?;
    }
    Ok(Cardinality { min, max })
}
fn apply_include_requirement(include: &TemplateRow, row: &mut TemplateRow) {
    match include.requirement {
        requirement::OPTIONAL => {
            row.requirement = requirement::OPTIONAL;
            row.condition_id.clear();
            row.condition = None;
        }
        requirement::CONDITIONAL => {
            if row.requirement == requirement::OPTIONAL {
                return;
            }
            if row.requirement == requirement::CONDITIONAL {
                row.condition_id = format!("{}+{}", include.condition_id, row.condition_id);
                row.condition =
                    combined_condition(include.condition.clone(), row.condition.clone());
                return;
            }
            row.requirement = requirement::CONDITIONAL;
            row.condition_id = include.condition_id.clone();
            row.condition = include.condition.clone();
        }
        _ => {}
    }
}
fn combined_condition(
    left: Option<TemplateCondition>,
    right: Option<TemplateCondition>,
) -> Option<TemplateCondition> {
    Some(Arc::new(move |input: &TemplateConditionContext| {
        let l = match &left {
            Some(c) => call_condition(c, input),
            None => Err("dicom/sr: template condition failed".into()),
        };
        match l {
            Ok(true) => match &right {
                Some(c) => call_condition(c, input),
                None => Err("dicom/sr: template condition failed".into()),
            },
            other => other,
        }
        .map_err(crate::BoxError::from)
    }))
}
fn graph_has_cycle<K: Eq + Hash + Clone>(
    adjacency: &HashMap<K, Vec<K>>,
    mut indegree: HashMap<K, i64>,
) -> bool {
    let mut queue: Vec<K> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut visited = 0;
    while let Some(key) = queue.pop() {
        visited += 1;
        if let Some(targets) = adjacency.get(&key) {
            for t in targets {
                let d = indegree.get_mut(t).expect("known node");
                *d -= 1;
                if *d == 0 {
                    queue.push(t.clone());
                }
            }
        }
    }
    visited != indegree.len()
}
fn validate_template_include_graph(
    templates: &HashMap<TemplateKey, TemplateDefinition>,
) -> Result<(), SrError> {
    let mut adjacency: HashMap<TemplateKey, Vec<TemplateKey>> = HashMap::new();
    let mut indegree: HashMap<TemplateKey, i64> =
        templates.keys().map(|k| (k.clone(), 0)).collect();
    let mut include_count = 0usize;
    for (key, definition) in templates {
        struct Entry<'a> {
            rows: &'a [TemplateRow],
            next: usize,
            depth: usize,
            non_by_value: bool,
        }
        let mut stack = vec![Entry {
            rows: &definition.rows,
            next: 0,
            depth: 0,
            non_by_value: false,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next == frame.rows.len() {
                stack.pop();
                continue;
            }
            let row = &frame.rows[frame.next];
            frame.next += 1;
            let (depth, non_by_value) = (frame.depth, frame.non_by_value);
            let Some(include) = &row.include else {
                if row.allow_recursive_include {
                    return Err(registry_error(TEMPLATE_REGISTRY_CODE_INVALID_DEFINITION));
                }
                if !row.children.is_empty() {
                    stack.push(Entry {
                        rows: &row.children,
                        next: 0,
                        depth: depth + 1,
                        non_by_value: non_by_value
                            || row.reference_mode != reference_mode::BY_VALUE,
                    });
                }
                continue;
            };
            include_count += 1;
            if include_count > MAX_REGISTRY_INCLUDES {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
            }
            if !templates.contains_key(include) {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE));
            }
            if row.allow_recursive_include {
                if include != key
                    || depth == 0
                    || non_by_value
                    || row.reference_mode != reference_mode::BY_VALUE
                {
                    return Err(registry_error(TEMPLATE_REGISTRY_CODE_INCLUDE_CYCLE));
                }
                continue;
            }
            adjacency
                .entry(key.clone())
                .or_default()
                .push(include.clone());
            *indegree.get_mut(include).expect("known template") += 1;
        }
    }
    if graph_has_cycle(&adjacency, indegree.clone()) {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_INCLUDE_CYCLE));
    }
    let mut depth: HashMap<TemplateKey, usize> = HashMap::new();
    let mut queue: Vec<TemplateKey> = indegree
        .iter()
        .filter(|(_, c)| **c == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut head = 0;
    while head < queue.len() {
        let key = queue[head].clone();
        head += 1;
        if let Some(targets) = adjacency.get(&key) {
            for t in targets {
                let d = depth.get(&key).copied().unwrap_or(0) + 1;
                if d > depth.get(t).copied().unwrap_or(0) {
                    depth.insert(t.clone(), d);
                }
                if depth.get(t).copied().unwrap_or(0) > MAX_TEMPLATE_INCLUDE_DEPTH {
                    return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
                }
                let c = indegree.get_mut(t).expect("known template");
                *c -= 1;
                if *c == 0 {
                    queue.push(t.clone());
                }
            }
        }
    }
    Ok(())
}
fn validate_context_group_includes(
    groups: &HashMap<ContextGroupKey, ContextGroupDefinition>,
) -> Result<(), SrError> {
    let mut adjacency: HashMap<ContextGroupKey, Vec<ContextGroupKey>> = HashMap::new();
    let mut indegree: HashMap<ContextGroupKey, i64> =
        groups.keys().map(|k| (k.clone(), 0)).collect();
    let mut count = 0usize;
    for (key, d) in groups {
        for include in &d.includes {
            count += 1;
            if count > MAX_REGISTRY_INCLUDES {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
            }
            if !groups.contains_key(include) {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE));
            }
            adjacency
                .entry(key.clone())
                .or_default()
                .push(include.clone());
            *indegree.get_mut(include).expect("known group") += 1;
        }
    }
    if graph_has_cycle(&adjacency, indegree) {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_INCLUDE_CYCLE));
    }
    Ok(())
}
fn validate_template_context_groups(
    templates: &HashMap<TemplateKey, TemplateDefinition>,
    groups: &HashMap<ContextGroupKey, ContextGroupDefinition>,
) -> Result<(), SrError> {
    for d in templates.values() {
        let mut stack: Vec<&TemplateRow> = d.rows.iter().collect();
        while let Some(row) = stack.pop() {
            stack.extend(row.children.iter());
            if let Some(g) = &row.context_group
                && !groups.contains_key(g)
            {
                return Err(registry_error(TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE));
            }
        }
    }
    Ok(())
}
pub fn current_measurement_report_template() -> TemplateDefinition {
    use super::{
        RELATIONSHIP_CONTAINS as C, RELATIONSHIP_HAS_CONCEPT_MOD as M,
        RELATIONSHIP_HAS_OBS_CONTEXT as O, RELATIONSHIP_INFERRED_FROM as I,
    };
    use crate::codes::{Code, dcm, sct};
    #[allow(clippy::too_many_arguments)]
    fn row(
        rule: &str,
        rel: &str,
        vt: &str,
        concept: Option<Code<'static>>,
        min: i64,
        max: i64,
        req: u8,
        children: Vec<TemplateRow>,
    ) -> TemplateRow {
        TemplateRow {
            rule_id: rule.into(),
            relationship_types: vec![rel.into()],
            reference_mode: reference_mode::BY_VALUE,
            value_types: vec![vt.into()],
            cardinality: Cardinality { min, max },
            requirement: req,
            children,
            concept_names: concept
                .map(|c| {
                    vec![CodeKey {
                        code_value: c.value.into(),
                        coding_scheme_designator: c.scheme.into(),
                        ..Default::default()
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        }
    }
    use requirement::{OPTIONAL as OPT, REQUIRED as REQ};
    let measurement = row(
        "measurement",
        C,
        value::NUM,
        None,
        1,
        0,
        OPT,
        vec![
            row(
                "measurement-image",
                I,
                value::IMAGE,
                Some(dcm::SourceOfMeasurement),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "measurement-spatial",
                I,
                value::SCOORD3D,
                Some(dcm::SourceOfMeasurement),
                1,
                1,
                OPT,
                vec![],
            ),
        ],
    );
    let group = row(
        "measurement-group",
        C,
        value::CONTAINER,
        Some(dcm::MeasurementGroup),
        1,
        0,
        OPT,
        vec![
            row(
                "tracking-identifier",
                O,
                value::TEXT,
                Some(dcm::TrackingIdentifier),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "tracking-unique-identifier",
                O,
                value::UIDREF,
                Some(dcm::TrackingUniqueIdentifier),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "finding",
                C,
                value::CODE,
                Some(dcm::Finding),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "referenced-segment",
                C,
                value::IMAGE,
                Some(dcm::ReferencedSegment),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "source-series-for-segmentation",
                C,
                value::UIDREF,
                Some(dcm::SourceSeriesForSegmentation),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "finding-site",
                M,
                value::CODE,
                Some(sct::FindingSite),
                1,
                0,
                OPT,
                vec![],
            ),
            measurement,
        ],
    );
    TemplateDefinition {
        key: TemplateKey {
            mapping_resource: "DCMRUST".into(),
            identifier: "CURRENT_MEASUREMENT_REPORT".into(),
            version: "2".into(),
        },
        order_significant: true,
        rows: vec![
            row(
                "language",
                M,
                value::CODE,
                Some(dcm::LanguageOfContentItemAndDescendants),
                1,
                1,
                REQ,
                vec![],
            ),
            row(
                "observer-type",
                O,
                value::CODE,
                Some(dcm::ObserverType),
                1,
                1,
                REQ,
                vec![],
            ),
            row(
                "person-observer-name",
                O,
                value::PNAME,
                Some(dcm::PersonObserverName),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "person-observer-organization",
                O,
                value::TEXT,
                Some(dcm::PersonObserversOrganizationName),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "device-observer-uid",
                O,
                value::UIDREF,
                Some(dcm::DeviceObserverUID),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "device-observer-name",
                O,
                value::TEXT,
                Some(dcm::DeviceObserverName),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "device-observer-manufacturer",
                O,
                value::TEXT,
                Some(dcm::DeviceObserverManufacturer),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "device-observer-model",
                O,
                value::TEXT,
                Some(dcm::DeviceObserverModelName),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "device-observer-serial-number",
                O,
                value::TEXT,
                Some(dcm::DeviceObserverSerialNumber),
                1,
                1,
                OPT,
                vec![],
            ),
            row(
                "procedure-reported",
                M,
                value::CODE,
                Some(dcm::ProcedureReported),
                1,
                0,
                REQ,
                vec![],
            ),
            row(
                "image-library",
                C,
                value::CONTAINER,
                Some(dcm::ImageLibrary),
                1,
                1,
                OPT,
                vec![row(
                    "image-library-group",
                    C,
                    value::CONTAINER,
                    Some(dcm::ImageLibraryGroup),
                    1,
                    0,
                    REQ,
                    vec![row(
                        "image-library-entry",
                        C,
                        value::IMAGE,
                        None,
                        1,
                        0,
                        REQ,
                        vec![],
                    )],
                )],
            ),
            row(
                "imaging-measurements",
                C,
                value::CONTAINER,
                Some(dcm::ImagingMeasurements),
                1,
                1,
                REQ,
                vec![group],
            ),
        ],
        ..Default::default()
    }
}
