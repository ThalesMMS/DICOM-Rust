use super::measurement_context::{
    DetailedMeasurementReport, MEASUREMENT_CONTEXT_DUPLICATE, MEASUREMENT_CONTEXT_LIMIT,
    MEASUREMENT_CONTEXT_OPAQUE_VALUES, MEASUREMENT_CONTEXT_RWVM_EXTENSION,
    MEASUREMENT_CONTEXT_SHAPE, MEASUREMENT_CONTEXT_UNSUPPORTED, MEASUREMENT_RWVM_STORAGE,
    MeasurementAlgorithm, MeasurementContextCode, MeasurementContextFinding,
    MeasurementContextOptions, MeasurementFindingSite, MeasurementGroupContext,
    MeasurementNumericContext, MeasurementQualitativeEvaluation, MeasurementQualitativeModifier,
    MeasurementReferenceEvidence, MeasurementReportContext, MeasurementTimePoint,
    bind_measurement_report_with_context, measurement_context_error, measurement_resident_scalar,
    normalize_measurement_context_options, read_measurement_reference_metadata,
};
use super::measurement_context_evidence::{read_measurement_evidence, valid_uid, walk_content};
use super::measurement_report::{
    MeasurementGroup, MeasurementReport, Observer, ReportMeasurement, SegmentReference, concept,
    measurement_sop_class,
};
use super::numeric_representation::{
    NumericRepresentation, read_document_numeric_representations, resident_string,
};
use super::read::clean_string;
use super::reference::{append_path, identifier_string};
use super::template_metadata::{CodeContext, ContentItemCodeContexts, code_key_for_entry};
use super::template_metadata::{valid_code_context_shape, valid_code_key, valid_equivalents};
use super::template_snapshot::bind_measurement_snapshot;
use super::{
    CodedEntry, ContentItem, Element, Kind, RELATIONSHIP_CONTAINS, RELATIONSHIP_HAS_CONCEPT_MOD,
    RELATIONSHIP_HAS_OBS_CONTEXT, RELATIONSHIP_INFERRED_FROM, SrError, tags as tg, value,
};
use crate::object::{Object, Tag, Value, Vr};
use std::collections::HashSet;
fn mc(value: &str, scheme: &str, meaning: &str) -> CodedEntry {
    CodedEntry::new(value, scheme, meaning)
}
pub(crate) mod context {
    use super::super::CodedEntry;
    use super::mc;
    macro_rules! concepts {
        ($($name:ident = $v:expr, $s:expr, $m:expr;)*) => {
            $(pub(crate) fn $name() -> CodedEntry { mc($v, $s, $m) })*
        };
    }
    concepts! {
        activity_session = "C67447", "NCIt", "Activity Session";
        method = "370129005", "SCT", "Measurement Method";
        laterality = "272741003", "SCT", "Laterality";
        topographical = "106233006", "SCT", "Topographical modifier";
        rwvm = "126100", "DCM", "Real World Value Map used for measurement";
        subject_time_point = "126070", "DCM", "Subject Time Point Identifier";
        protocol_time_point = "126071", "DCM", "Protocol Time Point Identifier";
        time_point = "C2348792", "UMLS", "Time Point";
        time_point_type = "126072", "DCM", "Time Point Type";
        time_point_order = "126073", "DCM", "Time Point Order";
        temporal_offset = "128740", "DCM", "Longitudinal Temporal Offset from Event";
        event_type = "128741", "DCM", "Longitudinal Temporal Event Type";
        algorithm_name = "111001", "DCM", "Algorithm Name";
        algorithm_version = "111003", "DCM", "Algorithm Version";
        algorithm_manufacturer = "122405", "DCM", "Algorithm Manufacturer";
        algorithm_parameters = "111002", "DCM", "Algorithm Parameters";
        algorithm_family = "111000", "DCM", "Algorithm Family";
        finding_category = "276214006", "SCT", "Finding category";
    }
}
pub(crate) fn measurement_context_concept(value: &str, scheme: &str, meaning: &str) -> CodedEntry {
    mc(value, scheme, meaning)
}
pub fn read_measurement_report_with_context(
    obj: Option<&Object>,
    options: MeasurementContextOptions,
) -> Result<DetailedMeasurementReport, SrError> {
    let Some(obj) = obj else {
        return Err(SrError::new(Kind::NoDataset, ""));
    };
    let o = normalize_measurement_context_options(options)?;
    admit_measurement_resident(obj.elements(), &o)?;
    let mut charset = Vec::new();
    let charset_tag = Tag::new(0x0008, 0x0005);
    if obj.has(charset_tag) {
        let values = obj
            .lookup_strings(charset_tag)
            .map_err(|_| measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, ""))?;
        if values.len() > 64 {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
        }
        charset.extend(values);
    }
    if obj.character_set().is_err() {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, ""));
    }
    let evidence = read_measurement_evidence(obj, &o)?;
    let references = read_measurement_reference_metadata(
        obj,
        o.read.max_items,
        o.read.max_depth,
        o.snapshot.max_items,
    )?;
    admit_measurement_content(obj, &o)?;
    let (result, numeric, err) = read_document_numeric_representations(Some(obj), o.read);
    if let Some(e) = err {
        return Err(measurement_context_read_error(e));
    }
    let document = result.document.unwrap_or_default();
    let snapshot = bind_measurement_snapshot(
        Some(&document),
        &result.template_metadata,
        &numeric,
        &references,
        o.snapshot,
    )?;
    let mut bind_options = o;
    bind_options.character_set = Some(charset.clone());
    bind_options.value_elements_big_endian = obj.value_big_endian();
    let mut r = bind_measurement_report_with_context(
        Some(&document),
        Some(&snapshot),
        &evidence,
        bind_options,
    )?;
    r.charset = charset;
    r.frame_of_reference_uid = clean_string(obj, Tag::new(0x0020, 0x0052));
    Ok(r)
}
fn shape(path: &str) -> SrError {
    measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, path)
}
fn admit_measurement_content(obj: &Object, o: &MeasurementContextOptions) -> Result<(), SrError> {
    match obj.get(tg::VALUE_TYPE) {
        None => return Err(shape("1")),
        Some(e) => match resident_string(e, Vr::CS) {
            Ok(v) if v.trim_matches(' ') == value::CONTAINER => {}
            _ => return Err(shape("1")),
        },
    }
    admit_measurement_code(obj, tg::CONCEPT_NAME_CODE_SEQ)?;
    fn walk(
        parent: &Object,
        depth: i64,
        count: &mut i64,
        o: &MeasurementContextOptions,
    ) -> Result<(), SrError> {
        let Some(element) = parent.get(tg::CONTENT_SEQUENCE) else {
            return Ok(());
        };
        let Value::Sequence(v) = &element.value else {
            return Err(shape(""));
        };
        if element.vr() != Vr::SQ {
            return Err(shape(""));
        }
        if v.len() as i64 > o.read.max_items - *count || (!v.is_empty() && depth > o.read.max_depth)
        {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
        }
        *count += v.len() as i64;
        let items = parent
            .get_sequence(tg::CONTENT_SEQUENCE)
            .unwrap_or_default();
        for it in &items {
            for (tag, vr) in [
                (tg::VALUE_TYPE, Vr::CS),
                (tg::RELATIONSHIP_TYPE, Vr::CS),
                (tg::TEXT_VALUE, Vr::UT),
                (tg::UID, Vr::UI),
                (tg::PERSON_NAME_VALUE, Vr::PN),
            ] {
                if let Some(e) = it.get(tag) {
                    let s = measurement_resident_scalar(e, vr).map_err(|_| shape(""))?;
                    if s.len() as i64 > o.max_string_bytes {
                        return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
                    }
                }
            }
            for tag in [
                tg::CONCEPT_NAME_CODE_SEQ,
                tg::CONCEPT_CODE_SEQ,
                tg::NUMERIC_VALUE_QUALIFIER_CODE_SEQ,
            ] {
                if it.has(tag) {
                    admit_measurement_code(it, tag)?;
                }
            }
            if let Some(e) = it.get(tg::MEASURED_VALUE_SEQ) {
                match &e.value {
                    Value::Sequence(sq) if e.vr() == Vr::SQ && sq.len() <= 1 => {}
                    _ => return Err(shape("")),
                }
                let values = it.get_sequence(tg::MEASURED_VALUE_SEQ).unwrap_or_default();
                for m in &values {
                    admit_measurement_code(m, tg::MEASUREMENT_UNITS_CODE)?;
                }
            }
            walk(it, depth + 1, count, o)?;
        }
        Ok(())
    }
    let mut count = 0i64;
    walk(obj, 1, &mut count, o)
}
pub(crate) fn admit_measurement_resident(
    elements: &[Element],
    o: &MeasurementContextOptions,
) -> Result<(), SrError> {
    struct State<'a> {
        remaining: i64,
        items: i64,
        o: &'a MeasurementContextOptions,
    }
    let limit = || measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, "");
    fn charge(st: &mut State, n: i64) -> Result<(), SrError> {
        if n < 0 || n > st.remaining {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
        }
        st.remaining -= n;
        Ok(())
    }
    fn walk(st: &mut State, elements: &[Element], depth: i64) -> Result<(), SrError> {
        let limit = || measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, "");
        if depth > 64 || elements.len() as i64 > st.o.max_evidence_items - st.items {
            return Err(limit());
        }
        st.items += elements.len() as i64;
        for e in elements {
            charge(st, 16)?;
            match &e.value {
                Value::Raw(v) => {
                    if e.vr().is_string_like() && v.len() as i64 > st.o.max_string_bytes {
                        return Err(limit());
                    }
                    charge(st, v.len() as i64)?;
                }
                Value::Strings(v) => {
                    if v.len() as i64 > st.o.max_evidence_items - st.items {
                        return Err(limit());
                    }
                    st.items += v.len() as i64;
                    for s in v {
                        if s.len() as i64 > st.o.max_string_bytes {
                            return Err(limit());
                        }
                        charge(st, s.len() as i64)?;
                    }
                }
                Value::Sequence(items) => {
                    if items.len() as i64 > st.o.max_evidence_items - st.items {
                        return Err(limit());
                    }
                    st.items += items.len() as i64;
                    for it in items {
                        walk(st, &it.elements, depth + 1)?;
                    }
                }
                _ => return Err(shape("")),
            }
        }
        Ok(())
    }
    let _ = limit;
    let mut st = State {
        remaining: o.snapshot.max_value_bytes,
        items: 0,
        o,
    };
    walk(&mut st, elements, 0)
}
fn admit_measurement_code(parent: &Object, tag: Tag) -> Result<(), SrError> {
    match parent.get(tag) {
        Some(e) if e.vr() == Vr::SQ && matches!(&e.value, Value::Sequence(sq) if sq.len() == 1) => {
        }
        _ => return Err(shape("")),
    }
    let items = parent.get_sequence(tag).unwrap_or_default();
    fn visit(it: &Object, depth: i64) -> Result<(), SrError> {
        if depth > 64 {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
        }
        let mut count = 0;
        let mut urn = false;
        for (tag, vr) in [
            (tg::CODE_VALUE, Vr::SH),
            (tg::LONG_CODE_VALUE, Vr::UC),
            (tg::URN_CODE_VALUE, Vr::UR),
        ] {
            if let Some(e) = it.get(tag) {
                match measurement_resident_scalar(e, vr) {
                    Ok(v) if !crate::object::text::trim_text_value(vr, &v).is_empty() => {}
                    _ => return Err(shape("")),
                }
                count += 1;
                urn = tag == tg::URN_CODE_VALUE;
            }
        }
        if count != 1 {
            return Err(shape(""));
        }
        for (tag, vr, required) in [
            (tg::CODING_SCHEME, Vr::SH, !urn),
            (tg::CODE_MEANING, Vr::LO, true),
            (tg::CODING_SCHEME_VERSION, Vr::SH, false),
        ] {
            if let Some(e) = it.get(tag) {
                match measurement_resident_scalar(e, vr) {
                    Ok(v) if !crate::object::text::trim_text_value(vr, &v).is_empty() => {}
                    _ => return Err(shape("")),
                }
            } else if required {
                return Err(shape(""));
            }
        }
        if it.has(tg::CODING_SCHEME_VERSION) && !it.has(tg::CODING_SCHEME) {
            return Err(shape(""));
        }
        if let Some(e) = it.get(tg::EQUIVALENT_CODE_SEQUENCE) {
            match &e.value {
                Value::Sequence(sq) if e.vr() == Vr::SQ && !sq.is_empty() => {}
                _ => return Err(shape("")),
            }
            let values = it
                .get_sequence(tg::EQUIVALENT_CODE_SEQUENCE)
                .unwrap_or_default();
            for child in &values {
                visit(child, depth + 1)?;
            }
        }
        Ok(())
    }
    visit(&items[0], 0)
}
fn measurement_context_read_error(err: SrError) -> SrError {
    if err.is(Kind::NumericRepresentation)
        || err.is(Kind::MetadataSnapshot)
        || err.is(Kind::TemplateValidation)
    {
        return err;
    }
    if err.is(Kind::ResourceLimitExceeded) {
        return measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, "");
    }
    shape("")
}
fn measurement_segment_sop_class(value: &str) -> bool {
    match crate::dictionary::uid::lookup_storage_sop_class(value) {
        Some(e) => matches!(
            e.keyword,
            "SegmentationStorage" | "SurfaceSegmentationStorage" | "LabelMapSegmentationStorage"
        ),
        None => false,
    }
}
fn measurement_opaque_values(elements: &[Element]) -> bool {
    for e in elements {
        match &e.value {
            Value::Raw(_) if !e.vr().is_string_like() => return true,
            Value::Sequence(items)
                if items
                    .iter()
                    .any(|it| measurement_opaque_values(&it.elements)) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}
impl DetailedMeasurementReport {
    pub(crate) fn opaque_output_admission(&self) -> Result<(), SrError> {
        if !self.options.value_elements_big_endian {
            return Ok(());
        }
        walk_content(&self.document.content, &[1], 1, &mut |item, path, _| {
            if measurement_opaque_values(&item.value_elements) {
                return Err(measurement_context_error(
                    MEASUREMENT_CONTEXT_OPAQUE_VALUES,
                    &identifier_string(path),
                ));
            }
            Ok(())
        })
    }
    pub(crate) fn project(
        &self,
    ) -> Result<
        (
            MeasurementReport,
            MeasurementReportContext,
            Vec<MeasurementContextFinding>,
        ),
        SrError,
    > {
        let d = &self.document;
        let mut p = Projection {
            r: self,
            unsupported: Vec::new(),
        };
        let mut c = MeasurementReportContext::default();
        let mut report = MeasurementReport {
            sop_class_uid: d.sop_class_uid.clone(),
            sop_instance_uid: d.sop_instance_uid.clone(),
            study_instance_uid: d.study_instance_uid.clone(),
            series_instance_uid: d.series_instance_uid.clone(),
            frame_of_reference_uid: self.frame_of_reference_uid.clone(),
            content_date: d.content_date.clone(),
            content_time: d.content_time.clone(),
            series_number: d.series_number.clone(),
            instance_number: d.instance_number.clone(),
            completion_flag: d.completion_flag.clone(),
            verification_flag: d.verification_flag.clone(),
            completion_flag_description: d.completion_flag_description.clone(),
            verifying_observers: d.verifying_observers.clone(),
            predecessor_documents: d.predecessor_documents.clone(),
            continuity_of_content: d.continuity_of_content.clone(),
            title: d.title.clone(),
            common: d.common.clone(),
            ..Default::default()
        };
        let root = self
            .snapshot
            .identification_index()
            .get("1")
            .cloned()
            .unwrap_or_default();
        if !measurement_sop_class(&d.sop_class_uid)
            || root.mapping_resource != "DCMR"
            || root.identifier != "1500"
        {
            return Err(shape("1"));
        }
        walk_content(&d.content, &[1], 1, &mut |item, path, _| {
            if measurement_opaque_values(&item.value_elements) {
                p.finding_at(MEASUREMENT_CONTEXT_OPAQUE_VALUES, &identifier_string(path))?;
            }
            Ok(())
        })?;
        let mut seen = HashSet::new();
        let mut observer: Option<Observer> = None;
        let mut library: Option<Vec<super::ImageReference>> = None;
        for (idx, item) in d.content.iter().enumerate() {
            let path = append_path(&[1], idx as u32 + 1);
            let key = identifier_string(&path);
            if p.matches(item, &key, &concept::language()) {
                p.row(item, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                singleton(&mut seen, "language", &key)?;
                report.language = item.code.clone();
            } else if p.matches(item, &key, &concept::procedure_reported()) {
                p.row(item, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                report.procedures_reported.push(item.code.clone());
            } else if p.matches(item, &key, &concept::observer_type()) {
                p.row(item, &key, value::CODE, RELATIONSHIP_HAS_OBS_CONTEXT)?;
                if let Some(o) = observer.take() {
                    report.observers.push(o);
                }
                observer = Some(Observer::default());
            } else if p.observer_attribute(item, &key) {
                let Some(o) = observer.as_mut() else {
                    return Err(shape(&key));
                };
                p.read_observer(item, &key, o)?;
            } else if p.matches(item, &key, &concept::image_library()) {
                p.row(item, &key, value::CONTAINER, RELATIONSHIP_CONTAINS)?;
                singleton(&mut seen, "library", &key)?;
                walk_content::<SrError>(&item.children, &path, 1, &mut |entry, _, _| {
                    if entry.value_type == value::IMAGE {
                        library
                            .get_or_insert_with(Vec::new)
                            .push(entry.image.clone());
                    }
                    Ok(())
                })?;
            } else if p.matches(item, &key, &concept::imaging_measurements()) {
                p.row(item, &key, value::CONTAINER, RELATIONSHIP_CONTAINS)?;
                singleton(&mut seen, "imaging", &key)?;
                c.imaging_algorithm = p.algorithm(&item.children, &path)?;
                for (j, g) in item.children.iter().enumerate() {
                    let gp = append_path(&path, j as u32 + 1);
                    let gk = identifier_string(&gp);
                    if p.algorithm_concept(g, &gk) > 0 {
                        continue;
                    }
                    if !p.matches(g, &gk, &concept::measurement_group()) {
                        if p.known(g, &gk) {
                            return Err(shape(&gk));
                        }
                        p.unsupported_at(&gk)?;
                        continue;
                    }
                    let identification = self
                        .snapshot
                        .identification_index()
                        .get(&gk)
                        .cloned()
                        .unwrap_or_default();
                    if identification.mapping_resource != "DCMR"
                        || (identification.identifier != "1501"
                            && identification.identifier != "1411")
                    {
                        p.unsupported_at(&gk)?;
                        continue;
                    }
                    let (group, gc) = p.group(g, &gp, &identification.identifier)?;
                    report.groups.push(group);
                    c.groups.push(gc);
                }
            } else if p.matches(item, &key, &concept::qualitative_evaluations()) {
                p.row(item, &key, value::CONTAINER, RELATIONSHIP_CONTAINS)?;
                singleton(&mut seen, "qualitative", &key)?;
                c.qualitative_algorithm = p.algorithm(&item.children, &path)?;
                for (j, q) in item.children.iter().enumerate() {
                    let qp = append_path(&path, j as u32 + 1);
                    let qk = identifier_string(&qp);
                    if p.algorithm_concept(q, &qk) > 0 {
                        continue;
                    }
                    if q.value_type == value::CODE || q.value_type == value::TEXT {
                        let v = p.qualitative(q, &qp)?;
                        c.qualitative.push(v);
                    } else {
                        p.unsupported_at(&qk)?;
                    }
                }
            } else {
                if p.known(item, &key) {
                    return Err(shape(&key));
                }
                p.unsupported_at(&key)?;
            }
        }
        if let Some(o) = observer {
            report.observers.push(o);
        }
        report.image_library = library;
        Ok((report, c, p.unsupported))
    }
}
fn singleton(seen: &mut HashSet<String>, key: &str, path: &str) -> Result<(), SrError> {
    if !seen.insert(key.to_string()) {
        return Err(measurement_context_error(
            MEASUREMENT_CONTEXT_DUPLICATE,
            path,
        ));
    }
    Ok(())
}
fn text_ok(s: &str, path: &str, max: i64) -> Result<(), SrError> {
    if s.len() as i64 > max {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, path));
    }
    for r in s.chars() {
        if r == '\0' || (r < ' ' && r != '\t' && r != '\n' && r != '\r') {
            return Err(shape(path));
        }
    }
    Ok(())
}
static EMPTY_CONTEXTS: std::sync::LazyLock<ContentItemCodeContexts> =
    std::sync::LazyLock::new(ContentItemCodeContexts::default);
struct Projection<'a> {
    r: &'a DetailedMeasurementReport,
    unsupported: Vec<MeasurementContextFinding>,
}
impl Projection<'_> {
    fn meta(&self, path: &str) -> &ContentItemCodeContexts {
        self.r
            .snapshot
            .code_index()
            .get(path)
            .unwrap_or(&EMPTY_CONTEXTS)
    }
    fn unsupported_at(&mut self, path: &str) -> Result<(), SrError> {
        self.finding_at(MEASUREMENT_CONTEXT_UNSUPPORTED, path)
    }
    fn finding_at(&mut self, code: &str, path: &str) -> Result<(), SrError> {
        if self.unsupported.len() as i64 >= self.r.options.max_context_findings {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, path));
        }
        self.unsupported.push(MeasurementContextFinding {
            code: code.to_string(),
            path: path.to_string(),
        });
        Ok(())
    }
    fn matches(&self, i: &ContentItem, path: &str, c: &CodedEntry) -> bool {
        code_key_for_entry(&i.concept_name, &self.meta(path).concept_name)
            == code_key_for_entry(c, &CodeContext::default())
    }
    fn code(&self, i: &ContentItem, path: &str, value: bool) -> MeasurementContextCode {
        let meta = self.meta(path);
        if value {
            return MeasurementContextCode {
                code: i.code.clone(),
                context: meta.value.clone(),
                equivalent_codes: meta.value_equivalent_codes.clone(),
            };
        }
        MeasurementContextCode {
            code: i.concept_name.clone(),
            context: meta.concept_name.clone(),
            equivalent_codes: meta.concept_name_equivalent_codes.clone(),
        }
    }
    fn row(&self, i: &ContentItem, path: &str, vt: &str, rel: &str) -> Result<(), SrError> {
        if i.value_type != vt
            || i.relationship_type != rel
            || !i.referenced_content_item_identifier.is_empty()
        {
            return Err(shape(path));
        }
        match vt {
            value::CODE => self.valid_code(&self.code(i, path, true), path),
            value::TEXT => {
                if i.text.is_empty() {
                    return Err(shape(path));
                }
                self.text(&i.text, path)
            }
            value::UIDREF => {
                if !valid_uid(&i.uid) {
                    return Err(shape(path));
                }
                Ok(())
            }
            value::NUM => match &i.measurement {
                Some(m) if m.value.is_finite() => Ok(()),
                _ => Err(shape(path)),
            },
            _ => Ok(()),
        }
    }
    fn text(&self, s: &str, path: &str) -> Result<(), SrError> {
        text_ok(s, path, self.r.options.max_string_bytes)
    }
    fn valid_code(&self, c: &MeasurementContextCode, path: &str) -> Result<(), SrError> {
        if !valid_code_key(&code_key_for_entry(&c.code, &c.context))
            || c.code.code_meaning.is_empty()
            || c.code.code_meaning.chars().count() > 64
            || !valid_code_context_shape(&c.context)
            || !valid_equivalents(&c.equivalent_codes)
        {
            return Err(shape(path));
        }
        self.text(&c.code.code_meaning, path)
    }
    fn known(&self, i: &ContentItem, path: &str) -> bool {
        let list = [
            context::activity_session(),
            context::method(),
            context::laterality(),
            context::topographical(),
            context::rwvm(),
            context::subject_time_point(),
            context::protocol_time_point(),
            context::time_point(),
            context::time_point_type(),
            context::time_point_order(),
            context::temporal_offset(),
            context::event_type(),
            context::algorithm_name(),
            context::algorithm_version(),
            context::algorithm_manufacturer(),
            context::algorithm_parameters(),
            context::algorithm_family(),
            concept::tracking_identifier(),
            concept::tracking_unique_identifier(),
            concept::finding(),
            concept::finding_site(),
            concept::referenced_segment(),
            concept::source_series(),
            concept::derivation(),
        ];
        list.iter().any(|c| self.matches(i, path, c))
    }
    fn observer_attribute(&self, i: &ContentItem, path: &str) -> bool {
        [
            concept::person_observer_name(),
            concept::person_observer_organization(),
            concept::device_observer_uid(),
            concept::device_observer_name(),
            concept::device_observer_manufacturer(),
            concept::device_observer_model(),
            concept::device_observer_serial(),
        ]
        .iter()
        .any(|c| self.matches(i, path, c))
    }
    fn read_observer(&self, i: &ContentItem, path: &str, o: &mut Observer) -> Result<(), SrError> {
        let mut vt = value::TEXT;
        if self.matches(i, path, &concept::person_observer_name()) {
            vt = value::PNAME;
        }
        if self.matches(i, path, &concept::device_observer_uid()) {
            vt = value::UIDREF;
        }
        self.row(i, path, vt, RELATIONSHIP_HAS_OBS_CONTEXT)?;
        if self.matches(i, path, &concept::person_observer_name()) {
            o.person_name = i.person_name.clone();
        } else if self.matches(i, path, &concept::person_observer_organization()) {
            o.organization_name = i.text.clone();
        } else if self.matches(i, path, &concept::device_observer_uid()) {
            o.device_uid = i.uid.clone();
        } else if self.matches(i, path, &concept::device_observer_name()) {
            o.device_name = i.text.clone();
        } else if self.matches(i, path, &concept::device_observer_manufacturer()) {
            o.device_manufacturer = i.text.clone();
        } else if self.matches(i, path, &concept::device_observer_model()) {
            o.device_model_name = i.text.clone();
        } else if self.matches(i, path, &concept::device_observer_serial()) {
            o.device_serial_number = i.text.clone();
        }
        Ok(())
    }
    fn algorithm_concept(&self, i: &ContentItem, path: &str) -> i32 {
        let list = [
            context::algorithm_name(),
            context::algorithm_version(),
            context::algorithm_manufacturer(),
            context::algorithm_parameters(),
            context::algorithm_family(),
        ];
        for (index, c) in list.iter().enumerate() {
            if self.matches(i, path, c) {
                if index == 0 && i.value_type == value::CODE {
                    return 2;
                }
                return [1, 3, 4, 5, 6][index];
            }
        }
        0
    }
    fn algorithm(
        &mut self,
        items: &[ContentItem],
        parent: &[u32],
    ) -> Result<Option<MeasurementAlgorithm>, SrError> {
        let mut a: Option<MeasurementAlgorithm> = None;
        let mut seen = HashSet::new();
        let mut previous = 0;
        for (idx, i) in items.iter().enumerate() {
            let path = identifier_string(&append_path(parent, idx as u32 + 1));
            let rank = self.algorithm_concept(i, &path);
            if rank == 0 {
                continue;
            }
            if rank < previous {
                return Err(shape(&path));
            }
            previous = rank;
            let alg = a.get_or_insert_with(MeasurementAlgorithm::default);
            let vt = if rank == 2 || rank == 6 {
                value::CODE
            } else {
                value::TEXT
            };
            self.row(i, &path, vt, RELATIONSHIP_HAS_CONCEPT_MOD)?;
            if !i.children.is_empty() {
                return Err(shape(&path));
            }
            if rank != 5 {
                let name = char::from(b'0' + rank as u8).to_string();
                singleton(&mut seen, &name, &path)?;
            }
            match rank {
                1 => alg.name = i.text.clone(),
                2 => alg.name_code = Some(self.code(i, &path, true)),
                3 => alg.version = i.text.clone(),
                4 => alg.manufacturer = i.text.clone(),
                5 => alg.parameters.push(i.text.clone()),
                6 => alg.family = Some(self.code(i, &path, true)),
                _ => {}
            }
        }
        if let Some(alg) = &a
            && (alg.name.is_empty() || alg.version.is_empty())
        {
            return Err(shape(&identifier_string(parent)));
        }
        Ok(a)
    }
    fn site(&mut self, i: &ContentItem, parent: &[u32]) -> Result<MeasurementFindingSite, SrError> {
        let path = identifier_string(parent);
        let mut s = MeasurementFindingSite {
            site: self.code(i, &path, true),
            ..Default::default()
        };
        self.row(i, &path, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
        let mut seen = HashSet::new();
        for (index, child) in i.children.iter().enumerate() {
            let key = identifier_string(&append_path(parent, index as u32 + 1));
            let lat = self.matches(child, &key, &context::laterality());
            let name = if lat {
                "laterality"
            } else if self.matches(child, &key, &context::topographical()) {
                "topographical"
            } else {
                if self.known(child, &key) {
                    return Err(shape(&key));
                }
                self.unsupported_at(&key)?;
                continue;
            };
            singleton(&mut seen, name, &key)?;
            self.row(child, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
            if !child.children.is_empty() {
                return Err(shape(&key));
            }
            let c = self.code(child, &key, true);
            if lat {
                s.laterality = Some(c);
            } else {
                s.topographical_modifier = Some(c);
            }
        }
        Ok(s)
    }
    fn rwvm(
        &self,
        i: &ContentItem,
        path: &str,
        rel: &str,
    ) -> Result<MeasurementReferenceEvidence, SrError> {
        self.row(i, path, value::COMPOSITE, rel)?;
        if !i.children.is_empty() || i.composite.sop_class_uid != MEASUREMENT_RWVM_STORAGE {
            return Err(shape(path));
        }
        self.r.evidence_for(
            &i.composite.sop_class_uid,
            &i.composite.sop_instance_uid,
            path,
        )
    }
    fn numeric(
        &self,
        i: &ContentItem,
        path: &str,
        rel: &str,
    ) -> Result<NumericRepresentation, SrError> {
        self.row(i, path, value::NUM, rel)?;
        let units = i
            .measurement
            .as_ref()
            .map(|m| m.units.clone())
            .unwrap_or_default();
        self.valid_code(
            &MeasurementContextCode {
                code: units,
                context: self.meta(path).measurement_units.clone(),
                equivalent_codes: Vec::new(),
            },
            path,
        )?;
        match self.r.snapshot.numeric_index().get(path) {
            Some(n) => Ok(n.clone()),
            None => Err(shape(path)),
        }
    }
    fn time_point_rank(&self, i: &ContentItem, path: &str) -> Option<usize> {
        [
            context::subject_time_point(),
            context::protocol_time_point(),
            context::time_point(),
            context::time_point_type(),
            context::time_point_order(),
            context::temporal_offset(),
        ]
        .iter()
        .position(|c| self.matches(i, path, c))
    }
    fn time_point(
        &mut self,
        items: &[ContentItem],
        parent: &[u32],
    ) -> Result<Option<MeasurementTimePoint>, SrError> {
        let mut t: Option<MeasurementTimePoint> = None;
        let mut seen = HashSet::new();
        for (idx, i) in items.iter().enumerate() {
            let ip = append_path(parent, idx as u32 + 1);
            let path = identifier_string(&ip);
            let Some(rank) = self.time_point_rank(i, &path) else {
                continue;
            };
            let tp = t.get_or_insert_with(MeasurementTimePoint::default);
            if rank != 3 {
                let name = char::from(b'0' + rank as u8).to_string();
                singleton(&mut seen, &name, &path)?;
            }
            match rank {
                0..=2 => {
                    self.row(i, &path, value::TEXT, RELATIONSHIP_HAS_OBS_CONTEXT)?;
                    if !i.children.is_empty() {
                        return Err(shape(&path));
                    }
                    match rank {
                        0 => tp.subject_identifier = i.text.clone(),
                        1 => tp.protocol_identifier = i.text.clone(),
                        _ => tp.time_point = i.text.clone(),
                    }
                }
                3 => {
                    self.row(i, &path, value::CODE, RELATIONSHIP_HAS_OBS_CONTEXT)?;
                    if !i.children.is_empty() {
                        return Err(shape(&path));
                    }
                    tp.types.push(self.code(i, &path, true));
                }
                _ => {
                    let n = self.numeric(i, &path, RELATIONSHIP_HAS_OBS_CONTEXT)?;
                    let unit = if rank == 5 { "d" } else { "1" };
                    let units = i
                        .measurement
                        .as_ref()
                        .map(|m| m.units.clone())
                        .unwrap_or_default();
                    if code_key_for_entry(&units, &self.meta(&path).measurement_units)
                        != code_key_for_entry(
                            &measurement_context_concept(unit, "UCUM", ""),
                            &CodeContext::default(),
                        )
                    {
                        return Err(shape(&path));
                    }
                    if rank == 4 {
                        if !i.children.is_empty() {
                            return Err(shape(&path));
                        }
                        tp.order = Some(n);
                    } else {
                        tp.temporal_offset = Some(n);
                        for (j, ch) in i.children.iter().enumerate() {
                            let key = identifier_string(&append_path(&ip, j as u32 + 1));
                            if !self.matches(ch, &key, &context::event_type()) {
                                self.unsupported_at(&key)?;
                                continue;
                            }
                            if tp.event_type.is_some() {
                                return Err(measurement_context_error(
                                    MEASUREMENT_CONTEXT_DUPLICATE,
                                    &key,
                                ));
                            }
                            self.row(ch, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                            if !ch.children.is_empty() {
                                return Err(shape(&key));
                            }
                            tp.event_type = Some(self.code(ch, &key, true));
                        }
                        if tp.event_type.is_none() {
                            return Err(shape(&path));
                        }
                    }
                }
            }
        }
        if let Some(tp) = &t
            && tp.time_point.is_empty()
        {
            return Err(shape(&identifier_string(parent)));
        }
        Ok(t)
    }
    fn qualitative(
        &mut self,
        i: &ContentItem,
        parent: &[u32],
    ) -> Result<MeasurementQualitativeEvaluation, SrError> {
        let path = identifier_string(parent);
        let mut q = MeasurementQualitativeEvaluation {
            concept_name: self.code(i, &path, false),
            value_type: i.value_type.clone(),
            ..Default::default()
        };
        if self.known(i, &path) || (i.value_type != value::CODE && i.value_type != value::TEXT) {
            return Err(shape(&path));
        }
        self.valid_code(&q.concept_name, &path)?;
        self.row(i, &path, &i.value_type, RELATIONSHIP_CONTAINS)?;
        if i.value_type == value::TEXT {
            q.text = i.text.clone();
        } else {
            q.code = self.code(i, &path, true);
        }
        for (idx, ch) in i.children.iter().enumerate() {
            let key = identifier_string(&append_path(parent, idx as u32 + 1));
            if i.value_type == value::CODE
                && ch.value_type == value::CODE
                && ch.relationship_type == RELATIONSHIP_HAS_CONCEPT_MOD
                && !self.known(ch, &key)
            {
                self.row(ch, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                let name = self.code(ch, &key, false);
                self.valid_code(&name, &key)?;
                if !ch.children.is_empty() {
                    return Err(shape(&key));
                }
                q.modifiers.push(MeasurementQualitativeModifier {
                    concept_name: name,
                    value: self.code(ch, &key, true),
                });
            } else {
                if self.known(ch, &key) {
                    return Err(shape(&key));
                }
                self.unsupported_at(&key)?;
            }
        }
        Ok(q)
    }
    fn scalar_children(&mut self, i: &ContentItem, path: &[u32]) -> Result<(), SrError> {
        for (idx, child) in i.children.iter().enumerate() {
            let key = identifier_string(&append_path(path, idx as u32 + 1));
            if self.known(child, &key) {
                return Err(shape(&key));
            }
            self.unsupported_at(&key)?;
        }
        Ok(())
    }
    fn group(
        &mut self,
        i: &ContentItem,
        parent: &[u32],
        template: &str,
    ) -> Result<(MeasurementGroup, MeasurementGroupContext), SrError> {
        let path = identifier_string(parent);
        let mut g = MeasurementGroup {
            template: template.to_string(),
            ..Default::default()
        };
        let mut c = MeasurementGroupContext {
            path: path.clone(),
            ..Default::default()
        };
        self.row(i, &path, value::CONTAINER, RELATIONSHIP_CONTAINS)?;
        c.algorithm = self.algorithm(&i.children, parent)?;
        c.time_point = self.time_point(&i.children, parent)?;
        let mut seen = HashSet::new();
        for (idx, ch) in i.children.iter().enumerate() {
            let cp = append_path(parent, idx as u32 + 1);
            let key = identifier_string(&cp);
            if self.algorithm_concept(ch, &key) > 0 || self.time_point_rank(ch, &key).is_some() {
                continue;
            }
            let single = |p: &Self, seen: &mut HashSet<String>, name: &str, vt: &str, rel: &str| {
                singleton(seen, name, &key)?;
                p.row(ch, &key, vt, rel)
            };
            if self.matches(ch, &key, &context::activity_session()) {
                single(
                    self,
                    &mut seen,
                    "session",
                    value::TEXT,
                    RELATIONSHIP_HAS_OBS_CONTEXT,
                )?;
                c.activity_session = ch.text.clone();
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::tracking_identifier()) {
                single(
                    self,
                    &mut seen,
                    "tracking-id",
                    value::TEXT,
                    RELATIONSHIP_HAS_OBS_CONTEXT,
                )?;
                g.tracking.identifier = ch.text.clone();
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::tracking_unique_identifier()) {
                single(
                    self,
                    &mut seen,
                    "tracking-uid",
                    value::UIDREF,
                    RELATIONSHIP_HAS_OBS_CONTEXT,
                )?;
                g.tracking.uid = ch.uid.clone();
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::finding()) {
                single(
                    self,
                    &mut seen,
                    "finding",
                    value::CODE,
                    RELATIONSHIP_CONTAINS,
                )?;
                g.finding = ch.code.clone();
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &context::finding_category()) {
                self.row(ch, &key, value::CODE, RELATIONSHIP_CONTAINS)?;
                self.unsupported_at(&key)?;
            } else if self.matches(ch, &key, &context::method()) {
                single(
                    self,
                    &mut seen,
                    "method",
                    value::CODE,
                    RELATIONSHIP_HAS_CONCEPT_MOD,
                )?;
                c.method = Some(self.code(ch, &key, true));
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::finding_site()) {
                let s = self.site(ch, &cp)?;
                g.finding_sites.push(s.site.code.clone());
                c.finding_sites.push(s);
            } else if self.matches(ch, &key, &context::rwvm()) {
                singleton(&mut seen, "rwvm", &key)?;
                c.real_world_value_map = Some(self.rwvm(ch, &key, RELATIONSHIP_CONTAINS)?);
            } else if self.matches(ch, &key, &concept::referenced_segment()) {
                if template != "1411" {
                    return Err(shape(&key));
                }
                single(
                    self,
                    &mut seen,
                    "segment",
                    value::IMAGE,
                    RELATIONSHIP_CONTAINS,
                )?;
                let detail = self.r.snapshot.reference_index().get(&key);
                let Some(detail) = detail.filter(|d| {
                    d.segments.len() == 1 && measurement_segment_sop_class(&ch.image.sop_class_uid)
                }) else {
                    return Err(shape(&key));
                };
                g.referenced_segment = SegmentReference {
                    study_instance_uid: ch.image.study_instance_uid.clone(),
                    series_instance_uid: ch.image.series_instance_uid.clone(),
                    sop_class_uid: ch.image.sop_class_uid.clone(),
                    sop_instance_uid: ch.image.sop_instance_uid.clone(),
                    segment_number: i64::from(detail.segments[0]),
                };
            } else if self.matches(ch, &key, &concept::source_series()) {
                if template != "1411" {
                    return Err(shape(&key));
                }
                single(
                    self,
                    &mut seen,
                    "source-series",
                    value::UIDREF,
                    RELATIONSHIP_CONTAINS,
                )?;
                g.source_series_uid = ch.uid.clone();
                self.scalar_children(ch, &cp)?;
            } else if ch.value_type == value::NUM && ch.relationship_type == RELATIONSHIP_CONTAINS {
                if self.known(ch, &key) {
                    return Err(shape(&key));
                }
                let (m, mc) = self.measurement(ch, &cp, template)?;
                g.measurements.push(m);
                c.measurements.push(mc);
            } else if (ch.value_type == value::CODE || ch.value_type == value::TEXT)
                && ch.relationship_type == RELATIONSHIP_CONTAINS
            {
                let q = self.qualitative(ch, &cp)?;
                c.qualitative.push(q);
            } else {
                if self.known(ch, &key) {
                    return Err(shape(&key));
                }
                self.unsupported_at(&key)?;
            }
        }
        Ok((g, c))
    }
    fn measurement(
        &mut self,
        i: &ContentItem,
        parent: &[u32],
        template: &str,
    ) -> Result<(ReportMeasurement, MeasurementNumericContext), SrError> {
        let path = identifier_string(parent);
        let mut m = ReportMeasurement {
            concept_name: i.concept_name.clone(),
            ..Default::default()
        };
        let meta = self.meta(&path).clone();
        let mut c = MeasurementNumericContext {
            path: path.clone(),
            concept_name: self.code(i, &path, false),
            ..Default::default()
        };
        let n = self.numeric(i, &path, RELATIONSHIP_CONTAINS)?;
        c.numeric = Some(n);
        let measured = i.measurement.clone().unwrap_or_default();
        m.value = measured.value;
        m.units = measured.units;
        c.units = MeasurementContextCode {
            code: m.units.clone(),
            context: meta.measurement_units.clone(),
            equivalent_codes: meta.measurement_units_equivalent_codes.clone(),
        };
        self.valid_code(&c.concept_name, &path)?;
        c.algorithm = self.algorithm(&i.children, parent)?;
        let mut seen = HashSet::new();
        for (idx, ch) in i.children.iter().enumerate() {
            let cp = append_path(parent, idx as u32 + 1);
            let key = identifier_string(&cp);
            if self.algorithm_concept(ch, &key) > 0 {
                continue;
            }
            let source = self.matches(ch, &key, &concept::source_of_measurement());
            if self.matches(ch, &key, &context::method()) {
                singleton(&mut seen, "method", &key)?;
                self.row(ch, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                c.method = Some(self.code(ch, &key, true));
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::derivation()) {
                singleton(&mut seen, "derivation", &key)?;
                self.row(ch, &key, value::CODE, RELATIONSHIP_HAS_CONCEPT_MOD)?;
                m.derivation = ch.code.clone();
                self.scalar_children(ch, &cp)?;
            } else if self.matches(ch, &key, &concept::finding_site()) {
                let s = self.site(ch, &cp)?;
                c.finding_sites.push(s);
            } else if self.matches(ch, &key, &context::rwvm()) {
                singleton(&mut seen, "rwvm", &key)?;
                c.real_world_value_map = Some(self.rwvm(ch, &key, RELATIONSHIP_INFERRED_FROM)?);
                if template == "1411" {
                    self.finding_at(MEASUREMENT_CONTEXT_RWVM_EXTENSION, &key)?;
                }
            } else if source && ch.value_type == value::IMAGE {
                singleton(&mut seen, "image", &key)?;
                self.row(ch, &key, value::IMAGE, RELATIONSHIP_INFERRED_FROM)?;
                m.image = ch.image.clone();
            } else if source && ch.value_type == value::SCOORD3D {
                singleton(&mut seen, "spatial", &key)?;
                self.row(ch, &key, value::SCOORD3D, RELATIONSHIP_INFERRED_FROM)?;
                m.spatial = ch.spatial3d.clone();
            } else {
                if self.known(ch, &key) {
                    return Err(shape(&key));
                }
                self.unsupported_at(&key)?;
            }
        }
        Ok((m, c))
    }
}
