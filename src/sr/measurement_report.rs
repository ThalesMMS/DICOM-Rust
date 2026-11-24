use super::image_library::{
    ImageLibraryGroup, default_image_library_options, image_library_error,
    structured_image_library_needed,
};
use super::image_library_structured::{
    ImageLibraryMetadataBinding, bind_image_library_metadata_with_order, image_library_data_sets,
    read_grouped_image_library, structured_library_container,
};
use super::lifecycle::{DocumentReference, VerifyingObserver, lifecycle_elements, read_lifecycle};
use super::read::{
    Decoder, EvidenceHierarchy, decode_code, read_current_requested_procedure_evidence,
    read_document, read_image_reference, read_measurement,
};
use super::reference::has_image_reference;
use super::relationship::{default_relationship_options, validate_relationships};
use super::tags as tg;
use super::{
    CodedEntry, ContentItem, DataSet, DerivedCommon, ImageReference, Kind, Measurement,
    RELATIONSHIP_CONTAINS, RELATIONSHIP_HAS_CONCEPT_MOD, RELATIONSHIP_HAS_OBS_CONTEXT,
    RELATIONSHIP_INFERRED_FROM, SrError, clean, code_sequence, default_string, derivedio,
    document_series_uid, evidence_references, from_code, hierarchical_reference_sequence,
    measured_value_sequence, referenced_sop_sequence, spatial_coordinate3d_elements,
    validate_document_state, value,
};
use crate::codes::sct;
use crate::object::inherit::inherit_modules;
use crate::object::{File, Object, Vr};
use std::collections::HashMap;
pub const ENHANCED_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.22";
pub const COMPREHENSIVE_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.33";
pub const COMPREHENSIVE_3D_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.34";
pub const GRAPHIC_TYPE_POINT_3D: &str = "POINT";
pub const GRAPHIC_TYPE_MULTIPOINT_3D: &str = "MULTIPOINT";
pub const GRAPHIC_TYPE_POLYLINE_3D: &str = "POLYLINE";
pub const GRAPHIC_TYPE_POLYGON_3D: &str = "POLYGON";
pub const DEFAULT_OBSERVER_DEVICE_UID: &str = "2.25.5904973083307394922725280422832941272";
pub const TEMPLATE_MEASUREMENT_REPORT: &str = "1500";
pub const TEMPLATE_MEASUREMENT_GROUP: &str = "1501";
pub const TEMPLATE_VOLUMETRIC_MEASUREMENTS: &str = "1411";
pub(crate) mod concept {
    use super::super::{CodedEntry, from_code};
    use crate::codes::{dcm, sct, umls};
    macro_rules! concepts {
        ($($name:ident = $code:expr;)*) => {
            $(pub(crate) fn $name() -> CodedEntry { from_code($code) })*
        };
    }
    concepts! {
        imaging_measurement_report = dcm::ImagingMeasurementReport;
        language = dcm::LanguageOfContentItemAndDescendants;
        observer_type = dcm::ObserverType;
        person_observer_type = dcm::Person;
        device_observer_type = dcm::Device;
        person_observer_name = dcm::PersonObserverName;
        person_observer_organization = dcm::PersonObserversOrganizationName;
        device_observer_uid = dcm::DeviceObserverUID;
        device_observer_name = dcm::DeviceObserverName;
        device_observer_manufacturer = dcm::DeviceObserverManufacturer;
        device_observer_model = dcm::DeviceObserverModelName;
        device_observer_serial = dcm::DeviceObserverSerialNumber;
        procedure_reported = dcm::ProcedureReported;
        image_library = dcm::ImageLibrary;
        image_library_group = dcm::ImageLibraryGroup;
        imaging_measurements = dcm::ImagingMeasurements;
        derived_imaging_measurements = dcm::DerivedImagingMeasurements;
        qualitative_evaluations = umls::QualitativeEvaluations;
        measurement_group = dcm::MeasurementGroup;
        tracking_identifier = dcm::TrackingIdentifier;
        tracking_unique_identifier = dcm::TrackingUniqueIdentifier;
        finding = dcm::Finding;
        finding_site = sct::FindingSite;
        referenced_segment = dcm::ReferencedSegment;
        source_series = dcm::SourceSeriesForSegmentation;
        source_of_measurement = dcm::SourceOfMeasurement;
        derivation = dcm::Derivation;
    }
}
pub fn default_language() -> CodedEntry {
    CodedEntry::new("en-US", "RFC5646", "English (US)")
}
pub fn default_procedure_reported() -> CodedEntry {
    from_code(sct::ImagingProcedure)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackingIdentifier {
    pub uid: String,
    pub identifier: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentReference {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub segment_number: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpatialReference {
    pub frame_of_reference_uid: String,
    pub graphic_type: String,
    pub coordinates: Vec<Point3D>,
    pub fiducial_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportMeasurement {
    pub concept_name: CodedEntry,
    pub value: f64,
    pub units: CodedEntry,
    pub derivation: CodedEntry,
    pub image: ImageReference,
    pub spatial: SpatialReference,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementGroup {
    pub template: String,
    pub tracking: TrackingIdentifier,
    pub finding: CodedEntry,
    pub finding_sites: Vec<CodedEntry>,
    pub referenced_segment: SegmentReference,
    pub source_series_uid: String,
    pub measurements: Vec<ReportMeasurement>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observer {
    pub person_name: String,
    pub organization_name: String,
    pub device_uid: String,
    pub device_name: String,
    pub device_manufacturer: String,
    pub device_model_name: String,
    pub device_serial_number: String,
}
pub fn default_observer() -> Observer {
    Observer {
        device_uid: DEFAULT_OBSERVER_DEVICE_UID.into(),
        device_name: "dcm-rust".into(),
        ..Default::default()
    }
}
#[derive(Clone, Debug, Default)]
pub struct MeasurementReport {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub content_date: String,
    pub content_time: String,
    pub series_number: String,
    pub instance_number: String,
    pub completion_flag: String,
    pub verification_flag: String,
    pub completion_flag_description: String,
    pub verifying_observers: Vec<VerifyingObserver>,
    pub predecessor_documents: Vec<DocumentReference>,
    pub continuity_of_content: String,
    pub title: CodedEntry,
    pub language: CodedEntry,
    pub observers: Vec<Observer>,
    pub procedures_reported: Vec<CodedEntry>,
    pub image_library: Option<Vec<ImageReference>>,
    pub image_library_groups: Option<Vec<ImageLibraryGroup>>,
    pub image_library_order: Vec<i64>,
    pub image_library_descriptors: Option<Vec<ContentItem>>,
    pub image_library_metadata: Option<ImageLibraryMetadataBinding>,
    pub image_library_read_projection: bool,
    pub groups: Vec<MeasurementGroup>,
    pub legacy: bool,
    pub common: DerivedCommon,
}
fn ds(elements: Vec<crate::object::Element>) -> DataSet {
    derivedio::data_set(elements)
}
fn invalid_report(suffix: &str) -> SrError {
    SrError::new(Kind::InvalidMeasurementReport, suffix)
}
pub fn write_measurement_report(report: Option<&MeasurementReport>) -> Result<File, SrError> {
    let Some(report) = report else {
        return Err(invalid_report(": report is nil"));
    };
    let sop_class_uid = if report.sop_class_uid.is_empty() {
        COMPREHENSIVE_3D_SR_STORAGE
    } else {
        report.sop_class_uid.as_str()
    };
    if !measurement_sop_class(sop_class_uid) {
        return Err(SrError::new(
            Kind::UnsupportedSrStorage,
            &format!(": {sop_class_uid}"),
        ));
    }
    let (completion, verification) = validate_document_state(
        &report.completion_flag,
        &report.verification_flag,
        report.verifying_observers.len(),
    )
    .map_err(|e| e.wrapped_v(Kind::InvalidMeasurementReport))?;
    let title = if report.title.empty() {
        concept::imaging_measurement_report()
    } else {
        report.title.clone()
    };
    let content = measurement_report_content(report)?;
    let evidence =
        evidence_references(&measurement_evidence(report), report.common.source.as_ref())
            .map_err(|e| e.wrapped(Kind::InvalidMeasurementReport))?;
    let mut elements = vec![
        derivedio::ui(tg::SOP_CLASS_UID, sop_class_uid),
        derivedio::ui(tg::SOP_INSTANCE_UID, &report.sop_instance_uid),
        derivedio::cs(tg::MODALITY, "SR"),
        derivedio::str_elem(
            tg::SERIES_NUMBER,
            Vr::IS,
            &default_string(&report.series_number, "1"),
        ),
        derivedio::str_elem(
            tg::INSTANCE_NUMBER,
            Vr::IS,
            &default_string(&report.instance_number, "1"),
        ),
        derivedio::cs(tg::VALUE_TYPE, value::CONTAINER),
        derivedio::cs(
            tg::CONTINUITY_OF_CONTENT,
            &default_string(&report.continuity_of_content, "SEPARATE"),
        ),
        derivedio::cs(tg::COMPLETION_FLAG, &completion),
        derivedio::cs(tg::VERIFICATION_FLAG, &verification),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &title),
        template_sequence(TEMPLATE_MEASUREMENT_REPORT),
        derivedio::seq(tg::REFERENCED_PERFORMED_PROCEDURE_STEP, Vec::new()),
        derivedio::seq(tg::PERFORMED_PROCEDURE_CODE_SEQUENCE, Vec::new()),
    ];
    if !report.study_instance_uid.is_empty() {
        elements.push(derivedio::ui(
            derivedio::TAG_STUDY_INSTANCE_UID,
            &report.study_instance_uid,
        ));
    }
    elements.push(derivedio::ui(
        derivedio::TAG_SERIES_INSTANCE_UID,
        &document_series_uid(
            sop_class_uid,
            &report.sop_instance_uid,
            &report.series_instance_uid,
        ),
    ));
    if !report.frame_of_reference_uid.is_empty() {
        elements.push(derivedio::ui(
            derivedio::TAG_FRAME_OF_REFERENCE_UID,
            &report.frame_of_reference_uid,
        ));
    }
    let (mut content_date, mut content_time) =
        (report.content_date.clone(), report.content_time.clone());
    if content_date.is_empty() || content_time.is_empty() {
        let now = chrono::Local::now();
        if content_date.is_empty() {
            content_date = now.format("%Y%m%d").to_string();
        }
        if content_time.is_empty() {
            content_time = now.format("%H%M%S%.6f").to_string();
        }
    }
    elements.push(derivedio::str_elem(tg::CONTENT_DATE, Vr::DA, &content_date));
    elements.push(derivedio::str_elem(tg::CONTENT_TIME, Vr::TM, &content_time));
    elements.push(derivedio::seq(tg::CONTENT_SEQUENCE, content));
    elements.extend(
        lifecycle_elements(
            &report.completion_flag_description,
            &report.verifying_observers,
            &report.predecessor_documents,
        )
        .map_err(|e| e.wrapped(Kind::InvalidMeasurementReport))?,
    );
    if !evidence.is_empty() {
        elements.push(hierarchical_reference_sequence(
            tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE,
            &evidence,
        ));
    }
    let mut dataset = derivedio::object(elements);
    validate_encoded_relationships(&dataset)
        .map_err(|e| e.wrapped(Kind::InvalidMeasurementReport))?;
    report
        .common
        .apply(&mut dataset, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)
        .map_err(|e| SrError::msg(e.to_string()))?;
    derivedio::file(sop_class_uid, &report.sop_instance_uid, Some(dataset)).map_err(|e| {
        let mut err = SrError::msg(e.to_string());
        if e.is(crate::object::Kind::MissingSopClassUid) {
            err.kinds.push(Kind::MissingSopClassUid);
        }
        if e.is(crate::object::Kind::MissingSopInstanceUid) {
            err.kinds.push(Kind::MissingSopInstanceUid);
        }
        err
    })
}
fn measurement_report_content(report: &MeasurementReport) -> Result<Vec<DataSet>, SrError> {
    let language = if report.language.empty() {
        default_language()
    } else {
        report.language.clone()
    };
    let mut content = vec![code_item(
        RELATIONSHIP_HAS_CONCEPT_MOD,
        &concept::language(),
        &language,
    )];
    let defaults = [default_observer()];
    let observers: &[Observer] = if report.observers.is_empty() {
        &defaults
    } else {
        &report.observers
    };
    for (i, o) in observers.iter().enumerate() {
        let items = observer_items(o).map_err(|e| {
            e.prefixed(&format!("observer {i}: "))
                .wrapped(Kind::InvalidMeasurementReport)
        })?;
        content.extend(items);
    }
    let default_procedures = [default_procedure_reported()];
    let procedures: &[CodedEntry] = if report.procedures_reported.is_empty() {
        &default_procedures
    } else {
        &report.procedures_reported
    };
    for p in procedures {
        content.push(code_item(
            RELATIONSHIP_HAS_CONCEPT_MOD,
            &concept::procedure_reported(),
            p,
        ));
    }
    let structured = image_library_data_sets(report)?;
    if let Some(children) = structured {
        content.push(structured_library_container(report, children)?);
    }
    let structured =
        report.image_library_groups.is_some() || report.image_library_descriptors.is_some();
    let library = match &report.image_library {
        _ if structured => Vec::new(),
        Some(l) => l.clone(),
        None => measured_images(&report.groups),
    };
    if !library.is_empty() {
        let entries = library
            .iter()
            .map(|image| {
                ds(vec![
                    derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_CONTAINS),
                    derivedio::cs(tg::VALUE_TYPE, value::IMAGE),
                    referenced_sop_sequence(image),
                ])
            })
            .collect();
        content.push(container_item(
            RELATIONSHIP_CONTAINS,
            &concept::image_library(),
            "",
            vec![container_item(
                RELATIONSHIP_CONTAINS,
                &concept::image_library_group(),
                "",
                entries,
            )],
        ));
    }
    let mut groups = Vec::with_capacity(report.groups.len());
    for (i, g) in report.groups.iter().enumerate() {
        let r = &g.referenced_segment;
        if has_segment_reference(r) && !(1..=65535).contains(&r.segment_number) {
            return Err(invalid_report(&format!(
                ": group {i}: referenced segment number {} must be in 1..65535",
                r.segment_number
            )));
        }
        groups.push(measurement_group_data_set(g));
    }
    content.push(container_item(
        RELATIONSHIP_CONTAINS,
        &concept::imaging_measurements(),
        "",
        groups,
    ));
    Ok(content)
}
fn observer_items(o: &Observer) -> Result<Vec<DataSet>, SrError> {
    if !o.person_name.is_empty() {
        let mut items = vec![
            code_item(
                RELATIONSHIP_HAS_OBS_CONTEXT,
                &concept::observer_type(),
                &concept::person_observer_type(),
            ),
            ds(vec![
                derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_HAS_OBS_CONTEXT),
                derivedio::cs(tg::VALUE_TYPE, value::PNAME),
                code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &concept::person_observer_name()),
                derivedio::str_elem(tg::PERSON_NAME_VALUE, Vr::PN, &o.person_name),
            ]),
        ];
        if !o.organization_name.is_empty() {
            items.push(text_item(
                RELATIONSHIP_HAS_OBS_CONTEXT,
                &concept::person_observer_organization(),
                &o.organization_name,
            ));
        }
        return Ok(items);
    }
    if o.device_uid.is_empty() {
        return Err(SrError::msg(
            "an observer needs a person name or a device UID",
        ));
    }
    let mut items = vec![
        code_item(
            RELATIONSHIP_HAS_OBS_CONTEXT,
            &concept::observer_type(),
            &concept::device_observer_type(),
        ),
        uid_item(
            RELATIONSHIP_HAS_OBS_CONTEXT,
            &concept::device_observer_uid(),
            &o.device_uid,
        ),
    ];
    for (c, v) in [
        (concept::device_observer_name(), &o.device_name),
        (
            concept::device_observer_manufacturer(),
            &o.device_manufacturer,
        ),
        (concept::device_observer_model(), &o.device_model_name),
        (concept::device_observer_serial(), &o.device_serial_number),
    ] {
        if !v.is_empty() {
            items.push(text_item(RELATIONSHIP_HAS_OBS_CONTEXT, &c, v));
        }
    }
    Ok(items)
}
fn measurement_evidence(report: &MeasurementReport) -> Vec<ImageReference> {
    let mut refs: Vec<ImageReference> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut add = |r: &ImageReference| {
        let r = ImageReference {
            study_instance_uid: r.study_instance_uid.clone(),
            series_instance_uid: r.series_instance_uid.clone(),
            sop_class_uid: r.sop_class_uid.clone(),
            sop_instance_uid: r.sop_instance_uid.clone(),
            frames: Vec::new(),
        };
        let key = clean(&r.sop_instance_uid).to_string();
        match index.get(&key) {
            None => {
                index.insert(key, refs.len());
                refs.push(r);
            }
            Some(&i) => {
                if clean(&refs[i].study_instance_uid).is_empty() {
                    refs[i].study_instance_uid = r.study_instance_uid;
                }
                if clean(&refs[i].series_instance_uid).is_empty() {
                    refs[i].series_instance_uid = r.series_instance_uid;
                }
            }
        }
    };
    if let Some(library) = &report.image_library {
        for r in library {
            add(r);
        }
    }
    for g in report.image_library_groups.iter().flatten() {
        for e in &g.entries {
            add(&e.image);
        }
    }
    for g in &report.groups {
        let s = &g.referenced_segment;
        if has_segment_reference(s) {
            add(&ImageReference {
                study_instance_uid: s.study_instance_uid.clone(),
                series_instance_uid: s.series_instance_uid.clone(),
                sop_class_uid: s.sop_class_uid.clone(),
                sop_instance_uid: s.sop_instance_uid.clone(),
                frames: Vec::new(),
            });
        }
        for m in &g.measurements {
            if has_image_reference(&m.image) {
                add(&m.image);
            }
        }
    }
    refs
}
fn measured_images(groups: &[MeasurementGroup]) -> Vec<ImageReference> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for g in groups {
        for m in &g.measurements {
            let r = &m.image;
            if r.sop_instance_uid.is_empty() || !seen.insert(r.sop_instance_uid.clone()) {
                continue;
            }
            out.push(ImageReference {
                study_instance_uid: r.study_instance_uid.clone(),
                series_instance_uid: r.series_instance_uid.clone(),
                sop_class_uid: r.sop_class_uid.clone(),
                sop_instance_uid: r.sop_instance_uid.clone(),
                frames: Vec::new(),
            });
        }
    }
    out
}
fn validate_encoded_relationships(dataset: &Object) -> Result<(), SrError> {
    let document = read_document(Some(dataset)).map_err(|e| e.error)?;
    if let (_, Some(e)) = validate_relationships(Some(&document), default_relationship_options())
        && !e.is(Kind::UnconstrainedSopClass)
    {
        return Err(e);
    }
    Ok(())
}
pub fn read_measurement_report(obj: Option<&Object>) -> Result<MeasurementReport, SrError> {
    let Some(obj) = obj else {
        return Err(SrError::new(Kind::NoDataset, ""));
    };
    let o = Some(obj);
    let mut dec = Decoder::default();
    let sop_class_uid = dec.string(o, tg::SOP_CLASS_UID);
    if let Some(e) = dec.err.take() {
        return Err(e);
    }
    if !measurement_sop_class(&sop_class_uid) {
        return Err(SrError::new(
            Kind::UnsupportedSrStorage,
            &format!(": {sop_class_uid}"),
        ));
    }
    let mut report = MeasurementReport {
        sop_class_uid,
        sop_instance_uid: dec.string(o, tg::SOP_INSTANCE_UID),
        study_instance_uid: dec.string(o, derivedio::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: dec.string(o, derivedio::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: dec.string(o, derivedio::TAG_FRAME_OF_REFERENCE_UID),
        content_date: dec.string(o, tg::CONTENT_DATE),
        content_time: dec.string(o, tg::CONTENT_TIME),
        series_number: dec.string(o, tg::SERIES_NUMBER),
        instance_number: dec.string(o, tg::INSTANCE_NUMBER),
        completion_flag: dec.string(o, tg::COMPLETION_FLAG),
        verification_flag: dec.string(o, tg::VERIFICATION_FLAG),
        continuity_of_content: dec.string(o, tg::CONTINUITY_OF_CONTENT),
        ..Default::default()
    };
    report.title = decode_code(&mut dec, o, tg::CONCEPT_NAME_CODE_SEQ).0;
    (
        report.completion_flag_description,
        report.verifying_observers,
        report.predecessor_documents,
    ) = read_lifecycle(&mut dec, obj);
    let (children, _) = dec.sequence(o, tg::CONTENT_SEQUENCE);
    let mut observer: Option<Observer> = None;
    for (index, child) in children.iter().enumerate() {
        let c = Some(child);
        let concept = decode_code(&mut dec, c, tg::CONCEPT_NAME_CODE_SEQ).0;
        let value_type = dec.string(c, tg::VALUE_TYPE);
        if observer.is_some() && !is_observer_attribute(&concept) {
            report.observers.push(observer.take().unwrap_or_default());
        }
        if matches_concept(&concept, &concept::language()) {
            report.language = decode_code(&mut dec, c, tg::CONCEPT_CODE_SEQ).0;
        } else if matches_concept(&concept, &concept::observer_type()) {
            observer = Some(Observer::default());
        } else if is_observer_attribute(&concept) {
            let o = observer.get_or_insert_with(Observer::default);
            read_observer_attribute(&mut dec, child, &concept, o);
        } else if matches_concept(&concept, &concept::procedure_reported()) {
            let (code, ok) = decode_code(&mut dec, c, tg::CONCEPT_CODE_SEQ);
            if ok {
                report.procedures_reported.push(code);
            }
        } else if value_type != value::CONTAINER {
        } else if matches_concept(&concept, &concept::image_library()) {
            if structured_image_library_needed(child)? {
                if report.image_library_groups.is_some() || report.image_library.is_some() {
                    return Err(image_library_error("duplicate-library", ""));
                }
                let (descriptors, groups, binding) =
                    read_grouped_image_library(child, default_image_library_options())?;
                report.image_library_descriptors = Some(descriptors);
                report.image_library_groups = Some(groups);
                report.image_library_order = binding.order.clone();
                report.image_library_metadata = Some(binding);
                let library = read_image_library(&mut dec, child);
                report.image_library = if library.is_empty() {
                    None
                } else {
                    Some(library)
                };
                report.image_library_read_projection = true;
                continue;
            }
            if report.image_library_groups.is_some() {
                return Err(image_library_error("duplicate-library", ""));
            }
            let library = read_image_library(&mut dec, child);
            if !library.is_empty() || report.image_library.is_some() {
                report
                    .image_library
                    .get_or_insert_with(Vec::new)
                    .extend(library);
            }
        } else if matches_concept(&concept, &concept::imaging_measurements()) {
            let (groups, _) = dec.sequence(c, tg::CONTENT_SEQUENCE);
            for (gi, g) in groups.iter().enumerate() {
                if dec.string(Some(g), tg::VALUE_TYPE) != value::CONTAINER {
                    continue;
                }
                let group = read_measurement_group(&mut dec, g)
                    .map_err(|e| e.prefixed(&format!("measurement group {gi}: ")))?;
                report.groups.push(group);
            }
        } else if matches_concept(&concept, &concept::derived_imaging_measurements())
            || matches_concept(&concept, &concept::qualitative_evaluations())
        {
        } else {
            let group = read_measurement_group(&mut dec, child)
                .map_err(|e| e.prefixed(&format!("measurement group {index}: ")))?;
            report.groups.push(group);
            report.legacy = true;
        }
    }
    if let Some(o) = observer {
        report.observers.push(o);
    }
    let evidence = read_current_requested_procedure_evidence(&mut dec, obj);
    apply_measurement_evidence(&mut report, &evidence);
    if let Some(b) = &report.image_library_metadata {
        let rebound = bind_image_library_metadata_with_order(
            report
                .image_library_descriptors
                .as_deref()
                .unwrap_or_default(),
            report.image_library_groups.as_deref().unwrap_or_default(),
            &report.image_library_order,
            &b.metadata(),
            &b.numeric_metadata(),
            b.options,
        )?;
        report.image_library_metadata = Some(rebound);
    }
    if let Some(e) = dec.err.take() {
        return Err(e);
    }
    Ok(report)
}
fn apply_measurement_evidence(
    report: &mut MeasurementReport,
    evidence: &HashMap<String, EvidenceHierarchy>,
) {
    let image = |r: &mut ImageReference| {
        if let Some(h) = evidence.get(clean(&r.sop_instance_uid)) {
            r.study_instance_uid = h.study_uid.clone();
            r.series_instance_uid = h.series_uid.clone();
        }
    };
    if let Some(library) = &mut report.image_library {
        library.iter_mut().for_each(image);
    }
    for g in report.image_library_groups.iter_mut().flatten() {
        for e in &mut g.entries {
            image(&mut e.image);
        }
    }
    for g in &mut report.groups {
        if let Some(h) = evidence.get(clean(&g.referenced_segment.sop_instance_uid)) {
            g.referenced_segment.study_instance_uid = h.study_uid.clone();
            g.referenced_segment.series_instance_uid = h.series_uid.clone();
        }
        for m in &mut g.measurements {
            image(&mut m.image);
        }
    }
}
fn observer_attributes() -> [CodedEntry; 7] {
    [
        concept::person_observer_name(),
        concept::person_observer_organization(),
        concept::device_observer_uid(),
        concept::device_observer_name(),
        concept::device_observer_manufacturer(),
        concept::device_observer_model(),
        concept::device_observer_serial(),
    ]
}
fn is_observer_attribute(c: &CodedEntry) -> bool {
    observer_attributes().iter().any(|a| matches_concept(c, a))
}
fn read_observer_attribute(dec: &mut Decoder, item: &Object, c: &CodedEntry, o: &mut Observer) {
    let i = Some(item);
    if matches_concept(c, &concept::person_observer_name()) {
        o.person_name = dec.string(i, tg::PERSON_NAME_VALUE);
    } else if matches_concept(c, &concept::person_observer_organization()) {
        o.organization_name = dec.string(i, tg::TEXT_VALUE);
    } else if matches_concept(c, &concept::device_observer_uid()) {
        o.device_uid = dec.string(i, tg::UID);
    } else if matches_concept(c, &concept::device_observer_name()) {
        o.device_name = dec.string(i, tg::TEXT_VALUE);
    } else if matches_concept(c, &concept::device_observer_manufacturer()) {
        o.device_manufacturer = dec.string(i, tg::TEXT_VALUE);
    } else if matches_concept(c, &concept::device_observer_model()) {
        o.device_model_name = dec.string(i, tg::TEXT_VALUE);
    } else if matches_concept(c, &concept::device_observer_serial()) {
        o.device_serial_number = dec.string(i, tg::TEXT_VALUE);
    }
}
fn read_image_library(dec: &mut Decoder, container: &Object) -> Vec<ImageReference> {
    let mut out = Vec::new();
    let (children, _) = dec.sequence(Some(container), tg::CONTENT_SEQUENCE);
    for child in &children {
        match dec.string(Some(child), tg::VALUE_TYPE).as_str() {
            value::IMAGE => out.push(read_image_reference(dec, child)),
            value::CONTAINER => out.extend(read_image_library(dec, child)),
            _ => {}
        }
    }
    out
}
pub(crate) fn template_sequence(identifier: &str) -> crate::object::Element {
    derivedio::seq(
        tg::CONTENT_TEMPLATE_SEQUENCE,
        vec![ds(vec![
            derivedio::cs(tg::MAPPING_RESOURCE, "DCMR"),
            derivedio::cs(tg::TEMPLATE_IDENTIFIER, identifier),
        ])],
    )
}
pub(crate) fn container_item(
    relationship: &str,
    concept: &CodedEntry,
    template: &str,
    children: Vec<DataSet>,
) -> DataSet {
    let mut elements = vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, relationship),
        derivedio::cs(tg::VALUE_TYPE, value::CONTAINER),
        derivedio::cs(tg::CONTINUITY_OF_CONTENT, "SEPARATE"),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, concept),
    ];
    if !template.is_empty() {
        elements.push(template_sequence(template));
    }
    if !children.is_empty() {
        elements.push(derivedio::seq(tg::CONTENT_SEQUENCE, children));
    }
    ds(elements)
}
pub(crate) fn code_item(relationship: &str, concept: &CodedEntry, v: &CodedEntry) -> DataSet {
    ds(vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, relationship),
        derivedio::cs(tg::VALUE_TYPE, value::CODE),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, concept),
        code_sequence(tg::CONCEPT_CODE_SEQ, v),
    ])
}
pub(crate) fn text_item(relationship: &str, concept: &CodedEntry, v: &str) -> DataSet {
    ds(vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, relationship),
        derivedio::cs(tg::VALUE_TYPE, value::TEXT),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, concept),
        derivedio::str_elem(tg::TEXT_VALUE, Vr::UT, v),
    ])
}
pub(crate) fn uid_item(relationship: &str, concept: &CodedEntry, v: &str) -> DataSet {
    ds(vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, relationship),
        derivedio::cs(tg::VALUE_TYPE, value::UIDREF),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, concept),
        derivedio::ui(tg::UID, v),
    ])
}
fn measurement_group_data_set(g: &MeasurementGroup) -> DataSet {
    let mut children = Vec::new();
    if !g.tracking.identifier.is_empty() {
        children.push(text_item(
            RELATIONSHIP_HAS_OBS_CONTEXT,
            &concept::tracking_identifier(),
            &g.tracking.identifier,
        ));
    }
    if !g.tracking.uid.is_empty() {
        children.push(uid_item(
            RELATIONSHIP_HAS_OBS_CONTEXT,
            &concept::tracking_unique_identifier(),
            &g.tracking.uid,
        ));
    }
    if !g.finding.empty() {
        children.push(code_item(
            RELATIONSHIP_CONTAINS,
            &concept::finding(),
            &g.finding,
        ));
    }
    if has_segment_reference(&g.referenced_segment) {
        children.push(segment_item(&g.referenced_segment));
    }
    if !g.source_series_uid.is_empty() {
        children.push(uid_item(
            RELATIONSHIP_CONTAINS,
            &concept::source_series(),
            &g.source_series_uid,
        ));
    }
    for site in &g.finding_sites {
        children.push(code_item(
            RELATIONSHIP_HAS_CONCEPT_MOD,
            &concept::finding_site(),
            site,
        ));
    }
    for m in &g.measurements {
        children.push(measurement_item(m));
    }
    let template = if !g.template.is_empty() {
        g.template.as_str()
    } else if has_segment_reference(&g.referenced_segment) {
        TEMPLATE_VOLUMETRIC_MEASUREMENTS
    } else {
        TEMPLATE_MEASUREMENT_GROUP
    };
    container_item(
        RELATIONSHIP_CONTAINS,
        &concept::measurement_group(),
        template,
        children,
    )
}
pub(crate) fn has_segment_reference(r: &SegmentReference) -> bool {
    !r.sop_class_uid.is_empty() || !r.sop_instance_uid.is_empty() || r.segment_number != 0
}
fn segment_item(r: &SegmentReference) -> DataSet {
    ds(vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_CONTAINS),
        derivedio::cs(tg::VALUE_TYPE, value::IMAGE),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &concept::referenced_segment()),
        derivedio::seq(
            tg::REF_SOP_SEQUENCE,
            vec![ds(vec![
                derivedio::ui(tg::REF_SOP_CLASS_UID, &r.sop_class_uid),
                derivedio::ui(tg::REF_SOP_INSTANCE_UID, &r.sop_instance_uid),
                derivedio::us(tg::REFERENCED_SEGMENT_NUMBER, &[r.segment_number as u16]),
            ])],
        ),
    ])
}
fn measurement_item(m: &ReportMeasurement) -> DataSet {
    let mut children = Vec::new();
    if !m.derivation.empty() {
        children.push(code_item(
            RELATIONSHIP_HAS_CONCEPT_MOD,
            &concept::derivation(),
            &m.derivation,
        ));
    }
    if has_image_reference(&m.image) {
        children.push(image_item(&m.image));
    }
    if has_spatial_reference(&m.spatial) {
        children.push(spatial_item(&m.spatial));
    }
    let mut elements = vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_CONTAINS),
        derivedio::cs(tg::VALUE_TYPE, value::NUM),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &m.concept_name),
        measured_value_sequence(&Measurement {
            value: m.value,
            units: m.units.clone(),
        }),
    ];
    if !children.is_empty() {
        elements.push(derivedio::seq(tg::CONTENT_SEQUENCE, children));
    }
    ds(elements)
}
pub(crate) fn has_spatial_reference(r: &SpatialReference) -> bool {
    !r.frame_of_reference_uid.is_empty() || !r.graphic_type.is_empty() || !r.coordinates.is_empty()
}
fn image_item(r: &ImageReference) -> DataSet {
    ds(vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_INFERRED_FROM),
        derivedio::cs(tg::VALUE_TYPE, value::IMAGE),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &concept::source_of_measurement()),
        referenced_sop_sequence(r),
    ])
}
fn spatial_item(r: &SpatialReference) -> DataSet {
    let mut elements = vec![
        derivedio::cs(tg::RELATIONSHIP_TYPE, RELATIONSHIP_INFERRED_FROM),
        derivedio::cs(tg::VALUE_TYPE, "SCOORD3D"),
        code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &concept::source_of_measurement()),
    ];
    elements.extend(spatial_coordinate3d_elements(r));
    ds(elements)
}
fn read_measurement_group(dec: &mut Decoder, obj: &Object) -> Result<MeasurementGroup, SrError> {
    let mut group = MeasurementGroup::default();
    let (templates, ok) = dec.sequence(Some(obj), tg::CONTENT_TEMPLATE_SEQUENCE);
    if ok && let Some(t) = templates.first() {
        group.template = dec.string(Some(t), tg::TEMPLATE_IDENTIFIER);
    }
    let (children, _) = dec.sequence(Some(obj), tg::CONTENT_SEQUENCE);
    for (index, child) in children.iter().enumerate() {
        let c = Some(child);
        let concept = decode_code(dec, c, tg::CONCEPT_NAME_CODE_SEQ).0;
        if matches_concept(&concept, &concept::tracking_identifier()) {
            group.tracking.identifier = dec.string(c, tg::TEXT_VALUE);
        } else if matches_concept(&concept, &concept::tracking_unique_identifier()) {
            group.tracking.uid = dec.string(c, tg::UID);
        } else if matches_concept(&concept, &concept::finding()) {
            group.finding = decode_code(dec, c, tg::CONCEPT_CODE_SEQ).0;
        } else if matches_concept(&concept, &concept::finding_site()) {
            let (site, ok) = decode_code(dec, c, tg::CONCEPT_CODE_SEQ);
            if ok {
                group.finding_sites.push(site);
            }
        } else if matches_concept(&concept, &concept::referenced_segment()) {
            group.referenced_segment = read_segment_reference(dec, child);
        } else if matches_concept(&concept, &concept::source_series()) {
            group.source_series_uid = dec.string(c, tg::UID);
        } else if dec.string(c, tg::VALUE_TYPE) == value::NUM {
            let m = read_report_measurement(dec, child)
                .map_err(|e| e.prefixed(&format!("measurement item {index}: ")))?;
            group.measurements.push(m);
        }
    }
    Ok(group)
}
pub(crate) fn matches_concept(got: &CodedEntry, want: &CodedEntry) -> bool {
    if got.coding_scheme_designator == want.coding_scheme_designator {
        return got.code_value == want.code_value;
    }
    got.code_meaning == want.code_meaning
}
fn read_report_measurement(dec: &mut Decoder, obj: &Object) -> Result<ReportMeasurement, SrError> {
    let mut rm = ReportMeasurement {
        concept_name: decode_code(dec, Some(obj), tg::CONCEPT_NAME_CODE_SEQ).0,
        ..Default::default()
    };
    if let Some((m, _, _)) = read_measurement(dec, obj) {
        rm.value = m.value;
        rm.units = m.units;
    }
    let (children, _) = dec.sequence(Some(obj), tg::CONTENT_SEQUENCE);
    for (index, child) in children.iter().enumerate() {
        match dec.string(Some(child), tg::VALUE_TYPE).as_str() {
            value::CODE => {
                let concept = decode_code(dec, Some(child), tg::CONCEPT_NAME_CODE_SEQ).0;
                if matches_concept(&concept, &concept::derivation()) {
                    rm.derivation = decode_code(dec, Some(child), tg::CONCEPT_CODE_SEQ).0;
                }
            }
            value::IMAGE => rm.image = read_image_reference(dec, child),
            value::SCOORD3D => {
                rm.spatial = read_spatial_reference(dec, child)
                    .map_err(|e| e.prefixed(&format!("child content item {index}: ")))?;
            }
            _ => {}
        }
    }
    Ok(rm)
}
fn read_segment_reference(dec: &mut Decoder, obj: &Object) -> SegmentReference {
    let (items, _) = dec.sequence(Some(obj), tg::REF_SOP_SEQUENCE);
    let Some(item) = items.first() else {
        return SegmentReference::default();
    };
    let numbers = dec.ints(Some(item), tg::REFERENCED_SEGMENT_NUMBER);
    SegmentReference {
        segment_number: numbers.first().copied().unwrap_or(0),
        sop_class_uid: dec.string(Some(item), tg::REF_SOP_CLASS_UID),
        sop_instance_uid: dec.string(Some(item), tg::REF_SOP_INSTANCE_UID),
        ..Default::default()
    }
}
pub(crate) fn read_spatial_reference(
    dec: &mut Decoder,
    obj: &Object,
) -> Result<SpatialReference, SrError> {
    let o = Some(obj);
    let mut r = SpatialReference {
        frame_of_reference_uid: dec.string(o, tg::REFERENCED_FRAME_OF_REF_UID),
        graphic_type: dec.string(o, tg::GRAPHIC_TYPE),
        fiducial_uid: dec.string(o, tg::FIDUCIAL_UID),
        coordinates: Vec::new(),
    };
    let values = dec.floats(o, tg::GRAPHIC_DATA);
    if !values.len().is_multiple_of(3) {
        return Err(SrError::new(
            Kind::InvalidGraphicData,
            &format!(
                ": SCOORD3D has {} values, want complete x/y/z triples",
                values.len()
            ),
        ));
    }
    for p in values.chunks(3) {
        r.coordinates.push(Point3D {
            x: p[0],
            y: p[1],
            z: p[2],
        });
    }
    Ok(r)
}
pub(crate) fn measurement_sop_class(uid: &str) -> bool {
    matches!(
        uid,
        ENHANCED_SR_STORAGE | COMPREHENSIVE_SR_STORAGE | COMPREHENSIVE_3D_SR_STORAGE
    )
}
