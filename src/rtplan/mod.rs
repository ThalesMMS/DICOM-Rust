#![forbid(unsafe_code)]
mod aperture;
mod bounded;
mod brachy;
mod geometry;
mod ion;
mod photon;
mod read;
mod records;
mod records_write;
pub mod reflect;
mod rtimage;
mod summary;
mod tags;
mod validate;
mod write;
pub use aperture::{
    APERTURE_COORDINATES, ApertureGeometry, FractionMeterset, JawGeometry, LeafOpening,
};
pub use bounded::{
    Limits, ReadOptions, default_limits, read, read_dataset, read_dataset_with_options,
    read_with_options,
};
pub use brachy::write_plan;
pub use geometry::GantryArc;
pub use records::*;
pub use records_write::write_treatment_record;
use reflect::Node;
pub use reflect::{Bytes, NilFloats, Reflect};
pub use rtimage::{Image, ImageExposure, read_image, read_image_dataset, write_image};
use std::fmt;
pub use summary::{
    BeamSummary, FractionGroupSummary, InspectionPage, InspectionRow, InspectionTarget,
    InspectionValue, IonBeamSummary, Summary,
};
pub use write::WriteOptions;
pub const RT_PLAN_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.5";
pub const RT_ION_PLAN_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.8";
pub const RT_BEAMS_TREATMENT_RECORD_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.4";
pub const RT_BRACHY_TREATMENT_RECORD_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.6";
pub const RT_TREATMENT_SUMMARY_RECORD_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.7";
pub const RT_ION_BEAMS_TREATMENT_RECORD_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.9";
pub const RT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.1";
pub const GEOMETRY_PATIENT: &str = "PATIENT";
pub const GEOMETRY_TREATMENT_DEVICE: &str = "TREATMENT_DEVICE";
pub const BEAM_TYPE_STATIC: &str = "STATIC";
pub const BEAM_TYPE_DYNAMIC: &str = "DYNAMIC";
pub const ROTATION_CLOCKWISE: &str = "CW";
pub const ROTATION_COUNTER_CLOCKWISE: &str = "CC";
pub const ROTATION_NONE: &str = "NONE";
pub const DEVICE_X: &str = "X";
pub const DEVICE_Y: &str = "Y";
pub const DEVICE_ASYM_X: &str = "ASYMX";
pub const DEVICE_ASYM_Y: &str = "ASYMY";
pub const DEVICE_MLC_X: &str = "MLCX";
pub const DEVICE_MLC_Y: &str = "MLCY";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    UnsupportedSopClass,
    InvalidObject,
    ResourceLimit,
    Unmaterialized,
    UnsupportedFeature,
    NoAperture,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::UnsupportedSopClass => "dicom/rtplan: unsupported SOP class",
            ErrorKind::InvalidObject => "dicom/rtplan: invalid object",
            ErrorKind::ResourceLimit => "dicom/rtplan: resource limit exceeded",
            ErrorKind::Unmaterialized => "dicom/rtplan: metadata must be materialized",
            ErrorKind::UnsupportedFeature => "dicom/rtplan: unsupported feature",
            ErrorKind::NoAperture => "dicom/rtplan: no MLC aperture",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub text: String,
    pub validation: bool,
    pub problems: Vec<Problem>,
}
impl Error {
    pub fn bare(kind: ErrorKind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
            validation: false,
            problems: Vec::new(),
        }
    }
    pub fn validation(problems: Vec<Problem>) -> Error {
        let kind = ErrorKind::InvalidObject;
        let text = if problems.is_empty() {
            kind.text().to_string()
        } else {
            let parts: Vec<String> = problems.iter().map(Problem::to_string).collect();
            format!("{}: {}", kind.text(), parts.join("; "))
        };
        Error {
            kind,
            text,
            validation: true,
            problems,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) fn path_error(kind: ErrorKind, path: &str, reason: &str) -> Error {
    Error {
        kind,
        text: format!("{}: {path}: {reason}", kind.text()),
        validation: false,
        problems: Vec::new(),
    }
}
pub(crate) fn finite(v: f64) -> bool {
    v.is_finite()
}
pub(crate) fn one_of(value: &str, candidates: &[&str]) -> bool {
    candidates.contains(&value)
}
macro_rules! model {
    ($(#[$m:meta])* $name:ident $go:literal { $($(#[$fm:meta])* $field:ident : $ty:ty = $gname:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name { $($(#[$fm])* pub $field: $ty),* }
        impl $crate::rtplan::reflect::Reflect for $name {
            fn node(&self) -> $crate::rtplan::reflect::Node<'_> {
                $crate::rtplan::reflect::Node::Struct($go, vec![$(($gname, &self.$field as &dyn $crate::rtplan::reflect::Reflect)),*])
            }
            fn type_name() -> &'static str { $go }
        }
    };
}
pub(crate) use model;
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute<T> {
    pub present: bool,
    pub value: Option<T>,
}
impl<T> Default for Attribute<T> {
    fn default() -> Self {
        Attribute {
            present: false,
            value: None,
        }
    }
}
impl<T> Attribute<T> {
    pub fn of(v: T) -> Attribute<T> {
        Attribute {
            present: true,
            value: Some(v),
        }
    }
}
impl<T: Reflect> Reflect for Attribute<T> {
    fn node(&self) -> Node<'_> {
        Node::Struct(
            "Attribute[T]",
            vec![("Present", &self.present), ("Value", &self.value)],
        )
    }
    fn type_name() -> &'static str {
        "Attribute[T]"
    }
}
impl Reflect for crate::pixel::Metadata {
    fn node(&self) -> Node<'_> {
        Node::Struct(
            "Metadata",
            vec![
                ("Rows", &self.rows),
                ("Columns", &self.columns),
                ("SamplesPerPixel", &self.samples_per_pixel),
                ("BitsAllocated", &self.bits_allocated),
                ("BitsStored", &self.bits_stored),
                ("HighBit", &self.high_bit),
                ("PixelRepresentation", &self.pixel_representation),
                ("PlanarConfiguration", &self.planar_configuration),
                (
                    "PlanarConfigurationPresent",
                    &self.planar_configuration_present,
                ),
                ("NumberOfFrames", &self.number_of_frames),
                (
                    "PhotometricInterpretation",
                    &self.photometric_interpretation,
                ),
            ],
        )
    }
    fn type_name() -> &'static str {
        "Metadata"
    }
}
model! {
    Problem "Problem" {
        path: String = "Path",
        message: String = "Message",
    }
}
impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}
model! {
    Reference "Reference" {
        sop_class_uid: String = "SOPClassUID",
        sop_instance_uid: String = "SOPInstanceUID",
    }
}
model! {
    PlanReference "PlanReference" {
        reference: Reference = "Reference",
        relationship: String = "Relationship",
    }
}
model! {
    Plan "Plan" {
        model_bytes: i64 = "ModelBytes",
        sop_class_uid: String = "SOPClassUID",
        sop_instance_uid: String = "SOPInstanceUID",
        study_instance_uid: String = "StudyInstanceUID",
        series_instance_uid: String = "SeriesInstanceUID",
        frame_of_reference_uid: String = "FrameOfReferenceUID",
        label: String = "Label",
        name: String = "Name",
        description: String = "Description",
        date: String = "Date",
        time: String = "Time",
        intent: String = "Intent",
        geometry: String = "Geometry",
        approval_status: String = "ApprovalStatus",
        referenced_structure_set: Option<Reference> = "ReferencedStructureSet",
        referenced_doses: Vec<Reference> = "ReferencedDoses",
        referenced_plans: Vec<PlanReference> = "ReferencedPlans",
        prescription_description: String = "PrescriptionDescription",
        dose_references: Vec<DoseReference> = "DoseReferences",
        tolerance_tables: Vec<ToleranceTable> = "ToleranceTables",
        patient_setups: Vec<PatientSetup> = "PatientSetups",
        fraction_groups: Vec<FractionGroup> = "FractionGroups",
        beams: Vec<Beam> = "Beams",
        ion_beams: Vec<IonBeam> = "IonBeams",
        sources: Vec<BrachySource> = "Sources",
        brachy_treatment_technique: String = "BrachyTreatmentTechnique",
        brachy_treatment_type: String = "BrachyTreatmentType",
        brachy_setups: Vec<BrachySetup> = "BrachySetups",
        warnings: Vec<Problem> = "Warnings",
    }
}
model! {
    DoseReference "DoseReference" {
        number: i64 = "Number",
        uid: String = "UID",
        structure_type: String = "StructureType",
        description: String = "Description",
        typ: String = "Type",
        referenced_roi_number: Option<i64> = "ReferencedROINumber",
        point_coordinates: Vec<f64> = "PointCoordinates",
        delivery_maximum_dose: Option<f64> = "DeliveryMaximumDose",
        target_minimum_dose: Option<f64> = "TargetMinimumDose",
        target_prescription_dose: Option<f64> = "TargetPrescriptionDose",
        target_maximum_dose: Option<f64> = "TargetMaximumDose",
        organ_at_risk_maximum_dose: Option<f64> = "OrganAtRiskMaximumDose",
    }
}
model! {
    DeviceTolerance "DeviceTolerance" {
        device_type: String = "DeviceType",
        position: f64 = "Position",
    }
}
model! {
    ToleranceTable "ToleranceTable" {
        number: i64 = "Number",
        label: String = "Label",
        gantry_angle: Option<f64> = "GantryAngle",
        beam_limiting_device_angle: Option<f64> = "BeamLimitingDeviceAngle",
        patient_support_angle: Option<f64> = "PatientSupportAngle",
        table_top_eccentric_angle: Option<f64> = "TableTopEccentricAngle",
        table_top_vertical_position: Option<f64> = "TableTopVerticalPosition",
        table_top_longitudinal_position: Option<f64> = "TableTopLongitudinalPosition",
        table_top_lateral_position: Option<f64> = "TableTopLateralPosition",
        beam_limiting_device_position_limit: Vec<DeviceTolerance> = "BeamLimitingDevicePositionLimit",
    }
}
model! {
    PatientSetup "PatientSetup" {
        number: i64 = "Number",
        label: String = "Label",
        patient_position: String = "PatientPosition",
        patient_additional_position: String = "PatientAdditionalPosition",
        setup_technique: String = "SetupTechnique",
    }
}
model! {
    ReferencedBeam "ReferencedBeam" {
        beam_number: i64 = "BeamNumber",
        dose: Option<f64> = "Dose",
        meterset: Option<f64> = "Meterset",
    }
}
model! {
    ReferencedBrachySetup "ReferencedBrachySetup" {
        setup_number: i64 = "SetupNumber",
        dose: Option<f64> = "Dose",
    }
}
model! {
    ReferencedDoseReference "ReferencedDoseReference" {
        number: i64 = "Number",
        constraint_weight: Option<f64> = "ConstraintWeight",
        delivery_maximum_dose: Option<f64> = "DeliveryMaximumDose",
        target_prescription_dose: Option<f64> = "TargetPrescriptionDose",
    }
}
model! {
    FractionGroup "FractionGroup" {
        number: i64 = "Number",
        description: String = "Description",
        fractions_planned: Option<i64> = "FractionsPlanned",
        fraction_pattern_digits_day: Option<i64> = "FractionPatternDigitsDay",
        repeat_fraction_cycle_length: Option<i64> = "RepeatFractionCycleLength",
        fraction_pattern: String = "FractionPattern",
        beam_dose_meaning: String = "BeamDoseMeaning",
        number_of_beams: i64 = "NumberOfBeams",
        number_of_brachy_setups: i64 = "NumberOfBrachySetups",
        referenced_beams: Vec<ReferencedBeam> = "ReferencedBeams",
        referenced_brachy_setups: Vec<ReferencedBrachySetup> = "ReferencedBrachySetups",
        referenced_dose_reference: Vec<ReferencedDoseReference> = "ReferencedDoseReference",
    }
}
model! {
    BeamLimitingDevice "BeamLimitingDevice" {
        typ: String = "Type",
        source_distance: Option<f64> = "SourceDistance",
        pairs: i64 = "Pairs",
        leaf_boundaries: NilFloats = "LeafBoundaries",
    }
}
impl BeamLimitingDevice {
    pub fn is_mlc(&self) -> bool {
        self.typ == DEVICE_MLC_X || self.typ == DEVICE_MLC_Y
    }
}
model! {
    Wedge "Wedge" {
        number: i64 = "Number",
        typ: String = "Type",
        id: String = "ID",
        angle: f64 = "Angle",
        factor: Option<f64> = "Factor",
        orientation: f64 = "Orientation",
    }
}
model! {
    Block "Block" {
        number: i64 = "Number",
        typ: String = "Type",
        name: String = "Name",
        divergence: String = "Divergence",
        number_of_points: i64 = "NumberOfPoints",
        contour: Vec<[f64; 2]> = "Contour",
        material_id: String = "MaterialID",
        thickness: Option<f64> = "Thickness",
        transmission: Option<f64> = "Transmission",
    }
}
model! {
    Applicator "Applicator" {
        id: String = "ID",
        typ: String = "Type",
    }
}
model! {
    DevicePosition "DevicePosition" {
        device_type: String = "DeviceType",
        positions: Vec<f64> = "Positions",
    }
}
model! {
    WedgePosition "WedgePosition" {
        wedge_number: i64 = "WedgeNumber",
        position: String = "Position",
    }
}
model! {
    ControlPointDoseReference "ControlPointDoseReference" {
        number: i64 = "Number",
        coefficient: Option<f64> = "Coefficient",
    }
}
pub type Field = u32;
pub const FIELD_NOMINAL_BEAM_ENERGY: Field = 1 << 0;
pub const FIELD_DOSE_RATE_SET: Field = 1 << 1;
pub const FIELD_GANTRY_ANGLE: Field = 1 << 2;
pub const FIELD_GANTRY_ROTATION_DIRECTION: Field = 1 << 3;
pub const FIELD_BEAM_LIMITING_DEVICE_ANGLE: Field = 1 << 4;
pub const FIELD_BEAM_LIMITING_DEVICE_ROTATION_DIRECTION: Field = 1 << 5;
pub const FIELD_PATIENT_SUPPORT_ANGLE: Field = 1 << 6;
pub const FIELD_PATIENT_SUPPORT_ROTATION_DIRECTION: Field = 1 << 7;
pub const FIELD_TABLE_TOP_ECCENTRIC_ANGLE: Field = 1 << 8;
pub const FIELD_TABLE_TOP_ECCENTRIC_ROTATION_DIRECTION: Field = 1 << 9;
pub const FIELD_TABLE_TOP_VERTICAL_POSITION: Field = 1 << 10;
pub const FIELD_TABLE_TOP_LONGITUDINAL_POSITION: Field = 1 << 11;
pub const FIELD_TABLE_TOP_LATERAL_POSITION: Field = 1 << 12;
pub const FIELD_ISOCENTER_POSITION: Field = 1 << 13;
pub const FIELD_DEVICE_POSITIONS: Field = 1 << 14;
pub const FIELD_TABLE_TOP_PITCH_ANGLE: Field = 1 << 15;
pub const FIELD_TABLE_TOP_ROLL_ANGLE: Field = 1 << 16;
model! {
    ControlPoint "ControlPoint" {
        index: i64 = "Index",
        cumulative_meterset_weight: Option<f64> = "CumulativeMetersetWeight",
        nominal_beam_energy: Option<f64> = "NominalBeamEnergy",
        dose_rate_set: Option<f64> = "DoseRateSet",
        gantry_angle: f64 = "GantryAngle",
        gantry_rotation_direction: String = "GantryRotationDirection",
        beam_limiting_device_angle: f64 = "BeamLimitingDeviceAngle",
        beam_limiting_device_rotation_direction: String = "BeamLimitingDeviceRotationDirection",
        patient_support_angle: f64 = "PatientSupportAngle",
        patient_support_rotation_direction: String = "PatientSupportRotationDirection",
        table_top_eccentric_angle: f64 = "TableTopEccentricAngle",
        table_top_eccentric_rotation_direction: String = "TableTopEccentricRotationDirection",
        table_top_pitch_angle: Option<f64> = "TableTopPitchAngle",
        table_top_roll_angle: Option<f64> = "TableTopRollAngle",
        table_top_vertical_position: Option<f64> = "TableTopVerticalPosition",
        table_top_longitudinal_position: Option<f64> = "TableTopLongitudinalPosition",
        table_top_lateral_position: Option<f64> = "TableTopLateralPosition",
        isocenter_position: Vec<f64> = "IsocenterPosition",
        source_to_surface_distance: Option<f64> = "SourceToSurfaceDistance",
        device_positions: Vec<DevicePosition> = "DevicePositions",
        encoded_device_types: Vec<String> = "EncodedDeviceTypes",
        wedge_positions: Vec<WedgePosition> = "WedgePositions",
        dose_references: Vec<ControlPointDoseReference> = "DoseReferences",
        encoded: Field = "Encoded",
    }
}
impl ControlPoint {
    pub fn encodes(&self, field: Field) -> bool {
        self.encoded & field != 0
    }
    pub fn device(&self, device_type: &str) -> Option<&DevicePosition> {
        self.device_positions
            .iter()
            .find(|d| d.device_type == device_type)
    }
}
model! {
    Beam "Beam" {
        number: i64 = "Number",
        name: String = "Name",
        description: String = "Description",
        typ: String = "Type",
        radiation_type: String = "RadiationType",
        high_dose_technique_type: String = "HighDoseTechniqueType",
        treatment_machine_name: String = "TreatmentMachineName",
        manufacturer: String = "Manufacturer",
        primary_dosimeter_unit: String = "PrimaryDosimeterUnit",
        source_axis_distance: Option<f64> = "SourceAxisDistance",
        treatment_delivery_type: String = "TreatmentDeliveryType",
        referenced_patient_setup_number: Option<i64> = "ReferencedPatientSetupNumber",
        referenced_tolerance_table_number: Option<i64> = "ReferencedToleranceTableNumber",
        devices: Vec<BeamLimitingDevice> = "Devices",
        number_of_wedges: i64 = "NumberOfWedges",
        number_of_compensators: i64 = "NumberOfCompensators",
        number_of_boli: i64 = "NumberOfBoli",
        number_of_blocks: i64 = "NumberOfBlocks",
        wedges: Vec<Wedge> = "Wedges",
        blocks: Vec<Block> = "Blocks",
        applicators: Vec<Applicator> = "Applicators",
        final_cumulative_meterset_weight: Option<f64> = "FinalCumulativeMetersetWeight",
        control_points: Vec<ControlPoint> = "ControlPoints",
    }
}
impl Beam {
    pub fn device(&self, device_type: &str) -> Option<&BeamLimitingDevice> {
        self.devices.iter().find(|d| d.typ == device_type)
    }
    pub fn mlc(&self) -> Option<&BeamLimitingDevice> {
        self.devices.iter().find(|d| d.is_mlc())
    }
}
model! {
    BrachySetup "BrachySetup" {
        number: i64 = "Number",
        typ: String = "Type",
        name: String = "Name",
        total_reference_air_kerma: Option<f64> = "TotalReferenceAirKerma",
        number_of_channels: i64 = "NumberOfChannels",
        referenced_source_numbers: Vec<i64> = "ReferencedSourceNumbers",
        channels: Vec<BrachyChannel> = "Channels",
    }
}
model! {
    BrachySource "BrachySource" {
        number: i64 = "Number",
        typ: String = "Type",
        serial_number: String = "SerialNumber",
        isotope_name: String = "IsotopeName",
        strength_units: String = "StrengthUnits",
        isotope_half_life: Attribute<f64> = "IsotopeHalfLife",
        reference_air_kerma_rate: Attribute<f64> = "ReferenceAirKermaRate",
        strength: Attribute<f64> = "Strength",
        strength_reference_date: String = "StrengthReferenceDate",
        strength_reference_time: String = "StrengthReferenceTime",
    }
}
model! {
    BrachyChannel "BrachyChannel" {
        number: i64 = "Number",
        source_number: i64 = "SourceNumber",
        movement: String = "Movement",
        length: Attribute<f64> = "Length",
        total_time: Attribute<f64> = "TotalTime",
        step_size: Attribute<f64> = "StepSize",
        pulse_interval: Attribute<f64> = "PulseInterval",
        number_of_pulses: Attribute<i64> = "NumberOfPulses",
        transfer_tube_number: Attribute<i64> = "TransferTubeNumber",
        transfer_tube_length: Attribute<f64> = "TransferTubeLength",
        final_cumulative_time_weight: Attribute<f64> = "FinalCumulativeTimeWeight",
        control_points: Vec<BrachyControlPoint> = "ControlPoints",
    }
}
model! {
    BrachyControlPoint "BrachyControlPoint" {
        index: i64 = "Index",
        cumulative_time_weight: Attribute<f64> = "CumulativeTimeWeight",
        relative_position: f64 = "RelativePosition",
        position_3d: Vec<f64> = "Position3D",
        orientation: Vec<f64> = "Orientation",
        position_3d_present: bool = "Position3DPresent",
        orientation_present: bool = "OrientationPresent",
    }
}
model! {
    IonBeam "IonBeam" {
        number: i64 = "Number",
        name: String = "Name",
        description: String = "Description",
        typ: String = "Type",
        radiation_type: String = "RadiationType",
        scan_mode: String = "ScanMode",
        treatment_machine_name: String = "TreatmentMachineName",
        primary_dosimeter_unit: String = "PrimaryDosimeterUnit",
        treatment_delivery_type: String = "TreatmentDeliveryType",
        patient_support_type: String = "PatientSupportType",
        atomic_number: Attribute<i64> = "AtomicNumber",
        mass_number: Attribute<i64> = "MassNumber",
        charge_state: Attribute<i64> = "ChargeState",
        referenced_patient_setup_number: Option<i64> = "ReferencedPatientSetupNumber",
        referenced_tolerance_table_number: Option<i64> = "ReferencedToleranceTableNumber",
        snout_id: String = "SnoutID",
        virtual_source_axis_distances: Vec<f64> = "VirtualSourceAxisDistances",
        devices: Vec<BeamLimitingDevice> = "Devices",
        range_shifters: Vec<RangeShifter> = "RangeShifters",
        final_cumulative_meterset_weight: Attribute<f64> = "FinalCumulativeMetersetWeight",
        control_points: Vec<IonControlPoint> = "ControlPoints",
    }
}
model! {
    RangeShifter "RangeShifter" {
        number: i64 = "Number",
        id: String = "ID",
        typ: String = "Type",
        description: String = "Description",
    }
}
model! {
    RangeShifterSetting "RangeShifterSetting" {
        number: i64 = "Number",
        setting: String = "Setting",
        distance: Attribute<f64> = "Distance",
        water_equivalent_thickness: Attribute<f64> = "WaterEquivalentThickness",
    }
}
model! {
    IonControlPoint "IonControlPoint" {
        control_point: ControlPoint = "ControlPoint",
        snout_position: Attribute<f64> = "SnoutPosition",
        gantry_pitch_angle: Attribute<f64> = "GantryPitchAngle",
        meterset_rate: Attribute<f64> = "MetersetRate",
        encoded_snout_position: bool = "EncodedSnoutPosition",
        encoded_gantry_pitch_angle: bool = "EncodedGantryPitchAngle",
        gantry_pitch_rotation_direction: String = "GantryPitchRotationDirection",
        table_top_pitch_rotation_direction: String = "TableTopPitchRotationDirection",
        table_top_roll_rotation_direction: String = "TableTopRollRotationDirection",
        range_shifter_settings: Vec<RangeShifterSetting> = "RangeShifterSettings",
        encoded_range_shifter_settings: bool = "EncodedRangeShifterSettings",
        scan_spot_tune_id: String = "ScanSpotTuneID",
        spot_positions: Vec<f32> = "SpotPositions",
        spot_weights: Vec<f32> = "SpotWeights",
        number_of_paintings: Attribute<i64> = "NumberOfPaintings",
    }
}
impl Plan {
    pub fn beam(&self, number: i64) -> Option<&Beam> {
        self.beams.iter().find(|b| b.number == number)
    }
}
