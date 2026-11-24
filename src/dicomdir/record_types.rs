#![forbid(unsafe_code)]
use crate::dictionary::uid::{self, Type};
use std::borrow::Cow;
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RecordType(pub Cow<'static, str>);
impl RecordType {
    pub const PATIENT: RecordType = RecordType(Cow::Borrowed("PATIENT"));
    pub const STUDY: RecordType = RecordType(Cow::Borrowed("STUDY"));
    pub const SERIES: RecordType = RecordType(Cow::Borrowed("SERIES"));
    pub const IMAGE: RecordType = RecordType(Cow::Borrowed("IMAGE"));
    pub const RT_DOSE: RecordType = RecordType(Cow::Borrowed("RT DOSE"));
    pub const RT_STRUCTURE_SET: RecordType = RecordType(Cow::Borrowed("RT STRUCTURE SET"));
    pub const RT_PLAN: RecordType = RecordType(Cow::Borrowed("RT PLAN"));
    pub const RT_TREATMENT_RECORD: RecordType = RecordType(Cow::Borrowed("RT TREAT RECORD"));
    pub const PRESENTATION: RecordType = RecordType(Cow::Borrowed("PRESENTATION"));
    pub const WAVEFORM: RecordType = RecordType(Cow::Borrowed("WAVEFORM"));
    pub const SR_DOCUMENT: RecordType = RecordType(Cow::Borrowed("SR DOCUMENT"));
    pub const KEY_OBJECT_DOCUMENT: RecordType = RecordType(Cow::Borrowed("KEY OBJECT DOC"));
    pub const SPECTROSCOPY: RecordType = RecordType(Cow::Borrowed("SPECTROSCOPY"));
    pub const RAW_DATA: RecordType = RecordType(Cow::Borrowed("RAW DATA"));
    pub const REGISTRATION: RecordType = RecordType(Cow::Borrowed("REGISTRATION"));
    pub const FIDUCIAL: RecordType = RecordType(Cow::Borrowed("FIDUCIAL"));
    pub const ENCAPSULATED_DOCUMENT: RecordType = RecordType(Cow::Borrowed("ENCAP DOC"));
    pub const VALUE_MAP: RecordType = RecordType(Cow::Borrowed("VALUE MAP"));
    pub const STEREOMETRIC: RecordType = RecordType(Cow::Borrowed("STEREOMETRIC"));
    pub const SURFACE: RecordType = RecordType(Cow::Borrowed("SURFACE"));
    pub const SURFACE_SCAN: RecordType = RecordType(Cow::Borrowed("SURFACE SCAN"));
    pub const TRACT: RecordType = RecordType(Cow::Borrowed("TRACT"));
    pub const ANNOTATION: RecordType = RecordType(Cow::Borrowed("ANNOTATION"));
    pub const WAVEFORM_PRESENTATION: RecordType = RecordType(Cow::Borrowed("WF PRESENTATION"));
    pub const HANGING_PROTOCOL: RecordType = RecordType(Cow::Borrowed("HANGING PROTOCOL"));
    pub const PALETTE: RecordType = RecordType(Cow::Borrowed("PALETTE"));
    pub const IMPLANT: RecordType = RecordType(Cow::Borrowed("IMPLANT"));
    pub const IMPLANT_ASSEMBLY: RecordType = RecordType(Cow::Borrowed("IMPLANT ASSY"));
    pub const IMPLANT_GROUP: RecordType = RecordType(Cow::Borrowed("IMPLANT GROUP"));
    pub const PLAN: RecordType = RecordType(Cow::Borrowed("PLAN"));
    pub const MEASUREMENT: RecordType = RecordType(Cow::Borrowed("MEASUREMENT"));
    pub const ASSESSMENT: RecordType = RecordType(Cow::Borrowed("ASSESSMENT"));
    pub const RADIOTHERAPY: RecordType = RecordType(Cow::Borrowed("RADIOTHERAPY"));
    pub const INVENTORY: RecordType = RecordType(Cow::Borrowed("INVENTORY"));
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for RecordType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<&str> for RecordType {
    fn from(s: &str) -> RecordType {
        RecordType(Cow::Owned(s.to_string()))
    }
}
impl From<String> for RecordType {
    fn from(s: String) -> RecordType {
        RecordType(Cow::Owned(s))
    }
}
impl PartialEq<str> for RecordType {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}
pub(crate) fn record_type_for_sop_class(sop_class_uid: &str) -> Option<RecordType> {
    let entry = uid::by_uid(sop_class_uid)?;
    if entry.typ != Type::SOP_CLASS {
        return None;
    }
    let t = match entry.keyword {
        "HangingProtocolStorage" => RecordType::HANGING_PROTOCOL,
        "ColorPaletteStorage" => RecordType::PALETTE,
        "GenericImplantTemplateStorage" => RecordType::IMPLANT,
        "ImplantAssemblyTemplateStorage" => RecordType::IMPLANT_ASSEMBLY,
        "ImplantTemplateGroupStorage" => RecordType::IMPLANT_GROUP,
        "InventoryStorage" => RecordType::INVENTORY,
        "ContentAssessmentResultsStorage" => RecordType::ASSESSMENT,
        "RTBeamsDeliveryInstructionStorage"
        | "RTBrachyApplicationSetupDeliveryInstructionStorage" => RecordType::PLAN,
        "LensometryMeasurementsStorage"
        | "AutorefractionMeasurementsStorage"
        | "KeratometryMeasurementsStorage"
        | "SubjectiveRefractionMeasurementsStorage"
        | "VisualAcuityMeasurementsStorage"
        | "OphthalmicAxialMeasurementsStorage"
        | "IntraocularLensCalculationsStorage"
        | "OphthalmicVisualFieldStaticPerimetryMeasurementsStorage" => RecordType::MEASUREMENT,
        "RTPhysicianIntentStorage"
        | "RTSegmentAnnotationStorage"
        | "RTRadiationSetStorage"
        | "CArmPhotonElectronRadiationStorage"
        | "TomotherapeuticRadiationStorage"
        | "RoboticArmRadiationStorage"
        | "RTRadiationRecordSetStorage"
        | "RTRadiationSalvageRecordStorage"
        | "TomotherapeuticRadiationRecordStorage"
        | "CArmPhotonElectronRadiationRecordStorage"
        | "RoboticRadiationRecordStorage"
        | "RTRadiationSetDeliveryInstructionStorage"
        | "RTTreatmentPreparationStorage"
        | "RTPatientPositionAcquisitionInstructionStorage" => RecordType::RADIOTHERAPY,
        "HardcopyGrayscaleImageStorage"
        | "HardcopyColorImageStorage"
        | "ComputedRadiographyImageStorage"
        | "DigitalXRayImageStorageForPresentation"
        | "DigitalXRayImageStorageForProcessing"
        | "DigitalMammographyXRayImageStorageForPresentation"
        | "DigitalMammographyXRayImageStorageForProcessing"
        | "DigitalIntraOralXRayImageStorageForPresentation"
        | "DigitalIntraOralXRayImageStorageForProcessing"
        | "XRayAngiographicImageStorage"
        | "EnhancedXAImageStorage"
        | "XRayRadiofluoroscopicImageStorage"
        | "EnhancedXRFImageStorage"
        | "XRayAngiographicBiPlaneImageStorage"
        | "PositronEmissionTomographyImageStorage"
        | "LegacyConvertedEnhancedPETImageStorage"
        | "XRay3DAngiographicImageStorage"
        | "XRay3DCraniofacialImageStorage"
        | "BreastTomosynthesisImageStorage"
        | "BreastProjectionXRayImageStorageForPresentation"
        | "BreastProjectionXRayImageStorageForProcessing"
        | "EnhancedPETImageStorage"
        | "IntravascularOpticalCoherenceTomographyImageStorageForPresentation"
        | "IntravascularOpticalCoherenceTomographyImageStorageForProcessing"
        | "CTImageStorage"
        | "EnhancedCTImageStorage"
        | "LegacyConvertedEnhancedCTImageStorage"
        | "NuclearMedicineImageStorage"
        | "UltrasoundMultiFrameImageStorageRetired"
        | "UltrasoundMultiFrameImageStorage"
        | "MRImageStorage"
        | "EnhancedMRImageStorage"
        | "EnhancedMRColorImageStorage"
        | "LegacyConvertedEnhancedMRImageStorage"
        | "RTImageStorage"
        | "EnhancedRTImageStorage"
        | "EnhancedContinuousRTImageStorage"
        | "NuclearMedicineImageStorageRetired"
        | "DICOSCTImageStorage"
        | "DICOSDigitalXRayImageStorageForPresentation"
        | "DICOSDigitalXRayImageStorageForProcessing"
        | "UltrasoundImageStorageRetired"
        | "UltrasoundImageStorage"
        | "PhotoacousticImageStorage"
        | "EddyCurrentImageStorage"
        | "EddyCurrentMultiFrameImageStorage"
        | "ThermographyImageStorage"
        | "ThermographyMultiFrameImageStorage"
        | "SecondaryCaptureImageStorage"
        | "MultiFrameSingleBitSecondaryCaptureImageStorage"
        | "MultiFrameGrayscaleByteSecondaryCaptureImageStorage"
        | "MultiFrameGrayscaleWordSecondaryCaptureImageStorage"
        | "MultiFrameTrueColorSecondaryCaptureImageStorage"
        | "VLImageStorageTrial"
        | "VLEndoscopicImageStorage"
        | "VideoEndoscopicImageStorage"
        | "VLMicroscopicImageStorage"
        | "VideoMicroscopicImageStorage"
        | "VLSlideCoordinatesMicroscopicImageStorage"
        | "VLPhotographicImageStorage"
        | "VideoPhotographicImageStorage"
        | "OphthalmicPhotography8BitImageStorage"
        | "OphthalmicPhotography16BitImageStorage"
        | "OphthalmicTomographyImageStorage"
        | "WideFieldOphthalmicPhotographyStereographicProjectionImageStorage"
        | "WideFieldOphthalmicPhotography3DCoordinatesImageStorage"
        | "OphthalmicOpticalCoherenceTomographyEnFaceImageStorage"
        | "OphthalmicOpticalCoherenceTomographyBscanVolumeAnalysisStorage"
        | "VLWholeSlideMicroscopyImageStorage"
        | "DermoscopicPhotographyImageStorage"
        | "ConfocalMicroscopyImageStorage"
        | "ConfocalMicroscopyTiledPyramidalImageStorage"
        | "VLMultiFrameImageStorageTrial"
        | "SegmentationStorage"
        | "LabelMapSegmentationStorage"
        | "HeightMapSegmentationStorage"
        | "ParametricMapStorage"
        | "EnhancedUSVolumeStorage"
        | "OphthalmicThicknessMapStorage"
        | "CornealTopographyMapStorage" => RecordType::IMAGE,
        "RTDoseStorage" => RecordType::RT_DOSE,
        "RTStructureSetStorage" => RecordType::RT_STRUCTURE_SET,
        "RTPlanStorage" | "RTIonPlanStorage" => RecordType::RT_PLAN,
        "RTBeamsTreatmentRecordStorage"
        | "RTBrachyTreatmentRecordStorage"
        | "RTTreatmentSummaryRecordStorage" => RecordType::RT_TREATMENT_RECORD,
        "GrayscaleSoftcopyPresentationStateStorage"
        | "ColorSoftcopyPresentationStateStorage"
        | "PseudoColorSoftcopyPresentationStateStorage"
        | "BlendingSoftcopyPresentationStateStorage"
        | "XAXRFGrayscaleSoftcopyPresentationStateStorage"
        | "GrayscalePlanarMPRVolumetricPresentationStateStorage"
        | "CompositingPlanarMPRVolumetricPresentationStateStorage"
        | "AdvancedBlendingPresentationStateStorage"
        | "VolumeRenderingVolumetricPresentationStateStorage"
        | "SegmentedVolumeRenderingVolumetricPresentationStateStorage"
        | "MultipleVolumeRenderingVolumetricPresentationStateStorage"
        | "VariableModalityLUTSoftcopyPresentationStateStorage"
        | "BasicStructuredDisplayStorage" => RecordType::PRESENTATION,
        "WaveformStorageTrial"
        | "TwelveLeadECGWaveformStorage"
        | "GeneralECGWaveformStorage"
        | "AmbulatoryECGWaveformStorage"
        | "General32bitECGWaveformStorage"
        | "HemodynamicWaveformStorage"
        | "CardiacElectrophysiologyWaveformStorage"
        | "BasicVoiceAudioWaveformStorage"
        | "GeneralAudioWaveformStorage"
        | "ArterialPulseWaveformStorage"
        | "RespiratoryWaveformStorage"
        | "MultichannelRespiratoryWaveformStorage"
        | "RoutineScalpElectroencephalogramWaveformStorage"
        | "ElectromyogramWaveformStorage"
        | "ElectrooculogramWaveformStorage"
        | "SleepElectroencephalogramWaveformStorage"
        | "BodyPositionWaveformStorage"
        | "UltrasoundWaveformStorage" => RecordType::WAVEFORM,
        "TextSRStorageTrial"
        | "AudioSRStorageTrial"
        | "DetailSRStorageTrial"
        | "ComprehensiveSRStorageTrial"
        | "BasicTextSRStorage"
        | "EnhancedSRStorage"
        | "ComprehensiveSRStorage"
        | "Comprehensive3DSRStorage"
        | "ExtensibleSRStorage"
        | "ProcedureLogStorage"
        | "MammographyCADSRStorage"
        | "ChestCADSRStorage"
        | "XRayRadiationDoseSRStorage"
        | "RadiopharmaceuticalRadiationDoseSRStorage"
        | "ColonCADSRStorage"
        | "ImplantationPlanSRStorage"
        | "AcquisitionContextSRStorage"
        | "SimplifiedAdultEchoSRStorage"
        | "PatientRadiationDoseSRStorage"
        | "PlannedImagingAgentAdministrationSRStorage"
        | "PerformedImagingAgentAdministrationSRStorage"
        | "EnhancedXRayRadiationDoseSRStorage"
        | "WaveformAnnotationSRStorage" => RecordType::SR_DOCUMENT,
        "KeyObjectSelectionDocumentStorage" => RecordType::KEY_OBJECT_DOCUMENT,
        "MRSpectroscopyStorage" => RecordType::SPECTROSCOPY,
        "RawDataStorage" => RecordType::RAW_DATA,
        "SpatialRegistrationStorage" | "DeformableSpatialRegistrationStorage" => {
            RecordType::REGISTRATION
        }
        "SpatialFiducialsStorage" => RecordType::FIDUCIAL,
        "EncapsulatedPDFStorage"
        | "EncapsulatedCDAStorage"
        | "EncapsulatedSTLStorage"
        | "EncapsulatedOBJStorage"
        | "EncapsulatedMTLStorage" => RecordType::ENCAPSULATED_DOCUMENT,
        "RealWorldValueMappingStorage" => RecordType::VALUE_MAP,
        "StereometricRelationshipStorage" => RecordType::STEREOMETRIC,
        "SurfaceSegmentationStorage" => RecordType::SURFACE,
        "SurfaceScanMeshStorage" | "SurfaceScanPointCloudStorage" => RecordType::SURFACE_SCAN,
        "TractographyResultsStorage" => RecordType::TRACT,
        "MicroscopyBulkSimpleAnnotationsStorage" => RecordType::ANNOTATION,
        "WaveformPresentationStateStorage" | "WaveformAcquisitionPresentationStateStorage" => {
            RecordType::WAVEFORM_PRESENTATION
        }
        _ => return None,
    };
    Some(t)
}
pub(crate) fn root_file_record_type(typ: &str) -> bool {
    matches!(
        typ,
        "HANGING PROTOCOL" | "PALETTE" | "IMPLANT" | "IMPLANT ASSY" | "IMPLANT GROUP" | "INVENTORY"
    )
}
pub(crate) fn standard_file_record_type(typ: &str) -> bool {
    matches!(
        typ,
        "IMAGE"
            | "RT DOSE"
            | "RT STRUCTURE SET"
            | "RT PLAN"
            | "RT TREAT RECORD"
            | "PRESENTATION"
            | "WAVEFORM"
            | "SR DOCUMENT"
            | "KEY OBJECT DOC"
            | "SPECTROSCOPY"
            | "RAW DATA"
            | "REGISTRATION"
            | "FIDUCIAL"
            | "ENCAP DOC"
            | "VALUE MAP"
            | "STEREOMETRIC"
            | "SURFACE"
            | "SURFACE SCAN"
            | "TRACT"
            | "ANNOTATION"
            | "WF PRESENTATION"
            | "PLAN"
            | "MEASUREMENT"
            | "ASSESSMENT"
            | "RADIOTHERAPY"
    ) || root_file_record_type(typ)
}
