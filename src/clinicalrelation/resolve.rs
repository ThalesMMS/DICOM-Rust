use super::{
    COMPOSITING_PLANAR_MPR_VPS_STORAGE, Cause, DEFORMABLE_SPATIAL_REGISTRATION_STORAGE, Diagnostic,
    DiagnosticCode, GRAYSCALE_PLANAR_MPR_VPS_STORAGE, Identity, LABEL_MAP_SEGMENTATION_STORAGE,
    MULTIPLE_VOLUME_RENDERING_VPS_STORAGE, ObjectKind, PARAMETRIC_MAP_STORAGE, RT_DOSE_STORAGE,
    ReadRequirement, Reference, ReferenceRole, Resolution, ResolutionError, SEGMENTATION_STORAGE,
    SEGMENTED_VOLUME_RENDERING_VPS_STORAGE, SPATIAL_REGISTRATION_STORAGE,
    VOLUME_RENDERING_VPS_STORAGE,
};
use crate::object::text::normalize_uid;
use crate::object::{Object, Tag, Vr, is_valid_uid};
use crate::{gsps, rtstruct, sr};
use std::collections::HashSet;
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
const TAG_REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
const TAG_REFERENCED_RT_PLAN_SEQUENCE: Tag = Tag::new(0x300C, 0x0002);
const TAG_REFERENCED_STRUCTURE_SET_SEQUENCE: Tag = Tag::new(0x300C, 0x0060);
pub fn resolve(
    dataset: Option<&Object>,
    identity: &Identity,
) -> Result<Resolution, ResolutionError> {
    let Some(dataset) = dataset else {
        return Err(ResolutionError {
            code: DiagnosticCode::NilDataset,
            kind: ObjectKind::Other,
            cause: Some(Cause::Message("dataset is nil".into())),
            resolution: Box::new(Resolution {
                kind: ObjectKind::Other,
                ..Default::default()
            }),
        });
    };
    let mut resolver = RelationResolver {
        resolution: Resolution {
            kind: ObjectKind::Image,
            sop_class_uid: first_non_empty(&[
                &identity.sop_class_uid,
                &dataset_uid(dataset, TAG_SOP_CLASS_UID),
            ]),
            sop_instance_uid: first_non_empty(&[
                &identity.sop_instance_uid,
                &dataset_uid(dataset, TAG_SOP_INSTANCE_UID),
            ]),
            study_instance_uid: first_non_empty(&[
                &identity.study_instance_uid,
                &dataset_uid(dataset, TAG_STUDY_INSTANCE_UID),
            ]),
            series_instance_uid: first_non_empty(&[
                &identity.series_instance_uid,
                &dataset_uid(dataset, TAG_SERIES_INSTANCE_UID),
            ]),
            frame_of_reference_uid: dataset_uid(dataset, TAG_FRAME_OF_REFERENCE_UID),
            ..Default::default()
        },
        reference_index: 0,
    };
    let (_, reader) = resolver_for_sop_class(&resolver.resolution.sop_class_uid);
    let outcome = match reader {
        RelationReader::Seg => resolver.resolve_seg(dataset),
        RelationReader::Gsps => resolver.resolve_gsps(dataset),
        RelationReader::Kos => resolver.resolve_kos(dataset),
        RelationReader::RtStruct => resolver.resolve_rtstruct(dataset),
        RelationReader::RtDose => resolver.resolve_rtdose(dataset),
        RelationReader::ParametricMap => resolver.resolve_parametric_map(dataset),
        RelationReader::SrMeasurement => resolver.resolve_sr_measurement(dataset),
        RelationReader::Vps => resolver.resolve_vps(dataset),
        RelationReader::AffineRegistration => resolver.resolve_affine_registration(dataset),
        RelationReader::DeformableRegistration => resolver.resolve_deformable_registration(dataset),
        RelationReader::Identity => {
            if resolver.resolution.sop_class_uid.is_empty() {
                resolver.resolution.kind = ObjectKind::Other;
            }
            Ok(())
        }
    };
    match outcome {
        Ok(()) => Ok(resolver.resolution),
        Err((kind, cause)) => Err(malformed(kind, cause, resolver.resolution)),
    }
}
pub fn resolve_dataset(
    dataset: &Object,
    identity: &Identity,
) -> Result<Resolution, ResolutionError> {
    resolve(Some(dataset), identity)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RelationReader {
    Identity,
    Seg,
    Gsps,
    Kos,
    RtStruct,
    RtDose,
    ParametricMap,
    SrMeasurement,
    Vps,
    AffineRegistration,
    DeformableRegistration,
}
pub(super) fn resolver_for_sop_class(sop_class_uid: &str) -> (ReadRequirement, RelationReader) {
    match sop_class_uid {
        SEGMENTATION_STORAGE | LABEL_MAP_SEGMENTATION_STORAGE => {
            (ReadRequirement::PixelData, RelationReader::Seg)
        }
        gsps::GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE => {
            (ReadRequirement::CompleteModules, RelationReader::Gsps)
        }
        sr::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE => {
            (ReadRequirement::CompleteModules, RelationReader::Kos)
        }
        rtstruct::RT_STRUCTURE_SET_STORAGE => {
            (ReadRequirement::CompleteModules, RelationReader::RtStruct)
        }
        RT_DOSE_STORAGE => (ReadRequirement::PixelData, RelationReader::RtDose),
        PARAMETRIC_MAP_STORAGE => (ReadRequirement::PixelData, RelationReader::ParametricMap),
        sr::measurement_report::ENHANCED_SR_STORAGE
        | sr::measurement_report::COMPREHENSIVE_SR_STORAGE
        | sr::measurement_report::COMPREHENSIVE_3D_SR_STORAGE => (
            ReadRequirement::CompleteModules,
            RelationReader::SrMeasurement,
        ),
        GRAYSCALE_PLANAR_MPR_VPS_STORAGE
        | COMPOSITING_PLANAR_MPR_VPS_STORAGE
        | VOLUME_RENDERING_VPS_STORAGE
        | SEGMENTED_VOLUME_RENDERING_VPS_STORAGE
        | MULTIPLE_VOLUME_RENDERING_VPS_STORAGE => {
            (ReadRequirement::CompleteModules, RelationReader::Vps)
        }
        SPATIAL_REGISTRATION_STORAGE => (
            ReadRequirement::CompleteModules,
            RelationReader::AffineRegistration,
        ),
        DEFORMABLE_SPATIAL_REGISTRATION_STORAGE => (
            ReadRequirement::CompleteModules,
            RelationReader::DeformableRegistration,
        ),
        _ => (ReadRequirement::Identity, RelationReader::Identity),
    }
}
fn more_general_registration_type(current: &str, candidate: &str) -> String {
    fn rank(value: &str) -> i32 {
        match value {
            "RIGID" => 1,
            "RIGID_SCALE" => 2,
            "AFFINE" => 3,
            _ => 0,
        }
    }
    if rank(candidate) > rank(current) {
        candidate.to_string()
    } else {
        current.to_string()
    }
}
struct RelationResolver {
    resolution: Resolution,
    reference_index: i64,
}
#[derive(Clone, Copy, Default)]
struct ReferenceRequirements {
    study: bool,
    series: bool,
}
type BranchError = (ObjectKind, Cause);
impl RelationResolver {
    fn resolve_seg(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::Seg;
        self.resolution.editable = true;
        let document = match read_seg(dataset) {
            Ok(document) => document,
            Err(cause) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::SourceImage);
                return Err((ObjectKind::Seg, cause));
            }
        };
        self.prefer_study(&document.study_instance_uid);
        self.prefer_frame(&document.frame_of_reference_uid);
        for reference in document.references {
            self.add_reference(
                Reference {
                    role: ReferenceRole::SourceImage,
                    study_instance_uid: String::new(),
                    series_instance_uid: reference.series_instance_uid,
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    frames: reference.frames,
                },
                ReferenceRequirements {
                    study: false,
                    series: true,
                },
            );
        }
        Ok(())
    }
    fn resolve_gsps(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::Gsps;
        self.resolution.editable = true;
        let state = match gsps::read(Some(dataset)) {
            Ok(state) => state,
            Err(err) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::PresentationImage);
                return Err((ObjectKind::Gsps, Cause::Gsps(err)));
            }
        };
        self.prefer_study(&state.study_instance_uid);
        for reference in state.referenced_images {
            self.add_reference(
                Reference {
                    role: ReferenceRole::PresentationImage,
                    study_instance_uid: String::new(),
                    series_instance_uid: reference.series_instance_uid,
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    frames: reference.frames,
                },
                ReferenceRequirements {
                    study: false,
                    series: true,
                },
            );
        }
        Ok(())
    }
    fn resolve_kos(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::Kos;
        self.resolution.editable = true;
        let document = match sr::read_document(Some(dataset)) {
            Ok(document) => document,
            Err(err) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::KeyObject);
                return Err((ObjectKind::Kos, Cause::Sr(err.error)));
            }
        };
        self.prefer_study(&document.study_instance_uid);
        for reference in sr::kos::key_object_selection_images(Some(&document)) {
            self.add_reference(
                Reference {
                    role: ReferenceRole::KeyObject,
                    study_instance_uid: reference.study_instance_uid,
                    series_instance_uid: reference.series_instance_uid,
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    frames: reference.frames,
                },
                ReferenceRequirements {
                    study: true,
                    series: true,
                },
            );
        }
        Ok(())
    }
    fn resolve_rtstruct(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::RtStruct;
        self.resolution.editable = true;
        let structure_set = match rtstruct::read(dataset) {
            Ok(structure_set) => structure_set,
            Err(err) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::ContourImage);
                return Err((ObjectKind::RtStruct, Cause::RtStruct(err)));
            }
        };
        self.prefer_study(&structure_set.study_instance_uid);
        self.prefer_frame(&structure_set.frame_of_reference_uid);
        for roi in structure_set.rois {
            for contour in roi.contours {
                for reference in contour.referenced_images {
                    self.add_reference(
                        Reference {
                            role: ReferenceRole::ContourImage,
                            study_instance_uid: reference.study_instance_uid,
                            series_instance_uid: reference.series_instance_uid,
                            sop_class_uid: reference.sop_class_uid,
                            sop_instance_uid: reference.sop_instance_uid,
                            frames: Vec::new(),
                        },
                        ReferenceRequirements::default(),
                    );
                }
            }
        }
        Ok(())
    }
    fn resolve_rtdose(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::RtDose;
        let dose = match read_rtdose(dataset) {
            Ok(dose) => dose,
            Err(cause) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::RtObject);
                return Err((ObjectKind::RtDose, cause));
            }
        };
        self.prefer_study(&dose.study_instance_uid);
        self.prefer_frame(&dose.frame_of_reference_uid);
        for reference in dose.references {
            self.add_reference(
                Reference {
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    ..Reference::new(ReferenceRole::RtObject)
                },
                ReferenceRequirements::default(),
            );
        }
        self.add_missing_direct_references(
            dataset,
            ReferenceRole::RtObject,
            &[
                TAG_REFERENCED_RT_PLAN_SEQUENCE,
                TAG_REFERENCED_STRUCTURE_SET_SEQUENCE,
            ],
        );
        Ok(())
    }
    fn resolve_parametric_map(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::ParametricMap;
        let parametric_map = match read_parametric_map(dataset) {
            Ok(parametric_map) => parametric_map,
            Err(cause) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::SourceImage);
                return Err((ObjectKind::ParametricMap, cause));
            }
        };
        self.prefer_study(&parametric_map.study_instance_uid);
        self.prefer_frame(&parametric_map.frame_of_reference_uid);
        for reference in parametric_map.references {
            self.add_reference(
                Reference {
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    ..Reference::new(ReferenceRole::SourceImage)
                },
                ReferenceRequirements::default(),
            );
        }
        self.add_missing_nested_references(dataset, ReferenceRole::SourceImage);
        Ok(())
    }
    fn resolve_sr_measurement(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::SrMeasurement;
        self.resolution.editable = true;
        let report = match sr::measurement_report::read_measurement_report(Some(dataset)) {
            Ok(report) => report,
            Err(err) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::MeasurementImage);
                return Err((ObjectKind::SrMeasurement, Cause::Sr(err)));
            }
        };
        for group in report.groups {
            self.add_reference(
                Reference {
                    sop_class_uid: group.referenced_segment.sop_class_uid,
                    sop_instance_uid: group.referenced_segment.sop_instance_uid,
                    ..Reference::new(ReferenceRole::Segment)
                },
                ReferenceRequirements::default(),
            );
            for measurement in group.measurements {
                self.add_reference(
                    Reference {
                        role: ReferenceRole::MeasurementImage,
                        study_instance_uid: measurement.image.study_instance_uid,
                        series_instance_uid: measurement.image.series_instance_uid,
                        sop_class_uid: measurement.image.sop_class_uid,
                        sop_instance_uid: measurement.image.sop_instance_uid,
                        frames: measurement.image.frames,
                    },
                    ReferenceRequirements::default(),
                );
            }
        }
        let mut seen: HashSet<String> = self
            .resolution
            .references
            .iter()
            .map(|r| library_key(&r.sop_class_uid, &r.sop_instance_uid, &r.frames))
            .collect();
        for image in report.image_library.into_iter().flatten() {
            let key = library_key(&image.sop_class_uid, &image.sop_instance_uid, &image.frames);
            if !seen.contains(&key) {
                self.add_reference(
                    Reference {
                        role: ReferenceRole::MeasurementImage,
                        study_instance_uid: image.study_instance_uid,
                        series_instance_uid: image.series_instance_uid,
                        sop_class_uid: image.sop_class_uid,
                        sop_instance_uid: image.sop_instance_uid,
                        frames: image.frames,
                    },
                    ReferenceRequirements::default(),
                );
                seen.insert(key);
            }
        }
        Ok(())
    }
    fn resolve_vps(&mut self, dataset: &Object) -> Result<(), BranchError> {
        self.resolution.kind = ObjectKind::Vps;
        self.resolution.editable = true;
        let state = match read_vps(dataset) {
            Ok(state) => state,
            Err(cause) => {
                self.add_malformed_nested_references(dataset, ReferenceRole::PresentationInput);
                return Err((ObjectKind::Vps, cause));
            }
        };
        self.prefer_study(&state.study_instance_uid);
        for reference in state.references {
            self.add_reference(
                Reference {
                    sop_class_uid: reference.sop_class_uid,
                    sop_instance_uid: reference.sop_instance_uid,
                    ..Reference::new(ReferenceRole::PresentationInput)
                },
                ReferenceRequirements::default(),
            );
        }
        Ok(())
    }
    fn resolve_affine_registration(&mut self, dataset: &Object) -> Result<(), BranchError> {
        let registration =
            read_affine_registration(dataset).map_err(|cause| (ObjectKind::Registration, cause))?;
        self.resolution.kind = ObjectKind::Registration;
        self.prefer_frame(&registration.registered_frame_of_reference_uid);
        for item in registration.registrations {
            append_unique(
                &mut self.resolution.source_frame_of_reference_uids,
                &item.source_frame_of_reference_uid,
            );
            self.resolution.registration_type = more_general_registration_type(
                &self.resolution.registration_type,
                &item.registration_type,
            );
            for sop_instance_uid in item.referenced_sop_instance_uids {
                self.add_reference(
                    Reference {
                        sop_instance_uid,
                        ..Reference::new(ReferenceRole::RegistrationSource)
                    },
                    ReferenceRequirements::default(),
                );
            }
        }
        Ok(())
    }
    fn resolve_deformable_registration(&mut self, dataset: &Object) -> Result<(), BranchError> {
        let registration = read_deformable_registration(dataset)
            .map_err(|cause| (ObjectKind::Registration, cause))?;
        self.resolution.kind = ObjectKind::Registration;
        self.resolution.registration_type = "DEFORMABLE".into();
        self.prefer_frame(&registration.registered_frame_of_reference_uid);
        for item in registration.registrations {
            append_unique(
                &mut self.resolution.source_frame_of_reference_uids,
                &item.source_frame_of_reference_uid,
            );
            for sop_instance_uid in item.referenced_sop_instance_uids {
                self.add_reference(
                    Reference {
                        sop_instance_uid,
                        ..Reference::new(ReferenceRole::RegistrationSource)
                    },
                    ReferenceRequirements::default(),
                );
            }
        }
        Ok(())
    }
    fn add_reference(&mut self, mut reference: Reference, requirements: ReferenceRequirements) {
        reference.study_instance_uid = clean_uid(&reference.study_instance_uid);
        reference.series_instance_uid = clean_uid(&reference.series_instance_uid);
        reference.sop_class_uid = clean_uid(&reference.sop_class_uid);
        reference.sop_instance_uid = clean_uid(&reference.sop_instance_uid);
        let index = self.reference_index;
        self.reference_index += 1;
        let role = reference.role;
        let mut valid = true;
        valid = self.validate_uid(
            role,
            index,
            &reference.sop_class_uid,
            true,
            DiagnosticCode::MissingSopClassUid,
            DiagnosticCode::InvalidSopClassUid,
        ) && valid;
        valid = self.validate_uid(
            role,
            index,
            &reference.sop_instance_uid,
            true,
            DiagnosticCode::MissingSopInstanceUid,
            DiagnosticCode::InvalidSopInstanceUid,
        ) && valid;
        valid = self.validate_uid(
            role,
            index,
            &reference.study_instance_uid,
            requirements.study,
            DiagnosticCode::MissingStudyInstanceUid,
            DiagnosticCode::InvalidStudyInstanceUid,
        ) && valid;
        valid = self.validate_uid(
            role,
            index,
            &reference.series_instance_uid,
            requirements.series,
            DiagnosticCode::MissingSeriesInstanceUid,
            DiagnosticCode::InvalidSeriesInstanceUid,
        ) && valid;
        if valid {
            self.resolution.references.push(reference);
        }
    }
    fn validate_uid(
        &mut self,
        role: ReferenceRole,
        index: i64,
        value: &str,
        required: bool,
        missing_code: DiagnosticCode,
        invalid_code: DiagnosticCode,
    ) -> bool {
        if value.is_empty() {
            if required {
                self.resolution.diagnostics.push(Diagnostic {
                    code: missing_code,
                    role,
                    reference_index: index,
                });
                return false;
            }
            return true;
        }
        if !is_valid_uid(value.as_bytes()) {
            self.resolution.diagnostics.push(Diagnostic {
                code: invalid_code,
                role,
                reference_index: index,
            });
            return false;
        }
        true
    }
    fn add_missing_direct_references(
        &mut self,
        dataset: &Object,
        role: ReferenceRole,
        sequence_tags: &[Tag],
    ) {
        for &sequence_tag in sequence_tags {
            for item in dataset.get_sequence(sequence_tag).unwrap_or_default() {
                if missing_referenced_sop_instance_uid(&item) {
                    self.add_reference(
                        reference_from_object(&item, role),
                        ReferenceRequirements::default(),
                    );
                }
            }
        }
    }
    fn add_missing_nested_references(&mut self, dataset: &Object, role: ReferenceRole) {
        visit_nested_references(dataset, &mut |item| {
            if missing_referenced_sop_instance_uid(item) {
                self.add_reference(
                    reference_from_object(item, role),
                    ReferenceRequirements::default(),
                );
            }
        });
    }
    fn add_malformed_nested_references(&mut self, dataset: &Object, role: ReferenceRole) {
        visit_nested_references(dataset, &mut |item| {
            if !item.has(TAG_REFERENCED_SOP_CLASS_UID) && !item.has(TAG_REFERENCED_SOP_INSTANCE_UID)
            {
                return;
            }
            let class_uid = object_uid(item, TAG_REFERENCED_SOP_CLASS_UID);
            let instance_uid = object_uid(item, TAG_REFERENCED_SOP_INSTANCE_UID);
            if is_valid_uid(class_uid.as_bytes()) && is_valid_uid(instance_uid.as_bytes()) {
                return;
            }
            self.add_reference(
                Reference {
                    sop_class_uid: class_uid,
                    sop_instance_uid: instance_uid,
                    ..Reference::new(role)
                },
                ReferenceRequirements::default(),
            );
        });
    }
    fn prefer_study(&mut self, value: &str) {
        self.resolution.study_instance_uid =
            first_non_empty(&[value, &self.resolution.study_instance_uid]);
    }
    fn prefer_frame(&mut self, value: &str) {
        self.resolution.frame_of_reference_uid =
            first_non_empty(&[value, &self.resolution.frame_of_reference_uid]);
    }
}
fn library_key(sop_class_uid: &str, sop_instance_uid: &str, frames: &[i64]) -> String {
    let frames: Vec<String> = frames.iter().map(i64::to_string).collect();
    format!("{sop_class_uid}/{sop_instance_uid}/[{}]", frames.join(" "))
}
fn visit_nested_references(dataset: &Object, visit_reference: &mut dyn FnMut(&Object)) {
    visit_reference(dataset);
    for element in dataset.elements() {
        if element.vr() != Vr::SQ {
            continue;
        }
        for item in dataset.get_sequence(element.tag()).unwrap_or_default() {
            visit_nested_references(&item, visit_reference);
        }
    }
}
fn missing_referenced_sop_instance_uid(item: &Object) -> bool {
    if !item.has(TAG_REFERENCED_SOP_CLASS_UID) {
        return false;
    }
    object_uid(item, TAG_REFERENCED_SOP_INSTANCE_UID).is_empty()
}
fn reference_from_object(item: &Object, role: ReferenceRole) -> Reference {
    Reference {
        sop_class_uid: object_uid(item, TAG_REFERENCED_SOP_CLASS_UID),
        sop_instance_uid: object_uid(item, TAG_REFERENCED_SOP_INSTANCE_UID),
        ..Reference::new(role)
    }
}
fn malformed(kind: ObjectKind, cause: Cause, resolution: Resolution) -> ResolutionError {
    let code = match cause {
        Cause::NotPorted(_) => DiagnosticCode::ReaderNotPorted,
        _ => DiagnosticCode::MalformedObject,
    };
    ResolutionError {
        code,
        kind,
        cause: Some(cause),
        resolution: Box::new(resolution),
    }
}
fn dataset_uid(dataset: &Object, tag: Tag) -> String {
    match dataset.get_uid(tag) {
        Some(value) => clean_uid(&value),
        None => String::new(),
    }
}
fn object_uid(dataset: &Object, tag: Tag) -> String {
    match dataset.get_string(tag) {
        Some(value) => clean_uid(&value),
        None => String::new(),
    }
}
pub(super) fn clean_uid(value: &str) -> String {
    normalize_uid(value).trim().to_string()
}
fn first_non_empty(values: &[&str]) -> String {
    for value in values {
        let cleaned = clean_uid(value);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    String::new()
}
pub(super) fn append_unique(values: &mut Vec<String>, value: &str) {
    let value = clean_uid(value);
    if value.is_empty() || values.contains(&value) {
        return;
    }
    values.push(value);
}
#[derive(Clone, Debug, Default)]
struct ReaderReference {
    series_instance_uid: String,
    sop_class_uid: String,
    sop_instance_uid: String,
    frames: Vec<i64>,
}
#[derive(Clone, Debug, Default)]
struct ReaderObject {
    study_instance_uid: String,
    frame_of_reference_uid: String,
    references: Vec<ReaderReference>,
}
#[derive(Clone, Debug, Default)]
struct ReaderRegistrationItem {
    source_frame_of_reference_uid: String,
    registration_type: String,
    referenced_sop_instance_uids: Vec<String>,
}
#[derive(Clone, Debug, Default)]
struct ReaderRegistration {
    registered_frame_of_reference_uid: String,
    registrations: Vec<ReaderRegistrationItem>,
}
fn read_seg(dataset: &Object) -> Result<ReaderObject, Cause> {
    let doc = crate::seg::read(dataset).map_err(Cause::Seg)?;
    Ok(ReaderObject {
        study_instance_uid: doc.study_instance_uid,
        frame_of_reference_uid: doc.frame_of_reference_uid,
        references: doc
            .referenced_images
            .into_iter()
            .map(|r| ReaderReference {
                series_instance_uid: r.series_instance_uid,
                sop_class_uid: r.sop_class_uid,
                sop_instance_uid: r.sop_instance_uid,
                frames: r.frames,
            })
            .collect(),
    })
}
fn read_rtdose(dataset: &Object) -> Result<ReaderObject, Cause> {
    let dose = crate::rtdose::read_dataset(Some(dataset)).map_err(Cause::RtDose)?;
    Ok(ReaderObject {
        study_instance_uid: dose.study_instance_uid,
        frame_of_reference_uid: dose.frame_of_reference_uid,
        references: dose
            .references
            .into_iter()
            .map(|reference| ReaderReference {
                sop_class_uid: reference.sop_class_uid,
                sop_instance_uid: reference.sop_instance_uid,
                ..ReaderReference::default()
            })
            .collect(),
    })
}
fn read_parametric_map(dataset: &Object) -> Result<ReaderObject, Cause> {
    let map = crate::parametricmap::read_dataset(dataset).map_err(Cause::ParametricMap)?;
    Ok(ReaderObject {
        study_instance_uid: map.study_instance_uid,
        frame_of_reference_uid: map.frame_of_reference_uid,
        references: map
            .references
            .into_iter()
            .map(|r| ReaderReference {
                sop_class_uid: r.sop_class_uid,
                sop_instance_uid: r.sop_instance_uid,
                ..ReaderReference::default()
            })
            .collect(),
    })
}
fn read_vps(dataset: &Object) -> Result<ReaderObject, Cause> {
    let state = crate::vps::read(Some(dataset)).map_err(Cause::Vps)?;
    Ok(ReaderObject {
        study_instance_uid: state.study_instance_uid,
        frame_of_reference_uid: String::new(),
        references: state
            .inputs
            .into_iter()
            .flat_map(|input| input.referenced_instances)
            .map(|r| ReaderReference {
                sop_class_uid: r.sop_class_uid,
                sop_instance_uid: r.sop_instance_uid,
                ..ReaderReference::default()
            })
            .collect(),
    })
}
fn read_affine_registration(dataset: &Object) -> Result<ReaderRegistration, Cause> {
    let r = crate::spatialreg::read_affine_dataset(Some(dataset))
        .map_err(|e| Cause::SpatialReg(super::SpatialRegError(e)))?;
    Ok(ReaderRegistration {
        registered_frame_of_reference_uid: r.registered_frame_of_reference_uid,
        registrations: r
            .registrations
            .into_iter()
            .map(|item| ReaderRegistrationItem {
                source_frame_of_reference_uid: item.source_frame_of_reference_uid,
                registration_type: item.r#type,
                referenced_sop_instance_uids: item.referenced_sop_instance_uids,
            })
            .collect(),
    })
}
fn read_deformable_registration(dataset: &Object) -> Result<ReaderRegistration, Cause> {
    let r = crate::spatialreg::read_dataset(Some(dataset))
        .map_err(|e| Cause::SpatialReg(super::SpatialRegError(e)))?;
    Ok(ReaderRegistration {
        registered_frame_of_reference_uid: r.registered_frame_of_reference_uid,
        registrations: r
            .registrations
            .into_iter()
            .map(|item| ReaderRegistrationItem {
                source_frame_of_reference_uid: item.source_frame_of_reference_uid,
                registration_type: String::new(),
                referenced_sop_instance_uids: item.referenced_sop_instance_uids,
            })
            .collect(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::clinicalrelation::VOLUME_RENDERING_VPS_STORAGE as VPS;
    #[test]
    fn vps_reader_is_ported() {
        let dataset = Object::new();
        let identity = Identity {
            sop_class_uid: VPS.into(),
            ..Default::default()
        };
        let err = resolve(Some(&dataset), &identity).unwrap_err();
        assert!(!err.is_not_ported());
        assert_eq!(err.kind, ObjectKind::Vps);
        assert!(matches!(err.cause, Some(Cause::Vps(_))));
        assert_eq!(err.resolution.kind, ObjectKind::Vps);
    }
}
